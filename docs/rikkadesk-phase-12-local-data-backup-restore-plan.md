# RikkaDesk Phase 12 Local Data Backup, Restore, And Migration Safety Plan

This document began as the Phase 12 P0 audit and design plan and is now the living implementation and status plan for local persistence, backup, restore, corruption recovery, schema migration, managed file blobs, and Windows DPAPI secret handling.

The original P0 audit was documentation only. Later sections record the implemented mutation hardening, portable Mode A/Mode B backup export, no-write restore validation, offline staging, and the internal journaled directory commit/rollback boundary.

## Current Status

- Original Phase 12 audit baseline commit: `6767eb841169db63b047ed280b6c1a2f3ca4b349`.
- Stable runtime tag: `rikkadesk-v0.1.0-beta.14`.
- App/package version: `0.1.0`.
- State filename: `state.v1.json`.
- State payload schema: `schemaVersion: 6`.
- Provider import/export version: `4`.
- Windows artifacts: unsigned private beta.
- GitHub Release: No.
- Real-provider image input: not enabled by default.
- Runtime mutation hardening: P1-C1 pure state, P1-C2 Provider/SecretStore, P1-C3 managed file/blob handled-failure semantics, and P1-C4 streaming lifecycle semantics are completed.
- Portable backup export: backup package format v1 Mode A (`state-only`) and Mode B (`full-local-data`) are implemented without secrets.
- Restore validation: P2-B no-write dry-run validation is implemented; it does not authorize or perform restore.
- Actual restore: not user-consumable. P2-C0 defines the offline transaction protocol; P2-C1 candidate staging and P2-C2 journaled commit/handled rollback are implemented internally; P2-C3 startup validation and interrupted-operation reconciliation remain required.

## P0 Decisions

1. Mode B, a full local data backup without secrets, should become the default portable backup format.
2. Mode A, metadata-only backup, is useful when conversation text and settings matter more than attachment availability.
3. Mode C, same-machine secure backup including encrypted secret blobs, should remain deferred until its threat model, consent, and recovery semantics are independently approved.
4. A directory copy made while RikkaDesk is running is not a consistent backup.
5. Restore must use a validated staging area, an automatic pre-restore backup, an explicit commit step, and rollback. It must never overwrite current app data directly.
6. Phase 12 should start implementation with P1 atomic state backup/write primitives before adding packaging or UI.

## P1-A Implementation Status

Phase 12 P1-A hardens the ordinary `state.v1.json` write path without changing the state schema, load/migration decisions, backup formats, or managed blob behavior.

Implemented behavior:

- All ordinary full-state saves are serialized by one process-local persistence mutex.
- P1-A's atomic writer still owns the one save mutex and one unique-temp/replace implementation. P1-C1 runtime transactions now build an explicit staged snapshot under the mutation mutex and pass that snapshot to the same writer; the persistence layer does not re-read live state for staged transactions.
- JSON serialization completes before any filesystem operation touches the primary state file.
- Every save uses a unique same-directory temp name in the form `state.v1.json.tmp.<pid>.<counter>` and creates it with create-new semantics.
- The writer performs `write_all`, `flush`, and `sync_all`, closes the temp handle, and only then commits it.
- On Windows, an existing primary state is replaced with `ReplaceFileW`; a first save uses a same-directory rename. Neither path deletes the primary state first.
- On non-Windows platforms, the same-directory rename path is used without a pre-delete.
- Serialization, temp create/write/flush/sync, and replacement failures preserve the previous primary state and best-effort remove only the temp file created by that save.
- State mutation handlers await the persistence result and return a safe HTTP 5xx response on failure instead of reporting success.
- Streaming completion cannot return an HTTP response, so P1-C4 emits a fixed safe `failed` terminal after a final persistence failure. It does not emit `finished`, and no state content, message, provider config, secret, local path, request body, or delta is logged.

P1-C1 removes live-ahead-of-disk behavior for the pure settings and conversation mutations listed below. P1-C2 extends staged state commits to Provider metadata and coordinates them with copy-on-write SecretStore operations. P1-C3 coordinates managed blob publication/deletion with staged file metadata. P1-C4 commits the user before provider/mock work, keeps deltas transient, and stages the final assistant append/replace before any terminal success event.

P1-A does not claim complete power-loss protection, a transactional boundary between state and managed blobs, backup/restore support, or complete corruption recovery.

## P1-B Implementation Status

Phase 12 P1-B makes startup state loading and schema migration fail closed. It does not add recovery UI or automatically repair damaged state.

Implemented behavior:

- The loader reads the exact primary `state.v1.json` path directly and distinguishes `NotFound` from every other I/O error. The old check-then-read `exists()` flow is removed.
- Only `NotFound` initializes schema v6 default state. Initialization is reported successful only after the P1-A atomic persistence helper succeeds.
- Permission, sharing, locking, transient, and other read failures stop mock API startup. They do not create a default state or a corrupt backup.
- Malformed JSON or an invalid current/legacy state shape is copied byte-for-byte to a unique same-directory `state.v1.corrupt.<millis>.<pid>.<counter>.json` file.
- Corrupt backups use create-new semantics followed by `write_all`, `flush`, and `sync_all`. The original primary state remains in place. Successful preservation returns a recovery-required startup error; backup failure stops startup without writing defaults.
- A numeric `schemaVersion` greater than 6 returns a dedicated newer-schema compatibility error. Future state is not labeled corrupt, backed up as corrupt, migrated, reset, or modified.
- Schema 1-5 migrations first preserve the original bytes in a unique `state.v1.pre-migration.v<from>-to-v6.<millis>.<pid>.<counter>.json` file.
- Migration backup, in-memory transform, result validation, and P1-A atomic persistence run in that order. Any failure stops startup; the old primary remains and a completed pre-migration backup is retained.
- Migration validation requires schema v6 output, object-shaped settings, normalized TEXT/IMAGE input capabilities, TEXT output, and preservation of schema v5 file metadata.
- Strictly named P1-A stale temp files are detected and ignored. They are never promoted, parsed as primary state, deleted, or allowed to replace the primary state.
- The automatic `reset_persisted_state` path is removed. No non-NotFound load failure writes default state.
- Startup errors use fixed stages and schema numbers only. State bytes, user content, provider data, secret references, and local paths are not logged.

P1-B does not provide a recovery UI. A user encountering corrupt state receives a startup failure while the original and durable corrupt backup remain available for a later explicit recovery workflow. It also does not provide state/blob transactions, formal backup packages, portable DPAPI backup, or cross-resource mutation compensation.

