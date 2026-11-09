use super::*;

#[test]
fn logical_path_rejects_traversal_and_absolute_paths() {
    assert!(safe_logical_path("../escape.txt").is_err());
    assert!(safe_logical_path("/absolute.txt").is_err());
    assert!(safe_logical_path("C:\\escape.txt").is_err() || cfg!(not(windows)));
    assert!(safe_logical_path("docs/article.html").is_ok());
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
    assert_eq!(version, 1);
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
