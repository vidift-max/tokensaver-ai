use crate::{error::{AppError, AppResult}, limits};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, io::Read, path::{Component, Path, PathBuf}, time::{Duration, Instant}};
use tauri::Manager;
use uuid::Uuid;

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
                Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _) | Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::DeviceNS(_) => {
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
        let rc=match r.next(){Some(value)=>value,None=>return true};
        let cc=match c.next(){Some(value)=>value,None=>return false};
        if !component_eq(&rc,&cc){return false;}
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
    {
        use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
        let mut info=MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
        let ok=unsafe{GetFileInformationByHandle(file.as_raw_handle(),info.as_mut_ptr())};
        if ok==0{return Err(AppError::new("PATH_NOT_ALLOWED","Não foi possível obter a identidade segura do arquivo."));}
        let info=unsafe{info.assume_init()};
        let index=((info.nFileIndexHigh as u64)<<32)|(info.nFileIndexLow as u64);
        if index==0{return Err(AppError::new("PATH_NOT_ALLOWED","Não foi possível obter a identidade segura do arquivo."));}
        return Ok(FileIdentity::Windows{volume:info.dwVolumeSerialNumber,index});
    }
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


const MAX_IMPORT_ROOTS_JSON_BYTES: u64 = 160 * 1024;
const MAX_IMPORT_ROOT_PATH_BYTES: usize = 4096;

struct RootsLock { file: File }
impl Drop for RootsLock {
    fn drop(&mut self) {
        #[cfg(unix)] unsafe { let _ = libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&self.file), libc::LOCK_UN); }
        #[cfg(windows)] unsafe {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
            use windows_sys::Win32::System::IO::OVERLAPPED;
            let mut ov: OVERLAPPED = std::mem::zeroed();
            let _ = UnlockFileEx(self.file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut ov);
        }
    }
}

fn lock_roots(app: &tauri::AppHandle) -> AppResult<RootsLock> {
    let path=config_path(app)?.with_file_name("import_roots.lock");
    if let Some(parent)=path.parent(){fs::create_dir_all(parent).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível preparar as configurações do aplicativo."))?;}
    let file=OpenOptions::new().read(true).write(true).create(true).open(&path)
        .map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível abrir o bloqueio das áreas de importação."))?;
    #[cfg(unix)] {
        use std::os::unix::io::AsRawFd;
        let deadline=Instant::now()+Duration::from_secs(5);
        loop { let rc=unsafe{libc::flock(file.as_raw_fd(),libc::LOCK_EX|libc::LOCK_NB)}; if rc==0{break;} if Instant::now()>=deadline{return Err(AppError::new("STORAGE_BUSY","As áreas de importação estão sendo atualizadas por outro processo."));} std::thread::sleep(Duration::from_millis(25)); }
    }
    #[cfg(windows)] unsafe {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::LockFileEx;
        use windows_sys::Win32::System::IO::OVERLAPPED;
        let mut ov: OVERLAPPED = std::mem::zeroed();
        let deadline=Instant::now()+Duration::from_secs(5);
        loop { let ok=LockFileEx(file.as_raw_handle(),0x00000002,0, u32::MAX,u32::MAX,&mut ov); if ok!=0{break;} if Instant::now()>=deadline{return Err(AppError::new("STORAGE_BUSY","As áreas de importação estão sendo atualizadas por outro processo."));} std::thread::sleep(Duration::from_millis(25)); }
    }
    Ok(RootsLock{file})
}

fn config_path(app:&tauri::AppHandle)->AppResult<PathBuf> {
    app.path().app_config_dir().map(|p|p.join("import_roots.json")).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível localizar as configurações do aplicativo."))
}

fn read_roots_file(path:&Path)->AppResult<Vec<ImportRoot>> {
    let metadata=match fs::symlink_metadata(path) {
        Ok(metadata)=>metadata,
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(Vec::new()),
        Err(_)=>return Err(AppError::new("STORAGE_ERROR","Não foi possível verificar as áreas de importação.")),
    };
    if !metadata.file_type().is_file() {
        return Err(AppError::new("PATH_NOT_ALLOWED","O arquivo de áreas de importação não pode ser um link ou diretório."));
    }
    let mut options=OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file=options.open(path).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível ler as áreas de importação."))?;
    let mut bytes=Vec::new();
    file.take(MAX_IMPORT_ROOTS_JSON_BYTES+1).read_to_end(&mut bytes).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível ler as áreas de importação."))?;
    if bytes.len() as u64>MAX_IMPORT_ROOTS_JSON_BYTES{return Err(AppError::new("PATH_NOT_ALLOWED","Arquivo de áreas de importação excede o limite."));}
    parse_roots_json(&bytes)
}

