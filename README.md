RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

# RikkaDesk

RikkaDesk is a non-official desktop-oriented derivative of [RikkaHub](https://github.com/rikkahub/rikkahub). It currently focuses on making the existing RikkaHub `web-ui` usable as the foundation for a local Windows desktop app.

This repository is in an early staged migration. The current prototype is a local Tauri desktop shell with an in-memory Mock API backend for development verification only.

- Phase 0 is complete: the upstream architecture, `web-ui`, Web Interface, and license were reviewed without code changes.
- Phase 1 is complete: the `web-ui` can run locally in a browser at `http://localhost:5173/`.
- Phase 2A is complete: Tauri v2 wraps the existing `web-ui` as a Windows desktop shell.
- Phase 2B is complete: the `/api/*` contract used by startup and chat flows was inventoried.
- Phase 2C is complete: a minimal Tauri Rust Mock API handles the P0/P1 startup and basic chat endpoints.

Current limitations:

- The Mock API is not a real model backend and does not call OpenAI, Gemini, Anthropic, DeepSeek, or any other provider.
- API keys, provider credentials, persistent settings, SQLite storage, and conversation persistence are not implemented.
- File uploads, attachments, search, MCP, tools, branching, and other P2/P3 endpoints are intentionally deferred.
- Messages are stored only in memory for the lifetime of the desktop process.

What works in the current prototype:

- The Windows desktop window can load the production `web-ui` build.
- The Mock API starts inside the Tauri process and listens on `127.0.0.1`.
- The UI can load settings, show a mock conversation, send a message, and receive the mock reply defined in `web-ui/src-tauri/src/mock_api.rs`.
- If `127.0.0.1:8080` is already in use, the Mock API falls back to a random local loopback port and the frontend reads it through the Tauri command `get_api_base_url`.

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

Useful validation commands:

```powershell
cd web-ui
pnpm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
pnpm run desktop:dev
pnpm run desktop:build
```

More details are in [docs/rikkadesk-dev.md](docs/rikkadesk-dev.md).

Security and compliance:

- Do not write API keys, tokens, passwords, private user data, or conversation data into source files.
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
