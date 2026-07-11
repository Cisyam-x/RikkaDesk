#![allow(dead_code)]

use super::restore_commit::{
    derive_restore_commit_paths, restore_commit_path_exists, sync_restore_parent_directory,
    update_restore_journal_atomic, validate_existing_mock_api_before_restore,
    validate_restore_commit_parent, validate_restore_operation_id, RestoreCommitPaths,
    RestoreJournal, RestoreJournalPhase, RestoreJournalSafeErrorCode, RESTORE_COMMIT_MUTEX,
    RESTORE_FAILED_DIR_PREFIX, RESTORE_JOURNAL_FILE_NAME, RESTORE_JOURNAL_FORMAT,
    RESTORE_JOURNAL_TEMP_PREFIX, RESTORE_JOURNAL_VERSION, RESTORE_OPERATION_ID_MAX_LEN,
    RESTORE_ROLLBACK_DIR_PREFIX,
};
use super::*;
use std::collections::BTreeSet;

const RESTORE_JOURNAL_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
pub(super) enum RestoreStartupDisposition {
    NoRestoreOperation,
    StartProvisionalRestore(RestoreStartupToken),
    StartRolledBackCurrent,
    StartCompletedCurrent,
}

#[derive(Debug)]
pub(super) struct RestoreStartupToken {
    parent: PathBuf,
    operation_id: String,
    mode: BackupManifestMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestoreRecoveryError {
    JournalInvalid,
    JournalUnsupported,
    TopologyConflict,
    OperationMismatch,
    CurrentInvalid,
    CandidateInvalid,
    RollbackFailed,
    StartupLoadFailed,
    CompletionFailed,
    ManualRecoveryRequired,
}

impl fmt::Display for RestoreRecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::JournalInvalid => "restore recovery journal is invalid",
            Self::JournalUnsupported => "restore recovery journal version is unsupported",
            Self::TopologyConflict => "restore recovery topology is ambiguous",
            Self::OperationMismatch => "restore recovery operation does not match",
            Self::CurrentInvalid => "restore recovery current data is invalid",
            Self::CandidateInvalid => "restore recovery candidate data is invalid",
            Self::RollbackFailed => "restore recovery rollback failed",
            Self::StartupLoadFailed => "restore provisional startup load failed",
            Self::CompletionFailed => "restore startup completion failed",
            Self::ManualRecoveryRequired => "restore recovery requires manual recovery",
        };
        formatter.write_str(message)
    }
}

impl Error for RestoreRecoveryError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreRecoveryRenameKind {
    CurrentToFailed,
    RollbackToCurrent,
}

trait RestoreRecoveryIo: Send + Sync {
    fn rename_directory(
        &self,
        source: &FilePath,
        destination: &FilePath,
        kind: RestoreRecoveryRenameKind,
    ) -> io::Result<()>;

    fn sync_parent(&self, parent: &FilePath) -> io::Result<()>;

    fn update_journal(
        &self,
        parent: &FilePath,
        journal: &RestoreJournal,
    ) -> Result<(), RestoreRecoveryError>;
}

struct RealRestoreRecoveryIo;

impl RestoreRecoveryIo for RealRestoreRecoveryIo {
    fn rename_directory(
        &self,
        source: &FilePath,
        destination: &FilePath,
        _kind: RestoreRecoveryRenameKind,
    ) -> io::Result<()> {
        std_fs::rename(source, destination)
    }

    fn sync_parent(&self, parent: &FilePath) -> io::Result<()> {
        sync_restore_parent_directory(parent)
    }

    fn update_journal(
        &self,
        parent: &FilePath,
        journal: &RestoreJournal,
    ) -> Result<(), RestoreRecoveryError> {
        update_restore_journal_atomic(parent, journal)
            .map_err(|_| RestoreRecoveryError::CompletionFailed)
    }
}

#[derive(Default)]
struct RestoreArtifactInventory {
    current_exists: bool,
    journal_exists: bool,
    journal_temp_count: usize,
    stages: BTreeSet<String>,
    temp_stages: BTreeSet<String>,
    rollbacks: BTreeSet<String>,
    failed: BTreeSet<String>,
}

impl RestoreArtifactInventory {
    fn all_operation_ids(&self) -> BTreeSet<&str> {
        self.stages
            .iter()
            .chain(self.temp_stages.iter())
            .chain(self.rollbacks.iter())
            .chain(self.failed.iter())
            .map(String::as_str)
            .collect()
    }

    fn operation_stage_exists(&self, operation_id: &str) -> bool {
        self.stages.contains(operation_id)
    }

    fn operation_temp_stage_exists(&self, operation_id: &str) -> bool {
        self.temp_stages.contains(operation_id)
    }

    fn operation_rollback_exists(&self, operation_id: &str) -> bool {
        self.rollbacks.contains(operation_id)
    }

    fn operation_failed_exists(&self, operation_id: &str) -> bool {
        self.failed.contains(operation_id)
    }
}

pub(super) async fn reconcile_restore_before_start(
    app_data_parent: &FilePath,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    let parent = app_data_parent.to_path_buf();
    tokio::task::spawn_blocking(move || {
        reconcile_restore_before_start_with_io(&parent, &RealRestoreRecoveryIo)
    })
    .await
    .map_err(|_| RestoreRecoveryError::TopologyConflict)?
}

pub(super) async fn mark_restore_startup_completed(
    token: RestoreStartupToken,
) -> Result<(), RestoreRecoveryError> {
    tokio::task::spawn_blocking(move || {
        mark_restore_startup_completed_with_io(token, &RealRestoreRecoveryIo)
    })
    .await
    .map_err(|_| RestoreRecoveryError::CompletionFailed)?
}

pub(super) async fn rollback_after_provisional_startup_failure(
    token: RestoreStartupToken,
) -> Result<(), RestoreRecoveryError> {
    tokio::task::spawn_blocking(move || {
        rollback_after_provisional_startup_failure_with_io(token, &RealRestoreRecoveryIo)
    })
    .await
    .map_err(|_| RestoreRecoveryError::RollbackFailed)?
}

fn reconcile_restore_before_start_with_io(
    app_data_parent: &FilePath,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    let _guard = RESTORE_COMMIT_MUTEX
        .lock()
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let Some(parent) = canonical_restore_parent_if_present(app_data_parent)? else {
        return Ok(RestoreStartupDisposition::NoRestoreOperation);
    };
    let inventory = inventory_restore_artifacts(&parent)?;
    if !inventory.journal_exists {
        return reconcile_without_journal(&inventory);
    }

    let journal = read_recovery_journal(&parent.join(RESTORE_JOURNAL_FILE_NAME))?;
    validate_inventory_for_journal(&inventory, &journal)?;
    let paths = derive_restore_commit_paths(parent, &journal.operation_id)
        .map_err(|_| RestoreRecoveryError::JournalInvalid)?;

    match journal.phase {
        RestoreJournalPhase::Staging | RestoreJournalPhase::BackupCurrent => {
            reconcile_pre_move_phase(&paths, journal, io)
        }
        RestoreJournalPhase::CommitOldMoved => perform_rollback(&paths, journal, io),
        RestoreJournalPhase::CommitNewMoved | RestoreJournalPhase::StartupValidation => {
            reconcile_provisional_phase(&paths, journal, io)
        }
        RestoreJournalPhase::RollbackRequired => reconcile_rollback_required(&paths, journal, io),
        RestoreJournalPhase::RollbackCompleted => {
            reconcile_rollback_completed(&paths, &inventory, &journal)
        }
        RestoreJournalPhase::RollbackFailed => Err(RestoreRecoveryError::ManualRecoveryRequired),
        RestoreJournalPhase::Completed => reconcile_completed(&paths, &inventory, &journal),
    }
}

fn canonical_restore_parent_if_present(
    parent: &FilePath,
) -> Result<Option<PathBuf>, RestoreRecoveryError> {
    match std_fs::symlink_metadata(parent) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(RestoreRecoveryError::TopologyConflict),
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() => {
            Err(RestoreRecoveryError::TopologyConflict)
        }
        Ok(_) => validate_restore_commit_parent(parent)
            .map(Some)
            .map_err(|_| RestoreRecoveryError::TopologyConflict),
    }
}

