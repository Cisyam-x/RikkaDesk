use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fs as std_fs, io,
    net::SocketAddr,
    path::PathBuf,
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_stream::stream;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    fs,
    net::TcpListener,
    sync::{broadcast, RwLock},
};
use tower_http::cors::{Any, CorsLayer};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{GetLastError, LocalFree},
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};

const PREFERRED_ADDR: &str = "127.0.0.1:8080";
const PERSIST_DIR_NAME: &str = "mock-api";
const STATE_FILE_NAME: &str = "state.v1.json";
const STATE_TMP_FILE_NAME: &str = "state.v1.json.tmp";
const STATE_SCHEMA_VERSION: u32 = 2;
const LEGACY_STATE_SCHEMA_VERSION: u32 = 1;
const SECRETS_DIR_NAME: &str = "secrets";
#[cfg(not(windows))]
const SECRET_SERVICE_NAME: &str = "RikkaDesk";
const OPENAI_COMPATIBLE_PROVIDER_TYPE: &str = "openai-compatible";
const OPENAI_CHAT_TIMEOUT_SECS: u64 = 60;
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

type PersistenceResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type SecretStoreResult<T> = Result<T, String>;

trait SecretStore: Send + Sync {
    fn set_secret(&self, secret_ref: &str, value: &str) -> SecretStoreResult<()>;
    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>>;
    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()>;

    fn has_secret(&self, secret_ref: &str) -> SecretStoreResult<bool> {
        self.get_secret(secret_ref).map(|value| value.is_some())
    }
}

#[cfg(not(windows))]
struct KeyringSecretStore {
    service: &'static str,
}

#[cfg(not(windows))]
impl KeyringSecretStore {
    fn new() -> Self {
        Self {
            service: SECRET_SERVICE_NAME,
        }
    }

