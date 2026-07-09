# Changelog

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

This changelog tracks the RikkaDesk desktop work in this fork. It does not replace the upstream RikkaHub changelog or release notes.

## 0.1.0 Private Beta Line

The current pushed private hotfix tag is `rikkadesk-v0.1.0-beta.13`. The current branch contains a beta.14 UX hotfix candidate after that tag; no beta.14 tag or public GitHub Release has been created yet.

This beta line is not a public GitHub Release.

### `rikkadesk-v0.1.0-beta.14` - UX Hotfix Candidate

Status:

- Candidate only; the tag has not been created.
- `rikkadesk-v0.1.0-beta.13` remains the current pushed private hotfix tag and must not be moved.
- This is not a public GitHub Release.

Fixed:

- Clear the TEXT-only image attachment validation error when switching conversations, entering the welcome/new-chat view, changing attachments, or changing models.
- Avoid carrying the composer red validation message from one chat context into another.

Updated:

- Refresh About RikkaDesk copy for the beta.14 candidate line.
- Show `rikkadesk-v0.1.0-beta.13` as the current private baseline and `beta.14 UX hotfix candidate` as the current build.
- Move local files/attachments and local image attachments into Current support.
- Clarify that real-provider image input is still not enabled by default, while local PNG/JPEG/WEBP/GIF image attachments are supported.

Unchanged:

- Real-provider image input remains disabled by default.
- The loopback-only capture path remains the only implemented image-send prototype.
- Local state remains `schemaVersion: 6`.
- Provider import/export remains version 4.
- The app/package version remains `0.1.0`.

### `rikkadesk-v0.1.0-beta.13` - Attachment Rendering Hotfix

Fixed:

- Fix local image attachment draft preview and sent-message rendering in Tauri production builds.
- Resolve managed file URLs from `/api/files/path/{id}` to the actual local mock API URL before image rendering.
- Keep the hidden file picker input stably mounted so repeated upload attempts do not silently lose the file input.
- Catch upload detection/upload errors and always reset the file input value, allowing the same file to be selected again after delete or failure.

Unchanged:

- Real-provider image input remains disabled.
- The loopback-only capture path remains the only implemented image-send prototype.
- Local state remains `schemaVersion: 6`.
- Provider import/export remains version 4.
- The app/package version remains `0.1.0`.
- `rikkadesk-v0.1.0-beta.12` must not be moved; beta.13 is a new hotfix tag.
- This is not a public GitHub Release.

### `rikkadesk-v0.1.0-beta.12` - Files, Attachments, And Loopback Image Capture Candidate

Added:

- Add a local attachment skeleton for managed files.
- Add safe PNG/JPEG/WEBP/GIF image attachments and TXT/PDF document chips.
- Add safe image/document message rendering for managed file URLs.
- Add provider model capability metadata with TEXT and IMAGE input markers.
- Add IMAGE-capable model confirmation UI.
- Add a loopback-only synthetic image capture prototype for one current-turn PNG/JPEG/WEBP image.
- Add real-provider image manual gate documentation.

Changed:

- Provider import/export is version 4 with model modality metadata.
- Local state schema is 6.
- Attachment workflows are documented as local-first and provider-safe.

Security:

- File blobs are stored under app data and referenced by managed file IDs.
- State must not contain file contents, base64 payloads, original absolute paths, provider request bodies, or API keys.
- Loopback capture keeps data URLs in memory for the request only and does not persist them.
- Real-provider image input remains disabled unless a later manual gate implementation is reviewed.

Known limits:

- Real-provider image input is not enabled in the beta.12 default scope.
- OCR, PDF/Office parsing, audio/video input, Workspace, MCP/tools, search, and full multimodal provider support remain unsupported.
- Installers are unsigned.

### `rikkadesk-v0.1.0-beta.11` - Markdown Rendering Hardening

Changed:

- Polish Markdown table overflow so wide GFM tables scroll inside message content.
- Improve Markdown/code block overflow and header layout.
- Enable KaTeX mhchem support for chemistry formulas such as `\ce{H2O}`.
- Harden message Markdown raw HTML handling by removing explicit `rehypeRaw`.
- Add safe link handling for message Markdown.
- Add safe image source handling for message Markdown.
- Block unsafe link schemes such as `javascript:`, `data:`, `file:`, and `blob:`.
- Keep normal `http:`, `https:`, and `mailto:` links with safe `target` / `rel` attributes.
- Narrow the Workbench preview iframe sandbox.
- Change Workbench Mermaid `securityLevel` from `loose` to `strict`.
- Keep Mermaid rendering in normal message Markdown disabled/deferred.
- Document residual risk: Workbench Mermaid still uses a remote CDN and should be revisited before public release.

### `rikkadesk-v0.1.0-beta.10` - Provider Advanced Request Config

Added:

- Add provider Advanced request config for OpenAI-compatible providers.
- Add non-sensitive custom headers and safe custom body JSON.
- Upgrade local provider state to `schemaVersion: 4`.
- Add shared OpenAI-compatible request builder for Test Connection and Streaming Chat.
- Keep Test Connection `max_tokens` forced to `1`.
- Preserve allowed custom `max_tokens` for streaming chat.
- Upgrade provider import/export to version 3 with safe `customHeaders` / `customBody`.
- Keep import compatibility for provider export versions 1 and 2.
- Continue excluding API keys, `secretRef`, Authorization, `x-api-key`, tokens, cookies, DPAPI blobs, and local secret-store files from exports.

### `rikkadesk-v0.1.0-beta.9` - Provider Multi-Model

Added:

- Add multi-model Provider Settings.
- Upgrade local provider state to `schemaVersion: 3` with `providers[].models[]`.
- Migrate `schemaVersion: 2` providers from `model` to `models[0]`.
- Allow one OpenAI-compatible provider to contain multiple models sharing the same Base URL and API key.
- Allow setting current model and testing connection per model row.
- Upgrade provider import/export to version 2 with `models[]`.
- Keep import compatibility for version 1 provider exports.
- Continue excluding API keys, `secretRef`, tokens, Authorization headers, DPAPI blobs, and local secret-store files from provider exports.

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
- Beta release copy hotfix, tagged as `rikkadesk-v0.1.0-beta.8`.
- Provider multi-model state, UI, and import/export v2 work, tagged or planned as `rikkadesk-v0.1.0-beta.9`.
- Provider advanced request config and import/export v3 work, tagged as `rikkadesk-v0.1.0-beta.10`.
- Markdown rendering polish, mhchem support, raw HTML hardening, and Workbench preview sandbox hardening, tagged as `rikkadesk-v0.1.0-beta.11`.
- Local attachment skeleton, safe attachment rendering, model capability metadata, and loopback-only synthetic image capture prototype, tagged as `rikkadesk-v0.1.0-beta.12`.
- Local image attachment rendering and file picker stability hotfix, tagged as `rikkadesk-v0.1.0-beta.13`.

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
- Local file attachments and safe attachment rendering exist in the beta.13 hotfix line, but real-provider image input is not enabled by default.
- One provider can contain multiple text models and model capability metadata, but per-model secrets, per-model Base URLs, tools, OCR/PDF parsing, and full multimodal provider requests are not implemented.
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
