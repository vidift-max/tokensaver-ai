use super::*;

#[test]
fn logical_path_rejects_traversal_and_absolute_paths() {
    assert!(safe_logical_path("../escape.txt").is_err());
    assert!(safe_logical_path("/absolute.txt").is_err());
    assert!(safe_logical_path("C:\\escape.txt").is_err() || cfg!(not(windows)));
    assert!(safe_logical_path("docs/article.html").is_ok());
}

#[test]
fn logical_path_rejects_empty_components_current_dir_and_nul() {
    assert!(safe_logical_path("docs//article.md").is_err());
    assert!(safe_logical_path("docs/./article.md").is_err());
    assert!(safe_logical_path("docs/article.md/").is_err());
    assert!(safe_logical_path("docs/..\\\\article.md").is_err());
    assert!(safe_logical_path("docs/\\0article.md").is_err());
}

#[test]
fn logical_path_enforces_depth() {
    let deep = (0..(limits::MAX_PATH_DEPTH + 1))
        .map(|_| "x")
        .collect::<Vec<_>>()
        .join("/");
    assert!(safe_logical_path(&deep).is_err());
}

#[test]
fn token_estimate_is_explicitly_approximate() {
    let value = estimate_tokens("123456789".to_string()).expect("estimate");
    assert_eq!(value.method, "local_estimate");
    assert!(value.approximate);
    assert_eq!(value.count, 3);
}

