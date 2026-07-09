RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

# RikkaDesk

RikkaDesk is a non-official desktop-oriented derivative of [RikkaHub](https://github.com/rikkahub/rikkahub). It currently focuses on making the existing RikkaHub `web-ui` usable as the foundation for a local Windows desktop app.

This repository is in an early staged migration. The current prototype is experimental and is being prepared as a local/private beta, not a public production release. The current private beta baseline is `rikkadesk-v0.1.0-beta.13`, and the current private hotfix tag is `rikkadesk-v0.1.0-beta.14`.

The beta.14 hotfix keeps the app/package version at `0.1.0`. It is not a public release and does not have a public GitHub Release.

> [!IMPORTANT]
> The upstream RikkaHub feature list later in this README describes the Android upstream project. It does not mean every upstream feature is available in the current RikkaDesk desktop beta.

The current RikkaDesk beta includes a local Tauri desktop shell with a Rust desktop API, JSON persistence, conversation and message management, multi-provider and multi-model Provider Settings, advanced non-sensitive provider request config, safe provider import/export, Markdown rendering polish, safer message Markdown link/image handling, local file attachment skeletons, safe attachment rendering, and an OpenAI-compatible text streaming chat path.

- Phase 0 is complete: the upstream architecture, `web-ui`, Web Interface, and license were reviewed without code changes.
- Phase 1 is complete: the `web-ui` can run locally in a browser at `http://localhost:5173/`.
- Phase 2A is complete: Tauri v2 wraps the existing `web-ui` as a Windows desktop shell.
- Phase 2B is complete: the `/api/*` contract used by startup and chat flows was inventoried.
- Phase 2C is complete: a minimal Tauri Rust Mock API handles the P0/P1 startup and basic chat endpoints.
- Phase 3A is complete: local JSON persistence keeps settings, conversations, messages, and idSeq across restarts.
- Phase 3B/3C are complete: provider config uses `secretRef`; API keys must not be stored in JSON.
- Phase 3D/3E are complete: one OpenAI-compatible provider path can return real text chat responses with streaming.
- Phase 4A/4B are complete: Provider Settings UI and smoke-test docs are available for local validation.
- Phase 6A is complete: conversation title, pin, delete, message edit/delete, regenerate, and unsupported visible action polish are available.
- Phase 6B P1/P2 are complete: Provider Settings supports a provider list, add/edit/delete, favorite model updates, setting the current model, and Test Connection.
- Phase 7 P1/P2 are complete: Provider Settings can safely export and import non-sensitive provider metadata.
- Phase 8 P1-P4 are complete: Provider state now supports `providers[].models[]`, Provider Settings can manage multiple models per provider, and provider import/export v2 handles multi-model config while retaining v1 import compatibility.
- Phase 9B P1-P4 are complete: Provider Settings can save non-sensitive custom headers and safe custom body JSON, Test Connection and Streaming Chat share the safe OpenAI-compatible request builder, and provider import/export v3 handles safe advanced request config while retaining v1/v2 import compatibility.
- Phase 9C P2-P4 are complete: Markdown table/code overflow polish, KaTeX mhchem chemistry formulas, message Markdown raw HTML hardening, and Workbench preview sandbox hardening are available while Mermaid in normal messages remains disabled/deferred.
- Phase 10 P2-P6.5 P0 are complete on this branch: the local file API skeleton, attachment upload alignment, safe image/document rendering, model capability metadata, image attachment gating, loopback-only synthetic image capture prototype, and real-provider manual gate documentation are available in the beta.13 hotfix line.
- Beta 14 UX hotfix clears transient TEXT-only image gating errors when switching chats, entering the welcome/new-chat view, changing attachments, or changing models.
- Beta 7 hotfix is complete: long OpenAI-compatible streaming responses run in the background after message send/regenerate requests return accepted, avoiding the previous 30-second POST timeout.

Current limitations:

- RikkaDesk currently supports only the OpenAI-compatible text chat path for real provider testing.
- API keys must never be written to source files, README files, logs, tests, or `state.v1.json`.
- On Windows, provider secrets are referenced from JSON by `secretRef` and stored as encrypted local secret blobs under app data.
- Provider custom headers/body must not contain API keys, tokens, passwords, Authorization headers, `x-api-key`, cookies, or other credentials; API keys still belong only in the API Key field and SecretStore / DPAPI path.
- Message Markdown no longer explicitly enables `rehypeRaw`. Unsafe link schemes such as `javascript:`, `data:`, `file:`, `blob:`, and relative URLs are blocked by default; normal `http:`, `https:`, and `mailto:` links keep `target="_blank"` and `rel="noopener noreferrer"`.
- Unsafe Markdown image sources are blocked by default.
- Workbench Mermaid preview uses strict Mermaid security and a narrower iframe sandbox, but it still loads Mermaid from a remote CDN and remains a residual risk to revisit before any public release.
- Local file attachments are implemented in the beta.13/beta.14 hotfix line: PNG/JPEG/WEBP/GIF images are local attachments, TXT/PDF uploads are document chips, PDF/TXT remain chip-only, and safe image/document rendering is implemented.
- Real OpenAI-compatible provider chat remains text-only by default. Real-provider image input is not enabled for ordinary beta testing.
- IMAGE-capable models can use a loopback-only synthetic image capture prototype for one current-turn PNG/JPEG/WEBP image. This path is for local synthetic testing only; base64 is in memory for the capture request and is not persisted to state, logs, exports, or message parts.
- OCR, PDF/Office parsing, audio/video input, search, MCP, tools, Workspace, full multimodal provider support, sync, and cloud backup are intentionally deferred.
- SQLite, sync, advanced migration tooling, and full upstream feature parity are not implemented.
- JSON remains the beta prototype persistence layer.
- Windows installers and `rikkadesk.exe` are currently unsigned, so Windows SmartScreen or similar unsigned-app warnings are expected.
- Windows 11 Smart App Control may directly block the installed unsigned executable from launching. This is a Windows security policy block for an unverified publisher, not an application crash. The private beta is best tested on development/test machines where Smart App Control is not enabled; do not disable Windows security features or bypass enterprise policy just to run this build.
- RikkaDesk is still a private beta and is not recommended for a public GitHub Release yet.

What works in the current prototype:

- The Windows desktop window can load the production `web-ui` build.
- The local Rust API starts inside the Tauri process and listens on `127.0.0.1`.
- The UI can load settings, show persisted conversations, rename/pin/delete conversations, edit/delete/regenerate text messages, send messages, and receive either mock fallback replies or OpenAI-compatible streaming text responses.
- Provider Settings can add, select, edit, and delete OpenAI-compatible providers.
- Provider Settings can manage multiple models under one provider; those models share the provider Base URL and API key.
- The model selector can show multiple models from the same provider.
- Provider Settings can set the current model and run a safe Test Connection request for a specific model row.
- Provider Settings supports Advanced request config for non-sensitive custom headers and safe custom body JSON. Test Connection and Streaming Chat share the same safe request builder; Test Connection forces `max_tokens=1`, while Streaming Chat can preserve allowed custom body fields such as `max_tokens`.
- Provider Settings can export/import non-sensitive provider metadata; export v4 includes multi-model `models[]`, model modality metadata, and safe `customHeaders` / `customBody`, v1/v2/v3 imports remain compatible, and exported files exclude API keys, `secretRef`, tokens, Authorization headers, `x-api-key`, cookies, DPAPI blobs, and local secret-store files, so imported providers require API keys to be re-entered.
- Provider Settings can save non-sensitive provider config and store API keys through the desktop SecretStore / Windows DPAPI-backed secret mechanism without returning the key to the frontend.
- Message Markdown supports GFM tables with contained horizontal overflow, inline/block math, KaTeX mhchem chemistry formulas, code blocks, code copy/download/preview actions, and safer link/image handling.
- Workbench preview uses a narrower iframe sandbox, and Mermaid preview uses `securityLevel: "strict"`; Mermaid in normal message Markdown remains disabled/deferred.
- Local attachments can be uploaded to the desktop mock API: PNG/JPEG/WEBP/GIF images remain local attachments, TXT/PDF files render as document chips, and unsafe or missing files degrade safely.
- Provider models carry TEXT/IMAGE input capability metadata. TEXT-only models block image attachments, while IMAGE-capable models may use the loopback-only synthetic capture prototype.
- The loopback-only synthetic image capture prototype sends at most one current-turn PNG/JPEG/WEBP image to a local capture server after confirmation. It does not enable real-provider image input.
- If `127.0.0.1:8080` is already in use, the local API falls back to a random loopback port and the frontend reads it through the Tauri command `get_api_base_url`.

## RikkaDesk Development

Install dependencies:

```powershell
cd web-ui
pnpm install
```

Run the browser-only `web-ui` development server:

```powershell
cd web-ui
pnpm run dev
```

Run the RikkaDesk desktop prototype:

```powershell
cd web-ui
pnpm run desktop:dev
```

Build Windows desktop installers:

```powershell
cd web-ui
pnpm run desktop:build
```

Build outputs:

- `web-ui/src-tauri/target/release/rikkadesk.exe`
- `web-ui/src-tauri/target/release/bundle/msi/RikkaDesk_0.1.0_x64_en-US.msi`
- `web-ui/src-tauri/target/release/bundle/nsis/RikkaDesk_0.1.0_x64-setup.exe`

The Windows installer artifacts and installed executable are currently unsigned. For private beta testing, SmartScreen or unsigned publisher warnings are expected. Windows 11 Smart App Control may block the unsigned `rikkadesk.exe` before it starts; that should be treated as an unsigned-publisher security policy block. Do not publish a public Release until code signing, support scope, and license obligations are reviewed.

Useful validation commands:

```powershell
cd web-ui
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run desktop:dev
pnpm run desktop:build
```

More details are in [docs/rikkadesk-dev.md](docs/rikkadesk-dev.md).

Beta packaging notes are in [docs/rikkadesk-beta-package-checklist.md](docs/rikkadesk-beta-package-checklist.md).

Private beta tester notes are in [docs/rikkadesk-beta4-release-notes.md](docs/rikkadesk-beta4-release-notes.md).

Beta release draft notes are in [docs/rikkadesk-release-draft.md](docs/rikkadesk-release-draft.md), and desktop changes are summarized in [CHANGELOG.md](CHANGELOG.md).

Security and compliance:

- Do not write API keys, tokens, passwords, private user data, or conversation data into source files.
- Do not put API keys in `state.v1.json`; JSON state should contain only `secretRef` for provider secrets.
- Do not paste real API keys into docs, issues, screenshots, prompts, or logs. Real keys should only be entered into the local Provider Settings UI by the human tester.
- Do not copy or share `mock-api/secrets/*.bin`.
- This project is derived from RikkaHub. Keep the upstream license notice in mind and review [NOTICE.md](NOTICE.md) plus [LICENSE](LICENSE) before using, modifying, or distributing this project.
- Commercial use or avoiding AGPL obligations may require upstream authorization according to the original RikkaHub license terms.

---

<div align="center">
  <img src="docs/icon.png" alt="App Icon" width="100" />
  <h1>RikkaHub</h1>

  [![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/rikkahub/rikkahub)
  [![Ask DeepWiki](https://img.shields.io/badge/zread.ai-blue?style=flat&logo=readthedocs)](https://zread.ai/rikkahub/rikkahub)

A native Android LLM chat client that supports switching between different providers for
conversations 🤖💬

Click to join our Discord server 👉 [【RikkaHub】](https://discord.gg/9weBqxe5c4)

[简体中文](README_ZH_CN.md) | [繁體中文](README_ZH_TW.md) | English
</div>

<div align="center">
  <img src="docs/img/chat.png" alt="Chat Interface" width="150" />
  <img src="docs/img/desktop.png" alt="Models Picker" width="450" />
</div>

## 🚀 Download

🔗 [Download from Website](https://rikka-ai.com/download)

🔗 [Download from Google Play](https://play.google.com/store/apps/details?id=me.rerere.rikkahub)

## 💖 Sponsors

<div align="center">
  <img src="app/src/main/assets/icons/aihubmix-color.svg" alt="Aihubmix" width="50" />
  <p style="font-size: 16px; font-weight: bold;">Aihubmix</p>
  <p style="font-size: 14px;">Thanks to <a href="https://aihubmix.com?aff=pG7r">aihubmix.com</a> for their financial support. We recommend using aihubmix as a one-stop shop for mainstream models worldwide. (OpenAI, Claude, Google Gemini, DeepSeek, Qwen, and hundreds more).</p>
</div>

## ✨ Features

- 🎨 Material You Design and 🌙 Dark mode
- 🔄 Multiple AI Provider Support: custom API / URL / models (all OpenAI, Google, Anthropic compatible api)
- 🖼️ Multimodal input support (Image, Text Documentation, PDF, Docx)
- 🖥️ Web access for multi-platform use
- 🛠️ MCP support
- 📝 Markdown Rendering (with code highlighting, Latex formulas, tables, Mermaid)
- 🪾 Message Branching
- 🔍 Search capabilities (Exa, Tavily, Zhipu, LinkUp, Brave, Perplexity, etc.)
- 🧩 Prompt variables (model name, time, etc.)
- 🤳 QR code export and import for providers
- 🤖 Agent customization
- 🧠 ChatGPT-like memory feature
- 📝 AI Translation
- 🌐 Custom HTTP request headers and request bodies
- 💌 Silly Tavern character card import

## ✨ Contributing

This project is developed using [Android Studio](https://developer.android.com/studio). PRs are
welcome!

Technology stack documentation:

- [Kotlin](https://kotlinlang.org/) (Development language)
- [Koin](https://insert-koin.io/) (Dependency Injection)
- [Jetpack Compose](https://developer.android.com/jetpack/compose) (UI framework)
- [DataStore](https://developer.android.com/topic/libraries/architecture/datastore) (Preference data
  storage)
- [Room](https://developer.android.com/training/data-storage/room) (Database)
- [Coil](https://coil-kt.github.io/coil/) (Image loading)
- [Material You](https://m3.material.io/) (UI design)
- [Navigation Compose](https://developer.android.com/develop/ui/compose/navigation) (Navigation)
- [Okhttp](https://square.github.io/okhttp/) (HTTP client)
- [kotlinx.serialization](https://github.com/Kotlin/kotlinx.serialization) (JSON serialization)
- [compose-icons/lucide](https://composeicons.com/icon-libraries/lucide) (Icon library)

> [!TIP]
> You need a `google-services.json` file at `app` folder to build the app.

> [!IMPORTANT]  
> The following PRs will be rejected: 
> 1. Translation related changes, such as adding new languages or updating existing translations
> 2. Adding new features, this project is opinionated and will not accept pull requests for new features
> 3. Large-scale refactoring and changes generated by AI

## 💰 Donate

* [Patreon](https://patreon.com/rikkahub)
* [爱发电](https://afdian.com/a/reovo)

## ⭐ Star History

If you like this project, please give it a star ⭐

[![Star History Chart](https://api.star-history.com/svg?repos=re-ovo/rikkahub&type=Date)](https://star-history.com/#re-ovo/rikkahub&Date)

## 📄 License

[License](LICENSE)