    fn entry(&self, secret_ref: &str) -> SecretStoreResult<keyring::Entry> {
        keyring::Entry::new(self.service, &secret_storage_key_for_ref(secret_ref))
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(windows))]
impl SecretStore for KeyringSecretStore {
    fn set_secret(&self, secret_ref: &str, value: &str) -> SecretStoreResult<()> {
        self.entry(secret_ref)?
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>> {
        match self.entry(secret_ref)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()> {
        match self.entry(secret_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(windows)]
struct WindowsDpapiSecretStore {
    secrets_dir: PathBuf,
}

#[cfg(windows)]
impl WindowsDpapiSecretStore {
    fn new(secrets_dir: PathBuf) -> Self {
        Self { secrets_dir }
    }

    fn secret_path(&self, secret_ref: &str) -> PathBuf {
        self.secrets_dir
            .join(format!("{}.bin", secret_storage_key_for_ref(secret_ref)))
    }
}

#[cfg(windows)]
impl SecretStore for WindowsDpapiSecretStore {
    fn set_secret(&self, secret_ref: &str, value: &str) -> SecretStoreResult<()> {
        std_fs::create_dir_all(&self.secrets_dir).map_err(secret_io_error)?;

        let path = self.secret_path(secret_ref);
        let tmp_path = path.with_extension("bin.tmp");
        let protected = dpapi_protect(value.as_bytes())?;
        std_fs::write(&tmp_path, protected).map_err(secret_io_error)?;
        std_fs::rename(&tmp_path, &path).map_err(secret_io_error)?;
        Ok(())
    }

    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>> {
        let path = self.secret_path(secret_ref);
        let protected = match std_fs::read(path) {
            Ok(protected) => protected,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(secret_io_error(error)),
        };

        let plaintext = dpapi_unprotect(&protected)?;
        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|_| "Secret store value is not valid UTF-8".to_string())
    }

    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()> {
        match std_fs::remove_file(self.secret_path(secret_ref)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(secret_io_error(error)),
        }
    }
}

fn create_secret_store(app_data_dir: &std::path::Path) -> Arc<dyn SecretStore> {
    #[cfg(windows)]
    {
        Arc::new(WindowsDpapiSecretStore::new(
            app_data_dir.join(PERSIST_DIR_NAME).join(SECRETS_DIR_NAME),
        ))
    }

    #[cfg(not(windows))]
    {
        let _ = app_data_dir;
        Arc::new(KeyringSecretStore::new())
    }
}

#[cfg(windows)]
fn dpapi_protect(data: &[u8]) -> SecretStoreResult<Vec<u8>> {
    let cb_data = u32::try_from(data.len()).map_err(|_| "Secret is too large".to_string())?;
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let result = unsafe {
        CryptProtectData(
            &in_blob,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };

    if result == 0 {
        return Err(format!("Windows DPAPI protect failed: {}", unsafe {
            GetLastError()
        }));
    }

    dpapi_take_blob(out_blob)
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> SecretStoreResult<Vec<u8>> {
    let cb_data =
        u32::try_from(data.len()).map_err(|_| "Protected secret is too large".to_string())?;
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: cb_data,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let result = unsafe {
        CryptUnprotectData(
            &in_blob,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };

    if result == 0 {
        return Err(format!("Windows DPAPI unprotect failed: {}", unsafe {
            GetLastError()
        }));
    }

    dpapi_take_blob(out_blob)
}

#[cfg(windows)]
fn dpapi_take_blob(blob: CRYPT_INTEGER_BLOB) -> SecretStoreResult<Vec<u8>> {
    if blob.pbData.is_null() {
        return Ok(Vec::new());
    }

    let value = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() };
    unsafe {
        LocalFree(blob.pbData.cast());
    }
    Ok(value)
}

#[cfg(windows)]
fn secret_io_error(error: io::Error) -> String {
    error.to_string()
}

#[derive(Clone)]
struct MockPersistence {
    state_dir: PathBuf,
    state_path: PathBuf,
}

impl MockPersistence {
    fn new(app_data_dir: PathBuf) -> Self {
        let state_dir = app_data_dir.join(PERSIST_DIR_NAME);
        let state_path = state_dir.join(STATE_FILE_NAME);

        Self {
            state_dir,
            state_path,
        }
    }

    fn state_path(&self) -> &std::path::Path {
        &self.state_path
    }

    async fn save(&self, persisted: &PersistedMockState) -> PersistenceResult<()> {
        fs::create_dir_all(&self.state_dir).await?;

        let tmp_path = self.state_dir.join(STATE_TMP_FILE_NAME);
        let data = serde_json::to_vec_pretty(persisted)?;
        fs::write(&tmp_path, data).await?;

        match fs::remove_file(&self.state_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Box::new(error)),
        }

        fs::rename(&tmp_path, &self.state_path).await?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedMockState {
    schema_version: u32,
    saved_at: u64,
    id_seq: u64,
    settings: Value,
    conversations: HashMap<String, ConversationDto>,
    #[serde(default)]
    providers: Vec<DesktopProviderConfig>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProviderConfig {
    id: String,
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    model: DesktopProviderModelConfig,
    secret_ref: String,
}

impl DesktopProviderConfig {
    fn model_id_for_settings(&self) -> &str {
        &self.model.id
    }

    fn to_settings_provider(&self) -> Value {
        json!({
            "id": self.id,
            "type": self.provider_type,
            "enabled": self.enabled,
            "name": self.name,
            "baseUrl": self.base_url,
            "secretRef": self.secret_ref,
            "models": [
                {
                    "id": self.model.id,
                    "modelId": self.model.model_id,
                    "displayName": self.model.display_name,
                    "type": "CHAT",
                    "inputModalities": ["TEXT"],
                    "outputModalities": ["TEXT"],
                    "abilities": []
                }
            ]
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProviderModelConfig {
    id: String,
    model_id: String,
    display_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertDesktopProviderRequest {
    id: Option<String>,
    #[serde(rename = "type")]
    provider_type: Option<String>,
    enabled: Option<bool>,
    name: Option<String>,
    base_url: Option<String>,
    model_id: Option<String>,
    display_name: Option<String>,
    api_key: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProviderResponse {
    id: String,
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    model: DesktopProviderModelConfig,
    secret_ref: String,
    has_secret: bool,
}

impl DesktopProviderResponse {
    fn from_config(config: &DesktopProviderConfig, has_secret: bool) -> Self {
        Self {
            id: config.id.clone(),
            provider_type: config.provider_type.clone(),
            enabled: config.enabled,
            name: config.name.clone(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            secret_ref: config.secret_ref.clone(),
            has_secret,
        }
    }
}

struct OpenAiChatConfig {
    base_url: String,
    model_id: String,
    api_key: String,
}

#[derive(Serialize)]
struct OpenAiChatCompletionRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    stream: bool,
}

#[derive(Clone, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAiChatStreamResponse {
    choices: Vec<OpenAiChatStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAiChatStreamChoice {
    delta: OpenAiChatStreamDelta,
}

#[derive(Deserialize)]
struct OpenAiChatStreamDelta {
    content: Option<String>,
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
struct UpdateConversationTitleRequest {
    title: String,
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
    persistence: MockPersistence,
    secret_store: Arc<dyn SecretStore>,
    http_client: reqwest::Client,
    settings: RwLock<Value>,
    conversations: RwLock<HashMap<String, ConversationDto>>,
    providers: RwLock<Vec<DesktopProviderConfig>>,
    generating_flags: RwLock<HashSet<String>>,
    conversation_txs: RwLock<HashMap<String, broadcast::Sender<SsePayload>>>,
    settings_tx: broadcast::Sender<SsePayload>,
    list_tx: broadcast::Sender<SsePayload>,
    seq: AtomicU64,
    id_seq: AtomicU64,
}

impl MockApiState {
    fn new(
        persistence: MockPersistence,
        secret_store: Arc<dyn SecretStore>,
        mut persisted: PersistedMockState,
    ) -> Self {
        sync_settings_with_desktop_providers(&mut persisted.settings, &persisted.providers);
        let initial_id_seq = persisted
            .id_seq
            .max(max_persisted_id_seq(&persisted.conversations))
            .max(1);
        let (settings_tx, _) = broadcast::channel(64);
        let (list_tx, _) = broadcast::channel(64);

        Self {
            persistence,
            secret_store,
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(OPENAI_CHAT_TIMEOUT_SECS))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            settings: RwLock::new(persisted.settings),
            conversations: RwLock::new(persisted.conversations),
            providers: RwLock::new(persisted.providers),
            generating_flags: RwLock::new(HashSet::new()),
            conversation_txs: RwLock::new(HashMap::new()),
            settings_tx,
            list_tx,
            seq: AtomicU64::new(1),
            id_seq: AtomicU64::new(initial_id_seq),
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

pub async fn start(app_data_dir: PathBuf) -> Result<MockApiHandle, Box<dyn std::error::Error>> {
    let secret_store = create_secret_store(&app_data_dir);
    let persistence = MockPersistence::new(app_data_dir);
    eprintln!(
        "RikkaDesk mock API state file: {}",
        persistence.state_path().display()
    );
    let persisted = load_persisted_state(&persistence).await;
    let state = Arc::new(MockApiState::new(persistence, secret_store, persisted));
    let router = Router::new()
        .route("/api/settings/stream", get(settings_stream))
        .route("/api/conversations/paged", get(conversations_paged))
        .route("/api/conversations/stream", get(conversations_stream))
        .route("/api/ai-icon", get(ai_icon))
        .route(
            "/api/desktop/providers",
            get(desktop_providers).post(upsert_desktop_provider),
        )
        .route(
            "/api/desktop/providers/{id}/secret",
            post(update_desktop_provider_secret).delete(delete_desktop_provider_secret),
        )
        .route("/api/conversations/{id}", get(conversation_detail))
        .route("/api/conversations/{id}/stream", get(conversation_stream))
        .route("/api/conversations/{id}/messages", post(send_message))
        .route(
            "/api/conversations/{id}/title",
            post(update_conversation_title),
        )
        .route("/api/conversations/{id}/pin", post(toggle_conversation_pin))
        .route("/api/conversations/{id}", delete(delete_conversation))
        .route("/api/conversations/{id}/stop", post(stop_conversation))
        .route("/api/settings/assistant", post(update_assistant))
        .route(
            "/api/settings/assistant/model",
            post(update_assistant_model),
        )
        .fallback(not_implemented)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
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

async fn load_persisted_state(persistence: &MockPersistence) -> PersistedMockState {
    if !persistence.state_path().exists() {
        let persisted = default_persisted_state();
        if let Err(error) = persistence.save(&persisted).await {
            eprintln!("RikkaDesk mock API failed to create default state: {error}");
        }
        return persisted;
    }

    let bytes = match fs::read(persistence.state_path()).await {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("RikkaDesk mock API failed to read state file: {error}");
            let persisted = default_persisted_state();
            if let Err(error) = persistence.save(&persisted).await {
                eprintln!("RikkaDesk mock API failed to save fallback state: {error}");
            }
            return persisted;
        }
    };

    match serde_json::from_slice::<PersistedMockState>(&bytes) {
        Ok(mut persisted) if persisted.schema_version == STATE_SCHEMA_VERSION => {
            sync_settings_with_desktop_providers(&mut persisted.settings, &persisted.providers);
            persisted
        }
        Ok(persisted) if persisted.schema_version == LEGACY_STATE_SCHEMA_VERSION => {
            let mut migrated = migrate_v1_to_v2(persisted);
            sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
            if let Err(error) = persistence.save(&migrated).await {
                eprintln!("RikkaDesk mock API failed to save migrated state: {error}");
            }
            migrated
        }
        Ok(persisted) => {
            reset_persisted_state(
                persistence,
                format!("unsupported schemaVersion {}", persisted.schema_version),
            )
            .await
        }
        Err(error) => reset_persisted_state(persistence, format!("invalid JSON: {error}")).await,
    }
}

async fn reset_persisted_state(
    persistence: &MockPersistence,
    reason: String,
) -> PersistedMockState {
    eprintln!("RikkaDesk mock API state reset: {reason}");

    if let Err(error) = backup_corrupt_state(persistence).await {
        eprintln!("RikkaDesk mock API failed to back up corrupt state: {error}");
    }

    let persisted = default_persisted_state();
    if let Err(error) = persistence.save(&persisted).await {
        eprintln!("RikkaDesk mock API failed to save fallback state: {error}");
    }

    persisted
}

async fn backup_corrupt_state(persistence: &MockPersistence) -> PersistenceResult<()> {
    if !persistence.state_path().exists() {
        return Ok(());
    }

    fs::create_dir_all(&persistence.state_dir).await?;
    let backup_path = persistence
        .state_dir
        .join(format!("state.v1.corrupt.{}.json", now_millis()));
    fs::rename(persistence.state_path(), backup_path).await?;
    Ok(())
}

async fn persist_mock_state(state: &Arc<MockApiState>) {
    let settings = state.settings.read().await.clone();
    let conversations = state.conversations.read().await.clone();
    let providers = state.providers.read().await.clone();
    let id_seq = state
        .id_seq
        .load(Ordering::Relaxed)
        .max(max_persisted_id_seq(&conversations))
        .max(1);

    let persisted = PersistedMockState {
        schema_version: STATE_SCHEMA_VERSION,
        saved_at: now_millis(),
        id_seq,
        settings,
        conversations,
        providers,
    };

    if let Err(error) = state.persistence.save(&persisted).await {
        eprintln!("RikkaDesk mock API failed to save state: {error}");
    }
}

async fn desktop_providers(State(state): State<Arc<MockApiState>>) -> impl IntoResponse {
    match desktop_provider_responses(&state).await {
        Ok(providers) => Json(providers).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn upsert_desktop_provider(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpsertDesktopProviderRequest>,
) -> impl IntoResponse {
    match build_desktop_provider(&state, payload).await {
        Ok((provider, api_key)) => {
            if let Some(api_key) = api_key {
                if let Err(error) = state
                    .secret_store
                    .set_secret(&provider.secret_ref, &api_key)
                {
                    eprintln!("RikkaDesk provider secret save failed: {error}");
                    return internal_error_response("Secret store is unavailable");
                }
            }

            {
                let mut providers = state.providers.write().await;
                if let Some(existing) = providers.iter_mut().find(|item| item.id == provider.id) {
                    *existing = provider.clone();
                } else {
                    providers.push(provider.clone());
                }
            }

            {
                let providers = state.providers.read().await;
                let mut settings = state.settings.write().await;
                sync_settings_with_desktop_providers(&mut settings, &providers);
                set_current_model_in_settings(&mut settings, provider.model_id_for_settings());
            }

            persist_mock_state(&state).await;
            broadcast_settings_update(&state).await;

            let has_secret = match state.secret_store.has_secret(&provider.secret_ref) {
                Ok(has_secret) => has_secret,
                Err(error) => {
                    eprintln!("RikkaDesk provider secret status failed: {error}");
                    return internal_error_response("Secret store is unavailable");
                }
            };

            Json(DesktopProviderResponse::from_config(&provider, has_secret)).into_response()
        }
        Err(response) => response,
    }
}

async fn update_desktop_provider_secret(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpsertDesktopProviderRequest>,
) -> impl IntoResponse {
    let Some(api_key) = payload.api_key.map(|value| value.trim().to_string()) else {
        return bad_request_response("apiKey is required");
    };
    if api_key.is_empty() {
        return bad_request_response("apiKey is required");
    }

    let provider = {
        let providers = state.providers.read().await;
        providers.iter().find(|item| item.id == id).cloned()
    };

    let Some(provider) = provider else {
        return not_found_response("Provider not found");
    };

    if let Err(error) = state
        .secret_store
        .set_secret(&provider.secret_ref, &api_key)
    {
        eprintln!("RikkaDesk provider secret update failed: {error}");
        return internal_error_response("Secret store is unavailable");
    }

    Json(json!({ "status": "ok", "hasSecret": true })).into_response()
}

async fn delete_desktop_provider_secret(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let provider = {
        let providers = state.providers.read().await;
        providers.iter().find(|item| item.id == id).cloned()
    };

    let Some(provider) = provider else {
        return not_found_response("Provider not found");
    };

    if let Err(error) = state.secret_store.delete_secret(&provider.secret_ref) {
        eprintln!("RikkaDesk provider secret delete failed: {error}");
        return internal_error_response("Secret store is unavailable");
    }

    Json(json!({ "status": "ok", "hasSecret": false })).into_response()
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

    let updated_after_user_message = {
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

        conversation.update_at = now_millis();
        conversation.is_generating = true;
        conversation.clone()
    };

    persist_mock_state(&state).await;
    broadcast_conversation_snapshot(&state, &updated_after_user_message).await;
    broadcast_list_invalidate(&state).await;

    let real_chat_config = if user_text.is_some() {
        match resolve_openai_chat_config(&state, &model_id).await {
            Ok(config) => config,
            Err(error) => {
                append_assistant_reply(
                    &state,
                    &id,
                    &assistant_id,
                    &model_id,
                    format!("Real provider request failed: {error}"),
                    now,
                )
                .await;
                return Json(json!({ "status": "accepted" }));
            }
        }
    } else {
        None
    };

    let Some(config) = real_chat_config else {
        let reply_text = if user_text.is_none() {
            "Phase 3E currently supports text-only chat.".to_string()
        } else {
            MOCK_REPLY_TEXT.to_string()
        };
        append_assistant_reply(&state, &id, &assistant_id, &model_id, reply_text, now).await;
        return Json(json!({ "status": "accepted" }));
    };

    let messages = openai_messages_from_conversation(&updated_after_user_message);
    if messages.is_empty() {
        append_assistant_reply(
            &state,
            &id,
            &assistant_id,
            &model_id,
            "Phase 3E currently supports text-only chat.".to_string(),
            now,
        )
        .await;
        return Json(json!({ "status": "accepted" }));
    }

    let assistant_message_id =
        append_empty_streaming_assistant_reply(&state, &id, &assistant_id, &model_id, now).await;
    start_generation(&state, &id).await;

    let stream_result =
        stream_openai_compatible_chat(&state, &id, &assistant_message_id, &config, messages).await;

    if let Err(error) = stream_result {
        let error_text = format!("Real provider request failed: {error}");
        append_text_to_assistant_message(&state, &id, &assistant_message_id, &error_text).await;
    }

    finish_streaming_assistant_reply(&state, &id, &assistant_message_id).await;

    Json(json!({ "status": "accepted" }))
}

async fn stop_conversation(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    stop_generation(&state, &id).await;

    let maybe_updated = {
        let mut conversations = state.conversations.write().await;
        conversations.get_mut(&id).map(|conversation| {
            conversation.is_generating = false;
            conversation.update_at = now_millis();
            conversation.clone()
        })
    };

    if let Some(conversation) = maybe_updated {
        persist_mock_state(&state).await;
        broadcast_conversation_snapshot(&state, &conversation).await;
        broadcast_list_invalidate(&state).await;
    }

    Json(json!({ "status": "stopped" }))
}

async fn update_conversation_title(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateConversationTitleRequest>,
) -> impl IntoResponse {
    let title = payload.title.trim();
    if title.is_empty() {
        return bad_request_response("Title cannot be empty");
    }

    let updated = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&id) else {
            return not_found_response("Conversation not found");
        };

        conversation.title = title.chars().take(120).collect();
        conversation.update_at = now_millis();
        conversation.clone()
    };

    persist_mock_state(&state).await;
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn toggle_conversation_pin(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let updated = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&id) else {
            return not_found_response("Conversation not found");
        };

        conversation.is_pinned = !conversation.is_pinned;
        conversation.clone()
    };

    persist_mock_state(&state).await;
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok", "isPinned": updated.is_pinned })).into_response()
}

async fn delete_conversation(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let removed = {
        let mut conversations = state.conversations.write().await;
        conversations.remove(&id)
    };

    if removed.is_none() {
        return not_found_response("Conversation not found");
    }

    stop_generation(&state, &id).await;
    state.conversation_txs.write().await.remove(&id);

    persist_mock_state(&state).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "deleted" })).into_response()
}

async fn update_assistant(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpdateAssistantRequest>,
) -> impl IntoResponse {
    {
        let mut settings = state.settings.write().await;
        settings["assistantId"] = json!(payload.assistant_id);
    }

    persist_mock_state(&state).await;
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

        if let Some(assistants) = settings.get_mut("assistants").and_then(Value::as_array_mut) {
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

    persist_mock_state(&state).await;
    broadcast_settings_update(&state).await;

    Json(json!({ "status": "ok" }))
}

async fn not_implemented() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "This endpoint is not implemented by the RikkaDesk mock API.",
            "code": 404,
        })),
    )
}

async fn desktop_provider_responses(
    state: &Arc<MockApiState>,
) -> Result<Vec<DesktopProviderResponse>, String> {
    let providers = state.providers.read().await;
    providers
        .iter()
        .map(|provider| {
            state
                .secret_store
                .has_secret(&provider.secret_ref)
                .map(|has_secret| DesktopProviderResponse::from_config(provider, has_secret))
        })
        .collect()
}

async fn append_assistant_reply(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_id: &str,
    model_id: &str,
    reply_text: String,
    now: u64,
) {
    let updated = {
        let mut conversations = state.conversations.write().await;
        let conversation = conversations
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                empty_conversation(conversation_id.to_string(), assistant_id.to_string(), now)
            });

        let reply_time = now_iso();
        conversation.messages.push(MessageNodeDto {
            id: state.next_id("node"),
            messages: vec![MessageDto {
                id: state.next_id("msg"),
                role: "ASSISTANT".to_string(),
                parts: vec![json!({
                    "type": "text",
                    "text": reply_text,
                })],
                annotations: None,
                created_at: reply_time.clone(),
                finished_at: Some(reply_time),
                model_id: Some(model_id.to_string()),
                usage: None,
                translation: None,
            }],
            select_index: 0,
        });

        conversation.update_at = now_millis();
        conversation.is_generating = false;
        conversation.clone()
    };

    persist_mock_state(state).await;
    broadcast_conversation_snapshot(state, &updated).await;
    broadcast_list_invalidate(state).await;
}

async fn append_empty_streaming_assistant_reply(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_id: &str,
    model_id: &str,
    now: u64,
) -> String {
    let message_id = state.next_id("msg");
    let updated = {
        let mut conversations = state.conversations.write().await;
        let conversation = conversations
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                empty_conversation(conversation_id.to_string(), assistant_id.to_string(), now)
            });

        let reply_time = now_iso();
        conversation.messages.push(MessageNodeDto {
            id: state.next_id("node"),
            messages: vec![MessageDto {
                id: message_id.clone(),
                role: "ASSISTANT".to_string(),
                parts: vec![json!({
                    "type": "text",
                    "text": "",
                })],
                annotations: None,
                created_at: reply_time,
                finished_at: None,
                model_id: Some(model_id.to_string()),
                usage: None,
                translation: None,
            }],
            select_index: 0,
        });

        conversation.update_at = now_millis();
        conversation.is_generating = true;
        conversation.clone()
    };

    persist_mock_state(state).await;
    broadcast_conversation_snapshot(state, &updated).await;
    broadcast_list_invalidate(state).await;
    message_id
}

async fn append_text_to_assistant_message(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_message_id: &str,
    text: &str,
) {
    if text.is_empty() {
        return;
    }

    let updated = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(conversation_id) else {
            return;
        };
        let Some(message) = find_message_mut(conversation, assistant_message_id) else {
            return;
        };

        append_text_part(&mut message.parts, text);
        conversation.update_at = now_millis();
        conversation.clone()
    };

    broadcast_conversation_snapshot(state, &updated).await;
}

async fn finish_streaming_assistant_reply(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_message_id: &str,
) {
    stop_generation(state, conversation_id).await;

    let updated = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(conversation_id) else {
            return;
        };
        if let Some(message) = find_message_mut(conversation, assistant_message_id) {
            if text_from_parts(&message.parts).is_none() {
                append_text_part(&mut message.parts, "Generation stopped.");
            }
            message.finished_at = Some(now_iso());
        }
        conversation.update_at = now_millis();
        conversation.is_generating = false;
        conversation.clone()
    };

    persist_mock_state(state).await;
    broadcast_conversation_snapshot(state, &updated).await;
    broadcast_list_invalidate(state).await;
}

async fn start_generation(state: &Arc<MockApiState>, conversation_id: &str) {
    state
        .generating_flags
        .write()
        .await
        .insert(conversation_id.to_string());
}

async fn stop_generation(state: &Arc<MockApiState>, conversation_id: &str) {
    state.generating_flags.write().await.remove(conversation_id);
}

async fn is_generation_active(state: &Arc<MockApiState>, conversation_id: &str) -> bool {
    state
        .generating_flags
        .read()
        .await
        .contains(conversation_id)
}

async fn resolve_openai_chat_config(
    state: &Arc<MockApiState>,
    selected_model_id: &str,
) -> Result<Option<OpenAiChatConfig>, String> {
    let provider = {
        let providers = state.providers.read().await;
        providers
            .iter()
            .find(|provider| {
                provider.enabled
                    && provider.provider_type == OPENAI_COMPATIBLE_PROVIDER_TYPE
                    && (provider.model.id == selected_model_id
                        || provider.model.model_id == selected_model_id)
            })
            .cloned()
    };

    let Some(provider) = provider else {
        return Ok(None);
    };

    let base_url = provider.base_url.trim();
    let model_id = provider.model.model_id.trim();
    if base_url.is_empty() || model_id.is_empty() {
        return Ok(None);
    }

    let Some(api_key) = state.secret_store.get_secret(&provider.secret_ref)? else {
        return Ok(None);
    };
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Ok(None);
    }

    Ok(Some(OpenAiChatConfig {
        base_url: base_url.to_string(),
        model_id: model_id.to_string(),
        api_key,
    }))
}

async fn stream_openai_compatible_chat(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_message_id: &str,
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
) -> Result<(), String> {
    let request = OpenAiChatCompletionRequest {
        model: config.model_id.clone(),
        messages,
        stream: true,
    };

    let mut response = state
        .http_client
        .post(openai_chat_completions_url(&config.base_url))
        .bearer_auth(&config.api_key)
        .json(&request)
        .send()
        .await
        .map_err(safe_reqwest_error)?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "{} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("HTTP error")
        ));
    }

    let mut buffer = String::new();
    while let Some(chunk) = response.chunk().await.map_err(safe_reqwest_error)? {
        if !is_generation_active(state, conversation_id).await {
            return Ok(());
        }

        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim_end_matches('\r').to_string();
            buffer.drain(..=line_end);
            if handle_openai_stream_line(state, conversation_id, assistant_message_id, &line)
                .await?
            {
                return Ok(());
            }
            if !is_generation_active(state, conversation_id).await {
                return Ok(());
            }
        }
    }

    if !buffer.trim().is_empty()
        && handle_openai_stream_line(state, conversation_id, assistant_message_id, &buffer).await?
    {
        return Ok(());
    }

    if !is_generation_active(state, conversation_id).await {
        Ok(())
    } else {
        Err("stream ended before DONE".to_string())
    }
}

