use ai_team_room_service::auth::{authenticate_session, require_permission, AuthenticatedIdentity};
use axum::{extract::{Path, State}, http::{HeaderMap, StatusCode}, routing::{get, post}, Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PhoneError {
    #[error("phone or AI identity not found")]
    NotFound,
    #[error("phone operation is not authorized")]
    Forbidden,
    #[error("phone endpoint is already attached or operation conflicts")]
    Conflict,
    #[error("phone attachment is stale or fenced")]
    Fenced,
    #[error("session or execution is expired or revoked")]
    InvalidSession,
    #[error("invalid phone request")]
    Invalid,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PhoneRecord {
    pub ai_phone_id: Uuid,
    pub ai_id: Uuid,
    pub phone_status: String,
    pub version: i64,
    pub endpoint_fence: i64,
    pub current_mission_id: Option<Uuid>,
    pub current_task_id: Option<Uuid>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AttachmentRecord {
    pub attachment_id: Uuid,
    pub ai_phone_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_fence: i64,
    pub attachment_status: String,
    pub attached_at: DateTime<Utc>,
    pub detached_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PhoneMessage {
    pub message_id: Uuid,
    pub ai_phone_id: Uuid,
    pub sender_principal_id: Uuid,
    pub sequence_no: i64,
    pub idempotency_key: String,
    pub message_state: String,
    pub content: Value,
    pub attempt_count: i32,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct AttachRequest {
    pub endpoint_id: Uuid,
    pub endpoint_type: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct MessageRequest {
    pub idempotency_key: String,
    pub content: Value,
}

#[derive(Debug, Deserialize)]
pub struct MissionLinkRequest {
    pub task_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct PhoneSnapshot {
    pub phone: PhoneRecord,
    pub attachment: Option<AttachmentRecord>,
    pub mission: Option<Value>,
}

async fn owned_phone(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, identity: &AuthenticatedIdentity)
    -> Result<PhoneRecord, PhoneError> {
    let ai_id: Uuid = sqlx::query_scalar(
        "SELECT ai_id FROM ai_identity_principals WHERE tenant_id=$1 AND principal_id=$2"
    ).bind(identity.tenant_id).bind(identity.principal_id)
      .fetch_optional(&mut **tx).await?.ok_or(PhoneError::NotFound)?;
    let phone = sqlx::query_as::<_, PhoneRecord>(
        "INSERT INTO ai_phones(ai_phone_id,ai_id) VALUES($1,$2) ON CONFLICT(ai_id) DO UPDATE SET updated_at=ai_phones.updated_at RETURNING ai_phone_id,ai_id,phone_status,version,endpoint_fence,current_mission_id,current_task_id,last_seen_at,created_at,updated_at"
    ).bind(Uuid::new_v4()).bind(ai_id).fetch_one(&mut **tx).await?;
    Ok(phone)
}

async fn validate_live_binding(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &AuthenticatedIdentity,
) -> Result<(), PhoneError> {
    let valid: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
            SELECT 1 FROM sessions s
            JOIN room_memberships m ON m.room_id=s.room_id AND m.session_id=s.session_id AND m.principal_id=s.principal_id
            JOIN workload_executions w ON w.tenant_id=s.tenant_id AND w.room_id=s.room_id
              AND w.participant_id=m.participant_id AND w.principal_id=s.principal_id AND w.session_id=s.session_id
            WHERE s.tenant_id=$1 AND s.room_id=$2 AND s.principal_id=$3 AND s.session_id=$4
              AND m.participant_id=$5 AND w.execution_id=$6
              AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW()
              AND m.status='ACTIVE' AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW()
        )"#
    ).bind(identity.tenant_id).bind(identity.room_id).bind(identity.principal_id)
      .bind(identity.session_id).bind(identity.participant_id).bind(identity.execution_id)
      .fetch_one(&mut **tx).await?;
    if !valid { return Err(PhoneError::InvalidSession); }
    Ok(())
}

fn valid_endpoint_type(value: &str) -> bool {
    matches!(value, "WEB" | "DESKTOP" | "MOBILE" | "WORKER" | "TEST")
}

fn contains_secret_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, child)| {
            let key = key.to_ascii_lowercase();
            ["token", "secret", "password", "api_key", "apikey", "credential", "private_key", "authorization"]
                .iter().any(|needle| key.contains(needle)) || contains_secret_key(child)
        }),
        Value::Array(items) => items.iter().any(contains_secret_key),
        _ => false,
    }
}

pub async fn get_my_phone(pool: &PgPool, identity: &AuthenticatedIdentity) -> Result<PhoneSnapshot, PhoneError> {
    require_permission(pool, identity, "phone.read").await.map_err(|_| PhoneError::Forbidden)?;
    let mut tx = pool.begin().await?;
    let phone = owned_phone(&mut tx, identity).await?;
    let attachment = sqlx::query_as::<_, AttachmentRecord>(
        "SELECT attachment_id,ai_phone_id,endpoint_id,endpoint_fence,attachment_status,attached_at,detached_at FROM phone_attachments WHERE ai_phone_id=$1 AND attachment_status='ACTIVE'"
    ).bind(phone.ai_phone_id).fetch_optional(&mut *tx).await?;
    let mission = if let Some(task_id) = phone.current_task_id {
        Some(sqlx::query_scalar::<_, Value>(
            "SELECT jsonb_build_object('task_id',task_id,'mission_id',mission_id,'status',status,'checkpoint',checkpoint,'checkpoint_version',checkpoint_version,'fencing_token',fencing_token,'lease_expires_at',lease_expires_at) FROM team_tasks WHERE task_id=$1 AND room_id=$2"
        ).bind(task_id).bind(identity.room_id).fetch_optional(&mut *tx).await?.ok_or(PhoneError::NotFound)?)
    } else { None };
    tx.commit().await?;
    Ok(PhoneSnapshot { phone, attachment, mission })
}

pub async fn attach(pool: &PgPool, identity: &AuthenticatedIdentity, request: AttachRequest, allow_transfer: bool)
    -> Result<AttachmentRecord, PhoneError> {
    require_permission(pool, identity, if allow_transfer { "phone.transfer" } else { "phone.attach" })
        .await.map_err(|_| PhoneError::Forbidden)?;
    if !valid_endpoint_type(&request.endpoint_type) || request.metadata.is_null()
        || !request.metadata.is_object() || request.metadata.to_string().len() > 4096
        || contains_secret_key(&request.metadata) { return Err(PhoneError::Invalid); }

    let mut tx = pool.begin().await?;
    validate_live_binding(&mut tx, identity).await?;
    let phone = owned_phone(&mut tx, identity).await?;
    let phone = sqlx::query_as::<_, PhoneRecord>(
        "SELECT ai_phone_id,ai_id,phone_status,version,endpoint_fence,current_mission_id,current_task_id,last_seen_at,created_at,updated_at FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE"
    ).bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;

    let current = sqlx::query_as::<_, (Uuid, Uuid, i64)>(
        "SELECT attachment_id,session_id,endpoint_fence FROM phone_attachments WHERE ai_phone_id=$1 AND attachment_status='ACTIVE' FOR UPDATE"
    ).bind(phone.ai_phone_id).fetch_optional(&mut *tx).await?;
    let had_current = current.is_some();
    if let Some((current_attachment, current_session, _)) = current {
        if current_session == identity.session_id {
            let existing = sqlx::query_as::<_, AttachmentRecord>(
                "SELECT attachment_id,ai_phone_id,endpoint_id,endpoint_fence,attachment_status,attached_at,detached_at FROM phone_attachments WHERE attachment_id=$1"
            ).bind(current_attachment).fetch_one(&mut *tx).await?;
            if existing.endpoint_id == request.endpoint_id {
                tx.commit().await?;
                return Ok(existing);
            }
            if !allow_transfer { return Err(PhoneError::Conflict); }
        } else {
            let old_session_live: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=$1 AND status='ACTIVE' AND revoked_at IS NULL AND expires_at>NOW())"
            ).bind(current_session).fetch_one(&mut *tx).await?;
            if old_session_live && !allow_transfer { return Err(PhoneError::Conflict); }
        }
        sqlx::query("UPDATE phone_attachments SET attachment_status='FENCED',detached_at=NOW() WHERE attachment_id=$1")
            .bind(current_attachment).execute(&mut *tx).await?;
        sqlx::query("UPDATE phone_endpoints SET endpoint_status='DETACHED',detached_at=NOW() WHERE endpoint_id=(SELECT endpoint_id FROM phone_attachments WHERE attachment_id=$1)")
            .bind(current_attachment).execute(&mut *tx).await?;
    }

    let endpoint_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM phone_endpoints WHERE endpoint_id=$1 AND ai_phone_id=$2 AND endpoint_status<>'REVOKED')"
    ).bind(request.endpoint_id).bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;
    if endpoint_exists {
        sqlx::query("UPDATE phone_endpoints SET endpoint_type=$1,endpoint_status='ATTACHED',metadata=$2,last_seen_at=NOW(),detached_at=NULL WHERE endpoint_id=$3 AND ai_phone_id=$4")
            .bind(&request.endpoint_type).bind(&request.metadata).bind(request.endpoint_id).bind(phone.ai_phone_id)
            .execute(&mut *tx).await?;
    } else {
        sqlx::query("INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,endpoint_type,endpoint_status,metadata,last_seen_at) VALUES($1,$2,$3,'ATTACHED',$4,NOW())")
            .bind(request.endpoint_id).bind(phone.ai_phone_id).bind(&request.endpoint_type).bind(&request.metadata)
            .execute(&mut *tx).await?;
    }
    let next_fence = phone.endpoint_fence + 1;
    sqlx::query("UPDATE ai_phones SET endpoint_fence=$1,phone_status=$2,version=version+1,last_seen_at=NOW(),updated_at=NOW() WHERE ai_phone_id=$3")
        .bind(next_fence).bind(if had_current { "ATTACHED_NEW_ENDPOINT" } else { "ATTACHED" }).bind(phone.ai_phone_id)
        .execute(&mut *tx).await?;
    let attachment_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO phone_attachments(attachment_id,ai_phone_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'ACTIVE')"
    ).bind(attachment_id).bind(phone.ai_phone_id).bind(request.endpoint_id).bind(identity.tenant_id)
      .bind(identity.room_id).bind(identity.principal_id).bind(identity.participant_id).bind(identity.session_id)
      .bind(identity.execution_id).bind(next_fence).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(sqlx::query_as::<_, AttachmentRecord>(
        "SELECT attachment_id,ai_phone_id,endpoint_id,endpoint_fence,attachment_status,attached_at,detached_at FROM phone_attachments WHERE attachment_id=$1"
    ).bind(attachment_id).fetch_one(pool).await?)
}

