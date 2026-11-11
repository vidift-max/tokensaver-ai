use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use ai_team_room_service::auth::AuthenticatedIdentity;

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
    pub owner_execution_id: Option<Uuid>,
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
        "SELECT task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version FROM team_tasks WHERE task_id=$1 AND room_id=$2 FOR UPDATE"
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
    let actor_valid: bool = sqlx::query_scalar(
      "SELECT EXISTS (SELECT 1 FROM sessions s JOIN workload_executions w ON w.session_id=s.session_id AND w.execution_id=$4 JOIN room_memberships m ON m.room_id=s.room_id AND m.principal_id=s.principal_id AND m.session_id=s.session_id JOIN principals p ON p.tenant_id=s.tenant_id AND p.principal_id=s.principal_id JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id WHERE s.session_id=$1 AND s.room_id=$2 AND s.principal_id=$3 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND m.status='ACTIVE' AND p.enabled=TRUE AND wi.status='ACTIVE')"
    ).bind(identity.session_id).bind(identity.room_id).bind(identity.principal_id).bind(identity.execution_id).fetch_one(&mut *tx).await?;
    if !actor_valid { return Err(TaskError::Fenced); }
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
    let actor_valid: bool = sqlx::query_scalar(
      "SELECT EXISTS (SELECT 1 FROM sessions s JOIN workload_executions w ON w.session_id=s.session_id AND w.execution_id=$4 JOIN room_memberships m ON m.room_id=s.room_id AND m.principal_id=s.principal_id AND m.session_id=s.session_id JOIN principals p ON p.tenant_id=s.tenant_id AND p.principal_id=s.principal_id JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id WHERE s.session_id=$1 AND s.room_id=$2 AND s.principal_id=$3 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND m.status='ACTIVE' AND p.enabled=TRUE AND wi.status='ACTIVE')"
    ).bind(identity.session_id).bind(identity.room_id).bind(identity.principal_id).bind(identity.execution_id).fetch_one(&mut *tx).await?;
    if !actor_valid { return Err(TaskError::Fenced); }
    if matches!(old.status.as_str(),"COMPLETED"|"CANCELLED") { return Err(TaskError::Terminal); }
    let still_live: bool = sqlx::query_scalar("SELECT ($1 IN ('OWNED','REASSIGNED') AND COALESCE($2 > NOW(),FALSE))")
        .bind(&old.status).bind(old.lease_expires_at).fetch_one(&mut *tx).await?;
    if still_live { return Err(TaskError::LeaseHeld); }

    let previous_owner = old.owner_principal_id;
    let previous_token = old.fencing_token;
    let was_reassigned = previous_owner.is_some() || matches!(old.status.as_str(),"LEASE_EXPIRED"|"RECOVERY_REQUIRED"|"HANDOFF_PENDING");
    let task = sqlx::query_as::<_,TaskLease>(
        "UPDATE team_tasks SET status=CASE WHEN $4 THEN 'REASSIGNED' ELSE 'OWNED' END, owner_principal_id=$2, owner_participant_id=$3, owner_session_id=$5, owner_execution_id=$6, fencing_token=fencing_token+1, lease_issued_at=NOW(), lease_renewed_at=NOW(), lease_expires_at=NOW()+($7 * INTERVAL '1 second'), updated_at=NOW() WHERE task_id=$1 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
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
        "SELECT ($1=$2 AND $3=$4 AND $5=$6 AND $7=$8 AND $9=$10 AND $11 IN ('OWNED','REASSIGNED') AND $12>NOW() AND EXISTS (SELECT 1 FROM sessions s JOIN workload_executions w ON w.session_id=s.session_id JOIN room_memberships m ON m.room_id=s.room_id AND m.principal_id=s.principal_id AND m.session_id=s.session_id JOIN principals p ON p.tenant_id=s.tenant_id AND p.principal_id=s.principal_id JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id WHERE s.session_id=$8 AND s.principal_id=$4 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.execution_id=$10 AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND m.status='ACTIVE' AND p.enabled=TRUE AND wi.status='ACTIVE'))"
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
    let _task=validate_current_lease(&mut tx,identity,task_id,fencing_token).await?;
    let updated=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET lease_renewed_at=NOW(),lease_expires_at=NOW()+($3 * INTERVAL '1 second'),updated_at=NOW() WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
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
    let _task=validate_current_lease(&mut tx,identity,task_id,fencing_token).await?;
    let updated=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET checkpoint=$3,checkpoint_version=checkpoint_version+1,updated_at=NOW() WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(fencing_token).bind(checkpoint).fetch_one(&mut *tx).await?;
    record_event(&mut tx,&updated,identity,"CHECKPOINTED",serde_json::json!({"checkpoint_version":updated.checkpoint_version})).await?;
    tx.commit().await?;
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
      "UPDATE team_tasks SET status='COMPLETED',checkpoint=$3,checkpoint_version=checkpoint_version+1,completed_at=NOW(),updated_at=NOW(),owner_principal_id=NULL,owner_participant_id=NULL,owner_session_id=NULL,owner_execution_id=NULL,lease_issued_at=NULL,lease_renewed_at=NULL,lease_expires_at=NULL WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(fencing_token).bind(final_checkpoint).fetch_one(&mut *tx).await?;
    record_event(&mut tx,&updated,identity,"COMPLETED",serde_json::json!({"checkpoint_version":updated.checkpoint_version})).await?;
    tx.commit().await?;
    Ok(updated)
}

