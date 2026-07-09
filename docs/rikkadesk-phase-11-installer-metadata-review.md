# RikkaDesk Phase 11 P1-A Installer Metadata Review

This document records the Phase 11 P1-A installer metadata review. It is a proposal only. It does not change `tauri.conf.json`, does not add signing, does not create a GitHub Release, and does not move or create beta tags.

## Scope

Reviewed files:

- `web-ui/src-tauri/tauri.conf.json`
- `web-ui/src-tauri/Cargo.toml`
- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-release-draft.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-phase-11-release-hardening-plan.md`

Out of scope for P1-A:

- Code signing implementation.
- Certificate purchase or configuration.
- `.pfx`, `.p12`, certificate, or private key material.
- Tauri config changes.
- Android app changes.
- Schema or provider import/export changes.
- GitHub Release creation.
- Beta tag changes.

## Current Installer Metadata

| Field | Current Value | Source | Notes |
|---|---|---|---|
| Product name | `RikkaDesk` | `tauri.conf.json` `productName` | Good. This should remain stable. |
| Identifier | `com.cisyamx.rikkadesk` | `tauri.conf.json` `identifier` | Good. This also aligns with app data paths. |
| Tauri version | `0.1.0` | `tauri.conf.json` `version` | Good for current private beta line. |
| Cargo package name | `rikkadesk` | `Cargo.toml` `package.name` | Good. |
| Cargo version | `0.1.0` | `Cargo.toml` `package.version` | Good for current private beta line. |
| Cargo description | `An unofficial desktop derivative / experiment based on RikkaHub.` | `Cargo.toml` | Accurate, but slightly long for installer surfaces. |
| Cargo authors | `Cisyam-x` | `Cargo.toml` | Matches repository ownership style. |
| Cargo license | `AGPL-3.0-or-later` | `Cargo.toml` | Correct and should remain. |
| Window title | `RikkaDesk` | `tauri.conf.json` `app.windows[0].title` | Good. |
| Bundle targets | `all` | `tauri.conf.json` `bundle.targets` | Produces the current Windows MSI and NSIS artifacts in this environment. |
| Icons | `icons/32x32.png`, `icons/128x128.png`, `icons/128x128@2x.png`, `icons/icon.ico` | `tauri.conf.json` `bundle.icon` | Existing icon set is configured. |
| Short description | `RikkaDesk desktop shell` | `tauri.conf.json` `bundle.shortDescription` | Safe but generic. Could be polished later. |
| Long description | `An unofficial desktop derivative / experiment based on RikkaHub.` | `tauri.conf.json` `bundle.longDescription` | Accurate and license-aware. |
| Publisher | Not explicitly set | `bundle.publisher` absent | Tauri schema says this maps to Windows Installer Manufacturer and defaults to the second element in the identifier string if unset. With `com.cisyamx.rikkadesk`, that likely means a derived `cisyamx` value, but it is not explicitly controlled. |
| Manufacturer | Not explicitly set | Windows MSI derived from publisher | No explicit MSI manufacturer value is configured. |
| Homepage | Not set | `bundle.homepage` absent and Cargo has no `homepage` | Could be set later if a public project URL is approved. |
| Copyright | Not set | `bundle.copyright` absent | Should wait for publisher/legal identity confirmation. |
| Bundle license | Not set in Tauri config | `bundle.license` absent | Tauri schema says it defaults to Cargo license when unset. |
| License file | Not set | `bundle.licenseFile` absent | Could be reviewed later, but path behavior should be build-tested before changing. |
| Category | Not set | `bundle.category` absent | Could be `Productivity` or `DeveloperTool`, but no urgent need. |
| Windows NSIS config | Not set | `bundle.windows.nsis` absent | Uses Tauri defaults. |
| Windows MSI config | Not set | `bundle.windows.wix` absent | Uses Tauri defaults. |
| WebView2 install mode | Not set | `bundle.windows.webviewInstallMode` absent | Uses Tauri defaults. |

## Current Artifact Names

Current build outputs:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

Recommendation:

- Keep these artifact names unchanged for now.
- Continue expressing beta identity through Git tags, release notes, checksums, and handoff notes.
- Do not put `beta.14` or future beta labels into installer filenames until the versioning strategy is tested as a separate phase.

## Windows Uninstall Entry Expectations

Based on current Tauri defaults and metadata, Windows uninstall surfaces are expected to show roughly:

- App name: `RikkaDesk`
- Version: `0.1.0`
- Publisher / manufacturer: unset or derived by the bundler, likely from the identifier if no explicit publisher is configured

P1-A does not verify the actual registry entry. P1-B or P4 should verify the installed app entry from Windows Settings after an installer smoke test.

## Metadata That Should Stay Unchanged

Keep these fields unchanged in P1-B unless there is a strong reason:

- `productName = "RikkaDesk"`
- `identifier = "com.cisyamx.rikkadesk"`
- `version = "0.1.0"`
- Cargo package `version = "0.1.0"`
- Window title `RikkaDesk`
- Existing icon configuration
- Current MSI / NSIS artifact filenames

Reasons:

- `RikkaDesk` is already the user-facing product name.
- The identifier is already tied to app data paths and should not churn.
- The internal app/package version is intentionally stable at `0.1.0` for this private beta line.
- Installer filename churn would add release-process complexity without solving signing or trust.

## Metadata That Could Be Polished Later

Low-risk candidates for a later implementation step:

| Field | Candidate Value | Reason |
|---|---|---|
| `bundle.shortDescription` | `Private beta desktop shell for RikkaDesk` | More precise than the current generic text, but only useful if installer surfaces display it. |
| `bundle.longDescription` | `An unofficial private beta desktop derivative / experiment based on RikkaHub.` | Adds private beta positioning while preserving derivative wording. |
| `bundle.category` | `Productivity` or `DeveloperTool` | Either can be justified; choose only if installer/package surfaces use it. |
| `bundle.homepage` | Repository URL, only after public project URL is approved | Avoid public-facing URL claims before release policy is settled. |
| `bundle.licenseFile` | A reviewed relative path to `LICENSE` | Potentially useful, but must be tested because the config file lives under `web-ui/src-tauri`. |

These are not urgent. P1-B can remain docs-only if there is no strong installer UX need.

## Metadata Not Recommended Yet

Do not set these in P1-B without human confirmation:

- `bundle.publisher`
- Windows manufacturer override
- `bundle.copyright`
- custom MSI `upgradeCode`
- custom NSIS installer hooks
- custom installer UI assets
- WebView2 install mode
- beta labels in installer filenames

Reasons:

- Publisher/manufacturer should align with the eventual code-signing certificate subject.
- `Cisyam-x` is appropriate as a repo owner and Cargo author, but may not match a future legal certificate subject.
- Setting a publisher before signing can make the metadata look more official without improving Windows trust.
- MSI `upgradeCode` is sticky and should be chosen carefully before public distribution.
- Custom NSIS hooks increase installer maintenance risk and are unnecessary for current private beta.

## Publisher / Manufacturer Recommendation

Recommendation for now:

- Do not explicitly set publisher/manufacturer in P1-B.
- Keep the current implicit/default behavior until signing identity is confirmed.

If a later phase confirms publisher identity, candidate values are:

- `Cisyam-x` if the signing certificate subject and project ownership are aligned with that name.
- A legal individual or organization name if that is the certificate subject.

Risk:

- If installer metadata says `Cisyam-x` but the signing certificate subject is different, users may see inconsistent publisher information across installer UI, Windows trust prompts, and file properties.
- If no certificate exists, explicit publisher metadata does not solve SmartScreen or Smart App Control warnings.

## Install / Uninstall Documentation Review

Current docs already state:

- NSIS is the normal private beta installer.
- MSI is an alternate installer artifact.
- Windows installers and `rikkadesk.exe` are unsigned.
- Windows SmartScreen and Smart App Control warnings/blocks are expected for unsigned builds.
- Testers should not be asked to disable Windows security features.
- Uninstall may leave user data and encrypted local secret blobs.
- App data root is documented as `$env:APPDATA\com.cisyamx.rikkadesk`.
- Mock API data is documented under `$env:APPDATA\com.cisyamx.rikkadesk\mock-api`.
- `mock-api/state.v1.json` is non-sensitive state.
- `mock-api/secrets/*.bin` stores encrypted secret blobs and must not be shared.

Small doc gap:

- The uninstall/manual cleanup section should explicitly say to close RikkaDesk before manually deleting app data.
- This should be a later docs/checklist polish, not a Tauri metadata change.

## P1-A Recommendations

Recommended keep:

- Product name: `RikkaDesk`
- Identifier: `com.cisyamx.rikkadesk`
- App/package version: `0.1.0`
- Artifact filenames
- Existing icon list
- No explicit publisher/manufacturer until signing identity is decided

Recommended later polish:

- Optionally refine `shortDescription` and `longDescription`.
- Optionally add a tested `licenseFile` reference.
- Optionally add a homepage only after public project URL and release policy are approved.
- Add clearer app-data cleanup instructions in docs.

Not recommended now:

- Changing installer filenames to include beta tags.
- Changing internal version to `0.1.0-beta.x`.
- Setting publisher/manufacturer before certificate identity is known.
- Adding custom NSIS/MSI behavior.
- Adding signing fields or signing secrets.

## Questions For Human Confirmation

- What should the eventual signing certificate subject be?
- Should installer publisher metadata be `Cisyam-x`, or a legal person/organization name?
- Is there an approved public homepage URL for installer metadata?
- Should RikkaDesk identify as `Productivity` or `DeveloperTool` if a category is added?
- Should installer metadata mention private beta, or should beta status remain only in About/docs/release notes?
- Should the uninstall docs include a supported manual app data cleanup command, or only describe the path?

## P1-B Proposal

Recommended P1-B path:

1. Do not change `tauri.conf.json` yet.
2. Add a checklist note that manual app data cleanup must be done only after RikkaDesk is fully closed.
3. If human confirms publisher/signing identity, prepare a separate metadata implementation PR.
4. If no identity is confirmed, keep P1-B docs-only and move to P2 signing workflow design.

P1-A conclusion:

- There is no urgent installer metadata change needed before continuing Phase 11.
- The safest near-term action is to keep current installer metadata stable and defer publisher/manufacturer until signing identity is known.
