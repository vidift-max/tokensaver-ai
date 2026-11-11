use ai_team_room_service::{connect_pool, migrate, readiness};
use axum::{http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use std::{env, net::SocketAddr};
use tracing_subscriber::EnvFilter;

#[derive(Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
}

async fn live() -> (StatusCode, Json<HealthResponse>) {
    (StatusCode::OK, Json(HealthResponse { service: "ai-team-room", status: "live" }))
}

async fn ready(
    axum::extract::State(pool): axum::extract::State<sqlx::PgPool>,
) -> (StatusCode, Json<HealthResponse>) {
    if readiness(&pool).await {
        (StatusCode::OK, Json(HealthResponse { service: "ai-team-room", status: "ready" }))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(HealthResponse { service: "ai-team-room", status: "not_ready" }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let database_url = env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL must point to the shared PostgreSQL service")?;
    let bind: SocketAddr = env::var("TEAM_ROOM_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8081".to_string())
        .parse()?;

    let pool = connect_pool(&database_url).await?;
    migrate(&pool).await?;

    let app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "AI Team Room service listening");
    axum::serve(listener, app).await?;
    Ok(())
}
