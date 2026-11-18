#define WIN32_LEAN_AND_MEAN
#include "SupervisorCredentialProvisioningR5.h"
#include <sddl.h>
#include <cstring>
#pragma comment(lib,"advapi32.lib")

namespace phone_supervisor::r5 {
namespace {
bool ReadExact(HANDLE pipe,BYTE* out,DWORD size,HANDLE stopEvent) {
    DWORD total=0;
    while(total<size) {
        if(WaitForSingleObject(stopEvent,0)==WAIT_OBJECT_0)return false;
        DWORD read=0;
        if(ReadFile(pipe,out+total,size-total,&read,nullptr)) {
            if(read==0)return false;
            total+=read;continue;
        }
        DWORD error=GetLastError();
        if(error==ERROR_NO_DATA||error==ERROR_PIPE_LISTENING){Sleep(25);continue;}
        return false;
    }
    return true;
}
bool IsAdminOrSystemClient() {
    BYTE adminBytes[SECURITY_MAX_SID_SIZE]{};DWORD adminSize=sizeof(adminBytes);
    BYTE systemBytes[SECURITY_MAX_SID_SIZE]{};DWORD systemSize=sizeof(systemBytes);
    if(!CreateWellKnownSid(WinBuiltinAdministratorsSid,nullptr,adminBytes,&adminSize)||
       !CreateWellKnownSid(WinLocalSystemSid,nullptr,systemBytes,&systemSize))return false;
    BOOL admin=FALSE,system=FALSE;
    return CheckTokenMembership(nullptr,adminBytes,&admin)&&CheckTokenMembership(nullptr,systemBytes,&system)&&(admin||system);
}
}
bool ProvisionClientSecretThroughAdminPipe(
    SupervisorCredentialStore& store,const std::wstring& serviceSid,HANDLE stopEvent,HANDLE readyEvent,std::wstring& result) {
    if(serviceSid.rfind(L"S-1-5-80-",0)!=0){result=L"PROVISION_PIPE_SERVICE_SID_INVALID";return false;}
    std::wstring sddl=L"D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;S-1-5-19)(A;;GA;;;"+serviceSid+L")";
    PSECURITY_DESCRIPTOR descriptor=nullptr;
    if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&descriptor,nullptr)){
        result=L"PROVISION_PIPE_DACL_BUILD_FAILED";return false;
    }
    SECURITY_ATTRIBUTES sa{sizeof(sa),descriptor,FALSE};
    HANDLE pipe=CreateNamedPipeW(kCredentialProvisioningPipe,PIPE_ACCESS_DUPLEX|FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE|PIPE_READMODE_BYTE|PIPE_NOWAIT|PIPE_REJECT_REMOTE_CLIENTS,1,512,512,0,&sa);
    LocalFree(descriptor);
    if(pipe==INVALID_HANDLE_VALUE){result=L"PROVISION_PIPE_CREATE_FAILED";return false;}
    bool success=false;ULONGLONG deadline=GetTickCount64()+60000;
    while(GetTickCount64()<deadline&&WaitForSingleObject(stopEvent,0)==WAIT_TIMEOUT) {
        BOOL connected=ConnectNamedPipe(pipe,nullptr);
        DWORD connectError=connected?ERROR_SUCCESS:GetLastError();
        if(!connected&&connectError!=ERROR_PIPE_CONNECTED&&connectError!=ERROR_PIPE_LISTENING&&connectError!=ERROR_NO_DATA){
            result=L"PROVISION_PIPE_CONNECT_FAILED";break;
        }
        if(!connected&&(connectError==ERROR_PIPE_LISTENING||connectError==ERROR_NO_DATA)){
            Sleep(50);continue;
        }
        if(!ImpersonateNamedPipeClient(pipe)){result=L"PROVISION_PIPE_IMPERSONATION_FAILED";DisconnectNamedPipe(pipe);break;}
        bool authorized=IsAdminOrSystemClient();
        BOOL reverted=RevertToSelf();
        if(!reverted){
            DWORD revertError=GetLastError();
            result=L"PROVISION_PIPE_REVERT_FAILED_ERROR_"+std::to_wstring(revertError);
            DisconnectNamedPipe(pipe);break;
        }
        if(!authorized){
            result=L"PROVISION_PIPE_CALLER_NOT_ADMIN";DisconnectNamedPipe(pipe);break;
        }
        BYTE magic[4]{},lengthBytes[4]{};BYTE secretBytes[256]{};
        bool readOk=ReadExact(pipe,magic,4,stopEvent)&&ReadExact(pipe,lengthBytes,4,stopEvent);
        DWORD length=0;if(readOk)std::memcpy(&length,lengthBytes,sizeof(length));
        if(!readOk||std::memcmp(magic,"TSR5",4)!=0||length<32||length>sizeof(secretBytes)){
            result=L"PROVISION_PIPE_MESSAGE_INVALID";
        } else if(!ReadExact(pipe,secretBytes,length,stopEvent)) {
            result=L"PROVISION_PIPE_SECRET_READ_FAILED";
        } else {
            SecureBuffer secret;
            if(!secret.Assign(secretBytes,length))result=L"PROVISION_PIPE_SECURE_BUFFER_FAILED";
            else if(store.WriteClientSecret(secret,result)) {
                result=L"CREDENTIAL_PROVISIONED=PASS";
                success=true;
                SetEvent(readyEvent);
            }
            secret.Clear();
        }
        SecureZeroMemory(secretBytes,sizeof(secretBytes));
        DWORD ack=success?0:1,written=0;
        WriteFile(pipe,&ack,sizeof(ack),&written,nullptr);FlushFileBuffers(pipe);
        DisconnectNamedPipe(pipe);
        break;
    }
    if(!success&&result.empty())result=L"PROVISION_PIPE_TIMEOUT";
    CloseHandle(pipe);
    return success;
}
}
