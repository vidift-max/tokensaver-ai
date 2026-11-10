use tauri::Manager;
use crate::{db, error::{AppError, AppResult}, limits};
use serde::Serialize;
use std::{fs, io::Read, path::PathBuf};

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
    let lower=text.to_lowercase();
    let pos=lower.find(needle);
    let chars:Vec<char>=text.chars().collect();
    let center=pos.map(|p| lower[..p].chars().count()).unwrap_or(0);
    let start=center.saturating_sub(100);
    let end=(center+260).min(chars.len());
    let mut out: String=chars[start..end].iter().collect();
    out=out.replace('\n', " ").replace('\r', " ");
    if start>0 { out=format!("…{out}"); } if end<chars.len(){out.push('…');} out
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
    let mut files_examined=0usize;
    let mut bytes_read=0u64;
    for row in rows { if files_examined >= limits::MAX_FILES_PER_BATCH || bytes_read >= limits::MAX_BATCH_BYTES { break; } let (id,path,version,bytes,sha)=row.map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler o contexto."))?; files_examined += 1; if sha.len()!=64 || !sha.bytes().all(|b|b.is_ascii_hexdigit()){continue;} let blob=blobs.join(&sha); let canonical=match blob.canonicalize(){Ok(p)=>p,Err(_)=>continue}; if canonical.parent()!=Some(blobs.as_path()){continue;} let mut raw=Vec::new(); let mut file=match fs::File::open(&canonical){Ok(f)=>f,Err(_)=>continue}; if std::io::Read::take(&mut file, limits::MAX_FILE_BYTES.saturating_add(1)).read_to_end(&mut raw).is_err(){continue;} if raw.len() as u64 > limits::MAX_FILE_BYTES {continue;} let next_total=bytes_read.saturating_add(raw.len() as u64); if next_total > limits::MAX_BATCH_BYTES {break;} bytes_read=next_total; let text=match String::from_utf8(raw){Ok(t)=>t,Err(_)=>continue}; let lower=text.to_lowercase(); let path_lower=path.to_lowercase(); let mut score=0i64; let mut first="".to_string(); for t in &terms { let pc=path_lower.matches(t).count() as i64; let cc=lower.matches(t).count() as i64; score += pc*12 + cc.min(20); if first.is_empty() && cc>0 {first=excerpt(&text,t);} } if score>0 {hits.push(ContextHit{file_id:id,logical_path:path,version_no:version,byte_len:bytes,token_estimate:((text.chars().count() as f64)/4.0).ceil() as i64,score,excerpt:if first.is_empty(){text.chars().take(360).collect()}else{first}});} }
    hits.sort_by(|a,b| b.score.cmp(&a.score).then_with(||a.logical_path.cmp(&b.logical_path))); hits.truncate(limit); Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn excerpt_handles_unicode_case_expansion_without_panicking() {
        let text = "prefix İSTO contexto final";
        let result = excerpt(text, "isto");
        assert!(result.contains("İSTO") || result.contains("contexto"));
    }
}
