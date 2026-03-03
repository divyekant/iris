# V7: AI On-Demand Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add on-demand AI features: thread summarization (lazy, cached) and AI compose assist (rewrite, tone adjust, suggestions).

**Architecture:** Two new backend endpoints call Ollama via the existing `OllamaClient`. Thread summaries are lazy-computed on first request and cached in the `ai_summary` column of the first message in the thread. AI assist takes content + action and returns transformed text. Frontend surfaces these through a collapsible summary panel in ThreadView and an AI assist dropdown in ComposeModal.

**Tech Stack:** Rust/Axum (existing `ai::ollama`), Svelte 5, Tailwind CSS 4

**Scoping note:** Semantic search (U40, U41, N18, N38) is deferred — it requires sqlite-vec embeddings infrastructure not yet built. Thread summary + AI assist deliver the highest user value without new dependencies.

---

## Task 1: Thread Summarize Backend Endpoint

**Files:**
- Create: `src/api/ai_actions.rs`
- Modify: `src/api/mod.rs` (add `pub mod ai_actions;`)
- Modify: `src/main.rs` (add route)

**What to build:**

`POST /api/threads/{id}/summarize` endpoint that:
1. Loads all messages in the thread via `MessageDetail::list_by_thread`
2. Checks if the first message already has `ai_summary` cached — if so, return it immediately
3. Checks AI is enabled (`ai_enabled` config) and model is set (`ai_model` config)
4. Builds a prompt with all message bodies (truncated to fit context)
5. Calls `OllamaClient::generate` with a summarization system prompt
6. Caches the result in the first message's `ai_summary` column via `message::update_ai_metadata`
7. Returns `{ summary: string }`

System prompt for summarization:
```
You are an email thread summarizer. Given a thread of emails, produce a concise 2-4 sentence summary covering:
- Key topic/decision being discussed
- Current status or outcome
- Any action items or next steps

Be factual and concise. Do not use bullet points. Respond with plain text only.
```

Prompt format — concatenate messages chronologically:
```
Thread: {subject}

Message 1 (from {from}, {date}):
{body_text truncated to 500 chars}

Message 2 (from {from}, {date}):
{body_text truncated to 500 chars}
...
```

Truncate total prompt to 3000 chars to stay within model context.

**Response type:**
```rust
#[derive(Serialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub cached: bool,
}
```

Return 404 if thread not found, 503 if AI not configured/available.

**Step 1:** Create `src/api/ai_actions.rs` with the `summarize_thread` handler function.

**Step 2:** Add `pub mod ai_actions;` to `src/api/mod.rs`.

**Step 3:** Add route to `src/main.rs`:
```rust
.route("/threads/{id}/summarize", post(api::ai_actions::summarize_thread))
```

**Step 4:** Run `cargo build` — verify it compiles.

**Step 5:** Commit:
```
feat(v7): thread summarize endpoint with caching
```

---

## Task 2: Thread Summarize Test

**Files:**
- Modify: `src/api/ai_actions.rs` (add test module)

**What to build:**

Unit test for prompt construction and caching logic. Since we can't call Ollama in tests, test:

