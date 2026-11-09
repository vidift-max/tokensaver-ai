use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

impl AppError {
    pub fn new(code: &str, message: &str) -> Self {
        Self { code: code.into(), message: message.into(), details: serde_json::json!({}) }
    }
    pub fn with_details(code: &str, message: &str, details: serde_json::Value) -> Self {
        Self { code: code.into(), message: message.into(), details }
    }
}
pub type AppResult<T> = Result<T, AppError>;
