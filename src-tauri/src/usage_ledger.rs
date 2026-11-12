use crate::provider_gateway::{ChatRequest, ChatResponse, Provider, Usage};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

fn provider_id(provider: Provider) -> &'static str {
    match provider {
        Provider::Openai => "openai",
        Provider::Anthropic => "anthropic",
        Provider::Gemini => "gemini",
        Provider::Kimi => "kimi",
    }
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn open(path: &Path) -> Result<Connection, rusqlite::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(error))
        })?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS provider_usage (
           request_id TEXT PRIMARY KEY,
           provider TEXT NOT NULL,
           model TEXT,
           project_id TEXT,
           actor_id TEXT,
           external_request_id TEXT,
           input_tokens INTEGER,
           output_tokens INTEGER,
           total_tokens INTEGER,
           requested_at INTEGER NOT NULL,
           latency_ms INTEGER,
           status TEXT NOT NULL CHECK(status IN ('started','succeeded','failed')),
           error_code TEXT,
           origin TEXT NOT NULL,
           estimated_cost_usd REAL,
           price_effective_from INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_provider_usage_project_time
           ON provider_usage(project_id, requested_at);
         CREATE INDEX IF NOT EXISTS idx_provider_usage_actor_time
           ON provider_usage(actor_id, requested_at);
         CREATE TABLE IF NOT EXISTS provider_model_prices (
           provider TEXT NOT NULL,
           model TEXT NOT NULL,
           input_usd_per_million REAL,
           output_usd_per_million REAL,
           effective_from INTEGER NOT NULL,
           source_url TEXT NOT NULL,
           recorded_at INTEGER NOT NULL,
           PRIMARY KEY(provider, model, effective_from),
           CHECK(input_usd_per_million IS NULL OR input_usd_per_million >= 0),
           CHECK(output_usd_per_million IS NULL OR output_usd_per_million >= 0)
         );"
    )?;
    Ok(conn)
}

/// Creates a durable, prompt-free usage record before the outbound provider call.
pub fn start(path: &Path, request_id: &str, request: &ChatRequest) -> Result<(), rusqlite::Error> {
    let conn = open(path)?;
    conn.execute(
        "INSERT INTO provider_usage
         (request_id, provider, model, project_id, actor_id, requested_at, status, origin)
         VALUES (?1, ?2, ?3, NULL, NULL, ?4, 'started', 'tauri_ipc')",
        params![request_id, provider_id(request.provider), request.model, now_epoch()],
    )?;
    Ok(())
}

/// Finalizes a successful request and returns a cost estimate only when a sourced,
/// effective-dated price and complete provider-reported usage are both available.
pub fn finish_success(path: &Path, response: &ChatResponse) -> Result<Option<f64>, rusqlite::Error> {
    let mut conn = open(path)?;
    let at = now_epoch();
    let cost = match response.usage.as_ref() {
        Some(usage) => calculate_cost(&conn, response.provider, &response.model, usage, at)?,
        None => None,
    };
    let effective_from: Option<i64> = conn.query_row(
        "SELECT MAX(effective_from) FROM provider_model_prices
         WHERE provider=?1 AND model=?2 AND effective_from<=?3",
        params![provider_id(response.provider), response.model, at],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE provider_usage SET model=?2, external_request_id=?3,
         input_tokens=?4, output_tokens=?5, total_tokens=?6, latency_ms=?7,
         status='succeeded', error_code=NULL, estimated_cost_usd=?8, price_effective_from=?9
         WHERE request_id=?1",
        params![
            response.request_id,
            response.model,
            response.provider_request_id,
            response.usage.as_ref().and_then(|u| u.input_tokens).and_then(|v| i64::try_from(v).ok()),
            response.usage.as_ref().and_then(|u| u.output_tokens).and_then(|v| i64::try_from(v).ok()),
            response.usage.as_ref().and_then(|u| u.total_tokens).and_then(|v| i64::try_from(v).ok()),
            i64::try_from(response.latency_ms).unwrap_or(i64::MAX),
            cost,
            if cost.is_some() { effective_from } else { None }
        ],
    )?;
    Ok(cost)
}

pub fn finish_failure(
    path: &Path,
    request_id: &str,
    error_code: &str,
    latency_ms: u128,
) -> Result<(), rusqlite::Error> {
    let conn = open(path)?;
    conn.execute(
        "UPDATE provider_usage SET status='failed', error_code=?2, latency_ms=?3
         WHERE request_id=?1",
        params![request_id, error_code, i64::try_from(latency_ms).unwrap_or(i64::MAX)],
    )?;
    Ok(())
}

