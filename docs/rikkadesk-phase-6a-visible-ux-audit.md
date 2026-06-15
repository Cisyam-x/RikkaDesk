# RikkaDesk Phase 6A Visible UX Audit

Review date: 2026-06-15

Branch: `rikkadesk/phase-6a-conversation-management`

This audit records visible desktop UI actions that can call `/api/*` endpoints and whether the current RikkaDesk beta backend supports them. It is intentionally scoped to visible UX completion and basic conversation management. It does not sync upstream, add providers, add search, add tools, or modify the Android app module.

## Currently Implemented Desktop Backend Endpoints

- `GET /api/settings/stream`
- `GET /api/conversations/paged`
- `GET /api/conversations/stream`
- `GET /api/ai-icon?name=`
- `GET /api/conversations/{id}`
- `GET /api/conversations/{id}/stream`
- `POST /api/conversations/{id}/messages`
- `POST /api/conversations/{id}/stop`
- `POST /api/settings/assistant`
- `POST /api/settings/assistant/model`
- `GET /api/desktop/providers`
- `POST /api/desktop/providers`
- `POST /api/desktop/providers/{id}/secret`
- `DELETE /api/desktop/providers/{id}/secret`

## Visible Unsupported Actions

| Area | UI action | Current API call | Current behavior | Phase 6A decision |
| --- | --- | --- | --- | --- |
| Conversation sidebar row menu | Edit title | `POST /api/conversations/{id}/title` | Raw endpoint error | Implement in P1 |
| Conversation sidebar row menu | Pin / unpin | `POST /api/conversations/{id}/pin` | Raw endpoint error | Implement in P1 |
| Conversation sidebar row menu | Delete conversation | `DELETE /api/conversations/{id}` | Raw endpoint error | Implement in P1 |
| Conversation sidebar row menu | Regenerate title | `POST /api/conversations/{id}/regenerate-title` | Raw endpoint error | Hide/disable in Phase 6A |
| Conversation sidebar row menu | Move to assistant | `POST /api/conversations/{id}/move` | Raw endpoint error if multiple assistants exist | Hide/disable in Phase 6A |
| Sidebar search button | Search conversations/messages | `GET /api/conversations/search?query=` | Raw endpoint error | Disable with friendly beta message |
| Message action row | Edit message | `POST /api/conversations/{id}/messages/{messageId}/edit` | Raw endpoint error | Implement text-only edit in P2 |
| Message action row | Delete message | `DELETE /api/conversations/{id}/messages/{messageId}` | Raw endpoint error | Implement in P2 |
| Message action row | Regenerate | `POST /api/conversations/{id}/regenerate` | Raw endpoint error | Implement last-turn/text regenerate in P2 |
| Message action menu | Create fork | `POST /api/conversations/{id}/fork` | Raw endpoint error | Hide in Phase 6A |
| Message branch controls | Select branch | `POST /api/conversations/{id}/nodes/{nodeId}/select` | Only visible when branches exist | Keep out of scope |
| Tool approval UI | Approve/deny tool | `POST /api/conversations/{id}/tool-approval` | Raw endpoint error if tool parts exist | Keep out of scope; no tool generation in beta |
| Attachment rendering | File URLs | `/api/files/path/{path}` | Not implemented | Keep out of scope |
| Search picker | Web search settings | `POST /api/settings/search/*` | Already disabled for desktop runtime | Keep disabled |

## Phase 6A Scope

### Implement

- Rename conversation.
- Pin and unpin conversation.
- Delete conversation.
- Text-only message edit.
- Delete message.
- Regenerate the last supported text turn.

### Hide Or Friendly-Disable

- Conversation title regeneration.
- Move conversation to another assistant.
- Conversation/message search.
- Forking and branch management.
- Files, attachments, MCP, tools, workspace, and web search.

## Persistence Expectations

Phase 6A should keep `state.v1.json` with `schemaVersion: 2`. No schema upgrade is expected. Conversation title, pinned state, deleted conversations, edited messages, deleted messages, and regenerated replies should be saved through the existing full-state JSON persistence path.

## Safety Notes

- Do not touch real API keys.
- Do not log API keys or Authorization headers.
- Do not write secrets to JSON, source, docs, tests, or logs.
- Continue using the existing SecretStore path for provider credentials.
