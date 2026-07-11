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

## Deferred Work

P2-B will design package validation and restore dry-run reporting without writing app data. Restore commit/rollback, backup UI/commands, ZIP packaging, Mode C, automatic orphan reconciliation, active-stream recovery, and cross-resource crash atomicity remain unimplemented.
