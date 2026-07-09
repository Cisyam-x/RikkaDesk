# RikkaDesk beta.14 UX Hotfix Private Release Draft

This document is a private beta hotfix draft for RikkaDesk. Do not publish a public GitHub Release from this phase.

## Release Title Suggestion

```text
RikkaDesk 0.1.0 Beta 14 - Private Windows Desktop UX Hotfix Candidate
```

## Tag Suggestion

Current pushed private hotfix tag:

```text
rikkadesk-v0.1.0-beta.13
```

Candidate tag if this hotfix is accepted:

```text
rikkadesk-v0.1.0-beta.14
```

Previous private beta tag:

```text
rikkadesk-v0.1.0-beta.12
```

The `rikkadesk-v0.1.0-beta.13` tag has been created and pushed. The `rikkadesk-v0.1.0-beta.14` tag has not been created yet. Existing tags must not be moved, deleted, or overwritten. Do not publish a public GitHub Release from this draft.

## Version Strategy

Current version files:

- `web-ui/src-tauri/tauri.conf.json`: `0.1.0`
- `web-ui/src-tauri/Cargo.toml`: `0.1.0`

Recommendation:

- Keep the internal package version as `0.1.0` for this private beta line.
- Use `RikkaDesk 0.1.0 Beta 14 UX hotfix candidate` in draft notes and private tester instructions until the beta.14 tag is explicitly created.
- Keep beta labels in Git tags and release notes unless the Windows bundler version strategy is explicitly changed later.
- Do not publish a public prerelease until installer signing, support scope, and license obligations are reviewed.

Reasoning:

- The existing installer artifact names already use `0.1.0`.
- The current package is intended for local/private testing, not public distribution.
- Some Windows packaging flows prefer numeric app versions; keeping `0.1.0` avoids installer churn while the beta process is still manual.

## Draft Release Notes

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This private beta hotfix candidate packages the existing `web-ui` into a Windows desktop app and adds a local desktop API layer for basic OpenAI-compatible text chat testing plus local attachment validation.

This beta includes:

Beta.14 UX hotfix candidate delta over beta.13:

- Clear the TEXT-only image attachment validation error when switching conversations, entering the welcome/new-chat view, changing attachments, or changing models.
- Keep TEXT-only image attachment gating intact while preventing the red composer validation message from leaking across chat contexts.
- Refresh About RikkaDesk copy to show `rikkadesk-v0.1.0-beta.13` as the current private baseline and `beta.14 UX hotfix candidate` as the current build.
- Move local files/attachments, PNG/JPEG/WEBP/GIF local image attachments, TXT/PDF document chips, TEXT/IMAGE model capability markers, and loopback-only synthetic image capture into Current support.
- Clarify that real-provider image input remains disabled by default and full multimodal provider support remains unsupported.
- Keep local state at `schemaVersion: 6`.
- Keep provider import/export at version 4.
- Keep the app package version at `0.1.0`.
- Do not create a public GitHub Release from this candidate.

Beta.13 hotfix delta over beta.12:

- Fix local image attachment draft preview and sent-message rendering in Tauri production builds.
- Resolve `/api/files/path/{id}` to the actual local mock API URL for managed image rendering.
- Keep the hidden file picker input stably mounted so upload actions do not silently lose the input element.
- Catch upload detection/upload failures and always reset the file input value so the same file can be selected again.
- Keep real-provider image input disabled.
- Keep the loopback-only capture path as the only implemented image-send prototype.
- Keep local state at `schemaVersion: 6`.
- Keep provider import/export at version 4.
- Keep the app package version at `0.1.0`.

- Tauri v2 Windows desktop shell.
- Local Rust API bound to `127.0.0.1`.
- Mock fallback API for reproducible offline testing.
- Local JSON persistence for settings, conversations, messages, provider config, and id sequence.
- Conversation and message management:
  - rename conversations
  - pin/unpin conversations
  - delete conversations
  - edit/delete text messages
  - regenerate supported text replies
- Provider Settings basic multi-provider management:
  - add providers
  - select and edit providers
  - delete providers and corresponding local secret blobs
  - multiple models per provider
- Favorite model updates through the local settings API.
- Set as current model from each Provider Settings model row.
- Test Connection for a specific OpenAI-compatible model row using a safe non-streaming `/chat/completions` probe.
- Provider config includes `providers[].models[]`, `providers[].customHeaders`, `providers[].customBody`, and migration from earlier provider schema shapes.
- Local state schema v6 includes managed file metadata and provider model capability metadata.
- Advanced provider request config for non-sensitive custom headers and safe custom body JSON.
- Shared OpenAI-compatible request builder for Test Connection and Streaming Chat:
  - Test Connection forces `max_tokens=1`
  - Streaming Chat preserves allowed custom body fields such as `max_tokens`
