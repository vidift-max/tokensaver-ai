// Adversarial regression proof: the same global AI_ID is bound to two tenants.
// This test deliberately asserts Architecture B (one AI_PHONE_ID per tenant binding).
// Against the current PR #7 schema/handler it is expected to fail with a shared phone ID.
#[tokio::test]
async fn same_global_ai_identity_must_resolve_distinct_tenant_phones_over_http() {
    let url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to the disposable PostgreSQL 16 CI service");
    let pool = connect_pool(&url).await.expect("connect disposable PostgreSQL");
    migrate(&pool).await.expect("apply migrations only to disposable CI database");

    let ai_id = Uuid::new_v4();
    sqlx::query("INSERT INTO ai_identities(ai_id,identity_key,identity_kind,display_label) VALUES($1,$2,'TEST','tenant-isolation-adversarial')")
        .bind(ai_id).bind(format!("test:tenant-isolation:{ai_id}"))
        .execute(&pool).await.unwrap();

    async fn provision_tenant(
        pool: &PgPool,
        ai_id: Uuid,
        label: &str,
    ) -> (Uuid, Uuid, Uuid, String) {
        let tenant = Uuid::new_v4();
        let room = Uuid::new_v4();
        let principal = Uuid::new_v4();
        let participant = Uuid::new_v4();
        let issuer = format!("tenant-isolation-{label}");
        let subject = format!("synthetic-{label}-{principal}");

        sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,$2)")
            .bind(tenant).bind(format!("disposable-{label}"))
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,$3)")
            .bind(room).bind(tenant).bind(format!("disposable-room-{label}"))
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI',$3,$4)")
            .bind(principal).bind(tenant).bind(&issuer).bind(&subject)
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)")
            .bind(participant).bind(tenant).bind(principal).bind(format!("synthetic-{label}"))
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,$4,$5,'ACTIVE')")
            .bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(&issuer).bind(&subject)
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO ai_identity_principals(ai_id,tenant_id,principal_id) VALUES($1,$2,$3)")
            .bind(ai_id).bind(tenant).bind(principal)
            .execute(pool).await.unwrap();

        grant(pool, tenant, room, principal, "phone.read").await;
        let (session, _execution) = add_session(pool, tenant, room, principal, participant).await;
        sqlx::query("INSERT INTO room_memberships(membership_id,tenant_id,room_id,participant_id,principal_id,session_id,status) VALUES($1,$2,$3,$4,$5,$6,'ACTIVE')")
            .bind(Uuid::new_v4()).bind(tenant).bind(room).bind(participant).bind(principal).bind(session)
            .execute(pool).await.unwrap();
        let token = bearer_for(pool, session).await;
        (tenant, room, principal, token)
    }

    let (tenant_a, _room_a, _principal_a, token_a) =
        provision_tenant(&pool, ai_id, "A").await;
    let (tenant_b, _room_b, _principal_b, token_b) =
        provision_tenant(&pool, ai_id, "B").await;

    let mapping_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ai_identity_principals WHERE ai_id=$1 AND tenant_id IN ($2,$3)"
    )
    .bind(ai_id).bind(tenant_a).bind(tenant_b)
    .fetch_one(&pool).await.unwrap();
    assert_eq!(mapping_count, 2, "fixture must bind one identical AI_ID to both tenant principals");

    let app = ai_team_room_service::phone::router(pool.clone());
    let response_a = http_call(&app, "GET", "/ai-phone/me", Some(&token_a), None).await;
    let status_a = response_a.status();
    let body_a = axum::body::to_bytes(response_a.into_body(), 100_000).await.unwrap();
    eprintln!("ADVERSARIAL tenant=A ai_id={ai_id} http_status={status_a} response={}", String::from_utf8_lossy(&body_a));
    assert_eq!(status_a, axum::http::StatusCode::OK, "Tenant A must resolve its own phone");
    let json_a: Value = serde_json::from_slice(&body_a).unwrap();
    let phone_a = json_a["phone"]["ai_phone_id"].as_str().expect("Tenant A response must contain AI_PHONE_ID").to_string();

    let response_b = http_call(&app, "GET", "/ai-phone/me", Some(&token_b), None).await;
    let status_b = response_b.status();
    let body_b = axum::body::to_bytes(response_b.into_body(), 100_000).await.unwrap();
    eprintln!("ADVERSARIAL tenant=B ai_id={ai_id} http_status={status_b} response={}", String::from_utf8_lossy(&body_b));
    assert_eq!(status_b, axum::http::StatusCode::OK, "Tenant B must resolve its own phone");
    let json_b: Value = serde_json::from_slice(&body_b).unwrap();
    let phone_b = json_b["phone"]["ai_phone_id"].as_str().expect("Tenant B response must contain AI_PHONE_ID").to_string();

    let persisted: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT ai_phone_id, ai_id FROM ai_phones WHERE ai_id=$1 ORDER BY ai_phone_id"
    )
    .bind(ai_id).fetch_all(&pool).await.unwrap();
    eprintln!("ADVERSARIAL persisted_phone_rows={} tenant_a_phone={} tenant_b_phone={}", persisted.len(), phone_a, phone_b);

    // This is the security assertion. PR #7's global UNIQUE(ai_id) and ON CONFLICT(ai_id)
    // currently cause the two authenticated HTTP contexts to return the same phone.
    assert_ne!(
        phone_a, phone_b,
        "CROSS-TENANT ISOLATION FAILURE: distinct tenant principals sharing one AI_ID resolved the same AI_PHONE_ID"
    );
    assert_eq!(persisted.len(), 2, "Architecture B requires one persisted phone per tenant binding");
}
