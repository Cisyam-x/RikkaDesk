# RikkaDesk 0.1.0 Beta 4 Private Testing Notes

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub. This document is for private Windows beta testers who receive a local installer package.

## 当前版本

- 当前功能稳定 tag: `rikkadesk-v0.1.0-beta.4`
- 内部应用版本: `0.1.0`
- 发布状态: private beta, not a public GitHub Release

This build is for local/private testing only. It is not recommended for broad public distribution.

## 推荐安装包

Recommended installer:

```text
web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe
```

MSI fallback:

```text
web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi
```

For most private testing, use the NSIS setup `.exe`.

## 安装前须知

- Windows 安装包目前未签名。
- Windows SmartScreen / unknown publisher warnings are expected.
- Windows 11 Smart App Control may block the installed unsigned executable, for example `C:\Users\<you>\AppData\Local\RikkaDesk\rikkadesk.exe`, before RikkaDesk can start.
- If Smart App Control blocks the app, this is a Windows security policy decision for an unverified publisher, not an application crash.
- This private beta is recommended for development or testing machines where Smart App Control is not enabled.
- Do not disable Windows security features or bypass enterprise security policy just to run a private beta build.
- The long-term fix is Windows code signing for the app executable and installer. Code signing can be handled in a separate future phase.
- This is a private beta, not a public release.
- Do not send real API keys to developers, Codex, GitHub issues, screenshots, logs, or chat transcripts.
- Do not paste real API keys into any feedback form.
- If a screenshot is needed, confirm the API Key field is empty or hidden first.

## 当前可测试功能

- Start the Windows desktop app.
- Create conversations.
- Rename, pin, and delete conversations.
- Edit, delete, and regenerate text messages.
- Provider Settings:
  - add provider
  - edit provider
  - delete provider
  - save API Key locally
  - confirm `hasSecret`
  - favorite/unfavorite model
  - set as current model
  - Test Connection
- OpenAI-compatible text streaming chat.
- Restart persistence for conversations, messages, provider config, and selected model state.
- Mock fallback when no usable provider secret is configured.

## 当前不支持功能

- Files and attachments.
- Image or audio input/output.
- Web search.
- MCP.
- Tool calling.
- Workspace.
- Multimodal provider calls.
- Provider import/export. Only the safety design document exists.
- Gemini, Claude, Anthropic, Vertex, or other provider-specific protocols.
- Public release auto-update flow.

## 推荐测试流程

1. Install RikkaDesk with the NSIS setup `.exe`.
2. Start RikkaDesk.
3. Open Provider Settings.
4. Add an OpenAI-compatible provider.
5. Fill Base URL and Model ID.
6. Enter the API Key locally.
7. Save the provider.
8. Confirm the API Key input is cleared.
9. Confirm the UI shows `hasSecret: true`.
10. Run Test Connection.
11. Set the provider model as the current model.
12. Send a text-only chat message.
13. Confirm the assistant response streams progressively.
14. Restart RikkaDesk.
15. Confirm provider config and chat history remain.
16. Delete a test provider.
17. Confirm the provider disappears and no secret value is shown.
18. Uninstall RikkaDesk.
19. Check whether app data remains if cleanup is part of the test.

## 安全检查说明

RikkaDesk should not store API keys in readable JSON state.

Important paths on Windows:

```text
%APPDATA%\com.cisyamx.rikkadesk
```

Secrets directory:

```text
%APPDATA%\com.cisyamx.rikkadesk\mock-api\secrets
```

State file:

```text
%APPDATA%\com.cisyamx.rikkadesk\mock-api\state.v1.json
```

Expected behavior:

- `state.v1.json` stores non-sensitive provider config and `secretRef`.
- `state.v1.json` should not contain API keys, access tokens, refresh tokens, Authorization header values, or `x-api-key` values.
- The UI should show only `hasSecret: true` or `hasSecret: false`.
- The app should not display the saved API Key after saving.

普通测试者不需要执行复杂命令。以上路径主要用于排查安装、卸载、数据保留或安全问题。

## 卸载和清理数据

Uninstalling RikkaDesk may leave app data and encrypted local secret blobs on disk. This is expected for the current beta.

To fully clear local beta data:

1. Close RikkaDesk.
2. Delete:

```text
%APPDATA%\com.cisyamx.rikkadesk
```

This removes local conversations, provider config, mock API state, and local encrypted secret blobs.

## 问题反馈模板

When reporting an issue, use this template.

```text
系统版本：
安装包类型：NSIS / MSI
是否出现 SmartScreen：是 / 否
Provider 类型：OpenAI-compatible
是否保存 API Key：是 / 否（不要粘贴 key）
复现步骤：
实际结果：
期望结果：
截图/日志：注意不要包含 API Key、Token、Authorization header、聊天隐私内容
```

Please remove or blur any private prompt, response, URL, or credential before sharing screenshots or logs.

## 不公开发布说明

- This is still a private beta.
- No public GitHub Release should be created from this testing note.
- No auto-update channel is promised.
- No compatibility migration promise is established for future beta data.
- Public distribution should wait until signing, support scope, security review, and license obligations are reviewed.
