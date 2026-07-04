# RikkaDesk Windows Code Signing Research

Review date: 2026-07-04

Current stable tag: `rikkadesk-v0.1.0-beta.5`

This document is research only. It does not configure signing, purchase a certificate, change Tauri settings, create a Release, create a tag, or provide instructions to bypass Windows or enterprise security policy.

## 1. Problem Background

RikkaDesk private beta currently produces unsigned Windows artifacts:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

For the current private beta, Windows SmartScreen or "unknown publisher" warnings are expected. On Windows 11, Smart App Control can be stricter: Microsoft documents that if its cloud service cannot confidently predict an app is safe, it checks whether the app has a valid signature; unsigned or invalidly signed apps can be treated as untrusted and blocked. Microsoft also states there is no per-app bypass for Smart App Control; the better path is to sign the app with a valid signature.

The long-term solution is Windows code signing for the executable and installer artifacts. RikkaDesk should not ask testers to disable Smart App Control, SmartScreen, Microsoft Defender, or enterprise application-control policy just to run a private beta build.

## 2. Windows Code Signing Basics

### Authenticode

Microsoft Authenticode signing attaches a digital signature to Windows binaries and installers. The signature identifies the publisher and allows Windows and users to detect whether the file has been modified after signing.

Relevant Microsoft tools and checks:

- `signtool sign` signs files.
- `signtool verify` verifies signatures.
- `Get-AuthenticodeSignature` checks signature status from PowerShell.

Microsoft SignTool is installed with Visual Studio / Windows SDK tooling and can sign, verify, and timestamp files.

### What needs signing?

For RikkaDesk, the signing strategy should treat each distributed artifact as important:

- `rikkadesk.exe`: should be signed because Smart App Control may directly evaluate the installed executable before launch.
- NSIS setup `.exe`: should be signed because it is the primary downloaded/installed artifact and affects SmartScreen / publisher prompts.
- MSI `.msi`: should be signed when distributed, especially for enterprise-style deployment.

Post-build manual signing can conceptually sign all three artifact types with Authenticode, but the final automated Tauri behavior must be verified with a real certificate or dry-run signing mechanism.

### Timestamping

Authenticode timestamps let a signature remain verifiable after the signing certificate expires, as long as the file was signed while the certificate was valid. The timestamp URL should come from the CA or trusted timestamp authority. Do not omit timestamping for release artifacts.

### What signing can solve

Signing can:

- Replace "Unknown Publisher" with the verified publisher identity.
- Prove the file has not been modified after signing.
- Improve trust signals for SmartScreen and Smart App Control.
- Make distribution more acceptable for testers and enterprise environments.

Signing cannot guarantee:

- That every Windows system will run the app without warnings.
- That SmartScreen reputation is immediate for every certificate type and every new binary.
- That enterprise application-control policy will allow the app.
- That the app is bug-free or free from vulnerabilities.

SmartScreen and Smart App Control can also depend on cloud reputation and policy decisions, so "signed" is necessary for a serious release but may not be sufficient by itself.

## 3. Certificate Types

### OV Code Signing Certificate

OV means Organization Validated. It verifies a legal organization identity and signs binaries with that organization name.

Typical characteristics:

- Usually requires a legal organization or business identity.
- Shows a verified publisher instead of "Unknown Publisher".
- Since CA/B Forum private-key protection changes, private keys generally need secure storage such as a hardware token, HSM, or approved cloud signing service.
- SmartScreen reputation may still need to build over time depending on CA, distribution volume, and Microsoft reputation systems.

Current rough cost expectation: usually hundreds of USD per year, sometimes more depending on CA, reseller, hardware token, HSM, or cloud-signing subscription. Prices change often and must be rechecked before purchase.

### EV Code Signing Certificate

EV means Extended Validation. It requires stricter identity validation than OV.

Typical characteristics:

- More rigorous vetting.
- Often associated with hardware token, HSM, or cloud signing.
- Historically marketed as better for Microsoft SmartScreen reputation, but reputation behavior can change. Some CA materials still describe SmartScreen advantages, while other vendor notes indicate modern SmartScreen reputation may still build over time. Treat immediate reputation as a point requiring confirmation with the chosen CA and current Microsoft behavior.
- Required for some Windows driver signing scenarios, but RikkaDesk is not a driver.

