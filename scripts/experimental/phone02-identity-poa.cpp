#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <userenv.h>
#include <sddl.h>
#include <stdio.h>
#include <wchar.h>
#include <string>
#include <cstring>
#include <vector>

#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "userenv.lib")

static const wchar_t* kProfile = L"TokenSaver.Phone02.IdentityPoA.Experimental";
static const wchar_t* kPipe = L"\\\\.\\pipe\\TokenSaver.Phone02.IdentityPoA";

static std::wstring SidText(PSID sid) {
    if (!sid || !IsValidSid(sid)) return L"UNAVAILABLE";
    LPWSTR p = nullptr;
    if (!ConvertSidToStringSidW(sid, &p)) return L"UNAVAILABLE";
    std::wstring s(p);
    LocalFree(p);
    return s;
}

static bool TokenBool(HANDLE token, TOKEN_INFORMATION_CLASS cls, bool& value) {
    DWORD n = 0;
    GetTokenInformation(token, cls, nullptr, 0, &n);
    if (!n) return false;
    std::vector<BYTE> b(n);
    if (!GetTokenInformation(token, cls, b.data(), n, &n)) return false;
    value = *reinterpret_cast<BYTE*>(b.data()) != 0;
    return true;
}

static std::wstring TokenSid(HANDLE token, TOKEN_INFORMATION_CLASS cls, bool appContainerInfo) {
    DWORD n = 0;
    GetTokenInformation(token, cls, nullptr, 0, &n);
    if (!n) return L"UNAVAILABLE";
    std::vector<BYTE> b(n);
    if (!GetTokenInformation(token, cls, b.data(), n, &n)) return L"UNAVAILABLE";
    PSID sid = appContainerInfo
        ? reinterpret_cast<TOKEN_APPCONTAINER_INFORMATION*>(b.data())->TokenAppContainer
        : reinterpret_cast<TOKEN_USER*>(b.data())->User.Sid;
    return SidText(sid);
}

static bool SendClient(const wchar_t* pipeName, const wchar_t* label) {
    HANDLE h = CreateFileW(pipeName, GENERIC_READ | GENERIC_WRITE, 0, nullptr, OPEN_EXISTING, 0, nullptr);
    if (h == INVALID_HANDLE_VALUE) {
        wprintf(L"CLIENT_CONNECT_FAILED label=%s win32=%lu\n", label, GetLastError());
        return false;
    }
    const char* msg = "HELLO|claimed_app=TokenSaver.Phone02.IdentityPoA.Experimental|claimed_package_sid=CLIENT_SUPPLIED|claimed_handshake=IDENTICAL";
    DWORD written = 0;
    BOOL ok = WriteFile(h, msg, (DWORD)strlen(msg), &written, nullptr);
    wprintf(L"CLIENT_SENT label=%s bytes=%lu ok=%u\n", label, written, ok ? 1 : 0);
    CloseHandle(h);
    return ok != FALSE;
}

static bool GetCurrentUserSid(std::wstring& sidText) {
    HANDLE t = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &t)) return false;
    sidText = TokenSid(t, TokenUser, false);
    CloseHandle(t);
    return sidText != L"UNAVAILABLE";
}

static bool LaunchAppContainer(const std::wstring& exe, PSID appSid, const wchar_t* pipeName, PROCESS_INFORMATION& pi) {
    SIZE_T bytes = 0;
    InitializeProcThreadAttributeList(nullptr, 1, 0, &bytes);
    std::vector<BYTE> storage(bytes);
    auto attrs = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(storage.data());
    if (!InitializeProcThreadAttributeList(attrs, 1, 0, &bytes)) return false;

    SECURITY_CAPABILITIES caps{};
    caps.AppContainerSid = appSid;
    caps.CapabilityCount = 0;
    caps.Capabilities = nullptr;
    caps.Reserved = 0;

    if (!UpdateProcThreadAttribute(attrs, 0, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
        &caps, sizeof(caps), nullptr, nullptr)) {
        DeleteProcThreadAttributeList(attrs);
        return false;
    }

    STARTUPINFOEXW si{};
    si.StartupInfo.cb = sizeof(si);
    si.lpAttributeList = attrs;
    std::wstring cmd = L"\"" + exe + L"\" --client \"" + pipeName + L"\" legitimate";
    std::vector<wchar_t> mutableCmd(cmd.begin(), cmd.end());
    mutableCmd.push_back(L'\0');

    BOOL ok = CreateProcessW(exe.c_str(), mutableCmd.data(), nullptr, nullptr, FALSE,
        EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW, nullptr, nullptr,
        &si.StartupInfo, &pi);
    DWORD err = GetLastError();
    DeleteProcThreadAttributeList(attrs);
    if (!ok) wprintf(L"APPCONTAINER_CREATEPROCESS_FAILED win32=%lu\n", err);
    return ok != FALSE;
}

