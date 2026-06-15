# RikkaDesk Upstream Sync Review

Review date: 2026-06-15

This document records the upstream sync review between RikkaDesk `beta/0.1.0` and the latest `rikkahub/rikkahub` `master` branch. This review is analysis-only. No upstream merge, checkout, or source synchronization was performed.

## Current State

- Upstream branch reviewed: `upstream/master`
- Latest upstream commit: `16952af1 docs: 官网下载链接标注为推荐`
- Latest upstream tag observed: `2.3.0`
- `2.3.0` commit: `090b9671 fix(setting): 文档链接根据系统语言跳转对应语言版本`
- Merge base with RikkaDesk beta: `7513058c feat: 适配opencode go思考参数`
- RikkaDesk branch reviewed: `beta/0.1.0`

## Upstream Change Themes

Since the merge base, upstream changes are mainly concentrated in the Android application and shared AI logic:

- Workspace / sandbox capability expansion.
- Android search results can be sorted by date.
- MCP / tools behavior adjustments.
- OpenAI image generation and ResponseAPI tool-output enhancements.
- Richtext / markdown fixes.
- README and official website download-link documentation updates.

These changes are useful to track, but most of them are outside the current RikkaDesk private beta scope.

## Web UI Conclusion

Upstream has not made actual `web-ui` changes since the merge base.

The meaningful comparison is:

```powershell
git diff --stat 7513058c..upstream/master -- web-ui
```

That diff is empty.

By contrast, this comparison:

```powershell
git diff --stat beta/0.1.0..upstream/master -- web-ui
```

shows many deletions, including RikkaDesk Tauri, Mock API, Provider Settings, API base URL, and Vite configuration files. Those deletions do not mean upstream changed `web-ui`. They mean upstream does not contain RikkaDesk desktop-specific files. A naive upstream merge or checkout could remove RikkaDesk's desktop shell and local backend work.

## DTO And API Impact

The upstream changes include some model and API-adjacent updates:

- `Assistant` adds `workspaceId`.
- `Conversation` adds `workspaceCwd`.
- Search adds sorting modes such as relevance, newest first, and oldest first.
- `Tool.needsApproval` changes from a static boolean-style field to parameter-aware logic.
- Typed message metadata was added for reasoning, thought, diff, and related message metadata.
- OpenAI / ResponseAPI changes mainly target image generation, tool output, and reasoning support.

Current impact on RikkaDesk beta:

- No direct impact on the current RikkaDesk startup path.
- No direct impact on Provider Settings.
- No direct impact on local JSON persistence or the current state schema.
- No direct impact on the current OpenAI-compatible text streaming implementation.
- No immediate need to change the RikkaDesk desktop API DTOs.

The new workspace-related fields should be recorded for a future schema review if RikkaDesk later implements workspace support.

## Current Recommendation

Do not sync upstream into RikkaDesk now.

Specifically:

- Do not perform a broad upstream merge for the current beta.
- Do not overwrite `web-ui` from upstream.
- Do not replace RikkaDesk's Tauri shell, Mock API, Provider Settings, SecretStore, JSON persistence, or OpenAI-compatible streaming logic.
- Keep the current private beta stable and continue testing the desktop main flow.

The current upstream updates are valuable, but they mostly target Android-side workspace, search, tools, image generation, and documentation behavior. They do not justify destabilizing the current RikkaDesk beta.

## Future Sync Strategy

Future upstream synchronization should be selective:

- Keep workspace, search, MCP, tools, file attachments, and related P2/P3 features out of the current beta.
- If RikkaDesk later implements workspace support, consider a state schema migration, likely schema v3, to account for fields such as `workspaceId` and `workspaceCwd`.
- If RikkaDesk later implements local search, review upstream's search sorting model before designing the desktop search API.
- If RikkaDesk later implements tools, MCP, reasoning, image generation, or multimodal outputs, review upstream's typed metadata and ResponseAPI changes.
- OpenAI / ResponseAPI error-handling ideas can be referenced, but the Android AI core should not be copied directly into the Rust desktop backend.
- Upstream README wording can be reviewed manually, but RikkaDesk README / NOTICE / CHANGELOG should not be overwritten because they contain desktop-specific beta, license, and non-official derivative notices.

## Risks If Not Synced Now

- RikkaDesk will continue to diverge from upstream Android workspace, search, MCP, and tool capabilities.
- If future upstream `web-ui` work starts depending on workspace fields, RikkaDesk will need DTO and persistence migration work.
- The current desktop beta still does not support files, attachments, search, MCP, tool calling, branching, or workspace features.
- The current OpenAI-compatible implementation only covers text streaming chat and does not inherit upstream image, reasoning, or tool-output enhancements.

These risks are acceptable for the current private beta because the tested main flow is installation, startup, Provider Settings, secret storage, model selection, text streaming chat, restart persistence, and uninstall behavior.
