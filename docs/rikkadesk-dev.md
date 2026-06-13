# RikkaDesk Development Notes

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. These notes describe the current mock desktop prototype and how to reproduce it locally.

## Phase Status

- Phase 0 complete: upstream architecture, Web Interface, `web-ui`, and license were reviewed without code changes.
- Phase 1 complete: `web-ui` runs in a browser at `http://localhost:5173/`.
- Phase 2A complete: Tauri v2 wraps `web-ui` as a Windows desktop shell named RikkaDesk.
- Phase 2B complete: startup and chat-related `/api/*` endpoints were inventoried.
- Phase 2C complete: a Tauri Rust in-memory Mock API handles P0/P1 startup and basic chat endpoints.
- Phase 2D current: document and stabilize the mock prototype without adding real model, storage, or P2/P3 features.

## Current Architecture

The current desktop prototype has three parts:

- `web-ui`: the existing React Router frontend.
- `web-ui/src-tauri`: the Tauri v2 desktop shell.
- `web-ui/src-tauri/src/mock_api.rs`: an in-process Rust Mock API implemented with axum and tokio.

In development mode, Vite serves the frontend on `http://localhost:5173/` and keeps the existing `/api` proxy to `http://localhost:8080`.

In desktop mode, Tauri starts the Mock API before the window is shown. The frontend asks Tauri for the API base URL through the command `get_api_base_url`, then sends API requests to that local address.

## Mock API Binding

The Mock API tries to listen on:

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

That command returns the actual Mock API origin, for example:

```text
http://127.0.0.1:8080
```

`AIIcon` uses the same resolver so icon requests work in both browser development and packaged desktop mode.

## Implemented Mock Endpoints

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

The mock conversation state lives only in memory. Sending a message appends a user message and an immediate mock assistant reply. SSE streams send `update`, `invalidate`, and `snapshot` events matching the frontend listeners.

## Not Implemented

The current prototype intentionally does not implement:

- Real model provider calls.
- API key, token, or password storage.
- SQLite or other persistence.
- File upload, attachment serving, or file deletion.
- Search and search index APIs.
- MCP servers and MCP tools.
- Tool approval execution.
- Conversation fork, branch selection, message edit/delete/regenerate, and title generation.
- Favorite models and other deeper settings mutations beyond basic assistant/model selection.

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
- The Mock API logs its loopback address.
- The sidebar shows the mock welcome conversation.
- Sending a message produces the mock assistant reply.
- `pnpm run desktop:build` produces MSI and NSIS installers.

## Next Phase Suggestions

Recommended next steps:

- Keep the Mock API as a protocol safety net while designing the real local backend.
- Decide whether the real backend should remain Rust/Tauri-native or move to a sidecar service.
- Add a small local configuration design before any API key support.
- Add persistence only after the DTO and storage boundaries are settled.
- Keep P2/P3 features deferred until basic conversation lifecycle behavior is stable.

