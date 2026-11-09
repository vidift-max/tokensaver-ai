use crate::{db, error::{AppError, AppResult}, hash, limits};
use rusqlite::params;
use serde::Serialize;
use std::{fs::File, io::{Read, Seek, SeekFrom}, path::PathBuf};
use tauri::Manager;

#[derive(Debug, Serialize)]
pub struct FileSummary { pub id: String, pub logical_path: String, pub latest_version: i64, pub byte_len: i64, pub sha256: String }
#[derive(Debug, Serialize)]
pub struct Page<T> { pub items: Vec<T>, pub offset: usize, pub limit: usize, pub has_more: bool }
#[derive(Debug, Serialize)]
pub struct FileChunk { pub file_id: String, pub version_no: i64, pub sha256: String, pub byte_len: i64, pub offset: usize, pub next_offset: usize, pub data: String, pub eof: bool }

fn project_root(app: &tauri::AppHandle, project_id: &str) -> AppResult<PathBuf> {
    if project_id.is_empty() || project_id.len() > 64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { return Err(AppError::new("INVALID_REQUEST", "project_id inválido.")); }
    app.path().app_data_dir().map(|p| p.join("storage").join("projects").join(project_id)).map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível localizar o armazenamento."))
}

#[tauri::command]
pub fn list_project_files(app: tauri::AppHandle, project_id: String, offset: usize, limit: usize) -> AppResult<Page<FileSummary>> {
    let limit=limit.clamp(1,100); let root=project_root(&app,&project_id)?;
    let conn=db::open(&root.join("db.sqlite"))?;
    let mut stmt=conn.prepare("SELECT f.id,f.logical_path,COALESCE(v.version_no,0),COALESCE(v.byte_len,0),COALESCE(v.blob_sha256,'') FROM files f LEFT JOIN file_versions v ON v.file_id=f.id AND v.version_no=(SELECT MAX(v2.version_no) FROM file_versions v2 WHERE v2.file_id=f.id) WHERE f.project_id=?1 ORDER BY f.logical_path LIMIT ?2 OFFSET ?3").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar os arquivos."))?;
    let rows=stmt.query_map(params![project_id,limit as i64+1,offset as i64],|r|Ok(FileSummary{id:r.get(0)?,logical_path:r.get(1)?,latest_version:r.get(2)?,byte_len:r.get(3)?,sha256:r.get(4)?})).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar os arquivos."))?;
    let mut items=Vec::new(); for row in rows.take(limit+1){items.push(row.map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler o índice de arquivos."))?);} let has_more=items.len()>limit; if has_more{items.truncate(limit);} Ok(Page{items,offset,limit,has_more})
}

#[tauri::command]
pub fn read_file_version(app: tauri::AppHandle, project_id: String, file_id: String, version_no: i64, offset: usize, limit: usize) -> AppResult<FileChunk> {
    if version_no<=0{return Err(AppError::new("INVALID_REQUEST","version_no inválido."));}
    let limit=limit.clamp(1,limits::MAX_FILE_BYTES as usize); let root=project_root(&app,&project_id)?;
    let conn=db::open(&root.join("db.sqlite")).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível abrir o banco do projeto."))?;
    let (sha,byte_len):(String,i64)=conn.query_row("SELECT v.blob_sha256,v.byte_len FROM file_versions v JOIN files f ON f.id=v.file_id WHERE v.file_id=?1 AND v.version_no=?2 AND f.project_id=?3",params![file_id,version_no,project_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|AppError::new("FILE_NOT_FOUND","Versão do arquivo não encontrada."))?;
    if byte_len<0{return Err(AppError::new("STORAGE_ERROR","Tamanho do arquivo está inconsistente."));} let total=byte_len as usize;
    let blob=root.join("blobs").join(&sha); let mut file=File::open(&blob).map_err(|_|AppError::new("STORAGE_ERROR","Blob do arquivo não está disponível."))?;
    let actual_len=file.metadata().map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob do arquivo."))?.len(); if actual_len!=total as u64{return Err(AppError::new("STORAGE_ERROR","Blob do arquivo está inconsistente."));}
    if offset>=total{return Ok(FileChunk{file_id,version_no,sha256:sha,byte_len,offset,next_offset:total,data:String::new(),eof:true});}
    file.seek(SeekFrom::Start(offset as u64)).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível posicionar a leitura."))?;
    let read_len=limit.min(total-offset); let mut buf=vec![0u8;read_len]; let mut filled=0usize;
    while filled<read_len{let n=file.read(&mut buf[filled..]).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível ler o blob do arquivo."))?; if n==0{break;} filled+=n;}
    buf.truncate(filled);
    while !std::str::from_utf8(&buf).is_ok() && !buf.is_empty(){buf.pop();}
    let data=std::str::from_utf8(&buf).map_err(|_|AppError::new("ENCODING_INVALID","O conteúdo armazenado não é UTF-8 válido."))?.to_owned();
    let next_offset=offset+buf.len(); Ok(FileChunk{file_id,version_no,sha256:sha,byte_len,offset,next_offset,data,eof:next_offset>=total})
}
