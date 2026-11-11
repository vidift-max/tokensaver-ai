use axum::http::HeaderMap;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
}

impl OidcConfig {
    pub fn from_env() -> Result<Self, String> {
        let issuer = std::env::var("OIDC_ISSUER").map_err(|_| "OIDC_ISSUER is required")?;
        let audience = std::env::var("OIDC_AUDIENCE").map_err(|_| "OIDC_AUDIENCE is required")?;
        let jwks_url = std::env::var("OIDC_JWKS_URL").map_err(|_| "OIDC_JWKS_URL is required")?;
        if !issuer.starts_with("https://") || !jwks_url.starts_with("https://") {
            return Err("OIDC issuer and JWKS URL must use HTTPS".into());
        }
        Ok(Self { issuer, audience, jwks_url })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("credential rejected")]
    Unauthorized,
    #[error("identity is not registered or enabled")]
    IdentityUnavailable,
    #[error("permission denied")]
    Forbidden,
    #[error("room unavailable")]
    RoomUnavailable,
    #[error("database failure: {0}")]
    Database(#[from] sqlx::Error),
    #[error("identity provider unavailable")]
    ProviderUnavailable,
}

#[derive(Debug, Deserialize)]
struct OidcClaims {
    iss: String,
    sub: String,
    aud: Value,
    exp: usize,
    #[serde(default)]
    nbf: Option<usize>,
    jti: String,
    #[serde(default)]
    scope: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedIdentity {
    pub tenant_id: Uuid,
    pub participant_id: Uuid,
    pub principal_id: Uuid,
    pub session_id: Uuid,
    pub execution_id: Uuid,
    pub room_id: Uuid,
    pub issuer: String,
    pub subject: String,
}

#[derive(Serialize)]
pub struct ExchangeResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_at: chrono::DateTime<Utc>,
    pub identity: AuthenticatedIdentity,
}

#[derive(FromRow)]
struct RegisteredIdentity {
    tenant_id: Uuid,
    principal_id: Uuid,
    participant_id: Uuid,
}

#[derive(FromRow)]
struct SessionIdentity {
    tenant_id: Uuid,
    participant_id: Uuid,
    principal_id: Uuid,
    session_id: Uuid,
    execution_id: Uuid,
    room_id: Uuid,
    issuer: String,
    subject: String,
}

async fn verify_oidc_token(config: &OidcConfig, token: &str) -> Result<OidcClaims, AuthError> {
    let header = decode_header(token).map_err(|_| AuthError::Unauthorized)?;
    if header.alg != Algorithm::RS256 {
        return Err(AuthError::Unauthorized);
    }
    let kid = header.kid.ok_or(AuthError::Unauthorized)?;

    // Fetch the configured issuer's JWKS over TLS. The issuer/JWKS URL are deployment
    // trust configuration, never values taken from an untrusted token.
    let response = reqwest::Client::new()
        .get(&config.jwks_url)
        .send().await.map_err(|_| AuthError::ProviderUnavailable)?;
    if !response.status().is_success() {
        return Err(AuthError::ProviderUnavailable);
    }
    let jwks: Value = response.json().await.map_err(|_| AuthError::ProviderUnavailable)?;
    let keys = jwks.get("keys").and_then(Value::as_array).ok_or(AuthError::ProviderUnavailable)?;
    let jwk = keys.iter().find(|key| {
        key.get("kid").and_then(Value::as_str) == Some(kid.as_str())
            && key.get("kty").and_then(Value::as_str) == Some("RSA")
            && key.get("alg").and_then(Value::as_str).map(|v| v == "RS256").unwrap_or(true)
            && key.get("use").and_then(Value::as_str).map(|v| v == "sig").unwrap_or(true)
    }).ok_or(AuthError::Unauthorized)?;
    let n = jwk.get("n").and_then(Value::as_str).ok_or(AuthError::Unauthorized)?;
    let e = jwk.get("e").and_then(Value::as_str).ok_or(AuthError::Unauthorized)?;
    let key = DecodingKey::from_rsa_components(n, e).map_err(|_| AuthError::Unauthorized)?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[config.issuer.as_str()]);
    validation.set_audience(&[config.audience.as_str()]);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = 0;
    let claims = decode::<OidcClaims>(token, &key, &validation)
        .map_err(|_| AuthError::Unauthorized)?.claims;

