use ai_team_room_service::{
    admit_authenticated_participant, connect_pool, leave_participant, migrate, RoomServiceError,
};
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    pool: PgPool,
    tenant_id: Uuid,
    room_id: Uuid,
    participants: Vec<(Uuid, Uuid, Uuid)>, // participant_id, principal_id, session_id
}

async fn fixture(count: usize) -> Fixture {
    let url = std::env::var("DATABASE_URL").expect("CI must provide PostgreSQL DATABASE_URL");
    let pool = connect_pool(&url).await.expect("connect PostgreSQL");
    migrate(&pool).await.expect("apply migrations");

    let tenant_id = Uuid::new_v4();
    let room_id = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants (tenant_id, name) VALUES ($1, 'integration-test')")
        .bind(tenant_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms (room_id, tenant_id, name, capacity_limit) VALUES ($1, $2, 'integration-room', 15)")
        .bind(room_id).bind(tenant_id).execute(&pool).await.unwrap();

    let mut participants = Vec::with_capacity(count);
    for _ in 0..count {
        let principal_id = Uuid::new_v4();
        let participant_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        sqlx::query("INSERT INTO principals (principal_id, tenant_id, principal_kind, provider, external_subject) VALUES ($1, $2, 'AI', 'test-provider', $3)")
            .bind(principal_id).bind(tenant_id).bind(Uuid::new_v4().to_string())
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO participants (participant_id, tenant_id, principal_id, display_name) VALUES ($1, $2, $3, 'Integration AI')")
            .bind(participant_id).bind(tenant_id).bind(principal_id)
            .execute(&pool).await.unwrap();
        insert_session(&pool, tenant_id, room_id, principal_id, session_id).await;
        participants.push((participant_id, principal_id, session_id));
    }

    Fixture { pool, tenant_id, room_id, participants }
}

async fn insert_session(pool: &PgPool, tenant_id: Uuid, room_id: Uuid, principal_id: Uuid, session_id: Uuid) {
    sqlx::query("INSERT INTO sessions (session_id, tenant_id, room_id, principal_id, issued_at, expires_at) VALUES ($1, $2, $3, $4, NOW() - INTERVAL '1 minute', NOW() + INTERVAL '10 minutes')")
        .bind(session_id).bind(tenant_id).bind(room_id).bind(principal_id)
        .execute(pool).await.unwrap();
}

async fn cleanup(f: &Fixture) {
    sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
        .bind(f.tenant_id).execute(&f.pool).await.unwrap();
}

#[tokio::test]
async fn admission_enforces_15_exit_reentry_reconnect_and_duplicate_session() {
    let f = fixture(16).await;

    // 1 -> 14 -> 15 is accepted.
    for index in 0..15 {
        let (participant_id, principal_id, session_id) = f.participants[index];
        let result = admit_authenticated_participant(&f.pool, f.room_id, participant_id, principal_id, session_id)
            .await.expect("admission under capacity");
        assert_eq!(result.active_ai_count, (index + 1) as i64);
    }

    // The 16th identity is denied.
    let (participant_id, principal_id, session_id) = f.participants[15];
    assert!(matches!(
        admit_authenticated_participant(&f.pool, f.room_id, participant_id, principal_id, session_id).await,
        Err(RoomServiceError::RoomCapacityReached)
    ));

    // Reusing the same active session cannot create a second connection.
    let (participant_id, principal_id, session_id) = f.participants[14];
    assert!(matches!(
        admit_authenticated_participant(&f.pool, f.room_id, participant_id, principal_id, session_id).await,
        Err(RoomServiceError::DuplicateActiveSession)
    ));

    // Leaving frees exactly one seat; the previously rejected participant can enter.
    let (_, leaving_principal, _) = f.participants[0];
    leave_participant(&f.pool, f.room_id, leaving_principal).await.unwrap();
    let (p16, principal16, session16) = f.participants[15];
    let admitted = admit_authenticated_participant(&f.pool, f.room_id, p16, principal16, session16).await.unwrap();
    assert_eq!(admitted.active_ai_count, 15);

    // Re-entry of a previously LEFT principal must still respect capacity.
    let (p1, principal1, _) = f.participants[0];
    let reentry_session = Uuid::new_v4();
    insert_session(&f.pool, f.tenant_id, f.room_id, principal1, reentry_session).await;
    assert!(matches!(
        admit_authenticated_participant(&f.pool, f.room_id, p1, principal1, reentry_session).await,
        Err(RoomServiceError::RoomCapacityReached)
    ));

    // Once another participant leaves, the prior member can re-enter.
    let (_, leaving_principal2, _) = f.participants[1];
    leave_participant(&f.pool, f.room_id, leaving_principal2).await.unwrap();
    let reentered = admit_authenticated_participant(&f.pool, f.room_id, p1, principal1, reentry_session).await.unwrap();
    assert!(reentered.reconnected);
    assert_eq!(reentered.active_ai_count, 15);

    // A fresh room-scoped session for an already-active principal reconnects without
    // allocating another membership/seat and revokes the prior session.
    let (p3, principal3, old_session) = f.participants[2];
    let new_session = Uuid::new_v4();
    insert_session(&f.pool, f.tenant_id, f.room_id, principal3, new_session).await;
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_memberships WHERE room_id = $1 AND status = 'ACTIVE'")
        .bind(f.room_id).fetch_one(&f.pool).await.unwrap();
    let reconnected = admit_authenticated_participant(&f.pool, f.room_id, p3, principal3, new_session).await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_memberships WHERE room_id = $1 AND status = 'ACTIVE'")
        .bind(f.room_id).fetch_one(&f.pool).await.unwrap();
    assert!(reconnected.reconnected);
    assert_eq!(before, after);
    let old_revoked: bool = sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM sessions WHERE session_id = $1")
        .bind(old_session).fetch_one(&f.pool).await.unwrap();
    assert!(old_revoked);

    cleanup(&f).await;
}

#[tokio::test]
async fn sixteen_simultaneous_admissions_never_exceed_fifteen() {
    let f = fixture(16).await;
    let mut tasks = tokio::task::JoinSet::new();

    for (participant_id, principal_id, session_id) in f.participants.iter().copied() {
        let pool = f.pool.clone();
        let room_id = f.room_id;
        tasks.spawn(async move {
            admit_authenticated_participant(&pool, room_id, participant_id, principal_id, session_id).await
        });
    }

    let mut admitted = 0;
    let mut capacity_rejections = 0;
    while let Some(result) = tasks.join_next().await {
        match result.expect("task join") {
            Ok(_) => admitted += 1,
            Err(RoomServiceError::RoomCapacityReached) => capacity_rejections += 1,
            Err(other) => panic!("unexpected admission result: {other}"),
        }
    }

    assert_eq!(admitted, 15);
    assert_eq!(capacity_rejections, 1);
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_memberships WHERE room_id = $1 AND status = 'ACTIVE'")
        .bind(f.room_id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(active, 15);

    cleanup(&f).await;
}


#[tokio::test]
async fn admission_rejects_unbound_expired_and_disabled_identity_sessions() {
    let f = fixture(1).await;
    let (participant_id, principal_id, session_id) = f.participants[0];

    // A session ID that is not bound to this principal/room is rejected.
    assert!(matches!(
        admit_authenticated_participant(&f.pool, f.room_id, participant_id, principal_id, Uuid::new_v4()).await,
        Err(RoomServiceError::InvalidSession)
    ));

    // Expired session is rejected and creates no membership.
    sqlx::query("UPDATE sessions SET expires_at = NOW() - INTERVAL '1 second' WHERE session_id = $1")
        .bind(session_id).execute(&f.pool).await.unwrap();
    assert!(matches!(
        admit_authenticated_participant(&f.pool, f.room_id, participant_id, principal_id, session_id).await,
        Err(RoomServiceError::InvalidSession)
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_memberships WHERE room_id = $1")
        .bind(f.room_id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 0);

    // Re-enable a fresh session but disable the participant: identity still fails.
    let fresh_session = Uuid::new_v4();
    insert_session(&f.pool, f.tenant_id, f.room_id, principal_id, fresh_session).await;
    sqlx::query("UPDATE participants SET enabled = FALSE WHERE participant_id = $1")
        .bind(participant_id).execute(&f.pool).await.unwrap();
    assert!(matches!(
        admit_authenticated_participant(&f.pool, f.room_id, participant_id, principal_id, fresh_session).await,
        Err(RoomServiceError::InvalidSession)
    ));

    cleanup(&f).await;
}
