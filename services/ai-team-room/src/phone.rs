use ai_team_room_service::auth::{authenticate_session, AuthenticatedIdentity, AuthError};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
enum PhoneError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("conflict")]
    Conflict,
    #[error("fenced")]
    Fenced,
    #[error("invalid request")]
    Invalid,
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Serialize)]
struct PhoneIdentity {
    ai_id: Uuid,
    display_key: String,
    ai_phone_id: Uuid,
    phone_status: String,
    binding_generation: i64,
    version: i64,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct Endpoint {
    endpoint_id: Uuid,
    ai_phone_id: Uuid,
    endpoint_key: Uuid,
    endpoint_kind: String,
    status: String,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct Attachment {
    attachment_id: Uuid,
    ai_phone_id: Uuid,
    ai_id: Uuid,
    endpoint_id: Uuid,
    session_id: Uuid,
    execution_id: Uuid,
    fencing_generation: i64,
    status: String,
    lease_expires_at: DateTime<Utc>,
    attached_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    display_key: String,
}

#[derive(Debug, Deserialize)]
struct RegisterEndpointRequest {
    endpoint_key: Uuid,
    endpoint_kind: String,
}

#[derive(Debug, Deserialize)]
struct AttachRequest {
    endpoint_id: Uuid,
    idempotency_key: Uuid,
    #[serde(default)]
    lease_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DetachRequest {
    attachment_id: Uuid,
    fencing_generation: i64,
}

#[derive(Debug, Deserialize)]
struct TransferRequest {
    target_endpoint_id: Uuid,
    expected_generation: i64,
    idempotency_key: Uuid,
    #[serde(default)]
    lease_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    attachment_id: Uuid,
    fencing_generation: i64,
    #[serde(default)]
    lease_seconds: Option<i64>,
}

fn status(error: PhoneError) -> StatusCode {
    match error {
        PhoneError::Unauthorized => StatusCode::UNAUTHORIZED,
        PhoneError::Forbidden => StatusCode::FORBIDDEN,
        PhoneError::NotFound => StatusCode::NOT_FOUND,
        PhoneError::Conflict => StatusCode::CONFLICT,
        PhoneError::Fenced => StatusCode::PRECONDITION_FAILED,
        PhoneError::Invalid => StatusCode::BAD_REQUEST,
        PhoneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn auth_status(error: AuthError) -> StatusCode {
    match error {
        AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
        crate::auth::AuthError::Forbidden | crate::auth::AuthError::IdentityUnavailable => StatusCode::FORBIDDEN,
        crate::auth::AuthError::RoomUnavailable => StatusCode::NOT_FOUND,
        crate::auth::AuthError::ProviderUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        crate::auth::AuthError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn lease_seconds(value: Option<i64>) -> Result<i64, PhoneError> {
    let seconds = value.unwrap_or(60);
    if !(15..=300).contains(&seconds) {
        return Err(PhoneError::Invalid);
    }
    Ok(seconds)
}

async fn validate_live_identity(
    tx: &mut Transaction<'_, Postgres>,
    identity: &AuthenticatedIdentity,
) -> Result<(), PhoneError> {
    let valid: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
          SELECT 1
          FROM principals p
          JOIN sessions s ON s.tenant_id=p.tenant_id AND s.principal_id=p.principal_id
          JOIN workload_executions w ON w.tenant_id=s.tenant_id AND w.room_id=s.room_id
            AND w.principal_id=s.principal_id AND w.session_id=s.session_id
          JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id
          JOIN participants pa ON pa.tenant_id=p.tenant_id AND pa.principal_id=p.principal_id
          JOIN room_memberships rm ON rm.tenant_id=p.tenant_id AND rm.room_id=s.room_id
            AND rm.principal_id=p.principal_id AND rm.participant_id=pa.participant_id
            AND rm.session_id=s.session_id
          WHERE p.tenant_id=$1 AND p.principal_id=$2 AND p.principal_kind='AI' AND p.enabled=TRUE
            AND s.session_id=$3 AND s.room_id=$4 AND s.status='ACTIVE'
            AND s.revoked_at IS NULL AND s.expires_at>NOW()
            AND w.execution_id=$5 AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW()
            AND wi.status='ACTIVE' AND wi.revoked_at IS NULL
            AND pa.participant_id=$6 AND pa.enabled=TRUE AND rm.status='ACTIVE'
        )"#,
    )
    .bind(identity.tenant_id)
    .bind(identity.principal_id)
    .bind(identity.session_id)
    .bind(identity.room_id)
    .bind(identity.execution_id)
    .bind(identity.participant_id)
    .fetch_one(&mut **tx)
    .await?;
    if valid { Ok(()) } else { Err(PhoneError::Unauthorized) }
}

async fn phone_for_identity(
    tx: &mut Transaction<'_, Postgres>,
    identity: &AuthenticatedIdentity,
) -> Result<PhoneIdentity, PhoneError> {
    sqlx::query_as::<_, (Uuid, String, Uuid, String, i64, i64, DateTime<Utc>)>(
        r#"SELECT ai.ai_id, ai.display_key, ph.ai_phone_id, ph.phone_status,
                  ph.binding_generation, ph.version, ph.created_at
           FROM ai_principal_bindings b
           JOIN ai_identities ai ON ai.ai_id=b.ai_id AND ai.status='ACTIVE'
           JOIN ai_phones ph ON ph.ai_id=ai.ai_id
           WHERE b.tenant_id=$1 AND b.principal_id=$2 AND b.status='ACTIVE'
             AND ph.phone_status='ACTIVE'"#,
    )
    .bind(identity.tenant_id)
    .bind(identity.principal_id)
    .fetch_optional(&mut **tx)
    .await?
    .map(|r| PhoneIdentity {
        ai_id: r.0, display_key: r.1, ai_phone_id: r.2, phone_status: r.3,
        binding_generation: r.4, version: r.5, created_at: r.6,
    })
    .ok_or(PhoneError::NotFound)
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    phone: &PhoneIdentity,
    identity: &AuthenticatedIdentity,
    endpoint_id: Option<Uuid>,
    attachment_id: Option<Uuid>,
    action: &str,
    outcome: &str,
    generation: Option<i64>,
    reason: &str,
) -> Result<(), PhoneError> {
    sqlx::query(
        "INSERT INTO ai_phone_audit_events(event_id,ai_phone_id,ai_id,endpoint_id,attachment_id,actor_tenant_id,actor_principal_id,action,outcome,fencing_generation,reason_code) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(Uuid::new_v4()).bind(phone.ai_phone_id).bind(phone.ai_id)
    .bind(endpoint_id).bind(attachment_id).bind(identity.tenant_id)
    .bind(identity.principal_id).bind(action).bind(outcome).bind(generation).bind(reason)
    .execute(&mut **tx).await?;
    Ok(())
}

pub async fn register(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    display_key: &str,
) -> Result<PhoneIdentity, PhoneError> {
    let key = display_key.trim();
    if key.is_empty() || key.len() > 64 { return Err(PhoneError::Invalid); }
    let mut tx = pool.begin().await?;
    validate_live_identity(&mut tx, identity).await?;

    // Serialize first-time provisioning for this tenant principal. This is
    // independent from task row locks and cannot affect task fencing.
    let lock_key = format!("{}:{}", identity.tenant_id, identity.principal_id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key).execute(&mut *tx).await?;

    let existing = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT ai.ai_id,ai.display_key FROM ai_principal_bindings b JOIN ai_identities ai ON ai.ai_id=b.ai_id WHERE b.tenant_id=$1 AND b.principal_id=$2 AND b.status='ACTIVE' FOR UPDATE OF b,ai",
    )
    .bind(identity.tenant_id).bind(identity.principal_id)
    .fetch_optional(&mut *tx).await?;

    let phone = if let Some((ai_id, existing_key)) = existing {
        if existing_key != key { return Err(PhoneError::Conflict); }
        sqlx::query_as::<_, (Uuid, String, Uuid, String, i64, i64, DateTime<Utc>)>(
            "SELECT ai.ai_id,ai.display_key,ph.ai_phone_id,ph.phone_status,ph.binding_generation,ph.version,ph.created_at FROM ai_identities ai JOIN ai_phones ph ON ph.ai_id=ai.ai_id WHERE ai.ai_id=$1"
        ).bind(ai_id).fetch_one(&mut *tx).await?
    } else {
        let ai_id = Uuid::new_v4();
        let ai_phone_id = Uuid::new_v4();
        sqlx::query("INSERT INTO ai_identities(ai_id,display_key) VALUES($1,$2)")
            .bind(ai_id).bind(key).execute(&mut *tx).await.map_err(|e| {
                if let sqlx::Error::Database(db) = &e {
                    if db.code().as_deref() == Some("23505") { return PhoneError::Conflict; }
                }
                PhoneError::Database(e)
            })?;
        sqlx::query("INSERT INTO ai_principal_bindings(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
            .bind(ai_id).bind(identity.tenant_id).bind(identity.principal_id)
            .execute(&mut *tx).await.map_err(|e| {
                if let sqlx::Error::Database(db) = &e {
                    if db.code().as_deref() == Some("23505") { return PhoneError::Conflict; }
                }
                PhoneError::Database(e)
            })?;
        sqlx::query("INSERT INTO ai_phones(ai_phone_id,ai_id) VALUES($1,$2)")
            .bind(ai_phone_id).bind(ai_id).execute(&mut *tx).await?;
        sqlx::query_as::<_, (Uuid, String, Uuid, String, i64, i64, DateTime<Utc>)>(
            "SELECT ai.ai_id,ai.display_key,ph.ai_phone_id,ph.phone_status,ph.binding_generation,ph.version,ph.created_at FROM ai_identities ai JOIN ai_phones ph ON ph.ai_id=ai.ai_id WHERE ai.ai_id=$1"
        ).bind(ai_id).fetch_one(&mut *tx).await?
    };

    let result = PhoneIdentity {
        ai_id: phone.0, display_key: phone.1, ai_phone_id: phone.2,
        phone_status: phone.3, binding_generation: phone.4, version: phone.5,
        created_at: phone.6,
    };
    audit(&mut tx, &result, identity, None, None, "REGISTERED", "SUCCESS",
        Some(result.binding_generation), "identity_phone_provisioned").await?;
    tx.commit().await?;
    Ok(result)
}

pub async fn register_endpoint(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    endpoint_key: Uuid,
    endpoint_kind: &str,
) -> Result<Endpoint, PhoneError> {
    let kind = endpoint_kind.to_ascii_uppercase();
    if !["MOBILE", "DESKTOP", "WORKER", "BROWSER"].contains(&kind.as_str()) {
        return Err(PhoneError::Invalid);
    }
    let mut tx = pool.begin().await?;
    validate_live_identity(&mut tx, identity).await?;
    let phone = phone_for_identity(&mut tx, identity).await?;
    let row = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, String, Option<DateTime<Utc>>, DateTime<Utc>, Option<DateTime<Utc>>)>(
        r#"INSERT INTO ai_phone_endpoints(endpoint_id,ai_phone_id,endpoint_key,endpoint_kind,registered_by_tenant_id,registered_by_principal_id)
           VALUES($1,$2,$3,$4,$5,$6)
           ON CONFLICT(ai_phone_id,endpoint_key) DO UPDATE
             SET last_seen_at=ai_phone_endpoints.last_seen_at
             WHERE ai_phone_endpoints.endpoint_kind=EXCLUDED.endpoint_kind
               AND ai_phone_endpoints.status NOT IN ('REVOKED','CLOSED')
           RETURNING endpoint_id,ai_phone_id,endpoint_key,endpoint_kind,status,last_seen_at,created_at,closed_at"#,
    )
    .bind(Uuid::new_v4()).bind(phone.ai_phone_id).bind(endpoint_key).bind(&kind)
    .bind(identity.tenant_id).bind(identity.principal_id)
    .fetch_optional(&mut *tx).await?.ok_or(PhoneError::Conflict)?;
    let endpoint = Endpoint {
        endpoint_id: row.0, ai_phone_id: row.1, endpoint_key: row.2, endpoint_kind: row.3,
        status: row.4, last_seen_at: row.5, created_at: row.6, closed_at: row.7,
    };
    audit(&mut tx, &phone, identity, Some(endpoint.endpoint_id), None,
        "ENDPOINT_REGISTERED", "SUCCESS", Some(phone.binding_generation), "endpoint_registered").await?;
    tx.commit().await?;
    Ok(endpoint)
}

async fn endpoint_for_phone(
    tx: &mut Transaction<'_, Postgres>,
    phone_id: Uuid,
    endpoint_id: Uuid,
) -> Result<(), PhoneError> {
    let found: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ai_phone_endpoints WHERE ai_phone_id=$1 AND endpoint_id=$2 AND status IN ('REGISTERED','ACTIVE'))"
    ).bind(phone_id).bind(endpoint_id).fetch_one(&mut **tx).await?;
    if found { Ok(()) } else { Err(PhoneError::NotFound) }
}

async fn insert_attachment(
    tx: &mut Transaction<'_, Postgres>,
    phone: &PhoneIdentity,
    identity: &AuthenticatedIdentity,
    endpoint_id: Uuid,
    idempotency_key: Uuid,
    seconds: i64,
) -> Result<Attachment, PhoneError> {
    let generation: i64 = sqlx::query_scalar(
        "UPDATE ai_phones SET binding_generation=binding_generation+1,version=version+1,updated_at=NOW() WHERE ai_phone_id=$1 AND phone_status='ACTIVE' RETURNING binding_generation"
    ).bind(phone.ai_phone_id).fetch_optional(&mut **tx).await?.ok_or(PhoneError::NotFound)?;
    let attachment_id = Uuid::new_v4();
    let row = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, i64, String, DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)>(
        r#"INSERT INTO ai_phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,principal_id,participant_id,session_id,execution_id,fencing_generation,idempotency_key,lease_expires_at)
           VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,NOW()+($12::bigint * INTERVAL '1 second'))
           RETURNING attachment_id,ai_phone_id,ai_id,endpoint_id,session_id,execution_id,fencing_generation,status,lease_expires_at,attached_at,last_seen_at"#,
    )
    .bind(attachment_id).bind(phone.ai_phone_id).bind(phone.ai_id).bind(endpoint_id)
    .bind(identity.tenant_id).bind(identity.principal_id).bind(identity.participant_id)
    .bind(identity.session_id).bind(identity.execution_id).bind(generation).bind(idempotency_key)
    .bind(seconds).fetch_one(&mut **tx).await?;
    sqlx::query("UPDATE ai_phone_endpoints SET status='ACTIVE',last_session_id=$1,last_execution_id=$2,last_seen_at=NOW() WHERE endpoint_id=$3")
        .bind(identity.session_id).bind(identity.execution_id).bind(endpoint_id).execute(&mut **tx).await?;
    Ok(Attachment {
        attachment_id: row.0, ai_phone_id: row.1, ai_id: row.2, endpoint_id: row.3,
        session_id: row.4, execution_id: row.5, fencing_generation: row.6,
        status: row.7, lease_expires_at: row.8, attached_at: row.9, last_seen_at: row.10,
    })
}

async fn current_attachment_is_live(
    tx: &mut Transaction<'_, Postgres>,
    attachment_id: Uuid,
) -> Result<bool, PhoneError> {
    Ok(sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM ai_phone_attachments a
           JOIN sessions s ON s.session_id=a.session_id
           JOIN workload_executions w ON w.execution_id=a.execution_id
           JOIN workload_identities wi ON wi.tenant_id=a.tenant_id AND wi.principal_id=a.principal_id
           WHERE a.attachment_id=$1 AND a.status='ACTIVE' AND a.lease_expires_at>NOW()
             AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW()
             AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW()
             AND wi.status='ACTIVE' AND wi.revoked_at IS NULL)"#
    ).bind(attachment_id).fetch_one(&mut **tx).await?)
}

