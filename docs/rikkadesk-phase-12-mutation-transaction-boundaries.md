# RikkaDesk Phase 12 Mutation Transaction Boundaries

This document began as the Phase 12 P1-C0 audit and design for runtime mutation transactions. It now records the completed P1-C1 pure-state implementation, P1-C2 Provider/SecretStore compensation, and P1-C3 managed file/blob compensation while retaining network and streaming boundaries for P1-C4.

P1-C1 through P1-C3 change the Rust mock API transaction path only. They do not add backup/restore, read real app data or secret blobs, change `schemaVersion: 6`, change provider import/export version 4, or enable real-provider image input by default.

## Current State Model

Persisted data is split across these live fields in `MockApiState`:

- `settings: RwLock<Value>`
- `conversations: RwLock<HashMap<String, ConversationDto>>`
- `providers: RwLock<Vec<DesktopProviderConfig>>`
- `files: RwLock<Vec<ManagedFileMetadata>>`
- `id_seq: AtomicU64`

Runtime-only data is separate:

- `generating_flags: RwLock<HashSet<String>>`
- `conversation_txs: RwLock<HashMap<String, broadcast::Sender<SsePayload>>>`
- settings/list broadcast senders
- SSE `seq: AtomicU64`
- HTTP client
- SecretStore
- Provider/SecretStore transaction mutex
- Provider secret-ref sequence
- managed blob store and file/blob transaction mutex
- persistence/save mutex and file operations

P1-A serializes writes and atomically replaces the state file. P1-B makes startup and migration fail closed. P1-C1 stages pure settings/conversation mutations, persists the explicit staged snapshot, and commits live state only after persistence succeeds. P1-C2 applies the same state transaction to Provider metadata and coordinates it with copy-on-write SecretStore operations. P1-C3 publishes managed blobs before one staged metadata commit, compensates handled upload failures, and uses durable tombstones before physical deletion. Streaming mutations remain the transitional P1-C4 path.

## P1-C1 Implementation Status

Implemented pure-state handlers:

- `POST /api/settings/assistant` via `update_assistant`
- `POST /api/settings/assistant/model` via `update_assistant_model`
- `POST /api/settings/favorite-models` via `update_favorite_models`
- `POST /api/conversations/{id}/title` via `update_conversation_title`
- `POST /api/conversations/{id}/pin` via `toggle_conversation_pin`
- `DELETE /api/conversations/{id}` via `delete_conversation`
- `POST /api/conversations/{id}/messages/{message_id}/edit` via `edit_message`
- `DELETE /api/conversations/{id}/messages/{message_id}` via `delete_message`

The implementation adds one process-local `mutation_transaction_mutex`, one live `commit_barrier`, a cloneable `PersistedMockState`, explicit staged-snapshot persistence, validation before persistence, and a runtime-only monotonic transaction revision. The revision is not serialized and does not change schema v6.

`conversation_detail` and `conversation_stream` no longer call a get-or-create helper. A missing ID receives a virtual empty DTO for compatibility with the frontend's navigate-before-POST new-chat flow. The virtual DTO is not inserted into conversations, does not advance `id_seq`, and is not persisted. The stream sender remains runtime-only.

P1-C1 success is published only after disk persistence and the live commit finish. Persistence or validation failure leaves pure live state, disk state, and revision unchanged and emits no success update.

Transitional Category B/D writers use the same mutation mutex and commit barrier for their short live-state write/persist window. This prevents interleaving with a staged commit and prevents an older persistence snapshot from overwriting a newer transaction. P1-C3 now owns the managed file/blob paths; it prevents handled failures from creating active dangling metadata but intentionally does not reconcile crash-only orphans. P1-C4 still owns streaming durability.

## P1-C2 Implementation Status

All Provider mutations now acquire one process-local `provider_secret_transaction_mutex`. SecretStore prepare/cleanup runs without component locks, the commit barrier, or the global state mutation mutex. The final Provider metadata mutation runs through `transact_persisted_state` with the Provider scope, so persistence failure leaves live Provider/settings state, durable state, `id_seq`, and revision unchanged.

Implemented Provider handlers:

- `POST /api/desktop/providers/import/confirm`
- `POST /api/desktop/providers`
- `POST /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}`

