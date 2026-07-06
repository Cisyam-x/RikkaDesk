# RikkaDesk Provider Import/Export Safety Design

Review date: 2026-07-04

This document defines the safety boundary for RikkaDesk provider configuration import/export. Phase 7 implemented safe non-sensitive import/export, Phase 8 upgraded the export document to version 2 for multiple `models[]`, and Phase 9B upgrades the export document to version 3 for safe non-sensitive `customHeaders[]` and `customBody`.

## Goals

- Allow users to back up and transfer non-sensitive provider configuration.
- Prevent API keys, tokens, or local credential blobs from entering exported files.
- Make imported providers explicit and reviewable before they affect local settings.
- Keep provider exports non-sensitive even when local state uses `state.v1.json` with `schemaVersion: 4`, `providers[].models[]`, `providers[].customHeaders`, and `providers[].customBody`.
- Keep version 1 and version 2 provider export imports compatible while emitting version 3 exports for multi-model providers with safe advanced request config.

## Exportable Provider Fields

A safe provider export may include only non-sensitive configuration:

- `type`: provider type, currently `openai-compatible`.
- `name`: user-facing provider name.
- `baseUrl`: provider base URL.
- `models[]`: provider model metadata.
  - `modelId`: provider API model identifier.
  - `displayName`: user-facing model display name.
- `customHeaders[]`: non-sensitive custom request headers that pass the backend allowlist/denylist validation.
  - `name`: HTTP header name.
  - `value`: non-sensitive HTTP header value.
- `customBody`: non-sensitive JSON object fields that pass backend validation.
- `enabled`: enabled state, if present in the current provider model.
- `hasSecret`: boolean state such as `true` or `false`, used only to tell the user whether the original provider had a saved secret.

Example safe export shape:

