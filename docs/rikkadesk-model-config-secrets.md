# RikkaDesk Model Config And Secret Storage Design

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This document records the Phase 3B design for model configuration and secret storage. It is a design-only phase: it does not implement real model calls, a secret store, OpenAI-compatible integration, or a schema upgrade.

Phase 3B must not upgrade `state.v1.json` to `schemaVersion: 2`. The schema migration and any new persisted provider fields are reserved for Phase 3C.

## Current Settings Shape

The web UI consumes a streamed `Settings` object from `/api/settings/stream`. The relevant desktop fields are:

- `providers`: provider profiles shown in the model picker.
- `assistants`: assistant profiles shown in the assistant picker and conversation sidebar.
- `chatModelId`: global default chat model id.
- `assistantId`: currently selected assistant id.
- `favoriteModels`: model ids pinned into the favorites section.

`ProviderProfile` currently contains `id`, `enabled`, `name`, and `models`. Each `ProviderModel` contains `id`, `modelId`, `displayName`, `type`, optional modalities, optional abilities, and optional built-in tools. `AssistantProfile` contains `id`, `name`, optional `chatModelId`, avatar metadata, tags, MCP references, injection references, lorebook references, and quick message references.

The Phase 2C/3A mock backend stores `settings` as JSON inside the local mock state file and returns it through SSE. That is fine for non-sensitive configuration, but it must not become the storage location for credentials.

## Upstream Android Shape

Upstream RikkaHub models providers with a sealed `ProviderSetting` structure:

- `ProviderSetting.OpenAI`
- `ProviderSetting.Google`
- `ProviderSetting.Claude`

These provider settings include normal configuration such as `id`, `enabled`, `name`, `models`, and `baseUrl`. They also include sensitive fields in the same structure, such as API key fields and Google Vertex service account fields. Android `SettingsStore` serializes `providers` into app preferences, so provider configuration and provider credentials are represented together in the upstream data model.

RikkaDesk should preserve the provider/model/assistant relationship, but it should not copy the upstream credential persistence shape.

## Why RikkaDesk Should Not Store Secrets In Settings JSON

RikkaDesk is a local desktop app with a readable JSON mock state file under the Tauri app data directory. JSON is useful because it is easy to inspect, migrate, back up, and debug. Those same properties make it a poor place for credentials.

RikkaDesk should not store secrets in `state.v1.json`, future state files, README files, tests, source code, logs, exported config files, or browser storage. The frontend should not receive secret values after saving them. Rust backend code should resolve secrets only at the point where a provider needs to make a request.

This separation keeps configuration portable while letting the operating system handle credential protection.

## JSON-Safe Configuration

The following fields may be stored in the JSON state file or a future JSON settings file:

- `provider.id`
- `provider.name`
- `provider.type`
- `provider.enabled`
- `provider.baseUrl`
- `provider.chatCompletionsPath`
- `provider.useResponseApi`
- `provider.includeHistoryReasoning`
- `provider.secretRef`
- `model.id`
- `model.modelId`
- `model.displayName`
- `model.type`
- `model.inputModalities`
- `model.outputModalities`
- `model.abilities`
- `assistant.id`
- `assistant.name`
- `assistant.chatModelId`
- `settings.chatModelId`
- `settings.assistantId`
- `settings.favoriteModels`

`secretRef` is safe because it is only a stable reference. It must not contain the secret value itself.

## Never Store These In JSON Or Logs

The following values must not be written to JSON, source code, README files, logs, tests, browser storage, or exported configuration:

- API keys
- Access tokens
- Refresh tokens
- Google service account private keys
- Full service account JSON files
- `Authorization` header values
- `x-api-key` header values
- Custom authentication header values
- WebDAV passwords
- S3 secret access keys
- Search provider credentials
- TTS or ASR provider credentials
- Any other sensitive credential material

Logs may mention whether a secret exists, for example `hasSecret: true`, but must never print the secret value or an unmasked header.

## Secret Reference Design

RikkaDesk should store only a `secretRef` in JSON. The real secret should live in a system-backed secret store.

Recommended reference format:

```text
rikkadesk:provider:<providerId>:api-key
```

Recommended lookup behavior:

1. Provider configuration is loaded from JSON.
2. The provider has a `secretRef`.
3. The Rust backend asks `SecretStore` for the value behind that `secretRef`.
4. The value is used only for the outbound provider request.
5. The value is never returned to the web UI and never printed to logs.

The frontend may display `hasSecret: true` or `hasSecret: false`. It should not display the stored secret after saving.

## Recommended Secret Store

The desktop backend should introduce a small Rust abstraction:

```rust
trait SecretStore {
    fn set_secret(&self, secret_ref: &str, value: SecretString) -> Result<()>;
    fn get_secret(&self, secret_ref: &str) -> Result<Option<SecretString>>;
    fn delete_secret(&self, secret_ref: &str) -> Result<()>;
}
```

The implementation should be selected behind this trait so the app can start simple and evolve without changing the provider schema.

