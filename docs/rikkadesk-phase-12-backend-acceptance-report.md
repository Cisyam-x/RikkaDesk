# RikkaDesk Phase 12 Backend Safety Acceptance Report

## Decision

**Backend safety chain accepted.**

**User-facing restore unavailable.**

This is not a statement that backup/restore is fully released. No restore HTTP API, Tauri command, folder picker, confirmation flow, restart orchestration, progress UI, tag, installer publication, or GitHub Release is included.

## Acceptance Boundary

| Item | Accepted value |
|---|---|
| Code boundary | `d618e96f` (`fix: reject ambiguous rollback residual topology`) |
| App version | `0.1.0` |
| State schema | `6` |
| Provider import/export | `4` |
| Backup format | `rikkadesk-backup` format version `1` |
| Mode A | `state-only` |
| Mode B | `full-local-data` |
| Restore strategy | Offline full replacement only |

No new migration, format v2, Mode C, ZIP, merge restore, restore route/command/UI, tag, Release, or dependency is part of this acceptance.

## Accepted Scope

- Atomic state persistence and fail-closed load/migration behavior.
- Staged settings/conversation mutations.
- Provider metadata plus SecretStore handled-failure compensation.
- Managed file metadata plus blob handled-failure compensation and tombstones.
- Streaming initial/final staged commits and terminal-event ordering.
- Mode A/B portable backup export and self-validation.
- Strict restore validation and no-write dry run.
- Independently validated same-volume restore staging.
- Offline journaled directory commit and handled rollback.
- Startup journal/topology reconciliation and guarded provisional loading.

The production restore primitives intentionally have no user-facing one-call orchestrator. Acceptance therefore composes the same internal handoffs under isolated synthetic parents and verifies that each handoff re-reads and revalidates its input. A prior dry run is never treated as authorization or a validation cache.

## Startup Chain Audit

The reviewed startup order is:

```text
main.rs
-> mock_api::start
-> reconcile_restore_before_start
-> guarded state load
-> state/storage ownership
-> mark_restore_startup_completed
-> router construction
-> listener bind
-> listener ready
```

Reconciliation precedes ordinary persistence load, default initialization, migration, corrupt/pre-migration backup, SecretStore construction, router construction, and listener bind. Ambiguous recovery stops before those actions. Provisional loading requires exact schema 6 and has no default, migration, corrupt-backup, or same-process retry path. `completed` is durable before listener bind; later listener failure does not roll back a completed restore.

## Runtime Mutation Audit

Production persisted mutations continue to use these boundaries:

| Category | Accepted behavior |
|---|---|
| Category A | Clone staged state, validate, persist, then commit to live state |
| Provider/SecretStore | Copy-on-write secret reference, state commit, bounded compensation/cleanup |
| File/blob | Publish before metadata commit with compensation; tombstone before delete cleanup |
| Streaming | Initial user commit before work; deltas transient; final assistant commit before success |

The production call-site review found no new `mutate live -> persist -> dirty live on failure` path. Direct persistence in this area remains inside the staged transaction helper; test-only helpers are excluded from production behavior.

## Synthetic Mode A Acceptance

The Mode A chain passed through isolated synthetic export, validation/dry-run, staging, commit, and startup-reconciliation tests.

- Settings and conversation state remain schema 6 and structurally valid.
- Attachment metadata remains present while no managed blob is restored.
- The dry-run and stage report unavailable attachments.
- Provider source secret references are replaced and Providers remain without usable secrets.
- Commit returns `PendingStartupValidation`, retains rollback, and stops at `commit-new-moved`.
- Startup reconciliation strictly validates the provisional current, guarded-loads it, and records `completed` before readiness.
- SecretStore access count remains zero throughout backup/restore primitives.

## Synthetic Mode B Acceptance

The Mode B chain passed through isolated synthetic export, validation/dry-run, staging, commit, and startup-reconciliation tests.

- Active referenced and active unreferenced blobs are included and restored.
- Tombstoned and orphan blobs are excluded.
- Restored files receive fresh storage keys and verified relative paths.
- Providers receive fresh non-source references and remain without usable secrets.
- Blob size and SHA-256 are verified during export, validation, copy, staged validation, commit validation, and provisional reconciliation.
- Missing or mismatched provisional Mode B blobs conservatively preserve failed-new and restore the rollback snapshot.
- The rollback snapshot remains after successful completion.

