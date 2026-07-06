# RikkaDesk Upstream Parity Roadmap

Review date: 2026-07-06

This document records a read-only roadmap for bringing RikkaDesk toward the upstream RikkaHub feature set. It does not merge upstream, change source code, create a tag, create a Release, read local secrets, or modify the Android app.

## 1. Current RikkaDesk Baseline

- Branch: `beta/0.1.0`
- Current feature-stable tag: `rikkadesk-v0.1.0-beta.9`
- Current tag commit: `a5476f51b9f77c926d58839bed1a86724ea0dfbe`
- Desktop state file: `state.v1.json`
- Current desktop state schema: `schemaVersion: 3`
- Current persistence model: local JSON state plus local SecretStore / Windows DPAPI-backed secret blobs.

Current RikkaDesk capabilities:

- Windows Tauri desktop shell.
- Local Rust desktop API.
- JSON persistence for settings, conversations, messages, providers, and schema metadata.
- Conversation and message management: title, pin, delete, message edit/delete, regenerate.
- OpenAI-compatible text streaming chat.
- Long OpenAI-compatible streaming responses run in background tasks after message send/regenerate returns `accepted`.
- Multi-provider Provider Settings.
- Multiple models under one OpenAI-compatible provider.
- Per-model Set as current and Test Connection.
- Provider import/export v2 for non-sensitive multi-model metadata, with v1 import compatibility.
- Local SecretStore / DPAPI-backed API key storage using `secretRef`.
- Windows MSI / NSIS packaging.

Current RikkaDesk limits:

- Only OpenAI-compatible text chat is implemented.
- Files, attachments, images, audio, search, MCP, tools, Workspace, multimodal input/output, and upstream Android-only flows are not implemented.
- JSON is still a beta prototype store, not the final database architecture.
- Windows packages are unsigned.

## 2. Original Upstream Baseline

The earlier upstream sync review identified the shared merge base as:

- Merge base: `7513058cb7fbe0201489e13320a0524dc057eec2`
- Merge-base subject: `feat: 适配opencode go思考参数`

The upstream `2.3.0` tag is a nearby major version node:

- `2.3.0`: `090b9671 fix(setting): 文档链接根据系统语言跳转对应语言版本`

Commands used to reconfirm the relationship:

```powershell
git fetch upstream --tags
git merge-base beta/0.1.0 upstream/master
git tag --contains 7513058cb7fbe0201489e13320a0524dc057eec2
```

Observed result:

- `git merge-base beta/0.1.0 upstream/master` still returns `7513058cb7fbe0201489e13320a0524dc057eec2`.
- `git tag --contains <merge-base>` includes `2.3.0`, `2.3.1`, `2.3.2`, `2.3.3`, `2.3.4`, `2.4.0`, and all current RikkaDesk beta tags.

Practical interpretation:

- RikkaDesk should first reach parity with the useful desktop-applicable parts of the original `2.3.0` era feature set.
- After that, RikkaDesk should follow upstream in small version bands: `2.3.1`, `2.3.2`, `2.3.3/2.3.4`, then `2.4.0`.
- Android-only implementation details should not be copied directly into the Tauri/Rust desktop architecture.

## 3. Current Upstream Status

Commands used:

```powershell
git fetch upstream --tags
git rev-parse upstream/master
git describe --tags upstream/master --always
gh release list --repo rikkahub/rikkahub --limit 12
git log --oneline 2.3.0..2.3.1 --max-count=80
git log --oneline 2.3.1..2.3.2 --max-count=80
git log --oneline 2.3.2..2.3.4 --max-count=100
git log --oneline 2.3.4..2.4.0 --max-count=100
git log --oneline 2.4.0..upstream/master --max-count=40
```

Observed upstream state:

- Latest GitHub Release observed: `2.4.0` at 2026-07-05.
- `upstream/master`: `89f8c4b12c5e5b1e1e47fd956ff7b9b3236155d8`
- `git describe upstream/master`: `2.4.0-3-g89f8c4b1`
- `2.4.0`: `ef564dca chore: 发布 2.4.0 版本 (versionCode 168)`
- `2.3.4`: `7b64059e chore(release): 升级版本至 2.3.4 (167)`
- `2.3.3`: `a383c209 chore(release): 升级版本至 2.3.3 (166)`
- `2.3.2`: `8e6da720 chore: 更新依赖和 baseline prof`
- `2.3.1`: `86977a41 chore: 更新 prof文件`
- `2.3.0`: `090b9671 fix(setting): 文档链接根据系统语言跳转对应语言版本`

Version-band summary:

