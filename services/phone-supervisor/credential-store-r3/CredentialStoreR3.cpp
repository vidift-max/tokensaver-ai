#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <ncrypt.h>
#include <sddl.h>
#include <bcrypt.h>
#include <vector>
#include <fstream>
#include <string>
#include <cstdio>
#pragma comment(lib,"ncrypt.lib")
#pragma comment(lib,"bcrypt.lib")
// DPAPI-NG declarations: some hosted Windows SDK images omit ncryptprotect.h.
// These declarations match the documented ncrypt.dll ABI.
using NCRYPT_DESCRIPTOR_HANDLE = HANDLE;
extern "C" SECURITY_STATUS WINAPI NCryptCreateProtectionDescriptor(LPCWSTR,DWORD,NCRYPT_DESCRIPTOR_HANDLE*);
extern "C" SECURITY_STATUS WINAPI NCryptCloseProtectionDescriptor(NCRYPT_DESCRIPTOR_HANDLE);
extern "C" SECURITY_STATUS WINAPI NCryptProtectSecret(NCRYPT_DESCRIPTOR_HANDLE,DWORD,const BYTE*,ULONG,const void*,HWND,BYTE**,ULONG*);
extern "C" SECURITY_STATUS WINAPI NCryptUnprotectSecret(NCRYPT_DESCRIPTOR_HANDLE*,DWORD,const BYTE*,ULONG,const void*,HWND,BYTE**,ULONG*);
static const wchar_t* SERVICE_NAME=L"TokenSaverPhone02Supervisor";
static const wchar_t* SERVICE_ACCOUNT=L"NT SERVICE\\TokenSaverPhone02Supervisor";
static const wchar_t* ROOT=L"C:\\ProgramData\\Phone02CredentialStoreR3";
static const wchar_t* STORE=L"C:\\ProgramData\\Phone02CredentialStoreR3\\credential.bin";
static void logline(const char* s){std::ofstream f("C:\\ProgramData\\Phone02CredentialStoreR3\\result.txt",std::ios::app);f<<s<<"\n";}
static bool sidDescriptor(std::wstring& d){DWORD n=0,m=0;SID_NAME_USE u;LookupAccountNameW(nullptr,SERVICE_ACCOUNT,nullptr,&n,nullptr,&m,&u);std::vector<BYTE>b(n);if(!LookupAccountNameW(nullptr,SERVICE_NAME,b.data(),&n,nullptr,&m,&u))return false;LPWSTR p=nullptr;if(!ConvertSidToStringSidW(b.data(),&p))return false;d=L"SID=";d+=p;LocalFree(p);return true;}
static bool protect(const std::vector<BYTE>& x,std::vector<BYTE>& y){std::wstring d;if(!sidDescriptor(d))return false;NCRYPT_DESCRIPTOR_HANDLE h=nullptr;PBYTE p=nullptr;ULONG n=0;auto s=NCryptCreateProtectionDescriptor(d.c_str(),0,&h);if(s==0)s=NCryptProtectSecret(h,0,(PBYTE)x.data(),(ULONG)x.size(),nullptr,nullptr,&p,&n);if(h)NCryptCloseProtectionDescriptor(h);if(s!=0)return false;y.assign(p,p+n);LocalFree(p);return true;}
static bool unprotect(const std::vector<BYTE>& x,std::vector<BYTE>& y){NCRYPT_DESCRIPTOR_HANDLE h=nullptr;PBYTE p=nullptr;ULONG n=0;auto s=NCryptUnprotectSecret(&h,0,(PBYTE)x.data(),(ULONG)x.size(),nullptr,nullptr,&p,&n);if(h)NCryptCloseProtectionDescriptor(h);if(s!=0)return false;y.assign(p,p+n);LocalFree(p);return true;}
static bool readblob(std::vector<BYTE>& x){std::ifstream f(STORE,std::ios::binary);if(!f)return false;x.assign(std::istreambuf_iterator<char>(f),{});return !x.empty();}
static bool writeblob(const std::vector<BYTE>& x){std::ofstream f(STORE,std::ios::binary|std::ios::trunc);f.write((const char*)x.data(),x.size());return f.good();}
static bool positive(){HANDLE t=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&t))return false;DWORD n=0;GetTokenInformation(t,TokenUser,nullptr,0,&n);std::vector<BYTE>userBuf(n);bool u=GetTokenInformation(t,TokenUser,userBuf.data(),n,&n)!=0;LPWSTR us=nullptr;if(u)ConvertSidToStringSidW(((TOKEN_USER*)userBuf.data())->User.Sid,&us);DWORD app=1,z=0;GetTokenInformation(t,TokenIsAppContainer,&app,sizeof(app),&z);DWORD gn=0;GetTokenInformation(t,TokenGroups,nullptr,0,&gn);std::vector<BYTE>g(gn);bool gok=GetTokenInformation(t,TokenGroups,g.data(),gn,&gn)!=0;std::wstring ds;sidDescriptor(ds);DWORD sn=0,dom=0;SID_NAME_USE use;LookupAccountNameW(nullptr,SERVICE_NAME,nullptr,&sn,nullptr,&dom,&use);std::vector<BYTE>sb(sn);bool found=false;if(sn&&LookupAccountNameW(nullptr,SERVICE_NAME,sb.data(),&sn,nullptr,&dom,&use)&&gok){auto* tg=(TOKEN_GROUPS*)g.data();for(DWORD i=0;i<tg->GroupCount;i++)if(EqualSid(tg->Groups[i].Sid,sb.data())&&(tg->Groups[i].Attributes&SE_GROUP_ENABLED))found=true;}char facts[512];char user[200]="UNKNOWN";if(us)WideCharToMultiByte(CP_UTF8,0,us,-1,user,sizeof(user),nullptr,nullptr);sprintf_s(facts,"TOKEN_USER=%s SERVICE_SID=%s TOKEN_IS_APPCONTAINER=%lu",user,found?"ENABLED":"MISSING",app);logline(facts);if(us)LocalFree(us);CloseHandle(t);if(!found||app!=0)return false;
std::vector<BYTE>a(48),b(48),c,blob,plain;if(BCryptGenRandom(nullptr,a.data(),48,BCRYPT_USE_SYSTEM_PREFERRED_RNG)||!protect(a,blob)||!writeblob(blob))return false;logline("WRITE=PASS");if(!readblob(blob)||!unprotect(blob,plain)||plain!=a)return false;logline("READ=PASS");if(BCryptGenRandom(nullptr,b.data(),48,BCRYPT_USE_SYSTEM_PREFERRED_RNG)||!protect(b,blob)||!writeblob(blob))return false;logline("REPLACE=PASS");if(!readblob(blob)||!unprotect(blob,plain)||plain!=b)return false;logline("READ_NEW=PASS");if(plain==a)return false;logline("VERIFY_OLD_NOT_CURRENT=PASS");SecureZeroMemory(a.data(),a.size());SecureZeroMemory(b.data(),b.size());SecureZeroMemory(plain.data(),plain.size());if(!DeleteFileW(STORE))return false;logline("REMOVE=PASS");if(readblob(blob))return false;logline("READ_AFTER_REMOVE=PASS");logline("MISSING_CREDENTIAL=PASS");return true;}
static SERVICE_STATUS_HANDLE sh;static HANDLE stopEvent;
static DWORD WINAPI handler(DWORD c,DWORD,LPVOID,LPVOID){if(c==SERVICE_CONTROL_STOP)SetEvent(stopEvent);return NO_ERROR;}
static void WINAPI mainSvc(DWORD,LPWSTR*){sh=RegisterServiceCtrlHandlerExW(SERVICE_NAME,handler,nullptr);if(!sh)return;SERVICE_STATUS s{};s.dwServiceType=SERVICE_WIN32_OWN_PROCESS;s.dwCurrentState=SERVICE_START_PENDING;s.dwControlsAccepted=SERVICE_ACCEPT_STOP;SetServiceStatus(sh,&s);stopEvent=CreateEventW(nullptr,TRUE,FALSE,nullptr);bool ok=positive();s.dwCurrentState=SERVICE_RUNNING;s.dwWin32ExitCode=ok?0:ERROR_ACCESS_DENIED;SetServiceStatus(sh,&s);WaitForSingleObject(stopEvent,1000);s.dwCurrentState=SERVICE_STOPPED;SetServiceStatus(sh,&s);}
int wmain(){SERVICE_TABLE_ENTRYW t[]={{(LPWSTR)SERVICE_NAME,mainSvc},{nullptr,nullptr}};return StartServiceCtrlDispatcherW(t)?0:(int)GetLastError();}