pub async fn attach(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    request: AttachRequest,
) -> Result<Attachment, PhoneError> {
    let seconds = lease_seconds(request.lease_seconds)?;
    let mut tx = pool.begin().await?;
    validate_live_identity(&mut tx, identity).await?;
    let mut phone = phone_for_identity(&mut tx, identity).await?;
    sqlx::query("SELECT ai_phone_id FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE")
        .bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;
    endpoint_for_phone(&mut tx, phone.ai_phone_id, request.endpoint_id).await?;

    if let Some(row) = sqlx::query_as::<_, (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, i64, String, DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)>(
        "SELECT attachment_id,ai_phone_id,ai_id,endpoint_id,session_id,execution_id,fencing_generation,status,lease_expires_at,attached_at,last_seen_at FROM ai_phone_attachments WHERE ai_phone_id=$1 AND idempotency_key=$2"
    ).bind(phone.ai_phone_id).bind(request.idempotency_key).fetch_optional(&mut *tx).await? {
        if row.3 != request.endpoint_id || row.4 != identity.session_id || row.5 != identity.execution_id {
            return Err(PhoneError::Conflict);
        }
        if row.7 == "ACTIVE" && current_attachment_is_live(&mut tx,row.0).await? {
            tx.commit().await?;
            return Ok(Attachment { attachment_id:row.0,ai_phone_id:row.1,ai_id:row.2,endpoint_id:row.3,session_id:row.4,execution_id:row.5,fencing_generation:row.6,status:row.7,lease_expires_at:row.8,attached_at:row.9,last_seen_at:row.10 });
        }
        return Err(PhoneError::Conflict);
    }

    if let Some((old_id, old_endpoint)) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT attachment_id,endpoint_id FROM ai_phone_attachments WHERE ai_phone_id=$1 AND status='ACTIVE' FOR UPDATE"
    ).bind(phone.ai_phone_id).fetch_optional(&mut *tx).await? {
        if current_attachment_is_live(&mut tx, old_id).await? {
            return Err(PhoneError::Conflict);
        }
        sqlx::query("UPDATE ai_phone_attachments SET status='EXPIRED',ended_at=NOW() WHERE attachment_id=$1")
            .bind(old_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE ai_phone_endpoints SET status='REGISTERED' WHERE endpoint_id=$1 AND status='ACTIVE'")
            .bind(old_endpoint).execute(&mut *tx).await?;
        audit(&mut tx,&phone,identity,Some(old_endpoint),Some(old_id),"EXPIRED","SUCCESS",
            Some(phone.binding_generation),"previous_binding_expired_or_session_revoked").await?;
    }
    let attachment = insert_attachment(&mut tx,&phone,identity,request.endpoint_id,request.idempotency_key,seconds).await?;
    phone.binding_generation = attachment.fencing_generation;
    audit(&mut tx,&phone,identity,Some(request.endpoint_id),Some(attachment.attachment_id),
        "ATTACHED","SUCCESS",Some(attachment.fencing_generation),"authenticated_attach").await?;
    tx.commit().await?;
    Ok(attachment)
}

