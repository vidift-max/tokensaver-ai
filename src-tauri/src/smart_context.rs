use crate::{db, error::{AppError, AppResult}};
use serde::Serialize;
use std::{fs, path::{Path, PathBuf}};

#[derive(Debug, Serialize)]
pub struct ContextHit {
    pub file_id: String,
    pub logical_path: String,
    pub version_no: i64,
    pub byte_len: i64,
    pub token_estimate: i64,
    pub score: i64,
    pub excerpt: String,
}

fn project_root(app: &tauri::AppHandle, project_id: &str) -> AppResult<PathBuf> {
    if project_id.is_empty() || project_id.len() > 64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric() || c=='-' || c=='_') { return Err(AppError::new("INVALID_REQUEST", "project_id inválido.")); }
    app.path().app_data_dir().map(|p| p.join("storage").join("projects").join(project_id)).map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível localizar o armazenamento."))
}
fn terms(query:&str)->Vec<String>{ query.split_whitespace().map(|s| s.trim_matches(|c:char| !c.is_alphanumeric() && c!='_' && c!='-').to_lowercase()).filter(|s|s.len()>=2).take(12).collect() }
fn excerpt(text:&str, needle:&str)->String {
    let lower=text.to_lowercase(); let pos=lower.find(needle).unwrap_or(0); let start=pos.saturating_sub(100); let end=(pos+260).min(text.len());
    let mut out=text.get(start..end).unwrap_or("").replace('\n', " ").replace('\r', " ");
    if start>0 { out=format!("…{out}"); } if end<text.len(){out.push('…');} out
}
#[tauri::command]
pub fn smart_context(app: tauri::AppHandle, project_id: String, query: String, max_files: usize) -> AppResult<Vec<ContextHit>> {
    let q=query.trim(); if q.len()>512 { return Err(AppError::new("INVALID_REQUEST", "A consulta é grande demais.")); }
    let terms=terms(q); if terms.is_empty(){ return Err(AppError::new("INVALID_REQUEST", "Informe pelo menos um termo de busca.")); }
    let limit=max_files.clamp(1,20); let root=project_root(&app,&project_id)?;
    let canonical_root=root.canonicalize().map_err(|_|AppError::new("STORAGE_ERROR","Projeto indisponível."))?;
    let blobs=canonical_root.join("blobs").canonicalize().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento de blobs indisponível."))?;
    if !blobs.starts_with(&canonical_root){return Err(AppError::new("STORAGE_ERROR","Armazenamento de blobs fora do projeto."));}
    let conn=db::open(&canonical_root.join("db.sqlite"))?;
    let mut stmt=conn.prepare("SELECT f.id,f.logical_path,v.version_no,v.byte_len,v.blob_sha256 FROM files f JOIN file_versions v ON v.file_id=f.id AND v.version_no=(SELECT MAX(v2.version_no) FROM file_versions v2 WHERE v2.file_id=f.id) WHERE f.project_id=?1 ORDER BY f.logical_path").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar os arquivos."))?;
    let rows=stmt.query_map(rusqlite::params![project_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?))).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar o contexto."))?;
    let mut hits=Vec::new();
    for row in rows { let (id,path,version,bytes,sha)=row.map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler o contexto."))?; if sha.len()!=64 || !sha.bytes().all(|b|b.is_ascii_hexdigit()){continue;} let blob=blobs.join(&sha); let canonical=match blob.canonicalize(){Ok(p)=>p,Err(_)=>continue}; if canonical.parent()!=Some(blobs.as_path()){continue;} let text=match fs::read_to_string(&canonical){Ok(t)=>t,Err(_)=>continue}; let lower=text.to_lowercase(); let path_lower=path.to_lowercase(); let mut score=0i64; let mut first="".to_string(); for t in &terms { let pc=path_lower.matches(t).count() as i64; let cc=lower.matches(t).count() as i64; score += pc*12 + cc.min(20); if first.is_empty() && cc>0 {first=excerpt(&text,t);} } if score>0 {hits.push(ContextHit{file_id:id,logical_path:path,version_no:version,byte_len:bytes,token_estimate:((text.chars().count() as f64)/4.0).ceil() as i64,score,excerpt:if first.is_empty(){text.chars().take(360).collect()}else{first}});} }
    hits.sort_by(|a,b| b.score.cmp(&a.score).then_with(||a.logical_path.cmp(&b.logical_path))); hits.truncate(limit); Ok(hits)
}
