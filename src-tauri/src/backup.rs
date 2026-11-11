use crate::{db, error::{AppError, AppResult}, filesystem};
use rusqlite::{backup::Backup, Connection};
use serde::{Deserialize, Serialize};
use crate::limits;
use std::{fs, io::{Read, Write}, path::{Component, Path, PathBuf}, time::Duration};
use tauri::Manager;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct BackupResult {
    pub project_id: String,
    pub backup_id: String,
    pub blob_count: usize,
    pub database_bytes: u64,
    pub backup_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BlobManifest {
    sha256: String,
    byte_len: u64,
    file_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    format_version: u32,
    project_id: String,
    schema_version: i32,
    created_at: String,
    database_sha256: String,
    blobs: Vec<BlobManifest>,
}


const MAX_BACKUP_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BACKUP_BLOBS: usize = 10_000;
const MAX_BACKUP_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BACKUP_PAYLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn app_storage(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    app.path().app_data_dir().map(|p| p.join("storage"))
        .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível localizar o armazenamento."))
}

fn validate_source_database_budget(database_path: &Path) -> AppResult<()> {
    let mut total = fs::symlink_metadata(database_path)
        .map_err(|_| AppError::new("PROJECT_NOT_FOUND", "Banco de dados do projeto indisponível."))?
        .len();
    let wal_path = database_path.with_file_name("db.sqlite-wal");
    match fs::symlink_metadata(&wal_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(AppError::new("STORAGE_ERROR", "O arquivo WAL do banco não é regular."));
            }
            total = total.checked_add(metadata.len())
                .ok_or_else(|| AppError::new("BACKUP_LIMIT_EXCEEDED", "O banco excede o limite de tamanho do backup."))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(AppError::new("STORAGE_ERROR", "Não foi possível verificar o arquivo WAL do banco.")),
    }
    if total > MAX_BACKUP_DATABASE_BYTES {
        return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O banco e seu WAL excedem o limite de tamanho do backup."));
    }
    Ok(())
}

fn validate_project_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::new("INVALID_REQUEST", "project_id inválido."));
    }
    Ok(())
}


fn safe_blob_source(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let path = Path::new(relative);
    let components: Vec<_> = path.components().collect();
    if components.len() != 2 || !matches!(components[0], Component::Normal(v) if v == "blobs")
        || !matches!(components[1], Component::Normal(_)) {
        return Err(AppError::new("BACKUP_ERROR", "O banco contém um caminho de blob inválido."));
    }
    let canonical_root = root.canonicalize().map_err(|_| AppError::new("STORAGE_ERROR", "Diretório do projeto indisponível."))?;
    let canonical_blobs = root.join("blobs").canonicalize().map_err(|_| AppError::new("STORAGE_ERROR", "Diretório de blobs indisponível."))?;
    let source = root.join(path).canonicalize().map_err(|_| AppError::new("BACKUP_ERROR", "Um blob referenciado não foi encontrado."))?;
    if !canonical_blobs.starts_with(&canonical_root) || source.parent() != Some(canonical_blobs.as_path()) || !source.is_file() {
        return Err(AppError::new("BACKUP_ERROR", "Um blob está fora do armazenamento autorizado."));
    }
    Ok(source)
}