async fn handle_openai_stream_line(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_message_id: &str,
    line: &str,
) -> Result<bool, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return Ok(false);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(false);
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(true);
    }

    let chunk = serde_json::from_str::<OpenAiChatStreamResponse>(data)
        .map_err(|_| "stream returned invalid JSON".to_string())?;
    for choice in chunk.choices {
        if let Some(content) = choice.delta.content {
            append_text_to_assistant_message(
                state,
                conversation_id,
                assistant_message_id,
                &content,
            )
            .await;
        }
    }

    Ok(false)
}

fn openai_chat_completions_url(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
    }
}

fn safe_reqwest_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "network timeout".to_string()
    } else if error.is_connect() {
        "network connection failed".to_string()
    } else if error.is_decode() {
        "response decode failed".to_string()
    } else if error.is_builder() {
        "request build failed".to_string()
    } else {
        "request failed".to_string()
    }
}

fn openai_messages_from_conversation(conversation: &ConversationDto) -> Vec<OpenAiChatMessage> {
    let mut messages = Vec::new();

    if let Some(system_prompt) = conversation
        .custom_system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        messages.push(OpenAiChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        });
    }

    for node in &conversation.messages {
        let message = node
            .messages
            .get(node.select_index)
            .or_else(|| node.messages.first());
        let Some(message) = message else {
            continue;
        };
        let role = match message.role.as_str() {
            "USER" => "user",
            "ASSISTANT" => "assistant",
            "SYSTEM" => "system",
            _ => continue,
        };
        let Some(content) = text_from_parts(&message.parts) else {
            continue;
        };

        messages.push(OpenAiChatMessage {
            role: role.to_string(),
            content,
        });
    }

    trim_openai_message_history(messages)
}

