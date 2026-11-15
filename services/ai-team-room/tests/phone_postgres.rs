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

async fn http_call_endpoint(app:&Router,method:&str,uri:&str,token:&str,endpoint_credential:&str,body:Option<Value>)->Response {
    let mut builder=Request::builder().method(method).uri(uri)
        .header("authorization",format!("Bearer {token}"))
        .header("x-ai-phone-endpoint-credential",endpoint_credential);
    if body.is_some(){builder=builder.header("content-type","application/json");}
    app.clone().oneshot(builder.body(Body::from(body.map(|v|v.to_string()).unwrap_or_default())).unwrap())
        .await.unwrap()
}

async fn seed_endpoint_credential(
    pool:&PgPool,tenant:Uuid,ai_id:Uuid,phone_id:Uuid,endpoint_id:Uuid,secret:&str,label:&str
) {
    sqlx::query("INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,tenant_id,ai_id,endpoint_type,endpoint_status,metadata) VALUES($1,$2,$3,$4,'TEST','REGISTERED',$5)")
        .bind(endpoint_id).bind(phone_id).bind(tenant).bind(ai_id)
        .bind(serde_json::json!({"label":label})).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO phone_endpoint_credentials(credential_id,endpoint_id,ai_phone_id,tenant_id,ai_id,credential_hash,credential_status,expires_at) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE',NOW()+INTERVAL '90 days')")
        .bind(Uuid::new_v4()).bind(endpoint_id).bind(phone_id).bind(tenant).bind(ai_id)
        .bind(Sha256::digest(secret.as_bytes()).to_vec()).execute(pool).await.unwrap();
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
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured; refusing to skip PostgreSQL security integration test");
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
    let identity_key=format!("test:ai-phone:{ai_id}");
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','AI-PHONE-TEST-IDENTITY')")
        .bind(ai_id).bind(&identity_key).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(tenant).bind(principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
        .bind(ai_id).bind(tenant).execute(&pool).await.unwrap();

    for permission in ["phone.read","phone.provision","phone.attach","phone.detach","phone.transfer","phone.heartbeat","phone.message.read","phone.message.send","phone.message.ack","task.read"] {
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
    let first = get_my_phone(&pool,&identity_a,None).await.expect("read explicitly provisioned permanent phone");
    assert_eq!(provisioned["ai_phone_id"],first.phone.ai_phone_id.to_string());
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"offline-queued","content":{"text":"persist while endpoint is offline"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"offline message is durably queued before endpoint attach");
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_a),Some(serde_json::json!({"principal_id":principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"repeated provision is idempotent");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let provisioned_again:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(provisioned_again["ai_phone_id"],first.phone.ai_phone_id.to_string());
    sqlx::query("DELETE FROM principal_permissions WHERE tenant_id=$1 AND principal_id=$2 AND permission='phone.provision'")
        .bind(tenant).bind(principal).execute(&pool).await.unwrap();
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_a),Some(serde_json::json!({"principal_id":principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::FORBIDDEN,"provisioning without the explicit tenant permission is rejected");
    grant(&pool,tenant,room,principal,"phone.provision").await;
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
    let endpoint_a_credential=format!("phone-01-endpoint-A-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,tenant,ai_id,first.phone.ai_phone_id,endpoint_a,&endpoint_a_credential,"endpoint-A").await;
    let attached_a = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: endpoint_a, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-A"}),
    },&endpoint_a_credential,false).await.expect("attach endpoint A");
    assert_eq!(attached_a.endpoint_fence,1);
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_a),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"CONNECTED");

    // Real Axum HTTP router E2E: authenticate with the persisted bearer hash and exercise
    // the public phone routes rather than calling only the internal service functions.
    let response=http_call_endpoint(&app,"POST","/ai-phone/attach",&token_a,&endpoint_a_credential,Some(serde_json::json!({
        "endpoint_id":endpoint_a,"endpoint_type":"TEST","metadata":{"label":"endpoint-A"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"attach route is reachable and authenticated");
    let response=http_call_endpoint(&app,"GET","/ai-phone/me",&token_a,&endpoint_a_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let response=http_call_endpoint(&app,"GET",&format!("/ai-phone/{}/endpoints",first.phone.ai_phone_id),&token_a,&endpoint_a_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"endpoint enumeration requires the active endpoint credential");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/endpoints",first.phone.ai_phone_id),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED,"AI bearer alone cannot enumerate endpoint identities");
    let response=http_call_endpoint(&app,"POST","/ai-phone/heartbeat",&token_a,&endpoint_a_credential,Some(serde_json::json!({"endpoint_fence":attached_a.endpoint_fence}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"http-e2e-1","content":{"text":"offline-to-online"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED);
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let created_message:Value=serde_json::from_slice(&bytes).unwrap();
    let created_message_id=created_message["message_id"].as_str().unwrap().to_string();
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"http-e2e-1","content":{"text":"offline-to-online"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"same idempotency key and payload replays the existing message");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let replay:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(replay["message_id"],created_message_id,"idempotent replay returns the original message ID");
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"http-e2e-1","content":{"text":"different payload"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CONFLICT,"same idempotency key with different payload is rejected");
    let response=http_call_endpoint(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),&token_a,&endpoint_a_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let inbox:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    assert!(inbox.iter().any(|m|m["idempotency_key"]=="offline-queued"),"offline message is recovered after endpoint attach");
    let http_message=inbox.iter().find(|m|m["idempotency_key"]=="http-e2e-1").unwrap();
    assert_eq!(http_message["message_state"],"DELIVERED");
    let message_id=http_message["message_id"].as_str().unwrap();
    let response=http_call(&app,"GET","/ai-phone/sent",Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"durable sender history is available");
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let sent:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    assert!(sent.iter().any(|m|m["message_id"]==message_id),"sender history contains the persisted outgoing message");
    let response=http_call_endpoint(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),&token_a,&endpoint_a_credential,None).await;
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let retried:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    let retried_message=retried.iter().find(|m|m["idempotency_key"]=="http-e2e-1").unwrap();
    assert_eq!(retried_message["attempt_count"],1,"repeated history polling does not inflate delivery attempts");
    assert_eq!(retried_message["message_state"],"DELIVERED");
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),&token_a,&endpoint_a_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT);
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),&token_a,&endpoint_a_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT,"duplicate ACK is idempotent");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/endpoints",Uuid::new_v4()),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"unknown or unauthorized phone must not leak endpoint data");
    let response=http_call(&app,"GET","/ai-phone/me",Some("invalid-session-token"),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED);

    // A valid phone UUID in another tenant is not sufficient to send to or inspect that phone.
    let foreign_tenant=Uuid::new_v4();
    let foreign_principal=Uuid::new_v4();
    let foreign_endpoint=Uuid::new_v4();
    let foreign_room=Uuid::new_v4();
    let foreign_participant=Uuid::new_v4();
    let foreign_task=Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'ai-phone-foreign-tenant')")
        .bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'ai-phone-foreign-room')")
        .bind(foreign_room).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','foreign-test',$3)")
        .bind(foreign_principal).bind(foreign_tenant).bind(format!("foreign-subject-{foreign_principal}"))
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,'AI-PHONE-FOREIGN-TEST')")
        .bind(foreign_participant).bind(foreign_tenant).bind(foreign_principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'foreign-test',$4,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(foreign_tenant).bind(foreign_principal).bind(format!("foreign-subject-{foreign_principal}"))
        .execute(&pool).await.unwrap();
    let (foreign_session,foreign_execution)=add_session(&pool,foreign_tenant,foreign_room,foreign_principal,foreign_participant).await;
    sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(foreign_tenant).bind(foreign_room).bind(foreign_participant).bind(foreign_principal).bind(foreign_session)
        .execute(&pool).await.unwrap();
    for permission in ["ai.identity.provision","phone.read","phone.provision","phone.attach","phone.detach","phone.transfer","phone.heartbeat","phone.message.read","phone.message.send","phone.message.ack","task.read"] {
        grant(&pool,foreign_tenant,foreign_room,foreign_principal,permission).await;
    }
    let token_c=bearer_for(&pool,foreign_session).await;
    let response=http_call(&app,"POST","/ai-phone/identities",Some(&token_c),Some(serde_json::json!({
        "identity_key":identity_key,"identity_kind":"TEST","display_label":"AI-PHONE-TEST-IDENTITY",
        "tenant_id":foreign_tenant,"principal_id":foreign_principal
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"tenant B binds the same stable global AI identity");
    let (provision_a,provision_b)=tokio::join!(
        http_call(&app,"POST","/ai-phone/provision",Some(&token_c),Some(serde_json::json!({"principal_id":foreign_principal}))),
        http_call(&app,"POST","/ai-phone/provision",Some(&token_c),Some(serde_json::json!({"principal_id":foreign_principal})))
    );
    let statuses=[provision_a.status(),provision_b.status()];
    assert!(statuses.contains(&axum::http::StatusCode::CREATED) && statuses.contains(&axum::http::StatusCode::OK),
        "concurrent idempotent provisioning has exactly one creator and one existing-phone response");
    let body_a=axum::body::to_bytes(provision_a.into_body(),100_000).await.unwrap();
    let body_b=axum::body::to_bytes(provision_b.into_body(),100_000).await.unwrap();
    let result_a:Value=serde_json::from_slice(&body_a).unwrap();
    let result_b:Value=serde_json::from_slice(&body_b).unwrap();
    assert_eq!(result_a["ai_phone_id"],result_b["ai_phone_id"],"concurrent provision requests converge on one tenant phone");
    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_c),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"tenant B reads its own phone");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let foreign_snapshot:Value=serde_json::from_slice(&bytes).unwrap();
    let foreign_phone=Uuid::parse_str(foreign_snapshot["phone"]["ai_phone_id"].as_str().unwrap()).unwrap();
    assert_ne!(foreign_phone,first.phone.ai_phone_id,"same AI_ID maps to distinct tenant phone IDs");
    let foreign_endpoint_credential=format!("tenant-B-endpoint-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,foreign_tenant,ai_id,foreign_phone,foreign_endpoint,&foreign_endpoint_credential,"tenant-B-endpoint").await;
    let response=http_call_endpoint(&app,"POST","/ai-phone/attach",&token_c,&foreign_endpoint_credential,Some(serde_json::json!({
        "endpoint_id":foreign_endpoint,"endpoint_type":"TEST","metadata":{"label":"tenant-B-endpoint"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"tenant B attaches to its own phone using its own endpoint credential");
    let response=http_call_endpoint(&app,"POST","/ai-phone/transfer",&token_a,&foreign_endpoint_credential,Some(serde_json::json!({
        "endpoint_id":foreign_endpoint,"endpoint_type":"TEST","metadata":{"label":"tenant-A-cannot-claim-B-endpoint"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED,"tenant A cannot use tenant B endpoint credential");
    let revoked_endpoint=Uuid::new_v4();
    let revoked_endpoint_credential=format!("tenant-B-revoked-endpoint-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,foreign_tenant,ai_id,foreign_phone,revoked_endpoint,&revoked_endpoint_credential,"revoked-endpoint").await;
    sqlx::query("UPDATE phone_endpoints SET endpoint_status='REVOKED' WHERE endpoint_id=$1 AND tenant_id=$2")
        .bind(revoked_endpoint).bind(foreign_tenant).execute(&pool).await.unwrap();
    let credential_status:String=sqlx::query_scalar("SELECT credential_status FROM phone_endpoint_credentials WHERE endpoint_id=$1")
        .bind(revoked_endpoint).fetch_one(&pool).await.unwrap();
    assert_eq!(credential_status,"REVOKED","endpoint lifecycle trigger revokes its credential");
    let response=http_call_endpoint(&app,"POST","/ai-phone/transfer",&token_c,&revoked_endpoint_credential,Some(serde_json::json!({
        "endpoint_id":revoked_endpoint,"endpoint_type":"TEST","metadata":{"label":"revoked-endpoint"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED,"revoked endpoint credential cannot reattach");

    // A second AI in tenant A proves actual same-tenant AI-to-AI delivery.
    let second_ai_id=Uuid::new_v4();
    let second_principal=Uuid::new_v4();
    let second_participant=Uuid::new_v4();
    let second_endpoint=Uuid::new_v4();
    let second_subject=format!("second-ai-subject-{second_principal}");
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','AI-PHONE-TEST-SECONDARY')")
        .bind(second_ai_id).bind(format!("test:ai-phone:secondary:{second_ai_id}")).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','ai-phone-test',$3)")
        .bind(second_principal).bind(tenant).bind(&second_subject).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,'AI-PHONE-SECONDARY')")
        .bind(second_participant).bind(tenant).bind(second_principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'ai-phone-test',$4,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(tenant).bind(second_principal).bind(&second_subject).execute(&pool).await.unwrap();
    let (second_session,_second_execution)=add_session(&pool,tenant,room,second_principal,second_participant).await;
    sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(second_participant).bind(second_principal).bind(second_session)
        .execute(&pool).await.unwrap();
    for permission in ["ai.identity.provision","phone.read","phone.provision","phone.attach","phone.detach","phone.transfer","phone.heartbeat","phone.message.read","phone.message.send","phone.message.ack","task.read"] {
        grant(&pool,tenant,room,second_principal,permission).await;
    }
    let token_second=bearer_for(&pool,second_session).await;
    let response=http_call(&app,"POST","/ai-phone/identities",Some(&token_second),Some(serde_json::json!({
        "identity_key":format!("test:ai-phone:secondary:{second_ai_id}"),"identity_kind":"TEST",
        "display_label":"AI-PHONE-TEST-SECONDARY","tenant_id":tenant,"principal_id":second_principal
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"second AI binds independently inside tenant A");
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_second),Some(serde_json::json!({"principal_id":second_principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"second AI receives its own tenant phone");
    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_second),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let second_snapshot:Value=serde_json::from_slice(&bytes).unwrap();
    let second_phone=Uuid::parse_str(second_snapshot["phone"]["ai_phone_id"].as_str().unwrap()).unwrap();
    assert_ne!(second_phone,first.phone.ai_phone_id,"two AI identities in one tenant have independent phones");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_second),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"second AI cannot read the first AI's mailbox");
    let response=http_call(&app,"GET",&format!("/ai-phone/{second_phone}/messages"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"first AI cannot read the second AI's mailbox");
    let second_endpoint_credential=format!("second-ai-endpoint-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,tenant,second_ai_id,second_phone,second_endpoint,&second_endpoint_credential,"second-ai-endpoint").await;
    let response=http_call(&app,"POST",&format!("/ai-phone/{second_phone}/messages"),Some(&token_a),Some(serde_json::json!({
        "idempotency_key":"ai-to-ai-offline-1","content":{"text":"durable message from AI-X to AI-Y"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"AI-X can send to AI-Y's phone within the same tenant");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let ai_to_ai:Value=serde_json::from_slice(&bytes).unwrap();
    let ai_to_ai_message_id=ai_to_ai["message_id"].as_str().unwrap().to_string();
    let queued_state:String=sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1 AND tenant_id=$2")
        .bind(Uuid::parse_str(&ai_to_ai_message_id).unwrap()).bind(tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(queued_state,"PENDING","offline AI-to-AI message is durably queued");
    let response=http_call_endpoint(&app,"POST","/ai-phone/attach",&token_second,&second_endpoint_credential,Some(serde_json::json!({
        "endpoint_id":second_endpoint,"endpoint_type":"TEST","metadata":{"label":"second-ai-endpoint"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"AI-Y reconnects to its own phone");
    let response=http_call_endpoint(&app,"GET",&format!("/ai-phone/{second_phone}/messages"),&token_second,&second_endpoint_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"AI-Y receives the durable offline message");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let ai_y_inbox:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    assert!(ai_y_inbox.iter().any(|m|m["message_id"]==ai_to_ai_message_id && m["message_state"]=="DELIVERED"));
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{second_phone}/messages/{ai_to_ai_message_id}/ack"),&token_second,&second_endpoint_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT,"AI-Y ACKs the message it received");
    let duplicate_principal=Uuid::new_v4();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','duplicate-test',$3)")
        .bind(duplicate_principal).bind(foreign_tenant).bind(format!("duplicate-subject-{duplicate_principal}"))
        .execute(&pool).await.unwrap();
    let duplicate_binding=sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(foreign_tenant).bind(duplicate_principal).execute(&pool).await;
    assert!(duplicate_binding.is_err(),"one global AI identity cannot bind to two principals in one tenant");
    sqlx::query("INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id,checkpoint,checkpoint_version) VALUES($1,$2,$3,$4,'Foreign tenant task',$5,'{}'::jsonb,1)")
        .bind(foreign_task).bind(foreign_tenant).bind(foreign_room).bind(Uuid::new_v4()).bind(foreign_principal)
        .execute(&pool).await.unwrap();
    let invalid_current_task=sqlx::query("UPDATE ai_phones SET current_task_id=$1,current_mission_id=(SELECT mission_id FROM team_tasks WHERE task_id=$1) WHERE ai_phone_id=$2 AND tenant_id=$3")
        .bind(foreign_task).bind(first.phone.ai_phone_id).bind(tenant).execute(&pool).await;
    assert!(invalid_current_task.is_err(),"composite task/mission FK rejects a foreign tenant task pointer");
    let tenant_phone_count:i64=sqlx::query_scalar("SELECT COUNT(DISTINCT ai_phone_id) FROM ai_phones WHERE ai_id=$1 AND tenant_id IN ($2,$3)")
        .bind(ai_id).bind(tenant).bind(foreign_tenant).fetch_one(&pool).await.unwrap();
    assert_eq!(tenant_phone_count,2,"same AI_ID must have independent phone IDs in two tenants");
    let response=http_call(&app,"POST",&format!("/ai-phone/{foreign_phone}/messages"),Some(&token_a),
        Some(serde_json::json!({"idempotency_key":"cross-tenant-probe","content":{"text":"must-not-cross-tenant"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant phone message send is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{foreign_phone}/endpoints"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant endpoint registry is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{foreign_phone}/presence"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant presence is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{foreign_phone}/messages"),Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"cross-tenant mailbox is concealed");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_c),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"tenant B cannot read tenant A presence");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/endpoints",first.phone.ai_phone_id),Some(&token_c),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"tenant B cannot enumerate tenant A endpoints");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_c),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"tenant B cannot read tenant A mailbox");
    let response=http_call(&app,"POST",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),Some(&token_c),
        Some(serde_json::json!({"idempotency_key":"cross-tenant-send-b","content":{"text":"must be rejected"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"tenant B cannot send to tenant A phone");
    let response=http_call(&app,"POST",&format!("/ai-phone/{foreign_phone}/messages"),Some(&token_c),
        Some(serde_json::json!({"idempotency_key":"tenant-b-own-message","content":{"text":"tenant B own message"}}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::CREATED,"tenant B can send to its own phone");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let foreign_message:Value=serde_json::from_slice(&bytes).unwrap();
    let foreign_message_id=foreign_message["message_id"].as_str().unwrap().to_string();
    let response=http_call_endpoint(&app,"GET",&format!("/ai-phone/{foreign_phone}/messages"),&token_c,&foreign_endpoint_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"tenant B can read its own inbox with its endpoint credential");
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{foreign_phone}/messages/{foreign_message_id}/ack"),&token_a,&endpoint_a_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"authenticated tenant A endpoint cannot address tenant B message");
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),&token_c,&foreign_endpoint_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"authenticated tenant B endpoint cannot address tenant A message");
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_a),Some(serde_json::json!({"principal_id":foreign_principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"tenant A cannot provision tenant B identity");
    let response=http_call(&app,"POST","/ai-phone/provision",Some(&token_c),Some(serde_json::json!({"principal_id":principal}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NOT_FOUND,"tenant B cannot provision tenant A identity");

    let response=http_call(&app,"POST","/ai-phone/mission",Some(&token_a),Some(serde_json::json!({"task_id":foreign_task}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::FORBIDDEN,"cross-tenant task cannot be linked to tenant-A phone");

    // A read-only task grant must not authorize changing the phone's canonical mission pointer.
    let owned_task_id = Uuid::new_v4();
    let owned_mission_id = Uuid::new_v4();
    sqlx::query("INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_issued_at,lease_expires_at,created_by_principal_id) VALUES($1,$2,$3,$4,'AI-PHONE mission-link authorization','OWNED',$5,$6,$7,$8,1,NOW(),NOW()+INTERVAL '60 seconds',$5)")
        .bind(owned_task_id).bind(tenant).bind(room).bind(owned_mission_id)
        .bind(principal).bind(participant).bind(session_a).bind(execution_a)
        .execute(&pool).await.unwrap();
    let denied_link = http_call(&app, "POST", "/ai-phone/mission", Some(&token_a),
        Some(serde_json::json!({"task_id":owned_task_id}))).await;
    assert_eq!(denied_link.status(), axum::http::StatusCode::FORBIDDEN,
        "task.read alone cannot mutate the AI phone mission pointer");
    grant(&pool, tenant, room, principal, "task.transition").await;
    let foreign_link_with_transition = http_call(&app, "POST", "/ai-phone/mission", Some(&token_a),
        Some(serde_json::json!({"task_id":foreign_task}))).await;
    assert_eq!(foreign_link_with_transition.status(), axum::http::StatusCode::FORBIDDEN,
        "task.transition does not authorize linking a foreign tenant task");
    let allowed_link = http_call(&app, "POST", "/ai-phone/mission", Some(&token_a),
        Some(serde_json::json!({"task_id":owned_task_id}))).await;
    assert_eq!(allowed_link.status(), axum::http::StatusCode::NO_CONTENT,
        "task.transition plus task ownership authorizes linking the canonical task");
    let linked_snapshot = get_my_phone(&pool, &identity_a,Some(&endpoint_a_credential)).await.unwrap();
    assert_eq!(linked_snapshot.phone.current_task_id, Some(owned_task_id));
    assert_eq!(linked_snapshot.mission.as_ref().unwrap()["task_id"], owned_task_id.to_string());
    let invalid_attachment=sqlx::query("INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,99,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(ai_id).bind(foreign_endpoint).bind(tenant).bind(room).bind(principal).bind(participant).bind(session_a).bind(execution_a)
        .execute(&pool).await;
    assert!(invalid_attachment.is_err(),"composite FK rejects cross-tenant endpoint attachment");
    let invalid_session_attachment=sqlx::query("INSERT INTO phone_attachments(attachment_id,ai_phone_id,ai_id,endpoint_id,tenant_id,room_id,principal_id,participant_id,session_id,execution_id,endpoint_fence,attachment_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,999,'ACTIVE')")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(ai_id).bind(endpoint_a).bind(tenant)
        .bind(foreign_room).bind(foreign_principal).bind(foreign_participant).bind(foreign_session).bind(foreign_execution)
        .execute(&pool).await;
    assert!(invalid_session_attachment.is_err(),"composite session/execution FK rejects a foreign tenant session");
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
    },&endpoint_a_credential,false).await.expect("idempotent attach retry");
    assert_eq!(retry_a.attachment_id,attached_a.attachment_id);
    let last_seen = heartbeat(&pool,&identity_a,&endpoint_a_credential,attached_a.endpoint_fence).await.expect("authenticated endpoint heartbeat");
    assert!(last_seen <= chrono::Utc::now());

    // A different endpoint cannot attach while the previous authenticated session remains live.
    let blocked = attach(&pool,&identity_a,AttachRequest {
        endpoint_id: Uuid::new_v4(), endpoint_type: "TEST".into(), metadata: serde_json::json!({}),
    },"invalid-unregistered-endpoint-secret",false).await;
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
    let endpoint_b_credential=format!("phone-01-endpoint-B-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,tenant,ai_id,first.phone.ai_phone_id,endpoint_b,&endpoint_b_credential,"endpoint-B").await;
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"RECONNECTING");
    let response=http_call_endpoint(&app,"POST","/ai-phone/reconnect",&token_b,&endpoint_b_credential,Some(serde_json::json!({
        "endpoint_id":endpoint_b,"endpoint_type":"TEST","metadata":{"label":"endpoint-B"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"same AI phone reconnects through the HTTP route");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"CONNECTED");
    let attached_b = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_b, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-B"}),
    },&endpoint_b_credential,false).await.expect("idempotent view of HTTP reconnect");
    assert_eq!(attached_b.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(attached_b.endpoint_fence,2);
    assert!(matches!(
        heartbeat(&pool,&identity_b,&endpoint_a_credential,attached_b.endpoint_fence).await,
        Err(PhoneError::EndpointUnauthorized)
    ),"new session cannot reuse the previous endpoint's revoked credential");
    let response=http_call(&app,"GET","/ai-phone/me",Some(&token_a),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED,"expired session A cannot operate after B reconnects");
    assert_ne!(attached_b.endpoint_id,attached_a.endpoint_id);

    // The expired old session cannot detach or mutate the newly fenced phone attachment.
    let stale = detach(&pool,&identity_a,&endpoint_a_credential,attached_a.endpoint_fence).await;
    assert!(matches!(stale,Err(PhoneError::InvalidSession)));

    // Explicit transfer is separately permission-gated and fences endpoint B.
    let endpoint_c = Uuid::new_v4();
    let endpoint_c_credential=format!("phone-01-endpoint-C-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,tenant,ai_id,first.phone.ai_phone_id,endpoint_c,&endpoint_c_credential,"endpoint-C").await;
    let response=http_call_endpoint(&app,"POST","/ai-phone/transfer",&token_b,&endpoint_c_credential,Some(serde_json::json!({
        "endpoint_id":endpoint_c,"endpoint_type":"TEST","metadata":{"label":"endpoint-C"}
    }))).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"explicit endpoint transfer is exercised through the HTTP route");
    let transferred = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_c, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-C"}),
    },&endpoint_c_credential,true).await.expect("idempotent view of HTTP transfer");
    assert_eq!(transferred.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(transferred.endpoint_fence,3);
    let stale_endpoint = detach(&pool,&identity_b,&endpoint_c_credential,attached_b.endpoint_fence).await;
    assert!(matches!(stale_endpoint,Err(PhoneError::Fenced)));
    detach(&pool,&identity_b,&endpoint_c_credential,transferred.endpoint_fence).await.expect("detach transferred endpoint");

    // Two concurrent endpoints race for a disconnected phone: the row lock + partial
    // unique index must produce exactly one winner and one conflict.
    let endpoint_d = Uuid::new_v4();
    let endpoint_e = Uuid::new_v4();
    let endpoint_d_credential=format!("phone-01-endpoint-D-{}",Uuid::new_v4());
    let endpoint_e_credential=format!("phone-01-endpoint-E-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,tenant,ai_id,first.phone.ai_phone_id,endpoint_d,&endpoint_d_credential,"endpoint-D").await;
    seed_endpoint_credential(&pool,tenant,ai_id,first.phone.ai_phone_id,endpoint_e,&endpoint_e_credential,"endpoint-E").await;
    let (race_left,race_right) = tokio::join!(
        attach(&pool,&identity_b,AttachRequest { endpoint_id:endpoint_d, endpoint_type:"TEST".into(), metadata:serde_json::json!({}) },&endpoint_d_credential,false),
        attach(&pool,&identity_b,AttachRequest { endpoint_id:endpoint_e, endpoint_type:"TEST".into(), metadata:serde_json::json!({}) },&endpoint_e_credential,false)
    );
    let race_winner = match (race_left,race_right) {
        (Ok(winner),Err(PhoneError::Conflict)) | (Err(PhoneError::Conflict),Ok(winner)) => winner,
        other => panic!("expected one concurrent attach winner and one conflict, got {other:?}"),
    };
    assert_eq!(race_winner.endpoint_fence,5);
    let race_winner_credential=if race_winner.endpoint_id==endpoint_d {&endpoint_d_credential} else {&endpoint_e_credential};
    detach(&pool,&identity_b,race_winner_credential,race_winner.endpoint_fence).await.expect("release concurrent winner");

    // Persistent offline inbox: no active attachment is required to enqueue a message.
    let sender = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'HUMAN','test-owner',$3)")
        .bind(sender).bind(tenant).bind(format!("owner-{sender}")).execute(&pool).await.unwrap();
    let human_mapping = sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(ai_id).bind(tenant).bind(sender).execute(&pool).await;
    assert!(human_mapping.is_err(), "a HUMAN principal must never bind to a global AI identity");
    let next_sequence:i64=sqlx::query_scalar("SELECT COALESCE(MAX(sequence_no),0)+1 FROM phone_messages WHERE ai_phone_id=$1 AND tenant_id=$2")
        .bind(first.phone.ai_phone_id).bind(tenant).fetch_one(&pool).await.unwrap();
    let message_id = Uuid::new_v4();
    sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,$6,'offline-msg-1',$7)")
        .bind(message_id).bind(first.phone.ai_phone_id).bind(tenant).bind(sender).bind(ai_id).bind(next_sequence)
        .bind(serde_json::json!({"text":"resume mission"})).execute(&pool).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(state,"PENDING");

    let duplicate = sqlx::query("INSERT INTO phone_messages(message_id,ai_phone_id,tenant_id,sender_principal_id,recipient_ai_id,sequence_no,idempotency_key,content) VALUES($1,$2,$3,$4,$5,$6,'offline-msg-1',$7) ON CONFLICT(ai_phone_id,sender_principal_id,idempotency_key) DO NOTHING")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(tenant).bind(sender).bind(ai_id).bind(next_sequence+1)
        .bind(serde_json::json!({"text":"resume mission"})).execute(&pool).await.unwrap();
    assert_eq!(duplicate.rows_affected(),0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM phone_messages WHERE ai_phone_id=$1 AND sender_principal_id=$2 AND idempotency_key='offline-msg-1'")
        .bind(first.phone.ai_phone_id).bind(sender).fetch_one(&pool).await.unwrap();
    assert_eq!(count,1);

    // Reconnect a fresh endpoint and verify the offline message remains in sequence.
    let endpoint_f = Uuid::new_v4();
    let endpoint_f_credential=format!("phone-01-endpoint-F-{}",Uuid::new_v4());
    seed_endpoint_credential(&pool,tenant,ai_id,first.phone.ai_phone_id,endpoint_f,&endpoint_f_credential,"endpoint-F").await;
    let attached_f = attach(&pool,&identity_b,AttachRequest {
        endpoint_id: endpoint_f, endpoint_type: "TEST".into(), metadata: serde_json::json!({"label":"endpoint-F"}),
    },&endpoint_f_credential,false).await.expect("reconnect to retrieve offline inbox");
    assert_eq!(attached_f.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(attached_f.endpoint_fence,7);
    let pending: (i64,String) = sqlx::query_as("SELECT sequence_no,message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(pending,(next_sequence,"PENDING".to_string()));
    let response=http_call_endpoint(&app,"GET",&format!("/ai-phone/{}/messages",first.phone.ai_phone_id),&token_b,&endpoint_f_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),1_000_000).await.unwrap();
    let inbox:Vec<Value>=serde_json::from_slice(&bytes).unwrap();
    let offline=inbox.iter().find(|m|m["idempotency_key"]=="offline-msg-1").unwrap();
    assert_eq!(offline["message_state"],"DELIVERED","offline PENDING message is delivered after endpoint B reconnects");
    assert_eq!(offline["attempt_count"],1);
    let sequences:Vec<i64>=inbox.iter().map(|m|m["sequence_no"].as_i64().unwrap()).collect();
    assert_eq!(sequences,vec![1,2,next_sequence],"inbox is returned in per-phone sequence order");
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),&token_b,&endpoint_f_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT);
    let response=http_call_endpoint(&app,"POST",&format!("/ai-phone/{}/messages/{}/ack",first.phone.ai_phone_id,message_id),&token_b,&endpoint_f_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT,"offline message ACK is idempotent");
    let acked: String = sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1")
        .bind(message_id).fetch_one(&pool).await.unwrap();
    assert_eq!(acked,"ACK");

    // Concurrent retries with the same idempotency key must converge to one durable message.
    let concurrent_body = serde_json::json!({
        "idempotency_key":"concurrent-message-key",
        "content":{"text":"concurrent idempotent send"}
    });
    let concurrent_uri = format!("/ai-phone/{}/messages", first.phone.ai_phone_id);
    let (send_left, send_right) = tokio::join!(
        http_call(&app, "POST", &concurrent_uri, Some(&token_b), Some(concurrent_body.clone())),
        http_call(&app, "POST", &concurrent_uri, Some(&token_b), Some(concurrent_body.clone()))
    );
    let mut concurrent_statuses = vec![send_left.status(), send_right.status()];
    concurrent_statuses.sort();
    assert_eq!(concurrent_statuses, vec![
        axum::http::StatusCode::OK,
        axum::http::StatusCode::CREATED,
    ], "concurrent identical sends return one create and one idempotent replay");
    let left_bytes = axum::body::to_bytes(send_left.into_body(), 100_000).await.unwrap();
    let right_bytes = axum::body::to_bytes(send_right.into_body(), 100_000).await.unwrap();
    let left_message: Value = serde_json::from_slice(&left_bytes).unwrap();
    let right_message: Value = serde_json::from_slice(&right_bytes).unwrap();
    assert_eq!(left_message["message_id"], right_message["message_id"],
        "concurrent retries return the same persisted message identity");
    let concurrent_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM phone_messages WHERE ai_phone_id=$1 AND tenant_id=$2 AND sender_principal_id=$3 AND idempotency_key='concurrent-message-key'"
    ).bind(first.phone.ai_phone_id).bind(tenant).bind(principal)
      .fetch_one(&pool).await.unwrap();
    assert_eq!(concurrent_count, 1, "concurrent idempotent send persists exactly one row");

    // The phone points to the existing canonical task/checkpoint record; it does not copy it.
    let task_id = Uuid::new_v4();
    let mission_id = Uuid::new_v4();
    sqlx::query("INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,created_by_principal_id,checkpoint,checkpoint_version) VALUES($1,$2,$3,$4,'AI-Phone continuity test',$5,$6,1)")
        .bind(task_id).bind(tenant).bind(room).bind(mission_id).bind(principal)
        .bind(serde_json::json!({"step":"persisted","progress":40})).execute(&pool).await.unwrap();
    sqlx::query("UPDATE ai_phones SET current_task_id=$1,current_mission_id=$2 WHERE ai_phone_id=$3 AND tenant_id=$4")
        .bind(task_id).bind(mission_id).bind(first.phone.ai_phone_id).bind(tenant).execute(&pool).await.unwrap();
    let invalid_current_mission=sqlx::query("UPDATE ai_phones SET current_mission_id=$1 WHERE ai_phone_id=$2 AND tenant_id=$3")
        .bind(Uuid::new_v4()).bind(first.phone.ai_phone_id).bind(tenant).execute(&pool).await;
    assert!(invalid_current_mission.is_err(),"composite task/mission FK rejects a mission ID not owned by the current task");
    let current = get_my_phone(&pool,&identity_b,Some(&endpoint_f_credential)).await.expect("same phone and mission after reconnect");
    assert_eq!(current.phone.ai_phone_id,first.phone.ai_phone_id);
    assert_eq!(current.phone.ai_id,ai_id);
    assert_eq!(current.phone.current_task_id,Some(task_id));
    assert_eq!(current.mission.as_ref().unwrap()["checkpoint"]["progress"],40);
    let response=http_call_endpoint(&app,"GET","/ai-phone/me",&token_c,&foreign_endpoint_credential,None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK,"tenant B can read its own active phone snapshot with its own endpoint credential");
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let tenant_b_snapshot:Value=serde_json::from_slice(&bytes).unwrap();
    let tenant_b_phone_id=Uuid::parse_str(tenant_b_snapshot["phone"]["ai_phone_id"].as_str().unwrap()).unwrap();
    assert_ne!(tenant_b_phone_id,first.phone.ai_phone_id);
    assert!(tenant_b_snapshot["mission"].is_null() || tenant_b_snapshot["mission"]["task_id"]!=task_id.to_string(),
        "tenant B cannot see tenant A mission/checkpoint snapshot");

    let response=http_call_endpoint(&app,"POST","/ai-phone/detach",&token_b,&endpoint_f_credential,
        Some(serde_json::json!({"endpoint_fence":attached_f.endpoint_fence}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::NO_CONTENT);
    let response=http_call_endpoint(&app,"POST","/ai-phone/heartbeat",&token_b,&endpoint_f_credential,
        Some(serde_json::json!({"endpoint_fence":attached_f.endpoint_fence}))).await;
    assert_eq!(response.status(),axum::http::StatusCode::UNAUTHORIZED,"detached endpoint credential is revoked");
    let response=http_call(&app,"GET",&format!("/ai-phone/{}/presence",first.phone.ai_phone_id),Some(&token_b),None).await;
    assert_eq!(response.status(),axum::http::StatusCode::OK);
    let bytes=axum::body::to_bytes(response.into_body(),100_000).await.unwrap();
    let presence:Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(presence["presence_state"],"RECONNECTING",
        "a pre-registered replacement endpoint keeps the phone in reconnecting state after detach");

    sqlx::query("DELETE FROM phone_mission_links WHERE ai_phone_id=$1").bind(foreign_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM team_tasks WHERE task_id=$1").bind(foreign_task).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_messages WHERE ai_phone_id=$1").bind(foreign_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_messages WHERE ai_phone_id=$1").bind(second_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoint_credentials WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_attachments WHERE ai_phone_id=$1").bind(second_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE endpoint_id=$1").bind(second_endpoint).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE ai_phone_id=$1").bind(second_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE ai_id=$1").bind(second_ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1").bind(second_ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identities WHERE ai_id=$1").bind(second_ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_attachments WHERE ai_phone_id=$1").bind(foreign_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE endpoint_id=$1").bind(foreign_endpoint).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE endpoint_id=$1").bind(revoked_endpoint).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE endpoint_id=$1").bind(revoked_endpoint).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE ai_phone_id=$1").bind(foreign_phone).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE ai_id=$1 AND tenant_id=$2").bind(ai_id).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1 AND tenant_id=$2").bind(ai_id).bind(foreign_tenant).execute(&pool).await.unwrap();
    // Real pre-0012 legacy migration execution is performed by ai_phone_legacy_migration.sh.

    sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_attachments WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_messages WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_mission_links WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE ai_phone_id=$1").bind(first.phone.ai_phone_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE ai_id=$1 AND tenant_id=$2").bind(ai_id).bind(tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1").bind(ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identities WHERE ai_id=$1").bind(ai_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();

}