static HANDLE CreatePoAPipe(const std::wstring& userSid, const std::wstring& appSid) {
    std::wstring sddl = L"D:P(A;;GA;;;" + userSid + L")(A;;GA;;;" + appSid + L")";
    PSECURITY_DESCRIPTOR sd = nullptr;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(), SDDL_REVISION_1, &sd, nullptr)) {
        wprintf(L"PIPE_SDDL_FAILED win32=%lu\n", GetLastError());
        return INVALID_HANDLE_VALUE;
    }
    SECURITY_ATTRIBUTES sa{ sizeof(sa), sd, FALSE };
    HANDLE h = CreateNamedPipeW(kPipe, PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        1, 4096, 4096, 5000, &sa);
    DWORD err = GetLastError();
    LocalFree(sd);
    if (h == INVALID_HANDLE_VALUE) wprintf(L"PIPE_CREATE_FAILED win32=%lu\n", err);
    return h;
}

static bool InspectConnection(HANDLE pipe, const std::wstring& expectedAppSid, const wchar_t* label, bool expectedAuthorized) {
    // Windows requires the server to read a client message before impersonation.
    char msg[2048]{};
    DWORD read = 0;
    BOOL readOk = ReadFile(pipe, msg, sizeof(msg) - 1, &read, nullptr);
    if (!readOk) {
        wprintf(L"PROTOCOL_READ_FAILED label=%s win32=%lu\n", label, GetLastError());
        return false;
    }
    BOOL impersonated = ImpersonateNamedPipeClient(pipe);
    if (!impersonated) {
        wprintf(L"IDENTITY_INSPECTION_FAILED label=%s win32=%lu\n", label, GetLastError());
        return false;
    }
    HANDLE threadToken = nullptr;
    bool got = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, TRUE, &threadToken) != FALSE;
    bool isApp = false;
    bool gotAppFlag = got && TokenBool(threadToken, TokenIsAppContainer, isApp);
    std::wstring user = got ? TokenSid(threadToken, TokenUser, false) : L"UNAVAILABLE";
    std::wstring app = got ? TokenSid(threadToken, TokenAppContainerSid, true) : L"UNAVAILABLE";
    std::wstring package = L"NOT_TESTED_NO_MSIX_PACKAGE_IDENTITY";
    bool authorized = got && gotAppFlag && isApp && app == expectedAppSid;
    RevertToSelf();
    if (threadToken) CloseHandle(threadToken);

    wprintf(L"CONNECTION_OBSERVED label=%s token_user_sid=%s token_is_appcontainer=%u appcontainer_sid=%s package_sid=%s protocol_read=%u protocol_bytes=%lu\n",
        label, user.c_str(), (gotAppFlag && isApp) ? 1 : 0, app.c_str(), package.c_str(), readOk ? 1 : 0, read);
    wprintf(L"AUTHORIZATION_RESULT label=%s result=%s\n", label, authorized ? L"ALLOW" : L"REJECT");
    if (expectedAuthorized && authorized) wprintf(L"LEGITIMATE_IDENTITY_OBSERVED\n");
    if (!expectedAuthorized && !authorized) wprintf(L"COMMON_PROCESS_REJECTED\n");
    return expectedAuthorized ? authorized : !authorized;
}

