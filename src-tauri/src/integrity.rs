use crate::{db, error::{AppError, AppResult}};
use serde::Serialize;
use sha2::{Digest, Sha256};
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
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível verificar um blob."))?;
        if read == 0 { break; }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()) == expected_sha)
}

#[tauri::command]
pub fn verify_project(app: tauri::AppHandle, project_id: String) -> AppResult<IntegrityReport> {
    let root = project_root(&app, &project_id)?;
    let conn = db::open(&root.join("db.sqlite"))?;
    let mut stmt = conn.prepare("SELECT sha256, byte_len, relative_path FROM blobs")
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível consultar os blobs."))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível consultar os blobs."))?;
    let canonical_root = root.canonicalize().map_err(|_| AppError::new("STORAGE_ERROR", "Diretório do projeto indisponível."))?;
    let canonical_blobs = canonical_root.join("blobs").canonicalize().ok();
    let mut checked = 0usize;
    let mut missing = 0usize;
    let mut corrupt = 0usize;
    for row in rows {
        let (sha, len, relative) = row.map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível ler o índice de blobs."))?;
        let path = safe_blob_path(&root, &relative)?;
        checked += 1;
        let canonical_path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => { missing += 1; continue; }
        };
        let confined = canonical_blobs.as_ref().is_some_and(|dir| dir.starts_with(&canonical_root) && canonical_path.parent() == Some(dir.as_path()) && canonical_path.starts_with(dir));
        if !confined { corrupt += 1; continue; }
        if !verify_file(&canonical_path, &sha, len)? { corrupt += 1; }
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_paths_are_confined_to_blobs_directory() {
        let root = Path::new("/project");
        assert!(safe_blob_path(root, "blobs/abc").is_ok());
        assert!(safe_blob_path(root, "../outside").is_err());
        assert!(safe_blob_path(root, "blobs/../outside").is_err());
        assert!(safe_blob_path(root, "/blobs/abc").is_err());
    }

    #[test]
    fn integrity_verification_detects_corruption_and_length_mismatch() {
        let dir = std::env::temp_dir().join(format!("tokensaver-guardian-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("blob");
        let original = b"guardian integrity payload";
        std::fs::write(&path, original).expect("write blob");
        let sha = format!("{:x}", Sha256::digest(original));
        assert!(verify_file(&path, &sha, original.len() as i64).expect("valid verification"));
        assert!(!verify_file(&path, &sha, (original.len() + 1) as i64).expect("length check"));
        std::fs::write(&path, b"guardian integrity changed").expect("mutate blob");
        assert!(!verify_file(&path, &sha, original.len() as i64).expect("corruption check"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
