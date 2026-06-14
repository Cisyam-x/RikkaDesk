# RikkaDesk 0.1.0 Beta Release Draft

This document is a private release draft for RikkaDesk. Do not publish a public GitHub Release from this phase.

## Release Title Suggestion

```text
RikkaDesk 0.1.0 Beta 1 - Local Windows Desktop Preview
```

## Tag Suggestion

Preferred private beta tag:

```text
rikkadesk-v0.1.0-beta.1
```

Keep the app/package version as `0.1.0` for this local beta unless the version strategy is explicitly changed in a later phase.

## Version Strategy

Current version files:

- `web-ui/src-tauri/tauri.conf.json`: `0.1.0`
- `web-ui/src-tauri/Cargo.toml`: `0.1.0`

Recommendation:

- Keep `0.1.0` for the first local/private beta package.
- Use `RikkaDesk 0.1.0 Beta 1` in release notes and private tester instructions.
- Before a public prerelease, test whether the Tauri Windows bundler accepts `0.1.0-beta.1` cleanly for MSI and NSIS outputs.
- Do not change version files in Phase 5B without explicit confirmation.

Reasoning:

- The existing installer artifact names already use `0.1.0`.
- The current package is intended for local/private testing, not public distribution.
- Some Windows packaging flows prefer numeric app versions; keeping `0.1.0` avoids installer churn while the beta process is still manual.

## Draft Release Notes

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This private beta packages the existing `web-ui` into a Windows desktop app and adds a local desktop API layer for basic chat testing.

This beta includes:

- Tauri v2 Windows desktop shell.
- Local Rust API bound to `127.0.0.1`.
- Mock fallback API for reproducible offline testing.
- Local JSON persistence for settings, conversations, messages, provider config, and id sequence.
- Provider Settings UI for one OpenAI-compatible provider.
- OpenAI-compatible text chat with streaming responses.
- Secret reference design where JSON stores `secretRef`, not the API key.
- Windows encrypted local secret blobs under app data.
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

The installer is currently unsigned. Windows SmartScreen warnings are expected until signing is added.

## Security Notes

- Do not send real API keys to Codex.
- Do not paste real API keys into GitHub issues, release notes, screenshots, logs, or chat transcripts.
- `state.v1.json` must not contain API keys, access tokens, refresh tokens, `Authorization` header values, or `x-api-key` values.
- Provider APIs should return `hasSecret`, never the actual key.
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
- No files, attachments, images, audio, tools, MCP, search, forks, or multimodal provider calls.
- Provider Settings is a minimal single-provider UI.
- Stop/cancel may not abort the underlying provider HTTP request immediately.
- JSON state is a beta persistence mechanism, not a final database design.
- Installers are unsigned.
- No auto-update channel is configured.
- No public release support, update policy, or compatibility promise is established yet.

## Why This Is Not Ready For Public Large-Scale Distribution

- The installer is unsigned.
- The provider settings UX is intentionally minimal.
- The data store is JSON-based beta persistence.
- Only one provider protocol family is supported.
- P2/P3 features from upstream RikkaHub are intentionally deferred.
- Security validation still depends on manual local checks.
- There is no rollback, backup, migration, or support policy for broad users.
- AGPL/commercial licensing boundaries need to be reviewed before any public distribution.

## Manual Verification Checklist

Before creating any private beta tag or draft release:

- Confirm working tree is clean.
- Confirm `origin` is `https://github.com/Cisyam-x/RikkaDesk.git`.
- Confirm `upstream` is `https://github.com/rikkahub/rikkahub.git`.
- Confirm `LICENSE` is present.
- Confirm `NOTICE.md` states RikkaDesk is an unofficial derivative.
- Run `pnpm run typecheck`.
- Run `cargo check --manifest-path src-tauri/Cargo.toml`.
- Run `pnpm run desktop:build`.
- Confirm MSI and NSIS installers are generated.
- Install the NSIS installer locally.
- Start RikkaDesk.
- Open Provider Settings.
- Save a test provider without exposing any real key to logs or docs.
- Confirm `hasSecret: true` only after a local key is saved.
- Send a text-only test message.
- Confirm streaming text appears when a real provider is configured.
- Restart RikkaDesk.
- Confirm conversation history persists.
- Confirm `state.v1.json` does not contain secret values.
- Search repository and app data for the local test key fragment.
- Uninstall RikkaDesk.
- Decide whether app data should be manually cleared before the next test.

## Merge And Tag Draft

Recommended sequence:

1. Push `rikkadesk/phase-5b-beta-release-draft`.
2. Open a Pull Request into the RikkaDesk default branch.
3. Review docs, build output, and security scan notes.
4. Merge via Pull Request if the beta docs are accepted.
5. Optionally create a protected beta integration branch such as `beta/0.1.0`.
6. Optionally create a private/local tag such as `rikkadesk-v0.1.0-beta.1`.
7. Do not publish a public GitHub Release until signing, support scope, and license obligations are reviewed.

Do not force-push `main` or `master`.
