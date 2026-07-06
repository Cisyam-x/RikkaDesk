# RikkaDesk Markdown Rendering Plan

Review date: 2026-07-06

This document records the Phase 9C Markdown rendering audit and the fixture checklist for later implementation phases. Phase 9C focuses only on message rendering quality and safety. It does not add file, attachment, search, MCP, tools, multimodal, Workspace, provider, app data, or secret handling.

Current recommended private beta tag:

```text
rikkadesk-v0.1.0-beta.11
```

## Current Rendering Path

RikkaDesk desktop renders message content through the current web-ui message part pipeline:

- `MessageDto.parts[]` is rendered by `MessageParts`.
- A `text` part is rendered by `TextPart`.
- `TextPart` renders the shared `Markdown` component.
- `reasoning` parts also render through the shared `Markdown` component.
- Tool previews that show textual answers also reuse `Markdown`.
- `Markdown` uses `Streamdown`.
- `Markdown` uses `remark-gfm`, `remark-math`, and `rehype-katex`.
- `Markdown` no longer explicitly enables `rehypeRaw` in the RikkaDesk plugin list.
- `Markdown` preprocesses escaped math delimiters:
  - `\(...\)` to `$...$`
  - `\[...\]` to `$$...$$`
- `Markdown` replaces the default code renderer with the local `CodeBlock` component.
- `CodeBlock` uses Shiki for syntax highlighting.
- `CodeBlock` falls back to raw rendering when highlighting is unavailable or the code block is too long.
- Mermaid is not rendered directly inside message Markdown.
- Mermaid is currently available indirectly through Workbench code preview.
- Workbench Mermaid preview renders inside an iframe and currently imports Mermaid from `https://esm.sh/mermaid@11`.
- Workbench Mermaid preview currently uses `securityLevel: "strict"`.

## Current Capability Matrix

| Capability | Current status | Notes |
| --- | --- | --- |
| Basic Markdown | Supported | Via `Streamdown` and Markdown plugins. |
| Inline code | Supported | Rendered as `.inline-code`. |
| Fenced code block | Supported | Routed to local `CodeBlock`. |
| Code copy | Supported | Code block action button. |
| Code download | Supported | Code block action button. |
| Code preview | Partially supported | Depends on language and Workbench preview support. |
| Shiki highlight | Supported | Long code has a size limit and fallback path. |
| GFM table | Supported | Via `remark-gfm`. |
| Table overflow | Supported | Wide tables scroll inside Markdown content after P2 polish. |
| Inline math | Supported | Via `remark-math` and `rehype-katex`. |
| Block math | Supported | Via `remark-math` and `rehype-katex`. |
| mhchem | Supported | `katex/dist/contrib/mhchem.mjs` is imported after P3. |
| Mermaid in message | Not supported | Message Markdown disables Mermaid controls/rendering. |
| Mermaid in Workbench | Supported with caveats | Uses remote CDN, `securityLevel: "strict"`, and an iframe sandbox; requires later review before public release. |
| Raw HTML | Hardened | RikkaDesk no longer explicitly enables `rehypeRaw`; unsafe link and image sources are blocked. |

## Phase 9C Progress

P2 completed:

- CSS-only table overflow polish completed.
- Wide GFM tables now scroll inside Markdown content instead of expanding the app viewport.
- Code block overflow/header minor polish completed.
- No parser, plugin, or dependency changes were made.

P3 completed:

- KaTeX mhchem enabled through `katex/dist/contrib/mhchem.mjs`.
- Existing math pipeline kept unchanged.
- Inline math, block math, and `\ce{...}` chemistry fixtures passed.
- Invalid chemistry does not crash rendering.

P4.1 completed:

- Removed explicit `rehypeRaw` from the RikkaDesk Markdown plugin list.
- Markdown now keeps `rehypeKatex` but no longer explicitly enables raw HTML parsing.
- Added safe link handling.
- Added safe image source handling.
- Dangerous href schemes are not clickable:
  - `javascript:`
  - `data:`
  - `file:`
  - `blob:`
  - relative URL by default
- Links keep `target="_blank"` and `rel="noopener noreferrer"`.
- Raw HTML no longer overrides link target/rel in message Markdown.

