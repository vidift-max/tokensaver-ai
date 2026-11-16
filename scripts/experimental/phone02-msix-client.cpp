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
#include <coroutine>
#include <winrt/Windows.Foundation.h>
#include <winrt/base.h>
#include <winrt/Windows.ApplicationModel.h>
#pragma comment(lib,"advapi32.lib")
#pragma comment(lib,"userenv.lib")
static std::wstring PIPE_NAME=L"\\\\.\\pipe\\LOCAL\\TokenSaver.Phone02.MsixPoA52881";
static std::wstring SidString(PSID sid){if(!sid||!IsValidSid(sid))return L"UNAVAILABLE";LPWSTR p=nullptr;if(!ConvertSidToStringSidW(sid,&p))return L"UNAVAILABLE";std::wstring s=p;LocalFree(p);return s;}
static std::wstring TokenSid(HANDLE t,TOKEN_INFORMATION_CLASS c){DWORD n=0;GetTokenInformation(t,c,nullptr,0,&n);if(!n)return L"UNAVAILABLE";std::vector<BYTE>b(n);if(!GetTokenInformation(t,c,b.data(),n,&n))return L"UNAVAILABLE";PSID sid=c==TokenUser?reinterpret_cast<TOKEN_USER*>(b.data())->User.Sid:reinterpret_cast<TOKEN_APPCONTAINER_INFORMATION*>(b.data())->TokenAppContainer;return SidString(sid);}
static DWORD TokenApp(HANDLE t){DWORD v=0,n=0;return GetTokenInformation(t,TokenIsAppContainer,&v,sizeof(v),&n)?v:0;}
static std::wstring PackageValue(HANDLE t,bool full){HMODULE mod=GetModuleHandleW(L"kernel32.dll");const char* name=full?"GetPackageFullNameFromToken":"GetPackageFamilyNameFromToken";using QueryFn=LONG(WINAPI*)(HANDLE,UINT32*,PWSTR);auto fn=reinterpret_cast<QueryFn>(GetProcAddress(mod,name));if(!fn)return L"API_UNAVAILABLE";UINT32 n=0;LONG rc=fn(t,&n,nullptr);if(rc!=ERROR_INSUFFICIENT_BUFFER||!n)return L"NO_PACKAGE_IDENTITY";std::vector<wchar_t>b(n);rc=fn(t,&n,b.data());return rc==ERROR_SUCCESS?std::wstring(b.data()):L"QUERY_FAILED";}
static std::wstring ProcessPath(HANDLE p){wchar_t b[32768]{};DWORD n=32768;return QueryFullProcessImageNameW(p,0,b,&n)?std::wstring(b,n):L"UNAVAILABLE";}
static std::wstring PackageValueFromProcess(HANDLE p,bool full){UINT32 n=0;LONG rc=full?GetPackageFullName(p,&n,nullptr):GetPackageFamilyName(p,&n,nullptr);if(rc==APPMODEL_ERROR_NO_PACKAGE)return L"NO_PACKAGE_IDENTITY";if(rc!=ERROR_INSUFFICIENT_BUFFER||!n)return L"QUERY_FAILED_"+std::to_wstring(rc);std::vector<wchar_t>b(n);rc=full?GetPackageFullName(p,&n,b.data()):GetPackageFamilyName(p,&n,b.data());return rc==ERROR_SUCCESS?std::wstring(b.data()):L"QUERY_FAILED_"+std::to_wstring(rc);}
static DWORD SessionId(DWORD pid){DWORD id=0;return ProcessIdToSessionId(pid,&id)?id:0xFFFFFFFF;}
static bool Same(const std::wstring&a,const std::wstring&b){return a!=L"UNAVAILABLE"&&a!=L"NO_PACKAGE_IDENTITY"&&a!=L"API_UNAVAILABLE"&&a!=L"QUERY_FAILED"&&a==b;}
static std::string ToUtf8(const std::wstring&w){int n=WideCharToMultiByte(CP_UTF8,0,w.c_str(),-1,nullptr,0,nullptr,nullptr);if(n<=1)return {};std::string s(n,'\0');WideCharToMultiByte(CP_UTF8,0,w.c_str(),-1,&s[0],n,nullptr,nullptr);s.pop_back();return s;}
static std::string ClaimValue(const std::string&message,const char*key){std::string prefix=std::string(key)+"=";size_t p=message.find(prefix);if(p==std::string::npos)return {};p+=prefix.size();size_t e=message.find(';',p);return message.substr(p,e==std::string::npos?std::string::npos:e-p);}
static void ClientDiagnostic(const std::wstring& message){
 wchar_t base[32768]{};DWORD n=GetEnvironmentVariableW(L"LOCALAPPDATA",base,32768);
 if(!n||n>=32768)return;
 std::wstring path=std::wstring(base)+L"\\Phone02-MsixPoA-client-"+std::to_wstring(GetCurrentProcessId())+L".log";
 HANDLE file=CreateFileW(path.c_str(),FILE_APPEND_DATA,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
 if(file==INVALID_HANDLE_VALUE)return;
 std::wstring line=message+L"\r\n";int bytes=WideCharToMultiByte(CP_UTF8,0,line.c_str(),(int)line.size(),nullptr,0,nullptr,nullptr);
 if(bytes>0){std::string utf8(bytes,0);WideCharToMultiByte(CP_UTF8,0,line.c_str(),(int)line.size(),&utf8[0],bytes,nullptr,nullptr);DWORD written=0;WriteFile(file,utf8.data(),(DWORD)utf8.size(),&written,nullptr);}
 CloseHandle(file);
}
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
static int Supervisor(const std::wstring&,const std::wstring&,const std::wstring&,const std::wstring&,const std::wstring&,const std::wstring&,DWORD);
static bool LaunchPackageSupervisor(const std::wstring&pipeName,DWORD clientPid){
 try{
  winrt::init_apartment(winrt::apartment_type::single_threaded);
  std::wstring args=L"--package-supervisor "+std::to_wstring(clientPid)+L" \""+pipeName+L"\"";
  auto operation=winrt::Windows::ApplicationModel::FullTrustProcessLauncher::LaunchFullTrustProcessForAppWithArgumentsAsync(L"Supervisor",args);
  HANDLE completed=CreateEventW(nullptr,TRUE,FALSE,nullptr);if(!completed){ClientDiagnostic(L"FULLTRUST_SUPERVISOR_LAUNCH=FAIL EVENT_ERROR="+std::to_wstring(GetLastError()));return false;}
  static unsigned int launchResult=0xFFFFFFFF;static HRESULT launchError=S_OK;
  operation.Completed([completed](auto const&op,winrt::Windows::Foundation::AsyncStatus status){
   if(status==winrt::Windows::Foundation::AsyncStatus::Completed){try{auto result=op.GetResults();launchResult=static_cast<unsigned int>(result.LaunchResult());}catch(winrt::hresult_error const&e){launchError=e.code().value;}}
   else launchError=E_ABORT;
   SetEvent(completed);
  });
  DWORD wait=WaitForSingleObject(completed,15000);CloseHandle(completed);
  if(wait!=WAIT_OBJECT_0){ClientDiagnostic(L"FULLTRUST_SUPERVISOR_LAUNCH=TIMEOUT");return false;}
  if(FAILED(launchError)||launchResult!=0){ClientDiagnostic(L"FULLTRUST_SUPERVISOR_LAUNCH=FAIL HRESULT="+std::to_wstring((unsigned long)launchError)+L" RESULT="+std::to_wstring(launchResult));return false;}
  ClientDiagnostic(L"FULLTRUST_SUPERVISOR_LAUNCH=SUCCESS API=FullTrustProcessLauncher PID=OS_ACTIVATION_RESULT_NOT_EXPOSED CLIENT_PID="+std::to_wstring(clientPid));return true;
 }catch(const winrt::hresult_error&e){
  ClientDiagnostic(L"FULLTRUST_SUPERVISOR_LAUNCH=FAIL HRESULT="+std::to_wstring((unsigned long)e.code().value));return false;
 }
}
static int PackageSupervisor(DWORD expectedPid,const std::wstring&pipeName){
 std::wstring family=PackageValueFromProcess(GetCurrentProcess(),false),full=PackageValueFromProcess(GetCurrentProcess(),true);
 if(family==L"NO_PACKAGE_IDENTITY"||full==L"NO_PACKAGE_IDENTITY"||family.empty()||full.empty())return 41;
 UINT32 n=0;LONG rc=GetPackagePathByFullName(full.c_str(),&n,nullptr);if(rc!=ERROR_INSUFFICIENT_BUFFER||!n)return 42;
 std::vector<wchar_t> pathBuffer(n);rc=GetPackagePathByFullName(full.c_str(),&n,pathBuffer.data());if(rc!=ERROR_SUCCESS)return 43;
 std::wstring installPath(pathBuffer.data());if(!installPath.empty()&&installPath.back()!=L'\\')installPath+=L'\\';
 std::wstring expectedPath=installPath+L"client.exe";PSID appSid=nullptr;
 LONG sidRc=DeriveAppContainerSidFromAppContainerName(family.c_str(),&appSid);if(sidRc!=ERROR_SUCCESS||!appSid)return 44;
 std::wstring sid=SidString(appSid);FreeSid(appSid);
 wchar_t local[32768]{};DWORD localLen=GetEnvironmentVariableW(L"LOCALAPPDATA",local,32768);
 if(localLen&&localLen<32768){std::wstring logPath=std::wstring(local)+L"\\Phone02-MsixPoA-supervisor-"+std::to_wstring(expectedPid)+L".log";FILE* fp=nullptr;_wfreopen_s(&fp,logPath.c_str(),L"w",stdout);setvbuf(stdout,nullptr,_IONBF,0);}
 HANDLE selfToken=nullptr;DWORD selfApp=0;if(OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&selfToken)){selfApp=TokenApp(selfToken);CloseHandle(selfToken);}
 wprintf(L"PACKAGE_SUPERVISOR_PROCESS=TRUE PID=%lu SESSION=%lu TOKEN_IS_APPCONTAINER=%u PACKAGE_FAMILY=%s PACKAGE_FULL_NAME=%s EXPECTED_CLIENT_PID=%lu EXPECTED_CLIENT_PATH=%s EXPECTED_APPCONTAINER_SID=%s PIPE=%s\n",GetCurrentProcessId(),SessionId(GetCurrentProcessId()),selfApp,family.c_str(),full.c_str(),expectedPid,expectedPath.c_str(),sid.c_str(),pipeName.c_str());
 return Supervisor(family,sid,full,expectedPath,L"same-package-fulltrust",pipeName,expectedPid);
}
static int Client(const std::wstring&mode,const std::wstring&family,const std::wstring&sid,const std::wstring&full,const std::wstring&pipeName,bool launchSibling=false){
 PIPE_NAME=pipeName;
 ClientDiagnostic(L"CLIENT_MAIN_ENTER PID="+std::to_wstring(GetCurrentProcessId())+L" SESSION="+std::to_wstring(SessionId(GetCurrentProcessId()))+L" MODE="+mode+L" PIPE="+PIPE_NAME);
 ClientDiagnostic(L"CLIENT_PROCESS_IMAGE="+ProcessPath(GetCurrentProcess()));
 if(launchSibling&&!LaunchPackageSupervisor(PIPE_NAME,GetCurrentProcessId()))return 22;
 HANDLE h=INVALID_HANDLE_VALUE;
 for(int attempt=0;attempt<150;attempt++){
   h=CreateFileW(PIPE_NAME.c_str(),GENERIC_READ|GENERIC_WRITE,0,nullptr,OPEN_EXISTING,0,nullptr);
   if(h!=INVALID_HANDLE_VALUE)break;
   DWORD error=GetLastError();
   if(attempt==0)ClientDiagnostic(L"CLIENT_PIPE_FIRST_ERROR="+std::to_wstring(error)+L" MODE="+mode+L" PIPE="+PIPE_NAME);
   if(error!=ERROR_PIPE_BUSY&&error!=ERROR_FILE_NOT_FOUND&&error!=ERROR_PATH_NOT_FOUND)break;
   WaitNamedPipeW(PIPE_NAME.c_str(),100);
   Sleep(100);
 }
 if(h==INVALID_HANDLE_VALUE){DWORD error=GetLastError();wprintf(L"CLIENT_PIPE_OPEN=DENY ERROR=%lu MODE=%s PIPE=%s\\n",error,mode.c_str(),PIPE_NAME.c_str());ClientDiagnostic(L"CLIENT_PIPE_OPEN=DENY ERROR="+std::to_wstring(error)+L" MODE="+mode+L" PIPE="+PIPE_NAME);return 20;}
 ClientDiagnostic(L"CLIENT_PIPE_OPEN=ALLOW MODE="+mode+L" PIPE="+PIPE_NAME);
 std::string msg=MessageFor(mode,family,sid,full);DWORD sent=0;BOOL wr=WriteFile(h,msg.data(),(DWORD)msg.size(),&sent,nullptr);
 char ack[32]{};DWORD got=0;BOOL rd=wr&&ReadFile(h,ack,sizeof(ack)-1,&got,nullptr);
 ClientDiagnostic(L"CLIENT_PIPE_RESULT WRITE="+std::to_wstring(wr)+L" SENT="+std::to_wstring(sent)+L" ACK="+std::to_wstring(rd));
 wprintf(L"CLIENT_PIPE_OPEN=ALLOW WRITE=%u ACK=%u MODE=%s\\n",wr,rd,mode.c_str());CloseHandle(h);return rd?0:21;
}
static int Supervisor(const std::wstring&expectedFamily,const std::wstring&expectedSid,const std::wstring&expectedFull,const std::wstring&expectedPath,const std::wstring&caseName,const std::wstring&pipeName,DWORD expectedPid){
 PIPE_NAME=pipeName;
 HANDLE self=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&self))return 4;
 std::wstring userSid=TokenSid(self,TokenUser);CloseHandle(self);
 std::wstring aclSid=expectedSid,activationSid=L"UNAVAILABLE",activationFamily=L"UNAVAILABLE",activationFull=L"UNAVAILABLE";DWORD activationApp=0;
 if(expectedPid){
   HANDLE activated=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,expectedPid);
   if(!activated){wprintf(L"ACTIVATED_PROCESS_OPEN_FAIL PID=%lu ERROR=%lu\n",expectedPid,GetLastError());return 31;}
   HANDLE activatedToken=nullptr;
   if(!OpenProcessToken(activated,TOKEN_QUERY,&activatedToken)){wprintf(L"ACTIVATED_TOKEN_OPEN_FAIL PID=%lu ERROR=%lu\n",expectedPid,GetLastError());CloseHandle(activated);return 32;}
   activationApp=TokenApp(activatedToken);activationSid=TokenSid(activatedToken,TokenAppContainerSid);
   activationFamily=PackageValueFromProcess(activated,false);activationFull=PackageValueFromProcess(activated,true);
   CloseHandle(activatedToken);CloseHandle(activated);
   wprintf(L"ACTIVATED_PROCESS_PID=%lu ACTIVATED_SESSION=%lu SUPERVISOR_PID=%lu SUPERVISOR_SESSION=%lu ACTIVATED_TOKEN_IS_APPCONTAINER=%u OBSERVED_APPCONTAINER_SID=%s OBSERVED_PACKAGE_FAMILY=%s OBSERVED_PACKAGE_FULL_NAME=%s\n",expectedPid,SessionId(expectedPid),GetCurrentProcessId(),SessionId(GetCurrentProcessId()),activationApp,activationSid.c_str(),activationFamily.c_str(),activationFull.c_str());
   if(activationApp==1&&activationSid!=L"UNAVAILABLE")aclSid=activationSid;else wprintf(L"EXPECTED_CLIENT_NOT_APPCONTAINER=TRUE OBSERVED_APP=%u OBSERVED_SID=%s\n",activationApp,activationSid.c_str());
 }
 std::wstring sddl=L"D:P(A;;GA;;;"+userSid+L")(A;;GA;;;"+aclSid+L")S:(ML;;NW;;;LW)";
 PSECURITY_DESCRIPTOR sd=nullptr;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&sd,nullptr)){wprintf(L"SDDL_FAIL=%lu\n",GetLastError());return 6;}
 SECURITY_ATTRIBUTES sa{sizeof(sa),sd,FALSE};
 HANDLE pipe=CreateNamedPipeW(PIPE_NAME.c_str(),PIPE_ACCESS_DUPLEX,PIPE_TYPE_MESSAGE|PIPE_READMODE_MESSAGE|PIPE_WAIT|PIPE_REJECT_REMOTE_CLIENTS,1,4096,4096,10000,&sa);LocalFree(sd);
 if(pipe==INVALID_HANDLE_VALUE){wprintf(L"PIPE_CREATE_FAIL=%lu\n",GetLastError());return 7;}
 wprintf(L"SUPERVISOR_READY CASE=%s PIPE=%s SUPERVISOR_SESSION=%lu EXPECTED_CLIENT_SESSION=%lu USER_SID=%s EXPECTED_APPCONTAINER_SID=%s ACL_APPCONTAINER_SID=%s EXPECTED_ACTIVATED_PID=%lu DACL_SAME_USER_LIMITATION=TRUE\n",caseName.c_str(),PIPE_NAME.c_str(),SessionId(GetCurrentProcessId()),expectedPid?SessionId(expectedPid):0xFFFFFFFF,userSid.c_str(),expectedSid.c_str(),aclSid.c_str(),expectedPid);fflush(stdout);
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
 if(pidOk&&clientPid){process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,clientPid);if(process){processPath=ProcessPath(process);processTokenOk=OpenProcessToken(process,TOKEN_QUERY,&processToken);if(processTokenOk){processUser=TokenSid(processToken,TokenUser);processSid=TokenSid(processToken,TokenAppContainerSid);processApp=TokenApp(processToken);processFull=PackageValueFromProcess(process,true);processFamily=PackageValueFromProcess(process,false);}}}
 observedFull=processFull;observedFamily=processFamily;
 std::string wireMessage(msg,rd?got:0);
 bool activationPidMatch=(expectedPid==0)||(pidOk&&clientPid==expectedPid);
 bool correlated=pidOk&&clientPid&&process&&processTokenOk&&Same(observedUser,processUser)&&app==processApp&&observedSid==processSid&&observedFull==processFull&&observedFamily==processFamily&&activationPidMatch;
 bool claimsMatch=ClaimValue(wireMessage,"CLAIMED_PACKAGE_FAMILY")==ToUtf8(observedFamily)&&ClaimValue(wireMessage,"CLAIMED_PACKAGE_FULL_NAME")==ToUtf8(observedFull)&&ClaimValue(wireMessage,"CLAIMED_APPCONTAINER_SID")==ToUtf8(observedSid);
 bool pathMatch=processPath!=L"UNAVAILABLE"&&_wcsicmp(processPath.c_str(),expectedPath.c_str())==0;
 bool allow=opened&&correlated&&claimsMatch&&pathMatch&&app==1&&Same(observedSid,expectedSid)&&Same(observedFamily,expectedFamily)&&Same(observedFull,expectedFull);
 wprintf(L"PIPE_CLIENT_PID=%lu PID_API_OK=%u EXPECTED_ACTIVATED_PID=%lu ACTIVATION_PID_MATCH=%s\n",clientPid,pidOk,expectedPid,activationPidMatch?L"TRUE":L"FALSE");
 wprintf(L"TOKEN_USER_SID=%s\nTOKEN_IS_APPCONTAINER=%u\nOBSERVED_APPCONTAINER_SID=%s\nOBSERVED_PACKAGE_FULL_NAME=%s\nOBSERVED_PACKAGE_FAMILY_NAME=%s\n",observedUser.c_str(),app,observedSid.c_str(),observedFull.c_str(),observedFamily.c_str());
 wprintf(L"PROCESS_IMAGE=%s\nPROCESS_TOKEN_USER_SID=%s\nPROCESS_TOKEN_IS_APPCONTAINER=%u\nPROCESS_TOKEN_APPCONTAINER_SID=%s\nPROCESS_TOKEN_PACKAGE_FULL_NAME=%s\nPROCESS_TOKEN_PACKAGE_FAMILY_NAME=%s\n",processPath.c_str(),processUser.c_str(),processApp,processSid.c_str(),processFull.c_str(),processFamily.c_str());
 wprintf(L"TOKEN_PROCESS_CORRELATION=%s\nPROTOCOL_CLAIMS_MATCH_OBSERVED=%s\nPACKAGE_SID_OBSERVED=NOT_AVAILABLE_IN_THIS_TOKEN_QUERY\nPROCESS_INSTALL_PATH_MATCH=%s\nEXPECTED_PROCESS_PATH=%s\nEXPECTED_APPCONTAINER_SID=%s\nEXPECTED_PACKAGE_FULL_NAME=%s\nPACKAGE_IDENTITY_SOURCE=GetPackageFullName/GetPackageFamilyName on Windows process handle; token separately queried\nAUTHORIZATION_RESULT=%s\nCLIENT_MESSAGE=%S\n",correlated?L"PASS":L"FAIL",claimsMatch?L"TRUE":L"FALSE",pathMatch?L"TRUE":L"FALSE",expectedPath.c_str(),expectedSid.c_str(),expectedFull.c_str(),allow?L"ALLOW":L"REJECT",msg);
 Sleep(2000);
 const char* ack="ACK";DWORD wrote=0;WriteFile(pipe,ack,3,&wrote,nullptr);FlushFileBuffers(pipe);
 if(processToken)CloseHandle(processToken);if(process)CloseHandle(process);if(threadToken)CloseHandle(threadToken);if(imp)RevertToSelf();DisconnectNamedPipe(pipe);CloseHandle(pipe);
 return allow?0:10;
}
int wmain(int argc,wchar_t**argv){
 if(argc>1&&wcscmp(argv[1],L"--supervisor")==0){if(argc<9)return 10;return Supervisor(argv[2],argv[3],argv[4],argv[5],argv[6],argv[7],(DWORD)_wtoi(argv[8]));}
 if(argc>1&&wcscmp(argv[1],L"--package-supervisor")==0){if(argc<4)return 12;return PackageSupervisor((DWORD)_wtoi(argv[2]),argv[3]);}
 if(argc>1&&wcscmp(argv[1],L"--adversary")==0){if(argc<7)return 11;return Client(argv[2],argv[3],argv[4],argv[5],argv[6],false);}
 std::wstring family=argc>1?argv[1]:L"",sid=argc>2?argv[2]:L"",full=argc>3?argv[3]:L"",mode=argc>4?argv[4]:L"legit";
 if(family.empty()||sid.empty()||full.empty()){
   HANDLE token=nullptr;
   if(OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token)){
     if(family.empty())family=PackageValue(token,false);
     if(full.empty())full=PackageValue(token,true);
     if(sid.empty())sid=TokenSid(token,TokenAppContainerSid);
     CloseHandle(token);
   }
 }
 std::wstring pipeName=L"\\\\.\\pipe\\LOCAL\\TokenSaver.Phone02.MsixPoA52881."+std::to_wstring(GetCurrentProcessId());
 return Client(mode,family,sid,full,pipeName,true);
}