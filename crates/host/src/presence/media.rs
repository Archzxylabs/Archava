//! Authenticated same-origin media bridge. No provider URL, model or credentials
//! are accepted from the browser. Provider text cannot execute business commands.
use super::*;
use axum::extract::{
    ws::{rejection::WebSocketUpgradeRejection, Message, WebSocket},
    WebSocketUpgrade,
};
use serde_json::Value;
use tokio::time::{timeout, Duration, Instant};
fn origin_matches(headers: &HeaderMap) -> bool {
    if headers.get("x-archava-client").is_some_and(|v| v == "cli")
        && headers.get("origin").is_none()
    {
        return true;
    }
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<axum::http::Uri>().ok());
    let host = headers.get("host").and_then(|v| v.to_str().ok());
    origin.is_some_and(|o| {
        matches!(o.scheme_str(), Some("http" | "https"))
            && o.authority().map(|a| a.as_str()) == host
            && o.path() == "/"
    })
}
async fn current(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    actor: &str,
) -> Result<PresenceSession, Response> {
    let session = authenticate(state, headers, false)
        .await
        .map_err(|(s, b)| (s, b).into_response())?;
    if session.principal_id != actor {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let client = get_voice_client(state).map_err(|e| *e)?;
    let target = client
        .find_session(id, now_ms())
        .map_err(|_| StatusCode::NOT_FOUND.into_response())?;
    if target.participant_id != actor
        || !target.is_active
        || target.current_mode == PresenceMode::Chat
    {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    verify_active_membership(state, &target.tenant_id, actor).await?;
    Ok(target)
}
pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    path: std::result::Result<RoutePath<String>, axum::extract::rejection::PathRejection>,
    upgrade: std::result::Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    let (Ok(RoutePath(id)), Ok(upgrade)) = (path, upgrade) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !origin_matches(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let session = match authenticate(&state, &headers, false).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = current(&state, &headers, &id, &session.principal_id).await {
        return e;
    }
    let Some(config) = state
        .presence
        .client
        .as_ref()
        .and_then(|c| c.live_config())
        .cloned()
    else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"voice_media_unconfigured"})),
        )
            .into_response();
    };
    upgrade
        .max_message_size(262144)
        .max_frame_size(262144)
        .on_upgrade(move |socket| run(socket, state, headers, id, session.principal_id, config))
        .into_response()
}
async fn event(socket: &mut WebSocket, value: Value) -> bool {
    timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}
async fn run(
    mut socket: WebSocket,
    state: AppState,
    mut headers: HeaderMap,
    id: String,
    actor: String,
    config: archava_providers::voice::live::LiveConfig,
) {
    // Browser WebSockets cannot set the CSRF header. The first bounded frame
    // echoes it; no provider connection opens before full mutation authentication.
    let authorized = async {
        let Some(Ok(Message::Text(text))) = socket.recv().await else {
            return false;
        };
        if text.len() > 1024 {
            return false;
        }
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return false;
        };
        let Some(csrf) = value["csrf"]
            .as_str()
            .and_then(|v| axum::http::HeaderValue::from_str(v).ok())
        else {
            return false;
        };
        headers.insert("x-archava-csrf", csrf);
        authenticate(&state, &headers, true)
            .await
            .is_ok_and(|s| s.principal_id == actor)
    };
    if !timeout(Duration::from_secs(5), authorized)
        .await
        .unwrap_or(false)
    {
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    let Some(client) = state.presence.client.as_ref() else {
        return;
    };
    let _lease = match client.claim_stream(&id) {
        Ok(v) => v,
        Err(_) => {
            event(&mut socket,json!({"type":"error","message":"This session already has a media connection, or voice capacity is busy."})).await;
            return;
        }
    };
    let mut live = match config.connect().await {
        Ok(v) => v,
        Err(_) => {
            event(&mut socket,json!({"type":"error","message":"Voice provider could not connect. Continue in chat."})).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };
    if !event(
        &mut socket,
        json!({"type":"ready","audioRate":24000,"inputRate":16000}),
    )
    .await
    {
        return;
    }
    let started = Instant::now();
    let mut checked = Instant::now();
    let mut video_at = Instant::now() - Duration::from_secs(1);
    let mut total = 0usize;
    let mut audio_window = Instant::now();
    let mut audio_bytes = 0usize;
    let mut text_at = Instant::now() - Duration::from_secs(1);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    loop {
        if started.elapsed() > Duration::from_secs(300) || total > 64 * 1024 * 1024 {
            break;
        }
        tokio::select! {
            _=heartbeat.tick()=>{
                if current(&state,&headers,&id,&actor).await.is_err(){break;}checked=Instant::now();
            },
            message=socket.recv()=>{
                if checked.elapsed()>Duration::from_secs(1)&&current(&state,&headers,&id,&actor).await.is_err(){break;}
                let result=match message {
                    Some(Ok(Message::Binary(pcm)))=>{if audio_window.elapsed()>=Duration::from_secs(1){audio_window=Instant::now();audio_bytes=0;}audio_bytes+=pcm.len();if audio_bytes>64000{break;}total+=pcm.len();live.audio(&pcm).await},
                    Some(Ok(Message::Text(text)))=>{total+=text.len();let Ok(value)=serde_json::from_str::<Value>(&text)else{break;};
                        match value["type"].as_str(){Some("video")if video_at.elapsed()>=Duration::from_millis(900)=>{video_at=Instant::now();live.video(value["data"].as_str().unwrap_or_default()).await},Some("text") if text_at.elapsed()>=Duration::from_millis(900)=>{text_at=Instant::now();live.text(value["text"].as_str().unwrap_or_default()).await},Some("stop")=>break,_=>break}
                    },
                    Some(Ok(Message::Ping(_)|Message::Pong(_)))=>continue,_=>break,
                };
                if result.is_err(){break;}
            },
            message=live.receive()=>{
                let Ok(body)=message else{break;};let Some(content)=body.get("serverContent")else{continue;};
                if current(&state,&headers,&id,&actor).await.is_err(){break;}
                if !event(&mut socket,json!({"type":"content","content":content})).await{break;}
            }
        }
    }
    live.close().await;
    if let Some(client) = state.presence.client.as_ref() {
        if let Ok(target) = client.find_session(&id, now_ms()) {
            let _ = client.degrade_to_chat(&target.tenant_id, &id, "media_disconnected", now_ms());
        }
    }
    event(&mut socket,json!({"type":"closed","message":"Media stopped. Continue in chat or start a new voice session."})).await;
    let _ = socket.send(Message::Close(None)).await;
}