pub async fn detach(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    request: DetachRequest,
) -> Result<Attachment, PhoneError> {
    if request.fencing_generation <= 0 { return Err(PhoneError::Invalid); }
    let mut tx = pool.begin().await?;
    validate_live_identity(&mut tx,identity).await?;
    let phone = phone_for_identity(&mut tx,identity).await?;
    sqlx::query("SELECT ai_phone_id FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE")
        .bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;
    let row = sqlx::query_as::<_, (Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,i64,String,DateTime<Utc>,DateTime<Utc>,DateTime<Utc>)>(
        "SELECT attachment_id,ai_phone_id,ai_id,endpoint_id,session_id,execution_id,fencing_generation,status,lease_expires_at,attached_at,last_seen_at FROM ai_phone_attachments WHERE attachment_id=$1 AND ai_phone_id=$2 FOR UPDATE"
    ).bind(request.attachment_id).bind(phone.ai_phone_id).fetch_optional(&mut *tx).await?.ok_or(PhoneError::NotFound)?;
    if row.4 != identity.session_id || row.5 != identity.execution_id { return Err(PhoneError::Forbidden); }
    if row.6 != request.fencing_generation { return Err(PhoneError::Fenced); }
    if row.7 == "ACTIVE" {
        sqlx::query("UPDATE ai_phone_attachments SET status='DETACHED',ended_at=NOW() WHERE attachment_id=$1")
            .bind(row.0).execute(&mut *tx).await?;
        sqlx::query("UPDATE ai_phone_endpoints SET status='REGISTERED' WHERE endpoint_id=$1 AND status='ACTIVE'")
            .bind(row.3).execute(&mut *tx).await?;
    } else if row.7 != "DETACHED" {
        return Err(PhoneError::Fenced);
    }
    audit(&mut tx,&phone,identity,Some(row.3),Some(row.0),"DETACHED","SUCCESS",
        Some(row.6),"endpoint_detached").await?;
    tx.commit().await?;
    Ok(Attachment { attachment_id:row.0,ai_phone_id:row.1,ai_id:row.2,endpoint_id:row.3,
        session_id:row.4,execution_id:row.5,fencing_generation:row.6,status:"DETACHED".into(),
        lease_expires_at:row.8,attached_at:row.9,last_seen_at:row.10 })
}