    if claims.iss != config.issuer
        || claims.sub.trim().is_empty()
        || claims.jti.trim().is_empty()
        || !claims.scope.split_ascii_whitespace().any(|s| s == "team_room:connect")
    {
        return Err(AuthError::Unauthorized);
    }
    Ok(claims)
}

pub async fn exchange_workload_token(
    pool: &PgPool,
    config: &OidcConfig,
    room_id: Uuid,
    external_token: &str,
) -> Result<ExchangeResponse, AuthError> {
    let claims = verify_oidc_token(config, external_token).await?;
    let now = Utc::now();
    let expires_at = std::cmp::min(
        chrono::DateTime::<Utc>::from_timestamp(claims.exp as i64, 0).ok_or(AuthError::Unauthorized)?,
        now + Duration::minutes(15),
    );
    if expires_at <= now { return Err(AuthError::Unauthorized); }

    let mut tx = pool.begin().await?;
    // A provider assertion is accepted once. Replaying the same jti cannot mint
    // another local session, even when two exchanges race.
    let replay = sqlx::query(
        "INSERT INTO auth_token_replays (issuer, token_jti, expires_at) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
    ).bind(&claims.iss).bind(&claims.jti).bind(expires_at)
        .execute(&mut *tx).await?;
    if replay.rows_affected() != 1 { return Err(AuthError::Unauthorized); }

    let registered = sqlx::query_as::<_, RegisteredIdentity>(
        r#"SELECT pr.tenant_id, pr.principal_id, p.participant_id
           FROM principals pr
           JOIN participants p ON p.tenant_id = pr.tenant_id AND p.principal_id = pr.principal_id
           JOIN rooms r ON r.tenant_id = pr.tenant_id AND r.room_id = $3 AND r.status = 'ACTIVE'
           WHERE pr.provider = $1 AND pr.external_subject = $2
             AND pr.principal_kind = 'AI' AND pr.enabled = TRUE AND p.enabled = TRUE
           ORDER BY p.created_at ASC LIMIT 1 FOR UPDATE OF pr, p"#
    ).bind(&claims.iss).bind(&claims.sub).bind(room_id)
        .fetch_optional(&mut *tx).await?.ok_or(AuthError::IdentityUnavailable)?;

    let allowed: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM principal_permissions
           WHERE tenant_id = $1 AND principal_id = $2 AND permission = 'room.join'
             AND revoked_at IS NULL AND (room_id IS NULL OR room_id = $3))"#
    ).bind(registered.tenant_id).bind(registered.principal_id).bind(room_id)
        .fetch_one(&mut *tx).await?;
    if !allowed { return Err(AuthError::Forbidden); }

    let previous: Option<Uuid> = sqlx::query_scalar(
        "SELECT session_id FROM sessions WHERE room_id = $1 AND principal_id = $2 AND status = 'ACTIVE' AND revoked_at IS NULL FOR UPDATE"
    ).bind(room_id).bind(registered.principal_id).fetch_optional(&mut *tx).await?;
    if let Some(old_id) = previous {
        sqlx::query("UPDATE sessions SET status = 'REVOKED', revoked_at = NOW() WHERE session_id = $1")
            .bind(old_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workload_executions SET status = 'REVOKED', revoked_at = NOW() WHERE session_id = $1 AND revoked_at IS NULL")
            .bind(old_id).execute(&mut *tx).await?;
    }

    let session_id = Uuid::new_v4();
    let execution_id = Uuid::new_v4();
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let local_token = URL_SAFE_NO_PAD.encode(raw);
    let token_hash = Sha256::digest(local_token.as_bytes()).to_vec();
    let worker_subject = format!("{}|{}", claims.iss, claims.sub);

    sqlx::query(
        r#"INSERT INTO sessions
           (session_id, tenant_id, room_id, principal_id, issued_at, expires_at, status, token_hash, rotated_from_session_id)
           VALUES ($1,$2,$3,$4,$5,$6,'ACTIVE',$7,$8)"#
    ).bind(session_id).bind(registered.tenant_id).bind(room_id).bind(registered.principal_id)
        .bind(now).bind(expires_at).bind(token_hash).bind(previous)
        .execute(&mut *tx).await?;

    sqlx::query(
        r#"INSERT INTO workload_executions
           (execution_id, tenant_id, room_id, participant_id, principal_id, session_id,
            worker_subject, heartbeat_at, expires_at, status)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'ACTIVE')"#
    ).bind(execution_id).bind(registered.tenant_id).bind(room_id).bind(registered.participant_id)
        .bind(registered.principal_id).bind(session_id).bind(worker_subject).bind(now).bind(expires_at)
        .execute(&mut *tx).await?;

    tx.commit().await?;
    Ok(ExchangeResponse {
        access_token: local_token,
        token_type: "Bearer",
        expires_at,
        identity: AuthenticatedIdentity {
            tenant_id: registered.tenant_id,
            participant_id: registered.participant_id,
            principal_id: registered.principal_id,
            session_id,
            execution_id,
            room_id,
            issuer: claims.iss,
            subject: claims.sub,
        },
    })
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AuthError> {
    let value = headers.get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok()).ok_or(AuthError::Unauthorized)?;
    value.strip_prefix("Bearer ").filter(|v| !v.is_empty()).ok_or(AuthError::Unauthorized)
}