1. `build_summary_prompt` — a helper function extracted from the handler that takes messages and returns the formatted prompt string. Verify it concatenates messages, truncates body to 500 chars each, caps total at 3000 chars.
2. Test that cached summary is returned when `ai_summary` is already set.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_summary_prompt_single_message() {
        // Create a MessageDetail with subject and body
        // Call build_summary_prompt
        // Assert prompt contains "Thread:", message from, body text
    }

    #[test]
    fn test_build_summary_prompt_truncates_long_body() {
        // Create message with 1000-char body
        // Assert body is truncated to 500 chars with "..."
    }

    #[test]
    fn test_build_summary_prompt_caps_total_length() {
        // Create 10 messages with 500-char bodies
        // Assert total prompt is <= 3000 chars
    }
}
```

**Step 1:** Extract `build_summary_prompt` as a standalone function in `ai_actions.rs`.

**Step 2:** Add test module with the 3 tests above.

**Step 3:** Run `cargo test` — verify all tests pass.

**Step 4:** Commit:
```
test(v7): thread summary prompt construction tests
```

---

## Task 3: Thread Summary Frontend — Collapsible Panel in ThreadView

**Files:**
- Modify: `web/src/lib/api.ts` (add `api.threads.summarize`)
- Modify: `web/src/pages/ThreadView.svelte` (add AI summary panel)

**What to build:**

1. Add to `api.ts`:
```typescript
threads: {
    get: ...,
    summarize: (id: string) => request<{ summary: string; cached: boolean }>(`/api/threads/${id}/summarize`, { method: 'POST' }),
},
```

2. In `ThreadView.svelte`, add a collapsible AI summary panel between the thread header and messages:
- A small "AI Summary" button/link in the thread header area
- On click, calls `api.threads.summarize(params.id)`
- Shows loading spinner while fetching
- Renders summary text in a subtle card (light purple/blue bg)
- Collapsible — toggle open/closed with chevron icon
- If AI is not configured (503 error), show a subtle message "Enable AI in Settings"
- State: `aiSummary` (string | null), `summaryLoading` (boolean), `summaryOpen` (boolean)

Layout — insert after the header `<div>` and before the messages scrollable area:
```svelte
{#if thread && thread.message_count > 1}
  <div class="px-4 py-2 border-b border-gray-200 dark:border-gray-700">
    <button onclick={toggleSummary} class="text-xs text-blue-500 flex items-center gap-1">
      {summaryOpen ? '▾' : '▸'} AI Summary
    </button>
    {#if summaryOpen}
      {#if summaryLoading}
        <div class="mt-2 text-xs text-gray-400">Summarizing...</div>
      {:else if aiSummary}
        <div class="mt-2 text-sm text-gray-700 dark:text-gray-300 bg-blue-50 dark:bg-blue-900/20 rounded-lg px-3 py-2">
          {aiSummary}
        </div>
      {:else if summaryError}
        <div class="mt-2 text-xs text-gray-400">{summaryError}</div>
      {/if}
    {/if}
  </div>
{/if}
```

Only show the summary button when thread has > 1 message (single-message threads don't need summarization).

**Step 1:** Add `api.threads.summarize` to `web/src/lib/api.ts`.

**Step 2:** Add summary state + toggle logic + UI panel to `ThreadView.svelte`.

**Step 3:** Run `npm run build` — verify frontend compiles.

**Step 4:** Commit:
```
feat(v7): collapsible AI thread summary in ThreadView
```

---

## Task 4: AI Assist Backend Endpoint

**Files:**
- Modify: `src/api/ai_actions.rs` (add `ai_assist` handler)
- Modify: `src/main.rs` (add route)

**What to build:**

`POST /api/ai/assist` endpoint that takes content + action and returns transformed text.

**Request:**
```rust
#[derive(Deserialize)]
pub struct AiAssistRequest {
    pub action: String,       // "rewrite", "formal", "casual", "shorter", "longer"
    pub content: String,      // The text to transform
    pub context: Option<String>, // Optional — original email for reply suggestions
}
```

**Response:**
```rust
#[derive(Serialize)]
pub struct AiAssistResponse {
    pub result: String,
}
```

System prompts by action:

- **"rewrite"**: "Rewrite the following text to be clearer and more professional. Preserve the original meaning. Return only the rewritten text, no explanation."
- **"formal"**: "Rewrite the following text in a formal, professional tone. Return only the rewritten text."
- **"casual"**: "Rewrite the following text in a casual, friendly tone. Return only the rewritten text."
- **"shorter"**: "Condense the following text to be more concise while preserving key points. Return only the shortened text."
- **"longer"**: "Expand the following text with more detail and elaboration. Return only the expanded text."

Handler logic:
1. Validate `action` is one of the supported values, return 400 if not
2. Check AI enabled + model configured, return 503 if not
3. Build system prompt based on action
4. Call `OllamaClient::generate(model, content, Some(system_prompt))`
5. Return result or 502 if Ollama fails

Route:
```rust
.route("/ai/assist", post(api::ai_actions::ai_assist))
```

**Step 1:** Add the `AiAssistRequest`, `AiAssistResponse` structs and `ai_assist` handler to `ai_actions.rs`.

**Step 2:** Add route to `src/main.rs`.

**Step 3:** Run `cargo build` — verify it compiles.

**Step 4:** Commit:
```
feat(v7): AI assist endpoint for compose text transformation
```

---

## Task 5: AI Assist Test

**Files:**
- Modify: `src/api/ai_actions.rs` (add tests)

**What to build:**

Test `get_assist_system_prompt` — a helper function that maps action strings to system prompts.

```rust
#[test]
fn test_get_assist_prompt_rewrite() {
    let prompt = get_assist_system_prompt("rewrite").unwrap();
    assert!(prompt.contains("clearer"));
}

#[test]
fn test_get_assist_prompt_formal() {
    let prompt = get_assist_system_prompt("formal").unwrap();
    assert!(prompt.contains("formal"));
}

#[test]
fn test_get_assist_prompt_invalid_action() {
    assert!(get_assist_system_prompt("invalid").is_none());
}
```

**Step 1:** Extract `get_assist_system_prompt(action: &str) -> Option<&'static str>` as a standalone function.

**Step 2:** Add tests.

**Step 3:** Run `cargo test` — all tests pass.

**Step 4:** Commit:
```
test(v7): AI assist action prompt mapping tests
```

---

## Task 6: AI Assist Frontend — Button in ComposeModal

**Files:**
- Modify: `web/src/lib/api.ts` (add `api.ai.assist`)
- Modify: `web/src/components/compose/ComposeModal.svelte` (add AI assist dropdown)

**What to build:**

1. Add to `api.ts`:
```typescript
ai: {
    ...existing...,
    assist: (data: { action: string; content: string; context?: string }) =>
      request<{ result: string }>('/api/ai/assist', { method: 'POST', body: JSON.stringify(data) }),
},
```

2. In `ComposeModal.svelte`, add an AI assist dropdown button in the footer (between "Save Draft" and "Send"):

State:
```typescript
let aiAssisting = $state(false);
let showAiMenu = $state(false);
```

Actions:
```typescript
const aiActions = [
  { action: 'rewrite', label: 'Improve writing' },
  { action: 'formal', label: 'Make formal' },
  { action: 'casual', label: 'Make casual' },
  { action: 'shorter', label: 'Make shorter' },
  { action: 'longer', label: 'Expand' },
];

async function handleAiAssist(action: string) {
  if (!body.trim()) return;
  showAiMenu = false;
  aiAssisting = true;
  try {
    const res = await api.ai.assist({ action, content: body });
    body = res.result;
  } catch {
    error = 'AI assist failed. Check AI settings.';
  } finally {
    aiAssisting = false;
  }
}
```

UI — a small button with sparkle icon (✨) that opens a dropdown menu:
```svelte
<div class="relative">
  <button
    class="px-3 py-1.5 text-sm text-gray-500 hover:text-blue-500 transition-colors disabled:opacity-50"
    onclick={() => (showAiMenu = !showAiMenu)}
    disabled={aiAssisting || sending || !body.trim()}
    title="AI Assist"
  >
    {aiAssisting ? 'Thinking...' : 'AI ✨'}
  </button>
  {#if showAiMenu}
    <div class="absolute bottom-full left-0 mb-1 bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg shadow-lg py-1 min-w-[160px]">
      {#each aiActions as { action, label }}
        <button
          class="w-full text-left px-3 py-1.5 text-sm hover:bg-gray-100 dark:hover:bg-gray-700"
          onclick={() => handleAiAssist(action)}
        >{label}</button>
      {/each}
    </div>
  {/if}
</div>
```

Close the menu when clicking outside (add click handler on the compose backdrop).

**Step 1:** Add `api.ai.assist` to `web/src/lib/api.ts`.

**Step 2:** Add AI assist dropdown to `ComposeModal.svelte` footer.

**Step 3:** Run `npm run build` — verify frontend compiles.

**Step 4:** Commit:
```
feat(v7): AI assist dropdown in compose modal
```

---

## Task 7: Integration Verification

**What to verify:**

1. `cargo test` — all tests pass (should be ~46: 40 existing + 3 summary + 3 assist)
2. `cd web && npm run build` — frontend compiles
3. `cargo build` — backend compiles with no new warnings beyond existing 3

**Step 1:** Run `cargo test`, verify all pass.

**Step 2:** Run frontend build, verify success.

**Step 3:** Update `docs/SESSION-STATUS.md` — add V7 section, update "What's Next" to V8.

**Step 4:** Update `~/.claude/projects/-Users-divyekant-Projects-iris/memory/MEMORY.md` — add V7 to "What's Built", bump state.

**Step 5:** Commit docs update:
```
docs: mark V7 AI On-Demand complete
```
