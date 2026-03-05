# V5: Keyword Search Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add full-text keyword search using the existing FTS5 index so users can search emails by content, subject, sender, with filters for date range and attachments.

**Architecture:** Backend adds `GET /api/search` endpoint that queries the existing `fts_messages` FTS5 virtual table with snippet extraction. Frontend adds a Search page (P5) with search input, filter chips, and results list. The existing disabled search bar in Header becomes functional, navigating to the search page.

**Tech Stack:** Existing SQLite FTS5 (already indexed via triggers), existing Svelte 5 frontend. No new dependencies.

**V5 Affordances:** U38 (search input), U39 (filter chips), U42 (results list), U43 (result row click)

**Already in place:** FTS5 virtual table `fts_messages` with columns (message_id, subject, body_text, from_address, from_name), auto-sync triggers on insert/update/delete, U8 (search bar stub in Header.svelte)

---

## Context for All Tasks

**Existing state:**
- `fts_messages` FTS5 index exists in `migrations/001_initial.sql:82-109`
- Triggers auto-populate FTS5 on message insert/update/delete
- `Header.svelte` has disabled search input placeholder
- `App.svelte` uses svelte-spa-router hash routing
- API pattern: handlers in `src/api/`, models in `src/models/`, routes in `src/main.rs`
- 30 tests currently passing

---

### Task 1: Backend — Search Endpoint

Add `GET /api/search?q=&has_attachment=&after=&before=&limit=` that queries FTS5 with snippet highlighting.

**Files:**
- Create: `src/api/search.rs`
- Modify: `src/api/mod.rs` (add search module)
- Modify: `src/main.rs` (wire route)

**Step 1: Create src/api/search.rs**

```rust
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub has_attachment: Option<bool>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub account_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub snippet: String,
    pub date: Option<i64>,
    pub is_read: bool,
    pub has_attachments: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: i64,
    pub query: String,
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let query_str = params.q.as_deref().unwrap_or("").trim().to_string();
    if query_str.is_empty() {
        return Ok(Json(SearchResponse {
            results: Vec::new(),
            total: 0,
            query: query_str,
        }));
    }

    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    // Build dynamic WHERE clauses for filters
    let mut conditions = Vec::new();
    let mut filter_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    // FTS5 match — parameter index 1
    conditions.push(format!("fts.fts_messages MATCH ?{param_idx}"));
    // Escape FTS5 special characters and wrap terms in quotes for safety
    let fts_query = query_str
        .split_whitespace()
        .map(|term| {
            // Strip FTS operators, wrap in double quotes for literal matching
            let clean = term.replace('"', "");
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");
    filter_params.push(Box::new(fts_query));
    param_idx += 1;

    if let Some(true) = params.has_attachment {
        conditions.push(format!("m.has_attachments = 1"));
    }

    if let Some(after) = params.after {
        conditions.push(format!("m.date >= ?{param_idx}"));
        filter_params.push(Box::new(after));
        param_idx += 1;
    }

    if let Some(before) = params.before {
        conditions.push(format!("m.date <= ?{param_idx}"));
        filter_params.push(Box::new(before));
        param_idx += 1;
    }

    if let Some(ref account_id) = params.account_id {
        conditions.push(format!("m.account_id = ?{param_idx}"));
        filter_params.push(Box::new(account_id.clone()));
        param_idx += 1;
    }

    conditions.push("m.is_deleted = 0".to_string());

    let where_clause = conditions.join(" AND ");

    // Search query with FTS5 snippet for highlighting
    let sql = format!(
        "SELECT m.id, m.account_id, m.thread_id, m.from_address, m.from_name,
                m.subject, snippet(fts, 3, '<mark>', '</mark>', '...', 40) as match_snippet,
                m.date, m.is_read, m.has_attachments
         FROM fts_messages fts
         JOIN messages m ON fts.rowid = m.rowid
         WHERE {where_clause}
         ORDER BY rank
         LIMIT ?{param_idx} OFFSET ?{}",
        param_idx + 1
    );
    filter_params.push(Box::new(limit));
    filter_params.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = filter_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Search query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let results: Vec<SearchResult> = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(SearchResult {
                id: row.get("id")?,
                account_id: row.get("account_id")?,
                thread_id: row.get("thread_id")?,
                from_address: row.get("from_address")?,
                from_name: row.get("from_name")?,
                subject: row.get("subject")?,
                snippet: row.get("match_snippet")?,
                date: row.get("date")?,
                is_read: row.get("is_read")?,
                has_attachments: row.get("has_attachments")?,
            })
        })
        .map_err(|e| {
            tracing::error!("Search execution error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Count total matches (without LIMIT/OFFSET)
    let count_sql = format!(
        "SELECT COUNT(*)
         FROM fts_messages fts
         JOIN messages m ON fts.rowid = m.rowid
         WHERE {where_clause}"
    );
    // Re-build params without limit/offset
    let count_params: Vec<&dyn rusqlite::types::ToSql> = filter_params[..filter_params.len() - 2]
        .iter()
        .map(|p| p.as_ref())
        .collect();
    let total: i64 = conn
        .query_row(&count_sql, count_params.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    Ok(Json(SearchResponse {
        results,
        total,
        query: query_str,
    }))
}
```

