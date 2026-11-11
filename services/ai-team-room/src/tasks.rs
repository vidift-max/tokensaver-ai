use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::auth::AuthenticatedIdentity;

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("task not found or outside authenticated room")]
    NotFound,
    #[error("task is completed or cancelled")]
    Terminal,
    #[error("task has a live owner lease")]
    LeaseHeld,
    #[error("lease token is stale, expired, or belongs to another executor")]
    Fenced,
    #[error("invalid lease duration")]
    InvalidLease,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TaskLease {
    pub task_id: Uuid,
    pub mission_id: Uuid,
    pub status: String,
    pub owner_principal_id: Option<Uuid>,
    pub owner_participant_id: Option<Uuid>,
    pub owner_session_id: Option<Uuid>,
    pub fencing_token: i64,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub checkpoint: Value,
    pub checkpoint_version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaseGrant {
    pub task: TaskLease,
    pub lease_seconds: i64,
    pub reassigned: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTask {
    pub mission_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub checkpoint: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LeaseRequest {
    pub lease_seconds: Option<i64>,
    pub fencing_token: Option<i64>,
    pub checkpoint: Option<Value>,
}

async fn lock_task<'a>(
    tx: &'a mut sqlx::Transaction<'_, sqlx::Postgres>,
    room_id: Uuid,
    task_id: Uuid,
) -> Result<TaskLease, TaskError> {
    sqlx::query_as::<_, TaskLease>(
        "SELECT task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version FROM team_tasks WHERE task_id=$1 AND room_id=$2 FOR UPDATE"
    ).bind(task_id).bind(room_id).fetch_optional(&mut **tx).await?.ok_or(TaskError::NotFound)
}

async fn record_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task: &TaskLease,
    identity: &AuthenticatedIdentity,
    event_type: &str,
    metadata: Value,
) -> Result<(), TaskError> {
    sqlx::query("INSERT INTO task_lease_events (lease_event_id,tenant_id,room_id,task_id,principal_id,participant_id,session_id,fencing_token,event_type,metadata) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(Uuid::new_v4()).bind(identity.tenant_id).bind(identity.room_id)
        .bind(task.task_id).bind(identity.principal_id).bind(identity.participant_id)
        .bind(identity.session_id).bind(task.fencing_token).bind(event_type).bind(metadata)
        .execute(&mut **tx).await?;
    Ok(())
}

pub async fn create_task(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    request: CreateTask,
) -> Result<TaskLease, TaskError> {
    let title = request.title.trim();
    if title.is_empty() || title.len() > 500 { return Err(TaskError::InvalidLease); }
    let task_id = Uuid::new_v4();
    let checkpoint = if request.checkpoint.is_null() { serde_json::json!({}) } else { request.checkpoint };
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO team_tasks (task_id,tenant_id,room_id,mission_id,title,checkpoint,created_by_principal_id) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(task_id).bind(identity.tenant_id).bind(identity.room_id)
        .bind(request.mission_id).bind(title).bind(checkpoint).bind(identity.principal_id)
        .execute(&mut *tx).await?;
    let task = lock_task(&mut tx,identity.room_id,task_id).await?;
    tx.commit().await?;
    Ok(task)
}

pub async fn acquire_lease(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    lease_seconds: i64,
) -> Result<LeaseGrant, TaskError> {
    if !(15..=300).contains(&lease_seconds) { return Err(TaskError::InvalidLease); }
    let mut tx = pool.begin().await?;
    let old = lock_task(&mut tx,identity.room_id,task_id).await?;
    if matches!(old.status.as_str(),"COMPLETED"|"CANCELLED") { return Err(TaskError::Terminal); }
    let still_live: bool = sqlx::query_scalar("SELECT ($1 IN ('OWNED','REASSIGNED') AND COALESCE($2 > NOW(),FALSE))")
        .bind(&old.status).bind(old.lease_expires_at).fetch_one(&mut *tx).await?;
    if still_live { return Err(TaskError::LeaseHeld); }

    let previous_owner = old.owner_principal_id;
    let previous_token = old.fencing_token;
    let was_reassigned = previous_owner.is_some() || matches!(old.status.as_str(),"LEASE_EXPIRED"|"RECOVERY_REQUIRED"|"HANDOFF_PENDING");
    let task = sqlx::query_as::<_,TaskLease>(
        "UPDATE team_tasks SET status=CASE WHEN $4 THEN 'REASSIGNED' ELSE 'OWNED' END, owner_principal_id=$2, owner_participant_id=$3, owner_session_id=$5, owner_execution_id=$6, fencing_token=fencing_token+1, lease_issued_at=NOW(), lease_renewed_at=NOW(), lease_expires_at=NOW()+($7 * INTERVAL '1 second'), updated_at=NOW() WHERE task_id=$1 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(identity.principal_id).bind(identity.participant_id).bind(was_reassigned)
     .bind(identity.session_id).bind(identity.execution_id).bind(lease_seconds)
     .fetch_one(&mut *tx).await?;
    record_event(&mut tx,&task,identity,"ACQUIRED",serde_json::json!({"previous_owner":previous_owner,"previous_fencing_token":previous_token})).await?;

    if was_reassigned {
        let handoff_id=Uuid::new_v4();
        sqlx::query("INSERT INTO task_handoffs (handoff_id,task_id,tenant_id,from_principal_id,to_principal_id,from_fencing_token,to_fencing_token,status,context,accepted_at) VALUES ($1,$2,$3,$4,$5,$6,$7,'RESUMED',$8,NOW())")
          .bind(handoff_id).bind(task_id).bind(identity.tenant_id).bind(previous_owner)
          .bind(identity.principal_id).bind(previous_token).bind(task.fencing_token)
          .bind(serde_json::json!({"checkpoint":task.checkpoint,"mission_id":task.mission_id,"task_id":task.task_id}))
          .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(LeaseGrant { task, lease_seconds, reassigned:was_reassigned })
}

async fn validate_current_lease(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    fencing_token: i64,
) -> Result<TaskLease, TaskError> {
    let task=lock_task(tx,identity.room_id,task_id).await?;
    let valid: bool=sqlx::query_scalar(
        "SELECT ($1=$2 AND $3=$4 AND $5=$6 AND $7=$8 AND $9=$10 AND $11 IN ('OWNED','REASSIGNED') AND $12>NOW() AND EXISTS (SELECT 1 FROM sessions s JOIN workload_executions w ON w.session_id=s.session_id WHERE s.session_id=$8 AND s.principal_id=$4 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.execution_id=$10 AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW())"
    ).bind(task.task_id).bind(task_id).bind(task.owner_principal_id).bind(identity.principal_id)
     .bind(task.owner_participant_id).bind(identity.participant_id).bind(task.owner_session_id)
     .bind(identity.session_id).bind(task.owner_execution_id).bind(identity.execution_id)
     .bind(&task.status).bind(task.lease_expires_at)
     .fetch_one(&mut **tx).await?;
    if !valid || fencing_token!=task.fencing_token { return Err(TaskError::Fenced); }
    Ok(task)
}

pub async fn renew_lease(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    fencing_token: i64,
    lease_seconds: i64,
) -> Result<TaskLease, TaskError> {
    if !(15..=300).contains(&lease_seconds) { return Err(TaskError::InvalidLease); }
    let mut tx=pool.begin().await?;
    let task=validate_current_lease(&mut tx,identity,task_id,fencing_token).await?;
    let updated=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET lease_renewed_at=NOW(),lease_expires_at=NOW()+($3 * INTERVAL '1 second'),updated_at=NOW() WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(fencing_token).bind(lease_seconds).fetch_one(&mut *tx).await?;
    record_event(&mut tx,&updated,identity,"RENEWED",serde_json::json!({"lease_seconds":lease_seconds})).await?;
    tx.commit().await?;
    Ok(updated)
}

pub async fn write_checkpoint(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    fencing_token: i64,
    checkpoint: Value,
) -> Result<TaskLease, TaskError> {
    let mut tx=pool.begin().await?;
    let task=validate_current_lease(&mut tx,identity,task_id,fencing_token).await?;
    let updated=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET checkpoint=$3,checkpoint_version=checkpoint_version+1,updated_at=NOW() WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(fencing_token).bind(checkpoint).fetch_one(&mut *tx).await?;
    record_event(&mut tx,&updated,identity,"RENEWED",serde_json::json!({"checkpoint_version":updated.checkpoint_version})).await?;
    tx.commit().await?;
    let _=task;
    Ok(updated)
}

pub async fn complete_task(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    fencing_token: i64,
    final_checkpoint: Value,
) -> Result<TaskLease, TaskError> {
    let mut tx=pool.begin().await?;
    let _task=validate_current_lease(&mut tx,identity,task_id,fencing_token).await?;
    let updated=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET status='COMPLETED',checkpoint=$3,checkpoint_version=checkpoint_version+1,completed_at=NOW(),updated_at=NOW(),owner_principal_id=NULL,owner_participant_id=NULL,owner_session_id=NULL,owner_execution_id=NULL,lease_issued_at=NULL,lease_renewed_at=NULL,lease_expires_at=NULL WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(fencing_token).bind(final_checkpoint).fetch_one(&mut *tx).await?;
    record_event(&mut tx,&updated,identity,"COMPLETED",serde_json::json!({"checkpoint_version":updated.checkpoint_version})).await?;
    tx.commit().await?;
    Ok(updated)
}

pub async fn recover_expired(pool: &PgPool, room_id: Uuid) -> Result<u64, TaskError> {
    let mut tx=pool.begin().await?;
    let expired=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET status='RECOVERY_REQUIRED',updated_at=NOW() WHERE room_id=$1 AND status IN ('OWNED','REASSIGNED') AND lease_expires_at<=NOW() RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(room_id).fetch_all(&mut *tx).await?;
    for task in &expired {
      sqlx::query("INSERT INTO task_lease_events (lease_event_id,tenant_id,room_id,task_id,principal_id,participant_id,session_id,fencing_token,event_type,metadata) VALUES ($1,(SELECT tenant_id FROM team_tasks WHERE task_id=$2),$3,$2,$4,$5,$6,$7,'EXPIRED',$8)")
        .bind(Uuid::new_v4()).bind(task.task_id).bind(room_id).bind(task.owner_principal_id)
        .bind(task.owner_participant_id).bind(task.owner_session_id).bind(task.fencing_token)
        .bind(serde_json::json!({"checkpoint_version":task.checkpoint_version})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(expired.len() as u64)
}
