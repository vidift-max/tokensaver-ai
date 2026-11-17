#include "TeamRoomScenarioGateR4.h"
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
using phone_supervisor::r4::Decision;
using phone_supervisor::r4::Evaluate;
using phone_supervisor::r4::ClassifySecretState;
using phone_supervisor::r4::SupervisorSecretState;

static bool Check(bool ok,const char* marker) {
    std::cout << marker << "=" << (ok ? "PASS" : "FAIL") << "\n";
    return ok;
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

    const auto corruptPath=temp/"credential.bin";
    {
        std::ofstream f(corruptPath,std::ios::binary|std::ios::trunc);
        const unsigned char corrupt[3]={0x00,0x7f,0x01};
        f.write(reinterpret_cast<const char*>(corrupt),sizeof(corrupt));
    }
    const auto corruptSize=std::filesystem::file_size(corruptPath,ec);
    const bool ciphertextDecryptable=(ec==std::error_code{} && corruptSize>=32);
    auto corruptState=ClassifySecretState(storeCreated,std::filesystem::exists(corruptPath),
                                          ciphertextDecryptable,false);
    ok &= Check(corruptSize==3 && corruptState==SupervisorSecretState::CiphertextCorrupt &&
                denied(corruptState,false,false,false),"R4_SECRET_CIPHERTEXT_CORRUPT");

    // Simulates successful decryption followed by a failed format/policy check.
    auto invalidState=ClassifySecretState(true,true,true,false);
    ok &= Check(invalidState==SupervisorSecretState::PlaintextInvalid &&
                denied(invalidState,false,false,false),"R4_SECRET_PLAINTEXT_INVALID");

    auto validState=ClassifySecretState(true,true,true,true);
    ok &= Check(validState==SupervisorSecretState::Valid &&
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

    std::filesystem::remove_all(temp,ec);
    ok &= Check(!std::filesystem::exists(temp),"R4_CLEANUP");
    ok &= Check(ok,"R4_FAIL_CLOSED_POLICY");
    return ok?0:1;
}