## P1-C3 Implementation Status

Phase 12 P1-C3 hardens handled managed file upload/delete failures without changing schema v6, the file API shape, or backup formats.

Implemented behavior:

- One file/blob transaction mutex serializes managed upload, delete, path reads, and provider-image blob reads without holding component locks or the global state mutation mutex across blob I/O.
- The accepted upload batch is validated first, then every blob is written to a unique create-new same-directory temp, flushed, synced, closed, and published to a non-existing program-generated final name.
- Blob storage keys are independent of numeric file IDs. File IDs and `id_seq` advance only in one staged file-metadata transaction after every batch blob is published.
- Blob write/publish failure commits no metadata and removes prior operation-created blobs. State persistence failure leaves live/disk metadata, revision, and `id_seq` unchanged and compensates all new final blobs with bounded retry.
- Exhausted upload compensation returns a fixed safe failure and can leave only an unreferenced blob with no API-accessible metadata.
- Ordinary DELETE scans numeric `metadata.fileId` references in persisted messages. A referenced file returns conflict. An unreferenced file is durably tombstoned before physical cleanup.
- Tombstone persistence failure leaves the active metadata and blob unchanged. Cleanup failure after commit returns logical success, leaves GET/path access blocked, emits a fixed redacted warning, and is retried by repeated DELETE.
- Message/conversation deletion does not automatically remove attachment blobs. Automatic orphan discovery, reconciliation, and GC remain deferred.

P1-C3 still does not make JSON state and blob storage crash-atomic. A process crash after final blob publication but before metadata commit can leave an unreferenced blob. A crash after durable tombstone commit but before physical deletion can leave an inaccessible physical blob. These residuals are inputs to the later reconciliation design, not evidence that backup/restore is implemented.

## P1-C4 Implementation Status

Phase 12 P1-C4 hardens send, regenerate, stop, mock fallback, provider streaming, and background finalization without changing schema v6, Provider import/export v4, provider protocols, or backup formats.

Implemented behavior:

- Initial conversation creation and the user message are one staged transaction. Persistence failure returns a safe HTTP failure, leaves live/disk/revision unchanged, registers no surviving generation, emits no start/success event, and starts no provider/mock task.
- Active generations are runtime-only and contain a monotonic generation ID, operation, phase, and cancellation handle. They do not persist prompts, deltas, request bodies, secrets, or provider responses.
- One conversation accepts at most one generation. Different conversations can stream concurrently. Token ownership prevents stale tasks from committing or deleting a newer registry entry.
- Provider/mock configuration is resolved only after the durable user commit and without holding persisted component, commit-barrier, mutation, file, or Provider transaction locks across network waits.
- Streaming text is task-local. `delta` and compatibility snapshots are explicitly transient; live persisted conversations and `state.v1.json` contain no partial assistant reply.
- Provider/mock completion stages the final assistant append, persists it, commits live, then emits the committed snapshot and `finished`. Final persistence failure retains only the durable user turn, emits fixed `failed: persistence`, and never emits `finished`.
- Regenerate keeps the old assistant reply until a final staged transaction atomically replaces it. Provider failure, persistence failure, or a changed/deleted source/target preserves user edits and prevents stale overwrite.
- Stop uses the discard-partial policy. It cancels stream reads, drops transient text, persists the stopped durable state, and emits exactly one `stopped`; persistence failure emits `failed` instead.
- Conversation delete commits first, cancels/removes the matching runtime token, and prevents an old finalizer from recreating the conversation. Category A mutations during a stream are retained by the final staged snapshot.
- Runtime restart intentionally drops active generations and transient assistant text. The durable user turn remains; stream resume and automatic request retry are not implemented.
- P1-C4 adds 32 synthetic transaction, registry, event-order, regenerate, stop, restart, and loopback-network tests. No real app data, encrypted secret blob, API key, user file, or real provider is used.

P1-C4 does not make state and an in-flight network request atomic, persist every delta, resume streams across restart, retry provider requests, implement backup/restore, or eliminate the documented SecretStore/blob crash-only orphan windows.

## P0 Persistence Baseline (Before P1-A)

The following audit records the implementation that P1-A replaced. It remains here as the rationale and risk baseline; statements in this section describing fixed temp names, pre-delete, missing sync, ignored save errors, or no save mutex are historical rather than current behavior.

### App Data And State Path

`web-ui/src-tauri/src/main.rs` obtains the Tauri app data directory with `app.path().app_data_dir()` and passes it to `mock_api::start`.

`MockPersistence::new` appends:

```text
mock-api/state.v1.json
```

On Windows, the documented effective layout is:

```text
%APPDATA%\com.cisyamx.rikkadesk\mock-api\state.v1.json
%APPDATA%\com.cisyamx.rikkadesk\mock-api\files\blobs\<storageKey>
%APPDATA%\com.cisyamx.rikkadesk\mock-api\secrets\<encoded-secret-ref>.bin
```

The API accepts file IDs, not filesystem paths. `ManagedBlobStore::final_path` validates `storageKey` and constructs blob paths under the managed blob directory.

The current startup path logs the full state path to stderr. Handoff, backup, and restore diagnostics must therefore exclude raw logs unless a human has reviewed them for local usernames and paths.

### Persisted State Shape

`PersistedMockState` contains:

- `schemaVersion`
- `savedAt`
- `idSeq`
- settings
- conversations and messages
- provider non-sensitive configuration, including `secretRef`
- managed file metadata

It does not contain API keys or managed file bytes. Managed file content and encrypted secret blobs are stored separately.

### Load Timing

State is loaded once during mock API startup, before the Axum router begins serving requests.

Current load behavior:

| Condition | Current behavior |
|---|---|
| State file is missing | Create default schema v6 state and attempt to save it. |
| Read succeeds and schema is 6 | Normalize providers/settings and use it in memory. |
| Read succeeds and schema is 1-5 | Migrate in memory to v6, then attempt to save migrated state. |
| JSON is invalid | Treat as corrupt, attempt corrupt backup, create and save default state. |
| Schema is unsupported, including a future version | Treat as corrupt, attempt corrupt backup, create and save default state. |
| State read fails for another reason | Log the read error, create default state, and attempt to save it without first creating a corrupt backup. |

There is no user-facing recovery notice or restore choice. Errors and reset reasons are written only to stderr.

### Save Timing

Most state-changing endpoints mutate in-memory state and immediately call `persist_mock_state`, including provider import/upsert/delete, conversation/message operations, settings changes, file upload metadata, and file deletion metadata.

