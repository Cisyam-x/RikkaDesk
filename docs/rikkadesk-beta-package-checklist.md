# RikkaDesk Local Beta Package Checklist

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This checklist prepares the first local Windows beta package for private testing only. It does not publish a GitHub Release.

Do not paste real API keys into documentation, commit messages, terminal transcripts, screenshots, or issue comments.

Current private beta baseline: `rikkadesk-v0.1.0-beta.13`.

Current private hotfix tag: `rikkadesk-v0.1.0-beta.14`.

Previous private beta tag: `rikkadesk-v0.1.0-beta.12`.

## Current Beta Scope

Included:

- Tauri v2 Windows desktop shell.
- Local Rust HTTP API bound to `127.0.0.1`.
- JSON persistence for settings, conversations, messages, provider config, and id sequence.
- Desktop Provider Settings UI for OpenAI-compatible provider management.
- Provider list, add/edit/delete provider, multiple models per provider, favorite model updates, Set as current model, and Test Connection.
- Advanced request config for non-sensitive provider custom headers and safe custom body JSON.
- Safe provider import/export v4 for multi-model provider metadata, model capabilities, and safe advanced request config, with v1/v2/v3 import compatibility.
- Secret references in JSON and encrypted local secret blobs for API keys.
- OpenAI-compatible text chat with streaming responses.
- Mock fallback when a real provider is not configured or cannot be used.
- Local attachment skeleton with managed file metadata.
- Safe raster image attachments for PNG, JPEG, WEBP, and GIF.
- TXT/PDF document chips with no inline PDF preview.
- Safe image/document message rendering for managed file URLs.
- Provider model capability metadata with TEXT and IMAGE input markers.
- Loopback-only synthetic image capture prototype for one current-turn PNG/JPEG/WEBP image.
- beta.13 hotfix fixes for Tauri production local image attachment preview/rendering and file picker stability.

Not included:

- Public GitHub Release publishing.
- Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- Real-provider image input by default.
- Full multimodal provider support.
- OCR, PDF/Office parsing, audio/video input, tools, MCP, search, Workspace, forks, or release auto-updates.
- SQLite, sync, multi-device backup, or production-grade migration tooling.
- Any change to the upstream Android `app` module.

## beta.13 Hotfix Scope

The beta.13 hotfix is a narrow follow-up to beta.12. The `rikkadesk-v0.1.0-beta.13` tag has been created and pushed. The `rikkadesk-v0.1.0-beta.12` tag already exists and must not be moved, deleted, or overwritten.

Fixed over beta.12:

- Local image attachment draft preview and sent-message rendering in Tauri production builds.
- Managed file URLs from `/api/files/path/{id}` resolve to the actual local mock API URL before image rendering.
- The hidden file picker input remains stably mounted.
- Upload detection/upload errors are caught and the file input value is always reset, so the same file can be selected again after delete or failure.

Unchanged:

- Real-provider image input remains disabled.
- The loopback-only capture path remains the only implemented image-send prototype.
- Local state remains `schemaVersion: 6`.
- Provider import/export remains version 4.
- The app/package version remains `0.1.0`.
- This is not a public GitHub Release.

## Version Recommendation

The current package version is `0.1.0` in:

- `web-ui/src-tauri/tauri.conf.json`
- `web-ui/src-tauri/Cargo.toml`

For the first local beta, keeping `0.1.0` is acceptable because the package is private and the existing Windows installer names are already stable.

Before a public prerelease, consider changing the project version to `0.1.0-beta.1` if the Tauri Windows bundle pipeline accepts the prerelease version cleanly. If Windows installer tooling rejects or normalizes prerelease metadata, keep the internal app version numeric and add the beta label in release notes and artifact filenames outside the installer.

Do not change the version casually during Phase 5A.

## Build Windows Installers

From the repository root:

```powershell
cd web-ui
pnpm install
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run desktop:build
```

Expected build outputs:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

Use the NSIS installer for the normal local beta installation smoke test. Keep the MSI as an alternate enterprise-style installer artifact.

The current Windows installers and `rikkadesk.exe` executable are unsigned. Windows SmartScreen or unsigned publisher warnings are expected until a signing workflow is configured.

