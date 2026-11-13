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
async fn ai_phone_02_has_independent_identity_phone_mailbox_and_authorization() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set; this test requires real PostgreSQL 16");
    let pool = connect_pool(&database_url)
        .await
        .expect("connect to PostgreSQL");
    migrate(&pool).await.expect("apply Team Room migrations");

    let tenant_id = Uuid::new_v4();
    let room_id = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'AI-PHONE-02 integration tenant')")
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'AI-PHONE-02 integration room')")
        .bind(room_id)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let (ai_1_principal, _, ai_1_session, _, _, _) =
        add_ai(&pool, tenant_id, room_id, "AI-PHONE-01-TEST").await;
    let (ai_2_principal, ai_2_participant, ai_2_session, ai_2_execution, _, _) =
        add_ai(&pool, tenant_id, room_id, "AI-PHONE-02-TEST").await;
    grant_permissions(&pool, tenant_id, room_id, ai_1_principal).await;
    grant_permissions(&pool, tenant_id, room_id, ai_2_principal).await;

    let ai_1_id = Uuid::new_v4();
    let ai_1_key = format!("phone-02-test:ai-1:{ai_1_id}");
    sqlx::query(
        "INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','AI-PHONE-01-TEST')",
    )
    .bind(ai_1_id)
    .bind(&ai_1_key)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)",
    )
    .bind(ai_1_id)
    .bind(tenant_id)
    .bind(ai_1_principal)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
        .bind(ai_1_id)
        .bind(tenant_id)
        .execute(&pool)
        .await
        .unwrap();

    let token_1 = bearer_for(&pool, ai_1_session).await;
    let token_2 = bearer_for(&pool, ai_2_session).await;
    // Test-only operator keys model separately provisioned control-plane secrets.
    let operator_key_1 = format!("operator-key-phone-01-{}", Uuid::new_v4());
    let operator_key_2 = format!("operator-key-phone-02-{}", Uuid::new_v4());
    sqlx::query("INSERT INTO phone_endpoint_operator_keys(operator_key_id,tenant_id,ai_id,key_hash) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4()).bind(tenant_id).bind(ai_1_id)
        .bind(Sha256::digest(operator_key_1.as_bytes()).to_vec())
        .execute(&pool).await.unwrap();
    let app = ai_team_room_service::phone::router(pool.clone());

    let (status, body) = call(
        &app,
        "POST",
        "/ai-phone/provision",
        &token_1,
        Some(serde_json::json!({"principal_id": ai_1_principal})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "Phone #1 fixture is explicitly provisioned");
    let phone_1: Value = serde_json::from_slice(&body).unwrap();
    let phone_1_id = Uuid::parse_str(phone_1["ai_phone_id"].as_str().unwrap()).unwrap();

    let (status, body) = call(
        &app,
        "POST",
        "/ai-phone/identities",
        &token_1,
        Some(serde_json::json!({
            "identity_key": format!("phone-02-test:ai-2:{}", ai_2_principal),
            "identity_kind": "TEST",
            "display_label": "AI-PHONE-02-TEST",
            "tenant_id": tenant_id,
            "principal_id": ai_2_principal
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "AI #2 receives its own stable identity binding");
    let identity_2: Value = serde_json::from_slice(&body).unwrap();
    let ai_2_id = Uuid::parse_str(identity_2["ai_id"].as_str().unwrap()).unwrap();
    assert_ne!(ai_1_id, ai_2_id, "AI #2 must not reuse AI #1 identity");
    sqlx::query("INSERT INTO phone_endpoint_operator_keys(operator_key_id,tenant_id,ai_id,key_hash) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4()).bind(tenant_id).bind(ai_2_id)
        .bind(Sha256::digest(operator_key_2.as_bytes()).to_vec())
        .execute(&pool).await.unwrap();

    let provision_body = serde_json::json!({"principal_id": ai_2_principal});
    let (first, second, third) = tokio::join!(
        call(&app, "POST", "/ai-phone/provision", &token_2, Some(provision_body.clone())),
        call(&app, "POST", "/ai-phone/provision", &token_2, Some(provision_body.clone())),
        call(&app, "POST", "/ai-phone/provision", &token_2, Some(provision_body)),
    );
    let results = [first, second, third];
    let mut phone_2_ids = Vec::new();
    let mut created = 0;
    let mut reused = 0;
    for (status, body) in results {
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "concurrent provisioning must return CREATED or idempotent OK, got {status}"
        );
        if status == StatusCode::CREATED {
            created += 1;
        } else {
            reused += 1;
        }
        let phone: Value = serde_json::from_slice(&body).unwrap();
        phone_2_ids.push(phone["ai_phone_id"].as_str().unwrap().to_owned());
        assert_eq!(phone["ai_id"], ai_2_id.to_string());
    }
    assert_eq!(created, 1, "exactly one concurrent request creates Phone #2");
    assert_eq!(reused, 2, "the other concurrent requests reuse Phone #2");
    assert!(phone_2_ids.windows(2).all(|pair| pair[0] == pair[1]));
    let phone_2_id = Uuid::parse_str(&phone_2_ids[0]).unwrap();
    assert_ne!(phone_1_id, phone_2_id, "AI #2 must receive a distinct AI_PHONE_ID");

    let (status, body) = call(&app, "GET", "/ai-phone/me", &token_2, None).await;
    assert_eq!(status, StatusCode::OK);
    let snapshot_2: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(snapshot_2["phone"]["ai_id"], ai_2_id.to_string());
    assert_eq!(snapshot_2["phone"]["ai_phone_id"], phone_2_id.to_string());

    let (status, _) = call(
        &app,
        "GET",
        &format!("/ai-phone/{phone_1_id}/messages"),
        &token_2,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "AI #2 cannot read Phone #1 mailbox");
    let (status, _) = call(
        &app,
        "GET",
        &format!("/ai-phone/{phone_2_id}/messages"),
        &token_1,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "AI #1 cannot read Phone #2 mailbox");

    let endpoint_2 = Uuid::new_v4();
    let endpoint_1 = Uuid::new_v4();
    let (status, _) = call(&app,"POST","/ai-phone/endpoints/register",&token_1,
        Some(serde_json::json!({"endpoint_id":endpoint_1,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-01-endpoint"}}))).await;
    assert_eq!(status,StatusCode::FORBIDDEN,
        "AI bearer alone cannot enroll an endpoint without the independent operator key");
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_1,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":endpoint_1,"endpoint_type":"TEST","metadata":{"label":"wrong-operator-key"}
        }))).await;
    assert_eq!(status,StatusCode::FORBIDDEN,
        "operator key scoped to AI #2 cannot enroll an endpoint for AI #1");
    let operator_hash:Vec<u8>=sqlx::query_scalar("SELECT key_hash FROM phone_endpoint_operator_keys WHERE tenant_id=$1 AND ai_id=$2 AND key_status='ACTIVE'")
        .bind(tenant_id).bind(ai_2_id).fetch_one(&pool).await.unwrap();
    assert_eq!(operator_hash,Sha256::digest(operator_key_2.as_bytes()).to_vec());
    assert_eq!(operator_hash.len(),32);
    assert_ne!(operator_hash,operator_key_2.as_bytes(),
        "operator key plaintext must never be stored in PostgreSQL");
    let (status, body) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_1,
        None,Some(&operator_key_1),
        Some(serde_json::json!({"endpoint_id":endpoint_1,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-01-endpoint"}}))).await;
    assert_eq!(status,StatusCode::CREATED);
    let endpoint_1_registration:Value=serde_json::from_slice(&body).unwrap();
    let endpoint_1_credential=endpoint_1_registration["endpoint_credential"].as_str().unwrap().to_owned();

    let (status, body) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),
        Some(serde_json::json!({"endpoint_id":endpoint_2,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-endpoint"}}))).await;
    assert_eq!(status,StatusCode::CREATED);
    let endpoint_2_registration:Value=serde_json::from_slice(&body).unwrap();
    let mut endpoint_2_credential=endpoint_2_registration["endpoint_credential"].as_str().unwrap().to_owned();
    let stored_hash:Vec<u8>=sqlx::query_scalar("SELECT credential_hash FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND tenant_id=$2 AND ai_id=$3 AND credential_status='ACTIVE'")
        .bind(endpoint_2).bind(tenant_id).bind(ai_2_id).fetch_one(&pool).await.unwrap();
    assert_eq!(stored_hash,Sha256::digest(endpoint_2_credential.as_bytes()).to_vec());
    assert_eq!(stored_hash.len(),32);
    assert_ne!(stored_hash,endpoint_2_credential.as_bytes(),
        "endpoint credential plaintext must never be stored in PostgreSQL");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/attach",&token_1,
        Some(&endpoint_2_credential),None,
        Some(serde_json::json!({"endpoint_id":endpoint_2,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-endpoint"}}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI #1 cannot attach AI-PHONE-02 endpoint using AI #2 endpoint credential");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/attach",&token_2,
        Some(&endpoint_1_credential),None,
        Some(serde_json::json!({"endpoint_id":endpoint_1,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-01-endpoint"}}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI #2 cannot attach AI-PHONE-01 endpoint using AI #1 endpoint credential");

    let phone_2_messages_path = format!("/ai-phone/{phone_2_id}/messages");
    let same_payload = serde_json::json!({
        "idempotency_key": "phone-02-concurrent-idempotency",
        "content": {"text": "durable message addressed to AI-PHONE-02"}
    });
    let (send_1, send_2, send_3) = tokio::join!(
        call(
            &app,
            "POST",
            &phone_2_messages_path,
            &token_1,
            Some(same_payload.clone())
        ),
        call(
            &app,
            "POST",
            &phone_2_messages_path,
            &token_1,
            Some(same_payload.clone())
        ),
        call(
            &app,
            "POST",
            &phone_2_messages_path,
            &token_1,
            Some(same_payload.clone())
        ),
    );
    let sends = [send_1, send_2, send_3];
    let mut message_ids = Vec::new();
    let mut created_messages = 0;
    for (status, body) in sends {
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "concurrent idempotent send must succeed, got {status}"
        );
        if status == StatusCode::CREATED {
            created_messages += 1;
        }
        let message: Value = serde_json::from_slice(&body).unwrap();
        message_ids.push(message["message_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(created_messages, 1, "only one concurrent send inserts a message");
    assert!(message_ids.windows(2).all(|pair| pair[0] == pair[1]));

    let (status, _) = call(
        &app,
        "POST",
        &format!("/ai-phone/{phone_2_id}/messages"),
        &token_1,
        Some(serde_json::json!({
            "idempotency_key": "phone-02-concurrent-idempotency",
            "content": {"text": "payload mutation must conflict"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "same key with a different payload is rejected");

    let (status, body) = call(
        &app,
        "POST",
        &format!("/ai-phone/{phone_2_id}/messages"),
        &token_1,
        Some(serde_json::json!({
            "idempotency_key": "phone-02-sequence-next",
            "content": {"text": "second ordered message"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let second_message: Value = serde_json::from_slice(&body).unwrap();
    let first_message_id = Uuid::parse_str(&message_ids[0]).unwrap();
    let second_message_id =
        Uuid::parse_str(second_message["message_id"].as_str().unwrap()).unwrap();
    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT sequence_no FROM phone_messages WHERE message_id IN ($1,$2) ORDER BY sequence_no",
    )
    .bind(first_message_id)
    .bind(second_message_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(sequences.len(), 2);
    assert!(sequences[0] < sequences[1], "mailbox sequence is monotonic");

    let first_message_state: String = sqlx::query_scalar(
        "SELECT message_state FROM phone_messages WHERE message_id=$1 AND ai_phone_id=$2 AND tenant_id=$3",
    )
    .bind(first_message_id)
    .bind(phone_2_id)
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        first_message_state, "PENDING",
        "messages persist while Phone #2 is offline"
    );
    let (status, _) = call_with_headers(&app,"POST",
        &format!("/ai-phone/{phone_2_id}/messages/{first_message_id}/ack"),&token_2,
        Some(&endpoint_2_credential),None,None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "AI bearer without an attached endpoint credential cannot ACK mailbox messages"
    );

    let (status, body) = call_with_headers(
        &app,
        "POST",
        "/ai-phone/attach",
        &token_2,
        Some(&endpoint_2_credential),
        None,
        Some(serde_json::json!({
            "endpoint_id": endpoint_2,
            "endpoint_type": "TEST",
            "metadata": {"label": "AI-PHONE-02-endpoint"}
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "Phone #2 attaches its own endpoint after offline messages queue"
    );
    let initial_attachment: Value = serde_json::from_slice(&body).unwrap();
    let initial_fence = initial_attachment["endpoint_fence"].as_i64().unwrap();

    // Active endpoint credential rotation preserves the generation but revokes the old secret.
    let endpoint_2_pre_rotation_credential=endpoint_2_credential.clone();
    let (status,body)=call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":endpoint_2,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-endpoint"}
        }))).await;
    assert_eq!(status,StatusCode::CREATED);
    let rotated_registration:Value=serde_json::from_slice(&body).unwrap();
    endpoint_2_credential=rotated_registration["endpoint_credential"].as_str().unwrap().to_owned();
    let rotated_fence:Option<i64>=sqlx::query_scalar("SELECT endpoint_fence FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND credential_hash=$2 AND credential_status='ACTIVE'")
        .bind(endpoint_2).bind(Sha256::digest(endpoint_2_credential.as_bytes()).to_vec())
        .fetch_one(&pool).await.unwrap();
    assert_eq!(rotated_fence,Some(initial_fence),"active rotation binds new credential to the current generation");
    let old_credential_status:String=sqlx::query_scalar("SELECT credential_status FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND credential_hash=$2")
        .bind(endpoint_2).bind(Sha256::digest(endpoint_2_pre_rotation_credential.as_bytes()).to_vec())
        .fetch_one(&pool).await.unwrap();
    assert_eq!(old_credential_status,"REVOKED");
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_pre_rotation_credential),None,Some(serde_json::json!({"endpoint_fence":initial_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"pre-rotation credential cannot heartbeat after rotation");
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":initial_fence}))).await;
    assert_eq!(status,StatusCode::OK,"rotated endpoint credential remains valid for the same fence");

    let (status, body) = call_with_headers(
        &app,"GET",&format!("/ai-phone/{phone_2_id}/messages"),&token_2,
        Some(&endpoint_2_credential),None,None
    ).await;
    assert_eq!(status, StatusCode::OK, "AI #2 endpoint reads its own durable mailbox");
    let inbox: Vec<Value> = serde_json::from_slice(&body).unwrap();
    assert!(
        inbox.iter().any(|message| message["message_id"] == first_message_id.to_string()),
        "the persisted offline/sent message is available to Phone #2"
    );

    let (status, _) = call_with_headers(
        &app,"POST",&format!("/ai-phone/{phone_2_id}/messages/{first_message_id}/ack"),&token_1,
        Some(&endpoint_1_credential),None,None
    ).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "AI #1 cannot address Phone #2's message through its own phone context"
    );

    let (status, _) = call_with_headers(
        &app,"POST",&format!("/ai-phone/{phone_2_id}/messages/{second_message_id}/ack"),&token_2,
        Some(&endpoint_2_credential),None,None
    ).await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "AI #2 can ACK a later delivered sequence before an earlier one"
    );
    let (status, _) = call_with_headers(&app,"POST",
        &format!("/ai-phone/{phone_2_id}/messages/{first_message_id}/ack"),&token_2,
        Some(&endpoint_2_credential),None,None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "AI #2 ACKs the earlier delivered message");
    let (status, _) = call_with_headers(&app,"POST",
        &format!("/ai-phone/{phone_2_id}/messages/{first_message_id}/ack"),&token_2,
        Some(&endpoint_2_credential),None,None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "duplicate ACK is idempotent");

    // Team Room remains the task authority: Phone #2 can link only a task owned by AI #2.
    let ai_2_task = Uuid::new_v4();
    let ai_2_mission = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO team_tasks(task_id,tenant_id,room_id,mission_id,title,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_issued_at,lease_expires_at,created_by_principal_id) VALUES($1,$2,$3,$4,'AI-PHONE-02 mission ownership','OWNED',$5,$6,$7,$8,1,NOW(),NOW()+INTERVAL '60 seconds',$5)",
    )
    .bind(ai_2_task)
    .bind(tenant_id)
    .bind(room_id)
    .bind(ai_2_mission)
    .bind(ai_2_principal)
    .bind(ai_2_participant)
    .bind(ai_2_session)
    .bind(ai_2_execution)
    .execute(&pool)
    .await
    .unwrap();

    let (status, _) = call(
        &app,
        "POST",
        "/ai-phone/mission",
        &token_2,
        Some(serde_json::json!({"task_id": ai_2_task})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "task.read alone cannot change Phone #2 mission linkage"
    );
    grant_permission(&pool, tenant_id, room_id, ai_2_principal, "task.transition").await;
    let (status, _) = call(
        &app,
        "POST",
        "/ai-phone/mission",
        &token_2,
        Some(serde_json::json!({"task_id": ai_2_task})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "AI #2 with task.transition can link its own Team Room task"
    );
    grant_permission(&pool, tenant_id, room_id, ai_1_principal, "task.transition").await;
    let (status, _) = call(
        &app,
        "POST",
        "/ai-phone/mission",
        &token_1,
        Some(serde_json::json!({"task_id": ai_2_task})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "AI #1 cannot link AI #2-owned task to Phone #1"
    );
    let (status, body) = call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&endpoint_2_credential),None,None).await;
    assert_eq!(status, StatusCode::OK);
    let linked_phone_2: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(linked_phone_2["phone"]["current_task_id"], ai_2_task.to_string());
    assert_eq!(linked_phone_2["phone"]["current_mission_id"], ai_2_mission.to_string());

    // Phone #2 lifecycle: heartbeat, detach, stale-fence rejection, reconnect and transfer.
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":initial_fence}))).await;
    assert_eq!(status, StatusCode::OK, "current Phone #2 endpoint heartbeat succeeds");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/detach",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":initial_fence}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "Phone #2 endpoint detaches");
    let (status, _) = call(
        &app,
        "POST",
        "/ai-phone/heartbeat",
        &token_2,
        Some(serde_json::json!({"endpoint_fence": initial_fence})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "detached endpoint credential is revoked and cannot heartbeat"
    );

    let (status, body) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":endpoint_2,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-endpoint"}
        }))).await;
    assert_eq!(status,StatusCode::CREATED,"operator rotates endpoint credential after detach");
    let registration:Value=serde_json::from_slice(&body).unwrap();
    endpoint_2_credential=registration["endpoint_credential"].as_str().unwrap().to_owned();
    let reconnect_body=serde_json::json!({
        "endpoint_id":endpoint_2,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-endpoint"}
    });
    let (reconnect_a,reconnect_b)=tokio::join!(
        call_with_headers(&app,"POST","/ai-phone/reconnect",&token_2,Some(&endpoint_2_credential),None,Some(reconnect_body.clone())),
        call_with_headers(&app,"POST","/ai-phone/reconnect",&token_2,Some(&endpoint_2_credential),None,Some(reconnect_body))
    );
    assert_eq!(reconnect_a.0,StatusCode::OK,"first concurrent reconnect attaches the endpoint");
    assert_eq!(reconnect_b.0,StatusCode::OK,"second concurrent reconnect is idempotent");
    let reconnect_a:Value=serde_json::from_slice(&reconnect_a.1).unwrap();
    let reconnect_b:Value=serde_json::from_slice(&reconnect_b.1).unwrap();
    assert_eq!(reconnect_a["attachment_id"],reconnect_b["attachment_id"],
        "concurrent reconnect requests converge on one attachment");
    let reconnect_fence=reconnect_b["endpoint_fence"].as_i64().unwrap();
    assert!(reconnect_fence > initial_fence, "reconnect advances the endpoint fence");

    let endpoint_3 = Uuid::new_v4();
    let (status, body) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":endpoint_3,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-transfer-target"}
        }))).await;
    assert_eq!(status,StatusCode::CREATED);
    let endpoint_3_registration:Value=serde_json::from_slice(&body).unwrap();
    let endpoint_3_credential=endpoint_3_registration["endpoint_credential"].as_str().unwrap().to_owned();
    let (status, body) = call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,
        Some(&endpoint_3_credential),None,Some(serde_json::json!({
            "endpoint_id":endpoint_3,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-transfer-target"}
        }))).await;
    assert_eq!(status, StatusCode::OK, "Phone #2 endpoint transfer succeeds");
    let transfer: Value = serde_json::from_slice(&body).unwrap();
    let transfer_fence = transfer["endpoint_fence"].as_i64().unwrap();
    assert!(transfer_fence > reconnect_fence, "transfer advances fencing token");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":reconnect_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "transferred-away endpoint credential is revoked even if old fence is supplied");

    // Attack A: a process retaining only the AI bearer cannot recover the active generation.
    let (status, _) = call(&app,"GET","/ai-phone/me",&token_2,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI bearer alone cannot read the active endpoint generation");
    let (status, _) = call(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI bearer plus a guessed/observed fence cannot heartbeat without endpoint credential");
    let (status,_) = call(&app,"GET",&phone_2_messages_path,&token_2,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI bearer alone cannot read/deliver the endpoint mailbox");
    let (status,_) = call(&app,"POST",
        &format!("/ai-phone/{phone_2_id}/messages/{first_message_id}/ack"),&token_2,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI bearer alone cannot ACK a mailbox message");
    let (status,body)=call(&app,"GET",&format!("/ai-phone/{phone_2_id}/endpoints"),&token_2,None).await;
    assert_eq!(status,StatusCode::OK,"AI authority may inspect endpoint metadata");
    let endpoint_metadata:Value=serde_json::from_slice(&body).unwrap();
    assert!(!endpoint_metadata.to_string().contains("endpoint_fence"));
    assert!(!endpoint_metadata.to_string().contains("endpoint_credential"));
    let (status,body)=call(&app,"GET",&format!("/ai-phone/{phone_2_id}/presence"),&token_2,None).await;
    assert_eq!(status,StatusCode::OK,"AI authority may inspect control-plane presence");
    assert!(!String::from_utf8_lossy(&body).contains("endpoint_fence"));
    assert!(!String::from_utf8_lossy(&body).contains("endpoint_credential"));
    let (status,_) = call(&app,"POST",&phone_2_messages_path,&token_2,
        Some(serde_json::json!({"idempotency_key":"old-process-ai-send","content":{"text":"AI authority remains separate from endpoint authority"}}))).await;
    assert_eq!(status,StatusCode::CREATED,
        "durable outbound message submission remains AI-authorized, not endpoint-authorized");
    // Attack B: the transferred-away endpoint's own credential is terminally revoked.
    let (status, _) = call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&endpoint_2_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"old endpoint credential cannot recover the new generation");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"old endpoint credential cannot heartbeat as the new generation");
    // Attack C: a still-active credential belonging to endpoint A cannot authenticate endpoint B.
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_1_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"credential A cannot authenticate endpoint B");
    let endpoint_4=Uuid::new_v4();
    let (status,body)=call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({"endpoint_id":endpoint_4,"endpoint_type":"TEST","metadata":{"label":"unattached-endpoint"}}))).await;
    assert_eq!(status,StatusCode::CREATED);
    let endpoint_4_initial:Value=serde_json::from_slice(&body).unwrap();
    let endpoint_4_initial_credential=endpoint_4_initial["endpoint_credential"].as_str().unwrap().to_owned();

    // Two concurrent credential rotations serialize on the phone row. Exactly one digest remains ACTIVE.
    let register_body=serde_json::json!({"endpoint_id":endpoint_4,"endpoint_type":"TEST","metadata":{"label":"unattached-endpoint"}});
    let (rotation_a,rotation_b)=tokio::join!(
        call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,None,Some(&operator_key_2),Some(register_body.clone())),
        call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,None,Some(&operator_key_2),Some(register_body))
    );
    assert_eq!(rotation_a.0,StatusCode::CREATED);
    assert_eq!(rotation_b.0,StatusCode::CREATED);
    let rotation_a:Value=serde_json::from_slice(&rotation_a.1).unwrap();
    let rotation_b:Value=serde_json::from_slice(&rotation_b.1).unwrap();
    let endpoint_4_credential_a=rotation_a["endpoint_credential"].as_str().unwrap().to_owned();
    let endpoint_4_credential_b=rotation_b["endpoint_credential"].as_str().unwrap().to_owned();
    let active_hash:Vec<u8>=sqlx::query_scalar("SELECT credential_hash FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND credential_status='ACTIVE'")
        .bind(endpoint_4).fetch_one(&pool).await.unwrap();
    assert_eq!(active_hash==Sha256::digest(endpoint_4_credential_a.as_bytes()).to_vec()
        || active_hash==Sha256::digest(endpoint_4_credential_b.as_bytes()).to_vec(),true,
        "exactly one concurrently issued secret matches the sole active credential hash");
    let endpoint_4_credential=if active_hash==Sha256::digest(endpoint_4_credential_a.as_bytes()).to_vec() {
        endpoint_4_credential_a.clone()
    } else { endpoint_4_credential_b.clone() };
    let active_credential_count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND credential_status='ACTIVE'")
        .bind(endpoint_4).fetch_one(&pool).await.unwrap();
    assert_eq!(active_credential_count,1);
    let initial_status:String=sqlx::query_scalar("SELECT credential_status FROM phone_endpoint_credentials WHERE credential_hash=$1")
        .bind(Sha256::digest(endpoint_4_initial_credential.as_bytes()).to_vec()).fetch_one(&pool).await.unwrap();
    assert_eq!(initial_status,"REVOKED","credential rotation invalidates the previous secret");
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_4_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "registered-but-unattached endpoint credential cannot impersonate the active endpoint");
    sqlx::query("UPDATE phone_endpoint_credentials SET expires_at=NOW()-INTERVAL '1 second' WHERE endpoint_id=$1 AND credential_hash=$2")
        .bind(endpoint_4).bind(Sha256::digest(endpoint_4_credential.as_bytes()).to_vec())
        .execute(&pool).await.unwrap();
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,
        Some(&endpoint_4_credential),None,Some(serde_json::json!({
            "endpoint_id":endpoint_4,"endpoint_type":"TEST","metadata":{"label":"unattached-endpoint"}
        }))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "expired endpoint credential cannot transfer into authority");
    let unchanged_fence:i64=sqlx::query_scalar("SELECT endpoint_fence FROM ai_phones WHERE ai_phone_id=$1 AND tenant_id=$2 AND ai_id=$3")
        .bind(phone_2_id).bind(tenant_id).bind(ai_2_id).fetch_one(&pool).await.unwrap();
    assert_eq!(unchanged_fence,transfer_fence,
        "failed expired-credential transfer rolls back the old endpoint fencing state");
    let (status,_) = call_with_headers(&app,"POST",&format!("/ai-phone/endpoints/{endpoint_4}/revoke"),&token_2,
        None,Some(&operator_key_2),None).await;
    assert_eq!(status,StatusCode::NO_CONTENT,"operator can revoke an unattached endpoint credential");
    let endpoint_4_status:String=sqlx::query_scalar("SELECT endpoint_status FROM phone_endpoints WHERE endpoint_id=$1")
        .bind(endpoint_4).fetch_one(&pool).await.unwrap();
    let endpoint_4_credential_status:String=sqlx::query_scalar("SELECT credential_status FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND credential_hash=$2")
        .bind(endpoint_4).bind(Sha256::digest(endpoint_4_credential.as_bytes()).to_vec()).fetch_one(&pool).await.unwrap();
    assert_eq!(endpoint_4_status,"REVOKED");
    assert_eq!(endpoint_4_credential_status,"REVOKED");
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":endpoint_4,"endpoint_type":"TEST","metadata":{"label":"unattached-endpoint"}
        }))).await;
    assert_eq!(status,StatusCode::CONFLICT,"revoked endpoint ID cannot be enrolled again");

    // Two simultaneous transfers plus heartbeat and ACK race against the old generation.
    let endpoint_5=Uuid::new_v4();
    let endpoint_6=Uuid::new_v4();
    let (reg5,reg6)=tokio::join!(
        call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,None,Some(&operator_key_2),
            Some(serde_json::json!({"endpoint_id":endpoint_5,"endpoint_type":"TEST","metadata":{"label":"transfer-race-5"}}))),
        call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,None,Some(&operator_key_2),
            Some(serde_json::json!({"endpoint_id":endpoint_6,"endpoint_type":"TEST","metadata":{"label":"transfer-race-6"}})))
    );
    assert_eq!(reg5.0,StatusCode::CREATED);
    assert_eq!(reg6.0,StatusCode::CREATED);
    let reg5:Value=serde_json::from_slice(&reg5.1).unwrap();
    let reg6:Value=serde_json::from_slice(&reg6.1).unwrap();
    let endpoint_5_credential=reg5["endpoint_credential"].as_str().unwrap().to_owned();
    let endpoint_6_credential=reg6["endpoint_credential"].as_str().unwrap().to_owned();

    let (status,body)=call(&app,"POST",&phone_2_messages_path,&token_1,
        Some(serde_json::json!({"idempotency_key":"phone-02-transfer-race-ack","content":{"text":"ACK races endpoint transfer"}}))).await;
    assert_eq!(status,StatusCode::CREATED);
    let race_message:Value=serde_json::from_slice(&body).unwrap();
    let race_message_id=race_message["message_id"].as_str().unwrap().to_owned();
    let (status,_) = call_with_headers(&app,"GET",&phone_2_messages_path,&token_2,
        Some(&endpoint_3_credential),None,None).await;
    assert_eq!(status,StatusCode::OK,"old generation can deliver before concurrent transfer");

    let transfer_body_5=serde_json::json!({"endpoint_id":endpoint_5,"endpoint_type":"TEST","metadata":{"label":"transfer-race-5"}});
    let transfer_body_6=serde_json::json!({"endpoint_id":endpoint_6,"endpoint_type":"TEST","metadata":{"label":"transfer-race-6"}});
    let race_ack_path=format!("/ai-phone/{phone_2_id}/messages/{race_message_id}/ack");
    let (transfer_5,transfer_6,heartbeat_old,ack_old)=tokio::join!(
        call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,Some(&endpoint_5_credential),None,Some(transfer_body_5)),
        call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,Some(&endpoint_6_credential),None,Some(transfer_body_6)),
        call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,Some(&endpoint_3_credential),None,
            Some(serde_json::json!({"endpoint_fence":transfer_fence}))),
        call_with_headers(&app,"POST",&race_ack_path,&token_2,
            Some(&endpoint_3_credential),None,None)
    );
    assert_eq!(transfer_5.0,StatusCode::OK,"first concurrent transfer is serialized and succeeds");
    assert_eq!(transfer_6.0,StatusCode::OK,"second concurrent transfer serializes after the first");
    assert!(heartbeat_old.0==StatusCode::OK || heartbeat_old.0==StatusCode::UNAUTHORIZED,
        "heartbeat racing transfer either commits before the fence or is rejected after it");
    assert!(ack_old.0==StatusCode::NO_CONTENT || ack_old.0==StatusCode::UNAUTHORIZED,
        "ACK racing transfer either commits before the fence or is rejected after it");

    let (current_endpoint_id,current_fence):(Uuid,i64)=sqlx::query_as(
        "SELECT a.endpoint_id,a.endpoint_fence FROM phone_attachments a WHERE a.ai_phone_id=$1 AND a.tenant_id=$2 AND a.ai_id=$3 AND a.attachment_status='ACTIVE'"
    ).bind(phone_2_id).bind(tenant_id).bind(ai_2_id).fetch_one(&pool).await.unwrap();
    assert!(current_endpoint_id==endpoint_5 || current_endpoint_id==endpoint_6,
        "exactly one target endpoint owns the final generation");
    let current_endpoint_credential=if current_endpoint_id==endpoint_5 {endpoint_5_credential.clone()} else {endpoint_6_credential.clone()};
    let active_race_credentials:i64=sqlx::query_scalar("SELECT COUNT(*) FROM phone_endpoint_credentials WHERE endpoint_id IN ($1,$2) AND credential_status='ACTIVE'")
        .bind(endpoint_5).bind(endpoint_6).fetch_one(&pool).await.unwrap();
    assert_eq!(active_race_credentials,1,"two concurrent transfers leave exactly one active target credential");
    let old_generation_status:String=sqlx::query_scalar("SELECT credential_status FROM phone_endpoint_credentials WHERE endpoint_id=$1 AND credential_hash=$2")
        .bind(endpoint_3).bind(Sha256::digest(endpoint_3_credential.as_bytes()).to_vec()).fetch_one(&pool).await.unwrap();
    assert_eq!(old_generation_status,"REVOKED","credential of the previous generation is revoked atomically");
    let (status,_) = call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&endpoint_3_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"transferred-away endpoint cannot recover the final generation");
    let (status,body)=call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&current_endpoint_credential),None,None).await;
    assert_eq!(status,StatusCode::OK);
    let current_snapshot:Value=serde_json::from_slice(&body).unwrap();
    assert_eq!(current_snapshot["attachment"]["endpoint_id"],current_endpoint_id.to_string());
    assert_eq!(current_snapshot["attachment"]["endpoint_fence"],current_fence);

    // Detach racing transfer: either detach commits first and transfer attaches after,
    // or transfer commits first and the old detach is rejected by credential revocation.
    let endpoint_7=Uuid::new_v4();
    let (register_7_status,register_7_body)=call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({"endpoint_id":endpoint_7,"endpoint_type":"TEST","metadata":{"label":"detach-transfer-race"}}))).await;
    assert_eq!(register_7_status,StatusCode::CREATED);
    let register_7:Value=serde_json::from_slice(&register_7_body).unwrap();
    let endpoint_7_credential=register_7["endpoint_credential"].as_str().unwrap().to_owned();
    let (detach_race,transfer_race)=tokio::join!(
        call_with_headers(&app,"POST","/ai-phone/detach",&token_2,Some(&current_endpoint_credential),None,
            Some(serde_json::json!({"endpoint_fence":current_fence}))),
        call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,Some(&endpoint_7_credential),None,
            Some(serde_json::json!({"endpoint_id":endpoint_7,"endpoint_type":"TEST","metadata":{"label":"detach-transfer-race"}})))
    );
    assert!(detach_race.0==StatusCode::NO_CONTENT || detach_race.0==StatusCode::UNAUTHORIZED,
        "detach racing transfer either completes first or is rejected after old credential revocation");
    assert_eq!(transfer_race.0,StatusCode::OK,"new endpoint transfer wins regardless of detach ordering");
    let (status,body)=call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&endpoint_7_credential),None,None).await;
    assert_eq!(status,StatusCode::OK);
    let final_snapshot:Value=serde_json::from_slice(&body).unwrap();
    assert_eq!(final_snapshot["attachment"]["endpoint_id"],endpoint_7.to_string());
    let current_endpoint_id=endpoint_7;
    let current_endpoint_credential=endpoint_7_credential;
    let current_fence=final_snapshot["attachment"]["endpoint_fence"].as_i64().unwrap();

    // Adversarial revocation: an ACTIVE attachment must stop working as soon as
    // its endpoint is marked REVOKED, even when the fencing generation is current.
    let (status, body) = call(
        &app,
        "POST",
        &phone_2_messages_path,
        &token_1,
        Some(serde_json::json!({
            "idempotency_key": "phone-02-revocation-probe",
            "content": {"text": "must not be acknowledged after endpoint revocation"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let revoked_probe: Value = serde_json::from_slice(&body).unwrap();
    let revoked_probe_id = Uuid::parse_str(revoked_probe["message_id"].as_str().unwrap()).unwrap();
    let (status, _) = call_with_headers(&app,"GET",&phone_2_messages_path,&token_2,
        Some(&current_endpoint_credential),None,None).await;
    assert_eq!(status,StatusCode::OK,"probe message is delivered before revocation");
    let revoke_endpoint_path=format!("/ai-phone/endpoints/{current_endpoint_id}/revoke");
    let (heartbeat_during_revoke,revoke_result)=tokio::join!(
        call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,Some(&current_endpoint_credential),None,
            Some(serde_json::json!({"endpoint_fence":current_fence}))),
        call_with_headers(&app,"POST",&revoke_endpoint_path,&token_2,
            None,Some(&operator_key_2),None)
    );
    assert!(heartbeat_during_revoke.0==StatusCode::OK || heartbeat_during_revoke.0==StatusCode::UNAUTHORIZED,
        "heartbeat racing operator revocation either commits first or is rejected after revocation");
    assert_eq!(revoke_result.0,StatusCode::NO_CONTENT,"operator revocation is atomic with endpoint fencing");

    let (status, body) = call(
        &app,
        "GET",
        &format!("/ai-phone/{phone_2_id}/presence"),
        &token_2,
        None,
    ).await;
    assert_eq!(status, StatusCode::OK);
    let presence: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(presence["presence_state"], "REVOKED",
        "presence must expose an active attachment's terminal revoked state");

    let revoked_attach_body = serde_json::json!({
        "endpoint_id": current_endpoint_id,
        "endpoint_type": "TEST",
        "metadata": {"label": "AI-PHONE-02-transfer-target"}
    });
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/reconnect",&token_2,
        Some(&current_endpoint_credential),None,Some(revoked_attach_body.clone())).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint credential cannot reconnect");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,
        Some(&current_endpoint_credential),None,Some(revoked_attach_body)).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint credential cannot transfer or reactivate");

    let (status, _) = call_with_headers(&app,"GET",&phone_2_messages_path,&token_2,
        Some(&current_endpoint_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint credential cannot receive mailbox messages");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&current_endpoint_credential),None,Some(serde_json::json!({"endpoint_fence":current_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "REVOKED endpoint credential must not heartbeat despite current fence");

    let (status, _) = call_with_headers(&app,"POST",
        &format!("/ai-phone/{phone_2_id}/messages/{revoked_probe_id}/ack"),&token_2,
        Some(&current_endpoint_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "REVOKED endpoint credential must not ACK even a previously delivered message");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/detach",&token_2,
        Some(&current_endpoint_credential),None,Some(serde_json::json!({"endpoint_fence":current_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint cannot detach or downgrade its terminal state");
    let endpoint_status: String = sqlx::query_scalar(
        "SELECT endpoint_status FROM phone_endpoints WHERE endpoint_id=$1 AND ai_phone_id=$2 AND tenant_id=$3 AND ai_id=$4"
    ).bind(current_endpoint_id).bind(phone_2_id).bind(tenant_id).bind(ai_2_id)
        .fetch_one(&pool).await.unwrap();
    assert_eq!(endpoint_status, "REVOKED",
        "detach must not downgrade a terminal endpoint revocation to DETACHED");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":current_endpoint_id,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-transfer-target"}
        }))).await;
    assert_eq!(status, StatusCode::CONFLICT,
        "a revoked endpoint identifier cannot be re-registered");

    let revoked_endpoint = Uuid::new_v4();
    let (status, body) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":revoked_endpoint,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-revoked"}
        }))).await;
    assert_eq!(status,StatusCode::CREATED);
    let revoked_registration:Value=serde_json::from_slice(&body).unwrap();
    let revoked_endpoint_credential=revoked_registration["endpoint_credential"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE phone_endpoints SET endpoint_status='REVOKED' WHERE endpoint_id=$1 AND ai_phone_id=$2 AND tenant_id=$3 AND ai_id=$4")
        .bind(revoked_endpoint)
        .bind(phone_2_id)
        .bind(tenant_id)
        .bind(ai_2_id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/attach",&token_2,
        Some(&revoked_endpoint_credential),None,Some(serde_json::json!({
            "endpoint_id":revoked_endpoint,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-revoked"}
        }))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "terminally revoked endpoint credential cannot be attached again");

    let cross_tenant_id = Uuid::new_v4();
    let cross_tenant_room = Uuid::new_v4();
    let cross_tenant_principal = Uuid::new_v4();
    let cross_tenant_participant = Uuid::new_v4();
    let cross_tenant_ai = Uuid::new_v4();
    let cross_tenant_phone = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'AI-PHONE-02 foreign tenant')")
        .bind(cross_tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'AI-PHONE-02 foreign room')")
        .bind(cross_tenant_room)
        .bind(cross_tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','foreign-test',$3)",
    )
    .bind(cross_tenant_principal)
    .bind(cross_tenant_id)
    .bind(format!("foreign-{cross_tenant_principal}"))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,'foreign AI')",
    )
    .bind(cross_tenant_participant)
    .bind(cross_tenant_id)
    .bind(cross_tenant_principal)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','foreign AI')",
    )
    .bind(cross_tenant_ai)
    .bind(format!("foreign-ai:{cross_tenant_ai}"))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)",
    )
    .bind(cross_tenant_ai)
    .bind(cross_tenant_id)
    .bind(cross_tenant_principal)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
        .bind(cross_tenant_ai)
        .bind(cross_tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ai_phones(ai_phone_id,ai_id,tenant_id) VALUES($1,$2,$3)")
        .bind(cross_tenant_phone)
        .bind(cross_tenant_ai)
        .bind(cross_tenant_id)
        .execute(&pool)
        .await
        .unwrap();
    let cross_tenant_endpoint=Uuid::new_v4();
    let cross_tenant_endpoint_secret=format!("foreign-endpoint-secret-{}",Uuid::new_v4());
    sqlx::query("INSERT INTO phone_endpoints(endpoint_id,ai_phone_id,tenant_id,ai_id,endpoint_type,endpoint_status,metadata) VALUES($1,$2,$3,$4,'TEST','REGISTERED','{}'::jsonb)")
        .bind(cross_tenant_endpoint).bind(cross_tenant_phone).bind(cross_tenant_id).bind(cross_tenant_ai)
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO phone_endpoint_credentials(credential_id,endpoint_id,ai_phone_id,tenant_id,ai_id,credential_hash,credential_status,expires_at) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE',NOW()+INTERVAL '90 days')")
        .bind(Uuid::new_v4()).bind(cross_tenant_endpoint).bind(cross_tenant_phone).bind(cross_tenant_id)
        .bind(cross_tenant_ai).bind(Sha256::digest(cross_tenant_endpoint_secret.as_bytes()).to_vec())
        .execute(&pool).await.unwrap();
    // Attack D: a valid credential hash scoped to tenant B is rejected under tenant A's bearer.
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_1,
        Some(&cross_tenant_endpoint_secret),None,Some(serde_json::json!({"endpoint_fence":1}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "tenant A bearer cannot use tenant B endpoint credential");

    let (status, _) = call(
        &app,
        "GET",
        &format!("/ai-phone/{cross_tenant_phone}/messages"),
        &token_1,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "tenant A cannot read tenant B phone");
    let (status, _) = call(
        &app,
        "POST",
        &format!("/ai-phone/{cross_tenant_phone}/messages"),
        &token_1,
        Some(serde_json::json!({
            "idempotency_key": "phone-02-cross-tenant",
            "content": {"text": "must not cross tenant boundary"}
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "tenant A cannot send into tenant B phone");

    // Operator keys are independently revocable and cannot be recovered from the AI bearer.
    sqlx::query("UPDATE phone_endpoint_operator_keys SET key_status='REVOKED',revoked_at=NOW() WHERE tenant_id=$1 AND ai_id=$2 AND key_hash=$3")
        .bind(tenant_id).bind(ai_2_id).bind(Sha256::digest(operator_key_2.as_bytes()).to_vec())
        .execute(&pool).await.unwrap();
    let (status,_) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":Uuid::new_v4(),"endpoint_type":"TEST","metadata":{"label":"operator-key-revoked"}
        }))).await;
    assert_eq!(status,StatusCode::FORBIDDEN,
        "revoked operator key cannot enroll or rotate endpoint credentials");
}
