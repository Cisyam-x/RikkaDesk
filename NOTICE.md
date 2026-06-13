# RikkaDesk Notice

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

RikkaHub upstream project:

https://github.com/rikkahub/rikkahub

RikkaDesk is not an official RikkaHub project and is not endorsed by the upstream RikkaHub maintainers unless explicitly stated by them.

## Current Scope

The current project goal is to explore a local Web UI plus Tauri desktop shell for Windows.

At this stage, RikkaDesk includes a Tauri desktop shell and a minimal in-memory Mock API backend for development verification. The Mock API is intended only to make the desktop prototype reproducible and testable while the real Windows local backend is still being designed.

The Mock API does not call real model providers, does not persist data, and does not implement API key management. It handles only the startup and basic chat endpoints needed by the current `web-ui` prototype. A complete compatible local backend for `/api/*` still needs to be designed and implemented in a later phase.

File upload, attachments, search, MCP, tool calls, conversation forks, and other enhanced features are not included in the current Mock API.

## Security

Do not commit API keys, tokens, passwords, private configuration, conversation exports containing private data, or user data to this repository. Runtime secrets should be provided through local configuration or another user-controlled secret mechanism, not hard-coded source files.

## License And Compliance

This repository is derived from RikkaHub and keeps the upstream `LICENSE` file. Use, modification, and distribution of this project must comply with the upstream license terms, including AGPL obligations where applicable.

Commercial use, use by organizations or user groups outside the upstream open-source allowance, or attempts to avoid AGPL source-availability obligations may require a commercial license from the upstream RikkaHub maintainers. Review the upstream `LICENSE` file and upstream licensing instructions before using or distributing RikkaDesk.