Windows 11 Smart App Control can be stricter than SmartScreen and may block the installed unsigned executable from launching, for example from `C:\Users\<you>\AppData\Local\RikkaDesk\rikkadesk.exe`. That is a Windows security policy block for an unverified publisher, not a RikkaDesk runtime crash. For the private beta, prefer development or testing machines where Smart App Control is not enabled. Do not ask testers to disable Windows security features or bypass enterprise security policy for this build. The long-term release-quality fix is code signing for the executable and installer, which should be handled as a separate Windows code signing phase.

## Install

1. Close any running RikkaDesk development windows.
2. Open:

```powershell
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

3. Complete the installer wizard.
4. Start RikkaDesk from the installer finish screen, Start Menu, or installed shortcut.
5. Confirm the desktop window opens.

If Windows SmartScreen warns about an unsigned installer, that is expected for the local beta until signing is configured. If Windows 11 Smart App Control blocks the installed `rikkadesk.exe` outright, record it as an unsigned-publisher security policy block and test on an appropriate development/test machine instead of changing system security settings.

## Uninstall

Use Windows Settings:

1. Open `Settings`.
2. Go to `Apps` > `Installed apps`.
3. Find `RikkaDesk`.
4. Choose `Uninstall`.

Or use the uninstaller created by the NSIS package in the install directory.

Uninstalling the app may leave user data and encrypted local secret blobs in the app data directory. That is expected for the current beta and should not be treated as an uninstall failure. Whether the installer should automatically remove app data is a later release policy decision.

## App Data Directory

On Windows, RikkaDesk uses:

```powershell
$env:APPDATA\com.cisyamx.rikkadesk
```

The current Mock API data lives under:

```powershell
$env:APPDATA\com.cisyamx.rikkadesk\mock-api
```

Important files:

- `mock-api/state.v1.json`
- `mock-api/secrets/*.bin`

Before manually cleaning app data:

- Exit RikkaDesk first.
- Confirm no `RikkaDesk` process is still running.
- Do not delete app data while RikkaDesk is running.
- Remember that deleting app data removes local provider keys and you will need to enter them again.

Safe process check:

```powershell
Get-Process RikkaDesk -ErrorAction SilentlyContinue
```

Direct cleanup, without reading secret blob contents:

```powershell
$AppData = Join-Path $env:APPDATA "com.cisyamx.rikkadesk"
if (Test-Path -LiteralPath $AppData) {
  Remove-Item -LiteralPath $AppData -Recurse -Force
}
```

Backup instead of delete:

```powershell
$AppData = Join-Path $env:APPDATA "com.cisyamx.rikkadesk"
$Backup = Join-Path $env:APPDATA ("com.cisyamx.rikkadesk.backup." + (Get-Date -Format "yyyyMMdd-HHmmss"))

if (Test-Path -LiteralPath $AppData) {
  Rename-Item -LiteralPath $AppData -NewName (Split-Path -Leaf $Backup)
  Write-Host "Backed up app data to: $Backup"
}
```

The backup directory may still contain encrypted secret blobs under `mock-api/secrets/*.bin`. Do not share backup directories, upload them to GitHub issues, send them to Codex / ChatGPT, or copy them into the repository.

## `state.v1.json`

`state.v1.json` stores non-sensitive local state for the desktop prototype. The filename remains `state.v1.json` even though the JSON payload may contain `schemaVersion: 6`.

Phase 8 upgrades provider state from `provider.model` to `provider.models[]`. Old beta.8 or earlier builds should not be started against a schema v3 state file; they may treat the state as unsupported and create a corrupt backup or default state. Back up app data before testing schema migration or moving between beta builds.

Phase 9B upgrades provider state to `schemaVersion: 4` with `providers[].customHeaders` and `providers[].customBody`. Old beta.9 or earlier builds should not be started against a schema v4 state file; they may not understand the provider shape. Back up app data before testing schema migration or moving between beta builds.

Phase 10 upgrades local desktop state to `schemaVersion: 5` with managed file metadata for the mock API file skeleton, then `schemaVersion: 6` with provider model capability metadata. Old beta.11 or earlier builds should not be started against schema v5/v6 state files. Use synthetic app data for Phase 10 file tests, or back up and restore real app data before switching builds.

Phase 12 P1-A serializes local state saves, uses unique same-directory temp files, flushes and syncs each complete temp file, and replaces the primary state without deleting it first. Packaging verification should run the synthetic `state_persist` tests and confirm save failures return a non-success response. P1-A does not yet add backup/restore, migration backup, future-schema protection, or in-memory rollback after a failed save.

Phase 12 P1-B initializes defaults only when the primary state is missing. Other read failures stop startup. Malformed state is preserved byte-for-byte in a unique corrupt backup and requires explicit recovery; a backup failure cannot fall through to default state. Future schemas stop startup without being marked corrupt, and schemas 1-5 receive a durable original-byte backup before migration. Strict P1-A stale temp files remain ignored and preserved. Recovery UI and non-pure mutation compensation are still pending.

Phase 12 P1-C1 stages pure settings and conversation mutations, persists the staged snapshot through the P1-A atomic writer, and commits live state only after persistence succeeds. Covered operations are assistant selection, current assistant model, favorites, title, pin/unpin, conversation delete, text message edit, and message delete. Missing conversation detail/stream GETs now return a virtual DTO without creating persisted state. Run the synthetic `staged_transaction`, `mutation_transaction`, `transaction_failure`, and `get_does_not_mutate` test groups and confirm success events occur only after commit.

Phase 12 P1-C2 serializes Provider/SecretStore mutations and commits Provider metadata through the same staged-state helper. Key create/update uses a new copy-on-write `secretRef`; failed state persistence deletes the operation-created encrypted blob and retains the old state/secret. Blank-key upsert keeps the old key. Clear/delete commits state before old-secret cleanup. Import never restores source `secretRef` or `hasSecret` and creates no secret. Run the synthetic `provider_transaction`, `secret_compensation`, `provider_import`, `provider_delete`, and `key_clear` groups plus the full Rust test suite.

P1-C2 verification must cover:

- Provider import success/failure, with `hasSecret: false` and no SecretStore write.
- Provider create/update key success, secret prepare failure, state persistence failure, and new-secret compensation.
- Blank-key upsert preserving the existing secret without a SecretStore write.
- Key clear and Provider delete preserving the old ref/blob on state failure.
- Post-commit cleanup failure returning logical success with a fixed redacted warning and leaving only an unreferenced encrypted orphan.
- Concurrent key updates, Provider mutation plus Category A mutation, and blank-key upsert plus clear completing without deadlock or lost state.
- Safe errors containing no key, encrypted bytes, secret ref, storage path, or state body.
- Synthetic temp state and an in-memory fake SecretStore only. Never read real app data or `mock-api/secrets/*.bin`.

P1-C1/P1-C2 do not make every mutation class atomic. File/blob consistency remains P1-C3, and send/regenerate/stop/streaming finalization remains P1-C4. Backup Mode A/B packaging is still blocked. Do not claim that SecretStore and JSON state are fully atomic across process crashes: an unreferenced encrypted orphan can remain between new-secret prepare and state commit or between state commit and old-secret cleanup. Managed blobs/metadata and streaming deltas are also not yet fully transactional.

It may contain:

- settings
- conversations
- messages
- idSeq
- provider id, name, type, enabled flag
- baseUrl
- model ids and displayNames under `providers[].models[]`
- model `inputModalities` and `outputModalities`
- non-sensitive custom headers under `providers[].customHeaders`
- safe custom body JSON under `providers[].customBody`
- managed file metadata under `files[]`
- assistant chatModelId
- secretRef
- savedAt

It must not contain:

- API key
- access token
- refresh token
- `Authorization` header
- `x-api-key` value
- service account private key
- any other sensitive credential
- file content
- base64 file payloads
- original absolute upload paths
- OCR text

## `mock-api/secrets/*.bin`

`mock-api/secrets/*.bin` stores encrypted secret blobs for the local desktop prototype on Windows. These files are not source-controlled and must stay in the user's app data directory.

The JSON state stores only a `secretRef`. The Rust backend uses that `secretRef` to find the encrypted local secret. The UI should only show `hasSecret: true` or `hasSecret: false`; it must never display the saved key.

Do not copy these files into the repository, README, logs, screenshots, issue reports, Codex prompts, or ChatGPT conversations. Do not read, print, parse, or share their contents during beta verification.

## Configure Provider Settings

1. Start RikkaDesk.
2. Open the sidebar.
3. Click `Provider Settings`.
4. Click `Add Provider` when creating a new provider.
5. Fill:
   - Provider Name: `OpenAI Compatible`
   - Base URL: an OpenAI-compatible API root, for example `https://api.openai.com/v1`
   - Model ID: a model supported by the endpoint, for example `gpt-4o-mini`
   - Display Name: optional, defaults to Model ID
   - API Key: enter locally only, never paste into documentation
6. Add a second model row under the same provider and confirm it shares the same Base URL and API Key.
7. Click `Save`.
8. Confirm the API Key field clears.
9. Confirm `hasSecret: true` when a key was saved.
10. Add a second provider.
11. Switch between providers in the provider list and confirm the edit form updates.
12. Edit provider name, base URL, model rows, or display names and save again.
13. Delete a model row and confirm at least one model remains.
14. Delete a provider and confirm the app uses an in-app confirmation dialog.
15. Confirm deleting a provider also deletes the corresponding local secret.

Leaving API Key blank should save only non-sensitive provider config. Existing saved secrets are kept.

Deleting a provider should remove its local secret and clear related favorite model entries. It must not expose the secret value.

## Test Favorite And Current Model Behavior

1. Open the model selector.
2. Confirm all provider models appear after Provider Settings changes, including multiple models from the same provider.
3. Click the favorite heart for a model.
4. Confirm the Favorites tab updates immediately.
5. Click the heart again and confirm the model leaves Favorites.
6. Open Provider Settings.
7. Select a provider.
8. Click `Set as current model` on a specific model row.
9. Confirm the chat model selector and input area reflect that exact model.
10. Confirm Set as current model does not automatically favorite the model.
11. Delete a non-current model and confirm favorites are cleaned only for the deleted model.
12. Delete the current model and confirm current-model fallback selects another available model without crashing.

## Test Connection

1. Open Provider Settings.
2. Select a provider with Base URL, at least one Model ID, and `hasSecret: true`.
3. Click `Test Connection` on a specific model row.
4. With a valid local provider configuration, confirm a success toast appears.
5. With an invalid Base URL, Model ID, or local key, confirm a safe failure toast appears.
6. Confirm the failure message does not include the API key, Authorization header, `x-api-key`, full request headers, or full request body.

## Test Advanced Request Config

1. Open Provider Settings and select or create an OpenAI-compatible provider.
2. Confirm the Advanced request config section is collapsed by default.
3. Add safe custom headers such as `OpenAI-Beta: assistants=v2` and `x-gateway-route: beta`.
4. Save, close, and reopen Provider Settings. Confirm the safe headers remain.
5. Clear the custom headers, save, close, and reopen. Confirm the header list stays empty.
6. Add safe custom body JSON such as:

```json
{
  "temperature": 0.7,
  "top_p": 0.9,
  "max_tokens": 123
}
```

7. Use `Format JSON`, save, close, and reopen. Confirm the safe body remains formatted and persisted.
8. Use `Clear JSON`, save, close, and reopen. Confirm the custom body is empty.
9. Confirm sensitive headers are rejected, including `Authorization`, `x-api-key`, `api-key`, `cookie`, and `proxy-authorization`.
10. Confirm sensitive custom body keys or values are rejected, including `apiKey`, `authorization`, `token`, `password`, `secret`, `credential`, `bearer`, and obvious key-like strings.
11. Confirm Test Connection uses the safe custom config but forces `max_tokens=1`.
12. Confirm Streaming Chat uses the same safe request builder and preserves allowed custom body fields such as `max_tokens`.
13. Confirm safe errors do not echo full custom header values or full custom body JSON.

## Test Markdown And Workbench Rendering Security

Markdown table/code/math rendering:

- Confirm a wide Markdown table scrolls horizontally inside a narrow message area instead of expanding the app viewport.
- Confirm code block copy, download, and preview actions still work after overflow polish.
- Confirm inline math and block math still render.
- Confirm mhchem chemistry fixtures such as `\ce{H2O}` render.
- Confirm invalid chemistry does not crash the message renderer.

Markdown raw HTML hardening:

- Confirm `rehypeRaw` is not explicitly enabled in the RikkaDesk Markdown plugin list.
- Confirm unsafe href schemes are blocked in message Markdown: `javascript:`, `data:`, `file:`, `blob:`, and relative URLs by default.
- Confirm unsafe image sources are blocked in message Markdown.
- Confirm normal `http:`, `https:`, and `mailto:` links remain clickable and keep `target="_blank"` plus `rel="noopener noreferrer"`.
- Confirm raw HTML cannot override link `target` or `rel` in message Markdown.
- Run the XSS fixture from `docs/rikkadesk-markdown-rendering-plan.md` and confirm scripts, event handlers, dangerous embed tags, and dangerous links do not execute in message bubbles.

Workbench preview hardening:

- Confirm HTML iframe sandbox is empty.
- Confirm SVG iframe sandbox is empty.
- Confirm Mermaid iframe sandbox is `allow-scripts`.
- Confirm Workbench preview iframe does not include `allow-same-origin`.
- Confirm Mermaid preview uses `securityLevel: "strict"`.
- Confirm remote Mermaid CDN residual risk is documented in `docs/rikkadesk-markdown-rendering-plan.md`.

## Test Local Attachment Skeleton

Use synthetic app data and synthetic fixture files only. Do not use real user files, real API keys, or existing `secrets/*.bin`.

Phase 10 P3 smoke checks:

- Confirm the image picker accepts only PNG, JPEG, WEBP, and GIF.
- Confirm the document picker accepts only plain text and PDF.
- Upload a synthetic `hello.txt` and confirm a document chip appears.
- Upload a synthetic PNG and confirm an image attachment appears.
- Upload a synthetic PDF and confirm it appears as a document chip only, with no inline PDF preview.
- Confirm synthetic SVG and HTML files are rejected by the frontend or safely rejected by the backend.
- Delete a draft attachment chip and confirm `DELETE /api/files/{id}` succeeds.
- Send a text message with synthetic image/document attachments and confirm the user message keeps the attachment parts.
- Confirm the mock assistant reply still works and provider requests remain text-only.
- Confirm `state.v1.json` has `schemaVersion: 5`, contains file metadata, and does not contain file contents, base64 payloads, or original absolute upload paths.

Phase 10 P4 smoke checks:

- Upload a synthetic PNG, send it, and confirm the message renders a safe raster image preview.
- Delete the underlying synthetic image file and confirm the message shows an unavailable state without crashing.
- Upload synthetic TXT and PDF files and confirm they render as document chips only.
- Confirm document chips do not create iframe, object, embed, PDF, Office, HTML, or SVG inline previews.
- Confirm unsafe legacy image/document URLs are blocked: `data:`, `blob:`, `file:`, `javascript:`, external HTTP(S), arbitrary relative paths, and malformed `/api/files/path/*`.
- Confirm SVG and HTML are not previewed as active images or documents.
- Confirm safe document links point only to controlled `/api/files/path/{id}` URLs and keep `target="_blank"` plus `rel="noopener noreferrer"`.
- Confirm no `dangerouslySetInnerHTML`, iframe, object, or embed is introduced for attachment message parts.

Phase 10 P5a smoke checks:

- Confirm migrating a synthetic schema v5 state writes `schemaVersion: 6`.
- Confirm file metadata survives the v5 to v6 migration.
- Confirm existing provider models default to `inputModalities: ["TEXT"]` and `outputModalities: ["TEXT"]`.
- In Provider Settings, create one text-only model and one model with Image input enabled. Save, close, reopen, and confirm the capability metadata persists.
- Confirm the model selector/settings payload includes the configured modalities.
- Export providers and confirm version 4 includes `inputModalities` and `outputModalities` for each model while excluding API keys, `secretRef`, local file metadata, file blobs, base64 payloads, and app data paths.
- Import a v3 provider export and confirm imported models default to TEXT/TEXT.
- Import a v4 provider export and confirm IMAGE input metadata is preserved.
- Select a text-only model, attach a synthetic PNG, and confirm send is blocked before `/messages`.
- Select an image-capable model, attach a synthetic PNG, and confirm the message is saved locally with a local-only assistant notice.
- Attach synthetic TXT/PDF documents and confirm they remain local-only.
- Confirm backend `/messages` and `/regenerate` do not call real providers when any non-text message part is present.
- Confirm `state.v1.json` does not contain file content, base64 payloads, original absolute upload paths, or provider request bodies.

Phase 10 P6.1 design checks:

- Confirm `docs/rikkadesk-openai-compatible-image-input-plan.md` exists.
- Confirm the recommended prototype path is OpenAI-compatible Chat Completions content array.
- Confirm Responses API, OpenAI Files API, OCR, PDF parsing, audio input, video input, Workspace, MCP, and tools remain deferred.
- Confirm the design requires explicit per-send confirmation before any image is sent to a provider.
- Confirm the design requires in-memory data URLs only and forbids base64 in `state.v1.json`, conversation parts, provider import/export, logs, and errors.
- Confirm the design limits provider-bound image input to one current-turn PNG/JPEG/WEBP image with a 5 MB limit.
- Confirm the design keeps GIF, document, PDF, TXT, audio, and video attachments local-only.
- Confirm the design defers image regenerate support and does not resend historical image attachments.
- Confirm the design requires a local synthetic capture server before optional manual real-provider testing.
- Confirm current implementation still does not send attachments to providers.

Phase 10 P6.2 static checks:

- Confirm text-only builder output still serializes `messages[].content` as a string.
- Confirm the internal vision builder can serialize Chat Completions content-array messages.
- Confirm internal `file_id` is not serialized into provider request JSON.
- Confirm unsupported data URL prefixes are rejected, including GIF, SVG, HTML, local file URLs, managed file URLs, and external HTTP(S) URLs.
- Confirm no runtime path calls the internal vision builder.
- Confirm attachment messages still return the local-only attachment notice.
- Confirm no provider call occurs for non-text message parts.
- Confirm no image blob is read, no file-derived base64 is generated, and no image request is sent.

Phase 10 P6.3 confirmation UI checks:

- Confirm TEXT-only model plus image attachment is blocked before `/messages`.
- Confirm TEXT-only model plus image attachment does not open the image confirmation dialog.
- Confirm IMAGE-capable model plus image attachment opens the confirmation dialog.
- Confirm Cancel keeps the draft text and attachments and does not call `/messages`.
- Confirm Continue calls `/messages`; without loopback capture eligibility, the backend still returns the local-only attachment notice.
- Confirm document-only attachments do not show the image confirmation dialog.
- Confirm text-only messages do not show the image confirmation dialog.
- Confirm P6.3 alone did not call the internal vision builder; P6.4 may call it only for loopback capture.
- Confirm P6.3 alone did not send image requests; P6.4 may send only to loopback capture.
- Confirm no image blob is read, no file-derived base64 is generated, and no base64 appears in state or logs.

Phase 10 P6.4 synthetic capture-server checks:

- Confirm TEXT-only model plus PNG is blocked before `/messages`.
- Confirm TEXT-only model plus GIF is also blocked before `/messages`.
- Confirm IMAGE-capable model plus PNG opens confirmation.
- Confirm Cancel keeps draft/attachments and capture server receives no request.
- Confirm Continue sends `/messages` with non-persistent capture intent.
- Confirm loopback capture server receives one request for `http://127.0.0.1:9999/v1`.
- Confirm request body has Chat Completions `messages[].content[]` array with text and `image_url` parts.
- Confirm `image_url.url` starts with `data:image/png;base64,` for the synthetic PNG.
- Confirm request body does not contain `/api/files/path`, `file://`, Windows paths, storage keys, or `secretRef`.
- Confirm state does not contain base64, `image_url`, `input_image`, request body, local absolute paths, or storage keys in provider-bound data.
- Confirm IMAGE-capable GIF does not open capture confirmation, remains local-only, and does not call capture server.
- Confirm TXT and PDF attachments do not open image confirmation, remain local-only, and do not call capture server.
- Confirm two provider-bound PNG/JPEG/WEBP images are blocked before `/messages` with a safe one-image prototype error.
- Confirm non-loopback provider Base URLs are rejected or local-only even after confirmation.
- Confirm no real provider endpoint or real API key is used.

Phase 10 P6.5 P0 manual gate checks:

- Confirm `docs/rikkadesk-real-provider-image-manual-gate.md` exists.
- Confirm real-provider image send is still not enabled in code.
- Confirm loopback-only capture remains the only implemented image-send behavior.
- Confirm no real-provider test was run.
- Confirm no real API key appears in docs, logs, commits, or terminal output.
- Confirm the manual gate requires a synthetic 1x1 PNG only.
- Confirm the manual gate forbids real user images, documents, GIF, audio, video, regenerate, and historical image resend.
- Confirm the manual gate does not allow a "do not ask again" option.
- Confirm the future code gate requires explicit confirmation, IMAGE capability, exactly one PNG/JPEG/WEBP image, a 5 MB limit, and a manual enable gate.
- Confirm the state/log check protocol excludes `mock-api/secrets/**`.
- Confirm beta.12 notes do not claim real-provider image input is generally enabled.

beta.13 hotfix live UI smoke checks:

- Use clean synthetic app data; restore real app data after the smoke.
- Confirm no real API key is used and no `mock-api/secrets/*.bin` file is read.
- Confirm plain text mock chat still works.
- Upload a synthetic PNG and confirm the draft chip image renders instead of showing a broken image.
- Delete the draft PNG and select the same PNG again; confirm upload fires again and the draft chip renders.
- Send the synthetic PNG with an IMAGE-capable non-loopback provider and confirm the confirmation dialog appears.
- Continue the confirmation and confirm the backend returns the loopback-only safe block instead of sending to a real provider.
- Confirm the sent user message renders the local image and does not show "Image unavailable" or "图片附件不可用".
- Restart RikkaDesk, reopen the conversation, and confirm the local image still renders.
- Upload/send synthetic JPEG and WEBP images and confirm they render locally with the loopback-only safe block.
- Upload/send a synthetic GIF and confirm it remains local-only and does not trigger capture confirmation.
- Upload/send synthetic TXT and PDF files and confirm they render as document chips only.
- Select a TEXT-only model, attach a synthetic PNG, and confirm the frontend blocks send before `/messages`.
- Try unsupported SVG and HTML files and confirm the UI shows a friendly unsupported-format error instead of silently doing nothing.
- Confirm `state.v1.json` does not contain `base64`, `image_url`, `input_image`, provider request bodies, local absolute paths, or key/header values.
- Confirm beta.13 post-tag checks do not move or overwrite the tag.

## Test Real OpenAI-Compatible Streaming Chat

1. Configure Provider Settings with a real OpenAI-compatible endpoint and a local user-entered API key.
2. Close Provider Settings.
3. Open the model selector and choose the configured model.
4. Send a simple text-only test message.
5. Confirm the assistant text appears progressively.
6. Confirm the final assistant message remains after generation completes.
7. Restart RikkaDesk.
8. Confirm the conversation and assistant reply are still present.

Signals that the real provider path was used:

- The selected model matches the configured provider model.
- Provider Settings shows `hasSecret: true`.
- The response is not the fixed mock fallback text.
- Streaming text appears incrementally.
- Test Connection succeeds for the same provider configuration.

## Test Mock Fallback

To return to mock fallback behavior:

1. Open Provider Settings.
2. Click `Clear API Key`, or save a provider without a key.
3. Optionally select a model that does not map to a usable provider secret.
4. Send a text message.

Expected result:

- The app remains usable.
- The conversation is saved.
- A mock reply or safe provider error is shown.
- No secret value appears in the UI or logs.

## Confirm Secrets Are Not Plaintext

Use a short fragment of a test key. Do not paste the full key into terminal history.

From the repository root:

```powershell
rg --fixed-strings "<key-fragment>" .
```

From app data:

```powershell
rg -a --fixed-strings "<key-fragment>" "$env:APPDATA/com.cisyamx.rikkadesk"
```

Correct result:

- No plaintext match in the repository.
- No plaintext match in app data files.
- `state.v1.json` includes only `secretRef`.
- `/api/desktop/providers` returns `hasSecret`, not the secret value.

Provider API sanity check:

```powershell
Invoke-RestMethod -Method Get -Uri "http://127.0.0.1:8080/api/desktop/providers" |
  ConvertTo-Json -Depth 10
```

The response must not include `apiKey`, `accessToken`, `refreshToken`, `Authorization`, or `x-api-key`.

## Clear Local Test Data

Close RikkaDesk before clearing local data.

To back up then clear beta data:

```powershell
$AppData = Join-Path $env:APPDATA "com.cisyamx.rikkadesk"
$Backup = Join-Path $env:APPDATA ("com.cisyamx.rikkadesk.backup." + (Get-Date -Format "yyyyMMdd-HHmmss"))
if (Test-Path -LiteralPath $AppData) {
  Copy-Item -LiteralPath $AppData -Destination $Backup -Recurse
  Remove-Item -LiteralPath $AppData -Recurse -Force
}
```

This removes:

- local settings
- conversations and messages
- provider config
- encrypted beta secret blobs

The next RikkaDesk launch recreates default mock data.

## Local Installation Smoke Test

Use a fake key for packaging verification unless a human tester is manually validating a real provider. Keep the fake value local to the test run and do not add it to this document or any committed file.

Checklist:

- Install the NSIS `.exe`.
- Start RikkaDesk.
- Open Provider Settings.
- Save a test provider and fake key.
- Confirm `hasSecret: true`.
- Confirm the API Key field clears.
- Add, edit, and delete a second provider.
- Confirm deleting a provider removes its local secret.
- Add two models under one provider and confirm both appear in the model selector.
- Delete one model and confirm current/favorite fallback behavior is safe.
- Add safe Advanced request config custom headers/body, then save, reopen, clear, and confirm persistence behavior.
- Confirm sensitive custom headers/body are rejected.
- Favorite and unfavorite a provider model from the model selector.
- Use Set as current model from Provider Settings.
- Run Test Connection for a specific model with both a valid test endpoint and an intentionally invalid endpoint when available.
- Export providers and confirm the JSON is version 4 with `providers[].models[]`, `inputModalities`, `outputModalities`, safe `customHeaders[]`, and safe `customBody`.
- Import a version 4 provider export with model modality metadata and safe advanced config, then confirm imported providers have `hasSecret: false`.
- Import a version 3 provider export with multiple models and safe advanced config, then confirm imported providers have `hasSecret: false`.
- Import a version 2 provider export with multiple models and confirm imported providers have `hasSecret: false` and no advanced config.
- Import an older version 1 provider export and confirm it imports as a single model.
- Confirm exports exclude API keys, `secretRef`, tokens, Authorization headers, `x-api-key`, cookies, DPAPI blobs, local secret-store files, and model internal ids.
- Send a text message.
- Restart RikkaDesk.
- Confirm the conversation remains.
- Confirm Provider Settings still shows `hasSecret: true`.
- Uninstall RikkaDesk.
- Check whether app data and encrypted local secret blobs remain.
- Tell beta testers that uninstall may not remove app data or secrets.
- Run plaintext key searches for the fake key.

Expected security result, replacing the placeholder with the fake key fragment used in the local UI:

```powershell
rg --fixed-strings "<fake-key-fragment>" .
rg -a --fixed-strings "<fake-key-fragment>" "$env:APPDATA/com.cisyamx.rikkadesk"
```

Both searches should return no matches after the fake key is saved through Provider Settings.

## Release Preflight

Before sharing a local beta installer:

- Confirm this is not published as a public GitHub Release.
- Confirm `origin` points to the RikkaDesk fork, not upstream RikkaHub.
- Confirm `LICENSE` remains present.
- Confirm `NOTICE.md` states this is an unofficial derivative.
- Confirm AGPL and upstream commercial-license boundaries are mentioned.
- Run `git status --short` and verify no local secrets or app data are staged.
- Run `pnpm run desktop:build` from a clean working tree.
- Keep hashes and artifact paths in the private beta notes.
- Tell testers not to share logs containing prompts or local data.
- Confirm `rikkadesk-v0.1.0-beta.13` remains on its original commit.
- Confirm `rikkadesk-v0.1.0-beta.14` points to `af1f9d8502f3afd198c7b00139d25c7be3b8907c`.
- Do not move or overwrite any existing tag.

## Known Limits

- Only OpenAI-compatible text chat is supported for real provider testing by default.
- Streaming support handles text deltas only.
- Stop/cancel behavior is minimal and may not abort the underlying provider request immediately.
- Provider Settings supports multi-provider list, add, edit, delete, multiple models per provider, favorite model, Set as current model, and per-model Test Connection flows.
- API keys are not exported or synced.
- Local file attachments and safe attachment rendering are implemented for the desktop beta candidate.
- Real-provider image input is not enabled by default.
- The image capture prototype is loopback-only and intended for synthetic local testing.
- No OCR, PDF/Office parsing, audio/video input, tools, MCP, search, Workspace, forks, or full multimodal provider calls.
- Local JSON state is a beta prototype store, not a final database schema.
- Installers and `rikkadesk.exe` are unsigned unless a signing workflow is added later.
- Windows 11 Smart App Control may block unsigned private beta builds before the app starts.