pub async fn recover_expired(pool: &PgPool, room_id: Uuid) -> Result<u64, TaskError> {
    let mut tx=pool.begin().await?;
    let expired=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET status='RECOVERY_REQUIRED',updated_at=NOW() WHERE room_id=$1 AND status IN ('OWNED','REASSIGNED') AND lease_expires_at<=NOW() RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
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

pub async fn release_lease(
    pool: &PgPool,
    identity: &AuthenticatedIdentity,
    task_id: Uuid,
    fencing_token: i64,
) -> Result<TaskLease, TaskError> {
    let mut tx=pool.begin().await?;
    let task=validate_current_lease(&mut tx,identity,task_id,fencing_token).await?;
    let updated=sqlx::query_as::<_,TaskLease>(
      "UPDATE team_tasks SET status='UNOWNED',owner_principal_id=NULL,owner_participant_id=NULL,owner_session_id=NULL,owner_execution_id=NULL,lease_issued_at=NULL,lease_renewed_at=NULL,lease_expires_at=NULL,updated_at=NOW() WHERE task_id=$1 AND fencing_token=$2 RETURNING task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version"
    ).bind(task_id).bind(fencing_token).fetch_one(&mut *tx).await?;
    record_event(&mut tx,&task,identity,"RELEASED",serde_json::json!({})).await?;
    tx.commit().await?;
    Ok(updated)
}



#[cfg(test)]
mod distributed_lease_tests {
    use super::*;
    use ai_team_room_service::{connect_pool,migrate};
    use std::sync::Arc;

    async fn fixture() -> (PgPool,Uuid,Vec<AuthenticatedIdentity>) {
        let url=std::env::var("DATABASE_URL").expect("PostgreSQL integration tests require DATABASE_URL");
        let pool=connect_pool(&url).await.unwrap();
        migrate(&pool).await.unwrap();
        let tenant=Uuid::new_v4();
        let room=Uuid::new_v4();
        sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'distributed-lease-test')").bind(tenant).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'lease-room')").bind(room).bind(tenant).execute(&pool).await.unwrap();
        let mut identities=Vec::new();
        for n in 0..10 {
            let principal=Uuid::new_v4();
            let participant=Uuid::new_v4();
            let workload=Uuid::new_v4();
            let session=Uuid::new_v4();
            let execution=Uuid::new_v4();
            let subject=format!("lease-test-{n}");
            sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','test-issuer',$3)")
                .bind(principal).bind(tenant).bind(&subject).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)")
                .bind(participant).bind(tenant).bind(principal).bind(format!("lease executor {n}")).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'test-issuer',$4,'ACTIVE')")
                .bind(workload).bind(tenant).bind(principal).bind(&subject).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE')")
                .bind(session).bind(tenant).bind(room).bind(principal).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE')")
                .bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).bind(&subject).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
                .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).execute(&pool).await.unwrap();
            identities.push(AuthenticatedIdentity{tenant_id:tenant,participant_id:participant,principal_id:principal,session_id:session,execution_id:execution,room_id:room,issuer:"test-issuer".into(),subject});
        }
        (pool,tenant,identities)
    }

    #[tokio::test]
    async fn ten_concurrent_executors_one_winner_then_expiry_reassignment_and_old_fence_rejection() {
        let (pool,tenant,identities)=fixture().await;
        let task=create_task(&pool,&identities[0],CreateTask{mission_id:Uuid::new_v4(),title:"distributed fencing E2E".into(),checkpoint:serde_json::json!({"progress":41,"next_action":"resume"})}).await.unwrap();
        let pool=Arc::new(pool);
        let mut attempts=Vec::new();
        for identity in identities.iter().cloned() {
            let p=pool.clone();
            let task_id=task.task_id;
            attempts.push(tokio::spawn(async move { acquire_lease(&p,&identity,task_id,30).await.map(|grant|(identity,grant)) }));
        }
        let mut winners=Vec::new();
        for attempt in attempts {
            if let Ok(Ok(winner))=attempt.await { winners.push(winner); }
        }
        assert_eq!(winners.len(),1,"exactly one executor may own the task");
        let (old_identity,old_grant)=winners.remove(0);
        assert_eq!(old_grant.task.fencing_token,1);
        let renewed=renew_lease(&pool,&old_identity,task.task_id,old_grant.task.fencing_token,45).await.unwrap();
        assert_eq!(renewed.fencing_token,1);
        assert!(renewed.lease_expires_at.unwrap()>old_grant.task.lease_expires_at.unwrap());

        sqlx::query("UPDATE team_tasks SET lease_expires_at=NOW()-INTERVAL '1 second' WHERE task_id=$1")
            .bind(task.task_id).execute(&*pool).await.unwrap();
        assert_eq!(recover_expired(&pool,task_room(&pool,task.task_id).await).await.unwrap(),1);
        let replacement=identities.iter().find(|id|id.principal_id!=old_identity.principal_id).unwrap().clone();
        let new_grant=acquire_lease(&pool,&replacement,task.task_id,30).await.unwrap();
        assert!(new_grant.reassigned);
        assert_eq!(new_grant.task.fencing_token,2);
        assert_eq!(new_grant.task.checkpoint["progress"],41);
        let downgrade=sqlx::query("UPDATE team_tasks SET fencing_token=1 WHERE task_id=$1")
            .bind(task.task_id).execute(&*pool).await;
        assert!(downgrade.is_err(),"PostgreSQL trigger must reject fencing token rollback");

        let stale=write_checkpoint(&pool,&old_identity,task.task_id,old_grant.task.fencing_token,serde_json::json!({"corrupt":true})).await;
        assert!(matches!(stale,Err(TaskError::Fenced)));
        let future=write_checkpoint(&pool,&replacement,task.task_id,99,serde_json::json!({"future":true})).await;
        assert!(matches!(future,Err(TaskError::Fenced)));

        let completed=complete_task(&pool,&replacement,task.task_id,new_grant.task.fencing_token,serde_json::json!({"done":true})).await.unwrap();
        assert_eq!(completed.status,"COMPLETED");
        let reopen=write_checkpoint(&pool,&replacement,task.task_id,new_grant.task.fencing_token,serde_json::json!({"reopen":true})).await;
        assert!(matches!(reopen,Err(TaskError::Fenced)));

        let handoffs:i64=sqlx::query_scalar("SELECT COUNT(*) FROM task_handoffs WHERE task_id=$1 AND status='RESUMED' AND from_fencing_token=1 AND to_fencing_token=2")
            .bind(task.task_id).fetch_one(&*pool).await.unwrap();
        assert_eq!(handoffs,1);
        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&*pool).await.unwrap();
    }

    async fn task_room(pool:&PgPool,task_id:Uuid)->Uuid {
        sqlx::query_scalar("SELECT room_id FROM team_tasks WHERE task_id=$1").bind(task_id).fetch_one(pool).await.unwrap()
    }
}

