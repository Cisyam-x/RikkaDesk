# RikkaDesk Real-Provider Image Input Manual Gate

Review date: 2026-07-08

This document is the Phase 10 P6.5 P0 design document for a future manual gate around real-provider image input testing. It does not implement real provider image sending, does not require Codex or automation tools to use a real API key, and does not change the current runtime gate.

Current code remains loopback-only for image capture. Real-provider image input is not a public feature, is not enabled for ordinary beta testers, and should not enter the beta.12 default release scope unless a later phase explicitly changes that decision.

## Manual Gate Principles

The manual gate is intentionally narrow:

- Use only a synthetic image.
- Start with exactly one 1x1 PNG.
- Do not use real user photos, screenshots, documents, identity files, chat records, or other private material.
- Do not test PDF, TXT, GIF, audio, video, Office, or archive attachments.
- Do not resend historical images.
- Do not support image regenerate.
- Require explicit confirmation before every image send.
- Do not add a "do not ask again" option.
- API keys may only be entered manually in the local Provider Settings UI by the human tester.
- API keys must not be written to docs, commit messages, terminal transcripts, screenshots, issues, prompts, chat conversations, or logs.
- Request bodies must not be printed.
- Data URLs must not be printed.
- Base64 must not be printed or stored in state.
- Local paths must not be sent to a provider.

## Preconditions For P6.5 P1

All of these must be true before a future P6.5 P1 code review begins:

- P6.4 capture-server smoke passed.
- P6.4 final hardening passed.
- `pnpm run typecheck` passed.
- `cargo check --manifest-path src-tauri/Cargo.toml` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml openai_vision` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml loopback` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml base64` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml provider_bound_image` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml image_data_url` passed.
- `pnpm run desktop:build` passed.
- The worktree is clean.
- No new tag or GitHub Release was created.
- No real-provider image test was run.
- No real API key appears in the repository, docs, terminal transcript, or committed files.
- The future P6.5 P1 code plan has a separate review before implementation.

## Future Code Gate Design

P6.5 P1 is not implemented by this document. A future implementation must require all of these conditions before it can send an image to a real provider:

```text
imageInputConfirmed === true
imageInputMode === "real-provider-manual"
selected model inputModalities contains IMAGE
exactly one provider-bound image
image MIME is PNG/JPEG/WEBP
image size <= 5 MB
current turn only
explicit real-provider confirmation dialog
manual gate enabled
provider base URL is HTTPS unless local testing explicitly allows otherwise
request body never logged
base64 never persisted
```

Recommended hard gate options:

### Option A: Dev-Only Environment Gate

```text
RIKKADESK_ENABLE_REAL_IMAGE_INPUT_MANUAL_TEST=1
```

This is the recommended first P6.5 P1 gate because it is explicit, easy to audit, and does not add a persistent product setting.

### Option B: Local Manual Gate File

```text
%APPDATA%\com.cisyamx.rikkadesk\manual-gates\enable-real-image-input-test
```

This keeps the gate local, but it adds filesystem behavior and cleanup risk.

### Option C: Temporary In-Memory Session Gate

The gate resets on every app start and can only be enabled by a local debug UI or development command. This is safer for users but requires an additional debug surface.

P6.5 P1 should start with Option A. Do not add a persistent UI switch, do not add an "always allow" setting, and do not expose real-provider image input to ordinary private beta testers.

## Manual Real-Provider Test Protocol

This protocol is for a human tester only. Codex and automation tools must not perform the real-provider test, read the API key, or record the API key.

Preparation:

1. Use clean synthetic app data.
2. Do not use real historical app data.
3. Do not read `mock-api/secrets/*.bin`.
4. Prepare one synthetic 1x1 PNG.
5. Manually enter the API key in local Provider Settings.
6. Do not screenshot the API key.
7. Do not paste the API key into a terminal, document, Codex prompt, ChatGPT conversation, issue, or log.
8. Mark the model with IMAGE input in Provider Settings.
9. Confirm the provider policy and cost risk are acceptable to the human tester.

Test steps:

1. Select the IMAGE-capable model.
2. Upload the synthetic 1x1 PNG.
3. Enter this text:

```text
Describe this synthetic test image.
```

4. Click Send.
5. Read the real-provider warning dialog.
6. Confirm once.
7. Confirm the assistant returns a normal text response.
8. Delete the test provider or clear the API key immediately after the test.
9. Close RikkaDesk.
10. Clean the synthetic app data.

## Tests That Must Not Be Run

Do not test with:

- Real photos.
- Identity documents.
- Chat screenshots.
- Images containing names, phone numbers, email addresses, school names, addresses, or account information.
- PDF or TXT documents.
- GIF images.
- Multiple images.
- Regenerate on an image message.
- Historical image resend.
- Proxy gateways unless the tester explicitly understands their provider policy and logging behavior.
- Failure logs that contain request bodies.
- Issue uploads that contain logs with local data.

Also forbidden:

- Do not let Codex read, store, or operate on the API key.
- Do not let Codex run the real-provider image test.
- Do not copy real provider request bodies into docs, issues, chat, or prompts.

## State And Log Check Protocol

Repository check:

```powershell
rg -n "sk-|apiKey|Authorization|x-api-key|Bearer|base64,|image_url|input_image" .
```

App data check, excluding secret blobs:

```powershell
$AppData = Join-Path $env:APPDATA "com.cisyamx.rikkadesk"
rg -n --glob "!mock-api/secrets/**" "sk-|apiKey|Authorization|x-api-key|Bearer|base64,|image_url|input_image|file://|C:\\\\" $AppData
```

State check:

```powershell
$State = Join-Path $env:APPDATA "com.cisyamx.rikkadesk\mock-api\state.v1.json"
Get-Content $State | Select-String -Pattern "base64,|image_url|input_image|Authorization|x-api-key|file://|C:\\"
```

Rules:

- Do not read `mock-api/secrets/*.bin`.
- Do not print complete state in public channels.
- Do not print API keys.
- Record only pass/fail and the category of any finding.

## Failure Handling

All real-provider errors must stay safe:

- 401/403: show a safe authentication failure; do not print key, header, or request body.
- 400: show a safe unsupported image request message; do not print request body.
- 413: show an image-too-large message.
- Timeout/connect errors: show a safe network error.
- Streaming parse failures: show a safe response parse error.

Errors must not contain:

- request body
- data URL
- base64
- local path
- storage key
- API key
- Authorization header

## Beta.12 Recommendation

Beta.12 may include the loopback capture prototype as an experimental development capability if final Phase 10 verification passes. It should not enable real-provider image send by default.

Release copy should be conservative:

- Local loopback capture prototype exists.
- Real-provider image input is not generally enabled.
- Real-provider manual gate is experimental and not for ordinary testers.
- No real-provider image test should be run by Codex or automation.

## P6.5 P0 Acceptance

- This document exists.
- No source code changed.
- No schema version changed.
- Provider import/export version remains unchanged.
- Loopback-only capture remains the only implemented image-send path.
- No real-provider image test was run.
- No real API key was used.
- No secrets blob was read.
- No base64, request body, local path, or real endpoint/key combination was added to committed files.
