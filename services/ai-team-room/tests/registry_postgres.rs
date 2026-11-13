use ai_team_room_service::{
    auth::AuthenticatedIdentity,
    connect_pool, migrate,
    phone::{attach, heartbeat, router as phone_router, AttachRequest},
};
use axum::{body::Body, http::Request, response::Response, Router};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn http_call(app: &Router, method: &str, uri: &str, token: Option<&str>) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    app.clone().oneshot(builder.body(Body::empty()).unwrap()).await.unwrap()
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

struct RegistryActor {
    identity: AuthenticatedIdentity,
    token: String,
    phone_id: Uuid,
    ai_id: Uuid,
}

#[tokio::test]
async fn registry_is_tenant_scoped_authorized_live_and_keyset_paginated() {
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be configured; refusing to skip registry integration test");
    let pool = connect_pool(&url).await.expect("connect PostgreSQL 16");
    migrate(&pool).await.expect("apply migrations");

    let tenant = Uuid::new_v4();
    let room = Uuid::new_v4();
    let foreign_tenant = Uuid::new_v4();
    let foreign_room = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'registry-tenant'),($2,'registry-foreign-tenant')")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'registry-room'),($3,$4,'registry-foreign-room')")
        .bind(room).bind(tenant).bind(foreign_room).bind(foreign_tenant).execute(&pool).await.unwrap();

    let mut actors = Vec::new();
    for index in 0..3 {
        let principal = Uuid::new_v4();
        let participant = Uuid::new_v4();
        let session = Uuid::new_v4();
        let execution = Uuid::new_v4();
        let ai_id = Uuid::new_v4();
        let phone_id = Uuid::new_v4();
        let token = format!("registry-session-{session}");
        let token_hash = Sha256::digest(token.as_bytes()).to_vec();

        sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','registry-test',$3)")
            .bind(principal).bind(tenant).bind(format!("registry-subject-{principal}"))
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)")
            .bind(participant).bind(tenant).bind(principal).bind(format!("Registry AI {index}"))
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,'registry-test',$4,'ACTIVE')")
            .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(format!("registry-subject-{principal}"))
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '20 minutes','ACTIVE',$5)")
            .bind(session).bind(tenant).bind(room).bind(principal).bind(token_hash)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,'registry-test-worker',NOW(),NOW()+INTERVAL '20 minutes','ACTIVE')")
            .bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
            .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(participant).bind(principal).bind(session)
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST',$3)")
            .bind(ai_id).bind(format!("registry:{ai_id}")).bind(format!("Registry AI {index}"))
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
            .bind(ai_id).bind(tenant).bind(principal).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
            .bind(ai_id).bind(tenant).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO ai_phones(ai_phone_id,ai_id,tenant_id) VALUES($1,$2,$3)")
            .bind(phone_id).bind(ai_id).bind(tenant).execute(&pool).await.unwrap();

        for permission in ["phone.attach", "phone.heartbeat", "phone.read"] {
            grant(&pool, tenant, room, principal, permission).await;
        }
        if index == 0 {
            grant_tenant_wide(&pool, tenant, principal, "phone.registry.read").await;
        } else if index == 1 {
            // A room-scoped permission must not authorize a tenant-wide registry.
            grant(&pool, tenant, room, principal, "phone.registry.read").await;
        }
        actors.push(RegistryActor {
            identity: AuthenticatedIdentity {
                tenant_id: tenant, participant_id: participant, principal_id: principal,
                session_id: session, execution_id: execution, room_id: room,
                issuer: "registry-test".into(), subject: format!("registry-subject-{principal}"),
            },
            token,
            phone_id,
            ai_id,
        });
    }

    // A real foreign-tenant phone exists. A caller cannot select it by supplying
    // tenant_id in the query string because the handler never accepts that field.
    let foreign_principal = Uuid::new_v4();
    let foreign_ai = Uuid::new_v4();
    let foreign_phone = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI','registry-test','foreign-registry-ai')")
        .bind(foreign_principal).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','Foreign registry AI')")
        .bind(foreign_ai).bind(format!("registry:{foreign_ai}")).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
        .bind(foreign_ai).bind(foreign_tenant).bind(foreign_principal).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_identity_tenants(ai_id,tenant_id) VALUES($1,$2)")
        .bind(foreign_ai).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ai_phones(ai_phone_id,ai_id,tenant_id) VALUES($1,$2,$3)")
        .bind(foreign_phone).bind(foreign_ai).bind(foreign_tenant).execute(&pool).await.unwrap();

    let mut tasks = tokio::task::JoinSet::new();
    for (index, actor) in actors.iter().enumerate() {
        let pool = pool.clone();
        let identity = actor.identity.clone();
        tasks.spawn(async move {
            attach(&pool, &identity, AttachRequest {
                endpoint_id: Uuid::new_v4(), endpoint_type: "TEST".into(),
                metadata: serde_json::json!({"fixture":"registry","index":index}),
            }, false).await.expect("attach fixture phone")
        });
    }
    let mut attachments = Vec::new();
    while let Some(result) = tasks.join_next().await {
        attachments.push(result.expect("attach task"));
    }
    assert_eq!(attachments.len(), 3);
    for actor in &actors {
        let attachment = attachments.iter()
            .find(|attachment| attachment.ai_phone_id == actor.phone_id)
            .expect("each fixture AI has its own attachment");
        heartbeat(&pool, &actor.identity, attachment.endpoint_fence).await
            .expect("heartbeat fixture phone");
    }

    let app = phone_router(pool.clone());
    let forbidden = http_call(&app, "GET", "/ai-phone/registry", Some(&actors[1].token)).await;
    assert_eq!(forbidden.status(), axum::http::StatusCode::FORBIDDEN,
        "registry requires explicit privileged permission");
    let unauthorized = http_call(&app, "GET", "/ai-phone/registry", Some("invalid-registry-token")).await;
    assert_eq!(unauthorized.status(), axum::http::StatusCode::UNAUTHORIZED);

    let first = http_call(&app, "GET",
        &format!("/ai-phone/registry?limit=2&tenant_id={foreign_tenant}"),
        Some(&actors[0].token)).await;
    assert_eq!(first.status(), axum::http::StatusCode::OK);
    let first: Value = serde_json::from_slice(&axum::body::to_bytes(first.into_body(), 100_000).await.unwrap()).unwrap();
    let first_items = first["items"].as_array().unwrap();
    assert_eq!(first_items.len(), 2);
    let cursor = first["next_cursor"].as_str().expect("next cursor for page one");
    let mut seen = first_items.iter().map(|item| {
        assert_eq!(item["tenant_id"], tenant.to_string());
        assert_ne!(item["ai_phone_id"], foreign_phone.to_string());
        Uuid::parse_str(item["ai_phone_id"].as_str().unwrap()).unwrap()
    }).collect::<Vec<_>>();

    let second = http_call(&app, "GET", &format!("/ai-phone/registry?limit=2&after={cursor}"),
        Some(&actors[0].token)).await;
    assert_eq!(second.status(), axum::http::StatusCode::OK);
    let second: Value = serde_json::from_slice(&axum::body::to_bytes(second.into_body(), 100_000).await.unwrap()).unwrap();
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next_cursor"].is_null());
    for item in second["items"].as_array().unwrap() {
        assert_eq!(item["tenant_id"], tenant.to_string());
        seen.push(Uuid::parse_str(item["ai_phone_id"].as_str().unwrap()).unwrap());
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 3, "keyset pagination returns every tenant phone exactly once");

    // Each listed phone exposes its current endpoint, session, execution and
    // heartbeat, without copying any identity or lifecycle state elsewhere.
    let all = http_call(&app, "GET", "/ai-phone/registry?limit=100", Some(&actors[0].token)).await;
    let all: Value = serde_json::from_slice(&axum::body::to_bytes(all.into_body(), 100_000).await.unwrap()).unwrap();
    for item in all["items"].as_array().unwrap() {
        let phone_id = Uuid::parse_str(item["ai_phone_id"].as_str().unwrap()).unwrap();
        let actor = actors.iter().find(|actor| actor.phone_id == phone_id).unwrap();
        assert_eq!(item["ai_id"], actor.ai_id.to_string());
        assert_eq!(item["tenant_id"], tenant.to_string());
        assert_eq!(item["phone_status"], "ATTACHED");
        assert!(item["active_endpoint_id"].is_string());
        assert_eq!(item["endpoint_status"], "ATTACHED");
        assert_eq!(item["active_session_id"], actor.identity.session_id.to_string());
        assert_eq!(item["active_execution_id"], actor.identity.execution_id.to_string());
        assert!(item["heartbeat_at"].is_string());
        assert!(item["last_activity_at"].is_string());
        assert_eq!(item["availability"], "AVAILABLE");
    }

    // Revoking a different phone's session changes only that phone's derived availability.
    sqlx::query("UPDATE sessions SET status='REVOKED',revoked_at=NOW() WHERE session_id=$1")
        .bind(actors[1].identity.session_id).execute(&pool).await.unwrap();
    let offline_page = http_call(&app, "GET", "/ai-phone/registry?limit=100", Some(&actors[0].token)).await;
    let offline_page: Value = serde_json::from_slice(
        &axum::body::to_bytes(offline_page.into_body(), 100_000).await.unwrap()).unwrap();
    let offline_item = offline_page["items"].as_array().unwrap().iter()
        .find(|item| item["ai_phone_id"] == actors[1].phone_id.to_string()).unwrap();
    assert_eq!(offline_item["availability"], "OFFLINE");

    // Revoking the querying session itself removes registry authorization.
    sqlx::query("UPDATE sessions SET status='REVOKED',revoked_at=NOW() WHERE session_id=$1")
        .bind(actors[0].identity.session_id).execute(&pool).await.unwrap();
    let offline = http_call(&app, "GET", "/ai-phone/registry?limit=100", Some(&actors[0].token)).await;
    assert_eq!(offline.status(), axum::http::StatusCode::UNAUTHORIZED,
        "revoking the querying session also revokes registry access");
    
    // Disposable cleanup; no permanent AI Phone is created.
    sqlx::query("DELETE FROM phone_messages WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_attachments WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM phone_endpoints WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_phones WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identity_tenants WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
    for actor in &actors {
        sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1").bind(actor.ai_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM ai_identities WHERE ai_id=$1").bind(actor.ai_id).execute(&pool).await.unwrap();
    }
    sqlx::query("DELETE FROM ai_identity_principals WHERE ai_id=$1").bind(foreign_ai).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM ai_identities WHERE ai_id=$1").bind(foreign_ai).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM tenants WHERE tenant_id IN ($1,$2)")
        .bind(tenant).bind(foreign_tenant).execute(&pool).await.unwrap();
}
