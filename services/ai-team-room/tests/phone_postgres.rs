use ai_team_room_service::{
    auth::AuthenticatedIdentity,
    connect_pool, migrate,
    phone::{attach, detach, get_my_phone, AttachRequest, PhoneError},
};
use sqlx::PgPool;
use uuid::Uuid;

async fn grant(pool: &PgPool, tenant: Uuid, room: Uuid, principal: Uuid, permission: &str) {
    sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).bind(permission)
        .execute(pool).await.unwrap();
}

async fn add_session(pool: &PgPool, tenant: Uuid, room: Uuid, principal: Uuid, participant: Uuid)
    -> (Uuid, Uuid) {
    let session = Uuid::new_v4();
    let execution = Uuid::new_v4();
    sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES($1,$2,$3,$4,NOW()-INTERVAL '1 minute',NOW()+INTERVAL '20 minutes','ACTIVE')")
        .bind(session).bind(tenant).bind(room).bind(principal).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,'ai-phone-postgres-test',NOW(),NOW()+INTERVAL '20 minutes','ACTIVE')")
        .bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session)
        .execute(pool).await.unwrap();
    (session, execution)
}

#[tokio::test]
async fn phone_identity_survives_session_rotation_and_endpoint_change() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("BLOCKED: DATABASE_URL is not configured; PostgreSQL integration test was not simulated.");
        return;
    };
    let pool = connect_pool(&url).await.expect("connect PostgreSQL");
    migrate(&pool).await.expect("apply all Team Room migrations");

    let tenant = Uuid::new_v4();
    let room = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let participant = Uuid::new_v4();
    let ai_id = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'ai-phone-integration-test')")
        .bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'ai-phone-integration-room')")
        .bind(room).bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','ai-phone-test',$3)")
        .bind(principal).bind(tenant).bind(format!("test-subject-{principal}"))
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,'AI-PHONE-TEST-IDENTITY')")
        .bind(participant).bind(tenant).bind(principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','AI-PHONE-TEST-IDENTITY')")
        .bind(ai_id).bind(format!("test:ai-phone:{ai_id}")).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(tenant).bind(principal).execute(&pool).await.unwrap();

    for permission in ["phone.read","phone.attach","phone.detach"] {
        grant(&pool,tenant,room,principal,permission).await;
    }

    let (session_a, execution_a) = add_session(&pool,tenant,room,principal,participant).await;
    sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(participant).bind(principal).bind(session_a)
        .execute(&pool).await.unwrap();
    let identity_a = AuthenticatedIdentity {
        tenant_id: tenant, participant_id: participant, principal_id: principal,
        session_id: session_a, execution_id: execution_a, room_id: room,
        issuer: "test-only".into(), subject: "test-only".into(),
    };

    let first = get_my_phone(&pool,&identity_a).await.expect("create permanent phone");
    let endpoint_a = Uuid::new_v4();
    let attached_a = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: endpoint_a, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-A"}),
    },false).await.expect("attach endpoint A");
    assert_eq!(attached_a.endpoint_fence,1);

    // A different endpoint cannot attach while the previous authenticated session remains live.
    let blocked = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: Uuid::new_v4(), endpoint_type: "TEST".into(), metadata: serde_json::json!({}),
    },false).await;
    assert!(matches!(blocked,Err(PhoneError::Conflict)));

    // Session A expires. Its phone attachment remains persisted, but it is no longer authoritative.
    sqlx::query("UPDATE sessions SET expires_at=NOW()-INTERVAL '1 second',status='EXPIRED' WHERE session_id=$1")
        .bind(session_a).execute(&pool).await.unwrap();
    sqlx::query("UPDATE workload_executions SET expires_at=NOW()-INTERVAL '1 second',status='EXPIRED' WHERE execution_id=$1")
        .bind(execution_a).execute(&pool).await.unwrap();

    let (session_b, execution_b) = add_session(&pool,tenant,room,principal,participant).await;
    sqlx::query("UPDATE room_memberships SET session_id=$1,last_seen_at=NOW() WHERE room_id=$2 AND principal_id=$3")
        .bind(session_b).bind(room).bind(principal).execute(&pool).await.unwrap();
    let identity_b = AuthenticatedIdentity {
        tenant_id: tenant, participant_id: participant, principal_id: principal,
        session_id: session_b, execution_id: execution_b, room_id: room,
        issuer: "test-only".into(), subject: "test-only".into(),
    };
    let endpoint_b = Uuid::new_v4();
    let attached_b = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_b, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-B"}),
    },false).await.expect("reconnect after session expiry");
    assert_eq!(attached_b.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(attached_b.endpoint_fence,2);
    assert_ne!(attached_b.endpoint_id,attached_a.endpoint_id);

    // The old endpoint/session cannot detach or mutate the newly fenced phone attachment.
    let stale = detach(&pool,&identity_a,attached_a.endpoint_fence).await;
    assert!(matches!(stale,Err(PhoneError::InvalidSession)));
    detach(&pool,&identity_b,attached_b.endpoint_fence).await.expect("detach current endpoint");

    // Persistent offline inbox: no active attachment is required to enqueue a message.
    let sender = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'HUMAN','test-owner',$3)")
        .bind(sender).bind(tenant).bind(format!("owner-{sender}")).execute(&pool).await.unwrap();
    let message_id = Uuid::new_v4();
    sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,1,'offline-msg-1',$6)")
        .bind(message_id).bind(first.phone.ai_phone_id).bind(tenant).bind(sender).bind(ai_id)
        .bind(serde_json::json!({"text":"resume mission"})).execute(&pool).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(state,"PENDING");

    let duplicate = sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,2,'offline-msg-1',$6) ON CONFLICT(ai_phone_id,sender_principal_id,idempotency_key) DO NOTHING")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(tenant).bind(sender).bind(ai_id)
        .bind(serde_json::json!({"text":"resume mission"})).execute(&pool).await.unwrap();
    assert_eq!(duplicate.rows_affected(),0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM phone_messages WHERE ai_phone_id=$1 AND sender_principal_id=$2 AND idempotency_key='offline-msg-1'")
        .bind(first.phone.ai_phone_id).bind(sender).fetch_one(&pool).await.unwrap();
    assert_eq!(count,1);

    // Reconnect a fresh endpoint and verify the offline message remains in sequence.
    let endpoint_c = Uuid::new_v4();
    let attached_c = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_c, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-C"}),
    },false).await.expect("reconnect to retrieve offline inbox");
    assert_eq!(attached_c.ai_phone_id,first.phone.ai_phone_id);
    let pending: (i64,String) = sqlx::query_as("SELECT sequence_no,message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(pending,(1,"PENDING".to_string()));
    sqlx::query("UPDATE phone_messages SET message_state='DELIVERED',delivered_at=NOW(),attempt_count=attempt_count+1 WHERE message_id=$1")
        .bind(message_id).execute(&pool).await.unwrap();
    sqlx::query("UPDATE phone_messages SET message_state='ACK',acknowledged_at=NOW() WHERE message_id=$1")
        .bind(message_id).execute(&pool).await.unwrap();
    let acked: String = sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(acked,"ACK");

    // The phone points to the existing canonical task/checkpoint record; it does not copy it.
    let task_id = Uuid::new_v4();
    let mission_id = Uuid::new_v4();
    sqlx::query("INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id,checkpoint,checkpoint_version) VALUES($1,$2,$3,$4,'AI-Phone continuity test',$5,$6,1)")
        .bind(task_id).bind(tenant).bind(room).bind(mission_id).bind(principal)
        .bind(serde_json::json!({"step":"persisted","progress":40})).execute(&pool).await.unwrap();
    sqlx::query("UPDATE ai_phones SET current_task_id=$1,current_mission_id=$2 WHERE ai_phone_id=$3")
        .bind(task_id).bind(mission_id).bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    let current = get_my_phone(&pool,&identity_b).await.expect("same phone and mission after reconnect");
    assert_eq!(current.phone.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(current.phone.ai_id,ai_id);
    assert_eq!(current.phone.current_task_id,Some(task_id));
    assert_eq!(current.mission.as_ref().unwrap()["checkpoint"]["progress"],40);

}
