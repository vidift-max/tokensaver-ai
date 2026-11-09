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
    let deep=(0..(limits::MAX_PATH_DEPTH+1)).map(|_| "x").collect::<Vec<_>>().join("/");
    assert!(safe_logical_path(&deep).is_err());
}

#[test]
fn token_estimate_is_explicitly_approximate() {
    let value=estimate_tokens("123456789").expect("estimate");
    assert_eq!(value.method,"local_estimate");
    assert!(value.approximate);
    assert_eq!(value.count,3);
}

#[test]
fn sqlite_migration_creates_required_tables() {
    let dir=std::env::temp_dir().join(format!("tokensaver-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let conn=db::open(&dir.join("db.sqlite")).expect("database");
    let count:i64=conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('projects','blobs','files','file_versions','token_counts','operations')",[],|r|r.get(0)).expect("schema query");
    assert_eq!(count,6);
    let version:i32=conn.pragma_query_value(None,"user_version",|r|r.get(0)).expect("version");
    assert_eq!(version,1);
    drop(conn);
    let _=std::fs::remove_dir_all(dir);
}