#[test]
fn sqlite_migration_creates_required_tables() {
    let dir = std::env::temp_dir().join(format!("tokensaver-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let conn = db::open(&dir.join("db.sqlite")).expect("database");
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('projects','blobs','files','file_versions','token_counts','operations')",
            [],
            |r| r.get(0),
        )
        .expect("schema query");
    assert_eq!(count, 6);
    let version: i32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .expect("version");
    assert_eq!(version, db::SCHEMA_VERSION);
    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reimport_same_content_reuses_blob_and_version() {
    let dir = std::env::temp_dir().join(format!("tokensaver-dedup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("blobs")).expect("storage");
    let conn = db::open(&dir.join("db.sqlite")).expect("database");
    let project = "p1";
    let logical = "docs/article.md";
    let bytes = b"same content";
    let sha = hash::sha256_bytes(bytes);
    let blob_rel = ensure_blob(&dir, &sha, bytes).expect("blob");
    let now = timestamp();

    conn.execute(
        "INSERT INTO projects(id,name,created_at) VALUES(?1,?2,?3)",
        rusqlite::params![project, project, now],
    ).expect("project");
    conn.execute(
        "INSERT INTO blobs(sha256,byte_len,relative_path,created_at) VALUES(?1,?2,?3,?4)",
        rusqlite::params![sha, bytes.len() as i64, blob_rel, now],
    ).expect("blob row");
    let file_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO files(id,project_id,logical_path,created_at) VALUES(?1,?2,?3,?4)",
        rusqlite::params![file_id, project, logical, now],
    ).expect("file");
    conn.execute(
        "INSERT INTO file_versions(id,file_id,version_no,blob_sha256,byte_len,created_at) VALUES(?1,?2,1,?3,?4,?5)",
        rusqlite::params![uuid::Uuid::new_v4().to_string(), file_id, sha, bytes.len() as i64, now],
    ).expect("version");

    let blob_rel_again = ensure_blob(&dir, &sha, bytes).expect("dedup blob");
    assert_eq!(blob_rel_again, blob_rel);

    let latest: i64 = conn.query_row(
        "SELECT MAX(version_no) FROM file_versions WHERE file_id=?1",
        rusqlite::params![file_id],
        |r| r.get(0),
    ).expect("latest");
    assert_eq!(latest, 1);

    let blob_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM blobs WHERE sha256=?1",
        rusqlite::params![sha],
        |r| r.get(0),
    ).expect("blob count");
    assert_eq!(blob_count, 1);

    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reimport_changed_content_creates_new_version_and_reuses_shared_blob() {
    let dir = std::env::temp_dir().join(format!("tokensaver-version-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("blobs")).expect("storage");
    let conn = db::open(&dir.join("db.sqlite")).expect("database");
    let sha1 = hash::sha256_bytes(b"one");
    let sha2 = hash::sha256_bytes(b"two");
    let now = timestamp();

    conn.execute(
        "INSERT INTO projects(id,name,created_at) VALUES('p1','p1',?1)",
        rusqlite::params![now],
    ).expect("project");
    for (sha, bytes) in [(&sha1, b"one".as_slice()), (&sha2, b"two".as_slice())] {
        let rel = ensure_blob(&dir, sha, bytes).expect("blob");
        conn.execute(
            "INSERT INTO blobs(sha256,byte_len,relative_path,created_at) VALUES(?1,?2,?3,?4)",
            rusqlite::params![sha, bytes.len() as i64, rel, now],
        ).expect("blob row");
    }

    let file_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO files(id,project_id,logical_path,created_at) VALUES(?1,'p1','docs/a.md',?2)",
        rusqlite::params![file_id, now],
    ).expect("file");

    for (version, sha, bytes) in [(1_i64, &sha1, b"one".as_slice()), (2_i64, &sha2, b"two".as_slice())] {
        conn.execute(
            "INSERT INTO file_versions(id,file_id,version_no,blob_sha256,byte_len,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), file_id, version, sha, bytes.len() as i64, now],
        ).expect("version");
    }

    let latest: (i64, String) = conn.query_row(
        "SELECT version_no,blob_sha256 FROM file_versions WHERE file_id=?1 ORDER BY version_no DESC LIMIT 1",
        rusqlite::params![file_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).expect("latest");
    assert_eq!(latest.0, 2);
    assert_eq!(latest.1, sha2);

    let blob_count: i64 = conn.query_row("SELECT COUNT(*) FROM blobs", [], |r| r.get(0)).expect("blob count");
    assert_eq!(blob_count, 2);

    drop(conn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn batch_limits_are_enforced() {
    assert!(commands::validate_batch(limits::MAX_FILES_PER_BATCH + 1, 0).is_err());
    assert!(commands::validate_batch(1, limits::MAX_BATCH_BYTES + 1).is_err());
    assert!(commands::validate_batch(1, limits::MAX_BATCH_BYTES).is_ok());
}

#[test]
fn future_schema_version_is_rejected_with_migration_error() {
    let dir = std::env::temp_dir().join(format!("tokensaver-future-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("db.sqlite");
    {
        let conn = rusqlite::Connection::open(&db_path).expect("raw connection");
        conn.pragma_update(None, "user_version", 99_i32).expect("set future version");
    }
    let err = db::open(&db_path).expect_err("schema futuro deve ser rejeitado");
    assert_eq!(err.code, "MIGRATION_ERROR");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn utf8_chunk_pagination_guarantees_progress_and_exact_reconstruction() {
    let original = "açé\u{1F600} texto com acentos e emoji \u{1F680} fim";
    let dir = std::env::temp_dir().join(format!("tokensaver-utf8-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let blob_path = dir.join("blob");
    std::fs::write(&blob_path, original.as_bytes()).expect("write blob");
    let total = original.len();
    for limit in [1usize, 2, 3, 7, 64] {
        let mut file = std::fs::File::open(&blob_path).expect("open blob");
        let mut offset = 0usize;
        let mut rebuilt = String::new();
        let mut steps = 0usize;
        loop {
            let (data, next_offset, eof) =
                file_center::read_utf8_chunk(&mut file, offset, total, limit).expect("chunk");
            assert!(next_offset > offset, "paginação sem progresso (limit={limit})");
            rebuilt.push_str(&data);
            offset = next_offset;
            steps += 1;
            assert!(steps <= total + 1, "loop sem fim detectado (limit={limit})");
            if eof { break; }
        }
        assert_eq!(rebuilt, original, "reconstrução exata falhou (limit={limit})");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn token_count_schema_accepts_local_estimate_metadata() {
    let dir = std::env::temp_dir().join(format!("tokensaver-token-count-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let conn = db::open(&dir.join("db.sqlite")).expect("database");
    conn.execute("INSERT INTO projects(id,name,created_at) VALUES('p','p','1')", []).expect("project");
    let file_id=uuid::Uuid::new_v4().to_string();
    let version_id=uuid::Uuid::new_v4().to_string();
    conn.execute("INSERT INTO files(id,project_id,logical_path,created_at) VALUES(?1,'p','a.md','1')",rusqlite::params![file_id]).expect("file");
    conn.execute("INSERT INTO blobs(sha256,byte_len,relative_path,created_at) VALUES('s',1,'blobs/s','1')",[]).expect("blob");
    conn.execute("INSERT INTO file_versions(id,file_id,version_no,blob_sha256,byte_len,created_at) VALUES(?1,?2,1,'s',1,'1')",rusqlite::params![version_id,file_id]).expect("version");
    conn.execute("INSERT INTO token_counts(id,file_version_id,method,count,approximate,limitation,created_at) VALUES('t',?1,'local_estimate',1,1,'approx','1')",rusqlite::params![version_id]).expect("count");
    let row:(String,i64,i64)=conn.query_row("SELECT method,count,approximate FROM token_counts WHERE file_version_id=?1",rusqlite::params![version_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).expect("query");
    assert_eq!(row,("local_estimate".into(),1,1));
    drop(conn); let _=std::fs::remove_dir_all(dir);
}


#[test]
fn read_blob_integrity_detects_same_size_corruption() {
    let dir = std::env::temp_dir().join(format!("tokensaver-integrity-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("blob");
    let original = b"same-size-original";
    std::fs::write(&path, original).expect("write");
    let mut file = std::fs::File::open(&path).expect("open");
    let sha = hash::sha256_bytes(original);
    file_center::verify_blob_integrity(&mut file, &sha, original.len()).expect("valid blob");
    drop(file);
    std::fs::write(&path, b"same-size-mutated").expect("mutate");
    let mut file = std::fs::File::open(&path).expect("reopen");
    let err = file_center::verify_blob_integrity(&mut file, &sha, original.len()).expect_err("corruption must be detected");
    assert_eq!(err.code, "STORAGE_ERROR");
    let _ = std::fs::remove_dir_all(dir);
}


#[test]
fn concurrent_blob_publication_is_no_replace_and_deduplicated() {
    use std::sync::Arc;
    use std::thread;

    let dir = std::env::temp_dir().join(format!("tokensaver-blob-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("blobs")).expect("storage");
    let root = Arc::new(dir.clone());
    let bytes: &'static [u8] = b"same immutable content";
    let sha = hash::sha256_bytes(bytes);
    let workers: Vec<_> = (0..8).map(|_| {
        let root = Arc::clone(&root);
        let sha = sha.clone();
        thread::spawn(move || ensure_blob_with_status(&root, &sha, bytes))
    }).collect();

    let mut created = 0;
    for worker in workers {
        let (_, was_created) = worker.join().expect("worker thread").expect("blob publication");
        if was_created { created += 1; }
    }
    assert_eq!(created, 1, "exactly one caller should publish the immutable blob");
    assert_eq!(std::fs::read(dir.join("blobs").join(&sha)).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(dir);
}
