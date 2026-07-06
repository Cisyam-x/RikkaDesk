# RikkaDesk Provider Custom Request Config Design

Review date: 2026-07-06

This document defines the Phase 9B design for provider-level custom request headers and custom request body fields. It is a design and schema draft only. Phase 9B P1 does not enable the feature, does not change `schemaVersion`, and does not modify backend, UI, Tauri, or Android code.

## Background

RikkaDesk currently supports:

- OpenAI-compatible providers.
- Multiple providers.
- Multiple models under one provider.
- Local provider state with `schemaVersion: 3`.
- Provider Settings.
- Per-model Set as current.
- Per-model Test Connection.
- Provider import/export version 2.
- SecretStore / Windows DPAPI storage for API keys.
- Long OpenAI-compatible streaming chat through background generation.

RikkaDesk does not yet support:

- Custom provider headers.
- Custom provider request body fields.
- An Advanced request config section in Provider Settings.
- Advanced compatibility options for relay services, enterprise gateways, and local OpenAI-compatible servers.

The goal of Phase 9B is to improve OpenAI-compatible provider interoperability without weakening the existing secret boundary.

## Current State Summary

Current backend provider state:

- `DesktopProviderConfig`: `id`, `type`, `enabled`, `name`, `baseUrl`, `models[]`, `secretRef`.
- `DesktopProviderModelConfig`: `id`, `modelId`, `displayName`.
- `OpenAiChatConfig`: `baseUrl`, `modelId`, `apiKey`.
- `OpenAiChatCompletionRequest`: `model`, `messages`, `stream`, `max_tokens`.
- Test Connection and streaming both add the Authorization header with `bearer_auth(apiKey)`.
- Test Connection keeps a 60 second timeout.
- Streaming requests do not use a total request timeout.

Current frontend provider settings:

- Provider Settings shows a provider list.
- Each provider can contain multiple model rows.
- Saved model rows support Set current and Test.
- There is no Advanced request config section.

## Goals

Phase 9B should support:

- Non-sensitive custom headers saved in local provider JSON.
- A safe custom body JSON object saved in local provider JSON.
- A shared request building path for Test Connection and Chat Streaming.
- Provider import/export support for safe advanced request config.
- Conservative validation that rejects credentials and reserved request fields.

Phase 9B should not support:

- Per-model custom headers.
- Per-model custom body.
- Sensitive custom headers stored as normal JSON.
- SecretStore-backed custom headers.
- Proxy configuration.
- New provider protocols.
- Anthropic, Gemini, Claude, or Vertex-specific protocols.
- Files, attachments, search, MCP, tools, multimodal, or Workspace.

## Schema V4 Draft

If Phase 9B persists custom request config, it should upgrade local state from `schemaVersion: 3` to `schemaVersion: 4`.

The state filename remains:

```text
state.v1.json
```

`DesktopProviderConfig` should add provider-level fields:

```rust
custom_headers: Vec<DesktopProviderCustomHeaderConfig>
custom_body: Option<serde_json::Value>
```

Suggested header config:

```rust
struct DesktopProviderCustomHeaderConfig {
    name: String,
    value: String,
}
```

Example persisted shape:

```json
{
  "schemaVersion": 4,
  "providers": [
    {
      "id": "desktop-provider-1",
      "type": "openai-compatible",
      "enabled": true,
      "name": "Gateway",
      "baseUrl": "https://gateway.example.com/v1",
      "models": [
        {
          "id": "desktop-model-1",
          "modelId": "deepseek-chat",
          "displayName": "DeepSeek Chat"
        }
      ],
      "secretRef": "rikkadesk:provider:desktop-provider-1:api-key",
      "customHeaders": [
        {
          "name": "OpenAI-Beta",
          "value": "assistants=v2"
        }
      ],
      "customBody": {
        "temperature": 0.7,
        "top_p": 0.9
      }
    }
  ]
}
```

### Migration

Version 3 to version 4 migration should:

- Set `customHeaders` to an empty array for every provider.
- Set `customBody` to `null` for every provider.
- Preserve provider `id`, `type`, `enabled`, `name`, `baseUrl`, `models[]`, and `secretRef`.
- Preserve settings, assistants, conversations, favorite models, and current model IDs.
- Not read or modify any SecretStore or DPAPI blob.
- Save future writes as `schemaVersion: 4`.

