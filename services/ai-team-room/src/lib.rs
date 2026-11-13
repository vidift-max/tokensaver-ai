pub mod auth;
pub mod phone;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_AI_PARTICIPANTS: i64 = 15;

#[derive(Debug, Error)]
pub enum RoomServiceError {
    #[error("room not found or inactive")]
    RoomUnavailable,
    #[error("participant identity is not enabled")]
    ParticipantNotAuthorized,
    #[error("session is invalid, expired, revoked, or not bound to this room/principal")]
    InvalidSession,
    #[error("room capacity reached")]
    RoomCapacityReached,
    #[error("participant already connected with this session")]
    DuplicateActiveSession,
    #[error("participant membership not found")]
    MembershipNotFound,
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct AdmissionResult {
    pub membership_id: Uuid,
    pub participant_id: Uuid,
    pub principal_id: Uuid,
    pub session_id: Uuid,
    pub reconnected: bool,
    pub active_ai_count: i64,
}

#[derive(Debug, Clone, FromRow)]
struct RoomRow {
    tenant_id: Uuid,
    capacity_limit: i16,
    status: String,
}

#[derive(Debug, Clone, FromRow)]
struct MembershipRow {
    membership_id: Uuid,
    session_id: Uuid,
    status: String,
}

pub async fn connect_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// Atomically admits an already-authenticated principal with a session issued by
/// a trusted identity adapter. This function is not exposed as an unauthenticated
/// HTTP endpoint. Callers must derive all IDs from verified server-side auth state.
pub async fn admit_authenticated_participant(
    pool: &PgPool,
    room_id: Uuid,
    participant_id: Uuid,
    principal_id: Uuid,
    session_id: Uuid,
) -> Result<AdmissionResult, RoomServiceError> {
    let mut tx = pool.begin().await?;

    // Serialize all admission/leave operations for this room before counting.
    let room = sqlx::query_as::<_, RoomRow>(
        "SELECT tenant_id, capacity_limit, status FROM rooms WHERE room_id = $1 FOR UPDATE",
    )
    .bind(room_id)
    .fetch_optional(&mut *tx)
    .await?
    .filter(|r| r.status == "ACTIVE" && i64::from(r.capacity_limit) == MAX_AI_PARTICIPANTS)
    .ok_or(RoomServiceError::RoomUnavailable)?;

    let identity_ok: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1
            FROM participants p
            JOIN principals pr ON pr.tenant_id = p.tenant_id AND pr.principal_id = p.principal_id
            JOIN sessions s ON s.tenant_id = pr.tenant_id AND s.principal_id = pr.principal_id
            WHERE p.tenant_id = $1 AND p.participant_id = $2
              AND p.principal_id = $3 AND p.enabled = TRUE AND pr.enabled = TRUE
              AND pr.principal_kind = 'AI'
              AND s.room_id = $4 AND s.session_id = $5
              AND s.revoked_at IS NULL AND s.issued_at <= NOW() AND s.expires_at > NOW()
        )"#,
    )
    .bind(room.tenant_id)
    .bind(participant_id)
    .bind(principal_id)
    .bind(room_id)
    .bind(session_id)
    .fetch_one(&mut *tx)
    .await?;

    if !identity_ok {
        return Err(RoomServiceError::InvalidSession);
    }

    let existing = sqlx::query_as::<_, MembershipRow>(
        "SELECT membership_id, session_id, status FROM room_memberships WHERE room_id = $1 AND principal_id = $2 FOR UPDATE",
    )
    .bind(room_id)
    .bind(principal_id)
    .fetch_optional(&mut *tx)
    .await?;

    let (membership_id, reconnected) = if let Some(existing) = existing {
        if existing.status == "REVOKED" {
            return Err(RoomServiceError::ParticipantNotAuthorized);
        }
        if existing.status == "ACTIVE" && existing.session_id == session_id {
            return Err(RoomServiceError::DuplicateActiveSession);
        }
        if existing.status != "ACTIVE" {
            let active_ai_count: i64 = sqlx::query_scalar(
                r#"SELECT COUNT(*)
                   FROM room_memberships m
                   JOIN principals p ON p.tenant_id = m.tenant_id AND p.principal_id = m.principal_id
                   WHERE m.room_id = $1 AND m.status = 'ACTIVE' AND p.principal_kind = 'AI'"#,
            )
            .bind(room_id)
            .fetch_one(&mut *tx)
            .await?;
            if active_ai_count >= MAX_AI_PARTICIPANTS {
                return Err(RoomServiceError::RoomCapacityReached);
            }
        } else {
            sqlx::query(
                "UPDATE sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE room_id = $1 AND session_id = $2 AND session_id <> $3",
            )
            .bind(room_id)
            .bind(existing.session_id)
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE room_memberships SET participant_id = $1, session_id = $2, status = 'ACTIVE', joined_at = NOW(), last_seen_at = NOW(), left_at = NULL WHERE membership_id = $3",
        )
        .bind(participant_id)
        .bind(session_id)
        .bind(existing.membership_id)
        .execute(&mut *tx)
        .await?;
        (existing.membership_id, true)
    } else {
        let active_ai_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*)
               FROM room_memberships m
               JOIN principals p ON p.tenant_id = m.tenant_id AND p.principal_id = m.principal_id
               WHERE m.room_id = $1 AND m.status = 'ACTIVE' AND p.principal_kind = 'AI'"#,
        )
        .bind(room_id)
        .fetch_one(&mut *tx)
        .await?;

        if active_ai_count >= MAX_AI_PARTICIPANTS {
            return Err(RoomServiceError::RoomCapacityReached);
        }

        let membership_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO room_memberships (membership_id, tenant_id, room_id, participant_id, principal_id, session_id, status) VALUES ($1, $2, $3, $4, $5, $6, 'ACTIVE')",
        )
        .bind(membership_id)
        .bind(room.tenant_id)
        .bind(room_id)
        .bind(participant_id)
        .bind(principal_id)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        (membership_id, false)
    };

    let active_ai_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
           FROM room_memberships m
           JOIN principals p ON p.tenant_id = m.tenant_id AND p.principal_id = m.principal_id
           WHERE m.room_id = $1 AND m.status = 'ACTIVE' AND p.principal_kind = 'AI'"#,
    )
    .bind(room_id)
    .fetch_one(&mut *tx)
    .await?;

    let event_id = Uuid::new_v4();
    let action = if reconnected { "participant.reconnected" } else { "participant.admitted" };
    let metadata = serde_json::json!({
        "room_id": room_id,
        "participant_id": participant_id,
        "principal_id": principal_id,
        "session_id": session_id,
        "active_ai_count": active_ai_count,
        "reconnected": reconnected
    });
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id, tenant_id, room_id, actor_principal_id, action, resource_type, resource_id, outcome, metadata) VALUES ($1, $2, $3, $4, $5, 'room_membership', $6, 'SUCCESS', $7)",
    )
    .bind(event_id)
    .bind(room.tenant_id)
    .bind(room_id)
    .bind(principal_id)
    .bind(action)
    .bind(membership_id)
    .bind(metadata.clone())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO outbox_events (outbox_event_id, tenant_id, aggregate_type, aggregate_id, event_type, payload) VALUES ($1, $2, 'room', $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(room.tenant_id)
    .bind(room_id)
    .bind(action)
    .bind(metadata)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(AdmissionResult {
        membership_id,
        participant_id,
        principal_id,
        session_id,
        reconnected,
        active_ai_count,
    })
}

