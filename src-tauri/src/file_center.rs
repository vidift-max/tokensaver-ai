use crate::{db, error::{AppError, AppResult}, limits};
use rusqlite::params;
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Serialize)]
pub struct FileSummary {
    pub id: String,
    pub logical_path: String,
    pub latest_version: i64,
    pub byte_len: i64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct FileChunk {
    pub file_id: String,
    pub version_no: i64,
    pub sha256: String,
    pub byte_len: i64,
    pub offset: usize,
    pub data: String,
    pub eof: bool,
}

fn project_root(app: &tauri::AppHandle, project_id: &str) -> AppResult<PathBuf> {
    if project_id.is_empty() || project_id.len() > 64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::new("INVALID_REQUEST", "project_id inválido."));
    }
    app.path().app_data_dir()
        .map(|p| p.join("storage").join("projects").join(project_id))
        .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível localizar o armazenamento."))
}

#[tauri::command]
pub fn list_project_files(app: tauri::AppHandle, project_id: String, offset: usize, limit: usize) -> AppResult<Page<FileSummary>> {
    let limit = limit.clamp(1, 100);
    let root = project_root(&app, &project_id)?;
    let conn = db::open(&root.join("db.sqlite")).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível abrir o banco do projeto."))?;
    let mut stmt = conn.prepare("SELECT f.id,f.logical_path,COALESCE(v.version_no,0),COALESCE(v.byte_len,0),COALESCE(v.blob_sha256,'') FROM files f LEFT JOIN file_versions v ON v.file_id=f.id AND v.version_no=(SELECT MAX(v2.version_no) FROM file_versions v2 WHERE v2.file_id=f.id) WHERE f.project_id=?1 ORDER BY f.logical_path LIMIT ?2 OFFSET ?3")
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível consultar os arquivos."))?;
    let rows = stmt.query_map(params![project_id, limit as i64 + 1, offset as i64], |r| Ok(FileSummary {
        id:r.get(0)?, logical_path:r.get(1)?, latest_version:r.get(2)?, byte_len:r.get(3)?, sha256:r.get(4)?
    })).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível consultar os arquivos."))?;
    let mut items=Vec::new();
    for row in rows.take(limit+1) { items.push(row.map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível ler o índice de arquivos."))?); }
    let has_more=items.len()>limit; if has_more { items.truncate(limit); }
    Ok(Page{items,offset,limit,has_more})
}

#[tauri::command]
pub fn read_file_version(app: tauri::AppHandle, project_id: String, file_id: String, version_no: i64, offset: usize, limit: usize) -> AppResult<FileChunk> {
    if version_no <= 0 { return Err(AppError::new("INVALID_REQUEST", "version_no inválido.")); }
    let limit=limit.clamp(1, limits::MAX_FILE_BYTES as usize);
    let root=project_root(&app,&project_id)?;
    let conn=db::open(&root.join("db.sqlite")).map_err(|_| AppError::new("DATABASE_ERROR","Não foi possível abrir o banco do projeto."))?;
    let (sha, byte_len):(String,i64)=conn.query_row("SELECT v.blob_sha256,v.byte_len FROM file_versions v JOIN files f ON f.id=v.file_id WHERE v.file_id=?1 AND v.version_no=?2 AND f.project_id=?3",params![file_id,version_no,project_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|AppError::new("FILE_NOT_FOUND","Versão do arquivo não encontrada."))?;
    if offset >= byte_len.max(0) as usize { return Ok(FileChunk{file_id,version_no,sha256:sha,byte_len,offset,data:String::new(),eof:true}); }
    let blob=root.join("blobs").join(&sha);
    let bytes=fs::read(&blob).map_err(|_|AppError::new("STORAGE_ERROR","Blob do arquivo não está disponível."))?;
    if bytes.len() as i64 != byte_len { return Err(AppError::new("STORAGE_ERROR","Blob do arquivo está inconsistente.")); }
    let end=(offset+limit).min(bytes.len());
    let slice=&bytes[offset..end];
    let data=std::str::from_utf8(slice).map_err(|_|AppError::new("ENCODING_INVALID","O conteúdo armazenado não é UTF-8 válido."))?.to_owned();
    Ok(FileChunk{file_id,version_no,sha256,byte_len,offset,data,eof:end>=bytes.len()})
}
