#define WIN32_LEAN_AND_MEAN
#include "SupervisorCredentialStoreR5.h"
#include <ncrypt.h>
#include <sddl.h>
#include <aclapi.h>
#include <algorithm>
#include <cstdio>
#include <limits>
#pragma comment(lib,"ncrypt.lib")
#pragma comment(lib,"advapi32.lib")

using NCRYPT_DESCRIPTOR_HANDLE = HANDLE;
extern "C" SECURITY_STATUS WINAPI NCryptCreateProtectionDescriptor(LPCWSTR,DWORD,NCRYPT_DESCRIPTOR_HANDLE*);
extern "C" SECURITY_STATUS WINAPI NCryptCloseProtectionDescriptor(NCRYPT_DESCRIPTOR_HANDLE);
extern "C" SECURITY_STATUS WINAPI NCryptProtectSecret(NCRYPT_DESCRIPTOR_HANDLE,DWORD,const BYTE*,ULONG,const void*,HWND,BYTE**,ULONG*);
extern "C" SECURITY_STATUS WINAPI NCryptUnprotectSecret(NCRYPT_DESCRIPTOR_HANDLE*,DWORD,const BYTE*,ULONG,const void*,HWND,BYTE**,ULONG*);

namespace phone_supervisor::r5 {
namespace {
constexpr size_t kMaxSecretBytes = 256;
constexpr size_t kMaxSecureBufferBytes = 16384;
constexpr size_t kMaxBlobBytes = 4096;
bool IsValidClientSecret(const BYTE* p,size_t n) {
    if(!p || n<32 || n>kMaxSecretBytes)return false;
    for(size_t i=0;i<n;i++)if(p[i]<0x21 || p[i]>0x7e)return false;
    return true;
}
bool Protect(const std::wstring& serviceSid,const BYTE* plain,size_t plainSize,std::vector<BYTE>& blob,std::wstring& reason) {
    NCRYPT_DESCRIPTOR_HANDLE descriptor=nullptr;PBYTE output=nullptr;ULONG outputSize=0;
    const std::wstring protectionDescriptor=L"SID="+serviceSid;
    SECURITY_STATUS status=NCryptCreateProtectionDescriptor(protectionDescriptor.c_str(),0,&descriptor);
    if(status==ERROR_SUCCESS)status=NCryptProtectSecret(descriptor,0,plain,static_cast<ULONG>(plainSize),nullptr,nullptr,&output,&outputSize);
    if(descriptor)NCryptCloseProtectionDescriptor(descriptor);
    if(status!=ERROR_SUCCESS || !output || outputSize==0 || outputSize>kMaxBlobBytes) {
        if(output)LocalFree(output);
        reason=L"DPAPI_NG_PROTECT_FAILED";return false;
    }
    blob.assign(output,output+outputSize);SecureZeroMemory(output,outputSize);LocalFree(output);
    reason.clear();return true;
}
bool Unprotect(const BYTE* blob,size_t blobSize,SecureBuffer& secret,std::wstring& reason) {
    NCRYPT_DESCRIPTOR_HANDLE descriptor=nullptr;PBYTE output=nullptr;ULONG outputSize=0;
    SECURITY_STATUS status=NCryptUnprotectSecret(&descriptor,0,blob,static_cast<ULONG>(blobSize),nullptr,nullptr,&output,&outputSize);
    if(descriptor)NCryptCloseProtectionDescriptor(descriptor);
    if(status!=ERROR_SUCCESS || !output || outputSize==0 || outputSize>kMaxSecretBytes) {
        if(output){SecureZeroMemory(output,outputSize);LocalFree(output);}
        reason=L"DPAPI_NG_UNPROTECT_FAILED";return false;
    }
    bool valid=IsValidClientSecret(output,outputSize);
    bool copied=valid && secret.Assign(output,outputSize);
    SecureZeroMemory(output,outputSize);LocalFree(output);
    if(!valid){reason=L"CLIENT_SECRET_FORMAT_INVALID";return false;}
    if(!copied){reason=L"SECURE_BUFFER_ALLOCATION_FAILED";return false;}
    reason.clear();return true;
}
bool EnsureDirectory(const std::wstring& path) {
    if(CreateDirectoryW(path.c_str(),nullptr))return true;
    return GetLastError()==ERROR_ALREADY_EXISTS;
}
bool SidEqualsWellKnown(PSID sid,WELL_KNOWN_SID_TYPE type) {
    BYTE storage[SECURITY_MAX_SID_SIZE]{};DWORD size=sizeof(storage);
    return CreateWellKnownSid(type,nullptr,storage,&size)&&EqualSid(sid,storage);
}
bool CurrentTokenHasEnabledServiceSid(const std::wstring& serviceSid) {
    PSID expected=nullptr;if(!ConvertStringSidToSidW(serviceSid.c_str(),&expected))return false;
    HANDLE token=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token)){LocalFree(expected);return false;}
    DWORD needed=0;GetTokenInformation(token,TokenGroups,nullptr,0,&needed);
    std::vector<BYTE> buffer(needed);bool found=false;
    if(needed&&GetTokenInformation(token,TokenGroups,buffer.data(),needed,&needed)){
        auto* groups=reinterpret_cast<TOKEN_GROUPS*>(buffer.data());
        for(DWORD i=0;i<groups->GroupCount;i++)if(EqualSid(groups->Groups[i].Sid,expected)&&(groups->Groups[i].Attributes&SE_GROUP_ENABLED)){found=true;break;}
    }
    CloseHandle(token);LocalFree(expected);return found;
}
}
SecureBuffer::~SecureBuffer(){Clear();}
SecureBuffer::SecureBuffer(SecureBuffer&& other) noexcept:bytes_(std::move(other.bytes_)),locked_(other.locked_){other.locked_=false;}
SecureBuffer& SecureBuffer::operator=(SecureBuffer&& other) noexcept {
    if(this!=&other){Clear();bytes_=std::move(other.bytes_);locked_=other.locked_;other.locked_=false;}return *this;
}
bool SecureBuffer::Assign(const BYTE* data,size_t size) {
    Clear();if(!data || size==0 || size>kMaxSecureBufferBytes)return false;
    try{bytes_.assign(data,data+size);}catch(...){return false;}
    locked_=VirtualLock(bytes_.data(),bytes_.size())!=FALSE;
    return true;
}
void SecureBuffer::Clear() noexcept {
    if(!bytes_.empty()){
        SecureZeroMemory(bytes_.data(),bytes_.size());
        if(locked_)VirtualUnlock(bytes_.data(),bytes_.size());
        std::vector<BYTE>().swap(bytes_);
    }
    locked_=false;
}
SupervisorCredentialStore::SupervisorCredentialStore(std::wstring root,std::wstring serviceSid)
    :root_(std::move(root)),filePath_(root_+L"\\oidc-client-secret.dpapi"),serviceSid_(std::move(serviceSid)){}
