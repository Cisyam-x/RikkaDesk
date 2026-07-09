# RikkaDesk Development Notes

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. These notes describe the current local desktop prototype and how to reproduce it locally.

## Phase Status

- Phase 0 complete: upstream architecture, Web Interface, `web-ui`, and license were reviewed without code changes.
- Phase 1 complete: `web-ui` runs in a browser at `http://localhost:5173/`.
- Phase 2A complete: Tauri v2 wraps `web-ui` as a Windows desktop shell named RikkaDesk.
- Phase 2B complete: startup and chat-related `/api/*` endpoints were inventoried.
- Phase 2C complete: a Tauri Rust in-memory Mock API handles P0/P1 startup and basic chat endpoints.
- Phase 2D complete: the mock prototype was documented and stabilized.
- Phase 3A complete: JSON persistence keeps settings, conversations, messages, and idSeq across restarts.
- Phase 3B complete: model config and secret storage design was documented.
- Phase 3C complete: provider config and `secretRef` persistence were added without storing API keys in JSON.
- Phase 3D complete: non-streaming OpenAI-compatible text chat was added.
- Phase 3E complete: streaming OpenAI-compatible text chat was added.
- Phase 3F complete: real provider smoke-test guidance was documented.
- Phase 4A complete: minimal Provider Settings UI was added.
- Phase 4B complete: Provider Settings UI smoke-test guidance was documented.
- Phase 5A complete: prepared the first local beta package checklist without publishing a public release.
- Phase 5B complete: prepared changelog, release draft, version strategy, and merge guidance without publishing a release.
- Phase 6A complete: conversation and message management were added and unsupported visible UX was polished.
- Phase 6B P1/P2 complete: Provider Settings basic management, favorite models, Set as current model, and Test Connection were added.
- Phase 6B P3 complete: Provider import/export safety design was documented.
- Phase 7 complete: safe Provider import/export was implemented for non-sensitive provider metadata.
- Phase 8 complete: Provider state, APIs, and Provider Settings support multiple models per provider.
- Phase 9B complete: Provider Settings supports advanced non-sensitive custom headers/body, shared OpenAI-compatible request building, and provider import/export v3.
- Phase 9C current: Markdown rendering has table/code overflow polish, mhchem chemistry support, message Markdown raw HTML hardening, and Workbench preview sandbox hardening.

## Current Architecture

The current desktop prototype has three parts:

- `web-ui`: the existing React Router frontend.
- `web-ui/src-tauri`: the Tauri v2 desktop shell.
- `web-ui/src-tauri/src/mock_api.rs`: an in-process Rust local API implemented with axum and tokio.

In development mode, Vite serves the frontend on `http://localhost:5173/` and keeps the existing `/api` proxy to `http://localhost:8080`.

In desktop mode, Tauri starts the local API before the window is shown. The frontend asks Tauri for the API base URL through the command `get_api_base_url`, then sends API requests to that local address.

## Mock API Binding

The local API tries to listen on:

```text
127.0.0.1:8080
```

If that port is already in use, it falls back to a random local loopback port by binding `127.0.0.1:0`.

The service binds only to `127.0.0.1`, so it is not exposed to the local network. CORS exists so the Tauri WebView and local browser development server can call the loopback API.

## API Base URL Resolution

Frontend API calls are centralized in:

```text
web-ui/app/services/api.ts
```

Browser-only development keeps using the relative prefix:

```text
/api
```

Tauri desktop mode uses:

```text
get_api_base_url
```

That command returns the actual local API origin, for example:

```text
http://127.0.0.1:8080
```

`AIIcon` uses the same resolver so icon requests work in both browser development and packaged desktop mode.

## Implemented Endpoints

P0 startup endpoints:

- `GET /api/settings/stream`
- `GET /api/conversations/paged`
- `GET /api/conversations/stream`
- `GET /api/ai-icon?name=`

P1 basic chat endpoints:

- `GET /api/conversations/{id}`
- `GET /api/conversations/{id}/stream`
- `POST /api/conversations/{id}/messages`
- `POST /api/conversations/{id}/stop`
- `POST /api/settings/assistant`
- `POST /api/settings/assistant/model`

Desktop provider endpoints:

- `GET /api/desktop/providers`
- `POST /api/desktop/providers`
- `DELETE /api/desktop/providers/{id}`
- `POST /api/desktop/providers/{id}/test`
- `POST /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}/secret`
- `GET /api/desktop/providers/export`
- `POST /api/desktop/providers/import/preview`
- `POST /api/desktop/providers/import/confirm`

Additional settings/conversation endpoints now used by the desktop beta:

- `POST /api/settings/favorite-models`
- `POST /api/conversations/{id}/title`
- `POST /api/conversations/{id}/pin`
- `DELETE /api/conversations/{id}`
- `POST /api/conversations/{id}/messages/{messageId}/edit`
- `DELETE /api/conversations/{id}/messages/{messageId}`
- `POST /api/conversations/{id}/regenerate`

The local state is persisted under the Tauri app data directory. Sending a message appends a user message, then either streams an OpenAI-compatible text response or falls back to a safe mock response. SSE streams send `update`, `invalidate`, and `snapshot` events matching the frontend listeners.

