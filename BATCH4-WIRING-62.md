# Feature #62: Bulk Operations via Chat

## Overview
Execute batch email operations through natural language in the AI Chat.
Examples: "archive all emails from LinkedIn", "mark all newsletters as read",
"delete all emails from noreply@example.com older than 30 days".

## Architecture
Leverages the existing agentic chat tool framework (V11). No new API routes needed.

### Flow
1. User asks for a bulk operation in chat
2. LLM uses `list_emails` or `search_emails` to find matching messages
3. LLM shows user how many match, with a sample of subjects
4. LLM emits `ACTION_PROPOSAL` with message IDs
5. User confirms via existing `/api/ai/chat/confirm` endpoint
6. Backend executes batch update

## Files Changed

### Backend (Rust)

**src/ai/tools.rs** — New tool + handler
- Added `bulk_update_emails` tool definition to `all_tools()` (tool #5)
- Actions: archive, mark_read, mark_unread, trash, star, unstar, move_to_category
- 500 message limit per call for safety
- Resolves truncated IDs (8-char prefix -> full UUID)
- Returns structured JSON: `{action, requested, resolved, updated, status}`
- 11 new unit tests covering all actions, edge cases, and error handling

**src/api/chat.rs** — System prompt + confirm_action updates
- System prompt extended with Bulk Operations section
- Instructions for LLM to always show count + sample before proposing action
- `confirm_action()` now handles: unstar, trash, move_to_category (previously only: archive, delete, mark_read, mark_unread, star)

**src/ai/mod.rs** — Added `pub mod tools`

**src/api/mod.rs** — Added `pub mod inbox_stats`

**src/db/migrations.rs** — Added migration 007_inbox_stats

### Prerequisites brought from main
These files were synced from main to support the agentic chat loop:
- `src/ai/tools.rs` (new file — core tool framework)
- `src/ai/provider.rs` (updated — `generate_with_tools()`)
- `src/ai/anthropic.rs` (updated — native tool calling)
- `src/ai/openai.rs` (updated — native function calling)
- `src/ai/ollama.rs` (updated — text-based tool fallback)
- `src/api/chat.rs` (updated — agentic loop replacing single-shot RAG)
- `src/api/inbox_stats.rs` (new file — precomputed inbox stats)
- `migrations/007_inbox_stats.sql` (new migration)

### Frontend (Svelte)

**web/src/components/chat/BulkActionCard.svelte** — New component
- Props: action, messageCount, sampleSubjects, onconfirm, oncancel
- Displays action icon, count badge, sample subject preview (up to 5)
- Confirm/Cancel buttons
- All colors use design tokens (--iris-color-*)
- Standalone component, does not modify ChatPanel.svelte

## Routes
No new routes. Uses existing:
- `POST /api/ai/chat` — sends message, triggers agentic loop with bulk tool
- `POST /api/ai/chat/confirm` — executes confirmed action proposals

## Safety Constraints
- Maximum 500 messages per bulk operation
- Confirmation always required before execution
- LLM instructed to show count + sample before proposing
- All actions are reversible (no permanent delete)

## Test Results
- 143 unit tests pass (11 new for bulk_update)
- 11 integration tests pass
- Frontend builds successfully
