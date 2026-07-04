# RikkaDesk Tauri Windows Signing Config Draft

Current stable tag: `rikkadesk-v0.1.0-beta.5`

This document is a placeholder-level draft for future Windows code signing work. It does not enable signing, does not modify `web-ui/src-tauri/tauri.conf.json`, and must not contain real certificate paths, passwords, token PINs, private keys, PFX files, or production certificate thumbprints.

## 1. Purpose

This document records a possible future direction for Tauri Windows code signing configuration.

Current status:

- RikkaDesk remains an unsigned private beta.
- The current build pipeline should continue to produce unsigned artifacts.
- No real signing certificate is configured.
- No Tauri configuration file is changed in this phase.
- No workaround for Windows SmartScreen, Smart App Control, or enterprise security policy is documented here.

The goal is to make a later implementation phase safer by documenting placeholders, decision points, and verification steps before any real signing credentials exist.

## 2. Current Build Artifacts

Current Windows build outputs:

```text
web-ui/src-tauri/target/release/rikkadesk.exe
web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

Future signing work should decide whether all three artifacts are signed by Tauri, by a post-build script, or by a controlled manual process.

## 3. Tauri v2 Windows Signing Configuration Points

Future work may need to inspect the `bundle.windows` section in `web-ui/src-tauri/tauri.conf.json`.

Potential signing-related configuration points include, subject to final Tauri v2 documentation review:

- Certificate thumbprint.
- Digest algorithm, usually SHA-256 for modern Windows signing.
- Timestamp URL from the certificate authority or trusted timestamp provider.
- Custom sign command, if the selected certificate/token/cloud signing method cannot use the default SignTool path.

Important: exact field names and support details must be rechecked against the current Tauri v2 documentation immediately before implementation. Certificate issuer documentation may override generic examples, especially for EV, post-2023 OV, hardware token, HSM, Azure Key Vault, or cloud signing workflows.

## 4. Placeholder Configuration Examples

The examples below are intentionally incomplete and must not be copied into the real config with live values during this phase.

### Conceptual `bundle.windows` example

```json
{
  "bundle": {
    "windows": {
      "certificateThumbprint": "<CERTIFICATE_THUMBPRINT>",
      "digestAlgorithm": "sha256",
      "timestampUrl": "<TIMESTAMP_URL>"
    }
  }
}
```

### Conceptual custom sign command example

Use a custom sign command only if the certificate storage method requires it and Tauri's default signing path is not enough.

```json
{
  "bundle": {
    "windows": {
      "signCommand": "<CUSTOM_SIGN_COMMAND>"
    }
  }
}
```

The custom command must not expose:

- Real certificate paths.
- PFX passwords.
- Token PINs.
- Cloud signing credentials.
- Private keys.
- Production certificate thumbprints that should remain private to the release pipeline.

## 5. Why Not Write This Into `tauri.conf.json` Now?

Do not modify `web-ui/src-tauri/tauri.conf.json` yet because:

- RikkaDesk does not currently have a selected signing certificate.
- OV, EV, IV, token, HSM, and cloud signing providers can require different tools and configuration.
- Incorrect signing config can break normal unsigned beta builds.
- The certificate type, storage method, timestamp URL, and signing command must be chosen first.
- A placeholder accidentally committed to production config may create confusion or a false sense that signing is active.

## 6. Manual Signing vs Tauri Automatic Signing

### Manual signing

Manual signing means building unsigned artifacts first, then signing each output explicitly:

- Sign `web-ui/src-tauri/target/release/rikkadesk.exe`.
- Sign `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`.
- Sign `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`.

This is easier to reason about during early validation because each artifact can be inspected before and after signing.

### Tauri automatic signing

Tauri automatic signing integrates signing into the packaging flow through configuration or a custom signing command.

Potential benefits:

- Fewer manual steps once the process is stable.
- Better repeatability for release builds.
- Easier future CI integration if signing credentials are safely managed.

Potential risks:

- More sensitive to certificate-provider differences.
- Easier to accidentally break unsigned developer builds.
- CI integration can expose high-value signing credentials if not designed carefully.

Current recommendation: design and test a local manual signing dry-run first, then decide whether to integrate automatic signing.

## 7. Verification Checklist

For any future signed build, verify all signed artifacts.

Signature checks:

```powershell
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/rikkadesk.exe"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
Get-AuthenticodeSignature "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"

signtool verify /pa /v "web-ui/src-tauri/target/release/rikkadesk.exe"
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi"
signtool verify /pa /v "web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe"
```

Installer checks:

- Install the NSIS setup `.exe`.
- Confirm the publisher shown by Windows matches the intended certificate subject.
- Start RikkaDesk.
- Confirm the About dialog and core beta flow still work.
- Observe SmartScreen and Smart App Control behavior on an appropriate test machine.
- Uninstall RikkaDesk.
- Confirm whether app data remains, matching the documented beta behavior.

Do not change Windows security settings merely to pass this checklist.

## 8. Must Confirm Before Implementation

Before any real signing implementation:

- Certificate type: OV, EV, or IV.
- Legal identity: organization, sole proprietor, or individual.
- Certificate storage: hardware token, HSM, cloud signing, Azure Key Vault, or CA-specific service.
- Timestamp URL.
- Whether `rikkadesk.exe`, MSI, and NSIS artifacts all need signing in the same workflow.
- Whether CI signing is allowed, and under what branch protection / approval rules.
- Whether the first implementation should be manual local signing or Tauri automatic signing.
- Whether release checklist, tester notes, and README need updates.
- Whether upstream license / AGPL, support scope, and public release policy are ready.

## 9. Recommended Next Step

Recommended next phase:

- Phase 6E P2: design a local signing dry-run checklist.

Until a certificate type and storage method are chosen:

- Do not change `web-ui/src-tauri/tauri.conf.json`.
- Do not add signing secrets to the repository.
- Do not create a public Release.
- Do not ask testers to bypass Smart App Control, SmartScreen, or enterprise security controls.

Before any public Release:

- Complete signing strategy review.
- Complete license / AGPL obligation review.
- Complete support-scope review.
- Complete signed installer validation on a clean Windows test machine.
