use ai_team_room_service::{
    admit_authenticated_participant,
    auth::{authenticate_session, close_session, exchange_workload_token, require_permission, AuthError, OidcConfig},
    connect_pool, migrate,
};
use axum::{routing::get, Json, Router};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use tokio::net::TcpListener;
use uuid::Uuid;

const ISSUER: &str = "https://team-room-test-issuer.example";
const AUDIENCE: &str = "team-room-api";
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

#[derive(Serialize)]
struct Claims { iss: String, sub: String, aud: String, exp: usize, nbf: usize, jti: String, scope: String }

fn token(subject: &str, jti: &str, lifetime_seconds: i64) -> String {
    let claims = Claims {
        iss: ISSUER.into(), sub: subject.into(), aud: AUDIENCE.into(),
        exp: (Utc::now() + Duration::seconds(lifetime_seconds)).timestamp() as usize,
        nbf: (Utc::now() - Duration::seconds(1)).timestamp() as usize,
        jti: jti.into(), scope: "team_room:connect".into(),
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    encode(&header, &claims, &EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).unwrap()).unwrap()
}

async fn setup() -> (PgPool, OidcConfig, Uuid, Uuid, Uuid, String, tokio::task::JoinHandle<()>) {
    let pool = connect_pool(&std::env::var("DATABASE_URL").expect("PostgreSQL CI required")).await.unwrap();
    migrate(&pool).await.unwrap();
    let tenant = Uuid::new_v4();
    let room = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let participant = Uuid::new_v4();
    let subject = format!("registered-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO tenants (tenant_id,name) VALUES ($1,'auth-test')").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms (room_id,tenant_id,name) VALUES ($1,$2,'auth-room')").bind(room).bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO principals (principal_id,tenant_id,principal_kind,provider,external_subject) VALUES ($1,$2,'AI',$3,$4)")
        .bind(principal).bind(tenant).bind(ISSUER).bind(&subject).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO participants (participant_id,tenant_id,principal_id,display_name) VALUES ($1,$2,$3,'declarative display name')")
        .bind(participant).bind(tenant).bind(principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO principal_permissions (permission_id,tenant_id,principal_id,room_id,permission) VALUES ($1,$2,$3,$4,'room.join'),($5,$2,$3,$4,'room.read')")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).bind(Uuid::new_v4()).execute(&pool).await.unwrap();

    let jwks = json!({"keys":[{"kty":"RSA","kid":"test-key","use":"sig","alg":"RS256","n":N,"e":"AQAB"}]});
    let app = Router::new().route("/jwks", get(move || {
        let value = jwks.clone();
        async move { Json(value) }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let config = OidcConfig { issuer: ISSUER.into(), audience: AUDIENCE.into(), jwks_url: format!("http://{address}/jwks"), require_discovery: false, expected_jwks_url: None };
    (pool, config, tenant, room, principal, subject, server)
}

#[tokio::test]
async fn signed_identity_to_session_authorization_admission_replay_and_revocation() {
    let (pool, config, tenant, room, principal, subject, server) = setup().await;
    let external = token(&subject, &Uuid::new_v4().to_string(), 300);
    let exchanged = exchange_workload_token(&pool, &config, room, &external).await.unwrap();
    assert_eq!(exchanged.identity.principal_id, principal);
    assert_ne!(exchanged.identity.participant_id, principal);
    assert_ne!(exchanged.identity.session_id, exchanged.identity.execution_id);

    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", format!("Bearer {}", exchanged.access_token).parse().unwrap());
    let identity = authenticate_session(&pool, &headers).await.unwrap();
    assert_eq!(identity.session_id, exchanged.identity.session_id);
    require_permission(&pool, &identity, "room.join").await.unwrap();
    require_permission(&pool, &identity, "room.read").await.unwrap();

    let admission = admit_authenticated_participant(&pool, room, identity.participant_id, identity.principal_id, identity.session_id).await.unwrap();
    assert_eq!(admission.active_ai_count, 1);

    assert!(matches!(exchange_workload_token(&pool, &config, room, &external).await, Err(AuthError::Unauthorized)));
    close_session(&pool, &identity).await.unwrap();
    assert!(matches!(authenticate_session(&pool, &headers).await, Err(AuthError::Unauthorized)));

    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1").bind(tenant).execute(&pool).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn simultaneous_replay_exchange_has_exactly_one_winner_and_permission_is_separate() {
    let (pool, config, tenant, room, principal, subject, server) = setup().await;
    let external = token(&subject, &Uuid::new_v4().to_string(), 300);
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..2 {
        let p = pool.clone();
        let c = config.clone();
        let t = external.clone();
        jobs.spawn(async move { exchange_workload_token(&p, &c, room, &t).await });
    }
    let mut success = 0;
    let mut rejected = 0;
    while let Some(result) = jobs.join_next().await {
        match result.unwrap() {
            Ok(_) => success += 1,
            Err(AuthError::Unauthorized) => rejected += 1,
            Err(other) => panic!("unexpected exchange result: {other}"),
        }
    }
    assert_eq!((success, rejected), (1, 1));

    // A registered but ungranted principal authenticates nowhere: no permission,
    // no local session, and no admission.
    let no_grant_principal = Uuid::new_v4();
    let no_grant_subject = format!("ungranted-{}", Uuid::new_v4());
    let no_grant_participant = Uuid::new_v4();
    sqlx::query("INSERT INTO principals (principal_id,tenant_id,principal_kind,provider,external_subject) VALUES ($1,$2,'AI',$3,$4)")
        .bind(no_grant_principal).bind(tenant).bind(ISSUER).bind(&no_grant_subject).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO participants (participant_id,tenant_id,principal_id,display_name) VALUES ($1,$2,$3,'ungranted')")
        .bind(no_grant_participant).bind(tenant).bind(no_grant_principal).execute(&pool).await.unwrap();
    let denied = token(&no_grant_subject, &Uuid::new_v4().to_string(), 300);
    assert!(matches!(exchange_workload_token(&pool, &config, room, &denied).await, Err(AuthError::Forbidden)));

    // Disabled identity is rejected even when the external token is cryptographically valid.
    sqlx::query("UPDATE principals SET enabled = FALSE WHERE principal_id = $1").bind(principal).execute(&pool).await.unwrap();
    let disabled = token(&subject, &Uuid::new_v4().to_string(), 300);
    assert!(matches!(exchange_workload_token(&pool, &config, room, &disabled).await, Err(AuthError::IdentityUnavailable)));

    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1").bind(tenant).execute(&pool).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn expired_assertion_is_rejected_before_session_creation() {
    let (pool, config, tenant, room, _principal, subject, server) = setup().await;
    let external = token(&subject, &Uuid::new_v4().to_string(), -30);
    assert!(matches!(exchange_workload_token(&pool, &config, room, &external).await, Err(AuthError::Unauthorized)));
    let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE room_id = $1").bind(room).fetch_one(&pool).await.unwrap();
    assert_eq!(sessions, 0);
    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1").bind(tenant).execute(&pool).await.unwrap();
    server.abort();
}


#[tokio::test]
async fn local_session_expiry_rotation_and_unknown_subject_are_enforced() {
    let (pool, config, tenant, room, _principal, subject, server) = setup().await;
    let first_assertion = token(&subject, &Uuid::new_v4().to_string(), 300);
    let first = exchange_workload_token(&pool, &config, room, &first_assertion).await.unwrap();

    // A new upstream assertion rotates the existing room session and execution.
    let second_assertion = token(&subject, &Uuid::new_v4().to_string(), 300);
    let second = exchange_workload_token(&pool, &config, room, &second_assertion).await.unwrap();
    let previous_status: String = sqlx::query_scalar("SELECT status FROM sessions WHERE session_id = $1")
        .bind(first.identity.session_id).fetch_one(&pool).await.unwrap();
    assert_eq!(previous_status, "REVOKED");

    let mut old_headers = axum::http::HeaderMap::new();
    old_headers.insert("authorization", format!("Bearer {}", first.access_token).parse().unwrap());
    assert!(matches!(authenticate_session(&pool, &old_headers).await, Err(AuthError::Unauthorized)));

    let mut new_headers = axum::http::HeaderMap::new();
    new_headers.insert("authorization", format!("Bearer {}", second.access_token).parse().unwrap());
    assert!(authenticate_session(&pool, &new_headers).await.is_ok());

    sqlx::query("UPDATE sessions SET issued_at = NOW() - INTERVAL '2 seconds', expires_at = NOW() - INTERVAL '1 second' WHERE session_id = $1")
        .bind(second.identity.session_id).execute(&pool).await.unwrap();
    assert!(matches!(authenticate_session(&pool, &new_headers).await, Err(AuthError::Unauthorized)));
    let expired_status: String = sqlx::query_scalar("SELECT status FROM sessions WHERE session_id = $1")
        .bind(second.identity.session_id).fetch_one(&pool).await.unwrap();
    assert_eq!(expired_status, "EXPIRED");

    let unknown_subject = token("not-registered", &Uuid::new_v4().to_string(), 300);
    assert!(matches!(
        exchange_workload_token(&pool, &config, room, &unknown_subject).await,
        Err(AuthError::IdentityUnavailable)
    ));

    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1").bind(tenant).execute(&pool).await.unwrap();
    server.abort();
}