/// Marks a member as LEFT and revokes its room-scoped session in the same transaction.
pub async fn leave_participant(
    pool: &PgPool,
    room_id: Uuid,
    principal_id: Uuid,
) -> Result<(), RoomServiceError> {
    let mut tx = pool.begin().await?;
    let room = sqlx::query_as::<_, RoomRow>(
        "SELECT tenant_id, capacity_limit, status FROM rooms WHERE room_id = $1 FOR UPDATE",
    )
    .bind(room_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RoomServiceError::RoomUnavailable)?;

    let membership = sqlx::query_as::<_, MembershipRow>(
        "SELECT membership_id, session_id, status FROM room_memberships WHERE room_id = $1 AND principal_id = $2 AND status = 'ACTIVE' FOR UPDATE",
    )
    .bind(room_id)
    .bind(principal_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RoomServiceError::MembershipNotFound)?;

    sqlx::query("UPDATE sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE room_id = $1 AND session_id = $2")
        .bind(room_id).bind(membership.session_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE room_memberships SET status = 'LEFT', left_at = NOW(), last_seen_at = NOW() WHERE membership_id = $1")
        .bind(membership.membership_id).execute(&mut *tx).await?;

    let metadata = serde_json::json!({"room_id": room_id, "principal_id": principal_id, "membership_id": membership.membership_id});
    sqlx::query(
        "INSERT INTO audit_events (audit_event_id, tenant_id, room_id, actor_principal_id, action, resource_type, resource_id, outcome, metadata) VALUES ($1, $2, $3, $4, 'participant.left', 'room_membership', $5, 'SUCCESS', $6)",
    )
    .bind(Uuid::new_v4()).bind(room.tenant_id).bind(room_id).bind(principal_id)
    .bind(membership.membership_id).bind(metadata.clone()).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO outbox_events (outbox_event_id, tenant_id, aggregate_type, aggregate_id, event_type, payload) VALUES ($1, $2, 'room', $3, 'participant.left', $4)",
    )
    .bind(Uuid::new_v4()).bind(room.tenant_id).bind(room_id).bind(metadata)
    .execute(&mut *tx).await?;

    tx.commit().await?;
    Ok(())
}

pub async fn readiness(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(pool).await.is_ok()
}

pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
