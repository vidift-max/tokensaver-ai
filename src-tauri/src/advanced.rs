use crate::{db, error::{AppError, AppResult}};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use uuid::Uuid;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CostResult { pub input_tokens:u64, pub output_tokens:u64, pub input_cost:f64, pub output_cost:f64, pub total_cost:f64, pub currency:String }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Template { pub id:String, pub name:String, pub content:String, pub updated_at:String }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArticleRow { pub file_id:String, pub logical_path:String, pub version_no:i64, pub byte_len:i64, pub sha256:String }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinkCandidate { pub file_id:String, pub logical_path:String, pub score:i64 }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClaudeConfig { pub enabled:bool, pub endpoint:String, pub model:String }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EditorialResult { pub title:String, pub body:String, pub word_count:usize, pub valid:bool, pub warnings:Vec<String> }

fn project_root(app:&tauri::AppHandle,id:&str)->AppResult<PathBuf>{
    if id.is_empty() || id.len()>64 || !id.chars().all(|c| c.is_ascii_alphanumeric()||c=='-'||c=='_') { return Err(AppError::new("INVALID_REQUEST","project_id inválido.")); }
    Ok(app.path().app_data_dir().map_err(|_|AppError::new("STORAGE_ERROR","Armazenamento indisponível."))?.join("storage/projects").join(id))
}
fn now()->String{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs().to_string()}
fn valid_text(s:&str,max:usize)->AppResult<()> { if s.len()>max {return Err(AppError::with_details("INPUT_TOO_LARGE","Entrada excede o limite.",serde_json::json!({"max_bytes":max})));} Ok(()) }

#[tauri::command]
pub fn simulate_cost(input_tokens:u64,output_tokens:u64,input_price_per_million:f64,output_price_per_million:f64)->AppResult<CostResult>{
    if input_tokens>100_000_000 || output_tokens>100_000_000 || !input_price_per_million.is_finite() || !output_price_per_million.is_finite() || input_price_per_million<0.0 || output_price_per_million<0.0 { return Err(AppError::new("INVALID_REQUEST","Valores de custo inválidos.")); }
    let ic=input_tokens as f64*input_price_per_million/1_000_000.0; let oc=output_tokens as f64*output_price_per_million/1_000_000.0;
    Ok(CostResult{input_tokens,output_tokens,input_cost:ic,output_cost:oc,total_cost:ic+oc,currency:"USD".into()})
}