fn trim_openai_message_history(messages: Vec<OpenAiChatMessage>) -> Vec<OpenAiChatMessage> {
    const MAX_MESSAGES: usize = 32;
    if messages.len() <= MAX_MESSAGES {
        return messages;
    }

    let mut trimmed = Vec::with_capacity(MAX_MESSAGES);
    let has_system = messages
        .first()
        .is_some_and(|message| message.role == "system");
    if has_system {
        trimmed.push(messages[0].clone());
    }

    let keep_tail = MAX_MESSAGES - trimmed.len();
    trimmed.extend(
        messages
            .into_iter()
            .rev()
            .take(keep_tail)
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    );
    trimmed
}

async fn build_desktop_provider(
    state: &Arc<MockApiState>,
    payload: UpsertDesktopProviderRequest,
) -> Result<(DesktopProviderConfig, Option<String>), Response> {
    let requested_type = payload
        .provider_type
        .unwrap_or_else(|| OPENAI_COMPATIBLE_PROVIDER_TYPE.to_string());
    let provider_type = requested_type.trim();
    if provider_type != OPENAI_COMPATIBLE_PROVIDER_TYPE {
        return Err(bad_request_response(
            "Only openai-compatible providers are supported",
        ));
    }

    let existing = if let Some(id) = payload
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let providers = state.providers.read().await;
        providers.iter().find(|item| item.id == id).cloned()
    } else {
        None
    };

    let id = payload
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| existing.as_ref().map(|provider| provider.id.clone()))
        .unwrap_or_else(|| state.next_id("desktop-provider"));

    if !is_safe_config_id(&id) {
        return Err(bad_request_response(
            "Provider id contains unsupported characters",
        ));
    }

    let base_url = payload
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| existing.as_ref().map(|provider| provider.base_url.clone()))
        .ok_or_else(|| bad_request_response("baseUrl is required"))?;

    let model_id = payload
        .model_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            existing
                .as_ref()
                .map(|provider| provider.model.model_id.clone())
        })
        .ok_or_else(|| bad_request_response("modelId is required"))?;

    let display_name = payload
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            existing
                .as_ref()
                .map(|provider| provider.model.display_name.clone())
        })
        .unwrap_or_else(|| model_id.clone());

    let model_record_id = existing
        .as_ref()
        .map(|provider| provider.model.id.clone())
        .unwrap_or_else(|| state.next_id("desktop-model"));

    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| existing.as_ref().map(|provider| provider.name.clone()))
        .unwrap_or_else(|| "OpenAI Compatible".to_string());

    let provider = DesktopProviderConfig {
        secret_ref: existing
            .as_ref()
            .map(|provider| provider.secret_ref.clone())
            .unwrap_or_else(|| secret_ref_for_provider(&id)),
        id,
        provider_type: OPENAI_COMPATIBLE_PROVIDER_TYPE.to_string(),
        enabled: payload
            .enabled
            .or_else(|| existing.as_ref().map(|provider| provider.enabled))
            .unwrap_or(true),
        name,
        base_url,
        model: DesktopProviderModelConfig {
            id: model_record_id,
            model_id,
            display_name,
        },
    };

    let api_key = payload
        .api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok((provider, api_key))
}

