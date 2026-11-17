#pragma once
#include <windows.h>
#include <string>
#include "SupervisorCredentialStoreR5.h"
namespace phone_supervisor::r5 {
inline constexpr wchar_t kCredentialProvisioningPipe[] = L"\\\\.\\pipe\\TokenSaver.Phone02.CredentialProvisioning.R5";
bool ProvisionClientSecretThroughAdminPipe(
    SupervisorCredentialStore& store,const std::wstring& serviceSid,HANDLE stopEvent,HANDLE readyEvent,std::wstring& result);
}