fn load_roots(app:&tauri::AppHandle)->AppResult<Vec<ImportRoot>> {
    read_roots_file(&config_path(app)?)
}

fn parse_roots_json(bytes: &[u8]) -> AppResult<Vec<ImportRoot>> {
    if bytes.len() as u64 > MAX_IMPORT_ROOTS_JSON_BYTES {
        return Err(AppError::new("PATH_NOT_ALLOWED","Arquivo de áreas de importação excede o limite."));
    }
    let text=std::str::from_utf8(bytes).map_err(|_|AppError::new("STORAGE_ERROR","As áreas de importação estão inválidas."))?;
    let roots:Vec<ImportRoot>=serde_json::from_str(text).map_err(|_|AppError::new("STORAGE_ERROR","As áreas de importação estão inválidas."))?;
    if roots.len()>limits::MAX_IMPORT_ROOTS { return Err(AppError::new("PATH_NOT_ALLOWED","Número de áreas de importação excede o limite.")); }
    for root in &roots {
        let p=Path::new(&root.path);
        if root.path.len()>MAX_IMPORT_ROOT_PATH_BYTES || p.as_os_str().is_empty() || !p.is_absolute() || p.components().any(|c|matches!(c,Component::ParentDir|Component::CurDir)) {
            return Err(AppError::new("PATH_NOT_ALLOWED","Área de importação persistida inválida."));
        }
    }
    Ok(roots)
}

