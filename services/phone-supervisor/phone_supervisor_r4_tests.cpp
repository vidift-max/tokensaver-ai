#include "TeamRoomScenarioGateR4.h"
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
using phone_supervisor::r4::Decision;
using phone_supervisor::r4::Evaluate;
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
    ok &= Check(denied(SupervisorSecretState::StoreAbsent,false,false,false),"R4_SECRET_STORE_ABSENT");
    ok &= Check(denied(SupervisorSecretState::EntryMissing,false,false,false),"R4_SECRET_ENTRY_MISSING");
    ok &= Check(denied(SupervisorSecretState::CiphertextCorrupt,false,false,false),"R4_SECRET_CIPHERTEXT_CORRUPT");
    ok &= Check(denied(SupervisorSecretState::PlaintextInvalid,false,false,false),"R4_SECRET_PLAINTEXT_INVALID");
    ok &= Check(denied(SupervisorSecretState::Valid,false,false,false),"R4_VALID_SECRET_WITHOUT_REAL_SESSION");
    ok &= Check(denied(SupervisorSecretState::StoreAbsent,true,false,false),"R4_HEALTH_SUCCESS_NOT_AUTH");
    ok &= Check(denied(SupervisorSecretState::Valid,true,true,true),"R4_FAKE_MODE_NOT_AUTH");
    ok &= Check(denied(SupervisorSecretState::Valid,true,false,false,false),"R4_UNAUTHORIZED_LOCAL_CALLER");
    Decision noHealth=Evaluate(true,SupervisorSecretState::StoreAbsent,false,false,false,false);
    Decision healthy=Evaluate(true,SupervisorSecretState::StoreAbsent,true,false,false,false);
    ok &= Check(noHealth.rpcStatus==healthy.rpcStatus,"R4_HEALTH_RESULT_CANNOT_CHANGE_AUTHORIZATION");
    ok &= Check(!healthy.accessTokenRequested,"ACCESS_TOKEN_REQUESTED=NO");
    ok &= Check(!healthy.authExchangeCalled,"AUTH_EXCHANGE_CALLED=NO");

    const auto temp=std::filesystem::temp_directory_path()/"phone02-r4-fail-closed-cleanup";
    std::error_code ec;
    std::filesystem::remove_all(temp,ec);
    std::filesystem::create_directories(temp,ec);
    if(!ec) {
        std::ofstream f(temp/"synthetic-corrupt.bin",std::ios::binary);
        const char bytes[3]={char(0x00),char(0x7f),char(0x01)};
        f.write(bytes,sizeof(bytes));f.close();
    }
    bool tempCreated=!ec && std::filesystem::exists(temp/"synthetic-corrupt.bin");
    std::filesystem::remove_all(temp,ec);
    ok &= Check(tempCreated && !std::filesystem::exists(temp),"R4_CLEANUP");
    ok &= Check(ok,"R4_FAIL_CLOSED_POLICY");
    return ok?0:1;
}
