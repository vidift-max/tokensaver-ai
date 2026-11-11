use sha2::{Digest, Sha256};
use std::{fs::{File, OpenOptions}, io::{self, Read}, path::Path};

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



/// Copies a regular file while enforcing a hard byte budget and returning actual bytes copied.
pub fn copy_file_limited(source: &Path, target: &Path, max_bytes: u64) -> io::Result<u64> {
    if target.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "target already exists"));
    }
    let temp = target.with_file_name(format!(".{}.{}.tmp", target.file_name().and_then(|v| v.to_str()).unwrap_or("copy"), uuid::Uuid::new_v4()));
    let mut temp_created = false;
    let result = (|| -> io::Result<u64> {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        temp_created = true;
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
        drop(output);
        publish_noreplace(&temp, target)?;
        Ok(total)
    })();
    if result.is_err() && temp_created { let _ = std::fs::remove_file(&temp); }
    result
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
    fn limited_copy_publishes_complete_file() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, b"complete-copy").unwrap();
        let copied = copy_file_limited(&source, &target, 1024).unwrap();
        assert_eq!(copied, 13);
        assert_eq!(fs::read(&target).unwrap(), b"complete-copy");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_copy_accepts_exact_byte_budget() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, b"12345678").unwrap();
        assert_eq!(copy_file_limited(&source, &target, 8).unwrap(), 8);
        assert_eq!(fs::read(&target).unwrap(), b"12345678");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_hash_accepts_exact_budget_and_returns_digest() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        fs::write(&source, b"abc").unwrap();
        let (digest, size) = sha256_file_limited(&source, 3).unwrap();
        assert_eq!(size, 3);
        assert_eq!(digest, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_hash_rejects_oversized_file() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        fs::write(&source, b"four").unwrap();
        let err = sha256_file_limited(&source, 3).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_publication_never_replaces_winner() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let stage_a = root.join("stage-a");
        let stage_b = root.join("stage-b");
        let target = root.join("target");
        fs::create_dir(&stage_a).unwrap();
        fs::create_dir(&stage_b).unwrap();
        fs::write(stage_a.join("winner"), b"a").unwrap();
        fs::write(stage_b.join("winner"), b"b").unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let a = {
            let barrier = barrier.clone();
            let stage = stage_a.clone();
            let target = target.clone();
            std::thread::spawn(move || {
                barrier.wait();
                publish_noreplace(&stage, &target)
            })
        };
        let b = {
            let barrier = barrier.clone();
            let stage = stage_b.clone();
            let target = target.clone();
            std::thread::spawn(move || {
                barrier.wait();
                publish_noreplace(&stage, &target)
            })
        };
        barrier.wait();
        let results = [a.join().unwrap(), b.join().unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|r| r.as_ref().err().map(|e| e.kind() == io::ErrorKind::AlreadyExists).unwrap_or(false)).count(), 1);
        let winner = fs::read(target.join("winner")).unwrap();
        assert!(winner == b"a" || winner == b"b");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_copy_does_not_replace_existing_target() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, b"new").unwrap();
        fs::write(&target, b"original").unwrap();
        let err = copy_file_limited(&source, &target, 1024).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"original");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_copy_cleans_staging_file_when_target_appears_before_publication() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, b"new-content").unwrap();
        fs::write(&target, b"winner-content").unwrap();

        let err = copy_file_limited(&source, &target, 1024).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"winner-content");

        let remaining: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            remaining.len(),
            2,
            "failed publication must remove its private staging file"
        );
        assert!(remaining.contains(&std::ffi::OsString::from("source")));
        assert!(remaining.contains(&std::ffi::OsString::from("target")));

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

    #[test]
    fn limited_copy_cleans_staging_file_after_budget_failure() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let target = root.join("target");
        fs::write(&source, b"this source exceeds the budget").unwrap();

        let err = copy_file_limited(&source, &target, 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
        assert!(!target.exists());

        let remaining: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(remaining, vec![std::ffi::OsString::from("source")]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn limited_copy_leaves_no_target_when_source_is_missing() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("missing-source");
        let target = root.join("target");

        assert!(copy_file_limited(&source, &target, 16).is_err());
        assert!(!target.exists());

        let remaining: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(remaining.is_empty());

        let _ = fs::remove_dir_all(root);
    }

}
