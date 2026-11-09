use crate::{error::{AppError, AppResult}, limits};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, path::{Component, Path, PathBuf}};
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRoot { pub path: String }

fn normalize_absolute(path: &Path) -> AppResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(AppError::new("PATH_NOT_ALLOWED", "Caminho de importação inválido."));
    }
    let absolute = if path.is_absolute() { path.to_path_buf() } else {
        return Err(AppError::new("PATH_NOT_ALLOWED", "Caminho de importação deve ser absoluto."));
    };
    #[cfg(windows)]
    {
        if let Some(Component::Prefix(prefix)) = absolute.components().next() {
            use std::path::Prefix;
            match prefix.kind() {
                Prefix::Unc(_, _) | Prefix::VerbatimUNC(_, _) | Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_) => {
                    return Err(AppError::new("PATH_NOT_ALLOWED", "Caminhos UNC ou namespaces de dispositivo não são permitidos."));
                }
                _ => {}
            }
        }
    }
    let mut out=PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix)=>out.push(prefix.as_os_str()),
            Component::RootDir=>out.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir=>{},
            Component::Normal(part)=>out.push(part),
            Component::ParentDir=>return Err(AppError::new("PATH_NOT_ALLOWED","Caminho de importação não pode conter '..'.")),
        }
    }
    Ok(out)
}

