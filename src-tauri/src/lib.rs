use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Manager;

#[derive(Debug, Serialize)]
struct AppHealth { ok: bool, version: &'static str }

#[tauri::command]
fn health() -> Result<AppHealth, String> {
    Ok(AppHealth { ok: true, version: "0.1.0" })
}

#[tauri::command]
fn sha256_text(input: String) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![health, sha256_text])
        .setup(|app| {
            let _ = app.path();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running TokenSaver AI");
}
