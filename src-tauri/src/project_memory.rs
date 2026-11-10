use tauri::Manager;
use crate::{db, error::{AppError, AppResult}};
use serde::Serialize;

const MAX_PROJECT_MEMORY_ENTRIES: i64 = 1000;

#[derive(Debug, Serialize)]
pub struct MemoryEntry { pub key: String, pub value: String, pub updated_at: String }

fn valid_key(k:&str)->bool { !k.is_empty() && k.len()<=128 && k.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c,'_'|'-'|'.')) }

#[tauri::command]
pub fn list_project_memory(app: tauri::AppHandle, project_id: String) -> AppResult<Vec<MemoryEntry>> {
    if project_id.is_empty() || project_id.len()>64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric()||c=='-'||c=='_') { return Err(AppError::new("INVALID_REQUEST","project_id inválido.")); }
    let root=app.path().app_data_dir().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento indisponível."))?.join("storage/projects").join(&project_id);
    let conn=db::open(&root.join("db.sqlite"))?;
    let mut st=conn.prepare("SELECT key,value,updated_at FROM project_memory WHERE project_id=?1 ORDER BY key LIMIT ?2").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar a memória."))?;
    let rows=st.query_map(rusqlite::params![project_id, MAX_PROJECT_MEMORY_ENTRIES],|r|Ok(MemoryEntry{key:r.get(0)?,value:r.get(1)?,updated_at:r.get(2)?})).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler a memória."))?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler um registro da memória."))
}
#[tauri::command]
pub fn set_project_memory(app: tauri::AppHandle, project_id:String, key:String, value:String)->AppResult<MemoryEntry>{
    if project_id.is_empty() || project_id.len()>64 || !project_id.chars().all(|c| c.is_ascii_alphanumeric()||c=='-'||c=='_') { return Err(AppError::new("INVALID_REQUEST","project_id inválido.")); }
    if !valid_key(&key) { return Err(AppError::new("INVALID_REQUEST","Chave de memória inválida.")); }
    if value.len()>100_000 { return Err(AppError::new("INVALID_REQUEST","Valor de memória excede o limite.")); }
    let root=app.path().app_data_dir().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento indisponível."))?.join("storage/projects").join(&project_id);
    let mut conn = db::open(&root.join("db.sqlite"))?;
    set_project_memory_in_db(&mut conn, &project_id, &key, &value)
}

fn set_project_memory_in_db(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    key: &str,
    value: &str,
) -> AppResult<MemoryEntry> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível iniciar a operação de memória."))?;

    let exists: i64 = tx.query_row(
        "SELECT count(*) FROM project_memory WHERE project_id=?1 AND key=?2",
        rusqlite::params![project_id, key],
        |r| r.get(0),
    ).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível verificar a memória."))?;
    if exists == 0 {
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM project_memory WHERE project_id=?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        ).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível verificar o limite da memória."))?;
        if count >= MAX_PROJECT_MEMORY_ENTRIES {
            return Err(AppError::new("INVALID_REQUEST", "O projeto atingiu o limite de entradas de memória."));
        }
    }
    let now = format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs());
    tx.execute(
        "INSERT INTO project_memory(key,value,updated_at,project_id) VALUES(?1,?2,?3,?4) ON CONFLICT(project_id,key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        rusqlite::params![key, value, now, project_id],
    ).map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível salvar a memória."))?;
    tx.commit().map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível concluir a operação de memória."))?;
    Ok(MemoryEntry { key: key.to_string(), value: value.to_string(), updated_at: now })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    fn test_db_path() -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "tokensaver-memory-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        (dir.clone(), dir.join("db.sqlite"))
    }

    #[test]
    fn memory_cap_allows_update_at_capacity_and_is_project_scoped() {
        let (dir, path) = test_db_path();
        let mut conn = db::open(&path).unwrap();
        for index in 0..MAX_PROJECT_MEMORY_ENTRIES {
            conn.execute(
                "INSERT INTO project_memory(key,value,updated_at,project_id) VALUES(?1,'v','1','project-a')",
                rusqlite::params![format!("key-{index}")],
            ).unwrap();
        }

        let updated = set_project_memory_in_db(&mut conn, "project-a", "key-0", "updated").unwrap();
        assert_eq!(updated.value, "updated");
        assert!(set_project_memory_in_db(&mut conn, "project-a", "new-key", "v").is_err());
        assert!(set_project_memory_in_db(&mut conn, "project-b", "new-key", "v").is_ok());

        drop(conn);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_writers_cannot_exceed_project_memory_cap() {
        let (dir, path) = test_db_path();
        let mut seed = db::open(&path).unwrap();
        for index in 0..(MAX_PROJECT_MEMORY_ENTRIES - 1) {
            seed.execute(
                "INSERT INTO project_memory(key,value,updated_at,project_id) VALUES(?1,'v','1','project-a')",
                rusqlite::params![format!("key-{index}")],
            ).unwrap();
        }
        drop(seed);

        let conn_a = db::open(&path).unwrap();
        let conn_b = db::open(&path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let a = {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut conn = conn_a;
                barrier.wait();
                set_project_memory_in_db(&mut conn, "project-a", "parallel-a", "v").is_ok()
            })
        };
        let b = {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut conn = conn_b;
                barrier.wait();
                set_project_memory_in_db(&mut conn, "project-a", "parallel-b", "v").is_ok()
            })
        };
        barrier.wait();
        let successes = [a.join().unwrap(), b.join().unwrap()].iter().filter(|ok| **ok).count();
        assert_eq!(successes, 1);

        let verify = db::open(&path).unwrap();
        let count: i64 = verify.query_row(
            "SELECT count(*) FROM project_memory WHERE project_id='project-a'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, MAX_PROJECT_MEMORY_ENTRIES);
        drop(verify);
        let _ = fs::remove_dir_all(dir);
    }
}