pub async fn transfer(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    request: TransferRequest,
) -> Result<Attachment, PhoneError> {
    let seconds = lease_seconds(request.lease_seconds)?;
    if request.expected_generation <= 0 { return Err(PhoneError::Invalid); }
    let mut tx = pool.begin().await?;
    validate_live_identity(&mut tx,identity).await?;
    let mut phone = phone_for_identity(&mut tx,identity).await?;
    sqlx::query("SELECT ai_phone_id FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE")
        .bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;
    endpoint_for_phone(&mut tx,phone.ai_phone_id,request.target_endpoint_id).await?;
    if let Some(row) = sqlx::query_as::<_, (Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,i64,String,DateTime<Utc>,DateTime<Utc>,DateTime<Utc>)>(
        "SELECT attachment_id,ai_phone_id,ai_id,endpoint_id,session_id,execution_id,fencing_generation,status,lease_expires_at,attached_at,last_seen_at FROM ai_phone_attachments WHERE ai_phone_id=$1 AND idempotency_key=$2"
    ).bind(phone.ai_phone_id).bind(request.idempotency_key).fetch_optional(&mut *tx).await? {
        if row.3 == request.target_endpoint_id && row.4 == identity.session_id && row.5 == identity.execution_id && row.7 == "ACTIVE" {
            tx.commit().await?;
            return Ok(Attachment { attachment_id:row.0,ai_phone_id:row.1,ai_id:row.2,endpoint_id:row.3,session_id:row.4,execution_id:row.5,fencing_generation:row.6,status:row.7,lease_expires_at:row.8,attached_at:row.9,last_seen_at:row.10 });
        }
        return Err(PhoneError::Conflict);
    }
    let (old_id,old_endpoint,old_generation) = sqlx::query_as::<_,(Uuid,Uuid,i64)>(
        "SELECT attachment_id,endpoint_id,fencing_generation FROM ai_phone_attachments WHERE ai_phone_id=$1 AND status='ACTIVE' FOR UPDATE"
    ).bind(phone.ai_phone_id).fetch_optional(&mut *tx).await?.ok_or(PhoneError::NotFound)?;
    if old_generation != request.expected_generation { return Err(PhoneError::Fenced); }
    sqlx::query("UPDATE ai_phone_attachments SET status='TRANSFERRED',ended_at=NOW() WHERE attachment_id=$1")
        .bind(old_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_phone_endpoints SET status='REGISTERED' WHERE endpoint_id=$1 AND status='ACTIVE'")
        .bind(old_endpoint).execute(&mut *tx).await?;
    let attachment=insert_attachment(&mut tx,&phone,identity,request.target_endpoint_id,request.idempotency_key,seconds).await?;
    phone.binding_generation=attachment.fencing_generation;
    audit(&mut tx,&phone,identity,Some(old_endpoint),Some(old_id),"TRANSFERRED","SUCCESS",
        Some(old_generation),"explicit_owner_transfer").await?;
    audit(&mut tx,&phone,identity,Some(request.target_endpoint_id),Some(attachment.attachment_id),
        "ATTACHED","SUCCESS",Some(attachment.fencing_generation),"transfer_target_attached").await?;
    tx.commit().await?;
    Ok(attachment)
}

pub async fn heartbeat(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    request: HeartbeatRequest,
) -> Result<Attachment, PhoneError> {
    let seconds=lease_seconds(request.lease_seconds)?;
    let mut tx=pool.begin().await?;
    validate_live_identity(&mut tx,identity).await?;
    let phone=phone_for_identity(&mut tx,identity).await?;
    sqlx::query("SELECT ai_phone_id FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE")
        .bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;
    let row=sqlx::query_as::<_,(Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,i64,String,DateTime<Utc>,DateTime<Utc>,DateTime<Utc>)>(
        r#"UPDATE ai_phone_attachments SET lease_expires_at=NOW()+($1::bigint*INTERVAL '1 second'),last_seen_at=NOW()
           WHERE attachment_id=$2 AND ai_phone_id=$3 AND session_id=$4 AND execution_id=$5
             AND fencing_generation=$6 AND status='ACTIVE' AND lease_expires_at>NOW()
           RETURNING attachment_id,ai_phone_id,ai_id,endpoint_id,session_id,execution_id,fencing_generation,status,lease_expires_at,attached_at,last_seen_at"#
    ).bind(seconds).bind(request.attachment_id).bind(phone.ai_phone_id).bind(identity.session_id)
     .bind(identity.execution_id).bind(request.fencing_generation).fetch_optional(&mut *tx).await?
     .ok_or(PhoneError::Fenced)?;
    sqlx::query("UPDATE ai_phone_endpoints SET last_seen_at=NOW(),last_session_id=$1,last_execution_id=$2 WHERE endpoint_id=$3")
        .bind(identity.session_id).bind(identity.execution_id).bind(row.3).execute(&mut *tx).await?;
    audit(&mut tx,&phone,identity,Some(row.3),Some(row.0),"HEARTBEAT","SUCCESS",
        Some(row.6),"lease_renewed").await?;
    tx.commit().await?;
    Ok(Attachment { attachment_id:row.0,ai_phone_id:row.1,ai_id:row.2,endpoint_id:row.3,
        session_id:row.4,execution_id:row.5,fencing_generation:row.6,status:row.7,
        lease_expires_at:row.8,attached_at:row.9,last_seen_at:row.10 })
}

async fn me(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    let mut tx=pool.begin().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    validate_live_identity(&mut tx,&identity).await.map_err(status)?;
    let phone=phone_for_identity(&mut tx,&identity).await.map_err(status)?;
    let endpoints=sqlx::query_as::<_,(Uuid,Uuid,Uuid,String,String,Option<DateTime<Utc>>,DateTime<Utc>,Option<DateTime<Utc>>)>(
        "SELECT endpoint_id,ai_phone_id,endpoint_key,endpoint_kind,status,last_seen_at,created_at,closed_at FROM ai_phone_endpoints WHERE ai_phone_id=$1 ORDER BY created_at"
    ).bind(phone.ai_phone_id).fetch_all(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let active=sqlx::query_as::<_,(Uuid,Uuid,Uuid,Uuid,i64,String,DateTime<Utc>)>(
        "SELECT a.attachment_id,a.endpoint_id,a.session_id,a.execution_id,a.fencing_generation,a.status,a.lease_expires_at FROM ai_phone_attachments a JOIN sessions s ON s.session_id=a.session_id JOIN workload_executions w ON w.execution_id=a.execution_id JOIN workload_identities wi ON wi.tenant_id=a.tenant_id AND wi.principal_id=a.principal_id WHERE a.ai_phone_id=$1 AND a.status='ACTIVE' AND a.lease_expires_at>NOW() AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND wi.status='ACTIVE' AND wi.revoked_at IS NULL"
    ).bind(phone.ai_phone_id).fetch_optional(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "ai_id":phone.ai_id,"display_key":phone.display_key,"ai_phone_id":phone.ai_phone_id,
        "phone_status":phone.phone_status,"binding_generation":phone.binding_generation,
        "version":phone.version,"created_at":phone.created_at,
        "connectivity":if active.is_some() {"CONNECTED"} else {"DISCONNECTED"},
        "current_attachment":active.map(|a|serde_json::json!({
            "attachment_id":a.0,"endpoint_id":a.1,"session_id":a.2,"execution_id":a.3,
            "fencing_generation":a.4,"status":a.5,"lease_expires_at":a.6
        })),
        "endpoints":endpoints.into_iter().map(|e|serde_json::json!({
            "endpoint_id":e.0,"ai_phone_id":e.1,"endpoint_key":e.2,"endpoint_kind":e.3,
            "status":e.4,"last_seen_at":e.5,"created_at":e.6,"closed_at":e.7
        })).collect::<Vec<_>>()
    })))
}

