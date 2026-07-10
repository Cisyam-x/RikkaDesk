# RikkaDesk Phase 12 Mutation Transaction Boundaries

This document began as the Phase 12 P1-C0 audit and design for runtime mutation transactions. It now also records the completed P1-C1 implementation for pure persisted-state mutations while retaining the SecretStore, managed blob, network, and streaming boundaries for later phases.

P1-C1 changes the Rust mock API transaction path only. It does not add backup/restore, read real app data or secret blobs, change `schemaVersion: 6`, change provider import/export version 4, or enable real-provider image input by default.

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
- persistence/save mutex and file operations

P1-A serializes writes and atomically replaces the state file. P1-B makes startup and migration fail closed. P1-C1 now stages pure settings/conversation mutations, persists the explicit staged snapshot, and commits live state only after persistence succeeds. Provider/SecretStore, file/blob, and streaming mutations remain transitional paths; their short state-write windows are serialized with P1-C1, but their cross-resource compensation is still pending.

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

Transitional Category B/C/D writers now use the same mutation mutex and commit barrier for their short live-state write/persist window. This prevents interleaving with a P1-C1 commit and prevents an older persistence snapshot from overwriting a newer transaction. It does not compensate a SecretStore change, remove an orphan blob, restore a deleted blob, or make streaming deltas durable. A failed transitional writer can still leave its live component ahead of disk; P1-C2, P1-C3, and P1-C4 own those remaining semantics.

## Mutation Call-Site Inventory

| Endpoint or task | Live structures changed | Persistence | External/runtime side effect | Current persistence-failure behavior | Safe direct rollback? | Category |
|---|---|---|---|---|---|---|
| Provider import confirm | providers, derived settings, `id_seq` | Yes | settings SSE | Transitional writer is serialized, but failed persistence can leave live state ahead | Defer with the provider domain | C2 |
| Provider upsert without key | providers, derived settings/favorites/current model, `id_seq` | Yes | settings SSE | Transitional writer is serialized, but failed persistence can leave live state ahead | Defer with the provider domain | C2 |
| Provider upsert with key | providers, derived settings, `id_seq` | Yes | SecretStore write happens first | Secret can change and live state remains even when state save fails | Requires secret compensation and staged state | C |
| Provider API key update | None in JSON state; existing `secretRef` is read | No | SecretStore replace | SecretStore error returns 500; successful secret change is immediate | SecretStore-owned atomic replace | C |
| Provider API key clear | None in JSON state; existing `secretRef` remains | No | SecretStore delete | Delete error returns 500; successful clear is immediate | Intentional missing secret, not state rollback | C |
| Provider delete | providers and derived settings/favorites/current model | Yes | SecretStore delete happens first; settings/list SSE after save | Save failure can leave live provider removed while disk still references a now-missing secret | Unsafe without reversible secret delete | C |
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
| Upload file | files, `id_seq` | Yes | Blob files are created first | State save failure leaves live metadata and blobs; no response success | Requires blob compensation/staging | D |
| Delete file | files tombstone | Yes | Blob is deleted first | Save failure leaves disk metadata active but blob missing | Reverse order; tombstone then deferred cleanup | D |
| Attachment references in messages | conversations | Through message transaction | References managed file IDs/URLs | File deletion can make committed messages unavailable | Requires reference policy in C3 | D |
| `id_seq` allocation | atomic counter | Included in later full save | IDs can be used in response/state/blob names | Failed mutations consume in-process IDs | Do not decrement; allow gaps, forbid duplicates | A-D |
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

### Recommended SecretStore Contract

P1-C2 should design a non-logging operation object with `prepare`, `apply`, `rollback`, and `finalize` semantics. On Windows, reversible operations should use encrypted blob staging/quarantine rather than placing plaintext secrets in rollback state. Cross-platform keyring behavior needs an equivalent contract or must explicitly report that an operation cannot be made reversible.

Operation rules:

- New provider plus key: prepare/create secret, persist staged provider/settings, commit live, finalize secret operation. State failure deletes the newly created secret.
- Existing provider config plus replacement key: preserve a reversible encrypted prior value, apply replacement, persist staged state, commit, finalize. State failure restores the prior encrypted value.
- API key update only: no JSON transaction is needed because `secretRef` does not change, but SecretStore replacement itself must be atomic.
- API key clear only: delete failure returns 500. A successful clear intentionally leaves provider config and `secretRef` with `hasSecret: false`; that is not a dangling state error.
- Provider delete: prepare a reversible secret deletion, persist staged provider/settings removal, commit live, then finalize deletion. State failure restores the secret before returning failure.
- Compensation failure: return a distinct safe 5xx, emit no success event, retain a fixed-stage reconciliation marker, and do not log secret material or paths.

