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
    sqlx::query("INSERT INTO phone_endpoint_operator_keys(operator_key_id,tenant_id,ai_id,key_hash) VALUES($1,$2,$3,$4),($5,$2,$6,$7)")
        .bind(Uuid::new_v4()).bind(tenant_id).bind(ai_1_id)
        .bind(Sha256::digest(operator_key_1.as_bytes()).to_vec())
        .bind(Uuid::new_v4()).bind(ai_2_id)
        .bind(Sha256::digest(operator_key_2.as_bytes()).to_vec())
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
        StatusCode::UNAUTHORIZED,
        "AI #1 endpoint credential cannot ACK a message delivered to Phone #2"
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
    let (status, _) = call(
        &app,
        "POST",
        "/ai-phone/heartbeat",
        &token_2,
        Some(serde_json::json!({"endpoint_fence": initial_fence})),
    )
    .await;
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
    let (status, body) = call_with_headers(&app,"POST","/ai-phone/reconnect",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({
            "endpoint_id":endpoint_2,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-endpoint"}
        }))).await;
    assert_eq!(status, StatusCode::OK, "Phone #2 reconnects using its registered endpoint");
    let reconnect: Value = serde_json::from_slice(&body).unwrap();
    let reconnect_fence = reconnect["endpoint_fence"].as_i64().unwrap();
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
    assert_eq!(
        status,
        StatusCode::PRECONDITION_FAILED,
        "transferred-away endpoint cannot heartbeat with its old fence"
    );

    // Attack A: a process retaining only the AI bearer cannot recover the active generation.
    let (status, _) = call(&app,"GET","/ai-phone/me",&token_2,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI bearer alone cannot read the active endpoint generation");
    let (status, _) = call(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "AI bearer plus a guessed/observed fence cannot heartbeat without endpoint credential");
    // Attack B: the transferred-away endpoint's own credential is terminally revoked.
    let (status, _) = call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&endpoint_2_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"old endpoint credential cannot recover the new generation");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"old endpoint credential cannot heartbeat as the new generation");
    // Attack C: endpoint A's credential is not accepted for endpoint B.
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_2_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,"credential A cannot authenticate endpoint B");
    let (status, body) = call_with_headers(&app,"GET","/ai-phone/me",&token_2,
        Some(&endpoint_3_credential),None,None).await;
    assert_eq!(status,StatusCode::OK,"only endpoint B's new credential can recover its own generation");
    let current_snapshot:Value=serde_json::from_slice(&body).unwrap();
    assert_eq!(current_snapshot["attachment"]["endpoint_id"],endpoint_3.to_string());
    assert_eq!(current_snapshot["attachment"]["endpoint_fence"],transfer_fence);

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
        Some(&endpoint_3_credential),None,None).await;
    assert_eq!(status,StatusCode::OK,"probe message is delivered before revocation");
    sqlx::query("UPDATE phone_endpoints SET endpoint_status='REVOKED' WHERE endpoint_id=$1 AND ai_phone_id=$2 AND tenant_id=$3 AND ai_id=$4")
        .bind(endpoint_3).bind(phone_2_id).bind(tenant_id).bind(ai_2_id)
        .execute(&pool).await.unwrap();

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
        "endpoint_id": endpoint_3,
        "endpoint_type": "TEST",
        "metadata": {"label": "AI-PHONE-02-transfer-target"}
    });
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/reconnect",&token_2,
        Some(&endpoint_3_credential),None,Some(revoked_attach_body.clone())).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint credential cannot reconnect");
    let (status, _) = call_with_headers(&app,"POST","/ai-phone/transfer",&token_2,
        Some(&endpoint_3_credential),None,Some(revoked_attach_body)).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint credential cannot transfer or reactivate");

    let (status, _) = call_with_headers(&app,"GET",&phone_2_messages_path,&token_2,
        Some(&endpoint_3_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint credential cannot receive mailbox messages");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/heartbeat",&token_2,
        Some(&endpoint_3_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "REVOKED endpoint credential must not heartbeat despite current fence");

    let (status, _) = call_with_headers(&app,"POST",
        &format!("/ai-phone/{phone_2_id}/messages/{revoked_probe_id}/ack"),&token_2,
        Some(&endpoint_3_credential),None,None).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "REVOKED endpoint credential must not ACK even a previously delivered message");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/detach",&token_2,
        Some(&endpoint_3_credential),None,Some(serde_json::json!({"endpoint_fence":transfer_fence}))).await;
    assert_eq!(status,StatusCode::UNAUTHORIZED,
        "revoked endpoint cannot detach or downgrade its terminal state");
    let endpoint_status: String = sqlx::query_scalar(
        "SELECT endpoint_status FROM phone_endpoints WHERE endpoint_id=$1 AND ai_phone_id=$2 AND tenant_id=$3 AND ai_id=$4"
    ).bind(endpoint_3).bind(phone_2_id).bind(tenant_id).bind(ai_2_id)
        .fetch_one(&pool).await.unwrap();
    assert_eq!(endpoint_status, "REVOKED",
        "detach must not downgrade a terminal endpoint revocation to DETACHED");

    let (status, _) = call_with_headers(&app,"POST","/ai-phone/endpoints/register",&token_2,
        None,Some(&operator_key_2),Some(serde_json::json!({
            "endpoint_id":endpoint_3,"endpoint_type":"TEST","metadata":{"label":"AI-PHONE-02-transfer-target"}
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
        "revoked endpoint credential cannot be reattached to Phone #2");

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
}