#[tauri::command]
pub fn create_project_backup(app: tauri::AppHandle, project_id: String) -> AppResult<BackupResult> {
    validate_project_id(&project_id)?;
    let storage = app_storage(&app)?;
    let projects_root = storage.join("projects");
    let project_root = projects_root.join(&project_id);
    let source_db = project_root.join("db.sqlite");
    let projects_meta = fs::symlink_metadata(&projects_root)
        .map_err(|_| AppError::new("PROJECT_NOT_FOUND", "Pasta de projetos não encontrada."))?;
    let project_meta = fs::symlink_metadata(&project_root)
        .map_err(|_| AppError::new("PROJECT_NOT_FOUND", "O projeto ainda não possui armazenamento."))?;
    let db_meta = fs::symlink_metadata(&source_db)
        .map_err(|_| AppError::new("PROJECT_NOT_FOUND", "O projeto ainda não possui banco de dados."))?;
    if projects_meta.file_type().is_symlink() || !projects_meta.is_dir()
        || project_meta.file_type().is_symlink() || !project_meta.is_dir()
        || db_meta.file_type().is_symlink() || !db_meta.is_file() {
        return Err(AppError::new("STORAGE_ERROR", "O armazenamento do projeto não é um diretório ou arquivo regular."));
    }
    let canonical_storage = storage.canonicalize()
        .map_err(|_| AppError::new("STORAGE_ERROR", "Armazenamento indisponível."))?;
    let canonical_projects = projects_root.canonicalize()
        .map_err(|_| AppError::new("STORAGE_ERROR", "Pasta de projetos indisponível."))?;
    let canonical_project_root = project_root.canonicalize()
        .map_err(|_| AppError::new("STORAGE_ERROR", "Diretório do projeto indisponível."))?;
    let canonical_source_db = source_db.canonicalize()
        .map_err(|_| AppError::new("PROJECT_NOT_FOUND", "Banco de dados do projeto indisponível."))?;
    if !canonical_projects.starts_with(&canonical_storage)
        || canonical_projects.parent() != Some(canonical_storage.as_path())
        || canonical_project_root.parent() != Some(canonical_projects.as_path())
        || canonical_project_root.file_name().and_then(|v| v.to_str()) != Some(project_id.as_str())
        || canonical_source_db.parent() != Some(canonical_project_root.as_path()) {
        return Err(AppError::new("STORAGE_ERROR", "O projeto está fora do armazenamento autorizado."));
    }

    validate_source_database_budget(&source_db)?;

    let backup_id = Uuid::new_v4().to_string();
    let backup_parent = storage.join("backups").join(&project_id);
    fs::create_dir_all(&backup_parent).map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível preparar a pasta de backups."))?;
    let canonical_storage = storage.canonicalize().map_err(|_| AppError::new("STORAGE_ERROR", "Armazenamento indisponível."))?;
    let canonical_backups = storage.join("backups").canonicalize().map_err(|_| AppError::new("BACKUP_ERROR", "Pasta de backups indisponível."))?;
    let canonical_backup_parent = backup_parent.canonicalize().map_err(|_| AppError::new("BACKUP_ERROR", "Pasta do projeto de backup indisponível."))?;
    if !canonical_backups.starts_with(&canonical_storage) || canonical_backup_parent.parent() != Some(canonical_backups.as_path()) || canonical_backup_parent.file_name().and_then(|v| v.to_str()) != Some(project_id.as_str()) {
        return Err(AppError::new("BACKUP_ERROR", "Pasta de backup fora do armazenamento autorizado."));
    }
    let staging = backup_parent.join(format!(".{backup_id}.tmp"));
    let final_dir = backup_parent.join(&backup_id);
    if fs::symlink_metadata(&final_dir).is_ok() {
        return Err(AppError::new("BACKUP_ERROR", "Já existe um backup com esse identificador."));
    }
    fs::create_dir(&staging).map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível criar a área temporária do backup."))?;

    let result = (|| -> AppResult<BackupResult> {
        let source = db::open(&source_db)?;
        let mut destination = Connection::open(staging.join("db.sqlite"))
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível criar a cópia do banco."))?;
        {
            let backup = Backup::new(&source, &mut destination)
                .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível iniciar a cópia consistente do SQLite."))?;
            backup.run_to_completion(64, Duration::from_millis(10), None)
                .map_err(|_| AppError::new("BACKUP_ERROR", "A cópia consistente do SQLite não foi concluída."))?;
        }
        destination.execute_batch("PRAGMA journal_mode=DELETE;")
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível finalizar o banco de backup."))?;
        let integrity: String = destination.query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível validar o banco de backup."))?;
        if integrity != "ok" { return Err(AppError::new("BACKUP_ERROR", "A validação do banco de backup falhou.")); }
        drop(destination);
        drop(source);

        let snapshot = Connection::open(staging.join("db.sqlite"))
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível consultar o banco de backup."))?;
        let mut stmt = snapshot.prepare("SELECT sha256, byte_len, relative_path FROM blobs ORDER BY sha256")
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível listar os blobs."))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível listar os blobs."))?;
        let blob_dir = staging.join("blobs");
        fs::create_dir(&blob_dir).map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível preparar os blobs do backup."))?;
        let mut manifest_blobs = Vec::new();
        let mut payload_bytes = 0u64;
        for row in rows {
            if manifest_blobs.len() >= MAX_BACKUP_BLOBS { return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O backup excede o número máximo de blobs.")); }
            let (sha, expected_len, relative) = row.map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível ler um registro de blob."))?;
            if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) || expected_len < 0 {
                return Err(AppError::new("BACKUP_ERROR", "O banco contém metadados de blob inválidos."));
            }
            if expected_len as u64 > limits::MAX_FILE_BYTES {
                return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "Um blob excede o limite de tamanho permitido."));
            }
            let source = safe_blob_source(&project_root, &relative)?;
            let file_name = sha.to_ascii_lowercase();
            let target = blob_dir.join(&file_name);
            let next_payload = payload_bytes.checked_add(expected_len as u64).ok_or_else(|| AppError::new("BACKUP_LIMIT_EXCEEDED", "O payload do backup excede o limite."))?;
            if next_payload > MAX_BACKUP_PAYLOAD_BYTES { return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O payload do backup excede o limite.")); }
            let copied_len = filesystem::copy_file_limited(&source, &target, limits::MAX_FILE_BYTES)
                .map_err(|err| if err.kind() == std::io::ErrorKind::FileTooLarge {
                    AppError::new("BACKUP_LIMIT_EXCEEDED", "Um blob excede o limite de tamanho permitido.")
                } else {
                    AppError::new("BACKUP_ERROR", "Não foi possível copiar um blob.")
                })?;
            let (actual_sha, actual_len) = filesystem::sha256_file_limited(&target, limits::MAX_FILE_BYTES)
                .map_err(|_| AppError::new("BACKUP_LIMIT_EXCEEDED", "Um blob excede o limite de tamanho permitido."))?;
            if copied_len != actual_len {
                return Err(AppError::new("BACKUP_ERROR", "O tamanho do blob mudou durante a cópia."));
            }
            if actual_sha != sha.to_ascii_lowercase() || actual_len != expected_len as u64 {
                return Err(AppError::new("BACKUP_ERROR", "Um blob não passou na validação SHA-256 e tamanho."));
            }
            payload_bytes = next_payload;
            manifest_blobs.push(BlobManifest { sha256: actual_sha, byte_len: actual_len, file_name });
        }
        drop(stmt);
        drop(snapshot);

        let (database_sha256, database_bytes) = filesystem::sha256_file_limited(&staging.join("db.sqlite"), MAX_BACKUP_DATABASE_BYTES).map_err(|_| AppError::new("BACKUP_LIMIT_EXCEEDED", "O banco excede o limite de tamanho do backup."))?;
        let manifest = BackupManifest {
            format_version: 1,
            project_id: project_id.clone(),
            schema_version: crate::db::SCHEMA_VERSION,
            created_at: format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()),
            database_sha256,
            blobs: manifest_blobs,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível gerar o manifesto do backup."))?;
        if manifest_bytes.len() as u64 > MAX_BACKUP_MANIFEST_BYTES { return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O manifesto excede o limite permitido.")); }
        let mut manifest_file = fs::File::create(staging.join("manifest.json"))
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível gravar o manifesto do backup."))?;
        manifest_file.write_all(&manifest_bytes)
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível finalizar o manifesto do backup."))?;
        manifest_file.sync_all().map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível sincronizar o manifesto do backup."))?;

        filesystem::publish_noreplace(&staging, &final_dir)
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível publicar o backup completo sem substituir outro backup."))?;
        Ok(BackupResult {
            project_id, backup_id, blob_count: manifest.blobs.len(), database_bytes,
            backup_path: final_dir.to_string_lossy().into_owned(),
        })
    })();

    if result.is_err() { let _ = fs::remove_dir_all(&staging); }
    result
}