P4.2a completed:

- Workbench iframe sandbox narrowed.
- HTML preview sandbox is empty.
- SVG preview sandbox is empty.
- Mermaid preview sandbox is `allow-scripts`.
- Removed `allow-same-origin` from the Workbench preview iframe.
- Mermaid `securityLevel` changed from `loose` to `strict`.
- No dependency changes were made.
- Mermaid message rendering remains deferred.

## Residual Risks

Workbench Mermaid still uses the remote CDN `https://esm.sh/mermaid@11`.
This remains a supply-chain and network-availability concern. It is accepted temporarily for the beta.11 candidate, but should be revisited before any public release.

Workbench HTML/SVG preview still renders user-provided `srcDoc`.
The iframe sandbox is now narrowed, but the content is still user-controlled. HTML/SVG preview should remain scoped to Workbench preview, not message bubbles. Further review may decide whether HTML/SVG preview should be disabled, sanitized, or moved to stricter isolated rendering.

Message Mermaid remains deferred.
Mermaid fenced blocks in messages should remain code blocks. Do not enable Streamdown message Mermaid rendering before a separate design. Do not put Mermaid loose security in the message stream.

Raw HTML in message Markdown remains intentionally disabled.
P4.1 removed explicit `rehypeRaw`. Any future attempt to re-enable raw HTML must include an explicit sanitize policy and XSS fixtures.

## Future Workbench Security Candidates

- Replace the remote Mermaid CDN with a local bundled Mermaid package.
- Keep Mermaid `securityLevel: "strict"`.
- Re-evaluate whether `allow-scripts` is needed only for Mermaid.
- Consider disabling HTML preview scripts permanently.
- Consider sanitizing or disabling SVG preview if needed.
- Add automated XSS fixture tests if test infrastructure is added later.

## Security Boundaries

Phase 9C must preserve these boundaries:

- Do not handle user files, attachments, or multimodal content.
- Do not introduce provider, API key, app data, SecretStore, or DPAPI logic into Markdown rendering.
- Markdown rendering must not read SecretStore, DPAPI, `secrets/*.bin`, app data, or provider secrets.
- Markdown rendering must not log message HTML, secret-bearing content, app data paths, or provider credentials.
- Raw HTML was the largest security concern in this phase.
- P4.1 removed explicit `rehypeRaw`; if raw HTML is re-enabled later, the Streamdown sanitize/harden behavior must be verified against the exact RikkaDesk plugin combination.
- Markdown must not execute `<script>`.
- Markdown must not allow event handlers such as `onclick` or `onerror`.
- Markdown must not allow `javascript:` links.
- Markdown should not allow dangerous message tags such as `iframe`, `object`, or `embed`.
- Normal links must keep `target="_blank"` and `rel="noopener noreferrer"`.
- Mermaid must not be enabled directly in the message stream with loose security.
- Workbench Mermaid preview currently uses a remote CDN and strict Mermaid security; the remote CDN remains a residual risk, not something to expand in message rendering.

## XSS And Raw HTML Fixture Checklist

The following fixtures should be used for manual or automated validation before changing raw HTML behavior:

```md
<script>alert(1)</script>
<img src=x onerror=alert(1)>
<a href="javascript:alert(1)">click</a>
<iframe src="https://example.com"></iframe>
<object data="x"></object>
<embed src="x">
<svg onload=alert(1)></svg>
<div onclick="alert(1)">x</div>
<style>body{display:none}</style>
```

Expected behavior:

- Script must not execute.
- Event handlers must not trigger.
- `javascript:` links must not be navigable.
- Dangerous embed tags must not render active content in message bubbles.
- Rendering must not leak local information.
- Rendering must not break or hide the main UI.
- Errors must be contained to the message preview, if any.

## Markdown And Table Fixture Checklist

General Markdown fixtures:

- Plain paragraph.
- `h1` through `h6`.
- Bold text.
- Italic text.
- Strikethrough.
- Blockquote.
- Ordered list.
- Unordered list.
- Task list.
- Mixed Chinese and English text.
- Long CJK sentence without spaces.
- Long link.
- Link with Markdown formatting in the label.
- Nested list.
- Horizontal rule.
- Footnote, if supported by current plugin behavior.

