use ai_team_room_service::{
    admit_authenticated_participant, auth::{authenticate_session, close_session, exchange_workload_token, require_permission, AuthError, OidcConfig},
    connect_pool, migrate, readiness,
};
use axum::{
    extract::{ConnectInfo, Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::{collections::{HashMap, VecDeque}, env, net::SocketAddr, sync::{Mutex, OnceLock}, time::{Duration, Instant}};
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

static EXCHANGE_ATTEMPTS: OnceLock<Mutex<HashMap<String, VecDeque<Instant>>>> = OnceLock::new();

async fn rate_limit_exchange(request: Request, next: Next) -> Response {
    let key = request.extensions().get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip().to_string())
        .unwrap_or_else(|| "unresolved-peer".to_string());
    let limits = EXCHANGE_ATTEMPTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut limits = limits.lock().expect("rate limiter mutex poisoned");
    let now = Instant::now();
    if limits.len() > 4096 {
        limits.retain(|_, attempts| attempts.back().is_some_and(|last| now.duration_since(*last) < Duration::from_secs(60)));
    }
    let attempts = limits.entry(key).or_default();
    while attempts.front().is_some_and(|instant| now.duration_since(*instant) >= Duration::from_secs(60)) {
        attempts.pop_front();
    }
    if attempts.len() >= 10 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    attempts.push_back(now);
    drop(limits);
    next.run(request).await
}

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

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/exchange", post(exchange).route_layer(middleware::from_fn(rate_limit_exchange)))
        .route("/auth/me", get(auth_me))
        .route("/auth/logout", post(logout))
        .route("/team-room/me", get(team_room_me))
        .route("/team-room/{room_id}/join", post(join_room))
        .layer(axum::extract::DefaultBodyLimit::max(20_000))
        .with_state(state)
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

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "AI Team Room service listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}


#[cfg(test)]
mod http_security_tests {
    use super::*;
    use axum::body::Body;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    fn test_app() -> Router {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://test:test@127.0.0.1/test")
            .expect("lazy pool configuration");
        build_router(AppState { pool, oidc: None })
    }

    #[tokio::test]
    async fn public_liveness_endpoint_is_available_without_authentication() {
        let response = test_app().oneshot(
            axum::http::Request::builder().uri("/health/live").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn identity_and_room_routes_reject_missing_bearer_credentials_over_http() {
        for path in ["/auth/me", "/team-room/me"] {
            let response = test_app().oneshot(
                axum::http::Request::builder().uri(path).body(Body::empty()).unwrap()
            ).await.unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn token_exchange_is_disabled_without_trusted_oidc_configuration() {
        let response = test_app().oneshot(
            axum::http::Request::builder().method("POST").uri("/auth/exchange")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"room_id":"00000000-0000-0000-0000-000000000001","workload_token":"synthetic"}"#)).unwrap()
        ).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