### Preferred Backend

Use system-level credential storage:

- Windows: Credential Manager
- macOS: Keychain
- Linux: Secret Service

The Rust keyring ecosystem is a good candidate because it provides cross-platform access to native credential stores, including Windows Credential Store, macOS Keychain, and Linux Secret Service. RikkaDesk can wrap it behind `SecretStore` and keep all secret operations in Rust.

### Tauri Stronghold Option

Tauri also has the official Stronghold plugin. Stronghold is an encrypted secret database and is listed by Tauri as an official plugin for encrypted secure storage. It may be useful if RikkaDesk wants a cross-platform encrypted vault controlled by the app.

Trade-off: Stronghold introduces password/hash, unlock, recovery, and user experience questions. For a Windows-first desktop prototype, the system credential store is the simpler first target. Stronghold remains a good later option if RikkaDesk needs an app-managed encrypted vault.

### Why Not JSON Or `tauri-plugin-store`

Plain JSON is inspectable by design and is not appropriate for credentials. `tauri-plugin-store` is a persistent key-value file store; it helps persist app state but is not a credential manager. It should be treated like JSON for security purposes.

## Data Structure Examples

Provider:

```json
{
  "id": "provider-openai-compatible-1",
  "type": "openai-compatible",
  "enabled": true,
  "name": "OpenAI Compatible",
  "baseUrl": "https://api.openai.com/v1",
  "chatCompletionsPath": "/chat/completions",
  "useResponseApi": false,
  "includeHistoryReasoning": true,
  "secretRef": "rikkadesk:provider:provider-openai-compatible-1:api-key",
  "models": [
    {
      "id": "model-openai-compatible-chat-1",
      "modelId": "gpt-4o-mini",
      "displayName": "gpt-4o-mini",
      "type": "CHAT",
      "inputModalities": ["TEXT"],
      "outputModalities": ["TEXT"],
      "abilities": []
    }
  ]
}
```

Model:

```json
{
  "id": "model-openai-compatible-chat-1",
  "modelId": "gpt-4o-mini",
  "displayName": "gpt-4o-mini",
  "type": "CHAT",
  "inputModalities": ["TEXT"],
  "outputModalities": ["TEXT"],
  "abilities": []
}
```

Assistant:

```json
{
  "id": "assistant-default",
  "name": "RikkaDesk Assistant",
  "chatModelId": "model-openai-compatible-chat-1",
  "tags": [],
  "mcpServers": [],
  "modeInjectionIds": [],
  "lorebookIds": [],
  "quickMessageIds": []
}
```

Secret reference metadata:

```json
{
  "secretRef": "rikkadesk:provider:provider-openai-compatible-1:api-key",
  "hasSecret": true
}
```

`hasSecret` may be returned by an API response. It should not be persisted as the source of truth unless the backend keeps it synchronized with the secret store.

## Minimal API Direction

Phase 3C can add mock-safe provider configuration APIs without real chat:

- `GET /api/desktop/providers`
- `POST /api/desktop/providers`
- `PUT /api/desktop/providers/{id}`
- `DELETE /api/desktop/providers/{id}`
- `POST /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}/secret`

The secret write endpoint should accept the secret value once, store it through `SecretStore`, and return only status plus `hasSecret`. It should not return the stored value.

## Phase 3C To 3E Route

Phase 3C: Persist provider configuration and `secretRef`.

- Upgrade persisted mock state schema in a controlled migration.
- Add provider config fields to settings JSON.
- Add `secretRef` references only.
- Add a placeholder `SecretStore` interface and, if approved, a system-backed implementation.
- Do not perform real chat calls yet.

Phase 3D: Add one OpenAI-compatible provider path.

- Support `baseUrl`, `modelId`, and `secretRef`.
- Resolve the API key in Rust only.
- Send non-streaming test requests only after explicit approval.
- Keep Gemini, Claude, tools, files, search, MCP, and attachments out of scope.

Phase 3E: Implement real SSE streaming.

- Map provider stream chunks into existing conversation SSE events.
- Preserve stop behavior.
- Avoid logging request headers or secret-derived values.
- Keep the UI protocol aligned with the existing web-ui DTOs.

## Risks And Rules

- API keys must not be displayed after saving.
- Logs must not print secret values or full authentication headers.
- Exported configuration must not include secrets.
- Deleting a provider must ask or decide whether to delete the corresponding secret.
- Schema migrations must never copy secrets into `state.v1.json` or future JSON state files.
- Importers must treat incoming credential fields as secrets and write them to `SecretStore`, not JSON.
- Crash reports and error messages must redact provider authentication details.
- Frontend state must store only input field drafts before save; after save, clear the secret input value.

## Phase 3B Non-Goals

- No real model calls.
- No OpenAI-compatible implementation.
- No secret store implementation.
- No schema upgrade to `schemaVersion: 2`.
- No SQLite.
- No Android `app` changes.
- No file, attachment, search, MCP, tool call, or fork support.