Table fixtures:

- Simple two-column table.
- Wide table.
- Many-column table.
- Table with inline code.
- Table with long CJK content.
- Table with long Latin content.
- Table with Markdown emphasis.
- Table with line breaks or escaped newlines.
- Table in assistant message.
- Table in user message bubble.
- Table during streaming.
- Table in narrow desktop window.
- Table in wide desktop window.
- Horizontal scrolling behavior.

Wide table fixture:

```md
| very long column name 1 | very long column name 2 | very long column name 3 | very long column name 4 |
|---|---|---|---|
| long long long content | long long long content | long long long content | long long long content |
```

Expected behavior:

- Wide tables should not overflow the entire app viewport.
- Table content should remain readable.
- Horizontal scrolling is acceptable for wide tables.
- Table styling should remain visually consistent in light and dark mode.

## Code Block Fixture Checklist

Languages:

- `js`
- `ts`
- `tsx`
- `python`
- `rust`
- `bash`
- `json`
- `markdown`
- `mermaid`
- Unknown language.
- No language.

Behavior fixtures:

- Very long code block over 12,000 characters.
- Copy button.
- Download button.
- Preview button.
- Preview button absence for unsupported languages.
- Line numbers on.
- Line numbers off.
- Wrap lines on.
- Wrap lines off.
- Streaming code fence before closing backticks arrive.
- Streaming code fence after closing backticks arrive.

Expected behavior:

- Long code must not crash the renderer.
- Shiki failures must fall back to raw code rendering.
- Copy must copy the code content only.
- Download must save the code content only.
- Preview must only appear for allowed preview languages.
- A Mermaid fenced block should remain a code block in the message and may offer Workbench preview if preview is enabled.

## LaTeX And mhchem Fixture Checklist

Current LaTeX state:

- `remark-math` is enabled.
- `rehype-katex` is enabled.
- `katex/dist/katex.min.css` is imported.
- `\(...\)` is preprocessed to `$...$`.
- `\[...\]` is preprocessed to `$$...$$`.
- `katex/dist/contrib/mhchem.mjs` is imported after P3.

P3 completed approach:

- Import `katex/dist/contrib/mhchem.mjs` in `markdown.tsx`.
- Do not change the general math rendering structure.
- Do not change dependency versions unless the build requires it.
- Add fixture-based manual validation before release copy.

Math fixtures:

```md
Inline math: \(E = mc^2\)
```

```md
\[
\int_0^1 x^2 dx = \frac{1}{3}
\]
```

Chemistry fixtures:

```md
$\ce{H2O}$

$\ce{CO2 + C -> 2CO}$

$\ce{SO4^2- + Ba^2+ -> BaSO4 v}$
```

Expected behavior:

- Existing inline math still renders.
- Existing block math still renders.
- Chemical formulas render after mhchem is enabled.
- Invalid chemistry syntax should not crash the app.

## Mermaid Design Review

Current Mermaid state:

- Message Markdown disables Mermaid controls/rendering.
- Workbench preview supports Mermaid code blocks.
- Workbench preview uses an iframe.
- Workbench preview imports Mermaid from `https://esm.sh/mermaid@11`.
- Workbench preview initializes Mermaid with `securityLevel: "strict"`.
- Workbench Mermaid iframe sandbox is `allow-scripts` and no longer includes `allow-same-origin`.

Decision:

- Do not render Mermaid directly inside message Markdown before P4.
- P4 should review local bundle support, sandboxing, and stricter Mermaid security.
- Do not expand the remote CDN Mermaid approach in message rendering.
- Do not place loose Mermaid security inside the normal message stream.

P4 optional routes:

- Route A: explicitly defer Mermaid-in-message and keep code blocks plus Workbench preview.
- Route B: replace Workbench Mermaid CDN usage with a local bundle and stricter sandbox.
- Route C: keep message Mermaid as code block only, with a Preview button rather than direct inline rendering.

## Recommended Implementation Split

### P2: Low-Risk Markdown, Table, And Code Polish

Scope:

