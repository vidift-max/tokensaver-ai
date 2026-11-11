use crate::{error::{AppError, AppResult}};
use std::{fs, path::{Component, Path, PathBuf}};

fn storage_error(message: &'static str) -> AppError {
    AppError::new("STORAGE_ERROR", message)
}

/// Creates each parent component individually and refuses to traverse symlinks.
/// This avoids create_dir_all following an intermediate symlink before validation.
fn ensure_directory_chain_no_symlinks(path: &Path) -> AppResult<()> {
    let mut current = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                current.push(component.as_os_str());
                continue;
            }
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(storage_error("O caminho da pasta de projetos contém componente inválido."));
            }
            Component::Normal(_) => current.push(component.as_os_str()),
        }

        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(storage_error("Um componente da pasta de projetos não é um diretório regular."));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(create_error) if create_error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)
                            .map_err(|_| storage_error("Não foi possível verificar a pasta de projetos."))?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(storage_error("Um componente da pasta de projetos não é um diretório regular."));
                        }
                    }
                    Err(_) => return Err(storage_error("Não foi possível preparar a pasta de projetos.")),
                }
            }
            Err(_) => return Err(storage_error("Não foi possível verificar a pasta de projetos.")),
        }
    }

    Ok(())
}

/// Creates a project directory without replacing an existing entry.
/// Parent components are prepared one by one and symlinks are rejected.
pub fn claim_project_root(root: &Path) -> AppResult<()> {
    let parent = root.parent()
        .ok_or_else(|| storage_error("Caminho do projeto inválido."))?;
    ensure_directory_chain_no_symlinks(parent)?;

    match fs::create_dir(root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(root)
                .map_err(|_| storage_error("Não foi possível verificar o armazenamento do projeto."))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(storage_error("O armazenamento do projeto não é um diretório regular."));
            }
            Ok(())
        }
        Err(_) => Err(storage_error("Não foi possível gravar no armazenamento.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread};
    use uuid::Uuid;

    fn temp_path() -> PathBuf {
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

    #[test]
    fn restore_publication_racing_project_initialization_never_replaces_winner() {
        use std::sync::Barrier;

        let base = temp_path();
        let projects = base.join("projects");
        fs::create_dir_all(&projects).unwrap();
        let root = Arc::new(projects.join("project-race"));
        let stage = projects.join(".restore-stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("restore-marker"), b"restored").unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let init = {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                claim_project_root(&root)
            })
        };
        let restore = {
            let root = Arc::clone(&root);
            let stage = stage.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                crate::filesystem::publish_noreplace(&stage, &root)
            })
        };

        barrier.wait();
        init.join().expect("initializer thread")
            .expect("initialization must accept the winning directory claim");
        let restore_result = restore.join().expect("restore thread");

        assert!(root.is_dir());
        if restore_result.is_ok() {
            assert_eq!(fs::read(root.join("restore-marker")).unwrap(), b"restored");
            assert!(!stage.exists());
        } else {
            assert_eq!(restore_result.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
            assert!(!root.join("restore-marker").exists());
            assert!(stage.join("restore-marker").is_file());
        }

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

    #[cfg(unix)]
    #[test]
    fn claim_rejects_intermediate_parent_symlink_without_writing_outside() {
        use std::os::unix::fs::symlink;
        let base = temp_path();
        fs::create_dir_all(&base).unwrap();
        let outside = base.join("outside");
        fs::create_dir(&outside).unwrap();
        let linked_parent = base.join("storage");
        symlink(&outside, &linked_parent).unwrap();
        let root = linked_parent.join("projects").join("project-a");

        assert!(claim_project_root(&root).is_err());
        assert!(!outside.join("projects").exists());
        let _ = fs::remove_dir_all(base);
    }
}
