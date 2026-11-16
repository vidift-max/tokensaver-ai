#include <windows.h>
#include <rpc.h>
#include <rpcdce.h>
#include <objbase.h>
#include <cstdlib>
#include <string>
#include <fstream>
#include "phone_supervisor_rpc.h"
#pragma comment(lib,"Rpcrt4.lib")
#pragma comment(lib,"Ole32.lib")
static const long RPC_OK=0;
static const long RPC_ACCESS_DENIED=2;
extern "C" void* __RPC_USER midl_user_allocate(size_t n){return malloc(n);}
extern "C" void __RPC_USER midl_user_free(void*p){free(p);}
static void Log(const std::wstring&s){
 wchar_t p[32768]{};DWORD n=GetEnvironmentVariableW(L"LOCALAPPDATA",p,32768);
 std::wstring path=(n&&n<32768)?std::wstring(p)+L"\\PhoneSupervisor-P236-client.log":L"PhoneSupervisor-P236-client.log";
 std::wofstream f(path,std::ios::app);if(f){f<<s<<L"\n";f.flush();}wprintf(L"%s\n",s.c_str());fflush(stdout);wprintf(L"%s\n",s.c_str());fflush(stdout);
}
int wmain(int argc,wchar_t**argv){
 RPC_WSTR str=nullptr;RPC_BINDING_HANDLE binding=nullptr;
 RPC_STATUS rc=RpcStringBindingComposeW(nullptr,(RPC_WSTR)L"ncalrpc",nullptr,(RPC_WSTR)L"TokenSaver.Phone02.Supervisor.v1",nullptr,&str);
 if(rc==RPC_S_OK)rc=RpcBindingFromStringBindingW(str,&binding);if(str)RpcStringFreeW(&str);
 if(rc==RPC_S_OK)rc=RpcBindingSetAuthInfoW(binding,nullptr,RPC_C_AUTHN_LEVEL_PKT_PRIVACY,RPC_C_AUTHN_WINNT,nullptr,RPC_C_AUTHZ_NONE);
 if(rc!=RPC_S_OK){Log(L"CLIENT_RPC_BIND=FAIL");return 10;}
 GUID g{};CoCreateGuid(&g);wchar_t corr[64]{};StringFromGUID2(g,corr,64);
 wchar_t*session=nullptr;wchar_t*nonce=nullptr;long status=99;
 BeginSession(binding,corr,&session,&nonce,&status);
 if(argc>1&&wcscmp(argv[1],L"--common")==0){
  Log(status==RPC_ACCESS_DENIED?L"COMMON_PROCESS_REJECT=PASS":L"COMMON_PROCESS_REJECT=FAIL");
  if(session)midl_user_free(session);if(nonce)midl_user_free(nonce);RpcBindingFree(&binding);
  return status==RPC_ACCESS_DENIED?0:11;
 }
 if(status!=RPC_OK||!session||!nonce){Log(L"LEGITIMATE_APPCONTAINER=FAIL");RpcBindingFree(&binding);return 12;}
 Log(L"LEGITIMATE_APPCONTAINER=PASS");
 Authorize(binding,corr,session,nonce,&status);SecureZeroMemory(nonce,wcslen(nonce)*sizeof(wchar_t));midl_user_free(nonce);
 if(status!=RPC_OK){Log(L"AUTHORIZATION_ALLOW=FAIL");midl_user_free(session);RpcBindingFree(&binding);return 13;}
 Log(L"WINDOWS_IDENTITY=PASS");Log(L"AUTHORIZATION_ALLOW=PASS");
 Heartbeat(binding,corr,session,&status);
 if(status!=RPC_OK){Log(L"HEARTBEAT=FAIL");midl_user_free(session);RpcBindingFree(&binding);return 14;}
 Log(L"HEARTBEAT=PASS");CloseSession(binding,corr,session,&status);midl_user_free(session);RpcBindingFree(&binding);
 if(status!=RPC_OK){Log(L"CLOSE_SESSION=FAIL");return 15;}Log(L"CLOSE_SESSION=PASS");return 0;
}
