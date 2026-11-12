use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use ai_team_room_service::auth::AuthenticatedIdentity;

#[derive(Debug, thiserror::Error)]
pub enum HandoffError {
    #[error("handoff not found in authenticated tenant/room")]
    NotFound,
    #[error("authenticated identity is not the addressed source or recipient")]
    Forbidden,
    #[error("handoff transition is not valid in its current state")]
    Conflict,
    #[error("handoff or source lease has expired")]
    Expired,
    #[error("current task owner or fencing generation no longer matches")]
    Fenced,
    #[error("invalid handoff request")]
    Invalid,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TaskHandoff {
    pub handoff_id: Uuid,
    pub task_id: Uuid,
    pub tenant_id: Uuid,
    pub room_id: Uuid,
    pub from_principal_id: Option<Uuid>,
    pub from_participant_id: Option<Uuid>,
    pub from_session_id: Option<Uuid>,
    pub from_execution_id: Option<Uuid>,
    pub to_principal_id: Option<Uuid>,
    pub to_participant_id: Option<Uuid>,
    pub to_session_id: Option<Uuid>,
    pub to_execution_id: Option<Uuid>,
    pub status: String,
    pub from_fencing_token: i64,
    pub to_fencing_token: Option<i64>,
    pub checkpoint_reference: Value,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateHandoffRequest {
    pub to_principal_id: Uuid,
    pub to_participant_id: Uuid,
    pub to_session_id: Uuid,
    pub to_execution_id: Uuid,
    pub fencing_token: i64,
    pub checkpoint_reference: Option<Value>,
    pub reason: String,
    pub expires_in_seconds: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HandoffReason {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoffGrant {
    pub handoff: TaskHandoff,
    pub task: crate::tasks::TaskLease,
}

#[derive(Debug, Clone, FromRow)]
struct TaskOwner {
    task_id: Uuid,
    tenant_id: Uuid,
    room_id: Uuid,
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_participant_id: Option<Uuid>,
    owner_session_id: Option<Uuid>,
    owner_execution_id: Option<Uuid>,
    fencing_token: i64,
    lease_expires_at: Option<DateTime<Utc>>,
    checkpoint_version: i64,
}

async fn lock_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
) -> Result<TaskOwner, HandoffError> {
    sqlx::query_as::<_, TaskOwner>(
        "SELECT task_id,tenant_id,room_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint_version FROM team_tasks WHERE task_id=$1 AND tenant_id=$2 AND room_id=$3 FOR UPDATE"
    )
    .bind(task_id).bind(identity.tenant_id).bind(identity.room_id)
    .fetch_optional(&mut **tx).await?.ok_or(HandoffError::NotFound)
}

async fn lock_handoff(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &AuthenticatedIdentity,
    handoff_id: Uuid,
) -> Result<TaskHandoff, HandoffError> {
    sqlx::query_as::<_, TaskHandoff>(
        "SELECT handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at FROM task_handoffs WHERE handoff_id=$1 AND tenant_id=$2 AND room_id=$3 FOR UPDATE"
    )
    .bind(handoff_id).bind(identity.tenant_id).bind(identity.room_id)
    .fetch_optional(&mut **tx).await?.ok_or(HandoffError::NotFound)
}

fn is_source(h: &TaskHandoff, i: &AuthenticatedIdentity) -> bool {
    h.from_principal_id == Some(i.principal_id)
        && h.from_participant_id == Some(i.participant_id)
        && h.from_session_id == Some(i.session_id)
        && h.from_execution_id == Some(i.execution_id)
}

fn is_recipient(h: &TaskHandoff, i: &AuthenticatedIdentity) -> bool {
    h.to_principal_id == Some(i.principal_id)
        && h.to_participant_id == Some(i.participant_id)
        && h.to_session_id == Some(i.session_id)
        && h.to_execution_id == Some(i.execution_id)
}

async fn recipient_is_live_and_authorized(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    room_id: Uuid,
    principal_id: Uuid,
    participant_id: Uuid,
    session_id: Uuid,
    execution_id: Uuid,
) -> Result<bool, HandoffError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM principals p JOIN participants pa ON pa.tenant_id=p.tenant_id AND pa.principal_id=p.principal_id JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id JOIN sessions s ON s.tenant_id=p.tenant_id AND s.principal_id=p.principal_id AND s.room_id=$2 JOIN workload_executions w ON w.tenant_id=s.tenant_id AND w.room_id=s.room_id AND w.participant_id=pa.participant_id AND w.principal_id=p.principal_id AND w.session_id=s.session_id JOIN room_memberships m ON m.tenant_id=s.tenant_id AND m.room_id=s.room_id AND m.participant_id=pa.participant_id AND m.principal_id=p.principal_id AND m.session_id=s.session_id JOIN principal_permissions pp ON pp.tenant_id=p.tenant_id AND pp.principal_id=p.principal_id AND (pp.room_id IS NULL OR pp.room_id=$2) AND pp.permission='task.transition' AND pp.revoked_at IS NULL WHERE p.tenant_id=$1 AND p.principal_id=$3 AND pa.participant_id=$4 AND pa.enabled=TRUE AND p.enabled=TRUE AND wi.status='ACTIVE' AND s.session_id=$5 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.execution_id=$6 AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND m.status='ACTIVE')"
    )
    .bind(tenant_id).bind(room_id).bind(principal_id).bind(participant_id)
    .bind(session_id).bind(execution_id).fetch_one(&mut **tx).await?)
}

async fn identity_is_live_with_permission(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &AuthenticatedIdentity,
    permission: &str,
) -> Result<bool,HandoffError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM principals p JOIN participants pa ON pa.tenant_id=p.tenant_id AND pa.principal_id=p.principal_id JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id JOIN sessions s ON s.tenant_id=p.tenant_id AND s.principal_id=p.principal_id AND s.room_id=$2 JOIN workload_executions w ON w.tenant_id=s.tenant_id AND w.room_id=s.room_id AND w.participant_id=pa.participant_id AND w.principal_id=p.principal_id AND w.session_id=s.session_id JOIN room_memberships m ON m.tenant_id=s.tenant_id AND m.room_id=s.room_id AND m.participant_id=pa.participant_id AND m.principal_id=p.principal_id AND m.session_id=s.session_id JOIN principal_permissions pp ON pp.tenant_id=p.tenant_id AND pp.principal_id=p.principal_id AND (pp.room_id IS NULL OR pp.room_id=$2) AND pp.permission=$7 AND pp.revoked_at IS NULL WHERE p.tenant_id=$1 AND p.principal_id=$3 AND pa.participant_id=$4 AND pa.enabled=TRUE AND p.enabled=TRUE AND wi.status='ACTIVE' AND s.session_id=$5 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.execution_id=$6 AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND m.status='ACTIVE')"
    ).bind(identity.tenant_id).bind(identity.room_id).bind(identity.principal_id)
     .bind(identity.participant_id).bind(identity.session_id).bind(identity.execution_id)
     .bind(permission).fetch_one(&mut **tx).await?)
}

