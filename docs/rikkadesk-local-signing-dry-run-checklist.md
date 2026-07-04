# RikkaDesk Local Windows Signing Dry-Run Checklist

Current stable tag: `rikkadesk-v0.1.0-beta.5`

This document is a future dry-run checklist only. It does not enable signing, does not modify Tauri config, does not purchase or configure a certificate, and does not include real certificate paths, PFX files, private keys, passwords, token PINs, cloud signing credentials, or production certificate thumbprints.

## 1. Purpose

This checklist is for a future local Windows machine dry-run of RikkaDesk code signing.

Current status:

- RikkaDesk private beta remains unsigned.
- No real signing certificate is configured.
- No signing command should be executed from this document during Phase 6E P2.
- All command examples use placeholders only.
- This document does not provide any way to bypass SmartScreen, Smart App Control, Microsoft Defender, or enterprise security policy.

## 2. Dry-Run Scope

The future dry-run should cover:

- Checking whether Windows SDK / SignTool is available.
- Checking whether build artifacts exist.
- Checking current unsigned Authenticode status before signing.
- Designing and recording a future signing order.
- Verifying signatures after signing.
- Installing, starting, and uninstalling RikkaDesk.
- Observing SmartScreen and Smart App Control behavior.

The dry-run cannot guarantee that signing will remove every warning or allow execution on every Windows machine. Enterprise policies, SmartScreen reputation, Smart App Control, and local security settings can still block or warn.

## 3. Prerequisite Checklist

Record local machine prerequisites before any future dry-run:

- [ ] Windows version recorded.
- [ ] Windows edition recorded.
- [ ] SmartScreen state observed, without changing it for the test.
- [ ] Smart App Control state observed, without changing it for the test.
- [ ] Visual Studio Build Tools or Windows SDK installed.
- [ ] `signtool.exe` available from a Developer PowerShell / Developer Command Prompt or known Windows SDK path.
- [ ] PowerShell `Get-AuthenticodeSignature` available.
- [ ] Local build passes:

```powershell
cd web-ui
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run desktop:build
```

- [ ] Build artifacts exist:

```text
web-ui/src-tauri/target/release/rikkadesk.exe
web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

## 4. Pre-Signing Checks

Before any future signing attempt, record current signature status.

PowerShell status checks:

```powershell
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/rikkadesk.exe"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

SignTool verification checks:

```powershell
signtool verify /pa /v "web-ui/src-tauri/target/release/rikkadesk.exe"
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

Expected for the current unsigned private beta:

- `Get-AuthenticodeSignature` may show `NotSigned`.
- `signtool verify` may fail.
- This is not a build failure; it is the expected current unsigned state.

## 5. Future Signing Order Draft

Initial signing order to test:

1. Sign `web-ui/src-tauri/target/release/rikkadesk.exe`.
2. Sign `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`.
3. Sign `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`.

This order is a draft. Final order must be validated during an actual dry-run because Tauri, MSI, and NSIS packaging behavior may affect which files contain or wrap other files.

## 6. Placeholder SignTool Commands

These commands are examples only. They use placeholders and must not be committed with real values.

Single-artifact placeholder:

```powershell
signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "<PATH_TO_ARTIFACT>"
```

RikkaDesk artifact placeholders:

```powershell
signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "web-ui/src-tauri/target/release/rikkadesk.exe"

signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"

signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

Do not write:

- Real thumbprints.
- Real certificate paths.
- PFX paths.
- Passwords.
- Token PINs.
- Cloud signing credentials.

## 7. Post-Signing Verification Checklist

After any future signing attempt:

- [ ] Verify `rikkadesk.exe`:

```powershell
signtool verify /pa /v "web-ui/src-tauri/target/release/rikkadesk.exe"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/rikkadesk.exe"
```

- [ ] Verify MSI:

```powershell
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
```

- [ ] Verify NSIS:

```powershell
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

- [ ] Check signer subject / publisher.
- [ ] Check timestamp exists.
- [ ] Record post-signing file hashes.
- [ ] Confirm all three artifacts are signed.
- [ ] Confirm no signing credential appears in logs, shell history snippets, docs, or screenshots.

Hash note: file hashes are expected to change after signing because the signature changes the file contents. Record both pre-signing and post-signing hashes if reproducibility analysis is needed.

## 8. Install and Launch Verification

Use a clean Windows test machine or VM appropriate for testing signed installers.

- [ ] Install NSIS setup:

```text
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

