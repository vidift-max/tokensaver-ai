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

fn is_canonical_sha256(sha: &str) -> bool {
    sha.len() == 64
        && sha.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
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
fn read_context_blob<R: Read>(
    reader: R,
    max_file_bytes: u64,
    remaining_batch_bytes: u64,
) -> std::io::Result<(Vec<u8>, bool, bool, bool)> {
    if remaining_batch_bytes == 0 {
        return Ok((Vec::new(), true, false, false));
    }

    // When the aggregate budget is the tighter limit, never read beyond it.
    // Otherwise, read one sentinel byte to detect a per-file limit violation.
    let read_limit = if remaining_batch_bytes > max_file_bytes {
        max_file_bytes.saturating_add(1)
    } else {
        remaining_batch_bytes
    };

    let mut raw = Vec::new();
    let read_failed = reader.take(read_limit).read_to_end(&mut raw).is_err();
    if read_failed {
        // read_to_end may have consumed bytes before returning an error.
        // Return those bytes and force the scan to stop to preserve accounting.
        return Ok((raw, true, false, true));
    }

    let aggregate_budget_exhausted =
        remaining_batch_bytes <= max_file_bytes && raw.len() as u64 == remaining_batch_bytes;
    let file_too_large =
        remaining_batch_bytes > max_file_bytes && raw.len() as u64 > max_file_bytes;

    Ok((raw, aggregate_budget_exhausted, file_too_large, false))
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
    for row in rows {
        if files_examined >= limits::MAX_FILES_PER_BATCH || bytes_read >= limits::MAX_BATCH_BYTES {
            break;
        }
        let (id, path, version, bytes, sha) = row
            .map_err(|_| AppError::new("DATABASE_ERROR", "Não foi possível ler o contexto."))?;
        files_examined += 1;
        if !is_canonical_sha256(&sha) {
            continue;
        }
        let blob = blobs.join(&sha);
        let canonical = match blob.canonicalize() {
            Ok(path) => path,
            Err(_) => continue,
        };
        if canonical.parent() != Some(blobs.as_path()) {
            continue;
        }
        let file = match fs::File::open(&canonical) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let remaining = limits::MAX_BATCH_BYTES.saturating_sub(bytes_read);
        let (raw, batch_exhausted, file_too_large, read_failed) =
            match read_context_blob(file, limits::MAX_FILE_BYTES, remaining) {
                Ok(result) => result,
                Err(_) => continue,
            };
        bytes_read = bytes_read.saturating_add(raw.len() as u64);
        if batch_exhausted || read_failed {
            // The last read may be a prefix of this file. Do not rank partial content.
            break;
        }
        if file_too_large {
            continue;
        }
        let text = match String::from_utf8(raw) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let lower = text.to_lowercase();
        let path_lower = path.to_lowercase();
        let mut score = 0i64;
        let mut first = String::new();
        for term in &terms {
            let path_count = path_lower.matches(term).count() as i64;
            let content_count = lower.matches(term).count() as i64;
            score += path_count * 12 + content_count.min(20);
            if first.is_empty() && content_count > 0 {
                first = excerpt(&text, term);
            }
        }
        if score > 0 {
            hits.push(ContextHit {
                file_id: id,
                logical_path: path,
                version_no: version,
                byte_len: bytes,
                token_estimate: ((text.chars().count() as f64) / 4.0).ceil() as i64,
                score,
                excerpt: if first.is_empty() {
                    text.chars().take(360).collect()
                } else {
                    first
                },
            });
        }
    }
    hits.sort_by(|a,b| b.score.cmp(&a.score).then_with(||a.logical_path.cmp(&b.logical_path))); hits.truncate(limit); Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::{excerpt, read_context_blob};
    use std::io::Cursor;

    #[test]
    fn smart_context_accepts_only_canonical_lowercase_sha256() {
        assert!(super::is_canonical_sha256(&"a".repeat(64)));
        assert!(!super::is_canonical_sha256(&"A".repeat(64)));
        assert!(!super::is_canonical_sha256("not-a-sha256"));
    }

    #[test]
    fn excerpt_handles_unicode_case_expansion_without_panicking() {
        let text = "prefix İSTO contexto final";
        let result = excerpt(text, "isto");
        assert!(result.contains("İSTO") || result.contains("contexto"));
    }

    #[test]
    fn context_read_never_exceeds_remaining_aggregate_budget() {
        let source = Cursor::new(b"abcdefgh".to_vec());
        let (bytes, exhausted, too_large, read_failed) = read_context_blob(source, 6, 3).unwrap();
        assert!(!read_failed);

        assert_eq!(bytes, b"abc");
        assert!(exhausted);
        assert!(!too_large);
    }

    #[test]
    fn context_read_detects_per_file_overflow_within_aggregate_budget() {
        let source = Cursor::new(b"abcde".to_vec());
        let (bytes, exhausted, too_large, read_failed) = read_context_blob(source, 4, 8).unwrap();
        assert!(!read_failed);

        assert_eq!(bytes, b"abcde");
        assert!(!exhausted);
        assert!(too_large);
    }

    #[test]
    fn context_read_accepts_file_smaller_than_remaining_budget() {
        let source = Cursor::new(b"abc".to_vec());
        let (bytes, exhausted, too_large) = read_context_blob(source, 4, 8).unwrap();

        assert_eq!(bytes, b"abc");
        assert!(!exhausted);
        assert!(!too_large);

    struct FailsAfterPrefix(bool);

    impl std::io::Read for FailsAfterPrefix {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.0 {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, "injected read failure"));
            }
            self.0 = true;
            buffer[..2].copy_from_slice(b"ab");
            Ok(2)
        }
    }

    #[test]
    fn context_read_accounts_for_partial_bytes_before_io_failure() {
        let (bytes, exhausted, too_large, read_failed) =
            read_context_blob(FailsAfterPrefix(false), 8, 16).unwrap();

        assert_eq!(bytes, b"ab");
        assert!(exhausted);
        assert!(!too_large);
        assert!(read_failed);
    }
    }
}
