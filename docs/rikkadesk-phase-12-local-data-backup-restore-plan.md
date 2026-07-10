# RikkaDesk Phase 12 Local Data Backup, Restore, And Migration Safety Plan

This document is the Phase 12 P0 audit and design plan. It records the current local persistence behavior and defines safe boundaries for future backup, restore, corruption recovery, schema migration, managed file blobs, and Windows DPAPI secret handling.

P0 is documentation only. It does not implement backup or restore APIs, add UI, read real app data, change `state.v1.json`, change `schemaVersion`, change provider import/export, or create a backup archive.

## Current Status

- Baseline commit: `6767eb841169db63b047ed280b6c1a2f3ca4b349`.
- Stable runtime tag: `rikkadesk-v0.1.0-beta.14`.
- App/package version: `0.1.0`.
- State filename: `state.v1.json`.
- State payload schema: `schemaVersion: 6`.
- Provider import/export version: `4`.
- Windows artifacts: unsigned private beta.
- GitHub Release: No.
- Real-provider image input: not enabled by default.

## P0 Decisions

1. Mode B, a full local data backup without secrets, should become the default portable backup format.
2. Mode A, metadata-only backup, is useful when conversation text and settings matter more than attachment availability.
3. Mode C, same-machine secure backup including encrypted secret blobs, should remain deferred until its threat model, consent, and recovery semantics are independently approved.
4. A directory copy made while RikkaDesk is running is not a consistent backup.
5. Restore must use a validated staging area, an automatic pre-restore backup, an explicit commit step, and rollback. It must never overwrite current app data directly.
6. Phase 12 should start implementation with P1 atomic state backup/write primitives before adding packaging or UI.

## Current Persistence Implementation

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

