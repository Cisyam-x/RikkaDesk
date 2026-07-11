use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    error::Error,
    fmt, fs as std_fs,
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr},
    path::{Path as FilePath, PathBuf},
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

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
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    net::TcpListener,
    sync::{broadcast, Mutex, Notify, RwLock},
};
use tower_http::cors::{Any, CorsLayer};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{GetLastError, LocalFree},
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
    Storage::FileSystem::ReplaceFileW,
};

const PREFERRED_ADDR: &str = "127.0.0.1:8080";
const PERSIST_DIR_NAME: &str = "mock-api";
const STATE_FILE_NAME: &str = "state.v1.json";
const STATE_TMP_FILE_PREFIX: &str = "state.v1.json.tmp";
const STATE_BACKUP_CREATE_ATTEMPTS: usize = 32;
const BACKUP_FORMAT_NAME: &str = "rikkadesk-backup";
const BACKUP_FORMAT_VERSION: u32 = 1;
const BACKUP_CREATE_ATTEMPTS: usize = 32;
const BACKUP_MANIFEST_FILE_NAME: &str = "manifest.json";
const BACKUP_STATE_FILE_NAME: &str = "state.json";
const BACKUP_CHECKSUM_FILE_NAME: &str = "SHA256SUMS.txt";
const BACKUP_BLOBS_DIR_NAME: &str = "blobs";
const BACKUP_TEMP_DIR_PREFIX: &str = ".rikkadesk-backup.tmp";
const BACKUP_FINAL_DIR_PREFIX: &str = "RikkaDesk-backup-v1";
const BACKUP_SECRET_REF_MARKER: &str = "backup-unavailable";
const STATE_SCHEMA_VERSION: u32 = 6;
const FILE_METADATA_STATE_SCHEMA_VERSION: u32 = 5;
const CUSTOM_REQUEST_CONFIG_STATE_SCHEMA_VERSION: u32 = 4;
const MULTI_MODEL_STATE_SCHEMA_VERSION: u32 = 3;
const PREVIOUS_STATE_SCHEMA_VERSION: u32 = 2;
const LEGACY_STATE_SCHEMA_VERSION: u32 = 1;
const SECRETS_DIR_NAME: &str = "secrets";
const FILES_DIR_NAME: &str = "files";
const FILE_BLOBS_DIR_NAME: &str = "blobs";
const FILE_BLOB_TEMP_PREFIX: &str = "blob.tmp";
const FILE_BLOB_CREATE_ATTEMPTS: usize = 32;
const FILE_BLOB_CLEANUP_ATTEMPTS: usize = 3;
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

type PersistenceResult<T> = Result<T, PersistenceError>;
type SecretStoreResult<T> = Result<T, String>;

#[derive(Debug)]
struct PersistenceError {
    stage: &'static str,
    source: Box<dyn Error + Send + Sync>,
}

impl PersistenceError {
    fn new<E>(stage: &'static str, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        Self {
            stage,
            source: Box::new(source),
        }
    }

    fn stage(&self) -> &'static str {
        self.stage
    }

    fn io_kind(&self) -> Option<io::ErrorKind> {
        self.source.downcast_ref::<io::Error>().map(io::Error::kind)
    }
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "state persistence failed during {}", self.stage)
    }
}

impl Error for PersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl From<io::Error> for PersistenceError {
    fn from(error: io::Error) -> Self {
        Self::new("filesystem operation", error)
    }
}

#[derive(Debug)]
enum StateLoadError {
    ReadFailed(io::Error),
    RecoveryRequired,
    UnsupportedFutureSchema {
        found: u64,
        supported: u32,
    },
    BackupFailed {
        purpose: &'static str,
        source: PersistenceError,
    },
    MigrationFailed {
        from: u32,
        reason: &'static str,
    },
    PersistFailed {
        purpose: &'static str,
        source: PersistenceError,
    },
}

impl fmt::Display for StateLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFailed(_) => write!(formatter, "state load failed during read"),
            Self::RecoveryRequired => write!(
                formatter,
                "local state is invalid and requires explicit recovery"
            ),
            Self::UnsupportedFutureSchema { found, supported } => write!(
                formatter,
                "local data was created by a newer RikkaDesk schema version; found schema {found}, this build supports up to schema {supported}"
            ),
            Self::BackupFailed { purpose, .. } => {
                write!(formatter, "state load failed during {purpose} backup")
            }
            Self::MigrationFailed { from, reason } => write!(
                formatter,
                "state migration from schema {from} failed during {reason}"
            ),
            Self::PersistFailed { purpose, .. } => {
                write!(formatter, "state load failed during {purpose} persistence")
            }
        }
    }
}

impl Error for StateLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadFailed(source) => Some(source),
            Self::BackupFailed { source, .. } | Self::PersistFailed { source, .. } => Some(source),
            Self::RecoveryRequired
            | Self::UnsupportedFutureSchema { .. }
            | Self::MigrationFailed { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StateLoadOutcome {
    Loaded,
    InitializedDefault,
    Migrated { from: u32, to: u32 },
}

struct StateLoadResult {
    persisted: PersistedMockState,
    outcome: StateLoadOutcome,
}

#[derive(Clone, Copy)]
enum StateBackupKind {
    Corrupt,
    PreMigration { from: u32, to: u32 },
}

impl StateBackupKind {
    fn purpose(self) -> &'static str {
        match self {
            Self::Corrupt => "corrupt state",
            Self::PreMigration { .. } => "pre-migration state",
        }
    }

    fn file_name(self, timestamp: u64, process_id: u32, sequence: u64) -> String {
        match self {
            Self::Corrupt => format!("state.v1.corrupt.{timestamp}.{process_id}.{sequence}.json"),
            Self::PreMigration { from, to } => format!(
                "state.v1.pre-migration.v{from}-to-v{to}.{timestamp}.{process_id}.{sequence}.json"
            ),
        }
    }
}

trait StateMigrator: Send + Sync {
    fn migrate(&self, persisted: PersistedMockState) -> Result<PersistedMockState, StateLoadError>;
}

struct RealStateMigrator;

trait SecretStore: Send + Sync {
    fn set_secret(&self, secret_ref: &str, value: &str) -> SecretStoreResult<()>;
    fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>>;
    fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()>;

    fn secret_exists(&self, secret_ref: &str) -> SecretStoreResult<bool> {
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

    fn secret_exists(&self, secret_ref: &str) -> SecretStoreResult<bool> {
        match std_fs::metadata(self.secret_path(secret_ref)) {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
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

trait ManagedBlobFileOps: Send + Sync {
    fn create_dir_all(&self, path: &FilePath) -> io::Result<()>;
    fn path_exists(&self, path: &FilePath) -> io::Result<bool>;
    fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File>;
    fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()>;
    fn flush(&self, file: &mut std_fs::File) -> io::Result<()>;
    fn sync_all(&self, file: &std_fs::File) -> io::Result<()>;
    fn publish(&self, temp: &FilePath, final_path: &FilePath) -> io::Result<()>;
    fn remove_file(&self, path: &FilePath) -> io::Result<()>;
}

struct RealManagedBlobFileOps;

impl ManagedBlobFileOps for RealManagedBlobFileOps {
    fn create_dir_all(&self, path: &FilePath) -> io::Result<()> {
        std_fs::create_dir_all(path)
    }

    fn path_exists(&self, path: &FilePath) -> io::Result<bool> {
        match std_fs::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File> {
        std_fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    }

    fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()> {
        file.write_all(data)
    }

    fn flush(&self, file: &mut std_fs::File) -> io::Result<()> {
        file.flush()
    }

    fn sync_all(&self, file: &std_fs::File) -> io::Result<()> {
        file.sync_all()
    }

    fn publish(&self, temp: &FilePath, final_path: &FilePath) -> io::Result<()> {
        publish_managed_blob_file(temp, final_path)
    }

    fn remove_file(&self, path: &FilePath) -> io::Result<()> {
        std_fs::remove_file(path)
    }
}

#[cfg(windows)]
fn publish_managed_blob_file(temp: &FilePath, final_path: &FilePath) -> io::Result<()> {
    std_fs::rename(temp, final_path)
}

#[cfg(not(windows))]
fn publish_managed_blob_file(temp: &FilePath, final_path: &FilePath) -> io::Result<()> {
    std_fs::hard_link(temp, final_path)?;
    if let Err(error) = std_fs::remove_file(temp) {
        let _ = std_fs::remove_file(final_path);
        return Err(error);
    }
    Ok(())
}

#[derive(Clone)]
struct ManagedBlobStore {
    blobs_dir: PathBuf,
    sequence: Arc<AtomicU64>,
    file_ops: Arc<dyn ManagedBlobFileOps>,
}

#[derive(Clone)]
struct PublishedManagedBlob {
    storage_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagedBlobOperationError {
    Operation,
    Cleanup,
}

impl ManagedBlobStore {
    fn new_with_file_ops(blobs_dir: PathBuf, file_ops: Arc<dyn ManagedBlobFileOps>) -> Self {
        Self {
            blobs_dir,
            sequence: Arc::new(AtomicU64::new(1)),
            file_ops,
        }
    }

    fn final_path(&self, storage_key: &str) -> Option<PathBuf> {
        is_safe_storage_key(storage_key).then(|| self.blobs_dir.join(storage_key))
    }

    fn prepare_and_publish(
        &self,
        bytes: &[u8],
    ) -> Result<PublishedManagedBlob, ManagedBlobOperationError> {
        self.file_ops
            .create_dir_all(&self.blobs_dir)
            .map_err(|_| ManagedBlobOperationError::Operation)?;

        for _ in 0..FILE_BLOB_CREATE_ATTEMPTS {
            let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
            let timestamp = now_millis();
            let process_id = std::process::id();
            let storage_key = format!("blob-{timestamp}-{process_id}-{sequence}");
            let Some(final_path) = self.final_path(&storage_key) else {
                return Err(ManagedBlobOperationError::Operation);
            };
            if self
                .file_ops
                .path_exists(&final_path)
                .map_err(|_| ManagedBlobOperationError::Operation)?
            {
                continue;
            }

            let temp_path = self
                .blobs_dir
                .join(format!("{FILE_BLOB_TEMP_PREFIX}.{process_id}.{sequence}"));
            let mut temp = match self.file_ops.create_temp(&temp_path) {
                Ok(temp) => temp,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(ManagedBlobOperationError::Operation),
            };

            let write_result = self
                .file_ops
                .write_all(&mut temp, bytes)
                .and_then(|_| self.file_ops.flush(&mut temp))
                .and_then(|_| self.file_ops.sync_all(&temp));
            drop(temp);
            if write_result.is_err() {
                return match self.delete_path_with_retry(&temp_path) {
                    Ok(()) => Err(ManagedBlobOperationError::Operation),
                    Err(()) => Err(ManagedBlobOperationError::Cleanup),
                };
            }

            let final_exists = match self.file_ops.path_exists(&final_path) {
                Ok(exists) => exists,
                Err(_) => {
                    return match self.delete_path_with_retry(&temp_path) {
                        Ok(()) => Err(ManagedBlobOperationError::Operation),
                        Err(()) => Err(ManagedBlobOperationError::Cleanup),
                    };
                }
            };
            if final_exists {
                return match self.delete_path_with_retry(&temp_path) {
                    Ok(()) => Err(ManagedBlobOperationError::Operation),
                    Err(()) => Err(ManagedBlobOperationError::Cleanup),
                };
            }
            if self.file_ops.publish(&temp_path, &final_path).is_err() {
                return match self.delete_path_with_retry(&temp_path) {
                    Ok(()) => Err(ManagedBlobOperationError::Operation),
                    Err(()) => Err(ManagedBlobOperationError::Cleanup),
                };
            }

            return Ok(PublishedManagedBlob { storage_key });
        }

        Err(ManagedBlobOperationError::Operation)
    }

    fn delete_storage_key(&self, storage_key: &str) -> Result<(), ()> {
        let Some(path) = self.final_path(storage_key) else {
            return Err(());
        };
        self.delete_path_with_retry(&path)
    }

    fn delete_path_with_retry(&self, path: &FilePath) -> Result<(), ()> {
        for _ in 0..FILE_BLOB_CLEANUP_ATTEMPTS {
            match self.file_ops.remove_file(path) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(_) => {}
            }
        }
        Err(())
    }
}

#[derive(Clone)]
struct MockPersistence {
    state_dir: PathBuf,
    state_path: PathBuf,
    save_lock: Arc<Mutex<()>>,
    temp_seq: Arc<AtomicU64>,
    backup_seq: Arc<AtomicU64>,
    file_ops: Arc<dyn StateFileOps>,
}

impl MockPersistence {
    fn new(app_data_dir: PathBuf) -> Self {
        Self::new_with_file_ops(app_data_dir, Arc::new(RealStateFileOps))
    }

    fn new_with_file_ops(app_data_dir: PathBuf, file_ops: Arc<dyn StateFileOps>) -> Self {
        let state_dir = app_data_dir.join(PERSIST_DIR_NAME);
        let state_path = state_dir.join(STATE_FILE_NAME);

        Self {
            state_dir,
            state_path,
            save_lock: Arc::new(Mutex::new(())),
            temp_seq: Arc::new(AtomicU64::new(1)),
            backup_seq: Arc::new(AtomicU64::new(1)),
            file_ops,
        }
    }

    fn file_blobs_dir(&self) -> PathBuf {
        self.state_dir
            .join(FILES_DIR_NAME)
            .join(FILE_BLOBS_DIR_NAME)
    }

    async fn save<T>(&self, persisted: &T) -> PersistenceResult<()>
    where
        T: Serialize + ?Sized,
    {
        let _save_guard = self.save_lock.lock().await;
        self.save_locked(persisted).await
    }

    async fn save_locked<T>(&self, persisted: &T) -> PersistenceResult<()>
    where
        T: Serialize + ?Sized,
    {
        let data = serde_json::to_vec_pretty(persisted)
            .map_err(|error| PersistenceError::new("serialization", error))?;
        let temp_path = self.next_temp_path();
        let state_dir = self.state_dir.clone();
        let state_path = self.state_path.clone();
        let file_ops = self.file_ops.clone();

        tokio::task::spawn_blocking(move || {
            write_state_file_atomically(
                file_ops.as_ref(),
                &state_dir,
                &state_path,
                &temp_path,
                &data,
            )
        })
        .await
        .map_err(|error| PersistenceError::new("blocking writer", error))?
    }

    async fn read_state_bytes(&self) -> io::Result<Vec<u8>> {
        let state_path = self.state_path.clone();
        let file_ops = self.file_ops.clone();
        tokio::task::spawn_blocking(move || file_ops.read(&state_path))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state read task failed"))?
    }

    async fn write_raw_backup(
        &self,
        kind: StateBackupKind,
        bytes: &[u8],
    ) -> PersistenceResult<PathBuf> {
        let _save_guard = self.save_lock.lock().await;
        self.write_raw_backup_locked(kind, bytes).await
    }

    async fn write_raw_backup_locked(
        &self,
        kind: StateBackupKind,
        bytes: &[u8],
    ) -> PersistenceResult<PathBuf> {
        let data = Arc::new(bytes.to_vec());
        for _ in 0..STATE_BACKUP_CREATE_ATTEMPTS {
            let backup_path = self.next_backup_path(kind);
            let file_ops = self.file_ops.clone();
            let path_for_write = backup_path.clone();
            let data = data.clone();
            let result = tokio::task::spawn_blocking(move || {
                write_new_file_durably(file_ops.as_ref(), &path_for_write, data.as_slice())
            })
            .await
            .map_err(|error| PersistenceError::new("backup writer task", error))?;

            match result {
                Ok(()) => return Ok(backup_path),
                Err(error) if error.io_kind() == Some(io::ErrorKind::AlreadyExists) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(PersistenceError::new(
            "backup file creation",
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "backup filename attempts exhausted",
            ),
        ))
    }

    fn has_stale_state_temp(&self) -> bool {
        let prefix = format!("{STATE_TMP_FILE_PREFIX}.");
        let Ok(entries) = std_fs::read_dir(&self.state_dir) else {
            return false;
        };

        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
        })
    }

    fn next_temp_path(&self) -> PathBuf {
        let sequence = self.temp_seq.fetch_add(1, Ordering::Relaxed);
        self.state_dir.join(format!(
            "{STATE_TMP_FILE_PREFIX}.{}.{}",
            std::process::id(),
            sequence
        ))
    }

    fn next_backup_path(&self, kind: StateBackupKind) -> PathBuf {
        let sequence = self.backup_seq.fetch_add(1, Ordering::Relaxed);
        self.state_dir
            .join(kind.file_name(now_millis(), std::process::id(), sequence))
    }
}

trait StateFileOps: Send + Sync {
    fn read(&self, path: &FilePath) -> io::Result<Vec<u8>>;
    fn create_dir_all(&self, path: &FilePath) -> io::Result<()>;
    fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File>;
    fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()>;
    fn flush(&self, file: &mut std_fs::File) -> io::Result<()>;
    fn sync_all(&self, file: &std_fs::File) -> io::Result<()>;
    fn replace(&self, replacement: &FilePath, target: &FilePath) -> io::Result<()>;
    fn remove_file(&self, path: &FilePath) -> io::Result<()>;
}

struct RealStateFileOps;

impl StateFileOps for RealStateFileOps {
    fn read(&self, path: &FilePath) -> io::Result<Vec<u8>> {
        std_fs::read(path)
    }

    fn create_dir_all(&self, path: &FilePath) -> io::Result<()> {
        std_fs::create_dir_all(path)
    }

    fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File> {
        std_fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
    }

    fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()> {
        file.write_all(data)
    }

    fn flush(&self, file: &mut std_fs::File) -> io::Result<()> {
        file.flush()
    }

    fn sync_all(&self, file: &std_fs::File) -> io::Result<()> {
        file.sync_all()
    }

    fn replace(&self, replacement: &FilePath, target: &FilePath) -> io::Result<()> {
        replace_state_file(replacement, target)
    }

    fn remove_file(&self, path: &FilePath) -> io::Result<()> {
        std_fs::remove_file(path)
    }
}

fn write_new_file_durably(
    file_ops: &dyn StateFileOps,
    path: &FilePath,
    data: &[u8],
) -> PersistenceResult<()> {
    let mut file = file_ops
        .create_temp(path)
        .map_err(|error| PersistenceError::new("backup file creation", error))?;

    let write_result = (|| {
        file_ops
            .write_all(&mut file, data)
            .map_err(|error| PersistenceError::new("backup file write", error))?;
        file_ops
            .flush(&mut file)
            .map_err(|error| PersistenceError::new("backup file flush", error))?;
        file_ops
            .sync_all(&file)
            .map_err(|error| PersistenceError::new("backup file sync", error))?;
        Ok(())
    })();

    drop(file);

    if let Err(error) = write_result {
        let _ = file_ops.remove_file(path);
        return Err(error);
    }

    Ok(())
}

fn write_state_file_atomically(
    file_ops: &dyn StateFileOps,
    state_dir: &FilePath,
    state_path: &FilePath,
    temp_path: &FilePath,
    data: &[u8],
) -> PersistenceResult<()> {
    file_ops
        .create_dir_all(state_dir)
        .map_err(|error| PersistenceError::new("state directory creation", error))?;

    let mut temp_file = file_ops
        .create_temp(temp_path)
        .map_err(|error| PersistenceError::new("temp file creation", error))?;

    let write_result = (|| {
        file_ops
            .write_all(&mut temp_file, data)
            .map_err(|error| PersistenceError::new("temp file write", error))?;
        file_ops
            .flush(&mut temp_file)
            .map_err(|error| PersistenceError::new("temp file flush", error))?;
        file_ops
            .sync_all(&temp_file)
            .map_err(|error| PersistenceError::new("temp file sync", error))?;
        Ok(())
    })();

    drop(temp_file);

    if let Err(error) = write_result {
        let _ = file_ops.remove_file(temp_path);
        return Err(error);
    }

    if let Err(error) = file_ops.replace(temp_path, state_path) {
        let _ = file_ops.remove_file(temp_path);
        return Err(PersistenceError::new("state file replacement", error));
    }

    Ok(())
}

#[cfg(windows)]
fn replace_state_file(replacement: &FilePath, target: &FilePath) -> io::Result<()> {
    if !target.exists() {
        return std_fs::rename(replacement, target);
    }

    let target_wide = wide_path(target.as_os_str());
    let replacement_wide = wide_path(replacement.as_os_str());
    let result = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            replacement_wide.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };

    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn wide_path(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
fn replace_state_file(replacement: &FilePath, target: &FilePath) -> io::Result<()> {
    std_fs::rename(replacement, target)
}

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackupExportMode {
    ModeA,
    ModeB,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum BackupManifestMode {
    #[serde(rename = "state-only")]
    StateOnly,
    #[serde(rename = "full-local-data")]
    FullLocalData,
}

impl BackupExportMode {
    fn manifest_mode(self) -> BackupManifestMode {
        match self {
            Self::ModeA => BackupManifestMode::StateOnly,
            Self::ModeB => BackupManifestMode::FullLocalData,
        }
    }

    fn includes_managed_blobs(self) -> bool {
        matches!(self, Self::ModeB)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifestState {
    path: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifestFile {
    file_id: u64,
    package_path: String,
    mime: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    format: String,
    format_version: u32,
    mode: BackupManifestMode,
    created_at: String,
    app_version: String,
    state_schema_version: u32,
    provider_import_export_version: u32,
    secrets_included: bool,
    managed_blobs_included: bool,
    runtime_generations_included: bool,
    state: BackupManifestState,
    files: Vec<BackupManifestFile>,
}

#[allow(dead_code)]
#[derive(Debug)]
struct BackupExportPackage {
    path: PathBuf,
    manifest: BackupManifest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackupExportError {
    SnapshotInvalid,
    BlobMissing,
    BlobReadFailed,
    WriteFailed,
    HashMismatch,
    PackageValidationFailed,
    PublishFailed,
}

impl fmt::Display for BackupExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SnapshotInvalid => "backup snapshot is invalid",
            Self::BlobMissing => "backup active blob is missing",
            Self::BlobReadFailed => "backup blob could not be read safely",
            Self::WriteFailed => "backup package write failed",
            Self::HashMismatch => "backup checksum validation failed",
            Self::PackageValidationFailed => "backup package validation failed",
            Self::PublishFailed => "backup package publish failed",
        };
        formatter.write_str(message)
    }
}

impl Error for BackupExportError {}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
    staged_id_seq: u64,
    is_new_provider: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenerationOperation {
    Send,
    Regenerate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GenerationPhase {
    Reserved,
    Running,
    Finalizing,
    Stopping,
}

struct GenerationCancellation {
    cancelled: AtomicBool,
    notify: Notify,
}

impl GenerationCancellation {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        while !self.is_cancelled() {
            self.notify.notified().await;
        }
    }
}

#[derive(Clone)]
struct GenerationHandle {
    generation_id: u64,
    cancellation: Arc<GenerationCancellation>,
}

#[derive(Clone)]
struct ActiveGeneration {
    handle: GenerationHandle,
    operation: GenerationOperation,
    phase: GenerationPhase,
}

#[derive(Clone)]
struct MessageRevision {
    message_id: String,
    fingerprint: u64,
}

#[derive(Clone)]
enum GenerationCommitTarget {
    Append {
        anchor: MessageRevision,
    },
    Replace {
        anchor: MessageRevision,
        target: MessageRevision,
    },
}

#[derive(Clone)]
struct GenerationCommitPlan {
    target: GenerationCommitTarget,
}

struct InitialSendCommit {
    conversation: ConversationDto,
    plan: GenerationCommitPlan,
}

struct RegenerationPreparation {
    conversation: ConversationDto,
    request_conversation: ConversationDto,
    plan: GenerationCommitPlan,
    last_user_has_non_text_parts: bool,
}

enum GenerationStreamOutcome {
    Completed(String),
    CancelledOrStale,
}

enum ParsedOpenAiStreamLine {
    Ignore,
    Delta(String),
    Done,
}

#[derive(Debug)]
enum BeginGenerationError {
    Conflict,
}

enum StopGenerationRequest {
    Owner(GenerationHandle),
    AlreadyStopping,
    Finalizing,
    NotFound,
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

#[derive(Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageNodeDto {
    id: String,
    messages: Vec<MessageDto>,
    select_index: usize,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
    blob_store: ManagedBlobStore,
    secret_store: Arc<dyn SecretStore>,
    http_client: reqwest::Client,
    provider_secret_transaction_mutex: Mutex<()>,
    file_blob_transaction_mutex: Mutex<()>,
    backup_export_mutex: Mutex<()>,
    generation_transition_mutex: Mutex<()>,
    mutation_transaction_mutex: Mutex<()>,
    commit_barrier: RwLock<()>,
    settings: RwLock<Value>,
    conversations: RwLock<HashMap<String, ConversationDto>>,
    providers: RwLock<Vec<DesktopProviderConfig>>,
    files: RwLock<Vec<ManagedFileMetadata>>,
    active_generations: Mutex<HashMap<String, ActiveGeneration>>,
    conversation_txs: RwLock<HashMap<String, broadcast::Sender<SsePayload>>>,
    settings_tx: broadcast::Sender<SsePayload>,
    list_tx: broadcast::Sender<SsePayload>,
    seq: AtomicU64,
    generation_seq: AtomicU64,
    backup_seq: AtomicU64,
    secret_seq: AtomicU64,
    id_seq: AtomicU64,
    revision: AtomicU64,
}

impl MockApiState {
    fn new(
        persistence: MockPersistence,
        secret_store: Arc<dyn SecretStore>,
        persisted: PersistedMockState,
    ) -> Self {
        Self::new_with_blob_file_ops(
            persistence,
            secret_store,
            persisted,
            Arc::new(RealManagedBlobFileOps),
        )
    }

    fn new_with_blob_file_ops(
        persistence: MockPersistence,
        secret_store: Arc<dyn SecretStore>,
        mut persisted: PersistedMockState,
        blob_file_ops: Arc<dyn ManagedBlobFileOps>,
    ) -> Self {
        sync_settings_with_desktop_providers(&mut persisted.settings, &persisted.providers);
        for conversation in persisted.conversations.values_mut() {
            conversation.is_generating = false;
        }
        let initial_id_seq = persisted
            .id_seq
            .max(max_persisted_id_seq(&persisted.conversations))
            .max(max_persisted_file_id(&persisted.files))
            .max(1);
        let (settings_tx, _) = broadcast::channel(64);
        let (list_tx, _) = broadcast::channel(64);
        let blob_store =
            ManagedBlobStore::new_with_file_ops(persistence.file_blobs_dir(), blob_file_ops);

        Self {
            persistence,
            blob_store,
            secret_store,
            http_client: reqwest::Client::builder()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            provider_secret_transaction_mutex: Mutex::new(()),
            file_blob_transaction_mutex: Mutex::new(()),
            backup_export_mutex: Mutex::new(()),
            generation_transition_mutex: Mutex::new(()),
            mutation_transaction_mutex: Mutex::new(()),
            commit_barrier: RwLock::new(()),
            settings: RwLock::new(persisted.settings),
            conversations: RwLock::new(persisted.conversations),
            providers: RwLock::new(persisted.providers),
            files: RwLock::new(persisted.files),
            active_generations: Mutex::new(HashMap::new()),
            conversation_txs: RwLock::new(HashMap::new()),
            settings_tx,
            list_tx,
            seq: AtomicU64::new(1),
            generation_seq: AtomicU64::new(1),
            backup_seq: AtomicU64::new(1),
            secret_seq: AtomicU64::new(1),
            id_seq: AtomicU64::new(initial_id_seq),
            revision: AtomicU64::new(0),
        }
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn next_provider_secret_ref(&self, provider_id: &str) -> String {
        let sequence = self.secret_seq.fetch_add(1, Ordering::Relaxed) + 1;
        format!(
            "{PROVIDER_SECRET_REF_PREFIX}{provider_id}:api-key:{}:{}:{sequence}",
            now_millis(),
            std::process::id()
        )
    }
}

pub async fn start(app_data_dir: PathBuf) -> Result<MockApiHandle, Box<dyn std::error::Error>> {
    let secret_store = create_secret_store(&app_data_dir);
    let persistence = MockPersistence::new(app_data_dir);
    let loaded = load_persisted_state(&persistence).await?;
    match loaded.outcome {
        StateLoadOutcome::Loaded => {}
        StateLoadOutcome::InitializedDefault => {
            eprintln!("RikkaDesk local state initialized");
        }
        StateLoadOutcome::Migrated { from, to } => {
            eprintln!("RikkaDesk local state migrated from schema {from} to schema {to}");
        }
    }
    let state = Arc::new(MockApiState::new(
        persistence,
        secret_store,
        loaded.persisted,
    ));
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

async fn load_persisted_state(
    persistence: &MockPersistence,
) -> Result<StateLoadResult, StateLoadError> {
    load_persisted_state_with_migrator(persistence, &RealStateMigrator).await
}

async fn load_persisted_state_with_migrator(
    persistence: &MockPersistence,
    migrator: &dyn StateMigrator,
) -> Result<StateLoadResult, StateLoadError> {
    if persistence.has_stale_state_temp() {
        eprintln!("RikkaDesk stale state temp file detected and ignored");
    }

    let bytes = match persistence.read_state_bytes().await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let persisted = default_persisted_state();
            persistence
                .save(&persisted)
                .await
                .map_err(|source| StateLoadError::PersistFailed {
                    purpose: "default initialization",
                    source,
                })?;
            return Ok(StateLoadResult {
                persisted,
                outcome: StateLoadOutcome::InitializedDefault,
            });
        }
        Err(error) => return Err(StateLoadError::ReadFailed(error)),
    };

    let raw_value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(_) => return Err(preserve_corrupt_state(persistence, &bytes).await),
    };
    let Some(schema_version) = raw_value.get("schemaVersion").and_then(Value::as_u64) else {
        return Err(preserve_corrupt_state(persistence, &bytes).await);
    };

    if schema_version > u64::from(STATE_SCHEMA_VERSION) {
        return Err(StateLoadError::UnsupportedFutureSchema {
            found: schema_version,
            supported: STATE_SCHEMA_VERSION,
        });
    }
    if schema_version == 0 {
        return Err(preserve_corrupt_state(persistence, &bytes).await);
    }

    let persisted = match serde_json::from_value::<PersistedMockState>(raw_value) {
        Ok(persisted) => persisted,
        Err(_) => return Err(preserve_corrupt_state(persistence, &bytes).await),
    };

    if persisted.schema_version == STATE_SCHEMA_VERSION {
        let mut persisted = persisted;
        normalize_desktop_providers(&mut persisted.providers);
        sync_settings_with_desktop_providers(&mut persisted.settings, &persisted.providers);
        ensure_current_model_exists(&mut persisted.settings);
        return Ok(StateLoadResult {
            persisted,
            outcome: StateLoadOutcome::Loaded,
        });
    }

    if !matches!(
        persisted.schema_version,
        LEGACY_STATE_SCHEMA_VERSION
            | PREVIOUS_STATE_SCHEMA_VERSION
            | MULTI_MODEL_STATE_SCHEMA_VERSION
            | CUSTOM_REQUEST_CONFIG_STATE_SCHEMA_VERSION
            | FILE_METADATA_STATE_SCHEMA_VERSION
    ) {
        return Err(preserve_corrupt_state(persistence, &bytes).await);
    }

    let from = persisted.schema_version;
    let _save_guard = persistence.save_lock.lock().await;
    persistence
        .write_raw_backup_locked(
            StateBackupKind::PreMigration {
                from,
                to: STATE_SCHEMA_VERSION,
            },
            &bytes,
        )
        .await
        .map_err(|source| StateLoadError::BackupFailed {
            purpose: StateBackupKind::PreMigration {
                from,
                to: STATE_SCHEMA_VERSION,
            }
            .purpose(),
            source,
        })?;

    let migrated = migrator.migrate(persisted)?;
    persistence
        .save_locked(&migrated)
        .await
        .map_err(|source| StateLoadError::PersistFailed {
            purpose: "migration",
            source,
        })?;

    Ok(StateLoadResult {
        persisted: migrated,
        outcome: StateLoadOutcome::Migrated {
            from,
            to: STATE_SCHEMA_VERSION,
        },
    })
}

async fn preserve_corrupt_state(persistence: &MockPersistence, bytes: &[u8]) -> StateLoadError {
    match persistence
        .write_raw_backup(StateBackupKind::Corrupt, bytes)
        .await
    {
        Ok(_) => StateLoadError::RecoveryRequired,
        Err(source) => StateLoadError::BackupFailed {
            purpose: StateBackupKind::Corrupt.purpose(),
            source,
        },
    }
}

#[cfg(test)]
async fn persist_mock_state(state: &Arc<MockApiState>) -> PersistenceResult<()> {
    let _mutation_guard = state.mutation_transaction_mutex.lock().await;
    persist_live_state_while_mutation_locked(state).await
}

async fn persisted_snapshot_from_live(state: &MockApiState) -> PersistedMockState {
    let _commit_guard = state.commit_barrier.read().await;

    // Keep this component order identical to commit_persisted_snapshot_to_live.
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

    PersistedMockState {
        schema_version: STATE_SCHEMA_VERSION,
        saved_at: now_millis(),
        id_seq,
        settings,
        conversations,
        providers,
        files,
    }
}

#[allow(dead_code)]
async fn export_backup_package(
    state: &Arc<MockApiState>,
    destination_root: &FilePath,
    mode: BackupExportMode,
) -> Result<BackupExportPackage, BackupExportError> {
    let _backup_guard = state.backup_export_mutex.lock().await;
    let destination_root = destination_root.to_path_buf();
    let sequence_start = state
        .backup_seq
        .fetch_add(BACKUP_CREATE_ATTEMPTS as u64, Ordering::Relaxed);

    match mode {
        BackupExportMode::ModeA => {
            let snapshot = portable_backup_snapshot(state).await?;
            tokio::task::spawn_blocking(move || {
                build_and_publish_backup_package(
                    &destination_root,
                    mode,
                    snapshot,
                    None,
                    sequence_start,
                )
            })
            .await
            .map_err(|_| BackupExportError::WriteFailed)?
        }
        BackupExportMode::ModeB => {
            let _file_guard = state.file_blob_transaction_mutex.lock().await;
            let snapshot = portable_backup_snapshot(state).await?;
            let blob_root = state.blob_store.blobs_dir.clone();
            tokio::task::spawn_blocking(move || {
                build_and_publish_backup_package(
                    &destination_root,
                    mode,
                    snapshot,
                    Some(&blob_root),
                    sequence_start,
                )
            })
            .await
            .map_err(|_| BackupExportError::WriteFailed)?
        }
    }
}

async fn portable_backup_snapshot(
    state: &Arc<MockApiState>,
) -> Result<PersistedMockState, BackupExportError> {
    let snapshot = {
        let _mutation_guard = state.mutation_transaction_mutex.lock().await;
        persisted_snapshot_from_live(state).await
    };
    sanitize_and_validate_portable_backup_snapshot(snapshot)
}

fn sanitize_and_validate_portable_backup_snapshot(
    mut snapshot: PersistedMockState,
) -> Result<PersistedMockState, BackupExportError> {
    if snapshot.schema_version != STATE_SCHEMA_VERSION {
        return Err(BackupExportError::SnapshotInvalid);
    }

    for conversation in snapshot.conversations.values_mut() {
        conversation.is_generating = false;
        if conversation.messages.iter().any(|node| {
            node.messages
                .iter()
                .any(|message| message.parts.iter().any(backup_part_is_unsafe))
        }) {
            return Err(BackupExportError::SnapshotInvalid);
        }
    }

    let mut settings_secret_index = 0_u64;
    sanitize_backup_secret_fields(&mut snapshot.settings, &mut settings_secret_index);
    for (index, provider) in snapshot.providers.iter_mut().enumerate() {
        provider.secret_ref =
            format!("{PROVIDER_SECRET_REF_PREFIX}{BACKUP_SECRET_REF_MARKER}-{index}:api-key");
    }
    sync_settings_with_desktop_providers(&mut snapshot.settings, &snapshot.providers);

    validate_persisted_state_for_transaction(&snapshot)
        .map_err(|_| BackupExportError::SnapshotInvalid)?;
    validate_provider_metadata_transaction(&snapshot)
        .map_err(|_| BackupExportError::SnapshotInvalid)?;
    validate_file_metadata_transaction(&snapshot)
        .map_err(|_| BackupExportError::SnapshotInvalid)?;

    Ok(snapshot)
}

fn sanitize_backup_secret_fields(value: &mut Value, sequence: &mut u64) {
    match value {
        Value::Object(object) => {
            for (key, field) in object {
                if key == "secretRef" {
                    *field = json!(format!("{BACKUP_SECRET_REF_MARKER}-settings-{sequence}"));
                    *sequence = sequence.saturating_add(1);
                } else if key == "hasSecret" {
                    *field = Value::Bool(false);
                } else {
                    sanitize_backup_secret_fields(field, sequence);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                sanitize_backup_secret_fields(item, sequence);
            }
        }
        _ => {}
    }
}

fn backup_part_is_unsafe(part: &Value) -> bool {
    let Some(object) = part.as_object() else {
        return false;
    };
    let Some(part_type) = object.get("type").and_then(Value::as_str) else {
        return false;
    };
    if !matches!(part_type, "image" | "video" | "audio" | "document") {
        return false;
    }
    object
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|url| {
            let value = url.trim();
            let lower = value.to_ascii_lowercase();
            lower.starts_with("data:")
                || lower.starts_with("file:")
                || value.starts_with("\\\\")
                || value.as_bytes().get(1) == Some(&b':')
        })
}

fn build_and_publish_backup_package(
    destination_root: &FilePath,
    mode: BackupExportMode,
    snapshot: PersistedMockState,
    blob_root: Option<&FilePath>,
    sequence_start: u64,
) -> Result<BackupExportPackage, BackupExportError> {
    let destination_root = validate_backup_destination_root(destination_root)?;

    for attempt in 0..BACKUP_CREATE_ATTEMPTS {
        let sequence = sequence_start + attempt as u64;
        let timestamp = now_millis();
        let temp_path = destination_root.join(format!(
            "{BACKUP_TEMP_DIR_PREFIX}.{}.{}",
            std::process::id(),
            sequence
        ));
        let final_path =
            destination_root.join(format!("{BACKUP_FINAL_DIR_PREFIX}-{timestamp}-{sequence}"));
        if temp_path.exists() || final_path.exists() {
            continue;
        }
        match std_fs::create_dir(&temp_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(BackupExportError::WriteFailed),
        }

        let build_result = write_backup_package_contents(&temp_path, mode, &snapshot, blob_root)
            .and_then(|manifest| {
                validate_backup_package(&temp_path)?;
                publish_backup_directory(&temp_path, &final_path)?;
                Ok(BackupExportPackage {
                    path: final_path.clone(),
                    manifest,
                })
            });

        if build_result.is_err() {
            let _ = std_fs::remove_dir_all(&temp_path);
        }
        return build_result;
    }

    Err(BackupExportError::PublishFailed)
}

fn validate_backup_destination_root(
    destination_root: &FilePath,
) -> Result<PathBuf, BackupExportError> {
    let metadata =
        std_fs::symlink_metadata(destination_root).map_err(|_| BackupExportError::WriteFailed)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupExportError::WriteFailed);
    }
    std_fs::canonicalize(destination_root).map_err(|_| BackupExportError::WriteFailed)
}

fn write_backup_package_contents(
    package_root: &FilePath,
    mode: BackupExportMode,
    snapshot: &PersistedMockState,
    blob_root: Option<&FilePath>,
) -> Result<BackupManifest, BackupExportError> {
    let mut state_bytes =
        serde_json::to_vec_pretty(snapshot).map_err(|_| BackupExportError::SnapshotInvalid)?;
    state_bytes.push(b'\n');
    let state_path = package_root.join(BACKUP_STATE_FILE_NAME);
    write_new_backup_file(&state_path, &state_bytes)?;
    let (state_sha256, state_size) = sha256_file(&state_path)?;

    let mut manifest_files = Vec::new();
    if mode.includes_managed_blobs() {
        let blob_root = blob_root.ok_or(BackupExportError::SnapshotInvalid)?;
        let package_blob_root = package_root.join(BACKUP_BLOBS_DIR_NAME);
        std_fs::create_dir(&package_blob_root).map_err(|_| BackupExportError::WriteFailed)?;

        let mut active_files = snapshot
            .files
            .iter()
            .filter(|file| file.deleted_at.is_none())
            .cloned()
            .collect::<Vec<_>>();
        active_files.sort_by_key(|file| file.id);
        if !active_files.is_empty() {
            validate_backup_blob_root(blob_root)?;
        }
        for file in active_files {
            let source = managed_blob_source_for_backup(blob_root, &file)?;
            let package_path = format!("{BACKUP_BLOBS_DIR_NAME}/file-{}.blob", file.id);
            let destination = package_root.join(&package_path);
            let (sha256, size_bytes) = copy_backup_blob(&source, &destination)?;
            if size_bytes != file.size_bytes
                || file
                    .sha256
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .is_some_and(|value| !value.eq_ignore_ascii_case(&sha256))
            {
                return Err(BackupExportError::HashMismatch);
            }
            manifest_files.push(BackupManifestFile {
                file_id: file.id,
                package_path,
                mime: file.mime,
                size_bytes,
                sha256,
            });
        }
    }

    let manifest = BackupManifest {
        format: BACKUP_FORMAT_NAME.to_string(),
        format_version: BACKUP_FORMAT_VERSION,
        mode: mode.manifest_mode(),
        created_at: now_iso(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        state_schema_version: STATE_SCHEMA_VERSION,
        provider_import_export_version: PROVIDER_IMPORT_EXPORT_VERSION,
        secrets_included: false,
        managed_blobs_included: mode.includes_managed_blobs(),
        runtime_generations_included: false,
        state: BackupManifestState {
            path: BACKUP_STATE_FILE_NAME.to_string(),
            sha256: state_sha256,
            size_bytes: state_size,
        },
        files: manifest_files,
    };
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|_| BackupExportError::WriteFailed)?;
    manifest_bytes.push(b'\n');
    let manifest_path = package_root.join(BACKUP_MANIFEST_FILE_NAME);
    write_new_backup_file(&manifest_path, &manifest_bytes)?;
    let (manifest_sha256, _) = sha256_file(&manifest_path)?;

    let mut checksum_entries = vec![
        (manifest_sha256, BACKUP_MANIFEST_FILE_NAME.to_string()),
        (
            manifest.state.sha256.clone(),
            BACKUP_STATE_FILE_NAME.to_string(),
        ),
    ];
    checksum_entries.extend(
        manifest
            .files
            .iter()
            .map(|file| (file.sha256.clone(), file.package_path.clone())),
    );
    checksum_entries.sort_by(|left, right| left.1.cmp(&right.1));
    let checksums = checksum_entries
        .into_iter()
        .map(|(hash, path)| format!("{hash}  {path}\n"))
        .collect::<String>();
    write_new_backup_file(
        &package_root.join(BACKUP_CHECKSUM_FILE_NAME),
        checksums.as_bytes(),
    )?;

    Ok(manifest)
}

fn validate_backup_blob_root(blob_root: &FilePath) -> Result<PathBuf, BackupExportError> {
    let metadata =
        std_fs::symlink_metadata(blob_root).map_err(|_| BackupExportError::BlobMissing)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupExportError::BlobReadFailed);
    }
    std_fs::canonicalize(blob_root).map_err(|_| BackupExportError::BlobReadFailed)
}

fn managed_blob_source_for_backup(
    blob_root: &FilePath,
    file: &ManagedFileMetadata,
) -> Result<PathBuf, BackupExportError> {
    if file.deleted_at.is_some()
        || !is_safe_storage_key(&file.storage_key)
        || file.relative_path
            != format!(
                "{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{}",
                file.storage_key
            )
    {
        return Err(BackupExportError::SnapshotInvalid);
    }
    let canonical_root = validate_backup_blob_root(blob_root)?;
    let source = blob_root.join(&file.storage_key);
    let metadata = match std_fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(BackupExportError::BlobMissing)
        }
        Err(_) => return Err(BackupExportError::BlobReadFailed),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackupExportError::BlobReadFailed);
    }
    let canonical_source =
        std_fs::canonicalize(&source).map_err(|_| BackupExportError::BlobReadFailed)?;
    if canonical_source.parent() != Some(canonical_root.as_path()) {
        return Err(BackupExportError::BlobReadFailed);
    }
    Ok(canonical_source)
}