- Add table overflow wrapper or CSS polish.
- Ensure tables scroll horizontally in narrow windows.
- Tune code block typography only if needed.
- Keep existing Markdown parser/plugins unchanged.
- Do not touch raw HTML behavior.
- Do not touch Mermaid behavior.
- Do not touch package dependencies.

Candidate files:

- `web-ui/app/components/markdown/markdown.css`
- `web-ui/app/components/markdown/code-block.tsx`

Validation:

- Run the Markdown and table fixtures.
- Run code block fixtures.
- Verify light and dark mode.
- Verify streaming half-rendered Markdown does not crash.

### P3: mhchem Support

Scope:

- Enable `katex/dist/contrib/mhchem.mjs`.
- Keep current math pipeline.
- Avoid dependency changes unless build or runtime requires them.
- Do not alter raw HTML handling.
- Do not alter Mermaid handling.

Candidate file:

- `web-ui/app/components/markdown/markdown.tsx`

Validation:

- Run inline math fixture.
- Run block math fixture.
- Run mhchem fixtures.
- Confirm invalid chemistry syntax does not crash rendering.

### P4: Mermaid And Raw HTML Security Review

Scope:

- P4.1 removed explicit `rehypeRaw`.
- Any future raw HTML support must include an explicit sanitize policy.
- Streamdown sanitize/harden behavior should be verified again if the plugin combination changes.
- Review Workbench Mermaid remote CDN usage.
- Review Mermaid sandbox and security level.
- Do not enable Mermaid in message rendering until the review is complete.

Candidate files:

- `web-ui/app/components/markdown/markdown.tsx`
- `web-ui/app/components/workbench/workbench-host.tsx`
- `web-ui/package.json`
- `web-ui/pnpm-lock.yaml`

Validation:

- Run XSS fixtures.
- Verify links remain safe.
- Verify dangerous HTML does not execute.
- Verify Workbench preview remains contained.

### P5: Docs, Release Copy, And Beta.11

Scope:

- Update README.
- Update CHANGELOG.
- Update About copy if visible support status changes.
- Update beta package checklist.
- Update release draft.
- Prepare beta.11 only after P2/P3/P4 validation.

Candidate files:

- `README.md`
- `CHANGELOG.md`
- `docs/rikkadesk-beta-package-checklist.md`
- `docs/rikkadesk-release-draft.md`
- `web-ui/app/components/about-rikkadesk-dialog.tsx`
- Locale files if About copy changes.

## Files That Should Not Be Modified

Do not modify these areas during Phase 9C P1/P2/P3 unless a later scoped request explicitly allows it:

- `app/**`
- `web-ui/src-tauri/tauri.conf.json`
- `web-ui/src-tauri/src/mock_api.rs`
- Provider Settings.
- SecretStore / DPAPI logic.
- Local app data.
- Provider import/export.
- Provider state schema version.
- Package dependencies in P1/P2 unless explicitly justified later.
- File, attachment, search, MCP, tools, multimodal, or Workspace code paths.

## Acceptance Criteria

P1 acceptance:

- Only `docs/rikkadesk-markdown-rendering-plan.md` is added.
- No rendering code changes.
- No CSS changes.
- No dependency changes.
- No Tauri config changes.
- No Android app changes.
- `git diff --check` passes.
- Worktree is clean after commit.
- Safety search contains only documentation references to security fields or XSS fixtures.
- No real key, token, Authorization header, app data, SecretStore, DPAPI blob, or `secrets/*.bin` content appears in the diff.

Recommended P1 validation commands:

```powershell
git diff --stat
git diff -- docs/rikkadesk-markdown-rendering-plan.md
git diff --check
git status
```

Recommended P1 safety search:

```powershell
git diff | rg -i "apiKey|Authorization|x-api-key|accessToken|refreshToken|secretRef|mock-api/secrets|DPAPI|console.log|.bin|password|token|cookie|bearer"
```

Allowed safety search matches:

- Documentation security notes.
- XSS fixture examples.
- Forbidden field names.

Disallowed safety search matches:

- Real API key.
- Real token.
- Real Authorization header.
- Real app data path containing private state.
- `secrets/*.bin` content.
- Logging guidance that prints sensitive values.