async fn write_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &AuthenticatedIdentity,
    action: &str,
    handoff_id: Uuid,
    metadata: Value,
) -> Result<(), HandoffError> {
    sqlx::query("INSERT INTO security_audit_events (security_event_id,tenant_id,room_id,principal_id,action,outcome,reason_code,metadata) VALUES ($1,$2,$3,$4,$5,'SUCCESS',$6,$7)")
        .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(identity.room_id)
        .bind(identity.principal_id).bind(action).bind(action).bind(serde_json::json!({"handoff_id":handoff_id,"details":metadata.clone()}))
        .execute(&mut **tx).await?;
    sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'task_handoff',$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(handoff_id).bind(action)
        .bind(serde_json::json!({"room_id":identity.room_id,"handoff_id":handoff_id,"actor_principal_id":identity.principal_id,"metadata":metadata}))
        .execute(&mut **tx).await?;
    Ok(())
}

async fn load_task_lease(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: Uuid,
) -> Result<crate::tasks::TaskLease, HandoffError> {
    Ok(sqlx::query_as::<_, crate::tasks::TaskLease>(
        "SELECT task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version FROM team_tasks WHERE task_id=$1"
    ).bind(task_id).fetch_one(&mut **tx).await?)
}

async fn insert_handoff(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task: &TaskOwner,
    from: &AuthenticatedIdentity,
    to_principal_id: Uuid,
    to_participant_id: Uuid,
    to_session_id: Uuid,
    to_execution_id: Uuid,
    checkpoint_reference: Value,
    reason: &str,
    expires_in_seconds: i64,
) -> Result<TaskHandoff, HandoffError> {
    let handoff_id=Uuid::new_v4();
    Ok(sqlx::query_as::<_,TaskHandoff>(
        "INSERT INTO task_handoffs (handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,from_fencing_token,status,context,checkpoint_reference,reason,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'PENDING',$14,$14,$15,NOW()+($16 * INTERVAL '1 second')) RETURNING handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at"
    )
    .bind(handoff_id).bind(task.task_id).bind(task.tenant_id).bind(task.room_id)
    .bind(from.principal_id).bind(from.participant_id).bind(from.session_id).bind(from.execution_id)
    .bind(to_principal_id).bind(to_participant_id).bind(to_session_id).bind(to_execution_id)
    .bind(task.fencing_token).bind(checkpoint_reference).bind(reason).bind(expires_in_seconds)
    .fetch_one(&mut **tx).await?)
}

pub async fn create(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    request: CreateHandoffRequest,
) -> Result<TaskHandoff, HandoffError> {
    if request.reason.trim().is_empty() || request.reason.len()>1000
        || !(30..=900).contains(&request.expires_in_seconds.unwrap_or(300))
        || request.fencing_token<1 || request.to_principal_id==identity.principal_id {
        return Err(HandoffError::Invalid);
    }
    let mut tx=pool.begin().await?;
    let task=lock_task(&mut tx,identity,task_id).await?;
    if !identity_is_live_with_permission(&mut tx,identity,"task.transition").await? { return Err(HandoffError::Forbidden); }
    let lease_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
        .bind(task.lease_expires_at).fetch_one(&mut *tx).await?;
    let source_valid=task.owner_principal_id==Some(identity.principal_id)
        && task.owner_participant_id==Some(identity.participant_id)
        && task.owner_session_id==Some(identity.session_id)
        && task.owner_execution_id==Some(identity.execution_id)
        && task.fencing_token==request.fencing_token
        && matches!(task.status.as_str(),"OWNED"|"REASSIGNED") && lease_live;
    if !source_valid { return Err(HandoffError::Fenced); }
    if !recipient_is_live_and_authorized(&mut tx,identity.tenant_id,identity.room_id,
        request.to_principal_id,request.to_participant_id,request.to_session_id,request.to_execution_id).await? {
        return Err(HandoffError::Forbidden);
    }
    sqlx::query("UPDATE task_handoffs SET status='EXPIRED' WHERE task_id=$1 AND status='PENDING' AND expires_at<=NOW()").bind(task_id).execute(&mut *tx).await?;
    let existing: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM task_handoffs WHERE task_id=$1 AND status='PENDING' AND expires_at>NOW())")
        .bind(task_id).fetch_one(&mut *tx).await?;
    if existing { return Err(HandoffError::Conflict); }
    let checkpoint_reference=match request.checkpoint_reference {
        Some(reference) => {
            if reference.get("task_id").and_then(Value::as_str)!=Some(task_id.to_string().as_str())
                || reference.get("checkpoint_version").and_then(Value::as_i64)!=Some(task.checkpoint_version) {
                return Err(HandoffError::Invalid);
            }
            reference
        },
        None => serde_json::json!({"task_id":task_id,"checkpoint_version":task.checkpoint_version})
    };
    let handoff=insert_handoff(&mut tx,&task,identity,request.to_principal_id,request.to_participant_id,
        request.to_session_id,request.to_execution_id,checkpoint_reference,&request.reason,
        request.expires_in_seconds.unwrap_or(300)).await?;
    write_audit(&mut tx,identity,"task.handoff.created",handoff.handoff_id,
        serde_json::json!({"to_principal_id":request.to_principal_id,"from_fencing_token":task.fencing_token})).await?;
    tx.commit().await?;
    Ok(handoff)
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecoveryHandoffRequest {
    pub to_principal_id: Uuid,
    pub to_participant_id: Uuid,
    pub to_session_id: Uuid,
    pub to_execution_id: Uuid,
    pub reason: String,
}