Current rough cost expectation: typically higher than OV, often several hundred to over one thousand USD per year depending on provider, validation type, token/HSM/cloud signing, and support plan. Recheck pricing before purchase.

### Individual / Sole Proprietor Options

Some providers offer Individual Validation (IV) or sole proprietor EV-style options. This may be relevant if RikkaDesk is distributed by an individual rather than a registered organization.

Tradeoffs:

- May display a personal legal name rather than an organization name.
- Availability and validation requirements differ by CA and jurisdiction.
- SmartScreen and Smart App Control behavior still needs confirmation.

### Hardware Token, HSM, and Cloud Signing

Modern public code signing generally expects protected private-key storage. Options include:

- CA-provided hardware token.
- Customer-owned approved hardware token or HSM.
- CA cloud signing / HSM service.
- Azure Key Vault or similar cloud-backed signing where supported.

Do not store a private key, PFX password, token PIN, or signing credential in the repository.

## 4. Supplier / CA Research

Only use reputable certificate authorities or established resellers. Do not use unclear, shared, rented, or "cheap unknown" certificates.

Examples worth comparing later:

- DigiCert: code signing with Organization Validated and Extended Validation options, KeyLocker cloud storage, hardware token, customer HSM, and published pricing that is explicitly subject to change.
- Sectigo: OV and EV code signing options; documentation highlights hardware token/PIN protection and Authenticode support for files including `.exe` and `.msi`.
- SSL.com: IV, OV, EV, sole proprietor options, and eSigner cloud signing that can integrate with CI/CD.
- GlobalSign: code signing offerings; current shop notes organization-only issuance for its listed code signing certificates and one-year certificate validity.

Purchasing is out of scope for Phase 6E P0. A future phase should compare exact legal identity requirements, total annual cost, token/cloud signing costs, renewal process, support quality, and CI integration.

## 5. Tauri v2 Signing Support

Tauri v2 has Windows code signing documentation. The docs show Windows bundle signing configuration under `bundle.windows`, including fields such as:

- `certificateThumbprint`
- `digestAlgorithm`
- `timestampUrl`

The Tauri Windows installer documentation also points signing work to the signing documentation and notes that cross-compiled Windows installer signing requires an external signing tool.

Important caveats for RikkaDesk:

- The Tauri page includes a warning that part of the OV certificate guide only applies to OV certificates acquired before 2023-06-01. For newer OV certificates, EV certificates, hardware tokens, or cloud signing, follow the CA documentation and/or Tauri custom signing command path.
- RikkaDesk currently must not add real certificate fields to `web-ui/src-tauri/tauri.conf.json`.
- Future configuration review should inspect `web-ui/src-tauri/tauri.conf.json` only in a non-secret, placeholder-only draft.
- Actual behavior for signing both MSI and NSIS artifacts should be verified in a dedicated signing dry-run phase.

Likely local prerequisites:

- Windows SDK / Visual Studio Build Tools with `signtool.exe`.
- Access to the certificate through Windows certificate store, token provider, HSM, cloud signing adapter, or custom signing command.
- Timestamp service URL from the CA or chosen timestamp provider.

## 6. Manual Signing Flow Draft

This is conceptual only. It intentionally uses placeholders and must not be copied with real secrets into the repository.

Build artifacts:

```text
web-ui/src-tauri/target/release/rikkadesk.exe
web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

Conceptual SignTool signing:

```powershell
signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL_FROM_CA>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "web-ui/src-tauri/target/release/rikkadesk.exe"

signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL_FROM_CA>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"

signtool sign `
  /fd SHA256 `
  /tr "<TIMESTAMP_URL_FROM_CA>" `
  /td SHA256 `
  /sha1 "<CERTIFICATE_THUMBPRINT>" `
  "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

Conceptual verification:

```powershell
signtool verify /pa /v "web-ui/src-tauri/target/release/rikkadesk.exe"
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"

Get-AuthenticodeSignature "web-ui/src-tauri/target/release/rikkadesk.exe"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