pub async fn detach(pool: &PgPool, identity: &AuthenticatedIdentity, fence: i64) -> Result<(), PhoneError> {
    require_permission(pool, identity, "phone.detach").await.map_err(|_| PhoneError::Forbidden)?;
    let mut tx = pool.begin().await?;
    let phone = owned_phone(&mut tx, identity).await?;
    let phone = sqlx::query_as::<_, PhoneRecord>(
        "SELECT ai_phone_id,ai_id,phone_status,version,endpoint_fence,current_mission_id,current_task_id,last_seen_at,created_at,updated_at FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE"
    ).bind(phone.ai_phone_id).fetch_one(&mut *tx).await?;
    let active = sqlx::query_as::<_, (Uuid,Uuid)>(
        "SELECT attachment_id,endpoint_id FROM phone_attachments WHERE ai_phone_id=$1 AND attachment_status='ACTIVE' AND session_id=$2 AND endpoint_fence=$3 FOR UPDATE"
    ).bind(phone.ai_phone_id).bind(identity.session_id).bind(fence).fetch_optional(&mut *tx).await?
      .ok_or(PhoneError::Fenced)?;
    sqlx::query("UPDATE phone_attachments SET attachment_status='DETACHED',detached_at=NOW() WHERE attachment_id=$1")
        .bind(active.0).execute(&mut *tx).await?;
    sqlx::query("UPDATE phone_endpoints SET endpoint_status='DETACHED',detached_at=NOW() WHERE endpoint_id=$1")
        .bind(active.1).execute(&mut *tx).await?;
    sqlx::query("UPDATE ai_phones SET endpoint_fence=endpoint_fence+1,phone_status='DISCONNECTED',version=version+1,updated_at=NOW() WHERE ai_phone_id=$1")
        .bind(phone.ai_phone_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn map_phone_error(error: PhoneError) -> StatusCode {
    match error {
        PhoneError::NotFound => StatusCode::NOT_FOUND,
        PhoneError::Forbidden => StatusCode::FORBIDDEN,
        PhoneError::Conflict => StatusCode::CONFLICT,
        PhoneError::Fenced => StatusCode::PRECONDITION_FAILED,
        PhoneError::InvalidSession => StatusCode::UNAUTHORIZED,
        PhoneError::Invalid => StatusCode::BAD_REQUEST,
        PhoneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn get_phone_handler(State(pool): State<PgPool>, headers: HeaderMap)
    -> Result<Json<PhoneSnapshot>, StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    get_my_phone(&pool, &identity).await.map(Json).map_err(|e| match e {
        PhoneError::NotFound => StatusCode::NOT_FOUND, PhoneError::Forbidden => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn attach_handler(State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<AttachRequest>)
    -> Result<(StatusCode, Json<AttachmentRecord>), StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    attach(&pool, &identity, request, false).await.map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(|e| match e { PhoneError::NotFound => StatusCode::NOT_FOUND, PhoneError::Forbidden => StatusCode::FORBIDDEN,
            PhoneError::Conflict => StatusCode::CONFLICT, PhoneError::Fenced => StatusCode::PRECONDITION_FAILED,
            PhoneError::InvalidSession => StatusCode::UNAUTHORIZED, PhoneError::Invalid => StatusCode::BAD_REQUEST,
            PhoneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR })
}

#[derive(Deserialize)]
struct DetachRequest { endpoint_fence: i64 }
async fn detach_handler(State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<DetachRequest>)
    -> Result<StatusCode, StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    detach(&pool, &identity, request.endpoint_fence).await.map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| match e { PhoneError::NotFound => StatusCode::NOT_FOUND, PhoneError::Forbidden => StatusCode::FORBIDDEN,
            PhoneError::Fenced => StatusCode::PRECONDITION_FAILED, PhoneError::InvalidSession => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR })
}

async fn message_list_handler(State(pool): State<PgPool>, Path(phone_id): Path<Uuid>, headers: HeaderMap)
    -> Result<Json<Vec<PhoneMessage>>, StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    require_permission(&pool, &identity, "phone.message.read").await.map_err(|_| StatusCode::FORBIDDEN)?;
    let owns: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_phones p JOIN ai_identity_principals m ON m.ai_id=p.ai_id WHERE p.ai_phone_id=$1 AND m.tenant_id=$2 AND m.principal_id=$3)")
        .bind(phone_id).bind(identity.tenant_id).bind(identity.principal_id).fetch_one(&pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !owns { return Err(StatusCode::NOT_FOUND); }
    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM phone_attachments WHERE ai_phone_id=$1 AND session_id=$2 AND attachment_status='ACTIVE' AND endpoint_fence=(SELECT endpoint_fence FROM ai_phones WHERE ai_phone_id=$1))")
        .bind(phone_id).bind(identity.session_id).fetch_one(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !active { return Err(StatusCode::PRECONDITION_FAILED); }
    sqlx::query("UPDATE phone_messages SET message_state='DELIVERED',delivered_at=COALESCE(delivered_at,NOW()),attempt_count=attempt_count+1,updated_at=NOW() WHERE ai_phone_id=$1 AND message_state='PENDING'")
        .bind(phone_id).execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = sqlx::query_as::<_, PhoneMessage>("SELECT message_id,ai_phone_id,sender_principal_id,sequence_no,idempotency_key,message_state,content,attempt_count,created_at,delivered_at,acknowledged_at FROM phone_messages WHERE ai_phone_id=$1 ORDER BY sequence_no")
        .bind(phone_id).fetch_all(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}

async fn message_send_handler(State(pool): State<PgPool>, Path(phone_id): Path<Uuid>, headers: HeaderMap, Json(request): Json<MessageRequest>)
    -> Result<(StatusCode, Json<PhoneMessage>), StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    require_permission(&pool, &identity, "phone.message.send").await.map_err(|_| StatusCode::FORBIDDEN)?;
    if request.idempotency_key.trim().is_empty() || request.idempotency_key.len()>200
        || request.content.to_string().len()>16_384 || contains_secret_key(&request.content) { return Err(StatusCode::BAD_REQUEST); }
    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let phone = sqlx::query_as::<_, (Uuid, Uuid)>("SELECT ai_id,ai_phone_id FROM ai_phones WHERE ai_phone_id=$1 FOR UPDATE")
        .bind(phone_id).fetch_optional(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    let existing = sqlx::query_as::<_, PhoneMessage>("SELECT message_id,ai_phone_id,sender_principal_id,sequence_no,idempotency_key,message_state,content,attempt_count,created_at,delivered_at,acknowledged_at FROM phone_messages WHERE ai_phone_id=$1 AND sender_principal_id=$2 AND idempotency_key=$3")
        .bind(phone_id).bind(identity.principal_id).bind(&request.idempotency_key).fetch_optional(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(existing) = existing {
        if existing.content != request.content { return Err(StatusCode::CONFLICT); }
        tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok((StatusCode::OK, Json(existing)));
    }
    let sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence_no),0)+1 FROM phone_messages WHERE ai_phone_id=$1")
        .bind(phone_id).fetch_one(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let message_id = Uuid::new_v4();
    sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(message_id).bind(phone_id).bind(identity.tenant_id).bind(identity.principal_id).bind(phone.0)
        .bind(sequence).bind(&request.idempotency_key).bind(&request.content)
        .execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let row = sqlx::query_as::<_, PhoneMessage>("SELECT message_id,ai_phone_id,sender_principal_id,sequence_no,idempotency_key,message_state,content,attempt_count,created_at,delivered_at,acknowledged_at FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn ack_handler(State(pool): State<PgPool>, Path((phone_id,message_id)): Path<(Uuid,Uuid)>, headers: HeaderMap)
    -> Result<StatusCode, StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    require_permission(&pool, &identity, "phone.message.ack").await.map_err(|_| StatusCode::FORBIDDEN)?;
    let updated = sqlx::query("UPDATE phone_messages SET message_state='ACK',acknowledged_at=NOW(),updated_at=NOW() WHERE message_id=$1 AND ai_phone_id=$2 AND message_state='DELIVERED' AND EXISTS(SELECT 1 FROM phone_attachments a JOIN ai_phones p ON p.ai_phone_id=a.ai_phone_id WHERE a.ai_phone_id=$2 AND a.session_id=$3 AND a.attachment_status='ACTIVE' AND a.endpoint_fence=p.endpoint_fence)")
        .bind(message_id).bind(phone_id).bind(identity.session_id).execute(&pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if updated.rows_affected()==0 { return Err(StatusCode::PRECONDITION_FAILED); }
    Ok(StatusCode::NO_CONTENT)
}

async fn mission_link_handler(State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<MissionLinkRequest>)
    -> Result<StatusCode, StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    require_permission(&pool, &identity, "task.read").await.map_err(|_| StatusCode::FORBIDDEN)?;
    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let phone = owned_phone(&mut tx, &identity).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let task = sqlx::query_as::<_, (Uuid,Uuid)>("SELECT mission_id,task_id FROM team_tasks WHERE task_id=$1 AND room_id=$2 AND tenant_id=$3 AND owner_principal_id=$4 AND status IN ('OWNED','REASSIGNED','HANDOFF_PENDING','RECOVERY_REQUIRED','LEASE_EXPIRED')")
        .bind(request.task_id).bind(identity.room_id).bind(identity.tenant_id).bind(identity.principal_id)
        .fetch_optional(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::FORBIDDEN)?;
    sqlx::query("INSERT INTO phone_mission_links(ai_phone_id,task_id) VALUES($1,$2) ON CONFLICT(ai_phone_id) DO UPDATE SET task_id=EXCLUDED.task_id,updated_at=NOW()")
        .bind(phone.ai_phone_id).bind(task.1).execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("UPDATE ai_phones SET current_task_id=$1,current_mission_id=$2,version=version+1,updated_at=NOW() WHERE ai_phone_id=$3")
        .bind(task.1).bind(task.0).bind(phone.ai_phone_id).execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}


async fn reconnect_handler(State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<AttachRequest>)
    -> Result<(StatusCode, Json<AttachmentRecord>), StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    attach(&pool, &identity, request, false).await.map(|v| (StatusCode::OK, Json(v)))
        .map_err(|e| match e { PhoneError::NotFound => StatusCode::NOT_FOUND, PhoneError::Forbidden => StatusCode::FORBIDDEN,
            PhoneError::Conflict => StatusCode::CONFLICT, PhoneError::Fenced => StatusCode::PRECONDITION_FAILED,
            PhoneError::InvalidSession => StatusCode::UNAUTHORIZED, PhoneError::Invalid => StatusCode::BAD_REQUEST,
            PhoneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR })
}

async fn transfer_handler(State(pool): State<PgPool>, headers: HeaderMap, Json(request): Json<AttachRequest>)
    -> Result<(StatusCode, Json<AttachmentRecord>), StatusCode> {
    let identity = authenticate_session(&pool, &headers).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
    attach(&pool, &identity, request, true).await.map(|v| (StatusCode::OK, Json(v)))
        .map_err(|e| match e { PhoneError::NotFound => StatusCode::NOT_FOUND, PhoneError::Forbidden => StatusCode::FORBIDDEN,
            PhoneError::Conflict => StatusCode::CONFLICT, PhoneError::Fenced => StatusCode::PRECONDITION_FAILED,
            PhoneError::InvalidSession => StatusCode::UNAUTHORIZED, PhoneError::Invalid => StatusCode::BAD_REQUEST,
            PhoneError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR })
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/ai-phone/me", get(get_phone_handler))
        .route("/ai-phone/attach", post(attach_handler))
        .route("/ai-phone/detach", post(detach_handler))
        .route("/ai-phone/reconnect", post(reconnect_handler))
        .route("/ai-phone/transfer", post(transfer_handler))
        .route("/ai-phone/{phone_id}/messages", get(message_list_handler).post(message_send_handler))
        .route("/ai-phone/{phone_id}/messages/{message_id}/ack", post(ack_handler))
        .route("/ai-phone/mission", post(mission_link_handler))
        .with_state(pool)
}