pub async fn authenticate_session(pool: &PgPool, headers: &HeaderMap) -> Result<AuthenticatedIdentity, AuthError> {
    let token = bearer_token(headers)?;
    let digest = Sha256::digest(token.as_bytes()).to_vec();
    let row = sqlx::query_as::<_, SessionIdentity>(
        r#"SELECT s.tenant_id, w.participant_id, s.principal_id, s.session_id, w.execution_id,
                  s.room_id, pr.provider AS issuer, pr.external_subject AS subject
           FROM sessions s
           JOIN principals pr ON pr.tenant_id = s.tenant_id AND pr.principal_id = s.principal_id
           JOIN participants p ON p.tenant_id = s.tenant_id AND p.principal_id = s.principal_id AND p.enabled = TRUE
           JOIN workload_executions w ON w.tenant_id = s.tenant_id AND w.room_id = s.room_id
                AND w.participant_id = p.participant_id AND w.principal_id = s.principal_id
                AND w.session_id = s.session_id
           WHERE s.token_hash = $1 AND s.status = 'ACTIVE' AND s.revoked_at IS NULL
             AND s.expires_at > NOW() AND pr.enabled = TRUE
             AND w.status = 'ACTIVE' AND w.revoked_at IS NULL AND w.expires_at > NOW()
           LIMIT 1"#
    ).bind(digest).fetch_optional(pool).await?.ok_or(AuthError::Unauthorized)?;
    Ok(AuthenticatedIdentity {
        tenant_id: row.tenant_id, participant_id: row.participant_id, principal_id: row.principal_id,
        session_id: row.session_id, execution_id: row.execution_id, room_id: row.room_id,
        issuer: row.issuer, subject: row.subject,
    })
}

pub async fn require_permission(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    permission: &str,
) -> Result<(), AuthError> {
    let allowed: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM principal_permissions
           WHERE tenant_id = $1 AND principal_id = $2 AND permission = $3
             AND revoked_at IS NULL AND (room_id IS NULL OR room_id = $4))"#
    ).bind(identity.tenant_id).bind(identity.principal_id).bind(permission).bind(identity.room_id)
        .fetch_one(pool).await?;
    if allowed { Ok(()) } else { Err(AuthError::Forbidden) }
}

pub async fn close_session(pool: &PgPool, identity: &AuthenticatedIdentity) -> Result<(), AuthError> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE sessions SET status = 'CLOSED', closed_at = NOW(), revoked_at = COALESCE(revoked_at, NOW()) WHERE session_id = $1 AND principal_id = $2")
        .bind(identity.session_id).bind(identity.principal_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE workload_executions SET status = 'CLOSED', revoked_at = COALESCE(revoked_at, NOW()) WHERE execution_id = $1 AND session_id = $2")
        .bind(identity.execution_id).bind(identity.session_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