fn save_roots_unlocked(app:&tauri::AppHandle,roots:&[ImportRoot])->AppResult<()> {
    if roots.len()>limits::MAX_IMPORT_ROOTS{return Err(AppError::new("PATH_NOT_ALLOWED","Número de áreas de importação excede o limite."));}
    if roots.iter().any(|r|r.path.len()>MAX_IMPORT_ROOT_PATH_BYTES){return Err(AppError::new("PATH_NOT_ALLOWED","Caminho de importação muito longo."));}
    let path=config_path(app)?;
    if let Some(parent)=path.parent(){fs::create_dir_all(parent).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível preparar as configurações do aplicativo."))?;}
    let tmp=path.with_file_name(format!(".import_roots.{}.tmp",Uuid::new_v4()));
    let data=serde_json::to_vec_pretty(roots).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível serializar as áreas de importação."))?;
    if data.len() as u64>MAX_IMPORT_ROOTS_JSON_BYTES{return Err(AppError::new("PATH_NOT_ALLOWED","Arquivo de áreas de importação excede o limite."));}
    { let mut file=OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível salvar as áreas de importação."))?;
      use std::io::Write;
      file.write_all(&data).map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível salvar as áreas de importação."))?;
      file.sync_all().map_err(|_|AppError::new("STORAGE_ERROR","Não foi possível sincronizar as áreas de importação."))?;
    }
    #[cfg(windows)]
    { use std::os::windows::ffi::OsStrExt; use windows_sys::Win32::Storage::FileSystem::{MoveFileExW,MOVEFILE_REPLACE_EXISTING,MOVEFILE_WRITE_THROUGH}; let from:Vec<u16>=tmp.as_os_str().encode_wide().chain(std::iter::once(0)).collect(); let to:Vec<u16>=path.as_os_str().encode_wide().chain(std::iter::once(0)).collect(); let ok=unsafe{MoveFileExW(from.as_ptr(),to.as_ptr(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)}; if ok==0{let _=fs::remove_file(&tmp);return Err(AppError::new("STORAGE_ERROR","Não foi possível substituir as áreas de importação."));} Ok(()) }
    #[cfg(not(windows))]
    fs::rename(&tmp,&path).map_err(|_|{let _=fs::remove_file(&tmp);AppError::new("STORAGE_ERROR","Não foi possível finalizar as áreas de importação.")})
}

fn is_forbidden_link_name(path:&Path)->bool {
    path.file_name()
        .and_then(|name|name.to_str())
        .map(|name|name.eq_ignore_ascii_case("import_roots.json") || name.ends_with(".lnk") || name.ends_with(".url"))
        .unwrap_or(false)
}

pub fn add_root(app:&tauri::AppHandle,path:String)->AppResult<Vec<ImportRoot>> {
    if path.len()>4096{return Err(AppError::new("PATH_NOT_ALLOWED","Caminho de importação muito longo."));}
    let p=PathBuf::from(path);
    if !p.is_absolute(){return Err(AppError::new("PATH_NOT_ALLOWED","A área de importação deve usar caminho absoluto."));}
    if is_forbidden_link_name(&p){return Err(AppError::new("PATH_NOT_ALLOWED","Atalhos do Windows não podem ser áreas de importação."));}
    let canonical=canonical_without_redirect(&p)?;
    if !canonical.is_dir(){return Err(AppError::new("INVALID_FILE","A área de importação deve ser um diretório."));}
    let _lock=lock_roots(app)?;
    let mut roots=load_roots(app)?;
    let canonical_string=canonical.to_string_lossy().into_owned();
    if roots.len()>=limits::MAX_IMPORT_ROOTS && !roots.iter().any(|r|same_path(Path::new(&r.path),&canonical)){return Err(AppError::new("PATH_NOT_ALLOWED","Número máximo de áreas de importação atingido."));}
    if !roots.iter().any(|r|same_path(Path::new(&r.path),&canonical)){roots.push(ImportRoot{path:canonical_string});save_roots_unlocked(app,&roots)?;}
    Ok(roots)
}

pub fn remove_root(app:&tauri::AppHandle,path:String)->AppResult<Vec<ImportRoot>> {
    let p=PathBuf::from(path);
    if !p.is_absolute(){return Err(AppError::new("PATH_NOT_ALLOWED","A área de importação deve usar caminho absoluto."));}
    if is_forbidden_link_name(&p){return Err(AppError::new("PATH_NOT_ALLOWED","Atalhos do Windows não podem ser áreas de importação."));}
    let _lock=lock_roots(app)?;
    let mut roots=load_roots(app)?;
    roots.retain(|r|!same_path(Path::new(&r.path),&p));
    save_roots_unlocked(app,&roots)?;
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
    fn roots_json_rejects_oversized_input_before_parsing() {
        let bytes = vec![b' '; (MAX_IMPORT_ROOTS_JSON_BYTES as usize) + 1];
        assert!(parse_roots_json(&bytes).is_err());
    }

    #[test]
    fn roots_json_rejects_malformed_and_invalid_utf8() {
        assert!(parse_roots_json(b"{not-json").is_err());
        assert!(parse_roots_json(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn roots_json_accepts_empty_list() {
        assert!(parse_roots_json(b"[]").unwrap().is_empty());
    }

    #[test]
    fn roots_json_rejects_relative_paths() {
        assert!(parse_roots_json(br#"[{"path":"relative/folder"}]"#).is_err());
    }

    #[test]
    fn roots_json_rejects_parent_components() {
        #[cfg(unix)]
        assert!(parse_roots_json(br#"[{"path":"/safe/../outside"}]"#).is_err());
        #[cfg(windows)]
        assert!(parse_roots_json(br#"[{"path":"C:\\safe\\..\\outside"}]"#).is_err());
    }

    #[test]
    fn roots_json_rejects_overlong_path_values() {
        let path = "a".repeat(MAX_IMPORT_ROOT_PATH_BYTES + 1);
        let json = serde_json::to_vec(&vec![ImportRoot { path }]).unwrap();
        assert!(parse_roots_json(&json).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn persisted_roots_file_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let root=std::env::temp_dir().join(format!("tokensaver-roots-link-{}",Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let target=root.join("valid.json");
        let link=root.join("import_roots.json");
        fs::write(&target,b"[]").unwrap();
        symlink(&target,&link).unwrap();

        assert!(read_roots_file(&link).is_err());

        let _=fs::remove_dir_all(root);
    }

    #[test]
    fn roots_json_enforces_the_configured_root_count_limit() {
        let roots: Vec<ImportRoot> = (0..limits::MAX_IMPORT_ROOTS)
            .map(|index| ImportRoot {
                #[cfg(windows)]
                path: format!(r"C:\\import-root-{index}"),
                #[cfg(not(windows))]
                path: format!("/tmp/import-root-{index}"),
            })
            .collect();
        let within_limit = serde_json::to_vec(&roots).unwrap();
        assert_eq!(parse_roots_json(&within_limit).unwrap().len(), limits::MAX_IMPORT_ROOTS);

        let mut over_limit = roots;
        over_limit.push(ImportRoot {
            #[cfg(windows)]
            path: r"C:\\import-root-over-limit".to_string(),
            #[cfg(not(windows))]
            path: "/tmp/import-root-over-limit".to_string(),
        });
        let over_limit = serde_json::to_vec(&over_limit).unwrap();
        assert!(parse_roots_json(&over_limit).is_err());
    }

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