```json
{
  "version": 3,
  "app": "RikkaDesk",
  "exportedAt": "2026-07-04T00:00:00Z",
  "providers": [
    {
      "type": "openai-compatible",
      "enabled": true,
      "name": "Example Provider",
      "baseUrl": "https://example.invalid/v1",
      "hasSecret": true,
      "models": [
        {
          "modelId": "example-model",
          "displayName": "Example Model"
        },
        {
          "modelId": "example-reasoner",
          "displayName": "Example Reasoner"
        }
      ],
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

`hasSecret` is informational only. It must not imply that the key itself is included or recoverable from the export.

## Forbidden Export Fields

Provider export must never include:

- API keys.
- Access tokens.
- Refresh tokens.
- Authorization header values.
- `x-api-key` header values.
- Cookie values.
- Passwords or credential-bearing custom header/body fields.
- Local provider IDs.
- Internal model IDs.
- SecretStore raw values.
- Windows DPAPI blobs.
- macOS Keychain items.
- Linux Secret Service values.
- Any local encrypted credential blob or raw secret file.
- Sensitive request headers or body fields that could contain credentials.
- Runtime logs, error payloads, or request bodies that contain sensitive credential material.

### `secretRef` Export Policy

`secretRef` should not be exported by default.

Reasons:

- A `secretRef` is a local pointer into this device's SecretStore namespace.
- Reusing it across machines can be misleading because the referenced secret will not exist on the new machine.
- Reusing it on the same machine can accidentally bind an imported provider to an existing secret without enough user intent.
- It can reveal local naming conventions for credentials.

If a future implementation needs to preserve reference identity, export a non-reusable marker instead, for example:

```json
{
  "hasSecret": true,
  "secretStatus": "present-on-source-device"
}
```

The importer must still require the user to enter a fresh API key.

## Import Behavior

Provider import should restore only non-sensitive configuration.

Recommended behavior:

- Show an import preview before writing anything.
- Restore provider name, type, base URL, model IDs, display names, and enabled state.
- Never restore an API key or SecretStore entry from the import file.
- After import, show `hasSecret: false` for imported providers until the user manually enters a new API key.
- Do not automatically overwrite an existing provider.
- Do not automatically set imported providers as the current model.
- Do not automatically favorite imported models.
- Do not automatically enable real provider calls if required fields are missing.
- Accept version 1 exports by converting singular `modelId` / `displayName` into `models[0]`.
- Accept version 2 exports by importing all safe `models[]` entries with empty advanced request config.
- Accept version 3 exports by importing all safe `models[]`, `customHeaders[]`, and `customBody` entries after backend validation.

### ID Conflict Handling

Current import behavior generates new local provider IDs and internal model IDs, so exported IDs are not imported. If a future importer ever supports overwrite behavior, it should offer explicit choices:

- `Keep both`: generate a new local ID for the imported provider.
- `Update existing`: overwrite only non-sensitive fields after user confirmation.
- `Skip`: do not import this provider.

The safe default should be `Keep both` or `Skip`, not silent overwrite.

Suggested generated ID format:

```text
provider-openai-compatible-imported-<timestamp>
```

When generating a new provider ID, the app also generates a new local secret reference internally. The import file must not provide that secret reference, and no secret is written until the user manually saves a new key.

## Security UX Recommendations

Export UX should make the security boundary visible:

- Before export, show: "This export does not include API keys or tokens."
- After export, show the saved file path and remind the user that secrets were excluded.
- Use a filename like `rikkadesk-providers-export-YYYYMMDD.json`.
- Do not call the file "full backup" because it excludes secrets.

Import UX should make missing secrets clear:

- Before import, show a preview of provider names, base URLs, model IDs, and whether the source device had a saved secret.
- After import, show: "Provider settings were imported. Please re-enter API keys before using real chat."
- In the provider list, imported providers should display `hasSecret: false` until the user saves a key.
- Test Connection should stay disabled or fail safely until base URL, model ID, and a local secret are present.

Suggested English copy:

- Export prompt: "Export provider settings? API keys and tokens will not be included."
- Export success: "Provider settings exported. API keys were not included."
- Import prompt: "Review providers before importing. API keys must be entered again."
- Import success: "Provider settings imported. Re-enter API keys to use real providers."
- Conflict prompt: "A provider with this ID already exists. Choose how to continue."

Suggested Chinese copy:

- Export prompt: "要导出 Provider 设置吗？导出文件不会包含 API Key 或 Token。"
- Export success: "Provider 设置已导出，API Key 未包含在导出文件中。"
- Import prompt: "请先预览要导入的 Provider。API Key 需要重新填写。"
- Import success: "Provider 设置已导入。请重新填写 API Key 后再使用真实模型。"
- Conflict prompt: "本地已存在相同 ID 的 Provider，请选择如何处理。"

## Current Implementation Summary

Implemented behavior:

1. Export non-sensitive JSON only.
   - `GET /api/desktop/providers/export` emits version 3 documents.
   - Exported providers include `models[]` with `modelId` and `displayName`.
   - Exported providers include safe non-sensitive `customHeaders[]` and safe `customBody` only after backend validation.
   - Exports exclude provider IDs, internal model IDs, API keys, local secret references, Authorization headers, `x-api-key`, tokens, cookies, DPAPI blobs, and secret-store files.

2. Import preview.
   - `POST /api/desktop/providers/import/preview` accepts version 1, version 2, and version 3 documents.
   - Preview validates schema, provider type, Base URL protocol, required model IDs, model count, duplicate model IDs, maximum field lengths, and safe advanced request config.
   - Preview shows advanced config summaries instead of full custom header values or full custom body JSON.
   - Preview does not write local state.

3. Confirmed import.
   - `POST /api/desktop/providers/import/confirm` repeats validation before writing.
   - Confirm generates new local provider IDs, new internal model IDs, and new internal secret references.
   - Imported providers have `hasSecret: false`.
   - Import does not set current model, favorite models, or overwrite existing providers.
   - Version 1 and version 2 imports set `customHeaders = []` and `customBody = null`.
   - Version 3 imports preserve safe validated `customHeaders[]` and `customBody`.

RikkaDesk should not implement one-click secret export. If encrypted backup is ever considered, it must be designed as a separate threat model and should not reuse the normal provider export path.

## Non-Goals

The normal provider import/export path does not include:

- Secret export.
- Provider import overwrite.
- Reusable `secretRef` export.
- Provider-specific import of real API keys.
- Provider connection testing changes.
- Any real API key handling.

## Checklist For Future Code Review

Before approving an import/export implementation, verify:

- Export files contain no API key, token, Authorization header value, `x-api-key` value, SecretStore value, or DPAPI blob.
- Export files do not include reusable `secretRef` values by default.
- Export files do not include provider IDs, internal model IDs, app data paths, or local secret-store file names.
- Version 3 exports include `providers[].models[]` and safe non-sensitive `customHeaders[]` / `customBody`.
- Version 1 imports remain compatible and become a single model on import.
- Version 2 imports remain compatible and import multiple models with empty advanced request config.
- Version 3 imports validate advanced request config and reject sensitive header/body fields.
- Imported providers require the user to manually enter API keys.
- Import does not silently overwrite existing providers.
- Import does not automatically favorite or select imported models.
- Logs and user-facing errors do not include secrets.
- Tests use placeholders only and do not introduce real credentials.