pub async fn create_recovery(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    to_principal_id: Uuid,
    to_participant_id: Uuid,
    to_session_id: Uuid,
    to_execution_id: Uuid,
    reason: String,
) -> Result<TaskHandoff, HandoffError> {
    if reason.trim().is_empty() || reason.len()>1000 { return Err(HandoffError::Invalid); }
    let mut tx=pool.begin().await?;
    let task=lock_task(&mut tx,identity,task_id).await?;
    if !identity_is_live_with_permission(&mut tx,identity,"task.assign").await? { return Err(HandoffError::Forbidden); }
    let lease_expired:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz<=NOW(),FALSE)")
        .bind(task.lease_expires_at).fetch_one(&mut *tx).await?;
    if task.status!="RECOVERY_REQUIRED" || !lease_expired
        || task.owner_principal_id.is_none() || task.owner_participant_id.is_none()
        || task.owner_session_id.is_none() || task.owner_execution_id.is_none()
        || (task.owner_principal_id==Some(to_principal_id)
            && task.owner_session_id==Some(to_session_id)
            && task.owner_execution_id==Some(to_execution_id)) {
        return Err(HandoffError::Conflict);
    }
    if !recipient_is_live_and_authorized(&mut tx,identity.tenant_id,identity.room_id,
        to_principal_id,to_participant_id,to_session_id,to_execution_id).await? {
        return Err(HandoffError::Forbidden);
    }
    sqlx::query("UPDATE task_handoffs SET status='EXPIRED' WHERE task_id=$1 AND status='PENDING' AND expires_at<=NOW()").bind(task_id).execute(&mut *tx).await?;
    if let Some(existing)=sqlx::query_as::<_,TaskHandoff>(
        "SELECT handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at FROM task_handoffs WHERE task_id=$1 AND status='PENDING' AND expires_at>NOW() FOR UPDATE"
    ).bind(task_id).fetch_optional(&mut *tx).await? {
        if is_recipient(&existing,&AuthenticatedIdentity{tenant_id:identity.tenant_id,room_id:identity.room_id,principal_id:to_principal_id,participant_id:to_participant_id,session_id:to_session_id,execution_id:to_execution_id,issuer:String::new(),subject:String::new()}) {
            tx.commit().await?;
            return Ok(existing);
        }
        sqlx::query("UPDATE task_handoffs SET status='CANCELLED',reason='recovery selected a different eligible recipient' WHERE handoff_id=$1")
            .bind(existing.handoff_id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'task_handoff',$3,'HANDOFF_CANCELLED',$4)")
            .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(existing.handoff_id)
            .bind(serde_json::json!({"task_id":task_id,"reason":"recovery selected a different eligible recipient"}))
            .execute(&mut *tx).await?;
    }
    let handoff_id=Uuid::new_v4();
    let checkpoint_reference=serde_json::json!({"task_id":task_id,"checkpoint_version":task.checkpoint_version});
    let handoff=sqlx::query_as::<_,TaskHandoff>(
        "INSERT INTO task_handoffs (handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,from_fencing_token,status,context,checkpoint_reference,reason,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'PENDING',$14,$14,$15,NOW()+INTERVAL '5 minutes') RETURNING handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at"
    ).bind(handoff_id).bind(task_id).bind(task.tenant_id).bind(task.room_id)
     .bind(task.owner_principal_id).bind(task.owner_participant_id).bind(task.owner_session_id).bind(task.owner_execution_id)
     .bind(to_principal_id).bind(to_participant_id).bind(to_session_id).bind(to_execution_id)
     .bind(task.fencing_token).bind(checkpoint_reference).bind(reason)
     .fetch_one(&mut *tx).await?;
    write_audit(&mut tx,identity,"task.handoff.recovery_created",handoff.handoff_id,
        serde_json::json!({"previous_owner":task.owner_principal_id,"old_fencing_token":task.fencing_token})).await?;
    tx.commit().await?;
    Ok(handoff)
}

pub async fn accept(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    handoff_id: Uuid,
    lease_seconds: i64,
) -> Result<HandoffGrant,HandoffError> {
    if !(15..=300).contains(&lease_seconds) { return Err(HandoffError::Invalid); }
    let task_id: Uuid=sqlx::query_scalar("SELECT task_id FROM task_handoffs WHERE handoff_id=$1 AND tenant_id=$2 AND room_id=$3")
        .bind(handoff_id).bind(identity.tenant_id).bind(identity.room_id)
        .fetch_optional(pool).await?.ok_or(HandoffError::NotFound)?;
    let mut tx=pool.begin().await?;
    let task=lock_task(&mut tx,identity,task_id).await?;
    let handoff=lock_handoff(&mut tx,identity,handoff_id).await?;
    if !identity_is_live_with_permission(&mut tx,identity,"task.transition").await? || !is_recipient(&handoff,identity) { return Err(HandoffError::Forbidden); }
    if handoff.status!="PENDING" { return Err(HandoffError::Conflict); }
    let handoff_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
        .bind(handoff.expires_at).fetch_one(&mut *tx).await?;
    if !handoff_live {
        sqlx::query("UPDATE task_handoffs SET status='EXPIRED' WHERE handoff_id=$1").bind(handoff_id).execute(&mut *tx).await?;
        write_audit(&mut tx,identity,"task.handoff.expired",handoff_id,serde_json::json!({"reason":"accept_after_expiry"})).await?;
        tx.commit().await?;
        return Err(HandoffError::Expired);
    }
    if !recipient_is_live_and_authorized(&mut tx,identity.tenant_id,identity.room_id,
        identity.principal_id,identity.participant_id,identity.session_id,identity.execution_id).await? {
        return Err(HandoffError::Forbidden);
    }
    let source_matches=task.owner_principal_id==handoff.from_principal_id
        && task.owner_participant_id==handoff.from_participant_id
        && task.owner_session_id==handoff.from_session_id
        && task.owner_execution_id==handoff.from_execution_id
        && task.fencing_token==handoff.from_fencing_token;
    let source_lease_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
        .bind(task.lease_expires_at).fetch_one(&mut *tx).await?;
    let source_state=matches!(task.status.as_str(),"OWNED"|"REASSIGNED") && source_lease_live;
    let recovery_state=task.status=="RECOVERY_REQUIRED" && !source_lease_live;
    if !source_matches || !(source_state||recovery_state) { return Err(HandoffError::Fenced); }
    let abandoned_handoffs=sqlx::query_scalar::<_,Uuid>("UPDATE task_handoffs SET status='CANCELLED',reason='recipient lease expired before task completion' WHERE task_id=$1 AND status='ACCEPTED' AND to_fencing_token=$2 AND handoff_id<>$3 RETURNING handoff_id")
        .bind(task_id).bind(task.fencing_token).bind(handoff_id).fetch_all(&mut *tx).await?;
    for abandoned_id in abandoned_handoffs {
        sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'task_handoff',$3,'HANDOFF_CANCELLED',$4)")
            .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(abandoned_id)
            .bind(serde_json::json!({"task_id":task_id,"reason":"recipient lease expired before task completion"}))
            .execute(&mut *tx).await?;
    }
    let new_fence=task.fencing_token+1;
    sqlx::query("UPDATE team_tasks SET status='REASSIGNED',owner_principal_id=$2,owner_participant_id=$3,owner_session_id=$4,owner_execution_id=$5,fencing_token=$6,lease_issued_at=NOW(),lease_renewed_at=NOW(),lease_expires_at=NOW()+($7 * INTERVAL '1 second'),updated_at=NOW() WHERE task_id=$1")
        .bind(task_id).bind(identity.principal_id).bind(identity.participant_id).bind(identity.session_id)
        .bind(identity.execution_id).bind(new_fence).bind(lease_seconds).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'team_task',$3,'TASK_UPDATED',$4)")
        .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(task_id)
        .bind(serde_json::json!({"room_id":identity.room_id,"task_id":task_id,"owner_principal_id":identity.principal_id,"owner_session_id":identity.session_id,"fencing_token":new_fence,"transition":"HANDOFF_ACCEPTED"}))
        .execute(&mut *tx).await?;
    let updated_handoff=sqlx::query_as::<_,TaskHandoff>(
        "UPDATE task_handoffs SET status='ACCEPTED',to_fencing_token=$2,accepted_at=NOW() WHERE handoff_id=$1 AND status='PENDING' RETURNING handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at"
    ).bind(handoff_id).bind(new_fence).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO task_lease_events (lease_event_id,tenant_id,room_id,task_id,principal_id,participant_id,session_id,fencing_token,event_type,metadata) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'ACQUIRED',$9)")
        .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(identity.room_id).bind(task_id)
        .bind(identity.principal_id).bind(identity.participant_id).bind(identity.session_id).bind(new_fence)
        .bind(serde_json::json!({"handoff_id":handoff_id,"previous_fencing_token":task.fencing_token}))
        .execute(&mut *tx).await?;
    write_audit(&mut tx,identity,"task.handoff.accepted",handoff_id,
        serde_json::json!({"old_fencing_token":task.fencing_token,"new_fencing_token":new_fence})).await?;
    let updated_task=load_task_lease(&mut tx,task_id).await?;
    tx.commit().await?;
    Ok(HandoffGrant{handoff:updated_handoff,task:updated_task})
}

