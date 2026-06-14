# RikkaDesk Real Provider Smoke Test Checklist

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This document describes the Phase 3F smoke test for an OpenAI-compatible provider. It does not add new model protocols, file support, tools, MCP, search, or release packaging.

Do not paste a real API key into Codex, source files, README files, tests, logs, JSON state files, screenshots, or commit messages. Enter a real key only in your own local RikkaDesk runtime when you perform the manual smoke test.

## Scope

Phase 3F verifies the existing OpenAI-compatible path:

- Provider config is saved through `/api/desktop/providers`.
- The secret is saved through `/api/desktop/providers/{id}/secret`.
- The JSON state file stores only non-sensitive config plus `secretRef`.
- `/api/desktop/providers` returns `hasSecret`, never the secret value.
- Chat requests use OpenAI-compatible `/chat/completions` with `stream: true`.
- The UI receives streaming updates through existing conversation SSE `snapshot` events.

Out of scope:

- Gemini, Claude, Anthropic, Vertex, or other provider-specific protocols.
- Images, audio, files, attachments, tool calls, MCP, search, and forks.
- API key management UI.
- Release creation.

## Start RikkaDesk

From the repository root:

```powershell
cd web-ui
pnpm run desktop:dev
```

The mock API normally listens on `http://127.0.0.1:8080`. If port `8080` is already in use, RikkaDesk falls back to a random local port and the frontend discovers it through the Tauri command `get_api_base_url`.

For API-only smoke tests, use the printed mock API base URL from the desktop process. If it is still on the preferred port:

```powershell
$ApiBase = "http://127.0.0.1:8080"
```

## Configure An OpenAI-Compatible Provider

Use a provider that accepts OpenAI-compatible chat completions.

Example base URLs:

```text
https://api.openai.com/v1
http://127.0.0.1:11434/v1
http://127.0.0.1:8000/v1
```

Example model ids:

```text
gpt-4o-mini
gpt-4.1-mini
local-model-name
```

Save non-sensitive provider config:

```powershell
$ApiBase = "http://127.0.0.1:8080"
$ProviderId = "real-provider-smoke-test"

$ProviderBody = @{
  id = $ProviderId
  type = "openai-compatible"
  enabled = $true
  name = "Real Provider Smoke Test"
  baseUrl = "https://api.openai.com/v1"
  modelId = "gpt-4o-mini"
  displayName = "gpt-4o-mini"
} | ConvertTo-Json

Invoke-RestMethod `
  -Method Post `
  -Uri "$ApiBase/api/desktop/providers" `
  -ContentType "application/json" `
  -Body $ProviderBody
```

Expected result:

- Response includes `id`, `type`, `baseUrl`, `model`, `secretRef`, and `hasSecret`.
- Response does not include `apiKey`, `accessToken`, `Authorization`, or `x-api-key`.
- `hasSecret` may be `false` until the secret is saved.

## Save The Secret

Do not put the real key directly in the command line. That can leak it through shell history, screenshots, logs, or copy buffers. Prompt locally instead:

```powershell
$ApiBase = "http://127.0.0.1:8080"
$ProviderId = "real-provider-smoke-test"

$SecureKey = Read-Host "Paste API key for local smoke test" -AsSecureString
$Bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($SecureKey)

try {
  $ApiKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($Bstr)
  $SecretBody = @{ apiKey = $ApiKey } | ConvertTo-Json

  Invoke-RestMethod `
    -Method Post `
    -Uri "$ApiBase/api/desktop/providers/$ProviderId/secret" `
    -ContentType "application/json" `
    -Body $SecretBody
}
finally {
  if ($Bstr -ne [IntPtr]::Zero) {
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($Bstr)
  }
  Remove-Variable ApiKey -ErrorAction SilentlyContinue
  Remove-Variable SecureKey -ErrorAction SilentlyContinue
  Remove-Variable SecretBody -ErrorAction SilentlyContinue
}
```

Expected result:

```json
{
  "status": "ok",
  "hasSecret": true
}
```

## Confirm Provider Status

