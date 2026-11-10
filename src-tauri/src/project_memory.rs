use tauri::Manager;
use crate::{db, error::{AppError, AppResult}};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct MemoryEntry { pub key: String, pub value: String, pub updated_at: String }

fn valid_key(k:&str)->bool { !k.is_empty() && k.len()<=128 && k.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c,'_'|'-'|'.')) }

#[tauri::command]
pub fn list_project_memory(app: tauri::AppHandle, project_id: String) -> AppResult<Vec<MemoryEntry>> {
    if project_id.is_empty() || project_id.len()>64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric()||c=='-'||c=='_') { return Err(AppError::new("INVALID_REQUEST","project_id inválido.")); }
    if project_id.is_empty() || project_id.len()>64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric()||c=='-'||c=='_') { return Err(AppError::new("INVALID_REQUEST","project_id inválido.")); }
    let root=app.path().app_data_dir().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento indisponível."))?.join("storage/projects").join(&project_id);
    let conn=db::open(&root.join("db.sqlite"))?;
    let mut st=conn.prepare("SELECT key,value,updated_at FROM project_memory WHERE project_id=?1 ORDER BY key").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar a memória."))?;
    let rows=st.query_map(rusqlite::params![project_id],|r|Ok(MemoryEntry{key:r.get(0)?,value:r.get(1)?,updated_at:r.get(2)?})).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler a memória."))?;
    Ok(rows.filter_map(Result::ok).collect())
}
#[tauri::command]
pub fn set_project_memory(app: tauri::AppHandle, project_id:String, key:String, value:String)->AppResult<MemoryEntry>{
    if !valid_key(&key) { return Err(AppError::new("INVALID_REQUEST","Chave de memória inválida.")); }
    if value.len()>100_000 { return Err(AppError::new("INVALID_REQUEST","Valor de memória excede o limite.")); }
    let root=app.path().app_data_dir().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento indisponível."))?.join("storage/projects").join(&project_id);
    let conn=db::open(&root.join("db.sqlite"))?; let now=format!("{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs());
    conn.execute("INSERT INTO project_memory(key,value,updated_at,project_id) VALUES(?1,?2,?3,?4) ON CONFLICT(project_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",rusqlite::params![key,value,now,project_id]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível salvar a memória."))?;
    Ok(MemoryEntry{key,value,updated_at:now})
}