- [ ] Start RikkaDesk from Start Menu or installed shortcut.
- [ ] Open the About RikkaDesk dialog.
- [ ] Confirm version: `0.1.0`.
- [ ] Confirm feature stable tag: `rikkadesk-v0.1.0-beta.5` if the About copy has been updated in a later phase; otherwise record the displayed tag.
- [ ] Open Provider Settings.
- [ ] Run Mock chat without entering a real API key.
- [ ] Uninstall RikkaDesk.
- [ ] Check whether app data remains, matching documented beta behavior.

Do not enter a real API key unless a human tester intentionally does so locally. If a real key is used in a separate manual test, it must not appear in logs, documents, screenshots, terminal output, or issue comments.

## 9. SmartScreen and Smart App Control Observation

Record observations without changing security settings for the purpose of passing the test:

- [ ] Does SmartScreen still warn?
- [ ] Does Windows show a verified publisher?
- [ ] Is the installed executable blocked by Smart App Control?
- [ ] Windows version recorded.
- [ ] Smart App Control state recorded.
- [ ] SmartScreen / reputation behavior recorded.

Important expectations:

- Signing does not guarantee that all machines show no warnings.
- SmartScreen reputation may depend on certificate type, publisher reputation, download volume, and Microsoft cloud reputation.
- Smart App Control can still make policy decisions.
- Do not advise testers to disable Smart App Control, SmartScreen, Defender, or enterprise application control just to pass this test.

## 10. Failure Handling

Record failures instead of bypassing security controls.

Common cases:

- `signtool.exe` not found: install or repair Windows SDK / Visual Studio Build Tools in a future setup step.
- Certificate unavailable: confirm token/HSM/cloud signing access with the certificate owner.
- Timestamp request fails: confirm `<TIMESTAMP_URL>` and network access.
- MSI verification fails: record the exact command and result.
- NSIS verification fails: record the exact command and result.
- Installer is signed but installed `rikkadesk.exe` is not signed: inspect whether the packaged executable was signed before installer creation, or whether signing order must change.
- Hash changed after signing: expected; record pre/post hashes.
- SmartScreen still warns: record behavior; do not bypass.
- Smart App Control still blocks: record behavior; do not bypass.
- Enterprise policy blocks execution: record policy context; do not bypass.

## 11. Result Record Template

```text
Windows version:
Windows edition:
Build commit:
Tag:
Certificate type: OV / EV / IV / test only
Signing method: manual SignTool / Tauri integrated / cloud signing
Certificate storage: hardware token / HSM / cloud signing / test only
Timestamp URL used: <TIMESTAMP_URL or not recorded>

Pre-sign exe status:
Pre-sign MSI status:
Pre-sign NSIS status:

Post-sign exe status:
Post-sign MSI status:
Post-sign NSIS status:

exe signer subject / publisher:
MSI signer subject / publisher:
NSIS signer subject / publisher:

SmartScreen result:
Smart App Control result:
Install result:
Launch result:
About dialog result:
Provider Settings result:
Mock chat result:
Uninstall result:
App data retained:

Contains real API key in logs/docs/screenshots: must be no
Contains signing credential in logs/docs/screenshots: must be no
Notes:
```

## 12. Relationship to P0 / P1 Documents

- Phase 6E P0: `docs/rikkadesk-windows-code-signing-research.md` researches Windows code signing and installer trust.
- Phase 6E P1: `docs/rikkadesk-tauri-signing-config-draft.md` sketches placeholder-level Tauri signing configuration.
- Phase 6E P2: this document defines a local dry-run checklist.

All three documents are planning artifacts only. None enables real signing.

## 13. Future Phase Suggestions

- Phase 6E P3: signed installer validation checklist.
- Phase 6E P4: public prerelease readiness review covering security, license obligations, support scope, and release process.

Before buying or configuring a real certificate, a human maintainer must confirm:

- Supplier / CA.
- Certificate type.
- Legal identity.
- Storage method.
- Signing operator.
- Whether CI signing is allowed.
