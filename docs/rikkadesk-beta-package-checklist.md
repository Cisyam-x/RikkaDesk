# RikkaDesk Local Beta Package Checklist

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This checklist prepares the first local Windows beta package for private testing only. It does not publish a GitHub Release.

Do not paste real API keys into documentation, commit messages, terminal transcripts, screenshots, or issue comments.

Current feature-stable private beta tag: `rikkadesk-v0.1.0-beta.10`.

## Current Beta Scope

Included:

- Tauri v2 Windows desktop shell.
- Local Rust HTTP API bound to `127.0.0.1`.
- JSON persistence for settings, conversations, messages, provider config, and id sequence.
- Desktop Provider Settings UI for OpenAI-compatible provider management.
- Provider list, add/edit/delete provider, multiple models per provider, favorite model updates, Set as current model, and Test Connection.
- Advanced request config for non-sensitive provider custom headers and safe custom body JSON.
- Safe provider import/export v3 for multi-model provider metadata and safe advanced request config, with v1/v2 import compatibility.
- Secret references in JSON and encrypted local secret blobs for API keys.
- OpenAI-compatible text chat with streaming responses.
- Mock fallback when a real provider is not configured or cannot be used.

Not included:

- Public GitHub Release publishing.
- Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- Files, attachments, images, audio, tools, MCP, search, Workspace, forks, or release auto-updates.
- SQLite, sync, multi-device backup, or production-grade migration tooling.
- Any change to the upstream Android `app` module.

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

Uninstalling the app may leave user data and encrypted local secret blobs in the app data directory. That is normal for many desktop apps, but beta testers should know how to clear it manually.

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

## `state.v1.json`

`state.v1.json` stores non-sensitive local state for the desktop prototype. The filename remains `state.v1.json` even though the JSON payload may contain `schemaVersion: 4`.

Phase 8 upgrades provider state from `provider.model` to `provider.models[]`. Old beta.8 or earlier builds should not be started against a schema v3 state file; they may treat the state as unsupported and create a corrupt backup or default state. Back up app data before testing schema migration or moving between beta builds.

Phase 9B upgrades provider state to `schemaVersion: 4` with `providers[].customHeaders` and `providers[].customBody`. Old beta.9 or earlier builds should not be started against a schema v4 state file; they may not understand the provider shape. Back up app data before testing schema migration or moving between beta builds.

It may contain:

- settings
- conversations
- messages
- idSeq
- provider id, name, type, enabled flag
- baseUrl
- model ids and displayNames under `providers[].models[]`
- non-sensitive custom headers under `providers[].customHeaders`
- safe custom body JSON under `providers[].customBody`
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

## `mock-api/secrets/*.bin`

`mock-api/secrets/*.bin` stores encrypted secret blobs for the local desktop prototype on Windows. These files are not source-controlled and must stay in the user's app data directory.

The JSON state stores only a `secretRef`. The Rust backend uses that `secretRef` to find the encrypted local secret. The UI should only show `hasSecret: true` or `hasSecret: false`; it must never display the saved key.

Do not copy these files into the repository, README, logs, screenshots, or issue reports.

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
- Export providers and confirm the JSON is version 3 with `providers[].models[]`, safe `customHeaders[]`, and safe `customBody`.
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

## Known Limits

- Only OpenAI-compatible text chat is supported.
- Streaming support handles text deltas only.
- Stop/cancel behavior is minimal and may not abort the underlying provider request immediately.
- Provider Settings supports multi-provider list, add, edit, delete, multiple models per provider, favorite model, Set as current model, and per-model Test Connection flows.
- API keys are not exported or synced.
- No file attachments, images, audio, tools, MCP, search, Workspace, forks, or multimodal provider calls.
- Local JSON state is a beta prototype store, not a final database schema.
- Installers and `rikkadesk.exe` are unsigned unless a signing workflow is added later.
- Windows 11 Smart App Control may block unsigned private beta builds before the app starts.
