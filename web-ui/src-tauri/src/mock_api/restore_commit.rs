#![allow(dead_code)]

use super::*;
use std::sync::{Mutex as StdMutex, MutexGuard as StdMutexGuard};

pub(super) const RESTORE_JOURNAL_FILE_NAME: &str = "restore-journal.json";
pub(super) const RESTORE_JOURNAL_TEMP_PREFIX: &str = "restore-journal.json.tmp";
pub(super) const RESTORE_JOURNAL_FORMAT: &str = "rikkadesk-restore-journal";
pub(super) const RESTORE_JOURNAL_VERSION: u32 = 1;
pub(super) const RESTORE_ROLLBACK_DIR_PREFIX: &str = "mock-api.pre-restore";
pub(super) const RESTORE_FAILED_DIR_PREFIX: &str = "mock-api.failed-restore";
pub(super) const RESTORE_OPERATION_ID_MAX_LEN: usize = 80;

pub(super) static RESTORE_COMMIT_MUTEX: StdMutex<()> = StdMutex::new(());
static RESTORE_JOURNAL_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct RestoreCommitRequest {
    app_data_parent: PathBuf,
    operation_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreCommitResult {
    PendingStartupValidation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestoreCommitError {
    OfflineGateFailed,
    Conflict,
    JournalExists,
    JournalWriteFailed,
    StageInvalid,
    CurrentInvalid,
    SnapshotFailed,
    PublishFailed,
    ValidationFailed,
    RollbackFailed,
    JournalAmbiguous,
    ManualRecoveryRequired,
}

impl fmt::Display for RestoreCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::OfflineGateFailed => "restore commit offline gate failed",
            Self::Conflict => "restore commit conflict detected",
            Self::JournalExists => "restore journal already exists",
            Self::JournalWriteFailed => "restore journal update failed",
            Self::StageInvalid => "restore candidate staging validation failed",
            Self::CurrentInvalid => "restore current data validation failed",
            Self::SnapshotFailed => "restore rollback snapshot failed",
            Self::PublishFailed => "restore candidate publish failed",
            Self::ValidationFailed => "restore committed candidate validation failed",
            Self::RollbackFailed => "restore handled rollback failed",
            Self::JournalAmbiguous => "restore journal state is ambiguous",
            Self::ManualRecoveryRequired => "restore requires manual recovery",
        };
        formatter.write_str(message)
    }
}

impl Error for RestoreCommitError {}

struct OfflineRestorePermit(());

trait RestoreOfflineGate: Send + Sync {
    fn verify_offline(
        &self,
        app_data_parent: &FilePath,
    ) -> Result<OfflineRestorePermit, RestoreCommitError>;
}

struct TrustedOfflineRestoreGate {
    _private: (),
}

impl TrustedOfflineRestoreGate {
    fn after_trusted_shutdown() -> Self {
        Self { _private: () }
    }
}

