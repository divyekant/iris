---
shaping: true
---

# V11: AI-Scalable Chat — Slices

Parent: [Shaping Doc](2026-03-08-v11-ai-scalable-chat-shaping.md)

## Slice Summary

| # | Slice | Parts | Demo |
|---|-------|-------|------|
| V11-S1 | Inbox Snapshot | D1 (all) | "How many unread emails?" → instant accurate count |
| V11-S2 | Agentic Chat Core | D2.1-D2.4, D3 (all), D4 (all) | "What does the security alert say?" → LLM calls search → reads email → answers |
| V11-S3 | List & Filter Tools | D2.5, D2.3 filters | "Show me unread emails from Google this week" → SQL-filtered results |

---

## V11-S1: Inbox Snapshot

**Mechanism:** D1 — Pre-computed inbox aggregates stored in SQLite, updated on sync.

**Why this slice is first:** Independently useful even without the agentic loop. Injecting stats into the existing system prompt already fixes the "How many emails?" class of questions. Zero risk — additive only, no changes to existing chat flow.

### New Affordances

| # | Component | Affordance | Control | Wires Out | Returns To |
|---|-----------|------------|---------|-----------|------------|
| S1 | SQLite | `inbox_stats` table | store | — | → N11, N17 |
| N11 | tools.rs | `inbox_stats` tool handler | call | → S1 | → N15 |
| N16 | stats.rs | `compute_inbox_stats()` | call | → S2 | → S1 |
| N17 | stats.rs | `GET /api/inbox/stats` | call | → S1 | → external |
| N18 | sync.rs | sync completion trigger | call | → N16 | — |

### Implementation Details

**Migration** (`migrations/007_inbox_stats.sql`):
```sql
CREATE TABLE IF NOT EXISTS inbox_stats (
    account_id TEXT NOT NULL,
    total INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0,
    by_category TEXT DEFAULT '{}',    -- JSON: {"primary": 50, "social": 20, ...}
    top_senders TEXT DEFAULT '[]',    -- JSON: [{"address": "x@y.com", "name": "X", "count": 15}, ...]
    today_count INTEGER NOT NULL DEFAULT 0,
    week_count INTEGER NOT NULL DEFAULT 0,
    month_count INTEGER NOT NULL DEFAULT 0,
    last_updated TEXT NOT NULL,
    PRIMARY KEY (account_id)
);
```

**Compute function** (`src/api/stats.rs` or inline in sync):
```
SELECT COUNT(*) as total,
       SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END) as unread,
       SUM(CASE WHEN is_starred = 1 THEN 1 ELSE 0 END) as starred,
       SUM(CASE WHEN date >= ?today_start THEN 1 ELSE 0 END) as today_count,
       SUM(CASE WHEN date >= ?week_start THEN 1 ELSE 0 END) as week_count,
       SUM(CASE WHEN date >= ?month_start THEN 1 ELSE 0 END) as month_count
FROM messages WHERE account_id = ?1
```

**Integration with existing chat:** Inject stats summary into system prompt:
```
Your user has: {total} total emails, {unread} unread, {starred} starred.
Today: {today_count} emails. This week: {week_count}. This month: {month_count}.
Top senders: {top_senders_formatted}
```

### Demo Script
1. Open AI Chat
2. Ask "How many unread emails do I have?"
3. AI responds with exact count (not "I don't have enough context")
4. Ask "Who sends me the most email?"
5. AI responds with top senders from stats

---

## V11-S2: Agentic Chat Core

**Mechanism:** D2.1-D2.4, D3, D4 — Replace single-shot LLM call with iterative tool-use loop. The LLM decides what information it needs and calls tools to get it.

**Why this slice is second:** This is the core transformation. After S1, the chat can answer aggregate questions. After S2, it can answer *any* question by iteratively gathering context. This is the biggest change — it refactors the chat handler from single-shot to multi-turn.

### New Affordances

