# RikkaDesk Files / Attachments / Multimodal Plan

Review date: 2026-07-07

This document records the Phase 10 P1 safety design for files, attachments, and future multimodal input in RikkaDesk desktop. It is a design document only. It does not implement file upload, file preview, OCR, PDF parsing, multimodal provider calls, tools, Workspace, schema migration, or Tauri configuration changes.

Current recommended private beta tag:

```text
rikkadesk-v0.1.0-beta.11
```

## Background And Current State

RikkaDesk currently inherits message part types from the upstream web UI:

- `text`
- `image`
- `video`
- `audio`
- `document`
- `reasoning`
- `tool`

The current desktop UI can route message parts through `MessageParts` and render `image`, `video`, `audio`, and `document` parts with local React components. `ChatInput` also has frontend-only attachment entry points:

- Upload button.
- Image file picker.
- General file picker.
- Drag and drop.
- Paste file.
- Paste long text as file.

The current `ChatInput` frontend calls:

- `api.postMultipart("files/upload", formData)`
- `api.delete("files/{fileId}")`

However, the current desktop backend is still text-only:

- The mock API does not implement `/api/files/upload`.
- The mock API does not implement `/api/files/{id}`.
- The mock API does not implement `/api/files/path/*`.
- The mock API does not have a local file store.
- The mock API does not have managed file metadata.
- The OpenAI-compatible request builder only uses text parts.
- OpenAI-compatible requests remain text-only.
- Desktop provider models are injected with `inputModalities: ["TEXT"]` and `outputModalities: ["TEXT"]`.
- OCR, PDF parsing, document parsing, image input, audio input, video input, and multimodal provider calls are not implemented.

Conclusion: current RikkaDesk has a frontend attachment UI shell plus message part type/rendering shell, backed by a text-only desktop mock API. It is not a complete file, attachment, or multimodal feature.

## Implementation Status

Phase 10 P2 implemented the local desktop mock API file skeleton:

- `POST /api/files/upload`
- `GET /api/files/{id}`
- `GET /api/files/path/{id}`
- `DELETE /api/files/{id}`

The local mock state is now `schemaVersion: 5` and stores managed file metadata only. File blobs are stored under the app data file store. Provider calls remain text-only, and no attachment content, base64 payload, local absolute path, OCR text, or provider request body is stored in `state.v1.json`.

Phase 10 P3 aligned the inherited upload UI with the P2 skeleton:

- Image picker accepts PNG, JPEG, WEBP, and GIF only.
- Document picker accepts plain text and PDF only.
- Frontend detection rejects SVG, HTML, script-like text, audio, video, Office documents, archives, executables, and unknown binaries before upload when possible.
- Successful uploads render as attachment chips with file metadata.
- PDF and text files remain document chips only; there is no inline PDF, Office, HTML, or SVG preview.
- Raster images may render as image attachments, but they are not sent to a provider for image understanding.
- Attachment deletion calls the local `DELETE /api/files/{id}` skeleton for draft attachments.
- Multimodal provider calls, OCR, PDF/Office parsing, Workspace, MCP/tools, and search remain deferred.

Phase 10 P4 hardened attachment message rendering:

- Image and document parts now resolve only controlled managed file URLs under `/api/files/path/{id}`.
- Image previews require managed metadata with a matching `fileId` and a raster image MIME: PNG, JPEG, WEBP, or GIF.
- SVG, HTML, `data:`, `blob:`, `file:`, `javascript:`, external HTTP(S), arbitrary relative paths, and malformed managed file paths are blocked in message part rendering.
- PDF and text files remain document chips only, with no inline PDF, Office, HTML, or SVG preview.
- Missing or deleted files render a safe unavailable state instead of breaking the message UI.
- Multimodal provider calls remain deferred, and attachments are still not sent to OpenAI-compatible providers.

Phase 10 P5a adds model capability metadata and attachment send gating:

