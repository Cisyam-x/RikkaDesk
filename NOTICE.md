# RikkaDesk Notice

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

RikkaHub upstream project:

https://github.com/rikkahub/rikkahub

RikkaDesk is not an official RikkaHub project and is not endorsed by the upstream RikkaHub maintainers unless explicitly stated by them.

## Current Scope

The current project goal is to explore a local Web UI plus Tauri desktop app for Windows.

At this stage, RikkaDesk includes a Tauri desktop shell, a local Rust API, JSON persistence, Provider Settings, encrypted local secret storage on Windows, and a minimal OpenAI-compatible text chat path for local testing.

The local API still keeps a mock fallback path so the desktop prototype remains reproducible when no real provider is configured. It is not a full replacement for every upstream `/api/*` route.

File upload, attachments, search, MCP, tool calls, conversation forks, multimodal provider calls, and other enhanced features are not included in the current desktop beta scope.

## Security

Do not commit API keys, tokens, passwords, private configuration, conversation exports containing private data, or user data to this repository. Runtime secrets must not be hard-coded in source files, documentation, tests, logs, or `state.v1.json`.

RikkaDesk JSON state should store only non-sensitive provider configuration and `secretRef`. The actual secret value should remain in the local desktop secret mechanism and must not be returned to the frontend or written to project files.

## License And Compliance

This repository is derived from RikkaHub and keeps the upstream `LICENSE` file. Use, modification, and distribution of this project must comply with the upstream license terms, including AGPL obligations where applicable.

Commercial use, use by organizations or user groups outside the upstream open-source allowance, or attempts to avoid AGPL source-availability obligations may require a commercial license from the upstream RikkaHub maintainers. Review the upstream `LICENSE` file and upstream licensing instructions before using or distributing RikkaDesk.