impl RestoreOfflineGate for TrustedOfflineRestoreGate {
    fn verify_offline(
        &self,
        _app_data_parent: &FilePath,
    ) -> Result<OfflineRestorePermit, RestoreCommitError> {
        Ok(OfflineRestorePermit(()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RestoreJournalPhase {
    Staging,
    BackupCurrent,
    CommitOldMoved,
    CommitNewMoved,
    StartupValidation,
    Completed,
    RollbackRequired,
    RollbackCompleted,
    RollbackFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RestoreJournalSafeErrorCode {
    CommitFailed,
    RollbackFailed,
    JournalAmbiguous,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RestoreJournal {
    pub(super) format: String,
    pub(super) version: u32,
    pub(super) operation_id: String,
    pub(super) phase: RestoreJournalPhase,
    pub(super) mode: BackupManifestMode,
    pub(super) created_at: String,
    pub(super) safe_error_code: Option<RestoreJournalSafeErrorCode>,
}

impl RestoreJournal {
    fn new(operation_id: &str, mode: BackupManifestMode) -> Self {
        Self {
            format: RESTORE_JOURNAL_FORMAT.to_string(),
            version: RESTORE_JOURNAL_VERSION,
            operation_id: operation_id.to_string(),
            phase: RestoreJournalPhase::Staging,
            mode,
            created_at: Utc::now().to_rfc3339(),
            safe_error_code: None,
        }
    }

    fn set_phase(
        &mut self,
        phase: RestoreJournalPhase,
        safe_error_code: Option<RestoreJournalSafeErrorCode>,
    ) {
        self.phase = phase;
        self.safe_error_code = safe_error_code;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreJournalIoStep {
    Create,
    Write,
    Flush,
    Sync,
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreRenameKind {
    CurrentToRollback,
    StageToCurrent,
    NewCurrentToFailed,
    RollbackToCurrent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreParentSyncKind {
    Journal,
    OldMoved,
    NewMoved,
    NewCurrentMovedToFailed,
    RollbackRestored,
}

trait RestoreCommitIo: Send + Sync {
    fn create_new(&self, path: &FilePath, phase: RestoreJournalPhase) -> io::Result<std_fs::File>;
    fn write_all(
        &self,
        file: &mut std_fs::File,
        data: &[u8],
        phase: RestoreJournalPhase,
    ) -> io::Result<()>;
    fn flush(&self, file: &mut std_fs::File, phase: RestoreJournalPhase) -> io::Result<()>;
    fn sync_file(&self, file: &std_fs::File, phase: RestoreJournalPhase) -> io::Result<()>;
    fn replace_journal(
        &self,
        replacement: &FilePath,
        target: &FilePath,
        phase: RestoreJournalPhase,
    ) -> io::Result<()>;
    fn remove_file(&self, path: &FilePath);
    fn rename_directory(
        &self,
        source: &FilePath,
        destination: &FilePath,
        kind: RestoreRenameKind,
    ) -> io::Result<()>;
    fn sync_parent(&self, parent: &FilePath, kind: RestoreParentSyncKind) -> io::Result<()>;
}

struct RealRestoreCommitIo;

impl RestoreCommitIo for RealRestoreCommitIo {
    fn create_new(&self, path: &FilePath, _phase: RestoreJournalPhase) -> io::Result<std_fs::File> {
        std_fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
    }

    fn write_all(
        &self,
        file: &mut std_fs::File,
        data: &[u8],
        _phase: RestoreJournalPhase,
    ) -> io::Result<()> {
        file.write_all(data)
    }

    fn flush(&self, file: &mut std_fs::File, _phase: RestoreJournalPhase) -> io::Result<()> {
        file.flush()
    }

    fn sync_file(&self, file: &std_fs::File, _phase: RestoreJournalPhase) -> io::Result<()> {
        file.sync_all()
    }

    fn replace_journal(
        &self,
        replacement: &FilePath,
        target: &FilePath,
        _phase: RestoreJournalPhase,
    ) -> io::Result<()> {
        replace_state_file(replacement, target)
    }

    fn remove_file(&self, path: &FilePath) {
        let _ = std_fs::remove_file(path);
    }

    fn rename_directory(
        &self,
        source: &FilePath,
        destination: &FilePath,
        _kind: RestoreRenameKind,
    ) -> io::Result<()> {
        std_fs::rename(source, destination)
    }

    fn sync_parent(&self, parent: &FilePath, _kind: RestoreParentSyncKind) -> io::Result<()> {
        sync_restore_parent_directory(parent)
    }
}

struct RestoreJournalStore<'a> {
    parent: &'a FilePath,
    journal_path: PathBuf,
    io: &'a dyn RestoreCommitIo,
}

impl<'a> RestoreJournalStore<'a> {
    fn new(parent: &'a FilePath, io: &'a dyn RestoreCommitIo) -> Self {
        Self {
            parent,
            journal_path: parent.join(RESTORE_JOURNAL_FILE_NAME),
            io,
        }
    }

    fn create(&self, journal: &RestoreJournal) -> Result<(), RestoreCommitError> {
        let data = serialize_restore_journal(journal)?;
        let phase = journal.phase;
        let mut file = match self.io.create_new(&self.journal_path, phase) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(RestoreCommitError::JournalExists)
            }
            Err(_) => return Err(RestoreCommitError::JournalWriteFailed),
        };
        let result = self
            .io
            .write_all(&mut file, &data, phase)
            .and_then(|_| self.io.flush(&mut file, phase))
            .and_then(|_| self.io.sync_file(&file, phase));
        drop(file);
        if result.is_err() {
            self.io.remove_file(&self.journal_path);
            return Err(RestoreCommitError::JournalWriteFailed);
        }
        self.io
            .sync_parent(self.parent, RestoreParentSyncKind::Journal)
            .map_err(|_| RestoreCommitError::JournalWriteFailed)
    }

    fn update(&self, journal: &RestoreJournal) -> Result<(), RestoreCommitError> {
        let data = serialize_restore_journal(journal)?;
        let phase = journal.phase;
        let sequence = RESTORE_JOURNAL_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_path = self.parent.join(format!(
            "{RESTORE_JOURNAL_TEMP_PREFIX}.{}.{}",
            std::process::id(),
            sequence
        ));
        let mut file = self
            .io
            .create_new(&temp_path, phase)
            .map_err(|_| RestoreCommitError::JournalWriteFailed)?;
        let write_result = self
            .io
            .write_all(&mut file, &data, phase)
            .and_then(|_| self.io.flush(&mut file, phase))
            .and_then(|_| self.io.sync_file(&file, phase));
        drop(file);
        if write_result.is_err() {
            self.io.remove_file(&temp_path);
            return Err(RestoreCommitError::JournalWriteFailed);
        }
        if self
            .io
            .replace_journal(&temp_path, &self.journal_path, phase)
            .is_err()
        {
            self.io.remove_file(&temp_path);
            return Err(RestoreCommitError::JournalWriteFailed);
        }
        self.io
            .sync_parent(self.parent, RestoreParentSyncKind::Journal)
            .map_err(|_| RestoreCommitError::JournalWriteFailed)
    }
}

#[derive(Clone)]
pub(super) struct RestoreCommitPaths {
    pub(super) parent: PathBuf,
    pub(super) current: PathBuf,
    pub(super) stage: PathBuf,
    pub(super) rollback: PathBuf,
    pub(super) failed: PathBuf,
    pub(super) journal: PathBuf,
    pub(super) temp_stage: PathBuf,
}

fn commit_restore_offline(
    request: RestoreCommitRequest,
    offline_gate: &dyn RestoreOfflineGate,
) -> Result<RestoreCommitResult, RestoreCommitError> {
    commit_restore_offline_with_io(request, offline_gate, &RealRestoreCommitIo)
}

fn commit_restore_offline_with_io(
    request: RestoreCommitRequest,
    offline_gate: &dyn RestoreOfflineGate,
    io: &dyn RestoreCommitIo,
) -> Result<RestoreCommitResult, RestoreCommitError> {
    let _commit_guard = acquire_restore_commit_mutex()?;
    let _permit = offline_gate.verify_offline(&request.app_data_parent)?;
    let parent = validate_restore_commit_parent(&request.app_data_parent)?;
    let paths = derive_restore_commit_paths(parent, &request.operation_id)?;
    validate_restore_commit_conflicts(&paths)?;
    let mode = validate_restore_commit_stage(&paths.stage)?;
    validate_existing_mock_api_before_restore(&paths.current)?;
    ensure_path_absent(&paths.rollback, RestoreCommitError::Conflict)?;
    ensure_path_absent(&paths.failed, RestoreCommitError::Conflict)?;

    let store = RestoreJournalStore::new(&paths.parent, io);
    let mut journal = RestoreJournal::new(&request.operation_id, mode);
    store.create(&journal)?;

    let _second_permit = offline_gate.verify_offline(&paths.parent)?;
    journal.set_phase(RestoreJournalPhase::BackupCurrent, None);
    store.update(&journal)?;

    if io
        .rename_directory(
            &paths.current,
            &paths.rollback,
            RestoreRenameKind::CurrentToRollback,
        )
        .is_err()
    {
        return Err(RestoreCommitError::SnapshotFailed);
    }
    if io
        .sync_parent(&paths.parent, RestoreParentSyncKind::OldMoved)
        .is_err()
        || verify_old_moved(&paths).is_err()
    {
        return rollback_after_old_moved(
            &paths,
            &mut journal,
            &store,
            io,
            RestoreCommitError::SnapshotFailed,
        );
    }

    journal.set_phase(RestoreJournalPhase::CommitOldMoved, None);
    if store.update(&journal).is_err() {
        return rollback_after_old_moved(
            &paths,
            &mut journal,
            &store,
            io,
            RestoreCommitError::JournalWriteFailed,
        );
    }

    if io
        .rename_directory(
            &paths.stage,
            &paths.current,
            RestoreRenameKind::StageToCurrent,
        )
        .is_err()
    {
        return rollback_after_old_moved(
            &paths,
            &mut journal,
            &store,
            io,
            RestoreCommitError::PublishFailed,
        );
    }
    if io
        .sync_parent(&paths.parent, RestoreParentSyncKind::NewMoved)
        .is_err()
        || verify_new_moved(&paths).is_err()
    {
        return rollback_after_new_moved(
            &paths,
            &mut journal,
            &store,
            io,
            RestoreCommitError::PublishFailed,
        );
    }

    journal.set_phase(RestoreJournalPhase::CommitNewMoved, None);
    if store.update(&journal).is_err() {
        return rollback_after_new_moved(
            &paths,
            &mut journal,
            &store,
            io,
            RestoreCommitError::JournalWriteFailed,
        );
    }

    if validate_staged_mock_api_directory(&paths.current, mode).is_err() {
        return rollback_after_new_moved(
            &paths,
            &mut journal,
            &store,
            io,
            RestoreCommitError::ValidationFailed,
        );
    }

    Ok(RestoreCommitResult::PendingStartupValidation)
}

fn acquire_restore_commit_mutex() -> Result<StdMutexGuard<'static, ()>, RestoreCommitError> {
    RESTORE_COMMIT_MUTEX
        .lock()
        .map_err(|_| RestoreCommitError::Conflict)
}

pub(super) fn validate_restore_operation_id(operation_id: &str) -> Result<(), RestoreCommitError> {
    if operation_id.is_empty()
        || operation_id.len() > RESTORE_OPERATION_ID_MAX_LEN
        || !operation_id.is_ascii()
    {
        return Err(RestoreCommitError::Conflict);
    }
    let components = operation_id.split('-').collect::<Vec<_>>();
    if components.len() != 3
        || components.iter().any(|component| {
            component.is_empty()
                || component.len() > 20
                || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(RestoreCommitError::Conflict);
    }
    Ok(())
}

pub(super) fn validate_restore_commit_parent(
    parent: &FilePath,
) -> Result<PathBuf, RestoreCommitError> {
    let metadata = std_fs::symlink_metadata(parent).map_err(|_| RestoreCommitError::Conflict)?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(RestoreCommitError::Conflict);
    }
    let canonical = std_fs::canonicalize(parent).map_err(|_| RestoreCommitError::Conflict)?;
    let canonical_metadata =
        std_fs::symlink_metadata(&canonical).map_err(|_| RestoreCommitError::Conflict)?;
    if metadata_is_link_or_reparse(&canonical_metadata) || !canonical_metadata.is_dir() {
        return Err(RestoreCommitError::Conflict);
    }
    Ok(canonical)
}

pub(super) fn derive_restore_commit_paths(
    parent: PathBuf,
    operation_id: &str,
) -> Result<RestoreCommitPaths, RestoreCommitError> {
    validate_restore_operation_id(operation_id)?;
    Ok(RestoreCommitPaths {
        current: parent.join(PERSIST_DIR_NAME),
        stage: parent.join(format!("{RESTORE_STAGE_FINAL_DIR_PREFIX}.{operation_id}")),
        rollback: parent.join(format!("{RESTORE_ROLLBACK_DIR_PREFIX}.{operation_id}")),
        failed: parent.join(format!("{RESTORE_FAILED_DIR_PREFIX}.{operation_id}")),
        journal: parent.join(RESTORE_JOURNAL_FILE_NAME),
        temp_stage: parent.join(format!("{RESTORE_STAGE_TEMP_DIR_PREFIX}.{operation_id}")),
        parent,
    })
}

fn validate_restore_commit_conflicts(paths: &RestoreCommitPaths) -> Result<(), RestoreCommitError> {
    if restore_commit_path_exists(&paths.journal)? {
        return Err(RestoreCommitError::JournalExists);
    }
    let expected = [
        paths.current.file_name(),
        paths.stage.file_name(),
        paths.rollback.file_name(),
        paths.failed.file_name(),
        paths.journal.file_name(),
        paths.temp_stage.file_name(),
    ]
    .into_iter()
    .flatten()
    .map(|name| name.to_string_lossy().into_owned())
    .collect::<Vec<_>>();

    for entry in std_fs::read_dir(&paths.parent).map_err(|_| RestoreCommitError::Conflict)? {
        let entry = entry.map_err(|_| RestoreCommitError::Conflict)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| RestoreCommitError::Conflict)?;
        if expected
            .iter()
            .any(|expected_name| name.eq_ignore_ascii_case(expected_name) && name != *expected_name)
            || name.starts_with(&format!("{RESTORE_ROLLBACK_DIR_PREFIX}."))
            || name.starts_with(&format!("{RESTORE_FAILED_DIR_PREFIX}."))
            || name.starts_with(&format!("{RESTORE_JOURNAL_TEMP_PREFIX}."))
        {
            return Err(RestoreCommitError::Conflict);
        }
    }
    if restore_commit_path_exists(&paths.temp_stage)?
        || restore_commit_path_exists(&paths.rollback)?
        || restore_commit_path_exists(&paths.failed)?
    {
        return Err(RestoreCommitError::Conflict);
    }
    Ok(())
}

pub(super) fn validate_restore_commit_stage(
    stage: &FilePath,
) -> Result<BackupManifestMode, RestoreCommitError> {
    if validate_staged_mock_api_directory(stage, BackupManifestMode::StateOnly).is_ok() {
        return Ok(BackupManifestMode::StateOnly);
    }
    if validate_staged_mock_api_directory(stage, BackupManifestMode::FullLocalData).is_ok() {
        return Ok(BackupManifestMode::FullLocalData);
    }
    Err(RestoreCommitError::StageInvalid)
}

pub(super) fn validate_existing_mock_api_before_restore(
    current: &FilePath,
) -> Result<PersistedMockState, RestoreCommitError> {
    let metadata =
        std_fs::symlink_metadata(current).map_err(|_| RestoreCommitError::CurrentInvalid)?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(RestoreCommitError::CurrentInvalid);
    }
    let canonical =
        std_fs::canonicalize(current).map_err(|_| RestoreCommitError::CurrentInvalid)?;
    let state_bytes = read_bounded_restore_file(
        &canonical,
        STATE_FILE_NAME,
        RESTORE_STATE_MAX_BYTES,
        RestoreValidationError::StateInvalid,
    )
    .map_err(|_| RestoreCommitError::CurrentInvalid)?;
    let state = parse_and_validate_restore_state(&state_bytes, STATE_SCHEMA_VERSION)
        .map_err(|_| RestoreCommitError::CurrentInvalid)?;

    for entry in std_fs::read_dir(&canonical).map_err(|_| RestoreCommitError::CurrentInvalid)? {
        let entry = entry.map_err(|_| RestoreCommitError::CurrentInvalid)?;
        let entry_metadata = std_fs::symlink_metadata(entry.path())
            .map_err(|_| RestoreCommitError::CurrentInvalid)?;
        if metadata_is_link_or_reparse(&entry_metadata) {
            return Err(RestoreCommitError::CurrentInvalid);
        }
    }
    validate_optional_current_directory(&canonical.join(SECRETS_DIR_NAME))?;
    let files_root = canonical.join(FILES_DIR_NAME);
    if restore_commit_path_exists(&files_root)? {
        validate_optional_current_directory(&files_root)?;
        validate_optional_current_directory(&files_root.join(FILE_BLOBS_DIR_NAME))?;
    }
    Ok(state)
}

fn validate_optional_current_directory(path: &FilePath) -> Result<(), RestoreCommitError> {
    match std_fs::symlink_metadata(path) {
        Ok(metadata) if !metadata_is_link_or_reparse(&metadata) && metadata.is_dir() => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(RestoreCommitError::CurrentInvalid),
    }
}

fn rollback_after_old_moved(
    paths: &RestoreCommitPaths,
    journal: &mut RestoreJournal,
    store: &RestoreJournalStore<'_>,
    io: &dyn RestoreCommitIo,
    original_error: RestoreCommitError,
) -> Result<RestoreCommitResult, RestoreCommitError> {
    let mut journal_ambiguous = false;
    journal.set_phase(
        RestoreJournalPhase::RollbackRequired,
        Some(RestoreJournalSafeErrorCode::CommitFailed),
    );
    if store.update(journal).is_err() {
        journal_ambiguous = true;
    }
    if restore_commit_path_exists(&paths.current).unwrap_or(true)
        || io
            .rename_directory(
                &paths.rollback,
                &paths.current,
                RestoreRenameKind::RollbackToCurrent,
            )
            .is_err()
        || io
            .sync_parent(&paths.parent, RestoreParentSyncKind::RollbackRestored)
            .is_err()
        || validate_existing_mock_api_before_restore(&paths.current).is_err()
    {
        mark_rollback_failed(journal, store);
        return Err(RestoreCommitError::ManualRecoveryRequired);
    }
    journal.set_phase(
        RestoreJournalPhase::RollbackCompleted,
        Some(RestoreJournalSafeErrorCode::CommitFailed),
    );
    if store.update(journal).is_err() {
        journal_ambiguous = true;
    }
    if journal_ambiguous {
        Err(RestoreCommitError::JournalAmbiguous)
    } else {
        Err(original_error)
    }
}

fn rollback_after_new_moved(
    paths: &RestoreCommitPaths,
    journal: &mut RestoreJournal,
    store: &RestoreJournalStore<'_>,
    io: &dyn RestoreCommitIo,
    original_error: RestoreCommitError,
) -> Result<RestoreCommitResult, RestoreCommitError> {
    let mut journal_ambiguous = false;
    journal.set_phase(
        RestoreJournalPhase::RollbackRequired,
        Some(RestoreJournalSafeErrorCode::CommitFailed),
    );
    if store.update(journal).is_err() {
        journal_ambiguous = true;
    }
    if restore_commit_path_exists(&paths.failed).unwrap_or(true)
        || io
            .rename_directory(
                &paths.current,
                &paths.failed,
                RestoreRenameKind::NewCurrentToFailed,
            )
            .is_err()
    {
        mark_rollback_failed(journal, store);
        return Err(RestoreCommitError::ManualRecoveryRequired);
    }
    if io
        .sync_parent(
            &paths.parent,
            RestoreParentSyncKind::NewCurrentMovedToFailed,
        )
        .is_err()
    {
        journal_ambiguous = true;
    }
    if io
        .rename_directory(
            &paths.rollback,
            &paths.current,
            RestoreRenameKind::RollbackToCurrent,
        )
        .is_err()
        || io
            .sync_parent(&paths.parent, RestoreParentSyncKind::RollbackRestored)
            .is_err()
        || validate_existing_mock_api_before_restore(&paths.current).is_err()
    {
        mark_rollback_failed(journal, store);
        return Err(RestoreCommitError::ManualRecoveryRequired);
    }
    journal.set_phase(
        RestoreJournalPhase::RollbackCompleted,
        Some(RestoreJournalSafeErrorCode::CommitFailed),
    );
    if store.update(journal).is_err() {
        journal_ambiguous = true;
    }
    if journal_ambiguous {
        Err(RestoreCommitError::JournalAmbiguous)
    } else {
        Err(original_error)
    }
}

fn mark_rollback_failed(journal: &mut RestoreJournal, store: &RestoreJournalStore<'_>) {
    journal.set_phase(
        RestoreJournalPhase::RollbackFailed,
        Some(RestoreJournalSafeErrorCode::RollbackFailed),
    );
    let _ = store.update(journal);
}

fn verify_old_moved(paths: &RestoreCommitPaths) -> Result<(), RestoreCommitError> {
    ensure_path_absent(&paths.current, RestoreCommitError::SnapshotFailed)?;
    ensure_regular_directory(&paths.rollback, RestoreCommitError::SnapshotFailed)
}

fn verify_new_moved(paths: &RestoreCommitPaths) -> Result<(), RestoreCommitError> {
    ensure_regular_directory(&paths.current, RestoreCommitError::PublishFailed)?;
    ensure_path_absent(&paths.stage, RestoreCommitError::PublishFailed)
}

fn ensure_path_absent(
    path: &FilePath,
    error: RestoreCommitError,
) -> Result<(), RestoreCommitError> {
    if restore_commit_path_exists(path)? {
        Err(error)
    } else {
        Ok(())
    }
}

fn ensure_regular_directory(
    path: &FilePath,
    error: RestoreCommitError,
) -> Result<(), RestoreCommitError> {
    let metadata = std_fs::symlink_metadata(path).map_err(|_| error)?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        Err(error)
    } else {
        Ok(())
    }
}

pub(super) fn restore_commit_path_exists(path: &FilePath) -> Result<bool, RestoreCommitError> {
    match std_fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(RestoreCommitError::Conflict),
    }
}

fn serialize_restore_journal(journal: &RestoreJournal) -> Result<Vec<u8>, RestoreCommitError> {
    serde_json::to_vec_pretty(journal).map_err(|_| RestoreCommitError::JournalWriteFailed)
}

pub(super) fn read_restore_journal_strict(
    path: &FilePath,
    expected_operation_id: &str,
    expected_mode: BackupManifestMode,
) -> Result<RestoreJournal, RestoreCommitError> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|_| RestoreCommitError::JournalWriteFailed)?;
    if metadata_is_link_or_reparse(&metadata)
        || !metadata.is_file()
        || metadata.len() > RESTORE_MANIFEST_MAX_BYTES
    {
        return Err(RestoreCommitError::JournalWriteFailed);
    }
    let bytes = std_fs::read(path).map_err(|_| RestoreCommitError::JournalWriteFailed)?;
    let journal: RestoreJournal =
        serde_json::from_slice(&bytes).map_err(|_| RestoreCommitError::JournalWriteFailed)?;
    if journal.format != RESTORE_JOURNAL_FORMAT
        || journal.version != RESTORE_JOURNAL_VERSION
        || journal.operation_id != expected_operation_id
        || journal.mode != expected_mode
        || journal.created_at.trim().is_empty()
        || validate_restore_operation_id(&journal.operation_id).is_err()
    {
        return Err(RestoreCommitError::JournalWriteFailed);
    }
    Ok(journal)
}

