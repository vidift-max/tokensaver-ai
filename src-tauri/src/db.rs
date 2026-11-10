use crate::error::{AppError, AppResult};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;

pub const SCHEMA_VERSION: i32 = 4;

pub fn open(path: &Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível preparar o diretório do banco."))?;
    }
    let mut conn = Connection::open(path)
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível abrir o banco do projeto."))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível configurar o banco do projeto."))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível configurar o banco do projeto."))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível configurar o banco do projeto."))?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível verificar a versão do banco."))?;
    if current > SCHEMA_VERSION {
        return Err(AppError::with_details(
            "MIGRATION_ERROR",
            "O banco foi criado por uma versão mais recente do aplicativo.",
            serde_json::json!({"supported_version": SCHEMA_VERSION, "found_version": current}),
        ));
    }
    if current == 1 {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível iniciar a migração."))?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS project_memory (key TEXT NOT NULL, value TEXT NOT NULL, updated_at TEXT NOT NULL, project_id TEXT NOT NULL, PRIMARY KEY(project_id, key));
CREATE INDEX IF NOT EXISTS idx_project_memory_project ON project_memory(project_id, key); ALTER TABLE operations ADD COLUMN project_id TEXT;")
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível atualizar o esquema do banco."))?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível registrar a versão do banco."))?;
        tx.commit()
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível concluir a migração."))?;
    }
    if current == 3 {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível iniciar a migração."))?;
        tx.execute_batch(r#"
CREATE TABLE project_memory_v4 (
 key TEXT NOT NULL,
 value TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 project_id TEXT NOT NULL,
 PRIMARY KEY(project_id, key)
);
INSERT INTO project_memory_v4(key,value,updated_at,project_id)
SELECT key,value,updated_at,'__legacy__' FROM project_memory;
DROP TABLE project_memory;
ALTER TABLE project_memory_v4 RENAME TO project_memory;
CREATE INDEX IF NOT EXISTS idx_project_memory_project ON project_memory(project_id, key);
"#).map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível atualizar a memória por projeto."))?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível registrar a versão do banco."))?;
        tx.commit()
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível concluir a migração."))?;
    }
    if current == 2 {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível iniciar a migração."))?;
        tx.execute_batch("ALTER TABLE operations ADD COLUMN project_id TEXT;")
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível atualizar o histórico de operações."))?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível registrar a versão do banco."))?;
        tx.commit()
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível concluir a migração."))?;
    }
    if current == 0 {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível iniciar a migração."))?;
        tx.execute_batch(r#"
CREATE TABLE IF NOT EXISTS projects (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS blobs (
 sha256 TEXT PRIMARY KEY, byte_len INTEGER NOT NULL CHECK(byte_len >= 0),
 relative_path TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
 logical_path TEXT NOT NULL, created_at TEXT NOT NULL,
 UNIQUE(project_id, logical_path)
);
CREATE TABLE IF NOT EXISTS file_versions (
 id TEXT PRIMARY KEY, file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
 version_no INTEGER NOT NULL CHECK(version_no > 0),
 blob_sha256 TEXT NOT NULL REFERENCES blobs(sha256),
 byte_len INTEGER NOT NULL CHECK(byte_len >= 0), created_at TEXT NOT NULL,
 UNIQUE(file_id, version_no)
);
CREATE TABLE IF NOT EXISTS token_counts (
 id TEXT PRIMARY KEY, file_version_id TEXT NOT NULL REFERENCES file_versions(id) ON DELETE CASCADE,
 method TEXT NOT NULL, count INTEGER NOT NULL CHECK(count >= 0),
 approximate INTEGER NOT NULL CHECK(approximate IN (0,1)),
 provider TEXT, model TEXT, limitation TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS project_memory (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS operations (
 id TEXT PRIMARY KEY, project_id TEXT, kind TEXT NOT NULL, status TEXT NOT NULL,
 progress INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL,
 finished_at TEXT, error_code TEXT
);
CREATE INDEX IF NOT EXISTS idx_file_versions_file_version ON file_versions(file_id, version_no);
CREATE INDEX IF NOT EXISTS idx_token_counts_version ON token_counts(file_version_id);
"#).map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível criar o esquema do banco."))?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível registrar a versão do banco."))?;
        tx.commit()
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível concluir a migração."))?;
    }
    // Keep v2 extensions idempotent so databases created by earlier v2 builds receive new modules.
    conn.execute_batch(r#"
CREATE TABLE IF NOT EXISTS templates (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, content TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS claude_config (
 id INTEGER PRIMARY KEY CHECK(id=1), enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
 endpoint TEXT NOT NULL, model TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_templates_name ON templates(name);
"#).map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível preparar o esquema dos módulos avançados."))?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn v1_migration_creates_v2_extensions() {
        let path = std::env::temp_dir().join(format!(
            "tokensaver-db-migration-{}.sqlite",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let mut conn = Connection::open(&path).expect("open");
        conn.execute_batch("PRAGMA user_version=1;").expect("set v1");
        migrate(&mut conn).expect("migrate");
        let version:i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        let memory:i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='project_memory'",[],|r|r.get(0)).unwrap();
        let templates:i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='templates'",[],|r|r.get(0)).unwrap();
        let claude:i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='claude_config'",[],|r|r.get(0)).unwrap();
        assert_eq!((memory,templates,claude),(1,1,1));
        drop(conn);
        let _=std::fs::remove_file(path);
    }
}