#[tauri::command]
pub fn list_templates(app:tauri::AppHandle,project_id:String)->AppResult<Vec<Template>>{
    let root=project_root(&app,&project_id)?; let c=db::open(&root.join("db.sqlite"))?; let mut st=c.prepare("SELECT id,name,content,updated_at FROM templates ORDER BY name").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível listar templates."))?;
    let rows=st.query_map([],|r|Ok(Template{id:r.get(0)?,name:r.get(1)?,content:r.get(2)?,updated_at:r.get(3)?})).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível listar templates."))?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler um registro."))
}
#[tauri::command]
pub fn save_template(app:tauri::AppHandle,project_id:String,id:Option<String>,name:String,content:String)->AppResult<Template>{
    valid_text(&name,128)?; valid_text(&content,200_000)?; if name.trim().is_empty(){return Err(AppError::new("INVALID_REQUEST","Nome do template obrigatório."));}
    let root=project_root(&app,&project_id)?; fs::create_dir_all(root.join("blobs")).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível preparar o projeto."))?; let c=db::open(&root.join("db.sqlite"))?;
    let id=id.filter(|x|!x.is_empty()).unwrap_or_else(||Uuid::new_v4().to_string()); let t=now();
    c.execute("INSERT INTO templates(id,name,content,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,content=excluded.content,updated_at=excluded.updated_at",params![id,name.trim(),content,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível salvar o template."))?;
    Ok(Template{id,name:name.trim().into(),content,updated_at:t})
}
#[tauri::command]
pub fn delete_template(app:tauri::AppHandle,project_id:String,id:String)->AppResult<()>{
    let c=db::open(&project_root(&app,&project_id)?.join("db.sqlite"))?; c.execute("DELETE FROM templates WHERE id=?1",params![id]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível excluir o template."))?; Ok(())
}

#[tauri::command]
pub fn editorial_validate(title:String,body:String)->AppResult<EditorialResult>{
    valid_text(&title,512)?; valid_text(&body,2_000_000)?; let mut w=Vec::new(); let tt=title.trim(); let bb=body.trim();
    if tt.is_empty(){w.push("Título vazio.".into())} if bb.is_empty(){w.push("Corpo vazio.".into())} if bb.len()<120{w.push("Conteúdo curto para uma página editorial.".into())}
    let words=bb.split_whitespace().count(); if words>20_000{w.push("Conteúdo muito longo; considere dividir em partes.".into())}
    Ok(EditorialResult{title:tt.into(),body:bb.into(),word_count:words,valid:w.is_empty(),warnings:w})
}

#[tauri::command]
pub fn list_articles(app:tauri::AppHandle,project_id:String,query:String)->AppResult<Vec<ArticleRow>>{
    valid_text(&query,512)?; let root=project_root(&app,&project_id)?; let c=db::open(&root.join("db.sqlite"))?;
    let escaped=query.trim().replace('\\','\\\\').replace('%','\\%').replace('_','\\_');
    let q=format!("%{}%",escaped);
    let mut st=c.prepare("SELECT f.id,f.logical_path,v.version_no,v.byte_len,v.blob_sha256 FROM files f JOIN file_versions v ON v.file_id=f.id WHERE v.version_no=(SELECT MAX(v2.version_no) FROM file_versions v2 WHERE v2.file_id=f.id) AND (?1='' OR f.logical_path LIKE ?1 ESCAPE '\\') ORDER BY f.logical_path").map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível listar artigos."))?;
    let rows=st.query_map(params![q],|r|Ok(ArticleRow{file_id:r.get(0)?,logical_path:r.get(1)?,version_no:r.get(2)?,byte_len:r.get(3)?,sha256:r.get(4)?})).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível listar artigos."))?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível ler um artigo."))
}

#[tauri::command]
pub fn find_internal_links(app:tauri::AppHandle,project_id:String,query:String,max_results:usize)->AppResult<Vec<LinkCandidate>>{
    valid_text(&query,512)?; let max=max_results.clamp(1,50); let q=query.to_lowercase(); let rows=list_articles(app,project_id,"".into())?;
    Ok(rows.into_iter().map(|a|{let p=a.logical_path.to_lowercase(); let mut score=0; for term in q.split_whitespace(){if p.contains(term){score+=10}} (a,score)}).filter(|(_,s)|*s>0).take(max).map(|(a,s)|LinkCandidate{file_id:a.file_id,logical_path:a.logical_path,score:s}).collect())
}

#[tauri::command]
pub fn build_page(title:String,body:String,template:String)->AppResult<String>{
    valid_text(&title,512)?; valid_text(&body,2_000_000)?; valid_text(&template,200_000)?;
    let escaped_title=title.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;");
    let safe_body=body.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
    let base=if template.trim().is_empty(){"<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{{title}}</title></head><body>{{body}}</body></html>".into()}else{template};
    Ok(base.replace("{{title}}",&escaped_title).replace("{{body}}",&safe_body))
}

#[tauri::command]
pub fn get_claude_config(app:tauri::AppHandle,project_id:String)->AppResult<ClaudeConfig>{
    let c=db::open(&project_root(&app,&project_id)?.join("db.sqlite"))?;
    let row=c.query_row("SELECT enabled,endpoint,model FROM claude_config WHERE id=1",[],|r|Ok(ClaudeConfig{enabled:r.get::<_,i64>(0)?!=0,endpoint:r.get(1)?,model:r.get(2)?}));
    match row {Ok(x)=>Ok(x),Err(_)=>Ok(ClaudeConfig{enabled:false,endpoint:String::new(),model:String::new()})}
}
fn validate_claude_endpoint(endpoint:&str)->AppResult<()> {
    if endpoint.is_empty() { return Ok(()); }
    let lower=endpoint.trim().to_ascii_lowercase();
    let local_ok=lower.starts_with("http://localhost/") || lower.starts_with("http://localhost:") || lower.starts_with("http://127.0.0.1/") || lower.starts_with("http://127.0.0.1:") || lower.starts_with("http://[::1]/") || lower.starts_with("http://[::1]:");
    if !(lower.starts_with("https://") || local_ok) || lower.contains('@') || lower.contains('\\') { return Err(AppError::new("INVALID_REQUEST","Endpoint deve usar HTTPS, ou localhost para desenvolvimento.")); }
    let rest=if let Some(v)=lower.strip_prefix("https://"){v}else if let Some(v)=lower.strip_prefix("http://"){v}else{""};
    let authority=rest.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.starts_with(':') { return Err(AppError::new("INVALID_REQUEST","Endpoint inválido.")); }
    Ok(())
}

#[tauri::command]
pub fn save_claude_config(app:tauri::AppHandle,project_id:String,enabled:bool,endpoint:String,model:String)->AppResult<ClaudeConfig>{
    valid_text(&endpoint,2048)?; valid_text(&model,256)?; validate_claude_endpoint(endpoint.trim())?;
    let c=db::open(&project_root(&app,&project_id)?.join("db.sqlite"))?; c.execute("INSERT INTO claude_config(id,enabled,endpoint,model,updated_at) VALUES(1,?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled,endpoint=excluded.endpoint,model=excluded.model,updated_at=excluded.updated_at",params![enabled as i64,endpoint.trim(),model.trim(),now()]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível salvar a configuração."))?;
    Ok(ClaudeConfig{enabled,endpoint:endpoint.trim().into(),model:model.trim().into()})
}

fn validate_logical_path(path: &str) -> AppResult<()> {
    if path.is_empty() || path.len() > 512 || path.contains('\0') || path.starts_with('/') || path.starts_with('\\') {
        return Err(AppError::new("PATH_NOT_ALLOWED", "Caminho lógico inválido."));
    }
    let mut components = 0usize;
    for part in path.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." {
            return Err(AppError::new("PATH_NOT_ALLOWED", "Caminho lógico inválido."));
        }
        components += 1;
        if components > 64 {
            return Err(AppError::new("PATH_NOT_ALLOWED", "Caminho lógico excede o limite de componentes."));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn oneshot_capture(app:tauri::AppHandle,project_id:String,logical_path:String,content:String)->AppResult<String>{
    valid_text(&content,2_000_000)?; validate_logical_path(&logical_path)?;
    let root=project_root(&app,&project_id)?; fs::create_dir_all(root.join("blobs")).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível preparar o armazenamento."))?;
    let bytes=content.as_bytes(); let mut h=Sha256::new(); h.update(bytes); let sha=format!("{:x}",h.finalize()); let blob_rel=format!("blobs/{sha}"); let blob=root.join(&blob_rel);
    let created_blob = if !blob.exists() {
        let tmp=root.join(format!(".oneshot-{}.tmp",Uuid::new_v4()));
        fs::write(&tmp,bytes).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível gravar o conteúdo."))?;
        match fs::rename(&tmp,&blob) { Ok(()) => true, Err(_) => { let _=fs::remove_file(&tmp); false } }
    } else { false };
    let meta=fs::metadata(&blob).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o conteúdo."))?;
    if !meta.is_file() || meta.len()!=bytes.len() as u64 { return Err(AppError::new("STORAGE_ERROR","Blob existente está inconsistente.")); }
    let actual=fs::read(&blob).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível verificar o conteúdo."))?;
    if actual.as_slice()!=bytes { return Err(AppError::new("STORAGE_ERROR","Blob existente está corrompido.")); }

    let cleanup_sha = sha.clone();
    let result = (|| -> AppResult<String> {
        let mut c=db::open(&root.join("db.sqlite"))?; let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível iniciar a operação."))?; let t=now();
        tx.execute("INSERT OR IGNORE INTO projects(id,name,created_at) VALUES(?1,?1,?2)",params![project_id,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o projeto."))?;
        tx.execute("INSERT OR IGNORE INTO blobs(sha256,byte_len,relative_path,created_at) VALUES(?1,?2,?3,?4)",params![sha,bytes.len() as i64,blob_rel,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o blob."))?;
        tx.execute("INSERT OR IGNORE INTO files(id,project_id,logical_path,created_at) VALUES(?1,?2,?3,?4)",params![Uuid::new_v4().to_string(),project_id,logical_path,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o arquivo."))?;
        let actual:String=tx.query_row("SELECT id FROM files WHERE project_id=?1 AND logical_path=?2",params![project_id,logical_path],|r|r.get(0)).map_err(|_|AppError::new("DATABASE_ERROR","Arquivo não encontrado após gravação."))?;
        let last:i64=tx.query_row("SELECT COALESCE(MAX(version_no),0) FROM file_versions WHERE file_id=?1",params![actual],|r|r.get(0)).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar versões."))?;
        let latest_sha:Option<String>=tx.query_row("SELECT blob_sha256 FROM file_versions WHERE file_id=?1 ORDER BY version_no DESC LIMIT 1",params![actual],|r|r.get(0)).optional().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível consultar a última versão."))?;
        if latest_sha.as_deref()==Some(sha.as_str()) {
            tx.execute("INSERT INTO operations(id,project_id,kind,status,progress,created_at,finished_at,error_code) VALUES(?1,?2,'oneshot_capture','completed',100,?3,?3,NULL)",params![Uuid::new_v4().to_string(),project_id,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o histórico da operação."))?;
            tx.commit().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível confirmar a captura."))?;
            return Ok(sha);
        }
        let vid=Uuid::new_v4().to_string(); tx.execute("INSERT INTO file_versions(id,file_id,version_no,blob_sha256,byte_len,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![vid,actual,last+1,sha,bytes.len() as i64,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível criar a versão."))?;
        tx.execute("INSERT INTO token_counts(id,file_version_id,method,count,approximate,provider,model,limitation,created_at) VALUES(?1,?2,'local_estimate',?3,1,NULL,NULL,'Estimativa local por caracteres.',?4)",params![Uuid::new_v4().to_string(),vid,((content.chars().count() as f64)/4.0).ceil() as i64,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar tokens."))?;
        tx.execute("INSERT INTO operations(id,project_id,kind,status,progress,created_at,finished_at,error_code) VALUES(?1,?2,'oneshot_capture','completed',100,?3,?3,NULL)",params![Uuid::new_v4().to_string(),project_id,t]).map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível registrar o histórico da operação."))?;
        tx.commit().map_err(|_|AppError::new("DATABASE_ERROR","Não foi possível confirmar a captura."))?; Ok(sha)
    })();

    if result.is_err() && created_blob {
        if let Ok(check) = db::open(&root.join("db.sqlite")) {
            let referenced:i64=check.query_row("SELECT COUNT(*) FROM blobs WHERE sha256=?1 OR EXISTS (SELECT 1 FROM file_versions WHERE blob_sha256=?1)",params![cleanup_sha],|r|r.get(0)).unwrap_or(1);
            if referenced==0 { let _=fs::remove_file(&blob); }
        } else { let _=fs::remove_file(&blob); }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_endpoint_validation_rejects_insecure_or_malformed_hosts() {
        assert!(validate_claude_endpoint("https://api.example.com/v1").is_ok());
        assert!(validate_claude_endpoint("http://localhost:8080/v1").is_ok());
        assert!(validate_claude_endpoint("http://127.0.0.1:8080/v1").is_ok());
        assert!(validate_claude_endpoint("http://localhost.evil/v1").is_err());
        assert!(validate_claude_endpoint("http://user@localhost:8080/v1").is_err());
        assert!(validate_claude_endpoint("https://").is_err());
        assert!(validate_claude_endpoint("http://example.com/v1").is_err());
    }

    #[test]
    fn logical_path_validation_allows_safe_names_and_rejects_traversal() {
        assert!(validate_logical_path("article..txt").is_ok());
        assert!(validate_logical_path("articles/post.html").is_ok());
        assert!(validate_logical_path("../outside.txt").is_err());
        assert!(validate_logical_path("articles/../outside.txt").is_err());
        assert!(validate_logical_path("articles//post.html").is_err());
        assert!(validate_logical_path("/absolute.txt").is_err());
        assert!(validate_logical_path(r"\\server\\share\\file.txt").is_err());
    }
}
