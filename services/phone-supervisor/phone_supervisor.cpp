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
#include "phone_supervisor_rpc.h"
#pragma comment(lib,"Rpcrt4.lib")
#pragma comment(lib,"Bcrypt.lib")
#pragma comment(lib,"Advapi32.lib")
#pragma comment(lib,"Userenv.lib")
#pragma comment(lib,"Shell32.lib")
#pragma comment(lib,"Ole32.lib")
static const wchar_t* SERVICE_NAME=L"TokenSaverPhone02Supervisor";
static const wchar_t* RPC_ENDPOINT=L"TokenSaver.Phone02.Supervisor.v1";
static const ULONGLONG SESSION_IDLE_MS=60000;
static SERVICE_STATUS_HANDLE gStatusHandle=nullptr;
static SERVICE_STATUS gStatus{};
static HANDLE gStopEvent=nullptr;
static std::wstring gConfigPath,gLogPath,gExpectedFamily,gExpectedFull,gExpectedAppSid,gServiceSid,gExpectedUserSid;
static std::mutex gMutex;static std::mutex gLogMutex;static HANDLE gLogHandle=INVALID_HANDLE_VALUE;
static bool gStopping=false;
enum RpcStatus:long{RPC_OK=0,RPC_INVALID_ARGUMENT=1,RPC_ACCESS_DENIED=2,RPC_SESSION_NOT_FOUND=3,RPC_SESSION_EXPIRED=4,RPC_REPLAY=5,RPC_INTERNAL_ERROR=6,RPC_SERVICE_STOPPING=7};
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
 Log(L"RPC_IMPERSONATE_BEGIN");
 RPC_STATUS impersonateStatus=RpcImpersonateClient(nullptr);
 Log(L"RPC_IMPERSONATE_RETURN status="+std::to_wstring(impersonateStatus));
 if(impersonateStatus!=RPC_S_OK)return fail((L"RPC_IMPERSONATE_CLIENT_STATUS_"+std::to_wstring(impersonateStatus)).c_str());
 HANDLE process=nullptr,token=nullptr;bool ok=false;std::wstring localReason=L"UNKNOWN";
 do{
  Log(L"RPC_THREAD_TOKEN_OPEN_BEGIN");
  if(!OpenThreadToken(GetCurrentThread(),TOKEN_QUERY,TRUE,&token)){localReason=L"OPEN_IMPERSONATED_THREAD_TOKEN_ERROR_"+std::to_wstring(GetLastError());break;}
  Log(L"RPC_THREAD_TOKEN_OPEN_OK");
  Log(L"RPC_OPEN_PROCESS_BEGIN");
  process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,out.pid);
  if(!process){localReason=L"OPEN_PROCESS_AS_CALLER_ERROR_"+std::to_wstring(GetLastError());break;}
  Log(L"RPC_OPEN_PROCESS_OK");
  if(!GetProcessTimes(process,&out.created,nullptr,nullptr,nullptr)){localReason=L"PROCESS_CREATION_TIME_ERROR_"+std::to_wstring(GetLastError());break;}
  if(!ProcessIdToSessionId(out.pid,&out.windowsSession)){localReason=L"WINDOWS_SESSION";break;}
  DWORD app=0,n=0;if(!GetTokenInformation(token,TokenIsAppContainer,&app,sizeof(app),&n)){localReason=L"TOKEN_IS_APPCONTAINER_QUERY";break;}
  out.isAppContainer=(app==1);if(!out.isAppContainer){localReason=L"TOKEN_IS_NOT_APPCONTAINER";break;}
  std::vector<BYTE>user,container;
  if(!TokenInfo(token,TokenUser,user)){localReason=L"TOKEN_USER_QUERY";break;}
  if(!TokenInfo(token,TokenAppContainerSid,container)){localReason=L"TOKEN_APPCONTAINER_SID_QUERY";break;}
  out.userSid=SidText(reinterpret_cast<TOKEN_USER*>(user.data())->User.Sid);
  out.appSid=SidText(reinterpret_cast<TOKEN_APPCONTAINER_INFORMATION*>(container.data())->TokenAppContainer);
  if(out.userSid.empty()){localReason=L"USER_SID_EMPTY";break;}
  if(out.appSid.empty()){localReason=L"APPCONTAINER_SID_EMPTY";break;}
  UINT32 size=0;LONG rc=GetPackageFamilyName(process,&size,nullptr);
  if(rc!=ERROR_INSUFFICIENT_BUFFER||!size){localReason=L"PACKAGE_FAMILY_QUERY";break;}
  std::vector<wchar_t>family(size);
  if(GetPackageFamilyName(process,&size,family.data())!=ERROR_SUCCESS){localReason=L"PACKAGE_FAMILY_READ";break;}
  out.family=family.data();
  size=0;rc=GetPackageFullName(process,&size,nullptr);
  if(rc!=ERROR_INSUFFICIENT_BUFFER||!size){localReason=L"PACKAGE_FULL_QUERY";break;}
  std::vector<wchar_t>full(size);
  if(GetPackageFullName(process,&size,full.data())!=ERROR_SUCCESS){localReason=L"PACKAGE_FULL_READ";break;}
  out.full=full.data();
  if(out.userSid!=gExpectedUserSid){localReason=L"USER_SID_PIN_MISMATCH";break;}
  if(out.appSid!=gExpectedAppSid){localReason=L"APPCONTAINER_SID_PIN_MISMATCH";break;}
  if(out.family!=gExpectedFamily){localReason=L"PACKAGE_FAMILY_PIN_MISMATCH";break;}
  if(out.full!=gExpectedFull){localReason=L"PACKAGE_FULL_PIN_MISMATCH";break;}
  ok=true;localReason=L"OK";
 }while(false);
 if(token)CloseHandle(token);if(process)CloseHandle(process);
 RPC_STATUS revertStatus=RpcRevertToSelf();Log(L"RPC_REVERT status="+std::to_wstring(revertStatus));
 if(revertStatus!=RPC_S_OK){ok=false;localReason=L"RPC_REVERT_TO_SELF_FAILED";}
 if(reason)*reason=localReason;
 if(out.pid){
  ULARGE_INTEGER created{};created.LowPart=out.created.dwLowDateTime;created.HighPart=out.created.dwHighDateTime;
  Log(L"RPC_OBSERVED pid="+std::to_wstring(out.pid)+L" process_created_100ns="+std::to_wstring(created.QuadPart)+L" windows_session="+std::to_wstring(out.windowsSession)+L" token_is_appcontainer="+(out.isAppContainer?L"1":L"0")+L" user_sid="+out.userSid+L" appcontainer_sid="+out.appSid+L" package_family="+out.family+L" package_full="+out.full+L" result="+(ok?L"IDENTITY_MATCH":localReason));
 }
 return ok;
}
static bool SameIdentity(const ObservedIdentity&a,const ObservedIdentity&b){return a.pid==b.pid&&a.windowsSession==b.windowsSession&&a.isAppContainer==b.isAppContainer&&a.created.dwHighDateTime==b.created.dwHighDateTime&&a.created.dwLowDateTime==b.created.dwLowDateTime&&a.userSid==b.userSid&&a.appSid==b.appSid&&a.family==b.family&&a.full==b.full;}
static bool IsStopping(){std::lock_guard<std::mutex>l(gMutex);return gStopping;}
void BeginSession(handle_t binding,wchar_t*correlation,wchar_t**sessionId,wchar_t**nonce,long*status){
 if(sessionId)*sessionId=nullptr;if(nonce)*nonce=nullptr;if(status)*status=RPC_INTERNAL_ERROR;if(!correlation||!sessionId||!nonce||!status||wcslen(correlation)>80){if(status)*status=RPC_INVALID_ARGUMENT;return;}if(IsStopping()){*status=RPC_SERVICE_STOPPING;return;}
 ObservedIdentity id;std::wstring identityReason;if(!ObserveCaller(binding,id,&identityReason)){*status=RPC_ACCESS_DENIED;Log(L"RPC_BEGIN result=REJECT reason="+identityReason);return;}
 GUID guid{};if(CoCreateGuid(&guid)!=S_OK){*status=RPC_INTERNAL_ERROR;return;}wchar_t sidBuf[64]{};StringFromGUID2(guid,sidBuf,64);std::wstring sid=sidBuf,raw;if(!RandomHex(32,raw)){*status=RPC_INTERNAL_ERROR;return;}
 std::vector<UCHAR>digest;if(!HashNonce(raw,digest)){SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_INTERNAL_ERROR;return;}
 Session s;s.identity=id;s.nonceHash=digest;s.expiresAt=GetTickCount64()+SESSION_IDLE_MS;{std::lock_guard<std::mutex>l(gMutex);if(gStopping){SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_SERVICE_STOPPING;return;}gSessions[sid]=s;}
 *sessionId=(wchar_t*)midl_user_allocate((sid.size()+1)*sizeof(wchar_t));*nonce=(wchar_t*)midl_user_allocate((raw.size()+1)*sizeof(wchar_t));
 if(!*sessionId||!*nonce){if(*sessionId)midl_user_free(*sessionId);if(*nonce)midl_user_free(*nonce);*sessionId=nullptr;*nonce=nullptr;{std::lock_guard<std::mutex>l(gMutex);gSessions.erase(sid);}SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_INTERNAL_ERROR;return;}
 wcscpy_s(*sessionId,sid.size()+1,sid.c_str());wcscpy_s(*nonce,raw.size()+1,raw.c_str());SecureZeroMemory(raw.data(),raw.size()*sizeof(wchar_t));*status=RPC_OK;Log(L"RPC_BEGIN result=ALLOW pid="+std::to_wstring(id.pid)+L" appcontainer_sid="+id.appSid+L" package_family="+id.family);
}
void Authorize(handle_t binding,wchar_t*correlation,wchar_t*sessionId,wchar_t*nonce,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;if(!correlation||!sessionId||!nonce||wcslen(correlation)>80||wcslen(sessionId)>80||wcslen(nonce)!=64)return;ObservedIdentity id;std::wstring identityReason;if(!ObserveCaller(binding,id,&identityReason)){*status=RPC_ACCESS_DENIED;Log(L"RPC_CALL result=REJECT reason="+identityReason);return;}std::vector<UCHAR>digest;if(!HashNonce(nonce,digest)){*status=RPC_INTERNAL_ERROR;return;}bool allowed=false;
 {std::lock_guard<std::mutex>l(gMutex);auto it=gSessions.find(sessionId);if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}auto&s=it->second;if(GetTickCount64()>s.expiresAt){gSessions.erase(it);*status=RPC_SESSION_EXPIRED;return;}if(!SameIdentity(s.identity,id)){gSessions.erase(it);*status=RPC_ACCESS_DENIED;return;}if(s.nonceUsed){*status=RPC_REPLAY;return;}s.nonceUsed=true;if(!ConstantEqual(s.nonceHash,digest)){gSessions.erase(it);*status=RPC_ACCESS_DENIED;return;}s.authorized=true;s.expiresAt=GetTickCount64()+SESSION_IDLE_MS;*status=RPC_OK;allowed=true;}
 if(allowed)Log(L"RPC_AUTHORIZE result=ALLOW pid="+std::to_wstring(id.pid)+L" session="+sessionId);
}
void Heartbeat(handle_t binding,wchar_t*correlation,wchar_t*sessionId,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;if(!correlation||!sessionId||wcslen(correlation)>80||wcslen(sessionId)>80)return;ObservedIdentity id;if(!ObserveCaller(binding,id)){*status=RPC_ACCESS_DENIED;return;}std::lock_guard<std::mutex>l(gMutex);auto it=gSessions.find(sessionId);if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}if(GetTickCount64()>it->second.expiresAt){gSessions.erase(it);*status=RPC_SESSION_EXPIRED;return;}if(!it->second.authorized||!SameIdentity(it->second.identity,id)){*status=RPC_ACCESS_DENIED;return;}it->second.expiresAt=GetTickCount64()+SESSION_IDLE_MS;*status=RPC_OK;
}
void CloseSession(handle_t binding,wchar_t*correlation,wchar_t*sessionId,long*status){
 if(!status)return;*status=RPC_INVALID_ARGUMENT;if(!correlation||!sessionId||wcslen(correlation)>80||wcslen(sessionId)>80)return;ObservedIdentity id;if(!ObserveCaller(binding,id)){*status=RPC_ACCESS_DENIED;return;}std::lock_guard<std::mutex>l(gMutex);auto it=gSessions.find(sessionId);if(it==gSessions.end()){*status=RPC_SESSION_NOT_FOUND;return;}if(!SameIdentity(it->second.identity,id)){*status=RPC_ACCESS_DENIED;return;}SecureZeroMemory(it->second.nonceHash.data(),it->second.nonceHash.size());gSessions.erase(it);*status=RPC_OK;
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
 gExpectedFamily=ReadValue(L"PACKAGE_FAMILY");gExpectedFull=ReadValue(L"PACKAGE_FULL");gExpectedAppSid=ReadValue(L"APPCONTAINER_SID");gServiceSid=ReadValue(L"SERVICE_SID");gExpectedUserSid=ReadValue(L"USER_SID");
 if(gExpectedFamily.empty()||gExpectedFull.empty()||gExpectedAppSid.empty()||gServiceSid.empty()||gExpectedUserSid.empty()){Log(L"SERVICE_CONFIG=INVALID");SetState(SERVICE_STOPPED,ERROR_INVALID_DATA);return;}
 if(!ServiceSidInToken()){Log(L"SERVICE_SID_IN_TOKEN=FAIL");SetState(SERVICE_STOPPED,ERROR_ACCESS_DENIED);return;}
 Log(L"SERVICE_IDENTITY=PASS SERVICE_NAME=TokenSaverPhone02Supervisor SERVICE_SID_IN_TOKEN=TRUE ACCOUNT=LocalService");
 gStopEvent=CreateEventW(nullptr,TRUE,FALSE,nullptr);if(!gStopEvent){SetState(SERVICE_STOPPED,GetLastError());return;}std::wstring rpcSddl=L"D:P(A;;GA;;;"+gExpectedUserSid+L")(A;;GA;;;"+gExpectedAppSid+L")";
 PSECURITY_DESCRIPTOR sd=nullptr;
 if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(rpcSddl.c_str(),SDDL_REVISION_1,&sd,nullptr)){Log(L"RPC_ENDPOINT_ACL=FAIL");SetState(SERVICE_STOPPED,GetLastError());CloseHandle(gStopEvent);return;}
 RPC_STATUS rc=RpcServerUseProtseqEpW((RPC_WSTR)L"ncalrpc",20,(RPC_WSTR)L"TokenSaver.Phone02.Supervisor.v1",sd);
 if(rc==RPC_S_OK)rc=RpcServerRegisterAuthInfoW(nullptr,RPC_C_AUTHN_WINNT,nullptr,nullptr);
 if(rc==RPC_S_OK)rc=RpcServerRegisterIf3(PhoneSupervisorRpc_v1_0_s_ifspec,nullptr,nullptr,
   RPC_IF_ALLOW_SECURE_ONLY|RPC_IF_ALLOW_LOCAL_ONLY,RPC_C_LISTEN_MAX_CALLS_DEFAULT,
   static_cast<unsigned int>(-1),SupervisorSecurityCallback,sd);
 LocalFree(sd);
 if(rc==RPC_S_OK)rc=RpcServerListen(1,RPC_C_LISTEN_MAX_CALLS_DEFAULT,TRUE);
 if(rc!=RPC_S_OK){Log(L"RPC_ENDPOINT=FAIL STATUS="+std::to_wstring(rc));SetState(SERVICE_STOPPED,rc);CloseHandle(gStopEvent);return;}
 SetState(SERVICE_RUNNING);Log(L"RPC_ENDPOINT=READY TRANSPORT=ncalrpc AUTH=WINNT PACKET_PRIVACY");DWORD wait=WaitForSingleObject(gStopEvent,INFINITE);
 RpcMgmtStopServerListening(nullptr);RpcMgmtWaitServerListen();RpcServerUnregisterIf(PhoneSupervisorRpc_v1_0_s_ifspec,nullptr,TRUE);
 {std::lock_guard<std::mutex>l(gMutex);for(auto&kv:gSessions)SecureZeroMemory(kv.second.nonceHash.data(),kv.second.nonceHash.size());gSessions.clear();}
 Log(L"SERVICE_CLEANUP_COMPLETE=TRUE SESSIONS=0");CloseHandle(gStopEvent);gStopEvent=nullptr;SetState(SERVICE_STOPPED,wait==WAIT_OBJECT_0?NO_ERROR:ERROR_GEN_FAILURE);if(gLogHandle!=INVALID_HANDLE_VALUE){CloseHandle(gLogHandle);gLogHandle=INVALID_HANDLE_VALUE;}
}
int wmain(){SERVICE_TABLE_ENTRYW table[]={{(LPWSTR)L"TokenSaverPhone02Supervisor",ServiceMain},{nullptr,nullptr}};if(!StartServiceCtrlDispatcherW(table))return(int)GetLastError();return 0;}