pub async fn reject(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    handoff_id: Uuid,
    reason: String,
) -> Result<TaskHandoff,HandoffError> {
    if reason.trim().is_empty() || reason.len()>1000 { return Err(HandoffError::Invalid); }
    let task_id: Uuid=sqlx::query_scalar("SELECT task_id FROM task_handoffs WHERE handoff_id=$1 AND tenant_id=$2 AND room_id=$3")
        .bind(handoff_id).bind(identity.tenant_id).bind(identity.room_id)
        .fetch_optional(pool).await?.ok_or(HandoffError::NotFound)?;
    let mut tx=pool.begin().await?;
    let _task=lock_task(&mut tx,identity,task_id).await?;
    let handoff=lock_handoff(&mut tx,identity,handoff_id).await?;
    if !is_recipient(&handoff,identity) { return Err(HandoffError::Forbidden); }
    if handoff.status!="PENDING" { return Err(HandoffError::Conflict); }
    let handoff_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
        .bind(handoff.expires_at).fetch_one(&mut *tx).await?;
    let status=if handoff_live {"REJECTED"} else {"EXPIRED"};
    let updated=sqlx::query_as::<_,TaskHandoff>(
        "UPDATE task_handoffs SET status=$2,reason=$3 WHERE handoff_id=$1 RETURNING handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at"
    ).bind(handoff_id).bind(status).bind(reason).fetch_one(&mut *tx).await?;
    write_audit(&mut tx,identity,"task.handoff.rejected",handoff_id,serde_json::json!({"status":status})).await?;
    tx.commit().await?;
    if status=="EXPIRED" { return Err(HandoffError::Expired); }
    Ok(updated)
}

pub async fn cancel(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    handoff_id: Uuid,
    reason: String,
) -> Result<TaskHandoff,HandoffError> {
    if reason.trim().is_empty() || reason.len()>1000 { return Err(HandoffError::Invalid); }
    let task_id: Uuid=sqlx::query_scalar("SELECT task_id FROM task_handoffs WHERE handoff_id=$1 AND tenant_id=$2 AND room_id=$3")
        .bind(handoff_id).bind(identity.tenant_id).bind(identity.room_id)
        .fetch_optional(pool).await?.ok_or(HandoffError::NotFound)?;
    let mut tx=pool.begin().await?;
    let task=lock_task(&mut tx,identity,task_id).await?;
    let handoff=lock_handoff(&mut tx,identity,handoff_id).await?;
    if !identity_is_live_with_permission(&mut tx,identity,"task.transition").await? { return Err(HandoffError::Forbidden); }
    match handoff.status.as_str() {
        "PENDING" if is_source(&handoff,identity)
            && task.owner_principal_id==Some(identity.principal_id)
            && task.owner_participant_id==Some(identity.participant_id)
            && task.owner_session_id==Some(identity.session_id)
            && task.owner_execution_id==Some(identity.execution_id)
            && task.fencing_token==handoff.from_fencing_token
            && matches!(task.status.as_str(),"OWNED"|"REASSIGNED")
            && task.lease_expires_at.is_some() => {
                let lease_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
                    .bind(task.lease_expires_at).fetch_one(&mut *tx).await?;
                if !lease_live { return Err(HandoffError::Fenced); }
                let handoff_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
                    .bind(handoff.expires_at).fetch_one(&mut *tx).await?;
                if !handoff_live {
                    sqlx::query("UPDATE task_handoffs SET status='EXPIRED' WHERE handoff_id=$1").bind(handoff_id).execute(&mut *tx).await?;
                    write_audit(&mut tx,identity,"task.handoff.expired",handoff_id,serde_json::json!({"reason":"cancel_after_expiry"})).await?;
                    tx.commit().await?;
                    return Err(HandoffError::Expired);
                }
            },
        "ACCEPTED" if is_recipient(&handoff,identity)
            && task.owner_principal_id==Some(identity.principal_id)
            && task.owner_participant_id==Some(identity.participant_id)
            && task.owner_session_id==Some(identity.session_id)
            && task.owner_execution_id==Some(identity.execution_id)
            && Some(task.fencing_token)==handoff.to_fencing_token
            && task.lease_expires_at.is_some() => {
                let lease_live:bool=sqlx::query_scalar("SELECT COALESCE($1::timestamptz>NOW(),FALSE)")
                    .bind(task.lease_expires_at).fetch_one(&mut *tx).await?;
                if !lease_live { return Err(HandoffError::Fenced); }
                sqlx::query("UPDATE team_tasks SET status='UNOWNED',owner_principal_id=NULL,owner_participant_id=NULL,owner_session_id=NULL,owner_execution_id=NULL,lease_issued_at=NULL,lease_renewed_at=NULL,lease_expires_at=NULL,updated_at=NOW() WHERE task_id=$1")
                    .bind(task_id).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO task_lease_events (lease_event_id,tenant_id,room_id,task_id,principal_id,participant_id,session_id,fencing_token,event_type,metadata) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'RELEASED',$9)")
                    .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(identity.room_id).bind(task_id)
                    .bind(identity.principal_id).bind(identity.participant_id).bind(identity.session_id)
                    .bind(task.fencing_token).bind(serde_json::json!({"handoff_id":handoff_id,"reason":"accepted handoff cancelled"}))
                    .execute(&mut *tx).await?;
                sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'team_task',$3,'TASK_UPDATED',$4)")
                    .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(task_id)
                    .bind(serde_json::json!({"room_id":identity.room_id,"task_id":task_id,"transition":"HANDOFF_CANCELLED","status":"UNOWNED","fencing_token":task.fencing_token}))
                    .execute(&mut *tx).await?;
            },
        _ => return Err(if is_source(&handoff,identity)||is_recipient(&handoff,identity) {HandoffError::Conflict} else {HandoffError::Forbidden}),
    }
    let updated=sqlx::query_as::<_,TaskHandoff>(
        "UPDATE task_handoffs SET status='CANCELLED',reason=$2 WHERE handoff_id=$1 RETURNING handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at"
    ).bind(handoff_id).bind(reason).fetch_one(&mut *tx).await?;
    write_audit(&mut tx,identity,"task.handoff.cancelled",handoff_id,serde_json::json!({"previous_status":handoff.status})).await?;
    tx.commit().await?;
    Ok(updated)
}

