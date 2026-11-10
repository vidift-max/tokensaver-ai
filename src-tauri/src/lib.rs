mod backup;
mod commands;
mod db;
mod error;
mod file_center;
mod hash;
mod integrity;
mod smart_context;
mod safe_optimizer;
mod project_memory;
mod history;
mod limits;
mod path_guard;

use error::{AppError, AppResult};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs::{self, File}, io::Read, path::{Component, Path, PathBuf}};
use tauri::Manager;
use uuid::Uuid;

#[derive(Debug, Serialize)] struct AppHealth { ok: bool, version: &'static str }
#[derive(Debug, Serialize)] struct ProjectInfo { id: String, path: String }
#[derive(Debug, Serialize)] struct TokenEstimate { method: &'static str, count: usize, approximate: bool, provider: Option<String>, model: Option<String>, limitation: Option<String> }

fn safe_project_id(id:&str)->AppResult<()> { if id.is_empty()||id.len()>64||!id.chars().all(|c|c.is_ascii_alphanumeric()||c=='-'||c=='_'){return Err(AppError::new("INVALID_REQUEST","project_id inválido."));} Ok(()) }
fn safe_logical_path(path:&str)->AppResult<PathBuf>{if path.is_empty()||path.len()>512{return Err(AppError::new("INVALID_FILE","Caminho lógico inválido."));} let p=Path::new(path); let mut out=PathBuf::new(); let mut depth=0usize; for c in p.components(){match c{Component::Normal(v)=>{out.push(v);depth+=1;} Component::CurDir=>{} Component::ParentDir|Component::RootDir|Component::Prefix(_)=>return Err(AppError::new("PATH_NOT_ALLOWED","Caminho lógico fora do projeto.")),}} if depth==0||depth>limits::MAX_PATH_DEPTH{return Err(AppError::new("PATH_NOT_ALLOWED","Profundidade de caminho não permitida."));} Ok(out)}
fn project_root(app:&tauri::AppHandle,project_id:&str)->AppResult<PathBuf>{safe_project_id(project_id)?; app.path().app_data_dir().map(|p|p.join("storage").join("projects").join(project_id)).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível localizar o armazenamento."))}
fn timestamp()->String{format!("{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())}
fn ensure_blob_with_status(root:&Path,sha:&str,bytes:&[u8])->AppResult<(String,bool)>{
    let blob_rel=format!("blobs/{sha}");
    let blob_abs=root.join(&blob_rel);
    if blob_abs.exists(){
        let meta=fs::metadata(&blob_abs).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob."))?;
        if !meta.is_file()||meta.len()!=bytes.len() as u64{return Err(AppError::new("STORAGE_ERROR","Blob existente está inconsistente."));}
        let mut f=File::open(&blob_abs).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob."))?;
        let mut hasher=Sha256::new();
        let mut buffer=[0u8;64*1024];
        loop {
            let n=f.read(&mut buffer).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob."))?;
            if n==0 { break; }
            hasher.update(&buffer[..n]);
        }
        if format!("{:x}",hasher.finalize())!=sha{return Err(AppError::new("STORAGE_ERROR","Blob existente está corrompido."));}
        return Ok((blob_rel,false));
    }
    let tmp=root.join(format!(".blob-{}.tmp",Uuid::new_v4()));
    fs::write(&tmp,bytes).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível gravar o blob temporário."))?;
    match fs::rename(&tmp,&blob_abs){
        Ok(())=>Ok((blob_rel,true)),
        Err(_)=>{
            let _=fs::remove_file(&tmp);
            if blob_abs.exists(){
                let meta=fs::metadata(&blob_abs).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob."))?;
                if !meta.is_file()||meta.len()!=bytes.len() as u64{return Err(AppError::new("STORAGE_ERROR","Blob existente está inconsistente."));}
                let mut f=File::open(&blob_abs).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob."))?;
                let mut hasher=Sha256::new();
                let mut buffer=[0u8;64*1024];
                loop {
                    let n=f.read(&mut buffer).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o blob."))?;
                    if n==0 { break; }
                    hasher.update(&buffer[..n]);
                }
                if format!("{:x}",hasher.finalize())!=sha{return Err(AppError::new("STORAGE_ERROR","Blob existente está corrompido."));}
                Ok((blob_rel,false))
            }else{
                Err(AppError::new("STORAGE_ERROR","Não foi possível finalizar o armazenamento do blob."))
            }
        }
    }
}

fn ensure_blob(root:&Path,sha:&str,bytes:&[u8])->AppResult<String>{
    ensure_blob_with_status(root,sha,bytes).map(|(path,_)|path)
}
fn allowed_extension(path:&str)->bool{matches!(Path::new(path).extension().and_then(|e|e.to_str()).map(|s|s.to_ascii_lowercase()).as_deref(),Some("txt")|Some("md")|Some("json")|Some("html")|Some("css"))}
fn read_source_safely(app:&tauri::AppHandle, source:&Path)->AppResult<Vec<u8>>{let f=path_guard::open_verified(app, source)?;let meta=f.metadata().map_err(|_|AppError::new("INVALID_FILE","Não foi possível verificar o arquivo."))?;if !meta.is_file(){return Err(AppError::new("INVALID_FILE","O caminho informado não é um arquivo regular."));}let mut bytes=Vec::new();f.take(limits::MAX_FILE_BYTES+1).read_to_end(&mut bytes).map_err(|_|AppError::new("INVALID_FILE","Não foi possível ler o arquivo informado."))?;if bytes.len() as u64>limits::MAX_FILE_BYTES{return Err(AppError::with_details("FILE_TOO_LARGE","O arquivo excede o limite permitido.",serde_json::json!({"limit_bytes":limits::MAX_FILE_BYTES,"actual_bytes":bytes.len()})));}if std::str::from_utf8(&bytes).is_err(){return Err(AppError::new("ENCODING_INVALID","O arquivo não é UTF-8 válido."));}Ok(bytes)}
#[tauri::command] fn health()->AppResult<AppHealth>{Ok(AppHealth{ok:true,version:"0.1.0"})}
#[tauri::command] fn sha256_text(input:String)->AppResult<String>{let mut h=Sha256::new();h.update(input.as_bytes());Ok(format!("{:x}",h.finalize()))}
#[tauri::command] fn estimate_tokens(input:String)->AppResult<TokenEstimate>{let count=((input.chars().count() as f64)/4.0).ceil() as usize;Ok(TokenEstimate{method:"local_estimate",count,approximate:true,provider:None,model:None,limitation:Some("Estimativa local por caracteres; não substitui a contagem do provedor.".into())})}
#[tauri::command] fn initialize_project(app:tauri::AppHandle,project_id:String)->AppResult<ProjectInfo>{let root=project_root(&app,&project_id)?;fs::create_dir_all(root.join("blobs")).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível gravar no armazenamento."))?;let db_path=root.join("db.sqlite");db::open(&db_path)?;Ok(ProjectInfo{id:project_id,path:root.to_string_lossy().into_owned()})}
#[tauri::command]
fn import_text_file(app:tauri::AppHandle,project_id:String,logical_path:String,source_path:String)->AppResult<String>{
    let logical=safe_logical_path(&logical_path)?;
    if !allowed_extension(&logical_path){return Err(AppError::new("FILE_TYPE_NOT_SUPPORTED","Tipo de arquivo não suportado pela Foundation."));}
    let source=PathBuf::from(source_path);
    let bytes=read_source_safely(&app,&source)?;
    let root=project_root(&app,&project_id)?;
    fs::create_dir_all(root.join("blobs")).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível acessar o armazenamento."))?;
    let mut conn=db::open(&root.join("db.sqlite"))?;
    let sha=hash::sha256_bytes(&bytes);
    let (blob_rel,blob_created)=ensure_blob_with_status(&root,&sha,&bytes)?;
    let result=(||->AppResult<String>{
        let now=timestamp();
        let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível iniciar a transação."))?;
        tx.execute("INSERT OR IGNORE INTO projects(id,name,created_at) VALUES(?1,?2,?3)",rusqlite::params![project_id,project_id,now]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o projeto."))?;
        tx.execute("INSERT OR IGNORE INTO blobs(sha256,byte_len,relative_path,created_at) VALUES(?1,?2,?3,?4)",rusqlite::params![sha,bytes.len() as i64,blob_rel,now]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o blob."))?;
        let file_id=Uuid::new_v4().to_string();
        tx.execute("INSERT OR IGNORE INTO files(id,project_id,logical_path,created_at) VALUES(?1,?2,?3,?4)",rusqlite::params![file_id,project_id,logical.to_string_lossy(),now]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o arquivo."))?;
        let actual_file_id:String=tx.query_row("SELECT id FROM files WHERE project_id=?1 AND logical_path=?2",rusqlite::params![project_id,logical.to_string_lossy()],|r|r.get(0)).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível localizar o arquivo registrado."))?;
        let last:i64=tx.query_row("SELECT COALESCE(MAX(version_no),0) FROM file_versions WHERE file_id=?1",rusqlite::params![actual_file_id],|r|r.get(0)).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar as versões do arquivo."))?;
        if last>0{
            let same:Option<String>=tx.query_row("SELECT blob_sha256 FROM file_versions WHERE file_id=?1 AND version_no=?2",rusqlite::params![actual_file_id,last],|r|r.get(0)).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível verificar a versão atual."))?;
            if same.as_deref()==Some(sha.as_str()){
                tx.commit().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível confirmar a importação."))?;
                return Ok(sha.clone());
            }
        }
        let next=last+1;
        let version_id=Uuid::new_v4().to_string();
        tx.execute("INSERT INTO file_versions(id,file_id,version_no,blob_sha256,byte_len,created_at) VALUES(?1,?2,?3,?4,?5,?6)",rusqlite::params![version_id,actual_file_id,next,sha,bytes.len() as i64,now]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar a nova versão."))?;
        let token_count=((String::from_utf8_lossy(&bytes).chars().count() as f64)/4.0).ceil() as i64;
        tx.execute("INSERT INTO token_counts(id,file_version_id,method,count,approximate,provider,model,limitation,created_at) VALUES(?1,?2,'local_estimate',?3,1,NULL,NULL,?4,?5)",rusqlite::params![Uuid::new_v4().to_string(),version_id,token_count,"Estimativa local por caracteres; não substitui a contagem do provedor.",now]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar a contagem de tokens."))?;
        tx.commit().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível confirmar a importação."))?;
        Ok(sha.clone())
    })();
    if result.is_err() && blob_created{
        let blob_abs=root.join(&blob_rel);
        let referenced=db::open(&root.join("db.sqlite")).ok().and_then(|c|{
            c.query_row("SELECT 1 FROM blobs WHERE sha256=?1 LIMIT 1",rusqlite::params![sha],|r|r.get::<_,i64>(0)).ok()
        }).is_some();
        if !referenced { let _=fs::remove_file(blob_abs); }
    }
    result
}
#[tauri::command]
fn add_import_root(app:tauri::AppHandle,path:String)->AppResult<Vec<path_guard::ImportRoot>>{path_guard::add_root(&app,path)}
#[tauri::command]
fn remove_import_root(app:tauri::AppHandle,path:String)->AppResult<Vec<path_guard::ImportRoot>>{path_guard::remove_root(&app,path)}
#[tauri::command]
fn list_import_roots(app:tauri::AppHandle)->AppResult<Vec<path_guard::ImportRoot>>{path_guard::list_roots(&app)}
#[cfg(test)] mod tests;
#[cfg_attr(mobile,tauri::mobile_entry_point)] pub fn run(){tauri::Builder::default().invoke_handler(tauri::generate_handler![health,sha256_text,estimate_tokens,initialize_project,import_text_file,file_center::list_project_files,file_center::read_file_version,integrity::verify_project,backup::create_project_backup,backup::restore_project_backup,smart_context::smart_context,safe_optimizer::optimize_text,project_memory::{list_project_memory,set_project_memory},history::list_operations,add_import_root,remove_import_root,list_import_roots]).setup(|app|{let _=app.path();Ok(())}).run(tauri::generate_context!()).expect("error while running TokenSaver AI");}