fn copy_backup_blob(
    source: &FilePath,
    destination: &FilePath,
) -> Result<(String, u64), BackupExportError> {
    let mut source_file =
        std_fs::File::open(source).map_err(|_| BackupExportError::BlobReadFailed)?;
    let mut destination_file = std_fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| BackupExportError::WriteFailed)?;
    let copy_result = (|| {
        io::copy(&mut source_file, &mut destination_file)
            .map_err(|_| BackupExportError::BlobReadFailed)?;
        destination_file
            .flush()
            .map_err(|_| BackupExportError::WriteFailed)?;
        destination_file
            .sync_all()
            .map_err(|_| BackupExportError::WriteFailed)?;
        Ok(())
    })();
    drop(destination_file);
    if let Err(error) = copy_result {
        let _ = std_fs::remove_file(destination);
        return Err(error);
    }
    sha256_file(destination)
}

fn write_new_backup_file(path: &FilePath, bytes: &[u8]) -> Result<(), BackupExportError> {
    let mut file = std_fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| BackupExportError::WriteFailed)?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|_| BackupExportError::WriteFailed)?;
        file.flush().map_err(|_| BackupExportError::WriteFailed)?;
        file.sync_all()
            .map_err(|_| BackupExportError::WriteFailed)?;
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = std_fs::remove_file(path);
    }
    result
}

fn sha256_file(path: &FilePath) -> Result<(String, u64), BackupExportError> {
    let metadata = std_fs::symlink_metadata(path).map_err(|_| BackupExportError::HashMismatch)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackupExportError::HashMismatch);
    }
    let mut file = std_fs::File::open(path).map_err(|_| BackupExportError::HashMismatch)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| BackupExportError::HashMismatch)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or(BackupExportError::HashMismatch)?;
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

fn publish_backup_directory(
    temp_path: &FilePath,
    final_path: &FilePath,
) -> Result<(), BackupExportError> {
    if final_path.exists() {
        return Err(BackupExportError::PublishFailed);
    }
    std_fs::rename(temp_path, final_path).map_err(|_| BackupExportError::PublishFailed)
}

fn validate_backup_package(package_root: &FilePath) -> Result<BackupManifest, BackupExportError> {
    let root_metadata = std_fs::symlink_metadata(package_root)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(BackupExportError::PackageValidationFailed);
    }

    let manifest_bytes = read_regular_backup_file(&package_root.join(BACKUP_MANIFEST_FILE_NAME))?;
    let manifest: BackupManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    if manifest.format != BACKUP_FORMAT_NAME
        || manifest.format_version != BACKUP_FORMAT_VERSION
        || manifest.app_version != env!("CARGO_PKG_VERSION")
        || manifest.state_schema_version != STATE_SCHEMA_VERSION
        || manifest.provider_import_export_version != PROVIDER_IMPORT_EXPORT_VERSION
        || manifest.secrets_included
        || manifest.runtime_generations_included
        || manifest.state.path != BACKUP_STATE_FILE_NAME
        || !is_safe_backup_relative_path(&manifest.state.path)
    {
        return Err(BackupExportError::PackageValidationFailed);
    }

    let state_bytes = read_regular_backup_file(&package_root.join(&manifest.state.path))?;
    let (state_hash, state_size) = sha256_file(&package_root.join(&manifest.state.path))?;
    if state_hash != manifest.state.sha256 || state_size != manifest.state.size_bytes {
        return Err(BackupExportError::HashMismatch);
    }
    let state: PersistedMockState = serde_json::from_slice(&state_bytes)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    validate_persisted_state_for_transaction(&state)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    validate_provider_metadata_transaction(&state)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    validate_file_metadata_transaction(&state)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    if state.schema_version != manifest.state_schema_version
        || state
            .conversations
            .values()
            .any(|conversation| conversation.is_generating)
        || state.providers.iter().any(|provider| {
            !provider.secret_ref.contains(BACKUP_SECRET_REF_MARKER)
                || !is_controlled_provider_secret_ref(&provider.secret_ref)
        })
    {
        return Err(BackupExportError::PackageValidationFailed);
    }

    let mut active_file_ids = state
        .files
        .iter()
        .filter(|file| file.deleted_at.is_none())
        .map(|file| file.id)
        .collect::<Vec<_>>();
    active_file_ids.sort_unstable();
    let manifest_file_ids = manifest
        .files
        .iter()
        .map(|file| file.file_id)
        .collect::<Vec<_>>();
    if !manifest_file_ids.windows(2).all(|ids| ids[0] < ids[1]) {
        return Err(BackupExportError::PackageValidationFailed);
    }

    match manifest.mode {
        BackupManifestMode::StateOnly => {
            if manifest.managed_blobs_included
                || !manifest.files.is_empty()
                || package_root.join(BACKUP_BLOBS_DIR_NAME).exists()
            {
                return Err(BackupExportError::PackageValidationFailed);
            }
        }
        BackupManifestMode::FullLocalData => {
            if !manifest.managed_blobs_included || manifest_file_ids != active_file_ids {
                return Err(BackupExportError::PackageValidationFailed);
            }
            validate_backup_blob_entries(package_root, &manifest.files)?;
        }
    }

    let manifest_path = package_root.join(BACKUP_MANIFEST_FILE_NAME);
    let (manifest_hash, _) = sha256_file(&manifest_path)?;
    let checksum_bytes = read_regular_backup_file(&package_root.join(BACKUP_CHECKSUM_FILE_NAME))?;
    let checksum_text = std::str::from_utf8(&checksum_bytes)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    let checksums = parse_backup_checksums(checksum_text)?;
    let mut expected_checksums = HashMap::from([
        (BACKUP_MANIFEST_FILE_NAME.to_string(), manifest_hash),
        (
            BACKUP_STATE_FILE_NAME.to_string(),
            manifest.state.sha256.clone(),
        ),
    ]);
    for file in &manifest.files {
        if !is_safe_backup_relative_path(&file.package_path)
            || file.package_path != format!("{BACKUP_BLOBS_DIR_NAME}/file-{}.blob", file.file_id)
            || expected_checksums
                .insert(file.package_path.clone(), file.sha256.clone())
                .is_some()
        {
            return Err(BackupExportError::PackageValidationFailed);
        }
    }
    if checksums != expected_checksums {
        return Err(BackupExportError::HashMismatch);
    }

    validate_backup_root_entries(package_root, manifest.managed_blobs_included)?;
    Ok(manifest)
}

fn validate_backup_root_entries(
    package_root: &FilePath,
    includes_blobs: bool,
) -> Result<(), BackupExportError> {
    let mut expected = HashSet::from([
        BACKUP_MANIFEST_FILE_NAME.to_string(),
        BACKUP_STATE_FILE_NAME.to_string(),
        BACKUP_CHECKSUM_FILE_NAME.to_string(),
    ]);
    if includes_blobs {
        expected.insert(BACKUP_BLOBS_DIR_NAME.to_string());
    }
    let actual = backup_directory_entry_names(package_root)?;
    if actual != expected {
        return Err(BackupExportError::PackageValidationFailed);
    }
    Ok(())
}

fn validate_backup_blob_entries(
    package_root: &FilePath,
    files: &[BackupManifestFile],
) -> Result<(), BackupExportError> {
    let blob_root = package_root.join(BACKUP_BLOBS_DIR_NAME);
    let metadata = std_fs::symlink_metadata(&blob_root)
        .map_err(|_| BackupExportError::PackageValidationFailed)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BackupExportError::PackageValidationFailed);
    }
    let expected = files
        .iter()
        .map(|file| format!("file-{}.blob", file.file_id))
        .collect::<HashSet<_>>();
    if backup_directory_entry_names(&blob_root)? != expected {
        return Err(BackupExportError::PackageValidationFailed);
    }
    for file in files {
        let path = package_root.join(&file.package_path);
        let (hash, size) = sha256_file(&path)?;
        if hash != file.sha256 || size != file.size_bytes {
            return Err(BackupExportError::HashMismatch);
        }
    }
    Ok(())
}

fn backup_directory_entry_names(
    directory: &FilePath,
) -> Result<HashSet<String>, BackupExportError> {
    let mut names = HashSet::new();
    let entries =
        std_fs::read_dir(directory).map_err(|_| BackupExportError::PackageValidationFailed)?;
    for entry in entries {
        let entry = entry.map_err(|_| BackupExportError::PackageValidationFailed)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| BackupExportError::PackageValidationFailed)?;
        if !names.insert(name) {
            return Err(BackupExportError::PackageValidationFailed);
        }
    }
    Ok(names)
}

fn read_regular_backup_file(path: &FilePath) -> Result<Vec<u8>, BackupExportError> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|_| BackupExportError::PackageValidationFailed)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackupExportError::PackageValidationFailed);
    }
    std_fs::read(path).map_err(|_| BackupExportError::PackageValidationFailed)
}

fn parse_backup_checksums(text: &str) -> Result<HashMap<String, String>, BackupExportError> {
    let mut checksums = HashMap::new();
    let mut previous_path: Option<&str> = None;
    for line in text.lines() {
        let (hash, path) = line
            .split_once("  ")
            .ok_or(BackupExportError::PackageValidationFailed)?;
        if hash.len() != 64
            || !hash
                .chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
            || !is_safe_backup_relative_path(path)
            || path == BACKUP_CHECKSUM_FILE_NAME
            || previous_path.is_some_and(|previous| previous >= path)
            || checksums
                .insert(path.to_string(), hash.to_string())
                .is_some()
        {
            return Err(BackupExportError::PackageValidationFailed);
        }
        previous_path = Some(path);
    }
    if checksums.is_empty() {
        return Err(BackupExportError::PackageValidationFailed);
    }
    Ok(checksums)
}

fn is_safe_backup_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !FilePath::new(path).is_absolute()
        && FilePath::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