- The local mock state is now `schemaVersion: 6`.
- Desktop provider models store `inputModalities` and `outputModalities`.
- Existing and imported older provider models default to `inputModalities: ["TEXT"]` and `outputModalities: ["TEXT"]`.
- Provider Settings can mark a model with optional `IMAGE` input metadata; `TEXT` input and `TEXT` output remain required.
- Provider import/export is now version 4 and includes model modalities while still excluding API keys, `secretRef`, local file metadata, file blobs, base64 payloads, and app data paths.
- `ChatInput` blocks image attachments when the selected model is text-only and explains that attachments remain local-only in this beta.
- The backend guards `/messages` and `/regenerate` so any non-text message parts are saved locally and answered with a local-only attachment notice instead of calling a real provider.
- OpenAI-compatible request bodies remain text-only; image input provider calls are still deferred to P6.

Phase 10 P6.1 adds a dedicated OpenAI-compatible image input prototype design:

- The design is documented in `docs/rikkadesk-openai-compatible-image-input-plan.md`.
- P6.1 does not change code, schema, provider import/export, file APIs, or provider request builders.
- The recommended prototype path is OpenAI-compatible Chat Completions content array, not Responses API.
- The design requires IMAGE capability gating, explicit per-send confirmation, in-memory data URLs, and synthetic capture-server validation before any real-provider test.
- Attachments still are not sent to providers in the current implementation.

Phase 10 P6.2 adds only the backend internal vision request skeleton:

- OpenAI-compatible vision structs and a Chat Completions content-array body builder now exist internally.
- The builder is not connected to `/messages`, `/regenerate`, Test Connection, or streaming.
- Text-only chat and attachment local-only guards remain the active runtime behavior.
- No image blob is read, no file-derived base64 is generated, and no image request is sent to any provider.

## Phase 10 Goals And Non-Goals

Long-term Phase 10 goals:

- Design safe local file storage.
- Design file ids, file metadata, and URL serving.
- Allow attachment message parts to persist safely.
- Allow the UI to show file chips, image placeholders, and document chips.
- Later, support controlled image input to OpenAI-compatible providers.

Phase 10 P1 does not:

- Implement file upload.
- Implement file preview.
- Send images or files to a real provider.
- Implement OCR.
- Implement PDF parsing.
- Implement audio or video understanding.
- Implement Workspace file system access.
- Implement MCP, tools, or file tool calls.
- Read real user files.
- Save base64 file content to state.
- Send local paths to providers.
- Change `schemaVersion`.

## Safety Principles

The file system boundary must be explicit and conservative:

1. File content must not be stored in `state.v1.json`.
2. Large base64 objects must not be stored in `state.v1.json`.
3. Provider requests must not include local absolute paths.
4. Message parts may reference a file id or controlled API URL, but not raw file content.
5. File ids must not be usable for path traversal.
6. File serving endpoints must not accept arbitrary paths.
7. Uploads must enforce a single-file size limit, per-request file count limit, total store quota, and MIME allowlist.
8. MIME type must not rely only on browser-provided `File.type`; it needs server-side validation and magic byte checks where practical.
9. SVG, HTML, PDF, and Office previews require separate review.
10. SVG and HTML must not be executed inline.
11. PDF and Office files should initially render only as attachment chips.
12. File deletion behavior must be explicit.
13. Conversation export must not include file content by default.
14. Conversation import must not silently restore local file content.
15. Multimodal provider sending must be gated by model capabilities.
16. Users must explicitly select files; RikkaDesk must not scan user disks.
17. Drag/drop and paste flows need clear UI so private files are not uploaded accidentally.
18. Logs must not record real file paths.
19. Logs must not print file content, EXIF metadata, or OCR text.
20. File modules must not introduce SecretStore, DPAPI, provider key, or signing credential logic.

## Local File Store Draft

Suggested app data layout:

```text
%APPDATA%\com.cisyamx.rikkadesk\mock-api\files\
  blobs\
    <storageKey>
  thumbnails\
    <thumbnailKey>
```

Design notes:

- Store files inside RikkaDesk app data.
- Do not use the original file name as the on-disk file name.
- Use a backend-generated storage key such as `file_<uuid>` or `<sha256>_<random>`.
- Treat the original file name only as `displayName`.
- Build all paths from backend metadata.
- Never accept a real user path from the frontend API.
- Prefer endpoints that accept only a file id.

## Managed File Metadata Draft

There are two reasonable storage options.

### Option A: Store Metadata In `state.v1.json`

Pros:

- Simple persistence model.
- Same backup and migration path as conversations.
- Easy to keep conversation references and file metadata together.

Cons:

- Requires a future `schemaVersion: 5`.
- The state file grows with file metadata.
- Care is required to avoid absolute paths and file content.

### Option B: Store Metadata In `files.v1.json`

Pros:

- File index is isolated from conversation state.
- Provider/chat schema migrations are less coupled to file metadata.

Cons:

- More consistency work.
- More backup/restore complexity.
- Conversation references and file metadata can drift.

Recommended P2 direction: use `schemaVersion: 5` and store only non-sensitive file metadata in the existing state model. Do not store file content, base64 content, original absolute paths, OCR text, EXIF payloads, or provider request bodies.

Suggested metadata:

```json
{
  "id": 1,
  "storageKey": "file_00000000-0000-4000-8000-000000000000",
  "displayName": "image.png",
  "mime": "image/png",
  "sizeBytes": 12345,
  "sha256": "sha256-hex-placeholder",
  "kind": "image",
  "relativePath": "files/blobs/file_00000000-0000-4000-8000-000000000000",
  "thumbnailRelativePath": null,
  "createdAt": "2026-07-07T00:00:00Z",
  "updatedAt": "2026-07-07T00:00:00Z",
  "source": "upload",
  "deletedAt": null
}
```

Metadata must not contain:

- Original absolute path.
- File content.
- Base64 file content.
- API key.
- Provider request body.
- OCR text.
- Raw EXIF payload.
- SecretStore or DPAPI material.

## Message Part Draft

Current inherited types already support URL-like attachment parts.

Image part:

```ts
interface ImagePart {
  type: "image";
  url: string;
  metadata?: Record<string, unknown> | null;
}
```

Document part:

```ts
interface DocumentPart {
  type: "document";
  url: string;
  fileName: string;
  mime: string;
  metadata?: Record<string, unknown> | null;
}
```

Future image message part:

```json
{
  "type": "image",
  "url": "/api/files/path/1",
  "metadata": {
    "fileId": 1,
    "mime": "image/png",
    "sizeBytes": 12345
  }
}
```

Future document message part:

```json
{
  "type": "document",
  "url": "/api/files/path/2",
  "fileName": "demo.pdf",
  "mime": "application/pdf",
  "metadata": {
    "fileId": 2,
    "sizeBytes": 45678
  }
}
```

Rules:

- A message part may store a backend-generated file id.
- A message part must not store base64 content.
- A message part must not store absolute paths.
- `url` should be a controlled local API URL.
- The backend owns file id generation.

## API Draft

Future P2/P3 endpoints:

- `POST /api/files/upload`
- `GET /api/files/{id}`
- `GET /api/files/path/{id}`
- `DELETE /api/files/{id}`

Optional later endpoint:

- `GET /api/files/{id}/thumbnail`

### `POST /api/files/upload`

Input:

- `multipart/form-data`
- Field name: `files`
- One request accepts at most a small fixed number of files.

Output:

```json
{
  "files": [
    {
      "id": 1,
      "fileName": "image.png",
      "mime": "image/png",
      "sizeBytes": 12345,
      "url": "/api/files/path/1"
    }
  ]
}
```

Security rules:

- Do not return absolute paths.
- Do not return `storageKey` unless it is strictly needed.
- Do not return EXIF metadata.
- Do not return OCR content.
- Upload failures must not include local paths.
- Sanitize display file names.
- Duplicate display names must not overwrite existing files.
- Generate unique storage keys on the backend.

### `GET /api/files/path/{id}`

Rules:

- `id` must be numeric or a restricted safe id.
- The backend must look up metadata first.
- The backend must build the app-data path from metadata.
- Reject `..`, slashes, URL-encoded separators, and arbitrary path parameters.
- Return only files inside the RikkaDesk file store.
- Return a safe `Content-Type`.
- Unknown MIME should use `application/octet-stream`.
- Add `X-Content-Type-Options: nosniff`.
- Use a conservative cache policy.

### `DELETE /api/files/{id}`

Rules:

- Delete or tombstone metadata.
- Delete the blob when safe.
- If a sent message still references the file, the initial product behavior must be explicit:
  - Allow deletion and show a missing-file chip; or
  - Only delete unsent draft attachments in early phases.
- Delete failures must not leak paths.

## File Limits

Initial recommended limits:

- Single file maximum: 20 MB.
- Single upload request: 5 files, or at most 10 files after testing.
- Total file-store quota: design for 200 MB or 500 MB before implementing enforcement.

Initial allowlist:

- `image/png`
- `image/jpeg`
- `image/webp`
- `image/gif`
- `text/plain`
- `application/pdf` as a document chip only

Initially deferred:

- `image/svg+xml`
- `text/html`
- Office document parsing.
- Archives such as zip, rar, and 7z.
- Executables and installers such as exe, dll, msi.
- Scripts and unknown binaries.

Notes:

- SVG should not be previewed as a normal inline image even if its MIME is `image/svg+xml`.
- HTML should not be opened in an automatic preview iframe.
- PDF should initially be a document chip only.
- Office documents should not be parsed or previewed in early phases.

## Preview Policy

P2/P3 preview policy:

- Show only file chips.
- Image chips may show a lightweight placeholder.
- Document chips show display name, MIME, and size.
- Do not implement PDF preview.
- Do not implement Office preview.
- Do not implement OCR.

P4 preview policy:

- Raster image preview may support png, jpeg, webp, and gif.
- SVG remains blocked from inline preview.
- HTML remains blocked from preview.
- PDF remains deferred, or goes through a separate sandbox review before any preview implementation.

## Conversation Persistence And Export Policy

Conversations may persist message part metadata:

- file id
- controlled local API URL
- display name
- MIME
- size

Conversations must not persist:

- file content
- base64 file content
- original absolute path
- OCR text by default
- EXIF payload

Conversation export defaults:

- Export text and attachment metadata only.
- Do not export blobs by default.
- If a future "export with attachments" feature is added, require explicit user confirmation and use a zip/package format.
- Conversation import must not silently recreate local files.
- Missing attachments should render as safe missing-file UI.

## Multimodal Provider Policy

Phase 10 P1 through P5 must not send files to real providers.

P6 may consider an OpenAI-compatible image input prototype only when all of these are true:

- The selected model declares `IMAGE` input capability.
- The user explicitly chose the file.
- RikkaDesk does not send a local path.
- RikkaDesk does not save base64 to state.
- RikkaDesk reads only a small file from the managed file store.
- Size limits are enforced.
- The UI can show a confirmation such as: `This file will be sent to the selected provider.`
- Text-only models block image attachments with a clear message.
- Documents and PDFs do not enter provider requests unless separately designed.

## Model Capability Metadata

Current desktop provider injection is text-only:

```json
{
  "inputModalities": ["TEXT"],
  "outputModalities": ["TEXT"]
}
```

Future P5 Provider Settings may allow per-model capabilities:

```json
{
  "id": "desktop-model-1",
  "modelId": "gpt-4o-mini",
  "displayName": "GPT-4o mini",
  "inputModalities": ["TEXT", "IMAGE"],
  "outputModalities": ["TEXT"]
}
```

Rules:

- Default every model to `TEXT` only.
- Let the user explicitly enable `IMAGE` input for a model.
- `ModelList` already has enough UI surface to show modalities.
- `ChatInput` must check current model capability before sending.
- If image attachments exist and the selected model is text-only, block sending and show a clear error.

## Temporary Product Strategy For Current UI Shell

Current `ChatInput` exposes attachment controls, but the desktop backend does not support `/api/files/upload`.

Short-term options:

### Option A: Friendly Disable

Hide or disable attachment upload controls and show a beta message such as:

```text
RikkaDesk beta does not support attachments yet.
```

### Option B: Local Skeleton First

Implement the local mock API skeleton first so the inherited UI no longer fails with a 404, while still keeping provider sending text-only.

Recommendation: P2 should either implement the local mock API skeleton or P3 should add a friendly disable state. Do not keep a visible upload UI that fails with `/api/files/upload` 404 for long.

## Upstream Reference

Useful upstream ideas:

- Android `FilesRoutes.kt` upload, delete, and path-serving design.
- Managed file metadata model.
- MIME and size-limit checks.
- Message part types.
- `isValidToUpload`.
- Provider content-part construction for future multimodal requests.
- `inputModalities` / `outputModalities` capability gating.

Do not directly port:

- Android `ContentResolver`.
- Android `Uri` assumptions.
- Room DAO code.
- DocumentsProvider.
- Workspace file mounts.
- OCR and tools coupling.
- Android permission model.
- Local path exposure to AI.
- Base64-heavy state or conversation history.

