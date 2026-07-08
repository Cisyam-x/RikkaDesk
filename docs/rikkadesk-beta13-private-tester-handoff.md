# RikkaDesk beta.13 Hotfix Private Tester Handoff

This note is for private testers only. RikkaDesk beta.13 is the current private hotfix tag for the beta.12 private package line. It is not a public GitHub Release and is not a production-ready release.

## Version Information

- App: RikkaDesk
- App version: 0.1.0
- Current hotfix tag: `rikkadesk-v0.1.0-beta.13`
- Previous private tag: `rikkadesk-v0.1.0-beta.12`
- GitHub Release: not published
- Public release: no
- Installer signing: unsigned

The `rikkadesk-v0.1.0-beta.13` tag has been created and pushed. The `rikkadesk-v0.1.0-beta.12` tag must not be moved, deleted, or overwritten.

## Hotfix Scope

beta.13 fixes the attachment smoke-test blockers found after beta.12:

- Local image attachment draft previews render correctly in Tauri production builds.
- Sent local image attachments render correctly in message bubbles.
- Managed file URLs such as `/api/files/path/{id}` resolve to the actual local mock API URL before image rendering.
- The hidden file picker input is stably mounted.
- Upload detection/upload failures are caught and the file input value is reset, so the same file can be selected again after delete or failure.

## Unchanged From beta.12

- Real-provider image input is still not enabled.
- The loopback-only capture path remains the only implemented image-send prototype.
- Local state remains `schemaVersion: 6`.
- Provider import/export remains version 4.
- The app/package version remains `0.1.0`.
- No public GitHub Release is created from this hotfix tag.

## Recommended Test Focus

Use synthetic files and synthetic app data for attachment tests. Do not use real user files for this hotfix smoke.

Required smoke checks:

1. Send a plain text mock message.
2. Upload a synthetic PNG and confirm the draft preview is not broken.
3. Delete the draft PNG and select the same PNG again.
4. Send the PNG and confirm the message image renders without an unavailable state.
5. Restart RikkaDesk and confirm the same image still renders.
6. Upload/send synthetic JPEG and WEBP files and confirm local image rendering.
7. Upload/send a synthetic GIF and confirm it remains local-only.
8. Upload/send synthetic TXT and PDF files and confirm document chips render without inline PDF preview.
9. Confirm a TEXT-only model blocks PNG send before `/messages`.
10. Confirm an IMAGE-capable non-loopback provider shows confirmation, then returns the loopback-only safe block.
11. Try synthetic SVG and HTML files and confirm a friendly unsupported-format error appears.
12. Confirm state does not contain image base64, provider image request bodies, local file paths, or key/header values.

## Safety Notes

- Do not send real API keys to developers, Codex, ChatGPT, GitHub issues, screenshots, or logs.
- API keys should only be entered locally in Provider Settings when a human tester intentionally tests text-only provider behavior.
- Do not share `mock-api/secrets/*.bin`.
- Do not run a real-provider image input test from this handoff.
- Do not share real user files or private conversation content in issue reports.

## Short Message For Testers

RikkaDesk 0.1.0 beta.13 is the current private hotfix tag for beta.12 attachment testing. It fixes local image preview/rendering and file picker reliability in the Windows desktop build. It is still unsigned, still private, and still does not enable real-provider image input. Use synthetic files for attachment tests and do not share API keys, logs with private content, or `mock-api/secrets/*.bin`.
