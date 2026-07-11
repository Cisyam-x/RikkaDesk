# RikkaDesk Portable Backup Package Format

This document defines Phase 12 P2-A backup package format version 1. P2-A implements internal backend export primitives only. It does not expose an HTTP endpoint, command, or UI; it does not implement restore, ZIP archives, Mode C, orphan cleanup, or stream resume.

## Current Boundary

- App version: `0.1.0`.
- State schema: `schemaVersion: 6`.
- Provider import/export: version `4`.
- Backup format: `rikkadesk-backup`, `formatVersion: 1`.
- Mode A: portable state-only package.
- Mode B: portable state plus every active managed blob; recommended default.
- SecretStore, Windows DPAPI blobs, API keys, runtime generations, transient deltas, logs, and local absolute paths are excluded.
- Mode C and portable secret recovery remain deferred.

## Directory Layout

```text
RikkaDesk-backup-v1-<timestamp>-<counter>/
|-- manifest.json
|-- state.json
|-- SHA256SUMS.txt
`-- blobs/
    |-- file-<file-id>.blob
    `-- ...
```

Mode A has no `blobs/` directory. Mode B always has `blobs/`, which may be empty when the snapshot contains no active managed files.

Package blob names use only the numeric persisted file ID. They never use the original display name or source `storageKey`. All manifest/checksum paths use `/` separators and controlled relative paths.

## Manifest

```json
{
  "format": "rikkadesk-backup",
  "formatVersion": 1,
  "mode": "state-only",
  "createdAt": "2026-01-01T00:00:00Z",
  "appVersion": "0.1.0",
  "stateSchemaVersion": 6,
  "providerImportExportVersion": 4,
  "secretsIncluded": false,
  "managedBlobsIncluded": false,
  "runtimeGenerationsIncluded": false,
  "state": {
    "path": "state.json",
    "sha256": "<lowercase-sha256>",
    "sizeBytes": 0
  },
  "files": []
}
```

Mode B uses `mode: "full-local-data"`, `managedBlobsIncluded: true`, and one sorted file record per active managed file:

```json
{
  "fileId": 123,
  "packagePath": "blobs/file-123.blob",
  "mime": "image/png",
  "sizeBytes": 456,
  "sha256": "<lowercase-sha256>"
}
```

The manifest does not contain source `storageKey`, display name, source/destination path, username, machine name, SID, API key, source `secretRef`, provider request/response, or blob bytes. File records are strictly increasing by `fileId`.

## Portable State Snapshot

`state.json` is serialized from a point-in-time `PersistedMockState` snapshot, not copied from a potentially stale disk file. The snapshot is obtained under the mutation transaction mutex and commit-barrier read path, then all state locks are released before package I/O.

Export is read-only:

- It does not persist state.
- It does not modify live state or revision.
- It does not allocate IDs.
- It does not emit SSE.
- Runtime generation registry entries and transient assistant deltas are not part of persisted state.
- A durable user turn may be present while its transient assistant reply is absent.

Before serialization, the exporter creates a portable clone:

- Every conversation has `isGenerating: false`.
- Desktop Provider `secretRef` values become controlled `backup-unavailable` placeholders.
- Other settings `secretRef` fields become non-source placeholders.
- Settings `hasSecret` fields become `false`.
- Desktop Provider settings are rebuilt to match the sanitized Provider metadata.

The placeholders are identifiers only, not credentials. No corresponding secret is included. Future restore must treat every Provider as having no usable secret and generate a fresh local reference before asking the user to enter an API key again.

## Mode A: State Only

Mode A contains:

- `manifest.json`
- `state.json`
- `SHA256SUMS.txt`

It does not inspect or copy the managed blob root. File metadata and message `fileId` references remain in `state.json`, so attachments can be unavailable after restore. Mode A never removes attachment parts from the snapshot and never silently claims that blob content is recoverable.

## Mode B: State And Active Blobs

An active managed file has metadata with `deletedAt: null`. Mode B includes every active metadata/blob pair, including active files not currently referenced by a message.

- Missing active blob: fail the export; do not skip it or downgrade to Mode A.
- Tombstoned metadata: retain it in `state.json`, but do not copy its blob.
- Orphan blob without metadata: do not include, inspect, or delete it.
- Duplicate file IDs or invalid storage metadata: reject the snapshot.
- Metadata size or existing metadata SHA256 mismatch: reject the export.