| # | Component | Affordance | Control | Wires Out | Returns To |
|---|-----------|------------|---------|-----------|------------|
| U5 | ChatPanel | tool activity indicator | render | — | — |
| N5 | chat.rs | `agentic_loop(messages, tools, max=5)` | call | → N6, → N15 | → N1 |
| N6 | provider.rs | `ProviderPool.generate_with_tools()` | call | → N7/N8/N9 | → N5 |
| N7 | anthropic.rs | `AnthropicClient.generate_with_tools()` | call | — | → N6 |
| N8 | openai.rs | `OpenAIClient.generate_with_tools()` | call | — | → N6 |
| N9 | ollama.rs | `OllamaClient.generate_with_tools()` — text fallback | call | — | → N6 |
| N10 | tools.rs | `ToolRegistry` | call | → N11-N13 | — |
| N12 | tools.rs | `search_emails` handler (basic, no filters) | call | → S2, S4, S5 | → N15 |
| N13 | tools.rs | `read_email` handler | call | → S2 | → N15 |
| N15 | tools.rs | `execute_tool(name, args)` | call | → N10 | → N5 |

### Key Data Types

```rust
// Tool definition (sent to LLM)
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema
}

// LLM response — either text or tool call
pub enum LlmResponse {
    Text(String),
    ToolCalls(Vec<ToolCall>),
}

pub struct ToolCall {
    pub id: String,          // tool_use_id for linking results
    pub name: String,
    pub arguments: serde_json::Value,
}

// Message for multi-turn conversation
pub struct LlmMessage {
    pub role: String,        // "user", "assistant", "tool"
    pub content: String,
    pub tool_call_id: Option<String>,   // links tool result to tool_use
    pub tool_calls: Option<Vec<ToolCall>>,  // assistant's tool calls
}
```

### Agentic Loop Pseudocode

```
fn agentic_loop(messages, tools, db, memories, max_iterations=5):
    for i in 0..max_iterations:
        response = provider_pool.generate_with_tools(messages, system, tools)
        match response:
            LlmResponse::Text(text) => return text
            LlmResponse::ToolCalls(calls) =>
                // Append assistant message with tool calls
                messages.push(assistant_message_with_tool_calls)
                for call in calls:
                    result = execute_tool(call.name, call.arguments, db, memories)
                    messages.push(tool_result_message(call.id, result))
                    // Stream tool activity to UI via response
    // Safety: if max iterations reached, force text response
    return provider_pool.generate(final_prompt, system)
```

### Provider Implementation Priority

1. **Anthropic** (primary) — Native tool use via `tools` parameter. Already supports multi-tool calling.
2. **OpenAI** — Native function calling via `tools` parameter. Very similar to Anthropic.
3. **Ollama** (fallback) — Embed tool descriptions in system prompt. Parse `TOOL_CALL:{"name":"...","arguments":{...}}` from response. Less reliable but works.

### Existing Chat Handler Changes

The current `POST /api/ai/chat` handler in `chat.rs` does:
1. Load history → FTS5 search → Memories search → build_chat_prompt → single generate() call

After S2, it becomes:
1. Load history → build_initial_messages() → agentic_loop() with tools

The FTS5/Memories search moves INTO the `search_emails` tool handler — the LLM decides when to search. Citations are collected from tool call results instead of from the pre-search step.

### Demo Script
1. Open AI Chat
2. Ask "What do my security alert emails say?"
3. Watch tool activity: "Searching emails..." → "Reading email..."
4. AI responds with specific details from the email body (not just subject/snippet)
5. Ask "Tell me more about when my passkey was added"
6. AI calls read_email on a specific email, provides date and device details

---

## V11-S3: List & Filter Tools

**Mechanism:** D2.5, D2.3 filters — Add `list_emails` tool for structured browsing, and filter parameters to `search_emails` for SQL-level precision.

**Why this slice is third:** Adds precision to the agentic loop. Without filters, the LLM can only do text search. With filters, it can do "unread emails from Google this week" as a structured query — critical for the 1M email scale.

