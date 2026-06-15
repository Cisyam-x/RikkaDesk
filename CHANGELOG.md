# Changelog

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

This changelog tracks the RikkaDesk desktop work in this fork. It does not replace the upstream RikkaHub changelog or release notes.

## 0.1.0 Beta Draft - 2026-06-14

This is the first local/private beta preparation checkpoint. It is not a public GitHub Release.

### Added

- Tauri v2 Windows desktop shell named `RikkaDesk`.
- Development mode loads `http://localhost:5173`.
- Production desktop mode loads the `web-ui` production build.
- Local Rust HTTP API runs inside the Tauri process and binds to `127.0.0.1`.
- API port strategy prefers `127.0.0.1:8080` and falls back to a random loopback port when needed.
- Frontend resolves the desktop API base URL through the Tauri `get_api_base_url` command.
- Minimal P0/P1 `/api/*` compatibility for startup, settings stream, conversations, conversation details, basic message send, stop, and AI icons.
- Mock fallback response path for development and safe offline verification.
- JSON persistence for settings, conversations, messages, provider config, `idSeq`, `savedAt`, and schema metadata.
- State file stored under the Tauri app data directory instead of the source tree.
- Provider configuration schema with non-sensitive OpenAI-compatible provider fields and `secretRef`.
- Desktop secret abstraction for provider API keys.
- Windows encrypted local secret blob storage under app data.
- OpenAI-compatible non-streaming text chat path.
- OpenAI-compatible streaming text chat path using `/chat/completions` and `stream: true`.
- Minimal Provider Settings UI in the desktop app.
- Provider Settings UI can save provider name, base URL, model ID, display name, and API key.
- API key input is cleared after save and is not returned to the frontend.
- Windows MSI and NSIS installer build outputs.
- Documentation for development, provider smoke tests, Provider Settings smoke tests, beta package checks, model config, and secret storage.

### Security

- API keys must not be stored in `state.v1.json`.
- JSON state stores only non-sensitive provider config and `secretRef`.
- Provider APIs return `hasSecret`, not the secret value.
- Logs and user-facing errors must not include API keys or `Authorization` header values.
- Real API keys must be entered only by the local user and must not be sent to Codex, committed, or copied into documentation.

### Known Limits

- Only OpenAI-compatible text chat is supported.
- Gemini, Claude, Anthropic, Vertex, and provider-specific protocols are not implemented.
- Files, attachments, images, audio, tools, MCP, search, forks, and multimodal requests are not implemented.
- Provider Settings is intentionally a minimal single-provider UI.
- Stop/cancel behavior is minimal and may not abort the underlying provider HTTP request immediately.
- JSON state is a beta prototype store, not the final database architecture.
- Windows installers are unsigned.
- This beta is not ready for public large-scale distribution.
- Upstream RikkaHub Android app behavior and Android build flow are intentionally unchanged.

### Package Outputs

Expected Windows build artifacts:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

### Validation

Required validation commands:

```powershell
cd web-ui
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run desktop:build
```

Required security checks:

```powershell
rg --fixed-strings "<key-fragment>" .
rg -a --fixed-strings "<key-fragment>" "$env:APPDATA/com.cisyamx.rikkadesk"
```

No plaintext key fragment should appear in the repository or app data files.
