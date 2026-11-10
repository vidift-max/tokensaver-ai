use crate::{db,error::{AppError,AppResult}};
use serde::Serialize;
#[derive(Debug,Serialize)] pub struct OperationRow{pub id:String,pub kind:String,pub status:String,pub progress:i64,pub created_at:String,pub finished_at:Option<String>,pub error_code:Option<String>}
#[tauri::command]
pub fn list_operations(app:tauri::AppHandle, project_id:String, limit:i64)->AppResult<Vec<OperationRow>>{
 let root=app.path().app_data_dir().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento indisponível."))?.join("storage/projects").join(&project_id);
 let conn=db::open(&root.join("db.sqlite"))?; let lim=limit.clamp(1,100);
 let mut st=conn.prepare("SELECT id,kind,status,progress,created_at,finished_at,error_code FROM operations ORDER BY created_at DESC LIMIT ?1").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar o histórico."))?;
 let rows=st.query_map(rusqlite::params![lim],|r|Ok(OperationRow{id:r.get(0)?,kind:r.get(1)?,status:r.get(2)?,progress:r.get(3)?,created_at:r.get(4)?,finished_at:r.get(5)?,error_code:r.get(6)?})).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler o histórico."))?;
 Ok(rows.filter_map(Result::ok).collect())
}