API key create/update uses a newly generated immutable `secretRef`; no existing encrypted blob is overwritten in place. A new blob is prepared first, the staged Provider metadata is persisted and committed, and the old blob is cleaned only after commit. State rejection compensates by deleting the operation-created blob. Blank-key upsert preserves the existing reference and performs no SecretStore write. Clear rotates to a new empty reference and delete removes Provider metadata; both defer old-blob deletion until after state commit. Import ignores source `hasSecret`, never accepts/restores a source `secretRef`, creates no secret blob, and returns `hasSecret: false`.

Secret compensation and cleanup use a bounded three-attempt idempotent delete. Old-secret cleanup failure after a successful state commit is logical success with a fixed redacted warning. The committed state does not reference the old encrypted blob, so it is an orphan rather than a dangling active reference. No complex partial-success UI was added. Handled state failures compensate operation-created blobs; if all compensation attempts are prevented by an underlying SecretStore failure, the API returns a distinct fixed safe error and requires later reconciliation.

JSON state and DPAPI/keyring storage are not one atomic resource. A process crash after a new encrypted blob is prepared but before state commit, or after state commit but before old-blob cleanup, can leave an unreferenced encrypted orphan. Startup orphan discovery/reconciliation is deferred. P1-C2 does not claim portable secret backup or complete cross-resource atomicity.

## P1-C3 Implementation Status

`POST /api/files/upload` and `DELETE /api/files/{id}` now use one process-local `file_blob_transaction_mutex`. Managed blob I/O happens outside component locks, the commit barrier, and the global state mutation mutex. File metadata is committed through `transact_persisted_state` with a dedicated validation scope, preserving the fixed outer lock order: file/blob mutex, blob work, then the short staged state transaction.

Uploads are atomic at the accepted batch level. Every item is validated before blob publication. Each blob receives a program-generated storage key independent of its numeric file ID, and is written to a unique same-directory `blob.tmp.<pid>.<counter>` file with create-new, `write_all`, `flush`, `sync_all`, close, and no-overwrite publish semantics. Only after all blobs are final does one staged transaction allocate file IDs, append all metadata, persist, and commit live state. Blob write/publish failure commits no metadata and cleans prior batch blobs. Metadata persistence failure leaves live state, disk state, `id_seq`, and revision unchanged and performs a bounded three-attempt cleanup of all operation-created final blobs. Exhausted compensation returns a fixed safe failure and can leave only an unreferenced, API-inaccessible orphan.

Deletion now checks numeric `metadata.fileId` references across all persisted conversation message parts inside the staged state transaction. An active reference returns conflict without changing metadata or the blob. An unreferenced file is first durably tombstoned with `deletedAt`; only after durable/live commit is its physical blob removed. Tombstone persistence failure leaves the active metadata and blob unchanged. Physical cleanup failure after commit remains logical success, records one fixed redacted warning, and leaves an inaccessible orphan for bounded retry on repeated DELETE. Repeated DELETE is idempotent.

Metadata/path reads reject missing and tombstoned entries. Path reads and delete operations share the file/blob mutex, so a committed tombstone cannot continue serving bytes. A blob without metadata has no API route. Message send also verifies every numeric managed file reference is active while serialized with state mutations, preventing a new persisted reference from racing a tombstone. Message/conversation deletion does not auto-delete blobs; conservative orphan reconciliation/GC remains deferred.

State and blob storage are not one crash-atomic resource. A process crash between final blob publish and metadata commit can leave an unreferenced blob. A crash between durable tombstone commit and physical deletion can leave tombstoned metadata plus an inaccessible physical blob. P1-C3 does not claim complete power-loss atomicity or automatic orphan cleanup.

## Mutation Call-Site Inventory