- OpenAI-compatible text chat with streaming responses.
- Secret reference design where JSON stores `secretRef`, not the API key.
- Windows encrypted local secret blobs under app data.
- Safe provider import/export v4 for multi-model metadata, model modality metadata, and safe advanced request config, with v1/v2/v3 import compatibility.
- Markdown table overflow polish for wide GFM tables inside message content.
- Markdown/code block overflow and header layout polish.
- KaTeX mhchem support for chemistry formulas.
- Message Markdown raw HTML hardening:
  - explicit `rehypeRaw` is no longer enabled in the RikkaDesk Markdown plugin list
  - unsafe link schemes are blocked by default
  - unsafe image sources are blocked by default
  - normal `http:`, `https:`, and `mailto:` links keep safe target/rel attributes
- Workbench preview sandbox hardening:
  - HTML and SVG iframe sandbox values are empty
  - Mermaid iframe sandbox is `allow-scripts`
  - Workbench preview iframe no longer uses `allow-same-origin`
  - Workbench Mermaid uses `securityLevel: "strict"`
- Mermaid rendering in normal message Markdown remains disabled/deferred.
- Local file/attachment skeleton:
  - managed file metadata and app-data blob storage
  - PNG/JPEG/WEBP/GIF image attachments
  - TXT/PDF document chips
  - PDF/TXT remain chip-only
  - unsafe or missing image/document parts degrade safely
- Provider model capability metadata for TEXT and IMAGE input markers.
- TEXT-only models block image attachments.
- IMAGE-capable models can use a loopback-only synthetic image capture prototype.
- Loopback capture sends only one current-turn PNG/JPEG/WEBP image to a local capture server after confirmation.
- Loopback capture uses an in-memory data URL for the request only; base64 is not persisted to state, logs, exports, or message parts.
- Real-provider image input remains disabled by default and is covered only by manual gate documentation.
- Windows MSI and NSIS installer artifacts.

This beta is intended for local/private validation only.

Tester-facing installation and feedback checks are in [rikkadesk-beta-package-checklist.md](rikkadesk-beta-package-checklist.md). The beta4 private testing notes remain a historical document.

## Installation Artifacts

Expected artifacts after `pnpm run desktop:build`:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

Recommended artifact for manual beta testing:

- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

## Install Notes

1. Download or copy the private beta installer locally.
2. Run `RikkaDesk_0.1.0_x64-setup.exe`.
3. Complete the installer.
4. Start RikkaDesk.
5. Open Provider Settings from the sidebar.
6. Configure an OpenAI-compatible provider if human-only text streaming testing is needed.
7. Use Test Connection before sending a real text chat message when possible.
8. Use synthetic app data for Phase 10 attachment and loopback capture tests.
9. Do not use real user files for attachment tests.
10. Do not run a real-provider image input test from this draft.
11. For beta.14 UX hotfix validation, include beta.13 image attachment checks plus TEXT-only composer validation error clearing across chat switch, welcome/new-chat view, attachment changes, and model changes.

The installer is currently unsigned. Windows SmartScreen or unsigned publisher warnings are expected until signing is added.

## Security Notes

- Do not send real API keys to Codex.
- Do not paste real API keys into GitHub issues, release notes, screenshots, logs, or chat transcripts.
- `state.v1.json` must not contain API keys, access tokens, refresh tokens, `Authorization` header values, or `x-api-key` values.
- Provider APIs should return `hasSecret`, never the actual key.
- Test Connection should return only safe success/failure results and must not expose API keys, Authorization headers, or full request bodies.
- Provider import/export must export non-sensitive provider config only and must not export keys, reusable local `secretRef` values, tokens, DPAPI blobs, or local secret-store files.
- API key fields in docs or API shapes are field names only; they are not secret values.
- Do not use real user files for attachment or image capture tests.
- Do not run real-provider image input tests in this phase.
- Do not copy or share `mock-api/secrets/*.bin`.
- Loopback capture must not persist base64, request bodies, local paths, or storage keys.
- When validating with a real provider, search only for a short key fragment and never paste the full key into terminal history.

Security check examples:

```powershell
rg --fixed-strings "<real-key-fragment>" .
rg -a --fixed-strings "<real-key-fragment>" "$env:APPDATA/com.cisyamx.rikkadesk"
```

Expected result:

- No plaintext match in the repository.
- No plaintext match in app data files.
- `state.v1.json` contains only `secretRef`.

## Known Limits

- Only OpenAI-compatible text chat is supported for real provider testing by default.
- No Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- Local attachments and safe attachment rendering are implemented for the desktop beta candidate.
- Real-provider image input is not enabled by default.
- The image capture prototype is loopback-only and intended for synthetic local testing.
- Beta.13 does not broaden image sending beyond the loopback-only synthetic capture prototype.
- No OCR, PDF/Office parsing, audio/video input, tools, MCP, search, Workspace, forks, or full multimodal provider calls.
- One provider can contain multiple text models and model capability metadata, but per-model secrets, per-model Base URLs, provider-specific protocols, tools, and full multimodal abilities are not implemented.
- Provider import/export supports non-sensitive provider metadata only; imported providers require API keys to be entered again.
- Mermaid in normal message Markdown remains disabled/deferred.
- Workbench Mermaid preview still loads Mermaid from a remote CDN and should be revisited before public release.
- Stop/cancel may not abort the underlying provider HTTP request immediately.
- JSON state is a beta persistence mechanism, not a final database design.
- Installers are unsigned.
- No auto-update channel is configured.
- No public release support, update policy, or compatibility promise is established yet.