## Persistence And Secrets

On Windows, local beta data lives under:

```text
%APPDATA%\com.cisyamx.rikkadesk\mock-api
```

Important files:

- `state.v1.json`: non-sensitive settings, conversations, messages, provider config, `secretRef`, and schema metadata. Phase 9B uses `schemaVersion: 4` with provider models stored in `providers[].models[]`, non-sensitive custom headers in `providers[].customHeaders`, and safe custom body JSON in `providers[].customBody`.
- `secrets/*.bin`: encrypted local secret blobs used by the desktop secret mechanism on Windows.

`state.v1.json` must not contain API keys, access tokens, refresh tokens, credential-bearing `Authorization` header values, `x-api-key` values, cookies, passwords, or service account private keys. Custom request config is intentionally limited to non-sensitive headers and safe JSON object fields.

Provider import/export currently uses export document version 3 for multi-model provider metadata plus safe advanced request config. Version 3 exports `providers[].models[]`, safe `customHeaders[]`, and safe `customBody`; it still excludes API keys, `secretRef`, internal IDs, credential headers, tokens, cookies, DPAPI blobs, and local secret-store files. Version 1 single-model and version 2 multi-model provider exports are still accepted by the import preview and confirm endpoints.

Test Connection and Streaming Chat now use the same OpenAI-compatible request builder. Test Connection always forces `max_tokens=1`; Streaming Chat can preserve allowed custom body fields such as `max_tokens` while RikkaDesk continues to control `model`, `messages`, and `stream`.

## Markdown Rendering And Workbench Preview

Message Markdown now keeps the existing Streamdown, GFM, math, KaTeX, and CodeBlock path, but RikkaDesk no longer explicitly enables `rehypeRaw` in the message Markdown plugin list.

Current rendering notes:

- Wide GFM tables scroll inside message content instead of expanding the app viewport.
- KaTeX mhchem is enabled through `katex/dist/contrib/mhchem.mjs`.
- Unsafe Markdown link schemes such as `javascript:`, `data:`, `file:`, and `blob:` are blocked by default.
- Normal `http:`, `https:`, and `mailto:` links keep `target="_blank"` and `rel="noopener noreferrer"`.
- Unsafe Markdown image sources are blocked by default.
- Mermaid in normal message Markdown remains disabled/deferred.
- Workbench Mermaid preview uses `securityLevel: "strict"`.
- Workbench Mermaid preview runs in an iframe sandbox without `allow-same-origin`.
- Workbench Mermaid still uses a remote CDN and remains a residual risk to revisit before any public release.

## Not Implemented

The current prototype intentionally does not implement:

- SQLite or other persistence.
- File upload, attachment serving, or file deletion.
- Search and search index APIs.
- MCP servers and MCP tools.
- Tool approval execution.
- Workspace behavior.
- Conversation fork, branch selection, and AI title generation.
- Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- Multimodal provider requests, tool calls, or upstream feature parity.

Unimplemented routes return a JSON 404 response from the Mock API fallback.

## Commands

Install frontend dependencies:

```powershell
cd web-ui
pnpm install
```

Run browser-only development:

```powershell
cd web-ui
pnpm run dev
```

Run desktop development:

```powershell
cd web-ui
pnpm run desktop:dev
```

Run type and Rust checks:

```powershell
cd web-ui
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
```

Build Windows installers:

```powershell
cd web-ui
pnpm run desktop:build
```

Build outputs:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

## Validation Checklist

- `pnpm run typecheck` passes.
- `cargo check --manifest-path src-tauri/Cargo.toml` passes.
- `pnpm run desktop:dev` opens a RikkaDesk window.
- The local API logs its loopback address.
- The sidebar shows persisted conversations.
- Provider Settings opens from the sidebar.
- Saving a provider shows `hasSecret` without showing the API key.
- Provider Settings can add, edit, delete, and test OpenAI-compatible providers.
- Provider Settings can manage multiple models under one provider.
- Set as current model updates the model selector for a specific provider model.
- Favorite model updates work from the model selector.
- Provider import/export v3 round-trips multiple models and safe advanced request config without exporting secrets, and v1/v2 imports remain compatible.
- Wide Markdown tables scroll inside message content.
- Inline math, block math, and mhchem chemistry formulas render.
- Unsafe message Markdown link schemes and image sources are blocked.
- Workbench preview iframe sandbox settings match the Markdown rendering plan.
- Sending a text message produces either a streaming OpenAI-compatible response or the mock fallback.
- `pnpm run desktop:build` produces MSI and NSIS installers.

## Next Phase Suggestions

Recommended next steps:

- Keep the mock fallback as a protocol safety net while hardening the real provider path.
- Verify unsigned Windows installer behavior with local beta testers.
- Treat `rikkadesk-v0.1.0-beta.13` as the current private hotfix tag after the Phase 10 attachment rendering hotfix.
- Decide whether a future public prerelease should use `0.1.0` or a beta label in release notes/artifact naming.
- Keep P2/P3 features deferred until provider settings, persistence, and streaming behavior are stable.