| Endpoint or task | Live structures changed | Persistence | External/runtime side effect | Current persistence-failure behavior | Safe direct rollback? | Category |
|---|---|---|---|---|---|---|
| Provider import confirm | providers, derived settings, `id_seq` | Yes | settings SSE after commit; no SecretStore write | Failed stage leaves live/disk/revision unchanged | Implemented staged Provider transaction | C |
| Provider upsert without key | providers, derived settings/favorites/current model, `id_seq` | Yes | settings SSE after commit; existing secret retained | Failed stage leaves live/disk/revision and secret unchanged | Implemented staged Provider transaction | C |
| Provider upsert with key | providers, derived settings, `id_seq`, rotated `secretRef` | Yes | New encrypted blob prepared; old blob cleaned after commit | Failed stage deletes the new blob and retains old state/secret | Implemented copy-on-write compensation | C |
| Provider API key update | Provider/derived settings with rotated `secretRef` | Yes | New encrypted blob prepared; old blob cleaned after commit | Failed stage deletes new blob and retains old state/secret | Implemented copy-on-write compensation | C |
| Provider API key clear | Provider/derived settings with new empty `secretRef` | Yes | Old blob deleted after state commit | Failed stage keeps old reference/blob and `hasSecret: true` | Implemented commit-then-cleanup | C |
| Provider delete | providers and derived settings/favorites/current model | Yes | Old blob deleted after commit; settings/list SSE after commit | Failed stage leaves provider and secret intact | Implemented commit-then-cleanup | C |
| Assistant selection | settings | Yes | settings/list SSE | P1-C1 discards failed stage; live/disk/revision remain old | Implemented staged transaction | A |
| Current assistant model | settings and assistant model field | Yes | settings SSE | P1-C1 discards failed stage; live/disk/revision remain old | Implemented staged transaction | A |
| Favorite models | settings | Yes | settings SSE | P1-C1 discards failed stage; live/disk/revision remain old | Implemented staged transaction | A |
| Conversation detail/stream missing-ID read | None in persisted state | No | Runtime-only stream sender | Returns a virtual DTO without inserting or saving | Implemented pure GET | Read |
| Send user message | conversations, title/mode/lorebook/generating state, `id_seq` | Yes before provider work | conversation/list SSE, then local reply or provider task | User turn remains live if initial save fails; provider is not started | Stage initial turn; do not rollback live | B |
| Stop conversation | conversations plus runtime generation flag | Yes | Stops runtime processing before save; conversation/list SSE after save | Live/runtime stop remains if save fails | Needs staged state plus generation token ordering | B |
| Conversation title | conversations | Yes | conversation/list SSE | P1-C1 leaves live/disk old on failure | Implemented staged transaction | A |
| Pin/unpin | conversations | Yes | conversation/list SSE | P1-C1 leaves live/disk old on failure | Implemented staged transaction | A |
| Conversation delete | conversations | Yes | Runtime generation/sender cleanup after commit; list SSE after commit | Failed stage leaves conversation and runtime resources intact | Implemented staged transaction | A |
| Message edit | conversations | Yes | conversation/list SSE | P1-C1 leaves live/disk old on failure | Implemented staged transaction | A |
| Message delete | conversations | Yes | conversation/list SSE | P1-C1 leaves live/disk old on failure | Implemented staged transaction | A |
| Regenerate preparation | conversations, truncation/generating state | Yes | SSE, then local reply or provider task | Live history remains truncated if save fails | Stage and commit before any task | B |
| Append local assistant reply | conversations, `id_seq` | Yes | conversation/list SSE | Live reply remains if save fails | Stage as short A/B transaction | B |
| Append empty streaming reply | conversations, `id_seq` | Yes | SSE, then generation flag/network task | Live placeholder remains if save fails | Stage before starting network | B |
| Append stream text delta | conversations | No | Snapshot SSE for each delta | UI/readers see text not yet durable | Must become transient runtime buffer or checkpoint transaction | B |
| Stream provider failure text | conversations | Only through later finish save | Snapshot SSE before final save | Failure text can be visible but not durable | Commit as one final short transaction | B |
| Finish stream | conversations and runtime generation flag | Yes | Stops runtime flag before save; final SSE after save | Live finished state remains; disk may still show generating; safe stage-only log | Needs generation token and retry/error event | B |
| Upload file | files, `id_seq` | Yes | Entire accepted batch is published before one staged metadata commit | Failed blob work commits no metadata; failed state commit compensates all new blobs and leaves live/disk/revision unchanged | Implemented publish/stage/compensate transaction | D |
| Delete file | files tombstone | Yes | Durable tombstone precedes physical blob cleanup | Failed tombstone persistence retains active metadata/blob; post-commit cleanup failure is logical success with inaccessible orphan | Implemented tombstone then cleanup | D |
| Attachment references in messages | conversations | Through message transaction | References numeric `metadata.fileId` | Persisted references block ordinary DELETE; send rejects missing/tombstoned IDs | Implemented conservative reference policy | D |
| `id_seq` allocation | staged state or coordinated legacy counter | Included in full save | File IDs are allocated only in the staged metadata transaction; storage keys are independent | Failed file metadata commit does not publish staged file IDs; gaps elsewhere remain allowed | Never decrement; allow gaps, forbid duplicates | A-D |
| `savedAt` | staged persisted snapshot | Every save | None | Failed save does not publish timestamp | Generate for staged snapshot only | A-D |
| SSE event sequence | runtime `seq` only | No | Event ordering | Gaps are harmless | Runtime-only, outside transaction | Runtime |