fn remap_project_scope(
    tx: &rusqlite::Transaction<'_>,
    source_project_id: &str,
    target_project_id: &str,
) -> AppResult<()> {
    tx.execute(
        "UPDATE files SET project_id=?1 WHERE project_id=?2",
        rusqlite::params![target_project_id, source_project_id],
    )
    .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível associar os arquivos ao novo projeto."))?;
    tx.execute(
        "UPDATE project_memory SET project_id=?1 WHERE project_id=?2",
        rusqlite::params![target_project_id, source_project_id],
    )
    .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível associar a memória ao novo projeto."))?;
    tx.execute(
        "UPDATE operations SET project_id=?1 WHERE project_id=?2",
        rusqlite::params![target_project_id, source_project_id],
    )
    .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível associar o histórico ao novo projeto."))?;
    Ok(())
}

fn publish_restore_stage(stage: &Path, target: &Path) -> std::io::Result<()> {
    let result = filesystem::publish_noreplace(stage, target);
    if result.is_err() {
        let _ = fs::remove_dir_all(stage);
    }
    result
}

fn prepare_restore_projects_root(storage: &Path) -> AppResult<PathBuf> {
    let projects_root = storage.join("projects");
    // Never use create_dir_all here: if "projects" is a symlink, it can
    // create directories at the symlink destination before we reject it.
    crate::project_storage::claim_project_root(&projects_root)
        .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível preparar a pasta de projetos."))?;
    Ok(projects_root)
}