fn inventory_restore_artifacts(
    parent: &FilePath,
) -> Result<RestoreArtifactInventory, RestoreRecoveryError> {
    let mut inventory = RestoreArtifactInventory::default();
    for entry in std_fs::read_dir(parent).map_err(|_| RestoreRecoveryError::TopologyConflict)? {
        let entry = entry.map_err(|_| RestoreRecoveryError::TopologyConflict)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
        let lower = name.to_ascii_lowercase();
        let metadata = std_fs::symlink_metadata(entry.path())
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?;

        if lower == PERSIST_DIR_NAME.to_ascii_lowercase() {
            if name != PERSIST_DIR_NAME
                || metadata_is_link_or_reparse(&metadata)
                || !metadata.is_dir()
            {
                return Err(RestoreRecoveryError::TopologyConflict);
            }
            inventory.current_exists = true;
            continue;
        }
        if lower == RESTORE_JOURNAL_FILE_NAME.to_ascii_lowercase() {
            if name != RESTORE_JOURNAL_FILE_NAME
                || metadata_is_link_or_reparse(&metadata)
                || !metadata.is_file()
            {
                return Err(RestoreRecoveryError::JournalInvalid);
            }
            inventory.journal_exists = true;
            continue;
        }
        if lower.starts_with(&format!(
            "{}.",
            RESTORE_JOURNAL_TEMP_PREFIX.to_ascii_lowercase()
        )) {
            if !name.starts_with(&format!("{RESTORE_JOURNAL_TEMP_PREFIX}."))
                || metadata_is_link_or_reparse(&metadata)
                || !metadata.is_file()
                || !valid_journal_temp_name(&name)
            {
                return Err(RestoreRecoveryError::TopologyConflict);
            }
            inventory.journal_temp_count += 1;
            continue;
        }
        if let Some(operation_id) =
            parse_restore_artifact_operation_id(&name, RESTORE_STAGE_TEMP_DIR_PREFIX, &metadata)?
        {
            inventory.temp_stages.insert(operation_id);
            continue;
        }
        if let Some(operation_id) =
            parse_restore_artifact_operation_id(&name, RESTORE_STAGE_FINAL_DIR_PREFIX, &metadata)?
        {
            inventory.stages.insert(operation_id);
            continue;
        }
        if let Some(operation_id) =
            parse_restore_artifact_operation_id(&name, RESTORE_ROLLBACK_DIR_PREFIX, &metadata)?
        {
            inventory.rollbacks.insert(operation_id);
            continue;
        }
        if let Some(operation_id) =
            parse_restore_artifact_operation_id(&name, RESTORE_FAILED_DIR_PREFIX, &metadata)?
        {
            inventory.failed.insert(operation_id);
            continue;
        }

        if lower.starts_with("mock-api.restore-stage.")
            || lower.starts_with("mock-api.pre-restore.")
            || lower.starts_with("mock-api.failed-restore.")
            || lower.starts_with("restore-journal.json")
        {
            return Err(RestoreRecoveryError::TopologyConflict);
        }
    }
    Ok(inventory)
}