Startup initialization, corrupt preservation, future-schema rejection, and schema 1-5 migration are Category E. P1-B already owns them; the runtime mutation helper must not replace or bypass P1-B.

## Transaction Categories

### Category A: Pure Persisted-State Mutation

This category changes settings, conversations, providers without secrets, file metadata without blob operations, or persisted counters only.

Required semantics:

1. Build and validate a staged state without changing live state.
2. Persist the staged snapshot through P1-A.
3. Commit staged fields to live state.
4. Emit SSE and return success only after live commit.
5. Persistence failure discards the stage; live state and events remain unchanged.

### Category B: State Plus Network Or Background Work

The initial user turn and assistant placeholder must commit before a provider task starts. No mutation, persistence, or component lock may remain held while waiting on the provider.

Each background result is a separate short transaction. Provider deltas are not durable success. Final content, stop, provider failure, and finish status need explicit commit semantics and generation identity checks.

### Category C: State Plus SecretStore

JSON state and DPAPI/keyring storage cannot be one filesystem transaction. These operations require a SecretStore prepare/apply/rollback/finalize contract or equivalent compensation. A normal state rollback alone cannot restore deleted or replaced credentials.

### Category D: State Plus Managed Blob

Blob publication/deletion and file metadata cannot be one state-file transaction. Upload requires compensation for newly published blobs. Delete should make metadata inaccessible before physical cleanup and retain enough durable information for retry.

### Category E: Startup And Migration

NotFound initialization, corrupt backup, future-schema handling, and migration remain exclusively under P1-B. They run before the API accepts mutations and do not use the runtime mutation transaction helper.

## Global Transaction Invariants

1. HTTP mutation success means the intended live state and durable state agree. Required external effects are complete or the response explicitly reports a durable pending state.
2. Persistence failure never returns success, never commits staged data to live state, and never emits a success snapshot/invalidation.
3. An external effect occurs only after state commit when that ordering is safe, or it is prepared/applied with a proven compensation path.
4. The global mutation mutex serializes snapshot, persistence, and commit. No component `RwLock` or commit barrier is held across disk sync, provider network wait, long-running stream, SSE send, SecretStore operation, or blob I/O. The mutation mutex is never held across network, SecretStore, blob I/O, or SSE.
5. Transaction N failure cannot restore or overwrite transaction N+1. Runtime persisted-state writers are globally serialized.
6. Readers never observe a partially committed multi-field state.
7. Provider calls start only after the initial user message and assistant placeholder required by that call are durable.
8. Success SSE is emitted after commit. Transient stream delta events must be explicitly distinguishable from durable snapshots.
9. State, paths, API keys, secret values, request bodies, and blob bytes never appear in transaction errors or logs.
10. ID gaps are allowed; duplicate IDs are forbidden. `id_seq` is never decremented during compensation.

## Implementation Approaches

### Option 1: Live Mutate, Persist, Then Roll Back

Advantages:

- Smaller initial code change.
- Existing handlers can retain much of their mutation code.

Risks:

- Readers can observe unpersisted changes during disk sync.
- Every mutation needs a correct previous-value snapshot.
- A rollback can overwrite a newer concurrent mutation unless all writers share a global mutex.
- Multi-lock provider/settings and conversation/runtime rollback is easy to make incomplete.
- Secret and blob side effects still need compensation; restoring JSON does not restore them.

Conclusion: do not use this as the general architecture. It is acceptable only for a tightly isolated runtime-only value where no persistence or external side effect is involved.

### Option 2: Stage, Persist, Commit Live

Recommended sequence:

1. Acquire the global mutation transaction mutex.
2. Acquire the commit barrier for a coherent read and clone the current persisted fields.
3. Release component read locks and the barrier.
4. Apply the mutation and validation to the staged snapshot.
5. Persist the staged snapshot with P1-A.
6. Acquire the commit barrier exclusively.
7. Replace live persisted fields under the fixed component lock order.
8. Publish `id_seq`/revision, then release component locks and the barrier.
9. Release the mutation mutex.
10. Emit SSE and return success.

Advantages:

- Failed persistence never requires live rollback.
- Readers see old committed state until the new state is durable.
- Global serialization prevents an older transaction from overwriting a newer one.
- Mutation functions become deterministic staged-state operations that are easy to test.