## Risk Register

| Risk | Level | Mitigation |
| --- | --- | --- |
| Path traversal | High | Use backend-generated ids; reject arbitrary paths; canonicalize inside app-data file store. |
| Arbitrary file read | High | Never accept real paths; serve only managed file ids. |
| Arbitrary file write | High | Generate storage keys; write only under app data; no user-controlled storage paths. |
| Disk exhaustion | High | Enforce per-file, per-request, and total store quotas. |
| MIME spoofing | High | Combine browser type, extension, magic bytes, and server-side allowlist. |
| Malicious SVG / HTML / PDF preview XSS | High | Do not inline SVG/HTML; defer PDF; use sandbox review before previews. |
| File name injection | Medium | Sanitize display names; never use display names as paths. |
| EXIF / metadata privacy | Medium | Do not log EXIF; consider stripping metadata before provider send. |
| Base64 in JSON state | High | Store blobs on disk; state stores only metadata. |
| App data leakage | High | Do not expose storage keys or paths unnecessarily; require auth token for file endpoints. |
| Conversation export leaks files | High | Export metadata only by default; attachment bundle export requires explicit confirmation. |
| Provider request leaks local path | High | Provider builder must use file bytes or data URLs only after explicit consent. |
| Drag/drop uploads private file accidentally | Medium | Use visible chips and clear remove controls before send. |
| OCR reads sensitive documents | High | Defer OCR; require explicit opt-in and do not log OCR text. |
| Provider image upload without consent | High | Add per-send confirmation before first multimodal provider upload. |
| Windows file permission mismatch | Medium | Store under app data; handle access denied safely. |
| Deletion / retention ambiguity | Medium | Define draft vs sent-message deletion behavior before implementation. |
| Model capability misclassification | High | Default to text-only and require explicit IMAGE capability. |

## Recommended Phase 10 Roadmap

### P1: Safety Design Document

Goal:

- Define the file store, metadata, API, preview policy, export policy, and multimodal boundaries.

Files:

- Create `docs/rikkadesk-files-attachments-multimodal-plan.md`.

Not included:

- No code changes.
- No schema changes.
- No real file reading.

Acceptance:

- Document exists.
- `git diff --check` passes.
- Security search finds no real secrets or user files.

### P2: Local File Metadata And Mock API Skeleton

Goal:

- Implement managed file metadata and minimal `/api/files/*` skeleton for synthetic fixtures.
- Keep provider calls text-only.

Candidate files:

- `web-ui/src-tauri/src/mock_api.rs`
- Locale files if user-facing errors are surfaced.
- `docs/rikkadesk-beta-package-checklist.md`

Not included:

- No real provider multimodal calls.
- No OCR.
- No PDF parsing.
- No broad preview support.

Acceptance:

- Upload synthetic small files.
- Store blobs under app data.
- Save metadata without absolute paths or content.
- Serve by id only.
- Reject traversal, unknown paths, oversized files, and disallowed types.

### P3: UI Attachment Placeholder Or Friendly Disable

Goal:

- Either connect the existing UI to the local file skeleton or make unsupported attachment upload explicitly disabled.

Candidate files:

- `web-ui/app/components/input/chat-input.tsx`
- `web-ui/app/components/message/parts/*`
- `web-ui/app/components/message/message-part.tsx`
- Locale files.

Not included:

- No provider file upload.
- No PDF/OCR parsing.
- No Workspace.

Acceptance:

- No visible 404-style attachment workflow.
- Draft file chips can be added and removed safely, or upload is clearly disabled.
- Removing a draft attachment does not delete files referenced by sent messages unless explicitly designed.

### P4: Image / Document Preview Safety

Goal:

- Add safe raster image preview and safe document chips.

Candidate files:

- Image/document part components.
- Workbench preview only if a separate preview surface is needed.

Not included:

- No inline SVG preview.
- No HTML preview.
- No automatic PDF or Office parsing.

Acceptance:

- Raster images preview safely.
- SVG, HTML, external URLs, `data:`, `blob:`, `file:`, `javascript:`, and malformed managed file paths are blocked.
- PDF and text files remain chip-only.
- Missing or deleted files show a safe fallback.
- Rendering uses no iframe, object, embed, or `dangerouslySetInnerHTML`.

