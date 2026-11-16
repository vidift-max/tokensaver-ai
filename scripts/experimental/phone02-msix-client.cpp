#define WIN32_LEAN_AND_MEAN
#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <appmodel.h>
#include <userenv.h>
#include <sddl.h>
#include <stdio.h>
#include <string>
#include <vector>
#include <wchar.h>
#pragma comment(lib,"advapi32.lib")
#pragma comment(lib,"userenv.lib")
static std::wstring PIPE_NAME=L"\\\\.\\pipe\\LOCAL\\TokenSaver.Phone02.MsixPoA52881";
static std::wstring SidString(PSID sid){if(!sid||!IsValidSid(sid))return L"UNAVAILABLE";LPWSTR p=nullptr;if(!ConvertSidToStringSidW(sid,&p))return L"UNAVAILABLE";std::wstring s=p;LocalFree(p);return s;}
static std::wstring TokenSid(HANDLE t,TOKEN_INFORMATION_CLASS c){DWORD n=0;GetTokenInformation(t,c,nullptr,0,&n);if(!n)return L"UNAVAILABLE";std::vector<BYTE>b(n);if(!GetTokenInformation(t,c,b.data(),n,&n))return L"UNAVAILABLE";PSID sid=c==TokenUser?reinterpret_cast<TOKEN_USER*>(b.data())->User.Sid:reinterpret_cast<TOKEN_APPCONTAINER_INFORMATION*>(b.data())->TokenAppContainer;return SidString(sid);}
static DWORD TokenApp(HANDLE t){DWORD v=0,n=0;return GetTokenInformation(t,TokenIsAppContainer,&v,sizeof(v),&n)?v:0;}
static std::wstring PackageValue(HANDLE t,bool full){HMODULE mod=GetModuleHandleW(L"kernel32.dll");const char* name=full?"GetPackageFullNameFromToken":"GetPackageFamilyNameFromToken";using QueryFn=LONG(WINAPI*)(HANDLE,UINT32*,PWSTR);auto fn=reinterpret_cast<QueryFn>(GetProcAddress(mod,name));if(!fn)return L"API_UNAVAILABLE";UINT32 n=0;LONG rc=fn(t,&n,nullptr);if(rc!=ERROR_INSUFFICIENT_BUFFER||!n)return L"NO_PACKAGE_IDENTITY";std::vector<wchar_t>b(n);rc=fn(t,&n,b.data());return rc==ERROR_SUCCESS?std::wstring(b.data()):L"QUERY_FAILED";}
static std::wstring ProcessPath(HANDLE p){wchar_t b[32768]{};DWORD n=32768;return QueryFullProcessImageNameW(p,0,b,&n)?std::wstring(b,n):L"UNAVAILABLE";}
static bool Same(const std::wstring&a,const std::wstring&b){return a!=L"UNAVAILABLE"&&a!=L"NO_PACKAGE_IDENTITY"&&a!=L"API_UNAVAILABLE"&&a!=L"QUERY_FAILED"&&a==b;}
static std::string ToUtf8(const std::wstring&w){int n=WideCharToMultiByte(CP_UTF8,0,w.c_str(),-1,nullptr,0,nullptr,nullptr);if(n<=1)return {};std::string s(n,'\0');WideCharToMultiByte(CP_UTF8,0,w.c_str(),-1,&s[0],n,nullptr,nullptr);s.pop_back();return s;}
static std::string MessageFor(const std::wstring&mode,const std::wstring&family,const std::wstring&sid,const std::wstring&full){
 std::wstring f=family,s=sid,ps=sid,pfn=full,hs=L"MSIX_TEST_CLIENT_HELLO";
 if(mode==L"spoof-package")f=L"Fake.Package_1234567890";
 if(mode==L"spoof-package-full")pfn=L"Fake.Package_1234567890_1.0.0.0_x64__fake";
 if(mode==L"spoof-appcontainer")s=L"S-1-15-2-111-222-333";
 if(mode==L"spoof-package-sid")ps=L"S-1-15-2-111-222-333";
 if(mode==L"spoof-handshake")hs=L"FORGED_HANDSHAKE";
 if(mode==L"spoof-all"){f=L"Fake.Package_1234567890";pfn=L"Fake.Package_1234567890_1.0.0.0_x64__fake";s=L"S-1-15-2-111-222-333";ps=L"S-1-15-2-111-222-333";hs=L"FORGED_HANDSHAKE";}
 std::wstring w=L"CLAIMED_PACKAGE_FAMILY="+f+L";CLAIMED_PACKAGE_FULL_NAME="+pfn+L";CLAIMED_APPCONTAINER_SID="+s+L";CLAIMED_PACKAGE_SID="+ps+L";HANDSHAKE="+hs;
 return ToUtf8(w);
}
static int Client(const std::wstring&mode,const std::wstring&family,const std::wstring&sid,const std::wstring&full,const std::wstring&pipeName){
 PIPE_NAME=pipeName;
 HANDLE h=CreateFileW(PIPE_NAME.c_str(),GENERIC_READ|GENERIC_WRITE,0,nullptr,OPEN_EXISTING,0,nullptr);
 if(h==INVALID_HANDLE_VALUE){wprintf(L"CLIENT_PIPE_OPEN=DENY ERROR=%lu MODE=%s\n",GetLastError(),mode.c_str());return 20;}
 std::string msg=MessageFor(mode,family,sid,full);DWORD sent=0;BOOL wr=WriteFile(h,msg.data(),(DWORD)msg.size(),&sent,nullptr);
 char ack[32]{};DWORD got=0;BOOL rd=wr&&ReadFile(h,ack,sizeof(ack)-1,&got,nullptr);
 wprintf(L"CLIENT_PIPE_OPEN=ALLOW WRITE=%u ACK=%u MODE=%s\n",wr,rd,mode.c_str());CloseHandle(h);return rd?0:21;
}
static int Supervisor(const std::wstring&expectedFamily,const std::wstring&expectedSid,const std::wstring&expectedFull,const std::wstring&expectedPath,const std::wstring&caseName,const std::wstring&pipeName){
 PIPE_NAME=pipeName;
 HANDLE self=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&self))return 4;
 std::wstring userSid=TokenSid(self,TokenUser);CloseHandle(self);
 std::wstring sddl=L"D:P(A;;GA;;;"+userSid+L")(A;;GA;;;"+expectedSid+L")S:(ML;;NW;;;LW)";
 PSECURITY_DESCRIPTOR sd=nullptr;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&sd,nullptr)){wprintf(L"SDDL_FAIL=%lu\n",GetLastError());return 6;}
 SECURITY_ATTRIBUTES sa{sizeof(sa),sd,FALSE};
 HANDLE pipe=CreateNamedPipeW(PIPE_NAME.c_str(),PIPE_ACCESS_DUPLEX,PIPE_TYPE_MESSAGE|PIPE_READMODE_MESSAGE|PIPE_WAIT|PIPE_REJECT_REMOTE_CLIENTS,1,4096,4096,10000,&sa);LocalFree(sd);
 if(pipe==INVALID_HANDLE_VALUE){wprintf(L"PIPE_CREATE_FAIL=%lu\n",GetLastError());return 7;}
 wprintf(L"SUPERVISOR_READY CASE=%s USER_SID=%s EXPECTED_APPCONTAINER_SID=%s DACL_SAME_USER_LIMITATION=TRUE\n",caseName.c_str(),userSid.c_str(),expectedSid.c_str());fflush(stdout);
 BOOL connected=ConnectNamedPipe(pipe,nullptr)?TRUE:(GetLastError()==ERROR_PIPE_CONNECTED);
 if(!connected){wprintf(L"PIPE_CONNECT_FAIL=%lu\n",GetLastError());CloseHandle(pipe);return 8;}
 ULONG clientPid=0;BOOL pidOk=GetNamedPipeClientProcessId(pipe,&clientPid);
 char msg[1024]{};DWORD got=0;BOOL rd=ReadFile(pipe,msg,sizeof(msg)-1,&got,nullptr);
 BOOL imp=rd&&ImpersonateNamedPipeClient(pipe);HANDLE threadToken=nullptr;BOOL opened=imp&&OpenThreadToken(GetCurrentThread(),TOKEN_QUERY,TRUE,&threadToken);
 DWORD app=opened?TokenApp(threadToken):0;
 std::wstring observedUser=opened?TokenSid(threadToken,TokenUser):L"UNAVAILABLE";
 std::wstring observedSid=opened?TokenSid(threadToken,TokenAppContainerSid):L"UNAVAILABLE";
 std::wstring observedFull=opened?PackageValue(threadToken,true):L"UNAVAILABLE";
 std::wstring observedFamily=opened?PackageValue(threadToken,false):L"UNAVAILABLE";
 HANDLE process=nullptr,processToken=nullptr;std::wstring processPath=L"UNAVAILABLE",processUser=L"UNAVAILABLE",processSid=L"UNAVAILABLE",processFull=L"UNAVAILABLE",processFamily=L"UNAVAILABLE";DWORD processApp=0;BOOL processTokenOk=FALSE;
 if(pidOk&&clientPid){process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,clientPid);if(process){processPath=ProcessPath(process);processTokenOk=OpenProcessToken(process,TOKEN_QUERY,&processToken);if(processTokenOk){processUser=TokenSid(processToken,TokenUser);processSid=TokenSid(processToken,TokenAppContainerSid);processApp=TokenApp(processToken);processFull=PackageValue(processToken,true);processFamily=PackageValue(processToken,false);}}}
 bool correlated=pidOk&&clientPid&&process&&processTokenOk&&Same(observedUser,processUser)&&app==processApp&&observedSid==processSid&&observedFull==processFull&&observedFamily==processFamily;
 bool pathMatch=processPath!=L"UNAVAILABLE"&&_wcsicmp(processPath.c_str(),expectedPath.c_str())==0;
 bool allow=opened&&correlated&&pathMatch&&app==1&&Same(observedSid,expectedSid)&&Same(observedFamily,expectedFamily)&&Same(observedFull,expectedFull);
 wprintf(L"PIPE_CLIENT_PID=%lu PID_API_OK=%u\n",clientPid,pidOk);
 wprintf(L"TOKEN_USER_SID=%s\nTOKEN_IS_APPCONTAINER=%u\nOBSERVED_APPCONTAINER_SID=%s\nOBSERVED_PACKAGE_FULL_NAME=%s\nOBSERVED_PACKAGE_FAMILY_NAME=%s\n",observedUser.c_str(),app,observedSid.c_str(),observedFull.c_str(),observedFamily.c_str());
 wprintf(L"PROCESS_IMAGE=%s\nPROCESS_TOKEN_USER_SID=%s\nPROCESS_TOKEN_IS_APPCONTAINER=%u\nPROCESS_TOKEN_APPCONTAINER_SID=%s\nPROCESS_TOKEN_PACKAGE_FULL_NAME=%s\nPROCESS_TOKEN_PACKAGE_FAMILY_NAME=%s\n",processPath.c_str(),processUser.c_str(),processApp,processSid.c_str(),processFull.c_str(),processFamily.c_str());
 wprintf(L"TOKEN_PROCESS_CORRELATION=%s\nPROCESS_INSTALL_PATH_MATCH=%s\nEXPECTED_PROCESS_PATH=%s\nEXPECTED_APPCONTAINER_SID=%s\nEXPECTED_PACKAGE_FULL_NAME=%s\nPACKAGE_IDENTITY_SOURCE=Windows token package APIs; expected values from Windows package registration\nAUTHORIZATION_RESULT=%s\nCLIENT_MESSAGE=%S\n",correlated?L"PASS":L"FAIL",pathMatch?L"TRUE":L"FALSE",expectedPath.c_str(),expectedSid.c_str(),expectedFull.c_str(),allow?L"ALLOW":L"REJECT",msg);
 Sleep(2000);
 const char* ack="ACK";DWORD wrote=0;WriteFile(pipe,ack,3,&wrote,nullptr);FlushFileBuffers(pipe);
 if(processToken)CloseHandle(processToken);if(process)CloseHandle(process);if(threadToken)CloseHandle(threadToken);if(imp)RevertToSelf();DisconnectNamedPipe(pipe);CloseHandle(pipe);
 return allow?0:10;
}
int wmain(int argc,wchar_t**argv){
 if(argc>1&&wcscmp(argv[1],L"--supervisor")==0){if(argc<8)return 10;return Supervisor(argv[2],argv[3],argv[4],argv[5],argv[6],argv[7]);}
 if(argc>1&&wcscmp(argv[1],L"--adversary")==0){if(argc<7)return 11;return Client(argv[2],argv[3],argv[4],argv[5],argv[6]);}
 std::wstring family=argc>1?argv[1]:L"",sid=argc>2?argv[2]:L"",full=argc>3?argv[3]:L"",pipeName=argc>4?argv[4]:PIPE_NAME,mode=argc>5?argv[5]:L"legit";
 return Client(mode,family,sid,full,pipeName);
}