Mode B holds `file_blob_transaction_mutex` from snapshot acquisition through blob copy. Upload, delete, file serving, and provider-image reads therefore cannot alter managed blobs during the package snapshot. Other state mutations can continue after the short persisted snapshot is cloned.

## Lock Order

Mode A:

```text
backup_export_mutex
  -> mutation_transaction_mutex
  -> commit barrier read and component snapshot
  -> release state locks
  -> serialize, hash, validate, publish
```

Mode B:

```text
backup_export_mutex
  -> file_blob_transaction_mutex
  -> mutation_transaction_mutex
  -> commit barrier read and component snapshot
  -> release mutation/component/commit locks
  -> copy and hash active blobs while retaining file/blob lock
  -> validate and publish
  -> release file/blob lock
```

No Provider/SecretStore transaction lock is acquired. No state/component/commit lock spans blob copy. Existing file operations already use file/blob lock before the staged state transaction, so no reverse mutation-to-file lock order is introduced.

## Blob Path Safety

For every Mode B blob, the exporter:

1. Validates the program-generated `storageKey` character allowlist.
2. Validates the expected controlled `relativePath`.
3. Rejects missing, symlink, directory, and non-regular sources.
4. Canonicalizes the blob root and source and requires the source parent to equal the controlled root.
5. Writes only `blobs/file-<file-id>.blob` with create-new semantics.
6. Streams the copy instead of loading the full blob into memory.
7. Flushes and syncs the destination file.
8. Re-reads the package copy to calculate size and SHA256.

The internal builder accepts a trusted destination folder selected by a future native caller. P2-A exposes no HTTP path parameter. The destination root is required to be an existing non-symlink directory and is canonicalized before child paths are created.

## Checksums

`SHA256SUMS.txt` contains lowercase SHA256 values and lexicographically sorted package-relative paths:

```text
<hash>  blobs/file-123.blob
<hash>  manifest.json
<hash>  state.json
```

It includes `manifest.json`, `state.json`, and every Mode B blob. It does not include itself. The manifest independently includes the state/blob hashes and sizes, avoiding a self-hash cycle.

Before publication, validation re-reads every listed file and verifies:

- manifest format, mode, schema, and fixed exclusion flags
- state hash, size, and schema
- sanitized secret/runtime state
- Mode A has no blob directory
- Mode B active file IDs exactly match manifest and blob entries
- blob hashes and sizes
- checksum file contents and ordering
- safe relative paths
- no symlinks or unknown root/blob entries
- no `secrets/` directory

## Atomic Publication

The exporter serializes concurrent exports with a runtime-only `backup_export_mutex` and writes under the trusted destination root:

1. Create a unique `.rikkadesk-backup.tmp.<pid>.<counter>` directory.
2. Write and sync `state.json`.
3. Copy and sync Mode B blobs.
4. Write and sync `manifest.json`.
5. Write and sync `SHA256SUMS.txt`.
6. Re-read and validate the complete package.
7. Rename the temp directory to a unique `RikkaDesk-backup-v1-<timestamp>-<counter>` directory.

The final directory must not exist and is never overwritten. Failure best-effort removes only the current temp directory. Existing packages and unrelated temp directories are untouched. Directory rename is not claimed to provide complete power-loss protection.

## Safe Errors

Export errors use fixed categories: invalid snapshot, missing blob, blob read failure, write failure, hash mismatch, validation failure, and publish failure. Errors and logs must not contain state/user/provider content, display name, `storageKey`, source/destination path, source `secretRef`, API key, request body, or blob bytes.

## P2-B Restore Validation And Dry Run

P2-B adds an internal, no-write validator for format v1 directory packages. It exposes no HTTP API, UI, native folder picker, or restore command. The source package path is accepted only from a trusted future internal caller; tests use synthetic temp directories exclusively.

Compatibility is fail-closed:

- `format` must be `rikkadesk-backup` and `formatVersion` must be `1`.
- Mode A is `state-only`; Mode B is the existing format v1 value `full-local-data`.
- Both manifest and state schema must be exactly `6`; schema 1-5 migration and newer schema support are deferred.
- Provider import/export metadata must be version `4`.
- `secretsIncluded` and `runtimeGenerationsIncluded` must be `false`.
- Unknown manifest fields, modes, paths, files, directories, and blob entries are rejected.