fn bad_request_response(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": message,
            "code": 400,
        })),
    )
        .into_response()
}

fn not_found_response(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": message,
            "code": 404,
        })),
    )
        .into_response()
}

fn internal_error_response(message: impl Into<String>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": message.into(),
            "code": 500,
        })),
    )
        .into_response()
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

async fn conversation_sender(state: &Arc<MockApiState>, id: &str) -> broadcast::Sender<SsePayload> {
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

fn default_persisted_state() -> PersistedMockState {
    let now = now_millis();
    let welcome = welcome_conversation(now);
    let mut conversations = HashMap::new();
    conversations.insert(welcome.id.clone(), welcome);

    PersistedMockState {
        schema_version: STATE_SCHEMA_VERSION,
        saved_at: now,
        id_seq: max_persisted_id_seq(&conversations).max(1),
        settings: default_settings(),
        conversations,
        providers: Vec::new(),
    }
}

fn migrate_v1_to_v2(mut persisted: PersistedMockState) -> PersistedMockState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.saved_at = now_millis();
    persisted.providers = Vec::new();
    persisted
}

fn sync_settings_with_desktop_providers(settings: &mut Value, providers: &[DesktopProviderConfig]) {
    let desktop_provider_ids: Vec<&str> = providers
        .iter()
        .map(|provider| provider.id.as_str())
        .collect();
    let existing_providers = settings
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut merged_providers: Vec<Value> = existing_providers
        .into_iter()
        .filter(|provider| {
            provider
                .get("id")
                .and_then(Value::as_str)
                .is_none_or(|id| !desktop_provider_ids.contains(&id))
        })
        .collect();

    merged_providers.extend(
        providers
            .iter()
            .map(DesktopProviderConfig::to_settings_provider),
    );

    settings["providers"] = Value::Array(merged_providers);
}

