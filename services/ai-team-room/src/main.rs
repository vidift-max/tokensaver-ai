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
    {
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
    }
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
    async fn exchange_rate_limit_and_body_limit_are_enforced_at_http_boundary() {
        let app = test_app();
        let mut last_status = StatusCode::OK;
        for _ in 0..11 {
            let mut request = axum::http::Request::builder().method("POST").uri("/auth/exchange")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"room_id":"00000000-0000-0000-0000-000000000001","workload_token":"x"}"#)).unwrap();
            request.extensions_mut().insert(ConnectInfo("127.0.0.11:8080".parse().unwrap()));
            let response = app.clone().oneshot(request).await.unwrap();
            last_status = response.status();
        }
        assert_eq!(last_status, StatusCode::TOO_MANY_REQUESTS);

        let mut oversized_request = axum::http::Request::builder().method("POST").uri("/auth/exchange")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"room_id":"00000000-0000-0000-0000-000000000001","workload_token":"{}"}}"#, "x".repeat(25_000)))).unwrap();
        oversized_request.extensions_mut().insert(ConnectInfo("127.0.0.12:8080".parse().unwrap()));
        let oversized = app.oneshot(oversized_request).await.unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn authenticated_http_routes_enforce_room_scope_permissions_and_revocation() {
        use sha2::{Digest, Sha256};
        use uuid::Uuid;

        let database_url = std::env::var("DATABASE_URL").expect("PostgreSQL CI required for HTTP integration test");
        let pool = connect_pool(&database_url).await.unwrap();
        migrate(&pool).await.unwrap();
        let tenant = Uuid::new_v4();
        let room = Uuid::new_v4();
        let other_room = Uuid::new_v4();
        let principal = Uuid::new_v4();
        let participant = Uuid::new_v4();
        let workload = Uuid::new_v4();
        let session = Uuid::new_v4();
        let execution = Uuid::new_v4();
        let issuer = "https://http-test-issuer.example";
        let subject = Uuid::new_v4().to_string();
        let raw_token = "http-integration-opaque-bearer";
        let digest = Sha256::digest(raw_token.as_bytes()).to_vec();

        sqlx::query("INSERT INTO tenants (tenant_id,name) VALUES ($1,'http-security-test')")
            .bind(tenant).execute(&pool).await.unwrap();
        for (room_id, name) in [(room, "http-room"), (other_room, "other-room")] {
            sqlx::query("INSERT INTO rooms (room_id,tenant_id,name) VALUES ($1,$2,$3)")
                .bind(room_id).bind(tenant).bind(name).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO principals (principal_id,tenant_id,principal_kind,provider,external_subject) VALUES ($1,$2,'AI',$3,$4)")
            .bind(principal).bind(tenant).bind(issuer).bind(&subject).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO participants (participant_id,tenant_id,principal_id,display_name) VALUES ($1,$2,$3,'http fixture')")
            .bind(participant).bind(tenant).bind(principal).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workload_identities (workload_id,tenant_id,principal_id,issuer,subject,status) VALUES ($1,$2,$3,$4,$5,'ACTIVE')")
            .bind(workload).bind(tenant).bind(principal).bind(issuer).bind(&subject).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO principal_permissions (permission_id,tenant_id,principal_id,room_id,permission) VALUES ($1,$2,$3,$4,'room.join')")
            .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO sessions (session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES ($1,$2,$3,$4,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE',$5)")
            .bind(session).bind(tenant).bind(room).bind(principal).bind(&digest).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workload_executions (execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES ($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE')")
            .bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session)
            .bind(format!("{issuer}|{subject}")).execute(&pool).await.unwrap();

        let app = build_router(AppState { pool: pool.clone(), oidc: None });
        let auth_response = app.clone().oneshot(
            axum::http::Request::builder().uri("/auth/me")
                .header("authorization", format!("Bearer {raw_token}"))
                .body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(auth_response.status(), StatusCode::OK);

        let read_denied = app.clone().oneshot(
            axum::http::Request::builder().uri("/team-room/me")
                .header("authorization", format!("Bearer {raw_token}"))
                .body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(read_denied.status(), StatusCode::FORBIDDEN);

        let cross_room = app.clone().oneshot(
            axum::http::Request::builder().method("POST")
                .uri(format!("/team-room/{other_room}/join"))
                .header("authorization", format!("Bearer {raw_token}"))
                .body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(cross_room.status(), StatusCode::FORBIDDEN);

        let joined = app.clone().oneshot(
            axum::http::Request::builder().method("POST")
                .uri(format!("/team-room/{room}/join"))
                .header("authorization", format!("Bearer {raw_token}"))
                .body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(joined.status(), StatusCode::OK);

        sqlx::query("UPDATE workload_identities SET status='REVOKED', revoked_at=NOW() WHERE workload_id=$1")
            .bind(workload).execute(&pool).await.unwrap();
        let revoked = app.oneshot(
            axum::http::Request::builder().uri("/auth/me")
                .header("authorization", format!("Bearer {raw_token}"))
                .body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);

        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
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
