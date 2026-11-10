use crate::error::{AppError, AppResult};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct OptimizationResult {
    pub original_chars: usize,
    pub optimized_chars: usize,
    pub saved_chars: usize,
    pub estimated_tokens_before: usize,
    pub estimated_tokens_after: usize,
    pub approximate: bool,
    pub changed: bool,
    pub content: String,
}

fn safe_normalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for raw in input.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let line = raw.trim_end_matches([' ', '\t']);
        if line.trim().is_empty() {
            if !out.ends_with("\n\n") && !out.is_empty() { out.push('\n'); }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.ends_with('\n') { out.pop(); }
    out
}

#[tauri::command]
pub fn optimize_text(input: String) -> AppResult<OptimizationResult> {
    if input.len() > 2_000_000 {
        return Err(AppError::new("INVALID_REQUEST", "O texto excede o limite do otimizador."));
    }
    let optimized = safe_normalize(&input);
    let before = ((input.chars().count() as f64) / 4.0).ceil() as usize;
    let after = ((optimized.chars().count() as f64) / 4.0).ceil() as usize;
    Ok(OptimizationResult {
        original_chars: input.chars().count(),
        optimized_chars: optimized.chars().count(),
        saved_chars: input.chars().count().saturating_sub(optimized.chars().count()),
        estimated_tokens_before: before,
        estimated_tokens_after: after,
        approximate: true,
        changed: optimized != input,
        content: optimized,
    })
}

#[cfg(test)]
mod tests {
    use super::safe_normalize;
    #[test]
    fn optimizer_preserves_content_and_removes_only_redundant_line_whitespace() {
        let input = "a  \n\n\n b  \n";
        assert_eq!(safe_normalize(input), "a  \n\nb");
    }
}