#[derive(Debug,Clone,Serialize,FromRow)]
pub struct TaskCandidate {
    pub principal_id: Uuid,
    pub participant_id: Uuid,
    pub session_id: Uuid,
    pub execution_id: Uuid,
    pub last_seen_at: DateTime<Utc>,
    pub active_lease_count: i64,
}

pub async fn list_candidates(
    pool: &PgPool,
    tenant_id: Uuid,
    room_id: Uuid,
    task_id: Uuid,
) -> Result<Vec<TaskCandidate>,TaskError> {
    let candidates=sqlx::query_as::<_,TaskCandidate>(
      "SELECT p.principal_id, m.participant_id, s.session_id, w.execution_id, m.last_seen_at, (SELECT COUNT(*) FROM team_tasks t WHERE t.owner_principal_id=p.principal_id AND t.status IN ('OWNED','REASSIGNED') AND t.lease_expires_at>NOW()) AS active_lease_count FROM room_memberships m JOIN principals p ON p.tenant_id=m.tenant_id AND p.principal_id=m.principal_id JOIN participants pa ON pa.tenant_id=m.tenant_id AND pa.participant_id=m.participant_id AND pa.principal_id=m.principal_id JOIN sessions s ON s.room_id=m.room_id AND s.session_id=m.session_id AND s.principal_id=m.principal_id JOIN workload_executions w ON w.session_id=s.session_id AND w.principal_id=p.principal_id AND w.participant_id=pa.participant_id JOIN workload_identities wi ON wi.tenant_id=p.tenant_id AND wi.principal_id=p.principal_id JOIN principal_permissions pp ON pp.tenant_id=p.tenant_id AND pp.principal_id=p.principal_id AND pp.room_id=m.room_id AND pp.permission='task.transition' AND pp.revoked_at IS NULL WHERE m.tenant_id=$1 AND m.room_id=$2 AND m.status='ACTIVE' AND m.last_seen_at>NOW()-INTERVAL '90 seconds' AND p.enabled=TRUE AND pa.enabled=TRUE AND wi.status='ACTIVE' AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND w.heartbeat_at>NOW()-INTERVAL '90 seconds' AND EXISTS (SELECT 1 FROM team_tasks target WHERE target.task_id=$3 AND target.room_id=m.room_id AND target.status IN ('LEASE_EXPIRED','RECOVERY_REQUIRED','HANDOFF_PENDING','UNOWNED')) ORDER BY active_lease_count,m.last_seen_at DESC"
    ).bind(tenant_id).bind(room_id).bind(task_id).fetch_all(pool).await?;
    Ok(candidates.into_iter().filter(|candidate|candidate.active_lease_count<3).collect())
}
