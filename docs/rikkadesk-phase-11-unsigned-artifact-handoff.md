# RikkaDesk Phase 11 Unsigned Artifact Handoff Runbook

This runbook standardizes local handoff of unsigned RikkaDesk Windows artifacts for self-testing and small-scope private beta testing. It is documentation only: it does not create artifacts, commit binaries or real hashes, sign packages, create a tag, or publish a GitHub Release.

## Current Policy

- Distribution scope: unsigned private beta only.
- Current stable self-test tag: `rikkadesk-v0.1.0-beta.14`.
- GitHub Release: No.
- Signing: deferred.
- Public release: not ready.
- App/package version: `0.1.0`.
- `schemaVersion`: `6`.
- Provider import/export version: `4`.
- Real-provider image input: not enabled by default.

## Scope

Use this process for:

- Local self-testing.
- Small, explicitly invited private beta testing.
- Private artifact transfer outside Git and outside GitHub Releases.
- Unsigned package testing where Windows security warnings or policy blocks are recorded honestly.

Do not use this process for:

- Public releases.
- Broad distribution.
- Enterprise deployment.
- Any release represented as signed or publisher-verified.
- Any request that asks testers to disable SmartScreen, Smart App Control, Microsoft Defender, or enterprise security policy.

## Local Handoff Directory

Create the handoff directory outside the repository. A recommended local layout is:

```text
RikkaDesk-beta14-private-unsigned/
|-- RikkaDesk_0.1.0_x64-setup.exe
|-- RikkaDesk_0.1.0_x64_en-US.msi
|-- SHA256SUMS.txt
|-- README-private-unsigned.md
`-- TEST_NOTES.md
```

This directory is local/private handoff material:

- Do not commit it to Git.
- Do not upload it to a GitHub Release.
- Do not place it inside the repository working tree.
- Do not include RikkaDesk app data.
- Do not include `mock-api/secrets/*.bin`.
- Do not include API keys, provider credentials, real chat history, or user files.
- Do not include command output that contains local user paths or secret values.

The standalone `rikkadesk.exe` may be retained for local verification, but the recommended tester handoff is the NSIS setup executable plus the MSI fallback.

## Build And Verify

Run from the repository root:

```powershell
pnpm --dir web-ui run typecheck
cargo check --manifest-path web-ui/src-tauri/Cargo.toml
pnpm --dir web-ui run desktop:build
```

Expected source artifact paths:

```text
web-ui/src-tauri/target/release/rikkadesk.exe
web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

Before copying artifacts, confirm the intended commit/tag, a clean working tree, and that all three files exist. Copy only the MSI and NSIS package into the normal private handoff directory unless the standalone executable is specifically needed for local verification.

## SHA256 Generation

Unsigned private artifacts still require SHA256 hashes so the sender and tester can verify byte-for-byte handoff integrity.

For the current unsigned workflow, generate hashes after the final build and after any artifact copy/rename step. If signing is introduced later, sign first and generate hashes only after all signatures are final because signing changes file bytes.

Repository artifact hash commands:

```powershell
Get-FileHash web-ui/src-tauri/target/release/rikkadesk.exe -Algorithm SHA256
Get-FileHash web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi -Algorithm SHA256
Get-FileHash web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe -Algorithm SHA256
```

`SHA256SUMS.txt` template:

```text
<SHA256>  rikkadesk.exe
<SHA256>  RikkaDesk_0.1.0_x64_en-US.msi
<SHA256>  RikkaDesk_0.1.0_x64-setup.exe
```

Rules:

- Replace placeholders only in the local handoff copy.
- Do not commit the real `SHA256SUMS.txt` in P4-A.
- Recompute hashes if any artifact is rebuilt, signed, renamed through a tool that rewrites it, or otherwise modified.
- Have the tester hash the received files and compare against the separately shared values.
- A matching hash verifies transfer integrity; it does not provide publisher identity or replace code signing.

## `README-private-unsigned.md` Template

Use this template only in the local handoff directory:

```markdown
# RikkaDesk beta.14 Private Unsigned Build

This package is an unsigned private beta for invited testing only. It is not a public release and no GitHub Release has been created.

## Install

- Recommended: `RikkaDesk_0.1.0_x64-setup.exe` (NSIS).
- Alternate: `RikkaDesk_0.1.0_x64_en-US.msi` (MSI).
- Verify the supplied SHA256 before installation.

Windows SmartScreen or Smart App Control may warn about or block unsigned software. Do not disable Windows security features or bypass organization policy. If execution is blocked, record it as an unsigned-publisher policy block and stop that test.

## Current Limits

- Real-provider image input is not enabled by default.
- Local attachments remain local-only in normal use.
- Installers and the application executable are unsigned.
- This build has no public-release support commitment.

## Privacy

- Do not share API keys, provider credentials, logs containing sensitive values, app data, chat history, or user files.
- Never share `mock-api/secrets/*.bin`.
- Manual app data cleanup deletes locally stored provider keys and they must be entered again.
- Stop RikkaDesk and confirm no RikkaDesk process is running before app data cleanup.
```

## `TEST_NOTES.md` Template

Use this template for private tester results. Do not attach API keys, app data, secret blobs, real request bodies, or sensitive logs.

```markdown
# RikkaDesk Private Unsigned Test Notes

- Build/tag: `rikkadesk-v0.1.0-beta.14`
- Windows version:
- Install method: NSIS / MSI
- SHA256 verified: Yes / No
- SmartScreen result:
- Smart App Control result:
- Enterprise policy result, if applicable:
- App launched: Pass / Fail
- About dialog version/tag: Pass / Fail
- Mock chat: Pass / Fail
- Provider Settings: Pass / Fail
- Text streaming with a human-supplied local test provider, if explicitly performed: Pass / Fail / Not tested
- Local PNG/JPEG/WEBP/GIF attachment: Pass / Fail / Not tested
- TXT/PDF document chip: Pass / Fail / Not tested
- TEXT-only image block: Pass / Fail / Not tested
- NSIS uninstall: Pass / Fail / Not tested
- MSI uninstall: Pass / Fail / Not tested
- App data retained after uninstall: Yes / No / Not checked
- No API keys, secret blobs, app data, or sensitive logs attached: Confirmed / Not confirmed
- Notes (no secrets or real user data):
```

If a tester uses a real provider locally for text-only testing, the tester must enter the key themselves and must not include it in screenshots, notes, logs, or handoff files. Real-provider image input remains disabled and is not part of this runbook.

## Install And Uninstall Smoke

Recommended NSIS smoke sequence:

1. Verify the NSIS SHA256 against the private handoff value.
2. Install `RikkaDesk_0.1.0_x64-setup.exe`.
3. Record SmartScreen, Smart App Control, or enterprise policy behavior without bypassing it.
4. Launch RikkaDesk.
5. Confirm the About dialog reports the beta.14 private line accurately.
6. Run mock text chat.
7. Check Provider Settings without exposing credentials.
8. Run local attachment smoke with synthetic/non-sensitive fixtures only.
9. Confirm TEXT-only image gating remains active.
10. Close RikkaDesk.
11. Confirm no `RikkaDesk` process remains.
12. Uninstall RikkaDesk.
13. Record whether app data remains.
14. Perform manual app data cleanup only if the tester explicitly intends to remove local conversations, attachments, settings, and provider keys.

MSI may be tested separately as the fallback artifact. Record NSIS and MSI results independently.

Windows uninstall may retain app data and encrypted local secret blobs. This is expected private-beta behavior and is not by itself an uninstall failure.

## App Data Cleanup

RikkaDesk app data is stored under:

Formal app data backup/restore and migration safety is handled by Phase 12. Manual folder copies are not yet a supported portable backup format.

```powershell
$env:APPDATA\com.cisyamx.rikkadesk
```

The mock API directory is:

```powershell
$env:APPDATA\com.cisyamx.rikkadesk\mock-api
```

Before cleanup:

- Exit RikkaDesk completely.
- Confirm no RikkaDesk process is running.
- Understand that cleanup deletes local conversations, attachment blobs, settings, and provider keys.
- Do not share the directory or any backup of it.
- Never inspect, print, parse, upload, or share `mock-api/secrets/*.bin`.

Process check and cleanup example:

```powershell
Get-Process RikkaDesk -ErrorAction SilentlyContinue

$AppData = Join-Path $env:APPDATA "com.cisyamx.rikkadesk"
if (Test-Path -LiteralPath $AppData) {
  Remove-Item -LiteralPath $AppData -Recurse -Force
}
```

Do not run the deletion block while `Get-Process` still reports RikkaDesk. A backup contains the same private data and encrypted secret blobs as the live directory, so a backup must also remain private and outside the repository.

## Security Search

### Repository Search

Before private handoff, inspect committed and pending changes for accidental secret or release-material additions:

```powershell
git status --short
git diff --check
git diff | rg -i "apiKey|Authorization|x-api-key|accessToken|refreshToken|secretRef|mock-api/secrets|DPAPI|password|token|cookie|bearer|certificate|private key|pfx|p12|base64,|request body|GitHub Release|real-provider|SHA256SUMS"
git ls-files | rg -i "(^|/)(secrets|app[-_ ]?data)(/|$)|\.pfx$|\.p12$|\.bin$|SHA256SUMS\.txt$|\.exe$|\.msi$"
```

Classify expected documentation and source-code references separately from prohibited material. Stop handoff if tracked files include real binaries, real hashes intended only for private handoff, real app data, secret blobs, certificate material, private keys, passwords, or tokens.

### Optional Test-Key Fragment Check

Only when a human intentionally used a non-production test key locally, the human may search a known non-secret fragment in the specific text state file. Do not recursively search app data and do not read `mock-api/secrets/*.bin`.

```powershell
$StatePath = Join-Path $env:APPDATA "com.cisyamx.rikkadesk\mock-api\state.v1.json"
if (Test-Path -LiteralPath $StatePath) {
  Select-String -LiteralPath $StatePath -SimpleMatch "<NON_SECRET_TEST_KEY_FRAGMENT>"
}
```

Expected result: no plaintext key fragment in `state.v1.json`. Do not paste a real full key into shell history or notes. Do not copy the state file into the repository or handoff directory.

## P4-A Explicit Non-Goals

P4-A does not:

- Commit `rikkadesk.exe`, MSI, or NSIS artifacts.
- Commit real SHA256 values or a completed `SHA256SUMS.txt` release artifact.
- Create or publish a GitHub Release.
- Sign artifacts or resume P3 signing smoke.
- Add certificate, private key, `.pfx`, `.p12`, password, token, or CI signing configuration.
- Enable auto-update.
- Change app/package version from `0.1.0`.
- Change `schemaVersion` or provider import/export version.
- Enable real-provider image input.

## Next Steps

Choose one of these scoped follow-ups:

- P4-B: generate a local unsigned handoff package and real SHA256 values outside the repository. Keep all generated artifacts and handoff notes uncommitted and private.
- Push the Phase 11 documentation branch and open a Draft PR without creating a tag or GitHub Release.
- P5: design updater channels, signing, rollout, and rollback later. Do not enable an updater while the private beta remains unsigned and has no public release channel.
