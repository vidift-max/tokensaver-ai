use ai_team_room_service::{connect_pool, migrate};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn add_replacement_session(
    pool: &PgPool,
    tenant_id: Uuid,
    room_id: Uuid,
    principal_id: Uuid,
    participant_id: Uuid,
    old_session_id: Uuid,
) -> (Uuid, Uuid, String) {
    let session_id = Uuid::new_v4();
    let execution_id = Uuid::new_v4();
    sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,status,issued_at,expires_at,rotated_from_session_id) VALUES($1,$2,$3,$4,'ACTIVE',NOW(),NOW()+INTERVAL '30 minutes',$5)")
        .bind(session_id).bind(tenant_id).bind(room_id).bind(principal_id).bind(old_session_id)
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,status,heartbeat_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,'phone-02-runner','ACTIVE',NOW(),NOW()+INTERVAL '30 minutes')")
        .bind(execution_id).bind(tenant_id).bind(room_id).bind(participant_id).bind(principal_id).bind(session_id)
        .execute(pool).await.unwrap();
    let moved = sqlx::query("UPDATE room_memberships SET session_id=$1 WHERE tenant_id=$2 AND room_id=$3 AND principal_id=$4 AND participant_id=$5 AND session_id=$6 AND status='ACTIVE'")
        .bind(session_id).bind(tenant_id).bind(room_id).bind(principal_id).bind(participant_id).bind(old_session_id)
        .execute(pool).await.unwrap();
    assert_eq!(moved.rows_affected(),1,"session rotation moves the active Team Room membership");
    let token = bearer_for(pool,session_id).await;
    (session_id,execution_id,token)
}

async fn add_ai(
    pool: &PgPool,
    tenant_id: Uuid,
    room_id: Uuid,
    label: &str,
) -> (Uuid, Uuid, Uuid, Uuid, Uuid, String) {
    let principal_id = Uuid::new_v4();
    let participant_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let execution_id = Uuid::new_v4();
    let subject = format!("phone-02-{label}-{principal_id}");

    sqlx::query(
        "INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','phone-02-test',$3)",
    )
    .bind(principal_id)
    .bind(tenant_id)
    .bind(&subject)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)",
    )
    .bind(participant_id)
    .bind(tenant_id)
    .bind(principal_id)
    .bind(label)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'phone-02-test',$4,'ACTIVE')",
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(principal_id)
    .bind(&subject)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status) VALUES($1,$2,$3,$4,NOW()-INTERVAL '1 minute',NOW()+INTERVAL '30 minutes','ACTIVE')",
    )
    .bind(session_id)
    .bind(tenant_id)
    .bind(room_id)
    .bind(principal_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,'phone-02-runner',NOW(),NOW()+INTERVAL '30 minutes','ACTIVE')",
    )
    .bind(execution_id)
    .bind(tenant_id)
    .bind(room_id)
    .bind(participant_id)
    .bind(principal_id)
    .bind(session_id)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')",
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(room_id)
    .bind(participant_id)
    .bind(principal_id)
    .bind(session_id)
    .execute(pool)
    .await
    .unwrap();

    (
        principal_id,
        participant_id,
        session_id,
        execution_id,
        Uuid::new_v4(),
        subject,
    )
}

async fn bearer_for(pool: &PgPool, session_id: Uuid) -> String {
    let token = format!("ai-phone-02-test-{session_id}");
    let digest = Sha256::digest(token.as_bytes()).to_vec();
    sqlx::query("UPDATE sessions SET token_hash=$1 WHERE session_id=$2")
        .bind(digest)
        .bind(session_id)
        .execute(pool)
        .await
        .unwrap();
    token
}