#[cfg(test)]
async fn persist_live_state_while_mutation_locked(state: &MockApiState) -> PersistenceResult<()> {
    let persisted = persisted_snapshot_from_live(state).await;
    state.persistence.save(&persisted).await
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum PureStateMutationScope {
    Settings,
    Conversations,
    SettingsAndConversations,
    ConversationGeneration,
    ProviderMetadata,
    FileMetadata,
}

#[derive(Debug)]
enum StateMutationError {
    BadRequest(&'static str),
    NotFound(&'static str),
    Conflict(&'static str),
    Validation(&'static str),
    Persistence(PersistenceError),
}

impl From<PersistenceError> for StateMutationError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

async fn transact_persisted_state<T, F>(
    state: &Arc<MockApiState>,
    scope: PureStateMutationScope,
    mutation: F,
) -> Result<T, StateMutationError>
where
    F: FnOnce(&mut PersistedMockState) -> Result<T, StateMutationError>,
{
    let _mutation_guard = state.mutation_transaction_mutex.lock().await;
    let before = persisted_snapshot_from_live(state).await;
    let mut staged = before.clone();
    let result = mutation(&mut staged)?;

    staged.saved_at = now_millis();
    validate_pure_state_transaction(&before, &staged, scope)?;
    state.persistence.save(&staged).await?;
    commit_persisted_snapshot_to_live(state, staged).await;

    Ok(result)
}

fn validate_pure_state_transaction(
    before: &PersistedMockState,
    staged: &PersistedMockState,
    scope: PureStateMutationScope,
) -> Result<(), StateMutationError> {
    validate_persisted_state_for_transaction(staged)?;

    if matches!(scope, PureStateMutationScope::ProviderMetadata) {
        if staged.id_seq < before.id_seq {
            return Err(StateMutationError::Validation(
                "Provider mutation lowered the ID high-water mark",
            ));
        }
        if staged.conversations != before.conversations || staged.files != before.files {
            return Err(StateMutationError::Validation(
                "Provider mutation crossed a protected component boundary",
            ));
        }
        validate_provider_metadata_transaction(staged)?;
    } else if matches!(scope, PureStateMutationScope::FileMetadata) {
        if staged.id_seq < before.id_seq {
            return Err(StateMutationError::Validation(
                "File mutation lowered the ID high-water mark",
            ));
        }
        if staged.settings != before.settings
            || staged.conversations != before.conversations
            || staged.providers != before.providers
        {
            return Err(StateMutationError::Validation(
                "File mutation crossed a protected component boundary",
            ));
        }
        validate_file_metadata_transaction(staged)?;
    } else if matches!(scope, PureStateMutationScope::ConversationGeneration) {
        if staged.id_seq < before.id_seq {
            return Err(StateMutationError::Validation(
                "Generation mutation lowered the ID high-water mark",
            ));
        }
        if staged.settings != before.settings
            || staged.providers != before.providers
            || staged.files != before.files
        {
            return Err(StateMutationError::Validation(
                "Generation mutation crossed a protected component boundary",
            ));
        }
    } else {
        if staged.id_seq != before.id_seq {
            return Err(StateMutationError::Validation(
                "Pure state mutation changed the ID high-water mark",
            ));
        }
        if staged.providers != before.providers || staged.files != before.files {
            return Err(StateMutationError::Validation(
                "Pure state mutation crossed a protected component boundary",
            ));
        }
        if matches!(scope, PureStateMutationScope::Settings)
            && staged.conversations != before.conversations
        {
            return Err(StateMutationError::Validation(
                "Settings mutation changed conversations",
            ));
        }
        if matches!(scope, PureStateMutationScope::Conversations)
            && staged.settings != before.settings
        {
            return Err(StateMutationError::Validation(
                "Conversation mutation changed settings",
            ));
        }
    }

    Ok(())
}

fn validate_file_metadata_transaction(
    persisted: &PersistedMockState,
) -> Result<(), StateMutationError> {
    let mut file_ids = HashSet::new();
    let mut storage_keys = HashSet::new();

    for file in &persisted.files {
        let expected_relative_path = format!(
            "{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{}",
            file.storage_key
        );
        if file.id == 0 || !file_ids.insert(file.id) {
            return Err(StateMutationError::Validation(
                "Staged managed file identifiers are invalid",
            ));
        }
        if !is_safe_storage_key(&file.storage_key)
            || !storage_keys.insert(file.storage_key.as_str())
            || file.relative_path != expected_relative_path
        {
            return Err(StateMutationError::Validation(
                "Staged managed file storage metadata is invalid",
            ));
        }
    }
    if persisted.id_seq < max_persisted_file_id(&persisted.files) {
        return Err(StateMutationError::Validation(
            "Staged managed file ID high-water mark is invalid",
        ));
    }

    Ok(())
}

fn validate_provider_metadata_transaction(
    persisted: &PersistedMockState,
) -> Result<(), StateMutationError> {
    let Some(settings_providers) = persisted
        .settings
        .get("providers")
        .and_then(Value::as_array)
    else {
        return Err(StateMutationError::Validation(
            "Staged settings provider list is invalid",
        ));
    };
    let mut provider_ids = HashSet::new();
    let mut provider_secret_refs = HashSet::new();
    let mut model_record_ids = HashSet::new();

    for provider in &persisted.providers {
        if !is_safe_config_id(&provider.id) || !provider_ids.insert(provider.id.as_str()) {
            return Err(StateMutationError::Validation(
                "Staged provider identifiers are invalid",
            ));
        }
        if provider.secret_ref.is_empty()
            || provider.secret_ref.len() > 512
            || !provider_secret_refs.insert(provider.secret_ref.as_str())
        {
            return Err(StateMutationError::Validation(
                "Staged provider secret references are invalid",
            ));
        }
        if provider.models.is_empty() {
            return Err(StateMutationError::Validation(
                "Staged provider model list is empty",
            ));
        }
        for model in &provider.models {
            if !is_safe_config_id(&model.id) || !model_record_ids.insert(model.id.as_str()) {
                return Err(StateMutationError::Validation(
                    "Staged provider model identifiers are invalid",
                ));
            }
        }
        if !settings_providers.iter().any(|settings_provider| {
            settings_provider.get("id").and_then(Value::as_str) == Some(provider.id.as_str())
                && settings_provider.get("secretRef").and_then(Value::as_str)
                    == Some(provider.secret_ref.as_str())
        }) {
            return Err(StateMutationError::Validation(
                "Staged provider settings reference is inconsistent",
            ));
        }
    }

    Ok(())
}

fn validate_persisted_state_for_transaction(
    persisted: &PersistedMockState,
) -> Result<(), StateMutationError> {
    if persisted.schema_version != STATE_SCHEMA_VERSION {
        return Err(StateMutationError::Validation(
            "Staged state schema version is invalid",
        ));
    }
    let Some(settings) = persisted.settings.as_object() else {
        return Err(StateMutationError::Validation(
            "Staged settings are invalid",
        ));
    };
    if settings.get("favoriteModels").is_some_and(|value| {
        !value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string))
    }) || settings
        .get("chatModelId")
        .is_some_and(|value| !value.is_string())
        || settings
            .get("assistants")
            .is_some_and(|value| !value.is_array())
    {
        return Err(StateMutationError::Validation(
            "Staged settings references are invalid",
        ));
    }

    let mut conversation_ids = HashSet::new();
    let mut node_ids = HashSet::new();
    let mut message_ids = HashSet::new();
    for (key, conversation) in &persisted.conversations {
        if key != &conversation.id || !conversation_ids.insert(conversation.id.as_str()) {
            return Err(StateMutationError::Validation(
                "Staged conversation identifiers are invalid",
            ));
        }
        for node in &conversation.messages {
            if !node_ids.insert(node.id.as_str()) {
                return Err(StateMutationError::Validation(
                    "Staged message node identifiers are duplicated",
                ));
            }
            for message in &node.messages {
                if !message_ids.insert(message.id.as_str()) {
                    return Err(StateMutationError::Validation(
                        "Staged message identifiers are duplicated",
                    ));
                }
            }
        }
    }

    Ok(())
}

async fn commit_persisted_snapshot_to_live(state: &MockApiState, staged: PersistedMockState) {
    let _commit_guard = state.commit_barrier.write().await;

    // Fixed order for every multi-component commit: settings, conversations,
    // providers, files, then the ID and revision high-water marks.
    let mut settings = state.settings.write().await;
    let mut conversations = state.conversations.write().await;
    let mut providers = state.providers.write().await;
    let mut files = state.files.write().await;

    *settings = staged.settings;
    *conversations = staged.conversations;
    *providers = staged.providers;
    *files = staged.files;
    state.id_seq.fetch_max(staged.id_seq, Ordering::Relaxed);
    state.revision.fetch_add(1, Ordering::Release);
}

fn state_mutation_error_response(context: &'static str, error: StateMutationError) -> Response {
    match error {
        StateMutationError::BadRequest(message) => bad_request_response(message),
        StateMutationError::NotFound(message) => not_found_response(message),
        StateMutationError::Conflict(message) => conflict_response(message),
        StateMutationError::Validation(reason) => {
            debug_assert!(!reason.is_empty());
            eprintln!("RikkaDesk state transaction validation failed in {context}");
            internal_error_response("Local state validation failed")
        }
        StateMutationError::Persistence(error) => persistence_error_response(context, &error),
    }
}

fn persistence_error_response(context: &'static str, error: &PersistenceError) -> Response {
    log_persistence_error(context, error);
    internal_error_response("Local state could not be saved")
}

fn log_persistence_error(context: &'static str, error: &PersistenceError) {
    eprintln!(
        "RikkaDesk state persistence failed in {context} during {}",
        error.stage()
    );
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

    let provider_guard = state.provider_secret_transaction_mutex.lock().await;
    let mut imported_secret_refs = Vec::with_capacity(providers.len());
    for _ in 0..providers.len() {
        let secret_ref = match unused_provider_secret_ref(&state, "imported-provider") {
            Ok(secret_ref) => secret_ref,
            Err(()) => {
                eprintln!("RikkaDesk provider empty secret reference allocation failed");
                return internal_error_response("Secret store is unavailable");
            }
        };
        imported_secret_refs.push(secret_ref);
    }
    let imported = if providers.is_empty() {
        Vec::new()
    } else {
        match transact_persisted_state(
            &state,
            PureStateMutationScope::ProviderMetadata,
            move |staged| {
                let mut imported_configs = Vec::with_capacity(providers.len());
                let mut imported = Vec::with_capacity(providers.len());

                for (provider, secret_ref) in providers.into_iter().zip(imported_secret_refs) {
                    let id = next_staged_id(staged, "desktop-provider");
                    let models = provider
                        .models
                        .iter()
                        .map(|model| DesktopProviderModelConfig {
                            id: next_staged_id(staged, "desktop-model"),
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
                        secret_ref,
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

                staged.providers.extend(imported_configs);
                sync_settings_with_desktop_providers(&mut staged.settings, &staged.providers);
                Ok(imported)
            },
        )
        .await
        {
            Ok(imported) => imported,
            Err(error) => {
                drop(provider_guard);
                return state_mutation_error_response("provider import", error);
            }
        }
    };
    drop(provider_guard);

    if !imported.is_empty() {
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
    let provider_guard = state.provider_secret_transaction_mutex.lock().await;
    let BuiltDesktopProvider {
        mut provider,
        api_key,
        removed_model_ids,
        used_models_request,
        staged_id_seq,
        is_new_provider,
    } = match build_desktop_provider(&state, payload).await {
        Ok(built) => built,
        Err(response) => return response,
    };

    let previous_secret_ref = provider.secret_ref.clone();
    let prepared_secret_ref = if let Some(api_key) = api_key.as_deref() {
        let secret_ref = match unused_provider_secret_ref(&state, &provider.id) {
            Ok(secret_ref) => secret_ref,
            Err(()) => {
                eprintln!("RikkaDesk provider secret reference allocation failed");
                return internal_error_response("Secret store is unavailable");
            }
        };
        if prepare_new_secret(state.secret_store.as_ref(), &secret_ref, api_key).is_err() {
            eprintln!("RikkaDesk provider secret prepare failed");
            return internal_error_response("Secret store is unavailable");
        }
        provider.secret_ref = secret_ref.clone();
        Some(secret_ref)
    } else {
        if is_new_provider {
            provider.secret_ref = match unused_provider_secret_ref(&state, &provider.id) {
                Ok(secret_ref) => secret_ref,
                Err(()) => {
                    eprintln!("RikkaDesk provider empty secret reference allocation failed");
                    return internal_error_response("Secret store is unavailable");
                }
            };
        }
        None
    };

    let has_secret = if prepared_secret_ref.is_some() {
        true
    } else if is_new_provider {
        false
    } else {
        match state.secret_store.secret_exists(&provider.secret_ref) {
            Ok(has_secret) => has_secret,
            Err(_) => {
                eprintln!("RikkaDesk provider secret status check failed");
                return internal_error_response("Secret store is unavailable");
            }
        }
    };

    let provider_for_commit = provider.clone();
    let transaction_result = transact_persisted_state(
        &state,
        PureStateMutationScope::ProviderMetadata,
        move |staged| {
            staged.id_seq = staged.id_seq.max(staged_id_seq);
            if let Some(existing) = staged
                .providers
                .iter_mut()
                .find(|item| item.id == provider_for_commit.id)
            {
                *existing = provider_for_commit.clone();
            } else {
                staged.providers.push(provider_for_commit.clone());
            }
            sync_settings_with_desktop_providers(&mut staged.settings, &staged.providers);
            if !removed_model_ids.is_empty() {
                remove_models_from_favorites(
                    &mut staged.settings,
                    removed_model_ids.iter().map(String::as_str),
                );
            }
            if !used_models_request {
                if let Some(model_id) = provider_for_commit.primary_model_id_for_settings() {
                    set_current_model_in_settings(&mut staged.settings, model_id);
                }
            }
            ensure_current_model_exists(&mut staged.settings);
            Ok(())
        },
    )
    .await;

    if let Err(error) = transaction_result {
        let response = state_mutation_error_response("provider upsert", error);
        if let Some(secret_ref) = prepared_secret_ref.as_deref() {
            if compensate_new_secret(state.secret_store.as_ref(), secret_ref).is_err() {
                eprintln!("RikkaDesk provider secret compensation failed after state rejection");
                return internal_error_response(
                    "Provider update failed and local secret cleanup requires attention",
                );
            }
        }
        return response;
    }

    if prepared_secret_ref.is_some()
        && !is_new_provider
        && previous_secret_ref != provider.secret_ref
    {
        cleanup_secret_after_state_commit(state.secret_store.as_ref(), &previous_secret_ref);
    }
    drop(provider_guard);
    broadcast_settings_update(&state).await;

    match DesktopProviderResponse::from_config(&provider, has_secret) {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error_response(error),
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

    let provider_guard = state.provider_secret_transaction_mutex.lock().await;
    let provider = {
        let providers = state.providers.read().await;
        providers.iter().find(|item| item.id == id).cloned()
    };

    let Some(provider) = provider else {
        return not_found_response("Provider not found");
    };

    let new_secret_ref = match unused_provider_secret_ref(&state, &provider.id) {
        Ok(secret_ref) => secret_ref,
        Err(()) => {
            eprintln!("RikkaDesk provider secret reference allocation failed");
            return internal_error_response("Secret store is unavailable");
        }
    };
    if prepare_new_secret(state.secret_store.as_ref(), &new_secret_ref, &api_key).is_err() {
        eprintln!("RikkaDesk provider secret prepare failed");
        return internal_error_response("Secret store is unavailable");
    }

    let provider_id = provider.id.clone();
    let committed_secret_ref = new_secret_ref.clone();
    let transaction_result = transact_persisted_state(
        &state,
        PureStateMutationScope::ProviderMetadata,
        move |staged| {
            let Some(existing) = staged
                .providers
                .iter_mut()
                .find(|item| item.id == provider_id)
            else {
                return Err(StateMutationError::NotFound("Provider not found"));
            };
            existing.secret_ref = committed_secret_ref;
            sync_settings_with_desktop_providers(&mut staged.settings, &staged.providers);
            Ok(())
        },
    )
    .await;

    if let Err(error) = transaction_result {
        let response = state_mutation_error_response("provider key update", error);
        if compensate_new_secret(state.secret_store.as_ref(), &new_secret_ref).is_err() {
            eprintln!("RikkaDesk provider secret compensation failed after state rejection");
            return internal_error_response(
                "Provider update failed and local secret cleanup requires attention",
            );
        }
        return response;
    }

    cleanup_secret_after_state_commit(state.secret_store.as_ref(), &provider.secret_ref);
    drop(provider_guard);
    broadcast_settings_update(&state).await;
    Json(json!({ "status": "ok", "hasSecret": true })).into_response()
}

async fn delete_desktop_provider_secret(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let provider_guard = state.provider_secret_transaction_mutex.lock().await;
    let provider = {
        let providers = state.providers.read().await;
        providers.iter().find(|item| item.id == id).cloned()
    };

    let Some(provider) = provider else {
        return not_found_response("Provider not found");
    };

    let cleared_secret_ref = match unused_provider_secret_ref(&state, &provider.id) {
        Ok(secret_ref) => secret_ref,
        Err(()) => {
            eprintln!("RikkaDesk provider empty secret reference allocation failed");
            return internal_error_response("Secret store is unavailable");
        }
    };
    let provider_id = provider.id.clone();
    let committed_secret_ref = cleared_secret_ref.clone();
    let transaction_result = transact_persisted_state(
        &state,
        PureStateMutationScope::ProviderMetadata,
        move |staged| {
            let Some(existing) = staged
                .providers
                .iter_mut()
                .find(|item| item.id == provider_id)
            else {
                return Err(StateMutationError::NotFound("Provider not found"));
            };
            existing.secret_ref = committed_secret_ref;
            sync_settings_with_desktop_providers(&mut staged.settings, &staged.providers);
            Ok(())
        },
    )
    .await;

    if let Err(error) = transaction_result {
        return state_mutation_error_response("provider key clear", error);
    }

    cleanup_secret_after_state_commit(state.secret_store.as_ref(), &provider.secret_ref);
    drop(provider_guard);
    broadcast_settings_update(&state).await;
    Json(json!({ "status": "ok", "hasSecret": false })).into_response()
}

async fn delete_desktop_provider(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let provider_guard = state.provider_secret_transaction_mutex.lock().await;
    let provider = {
        let providers = state.providers.read().await;
        providers.iter().find(|item| item.id == id).cloned()
    };

    let Some(provider) = provider else {
        return not_found_response("Provider not found");
    };

    let provider_id = provider.id.clone();
    let removed_model_ids = provider
        .model_ids_for_settings()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let transaction_result = transact_persisted_state(
        &state,
        PureStateMutationScope::ProviderMetadata,
        move |staged| {
            let before_len = staged.providers.len();
            staged.providers.retain(|item| item.id != provider_id);
            if staged.providers.len() == before_len {
                return Err(StateMutationError::NotFound("Provider not found"));
            }
            sync_settings_with_desktop_providers(&mut staged.settings, &staged.providers);
            remove_models_from_favorites(
                &mut staged.settings,
                removed_model_ids.iter().map(String::as_str),
            );
            ensure_current_model_exists(&mut staged.settings);
            Ok(())
        },
    )
    .await;

    if let Err(error) = transaction_result {
        return state_mutation_error_response("provider delete", error);
    }

    cleanup_secret_after_state_commit(state.secret_store.as_ref(), &provider.secret_ref);
    drop(provider_guard);
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
    let active_ids = active_generation_conversation_ids(&state).await;
    let conversations = state.conversations.read().await;
    let mut items: Vec<_> = conversations
        .values()
        .filter(|conversation| {
            keyword.is_empty() || conversation.title.to_lowercase().contains(&keyword)
        })
        .map(|conversation| {
            let mut item = conversation.to_list_item();
            item.is_generating = active_ids.contains(&conversation.id);
            item
        })
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
    let conversation = conversation_or_virtual_for_read(&state, &id).await;
    Json(conversation)
}

async fn conversation_stream(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let conversation = conversation_or_virtual_for_read(&state, &id).await;
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
    let user_text = first_text_part(&payload.parts);
    let user_has_non_text_parts = has_non_text_parts(&payload.parts);
    let request_parts = payload.parts.clone();
    let capture_intent = is_capture_local_image_intent(&payload);
    let transition_guard = state.generation_transition_mutex.lock().await;
    let handle = match begin_generation(&state, &id, GenerationOperation::Send).await {
        Ok(handle) => handle,
        Err(BeginGenerationError::Conflict) => {
            return conflict_response("A generation is already active for this conversation")
        }
    };
    let initial = match commit_initial_user_message(
        &state,
        id.clone(),
        assistant_id.clone(),
        payload.parts,
        payload.mode_injection_ids,
        payload.lorebook_ids,
        now,
    )
    .await
    {
        Ok(initial) => initial,
        Err(error) => {
            remove_generation_if_current(&state, &id, &handle).await;
            return state_mutation_error_response("send initial message", error);
        }
    };
    if !activate_generation(&state, &id, &handle).await {
        remove_generation_if_current(&state, &id, &handle).await;
        return internal_error_response("Generation could not be started");
    }
    broadcast_conversation_snapshot(&state, &initial.conversation).await;
    broadcast_list_invalidate(&state).await;
    broadcast_generation_start(&state, &id, &handle).await;
    drop(transition_guard);

    if user_has_non_text_parts {
        if capture_intent {
            match prepare_local_image_capture_prototype(
                &state,
                &model_id,
                user_text.clone(),
                &request_parts,
            )
            .await
            {
                Ok(Some((config, messages))) => {
                    spawn_openai_vision_capture_generation(
                        state.clone(),
                        id,
                        handle,
                        initial.plan,
                        model_id,
                        config,
                        messages,
                    );
                    return Json(json!({ "status": "accepted" })).into_response();
                }
                Ok(None) => {}
                Err(LocalImageCaptureStartError::User(error)) => {
                    spawn_local_reply_generation(
                        state.clone(),
                        id,
                        handle,
                        initial.plan,
                        model_id,
                        error,
                    );
                    return Json(json!({ "status": "accepted" })).into_response();
                }
                Err(LocalImageCaptureStartError::Config) => {
                    fail_running_generation(&state, &id, &handle, "configuration").await;
                    return Json(json!({ "status": "accepted" })).into_response();
                }
            }
        }

        spawn_local_reply_generation(
            state.clone(),
            id,
            handle,
            initial.plan,
            model_id,
            LOCAL_ATTACHMENT_REPLY_TEXT.to_string(),
        );
        return Json(json!({ "status": "accepted" })).into_response();
    }

    let real_chat_config = if user_text.is_some() {
        match resolve_openai_chat_config(&state, &model_id).await {
            Ok(config) => config,
            Err(_) => {
                fail_running_generation(&state, &id, &handle, "configuration").await;
                return Json(json!({ "status": "accepted" })).into_response();
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
        spawn_local_reply_generation(
            state.clone(),
            id,
            handle,
            initial.plan,
            model_id,
            reply_text,
        );
        return Json(json!({ "status": "accepted" })).into_response();
    };

    let messages = openai_messages_from_conversation(&initial.conversation);
    if messages.is_empty() {
        spawn_local_reply_generation(
            state.clone(),
            id,
            handle,
            initial.plan,
            model_id,
            "Phase 3E currently supports text-only chat.".to_string(),
        );
        return Json(json!({ "status": "accepted" })).into_response();
    }

    spawn_openai_stream_generation(
        state.clone(),
        id,
        handle,
        initial.plan,
        model_id,
        config,
        messages,
    );

    Json(json!({ "status": "accepted" })).into_response()
}

fn is_capture_local_image_intent(payload: &SendMessageRequest) -> bool {
    payload.image_input_confirmed == Some(true)
        && payload.image_input_mode.as_deref() == Some("capture-local")
}

enum LocalImageCaptureStartError {
    User(String),
    Config,
}

impl From<String> for LocalImageCaptureStartError {
    fn from(error: String) -> Self {
        Self::User(error)
    }
}

async fn prepare_local_image_capture_prototype(
    state: &Arc<MockApiState>,
    model_id: &str,
    user_text: Option<String>,
    parts: &[Value],
) -> Result<Option<(OpenAiChatConfig, Vec<OpenAiCompatibleChatMessage>)>, LocalImageCaptureStartError>
{
    let image_file_ids = provider_bound_image_file_ids(parts)?;
    let Some(file_id) = image_file_ids.first().copied() else {
        return Ok(None);
    };

    let config = resolve_openai_chat_config(state, model_id)
        .await
        .map_err(|_| LocalImageCaptureStartError::Config)?
        .ok_or_else(|| LOCAL_IMAGE_CAPTURE_CONFIG_REQUIRED_TEXT.to_string())?;

    if !is_loopback_provider_base_url(&config.base_url) {
        return Err(LOCAL_IMAGE_CAPTURE_LOOPBACK_REQUIRED_TEXT
            .to_string()
            .into());
    }
    if !config
        .input_modalities
        .iter()
        .any(|modality| modality == MODEL_MODALITY_IMAGE)
    {
        return Err(LOCAL_IMAGE_CAPTURE_CAPABILITY_REQUIRED_TEXT
            .to_string()
            .into());
    }

    let image = managed_file_for_provider_image_input(state, file_id).await?;
    let messages = openai_vision_messages_for_current_turn(user_text, image)?;
    Ok(Some((config, messages)))
}

async fn stop_conversation(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let transition_guard = state.generation_transition_mutex.lock().await;
    let handle = match request_generation_stop(&state, &id).await {
        StopGenerationRequest::Owner(handle) => handle,
        StopGenerationRequest::AlreadyStopping => {
            return Json(json!({ "status": "stopping" })).into_response()
        }
        StopGenerationRequest::Finalizing => {
            return conflict_response("Generation is already finalizing")
        }
        StopGenerationRequest::NotFound => return not_found_response("No active generation"),
    };

    let result = commit_stopped_generation(&state, id.clone()).await;
    let removed = remove_generation_if_current(&state, &id, &handle).await;

    let updated = match result {
        Ok(updated) => updated,
        Err(error) => {
            if removed {
                broadcast_current_conversation_snapshot(&state, &id).await;
                broadcast_list_invalidate(&state).await;
                broadcast_generation_terminal(&state, &id, &handle, "failed", Some("persistence"))
                    .await;
            }
            drop(transition_guard);
            return state_mutation_error_response("stop generation", error);
        }
    };

    if removed {
        broadcast_conversation_snapshot(&state, &updated).await;
        broadcast_list_invalidate(&state).await;
        broadcast_generation_terminal(&state, &id, &handle, "stopped", None).await;
    }
    drop(transition_guard);

    Json(json!({ "status": "stopped" })).into_response()
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

    let title = title.chars().take(120).collect::<String>();
    let updated = match transact_persisted_state(
        &state,
        PureStateMutationScope::Conversations,
        move |staged| {
            let Some(conversation) = staged.conversations.get_mut(&id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
            };

            conversation.title = title;
            conversation.update_at = now_millis();
            Ok(conversation.clone())
        },
    )
    .await
    {
        Ok(updated) => updated,
        Err(error) => return state_mutation_error_response("update conversation title", error),
    };
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn toggle_conversation_pin(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let updated = match transact_persisted_state(
        &state,
        PureStateMutationScope::Conversations,
        move |staged| {
            let Some(conversation) = staged.conversations.get_mut(&id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
            };

            conversation.is_pinned = !conversation.is_pinned;
            Ok(conversation.clone())
        },
    )
    .await
    {
        Ok(updated) => updated,
        Err(error) => return state_mutation_error_response("toggle conversation pin", error),
    };
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok", "isPinned": updated.is_pinned })).into_response()
}

async fn delete_conversation(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let transition_guard = state.generation_transition_mutex.lock().await;
    if let Err(error) = transact_persisted_state(&state, PureStateMutationScope::Conversations, {
        let id = id.clone();
        move |staged| {
            staged
                .conversations
                .remove(&id)
                .ok_or(StateMutationError::NotFound("Conversation not found"))?;
            Ok(())
        }
    })
    .await
    {
        return state_mutation_error_response("delete conversation", error);
    }

    let cancelled = take_generation_after_conversation_delete(&state, &id).await;
    if let Some(handle) = cancelled {
        broadcast_generation_terminal(&state, &id, &handle, "failed", Some("conversation-deleted"))
            .await;
    }
    state.conversation_txs.write().await.remove(&id);
    drop(transition_guard);
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

    let updated = match transact_persisted_state(
        &state,
        PureStateMutationScope::Conversations,
        move |staged| {
            let Some(conversation) = staged.conversations.get_mut(&id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
            };
            let Some(message) = find_message_mut(conversation, &message_id) else {
                return Err(StateMutationError::NotFound("Message not found"));
            };

            if message.role != "USER" && message.role != "ASSISTANT" {
                return Err(StateMutationError::BadRequest(
                    "Only user and assistant text messages can be edited.",
                ));
            }

            message.parts = vec![json!({
                "type": "text",
                "text": text,
            })];
            conversation.update_at = now_millis();
            Ok(conversation.clone())
        },
    )
    .await
    {
        Ok(updated) => updated,
        Err(error) => return state_mutation_error_response("edit message", error),
    };
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn delete_message(
    State(state): State<Arc<MockApiState>>,
    Path((id, message_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let updated = match transact_persisted_state(
        &state,
        PureStateMutationScope::Conversations,
        move |staged| {
            let Some(conversation) = staged.conversations.get_mut(&id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
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
                return Err(StateMutationError::NotFound("Message not found"));
            }

            conversation
                .messages
                .retain(|node| !node.messages.is_empty());
            conversation.update_at = now_millis();
            Ok(conversation.clone())
        },
    )
    .await
    {
        Ok(updated) => updated,
        Err(error) => return state_mutation_error_response("delete message", error),
    };
    broadcast_conversation_snapshot(&state, &updated).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "deleted" })).into_response()
}

async fn regenerate_message(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<String>,
    Json(payload): Json<RegenerateRequest>,
) -> impl IntoResponse {
    let assistant_id = current_assistant_id(&state).await;
    let model_id = current_model_id(&state, &assistant_id).await;
    let transition_guard = state.generation_transition_mutex.lock().await;
    let handle = match begin_generation(&state, &id, GenerationOperation::Regenerate).await {
        Ok(handle) => handle,
        Err(BeginGenerationError::Conflict) => {
            return conflict_response("A generation is already active for this conversation")
        }
    };
    let prepared =
        match prepare_regeneration_transaction(&state, id.clone(), payload.message_id).await {
            Ok(prepared) => prepared,
            Err(error) => {
                remove_generation_if_current(&state, &id, &handle).await;
                return state_mutation_error_response("prepare regeneration", error);
            }
        };
    if !activate_generation(&state, &id, &handle).await {
        remove_generation_if_current(&state, &id, &handle).await;
        return internal_error_response("Generation could not be started");
    }
    broadcast_conversation_snapshot(&state, &prepared.conversation).await;
    broadcast_list_invalidate(&state).await;
    broadcast_generation_start(&state, &id, &handle).await;
    drop(transition_guard);

    if prepared.last_user_has_non_text_parts {
        spawn_local_reply_generation(
            state.clone(),
            id,
            handle,
            prepared.plan,
            model_id,
            LOCAL_ATTACHMENT_REPLY_TEXT.to_string(),
        );
        return Json(json!({ "status": "accepted" })).into_response();
    }

    let real_chat_config = match resolve_openai_chat_config(&state, &model_id).await {
        Ok(config) => config,
        Err(_) => {
            fail_running_generation(&state, &id, &handle, "configuration").await;
            return Json(json!({ "status": "accepted" })).into_response();
        }
    };

    let Some(config) = real_chat_config else {
        spawn_local_reply_generation(
            state.clone(),
            id,
            handle,
            prepared.plan,
            model_id,
            MOCK_REPLY_TEXT.to_string(),
        );
        return Json(json!({ "status": "accepted" })).into_response();
    };

    let messages = openai_messages_from_conversation(&prepared.request_conversation);
    if messages.is_empty() {
        spawn_local_reply_generation(
            state.clone(),
            id,
            handle,
            prepared.plan,
            model_id,
            "Phase 6A currently supports regenerating text-only chat.".to_string(),
        );
        return Json(json!({ "status": "accepted" })).into_response();
    }

    spawn_openai_stream_generation(
        state.clone(),
        id,
        handle,
        prepared.plan,
        model_id,
        config,
        messages,
    );

    Json(json!({ "status": "accepted" })).into_response()
}

async fn update_assistant(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpdateAssistantRequest>,
) -> impl IntoResponse {
    if let Err(error) =
        transact_persisted_state(&state, PureStateMutationScope::Settings, move |staged| {
            staged.settings["assistantId"] = json!(payload.assistant_id);
            Ok(())
        })
        .await
    {
        return state_mutation_error_response("update assistant", error);
    }
    broadcast_settings_update(&state).await;
    broadcast_list_invalidate(&state).await;

    Json(json!({ "status": "ok" })).into_response()
}

async fn update_assistant_model(
    State(state): State<Arc<MockApiState>>,
    Json(payload): Json<UpdateAssistantModelRequest>,
) -> impl IntoResponse {
    if let Err(error) =
        transact_persisted_state(&state, PureStateMutationScope::Settings, move |staged| {
            let settings = &mut staged.settings;
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
            Ok(())
        })
        .await
    {
        return state_mutation_error_response("update assistant model", error);
    }
    broadcast_settings_update(&state).await;

    Json(json!({ "status": "ok" })).into_response()
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

    if let Err(error) =
        transact_persisted_state(&state, PureStateMutationScope::Settings, move |staged| {
            staged.settings["favoriteModels"] = json!(model_ids);
            Ok(())
        })
        .await
    {
        return state_mutation_error_response("update favorite models", error);
    }
    broadcast_settings_update(&state).await;

    Json(json!({ "status": "ok" })).into_response()
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

struct ValidatedFileUpload {
    display_name: String,
    mime: String,
    size_bytes: u64,
    kind: String,
    bytes: Vec<u8>,
}

struct PublishedFileUpload {
    display_name: String,
    mime: String,
    size_bytes: u64,
    kind: String,
    storage_key: String,
}

#[derive(Debug)]
enum FileUploadTransactionError {
    BlobOperation,
    State(StateMutationError),
    Compensation,
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
        prepared.push(ValidatedFileUpload {
            display_name,
            mime: mime.to_string(),
            size_bytes: size as u64,
            kind: upload_kind_for_mime(mime).to_string(),
            bytes: bytes.to_vec(),
        });
    }

    if prepared.is_empty() {
        return bad_request_response("No files were uploaded");
    }

    let uploaded = match commit_file_upload_batch(&state, prepared).await {
        Ok(uploaded) => uploaded,
        Err(error) => return file_upload_transaction_error_response(error),
    };

    Json(UploadFilesResponse { files: uploaded }).into_response()
}

fn file_upload_transaction_error_response(error: FileUploadTransactionError) -> Response {
    match error {
        FileUploadTransactionError::BlobOperation => internal_error_response("File upload failed"),
        FileUploadTransactionError::State(error) => {
            state_mutation_error_response("file upload", error)
        }
        FileUploadTransactionError::Compensation => {
            internal_error_response("File upload failed and local blob cleanup requires attention")
        }
    }
}

async fn commit_file_upload_batch(
    state: &Arc<MockApiState>,
    uploads: Vec<ValidatedFileUpload>,
) -> Result<Vec<UploadedFileResponse>, FileUploadTransactionError> {
    let _file_guard = state.file_blob_transaction_mutex.lock().await;
    let mut published_uploads = Vec::with_capacity(uploads.len());

    for upload in uploads {
        let published = match prepare_managed_blob(&state.blob_store, upload.bytes).await {
            Ok(published) => published,
            Err(error) => {
                let cleanup_keys = published_uploads
                    .iter()
                    .map(|item: &PublishedFileUpload| item.storage_key.clone())
                    .collect::<Vec<_>>();
                let cleanup_failed = cleanup_new_managed_blobs(&state.blob_store, cleanup_keys)
                    .await
                    .is_err();
                if error == ManagedBlobOperationError::Cleanup || cleanup_failed {
                    eprintln!("RikkaDesk managed blob compensation cleanup failed");
                    return Err(FileUploadTransactionError::Compensation);
                }
                return Err(FileUploadTransactionError::BlobOperation);
            }
        };
        published_uploads.push(PublishedFileUpload {
            display_name: upload.display_name,
            mime: upload.mime,
            size_bytes: upload.size_bytes,
            kind: upload.kind,
            storage_key: published.storage_key,
        });
    }

    let cleanup_keys = published_uploads
        .iter()
        .map(|upload| upload.storage_key.clone())
        .collect::<Vec<_>>();
    let transaction_result =
        transact_persisted_state(state, PureStateMutationScope::FileMetadata, move |staged| {
            let mut uploaded = Vec::with_capacity(published_uploads.len());
            for upload in published_uploads {
                let id = next_staged_file_id(staged);
                let now = now_iso();
                let metadata = ManagedFileMetadata {
                    id,
                    storage_key: upload.storage_key.clone(),
                    display_name: upload.display_name,
                    mime: upload.mime,
                    size_bytes: upload.size_bytes,
                    sha256: None,
                    kind: upload.kind,
                    relative_path: format!(
                        "{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{}",
                        upload.storage_key
                    ),
                    created_at: now.clone(),
                    updated_at: now,
                    source: "upload".to_string(),
                    deleted_at: None,
                };
                uploaded.push(UploadedFileResponse::from_metadata(&metadata));
                staged.files.push(metadata);
            }
            Ok(uploaded)
        })
        .await;

    match transaction_result {
        Ok(uploaded) => Ok(uploaded),
        Err(error) => {
            if cleanup_new_managed_blobs(&state.blob_store, cleanup_keys)
                .await
                .is_err()
            {
                eprintln!("RikkaDesk managed blob compensation cleanup failed");
                Err(FileUploadTransactionError::Compensation)
            } else {
                Err(FileUploadTransactionError::State(error))
            }
        }
    }
}

async fn file_metadata(
    State(state): State<Arc<MockApiState>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let metadata = {
        let _commit_guard = state.commit_barrier.read().await;
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
    let _file_guard = state.file_blob_transaction_mutex.lock().await;
    let metadata = {
        let _commit_guard = state.commit_barrier.read().await;
        let files = state.files.read().await;
        files
            .iter()
            .find(|file| file.id == id && file.deleted_at.is_none())
            .cloned()
    };

    let Some(metadata) = metadata else {
        return not_found_response("File not found");
    };

    let Some(path) = state.blob_store.final_path(&metadata.storage_key) else {
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
    let _file_guard = state.file_blob_transaction_mutex.lock().await;
    let metadata = {
        let _commit_guard = state.commit_barrier.read().await;
        let files = state.files.read().await;
        files.iter().find(|file| file.id == id).cloned()
    };

    let Some(metadata) = metadata else {
        return not_found_response("File not found");
    };

    if metadata.deleted_at.is_some() {
        if delete_managed_blob(&state.blob_store, metadata.storage_key)
            .await
            .is_err()
        {
            eprintln!("RikkaDesk managed blob cleanup failed after file tombstone commit");
        }
        return Json(json!({ "status": "deleted" })).into_response();
    }

    let transaction_result = transact_persisted_state(
        &state,
        PureStateMutationScope::FileMetadata,
        move |staged| {
            if is_managed_file_referenced(&staged.conversations, id) {
                return Err(StateMutationError::Conflict(
                    "File is referenced by a saved message",
                ));
            }
            let Some(file) = staged.files.iter_mut().find(|file| file.id == id) else {
                return Err(StateMutationError::NotFound("File not found"));
            };
            if file.deleted_at.is_none() {
                let now = now_iso();
                file.deleted_at = Some(now.clone());
                file.updated_at = now;
            }
            Ok(())
        },
    )
    .await;

    if let Err(error) = transaction_result {
        return state_mutation_error_response("file delete", error);
    }

    if delete_managed_blob(&state.blob_store, metadata.storage_key)
        .await
        .is_err()
    {
        eprintln!("RikkaDesk managed blob cleanup failed after file tombstone commit");
    }

    Json(json!({ "status": "deleted" })).into_response()
}

async fn prepare_managed_blob(
    blob_store: &ManagedBlobStore,
    bytes: Vec<u8>,
) -> Result<PublishedManagedBlob, ManagedBlobOperationError> {
    let blob_store = blob_store.clone();
    tokio::task::spawn_blocking(move || blob_store.prepare_and_publish(&bytes))
        .await
        .map_err(|_| ManagedBlobOperationError::Operation)?
}

async fn cleanup_new_managed_blobs(
    blob_store: &ManagedBlobStore,
    storage_keys: Vec<String>,
) -> Result<(), ()> {
    let blob_store = blob_store.clone();
    tokio::task::spawn_blocking(move || {
        let mut failed = false;
        for storage_key in storage_keys {
            if blob_store.delete_storage_key(&storage_key).is_err() {
                failed = true;
            }
        }
        (!failed).then_some(()).ok_or(())
    })
    .await
    .map_err(|_| ())?
}

async fn delete_managed_blob(blob_store: &ManagedBlobStore, storage_key: String) -> Result<(), ()> {
    let blob_store = blob_store.clone();
    tokio::task::spawn_blocking(move || blob_store.delete_storage_key(&storage_key))
        .await
        .map_err(|_| ())?
}

async fn managed_file_for_provider_image_input(
    state: &Arc<MockApiState>,
    file_id: u64,
) -> Result<ManagedImageProviderInput, String> {
    let _file_guard = state.file_blob_transaction_mutex.lock().await;
    let metadata = {
        let _commit_guard = state.commit_barrier.read().await;
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

    let Some(path) = state.blob_store.final_path(&metadata.storage_key) else {
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
    let providers = state.providers.read().await.clone();
    providers
        .iter()
        .map(|provider| {
            state
                .secret_store
                .secret_exists(&provider.secret_ref)
                .map_err(|_| "Secret store is unavailable".to_string())
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
                .secret_exists(&provider.secret_ref)
                .map_err(|_| "Secret store is unavailable".to_string())
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

fn is_managed_file_referenced(
    conversations: &HashMap<String, ConversationDto>,
    file_id: u64,
) -> bool {
    conversations.values().any(|conversation| {
        conversation.messages.iter().any(|node| {
            node.messages
                .iter()
                .any(|message| managed_file_ids_from_parts(&message.parts).contains(&file_id))
        })
    })
}

fn managed_file_ids_from_parts(parts: &[Value]) -> HashSet<u64> {
    parts
        .iter()
        .filter_map(|part| {
            part.get("metadata")
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("fileId"))
                .and_then(Value::as_u64)
        })
        .collect()
}

fn is_provider_image_mime(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/webp")
}

fn field_is_too_long(value: &str, max_chars: usize) -> bool {
    value.chars().count() > max_chars
}

async fn begin_generation(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    operation: GenerationOperation,
) -> Result<GenerationHandle, BeginGenerationError> {
    let mut generations = state.active_generations.lock().await;
    if generations.contains_key(conversation_id) {
        return Err(BeginGenerationError::Conflict);
    }

    let generation_id = state.generation_seq.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = GenerationHandle {
        generation_id,
        cancellation: Arc::new(GenerationCancellation::new()),
    };
    generations.insert(
        conversation_id.to_string(),
        ActiveGeneration {
            handle: handle.clone(),
            operation,
            phase: GenerationPhase::Reserved,
        },
    );
    Ok(handle)
}

async fn activate_generation(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
) -> bool {
    let mut generations = state.active_generations.lock().await;
    let Some(active) = generations.get_mut(conversation_id) else {
        return false;
    };
    if active.handle.generation_id != handle.generation_id
        || active.phase != GenerationPhase::Reserved
    {
        return false;
    }
    active.phase = GenerationPhase::Running;
    true
}

async fn is_generation_running(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
) -> bool {
    if handle.cancellation.is_cancelled() {
        return false;
    }
    state
        .active_generations
        .lock()
        .await
        .get(conversation_id)
        .is_some_and(|active| {
            active.handle.generation_id == handle.generation_id
                && active.phase == GenerationPhase::Running
        })
}

async fn has_active_generation(state: &Arc<MockApiState>, conversation_id: &str) -> bool {
    state
        .active_generations
        .lock()
        .await
        .contains_key(conversation_id)
}

async fn active_generation_conversation_ids(state: &Arc<MockApiState>) -> HashSet<String> {
    state
        .active_generations
        .lock()
        .await
        .keys()
        .cloned()
        .collect()
}

async fn claim_generation_for_finalization(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
) -> bool {
    let mut generations = state.active_generations.lock().await;
    let Some(active) = generations.get_mut(conversation_id) else {
        return false;
    };
    if active.handle.generation_id != handle.generation_id
        || active.phase != GenerationPhase::Running
        || handle.cancellation.is_cancelled()
    {
        return false;
    }
    active.phase = GenerationPhase::Finalizing;
    true
}

async fn remove_generation_if_current(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
) -> bool {
    let mut generations = state.active_generations.lock().await;
    let is_current = generations
        .get(conversation_id)
        .is_some_and(|active| active.handle.generation_id == handle.generation_id);
    if is_current {
        generations.remove(conversation_id);
    }
    is_current
}

async fn remove_running_generation_if_current(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
) -> bool {
    let mut generations = state.active_generations.lock().await;
    let removable = generations.get(conversation_id).is_some_and(|active| {
        active.handle.generation_id == handle.generation_id
            && matches!(
                active.phase,
                GenerationPhase::Reserved | GenerationPhase::Running
            )
    });
    if removable {
        generations.remove(conversation_id);
    }
    removable
}

async fn request_generation_stop(
    state: &Arc<MockApiState>,
    conversation_id: &str,
) -> StopGenerationRequest {
    let mut generations = state.active_generations.lock().await;
    let Some(active) = generations.get_mut(conversation_id) else {
        return StopGenerationRequest::NotFound;
    };
    match active.phase {
        GenerationPhase::Reserved | GenerationPhase::Running => {
            active.phase = GenerationPhase::Stopping;
            active.handle.cancellation.cancel();
            StopGenerationRequest::Owner(active.handle.clone())
        }
        GenerationPhase::Stopping => StopGenerationRequest::AlreadyStopping,
        GenerationPhase::Finalizing => StopGenerationRequest::Finalizing,
    }
}

async fn take_generation_after_conversation_delete(
    state: &Arc<MockApiState>,
    conversation_id: &str,
) -> Option<GenerationHandle> {
    let active = state
        .active_generations
        .lock()
        .await
        .remove(conversation_id)?;
    active.handle.cancellation.cancel();
    Some(active.handle)
}

fn message_revision(message: &MessageDto) -> MessageRevision {
    MessageRevision {
        message_id: message.id.clone(),
        fingerprint: message_fingerprint(message),
    }
}

fn message_fingerprint(message: &MessageDto) -> u64 {
    let bytes = serde_json::to_vec(message).unwrap_or_default();
    bytes
        .into_iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

fn message_matches_revision(message: &MessageDto, revision: &MessageRevision) -> bool {
    message.id == revision.message_id && message_fingerprint(message) == revision.fingerprint
}

async fn commit_initial_user_message(
    state: &Arc<MockApiState>,
    conversation_id: String,
    assistant_id: String,
    parts: Vec<Value>,
    mode_injection_ids: Option<Vec<String>>,
    lorebook_ids: Option<Vec<String>>,
    now: u64,
) -> Result<InitialSendCommit, StateMutationError> {
    let managed_file_ids = managed_file_ids_from_parts(&parts);
    let user_text = first_text_part(&parts);
    let created_at = now_iso();
    transact_persisted_state(
        state,
        PureStateMutationScope::ConversationGeneration,
        move |staged| {
            if managed_file_ids.iter().any(|file_id| {
                !staged
                    .files
                    .iter()
                    .any(|file| file.id == *file_id && file.deleted_at.is_none())
            }) {
                return Err(StateMutationError::BadRequest("Attachment is unavailable"));
            }

            let node_id = next_staged_id(staged, "node");
            let message_id = next_staged_id(staged, "msg");
            let conversation = staged
                .conversations
                .entry(conversation_id.clone())
                .or_insert_with(|| {
                    empty_conversation(conversation_id.clone(), assistant_id.clone(), now)
                });

            if conversation.messages.is_empty() {
                if let Some(title) = title_from_text(user_text.as_deref()) {
                    conversation.title = title;
                }
            }
            conversation.mode_injection_ids = mode_injection_ids;
            conversation.lorebook_ids = lorebook_ids;
            let message = MessageDto {
                id: message_id,
                role: "USER".to_string(),
                parts,
                annotations: None,
                created_at: created_at.clone(),
                finished_at: Some(created_at.clone()),
                model_id: None,
                usage: None,
                translation: None,
            };
            let anchor = message_revision(&message);
            conversation.messages.push(MessageNodeDto {
                id: node_id,
                messages: vec![message],
                select_index: 0,
            });
            conversation.update_at = now_millis();
            conversation.is_generating = false;

            Ok(InitialSendCommit {
                conversation: conversation.clone(),
                plan: GenerationCommitPlan {
                    target: GenerationCommitTarget::Append { anchor },
                },
            })
        },
    )
    .await
}

async fn prepare_regeneration_transaction(
    state: &Arc<MockApiState>,
    conversation_id: String,
    requested_message_id: Option<String>,
) -> Result<RegenerationPreparation, StateMutationError> {
    transact_persisted_state(
        state,
        PureStateMutationScope::ConversationGeneration,
        move |staged| {
            let Some(conversation) = staged.conversations.get(&conversation_id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
            };
            build_regeneration_preparation(conversation, requested_message_id.as_deref())
        },
    )
    .await
}

fn build_regeneration_preparation(
    conversation: &ConversationDto,
    requested_message_id: Option<&str>,
) -> Result<RegenerationPreparation, StateMutationError> {
    let Some(target_index) = find_regeneratable_node_index(conversation, requested_message_id)
    else {
        return Err(StateMutationError::BadRequest(
            "Phase 6A currently supports regenerating only the latest text turn.",
        ));
    };
    let Some(target_message) = selected_message(&conversation.messages[target_index]) else {
        return Err(StateMutationError::BadRequest(
            "Phase 6A currently supports regenerating only the latest text turn.",
        ));
    };

    let (anchor, replacement, request_end) = match target_message.role.as_str() {
        "ASSISTANT" => {
            let Some(anchor) = conversation.messages[..target_index]
                .iter()
                .rev()
                .filter_map(selected_message)
                .find(|message| message.role == "USER")
            else {
                return Err(StateMutationError::BadRequest(
                    "No user text message is available to regenerate from.",
                ));
            };
            (
                message_revision(anchor),
                Some(message_revision(target_message)),
                target_index,
            )
        }
        "USER" => {
            let replacement = conversation
                .messages
                .get(target_index + 1)
                .and_then(selected_message)
                .filter(|message| message.role == "ASSISTANT")
                .map(message_revision);
            (
                message_revision(target_message),
                replacement,
                target_index + 1,
            )
        }
        _ => {
            return Err(StateMutationError::BadRequest(
                "Only user and assistant text messages can be regenerated.",
            ));
        }
    };

    let Some(anchor_message) = find_message(conversation, &anchor.message_id) else {
        return Err(StateMutationError::BadRequest(
            "No user text message is available to regenerate from.",
        ));
    };
    let last_user_has_non_text_parts = has_non_text_parts(&anchor_message.parts);
    if !last_user_has_non_text_parts && text_from_parts(&anchor_message.parts).is_none() {
        return Err(StateMutationError::BadRequest(
            "Phase 6A currently supports regenerating text-only chat.",
        ));
    }

    let mut request_conversation = conversation.clone();
    request_conversation.messages.truncate(request_end);
    request_conversation.is_generating = false;
    let target = replacement.map_or_else(
        || GenerationCommitTarget::Append {
            anchor: anchor.clone(),
        },
        |target| GenerationCommitTarget::Replace {
            anchor: anchor.clone(),
            target,
        },
    );

    Ok(RegenerationPreparation {
        conversation: conversation.clone(),
        request_conversation,
        plan: GenerationCommitPlan { target },
        last_user_has_non_text_parts,
    })
}

async fn commit_final_assistant_message(
    state: &Arc<MockApiState>,
    conversation_id: String,
    plan: GenerationCommitPlan,
    model_id: String,
    reply_text: String,
) -> Result<ConversationDto, StateMutationError> {
    transact_persisted_state(
        state,
        PureStateMutationScope::ConversationGeneration,
        move |staged| {
            let Some(conversation) = staged.conversations.get(&conversation_id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
            };
            let (anchor, target) = match &plan.target {
                GenerationCommitTarget::Append { anchor } => (anchor, None),
                GenerationCommitTarget::Replace { anchor, target } => (anchor, Some(target)),
            };
            let Some(anchor_message) = find_message(conversation, &anchor.message_id) else {
                return Err(StateMutationError::Conflict(
                    "Generation source message changed",
                ));
            };
            if !message_matches_revision(anchor_message, anchor) {
                return Err(StateMutationError::Conflict(
                    "Generation source message changed",
                ));
            }
            if let Some(target) = target {
                let Some(target_message) = find_message(conversation, &target.message_id) else {
                    return Err(StateMutationError::Conflict("Regeneration target changed"));
                };
                if !message_matches_revision(target_message, target) {
                    return Err(StateMutationError::Conflict("Regeneration target changed"));
                }
            }

            let append_ids =
                matches!(plan.target, GenerationCommitTarget::Append { .. }).then(|| {
                    (
                        next_staged_id(staged, "node"),
                        next_staged_id(staged, "msg"),
                    )
                });
            let reply_time = now_iso();
            let conversation = staged
                .conversations
                .get_mut(&conversation_id)
                .ok_or(StateMutationError::NotFound("Conversation not found"))?;

            match &plan.target {
                GenerationCommitTarget::Append { .. } => {
                    let Some((node_id, message_id)) = append_ids else {
                        return Err(StateMutationError::Validation(
                            "Generation append IDs are unavailable",
                        ));
                    };
                    conversation.messages.push(MessageNodeDto {
                        id: node_id,
                        messages: vec![MessageDto {
                            id: message_id,
                            role: "ASSISTANT".to_string(),
                            parts: vec![json!({ "type": "text", "text": reply_text })],
                            annotations: None,
                            created_at: reply_time.clone(),
                            finished_at: Some(reply_time.clone()),
                            model_id: Some(model_id),
                            usage: None,
                            translation: None,
                        }],
                        select_index: 0,
                    });
                }
                GenerationCommitTarget::Replace { target, .. } => {
                    let Some(message) = find_message_mut(conversation, &target.message_id) else {
                        return Err(StateMutationError::Conflict("Regeneration target changed"));
                    };
                    message.role = "ASSISTANT".to_string();
                    message.parts = vec![json!({ "type": "text", "text": reply_text })];
                    message.annotations = None;
                    message.created_at = reply_time.clone();
                    message.finished_at = Some(reply_time.clone());
                    message.model_id = Some(model_id);
                    message.usage = None;
                    message.translation = None;
                }
            }

            conversation.update_at = now_millis();
            conversation.is_generating = false;
            Ok(conversation.clone())
        },
    )
    .await
}

async fn commit_stopped_generation(
    state: &Arc<MockApiState>,
    conversation_id: String,
) -> Result<ConversationDto, StateMutationError> {
    transact_persisted_state(
        state,
        PureStateMutationScope::Conversations,
        move |staged| {
            let Some(conversation) = staged.conversations.get_mut(&conversation_id) else {
                return Err(StateMutationError::NotFound("Conversation not found"));
            };
            conversation.is_generating = false;
            conversation.update_at = now_millis();
            Ok(conversation.clone())
        },
    )
    .await
}

async fn finalize_generation_with_text(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    plan: GenerationCommitPlan,
    model_id: String,
    reply_text: String,
) -> Result<bool, StateMutationError> {
    let transition_guard = state.generation_transition_mutex.lock().await;
    if !claim_generation_for_finalization(state, conversation_id, handle).await {
        return Ok(false);
    }
    let result = commit_final_assistant_message(
        state,
        conversation_id.to_string(),
        plan,
        model_id,
        reply_text,
    )
    .await;
    let removed = remove_generation_if_current(state, conversation_id, handle).await;

    match result {
        Ok(updated) => {
            if removed {
                broadcast_conversation_snapshot(state, &updated).await;
                broadcast_list_invalidate(state).await;
                broadcast_generation_terminal(state, conversation_id, handle, "finished", None)
                    .await;
            }
            drop(transition_guard);
            Ok(removed)
        }
        Err(error) => {
            if removed {
                broadcast_current_conversation_snapshot(state, conversation_id).await;
                broadcast_list_invalidate(state).await;
                let reason = if matches!(error, StateMutationError::Persistence(_)) {
                    "persistence"
                } else {
                    "stale"
                };
                broadcast_generation_terminal(
                    state,
                    conversation_id,
                    handle,
                    "failed",
                    Some(reason),
                )
                .await;
            }
            drop(transition_guard);
            Err(error)
        }
    }
}

async fn fail_running_generation(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    reason: &'static str,
) -> bool {
    let transition_guard = state.generation_transition_mutex.lock().await;
    let removed = remove_running_generation_if_current(state, conversation_id, handle).await;
    if removed {
        broadcast_current_conversation_snapshot(state, conversation_id).await;
        broadcast_list_invalidate(state).await;
        broadcast_generation_terminal(state, conversation_id, handle, "failed", Some(reason)).await;
    }
    drop(transition_guard);
    removed
}

async fn broadcast_generation_start(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
) {
    let operation = state
        .active_generations
        .lock()
        .await
        .get(conversation_id)
        .filter(|active| active.handle.generation_id == handle.generation_id)
        .map(|active| match active.operation {
            GenerationOperation::Send => "send",
            GenerationOperation::Regenerate => "regenerate",
        })
        .unwrap_or("generation");
    let sender = conversation_sender(state, conversation_id).await;
    let _ = sender.send(SsePayload {
        event: "generation-start".to_string(),
        data: json!({
            "type": "generation-start",
            "generationId": handle.generation_id,
            "operation": operation,
        }),
    });
}

async fn broadcast_generation_delta(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    plan: &GenerationCommitPlan,
    model_id: &str,
    delta: &str,
    accumulated: &str,
) {
    let sender = conversation_sender(state, conversation_id).await;
    let _ = sender.send(SsePayload {
        event: "delta".to_string(),
        data: json!({
            "type": "delta",
            "generationId": handle.generation_id,
            "text": delta,
            "transient": true,
        }),
    });
    broadcast_transient_generation_snapshot(
        state,
        conversation_id,
        handle,
        plan,
        model_id,
        accumulated,
    )
    .await;
}

async fn broadcast_generation_terminal(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    terminal: &'static str,
    reason: Option<&'static str>,
) {
    let sender = conversation_sender(state, conversation_id).await;
    let mut data = json!({
        "type": terminal,
        "generationId": handle.generation_id,
    });
    if let Some(reason) = reason {
        data["reason"] = json!(reason);
    }
    let _ = sender.send(SsePayload {
        event: terminal.to_string(),
        data,
    });
}

async fn broadcast_current_conversation_snapshot(state: &Arc<MockApiState>, conversation_id: &str) {
    let conversation = {
        let _commit_guard = state.commit_barrier.read().await;
        state
            .conversations
            .read()
            .await
            .get(conversation_id)
            .cloned()
    };
    if let Some(conversation) = conversation {
        broadcast_conversation_snapshot(state, &conversation).await;
    }
}

async fn broadcast_transient_generation_snapshot(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    plan: &GenerationCommitPlan,
    model_id: &str,
    accumulated: &str,
) {
    let mut conversation = {
        let _commit_guard = state.commit_barrier.read().await;
        state
            .conversations
            .read()
            .await
            .get(conversation_id)
            .cloned()
    };
    let Some(ref mut conversation) = conversation else {
        return;
    };
    conversation.is_generating = true;
    let runtime_time = now_iso();
    match &plan.target {
        GenerationCommitTarget::Append { .. } => {
            conversation.messages.push(MessageNodeDto {
                id: format!("runtime-node-{}", handle.generation_id),
                messages: vec![MessageDto {
                    id: format!("runtime-message-{}", handle.generation_id),
                    role: "ASSISTANT".to_string(),
                    parts: vec![json!({ "type": "text", "text": accumulated })],
                    annotations: None,
                    created_at: runtime_time,
                    finished_at: None,
                    model_id: Some(model_id.to_string()),
                    usage: None,
                    translation: None,
                }],
                select_index: 0,
            });
        }
        GenerationCommitTarget::Replace { target, .. } => {
            let Some(message) = find_message_mut(conversation, &target.message_id) else {
                return;
            };
            message.parts = vec![json!({ "type": "text", "text": accumulated })];
            message.finished_at = None;
            message.model_id = Some(model_id.to_string());
        }
    }
    let sender = conversation_sender(state, conversation_id).await;
    let mut data = conversation_snapshot_payload(state, conversation);
    data["transient"] = json!(true);
    data["generationId"] = json!(handle.generation_id);
    let _ = sender.send(SsePayload {
        event: "snapshot".to_string(),
        data,
    });
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

fn spawn_local_reply_generation(
    state: Arc<MockApiState>,
    conversation_id: String,
    handle: GenerationHandle,
    plan: GenerationCommitPlan,
    model_id: String,
    reply_text: String,
) {
    tokio::spawn(async move {
        let _ = finalize_generation_with_text(
            &state,
            &conversation_id,
            &handle,
            plan,
            model_id,
            reply_text,
        )
        .await;
    });
}

fn spawn_openai_stream_generation(
    state: Arc<MockApiState>,
    conversation_id: String,
    handle: GenerationHandle,
    plan: GenerationCommitPlan,
    model_id: String,
    config: OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
) {
    tokio::spawn(async move {
        match stream_openai_compatible_chat(
            &state,
            &conversation_id,
            &handle,
            &plan,
            &model_id,
            &config,
            messages,
        )
        .await
        {
            Ok(GenerationStreamOutcome::Completed(reply_text)) if !reply_text.is_empty() => {
                let _ = finalize_generation_with_text(
                    &state,
                    &conversation_id,
                    &handle,
                    plan,
                    model_id,
                    reply_text,
                )
                .await;
            }
            Ok(GenerationStreamOutcome::Completed(_)) | Err(_) => {
                fail_running_generation(&state, &conversation_id, &handle, "provider").await;
            }
            Ok(GenerationStreamOutcome::CancelledOrStale) => {}
        }
    });
}

fn spawn_openai_vision_capture_generation(
    state: Arc<MockApiState>,
    conversation_id: String,
    handle: GenerationHandle,
    plan: GenerationCommitPlan,
    model_id: String,
    config: OpenAiChatConfig,
    messages: Vec<OpenAiCompatibleChatMessage>,
) {
    tokio::spawn(async move {
        match stream_openai_compatible_vision_capture(
            &state,
            &conversation_id,
            &handle,
            &plan,
            &model_id,
            &config,
            messages,
        )
        .await
        {
            Ok(GenerationStreamOutcome::Completed(reply_text)) if !reply_text.is_empty() => {
                let _ = finalize_generation_with_text(
                    &state,
                    &conversation_id,
                    &handle,
                    plan,
                    model_id,
                    reply_text,
                )
                .await;
            }
            Ok(GenerationStreamOutcome::Completed(_)) | Err(_) => {
                fail_running_generation(&state, &conversation_id, &handle, "provider").await;
            }
            Ok(GenerationStreamOutcome::CancelledOrStale) => {}
        }
    });
}

async fn stream_openai_compatible_chat(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    plan: &GenerationCommitPlan,
    model_id: &str,
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
) -> Result<GenerationStreamOutcome, String> {
    if !is_generation_running(state, conversation_id, handle).await {
        return Ok(GenerationStreamOutcome::CancelledOrStale);
    }
    let body =
        build_openai_chat_request_body(config, messages, true, OpenAiRequestKind::StreamingChat)?;

    let request = state
        .http_client
        .post(openai_chat_completions_url(&config.base_url));
    let request = apply_openai_custom_headers(request, &config.custom_headers)?;
    let request = request.bearer_auth(&config.api_key).json(&body);
    let mut response = tokio::select! {
        result = request.send() => result.map_err(safe_reqwest_error)?,
        _ = handle.cancellation.cancelled() => {
            return Ok(GenerationStreamOutcome::CancelledOrStale);
        }
    };

    let status = response.status();
    if !status.is_success() {
        return Err(safe_http_status_error(status));
    }

    let mut buffer = String::new();
    let mut buffered_reply = String::new();
    loop {
        if !is_generation_running(state, conversation_id, handle).await {
            return Ok(GenerationStreamOutcome::CancelledOrStale);
        }
        let chunk = tokio::select! {
            result = response.chunk() => result.map_err(safe_reqwest_error)?,
            _ = handle.cancellation.cancelled() => {
                return Ok(GenerationStreamOutcome::CancelledOrStale);
            }
        };
        let Some(chunk) = chunk else {
            break;
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim_end_matches('\r').to_string();
            buffer.drain(..=line_end);
            match parse_openai_stream_line(&line)? {
                ParsedOpenAiStreamLine::Ignore => {}
                ParsedOpenAiStreamLine::Done => {
                    return Ok(GenerationStreamOutcome::Completed(buffered_reply));
                }
                ParsedOpenAiStreamLine::Delta(delta) => {
                    if !is_generation_running(state, conversation_id, handle).await {
                        return Ok(GenerationStreamOutcome::CancelledOrStale);
                    }
                    buffered_reply.push_str(&delta);
                    broadcast_generation_delta(
                        state,
                        conversation_id,
                        handle,
                        plan,
                        model_id,
                        &delta,
                        &buffered_reply,
                    )
                    .await;
                }
            }
        }
    }

    if !buffer.trim().is_empty() {
        match parse_openai_stream_line(&buffer)? {
            ParsedOpenAiStreamLine::Ignore => {}
            ParsedOpenAiStreamLine::Done => {
                return Ok(GenerationStreamOutcome::Completed(buffered_reply));
            }
            ParsedOpenAiStreamLine::Delta(delta) => {
                buffered_reply.push_str(&delta);
                broadcast_generation_delta(
                    state,
                    conversation_id,
                    handle,
                    plan,
                    model_id,
                    &delta,
                    &buffered_reply,
                )
                .await;
            }
        }
    }

    if !is_generation_running(state, conversation_id, handle).await {
        Ok(GenerationStreamOutcome::CancelledOrStale)
    } else {
        Err("stream ended before DONE".to_string())
    }
}

async fn stream_openai_compatible_vision_capture(
    state: &Arc<MockApiState>,
    conversation_id: &str,
    handle: &GenerationHandle,
    plan: &GenerationCommitPlan,
    model_id: &str,
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiCompatibleChatMessage>,
) -> Result<GenerationStreamOutcome, String> {
    if !is_generation_running(state, conversation_id, handle).await {
        return Ok(GenerationStreamOutcome::CancelledOrStale);
    }
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
    let request = request.bearer_auth(&config.api_key).json(&body);
    let mut response = tokio::select! {
        result = request.send() => result.map_err(safe_reqwest_error)?,
        _ = handle.cancellation.cancelled() => {
            return Ok(GenerationStreamOutcome::CancelledOrStale);
        }
    };

    let status = response.status();
    if !status.is_success() {
        return Err(safe_http_status_error(status));
    }

    let mut buffer = String::new();
    let mut buffered_reply = String::new();
    loop {
        if !is_generation_running(state, conversation_id, handle).await {
            return Ok(GenerationStreamOutcome::CancelledOrStale);
        }
        let chunk = tokio::select! {
            result = response.chunk() => result.map_err(safe_reqwest_error)?,
            _ = handle.cancellation.cancelled() => {
                return Ok(GenerationStreamOutcome::CancelledOrStale);
            }
        };
        let Some(chunk) = chunk else {
            break;
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim_end_matches('\r').to_string();
            buffer.drain(..=line_end);
            match parse_openai_stream_line(&line)? {
                ParsedOpenAiStreamLine::Ignore => {}
                ParsedOpenAiStreamLine::Done => {
                    return Ok(GenerationStreamOutcome::Completed(buffered_reply));
                }
                ParsedOpenAiStreamLine::Delta(delta) => {
                    if !is_generation_running(state, conversation_id, handle).await {
                        return Ok(GenerationStreamOutcome::CancelledOrStale);
                    }
                    buffered_reply.push_str(&delta);
                    broadcast_generation_delta(
                        state,
                        conversation_id,
                        handle,
                        plan,
                        model_id,
                        &delta,
                        &buffered_reply,
                    )
                    .await;
                }
            }
        }
    }

    if !buffer.trim().is_empty() {
        match parse_openai_stream_line(&buffer)? {
            ParsedOpenAiStreamLine::Ignore => {}
            ParsedOpenAiStreamLine::Done => {
                return Ok(GenerationStreamOutcome::Completed(buffered_reply));
            }
            ParsedOpenAiStreamLine::Delta(delta) => {
                buffered_reply.push_str(&delta);
                broadcast_generation_delta(
                    state,
                    conversation_id,
                    handle,
                    plan,
                    model_id,
                    &delta,
                    &buffered_reply,
                )
                .await;
            }
        }
    }

    if !is_generation_running(state, conversation_id, handle).await {
        Ok(GenerationStreamOutcome::CancelledOrStale)
    } else {
        Err("stream ended before DONE".to_string())
    }
}

fn parse_openai_stream_line(line: &str) -> Result<ParsedOpenAiStreamLine, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return Ok(ParsedOpenAiStreamLine::Ignore);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(ParsedOpenAiStreamLine::Ignore);
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(ParsedOpenAiStreamLine::Done);
    }

    let chunk = serde_json::from_str::<OpenAiChatStreamResponse>(data)
        .map_err(|_| "stream returned invalid JSON".to_string())?;
    let mut delta = String::new();
    for choice in chunk.choices {
        if let Some(content) = choice.delta.content {
            delta.push_str(&content);
        }
    }

    if delta.is_empty() {
        Ok(ParsedOpenAiStreamLine::Ignore)
    } else {
        Ok(ParsedOpenAiStreamLine::Delta(delta))
    }
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
    let mut staged = persisted_snapshot_from_live(state).await;
    build_desktop_provider_from_staged(&mut staged, payload)
}

fn build_desktop_provider_from_staged(
    staged: &mut PersistedMockState,
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
        staged.providers.iter().find(|item| item.id == id).cloned()
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
        .unwrap_or_else(|| next_staged_id(staged, "desktop-provider"));

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
        build_models_from_multi_request(staged, existing.as_ref(), models)?
    } else {
        build_models_from_singular_request(
            staged,
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
        staged_id_seq: staged.id_seq,
        is_new_provider: existing.is_none(),
    })
}

fn build_models_from_multi_request(
    staged: &mut PersistedMockState,
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
            .unwrap_or_else(|| next_staged_id(staged, "desktop-model"));

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
    staged: &mut PersistedMockState,
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
        .unwrap_or_else(|| next_staged_id(staged, "desktop-model"));

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

fn conflict_response(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": message,
            "code": 409,
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

async fn conversation_or_virtual_for_read(state: &Arc<MockApiState>, id: &str) -> ConversationDto {
    let mut conversation = {
        let _commit_guard = state.commit_barrier.read().await;
        let assistant_id = state
            .settings
            .read()
            .await
            .get("assistantId")
            .and_then(Value::as_str)
            .unwrap_or(MOCK_ASSISTANT_ID)
            .to_string();
        state
            .conversations
            .read()
            .await
            .get(id)
            .cloned()
            .unwrap_or_else(|| empty_conversation(id.to_string(), assistant_id, now_millis()))
    };
    conversation.is_generating = has_active_generation(state, id).await;
    conversation
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
    let mut conversation = conversation.clone();
    conversation.is_generating = has_active_generation(state, &conversation.id).await;
    let sender = conversation_sender(state, &conversation.id).await;
    let data = conversation_snapshot_payload(state, &conversation);
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

impl StateMigrator for RealStateMigrator {
    fn migrate(&self, persisted: PersistedMockState) -> Result<PersistedMockState, StateLoadError> {
        let from = persisted.schema_version;
        let original_files =
            (from == FILE_METADATA_STATE_SCHEMA_VERSION).then(|| persisted.files.clone());
        let mut migrated = match from {
            LEGACY_STATE_SCHEMA_VERSION => migrate_v1_to_v6(persisted),
            PREVIOUS_STATE_SCHEMA_VERSION => migrate_v2_to_v6(persisted),
            MULTI_MODEL_STATE_SCHEMA_VERSION => migrate_v3_to_v6(persisted),
            CUSTOM_REQUEST_CONFIG_STATE_SCHEMA_VERSION => migrate_v4_to_v6(persisted),
            FILE_METADATA_STATE_SCHEMA_VERSION => migrate_v5_to_v6(persisted),
            _ => {
                return Err(StateLoadError::MigrationFailed {
                    from,
                    reason: "source validation",
                });
            }
        };

        sync_settings_with_desktop_providers(&mut migrated.settings, &migrated.providers);
        ensure_current_model_exists(&mut migrated.settings);
        validate_migrated_state(&migrated, from)?;

        if original_files
            .as_ref()
            .is_some_and(|files| files != &migrated.files)
        {
            return Err(StateLoadError::MigrationFailed {
                from,
                reason: "file metadata validation",
            });
        }

        Ok(migrated)
    }
}

fn validate_migrated_state(
    persisted: &PersistedMockState,
    from: u32,
) -> Result<(), StateLoadError> {
    if persisted.schema_version != STATE_SCHEMA_VERSION || !persisted.settings.is_object() {
        return Err(StateLoadError::MigrationFailed {
            from,
            reason: "result validation",
        });
    }

    let modalities_valid = persisted.providers.iter().all(|provider| {
        provider.models.iter().all(|model| {
            model.input_modalities.first().map(String::as_str) == Some(MODEL_MODALITY_TEXT)
                && model.input_modalities.iter().all(|modality| {
                    matches!(
                        modality.as_str(),
                        MODEL_MODALITY_TEXT | MODEL_MODALITY_IMAGE
                    )
                })
                && model.output_modalities == default_output_modalities()
        })
    });
    if !modalities_valid {
        return Err(StateLoadError::MigrationFailed {
            from,
            reason: "model capability validation",
        });
    }

    Ok(())
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

fn next_staged_id(staged: &mut PersistedMockState, prefix: &str) -> String {
    staged.id_seq += 1;
    format!("{prefix}-{}", staged.id_seq)
}

fn next_staged_file_id(staged: &mut PersistedMockState) -> u64 {
    staged.id_seq += 1;
    staged.id_seq
}

fn secret_ref_for_provider(provider_id: &str) -> String {
    format!("{PROVIDER_SECRET_REF_PREFIX}{provider_id}:api-key")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderSecretCleanupStatus {
    Clean,
    Residual,
}

fn is_controlled_provider_secret_ref(secret_ref: &str) -> bool {
    secret_ref.starts_with(PROVIDER_SECRET_REF_PREFIX)
        && secret_ref.len() <= 512
        && secret_ref
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
}

fn prepare_new_secret(
    secret_store: &dyn SecretStore,
    secret_ref: &str,
    value: &str,
) -> Result<(), ()> {
    if !is_controlled_provider_secret_ref(secret_ref) {
        return Err(());
    }
    if !matches!(secret_store.secret_exists(secret_ref), Ok(false)) {
        return Err(());
    }
    secret_store.set_secret(secret_ref, value).map_err(|_| ())
}

fn unused_provider_secret_ref(state: &MockApiState, provider_id: &str) -> Result<String, ()> {
    const MAX_ATTEMPTS: usize = 32;

    for _ in 0..MAX_ATTEMPTS {
        let secret_ref = state.next_provider_secret_ref(provider_id);
        match state.secret_store.secret_exists(&secret_ref) {
            Ok(false) => return Ok(secret_ref),
            Ok(true) => continue,
            Err(_) => return Err(()),
        }
    }

    Err(())
}

fn delete_secret_for_ref(secret_store: &dyn SecretStore, secret_ref: &str) -> Result<(), ()> {
    if !is_controlled_provider_secret_ref(secret_ref) {
        return Err(());
    }
    secret_store.delete_secret(secret_ref).map_err(|_| ())
}

fn compensate_new_secret(secret_store: &dyn SecretStore, secret_ref: &str) -> Result<(), ()> {
    delete_secret_for_ref_with_retry(secret_store, secret_ref)
}

fn cleanup_secret_after_state_commit(
    secret_store: &dyn SecretStore,
    secret_ref: &str,
) -> ProviderSecretCleanupStatus {
    match delete_secret_for_ref_with_retry(secret_store, secret_ref) {
        Ok(()) => ProviderSecretCleanupStatus::Clean,
        Err(()) => {
            eprintln!("RikkaDesk provider secret cleanup failed after state commit");
            ProviderSecretCleanupStatus::Residual
        }
    }
}

fn delete_secret_for_ref_with_retry(
    secret_store: &dyn SecretStore,
    secret_ref: &str,
) -> Result<(), ()> {
    const MAX_ATTEMPTS: usize = 3;

    for _ in 0..MAX_ATTEMPTS {
        if delete_secret_for_ref(secret_store, secret_ref).is_ok() {
            return Ok(());
        }
    }

    Err(())
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

fn find_message<'a>(conversation: &'a ConversationDto, message_id: &str) -> Option<&'a MessageDto> {
    conversation
        .messages
        .iter()
        .flat_map(|node| node.messages.iter())
        .find(|message| message.id == message_id)
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

    static TEST_TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

    struct SyntheticTempDir {
        path: PathBuf,
    }

    impl SyntheticTempDir {
        fn new(label: &str) -> Self {
            let sequence = TEST_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rikkadesk-state-persist-{label}-{}-{sequence}",
                std::process::id()
            ));
            std_fs::create_dir_all(&path).expect("synthetic temp directory should be created");
            Self { path }
        }
    }

    impl Drop for SyntheticTempDir {
        fn drop(&mut self) {
            let _ = std_fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TestFailureStage {
        Read,
        BackupCreate,
        Write,
        Flush,
        Sync,
        Replace,
    }

    struct FaultingStateFileOps {
        stage: TestFailureStage,
    }

    impl FaultingStateFileOps {
        fn failure(&self) -> io::Error {
            io::Error::new(io::ErrorKind::Other, "synthetic persistence failure")
        }
    }

    impl StateFileOps for FaultingStateFileOps {
        fn read(&self, path: &FilePath) -> io::Result<Vec<u8>> {
            if self.stage == TestFailureStage::Read {
                return Err(self.failure());
            }
            RealStateFileOps.read(path)
        }

        fn create_dir_all(&self, path: &FilePath) -> io::Result<()> {
            RealStateFileOps.create_dir_all(path)
        }

        fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File> {
            if self.stage == TestFailureStage::BackupCreate && is_test_backup_path(path) {
                return Err(self.failure());
            }
            RealStateFileOps.create_temp(path)
        }

        fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()> {
            if self.stage == TestFailureStage::Write {
                return Err(self.failure());
            }
            RealStateFileOps.write_all(file, data)
        }

        fn flush(&self, file: &mut std_fs::File) -> io::Result<()> {
            if self.stage == TestFailureStage::Flush {
                return Err(self.failure());
            }
            RealStateFileOps.flush(file)
        }

        fn sync_all(&self, file: &std_fs::File) -> io::Result<()> {
            if self.stage == TestFailureStage::Sync {
                return Err(self.failure());
            }
            RealStateFileOps.sync_all(file)
        }

        fn replace(&self, replacement: &FilePath, target: &FilePath) -> io::Result<()> {
            if self.stage == TestFailureStage::Replace {
                return Err(self.failure());
            }
            RealStateFileOps.replace(replacement, target)
        }

        fn remove_file(&self, path: &FilePath) -> io::Result<()> {
            RealStateFileOps.remove_file(path)
        }
    }

    struct FailOnReplaceCallStateFileOps {
        fail_on: u64,
        replace_calls: AtomicU64,
    }

    impl FailOnReplaceCallStateFileOps {
        fn new(fail_on: u64) -> Self {
            Self {
                fail_on,
                replace_calls: AtomicU64::new(0),
            }
        }
    }

    impl StateFileOps for FailOnReplaceCallStateFileOps {
        fn read(&self, path: &FilePath) -> io::Result<Vec<u8>> {
            RealStateFileOps.read(path)
        }

        fn create_dir_all(&self, path: &FilePath) -> io::Result<()> {
            RealStateFileOps.create_dir_all(path)
        }

        fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File> {
            RealStateFileOps.create_temp(path)
        }

        fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()> {
            RealStateFileOps.write_all(file, data)
        }

        fn flush(&self, file: &mut std_fs::File) -> io::Result<()> {
            RealStateFileOps.flush(file)
        }

        fn sync_all(&self, file: &std_fs::File) -> io::Result<()> {
            RealStateFileOps.sync_all(file)
        }

        fn replace(&self, replacement: &FilePath, target: &FilePath) -> io::Result<()> {
            let call = self.replace_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == self.fail_on {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic nth persistence failure",
                ));
            }
            RealStateFileOps.replace(replacement, target)
        }

        fn remove_file(&self, path: &FilePath) -> io::Result<()> {
            RealStateFileOps.remove_file(path)
        }
    }

    struct CollisionOnceStateFileOps {
        collided: std::sync::atomic::AtomicBool,
    }

    impl CollisionOnceStateFileOps {
        fn new() -> Self {
            Self {
                collided: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    impl StateFileOps for CollisionOnceStateFileOps {
        fn read(&self, path: &FilePath) -> io::Result<Vec<u8>> {
            RealStateFileOps.read(path)
        }

        fn create_dir_all(&self, path: &FilePath) -> io::Result<()> {
            RealStateFileOps.create_dir_all(path)
        }

        fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File> {
            if !self.collided.swap(true, Ordering::SeqCst) {
                let mut existing = RealStateFileOps.create_temp(path)?;
                RealStateFileOps.write_all(&mut existing, b"synthetic existing backup")?;
                RealStateFileOps.flush(&mut existing)?;
                RealStateFileOps.sync_all(&existing)?;
                drop(existing);
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "synthetic backup collision",
                ));
            }
            RealStateFileOps.create_temp(path)
        }

        fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()> {
            RealStateFileOps.write_all(file, data)
        }

        fn flush(&self, file: &mut std_fs::File) -> io::Result<()> {
            RealStateFileOps.flush(file)
        }

        fn sync_all(&self, file: &std_fs::File) -> io::Result<()> {
            RealStateFileOps.sync_all(file)
        }

        fn replace(&self, replacement: &FilePath, target: &FilePath) -> io::Result<()> {
            RealStateFileOps.replace(replacement, target)
        }

        fn remove_file(&self, path: &FilePath) -> io::Result<()> {
            RealStateFileOps.remove_file(path)
        }
    }

    struct FailingStateMigrator;

    impl StateMigrator for FailingStateMigrator {
        fn migrate(
            &self,
            persisted: PersistedMockState,
        ) -> Result<PersistedMockState, StateLoadError> {
            Err(StateLoadError::MigrationFailed {
                from: persisted.schema_version,
                reason: "synthetic transform",
            })
        }
    }

    struct UnexpectedStateMigrator;

    impl StateMigrator for UnexpectedStateMigrator {
        fn migrate(
            &self,
            _persisted: PersistedMockState,
        ) -> Result<PersistedMockState, StateLoadError> {
            panic!("migration must not run when the pre-migration backup fails")
        }
    }

    struct FailingSerialize;

    impl Serialize for FailingSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("synthetic serialization failure"))
        }
    }

    struct TestSecretStore;

    impl SecretStore for TestSecretStore {
        fn set_secret(&self, _secret_ref: &str, _value: &str) -> SecretStoreResult<()> {
            Ok(())
        }

        fn get_secret(&self, _secret_ref: &str) -> SecretStoreResult<Option<String>> {
            Ok(None)
        }

        fn delete_secret(&self, _secret_ref: &str) -> SecretStoreResult<()> {
            Ok(())
        }
    }

    struct PanicOnAccessSecretStore;

    impl SecretStore for PanicOnAccessSecretStore {
        fn set_secret(&self, _secret_ref: &str, _value: &str) -> SecretStoreResult<()> {
            panic!("backup export must not access SecretStore")
        }

        fn get_secret(&self, _secret_ref: &str) -> SecretStoreResult<Option<String>> {
            panic!("backup export must not access SecretStore")
        }

        fn delete_secret(&self, _secret_ref: &str) -> SecretStoreResult<()> {
            panic!("backup export must not access SecretStore")
        }

        fn secret_exists(&self, _secret_ref: &str) -> SecretStoreResult<bool> {
            panic!("backup export must not access SecretStore")
        }
    }

    #[derive(Default)]
    struct FaultingManagedBlobFileOps {
        write_failure_at: Option<u64>,
        publish_failure_at: Option<u64>,
        delete_failures: AtomicU64,
        write_calls: AtomicU64,
        publish_calls: AtomicU64,
        remove_calls: AtomicU64,
    }

    impl FaultingManagedBlobFileOps {
        fn fail_write_at(call: u64) -> Self {
            Self {
                write_failure_at: Some(call),
                ..Self::default()
            }
        }

        fn fail_publish_at(call: u64) -> Self {
            Self {
                publish_failure_at: Some(call),
                ..Self::default()
            }
        }

        fn fail_delete_times(count: u64) -> Self {
            Self {
                delete_failures: AtomicU64::new(count),
                ..Self::default()
            }
        }

        fn consume_failure(counter: &AtomicU64) -> bool {
            counter
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    if value == 0 {
                        None
                    } else {
                        Some(value - 1)
                    }
                })
                .is_ok()
        }

        fn remove_call_count(&self) -> u64 {
            self.remove_calls.load(Ordering::SeqCst)
        }
    }

    impl ManagedBlobFileOps for FaultingManagedBlobFileOps {
        fn create_dir_all(&self, path: &FilePath) -> io::Result<()> {
            RealManagedBlobFileOps.create_dir_all(path)
        }

        fn path_exists(&self, path: &FilePath) -> io::Result<bool> {
            RealManagedBlobFileOps.path_exists(path)
        }

        fn create_temp(&self, path: &FilePath) -> io::Result<std_fs::File> {
            RealManagedBlobFileOps.create_temp(path)
        }

        fn write_all(&self, file: &mut std_fs::File, data: &[u8]) -> io::Result<()> {
            let call = self.write_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.write_failure_at == Some(call) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic managed blob write failure",
                ));
            }
            RealManagedBlobFileOps.write_all(file, data)
        }

        fn flush(&self, file: &mut std_fs::File) -> io::Result<()> {
            RealManagedBlobFileOps.flush(file)
        }

        fn sync_all(&self, file: &std_fs::File) -> io::Result<()> {
            RealManagedBlobFileOps.sync_all(file)
        }

        fn publish(&self, temp: &FilePath, final_path: &FilePath) -> io::Result<()> {
            let call = self.publish_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.publish_failure_at == Some(call) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic managed blob publish failure",
                ));
            }
            RealManagedBlobFileOps.publish(temp, final_path)
        }

        fn remove_file(&self, path: &FilePath) -> io::Result<()> {
            self.remove_calls.fetch_add(1, Ordering::SeqCst);
            if Self::consume_failure(&self.delete_failures) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic managed blob delete failure",
                ));
            }
            RealManagedBlobFileOps.remove_file(path)
        }
    }

    #[derive(Default)]
    struct ProviderTransactionTestSecretStore {
        secrets: std::sync::Mutex<HashMap<String, String>>,
        set_calls: AtomicU64,
        delete_calls: AtomicU64,
        fail_next_set: AtomicU64,
        fail_next_delete: AtomicU64,
    }

    impl ProviderTransactionTestSecretStore {
        fn seed(&self, secret_ref: &str) {
            self.secrets
                .lock()
                .expect("synthetic secret store lock should be available")
                .insert(secret_ref.to_string(), "synthetic-secret-value".to_string());
        }

        fn contains(&self, secret_ref: &str) -> bool {
            self.secrets
                .lock()
                .expect("synthetic secret store lock should be available")
                .contains_key(secret_ref)
        }

        fn len(&self) -> usize {
            self.secrets
                .lock()
                .expect("synthetic secret store lock should be available")
                .len()
        }

        fn set_call_count(&self) -> u64 {
            self.set_calls.load(Ordering::SeqCst)
        }

        fn delete_call_count(&self) -> u64 {
            self.delete_calls.load(Ordering::SeqCst)
        }

        fn fail_next_set(&self) {
            self.fail_next_set.store(1, Ordering::SeqCst);
        }

        fn fail_next_delete(&self) {
            self.fail_next_delete.store(1, Ordering::SeqCst);
        }

        fn fail_delete_times(&self, count: u64) {
            self.fail_next_delete.store(count, Ordering::SeqCst);
        }

        fn consume_failure(counter: &AtomicU64) -> bool {
            counter
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    if value == 0 {
                        None
                    } else {
                        Some(value - 1)
                    }
                })
                .is_ok()
        }
    }

    impl SecretStore for ProviderTransactionTestSecretStore {
        fn set_secret(&self, secret_ref: &str, value: &str) -> SecretStoreResult<()> {
            self.set_calls.fetch_add(1, Ordering::SeqCst);
            if Self::consume_failure(&self.fail_next_set) {
                return Err("synthetic secret write failure".to_string());
            }
            self.secrets
                .lock()
                .expect("synthetic secret store lock should be available")
                .insert(secret_ref.to_string(), value.to_string());
            Ok(())
        }

        fn get_secret(&self, secret_ref: &str) -> SecretStoreResult<Option<String>> {
            Ok(self
                .secrets
                .lock()
                .expect("synthetic secret store lock should be available")
                .get(secret_ref)
                .cloned())
        }

        fn delete_secret(&self, secret_ref: &str) -> SecretStoreResult<()> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            if Self::consume_failure(&self.fail_next_delete) {
                return Err("synthetic secret delete failure".to_string());
            }
            self.secrets
                .lock()
                .expect("synthetic secret store lock should be available")
                .remove(secret_ref);
            Ok(())
        }

        fn secret_exists(&self, secret_ref: &str) -> SecretStoreResult<bool> {
            Ok(self.contains(secret_ref))
        }
    }

    fn test_persistence(temp: &SyntheticTempDir) -> MockPersistence {
        MockPersistence::new(temp.path.clone())
    }

    fn faulting_test_persistence(
        temp: &SyntheticTempDir,
        stage: TestFailureStage,
    ) -> MockPersistence {
        MockPersistence::new_with_file_ops(
            temp.path.clone(),
            Arc::new(FaultingStateFileOps { stage }),
        )
    }

    fn read_test_state(persistence: &MockPersistence) -> Value {
        let data = std_fs::read(&persistence.state_path).expect("state file should exist");
        serde_json::from_slice(&data).expect("state file should contain valid JSON")
    }

    fn write_test_state_bytes(persistence: &MockPersistence, bytes: &[u8]) {
        std_fs::create_dir_all(&persistence.state_dir)
            .expect("synthetic state directory should be created");
        std_fs::write(&persistence.state_path, bytes).expect("synthetic state should be written");
    }

    async fn transaction_test_state(
        temp: &SyntheticTempDir,
        persisted: PersistedMockState,
        failure_stage: Option<TestFailureStage>,
    ) -> Arc<MockApiState> {
        transaction_test_state_with_secret_store(
            temp,
            persisted,
            failure_stage,
            Arc::new(TestSecretStore),
        )
        .await
    }

    async fn transaction_test_state_with_secret_store(
        temp: &SyntheticTempDir,
        persisted: PersistedMockState,
        failure_stage: Option<TestFailureStage>,
        secret_store: Arc<dyn SecretStore>,
    ) -> Arc<MockApiState> {
        transaction_test_state_with_stores(
            temp,
            persisted,
            failure_stage,
            secret_store,
            Arc::new(RealManagedBlobFileOps),
        )
        .await
    }

    async fn transaction_test_state_with_stores(
        temp: &SyntheticTempDir,
        persisted: PersistedMockState,
        failure_stage: Option<TestFailureStage>,
        secret_store: Arc<dyn SecretStore>,
        blob_file_ops: Arc<dyn ManagedBlobFileOps>,
    ) -> Arc<MockApiState> {
        test_persistence(temp)
            .save(&persisted)
            .await
            .expect("synthetic initial state should save");
        let persistence = failure_stage.map_or_else(
            || test_persistence(temp),
            |stage| faulting_test_persistence(temp, stage),
        );
        Arc::new(MockApiState::new_with_blob_file_ops(
            persistence,
            secret_store,
            persisted,
            blob_file_ops,
        ))
    }

    async fn transaction_test_state_failing_on_replace(
        temp: &SyntheticTempDir,
        persisted: PersistedMockState,
        fail_on: u64,
    ) -> Arc<MockApiState> {
        test_persistence(temp)
            .save(&persisted)
            .await
            .expect("synthetic initial state should save");
        let persistence = MockPersistence::new_with_file_ops(
            temp.path.clone(),
            Arc::new(FailOnReplaceCallStateFileOps::new(fail_on)),
        );
        Arc::new(MockApiState::new(
            persistence,
            Arc::new(TestSecretStore),
            persisted,
        ))
    }

    async fn live_conversation(state: &MockApiState, id: &str) -> ConversationDto {
        state
            .conversations
            .read()
            .await
            .get(id)
            .cloned()
            .expect("synthetic conversation should exist")
    }

    fn disk_conversation(temp: &SyntheticTempDir, id: &str) -> Value {
        read_test_state(&test_persistence(temp))["conversations"][id].clone()
    }

    async fn append_synthetic_assistant_transaction(
        state: &Arc<MockApiState>,
        conversation_id: &str,
        text: &str,
    ) -> Result<(), StateMutationError> {
        let conversation_id = conversation_id.to_string();
        let text = text.to_string();
        transact_persisted_state(
            state,
            PureStateMutationScope::ConversationGeneration,
            move |staged| {
                let node_id = next_staged_id(staged, "node");
                let message_id = next_staged_id(staged, "msg");
                let timestamp = now_iso();
                let conversation = staged
                    .conversations
                    .get_mut(&conversation_id)
                    .ok_or(StateMutationError::NotFound("Conversation not found"))?;
                conversation.messages.push(MessageNodeDto {
                    id: node_id,
                    messages: vec![MessageDto {
                        id: message_id,
                        role: "ASSISTANT".to_string(),
                        parts: vec![json!({ "type": "text", "text": text })],
                        annotations: None,
                        created_at: timestamp.clone(),
                        finished_at: Some(timestamp),
                        model_id: Some(MOCK_MODEL_ID.to_string()),
                        usage: None,
                        translation: None,
                    }],
                    select_index: 0,
                });
                conversation.update_at = now_millis();
                Ok(())
            },
        )
        .await
    }

    fn synthetic_text_parts(text: &str) -> Vec<Value> {
        vec![json!({ "type": "text", "text": text })]
    }

    fn synthetic_text_conversation(
        conversation_id: &str,
        user_text: &str,
        assistant_text: &str,
    ) -> ConversationDto {
        let timestamp = "2026-01-01T00:00:00Z".to_string();
        ConversationDto {
            id: conversation_id.to_string(),
            assistant_id: MOCK_ASSISTANT_ID.to_string(),
            title: "Synthetic transaction chat".to_string(),
            messages: vec![
                MessageNodeDto {
                    id: format!("{conversation_id}-user-node"),
                    messages: vec![MessageDto {
                        id: format!("{conversation_id}-user-message"),
                        role: "USER".to_string(),
                        parts: synthetic_text_parts(user_text),
                        annotations: None,
                        created_at: timestamp.clone(),
                        finished_at: Some(timestamp.clone()),
                        model_id: None,
                        usage: None,
                        translation: None,
                    }],
                    select_index: 0,
                },
                MessageNodeDto {
                    id: format!("{conversation_id}-assistant-node"),
                    messages: vec![MessageDto {
                        id: format!("{conversation_id}-assistant-message"),
                        role: "ASSISTANT".to_string(),
                        parts: synthetic_text_parts(assistant_text),
                        annotations: None,
                        created_at: timestamp.clone(),
                        finished_at: Some(timestamp),
                        model_id: Some(MOCK_MODEL_ID.to_string()),
                        usage: None,
                        translation: None,
                    }],
                    select_index: 0,
                },
            ],
            truncate_index: -1,
            chat_suggestions: Vec::new(),
            is_pinned: false,
            custom_system_prompt: None,
            mode_injection_ids: Some(Vec::new()),
            lorebook_ids: Some(Vec::new()),
            create_at: 1,
            update_at: 1,
            is_generating: false,
        }
    }

    async fn start_synthetic_generation(
        state: &Arc<MockApiState>,
        conversation_id: &str,
        operation: GenerationOperation,
    ) -> (GenerationHandle, GenerationCommitPlan) {
        let transition_guard = state.generation_transition_mutex.lock().await;
        let handle = match begin_generation(state, conversation_id, operation).await {
            Ok(handle) => handle,
            Err(BeginGenerationError::Conflict) => panic!("synthetic generation should reserve"),
        };
        let plan = match operation {
            GenerationOperation::Send => {
                commit_initial_user_message(
                    state,
                    conversation_id.to_string(),
                    MOCK_ASSISTANT_ID.to_string(),
                    synthetic_text_parts("synthetic user turn"),
                    None,
                    None,
                    10,
                )
                .await
                .expect("synthetic initial user should persist")
                .plan
            }
            GenerationOperation::Regenerate => {
                prepare_regeneration_transaction(state, conversation_id.to_string(), None)
                    .await
                    .expect("synthetic regeneration should prepare")
                    .plan
            }
        };
        assert!(activate_generation(state, conversation_id, &handle).await);
        broadcast_current_conversation_snapshot(state, conversation_id).await;
        broadcast_generation_start(state, conversation_id, &handle).await;
        drop(transition_guard);
        (handle, plan)
    }

    async fn subscribe_generation_events(
        state: &Arc<MockApiState>,
        conversation_id: &str,
    ) -> broadcast::Receiver<SsePayload> {
        conversation_sender(state, conversation_id)
            .await
            .subscribe()
    }

    fn drain_event_names(receiver: &mut broadcast::Receiver<SsePayload>) -> Vec<String> {
        let mut events = Vec::new();
        loop {
            match receiver.try_recv() {
                Ok(payload) => events.push(payload.event),
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
            }
        }
        events
    }

    fn selected_texts(conversation: &ConversationDto) -> Vec<String> {
        conversation
            .messages
            .iter()
            .filter_map(selected_message)
            .filter_map(|message| text_from_parts(&message.parts))
            .collect()
    }

    fn backup_destination(temp: &SyntheticTempDir, label: &str) -> PathBuf {
        let destination = temp.path.join(format!("backup-destination-{label}"));
        std_fs::create_dir_all(&destination)
            .expect("synthetic backup destination should be created");
        destination
    }

    async fn backup_test_state(
        temp: &SyntheticTempDir,
        uploads: Vec<ValidatedFileUpload>,
    ) -> Arc<MockApiState> {
        let state = transaction_test_state_with_secret_store(
            temp,
            default_persisted_state(),
            None,
            Arc::new(PanicOnAccessSecretStore),
        )
        .await;
        if !uploads.is_empty() {
            commit_file_upload_batch(&state, uploads)
                .await
                .expect("synthetic managed files should upload");
        }
        state
    }

    fn read_backup_manifest(package_root: &FilePath) -> BackupManifest {
        serde_json::from_slice(
            &std_fs::read(package_root.join(BACKUP_MANIFEST_FILE_NAME))
                .expect("synthetic backup manifest should be readable"),
        )
        .expect("synthetic backup manifest should be valid")
    }

    fn read_backup_state(package_root: &FilePath) -> PersistedMockState {
        serde_json::from_slice(
            &std_fs::read(package_root.join(BACKUP_STATE_FILE_NAME))
                .expect("synthetic backup state should be readable"),
        )
        .expect("synthetic backup state should be valid")
    }

    async fn active_blob_path(state: &Arc<MockApiState>, index: usize) -> PathBuf {
        let storage_key = state.files.read().await[index].storage_key.clone();
        state
            .blob_store
            .final_path(&storage_key)
            .expect("synthetic storage key should resolve")
    }

    async fn tombstone_synthetic_file(state: &Arc<MockApiState>, file_id: u64) {
        transact_persisted_state(state, PureStateMutationScope::FileMetadata, move |staged| {
            staged
                .files
                .iter_mut()
                .find(|file| file.id == file_id)
                .expect("synthetic managed file should exist")
                .deleted_at = Some("2026-01-01T00:00:00Z".to_string());
            Ok(())
        })
        .await
        .expect("synthetic tombstone should persist");
    }

    fn create_unpublished_backup_package(
        temp: &SyntheticTempDir,
        label: &str,
        mode: BackupExportMode,
        snapshot: &PersistedMockState,
        blob_root: Option<&FilePath>,
    ) -> PathBuf {
        let package = temp.path.join(format!("unpublished-backup-{label}"));
        std_fs::create_dir(&package).expect("synthetic unpublished package should be created");
        write_backup_package_contents(&package, mode, snapshot, blob_root)
            .expect("synthetic package contents should be written");
        package
    }

    fn synthetic_desktop_provider() -> DesktopProviderConfig {
        let id = "synthetic-provider".to_string();
        DesktopProviderConfig {
            secret_ref: secret_ref_for_provider(&id),
            id,
            provider_type: OPENAI_COMPATIBLE_PROVIDER_TYPE.to_string(),
            enabled: true,
            name: "Synthetic Provider".to_string(),
            base_url: "http://127.0.0.1:9999/v1".to_string(),
            models: vec![DesktopProviderModelConfig {
                id: "synthetic-provider-model".to_string(),
                model_id: "synthetic-model-api".to_string(),
                display_name: "Synthetic Model".to_string(),
                input_modalities: default_input_modalities(),
                output_modalities: default_output_modalities(),
            }],
            legacy_model: None,
            custom_headers: Vec::new(),
            custom_body: None,
        }
    }

    fn synthetic_provider_state() -> PersistedMockState {
        let mut persisted = default_persisted_state();
        persisted.id_seq = 100;
        persisted.providers.push(synthetic_desktop_provider());
        sync_settings_with_desktop_providers(&mut persisted.settings, &persisted.providers);
        persisted
    }

    fn synthetic_provider_upsert_request(
        id: Option<&str>,
        api_key: Option<&str>,
    ) -> UpsertDesktopProviderRequest {
        UpsertDesktopProviderRequest {
            id: id.map(ToOwned::to_owned),
            provider_type: Some(OPENAI_COMPATIBLE_PROVIDER_TYPE.to_string()),
            enabled: Some(true),
            name: Some("Synthetic Updated Provider".to_string()),
            base_url: Some("http://127.0.0.1:9999/v1".to_string()),
            models: None,
            model_id: Some("synthetic-model-api".to_string()),
            display_name: Some("Synthetic Model".to_string()),
            api_key: api_key.map(ToOwned::to_owned),
            custom_headers: None,
            custom_body: CustomBodyUpdate::Missing,
        }
    }

    fn synthetic_provider_secret_request(api_key: &str) -> UpsertDesktopProviderRequest {
        UpsertDesktopProviderRequest {
            id: None,
            provider_type: None,
            enabled: None,
            name: None,
            base_url: None,
            models: None,
            model_id: None,
            display_name: None,
            api_key: Some(api_key.to_string()),
            custom_headers: None,
            custom_body: CustomBodyUpdate::Missing,
        }
    }

    fn synthetic_provider_import_document() -> Value {
        json!({
            "version": PROVIDER_IMPORT_EXPORT_VERSION,
            "app": "RikkaDesk",
            "exportedAt": "2026-01-01T00:00:00Z",
            "providers": [{
                "type": OPENAI_COMPATIBLE_PROVIDER_TYPE,
                "enabled": true,
                "name": "Synthetic Imported Provider",
                "baseUrl": "http://127.0.0.1:9999/v1",
                "hasSecret": true,
                "models": [{
                    "modelId": "synthetic-import-model",
                    "displayName": "Synthetic Import Model",
                    "inputModalities": [MODEL_MODALITY_TEXT],
                    "outputModalities": [MODEL_MODALITY_TEXT]
                }],
                "customHeaders": [],
                "customBody": null
            }]
        })
    }

    async fn response_json(response: Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("synthetic response body should be readable");
        serde_json::from_slice(&body).expect("synthetic response body should be JSON")
    }

    fn synthetic_png_upload(display_name: &str) -> ValidatedFileUpload {
        ValidatedFileUpload {
            display_name: display_name.to_string(),
            mime: "image/png".to_string(),
            size_bytes: 12,
            kind: "image".to_string(),
            bytes: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0],
        }
    }

    fn synthetic_text_upload(display_name: &str) -> ValidatedFileUpload {
        let bytes = b"synthetic managed file text".to_vec();
        ValidatedFileUpload {
            display_name: display_name.to_string(),
            mime: "text/plain".to_string(),
            size_bytes: bytes.len() as u64,
            kind: "document".to_string(),
            bytes,
        }
    }

    fn synthetic_blob_paths(state: &MockApiState) -> Vec<PathBuf> {
        let Ok(entries) = std_fs::read_dir(&state.blob_store.blobs_dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect()
    }

    fn synthetic_final_blob_count(state: &MockApiState) -> usize {
        synthetic_blob_paths(state)
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with(FILE_BLOB_TEMP_PREFIX))
            })
            .count()
    }

    fn synthetic_temp_blob_count(state: &MockApiState) -> usize {
        synthetic_blob_paths(state)
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(FILE_BLOB_TEMP_PREFIX))
            })
            .count()
    }

    async fn add_synthetic_file_reference(
        state: &Arc<MockApiState>,
        file_id: u64,
        part_type: &str,
    ) {
        let part_type = part_type.to_string();
        transact_persisted_state(
            state,
            PureStateMutationScope::Conversations,
            move |staged| {
                let message = &mut staged
                    .conversations
                    .get_mut(MOCK_WELCOME_CONVERSATION_ID)
                    .expect("synthetic welcome conversation should exist")
                    .messages[0]
                    .messages[0];
                message.parts.push(json!({
                    "type": part_type,
                    "metadata": { "fileId": file_id }
                }));
                Ok(())
            },
        )
        .await
        .expect("synthetic file reference should persist");
    }

    fn synthetic_managed_file_metadata(id: u64, storage_key: &str) -> ManagedFileMetadata {
        ManagedFileMetadata {
            id,
            storage_key: storage_key.to_string(),
            display_name: "synthetic-file.txt".to_string(),
            mime: "text/plain".to_string(),
            size_bytes: 27,
            sha256: None,
            kind: "document".to_string(),
            relative_path: format!("{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{storage_key}"),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            source: "synthetic".to_string(),
            deleted_at: None,
        }
    }

    async fn seeded_file_transaction_state(
        temp: &SyntheticTempDir,
        failure_stage: Option<TestFailureStage>,
        blob_file_ops: Arc<dyn ManagedBlobFileOps>,
    ) -> Arc<MockApiState> {
        let storage_key = "synthetic-seeded-blob";
        let mut persisted = default_persisted_state();
        persisted.id_seq = 42;
        persisted
            .files
            .push(synthetic_managed_file_metadata(42, storage_key));
        let state = transaction_test_state_with_stores(
            temp,
            persisted,
            failure_stage,
            Arc::new(TestSecretStore),
            blob_file_ops,
        )
        .await;
        std_fs::create_dir_all(&state.blob_store.blobs_dir)
            .expect("synthetic blob directory should be created");
        std_fs::write(
            state
                .blob_store
                .final_path(storage_key)
                .expect("synthetic storage key should be safe"),
            b"synthetic managed file text",
        )
        .expect("synthetic managed blob should be seeded");
        state
    }

    fn is_test_backup_path(path: &FilePath) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("state.v1.corrupt.") || name.starts_with("state.v1.pre-migration.")
            })
    }

    fn state_backup_files(persistence: &MockPersistence, marker: &str) -> Vec<PathBuf> {
        let Ok(entries) = std_fs::read_dir(&persistence.state_dir) else {
            return Vec::new();
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(marker))
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    fn synthetic_state_for_schema(schema_version: u32) -> PersistedMockState {
        let mut persisted = default_persisted_state();
        persisted.schema_version = schema_version;
        persisted.settings["syntheticLoadMarker"] = json!("preserved");
        persisted.files.push(ManagedFileMetadata {
            id: 42,
            storage_key: "synthetic-file-42".to_string(),
            display_name: "synthetic.txt".to_string(),
            mime: "text/plain".to_string(),
            size_bytes: 16,
            sha256: None,
            kind: "document".to_string(),
            relative_path: "files/blobs/synthetic-file-42".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            source: "synthetic".to_string(),
            deleted_at: None,
        });
        persisted.providers.push(DesktopProviderConfig {
            id: "synthetic-provider".to_string(),
            provider_type: OPENAI_COMPATIBLE_PROVIDER_TYPE.to_string(),
            enabled: true,
            name: "Synthetic Provider".to_string(),
            base_url: "http://127.0.0.1:9999/v1".to_string(),
            models: vec![DesktopProviderModelConfig {
                id: "synthetic-model".to_string(),
                model_id: "synthetic-model".to_string(),
                display_name: "Synthetic Model".to_string(),
                input_modalities: vec!["image".to_string(), "text".to_string()],
                output_modalities: vec!["text".to_string()],
            }],
            legacy_model: None,
            secret_ref: "synthetic-provider-reference".to_string(),
            custom_headers: Vec::new(),
            custom_body: None,
        });
        persisted
    }

    async fn assert_state_load_migration(from: u32) {
        let temp = SyntheticTempDir::new("migration");
        let persistence = test_persistence(&temp);
        let persisted = synthetic_state_for_schema(from);
        let original_conversation_ids = persisted
            .conversations
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        let original_files = persisted.files.clone();
        let original_bytes =
            serde_json::to_vec_pretty(&persisted).expect("synthetic state should serialize");
        write_test_state_bytes(&persistence, &original_bytes);

        let loaded = load_persisted_state(&persistence)
            .await
            .expect("migration should succeed");

        assert_eq!(
            loaded.outcome,
            StateLoadOutcome::Migrated {
                from,
                to: STATE_SCHEMA_VERSION,
            }
        );
        assert_eq!(loaded.persisted.schema_version, STATE_SCHEMA_VERSION);
        assert_eq!(
            loaded.persisted.settings["syntheticLoadMarker"],
            json!("preserved")
        );
        assert_eq!(
            loaded
                .persisted
                .conversations
                .keys()
                .cloned()
                .collect::<HashSet<_>>(),
            original_conversation_ids
        );
        if from == LEGACY_STATE_SCHEMA_VERSION {
            assert!(loaded.persisted.providers.is_empty());
        } else {
            let model = &loaded.persisted.providers[0].models[0];
            assert_eq!(
                model.input_modalities,
                vec![
                    MODEL_MODALITY_TEXT.to_string(),
                    MODEL_MODALITY_IMAGE.to_string()
                ]
            );
            assert_eq!(model.output_modalities, default_output_modalities());
        }
        if from == FILE_METADATA_STATE_SCHEMA_VERSION {
            assert!(loaded.persisted.files == original_files);
        } else {
            assert!(loaded.persisted.files.is_empty());
        }

        let backups = state_backup_files(&persistence, ".pre-migration.");
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std_fs::read(&backups[0]).expect("migration backup should be readable"),
            original_bytes
        );
        assert_eq!(
            read_test_state(&persistence)["schemaVersion"],
            json!(STATE_SCHEMA_VERSION)
        );
    }

    #[tokio::test]
    async fn state_load_missing_initializes_default_only_after_persistence() {
        let temp = SyntheticTempDir::new("missing");
        let persistence = test_persistence(&temp);

        let loaded = load_persisted_state(&persistence)
            .await
            .expect("missing state should initialize");

        assert_eq!(loaded.outcome, StateLoadOutcome::InitializedDefault);
        assert_eq!(loaded.persisted.schema_version, STATE_SCHEMA_VERSION);
        assert_eq!(
            read_test_state(&persistence)["schemaVersion"],
            json!(STATE_SCHEMA_VERSION)
        );
        assert!(state_backup_files(&persistence, ".corrupt.").is_empty());
        assert!(state_backup_files(&persistence, ".pre-migration.").is_empty());
    }

    #[tokio::test]
    async fn state_load_missing_persistence_failure_stops_initialization() {
        let temp = SyntheticTempDir::new("missing-persist-failure");
        let persistence = faulting_test_persistence(&temp, TestFailureStage::Write);

        let result = load_persisted_state(&persistence).await;

        assert!(matches!(
            result,
            Err(StateLoadError::PersistFailed {
                purpose: "default initialization",
                ..
            })
        ));
        assert!(!persistence.state_path.exists());
        assert!(state_backup_files(&persistence, ".corrupt.").is_empty());
    }

    #[tokio::test]
    async fn state_load_read_failure_is_fail_closed_without_reset() {
        let temp = SyntheticTempDir::new("read-failure");
        let real = test_persistence(&temp);
        let original = serde_json::to_vec_pretty(&synthetic_state_for_schema(STATE_SCHEMA_VERSION))
            .expect("synthetic state should serialize");
        write_test_state_bytes(&real, &original);
        let persistence = faulting_test_persistence(&temp, TestFailureStage::Read);

        let result = load_persisted_state(&persistence).await;

        assert!(matches!(result, Err(StateLoadError::ReadFailed(_))));
        assert_eq!(
            std_fs::read(&real.state_path).expect("original state should remain"),
            original
        );
        assert!(state_backup_files(&real, ".corrupt.").is_empty());
        assert!(state_backup_files(&real, ".pre-migration.").is_empty());
    }

    #[tokio::test]
    async fn state_load_corrupt_json_preserves_original_and_requires_recovery() {
        let temp = SyntheticTempDir::new("corrupt");
        let persistence = test_persistence(&temp);
        let original = b"{ synthetic malformed JSON".to_vec();
        write_test_state_bytes(&persistence, &original);

        let result = load_persisted_state(&persistence).await;

        assert!(matches!(result, Err(StateLoadError::RecoveryRequired)));
        assert_eq!(
            std_fs::read(&persistence.state_path).expect("original state should remain"),
            original
        );
        let backups = state_backup_files(&persistence, ".corrupt.");
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std_fs::read(&backups[0]).expect("corrupt backup should be readable"),
            original
        );
    }

    #[tokio::test]
    async fn state_load_corrupt_backup_failure_preserves_original_and_stops() {
        let temp = SyntheticTempDir::new("corrupt-backup-failure");
        let real = test_persistence(&temp);
        let original = b"{ synthetic malformed JSON".to_vec();
        write_test_state_bytes(&real, &original);
        let persistence = faulting_test_persistence(&temp, TestFailureStage::BackupCreate);

        let result = load_persisted_state(&persistence).await;

        assert!(matches!(
            result,
            Err(StateLoadError::BackupFailed {
                purpose: "corrupt state",
                ..
            })
        ));
        assert_eq!(
            std_fs::read(&real.state_path).expect("original state should remain"),
            original
        );
        assert!(state_backup_files(&real, ".corrupt.").is_empty());
    }

    #[tokio::test]
    async fn state_load_future_schema_is_fail_closed_without_corrupt_backup() {
        let temp = SyntheticTempDir::new("future-schema");
        let persistence = test_persistence(&temp);
        let original = br#"{"schemaVersion":7,"syntheticMarker":"do-not-log"}"#.to_vec();
        write_test_state_bytes(&persistence, &original);

        let error = match load_persisted_state(&persistence).await {
            Err(error) => error,
            Ok(_) => panic!("future schema should stop startup"),
        };

        assert!(matches!(
            error,
            StateLoadError::UnsupportedFutureSchema {
                found: 7,
                supported: STATE_SCHEMA_VERSION,
            }
        ));
        assert_eq!(
            std_fs::read(&persistence.state_path).expect("future state should remain"),
            original
        );
        assert!(state_backup_files(&persistence, ".corrupt.").is_empty());
        assert!(state_backup_files(&persistence, ".pre-migration.").is_empty());
    }

    #[tokio::test]
    async fn state_load_migration_v1_to_v6() {
        assert_state_load_migration(LEGACY_STATE_SCHEMA_VERSION).await;
    }

    #[tokio::test]
    async fn state_load_migration_v2_to_v6() {
        assert_state_load_migration(PREVIOUS_STATE_SCHEMA_VERSION).await;
    }

    #[tokio::test]
    async fn state_load_migration_v3_to_v6() {
        assert_state_load_migration(MULTI_MODEL_STATE_SCHEMA_VERSION).await;
    }

    #[tokio::test]
    async fn state_load_migration_v4_to_v6() {
        assert_state_load_migration(CUSTOM_REQUEST_CONFIG_STATE_SCHEMA_VERSION).await;
    }

    #[tokio::test]
    async fn state_load_migration_v5_to_v6() {
        assert_state_load_migration(FILE_METADATA_STATE_SCHEMA_VERSION).await;
    }

    #[tokio::test]
    async fn state_load_migration_backup_failure_stops_before_transform() {
        let temp = SyntheticTempDir::new("migration-backup-failure");
        let real = test_persistence(&temp);
        let original = serde_json::to_vec_pretty(&synthetic_state_for_schema(
            FILE_METADATA_STATE_SCHEMA_VERSION,
        ))
        .expect("synthetic state should serialize");
        write_test_state_bytes(&real, &original);
        let persistence = faulting_test_persistence(&temp, TestFailureStage::BackupCreate);

        let result =
            load_persisted_state_with_migrator(&persistence, &UnexpectedStateMigrator).await;

        assert!(matches!(
            result,
            Err(StateLoadError::BackupFailed {
                purpose: "pre-migration state",
                ..
            })
        ));
        assert_eq!(
            std_fs::read(&real.state_path).expect("old state should remain"),
            original
        );
        assert!(state_backup_files(&real, ".pre-migration.").is_empty());
    }

    #[tokio::test]
    async fn state_load_migration_transform_failure_preserves_state_and_backup() {
        let temp = SyntheticTempDir::new("migration-transform-failure");
        let persistence = test_persistence(&temp);
        let original = serde_json::to_vec_pretty(&synthetic_state_for_schema(
            FILE_METADATA_STATE_SCHEMA_VERSION,
        ))
        .expect("synthetic state should serialize");
        write_test_state_bytes(&persistence, &original);

        let result = load_persisted_state_with_migrator(&persistence, &FailingStateMigrator).await;

        assert!(matches!(
            result,
            Err(StateLoadError::MigrationFailed {
                from: FILE_METADATA_STATE_SCHEMA_VERSION,
                reason: "synthetic transform",
            })
        ));
        assert_eq!(
            std_fs::read(&persistence.state_path).expect("old state should remain"),
            original
        );
        let backups = state_backup_files(&persistence, ".pre-migration.");
        assert_eq!(backups.len(), 1);
        assert_eq!(std_fs::read(&backups[0]).unwrap(), original);
    }

    #[tokio::test]
    async fn state_load_migration_persistence_failure_preserves_state_and_backup() {
        let temp = SyntheticTempDir::new("migration-persist-failure");
        let real = test_persistence(&temp);
        let original = serde_json::to_vec_pretty(&synthetic_state_for_schema(
            FILE_METADATA_STATE_SCHEMA_VERSION,
        ))
        .expect("synthetic state should serialize");
        write_test_state_bytes(&real, &original);
        let persistence = faulting_test_persistence(&temp, TestFailureStage::Replace);

        let result = load_persisted_state(&persistence).await;

        assert!(matches!(
            result,
            Err(StateLoadError::PersistFailed {
                purpose: "migration",
                ..
            })
        ));
        assert_eq!(
            std_fs::read(&real.state_path).expect("old state should remain"),
            original
        );
        let backups = state_backup_files(&real, ".pre-migration.");
        assert_eq!(backups.len(), 1);
        assert_eq!(std_fs::read(&backups[0]).unwrap(), original);
    }

    #[tokio::test]
    async fn state_load_corrupt_backup_filename_collision_does_not_overwrite() {
        let temp = SyntheticTempDir::new("backup-collision");
        let persistence = MockPersistence::new_with_file_ops(
            temp.path.clone(),
            Arc::new(CollisionOnceStateFileOps::new()),
        );
        let original = b"{ synthetic malformed JSON".to_vec();
        write_test_state_bytes(&persistence, &original);

        let result = load_persisted_state(&persistence).await;

        assert!(matches!(result, Err(StateLoadError::RecoveryRequired)));
        let backups = state_backup_files(&persistence, ".corrupt.");
        assert_eq!(backups.len(), 2);
        let contents = backups
            .iter()
            .map(|path| std_fs::read(path).expect("backup should be readable"))
            .collect::<Vec<_>>();
        assert!(contents.contains(&b"synthetic existing backup".to_vec()));
        assert!(contents.contains(&original));
    }

    #[tokio::test]
    async fn state_load_stale_temp_is_ignored_and_preserved() {
        let temp = SyntheticTempDir::new("stale-temp");
        let persistence = test_persistence(&temp);
        let current = serde_json::to_vec_pretty(&synthetic_state_for_schema(STATE_SCHEMA_VERSION))
            .expect("current state should serialize");
        write_test_state_bytes(&persistence, &current);
        let stale_temp = persistence
            .state_dir
            .join(format!("{STATE_TMP_FILE_PREFIX}.999.1"));
        std_fs::write(&stale_temp, br#"{"schemaVersion":7}"#)
            .expect("stale temp should be created");

        let loaded = load_persisted_state(&persistence)
            .await
            .expect("primary state should load");

        assert_eq!(loaded.outcome, StateLoadOutcome::Loaded);
        assert!(stale_temp.exists());
        assert_eq!(
            std_fs::read(&persistence.state_path).expect("primary state should remain"),
            current
        );
    }

    #[tokio::test]
    async fn state_load_error_messages_do_not_expose_content_or_absolute_path() {
        let temp = SyntheticTempDir::new("safe-error");
        let persistence = test_persistence(&temp);
        let original = br#"{"schemaVersion":7,"syntheticMarker":"private-value"}"#.to_vec();
        write_test_state_bytes(&persistence, &original);

        let error = match load_persisted_state(&persistence).await {
            Err(error) => error,
            Ok(_) => panic!("future schema should fail"),
        };
        let message = error.to_string();

        assert!(message.contains("schema 7"));
        assert!(!message.contains("private-value"));
        assert!(!message.contains("state.v1.json"));
        assert!(!message.contains(temp.path.to_string_lossy().as_ref()));
    }

    fn state_temp_files(persistence: &MockPersistence) -> Vec<PathBuf> {
        let Ok(entries) = std_fs::read_dir(&persistence.state_dir) else {
            return Vec::new();
        };

        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(STATE_TMP_FILE_PREFIX))
            })
            .collect()
    }

    async fn assert_failed_save_preserves_state(stage: TestFailureStage) {
        let temp = SyntheticTempDir::new("failure");
        let real = test_persistence(&temp);
        real.save(&json!({ "revision": "original" }))
            .await
            .expect("initial save should succeed");

        let failing = faulting_test_persistence(&temp, stage);
        let result = failing.save(&json!({ "revision": "replacement" })).await;

        assert!(result.is_err());
        assert_eq!(read_test_state(&real)["revision"], json!("original"));
        assert!(state_temp_files(&real).is_empty());
    }

    #[tokio::test]
    async fn state_persistence_first_save_creates_valid_json_without_temp() {
        let temp = SyntheticTempDir::new("first-save");
        let persistence = test_persistence(&temp);

        persistence
            .save(&json!({ "schemaVersion": STATE_SCHEMA_VERSION, "value": 1 }))
            .await
            .expect("first save should succeed");

        assert_eq!(read_test_state(&persistence)["value"], json!(1));
        assert!(state_temp_files(&persistence).is_empty());
    }

    #[tokio::test]
    async fn state_persistence_atomic_replace_updates_existing_state() {
        let temp = SyntheticTempDir::new("replace");
        let persistence = test_persistence(&temp);
        persistence
            .save(&json!({ "revision": 1 }))
            .await
            .expect("initial save should succeed");

        persistence
            .save(&json!({ "revision": 2 }))
            .await
            .expect("replacement save should succeed");

        assert_eq!(read_test_state(&persistence)["revision"], json!(2));
        assert!(state_temp_files(&persistence).is_empty());
    }

    #[tokio::test]
    async fn state_persistence_serialization_failure_preserves_existing_state() {
        let temp = SyntheticTempDir::new("serialization");
        let persistence = test_persistence(&temp);
        persistence
            .save(&json!({ "revision": "original" }))
            .await
            .expect("initial save should succeed");

        let result = persistence.save(&FailingSerialize).await;

        assert!(result.is_err());
        assert_eq!(read_test_state(&persistence)["revision"], json!("original"));
        assert!(state_temp_files(&persistence).is_empty());
    }

    #[tokio::test]
    async fn state_persistence_write_failure_preserves_existing_state() {
        assert_failed_save_preserves_state(TestFailureStage::Write).await;
    }

    #[tokio::test]
    async fn state_persistence_flush_failure_preserves_existing_state() {
        assert_failed_save_preserves_state(TestFailureStage::Flush).await;
    }

    #[tokio::test]
    async fn state_persistence_sync_failure_preserves_existing_state() {
        assert_failed_save_preserves_state(TestFailureStage::Sync).await;
    }

    #[tokio::test]
    async fn state_persistence_replace_failure_preserves_existing_state() {
        assert_failed_save_preserves_state(TestFailureStage::Replace).await;
    }

    #[tokio::test]
    async fn state_persistence_failure_does_not_remove_unrelated_temp_file() {
        let temp = SyntheticTempDir::new("unrelated-temp");
        let real = test_persistence(&temp);
        real.save(&json!({ "revision": "original" }))
            .await
            .expect("initial save should succeed");
        let unrelated = real
            .state_dir
            .join(format!("{STATE_TMP_FILE_PREFIX}.unrelated"));
        std_fs::write(&unrelated, b"synthetic unrelated temp")
            .expect("unrelated temp should be created");

        let failing = faulting_test_persistence(&temp, TestFailureStage::Write);
        assert!(failing.save(&json!({ "revision": "new" })).await.is_err());

        assert!(unrelated.exists());
        let temp_files = state_temp_files(&real);
        assert_eq!(temp_files, vec![unrelated]);
    }

    #[tokio::test]
    async fn state_persistence_concurrent_snapshots_keep_latest_state() {
        let temp = SyntheticTempDir::new("concurrent");
        let persistence = test_persistence(&temp);
        let state = Arc::new(MockApiState::new(
            persistence,
            Arc::new(TestSecretStore),
            default_persisted_state(),
        ));
        let save_lock = state.persistence.save_lock.clone();
        let save_guard = save_lock.lock().await;

        state.settings.write().await["syntheticRevision"] = json!(1);
        let first_state = state.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let first_save = tokio::spawn(async move {
            let _ = started_tx.send(());
            persist_mock_state(&first_state).await
        });
        started_rx.await.expect("first save should start");
        tokio::task::yield_now().await;

        state.settings.write().await["syntheticRevision"] = json!(2);
        let second_state = state.clone();
        let second_save = tokio::spawn(async move { persist_mock_state(&second_state).await });
        drop(save_guard);

        first_save
            .await
            .expect("first save task should finish")
            .expect("first save should succeed");
        second_save
            .await
            .expect("second save task should finish")
            .expect("second save should succeed");

        assert_eq!(
            read_test_state(&state.persistence)["settings"]["syntheticRevision"],
            json!(2)
        );
        assert!(state_temp_files(&state.persistence).is_empty());
    }

    #[tokio::test]
    async fn state_persistence_handler_returns_safe_error_when_replace_fails() {
        let temp = SyntheticTempDir::new("handler-error");
        let real = test_persistence(&temp);
        real.save(&default_persisted_state())
            .await
            .expect("initial state should save");
        let state = Arc::new(MockApiState::new(
            faulting_test_persistence(&temp, TestFailureStage::Replace),
            Arc::new(TestSecretStore),
            default_persisted_state(),
        ));

        let response = update_assistant(
            State(state),
            Json(UpdateAssistantRequest {
                assistant_id: "synthetic-assistant".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should be readable");
        let body = String::from_utf8(body.to_vec()).expect("error response should be UTF-8");
        assert!(body.contains("Local state could not be saved"));
        assert!(!body.contains("synthetic-assistant"));
        assert!(!body.contains("state.v1.json"));
        assert!(!body.contains(temp.path.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn staged_transaction_success_updates_disk_live_and_revision() {
        let temp = SyntheticTempDir::new("staged-success");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        transact_persisted_state(&state, PureStateMutationScope::Settings, |staged| {
            staged.settings["syntheticTransaction"] = json!("committed");
            Ok(())
        })
        .await
        .expect("staged transaction should commit");

        assert_eq!(
            state.settings.read().await["syntheticTransaction"],
            json!("committed")
        );
        assert_eq!(
            read_test_state(&test_persistence(&temp))["settings"]["syntheticTransaction"],
            json!("committed")
        );
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), initial_id_seq);
    }

    #[tokio::test]
    async fn transaction_failure_preserves_live_disk_and_revision() {
        let temp = SyntheticTempDir::new("staged-failure");
        let initial = default_persisted_state();
        let state = transaction_test_state(&temp, initial, Some(TestFailureStage::Replace)).await;

        let result = transact_persisted_state(&state, PureStateMutationScope::Settings, |staged| {
            staged.settings["syntheticTransaction"] = json!("must-not-commit");
            Ok(())
        })
        .await;

        assert!(matches!(result, Err(StateMutationError::Persistence(_))));
        assert!(state.settings.read().await["syntheticTransaction"].is_null());
        assert!(
            read_test_state(&test_persistence(&temp))["settings"]["syntheticTransaction"].is_null()
        );
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn staged_transaction_validation_failure_does_not_write_or_commit() {
        let temp = SyntheticTempDir::new("staged-validation");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let before = std_fs::read(&state.persistence.state_path)
            .expect("synthetic state bytes should be readable");

        let result = transact_persisted_state(&state, PureStateMutationScope::Settings, |staged| {
            staged.id_seq += 1;
            Ok(())
        })
        .await;

        assert!(matches!(result, Err(StateMutationError::Validation(_))));
        assert_eq!(
            std_fs::read(&state.persistence.state_path)
                .expect("synthetic state bytes should remain readable"),
            before
        );
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn transaction_failure_staged_state_is_hidden_and_component_locks_are_free() {
        let temp = SyntheticTempDir::new("staged-hidden");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let save_guard = state.persistence.save_lock.lock().await;
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            transact_persisted_state(&task_state, PureStateMutationScope::Settings, |staged| {
                staged.settings["syntheticTransaction"] = json!("hidden");
                Ok(())
            })
            .await
        });

        for _ in 0..100 {
            if state.mutation_transaction_mutex.try_lock().is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
        let visible = tokio::time::timeout(Duration::from_secs(1), async {
            let _barrier = state.commit_barrier.read().await;
            state.settings.read().await["syntheticTransaction"].clone()
        })
        .await
        .expect("component reads must remain available during staged disk wait");
        assert!(visible.is_null());

        drop(save_guard);
        let result = task.await.expect("transaction task should finish");
        assert!(matches!(result, Err(StateMutationError::Persistence(_))));
        assert!(state.settings.read().await["syntheticTransaction"].is_null());
    }

    #[tokio::test]
    async fn staged_transaction_commit_visibility_is_atomic_across_components() {
        let temp = SyntheticTempDir::new("staged-visibility");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let save_guard = state.persistence.save_lock.lock().await;
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            transact_persisted_state(
                &task_state,
                PureStateMutationScope::SettingsAndConversations,
                |staged| {
                    staged.settings["syntheticPair"] = json!("new");
                    staged
                        .conversations
                        .get_mut(MOCK_WELCOME_CONVERSATION_ID)
                        .expect("welcome conversation should exist")
                        .title = "new".to_string();
                    Ok(())
                },
            )
            .await
        });

        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        let barrier_guard = state.commit_barrier.read().await;
        drop(save_guard);

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(bytes) = std_fs::read(&state.persistence.state_path) {
                    if serde_json::from_slice::<Value>(&bytes)
                        .is_ok_and(|value| value["settings"]["syntheticPair"] == json!("new"))
                    {
                        break;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("staged snapshot should persist before live commit");

        let old_settings = state.settings.read().await["syntheticPair"].clone();
        let old_title = live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID)
            .await
            .title;
        assert!(old_settings.is_null());
        assert_eq!(old_title, "RikkaDesk Mock Welcome");

        drop(barrier_guard);
        task.await
            .expect("transaction task should finish")
            .expect("transaction should commit");

        let _barrier = state.commit_barrier.read().await;
        assert_eq!(state.settings.read().await["syntheticPair"], json!("new"));
        assert_eq!(
            live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID)
                .await
                .title,
            "new"
        );
    }

    #[tokio::test]
    async fn mutation_transaction_concurrent_updates_are_serialized() {
        let temp = SyntheticTempDir::new("staged-concurrent");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let start = Arc::new(tokio::sync::Barrier::new(4));
        let mut tasks = Vec::new();

        for marker in ["one", "two", "three"] {
            let task_state = state.clone();
            let task_start = start.clone();
            tasks.push(tokio::spawn(async move {
                task_start.wait().await;
                transact_persisted_state(
                    &task_state,
                    PureStateMutationScope::Settings,
                    move |staged| {
                        staged.settings[format!("synthetic-{marker}")] = json!(true);
                        Ok(())
                    },
                )
                .await
            }));
        }

        start.wait().await;
        for task in tasks {
            task.await
                .expect("concurrent transaction should finish")
                .expect("concurrent transaction should commit");
        }

        let settings = state.settings.read().await;
        assert_eq!(settings["synthetic-one"], json!(true));
        assert_eq!(settings["synthetic-two"], json!(true));
        assert_eq!(settings["synthetic-three"], json!(true));
        assert_eq!(state.revision.load(Ordering::Acquire), 3);
        let disk = read_test_state(&test_persistence(&temp));
        assert_eq!(disk["settings"]["synthetic-one"], json!(true));
        assert_eq!(disk["settings"]["synthetic-two"], json!(true));
        assert_eq!(disk["settings"]["synthetic-three"], json!(true));
    }

    #[tokio::test]
    async fn mutation_transaction_older_commit_cannot_overwrite_newer_commit() {
        let temp = SyntheticTempDir::new("staged-order");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let queue_guard = state.mutation_transaction_mutex.lock().await;

        let first_state = state.clone();
        let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
        let first = tokio::spawn(async move {
            let _ = first_started_tx.send(());
            transact_persisted_state(&first_state, PureStateMutationScope::Settings, |staged| {
                staged.settings["syntheticOrder"] = json!(1);
                Ok(())
            })
            .await
        });
        first_started_rx
            .await
            .expect("first transaction should queue");
        tokio::task::yield_now().await;

        let second_state = state.clone();
        let (second_started_tx, second_started_rx) = tokio::sync::oneshot::channel();
        let second = tokio::spawn(async move {
            let _ = second_started_tx.send(());
            transact_persisted_state(&second_state, PureStateMutationScope::Settings, |staged| {
                staged.settings["syntheticOrder"] = json!(2);
                Ok(())
            })
            .await
        });
        second_started_rx
            .await
            .expect("second transaction should queue");
        tokio::task::yield_now().await;
        drop(queue_guard);

        first
            .await
            .expect("first transaction should finish")
            .expect("first transaction should commit");
        second
            .await
            .expect("second transaction should finish")
            .expect("second transaction should commit");

        assert_eq!(state.settings.read().await["syntheticOrder"], json!(2));
        assert_eq!(
            read_test_state(&test_persistence(&temp))["settings"]["syntheticOrder"],
            json!(2)
        );
    }

    #[tokio::test]
    async fn transaction_failure_title_endpoint_preserves_live_and_disk() {
        let temp = SyntheticTempDir::new("title-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;

        let response = update_conversation_title(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
            Json(UpdateConversationTitleRequest {
                title: "must not commit".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID)
                .await
                .title,
            "RikkaDesk Mock Welcome"
        );
        assert_eq!(
            disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID)["title"],
            json!("RikkaDesk Mock Welcome")
        );
    }

    #[tokio::test]
    async fn transaction_failure_pin_endpoint_preserves_live_and_disk() {
        let temp = SyntheticTempDir::new("pin-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;

        let response = toggle_conversation_pin(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID)
                .await
                .is_pinned
        );
        assert_eq!(
            disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID)["isPinned"],
            json!(false)
        );
    }

    #[tokio::test]
    async fn transaction_failure_conversation_delete_preserves_live_and_disk() {
        let temp = SyntheticTempDir::new("conversation-delete-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;

        let response = delete_conversation(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state
            .conversations
            .read()
            .await
            .contains_key(MOCK_WELCOME_CONVERSATION_ID));
        assert!(!disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID).is_null());
    }

    #[tokio::test]
    async fn transaction_failure_settings_endpoint_preserves_live_and_disk() {
        let temp = SyntheticTempDir::new("settings-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let original = state.settings.read().await["favoriteModels"].clone();

        let response = update_favorite_models(
            State(state.clone()),
            Json(UpdateFavoriteModelsRequest {
                model_ids: vec!["synthetic-model".to_string()],
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(state.settings.read().await["favoriteModels"], original);
        assert_eq!(
            read_test_state(&test_persistence(&temp))["settings"]["favoriteModels"],
            original
        );
    }

    #[tokio::test]
    async fn transaction_failure_message_edit_endpoint_preserves_live_and_disk() {
        let temp = SyntheticTempDir::new("message-edit-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let original = live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID).await;

        let response = edit_message(
            State(state.clone()),
            Path((
                MOCK_WELCOME_CONVERSATION_ID.to_string(),
                "welcome-message-1".to_string(),
            )),
            Json(EditMessageRequest {
                parts: vec![json!({ "type": "text", "text": "must not commit" })],
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID).await == original);
        assert_eq!(
            disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID)["messages"][0]["messages"][0]
                ["parts"],
            json!(original.messages[0].messages[0].parts)
        );
    }

    #[tokio::test]
    async fn transaction_failure_message_delete_endpoint_preserves_live_and_disk() {
        let temp = SyntheticTempDir::new("message-delete-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;

        let response = delete_message(
            State(state.clone()),
            Path((
                MOCK_WELCOME_CONVERSATION_ID.to_string(),
                "welcome-message-1".to_string(),
            )),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID)
                .await
                .messages
                .len(),
            1
        );
        assert_eq!(
            disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID)["messages"]
                .as_array()
                .expect("disk messages should be an array")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn staged_transaction_success_response_observes_committed_live_and_disk() {
        let temp = SyntheticTempDir::new("endpoint-success");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        let response = update_conversation_title(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
            Json(UpdateConversationTitleRequest {
                title: "committed title".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID)
                .await
                .title,
            "committed title"
        );
        assert_eq!(
            disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID)["title"],
            json!("committed title")
        );
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn get_does_not_mutate_missing_conversation_detail() {
        let temp = SyntheticTempDir::new("get-detail-pure");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let before_bytes = std_fs::read(&state.persistence.state_path)
            .expect("synthetic state bytes should be readable");
        let before_count = state.conversations.read().await.len();
        let before_id = state.id_seq.load(Ordering::Relaxed);

        let response = conversation_detail(
            State(state.clone()),
            Path("synthetic-missing-conversation".to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.conversations.read().await.len(), before_count);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), before_id);
        assert_eq!(
            std_fs::read(&state.persistence.state_path)
                .expect("synthetic state bytes should remain readable"),
            before_bytes
        );
    }

    #[tokio::test]
    async fn get_does_not_mutate_missing_conversation_stream() {
        let temp = SyntheticTempDir::new("get-stream-pure");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let before_bytes = std_fs::read(&state.persistence.state_path)
            .expect("synthetic state bytes should be readable");
        let before_count = state.conversations.read().await.len();

        let response = conversation_stream(
            State(state.clone()),
            Path("synthetic-missing-stream".to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.conversations.read().await.len(), before_count);
        assert_eq!(
            std_fs::read(&state.persistence.state_path)
                .expect("synthetic state bytes should remain readable"),
            before_bytes
        );
    }

    #[tokio::test]
    async fn get_does_not_mutate_repeated_missing_conversation_reads() {
        let temp = SyntheticTempDir::new("get-repeated-pure");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let before = persisted_snapshot_from_live(&state).await;
        let before_bytes = std_fs::read(&state.persistence.state_path)
            .expect("synthetic state bytes should be readable");

        for _ in 0..3 {
            let _ = conversation_detail(
                State(state.clone()),
                Path("synthetic-repeated-missing".to_string()),
            )
            .await;
        }

        let after = persisted_snapshot_from_live(&state).await;
        assert_eq!(before.settings, after.settings);
        assert!(before.conversations == after.conversations);
        assert!(before.providers == after.providers);
        assert!(before.files == after.files);
        assert_eq!(before.id_seq, after.id_seq);
        assert_eq!(
            std_fs::read(&state.persistence.state_path)
                .expect("synthetic state bytes should remain readable"),
            before_bytes
        );
    }

    #[tokio::test]
    async fn virtual_conversation_get_then_message_post_creates_durable_chat() {
        let temp = SyntheticTempDir::new("virtual-then-post");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "synthetic-new-chat".to_string();

        let response = conversation_detail(State(state.clone()), Path(id.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!state.conversations.read().await.contains_key(&id));

        let response = send_message(
            State(state.clone()),
            Path(id.clone()),
            Json(SendMessageRequest {
                parts: vec![json!({ "type": "text", "text": "synthetic new chat" })],
                mode_injection_ids: None,
                lorebook_ids: None,
                image_input_confirmed: None,
                image_input_mode: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.conversations.read().await.contains_key(&id));
        assert!(!disk_conversation(&temp, &id).is_null());
    }

    #[tokio::test]
    async fn mutation_transaction_coordinates_with_legacy_background_commit() {
        let temp = SyntheticTempDir::new("staged-legacy");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let start = Arc::new(tokio::sync::Barrier::new(3));

        let staged_state = state.clone();
        let staged_start = start.clone();
        let staged = tokio::spawn(async move {
            staged_start.wait().await;
            transact_persisted_state(
                &staged_state,
                PureStateMutationScope::Conversations,
                |snapshot| {
                    snapshot
                        .conversations
                        .get_mut(MOCK_WELCOME_CONVERSATION_ID)
                        .expect("welcome conversation should exist")
                        .title = "transaction title".to_string();
                    Ok(())
                },
            )
            .await
        });

        let legacy_state = state.clone();
        let legacy_start = start.clone();
        let legacy = tokio::spawn(async move {
            legacy_start.wait().await;
            append_synthetic_assistant_transaction(
                &legacy_state,
                MOCK_WELCOME_CONVERSATION_ID,
                "synthetic background reply",
            )
            .await
        });

        start.wait().await;
        tokio::time::timeout(Duration::from_secs(5), async {
            staged
                .await
                .expect("staged transaction should finish")
                .expect("staged transaction should commit");
            legacy
                .await
                .expect("legacy mutation should finish")
                .expect("legacy mutation should persist");
        })
        .await
        .expect("coordinated mutations must not deadlock");

        let conversation = live_conversation(&state, MOCK_WELCOME_CONVERSATION_ID).await;
        assert_eq!(conversation.title, "transaction title");
        assert_eq!(conversation.messages.len(), 2);
        let disk = disk_conversation(&temp, MOCK_WELCOME_CONVERSATION_ID);
        assert_eq!(disk["title"], json!("transaction title"));
        assert_eq!(
            disk["messages"]
                .as_array()
                .expect("disk messages should be an array")
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn mutation_transaction_no_deadlock_across_category_a_and_legacy_commits() {
        let temp = SyntheticTempDir::new("staged-no-deadlock");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        tokio::time::timeout(Duration::from_secs(5), async {
            let title_state = state.clone();
            let title = tokio::spawn(async move {
                update_conversation_title(
                    State(title_state),
                    Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
                    Json(UpdateConversationTitleRequest {
                        title: "no deadlock".to_string(),
                    }),
                )
                .await
                .into_response()
                .status()
            });
            let settings_state = state.clone();
            let settings = tokio::spawn(async move {
                update_assistant(
                    State(settings_state),
                    Json(UpdateAssistantRequest {
                        assistant_id: "synthetic-assistant".to_string(),
                    }),
                )
                .await
                .into_response()
                .status()
            });
            let legacy_state = state.clone();
            let legacy = tokio::spawn(async move {
                append_synthetic_assistant_transaction(
                    &legacy_state,
                    MOCK_WELCOME_CONVERSATION_ID,
                    "synthetic no-deadlock reply",
                )
                .await
            });

            assert_eq!(
                title.await.expect("title task should finish"),
                StatusCode::OK
            );
            assert_eq!(
                settings.await.expect("settings task should finish"),
                StatusCode::OK
            );
            legacy
                .await
                .expect("legacy task should finish")
                .expect("legacy task should persist");
        })
        .await
        .expect("mixed mutation tasks must finish before timeout");

        let disk = read_test_state(&test_persistence(&temp));
        assert_eq!(disk["schemaVersion"], json!(STATE_SCHEMA_VERSION));
    }

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

    #[tokio::test]
    async fn file_transaction_single_upload_success() {
        let temp = SyntheticTempDir::new("file-single-upload");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        let uploaded =
            commit_file_upload_batch(&state, vec![synthetic_png_upload("synthetic-image.png")])
                .await
                .expect("synthetic upload should commit");
        let file_id = uploaded[0].id;

        assert_eq!(uploaded.len(), 1);
        assert_eq!(state.files.read().await.len(), 1);
        assert_eq!(synthetic_final_blob_count(&state), 1);
        assert_eq!(synthetic_temp_blob_count(&state), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
        assert_eq!(
            read_test_state(&test_persistence(&temp))["files"]
                .as_array()
                .expect("disk files should be an array")
                .len(),
            1
        );

        let response = file_path(State(state), Path(file_id)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("x-content-type-options"),
            Some(&HeaderValue::from_static("nosniff"))
        );
    }

    #[tokio::test]
    async fn file_transaction_batch_upload_success() {
        let temp = SyntheticTempDir::new("file-batch-upload");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        let uploaded = commit_file_upload_batch(
            &state,
            vec![
                synthetic_png_upload("synthetic-one.png"),
                synthetic_text_upload("synthetic-two.txt"),
            ],
        )
        .await
        .expect("synthetic batch should commit");
        let ids = uploaded.iter().map(|file| file.id).collect::<HashSet<_>>();
        let storage_keys = state
            .files
            .read()
            .await
            .iter()
            .map(|file| file.storage_key.clone())
            .collect::<HashSet<_>>();

        assert_eq!(uploaded.len(), 2);
        assert_eq!(ids.len(), 2);
        assert_eq!(storage_keys.len(), 2);
        assert_eq!(synthetic_final_blob_count(&state), 2);
        assert_eq!(synthetic_temp_blob_count(&state), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
        assert_eq!(
            read_test_state(&test_persistence(&temp))["files"]
                .as_array()
                .expect("disk files should be an array")
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn file_transaction_blob_write_failure_preserves_state() {
        let temp = SyntheticTempDir::new("file-write-failure");
        let state = transaction_test_state_with_stores(
            &temp,
            default_persisted_state(),
            None,
            Arc::new(TestSecretStore),
            Arc::new(FaultingManagedBlobFileOps::fail_write_at(1)),
        )
        .await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        let result = commit_file_upload_batch(
            &state,
            vec![synthetic_png_upload("synthetic-write-failure.png")],
        )
        .await;

        assert!(matches!(
            result,
            Err(FileUploadTransactionError::BlobOperation)
        ));
        assert!(state.files.read().await.is_empty());
        assert!(read_test_state(&test_persistence(&temp))["files"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert_eq!(synthetic_final_blob_count(&state), 0);
        assert_eq!(synthetic_temp_blob_count(&state), 0);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), initial_id_seq);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn file_transaction_blob_publish_failure_mid_batch_is_atomic() {
        let temp = SyntheticTempDir::new("file-publish-mid-batch");
        let state = transaction_test_state_with_stores(
            &temp,
            default_persisted_state(),
            None,
            Arc::new(TestSecretStore),
            Arc::new(FaultingManagedBlobFileOps::fail_publish_at(2)),
        )
        .await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        let result = commit_file_upload_batch(
            &state,
            vec![
                synthetic_png_upload("synthetic-first.png"),
                synthetic_text_upload("synthetic-second.txt"),
            ],
        )
        .await;

        assert!(matches!(
            result,
            Err(FileUploadTransactionError::BlobOperation)
        ));
        assert!(state.files.read().await.is_empty());
        assert_eq!(synthetic_final_blob_count(&state), 0);
        assert_eq!(synthetic_temp_blob_count(&state), 0);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), initial_id_seq);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn blob_compensation_state_failure_removes_published_blobs() {
        let temp = SyntheticTempDir::new("blob-state-compensation");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        let result = commit_file_upload_batch(
            &state,
            vec![
                synthetic_png_upload("synthetic-state-failure.png"),
                synthetic_text_upload("synthetic-state-failure.txt"),
            ],
        )
        .await;

        assert!(matches!(
            result,
            Err(FileUploadTransactionError::State(
                StateMutationError::Persistence(_)
            ))
        ));
        assert!(state.files.read().await.is_empty());
        assert!(read_test_state(&test_persistence(&temp))["files"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert_eq!(synthetic_final_blob_count(&state), 0);
        assert_eq!(synthetic_temp_blob_count(&state), 0);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), initial_id_seq);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn blob_compensation_cleanup_failure_leaves_unreferenced_orphan() {
        let temp = SyntheticTempDir::new("blob-compensation-failure");
        let blob_ops = Arc::new(FaultingManagedBlobFileOps::fail_delete_times(3));
        let state = transaction_test_state_with_stores(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
            Arc::new(TestSecretStore),
            blob_ops.clone(),
        )
        .await;

        let result = commit_file_upload_batch(
            &state,
            vec![synthetic_png_upload("synthetic-cleanup-failure.png")],
        )
        .await;

        assert!(matches!(
            result,
            Err(FileUploadTransactionError::Compensation)
        ));
        assert!(state.files.read().await.is_empty());
        assert!(read_test_state(&test_persistence(&temp))["files"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert_eq!(synthetic_final_blob_count(&state), 1);
        assert_eq!(blob_ops.remove_call_count(), 3);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn file_delete_draft_unreferenced_file_success() {
        let temp = SyntheticTempDir::new("file-delete-unreferenced");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let file_id =
            commit_file_upload_batch(&state, vec![synthetic_text_upload("synthetic-draft.txt")])
                .await
                .expect("synthetic draft should upload")[0]
                .id;

        let response = delete_file(State(state.clone()), Path(file_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.files.read().await[0].deleted_at.is_some());
        assert!(read_test_state(&test_persistence(&temp))["files"][0]["deletedAt"].is_string());
        assert_eq!(synthetic_final_blob_count(&state), 0);
        assert_eq!(
            file_path(State(state), Path(file_id))
                .await
                .into_response()
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn file_delete_persistence_failure_preserves_blob() {
        let temp = SyntheticTempDir::new("file-delete-state-failure");
        let state = seeded_file_transaction_state(
            &temp,
            Some(TestFailureStage::Replace),
            Arc::new(RealManagedBlobFileOps),
        )
        .await;

        let response = delete_file(State(state.clone()), Path(42))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.files.read().await[0].deleted_at.is_none());
        assert!(read_test_state(&test_persistence(&temp))["files"][0]["deletedAt"].is_null());
        assert_eq!(synthetic_final_blob_count(&state), 1);
        assert_eq!(
            file_path(State(state), Path(42))
                .await
                .into_response()
                .status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn file_delete_blob_cleanup_failure_is_partial_success() {
        let temp = SyntheticTempDir::new("file-delete-cleanup-failure");
        let blob_ops = Arc::new(FaultingManagedBlobFileOps::fail_delete_times(3));
        let state = seeded_file_transaction_state(&temp, None, blob_ops.clone()).await;

        let response = delete_file(State(state.clone()), Path(42))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.files.read().await[0].deleted_at.is_some());
        assert_eq!(synthetic_final_blob_count(&state), 1);
        assert_eq!(blob_ops.remove_call_count(), 3);
        assert_eq!(
            file_path(State(state), Path(42))
                .await
                .into_response()
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn file_delete_repeated_is_idempotent() {
        let temp = SyntheticTempDir::new("file-delete-idempotent");
        let state =
            seeded_file_transaction_state(&temp, None, Arc::new(RealManagedBlobFileOps)).await;

        let first = delete_file(State(state.clone()), Path(42))
            .await
            .into_response();
        let revision_after_first = state.revision.load(Ordering::Acquire);
        let second = delete_file(State(state.clone()), Path(42))
            .await
            .into_response();

        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(state.revision.load(Ordering::Acquire), revision_after_first);
        assert!(state.files.read().await[0].deleted_at.is_some());
        assert_eq!(synthetic_final_blob_count(&state), 0);
    }

    #[tokio::test]
    async fn file_delete_referenced_file_returns_conflict() {
        let temp = SyntheticTempDir::new("file-delete-referenced");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let file_id = commit_file_upload_batch(
            &state,
            vec![synthetic_png_upload("synthetic-referenced.png")],
        )
        .await
        .expect("synthetic file should upload")[0]
            .id;
        add_synthetic_file_reference(&state, file_id, "image").await;
        let revision_before_delete = state.revision.load(Ordering::Acquire);

        let response = delete_file(State(state.clone()), Path(file_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(state.files.read().await[0].deleted_at.is_none());
        assert_eq!(synthetic_final_blob_count(&state), 1);
        assert_eq!(
            state.revision.load(Ordering::Acquire),
            revision_before_delete
        );
    }

    #[test]
    fn file_reference_detects_image_part() {
        let mut persisted = default_persisted_state();
        persisted
            .conversations
            .get_mut(MOCK_WELCOME_CONVERSATION_ID)
            .expect("synthetic welcome conversation should exist")
            .messages[0]
            .messages[0]
            .parts
            .push(json!({ "type": "image", "metadata": { "fileId": 42 } }));

        assert!(is_managed_file_referenced(&persisted.conversations, 42));
    }

    #[test]
    fn file_reference_detects_document_part() {
        let mut persisted = default_persisted_state();
        persisted
            .conversations
            .get_mut(MOCK_WELCOME_CONVERSATION_ID)
            .expect("synthetic welcome conversation should exist")
            .messages[0]
            .messages[0]
            .parts
            .push(json!({ "type": "document", "metadata": { "fileId": 42 } }));

        assert!(is_managed_file_referenced(&persisted.conversations, 42));
    }

    #[test]
    fn file_reference_ignores_legacy_url_without_numeric_metadata() {
        let mut persisted = default_persisted_state();
        persisted
            .conversations
            .get_mut(MOCK_WELCOME_CONVERSATION_ID)
            .expect("synthetic welcome conversation should exist")
            .messages[0]
            .messages[0]
            .parts
            .push(json!({
                "type": "image",
                "url": "/api/files/path/42",
                "metadata": { "fileId": "../../42" }
            }));

        assert!(!is_managed_file_referenced(&persisted.conversations, 42));
    }

    #[tokio::test]
    async fn file_reference_deleted_message_does_not_auto_gc_blob() {
        let temp = SyntheticTempDir::new("file-reference-message-delete");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let file_id = commit_file_upload_batch(
            &state,
            vec![synthetic_png_upload("synthetic-message-reference.png")],
        )
        .await
        .expect("synthetic file should upload")[0]
            .id;
        add_synthetic_file_reference(&state, file_id, "image").await;

        let response = delete_message(
            State(state.clone()),
            Path((
                MOCK_WELCOME_CONVERSATION_ID.to_string(),
                "welcome-message-1".to_string(),
            )),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let conversations = state.conversations.read().await;
        assert!(!is_managed_file_referenced(&conversations, file_id));
        drop(conversations);
        assert!(state.files.read().await[0].deleted_at.is_none());
        assert_eq!(synthetic_final_blob_count(&state), 1);
    }

    #[tokio::test]
    async fn file_reference_send_rejects_tombstoned_attachment() {
        let temp = SyntheticTempDir::new("file-reference-send-tombstone");
        let state =
            seeded_file_transaction_state(&temp, None, Arc::new(RealManagedBlobFileOps)).await;
        let delete_response = delete_file(State(state.clone()), Path(42))
            .await
            .into_response();
        assert_eq!(delete_response.status(), StatusCode::OK);
        let revision_after_delete = state.revision.load(Ordering::Acquire);

        let response = send_message(
            State(state.clone()),
            Path("synthetic-tombstoned-attachment-chat".to_string()),
            Json(SendMessageRequest {
                parts: vec![json!({
                    "type": "document",
                    "metadata": { "fileId": 42 }
                })],
                mode_injection_ids: None,
                lorebook_ids: None,
                image_input_confirmed: None,
                image_input_mode: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!state
            .conversations
            .read()
            .await
            .contains_key("synthetic-tombstoned-attachment-chat"));
        assert_eq!(
            state.revision.load(Ordering::Acquire),
            revision_after_delete
        );
    }

    #[tokio::test]
    async fn file_transaction_concurrent_uploads_are_serialized() {
        let temp = SyntheticTempDir::new("file-concurrent-uploads");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        let first_state = state.clone();
        let first = tokio::spawn(async move {
            commit_file_upload_batch(
                &first_state,
                vec![synthetic_png_upload("synthetic-concurrent-one.png")],
            )
            .await
        });
        let second_state = state.clone();
        let second = tokio::spawn(async move {
            commit_file_upload_batch(
                &second_state,
                vec![synthetic_text_upload("synthetic-concurrent-two.txt")],
            )
            .await
        });

        let first = first
            .await
            .expect("first upload task should finish")
            .expect("first upload should commit");
        let second = second
            .await
            .expect("second upload task should finish")
            .expect("second upload should commit");
        let ids = [first[0].id, second[0].id]
            .into_iter()
            .collect::<HashSet<_>>();
        let keys = state
            .files
            .read()
            .await
            .iter()
            .map(|file| file.storage_key.clone())
            .collect::<HashSet<_>>();

        assert_eq!(ids.len(), 2);
        assert_eq!(keys.len(), 2);
        assert_eq!(state.files.read().await.len(), 2);
        assert_eq!(synthetic_final_blob_count(&state), 2);
        assert_eq!(state.revision.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn file_transaction_preserves_concurrent_category_a_update() {
        let temp = SyntheticTempDir::new("file-category-a-concurrent");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        let upload_state = state.clone();
        let upload = tokio::spawn(async move {
            commit_file_upload_batch(
                &upload_state,
                vec![synthetic_png_upload("synthetic-category-a.png")],
            )
            .await
        });
        let settings_state = state.clone();
        let settings = tokio::spawn(async move {
            update_favorite_models(
                State(settings_state),
                Json(UpdateFavoriteModelsRequest {
                    model_ids: vec!["synthetic-favorite-model".to_string()],
                }),
            )
            .await
            .into_response()
            .status()
        });

        upload
            .await
            .expect("upload task should finish")
            .expect("upload should commit");
        assert_eq!(
            settings.await.expect("settings task should finish"),
            StatusCode::OK
        );
        assert_eq!(state.files.read().await.len(), 1);
        assert_eq!(
            state.settings.read().await["favoriteModels"],
            json!(["synthetic-favorite-model"])
        );
        let disk = read_test_state(&test_persistence(&temp));
        assert_eq!(disk["files"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            disk["settings"]["favoriteModels"],
            json!(["synthetic-favorite-model"])
        );
    }

    #[tokio::test]
    async fn file_transaction_no_deadlock_with_provider_transaction() {
        let temp = SyntheticTempDir::new("file-provider-concurrent");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state = transaction_test_state_with_stores(
            &temp,
            persisted,
            None,
            secret_store.clone(),
            Arc::new(RealManagedBlobFileOps),
        )
        .await;

        let upload_state = state.clone();
        let upload = tokio::spawn(async move {
            commit_file_upload_batch(
                &upload_state,
                vec![synthetic_png_upload("synthetic-provider-concurrent.png")],
            )
            .await
        });
        let provider_state = state.clone();
        let provider = tokio::spawn(async move {
            update_desktop_provider_secret(
                State(provider_state),
                Path("synthetic-provider".to_string()),
                Json(synthetic_provider_secret_request(
                    "synthetic-file-provider-key",
                )),
            )
            .await
            .into_response()
            .status()
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            upload
                .await
                .expect("upload task should finish")
                .expect("upload should commit");
            assert_eq!(
                provider.await.expect("provider task should finish"),
                StatusCode::OK
            );
        })
        .await
        .expect("file and provider transactions must not deadlock");

        assert_eq!(state.files.read().await.len(), 1);
        let current_secret_ref = state.providers.read().await[0].secret_ref.clone();
        assert!(secret_store.contains(&current_secret_ref));
        assert_eq!(state.revision.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn file_transaction_delete_and_get_are_serialized() {
        let temp = SyntheticTempDir::new("file-delete-get-concurrent");
        let state =
            seeded_file_transaction_state(&temp, None, Arc::new(RealManagedBlobFileOps)).await;

        let read_state = state.clone();
        let read = tokio::spawn(async move {
            file_path(State(read_state), Path(42))
                .await
                .into_response()
                .status()
        });
        let delete_state = state.clone();
        let delete = tokio::spawn(async move {
            delete_file(State(delete_state), Path(42))
                .await
                .into_response()
                .status()
        });

        let read_status = read.await.expect("read task should finish");
        let delete_status = delete.await.expect("delete task should finish");
        assert!(matches!(
            read_status,
            StatusCode::OK | StatusCode::NOT_FOUND
        ));
        assert_eq!(delete_status, StatusCode::OK);
        assert_eq!(
            file_path(State(state), Path(42))
                .await
                .into_response()
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn file_transaction_safe_errors_hide_blob_material() {
        let response =
            file_upload_transaction_error_response(FileUploadTransactionError::Compensation);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("safe file error body should be readable");
        let body = String::from_utf8(body.to_vec()).expect("safe file error should be UTF-8");

        assert!(!body.contains("synthetic managed file text"));
        assert!(!body.contains("synthetic-file.txt"));
        assert!(!body.contains("synthetic-seeded-blob"));
        assert!(!body.contains("state.v1.json"));
        assert!(!body.contains("C:\\"));
    }

    #[tokio::test]
    async fn file_transaction_uses_synthetic_blob_root_only() {
        let temp = SyntheticTempDir::new("file-synthetic-root");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;

        commit_file_upload_batch(&state, vec![synthetic_png_upload("synthetic-root.png")])
            .await
            .expect("synthetic upload should commit");

        assert!(state.blob_store.blobs_dir.starts_with(&temp.path));
        assert_eq!(synthetic_final_blob_count(&state), 1);
    }

    #[tokio::test]
    async fn provider_import_transaction_success_has_no_secret() {
        let temp = SyntheticTempDir::new("provider-import-success");
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        let state = transaction_test_state_with_secret_store(
            &temp,
            default_persisted_state(),
            None,
            secret_store.clone(),
        )
        .await;

        let response = confirm_desktop_provider_import(
            State(state.clone()),
            Ok(Json(synthetic_provider_import_document())),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;

        assert_eq!(body["importedCount"], json!(1));
        assert_eq!(body["providers"][0]["hasSecret"], json!(false));
        assert_eq!(state.providers.read().await.len(), 1);
        assert_eq!(secret_store.set_call_count(), 0);
        assert_eq!(secret_store.len(), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
        assert_eq!(
            read_test_state(&test_persistence(&temp))["providers"]
                .as_array()
                .expect("disk providers should be an array")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn provider_import_transaction_failure_preserves_state() {
        let temp = SyntheticTempDir::new("provider-import-failure");
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        let state = transaction_test_state_with_secret_store(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
            secret_store.clone(),
        )
        .await;

        let response = confirm_desktop_provider_import(
            State(state.clone()),
            Ok(Json(synthetic_provider_import_document())),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.providers.read().await.is_empty());
        assert!(read_test_state(&test_persistence(&temp))["providers"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert_eq!(secret_store.set_call_count(), 0);
        assert_eq!(secret_store.delete_call_count(), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn provider_transaction_blank_key_upsert_preserves_secret() {
        let temp = SyntheticTempDir::new("provider-blank-key");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response = upsert_desktop_provider(
            State(state.clone()),
            Json(synthetic_provider_upsert_request(
                Some("synthetic-provider"),
                Some("   "),
            )),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;

        assert_eq!(body["hasSecret"], json!(true));
        assert!(state.providers.read().await[0].secret_ref == old_secret_ref);
        assert!(secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.set_call_count(), 0);
        assert_eq!(secret_store.delete_call_count(), 0);
    }

    #[tokio::test]
    async fn provider_transaction_create_key_success() {
        let temp = SyntheticTempDir::new("provider-create-key");
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        let state = transaction_test_state_with_secret_store(
            &temp,
            default_persisted_state(),
            None,
            secret_store.clone(),
        )
        .await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        let response = upsert_desktop_provider(
            State(state.clone()),
            Json(synthetic_provider_upsert_request(
                None,
                Some("synthetic-create-key"),
            )),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let providers = state.providers.read().await;
        let secret_ref = providers[0].secret_ref.clone();

        assert_eq!(body["hasSecret"], json!(true));
        assert!(is_controlled_provider_secret_ref(&secret_ref));
        assert!(secret_store.contains(&secret_ref));
        assert_eq!(secret_store.len(), 1);
        assert!(state.id_seq.load(Ordering::Relaxed) > initial_id_seq);
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
        assert!(
            read_test_state(&test_persistence(&temp))["providers"][0]["secretRef"]
                == json!(secret_ref)
        );
    }

    #[tokio::test]
    async fn provider_transaction_secret_write_failure_preserves_state() {
        let temp = SyntheticTempDir::new("provider-secret-write-failure");
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.fail_next_set();
        let state = transaction_test_state_with_secret_store(
            &temp,
            default_persisted_state(),
            None,
            secret_store.clone(),
        )
        .await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        let response = upsert_desktop_provider(
            State(state.clone()),
            Json(synthetic_provider_upsert_request(
                None,
                Some("synthetic-write-failure-key"),
            )),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.providers.read().await.is_empty());
        assert!(read_test_state(&test_persistence(&temp))["providers"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert_eq!(secret_store.len(), 0);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), initial_id_seq);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn secret_compensation_removes_new_secret_after_create_persistence_failure() {
        let temp = SyntheticTempDir::new("secret-compensation-create");
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        let state = transaction_test_state_with_secret_store(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
            secret_store.clone(),
        )
        .await;
        let initial_id_seq = state.id_seq.load(Ordering::Relaxed);

        let response = upsert_desktop_provider(
            State(state.clone()),
            Json(synthetic_provider_upsert_request(
                None,
                Some("synthetic-compensation-key"),
            )),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.providers.read().await.is_empty());
        assert_eq!(secret_store.set_call_count(), 1);
        assert_eq!(secret_store.delete_call_count(), 1);
        assert_eq!(secret_store.len(), 0);
        assert_eq!(state.id_seq.load(Ordering::Relaxed), initial_id_seq);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn secret_compensation_retries_transient_delete_failure() {
        let temp = SyntheticTempDir::new("secret-compensation-retry");
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.fail_next_delete();
        let state = transaction_test_state_with_secret_store(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
            secret_store.clone(),
        )
        .await;

        let response = upsert_desktop_provider(
            State(state.clone()),
            Json(synthetic_provider_upsert_request(
                None,
                Some("synthetic-compensation-retry-key"),
            )),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.providers.read().await.is_empty());
        assert_eq!(secret_store.delete_call_count(), 2);
        assert_eq!(secret_store.len(), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn provider_transaction_key_update_success_rotates_secret() {
        let temp = SyntheticTempDir::new("provider-key-update");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response = update_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
            Json(synthetic_provider_secret_request("synthetic-update-key")),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let new_secret_ref = state.providers.read().await[0].secret_ref.clone();

        assert_eq!(body["hasSecret"], json!(true));
        assert!(new_secret_ref != old_secret_ref);
        assert!(secret_store.contains(&new_secret_ref));
        assert!(!secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.len(), 1);
        assert!(
            read_test_state(&test_persistence(&temp))["providers"][0]["secretRef"]
                == json!(new_secret_ref)
        );
    }

    #[tokio::test]
    async fn secret_compensation_removes_new_secret_after_update_persistence_failure() {
        let temp = SyntheticTempDir::new("secret-compensation-update");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state = transaction_test_state_with_secret_store(
            &temp,
            persisted,
            Some(TestFailureStage::Replace),
            secret_store.clone(),
        )
        .await;

        let response = update_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
            Json(synthetic_provider_secret_request(
                "synthetic-update-failure-key",
            )),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.providers.read().await[0].secret_ref == old_secret_ref);
        assert!(secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.len(), 1);
        assert_eq!(secret_store.delete_call_count(), 1);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
        assert!(
            read_test_state(&test_persistence(&temp))["providers"][0]["secretRef"]
                == json!(old_secret_ref)
        );
    }

    #[tokio::test]
    async fn provider_transaction_key_update_cleanup_failure_is_partial_success() {
        let temp = SyntheticTempDir::new("provider-key-cleanup-failure");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        secret_store.fail_delete_times(3);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response = update_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
            Json(synthetic_provider_secret_request(
                "synthetic-cleanup-failure-key",
            )),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let new_secret_ref = state.providers.read().await[0].secret_ref.clone();

        assert!(new_secret_ref != old_secret_ref);
        assert!(secret_store.contains(&new_secret_ref));
        assert!(secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.len(), 2);
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn key_clear_transaction_success() {
        let temp = SyntheticTempDir::new("key-clear-success");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response = delete_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let cleared_secret_ref = state.providers.read().await[0].secret_ref.clone();

        assert_eq!(body["hasSecret"], json!(false));
        assert!(cleared_secret_ref != old_secret_ref);
        assert!(!secret_store.contains(&old_secret_ref));
        assert!(!secret_store.contains(&cleared_secret_ref));
        assert_eq!(secret_store.len(), 0);
        let responses = desktop_provider_responses(&state)
            .await
            .expect("provider response should succeed");
        assert!(!responses[0].has_secret);
    }

    #[tokio::test]
    async fn key_clear_transaction_persistence_failure_preserves_secret() {
        let temp = SyntheticTempDir::new("key-clear-persistence-failure");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state = transaction_test_state_with_secret_store(
            &temp,
            persisted,
            Some(TestFailureStage::Replace),
            secret_store.clone(),
        )
        .await;

        let response = delete_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.providers.read().await[0].secret_ref == old_secret_ref);
        assert!(secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.delete_call_count(), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn key_clear_transaction_cleanup_failure_is_partial_success() {
        let temp = SyntheticTempDir::new("key-clear-cleanup-failure");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        secret_store.fail_delete_times(3);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response = delete_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let cleared_secret_ref = state.providers.read().await[0].secret_ref.clone();

        assert!(cleared_secret_ref != old_secret_ref);
        assert!(secret_store.contains(&old_secret_ref));
        assert!(!secret_store.contains(&cleared_secret_ref));
        let responses = desktop_provider_responses(&state)
            .await
            .expect("provider response should succeed");
        assert!(!responses[0].has_secret);
    }

    #[tokio::test]
    async fn provider_delete_transaction_success() {
        let temp = SyntheticTempDir::new("provider-delete-success");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response =
            delete_desktop_provider(State(state.clone()), Path("synthetic-provider".to_string()))
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.providers.read().await.is_empty());
        assert_eq!(secret_store.len(), 0);
        assert!(read_test_state(&test_persistence(&temp))["providers"]
            .as_array()
            .is_some_and(Vec::is_empty));
    }

    #[tokio::test]
    async fn provider_delete_transaction_persistence_failure_preserves_provider_and_secret() {
        let temp = SyntheticTempDir::new("provider-delete-persistence-failure");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state = transaction_test_state_with_secret_store(
            &temp,
            persisted,
            Some(TestFailureStage::Replace),
            secret_store.clone(),
        )
        .await;

        let response =
            delete_desktop_provider(State(state.clone()), Path("synthetic-provider".to_string()))
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(state.providers.read().await.len(), 1);
        assert!(secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.delete_call_count(), 0);
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn provider_delete_transaction_cleanup_failure_is_partial_success() {
        let temp = SyntheticTempDir::new("provider-delete-cleanup-failure");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        secret_store.fail_delete_times(3);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let response =
            delete_desktop_provider(State(state.clone()), Path("synthetic-provider".to_string()))
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.providers.read().await.is_empty());
        assert!(secret_store.contains(&old_secret_ref));
        assert!(read_test_state(&test_persistence(&temp))["providers"]
            .as_array()
            .is_some_and(Vec::is_empty));
    }

    #[tokio::test]
    async fn provider_transaction_concurrent_key_updates_are_serialized() {
        let temp = SyntheticTempDir::new("provider-concurrent-key-update");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let first_state = state.clone();
        let first = tokio::spawn(async move {
            update_desktop_provider_secret(
                State(first_state),
                Path("synthetic-provider".to_string()),
                Json(synthetic_provider_secret_request(
                    "synthetic-concurrent-one",
                )),
            )
            .await
            .into_response()
            .status()
        });
        let second_state = state.clone();
        let second = tokio::spawn(async move {
            update_desktop_provider_secret(
                State(second_state),
                Path("synthetic-provider".to_string()),
                Json(synthetic_provider_secret_request(
                    "synthetic-concurrent-two",
                )),
            )
            .await
            .into_response()
            .status()
        });

        assert_eq!(
            first.await.expect("first update should finish"),
            StatusCode::OK
        );
        assert_eq!(
            second.await.expect("second update should finish"),
            StatusCode::OK
        );
        let final_secret_ref = state.providers.read().await[0].secret_ref.clone();
        assert!(secret_store.contains(&final_secret_ref));
        assert!(!secret_store.contains(&old_secret_ref));
        assert_eq!(secret_store.len(), 1);
        assert_eq!(state.revision.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn provider_transaction_preserves_concurrent_category_a_update() {
        let temp = SyntheticTempDir::new("provider-category-a-concurrent");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let provider_state = state.clone();
        let provider_update = tokio::spawn(async move {
            update_desktop_provider_secret(
                State(provider_state),
                Path("synthetic-provider".to_string()),
                Json(synthetic_provider_secret_request(
                    "synthetic-category-a-key",
                )),
            )
            .await
            .into_response()
            .status()
        });
        let settings_state = state.clone();
        let settings_update = tokio::spawn(async move {
            update_favorite_models(
                State(settings_state),
                Json(UpdateFavoriteModelsRequest {
                    model_ids: vec!["synthetic-provider-model".to_string()],
                }),
            )
            .await
            .into_response()
            .status()
        });

        assert_eq!(
            provider_update
                .await
                .expect("provider update should finish"),
            StatusCode::OK
        );
        assert_eq!(
            settings_update
                .await
                .expect("settings update should finish"),
            StatusCode::OK
        );
        assert_eq!(
            state.settings.read().await["favoriteModels"],
            json!(["synthetic-provider-model"])
        );
        assert_eq!(
            read_test_state(&test_persistence(&temp))["settings"]["favoriteModels"],
            json!(["synthetic-provider-model"])
        );
        let final_secret_ref = state.providers.read().await[0].secret_ref.clone();
        assert!(secret_store.contains(&final_secret_ref));
    }

    #[tokio::test]
    async fn provider_transaction_blank_upsert_and_clear_are_serialized() {
        let temp = SyntheticTempDir::new("provider-blank-clear-concurrent");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store.clone())
                .await;

        let upsert_state = state.clone();
        let upsert = tokio::spawn(async move {
            upsert_desktop_provider(
                State(upsert_state),
                Json(synthetic_provider_upsert_request(
                    Some("synthetic-provider"),
                    Some("   "),
                )),
            )
            .await
            .into_response()
            .status()
        });
        let clear_state = state.clone();
        let clear = tokio::spawn(async move {
            delete_desktop_provider_secret(
                State(clear_state),
                Path("synthetic-provider".to_string()),
            )
            .await
            .into_response()
            .status()
        });

        assert_eq!(
            upsert.await.expect("blank upsert should finish"),
            StatusCode::OK
        );
        assert_eq!(
            clear.await.expect("key clear should finish"),
            StatusCode::OK
        );
        assert_eq!(secret_store.set_call_count(), 0);
        assert_eq!(secret_store.len(), 0);
        let responses = desktop_provider_responses(&state)
            .await
            .expect("provider response should succeed");
        assert!(!responses[0].has_secret);
    }

    #[tokio::test]
    async fn provider_transaction_safe_errors_hide_secret_material() {
        let temp = SyntheticTempDir::new("provider-safe-error");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        secret_store.fail_next_set();
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store).await;
        let fake_key = "synthetic-safe-error-key";

        let response = update_desktop_provider_secret(
            State(state),
            Path("synthetic-provider".to_string()),
            Json(synthetic_provider_secret_request(fake_key)),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("safe error body should be readable");
        let body = String::from_utf8(body.to_vec()).expect("safe error body should be UTF-8");

        assert!(!body.contains(fake_key));
        assert!(!body.contains(&old_secret_ref));
        assert!(!body.contains(temp.path.to_string_lossy().as_ref()));
        assert!(!body.contains("synthetic secret"));
    }

    #[tokio::test]
    async fn provider_transaction_has_secret_consistency_matrix() {
        let temp = SyntheticTempDir::new("provider-has-secret-matrix");
        let persisted = synthetic_provider_state();
        let old_secret_ref = persisted.providers[0].secret_ref.clone();
        let secret_store = Arc::new(ProviderTransactionTestSecretStore::default());
        secret_store.seed(&old_secret_ref);
        let state =
            transaction_test_state_with_secret_store(&temp, persisted, None, secret_store).await;

        let before = desktop_provider_responses(&state)
            .await
            .expect("configured provider response should succeed");
        assert!(before[0].has_secret);

        let response = delete_desktop_provider_secret(
            State(state.clone()),
            Path("synthetic-provider".to_string()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let after = desktop_provider_responses(&state)
            .await
            .expect("cleared provider response should succeed");
        assert!(!after[0].has_secret);
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

    #[tokio::test]
    async fn streaming_transaction_initial_send_success() {
        let temp = SyntheticTempDir::new("stream-initial-success");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let initial = commit_initial_user_message(
            &state,
            "stream-initial-success".to_string(),
            MOCK_ASSISTANT_ID.to_string(),
            synthetic_text_parts("synthetic initial turn"),
            None,
            None,
            10,
        )
        .await
        .expect("initial user message should persist");

        assert_eq!(initial.conversation.messages.len(), 1);
        assert_eq!(
            live_conversation(&state, "stream-initial-success")
                .await
                .messages
                .len(),
            1
        );
        assert_eq!(
            disk_conversation(&temp, "stream-initial-success")["messages"]
                .as_array()
                .expect("disk messages should be an array")
                .len(),
            1
        );
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn streaming_transaction_initial_persistence_failure_has_no_start() {
        let temp = SyntheticTempDir::new("stream-initial-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let id = "stream-initial-failure".to_string();
        let mut events = subscribe_generation_events(&state, &id).await;
        let response = send_message(
            State(state.clone()),
            Path(id.clone()),
            Json(SendMessageRequest {
                parts: synthetic_text_parts("synthetic rejected turn"),
                mode_injection_ids: None,
                lorebook_ids: None,
                image_input_confirmed: None,
                image_input_mode: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!state.conversations.read().await.contains_key(&id));
        assert!(!has_active_generation(&state, &id).await);
        assert!(drain_event_names(&mut events).is_empty());
        assert_eq!(state.revision.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn streaming_transaction_virtual_first_post_is_single_durable_commit() {
        let temp = SyntheticTempDir::new("stream-virtual-first");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-virtual-first";
        assert!(!state.conversations.read().await.contains_key(id));

        let initial = commit_initial_user_message(
            &state,
            id.to_string(),
            MOCK_ASSISTANT_ID.to_string(),
            synthetic_text_parts("synthetic virtual first turn"),
            None,
            None,
            10,
        )
        .await
        .expect("virtual conversation should become durable");

        let message = selected_message(&initial.conversation.messages[0])
            .expect("initial message should exist");
        assert!(message.id.starts_with("msg-"));
        assert_eq!(initial.conversation.messages.len(), 1);
        assert!(!disk_conversation(&temp, id).is_null());
        assert_eq!(state.revision.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn streaming_transaction_deltas_are_transient() {
        let temp = SyntheticTempDir::new("stream-transient-delta");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-transient-delta";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        broadcast_generation_delta(
            &state,
            id,
            &handle,
            &plan,
            MOCK_MODEL_ID,
            "synthetic delta",
            "synthetic delta",
        )
        .await;

        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
        assert_eq!(
            disk_conversation(&temp, id)["messages"]
                .as_array()
                .expect("disk messages should be an array")
                .len(),
            1
        );
        let names = drain_event_names(&mut events);
        assert!(names.iter().any(|event| event == "delta"));
        assert!(names.iter().filter(|event| *event == "snapshot").count() >= 2);
    }

    #[tokio::test]
    async fn streaming_transaction_final_commit_success() {
        let temp = SyntheticTempDir::new("stream-final-success");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-final-success";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic final reply".to_string(),
        )
        .await
        .expect("final assistant should persist"));

        let live = live_conversation(&state, id).await;
        assert_eq!(
            selected_texts(&live).last(),
            Some(&"synthetic final reply".to_string())
        );
        assert_eq!(
            disk_conversation(&temp, id)["messages"]
                .as_array()
                .expect("disk messages should be an array")
                .len(),
            2
        );
        assert!(!has_active_generation(&state, id).await);
    }

    #[tokio::test]
    async fn streaming_transaction_final_persistence_failure_preserves_user_only() {
        let temp = SyntheticTempDir::new("stream-final-failure");
        let state =
            transaction_test_state_failing_on_replace(&temp, default_persisted_state(), 2).await;
        let id = "stream-final-failure";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        let revision_after_user = state.revision.load(Ordering::Acquire);

        let result = finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic rejected final".to_string(),
        )
        .await;

        assert!(matches!(result, Err(StateMutationError::Persistence(_))));
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
        assert_eq!(
            disk_conversation(&temp, id)["messages"]
                .as_array()
                .expect("disk messages should be an array")
                .len(),
            1
        );
        assert_eq!(state.revision.load(Ordering::Acquire), revision_after_user);
        let names = drain_event_names(&mut events);
        assert!(names.iter().any(|event| event == "failed"));
        assert!(!names.iter().any(|event| event == "finished"));
    }

    #[tokio::test]
    async fn streaming_transaction_final_commit_without_sse_subscriber() {
        let temp = SyntheticTempDir::new("stream-no-subscriber");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-no-subscriber";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic durable without subscriber".to_string(),
        )
        .await
        .expect("SSE absence must not block persistence"));
        assert_eq!(live_conversation(&state, id).await.messages.len(), 2);
    }

    #[tokio::test]
    async fn streaming_transaction_mock_final_persistence_failure_is_safe() {
        let temp = SyntheticTempDir::new("stream-mock-final-failure");
        let state =
            transaction_test_state_failing_on_replace(&temp, default_persisted_state(), 2).await;
        let id = "stream-mock-final-failure";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        let result = finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            MOCK_REPLY_TEXT.to_string(),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
        let names = drain_event_names(&mut events);
        assert_eq!(names.iter().filter(|event| *event == "failed").count(), 1);
        assert_eq!(names.iter().filter(|event| *event == "finished").count(), 0);
    }

    #[tokio::test]
    async fn generation_registry_configuration_failure_cleans_up() {
        let temp = SyntheticTempDir::new("generation-config-failure");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "generation-config-failure";
        let (handle, _) = start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        assert!(fail_running_generation(&state, id, &handle, "configuration").await);
        assert!(!has_active_generation(&state, id).await);
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
    }

    #[tokio::test]
    async fn generation_registry_provider_failure_cleans_up() {
        let temp = SyntheticTempDir::new("generation-provider-failure");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "generation-provider-failure";
        let (handle, _) = start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        assert!(fail_running_generation(&state, id, &handle, "provider").await);
        assert!(!has_active_generation(&state, id).await);
        assert_eq!(
            disk_conversation(&temp, id)["messages"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn streaming_transaction_malformed_stream_is_safe_failure() {
        let temp = SyntheticTempDir::new("stream-malformed");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-malformed";
        let (handle, _) = start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        assert!(parse_openai_stream_line("data: {not-json}").is_err());
        assert!(fail_running_generation(&state, id, &handle, "provider").await);
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
    }

    #[tokio::test]
    async fn regenerate_transaction_success_atomically_replaces_old_reply() {
        let temp = SyntheticTempDir::new("regenerate-success");
        let id = "regenerate-success";
        let mut persisted = default_persisted_state();
        persisted.conversations.insert(
            id.to_string(),
            synthetic_text_conversation(id, "synthetic question", "synthetic old reply"),
        );
        let state = transaction_test_state(&temp, persisted, None).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Regenerate).await;

        assert_eq!(
            selected_texts(&live_conversation(&state, id).await).last(),
            Some(&"synthetic old reply".to_string())
        );
        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic regenerated reply".to_string(),
        )
        .await
        .expect("regenerated reply should persist"));

        let live = live_conversation(&state, id).await;
        assert_eq!(live.messages.len(), 2);
        assert_eq!(
            selected_texts(&live).last(),
            Some(&"synthetic regenerated reply".to_string())
        );
    }

    #[tokio::test]
    async fn regenerate_transaction_provider_failure_preserves_old_reply() {
        let temp = SyntheticTempDir::new("regenerate-provider-failure");
        let id = "regenerate-provider-failure";
        let mut persisted = default_persisted_state();
        persisted.conversations.insert(
            id.to_string(),
            synthetic_text_conversation(id, "synthetic question", "synthetic old reply"),
        );
        let state = transaction_test_state(&temp, persisted, None).await;
        let (handle, _) =
            start_synthetic_generation(&state, id, GenerationOperation::Regenerate).await;

        assert!(fail_running_generation(&state, id, &handle, "provider").await);
        assert_eq!(
            selected_texts(&live_conversation(&state, id).await).last(),
            Some(&"synthetic old reply".to_string())
        );
        assert_eq!(
            disk_conversation(&temp, id)["messages"][1]["messages"][0]["parts"][0]["text"],
            json!("synthetic old reply")
        );
    }

    #[tokio::test]
    async fn regenerate_transaction_final_persistence_failure_preserves_old_reply() {
        let temp = SyntheticTempDir::new("regenerate-final-failure");
        let id = "regenerate-final-failure";
        let mut persisted = default_persisted_state();
        persisted.conversations.insert(
            id.to_string(),
            synthetic_text_conversation(id, "synthetic question", "synthetic old reply"),
        );
        let state = transaction_test_state_failing_on_replace(&temp, persisted, 2).await;
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Regenerate).await;

        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic rejected regeneration".to_string(),
        )
        .await
        .is_err());
        assert_eq!(
            selected_texts(&live_conversation(&state, id).await).last(),
            Some(&"synthetic old reply".to_string())
        );
        let names = drain_event_names(&mut events);
        assert!(names.iter().any(|event| event == "failed"));
        assert!(!names.iter().any(|event| event == "finished"));
    }

    #[tokio::test]
    async fn regenerate_transaction_edited_target_rejects_stale_finalizer() {
        let temp = SyntheticTempDir::new("regenerate-edited-target");
        let id = "regenerate-edited-target";
        let mut persisted = default_persisted_state();
        persisted.conversations.insert(
            id.to_string(),
            synthetic_text_conversation(id, "synthetic question", "synthetic old reply"),
        );
        let state = transaction_test_state(&temp, persisted, None).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Regenerate).await;
        let target_id = format!("{id}-assistant-message");
        transact_persisted_state(
            &state,
            PureStateMutationScope::Conversations,
            move |staged| {
                let conversation = staged.conversations.get_mut(id).unwrap();
                find_message_mut(conversation, &target_id).unwrap().parts =
                    synthetic_text_parts("synthetic user-edited reply");
                Ok(())
            },
        )
        .await
        .expect("synthetic edit should persist");

        let result = finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic stale regeneration".to_string(),
        )
        .await;
        assert!(matches!(result, Err(StateMutationError::Conflict(_))));
        assert_eq!(
            selected_texts(&live_conversation(&state, id).await).last(),
            Some(&"synthetic user-edited reply".to_string())
        );
    }

    #[tokio::test]
    async fn stop_transaction_active_stream_discards_transient_reply() {
        let temp = SyntheticTempDir::new("stop-active");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stop-active";
        let mut events = subscribe_generation_events(&state, id).await;
        let _ = start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        let response = stop_conversation(State(state.clone()), Path(id.to_string()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
        assert!(!has_active_generation(&state, id).await);
        let names = drain_event_names(&mut events);
        assert_eq!(names.iter().filter(|event| *event == "stopped").count(), 1);
        assert_eq!(names.iter().filter(|event| *event == "finished").count(), 0);
    }

    #[tokio::test]
    async fn stop_transaction_finish_race_has_one_terminal() {
        let temp = SyntheticTempDir::new("stop-finish-race");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stop-finish-race";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        let finish_state = state.clone();
        let finish_handle = handle.clone();
        let finish = tokio::spawn(async move {
            finalize_generation_with_text(
                &finish_state,
                id,
                &finish_handle,
                plan,
                MOCK_MODEL_ID.to_string(),
                "synthetic race reply".to_string(),
            )
            .await
        });
        let stop_state = state.clone();
        let stop = tokio::spawn(async move {
            stop_conversation(State(stop_state), Path(id.to_string()))
                .await
                .into_response()
                .status()
        });

        let _ = finish.await.expect("finish task should complete");
        let stop_status = stop.await.expect("stop task should complete");
        assert!(matches!(
            stop_status,
            StatusCode::OK | StatusCode::NOT_FOUND | StatusCode::CONFLICT
        ));
        let names = drain_event_names(&mut events);
        let terminals = names
            .iter()
            .filter(|event| matches!(event.as_str(), "finished" | "stopped" | "failed"))
            .count();
        assert_eq!(terminals, 1);
        assert!(live_conversation(&state, id).await.messages.len() <= 2);
    }

    #[tokio::test]
    async fn stop_transaction_nonexistent_generation_is_safe_not_found() {
        let temp = SyntheticTempDir::new("stop-missing");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let revision = state.revision.load(Ordering::Acquire);

        let response = stop_conversation(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(state.revision.load(Ordering::Acquire), revision);
    }

    #[tokio::test]
    async fn stop_transaction_persistence_failure_has_failed_not_stopped() {
        let temp = SyntheticTempDir::new("stop-persistence-failure");
        let state =
            transaction_test_state_failing_on_replace(&temp, default_persisted_state(), 2).await;
        let id = "stop-persistence-failure";
        let mut events = subscribe_generation_events(&state, id).await;
        let _ = start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        let response = stop_conversation(State(state.clone()), Path(id.to_string()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!has_active_generation(&state, id).await);
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
        let names = drain_event_names(&mut events);
        assert_eq!(names.iter().filter(|event| *event == "failed").count(), 1);
        assert_eq!(names.iter().filter(|event| *event == "stopped").count(), 0);
    }

    #[tokio::test]
    async fn generation_registry_concurrent_send_same_conversation_conflicts() {
        let temp = SyntheticTempDir::new("generation-same-conflict");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "generation-same-conflict";
        let first = begin_generation(&state, id, GenerationOperation::Send)
            .await
            .expect("first generation should reserve");

        assert!(matches!(
            begin_generation(&state, id, GenerationOperation::Send).await,
            Err(BeginGenerationError::Conflict)
        ));
        assert!(remove_generation_if_current(&state, id, &first).await);
    }

    #[tokio::test]
    async fn streaming_transaction_two_conversations_finalize_independently() {
        let temp = SyntheticTempDir::new("stream-two-conversations");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let first_id = "stream-two-first";
        let second_id = "stream-two-second";
        let (first_handle, first_plan) =
            start_synthetic_generation(&state, first_id, GenerationOperation::Send).await;
        let (second_handle, second_plan) =
            start_synthetic_generation(&state, second_id, GenerationOperation::Send).await;

        let (first, second) = tokio::join!(
            finalize_generation_with_text(
                &state,
                first_id,
                &first_handle,
                first_plan,
                MOCK_MODEL_ID.to_string(),
                "synthetic first reply".to_string(),
            ),
            finalize_generation_with_text(
                &state,
                second_id,
                &second_handle,
                second_plan,
                MOCK_MODEL_ID.to_string(),
                "synthetic second reply".to_string(),
            )
        );
        assert!(first.expect("first final should persist"));
        assert!(second.expect("second final should persist"));
        assert_eq!(live_conversation(&state, first_id).await.messages.len(), 2);
        assert_eq!(live_conversation(&state, second_id).await.messages.len(), 2);
    }

    #[tokio::test]
    async fn generation_registry_stale_finish_cannot_remove_new_generation() {
        let temp = SyntheticTempDir::new("generation-stale-finish");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "generation-stale-finish";
        let (old_handle, old_plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        assert!(remove_generation_if_current(&state, id, &old_handle).await);
        let new_handle = begin_generation(&state, id, GenerationOperation::Send)
            .await
            .expect("new generation should reserve");
        assert!(activate_generation(&state, id, &new_handle).await);

        assert!(!finalize_generation_with_text(
            &state,
            id,
            &old_handle,
            old_plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic stale reply".to_string(),
        )
        .await
        .expect("stale finalizer should be ignored"));
        let generations = state.active_generations.lock().await;
        assert_eq!(
            generations.get(id).unwrap().handle.generation_id,
            new_handle.generation_id
        );
    }

    #[tokio::test]
    async fn generation_registry_conversation_delete_cancels_without_recreation() {
        let temp = SyntheticTempDir::new("generation-delete-conversation");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "generation-delete-conversation";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        let response = delete_conversation(State(state.clone()), Path(id.to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(handle.cancellation.is_cancelled());
        assert!(!finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic late reply".to_string(),
        )
        .await
        .expect("deleted generation should be stale"));
        assert!(!state.conversations.read().await.contains_key(id));
        assert!(disk_conversation(&temp, id).is_null());
    }

    #[tokio::test]
    async fn streaming_transaction_category_a_update_survives_final_commit() {
        let temp = SyntheticTempDir::new("stream-category-a");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-category-a";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;

        let response = update_conversation_title(
            State(state.clone()),
            Path(id.to_string()),
            Json(UpdateConversationTitleRequest {
                title: "synthetic concurrent title".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic final after title".to_string(),
        )
        .await
        .expect("final should preserve latest title"));

        let live = live_conversation(&state, id).await;
        assert_eq!(live.title, "synthetic concurrent title");
        assert_eq!(live.messages.len(), 2);
    }

    #[tokio::test]
    async fn streaming_transaction_file_transaction_and_final_do_not_deadlock() {
        let temp = SyntheticTempDir::new("stream-file-concurrency");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-file-concurrency";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        let file_state = state.clone();
        let file_transaction =
            tokio::spawn(async move {
                transact_persisted_state(&file_state, PureStateMutationScope::FileMetadata, |_| {
                    Ok(())
                })
                .await
            });

        let final_result = tokio::time::timeout(
            Duration::from_secs(5),
            finalize_generation_with_text(
                &state,
                id,
                &handle,
                plan,
                MOCK_MODEL_ID.to_string(),
                "synthetic concurrent final".to_string(),
            ),
        )
        .await
        .expect("final transaction should not deadlock")
        .expect("final transaction should persist");
        assert!(final_result);
        file_transaction
            .await
            .expect("file transaction task should finish")
            .expect("file transaction should persist");
    }

    #[tokio::test]
    async fn stream_event_order_success_is_commit_start_delta_commit_finished() {
        let temp = SyntheticTempDir::new("event-order-success");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "event-order-success";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        broadcast_generation_delta(
            &state,
            id,
            &handle,
            &plan,
            MOCK_MODEL_ID,
            "synthetic event delta",
            "synthetic event delta",
        )
        .await;
        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic event final".to_string(),
        )
        .await
        .expect("event-order final should persist"));

        let names = drain_event_names(&mut events);
        let start = names
            .iter()
            .position(|event| event == "generation-start")
            .unwrap();
        let delta = names.iter().position(|event| event == "delta").unwrap();
        let finished = names.iter().position(|event| event == "finished").unwrap();
        let final_snapshot = names[..finished]
            .iter()
            .rposition(|event| event == "snapshot")
            .unwrap();
        assert!(start < delta);
        assert!(delta < final_snapshot);
        assert!(final_snapshot < finished);
        assert_eq!(live_conversation(&state, id).await.messages.len(), 2);
    }

    #[tokio::test]
    async fn stream_event_order_initial_failure_has_no_generation_events() {
        let temp = SyntheticTempDir::new("event-order-initial-failure");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let id = "event-order-initial-failure".to_string();
        let mut events = subscribe_generation_events(&state, &id).await;

        let response = send_message(
            State(state.clone()),
            Path(id.clone()),
            Json(SendMessageRequest {
                parts: synthetic_text_parts("synthetic initial failure"),
                mode_injection_ids: None,
                lorebook_ids: None,
                image_input_confirmed: None,
                image_input_mode: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let names = drain_event_names(&mut events);
        assert!(!names.iter().any(|event| {
            matches!(
                event.as_str(),
                "generation-start" | "delta" | "finished" | "stopped"
            )
        }));
    }

    #[tokio::test]
    async fn stream_event_order_final_failure_has_failed_without_finished() {
        let temp = SyntheticTempDir::new("event-order-final-failure");
        let state =
            transaction_test_state_failing_on_replace(&temp, default_persisted_state(), 2).await;
        let id = "event-order-final-failure";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        let result = finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic rejected event final".to_string(),
        )
        .await;

        assert!(result.is_err());
        let names = drain_event_names(&mut events);
        assert_eq!(names.iter().filter(|event| *event == "failed").count(), 1);
        assert_eq!(names.iter().filter(|event| *event == "finished").count(), 0);
        assert_eq!(live_conversation(&state, id).await.messages.len(), 1);
    }

    #[tokio::test]
    async fn stream_event_order_exactly_one_terminal_event() {
        let temp = SyntheticTempDir::new("event-order-one-terminal");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "event-order-one-terminal";
        let mut events = subscribe_generation_events(&state, id).await;
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        assert!(finalize_generation_with_text(
            &state,
            id,
            &handle,
            plan,
            MOCK_MODEL_ID.to_string(),
            "synthetic one terminal".to_string(),
        )
        .await
        .expect("final should persist"));
        assert!(!fail_running_generation(&state, id, &handle, "provider").await);

        let names = drain_event_names(&mut events);
        assert_eq!(
            names
                .iter()
                .filter(|event| matches!(event.as_str(), "finished" | "stopped" | "failed"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn generation_registry_restart_drops_runtime_and_keeps_durable_user() {
        let temp = SyntheticTempDir::new("generation-restart");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "generation-restart";
        let _ = start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        assert!(has_active_generation(&state, id).await);

        let persisted: PersistedMockState =
            serde_json::from_value(read_test_state(&test_persistence(&temp)))
                .expect("synthetic disk state should deserialize");
        let restarted = Arc::new(MockApiState::new(
            test_persistence(&temp),
            Arc::new(TestSecretStore),
            persisted,
        ));

        assert!(!has_active_generation(&restarted, id).await);
        let conversation = live_conversation(&restarted, id).await;
        assert_eq!(conversation.messages.len(), 1);
        assert!(!conversation.is_generating);
    }

    #[tokio::test]
    async fn streaming_transaction_safe_errors_hide_content_and_paths() {
        let temp = SyntheticTempDir::new("stream-safe-errors");
        let state = transaction_test_state(
            &temp,
            default_persisted_state(),
            Some(TestFailureStage::Replace),
        )
        .await;
        let id = "stream-safe-errors".to_string();
        let synthetic_content = "synthetic-sensitive-message-marker";
        let response = send_message(
            State(state),
            Path(id),
            Json(SendMessageRequest {
                parts: synthetic_text_parts(synthetic_content),
                mode_injection_ids: None,
                lorebook_ids: None,
                image_input_confirmed: None,
                image_input_mode: None,
            }),
        )
        .await
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("safe error body should be readable");
        let body = String::from_utf8(body.to_vec()).expect("safe error should be UTF-8");

        assert!(!body.contains(synthetic_content));
        assert!(!body.contains(temp.path.to_string_lossy().as_ref()));
        assert!(!body.contains(PROVIDER_SECRET_REF_PREFIX));
        assert!(!body.contains("request body"));
    }

    #[tokio::test]
    async fn streaming_transaction_network_wait_holds_no_state_locks() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let temp = SyntheticTempDir::new("stream-network-no-locks");
        let state = transaction_test_state(&temp, default_persisted_state(), None).await;
        let id = "stream-network-no-locks";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("synthetic loopback listener should bind");
        let address = listener
            .local_addr()
            .expect("synthetic loopback address should exist");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let release = Arc::new(Notify::new());
        let server_release = release.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("synthetic request should connect");
            let mut buffer = [0_u8; 4096];
            let read = socket
                .read(&mut buffer)
                .await
                .expect("synthetic request should be readable");
            assert!(read > 0);
            let _ = started_tx.send(());
            server_release.notified().await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"synthetic reply\"}}]}\n\ndata: [DONE]\n\n",
                )
                .await
                .expect("synthetic SSE response should be written");
        });
        let config = OpenAiChatConfig {
            base_url: format!("http://{address}/v1"),
            model_id: "synthetic-loopback-model".to_string(),
            api_key: "synthetic-loopback-key".to_string(),
            custom_headers: Vec::new(),
            custom_body: None,
            input_modalities: default_input_modalities(),
        };
        let stream_state = state.clone();
        let stream_handle = handle.clone();
        let stream_plan = plan.clone();
        let stream = tokio::spawn(async move {
            stream_openai_compatible_chat(
                &stream_state,
                id,
                &stream_handle,
                &stream_plan,
                MOCK_MODEL_ID,
                &config,
                vec![OpenAiChatMessage {
                    role: "user".to_string(),
                    content: "synthetic loopback request".to_string(),
                }],
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), started_rx)
            .await
            .expect("synthetic network request should start")
            .expect("synthetic start signal should arrive");

        let title_response = tokio::time::timeout(
            Duration::from_secs(2),
            update_conversation_title(
                State(state.clone()),
                Path(id.to_string()),
                Json(UpdateConversationTitleRequest {
                    title: "synthetic title during network".to_string(),
                }),
            ),
        )
        .await
        .expect("Category A mutation must not wait for provider network")
        .into_response();
        assert_eq!(title_response.status(), StatusCode::OK);

        release.notify_waiters();
        let outcome = stream
            .await
            .expect("synthetic stream task should finish")
            .expect("synthetic stream should parse");
        assert!(matches!(outcome, GenerationStreamOutcome::Completed(_)));
        server
            .await
            .expect("synthetic loopback server should finish");
        assert_eq!(
            live_conversation(&state, id).await.title,
            "synthetic title during network"
        );
    }

    #[tokio::test]
    async fn backup_mode_a_export_success() {
        let temp = SyntheticTempDir::new("backup-mode-a-success");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "mode-a-success");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("Mode A export should succeed");

        assert!(package.path.is_dir());
        assert!(package.path.join(BACKUP_MANIFEST_FILE_NAME).is_file());
        assert!(package.path.join(BACKUP_STATE_FILE_NAME).is_file());
        assert!(package.path.join(BACKUP_CHECKSUM_FILE_NAME).is_file());
        assert!(!package.path.join(BACKUP_BLOBS_DIR_NAME).exists());
        assert_eq!(package.manifest.mode, BackupManifestMode::StateOnly);
        validate_backup_package(&package.path).expect("Mode A package should self-validate");
    }

    #[tokio::test]
    async fn backup_mode_a_does_not_read_blobs() {
        let temp = SyntheticTempDir::new("backup-mode-a-no-blob-read");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        std_fs::remove_dir_all(&state.blob_store.blobs_dir)
            .expect("synthetic blob root should be removable");
        let destination = backup_destination(&temp, "mode-a-no-blob-read");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("Mode A should not require managed blobs");

        assert!(package.path.is_dir());
        assert!(!package.path.join(BACKUP_BLOBS_DIR_NAME).exists());
    }

    #[tokio::test]
    async fn backup_mode_a_state_point_in_time() {
        let temp = SyntheticTempDir::new("backup-mode-a-point-in-time");
        let state = backup_test_state(&temp, Vec::new()).await;
        let snapshot = portable_backup_snapshot(&state)
            .await
            .expect("synthetic snapshot should succeed");
        let response = update_conversation_title(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
            Json(UpdateConversationTitleRequest {
                title: "synthetic title after snapshot".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let destination = backup_destination(&temp, "mode-a-point-in-time");
        let package = build_and_publish_backup_package(
            &destination,
            BackupExportMode::ModeA,
            snapshot,
            None,
            1,
        )
        .expect("snapshot package should publish");

        assert_ne!(
            read_backup_state(&package.path).conversations[MOCK_WELCOME_CONVERSATION_ID].title,
            "synthetic title after snapshot"
        );
    }

    #[tokio::test]
    async fn backup_mode_a_contains_no_blob_bytes() {
        let temp = SyntheticTempDir::new("backup-mode-a-no-blob-bytes");
        let state = backup_test_state(&temp, vec![synthetic_text_upload("synthetic.txt")]).await;
        let destination = backup_destination(&temp, "mode-a-no-blob-bytes");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("Mode A export should succeed");
        let package_bytes = std_fs::read(package.path.join(BACKUP_STATE_FILE_NAME)).unwrap();

        assert!(!String::from_utf8_lossy(&package_bytes).contains("synthetic managed file text"));
        assert!(!package.path.join(BACKUP_BLOBS_DIR_NAME).exists());
    }

    #[tokio::test]
    async fn backup_mode_a_sanitizes_provider_secret_references() {
        let temp = SyntheticTempDir::new("backup-mode-a-secret-ref");
        let persisted = synthetic_provider_state();
        let source_ref = persisted.providers[0].secret_ref.clone();
        let state = transaction_test_state_with_secret_store(
            &temp,
            persisted,
            None,
            Arc::new(PanicOnAccessSecretStore),
        )
        .await;
        let destination = backup_destination(&temp, "mode-a-secret-ref");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("Mode A export should not access secrets");
        let backup_state = read_backup_state(&package.path);

        assert_ne!(backup_state.providers[0].secret_ref, source_ref);
        assert!(backup_state.providers[0]
            .secret_ref
            .contains(BACKUP_SECRET_REF_MARKER));
        assert!(!serde_json::to_string(&backup_state)
            .unwrap()
            .contains(&source_ref));
    }

    #[tokio::test]
    async fn backup_mode_b_export_success() {
        let temp = SyntheticTempDir::new("backup-mode-b-success");
        let state = backup_test_state(
            &temp,
            vec![
                synthetic_png_upload("synthetic.png"),
                synthetic_text_upload("synthetic.txt"),
            ],
        )
        .await;
        let destination = backup_destination(&temp, "mode-b-success");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("Mode B export should succeed");

        assert_eq!(package.manifest.mode, BackupManifestMode::FullLocalData);
        assert_eq!(package.manifest.files.len(), 2);
        assert!(package.path.join(BACKUP_BLOBS_DIR_NAME).is_dir());
        validate_backup_package(&package.path).expect("Mode B package should self-validate");
    }

    #[tokio::test]
    async fn backup_mode_b_tombstoned_blob_excluded() {
        let temp = SyntheticTempDir::new("backup-mode-b-tombstone");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let file_id = state.files.read().await[0].id;
        tombstone_synthetic_file(&state, file_id).await;
        let destination = backup_destination(&temp, "mode-b-tombstone");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("tombstoned blob should be excluded");

        assert!(package.manifest.files.is_empty());
        assert!(read_backup_state(&package.path).files[0]
            .deleted_at
            .is_some());
        assert!(
            backup_directory_entry_names(&package.path.join(BACKUP_BLOBS_DIR_NAME))
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn backup_mode_b_orphan_blob_excluded() {
        let temp = SyntheticTempDir::new("backup-mode-b-orphan");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        std_fs::write(
            state.blob_store.blobs_dir.join("orphan-synthetic-blob"),
            b"synthetic orphan bytes",
        )
        .expect("synthetic orphan should be created");
        let destination = backup_destination(&temp, "mode-b-orphan");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("orphan should not block export");

        assert_eq!(package.manifest.files.len(), 1);
        assert!(!package
            .path
            .join(BACKUP_BLOBS_DIR_NAME)
            .join("orphan-synthetic-blob")
            .exists());
    }

    #[tokio::test]
    async fn backup_mode_b_unreferenced_active_blob_included() {
        let temp = SyntheticTempDir::new("backup-mode-b-unreferenced");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "mode-b-unreferenced");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("active managed blob should be included");

        assert_eq!(package.manifest.files.len(), 1);
        let conversations = state.conversations.read().await;
        assert!(!is_managed_file_referenced(
            &conversations,
            package.manifest.files[0].file_id,
        ));
    }

    #[tokio::test]
    async fn backup_mode_b_missing_active_blob_fails() {
        let temp = SyntheticTempDir::new("backup-mode-b-missing");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        std_fs::remove_file(active_blob_path(&state, 0).await)
            .expect("synthetic active blob should be removed");
        let destination = backup_destination(&temp, "mode-b-missing");

        let result = export_backup_package(&state, &destination, BackupExportMode::ModeB).await;

        assert!(matches!(result, Err(BackupExportError::BlobMissing)));
        assert!(backup_directory_entry_names(&destination)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn backup_mode_b_blob_copy_failure_has_no_final_package() {
        let temp = SyntheticTempDir::new("backup-mode-b-copy-failure");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let source = active_blob_path(&state, 0).await;
        std_fs::remove_file(&source).unwrap();
        std_fs::create_dir(&source).unwrap();
        let destination = backup_destination(&temp, "mode-b-copy-failure");

        let result = export_backup_package(&state, &destination, BackupExportMode::ModeB).await;

        assert!(matches!(result, Err(BackupExportError::BlobReadFailed)));
        assert!(backup_directory_entry_names(&destination)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn backup_mode_b_invalid_storage_key_fails_closed() {
        let temp = SyntheticTempDir::new("backup-mode-b-invalid-key");
        let mut persisted = default_persisted_state();
        persisted.files.push(ManagedFileMetadata {
            id: 900,
            storage_key: "../synthetic-escape".to_string(),
            display_name: "synthetic.bin".to_string(),
            mime: "application/octet-stream".to_string(),
            size_bytes: 1,
            sha256: None,
            kind: "document".to_string(),
            relative_path: "files/blobs/../synthetic-escape".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            source: "upload".to_string(),
            deleted_at: None,
        });
        let state = transaction_test_state(&temp, persisted, None).await;
        let destination = backup_destination(&temp, "mode-b-invalid-key");

        let result = export_backup_package(&state, &destination, BackupExportMode::ModeB).await;

        assert!(matches!(result, Err(BackupExportError::SnapshotInvalid)));
        assert!(backup_directory_entry_names(&destination)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn backup_mode_b_symlink_or_escape_source_rejected() {
        let temp = SyntheticTempDir::new("backup-mode-b-symlink");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let source = active_blob_path(&state, 0).await;
        let outside = temp.path.join("synthetic-outside-file");
        std_fs::write(&outside, b"synthetic outside bytes").unwrap();
        std_fs::remove_file(&source).unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, &source).is_ok();
        #[cfg(not(windows))]
        let linked = std::os::unix::fs::symlink(&outside, &source).is_ok();
        if !linked {
            std_fs::create_dir(&source).unwrap();
        }
        let destination = backup_destination(&temp, "mode-b-symlink");

        let result = export_backup_package(&state, &destination, BackupExportMode::ModeB).await;

        assert!(matches!(result, Err(BackupExportError::BlobReadFailed)));
        assert!(backup_directory_entry_names(&destination)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn backup_manifest_mode_b_files_are_sorted_by_file_id() {
        let temp = SyntheticTempDir::new("backup-manifest-sorted");
        let state = backup_test_state(
            &temp,
            vec![
                synthetic_png_upload("synthetic-a.png"),
                synthetic_text_upload("synthetic-b.txt"),
            ],
        )
        .await;
        let destination = backup_destination(&temp, "manifest-sorted");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("Mode B export should succeed");

        assert!(package
            .manifest
            .files
            .windows(2)
            .all(|files| files[0].file_id < files[1].file_id));
    }

    #[tokio::test]
    async fn backup_mode_b_zero_active_files_has_empty_blob_directory() {
        let temp = SyntheticTempDir::new("backup-mode-b-empty");
        let state = backup_test_state(&temp, Vec::new()).await;
        let destination = backup_destination(&temp, "mode-b-empty");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("empty Mode B export should succeed");

        assert!(package.manifest.files.is_empty());
        assert!(package.path.join(BACKUP_BLOBS_DIR_NAME).is_dir());
    }

    #[tokio::test]
    async fn backup_checksum_metadata_hash_mismatch_fails_export() {
        let temp = SyntheticTempDir::new("backup-metadata-hash-mismatch");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        transact_persisted_state(&state, PureStateMutationScope::FileMetadata, |staged| {
            staged.files[0].sha256 = Some("0".repeat(64));
            Ok(())
        })
        .await
        .unwrap();
        let destination = backup_destination(&temp, "metadata-hash-mismatch");

        let result = export_backup_package(&state, &destination, BackupExportMode::ModeB).await;

        assert!(matches!(result, Err(BackupExportError::HashMismatch)));
        assert!(backup_directory_entry_names(&destination)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn backup_concurrency_category_a_export_is_complete_snapshot() {
        let temp = SyntheticTempDir::new("backup-category-a-concurrency");
        let state = backup_test_state(&temp, Vec::new()).await;
        let destination = backup_destination(&temp, "category-a-concurrency");
        let export_state = state.clone();
        let export_destination = destination.clone();
        let export = tokio::spawn(async move {
            export_backup_package(&export_state, &export_destination, BackupExportMode::ModeA).await
        });
        let response = update_conversation_title(
            State(state.clone()),
            Path(MOCK_WELCOME_CONVERSATION_ID.to_string()),
            Json(UpdateConversationTitleRequest {
                title: "synthetic concurrent backup title".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let package = export.await.unwrap().unwrap();
        let backup_state = read_backup_state(&package.path);
        let conversation = &backup_state.conversations[MOCK_WELCOME_CONVERSATION_ID];

        assert!(matches!(
            conversation.title.as_str(),
            "RikkaDesk Mock Welcome" | "synthetic concurrent backup title"
        ));
        assert_eq!(conversation.messages.len(), 1);
    }

    #[tokio::test]
    async fn backup_concurrency_mode_b_lock_blocks_file_delete() {
        let temp = SyntheticTempDir::new("backup-block-delete");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let file_id = state.files.read().await[0].id;
        let file_guard = state.file_blob_transaction_mutex.lock().await;
        let delete_state = state.clone();
        let delete = tokio::spawn(async move {
            delete_file(State(delete_state), Path(file_id))
                .await
                .into_response()
                .status()
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!delete.is_finished());
        drop(file_guard);

        assert_eq!(delete.await.unwrap(), StatusCode::OK);
    }

    #[tokio::test]
    async fn backup_concurrency_mode_b_lock_blocks_upload_publish() {
        let temp = SyntheticTempDir::new("backup-block-upload");
        let state = backup_test_state(&temp, Vec::new()).await;
        let file_guard = state.file_blob_transaction_mutex.lock().await;
        let upload_state = state.clone();
        let upload = tokio::spawn(async move {
            commit_file_upload_batch(
                &upload_state,
                vec![synthetic_png_upload("synthetic-new.png")],
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!upload.is_finished());
        drop(file_guard);

        assert_eq!(upload.await.unwrap().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn backup_concurrency_provider_transaction_is_old_or_new_snapshot() {
        let temp = SyntheticTempDir::new("backup-provider-concurrency");
        let state = transaction_test_state_with_secret_store(
            &temp,
            synthetic_provider_state(),
            None,
            Arc::new(PanicOnAccessSecretStore),
        )
        .await;
        let snapshot = portable_backup_snapshot(&state).await.unwrap();
        transact_persisted_state(&state, PureStateMutationScope::ProviderMetadata, |staged| {
            staged.providers[0].name = "Synthetic Provider After Snapshot".to_string();
            sync_settings_with_desktop_providers(&mut staged.settings, &staged.providers);
            Ok(())
        })
        .await
        .unwrap();
        let destination = backup_destination(&temp, "provider-concurrency");
        let package = build_and_publish_backup_package(
            &destination,
            BackupExportMode::ModeA,
            snapshot,
            None,
            1,
        )
        .unwrap();

        assert_eq!(
            read_backup_state(&package.path).providers[0].name,
            "Synthetic Provider"
        );
        assert_eq!(
            state.providers.read().await[0].name,
            "Synthetic Provider After Snapshot"
        );
    }

    #[tokio::test]
    async fn backup_concurrency_active_generation_is_excluded() {
        let temp = SyntheticTempDir::new("backup-active-generation");
        let state = backup_test_state(&temp, Vec::new()).await;
        let id = "backup-active-generation";
        let (handle, plan) =
            start_synthetic_generation(&state, id, GenerationOperation::Send).await;
        broadcast_generation_delta(
            &state,
            id,
            &handle,
            &plan,
            MOCK_MODEL_ID,
            "synthetic transient backup delta",
            "synthetic transient backup delta",
        )
        .await;
        let destination = backup_destination(&temp, "active-generation");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("active generation should not block durable backup");
        let backup_state = read_backup_state(&package.path);
        let backup_conversation = &backup_state.conversations[id];

        assert!(!backup_conversation.is_generating);
        assert_eq!(backup_conversation.messages.len(), 1);
        assert!(!serde_json::to_string(backup_conversation)
            .unwrap()
            .contains("synthetic transient backup delta"));
        assert!(remove_generation_if_current(&state, id, &handle).await);
    }

    #[tokio::test]
    async fn backup_concurrency_exports_serialize_and_use_unique_names() {
        let temp = SyntheticTempDir::new("backup-concurrent-exports");
        let state = backup_test_state(&temp, Vec::new()).await;
        let destination = backup_destination(&temp, "concurrent-exports");
        let first_state = state.clone();
        let first_destination = destination.clone();
        let first = tokio::spawn(async move {
            export_backup_package(&first_state, &first_destination, BackupExportMode::ModeA).await
        });
        let second_state = state.clone();
        let second_destination = destination.clone();
        let second = tokio::spawn(async move {
            export_backup_package(&second_state, &second_destination, BackupExportMode::ModeA).await
        });
        let first_package = first.await.unwrap().unwrap();
        let second_package = second.await.unwrap().unwrap();

        assert_ne!(first_package.path, second_package.path);
        validate_backup_package(&first_package.path).unwrap();
        validate_backup_package(&second_package.path).unwrap();
    }

    #[tokio::test]
    async fn backup_export_package_write_failure_does_not_publish() {
        let temp = SyntheticTempDir::new("backup-write-failure");
        let state = backup_test_state(&temp, Vec::new()).await;
        let snapshot = portable_backup_snapshot(&state).await.unwrap();
        let package = temp.path.join("synthetic-write-failure-package");
        std_fs::create_dir(&package).unwrap();
        std_fs::write(package.join(BACKUP_STATE_FILE_NAME), b"occupied").unwrap();

        let result =
            write_backup_package_contents(&package, BackupExportMode::ModeA, &snapshot, None);

        assert!(matches!(result, Err(BackupExportError::WriteFailed)));
        assert!(!package.join(BACKUP_CHECKSUM_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn backup_export_manifest_write_failure_does_not_validate() {
        let temp = SyntheticTempDir::new("backup-manifest-write-failure");
        let state = backup_test_state(&temp, Vec::new()).await;
        let snapshot = portable_backup_snapshot(&state).await.unwrap();
        let package = temp.path.join("synthetic-manifest-write-failure-package");
        std_fs::create_dir(&package).unwrap();
        std_fs::write(package.join(BACKUP_MANIFEST_FILE_NAME), b"occupied").unwrap();

        let result =
            write_backup_package_contents(&package, BackupExportMode::ModeA, &snapshot, None);

        assert!(matches!(result, Err(BackupExportError::WriteFailed)));
        assert!(!package.join(BACKUP_CHECKSUM_FILE_NAME).exists());
    }

    #[test]
    fn backup_export_publish_failure_preserves_existing_directory() {
        let temp = SyntheticTempDir::new("backup-publish-failure");
        let unpublished = temp.path.join("synthetic-unpublished");
        let existing = temp.path.join("synthetic-existing");
        std_fs::create_dir(&unpublished).unwrap();
        std_fs::create_dir(&existing).unwrap();
        std_fs::write(existing.join("marker"), b"synthetic existing backup").unwrap();

        let result = publish_backup_directory(&unpublished, &existing);

        assert!(matches!(result, Err(BackupExportError::PublishFailed)));
        assert_eq!(
            std_fs::read(existing.join("marker")).unwrap(),
            b"synthetic existing backup"
        );
        assert!(unpublished.exists());
    }

    #[tokio::test]
    async fn backup_export_name_collision_uses_new_unique_directory() {
        let temp = SyntheticTempDir::new("backup-name-collision");
        let state = backup_test_state(&temp, Vec::new()).await;
        let destination = backup_destination(&temp, "name-collision");
        let collision =
            destination.join(format!("{BACKUP_TEMP_DIR_PREFIX}.{}.1", std::process::id()));
        std_fs::create_dir(&collision).unwrap();
        std_fs::write(collision.join("marker"), b"synthetic unrelated temp").unwrap();

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("collision should use another unique name");

        assert!(package.path.is_dir());
        assert!(collision.join("marker").is_file());
        assert_ne!(package.path, collision);
    }

    #[tokio::test]
    async fn backup_export_failure_cleans_only_current_temp_directory() {
        let temp = SyntheticTempDir::new("backup-temp-cleanup");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        std_fs::remove_file(active_blob_path(&state, 0).await).unwrap();
        let destination = backup_destination(&temp, "temp-cleanup");
        let unrelated = destination.join("unrelated-backup");
        std_fs::create_dir(&unrelated).unwrap();
        std_fs::write(unrelated.join("marker"), b"synthetic unrelated backup").unwrap();

        let result = export_backup_package(&state, &destination, BackupExportMode::ModeB).await;

        assert!(matches!(result, Err(BackupExportError::BlobMissing)));
        assert!(unrelated.join("marker").is_file());
        assert!(backup_directory_entry_names(&destination)
            .unwrap()
            .iter()
            .all(|name| !name.starts_with(BACKUP_TEMP_DIR_PREFIX)));
    }

    #[tokio::test]
    async fn backup_checksum_file_matches_all_entries() {
        let temp = SyntheticTempDir::new("backup-checksum-entries");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "checksum-entries");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .unwrap();
        let checksum_text =
            std_fs::read_to_string(package.path.join(BACKUP_CHECKSUM_FILE_NAME)).unwrap();
        let checksums = parse_backup_checksums(&checksum_text).unwrap();

        assert_eq!(checksums.len(), 3);
        for (path, expected_hash) in checksums {
            assert_eq!(
                sha256_file(&package.path.join(path)).unwrap().0,
                expected_hash
            );
        }
    }

    #[tokio::test]
    async fn backup_manifest_hashes_match_package_copies() {
        let temp = SyntheticTempDir::new("backup-manifest-hashes");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "manifest-hashes");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .unwrap();
        let manifest = read_backup_manifest(&package.path);

        assert_eq!(
            sha256_file(&package.path.join(BACKUP_STATE_FILE_NAME))
                .unwrap()
                .0,
            manifest.state.sha256
        );
        for file in manifest.files {
            let (hash, size) = sha256_file(&package.path.join(file.package_path)).unwrap();
            assert_eq!(hash, file.sha256);
            assert_eq!(size, file.size_bytes);
        }
    }

    #[tokio::test]
    async fn backup_checksum_tampered_temp_package_rejected_before_publish() {
        let temp = SyntheticTempDir::new("backup-tampered-temp");
        let state = backup_test_state(&temp, Vec::new()).await;
        let snapshot = portable_backup_snapshot(&state).await.unwrap();
        let package = create_unpublished_backup_package(
            &temp,
            "tampered-temp",
            BackupExportMode::ModeA,
            &snapshot,
            None,
        );
        std_fs::write(
            package.join(BACKUP_STATE_FILE_NAME),
            b"synthetic tampered state",
        )
        .unwrap();

        assert!(matches!(
            validate_backup_package(&package),
            Err(BackupExportError::HashMismatch)
        ));
    }

    #[tokio::test]
    async fn backup_manifest_and_sums_contain_only_relative_paths() {
        let temp = SyntheticTempDir::new("backup-relative-paths");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "relative-paths");
        let package = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .unwrap();
        let manifest = read_backup_manifest(&package.path);
        let sums = std_fs::read_to_string(package.path.join(BACKUP_CHECKSUM_FILE_NAME)).unwrap();

        assert!(is_safe_backup_relative_path(&manifest.state.path));
        assert!(manifest
            .files
            .iter()
            .all(|file| is_safe_backup_relative_path(&file.package_path)));
        assert!(!sums.contains(":\\"));
        assert!(!sums.contains("\\\\"));
        assert!(!sums.contains("../"));
    }

    #[tokio::test]
    async fn backup_manifest_unknown_blob_entry_is_rejected() {
        let temp = SyntheticTempDir::new("backup-unknown-blob");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let snapshot = portable_backup_snapshot(&state).await.unwrap();
        let package = create_unpublished_backup_package(
            &temp,
            "unknown-blob",
            BackupExportMode::ModeB,
            &snapshot,
            Some(&state.blob_store.blobs_dir),
        );
        std_fs::write(
            package.join(BACKUP_BLOBS_DIR_NAME).join("unknown.blob"),
            b"synthetic unknown blob",
        )
        .unwrap();

        assert!(matches!(
            validate_backup_package(&package),
            Err(BackupExportError::PackageValidationFailed)
        ));
    }

    #[tokio::test]
    async fn backup_export_package_contains_no_secrets_directory() {
        let temp = SyntheticTempDir::new("backup-no-secrets-directory");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "no-secrets-directory");
        let mode_a = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .unwrap();
        let mode_b = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .unwrap();

        assert!(!mode_a.path.join(SECRETS_DIR_NAME).exists());
        assert!(!mode_b.path.join(SECRETS_DIR_NAME).exists());
        assert!(!read_backup_manifest(&mode_b.path).secrets_included);
    }

    #[tokio::test]
    async fn backup_export_secret_store_access_count_is_zero() {
        let temp = SyntheticTempDir::new("backup-secret-store-zero");
        let state = backup_test_state(&temp, vec![synthetic_png_upload("synthetic.png")]).await;
        let destination = backup_destination(&temp, "secret-store-zero");

        export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .expect("Mode A must not access panic SecretStore");
        export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect("Mode B must not access panic SecretStore");
    }

    #[tokio::test]
    async fn backup_export_safe_errors_hide_paths_and_user_metadata() {
        let temp = SyntheticTempDir::new("backup-safe-errors");
        let file_name = "synthetic-private-name.png";
        let state = backup_test_state(&temp, vec![synthetic_png_upload(file_name)]).await;
        let storage_key = state.files.read().await[0].storage_key.clone();
        std_fs::remove_file(active_blob_path(&state, 0).await).unwrap();
        let destination = backup_destination(&temp, "safe-errors");

        let error = export_backup_package(&state, &destination, BackupExportMode::ModeB)
            .await
            .expect_err("missing active blob should fail");
        let message = error.to_string();

        assert!(!message.contains(file_name));
        assert!(!message.contains(&storage_key));
        assert!(!message.contains(temp.path.to_string_lossy().as_ref()));
        assert!(!message.contains(PROVIDER_SECRET_REF_PREFIX));
    }

    #[tokio::test]
    async fn backup_export_uses_synthetic_destination_only() {
        let temp = SyntheticTempDir::new("backup-synthetic-root");
        let state = backup_test_state(&temp, Vec::new()).await;
        let destination = backup_destination(&temp, "synthetic-root");

        let package = export_backup_package(&state, &destination, BackupExportMode::ModeA)
            .await
            .unwrap();

        let canonical_package = std_fs::canonicalize(&package.path).unwrap();
        let canonical_temp = std_fs::canonicalize(&temp.path).unwrap();
        let canonical_destination = std_fs::canonicalize(&destination).unwrap();
        assert!(canonical_package.starts_with(canonical_temp));
        assert!(canonical_package.starts_with(canonical_destination));
    }
}
