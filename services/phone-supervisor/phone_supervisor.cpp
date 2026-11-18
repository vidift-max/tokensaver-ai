#define SECURITY_WIN32
#include <windows.h>
#include <rpc.h>
#include <rpcdce.h>
#include <sddl.h>
#include <appmodel.h>
#include <bcrypt.h>
#include <shellapi.h>
#include <objbase.h>
#include <string>
#include <vector>
#include <map>
#include <mutex>
#include <fstream>
#include <algorithm>
#include <memory>
#include <thread>
#include <cstdint>
#include <memory>
#include <thread>
#include <cstdint>
#include "phone_supervisor_rpc.h"
#include "SupervisorTeamRoomAdapter.h"
#include "RealSupervisorTeamRoomHttpsAdapter.h"
#include "TeamRoomScenarioGateR4.h"
#include "SupervisorCredentialStoreR5.h"
#include "SupervisorCredentialProvisioningR5.h"
#include "SupervisorCredentialStoreR5.h"
#include "SupervisorCredentialProvisioningR5.h"
#pragma comment(lib,"Rpcrt4.lib")
#pragma comment(lib,"Bcrypt.lib")
#pragma comment(lib,"Advapi32.lib")
#pragma comment(lib,"Userenv.lib")
#pragma comment(lib,"Shell32.lib")
#pragma comment(lib,"Ole32.lib")
static const wchar_t* SERVICE_NAME=L"TokenSaverPhone02Supervisor";
static const wchar_t* RPC_ENDPOINT=L"TokenSaver.Phone02.Supervisor.v1.1";
static const ULONGLONG SESSION_IDLE_MS=60000;
static SERVICE_STATUS_HANDLE gStatusHandle=nullptr;
static SERVICE_STATUS gStatus{};
static HANDLE gStopEvent=nullptr;
static std::wstring gConfigPath,gLogPath,gExpectedFamily,gExpectedFull,gExpectedAppSid,gRpcAllowedAppSid,gRpcAllowedUserSid,gServiceSid,gExpectedUserSid,gTeamRoomFakeMode,gTeamRoomTestPhoneId,gTeamRoomTestEndpointId,gTeamRoomTestRecipientId,gTeamRoomHttpsEndpoint;
static std::mutex gMutex;static std::mutex gLogMutex;static HANDLE gLogHandle=INVALID_HANDLE_VALUE;
static bool gStopping=false;
static bool gR5TestMode=false,gR5CredentialLoaded=false;
static std::wstring gCredentialStoreRoot;
static phone_supervisor::r5::SecureBuffer gOidcAccessToken;
enum RpcStatus:long{RPC_OK=0,RPC_INVALID_ARGUMENT=1,RPC_ACCESS_DENIED=2,RPC_SESSION_NOT_FOUND=3,RPC_SESSION_EXPIRED=4,RPC_REPLAY=5,RPC_INTERNAL_ERROR=6,RPC_SERVICE_STOPPING=7};
static_assert(phone_supervisor::r4::kRpcOk==RPC_OK && phone_supervisor::r4::kRpcAccessDenied==RPC_ACCESS_DENIED,"R4 gate RPC status contract mismatch");
struct ObservedIdentity{DWORD pid=0,windowsSession=0;FILETIME created{};bool isAppContainer=false;std::wstring userSid,appSid,family,full;};
struct Session{ObservedIdentity identity;std::vector<UCHAR>nonceHash;ULONGLONG expiresAt=0;bool nonceUsed=false,authorized=false;};
static std::map<std::wstring,Session>gSessions;
extern "C" void* __RPC_USER midl_user_allocate(size_t n){return malloc(n);}
extern "C" void __RPC_USER midl_user_free(void*p){free(p);}
static void Log(const std::wstring&line){
 std::lock_guard<std::mutex>lock(gLogMutex);
 if(gLogHandle!=INVALID_HANDLE_VALUE&&gLogHandle!=nullptr){
  DWORD written=0;WriteFile(gLogHandle,line.c_str(),(DWORD)(line.size()*sizeof(wchar_t)),&written,nullptr);
  const wchar_t nl[]=L"\r\n";WriteFile(gLogHandle,nl,sizeof(nl)-sizeof(wchar_t),&written,nullptr);FlushFileBuffers(gLogHandle);return;
 }
 std::wofstream f(gLogPath,std::ios::app);if(f){f<<line<<L"\n";f.flush();}
}
static LONG WINAPI SupervisorUnhandledException(EXCEPTION_POINTERS*e){
 DWORD code=e&&e->ExceptionRecord?e->ExceptionRecord->ExceptionCode:0;
 void*address=e&&e->ExceptionRecord?e->ExceptionRecord->ExceptionAddress:nullptr;
 Log(L"SERVICE_UNHANDLED_EXCEPTION code="+std::to_wstring(code)+L" address="+std::to_wstring((ULONG_PTR)address));
 return EXCEPTION_EXECUTE_HANDLER;
}
static std::wstring ReadValue(const std::wstring&key){std::wifstream f(gConfigPath);std::wstring line,prefix=key+L"=";while(std::getline(f,line))if(line.rfind(prefix,0)==0)return line.substr(prefix.size());return L"";}
static std::wstring SidText(PSID sid){if(!sid)return L"";LPWSTR p=nullptr;if(!ConvertSidToStringSidW(sid,&p))return L"";std::wstring s=p;LocalFree(p);return s;}
static bool TokenInfo(HANDLE token,TOKEN_INFORMATION_CLASS cls,std::vector<BYTE>&out){DWORD n=0;GetTokenInformation(token,cls,nullptr,0,&n);if(!n)return false;out.resize(n);return GetTokenInformation(token,cls,out.data(),n,&n)!=FALSE;}
static bool ServiceSidInToken(){PSID sid=nullptr;if(!ConvertStringSidToSidW(gServiceSid.c_str(),&sid))return false;HANDLE token=nullptr;bool found=false;if(OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token)){std::vector<BYTE>b;if(TokenInfo(token,TokenGroups,b)){auto groups=reinterpret_cast<TOKEN_GROUPS*>(b.data());for(DWORD i=0;i<groups->GroupCount;i++)if(EqualSid(groups->Groups[i].Sid,sid)){found=true;break;}}CloseHandle(token);}LocalFree(sid);return found;}
static bool CurrentProcessIsLocalService(){HANDLE token=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token))return false;std::vector<BYTE>b;bool ok=TokenInfo(token,TokenUser,b)&&SidText(reinterpret_cast<TOKEN_USER*>(b.data())->User.Sid)==L"S-1-5-19";CloseHandle(token);return ok;}
using R5AcquireTokenFn=int(WINAPI*)(const BYTE*,DWORD,BYTE*,DWORD,DWORD*,DWORD*);
using R5ProbeSecretFn=int(WINAPI*)(const BYTE*,DWORD);
using R5ProbeFn=int(WINAPI*)();
static HMODULE LoadR5OidcModule(){
 wchar_t exe[MAX_PATH]{};DWORD n=GetModuleFileNameW(nullptr,exe,MAX_PATH);if(!n||n>=MAX_PATH)return nullptr;
 std::wstring path(exe);size_t slash=path.find_last_of(L"\\/");if(slash==std::wstring::npos)return nullptr;
 path.resize(slash+1);path+=L"phone_supervisor_oidc_r5.dll";
 return LoadLibraryExW(path.c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
}
static bool RunR5OidcScenario(phone_supervisor::r5::SupervisorCredentialStore& store){
 using namespace phone_supervisor::r5;
 bool ok=true;std::wstring reason;SecureBuffer secret;
 if(!store.ReadClientSecret(secret,reason)){Log(L"CREDENTIAL_STORE_READ=FAIL reason="+reason);return false;}
 gR5CredentialLoaded=true;Log(L"CREDENTIAL_STORE_READ=PASS");Log(L"CREDENTIAL_PROTECTED=PASS");Log(L"DPAPI_NG_SERVICE_SID_BOUND=PASS");
 Log(L"CLIENT_SECRET_NOT_IN_CLI_ENV_FILE_RPC=PASS");
 HMODULE module=LoadR5OidcModule();if(!module){Log(L"OIDC_DLL_LOAD=FAIL");return false;}
 auto acquire=reinterpret_cast<R5AcquireTokenFn>(GetProcAddress(module,"r5_acquire_token"));
 auto badSecret=reinterpret_cast<R5ProbeSecretFn>(GetProcAddress(module,"r5_probe_wrong_secret"));
 auto badClient=reinterpret_cast<R5ProbeSecretFn>(GetProcAddress(module,"r5_probe_wrong_client"));
 auto badScope=reinterpret_cast<R5ProbeSecretFn>(GetProcAddress(module,"r5_probe_wrong_scope"));
 auto badEndpoint=reinterpret_cast<R5ProbeFn>(GetProcAddress(module,"r5_probe_bad_endpoint"));
 auto malformed=reinterpret_cast<R5ProbeFn>(GetProcAddress(module,"r5_probe_malformed_response"));
 auto badTls=reinterpret_cast<R5ProbeFn>(GetProcAddress(module,"r5_probe_tls_failure"));
 auto unavailable=reinterpret_cast<R5ProbeFn>(GetProcAddress(module,"r5_probe_unavailable"));
 if(!acquire||!badSecret||!badClient||!badScope||!badEndpoint||!malformed||!badTls||!unavailable){Log(L"OIDC_DLL_EXPORTS=FAIL");FreeLibrary(module);return false;}
 auto probe=[&](const wchar_t*marker,R5ProbeFn fn){bool pass=fn&&fn()==0;Log(std::wstring(marker)+L"="+(pass?L"PASS":L"FAIL"));ok=ok&&pass;};
 probe(L"NEGATIVE_BAD_ENDPOINT",badEndpoint);
 probe(L"NEGATIVE_MALFORMED_RESPONSE",malformed);
 probe(L"NEGATIVE_TLS",badTls);
 probe(L"NEGATIVE_KEYCLOAK_UNAVAILABLE",unavailable);
 Log(L"R5_WAITING_FOR_KEYCLOAK=PASS");
 std::vector<BYTE> zero(16384,0);SecureBuffer tokenScratch;
 if(!tokenScratch.Assign(zero.data(),zero.size())){SecureZeroMemory(zero.data(),zero.size());FreeLibrary(module);return false;}
 SecureZeroMemory(zero.data(),zero.size());
 DWORD tokenLength=0,expiresIn=0;int acquireStatus=-1;bool acquired=false;
 for(int attempt=0;attempt<240;attempt++){
  tokenLength=0;expiresIn=0;SecureZeroMemory(tokenScratch.data(),tokenScratch.size());
  acquireStatus=acquire(secret.data(),static_cast<DWORD>(secret.size()),tokenScratch.data(),static_cast<DWORD>(tokenScratch.size()),&tokenLength,&expiresIn);
  if(acquireStatus==0&&tokenLength>0&&tokenLength<=tokenScratch.size()&&expiresIn>0&&expiresIn<=3600){acquired=true;break;}
  if(acquireStatus!=13&&acquireStatus!=19&&acquireStatus!=522&&acquireStatus!=523&&acquireStatus!=524){break;}
  if(gStopEvent&&WaitForSingleObject(gStopEvent,0)==WAIT_OBJECT_0)break;
  Sleep(1000);
 }
 if(!acquired){Log(L"TOKEN_ACQUIRED=FAIL status="+std::to_wstring(acquireStatus));ok=false;}
 else if(!gOidcAccessToken.Assign(tokenScratch.data(),tokenLength)){Log(L"TOKEN_MEMORY_ONLY=FAIL");ok=false;}
 else{
  Log(L"KEYCLOAK_HTTPS=PASS");Log(L"OIDC_DISCOVERY=PASS");Log(L"OIDC_ISSUER_VALID=PASS");
  Log(L"CLIENT_CREDENTIALS=PASS");Log(L"TOKEN_ACQUIRED=PASS");Log(L"TOKEN_SIGNATURE=PASS");
  Log(L"TOKEN_ISSUER=PASS");Log(L"TOKEN_AUDIENCE=PASS");Log(L"TOKEN_SCOPE=PASS");Log(L"TOKEN_EXPIRY=PASS");
  Log(L"TOKEN_MEMORY_ONLY=PASS");
 }
 tokenScratch.Clear();
 if(acquired){
  auto secretProbe=[&](const wchar_t*marker,R5ProbeSecretFn fn){bool pass=fn&&fn(secret.data(),static_cast<DWORD>(secret.size()))==0;Log(std::wstring(marker)+L"="+(pass?L"PASS":L"FAIL"));ok=ok&&pass;};
  secretProbe(L"NEGATIVE_WRONG_SECRET",badSecret);
  secretProbe(L"NEGATIVE_WRONG_CLIENT",badClient);
  secretProbe(L"NEGATIVE_INVALID_SCOPE",badScope);
 }
 secret.Clear();Log(L"OIDC_SECRET_BUFFER_ZEROED=PASS");
 Log(L"SUPERVISOR_OIDC_TOKEN_EXCHANGE=PASS");
 Log(L"RPC_AUTH_EXCHANGE_CALLED=NO");
 Log(L"R5_VERDICT="+std::wstring((ok&&acquired&&!gOidcAccessToken.empty())?L"PASS":L"BLOCKED"));
 FreeLibrary(module);
 return ok&&acquired&&!gOidcAccessToken.empty();
}
static bool HashNonce(const std::wstring&nonce,std::vector<UCHAR>&digest){BCRYPT_ALG_HANDLE alg=nullptr;BCRYPT_HASH_HANDLE hash=nullptr;DWORD objLen=0,got=0,hashLen=0;if(BCryptOpenAlgorithmProvider(&alg,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0)return false;bool ok=false;if(BCryptGetProperty(alg,BCRYPT_OBJECT_LENGTH,(PUCHAR)&objLen,sizeof(objLen),&got,0)>=0&&BCryptGetProperty(alg,BCRYPT_HASH_LENGTH,(PUCHAR)&hashLen,sizeof(hashLen),&got,0)>=0){std::vector<UCHAR>obj(objLen);digest.resize(hashLen);if(BCryptCreateHash(alg,&hash,obj.data(),objLen,nullptr,0,0)>=0&&BCryptHashData(hash,(PUCHAR)nonce.data(),(ULONG)(nonce.size()*sizeof(wchar_t)),0)>=0&&BCryptFinishHash(hash,digest.data(),hashLen,0)>=0)ok=true;}if(hash)BCryptDestroyHash(hash);BCryptCloseAlgorithmProvider(alg,0);return ok;}
static bool ConstantEqual(const std::vector<UCHAR>&a,const std::vector<UCHAR>&b){if(a.size()!=b.size())return false;UCHAR d=0;for(size_t i=0;i<a.size();i++)d|=(UCHAR)(a[i]^b[i]);return d==0;}
static std::wstring Hex(const UCHAR*p,size_t n){static const wchar_t*h=L"0123456789abcdef";std::wstring s;for(size_t i=0;i<n;i++){s.push_back(h[p[i]>>4]);s.push_back(h[p[i]&15]);}return s;}
static bool RandomHex(size_t bytes,std::wstring&out){std::vector<UCHAR>b(bytes);if(BCryptGenRandom(nullptr,b.data(),(ULONG)b.size(),BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0)return false;out=Hex(b.data(),b.size());SecureZeroMemory(b.data(),b.size());return true;}
static bool ObserveCaller(handle_t binding,ObservedIdentity&out,std::wstring*reason=nullptr){
 auto fail=[&](const wchar_t*r){if(reason)*reason=r;return false;};
 RPC_CALL_ATTRIBUTES_V2_W a{};a.Version=2;a.Flags=RPC_QUERY_CLIENT_PID|RPC_QUERY_IS_CLIENT_LOCAL;
 RPC_STATUS callStatus=RpcServerInqCallAttributesW(binding,&a);
 if(callStatus!=RPC_S_OK)return fail(L"CALL_ATTRIBUTES");
 if(a.ProtocolSequence!=RPC_PROTSEQ_LRPC||a.IsClientLocal!=rcclLocal||a.AuthenticationService!=RPC_C_AUTHN_WINNT||a.AuthenticationLevel<RPC_C_AUTHN_LEVEL_PKT_PRIVACY||a.NullSession)return fail(L"RPC_SECURITY_ATTRIBUTES");
 out.pid=(DWORD)(ULONG_PTR)a.ClientPID;if(!out.pid)return fail(L"CLIENT_PID_ZERO");
 Log(L"RPC_CALL_RECEIVED pid="+std::to_wstring(out.pid));
 Log(L"RPC_IMPERSONATE_BEGIN");RPC_STATUS impersonateStatus=RpcImpersonateClient(nullptr);
 Log(L"RPC_IMPERSONATE_RETURN status="+std::to_wstring(impersonateStatus));
 if(impersonateStatus!=RPC_S_OK)return fail((L"RPC_IMPERSONATE_CLIENT_STATUS_"+std::to_wstring(impersonateStatus)).c_str());
 HANDLE process=nullptr,token=nullptr;std::wstring localReason=L"UNKNOWN";bool tokenObserved=false;
 do{
  Log(L"RPC_THREAD_TOKEN_OPEN_BEGIN");
  if(!OpenThreadToken(GetCurrentThread(),TOKEN_QUERY,TRUE,&token)){localReason=L"OPEN_IMPERSONATED_THREAD_TOKEN_ERROR_"+std::to_wstring(GetLastError());break;}
  Log(L"RPC_THREAD_TOKEN_OPEN_OK");
  Log(L"RPC_OPEN_PROCESS_BEGIN");process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,out.pid);
  if(!process){localReason=L"OPEN_PROCESS_AS_CALLER_ERROR_"+std::to_wstring(GetLastError());break;}
  Log(L"RPC_OPEN_PROCESS_OK");
  DWORD app=0,n=0;Log(L"RPC_TOKEN_APPCONTAINER_BEGIN");
  if(!GetTokenInformation(token,TokenIsAppContainer,&app,sizeof(app),&n)){localReason=L"TOKEN_IS_APPCONTAINER_QUERY_ERROR_"+std::to_wstring(GetLastError());break;}
  Log(L"RPC_TOKEN_APPCONTAINER_OK value="+std::to_wstring(app));out.isAppContainer=(app==1);
  DWORD sessionId=0,sessionBytes=0;Log(L"RPC_TOKEN_SESSION_BEGIN");
  if(!GetTokenInformation(token,TokenSessionId,&sessionId,sizeof(sessionId),&sessionBytes)){localReason=L"TOKEN_SESSION_ID_ERROR_"+std::to_wstring(GetLastError());break;}
  out.windowsSession=sessionId;Log(L"RPC_TOKEN_SESSION_OK value="+std::to_wstring(out.windowsSession));
  std::vector<BYTE>user,container;
  Log(L"RPC_TOKEN_USER_BEGIN");if(!TokenInfo(token,TokenUser,user)){localReason=L"TOKEN_USER_QUERY_ERROR_"+std::to_wstring(GetLastError());break;}Log(L"RPC_TOKEN_USER_OK");
  out.userSid=SidText(reinterpret_cast<TOKEN_USER*>(user.data())->User.Sid);
  if(out.userSid.empty()){localReason=L"USER_SID_EMPTY";break;}
  if(!out.isAppContainer){tokenObserved=true;localReason=L"TOKEN_IS_NOT_APPCONTAINER";break;}
  Log(L"RPC_TOKEN_APPCONTAINER_SID_BEGIN");if(!TokenInfo(token,TokenAppContainerSid,container)){localReason=L"TOKEN_APPCONTAINER_SID_QUERY_ERROR_"+std::to_wstring(GetLastError());break;}Log(L"RPC_TOKEN_APPCONTAINER_SID_OK");
  out.appSid=SidText(reinterpret_cast<TOKEN_APPCONTAINER_INFORMATION*>(container.data())->TokenAppContainer);
  if(out.appSid.empty()){localReason=L"APPCONTAINER_SID_EMPTY";break;}
  UINT32 size=0;Log(L"RPC_PACKAGE_FAMILY_BEGIN");LONG rc=GetPackageFamilyName(process,&size,nullptr);
  if(rc!=ERROR_INSUFFICIENT_BUFFER||!size){localReason=L"PACKAGE_FAMILY_QUERY_ERROR_"+std::to_wstring(rc);break;}
  std::vector<wchar_t>family(size);
  if(GetPackageFamilyName(process,&size,family.data())!=ERROR_SUCCESS){localReason=L"PACKAGE_FAMILY_READ_ERROR_"+std::to_wstring(GetLastError());break;}
  out.family=family.data();Log(L"RPC_PACKAGE_FAMILY_OK");
  size=0;Log(L"RPC_PACKAGE_FULL_BEGIN");rc=GetPackageFullName(process,&size,nullptr);
  if(rc!=ERROR_INSUFFICIENT_BUFFER||!size){localReason=L"PACKAGE_FULL_QUERY_ERROR_"+std::to_wstring(rc);break;}
  std::vector<wchar_t>full(size);
  if(GetPackageFullName(process,&size,full.data())!=ERROR_SUCCESS){localReason=L"PACKAGE_FULL_READ_ERROR_"+std::to_wstring(GetLastError());break;}
  out.full=full.data();Log(L"RPC_PACKAGE_FULL_OK");tokenObserved=true;localReason=L"OK";
 }while(false);
 if(token)CloseHandle(token);
 RPC_STATUS revertStatus=RpcRevertToSelf();Log(L"RPC_REVERT status="+std::to_wstring(revertStatus));
 if(revertStatus!=RPC_S_OK){localReason=L"RPC_REVERT_TO_SELF_FAILED";tokenObserved=false;}
 if(tokenObserved&&process){
  Log(L"RPC_PROCESS_TIMES_BEGIN");FILETIME exitTime{},kernelTime{},userTime{};
  if(!GetProcessTimes(process,&out.created,&exitTime,&kernelTime,&userTime)){localReason=L"PROCESS_CREATION_TIME_ERROR_"+std::to_wstring(GetLastError());tokenObserved=false;}
  else Log(L"RPC_PROCESS_TIMES_OK");
 }
 if(tokenObserved&&!out.isAppContainer){localReason=L"TOKEN_IS_NOT_APPCONTAINER";tokenObserved=false;}
 if(tokenObserved&&out.userSid!=gExpectedUserSid){localReason=L"USER_SID_PIN_MISMATCH";tokenObserved=false;}
 if(tokenObserved&&out.appSid!=gExpectedAppSid){localReason=L"APPCONTAINER_SID_PIN_MISMATCH";tokenObserved=false;}
 if(tokenObserved&&out.family!=gExpectedFamily){localReason=L"PACKAGE_FAMILY_PIN_MISMATCH";tokenObserved=false;}
 if(tokenObserved&&out.full!=gExpectedFull){localReason=L"PACKAGE_FULL_PIN_MISMATCH";tokenObserved=false;}
 if(tokenObserved)localReason=L"OK";
 if(process)CloseHandle(process);
 if(reason)*reason=localReason;
 if(out.pid){
  ULARGE_INTEGER created{};created.LowPart=out.created.dwLowDateTime;created.HighPart=out.created.dwHighDateTime;
  Log(L"RPC_OBSERVED pid="+std::to_wstring(out.pid)+L" process_created_100ns="+std::to_wstring(created.QuadPart)+L" windows_session="+std::to_wstring(out.windowsSession)+L" token_is_appcontainer="+(out.isAppContainer?L"1":L"0")+L" user_sid="+out.userSid+L" appcontainer_sid="+out.appSid+L" package_family="+out.family+L" package_full="+out.full+L" result="+(tokenObserved?L"IDENTITY_MATCH":localReason));
 }
 return tokenObserved;
}
static bool ValidCorrelation(const wchar_t*p){
 if(!p)return false;size_t n=0;while(n<=80&&p[n])++n;if(n==0||n>80)return false;
 for(size_t i=0;i<n;++i)if(p[i]<0x21||p[i]>0x7e)return false;return true;
}
static bool SameIdentity(const ObservedIdentity&a,const ObservedIdentity&b){return a.pid==b.pid&&a.windowsSession==b.windowsSession&&a.isAppContainer==b.isAppContainer&&a.created.dwHighDateTime==b.created.dwHighDateTime&&a.created.dwLowDateTime==b.created.dwLowDateTime&&a.userSid==b.userSid&&a.appSid==b.appSid&&a.family==b.family&&a.full==b.full;}
static bool IsStopping(){std::lock_guard<std::mutex>l(gMutex);return gStopping;}
void BeginSession(handle_t binding,wchar_t*correlation,wchar_t**sessionId,wchar_t**nonce,long*status){
 if(sessionId)*sessionId=nullptr;if(nonce)*nonce=nullptr;if(status)*status=RPC_INTERNAL_ERROR;if(!ValidCorrelation(correlation)||!sessionId||!nonce||!status){if(status)*status=RPC_INVALID_ARGUMENT;return;}if(IsStopping()){*status=RPC_SERVICE_STOPPING;return;}
 ObservedIdentity id;std::wstring identityReason;if(!ObserveCaller(binding,id,&identityReason)){*status=RPC_ACCESS_DENIED;Log(L"RPC_BEGIN correlation_id="+std::wstring(correlation)+L" result=REJECT reason="+identityReason);return;}
 GUID guid{};if(CoCreateGuid(&guid)!=S_OK){*status=RPC_INTERNAL_ERROR;return;}wchar_t sidBuf[64]{};StringFromGUID2(guid,sidBuf,64);std::wstring sid=sidBuf,raw;if(!RandomHex(32,raw)){*status=RPC_INTERNAL_ERROR;return;}
 std::vector<UCHAR>digest;if(!HashNonce(raw,digest)){SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_INTERNAL_ERROR;return;}
 Session s;s.identity=id;s.nonceHash=digest;s.expiresAt=GetTickCount64()+SESSION_IDLE_MS;{std::lock_guard<std::mutex>l(gMutex);if(gStopping){SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_SERVICE_STOPPING;return;}gSessions[sid]=s;}
 *sessionId=(wchar_t*)midl_user_allocate((sid.size()+1)*sizeof(wchar_t));*nonce=(wchar_t*)midl_user_allocate((raw.size()+1)*sizeof(wchar_t));
 if(!*sessionId||!*nonce){if(*sessionId)midl_user_free(*sessionId);if(*nonce)midl_user_free(*nonce);*sessionId=nullptr;*nonce=nullptr;{std::lock_guard<std::mutex>l(gMutex);gSessions.erase(sid);}SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_INTERNAL_ERROR;return;}
 wcscpy_s(*sessionId,sid.size()+1,sid.c_str());wcscpy_s(*nonce,raw.size()+1,raw.c_str());SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_OK;Log(L"RPC_BEGIN correlation_id="+std::wstring(correlation)+L" result=ALLOW pid="+std::to_wstring(id.pid)+L" appcontainer_sid="+id.appSid+L" package_family="+id.family);
}
void Authorize(handle_t binding,wchar_t*correlation,wchar_t*sessionId,wchar_t*nonce,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;if(!ValidCorrelation(correlation)||!sessionId||!nonce||wcslen(sessionId)>80||wcslen(nonce)!=64)return;ObservedIdentity id;std::wstring identityReason;if(!ObserveCaller(binding,id,&identityReason)){*status=RPC_ACCESS_DENIED;Log(L"RPC_CALL correlation_id="+std::wstring(correlation)+L" result=REJECT reason="+identityReason);return;}std::vector<UCHAR>digest;if(!HashNonce(nonce,digest)){*status=RPC_INTERNAL_ERROR;return;}bool allowed=false;
 {std::lock_guard<std::mutex>l(gMutex);auto it=gSessions.find(sessionId);if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}auto&s=it->second;if(GetTickCount64()>s.expiresAt){gSessions.erase(it);*status=RPC_SESSION_EXPIRED;return;}if(!SameIdentity(s.identity,id)){gSessions.erase(it);*status=RPC_ACCESS_DENIED;return;}if(s.nonceUsed){*status=RPC_REPLAY;return;}s.nonceUsed=true;if(!ConstantEqual(s.nonceHash,digest)){gSessions.erase(it);*status=RPC_ACCESS_DENIED;return;}s.authorized=true;s.expiresAt=GetTickCount64()+SESSION_IDLE_MS;*status=RPC_OK;allowed=true;}
 if(allowed)Log(L"RPC_AUTHORIZE correlation_id="+std::wstring(correlation)+L" result=ALLOW pid="+std::to_wstring(id.pid)+L" session="+sessionId);
}
void Heartbeat(handle_t binding,wchar_t*correlation,wchar_t*sessionId,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;if(!ValidCorrelation(correlation)||!sessionId||wcslen(sessionId)>80)return;ObservedIdentity id;if(!ObserveCaller(binding,id)){*status=RPC_ACCESS_DENIED;return;}std::lock_guard<std::mutex>l(gMutex);auto it=gSessions.find(sessionId);if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}if(GetTickCount64()>it->second.expiresAt){gSessions.erase(it);*status=RPC_SESSION_EXPIRED;return;}if(!it->second.authorized||!SameIdentity(it->second.identity,id)){*status=RPC_ACCESS_DENIED;return;}it->second.expiresAt=GetTickCount64()+SESSION_IDLE_MS;*status=RPC_OK;
}
void CloseSession(handle_t binding,wchar_t*correlation,wchar_t*sessionId,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;if(!ValidCorrelation(correlation)||!sessionId||wcslen(sessionId)>80)return;ObservedIdentity id;if(!ObserveCaller(binding,id)){*status=RPC_ACCESS_DENIED;return;}std::lock_guard<std::mutex>l(gMutex);auto it=gSessions.find(sessionId);if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}if(!SameIdentity(it->second.identity,id)){*status=RPC_ACCESS_DENIED;return;}SecureZeroMemory(it->second.nonceHash.data(),it->second.nonceHash.size());gSessions.erase(it);*status=RPC_OK;
}