pub(super) fn update_restore_journal_atomic(
    parent: &FilePath,
    journal: &RestoreJournal,
) -> Result<(), RestoreCommitError> {
    RestoreJournalStore::new(parent, &RealRestoreCommitIo).update(journal)
}

#[cfg(windows)]
pub(super) fn sync_restore_parent_directory(parent: &FilePath) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    let path = wide_path(parent.as_os_str());
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // Windows does not provide a portable directory fsync equivalent. Opening and closing a
    // directory handle is a best-effort barrier; P2-C3 must still reconcile and revalidate.
    let close_result = unsafe { CloseHandle(handle) };
    if close_result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn sync_restore_parent_directory(parent: &FilePath) -> io::Result<()> {
    std_fs::File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Barrier, Mutex as TestMutex};

    static SYNTHETIC_COMMIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct SyntheticCommitTemp {
        path: PathBuf,
    }

    impl SyntheticCommitTemp {
        fn new(label: &str) -> Self {
            let sequence = SYNTHETIC_COMMIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rikkadesk-restore-commit-{label}-{}-{sequence}",
                std::process::id()
            ));
            let _ = std_fs::remove_dir_all(&path);
            std_fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for SyntheticCommitTemp {
        fn drop(&mut self) {
            let _ = std_fs::remove_dir_all(&self.path);
        }
    }

    struct SyntheticCommitFixture {
        _temp: SyntheticCommitTemp,
        parent: PathBuf,
        operation_id: String,
        paths: RestoreCommitPaths,
        mode: BackupManifestMode,
        original_current_fingerprint: HashMap<String, String>,
        opaque_secret_bytes: Vec<u8>,
    }

    impl SyntheticCommitFixture {
        fn new(label: &str, mode: BackupManifestMode) -> Self {
            let temp = SyntheticCommitTemp::new(label);
            let parent = temp.path.join("synthetic-app-data-parent");
            std_fs::create_dir(&parent).unwrap();
            let sequence = SYNTHETIC_COMMIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let operation_id = format!("1700000000000-4242-{sequence}");
            let paths = derive_restore_commit_paths(parent.clone(), &operation_id).unwrap();
            let opaque_secret_bytes = b"opaque synthetic encrypted blob bytes".to_vec();
            create_synthetic_current(&paths.current, &opaque_secret_bytes);
            create_synthetic_stage(&paths.stage, &operation_id, mode);
            let original_current_fingerprint = directory_fingerprint(&paths.current);
            Self {
                _temp: temp,
                parent,
                operation_id,
                paths,
                mode,
                original_current_fingerprint,
                opaque_secret_bytes,
            }
        }

        fn request(&self) -> RestoreCommitRequest {
            RestoreCommitRequest {
                app_data_parent: self.parent.clone(),
                operation_id: self.operation_id.clone(),
            }
        }

        fn read_journal(&self) -> RestoreJournal {
            read_restore_journal_strict(&self.paths.journal, &self.operation_id, self.mode).unwrap()
        }

        fn current_id_seq(&self) -> u64 {
            read_synthetic_state(&self.paths.current).id_seq
        }
    }

    fn create_synthetic_current(root: &FilePath, opaque_secret_bytes: &[u8]) {
        std_fs::create_dir(root).unwrap();
        std_fs::create_dir(root.join(FILES_DIR_NAME)).unwrap();
        std_fs::create_dir(root.join(FILES_DIR_NAME).join(FILE_BLOBS_DIR_NAME)).unwrap();
        std_fs::create_dir(root.join(SECRETS_DIR_NAME)).unwrap();
        let mut state = default_persisted_state();
        state.id_seq = 1_000;
        state.saved_at = 1_000;
        write_synthetic_state(root, &state);
        std_fs::write(
            root.join(SECRETS_DIR_NAME).join("opaque-synthetic.bin"),
            opaque_secret_bytes,
        )
        .unwrap();
        std_fs::write(
            root.join("state.v1.pre-migration.synthetic.json"),
            b"synthetic diagnostic bytes",
        )
        .unwrap();
        std_fs::write(
            root.join(format!("{STATE_TMP_FILE_PREFIX}.synthetic-stale")),
            b"synthetic stale temp bytes",
        )
        .unwrap();
        validate_existing_mock_api_before_restore(root).unwrap();
    }

    fn create_synthetic_stage(root: &FilePath, operation_id: &str, mode: BackupManifestMode) {
        std_fs::create_dir(root).unwrap();
        let files_root = root.join(FILES_DIR_NAME);
        let blob_root = files_root.join(FILE_BLOBS_DIR_NAME);
        std_fs::create_dir(&files_root).unwrap();
        std_fs::create_dir(&blob_root).unwrap();
        std_fs::create_dir(root.join(SECRETS_DIR_NAME)).unwrap();

        let mut state = default_persisted_state();
        state.id_seq = 10_000;
        state.saved_at = 10_000;
        let storage_key = format!("restore-blob-{operation_id}-0");
        let content = b"synthetic restored managed blob".to_vec();
        let sha256 = sha256_bytes(&content);
        state.files.push(ManagedFileMetadata {
            id: 500,
            storage_key: storage_key.clone(),
            display_name: "synthetic-restored.png".to_string(),
            mime: "image/png".to_string(),
            size_bytes: content.len() as u64,
            sha256: Some(sha256),
            kind: "image".to_string(),
            relative_path: format!("{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{storage_key}"),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            source: "upload".to_string(),
            deleted_at: None,
        });
        if mode == BackupManifestMode::FullLocalData {
            std_fs::write(blob_root.join(&storage_key), content).unwrap();
        }
        write_synthetic_state(root, &state);
        validate_staged_mock_api_directory(root, mode).unwrap();
    }

    fn write_synthetic_state(root: &FilePath, state: &PersistedMockState) {
        std_fs::write(
            root.join(STATE_FILE_NAME),
            serde_json::to_vec_pretty(state).unwrap(),
        )
        .unwrap();
    }

    fn read_synthetic_state(root: &FilePath) -> PersistedMockState {
        serde_json::from_slice(&std_fs::read(root.join(STATE_FILE_NAME)).unwrap()).unwrap()
    }

    fn directory_fingerprint(root: &FilePath) -> HashMap<String, String> {
        fn collect(root: &FilePath, current: &FilePath, output: &mut HashMap<String, String>) {
            for entry in std_fs::read_dir(current).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let metadata = std_fs::symlink_metadata(&path).unwrap();
                if metadata.is_dir() && !metadata_is_link_or_reparse(&metadata) {
                    collect(root, &path, output);
                } else if metadata.is_file() {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    output.insert(relative, sha256_bytes(&std_fs::read(path).unwrap()));
                }
            }
        }
        let mut output = HashMap::new();
        collect(root, root, &mut output);
        output
    }

    fn path_absent(path: &FilePath) -> bool {
        matches!(
            std_fs::symlink_metadata(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        )
    }

    struct SyntheticOfflineGate {
        fail_on_call: Option<u64>,
        calls: AtomicU64,
        barrier: Option<Arc<Barrier>>,
    }

    impl SyntheticOfflineGate {
        fn confirmed() -> Self {
            Self {
                fail_on_call: None,
                calls: AtomicU64::new(0),
                barrier: None,
            }
        }

        fn rejected() -> Self {
            Self {
                fail_on_call: Some(1),
                calls: AtomicU64::new(0),
                barrier: None,
            }
        }

        fn fail_on(call: u64) -> Self {
            Self {
                fail_on_call: Some(call),
                calls: AtomicU64::new(0),
                barrier: None,
            }
        }
    }

    impl RestoreOfflineGate for SyntheticOfflineGate {
        fn verify_offline(
            &self,
            _app_data_parent: &FilePath,
        ) -> Result<OfflineRestorePermit, RestoreCommitError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(barrier) = &self.barrier {
                if call == 1 {
                    barrier.wait();
                }
            }
            if self.fail_on_call == Some(call) {
                Err(RestoreCommitError::OfflineGateFailed)
            } else {
                Ok(OfflineRestorePermit(()))
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SyntheticCommitFault {
        Journal {
            phase: RestoreJournalPhase,
            step: RestoreJournalIoStep,
        },
        Rename(RestoreRenameKind),
        ParentSync(RestoreParentSyncKind),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SyntheticCommitMutation {
        NewCurrentInvalid,
        RestoredCurrentInvalid,
    }

    struct SyntheticRestoreCommitIo {
        fault: Option<SyntheticCommitFault>,
        second_fault: Option<SyntheticCommitFault>,
        fault_used: AtomicU64,
        mutation: Option<SyntheticCommitMutation>,
        journal_writes: AtomicU64,
        renames: TestMutex<Vec<RestoreRenameKind>>,
        parent_syncs: TestMutex<Vec<RestoreParentSyncKind>>,
    }

    impl SyntheticRestoreCommitIo {
        fn clean() -> Self {
            Self {
                fault: None,
                second_fault: None,
                fault_used: AtomicU64::new(0),
                mutation: None,
                journal_writes: AtomicU64::new(0),
                renames: TestMutex::new(Vec::new()),
                parent_syncs: TestMutex::new(Vec::new()),
            }
        }

        fn failing(fault: SyntheticCommitFault) -> Self {
            Self {
                fault: Some(fault),
                ..Self::clean()
            }
        }

        fn failing_two(first: SyntheticCommitFault, second: SyntheticCommitFault) -> Self {
            Self {
                fault: Some(first),
                second_fault: Some(second),
                ..Self::clean()
            }
        }

        fn failing_and_mutating(
            fault: SyntheticCommitFault,
            mutation: SyntheticCommitMutation,
        ) -> Self {
            Self {
                fault: Some(fault),
                mutation: Some(mutation),
                ..Self::clean()
            }
        }

        fn mutating(mutation: SyntheticCommitMutation) -> Self {
            Self {
                mutation: Some(mutation),
                ..Self::clean()
            }
        }

        fn fail_journal_step(
            &self,
            phase: RestoreJournalPhase,
            step: RestoreJournalIoStep,
        ) -> bool {
            self.fail_once(SyntheticCommitFault::Journal { phase, step })
        }

        fn fail_once(&self, candidate: SyntheticCommitFault) -> bool {
            loop {
                let used = self.fault_used.load(Ordering::SeqCst);
                let bit = if self.fault == Some(candidate) && used & 1 == 0 {
                    1
                } else if self.second_fault == Some(candidate) && used & 2 == 0 {
                    2
                } else {
                    return false;
                };
                if self
                    .fault_used
                    .compare_exchange(used, used | bit, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    return true;
                }
            }
        }
    }

    impl RestoreCommitIo for SyntheticRestoreCommitIo {
        fn create_new(
            &self,
            path: &FilePath,
            phase: RestoreJournalPhase,
        ) -> io::Result<std_fs::File> {
            if self.fail_journal_step(phase, RestoreJournalIoStep::Create) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic create failure",
                ));
            }
            std_fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
        }

        fn write_all(
            &self,
            file: &mut std_fs::File,
            data: &[u8],
            phase: RestoreJournalPhase,
        ) -> io::Result<()> {
            self.journal_writes.fetch_add(1, Ordering::Relaxed);
            if self.fail_journal_step(phase, RestoreJournalIoStep::Write) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic write failure",
                ));
            }
            file.write_all(data)
        }

        fn flush(&self, file: &mut std_fs::File, phase: RestoreJournalPhase) -> io::Result<()> {
            if self.fail_journal_step(phase, RestoreJournalIoStep::Flush) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic flush failure",
                ));
            }
            file.flush()
        }

        fn sync_file(&self, file: &std_fs::File, phase: RestoreJournalPhase) -> io::Result<()> {
            if self.fail_journal_step(phase, RestoreJournalIoStep::Sync) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic sync failure",
                ));
            }
            file.sync_all()
        }

        fn replace_journal(
            &self,
            replacement: &FilePath,
            target: &FilePath,
            phase: RestoreJournalPhase,
        ) -> io::Result<()> {
            if self.fail_journal_step(phase, RestoreJournalIoStep::Replace) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic replace failure",
                ));
            }
            replace_state_file(replacement, target)
        }

        fn remove_file(&self, path: &FilePath) {
            let _ = std_fs::remove_file(path);
        }

        fn rename_directory(
            &self,
            source: &FilePath,
            destination: &FilePath,
            kind: RestoreRenameKind,
        ) -> io::Result<()> {
            self.renames.lock().unwrap().push(kind);
            if self.fail_once(SyntheticCommitFault::Rename(kind)) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic rename failure",
                ));
            }
            std_fs::rename(source, destination)?;
            match (self.mutation, kind) {
                (
                    Some(SyntheticCommitMutation::NewCurrentInvalid),
                    RestoreRenameKind::StageToCurrent,
                )
                | (
                    Some(SyntheticCommitMutation::RestoredCurrentInvalid),
                    RestoreRenameKind::RollbackToCurrent,
                ) => {
                    std_fs::write(
                        destination.join(STATE_FILE_NAME),
                        b"invalid synthetic state",
                    )?;
                }
                _ => {}
            }
            Ok(())
        }

        fn sync_parent(&self, _parent: &FilePath, kind: RestoreParentSyncKind) -> io::Result<()> {
            self.parent_syncs.lock().unwrap().push(kind);
            if self.fail_once(SyntheticCommitFault::ParentSync(kind)) {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "synthetic parent sync failure",
                ));
            }
            Ok(())
        }
    }

    fn commit_fixture(
        fixture: &SyntheticCommitFixture,
        io: &SyntheticRestoreCommitIo,
    ) -> Result<RestoreCommitResult, RestoreCommitError> {
        commit_restore_offline_with_io(fixture.request(), &SyntheticOfflineGate::confirmed(), io)
    }

    fn assert_original_current_restored(fixture: &SyntheticCommitFixture) {
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(path_absent(&fixture.paths.rollback));
    }

    fn try_create_directory_symlink(target: &FilePath, link: &FilePath) -> bool {
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link).is_ok()
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (target, link);
            false
        }
    }

    #[test]
    fn restore_commit_success_returns_pending_startup_validation() {
        let fixture = SyntheticCommitFixture::new("success", BackupManifestMode::FullLocalData);
        let io = SyntheticRestoreCommitIo::clean();

        let result = commit_fixture(&fixture, &io).unwrap();

        assert_eq!(result, RestoreCommitResult::PendingStartupValidation);
    }

    #[test]
    fn restore_commit_offline_gate_failure_zero_writes() {
        let fixture =
            SyntheticCommitFixture::new("offline-rejected", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::clean();
        let gate = SyntheticOfflineGate::rejected();

        let result = commit_restore_offline_with_io(fixture.request(), &gate, &io);

        assert_eq!(result, Err(RestoreCommitError::OfflineGateFailed));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(fixture.paths.stage.is_dir());
        assert!(path_absent(&fixture.paths.journal));
        assert_eq!(io.journal_writes.load(Ordering::Relaxed), 0);
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_commit_second_offline_gate_failure_stops_before_rename() {
        let fixture = SyntheticCommitFixture::new("offline-second", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::clean();
        let gate = SyntheticOfflineGate::fail_on(2);

        let result = commit_restore_offline_with_io(fixture.request(), &gate, &io);

        assert_eq!(result, Err(RestoreCommitError::OfflineGateFailed));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(fixture.read_journal().phase, RestoreJournalPhase::Staging);
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_commit_existing_journal_rejected() {
        let fixture = SyntheticCommitFixture::new("journal-exists", BackupManifestMode::StateOnly);
        std_fs::write(&fixture.paths.journal, b"synthetic existing journal").unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::JournalExists));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(fixture.paths.stage.is_dir());
    }

    #[test]
    fn restore_commit_missing_current_rejected_without_default() {
        let fixture = SyntheticCommitFixture::new("missing-current", BackupManifestMode::StateOnly);
        std_fs::remove_dir_all(&fixture.paths.current).unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::CurrentInvalid));
        assert!(path_absent(&fixture.paths.current));
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_invalid_current_rejected() {
        let fixture = SyntheticCommitFixture::new("invalid-current", BackupManifestMode::StateOnly);
        std_fs::write(
            fixture.paths.current.join(STATE_FILE_NAME),
            b"invalid state",
        )
        .unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::CurrentInvalid));
        assert!(fixture.paths.stage.is_dir());
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_missing_stage_rejected() {
        let fixture = SyntheticCommitFixture::new("missing-stage", BackupManifestMode::StateOnly);
        std_fs::remove_dir_all(&fixture.paths.stage).unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::StageInvalid));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_tampered_stage_rejected() {
        let fixture =
            SyntheticCommitFixture::new("tampered-stage", BackupManifestMode::FullLocalData);
        std_fs::write(fixture.paths.stage.join("manifest.json"), b"synthetic").unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::StageInvalid));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_rollback_target_collision_rejected() {
        let fixture =
            SyntheticCommitFixture::new("rollback-collision", BackupManifestMode::StateOnly);
        std_fs::create_dir(&fixture.paths.rollback).unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::Conflict));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
    }

    #[test]
    fn restore_commit_failed_target_collision_rejected() {
        let fixture =
            SyntheticCommitFixture::new("failed-collision", BackupManifestMode::StateOnly);
        std_fs::create_dir(&fixture.paths.failed).unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::Conflict));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
    }

    #[test]
    fn restore_commit_current_symlink_rejected() {
        let fixture = SyntheticCommitFixture::new("current-symlink", BackupManifestMode::StateOnly);
        let real_current = fixture.parent.join("synthetic-real-current");
        std_fs::rename(&fixture.paths.current, &real_current).unwrap();
        if !try_create_directory_symlink(&real_current, &fixture.paths.current) {
            return;
        }

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::CurrentInvalid));
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_stage_symlink_rejected() {
        let fixture = SyntheticCommitFixture::new("stage-symlink", BackupManifestMode::StateOnly);
        let real_stage = fixture.parent.join("synthetic-real-stage");
        std_fs::rename(&fixture.paths.stage, &real_stage).unwrap();
        if !try_create_directory_symlink(&real_stage, &fixture.paths.stage) {
            return;
        }

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::StageInvalid));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
    }

    #[test]
    fn restore_commit_current_secret_directory_symlink_rejected() {
        let fixture =
            SyntheticCommitFixture::new("secret-dir-symlink", BackupManifestMode::StateOnly);
        let secrets = fixture.paths.current.join(SECRETS_DIR_NAME);
        let real_secrets = fixture.parent.join("synthetic-real-secrets");
        std_fs::rename(&secrets, &real_secrets).unwrap();
        if !try_create_directory_symlink(&real_secrets, &secrets) {
            return;
        }

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::CurrentInvalid));
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_current_diagnostics_are_allowed() {
        let fixture =
            SyntheticCommitFixture::new("current-diagnostics", BackupManifestMode::StateOnly);

        let state = validate_existing_mock_api_before_restore(&fixture.paths.current).unwrap();

        assert_eq!(state.id_seq, 1_000);
        assert!(fixture
            .paths
            .current
            .join("state.v1.pre-migration.synthetic.json")
            .is_file());
        assert!(fixture
            .paths
            .current
            .join(format!("{STATE_TMP_FILE_PREFIX}.synthetic-stale"))
            .is_file());
    }

    #[test]
    fn restore_commit_operation_id_rejects_traversal() {
        let fixture =
            SyntheticCommitFixture::new("operation-traversal", BackupManifestMode::StateOnly);
        let mut request = fixture.request();
        request.operation_id = "1700000000000-4242-../1".to_string();

        assert_eq!(
            commit_restore_offline_with_io(
                request,
                &SyntheticOfflineGate::confirmed(),
                &SyntheticRestoreCommitIo::clean(),
            ),
            Err(RestoreCommitError::Conflict)
        );
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_operation_id_rejects_unicode() {
        let fixture =
            SyntheticCommitFixture::new("operation-unicode", BackupManifestMode::StateOnly);
        let mut request = fixture.request();
        request.operation_id = "1700000000000-４２４２-1".to_string();

        assert_eq!(
            commit_restore_offline_with_io(
                request,
                &SyntheticOfflineGate::confirmed(),
                &SyntheticRestoreCommitIo::clean(),
            ),
            Err(RestoreCommitError::Conflict)
        );
    }

    #[test]
    fn restore_commit_operation_id_rejects_extra_component() {
        assert_eq!(
            validate_restore_operation_id("1700000000000-4242-1-2"),
            Err(RestoreCommitError::Conflict)
        );
    }

    #[test]
    fn restore_commit_initial_journal_create_failure() {
        let fixture =
            SyntheticCommitFixture::new("journal-create-failure", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::Staging,
            step: RestoreJournalIoStep::Create,
        });

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::JournalWriteFailed));
        assert!(path_absent(&fixture.paths.journal));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_journal_staging_write_failure_removes_partial_file() {
        let fixture =
            SyntheticCommitFixture::new("journal-write-failure", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::Staging,
            step: RestoreJournalIoStep::Write,
        });

        assert_eq!(
            commit_fixture(&fixture, &io),
            Err(RestoreCommitError::JournalWriteFailed)
        );
        assert!(path_absent(&fixture.paths.journal));
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_journal_staging_flush_failure_removes_partial_file() {
        let fixture =
            SyntheticCommitFixture::new("journal-flush-failure", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::Staging,
            step: RestoreJournalIoStep::Flush,
        });

        assert_eq!(
            commit_fixture(&fixture, &io),
            Err(RestoreCommitError::JournalWriteFailed)
        );
        assert!(path_absent(&fixture.paths.journal));
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_journal_staging_sync_failure_removes_partial_file() {
        let fixture =
            SyntheticCommitFixture::new("journal-sync-failure", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::Staging,
            step: RestoreJournalIoStep::Sync,
        });

        assert_eq!(
            commit_fixture(&fixture, &io),
            Err(RestoreCommitError::JournalWriteFailed)
        );
        assert!(path_absent(&fixture.paths.journal));
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_journal_backup_current_replace_failure_keeps_directories() {
        let fixture =
            SyntheticCommitFixture::new("journal-backup-failure", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::BackupCurrent,
            step: RestoreJournalIoStep::Replace,
        });

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::JournalWriteFailed));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(fixture.read_journal().phase, RestoreJournalPhase::Staging);
        assert!(io.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn restore_journal_unknown_field_rejected() {
        let fixture =
            SyntheticCommitFixture::new("journal-unknown-field", BackupManifestMode::StateOnly);
        let mut value =
            serde_json::to_value(RestoreJournal::new(&fixture.operation_id, fixture.mode)).unwrap();
        value["unknown"] = json!(true);
        std_fs::write(&fixture.paths.journal, serde_json::to_vec(&value).unwrap()).unwrap();

        assert_eq!(
            read_restore_journal_strict(
                &fixture.paths.journal,
                &fixture.operation_id,
                fixture.mode
            ),
            Err(RestoreCommitError::JournalWriteFailed)
        );
    }

    #[test]
    fn restore_journal_unknown_phase_rejected() {
        let fixture =
            SyntheticCommitFixture::new("journal-unknown-phase", BackupManifestMode::StateOnly);
        let mut value =
            serde_json::to_value(RestoreJournal::new(&fixture.operation_id, fixture.mode)).unwrap();
        value["phase"] = json!("unknown-phase");
        std_fs::write(&fixture.paths.journal, serde_json::to_vec(&value).unwrap()).unwrap();

        assert_eq!(
            read_restore_journal_strict(
                &fixture.paths.journal,
                &fixture.operation_id,
                fixture.mode
            ),
            Err(RestoreCommitError::JournalWriteFailed)
        );
    }

    #[test]
    fn restore_journal_operation_id_mismatch_rejected() {
        let fixture = SyntheticCommitFixture::new(
            "journal-operation-mismatch",
            BackupManifestMode::StateOnly,
        );
        let journal = RestoreJournal::new(&fixture.operation_id, fixture.mode);
        std_fs::write(
            &fixture.paths.journal,
            serialize_restore_journal(&journal).unwrap(),
        )
        .unwrap();

        assert_eq!(
            read_restore_journal_strict(
                &fixture.paths.journal,
                "1700000000000-4242-999",
                fixture.mode
            ),
            Err(RestoreCommitError::JournalWriteFailed)
        );
    }

    #[test]
    fn restore_journal_mode_mismatch_rejected() {
        let fixture =
            SyntheticCommitFixture::new("journal-mode-mismatch", BackupManifestMode::StateOnly);
        let journal = RestoreJournal::new(&fixture.operation_id, fixture.mode);
        std_fs::write(
            &fixture.paths.journal,
            serialize_restore_journal(&journal).unwrap(),
        )
        .unwrap();

        assert_eq!(
            read_restore_journal_strict(
                &fixture.paths.journal,
                &fixture.operation_id,
                BackupManifestMode::FullLocalData,
            ),
            Err(RestoreCommitError::JournalWriteFailed)
        );
    }

    #[test]
    fn restore_journal_rollback_completed_round_trip() {
        let fixture = SyntheticCommitFixture::new(
            "journal-rollback-completed",
            BackupManifestMode::StateOnly,
        );
        let mut journal = RestoreJournal::new(&fixture.operation_id, fixture.mode);
        journal.set_phase(
            RestoreJournalPhase::RollbackCompleted,
            Some(RestoreJournalSafeErrorCode::CommitFailed),
        );
        std_fs::write(
            &fixture.paths.journal,
            serialize_restore_journal(&journal).unwrap(),
        )
        .unwrap();

        let parsed = fixture.read_journal();

        assert_eq!(parsed.phase, RestoreJournalPhase::RollbackCompleted);
        assert_eq!(
            parsed.safe_error_code,
            Some(RestoreJournalSafeErrorCode::CommitFailed)
        );
    }

    #[test]
    fn restore_commit_failure_current_to_rollback_rename() {
        let fixture =
            SyntheticCommitFixture::new("old-rename-failure", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Rename(
            RestoreRenameKind::CurrentToRollback,
        ));

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::SnapshotFailed));
        assert_eq!(
            directory_fingerprint(&fixture.paths.current),
            fixture.original_current_fingerprint
        );
        assert!(fixture.paths.stage.is_dir());
        assert!(path_absent(&fixture.paths.rollback));
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::BackupCurrent
        );
    }

    #[test]
    fn restore_rollback_old_parent_sync_failure_restores_current() {
        let fixture =
            SyntheticCommitFixture::new("old-sync-rollback", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::ParentSync(
            RestoreParentSyncKind::OldMoved,
        ));

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::SnapshotFailed));
        assert_original_current_restored(&fixture);
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_rollback_commit_old_moved_journal_failure_restores_current() {
        let fixture =
            SyntheticCommitFixture::new("old-journal-rollback", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::CommitOldMoved,
            step: RestoreJournalIoStep::Replace,
        });

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::JournalWriteFailed));
        assert_original_current_restored(&fixture);
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_rollback_stage_to_current_failure_restores_old() {
        let fixture =
            SyntheticCommitFixture::new("new-rename-rollback", BackupManifestMode::FullLocalData);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Rename(
            RestoreRenameKind::StageToCurrent,
        ));

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::PublishFailed));
        assert_original_current_restored(&fixture);
        assert!(fixture.paths.stage.is_dir());
        assert!(path_absent(&fixture.paths.failed));
    }

    #[test]
    fn restore_rollback_old_moved_restore_rename_failure_is_fatal() {
        let fixture =
            SyntheticCommitFixture::new("old-rollback-fatal", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing_two(
            SyntheticCommitFault::Rename(RestoreRenameKind::StageToCurrent),
            SyntheticCommitFault::Rename(RestoreRenameKind::RollbackToCurrent),
        );

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::ManualRecoveryRequired));
        assert!(path_absent(&fixture.paths.current));
        assert!(fixture.paths.rollback.is_dir());
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_rollback_new_parent_sync_failure_preserves_failed_and_restores_old() {
        let fixture =
            SyntheticCommitFixture::new("new-sync-rollback", BackupManifestMode::FullLocalData);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::ParentSync(
            RestoreParentSyncKind::NewMoved,
        ));

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::PublishFailed));
        assert_original_current_restored(&fixture);
        assert!(fixture.paths.failed.is_dir());
        assert!(path_absent(&fixture.paths.stage));
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_rollback_commit_new_moved_journal_failure() {
        let fixture =
            SyntheticCommitFixture::new("new-journal-rollback", BackupManifestMode::FullLocalData);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Journal {
            phase: RestoreJournalPhase::CommitNewMoved,
            step: RestoreJournalIoStep::Replace,
        });

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::JournalWriteFailed));
        assert_original_current_restored(&fixture);
        assert!(fixture.paths.failed.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_rollback_new_current_validation_failure() {
        let fixture = SyntheticCommitFixture::new(
            "new-validation-rollback",
            BackupManifestMode::FullLocalData,
        );
        let io = SyntheticRestoreCommitIo::mutating(SyntheticCommitMutation::NewCurrentInvalid);

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::ValidationFailed));
        assert_original_current_restored(&fixture);
        assert!(fixture.paths.failed.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_rollback_failed_directory_rename_failure_is_fatal() {
        let fixture =
            SyntheticCommitFixture::new("failed-rename-fatal", BackupManifestMode::FullLocalData);
        let io = SyntheticRestoreCommitIo::failing_and_mutating(
            SyntheticCommitFault::Rename(RestoreRenameKind::NewCurrentToFailed),
            SyntheticCommitMutation::NewCurrentInvalid,
        );

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::ManualRecoveryRequired));
        assert!(fixture.paths.current.is_dir());
        assert!(fixture.paths.rollback.is_dir());
        assert!(path_absent(&fixture.paths.failed));
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_rollback_current_restore_rename_failure_is_fatal() {
        let fixture = SyntheticCommitFixture::new(
            "rollback-current-fatal",
            BackupManifestMode::FullLocalData,
        );
        let io = SyntheticRestoreCommitIo::failing_and_mutating(
            SyntheticCommitFault::Rename(RestoreRenameKind::RollbackToCurrent),
            SyntheticCommitMutation::NewCurrentInvalid,
        );

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::ManualRecoveryRequired));
        assert!(path_absent(&fixture.paths.current));
        assert!(fixture.paths.rollback.is_dir());
        assert!(fixture.paths.failed.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_rollback_restored_original_validation_failure_is_fatal() {
        let fixture =
            SyntheticCommitFixture::new("restored-invalid-fatal", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing_and_mutating(
            SyntheticCommitFault::Rename(RestoreRenameKind::StageToCurrent),
            SyntheticCommitMutation::RestoredCurrentInvalid,
        );

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::ManualRecoveryRequired));
        assert!(fixture.paths.current.is_dir());
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_journal_rollback_required_failure_returns_ambiguous() {
        let fixture = SyntheticCommitFixture::new(
            "rollback-required-ambiguous",
            BackupManifestMode::StateOnly,
        );
        let io = SyntheticRestoreCommitIo::failing_two(
            SyntheticCommitFault::Rename(RestoreRenameKind::StageToCurrent),
            SyntheticCommitFault::Journal {
                phase: RestoreJournalPhase::RollbackRequired,
                step: RestoreJournalIoStep::Write,
            },
        );

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::JournalAmbiguous));
        assert_original_current_restored(&fixture);
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_journal_rollback_completed_failure_returns_ambiguous() {
        let fixture = SyntheticCommitFixture::new(
            "rollback-completed-ambiguous",
            BackupManifestMode::StateOnly,
        );
        let io = SyntheticRestoreCommitIo::failing_two(
            SyntheticCommitFault::Rename(RestoreRenameKind::StageToCurrent),
            SyntheticCommitFault::Journal {
                phase: RestoreJournalPhase::RollbackCompleted,
                step: RestoreJournalIoStep::Replace,
            },
        );

        let result = commit_fixture(&fixture, &io);

        assert_eq!(result, Err(RestoreCommitError::JournalAmbiguous));
        assert_original_current_restored(&fixture);
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackRequired
        );
    }

    #[test]
    fn restore_commit_success_keeps_rollback_directory() {
        let fixture = SyntheticCommitFixture::new(
            "success-keeps-rollback",
            BackupManifestMode::FullLocalData,
        );

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert!(fixture.paths.rollback.is_dir());
        assert_eq!(
            directory_fingerprint(&fixture.paths.rollback),
            fixture.original_current_fingerprint
        );
    }

    #[test]
    fn restore_commit_success_keeps_journal_at_commit_new_moved() {
        let fixture = SyntheticCommitFixture::new(
            "success-journal-boundary",
            BackupManifestMode::FullLocalData,
        );

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        let journal = fixture.read_journal();
        assert_eq!(journal.phase, RestoreJournalPhase::CommitNewMoved);
        assert_eq!(journal.safe_error_code, None);
    }

    #[test]
    fn restore_commit_success_consumes_stage_into_current() {
        let fixture = SyntheticCommitFixture::new(
            "success-consumes-stage",
            BackupManifestMode::FullLocalData,
        );

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert!(path_absent(&fixture.paths.stage));
        assert_eq!(fixture.current_id_seq(), 10_000);
    }

    #[test]
    fn restore_commit_success_does_not_create_failed_directory() {
        let fixture =
            SyntheticCommitFixture::new("success-no-failed", BackupManifestMode::StateOnly);

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert!(path_absent(&fixture.paths.failed));
    }

    #[test]
    fn restore_commit_success_never_writes_startup_or_completed() {
        let fixture =
            SyntheticCommitFixture::new("success-not-completed", BackupManifestMode::StateOnly);

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert!(!matches!(
            fixture.read_journal().phase,
            RestoreJournalPhase::StartupValidation | RestoreJournalPhase::Completed
        ));
    }

    #[test]
    fn restore_commit_opaque_secret_fixture_preserved_in_rollback() {
        let fixture =
            SyntheticCommitFixture::new("opaque-secret-success", BackupManifestMode::StateOnly);

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert_eq!(
            std_fs::read(
                fixture
                    .paths
                    .rollback
                    .join(SECRETS_DIR_NAME)
                    .join("opaque-synthetic.bin")
            )
            .unwrap(),
            fixture.opaque_secret_bytes
        );
    }

    #[cfg(windows)]
    #[test]
    fn restore_commit_current_prevalidation_does_not_open_secret_files() {
        use std::os::windows::fs::OpenOptionsExt;

        let fixture =
            SyntheticCommitFixture::new("opaque-secret-read-probe", BackupManifestMode::StateOnly);
        let secret_path = fixture
            .paths
            .current
            .join(SECRETS_DIR_NAME)
            .join("opaque-synthetic.bin");
        let _exclusive_secret_handle = std_fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(secret_path)
            .unwrap();

        validate_existing_mock_api_before_restore(&fixture.paths.current).unwrap();
    }

    #[test]
    fn restore_rollback_preserves_opaque_secret_fixture() {
        let fixture =
            SyntheticCommitFixture::new("opaque-secret-rollback", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing(SyntheticCommitFault::Rename(
            RestoreRenameKind::StageToCurrent,
        ));

        commit_fixture(&fixture, &io).unwrap_err();

        assert_eq!(
            std_fs::read(
                fixture
                    .paths
                    .current
                    .join(SECRETS_DIR_NAME)
                    .join("opaque-synthetic.bin")
            )
            .unwrap(),
            fixture.opaque_secret_bytes
        );
    }

    #[test]
    fn restore_commit_safe_errors_contain_no_local_values() {
        let fixture = SyntheticCommitFixture::new("safe-errors", BackupManifestMode::StateOnly);
        let local_path = fixture.parent.to_string_lossy();
        let operation_id = fixture.operation_id.clone();
        let messages = [
            RestoreCommitError::OfflineGateFailed,
            RestoreCommitError::Conflict,
            RestoreCommitError::JournalExists,
            RestoreCommitError::JournalWriteFailed,
            RestoreCommitError::StageInvalid,
            RestoreCommitError::CurrentInvalid,
            RestoreCommitError::SnapshotFailed,
            RestoreCommitError::PublishFailed,
            RestoreCommitError::ValidationFailed,
            RestoreCommitError::RollbackFailed,
            RestoreCommitError::JournalAmbiguous,
            RestoreCommitError::ManualRecoveryRequired,
        ]
        .map(|error| error.to_string());

        assert!(messages.iter().all(|message| {
            !message.contains(local_path.as_ref())
                && !message.contains(&operation_id)
                && !message.contains("secretRef")
                && !message.contains("storageKey")
                && !message.contains("opaque-synthetic.bin")
        }));
    }

    #[test]
    fn restore_commit_unrelated_directory_preserved() {
        let fixture =
            SyntheticCommitFixture::new("unrelated-preserved", BackupManifestMode::StateOnly);
        let unrelated = fixture.parent.join("synthetic-unrelated-directory");
        std_fs::create_dir(&unrelated).unwrap();
        std_fs::write(unrelated.join("marker"), b"synthetic marker").unwrap();

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert_eq!(
            std_fs::read(unrelated.join("marker")).unwrap(),
            b"synthetic marker"
        );
    }

    #[test]
    fn restore_commit_case_folded_current_name_collision_rejected() {
        let fixture =
            SyntheticCommitFixture::new("case-folded-current", BackupManifestMode::StateOnly);
        let alternate = fixture.parent.join("MOCK-API");
        std_fs::rename(&fixture.paths.current, &alternate).unwrap();

        let result = commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean());

        assert_eq!(result, Err(RestoreCommitError::Conflict));
        assert!(path_absent(&fixture.paths.journal));
    }

    #[test]
    fn restore_commit_concurrent_attempts_serialize_and_second_fails_closed() {
        let fixture = SyntheticCommitFixture::new("concurrent", BackupManifestMode::StateOnly);

        let (first, second) = std::thread::scope(|scope| {
            let first =
                scope.spawn(|| commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()));
            let second =
                scope.spawn(|| commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()));
            (first.join().unwrap(), second.join().unwrap())
        });

        assert!(matches!(
            (first, second),
            (
                Ok(RestoreCommitResult::PendingStartupValidation),
                Err(RestoreCommitError::JournalExists)
            ) | (
                Err(RestoreCommitError::JournalExists),
                Ok(RestoreCommitResult::PendingStartupValidation)
            )
        ));
    }

    #[test]
    fn restore_journal_success_leaves_no_temp_file() {
        let fixture = SyntheticCommitFixture::new("journal-no-temp", BackupManifestMode::StateOnly);

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert!(std_fs::read_dir(&fixture.parent).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{RESTORE_JOURNAL_TEMP_PREFIX}."))
        }));
    }

    #[test]
    fn restore_commit_success_preserves_schema_and_provider_versions() {
        let fixture =
            SyntheticCommitFixture::new("version-boundary", BackupManifestMode::StateOnly);

        commit_fixture(&fixture, &SyntheticRestoreCommitIo::clean()).unwrap();

        assert_eq!(
            read_synthetic_state(&fixture.paths.current).schema_version,
            6
        );
        assert_eq!(STATE_SCHEMA_VERSION, 6);
        assert_eq!(PROVIDER_IMPORT_EXPORT_VERSION, 4);
    }

    #[test]
    fn restore_commit_success_uses_directory_moves_not_state_overwrite() {
        let fixture =
            SyntheticCommitFixture::new("directory-moves", BackupManifestMode::FullLocalData);
        let io = SyntheticRestoreCommitIo::clean();

        commit_fixture(&fixture, &io).unwrap();

        assert_eq!(
            directory_fingerprint(&fixture.paths.rollback),
            fixture.original_current_fingerprint
        );
        assert_eq!(fixture.current_id_seq(), 10_000);
        assert_eq!(
            io.renames.lock().unwrap().as_slice(),
            &[
                RestoreRenameKind::CurrentToRollback,
                RestoreRenameKind::StageToCurrent,
            ]
        );
    }

    #[test]
    fn restore_rollback_failure_never_creates_default_state() {
        let fixture =
            SyntheticCommitFixture::new("no-default-on-fatal", BackupManifestMode::StateOnly);
        let io = SyntheticRestoreCommitIo::failing_two(
            SyntheticCommitFault::Rename(RestoreRenameKind::StageToCurrent),
            SyntheticCommitFault::Rename(RestoreRenameKind::RollbackToCurrent),
        );

        assert_eq!(
            commit_fixture(&fixture, &io),
            Err(RestoreCommitError::ManualRecoveryRequired)
        );
        assert!(path_absent(&fixture.paths.current));
        assert!(fixture.paths.rollback.join(STATE_FILE_NAME).is_file());
    }

    #[cfg(windows)]
    #[test]
    fn restore_commit_windows_parent_sync_smoke() {
        let temp = SyntheticCommitTemp::new("windows-parent-sync");

        sync_restore_parent_directory(&temp.path).unwrap();
    }
}