The API accepts file IDs, not filesystem paths. `file_blob_path` validates `storageKey` and constructs blob paths under the managed blob directory.

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
```

On Windows, the reference is encoded into a safe filename and mapped to:

```text
mock-api/secrets/<encoded-secret-ref>.bin
```

The blob contains DPAPI-protected bytes, not plaintext. `state.v1.json` does not contain the API key.

### Provider Deletion

Deleting a provider first deletes its secret blob. If secret deletion fails, provider deletion stops. If secret deletion succeeds but later state persistence fails, disk state may still contain the provider and `secretRef` while its blob is gone.

### Backup Combinations

State without secret blobs:

- Provider non-sensitive configuration and `secretRef` are restored.
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

Upload currently:

1. Writes each blob through a per-blob temp file and rename.
2. Adds metadata to in-memory state.
3. Persists full state.

If step 3 fails, blobs can exist without persisted metadata.

Delete currently:

1. Deletes the blob.
2. Marks metadata deleted in memory.
3. Persists full state.

If step 3 fails or the process stops between steps, persisted metadata can refer to a missing blob.

There is no startup orphan scan, orphan cleanup, missing-blob reconciliation, or metadata/blob transaction.

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
| Streaming deltas not durably persisted | Medium | Define checkpoint policy and clear interrupted generating state on recovery. |
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
- The manifest must say `includesFiles: false` and `includesSecrets: false`.

Use Mode A only when incomplete attachment restoration is acceptable.

### Mode B: Full Local Data Backup Without Secrets

Includes:

- `state.v1.json`
- managed file blobs referenced by active metadata
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

### Mode C: Same-Machine Secure Backup

Includes:

- everything in Mode B
- encrypted secret blobs
- manifest declaration `includesSecrets: true`

Restrictions:

- Explicit opt-in only.
- Strong warning before creation and restore.
- Intended only for the same Windows machine and same Windows user/profile.
- No guarantee of successful decryption, even after copying.
- Not portable backup.
- Never share, upload, commit, or attach.

Recommendation: defer Mode C implementation. Phase 12 should deliver Modes A/B and a clear secret re-entry workflow first.

## Backup Manifest Draft

The package manifest should be UTF-8 JSON and contain metadata only:

```json
{
  "formatVersion": 1,
  "appVersion": "0.1.0",
  "schemaVersion": 6,
  "createdAt": "<ISO-8601>",
  "sourcePlatform": "windows",
  "sourceAppIdentifier": "com.cisyamx.rikkadesk",
  "backupMode": "metadata-only | full-without-secrets | same-machine-secure",
  "includesState": true,
  "includesFiles": false,
  "includesSecrets": false,
  "fileCount": 0,
  "checksums": {
    "state.v1.json": "<SHA256>"
  },
  "notes": []
}
```

Manifest rules:

- No API key, Authorization value, custom header value, cookie, token, or password.
- No secret blob content.
- No chat content or chat summary.
- No Windows username, user profile path, or absolute path.
- Every package entry uses a normalized relative archive path.
- Checksums cover the exact archived bytes.
- `fileCount` and `checksums` must agree with archive contents.
- Unknown files in the package fail validation rather than being silently extracted.
- `secretRef` may remain inside state because it is an identifier, not credential material. A portable restore must still mark the provider as missing its key until a secret is re-entered.

Recommended additions during P2/P3 design:

- package creation ID
- state saved timestamp
- total uncompressed bytes
- active/missing/orphan file counts
- archive entry allowlist version

None of those fields may contain user content or local paths.

## Safe Restore Transaction Design

Future restore must be offline or coordinated by a helper that can guarantee the running app has released state/blob files.

Required sequence:

1. Require RikkaDesk to be fully closed.
2. Confirm no RikkaDesk process is running.
3. Open the backup as data, not as executable content.
4. Read and parse the manifest before extracting entries.
5. Validate `formatVersion` against the supported backup format.
6. Validate `schemaVersion` against the current app's supported migration range.
7. Validate `sourceAppIdentifier` exactly.
8. Validate backup mode and reject unexpected secret entries.
9. Validate entry names against an allowlist and reject traversal, absolute paths, links, devices, and duplicate names.
10. Check available disk space for staging, current-data backup, and rollback.
11. Verify every checksum before changing app data.
12. Create a timestamped automatic backup of current app data without reading secret contents.
13. Extract/copy restore contents into a unique sibling staging directory on the same volume.
14. Parse staged state JSON and validate schema, required fields, IDs, storage keys, relative paths, MIME, and size metadata.
15. Validate state/blob consistency and produce missing/orphan reports.
16. For Mode A, explicitly accept unavailable attachments before commit.
17. For Mode C, attempt DPAPI validation without logging plaintext; abort or restore providers without secrets according to explicit user choice.
18. Write a restore journal containing only operation IDs, phase names, and safe error codes.
19. Commit by preserving current app data as rollback data and replacing it with the fully validated staged snapshot.
20. If commit fails, restore the original app data and leave the staged directory for safe cleanup.
21. Keep failed-restore diagnostics free of chat content, file content, secrets, usernames, and absolute paths.
22. On first startup, perform integrity checks before allowing new writes.
23. Clear the restore journal only after startup validation succeeds.

Direct overwrite of current app data is forbidden.

### Failure Handling

| Failure | Required behavior |
|---|---|
| Power loss during staging | Current app data remains untouched; stale staging is detected next run. |
| Power loss during commit | Journal identifies whether rollback or completion is required. |
| RikkaDesk starts during restore | Abort before commit; do not race the app writer. |
| Disk space insufficient | Abort before automatic backup/extraction changes current data. |
| Defender/antivirus locks a file | Abort and retain original data; report a safe locked-file error. |
| Checksum mismatch | Reject the package; do not extract/restore it. |
| Unsupported schema | Preserve both package and current data; require a compatible build. |
| Missing blob | Report it; require explicit Mode A-style acceptance or abort Mode B/C. |
| Orphan blob | Report it; do not automatically trust or expose it. |
| DPAPI decryption failure | Keep provider config, mark secret unavailable, never expose/delete blob silently. |
| Package modified or contains unknown entry | Reject before app data changes. |
| Rollback fails | Stop, preserve all directories/journal, and require manual recovery; never create default state over them. |

Restore failure must not silently create and save default state over user data.

## Documentation Audit

Current docs describe:

- manual app data cleanup
- warnings that backups can contain encrypted secret blobs
- using synthetic app data for schema/file smoke tests
- avoiding old builds against newer schemas
- private handoff checks

They do not define a formal backup package, manifest, checksum set, consistent state/blob snapshot, restore transaction, or portable-secret policy. Manual folder copies are therefore not a supported portable backup format.

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

### P1: Atomic State Backup Primitives

Goal:

- Make ordinary state saves and migrations preserve the last valid state.

Candidate file:

- `web-ui/src-tauri/src/mock_api.rs`

Required design:

- Serialize before touching the primary file.
- Use a unique same-directory temp file.
- Write all bytes, flush, and `sync_all`.
- Parse/validate the temp state before commit.
- Serialize saves through one persistence mutex/writer.
- Use a Windows-safe atomic replace primitive or a proven equivalent that never deletes the only good file first.
- Preserve a verified pre-migration backup.
- Abort reset/migration if backup cannot be created.
- Propagate save failure instead of returning success silently.
- Recover/clean stale temp files without deleting valid state.

Tests:

- temp write failure
- replace failure
- concurrent saves
- truncated temp
- migration backup failure
- original state survives every failed commit point

### P2: Portable Metadata/Full Backup Package

Goal:

- Implement manifest and checksum generation for Modes A/B, excluding secrets.

Candidate files:

- `web-ui/src-tauri/src/mock_api.rs` or a new focused backup module
- backend tests
- backup format documentation

Acceptance:

- Manifest contains no local path/user content/secret.
- Mode A exports state metadata only.
- Mode B exports state plus referenced blobs.
- Every entry is checksummed and allowlisted.
- No API key or DPAPI blob is exported.

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

## P0 Conclusion And Blockers

Current blocker conclusions:

- Atomic state write exists: No.
- Corrupt backup exists: Yes, but only for parse/unsupported-schema paths and it is not fail-closed.
- Direct primary-state removal exists: Yes.
- Persistence save mutex exists: No.
- Silent reset risk exists: Yes.
- Silent data loss risk exists: Yes.
- State/blob consistency risk exists: Yes.
- DPAPI secret blobs are portable across machines/users: No.
- A formal backup/restore package currently exists: No.

Recommended next step: Phase 12 P1 atomic state backup primitives. P1 should be completed and fault-tested before Mode A/B packaging, managed blob restore, or any backup/restore UI.