fn parse_restore_artifact_operation_id(
    name: &str,
    prefix: &str,
    metadata: &std_fs::Metadata,
) -> Result<Option<String>, RestoreRecoveryError> {
    let expected_prefix = format!("{prefix}.");
    if !name
        .to_ascii_lowercase()
        .starts_with(&expected_prefix.to_ascii_lowercase())
    {
        return Ok(None);
    }
    if !name.starts_with(&expected_prefix)
        || metadata_is_link_or_reparse(metadata)
        || !metadata.is_dir()
    {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    let operation_id = &name[expected_prefix.len()..];
    validate_restore_operation_id(operation_id)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    Ok(Some(operation_id.to_string()))
}

fn valid_journal_temp_name(name: &str) -> bool {
    let prefix = format!("{RESTORE_JOURNAL_TEMP_PREFIX}.");
    let Some(suffix) = name.strip_prefix(&prefix) else {
        return false;
    };
    let components = suffix.split('.').collect::<Vec<_>>();
    components.len() == 2
        && components.iter().all(|component| {
            !component.is_empty()
                && component.len() <= 20
                && component.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn reconcile_without_journal(
    inventory: &RestoreArtifactInventory,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    if !inventory.rollbacks.is_empty() || !inventory.failed.is_empty() {
        return Err(RestoreRecoveryError::ManualRecoveryRequired);
    }
    Ok(RestoreStartupDisposition::NoRestoreOperation)
}

fn read_recovery_journal(path: &FilePath) -> Result<RestoreJournal, RestoreRecoveryError> {
    let metadata =
        std_fs::symlink_metadata(path).map_err(|_| RestoreRecoveryError::JournalInvalid)?;
    if metadata_is_link_or_reparse(&metadata)
        || !metadata.is_file()
        || metadata.len() > RESTORE_JOURNAL_MAX_BYTES
    {
        return Err(RestoreRecoveryError::JournalInvalid);
    }
    let bytes = std_fs::read(path).map_err(|_| RestoreRecoveryError::JournalInvalid)?;
    if bytes.len() as u64 > RESTORE_JOURNAL_MAX_BYTES {
        return Err(RestoreRecoveryError::JournalInvalid);
    }
    let raw: Value =
        serde_json::from_slice(&bytes).map_err(|_| RestoreRecoveryError::JournalInvalid)?;
    let version = raw
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(RestoreRecoveryError::JournalInvalid)?;
    if version > u64::from(RESTORE_JOURNAL_VERSION) {
        return Err(RestoreRecoveryError::JournalUnsupported);
    }
    if version != u64::from(RESTORE_JOURNAL_VERSION) {
        return Err(RestoreRecoveryError::JournalInvalid);
    }
    let journal: RestoreJournal =
        serde_json::from_value(raw).map_err(|_| RestoreRecoveryError::JournalInvalid)?;
    if journal.format != RESTORE_JOURNAL_FORMAT
        || journal.version != RESTORE_JOURNAL_VERSION
        || journal.created_at.trim().is_empty()
        || journal.operation_id.len() > RESTORE_OPERATION_ID_MAX_LEN
        || validate_restore_operation_id(&journal.operation_id).is_err()
    {
        return Err(RestoreRecoveryError::JournalInvalid);
    }
    Ok(journal)
}

fn validate_inventory_for_journal(
    inventory: &RestoreArtifactInventory,
    journal: &RestoreJournal,
) -> Result<(), RestoreRecoveryError> {
    if inventory.rollbacks.len() > 1 || inventory.failed.len() > 1 {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    if inventory
        .all_operation_ids()
        .iter()
        .any(|operation_id| *operation_id != journal.operation_id)
    {
        return Err(RestoreRecoveryError::OperationMismatch);
    }
    Ok(())
}

fn reconcile_pre_move_phase(
    paths: &RestoreCommitPaths,
    journal: RestoreJournal,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    if restore_commit_path_exists(&paths.failed)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
    {
        return Err(RestoreRecoveryError::ManualRecoveryRequired);
    }
    if restore_commit_path_exists(&paths.rollback)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
    {
        return perform_rollback(paths, journal, io);
    }
    validate_existing_mock_api_before_restore(&paths.current)
        .map_err(|_| RestoreRecoveryError::CurrentInvalid)?;
    transition_journal(
        paths,
        &journal,
        RestoreJournalPhase::RollbackCompleted,
        None,
        io,
    )?;
    Ok(RestoreStartupDisposition::StartRolledBackCurrent)
}

fn reconcile_provisional_phase(
    paths: &RestoreCommitPaths,
    journal: RestoreJournal,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    if !restore_commit_path_exists(&paths.rollback)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
    {
        return Err(RestoreRecoveryError::ManualRecoveryRequired);
    }
    let current_exists = restore_commit_path_exists(&paths.current)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let stage_exists = restore_commit_path_exists(&paths.stage)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let temp_stage_exists = restore_commit_path_exists(&paths.temp_stage)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let failed_exists = restore_commit_path_exists(&paths.failed)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;

    if current_exists && !stage_exists && !temp_stage_exists && !failed_exists {
        if validate_staged_mock_api_directory(&paths.current, journal.mode).is_ok() {
            let journal = if journal.phase == RestoreJournalPhase::CommitNewMoved {
                transition_journal(
                    paths,
                    &journal,
                    RestoreJournalPhase::StartupValidation,
                    None,
                    io,
                )?
            } else {
                journal
            };
            return Ok(RestoreStartupDisposition::StartProvisionalRestore(
                RestoreStartupToken {
                    parent: paths.parent.clone(),
                    operation_id: journal.operation_id,
                    mode: journal.mode,
                },
            ));
        }
    }

    perform_rollback(paths, journal, io)
}

fn reconcile_rollback_required(
    paths: &RestoreCommitPaths,
    journal: RestoreJournal,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    if restore_commit_path_exists(&paths.rollback)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
    {
        return perform_rollback(paths, journal, io);
    }
    let current_exists = restore_commit_path_exists(&paths.current)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    if restore_commit_path_exists(&paths.temp_stage)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
    {
        return Err(RestoreRecoveryError::ManualRecoveryRequired);
    }
    let residuals = usize::from(
        restore_commit_path_exists(&paths.stage)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?,
    ) + usize::from(
        restore_commit_path_exists(&paths.failed)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?,
    );
    if current_exists && residuals == 1 {
        validate_existing_mock_api_before_restore(&paths.current)
            .map_err(|_| RestoreRecoveryError::CurrentInvalid)?;
        transition_journal(
            paths,
            &journal,
            RestoreJournalPhase::RollbackCompleted,
            Some(RestoreJournalSafeErrorCode::CommitFailed),
            io,
        )?;
        return Ok(RestoreStartupDisposition::StartRolledBackCurrent);
    }
    Err(RestoreRecoveryError::ManualRecoveryRequired)
}

fn reconcile_rollback_completed(
    paths: &RestoreCommitPaths,
    inventory: &RestoreArtifactInventory,
    journal: &RestoreJournal,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    if restore_commit_path_exists(&paths.rollback)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        || inventory.operation_temp_stage_exists(&journal.operation_id)
    {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    validate_existing_mock_api_before_restore(&paths.current)
        .map_err(|_| RestoreRecoveryError::CurrentInvalid)?;
    Ok(RestoreStartupDisposition::StartRolledBackCurrent)
}

fn reconcile_completed(
    paths: &RestoreCommitPaths,
    inventory: &RestoreArtifactInventory,
    journal: &RestoreJournal,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    if inventory.operation_stage_exists(&journal.operation_id)
        || inventory.operation_temp_stage_exists(&journal.operation_id)
        || inventory.operation_failed_exists(&journal.operation_id)
    {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    let metadata = std_fs::symlink_metadata(&paths.current)
        .map_err(|_| RestoreRecoveryError::CurrentInvalid)?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(RestoreRecoveryError::CurrentInvalid);
    }
    Ok(RestoreStartupDisposition::StartCompletedCurrent)
}

fn perform_rollback(
    paths: &RestoreCommitPaths,
    journal: RestoreJournal,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
    let original_phase = journal.phase;
    if journal.phase != RestoreJournalPhase::RollbackRequired {
        let _ = transition_journal(
            paths,
            &journal,
            RestoreJournalPhase::RollbackRequired,
            Some(RestoreJournalSafeErrorCode::CommitFailed),
            io,
        );
    }

    let result = (|| {
        if !restore_commit_path_exists(&paths.rollback)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        {
            return Err(RestoreRecoveryError::ManualRecoveryRequired);
        }
        if restore_commit_path_exists(&paths.current)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        {
            if restore_commit_path_exists(&paths.failed)
                .map_err(|_| RestoreRecoveryError::TopologyConflict)?
            {
                return Err(RestoreRecoveryError::ManualRecoveryRequired);
            }
            io.rename_directory(
                &paths.current,
                &paths.failed,
                RestoreRecoveryRenameKind::CurrentToFailed,
            )
            .map_err(|_| RestoreRecoveryError::RollbackFailed)?;
            io.sync_parent(&paths.parent)
                .map_err(|_| RestoreRecoveryError::RollbackFailed)?;
        }
        io.rename_directory(
            &paths.rollback,
            &paths.current,
            RestoreRecoveryRenameKind::RollbackToCurrent,
        )
        .map_err(|_| RestoreRecoveryError::RollbackFailed)?;
        io.sync_parent(&paths.parent)
            .map_err(|_| RestoreRecoveryError::RollbackFailed)?;
        validate_existing_mock_api_before_restore(&paths.current)
            .map_err(|_| RestoreRecoveryError::RollbackFailed)?;
        transition_journal_from_actual_phase(
            paths,
            &journal,
            original_phase,
            RestoreJournalPhase::RollbackCompleted,
            Some(RestoreJournalSafeErrorCode::CommitFailed),
            io,
        )?;
        Ok(RestoreStartupDisposition::StartRolledBackCurrent)
    })();

    if result.is_err() {
        mark_rollback_failed(paths, &journal, io);
        return Err(RestoreRecoveryError::ManualRecoveryRequired);
    }
    result
}

fn transition_journal(
    paths: &RestoreCommitPaths,
    journal: &RestoreJournal,
    new_phase: RestoreJournalPhase,
    safe_error_code: Option<RestoreJournalSafeErrorCode>,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreJournal, RestoreRecoveryError> {
    let actual = read_recovery_journal(&paths.journal)?;
    if actual.operation_id != journal.operation_id || actual.mode != journal.mode {
        return Err(RestoreRecoveryError::OperationMismatch);
    }
    if actual.phase != journal.phase {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    let mut updated = actual;
    updated.phase = new_phase;
    updated.safe_error_code = safe_error_code;
    io.update_journal(&paths.parent, &updated)?;
    Ok(updated)
}

fn transition_journal_from_actual_phase(
    paths: &RestoreCommitPaths,
    journal: &RestoreJournal,
    original_phase: RestoreJournalPhase,
    new_phase: RestoreJournalPhase,
    safe_error_code: Option<RestoreJournalSafeErrorCode>,
    io: &dyn RestoreRecoveryIo,
) -> Result<RestoreJournal, RestoreRecoveryError> {
    let actual = read_recovery_journal(&paths.journal)?;
    if actual.operation_id != journal.operation_id || actual.mode != journal.mode {
        return Err(RestoreRecoveryError::OperationMismatch);
    }
    if actual.phase != RestoreJournalPhase::RollbackRequired && actual.phase != original_phase {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    let mut updated = actual;
    updated.phase = new_phase;
    updated.safe_error_code = safe_error_code;
    io.update_journal(&paths.parent, &updated)?;
    Ok(updated)
}

fn mark_rollback_failed(
    paths: &RestoreCommitPaths,
    journal: &RestoreJournal,
    io: &dyn RestoreRecoveryIo,
) {
    let Ok(mut actual) = read_recovery_journal(&paths.journal) else {
        return;
    };
    if actual.operation_id != journal.operation_id || actual.mode != journal.mode {
        return;
    }
    actual.phase = RestoreJournalPhase::RollbackFailed;
    actual.safe_error_code = Some(RestoreJournalSafeErrorCode::RollbackFailed);
    let _ = io.update_journal(&paths.parent, &actual);
}

fn mark_restore_startup_completed_with_io(
    token: RestoreStartupToken,
    io: &dyn RestoreRecoveryIo,
) -> Result<(), RestoreRecoveryError> {
    let _guard = RESTORE_COMMIT_MUTEX
        .lock()
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let parent = validate_restore_commit_parent(&token.parent)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let paths = derive_restore_commit_paths(parent, &token.operation_id)
        .map_err(|_| RestoreRecoveryError::OperationMismatch)?;
    let journal = read_recovery_journal(&paths.journal)?;
    if journal.operation_id != token.operation_id
        || journal.mode != token.mode
        || journal.phase != RestoreJournalPhase::StartupValidation
    {
        return Err(RestoreRecoveryError::OperationMismatch);
    }
    if !restore_commit_path_exists(&paths.current)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        || !restore_commit_path_exists(&paths.rollback)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        || restore_commit_path_exists(&paths.stage)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        || restore_commit_path_exists(&paths.temp_stage)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?
        || restore_commit_path_exists(&paths.failed)
            .map_err(|_| RestoreRecoveryError::TopologyConflict)?
    {
        return Err(RestoreRecoveryError::TopologyConflict);
    }
    transition_journal(&paths, &journal, RestoreJournalPhase::Completed, None, io)?;
    Ok(())
}

fn rollback_after_provisional_startup_failure_with_io(
    token: RestoreStartupToken,
    io: &dyn RestoreRecoveryIo,
) -> Result<(), RestoreRecoveryError> {
    let _guard = RESTORE_COMMIT_MUTEX
        .lock()
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let parent = validate_restore_commit_parent(&token.parent)
        .map_err(|_| RestoreRecoveryError::TopologyConflict)?;
    let paths = derive_restore_commit_paths(parent, &token.operation_id)
        .map_err(|_| RestoreRecoveryError::OperationMismatch)?;
    let journal = read_recovery_journal(&paths.journal)?;
    if journal.operation_id != token.operation_id
        || journal.mode != token.mode
        || journal.phase != RestoreJournalPhase::StartupValidation
    {
        return Err(RestoreRecoveryError::OperationMismatch);
    }
    match perform_rollback(&paths, journal, io)? {
        RestoreStartupDisposition::StartRolledBackCurrent => Ok(()),
        _ => Err(RestoreRecoveryError::RollbackFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as TestMutex;

    static RECOVERY_TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct SyntheticRecoveryTemp {
        path: PathBuf,
    }

    impl SyntheticRecoveryTemp {
        fn new(label: &str) -> Self {
            let sequence = RECOVERY_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rikkadesk-restore-recovery-{label}-{}-{sequence}",
                std::process::id()
            ));
            let _ = std_fs::remove_dir_all(&path);
            std_fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for SyntheticRecoveryTemp {
        fn drop(&mut self) {
            let _ = std_fs::remove_dir_all(&self.path);
        }
    }

    struct SyntheticRecoveryFixture {
        _temp: SyntheticRecoveryTemp,
        parent: PathBuf,
        operation_id: String,
        paths: RestoreCommitPaths,
        opaque_secret: Vec<u8>,
    }

    impl SyntheticRecoveryFixture {
        fn new(label: &str) -> Self {
            let temp = SyntheticRecoveryTemp::new(label);
            let parent = temp.path.join("synthetic-app-data-parent");
            std_fs::create_dir(&parent).unwrap();
            let sequence = RECOVERY_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let operation_id = format!("1700000000000-4242-{sequence}");
            let paths = derive_restore_commit_paths(parent.clone(), &operation_id).unwrap();
            Self {
                _temp: temp,
                parent,
                operation_id,
                paths,
                opaque_secret: b"opaque synthetic recovery bytes".to_vec(),
            }
        }

        fn create_current(&self) {
            create_existing_current(&self.paths.current, &self.opaque_secret);
        }

        fn create_stage(&self, mode: BackupManifestMode) {
            create_candidate(&self.paths.stage, &self.operation_id, mode);
        }

        fn write_journal(&self, phase: RestoreJournalPhase, mode: BackupManifestMode) {
            write_synthetic_journal(
                &self.paths.journal,
                &RestoreJournal {
                    format: RESTORE_JOURNAL_FORMAT.to_string(),
                    version: RESTORE_JOURNAL_VERSION,
                    operation_id: self.operation_id.clone(),
                    phase,
                    mode,
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    safe_error_code: None,
                },
            );
        }

        fn read_journal(&self) -> RestoreJournal {
            read_recovery_journal(&self.paths.journal).unwrap()
        }

        fn setup_uncommitted(&self, phase: RestoreJournalPhase) {
            self.create_current();
            self.create_stage(BackupManifestMode::StateOnly);
            self.write_journal(phase, BackupManifestMode::StateOnly);
        }

        fn setup_old_moved(&self, phase: RestoreJournalPhase) {
            self.create_current();
            self.create_stage(BackupManifestMode::StateOnly);
            std_fs::rename(&self.paths.current, &self.paths.rollback).unwrap();
            self.write_journal(phase, BackupManifestMode::StateOnly);
        }

        fn setup_committed(&self, phase: RestoreJournalPhase, mode: BackupManifestMode) {
            self.create_current();
            std_fs::rename(&self.paths.current, &self.paths.rollback).unwrap();
            create_candidate(&self.paths.current, &self.operation_id, mode);
            self.write_journal(phase, mode);
        }

        fn reconcile(&self) -> Result<RestoreStartupDisposition, RestoreRecoveryError> {
            reconcile_restore_before_start_with_io(&self.parent, &SyntheticRecoveryIo::clean())
        }
    }

    fn create_existing_current(root: &FilePath, opaque_secret: &[u8]) {
        std_fs::create_dir(root).unwrap();
        std_fs::create_dir(root.join(FILES_DIR_NAME)).unwrap();
        std_fs::create_dir(root.join(FILES_DIR_NAME).join(FILE_BLOBS_DIR_NAME)).unwrap();
        std_fs::create_dir(root.join(SECRETS_DIR_NAME)).unwrap();
        write_state(root, &default_persisted_state());
        std_fs::write(
            root.join(SECRETS_DIR_NAME).join("opaque-synthetic.bin"),
            opaque_secret,
        )
        .unwrap();
        std_fs::write(
            root.join("state.v1.pre-migration.synthetic.json"),
            b"synthetic diagnostic",
        )
        .unwrap();
    }

    fn create_candidate(root: &FilePath, operation_id: &str, mode: BackupManifestMode) {
        std_fs::create_dir(root).unwrap();
        let files_root = root.join(FILES_DIR_NAME);
        let blobs_root = files_root.join(FILE_BLOBS_DIR_NAME);
        std_fs::create_dir(&files_root).unwrap();
        std_fs::create_dir(&blobs_root).unwrap();
        std_fs::create_dir(root.join(SECRETS_DIR_NAME)).unwrap();
        let mut state = default_persisted_state();
        state.id_seq = 9_000;
        state.saved_at = 9_000;
        if mode == BackupManifestMode::FullLocalData {
            let bytes = b"synthetic recovery managed blob";
            let storage_key = format!("restore-blob-{operation_id}-0");
            state.files.push(ManagedFileMetadata {
                id: 900,
                storage_key: storage_key.clone(),
                display_name: "synthetic.png".to_string(),
                mime: "image/png".to_string(),
                size_bytes: bytes.len() as u64,
                sha256: Some(sha256_bytes(bytes)),
                kind: "image".to_string(),
                relative_path: format!("{FILES_DIR_NAME}/{FILE_BLOBS_DIR_NAME}/{storage_key}"),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                source: "upload".to_string(),
                deleted_at: None,
            });
            std_fs::write(blobs_root.join(storage_key), bytes).unwrap();
        }
        write_state(root, &state);
        validate_staged_mock_api_directory(root, mode).unwrap();
    }

    fn write_state(root: &FilePath, state: &PersistedMockState) {
        std_fs::write(
            root.join(STATE_FILE_NAME),
            serde_json::to_vec_pretty(state).unwrap(),
        )
        .unwrap();
    }

    fn write_synthetic_journal(path: &FilePath, journal: &RestoreJournal) {
        std_fs::write(path, serde_json::to_vec_pretty(journal).unwrap()).unwrap();
    }

    fn expect_provisional(disposition: RestoreStartupDisposition) -> RestoreStartupToken {
        match disposition {
            RestoreStartupDisposition::StartProvisionalRestore(token) => token,
            _ => panic!("expected provisional restore"),
        }
    }

    fn assert_rolled_back(disposition: RestoreStartupDisposition) {
        assert!(matches!(
            disposition,
            RestoreStartupDisposition::StartRolledBackCurrent
        ));
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SyntheticRecoveryFault {
        Rename(RestoreRecoveryRenameKind),
        Sync,
        Journal(RestoreJournalPhase),
        CorruptRestoredCurrent,
    }

    struct SyntheticRecoveryIo {
        fault: TestMutex<Option<SyntheticRecoveryFault>>,
        renames: TestMutex<Vec<RestoreRecoveryRenameKind>>,
        journal_updates: TestMutex<Vec<RestoreJournalPhase>>,
    }

    impl SyntheticRecoveryIo {
        fn clean() -> Self {
            Self {
                fault: TestMutex::new(None),
                renames: TestMutex::new(Vec::new()),
                journal_updates: TestMutex::new(Vec::new()),
            }
        }

        fn failing(fault: SyntheticRecoveryFault) -> Self {
            Self {
                fault: TestMutex::new(Some(fault)),
                renames: TestMutex::new(Vec::new()),
                journal_updates: TestMutex::new(Vec::new()),
            }
        }

        fn take_fault(&self, expected: SyntheticRecoveryFault) -> bool {
            let mut fault = self.fault.lock().unwrap();
            if *fault == Some(expected) {
                *fault = None;
                true
            } else {
                false
            }
        }
    }

    impl RestoreRecoveryIo for SyntheticRecoveryIo {
        fn rename_directory(
            &self,
            source: &FilePath,
            destination: &FilePath,
            kind: RestoreRecoveryRenameKind,
        ) -> io::Result<()> {
            self.renames.lock().unwrap().push(kind);
            if self.take_fault(SyntheticRecoveryFault::Rename(kind)) {
                return Err(io::Error::new(io::ErrorKind::Other, "synthetic rename"));
            }
            std_fs::rename(source, destination)?;
            if kind == RestoreRecoveryRenameKind::RollbackToCurrent
                && self.take_fault(SyntheticRecoveryFault::CorruptRestoredCurrent)
            {
                std_fs::write(
                    destination.join(STATE_FILE_NAME),
                    b"invalid synthetic state",
                )?;
            }
            Ok(())
        }

        fn sync_parent(&self, _parent: &FilePath) -> io::Result<()> {
            if self.take_fault(SyntheticRecoveryFault::Sync) {
                Err(io::Error::new(io::ErrorKind::Other, "synthetic sync"))
            } else {
                Ok(())
            }
        }

        fn update_journal(
            &self,
            parent: &FilePath,
            journal: &RestoreJournal,
        ) -> Result<(), RestoreRecoveryError> {
            self.journal_updates.lock().unwrap().push(journal.phase);
            if self.take_fault(SyntheticRecoveryFault::Journal(journal.phase)) {
                return Err(RestoreRecoveryError::CompletionFailed);
            }
            update_restore_journal_atomic(parent, journal)
                .map_err(|_| RestoreRecoveryError::CompletionFailed)
        }
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
    fn restore_recovery_no_journal_existing_current_starts() {
        let fixture = SyntheticRecoveryFixture::new("no-journal-current");
        fixture.create_current();
        assert!(matches!(
            fixture.reconcile().unwrap(),
            RestoreStartupDisposition::NoRestoreOperation
        ));
    }

    #[test]
    fn restore_recovery_no_journal_first_run_missing_current_is_normal() {
        let temp = SyntheticRecoveryTemp::new("first-run-missing-parent");
        let missing = temp.path.join("missing-app-data-parent");
        assert!(matches!(
            reconcile_restore_before_start_with_io(&missing, &SyntheticRecoveryIo::clean())
                .unwrap(),
            RestoreStartupDisposition::NoRestoreOperation
        ));
        assert!(!missing.exists());
    }

    #[test]
    fn restore_recovery_stage_only_does_not_publish() {
        let fixture = SyntheticRecoveryFixture::new("stage-only");
        fixture.create_current();
        fixture.create_stage(BackupManifestMode::StateOnly);
        fixture.reconcile().unwrap();
        assert!(fixture.paths.stage.is_dir());
        assert!(fixture.paths.current.is_dir());
    }

    #[test]
    fn restore_recovery_rollback_without_journal_blocks() {
        let fixture = SyntheticRecoveryFixture::new("rollback-no-journal");
        create_existing_current(&fixture.paths.rollback, &fixture.opaque_secret);
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[test]
    fn restore_recovery_failed_without_journal_blocks() {
        let fixture = SyntheticRecoveryFixture::new("failed-no-journal");
        create_candidate(
            &fixture.paths.failed,
            &fixture.operation_id,
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[test]
    fn restore_recovery_orphan_journal_temp_with_commit_artifact_blocks() {
        let fixture = SyntheticRecoveryFixture::new("temp-with-rollback");
        create_existing_current(&fixture.paths.rollback, &fixture.opaque_secret);
        std_fs::write(
            fixture.parent.join("restore-journal.json.tmp.4242.1"),
            b"synthetic temp",
        )
        .unwrap();
        assert!(fixture.reconcile().is_err());
    }

    #[test]
    fn restore_recovery_malformed_journal_blocks() {
        let fixture = SyntheticRecoveryFixture::new("malformed-journal");
        fixture.create_current();
        std_fs::write(&fixture.paths.journal, b"not json").unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::JournalInvalid
        );
    }

    #[test]
    fn restore_recovery_journal_unknown_field_rejected() {
        let fixture = SyntheticRecoveryFixture::new("journal-unknown-field");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let mut value: Value =
            serde_json::from_slice(&std_fs::read(&fixture.paths.journal).unwrap()).unwrap();
        value["unexpected"] = json!(true);
        std_fs::write(&fixture.paths.journal, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::JournalInvalid
        );
    }

    #[test]
    fn restore_recovery_journal_future_version_rejected() {
        let fixture = SyntheticRecoveryFixture::new("journal-future-version");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let mut value: Value =
            serde_json::from_slice(&std_fs::read(&fixture.paths.journal).unwrap()).unwrap();
        value["version"] = json!(RESTORE_JOURNAL_VERSION + 1);
        std_fs::write(&fixture.paths.journal, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::JournalUnsupported
        );
    }

    #[test]
    fn restore_recovery_journal_unknown_phase_rejected() {
        let fixture = SyntheticRecoveryFixture::new("journal-unknown-phase");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let mut value: Value =
            serde_json::from_slice(&std_fs::read(&fixture.paths.journal).unwrap()).unwrap();
        value["phase"] = json!("unknown-phase");
        std_fs::write(&fixture.paths.journal, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::JournalInvalid
        );
    }

    #[test]
    fn restore_recovery_journal_invalid_operation_id_rejected() {
        let fixture = SyntheticRecoveryFixture::new("journal-invalid-operation");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let mut value: Value =
            serde_json::from_slice(&std_fs::read(&fixture.paths.journal).unwrap()).unwrap();
        value["operationId"] = json!("../unsafe");
        std_fs::write(&fixture.paths.journal, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::JournalInvalid
        );
    }

    #[test]
    fn restore_recovery_journal_symlink_rejected() {
        let fixture = SyntheticRecoveryFixture::new("journal-symlink");
        fixture.create_current();
        let real = fixture.parent.join("synthetic-journal-target");
        std_fs::write(&real, b"synthetic").unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&real, &fixture.paths.journal).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&real, &fixture.paths.journal).is_ok();
        #[cfg(not(any(windows, unix)))]
        let linked = false;
        if linked {
            assert_eq!(
                fixture.reconcile().unwrap_err(),
                RestoreRecoveryError::JournalInvalid
            );
        }
    }

    #[test]
    fn restore_recovery_valid_journal_preserves_stale_temp() {
        let fixture = SyntheticRecoveryFixture::new("journal-stale-temp");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let temp = fixture.parent.join("restore-journal.json.tmp.4242.7");
        std_fs::write(&temp, b"synthetic stale").unwrap();
        fixture.reconcile().unwrap();
        assert!(temp.is_file());
    }

    #[test]
    fn restore_reconciliation_staging_current_unchanged_marks_rollback_completed() {
        let fixture = SyntheticRecoveryFixture::new("staging-unchanged");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        assert_rolled_back(fixture.reconcile().unwrap());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_reconciliation_backup_current_unchanged_marks_rollback_completed() {
        let fixture = SyntheticRecoveryFixture::new("backup-unchanged");
        fixture.setup_uncommitted(RestoreJournalPhase::BackupCurrent);
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.stage.is_dir());
    }

    #[test]
    fn restore_crash_stale_staging_phase_with_old_moved_restores_rollback() {
        let fixture = SyntheticRecoveryFixture::new("stale-staging-old-moved");
        fixture.setup_old_moved(RestoreJournalPhase::Staging);
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.current.is_dir());
        assert!(!fixture.paths.rollback.exists());
    }

    #[test]
    fn restore_crash_current_plus_rollback_preserves_current_as_failed() {
        let fixture = SyntheticRecoveryFixture::new("current-plus-rollback");
        fixture.setup_uncommitted(RestoreJournalPhase::BackupCurrent);
        create_existing_current(&fixture.paths.rollback, &fixture.opaque_secret);
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.failed.is_dir());
        assert!(fixture.paths.current.is_dir());
    }

    #[test]
    fn restore_crash_failed_collision_requires_manual_recovery() {
        let fixture = SyntheticRecoveryFixture::new("failed-collision");
        fixture.setup_old_moved(RestoreJournalPhase::BackupCurrent);
        create_candidate(
            &fixture.paths.current,
            &fixture.operation_id,
            BackupManifestMode::StateOnly,
        );
        create_candidate(
            &fixture.paths.failed,
            &fixture.operation_id,
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[test]
    fn restore_reconciliation_commit_old_moved_restores_rollback_not_stage() {
        let fixture = SyntheticRecoveryFixture::new("commit-old-restore");
        fixture.setup_old_moved(RestoreJournalPhase::CommitOldMoved);
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.stage.is_dir());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_crash_commit_old_with_unexpected_current_preserves_failed() {
        let fixture = SyntheticRecoveryFixture::new("commit-old-current");
        fixture.setup_old_moved(RestoreJournalPhase::CommitOldMoved);
        create_candidate(
            &fixture.paths.current,
            &fixture.operation_id,
            BackupManifestMode::StateOnly,
        );
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.failed.is_dir());
    }

    #[test]
    fn restore_recovery_commit_old_missing_rollback_blocks() {
        let fixture = SyntheticRecoveryFixture::new("commit-old-no-rollback");
        fixture.create_stage(BackupManifestMode::StateOnly);
        fixture.write_journal(
            RestoreJournalPhase::CommitOldMoved,
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[test]
    fn restore_startup_commit_new_valid_candidate_returns_token() {
        let fixture = SyntheticRecoveryFixture::new("commit-new-valid");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let _token = expect_provisional(fixture.reconcile().unwrap());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::StartupValidation
        );
    }

    #[test]
    fn restore_reconciliation_commit_new_invalid_candidate_rolls_back() {
        let fixture = SyntheticRecoveryFixture::new("commit-new-invalid");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        std_fs::write(
            fixture.paths.current.join(STATE_FILE_NAME),
            b"invalid synthetic candidate",
        )
        .unwrap();
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.failed.is_dir());
    }

    #[test]
    fn restore_crash_commit_new_missing_current_restores_rollback() {
        let fixture = SyntheticRecoveryFixture::new("commit-new-no-current");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        std_fs::remove_dir_all(&fixture.paths.current).unwrap();
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.current.is_dir());
    }

    #[test]
    fn restore_recovery_commit_new_missing_rollback_blocks() {
        let fixture = SyntheticRecoveryFixture::new("commit-new-no-rollback");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        std_fs::remove_dir_all(&fixture.paths.rollback).unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[test]
    fn restore_reconciliation_commit_new_mode_mismatch_rolls_back() {
        let fixture = SyntheticRecoveryFixture::new("commit-new-mode-mismatch");
        fixture.create_current();
        std_fs::rename(&fixture.paths.current, &fixture.paths.rollback).unwrap();
        create_candidate(
            &fixture.paths.current,
            &fixture.operation_id,
            BackupManifestMode::FullLocalData,
        );
        fixture.write_journal(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        assert_rolled_back(fixture.reconcile().unwrap());
    }

    #[test]
    fn restore_reconciliation_commit_new_with_stage_remaining_rolls_back() {
        let fixture = SyntheticRecoveryFixture::new("commit-new-stage-remains");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        fixture.create_stage(BackupManifestMode::StateOnly);
        assert_rolled_back(fixture.reconcile().unwrap());
        assert!(fixture.paths.failed.is_dir());
        assert!(fixture.paths.stage.is_dir());
    }

    #[test]
    fn restore_startup_validation_revalidates_candidate() {
        let fixture = SyntheticRecoveryFixture::new("startup-revalidate");
        fixture.setup_committed(
            RestoreJournalPhase::StartupValidation,
            BackupManifestMode::StateOnly,
        );
        let _token = expect_provisional(fixture.reconcile().unwrap());
    }

    #[test]
    fn restore_startup_validation_valid_candidate_returns_token() {
        let fixture = SyntheticRecoveryFixture::new("startup-valid-token");
        fixture.setup_committed(
            RestoreJournalPhase::StartupValidation,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        assert_eq!(token.operation_id, fixture.operation_id);
    }

    #[test]
    fn restore_startup_validation_tampered_candidate_rolls_back() {
        let fixture = SyntheticRecoveryFixture::new("startup-tampered");
        fixture.setup_committed(
            RestoreJournalPhase::StartupValidation,
            BackupManifestMode::StateOnly,
        );
        std_fs::write(fixture.paths.current.join("unexpected"), b"synthetic").unwrap();
        assert_rolled_back(fixture.reconcile().unwrap());
    }

    #[test]
    fn restore_startup_validation_old_cached_result_not_reused() {
        let fixture = SyntheticRecoveryFixture::new("startup-no-cache");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let _token = expect_provisional(fixture.reconcile().unwrap());
        std_fs::write(fixture.paths.current.join("tampered"), b"synthetic").unwrap();
        assert_rolled_back(fixture.reconcile().unwrap());
    }

    #[test]
    fn restore_reconciliation_rollback_required_restores_old() {
        let fixture = SyntheticRecoveryFixture::new("rollback-required");
        fixture.setup_committed(
            RestoreJournalPhase::RollbackRequired,
            BackupManifestMode::StateOnly,
        );
        assert_rolled_back(fixture.reconcile().unwrap());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_reconciliation_rollback_required_already_restored_advances() {
        let fixture = SyntheticRecoveryFixture::new("rollback-already-restored");
        fixture.create_current();
        fixture.create_stage(BackupManifestMode::StateOnly);
        fixture.write_journal(
            RestoreJournalPhase::RollbackRequired,
            BackupManifestMode::StateOnly,
        );
        assert_rolled_back(fixture.reconcile().unwrap());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_reconciliation_rollback_completed_valid_current_starts() {
        let fixture = SyntheticRecoveryFixture::new("rollback-completed-valid");
        fixture.create_current();
        fixture.write_journal(
            RestoreJournalPhase::RollbackCompleted,
            BackupManifestMode::StateOnly,
        );
        assert_rolled_back(fixture.reconcile().unwrap());
    }

    #[test]
    fn restore_recovery_rollback_completed_invalid_current_blocks() {
        let fixture = SyntheticRecoveryFixture::new("rollback-completed-invalid");
        fixture.create_current();
        std_fs::write(fixture.paths.current.join(STATE_FILE_NAME), b"invalid").unwrap();
        fixture.write_journal(
            RestoreJournalPhase::RollbackCompleted,
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::CurrentInvalid
        );
    }

    #[test]
    fn restore_recovery_rollback_failed_always_blocks() {
        let fixture = SyntheticRecoveryFixture::new("rollback-failed");
        fixture.create_current();
        fixture.write_journal(
            RestoreJournalPhase::RollbackFailed,
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[test]
    fn restore_reconciliation_completed_current_starts_normally() {
        let fixture = SyntheticRecoveryFixture::new("completed-current");
        fixture.create_current();
        fixture.write_journal(
            RestoreJournalPhase::Completed,
            BackupManifestMode::StateOnly,
        );
        assert!(matches!(
            fixture.reconcile().unwrap(),
            RestoreStartupDisposition::StartCompletedCurrent
        ));
    }

    #[test]
    fn restore_reconciliation_completed_allows_populated_secrets() {
        let fixture = SyntheticRecoveryFixture::new("completed-secrets");
        fixture.create_current();
        fixture.write_journal(
            RestoreJournalPhase::Completed,
            BackupManifestMode::StateOnly,
        );
        assert!(matches!(
            fixture.reconcile().unwrap(),
            RestoreStartupDisposition::StartCompletedCurrent
        ));
        assert!(fixture
            .paths
            .current
            .join(SECRETS_DIR_NAME)
            .join("opaque-synthetic.bin")
            .is_file());
    }

    #[tokio::test]
    async fn restore_startup_completed_ordinary_state_error_does_not_rollback() {
        let fixture = SyntheticRecoveryFixture::new("completed-ordinary-error");
        fixture.create_current();
        create_existing_current(&fixture.paths.rollback, &fixture.opaque_secret);
        fixture.write_journal(
            RestoreJournalPhase::Completed,
            BackupManifestMode::StateOnly,
        );
        std_fs::write(fixture.paths.current.join(STATE_FILE_NAME), b"invalid").unwrap();

        let persistence = MockPersistence::new(fixture.parent.clone());
        assert!(load_persisted_state(&persistence).await.is_err());
        assert!(fixture.paths.rollback.is_dir());
        assert_eq!(fixture.read_journal().phase, RestoreJournalPhase::Completed);
    }

    #[test]
    fn restore_reconciliation_completed_retains_rollback_snapshot() {
        let fixture = SyntheticRecoveryFixture::new("completed-retains-rollback");
        fixture.create_current();
        create_existing_current(&fixture.paths.rollback, &fixture.opaque_secret);
        fixture.write_journal(
            RestoreJournalPhase::Completed,
            BackupManifestMode::StateOnly,
        );
        fixture.reconcile().unwrap();
        assert!(fixture.paths.rollback.is_dir());
    }

    #[tokio::test]
    async fn restore_startup_provisional_load_success_writes_completed_before_ready() {
        let fixture = SyntheticRecoveryFixture::new("startup-success-completed");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );

        let state = prepare_mock_api_startup(fixture.parent.clone())
            .await
            .unwrap();

        assert_eq!(fixture.read_journal().phase, RestoreJournalPhase::Completed);
        assert!(fixture.paths.rollback.is_dir());
        assert_eq!(state.revision.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn restore_startup_completed_write_failure_blocks_ready() {
        let fixture = SyntheticRecoveryFixture::new("completion-write-failure");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        let io = SyntheticRecoveryIo::failing(SyntheticRecoveryFault::Journal(
            RestoreJournalPhase::Completed,
        ));

        assert_eq!(
            mark_restore_startup_completed_with_io(token, &io).unwrap_err(),
            RestoreRecoveryError::CompletionFailed
        );
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::StartupValidation
        );
    }

    #[test]
    fn restore_startup_stale_completion_token_rejected() {
        let fixture = SyntheticRecoveryFixture::new("stale-completion-token");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        let stale = RestoreStartupToken {
            parent: token.parent.clone(),
            operation_id: token.operation_id.clone(),
            mode: token.mode,
        };
        mark_restore_startup_completed_with_io(token, &SyntheticRecoveryIo::clean()).unwrap();

        assert_eq!(
            mark_restore_startup_completed_with_io(stale, &SyntheticRecoveryIo::clean())
                .unwrap_err(),
            RestoreRecoveryError::OperationMismatch
        );
    }

    #[test]
    fn restore_startup_journal_phase_changed_before_completion_rejected() {
        let fixture = SyntheticRecoveryFixture::new("completion-phase-changed");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        let mut journal = fixture.read_journal();
        journal.phase = RestoreJournalPhase::RollbackRequired;
        update_restore_journal_atomic(&fixture.parent, &journal).unwrap();

        assert_eq!(
            mark_restore_startup_completed_with_io(token, &SyntheticRecoveryIo::clean())
                .unwrap_err(),
            RestoreRecoveryError::OperationMismatch
        );
    }

    #[test]
    fn restore_startup_temp_stage_created_before_completion_rejected() {
        let fixture = SyntheticRecoveryFixture::new("completion-temp-stage");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        std_fs::create_dir(&fixture.paths.temp_stage).unwrap();

        assert_eq!(
            mark_restore_startup_completed_with_io(token, &SyntheticRecoveryIo::clean())
                .unwrap_err(),
            RestoreRecoveryError::TopologyConflict
        );
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::StartupValidation
        );
    }

    #[tokio::test]
    async fn restore_startup_provisional_missing_state_does_not_create_default() {
        let fixture = SyntheticRecoveryFixture::new("load-missing-no-default");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        std_fs::remove_file(fixture.paths.current.join(STATE_FILE_NAME)).unwrap();
        let persistence = MockPersistence::new(fixture.parent.clone());

        assert!(load_restore_validated_state(&persistence).await.is_err());
        assert!(!fixture.paths.current.join(STATE_FILE_NAME).exists());
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
    }

    #[tokio::test]
    async fn restore_startup_provisional_old_schema_does_not_migrate() {
        let fixture = SyntheticRecoveryFixture::new("load-old-no-migrate");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        let mut state = default_persisted_state();
        state.schema_version = FILE_METADATA_STATE_SCHEMA_VERSION;
        write_state(&fixture.paths.current, &state);
        let persistence = MockPersistence::new(fixture.parent.clone());

        assert!(load_restore_validated_state(&persistence).await.is_err());
        assert!(!std_fs::read_dir(&fixture.paths.current)
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("pre-migration")));
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
    }

    #[tokio::test]
    async fn restore_startup_provisional_future_schema_does_not_migrate() {
        let fixture = SyntheticRecoveryFixture::new("load-future-no-migrate");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        let mut state = default_persisted_state();
        state.schema_version = STATE_SCHEMA_VERSION + 1;
        write_state(&fixture.paths.current, &state);
        let persistence = MockPersistence::new(fixture.parent.clone());

        assert!(load_restore_validated_state(&persistence).await.is_err());
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
    }

    #[tokio::test]
    async fn restore_startup_provisional_malformed_state_creates_no_corrupt_backup() {
        let fixture = SyntheticRecoveryFixture::new("load-malformed-no-backup");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        std_fs::write(fixture.paths.current.join(STATE_FILE_NAME), b"invalid").unwrap();
        let persistence = MockPersistence::new(fixture.parent.clone());

        assert!(load_restore_validated_state(&persistence).await.is_err());
        assert!(!std_fs::read_dir(&fixture.paths.current)
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("corrupt")));
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
    }

    #[test]
    fn restore_crash_provisional_load_failure_moves_new_to_failed() {
        let fixture = SyntheticRecoveryFixture::new("load-failure-failed-dir");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
        assert!(fixture.paths.failed.is_dir());
    }

    #[test]
    fn restore_crash_provisional_load_failure_restores_and_validates_old() {
        let fixture = SyntheticRecoveryFixture::new("load-failure-restores-old");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
        validate_existing_mock_api_before_restore(&fixture.paths.current).unwrap();
    }

    #[test]
    fn restore_crash_provisional_load_failure_marks_rollback_completed() {
        let fixture = SyntheticRecoveryFixture::new("load-failure-journal-completed");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_crash_provisional_rollback_rename_failure_marks_failed() {
        let fixture = SyntheticRecoveryFixture::new("load-failure-rename-fails");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        let io = SyntheticRecoveryIo::failing(SyntheticRecoveryFault::Rename(
            RestoreRecoveryRenameKind::CurrentToFailed,
        ));

        assert_eq!(
            rollback_after_provisional_startup_failure_with_io(token, &io).unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_crash_provisional_failed_target_collision_blocks() {
        let fixture = SyntheticRecoveryFixture::new("load-failure-failed-collision");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        create_candidate(
            &fixture.paths.failed,
            &fixture.operation_id,
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            rollback_after_provisional_startup_failure_with_io(
                token,
                &SyntheticRecoveryIo::clean(),
            )
            .unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
    }

    #[tokio::test]
    async fn restore_startup_provisional_failure_has_no_same_process_default_retry() {
        let fixture = SyntheticRecoveryFixture::new("load-failure-no-retry");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let token = expect_provisional(fixture.reconcile().unwrap());
        std_fs::remove_file(fixture.paths.current.join(STATE_FILE_NAME)).unwrap();
        let persistence = MockPersistence::new(fixture.parent.clone());
        assert!(load_restore_validated_state(&persistence).await.is_err());
        assert!(!fixture.paths.current.join(STATE_FILE_NAME).exists());
        rollback_after_provisional_startup_failure_with_io(token, &SyntheticRecoveryIo::clean())
            .unwrap();
        assert_ne!(
            std_fs::read(fixture.paths.current.join(STATE_FILE_NAME)).unwrap(),
            serde_json::to_vec_pretty(&default_persisted_state()).unwrap()
        );
    }

    #[tokio::test]
    async fn restore_startup_normal_first_run_still_initializes_default() {
        let temp = SyntheticRecoveryTemp::new("normal-first-run-default");
        let parent = temp.path.join("new-app-data-parent");
        std_fs::create_dir(&parent).unwrap();
        let disposition = reconcile_restore_before_start(&parent).await.unwrap();
        assert!(matches!(
            disposition,
            RestoreStartupDisposition::NoRestoreOperation
        ));
        let persistence = MockPersistence::new(parent);
        let loaded = load_persisted_state(&persistence).await.unwrap();
        assert_eq!(loaded.outcome, StateLoadOutcome::InitializedDefault);
    }

    #[tokio::test]
    async fn restore_startup_ambiguous_recovery_prevents_state_write() {
        let fixture = SyntheticRecoveryFixture::new("ambiguous-no-state-write");
        create_existing_current(&fixture.paths.rollback, &fixture.opaque_secret);
        assert!(prepare_mock_api_startup(fixture.parent.clone())
            .await
            .is_err());
        assert!(!fixture.paths.current.exists());
    }

    #[test]
    fn restore_startup_ambiguous_recovery_prevents_listener_preparation() {
        let fixture = SyntheticRecoveryFixture::new("ambiguous-no-ready");
        fixture.create_current();
        fixture.write_journal(
            RestoreJournalPhase::RollbackFailed,
            BackupManifestMode::StateOnly,
        );
        assert!(fixture.reconcile().is_err());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[tokio::test]
    async fn restore_startup_completed_written_only_after_state_ownership() {
        let fixture = SyntheticRecoveryFixture::new("ownership-before-completed");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let state = prepare_mock_api_startup(fixture.parent.clone())
            .await
            .unwrap();
        assert_eq!(state.id_seq.load(Ordering::Relaxed), 9_000);
        assert_eq!(fixture.read_journal().phase, RestoreJournalPhase::Completed);
    }

    #[tokio::test]
    async fn restore_startup_later_listener_failure_does_not_rollback_completed_data() {
        let fixture = SyntheticRecoveryFixture::new("listener-failure-no-rollback");
        fixture.setup_committed(
            RestoreJournalPhase::CommitNewMoved,
            BackupManifestMode::StateOnly,
        );
        let _state = prepare_mock_api_startup(fixture.parent.clone())
            .await
            .unwrap();
        let synthetic_listener_result: io::Result<()> = Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "synthetic listener failure",
        ));
        assert!(synthetic_listener_result.is_err());
        assert_eq!(fixture.read_journal().phase, RestoreJournalPhase::Completed);
        assert!(fixture.paths.rollback.is_dir());
    }

    #[test]
    fn restore_reconciliation_multiple_operation_ids_block() {
        let fixture = SyntheticRecoveryFixture::new("multiple-operation-ids");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let other = fixture
            .parent
            .join("mock-api.restore-stage.1700000000000-4242-999999");
        create_candidate(
            &other,
            "1700000000000-4242-999999",
            BackupManifestMode::StateOnly,
        );
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::OperationMismatch
        );
    }

    #[test]
    fn restore_reconciliation_case_folded_collision_blocks() {
        let fixture = SyntheticRecoveryFixture::new("case-folded-collision");
        create_existing_current(&fixture.parent.join("MOCK-API"), &fixture.opaque_secret);
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::TopologyConflict
        );
    }

    #[test]
    fn restore_reconciliation_concurrent_attempts_serialize() {
        let fixture = SyntheticRecoveryFixture::new("concurrent-reconciliation");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| fixture.reconcile());
            let second = scope.spawn(|| fixture.reconcile());
            (first.join().unwrap(), second.join().unwrap())
        });
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackCompleted
        );
    }

    #[test]
    fn restore_reconciliation_commit_and_recovery_share_mutex() {
        let guard = RESTORE_COMMIT_MUTEX.lock().unwrap();
        assert!(RESTORE_COMMIT_MUTEX.try_lock().is_err());
        drop(guard);
        assert!(RESTORE_COMMIT_MUTEX.try_lock().is_ok());
    }

    #[test]
    fn restore_reconciliation_unrelated_directories_untouched() {
        let fixture = SyntheticRecoveryFixture::new("unrelated-untouched");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let unrelated = fixture.parent.join("synthetic-unrelated");
        std_fs::create_dir(&unrelated).unwrap();
        std_fs::write(unrelated.join("marker"), b"unchanged").unwrap();
        fixture.reconcile().unwrap();
        assert_eq!(
            std_fs::read(unrelated.join("marker")).unwrap(),
            b"unchanged"
        );
    }

    #[cfg(windows)]
    #[test]
    fn restore_recovery_never_opens_opaque_secret_fixture() {
        use std::os::windows::fs::OpenOptionsExt;

        let fixture = SyntheticRecoveryFixture::new("opaque-read-probe");
        fixture.setup_uncommitted(RestoreJournalPhase::Staging);
        let secret_path = fixture
            .paths
            .current
            .join(SECRETS_DIR_NAME)
            .join("opaque-synthetic.bin");
        let _exclusive = std_fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(secret_path)
            .unwrap();
        assert_rolled_back(fixture.reconcile().unwrap());
    }

    #[test]
    fn restore_recovery_rollback_preserves_opaque_fixture() {
        let fixture = SyntheticRecoveryFixture::new("opaque-rollback-preserved");
        fixture.setup_committed(
            RestoreJournalPhase::RollbackRequired,
            BackupManifestMode::StateOnly,
        );
        fixture.reconcile().unwrap();
        assert_eq!(
            std_fs::read(
                fixture
                    .paths
                    .current
                    .join(SECRETS_DIR_NAME)
                    .join("opaque-synthetic.bin")
            )
            .unwrap(),
            fixture.opaque_secret
        );
    }

    #[test]
    fn restore_recovery_errors_contain_no_secret_filename_or_content() {
        let fixture = SyntheticRecoveryFixture::new("safe-secret-errors");
        let errors = [
            RestoreRecoveryError::JournalInvalid,
            RestoreRecoveryError::JournalUnsupported,
            RestoreRecoveryError::TopologyConflict,
            RestoreRecoveryError::OperationMismatch,
            RestoreRecoveryError::CurrentInvalid,
            RestoreRecoveryError::CandidateInvalid,
            RestoreRecoveryError::RollbackFailed,
            RestoreRecoveryError::StartupLoadFailed,
            RestoreRecoveryError::CompletionFailed,
            RestoreRecoveryError::ManualRecoveryRequired,
        ];
        assert!(errors.iter().all(|error| {
            let message = error.to_string();
            !message.contains("opaque-synthetic.bin")
                && !message.contains("opaque synthetic recovery bytes")
                && !message.contains("secretRef")
        }));
        drop(fixture);
    }

    #[test]
    fn restore_recovery_errors_contain_no_path_operation_or_reference() {
        let fixture = SyntheticRecoveryFixture::new("safe-path-errors");
        let path = fixture.parent.to_string_lossy().into_owned();
        let operation_id = fixture.operation_id.clone();
        let errors = [
            RestoreRecoveryError::JournalInvalid,
            RestoreRecoveryError::JournalUnsupported,
            RestoreRecoveryError::TopologyConflict,
            RestoreRecoveryError::OperationMismatch,
            RestoreRecoveryError::CurrentInvalid,
            RestoreRecoveryError::CandidateInvalid,
            RestoreRecoveryError::RollbackFailed,
            RestoreRecoveryError::StartupLoadFailed,
            RestoreRecoveryError::CompletionFailed,
            RestoreRecoveryError::ManualRecoveryRequired,
        ];
        assert!(errors.iter().all(|error| {
            let message = error.to_string();
            !message.contains(&path)
                && !message.contains(&operation_id)
                && !message.contains("storageKey")
        }));
    }

    #[test]
    fn restore_recovery_failure_never_creates_default_state() {
        let fixture = SyntheticRecoveryFixture::new("failure-no-default");
        fixture.write_journal(
            RestoreJournalPhase::RollbackFailed,
            BackupManifestMode::StateOnly,
        );
        assert!(fixture.reconcile().is_err());
        assert!(!fixture.paths.current.exists());
    }

    #[test]
    fn restore_crash_rollback_sync_failure_marks_failed() {
        let fixture = SyntheticRecoveryFixture::new("rollback-sync-failure");
        fixture.setup_committed(
            RestoreJournalPhase::RollbackRequired,
            BackupManifestMode::StateOnly,
        );
        let io = SyntheticRecoveryIo::failing(SyntheticRecoveryFault::Sync);
        assert_eq!(
            reconcile_restore_before_start_with_io(&fixture.parent, &io).unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_crash_restored_current_validation_failure_marks_failed() {
        let fixture = SyntheticRecoveryFixture::new("restored-current-invalid");
        fixture.setup_old_moved(RestoreJournalPhase::CommitOldMoved);
        let io = SyntheticRecoveryIo::failing(SyntheticRecoveryFault::CorruptRestoredCurrent);
        assert_eq!(
            reconcile_restore_before_start_with_io(&fixture.parent, &io).unwrap_err(),
            RestoreRecoveryError::ManualRecoveryRequired
        );
        assert_eq!(
            fixture.read_journal().phase,
            RestoreJournalPhase::RollbackFailed
        );
    }

    #[test]
    fn restore_recovery_journal_size_limit_blocks() {
        let fixture = SyntheticRecoveryFixture::new("journal-size-limit");
        fixture.create_current();
        std_fs::write(
            &fixture.paths.journal,
            vec![b'x'; RESTORE_JOURNAL_MAX_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::JournalInvalid
        );
    }

    #[test]
    fn restore_recovery_malformed_journal_temp_name_blocks() {
        let fixture = SyntheticRecoveryFixture::new("bad-journal-temp-name");
        fixture.create_current();
        std_fs::write(
            fixture.parent.join("restore-journal.json.tmp.bad.name"),
            b"synthetic",
        )
        .unwrap();
        assert_eq!(
            fixture.reconcile().unwrap_err(),
            RestoreRecoveryError::TopologyConflict
        );
    }
}
