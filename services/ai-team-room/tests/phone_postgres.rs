use ai_team_room_service::{
    auth::AuthenticatedIdentity,
    connect_pool, migrate,
    phone::{attach, detach, get_my_phone, heartbeat, AttachRequest, PhoneError},
};
use sqlx::PgPool;
use uuid::Uuid;
use serde_json::Value;
use sha2::{Digest,Sha256};
use tower::ServiceExt;
use axum::{body::Body,http::Request,response::Response,Router};


async fn bearer_for(pool:&PgPool,session:Uuid)->String {
    let token=format!("ai-phone-http-test-{session}");
    let digest=Sha256::digest(token.as_bytes()).to_vec();
    sqlx::query("UPDATE sessions SET token_hash=$1 WHERE session_id=$2")
        .bind(digest).bind(session).execute(pool).await.unwrap();
    token
}

async fn http_call(app:&Router,method:&str,uri:&str,token:Option<&str>,body:Option<Value>)->Response {
    let mut builder=Request::builder().method(method).uri(uri);
    if let Some(token)=token { builder=builder.header("authorization",format!("Bearer {token}")); }
    if body.is_some() { builder=builder.header("content-type","application/json"); }
    app.clone().oneshot(builder.body(Body::from(body.map(|v|v.to_string()).unwrap_or_default())).unwrap())
        .await.unwrap()
}

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
    sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'ai-phone-test',$4,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(format!("test-subject-{principal}"))
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','AI-PHONE-TEST-IDENTITY')")
        .bind(ai_id).bind(format!("test:ai-phone:{ai_id}")).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(tenant).bind(principal).execute(&pool).await.unwrap();

    for permission in ["phone.read","phone.provision","phone.attach","phone.detach","phone.transfer","phone.heartbeat","phone.message.read","phone.message.send","phone.message.ack"] {
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
    let token_a=bearer_for(&pool,session_a).await;
    let app=ai_team_room_service::phone::router(pool.clone());

    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"GET /me is read-only and does not provision a phone");
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_a),Some(serde_json::json!({"principal_id":principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"explicit authorized provision creates the phone");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let provisioned:Value=serde_json::from_slice(&bytes).unwrap();
    let first = get_my_phone(&pool,&identity_a).await.expect("read explicitly provisioned permanent phone");
    assert_eq!(provisioned["ai_phone_id"],first.phone.ai_phone_id.to_string());
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_a),Some(serde_json::json!({"principal_id":principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"repeated provision is idempotent");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let provisioned_again:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(provisioned_again["ai_phone_id"],first.phone.ai_phone_id.to_string());
    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"GET /me reads the existing phone without creating it");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let created_snapshot:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created_snapshot["phone"]["ai_phone_id"],first.phone.ai_phone_id.to_string());
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_a),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"REGISTERED");
    let endpoint_a = Uuid::new_v4();
    let attached_a = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: endpoint_a, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-A"}),
    },false).await.expect("attach endpoint A");
    assert_eq!(attached_a.endpoint_fence,1);
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_a),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"CONNECTED");

    // Real Axum HTTP router E2E: authenticate with the persisted bearer hash and exercise
    // the public phone routes rather than calling only the internal service functions.
    let response=http_call(&app,"POST","/ai-phone/attach",Some(&token_a),Some(serde_json::json!({
        "endpoint_id":endpoint_a,"endpoint_type":"TEST","metadata":{"label":"endpoint-A"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"attach route is reachable and authenticated");
    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/endpoints",first.phone.ai_phone_id),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let response=http_call(&app,"POST","/ai-phone/heartbeat",Some(&token_a),Some(serde_json::json!({"endpoint_fence":attached_a.endpoint_fence}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"http-e2e-1","content":{"text":"offline-to-online"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED);
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let inbox:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    let http_message=inbox.iter().find(|m|m["idempotency_key"]=="http-e2e-1").unwrap();
    assert_eq!(http_message["message_state"],"DELIVERED");
    let message_id=http_message["message_id"].as_str().unwrap();
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let retried:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    let retried_message=retried.iter().find(|m|m["idempotency_key"]=="http-e2e-1").unwrap();
    assert_eq!(retried_message["attempt_count"],2,"unacknowledged delivery is retried on the next authenticated poll");
    assert_eq!(retried_message["message_state"],"DELIVERED");
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT);
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT,"duplicate ACK is idempotent");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/endpoints",Uuid::new_v4()),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"unknown or unauthorized phone must not leak endpoint data");
    let response=http_call(&app,"GET","/ai-phone/me",Some("invalid-session-token"),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED);

    // A valid phone UUID in another tenant is not sufficient to send to or inspect that phone.
    let foreign_tenant=Uuid::new_v4();
    let foreign_principal=Uuid::new_v4();
    let foreign_phone=Uuid::new_v4();
    let foreign_endpoint=Uuid::new_v4();
    let foreign_room=Uuid::new_v4();
    let foreign_task=Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'ai-phone-foreign-tenant')")
        .bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'ai-phone-foreign-room')")
        .bind(foreign_room).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','foreign-test',$3)")
        .bind(foreign_principal).bind(foreign_tenant).bind(format!("foreign-subject-{foreign_principal}"))
        .execute(&pool).await.unwrap();
    // Same global AI_ID, but a distinct phone per tenant.
    sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(foreign_tenant).bind(foreign_principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
        .bind(ai_id).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_phones(ai_phone_id,ai_id,tenant_id) VALUES($1,$2,$3)")
        .bind(foreign_phone).bind(ai_id).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,tenant_id,ai_id,endpoint_type,endpoint_status,metadata) VALUES($1,$2,$3,$4,'TEST','REGISTERED','{}'::jsonb)")
        .bind(foreign_endpoint).bind(foreign_phone).bind(foreign_tenant).bind(ai_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id,checkpoint,checkpoint_version) VALUES($1,$2,$3,$4,'Foreign tenant task',$5,'{}'::jsonb,1)")
        .bind(foreign_task).bind(foreign_tenant).bind(foreign_room).bind(Uuid::new_v4()).bind(foreign_principal)
        .execute(&pool).await.unwrap();
    let tenant_phone_count:i64=sqlx::query_scalar("SELECT COUNT(DISTINCT ai_phone_id) FROM ai_phones WHERE ai_id=$1 AND tenant_id IN ($2,$3)")
        .bind(ai_id).bind(tenant).bind(foreign_tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(tenant_phone_count,2,"same AI_ID must have independent phone IDs in two tenants");
    assert_ne!(foreign_phone,first.phone.ai_phone_id);
    let response=http_call(&app,"POST",&format!("/ai-phone/{foreign_phone}/messages"),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"cross-tenant-probe","content":{"text":"must-not-cross-tenant"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant phone message send is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{foreign_phone}/endpoints"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant endpoint registry is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{foreign_phone}/presence"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant presence is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{foreign_phone}/messages"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant mailbox is concealed");
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/mission",first.phone.ai_phone_id),Some(&token_a),Some(serde_json::json!({"task_id":foreign_task}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::FORBIDDEN,"cross-tenant task cannot be linked to tenant-A phone");
    let invalid_attachment=sqlx::query("INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,99,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(ai_id).bind(foreign_endpoint).bind(tenant).bind(room).bind(principal).bind(participant).bind(session_a).bind(execution_a)
        .execute(&pool).await;
    assert!(invalid_attachment.is_err(),"composite FK rejects cross-tenant endpoint attachment");
    let invalid_message=sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,99,'cross-tenant-fk-probe',$6)")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(foreign_tenant).bind(foreign_principal).bind(ai_id).bind(serde_json::json!({"text":"must be rejected"}))
        .execute(&pool).await;
    assert!(invalid_message.is_err(),"composite FK rejects cross-tenant mailbox rows");
    let invalid_mission=sqlx::query("INSERT INTO phone_mission_links(ai_phone_id,tenant_id,room_id,task_id) VALUES($1,$2,$3,$4)")
        .bind(first.phone.ai_phone_id).bind(tenant).bind(foreign_room).bind(foreign_task).execute(&pool).await;
    assert!(invalid_mission.is_err(),"composite FK rejects cross-tenant mission links");
    sqlx::query("UPDATE phone_endpoints SET last_seen_at=NOW()-INTERVAL '2 minutes' WHERE endpoint_id=$1").bind(endpoint_a).execute(&pool).await.unwrap();
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_a),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"ATTACHED","stale endpoint heartbeat differs from session attachment");

    let retry_a = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: endpoint_a, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-A"}),
    },false).await.expect("idempotent attach retry");
    assert_eq!(retry_a.attachment_id,attached_a.attachment_id);
    let last_seen = heartbeat(&pool,&identity_a,attached_a.endpoint_fence).await.expect("authenticated endpoint heartbeat");
    assert!(last_seen <= chrono::Utc::now());

    // A different endpoint cannot attach while the previous authenticated session remains live.
    let blocked = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: Uuid::new_v4(), endpoint_type: "TEST".into(), metadata: serde_json::json!({}),
    },false).await;
    assert!(matches!(blocked,Err(PhoneError::Conflict)));

    // Session A expires. Its phone attachment remains persisted, but it is no longer authoritative.
    sqlx::query("UPDATE sessions SET expires_at=NOW()-INTERVAL '1 second',status='EXPIRED' WHERE session_id=$1")
        .bind(session_a).execute(&pool).await.unwrap();
    sqlx::query("UPDATE workload_executions SET heartbeat_at=NOW()-INTERVAL '2 minutes',expires_at=NOW()-INTERVAL '1 second',status='EXPIRED' WHERE execution_id=$1")
        .bind(execution_a).execute(&pool).await.unwrap();

    let (session_b, execution_b) = add_session(&pool,tenant,room,principal,participant).await;
    sqlx::query("UPDATE room_memberships SET session_id=$1,last_seen_at=NOW() WHERE room_id=$2 AND principal_id=$3")
        .bind(session_b).bind(room).bind(principal).execute(&pool).await.unwrap();
    let identity_b = AuthenticatedIdentity {
        tenant_id: tenant, participant_id: participant, principal_id: principal,
        session_id: session_b, execution_id: execution_b, room_id: room,
        issuer: "test-only".into(), subject: "test-only".into(),
    };
    let token_b=bearer_for(&pool,session_b).await;
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"OFFLINE","expired session A leaves the persistent phone offline");
    let endpoint_b = Uuid::new_v4();
    let response=http_call(&app,"POST","/ai-phone/endpoints/register",Some(&token_b),Some(serde_json::json!({
        "endpoint_id":endpoint_b,"endpoint_type":"TEST","metadata":{"label":"endpoint-B"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"endpoint registry accepts a new authenticated endpoint");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"RECONNECTING");
    let response=http_call(&app,"POST","/ai-phone/reconnect",Some(&token_b),Some(serde_json::json!({
        "endpoint_id":endpoint_b,"endpoint_type":"TEST","metadata":{"label":"endpoint-B"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"same AI phone reconnects through the HTTP route");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"CONNECTED");
    let attached_b = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_b, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-B"}),
    },false).await.expect("idempotent view of HTTP reconnect");
    assert_eq!(attached_b.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(attached_b.endpoint_fence,2);
    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED,"expired session A cannot operate after B reconnects");
    assert_ne!(attached_b.endpoint_id,attached_a.endpoint_id);

    // The expired old session cannot detach or mutate the newly fenced phone attachment.
    let stale = detach(&pool,&identity_a,attached_a.endpoint_fence).await;
    assert!(matches!(stale,Err(PhoneError::InvalidSession)));

    // Explicit transfer is separately permission-gated and fences endpoint B.
    let endpoint_c = Uuid::new_v4();
    let response=http_call(&app,"POST","/ai-phone/transfer",Some(&token_b),Some(serde_json::json!({
        "endpoint_id":endpoint_c,"endpoint_type":"TEST","metadata":{"label":"endpoint-C"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"explicit endpoint transfer is exercised through the HTTP route");
    let transferred = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_c, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-C"}),
    },true).await.expect("idempotent view of HTTP transfer");
    assert_eq!(transferred.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(transferred.endpoint_fence,3);
    let stale_endpoint = detach(&pool,&identity_b,attached_b.endpoint_fence).await;
    assert!(matches!(stale_endpoint,Err(PhoneError::Fenced)));
    detach(&pool,&identity_b,transferred.endpoint_fence).await.expect("detach transferred endpoint");

    // Two concurrent endpoints race for a disconnected phone: the row lock + partial
    // unique index must produce exactly one winner and one conflict.
    let endpoint_d = Uuid::new_v4();
    let endpoint_e = Uuid::new_v4();
    let (race_left,race_right) = tokio::join!(
        attach(&pool,&identity_b,AttachRequest { endpoint_id:endpoint_d, endpoint_type:"TEST".into(), metadata:serde_json::json!({}) },false),
        attach(&pool,&identity_b,AttachRequest { endpoint_id:endpoint_e, endpoint_type:"TEST".into(), metadata:serde_json::json!({}) },false)
    );
    let race_winner = match (race_left,race_right) {
        (Ok(winner),Err(PhoneError::Conflict)) | (Err(PhoneError::Conflict),Ok(winner)) => winner,
        other => panic!("expected one concurrent attach winner and one conflict, got {other:?}"),
    };
    assert_eq!(race_winner.endpoint_fence,5);
    detach(&pool,&identity_b,race_winner.endpoint_fence).await.expect("release concurrent winner");

    // Persistent offline inbox: no active attachment is required to enqueue a message.
    let sender = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'HUMAN','test-owner',$3)")
        .bind(sender).bind(tenant).bind(format!("owner-{sender}")).execute(&pool).await.unwrap();
    let human_mapping = sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(tenant).bind(sender).execute(&pool).await;
    assert!(human_mapping.is_err(), "a HUMAN principal must never bind to a global AI identity");
    let message_id = Uuid::new_v4();
    sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,2,'offline-msg-1',$6)")
        .bind(message_id).bind(first.phone.ai_phone_id).bind(tenant).bind(sender).bind(ai_id)
        .bind(serde_json::json!({"text":"resume mission"})).execute(&pool).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(state,"PENDING");

    let duplicate = sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,3,'offline-msg-1',$6) ON CONFLICT(ai_phone_id,sender_principal_id,idempotency_key) DO NOTHING")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(tenant).bind(sender).bind(ai_id)
        .bind(serde_json::json!({"text":"resume mission"})).execute(&pool).await.unwrap();
    assert_eq!(duplicate.rows_affected(),0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM phone_messages WHERE ai_phone_id=$1 AND sender_principal_id=$2 AND idempotency_key='offline-msg-1'")
        .bind(first.phone.ai_phone_id).bind(sender).fetch_one(&pool).await.unwrap();
    assert_eq!(count,1);

    // Reconnect a fresh endpoint and verify the offline message remains in sequence.
    let endpoint_f = Uuid::new_v4();
    let attached_f = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_f, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-F"}),
    },false).await.expect("reconnect to retrieve offline inbox");
    assert_eq!(attached_f.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(attached_f.endpoint_fence,7);
    let pending: (i64,String) = sqlx::query_as("SELECT sequence_no,message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(pending,(2,"PENDING".to_string()));
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let inbox:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    let offline=inbox.iter().find(|m|m["idempotency_key"]=="offline-msg-1").unwrap();
    assert_eq!(offline["message_state"],"DELIVERED","offline PENDING message is delivered after endpoint B reconnects");
    assert_eq!(offline["attempt_count"],1);
    let sequences:Vec<i64>=inbox.iter().map(|m|m["sequence_no"].as_i64().unwrap()).collect();
    assert_eq!(sequences,vec![1,2],"inbox is returned in per-phone sequence order");
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT);
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT,"offline message ACK is idempotent");
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

    let response=http_call(&app,"POST","/ai-phone/detach",Some(&token_b),
        Some(serde_json::json!({"endpoint_fence":attached_f.endpoint_fence}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT);
    let response=http_call(&app,"POST","/ai-phone/heartbeat",Some(&token_b),
        Some(serde_json::json!({"endpoint_fence":attached_f.endpoint_fence}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::PRECONDITION_FAILED,"detached endpoint cannot heartbeat");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"DISCONNECTED");

    sqlx::query("DELETE FROM team_tasks WHERE task_id=$1").bind(foreign_task).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE endpoint_id=$1").bind(foreign_endpoint).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE ai_phone_id=$1").bind(foreign_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE ai_id=$1 AND tenant_id=$2").bind(ai_id).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1 AND tenant_id=$2").bind(ai_id).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_attachments WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_messages WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_mission_links WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1").bind(ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identities WHERE ai_id=$1").bind(ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();

}