| Upstream band | Main changes observed | RikkaDesk sync implication |
| --- | --- | --- |
| `2.3.0` baseline | Workspace fields, search sorting, MCP/tool adjustments, OpenAI image / ResponseAPI improvements, richtext fixes, docs updates. | Treat as the first parity target, but desktopize instead of merging wholesale. |
| `2.3.0..2.3.1` | Chat attachment handling moved into `ChatPage`, message edit file display/export/share, MCP name validation, workspace documents provider exposure, richtext table fix, OpenAI image-generation refactor. | Mostly deferred because RikkaDesk lacks files, tools, workspace, and image generation. Richtext table fix may be a focused candidate. |
| `2.3.1..2.3.2` | Firecrawl no-key mode, MiMo / Step ASR, request logging toggle, developer log removal, provider setting UI fixes, streaming SelectionContainer crash fix. | Request logging and desktop-safe richtext fixes may be useful; ASR is later. |
| `2.3.2..2.3.4` | Provider recommendations, Serper search, StepFun / ElevenLabs TTS, screen time and calendar tools, upload directory workspace exposure, recent conversation as tool, OCR custom headers/body fix. | Search, TTS, local tools, and workspace are later feature phases. OCR custom headers/body points toward provider advanced request config. |
| `2.3.4..2.4.0` | MCP OAuth 2.1, OAuth refresh/reconnect, OpenAI multimodal tool calls, Google multimedia tool responses, conversation folders, assistant avatar cropping, markdown mhchem, search UI fixes. | Conversation folders and markdown mhchem are plausible focused desktop syncs. MCP/multimodal stay later. |
| `2.4.0..upstream/master` | Tool result images routed according to model input modalities, WebView mhchem import fix, Ollama fetch search provider. | Useful later for multimodal/search; low-risk web-ui mhchem already identified. |

## 4. Gap Matrix

| Module | RikkaDesk current status | Upstream status | Desktop need | Priority | Suggested stage |
| --- | --- | --- | --- | --- | --- |
| Provider / model | Multi-provider and multi-model OpenAI-compatible text providers. No Gemini/Claude-specific protocols. No advanced custom headers/body UI. | Multiple provider families, custom API/URL/models, custom headers/body, provider recommendations. | Yes. Provider advanced config is directly useful. | P0 | Phase 9B |
| Files / attachments / multimodal | Not implemented; unsupported UI is hidden or friendly-disabled. | Image, document, PDF/DOCX, attachment handling, edited files display, OCR transformer, multimodal tool responses. | Yes, but high-risk. Needs desktop file storage and safety model. | P1 | Phase 10 |
| Search | Not implemented; visible unsupported actions are friendly-disabled. | Multiple search providers, sorting, image results, Ollama fetch, Serper. | Yes, but should be separately designed for desktop privacy and provider secrets. | P1 | Phase 11 |
| MCP / tools | Not implemented. | MCP servers, MCP OAuth 2.1, local tools, approval logic, tool UI, tool result handling. | Yes, but large trust boundary. | P2 | Phase 12 |
| Workspace / Sandbox | Not implemented. | Workspace rootfs/proot module, workspace terminal, workspace tools, workspace documents provider. | Eventually, but Android implementation is not directly portable to Windows desktop. | P3 | Phase 13 |
| Markdown / LaTeX / Mermaid / tables | Existing web-ui markdown works; beta.9 has no dedicated upstream markdown sync beyond current inherited web-ui. | Table fixes, bold font fix, LaTeX font-size fix, `mhchem` chemistry support. | Yes, low-risk and visible. | P0 | Phase 9C |
| Message branching | Not implemented. | Listed as upstream feature. | Useful for chat UX, but needs desktop state design. | P2 | Later after file/search basics |
| Prompt variables | Not implemented as a user feature. | Upstream supports prompt variables such as model name and time. | Useful, smaller than MCP. | P1 | After provider advanced config |
| Memory | Not implemented. | Upstream lists ChatGPT-like memory. | Useful but privacy-sensitive. | P2 | After stable storage/privacy model |
| Translation | Not implemented. | Upstream lists AI translation. | Optional desktop utility. | P3 | Later |
| TTS / ASR | Not implemented. | MiMo/Step ASR, StepFun/ElevenLabs TTS, TTS local tool. | Useful, but separate provider/credential surface. | P2 | After search/tools decisions |
| Image generation | Not implemented. | OpenAI image generation and Responses API improvements. | Useful later, but adds binary/media storage and model capability metadata. | P2 | After multimodal P0 |
| Calendar / screen time | Not implemented. | Local Android tools. | Mostly Android-specific; desktop equivalent would need Windows-specific permissions/design. | P3 | Defer |
| Recent conversation reference | Basic conversation list and messages exist; no conversation reference as tool. | Recent chat reference converted into on-demand conversation tool. | Useful only after tool system exists. | P3 | After MCP/tools |
| Cloud sync | Not implemented. | S3/COS fixes exist upstream. | Useful but data-security heavy. | P3 | Defer until local storage stabilizes |
| Custom headers/body | Not implemented in desktop Provider Settings. | Upstream supports provider custom headers/bodies; OCR custom header/body bug fix exists. | Yes, directly useful for OpenAI-compatible providers and gateways. | P0 | Phase 9B |
| SillyTavern character card | Not implemented. | Listed upstream feature. | Optional; likely not blocking desktop beta parity. | P3 | Defer |
| Conversation folders | Not implemented. | Added around `2.4.0`. | Useful for desktop UX after conversation management. | P1 | After Phase 9C or Phase 10 P0 |
| Code signing / installer trust | Research docs exist; packages still unsigned. | Upstream Android release flow does not solve Windows signing. | Yes for public Windows beta. | P1 | Separate signing phase before public release |

