# RikkaDesk Phase 11 Release Hardening Plan

Phase 11 focuses on installer, signing, and release hardening for the RikkaDesk private beta line.

This document is the Phase 11 P0 audit and plan. It does not implement signing, does not buy or install a certificate, does not create a GitHub Release, and does not change the Tauri, Cargo, schema, provider import/export, Android, or SecretStore code paths.

## Current Status

- Branch baseline: `beta/0.1.0` after PR #16 was merged with a merge commit.
- Current stable self-test tag: `rikkadesk-v0.1.0-beta.14`.
- GitHub Release: not created.
- App/package version: `0.1.0`.
- `schemaVersion`: `6`.
- Provider import/export version: `4`.
- Windows packages: generated locally, unsigned.
- Real-provider image input: not enabled by default.
- Loopback-only synthetic image capture remains the only image-send prototype.
- Private beta only; not a public release.

## P0 Scope

P0 is audit and planning only.

P0 does not:

- Sign `rikkadesk.exe`, NSIS, or MSI artifacts.
- Add or configure signing certificates.
- Add CI signing.
- Create a GitHub Release.
- Move, delete, or create beta tags.
- Change app/package version.
- Change installer configuration.
- Change schemaVersion or provider import/export version.
- Change Android app files.
- Touch real API keys, SecretStore blobs, or `mock-api/secrets/*.bin`.

## Windows Bundle And Installer Audit

Audited files:

- `web-ui/src-tauri/tauri.conf.json`
- `web-ui/src-tauri/Cargo.toml`

Observed Tauri configuration:

| Item | Current Value | P0 Notes |
|---|---|---|
| Product name | `RikkaDesk` | Stable product name for private beta. |
| Identifier | `com.cisyamx.rikkadesk` | Stable desktop app identifier. |
| Tauri app version | `0.1.0` | Keep unchanged in P0. |
| Cargo package version | `0.1.0` | Keep unchanged in P0. |
| Window title | `RikkaDesk` | No Phase 11 change. |
| Bundle active | `true` | Desktop bundle generation is enabled. |
| Bundle targets | `all` | Current Windows build emits MSI and NSIS artifacts when the target toolchain is available. |
| Icons | `icons/32x32.png`, `icons/128x128.png`, `icons/128x128@2x.png`, `icons/icon.ico` | Existing icon set is configured. |
| Short description | `RikkaDesk desktop shell` | Could be polished in P1 if needed. |
| Long description | `An unofficial desktop derivative / experiment based on RikkaHub.` | Correct private-beta positioning. |
| Publisher / manufacturer metadata | Not explicitly configured in `tauri.conf.json` | P1 should decide whether to add formal publisher metadata before signing. |
| Signing config | Not configured | Expected for P0. |

Observed artifact paths:

- Executable: `web-ui/src-tauri/target/release/rikkadesk.exe`
- MSI: `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- NSIS: `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

Installer behavior notes:

- NSIS is the primary private beta smoke-test installer.
- MSI is retained as an alternate installer artifact.
- Install and uninstall behavior currently use Tauri defaults unless overridden by the bundler.
- Existing private beta checklist states that uninstall may leave app data and encrypted local secret blobs in the app data directory. That should remain explicit until an app-data retention/removal policy is designed.

Potential future metadata polish:

- Confirm publisher/manufacturer metadata before public release.
- Confirm installed app display name and uninstall entry text on Windows.
- Confirm whether installer filenames should include beta tag labels outside the internal app version.
- Avoid hardcoding beta tag copy in code paths that should eventually be generated from release metadata.

## Current Release Documentation Audit

Audited files:

- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-release-draft.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `NOTICE.md`
- `LICENSE`

Current documentation is broadly consistent on these points:

- RikkaDesk is a private beta / prototype, not a public production release.
- Current private beta baseline is `rikkadesk-v0.1.0-beta.13`.
- Current private hotfix tag is `rikkadesk-v0.1.0-beta.14`.
- GitHub Release has not been created for the private beta tag.
- Windows installers and `rikkadesk.exe` are unsigned.
- Public release requires signing, support scope, and license-obligation review.
- Real-provider image input is not enabled by default.
- `LICENSE` is present and states AGPL v3 / commercial-license boundaries.
- `NOTICE.md` is present and states RikkaDesk is an unofficial derivative of RikkaHub.

Documentation gaps to consider in later phases:

- A dedicated signing checklist does not yet exist outside this Phase 11 plan.
- SHA256 handoff is manual and not yet standardized as a committed `SHA256SUMS` artifact.
- Public release readiness is described in several places but not yet represented as a single approval gate.
- Installer metadata and signing requirements are not yet tied to CI.

## Why Not Sign Immediately

Code signing should not be added in P0 because:

- No certificate material has been selected, procured, or approved.
- Certificate ownership and publisher identity need a project decision.
- Local signing and CI signing have different key custody and audit requirements.
- Signing secrets must never be committed to the repository.
- CI secrets must never be printed in logs.
- Timestamping, revocation, and certificate renewal need design before implementation.
- Unsigned package behavior is already documented for private beta testers.

The private beta can continue to use unsigned installers while Phase 11 designs the release-quality path.

## Signing Scope

