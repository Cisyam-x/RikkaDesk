# RikkaDesk Provider Import/Export Safety Design

Review date: 2026-07-04

This document defines the safety boundary for future RikkaDesk provider configuration import/export. It is a design note only. Phase 6B P3 does not implement provider import/export, does not change Provider Settings UI, does not change the Mock API, and does not upgrade the local state schema.

## Goals

- Allow users to back up and transfer non-sensitive provider configuration.
- Prevent API keys, tokens, or local credential blobs from entering exported files.
- Make imported providers explicit and reviewable before they affect local settings.
- Keep the current `state.v1.json` / `schemaVersion: 2` model unchanged until a later implementation phase needs code changes.

## Exportable Provider Fields

A safe provider export may include only non-sensitive configuration:

- `id`: local provider identifier.
- `type`: provider type, currently `openai-compatible`.
- `name`: user-facing provider name.
- `baseUrl`: provider base URL.
- `modelId`: provider model identifier.
- `displayName`: user-facing model display name.
- `enabled`: enabled state, if present in the current provider model.
- `hasSecret`: boolean state such as `true` or `false`, used only to tell the user whether the original provider had a saved secret.

Example safe export shape:

```json
{
  "schema": "rikkadesk.provider-export.v1",
  "exportedAt": "2026-07-04T00:00:00Z",
  "providers": [
    {
      "id": "provider-openai-compatible-example",
      "type": "openai-compatible",
      "name": "Example Provider",
      "baseUrl": "https://example.invalid/v1",
      "modelId": "example-model",
      "displayName": "Example Model",
      "enabled": true,
      "hasSecret": true
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
- SecretStore raw values.
- Windows DPAPI blobs.
- macOS Keychain items.
- Linux Secret Service values.
- Any local encrypted credential blob or raw secret file.
- Request headers that could contain credentials.
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
- Restore provider name, type, base URL, model ID, display name, and enabled state.
- Never restore an API key or SecretStore entry from the import file.
- After import, show `hasSecret: false` for imported providers until the user manually enters a new API key.
- Do not automatically overwrite an existing provider.
- Do not automatically set imported providers as the current model.
- Do not automatically favorite imported models.
- Do not automatically enable real provider calls if required fields are missing.

### ID Conflict Handling

If an imported provider `id` already exists locally, the importer should offer explicit choices:

- `Keep both`: generate a new local ID for the imported provider.
- `Update existing`: overwrite only non-sensitive fields after user confirmation.
- `Skip`: do not import this provider.

The safe default should be `Keep both` or `Skip`, not silent overwrite.

Suggested generated ID format:

```text
provider-openai-compatible-imported-<timestamp>
```

When generating a new provider ID, the app should also generate a new local secret reference internally after the user saves a new key. The import file should not provide that secret reference.

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

## Future Implementation Order

Recommended implementation sequence:

1. Export non-sensitive JSON only.
   - Read current provider list.
   - Strip all secrets and local secret references.
   - Write only the safe export fields listed in this document.

2. Import preview.
   - Parse the JSON file.
   - Validate schema and provider type.
   - Show providers before changing local state.
   - Show conflict choices for duplicate IDs.

3. Confirmed import.
   - Write only non-sensitive provider config.
   - Generate new local secret references internally.
   - Mark imported providers as missing secrets.
   - Ask users to enter API keys manually in Provider Settings.

4. Optional export/import polish.
   - Add localized UI copy.
   - Add friendly validation errors.
   - Add import summary and skipped-provider report.

RikkaDesk should not implement one-click secret export. If encrypted backup is ever considered, it must be designed as a separate threat model and should not reuse the normal provider export path.

## Non-Goals

Phase 6B P3 does not include:

- Provider import/export code.
- Provider Settings UI changes.
- Mock API changes.
- SecretStore changes.
- Schema migration.
- Multi-model provider support.
- Provider connection testing changes.
- Any real API key handling.

## Checklist For Future Code Review

Before approving an import/export implementation, verify:

- Export files contain no API key, token, Authorization header value, `x-api-key` value, SecretStore value, or DPAPI blob.
- Export files do not include reusable `secretRef` values by default.
- Imported providers require the user to manually enter API keys.
- Import does not silently overwrite existing providers.
- Import does not automatically favorite or select imported models.
- Logs and user-facing errors do not include secrets.
- Tests use placeholders only and do not introduce real credentials.