## 5. Strategy Recommendation

Do not chase upstream latest immediately.

Recommended strategy:

1. Finish the desktop-applicable subset of the original `2.3.0` era baseline first.
2. Keep RikkaDesk's Tauri shell, Rust Mock API, SecretStore, JSON schema, and Provider Settings as the desktop architecture foundation.
3. Sync upstream ideas selectively, not by broad merge.
4. Open a focused review for each upstream version band:
   - `2.3.0` baseline parity.
   - `2.3.1` delta.
   - `2.3.2` delta.
   - `2.3.3/2.3.4` delta.
   - `2.4.0` delta.
5. Treat Android-only features as product requirements, not implementation patches.
6. Avoid any upstream checkout or merge that could delete RikkaDesk's `web-ui/src-tauri`, local API, provider settings, or desktop documentation.

Rationale:

- RikkaDesk beta.9 already has a stable desktop main flow.
- The largest upstream gaps are feature-surface gaps, not small patch gaps.
- Trying to merge latest upstream before finishing the original parity target would mix product design, state migration, security, and UI work into one unstable step.
- A version-by-version roadmap keeps the desktop product coherent and easier to test.

## 6. Recommended Future Phases

### Phase 9B: Provider Custom Headers / Body

Goal:

- Add non-secret advanced OpenAI-compatible provider request configuration for gateways that need custom headers or custom JSON body fields.

Scope:

- Provider Settings advanced section.
- Safe persistence of non-sensitive custom headers/body.
- Secret-like header values must not be stored in JSON; if supported, they need SecretStore-backed references.
- OpenAI-compatible chat and Test Connection should apply allowed custom config.

Why first:

- It is directly aligned with upstream custom request support.
- It improves real provider compatibility without introducing files/tools/workspace.

### Phase 9C: Markdown / Mermaid / LaTeX / Table Rendering Audit

Goal:

- Compare current RikkaDesk web markdown with upstream richtext fixes and decide which are safe to port.

Candidates:

- `mhchem` chemistry formula support.
- Table width / header rendering fixes.
- Bold font rendering fix.
- LaTeX sizing behavior.
- Mermaid smoke test if current web-ui already exposes it.

Why early:

- Low security risk.
- Visible UX improvement.
- Mostly independent of provider and storage schemas.

### Phase 10: Files / Attachments / Multimodal P0

Goal:

- Design desktop-safe file attachment foundations before implementing provider multimodal calls.

Scope:

- Local file selection/storage policy.
- Message part representation.
- State persistence boundaries.
- Export/security behavior.
- Text-only fallback for unsupported models.

Do not start with image/audio generation before file and attachment safety is settled.

### Phase 11: Search P0

Goal:

- Design desktop search provider configuration and safe search result rendering.

Scope:

- Provider secrets for search APIs.
- Result sorting and source display.
- Friendly unsupported state until implemented.
- No browser automation or hidden network calls without explicit user action.

### Phase 12: MCP / Tools P0

Goal:

- Define desktop trust boundary for tools before exposing any tool execution.

Scope:

- Tool approval model.
- MCP server configuration.
- OAuth implications.
- Local command / filesystem restrictions.
- UI for tool results.

This should not be rushed. It is the largest security boundary after provider secrets.

### Phase 13: Workspace / Sandbox P0

Goal:

- Decide whether RikkaDesk wants a Windows-native workspace/sandbox concept or only a lighter project-folder context.

Scope:

- Workspace data model.
- Terminal/sandbox feasibility on Windows.
- File indexing and ignore rules.
- Relationship to tools and attachments.

Android `proot`/rootfs code is not directly reusable for Windows desktop.

## 7. Sync Rules For Future Work

- Never broad-merge upstream into `beta/0.1.0`.
- Never checkout upstream over RikkaDesk `web-ui`.
- Never overwrite `web-ui/src-tauri`, Provider Settings, SecretStore, or local JSON persistence.
- Keep Android `app` unchanged unless a future phase explicitly targets Android.
- For every upstream version band, create a read-only sync review first.
- For every imported idea, write a desktop design note before implementation if it touches secrets, files, network, tools, workspace, or persistence.
- Maintain explicit security checks for API keys, tokens, Authorization headers, `secretRef`, DPAPI blobs, and `mock-api/secrets/*.bin`.

## 8. Immediate Recommendation

Proceed with Phase 9B: Provider Custom Headers / Body.

Do not update to upstream latest first. The better path is:

1. Finish the original upstream-era desktop parity surface.
2. Stabilize each feature behind beta tags.
3. Then follow upstream deltas one version band at a time.

This keeps RikkaDesk understandable, testable, and safer than a large upstream sync.