fn same_path(a:&Path,b:&Path)->bool {
    #[cfg(windows)]
    {
        fn normalize_windows(s:&str)->String {
            let mut v=s.replace('/',"\\");
            if let Some(rest)=v.strip_prefix(r"\\?\UNC\") { v=format!(r"\\{}",rest); }
            else if let Some(rest)=v.strip_prefix(r"\\?\") { v=rest.to_string(); }
            while v.len()>3 && v.ends_with('\\') { v.pop(); }
            v
        }
        normalize_windows(&a.to_string_lossy()).eq_ignore_ascii_case(&normalize_windows(&b.to_string_lossy()))
    }
    #[cfg(not(windows))]
    { a==b }
}

#[cfg(windows)]
fn component_eq(a:&Component<'_>,b:&Component<'_>)->bool {
    a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

#[cfg(not(windows))]
fn component_eq(a:&Component<'_>,b:&Component<'_>)->bool { a==b }

fn is_within(root:&Path,candidate:&Path)->bool {
    let mut r=root.components();
    let mut c=candidate.components();
    loop {
        match (r.next(),c.next()) {
            (None,_)=>true,
            (Some(rc),Some(cc))=>{ if !component_eq(&rc,&cc){return false;} true }
            (Some(_),None)=>false,
        }
    }
}

fn canonical_without_redirect(path:&Path)->AppResult<PathBuf> {
    let absolute=normalize_absolute(path)?;
    let canonical=fs::canonicalize(&absolute).map_err(|_|AppError::new("PATH_NOT_ALLOWED","Não foi possível validar o caminho de importação."))?;
    if !same_path(&absolute,&canonical) {
        return Err(AppError::new("PATH_NOT_ALLOWED","O caminho de importação atravessa um link, junction ou redirecionamento."));
    }
    Ok(canonical)
}

fn identity(file:&File)->AppResult<FileIdentity> {
    let metadata=file.metadata().map_err(|_|AppError::new("PATH_NOT_ALLOWED","Não foi possível verificar a identidade do arquivo."))?;
    if !metadata.is_file(){return Err(AppError::new("INVALID_FILE","O caminho informado não é um arquivo regular."));}
    #[cfg(unix)]
    { use std::os::unix::fs::MetadataExt; return Ok(FileIdentity::Unix{dev:metadata.dev(),ino:metadata.ino()}); }
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; let volume=metadata.volume_serial_number().ok_or_else(||AppError::new("PATH_NOT_ALLOWED","Não foi possível obter a identidade segura do arquivo."))?; let index=metadata.file_index().ok_or_else(||AppError::new("PATH_NOT_ALLOWED","Não foi possível obter a identidade segura do arquivo."))?; return Ok(FileIdentity::Windows{volume,index}); }
    #[cfg(not(any(unix,windows)))]
    { Err(AppError::new("PATH_NOT_ALLOWED","A plataforma não oferece identidade segura de arquivo.")) }
}

#[derive(Debug,PartialEq,Eq)]
enum FileIdentity {
    #[cfg(unix)] Unix{dev:u64,ino:u64},
    #[cfg(windows)] Windows{volume:u32,index:u64},
}

fn canonical_identity(path:&Path)->AppResult<FileIdentity> {
    let f=OpenOptions::new().read(true).open(path).map_err(|_|AppError::new("FILE_NOT_FOUND","Arquivo não encontrado."))?;
    identity(&f)
}

fn config_path(app:&tauri::AppHandle)->AppResult<PathBuf> {
    app.path().app_config_dir().map(|p|p.join("import_roots.json")).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível localizar as configurações do aplicativo."))
}

fn load_roots(app:&tauri::AppHandle)->AppResult<Vec<ImportRoot>> {
    let path=config_path(app)?;
    if !path.exists(){return Ok(Vec::new());}
    let text=fs::read_to_string(path).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível ler as áreas de importação."))?;
    let roots:Vec<ImportRoot>=serde_json::from_str(&text).map_err(|_|AppError::new("STORAGE_ERROR","As áreas de importação estão inválidas."))?;
    if roots.len()>limits::MAX_IMPORT_ROOTS { return Err(AppError::new("PATH_NOT_ALLOWED","Número de áreas de importação excede o limite.")); }
    for root in &roots {
        let p=Path::new(&root.path);
        if p.as_os_str().is_empty() || !p.is_absolute() || p.components().any(|c|matches!(c,Component::ParentDir|Component::CurDir)) {
            return Err(AppError::new("PATH_NOT_ALLOWED","Área de importação persistida inválida."));
        }
    }
    Ok(roots)
}

fn save_roots(app:&tauri::AppHandle,roots:&[ImportRoot])->AppResult<()> {
    let path=config_path(app)?;
    if let Some(parent)=path.parent(){fs::create_dir_all(parent).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível preparar as configurações do aplicativo."))?;}
    let tmp=path.with_extension("json.tmp");
    let data=serde_json::to_vec_pretty(roots).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível serializar as áreas de importação."))?;
    fs::write(&tmp,data).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível salvar as áreas de importação."))?;
    #[cfg(windows)]
    { if path.exists(){fs::remove_file(&path).map_err(|_|{let _=fs::remove_file(&tmp);AppError::new("STORAGE_ERROR","Não foi possível substituir as áreas de importação.")})?;} }
    fs::rename(&tmp,&path).map_err(|_|{let _=fs::remove_file(&tmp);AppError::new("STORAGE_ERROR","Não foi possível finalizar as áreas de importação.")})
}

pub fn add_root(app:&tauri::AppHandle,path:String)->AppResult<Vec<ImportRoot>> {
    if path.len()>4096{return Err(AppError::new("PATH_NOT_ALLOWED","Caminho de importação muito longo."));}
    let p=PathBuf::from(path);
    if !p.is_absolute(){return Err(AppError::new("PATH_NOT_ALLOWED","A área de importação deve usar caminho absoluto."));}
    let canonical=canonical_without_redirect(&p)?;
    if !canonical.is_dir(){return Err(AppError::new("INVALID_FILE","A área de importação deve ser um diretório."));}
    let mut roots=load_roots(app)?;
    let canonical_string=canonical.to_string_lossy().into_owned();
    if roots.len()>=limits::MAX_IMPORT_ROOTS && !roots.iter().any(|r|same_path(Path::new(&r.path),&canonical)){return Err(AppError::new("PATH_NOT_ALLOWED","Número máximo de áreas de importação atingido."));}
    if !roots.iter().any(|r|same_path(Path::new(&r.path),&canonical)){roots.push(ImportRoot{path:canonical_string});save_roots(app,&roots)?;}
    Ok(roots)
}

pub fn remove_root(app:&tauri::AppHandle,path:String)->AppResult<Vec<ImportRoot>> {
    let p=PathBuf::from(path);
    if !p.is_absolute(){return Err(AppError::new("PATH_NOT_ALLOWED","A área de importação deve usar caminho absoluto."));}
    let mut roots=load_roots(app)?;
    roots.retain(|r|!same_path(Path::new(&r.path),&p));
    save_roots(app,&roots)?;
    Ok(roots)
}

pub fn list_roots(app:&tauri::AppHandle)->AppResult<Vec<ImportRoot>> {
    let roots=load_roots(app)?;
    for root in &roots { let canonical=canonical_without_redirect(Path::new(&root.path))?; if !canonical.is_dir(){return Err(AppError::new("PATH_NOT_ALLOWED","Área de importação persistida não é um diretório."));} }
    Ok(roots)
}

pub fn open_verified(app:&tauri::AppHandle,source:&Path)->AppResult<File> {
    if source.as_os_str().is_empty() || !source.is_absolute(){return Err(AppError::new("PATH_NOT_ALLOWED","O caminho de importação deve ser absoluto."));}
    let normalized_source=normalize_absolute(source)?;
    if source.components().any(|c|matches!(c,Component::ParentDir|Component::CurDir)){return Err(AppError::new("PATH_NOT_ALLOWED","O caminho de importação não pode conter componentes relativos."));}
    if source.extension().and_then(|e|e.to_str()).map(|e|e.eq_ignore_ascii_case("lnk")||e.eq_ignore_ascii_case("url")).unwrap_or(false){return Err(AppError::new("PATH_NOT_ALLOWED","Atalhos do Windows não são permitidos como arquivos de origem."));}
    let roots=load_roots(app)?;
    if roots.is_empty(){return Err(AppError::new("PATH_NOT_ALLOWED","Nenhuma área de importação autorizada foi configurada."));}
    let canonical=fs::canonicalize(&normalized_source).map_err(|_|AppError::new("FILE_NOT_FOUND","Arquivo não encontrado."))?;
    if roots.iter().any(|r|canonical_without_redirect(Path::new(&r.path)).is_err()){return Err(AppError::new("PATH_NOT_ALLOWED","Uma área de importação autorizada está inválida."));}
    if roots.iter().any(|r|same_path(Path::new(&r.path),&canonical)||is_within(Path::new(&r.path),&canonical))==false{return Err(AppError::new("PATH_NOT_ALLOWED","O arquivo está fora das áreas de importação autorizadas."));}
    if !same_path(&normalized_source,&canonical){return Err(AppError::new("PATH_NOT_ALLOWED","O caminho atravessa um link, junction ou redirecionamento."));}
    let expected_id=canonical_identity(&canonical)?;
    let file=OpenOptions::new().read(true).open(&normalized_source).map_err(|_|AppError::new("FILE_NOT_FOUND","Arquivo não encontrado."))?;
    let opened_id=identity(&file)?;
    if opened_id!=expected_id{return Err(AppError::new("PATH_NOT_ALLOWED","O arquivo foi alterado durante a validação."));}
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalize_absolute_rejects_relative_and_parent_components(){
        assert!(normalize_absolute(Path::new("relative/file.txt")).is_err());
        if cfg!(windows){assert!(normalize_absolute(Path::new(r"C:\root\..\file.txt")).is_err());}
        else{assert!(normalize_absolute(Path::new("/tmp/../file.txt")).is_err());}
    }
    #[test]
    fn within_check_is_component_aware(){
        let base=std::env::temp_dir().join("tokensaver-root");
        assert!(is_within(&base,&base.join("file.txt")));
        assert!(!is_within(&base,&std::env::temp_dir().join("tokensaver-root-other").join("file.txt")));
    }
    #[cfg(windows)]
    #[test]
    fn windows_unc_and_device_namespaces_are_rejected(){
        assert!(normalize_absolute(Path::new(r"\\server\share\file.txt")).is_err());
        assert!(normalize_absolute(Path::new(r"\\?\C:\file.txt")).is_err());
        assert!(normalize_absolute(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_verbatim_drive_paths_compare_as_same_path(){
        assert!(same_path(Path::new(r"C:\Users\Test\file.txt"),Path::new(r"\\?\C:\Users\Test\file.txt")));
    }
}