async fn register_http(
    State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<RegisterRequest>,
) -> Result<(StatusCode,Json<PhoneIdentity>),StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    register(&pool,&identity,&request.display_key).await.map(|p|(StatusCode::CREATED,Json(p))).map_err(status)
}
async fn endpoint_http(
    State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<RegisterEndpointRequest>,
) -> Result<(StatusCode,Json<Endpoint>),StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    register_endpoint(&pool,&identity,request.endpoint_key,&request.endpoint_kind).await
        .map(|e|(StatusCode::CREATED,Json(e))).map_err(status)
}
async fn attach_http(
    State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<AttachRequest>,
) -> Result<Json<Attachment>,StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    attach(&pool,&identity,request).await.map(Json).map_err(status)
}
async fn detach_http(
    State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<DetachRequest>,
) -> Result<Json<Attachment>,StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    detach(&pool,&identity,request).await.map(Json).map_err(status)
}
async fn transfer_http(
    State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<TransferRequest>,
) -> Result<Json<Attachment>,StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    transfer(&pool,&identity,request).await.map(Json).map_err(status)
}
async fn heartbeat_http(
    State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<HeartbeatRequest>,
) -> Result<Json<Attachment>,StatusCode> {
    let identity=authenticate_session(&pool,&headers).await.map_err(auth_status)?;
    heartbeat(&pool,&identity,request).await.map(Json).map_err(status)
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/ai-phone/me",get(me))
        .route("/ai-phone/register",post(register_http))
        .route("/ai-phone/endpoints",post(endpoint_http))
        .route("/ai-phone/attach",post(attach_http))
        .route("/ai-phone/detach",post(detach_http))
        .route("/ai-phone/transfer",post(transfer_http))
        .route("/ai-phone/heartbeat",post(heartbeat_http))
        .with_state(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{connect_pool,migrate};
    use sha2::{Digest,Sha256};

    async fn fixture(pool:&PgPool)->(AuthenticatedIdentity,String,Uuid) {
        let tenant=Uuid::new_v4(); let room=Uuid::new_v4(); let principal=Uuid::new_v4();
        let participant=Uuid::new_v4(); let workload=Uuid::new_v4(); let session=Uuid::new_v4();
        let execution=Uuid::new_v4(); let token=format!("phone-test-{}",Uuid::new_v4());
        let issuer="https://ai-phone-test.invalid"; let subject=Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'ai-phone-test')").bind(tenant).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'ai-phone-room')").bind(room).bind(tenant).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI',$3,$4)").bind(principal).bind(tenant).bind(issuer).bind(&subject).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,'AI-01')").bind(participant).bind(tenant).bind(principal).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,$4,$5,'ACTIVE')").bind(workload).bind(tenant).bind(principal).bind(issuer).bind(&subject).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE',$5)")
            .bind(session).bind(tenant).bind(room).bind(principal).bind(Sha256::digest(token.as_bytes()).to_vec()).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE')")
            .bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).bind(format!("{issuer}|{subject}")).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
            .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).execute(pool).await.unwrap();
        (AuthenticatedIdentity{tenant_id:tenant,participant_id:participant,principal_id:principal,session_id:session,execution_id:execution,room_id:room,issuer:issuer.into(),subject},token,tenant)
    }

    async fn new_session(pool:&PgPool, old:&AuthenticatedIdentity)->AuthenticatedIdentity {
        let session=Uuid::new_v4(); let execution=Uuid::new_v4();
        sqlx::query("UPDATE sessions SET status='REVOKED',revoked_at=NOW() WHERE session_id=$1").bind(old.session_id).execute(pool).await.unwrap();
        sqlx::query("UPDATE workload_executions SET status='REVOKED',revoked_at=NOW() WHERE execution_id=$1").bind(old.execution_id).execute(pool).await.unwrap();
        sqlx::query("UPDATE room_memberships SET status='LEFT' WHERE session_id=$1").bind(old.session_id).execute(pool).await.unwrap();
        let digest=Sha256::digest(format!("session-b-{session}").as_bytes()).to_vec();
        sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE',$5)")
            .bind(session).bind(old.tenant_id).bind(old.room_id).bind(old.principal_id).bind(digest).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,'phone-test-b',NOW(),NOW()+INTERVAL '10 minutes','ACTIVE')")
            .bind(execution).bind(old.tenant_id).bind(old.room_id).bind(old.participant_id).bind(old.principal_id).bind(session).execute(pool).await.unwrap();
        sqlx::query("UPDATE room_memberships SET session_id=$1,status='ACTIVE',joined_at=NOW(),last_seen_at=NOW(),left_at=NULL WHERE room_id=$2 AND principal_id=$3")
            .bind(session).bind(old.room_id).bind(old.principal_id).execute(pool).await.unwrap();
        AuthenticatedIdentity{session_id:session,execution_id:execution,..old.clone()}
    }

    async fn db()->PgPool {
        let url=std::env::var("DATABASE_URL").expect("PostgreSQL integration tests require DATABASE_URL");
        let pool=connect_pool(&url).await.unwrap(); migrate(&pool).await.unwrap(); pool
    }

    #[tokio::test]
    async fn ai_phone_lifecycle_covers_identity_endpoint_reconnect_fencing_takeover_and_expiry() {
        let pool=db().await;
        let (identity,_token,tenant)=fixture(&pool).await;

        // 1. Permanent AI_ID creation and idempotent registration.
        let phone=register(&pool,&identity,"AI-01").await.unwrap();
        let same=register(&pool,&identity,"AI-01").await.unwrap();
        assert_eq!(phone.ai_id,same.ai_id);

        // 2. Permanent AI_PHONE_ID creation.
        assert_ne!(phone.ai_phone_id,Uuid::nil());
        assert_eq!(phone.ai_phone_id,same.ai_phone_id);

        // 3. Endpoint A registration.
        let endpoint_a=register_endpoint(&pool,&identity,Uuid::new_v4(),"WORKER").await.unwrap();

        // 4. Attach Session A -> Phone X.
        let attach_a_key=Uuid::new_v4();
        let attach_a=attach(&pool,&identity,AttachRequest{endpoint_id:endpoint_a.endpoint_id,idempotency_key:attach_a_key,lease_seconds:Some(60)}).await.unwrap();
        assert_eq!(attach_a.ai_phone_id,phone.ai_phone_id);

        // 5. Same idempotency key returns the same attachment/generation.
        let endpoint_idempotent=register_endpoint(&pool,&identity,endpoint_a.endpoint_key,"WORKER").await.unwrap();
        assert_eq!(endpoint_idempotent.endpoint_id,endpoint_a.endpoint_id);
        let idem=attach(&pool,&identity,AttachRequest{endpoint_id:endpoint_a.endpoint_id,idempotency_key:attach_a_key,lease_seconds:Some(60)}).await.unwrap();
        assert_eq!(idem.attachment_id,attach_a.attachment_id);
        assert_eq!(idem.fencing_generation,attach_a.fencing_generation);

        // 6. Live competing endpoint cannot take over silently.
        let endpoint_b=register_endpoint(&pool,&identity,Uuid::new_v4(),"MOBILE").await.unwrap();
        let takeover=attach(&pool,&identity,AttachRequest{endpoint_id:endpoint_b.endpoint_id,idempotency_key:Uuid::new_v4(),lease_seconds:Some(60)}).await;
        assert!(matches!(takeover,Err(PhoneError::Conflict)));

        // 7. Explicit transfer fences endpoint A and activates endpoint B.
        let transfer=transfer(&pool,&identity,TransferRequest{target_endpoint_id:endpoint_b.endpoint_id,expected_generation:attach_a.fencing_generation,idempotency_key:Uuid::new_v4(),lease_seconds:Some(60)}).await.unwrap();
        assert!(transfer.fencing_generation>attach_a.fencing_generation);

        // 8. Endpoint A's stale generation cannot renew the phone.
        let stale=heartbeat(&pool,&identity,HeartbeatRequest{attachment_id:attach_a.attachment_id,fencing_generation:attach_a.fencing_generation,lease_seconds:Some(60)}).await;
        assert!(matches!(stale,Err(PhoneError::Fenced)));

        // 9. Endpoint B's current generation is accepted.
        let live=heartbeat(&pool,&identity,HeartbeatRequest{attachment_id:transfer.attachment_id,fencing_generation:transfer.fencing_generation,lease_seconds:Some(60)}).await.unwrap();
        assert_eq!(live.fencing_generation,transfer.fencing_generation);

        // 10. Session A -> Session B, preserving the same phone after old session revocation.
        let session_b=new_session(&pool,&identity).await;
        let reconnect=attach(&pool,&session_b,AttachRequest{endpoint_id:endpoint_a.endpoint_id,idempotency_key:Uuid::new_v4(),lease_seconds:Some(60)}).await.unwrap();
        assert_eq!(reconnect.ai_phone_id,phone.ai_phone_id);
        assert!(reconnect.fencing_generation>transfer.fencing_generation);

        // 11. Expired lease is reclaimed and fenced.
        sqlx::query("UPDATE ai_phone_attachments SET lease_expires_at=NOW()-INTERVAL '1 second' WHERE attachment_id=$1")
            .bind(reconnect.attachment_id).execute(&pool).await.unwrap();
        let reclaimed=attach(&pool,&session_b,AttachRequest{endpoint_id:endpoint_b.endpoint_id,idempotency_key:Uuid::new_v4(),lease_seconds:Some(60)}).await.unwrap();
        assert!(reclaimed.fencing_generation>reconnect.fencing_generation);

        // 12. Detach and reject stale generation after recovery.
        let detached=detach(&pool,&session_b,DetachRequest{attachment_id:reclaimed.attachment_id,fencing_generation:reclaimed.fencing_generation}).await.unwrap();
        assert_eq!(detached.status,"DETACHED");
        let stale_detach=detach(&pool,&session_b,DetachRequest{attachment_id:reconnect.attachment_id,fencing_generation:reconnect.fencing_generation}).await;
        assert!(matches!(stale_detach,Err(PhoneError::Fenced)));

        sqlx::query("DELETE FROM ai_phones WHERE ai_id IN (SELECT ai_id FROM ai_principal_bindings WHERE tenant_id=$1 AND principal_id=$2)").bind(tenant).bind(identity.principal_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_principal_bindings WHERE tenant_id=$1 AND principal_id=$2").bind(tenant).bind(identity.principal_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_identities WHERE ai_id NOT IN (SELECT ai_id FROM ai_principal_bindings)").execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_attach_allows_exactly_one_phone_owner() {
        let pool=db().await;
        let (identity,_token,tenant)=fixture(&pool).await;
        let phone=register(&pool,&identity,&format!("AI-RACE-{}",Uuid::new_v4())).await.unwrap();
        let endpoint_a=register_endpoint(&pool,&identity,Uuid::new_v4(),"WORKER").await.unwrap();
        let endpoint_b=register_endpoint(&pool,&identity,Uuid::new_v4(),"MOBILE").await.unwrap();
        let request_a=AttachRequest{endpoint_id:endpoint_a.endpoint_id,idempotency_key:Uuid::new_v4(),lease_seconds:Some(60)};
        let request_b=AttachRequest{endpoint_id:endpoint_b.endpoint_id,idempotency_key:Uuid::new_v4(),lease_seconds:Some(60)};
        let (result_a,result_b)=tokio::join!(
            attach(&pool,&identity,request_a),
            attach(&pool,&identity,request_b)
        );
        assert_eq!((result_a.is_ok() as usize)+(result_b.is_ok() as usize),1);
        let active:i64=sqlx::query_scalar("SELECT COUNT(*) FROM ai_phone_attachments WHERE ai_phone_id=$1 AND status='ACTIVE' AND lease_expires_at>NOW()")
            .bind(phone.ai_phone_id).fetch_one(&pool).await.unwrap();
        assert_eq!(active,1);
        sqlx::query("DELETE FROM ai_phones WHERE ai_id IN (SELECT ai_id FROM ai_principal_bindings WHERE tenant_id=$1 AND principal_id=$2)").bind(tenant).bind(identity.principal_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_principal_bindings WHERE tenant_id=$1 AND principal_id=$2").bind(tenant).bind(identity.principal_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_identities WHERE ai_id NOT IN (SELECT ai_id FROM ai_principal_bindings)").execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    }

}
