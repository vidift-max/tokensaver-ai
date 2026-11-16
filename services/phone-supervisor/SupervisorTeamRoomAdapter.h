#pragma once
#include <cstdint>
#include <string>
#include <vector>
namespace phone_supervisor {
struct ProvisionedPhoneBinding {
 std::string ai_phone_id, endpoint_id, package_family, package_full_name, appcontainer_sid;
 std::uint64_t install_generation = 0;
};
struct TeamRoomAttachment { std::string attachment_id, sender_ai_phone_id, team_room_session_id, execution_id, endpoint_id; std::uint64_t endpoint_fence = 0; };
struct TeamRoomMessage { std::string message_id, sender_ai_phone_id, payload; };
class SupervisorTeamRoomAdapter {
public:
 virtual ~SupervisorTeamRoomAdapter() = default;
 virtual bool authenticate(const std::string& workload_assertion) = 0;
 virtual bool attach(const ProvisionedPhoneBinding&, TeamRoomAttachment&) = 0;
 virtual bool heartbeat(const TeamRoomAttachment&) = 0;
 virtual bool reconnect(const TeamRoomAttachment&, const std::string& fresh_challenge) = 0;
 virtual bool detach(const TeamRoomAttachment&) = 0;
 virtual bool send(const TeamRoomAttachment&, const std::string& target_phone_id,
  const std::string& envelope, const std::string& idempotency_key, TeamRoomMessage& receipt) = 0;
 virtual bool receive(const TeamRoomAttachment&, std::vector<TeamRoomMessage>& out) = 0;
 virtual bool ack(const TeamRoomAttachment&, const std::string& message_id) = 0;
 virtual bool revoke(const TeamRoomAttachment&) = 0;
};
class FakeSupervisorTeamRoomAdapter final : public SupervisorTeamRoomAdapter {
public:
 bool authenticate(const std::string& assertion) override {
  if(assertion!="P2.37-TEST-ONLY-ASSERTION")return false;
  authenticated_=true;session_token_="FAKE-SESSION-TOKEN-NOT-FOR-EXPORT";return true;
 }
 bool attach(const ProvisionedPhoneBinding& b,TeamRoomAttachment& out) override {
  if(!authenticated_||b.ai_phone_id!="TEST-AI-PHONE-02"||b.endpoint_id!="TEST-ENDPOINT-02"||
    b.package_family.empty()||b.package_full_name.empty()||b.appcontainer_sid.empty()||b.install_generation!=1)return false;
  endpoint_credential_="FAKE-ENDPOINT-CREDENTIAL-NOT-FOR-EXPORT";
  attachment_={"fake-attachment-p2-37",b.ai_phone_id,"FAKE-TEAM-ROOM-SESSION-P2-37","FAKE-EXECUTION-P2-37",b.endpoint_id,1};attached_=true;
  inbox_={{"fake-inbound-1","FAKE-REMOTE-SENDER","fixture-payload"}};
  out=attachment_;return true;
 }
 bool heartbeat(const TeamRoomAttachment& a) override {return authenticated_&&attached_&&same(a)&&!endpoint_credential_.empty();}
 bool reconnect(const TeamRoomAttachment& a,const std::string& c) override {
  if(!authenticated_||!attached_||!same(a)||c!="P2.37-FRESH-CHALLENGE")return false;
  ++reconnect_generation_;return true;
 }
 bool detach(const TeamRoomAttachment& a) override {
  if(!authenticated_||!attached_||!same(a))return false;
  attached_=false;endpoint_credential_.clear();return true;
 }
 bool send(const TeamRoomAttachment& a,const std::string& target,const std::string& envelope,
  const std::string& key,TeamRoomMessage& receipt) override {
  if(!authenticated_||!attached_||!same(a)||target.empty()||envelope.empty()||key.empty()||!pair_granted_)return false;
  receipt={"fake-outbound-1",attachment_.sender_ai_phone_id,envelope};sent_.push_back(receipt);return true;
 }
 bool receive(const TeamRoomAttachment& a,std::vector<TeamRoomMessage>& out) override {
  if(!authenticated_||!attached_||!same(a))return false;out=inbox_;return true;
 }
 bool ack(const TeamRoomAttachment& a,const std::string& id) override {
  if(!authenticated_||!attached_||!same(a)||id.empty())return false;
  for(auto it=inbox_.begin();it!=inbox_.end();++it)if(it->message_id==id){inbox_.erase(it);acknowledged_.push_back(id);return true;}
  return false;
 }
 bool revoke(const TeamRoomAttachment& a) override {
  if(!same(a))return false;attached_=false;authenticated_=false;endpoint_credential_.clear();
  session_token_.clear();pair_granted_=false;return true;
 }
 bool grantPairForTest(const std::string& s,const std::string& r) {
  pair_granted_=s=="TEST-AI-PHONE-02"&&r=="FAKE-RECIPIENT-PHONE";return pair_granted_;
 }
 bool credentialsConfined() const {
  return authenticated_&&!session_token_.empty()&&(!attached_||!endpoint_credential_.empty());
 }
private:
 bool same(const TeamRoomAttachment& a) const {
  return a.attachment_id==attachment_.attachment_id&&a.sender_ai_phone_id==attachment_.sender_ai_phone_id&&a.team_room_session_id==attachment_.team_room_session_id&&a.execution_id==attachment_.execution_id&&a.endpoint_id==attachment_.endpoint_id&&a.endpoint_fence==attachment_.endpoint_fence;
 }
 bool authenticated_=false,attached_=false,pair_granted_=false;
 unsigned reconnect_generation_=0;
 std::string session_token_,endpoint_credential_;
 TeamRoomAttachment attachment_;
 std::vector<TeamRoomMessage> inbox_,sent_;
 std::vector<std::string> acknowledged_;
};
} // namespace phone_supervisor
