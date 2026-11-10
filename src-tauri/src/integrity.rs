use crate::{db, error::{AppError, AppResult}, hash};
use serde::Serialize;
use std::{fs::File, io::Read, path::{Component, Path, PathBuf}};
use tauri::Manager;

#[derive(Debug, Serialize)]
pub struct IntegrityReport {
    pub project_id: String,
    pub checked_blobs: usize,
    pub missing_blobs: usize,
    pub corrupt_blobs: usize,
    pub orphan_blobs: usize,
    pub broken_references: usize,
    pub healthy: bool,
}

fn project_root(app: &tauri::AppHandle, project_id: &str) -> AppResult<PathBuf> {
    if project_id.is_empty() || project_id.len() > 64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::new("INVALID_REQUEST", "project_id inválido."));
    }
    app.path().app_data_dir().map(|p| p.join("storage").join("projects").join(project_id))
        .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível localizar o armazenamento."))
}

fn safe_blob_path(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let p = Path::new(relative);
    let mut out = PathBuf::from(root);
    let mut count = 0usize;
    let mut first_is_blobs = false;
    for component in p.components() {
        match component {
            Component::Normal(v) => {
                if count == 0 { first_is_blobs = v.to_string_lossy() == "blobs"; }
                out.push(v);
                count += 1;
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                return Err(AppError::new("STORAGE_ERROR", "Referência de blob inválida."));
            }
        }
    }
    if count != 2 || !first_is_blobs {
        return Err(AppError::new("STORAGE_ERROR", "Referência de blob inválida."));
    }
    Ok(out)
}

fn verify_file(path: &Path, expected_sha: &str, expected_len: i64) -> AppResult<bool> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Ok(false),
    };
    let len = file.metadata().map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível verificar um blob."))?.len();
    if expected_len < 0 || len != expected_len as u64 { return Ok(false); }
    let mut bytes = Vec::with_capacity(len as usize);
    file.read_to_end(&mut bytes).map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível verificar um blob."))?;
    Ok(hash::sha256_bytes(&bytes) == expected_sha)
}

#[tauri::command]
pub fn verify_project(app: tauri::AppHandle, project_id: String) -> AppResult<IntegrityReport> {
    let root = project_root(&app, &project_id)?;
    let conn = db::open(&root.join("db.sqlite"))?;
    let mut stmt = conn.prepare("SELECT sha256, byte_len, relative_path FROM blobs")
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível consultar os blobs."))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível consultar os blobs."))?;
    let mut checked = 0usize;
    let mut missing = 0usize;
    let mut corrupt = 0usize;
    for row in rows {
        let (sha, len, relative) = row.map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível ler o índice de blobs."))?;
        let path = safe_blob_path(&root, &relative)?;
        checked += 1;
        if !path.exists() { missing += 1; continue; }
        if !verify_file(&path, &sha, len)? { corrupt += 1; }
    }
    let broken_references: usize = conn.query_row(
        "SELECT COUNT(*) FROM file_versions v LEFT JOIN blobs b ON b.sha256=v.blob_sha256 WHERE b.sha256 IS NULL",
        [], |r| r.get(0)
    ).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível verificar as referências de blobs."))?;
    let orphan_blobs: usize = conn.query_row(
        "SELECT COUNT(*) FROM blobs b LEFT JOIN file_versions v ON v.blob_sha256=b.sha256 WHERE v.id IS NULL",
        [], |r| r.get(0)
    ).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível verificar blobs órfãos."))?;
    Ok(IntegrityReport {
        project_id, checked_blobs: checked, missing_blobs: missing, corrupt_blobs: corrupt,
        orphan_blobs, broken_references,
        healthy: missing == 0 && corrupt == 0 && broken_references == 0,
    })
}
