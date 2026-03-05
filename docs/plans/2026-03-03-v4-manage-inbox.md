# V4: Manage Inbox — Bulk Ops + View Toggle + Multi-Account

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add category tabs, view mode toggle, bulk message actions, account switcher, and thread-level actions so users can manage their inbox efficiently across multiple accounts.

**Architecture:** Backend adds batch update endpoint (`PATCH /messages/batch`) and view mode config persistence. Frontend adds category tab bar, selection checkboxes with bulk action bar, messaging-style view toggle, account switcher in sidebar, and thread action buttons. All changes extend existing patterns — no new crates or schema migrations needed.

**Tech Stack:** Existing Rust/Axum backend + Svelte 5 frontend. No new dependencies.

**V4 Affordances (this plan):** U1 (category tabs), U5 (view toggle), U6 (bulk action bar), U10 (account switcher), U21 (thread actions)

**Already done by V1-V3 (reused, not modified):** U7 (compose button), U8 (search bar stub → V5)

**Deferred:** U9 (chat panel toggle → V8), N47 (syncToServer/bidirectional IMAP sync → V4.1), messaging view layout (V4.1 — toggle saves preference, layout deferred)

---

## Context for All Tasks

**Existing state (do not modify these patterns, extend them):**
- `api/messages.rs` already has unified inbox query (no `account_id` = all active accounts) and single-account filtering via `?account_id=`
- `api/config.rs` uses `config` table (key-value) for theme storage — same pattern for `view_mode`
- `models/message.rs` has `MessageSummary` with `labels`, `ai_category` fields already available
- Frontend `api.ts` has typed `request<T>()` helper, `Inbox.svelte` loads messages on mount
- `Sidebar.svelte` has static nav items — extend for account switcher
- `MessageRow.svelte` renders individual rows — add checkbox for selection
- 26 tests currently passing
- Router: `svelte-spa-router` hash-based in `App.svelte`

---

### Task 1: Backend — Batch Message Update Endpoint

Add `PATCH /api/messages/batch` for bulk operations (archive, delete, mark read/unread, star/unstar, add/remove label). This is N28 in the breadboard.

**Files:**
- Modify: `src/models/message.rs` (add `batch_update` function)
- Modify: `src/api/messages.rs` (add `batch_update_messages` handler)
- Modify: `src/main.rs` (wire `patch` route)

**Step 1: Write the failing test**

Add to `src/models/message.rs` in `mod tests`:

```rust
    #[test]
    fn test_batch_update_archive() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Msg 1", false);
        msg1.message_id = Some("<batch-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1);

        let mut msg2 = make_insert_message(&account.id, "INBOX", "Msg 2", false);
        msg2.message_id = Some("<batch-2@example.com>".to_string());
        let id2 = InsertMessage::insert(&conn, &msg2);

        let mut msg3 = make_insert_message(&account.id, "INBOX", "Msg 3", false);
        msg3.message_id = Some("<batch-3@example.com>".to_string());
        let _id3 = InsertMessage::insert(&conn, &msg3);

        let updated = batch_update(&conn, &[&id1, &id2], "archive");
        assert_eq!(updated, 2);

        // Archived messages should no longer appear in INBOX
        let inbox = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 50, 0);
        assert_eq!(inbox.len(), 1);

        // They should be in Archive folder
        let archived = MessageSummary::list_by_folder(&conn, &account.id, "Archive", 50, 0);
        assert_eq!(archived.len(), 2);
    }

    #[test]
    fn test_batch_update_delete() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Delete me", false);
        msg1.message_id = Some("<del-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1);

        let updated = batch_update(&conn, &[&id1], "delete");
        assert_eq!(updated, 1);

        // Soft-deleted, not in inbox
        let inbox = MessageSummary::list_by_folder(&conn, &account.id, "INBOX", 50, 0);
        assert_eq!(inbox.len(), 0);
    }

    #[test]
    fn test_batch_update_read_unread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Unread", false);
        msg1.message_id = Some("<read-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1);

        // Mark as read
        let updated = batch_update(&conn, &[&id1], "mark_read");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(detail.is_read);

        // Mark as unread
        let updated = batch_update(&conn, &[&id1], "mark_unread");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(!detail.is_read);
    }

    #[test]
    fn test_batch_update_star() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg1 = make_insert_message(&account.id, "INBOX", "Star me", false);
        msg1.message_id = Some("<star-1@example.com>".to_string());
        let id1 = InsertMessage::insert(&conn, &msg1);

        let updated = batch_update(&conn, &[&id1], "star");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(detail.is_starred);

        let updated = batch_update(&conn, &[&id1], "unstar");
        assert_eq!(updated, 1);
        let detail = MessageDetail::get_by_id(&conn, &id1).unwrap();
        assert!(!detail.is_starred);
    }
```