### P5: Model Capability Metadata

Goal:

- Add explicit model capability metadata and UI gating.

Candidate files:

- `web-ui/app/components/input/model-list.tsx`
- `web-ui/app/components/provider-settings-dialog.tsx`
- Provider import/export only if capabilities need export later.
- Locale files.

Not included:

- No provider multimodal request yet.

Acceptance:

- Default models are `TEXT` only.
- IMAGE capability is opt-in.
- ModelList shows capability.
- ChatInput blocks image attachments for text-only models.

### P6: OpenAI-Compatible Image Input Prototype

Goal:

- Add a small, controlled image input prototype for OpenAI-compatible providers.

Candidate files:

- `web-ui/src-tauri/src/mock_api.rs`
- Provider model capability code.
- OpenAI-compatible content part builder.

Not included:

- No documents/PDF provider upload.
- No OCR.
- No arbitrary file sending.

Acceptance:

- Only small managed raster images can be sent.
- User confirmation is visible.
- No local paths are sent.
- No base64 is stored in state.
- Text-only models are blocked.

Current P6.1 design decision:

- Start with a design document only.
- Prefer Chat Completions content array for the prototype.
- Keep Responses API and OpenAI Files API deferred.
- Send only the current turn image, never historical images.
- Defer image regenerate support.
- Use synthetic capture-server validation before optional manual real-provider testing.

### P7: Docs / Release Copy / Beta Candidate

Goal:

- Update release notes and beta checklist once a safe subset is implemented and verified.

Candidate files:

- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-release-draft.md`
- About dialog and locale files if support status changes.

Not included:

- No release or tag until explicitly requested.

Acceptance:

- Docs match actual implemented behavior.
- Known limits stay visible.
- No claim of full multimodal support before provider upload is implemented and verified.

## Candidate Files By Future Phase

P2:

- `web-ui/src-tauri/src/mock_api.rs`
- Locale files, if needed.
- `docs/rikkadesk-beta-package-checklist.md`

P3:

- `web-ui/app/components/input/chat-input.tsx`
- `web-ui/app/components/message/parts/*`
- `web-ui/app/components/message/message-part.tsx`
- Locale files.

P4:

- Image and document part components.
- Workbench preview only after separate review.

P5:

- `web-ui/app/components/input/model-list.tsx`
- `web-ui/app/components/provider-settings-dialog.tsx`
- Provider import/export only if capability metadata needs migration/import/export.
- Locale files.

P6:

- `docs/rikkadesk-openai-compatible-image-input-plan.md`
- `web-ui/src-tauri/src/mock_api.rs`
- Provider model capability code.
- OpenAI-compatible content part builder.

P7:

- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-release-draft.md`
- About dialog and locale files if support status changes.

## Files Not To Modify In P1

- `Android app/**`
- `web-ui/src-tauri/tauri.conf.json`
- SecretStore / DPAPI code.
- Provider Advanced Config.
- Existing provider import/export v3.
- Current `schemaVersion`.
- Real app data.
- Package dependencies.
- Release or tag files.

## P1 Acceptance Criteria

Phase 10 P1 is complete when:

- Only `docs/rikkadesk-files-attachments-multimodal-plan.md` is added.
- No frontend code is changed.
- No backend code is changed.
- No dependency files are changed.
- No Tauri or Android files are changed.
- `git diff --check` passes.
- Security search contains only documentation safety references and no real secrets, app data, or user file content.

Recommended verification:

```powershell
git diff --stat
git diff -- docs/rikkadesk-files-attachments-multimodal-plan.md
git diff --check
git status
git diff | rg -i "apiKey|Authorization|x-api-key|accessToken|refreshToken|secretRef|mock-api/secrets|DPAPI|console.log|.bin|password|token|cookie|bearer|file://|\.\.|base64|ContentResolver|Uri|OCR"
```

Allowed search hits:

- Documentation safety explanations.
- Risk table entries.
- Prohibited field explanations.
- Upstream reference names such as `ContentResolver`, `Uri`, and OCR.

Disallowed search hits:

- Real API keys or tokens.
- Real Authorization headers.
- Real app data path content.
- `secrets/*.bin` content.
- Real user file content.
