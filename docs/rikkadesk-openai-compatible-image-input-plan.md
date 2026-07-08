# RikkaDesk OpenAI-Compatible Image Input Prototype Plan

Review date: 2026-07-08

This document is the Phase 10 P6.1 design plan for a future OpenAI-compatible image input prototype in RikkaDesk desktop. It provides a safety route for P6.2 through P6.6 only. It does not implement real multimodal sending.

Current RikkaDesk provider requests remain text-only. The P6 prototype only considers OpenAI-compatible Chat Completions image input. Responses API, OpenAI Files API, PDF parsing, OCR, audio input, video input, Workspace, MCP, and tools remain deferred.

Current recommended private beta tag:

```text
rikkadesk-v0.1.0-beta.11
```

## Current State

Current desktop state and provider metadata:

- Local state is `schemaVersion: 6`.
- Provider import/export is version 4.
- Desktop provider models store `inputModalities` and `outputModalities`.
- Existing and imported older models default to `inputModalities: ["TEXT"]` and `outputModalities: ["TEXT"]`.
- Provider Settings can mark a model with optional `IMAGE` input metadata.

Current attachment behavior:

- `ChatInput` blocks image attachment sends when the selected model is text-only.
- IMAGE-capable models still keep attachments local-only.
- Attachments are saved in the local conversation and are not sent to providers.

Current backend guard:

- `/api/conversations/{id}/messages` does not call a provider when the user message contains any non-text part.
- `/api/conversations/{id}/regenerate` does not call a provider when the regenerated user turn contains any non-text part.
- Both paths append a local-only attachment notice instead.

Current OpenAI-compatible request path:

- `OpenAiChatMessage` is still `role: String` plus `content: String`.
- `build_openai_chat_request_body` still writes only `model`, `messages`, and `stream`.
- Test Connection is still text-only `ping`.
- The streaming parser still reads text deltas from `delta.content`.
- `customBody` validation still prevents overriding `model`, `messages`, and `stream`.

Conclusion:

```text
P6 must introduce a parallel vision request path instead of mutating the existing text-only path in place.
```

## API Shape Options

### Option A: Chat Completions Content Array

Example request shape:

```json
{
  "role": "user",
  "content": [
    { "type": "text", "text": "Describe this image" },
    {
      "type": "image_url",
      "image_url": {
        "url": "data:image/png;base64,...",
        "detail": "auto"
      }
    }
  ]
}
```

Recommendation: use this for the P6 prototype.

Reasons:

- It is closest to the current `/chat/completions` endpoint.
- It is the most likely OpenAI-compatible shape for third-party gateways that already mimic Chat Completions.
- It can reuse the current provider base URL, Test Connection strategy, and streaming parser with the least disruption.
- It can be validated first against a local synthetic capture server before any real provider test.

Risks:

- Not every OpenAI-compatible provider supports image parts.
- Data URLs can be large.
- Base64 must never enter state, logs, exports, or user-visible errors.
- Gateway behavior with `customBody` and custom headers needs capture-server smoke tests.

### Option B: Responses API `input_image`

Example request shape:

```json
{
  "input": [
    {
      "role": "user",
      "content": [
        { "type": "input_text", "text": "Describe this image" },
        {
          "type": "input_image",
          "image_url": "data:image/png;base64,..."
        }
      ]
    }
  ]
}
```

Assessment:

- The shape is clearer for future text, image, and file inputs.
- It would affect endpoint selection, provider settings, Test Connection, streaming parser behavior, and custom request config.
- Many generic OpenAI-compatible Chat Completions providers do not expose Responses API.

Decision: defer Responses API to a separate provider adapter or Responses mode.

### Option C: Provider Adapter Layer

Long-term direction:

- `ProviderRequestMode::TextChat`
- `ProviderRequestMode::OpenAiChatCompletionsVision`
- `ProviderRequestMode::ResponsesVision`

Assessment:

- This is the right long-term design boundary.
- P6 should avoid implementing the whole adapter stack.
- P6.2 can introduce internal request types that make this future split easier without changing current text-only behavior.

## Recommended P6 Prototype

```text
P6 prototype should use OpenAI-compatible Chat Completions content array with in-memory base64 data URL, behind IMAGE capability gating and explicit per-send confirmation.
```

Prototype limits:

- Send only images from the current user turn.
- Do not send historical images.
- Do not support image resend during regenerate.
- Do not send document, PDF, text, audio, or video attachments to providers.
- Do not send GIF to providers in P6; keep GIF as a local attachment only.
- Allow only PNG, JPEG, and WEBP.
- Allow at most one provider-bound image per message.
- Limit provider-bound image size to 5 MB.
- Use image detail `auto`.
- Do not expose a detail selector UI.