**Step 2: Run test to verify it fails**

Run: `cargo test models::message::tests::test_batch_update -- --nocapture 2>&1`
Expected: FAIL — `batch_update` function doesn't exist.

**Step 3: Implement batch_update**

Add to `src/models/message.rs` after `finalize_draft_as_sent`:

```rust
/// Batch update messages by action. Returns count of rows affected.
/// Supported actions: archive, delete, mark_read, mark_unread, star, unstar.
pub fn batch_update(conn: &Conn, ids: &[&str], action: &str) -> usize {
    if ids.is_empty() {
        return 0;
    }

    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(", ");

    let sql = match action {
        "archive" => format!(
            "UPDATE messages SET folder = 'Archive', updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "delete" => format!(
            "UPDATE messages SET is_deleted = 1, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "mark_read" => format!(
            "UPDATE messages SET is_read = 1, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "mark_unread" => format!(
            "UPDATE messages SET is_read = 0, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "star" => format!(
            "UPDATE messages SET is_starred = 1, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        "unstar" => format!(
            "UPDATE messages SET is_starred = 0, updated_at = unixepoch() WHERE id IN ({in_clause})"
        ),
        _ => return 0,
    };

    let params: Vec<&dyn rusqlite::types::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params.as_slice())
        .unwrap_or(0)
}
```

**Step 4: Add the API handler**

Add to `src/api/messages.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct BatchUpdateRequest {
    pub ids: Vec<String>,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateResponse {
    pub updated: usize,
}

pub async fn batch_update_messages(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchUpdateRequest>,
) -> Result<Json<BatchUpdateResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let valid_actions = ["archive", "delete", "mark_read", "mark_unread", "star", "unstar"];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let id_refs: Vec<&str> = req.ids.iter().map(|s| s.as_str()).collect();
    let updated = message::batch_update(&conn, &id_refs, &req.action);

    Ok(Json(BatchUpdateResponse { updated }))
}
```

**Step 5: Wire route in main.rs**

Update the routing import to include `patch`:

```rust
use axum::{Router, routing::{get, put, post, delete, patch}};
```

Add after the existing `/messages/{id}/read` route:

```rust
.route("/messages/batch", patch(api::messages::batch_update_messages))
```

**Step 6: Run tests to verify they pass**

Run: `cargo test 2>&1`
Expected: All 30 tests pass (26 previous + 4 new batch tests).

**Step 7: Commit**

```bash
git add src/models/message.rs src/api/messages.rs src/main.rs
git commit -m "feat(v4): batch message update endpoint — archive, delete, read, star"
```

---

### Task 2: Backend — View Mode Config Persistence

Store the user's preferred view mode (traditional/messaging) using the existing config key-value table.

**Files:**
- Modify: `src/api/config.rs` (add view_mode to ConfigResponse, add set_view_mode handler)
- Modify: `src/main.rs` (wire new route)

**Step 1: Update ConfigResponse and add view_mode handler**

In `src/api/config.rs`, update `ConfigResponse` and add a new handler:

```rust
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub theme: String,
    pub view_mode: String,
}

#[derive(Debug, Deserialize)]
pub struct SetViewModeRequest {
    pub view_mode: String,
}
```

Update `get_config` to also read `view_mode`:

```rust
pub async fn get_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let theme: String = conn
        .query_row("SELECT value FROM config WHERE key = 'theme'", [], |row| row.get(0))
        .unwrap_or_else(|_| "system".to_string());

    let view_mode: String = conn
        .query_row("SELECT value FROM config WHERE key = 'view_mode'", [], |row| row.get(0))
        .unwrap_or_else(|_| "traditional".to_string());

    Ok(Json(ConfigResponse { theme, view_mode }))
}
```

Add `set_view_mode` handler:

```rust
pub async fn set_view_mode(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetViewModeRequest>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    match input.view_mode.as_str() {
        "traditional" | "messaging" => {}
        _ => return Err(StatusCode::BAD_REQUEST),
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    conn.execute(
        "INSERT INTO config (key, value) VALUES ('view_mode', ?1)
         ON CONFLICT(key) DO UPDATE SET value = ?1",
        rusqlite::params![input.view_mode],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let theme: String = conn
        .query_row("SELECT value FROM config WHERE key = 'theme'", [], |row| row.get(0))
        .unwrap_or_else(|_| "system".to_string());

    Ok(Json(ConfigResponse {
        theme,
        view_mode: input.view_mode,
    }))
}
```

**Step 2: Wire route in main.rs**

Add after the existing `/config/theme` route:

```rust
.route("/config/view-mode", put(api::config::set_view_mode))
```

**Step 3: Build and verify**

Run: `cargo build 2>&1`
Expected: Compiles.

Run: `cargo test 2>&1`
Expected: All 30 tests pass (no new tests — config table is already tested by theme tests).

**Step 4: Commit**

```bash
git add src/api/config.rs src/main.rs
git commit -m "feat(v4): view mode config persistence — traditional/messaging toggle"
```

---

### Task 3: Frontend — API Client Updates

Add batch update, view mode, and account list API methods to `api.ts`.

**Files:**
- Modify: `web/src/lib/api.ts`

**Step 1: Add new API methods**

Add `batch` method to `messages` namespace:

```typescript
messages: {
    // ... existing list, get, markRead methods ...
    batch: (ids: string[], action: string) =>
      request<{ updated: number }>('/api/messages/batch', {
        method: 'PATCH',
        body: JSON.stringify({ ids, action }),
      }),
},
```

Update `config` namespace:

```typescript
config: {
    get: () => request<{ theme: string; view_mode: string }>('/api/config'),
    setTheme: (theme: string) => request<void>('/api/config/theme', { method: 'PUT', body: JSON.stringify({ theme }) }),
    setViewMode: (view_mode: string) =>
      request<{ theme: string; view_mode: string }>('/api/config/view-mode', {
        method: 'PUT',
        body: JSON.stringify({ view_mode }),
      }),
},
```

**Step 2: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 3: Commit**

```bash
git add web/src/lib/api.ts
git commit -m "feat(v4): API client — batch update, view mode, config methods"
```

---

### Task 4: Frontend — Category Tabs

Add static category tabs (All, Primary, Updates, Social, Promotions) to the inbox header. Tabs filter messages by `labels` JSON field. "All" shows everything.

**Files:**
- Modify: `web/src/pages/Inbox.svelte` (add category tab bar, pass category to API)
- Modify: `web/src/lib/api.ts` (add `category` param to `messages.list`)

**Step 1: Add category parameter to messages.list in api.ts**

In the `messages.list` function, add category support:

```typescript
list: (params?: { account_id?: string; folder?: string; category?: string; limit?: number; offset?: number }) => {
    const query = new URLSearchParams();
    if (params?.account_id) query.set('account_id', params.account_id);
    if (params?.folder) query.set('folder', params.folder);
    if (params?.category) query.set('category', params.category);
    if (params?.limit) query.set('limit', String(params.limit));
    if (params?.offset) query.set('offset', String(params.offset));
    return request<{ messages: any[]; unread_count: number; total: number }>(`/api/messages?${query}`);
},
```

**Step 2: Add category filtering to backend**

In `src/api/messages.rs`, add `category` to `ListMessagesParams`:

```rust
#[derive(Debug, Deserialize)]
pub struct ListMessagesParams {
    pub account_id: Option<String>,
    pub folder: Option<String>,
    pub category: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
```

In `list_messages`, add category filtering to both the single-account and unified queries. The category is matched against the `labels` JSON column using SQLite's `LIKE` (labels is a JSON array string).

For the single-account query, change the SQL to:

```rust
let (messages, unread, total) = if let Some(ref account_id) = params.account_id {
    let category_filter = if let Some(ref cat) = params.category {
        format!(" AND m.labels LIKE '%\"{}%'", cat.replace('\'', "''"))
    } else {
        String::new()
    };

    let query = format!(
        "SELECT id, account_id, thread_id, folder, from_address, from_name, subject, snippet,
                date, is_read, is_starred, has_attachments, labels, ai_priority_label, ai_category
         FROM messages m
         WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0{category_filter}
         ORDER BY m.date DESC
         LIMIT ?3 OFFSET ?4"
    );
    // ... execute with params
```