Once a state file is written as `schemaVersion: 4`, beta.9 and earlier builds should not be used against it.

## Custom Headers Safety Rules

Only non-sensitive headers should be accepted in Phase 9B.

Examples of allowed header names:

- `OpenAI-Beta`
- `x-gateway-route`
- `x-provider-mode`
- Other ordinary gateway headers that do not carry credentials.

Header name checks should be case-insensitive.

Forbidden header names:

- `authorization`
- `proxy-authorization`
- `x-api-key`
- `api-key`
- `apikey`
- `api_key`
- `cookie`
- `set-cookie`
- `authentication`
- `x-auth-token`
- `x-access-token`

Forbidden terms in header names or values:

- `bearer`
- `token`
- `secret`
- `password`
- `passwd`
- `credential`
- `api key`
- `sk-`
- `refresh_token`
- `access_token`

These checks cannot perfectly detect every credential shape, so Phase 9B should intentionally be conservative. If a gateway requires sensitive headers such as `x-api-key`, that should be handled later with a separate SecretStore-backed design. Sensitive custom headers must not enter normal provider JSON, import/export files, logs, or screenshots.

## Custom Body Safety Rules

The custom body must be a JSON object.

Allowed examples:

- `temperature`
- `top_p`
- `max_tokens`
- `presence_penalty`
- `frequency_penalty`
- `response_format`
- `reasoning_effort`
- `seed`
- `stop`
- `user`
- Other non-sensitive OpenAI-compatible request fields.

Forbidden top-level fields:

- `model`
- `messages`
- `stream`
- `apiKey`
- `api_key`
- `authorization`
- `x-api-key`
- `token`
- `accessToken`
- `refreshToken`
- `password`
- `secret`
- `credential`

Recommended body limits:

- Top-level value must be an object.
- Top-level array must be rejected.
- Top-level string, number, boolean, and null must be rejected.
- Serialized custom body should be limited to 16 KB or 32 KB.
- Optional depth limit can be added if nested objects become a problem.

Core request ownership:

- RikkaDesk always controls `model`.
- RikkaDesk always controls `messages`.
- RikkaDesk always controls `stream`.
- Test Connection always forces `max_tokens = 1`.
- Streaming chat may use a user-provided allowed `max_tokens` value.

## Request Builder Design

Test Connection and streaming should use one shared request building rule set.

Suggested helper:

```rust
fn build_openai_chat_request_body(
    config: &OpenAiChatConfig,
    messages: Vec<OpenAiChatMessage>,
    stream: bool,
    request_kind: OpenAiRequestKind,
) -> Result<serde_json::Value, String>
```

Suggested request kind:

```rust
enum OpenAiRequestKind {
    TestConnection,
    StreamingChat,
}
```

Body construction order:

1. Clone the provider `customBody` object, or start with an empty object.
2. Validate that the object contains no reserved or sensitive fields.
3. Write RikkaDesk-owned `model`.
4. Write RikkaDesk-owned `messages`.
5. Write RikkaDesk-owned `stream`.
6. If `request_kind == TestConnection`, force `max_tokens = 1`.
7. If `request_kind == StreamingChat`, keep an allowed custom `max_tokens` if present.

Header construction order:

1. Start the reqwest request builder.
2. Add validated non-sensitive custom headers.
3. Add RikkaDesk-owned bearer auth with the provider API key.

Custom headers must never be allowed to overwrite Authorization, `x-api-key`, cookie, or other credential-like headers.

Errors returned by the request builder should be safe. They must not include the full request body, full headers, API key, Authorization header, or local secret reference.

## API Draft

The minimal API path should reuse:

```text
POST /api/desktop/providers
GET /api/desktop/providers
```

`UpsertDesktopProviderRequest` can add:

```ts
customHeaders?: Array<{ name: string; value: string }>
customBody?: unknown
```

`DesktopProviderResponse` can add:

```ts
customHeaders: Array<{ name: string; value: string }>
customBody: unknown | null
```

Rules:

- Only validated non-sensitive custom config is saved.
- The API response returns only non-sensitive custom config.
- The API response must not return an API key.
- Existing `secretRef` behavior should not be expanded.
- P2 can keep using provider upsert; a separate advanced endpoint is not needed initially.

Possible future endpoint, not recommended for P2:

```text
POST /api/desktop/providers/{id}/advanced-request-config
```

## Import / Export V3 Draft

Current provider export version is 2 and supports `providers[].models[]`.