### New Affordances

| # | Component | Affordance | Control | Wires Out | Returns To |
|---|-----------|------------|---------|-----------|------------|
| N14 | tools.rs | `list_emails` handler — SQL with filters | call | → S2 | → N15 |
| N12' | tools.rs | `search_emails` handler — upgraded with filter params | call | → S2, S4, S5 | → N15 |

### list_emails Tool Schema

```json
{
    "name": "list_emails",
    "description": "List emails with optional filters. Returns summary (no body). Use for browsing, counting filtered results, or finding specific emails by metadata.",
    "input_schema": {
        "type": "object",
        "properties": {
            "date_from": { "type": "string", "description": "ISO date, e.g. 2026-03-01" },
            "date_to": { "type": "string", "description": "ISO date, e.g. 2026-03-08" },
            "sender": { "type": "string", "description": "Email address or name substring" },
            "is_read": { "type": "boolean", "description": "Filter by read status" },
            "category": { "type": "string", "description": "AI category: primary, social, promotions, updates" },
            "folder": { "type": "string", "description": "IMAP folder name" },
            "sort": { "type": "string", "enum": ["newest", "oldest"], "default": "newest" },
            "limit": { "type": "integer", "default": 20, "maximum": 50 }
        }
    }
}
```

### SQL Pre-filter Implementation

```sql
SELECT id, subject, from_address, from_name, date, is_read, is_starred, ai_category, snippet
FROM messages
WHERE account_id = ?1
  AND (?date_from IS NULL OR date >= ?date_from)
  AND (?date_to IS NULL OR date <= ?date_to)
  AND (?sender IS NULL OR from_address LIKE '%' || ?sender || '%' OR from_name LIKE '%' || ?sender || '%')
  AND (?is_read IS NULL OR is_read = ?is_read)
  AND (?category IS NULL OR ai_category = ?category)
  AND (?folder IS NULL OR folder = ?folder)
ORDER BY date DESC
LIMIT ?limit
```

For `search_emails` with filters: apply SQL WHERE first, then JOIN with FTS5 on the filtered result set.

### Demo Script
1. Open AI Chat
2. Ask "Show me unread emails from Google this week"
3. AI calls `list_emails` with `{sender: "google", is_read: false, date_from: "2026-03-02"}`
4. Returns precise filtered list
5. Ask "How many promotional emails did I get this month?"
6. AI calls `list_emails` with `{category: "promotions", date_from: "2026-03-01"}`
7. Counts results and answers accurately

---

## Slice Dependencies

```
V11-S1 (Inbox Snapshot) ──independent──→ can ship alone
    ↓
V11-S2 (Agentic Core) ──uses S1──→ inbox_stats tool available in loop
    ↓
V11-S3 (List & Filter) ──uses S2──→ tools added to existing registry
```

S1 is fully independent. S2 builds on S1 (includes the inbox_stats tool). S3 builds on S2 (adds tools to the registry and upgrades search_emails).

## Files Changed Per Slice

| File | S1 | S2 | S3 |
|------|:--:|:--:|:--:|
| `migrations/007_inbox_stats.sql` | new | — | — |
| `src/api/stats.rs` | new | — | — |
| `src/api/chat.rs` | modify (inject stats in prompt) | refactor (agentic loop) | — |
| `src/ai/tools.rs` | — | new | modify (add list_emails, filters) |
| `src/ai/provider.rs` | — | modify (add generate_with_tools) | — |
| `src/ai/anthropic.rs` | — | modify (add generate_with_tools) | — |
| `src/ai/openai.rs` | — | modify (add generate_with_tools) | — |
| `src/ai/ollama.rs` | — | modify (add generate_with_tools) | — |
| `src/sync.rs` | modify (trigger stats compute) | — | — |
| `src/api/mod.rs` | modify (add stats route) | — | — |
| `src/lib.rs` | modify (register route) | — | — |
| `web/src/lib/ChatPanel.svelte` | — | modify (tool activity UI) | — |
