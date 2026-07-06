# RikkaDesk Provider Settings UI Smoke Test

This guide is for local smoke testing through the RikkaDesk desktop UI. It verifies that OpenAI-compatible providers and their model rows can be configured without manual API calls.

Do not send real API keys to Codex or commit them to the repository. Real keys should be entered only by the local user in the desktop UI.

## Scope

Current Provider Settings covers:

- Provider Settings UI for multiple OpenAI-compatible providers.
- Multiple model rows under one provider.
- Non-sensitive provider configuration saved in local JSON state.
- API key storage through the desktop secret store.
- Per-model Set as current model and Test Connection actions.
- Advanced request config for non-sensitive custom headers and safe custom body JSON.
- Safe provider import/export v3 with v1/v2 import compatibility.
- Real text chat through the existing OpenAI-compatible path.
- Streaming text response verification.

This guide does not cover:

- Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- Files, attachments, images, audio, MCP, tools, search, or forks.
- Release packaging or sync across devices.
- Any change to the Android `app` module.

## Open Provider Settings

1. Start RikkaDesk in desktop mode.
2. Open the sidebar.
3. Click the key-shaped `Provider Settings` button near the sidebar actions.
4. Confirm the dialog title is `Provider Settings`.

The dialog should show a provider list, model rows, and a secret status area with either `hasSecret: true` or `hasSecret: false`.

## Fill Provider Settings

Use an OpenAI-compatible endpoint. Do not paste real keys into documentation, logs, chat, screenshots, or issue comments.

Recommended fields:

| Field | Example | Notes |
| --- | --- | --- |
| Provider Name | `OpenAI Compatible` | Local display name only. |
| Base URL | `https://api.openai.com/v1` | API root URL is preferred. |
| Model ID | `gpt-4o-mini` | Use a model supported by the configured endpoint. Add more model rows for the same provider when needed. |
| Display Name | `gpt-4o-mini` | Optional per model; defaults to Model ID if blank. |
| Custom Headers | `OpenAI-Beta: assistants=v2` | Optional Advanced request config. Use only non-sensitive headers. |
| Custom Body JSON | `{ "temperature": 0.7 }` | Optional Advanced request config. Must be a JSON object and must not contain credentials. |
| API Key | entered locally in the password field | Never echoed after save. |

The backend also accepts a full chat completions endpoint, for example `https://example.test/v1/chat/completions`, but the API root form is easier to read and less error-prone.

## Save Behavior

1. Fill `Base URL` and at least one `Model ID`.
2. Add a second model row if the provider should expose multiple models that share the same Base URL and API key.
3. Enter the API key in the password field if a secret needs to be saved or replaced.
4. Click `Save`.
5. Confirm the success toast appears.
6. Confirm the API key input is cleared after saving.
7. Confirm the dialog shows `hasSecret: true` when a key was saved.

If the API key field is left blank, the UI saves only the non-sensitive provider configuration. Existing saved secrets are kept.

`Clear API Key` deletes the secret from the desktop secret store and changes the visible status to `hasSecret: false`. It must not modify chat history.

## Advanced Request Config

The Advanced request config section is for OpenAI-compatible gateways that require non-sensitive custom request fields. It is collapsed by default so normal users can leave it alone.

Allowed examples:

- `OpenAI-Beta: assistants=v2`
- `x-gateway-route: beta`
- Custom body JSON such as `{ "temperature": 0.7, "top_p": 0.9, "max_tokens": 123 }`

Do not put API keys, tokens, passwords, `Authorization`, `x-api-key`, cookies, or other credentials in custom headers or custom body JSON. API keys belong only in the API Key field and are saved through the desktop SecretStore / DPAPI path.

UI validation is a convenience layer only. The Rust backend is the final safety boundary and must reject sensitive header names, sensitive values, reserved body fields such as `model`, `messages`, or `stream`, non-object custom body JSON, and oversized custom body payloads.

Test Connection and Streaming Chat share the same safe OpenAI-compatible request builder. Test Connection forces `max_tokens=1`; Streaming Chat may preserve allowed custom body fields such as `max_tokens`.

## Confirm Model Availability

After saving a provider:

1. Close the Provider Settings dialog.
2. Open the model selector in the chat UI.
3. Confirm every configured model row is visible, including multiple models from the same provider.
4. Select the configured model if it is not already selected.

The settings stream should now include the configured provider and model. If the model list does not refresh immediately, close and reopen the window once before debugging deeper.

## Send A Test Message

Use a simple text-only prompt, for example:

```text
Reply with one short sentence.
```

Expected result:

- The assistant message starts streaming text into the current conversation.
- The final reply remains in the conversation after streaming completes.
- Restarting RikkaDesk keeps the provider configuration and chat history.
- The API key field remains blank after restart.
- `hasSecret: true` still appears after restart if the secret was saved successfully.

## Confirm Real Provider Instead Of Mock Fallback

The mock fallback reply is a fixed RikkaDesk test response. A real provider response should differ from that fixed message and should follow the selected model behavior.

Check these signals:

- The selected model is the provider model configured in the dialog.
- `hasSecret: true` is visible in Provider Settings.
- `Base URL` and `Model ID` are not empty.
- The response streams progressively instead of appearing only as the old mock text.
- An invalid model or invalid key produces a safe provider error such as `401 Unauthorized`, `429 Too Many Requests`, or another sanitized HTTP error.

