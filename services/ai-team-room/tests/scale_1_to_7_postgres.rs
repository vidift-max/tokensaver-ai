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
    operator_key: String,
    endpoint_id: Uuid,
    endpoint_credential: String,
    reconnect_endpoint_id: Uuid,
    reconnect_endpoint_credential: String,
}

async fn request(app: &Router, method: &str, uri: &str, token: &str, body: Option<Value>) -> Response {
    let mut builder = Request::builder().method(method).uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if body.is_some() { builder = builder.header("content-type", "application/json"); }
    app.clone().oneshot(builder.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default())).unwrap())
        .await.unwrap()
}

async fn request_endpoint(app: &Router, method: &str, uri: &str, token: &str, credential: &str, body: Option<Value>) -> Response {
    let mut builder = Request::builder().method(method).uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-ai-phone-endpoint-credential", credential);
    if body.is_some() { builder = builder.header("content-type", "application/json"); }
    app.clone().oneshot(builder.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default())).unwrap())
        .await.unwrap()
}

async fn request_operator(app: &Router, method: &str, uri: &str, token: &str, operator_key: &str, body: Option<Value>) -> Response {
    let mut builder = Request::builder().method(method).uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("x-ai-phone-operator-key", operator_key);
    if body.is_some() { builder = builder.header("content-type", "application/json"); }
    app.clone().oneshot(builder.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default())).unwrap())
        .await.unwrap()
}

async fn grant(pool: &PgPool, tenant: Uuid, room: Uuid, principal: Uuid, permission: &str) {
    sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).bind(permission)
        .execute(pool).await.unwrap();
}

async fn grant_tenant_wide(pool: &PgPool, tenant: Uuid, principal: Uuid, permission: &str) {
    sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,NULL,$4)")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(permission)
        .execute(pool).await.unwrap();
}