**Step 2: Add search module to src/api/mod.rs**

```rust
pub mod search;
```

**Step 3: Wire route in src/main.rs**

Add after the threads route:

```rust
.route("/search", get(api::search::search))
```

**Step 4: Build and test**

Run: `cargo build 2>&1`
Expected: Compiles.

Run: `cargo test 2>&1`
Expected: All 30 tests pass.

**Step 5: Commit**

```bash
git add src/api/search.rs src/api/mod.rs src/main.rs
git commit -m "feat(v5): FTS5 search endpoint with snippet highlighting and filters"
```

---

### Task 2: Backend — Search Tests

Add tests for the FTS5 search functionality to verify it works with the existing triggers.

**Files:**
- Create: `src/api/search_tests.rs` or add tests inline

Since the search function is async and needs AppState, we'll test the underlying FTS5 queries directly in the models layer.

**Files:**
- Modify: `src/models/message.rs` (add search test)

**Step 1: Add FTS5 search test**

Add to `mod tests` in `src/models/message.rs`:

```rust
    #[test]
    fn test_fts5_search() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Invoice from Amazon", false);
        msg1.message_id = Some("<fts-1@example.com>".to_string());
        msg1.body_text = Some("Please find attached your invoice for order #12345.".to_string());
        InsertMessage::insert(&conn, &msg1);

        let mut msg2 = make_insert_message(&account.id, "INBOX", "Meeting tomorrow", false);
        msg2.message_id = Some("<fts-2@example.com>".to_string());
        msg2.body_text = Some("Let's meet at 3pm to discuss the project.".to_string());
        InsertMessage::insert(&conn, &msg2);

        // Search for "invoice" — should match msg1
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fts_messages WHERE fts_messages MATCH '\"invoice\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(count >= 1);

        // Search for "meeting" — should match msg2
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fts_messages WHERE fts_messages MATCH '\"meeting\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(count >= 1);

        // Search for "nonexistent" — should match nothing
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM fts_messages WHERE fts_messages MATCH '\"zzzznonexistent\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 0);

        // Snippet extraction
        let snippet: String = conn.query_row(
            "SELECT snippet(fts_messages, 3, '<mark>', '</mark>', '...', 20)
             FROM fts_messages WHERE fts_messages MATCH '\"invoice\"'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(snippet.contains("<mark>"));
    }
```

**Step 2: Run tests**

Run: `cargo test 2>&1`
Expected: All 31 tests pass.

**Step 3: Commit**

```bash
git add src/models/message.rs
git commit -m "test(v5): FTS5 search integration test with snippet extraction"
```

---

### Task 3: Frontend — Search Page

Create the Search page with search input, filter chips, and results list.

**Files:**
- Create: `web/src/pages/Search.svelte`
- Modify: `web/src/lib/api.ts` (add search method)
- Modify: `web/src/App.svelte` (add search route)

**Step 1: Add search API method to api.ts**