```powershell
Invoke-RestMethod -Method Get -Uri "$ApiBase/api/desktop/providers"
```

Expected result:

- The provider appears in the list.
- `hasSecret` is `true`.
- `secretRef` is present.
- The real secret value is not present.

## Send A Test Message

Use the desktop UI:

1. Open the mock or existing conversation.
2. Select the configured provider/model if it is not already selected.
3. Send a short text-only message.
4. Watch the assistant response update progressively.

Phase 3E supports text-only streaming. Non-text parts, files, images, tools, and multimodal inputs are not part of this smoke test.

## Confirm It Used The Real Provider

These checks help distinguish real provider behavior from mock fallback:

- The assistant response is not the fixed mock text: `这是 RikkaDesk Mock 后端返回的测试回复。`
- The response arrives progressively during generation.
- If testing against a local stub, confirm the stub received `stream: true`.
- If testing against a hosted provider, confirm usage in that provider's own dashboard, if available.
- If the provider is deliberately configured with a bad model id or invalid key, the conversation should receive a safe error message such as `Real provider request failed: 401 Unauthorized`, without any key or header values.

## Confirm Streaming

Expected streaming behavior:

- The backend sends an OpenAI-compatible request body with `stream: true`.
- The backend parses lines shaped like `data: {"choices":[{"delta":{"content":"..."}}]}`.
- The backend stops on `data: [DONE]`.
- The frontend is updated through existing conversation SSE `snapshot` events.
- The final assistant message is persisted after completion.

## Inspect Local State

RikkaDesk stores mock API state under the Tauri app data directory:

```powershell
$StateFile = Join-Path $env:APPDATA "com.cisyamx.rikkadesk/mock-api/state.v1.json"
Get-Content $StateFile
```

Expected state:

- `schemaVersion` remains the current persisted schema version.
- Provider config contains `id`, `type`, `enabled`, `name`, `baseUrl`, `model`, and `secretRef`.
- Conversation messages are persisted.
- The real API key is not present.
- `Authorization` and `x-api-key` header values are not present.

## Safety Checks

Pick a short fragment from your real key locally. Do not share it with Codex. Use it only in your terminal:

```powershell
$KeyFragment = Read-Host "Enter a short local-only key fragment for scanning"
rg --fixed-strings $KeyFragment .
rg -a --fixed-strings $KeyFragment "$env:APPDATA/com.cisyamx.rikkadesk"
```

Correct result:

- No matches in the source repository.
- No matches in JSON state files.
- No matches in readable app data files.
- If a platform credential backend stores encrypted/protected data, it must not expose the key fragment as plaintext.

Check provider response shape:

```powershell
Invoke-RestMethod -Method Get -Uri "$ApiBase/api/desktop/providers" | ConvertTo-Json -Depth 10
```

Correct result:

- Provider entries include `hasSecret`.
- Provider entries include `secretRef`.
- Provider entries do not include `apiKey`, `accessToken`, `refreshToken`, `Authorization`, or `x-api-key`.

Search logs or captured terminal output if you saved any:

```powershell
rg --fixed-strings "Authorization:" .
rg --fixed-strings "Bearer " .
```

Correct result:

- Source files may contain safe code references such as `.bearer_auth(...)`.
- Runtime logs must not contain the complete `Authorization` header or a real bearer value.

## Common Compatibility Notes

- Prefer a base URL that ends at the API root, for example `https://api.openai.com/v1`.
- RikkaDesk also accepts a full endpoint that already ends in `/chat/completions`; it will not append the path twice.
- A trailing slash in `baseUrl` is safe.
- Empty SSE lines and comment lines are ignored.
- If a provider returns HTTP `401`, `403`, `429`, or `5xx`, RikkaDesk records a safe error message in the conversation.
- If the response is not OpenAI-compatible streaming JSON, RikkaDesk records a safe parse error message.
- Stop currently uses a lightweight cancel flag. It prevents further text from being appended once observed, but it does not forcibly abort the underlying HTTP request immediately.

## Build Verification

Run these before considering the smoke test branch ready:

```powershell
cd web-ui
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run desktop:build
```

The build should not require any real API key.