Costs:

- All persisted-state writers must migrate to one helper.
- Readers must obey the commit barrier until persisted fields move into one container.
- External side effects still need operation-specific compensation.

Conclusion: adopt Option 2.

## Recommended Runtime Architecture

### P1-C1 Minimal Safe Shape

Add runtime-only coordination primitives:

- `mutation_transaction_mutex: Mutex<()>`
- `live_commit_barrier: RwLock<()>`
- optional runtime `commit_revision: AtomicU64`

Define a cloneable staged state containing exactly:

- settings
- conversations
- providers
- managed file metadata
- staged/persisted `id_seq`

Do not include HTTP client, SecretStore, channels, generation flags, persistence internals, or SSE sequence in the staged state.

All readers that combine or expose persisted fields must hold the commit barrier for the short read. The barrier is not held during persistence. The mutation mutex remains held across stage, persistence, and live commit, but it guards writers only and does not block old-state reads during disk sync.

### Long-Term Shape

Converge persisted live data into one `RwLock<PersistedRuntimeState>` while keeping runtime-only fields outside it. A single container removes mixed component commits and most commit-barrier plumbing. This refactor should follow the staged transaction API rather than precede it.

Current independent component locks are not sufficient by themselves. A writer mutex orders writers, but readers could still observe settings from one revision and providers from another during multi-lock commit.

## Fixed Lock Order

The transition design uses this order:

```text
mutation_transaction_mutex
  -> live_commit_barrier (read, clone only; then release)
    -> settings read
    -> conversations read
    -> providers read
    -> files read
  -> release component reads and barrier
  -> persistence/save mutex inside P1-A
  -> release persistence/save mutex
  -> live_commit_barrier (write, commit only)
    -> settings write
    -> conversations write
    -> providers write
    -> files write
    -> publish id_seq and commit revision
  -> release component writes and barrier
  -> release mutation_transaction_mutex
  -> SSE/UI send
```

Rules:

- Never acquire the mutation mutex while holding the persistence mutex.
- Never call the P1-A snapshot helper from inside a staged transaction; persist the already-built staged snapshot directly.
- Never hold component locks while awaiting persistence, SecretStore, blob I/O, network, or SSE.
- Background and HTTP mutations use the same transaction helper and lock order.
- Runtime-only generation/channel cleanup happens after persisted commit unless an operation-specific rule says otherwise.

## Commit Visibility

Without a barrier, replacing settings, conversations, providers, and files one lock at a time exposes a mixed revision. P1-C1 should require a short read barrier for all API/SSE payload builders and a write barrier for live commit.

Readers may continue to see the old committed revision while the staged snapshot is being synced. After persistence succeeds, the write barrier makes the live swap appear as one revision to participating readers. No barrier is held over disk sync.

The optional runtime revision helps tests and diagnostics reject stale background results. It is not a substitute for the transaction mutex or read barrier and does not need to enter schema v6.

## ID And Timestamp Policy

- ID gaps are explicitly allowed.
- `id_seq` is never rolled back or decremented, because reuse after partial external effects is more dangerous than gaps.
- A failed transaction must not expose its generated IDs through HTTP success or success SSE.
- A later successful transaction persists the highest reserved counter.
- A crash can reuse an uncommitted ID only if no durable state or uncompensated external artifact references it. Categories C/D must finish compensation before treating a failed ID as uncommitted.
- `savedAt` is generated for the staged snapshot immediately before persistence and becomes live only after commit.
- SSE sequence remains runtime-only and may contain gaps.

## SecretStore Transaction Boundary

### Ordering Alternatives

**A. Secret first, staged state persist second, compensate secret on failure**

- Prevents durable state from pointing to a secret that was never created.
- Requires reversible replacement/deletion and restoration of the previous secret.
- Is the preferred base ordering when a provider config and key are submitted together.

**B. State first, secret second, then a second state compensation commit**

- Can leave durable state pointing to an absent secret between commits or after a crash.
- Requires another state transaction on secret failure.
- Exposes a larger inconsistency window and is not recommended as the default.

### Implemented P1-C2 Contract

P1-C2 uses copy-on-write secret references instead of retaining plaintext rollback data or overwriting the active encrypted value. All helper failures are converted to fixed safe errors; API keys, encrypted bytes, refs, storage keys, and paths are not logged.

Operation rules:

- New provider plus key: create a unique ref, prepare its encrypted blob, persist staged provider/settings, commit live, then return success. State failure deletes only the new blob.
- Existing provider plus replacement key: prepare a unique new ref/blob, stage the Provider to the new ref, commit, then delete the old ref/blob. State failure deletes the new blob; the old ref/blob remains active.
- Blank-key Provider upsert: retain the existing ref and secret; a new Provider gets a deterministic empty ref and no blob.
- API key clear: stage a new empty ref, persist and commit, then delete the old blob. State failure leaves the old ref/blob active.
- Provider delete: persist and commit Provider/settings removal first, then delete the old blob. State failure leaves both Provider and secret active.
- Import confirm: import only validated non-sensitive metadata, generate local IDs/empty refs, ignore source `hasSecret`, and perform no SecretStore write.
- New-secret compensation failure: return a distinct fixed safe 5xx and no success event. Old-secret cleanup failure after commit keeps the logical state success, emits one fixed warning, and leaves an unreferenced encrypted orphan for future reconciliation.

On Windows, `secret_exists` checks the controlled blob path without reading or decrypting blob content. `get_secret` remains the only provider-runtime path that reads and DPAPI-decrypts a configured API key. P1-C2 tests replace the real store with an in-memory synthetic store.

## Managed Blob Transaction Boundary

### Upload

Implemented P1-C3 sequence:

1. Validate the complete accepted upload batch before blob publication.
2. Acquire the file/blob transaction mutex and generate storage keys without consuming file IDs.
3. Write and sync each blob to a unique controlled temp, close it, and publish to a non-existing final managed name.
4. If any blob step fails, clean all temps/finals created by this batch and commit no metadata.
5. Build all file metadata and allocate numeric IDs in one staged state transaction.
6. Persist the staged state and commit live metadata.
7. On state failure, delete only final blobs created by this operation with bounded retry; record a fixed safe compensation warning if deletion remains blocked.
8. On success, return every file response as one batch.

This ordering prefers an unreferenced orphan over durable metadata that points to a missing blob. Handled failures normally remove operation-created blobs; an exhausted cleanup or process crash can still leave an orphan. Orphans have no numeric metadata route and are never exposed through the file API.

### Delete

P1-C3 does not delete the blob first. It scans persisted message parts for the numeric file ID, then persists and commits a metadata tombstone before physical deletion:

- Active message reference: return conflict; metadata and blob stay active.
- Tombstone persistence failure: return failure; live/disk metadata and blob stay active.
- Blob deletion success or already missing: return logical success.
- Blob deletion failure: retain the durable tombstone, deny metadata/path reads, return logical success, emit a fixed warning, and retry cleanup on repeated DELETE.

Message and conversation deletion intentionally do not auto-GC their former attachments. This prevents accidental deletion of shared or conservatively referenced blobs. Orphan discovery, reconciliation, and garbage collection remain deferred to Phase 12 P6 or a dedicated maintenance phase.

The process-crash windows remain final publish before metadata commit, and durable tombstone before physical delete. Both can leave inaccessible orphans, but ordinary handled failures do not leave active metadata pointing to a newly missing blob.

## Provider Network And Streaming Boundary

- The provider request starts only after the user turn and assistant placeholder required for generation are durable and committed live.
- No transaction, component, persistence, or commit-barrier lock is held during provider connection or streaming.
- Every final background mutation uses the same staged transaction helper as HTTP mutations.
- Each generation receives a conversation generation token/epoch. Stop, delete, regenerate, and late stream completion compare the token before committing so an older task cannot overwrite a newer turn.
- Provider/network errors and persistence errors remain distinct safe error classes.

### Delta Visibility

Current deltas mutate the persisted conversation object and broadcast snapshot events without persistence. P1-C4 should instead keep the in-progress text in a runtime-only generation buffer and emit explicitly transient `delta` events. A durable snapshot event is emitted only after a checkpoint/final staged commit.

Recommended initial policy:

- Persist the user turn and empty assistant placeholder before network start.
- Buffer deltas outside persisted live state.
- Optionally checkpoint at a bounded interval/size through short transactions.
- On DONE, provider error, or Stop, stage the accumulated text and final status, persist, commit, then emit final snapshot.
- If final persistence fails, emit a safe stream-persistence error event, do not emit final success, keep retryable runtime data, and never label it as provider failure.

Background streaming is handled separately in P1-C4.

## Endpoint Success And Error Semantics