If Phase 9B exports advanced request config, provider export should become version 3 and imports should remain compatible with versions 1 and 2.

Version 3 provider export fields:

- `type`
- `enabled`
- `name`
- `baseUrl`
- `hasSecret`
- `models[]`
- `customHeaders[]`
- `customBody`

Version 3 export must not include:

- API key.
- `secretRef`.
- Authorization header values.
- `x-api-key`.
- `api-key`.
- Access tokens.
- Refresh tokens.
- Passwords.
- Cookies.
- Windows DPAPI blob.
- SecretStore raw values.
- `mock-api/secrets/*.bin`.
- Local app data paths.
- Full request logs.

Import behavior:

- Version 1 import sets `customHeaders = []` and `customBody = null`.
- Version 2 import sets `customHeaders = []` and `customBody = null`.
- Version 3 import validates custom headers and custom body with the same backend rules used by provider save.
- Sensitive advanced fields should fail import instead of being silently dropped.
- Imported providers still have `hasSecret = false`.
- Import does not set current model.
- Import does not favorite imported models.
- Import does not overwrite existing providers.

## UI Draft

Provider Settings should add an Advanced request config section.

Placement:

- Prefer after provider basic fields and the Models section.
- API Key can stay near the bottom, but the Advanced section should not crowd the primary setup flow.
- The section should be collapsed by default.

Suggested English copy:

- Section title: `Advanced request config`
- Description: `Optional non-secret request headers and JSON body fields for OpenAI-compatible gateways.`
- Warning: `Do not enter API keys, tokens, passwords, Authorization headers, or cookies here. API keys belong in the API Key field and are saved in the local SecretStore.`
- Custom headers title: `Custom headers`
- Add header: `Add header`
- Remove header: `Remove header`
- Header name: `Header name`
- Header value: `Header value`
- Custom body title: `Custom body JSON`
- Format JSON: `Format JSON`
- Clear JSON: `Clear JSON`
- Invalid JSON: `Custom body must be a valid JSON object.`
- Sensitive header error: `This header looks sensitive. Store API keys only in the API Key field.`

Suggested Chinese copy:

- Section title: `高级请求配置`
- Description: `用于 OpenAI-compatible 网关的可选非敏感请求 Header 和 JSON Body 字段。`
- Warning: `不要在这里填写 API Key、Token、密码、Authorization Header 或 Cookie。API Key 应填写在 API Key 输入框，并保存到本地 SecretStore。`
- Custom headers title: `自定义 Header`
- Add header: `新增 Header`
- Remove header: `删除 Header`
- Header name: `Header 名称`
- Header value: `Header 值`
- Custom body title: `自定义 Body JSON`
- Format JSON: `格式化 JSON`
- Clear JSON: `清空 JSON`
- Invalid JSON: `自定义 Body 必须是合法的 JSON object。`
- Sensitive header error: `这个 Header 看起来包含敏感信息。API Key 只能保存到 API Key 输入框。`

UI validation:

- Advanced section is collapsed by default.
- Header rows can be added and deleted.
- Empty header rows are ignored or blocked consistently.
- Sensitive header names are blocked before save.
- Sensitive header values are blocked before save.
- Custom Body must parse as a JSON object.
- Custom Body must pass reserved-field validation before save.
- Format JSON should never log the raw content.
- Errors should not include full headers or full custom body content.

## Implementation Split

Recommended Phase 9B sequence:

### P2: Backend Schema V4 And Request Builder

Files:

- `web-ui/src-tauri/src/mock_api.rs`

Scope:

- Upgrade state to `schemaVersion: 4`.
- Add `customHeaders` and `customBody` to provider state.
- Add v3 to v4 migration defaults.
- Add backend validation for custom headers and custom body.
- Add shared OpenAI request builder for Test Connection and streaming.
- Apply validated custom headers and custom body to Test Connection and streaming.
- Keep UI and import/export unchanged in P2.

Validation:

- Synthetic v3 to v4 migration.
- Save/read provider advanced config through API-level test calls.
- Test Connection forces `max_tokens = 1`.
- Streaming remains free of total request timeout.
- Sensitive headers and reserved body keys fail safely.

### P3: Provider Settings Advanced UI

Files:

- `web-ui/app/components/provider-settings-dialog.tsx`
- `web-ui/app/locales/en-US/common.json`
- `web-ui/app/locales/zh-CN/common.json`

