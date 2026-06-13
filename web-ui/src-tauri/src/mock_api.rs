use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use async_stream::stream;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{broadcast, RwLock},
};
use tower_http::cors::{Any, CorsLayer};

const PREFERRED_ADDR: &str = "127.0.0.1:8080";
const MOCK_ASSISTANT_ID: &str = "mock-assistant";
const MOCK_MODEL_ID: &str = "mock-chat";
const MOCK_PROVIDER_ID: &str = "mock-provider";
const MOCK_WELCOME_CONVERSATION_ID: &str = "mock-welcome";
const MOCK_REPLY_TEXT: &str = "这是 RikkaDesk Mock 后端返回的测试回复。";

#[derive(Clone)]
pub struct MockApiHandle {
    base_url: String,
}

impl MockApiHandle {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

#[derive(Clone, Serialize)]
struct SsePayload {
    event: String,
    data: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationListDto {
    id: String,
    assistant_id: String,
    title: String,
    is_pinned: bool,
    create_at: u64,
    update_at: u64,
    is_generating: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageDto {
    id: String,
    role: String,
    parts: Vec<Value>,
    annotations: Option<Vec<Value>>,
    created_at: String,
    finished_at: Option<String>,
    model_id: Option<String>,
    usage: Option<Value>,
    translation: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageNodeDto {
    id: String,
    messages: Vec<MessageDto>,
    select_index: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationDto {
    id: String,
    assistant_id: String,
    title: String,
    messages: Vec<MessageNodeDto>,
    truncate_index: i32,
    chat_suggestions: Vec<String>,
    is_pinned: bool,
    custom_system_prompt: Option<String>,
    mode_injection_ids: Option<Vec<String>>,
    lorebook_ids: Option<Vec<String>>,
    create_at: u64,
    update_at: u64,
    is_generating: bool,
}

impl ConversationDto {
    fn to_list_item(&self) -> ConversationListDto {
        ConversationListDto {
            id: self.id.clone(),
            assistant_id: self.assistant_id.clone(),
            title: self.title.clone(),
            is_pinned: self.is_pinned,
            create_at: self.create_at,
            update_at: self.update_at,
            is_generating: self.is_generating,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PagedResult<T> {
    items: Vec<T>,
    next_offset: Option<usize>,
    has_more: bool,
}

#[derive(Deserialize)]
struct PagedQuery {
    offset: Option<usize>,
    limit: Option<usize>,
    query: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageRequest {
    parts: Vec<Value>,
    mode_injection_ids: Option<Vec<String>>,
    lorebook_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAssistantRequest {
    assistant_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAssistantModelRequest {
    assistant_id: String,
    model_id: String,
}

#[derive(Deserialize)]
struct AiIconQuery {
    name: Option<String>,
}

struct MockApiState {
    settings: RwLock<Value>,
    conversations: RwLock<HashMap<String, ConversationDto>>,
    conversation_txs: RwLock<HashMap<String, broadcast::Sender<SsePayload>>>,
    settings_tx: broadcast::Sender<SsePayload>,
    list_tx: broadcast::Sender<SsePayload>,
    seq: AtomicU64,
    id_seq: AtomicU64,
}

impl MockApiState {
    fn new() -> Self {
        let now = now_millis();
        let welcome = welcome_conversation(now);
        let mut conversations = HashMap::new();
        conversations.insert(welcome.id.clone(), welcome);
        let (settings_tx, _) = broadcast::channel(64);
        let (list_tx, _) = broadcast::channel(64);

        Self {
            settings: RwLock::new(default_settings()),
            conversations: RwLock::new(conversations),
            conversation_txs: RwLock::new(HashMap::new()),
            settings_tx,
            list_tx,
            seq: AtomicU64::new(1),
            id_seq: AtomicU64::new(1),
        }
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn next_id(&self, prefix: &str) -> String {
        let id = self.id_seq.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{prefix}-{id}")
    }
}

pub async fn start() -> Result<MockApiHandle, Box<dyn std::error::Error>> {
    let state = Arc::new(MockApiState::new());
    let router = Router::new()
        .route("/api/settings/stream", get(settings_stream))
        .route("/api/conversations/paged", get(conversations_paged))
        .route("/api/conversations/stream", get(conversations_stream))
        .route("/api/ai-icon", get(ai_icon))
        .route("/api/conversations/{id}", get(conversation_detail))
        .route("/api/conversations/{id}/stream", get(conversation_stream))
        .route("/api/conversations/{id}/messages", post(send_message))
        .route("/api/conversations/{id}/stop", post(stop_conversation))
        .route("/api/settings/assistant", post(update_assistant))
        .route("/api/settings/assistant/model", post(update_assistant_model))
        .fallback(not_implemented)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([header::ACCEPT, header::AUTHORIZATION, header::CONTENT_TYPE]),
        )
        .with_state(state);

    let preferred: SocketAddr = PREFERRED_ADDR.parse()?;
    let listener = match TcpListener::bind(preferred).await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?,
    };
    let addr = listener.local_addr()?;
    let base_url = format!("http://{addr}");
    eprintln!("RikkaDesk mock API listening on {base_url}");

    tauri::async_runtime::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            eprintln!("RikkaDesk mock API stopped: {error}");
        }
    });

    Ok(MockApiHandle { base_url })
}

async fn settings_stream(State(state): State<Arc<MockApiState>>) -> impl IntoResponse {
    let initial = settings_payload(&state).await;
    let mut rx = state.settings_tx.subscribe();
    let stream = stream! {
        yield sse_event("update", initial);

        loop {
            match rx.recv().await {
                Ok(payload) => yield sse_event(&payload.event, payload.data),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn conversations_paged(
    State(state): State<Arc<MockApiState>>,
    Query(query): Query<PagedQuery>,
) -> impl IntoResponse {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let keyword = query.query.unwrap_or_default().trim().to_lowercase();
    let conversations = state.conversations.read().await;
    let mut items: Vec<_> = conversations
        .values()
        .filter(|conversation| {
            keyword.is_empty() || conversation.title.to_lowercase().contains(&keyword)
        })
        .map(ConversationDto::to_list_item)
        .collect();

    items.sort_by(|left, right| {
        right
            .is_pinned
            .cmp(&left.is_pinned)
            .then_with(|| right.update_at.cmp(&left.update_at))
    });

    let total = items.len();
    let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
    let next_offset = offset + page.len();
    let has_more = next_offset < total;

    Json(PagedResult {
        items: page,
        next_offset: has_more.then_some(next_offset),
        has_more,
    })
}

async fn conversations_stream(State(state): State<Arc<MockApiState>>) -> impl IntoResponse {
    let initial = conversation_list_invalidate_payload(&state).await;
    let mut rx = state.list_tx.subscribe();
    let stream = stream! {
        yield sse_event("invalidate", initial);

        loop {
            match rx.recv().await {
                Ok(payload) => yield sse_event(&payload.event, payload.data),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn ai_icon(Query(query): Query<AiIconQuery>) -> impl IntoResponse {
    let name = query.name.unwrap_or_else(|| "RikkaDesk".to_string());
    let label = first_icon_char(&name);
    let escaped_name = escape_xml(&name);
    let escaped_label = escape_xml(&label);
    let color = color_for_name(&name);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="{escaped_name}">
  <rect width="64" height="64" rx="16" fill="#{color}"/>
  <circle cx="46" cy="18" r="7" fill="rgba(255,255,255,.25)"/>
  <text x="32" y="40" text-anchor="middle" font-family="Arial, sans-serif" font-size="28" font-weight="700" fill="white">{escaped_label}</text>
</svg>"##
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );

    (headers, svg)
}

async fn conversation_detail(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conversation = get_or_create_conversation(&state, &id).await;
    Json(conversation)
}

async fn conversation_stream(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conversation = get_or_create_conversation(&state, &id).await;
    let initial = conversation_snapshot_payload(&state, &conversation);
    let tx = conversation_sender(&state, &id).await;
    let mut rx = tx.subscribe();
    let stream = stream! {
        yield sse_event("snapshot", initial);

        loop {
            match rx.recv().await {
                Ok(payload) => yield sse_event(&payload.event, payload.data),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn send_message(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
    Json(payload): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let now = now_millis();
    let assistant_id = current_assistant_id(&state).await;
    let model_id = current_model_id(&state, &assistant_id).await;
    let created_at = now_iso();
    let user_text = first_text_part(&payload.parts);

    let updated = {
        let mut conversations = state.conversations.write().await;
        let conversation = conversations
            .entry(id.clone())
            .or_insert_with(|| empty_conversation(id.clone(), assistant_id.clone(), now));

        if conversation.messages.is_empty() {
            if let Some(title) = title_from_text(user_text.as_deref()) {
                conversation.title = title;
            }
        }

        conversation.mode_injection_ids = payload.mode_injection_ids;
        conversation.lorebook_ids = payload.lorebook_ids;
        conversation.messages.push(MessageNodeDto {
            id: state.next_id("node"),
            messages: vec![MessageDto {
                id: state.next_id("msg"),
                role: "USER".to_string(),
                parts: payload.parts,
                annotations: None,
                created_at: created_at.clone(),
                finished_at: Some(created_at.clone()),
                model_id: None,
                usage: None,
                translation: None,
            }],
            select_index: 0,
        });

        let reply_time = now_iso();
        conversation.messages.push(MessageNodeDto {
            id: state.next_id("node"),
            messages: vec![MessageDto {
                id: state.next_id("msg"),
                role: "ASSISTANT".to_string(),
                parts: vec![json!({
                    "type": "text",
                    "text": MOCK_REPLY_TEXT,
                })],
                annotations: None,
                created_at: reply_time.clone(),
                finished_at: Some(reply_time),
                model_id: Some(model_id),
                usage: None,
                translation: None,
            }],
            select_index: 0,
        });

        conversation.update_at = now_millis();
        conversation.is_generating = false;
        conversation.clone()
    };

    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "accepted" }))
}

async fn stop_conversation(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let maybe_updated = {
        let mut conversations = state.conversations.write().await;
        conversations.get_mut(&id).map(|conversation| {
            conversation.is_generating = false;
            conversation.update_at = now_millis();
            conversation.clone()
        })
    };

    if let Some(conversation) = maybe_updated {
        broadcast_conversation_snapshot(&state, &conversation).await;
        broadcast_list_invalidate(&state).await;
    }

    Json(json!({ "status": "stopped" }))
}

async fn update_assistant(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpdateAssistantRequest>,
) -> impl IntoResponse {
    {
        let mut settings = state.settings.write().await;
        settings["assistantId"] = json!(payload.assistant_id);
    }

    broadcast_settings_update(&state).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" }))
}

async fn update_assistant_model(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpdateAssistantModelRequest>,
) -> impl IntoResponse {
    {
        let mut settings = state.settings.write().await;
        settings["chatModelId"] = json!(payload.model_id.clone());

        if let Some(assistants) = settings
            .get_mut("assistants")
            .and_then(Value::as_array_mut)
        {
            for assistant in assistants {
                let is_target = assistant
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == payload.assistant_id);
                if is_target {
                    assistant["chatModelId"] = json!(payload.model_id);
                    break;
                }
            }
        }
    }

    broadcast_settings_update(&state).await;

    Json(json!({ "status": "ok" }))
}

async fn not_implemented() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "This endpoint is not implemented by the RikkaDesk Phase 2C mock API.",
            "code": 404,
        })),
    )
}

async fn get_or_create_conversation(state: &Arc<MockApiState>, id: &str) -> ConversationDto {
    if let Some(conversation) = state.conversations.read().await.get(id).cloned() {
        return conversation;
    }

    let assistant_id = current_assistant_id(state).await;
    let now = now_millis();
    let mut conversations = state.conversations.write().await;
    conversations
        .entry(id.to_string())
        .or_insert_with(|| empty_conversation(id.to_string(), assistant_id, now))
        .clone()
}

async fn conversation_sender(
    state: &Arc<MockApiState>,
    id: &str,
) -> broadcast::Sender<SsePayload> {
    if let Some(sender) = state.conversation_txs.read().await.get(id).cloned() {
        return sender;
    }

    let mut senders = state.conversation_txs.write().await;
    senders
        .entry(id.to_string())
        .or_insert_with(|| {
            let (tx, _) = broadcast::channel(64);
            tx
        })
        .clone()
}

async fn broadcast_settings_update(state: &Arc<MockApiState>) {
    let data = settings_payload(state).await;
    let _ = state.settings_tx.send(SsePayload {
        event: "update".to_string(),
        data,
    });
}

async fn broadcast_list_invalidate(state: &Arc<MockApiState>) {
    let data = conversation_list_invalidate_payload(state).await;
    let _ = state.list_tx.send(SsePayload {
        event: "invalidate".to_string(),
        data,
    });
}

async fn broadcast_conversation_snapshot(
    state: &Arc<MockApiState>,
    conversation: &ConversationDto,
) {
    let sender = conversation_sender(state, &conversation.id).await;
    let data = conversation_snapshot_payload(state, conversation);
    let _ = sender.send(SsePayload {
        event: "snapshot".to_string(),
        data,
    });
}

async fn settings_payload(state: &Arc<MockApiState>) -> Value {
    state.settings.read().await.clone()
}

async fn conversation_list_invalidate_payload(state: &Arc<MockApiState>) -> Value {
    json!({
        "type": "invalidate",
        "assistantId": current_assistant_id(state).await,
        "timestamp": now_millis(),
    })
}

fn conversation_snapshot_payload(state: &MockApiState, conversation: &ConversationDto) -> Value {
    json!({
        "type": "snapshot",
        "seq": state.next_seq(),
        "conversation": conversation,
        "serverTime": now_millis(),
    })
}

fn sse_event(event: &str, data: Value) -> Result<Event, Infallible> {
    let data = serde_json::to_string(&data).unwrap_or_else(|_| "null".to_string());
    Ok(Event::default().event(event).data(data))
}

async fn current_assistant_id(state: &Arc<MockApiState>) -> String {
    state
        .settings
        .read()
        .await
        .get("assistantId")
        .and_then(Value::as_str)
        .unwrap_or(MOCK_ASSISTANT_ID)
        .to_string()
}

async fn current_model_id(state: &Arc<MockApiState>, assistant_id: &str) -> String {
    let settings = state.settings.read().await;
    if let Some(assistants) = settings.get("assistants").and_then(Value::as_array) {
        for assistant in assistants {
            let is_target = assistant
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == assistant_id);
            if is_target {
                if let Some(model_id) = assistant.get("chatModelId").and_then(Value::as_str) {
                    return model_id.to_string();
                }
            }
        }
    }

    settings
        .get("chatModelId")
        .and_then(Value::as_str)
        .unwrap_or(MOCK_MODEL_ID)
        .to_string()
}

fn default_settings() -> Value {
    json!({
        "dynamicColor": false,
        "themeId": "system",
        "developerMode": false,
        "displaySetting": {
            "userNickname": "You",
            "showUserAvatar": true,
            "showModelIcon": true,
            "showModelName": true,
            "showTokenUsage": false,
            "showThinkingContent": true,
            "autoCloseThinking": false,
            "codeBlockAutoWrap": true,
            "codeBlockAutoCollapse": false,
            "showLineNumbers": true,
            "sendOnEnter": true,
            "enableAutoScroll": true,
            "fontSizeRatio": 1,
            "pasteLongTextAsFile": false,
            "pasteLongTextThreshold": 1000
        },
        "enableWebSearch": false,
        "favoriteModels": [MOCK_MODEL_ID],
        "chatModelId": MOCK_MODEL_ID,
        "assistantId": MOCK_ASSISTANT_ID,
        "providers": [
            {
                "id": MOCK_PROVIDER_ID,
                "enabled": true,
                "name": "RikkaDesk Mock",
                "models": [
                    {
                        "id": MOCK_MODEL_ID,
                        "modelId": "rikkadesk-mock",
                        "displayName": "RikkaDesk Mock",
                        "type": "CHAT",
                        "inputModalities": ["TEXT"],
                        "outputModalities": ["TEXT"],
                        "abilities": []
                    }
                ]
            }
        ],
        "assistants": [
            {
                "id": MOCK_ASSISTANT_ID,
                "chatModelId": MOCK_MODEL_ID,
                "reasoningLevel": null,
                "mcpServers": [],
                "modeInjectionIds": [],
                "lorebookIds": [],
                "allowConversationPromptInjection": false,
                "allowConversationSystemPrompt": false,
                "name": "RikkaDesk Mock",
                "avatar": {
                    "type": "text",
                    "content": "R"
                },
                "useAssistantAvatar": false,
                "tags": [],
                "quickMessageIds": []
            }
        ],
        "assistantTags": [],
        "modeInjections": [],
        "lorebooks": [],
        "mcpServers": [],
        "searchServices": [],
        "quickMessages": [],
        "searchServiceSelected": 0,
        "webServerJwtEnabled": false
    })
}

fn welcome_conversation(now: u64) -> ConversationDto {
    let created_at = now_iso();
    ConversationDto {
        id: MOCK_WELCOME_CONVERSATION_ID.to_string(),
        assistant_id: MOCK_ASSISTANT_ID.to_string(),
        title: "RikkaDesk Mock Welcome".to_string(),
        messages: vec![MessageNodeDto {
            id: "welcome-node-1".to_string(),
            messages: vec![MessageDto {
                id: "welcome-message-1".to_string(),
                role: "ASSISTANT".to_string(),
                parts: vec![json!({
                    "type": "text",
                    "text": "RikkaDesk Mock API 已启动。你可以发送一条消息测试本地桌面壳。",
                })],
                annotations: None,
                created_at: created_at.clone(),
                finished_at: Some(created_at),
                model_id: Some(MOCK_MODEL_ID.to_string()),
                usage: None,
                translation: None,
            }],
            select_index: 0,
        }],
        truncate_index: -1,
        chat_suggestions: vec!["测试 RikkaDesk Mock 后端".to_string()],
        is_pinned: false,
        custom_system_prompt: None,
        mode_injection_ids: Some(Vec::new()),
        lorebook_ids: Some(Vec::new()),
        create_at: now,
        update_at: now,
        is_generating: false,
    }
}

fn empty_conversation(id: String, assistant_id: String, now: u64) -> ConversationDto {
    ConversationDto {
        id,
        assistant_id,
        title: "RikkaDesk Mock Chat".to_string(),
        messages: Vec::new(),
        truncate_index: -1,
        chat_suggestions: Vec::new(),
        is_pinned: false,
        custom_system_prompt: None,
        mode_injection_ids: Some(Vec::new()),
        lorebook_ids: Some(Vec::new()),
        create_at: now,
        update_at: now,
        is_generating: false,
    }
}

fn first_text_part(parts: &[Value]) -> Option<String> {
    parts
        .iter()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .and_then(|part| part.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn title_from_text(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    if text.is_empty() {
        return None;
    }

    let mut title: String = text.chars().take(48).collect();
    if text.chars().count() > 48 {
        title.push_str("...");
    }
    Some(title)
}

fn first_icon_char(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|ch| ch.to_uppercase().collect())
        .unwrap_or_else(|| "R".to_string())
}

fn color_for_name(name: &str) -> String {
    let mut hash = 0u32;
    for byte in name.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(byte));
    }
    let hue = hash % 360;
    hsl_to_rgb_hex(hue as f32, 0.62, 0.46)
}

fn hsl_to_rgb_hex(h: f32, s: f32, l: f32) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h_prime as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let r = ((r1 + m) * 255.0).round() as u8;
    let g = ((g1 + m) * 255.0).round() as u8;
    let b = ((b1 + m) * 255.0).round() as u8;
    format!("{r:02x}{g:02x}{b:02x}")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}