async fn seed_actor(pool: &PgPool, tenant: Uuid, room: Uuid, index: usize) -> Actor {
    let principal_id = Uuid::new_v4();
    let participant_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let execution_id = Uuid::new_v4();
    let ai_id = Uuid::new_v4();
    let operator_key = format!("scale-operator-{index}-{}", Uuid::new_v4());
    let endpoint_id = Uuid::new_v4();
    let endpoint_credential = String::new();
    let reconnect_endpoint_id = Uuid::new_v4();
    let reconnect_endpoint_credential = String::new();
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
        "phone.message.ack", "phone.endpoint.manage", "room.read", "room.write",
    ] {
        grant(pool, tenant, room, principal_id, permission).await;
    }
    Actor { tenant_id: tenant, room_id: room, principal_id, participant_id,
        session_id, execution_id, ai_id, token, operator_key, endpoint_id, endpoint_credential,
        reconnect_endpoint_id, reconnect_endpoint_credential }
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
    let identity_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_identity_tenants WHERE tenant_id=$1")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(identity_count, 7, "exactly seven tenant-bound AI identities");
    let session_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE tenant_id=$1 AND status='ACTIVE'")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(session_count, 7, "exactly seven active disposable sessions");

    // Provision out-of-band operator-key hashes, then exercise real endpoint
    // registration twice per AI: initial generation and replacement generation.
    for actor in actors.iter_mut() {
        sqlx::query("INSERT INTO phone_endpoint_operator_keys(operator_key_id,tenant_id,ai_id,key_hash,expires_at) VALUES($1,$2,$3,$4,NOW()+INTERVAL '90 days')")
            .bind(Uuid::new_v4()).bind(tenant).bind(actor.ai_id)
            .bind(Sha256::digest(actor.operator_key.as_bytes()).to_vec())
            .execute(&pool).await.unwrap();
        let initial = request_operator(&app,"POST","/ai-phone/endpoints/register",&actor.token,
            &actor.operator_key,Some(serde_json::json!({
                "endpoint_id":actor.endpoint_id,"endpoint_type":"TEST",
                "metadata":{"fixture":"disposable-scale-1-to-7","generation":"initial"}
            }))).await;
        assert_eq!(initial.status(),StatusCode::CREATED,"operator enrolls initial endpoint");
        assert_eq!(initial.headers().get(axum::http::header::CACHE_CONTROL).unwrap(),"no-store");
        let initial_body:Value=serde_json::from_slice(
            &axum::body::to_bytes(initial.into_body(),32_000).await.unwrap()).unwrap();
        actor.endpoint_credential=initial_body["endpoint_credential"].as_str().unwrap().to_owned();

        let replacement = request_operator(&app,"POST","/ai-phone/endpoints/register",&actor.token,
            &actor.operator_key,Some(serde_json::json!({
                "endpoint_id":actor.reconnect_endpoint_id,"endpoint_type":"TEST",
                "metadata":{"fixture":"disposable-scale-1-to-7","generation":"replacement"}
            }))).await;
        assert_eq!(replacement.status(),StatusCode::CREATED,"operator enrolls replacement endpoint");
        let replacement_body:Value=serde_json::from_slice(
            &axum::body::to_bytes(replacement.into_body(),32_000).await.unwrap()).unwrap();
        actor.reconnect_endpoint_credential=replacement_body["endpoint_credential"].as_str().unwrap().to_owned();
    }

    // Attach all seven unique endpoints concurrently.
    let started = std::time::Instant::now();
    let mut attachments = tokio::task::JoinSet::new();
    for (index, actor) in actors.iter().cloned().enumerate() {
        let app = app.clone();
        attachments.spawn(async move {
            let principal_id = actor.principal_id;
            let response = request_endpoint(&app, "POST", "/ai-phone/attach", &actor.token,
                &actor.endpoint_credential, Some(serde_json::json!({
                    "endpoint_id":actor.endpoint_id,"endpoint_type":"TEST",
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
            request_endpoint(&app, "POST", "/ai-phone/heartbeat", &actor.token,
                &actor.endpoint_credential, Some(serde_json::json!({"endpoint_fence":attachment["endpoint_fence"]}))).await.status()
        });
    }
    while let Some(result) = heartbeats.join_next().await {
        assert_eq!(result.expect("heartbeat task"), StatusCode::OK);
    }

    // The registry returns exactly the seven tenant phones with operational
    // endpoint/session/execution and heartbeat data.
    grant_tenant_wide(&pool, tenant, actors[0].principal_id, "phone.registry.read").await;
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
                    "content":{"text":format!("hello AI #{}", index+1)}
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
    // AI #7 also sends one message back to AI #1, exercising all seven inboxes.
    {
        let app = app.clone();
        let sender = actors[6].clone();
        let phone_id = phones[0];
        sends.spawn(async move {
            let response = request(&app, "POST", &format!("/ai-phone/{phone_id}/messages"),
                &sender.token, Some(serde_json::json!({
                    "idempotency_key":"scale-message-to-ai-1",
                    "content":{"text":"hello AI #1"}
                }))).await;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 32_000).await.unwrap();
            (phone_id, status, serde_json::from_slice::<Value>(&body).unwrap())
        });
    }
    // The first six sends have already been collected above; collect the final
    // return message separately so every phone has a recipient inbox message.
    while let Some(result) = sends.join_next().await {
        let (phone_id, status, body) = result.expect("return message task");
        assert_eq!(status, StatusCode::CREATED);
        assert!(messages_by_phone.insert(phone_id, body).is_none());
    }
    assert_eq!(messages_by_phone.len(), 7);

    // Each of the seven recipients independently reads its own mailbox and ACKs.
    let mut mailbox_tasks = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        let phone_id = phone_by_principal[&actor.principal_id];
        let message_id = messages_by_phone[&phone_id]["message_id"].as_str().unwrap().to_string();
        mailbox_tasks.spawn(async move {
            let inbox = request_endpoint(&app, "GET", &format!("/ai-phone/{phone_id}/messages"),
                &actor.token, &actor.endpoint_credential, None).await;
            let status = inbox.status();
            let bytes = axum::body::to_bytes(inbox.into_body(), 32_000).await.unwrap();
            (status, bytes, message_id, phone_id, actor)
        });
    }
    // A cross-phone mailbox read by AI #1 is always denied.
    let cross_read = request_endpoint(&app, "GET", &format!("/ai-phone/{}/messages", phones[1]),
        &actors[0].token, &actors[0].endpoint_credential, None).await;
    assert_eq!(cross_read.status(), StatusCode::NOT_FOUND);

    // Concurrent retries against one recipient converge to the same durable message.
    let target_phone = phone_by_principal[&actors[1].principal_id];
    let duplicate_body = serde_json::json!({"idempotency_key":"scale-duplicate-key","content":{"text":"duplicate-safe"}});
    let duplicate_uri = format!("/ai-phone/{target_phone}/messages");
    let (left, right) = tokio::join!(
        request(&app, "POST", &duplicate_uri, &actors[0].token, Some(duplicate_body.clone())),
        request(&app, "POST", &duplicate_uri, &actors[0].token, Some(duplicate_body.clone()))
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
        let ack = request_endpoint(&app, "POST", &format!("/ai-phone/{phone_id}/messages/{message_id}/ack"),
            &actor.token, &actor.endpoint_credential, None).await;
        assert_eq!(ack.status(), StatusCode::NO_CONTENT);
        acked += 1;
    }
    assert_eq!(acked, 7);

    // Cross-endpoint claim: AI #7 cannot attach AI #1's registered endpoint
    // even when it presents its own valid endpoint credential.
    let endpoint_one = actors[0].endpoint_id;
    let endpoint_claim = request_operator(&app, "POST", "/ai-phone/endpoints/register", &actors[6].token,
        &actors[6].operator_key,
        Some(serde_json::json!({"endpoint_id":endpoint_one,"endpoint_type":"TEST","metadata":{}}))).await;
    assert_eq!(endpoint_claim.status(), StatusCode::CONFLICT,
        "another AI cannot register an endpoint ID already owned by AI #1");
    let cross_attach = request_endpoint(&app, "POST", "/ai-phone/attach", &actors[6].token,
        &actors[0].endpoint_credential,
        Some(serde_json::json!({"endpoint_id":actors[6].reconnect_endpoint_id,"endpoint_type":"TEST","metadata":{}}))).await;
    assert_eq!(cross_attach.status(), StatusCode::UNAUTHORIZED,
        "AI #7 cannot use AI #1's endpoint credential even for AI #7's own registered endpoint");

    // Cross-ACK: AI #3 cannot acknowledge a message belonging to AI #7.
    let phone_seven = phone_by_principal[&actors[6].principal_id];
    let message_seven = messages_by_phone[&phone_seven]["message_id"].as_str().unwrap();
    let cross_ack = request_endpoint(&app, "POST",
        &format!("/ai-phone/{phone_seven}/messages/{message_seven}/ack"),
        &actors[2].token, &actors[2].endpoint_credential, None).await;
    assert_eq!(cross_ack.status(), StatusCode::NOT_FOUND);

    // Detach all seven phones concurrently, then reconnect all with fresh endpoints.
    let mut detaches = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        let attachment = attachment_by_principal[&actor.principal_id].clone();
        detaches.spawn(async move {
            request_endpoint(&app, "POST", "/ai-phone/detach", &actor.token,
                &actor.endpoint_credential, Some(serde_json::json!({"endpoint_fence":attachment["endpoint_fence"]}))).await.status()
        });
    }
    while let Some(result) = detaches.join_next().await {
        assert_eq!(result.expect("detach task"), StatusCode::NO_CONTENT);
    }
    let mut reconnects = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        reconnects.spawn(async move {
            request_endpoint(&app, "POST", "/ai-phone/reconnect", &actor.token,
                &actor.reconnect_endpoint_credential, Some(serde_json::json!({
                    "endpoint_id":actor.reconnect_endpoint_id,"endpoint_type":"TEST",
                    "metadata":{"fixture":"scale-reconnect"}
                }))).await.status()
        });
    }
    while let Some(result) = reconnects.join_next().await {
        assert_eq!(result.expect("reconnect task"), StatusCode::OK);
    }

    // The same AI bearer is retained across the transfer, but each old endpoint
    // credential must be unable to discover or operate the replacement generation.
    for actor in &actors {
        let phone_id = phone_by_principal[&actor.principal_id];
        let old_snapshot = request_endpoint(&app, "GET", "/ai-phone/me", &actor.token,
            &actor.endpoint_credential, None).await;
        assert_eq!(old_snapshot.status(), StatusCode::UNAUTHORIZED,
            "old endpoint credential cannot recover the replacement fence for AI {}", actor.principal_id);
        let old_endpoint_list = request_endpoint(&app, "GET", &format!("/ai-phone/{phone_id}/endpoints"),
            &actor.token, &actor.endpoint_credential, None).await;
        assert_eq!(old_endpoint_list.status(), StatusCode::UNAUTHORIZED,
            "old endpoint credential cannot enumerate replacement endpoints");
        let new_snapshot = request_endpoint(&app, "GET", "/ai-phone/me", &actor.token,
            &actor.reconnect_endpoint_credential, None).await;
        assert_eq!(new_snapshot.status(), StatusCode::OK,
            "new endpoint credential can inspect only its current phone generation");
    }
    let active_generation_credentials: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT c.credential_hash) FROM phone_endpoint_credentials c JOIN ai_phones p ON p.ai_phone_id=c.ai_phone_id AND p.tenant_id=c.tenant_id AND p.ai_id=c.ai_id JOIN phone_attachments a ON a.endpoint_id=c.endpoint_id AND a.ai_phone_id=c.ai_phone_id AND a.tenant_id=c.tenant_id AND a.ai_id=c.ai_id WHERE c.tenant_id=$1 AND c.credential_status='ACTIVE' AND c.endpoint_fence=p.endpoint_fence AND a.attachment_status='ACTIVE' AND a.endpoint_fence=p.endpoint_fence"
    ).bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(active_generation_credentials, 7,
        "all seven phones have one distinct active credential bound to their current generation");
    let revoked_old_credentials: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM phone_endpoint_credentials WHERE tenant_id=$1 AND credential_status='REVOKED'"
    ).bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(revoked_old_credentials, 7,
        "all seven prior endpoint credentials were revoked during detach");

    // Old fencing tokens cannot heartbeat after detach/reconnect.
    let mut stale = tokio::task::JoinSet::new();
    for actor in actors.iter().cloned() {
        let app = app.clone();
        let attachment = attachment_by_principal[&actor.principal_id].clone();
        stale.spawn(async move {
            request_endpoint(&app, "POST", "/ai-phone/heartbeat", &actor.token,
                &actor.endpoint_credential, Some(serde_json::json!({"endpoint_fence":attachment["endpoint_fence"]}))).await.status()
        });
    }
    while let Some(result) = stale.join_next().await {
        assert_eq!(result.expect("stale heartbeat task"), StatusCode::UNAUTHORIZED,
            "detached endpoint credential is revoked before fence evaluation");
    }
    // Even a currently valid replacement credential cannot operate against the old fence.
    for actor in &actors {
        let old_fence = attachment_by_principal[&actor.principal_id]["endpoint_fence"].as_i64().unwrap();
        let stale_generation = request_endpoint(&app, "POST", "/ai-phone/heartbeat", &actor.token,
            &actor.reconnect_endpoint_credential,
            Some(serde_json::json!({"endpoint_fence":old_fence}))).await;
        assert_eq!(stale_generation.status(), StatusCode::PRECONDITION_FAILED,
            "new endpoint credential cannot operate against a stale generation");
    }

    // Three required cross-AI attacks: mailbox access is owner-only.
    for (caller, target) in [(0usize, 1usize), (1, 6), (6, 0), (2, 3)] {
        let actor = &actors[caller];
        let response = request_endpoint(&app, "GET", &format!("/ai-phone/{}/messages", phones[target]),
            &actor.token, &actor.reconnect_endpoint_credential, None).await;
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
    sqlx::query("DELETE FROM phone_endpoint_credentials WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoint_operator_keys WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
    let ai_ids = actors.iter().map(|actor| actor.ai_id).collect::<Vec<_>>();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=ANY($1::uuid[])").bind(&ai_ids).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identities WHERE ai_id=ANY($1::uuid[])").bind(&ai_ids).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();

    let residual_phones: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_phones WHERE tenant_id=$1")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    let residual_identities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_identity_tenants WHERE tenant_id=$1")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    let tenant_remains: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE tenant_id=$1)")
        .bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(residual_phones, 0, "all seven disposable phones must be removed");
    assert_eq!(residual_identities, 0, "all disposable AI identity bindings must be removed");
    assert!(!tenant_remains, "the disposable tenant must be removed");
    println!("SCALE_1_TO_7_CLEANUP=PASS");
}