| Endpoint/task | State commit failure | External side-effect failure | HTTP/SSE result |
|---|---|---|---|
| Provider import/upsert without key | Discard stage | None; existing secret is untouched | Safe 5xx; no settings success event |
| Provider upsert/key update with key | Discard stage; delete operation-created blob | Prepare/compensation failure is distinct and redacted | Safe 5xx; no provider response/event |
| Key clear | Discard stage and retain old ref/blob | Post-commit old-blob cleanup failure is orphan-only partial success | State failure is safe 5xx; committed clear returns success with fixed warning only |
| Provider delete | Discard stage and retain old ref/blob | Post-commit old-blob cleanup failure is orphan-only partial success | State failure is safe 5xx; committed delete remains success |
| Assistant/model/favorites | Discard stage | None | Safe 5xx; no settings event |
| Title/pin/edit/delete message | Discard stage | None | Safe 5xx; no conversation/list event |
| Conversation delete | Discard stage; retain runtime sender/generation until commit | Runtime cleanup failure is local and retryable | Safe 5xx; no delete invalidate |
| Send initial turn | Discard stage | Provider not started | Safe 5xx; no accepted response/success snapshot |
| Regenerate preparation | Discard stage | Provider not started | Safe 5xx; old history remains visible |
| Stream placeholder | Discard stage | Provider not started | Safe persistence error; no generation start |
| Stream delta | No durable success until checkpoint/final | Network error becomes final staged result | Transient delta only; never durable snapshot claim |
| Stream finish/failure | Keep runtime buffer for retry; no final commit | Network and persistence errors stay distinct | Safe SSE error; no final-success snapshot |
| Stop | Failed staged stop leaves prior durable generating state | Runtime cancellation uses generation token | Safe 5xx or retry status; no stopped success event |
| File upload | Discard metadata stage; delete operation-created blobs | Cleanup failure records orphan safely | Safe 5xx; no file response |
| File delete | Discard failed tombstone stage and retain active metadata/blob | Post-commit blob cleanup failure leaves an inaccessible orphan | Reference conflict is 409; committed tombstone remains logical success with fixed warning |

Errors may include fixed operation/stage codes and non-sensitive numeric IDs only when needed. They must not contain state JSON, local paths, API keys, secret values/references, request bodies, or blob content.

## P1-C Implementation Split

### P1-C1: Pure State Staged Transactions (Completed)

Scope:

- Global mutation mutex and live commit barrier.
- Cloneable staged persisted state and deterministic mutation functions.
- Assistant/current model/favorites.
- Conversation title, pin, delete, message edit/delete, and lazy conversation creation policy.
- State-only ID/timestamp semantics.
- Persist before live commit; commit before events.

Provider import/upsert remains outside P1-C1 even when a request omits a key, so that all provider state and SecretStore ordering can be resolved together in P1-C2. Exclude SecretStore compensation, blob transactions, send/regenerate/stop semantics, and streaming finalization.

### P1-C2: Provider And SecretStore Compensation (Completed)

Scope:

- Provider import and blank-key metadata upsert through staged state.
- Provider upsert with key.
- Key replacement and clear semantics.
- Provider deletion.
- Provider-specific serialization, copy-on-write refs, state-failure compensation, and safe cleanup errors.

Completed P1-C2 prevents handled persistence failures from leaving state pointing to a newly missing secret and prevents key clear/delete from destroying the old secret before state durability. Process-crash orphan discovery and deletion are intentionally deferred; P1-C2 does not scan secret storage.

### P1-C3: File Blob Transaction And Compensation (Completed)

Scope:

- Atomic accepted-batch upload publication plus one staged metadata/ID commit.
- Unique create-new temp files and non-overwriting final storage keys independent of file IDs.
- State-failure blob compensation with bounded retry and fixed redacted failures.
- Durable tombstone before physical deletion, including idempotent cleanup retry.
- Numeric persisted-reference protection and send-time active attachment checks.
- API-inaccessible orphan policy; automatic reconciliation/GC remains deferred.

### P1-C4: Background Streaming Transaction Safety

Scope:

- Initial send/regenerate transaction.
- Durable assistant placeholder before network.
- Runtime-only delta buffer or bounded checkpoints.
- Generation tokens for stop/delete/regenerate races.
- Final/failure persistence, retry visibility, and SSE ordering.
- Proof that no transaction lock is held during network wait.

## Synthetic Test Plan

### Pure State

