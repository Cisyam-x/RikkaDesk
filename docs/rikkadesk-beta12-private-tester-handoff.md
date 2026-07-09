# RikkaDesk beta.12 Private Tester Handoff

This note is for private testers only. RikkaDesk beta.12 is a private candidate package, not a public GitHub Release and not a production-ready release.

## Version Information

- App: RikkaDesk
- Version: 0.1.0 Beta 12 private candidate
- Tag: `rikkadesk-v0.1.0-beta.12`
- Commit: `f5cfbc52a8f56995333f585f0169a261e2334b91`
- GitHub Release: not published
- Public release: no
- Installer signing: unsigned

## Recommended Package

Recommended for normal private testing:

- `RikkaDesk_0.1.0_x64-setup.exe`

Package guidance:

- The NSIS setup exe is the preferred installer for normal private testing.
- The MSI package is available as a backup or enterprise-style installer.
- `rikkadesk.exe` is intended for development or direct-run verification and is not recommended for ordinary tester installation.

## SHA256 Checksums

| Artifact | Size | SHA256 |
|---|---:|---|
| `rikkadesk.exe` | 17,570,304 bytes / 16.76 MB | `A082AA57249375336DF0D9662F6AF95BBF3AD2A2A97BE56780F797AE0A421DDE` |
| `RikkaDesk_0.1.0_x64_en-US.msi` | 8,245,248 bytes / 7.86 MB | `643CE1EA969A99C5018AB3491BBC7B26190BE770B91192C559820BCFA50A907A` |
| `RikkaDesk_0.1.0_x64-setup.exe` | 6,659,529 bytes / 6.35 MB | `5486FB6EA1DF9DA63F4A1D5D0C210E8DEF6AAC6C0AC514BF1CB3878355871A85` |

PowerShell checksum command for the recommended setup package:

```powershell
Get-FileHash .\RikkaDesk_0.1.0_x64-setup.exe -Algorithm SHA256
```

Expected SHA256:

```text
5486FB6EA1DF9DA63F4A1D5D0C210E8DEF6AAC6C0AC514BF1CB3878355871A85
```

## Installation Notes

- Windows SmartScreen or unsigned publisher warnings are expected.
- Windows 11 Smart App Control may block unsigned executables directly.
- Do not ask testers to disable Windows security features or bypass enterprise security policy.
- If Smart App Control blocks the installer or app, record the result as an unsigned-publisher policy block.

## Test Scope

beta.12 private testing includes:

- Windows Tauri desktop shell
- Local Rust API
- Local JSON persistence
- Conversation and message management
- Provider Settings
- Multi-provider and multi-model provider configuration
- Provider import/export v4
- Advanced non-sensitive custom headers/body
- OpenAI-compatible text streaming
- Markdown rendering polish
- Safe Markdown link/image handling
- Workbench sandbox hardening
- Local attachment skeleton
- PNG/JPEG/WEBP/GIF local image attachments
- TXT/PDF document chips
- Safe image/document rendering
- TEXT/IMAGE model capability metadata
- TEXT-only model blocks image attachments
- Loopback-only synthetic image capture prototype

## Explicitly Not Included

beta.12 private testing does not include:

- Public GitHub Release
- Real-provider image input by default
- Full multimodal provider support
- OCR
- PDF/Office parsing
- Audio/video input
- Workspace
- MCP/tools/search
- Auto-update
- Code signing
- Production-readiness guarantee

## API Key Safety

- Do not send real API keys to developers, Codex, ChatGPT, GitHub issues, screenshots, or logs.
- API keys should only be entered locally in Provider Settings.
- Do not share `mock-api/secrets/*.bin`.
- When reporting issues, do not include logs containing API keys, prompts, private chat content, personal file content, or secrets.
- Do not test real-provider image input in this beta. Image attachments remain local by default, and the image capture prototype is loopback-only.

## Suggested Test Flow

1. Install with `RikkaDesk_0.1.0_x64-setup.exe`.
2. Launch RikkaDesk.
3. Open Provider Settings.
4. Add an OpenAI-compatible provider.
5. Enter an API key only locally.
6. Run Test Connection.
7. Send one text-only message.
8. Restart the app and confirm the conversation is still present.
9. Upload synthetic PNG/JPEG/WEBP/GIF files and confirm local image attachments render.
10. Upload synthetic TXT/PDF files and confirm document chips render.
11. Confirm a TEXT-only model blocks image attachments.
12. Do not test real-provider image input.
13. Uninstall the app and record whether app data is retained.

## Feedback Template

```text
- Windows version:
- Install method: NSIS / MSI / direct exe
- SmartScreen / Smart App Control:
- App launched: Yes/No
- Provider Settings works: Yes/No
- Test Connection result:
- Text streaming result:
- Attachment upload result:
- Local image/document rendering result:
- Any crash or freeze:
- Logs/screenshots attached: Yes/No
- Confirm no API keys included: Yes/No
```

## Short Message For Testers

这是 RikkaDesk 0.1.0 beta.12 私测包，仅用于本地测试，不是公开发布。推荐安装 RikkaDesk_0.1.0_x64-setup.exe。安装包未签名，SmartScreen 提示属于预期。请不要把真实 API Key、聊天内容、个人文件或 secrets 文件发给任何人。当前只支持真实 provider 文本聊天；图片附件为本地附件，真实 provider 图片输入默认未启用。
