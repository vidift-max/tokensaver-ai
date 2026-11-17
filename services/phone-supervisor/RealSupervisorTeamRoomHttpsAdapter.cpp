#include "RealSupervisorTeamRoomHttpsAdapter.h"
#include <winhttp.h>
#pragma comment(lib, "winhttp.lib")
namespace phone_supervisor {
namespace {
constexpr wchar_t kHost[] = L"team-room.p238.test";
constexpr INTERNET_PORT kPort = 8443;
constexpr wchar_t kPath[] = L"/health/live";
class Handle final {
public:
 explicit Handle(HINTERNET h=nullptr):h_(h){}
 ~Handle(){if(h_)WinHttpCloseHandle(h_);}
 Handle(const Handle&)=delete; Handle& operator=(const Handle&)=delete;
 HINTERNET get()const{return h_;}
private:HINTERNET h_;
};
std::wstring Error(const wchar_t* op,DWORD e){return std::wstring(op)+L" failed; WINHTTP_ERROR="+std::to_wstring(e);}
}
bool RealSupervisorTeamRoomHttpsAdapter::ValidateEndpoint(const std::wstring& url,std::wstring& reason){
 reason.clear();
 if(url.empty()||url.size()>2048){reason=L"ENDPOINT_EMPTY_OR_TOO_LONG";return false;}
 URL_COMPONENTS c{};c.dwStructSize=sizeof(c);c.dwSchemeLength=c.dwHostNameLength=c.dwUserNameLength=c.dwPasswordLength=c.dwUrlPathLength=c.dwExtraInfoLength=static_cast<DWORD>(-1);
 if(!WinHttpCrackUrl(url.c_str(),static_cast<DWORD>(url.size()),0,&c)){reason=Error(L"WinHttpCrackUrl",GetLastError());return false;}
 if(c.nScheme!=INTERNET_SCHEME_HTTPS){reason=L"HTTPS_REQUIRED_HTTP_REJECTED";return false;}
 if((c.lpszUserName&&c.dwUserNameLength)||(c.lpszPassword&&c.dwPasswordLength)){reason=L"ENDPOINT_USERINFO_FORBIDDEN";return false;}
 std::wstring host(c.lpszHostName,c.dwHostNameLength);
 if(CompareStringOrdinal(host.c_str(),static_cast<int>(host.size()),kHost,-1,TRUE)!=CSTR_EQUAL){reason=L"ENDPOINT_HOST_NOT_ALLOWLISTED";return false;}
 if(c.nPort!=kPort){reason=L"ENDPOINT_PORT_NOT_ALLOWLISTED";return false;}
 std::wstring path=c.lpszUrlPath?std::wstring(c.lpszUrlPath,c.dwUrlPathLength):L"/";
 if(path!=kPath){reason=L"ENDPOINT_PATH_NOT_ALLOWLISTED";return false;}
 if(c.lpszExtraInfo&&c.dwExtraInfoLength){reason=L"ENDPOINT_QUERY_OR_FRAGMENT_FORBIDDEN";return false;}
 return true;
}
bool RealSupervisorTeamRoomHttpsAdapter::ProbeHealth(const std::wstring& url,DWORD& status,std::wstring& reason)const{
 status=0;if(!ValidateEndpoint(url,reason))return false;
 URL_COMPONENTS c{};c.dwStructSize=sizeof(c);c.dwSchemeLength=c.dwHostNameLength=c.dwUrlPathLength=static_cast<DWORD>(-1);
 if(!WinHttpCrackUrl(url.c_str(),static_cast<DWORD>(url.size()),0,&c)){reason=Error(L"WinHttpCrackUrl",GetLastError());return false;}
 std::wstring host(c.lpszHostName,c.dwHostNameLength),path(c.lpszUrlPath,c.dwUrlPathLength);
 // OS chain and server-name validation stay enabled: no SECURITY_FLAGS override.
 Handle session(WinHttpOpen(L"TokenSaver-Phone02-Supervisor/P2.38-A",WINHTTP_ACCESS_TYPE_NO_PROXY,WINHTTP_NO_PROXY_NAME,WINHTTP_NO_PROXY_BYPASS,0));
 if(!session.get()){reason=Error(L"WinHttpOpen",GetLastError());return false;}
 DWORD protocols=WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2;
 if(!WinHttpSetOption(session.get(),WINHTTP_OPTION_SECURE_PROTOCOLS,&protocols,sizeof(protocols))){reason=Error(L"SECURE_PROTOCOLS",GetLastError());return false;}
 WinHttpSetTimeouts(session.get(),5000,5000,5000,5000);
 Handle connection(WinHttpConnect(session.get(),host.c_str(),c.nPort,0));
 if(!connection.get()){reason=Error(L"WinHttpConnect",GetLastError());return false;}
 Handle request(WinHttpOpenRequest(connection.get(),L"GET",path.c_str(),nullptr,WINHTTP_NO_REFERER,WINHTTP_DEFAULT_ACCEPT_TYPES,WINHTTP_FLAG_SECURE));
 if(!request.get()){reason=Error(L"WinHttpOpenRequest",GetLastError());return false;}
 DWORD redirect=WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
 if(!WinHttpSetOption(request.get(),WINHTTP_OPTION_REDIRECT_POLICY,&redirect,sizeof(redirect))){reason=Error(L"REDIRECT_POLICY",GetLastError());return false;}
 if(!WinHttpSendRequest(request.get(),WINHTTP_NO_ADDITIONAL_HEADERS,0,WINHTTP_NO_REQUEST_DATA,0,0,0)){reason=Error(L"HTTPS_SEND_TLS_OR_TRANSPORT",GetLastError());return false;}
 if(!WinHttpReceiveResponse(request.get(),nullptr)){reason=Error(L"HTTPS_RECEIVE_TLS_OR_TRANSPORT",GetLastError());return false;}
 DWORD bytes=sizeof(status);
 if(!WinHttpQueryHeaders(request.get(),WINHTTP_QUERY_STATUS_CODE|WINHTTP_QUERY_FLAG_NUMBER,WINHTTP_HEADER_NAME_BY_INDEX,&status,&bytes,WINHTTP_NO_HEADER_INDEX)){reason=Error(L"HTTP_STATUS",GetLastError());return false;}
 if(status!=200){reason=L"TEAM_ROOM_HEALTH_HTTP_STATUS_"+std::to_wstring(status);return false;}
 std::string body;
 for(;;){DWORD available=0;if(!WinHttpQueryDataAvailable(request.get(),&available)){reason=Error(L"QUERY_DATA",GetLastError());return false;}if(!available)break;
  if(available>4096||body.size()+available>4096){reason=L"HEALTH_RESPONSE_TOO_LARGE";return false;}
  size_t old=body.size();body.resize(old+available);DWORD got=0;
  if(!WinHttpReadData(request.get(),body.data()+old,available,&got)){reason=Error(L"READ_DATA",GetLastError());return false;}body.resize(old+got);if(!got)break;
 }
 if(body.find("\"service\":\"ai-team-room\"")==std::string::npos||body.find("\"status\":\"live\"")==std::string::npos){reason=L"TEAM_ROOM_HEALTH_BODY_UNEXPECTED";return false;}
 reason=L"OK";return true;
}
}
