#include "RealSupervisorTeamRoomHttpsAdapter.h"
#include <iostream>
static bool Check(const wchar_t* name,const std::wstring& url,bool expected){
 std::wstring reason;bool actual=phone_supervisor::RealSupervisorTeamRoomHttpsAdapter::ValidateEndpoint(url,reason);
 bool pass=actual==expected;std::wcout<<name<<L"="<<(pass?L"PASS":L"FAIL")<<L" reason="<<(reason.empty()?L"ALLOWLIST_MATCH":reason)<<L"\n";return pass;
}
int wmain(){bool ok=true;
 ok&=Check(L"ALLOWLIST_HTTPS",L"https://team-room.p238.test:8443/health/live",true);
 ok&=Check(L"REJECT_HTTP",L"http://team-room.p238.test:8443/health/live",false);
 ok&=Check(L"REJECT_HOST",L"https://attacker.invalid:8443/health/live",false);
 ok&=Check(L"REJECT_PORT",L"https://team-room.p238.test:9443/health/live",false);
 ok&=Check(L"REJECT_PATH",L"https://team-room.p238.test:8443/admin",false);
 ok&=Check(L"REJECT_QUERY",L"https://team-room.p238.test:8443/health/live?q=x",false);
 ok&=Check(L"REJECT_USERINFO",L"https://user@team-room.p238.test:8443/health/live",false);
 return ok?0:1;
}