fn calculate_cost(
    conn: &Connection,
    provider: Provider,
    model: &str,
    usage: &Usage,
    at: i64,
) -> Result<Option<f64>, rusqlite::Error> {
    let (Some(input), Some(output), Some(source)) =
        (usage.input_tokens, usage.output_tokens, Some(usage.source.as_str()))
    else {
        return Ok(None);
    };
    if source != "provider_reported" {
        return Ok(None);
    }
    let price: Option<(Option<f64>, Option<f64>)> = conn.query_row(
        "SELECT input_usd_per_million, output_usd_per_million
         FROM provider_model_prices
         WHERE provider=?1 AND model=?2 AND effective_from<=?3
         ORDER BY effective_from DESC LIMIT 1",
        params![provider_id(provider), model, at],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    let Some((Some(input_rate), Some(output_rate))) = price else {
        return Ok(None);
    };
    let amount = (input as f64 * input_rate + output as f64 * output_rate) / 1_000_000.0;
    if amount.is_finite() { Ok(Some(amount)) } else { Ok(None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tokensaver-usage-{}.sqlite",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn ledger_persists_prompt_free_request_and_provider_usage() {
        let path = temp_db();
        let request = ChatRequest {
            provider: Provider::Openai, model: Some("test-model".into()), system: None,
            messages: vec![], temperature: None, max_output_tokens: None, stream: false
        };
        start(&path, "request-1", &request).expect("start ledger row");
        let conn = open(&path).expect("ledger");
        let (status, origin, prompt_columns): (String, String, i64) = conn.query_row(
            "SELECT status, origin, (SELECT COUNT(*) FROM pragma_table_info('provider_usage') WHERE name LIKE '%prompt%' OR name LIKE '%content%')
             FROM provider_usage WHERE request_id='request-1'",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        ).expect("ledger row");
        assert_eq!(status, "started");
        assert_eq!(origin, "tauri_ipc");
        assert_eq!(prompt_columns, 0);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn success_finalizes_real_usage_without_inventing_cost() {
        let path = temp_db();
        let request = ChatRequest {
            provider: Provider::Anthropic, model: Some("claude-test".into()), system: None,
            messages: vec![], temperature: None, max_output_tokens: None, stream: false
        };
        start(&path, "request-success", &request).expect("start");
        let response = ChatResponse {
            provider: Provider::Anthropic, model: "claude-test".into(), text: "not persisted".into(),
            usage: Some(Usage { input_tokens: Some(12), output_tokens: Some(5), total_tokens: Some(17), source: "provider_reported".into() }),
            latency_ms: 321, request_id: "request-success".into(), provider_request_id: Some("external-123".into()), estimated_cost: None
        };
        assert_eq!(finish_success(&path, &response).expect("finish"), None);
        let conn = open(&path).expect("ledger");
        let (status, input, output, total, external_id, latency, cost): (String, i64, i64, i64, String, i64, Option<f64>) = conn.query_row(
            "SELECT status,input_tokens,output_tokens,total_tokens,external_request_id,latency_ms,estimated_cost_usd FROM provider_usage WHERE request_id='request-success'",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?))
        ).expect("finalized row");
        assert_eq!(status, "succeeded");
        assert_eq!((input, output, total), (12, 5, 17));
        assert_eq!(external_id, "external-123");
        assert_eq!(latency, 321);
        assert_eq!(cost, None);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failure_finalizes_status_and_normalized_error_only() {
        let path = temp_db();
        let request = ChatRequest {
            provider: Provider::Gemini, model: Some("gemini-test".into()), system: None,
            messages: vec![], temperature: None, max_output_tokens: None, stream: false
        };
        start(&path, "request-failed", &request).expect("start");
        finish_failure(&path, "request-failed", "TIMEOUT", 999).expect("failure update");
        let conn = open(&path).expect("ledger");
        let (status, code, latency): (String, String, i64) = conn.query_row(
            "SELECT status,error_code,latency_ms FROM provider_usage WHERE request_id='request-failed'",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))
        ).expect("failed row");
        assert_eq!(status, "failed");
        assert_eq!(code, "TIMEOUT");
        assert_eq!(latency, 999);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cost_is_unknown_without_a_sourced_effective_price() {
        let path = temp_db();
        let conn = open(&path).expect("ledger");
        let usage = Usage { input_tokens: Some(100), output_tokens: Some(50), total_tokens: Some(150), source: "provider_reported".into() };
        assert_eq!(calculate_cost(&conn, Provider::Openai, "unpriced-model", &usage, now_epoch()).unwrap(), None);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cost_uses_latest_effective_versioned_price() {
        let path = temp_db();
        let conn = open(&path).expect("ledger");
        let now = now_epoch();
        conn.execute(
            "INSERT INTO provider_model_prices(provider,model,input_usd_per_million,output_usd_per_million,effective_from,source_url,recorded_at)
             VALUES('openai','test-model',2.0,8.0,?1,'https://pricing.example/source',?1)",
            params![now - 100]
        ).unwrap();
        conn.execute(
            "INSERT INTO provider_model_prices(provider,model,input_usd_per_million,output_usd_per_million,effective_from,source_url,recorded_at)
             VALUES('openai','test-model',4.0,12.0,?1,'https://pricing.example/new-source',?1)",
            params![now - 10]
        ).unwrap();
        let usage = Usage { input_tokens: Some(1_000_000), output_tokens: Some(500_000), total_tokens: Some(1_500_000), source: "provider_reported".into() };
        let cost = calculate_cost(&conn, Provider::Openai, "test-model", &usage, now).unwrap().unwrap();
        assert!((cost - 10.0).abs() < 0.000001);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }
}
