mod commands;
mod db;
mod error;
mod hash;
mod limits;

use error::{AppError, AppResult};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, path::{Component, Path, PathBuf}};
use tauri::Manager;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct AppHealth {
    ok: bool,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ProjectInfo {
    id: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct TokenEstimate {
    method: &'static str,
    count: usize,
    approximate: bool,
    provider: Option<String>,
    model: Option<String>,
    limitation: Option<String>,
}

fn safe_project_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::new("INVALID_REQUEST", "project_id inválido."));
    }
    Ok(())
}

fn safe_logical_path(path: &str) -> AppResult<PathBuf> {
    if path.is_empty() || path.len() > 512 {
        return Err(AppError::new("INVALID_FILE", "Caminho lógico inválido."));
    }
    let p = Path::new(path);
    let mut out = PathBuf::new();
    let mut depth = 0usize;
    for c in p.components() {
        match c {
            Component::Normal(v) => { out.push(v); depth += 1; }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::new("PATH_NOT_ALLOWED", "Caminho lógico fora do projeto."));
            }
        }
    }
    if depth == 0 || depth > limits::MAX_PATH_DEPTH {
        return Err(AppError::new("PATH_NOT_ALLOWED", "Profundidade de caminho não permitida."));
    }
    Ok(out)
}

#[tauri::command]
fn health() -> AppResult<AppHealth> {
    Ok(AppHealth { ok: true, version: "0.1.0" })
}

#[tauri::command]
fn sha256_text(input: String) -> AppResult<String> {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

#[tauri::command]
fn estimate_tokens(input: String) -> AppResult<TokenEstimate> {
    let count = ((input.chars().count() as f64) / 4.0).ceil() as usize;
    Ok(TokenEstimate {
        method: "local_estimate",
        count,
        approximate: true,
        provider: None,
        model: None,
        limitation: Some("Estimativa local por caracteres; não substitui a contagem do provedor.".into()),
    })
}

#[tauri::command]
fn initialize_project(app: tauri::AppHandle, project_id: String) -> AppResult<ProjectInfo> {
    safe_project_id(&project_id)?;
    let root = app.path().app_data_dir()
        .map_err(|e| AppError::with_details("STORAGE_ERROR", "Não foi possível localizar o armazenamento.", serde_json::json!({"reason": e.to_string()})))?
        .join("storage").join("projects").join(&project_id);
    fs::create_dir_all(root.join("blobs"))
        .map_err(|e| AppError::new("STORAGE_ERROR", &e.to_string()))?;
    let db_path = root.join("db.sqlite");
    db::open(&db_path).map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    Ok(ProjectInfo { id: project_id, path: root.to_string_lossy().into_owned() })
}

#[tauri::command]
fn import_text_file(app: tauri::AppHandle, project_id: String, logical_path: String, source_path: String) -> AppResult<String> {
    safe_project_id(&project_id)?;
    let logical = safe_logical_path(&logical_path)?;
    let source = PathBuf::from(source_path);
    let meta = fs::symlink_metadata(&source).map_err(|_| AppError::new("FILE_NOT_FOUND", "Arquivo não encontrado."))?;
    if !meta.file_type().is_file() {
        return Err(AppError::new("INVALID_FILE", "O caminho informado não é um arquivo regular."));
    }
    if meta.len() > limits::MAX_FILE_BYTES {
        return Err(AppError::with_details("FILE_TOO_LARGE", "O arquivo excede o limite permitido.", serde_json::json!({"limit_bytes": limits::MAX_FILE_BYTES, "actual_bytes": meta.len()})));
    }
    let bytes = fs::read(&source).map_err(|e| AppError::new("INVALID_FILE", &e.to_string()))?;
    if std::str::from_utf8(&bytes).is_err() {
        return Err(AppError::new("ENCODING_INVALID", "O arquivo não é UTF-8 válido."));
    }

    let root = app.path().app_data_dir()
        .map_err(|e| AppError::new("STORAGE_ERROR", &e.to_string()))?
        .join("storage").join("projects").join(&project_id);
    fs::create_dir_all(root.join("blobs")).map_err(|e| AppError::new("STORAGE_ERROR", &e.to_string()))?;
    let db_path = root.join("db.sqlite");
    let conn = db::open(&db_path).map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;

    let sha = hash::sha256_bytes(&bytes);
    let blob_rel = format!("blobs/{}", sha);
    let blob_abs = root.join(&blob_rel);
    if !blob_abs.exists() {
        let tmp = root.join(format!(".blob-{}.tmp", Uuid::new_v4()));
        fs::write(&tmp, &bytes).map_err(|e| AppError::new("STORAGE_ERROR", &e.to_string()))?;
        fs::rename(&tmp, &blob_abs).map_err(|e| AppError::new("STORAGE_ERROR", &e.to_string()))?;
    }

    let now = format!("{:?}", std::time::SystemTime::now());
    conn.execute("INSERT OR IGNORE INTO projects(id,name,created_at) VALUES(?1,?2,?3)", rusqlite::params![project_id, project_id, now])
        .map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    conn.execute("INSERT OR IGNORE INTO blobs(sha256,byte_len,relative_path,created_at) VALUES(?1,?2,?3,?4)", rusqlite::params![sha, bytes.len() as i64, blob_rel, now])
        .map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    let file_id = Uuid::new_v4().to_string();
    conn.execute("INSERT OR IGNORE INTO files(id,project_id,logical_path,created_at) VALUES(?1,?2,?3,?4)", rusqlite::params![file_id, project_id, logical.to_string_lossy(), now])
        .map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    let actual_file_id: String = conn.query_row("SELECT id FROM files WHERE project_id=?1 AND logical_path=?2", rusqlite::params![project_id, logical.to_string_lossy()], |r| r.get(0))
        .map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    let last: i64 = conn.query_row("SELECT COALESCE(MAX(version_no),0) FROM file_versions WHERE file_id=?1", rusqlite::params![actual_file_id], |r| r.get(0))
        .map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    let same: Option<String> = conn.query_row("SELECT blob_sha256 FROM file_versions WHERE file_id=?1 AND version_no=?2", rusqlite::params![actual_file_id, last], |r| r.get(0)).ok();
    if same.as_deref() == Some(sha.as_str()) {
        return Ok(sha);
    }
    conn.execute("INSERT INTO file_versions(id,file_id,version_no,blob_sha256,byte_len,created_at) VALUES(?1,?2,?3,?4,?5,?6)", rusqlite::params![Uuid::new_v4().to_string(), actual_file_id, last + 1, sha, bytes.len() as i64, now])
        .map_err(|e| AppError::new("DATABASE_ERROR", &e.to_string()))?;
    Ok(sha)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![health, sha256_text, estimate_tokens, initialize_project, import_text_file, file_center::list_project_files, file_center::read_file_version])
        .setup(|app| {
            let _ = app.path();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running TokenSaver AI");
}