Streaming behavior is less granular:

- The empty streaming assistant message is persisted when streaming starts.
- Text deltas update only in-memory conversation state and are broadcast to the UI.
- The final message is persisted when streaming finishes.
- A forced shutdown during streaming can lose partial text and can leave the last persisted conversation marked as generating.

### Persistence Helper

`persist_mock_state` is the common full-state persistence helper. It:

1. Clones settings.
2. Clones conversations.
3. Clones providers.
4. Clones managed file metadata.
5. Recomputes `idSeq` bounds.
6. Calls `MockPersistence::save`.

Each component has an independent `tokio::sync::RwLock`. The helper acquires those read locks one at a time rather than taking one coherent state snapshot.

### Current Write Sequence

`MockPersistence::save` currently:

1. Creates the state directory.
2. Serializes the full state to pretty JSON in memory.
3. Writes a fixed `state.v1.json.tmp` file.
4. Removes the existing `state.v1.json` if present.
5. Renames the temp file to `state.v1.json`.

This is temp-write-then-remove-then-rename. It is not a safe atomic replace.

### Atomicity And Durability

Current conclusion:

- Atomic write: No.
- Direct overwrite: not a direct byte-for-byte write, but the implementation explicitly deletes the current state before rename, creating a no-state window.
- `flush`: not explicitly called.
- `sync_all` / `fsync`: not called for the temp file or parent directory.
- Unique temp name: No; every save uses `state.v1.json.tmp`.
- Windows replace primitive: No `ReplaceFileW`, `MoveFileExW` replace/write-through mode, or equivalent is used.

Failure windows include:

- Temp write fails and leaves a partial or stale temp file.
- Original removal fails and leaves the temp file behind.
- Original removal succeeds but rename fails, leaving no primary state file.
- Power loss occurs after original removal and before rename.
- Rename completes but file contents were not durably flushed before power loss.

### Concurrency And Locking

Current conclusion:

- In-memory fields use independent `RwLock` values.
- There is no global persistence mutex.
- There is no save generation, journal, compare-and-swap, or single writer queue.
- Concurrent saves use the same temp path.
- A full persisted snapshot is assembled from fields read at different moments.
- API mutations are not held in one transaction with their save.

Two concurrent save calls can interfere with the same temp file or replace each other's output. Because errors are swallowed by `persist_mock_state`, an API request can report success even when its state was not durably saved.

### Save Failure Behavior

`persist_mock_state` returns no result to callers. It logs save errors and allows the request flow to continue. Consequences:

- The UI generally receives success after an in-memory mutation even if disk persistence failed.
- A later save may persist the in-memory state, but that is not guaranteed before shutdown.
- A crash or forced close can silently discard acknowledged changes.
- There is no dirty-state indicator, retry queue, user notification, or shutdown flush contract.

Silent data loss risk exists and is material.

## Existing Corrupt-State Handling

### Corrupt Backup

A corrupt backup already exists for JSON parse failures and unsupported schema versions.

Naming format:

```text
state.v1.corrupt.<unix-milliseconds>.json
```

The backup is created by renaming the primary state file in the same directory. It is not copied and it has no manifest or checksum.

### Backup Conditions

`backup_corrupt_state` is attempted when:

- JSON cannot deserialize into `PersistedMockState`.
- `schemaVersion` is not one of 1, 2, 3, 4, 5, or 6.
- `schemaVersion` is missing, because the required field causes deserialization to fail.

It is not attempted when reading the state file itself fails before parsing.

### Backup Failure

If corrupt backup rename fails, the error is logged but reset continues. The default-state save can then remove and replace the original file. Therefore backup failure does not protect the original state from overwrite.

### User Visibility And Reset Risk

- Corruption/reset is reported only through stderr.
- The app silently continues with a default welcome state from the user's perspective.
- There is no UI warning, recovery path, or link to the corrupt backup.
- A future schema is treated as corrupt rather than as a non-destructive compatibility error.
- Read errors can result in default-state save without preserving the unreadable original.

Current conclusions:

- Corrupt backup exists: Yes, for parse/unsupported-schema paths.
- Backup is guaranteed before reset: No.
- Silent reset risk: Yes.
- Silent data loss risk: Yes.

## Existing Schema Migration Behavior

The filename remains `state.v1.json`; payload evolution is controlled by `schemaVersion`.

Current code accepts versions 1 through 6:

| Source schema | Migration to v6 | Main behavior |
|---:|---|---|
| 1 | `migrate_v1_to_v6` | Preserve base state, set v6/timestamp, clear providers and files. |
| 2 | `migrate_v2_to_v6` | Preserve provider state, normalize provider models/modalities, clear files. |
| 3 | `migrate_v3_to_v6` | Preserve multi-model providers, normalize models/modalities, clear files. |
| 4 | `migrate_v4_to_v6` | Preserve custom request configuration through serde/defaults, normalize providers, clear files. |
| 5 | `migrate_v5_to_v6` | Preserve managed file metadata and providers, normalize model modalities. |
| 6 | No migration | Normalize providers/settings and use current state. |

Serde defaults supply fields missing from older shapes. After migration, settings are synchronized with desktop providers and the current model is validated.

Migration safety gaps:

- No pre-migration backup is created.
- Migration save uses the same non-atomic remove-and-rename helper.
- Migration failure is logged, but migrated in-memory state is still used.
- A failed migration save can leave disk state old, missing, or partially recoverable while the running process uses v6 in memory.
- There is no migration journal or rollback marker.

### Unsupported Future Schema

A future schema is renamed as `corrupt` and replaced with default state. The safe behavior should instead be a non-destructive compatibility stop that preserves the primary state and tells the user a newer build is required.

### Missing Schema Version

Because `schemaVersion` is required, a missing field is treated as invalid JSON shape, then follows corrupt backup/reset behavior.

### Downgrade Risk

The current checklist correctly warns that beta.11 or earlier builds must not open schema v5/v6 app data. Downgrade can:

- Classify a newer state as corrupt or unsupported.
- Rename it to a corrupt backup.
- Create a default older state.
- Lose or ignore fields unknown to the older build.
- Break state/blob metadata consistency.

Before any downgrade test, use synthetic app data or create a verified backup while RikkaDesk is fully closed. Never point an old build at the only copy of newer-schema app data.

## SecretStore And Windows DPAPI Boundary

### State Reference Model

Provider state stores a non-secret `secretRef`, generated in the form:

```text
rikkadesk:provider:<provider-id>:api-key
rikkadesk:provider:<provider-id>:api-key:<millis>:<process>:<sequence>
```

The first form is the deterministic empty/legacy reference. P1-C2 key create/update uses the second copy-on-write form so an active encrypted blob is never overwritten before the corresponding Provider state commit.

On Windows, the reference is encoded into a safe filename and mapped to:

```text
mock-api/secrets/<encoded-secret-ref>.bin
```

The blob contains DPAPI-protected bytes, not plaintext. `state.v1.json` does not contain the API key.

### Provider Secret Mutation And Deletion

P1-C2 serializes all Provider mutations with a Provider/SecretStore mutex and commits Provider metadata through the P1-C1 staged-state helper:

- New/replacement keys are written to a new `secretRef`; state failure compensates by deleting only that operation-created blob.
- Blank-key upsert retains the existing ref and performs no SecretStore write.
- Clear commits a new empty ref before deleting the old encrypted blob.
- Provider delete commits Provider/settings removal before deleting the old encrypted blob.
- Import confirm ignores source `hasSecret`, creates no key/blob, and never restores a source `secretRef`.

Handled persistence failure leaves the prior live/durable Provider state and prior secret intact. Old-blob cleanup failure after a committed clear/update/delete returns logical success with a fixed redacted warning; the old blob is no longer referenced and remains an encrypted orphan. A process crash can also leave an orphan between new-blob preparation and state commit, or between state commit and old-blob cleanup. State and DPAPI/keyring storage are therefore not claimed to be fully atomic. Startup orphan reconciliation is deferred.

On Windows, `hasSecret` existence checks use file metadata and do not read/decrypt the blob. Actual provider use still calls the controlled SecretStore read and DPAPI unprotect path.

### Backup Combinations

State without secret blobs:

- Provider non-sensitive configuration is restored, but package/source `secretRef` values are never treated as usable and are replaced for a future actual restore.
- Secret lookup reports missing.
- The provider key must be entered again.
- `secretRef` is an identifier, not secret material, but it does not restore credentials.

Secret blobs without state:

- No provider configuration refers to the blobs.
- The blobs are orphaned and are not automatically discovered.
- They must not be treated as a usable credential backup by themselves.

### DPAPI Portability

The Windows implementation calls `CryptProtectData` without machine-scope flags, so it uses Windows user-scope DPAPI behavior. Recovery is tied to the Windows security context/profile that protected the data and is not a portable cross-machine format.

Conclusions:

- Same machine and same Windows user/profile: best-effort recovery may work.
- Different Windows user: decryption should be expected to fail.
- Different machine/profile: decryption is not guaranteed and should be expected to fail for backup-design purposes.
- DPAPI failure returns a safe SecretStore error; the blob is not automatically deleted and plaintext is not returned.
- Encrypted secret blobs cannot be represented as portable backup.

Default full backup must exclude `mock-api/secrets/*.bin`. A same-machine secret backup can only be an explicit opt-in mode with a strong warning and should remain deferred in the current phase.

Secret blobs and any app data backup must never be committed, attached to an issue, uploaded to GitHub, or sent to Codex / ChatGPT.

## Managed File Blob Boundary

### Metadata And Blob Mapping

Managed file metadata is stored in `PersistedMockState.files` and includes:

- numeric file ID
- `storageKey`
- display name
- MIME type
- byte size
- optional SHA256, currently `None` for uploaded files
- kind
- relative path
- timestamps/source/deleted marker

Blob bytes are stored separately under `mock-api/files/blobs`. Runtime blob lookup validates `storageKey` and constructs the internal path; the API URL uses the numeric file ID.

### Upload And Delete Ordering

Upload after P1-C3:

1. Validates the complete accepted batch and publishes every blob through a unique create-new, flush/sync/close temp path.
2. Allocates numeric file IDs and appends all metadata in one staged state transaction.
3. Persists the staged snapshot, commits live metadata, and only then returns the batch.
4. Compensates operation-created blobs if publication or metadata persistence fails.

Handled state persistence failure does not publish metadata and normally removes all new blobs. Cleanup exhaustion or a process crash between blob publication and metadata commit can still leave an unreferenced, API-inaccessible orphan.

Delete after P1-C3:

1. Rejects the operation if any persisted message part has the numeric `metadata.fileId` reference.
2. Persists and commits `deletedAt` for an unreferenced file.
3. Deletes the physical blob only after the tombstone is durable and live.

If tombstone persistence fails, the active metadata and original blob remain. If post-commit cleanup fails or the process stops between steps 2 and 3, the tombstone blocks all API reads while the physical blob remains an inaccessible orphan.

There is still no startup orphan scan, automatic orphan cleanup, or full state/blob crash transaction. P1-C3 provides handled-failure compensation and tombstone semantics; reconciliation remains deferred.

### Partial Backup And Restore Cases

#### A. State Only, No Blobs

- Conversations and attachment parts can be restored.
- Managed file metadata and file IDs remain.
- Image requests return missing/unavailable and the image UI falls back safely.
- TXT/PDF document chips remain non-executable managed links, but opening them can return 404.
- The UI should not crash, but attachments are not usable.

#### B. Blobs Only, No State

- Blobs have no active metadata reference.
- They become orphans.
- The current app does not discover or display them.
- They remain on disk until manually removed or a future orphan-cleanup policy exists.

#### C. State And Blobs From Different Times

- Metadata can point to missing or older blobs.
- Deleted blobs can be restored while metadata still marks them deleted.
- Newer blobs can be orphaned by an older state snapshot.
- Restoring an older `idSeq`/metadata snapshot can reuse numeric file IDs for future uploads; timestamped storage keys reduce physical filename collision risk but do not fix logical consistency.

#### D. Restore Stops Halfway

- Replacing state first can expose metadata before blobs exist.
- Replacing blobs first can expose orphan blobs until state replacement finishes.
- A failed second half leaves a mixed snapshot with no automatic rollback.

Current conclusion: state and blob consistency risk is high. Mode B/C backup must use one quiescent snapshot, a manifest, per-file checksums, validation, and a restore transaction with rollback.

## Identified Risks