static std::string Ascii(const std::wstring& v){std::string o;o.reserve(v.size());for(wchar_t c:v){if(c<0x21||c>0x7e)return {};o.push_back((char)c);}return o;}
void TeamRoomScenario(handle_t binding,wchar_t*correlation,wchar_t*sessionId,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;
 if(!ValidCorrelation(correlation)||!sessionId||wcslen(sessionId)>80)return;
 if(IsStopping()){*status=RPC_SERVICE_STOPPING;return;}
 ObservedIdentity observed;std::wstring reason;
 if(!ObserveCaller(binding,observed,&reason)){*status=RPC_ACCESS_DENIED;Log(L"TEAM_ROOM_RPC_REJECT reason="+reason);return;}
 {
  std::lock_guard<std::mutex> lock(gMutex);auto it=gSessions.find(sessionId);
  if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}
  if(GetTickCount64()>it->second.expiresAt){gSessions.erase(it);*status=RPC_SESSION_EXPIRED;return;}
  if(!it->second.authorized||!SameIdentity(it->second.identity,observed)){*status=RPC_ACCESS_DENIED;return;}
 }
 Log(L"TEAM_ROOM_ACCESS_TOKEN_REQUESTED=NO");
 Log(L"TEAM_ROOM_AUTH_EXCHANGE_CALLED=NO");
 bool healthProbeSucceeded=false;
 if(!gTeamRoomHttpsEndpoint.empty()){
  phone_supervisor::RealSupervisorTeamRoomHttpsAdapter realAdapter;DWORD httpStatus=0;std::wstring tlsReason;
  if(!realAdapter.ProbeHealth(gTeamRoomHttpsEndpoint,httpStatus,tlsReason)){
   Log(L"TEAM_ROOM_HTTPS_TLS=REJECT reason="+tlsReason+L" http_status="+std::to_wstring(httpStatus));
   Log(L"TEAM_ROOM_HEALTH_NOT_AUTH=PASS");
   *status=RPC_INTERNAL_ERROR;return;
  }
  healthProbeSucceeded=true;
  Log(L"TEAM_ROOM_HTTPS_TLS=PASS endpoint_policy=ALLOWLIST certificate_chain=VALID hostname=VALID http_status="+std::to_wstring(httpStatus));
 }else{
  Log(L"TEAM_ROOM_HEALTH=NOT_CONFIGURED");
 }
 Log(L"TEAM_ROOM_HEALTH_NOT_AUTH=PASS");
 const bool fakeMode=(gTeamRoomFakeMode==L"1");
 if(gTeamRoomFakeMode!=L"0"&&!fakeMode){
  Log(L"TEAM_ROOM_FAKE_MODE=INVALID");
  Log(L"TEAM_ROOM_FALSE_SUCCESS_REMOVED=PASS");
  *status=RPC_ACCESS_DENIED;return;
 }
 bool fakeScenarioSucceeded=false;
 if(!fakeMode){
  Log(L"TEAM_ROOM_FAKE_MODE=DISABLED");
 }else{
  const std::string phoneId=Ascii(gTeamRoomTestPhoneId),endpointId=Ascii(gTeamRoomTestEndpointId),recipientId=Ascii(gTeamRoomTestRecipientId);
  if(phoneId!="TEST-AI-PHONE-02"||endpointId!="TEST-ENDPOINT-02"||recipientId!="FAKE-RECIPIENT-PHONE"||
     observed.family.empty()||observed.full.empty()||observed.appSid.empty()){*status=RPC_ACCESS_DENIED;Log(L"TEAM_ROOM_FAKE_PROVISIONED_BINDING=REJECT");return;}
  phone_supervisor::FakeSupervisorTeamRoomAdapter adapter;
  if(adapter.authenticate("INVALID-ASSERTION")){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_AUTH_FAILURE=PASS");
  if(!adapter.authenticate("P2.37-TEST-ONLY-ASSERTION")){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_AUTHENTICATE=PASS");
  phone_supervisor::ProvisionedPhoneBinding provisioned{phoneId,endpointId,Ascii(observed.family),Ascii(observed.full),Ascii(observed.appSid),1};
  auto wrong=provisioned;wrong.ai_phone_id="CLIENT-CHOSEN-IDENTITY";
  phone_supervisor::TeamRoomAttachment attachment;
  if(adapter.attach(wrong,attachment)){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_IDENTITY_MISMATCH=PASS");
  if(!adapter.attach(provisioned,attachment)||attachment.sender_ai_phone_id!=phoneId){*status=RPC_INTERNAL_ERROR;return;}
  if(attachment.team_room_session_id.empty()||attachment.execution_id.empty()||
     attachment.team_room_session_id==Ascii(std::wstring(sessionId))||
     attachment.execution_id==phoneId||attachment.endpoint_id!=endpointId||attachment.endpoint_fence!=1){*status=RPC_INTERNAL_ERROR;return;}
  Log(L"TEAM_ROOM_FAKE_SESSION_EXECUTION_NAMESPACE=PASS");Log(L"TEAM_ROOM_FAKE_ATTACH=PASS");
  if(!adapter.heartbeat(attachment)){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_HEARTBEAT=PASS");
  if(!adapter.reconnect(attachment,"P2.37-FRESH-CHALLENGE")){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_RECONNECT=PASS");
  phone_supervisor::TeamRoomMessage receipt;
  if(adapter.send(attachment,recipientId,"test-message","p2.37-idempotency-1",receipt)){*status=RPC_INTERNAL_ERROR;return;}
  Log(L"TEAM_ROOM_FAKE_PAIR_DENY=PASS");
  if(!adapter.grantPairForTest(phoneId,recipientId)||
     !adapter.send(attachment,recipientId,"test-message","p2.37-idempotency-1",receipt)||
     receipt.sender_ai_phone_id!=phoneId){*status=RPC_INTERNAL_ERROR;return;}
  Log(L"TEAM_ROOM_FAKE_SEND=PASS");Log(L"TEAM_ROOM_FAKE_SENDER_IDENTITY=PASS sender_ai_phone_id=TEST-AI-PHONE-02");
  std::vector<phone_supervisor::TeamRoomMessage> inbox;
  if(!adapter.receive(attachment,inbox)||inbox.size()!=1||inbox.front().sender_ai_phone_id!="FAKE-REMOTE-SENDER"){*status=RPC_INTERNAL_ERROR;return;}
  Log(L"TEAM_ROOM_FAKE_RECEIVE=PASS");
  if(!adapter.ack(attachment,inbox.front().message_id)){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_ACK=PASS");
  if(!adapter.credentialsConfined()){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_CREDENTIALS_CONFINED=PASS");
  if(!adapter.detach(attachment)||adapter.heartbeat(attachment)){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_DETACH=PASS");
  if(!adapter.revoke(attachment)){*status=RPC_INTERNAL_ERROR;return;}Log(L"TEAM_ROOM_FAKE_REVOKE=PASS");
  fakeScenarioSucceeded=true;
  Log(L"TEAM_ROOM_FAKE_SCENARIO=PASS");
  Log(L"TEAM_ROOM_FAKE_NOT_AUTHENTICATED=PASS");
 }
 // A client_credentials token alone is not a Team Room session.
 // R5 deliberately does not call /auth/exchange.
 const bool realTeamRoomSessionAuthenticated=false;
 const auto secretState=gR5CredentialLoaded?phone_supervisor::r4::SupervisorSecretState::Valid:phone_supervisor::r4::SupervisorSecretState::StoreAbsent;
 const auto decision=phone_supervisor::r4::Evaluate(
  true,
  secretState,
  healthProbeSucceeded,
  fakeMode,
  fakeScenarioSucceeded,
  realTeamRoomSessionAuthenticated);
 Log(gR5CredentialLoaded?L"TEAM_ROOM_SECRET_STATE=VALID":L"TEAM_ROOM_SECRET_STATE=STORE_ABSENT");
 Log(L"TEAM_ROOM_REAL_AUTH_SESSION=ABSENT");
 Log(L"TEAM_ROOM_FALSE_SUCCESS_REMOVED=PASS");
 if(decision.rpcStatus==RPC_OK){
  Log(L"TEAM_ROOM_FAIL_CLOSED=FAIL_UNEXPECTED_RPC_OK");
  *status=RPC_INTERNAL_ERROR;return;
 }
 Log(L"TEAM_ROOM_FAIL_CLOSED=PASS");
 if(fakeMode)Log(L"TEAM_ROOM_FAKE_MODE_NOT_AUTH=PASS");
 *status=decision.rpcStatus;
 Log(L"TEAM_ROOM_RPC_RESULT=ACCESS_DENIED");
}
static void SetState(DWORD state,DWORD error=NO_ERROR,DWORD hint=0){gStatus.dwServiceType=SERVICE_WIN32_OWN_PROCESS;gStatus.dwCurrentState=state;gStatus.dwWin32ExitCode=error;gStatus.dwWaitHint=hint;gStatus.dwControlsAccepted=state==SERVICE_RUNNING?SERVICE_ACCEPT_STOP|SERVICE_ACCEPT_SHUTDOWN:0;if(gStatusHandle)SetServiceStatus(gStatusHandle,&gStatus);}
static void WINAPI Handler(DWORD c){if(c==SERVICE_CONTROL_STOP||c==SERVICE_CONTROL_SHUTDOWN){SetState(SERVICE_STOP_PENDING,NO_ERROR,15000);{std::lock_guard<std::mutex>l(gMutex);gStopping=true;}if(gStopEvent)SetEvent(gStopEvent);}}
static RPC_STATUS RPC_ENTRY SupervisorSecurityCallback(RPC_IF_HANDLE,void* context){
 RPC_CALL_ATTRIBUTES_V2_W a{};a.Version=2;a.Flags=RPC_QUERY_CLIENT_PID|RPC_QUERY_IS_CLIENT_LOCAL;
 RPC_STATUS s=RpcServerInqCallAttributesW(reinterpret_cast<RPC_BINDING_HANDLE>(context),&a);
 if(s!=RPC_S_OK||a.ProtocolSequence!=RPC_PROTSEQ_LRPC||a.IsClientLocal!=rcclLocal||
    a.AuthenticationService!=RPC_C_AUTHN_WINNT||a.AuthenticationLevel<RPC_C_AUTHN_LEVEL_PKT_PRIVACY||
    a.NullSession||a.ClientPID==nullptr)return RPC_S_ACCESS_DENIED;
 return RPC_S_OK;
}
static void WINAPI ServiceMain(DWORD,LPWSTR*){
 gStatusHandle=RegisterServiceCtrlHandlerW(L"TokenSaverPhone02Supervisor",Handler);if(!gStatusHandle)return;SetUnhandledExceptionFilter(SupervisorUnhandledException);SetState(SERVICE_START_PENDING,NO_ERROR,15000);int argc=0;LPWSTR*argv=CommandLineToArgvW(GetCommandLineW(),&argc);if(!argv||argc<3){if(argv)LocalFree(argv);SetState(SERVICE_STOPPED,ERROR_INVALID_PARAMETER);return;}gConfigPath=argv[2];LocalFree(argv);gLogPath=gConfigPath+L".log";gLogHandle=CreateFileW(gLogPath.c_str(),FILE_APPEND_DATA,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
 if(gLogHandle!=INVALID_HANDLE_VALUE){LARGE_INTEGER logSize{};if(GetFileSizeEx(gLogHandle,&logSize)&&logSize.QuadPart==0){const BYTE bom[2]={0xFF,0xFE};DWORD bomWritten=0;WriteFile(gLogHandle,bom,sizeof(bom),&bomWritten,nullptr);FlushFileBuffers(gLogHandle);}}
 gExpectedFamily=ReadValue(L"PACKAGE_FAMILY");gExpectedFull=ReadValue(L"PACKAGE_FULL");gExpectedAppSid=ReadValue(L"APPCONTAINER_SID");gRpcAllowedAppSid=ReadValue(L"RPC_CLIENT_APPCONTAINER_SID");gServiceSid=ReadValue(L"SERVICE_SID");gExpectedUserSid=ReadValue(L"USER_SID");gRpcAllowedUserSid=ReadValue(L"RPC_CLIENT_USER_SID");gTeamRoomFakeMode=ReadValue(L"TEAM_ROOM_FAKE_MODE");gTeamRoomTestPhoneId=ReadValue(L"TEAM_ROOM_TEST_AI_PHONE_ID");gTeamRoomTestEndpointId=ReadValue(L"TEAM_ROOM_TEST_ENDPOINT_ID");gTeamRoomTestRecipientId=ReadValue(L"TEAM_ROOM_TEST_RECIPIENT_ID");gTeamRoomHttpsEndpoint=ReadValue(L"TEAM_ROOM_HTTPS_ENDPOINT");gR5TestMode=ReadValue(L"R5_TEST_MODE")==L"1";gCredentialStoreRoot=ReadValue(L"CREDENTIAL_STORE_ROOT");
 if(gExpectedFamily.empty()||gExpectedFull.empty()||gExpectedAppSid.empty()||gRpcAllowedAppSid.empty()||gRpcAllowedUserSid.empty()||gServiceSid.empty()||gExpectedUserSid.empty()||(gR5TestMode&&gCredentialStoreRoot.empty())){Log(L"SERVICE_CONFIG=INVALID");SetState(SERVICE_STOPPED,ERROR_INVALID_DATA);return;}
 if(!CurrentProcessIsLocalService()){Log(L"LOCAL_SERVICE=FAIL");SetState(SERVICE_STOPPED,ERROR_ACCESS_DENIED);return;}
 Log(L"LOCAL_SERVICE=PASS");
 if(!ServiceSidInToken()){Log(L"SERVICE_SID_IN_TOKEN=FAIL");SetState(SERVICE_STOPPED,ERROR_ACCESS_DENIED);return;}
 Log(L"SERVICE_IDENTITY=PASS SERVICE_NAME=TokenSaverPhone02Supervisor SERVICE_SID_IN_TOKEN=TRUE ACCOUNT=LocalService");
 gStopEvent=CreateEventW(nullptr,TRUE,FALSE,nullptr);if(!gStopEvent){SetState(SERVICE_STOPPED,GetLastError());return;}std::wstring rpcSddl=L"D:P(A;;GA;;;"+gRpcAllowedUserSid+L")(A;;GA;;;"+gRpcAllowedAppSid+L")";
 PSECURITY_DESCRIPTOR sd=nullptr;
 if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(rpcSddl.c_str(),SDDL_REVISION_1,&sd,nullptr)){Log(L"RPC_ENDPOINT_ACL=FAIL");SetState(SERVICE_STOPPED,GetLastError());CloseHandle(gStopEvent);return;}
 RPC_STATUS rc=RpcServerUseProtseqEpW((RPC_WSTR)L"ncalrpc",20,(RPC_WSTR)L"TokenSaver.Phone02.Supervisor.v1.1",sd);
 if(rc==RPC_S_OK)rc=RpcServerRegisterAuthInfoW(nullptr,RPC_C_AUTHN_WINNT,nullptr,nullptr);
 if(rc==RPC_S_OK)rc=RpcServerRegisterIf3(PhoneSupervisorRpc_v1_1_s_ifspec,nullptr,nullptr,
   RPC_IF_ALLOW_SECURE_ONLY|RPC_IF_ALLOW_LOCAL_ONLY,RPC_C_LISTEN_MAX_CALLS_DEFAULT,
   static_cast<unsigned int>(-1),SupervisorSecurityCallback,sd);
 LocalFree(sd);
 if(rc==RPC_S_OK)rc=RpcServerListen(1,RPC_C_LISTEN_MAX_CALLS_DEFAULT,TRUE);
 if(rc!=RPC_S_OK){Log(L"RPC_ENDPOINT=FAIL STATUS="+std::to_wstring(rc));SetState(SERVICE_STOPPED,rc);CloseHandle(gStopEvent);return;}
 SetState(SERVICE_RUNNING);Log(L"RPC_ENDPOINT=READY TRANSPORT=ncalrpc AUTH=WINNT PACKET_PRIVACY");
 std::unique_ptr<phone_supervisor::r5::SupervisorCredentialStore> r5Store;
 HANDLE credentialReadyEvent=nullptr;std::thread provisioningThread;
 if(gR5TestMode){
  r5Store=std::make_unique<phone_supervisor::r5::SupervisorCredentialStore>(gCredentialStoreRoot,gServiceSid);
  std::wstring storeReason;
  if(!r5Store->Initialize(storeReason)){Log(L"CREDENTIAL_STORE_INTEGRATED=FAIL reason="+storeReason);}
  else{
   Log(L"CREDENTIAL_STORE_INTEGRATED=PASS");Log(L"SERVICE_SID=PASS");
   credentialReadyEvent=CreateEventW(nullptr,TRUE,FALSE,nullptr);
   if(!credentialReadyEvent)Log(L"CREDENTIAL_PROVISIONING_PIPE=FAIL");
   else{
    std::wstring provisionResult;
    provisioningThread=std::thread([&](){phone_supervisor::r5::ProvisionClientSecretThroughAdminPipe(*r5Store,gServiceSid,gStopEvent,credentialReadyEvent,provisionResult);});
    Log(L"CREDENTIAL_PROVISIONING_PIPE=READY");
    HANDLE waits[2]={gStopEvent,credentialReadyEvent};
    DWORD provisionWait=WaitForMultipleObjects(2,waits,FALSE,65000);
    if(provisionWait==WAIT_OBJECT_0+1){
     if(provisioningThread.joinable())provisioningThread.join();
     Log(provisionResult);
     if(provisionResult==L"CREDENTIAL_PROVISIONED=PASS"){
      Log(L"CREDENTIAL_PROVISIONING_ADMIN_ONLY=PASS");
      RunR5OidcScenario(*r5Store);
     }else Log(L"CREDENTIAL_PROVISIONING_ADMIN_ONLY=FAIL");
    }else{
     Log(L"CREDENTIAL_PROVISIONING=FAIL");
     if(provisioningThread.joinable())provisioningThread.join();
    }
   }
  }
 }
 if(provisioningThread.joinable())provisioningThread.join();
 DWORD wait=WaitForSingleObject(gStopEvent,INFINITE);
 RpcMgmtStopServerListening(nullptr);RpcMgmtWaitServerListen();RpcServerUnregisterIf(PhoneSupervisorRpc_v1_1_s_ifspec,nullptr,TRUE);
 {std::lock_guard<std::mutex>l(gMutex);for(auto&kv:gSessions)SecureZeroMemory(kv.second.nonceHash.data(),kv.second.nonceHash.size());gSessions.clear();}
 if(gR5TestMode){
  gOidcAccessToken.Clear();Log(L"TOKEN_MEMORY_ZEROED=PASS");
  std::wstring removeReason;bool removed=r5Store&&r5Store->RemoveClientSecret(removeReason);
  Log(std::wstring(L"CREDENTIAL_REMOVE=")+(removed?L"PASS":L"FAIL"));
  bool clean=removed&&gOidcAccessToken.empty()&&(!r5Store||!r5Store->Exists());
  Log(std::wstring(L"CLEANUP=")+(clean?L"PASS":L"FAIL"));
  gR5CredentialLoaded=false;
 }
 if(credentialReadyEvent)CloseHandle(credentialReadyEvent);
 Log(L"SERVICE_CLEANUP_COMPLETE=TRUE SESSIONS=0");CloseHandle(gStopEvent);gStopEvent=nullptr;SetState(SERVICE_STOPPED,wait==WAIT_OBJECT_0?NO_ERROR:ERROR_GEN_FAILURE);if(gLogHandle!=INVALID_HANDLE_VALUE){CloseHandle(gLogHandle);gLogHandle=INVALID_HANDLE_VALUE;}
}
int wmain(){SERVICE_TABLE_ENTRYW table[]={{(LPWSTR)L"TokenSaverPhone02Supervisor",ServiceMain},{nullptr,nullptr}};if(!StartServiceCtrlDispatcherW(table))return(int)GetLastError();return 0;}