Objects that need signing before a public release:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`

Signing order needs validation in P2/P3:

- Sign the executable before packaging, so installed app binaries carry a publisher signature.
- Sign the NSIS installer after packaging.
- Sign the MSI after packaging.
- Verify signatures after signing and after installer smoke tests.

## Local Signing Vs CI Signing

| Topic | Local Signing | CI Signing |
|---|---|---|
| Key custody | Certificate material stays on a trusted local signing machine. | Certificate or signing service credentials are available to CI. |
| Repeatability | Manual unless scripted. | Better repeatability and artifact traceability. |
| Secret exposure risk | Lower CI exposure, higher local-machine dependency. | Requires strict CI secret handling and log review. |
| Audit trail | Manual notes unless standardized. | CI run logs can provide an audit trail if secrets are masked. |
| Best use | Early private smoke if a certificate already exists. | Release-quality signed artifacts after workflow design. |

Rules for either path:

- Certificate files, private key material, `.pfx`, `.p12`, and signing passphrases must not enter the repository.
- Signing commands must not echo secrets.
- CI must mask secret values.
- Logs must not include certificate contents, private key contents, or signing secret values.
- Signed artifacts should be verified with an explicit signature-verification command before distribution.

## Timestamping

Timestamping should be researched before public release.

Open questions:

- Which timestamp server should be used.
- Whether the chosen signing tool supports RFC 3161 timestamping.
- How timestamp failures should affect the release workflow.
- Whether local and CI signing should use the same timestamp policy.

Until this is decided, P0 should not add signing commands.

## SmartScreen And Smart App Control

Current private beta packages are unsigned. As a result:

- Windows SmartScreen warnings are expected.
- Windows 11 Smart App Control may block unsigned executables before the app starts.
- This should be treated as an unsigned-publisher policy block, not a runtime crash.
- RikkaDesk should not ask testers to disable Windows security features.
- RikkaDesk should not ask testers to bypass enterprise policy.

The release-quality fix is code signing plus a clear release/distribution policy.

## Release Readiness Checklist

Use this checklist before any private handoff or public release decision.

### Repository State

- [ ] Working tree is clean.
- [ ] Current branch is the intended release or phase branch.
- [ ] `origin` points to `Cisyam-x/RikkaDesk`.
- [ ] Upstream remote is understood before any upstream sync work.
- [ ] Existing beta tags are not moved, deleted, or overwritten.
- [ ] No public GitHub Release is created unless explicitly approved.

### License And Notice

- [ ] `LICENSE` exists.
- [ ] `NOTICE.md` exists.
- [ ] RikkaDesk is described as an unofficial derivative.
- [ ] AGPL obligations are acknowledged.
- [ ] Upstream commercial-license boundary is acknowledged.
- [ ] Public distribution scope is reviewed before publishing.

### Build And Verification

- [ ] `pnpm --dir web-ui run typecheck`
- [ ] `cargo check --manifest-path web-ui/src-tauri/Cargo.toml`
- [ ] `pnpm --dir web-ui run desktop:build`
- [ ] Confirm executable artifact exists.
- [ ] Confirm MSI artifact exists.
- [ ] Confirm NSIS artifact exists.
- [ ] Confirm only known sourcemap/chunk warnings appear.

### Artifact Paths

- [ ] `web-ui/src-tauri/target/release/rikkadesk.exe`
- [ ] `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- [ ] `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

### Hashes

- [ ] Generate SHA256 for `rikkadesk.exe`.
- [ ] Generate SHA256 for the MSI.
- [ ] Generate SHA256 for the NSIS installer.
- [ ] Store hashes in private handoff notes or a reviewed `SHA256SUMS` artifact.
- [ ] Do not include secrets, local user paths, or private app data in hash notes.

### Installer Smoke

- [ ] Install with NSIS on a test machine.
- [ ] Launch RikkaDesk from the installer finish screen or Start Menu.
- [ ] Confirm About dialog release copy.
- [ ] Confirm basic text chat behavior.
- [ ] Confirm Provider Settings behavior.
- [ ] Confirm local attachment smoke if the release includes Phase 10 behavior.
- [ ] Confirm uninstall entry exists.
- [ ] Uninstall the app.
- [ ] Document whether app data is retained after uninstall.
- [ ] If app data is retained, document manual cleanup instructions for private testers.
- [ ] Confirm manual app data cleanup starts only after RikkaDesk is closed.
- [ ] Confirm no `RikkaDesk` process is running before deleting `%APPDATA%\com.cisyamx.rikkadesk`.
- [ ] Confirm cleanup docs warn that deleting app data removes local provider keys.
- [ ] Confirm `mock-api/secrets/*.bin` and app data backups are never shared, uploaded, sent to Codex / ChatGPT, or copied into the repository.

### Security Search

- [ ] Search for plaintext API key patterns.
- [ ] Search for provider secret fields in exported artifacts.
- [ ] Search for `mock-api/secrets`.
- [ ] Search for `.bin` secret blob references.
- [ ] Search for signing certificate or private key material.
- [ ] Confirm no real app data is copied into the repository.
- [ ] Confirm no real-provider image input is enabled by default.

### Release Approval

- [ ] Confirm whether the release is private beta only.
- [ ] Confirm whether a public GitHub Release is explicitly approved.
- [ ] Confirm installer signing status.
- [ ] Confirm support scope.
- [ ] Confirm known limitations.
- [ ] Confirm tester handoff wording.

## Version Strategy

Current strategy:

- Keep Tauri app version at `0.1.0`.
- Keep Cargo package version at `0.1.0`.
- Express beta milestones through Git tags and release notes.
- Do not modify Tauri/Cargo versions in P0.

Rationale:

- The Windows installer artifact names already use `0.1.0`.
- Private beta tags provide enough granularity for current testing.
- Some Windows bundling tools may have constraints around prerelease semver labels.
- Changing the internal app version should be tested independently from signing.

Future decision:

- Evaluate whether `0.1.0-beta.x` is accepted by the Tauri Windows bundle pipeline.
- If prerelease labels cause installer issues, keep internal version numeric and place beta labels in tag names, artifact notes, and release metadata.

## Auto-Update Strategy

Current status:

- No auto-update channel is configured.
- No public release channel exists.
- No update signing workflow exists.

P0 recommendation:

- Do not enable auto-update before public release hardening.
- Treat updater design as an independent later phase.
- Updater work should be coupled with signing, release channel, rollback, and support-scope decisions.

Future updater questions:

- Which channel names exist: private, beta, stable.
- Which update server or GitHub Release feed is used.
- How update signatures are generated and verified.
- How unsigned private beta builds are prevented from auto-updating into public builds.
- How downgrade and rollback are handled.

## Blockers

Public release blockers:

- No code signing certificate or signing service has been selected.
- No signing workflow exists for `rikkadesk.exe`, NSIS, or MSI.
- Installer publisher/manufacturer metadata has not been finalized.
- SHA256 handoff is manual.
- No single release approval gate exists.
- No public support scope has been approved.
- License and upstream commercial-use boundaries need explicit public-release review.
- Auto-update is not designed or enabled.

Private beta blockers:

- No blocking issue for continued private beta testing if unsigned-package warnings are accepted and documented.

## Recommended Phase 11 Split

### P0: Audit / Plan

Goal:

- Capture current installer, signing, and release status.
- Define signing/release hardening requirements.

Files:

- Create `docs/rikkadesk-phase-11-release-hardening-plan.md`.

Validation:

- `git diff --check`
- `pnpm --dir web-ui run typecheck`
- `cargo check --manifest-path web-ui/src-tauri/Cargo.toml`

### P1: Installer Metadata Polish

Goal:

- Review and, if approved, polish installer metadata without signing.

Candidate files:

- `web-ui/src-tauri/tauri.conf.json`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-release-draft.md`

Do not:

- Add certificate material.
- Change schemaVersion.
- Change provider import/export version.
- Create Release or tags.

Validation:

- Desktop build.
- Artifact name and metadata smoke.
- Install/uninstall smoke.

### P2: Signing Workflow Design

Goal:

- Decide local signing vs CI signing.
- Document exact signing commands and secret handling.
- Use [rikkadesk-phase-11-signing-workflow-design.md](rikkadesk-phase-11-signing-workflow-design.md) as the P2 signing runbook.

Candidate files:

- `docs/rikkadesk-phase-11-release-hardening-plan.md`
- Optional dedicated signing runbook.

Do not:

- Commit certificate files.
- Commit private key material.
- Print signing secrets.

Validation:

- Review-only until certificate material exists.

### P3: Local Signing Smoke

Goal:

- Only if certificate/material exists, sign a local test artifact.

Current decision:

- P3 signing smoke is deferred for the current private beta.
- The private beta will continue with unsigned artifacts while distribution remains small-scope and private.
- Signing is required before public release, broad distribution, enterprise use, or any workflow that asks testers to trust RikkaDesk as a published Windows app.
- Do not enter P3 unless certificate material already exists or a public/broad distribution requirement is approved.

Free or lower-cost options to research later:

- Microsoft Store MSIX distribution path.
- SignPath Foundation for qualifying open-source projects.
- Self-signed certificate only for local development, managed machines, or explicitly controlled test environments; self-signed certificates are not suitable for public distribution trust.

Preconditions:

- Certificate ownership approved.
- Secret handling approved.
- Signing command reviewed.
- Timestamp policy decided or explicitly deferred.

Validation:

- Verify executable signature.
- Verify NSIS installer signature.
- Verify MSI signature.
- Install/uninstall smoke.
- Confirm no signing material entered the repository.

### P4: Release Checklist / Artifact Handoff

Goal:

- Standardize release candidate handoff notes and artifact hashes.

Candidate files:

- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-release-draft.md`
- Optional `SHA256SUMS` generated only after approval.

Validation:

- Build.
- Hashes.
- Installer smoke.
- Secret search.

### P5: Optional Updater Design

Goal:

- Design updater channel, signing, rollout, and rollback strategy.

Do not:

- Enable updater before signing and public release policy are approved.

Validation:

- Design review first; implementation later.

## P0 Conclusion

RikkaDesk can continue private beta testing with unsigned installers because the limitations are documented and beta tags are used for traceability. Public release is not ready until signing, support scope, artifact handoff, and license-obligation review are completed.
