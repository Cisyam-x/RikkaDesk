# Changelog

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

This changelog tracks the RikkaDesk desktop work in this fork. It does not replace the upstream RikkaHub changelog or release notes.

## 0.1.0 Private Beta Line

The current feature-stable private beta tag is planned as `rikkadesk-v0.1.0-beta.8`. The `beta/0.1.0` branch may contain later documentation or feature work after that tag.

This beta line is not a public GitHub Release.

### `rikkadesk-v0.1.0-beta.8` - Release Copy Hotfix

Changed:

- Fix About / README release copy to show the current feature-stable tag.
- No provider, schema, secret, or streaming behavior changes.

### `rikkadesk-v0.1.0-beta.7` - Long Stream Timeout Hotfix

Fixed:

- Fix long OpenAI-compatible streaming responses timing out after about 30 seconds.
- Run long chat streams in the background after `POST /api/conversations/{id}/messages` or `POST /api/conversations/{id}/regenerate` returns accepted.
- Remove global Rust reqwest total timeout for streaming requests.
- Keep Test Connection timeout separate.

### `rikkadesk-v0.1.0-beta.6` - Phase 7

Added:

- Safe Provider import/export in Provider Settings.
- Export excludes API keys, `secretRef`, tokens, and local secret-store blobs.
- Import restores provider metadata only; API keys must be re-entered.

### `rikkadesk-v0.1.0-beta.4` - Phase 6B P2

Added:

- Provider Settings action to set a provider model as the current chat model.
- Reuse of `POST /api/settings/assistant/model` for current model updates.
- Provider Test Connection endpoint: `POST /api/desktop/providers/{id}/test`.
- Safe OpenAI-compatible non-streaming `/chat/completions` probe for Test Connection.
- Redacted Test Connection result handling that does not return API keys, Authorization headers, or full request bodies.
- English and Chinese i18n copy for current model and Test Connection flows.

Validation notes:

- `pnpm run typecheck` passed before tagging.
- `pnpm run desktop:build` passed before tagging.
- `cargo check --manifest-path src-tauri/Cargo.toml` was blocked on the local test machine by Windows Application Control for the debug build script; release build still passed.

### `rikkadesk-v0.1.0-beta.3` - Phase 6B P1

Added:

- Provider Settings provider list.
- Add, select, edit, and delete provider flows.
- Provider deletion API: `DELETE /api/desktop/providers/{id}`.
- Secret deletion during provider deletion to avoid orphaned local credentials.
- Favorite model API: `POST /api/settings/favorite-models`.
- Favorite model updates in the model selector, including the currently selected model.
- Provider Settings spacing and bilingual UI copy polish.

Changed:

- Saving or adding a provider no longer favorites its model automatically.
- Deleting a provider removes related favorite model entries.
- Provider Settings is no longer a minimal single-provider form; it supports basic multi-provider management while still keeping one model per provider.

### `rikkadesk-v0.1.0-beta.2` - Phase 6A

Added:

- Conversation title update: `POST /api/conversations/{id}/title`.
- Conversation pin/unpin: `POST /api/conversations/{id}/pin`.
- Conversation delete: `DELETE /api/conversations/{id}`.
- Text message edit: `POST /api/conversations/{id}/messages/{messageId}/edit`.
- Message delete: `DELETE /api/conversations/{id}/messages/{messageId}`.
- Regenerate support for the latest supported text reply path.
- App-native confirmation dialogs for regenerate and delete actions.
- Better UX for unsupported visible actions, including Markdown export and search entry points.

Changed:

- New conversation sends navigate immediately instead of waiting for the assistant response to finish.
- Message input clears immediately after send.
- Conversation and message management changes persist in local JSON state.

### `rikkadesk-v0.1.0-beta.1` - Early Private Beta

Added:

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
- Initial Provider Settings UI.
- Windows MSI and NSIS installer build outputs.
- Documentation for development, provider smoke tests, Provider Settings smoke tests, beta package checks, model config, and secret storage.

## Historical Updates After `rikkadesk-v0.1.0-beta.4`

The `beta/0.1.0` branch included these follow-up updates after the beta.4 feature tag:

- Provider import/export safety design.
- Release experience review and beta documentation refresh work.
- About RikkaDesk version dialog and unsigned Windows installer notes, tagged as `rikkadesk-v0.1.0-beta.5`.
- Safe Provider import/export support, tagged as `rikkadesk-v0.1.0-beta.6`.
- Long streaming timeout hotfix, tagged as `rikkadesk-v0.1.0-beta.7`.

## Security

- API keys must not be stored in `state.v1.json`.
- JSON state stores only non-sensitive provider config and `secretRef`.
- Provider APIs return `hasSecret`, not the secret value.
- Test Connection returns only safe status/error results.
- Logs and user-facing errors must not include API keys or `Authorization` header values.
- Real API keys must be entered only by the local user and must not be sent to Codex, committed, copied into documentation, or pasted into issues.

## Known Limits

- Only OpenAI-compatible text chat is supported.
- Gemini, Claude, Anthropic, Vertex, and provider-specific protocols are not implemented.
- Files, attachments, images, audio, tools, MCP, search, Workspace, forks, and multimodal requests are not implemented.
- One provider currently maps to one model in Provider Settings.
- Stop/cancel behavior is minimal and may not abort the underlying provider HTTP request immediately.
- JSON state is a beta prototype store, not the final database architecture.
- Windows installers are unsigned.
- This beta is not ready for public large-scale distribution.
- Upstream RikkaHub Android app behavior and Android build flow are intentionally unchanged.

## Package Outputs

Expected Windows build artifacts:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

## Validation

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
