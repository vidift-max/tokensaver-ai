use crate::{db, error::{AppError, AppResult}};
use rusqlite::{backup::Backup, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};
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

#[derive(Serialize)]
struct BlobManifest {
    sha256: String,
    byte_len: u64,
    file_name: String,
}

#[derive(Serialize)]
struct BackupManifest {
    format_version: u32,
    project_id: String,
    schema_version: i32,
    created_at: String,
    database_sha256: String,
    blobs: Vec<BlobManifest>,
}

fn app_storage(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    app.path().app_data_dir().map(|p| p.join("storage"))
        .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível localizar o armazenamento."))
}

fn validate_project_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::new("INVALID_REQUEST", "project_id inválido."));
    }
    Ok(())
}

fn hash_file(path: &Path) -> AppResult<(String, u64)> {
    let mut file = fs::File::open(path).map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível ler um arquivo do backup."))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|_| AppError::new("BACKUP_ERROR", "Falha ao calcular SHA-256 do backup."))?;
        if n == 0 { break; }
        size = size.saturating_add(n as u64);
        hasher.update(&buffer[..n]);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
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
    let project_root = storage.join("projects").join(&project_id);
    let source_db = project_root.join("db.sqlite");
    if !source_db.is_file() {
        return Err(AppError::new("PROJECT_NOT_FOUND", "O projeto ainda não possui banco de dados."));
    }

    let backup_id = Uuid::new_v4().to_string();
    let backup_parent = storage.join("backups").join(&project_id);
    fs::create_dir_all(&backup_parent).map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível preparar a pasta de backups."))?;
    let staging = backup_parent.join(format!(".{backup_id}.tmp"));
    let final_dir = backup_parent.join(&backup_id);
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
        for row in rows {
            let (sha, expected_len, relative) = row.map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível ler um registro de blob."))?;
            if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) || expected_len < 0 {
                return Err(AppError::new("BACKUP_ERROR", "O banco contém metadados de blob inválidos."));
            }
            let source = safe_blob_source(&project_root, &relative)?;
            let file_name = sha.to_ascii_lowercase();
            let target = blob_dir.join(&file_name);
            fs::copy(&source, &target).map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível copiar um blob."))?;
            let (actual_sha, actual_len) = hash_file(&target)?;
            if actual_sha != sha.to_ascii_lowercase() || actual_len != expected_len as u64 {
                return Err(AppError::new("BACKUP_ERROR", "Um blob não passou na validação SHA-256 e tamanho."));
            }
            manifest_blobs.push(BlobManifest { sha256: actual_sha, byte_len: actual_len, file_name });
        }
        drop(stmt);
        drop(snapshot);

        let (database_sha256, database_bytes) = hash_file(&staging.join("db.sqlite"))?;
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
        let mut manifest_file = fs::File::create(staging.join("manifest.json"))
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível gravar o manifesto do backup."))?;
        manifest_file.write_all(&manifest_bytes)
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível finalizar o manifesto do backup."))?;
        manifest_file.sync_all().map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível sincronizar o manifesto do backup."))?;

        fs::rename(&staging, &final_dir)
            .map_err(|_| AppError::new("BACKUP_ERROR", "Não foi possível publicar o backup completo."))?;
        Ok(BackupResult {
            project_id, backup_id, blob_count: manifest.blobs.len(), database_bytes,
            backup_path: final_dir.to_string_lossy().into_owned(),
        })
    })();

    if result.is_err() { let _ = fs::remove_dir_all(&staging); }
    result
}
