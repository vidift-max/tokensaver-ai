use ai_team_room_service::{
    admit_authenticated_participant, connect_pool, migrate,
    phone::router as phone_router,
};
use axum::{body::Body, http::{Request, StatusCode}, response::Response, Router};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use std::collections::HashMap;

#[derive(Clone)]
struct Actor {
    tenant_id: Uuid,
    room_id: Uuid,
    principal_id: Uuid,
    participant_id: Uuid,
    session_id: Uuid,
    execution_id: Uuid,
    ai_id: Uuid,
    token: String,
}

async fn request(app: &Router, method: &str, uri: &str, token: &str, body: Option<Value>) -> Response {
    let mut builder = Request::builder().method(method).uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if body.is_some() { builder = builder.header("content-type", "application/json"); }
    app.clone().oneshot(builder.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default())).unwrap())
        .await.unwrap()
}

async fn grant(pool: &PgPool, tenant: Uuid, room: Uuid, principal: Uuid, permission: &str) {
    sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).bind(permission)
        .execute(pool).await.unwrap();
}

async fn seed_actor(pool: &PgPool, tenant: Uuid, room: Uuid, index: usize) -> Actor {
    let principal_id = Uuid::new_v4();
    let participant_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let execution_id = Uuid::new_v4();
    let ai_id = Uuid::new_v4();
    let subject = format!("scale-ai-{index}-{principal_id}");
    let token = format!("scale-session-{session_id}");
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();

    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','scale-1-to-7',$3)")
        .bind(principal_id).bind(tenant).bind(&subject).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)")
        .bind(participant_id).bind(tenant).bind(principal_id).bind(format!("Scale AI #{index}"))
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'scale-1-to-7',$4,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal_id).bind(&subject)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '30 minutes','ACTIVE',$5)")
        .bind(session_id).bind(tenant).bind(room).bind(principal_id).bind(token_hash)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '30 minutes','ACTIVE')")
        .bind(execution_id).bind(tenant).bind(room).bind(participant_id).bind(principal_id).bind(session_id)
        .bind(format!("scale-worker-{index}")).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST',$3)")
        .bind(ai_id).bind(format!("scale-1-to-7:{ai_id}")).bind(format!("Scale AI #{index}"))
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(tenant).bind(principal_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
        .bind(ai_id).bind(tenant).execute(pool).await.unwrap();

    for permission in [
        "phone.provision", "phone.read", "phone.attach", "phone.detach",
        "phone.heartbeat", "phone.message.read", "phone.message.send",
        "phone.message.ack", "room.read", "room.write",
    ] {
        grant(pool, tenant, room, principal_id, permission).await;
    }
    Actor { tenant_id: tenant, room_id: room, principal_id, participant_id,
        session_id, execution_id, ai_id, token }
}