fn set_current_model_in_settings(settings: &mut Value, model_id: &str) {
    settings["chatModelId"] = json!(model_id);

    let current_assistant_id = settings
        .get("assistantId")
        .and_then(Value::as_str)
        .unwrap_or(MOCK_ASSISTANT_ID)
        .to_string();

    if let Some(assistants) = settings.get_mut("assistants").and_then(Value::as_array_mut) {
        for assistant in assistants {
            let is_current = assistant
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == current_assistant_id);
            if is_current {
                assistant["chatModelId"] = json!(model_id);
            }
        }
    }

    if let Some(favorite_models) = settings
        .get_mut("favoriteModels")
        .and_then(Value::as_array_mut)
    {
        let already_favorite = favorite_models
            .iter()
            .any(|item| item.as_str() == Some(model_id));
        if !already_favorite {
            favorite_models.push(json!(model_id));
        }
    } else {
        settings["favoriteModels"] = json!([model_id]);
    }
}

fn secret_ref_for_provider(provider_id: &str) -> String {
    format!("rikkadesk:provider:{provider_id}:api-key")
}

fn secret_storage_key_for_ref(secret_ref: &str) -> String {
    let mut account = String::with_capacity(secret_ref.len());
    for byte in secret_ref.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                account.push(byte as char);
            }
            _ => account.push_str(&format!("%{byte:02X}")),
        }
    }
    account
}

