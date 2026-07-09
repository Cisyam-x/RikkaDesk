# RikkaDesk Phase 11 P2 Signing Workflow Design

This document designs a future Windows code signing workflow for RikkaDesk. It is a runbook and security plan only.

P2 does not sign artifacts, does not buy or configure a certificate, does not add a GitHub Actions workflow with signing secrets, does not create a GitHub Release, and does not create or move beta tags.

## Current Status

- Current stable self-test tag: `rikkadesk-v0.1.0-beta.14`.
- Current Windows artifacts are unsigned.
- GitHub Release: not created.
- App/package version: `0.1.0`.
- `schemaVersion`: `6`.
- Provider import/export version: `4`.
- Real-provider image input: not enabled by default.

Public release blockers:

- No signing certificate has been selected or approved.
- No signing workflow exists.
- Publisher/manufacturer identity is not confirmed.
- Timestamping policy is not confirmed.
- Release approval and support scope are not finalized.

## Signing Objects

Future signing work must cover these artifacts:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`

## Recommended Signing Order

Recommended release-quality order:

1. Build unsigned `rikkadesk.exe`.
2. Sign `rikkadesk.exe`.
3. Package NSIS and MSI using the signed executable.
4. Sign the NSIS installer.
5. Sign the MSI.
6. Verify all signatures.
7. Generate SHA256 hashes after signing.
8. Run install/uninstall smoke tests.

Important:

- Signing changes file bytes.
- SHA256 hashes must be generated after final signing.
- If hashes are generated before signing, those hashes become invalid after signing.
- Installer smoke must verify both the installer signature and the installed `rikkadesk.exe` signature.

## Local Signing Workflow Design

Local signing is appropriate for early manual smoke testing only when approved certificate material already exists.

Local signing rules:

- Certificate material stays on a trusted local signing machine.
- Certificate material does not enter the repository.
- Certificate material does not enter logs.
- Certificate material does not enter screenshots.
- Certificate material is not sent to Codex / ChatGPT.
- Certificate material is not placed in CI.
- Signing passwords are not written into scripts.
- Signing passwords are not committed.
- Signing commands with real paths/passwords should not be pasted into docs or issue comments.

Placeholder command pattern:

```powershell
signtool sign /fd SHA256 /tr <TIMESTAMP_URL> /td SHA256 /f <CERTIFICATE_PFX_PATH> /p <PFX_PASSWORD> <ARTIFACT_PATH>
```

Required placeholders:

- `<CERTIFICATE_PFX_PATH>` is provided manually on the local signing machine.
- `<PFX_PASSWORD>` is provided manually by the signing operator.
- `<TIMESTAMP_URL>` is provided manually after timestamp policy is approved.
- `<ARTIFACT_PATH>` is one of the built artifacts.

Do not:

- Commit the command with real values.
- Store the real command in repository scripts.
- Store the PFX password in shell history, CI logs, screenshots, or chat transcripts.
- Read, print, or parse unrelated local secrets such as `mock-api/secrets/*.bin`.

Example artifact sequence with placeholders:

```powershell
signtool sign /fd SHA256 /tr <TIMESTAMP_URL> /td SHA256 /f <CERTIFICATE_PFX_PATH> /p <PFX_PASSWORD> <PATH_TO_RIKKADESK_EXE>
signtool sign /fd SHA256 /tr <TIMESTAMP_URL> /td SHA256 /f <CERTIFICATE_PFX_PATH> /p <PFX_PASSWORD> <PATH_TO_NSIS_INSTALLER>
signtool sign /fd SHA256 /tr <TIMESTAMP_URL> /td SHA256 /f <CERTIFICATE_PFX_PATH> /p <PFX_PASSWORD> <PATH_TO_MSI_INSTALLER>
```

## CI Signing Workflow Design

CI signing is not implemented in P2. A future CI signing design must decide the key custody model first.

Possible CI signing models:

| Model | Summary | P2 Recommendation |
|---|---|---|
| GitHub Actions secret containing long-lived PFX | Store PFX and password as CI secrets. | Avoid unless explicitly approved; high-value long-lived secret in CI. |
| Cloud signing service | CI requests signing through a managed service. | Prefer for release-quality CI if available. |
| HSM-backed signing | Key never leaves hardware/security boundary. | Strong option if available. |
| Hardware token on local machine | Manual signing with a physical token. | Better for local smoke than fully automated CI. |
| Windows certificate store on trusted runner | Certificate installed outside repository and selected by thumbprint. | Possible for self-hosted runner; needs strict runner control. |

CI signing rules:

- CI logs must mask secrets.
- Workflows must not echo secrets.
- Pull requests from forks must not have access to signing secrets.
- Signing jobs should run only on protected branches, protected tags, or manual approval workflows.
- Signing jobs should require human approval for public release artifacts.
- Public GitHub Release creation must be a separate explicit approval step.
- Workflow artifacts must not include certificate material, secret dumps, command history with passwords, or signing service tokens.
- CI should verify the final signatures and fail if any expected signature is missing or invalid.

CI branch/tag policy recommendation:

- Private phase branches: no signing.
- `beta/0.1.0`: optional unsigned build verification only.
- Protected beta tags: future signing can be considered after approval.
- Public release tags: signing and release approval required.

## Timestamping Policy

Timestamping is required for release-quality signing, but P2 does not choose a timestamp provider.

P3/P4 must decide:

- Which timestamp server is approved.
- Whether RFC 3161 timestamping is required.
- Whether local and CI signing use the same timestamp server.
- Whether timestamp failure blocks release.
- Whether retry logic is allowed.
- How timestamp evidence is recorded in handoff notes.

Recommended failure rule:

- For public release, timestamp failure should block release.
- For private local smoke, timestamp failure may be recorded as a known limitation only if the artifact is clearly not public.

## Signature Verification

Placeholder verification commands:

```powershell
Get-AuthenticodeSignature .\rikkadesk.exe
Get-AuthenticodeSignature .\RikkaDesk_0.1.0_x64-setup.exe
Get-AuthenticodeSignature .\RikkaDesk_0.1.0_x64_en-US.msi
```

Expected release-quality result:

- `Status` is `Valid`.
- `SignerCertificate.Subject` matches the approved publisher/manufacturer identity.
- Timestamp information is present and recorded.
- The installed `rikkadesk.exe` is also signed and valid after installation.

Suggested verification checklist:

- [ ] Verify `rikkadesk.exe` before packaging if signing happens pre-package.
- [ ] Verify NSIS installer after signing.
- [ ] Verify MSI after signing.
- [ ] Install NSIS package.
- [ ] Verify installed `rikkadesk.exe`.
- [ ] Uninstall app.
- [ ] Confirm app data retention/cleanup behavior is documented.
- [ ] Generate SHA256 hashes only after all signing steps are complete.

## Failure Handling

| Failure | Expected Handling |
|---|---|
| `signtool` is not installed | Stop. Install or locate the approved Windows SDK signing tool on a trusted machine. |
| Certificate is unavailable | Stop. Do not substitute an unapproved certificate. |
| PFX password is wrong | Stop. Do not log or paste the password while troubleshooting. |
| Timestamp server fails | For public release, stop. For private smoke, record explicitly if timestamping is deferred. |
| `rikkadesk.exe` is signed but installer is unsigned | Not release-ready. Sign installers or label artifacts as private unsigned smoke only. |
| Installer is signed but installed exe is unsigned | Not release-ready. Fix signing order so the executable inside the installer is signed. |
| Hash was generated before signing | Regenerate hashes after signing. |
| SmartScreen still warns | Record the result. Signing helps trust but does not guarantee immediate SmartScreen reputation. |
| Smart App Control still blocks | Record the result. Do not ask testers to disable Smart App Control. |
| Enterprise policy blocks execution | Record the policy block. Do not ask testers to bypass enterprise policy. |
| CI secret appears in logs | Treat as a security incident. Revoke/rotate affected material and remove logs if possible. |

Do not ask testers to disable:

- SmartScreen
- Smart App Control
- Microsoft Defender
- Enterprise endpoint protection
- Enterprise execution policy

The correct response is to record the result and improve signing/release process.

## P2 Explicit Non-Goals

P2 does not:

- Sign any artifact.
- Buy or request a certificate.
- Add real GitHub Actions signing workflow.
- Commit `.pfx`, `.p12`, certificate, or private key material.
- Commit signing passwords.
- Commit CI tokens or signing service tokens.
- Set publisher/manufacturer metadata.
- Create a GitHub Release.
- Create or move tags.
- Change Tauri/Cargo signing configuration.
- Change `schemaVersion`.
- Change provider import/export version.
- Enable real-provider image input.

## P3 Entry Conditions

Do not enter P3 local signing smoke until all of these are true:

- Human confirms the signing certificate subject.
- Human confirms certificate type: OV, EV, IV, or test-only.
- Human confirms key custody model: local machine, hardware token, HSM, cloud signing, or approved certificate store.
- Human confirms whether CI signing is allowed.
- Timestamp policy is decided or explicitly deferred for private smoke only.
- Signing operator is identified.
- A dry-run checklist with no real secrets has passed.
- Publisher/manufacturer metadata decision is either aligned with signing identity or explicitly deferred.
- No certificate, private key, password, token, or CI secret is present in the repository.

## Related Documents

- [Phase 11 release hardening plan](rikkadesk-phase-11-release-hardening-plan.md)
- [Phase 11 installer metadata review](rikkadesk-phase-11-installer-metadata-review.md)
- [Beta package checklist](rikkadesk-beta-package-checklist.md)

P2 conclusion:

- Signing workflow design is ready for review.
- Actual signing must wait for human approval of certificate subject, certificate type, key custody, timestamp policy, and signing operator.
- RikkaDesk should continue private beta distribution as unsigned until P3/P4 signing validation is explicitly approved.

## Current Decision: Defer P3

RikkaDesk should not enter P3 signing smoke now.

Reasoning:

- Current distribution is small-scope private beta / self-test only.
- No GitHub Release is being created.
- No public broad distribution is planned for this checkpoint.
- Certificate cost and custody overhead are not justified for the current scope.
- Unsigned installer risk is already documented in the private beta docs.
- P3 should be entered only when public release, broad distribution, enterprise use, or existing approved certificate material makes signing necessary.

Current path:

- Continue with unsigned private beta artifacts.
- Keep documenting SmartScreen / Smart App Control risk.
- Do not ask testers to disable Windows security features.
- Keep beta tags and private handoff notes as the traceability mechanism.

Next recommended phase:

- Phase 11 P4 unsigned artifact handoff / SHA256SUMS / release checklist.
- P4 should standardize artifact paths, post-build hashes, installer smoke records, app data cleanup notes, and no-Release private handoff wording.
