use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fs as std_fs, io,
    net::{IpAddr, SocketAddr},
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
    extract::{rejection::JsonRejection, DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use reqwest::header::{HeaderName as ReqwestHeaderName, HeaderValue as ReqwestHeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
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
const STATE_SCHEMA_VERSION: u32 = 6;
const FILE_METADATA_STATE_SCHEMA_VERSION: u32 = 5;
const CUSTOM_REQUEST_CONFIG_STATE_SCHEMA_VERSION: u32 = 4;
const MULTI_MODEL_STATE_SCHEMA_VERSION: u32 = 3;
const PREVIOUS_STATE_SCHEMA_VERSION: u32 = 2;
const LEGACY_STATE_SCHEMA_VERSION: u32 = 1;
const SECRETS_DIR_NAME: &str = "secrets";
const FILES_DIR_NAME: &str = "files";
const FILE_BLOBS_DIR_NAME: &str = "blobs";
const FILE_UPLOAD_MAX_ITEMS: usize = 5;
const FILE_UPLOAD_MAX_BYTES: usize = 20 * 1024 * 1024;
const FILE_UPLOAD_TOTAL_MAX_BYTES: usize = FILE_UPLOAD_MAX_ITEMS * FILE_UPLOAD_MAX_BYTES;
const FILE_DISPLAY_NAME_MAX_CHARS: usize = 160;
const PROVIDER_IMAGE_INPUT_MAX_BYTES: usize = 5 * 1024 * 1024;
#[cfg(not(windows))]
const SECRET_SERVICE_NAME: &str = "RikkaDesk";
const OPENAI_COMPATIBLE_PROVIDER_TYPE: &str = "openai-compatible";
const PROVIDER_SECRET_REF_PREFIX: &str = "rikkadesk:provider:";
const OPENAI_TEST_TIMEOUT_SECS: u64 = 60;
const PROVIDER_IMPORT_EXPORT_VERSION: u32 = 4;
const CUSTOM_PROVIDER_IMPORT_EXPORT_VERSION: u32 = 3;
const MULTI_MODEL_PROVIDER_IMPORT_EXPORT_VERSION: u32 = 2;
const LEGACY_PROVIDER_IMPORT_EXPORT_VERSION: u32 = 1;
const PROVIDER_IMPORT_MAX_ITEMS: usize = 50;
const PROVIDER_IMPORT_MAX_NAME_LEN: usize = 120;
const PROVIDER_IMPORT_MAX_DISPLAY_NAME_LEN: usize = 160;
const PROVIDER_IMPORT_MAX_MODEL_ID_LEN: usize = 200;
const PROVIDER_IMPORT_MAX_BASE_URL_LEN: usize = 512;
const PROVIDER_MODEL_MAX_ITEMS: usize = 50;
const PROVIDER_CUSTOM_HEADER_MAX_ITEMS: usize = 32;
const PROVIDER_CUSTOM_HEADER_MAX_NAME_LEN: usize = 128;
const PROVIDER_CUSTOM_HEADER_MAX_VALUE_LEN: usize = 1024;
const PROVIDER_CUSTOM_BODY_MAX_BYTES: usize = 16 * 1024;
const MODEL_MODALITY_TEXT: &str = "TEXT";
const MODEL_MODALITY_IMAGE: &str = "IMAGE";
const MOCK_ASSISTANT_ID: &str = "mock-assistant";
const MOCK_MODEL_ID: &str = "mock-chat";
const MOCK_PROVIDER_ID: &str = "mock-provider";
const MOCK_WELCOME_CONVERSATION_ID: &str = "mock-welcome";
const MOCK_REPLY_TEXT: &str = "这是 RikkaDesk Mock 后端返回的测试回复。";
const LOCAL_ATTACHMENT_REPLY_TEXT: &str =
    "附件已保存到本地会话。当前 beta 暂不支持将附件发送给模型服务。";
const LOCAL_IMAGE_CAPTURE_CONFIG_REQUIRED_TEXT: &str =
    "图片输入捕获测试需要配置本地 loopback capture provider 和测试密钥。";
const LOCAL_IMAGE_CAPTURE_LOOPBACK_REQUIRED_TEXT: &str =
    "图片输入捕获测试只允许本机 loopback capture provider。附件已保存在本地会话，未发送给模型服务。";
const LOCAL_IMAGE_CAPTURE_CAPABILITY_REQUIRED_TEXT: &str =
    "当前模型未启用图片输入能力。附件已保存在本地会话，未发送给模型服务。";
const LOCAL_IMAGE_CAPTURE_UNSUPPORTED_TEXT: &str =
    "当前图片输入捕获测试只支持一张 PNG、JPEG 或 WEBP 图片。附件已保存在本地会话，未发送给模型服务。";

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

    fn file_blobs_dir(&self) -> PathBuf {
        self.state_dir
            .join(FILES_DIR_NAME)
            .join(FILE_BLOBS_DIR_NAME)
    }

    fn file_blob_path(&self, storage_key: &str) -> Option<PathBuf> {
        is_safe_storage_key(storage_key).then(|| self.file_blobs_dir().join(storage_key))
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
    #[serde(default)]
    files: Vec<ManagedFileMetadata>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedFileMetadata {
    id: u64,
    storage_key: String,
    display_name: String,
    mime: String,
    size_bytes: u64,
    sha256: Option<String>,
    kind: String,
    relative_path: String,
    created_at: String,
    updated_at: String,
    source: String,
    deleted_at: Option<String>,
}

impl ManagedFileMetadata {
    fn url(&self) -> String {
        format!("/api/files/path/{}", self.id)
    }
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
    #[serde(default)]
    models: Vec<DesktopProviderModelConfig>,
    #[serde(default, rename = "model", skip_serializing)]
    legacy_model: Option<DesktopProviderModelConfig>,
    secret_ref: String,
    #[serde(default)]
    custom_headers: Vec<DesktopProviderCustomHeaderConfig>,
    #[serde(default)]
    custom_body: Option<Value>,
}

impl DesktopProviderConfig {
    fn normalize_models(&mut self) {
        if self.models.is_empty() {
            if let Some(model) = self.legacy_model.take() {
                self.models.push(model);
            }
        } else {
            self.legacy_model = None;
        }

        for model in &mut self.models {
            model.normalize_modalities();
        }
    }

    fn primary_model(&self) -> Option<&DesktopProviderModelConfig> {
        self.models.first()
    }

    fn primary_model_id_for_settings(&self) -> Option<&str> {
        self.primary_model().map(|model| model.id.as_str())
    }

    fn model_ids_for_settings(&self) -> impl Iterator<Item = &str> {
        self.models.iter().map(|model| model.id.as_str())
    }

    fn to_settings_provider(&self) -> Value {
        let models = self
            .models
            .iter()
            .map(|model| {
                json!({
                    "id": model.id,
                    "modelId": model.model_id,
                    "displayName": model.display_name,
                    "type": "CHAT",
                    "inputModalities": model.input_modalities.clone(),
                    "outputModalities": model.output_modalities.clone(),
                    "abilities": []
                })
            })
            .collect::<Vec<_>>();

        json!({
            "id": self.id,
            "type": self.provider_type,
            "enabled": self.enabled,
            "name": self.name,
            "baseUrl": self.base_url,
            "secretRef": self.secret_ref,
            "models": models
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProviderModelConfig {
    id: String,
    model_id: String,
    display_name: String,
    #[serde(default = "default_input_modalities")]
    input_modalities: Vec<String>,
    #[serde(default = "default_output_modalities")]
    output_modalities: Vec<String>,
}

impl DesktopProviderModelConfig {
    fn normalize_modalities(&mut self) {
        self.input_modalities = normalize_input_modalities(Some(&self.input_modalities))
            .unwrap_or_else(|_| default_input_modalities());
        self.output_modalities = normalize_output_modalities(Some(&self.output_modalities))
            .unwrap_or_else(|_| default_output_modalities());
    }
}

fn default_input_modalities() -> Vec<String> {
    vec![MODEL_MODALITY_TEXT.to_string()]
}

fn default_output_modalities() -> Vec<String> {
    vec![MODEL_MODALITY_TEXT.to_string()]
}

fn normalize_input_modalities(modalities: Option<&Vec<String>>) -> Result<Vec<String>, String> {
    let Some(modalities) = modalities else {
        return Ok(default_input_modalities());
    };

    if modalities.is_empty() {
        return Err("Model input capabilities must include Text".to_string());
    }

    let mut has_text = false;
    let mut has_image = false;
    for modality in modalities {
        let normalized = modality.trim().to_ascii_uppercase();
        if normalized.is_empty() || normalized.len() > 32 {
            return Err("Model input capability is invalid".to_string());
        }
        match normalized.as_str() {
            MODEL_MODALITY_TEXT => has_text = true,
            MODEL_MODALITY_IMAGE => has_image = true,
            _ => return Err("Model input capability is not supported".to_string()),
        }
    }

    if !has_text {
        return Err("Model input capabilities must include Text".to_string());
    }

    let mut result = vec![MODEL_MODALITY_TEXT.to_string()];
    if has_image {
        result.push(MODEL_MODALITY_IMAGE.to_string());
    }
    Ok(result)
}

fn normalize_output_modalities(modalities: Option<&Vec<String>>) -> Result<Vec<String>, String> {
    let Some(modalities) = modalities else {
        return Ok(default_output_modalities());
    };

    if modalities.is_empty() {
        return Err("Model output capabilities must include Text".to_string());
    }

    for modality in modalities {
        let normalized = modality.trim().to_ascii_uppercase();
        if normalized.is_empty() || normalized.len() > 32 {
            return Err("Model output capability is invalid".to_string());
        }
        if normalized != MODEL_MODALITY_TEXT {
            return Err("Model output capability is not supported".to_string());
        }
    }

    Ok(default_output_modalities())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProviderCustomHeaderConfig {
    name: String,
    value: String,
}

#[derive(Clone)]
enum CustomBodyUpdate {
    Missing,
    Clear,
    Set(Value),
}

impl Default for CustomBodyUpdate {
    fn default() -> Self {
        Self::Missing
    }
}

fn deserialize_custom_body_update<'de, D>(deserializer: D) -> Result<CustomBodyUpdate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(value) => CustomBodyUpdate::Set(value),
        None => CustomBodyUpdate::Clear,
    })
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
    models: Option<Vec<UpsertDesktopProviderModelRequest>>,
    model_id: Option<String>,
    display_name: Option<String>,
    api_key: Option<String>,
    custom_headers: Option<Vec<DesktopProviderCustomHeaderConfig>>,
    #[serde(default, deserialize_with = "deserialize_custom_body_update")]
    custom_body: CustomBodyUpdate,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertDesktopProviderModelRequest {
    id: Option<String>,
    model_id: String,
    display_name: Option<String>,
    input_modalities: Option<Vec<String>>,
    output_modalities: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestDesktopProviderConnectionRequest {
    model_id: Option<String>,
}

struct BuiltDesktopProvider {
    provider: DesktopProviderConfig,
    api_key: Option<String>,
    removed_model_ids: Vec<String>,
    used_models_request: bool,
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
    models: Vec<DesktopProviderModelConfig>,
    secret_ref: String,
    has_secret: bool,
    custom_headers: Vec<DesktopProviderCustomHeaderConfig>,
    custom_body: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadFilesResponse {
    files: Vec<UploadedFileResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadedFileResponse {
    id: u64,
    file_name: String,
    mime: String,
    size_bytes: u64,
    size: u64,
    url: String,
}

impl UploadedFileResponse {
    fn from_metadata(metadata: &ManagedFileMetadata) -> Self {
        Self {
            id: metadata.id,
            file_name: metadata.display_name.clone(),
            mime: metadata.mime.clone(),
            size_bytes: metadata.size_bytes,
            size: metadata.size_bytes,
            url: metadata.url(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedFileResponse {
    id: u64,
    file_name: String,
    mime: String,
    size_bytes: u64,
    size: u64,
    kind: String,
    created_at: String,
    updated_at: String,
    source: String,
    deleted_at: Option<String>,
    url: String,
}

impl ManagedFileResponse {
    fn from_metadata(metadata: &ManagedFileMetadata) -> Self {
        Self {
            id: metadata.id,
            file_name: metadata.display_name.clone(),
            mime: metadata.mime.clone(),
            size_bytes: metadata.size_bytes,
            size: metadata.size_bytes,
            kind: metadata.kind.clone(),
            created_at: metadata.created_at.clone(),
            updated_at: metadata.updated_at.clone(),
            source: metadata.source.clone(),
            deleted_at: metadata.deleted_at.clone(),
            url: metadata.url(),
        }
    }
}

impl DesktopProviderResponse {
    fn from_config(config: &DesktopProviderConfig, has_secret: bool) -> Result<Self, String> {
        let model = config
            .primary_model()
            .cloned()
            .ok_or_else(|| "Provider has no models".to_string())?;

        Ok(Self {
            id: config.id.clone(),
            provider_type: config.provider_type.clone(),
            enabled: config.enabled,
            name: config.name.clone(),
            base_url: config.base_url.clone(),
            model,
            models: config.models.clone(),
            secret_ref: config.secret_ref.clone(),
            has_secret,
            custom_headers: config.custom_headers.clone(),
            custom_body: config.custom_body.clone(),
        })
    }
}

#[derive(Clone)]
struct ValidatedProviderImportItem {
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    models: Vec<ValidatedProviderImportModel>,
    has_secret: bool,
    custom_headers: Vec<DesktopProviderCustomHeaderConfig>,
    custom_body: Option<Value>,
}

#[derive(Clone)]
struct ValidatedProviderImportModel {
    model_id: String,
    display_name: String,
    input_modalities: Vec<String>,
    output_modalities: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportModel {
    model_id: String,
    display_name: String,
    input_modalities: Vec<String>,
    output_modalities: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportModelLegacy {
    model_id: String,
    display_name: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportItem {
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    has_secret: bool,
    models: Vec<ProviderExportModel>,
    custom_headers: Vec<DesktopProviderCustomHeaderConfig>,
    custom_body: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportItemV2 {
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    has_secret: bool,
    models: Vec<ProviderExportModelLegacy>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportItemV3 {
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    has_secret: bool,
    models: Vec<ProviderExportModelLegacy>,
    custom_headers: Vec<DesktopProviderCustomHeaderConfig>,
    custom_body: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportItemV1 {
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    model_id: String,
    display_name: String,
    has_secret: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderExportDocument {
    version: u32,
    app: String,
    exported_at: String,
    providers: Vec<ProviderExportItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct ProviderExportDocumentV1 {
    version: u32,
    app: String,
    exported_at: String,
    providers: Vec<ProviderExportItemV1>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct ProviderExportDocumentV2 {
    version: u32,
    app: String,
    exported_at: String,
    providers: Vec<ProviderExportItemV2>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct ProviderExportDocumentV3 {
    version: u32,
    app: String,
    exported_at: String,
    providers: Vec<ProviderExportItemV3>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderImportPreviewResponse {
    status: &'static str,
    importable_count: usize,
    notice: &'static str,
    providers: Vec<ProviderImportPreviewItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderImportPreviewItem {
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    has_secret: bool,
    models: Vec<ProviderExportModel>,
    advanced_config: ProviderImportAdvancedSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderImportAdvancedSummary {
    custom_header_count: usize,
    custom_body_present: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderImportConfirmResponse {
    status: &'static str,
    imported_count: usize,
    providers: Vec<ProviderImportConfirmItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderImportConfirmItem {
    id: String,
    #[serde(rename = "type")]
    provider_type: String,
    enabled: bool,
    name: String,
    base_url: String,
    has_secret: bool,
    models: Vec<ProviderImportConfirmModel>,
    advanced_config: ProviderImportAdvancedSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderImportConfirmModel {
    id: String,
    model_id: String,
    display_name: String,
    input_modalities: Vec<String>,
    output_modalities: Vec<String>,
}

struct OpenAiChatConfig {
    base_url: String,
    model_id: String,
    api_key: String,
    custom_headers: Vec<DesktopProviderCustomHeaderConfig>,
    custom_body: Option<Value>,
    input_modalities: Vec<String>,
}

#[derive(Clone, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}

struct ManagedImageProviderInput {
    file_id: u64,
    mime: String,
    bytes: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Clone)]
enum OpenAiCompatibleMessageContent {
    Text(String),
    Parts(Vec<OpenAiCompatibleContentPart>),
}

#[allow(dead_code)]
#[derive(Clone)]
enum OpenAiCompatibleContentPart {
    Text(String),
    ImageUrl {
        data_url: String,
        detail: OpenAiImageDetail,
        file_id: u64,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum OpenAiImageDetail {
    Auto,
    Low,
    High,
}

#[allow(dead_code)]
#[derive(Clone)]
struct OpenAiCompatibleChatMessage {
    role: String,
    content: OpenAiCompatibleMessageContent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenAiRequestKind {
    TestConnection,
    StreamingChat,
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
    image_input_confirmed: Option<bool>,
    image_input_mode: Option<String>,
}

#[derive(Deserialize)]
struct UpdateConversationTitleRequest {
    title: String,
}

#[derive(Deserialize)]
struct EditMessageRequest {
    parts: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegenerateRequest {
    message_id: Option<String>,
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
#[serde(rename_all = "camelCase")]
struct UpdateFavoriteModelsRequest {
    model_ids: Vec<String>,
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
    files: RwLock<Vec<ManagedFileMetadata>>,
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
            .max(max_persisted_file_id(&persisted.files))
            .max(1);
        let (settings_tx, _) = broadcast::channel(64);
        let (list_tx, _) = broadcast::channel(64);

        Self {
            persistence,
            secret_store,
            http_client: reqwest::Client::builder()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            settings: RwLock::new(persisted.settings),
            conversations: RwLock::new(persisted.conversations),
            providers: RwLock::new(persisted.providers),
            files: RwLock::new(persisted.files),
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

    fn next_file_id(&self) -> u64 {
        self.id_seq.fetch_add(1, Ordering::Relaxed) + 1
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
        .route("/api/files/upload", post(upload_files))
        .route("/api/files/path/{id}", get(file_path))
        .route("/api/files/{id}", get(file_metadata).delete(delete_file))
        .route(
            "/api/desktop/providers",
            get(desktop_providers).post(upsert_desktop_provider),
        )
        .route(
            "/api/desktop/providers/export",
            get(export_desktop_providers),
        )
        .route(
            "/api/desktop/providers/import/preview",
            post(preview_desktop_provider_import),
        )
        .route(
            "/api/desktop/providers/import/confirm",
            post(confirm_desktop_provider_import),
        )
        .route(
            "/api/desktop/providers/{id}",
            delete(delete_desktop_provider),
        )
        .route(
            "/api/desktop/providers/{id}/test",
            post(test_desktop_provider_connection),
        )
        .route(
            "/api/desktop/providers/{id}/secret",
            post(update_desktop_provider_secret).delete(delete_desktop_provider_secret),
        )
        .route("/api/conversations/{id}", get(conversation_detail))
        .route("/api/conversations/{id}/stream", get(conversation_stream))
        .route("/api/conversations/{id}/messages", post(send_message))
        .route(
            "/api/conversations/{id}/messages/{message_id}/edit",
            post(edit_message),
        )
        .route(
            "/api/conversations/{id}/messages/{message_id}",
            delete(delete_message),
        )
        .route(
            "/api/conversations/{id}/regenerate",
            post(regenerate_message),
        )
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
        .route(
            "/api/settings/favorite-models",
            post(update_favorite_models),
        )
        .fallback(not_implemented)
        .layer(DefaultBodyLimit::max(
            FILE_UPLOAD_TOTAL_MAX_BYTES + 1024 * 1024,
        ))
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
            normalize_desktop_providers(&mut persisted.providers);
            sync_settings_with_desktop_providers(&mut persisted.settings, &persisted.providers);
            ensure_current_model_exists(&mut persisted.settings);
            persisted
        }
        Ok(persisted) if persisted.schema_version == FILE_METADATA_STATE_SCHEMA_VERSION => {
            let mut migrated = migrate_v5_to_v6(persisted);
            sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
            ensure_current_model_exists(&mut migrated.settings);
            if let Err(error) = persistence.save(&migrated).await {
                eprintln!("RikkaDesk mock API failed to save migrated state: {error}");
            }
            migrated
        }
        Ok(persisted) if persisted.schema_version == CUSTOM_REQUEST_CONFIG_STATE_SCHEMA_VERSION => {
            let mut migrated = migrate_v4_to_v6(persisted);
            sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
            ensure_current_model_exists(&mut migrated.settings);
            if let Err(error) = persistence.save(&migrated).await {
                eprintln!("RikkaDesk mock API failed to save migrated state: {error}");
            }
            migrated
        }
        Ok(persisted) if persisted.schema_version == MULTI_MODEL_STATE_SCHEMA_VERSION => {
            let mut migrated = migrate_v3_to_v6(persisted);
            sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
            ensure_current_model_exists(&mut migrated.settings);
            if let Err(error) = persistence.save(&migrated).await {
                eprintln!("RikkaDesk mock API failed to save migrated state: {error}");
            }
            migrated
        }
        Ok(persisted) if persisted.schema_version == PREVIOUS_STATE_SCHEMA_VERSION => {
            let mut migrated = migrate_v2_to_v6(persisted);
            sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
            ensure_current_model_exists(&mut migrated.settings);
            if let Err(error) = persistence.save(&migrated).await {
                eprintln!("RikkaDesk mock API failed to save migrated state: {error}");
            }
            migrated
        }
        Ok(persisted) if persisted.schema_version == LEGACY_STATE_SCHEMA_VERSION => {
            let mut migrated = migrate_v1_to_v6(persisted);
            sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
            ensure_current_model_exists(&mut migrated.settings);
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
    let files = state.files.read().await.clone();
    let id_seq = state
        .id_seq
        .load(Ordering::Relaxed)
        .max(max_persisted_id_seq(&conversations))
        .max(max_persisted_file_id(&files))
        .max(1);

    let persisted = PersistedMockState {
        schema_version: STATE_SCHEMA_VERSION,
        saved_at: now_millis(),
        id_seq,
        settings,
        conversations,
        providers,
        files,
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

async fn export_desktop_providers(State(state): State<Arc<MockApiState>>) -> impl IntoResponse {
    match provider_export_items(&state).await {
        Ok(providers) => Json(ProviderExportDocument {
            version: PROVIDER_IMPORT_EXPORT_VERSION,
            app: "RikkaDesk".to_string(),
            exported_at: now_iso(),
            providers,
        })
        .into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn preview_desktop_provider_import(
    payload: Result<Json<Value>, JsonRejection>,
) -> impl IntoResponse {
    let document = match import_document_from_payload(payload) {
        Ok(document) => document,
        Err(response) => return response,
    };
    let providers = match validate_provider_import_document(&document) {
        Ok(providers) => providers,
        Err(response) => return response,
    };
    let providers = providers
        .iter()
        .map(provider_import_preview_item)
        .collect::<Vec<_>>();

    Json(ProviderImportPreviewResponse {
        status: "ok",
        importable_count: providers.len(),
        notice: "hasSecret only indicates the source provider had a secret; imported providers will not restore API keys.",
        providers,
    })
    .into_response()
}

async fn confirm_desktop_provider_import(
    State(state): State<Arc<MockApiState>>,
    payload: Result<Json<Value>, JsonRejection>,
) -> impl IntoResponse {
    let document = match import_document_from_payload(payload) {
        Ok(document) => document,
        Err(response) => return response,
    };
    let providers = match validate_provider_import_document(&document) {
        Ok(providers) => providers,
        Err(response) => return response,
    };

    let mut imported_configs = Vec::with_capacity(providers.len());
    let mut imported = Vec::with_capacity(providers.len());
    for provider in providers {
        let id = state.next_id("desktop-provider");
        let models = provider
            .models
            .iter()
            .map(|model| DesktopProviderModelConfig {
                id: state.next_id("desktop-model"),
                model_id: model.model_id.clone(),
                display_name: model.display_name.clone(),
                input_modalities: model.input_modalities.clone(),
                output_modalities: model.output_modalities.clone(),
            })
            .collect::<Vec<_>>();
        let imported_models = models
            .iter()
            .map(|model| ProviderImportConfirmModel {
                id: model.id.clone(),
                model_id: model.model_id.clone(),
                display_name: model.display_name.clone(),
                input_modalities: model.input_modalities.clone(),
                output_modalities: model.output_modalities.clone(),
            })
            .collect::<Vec<_>>();
        let config = DesktopProviderConfig {
            secret_ref: secret_ref_for_provider(&id),
            id: id.clone(),
            provider_type: provider.provider_type.clone(),
            enabled: provider.enabled,
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            models,
            legacy_model: None,
            custom_headers: provider.custom_headers.clone(),
            custom_body: provider.custom_body.clone(),
        };

        let advanced_config = provider_import_advanced_summary(&provider);
        imported.push(ProviderImportConfirmItem {
            id,
            provider_type: provider.provider_type,
            enabled: provider.enabled,
            name: provider.name,
            base_url: provider.base_url,
            has_secret: false,
            models: imported_models,
            advanced_config,
        });
        imported_configs.push(config);
    }

    if !imported_configs.is_empty() {
        {
            let mut providers = state.providers.write().await;
            providers.extend(imported_configs);
        }

        {
            let providers = state.providers.read().await;
            let mut settings = state.settings.write().await;
            sync_settings_with_desktop_providers(&mut settings, &providers);
        }

        persist_mock_state(&state).await;
        broadcast_settings_update(&state).await;
    }

    Json(ProviderImportConfirmResponse {
        status: "ok",
        imported_count: imported.len(),
        providers: imported,
    })
    .into_response()
}

async fn upsert_desktop_provider(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpsertDesktopProviderRequest>,
) -> impl IntoResponse {
    match build_desktop_provider(&state, payload).await {
        Ok(BuiltDesktopProvider {
            provider,
            api_key,
            removed_model_ids,
            used_models_request,
        }) => {
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
                if !removed_model_ids.is_empty() {
                    remove_models_from_favorites(
                        &mut settings,
                        removed_model_ids.iter().map(String::as_str),
                    );
                }
                if !used_models_request {
                    if let Some(model_id) = provider.primary_model_id_for_settings() {
                        set_current_model_in_settings(&mut settings, model_id);
                    }
                }
                ensure_current_model_exists(&mut settings);
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

            match DesktopProviderResponse::from_config(&provider, has_secret) {
                Ok(response) => Json(response).into_response(),
                Err(error) => internal_error_response(error),
            }
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

async fn delete_desktop_provider(
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

    {
        let mut providers = state.providers.write().await;
        providers.retain(|item| item.id != provider.id);
    }

    {
        let providers = state.providers.read().await;
        let mut settings = state.settings.write().await;
        sync_settings_with_desktop_providers(&mut settings, &providers);
        remove_models_from_favorites(&mut settings, provider.model_ids_for_settings());
        ensure_current_model_exists(&mut settings);
    }

    persist_mock_state(&state).await;
    broadcast_settings_update(&state).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn test_desktop_provider_connection(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
    payload: Option<Json<TestDesktopProviderConnectionRequest>>,
) -> impl IntoResponse {
    let selected_model_id = payload
        .as_ref()
        .and_then(|Json(payload)| payload.model_id.as_deref())
        .map(str::trim)
        .filter(|model_id| !model_id.is_empty());

    match resolve_openai_chat_config_for_provider(&state, &id, selected_model_id).await {
        Ok(Some(config)) => match test_openai_compatible_chat_connection(&state, &config).await {
            Ok(()) => Json(json!({ "ok": true })).into_response(),
            Err(error) => Json(json!({ "ok": false, "error": error })).into_response(),
        },
        Ok(None) => Json(json!({
            "ok": false,
            "error": "Provider is missing Base URL, Model ID, or API Key."
        }))
        .into_response(),
        Err(error) => Json(json!({ "ok": false, "error": error })).into_response(),
    }
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
    let user_has_non_text_parts = has_non_text_parts(&payload.parts);
    let request_parts = payload.parts.clone();
    let capture_intent = is_capture_local_image_intent(&payload);

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

    if user_has_non_text_parts {
        if capture_intent {
            match start_local_image_capture_prototype(
                &state,
                &id,
                &assistant_id,
                &model_id,
                user_text.clone(),
                &request_parts,
                now,
            )
            .await
            {
                Ok(true) => return Json(json!({ "status": "accepted" })),
                Ok(false) => {}
                Err(error) => {
                    append_assistant_reply(&state, &id, &assistant_id, &model_id, error, now).await;
                    return Json(json!({ "status": "accepted" }));
                }
            }
        }

        append_assistant_reply(
            &state,
            &id,
            &assistant_id,
            &model_id,
            LOCAL_ATTACHMENT_REPLY_TEXT.to_string(),
            now,
        )
        .await;
        return Json(json!({ "status": "accepted" }));
    }

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
    spawn_openai_stream_generation(state.clone(), id, assistant_message_id, config, messages);

    Json(json!({ "status": "accepted" }))
}

fn is_capture_local_image_intent(payload: &SendMessageRequest) -> bool {
    payload.image_input_confirmed == Some(true)
        && payload.image_input_mode.as_deref() == Some("capture-local")
}

async fn start_local_image_capture_prototype(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_id: &str,
    model_id: &str,
    user_text: Option<String>,
    parts: &[Value],
    now: u64,
) -> Result<bool, String> {
    let image_file_ids = provider_bound_image_file_ids(parts)?;
    let Some(file_id) = image_file_ids.first().copied() else {
        return Ok(false);
    };

    let config = resolve_openai_chat_config(state, model_id)
        .await
        .map_err(|_| LOCAL_IMAGE_CAPTURE_CONFIG_REQUIRED_TEXT.to_string())?
        .ok_or_else(|| LOCAL_IMAGE_CAPTURE_CONFIG_REQUIRED_TEXT.to_string())?;

    if !is_loopback_provider_base_url(&config.base_url) {
        return Err(LOCAL_IMAGE_CAPTURE_LOOPBACK_REQUIRED_TEXT.to_string());
    }
    if !config
        .input_modalities
        .iter()
        .any(|modality| modality == MODEL_MODALITY_IMAGE)
    {
        return Err(LOCAL_IMAGE_CAPTURE_CAPABILITY_REQUIRED_TEXT.to_string());
    }

    let image = managed_file_for_provider_image_input(state, file_id).await?;
    let messages = openai_vision_messages_for_current_turn(user_text, image)?;
    let assistant_message_id =
        append_empty_streaming_assistant_reply(state, conversation_id, assistant_id, model_id, now)
            .await;
    start_generation(state, conversation_id).await;
    spawn_openai_vision_capture_generation(
        state.clone(),
        conversation_id.to_string(),
        assistant_message_id,
        config,
        messages,
    );

    Ok(true)
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

async fn edit_message(
    State(state): State<Arc<MockApiState>>,
    Path((id, message_id)): Path<(String, String)>,
    Json(payload): Json<EditMessageRequest>,
) -> impl IntoResponse {
    let Some(text) = first_text_part(&payload.parts) else {
        return bad_request_response("Phase 6A currently supports text-only message editing.");
    };

    let updated = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&id) else {
            return not_found_response("Conversation not found");
        };
        let Some(message) = find_message_mut(conversation, &message_id) else {
            return not_found_response("Message not found");
        };

        if message.role != "USER" && message.role != "ASSISTANT" {
            return bad_request_response("Only user and assistant text messages can be edited.");
        }

        message.parts = vec![json!({
            "type": "text",
            "text": text,
        })];
        conversation.update_at = now_millis();
        conversation.clone()
    };

    persist_mock_state(&state).await;
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn delete_message(
    State(state): State<Arc<MockApiState>>,
    Path((id, message_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let updated = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&id) else {
            return not_found_response("Conversation not found");
        };

        let mut removed = false;
        for node in &mut conversation.messages {
            let before = node.messages.len();
            node.messages.retain(|message| message.id != message_id);
            if node.messages.len() != before {
                removed = true;
                if !node.messages.is_empty() && node.select_index >= node.messages.len() {
                    node.select_index = node.messages.len() - 1;
                }
            }
        }

        if !removed {
            return not_found_response("Message not found");
        }

        conversation
            .messages
            .retain(|node| !node.messages.is_empty());
        conversation.update_at = now_millis();
        conversation.clone()
    };

    persist_mock_state(&state).await;
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "deleted" })).into_response()
}

async fn regenerate_message(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
    Json(payload): Json<RegenerateRequest>,
) -> impl IntoResponse {
    let now = now_millis();
    let assistant_id = current_assistant_id(&state).await;
    let model_id = current_model_id(&state, &assistant_id).await;

    let (prepared, last_user_has_non_text_parts) = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&id) else {
            return not_found_response("Conversation not found");
        };

        let Some(target_index) =
            find_regeneratable_node_index(conversation, payload.message_id.as_deref())
        else {
            return bad_request_response(
                "Phase 6A currently supports regenerating only the latest text turn.",
            );
        };

        let Some(target_message) = selected_message(&conversation.messages[target_index]) else {
            return bad_request_response(
                "Phase 6A currently supports regenerating only the latest text turn.",
            );
        };

        match target_message.role.as_str() {
            "ASSISTANT" => {
                conversation.messages.truncate(target_index);
            }
            "USER" => {
                conversation.messages.truncate(target_index + 1);
            }
            _ => {
                return bad_request_response(
                    "Only user and assistant text messages can be regenerated.",
                );
            }
        }

        let Some(last_user_message) = conversation
            .messages
            .iter()
            .rev()
            .filter_map(selected_message)
            .find(|message| message.role == "USER")
        else {
            return bad_request_response("No user text message is available to regenerate from.");
        };

        let last_user_has_non_text_parts = has_non_text_parts(&last_user_message.parts);
        if !last_user_has_non_text_parts && text_from_parts(&last_user_message.parts).is_none() {
            return bad_request_response(
                "Phase 6A currently supports regenerating text-only chat.",
            );
        }

        conversation.update_at = now_millis();
        conversation.is_generating = true;
        (conversation.clone(), last_user_has_non_text_parts)
    };

    persist_mock_state(&state).await;
    broadcast_conversation_snapshot(&state, &prepared).await;
    broadcast_list_invalidate(&state).await;

    if last_user_has_non_text_parts {
        append_assistant_reply(
            &state,
            &id,
            &assistant_id,
            &model_id,
            LOCAL_ATTACHMENT_REPLY_TEXT.to_string(),
            now,
        )
        .await;
        return Json(json!({ "status": "accepted" })).into_response();
    }

    let real_chat_config = match resolve_openai_chat_config(&state, &model_id).await {
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
            return Json(json!({ "status": "accepted" })).into_response();
        }
    };

    let Some(config) = real_chat_config else {
        append_assistant_reply(
            &state,
            &id,
            &assistant_id,
            &model_id,
            MOCK_REPLY_TEXT.to_string(),
            now,
        )
        .await;
        return Json(json!({ "status": "accepted" })).into_response();
    };

    let messages = openai_messages_from_conversation(&prepared);
    if messages.is_empty() {
        append_assistant_reply(
            &state,
            &id,
            &assistant_id,
            &model_id,
            "Phase 6A currently supports regenerating text-only chat.".to_string(),
            now,
        )
        .await;
        return Json(json!({ "status": "accepted" })).into_response();
    }

    let assistant_message_id =
        append_empty_streaming_assistant_reply(&state, &id, &assistant_id, &model_id, now).await;
    start_generation(&state, &id).await;
    spawn_openai_stream_generation(state.clone(), id, assistant_message_id, config, messages);

    Json(json!({ "status": "accepted" })).into_response()
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

async fn update_favorite_models(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpdateFavoriteModelsRequest>,
) -> impl IntoResponse {
    let mut seen = HashSet::new();
    let model_ids: Vec<String> = payload
        .model_ids
        .into_iter()
        .map(|model_id| model_id.trim().to_string())
        .filter(|model_id| !model_id.is_empty())
        .filter(|model_id| seen.insert(model_id.clone()))
        .collect();

    {
        let mut settings = state.settings.write().await;
        settings["favoriteModels"] = json!(model_ids);
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

struct PreparedFileUpload {
    metadata: ManagedFileMetadata,
    bytes: Vec<u8>,
}

async fn upload_files(
    State(state): State<Arc<MockApiState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut prepared = Vec::new();
    let mut total_bytes = 0usize;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(_) => return bad_request_response("Invalid file upload"),
        };

        if field.name() != Some("files") {
            continue;
        }

        if prepared.len() >= FILE_UPLOAD_MAX_ITEMS {
            return bad_request_response("Too many files");
        }

        let display_name = sanitize_upload_file_name(field.file_name());
        let bytes = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => return bad_request_response("Invalid file upload"),
        };
        let size = bytes.len();

        if size > FILE_UPLOAD_MAX_BYTES {
            return bad_request_response("File is too large");
        }

        total_bytes = match total_bytes.checked_add(size) {
            Some(total_bytes) => total_bytes,
            None => return bad_request_response("Upload is too large"),
        };
        if total_bytes > FILE_UPLOAD_TOTAL_MAX_BYTES {
            return bad_request_response("Upload is too large");
        }

        let mime = match detect_safe_upload_mime(&bytes, &display_name) {
            Ok(mime) => mime,
            Err(message) => return bad_request_response(message),
        };
        let kind = upload_kind_for_mime(mime);
        let id = state.next_file_id();
        let storage_key = storage_key_for_file(id);
        let now = now_iso();
        let metadata = ManagedFileMetadata {
            id,
            storage_key: storage_key.clone(),
            display_name,
            mime: mime.to_string(),
            size_bytes: size as u64,
            sha256: None,
            kind: kind.to_string(),
            relative_path: format!("{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{storage_key}"),
            created_at: now.clone(),
            updated_at: now,
            source: "upload".to_string(),
            deleted_at: None,
        };

        prepared.push(PreparedFileUpload {
            metadata,
            bytes: bytes.to_vec(),
        });
    }

    if prepared.is_empty() {
        return bad_request_response("No files were uploaded");
    }

    if save_prepared_file_uploads(&state, &prepared).await.is_err() {
        return internal_error_response("File upload failed");
    }

    let uploaded = {
        let mut files = state.files.write().await;
        let uploaded = prepared
            .iter()
            .map(|upload| UploadedFileResponse::from_metadata(&upload.metadata))
            .collect::<Vec<_>>();
        files.extend(prepared.into_iter().map(|upload| upload.metadata));
        uploaded
    };

    persist_mock_state(&state).await;

    Json(UploadFilesResponse { files: uploaded }).into_response()
}

async fn file_metadata(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let metadata = {
        let files = state.files.read().await;
        files
            .iter()
            .find(|file| file.id == id && file.deleted_at.is_none())
            .cloned()
    };

    match metadata {
        Some(metadata) => Json(ManagedFileResponse::from_metadata(&metadata)).into_response(),
        None => not_found_response("File not found"),
    }
}

async fn file_path(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let metadata = {
        let files = state.files.read().await;
        files
            .iter()
            .find(|file| file.id == id && file.deleted_at.is_none())
            .cloned()
    };

    let Some(metadata) = metadata else {
        return not_found_response("File not found");
    };

    let Some(path) = state.persistence.file_blob_path(&metadata.storage_key) else {
        return internal_error_response("File is unavailable");
    };

    let bytes = match fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return not_found_response("File not found")
        }
        Err(_) => return internal_error_response("File is unavailable"),
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&metadata.mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if !metadata.mime.starts_with("image/") {
        let file_name = safe_header_file_name(&metadata.display_name);
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\"")) {
            headers.insert(header::CONTENT_DISPOSITION, value);
        }
    }

    (headers, bytes).into_response()
}

async fn delete_file(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let metadata = {
        let files = state.files.read().await;
        files
            .iter()
            .find(|file| file.id == id && file.deleted_at.is_none())
            .cloned()
    };

    let Some(metadata) = metadata else {
        return not_found_response("File not found");
    };

    if let Some(path) = state.persistence.file_blob_path(&metadata.storage_key) {
        match fs::remove_file(path).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return internal_error_response("File delete failed"),
        }
    }

    {
        let now = now_iso();
        let mut files = state.files.write().await;
        if let Some(file) = files.iter_mut().find(|file| file.id == id) {
            file.deleted_at = Some(now.clone());
            file.updated_at = now;
        }
    }

    persist_mock_state(&state).await;

    Json(json!({ "status": "deleted" })).into_response()
}

async fn save_prepared_file_uploads(
    state: &Arc<MockApiState>,
    uploads: &[PreparedFileUpload],
) -> Result<(), ()> {
    let blobs_dir = state.persistence.file_blobs_dir();
    fs::create_dir_all(&blobs_dir).await.map_err(|_| ())?;

    let mut written = Vec::new();
    for upload in uploads {
        let Some(path) = state
            .persistence
            .file_blob_path(&upload.metadata.storage_key)
        else {
            cleanup_uploaded_blobs(written).await;
            return Err(());
        };
        let tmp_path = path.with_extension("tmp");
        if fs::write(&tmp_path, &upload.bytes).await.is_err() {
            let _ = fs::remove_file(&tmp_path).await;
            cleanup_uploaded_blobs(written).await;
            return Err(());
        }
        if fs::rename(&tmp_path, &path).await.is_err() {
            let _ = fs::remove_file(&tmp_path).await;
            cleanup_uploaded_blobs(written).await;
            return Err(());
        }
        written.push(path);
    }

    Ok(())
}

async fn cleanup_uploaded_blobs(paths: Vec<PathBuf>) {
    for path in paths {
        let _ = fs::remove_file(path).await;
    }
}

async fn managed_file_for_provider_image_input(
    state: &Arc<MockApiState>,
    file_id: u64,
) -> Result<ManagedImageProviderInput, String> {
    let metadata = {
        let files = state.files.read().await;
        files
            .iter()
            .find(|file| file.id == file_id && file.deleted_at.is_none())
            .cloned()
    }
    .ok_or_else(|| "Image attachment is unavailable".to_string())?;

    if !is_safe_storage_key(&metadata.storage_key) {
        return Err("Image attachment is unavailable".to_string());
    }
    if !is_provider_image_mime(&metadata.mime) {
        return Err(LOCAL_IMAGE_CAPTURE_UNSUPPORTED_TEXT.to_string());
    }
    if metadata.size_bytes > PROVIDER_IMAGE_INPUT_MAX_BYTES as u64 {
        return Err("Image attachment is too large for capture testing".to_string());
    }

    let Some(path) = state.persistence.file_blob_path(&metadata.storage_key) else {
        return Err("Image attachment is unavailable".to_string());
    };
    let bytes = fs::read(path)
        .await
        .map_err(|_| "Image attachment is unavailable".to_string())?;
    if bytes.len() > PROVIDER_IMAGE_INPUT_MAX_BYTES {
        return Err("Image attachment is too large for capture testing".to_string());
    }

    let detected = detect_safe_upload_mime(&bytes, &metadata.display_name)
        .map_err(|_| "Image attachment type is not supported for capture testing".to_string())?;
    if detected != metadata.mime || !is_provider_image_mime(detected) {
        return Err("Image attachment type is not supported for capture testing".to_string());
    }

    Ok(ManagedImageProviderInput {
        file_id,
        mime: metadata.mime,
        bytes,
    })
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
                .and_then(|has_secret| DesktopProviderResponse::from_config(provider, has_secret))
        })
        .collect()
}

async fn provider_export_items(
    state: &Arc<MockApiState>,
) -> Result<Vec<ProviderExportItem>, String> {
    let providers = state.providers.read().await.clone();
    providers
        .iter()
        .map(|provider| {
            state
                .secret_store
                .has_secret(&provider.secret_ref)
                .and_then(|has_secret| {
                    if provider.models.is_empty() {
                        return Err("Provider has no models".to_string());
                    }

                    let custom_headers = validate_custom_headers(&provider.custom_headers)
                        .map_err(|_| {
                            "Provider custom request config is not safe to export".to_string()
                        })?;
                    let custom_body = provider
                        .custom_body
                        .as_ref()
                        .map(validate_custom_body_value)
                        .transpose()
                        .map_err(|_| {
                            "Provider custom request config is not safe to export".to_string()
                        })?;

                    Ok(ProviderExportItem {
                        provider_type: provider.provider_type.clone(),
                        enabled: provider.enabled,
                        name: provider.name.clone(),
                        base_url: provider.base_url.clone(),
                        has_secret,
                        models: provider
                            .models
                            .iter()
                            .map(|model| ProviderExportModel {
                                model_id: model.model_id.clone(),
                                display_name: model.display_name.clone(),
                                input_modalities: model.input_modalities.clone(),
                                output_modalities: model.output_modalities.clone(),
                            })
                            .collect(),
                        custom_headers,
                        custom_body,
                    })
                })
        })
        .collect()
}

fn import_document_from_payload(
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Value, Response> {
    payload
        .map(|Json(document)| document)
        .map_err(|_| bad_request_response("Invalid provider import document"))
}

fn validate_provider_import_document(
    document: &Value,
) -> Result<Vec<ValidatedProviderImportItem>, Response> {
    let version = document
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad_request_response("Provider import document version is required"))?;

    match version as u32 {
        LEGACY_PROVIDER_IMPORT_EXPORT_VERSION => {
            let document = serde_json::from_value::<ProviderExportDocumentV1>(document.clone())
                .map_err(|_| bad_request_response("Invalid provider import document"))?;
            validate_provider_import_document_v1(&document)
        }
        MULTI_MODEL_PROVIDER_IMPORT_EXPORT_VERSION => {
            let document = serde_json::from_value::<ProviderExportDocumentV2>(document.clone())
                .map_err(|_| bad_request_response("Invalid provider import document"))?;
            validate_provider_import_document_v2(&document)
        }
        CUSTOM_PROVIDER_IMPORT_EXPORT_VERSION => {
            let document = serde_json::from_value::<ProviderExportDocumentV3>(document.clone())
                .map_err(|_| bad_request_response("Invalid provider import document"))?;
            validate_provider_import_document_v3(&document)
        }
        PROVIDER_IMPORT_EXPORT_VERSION => {
            let document = serde_json::from_value::<ProviderExportDocument>(document.clone())
                .map_err(|_| bad_request_response("Invalid provider import document"))?;
            validate_provider_import_document_v4(&document)
        }
        _ => Err(bad_request_response(
            "Unsupported provider import document version",
        )),
    }
}

fn validate_provider_import_document_v1(
    document: &ProviderExportDocumentV1,
) -> Result<Vec<ValidatedProviderImportItem>, Response> {
    if document.version != LEGACY_PROVIDER_IMPORT_EXPORT_VERSION {
        return Err(bad_request_response(
            "Unsupported provider import document version",
        ));
    }

    validate_provider_import_count(document.providers.len())?;

    document
        .providers
        .iter()
        .enumerate()
        .map(|(index, provider)| validate_provider_import_item_v1(index + 1, provider))
        .collect()
}

fn validate_provider_import_document_v2(
    document: &ProviderExportDocumentV2,
) -> Result<Vec<ValidatedProviderImportItem>, Response> {
    if document.version != MULTI_MODEL_PROVIDER_IMPORT_EXPORT_VERSION {
        return Err(bad_request_response(
            "Unsupported provider import document version",
        ));
    }

    validate_provider_import_count(document.providers.len())?;

    document
        .providers
        .iter()
        .enumerate()
        .map(|(index, provider)| validate_provider_import_item_v2(index + 1, provider))
        .collect()
}

fn validate_provider_import_document_v3(
    document: &ProviderExportDocumentV3,
) -> Result<Vec<ValidatedProviderImportItem>, Response> {
    if document.version != CUSTOM_PROVIDER_IMPORT_EXPORT_VERSION {
        return Err(bad_request_response(
            "Unsupported provider import document version",
        ));
    }

    validate_provider_import_count(document.providers.len())?;

    document
        .providers
        .iter()
        .enumerate()
        .map(|(index, provider)| validate_provider_import_item_v3(index + 1, provider))
        .collect()
}

fn validate_provider_import_document_v4(
    document: &ProviderExportDocument,
) -> Result<Vec<ValidatedProviderImportItem>, Response> {
    if document.version != PROVIDER_IMPORT_EXPORT_VERSION {
        return Err(bad_request_response(
            "Unsupported provider import document version",
        ));
    }

    validate_provider_import_count(document.providers.len())?;

    document
        .providers
        .iter()
        .enumerate()
        .map(|(index, provider)| validate_provider_import_item_v4(index + 1, provider))
        .collect()
}

fn validate_provider_import_count(count: usize) -> Result<(), Response> {
    if count > PROVIDER_IMPORT_MAX_ITEMS {
        return Err(bad_request_response(
            "Provider import document contains too many providers",
        ));
    }
    Ok(())
}

fn validate_provider_import_item_v1(
    position: usize,
    provider: &ProviderExportItemV1,
) -> Result<ValidatedProviderImportItem, Response> {
    let models = vec![ProviderExportModel {
        model_id: provider.model_id.clone(),
        display_name: provider.display_name.clone(),
        input_modalities: default_input_modalities(),
        output_modalities: default_output_modalities(),
    }];
    validate_provider_import_item(
        position,
        &provider.provider_type,
        provider.enabled,
        &provider.name,
        &provider.base_url,
        provider.has_secret,
        &models,
        &[],
        None,
    )
}

fn validate_provider_import_item_v2(
    position: usize,
    provider: &ProviderExportItemV2,
) -> Result<ValidatedProviderImportItem, Response> {
    let models = legacy_provider_export_models(&provider.models);
    validate_provider_import_item(
        position,
        &provider.provider_type,
        provider.enabled,
        &provider.name,
        &provider.base_url,
        provider.has_secret,
        &models,
        &[],
        None,
    )
}

fn validate_provider_import_item_v3(
    position: usize,
    provider: &ProviderExportItemV3,
) -> Result<ValidatedProviderImportItem, Response> {
    let models = legacy_provider_export_models(&provider.models);
    validate_provider_import_item(
        position,
        &provider.provider_type,
        provider.enabled,
        &provider.name,
        &provider.base_url,
        provider.has_secret,
        &models,
        &provider.custom_headers,
        provider.custom_body.as_ref(),
    )
}

fn validate_provider_import_item_v4(
    position: usize,
    provider: &ProviderExportItem,
) -> Result<ValidatedProviderImportItem, Response> {
    validate_provider_import_item(
        position,
        &provider.provider_type,
        provider.enabled,
        &provider.name,
        &provider.base_url,
        provider.has_secret,
        &provider.models,
        &provider.custom_headers,
        provider.custom_body.as_ref(),
    )
}

fn legacy_provider_export_models(models: &[ProviderExportModelLegacy]) -> Vec<ProviderExportModel> {
    models
        .iter()
        .map(|model| ProviderExportModel {
            model_id: model.model_id.clone(),
            display_name: model.display_name.clone(),
            input_modalities: default_input_modalities(),
            output_modalities: default_output_modalities(),
        })
        .collect()
}

fn validate_provider_import_item(
    position: usize,
    provider_type: &str,
    enabled: bool,
    name: &str,
    base_url: &str,
    has_secret: bool,
    models: &[ProviderExportModel],
    custom_headers: &[DesktopProviderCustomHeaderConfig],
    custom_body: Option<&Value>,
) -> Result<ValidatedProviderImportItem, Response> {
    let provider_type = provider_type.trim();
    if provider_type != OPENAI_COMPATIBLE_PROVIDER_TYPE {
        return Err(bad_request_response(&format!(
            "Provider {position} type is not supported"
        )));
    }

    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err(bad_request_response(&format!(
            "Provider {position} baseUrl is required"
        )));
    }
    if field_is_too_long(base_url, PROVIDER_IMPORT_MAX_BASE_URL_LEN) {
        return Err(bad_request_response(&format!(
            "Provider {position} baseUrl is too long"
        )));
    }
    if !is_supported_provider_base_url(base_url) {
        return Err(bad_request_response(&format!(
            "Provider {position} baseUrl must use http or https"
        )));
    }

    let name = name.trim();
    if field_is_too_long(name, PROVIDER_IMPORT_MAX_NAME_LEN) {
        return Err(bad_request_response(&format!(
            "Provider {position} name is too long"
        )));
    }
    let name = if name.is_empty() {
        "OpenAI Compatible"
    } else {
        name
    };

    let models = validate_provider_import_models(position, models)?;
    let custom_headers = validate_custom_headers(custom_headers).map_err(|_| {
        bad_request_response(&format!(
            "Provider {position} custom headers are not allowed"
        ))
    })?;
    let custom_body = custom_body
        .map(validate_custom_body_value)
        .transpose()
        .map_err(|_| {
            bad_request_response(&format!("Provider {position} custom body is invalid"))
        })?;

    Ok(ValidatedProviderImportItem {
        provider_type: OPENAI_COMPATIBLE_PROVIDER_TYPE.to_string(),
        enabled,
        name: name.to_string(),
        base_url: base_url.to_string(),
        models,
        has_secret,
        custom_headers,
        custom_body,
    })
}

fn validate_provider_import_models(
    provider_position: usize,
    models: &[ProviderExportModel],
) -> Result<Vec<ValidatedProviderImportModel>, Response> {
    if models.is_empty() {
        return Err(bad_request_response(&format!(
            "Provider {provider_position} must include at least one model"
        )));
    }
    if models.len() > PROVIDER_MODEL_MAX_ITEMS {
        return Err(bad_request_response(&format!(
            "Provider {provider_position} contains too many models"
        )));
    }

    let mut seen_model_ids = HashSet::new();
    let mut validated = Vec::with_capacity(models.len());
    for (index, model) in models.iter().enumerate() {
        let model_position = index + 1;
        let model_id = model.model_id.trim();
        if model_id.is_empty() {
            return Err(bad_request_response(&format!(
                "Provider {provider_position} model {model_position} modelId is required"
            )));
        }
        if field_is_too_long(model_id, PROVIDER_IMPORT_MAX_MODEL_ID_LEN) {
            return Err(bad_request_response(&format!(
                "Provider {provider_position} model {model_position} modelId is too long"
            )));
        }
        if !seen_model_ids.insert(model_id.to_string()) {
            return Err(bad_request_response(&format!(
                "Provider {provider_position} model {model_position} modelId is duplicated"
            )));
        }

        let display_name = model.display_name.trim();
        if field_is_too_long(display_name, PROVIDER_IMPORT_MAX_DISPLAY_NAME_LEN) {
            return Err(bad_request_response(&format!(
                "Provider {provider_position} model {model_position} displayName is too long"
            )));
        }
        let display_name = if display_name.is_empty() {
            model_id
        } else {
            display_name
        };

        validated.push(ValidatedProviderImportModel {
            model_id: model_id.to_string(),
            display_name: display_name.to_string(),
            input_modalities: normalize_input_modalities(Some(&model.input_modalities))
                .map_err(|_| {
                    bad_request_response(&format!(
                        "Provider {provider_position} model {model_position} input capabilities are invalid"
                    ))
                })?,
            output_modalities: normalize_output_modalities(Some(&model.output_modalities))
                .map_err(|_| {
                    bad_request_response(&format!(
                        "Provider {provider_position} model {model_position} output capabilities are invalid"
                    ))
                })?,
        });
    }

    Ok(validated)
}

fn provider_import_preview_item(
    provider: &ValidatedProviderImportItem,
) -> ProviderImportPreviewItem {
    ProviderImportPreviewItem {
        provider_type: provider.provider_type.clone(),
        enabled: provider.enabled,
        name: provider.name.clone(),
        base_url: provider.base_url.clone(),
        has_secret: provider.has_secret,
        models: provider
            .models
            .iter()
            .map(|model| ProviderExportModel {
                model_id: model.model_id.clone(),
                display_name: model.display_name.clone(),
                input_modalities: model.input_modalities.clone(),
                output_modalities: model.output_modalities.clone(),
            })
            .collect(),
        advanced_config: provider_import_advanced_summary(provider),
    }
}

fn provider_import_advanced_summary(
    provider: &ValidatedProviderImportItem,
) -> ProviderImportAdvancedSummary {
    ProviderImportAdvancedSummary {
        custom_header_count: provider.custom_headers.len(),
        custom_body_present: provider.custom_body.is_some(),
    }
}

fn is_supported_provider_base_url(value: &str) -> bool {
    match reqwest::Url::parse(value) {
        Ok(url) => matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        Err(_) => false,
    }
}

fn is_loopback_provider_base_url(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn provider_bound_image_file_ids(parts: &[Value]) -> Result<Vec<u64>, String> {
    let mut file_ids = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) != Some("image") {
            continue;
        }

        let Some(metadata) = part.get("metadata").and_then(Value::as_object) else {
            return Err("Image attachment metadata is missing".to_string());
        };
        let mime = metadata
            .get("mime")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if mime == "image/gif" {
            continue;
        }
        if !is_provider_image_mime(mime) {
            return Err(LOCAL_IMAGE_CAPTURE_UNSUPPORTED_TEXT.to_string());
        }
        let Some(file_id) = metadata.get("fileId").and_then(Value::as_u64) else {
            return Err("Image attachment metadata is missing".to_string());
        };
        file_ids.push(file_id);
    }

    if file_ids.len() > 1 {
        return Err("Only one image can be sent in this prototype.".to_string());
    }

    Ok(file_ids)
}

fn is_provider_image_mime(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp")
}

fn field_is_too_long(value: &str, max_chars: usize) -> bool {
    value.chars().count() > max_chars
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
    let config = {
        let providers = state.providers.read().await;
        providers.iter().find_map(|provider| {
            if !provider.enabled || provider.provider_type != OPENAI_COMPATIBLE_PROVIDER_TYPE {
                return None;
            }

            provider.models.iter().find_map(|model| {
                (model.id == selected_model_id || model.model_id == selected_model_id).then(|| {
                    (
                        provider.base_url.clone(),
                        model.model_id.clone(),
                        provider.secret_ref.clone(),
                        provider.custom_headers.clone(),
                        provider.custom_body.clone(),
                        model.input_modalities.clone(),
                    )
                })
            })
        })
    };

    let Some((base_url, model_id, secret_ref, custom_headers, custom_body, input_modalities)) =
        config
    else {
        return Ok(None);
    };

    let base_url = base_url.trim();
    let model_id = model_id.trim();
    if base_url.is_empty() || model_id.is_empty() {
        return Ok(None);
    }

    let Some(api_key) = state.secret_store.get_secret(&secret_ref)? else {
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
        custom_headers,
        custom_body,
        input_modalities,
    }))
}

async fn resolve_openai_chat_config_for_provider(
    state: &Arc<MockApiState>,
    provider_id: &str,
    selected_model_id: Option<&str>,
) -> Result<Option<OpenAiChatConfig>, String> {
    let provider = {
        let providers = state.providers.read().await;
        providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
    };

    let Some(provider) = provider else {
        return Err("Provider not found.".to_string());
    };

    if !provider.enabled || provider.provider_type != OPENAI_COMPATIBLE_PROVIDER_TYPE {
        return Err("Only openai-compatible providers can be tested.".to_string());
    }

    let base_url = provider.base_url.trim();
    let model = if let Some(selected_model_id) = selected_model_id {
        provider
            .models
            .iter()
            .find(|model| model.id == selected_model_id || model.model_id == selected_model_id)
            .ok_or_else(|| "Provider model not found.".to_string())?
    } else {
        provider
            .primary_model()
            .ok_or_else(|| "Provider has no models.".to_string())?
    };
    let model_id = model.model_id.trim();
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
        custom_headers: provider.custom_headers.clone(),
        custom_body: provider.custom_body.clone(),
        input_modalities: model.input_modalities.clone(),
    }))
}

fn build_openai_chat_request_body(
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
    stream: bool,
    kind: OpenAiRequestKind,
) -> Result<Value, String> {
    let mut body = if let Some(custom_body) = config.custom_body.as_ref() {
        let validated = validate_custom_body_value(custom_body)?;
        validated
            .as_object()
            .cloned()
            .ok_or_else(|| "Custom body must be a JSON object".to_string())?
    } else {
        Map::new()
    };

    body.insert("model".to_string(), json!(config.model_id.clone()));
    body.insert(
        "messages".to_string(),
        serde_json::to_value(messages).map_err(|_| "Failed to build request body".to_string())?,
    );
    body.insert("stream".to_string(), json!(stream));

    if kind == OpenAiRequestKind::TestConnection {
        body.insert("max_tokens".to_string(), json!(1));
    }

    Ok(Value::Object(body))
}

#[allow(dead_code)]
fn build_openai_vision_chat_request_body(
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiCompatibleChatMessage>,
    stream: bool,
    kind: OpenAiRequestKind,
) -> Result<Value, String> {
    let mut body = if let Some(custom_body) = config.custom_body.as_ref() {
        let validated = validate_custom_body_value(custom_body)?;
        validated
            .as_object()
            .cloned()
            .ok_or_else(|| "Custom body must be a JSON object".to_string())?
    } else {
        Map::new()
    };

    let messages = messages
        .iter()
        .map(openai_compatible_message_to_value)
        .collect::<Result<Vec<_>, _>>()?;

    body.insert("model".to_string(), json!(config.model_id.clone()));
    body.insert("messages".to_string(), Value::Array(messages));
    body.insert("stream".to_string(), json!(stream));

    if kind == OpenAiRequestKind::TestConnection {
        body.insert("max_tokens".to_string(), json!(1));
    }

    Ok(Value::Object(body))
}

#[allow(dead_code)]
fn openai_compatible_message_to_value(
    message: &OpenAiCompatibleChatMessage,
) -> Result<Value, String> {
    let content = match &message.content {
        OpenAiCompatibleMessageContent::Text(text) => Value::String(text.clone()),
        OpenAiCompatibleMessageContent::Parts(parts) => {
            let parts = parts
                .iter()
                .map(openai_compatible_content_part_to_value)
                .collect::<Result<Vec<_>, _>>()?;
            Value::Array(parts)
        }
    };

    Ok(json!({
        "role": message.role.clone(),
        "content": content,
    }))
}

#[allow(dead_code)]
fn openai_compatible_content_part_to_value(
    part: &OpenAiCompatibleContentPart,
) -> Result<Value, String> {
    match part {
        OpenAiCompatibleContentPart::Text(text) => Ok(json!({
            "type": "text",
            "text": text,
        })),
        OpenAiCompatibleContentPart::ImageUrl {
            data_url,
            detail,
            file_id: _,
        } => {
            if !is_supported_image_data_url(data_url) {
                return Err("Image data URL is not supported".to_string());
            }

            Ok(json!({
                "type": "image_url",
                "image_url": {
                    "url": data_url,
                    "detail": openai_image_detail_value(*detail),
                }
            }))
        }
    }
}

#[allow(dead_code)]
fn openai_image_detail_value(detail: OpenAiImageDetail) -> &'static str {
    match detail {
        OpenAiImageDetail::Auto => "auto",
        OpenAiImageDetail::Low => "low",
        OpenAiImageDetail::High => "high",
    }
}

#[allow(dead_code)]
fn is_supported_image_data_url(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }

    [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/webp;base64,",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn image_data_url(mime: &str, bytes: &[u8]) -> Result<String, String> {
    if !is_provider_image_mime(mime) {
        return Err("Image attachment type is not supported for capture testing".to_string());
    }
    if bytes.len() > PROVIDER_IMAGE_INPUT_MAX_BYTES {
        return Err("Image attachment is too large for capture testing".to_string());
    }

    Ok(format!(
        "data:{mime};base64,{}",
        base64_encode_for_data_url(bytes)
    ))
}

fn base64_encode_for_data_url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn openai_vision_messages_for_current_turn(
    user_text: Option<String>,
    image: ManagedImageProviderInput,
) -> Result<Vec<OpenAiCompatibleChatMessage>, String> {
    let text = user_text
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Describe this image.".to_string());
    let data_url = image_data_url(&image.mime, &image.bytes)?;

    Ok(vec![OpenAiCompatibleChatMessage {
        role: "user".to_string(),
        content: OpenAiCompatibleMessageContent::Parts(vec![
            OpenAiCompatibleContentPart::Text(text),
            OpenAiCompatibleContentPart::ImageUrl {
                data_url,
                detail: OpenAiImageDetail::Auto,
                file_id: image.file_id,
            },
        ]),
    }])
}

fn apply_openai_custom_headers(
    builder: reqwest::RequestBuilder,
    headers: &[DesktopProviderCustomHeaderConfig],
) -> Result<reqwest::RequestBuilder, String> {
    let headers = validate_custom_headers(headers)?;
    let mut builder = builder;
    for header in headers {
        let name = ReqwestHeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| "Custom header name is invalid".to_string())?;
        let value = ReqwestHeaderValue::from_str(&header.value)
            .map_err(|_| "Custom header value is invalid".to_string())?;
        builder = builder.header(name, value);
    }
    Ok(builder)
}

async fn test_openai_compatible_chat_connection(
    state: &Arc<MockApiState>,
    config: &OpenAiChatConfig,
) -> Result<(), String> {
    let body = build_openai_chat_request_body(
        config,
        vec![OpenAiChatMessage {
            role: "user".to_string(),
            content: "ping".to_string(),
        }],
        false,
        OpenAiRequestKind::TestConnection,
    )?;

    let request = state
        .http_client
        .post(openai_chat_completions_url(&config.base_url))
        .timeout(Duration::from_secs(OPENAI_TEST_TIMEOUT_SECS));
    let request = apply_openai_custom_headers(request, &config.custom_headers)?;
    let response = request
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await
        .map_err(safe_reqwest_error)?;

    let status = response.status();
    if !status.is_success() {
        return Err(safe_http_status_error(status));
    }

    response
        .json::<Value>()
        .await
        .map_err(|_| "provider response was not valid JSON".to_string())?;

    Ok(())
}

fn spawn_openai_stream_generation(
    state: Arc<MockApiState>,
    conversation_id: String,
    assistant_message_id: String,
    config: OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
) {
    tokio::spawn(async move {
        let stream_result = stream_openai_compatible_chat(
            &state,
            &conversation_id,
            &assistant_message_id,
            &config,
            messages,
        )
        .await;

        if let Err(error) = stream_result {
            let error_text = format!("Real provider request failed: {error}");
            append_text_to_assistant_message(
                &state,
                &conversation_id,
                &assistant_message_id,
                &error_text,
            )
            .await;
        }

        finish_streaming_assistant_reply(&state, &conversation_id, &assistant_message_id).await;
    });
}

fn spawn_openai_vision_capture_generation(
    state: Arc<MockApiState>,
    conversation_id: String,
    assistant_message_id: String,
    config: OpenAiChatConfig,
    messages: Vec<OpenAiCompatibleChatMessage>,
) {
    tokio::spawn(async move {
        let stream_result = stream_openai_compatible_vision_capture(
            &state,
            &conversation_id,
            &assistant_message_id,
            &config,
            messages,
        )
        .await;

        if let Err(error) = stream_result {
            append_text_to_assistant_message(
                &state,
                &conversation_id,
                &assistant_message_id,
                &error,
            )
            .await;
        }

        finish_streaming_assistant_reply(&state, &conversation_id, &assistant_message_id).await;
    });
}

async fn stream_openai_compatible_chat(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_message_id: &str,
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
) -> Result<(), String> {
    let body =
        build_openai_chat_request_body(config, messages, true, OpenAiRequestKind::StreamingChat)?;

    let request = state
        .http_client
        .post(openai_chat_completions_url(&config.base_url));
    let request = apply_openai_custom_headers(request, &config.custom_headers)?;
    let mut response = request
        .bearer_auth(&config.api_key)
        .json(&body)
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

async fn stream_openai_compatible_vision_capture(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    assistant_message_id: &str,
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiCompatibleChatMessage>,
) -> Result<(), String> {
    let body = build_openai_vision_chat_request_body(
        config,
        messages,
        true,
        OpenAiRequestKind::StreamingChat,
    )?;

    let request = state
        .http_client
        .post(openai_chat_completions_url(&config.base_url));
    let request = apply_openai_custom_headers(request, &config.custom_headers)?;
    let mut response = request
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await
        .map_err(safe_reqwest_error)?;

    let status = response.status();
    if !status.is_success() {
        return Err(safe_http_status_error(status));
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

fn safe_http_status_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "authentication failed".to_string(),
        404 => "provider endpoint or model was not found".to_string(),
        408 => "provider request timeout".to_string(),
        429 => "provider rate limited the request".to_string(),
        500..=599 => "provider service error".to_string(),
        code => format!(
            "{} {}",
            code,
            status.canonical_reason().unwrap_or("HTTP error")
        ),
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
) -> Result<BuiltDesktopProvider, Response> {
    let requested_type = payload
        .provider_type
        .as_deref()
        .unwrap_or(OPENAI_COMPATIBLE_PROVIDER_TYPE);
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

    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| existing.as_ref().map(|provider| provider.name.clone()))
        .unwrap_or_else(|| "OpenAI Compatible".to_string());

    let used_models_request = payload.models.is_some();
    let models = if let Some(models) = payload.models.as_ref() {
        build_models_from_multi_request(state, existing.as_ref(), models)?
    } else {
        build_models_from_singular_request(
            state,
            existing.as_ref(),
            payload.model_id.as_deref(),
            payload.display_name.as_deref(),
        )?
    };

    let new_model_ids = models
        .iter()
        .map(|model| model.id.as_str())
        .collect::<HashSet<_>>();
    let removed_model_ids = existing
        .as_ref()
        .map(|provider| {
            provider
                .models
                .iter()
                .filter(|model| !new_model_ids.contains(model.id.as_str()))
                .map(|model| model.id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let custom_headers = if let Some(headers) = payload.custom_headers.as_ref() {
        validate_custom_headers(headers).map_err(|message| bad_request_response(&message))?
    } else {
        existing
            .as_ref()
            .map(|provider| provider.custom_headers.clone())
            .unwrap_or_default()
    };

    let custom_body = match &payload.custom_body {
        CustomBodyUpdate::Set(value) => Some(
            validate_custom_body_value(value).map_err(|message| bad_request_response(&message))?,
        ),
        CustomBodyUpdate::Clear => None,
        CustomBodyUpdate::Missing => existing
            .as_ref()
            .and_then(|provider| provider.custom_body.clone()),
    };

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
        models,
        legacy_model: None,
        custom_headers,
        custom_body,
    };

    let api_key = payload
        .api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(BuiltDesktopProvider {
        provider,
        api_key,
        removed_model_ids,
        used_models_request,
    })
}

fn build_models_from_multi_request(
    state: &Arc<MockApiState>,
    existing: Option<&DesktopProviderConfig>,
    requests: &[UpsertDesktopProviderModelRequest],
) -> Result<Vec<DesktopProviderModelConfig>, Response> {
    if requests.is_empty() {
        return Err(bad_request_response(
            "models must include at least one model",
        ));
    }
    if requests.len() > PROVIDER_MODEL_MAX_ITEMS {
        return Err(bad_request_response("Provider has too many models"));
    }

    let mut seen_model_ids = HashSet::new();
    let mut seen_record_ids = HashSet::new();
    let mut models = Vec::with_capacity(requests.len());

    for (index, request) in requests.iter().enumerate() {
        let position = index + 1;
        let model_id = request.model_id.trim();
        if model_id.is_empty() {
            return Err(bad_request_response(&format!(
                "Provider model {position} modelId is required"
            )));
        }
        if field_is_too_long(model_id, PROVIDER_IMPORT_MAX_MODEL_ID_LEN) {
            return Err(bad_request_response(&format!(
                "Provider model {position} modelId is too long"
            )));
        }
        if !seen_model_ids.insert(model_id.to_string()) {
            return Err(bad_request_response(&format!(
                "Provider model {position} modelId is duplicated"
            )));
        }

        let display_name = request
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(model_id);
        if field_is_too_long(display_name, PROVIDER_IMPORT_MAX_DISPLAY_NAME_LEN) {
            return Err(bad_request_response(&format!(
                "Provider model {position} displayName is too long"
            )));
        }

        let requested_record_id = request
            .id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                if is_safe_config_id(id) {
                    Ok(id.to_string())
                } else {
                    Err(bad_request_response(&format!(
                        "Provider model {position} id contains unsupported characters"
                    )))
                }
            })
            .transpose()?;

        let existing_model = existing.and_then(|provider| {
            requested_record_id
                .as_ref()
                .and_then(|record_id| provider.models.iter().find(|model| model.id == *record_id))
                .or_else(|| {
                    provider
                        .models
                        .iter()
                        .find(|model| model.model_id == model_id)
                })
        });

        let record_id = requested_record_id
            .or_else(|| existing_model.map(|model| model.id.clone()))
            .unwrap_or_else(|| state.next_id("desktop-model"));

        if !seen_record_ids.insert(record_id.clone()) {
            return Err(bad_request_response(&format!(
                "Provider model {position} id is duplicated"
            )));
        }

        let input_modalities = if let Some(modalities) = request.input_modalities.as_ref() {
            normalize_input_modalities(Some(modalities)).map_err(|message| {
                bad_request_response(&format!("Provider model {position}: {message}"))
            })?
        } else {
            existing_model
                .map(|model| model.input_modalities.clone())
                .unwrap_or_else(default_input_modalities)
        };
        let output_modalities = if let Some(modalities) = request.output_modalities.as_ref() {
            normalize_output_modalities(Some(modalities)).map_err(|message| {
                bad_request_response(&format!("Provider model {position}: {message}"))
            })?
        } else {
            existing_model
                .map(|model| model.output_modalities.clone())
                .unwrap_or_else(default_output_modalities)
        };

        models.push(DesktopProviderModelConfig {
            id: record_id,
            model_id: model_id.to_string(),
            display_name: display_name.to_string(),
            input_modalities,
            output_modalities,
        });
    }

    Ok(models)
}

fn build_models_from_singular_request(
    state: &Arc<MockApiState>,
    existing: Option<&DesktopProviderConfig>,
    model_id: Option<&str>,
    display_name: Option<&str>,
) -> Result<Vec<DesktopProviderModelConfig>, Response> {
    let model_id = model_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            existing
                .and_then(DesktopProviderConfig::primary_model)
                .map(|model| model.model_id.clone())
        })
        .ok_or_else(|| bad_request_response("modelId is required"))?;
    if field_is_too_long(&model_id, PROVIDER_IMPORT_MAX_MODEL_ID_LEN) {
        return Err(bad_request_response("modelId is too long"));
    }

    let display_name = display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            existing
                .and_then(DesktopProviderConfig::primary_model)
                .map(|model| model.display_name.clone())
        })
        .unwrap_or_else(|| model_id.clone());
    if field_is_too_long(&display_name, PROVIDER_IMPORT_MAX_DISPLAY_NAME_LEN) {
        return Err(bad_request_response("displayName is too long"));
    }

    let model_record_id = existing
        .and_then(DesktopProviderConfig::primary_model)
        .map(|model| model.id.clone())
        .unwrap_or_else(|| state.next_id("desktop-model"));

    let mut models = existing
        .map(|provider| provider.models.clone())
        .unwrap_or_default();
    if let Some(model) = models.first_mut() {
        model.id = model_record_id;
        model.model_id = model_id;
        model.display_name = display_name;
        model.normalize_modalities();
    } else {
        models.push(DesktopProviderModelConfig {
            id: model_record_id,
            model_id,
            display_name,
            input_modalities: default_input_modalities(),
            output_modalities: default_output_modalities(),
        });
    }

    Ok(models)
}

fn validate_custom_headers(
    headers: &[DesktopProviderCustomHeaderConfig],
) -> Result<Vec<DesktopProviderCustomHeaderConfig>, String> {
    if headers.len() > PROVIDER_CUSTOM_HEADER_MAX_ITEMS {
        return Err("Custom header list is too large".to_string());
    }

    headers
        .iter()
        .map(|header| {
            let name = header.name.trim();
            let value = header.value.trim();
            if name.is_empty() {
                return Err("Custom header name is required".to_string());
            }
            if value.is_empty() {
                return Err("Custom header value is required".to_string());
            }
            if field_is_too_long(name, PROVIDER_CUSTOM_HEADER_MAX_NAME_LEN) {
                return Err("Custom header name is too long".to_string());
            }
            if field_is_too_long(value, PROVIDER_CUSTOM_HEADER_MAX_VALUE_LEN) {
                return Err("Custom header value is too long".to_string());
            }
            if is_forbidden_custom_header_name(name)
                || contains_sensitive_custom_text(name)
                || contains_sensitive_custom_text(value)
            {
                return Err("Custom header is not allowed".to_string());
            }

            ReqwestHeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "Custom header name is invalid".to_string())?;
            ReqwestHeaderValue::from_str(value)
                .map_err(|_| "Custom header value is invalid".to_string())?;

            Ok(DesktopProviderCustomHeaderConfig {
                name: name.to_string(),
                value: value.to_string(),
            })
        })
        .collect()
}

fn is_forbidden_custom_header_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "apikey"
            | "api_key"
            | "cookie"
            | "set-cookie"
            | "authentication"
            | "x-auth-token"
            | "x-access-token"
    )
}

fn contains_sensitive_custom_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "bearer",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "api key",
        "sk-",
        "refresh_token",
        "access_token",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

fn validate_custom_body_value(value: &Value) -> Result<Value, String> {
    let serialized = serde_json::to_vec(value).map_err(|_| "Custom body is invalid".to_string())?;
    if serialized.len() > PROVIDER_CUSTOM_BODY_MAX_BYTES {
        return Err("Custom body is too large".to_string());
    }

    let object = value
        .as_object()
        .ok_or_else(|| "Custom body must be a JSON object".to_string())?;

    for key in object.keys() {
        if is_reserved_custom_body_key(key) {
            return Err("Custom body contains reserved fields".to_string());
        }
        if is_sensitive_custom_body_key(key) {
            return Err("Custom body contains sensitive fields".to_string());
        }
    }

    scan_custom_body_for_sensitive_values(value)?;
    Ok(Value::Object(object.clone()))
}

fn is_reserved_custom_body_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "model" | "messages" | "stream"
    )
}

fn is_sensitive_custom_body_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    if lower == "max_tokens" {
        return false;
    }

    matches!(
        lower.as_str(),
        "apikey"
            | "api_key"
            | "authorization"
            | "x-api-key"
            | "token"
            | "access_token"
            | "accesstoken"
            | "refresh_token"
            | "refreshtoken"
            | "password"
            | "secret"
            | "credential"
    ) || [
        "bearer",
        "token",
        "password",
        "passwd",
        "secret",
        "credential",
        "api key",
        "sk-",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

fn scan_custom_body_for_sensitive_values(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                if is_sensitive_custom_body_key(key) {
                    return Err("Custom body contains sensitive fields".to_string());
                }
                scan_custom_body_for_sensitive_values(nested)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                scan_custom_body_for_sensitive_values(item)?;
            }
        }
        Value::String(text) if contains_sensitive_custom_text(text) => {
            return Err("Custom body contains sensitive values".to_string());
        }
        _ => {}
    }

    Ok(())
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
        files: Vec::new(),
    }
}

fn migrate_v1_to_v6(mut persisted: PersistedMockState) -> PersistedMockState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.saved_at = now_millis();
    persisted.providers = Vec::new();
    persisted.files = Vec::new();
    persisted
}

fn migrate_v2_to_v6(mut persisted: PersistedMockState) -> PersistedMockState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.saved_at = now_millis();
    normalize_desktop_providers(&mut persisted.providers);
    persisted.files = Vec::new();
    persisted
}

fn migrate_v3_to_v6(mut persisted: PersistedMockState) -> PersistedMockState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.saved_at = now_millis();
    normalize_desktop_providers(&mut persisted.providers);
    persisted.files = Vec::new();
    persisted
}

fn migrate_v4_to_v6(mut persisted: PersistedMockState) -> PersistedMockState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.saved_at = now_millis();
    normalize_desktop_providers(&mut persisted.providers);
    persisted.files = Vec::new();
    persisted
}

fn migrate_v5_to_v6(mut persisted: PersistedMockState) -> PersistedMockState {
    persisted.schema_version = STATE_SCHEMA_VERSION;
    persisted.saved_at = now_millis();
    normalize_desktop_providers(&mut persisted.providers);
    persisted
}

fn normalize_desktop_providers(providers: &mut Vec<DesktopProviderConfig>) {
    for provider in providers {
        provider.normalize_models();
    }
}

fn sync_settings_with_desktop_providers(settings: &mut Value, providers: &[DesktopProviderConfig]) {
    let existing_providers = settings
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut merged_providers: Vec<Value> = existing_providers
        .into_iter()
        .filter(|provider| {
            !provider
                .get("secretRef")
                .and_then(Value::as_str)
                .is_some_and(|secret_ref| secret_ref.starts_with(PROVIDER_SECRET_REF_PREFIX))
                && provider
                    .get("id")
                    .and_then(Value::as_str)
                    .is_none_or(|id| !providers.iter().any(|provider| provider.id == id))
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
}

fn remove_models_from_favorites<'a, I>(settings: &mut Value, model_ids: I)
where
    I: IntoIterator<Item = &'a str>,
{
    let model_ids = model_ids.into_iter().collect::<HashSet<_>>();
    if model_ids.is_empty() {
        return;
    }

    let Some(favorite_models) = settings
        .get_mut("favoriteModels")
        .and_then(Value::as_array_mut)
    else {
        return;
    };

    favorite_models.retain(|item| {
        item.as_str()
            .is_none_or(|model_id| !model_ids.contains(model_id))
    });
}

fn ensure_current_model_exists(settings: &mut Value) {
    let current_model_id = settings
        .get("chatModelId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let current_model_exists = current_model_id
        .as_deref()
        .is_some_and(|model_id| settings_contains_model(settings, model_id));

    if !current_model_exists {
        if let Some(model_id) = first_settings_model_id(settings) {
            set_current_model_in_settings(settings, &model_id);
        }
        return;
    }

    let current_assistant_id = settings
        .get("assistantId")
        .and_then(Value::as_str)
        .unwrap_or(MOCK_ASSISTANT_ID)
        .to_string();
    let assistant_model_id = settings
        .get("assistants")
        .and_then(Value::as_array)
        .and_then(|assistants| {
            assistants.iter().find_map(|assistant| {
                let is_current = assistant
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == current_assistant_id);
                is_current.then(|| assistant.get("chatModelId").and_then(Value::as_str))?
            })
        });
    let assistant_model_exists =
        assistant_model_id.is_some_and(|model_id| settings_contains_model(settings, model_id));

    if !assistant_model_exists {
        if let Some(model_id) = current_model_id {
            set_current_model_in_settings(settings, &model_id);
        }
    }
}

fn settings_contains_model(settings: &Value, model_id: &str) -> bool {
    settings
        .get("providers")
        .and_then(Value::as_array)
        .is_some_and(|providers| {
            providers.iter().any(|provider| {
                provider
                    .get("models")
                    .and_then(Value::as_array)
                    .is_some_and(|models| {
                        models.iter().any(|model| {
                            model
                                .get("id")
                                .and_then(Value::as_str)
                                .is_some_and(|id| id == model_id)
                        })
                    })
            })
        })
}

fn first_settings_model_id(settings: &Value) -> Option<String> {
    settings
        .get("providers")
        .and_then(Value::as_array)?
        .iter()
        .filter(|provider| {
            provider
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .find_map(|provider| {
            provider
                .get("models")
                .and_then(Value::as_array)?
                .iter()
                .find_map(|model| {
                    let is_chat = model
                        .get("type")
                        .and_then(Value::as_str)
                        .is_none_or(|model_type| model_type == "CHAT");
                    is_chat.then(|| {
                        model
                            .get("id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    })?
                })
        })
}

fn secret_ref_for_provider(provider_id: &str) -> String {
    format!("{PROVIDER_SECRET_REF_PREFIX}{provider_id}:api-key")
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

fn is_safe_storage_key(storage_key: &str) -> bool {
    !storage_key.is_empty()
        && storage_key.len() <= 128
        && storage_key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn storage_key_for_file(id: u64) -> String {
    format!("file-{id}-{}", now_millis())
}

fn sanitize_upload_file_name(file_name: Option<&str>) -> String {
    let file_name = file_name
        .unwrap_or("upload.bin")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("upload.bin");
    let mut sanitized = file_name
        .chars()
        .filter(|ch| {
            !ch.is_control() && !matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .collect::<String>()
        .trim()
        .to_string();

    if sanitized.is_empty() {
        sanitized = "upload.bin".to_string();
    }

    truncate_chars(&sanitized, FILE_DISPLAY_NAME_MAX_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn safe_header_file_name(file_name: &str) -> String {
    let value = sanitize_upload_file_name(Some(file_name));
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii() && !ch.is_control() && !matches!(ch, '"' | '\\') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "download.bin".to_string()
    } else {
        sanitized
    }
}

fn detect_safe_upload_mime(bytes: &[u8], display_name: &str) -> Result<&'static str, &'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Ok("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp");
    }
    if bytes.starts_with(b"%PDF-") {
        return Ok("application/pdf");
    }
    if is_utf8_plain_text(bytes) {
        if is_dangerous_text_upload(bytes, display_name) {
            return Err("File type is not supported");
        }
        return Ok("text/plain");
    }

    Err("File type is not supported")
}

fn is_utf8_plain_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

fn is_dangerous_text_upload(bytes: &[u8], display_name: &str) -> bool {
    if has_dangerous_upload_extension(display_name) {
        return true;
    }

    let Ok(text) = std::str::from_utf8(bytes) else {
        return true;
    };
    let lower = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .take(1024)
        .collect::<String>()
        .to_ascii_lowercase();

    lower.starts_with("<svg")
        || lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<script")
        || lower.contains("<script")
        || lower.contains("<svg")
        || lower.contains("<iframe")
        || lower.contains("<object")
        || lower.contains("<embed")
}

fn has_dangerous_upload_extension(display_name: &str) -> bool {
    let extension = display_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    matches!(
        extension.as_deref(),
        Some(
            "svg"
                | "html"
                | "htm"
                | "js"
                | "mjs"
                | "cjs"
                | "jsx"
                | "ts"
                | "tsx"
                | "vbs"
                | "ps1"
                | "bat"
                | "cmd"
                | "sh"
                | "exe"
                | "dll"
                | "msi"
                | "zip"
                | "rar"
                | "7z"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
        )
    )
}

fn upload_kind_for_mime(mime: &str) -> &'static str {
    if mime.starts_with("image/") {
        "image"
    } else {
        "document"
    }
}

fn max_persisted_file_id(files: &[ManagedFileMetadata]) -> u64 {
    files.iter().map(|file| file.id).max().unwrap_or(1)
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

fn has_non_text_parts(parts: &[Value]) -> bool {
    parts
        .iter()
        .any(|part| part.get("type").and_then(Value::as_str) != Some("text"))
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

fn selected_message(node: &MessageNodeDto) -> Option<&MessageDto> {
    node.messages
        .get(node.select_index)
        .or_else(|| node.messages.first())
}

fn find_regeneratable_node_index(
    conversation: &ConversationDto,
    requested_message_id: Option<&str>,
) -> Option<usize> {
    let last_index = conversation.messages.len().checked_sub(1)?;
    let last_message = selected_message(&conversation.messages[last_index])?;
    let target_message_id = requested_message_id.unwrap_or(&last_message.id);

    let target_index = conversation.messages.iter().position(|node| {
        selected_message(node).is_some_and(|message| message.id == target_message_id)
    })?;

    if target_index == last_index {
        return Some(target_index);
    }

    let target_message = selected_message(&conversation.messages[target_index])?;
    if target_index + 1 == last_index
        && target_message.role == "USER"
        && last_message.role == "ASSISTANT"
    {
        return Some(target_index);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_vision_test_config(custom_body: Option<Value>) -> OpenAiChatConfig {
        OpenAiChatConfig {
            base_url: "http://127.0.0.1:9999/v1".to_string(),
            model_id: "vision-test-model".to_string(),
            api_key: "test-key".to_string(),
            custom_headers: Vec::new(),
            custom_body,
            input_modalities: vec![
                MODEL_MODALITY_TEXT.to_string(),
                MODEL_MODALITY_IMAGE.to_string(),
            ],
        }
    }

    fn openai_vision_test_message(data_url: &str) -> OpenAiCompatibleChatMessage {
        OpenAiCompatibleChatMessage {
            role: "user".to_string(),
            content: OpenAiCompatibleMessageContent::Parts(vec![
                OpenAiCompatibleContentPart::Text("Describe this image".to_string()),
                OpenAiCompatibleContentPart::ImageUrl {
                    data_url: data_url.to_string(),
                    detail: OpenAiImageDetail::Auto,
                    file_id: 123,
                },
            ]),
        }
    }

    #[test]
    fn openai_vision_builder_serializes_content_array() {
        let config = openai_vision_test_config(None);
        let body = build_openai_vision_chat_request_body(
            &config,
            vec![openai_vision_test_message("data:image/png;base64,AAAA")],
            true,
            OpenAiRequestKind::StreamingChat,
        )
        .expect("vision request body should build");

        assert_eq!(body["model"], json!("vision-test-model"));
        assert_eq!(body["stream"], json!(true));

        let content = body["messages"][0]["content"]
            .as_array()
            .expect("vision message content should be an array");
        assert_eq!(content[0]["type"], json!("text"));
        assert_eq!(content[0]["text"], json!("Describe this image"));
        assert_eq!(content[1]["type"], json!("image_url"));
        assert_eq!(
            content[1]["image_url"]["url"]
                .as_str()
                .expect("image URL should be a string")
                .starts_with("data:image/png;base64,"),
            true
        );
        assert_eq!(content[1]["image_url"]["detail"], json!("auto"));
    }

    #[test]
    fn openai_vision_builder_does_not_serialize_internal_file_id() {
        let config = openai_vision_test_config(None);
        let body = build_openai_vision_chat_request_body(
            &config,
            vec![openai_vision_test_message("data:image/jpeg;base64,AAAA")],
            true,
            OpenAiRequestKind::StreamingChat,
        )
        .expect("vision request body should build");

        let serialized = serde_json::to_string(&body).expect("body should serialize");
        assert!(!serialized.contains("file_id"));
        assert!(!serialized.contains("storageKey"));
        assert!(!serialized.contains("/api/files/path"));
        assert!(!serialized.contains("file://"));
        assert!(!serialized.contains("123"));
    }

    #[test]
    fn openai_vision_builder_rejects_unsupported_data_url() {
        let config = openai_vision_test_config(None);
        for value in [
            "data:image/svg+xml;base64,AAAA",
            "data:image/gif;base64,AAAA",
            "data:text/html;base64,AAAA",
            "file:///C:/test.png",
            "/api/files/path/1",
            "https://example.com/image.png",
            "",
        ] {
            let result = build_openai_vision_chat_request_body(
                &config,
                vec![openai_vision_test_message(value)],
                true,
                OpenAiRequestKind::StreamingChat,
            );
            assert!(result.is_err(), "{value} should be rejected");
        }
    }

    #[test]
    fn openai_vision_text_builder_still_serializes_string_content() {
        let config = openai_vision_test_config(None);
        let body = build_openai_chat_request_body(
            &config,
            vec![OpenAiChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            }],
            true,
            OpenAiRequestKind::StreamingChat,
        )
        .expect("text request body should build");

        assert_eq!(body["messages"][0]["content"], json!("hello"));
        assert!(body["messages"][0]["content"].as_array().is_none());
    }

    #[test]
    fn loopback_base_url_allows_localhost() {
        for value in [
            "http://127.0.0.1:9999/v1",
            "http://localhost:9999/v1",
            "http://[::1]:9999/v1",
            "https://127.0.0.1:9999/v1",
        ] {
            assert!(
                is_loopback_provider_base_url(value),
                "{value} should be allowed"
            );
        }
    }

    #[test]
    fn loopback_base_url_rejects_public_and_private_hosts() {
        for value in [
            "https://api.openai.com/v1",
            "http://example.com/v1",
            "http://192.168.1.10:9999/v1",
            "http://10.0.0.1:9999/v1",
            "http://172.16.0.1:9999/v1",
            "http://0.0.0.0:9999/v1",
            "file:///C:/test",
            "/v1",
            "",
        ] {
            assert!(
                !is_loopback_provider_base_url(value),
                "{value} should be rejected"
            );
        }
    }

    #[test]
    fn loopback_base_url_rejects_private_networks() {
        for value in [
            "http://192.168.0.2:9999/v1",
            "http://10.1.2.3:9999/v1",
            "http://172.16.0.1:9999/v1",
            "http://172.31.255.254:9999/v1",
            "http://0.0.0.0:9999/v1",
        ] {
            assert!(
                !is_loopback_provider_base_url(value),
                "{value} should be rejected"
            );
        }
    }

    #[test]
    fn provider_bound_image_file_ids_ignores_gif() {
        let parts = vec![json!({
            "type": "image",
            "metadata": {
                "fileId": 42,
                "mime": "image/gif"
            }
        })];

        let file_ids =
            provider_bound_image_file_ids(&parts).expect("gif should not be provider-bound");

        assert!(file_ids.is_empty());
    }

    #[test]
    fn provider_bound_image_file_ids_rejects_multiple_provider_images() {
        let parts = vec![
            json!({
                "type": "image",
                "metadata": {
                    "fileId": 1,
                    "mime": "image/png"
                }
            }),
            json!({
                "type": "image",
                "metadata": {
                    "fileId": 2,
                    "mime": "image/webp"
                }
            }),
        ];

        let result = provider_bound_image_file_ids(&parts);

        assert!(result.is_err());
    }

    #[test]
    fn provider_bound_image_file_ids_rejects_missing_metadata() {
        let parts = vec![json!({
            "type": "image",
            "url": "/api/files/path/1"
        })];

        let result = provider_bound_image_file_ids(&parts);

        assert!(result.is_err());
    }

    #[test]
    fn image_data_url_rejects_gif() {
        let result = image_data_url("image/gif", b"GIF89a");

        assert!(result.is_err());
    }

    #[test]
    fn image_data_url_rejects_oversized_bytes() {
        let bytes = vec![0; PROVIDER_IMAGE_INPUT_MAX_BYTES + 1];
        let result = image_data_url("image/png", &bytes);

        assert!(result.is_err());
    }

    #[test]
    fn base64_encoder_known_value() {
        assert_eq!(base64_encode_for_data_url(b""), "");
        assert_eq!(base64_encode_for_data_url(b"f"), "Zg==");
        assert_eq!(base64_encode_for_data_url(b"fo"), "Zm8=");
        assert_eq!(base64_encode_for_data_url(b"foo"), "Zm9v");
        assert_eq!(base64_encode_for_data_url(b"hello"), "aGVsbG8=");
    }
}