pub async fn get(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    handoff_id: Uuid,
) -> Result<TaskHandoff,HandoffError> {
    let handoff=sqlx::query_as::<_,TaskHandoff>(
        "SELECT handoff_id,task_id,tenant_id,room_id,from_principal_id,from_participant_id,from_session_id,from_execution_id,to_principal_id,to_participant_id,to_session_id,to_execution_id,status,from_fencing_token,to_fencing_token,checkpoint_reference,reason,created_at,expires_at,accepted_at,completed_at FROM task_handoffs WHERE handoff_id=$1 AND tenant_id=$2 AND room_id=$3 AND ((from_principal_id=$4 AND from_participant_id=$5 AND from_session_id=$6 AND from_execution_id=$7) OR (to_principal_id=$4 AND to_participant_id=$5 AND to_session_id=$6 AND to_execution_id=$7))"
    ).bind(handoff_id).bind(identity.tenant_id).bind(identity.room_id).bind(identity.principal_id)
     .bind(identity.participant_id).bind(identity.session_id).bind(identity.execution_id)
     .fetch_optional(pool).await?.ok_or(HandoffError::NotFound)?;
    Ok(handoff)
}



#[cfg(test)]
mod postgres_tests {
    use super::*;
    use crate::tasks::{self,CreateTask};
    use crate::phone::{self,AttachRequest};
    use ai_team_room_service::{connect_pool,migrate};