## Base64 And URL Strategy

Forbidden:

- Saving base64 in `state.v1.json`.
- Saving base64 in conversation message parts.
- Saving base64 in provider import/export.
- Printing base64 in logs.
- Returning base64 in API responses.
- Including base64 in error messages.
- Sending local `/api/files/path/{id}` URLs to remote providers.
- Sending `file://` URLs to providers.
- Sending local absolute paths to providers.
- Sending `storageKey` values to providers.

Recommended strategy:

- The backend receives message parts containing managed `fileId` metadata.
- The backend resolves the file through managed metadata only.
- The backend reads blob bytes from the controlled app data file store.
- The backend revalidates:
  - file exists
  - file is not deleted
  - storage key is safe
  - MIME is PNG, JPEG, or WEBP
  - size is at most 5 MB
- The backend encodes a temporary data URL such as `data:image/png;base64,...`.
- The data URL exists only in the in-memory provider request body.
- The data URL is not persisted, logged, exported, or returned to the UI.
- Provider errors use safe messages that do not include request bodies.

Do not use localhost URLs for provider image input. Remote providers usually cannot access `127.0.0.1`, and exposing local file serving to a provider would create a bad security boundary.

Do not use OpenAI Files API in P6. Generic OpenAI-compatible providers differ too much, and file upload APIs require a separate provider adapter.

## File Store Helper Design

Future P6.2 or P6.4 can add an internal provider-input helper:

```rust
struct ManagedImageProviderInput {
    file_id: u64,
    mime: String,
    bytes: Vec<u8>,
}
```

Candidate function:

```rust
async fn managed_file_for_provider_image_input(
    state: &Arc<MockApiState>,
    file_id: u64,
) -> Result<ManagedImageProviderInput, String>
```

Requirements:

- Accept only `metadata.fileId`.
- Do not accept a path.
- Do not accept a URL.
- Do not return absolute paths.
- Do not return `storageKey`.
- Do not read deleted files.
- Allow only PNG, JPEG, and WEBP.
- Reject GIF for provider sending in P6.
- Reject oversized files with a safe error.
- Return safe errors that do not include paths, storage keys, bytes, or base64.

This helper should reuse the existing managed file metadata boundary and should perform defensive checks even if upload-time validation already ran.

## User Confirmation UX

Trigger conditions:

- The selected model includes `IMAGE` in `inputModalities`.
- The draft contains an image attachment.
- The user clicks Send.
- The P6 image provider path is enabled.

Chinese confirmation copy:

```text
这条消息包含图片附件。发送后，图片内容会上传到当前选择的模型服务商进行处理。RikkaDesk 不会发送本地文件路径，也不会把图片 base64 保存到本地会话。
```

English confirmation copy:

```text
This message includes image attachments. If you continue, the image content will be sent to the selected model provider. RikkaDesk will not send local file paths or persist image base64 in local conversation state.
```

Buttons:

- Send with image
- Cancel

P6 should not add:

- Do not ask again.
- Provider-level permanent consent.
- Conversation-level permanent consent.

Recommendation: ask every time before sending an image to a provider. This keeps the first prototype boring in a good way.

## Model Capability Gating

Current P5a behavior:

- TEXT-only models block image attachment send.
- IMAGE-capable models still save image attachments locally only.

P6 behavior:

- TEXT-only model: continue blocking image attachment send.
- IMAGE-capable model without confirmation: do not send.
- IMAGE-capable model with confirmation: enter the image request path.
- Document, PDF, and text attachments: remain local-only.
- GIF attachments: remain local-only.
- Multiple image attachments: block and ask the user to send one image at a time.

Do not add a persisted privacy setting in P6. Use `inputModalities` plus per-send confirmation.

## Request Model Design

Keep the current text-only model intact:

```rust
#[derive(Clone, Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}
```

Add parallel vision-oriented structures later:

```rust
#[derive(Clone)]
enum OpenAiCompatibleMessageContent {
    Text(String),
    Parts(Vec<OpenAiCompatibleContentPart>),
}

#[derive(Clone)]
enum OpenAiCompatibleContentPart {
    Text(String),
    ImageUrl {
        data_url: String,
        detail: String,
        file_id: u64,
    },
}

#[derive(Clone)]
struct OpenAiCompatibleChatMessage {
    role: String,
    content: OpenAiCompatibleMessageContent,
}
```

Alternative internal representation:

```rust
enum ProviderChatContentPart {
    Text(String),
    Image {
        mime: String,
        bytes: Vec<u8>,
        detail: ImageDetail,
        file_id: u64,
    },
}
```

Recommendation:

- Keep `OpenAiChatMessage` for text-only.
- Add a parallel vision struct.
- Add `build_openai_vision_chat_request_body`.
- Keep Test Connection text-only.
- Keep the current streaming parser limited to text deltas.
- Keep `customBody` validation that blocks `model`, `messages`, and `stream`.
- In the vision builder, write RikkaDesk-owned fields after applying safe custom body so custom config cannot override `model`, `messages`, or `stream`.

Functions likely affected in P6.2 through P6.4:

- `openai_messages_from_conversation`
- `trim_openai_message_history`
- `build_openai_chat_request_body`
- future `build_openai_vision_chat_request_body`
- `stream_openai_compatible_chat`
- `spawn_openai_stream_generation`
- `/messages`
- `/regenerate`

## History And Regenerate Strategy

P6 prototype should send only the current user turn image.

Rules:

- Historical image attachments are not resent.
- Historical messages contribute text only.
- Regenerate for a user turn with images is deferred.
- Regenerate should return a safe prompt to send a new message and confirm image upload again.

Suggested copy:

```text
Regenerating image messages is not supported in this beta. Please send a new message and confirm image upload again.
```

Reason: automatically resending historical private images is too surprising for a prototype.

## Size, Count, MIME, And Detail Limits

Recommended P6 limits:

- Image count: 1.
- Single provider-bound image size: 5 MB.
- Total provider-bound image payload: 5 MB.
- MIME allowlist: PNG, JPEG, WEBP.
- GIF: local attachment only.
- SVG: blocked.
- HTML: blocked.
- PDF/TXT/Office/audio/video: local attachment only, not provider-bound.
- Detail: `auto`.
- Detail UI: not exposed.

Oversized image behavior:

- Block sending.
- Keep the draft intact.
- Ask the user to compress the image or choose a smaller one.

Deleted or missing file behavior:

- Block sending.
- Ask the user to re-upload the image.
- Do not call the provider.

## Error Handling And Logging

Hard requirements:

- Do not print request bodies.
- Do not print data URLs.
- Do not print base64.
- Do not print image bytes.
- Do not print file store paths.
- Do not print `storageKey`.
- Do not return base64 in API responses.
- Provider errors must use safe messages.
- Timeout, connection, and HTTP status errors must continue using safe error helpers.
- `safe_reqwest_error` must not expose request bodies.
- `safe_http_status_error` must not expose response bodies.

Capture-server tests may record only safe booleans, for example:

- `hasImageUrl: true`
- `dataUrlPrefixOk: true`
- `containsLocalPath: false`
- `containsSecretRef: false`

They must not record the complete request body or complete data URL.

## Capture-Server Test Strategy

P6.4 should use a local synthetic capture server before any real provider test.

Rules:

- Do not use a real provider.
- Do not use a real API key.
- Use a local endpoint shaped like `/chat/completions`.
- Use a 1x1 synthetic PNG fixture.
- Do not use real user files.
- Do not print full request bodies.

Capture-server assertions:

- `messages[].content[]` is an array.
- The array contains `{ "type": "text" }`.
- The array contains `{ "type": "image_url" }`.
- `image_url.url` starts with `data:image/png;base64,`.
- The body does not contain `/api/files/path`.
- The body does not contain `file://`.
- The body does not contain a Windows local path.
- The body does not contain `storageKey`.
- The body does not contain `secretRef`.
- The body does not contain API keys or auth headers.

Fake response:

- Return SSE text deltas compatible with the current parser.
- Verify the assistant message finishes normally.

State assertions after send:

- No base64.
- No file content.
- No local absolute path.
- No provider request body.

UI assertions:

- TEXT-only model blocks image send.
- IMAGE-capable model opens confirmation.
- Cancel does not call provider.
- Confirm calls only the capture server during prototype testing.
- Document attachments remain local-only.

## Risk Table

| Risk | Severity | P6 Mitigation |
|---|---:|---|
| User accidentally sends a private image to a provider | High | Require explicit per-send confirmation with clear provider disclosure. |
| Local path leaks to provider | High | Resolve by file id only and send only in-memory data URL. |
| Base64 enters `state.v1.json` | High | Keep base64 only inside the transient request body and assert state contains none. |
| Base64 enters logs | High | Forbid request-body logging and capture only safe booleans in tests. |
| Large images cause memory or request-body blowups | High | Limit provider-bound image to one image and 5 MB. |
| GIF or unusual image format causes provider incompatibility | Medium | Keep GIF local-only in P6; allow only PNG/JPEG/WEBP. |
| SVG or HTML bypasses image restrictions | High | Provider helper allows only raster MIME and revalidates metadata. |
| Provider does not support image content despite IMAGE metadata | Medium | Use capture-server first; real provider test remains optional and manual. |
| `customBody` interferes with multimodal request structure | Medium | Keep reserved-field validation and write RikkaDesk-owned fields last. |
| Regenerate resends historical private images unexpectedly | High | Defer image regenerate and require a fresh send with confirmation. |
| Export/import leaks image content | High | Provider export stays provider metadata only; conversation export must not include blobs by default. |
| Capture-server test accidentally points at a real endpoint | High | Use explicit local-only base URL and no real API key in automated smoke tests. |
| Provider error leaks request body | High | Keep safe error mapping and never include provider request body in UI/logs. |
| Proxy or gateway records full request body | Medium | Disclose provider handling in confirmation copy; keep prototype manual and explicit. |
| Provider privacy policy boundary is unclear | Medium | Include provider disclosure in UX and release notes before enabling real tests. |

