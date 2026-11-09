use crate::error::AppError;

pub fn validate_batch(file_count: usize, total_bytes: u64) -> Result<(), AppError> {
    if file_count > crate::limits::MAX_FILES_PER_BATCH {
        return Err(AppError::with_details(
            "INVALID_REQUEST",
            "O lote excede o número máximo de arquivos.",
            serde_json::json!({"limit_files": crate::limits::MAX_FILES_PER_BATCH, "actual_files": file_count}),
        ));
    }
    if total_bytes > crate::limits::MAX_BATCH_BYTES {
        return Err(AppError::with_details(
            "FILE_TOO_LARGE",
            "O lote excede o tamanho total permitido.",
            serde_json::json!({"limit_bytes": crate::limits::MAX_BATCH_BYTES, "actual_bytes": total_bytes}),
        ));
    }
    Ok(())
}
