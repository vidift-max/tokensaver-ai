#pragma once
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <string>
#include <vector>

namespace phone_supervisor::r5 {
class SecureBuffer final {
public:
    SecureBuffer() = default;
    ~SecureBuffer();
    SecureBuffer(const SecureBuffer&) = delete;
    SecureBuffer& operator=(const SecureBuffer&) = delete;
    SecureBuffer(SecureBuffer&& other) noexcept;
    SecureBuffer& operator=(SecureBuffer&& other) noexcept;
    bool Assign(const BYTE* data, size_t size);
    void Clear() noexcept;
    BYTE* data() noexcept { return bytes_.empty() ? nullptr : bytes_.data(); }
    const BYTE* data() const noexcept { return bytes_.empty() ? nullptr : bytes_.data(); }
    size_t size() const noexcept { return bytes_.size(); }
    bool empty() const noexcept { return bytes_.empty(); }
private:
    std::vector<BYTE> bytes_;
    bool locked_ = false;
};

class SupervisorCredentialStore final {
public:
    SupervisorCredentialStore(std::wstring root, std::wstring serviceSid);
    bool Initialize(std::wstring& reason);
    bool WriteClientSecret(const SecureBuffer& secret, std::wstring& reason);
    bool ReadClientSecret(SecureBuffer& secret, std::wstring& reason) const;
    bool RemoveClientSecret(std::wstring& reason) const;
    bool Exists() const;
    const std::wstring& Path() const noexcept { return filePath_; }
private:
    std::wstring root_;
    std::wstring filePath_;
    std::wstring serviceSid_;
    bool ApplyProtectedDacl(const std::wstring& path, bool directory, std::wstring& reason) const;
    bool VerifyProtectedDacl(const std::wstring& path, std::wstring& reason) const;
};
} // namespace phone_supervisor::r5
