use ai_team_room_service::{
    admit_authenticated_participant, auth::{authenticate_session, close_session, exchange_workload_token, require_permission, AuthError, OidcConfig},
    connect_pool, migrate, readiness,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{env, net::SocketAddr};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    oidc: Option<OidcConfig>,
}

#[derive(Serialize)]
struct HealthResponse { service: &'static str, status: &'static str }

#[derive(Deserialize)]
struct ExchangeRequest { room_id: Uuid, workload_token: String }

async fn live() -> (StatusCode, Json<HealthResponse>) {
    (StatusCode::OK, Json(HealthResponse { service: "ai-team-room", status: "live" }))
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    if readiness(&state.pool).await {
        (StatusCode::OK, Json(HealthResponse { service: "ai-team-room", status: "ready" }))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(HealthResponse { service: "ai-team-room", status: "not_ready" }))
    }
}

fn auth_status(error: AuthError) -> StatusCode {
    match error {
        AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
        AuthError::IdentityUnavailable | AuthError::Forbidden => StatusCode::FORBIDDEN,
        AuthError::RoomUnavailable => StatusCode::NOT_FOUND,
        AuthError::ProviderUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        AuthError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn exchange(
    State(state): State<AppState>,
    Json(request): Json<ExchangeRequest>,
) -> Result<Json<ai_team_room_service::auth::ExchangeResponse>, StatusCode> {
    if request.workload_token.len() > 16_384 { return Err(StatusCode::PAYLOAD_TOO_LARGE); }
    let config = state.oidc.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    exchange_workload_token(&state.pool, config, request.room_id, &request.workload_token)
        .await.map(Json).map_err(auth_status)
}

async fn auth_me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ai_team_room_service::auth::AuthenticatedIdentity>, StatusCode> {
    authenticate_session(&state.pool, &headers).await.map(Json).map_err(auth_status)
}

async fn team_room_me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ai_team_room_service::auth::AuthenticatedIdentity>, StatusCode> {
    let identity = authenticate_session(&state.pool, &headers).await.map_err(auth_status)?;
    require_permission(&state.pool, &identity, "room.read").await.map_err(auth_status)?;
    Ok(Json(identity))
}

async fn join_room(
    State(state): State<AppState>,
    Path(room_id): Path<Uuid>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ai_team_room_service::AdmissionResult>, StatusCode> {
    let identity = authenticate_session(&state.pool, &headers).await.map_err(auth_status)?;
    if identity.room_id != room_id { return Err(StatusCode::FORBIDDEN); }
    require_permission(&state.pool, &identity, "room.join").await.map_err(auth_status)?;
    admit_authenticated_participant(
        &state.pool, room_id, identity.participant_id, identity.principal_id, identity.session_id,
    ).await.map(Json).map_err(|error| match error {
        ai_team_room_service::RoomServiceError::RoomCapacityReached => StatusCode::CONFLICT,
        ai_team_room_service::RoomServiceError::RoomUnavailable => StatusCode::NOT_FOUND,
        ai_team_room_service::RoomServiceError::ParticipantNotAuthorized
        | ai_team_room_service::RoomServiceError::InvalidSession
        | ai_team_room_service::RoomServiceError::DuplicateActiveSession => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })
}

async fn logout(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let identity = authenticate_session(&state.pool, &headers).await.map_err(auth_status)?;
    close_session(&state.pool, &identity).await.map_err(auth_status)?;
    Ok(StatusCode::NO_CONTENT)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let database_url = env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL must point to the shared PostgreSQL service")?;
    let bind: SocketAddr = env::var("TEAM_ROOM_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8081".to_string()).parse()?;

    let pool = connect_pool(&database_url).await?;
    migrate(&pool).await?;
    let oidc = match OidcConfig::from_env() {
        Ok(config) => { tracing::info!("OIDC workload trust configuration loaded"); Some(config) }
        Err(reason) => { tracing::warn!(%reason, "OIDC authentication disabled until trust configuration is provided"); None }
    };
    let state = AppState { pool, oidc };

    let app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/exchange", post(exchange))
        .route("/auth/me", get(auth_me))
        .route("/auth/logout", post(logout))
        .route("/team-room/me", get(team_room_me))
        .route("/team-room/{room_id}/join", post(join_room))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "AI Team Room service listening");
    axum::serve(listener, app).await?;
    Ok(())
}
