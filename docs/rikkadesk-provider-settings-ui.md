# RikkaDesk Provider Settings UI Smoke Test

This guide is for Phase 4B local smoke testing through the RikkaDesk desktop UI. It verifies that an OpenAI-compatible provider can be configured without manual API calls.

Do not send real API keys to Codex or commit them to the repository. Real keys should be entered only by the local user in the desktop UI.

## Scope

Phase 4B covers:

- Provider Settings UI for one OpenAI-compatible provider.
- Non-sensitive provider configuration saved in local JSON state.
- API key storage through the desktop secret store.
- Real text chat through the existing OpenAI-compatible path.
- Streaming text response verification from Phase 3E.

Phase 4B does not cover:

- Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- Files, attachments, images, audio, MCP, tools, search, or forks.
- Full provider management, release packaging, or sync across devices.
- Any change to the Android `app` module.

## Open Provider Settings

1. Start RikkaDesk in desktop mode.
2. Open the sidebar.
3. Click the key-shaped `Provider Settings` button near the sidebar actions.
4. Confirm the dialog title is `Provider Settings`.

The dialog should show a `Secret status` area with either `hasSecret: true` or `hasSecret: false`.

## Fill Provider Settings

Use an OpenAI-compatible endpoint. Do not paste real keys into documentation, logs, chat, screenshots, or issue comments.

Recommended fields:

| Field | Example | Notes |
| --- | --- | --- |
| Provider Name | `OpenAI Compatible` | Local display name only. |
| Base URL | `https://api.openai.com/v1` | API root URL is preferred. |
| Model ID | `gpt-4o-mini` | Use a model supported by the configured endpoint. |
| Display Name | `gpt-4o-mini` | Optional; defaults to Model ID if blank. |
| API Key | entered locally in the password field | Never echoed after save. |

The backend also accepts a full chat completions endpoint, for example `https://example.test/v1/chat/completions`, but the API root form is easier to read and less error-prone.

## Save Behavior

1. Fill `Base URL` and `Model ID`.
2. Enter the API key in the password field if a secret needs to be saved or replaced.
3. Click `Save`.
4. Confirm the success toast appears.
5. Confirm the API key input is cleared after saving.
6. Confirm the dialog shows `hasSecret: true` when a key was saved.

If the API key field is left blank, the UI saves only the non-sensitive provider configuration. Existing saved secrets are kept.

`Clear API Key` deletes the secret from the desktop secret store and changes the visible status to `hasSecret: false`. It must not modify chat history.

## Confirm Model Availability

After saving a provider:

1. Close the Provider Settings dialog.
2. Open the model selector in the chat UI.
3. Confirm the configured model is visible.
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
- model id
- displayName
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
- `POST /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}/secret`

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
    "secretRef": "rikkadesk.provider.rikkadesk-openai-compatible.api-key",
    "hasSecret": true
  }
]
```

## Smoke Test Checklist

- Provider Settings opens from the sidebar.
- `Base URL` and `Model ID` validation prevents empty required fields.
- Saving provider config works without an API key.
- Saving with an API key changes status to `hasSecret: true`.
- API key input is cleared after save.
- API key is not displayed after reopening the dialog.
- `Clear API Key` changes status to `hasSecret: false`.
- Model selector shows the configured model.
- A text-only message can be sent with the configured model selected.
- Real provider response streams into the assistant message.
- Restart keeps provider config, chat history, and `hasSecret` state.
- Repository search finds no plaintext key fragment.
- App data search finds no plaintext key fragment.
- `state.v1.json` contains only `secretRef`, not secret values.
- Error messages do not include secret values or sensitive headers.

## Troubleshooting Notes

- If the configured `Base URL` already ends with `/chat/completions`, RikkaDesk should use it directly.
- If the configured `Base URL` is an API root such as `/v1`, RikkaDesk appends `/chat/completions`.
- `401` or `403` usually means the secret is missing, invalid, revoked, or not authorized for the model.
- `429` usually means rate limit, quota, or provider throttling.
- Empty `choices` or malformed streaming chunks should produce a safe error message and keep the conversation usable.
- Non-text messages are outside Phase 4B scope and may fall back to a clear unsupported-message response.
