use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, FromRequestParts, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use crate::tasks;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgListener, FromRow, PgPool, Row};
use uuid::Uuid;

use ai_team_room_service::auth::{authenticate_session, require_permission, AuthenticatedIdentity, AuthError};

#[derive(Debug, Serialize, FromRow)]
struct MessageRow {
    message_id: Uuid,
    room_id: Uuid,
    channel_id: Uuid,
    sender_principal_id: Uuid,
    sender_display_name: String,
    parent_message_id: Option<Uuid>,
    body: String,
    created_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct CreateMessage {
    client_message_id: Uuid,
    body: String,
    parent_message_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    before: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct HistoryResponse {
    channel_id: Uuid,
    items: Vec<MessageRow>,
    has_more: bool,
    unread_count: i64,
}

fn auth_status(error: AuthError) -> StatusCode {
    match error {
        AuthError::Unauthorized => StatusCode::UNAUTHORIZED,
        AuthError::IdentityUnavailable | AuthError::Forbidden => StatusCode::FORBIDDEN,
        AuthError::RoomUnavailable => StatusCode::NOT_FOUND,
        AuthError::ProviderUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        AuthError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn identity(pool: &PgPool, headers: &HeaderMap) -> Result<AuthenticatedIdentity, StatusCode> {
    authenticate_session(pool, headers).await.map_err(auth_status)
}

async fn require_room_member(pool: &PgPool, who: &AuthenticatedIdentity, room_id: Uuid) -> Result<(), StatusCode> {
    if who.room_id != room_id { return Err(StatusCode::FORBIDDEN); }
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM room_memberships WHERE room_id=$1 AND principal_id=$2 AND session_id=$3 AND status='ACTIVE')"
    ).bind(room_id).bind(who.principal_id).bind(who.session_id)
        .fetch_one(pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !active { return Err(StatusCode::FORBIDDEN); }
    Ok(())
}

async fn general_channel(pool: &PgPool, who: &AuthenticatedIdentity, room_id: Uuid) -> Result<Uuid, StatusCode> {
    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let channel_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO team_room_channels(channel_id,tenant_id,room_id,channel_type,created_by_principal_id) VALUES($1,$2,$3,'GENERAL',$4) ON CONFLICT (room_id) WHERE channel_type='GENERAL' DO NOTHING"
    ).bind(channel_id).bind(who.tenant_id).bind(room_id).bind(who.principal_id)
        .execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let id: Uuid = sqlx::query_scalar("SELECT channel_id FROM team_room_channels WHERE room_id=$1 AND channel_type='GENERAL'")
        .bind(room_id).fetch_one(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(id)
}

async fn private_channel(pool: &PgPool, who: &AuthenticatedIdentity, room_id: Uuid, other: Uuid) -> Result<Uuid, StatusCode> {
    if other == who.principal_id { return Err(StatusCode::BAD_REQUEST); }
    let recipient_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM room_memberships WHERE room_id=$1 AND principal_id=$2 AND status='ACTIVE')"
    ).bind(room_id).bind(other).fetch_one(pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !recipient_active { return Err(StatusCode::NOT_FOUND); }
    let both_authorized:bool=sqlx::query_scalar("SELECT COUNT(DISTINCT principal_id)=2 FROM principal_permissions WHERE tenant_id=$1 AND principal_id=ANY($2::uuid[]) AND permission='private_chat' AND revoked_at IS NULL AND (room_id IS NULL OR room_id=$3)")
        .bind(who.tenant_id).bind(vec![who.principal_id,other]).bind(room_id).fetch_one(pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if !both_authorized{return Err(StatusCode::FORBIDDEN);}
    let (a,b) = if who.principal_id < other {(who.principal_id,other)} else {(other,who.principal_id)};
    let mut tx = pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let lock_key = format!("{room_id}:{a}:{b}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))").bind(lock_key)
        .execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT c.channel_id FROM team_room_channels c JOIN team_room_channel_members ma ON ma.channel_id=c.channel_id AND ma.principal_id=$2 JOIN team_room_channel_members mb ON mb.channel_id=c.channel_id AND mb.principal_id=$3 WHERE c.room_id=$1 AND c.channel_type='PRIVATE' LIMIT 1"
    ).bind(room_id).bind(a).bind(b).fetch_optional(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let channel_id = if let Some(id)=existing { id } else {
        let id=Uuid::new_v4();
        sqlx::query("INSERT INTO team_room_channels(channel_id,tenant_id,room_id,channel_type,created_by_principal_id) VALUES($1,$2,$3,'PRIVATE',$4)")
            .bind(id).bind(who.tenant_id).bind(room_id).bind(who.principal_id)
            .execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for principal in [a,b] {
            sqlx::query("INSERT INTO team_room_channel_members(channel_id,principal_id,added_by_principal_id) VALUES($1,$2,$3)")
                .bind(id).bind(principal).bind(who.principal_id)
                .execute(&mut *tx).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        id
    };
    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(channel_id)
}

async fn history(
    State(pool): State<PgPool>, Path(room_id): Path<Uuid>, Query(query): Query<HistoryQuery>, headers: HeaderMap,
) -> Result<Json<HistoryResponse>, StatusCode> {
    let who=identity(&pool,&headers).await?;
    require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"room.read").await.map_err(auth_status)?;
    let channel=general_channel(&pool,&who,room_id).await?;
    let limit=query.limit.unwrap_or(50).clamp(1,100);
    let rows=sqlx::query_as::<_,MessageRow>(
        "SELECT m.message_id,m.room_id,m.channel_id,m.sender_principal_id,p.display_name AS sender_display_name,m.parent_message_id,m.body,m.created_at,m.edited_at,m.deleted_at FROM team_room_messages m JOIN participants p ON p.principal_id=m.sender_principal_id AND p.tenant_id=m.tenant_id WHERE m.channel_id=$1 AND m.deleted_at IS NULL AND ($2::timestamptz IS NULL OR m.created_at<$2) ORDER BY m.created_at DESC,m.message_id DESC LIMIT $3"
    ).bind(channel).bind(query.before).bind(limit+1).fetch_all(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let has_more=rows.len() as i64>limit;
    let mut items:Vec<_>=rows.into_iter().take(limit as usize).collect();
    items.reverse();
    let unread_count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM team_room_messages m LEFT JOIN team_room_message_reads r ON r.channel_id=m.channel_id AND r.principal_id=$2 WHERE m.channel_id=$1 AND m.sender_principal_id<>$2 AND m.deleted_at IS NULL AND m.created_at>COALESCE(r.last_read_at,'-infinity'::timestamptz)")
        .bind(channel).bind(who.principal_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO team_room_message_reads(channel_id,principal_id,last_read_at) VALUES($1,$2,NOW()) ON CONFLICT(channel_id,principal_id) DO UPDATE SET last_read_at=EXCLUDED.last_read_at")
        .bind(channel).bind(who.principal_id).execute(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(HistoryResponse{channel_id:channel,items,has_more,unread_count}))
}

async fn send_general(
    State(pool): State<PgPool>, Path(room_id): Path<Uuid>, headers: HeaderMap, Json(input): Json<CreateMessage>,
) -> Result<(StatusCode,Json<MessageRow>),StatusCode> {
    let who=identity(&pool,&headers).await?;
    require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"room.write").await.map_err(auth_status)?;
    let channel=general_channel(&pool,&who,room_id).await?;
    persist_message(&pool,&who,room_id,channel,"GENERAL",input).await
}

async fn private_history(
    State(pool): State<PgPool>, Path((room_id,other)): Path<(Uuid,Uuid)>, Query(query): Query<HistoryQuery>, headers: HeaderMap,
) -> Result<Json<HistoryResponse>,StatusCode> {
    let who=identity(&pool,&headers).await?;
    require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"private_chat").await.map_err(auth_status)?;
    let channel=private_channel(&pool,&who,room_id,other).await?;
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_channel_members WHERE channel_id=$1 AND principal_id=$2)")
        .bind(channel).bind(who.principal_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if !allowed{return Err(StatusCode::FORBIDDEN);}
    let limit=query.limit.unwrap_or(50).clamp(1,100);
    let rows=sqlx::query_as::<_,MessageRow>(
        "SELECT m.message_id,m.room_id,m.channel_id,m.sender_principal_id,p.display_name AS sender_display_name,m.parent_message_id,m.body,m.created_at,m.edited_at,m.deleted_at FROM team_room_messages m JOIN participants p ON p.principal_id=m.sender_principal_id AND p.tenant_id=m.tenant_id WHERE m.channel_id=$1 AND m.deleted_at IS NULL AND ($2::timestamptz IS NULL OR m.created_at<$2) ORDER BY m.created_at DESC,m.message_id DESC LIMIT $3"
    ).bind(channel).bind(query.before).bind(limit+1).fetch_all(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let has_more=rows.len() as i64>limit;
    let mut items:Vec<_>=rows.into_iter().take(limit as usize).collect(); items.reverse();
    let unread_count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM team_room_messages m LEFT JOIN team_room_message_reads r ON r.channel_id=m.channel_id AND r.principal_id=$2 WHERE m.channel_id=$1 AND m.sender_principal_id<>$2 AND m.deleted_at IS NULL AND m.created_at>COALESCE(r.last_read_at,'-infinity'::timestamptz)")
        .bind(channel).bind(who.principal_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    sqlx::query("INSERT INTO team_room_message_reads(channel_id,principal_id,last_read_at) VALUES($1,$2,NOW()) ON CONFLICT(channel_id,principal_id) DO UPDATE SET last_read_at=EXCLUDED.last_read_at")
        .bind(channel).bind(who.principal_id).execute(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(HistoryResponse{channel_id:channel,items,has_more,unread_count}))
}

async fn send_private(
    State(pool): State<PgPool>, Path((room_id,other)): Path<(Uuid,Uuid)>, headers: HeaderMap, Json(input): Json<CreateMessage>,
) -> Result<(StatusCode,Json<MessageRow>),StatusCode> {
    let who=identity(&pool,&headers).await?;
    require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"private_chat").await.map_err(auth_status)?;
    let channel=private_channel(&pool,&who,room_id,other).await?;
    persist_message(&pool,&who,room_id,channel,"PRIVATE",input).await
}

async fn persist_message(pool:&PgPool,who:&AuthenticatedIdentity,room_id:Uuid,channel:Uuid,channel_type:&str,input:CreateMessage)->Result<(StatusCode,Json<MessageRow>),StatusCode>{
    if input.body.trim().is_empty()||input.body.len()>12_000{return Err(StatusCode::BAD_REQUEST);}
    let mut tx=pool.begin().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let allowed:bool=if channel_type=="GENERAL"{
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_memberships WHERE room_id=$1 AND principal_id=$2 AND session_id=$3 AND status='ACTIVE')")
            .bind(room_id).bind(who.principal_id).bind(who.session_id).fetch_one(&mut *tx).await
    }else{
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_channel_members WHERE channel_id=$1 AND principal_id=$2)")
            .bind(channel).bind(who.principal_id).fetch_one(&mut *tx).await
    }.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if !allowed{return Err(StatusCode::FORBIDDEN);}
    if let Some(parent)=input.parent_message_id{
        let same:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_messages WHERE message_id=$1 AND channel_id=$2 AND deleted_at IS NULL)")
            .bind(parent).bind(channel).fetch_one(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        if !same{return Err(StatusCode::BAD_REQUEST);}
    }
    let message_id=Uuid::new_v4();
    let inserted:Option<Uuid>=sqlx::query_scalar(
        "INSERT INTO team_room_messages(message_id,tenant_id,room_id,channel_id,sender_principal_id,client_message_id,parent_message_id,body) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(channel_id,sender_principal_id,client_message_id) DO NOTHING RETURNING message_id"
    ).bind(message_id).bind(who.tenant_id).bind(room_id).bind(channel).bind(who.principal_id).bind(input.client_message_id).bind(input.parent_message_id).bind(input.body.trim())
        .fetch_optional(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let (canonical_id,created)=if let Some(id)=inserted{(id,true)}else{
        let id:Uuid=sqlx::query_scalar("SELECT message_id FROM team_room_messages WHERE channel_id=$1 AND sender_principal_id=$2 AND client_message_id=$3")
            .bind(channel).bind(who.principal_id).bind(input.client_message_id).fetch_one(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        let existing=sqlx::query("SELECT body,parent_message_id FROM team_room_messages WHERE message_id=$1").bind(id).fetch_one(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        let existing_body:String=existing.try_get("body").map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        let existing_parent:Option<Uuid>=existing.try_get("parent_message_id").map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        if existing_body!=input.body.trim()||existing_parent!=input.parent_message_id{return Err(StatusCode::CONFLICT);}
        (id,false)
    };
    let row=sqlx::query_as::<_,MessageRow>("SELECT m.message_id,m.room_id,m.channel_id,m.sender_principal_id,p.display_name AS sender_display_name,m.parent_message_id,m.body,m.created_at,m.edited_at,m.deleted_at FROM team_room_messages m JOIN participants p ON p.principal_id=m.sender_principal_id AND p.tenant_id=m.tenant_id WHERE m.message_id=$1")
        .bind(canonical_id).fetch_one(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if created{
        let payload=serde_json::json!({"room_id":room_id,"channel_id":channel,"channel_type":channel_type,"message":row});
        sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'team_room_channel',$3,'MESSAGE_CREATED',$4)")
            .bind(Uuid::new_v4()).bind(who.tenant_id).bind(channel).bind(payload)
            .execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    tx.commit().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED,Json(row)))
}

#[derive(Serialize)]
struct WsTicket { ticket:String, expires_at:DateTime<Utc> }

async fn issue_ws_ticket(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap)->Result<Json<WsTicket>,StatusCode>{
    let who=identity(&pool,&headers).await?;
    require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"room.read").await.map_err(auth_status)?;
    let mut raw=[0u8;32]; rand::rngs::OsRng.fill_bytes(&mut raw);
    let ticket=URL_SAFE_NO_PAD.encode(raw);
    let digest=Sha256::digest(ticket.as_bytes()).to_vec();
    let expires=Utc::now()+chrono::Duration::seconds(30);
    sqlx::query("INSERT INTO team_room_ws_tickets(ticket_hash,tenant_id,room_id,principal_id,session_id,expires_at) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(digest).bind(who.tenant_id).bind(room_id).bind(who.principal_id).bind(who.session_id).bind(expires)
        .execute(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(WsTicket{ticket,expires_at:expires}))
}

#[derive(Deserialize)]
struct TicketQuery { ticket:String }

#[derive(FromRow)]
struct WsAuthRow { tenant_id:Uuid,participant_id:Uuid,principal_id:Uuid,session_id:Uuid,execution_id:Uuid,room_id:Uuid,issuer:String,subject:String }

async fn websocket(
    State(pool):State<PgPool>,Path(room_id):Path<Uuid>,Query(query):Query<TicketQuery>,
    request:axum::extract::Request,
)->Result<axum::response::Response,StatusCode>{
    if query.ticket.len()>128{return Err(StatusCode::UNAUTHORIZED);}
    let digest=Sha256::digest(query.ticket.as_bytes()).to_vec();
    let ticket=sqlx::query("UPDATE team_room_ws_tickets SET consumed_at=NOW() WHERE ticket_hash=$1 AND room_id=$2 AND consumed_at IS NULL AND expires_at>NOW() RETURNING tenant_id,principal_id,session_id")
        .bind(digest).bind(room_id).fetch_optional(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::UNAUTHORIZED)?;
    let tenant:Uuid=ticket.try_get("tenant_id").map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let principal:Uuid=ticket.try_get("principal_id").map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let session:Uuid=ticket.try_get("session_id").map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let who=sqlx::query_as::<_,WsAuthRow>(
        "SELECT s.tenant_id,p.participant_id,s.principal_id,s.session_id,w.execution_id,s.room_id,pr.provider AS issuer,pr.external_subject AS subject FROM sessions s JOIN principals pr ON pr.principal_id=s.principal_id JOIN participants p ON p.principal_id=s.principal_id AND p.tenant_id=s.tenant_id JOIN workload_executions w ON w.session_id=s.session_id WHERE s.tenant_id=$1 AND s.room_id=$2 AND s.principal_id=$3 AND s.session_id=$4 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND p.enabled AND pr.enabled AND EXISTS(SELECT 1 FROM room_memberships m WHERE m.room_id=s.room_id AND m.principal_id=s.principal_id AND m.session_id=s.session_id AND m.status='ACTIVE')"
    ).bind(tenant).bind(room_id).bind(principal).bind(session).fetch_optional(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::UNAUTHORIZED)?;
    let identity=AuthenticatedIdentity{tenant_id:who.tenant_id,participant_id:who.participant_id,principal_id:who.principal_id,session_id:who.session_id,execution_id:who.execution_id,room_id:who.room_id,issuer:who.issuer,subject:who.subject};
    let can_read=require_permission(&pool,&identity,"room.read").await.is_ok();
    if !can_read{return Err(StatusCode::FORBIDDEN);}
    // Authenticate and authorize the single-use ticket before parsing the
    // WebSocket upgrade. Invalid tickets therefore receive 401, not an
    // extractor-level 426 response.
    let (mut parts,_body)=request.into_parts();
    let ws=WebSocketUpgrade::from_request_parts(&mut parts,&pool).await.map_err(|_|StatusCode::UPGRADE_REQUIRED)?;
    Ok(ws.protocols(["team-room.v1"]).on_upgrade(move |socket| websocket_loop(socket,pool,room_id,principal,identity.session_id)))
}

async fn websocket_loop(mut socket:WebSocket,pool:PgPool,room_id:Uuid,principal:Uuid,session:Uuid){
    let Ok(mut listener)=PgListener::connect_with(&pool).await else{return;};
    if listener.listen("team_room_outbox").await.is_err(){return;}
    let mut revalidate=tokio::time::interval(std::time::Duration::from_secs(15));
    loop{
        tokio::select!{
            _=revalidate.tick()=>{
                let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN workload_executions w ON w.session_id=s.session_id JOIN room_memberships m ON m.session_id=s.session_id AND m.room_id=s.room_id AND m.principal_id=s.principal_id WHERE s.session_id=$1 AND s.room_id=$2 AND s.principal_id=$3 AND s.status='ACTIVE' AND s.revoked_at IS NULL AND s.expires_at>NOW() AND w.status='ACTIVE' AND w.revoked_at IS NULL AND w.expires_at>NOW() AND m.status='ACTIVE' AND EXISTS(SELECT 1 FROM principal_permissions pp WHERE pp.tenant_id=s.tenant_id AND pp.principal_id=s.principal_id AND pp.permission='room.read' AND pp.revoked_at IS NULL AND (pp.room_id IS NULL OR pp.room_id=s.room_id)))")
                    .bind(session).bind(room_id).bind(principal).fetch_one(&pool).await.unwrap_or(false);
                if !active{let _=socket.send(Message::Close(None)).await;break;}
            }
            incoming=socket.recv()=>{
                match incoming{
                    Some(Ok(Message::Close(_)))|None=>break,
                    Some(Err(_))=>break,
                    _=>{}
                }
            }
            notification=listener.recv()=>{
                let Ok(notification)=notification else{break;};
                let Ok(event_id)=Uuid::parse_str(notification.payload()) else{continue;};
                let event=sqlx::query("SELECT event_type,payload,aggregate_id FROM outbox_events WHERE outbox_event_id=$1")
                    .bind(event_id).fetch_optional(&pool).await;
                let Ok(Some(event))=event else{continue;};
                let payload:serde_json::Value=event.try_get("payload").unwrap_or_default();
                if payload.get("room_id").and_then(|v|v.as_str())!=Some(room_id.to_string().as_str()){continue;}
                let channel_id:Option<Uuid>=event.try_get("aggregate_id").ok();
                if let Some(channel)=channel_id{
                    let channel_type:Option<String>=sqlx::query_scalar("SELECT channel_type FROM team_room_channels WHERE channel_id=$1")
                        .bind(channel).fetch_optional(&pool).await.unwrap_or(None);
                    if channel_type.as_deref()==Some("PRIVATE"){
                        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_channel_members cm JOIN team_room_channels ch ON ch.channel_id=cm.channel_id JOIN principal_permissions pp ON pp.tenant_id=ch.tenant_id AND pp.principal_id=cm.principal_id AND pp.permission='private_chat' AND pp.revoked_at IS NULL AND (pp.room_id IS NULL OR pp.room_id=ch.room_id) WHERE cm.channel_id=$1 AND cm.principal_id=$2)")
                            .bind(channel).bind(principal).fetch_one(&pool).await.unwrap_or(false);
                        if !allowed{continue;}
                    }
                }
                let raw_type:String=event.try_get("event_type").unwrap_or_default();
                let event_type=match raw_type.as_str(){"participant.admitted"|"participant.reconnected"=>"PARTICIPANT_JOINED","participant.left"=>"PARTICIPANT_LEFT",other=>other};
                let data=serde_json::json!({"event_id":event_id,"type":event_type,"payload":payload});
                if socket.send(Message::Text(data.to_string().into())).await.is_err(){break;}
            }
        }
    }
}

#[derive(Serialize,FromRow)]
struct ParticipantItem { participant_id:Uuid,principal_id:Uuid,display_name:String,provider:String,last_seen_at:Option<DateTime<Utc>>,execution_heartbeat:Option<DateTime<Utc>>,registered:bool }

async fn participants(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap)->Result<Json<Vec<ParticipantItem>>,StatusCode>{
    let who=identity(&pool,&headers).await?;
    require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"room.read").await.map_err(auth_status)?;
    let rows=sqlx::query_as::<_,ParticipantItem>(
        "SELECT p.participant_id,p.principal_id,p.display_name,pr.provider,m.last_seen_at,w.heartbeat_at AS execution_heartbeat,(s.status='ACTIVE' AND s.expires_at>NOW() AND s.revoked_at IS NULL) AS registered FROM room_memberships m JOIN participants p ON p.participant_id=m.participant_id JOIN principals pr ON pr.principal_id=m.principal_id JOIN sessions s ON s.session_id=m.session_id LEFT JOIN workload_executions w ON w.session_id=s.session_id AND w.status='ACTIVE' AND w.revoked_at IS NULL WHERE m.room_id=$1 AND m.status='ACTIVE' ORDER BY p.display_name"
    ).bind(room_id).fetch_all(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct DecisionInput { mission_id:Option<Uuid>,title:String,decision:String,participants:Vec<Uuid>,reference_message_id:Option<Uuid> }
#[derive(Serialize,FromRow)]
struct DecisionRow { decision_id:Uuid,author_principal_id:Uuid,mission_id:Option<Uuid>,title:String,decision:String,participants:Vec<Uuid>,reference_message_id:Option<Uuid>,created_at:DateTime<Utc> }

async fn create_decision(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap,Json(input):Json<DecisionInput>)->Result<(StatusCode,Json<DecisionRow>),StatusCode>{
    let who=identity(&pool,&headers).await?; require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"decision.propose").await.map_err(auth_status)?;
    if input.title.trim().is_empty()||input.title.len()>300||input.decision.trim().is_empty()||input.decision.len()>12000||input.participants.len()>15{return Err(StatusCode::BAD_REQUEST);}
    let valid:bool=sqlx::query_scalar("SELECT cardinality($2::uuid[])=0 OR COALESCE(bool_and(EXISTS(SELECT 1 FROM room_memberships m WHERE m.room_id=$1 AND m.principal_id=x AND m.status='ACTIVE')),FALSE) FROM unnest($2::uuid[]) x")
        .bind(room_id).bind(&input.participants).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if !valid{return Err(StatusCode::BAD_REQUEST);}
    if let Some(reference)=input.reference_message_id{
        let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_messages m JOIN team_room_channels c ON c.channel_id=m.channel_id WHERE m.message_id=$1 AND m.room_id=$2 AND (c.channel_type='GENERAL' OR EXISTS(SELECT 1 FROM team_room_channel_members cm WHERE cm.channel_id=c.channel_id AND cm.principal_id=$3)))")
            .bind(reference).bind(room_id).bind(who.principal_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        if !visible{return Err(StatusCode::FORBIDDEN);}
    }
    let mut tx=pool.begin().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let row=sqlx::query_as::<_,DecisionRow>("INSERT INTO team_room_decisions(decision_id,tenant_id,room_id,author_principal_id,mission_id,title,decision,participants,reference_message_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING decision_id,author_principal_id,mission_id,title,decision,participants,reference_message_id,created_at")
        .bind(Uuid::new_v4()).bind(who.tenant_id).bind(room_id).bind(who.principal_id).bind(input.mission_id).bind(input.title.trim()).bind(input.decision.trim()).bind(input.participants).bind(input.reference_message_id)
        .fetch_one(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload=serde_json::json!({"room_id":room_id,"decision":row});
    sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'room',$3,'DECISION_CREATED',$4)")
        .bind(Uuid::new_v4()).bind(who.tenant_id).bind(room_id).bind(payload).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED,Json(row)))
}