SecretStore does not belong in P1-C1.

## Managed Blob Transaction Boundary

### Upload

Recommended P1-C3 sequence:

1. Validate and reserve IDs/storage keys.
2. Write and sync each blob to a controlled transaction temp.
3. Publish blobs to final managed names.
4. Build file metadata in staged state.
5. Persist staged state.
6. On state failure, delete only blobs created by this operation; record safe orphan cleanup if deletion fails.
7. On success, commit live metadata and return file responses.

This ordering prefers an unreferenced orphan over durable metadata that points to a missing blob. Orphans must never be exposed through the file API and need a referenced-file-aware cleanup policy.

### Delete

Do not delete the blob first. Persist and commit a metadata tombstone or pending-delete state before physical deletion. Then remove the blob:

- Blob deletion success: finalize deletion and return success.
- Blob deletion failure: keep the durable tombstone/pending cleanup state and return an explicit pending/failure result; retry later.
- Active message references: initially reject deletion unless the operation is a draft cancellation or the user explicitly accepts that committed attachments become unavailable.

P1-C0 defines this boundary only. Actual blob consistency, cleanup journal/tombstone shape, orphan scanning, and reference policy belong in P1-C3 and may be merged with the original Phase 12 P3 blob consistency work.

Files do not belong in P1-C1 except for proving that pure staged snapshots can carry unchanged file metadata.

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
| Provider import/upsert without key | Discard stage | None | Safe 5xx; no settings success event |
| Provider upsert with key | Discard stage; compensate secret | Secret apply/rollback failure is distinct | Safe 5xx; no provider response/event |
| Key update/clear | No state commit | SecretStore failure leaves old/known state | Safe 5xx; never return `hasSecret` success falsely |
| Provider delete | Discard stage; restore prepared secret deletion | Compensation failure requires reconciliation | Safe 5xx; no list/settings success event |
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
| File delete | Keep/commit tombstone policy; never restore stale live metadata blindly | Blob cleanup failure remains pending | Explicit pending/failure; no false physical-delete success |

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

### P1-C2: Provider And SecretStore Compensation

Scope:

- Provider upsert with key.
- Key replacement and clear semantics.
- Provider deletion.
- Reversible secret operation contract.
- Orphan/dangling-reference reconciliation and safe compensation errors.

### P1-C3: File Blob Transaction And Compensation

Scope:

- Upload staging/publication plus metadata commit.
- Delete tombstone/deferred cleanup.
- Referenced attachment policy.
- Missing/orphan reconciliation.
- Integration with the original Phase 12 P3 blob consistency work.

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
- Existing secret replacement plus state failure restores the prior encrypted value.
- Secret apply failure does not commit provider state.
- Provider delete state failure restores prepared secret deletion.
- Compensation failure produces a safe reconciliation state.
- No orphan secret or state reference to an unexpectedly missing secret remains after successful compensation.

### Files

- Blob publication plus metadata failure removes only operation-created blobs.
- Cleanup failure creates a safe orphan report, not exposed metadata.
- Metadata tombstone success plus blob failure remains retryable.
- Delete does not silently break committed message references.
- Orphan cleanup never removes referenced blobs.

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
  -> P1-C2 provider secret consistency
  -> P1-C3 file/blob consistency
  -> P1-C4 streaming transaction safety
  -> Mode A/B backup package
```

Reason: a backup snapshot cannot be represented as consistent while runtime writers can expose uncommitted state, SecretStore operations can leave cross-resource inconsistencies, blobs can be missing/orphaned, or streaming can mutate persisted objects outside transaction boundaries.

## P1-C1 Conclusions

- Recommended architecture: stage, persist, then commit live.
- Global mutation mutex required: Yes.
- Commit read/write barrier required during the multi-lock transition: Yes.
- Current component locks sufficient alone: No.
- Long-term single persisted-state container recommended: Yes.
- Live mutate then rollback recommended: No, except isolated runtime-only values.
- SecretStore included in P1-C1: No; handle in P1-C2.
- File/blob operations included in P1-C1: No; unchanged metadata may be carried, but operations belong in P1-C3.
- Background streaming handled separately: Yes, in P1-C4.
- ID policy: gaps allowed, duplicates forbidden, never decrement `id_seq`.
- Pure settings/conversation staged transactions implemented: Yes.
- Missing conversation GET mutates persisted state: No.
- Runtime revision persisted in schema v6: No.
- Recovery/backup modes implemented by P1-C1: No.

Recommended next step: P1-C2 Provider and SecretStore compensation. Mode A/B backup packaging remains blocked through P1-C4.