#[tokio::test]
async fn disposable_scale_one_to_seven_provisions_and_operates_concurrently() {
    let database = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured; scale test must not silently skip");
    let pool = connect_pool(&database).await.expect("connect disposable PostgreSQL 16");
    migrate(&pool).await.expect("apply all migrations");

    let tenant = Uuid::new_v4();
    let room = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'DISPOSABLE scale 1-to-7')")
        .bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name,capacity_limit) VALUES($1,$2,'DISPOSABLE scale room',15)")
        .bind(room).bind(tenant).execute(&pool).await.unwrap();

    let mut actors = Vec::new();
    for index in 1..=7 {
        actors.push(seed_actor(&pool, tenant, room, index).await);
    }

    // Seven room admissions execute concurrently and are serialized by the
    // existing room-row lock. This creates seven distinct active memberships.
    let mut admissions = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let pool = pool.clone();
        admissions.spawn(async move {
            admit_authenticated_participant(&pool, actor.room_id, actor.participant_id,
                actor.principal_id, actor.session_id).await
        });
    }
    let mut admitted = 0;
    while let Some(result) = admissions.join_next().await {
        let admission = result.expect("admission task").expect("concurrent room admission");
        admitted += 1;
        assert!(admission.active_ai_count <= 7);
    }
    assert_eq!(admitted, 7);
    let membership_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM room_memberships WHERE tenant_id=$1 AND room_id=$2 AND status='ACTIVE'")
        .bind(tenant).bind(room).fetch_one(&pool).await.unwrap();
    assert_eq!(membership_count, 7);

    let app = phone_router(pool.clone());

    // Provision all seven phones through the authenticated HTTP endpoint in parallel.
    let started = std::time::Instant::now();
    let mut provisions = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        provisions.spawn(async move {
            let principal_id = actor.principal_id;
            let response = request(&app, "POST", "/ai-phone/provision", &actor.token,
                Some(serde_json::json!({"principal_id":principal_id}))).await;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 32_000).await.unwrap();
            (principal_id, status, serde_json::from_slice::<Value>(&body).unwrap())
        });
    }
    let mut phone_by_principal = HashMap::new();
    while let Some(result) = provisions.join_next().await {
        let (principal_id, status, body) = result.expect("provision task");
        assert_eq!(status, StatusCode::CREATED, "each AI must receive its own phone");
        let phone_id = Uuid::parse_str(body["ai_phone_id"].as_str().expect("phone id")).unwrap();
        assert!(phone_by_principal.insert(principal_id, phone_id).is_none(),
            "one canonical phone per AI principal");
    }
    let provisioning_elapsed_ms = started.elapsed().as_millis();
    let phones = actors.iter().map(|actor| phone_by_principal[&actor.principal_id]).collect::<Vec<_>>();
    let unique_phones = phones.iter().copied().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_phones.len(), 7, "exactly seven unique AI_PHONE_ID values");
    let phone_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_phones WHERE tenant_id=$1")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(phone_count, 7);

    // Attach all seven unique endpoints concurrently.
    let started = std::time::Instant::now();
    let mut attachments = tokio::task::JoinSet::new();
    for (index, actor) in actors.iter().cloned().enumerate() {
        let app = app.clone();
        attachments.spawn(async move {
            let principal_id = actor.principal_id;
            let response = request(&app, "POST", "/ai-phone/attach", &actor.token,
                Some(serde_json::json!({
                    "endpoint_id":Uuid::new_v4(),"endpoint_type":"TEST",
                    "metadata":{"fixture":"disposable-scale-1-to-7","ai_index":index+1}
                }))).await;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 32_000).await.unwrap();
            (principal_id, status, serde_json::from_slice::<Value>(&body).unwrap())
        });
    }
    let mut attachment_by_principal = HashMap::new();
    while let Some(result) = attachments.join_next().await {
        let (principal_id, status, body) = result.expect("attach task");
        assert_eq!(status, StatusCode::CREATED);
        assert!(attachment_by_principal.insert(principal_id, body).is_none());
    }
    assert_eq!(attachment_by_principal.len(), 7);
    let endpoint_ids = attachment_by_principal.values().map(|row|
        Uuid::parse_str(row["endpoint_id"].as_str().unwrap()).unwrap()).collect::<std::collections::HashSet<_>>();
    assert_eq!(endpoint_ids.len(), 7, "seven globally unique endpoint IDs");

    // Heartbeats run concurrently across all seven distinct phone rows.
    let mut heartbeats = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        let attachment = attachment_by_principal[&actor.principal_id].clone();
        heartbeats.spawn(async move {
            request(&app, "POST", "/ai-phone/heartbeat", &actor.token,
                Some(serde_json::json!({"endpoint_fence":attachment["endpoint_fence"]}))).await.status()
        });
    }
    while let Some(result) = heartbeats.join_next().await {
        assert_eq!(result.expect("heartbeat task"), StatusCode::OK);
    }

    // The registry returns exactly the seven tenant phones with operational
    // endpoint/session/execution and heartbeat data.
    grant(&pool, tenant, room, actors[0].principal_id, "phone.registry.read").await;
    let registry = request(&app, "GET", "/ai-phone/registry?limit=100", &actors[0].token, None).await;
    assert_eq!(registry.status(), StatusCode::OK);
    let registry: Value = serde_json::from_slice(&axum::body::to_bytes(registry.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(registry["items"].as_array().unwrap().len(), 7);
    let registry_phone_ids = registry["items"].as_array().unwrap().iter().map(|item|
        Uuid::parse_str(item["ai_phone_id"].as_str().unwrap()).unwrap()).collect::<std::collections::HashSet<_>>();
    assert_eq!(registry_phone_ids.len(), 7);
    for item in registry["items"].as_array().unwrap() {
        assert_eq!(item["tenant_id"], tenant.to_string());
        assert!(item["active_endpoint_id"].is_string());
        assert!(item["active_session_id"].is_string());
        assert!(item["active_execution_id"].is_string());
        assert!(item["heartbeat_at"].is_string());
        assert_eq!(item["availability"], "AVAILABLE");
    }

    // AI #1 sends a distinct durable message to each of the other six phones.
    let mut sends = tokio::task::JoinSet::new();
    for (index, phone_id) in phones.iter().copied().enumerate().skip(1) {
        let app = app.clone();
        let sender = actors[0].clone();
        sends.spawn(async move {
            let response = request(&app, "POST", &format!("/ai-phone/{phone_id}/messages"),
                &sender.token, Some(serde_json::json!({
                    "idempotency_key":format!("scale-message-{index}"),
                    "content":{"text":format!("hello AI #{index+1}")}
                }))).await;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 32_000).await.unwrap();
            (phone_id, status, serde_json::from_slice::<Value>(&body).unwrap())
        });
    }
    let mut messages_by_phone = HashMap::new();
    while let Some(result) = sends.join_next().await {
        let (phone_id, status, body) = result.expect("message send task");
        assert_eq!(status, StatusCode::CREATED);
        assert!(messages_by_phone.insert(phone_id, body).is_none());
    }
    assert_eq!(messages_by_phone.len(), 6);

    // Each recipient independently reads its own mailbox and ACKs its message.
    let mut mailbox_tasks = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned().skip(1) {
        let app = app.clone();
        let phone_id = phone_by_principal[&actor.principal_id];
        let message_id = messages_by_phone[&phone_id]["message_id"].as_str().unwrap().to_string();
        mailbox_tasks.spawn(async move {
            let inbox = request(&app, "GET", &format!("/ai-phone/{phone_id}/messages"),
                &actor.token, None).await;
            let status = inbox.status();
            let bytes = axum::body::to_bytes(inbox.into_body(), 32_000).await.unwrap();
            (status, bytes, message_id, phone_id, actor)
        });
    }
    // A cross-phone mailbox read by AI #1 is always denied.
    let cross_read = request(&app, "GET", &format!("/ai-phone/{}/messages", phones[1]),
        &actors[0].token, None).await;
    assert_eq!(cross_read.status(), StatusCode::NOT_FOUND);

    // Concurrent retries against one recipient converge to the same durable message.
    let target_phone = phone_by_principal[&actors[1].principal_id];
    let duplicate_body = serde_json::json!({"idempotency_key":"scale-duplicate-key","content":{"text":"duplicate-safe"}});
    let (left, right) = tokio::join!(
        request(&app, "POST", &format!("/ai-phone/{target_phone}/messages"), &actors[0].token, Some(duplicate_body.clone())),
        request(&app, "POST", &format!("/ai-phone/{target_phone}/messages"), &actors[0].token, Some(duplicate_body.clone()))
    );
    let mut duplicate_statuses = vec![left.status(), right.status()];
    duplicate_statuses.sort();
    assert_eq!(duplicate_statuses, vec![StatusCode::OK, StatusCode::CREATED]);

    let mut acked = 0;
    while let Some(result) = mailbox_tasks.join_next().await {
        let (status, bytes, message_id, phone_id, actor) = result.expect("mailbox task");
        assert_eq!(status, StatusCode::OK);
        let inbox: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(inbox.as_array().unwrap().iter().any(|message| message["message_id"] == message_id));
        let ack = request(&app, "POST", &format!("/ai-phone/{phone_id}/messages/{message_id}/ack"),
            &actor.token, None).await;
        assert_eq!(ack.status(), StatusCode::NO_CONTENT);
        acked += 1;
    }
    assert_eq!(acked, 6);

    // Detach all seven phones concurrently, then reconnect all with fresh endpoints.
    let mut detaches = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        let attachment = attachment_by_principal[&actor.principal_id].clone();
        detaches.spawn(async move {
            request(&app, "POST", "/ai-phone/detach", &actor.token,
                Some(serde_json::json!({"endpoint_fence":attachment["endpoint_fence"]}))).await.status()
        });
    }
    while let Some(result) = detaches.join_next().await {
        assert_eq!(result.expect("detach task"), StatusCode::NO_CONTENT);
    }
    let mut reconnects = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        reconnects.spawn(async move {
            request(&app, "POST", "/ai-phone/reconnect", &actor.token,
                Some(serde_json::json!({
                    "endpoint_id":Uuid::new_v4(),"endpoint_type":"TEST",
                    "metadata":{"fixture":"scale-reconnect"}
                }))).await.status()
        });
    }
    while let Some(result) = reconnects.join_next().await {
        assert_eq!(result.expect("reconnect task"), StatusCode::OK);
    }

    // Old fencing tokens cannot heartbeat after detach/reconnect.
    let mut stale = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        let attachment = attachment_by_principal[&actor.principal_id].clone();
        stale.spawn(async move {
            request(&app, "POST", "/ai-phone/heartbeat", &actor.token,
                Some(serde_json::json!({"endpoint_fence":attachment["endpoint_fence"]}))).await.status()
        });
    }
    while let Some(result) = stale.join_next().await {
        assert_eq!(result.expect("stale heartbeat task"), StatusCode::PRECONDITION_FAILED);
    }

    // Three required cross-AI attacks: mailbox access is owner-only.
    for (caller, target) in [(0usize, 1usize), (1, 6), (6, 0), (2, 3)] {
        let response = request(&app, "GET", &format!("/ai-phone/{}/messages", phones[target]),
            &actors[caller].token, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND,
            "AI #{} must not read AI #{} mailbox", caller+1, target+1);
    }

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_phones WHERE tenant_id=$1")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(remaining, 7, "exactly seven disposable phone rows remain before cleanup");
    println!("SCALE_1_TO_7_PROVISIONING_MS={provisioning_elapsed_ms}");
    println!("SCALE_1_TO_7_PHONE_COUNT={remaining}");
    println!("SCALE_1_TO_7_ENDPOINT_COUNT={}", endpoint_ids.len());
    println!("SCALE_1_TO_7_ROOM_MEMBERS={membership_count}");
    println!("SCALE_1_TO_7_ACK_COUNT={acked}");

    // Explicitly clean every disposable phone and identity; tenant cascade removes
    // sessions, executions, memberships, principals and permissions.
    sqlx::query("DELETE FROM phone_messages WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_attachments WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    let ai_ids = actors.iter().map(|actor| actor.ai_id).collect::<Vec<_>>();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=ANY($1::uuid[])").bind(&ai_ids).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identities WHERE ai_id=ANY($1::uuid[])").bind(&ai_ids).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
}