#[derive(Deserialize)]
struct AttachmentInput { channel_id:Uuid,message_id:Option<Uuid>,task_id:Option<Uuid>,file_name:String,storage_ref:String,content_type:Option<String>,byte_len:Option<i64>,sha256:Option<String> }
#[derive(Serialize,FromRow)]
struct AttachmentRow { attachment_id:Uuid,channel_id:Uuid,message_id:Option<Uuid>,task_id:Option<Uuid>,file_name:String,storage_ref:String,content_type:Option<String>,byte_len:Option<i64>,sha256:Option<String>,created_at:DateTime<Utc> }

async fn create_attachment_ref(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap,Json(input):Json<AttachmentInput>)->Result<(StatusCode,Json<AttachmentRow>),StatusCode>{
    let who=identity(&pool,&headers).await?; require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"attachment.upload").await.map_err(auth_status)?;
    if input.file_name.trim().is_empty()||input.file_name.len()>255||input.storage_ref.trim().is_empty()||input.storage_ref.len()>1024{return Err(StatusCode::BAD_REQUEST);}
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_channels c WHERE c.channel_id=$1 AND c.room_id=$2 AND (c.channel_type='GENERAL' OR EXISTS(SELECT 1 FROM team_room_channel_members m WHERE m.channel_id=c.channel_id AND m.principal_id=$3)))")
        .bind(input.channel_id).bind(room_id).bind(who.principal_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if !allowed{return Err(StatusCode::FORBIDDEN);}
    if let Some(message)=input.message_id{
        let same:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_room_messages WHERE message_id=$1 AND channel_id=$2 AND room_id=$3)")
            .bind(message).bind(input.channel_id).bind(room_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        if !same{return Err(StatusCode::BAD_REQUEST);}
    }
    if let Some(task_id)=input.task_id{
        let task_visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM team_tasks WHERE task_id=$1 AND tenant_id=$2 AND room_id=$3)")
            .bind(task_id).bind(who.tenant_id).bind(room_id).fetch_one(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        if !task_visible{return Err(StatusCode::NOT_FOUND);}
    }
    let mut tx=pool.begin().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let row=sqlx::query_as::<_,AttachmentRow>("INSERT INTO team_room_attachment_refs(attachment_id,tenant_id,room_id,channel_id,uploaded_by_principal_id,message_id,task_id,file_name,storage_ref,content_type,byte_len,sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING attachment_id,channel_id,message_id,task_id,file_name,storage_ref,content_type,byte_len,sha256,created_at")
        .bind(Uuid::new_v4()).bind(who.tenant_id).bind(room_id).bind(input.channel_id).bind(who.principal_id).bind(input.message_id).bind(input.task_id).bind(input.file_name.trim()).bind(input.storage_ref.trim()).bind(input.content_type).bind(input.byte_len).bind(input.sha256)
        .fetch_one(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload=serde_json::json!({"room_id":room_id,"attachment":row});
    sqlx::query("INSERT INTO outbox_events(outbox_event_id,tenant_id,aggregate_type,aggregate_id,event_type,payload) VALUES($1,$2,'team_room_channel',$3,'ATTACHMENT_CREATED',$4)")
        .bind(Uuid::new_v4()).bind(who.tenant_id).bind(input.channel_id).bind(payload).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit().await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED,Json(row)))
}