Safe error messages must not include the API key, `Authorization` header, `x-api-key`, full request headers, or provider secrets.

## Local State Checks

RikkaDesk stores non-sensitive mock API state in the Tauri app data directory, not in the source tree. On Windows, check:

```powershell
$StatePath = Join-Path $env:APPDATA "com.cisyamx.rikkadesk\mock-api\state.v1.json"
Get-Content -LiteralPath $StatePath
```

The JSON state may include:

- provider id
- provider name
- provider type
- enabled flag
- baseUrl
- `models[]` with model id and displayName
- assistant chatModelId
- secretRef

The JSON state must not include:

- API key
- access token
- refresh token
- `Authorization` header
- `x-api-key` value
- service account private key
- any other sensitive credential

## Plaintext Key Search

After testing with a real key, search only for a short fragment of the key. Do not paste the full key into terminal history.

From the repository root:

```powershell
rg --fixed-strings "<real-key-fragment>" .
```

From the app data directory:

```powershell
rg -a --fixed-strings "<real-key-fragment>" "$env:APPDATA/com.cisyamx.rikkadesk"
```

Correct result:

- No match in the source repository.
- No plaintext match in `state.v1.json`.
- No plaintext match in local logs or app data files.

The secret may exist inside the operating system credential store or an encrypted credential backend. It should not appear as readable plaintext in the JSON state file.

## Provider API Sanity Check

The UI uses these local desktop API endpoints:

- `GET /api/desktop/providers`
- `POST /api/desktop/providers`
- `DELETE /api/desktop/providers/{id}`
- `POST /api/desktop/providers/{id}/test`
- `POST /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}/secret`
- `GET /api/desktop/providers/export`
- `POST /api/desktop/providers/import/preview`
- `POST /api/desktop/providers/import/confirm`

`GET /api/desktop/providers` should return `hasSecret`, but it must never return the secret value.

Expected response shape:

```json
[
  {
    "id": "rikkadesk-openai-compatible",
    "type": "openai-compatible",
    "enabled": true,
    "name": "OpenAI Compatible",
    "baseUrl": "https://api.openai.com/v1",
    "model": {
      "id": "rikkadesk-openai-compatible:gpt-4o-mini",
      "modelId": "gpt-4o-mini",
      "displayName": "gpt-4o-mini"
    },
    "models": [
      {
        "id": "rikkadesk-openai-compatible:gpt-4o-mini",
        "modelId": "gpt-4o-mini",
        "displayName": "gpt-4o-mini"
      },
      {
        "id": "rikkadesk-openai-compatible:gpt-4o-mini-fast",
        "modelId": "gpt-4o-mini-fast",
        "displayName": "gpt-4o-mini Fast"
      }
    ],
    "customHeaders": [
      {
        "name": "OpenAI-Beta",
        "value": "assistants=v2"
      }
    ],
    "customBody": {
      "temperature": 0.7
    },
    "secretRef": "rikkadesk.provider.rikkadesk-openai-compatible.api-key",
    "hasSecret": true
  }
]
```

The `model` field remains for older UI compatibility and mirrors the first entry in `models[]`.

## Smoke Test Checklist

- Provider Settings opens from the sidebar.
- `Base URL` and `Model ID` validation prevents empty required fields.
- Adding and deleting model rows works while keeping at least one model.
- Saving provider config works without an API key.
- Saving with an API key changes status to `hasSecret: true`.
- API key input is cleared after save.
- API key is not displayed after reopening the dialog.
- `Clear API Key` changes status to `hasSecret: false`.
- Model selector shows the configured model.
- Model selector shows multiple models from the same provider.
- Set as current model works from a specific model row.
- Test Connection works from a specific model row.
- Advanced request config saves safe custom headers and safe custom body JSON.
- Advanced request config can be cleared and remains cleared after reopening.
- Sensitive custom headers/body are rejected by UI/backend validation.
- Provider export produces version 3 JSON with `providers[].models[]`, safe `customHeaders[]`, and safe `customBody`.
- Provider import preview shows every model row.
- Provider import preview shows advanced config summary without header values or full custom body JSON.
- Provider import confirm creates imported providers with `hasSecret: false`.
- Version 1 provider exports can still be imported as a single model.
- Version 2 provider exports can still be imported as multi-model providers with empty advanced config.
- A text-only message can be sent with the configured model selected.
- Real provider response streams into the assistant message.
- Restart keeps provider config, chat history, and `hasSecret` state.
- Repository search finds no plaintext key fragment.
- App data search finds no plaintext key fragment.
- `state.v1.json` contains only `secretRef`, not secret values.
- Provider exports contain no API keys, `secretRef`, tokens, Authorization headers, `x-api-key`, cookies, DPAPI blobs, local secret-store files, provider IDs, or internal model IDs.
- Error messages do not include secret values or sensitive headers.

## Troubleshooting Notes

- If the configured `Base URL` already ends with `/chat/completions`, RikkaDesk should use it directly.
- If the configured `Base URL` is an API root such as `/v1`, RikkaDesk appends `/chat/completions`.
- `401` or `403` usually means the secret is missing, invalid, revoked, or not authorized for the model.
- `429` usually means rate limit, quota, or provider throttling.
- Empty `choices` or malformed streaming chunks should produce a safe error message and keep the conversation usable.
- Non-text messages are outside Phase 4B scope and may fall back to a clear unsupported-message response.