```typescript
search: (params: { q: string; has_attachment?: boolean; after?: number; before?: number; account_id?: string; limit?: number; offset?: number }) => {
    const query = new URLSearchParams();
    query.set('q', params.q);
    if (params.has_attachment) query.set('has_attachment', 'true');
    if (params.after) query.set('after', String(params.after));
    if (params.before) query.set('before', String(params.before));
    if (params.account_id) query.set('account_id', params.account_id);
    if (params.limit) query.set('limit', String(params.limit));
    if (params.offset) query.set('offset', String(params.offset));
    return request<{ results: any[]; total: number; query: string }>(`/api/search?${query}`);
},
```

**Step 2: Create web/src/pages/Search.svelte**

Full search page with:
- Search input at top (auto-focused, debounced 300ms)
- Filter chips row: has:attachment toggle, date range (Last 7 days, Last 30 days, Last year)
- Results list with highlighted snippets (rendered as HTML via {@html})
- Result row click → navigate to thread
- Empty state, loading state, no results state
- Total count display

```svelte
<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';

  let { params }: { params: { query?: string } } = $props();

  let searchQuery = $state(params?.query || '');
  let results = $state<any[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let searched = $state(false);

  // Filters
  let hasAttachment = $state(false);
  let dateFilter = $state('');

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  function getDateRange(): { after?: number; before?: number } {
    const now = Math.floor(Date.now() / 1000);
    switch (dateFilter) {
      case '7d': return { after: now - 7 * 86400 };
      case '30d': return { after: now - 30 * 86400 };
      case '1y': return { after: now - 365 * 86400 };
      default: return {};
    }
  }

  async function doSearch() {
    if (!searchQuery.trim()) {
      results = [];
      total = 0;
      searched = false;
      return;
    }
    loading = true;
    searched = true;
    try {
      const dateRange = getDateRange();
      const res = await api.search({
        q: searchQuery,
        has_attachment: hasAttachment || undefined,
        ...dateRange,
      });
      results = res.results;
      total = res.total;
    } catch {
      results = [];
      total = 0;
    } finally {
      loading = false;
    }
  }

  function onInput() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(doSearch, 300);
  }

  function handleResultClick(result: any) {
    const threadId = result.thread_id || result.id;
    push(`/thread/${encodeURIComponent(threadId)}`);
  }

  function formatDate(timestamp: number): string {
    const d = new Date(timestamp * 1000);
    return d.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
  }

  function toggleFilter(filter: string) {
    dateFilter = dateFilter === filter ? '' : filter;
    doSearch();
  }

  function toggleAttachment() {
    hasAttachment = !hasAttachment;
    doSearch();
  }

  $effect(() => {
    if (searchQuery) doSearch();
    return () => { if (debounceTimer) clearTimeout(debounceTimer); };
  });
</script>

<div class="h-full flex flex-col">
  <!-- Search header -->
  <div class="px-4 py-3 border-b border-gray-200 dark:border-gray-700">
    <div class="flex items-center gap-3">
      <button
        class="p-1 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
        onclick={() => push('/')}
        title="Back to inbox"
      >&larr;</button>
      <input
        type="text"
        bind:value={searchQuery}
        oninput={onInput}
        placeholder="Search emails..."
        class="flex-1 px-3 py-2 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        autofocus
      />
    </div>

    <!-- Filter chips -->
    <div class="flex gap-2 mt-2">
      <button
        class="px-3 py-1 text-xs rounded-full border transition-colors
               {hasAttachment ? 'bg-blue-100 dark:bg-blue-900/40 border-blue-300 dark:border-blue-700 text-blue-700 dark:text-blue-300' : 'border-gray-300 dark:border-gray-600 text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={toggleAttachment}
      >has:attachment</button>
      {#each [{ id: '7d', label: 'Last 7 days' }, { id: '30d', label: 'Last 30 days' }, { id: '1y', label: 'Last year' }] as f}
        <button
          class="px-3 py-1 text-xs rounded-full border transition-colors
                 {dateFilter === f.id ? 'bg-blue-100 dark:bg-blue-900/40 border-blue-300 dark:border-blue-700 text-blue-700 dark:text-blue-300' : 'border-gray-300 dark:border-gray-600 text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800'}"
          onclick={() => toggleFilter(f.id)}
        >{f.label}</button>
      {/each}
    </div>
  </div>

  <!-- Results -->
  <div class="flex-1 overflow-auto">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 border-blue-200 border-t-blue-600 rounded-full animate-spin"></div>
      </div>
    {:else if searched && results.length === 0}
      <div class="text-center py-16 text-gray-400 dark:text-gray-500">
        <p class="text-lg mb-2">No results found</p>
        <p class="text-sm">Try different keywords or adjust your filters.</p>
      </div>
    {:else if results.length > 0}
      <div class="px-4 py-2 text-xs text-gray-400 dark:text-gray-500">
        {total} result{total === 1 ? '' : 's'}
      </div>
      <div class="divide-y divide-gray-100 dark:divide-gray-800">
        {#each results as result (result.id)}
          <button
            class="w-full text-left px-4 py-3 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors flex items-start gap-3"
            onclick={() => handleResultClick(result)}
          >
            <div class="pt-1.5 w-3 flex-shrink-0">
              {#if !result.is_read}
                <div class="w-2.5 h-2.5 rounded-full bg-blue-500"></div>
              {/if}
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-baseline gap-2">
                <span class="text-sm truncate {result.is_read ? 'text-gray-700 dark:text-gray-300' : 'font-semibold text-gray-900 dark:text-gray-100'}">
                  {result.from_name || result.from_address || 'Unknown'}
                </span>
                <span class="flex-shrink-0 text-xs text-gray-400 dark:text-gray-500 ml-auto">
                  {#if result.has_attachments}
                    <span class="mr-1.5" title="Has attachments">{'\u{1F4CE}'}</span>
                  {/if}
                  {#if result.date}
                    {formatDate(result.date)}
                  {/if}
                </span>
              </div>
              <div class="text-sm truncate {result.is_read ? 'text-gray-600 dark:text-gray-400' : 'font-medium text-gray-800 dark:text-gray-200'}">
                {result.subject || '(no subject)'}
              </div>
              <div class="text-xs text-gray-400 dark:text-gray-500 mt-0.5 line-clamp-2">
                {@html result.snippet}
              </div>
            </div>
          </button>
        {/each}
      </div>
    {:else}
      <div class="text-center py-16 text-gray-400 dark:text-gray-500">
        <p class="text-lg mb-2">Search your emails</p>
        <p class="text-sm">Type a keyword to find messages by content, subject, or sender.</p>
      </div>
    {/if}
  </div>
</div>
```

