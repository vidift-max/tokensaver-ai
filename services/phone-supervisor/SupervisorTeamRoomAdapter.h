#pragma once
#include <string>
#include <vector>

namespace phone_supervisor {

// Deliberately transport-free contract. No token/secret is accepted or stored here.
struct TeamRoomAttachment { std::string attachment_id; };
struct TeamRoomMessage { std::string message_id; std::string payload; };

class SupervisorTeamRoomAdapter {
public:
    virtual ~SupervisorTeamRoomAdapter() = default;
    virtual bool authenticate(const std::string& test_workload_assertion) = 0;
    virtual bool attach(const std::string& provisioned_phone_binding, TeamRoomAttachment& out) = 0;
    virtual bool heartbeat(const TeamRoomAttachment&) = 0;
    virtual bool reconnect(const TeamRoomAttachment&, const std::string& fresh_challenge) = 0;
    virtual bool detach(const TeamRoomAttachment&) = 0;
    virtual bool send(const TeamRoomAttachment&, const std::string& target_phone_id,
                      const std::string& envelope, const std::string& idempotency_key) = 0;
    virtual bool receive(const TeamRoomAttachment&, std::vector<TeamRoomMessage>& out) = 0;
    virtual bool ack(const TeamRoomAttachment&, const std::string& message_id) = 0;
    virtual bool revoke(const TeamRoomAttachment&) = 0;
};

// Deterministic in-memory fake. It has no socket, HTTP, credential-store or filesystem code.
class FakeSupervisorTeamRoomAdapter final : public SupervisorTeamRoomAdapter {
public:
    bool authenticate(const std::string& assertion) override { return assertion == "TEST-ONLY"; }
    bool attach(const std::string& binding, TeamRoomAttachment& out) override {
        if (binding != "TEST-PROVISIONED-PHONE") return false;
        out.attachment_id = "fake-attachment-1"; attached_ = true; return true;
    }
    bool heartbeat(const TeamRoomAttachment& a) override { return attached_ && a.attachment_id == "fake-attachment-1"; }
    bool reconnect(const TeamRoomAttachment& a, const std::string& challenge) override {
        return attached_ && a.attachment_id == "fake-attachment-1" && !challenge.empty();
    }
    bool detach(const TeamRoomAttachment& a) override {
        if (!attached_ || a.attachment_id != "fake-attachment-1") return false;
        attached_ = false; return true;
    }
    bool send(const TeamRoomAttachment& a, const std::string& target, const std::string& envelope,
              const std::string& key) override {
        return attached_ && a.attachment_id == "fake-attachment-1" && !target.empty() &&
               !envelope.empty() && !key.empty();
    }
    bool receive(const TeamRoomAttachment& a, std::vector<TeamRoomMessage>& out) override {
        if (!attached_ || a.attachment_id != "fake-attachment-1") return false;
        out.clear(); return true;
    }
    bool ack(const TeamRoomAttachment& a, const std::string& id) override {
        return attached_ && a.attachment_id == "fake-attachment-1" && !id.empty();
    }
    bool revoke(const TeamRoomAttachment& a) override { return detach(a); }
private:
    bool attached_ = false;
};

} // namespace phone_supervisor