## Why This Is Not Ready For Public Large-Scale Distribution

- The installer is unsigned.
- The provider settings UX is still beta-level.
- The data store is JSON-based beta persistence.
- Only one provider protocol family is supported.
- P2/P3 features from upstream RikkaHub are intentionally deferred.
- Security validation still depends on manual local checks.
- There is no rollback, backup, migration, or support policy for broad users.
- AGPL/commercial licensing boundaries need to be reviewed before any public distribution.

## Manual Verification Checklist

Before sharing any private beta installer:

- Confirm working tree is clean.
- Confirm `origin` is `https://github.com/Cisyam-x/RikkaDesk.git`.
- Confirm `upstream` is `https://github.com/rikkahub/rikkahub.git`.
- Confirm `LICENSE` is present.
- Confirm `NOTICE.md` states RikkaDesk is an unofficial derivative.
- Run `pnpm run typecheck`.
- Run `cargo check --manifest-path src-tauri/Cargo.toml`, or record any local Windows Application Control block.
- Run `pnpm run desktop:build`.
- Confirm MSI and NSIS installers are generated.
- Confirm installers are expected to be unsigned.
- Install the NSIS installer locally.
- Start RikkaDesk.
- Open Provider Settings.
- Add, edit, and delete a provider without exposing any real key to logs or docs.
- Add multiple model rows under one provider.
- Add safe Advanced request config and confirm unsafe custom headers/body are rejected.
- Confirm provider deletion removes the corresponding local secret.
- Confirm favorite model changes work.
- Confirm Set as current model updates the model selector for a specific model row.
- Confirm Test Connection succeeds with a valid provider/model row and fails safely with an invalid provider or model.
- Confirm Test Connection and Streaming Chat both use the safe custom request config behavior.
- Confirm provider export writes version 4 JSON with `providers[].models[]`, model modality metadata, safe `customHeaders[]`, and safe `customBody`.
- Confirm provider import works for version 4 modality exports, version 3 advanced exports, version 2 multi-model exports, and older version 1 single-model exports.
- Confirm provider exports do not include API keys, `secretRef`, tokens, Authorization headers, `x-api-key`, cookies, DPAPI blobs, local secret-store files, or internal model ids.
- Confirm wide Markdown tables scroll inside the message content area.
- Confirm code block copy, download, and preview actions still work.
- Confirm inline math, block math, and mhchem chemistry formulas render.
- Confirm unsafe Markdown link schemes and unsafe image sources are blocked.
- Confirm the Markdown XSS fixture does not execute in message bubbles.
- Confirm Workbench HTML/SVG preview sandbox values are empty.
- Confirm Workbench Mermaid preview uses `allow-scripts`, does not use `allow-same-origin`, and runs with `securityLevel: "strict"`.
- Upload synthetic PNG/JPEG/WEBP/GIF images and confirm they remain local attachments.
- Upload synthetic TXT/PDF files and confirm they render as document chips only.
- Confirm unsafe or missing image/document parts degrade safely.
- Confirm TEXT-only models block image attachments.
- Confirm IMAGE-capable models can run the loopback-only synthetic capture path with one current-turn PNG/JPEG/WEBP image.
- Confirm loopback capture does not persist base64, `image_url`, request bodies, local paths, storage keys, or API keys.
- Confirm real-provider image input remains disabled.
- Confirm `hasSecret: true` only after a local key is saved.
- Send a text-only test message.
- Confirm streaming text appears when a real provider is configured.
- Restart RikkaDesk.
- Confirm conversation history persists.
- Confirm `state.v1.json` does not contain secret values.
- Search repository and app data for the local test key fragment.
- Uninstall RikkaDesk.
- Confirm whether app data remains and tell testers how to clear it manually.

## Merge And Release Draft

Recommended private beta flow:

1. Develop in phase branches.
2. Merge accepted phase PRs into `beta/0.1.0`.
3. Create private beta tags only for feature-stable checkpoints.
4. Keep `rikkadesk-v0.1.0-beta.12` unchanged and keep `rikkadesk-v0.1.0-beta.13` pointing at the accepted beta.13 hotfix docs commit. Create `rikkadesk-v0.1.0-beta.14` only after beta.14 preflight is explicitly approved.
5. Do not publish a public GitHub Release until signing, support scope, and license obligations are reviewed.

Do not force-push `main`, `master`, or `beta/0.1.0`.