**Step 3: Add search route to App.svelte**

Import Search and add route:

```typescript
import Search from './pages/Search.svelte';

const routes = {
    '/': Inbox,
    '/search': Search,
    '/thread/:id': ThreadView,
    '/setup': AccountSetup,
    '/setup/*': AccountSetup,
    '/settings': Settings,
};
```

**Step 4: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 5: Commit**

```bash
git add web/src/pages/Search.svelte web/src/lib/api.ts web/src/App.svelte
git commit -m "feat(v5): search page with FTS5 results, snippet highlighting, filter chips"
```

---

### Task 4: Frontend — Wire Header Search Bar

Make the disabled search bar in Header.svelte functional — clicking/focusing navigates to the search page.

**Files:**
- Modify: `web/src/components/Header.svelte`

**Step 1: Update Header.svelte**

```svelte
<script lang="ts">
  import { push } from 'svelte-spa-router';

  function openSearch() {
    push('/search');
  }
</script>

<header class="h-14 border-b border-gray-200 dark:border-gray-700 flex items-center px-4 gap-4">
  <div class="flex-1">
    <input
      type="text"
      placeholder="Search emails..."
      readonly
      class="w-full max-w-md px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm placeholder-gray-400 cursor-pointer hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors"
      onfocus={openSearch}
      onclick={openSearch}
    />
  </div>
</header>
```

**Step 2: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 3: Commit**

```bash
git add web/src/components/Header.svelte
git commit -m "feat(v5): wire header search bar — click navigates to search page"
```

---

### Task 5: Integration Verification

Build everything, run all tests, verify the full V5 feature set.

**Files:**
- No new files

**Step 1: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 2: Build backend**

Run: `cargo build 2>&1`
Expected: Compiles.

**Step 3: Run all tests**

Run: `cargo test 2>&1`
Expected: All 31 tests pass (30 V1-V4 + 1 new FTS5 test).
