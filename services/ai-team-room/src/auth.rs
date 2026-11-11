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

#[derive(Debug, Deserialize, Serialize)]
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
    sqlx::query("DELETE FROM auth_token_replays WHERE expires_at <= NOW()")
        .execute(&mut *tx).await?;
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
    sqlx::query("UPDATE sessions SET status = 'EXPIRED' WHERE token_hash = $1 AND status = 'ACTIVE' AND expires_at <= NOW()")
        .bind(&digest).execute(pool).await?;
    sqlx::query("UPDATE workload_executions SET status = 'EXPIRED' WHERE session_id IN (SELECT session_id FROM sessions WHERE token_hash = $1 AND status = 'EXPIRED') AND status = 'ACTIVE'")
        .bind(&digest).execute(pool).await?;
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


#[cfg(test)]
mod jwt_tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;
    use tokio::net::TcpListener;

    const PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDIuXmGCianrTIb
EGxBtFvQb2PxhHpiiWo5UPDDqMKIIa6tDqX+02a9aHORgwWRHR9J9E5ZqjJ6su4a
UTpEtwNqsZKOTRbTuQfp1xa1DLknwFZj2Y7MmNwPIw9xK16vURIu5ycZtU4WzhCP
4mr5aokOycOTNRId5mgKI1SCAunH/kMVVzYUsumospaQqvZptx9ksXCW7fBukQ5L
fCFwE6mll38r4cBWthibCIMNVBUDoTiKWrvylw6spV0JqctB5GDJvKRmxqVGn0ec
3pCxlXCjeTQ8cIeim4agbIW7xlsG9fPpyybnQqiPp4+RP0Flpab9z3iMjGRyKQMr
mvSHPo/fAgMBAAECggEABKIe8huwUkpOLFbeJxMA0/9uPX9KIbImjQBehXzL39vm
SmwhzlaWq8DRXo25W7pFlhFR8MkGoCjh2z83MeMVXo05qgmtuf2gaQZrN+A2T0HQ
iulGz8mLgRHxxzIx3eF2osOjZ/kRE9QgwYUN5KSlPQvQynL9DNi/sPCpZCn+m0OA
49UQuF7wXhvE0ZnniUPMubAlLE9QCjm0oJyE2Cz1ggEkQVLOkgJVPWcAxx7+Rq+a
HlLTkvfAZqF2WoGkORqKBoXu50HzA+y/VGQzEWW04WaqzZD2eHCWVgFnC+TIFBhN
FwctX2iG0wjpcDpIYZ3odkTB/BrjyiRLwOVbiojZUQKBgQDi5asixAhqdCBHe31J
ojM17YtRXlZnkbdtgqfAPErhyPgzOqc/bE04UDN7yK4eMYvS1dYOhwQEU6UNiiNp
6c40N8qUES2f6Q6+ATH2wGQqpX88JT4/7KSoDGWKujAy/NKNMt4mWCC6shfnykUM
QyuFgex9GPp6wDg+hZ5x6SQICQKBgQDieGivz+fgOECpJFLrNq1Mb5yycfPHQNl
LtGKQgWEK+aNLmbDnZzhcRCwmGj/HaquIdx4gIhz4wj6py2BQEeEvTgSgDKYn0JCl
F9VhzxDitsWH/Ik8n84MhqV13rLpG9e577iucTcsr0HE9FILj1V4h565C14I9XJk
vZConhNCpwKBgQCe4x+tT8No8HruM66vkpLz762AxGQTULo3SKg5+L6jT71m+mrG
AgwtJBR5WTV81UtJ0A5+y9NnSD+rtJFKCL0qWetVNpZRhb2aFnIrjdxW9N+deaMW
1psTRuMQe4NnIR48XGOuTEZXFoGhAkbJ6HugmTPHAmfc76AzQBSNRirCKQKBgGNR
VO/NaK4vXUQQNwEKUIPV6s1jJfq7GNCXgiZHZwyOICv2cdt5rADisM3g8xl8UU+e
laqX60a4wQxrsYhL3wqE14htI60QTW7pfd3QFmKmrwgNGw5oMs/ReNyxVpreaAQV
+lMeOYkyLtC2KQkCygP6zgPu1UVuhsVhUQBxmpa/AoGBAIOLs6rSTcZ2/C7pI/Un
+J0c2OUloGFyh2myDNABc3qV5XW2Ax8tNc4qHkHf5j4WS3mWDvjsNDYrHKcfI0og
Zz2oCgvmWOAGdhIM+9i/xRZaXDUMyZSx/1bSRaWrcJHgWcnGuVQ75O5rVSjordo7
a93ROy+abL98Yf/fSAlNg3Lw
-----END PRIVATE KEY-----"#;

    const N: &str = "yLl5hgomp60yGxBsQbRb0G9j8YR6YolqOVDww6jCiCGurQ6l_tNmvWhzkYMFkR0fSfROWaoyerLuGlE6RLcDarGSjk0W07kH6dcWtQy5J8BWY9mOzJjcDyMPcSter1ESLucnGbVOFs4Qj-Jq-WqJDsnDkzUSHeZoCiNUggLpx_5DFVc2FLLpqLKWkKr2abcfZLFwlu3wbpEOS3whcBOppZd_K-HAVrYYmwiDDVQVA6E4ilq78pcOrKVdCanLQeRgybykZsalRp9HnN6QsZVwo3k0PHCHopuGoGyFu8ZbBvXz6csm50Koj6ePkT9BZaWm_c94jIxkcikDK5r0hz6P3w";

    async fn test_server() -> (String, tokio::task::JoinHandle<()>) {
        let jwks = json!({"keys":[{"kty":"RSA","kid":"test-key","use":"sig","alg":"RS256","n":N,"e":"AQAB"}]});
        let app = Router::new().route("/jwks", get(move || {
            let value = jwks.clone();
            async move { Json(value) }
        }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        (format!("http://{address}/jwks"), task)
    }

    fn signed_token(issuer: &str, audience: &str, exp: usize, kid: &str) -> String {
        let claims = OidcClaims {
            iss: issuer.into(), sub: "agent-subject".into(), aud: json!(audience),
            exp, nbf: Some(Utc::now().timestamp() as usize - 1),
            jti: Uuid::new_v4().to_string(), scope: "team_room:connect".into(),
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.into());
        encode(&header, &claims, &EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn oidc_jwks_signature_issuer_audience_expiry_and_kid_are_enforced() {
        let (jwks_url, server) = test_server().await;
        let config = OidcConfig { issuer: "https://issuer.example.test".into(), audience: "team-room-api".into(), jwks_url };
        let valid = signed_token(&config.issuer, &config.audience, (Utc::now() + Duration::minutes(2)).timestamp() as usize, "test-key");
        assert!(verify_oidc_token(&config, &valid).await.is_ok());

        let expired = signed_token(&config.issuer, &config.audience, (Utc::now() - Duration::minutes(2)).timestamp() as usize, "test-key");
        assert!(matches!(verify_oidc_token(&config, &expired).await, Err(AuthError::Unauthorized)));

        let wrong_issuer = signed_token("https://attacker.example.test", &config.audience, (Utc::now() + Duration::minutes(2)).timestamp() as usize, "test-key");
        assert!(matches!(verify_oidc_token(&config, &wrong_issuer).await, Err(AuthError::Unauthorized)));

        let wrong_audience = signed_token(&config.issuer, "other-api", (Utc::now() + Duration::minutes(2)).timestamp() as usize, "test-key");
        assert!(matches!(verify_oidc_token(&config, &wrong_audience).await, Err(AuthError::Unauthorized)));

        let unknown_kid = signed_token(&config.issuer, &config.audience, (Utc::now() + Duration::minutes(2)).timestamp() as usize, "unknown-key");
        assert!(matches!(verify_oidc_token(&config, &unknown_kid).await, Err(AuthError::Unauthorized)));

        let mut invalid_signature = valid.clone();
        invalid_signature.push('x');
        assert!(matches!(verify_oidc_token(&config, &invalid_signature).await, Err(AuthError::Unauthorized)));
        server.abort();
    }

    #[tokio::test]
    async fn oidc_algorithms_other_than_rs256_are_rejected_before_jwks_use() {
        let config = OidcConfig { issuer: "https://issuer.example.test".into(), audience: "team-room-api".into(), jwks_url: "https://unused.invalid/jwks".into() };
        let token = jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &json!({"iss":config.issuer,"sub":"x","aud":config.audience,"exp":(Utc::now()+Duration::minutes(1)).timestamp(),"jti":"x","scope":"team_room:connect"}),
            &jsonwebtoken::EncodingKey::from_secret(b"test-only-secret"),
        ).unwrap();
        assert!(matches!(verify_oidc_token(&config, &token).await, Err(AuthError::Unauthorized)));
    }
}
