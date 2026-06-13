# RikkaDesk Notice

RikkaDesk is an unofficial desktop derivative / experiment based on RikkaHub.

RikkaHub upstream project:

https://github.com/rikkahub/rikkahub

RikkaDesk is not an official RikkaHub project and is not endorsed by the upstream RikkaHub maintainers unless explicitly stated by them.

## Current Scope

The current project goal is to explore a local Web UI plus Tauri desktop shell for Windows.

At this stage, RikkaDesk does not include a complete Windows local backend. The existing `web-ui` can run locally, but chat and related runtime features still depend on compatible `/api/*` endpoints. A compatible local backend for `/api/*` must be designed and implemented in a later phase.

## Security

Do not commit API keys, tokens, passwords, private configuration, conversation exports containing private data, or user data to this repository. Runtime secrets should be provided through local configuration or another user-controlled secret mechanism, not hard-coded source files.

## License And Compliance

This repository is derived from RikkaHub and keeps the upstream `LICENSE` file. Use, modification, and distribution of this project must comply with the upstream license terms, including AGPL obligations where applicable.

Commercial use, use by organizations or user groups outside the upstream open-source allowance, or attempts to avoid AGPL source-availability obligations may require a commercial license from the upstream RikkaHub maintainers. Review the upstream `LICENSE` file and upstream licensing instructions before using or distributing RikkaDesk.