    async fn fixture() -> (PgPool,Uuid,Vec<AuthenticatedIdentity>) {
        let url=std::env::var("DATABASE_URL").expect("PostgreSQL integration tests require DATABASE_URL");
        let pool=connect_pool(&url).await.unwrap();
        migrate(&pool).await.unwrap();
        let tenant=Uuid::new_v4();
        let room=Uuid::new_v4();
        sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'handoff-postgres-test')").bind(tenant).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'handoff-room')").bind(room).bind(tenant).execute(&pool).await.unwrap();
        let mut identities=Vec::new();
        for n in 0..4 {
            let principal=Uuid::new_v4();
            let participant=Uuid::new_v4();
            let workload=Uuid::new_v4();
            let session=Uuid::new_v4();
            let execution=Uuid::new_v4();
            let subject=format!("handoff-test-{n}");
            sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','handoff-test',$3)")
                .bind(principal).bind(tenant).bind(&subject).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)")
                .bind(participant).bind(tenant).bind(principal).bind(format!("handoff executor {n}")).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'handoff-test',$4,'ACTIVE')")
                .bind(workload).bind(tenant).bind(principal).bind(&subject).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '20 minutes','ACTIVE')")
                .bind(session).bind(tenant).bind(room).bind(principal).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '20 minutes','ACTIVE')")
                .bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).bind(&subject).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
                .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).execute(&pool).await.unwrap();
            for permission in ["task.read","task.transition"] {
                sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)")
                    .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).bind(permission).execute(&pool).await.unwrap();
            }
            if n>=2 {
                sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,'task.assign')")
                    .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).execute(&pool).await.unwrap();
            }
            identities.push(AuthenticatedIdentity{tenant_id:tenant,participant_id:participant,principal_id:principal,session_id:session,execution_id:execution,room_id:room,issuer:"handoff-test".into(),subject});
        }
        (pool,tenant,identities)
    }

    fn request(target:&AuthenticatedIdentity,fence:i64)->CreateHandoffRequest {
        CreateHandoffRequest {
            to_principal_id:target.principal_id,to_participant_id:target.participant_id,
            to_session_id:target.session_id,to_execution_id:target.execution_id,
            fencing_token:fence,checkpoint_reference:None,reason:"planned authenticated transfer".into(),
            expires_in_seconds:Some(300)
        }
    }

    #[tokio::test]
    async fn postgres_handoff_states_authorization_fencing_and_candidate_race() {
        let (pool,tenant,ids)=fixture().await;
        let task=tasks::create_task(&pool,&ids[0],CreateTask{mission_id:Uuid::new_v4(),title:"handoff acceptance".into(),checkpoint:serde_json::json!({"progress":55})}).await.unwrap();
        let first=tasks::acquire_lease(&pool,&ids[0],task.task_id,60).await.unwrap();
        assert_eq!(first.task.fencing_token,1);
        let handoff=create(&pool,&ids[0],task.task_id,request(&ids[1],1)).await.unwrap();
        assert_eq!(handoff.status,"PENDING");
        assert_eq!(handoff.from_session_id,Some(ids[0].session_id));
        assert_eq!(handoff.to_session_id,Some(ids[1].session_id));
        assert_eq!(handoff.from_fencing_token,1);
        assert_eq!(handoff.to_fencing_token,None);
        tasks::renew_lease(&pool,&ids[0],task.task_id,1,60).await.unwrap();
        tasks::write_checkpoint(&pool,&ids[0],task.task_id,1,serde_json::json!({"progress":60})).await.unwrap();
        let refreshed_offer=get(&pool,&ids[0],handoff.handoff_id).await.unwrap();
        assert_eq!(refreshed_offer.checkpoint_reference["checkpoint_version"],1,"source checkpoints while pending must refresh the persisted handoff reference");
        assert!(matches!(create(&pool,&ids[2],task.task_id,request(&ids[1],1)).await,Err(HandoffError::Fenced)),"a non-owner cannot create a handoff for another executor's task");
        let mut spoofed=ids[1].clone();
        spoofed.session_id=Uuid::new_v4();
        assert!(matches!(accept(&pool,&spoofed,handoff.handoff_id,60).await,Err(HandoffError::Forbidden)),"handoff UUID plus a different session is insufficient");

        // Two independent authenticated candidates race the same handoff. The
        // addressed session wins; the other active, permitted candidate is denied.
        let p1=pool.clone(); let p2=pool.clone();
        let candidate=ids[1].clone(); let competitor=ids[2].clone();
        let handoff_id=handoff.handoff_id;
        let (accepted,denied)=tokio::join!(
            accept(&p1,&candidate,handoff_id,60),
            accept(&p2,&competitor,handoff_id,60)
        );
        let grant=accepted.unwrap();
        assert_eq!(grant.handoff.status,"ACCEPTED");
        assert_eq!(grant.handoff.to_fencing_token,Some(2));
        assert_eq!(grant.task.fencing_token,2);
        assert!(matches!(denied,Err(HandoffError::Forbidden)));
        assert!(matches!(accept(&pool,&ids[1],handoff_id,60).await,Err(HandoffError::Conflict)));
        assert!(matches!(tasks::renew_lease(&pool,&ids[0],task.task_id,1,60).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::write_checkpoint(&pool,&ids[0],task.task_id,1,serde_json::json!({"stale":true})).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::complete_task(&pool,&ids[0],task.task_id,1,serde_json::json!({})).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::release_lease(&pool,&ids[0],task.task_id,1).await,Err(tasks::TaskError::Fenced)));
        tasks::renew_lease(&pool,&ids[1],task.task_id,2,60).await.unwrap();
        tasks::write_checkpoint(&pool,&ids[1],task.task_id,2,serde_json::json!({"progress":70})).await.unwrap();
        let completed=tasks::complete_task(&pool,&ids[1],task.task_id,2,serde_json::json!({"done":true})).await.unwrap();
        assert_eq!(completed.status,"COMPLETED");
        assert_eq!(get(&pool,&ids[1],handoff_id).await.unwrap().status,"COMPLETED");
        assert!(matches!(accept(&pool,&ids[1],handoff_id,60).await,Err(HandoffError::Conflict)),"COMPLETED handoff must never reopen");
        assert!(sqlx::query("UPDATE task_handoffs SET status='ACCEPTED' WHERE handoff_id=$1").bind(handoff_id).execute(&pool).await.is_err(),"database trigger must reject COMPLETED -> ACCEPTED");

        // PENDING -> REJECTED; terminal transitions cannot be reopened.
        let rejected_task=tasks::create_task(&pool,&ids[0],CreateTask{mission_id:Uuid::new_v4(),title:"reject transition".into(),checkpoint:serde_json::json!({})}).await.unwrap();
        let rejected_lease=tasks::acquire_lease(&pool,&ids[0],rejected_task.task_id,60).await.unwrap();
        let rejected=create(&pool,&ids[0],rejected_task.task_id,request(&ids[1],rejected_lease.task.fencing_token)).await.unwrap();
        assert_eq!(reject(&pool,&ids[1],rejected.handoff_id,"not available".into()).await.unwrap().status,"REJECTED");
        assert!(matches!(accept(&pool,&ids[1],rejected.handoff_id,60).await,Err(HandoffError::Conflict)));
        assert!(sqlx::query("UPDATE task_handoffs SET status='ACCEPTED' WHERE handoff_id=$1").bind(rejected.handoff_id).execute(&pool).await.is_err(),"database trigger must reject REJECTED -> ACCEPTED");

        // PENDING -> CANCELLED by source; recipient cannot subsequently accept.
        let cancelled_task=tasks::create_task(&pool,&ids[0],CreateTask{mission_id:Uuid::new_v4(),title:"cancel transition".into(),checkpoint:serde_json::json!({})}).await.unwrap();
        let cancelled_lease=tasks::acquire_lease(&pool,&ids[0],cancelled_task.task_id,60).await.unwrap();
        let cancelled=create(&pool,&ids[0],cancelled_task.task_id,request(&ids[1],cancelled_lease.task.fencing_token)).await.unwrap();
        assert_eq!(cancel(&pool,&ids[0],cancelled.handoff_id,"owner cancelled".into()).await.unwrap().status,"CANCELLED");
        assert!(matches!(accept(&pool,&ids[1],cancelled.handoff_id,60).await,Err(HandoffError::Conflict)));
        assert!(sqlx::query("UPDATE task_handoffs SET status='ACCEPTED' WHERE handoff_id=$1").bind(cancelled.handoff_id).execute(&pool).await.is_err(),"database trigger must reject CANCELLED -> ACCEPTED");

        // PENDING -> EXPIRED uses the PostgreSQL timestamp and persists the state.
        let expired_task=tasks::create_task(&pool,&ids[0],CreateTask{mission_id:Uuid::new_v4(),title:"expire transition".into(),checkpoint:serde_json::json!({})}).await.unwrap();
        let expired_lease=tasks::acquire_lease(&pool,&ids[0],expired_task.task_id,60).await.unwrap();
        let expired=create(&pool,&ids[0],expired_task.task_id,request(&ids[1],expired_lease.task.fencing_token)).await.unwrap();
        sqlx::query("UPDATE task_handoffs SET created_at=NOW()-INTERVAL '2 minutes',expires_at=NOW()-INTERVAL '1 second' WHERE handoff_id=$1").bind(expired.handoff_id).execute(&pool).await.unwrap();
        assert!(matches!(accept(&pool,&ids[1],expired.handoff_id,60).await,Err(HandoffError::Expired)));
        assert_eq!(get(&pool,&ids[1],expired.handoff_id).await.unwrap().status,"EXPIRED");
        assert!(sqlx::query("UPDATE task_handoffs SET status='ACCEPTED' WHERE handoff_id=$1").bind(expired.handoff_id).execute(&pool).await.is_err(),"database trigger must reject EXPIRED -> ACCEPTED");

        // ACCEPTED -> CANCELLED releases the candidate lease without reviving A.
        let accepted_cancel_task=tasks::create_task(&pool,&ids[0],CreateTask{mission_id:Uuid::new_v4(),title:"accepted cancel transition".into(),checkpoint:serde_json::json!({})}).await.unwrap();
        let accepted_cancel_lease=tasks::acquire_lease(&pool,&ids[0],accepted_cancel_task.task_id,60).await.unwrap();
        let accepted_cancel=create(&pool,&ids[0],accepted_cancel_task.task_id,request(&ids[1],accepted_cancel_lease.task.fencing_token)).await.unwrap();
        let original_session_times:(DateTime<Utc>,DateTime<Utc>)=sqlx::query_as(
            "SELECT issued_at,expires_at FROM sessions WHERE session_id=$1"
        ).bind(ids[1].session_id).fetch_one(&pool).await.unwrap();
        sqlx::query("UPDATE sessions SET issued_at=NOW()-INTERVAL '2 minutes',expires_at=NOW()-INTERVAL '1 second' WHERE session_id=$1")
            .bind(ids[1].session_id).execute(&pool).await.unwrap();
        let expired_session_is_temporally_valid:bool=sqlx::query_scalar(
            "SELECT issued_at<expires_at AND expires_at<=NOW() FROM sessions WHERE session_id=$1"
        ).bind(ids[1].session_id).fetch_one(&pool).await.unwrap();
        assert!(expired_session_is_temporally_valid,"expired session fixture must preserve issued_at < expires_at");
        assert!(matches!(accept(&pool,&ids[1],accepted_cancel.handoff_id,60).await,Err(HandoffError::Forbidden)),"an expired recipient session cannot accept");
        sqlx::query("UPDATE sessions SET issued_at=$2,expires_at=$3 WHERE session_id=$1")
            .bind(ids[1].session_id).bind(original_session_times.0).bind(original_session_times.1)
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE sessions SET status='REVOKED',revoked_at=NOW() WHERE session_id=$1").bind(ids[1].session_id).execute(&pool).await.unwrap();
        assert!(matches!(accept(&pool,&ids[1],accepted_cancel.handoff_id,60).await,Err(HandoffError::Forbidden)),"a revoked recipient session cannot accept");
        sqlx::query("UPDATE sessions SET status='ACTIVE',revoked_at=NULL WHERE session_id=$1").bind(ids[1].session_id).execute(&pool).await.unwrap();
        let accepted_cancel_grant=accept(&pool,&ids[1],accepted_cancel.handoff_id,60).await.unwrap();
        assert_eq!(cancel(&pool,&ids[1],accepted_cancel.handoff_id,"recipient relinquished".into()).await.unwrap().status,"CANCELLED");
        let released_status:String=sqlx::query_scalar("SELECT status FROM team_tasks WHERE task_id=$1").bind(accepted_cancel_task.task_id).fetch_one(&pool).await.unwrap();
        assert_eq!(released_status,"UNOWNED");
        assert!(matches!(accept(&pool,&ids[1],accepted_cancel.handoff_id,60).await,Err(HandoffError::Conflict)));
        assert!(sqlx::query("UPDATE task_handoffs SET status='ACCEPTED' WHERE handoff_id=$1").bind(accepted_cancel.handoff_id).execute(&pool).await.is_err(),"database trigger must reject CANCELLED -> ACCEPTED");
        assert_eq!(accepted_cancel_grant.task.fencing_token,2);

        // Recovery workers race to persist the same candidate-bound offer.
        let recovery_task=tasks::create_task(&pool,&ids[0],CreateTask{mission_id:Uuid::new_v4(),title:"recovery handoff".into(),checkpoint:serde_json::json!({"resume":true})}).await.unwrap();
        let recovery_lease=tasks::acquire_lease(&pool,&ids[0],recovery_task.task_id,15).await.unwrap();
        sqlx::query("UPDATE team_tasks SET lease_expires_at=NOW()-INTERVAL '1 second' WHERE task_id=$1").bind(recovery_task.task_id).execute(&pool).await.unwrap();
        assert_eq!(tasks::recover_expired(&pool,ids[0].room_id).await.unwrap(),1);
        let p1=pool.clone();let p2=pool.clone();let worker_a=ids[2].clone();let worker_b=ids[3].clone();
        let target=ids[1].clone();let target2=ids[1].clone();let task_id=recovery_task.task_id;
        let (offer_a,offer_b)=tokio::join!(
            create_recovery(&p1,&worker_a,task_id,target.principal_id,target.participant_id,target.session_id,target.execution_id,"recovery after lease loss".into()),
            create_recovery(&p2,&worker_b,task_id,target2.principal_id,target2.participant_id,target2.session_id,target2.execution_id,"recovery after lease loss".into())
        );
        let offer_a=offer_a.unwrap();let offer_b=offer_b.unwrap();
        assert_eq!(offer_a.handoff_id,offer_b.handoff_id);
        assert_eq!(offer_a.status,"PENDING");
        let recovered=accept(&pool,&ids[1],offer_a.handoff_id,60).await.unwrap();
        assert_eq!(recovered.task.fencing_token,recovery_lease.task.fencing_token+1);
        assert_eq!(recovered.task.checkpoint["resume"],true);

        // Recipient B accepts but disappears before completion. Recovery
        // offers the same persisted checkpoint to C with a third fence.
        sqlx::query("UPDATE team_tasks SET lease_expires_at=NOW()-INTERVAL '1 second' WHERE task_id=$1").bind(task_id).execute(&pool).await.unwrap();
        assert_eq!(tasks::recover_expired(&pool,ids[0].room_id).await.unwrap(),1);
        let second_offer=create_recovery(&pool,&ids[3],task_id,ids[2].principal_id,ids[2].participant_id,ids[2].session_id,ids[2].execution_id,"recipient B disconnected".into()).await.unwrap();
        let third_grant=accept(&pool,&ids[2],second_offer.handoff_id,60).await.unwrap();
        assert_eq!(third_grant.task.fencing_token,3);
        assert_eq!(get(&pool,&ids[1],offer_a.handoff_id).await.unwrap().status,"CANCELLED");
        assert!(matches!(tasks::renew_lease(&pool,&ids[1],task_id,2,60).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::write_checkpoint(&pool,&ids[1],task_id,2,serde_json::json!({"stale_b":true})).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::complete_task(&pool,&ids[1],task_id,2,serde_json::json!({"stale_b":true})).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::release_lease(&pool,&ids[1],task_id,2).await,Err(tasks::TaskError::Fenced)));
        tasks::write_checkpoint(&pool,&ids[2],task_id,3,serde_json::json!({"resumed_by_c":true})).await.unwrap();
        tasks::complete_task(&pool,&ids[2],task_id,3,serde_json::json!({"done_by_c":true})).await.unwrap();
        assert_eq!(get(&pool,&ids[2],second_offer.handoff_id).await.unwrap().status,"COMPLETED");

        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn postgres_same_ai_phone_session_rotation_recovers_real_task_lease_and_fences_old_session() {
        let (pool,tenant,ids)=fixture().await;
        let identity_a=ids[0].clone();

        // Bind the same AI principal to one durable phone and attach endpoint A.
        let ai_id=Uuid::new_v4();
        sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','continuity-ai')")
            .bind(ai_id).bind(format!("test:continuity:{ai_id}")).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
            .bind(ai_id).bind(tenant).bind(identity_a.principal_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
            .bind(ai_id).bind(tenant).execute(&pool).await.unwrap();
        for permission in ["phone.read","phone.provision","phone.attach","phone.heartbeat","phone.detach"] {
            sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)")
                .bind(Uuid::new_v4()).bind(tenant).bind(identity_a.principal_id).bind(identity_a.room_id).bind(permission)
                .execute(&pool).await.unwrap();
        }
        let (provisioned_phone,created)=phone::provision_phone(&pool,&identity_a,phone::PhoneProvisionRequest { principal_id:identity_a.principal_id }).await.unwrap();
        assert!(created,"first explicit phone provision creates one durable phone");
        let phone_a=phone::get_my_phone(&pool,&identity_a).await.unwrap();
        assert_eq!(provisioned_phone.ai_phone_id,phone_a.phone.ai_phone_id);
        let endpoint_a=Uuid::new_v4();
        let attachment_a=phone::attach(&pool,&identity_a,AttachRequest {
            endpoint_id:endpoint_a,endpoint_type:"TEST".into(),metadata:serde_json::json!({"label":"continuity-A"})
        },false).await.unwrap();
        assert_eq!(attachment_a.endpoint_fence,1);

        // Session A owns a real Team Room task lease and writes a canonical checkpoint.
        let task=tasks::create_task(&pool,&identity_a,CreateTask {
            mission_id:Uuid::new_v4(),title:"AI-Phone same-identity continuity".into(),
            checkpoint:serde_json::json!({"step":"before_disconnect","progress":40})
        }).await.unwrap();
        let lease_a=tasks::acquire_lease(&pool,&identity_a,task.task_id,60).await.unwrap();
        let checkpoint_a=tasks::write_checkpoint(&pool,&identity_a,task.task_id,lease_a.task.fencing_token,
            serde_json::json!({"step":"checkpoint-A","progress":55})).await.unwrap();
        sqlx::query("INSERT INTO phone_mission_links(ai_phone_id,tenant_id,room_id,task_id) VALUES($1,$2,$3,$4)")
            .bind(phone_a.phone.ai_phone_id).bind(tenant).bind(identity_a.room_id).bind(task.task_id).execute(&pool).await.unwrap();
        sqlx::query("UPDATE ai_phones SET current_task_id=$1,current_mission_id=$2 WHERE ai_phone_id=$3 AND tenant_id=$4")
            .bind(task.task_id).bind(task.mission_id).bind(phone_a.phone.ai_phone_id).bind(tenant).execute(&pool).await.unwrap();

        // A expires. The phone remains durable; only its session attachment becomes stale.
        sqlx::query("UPDATE sessions SET status='EXPIRED',issued_at=NOW()-INTERVAL '2 minutes',expires_at=NOW()-INTERVAL '1 second' WHERE session_id=$1")
            .bind(identity_a.session_id).execute(&pool).await.unwrap();
        sqlx::query("UPDATE workload_executions SET status='EXPIRED',expires_at=NOW()-INTERVAL '1 second',heartbeat_at=NOW()-INTERVAL '2 minutes' WHERE execution_id=$1")
            .bind(identity_a.execution_id).execute(&pool).await.unwrap();
        sqlx::query("UPDATE team_tasks SET lease_expires_at=NOW()-INTERVAL '1 second' WHERE task_id=$1")
            .bind(task.task_id).execute(&pool).await.unwrap();
        assert_eq!(tasks::recover_expired(&pool,identity_a.room_id).await.unwrap(),1);

        // Session B is a new authenticated execution of the same AI principal.
        let session_b=Uuid::new_v4();
        let execution_b=Uuid::new_v4();
        let subject=identity_a.subject.clone();
        sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,rotated_from_session_id) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '20 minutes','ACTIVE',$5)")
            .bind(session_b).bind(tenant).bind(identity_a.room_id).bind(identity_a.principal_id).bind(identity_a.session_id)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '20 minutes','ACTIVE')")
            .bind(execution_b).bind(tenant).bind(identity_a.room_id).bind(identity_a.participant_id).bind(identity_a.principal_id).bind(session_b).bind(&subject)
            .execute(&pool).await.unwrap();
        sqlx::query("UPDATE room_memberships SET session_id=$1,last_seen_at=NOW() WHERE room_id=$2 AND principal_id=$3")
            .bind(session_b).bind(identity_a.room_id).bind(identity_a.principal_id).execute(&pool).await.unwrap();
        let identity_b=AuthenticatedIdentity {
            tenant_id:tenant,participant_id:identity_a.participant_id,principal_id:identity_a.principal_id,
            session_id:session_b,execution_id:execution_b,room_id:identity_a.room_id,
            issuer:identity_a.issuer.clone(),subject,
        };

        // B reconnects to the exact same AI_PHONE_ID using a new endpoint and phone fence.
        let endpoint_b=Uuid::new_v4();
        let attachment_b=phone::attach(&pool,&identity_b,AttachRequest {
            endpoint_id:endpoint_b,endpoint_type:"TEST".into(),metadata:serde_json::json!({"label":"continuity-B"})
        },false).await.unwrap();
        assert_eq!(attachment_b.ai_phone_id,phone_a.phone.ai_phone_id);
        assert_eq!(attachment_b.endpoint_fence,2);
        let snapshot=phone::get_my_phone(&pool,&identity_b).await.unwrap();
        assert_eq!(snapshot.phone.ai_phone_id,phone_a.phone.ai_phone_id);
        assert_eq!(snapshot.mission.as_ref().unwrap()["checkpoint"]["progress"],55);

        // Recovery Worker creates a normal, authorized task handoff to B's new session.
        // This does not bypass task recovery, task fencing, or the lease state machine.
        let offer=create_recovery(&pool,&ids[2],task.task_id,identity_b.principal_id,identity_b.participant_id,
            identity_b.session_id,identity_b.execution_id,"same AI identity reconnected on endpoint B".into()).await.unwrap();
        let lease_b=accept(&pool,&identity_b,offer.handoff_id,60).await.unwrap();
        assert_eq!(lease_b.task.fencing_token,lease_a.task.fencing_token+1);
        assert_eq!(lease_b.task.checkpoint,checkpoint_a.checkpoint);

        // A is stale after B acquires the new lease; every task mutation and phone operation is rejected.
        assert!(matches!(tasks::renew_lease(&pool,&identity_a,task.task_id,lease_a.task.fencing_token,60).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::write_checkpoint(&pool,&identity_a,task.task_id,lease_a.task.fencing_token,serde_json::json!({"stale":true})).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::complete_task(&pool,&identity_a,task.task_id,lease_a.task.fencing_token,serde_json::json!({"stale":true})).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(tasks::release_lease(&pool,&identity_a,task.task_id,lease_a.task.fencing_token).await,Err(tasks::TaskError::Fenced)));
        assert!(matches!(phone::heartbeat(&pool,&identity_a,attachment_a.endpoint_fence).await,Err(phone::PhoneError::InvalidSession)));
        let continued=tasks::write_checkpoint(&pool,&identity_b,task.task_id,lease_b.task.fencing_token,
            serde_json::json!({"step":"resumed-by-B","progress":80})).await.unwrap();
        assert_eq!(continued.checkpoint["step"],"resumed-by-B");
        let completed=tasks::complete_task(&pool,&identity_b,task.task_id,lease_b.task.fencing_token,
            serde_json::json!({"completed_by":"session-B"})).await.unwrap();
        assert_eq!(completed.status,"COMPLETED");

        println!("AI_PHONE_CONTINUITY_EVIDENCE={}",serde_json::json!({
            "ai_phone_id":phone_a.phone.ai_phone_id,"endpoint_a":endpoint_a,"endpoint_b":endpoint_b,
            "session_a":identity_a.session_id,"session_b":identity_b.session_id,
            "phone_fence_a":attachment_a.endpoint_fence,"phone_fence_b":attachment_b.endpoint_fence,
            "task_fence_a":lease_a.task.fencing_token,"task_fence_b":lease_b.task.fencing_token,
            "checkpoint_a_version":checkpoint_a.checkpoint_version,"checkpoint_b_version":continued.checkpoint_version,
            "a_renew":"REJECTED","a_checkpoint":"REJECTED","a_complete":"REJECTED","a_release":"REJECTED","a_phone_heartbeat":"REJECTED",
            "b_lease":"ACQUIRED","b_checkpoint":"ACCEPTED","b_complete":"ACCEPTED","final_status":completed.status
        }));
        sqlx::query("DELETE FROM phone_attachments WHERE ai_phone_id=$1").bind(phone_a.phone.ai_phone_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM phone_endpoints WHERE ai_phone_id=$1").bind(phone_a.phone.ai_phone_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM phone_messages WHERE ai_phone_id=$1").bind(phone_a.phone.ai_phone_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM phone_mission_links WHERE ai_phone_id=$1").bind(phone_a.phone.ai_phone_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_phones WHERE ai_phone_id=$1").bind(phone_a.phone.ai_phone_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_identity_tenants WHERE ai_id=$1 AND tenant_id=$2").bind(ai_id).bind(tenant).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1").bind(ai_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_identities WHERE ai_id=$1").bind(ai_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    }

}