## Secret Boundary

- Backup export, restore dry-run, restore staging, restore commit, rollback, and startup reconciliation have no SecretStore calls.
- Synthetic panic-on-access stores and opaque non-secret fixtures prove zero access where a store can be supplied.
- Current encrypted blobs are preserved only through directory rename into the local rollback snapshot.
- Recovery never opens, hashes, decrypts, copies, or logs an opaque secret blob.
- Portable packages and restored Providers contain no usable source secret; API keys must be re-entered.

## Tamper And Crash Acceptance

- Source changes after dry run are caught by complete revalidation.
- Package, manifest, state, checksum, and blob tampering fail closed.
- Copy-time and final-stage tampering produce no published candidate/current.
- Journal phases `staging`, `backup-current`, `commit-old-moved`, `commit-new-moved`, `startup-validation`, `rollback-required`, `rollback-completed`, `rollback-failed`, and `completed` are covered.
- Deterministic states restore the old current conservatively; ambiguous states block startup.
- No recovery failure creates default state or overwrites an existing controlled artifact.
- Missing/malformed/old/future provisional state triggers safe failure and rollback without migration or corrupt backup.
- Completion races involving stage, temp-stage, failed-new, missing rollback, stale token, or changed journal phase block completion and listener readiness.
- `rollback-completed` accepts no residual, only stage, or only failed-new; it rejects stage plus failed-new, temp-stage, or retained rollback.

## Fault-Injection Results

These filter counts overlap and must not be summed. Every listed group passed.

| Filter/group | Passed |
|---|---:|
| `state_persist` | 10 |
| `state_load` | 17 |
| `staged_transaction` | 4 |
| `provider_transaction` | 12 |
| `file_transaction` | 11 |
| `streaming_transaction` | 14 |
| `backup_` | 41 |
| `restore_dry_run` | 9 |
| `restore_stage` | 11 |
| `restore_staging` | 35 |
| `restore_validation` | 33 |
| `restore_checksum` | 10 |
| `restore_path` | 10 |
| `restore_stage_capacity` | 3 |
| `restore_commit` | 57 |
| `restore_journal` | 12 |
| `restore_rollback` | 12 |
| `restore_recovery` | 82 |
| `restore_startup` | 24 |
| `restore_reconciliation` | 22 |
| `restore_crash` | 12 |
| Complete Rust suite | 433 |

Additional targeted filters passed: secret compensation 3, managed blob 1, stream event order 4, stop transaction 4, regenerate transaction 4, and backup checksum 3.

## Static Security Audit

- Restore errors use fixed redacted text and map path-bearing I/O failures to fixed categories.
- No restore log prints an app-data path, operation ID, journal/state content, original filename, storage key, secret reference, request/response body, stream delta, or blob/secret bytes.
- Existing reviewed logs contain only fixed operation context, fixed persistence stages, schema numbers, or the loopback listener address.
- Package/stage/commit/recovery paths derive from a canonical trusted parent, fixed names/prefixes, and program-generated validated operation IDs.
- Absolute package child paths, traversal, backslashes, links/reparse points, case-fold collisions, unknown entries, multiple operations, and existing destinations are rejected.
- No real app data, real backup, real user file, real Provider, API key, or `mock-api/secrets/*.bin` was used.

## Remaining Residual Risks

- Windows parent-directory sync remains best effort; complete power-loss atomicity is not claimed.
- JSON state and DPAPI/SecretStore are not one cross-resource atomic transaction.
- State and managed blobs retain documented crash-only orphan windows.
- Rollback, journal, stage, and failed-new artifacts are not automatically cleaned.
- Completed journals and rollback snapshots remain retained.
- Mode A can leave attachment metadata unavailable because blobs are intentionally absent.
- API keys must be re-entered after a portable restore.
- Active streams cannot resume after restart.
- No user-facing restore orchestration exists.
- Windows artifacts remain unsigned.
- Mode C and portable secret restoration remain deferred.

## Final Result

**Phase 12 backend acceptance: PASS.**

**Backend safety chain accepted. User-facing restore unavailable.**