The dry run reads `manifest.json` and `state.json` once each and hashes the same bytes it parses. Manifest reads are limited to 4 MiB, checksum reads to 8 MiB, state reads to 128 MiB, and manifest file records to 100,000. Blobs are hashed as streams and must match both manifest and state metadata. `SHA256SUMS.txt` accepts hexadecimal case but requires exactly two spaces, controlled relative paths, no duplicate/case-folded collisions, no self-entry, and exact package coverage.

Directory enumeration rejects absolute paths, drive/UNC paths, `..`, `.`, empty components, backslashes, symlinks, Windows reparse points, nested unknown directories, and case-insensitive path collisions. Mode A may omit `blobs/` or contain an empty `blobs/`; any blob bytes are rejected. Mode B requires an exact one-to-one match between active file metadata and `blobs/file-<file-id>.blob`. Tombstoned and orphan blob entries are rejected, while active unreferenced files remain restorable.

State validation reuses current staged-state, Provider, managed-file, custom-header, custom-body, and modality invariants. It additionally verifies ID high-water marks, branch selection, no active generation marker, no unsafe attachment URL, and the current structured state shape. Validation is structural: ordinary conversation text may contain words such as `Authorization` without being rejected.

The in-memory restore plan never trusts package `secretRef` or `storageKey` values. Every Provider is planned with a fresh controlled placeholder and `hasSecret: false`; every managed file receives a new planned storage key. These values are not persisted, returned in the public-safe report, or used to create files/secrets. API keys must be entered again after a future restore.

Dry-run reports only versions, mode, safe counts, fixed warnings, and normalization counts. Restore strategy is full replacement only; merge restore is unsupported. Mode A reports active files as potentially unavailable and does not remove attachment parts. Mode B reports the exact restorable blob count.

The validator takes no `MockApiState` or SecretStore handle. It cannot modify live state, disk state, revision, IDs, SSE, active generations, the formal blob root, or encrypted secret blobs. It does not create a pre-restore backup or temp restore directory. A successful dry run is not an authorization token or validation cache: P2-C must completely re-read and revalidate the package immediately before any restore transaction.

## P2-C0 Actual Restore Protocol Boundary

The future actual restore protocol is defined in `docs/rikkadesk-phase-12-restore-transaction-protocol.md`. Actual restore is offline full replacement only: RikkaDesk must be closed, format v1 must be completely revalidated, and commit must use a same-volume staged `mock-api` directory plus a mandatory local rollback snapshot.

The portable package and rollback snapshot are different artifacts:

- A portable Mode A/B package always remains secret-free and can never restore an API key or source-machine secret reference.
- A local rollback snapshot is an opaque rename-preserved copy of the current complete `mock-api` directory. It can contain encrypted secret blobs solely so the same-machine pre-restore state can be restored after failure. It is not portable and must never be shared or uploaded.

Actual staging must generate fresh local Provider secret references and file storage keys. It must not reuse P2-B deterministic planned values. After source validation, package content is copied and revalidated inside staging; commit reads only the staging copy. A prior dry-run never permits P2-C to skip this work.

P2-C1 implements an internal staging writer. It revalidates format v1 on every call, creates fresh local Provider references and file storage keys, builds an independently validated same-volume candidate `mock-api` directory, and never accesses SecretStore or current formal data.

P2-C2 implements an internal offline journaled directory commit and handled-failure rollback. It revalidates the stage, read-only validates current schema 6 data, preserves the complete current `mock-api` directory by same-parent rename, publishes the stage by rename, and retains both the rollback snapshot and journal. Current encrypted secret blobs are opaque rollback data and are never read or decrypted. Success stops at `commit-new-moved` with `PendingStartupValidation`.

P2-C3 now reconciles the retained journal and direct-child directory topology before ordinary state loading or any default/migration/write path. Valid provisional current data is revalidated, loaded through an exact-schema no-write loader, and marked `completed` before listener readiness. Invalid provisional data is preserved and the old current is restored when unambiguous. `rollback-completed` allows the validated old current to start; rollback failure, malformed/future journal data, operation mismatch, and topology ambiguity block startup. Completed journals and rollback snapshots remain retained.

## Deferred Work

P2-C1 staging, P2-C2 journaled commit/rollback, and P2-C3 startup reconciliation are implemented as internal backend safety primitives. Restore UI/commands and user orchestration remain unavailable. ZIP packaging, Mode C, merge restore, automatic artifact/orphan cleanup, active-stream recovery, portable-secret restoration, and complete power-loss atomicity remain unimplemented.
