use crate::error::{AppError, AppResult};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;

pub const SCHEMA_VERSION: i32 = 2;

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
        tx.execute_batch("CREATE TABLE IF NOT EXISTS project_memory (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL);")
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível atualizar o esquema do banco."))?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível registrar a versão do banco."))?;
        tx.commit()
            .map_err(|_| AppError::new("MIGRATION_ERROR", "Não foi possível concluir a migração."))?;
        return Ok(());
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
 id TEXT PRIMARY KEY, kind TEXT NOT NULL, status TEXT NOT NULL,
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
    Ok(())
}
