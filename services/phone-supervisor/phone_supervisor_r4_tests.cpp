#include <windows.h>
#include <algorithm>
#include <ncrypt.h>

#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>
#pragma comment(lib,"ncrypt.lib")
typedef PVOID NCRYPT_DESCRIPTOR_HANDLE;
extern "C" SECURITY_STATUS WINAPI NCryptCreateProtectionDescriptor(
    LPCWSTR pwszDescriptorString,DWORD dwFlags,NCRYPT_DESCRIPTOR_HANDLE* phDescriptor);
extern "C" SECURITY_STATUS WINAPI NCryptCloseProtectionDescriptor(
    NCRYPT_DESCRIPTOR_HANDLE hDescriptor);
extern "C" SECURITY_STATUS WINAPI NCryptProtectSecret(
    NCRYPT_DESCRIPTOR_HANDLE hDescriptor,DWORD dwFlags,PBYTE pbData,ULONG cbData,
    PVOID pMemPara,HWND hWnd,PBYTE* ppbProtectedBlob,ULONG* pcbProtectedBlob);
extern "C" SECURITY_STATUS WINAPI NCryptUnprotectSecret(
    NCRYPT_DESCRIPTOR_HANDLE* phDescriptor,DWORD dwFlags,PBYTE pbProtectedBlob,ULONG cbProtectedBlob,
    PVOID pMemPara,HWND hWnd,PBYTE* ppbData,ULONG* pcbData);
#include "TeamRoomScenarioGateR4.h"
using phone_supervisor::r4::Decision;
using phone_supervisor::r4::Evaluate;
using phone_supervisor::r4::ClassifySecretState;
using phone_supervisor::r4::SupervisorSecretState;