Similarly for the unified query. Use the `m` alias consistently.

**Step 3: Add category tab bar to Inbox.svelte**

Add after the existing header bar, before `<SyncStatus />`:

```svelte
  let activeCategory = $state('');

  const categories = [
    { id: '', label: 'All' },
    { id: 'primary', label: 'Primary' },
    { id: 'updates', label: 'Updates' },
    { id: 'social', label: 'Social' },
    { id: 'promotions', label: 'Promotions' },
  ];
```

Update `loadMessages` to pass category:

```typescript
const res = await api.messages.list({ category: activeCategory || undefined });
```

Add the tab bar HTML between the inbox header and SyncStatus:

```svelte
  <!-- Category tabs -->
  <div class="px-4 border-b border-gray-200 dark:border-gray-700 flex gap-1 overflow-x-auto">
    {#each categories as cat}
      <button
        class="px-3 py-2 text-sm whitespace-nowrap border-b-2 transition-colors
               {activeCategory === cat.id
                 ? 'border-blue-600 text-blue-600 font-medium'
                 : 'border-transparent text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'}"
        onclick={() => { activeCategory = cat.id; loadMessages(); }}
      >
        {cat.label}
      </button>
    {/each}
  </div>
```

**Step 4: Build and verify**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

Run: `cargo build 2>&1`
Expected: Compiles.

**Step 5: Commit**

```bash
git add web/src/pages/Inbox.svelte web/src/lib/api.ts src/api/messages.rs
git commit -m "feat(v4): category tabs — filter inbox by Primary, Updates, Social, Promotions"
```

---

### Task 5: Frontend — Message Selection + Bulk Action Bar

Add selection checkboxes to message rows and a bulk action bar that appears when messages are selected.

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte` (add checkbox)
- Modify: `web/src/components/inbox/MessageList.svelte` (manage selection state)
- Modify: `web/src/pages/Inbox.svelte` (add bulk action bar, handle actions)

**Step 1: Update MessageRow with checkbox**

In `web/src/components/inbox/MessageRow.svelte`, add a `selected` prop and checkbox:

```svelte
<script lang="ts">
  interface Message {
    id: string;
    from_name?: string;
    from_address: string;
    subject?: string;
    snippet?: string;
    date: string;
    is_read: boolean;
    has_attachments?: boolean;
  }

  let {
    message,
    onclick,
    selected = false,
    onselect,
  }: {
    message: Message;
    onclick: (id: string) => void;
    selected?: boolean;
    onselect?: (id: string, checked: boolean) => void;
  } = $props();

  let senderDisplay = $derived(message.from_name || message.from_address);
  let subjectDisplay = $derived(message.subject || '(no subject)');

  let formattedDate = $derived.by(() => {
    const msgDate = new Date(message.date);
    const now = new Date();
    const isToday =
      msgDate.getFullYear() === now.getFullYear() &&
      msgDate.getMonth() === now.getMonth() &&
      msgDate.getDate() === now.getDate();

    if (isToday) {
      return msgDate.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    }
    return msgDate.toLocaleDateString([], { month: 'short', day: 'numeric' });
  });

  function handleCheckbox(e: Event) {
    e.stopPropagation();
    const target = e.target as HTMLInputElement;
    onselect?.(message.id, target.checked);
  }
</script>

<button
  class="w-full text-left px-4 py-3 border-b border-gray-100 dark:border-gray-800 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors flex items-start gap-3
         {selected ? 'bg-blue-50 dark:bg-blue-900/20' : ''}"
  onclick={() => onclick(message.id)}