Do not commit:

- Certificate files.
- PFX files.
- Token PINs.
- Certificate passwords.
- Cloud signing credentials.
- Real certificate thumbprints if they identify a private signing setup and are not intended for publication.

## 7. CI/CD Signing Risks

GitHub Actions signing can be convenient but introduces serious secret-management risk:

- A PFX, private key, token PIN, password, or cloud signing credential in repository files is unacceptable.
- GitHub Actions secrets reduce exposure but are still high-value credentials.
- Pull requests from untrusted branches, logs, artifact uploads, and workflow modifications can become exfiltration paths if not carefully controlled.
- Hardware-token workflows may not work well in hosted CI.
- Cloud signing can reduce private-key handling but requires strict access control, audit logs, limited service principals, and least-privilege permissions.

For the private beta stage, manual local signing is safer to evaluate than fully automated CI signing. CI signing should wait until the project has a clear release process, protected branches, restricted workflows, code-owner review, and a chosen certificate storage model.

## 8. Recommended Route

### Current private beta

Keep the current unsigned build process and clear documentation. Continue warning testers that SmartScreen warnings and Smart App Control blocks can happen. Do not ask testers to disable security features.

### Small external test group

Consider OV or EV/IV signing depending on the legal entity:

- If distributing as an organization, compare OV vs EV from DigiCert, Sectigo, SSL.com, and GlobalSign.
- If distributing as an individual, investigate IV or sole-proprietor options from providers that support them.
- Prefer CA-backed cloud signing or hardware-token/HSM storage over exportable private keys.

### Before public release

Do not publish a broad public Release until:

- Windows signing strategy is chosen.
- Support scope is documented.
- Upstream license / AGPL obligations are reviewed.
- Installer trust and uninstall/data-retention behavior are retested.
- A signed installer checklist passes on a clean Windows test machine.

## 9. Future Phase Suggestions

- Phase 6E P1: Draft Tauri signing configuration with placeholders only; do not include real certificate data.
- Phase 6E P2: Design a local signing dry-run using a non-production test certificate or CA-provided process; verify SignTool paths and artifact order.
- Phase 6E P3: Create signed installer validation checklist covering signature status, SmartScreen, Smart App Control behavior, install, launch, uninstall, and app data retention.
- Phase 6E P4: Public prerelease readiness review covering signing, security, license obligations, support scope, and release notes.

## Sources Reviewed

- Microsoft Learn: [SignTool.exe / Sign Tool](https://learn.microsoft.com/en-us/dotnet/framework/tools/signtool-exe)
- Microsoft Learn: [SignTool - Win32 apps](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)
- Microsoft Learn: [Time Stamping Authenticode Signatures](https://learn.microsoft.com/en-us/windows/win32/seccrypto/time-stamping-authenticode-signatures)
- Microsoft Support: [Smart App Control Frequently Asked Questions](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions)
- Microsoft Learn: [Application Control for Windows](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/appcontrol)
- Tauri v2: [Windows Code Signing](https://v2.tauri.app/distribute/sign/windows/)
- Tauri v2: [Windows Installer](https://v2.tauri.app/distribute/windows-installer/)
- Tauri v2: [Configuration Reference](https://v2.tauri.app/reference/config/)
- DigiCert: [Code Signing Certificates](https://www.digicert.com/signing/code-signing-certificates)
- Sectigo: [Code Signing Certificates](https://www.sectigo.com/ssl-certificates-tls/code-signing)
- SSL.com: [Software Integrity Code Signing Solutions](https://www.ssl.com/products/software-integrity/)
- GlobalSign: [Code Signing Certificates](https://shop.globalsign.com/en/code-signing)

## Items Requiring Future Confirmation

- Exact SmartScreen reputation behavior for the chosen OV/EV/IV certificate in 2026.
- Whether the selected CA supports the desired legal identity type for RikkaDesk distribution.
- Exact Tauri v2 signing behavior for both NSIS and MSI artifacts with the chosen certificate storage method.
- Whether a cloud signing service or hardware token best fits the future release pipeline.
- Final cost, renewal, token, and cloud-signing fees from the selected provider.