- Persistence failure leaves live state unchanged.
- Successful staged transaction updates live and disk to the same revision.
- Concurrent mutations serialize.
- An older failed transaction cannot overwrite a newer commit.
- Readers do not observe a staged failed state or mixed component commit.
- Event observers receive no success before commit.
- ID uniqueness is preserved and gaps are accepted.

### Provider Secrets

- New secret write plus state failure deletes the new secret.
- Existing secret replacement plus state failure deletes the new blob and retains the prior ref/blob.
- Secret apply failure does not commit provider state.
- Key clear/provider delete state failure never deletes the prior blob.
- Old-secret cleanup failure after commit keeps logical success and leaves only an unreferenced orphan.
- Compensation/cleanup errors are fixed and redacted.
- No orphan secret or state reference to an unexpectedly missing secret remains after successful compensation.

### Files

- Single and batch upload success publish every blob and metadata item with unique IDs.
- Blob write/publish failure commits no metadata and cleans prior batch blobs.
- Blob publication plus metadata failure removes only operation-created blobs and leaves live/disk/revision unchanged.
- Compensation cleanup exhaustion creates only an API-inaccessible orphan and returns a fixed safe failure.
- Tombstone persistence failure preserves active metadata and blob.
- Post-tombstone blob cleanup failure is logical success; GET remains blocked and repeated DELETE retries cleanup.
- Image/document numeric references block ordinary DELETE; malformed legacy URLs do not identify another file.
- Concurrent upload, Category A, Provider, delete, and GET cases preserve lock ordering and complete without lost updates or deadlock.
- The P1-C3 implementation adds 22 synthetic file/blob tests; the complete Rust suite contains 103 passing tests at completion.

### Streaming

- Initial persistence failure means no provider call.
- Placeholder persistence failure means no provider call.
- Delta events are marked transient.
- Final persistence failure emits no final success.
- Stop and finish racing with the same generation token produce one final commit.
- A late older stream cannot modify a regenerated conversation.
- No transaction/component lock is held during a synthetic network wait.

All tests must use synthetic state, SecretStore fakes, managed blob temp directories, and local capture servers only. They must not read real app data, encrypted secret blobs, or real API keys.

## Backup Mode Gate

Do not implement Mode A/B backup packaging until these gates pass:

```text
P1-C1 pure state transaction safety
  -> P1-C2 provider secret consistency (completed)
  -> P1-C3 file/blob handled-failure consistency (completed)
  -> P1-C4 streaming transaction safety
  -> Mode A/B backup package
```

Reason: a backup snapshot cannot be represented as consistent while runtime writers can expose uncommitted state, SecretStore operations can leave cross-resource inconsistencies, blobs can be missing/orphaned, or streaming can mutate persisted objects outside transaction boundaries.

## P1-C1 Through P1-C3 Conclusions

- Recommended architecture: stage, persist, then commit live.
- Global mutation mutex required: Yes.
- Commit read/write barrier required during the multi-lock transition: Yes.
- Current component locks sufficient alone: No.
- Long-term single persisted-state container recommended: Yes.
- Live mutate then rollback recommended: No, except isolated runtime-only values.
- SecretStore included in P1-C1: No.
- Provider/SecretStore compensation implemented in P1-C2: Yes, with copy-on-write refs and handled-failure compensation.
- State and SecretStore fully atomic across process crashes: No; encrypted orphan windows remain documented.
- File/blob operations included in P1-C1: No; P1-C3 now implements their coordinated handled-failure semantics.
- State and managed blobs fully atomic across process crashes: No; upload/delete orphan windows remain documented and reconciliation is deferred.
- Ordinary handled file failures leave active dangling metadata: No; failed upload state commits compensate new blobs, and failed delete state commits retain the original blob.
- Referenced managed file ordinary DELETE permitted: No; persisted numeric references return conflict.
- Message/conversation deletion auto-GCs managed blobs: No; conservative orphan cleanup is deferred.
- Background streaming handled separately: Yes, in P1-C4.
- ID policy: gaps allowed, duplicates forbidden, never decrement `id_seq`.
- Pure settings/conversation staged transactions implemented: Yes.
- Missing conversation GET mutates persisted state: No.
- Runtime revision persisted in schema v6: No.
- Recovery/backup modes implemented by P1-C1/P1-C2/P1-C3: No.

Recommended next step: P1-C4 background streaming transaction safety. Mode A/B backup packaging remains blocked through P1-C4.