## P6 Roadmap

### P6.1: Design Document

Goal:

- Record this plan.
- Do not modify code.
- Do not enable image sending.

Validation:

- Only docs change.
- `git diff --check` passes.
- Safety search finds only documented risks and examples.

### P6.2: Backend Internal Request Model Refactor

Goal:

- Add parallel vision request structs and builder skeleton.
- Keep default behavior text-only.
- Do not send image data.
- Do not change Provider Settings or ChatInput behavior.

Candidate files:

- `web-ui/src-tauri/src/mock_api.rs`
- optional tests if a local Rust test pattern is introduced later

Non-goals:

- No real provider image send.
- No schema bump unless a later implementation truly needs persisted flags.
- No base64 persistence.

### P6.3: Confirmation UI

Goal:

- Add explicit image-send confirmation for IMAGE-capable models.
- Cancel does not call provider.
- Confirm can initially pass an explicit in-memory intent flag while backend still returns local-only.

Candidate files:

- `web-ui/app/components/input/chat-input.tsx`
- locale files

Non-goals:

- No persistent "do not ask again".
- No provider-level privacy setting.

### P6.4: Synthetic Capture-Server Prototype

Goal:

- Send one synthetic 1x1 PNG to a local capture server.
- Validate Chat Completions content array request shape.
- Verify base64 is absent from state/logs.

Candidate files:

- `web-ui/src-tauri/src/mock_api.rs`
- local smoke scripts or temporary test notes, if kept out of commits unless explicitly requested

Non-goals:

- No real provider.
- No real user files.
- No real API key.

### P6.5: Optional Real-Provider Manual Gate

Goal:

- Only after capture-server tests pass, optionally test with a manually entered key and one synthetic PNG.

Rules:

- Do not record the key.
- Do not screenshot the key.
- Do not print request body.
- Do not print base64.
- This step can be deferred to beta.13 or later.

### P6.6: Docs And Release Copy

Goal:

- Document experimental image input if and only if implementation and verification pass.

Candidate files:

- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-release-draft.md`
- About dialog and locale files

Required copy:

- Image input is experimental.
- Only PNG/JPEG/WEBP are provider-bound.
- User confirmation is required.
- Base64 is not persisted.
- Documents, PDFs, audio, video, OCR, Workspace, MCP, and tools remain unsupported unless separately implemented.

## P6.1 Acceptance Criteria

- Only `docs/rikkadesk-openai-compatible-image-input-plan.md` is added.
- Existing Phase 10 plan/checklist may receive a small P6.1 design status note.
- No code changes.
- No dependency changes.
- No `schemaVersion` change.
- No Provider import/export version change.
- No Tauri configuration change.
- No Android app change.
- No frontend/backend behavior change.
- No real provider connection.
- No real user file read.
- No image sending.
- No base64 persisted anywhere.
- `git diff --check` passes.
- Worktree is clean after commit.

Verification commands:

```powershell
git diff --stat
git diff -- docs/rikkadesk-openai-compatible-image-input-plan.md docs/rikkadesk-files-attachments-multimodal-plan.md docs/rikkadesk-beta-package-checklist.md
git diff --check
git status
```

Safety search:

```powershell
git diff | rg -i "apiKey|Authorization|x-api-key|accessToken|refreshToken|secretRef|mock-api/secrets|DPAPI|console\.log|\.bin|password|token|cookie|bearer|file://|\.\.|base64|ContentResolver|Uri|OCR|multipart|files/upload|files/path|storageKey|absolute path|dangerouslySetInnerHTML|allow-same-origin|image/svg|text/html|javascript:|data:|blob:|iframe|object|embed|window.open|provider|OpenAI|inputModalities|outputModalities|image_url|input_image|Responses"
```

Allowed hits:

- Documented risk descriptions.
- Explicitly forbidden fields.
- Example request shapes.
- OpenAI-compatible and Responses API comparison.
- Base64 not persisted statements.

Disallowed hits:

- Real keys, tokens, headers, or credentials.
- `secrets/*.bin` content.
- Real app data content.
- Real user file content.
- New code implementation.
