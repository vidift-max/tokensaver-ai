#define SECURITY_WIN32
#include <windows.h>
#include <sddl.h>
#include <rpc.h>
#include <rpcdce.h>
#include <rpcasync.h>
#include <userenv.h>
#include <appmodel.h>
#include <string>
#include <vector>
#include <sstream>
#include <iostream>
#include <fstream>
#include <cwchar>
#include <cwctype>
#include <cstdint>
#include "phone02-msix-rpc.h"

static std::wstring gExpectedFamily,gExpectedSid,gExpectedFull,gExpectedPath,gUserSid,gEndpoint;
static void ClientLog(const std::wstring&line){
 wchar_t local[32768]{};DWORD n=GetEnvironmentVariableW(L"LOCALAPPDATA",local,32768);
 if(n&&n<32768){std::wstring file=std::wstring(local)+L"\\Phone02-MsixPoA-rpc-client-"+std::to_wstring(GetCurrentProcessId())+L".log";std::wofstream out(file,std::ios::app);if(out){out<<line<<L"\n";out.flush();}}
 wprintf(L"%s\n",line.c_str());fflush(stdout);
}
static bool Same(const std::wstring&a,const std::wstring&b){return _wcsicmp(a.c_str(),b.c_str())==0;}
static std::wstring SidString(PSID sid){if(!sid)return L"UNAVAILABLE";LPWSTR p=nullptr;if(!ConvertSidToStringSidW(sid,&p))return L"UNAVAILABLE";std::wstring s=p;LocalFree(p);return s;}
static std::wstring TokenSid(HANDLE token,TOKEN_INFORMATION_CLASS cls){
 DWORD n=0;GetTokenInformation(token,cls,nullptr,0,&n);if(!n)return L"UNAVAILABLE";
 std::vector<BYTE>b(n);if(!GetTokenInformation(token,cls,b.data(),n,&n))return L"UNAVAILABLE";
 PSID sid=nullptr;
 if(cls==TokenUser)sid=reinterpret_cast<TOKEN_USER*>(b.data())->User.Sid;
 else if(cls==TokenAppContainerSid)sid=reinterpret_cast<TOKEN_APPCONTAINER_INFORMATION*>(b.data())->TokenAppContainer;
 return SidString(sid);
}
static DWORD TokenApp(HANDLE token){DWORD v=0,n=0;if(!GetTokenInformation(token,TokenIsAppContainer,&v,sizeof(v),&n))return 0;return v;}
static std::wstring PackageValue(HANDLE process,bool full){
 UINT32 n=0;LONG rc=full?GetPackageFullName(process,&n,nullptr):GetPackageFamilyName(process,&n,nullptr);
 if(rc==APPMODEL_ERROR_NO_PACKAGE)return L"NO_PACKAGE_IDENTITY";
 if(rc!=ERROR_INSUFFICIENT_BUFFER||!n)return L"PACKAGE_QUERY_FAILED_"+std::to_wstring(rc);
 std::vector<wchar_t>b(n);rc=full?GetPackageFullName(process,&n,b.data()):GetPackageFamilyName(process,&n,b.data());
 return rc==ERROR_SUCCESS?std::wstring(b.data()):L"PACKAGE_QUERY_FAILED_"+std::to_wstring(rc);
}
static std::wstring ProcessPath(HANDLE process){wchar_t b[32768]{};DWORD n=32768;return QueryFullProcessImageNameW(process,0,b,&n)?std::wstring(b,n):L"UNAVAILABLE";}
static DWORD ProcessSession(DWORD pid){DWORD s=0;return ProcessIdToSessionId(pid,&s)?s:0xFFFFFFFF;}
static std::wstring ClaimValue(const std::wstring&claims,const std::wstring&key){
 std::wstring prefix=key+L"=";size_t p=claims.find(prefix);if(p==std::wstring::npos)return L"";
 p+=prefix.size();size_t e=claims.find(L';',p);return claims.substr(p,e==std::wstring::npos?e:e-p);
}
static std::wstring MakeClaims(const std::wstring&mode,const std::wstring&family,const std::wstring&sid,const std::wstring&full){
 std::wstring pid=std::to_wstring(GetCurrentProcessId()),app=L"1",claimedFamily=family,claimedSid=sid,claimedFull=full;
 if(mode==L"spoof-pid"||mode==L"spoof-all")pid=L"1";
 if(mode==L"spoof-app"||mode==L"spoof-all")app=L"1";
 if(mode==L"spoof-sid"||mode==L"spoof-all")claimedSid=L"S-1-15-2-999999999";
 if(mode==L"spoof-family"||mode==L"spoof-all")claimedFamily=L"Fake.Package_Fake";
 if(mode==L"spoof-full"||mode==L"spoof-all")claimedFull=L"Fake.Package_9.9.9.9_x64__fake";
 return L"PID="+pid+L";APP="+app+L";SID="+claimedSid+L";FAMILY="+claimedFamily+L";FULL="+claimedFull+L";";
}
static RPC_STATUS RPC_ENTRY Phone02SecurityCallback(RPC_IF_HANDLE,void* context){
 RPC_CALL_ATTRIBUTES_V2_W a{};a.Version=2;a.Flags=RPC_QUERY_CLIENT_PID;
 RPC_STATUS s=RpcServerInqCallAttributesW(reinterpret_cast<RPC_BINDING_HANDLE>(context),&a);
 if(s!=RPC_S_OK)return RPC_S_ACCESS_DENIED;
 if(a.ProtocolSequence!=RPC_PROTSEQ_LRPC||a.IsClientLocal!=rcclLocal||
    a.AuthenticationLevel<RPC_C_AUTHN_LEVEL_PKT_PRIVACY||
    a.AuthenticationService!=RPC_C_AUTHN_WINNT||a.NullSession||a.ClientPID==nullptr)
   return RPC_S_ACCESS_DENIED;
 return RPC_S_OK;
}
extern "C" void Phone02Authorize(handle_t hBinding,wchar_t*claims,long*authorized){
 if(authorized)*authorized=0;
 RPC_CALL_ATTRIBUTES_V2_W a{};a.Version=2;a.Flags=RPC_QUERY_CLIENT_PID;
 RPC_STATUS attrStatus=RpcServerInqCallAttributesW(hBinding,&a);
 DWORD pid=attrStatus==RPC_S_OK?static_cast<DWORD>(reinterpret_cast<ULONG_PTR>(a.ClientPID)):0;
 HANDLE process=pid?OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,pid):nullptr;
 HANDLE processToken=nullptr,threadToken=nullptr;
 DWORD processApp=0,threadApp=0,processSession=0xFFFFFFFF;
 std::wstring processUser=L"UNAVAILABLE",threadUser=L"UNAVAILABLE",processSid=L"UNAVAILABLE",threadSid=L"UNAVAILABLE";
 std::wstring family=L"UNAVAILABLE",full=L"UNAVAILABLE",path=L"UNAVAILABLE";
 bool processTokenOk=process&&OpenProcessToken(process,TOKEN_QUERY,&processToken);
 if(processTokenOk){
  processApp=TokenApp(processToken);processUser=TokenSid(processToken,TokenUser);processSid=TokenSid(processToken,TokenAppContainerSid);
  family=PackageValue(process,false);full=PackageValue(process,true);path=ProcessPath(process);processSession=ProcessSession(pid);
 }
 RPC_STATUS impersonateStatus=RpcImpersonateClient(hBinding);
 bool threadTokenOk=impersonateStatus==RPC_S_OK&&OpenThreadToken(GetCurrentThread(),TOKEN_QUERY,TRUE,&threadToken);
 if(threadTokenOk){threadApp=TokenApp(threadToken);threadUser=TokenSid(threadToken,TokenUser);threadSid=TokenSid(threadToken,TokenAppContainerSid);}
 if(impersonateStatus==RPC_S_OK)RpcRevertToSelf();
 bool correlation=processTokenOk&&threadTokenOk&&processApp==threadApp&&Same(processUser,threadUser)&&Same(processSid,threadSid);
 bool packageMatch=Same(family,gExpectedFamily)&&Same(full,gExpectedFull)&&Same(processSid,gExpectedSid);
 bool pathMatch=Same(path,gExpectedPath);
 bool sessionMatch=processSession==ProcessSession(GetCurrentProcessId());
 bool userMatch=Same(processUser,gUserSid)&&Same(threadUser,gUserSid);
 bool allow=attrStatus==RPC_S_OK&&pid!=0&&processTokenOk&&threadTokenOk&&processApp==1&&threadApp==1&&correlation&&packageMatch&&pathMatch&&sessionMatch&&userMatch;
 std::wstring c=claims?claims:L"";
 bool claimsMatch=ClaimValue(c,L"PID")==std::to_wstring(pid)&&ClaimValue(c,L"APP")==L"1"&&
  Same(ClaimValue(c,L"SID"),processSid)&&Same(ClaimValue(c,L"FAMILY"),family)&&Same(ClaimValue(c,L"FULL"),full);
 wprintf(L"RPC_CALL_RECEIVED=TRUE RPC_CLIENT_PID=%lu ATTR_STATUS=%lu RPC_PROTOCOL=%lu RPC_LOCALITY=%u AUTH_LEVEL=%lu AUTH_SERVICE=%lu NULL_SESSION=%u\n",pid,attrStatus,a.ProtocolSequence,a.IsClientLocal,a.AuthenticationLevel,a.AuthenticationService,a.NullSession);
 wprintf(L"RPC_CLIENT_SESSION=%lu SUPERVISOR_SESSION=%lu TOKEN_IS_APPCONTAINER=%u THREAD_TOKEN_IS_APPCONTAINER=%u\n",processSession,ProcessSession(GetCurrentProcessId()),processApp,threadApp);
 wprintf(L"OBSERVED_USER_SID=%s THREAD_USER_SID=%s\nOBSERVED_APPCONTAINER_SID=%s THREAD_APPCONTAINER_SID=%s\n",processUser.c_str(),threadUser.c_str(),processSid.c_str(),threadSid.c_str());
 wprintf(L"OBSERVED_PACKAGE_FAMILY=%s\nOBSERVED_PACKAGE_FULL_NAME=%s\nOBSERVED_PROCESS_PATH=%s\n",family.c_str(),full.c_str(),path.c_str());
 wprintf(L"PID_PROCESS_TOKEN_CORRELATION=%s PACKAGE_IDENTITY_MATCH=%s PROCESS_PATH_MATCH=%s SESSION_MATCH=%s USER_MATCH=%s\n",correlation?L"PASS":L"FAIL",packageMatch?L"PASS":L"FAIL",pathMatch?L"PASS":L"FAIL",sessionMatch?L"PASS":L"FAIL",userMatch?L"PASS":L"FAIL");
 wprintf(L"CLIENT_CLAIMS_MATCH_OBSERVED=%s CLAIMS_IGNORED_FOR_AUTHORIZATION=TRUE\n",claimsMatch?L"TRUE":L"FALSE");
 wprintf(L"AUTHORIZATION_RESULT=%s\n",allow?L"ALLOW":L"REJECT");
 if(authorized)*authorized=allow?1:0;
 if(processToken)CloseHandle(processToken);if(threadToken)CloseHandle(threadToken);if(process)CloseHandle(process);
 RpcMgmtStopServerListening(nullptr);
}
static int RpcSupervisor(const std::wstring&family,const std::wstring&sid,const std::wstring&full,const std::wstring&path,const std::wstring&endpoint){
 gExpectedFamily=family;gExpectedSid=sid;gExpectedFull=full;gExpectedPath=path;gEndpoint=endpoint;
 HANDLE selfToken=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&selfToken))return 51;
 gUserSid=TokenSid(selfToken,TokenUser);CloseHandle(selfToken);
 std::wstring sddl=L"D:P(A;;GA;;;"+gUserSid+L")(A;;GA;;;"+gExpectedSid+L")";
 PSECURITY_DESCRIPTOR sd=nullptr;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&sd,nullptr)){wprintf(L"RPC_SDDL_FAIL=%lu\n",GetLastError());return 52;}
 RPC_STATUS s=RpcServerUseProtseqEpW(reinterpret_cast<RPC_WSTR>(const_cast<wchar_t*>(L"ncalrpc")),RPC_C_PROTSEQ_MAX_REQS_DEFAULT,reinterpret_cast<RPC_WSTR>(const_cast<wchar_t*>(endpoint.c_str())),sd);
 LocalFree(sd);
 if(s!=RPC_S_OK){wprintf(L"RPC_ENDPOINT_CREATE=FAIL STATUS=%lu\n",s);return 53;}
 s=RpcServerRegisterAuthInfoW(nullptr,RPC_C_AUTHN_WINNT,nullptr,nullptr);
 if(s!=RPC_S_OK){wprintf(L"RPC_AUTH_REGISTER=FAIL STATUS=%lu\n",s);return 54;}
 s=RpcServerRegisterIf3(Phone02Ipc_v1_0_s_ifspec,nullptr,nullptr,RPC_IF_ALLOW_SECURE_ONLY|RPC_IF_ALLOW_LOCAL_ONLY,RPC_C_LISTEN_MAX_CALLS_DEFAULT,static_cast<unsigned int>(-1),Phone02SecurityCallback,nullptr);
 if(s!=RPC_S_OK){wprintf(L"RPC_INTERFACE_REGISTER=FAIL STATUS=%lu\n",s);return 55;}
 wprintf(L"RPC_SERVER_READY=TRUE PROTOCOL=ncalrpc ENDPOINT=%s USER_SID=%s EXPECTED_APPCONTAINER_SID=%s PACKAGE_FAMILY=%s PACKAGE_FULL_NAME=%s EXPECTED_CLIENT_PATH=%s\n",endpoint.c_str(),gUserSid.c_str(),sid.c_str(),family.c_str(),full.c_str(),path.c_str());fflush(stdout);
 s=RpcServerListen(1,RPC_C_LISTEN_MAX_CALLS_DEFAULT,FALSE);
 wprintf(L"RPC_SERVER_LISTEN_EXIT=%lu\n",s);
 RpcServerUnregisterIf(Phone02Ipc_v1_0_s_ifspec,nullptr,TRUE);
 return s==RPC_S_OK?0:56;
}
static int RpcClient(const std::wstring&family,const std::wstring&sid,const std::wstring&full,const std::wstring&mode,const std::wstring&endpoint){
 ClientLog(L"RPC_CLIENT_MAIN PID="+std::to_wstring(GetCurrentProcessId())+L" SESSION="+std::to_wstring(ProcessSession(GetCurrentProcessId())));
 ClientLog(L"RPC_CLIENT_PROCESS_IMAGE="+ProcessPath(GetCurrentProcess()));
 std::wstring claims=MakeClaims(mode,family,sid,full);
 RPC_WSTR stringBinding=nullptr;RPC_BINDING_HANDLE binding=nullptr;
 RPC_STATUS s=RpcStringBindingComposeW(nullptr,reinterpret_cast<RPC_WSTR>(const_cast<wchar_t*>(L"ncalrpc")),nullptr,reinterpret_cast<RPC_WSTR>(const_cast<wchar_t*>(endpoint.c_str())),nullptr,&stringBinding);
 if(s!=RPC_S_OK){ClientLog(L"RPC_CLIENT_BINDING=FAIL STAGE=COMPOSE STATUS="+std::to_wstring(s));return 61;}
 s=RpcBindingFromStringBindingW(stringBinding,&binding);RpcStringFreeW(&stringBinding);
 if(s!=RPC_S_OK){ClientLog(L"RPC_CLIENT_BINDING=FAIL STAGE=FROM_STRING STATUS="+std::to_wstring(s));return 62;}
 s=RpcBindingSetAuthInfoW(binding,nullptr,RPC_C_AUTHN_LEVEL_PKT_PRIVACY,RPC_C_AUTHN_WINNT,nullptr,RPC_C_AUTHZ_NONE);
 if(s!=RPC_S_OK){ClientLog(L"RPC_CLIENT_AUTH=FAIL STATUS="+std::to_wstring(s));RpcBindingFree(&binding);return 63;}
 long authorized=-1;RPC_STATUS callStatus=RPC_S_OK;
 RpcTryExcept{Phone02Authorize(binding,const_cast<wchar_t*>(claims.c_str()),&authorized);}
 RpcExcept(1){callStatus=RpcExceptionCode();}
 RpcEndExcept
 RpcBindingFree(&binding);
 ClientLog(L"RPC_CLIENT_BINDING=PASS ENDPOINT="+endpoint+L" MODE="+mode+L" PID="+std::to_wstring(GetCurrentProcessId()));
 ClientLog(L"RPC_CLIENT_CALL_STATUS="+std::to_wstring(callStatus)+L" RPC_CLIENT_AUTHORIZATION="+(authorized==1?std::wstring(L"ALLOW"):std::wstring(L"REJECT"))+L" RESULT="+std::to_wstring(authorized));
 return callStatus==RPC_S_OK?0:64;
}
int wmain(int argc,wchar_t**argv){
 if(argc>1&&wcscmp(argv[1],L"--rpc-supervisor")==0){if(argc<7)return 10;return RpcSupervisor(argv[2],argv[3],argv[4],argv[5],argv[6]);}
 if(argc>1&&wcscmp(argv[1],L"--rpc-client")==0){if(argc<7)return 11;return RpcClient(argv[2],argv[3],argv[4],argv[5],argv[6]);}
 if(argc>=6){return RpcClient(argv[1],argv[2],argv[3],argv[4],argv[5]);}
 return 12;
}