bool SupervisorCredentialStore::ApplyProtectedDacl(const std::wstring& path,bool directory,std::wstring& reason) const {
    if(serviceSid_.rfind(L"S-1-5-80-",0)!=0){reason=L"SERVICE_SID_INVALID";return false;}
    std::wstring sddl=L"D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;";
    if(directory)sddl=L"D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;";
    sddl+=serviceSid_;sddl+=directory?L")":L")";
    PSECURITY_DESCRIPTOR descriptor=nullptr;
    if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&descriptor,nullptr)){reason=L"DACL_DESCRIPTOR_INVALID";return false;}
    BOOL present=FALSE,defaulted=FALSE;PACL dacl=nullptr;
    bool valid=GetSecurityDescriptorDacl(descriptor,&present,&dacl,&defaulted)&&present&&dacl;
    DWORD status=valid?SetNamedSecurityInfoW(const_cast<LPWSTR>(path.c_str()),SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,nullptr,nullptr,dacl,nullptr):ERROR_INVALID_SECURITY_DESCR;
    LocalFree(descriptor);
    if(status!=ERROR_SUCCESS){reason=L"PROTECTED_DACL_APPLY_FAILED_"+std::to_wstring(status);return false;}
    reason.clear();return true;
}
bool SupervisorCredentialStore::VerifyProtectedDacl(const std::wstring& path,std::wstring& reason) const {
    PACL dacl=nullptr;PSECURITY_DESCRIPTOR descriptor=nullptr;
    DWORD status=GetNamedSecurityInfoW(const_cast<LPWSTR>(path.c_str()),SE_FILE_OBJECT,DACL_SECURITY_INFORMATION,nullptr,nullptr,&dacl,nullptr,&descriptor);
    if(status!=ERROR_SUCCESS){reason=L"PROTECTED_DACL_READ_FAILED_"+std::to_wstring(status);return false;}
    SECURITY_DESCRIPTOR_CONTROL control=0;DWORD revision=0;
    bool protectedDacl=GetSecurityDescriptorControl(descriptor,&control,&revision)&&((control&SE_DACL_PROTECTED)!=0);
    DWORD aceCount=dacl?dacl->AceCount:0;
    bool hasSystem=false,hasAdmins=false,hasService=false,hasEveryone=false,hasUsers=false,hasInteractive=false,badMask=false;
    if(dacl){for(DWORD i=0;i<dacl->AceCount;i++){void* raw=nullptr;if(!GetAce(dacl,i,&raw))continue;auto* header=static_cast<ACE_HEADER*>(raw);if(header->AceType!=ACCESS_ALLOWED_ACE_TYPE)continue;auto* ace=static_cast<ACCESS_ALLOWED_ACE*>(raw);if(ace->Mask!=FILE_ALL_ACCESS)badMask=true;PSID sid=&ace->SidStart;
        if(SidEqualsWellKnown(sid,WinLocalSystemSid))hasSystem=true;
        if(SidEqualsWellKnown(sid,WinBuiltinAdministratorsSid))hasAdmins=true;
        PSID expected=nullptr;if(ConvertStringSidToSidW(serviceSid_.c_str(),&expected)){if(EqualSid(sid,expected))hasService=true;LocalFree(expected);}
        if(SidEqualsWellKnown(sid,WinWorldSid))hasEveryone=true;
        if(SidEqualsWellKnown(sid,WinBuiltinUsersSid))hasUsers=true;
        if(SidEqualsWellKnown(sid,WinInteractiveSid))hasInteractive=true;
    }}
    LocalFree(descriptor);
    if(!protectedDacl||aceCount!=3||badMask||!hasSystem||!hasAdmins||!hasService||hasEveryone||hasUsers||hasInteractive){reason=L"PROTECTED_DACL_VERIFY_FAILED";return false;}
    reason.clear();return true;
}
bool SupervisorCredentialStore::Initialize(std::wstring& reason) {
    if(root_.empty()||serviceSid_.rfind(L"S-1-5-80-",0)!=0){reason=L"STORE_CONFIG_INVALID";return false;}
    if(!EnsureDirectory(L"C:\\ProgramData\\TokenSaver")||
       !EnsureDirectory(L"C:\\ProgramData\\TokenSaver\\Phone02")||
       !EnsureDirectory(root_)){reason=L"STORE_DIRECTORY_CREATE_FAILED";return false;}
    if(!CurrentTokenHasEnabledServiceSid(serviceSid_)){reason=L"SERVICE_SID_NOT_ENABLED_IN_PROCESS_TOKEN";return false;}
    if(!ApplyProtectedDacl(root_,true,reason))return false;
    return VerifyProtectedDacl(root_,reason);
}
bool SupervisorCredentialStore::WriteClientSecret(const SecureBuffer& secret,std::wstring& reason) {
    if(!IsValidClientSecret(secret.data(),secret.size())){reason=L"CLIENT_SECRET_FORMAT_INVALID";return false;}
    if(!Initialize(reason))return false;
    std::vector<BYTE> blob;if(!Protect(serviceSid_,secret.data(),secret.size(),blob,reason))return false;
    const std::wstring temp=filePath_+L".tmp";
    HANDLE file=CreateFileW(temp.c_str(),GENERIC_WRITE,0,nullptr,CREATE_ALWAYS,FILE_ATTRIBUTE_HIDDEN|FILE_FLAG_WRITE_THROUGH,nullptr);
    if(file==INVALID_HANDLE_VALUE){SecureZeroMemory(blob.data(),blob.size());reason=L"STORE_TEMP_CREATE_FAILED";return false;}
    DWORD written=0;bool ok=WriteFile(file,blob.data(),static_cast<DWORD>(blob.size()),&written,nullptr)&&written==static_cast<DWORD>(blob.size())&&FlushFileBuffers(file);
    CloseHandle(file);SecureZeroMemory(blob.data(),blob.size());blob.clear();
    if(!ok){DeleteFileW(temp.c_str());reason=L"STORE_CIPHERTEXT_WRITE_FAILED";return false;}
    if(!ApplyProtectedDacl(temp,false,reason)){DeleteFileW(temp.c_str());return false;}
    if(!MoveFileExW(temp.c_str(),filePath_.c_str(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)){DeleteFileW(temp.c_str());reason=L"STORE_ATOMIC_REPLACE_FAILED";return false;}
    if(!ApplyProtectedDacl(filePath_,false,reason))return false;
    return VerifyProtectedDacl(filePath_,reason);
}
bool SupervisorCredentialStore::ReadClientSecret(SecureBuffer& secret,std::wstring& reason) const {
    if(!VerifyProtectedDacl(root_,reason)||!VerifyProtectedDacl(filePath_,reason))return false;
    HANDLE file=CreateFileW(filePath_.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    if(file==INVALID_HANDLE_VALUE){reason=GetLastError()==ERROR_FILE_NOT_FOUND?L"CREDENTIAL_NOT_PROVISIONED":L"STORE_OPEN_FAILED";return false;}
    LARGE_INTEGER size{};if(!GetFileSizeEx(file,&size)||size.QuadPart<=0||size.QuadPart>static_cast<LONGLONG>(kMaxBlobBytes)){CloseHandle(file);reason=L"STORE_BLOB_SIZE_INVALID";return false;}
    std::vector<BYTE> blob(static_cast<size_t>(size.QuadPart));DWORD read=0;bool ok=ReadFile(file,blob.data(),static_cast<DWORD>(blob.size()),&read,nullptr)&&read==blob.size();CloseHandle(file);
    if(!ok){SecureZeroMemory(blob.data(),blob.size());reason=L"STORE_BLOB_READ_FAILED";return false;}
    bool result=Unprotect(blob.data(),blob.size(),secret,reason);SecureZeroMemory(blob.data(),blob.size());return result;
}
bool SupervisorCredentialStore::RemoveClientSecret(std::wstring& reason) const {
    if(DeleteFileW(filePath_.c_str())||GetLastError()==ERROR_FILE_NOT_FOUND){if(GetFileAttributesW(filePath_.c_str())==INVALID_FILE_ATTRIBUTES){reason.clear();return true;}}
    reason=L"STORE_REMOVE_FAILED";return false;
}
bool SupervisorCredentialStore::Exists() const {return GetFileAttributesW(filePath_.c_str())!=INVALID_FILE_ATTRIBUTES;}
} // namespace phone_supervisor::r5
