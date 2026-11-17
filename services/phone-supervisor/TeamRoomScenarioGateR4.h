#pragma once
namespace phone_supervisor::r4 {
constexpr long kRpcOk = 0;
constexpr long kRpcAccessDenied = 2;

enum class SupervisorSecretState {
    StoreAbsent,
    EntryMissing,
    CiphertextCorrupt,
    PlaintextInvalid,
    Valid
};

inline SupervisorSecretState ClassifySecretState(
    bool storePresent,
    bool entryPresent,
    bool ciphertextDecryptable,
    bool plaintextValid) noexcept {
    if (!storePresent) return SupervisorSecretState::StoreAbsent;
    if (!entryPresent) return SupervisorSecretState::EntryMissing;
    if (!ciphertextDecryptable) return SupervisorSecretState::CiphertextCorrupt;
    if (!plaintextValid) return SupervisorSecretState::PlaintextInvalid;
    return SupervisorSecretState::Valid;
}

struct Decision {
    long rpcStatus;
    bool accessTokenRequested;
    bool authExchangeCalled;
};

inline Decision Evaluate(
    bool localSessionAuthorized,
    SupervisorSecretState secretState,
    bool healthProbeSucceeded,
    bool fakeMode,
    bool fakeScenarioSucceeded,
    bool realTeamRoomSessionAuthenticated) noexcept {
    (void)healthProbeSucceeded;
    (void)fakeMode;
    (void)fakeScenarioSucceeded;
    const bool maySucceed =
        localSessionAuthorized &&
        secretState == SupervisorSecretState::Valid &&
        realTeamRoomSessionAuthenticated;
    return { maySucceed ? kRpcOk : kRpcAccessDenied, false, false };
}
} // namespace phone_supervisor::r4