async fn grant_permissions(pool: &PgPool, tenant_id: Uuid, room_id: Uuid, principal_id: Uuid) {
    for permission in [
        "ai.identity.provision",
        "phone.read",
        "phone.provision",
        "phone.attach",
        "phone.detach",
        "phone.transfer",
        "phone.endpoint.manage",
        "phone.heartbeat",
        "phone.message.read",
        "phone.message.send",
        "phone.message.ack",
        "phone.pair.grant",
        "task.read",
    ] {
        sqlx::query(
            "INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(principal_id)
        .bind(room_id)
        .bind(permission)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn grant_permission(
    pool: &PgPool,
    tenant_id: Uuid,
    room_id: Uuid,
    principal_id: Uuid,
    permission: &str,
) {
    sqlx::query(
        "INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(principal_id)
    .bind(room_id)
    .bind(permission)
    .execute(pool)
    .await
    .unwrap();
}

async fn call_with_headers(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    endpoint_credential: Option<&str>,
    operator_key: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if let Some(secret)=endpoint_credential {
        builder=builder.header("x-ai-phone-endpoint-credential",secret);
    }
    if let Some(secret)=operator_key {
        builder=builder.header("x-ai-phone-operator-key",secret);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(
                    body.map(|value| value.to_string()).unwrap_or_default(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, bytes.to_vec())
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Vec<u8>) {
    call_with_headers(app,method,uri,token,None,None,body).await
}


#[tokio::test]
async fn p2_33_directional_pair_authorization_and_identity_revocation() {
    let database_url=std::env::var("DATABASE_URL").expect("DATABASE_URL required for PostgreSQL adversarial test");
    let pool=connect_pool(&database_url).await.expect("connect PostgreSQL 16");
    migrate(&pool).await.expect("apply migrations");

    let tenant_id=Uuid::new_v4();
    let room_id=Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'P2.33 focused tenant')")
        .bind(tenant_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'P2.33 focused room')")
        .bind(room_id).bind(tenant_id).execute(&pool).await.unwrap();

    let (p1,_,s1,e1,_,_)=add_ai(&pool,tenant_id,room_id,"P2.33-PHONE-01").await;
    let (p2,_,s2,e2,_,_)=add_ai(&pool,tenant_id,room_id,"P2.33-PHONE-02").await;
    let (p3,_,s3,e3,_,_)=add_ai(&pool,tenant_id,room_id,"P2.33-PHONE-03").await;
    for principal in [p1,p2,p3] { grant_permissions(&pool,tenant_id,room_id,principal).await; }
    let token1=bearer_for(&pool,s1).await;
    let token2=bearer_for(&pool,s2).await;
    let token3=bearer_for(&pool,s3).await;
    let app=ai_team_room_service::phone::router(pool.clone());

    async fn provision_identity(app:&Router,token:&str,tenant:Uuid,principal:Uuid,label:&str)->Uuid {
        let key=format!("p2.33:{label}:{principal}");
        let (status,body)=call(app,"POST","/ai-phone/identities",token,Some(serde_json::json!({
            "identity_key":key,"identity_kind":"TEST","display_label":label,
            "tenant_id":tenant,"principal_id":principal
        }))).await;
        assert_eq!(status,StatusCode::CREATED,"trusted identity provisioning for {label}");
        let value:Value=serde_json::from_slice(&body).unwrap();
        Uuid::parse_str(value["ai_id"].as_str().unwrap()).unwrap()
    }

    let ai1=provision_identity(&app,&token1,tenant_id,p1,"P2.33-PHONE-01").await;
    let ai2=provision_identity(&app,&token2,tenant_id,p2,"P2.33-PHONE-02").await;
    let ai3=provision_identity(&app,&token3,tenant_id,p3,"P2.33-PHONE-03").await;

    async fn provision_phone(app:&Router,token:&str)->Uuid {
        let (status,body)=call(app,"POST","/ai-phone/provision",token,Some(serde_json::json!({}))).await;
        assert_eq!(status,StatusCode::CREATED,"phone identity is derived from authenticated principal");
        let value:Value=serde_json::from_slice(&body).unwrap();
        Uuid::parse_str(value["ai_phone_id"].as_str().unwrap()).unwrap()
    }
    let phone1=provision_phone(&app,&token1).await;
    let phone2=provision_phone(&app,&token2).await;
    let phone3=provision_phone(&app,&token3).await;

    let (spoof_status,_)=call(&app,"POST","/ai-phone/provision",&token1,Some(serde_json::json!({
        "principal_id":p2,"ai_id":ai2,"ai_phone_id":phone2
    }))).await;
    assert_eq!(spoof_status,StatusCode::UNPROCESSABLE_ENTITY,"phone provisioning rejects client-selected principal, AI and phone IDs");

    let (grant_status,grant_body)=call(&app,"POST","/ai-phone/pair-grants",&token1,Some(serde_json::json!({
        "destination_ai_phone_id":phone2
    }))).await;
    assert_eq!(grant_status,StatusCode::CREATED,"Phone-01 -> Phone-02 explicit grant is created");
    let grant:Value=serde_json::from_slice(&grant_body).unwrap();
    let grant_id=Uuid::parse_str(grant["grant_id"].as_str().unwrap()).unwrap();
    assert_eq!(grant["source_ai_phone_id"],phone1.to_string());
    assert_eq!(grant["destination_ai_phone_id"],phone2.to_string());

    let (duplicate_status,_)=call(&app,"POST","/ai-phone/pair-grants",&token1,Some(serde_json::json!({
        "destination_ai_phone_id":phone2
    }))).await;
    assert_eq!(duplicate_status,StatusCode::CONFLICT,"duplicate active grants are rejected");

    let (allow_status,allow_body)=call(&app,"POST",&format!("/ai-phone/{phone2}/messages"),&token1,Some(serde_json::json!({
        "idempotency_key":"p2.33-allow-01-to-02","content":{"text":"authorized pair"}
    }))).await;
    assert_eq!(allow_status,StatusCode::CREATED,"Phone-01 -> Phone-02 is allowed");
    let allowed:Value=serde_json::from_slice(&allow_body).unwrap();
    let allowed_id=Uuid::parse_str(allowed["message_id"].as_str().unwrap()).unwrap();
    let stored:(Uuid,Uuid,Uuid,Uuid,Uuid,i64,i64)=sqlx::query_as(
        "SELECT source_ai_phone_id,ai_phone_id,sender_principal_id,destination_principal_id,room_id,source_binding_version,destination_binding_version FROM phone_messages WHERE message_id=$1"
    ).bind(allowed_id).fetch_one(&pool).await.unwrap();
    assert_eq!(stored.0,phone1,"source phone is server-derived");
    assert_eq!(stored.1,phone2,"destination phone is server-resolved");
    assert_eq!(stored.2,p1,"sender principal is server-derived");
    assert_eq!(stored.3,p2,"destination principal is server-resolved");
    assert_eq!(stored.4,room_id,"room is server-derived from authenticated session");
    assert!(stored.5>0 && stored.6>0,"message records both identity binding versions");

    let (deny_status,_)=call(&app,"POST",&format!("/ai-phone/{phone3}/messages"),&token1,Some(serde_json::json!({
        "idempotency_key":"p2.33-deny-01-to-03","content":{"text":"no grant"}
    }))).await;
    assert_eq!(deny_status,StatusCode::FORBIDDEN,"Phone-01 -> Phone-03 is denied without a grant");

    let (reverse_deny,_)=call(&app,"POST",&format!("/ai-phone/{phone1}/messages"),&token2,Some(serde_json::json!({
        "idempotency_key":"p2.33-deny-02-to-01","content":{"text":"no reverse grant"}
    }))).await;
    assert_eq!(reverse_deny,StatusCode::FORBIDDEN,"authorization is directional");

    for claim in ["origin","destination","peer","room","sender","room_id","sender_principal_id","source_ai_phone_id"] {
        let mut content=serde_json::Map::new();
        content.insert(claim.to_string(),serde_json::json!("forged"));
        content.insert("text".to_string(),serde_json::json!("reject"));
        let (status,_)=call(&app,"POST",&format!("/ai-phone/{phone2}/messages"),&token1,Some(serde_json::json!({
            "idempotency_key":format!("p2.33-spoof-{claim}"),"content":Value::Object(content)
        }))).await;
        assert_eq!(status,StatusCode::BAD_REQUEST,"nested {claim} claim is rejected");
    }
    let (outer_spoof,_)=call(&app,"POST",&format!("/ai-phone/{phone2}/messages"),&token1,Some(serde_json::json!({
        "idempotency_key":"p2.33-outer-spoof","content":{"text":"reject"},"origin":"forged"
    }))).await;
    assert_eq!(outer_spoof,StatusCode::UNPROCESSABLE_ENTITY,"unknown outer identity fields are rejected");

    let (pending_status,pending_body)=call(&app,"POST",&format!("/ai-phone/{phone2}/messages"),&token1,Some(serde_json::json!({
        "idempotency_key":"p2.33-pending-before-revoke","content":{"text":"must fail after grant revoke"}
    }))).await;
    assert_eq!(pending_status,StatusCode::CREATED);
    let pending:Value=serde_json::from_slice(&pending_body).unwrap();
    let pending_id=Uuid::parse_str(pending["message_id"].as_str().unwrap()).unwrap();
    let (revoke_status,_)=call(&app,"POST",&format!("/ai-phone/pair-grants/{grant_id}/revoke"),&token1,Some(serde_json::json!({
        "reason":"P2.33 focused revocation test"
    }))).await;
    assert_eq!(revoke_status,StatusCode::NO_CONTENT,"directional grant revocation succeeds");
    let pending_state:String=sqlx::query_scalar("SELECT message_state FROM phone_messages WHERE message_id=$1")
        .bind(pending_id).fetch_one(&pool).await.unwrap();
    assert_eq!(pending_state,"FAILED","revocation prevents pending delivery");
    let (after_revoke,_)=call(&app,"POST",&format!("/ai-phone/{phone2}/messages"),&token1,Some(serde_json::json!({
        "idempotency_key":"p2.33-after-revoke","content":{"text":"deny"}
    }))).await;
    assert_eq!(after_revoke,StatusCode::FORBIDDEN,"revoked grant denies new sends");

    let grant_events:i64=sqlx::query_scalar("SELECT COUNT(*) FROM phone_pair_grant_events WHERE grant_id=$1 AND event_type IN ('GRANTED','REVOKED')")
        .bind(grant_id).fetch_one(&pool).await.unwrap();
    assert_eq!(grant_events,2,"grant creation and revocation are append-only audited");

    sqlx::query("UPDATE ai_identity_principals SET binding_status='REVOKED',revoked_at=NOW(),revocation_reason='P2.33 identity revocation test' WHERE tenant_id=$1 AND principal_id=$2 AND binding_status='ACTIVE'")
        .bind(tenant_id).bind(p3).execute(&pool).await.unwrap();
    let phone3_state:String=sqlx::query_scalar("SELECT phone_status FROM ai_phones WHERE ai_phone_id=$1")
        .bind(phone3).fetch_one(&pool).await.unwrap();
    assert_eq!(phone3_state,"REVOKED","identity binding revocation terminally revokes its phone");
    let session3_state:String=sqlx::query_scalar("SELECT status FROM sessions WHERE session_id=$1")
        .bind(s3).fetch_one(&pool).await.unwrap();
    let execution3_state:String=sqlx::query_scalar("SELECT status FROM workload_executions WHERE execution_id=$1")
        .bind(e3).fetch_one(&pool).await.unwrap();
    assert_eq!(session3_state,"REVOKED","identity revocation invalidates sessions");
    assert_eq!(execution3_state,"REVOKED","identity revocation invalidates executions");

    let binding_reuse=sqlx::query("UPDATE ai_identity_principals SET binding_status='ACTIVE',revoked_at=NULL,revocation_reason=NULL WHERE tenant_id=$1 AND principal_id=$2")
        .bind(tenant_id).bind(p3).execute(&pool).await;
    assert!(binding_reuse.is_err(),"revoked principal binding cannot be reactivated");
    let phone_reuse=sqlx::query("UPDATE ai_phones SET phone_status='DISCONNECTED',revoked_at=NULL,revocation_reason=NULL WHERE ai_phone_id=$1")
        .bind(phone3).execute(&pool).await;
    assert!(phone_reuse.is_err(),"revoked AI_PHONE_ID cannot be reactivated");
    sqlx::query("UPDATE ai_identities SET identity_status='RETIRED',retired_at=NOW(),retirement_reason='P2.33 identity retirement test' WHERE ai_id=$1")
        .bind(ai3).execute(&pool).await.unwrap();
    let identity_reuse=sqlx::query("UPDATE ai_identities SET identity_status='ACTIVE',retired_at=NULL,retirement_reason=NULL WHERE ai_id=$1")
        .bind(ai3).execute(&pool).await;
    assert!(identity_reuse.is_err(),"retired AI_ID cannot be reactivated");

    let (reuse_status,_)=call(&app,"POST","/ai-phone/identities",&token1,Some(serde_json::json!({
        "identity_key":format!("p2.33:P2.33-PHONE-03:{p3}"),
        "identity_kind":"TEST","display_label":"P2.33-PHONE-03",
        "tenant_id":tenant_id,"principal_id":p3
    }))).await;
    assert_eq!(reuse_status,StatusCode::FORBIDDEN,"retired AI_ID key cannot be reused");

    let identity_events:i64=sqlx::query_scalar("SELECT COUNT(*) FROM ai_identity_binding_events WHERE ai_id=$1 AND event_type IN ('CREATED','REVOKED')")
        .bind(ai3).fetch_one(&pool).await.unwrap();
    let phone_events:i64=sqlx::query_scalar("SELECT COUNT(*) FROM ai_phone_lifecycle_events WHERE ai_phone_id=$1 AND event_type IN ('CREATED','REVOKED')")
        .bind(phone3).fetch_one(&pool).await.unwrap();
    assert_eq!(identity_events,2,"identity binding creation and revocation are audited");
    assert_eq!(phone_events,2,"phone creation and revocation are audited");

    // Cross-tenant destination identifiers are not resolvable by another tenant.
    let tenant_b=Uuid::new_v4();
    let room_b=Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'P2.33 foreign tenant')")
        .bind(tenant_b).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'P2.33 foreign room')")
        .bind(room_b).bind(tenant_b).execute(&pool).await.unwrap();
    let (p4,_,s4,_,_,_)=add_ai(&pool,tenant_b,room_b,"P2.33-PHONE-04").await;
    grant_permissions(&pool,tenant_b,room_b,p4).await;
    let token4=bearer_for(&pool,s4).await;
    let ai4=provision_identity(&app,&token4,tenant_b,p4,"P2.33-PHONE-04").await;
    let phone4=provision_phone(&app,&token4).await;
    let (cross_tenant_status,_)=call(&app,"POST",&format!("/ai-phone/{phone4}/messages"),&token1,Some(serde_json::json!({
        "idempotency_key":"p2.33-cross-tenant","content":{"text":"must not resolve foreign phone"}
    }))).await;
    assert_eq!(cross_tenant_status,StatusCode::NOT_FOUND,"cross-tenant phone IDs are concealed");
    let _=(e1,e2,ai1,ai2,ai4);
}