static bool Check(bool ok,const char* marker) {
    std::cout << marker << "=" << (ok ? "PASS" : "FAIL") << "\n";
    return ok;
}
static bool ProtectSynthetic(const std::vector<BYTE>& plain,std::vector<BYTE>& blob) {
    NCRYPT_DESCRIPTOR_HANDLE descriptor=nullptr;
    PBYTE out=nullptr;ULONG outSize=0;
    SECURITY_STATUS s=NCryptCreateProtectionDescriptor(L"LOCAL=user",0,&descriptor);
    if(s==ERROR_SUCCESS)
        s=NCryptProtectSecret(descriptor,0,const_cast<PBYTE>(plain.data()),
                              static_cast<ULONG>(plain.size()),nullptr,nullptr,&out,&outSize);
    if(descriptor)NCryptCloseProtectionDescriptor(descriptor);
    if(s!=ERROR_SUCCESS)return false;
    blob.assign(out,out+outSize);LocalFree(out);return true;
}
static bool UnprotectSynthetic(const std::vector<BYTE>& blob,std::vector<BYTE>& plain) {
    NCRYPT_DESCRIPTOR_HANDLE descriptor=nullptr;
    PBYTE out=nullptr;ULONG outSize=0;
    SECURITY_STATUS s=NCryptUnprotectSecret(&descriptor,0,
        const_cast<PBYTE>(blob.data()),static_cast<ULONG>(blob.size()),
        nullptr,nullptr,&out,&outSize);
    if(descriptor)NCryptCloseProtectionDescriptor(descriptor);
    if(s!=ERROR_SUCCESS)return false;
    plain.assign(out,out+outSize);LocalFree(out);return true;
}
static bool ValidSyntheticFormat(const std::vector<BYTE>& plain) {
    const std::string prefix="R4-VALID-SUPERVISOR-SECRET-V1:";
    return plain.size()>prefix.size() &&
           std::equal(prefix.begin(),prefix.end(),plain.begin());
}
int main() {
    bool ok=true;
    const auto denied=[](SupervisorSecretState state,bool health,bool fake,bool fakeOk,bool local=true) {
        Decision d=Evaluate(local,state,health,fake,fakeOk,false);
        return d.rpcStatus==phone_supervisor::r4::kRpcAccessDenied &&
               !d.accessTokenRequested && !d.authExchangeCalled;
    };

    const auto temp=std::filesystem::temp_directory_path()/"phone02-r4-fail-closed-cleanup";
    std::error_code ec;
    std::filesystem::remove_all(temp,ec);
    const bool storeAbsent=!std::filesystem::exists(temp);
    auto absentState=ClassifySecretState(!storeAbsent,false,false,false);
    ok &= Check(storeAbsent && absentState==SupervisorSecretState::StoreAbsent &&
                denied(absentState,false,false,false),"R4_SECRET_STORE_ABSENT");

    std::filesystem::create_directories(temp,ec);
    const bool storeCreated=!ec && std::filesystem::is_directory(temp);
    auto missingState=ClassifySecretState(storeCreated,false,false,false);
    ok &= Check(storeCreated && missingState==SupervisorSecretState::EntryMissing &&
                denied(missingState,false,false,false),"R4_SECRET_ENTRY_MISSING");

    const std::string invalidText="NOT-A-VALID-SUPERVISOR-CREDENTIAL";
    std::vector<BYTE> invalidPlain(invalidText.begin(),invalidText.end()),invalidBlob,roundTrip;
    std::cout << "R4_DPAPI_STAGE=INVALID_PROTECT_START" << std::endl;
    bool invalidProtected=ProtectSynthetic(invalidPlain,invalidBlob);
    std::cout << "R4_DPAPI_STAGE=INVALID_UNPROTECT_START" << std::endl;
    bool invalidDecrypted=invalidProtected&&UnprotectSynthetic(invalidBlob,roundTrip);
    bool invalidFormat=invalidDecrypted&&ValidSyntheticFormat(roundTrip);
    auto invalidState=ClassifySecretState(storeCreated,invalidProtected,invalidDecrypted,invalidFormat);
    ok &= Check(invalidProtected&&invalidDecrypted&&!invalidFormat&&
                invalidState==SupervisorSecretState::PlaintextInvalid&&
                denied(invalidState,false,false,false),"R4_SECRET_PLAINTEXT_INVALID");

    const std::string validText="R4-VALID-SUPERVISOR-SECRET-V1:synthetic-test-only";
    std::vector<BYTE> validPlain(validText.begin(),validText.end()),validBlob,validRoundTrip;
    std::cout << "R4_DPAPI_STAGE=VALID_PROTECT_START" << std::endl;
    bool protectedOk=ProtectSynthetic(validPlain,validBlob);
    std::cout << "R4_DPAPI_STAGE=VALID_UNPROTECT_START" << std::endl;
    bool decryptedOk=protectedOk&&UnprotectSynthetic(validBlob,validRoundTrip);
    bool formatOk=decryptedOk&&ValidSyntheticFormat(validRoundTrip)&&validRoundTrip==validPlain;
    ok &= Check(formatOk,"R4_DPAPI_NG_SYNTHETIC_ROUNDTRIP");

    bool corruptRejected=false;
    std::vector<BYTE> corruptBlob=validBlob,corruptPlain;
    std::cout << "R4_DPAPI_STAGE=CORRUPT_UNPROTECT_START" << std::endl;
    if(!corruptBlob.empty()){
        for(size_t i=0;i<corruptBlob.size()&&!corruptRejected;i+=std::max<size_t>(1,corruptBlob.size()/11)){
            corruptBlob=validBlob;corruptBlob[i]^=0x5A;
            corruptRejected=!UnprotectSynthetic(corruptBlob,corruptPlain);
            if(!corruptPlain.empty()){SecureZeroMemory(corruptPlain.data(),corruptPlain.size());corruptPlain.clear();}
        }
    }
    std::cout << "R4_DPAPI_STAGE=CORRUPT_UNPROTECT_COMPLETE=" << (corruptRejected?"YES":"NO") << std::endl;
    const auto corruptPath=temp/"credential.bin";
    {std::ofstream f(corruptPath,std::ios::binary|std::ios::trunc);if(!corruptBlob.empty())f.write(reinterpret_cast<const char*>(corruptBlob.data()),static_cast<std::streamsize>(corruptBlob.size()));}
    const bool corruptFilePresent=std::filesystem::exists(corruptPath);
    auto corruptState=ClassifySecretState(storeCreated,corruptFilePresent,!corruptRejected,false);
    ok &= Check(corruptRejected&&corruptState==SupervisorSecretState::CiphertextCorrupt&&
                denied(corruptState,false,false,false),"R4_SECRET_CIPHERTEXT_CORRUPT");

    auto validState=ClassifySecretState(true,protectedOk,decryptedOk,formatOk);
    ok &= Check(formatOk&&validState==SupervisorSecretState::Valid&&
                denied(validState,false,false,false),"R4_VALID_SECRET_WITHOUT_REAL_SESSION");
    ok &= Check(denied(SupervisorSecretState::StoreAbsent,true,false,false),
                "R4_HEALTH_SUCCESS_NOT_AUTH");
    ok &= Check(denied(SupervisorSecretState::Valid,true,true,true),
                "R4_FAKE_MODE_NOT_AUTH");
    ok &= Check(denied(SupervisorSecretState::Valid,true,false,false,false),
                "R4_UNAUTHORIZED_LOCAL_CALLER");
    Decision noHealth=Evaluate(true,SupervisorSecretState::StoreAbsent,false,false,false,false);
    Decision healthy=Evaluate(true,SupervisorSecretState::StoreAbsent,true,false,false,false);
    ok &= Check(noHealth.rpcStatus==healthy.rpcStatus,
                "R4_HEALTH_RESULT_CANNOT_CHANGE_AUTHORIZATION");
    ok &= Check(!healthy.accessTokenRequested,"ACCESS_TOKEN_REQUESTED=NO");
    ok &= Check(!healthy.authExchangeCalled,"AUTH_EXCHANGE_CALLED=NO");

    if(!roundTrip.empty())SecureZeroMemory(roundTrip.data(),roundTrip.size());
    if(!validRoundTrip.empty())SecureZeroMemory(validRoundTrip.data(),validRoundTrip.size());
    if(!invalidPlain.empty())SecureZeroMemory(invalidPlain.data(),invalidPlain.size());
    if(!validPlain.empty())SecureZeroMemory(validPlain.data(),validPlain.size());
    if(!invalidBlob.empty())SecureZeroMemory(invalidBlob.data(),invalidBlob.size());
    if(!validBlob.empty())SecureZeroMemory(validBlob.data(),validBlob.size());
    if(!corruptBlob.empty())SecureZeroMemory(corruptBlob.data(),corruptBlob.size());
    std::filesystem::remove_all(temp,ec);
    ok &= Check(!std::filesystem::exists(temp),"R4_CLEANUP");
    ok &= Check(ok,"R4_FAIL_CLOSED_POLICY");
    return ok?0:1;
}
