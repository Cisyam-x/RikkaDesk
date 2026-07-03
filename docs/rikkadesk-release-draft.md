# RikkaDesk 0.1.0 Beta 4 Release Draft

This document is a private release draft for RikkaDesk. Do not publish a public GitHub Release from this phase.

## Release Title Suggestion

```text
RikkaDesk 0.1.0 Beta 4 - Private Windows Desktop Candidate
```

## Tag Suggestion

Current feature-stable private beta tag:

```text
rikkadesk-v0.1.0-beta.4
```

The `beta/0.1.0` branch currently contains additional documentation-only work after beta.4, including the Provider import/export safety design. The feature-stable installer behavior remains represented by `rikkadesk-v0.1.0-beta.4`.

## Version Strategy

Current version files:

- `web-ui/src-tauri/tauri.conf.json`: `0.1.0`
- `web-ui/src-tauri/Cargo.toml`: `0.1.0`

Recommendation:

- Keep the internal package version as `0.1.0` for this private beta line.
- Use `RikkaDesk 0.1.0 Beta 4` in release notes and private tester instructions.
- Keep beta labels in Git tags and release notes unless the Windows bundler version strategy is explicitly changed later.
- Do not publish a public prerelease until installer signing, support scope, and license obligations are reviewed.

Reasoning:

- The existing installer artifact names already use `0.1.0`.
- The current package is intended for local/private testing, not public distribution.
- Some Windows packaging flows prefer numeric app versions; keeping `0.1.0` avoids installer churn while the beta process is still manual.

## Draft Release Notes

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This private beta packages the existing `web-ui` into a Windows desktop app and adds a local desktop API layer for basic OpenAI-compatible text chat testing.

This beta includes:

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
  - one model per provider
- Favorite model updates through the local settings API.
- Set as current model from Provider Settings.
- Test Connection for OpenAI-compatible providers using a safe non-streaming `/chat/completions` probe.
- OpenAI-compatible text chat with streaming responses.
- Secret reference design where JSON stores `secretRef`, not the API key.
- Windows encrypted local secret blobs under app data.
- Provider import/export safety design documentation. The import/export feature itself is not implemented.
- Windows MSI and NSIS installer artifacts.

This beta is intended for local/private validation only.

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
6. Configure an OpenAI-compatible provider if real-provider testing is needed.
7. Use Test Connection before sending a real chat message when possible.

The installer is currently unsigned. Windows SmartScreen or unsigned publisher warnings are expected until signing is added.

## Security Notes

- Do not send real API keys to Codex.
- Do not paste real API keys into GitHub issues, release notes, screenshots, logs, or chat transcripts.
- `state.v1.json` must not contain API keys, access tokens, refresh tokens, `Authorization` header values, or `x-api-key` values.
- Provider APIs should return `hasSecret`, never the actual key.
- Test Connection should return only safe success/failure results and must not expose API keys, Authorization headers, or full request bodies.
- Provider import/export, when implemented later, must export non-sensitive provider config only and must not export keys or reusable local `secretRef` values.
- API key fields in docs or API shapes are field names only; they are not secret values.
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

- Only OpenAI-compatible text chat is supported.
- No Gemini, Claude, Anthropic, Vertex, or provider-specific protocols.
- No files, attachments, images, audio, tools, MCP, search, Workspace, forks, or multimodal provider calls.
- One provider currently maps to one model in Provider Settings.
- Provider import/export is documented but not implemented.
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
- Confirm provider deletion removes the corresponding local secret.
- Confirm favorite model changes work.
- Confirm Set as current model updates the model selector.
- Confirm Test Connection succeeds with a valid provider and fails safely with an invalid provider.
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
4. Keep `rikkadesk-v0.1.0-beta.4` as the current feature-stable beta tag until a later feature checkpoint is validated.
5. Do not publish a public GitHub Release until signing, support scope, and license obligations are reviewed.

Do not force-push `main`, `master`, or `beta/0.1.0`.
