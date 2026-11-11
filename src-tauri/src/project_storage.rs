use crate::{error::{AppError, AppResult}};
use std::{fs, path::Path};

/// Creates a project directory without replacing an existing entry.
/// The parent is prepared first so first-run initialization works on a clean install.
pub fn claim_project_root(root: &Path) -> AppResult<()> {
    let parent = root.parent()
        .ok_or_else(|| AppError::new("STORAGE_ERROR", "Caminho do projeto inválido."))?;
    fs::create_dir_all(parent)
        .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível preparar a pasta de projetos."))?;

    let parent_meta = fs::symlink_metadata(parent)
        .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível verificar a pasta de projetos."))?;
    if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
        return Err(AppError::new("STORAGE_ERROR", "A pasta de projetos não é um diretório regular."));
    }

    match fs::create_dir(root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(root)
                .map_err(|_| AppError::new("STORAGE_ERROR", "Não foi possível verificar o armazenamento do projeto."))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AppError::new("STORAGE_ERROR", "O armazenamento do projeto não é um diretório regular."));
            }
            Ok(())
        }
        Err(_) => Err(AppError::new("STORAGE_ERROR", "Não foi possível gravar no armazenamento.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread};
    use uuid::Uuid;

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tokensaver-project-claim-{}", Uuid::new_v4()))
    }

    #[test]
    fn claim_creates_missing_parent_and_project_directory() {
        let base = temp_path();
        let root = base.join("storage").join("projects").join("project-a");
        claim_project_root(&root).expect("first project should initialize on a clean install");
        assert!(root.is_dir());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn claim_is_idempotent_for_existing_regular_directory() {
        let base = temp_path();
        let root = base.join("projects").join("project-a");
        fs::create_dir_all(&root).unwrap();
        claim_project_root(&root).expect("existing project directory should be accepted");
        assert!(root.is_dir());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn concurrent_claims_never_replace_or_fail_for_same_directory() {
        let base = temp_path();
        let root = Arc::new(base.join("storage").join("projects").join("project-a"));
        let workers: Vec<_> = (0..8).map(|_| {
            let root = Arc::clone(&root);
            thread::spawn(move || claim_project_root(&root))
        }).collect();
        for worker in workers {
            worker.join().expect("worker thread").expect("claim should be idempotent");
        }
        assert!(root.is_dir());
        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn claim_rejects_symlink_project_directory() {
        use std::os::unix::fs::symlink;
        let base = temp_path();
        let parent = base.join("projects");
        fs::create_dir_all(&parent).unwrap();
        let outside = base.join("outside");
        fs::create_dir(&outside).unwrap();
        let root = parent.join("project-a");
        symlink(&outside, &root).unwrap();
        assert!(claim_project_root(&root).is_err());
        let _ = fs::remove_dir_all(base);
    }
}