fn insert_restore_db_blob(
    blobs: &mut std::collections::BTreeMap<String, u64>,
    sha: String,
    len: i64,
    relative: String,
) -> AppResult<()> {
    if blobs.len() >= MAX_BACKUP_BLOBS {
        return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O banco do backup excede o número máximo de blobs."));
    }
    // SHA-256 identifiers and their relative paths are canonical lowercase.
    // Accepting uppercase here would normalize only the comparison map while
    // leaving the restored SQLite row pointing at a differently-cased path.
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        || sha != sha.to_ascii_lowercase()
        || len < 0 || relative != format!("blobs/{}", sha) {
        return Err(AppError::new("BACKUP_INVALID", "Metadados de blob inválidos no banco."));
    }
    if blobs.insert(sha, len as u64).is_some() {
        return Err(AppError::new("BACKUP_INVALID", "O banco do backup contém blobs duplicados."));
    }
    Ok(())
}

fn validate_backup_id(id: &str) -> AppResult<()> {
    let parsed = Uuid::parse_str(id).map_err(|_| AppError::new("INVALID_REQUEST", "backup_id inválido."))?;
    if parsed.to_string() != id.to_ascii_lowercase() {
        return Err(AppError::new("INVALID_REQUEST", "backup_id inválido."));
    }
    Ok(())
}