fn is_safe_config_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
}

fn max_persisted_id_seq(conversations: &HashMap<String, ConversationDto>) -> u64 {
    let mut max_id = 1;

    for conversation in conversations.values() {
        if let Some(id) = numeric_suffix(&conversation.id) {
            max_id = max_id.max(id);
        }

        for node in &conversation.messages {
            if let Some(id) = numeric_suffix(&node.id) {
                max_id = max_id.max(id);
            }

            for message in &node.messages {
                if let Some(id) = numeric_suffix(&message.id) {
                    max_id = max_id.max(id);
                }
            }
        }
    }

    max_id
}

fn numeric_suffix(value: &str) -> Option<u64> {
    value.rsplit_once('-')?.1.parse().ok()
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

fn text_from_parts(parts: &[Value]) -> Option<String> {
    let text = parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    (!text.is_empty()).then_some(text)
}

fn find_message_mut<'a>(
    conversation: &'a mut ConversationDto,
    message_id: &str,
) -> Option<&'a mut MessageDto> {
    for node in &mut conversation.messages {
        for message in &mut node.messages {
            if message.id == message_id {
                return Some(message);
            }
        }
    }
    None
}

fn append_text_part(parts: &mut Vec<Value>, text: &str) {
    if let Some(part) = parts
        .iter_mut()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
    {
        let existing = part
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        part["text"] = json!(format!("{existing}{text}"));
    } else {
        parts.push(json!({
            "type": "text",
            "text": text,
        }));
    }
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