| Risk | Severity | Required mitigation |
|---|---:|---|
| Primary state deleted before rename | High | Same-directory durable temp write plus real atomic replace; never remove the only good state first. |
| No file/directory sync | High | Flush and `sync_all`; define Windows durability behavior. |
| Fixed temp path and concurrent saves | High | Unique temp names and a single persistence writer/mutex. |
| Multi-lock snapshot is not coherent | High | Serialize snapshot/save or introduce a state transaction boundary. |
| Save errors are swallowed | High | Return errors, retain dirty state, notify caller/user, and retry safely. |
| Corrupt backup failure still permits reset | Critical | Abort reset if original cannot be preserved. |
| Future schema treated as corrupt | High | Non-destructive compatibility stop. |
| Migration has no pre-migration backup | High | Verified backup before any destructive migration save. |
| Streaming deltas not durably persisted | Low/accepted | P1-C4 marks deltas transient, persists no placeholder/partial reply, and retains only the durable user turn across restart. |
| State/blob snapshot mismatch | High | Quiescent snapshot, manifest, checksums, validation, transaction/rollback. |
| Orphan/missing blobs are not reconciled | Medium | Generate restore report; defer deletion until explicit cleanup approval. |
| DPAPI blobs assumed portable | High | Exclude by default; mark same-machine mode non-portable and opt-in. |
| Logs expose local state path | Medium | Exclude logs and redact local paths from backup diagnostics. |
| Manual folder copy represented as supported backup | High | Explicitly label manual copies unsupported until Phase 12 package semantics exist. |

## Backup Modes

### Mode A: Metadata-Only Backup

Includes:

- settings
- conversations and messages
- provider non-sensitive configuration
- assistant/model selection
- managed file metadata

Excludes:

- managed file blobs
- encrypted secret blobs
- API keys
- original absolute paths
- logs

Restore result:

- Conversation text and settings can be restored.
- Attachments can display unavailable/missing states.
- Provider configuration remains, but API keys must be entered again.
- The format v1 manifest must use `mode: "state-only"`, `managedBlobsIncluded: false`, and `secretsIncluded: false`.

Use Mode A only when incomplete attachment restoration is acceptable.

### Mode B: Full Local Data Backup Without Secrets

Includes:

- `state.v1.json`
- every managed file blob represented by active metadata, including active unreferenced files
- backup manifest
- per-file SHA256 checksums
- consistency report

Excludes:

- `mock-api/secrets/*.bin`
- API keys and DPAPI blobs
- logs
- local absolute paths/usernames
- temp/corrupt files unless explicitly selected for diagnostics

Restore result:

- Conversations and attachments should be complete after validation.
- Provider non-sensitive configuration remains.
- API keys must be entered again.

Mode B is the recommended default portable backup format.

### Mode C: Same-Machine Secure Backup (Deferred)

Mode C is not part of portable backup format v1. Format v1 validation requires `secretsIncluded: false` and rejects any secret directory or encrypted secret entry.

Restrictions:

- Explicit opt-in only.
- Strong warning before creation and restore.
- Intended only for the same Windows machine and same Windows user/profile.
- No guarantee of successful decryption, even after copying.
- Not portable backup.
- Never share, upload, commit, or attach.

Recommendation: defer Mode C implementation. Phase 12 should deliver Modes A/B and a clear secret re-entry workflow first.

## Historical Backup Manifest Draft

The earlier P0 draft used fields such as `backupMode`, `includesFiles`, and `includesSecrets`. That draft is historical, is not accepted by the implementation, and must not be used to build or validate a package.

The only formal format v1 authority is `docs/rikkadesk-phase-12-backup-package-format.md`. Its manifest uses `format`, `formatVersion`, `mode`, `stateSchemaVersion`, `providerImportExportVersion`, fixed exclusion flags, one state record, and controlled file records. Format v1 supports only `state-only` and `full-local-data`, always requires `secretsIncluded: false`, and never restores a package/source secret reference as usable credentials.

## Safe Restore Transaction Design

The formal transaction, staging, journal, commit, rollback, crash recovery, and disk-space protocol is now defined only in `docs/rikkadesk-phase-12-restore-transaction-protocol.md`.

The approved P2-C0 decision is offline full replacement: RikkaDesk and the mock API must be stopped, the package must be completely revalidated, staging must become an independently valid same-volume `mock-api` directory, and current data must be preserved by directory rename before staged data becomes official. Direct state overwrite, live in-process restore, merge restore, Mode C, and default-state fallback are forbidden.

## Documentation Audit

Current docs describe:

- manual app data cleanup
- warnings that backups can contain encrypted secret blobs
- using synthetic app data for schema/file smoke tests
- avoiding old builds against newer schemas
- private handoff checks

Mode A/B export and P2-B validation/dry-run now define a formal package, manifest, checksum set, and consistent export snapshot. P2-C0 defines the future offline restore protocol, but no actual restore writer exists. Manual folder copies are not a supported portable backup format.

Phase 12 owns formal app data backup/restore and migration safety semantics.

## P0 Explicit Non-Goals

P0 does not:

- Implement a backup API.
- Implement a restore API.
- Add backup/restore UI.
- Create a backup archive.
- Read or copy real `state.v1.json`.
- Read, print, or parse `mock-api/secrets/*.bin`.
- Change state format or `schemaVersion` from 6.
- Change provider import/export version from 4.
- Add a schema migration.
- Change managed file storage.
- Change SecretStore or DPAPI behavior.
- Enable real-provider image input.
- Change Android or Tauri configuration.
- Create or move tags.
- Create a GitHub Release.

## Recommended Phase 12 Split

### P0: Audit / Plan

Goal:

- Document current behavior and approve backup/restore safety boundaries.

Files:

- Create `docs/rikkadesk-phase-12-local-data-backup-restore-plan.md`.
- Add one narrow Phase 12 ownership note to the existing manual-cleanup documentation.

Acceptance:

- Docs-only diff.
- No runtime data accessed.
- Current schema/import-export versions unchanged.

### P1-A: Atomic State Write Primitives (Completed)

Goal:

- Make ordinary state saves and migrations preserve the last valid state.

Candidate file:

- `web-ui/src-tauri/src/mock_api.rs`

Implemented design:

- Serialize before touching the primary file.
- Use a unique same-directory temp file.
- Write all bytes, flush, and `sync_all`.
- Serialize saves through one persistence mutex/writer.
- Use a Windows-safe atomic replace primitive or a proven equivalent that never deletes the only good file first.
- Propagate save failure instead of returning success silently.

Tests:

- temp write failure
- replace failure
- concurrent saves
- original state survives every failed commit point

P1-A uses synthetic temporary directories and injected file-operation failures. It does not read real app data or secret blobs.

### P1-B: Load And Migration Safety (Completed)

Goal:

- Make startup, migration, and recovery fail closed without overwriting the last recoverable state.

Required design:

- Create and verify a backup before migration writes.
- Treat unsupported future schemas as a non-destructive compatibility error.
- Abort default-state replacement when corrupt backup preservation fails.
- Distinguish state read errors from JSON/shape parse errors.
- Define downgrade protection and a user-visible recovery decision.
- Define startup handling for stale unique temp files without deleting a valid primary state.
- Add truncated-temp, migration-backup-failure, future-schema, and backup-failure tests.

Implemented with synthetic state and injected read/write/backup/migration failures. No real app data or encrypted secret blob is read.

### P1-C0: Mutation Transaction Boundary Design (Completed)

Goal:

- Audit every persisted mutation and define state, SecretStore, blob, network, streaming, lock, commit, rollback, compensation, and event-ordering boundaries before implementation.

Decision:

- Use stage, persist, then commit live rather than live mutation followed by rollback.
- Add one global mutation transaction mutex.
- Use a short live commit read/write barrier while persisted fields remain split across component locks.
- Never hold component/commit locks across disk sync, SecretStore/blob operations, network waits, or SSE sends.
- Emit success events only after durable state and live state agree.
- Allow ID gaps, forbid duplicate IDs, and never decrement `id_seq` during compensation.
- Keep startup/migration under P1-B rather than the runtime transaction helper.

The complete call-site inventory, lock order, external-side-effect matrix, compensation rules, and synthetic test plan are in `docs/rikkadesk-phase-12-mutation-transaction-boundaries.md`.

### P1-C1: Pure State Staged Transactions (Completed)

- Added a global mutation transaction mutex and short read/write commit barrier around the split persisted components.
- Added a cloneable staged `PersistedMockState`, validation, explicit snapshot persistence through the P1-A atomic writer, and persist-before-live-commit behavior.
- Migrated assistant selection, current assistant model, favorite models, conversation title, pin/unpin, conversation delete, text message edit, and message delete.
- Pure persistence or validation failure now leaves live state, disk state, and the runtime-only revision unchanged. Success SSE/invalidation is emitted only after live commit.
- Removed GET-time persisted conversation creation. Detail and stream GETs return a virtual empty DTO for a missing ID without changing conversations, `id_seq`, or disk.
- Added a runtime-only monotonic revision; it is not written to schema v6. ID gaps remain allowed, duplicate IDs are validated, and `id_seq` is never decremented.
- Coordinated transitional Provider/SecretStore, file/blob, send/regenerate/stop, and background write windows with the mutation mutex and commit barrier without changing their external side-effect order.
- P1-C1 intentionally kept Provider/SecretStore compensation, blob consistency, and streaming transaction semantics out of that step. Provider/SecretStore handled-failure semantics are now completed by P1-C2, file/blob handled-failure semantics by P1-C3, and streaming risk remains for P1-C4.

### P1-C2: Provider And SecretStore Compensation (Completed)

- Added one Provider/SecretStore transaction mutex for import, upsert, key update/clear, and Provider delete.
- Provider metadata, derived settings, and Provider ID allocation now use the existing staged state transaction; failed persistence leaves live/disk/revision and persisted ID high-water unchanged.
- Key create/update uses a unique copy-on-write `secretRef`; failed state persistence compensates the new encrypted blob while preserving the old reference/blob.
- Blank-key upsert performs no secret write and preserves existing `hasSecret` behavior.
- Clear and Provider delete commit state before old-secret cleanup, so state failure cannot destroy the old key.
- Import restores only validated non-sensitive metadata and always imports with `hasSecret: false`.
- Compensation/cleanup uses a bounded three-attempt idempotent delete. Exhausted cleanup after state commit is fixed-warning partial success with an unreferenced encrypted orphan. Process-crash orphan reconciliation remains future work and no secret directory scan was added.
- All tests use synthetic state and an in-memory fake SecretStore; no real app data, DPAPI blob, or API key is read.

### P1-C3: File Blob Transaction And Compensation (Completed)

- Added a dedicated file/blob transaction mutex and a no-overwrite managed blob publisher using unique create-new temps, flush/sync/close, and program-generated storage keys independent of file IDs.
- Accepted upload batches publish all blobs before one staged metadata/ID transaction. Publication failure cleans prior batch blobs; state failure leaves live/disk/revision unchanged and compensates all operation-created finals.
- Added bounded cleanup retry and fixed redacted failures. Cleanup exhaustion or process crash can leave only an API-inaccessible orphan; no automatic orphan scan/GC was added.
- DELETE now blocks persisted numeric attachment references, commits a durable `deletedAt` tombstone before physical cleanup, preserves the blob on state failure, and treats post-commit cleanup failure as logical success with safe retry.
- Path reads reject tombstoned metadata and serialize against delete. Send rejects missing/tombstoned numeric attachment references so a new persisted message cannot race a delete.
- Message/conversation deletion intentionally does not auto-GC attachments. Reconciliation remains part of the later P6 safety work.
- Added 22 synthetic file transaction, compensation, delete, reference, concurrency, and safe-error tests; no real app data or user files are read.

### P1-C4: Background Streaming Transaction Safety (Completed)

- Commits the initial user turn before provider/mock startup and uses no durable assistant placeholder.
- Keeps provider waits and stream reads outside transaction/component/commit/file/Provider locks.
- Uses runtime generation tokens, cancellation, transient delta semantics, final/failure staged commits, stale target checks, and post-commit terminal SSE ordering.
- Defines stop as discard-partial, makes terminal events mutually exclusive, and prevents old tasks from recreating deleted conversations.
- Adds 32 synthetic tests, including blocked loopback SSE with a concurrent Category A transaction.

The runtime mutation gate through P1-C4 is complete. Mode A/B export and P2-B validation/dry-run are also complete; actual offline restore remains unimplemented.

## P2-A Implementation Status

Phase 12 P2-A implements internal portable backup export primitives and directory package format version 1. It does not expose a user command, HTTP endpoint, or UI, and it does not implement restore.

Implemented behavior:

- Mode A exports one consistent sanitized `state.json`, `manifest.json`, and `SHA256SUMS.txt`. It does not inspect or copy managed blobs.
- Mode B exports the same point-in-time state plus every active managed blob under controlled `blobs/file-<file-id>.blob` names. Missing active blobs fail closed; tombstoned and orphan blobs are excluded; active unreferenced files remain included.
- Source Provider/settings secret references are replaced with backup-unavailable placeholders and `hasSecret` state is cleared. Export never calls SecretStore or copies `mock-api/secrets/*.bin`; API keys must be re-entered after a future restore.
- Runtime generations and transient deltas are absent. A committed user turn can be included without an in-progress transient assistant reply, and backup does not support stream resume.
- `backup_export_mutex` serializes exports. Mode B then takes file/blob lock before the short mutation/snapshot lock, releases mutation/component/commit locks, and retains only file/blob lock while streaming blob copies.
- Package files use create-new, flush, and sync. SHA256 is computed from package copies. Manifest/checksum/state/blob relationships are re-read and validated before a unique temp directory is renamed to a non-existing final directory.
- Source blob resolution validates storage metadata, rejects symlink/non-regular/escape sources, and never uses display names or source storage keys as package paths.
- Validation rejects unsafe relative paths, duplicate/mismatched files, unknown blob entries, tampering, secrets directories, schema/version mismatch, and checksum/size mismatch.
- P2-A adds 36 synthetic backup tests. No real app data, secret blob, API key, user file, or real provider is used, and no generated package is stored in the repository.

The complete format is defined in `docs/rikkadesk-phase-12-backup-package-format.md`. P2-A uses a direct `sha2` dependency already present transitively in the lockfile; it adds no ZIP/archive framework.

P2-A does not implement restore validation/dry-run, restore commit/rollback, UI, ZIP, Mode C, orphan scan/cleanup, cross-resource crash atomicity, or complete power-loss protection.

## P2-B Implementation Status

Phase 12 P2-B implements internal restore package validation and a no-write dry-run report for backup format v1. It supports only state schema 6 and the existing format modes `state-only` and `full-local-data`.

- Manifest parsing rejects unknown fields and distinguishes unsupported format/schema from malformed packages.
- Control files are bounded; blobs are stream-hashed. Manifest, state, checksum, size, directory tree, and blob sets must agree exactly.
- Package enumeration rejects symlink/reparse entries, traversal, absolute/UNC/drive paths, backslashes, nested unknown directories, hidden secrets directories, and case-folded collisions.
- State validation is structural and reuses current staged-state, Provider, file, custom request, and modality invariants. Conversation text is never keyword-scanned.
- Package Provider secret references and managed-file storage keys are never trusted. Dry-run creates memory-only planned replacements, reports every Provider as requiring a new API key, and never calls SecretStore.
- Mode A retains attachment metadata/parts and reports active files as potentially unavailable. Mode B requires exactly one verified blob for every active file, including unreferenced active files.
- The restore strategy is full replacement only. Merge restore, schema migration, Mode C, and stream resume are unsupported.
- Dry-run accepts no live-state handle and performs no state/blob/secret/SSE writes. It does not create a pre-restore backup.
- Every invocation re-reads the package. A successful dry-run result cannot be reused to skip P2-C validation.
- P2-B adds 77 synthetic restore tests; no real app data, secret, user backup, user file, API key, or provider is used.

P2-C must revalidate the selected package, create a mandatory pre-restore backup, and then implement an atomic staged restore/rollback transaction. None of those write paths exists in P2-B.

## P2-C0 Protocol Design Status

P2-C0 defines the offline full-replacement restore protocol in `docs/rikkadesk-phase-12-restore-transaction-protocol.md`.

- Live in-process restore and public path-based HTTP restore are rejected.
- Staging, rollback, failed-new data, and journal are same-volume siblings outside the current directory.
- A mandatory local rollback snapshot preserves the complete current `mock-api` directory, including opaque encrypted secret blobs, without reading or decrypting them.
- Portable Mode A/B packages remain secret-free and are fully revalidated for every actual attempt.
- Commit uses old-directory rename followed by staged-directory rename; current data is never deleted or directly overwritten.
- Journal phases and directory existence are reconciled after crashes; ambiguous or rollback-failed states block startup.
- P2-C1 builds and validates staging only. P2-C2 adds journal/rename/rollback. P2-C3 integrates startup validation and interrupted-restore recovery.

P2-C0 changes documentation only. Actual restore remains unavailable.

## P2-C1 Offline Staging Implementation Status

P2-C1 implements an internal offline staging builder with no API/UI/runtime call path:

- Every call completely re-runs P2-B format v1 package validation; a prior dry-run is never accepted as authorization.
- A controlled operation ID allocates same-parent `mock-api.restore-stage.tmp.<operation-id>` and final `mock-api.restore-stage.<operation-id>` names without overwriting other stages.
- Every Provider receives a fresh local controlled `secretRef`, every managed file receives a fresh `storageKey`/`relativePath`, and all Providers remain without a usable API key.
- Mode A writes validated state with an empty blob directory and reports active attachments unavailable without deleting attachment parts or metadata.
- Mode B stream-copies exactly every active package blob, including unreferenced active files, while rechecking source and staged size/SHA256. Tombstoned and unknown blobs are not copied.
- Staged `state.v1.json` uses the P1-A durable unique-temp/flush/sync/rename primitive against the stage path, never formal persistence.
- A pure read-only validator enforces the exact `state.v1.json`, `files/blobs`, and empty `secrets` layout, schema/state invariants, no runtime generation, controlled refs/keys, exact Mode B blob set, and no links/reparse points or package control files.
- Capacity is checked before stage creation. Windows uses the existing `windows-sys` filesystem feature; tests use a fail-closed synthetic checker.
- Only a fully validated temp stage is renamed to the candidate final name, then re-opened and validated again. Failures best-effort remove only the current operation's owned stage.
- Forty-six dedicated synthetic tests use no real app data, backup, user file, secret blob, API key, or provider.

The builder has no `MockApiState` or SecretStore parameter and does not write, rename, enumerate, or replace the current formal `mock-api` directory. It creates no pre-restore/failed-restore directory or journal and emits no SSE/revision/live commit. A final stage is only a candidate directory, not restored or official data. P2-C2 consumes that controlled stage through a separate offline commit primitive.

## P2-C2 Journaled Commit And Rollback Implementation Status

P2-C2 implements an internal, unwired offline commit engine:

- The engine accepts only a trusted app-data parent and a controlled three-component numeric operation ID. All operation paths are derived internally beside `mock-api`; no arbitrary restore path is accepted.
- A process mutex and twice-checked trusted offline permit gate the transaction. Existing journals, operation collisions, unrelated rollback/failed artifacts, links/reparse points, invalid current state, and invalid stages fail before current data is renamed.
- The P2-C1 stage validator is rerun immediately before commit. Current `state.v1.json` is validated read-only as schema 6, while existing secret blobs and allowed diagnostic data remain opaque and untouched.
- Journal updates use create-new or unique-temp durable writes plus atomic replacement. Directory commit is current to rollback, then stage to current; no current-directory delete or direct state overwrite exists.
- A handled failure after the old directory moves restores rollback to current. If a provisional new current exists, it is preserved as a failed-restore directory first. The restored original is revalidated before `rollback-completed` is recorded.
- Rollback failure or journal ambiguity fails closed, retains operation evidence, and requires manual/P2-C3 recovery. No default state is created.
- Success returns `PendingStartupValidation` and leaves journal phase `commit-new-moved`, the rollback snapshot, and provisional new current in place. It does not write `startup-validation`/`completed`, start the mock API, or expose an API/UI.
- Windows parent-directory durability remains a documented best-effort handle barrier rather than a claim of complete power-loss atomicity.
- Fifty-seven synthetic P2-C2 tests cover journal and rename fault boundaries, rollback paths, operation conflicts, concurrency, secret opacity, and the pending-startup boundary. No real app data, user backup, user file, secret, API key, or provider is used.

P2-C3 remains mandatory before launch or user orchestration. It must reconcile retained journal/directory states after interruption, repeat read-only validation, decide whether to continue or roll back, and block startup on ambiguity without default-state fallback.

### P2-A: Portable Metadata/Full Backup Export Package (Completed)

Goal:

- Export and self-validate format v1 Mode A/B directory packages while excluding secrets.

Candidate files:

- `web-ui/src-tauri/src/mock_api.rs` or a new focused backup module
- backend tests
- backup format documentation

Acceptance:

- Manifest contains no source path, source secret reference, API key, or blob content.
- Mode A exports a sanitized state snapshot only and does not read managed blobs.
- Mode B exports state plus every active blob, including unreferenced active files.
- Every package file is allowlisted and SHA256-verified before publication.
- No API key, SecretStore value, or DPAPI blob is accessed or exported.

Implemented with backend-only primitives and synthetic tests. Restore remains unavailable.

### P2-B: Restore Validation And Dry Run (Completed)

Goal:

- Parse and validate a selected format v1 package without modifying current app data.

Required behavior:

- Reuse package path/hash/schema/mode validation.
- Report missing, unknown, tombstoned, and incompatible entries with fixed safe diagnostics.
- Normalize all Providers to `hasSecret: false` in the proposed restore state.
- Produce a dry-run report only; do not replace state or blobs.
- Keep Mode C and secret restoration deferred.

Implemented as an internal full-replacement preview with fixed safe counts/warnings and zero writes. Actual restore remains unavailable.

### P3: Managed File Blob Backup/Restore

Goal:

- Create a consistent state/blob snapshot and validation report.

Required behavior:

- Stop or quiesce mutations during snapshot.
- Validate active metadata against blob existence, size, MIME, and checksum.
- Report missing/orphan/deleted entries.
- Do not auto-delete orphans during restore.
- Restore through staging and rollback.

### P4: Same-Machine Encrypted Secret Policy

Goal:

- Decide whether Mode C should exist.

Current recommendation:

- Keep deferred unless a concrete same-machine recovery need justifies the risk.
- If approved, require explicit opt-in, same-user/machine warning, and no portability promise.
- Never merge Mode C into default portable backup.

### P5: Backup/Restore UI Or Manual Command

Goal:

- Provide a user-controlled entry point after backend safety primitives are complete.

Required UX:

- Choose backup mode and destination.
- Show included/excluded data before creation.
- Restore preview with schema/mode/checksum and missing/orphan counts.
- Explicit overwrite/rollback warning.
- Enforce app shutdown or use a separate restore helper.
- Never display secret content or raw local paths in diagnostics.

### P6: Corruption, Rollback, And Migration Smoke

Fixtures:

- truncated JSON
- malformed JSON
- missing `schemaVersion`
- unsupported future schema
- schemas 1 through 5 migrating to 6
- migration backup failure
- missing blob
- orphan blob
- interrupted state replace
- interrupted restore commit
- rollback success and rollback failure
- downgrade attempt with synthetic data
- DPAPI unavailable/decrypt failure using synthetic non-secret fixtures only

Acceptance:

- No failed scenario overwrites the last valid state silently.
- No test reads real app data or secret blobs.
- Recovery reports contain no user content or secrets.

## Phase 12 Status And Remaining Blockers

Current P1-A/P1-B/P1-C1/P1-C2/P1-C3/P1-C4 status:

- Atomic replacement state write exists: Yes for the P1-A single-file commit path; no pre-delete remains.
- Corrupt backup is fail-closed: Yes after P1-B; the primary is retained and no default is written.
- Direct primary-state removal in ordinary saves exists: No.
- Persistence save mutex exists: Yes.
- Persistence errors reach mutation handlers: Yes; background completion emits a fixed safe `failed` terminal and no `finished` success.
- Pure settings/conversation persistence failure leaves live state unchanged: Yes after P1-C1.
- Provider/SecretStore handled-failure compensation exists: Yes after P1-C2; process-crash encrypted orphan reconciliation remains pending.
- Managed file/blob handled-failure compensation exists: Yes after P1-C3; process-crash orphan reconciliation and automatic GC remain pending.
- Streaming handled-failure transaction semantics exist: Yes after P1-C4; initial/final stages are fail-closed and deltas are transient.
- Silent automatic reset after read/parse/schema failure exists: No.
- Future schema is fail-closed: Yes; it is not treated as corrupt or migrated.
- Schema 1-5 pre-migration backup exists: Yes.
- Automatic non-NotFound reset exists: No.
- Runtime mutation consistency risk exists: Reduced through P1-C4 for Category A, Provider/SecretStore, managed file/blob, and streaming handled-failure paths. State/network are not one atomic transaction and active streams do not resume after restart.
- State/blob consistency risk exists: Handled failures are compensated/tombstoned after P1-C3, but crash-only orphan windows and restore reconciliation remain.
- DPAPI secret blobs are portable across machines/users: No.
- A formal Mode A/B backup package exists: Yes, format v1 export, P2-B strict validation/dry-run, P2-C1 independently validated candidate staging, and P2-C2 internal journaled commit/handled rollback. A user-consumable restore does not exist because P2-C3 recovery/startup integration is still required.

Recommended next step: P2-C3 startup validation and interrupted-restore reconciliation. Do not expose actual restore until journal/directory crash-state recovery, no-default fallback, and startup ownership tests pass.
