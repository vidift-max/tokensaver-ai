#pragma once
#include <windows.h>
#include <string>
namespace phone_supervisor {
class RealSupervisorTeamRoomHttpsAdapter final {
public:
 static bool ValidateEndpoint(const std::wstring& endpoint, std::wstring& reason);
 bool ProbeHealth(const std::wstring& endpoint, DWORD& httpStatus, std::wstring& reason) const;
};
}