#[tauri::command]
pub fn restore_project_backup(
    app: tauri::AppHandle,
    source_project_id: String,
    backup_id: String,
    target_project_id: String,
) -> AppResult<BackupResult> {
    validate_project_id(&source_project_id)?;
    validate_project_id(&target_project_id)?;
    validate_backup_id(&backup_id)?;
    if source_project_id == target_project_id {
        return Err(AppError::new("RESTORE_TARGET_EXISTS", "A restauração deve criar um projeto novo; o projeto original não será sobrescrito."));
    }

    let storage = app_storage(&app)?;
    let backup_parent = storage.join("backups").join(&source_project_id);
    let backup_dir = backup_parent.join(&backup_id);
    let metadata = fs::symlink_metadata(&backup_dir)
        .map_err(|_| AppError::new("BACKUP_NOT_FOUND", "Backup não encontrado."))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::new("BACKUP_INVALID", "A pasta do backup não é válida."));
    }
    let canonical_parent = backup_parent.canonicalize()
        .map_err(|_| AppError::new("BACKUP_NOT_FOUND", "Pasta de backups não encontrada."))?;
    let canonical_storage = storage.canonicalize()
        .map_err(|_| AppError::new("STORAGE_ERROR", "Armazenamento indisponível."))?;
    let canonical_backups = storage.join("backups").canonicalize()
        .map_err(|_| AppError::new("BACKUP_NOT_FOUND", "Pasta de backups não encontrada."))?;
    if !canonical_backups.starts_with(&canonical_storage) || canonical_parent.parent() != Some(canonical_backups.as_path()) || canonical_parent.file_name().and_then(|v| v.to_str()) != Some(source_project_id.as_str()) {
        return Err(AppError::new("BACKUP_INVALID", "Pasta de backup fora do armazenamento autorizado."));
    }
    let canonical_backup = backup_dir.canonicalize()
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível validar a pasta do backup."))?;
    if canonical_backup.parent() != Some(canonical_parent.as_path()) {
        return Err(AppError::new("BACKUP_INVALID", "O backup está fora da pasta autorizada."));
    }

    let manifest_path = canonical_backup.join("manifest.json");
    let manifest_meta = fs::symlink_metadata(&manifest_path)
        .map_err(|_| AppError::new("BACKUP_INVALID", "Manifesto ausente."))?;
    if manifest_meta.file_type().is_symlink() || !manifest_meta.is_file() {
        return Err(AppError::new("BACKUP_INVALID", "Manifesto inválido."));
    }
    let manifest_bytes = { let f = fs::File::open(&manifest_path).map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível ler o manifesto."))?; let mut bytes = Vec::new(); f.take(MAX_BACKUP_MANIFEST_BYTES.saturating_add(1)).read_to_end(&mut bytes).map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível ler o manifesto."))?; if bytes.len() as u64 > MAX_BACKUP_MANIFEST_BYTES { return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O manifesto excede o limite permitido.")); } bytes }; let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes).map_err(|_| AppError::new("BACKUP_INVALID", "Manifesto do backup inválido."))?; if manifest.blobs.len() > MAX_BACKUP_BLOBS { return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O backup excede o número máximo de blobs.")); }
    if manifest.format_version != 1 || manifest.project_id != source_project_id
        || manifest.schema_version != crate::db::SCHEMA_VERSION {
        return Err(AppError::new("BACKUP_INCOMPATIBLE", "O backup pertence a outro projeto ou usa um formato incompatível."));
    }

    let backup_db = canonical_backup.join("db.sqlite");
    let db_meta = fs::symlink_metadata(&backup_db)
        .map_err(|_| AppError::new("BACKUP_INVALID", "Banco do backup ausente."))?;
    if db_meta.file_type().is_symlink() || !db_meta.is_file() {
        return Err(AppError::new("BACKUP_INVALID", "Banco do backup inválido."));
    }
    let (db_hash, db_bytes) = filesystem::sha256_file_limited(&backup_db, MAX_BACKUP_DATABASE_BYTES).map_err(|_| AppError::new("BACKUP_LIMIT_EXCEEDED", "O banco excede o limite de tamanho do backup."))?;
    if db_hash != manifest.database_sha256 {
        return Err(AppError::new("BACKUP_CORRUPT", "O banco do backup não passou na validação SHA-256."));
    }

    let snapshot = Connection::open(&backup_db)
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível abrir o banco do backup."))?;
    let integrity: String = snapshot.query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível validar o banco do backup."))?;
    if integrity != "ok" {
        return Err(AppError::new("BACKUP_CORRUPT", "A integridade SQLite do backup falhou."));
    }
    let mut fk_stmt = snapshot.prepare("PRAGMA foreign_key_check")
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível validar as referências do banco."))?;
    if fk_stmt.exists([]).map_err(|_| AppError::new("BACKUP_INVALID", "Falha ao validar referências do banco."))? {
        return Err(AppError::new("BACKUP_CORRUPT", "O backup contém referências SQLite quebradas."));
    }
    drop(fk_stmt);

    let mut db_blobs = std::collections::BTreeMap::new();
    let mut stmt = snapshot.prepare("SELECT sha256, byte_len, relative_path FROM blobs")
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível listar os blobs do banco."))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível listar os blobs do banco."))?;
    for row in rows {
        let (sha, len, relative) = row.map_err(|_| AppError::new("BACKUP_INVALID", "Registro de blob inválido."))?;
        insert_restore_db_blob(&mut db_blobs, sha, len, relative)?;
    }
    drop(stmt);
    drop(snapshot);

    let backup_blobs_dir = canonical_backup.join("blobs");
    let blobs_meta = fs::symlink_metadata(&backup_blobs_dir)
        .map_err(|_| AppError::new("BACKUP_INVALID", "Pasta de blobs do backup ausente."))?;
    if blobs_meta.file_type().is_symlink() || !blobs_meta.is_dir()
        || backup_blobs_dir.canonicalize().ok().and_then(|p| p.parent().map(|v| v.to_path_buf())) != Some(canonical_backup.clone()) {
        return Err(AppError::new("BACKUP_INVALID", "Pasta de blobs do backup inválida."));
    }

    let mut manifest_blobs = std::collections::BTreeMap::new();
    let mut restore_payload_bytes = 0u64;
    for blob in &manifest.blobs {
        if blob.sha256.len() != 64 || !blob.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || blob.file_name != blob.sha256.to_ascii_lowercase()
            || manifest_blobs.insert(blob.sha256.to_ascii_lowercase(), blob.byte_len).is_some() {
            return Err(AppError::new("BACKUP_INVALID", "Manifesto contém blob inválido ou duplicado."));
        }
        let path = backup_blobs_dir.join(&blob.file_name);
        let meta = fs::symlink_metadata(&path)
            .map_err(|_| AppError::new("BACKUP_CORRUPT", "Um blob do backup está ausente."))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(AppError::new("BACKUP_INVALID", "Um blob do backup não é um arquivo regular."));
        }
        let (hash, len) = filesystem::sha256_file_limited(&path, limits::MAX_FILE_BYTES).map_err(|_| AppError::new("BACKUP_LIMIT_EXCEEDED", "Um blob excede o limite de tamanho permitido."))?;
        restore_payload_bytes = restore_payload_bytes.checked_add(len).ok_or_else(|| AppError::new("BACKUP_LIMIT_EXCEEDED", "O payload do backup excede o limite."))?;
        if restore_payload_bytes > MAX_BACKUP_PAYLOAD_BYTES { return Err(AppError::new("BACKUP_LIMIT_EXCEEDED", "O payload do backup excede o limite.")); }
        if hash != blob.sha256 || len != blob.byte_len {
            return Err(AppError::new("BACKUP_CORRUPT", "Um blob do backup falhou na validação SHA-256."));
        }
    }
    if manifest_blobs != db_blobs {
        return Err(AppError::new("BACKUP_CORRUPT", "Manifesto e banco possuem listas de blobs diferentes."));
    }
    let entries = fs::read_dir(&backup_blobs_dir)
        .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível listar os blobs do backup."))?;
    for entry in entries {
        let entry = entry.map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível ler a pasta de blobs do backup."))?;
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível validar um arquivo do backup."))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(AppError::new("BACKUP_INVALID", "A pasta de blobs contém uma entrada inválida."));
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !manifest_blobs.contains_key(&name) {
            return Err(AppError::new("BACKUP_CORRUPT", "O backup contém um blob não registrado no manifesto."));
        }
    }

    let projects_root = prepare_restore_projects_root(&storage)?;
    let target = projects_root.join(&target_project_id);
    let projects_meta = fs::symlink_metadata(&projects_root)
        .map_err(|_| AppError::new("RESTORE_ERROR", "Pasta de projetos indisponível."))?;
    let canonical_projects = projects_root.canonicalize()
        .map_err(|_| AppError::new("RESTORE_ERROR", "Pasta de projetos indisponível."))?;
    if projects_meta.file_type().is_symlink() || !projects_meta.is_dir()
        || canonical_projects.parent() != Some(canonical_storage.as_path()) {
        return Err(AppError::new("RESTORE_ERROR", "Pasta de projetos fora do armazenamento autorizado."));
    }
    if fs::symlink_metadata(&target).is_ok() {
        return Err(AppError::new("RESTORE_TARGET_EXISTS", "Já existe um projeto com esse identificador. Nada foi alterado."));
    }
    let stage = projects_root.join(format!(".restore-{}.tmp", Uuid::new_v4()));
    fs::create_dir(&stage)
        .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível criar a área temporária da restauração."))?;

    let restored = (|| -> AppResult<()> {
        fs::create_dir(stage.join("blobs"))
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível preparar os blobs restaurados."))?;
        for blob in &manifest.blobs {
            filesystem::copy_file_limited(
                &canonical_backup.join("blobs").join(&blob.file_name),
                &stage.join("blobs").join(&blob.file_name),
                limits::MAX_FILE_BYTES,
            )
            .map_err(|err| if err.kind() == std::io::ErrorKind::FileTooLarge {
                AppError::new("BACKUP_LIMIT_EXCEEDED", "Um blob excede o limite de tamanho permitido.")
            } else {
                AppError::new("RESTORE_ERROR", "Não foi possível restaurar um blob.")
            })?;
        }
        filesystem::copy_file_limited(
            &backup_db,
            &stage.join("db.sqlite"),
            MAX_BACKUP_DATABASE_BYTES,
        )
        .map_err(|err| if err.kind() == std::io::ErrorKind::FileTooLarge {
            AppError::new("BACKUP_LIMIT_EXCEEDED", "O banco excede o limite de tamanho do backup.")
        } else {
            AppError::new("RESTORE_ERROR", "Não foi possível restaurar o banco.")
        })?;
        let mut restored_db = Connection::open(stage.join("db.sqlite"))
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível abrir o banco restaurado."))?;
        restored_db.pragma_update(None, "foreign_keys", "ON")
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível proteger as referências do banco."))?;
        let tx = restored_db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível iniciar a atualização do projeto restaurado."))?;
        let (name, created_at): (String, String) = tx.query_row(
            "SELECT name, created_at FROM projects WHERE id=?1", [&source_project_id],
            |r| Ok((r.get(0)?, r.get(1)?))
        ).map_err(|_| AppError::new("BACKUP_INVALID", "O banco não contém o projeto esperado."))?;
        tx.execute("INSERT INTO projects(id,name,created_at) VALUES(?1,?2,?3)",
            rusqlite::params![target_project_id, name, created_at])
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível registrar o projeto restaurado."))?;
        remap_project_scope(&tx, &source_project_id, &target_project_id)?;
        tx.execute("DELETE FROM projects WHERE id=?1", [&source_project_id])
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível finalizar a identificação do projeto restaurado."))?;
        tx.commit().map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível confirmar a restauração."))?;
        drop(restored_db);
        publish_restore_stage(&stage, &target)
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível publicar o projeto restaurado: o destino já existe ou a publicação segura não está disponível."))?;
        Ok(())
    })();

    if restored.is_err() { let _ = fs::remove_dir_all(&stage); }
    restored?;
    Ok(BackupResult {
        project_id: target_project_id,
        backup_id,
        blob_count: manifest.blobs.len(),
        database_bytes: db_bytes,
        backup_path: target.to_string_lossy().into_owned(),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

 

    #[test]
    fn concurrent_restore_stages_publish_exactly_one_and_clean_loser() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let base = std::env::temp_dir().join(format!("tokensaver-restore-race-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let target = Arc::new(base.join("restored-project"));
        let stage_a = base.join(".restore-a");
        let stage_b = base.join(".restore-b");
        fs::create_dir(&stage_a).unwrap();
        fs::create_dir(&stage_b).unwrap();
        fs::write(stage_a.join("winner"), b"a").unwrap();
        fs::write(stage_b.join("winner"), b"b").unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let spawn = |stage: std::path::PathBuf| {
            let target = Arc::clone(&target);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                publish_restore_stage(&stage, &target)
            })
        };
        let a = spawn(stage_a.clone());
        let b = spawn(stage_b.clone());
        barrier.wait();
        let result_a = a.join().unwrap();
        let result_b = b.join().unwrap();

        assert_ne!(result_a.is_ok(), result_b.is_ok(), "exactly one restore may publish");
        assert!(target.is_dir());
        assert_eq!(
            fs::read(target.join("winner")).unwrap(),
            if result_a.is_ok() { b"a" } else { b"b" }
        );
        assert_eq!(stage_a.exists(), result_a.is_ok());
        assert_eq!(stage_b.exists(), result_b.is_ok());

        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn restore_projects_root_symlink_is_rejected_without_creating_outside_target() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("tokensaver-restore-root-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let storage = base.join("storage");
        fs::create_dir_all(&storage).unwrap();
        let outside = base.join("outside-not-created");
        symlink(&outside, storage.join("projects")).unwrap();

        let error = prepare_restore_projects_root(&storage)
            .expect_err("restore must reject a symlinked projects root");
        assert_eq!(error.code, "RESTORE_ERROR");
        assert!(!outside.exists(), "symlink destination must not be created as a side effect");

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn restore_db_blob_map_enforces_manifest_blob_cap() {
        let mut blobs = std::collections::BTreeMap::new();
        for index in 0..MAX_BACKUP_BLOBS {
            blobs.insert(format!("{index:064x}"), 1);
        }
        let sha = "f".repeat(64);
        let error = insert_restore_db_blob(&mut blobs, sha.clone(), 1, format!("blobs/{sha}"))
            .expect_err("database blob list above cap must be rejected");
        assert_eq!(error.code, "BACKUP_LIMIT_EXCEEDED");
        assert_eq!(blobs.len(), MAX_BACKUP_BLOBS);
    }

    #[test]
    fn restore_db_blob_map_rejects_duplicate_and_invalid_relative_path() {
        let mut blobs = std::collections::BTreeMap::new();
        let sha = "a".repeat(64);
        insert_restore_db_blob(&mut blobs, sha.clone(), 1, format!("blobs/{sha}")).unwrap();
        let duplicate = insert_restore_db_blob(&mut blobs, sha.clone(), 1, format!("blobs/{sha}"))
            .expect_err("duplicate SHA must be rejected");
        assert_eq!(duplicate.code, "BACKUP_INVALID");
        let invalid = insert_restore_db_blob(&mut blobs, "b".repeat(64), 1, "outside/blob".into())
            .expect_err("path outside blobs directory must be rejected");
        assert_eq!(invalid.code, "BACKUP_INVALID");
    }

    #[test]
    fn restore_db_blob_map_rejects_uppercase_sha_and_path() {
        let mut blobs = std::collections::BTreeMap::new();
        let uppercase_sha = "A".repeat(64);
        let error = insert_restore_db_blob(
            &mut blobs,
            uppercase_sha.clone(),
            1,
            format!("blobs/{uppercase_sha}"),
        ).expect_err("uppercase SHA must not leave a non-canonical restored path");
        assert_eq!(error.code, "BACKUP_INVALID");
        assert!(blobs.is_empty());

        let canonical_sha = "b".repeat(64);
        let uppercase_path = format!("blobs/{}", canonical_sha.to_ascii_uppercase());
        let path_error = insert_restore_db_blob(
            &mut blobs,
            canonical_sha,
            1,
            uppercase_path,
        ).expect_err("relative blob path must also be canonical lowercase");
        assert_eq!(path_error.code, "BACKUP_INVALID");
        assert!(blobs.is_empty());
    }

    #[test]
    fn source_database_budget_counts_database_and_wal() {
        let root = std::env::temp_dir().join(format!("tokensaver-db-budget-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("db.sqlite");
        fs::write(&database, vec![0u8; 8]).unwrap();
        fs::write(root.join("db.sqlite-wal"), vec![0u8; 8]).unwrap();
        assert!(validate_source_database_budget(&database).is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_database_budget_rejects_combined_database_and_wal_over_limit() {
        let root = std::env::temp_dir().join(format!("tokensaver-db-budget-over-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("db.sqlite");
        let file = fs::File::create(&database).unwrap();
        file.set_len(MAX_BACKUP_DATABASE_BYTES).unwrap();
        fs::write(root.join("db.sqlite-wal"), b"x").unwrap();
        assert!(validate_source_database_budget(&database).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_database_budget_rejects_non_file_wal() {
        let root = std::env::temp_dir().join(format!("tokensaver-db-wal-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = root.join("db.sqlite");
        fs::write(&database, b"db").unwrap();
        fs::create_dir(root.join("db.sqlite-wal")).unwrap();
        assert!(validate_source_database_budget(&database).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_and_backup_ids_reject_path_components() {
        assert!(validate_project_id("project-01").is_ok());
        assert!(validate_project_id("../outside").is_err());
        assert!(validate_project_id("nested/project").is_err());
        assert!(validate_backup_id(&Uuid::new_v4().to_string()).is_ok());
        assert!(validate_backup_id("../backup").is_err());
    }

    #[test]
    fn remap_project_scope_moves_only_requested_project_rows() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(r#"
            CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL);
            CREATE TABLE files (id TEXT PRIMARY KEY, project_id TEXT NOT NULL);
            CREATE TABLE project_memory (key TEXT NOT NULL, value TEXT NOT NULL, updated_at TEXT NOT NULL, project_id TEXT NOT NULL);
            CREATE TABLE operations (id TEXT PRIMARY KEY, project_id TEXT, kind TEXT NOT NULL, status TEXT NOT NULL, progress INTEGER NOT NULL, created_at TEXT NOT NULL, finished_at TEXT, error_code TEXT);
            INSERT INTO projects VALUES ('source','Source','1'),('other','Other','1');
            INSERT INTO files VALUES ('f1','source'),('f2','other');
            INSERT INTO project_memory VALUES ('k1','v1','1','source'),('k2','v2','1','other');
            INSERT INTO operations VALUES ('o1','source','import','completed',100,'1',NULL,NULL),('o2','other','import','completed',100,'1',NULL,NULL);
        "#).expect("prepare");

        let tx = conn.unchecked_transaction().expect("transaction");
        remap_project_scope(&tx, "source", "restored").expect("remap");
        tx.commit().expect("commit");

        let source_files:i64 = conn.query_row("SELECT count(*) FROM files WHERE project_id='source'",[],|r|r.get(0)).unwrap();
        let restored_files:i64 = conn.query_row("SELECT count(*) FROM files WHERE project_id='restored'",[],|r|r.get(0)).unwrap();
        let other_files:i64 = conn.query_row("SELECT count(*) FROM files WHERE project_id='other'",[],|r|r.get(0)).unwrap();
        let restored_memory:i64 = conn.query_row("SELECT count(*) FROM project_memory WHERE project_id='restored'",[],|r|r.get(0)).unwrap();
        let other_memory:i64 = conn.query_row("SELECT count(*) FROM project_memory WHERE project_id='other'",[],|r|r.get(0)).unwrap();
        let restored_ops:i64 = conn.query_row("SELECT count(*) FROM operations WHERE project_id='restored'",[],|r|r.get(0)).unwrap();
        let other_ops:i64 = conn.query_row("SELECT count(*) FROM operations WHERE project_id='other'",[],|r|r.get(0)).unwrap();

        assert_eq!((source_files, restored_files, other_files), (0, 1, 1));
        assert_eq!((restored_memory, other_memory), (1, 1));
        assert_eq!((restored_ops, other_ops), (1, 1));
    }

    #[test]
    fn blob_source_accepts_only_direct_files_inside_project_blobs() {
        let root = std::env::temp_dir().join(format!("tokensaver-backup-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("blobs")).expect("create blob directory");
        let bytes = b"backup test";
        fs::write(root.join("blobs/test-blob"), bytes).expect("write blob");

        assert!(safe_blob_source(&root, "blobs/test-blob").is_ok());
        assert!(safe_blob_source(&root, "../outside").is_err());
        assert!(safe_blob_source(&root, "blobs/../../outside").is_err());
        assert!(safe_blob_source(&root, "other/test-blob").is_err());

        let _ = fs::remove_dir_all(root);
    }
}
