use crate::{db, error::{AppError, AppResult}};
use rusqlite::{backup::Backup, Connection};
use serde::{Deserialize, Serialize};
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
#[derive(Deserialize)]\nstruct BlobManifest {
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
    let canonical_storage = storage.canonicalize().map_err(|_| AppError::new("STORAGE_ERROR", "Armazenamento indisponível."))?;
    let canonical_backups = storage.join("backups").canonicalize().map_err(|_| AppError::new("BACKUP_ERROR", "Pasta de backups indisponível."))?;
    let canonical_backup_parent = backup_parent.canonicalize().map_err(|_| AppError::new("BACKUP_ERROR", "Pasta do projeto de backup indisponível."))?;
    if !canonical_backups.starts_with(&canonical_storage) || canonical_backup_parent.parent() != Some(canonical_backups.as_path()) || canonical_backup_parent.file_name().and_then(|v| v.to_str()) != Some(project_id.as_str()) {
        return Err(AppError::new("BACKUP_ERROR", "Pasta de backup fora do armazenamento autorizado."));
    }
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
    let manifest: BackupManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(|_| AppError::new("BACKUP_INVALID", "Não foi possível ler o manifesto."))?
    ).map_err(|_| AppError::new("BACKUP_INVALID", "Manifesto do backup inválido."))?;
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
    let (db_hash, db_bytes) = hash_file(&backup_db)?;
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
        if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit())
            || len < 0 || relative != format!("blobs/{}", sha) {
            return Err(AppError::new("BACKUP_INVALID", "Metadados de blob inválidos no banco."));
        }
        db_blobs.insert(sha.to_ascii_lowercase(), len as u64);
    }
    drop(stmt);
    drop(snapshot);

    let mut manifest_blobs = std::collections::BTreeMap::new();
    for blob in &manifest.blobs {
        if blob.sha256.len() != 64 || !blob.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || blob.file_name != blob.sha256.to_ascii_lowercase()
            || manifest_blobs.insert(blob.sha256.to_ascii_lowercase(), blob.byte_len).is_some() {
            return Err(AppError::new("BACKUP_INVALID", "Manifesto contém blob inválido ou duplicado."));
        }
        let path = canonical_backup.join("blobs").join(&blob.file_name);
        let meta = fs::symlink_metadata(&path)
            .map_err(|_| AppError::new("BACKUP_CORRUPT", "Um blob do backup está ausente."))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(AppError::new("BACKUP_INVALID", "Um blob do backup não é um arquivo regular."));
        }
        let (hash, len) = hash_file(&path)?;
        if hash != blob.sha256 || len != blob.byte_len {
            return Err(AppError::new("BACKUP_CORRUPT", "Um blob do backup falhou na validação SHA-256."));
        }
    }
    if manifest_blobs != db_blobs {
        return Err(AppError::new("BACKUP_CORRUPT", "Manifesto e banco possuem listas de blobs diferentes."));
    }

    let projects_root = storage.join("projects");
    fs::create_dir_all(&projects_root)
        .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível preparar a pasta de projetos."))?;
    let target = projects_root.join(&target_project_id);
    if target.exists() {
        return Err(AppError::new("RESTORE_TARGET_EXISTS", "Já existe um projeto com esse identificador. Nada foi alterado."));
    }
    let stage = projects_root.join(format!(".restore-{}.tmp", Uuid::new_v4()));
    fs::create_dir(&stage)
        .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível criar a área temporária da restauração."))?;

    let restored = (|| -> AppResult<()> {
        fs::create_dir(stage.join("blobs"))
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível preparar os blobs restaurados."))?;
        for blob in &manifest.blobs {
            fs::copy(canonical_backup.join("blobs").join(&blob.file_name), stage.join("blobs").join(&blob.file_name))
                .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível restaurar um blob."))?;
        }
        fs::copy(&backup_db, stage.join("db.sqlite"))
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível restaurar o banco."))?;
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
        tx.execute("UPDATE files SET project_id=?1 WHERE project_id=?2",
            rusqlite::params![target_project_id, source_project_id])
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível associar os arquivos ao novo projeto."))?;
        tx.execute("DELETE FROM projects WHERE id=?1", [&source_project_id])
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível finalizar a identificação do projeto restaurado."))?;
        tx.commit().map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível confirmar a restauração."))?;
        drop(restored_db);
        fs::rename(&stage, &target)
            .map_err(|_| AppError::new("RESTORE_ERROR", "Não foi possível publicar o projeto restaurado."))?;
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
