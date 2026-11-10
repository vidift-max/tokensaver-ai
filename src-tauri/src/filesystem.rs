use sha2::{Digest, Sha256};
use std::{fs::File, io::{self, Read}, path::Path};

/// Atomically publishes stage at target without replacing an existing target.
/// Fails closed on platforms without a supported native primitive.
pub fn publish_noreplace(stage: &Path, target: &Path) -> io::Result<()> {
    if stage == target {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "stage and target must differ"));
    }
    if !stage.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "staging path does not exist"));
    }
    if target.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "target already exists"));
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let old = std::ffi::CString::new(stage.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "stage contains NUL"))?;
        let new = std::ffi::CString::new(target.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "target contains NUL"))?;
        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                old.as_ptr(),
                libc::AT_FDCWD,
                new.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 { return Ok(()); }
        return Err(io::Error::last_os_error());
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let old: Vec<u16> = stage.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let new: Vec<u16> = target.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(old.as_ptr(), new.as_ptr(), 0)
        };
        if ok != 0 { return Ok(()); }
        return Err(io::Error::last_os_error());
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let _ = (stage, target);
        Err(io::Error::new(io::ErrorKind::Unsupported, "atomic no-replace publication is not implemented on this Unix target"))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (stage, target);
        Err(io::Error::new(io::ErrorKind::Unsupported, "atomic no-replace publication is not implemented on this platform"))
    }
}



/// Hashes a regular file while enforcing a hard byte budget.
/// The byte count is based on bytes actually read, not filesystem metadata.
/// Copies a regular file while enforcing a hard byte budget and returning actual bytes copied.
pub fn copy_file_limited(source: &Path, target: &Path, max_bytes: u64) -> io::Result<u64> {
    let mut input = File::open(source)?;
    let mut output = File::create(target)?;
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 { break; }
        total = total.checked_add(n as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "byte count overflow"))?;
        if total > max_bytes {
            return Err(io::Error::new(io::ErrorKind::FileTooLarge, "file exceeds configured byte budget"));
        }
        std::io::Write::write_all(&mut output, &buffer[..n])?;
    }
    output.sync_all()?;
    Ok(total)
}

pub fn sha256_file_limited(path: &Path, max_bytes: u64) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 { break; }
        total = total.checked_add(n as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "byte count overflow"))?;
        if total > max_bytes {
            return Err(io::Error::new(io::ErrorKind::FileTooLarge, "file exceeds configured byte budget"));
        }
        hasher.update(&buffer[..n]);
    }
    Ok((format!("{:x}", hasher.finalize()), total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tokensaver-fs-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()))
    }

    #[test]
    fn publishes_when_target_is_absent() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let stage = root.join("stage");
        let target = root.join("target");
        fs::create_dir(&stage).unwrap();
        publish_noreplace(&stage, &target).unwrap();
        assert!(target.is_dir());
        assert!(!stage.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_copy_rejects_oversized_source() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, vec![7u8; 32]).unwrap();
        let err = copy_file_limited(&source, &target, 16).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_existing_target() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let stage = root.join("stage");
        let target = root.join("target");
        fs::create_dir(&stage).unwrap();
        fs::create_dir(&target).unwrap();
        let err = publish_noreplace(&stage, &target).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(stage.is_dir());
        assert!(target.is_dir());
        let _ = fs::remove_dir_all(root);
    }
}