>
  <!-- Checkbox -->
  <div class="pt-1 flex-shrink-0">
    <input
      type="checkbox"
      checked={selected}
      onclick={handleCheckbox}
      class="w-4 h-4 rounded border-gray-300 dark:border-gray-600 text-blue-600 focus:ring-blue-500"
    />
  </div>

  <!-- Unread indicator -->
  <div class="pt-1.5 w-3 flex-shrink-0">
    {#if !message.is_read}
      <div class="w-2.5 h-2.5 rounded-full bg-blue-500"></div>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 min-w-0">
    <div class="flex items-baseline gap-2">
      <span class="text-sm truncate {message.is_read ? 'text-gray-700 dark:text-gray-300' : 'font-semibold text-gray-900 dark:text-gray-100'}">
        {senderDisplay}
      </span>
      <span class="flex-shrink-0 text-xs text-gray-400 dark:text-gray-500 ml-auto">
        {#if message.has_attachments}
          <span class="mr-1.5" title="Has attachments">{'\u{1F4CE}'}</span>
        {/if}
        {formattedDate}
      </span>
    </div>
    <div class="text-sm truncate {message.is_read ? 'text-gray-600 dark:text-gray-400' : 'font-medium text-gray-800 dark:text-gray-200'}">
      {subjectDisplay}
    </div>
    {#if message.snippet}
      <div class="text-xs text-gray-400 dark:text-gray-500 truncate mt-0.5">
        {message.snippet}
      </div>
    {/if}
  </div>
</button>
```

**Step 2: Update MessageList to pass selection**

In `web/src/components/inbox/MessageList.svelte`:

```svelte
<script lang="ts">
  import MessageRow from './MessageRow.svelte';

  let {
    messages,
    onclick,
    selectedIds = $bindable(new Set<string>()),
  }: {
    messages: any[];
    onclick: (id: string) => void;
    selectedIds?: Set<string>;
  } = $props();

  function handleSelect(id: string, checked: boolean) {
    if (checked) {
      selectedIds = new Set([...selectedIds, id]);
    } else {
      const next = new Set(selectedIds);
      next.delete(id);
      selectedIds = next;
    }
  }
</script>

{#if messages.length === 0}
  <div class="text-center py-16 text-gray-400 dark:text-gray-500">
    <p class="text-lg mb-2">No messages yet</p>
    <p class="text-sm">Add an email account to get started.</p>
  </div>
{:else}
  <div class="divide-y divide-gray-100 dark:divide-gray-800">
    {#each messages as message (message.id)}
      <MessageRow
        {message}
        {onclick}
        selected={selectedIds.has(message.id)}
        onselect={handleSelect}
      />
    {/each}
  </div>
{/if}
```

**Step 3: Add bulk action bar to Inbox.svelte**

Add state and handlers:

```typescript
  let selectedIds = $state(new Set<string>());

  async function handleBulkAction(action: string) {
    if (selectedIds.size === 0) return;
    try {
      await api.messages.batch([...selectedIds], action);
      selectedIds = new Set();
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Bulk action failed';
    }
  }
```

Update the MessageList to bind selectedIds:

```svelte
<MessageList {messages} onclick={handleMessageClick} bind:selectedIds />
```

Add the bulk action bar above the message list (visible when selection > 0):

```svelte
  {#if selectedIds.size > 0}
    <div class="px-4 py-2 bg-blue-50 dark:bg-blue-900/30 border-b border-blue-200 dark:border-blue-800 flex items-center gap-2">
      <span class="text-sm font-medium text-blue-700 dark:text-blue-300">
        {selectedIds.size} selected
      </span>
      <span class="flex-1"></span>
      <button
        class="px-3 py-1 text-xs font-medium bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        onclick={() => handleBulkAction('archive')}
      >Archive</button>
      <button
        class="px-3 py-1 text-xs font-medium bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        onclick={() => handleBulkAction('mark_read')}
      >Mark Read</button>
      <button
        class="px-3 py-1 text-xs font-medium bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        onclick={() => handleBulkAction('mark_unread')}
      >Mark Unread</button>
      <button
        class="px-3 py-1 text-xs font-medium bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        onclick={() => handleBulkAction('star')}
      >Star</button>
      <button
        class="px-3 py-1 text-xs font-medium text-red-600 dark:text-red-400 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
        onclick={() => handleBulkAction('delete')}
      >Delete</button>
      <button
        class="px-3 py-1 text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
        onclick={() => (selectedIds = new Set())}
      >Clear</button>
    </div>
  {/if}
```

**Step 4: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 5: Commit**

```bash
git add web/src/components/inbox/MessageRow.svelte web/src/components/inbox/MessageList.svelte web/src/pages/Inbox.svelte
git commit -m "feat(v4): message selection checkboxes + bulk action bar"
```

---

### Task 6: Frontend — Account Switcher in Sidebar

Add an account switcher to the sidebar that shows all configured accounts and allows filtering the inbox by account. "All Accounts" shows the unified inbox.

**Files:**
- Modify: `web/src/components/Sidebar.svelte` (add account list, switcher)
- Modify: `web/src/pages/Inbox.svelte` (pass/receive active account filter)

**Step 1: Update Sidebar.svelte**

```svelte
<script lang="ts">
  import { push, location } from 'svelte-spa-router';
  import { api } from '../lib/api';

  let accounts = $state<any[]>([]);
  let activeAccountId = $state('');

  const navItems = [
    { path: '/', label: 'Inbox', icon: '\u{1F4E5}' },
    { path: '/setup', label: 'Add Account', icon: '\u{2795}' },
    { path: '/settings', label: 'Settings', icon: '\u{2699}\u{FE0F}' },
  ];

  async function loadAccounts() {
    try {
      accounts = await api.accounts.list();
    } catch { /* ignore */ }
  }

  function selectAccount(id: string) {
    activeAccountId = id;
    // Dispatch custom event for Inbox to pick up
    window.dispatchEvent(new CustomEvent('account-switch', { detail: { accountId: id } }));
    push('/');
  }

  $effect(() => {
    loadAccounts();
  });
</script>

<aside class="w-56 border-r border-gray-200 dark:border-gray-700 flex flex-col">
  <div class="p-4 border-b border-gray-200 dark:border-gray-700">
    <h1 class="text-xl font-bold">Iris</h1>
  </div>

  <!-- Account switcher -->
  {#if accounts.length > 0}
    <div class="p-2 border-b border-gray-200 dark:border-gray-700">
      <p class="px-3 py-1 text-xs font-semibold text-gray-400 uppercase tracking-wider">Accounts</p>
      <button
        class="w-full text-left px-3 py-1.5 rounded text-sm transition-colors
               {activeAccountId === '' ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800 text-gray-600 dark:text-gray-400'}"
        onclick={() => selectAccount('')}
      >
        All Accounts
      </button>
      {#each accounts as account}
        <button
          class="w-full text-left px-3 py-1.5 rounded text-sm transition-colors truncate
                 {activeAccountId === account.id ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800 text-gray-600 dark:text-gray-400'}"
          onclick={() => selectAccount(account.id)}
          title={account.email}
        >
          {account.email}
        </button>
      {/each}
    </div>
  {/if}

  <nav class="flex-1 p-2 space-y-1">
    {#each navItems as item}
      <button
        class="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors
               {$location === item.path ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={() => push(item.path)}
      >
        <span class="mr-2">{item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>
</aside>
```

**Step 2: Listen for account-switch in Inbox.svelte**

In `Inbox.svelte`, add account filter support:

```typescript
  let filterAccountId = $state('');

  function onAccountSwitch(e: Event) {
    const detail = (e as CustomEvent).detail;
    filterAccountId = detail.accountId || '';
    loadMessages();
  }
```

Update `loadMessages` to pass `account_id`:

```typescript
  async function loadMessages() {
    loading = true;
    error = '';
    try {
      const res = await api.messages.list({
        account_id: filterAccountId || undefined,
        category: activeCategory || undefined,
      });
```

Add/remove event listener in `$effect`:

```typescript
  $effect(() => {
    loadMessages();
    wsClient.connect();
    const offNewEmail = wsClient.on('NewEmail', () => { loadMessages(); });
    window.addEventListener('account-switch', onAccountSwitch);
    return () => {
      offNewEmail();
      window.removeEventListener('account-switch', onAccountSwitch);
    };
  });
```

Update `ensureAccountId` to prefer the filtered account:

```typescript
  async function ensureAccountId() {
    if (filterAccountId) {
      activeAccountId = filterAccountId;
      return;
    }
    if (activeAccountId) return;
    try {
      const accounts = await api.accounts.list();
      if (accounts.length > 0) activeAccountId = accounts[0].id;
    } catch { /* ignore */ }
  }
```

**Step 3: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 4: Commit**

```bash
git add web/src/components/Sidebar.svelte web/src/pages/Inbox.svelte
git commit -m "feat(v4): account switcher in sidebar — filter inbox by account"
```

---

### Task 7: Frontend — Thread Actions

Add action buttons to the thread view header: archive, star, mark unread, delete. These call the batch update API for the current thread's messages.

**Files:**
- Modify: `web/src/pages/ThreadView.svelte` (add action buttons in header)

**Step 1: Add thread action handlers**

In `ThreadView.svelte`, add action functions:

```typescript
  async function handleThreadAction(action: string) {
    if (!thread) return;
    const ids = thread.messages.map((m: any) => m.id);
    try {
      await api.messages.batch(ids, action);
      if (action === 'archive' || action === 'delete') {
        push('/');
      } else {
        await loadThread();
      }
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }
```

**Step 2: Add action buttons to thread header**

In the header bar (the `<div>` with `px-4 py-3 border-b`), add action buttons after the thread info:

```svelte
    {#if thread}
      <div class="flex-1 min-w-0">
        <h2 class="text-lg font-semibold truncate">{thread.subject || '(no subject)'}</h2>
        <p class="text-xs text-gray-500 truncate">
          {thread.participants.map((p: any) => p.name || p.email).join(', ')}
          &middot; {thread.message_count} message{thread.message_count === 1 ? '' : 's'}
        </p>
      </div>
      <div class="flex items-center gap-1">
        <button
          class="p-2 text-gray-400 hover:text-yellow-500 transition-colors"
          onclick={() => handleThreadAction('star')}
          title="Star"
        >&#9734;</button>
        <button
          class="p-2 text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
          onclick={() => handleThreadAction('archive')}
          title="Archive"
        >&#128230;</button>
        <button
          class="p-2 text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
          onclick={() => handleThreadAction('mark_unread')}
          title="Mark unread"
        >&#9993;</button>
        <button
          class="p-2 text-gray-400 hover:text-red-500 transition-colors"
          onclick={() => handleThreadAction('delete')}
          title="Delete"
        >&#128465;</button>
      </div>
    {/if}
```

**Step 3: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 4: Commit**

```bash
git add web/src/pages/ThreadView.svelte
git commit -m "feat(v4): thread actions — star, archive, mark unread, delete"
```

---

### Task 8: Frontend — View Mode Toggle

Add a view toggle button in the inbox header to switch between "Traditional" and "Messaging" views. For V4, the toggle saves the preference and changes the layout label. The actual messaging bubble layout ships in V4.1.

**Files:**
- Modify: `web/src/pages/Inbox.svelte` (add view toggle, load/save preference)

**Step 1: Add view mode state and toggle**

```typescript
  let viewMode = $state('traditional');

  async function loadViewMode() {
    try {
      const config = await api.config.get();
      viewMode = config.view_mode || 'traditional';
    } catch { /* ignore */ }
  }

  async function toggleViewMode() {
    const next = viewMode === 'traditional' ? 'messaging' : 'traditional';
    viewMode = next;
    try {
      await api.config.setViewMode(next);
    } catch { /* ignore */ }
  }
```

Call `loadViewMode()` in the `$effect` block alongside `loadMessages()`.

**Step 2: Add toggle button in the inbox header**

Add before the Compose button:

```svelte
    <button
      class="px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors
             {viewMode === 'traditional'
               ? 'border-gray-300 dark:border-gray-600 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800'
               : 'border-blue-300 dark:border-blue-700 text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/30'}"
      onclick={toggleViewMode}
      title="Toggle view mode"
    >
      {viewMode === 'traditional' ? 'Traditional' : 'Messaging'}
    </button>
```

**Step 3: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 4: Commit**

```bash
git add web/src/pages/Inbox.svelte
git commit -m "feat(v4): view mode toggle — traditional/messaging preference"
```

---

### Task 9: Integration Verification

Build everything, run all tests, verify the full V4 feature set compiles and works correctly.

**Files:**
- No new files

**Step 1: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 2: Build backend**

Run: `cargo build 2>&1`
Expected: Compiles with only expected dead_code warnings.

**Step 3: Run all tests**

Run: `cargo test 2>&1`
Expected: All 30 tests pass (26 V1-V3 + 4 new batch tests).

**Step 4: Verify routes are wired**

Check that all new routes compile by reviewing the router in main.rs:
- `PATCH /api/messages/batch` — bulk update
- `PUT /api/config/view-mode` — view mode toggle
- Existing routes unchanged

**Step 5: Final commit (if any fixups needed)**

No commit if everything passes on first try.