Scope:

- Add collapsed Advanced request config section.
- Add custom header rows.
- Add custom body JSON textarea.
- Add client-side validation and formatting.
- Save advanced config through existing provider upsert.

Validation:

- Add/delete header.
- Block Authorization and `x-api-key`.
- Validate custom body JSON object.
- Save, close, reopen, and confirm persistence.
- Confirm API key still uses SecretStore only.

### P4: Import / Export Version 3

Files:

- `web-ui/src-tauri/src/mock_api.rs`
- `web-ui/app/components/provider-settings-dialog.tsx`
- `web-ui/app/locales/en-US/common.json`
- `web-ui/app/locales/zh-CN/common.json`
- `docs/rikkadesk-provider-import-export-safety.md`

Scope:

- Emit export version 3 with safe `customHeaders` and `customBody`.
- Keep import compatibility for versions 1 and 2.
- Validate version 3 advanced config during preview and confirm.
- Show advanced config summary in import preview.
- Reject sensitive imported advanced config.

Validation:

- Export excludes API key, `secretRef`, tokens, cookies, DPAPI blobs, and secret-store files.
- Import v1 and v2 still works.
- Import v3 with safe advanced config works.
- Import v3 with sensitive advanced config fails safely.

### P5: Smoke Test And Release Copy

Scope:

- Test with a local stub or user-managed real gateway.
- Do not ask Codex for real API keys.
- Update About, README, CHANGELOG, beta checklist, release draft, provider settings docs, dev docs, and import/export safety docs.
- Prepare beta.10 only after manual validation.

## Test Plan

Backend tests and smoke checks:

- `schemaVersion: 3` state migrates to `schemaVersion: 4`.
- Providers default to `customHeaders: []`.
- Providers default to `customBody: null`.
- Custom headers save and load.
- Custom body saves and loads.
- Forbidden header names fail.
- Header values containing sensitive terms fail.
- Custom body reserved keys fail.
- Custom body non-object values fail.
- Custom body size limit fails safely.
- Test Connection applies safe custom headers and body.
- Streaming applies safe custom headers and body.
- Test Connection always forces `max_tokens = 1`.
- Streaming remains compatible with the long-stream background task.

UI tests:

- Advanced section is collapsed by default.
- Header row add/delete works.
- Authorization header is blocked.
- `x-api-key` header is blocked.
- Custom body JSON parse errors are visible.
- JSON formatting works without logging content.
- Saved custom config appears after reopening Provider Settings.

Security tests:

- Export does not contain API keys.
- Export does not contain `secretRef`.
- Export does not contain Authorization or `x-api-key` values.
- Export does not contain tokens, passwords, cookies, DPAPI blobs, or local secret-store file contents.
- Logs do not print full headers.
- Logs do not print full request bodies.
- Error messages do not echo sensitive values.
- App data JSON does not contain real API keys, tokens, or passwords.

## Risks And Decisions

Risks:

- Users may try to put credentials into normal headers or custom body.
- Value detection can false-positive or miss an unknown credential format.
- Some gateways require `x-api-key`, which Phase 9B intentionally blocks.
- Custom body may conflict with core request fields.
- Import/export could accidentally leak advanced config if validation is not shared.
- Test Connection and streaming may diverge if they use separate builders.
- `schemaVersion: 4` will be incompatible with beta.9 and earlier.

Decisions:

- Phase 9B prioritizes safety over maximum gateway compatibility.
- Sensitive custom headers are out of scope for normal JSON storage.
- Gateways that require `x-api-key` should be handled later with a SecretStore-backed sensitive header design.
- RikkaDesk should reject reserved request fields instead of allowing custom body to override them.
- Test Connection and streaming should share request-building validation.

## Upstream Reference

Upstream Android has `CustomHeader`, `CustomBody`, and request body merge concepts. RikkaDesk should use the idea as a reference but not copy the looser behavior directly.

RikkaDesk differences:

- Reserved request keys should be rejected rather than overwritten.
- Sensitive headers should be rejected rather than saved in normal provider JSON.
- Import/export should keep advanced config non-sensitive.
- Test Connection and streaming should share one backend validation path.

## Follow-Up Documentation

After implementation, update:

- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-dev.md`
- `docs/rikkadesk-provider-settings-ui.md`
- `docs/rikkadesk-provider-import-export-safety.md`
- `docs/rikkadesk-release-draft.md`