async fn list_tasks(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap)->Result<Json<Vec<tasks::TaskLease>>,StatusCode>{
    let who=identity(&pool,&headers).await?; require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"task.read").await.map_err(auth_status)?;
    let rows=sqlx::query_as::<_,tasks::TaskLease>("SELECT task_id,mission_id,status,owner_principal_id,owner_participant_id,owner_session_id,owner_execution_id,fencing_token,lease_expires_at,checkpoint,checkpoint_version FROM team_tasks WHERE tenant_id=$1 AND room_id=$2 ORDER BY updated_at DESC LIMIT 100")
        .bind(who.tenant_id).bind(room_id).fetch_all(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}
async fn list_decisions(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap)->Result<Json<Vec<DecisionRow>>,StatusCode>{
    let who=identity(&pool,&headers).await?; require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"room.read").await.map_err(auth_status)?;
    let rows=sqlx::query_as::<_,DecisionRow>("SELECT decision_id,author_principal_id,mission_id,title,decision,participants,reference_message_id,created_at FROM team_room_decisions WHERE tenant_id=$1 AND room_id=$2 ORDER BY created_at DESC LIMIT 100")
        .bind(who.tenant_id).bind(room_id).fetch_all(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}
async fn list_attachments(State(pool):State<PgPool>,Path(room_id):Path<Uuid>,headers:HeaderMap)->Result<Json<Vec<AttachmentRow>>,StatusCode>{
    let who=identity(&pool,&headers).await?; require_room_member(&pool,&who,room_id).await?;
    require_permission(&pool,&who,"attachment.read").await.map_err(auth_status)?;
    let rows=sqlx::query_as::<_,AttachmentRow>("SELECT a.attachment_id,a.channel_id,a.message_id,a.task_id,a.file_name,a.storage_ref,a.content_type,a.byte_len,a.sha256,a.created_at FROM team_room_attachment_refs a JOIN team_room_channels c ON c.channel_id=a.channel_id WHERE a.tenant_id=$1 AND a.room_id=$2 AND (c.channel_type='GENERAL' OR EXISTS(SELECT 1 FROM team_room_channel_members m WHERE m.channel_id=c.channel_id AND m.principal_id=$3)) ORDER BY a.created_at DESC LIMIT 100")
        .bind(who.tenant_id).bind(room_id).bind(who.principal_id).fetch_all(&pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}

pub fn router(pool:PgPool)->Router{
    Router::new()
        .route("/team-room/{room_id}/messages",get(history).post(send_general))
        .route("/team-room/{room_id}/private/{other}/messages",get(private_history).post(send_private))
        .route("/team-room/{room_id}/participants",get(participants))
        .route("/team-room/{room_id}/tasks",get(list_tasks))
        .route("/team-room/{room_id}/decisions",get(list_decisions).post(create_decision))
        .route("/team-room/{room_id}/attachments",get(list_attachments).post(create_attachment_ref))
        .route("/team-room/{room_id}/ws-ticket",post(issue_ws_ticket))
        .route("/team-room/{room_id}/ws",get(websocket))
        
        .layer(axum::extract::DefaultBodyLimit::max(20_000))
        .with_state(pool)
}

#[cfg(test)]
mod postgres_tests {
    use super::*;
    use axum::body::Body;
    use ai_team_room_service::{admit_authenticated_participant,migrate,connect_pool};
    use sha2::{Digest,Sha256};
    use serde_json::json;
    use tower::ServiceExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    struct Actor { principal:Uuid, participant:Uuid, session:Uuid, token:String }

    async fn actor(pool:&PgPool,tenant:Uuid,room:Uuid,name:&str,permissions:&[&str])->Actor{
        let principal=Uuid::new_v4();let participant=Uuid::new_v4();let session=Uuid::new_v4();let execution=Uuid::new_v4();
        let issuer="https://communication-test.example";let subject=format!("{name}-{}",Uuid::new_v4());let token=format!("communication-session-{name}-{}",Uuid::new_v4());
        let digest=Sha256::digest(token.as_bytes()).to_vec();
        sqlx::query("INSERT INTO principals(principal_id,tenant_id,principal_kind,provider,external_subject) VALUES($1,$2,'AI',$3,$4)").bind(principal).bind(tenant).bind(issuer).bind(&subject).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO participants(participant_id,tenant_id,principal_id,display_name) VALUES($1,$2,$3,$4)").bind(participant).bind(tenant).bind(principal).bind(name).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workload_identities(workload_id,tenant_id,principal_id,issuer,subject,status) VALUES($1,$2,$3,$4,$5,'ACTIVE')").bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(issuer).bind(&subject).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO sessions(session_id,tenant_id,room_id,principal_id,issued_at,expires_at,status,token_hash) VALUES($1,$2,$3,$4,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE',$5)").bind(session).bind(tenant).bind(room).bind(principal).bind(digest).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO workload_executions(execution_id,tenant_id,room_id,participant_id,principal_id,session_id,worker_subject,heartbeat_at,expires_at,status) VALUES($1,$2,$3,$4,$5,$6,$7,NOW(),NOW()+INTERVAL '10 minutes','ACTIVE')").bind(execution).bind(tenant).bind(room).bind(participant).bind(principal).bind(session).bind(format!("{issuer}|{subject}")).execute(pool).await.unwrap();
        for permission in permissions{
            sqlx::query("INSERT INTO principal_permissions(permission_id,tenant_id,principal_id,room_id,permission) VALUES($1,$2,$3,$4,$5)").bind(Uuid::new_v4()).bind(tenant).bind(principal).bind(room).bind(*permission).execute(pool).await.unwrap();
        }
        admit_authenticated_participant(pool,room,participant,principal,session).await.unwrap();
        Actor{principal,participant,session,token}
    }

    async fn request(app:Router,method:&str,path:String,token:&str,body:Option<serde_json::Value>)->axum::response::Response{
        let mut builder=axum::http::Request::builder().method(method).uri(path).header("authorization",format!("Bearer {token}"));
        if body.is_some(){builder=builder.header("content-type","application/json");}
        app.oneshot(builder.body(body.map(|v|Body::from(v.to_string())).unwrap_or_else(Body::empty)).unwrap()).await.unwrap()
    }

    #[tokio::test]
    async fn postgres_general_private_acl_and_message_idempotency(){
        let database=std::env::var("DATABASE_URL").expect("PostgreSQL integration database required");
        let pool=connect_pool(&database).await.unwrap();migrate(&pool).await.unwrap();
        let tenant=Uuid::new_v4();let room=Uuid::new_v4();
        sqlx::query("INSERT INTO tenants(tenant_id,name) VALUES($1,'communication-integration')").bind(tenant).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO rooms(room_id,tenant_id,name) VALUES($1,$2,'communication-room')").bind(room).bind(tenant).execute(&pool).await.unwrap();
        let grants=["room.join","room.read","room.write","private_chat","presence.update","task.read","task.write","decision.propose","attachment.upload","attachment.read"];
        let a=actor(&pool,tenant,room,"Agent A",&grants).await;
        let b=actor(&pool,tenant,room,"Agent B",&grants).await;
        let c_grants=["room.join","room.read","room.write","presence.update","task.read","task.write","decision.propose","attachment.upload","attachment.read"];
        let c=actor(&pool,tenant,room,"Agent C",&c_grants).await;
        let app=router(pool.clone());
        let client_id=Uuid::new_v4();
        let message=json!({"client_message_id":client_id,"body":"general hello","parent_message_id":null});
        let sent=request(app.clone(),"POST",format!("/team-room/{room}/messages"),&a.token,Some(message.clone())).await;
        assert_eq!(sent.status(),StatusCode::CREATED);
        let first:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(sent.into_body(),20000).await.unwrap()).unwrap();
        let duplicate=request(app.clone(),"POST",format!("/team-room/{room}/messages"),&a.token,Some(message)).await;
        assert_eq!(duplicate.status(),StatusCode::CREATED);
        let second:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(duplicate.into_body(),20000).await.unwrap()).unwrap();
        assert_eq!(first["message_id"],second["message_id"]);
        let event_count:i64=sqlx::query_scalar("SELECT COUNT(*) FROM outbox_events WHERE event_type='MESSAGE_CREATED' AND aggregate_id=$1").bind(Uuid::parse_str(first["channel_id"].as_str().unwrap()).unwrap()).fetch_one(&pool).await.unwrap();
        assert_eq!(event_count,1,"idempotent retry must not duplicate outbox events");

        let private_body=json!({"client_message_id":Uuid::new_v4(),"body":"A-to-B confidential","parent_message_id":null});
        let sent_private=request(app.clone(),"POST",format!("/team-room/{room}/private/{}/messages",b.principal),&a.token,Some(private_body)).await;
        assert_eq!(sent_private.status(),StatusCode::CREATED);
        let b_history=request(app.clone(),"GET",format!("/team-room/{room}/private/{}/messages",a.principal),&b.token,None).await;
        assert_eq!(b_history.status(),StatusCode::OK);
        let b_json:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(b_history.into_body(),20000).await.unwrap()).unwrap();
        assert!(b_json["items"].as_array().unwrap().iter().any(|m|m["body"]=="A-to-B confidential"));
        let c_history=request(app.clone(),"GET",format!("/team-room/{room}/private/{}/messages",b.principal),&c.token,None).await;
        assert_eq!(c_history.status(),StatusCode::FORBIDDEN,"third participant without private_chat grant cannot read A↔B conversation");

        let decision=request(app.clone(),"POST",format!("/team-room/{room}/decisions"),&a.token,Some(json!({"title":"Use shared outbox","decision":"All room events must be committed with their aggregate mutation.","participants":[],"mission_id":null,"reference_message_id":null}))).await;
        assert_eq!(decision.status(),StatusCode::CREATED,"empty participant lists are valid");
        let decisions=request(app.clone(),"GET",format!("/team-room/{room}/decisions"),&b.token,None).await;
        assert_eq!(decisions.status(),StatusCode::OK);
        let decision_json:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(decisions.into_body(),20000).await.unwrap()).unwrap();
        assert!(decision_json.as_array().unwrap().iter().any(|d|d["title"]=="Use shared outbox"));

        let channel_id=first["channel_id"].as_str().unwrap();
        let attachment=request(app.clone(),"POST",format!("/team-room/{room}/attachments"),&a.token,Some(json!({"channel_id":channel_id,"message_id":first["message_id"],"task_id":null,"file_name":"architecture.md","storage_ref":"object://team-room/architecture-v1","content_type":"text/markdown","byte_len":128,"sha256":null}))).await;
        assert_eq!(attachment.status(),StatusCode::CREATED);
        let files=request(app.clone(),"GET",format!("/team-room/{room}/attachments"),&b.token,None).await;
        assert_eq!(files.status(),StatusCode::OK);
        let files_json:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(files.into_body(),20000).await.unwrap()).unwrap();
        assert!(files_json.as_array().unwrap().iter().any(|item|item["file_name"]=="architecture.md"));

        let no_session=request(app.clone(),"GET",format!("/team-room/{room}/messages"),"invalid-session",None).await;
        assert_eq!(no_session.status(),StatusCode::UNAUTHORIZED);
        let ticket=request(app.clone(),"POST",format!("/team-room/{room}/ws-ticket"),&a.token,Some(json!({}))).await;
        assert_eq!(ticket.status(),StatusCode::OK);
        let ticket_json:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(ticket.into_body(),4096).await.unwrap()).unwrap();
        let ticket_value=ticket_json["ticket"].as_str().unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server_pool=pool.clone();
        let server=tokio::spawn(async move{axum::serve(listener,router(server_pool)).await.unwrap();});
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let ws_url=format!("ws://{address}/team-room/{room}/ws?ticket={ticket_value}");
        let mut ws_request=ws_url.into_client_request().unwrap();
        ws_request.headers_mut().insert("Sec-WebSocket-Protocol","team-room.v1".parse().unwrap());
        let (mut ws,_)=tokio_tungstenite::connect_async(ws_request).await.expect("authenticated websocket handshake");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let client=reqwest::Client::new();
        let ws_message=json!({"client_message_id":Uuid::new_v4(),"body":"websocket live delivery","parent_message_id":null});
        let response=client.post(format!("http://{address}/team-room/{room}/messages")).bearer_auth(&a.token).json(&ws_message).send().await.unwrap();
        assert_eq!(response.status(),reqwest::StatusCode::CREATED);
        use futures_util::StreamExt;
        let frame=tokio::time::timeout(std::time::Duration::from_secs(5),ws.next()).await.unwrap().unwrap().unwrap();
        let event:serde_json::Value=serde_json::from_str(frame.into_text().unwrap().as_str()).unwrap();
        assert_eq!(event["type"],"MESSAGE_CREATED");

        let c_ticket=request(app.clone(),"POST",format!("/team-room/{room}/ws-ticket"),&c.token,Some(json!({}))).await;
        assert_eq!(c_ticket.status(),StatusCode::OK);
        let c_ticket_json:serde_json::Value=serde_json::from_slice(&axum::body::to_bytes(c_ticket.into_body(),4096).await.unwrap()).unwrap();
        let c_ws_url=format!("ws://{address}/team-room/{room}/ws?ticket={}",c_ticket_json["ticket"].as_str().unwrap());
        let mut c_ws_request=c_ws_url.into_client_request().unwrap();
        c_ws_request.headers_mut().insert("Sec-WebSocket-Protocol","team-room.v1".parse().unwrap());
        let (mut c_ws,_)=tokio_tungstenite::connect_async(c_ws_request).await.expect("third participant websocket");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let private_event=client.post(format!("http://{address}/team-room/{room}/private/{}/messages",b.principal)).bearer_auth(&a.token)
            .json(&json!({"client_message_id":Uuid::new_v4(),"body":"private event must not fan out to C","parent_message_id":null})).send().await.unwrap();
        assert_eq!(private_event.status(),reqwest::StatusCode::CREATED);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(500),c_ws.next()).await.is_err(),"non-member C must not receive private-channel outbox notifications");
        server.abort();

        let bad_ws=request(app.clone(),"GET",format!("/team-room/{room}/ws?ticket=invalid"),&a.token,None).await;
        assert_eq!(bad_ws.status(),StatusCode::UNAUTHORIZED,"invalid websocket tickets must be rejected before upgrade parsing");

        // Remove channel memberships first: added_by_principal_id preserves the
        // actor reference and intentionally has no cascading delete policy.
        sqlx::query("DELETE FROM team_room_channel_members WHERE channel_id IN (SELECT channel_id FROM team_room_channels WHERE tenant_id=$1)")
            .bind(tenant).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tenants WHERE tenant_id=$1").bind(tenant).execute(&pool).await.unwrap();
        let _=(a.participant,a.session,b.participant,b.session,c.participant,c.session);
    }
}