int wmain(int argc, wchar_t** argv) {
    if (argc >= 4 && wcscmp(argv[1], L"--client") == 0) return SendClient(argv[2], argv[3]) ? 0 : 2;

    wprintf(L"ROUND_1_ENVIRONMENT branch_and_sha_supplied_by_workflow\n");
    std::wstring userSid;
    if (!GetCurrentUserSid(userSid)) {
        wprintf(L"BLOCKED_USER_SID_UNAVAILABLE\n");
        return 10;
    }
    wprintf(L"ROUND_2_IDENTITY user_sid=%s\n", userSid.c_str());

    PSID appSid = nullptr;
    HRESULT hr = CreateAppContainerProfile(kProfile, L"TokenSaver Phone02 Identity PoA", L"TEST-ONLY disposable AppContainer identity", nullptr, 0, &appSid);
    if (FAILED(hr)) {
        if (hr == HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS))
            hr = DeriveAppContainerSidFromAppContainerName(kProfile, &appSid);
    }
    if (FAILED(hr) || !appSid) {
        wprintf(L"BLOCKED_APPCONTAINER_PROFILE_UNAVAILABLE hresult=0x%08lx\n", (unsigned long)hr);
        return 11;
    }
    std::wstring appSidText = SidText(appSid);
    wprintf(L"APPCONTAINER_PROFILE_CREATED name=%s appcontainer_sid=%s\n", kProfile, appSidText.c_str());

    wchar_t path[MAX_PATH]{};
    GetModuleFileNameW(nullptr, path, MAX_PATH);
    std::wstring exe(path);
    wprintf(L"ROUND_3_SUPERVISOR pid=%lu pipe=%s\n", GetCurrentProcessId(), kPipe);

    HANDLE pipe = CreatePoAPipe(userSid, appSidText);
    if (pipe == INVALID_HANDLE_VALUE) { FreeSid(appSid); return 12; }

    PROCESS_INFORMATION client{};
    if (!LaunchAppContainer(exe, appSid, kPipe, client)) {
        CloseHandle(pipe); FreeSid(appSid); return 13;
    }
    wprintf(L"ROUND_4_LEGITIMATE_CLIENT_STARTED pid=%lu\n", client.dwProcessId);
    BOOL connected = ConnectNamedPipe(pipe, nullptr) ? TRUE : (GetLastError() == ERROR_PIPE_CONNECTED);
    bool legitOk = false;
    if (connected) legitOk = InspectConnection(pipe, appSidText, L"legitimate_appcontainer", true);
    else wprintf(L"LEGITIMATE_CONNECT_FAILED win32=%lu\n", GetLastError());
    FlushFileBuffers(pipe);
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    WaitForSingleObject(client.hProcess, 10000);
    CloseHandle(client.hThread);
    CloseHandle(client.hProcess);

    wprintf(L"ROUND_6_SAME_USER_ADVERSARY\n");
    pipe = CreatePoAPipe(userSid, appSidText);
    if (pipe == INVALID_HANDLE_VALUE) { FreeSid(appSid); return 14; }
    PROCESS_INFORMATION attacker{};
    std::wstring cmd = L"\"" + exe + L"\" --client \"" + std::wstring(kPipe) + L"\" attacker";
    std::vector<wchar_t> mutableCmd(cmd.begin(), cmd.end()); mutableCmd.push_back(L'\0');
    STARTUPINFOW si{}; si.cb = sizeof(si);
    BOOL started = CreateProcessW(exe.c_str(), mutableCmd.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW, nullptr, nullptr, &si, &attacker);
    if (!started) {
        wprintf(L"ATTACKER_START_FAILED win32=%lu\n", GetLastError());
        CloseHandle(pipe); FreeSid(appSid); return 15;
    }
    connected = ConnectNamedPipe(pipe, nullptr) ? TRUE : (GetLastError() == ERROR_PIPE_CONNECTED);
    bool attackerRejected = false;
    if (connected) attackerRejected = InspectConnection(pipe, appSidText, L"same_user_common_process", false);
    else wprintf(L"ATTACKER_CONNECT_FAILED win32=%lu\n", GetLastError());
    FlushFileBuffers(pipe);
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    WaitForSingleObject(attacker.hProcess, 10000);
    CloseHandle(attacker.hThread);
    CloseHandle(attacker.hProcess);

    wprintf(L"ROUND_7_BYPASS_TESTS protocol_claims_ignored=1 username_not_used=1 executable_name_not_used=1\n");
    wprintf(L"ROUND_8_PIPE_DACL user_sid_and_appcontainer_sid_only; everyone=absent; anonymous=absent; remote_clients=rejected\n");
    wprintf(L"ROUND_9_LIMITATIONS msix_package_identity_not_installed; package_sid_not_proven; node_libuv_not_tested; anonymous_remote_attempt_not_executed\n");
    wprintf(L"ROUND_10_REPORT evidence_emitted_to_workflow_log\n");

    if (legitOk && attackerRejected) wprintf(L"POA_RESULT=PARTIAL_APPCONTAINER_TOKEN_IDENTITY_PROVEN_MSIX_PACKAGE_IDENTITY_NOT_PROVEN\n");
    else wprintf(L"POA_RESULT=FAIL_OR_BLOCKED\n");

    FreeSid(appSid);
    return (legitOk && attackerRejected) ? 0 : 20;
}