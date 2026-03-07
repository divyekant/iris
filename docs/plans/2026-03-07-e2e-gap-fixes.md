# E2E Gap Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 17 gaps found during E2E testing to bring Iris to release-quality.

**Architecture:** Fixes span backend (Rust) and frontend (Svelte 5). P0 fixes critical bugs, P1 adds missing core features, P2 polishes UX. Each task is independently committable.

**Tech Stack:** Rust (mailparse for RFC 2047), Svelte 5, Tailwind CSS 4, lucide-svelte for icons.

---

## P0 — Critical Bugs

### Task 1: Fix date display (epoch seconds vs milliseconds)

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte:26`
- Modify: `web/src/pages/Search.svelte:67`

All dates show "Jan 21" because JS `new Date()` expects milliseconds but backend sends epoch seconds.

**Step 1: Fix MessageRow date parsing**

In `web/src/components/inbox/MessageRow.svelte`, line 26, change:
```typescript
const msgDate = new Date(message.date);
```
to:
```typescript
const msgDate = new Date(typeof message.date === 'number' && message.date < 1e12 ? message.date * 1000 : message.date);
```

**Step 2: Fix Search date formatting**

In `web/src/pages/Search.svelte`, line 67, `formatDate` already does `* 1000` — this one is correct. Verify no other date consumers exist.

**Step 3: Verify in browser**

Dates should now show relative format (today = time, older = "Mar 6", etc).

**Step 4: Commit**
```bash
git add web/src/components/inbox/MessageRow.svelte
git commit -m "fix: date display — convert epoch seconds to milliseconds"
```

---

### Task 2: Decode RFC 2047 encoded subjects

**Files:**
- Modify: `src/imap/sync.rs:222-227`
- Modify: `Cargo.toml` (add `charset` crate if needed)

**Step 1: Add RFC 2047 decoding function to sync.rs**

In `src/imap/sync.rs`, add a helper above `parse_fetch`:
```rust
/// Decode RFC 2047 encoded-words in a header value.
/// Falls back to lossy UTF-8 if decoding fails.
fn decode_header_value(raw: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(raw).to_string();
    // If it doesn't contain encoded-words, return as-is
    if !lossy.contains("=?") {
        return lossy;
    }
    // Use mailparse to decode RFC 2047
    match mailparse::headers::decode_header(&lossy) {
        Ok(decoded) => decoded,
        Err(_) => lossy,
    }
}
```

Note: `mailparse` (already in Cargo.toml) provides `mailparse::headers::decode_header` which handles `=?charset?encoding?text?=` patterns.

**Step 2: Use the decoder for subject extraction**

Replace lines 222-227 in sync.rs:
```rust
let subject = envelope.and_then(|env| {
    env.subject
        .as_ref()
        .map(|s| String::from_utf8_lossy(s).to_string())
});
```
with:
```rust
let subject = envelope.and_then(|env| {
    env.subject
        .as_ref()
        .map(|s| decode_header_value(s))
});
```

**Step 3: Also decode from_name**

Apply same pattern to `from_name` extraction (around line 193-207 in parse_fetch). The sender name can also be RFC 2047 encoded.

**Step 4: Add a test**

```rust
#[test]
fn test_decode_header_value() {
    let encoded = b"=?UTF-8?Q?BadBo_1.0_Making_History_=F0=9F=90=B0?=";
    let decoded = decode_header_value(encoded);
    assert!(decoded.contains("BadBo 1.0 Making History"));
}
```

**Step 5: Fix existing data — re-decode stored subjects**

Add a one-time migration or API endpoint to re-decode existing subjects in the database. A simple approach: add a `/api/admin/redecode-subjects` endpoint that reads raw_headers for each message and re-extracts the subject.

Alternative (simpler): just re-sync the account — the next sync will overwrite with decoded subjects. Document this as a known limitation for now.

**Step 6: Commit**
```bash
git add src/imap/sync.rs
git commit -m "fix: decode RFC 2047 encoded email subjects and sender names"
```

---

### Task 3: Force white background for email body iframe

**Files:**
- Modify: `web/src/components/thread/EmailBody.svelte:41`

**Step 1: Add background-color to iframe body style**

In `EmailBody.svelte`, line 41, change:
```css
body{margin:0;padding:8px;font-family:-apple-system,system-ui,sans-serif;font-size:14px;line-height:1.6;color:#333;}
```
to:
```css
body{margin:0;padding:8px;font-family:-apple-system,system-ui,sans-serif;font-size:14px;line-height:1.6;color:#333;background:#fff;}
```

**Step 2: Allow style attribute in DOMPurify**

Most HTML marketing emails rely heavily on inline `style` attributes for layout (tables, colors, widths). Currently `style` is stripped. Add it to ALLOWED_ATTR:

In line 17-20, add `'style'` to the array:
```typescript
ALLOWED_ATTR: [
  'href', 'src', 'alt', 'target', 'width', 'height',
  'colspan', 'rowspan', 'style',
],
```

This is safe inside a sandboxed iframe with `allow-same-origin` only (no scripts).

**Step 3: Verify StockX email renders correctly**

Open a StockX email in thread view — HTML should now render with proper styling and white background.

**Step 4: Commit**
```bash
git add web/src/components/thread/EmailBody.svelte
git commit -m "fix: white background for email body, allow inline styles in sanitizer"
```

---

### Task 4: Diagnose and fix email send

**Files:**
- Modify: `src/api/compose.rs` (improve error reporting)
- Modify: `web/src/components/compose/ComposeModal.svelte` (surface errors)

**Step 1: Test send from API directly**

```bash
TOKEN="<session_token>"
curl -v -X POST http://localhost:3000/api/send \
  -H "X-Session-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "<account_id>",
    "to": ["tataunistorescribe1@gmail.com"],
    "subject": "Test from Iris",
    "body_text": "Hello from Iris!"
  }'
```

Examine the error response. Common issues:
- OAuth token expired (502 with token refresh error)
- Gmail "Less secure apps" not enabled
- Gmail SMTP requires app-specific password or OAuth
- Missing SMTP host/port in account record

**Step 2: Improve error surfacing in ComposeModal**

In `ComposeModal.svelte`, the `handleSend` function (line 139-176) should display the error to the user. Currently errors are silently caught. Add visible error state:

After the catch block, display the error message to the user in the compose UI.

**Step 3: Verify send works**

Send a test email and confirm it arrives.

**Step 4: Commit**
```bash
git add src/api/compose.rs web/src/components/compose/ComposeModal.svelte
git commit -m "fix: surface send errors in compose modal, improve SMTP diagnostics"
```

---

## P1 — Core Missing Features

### Task 5: Install lucide-svelte and replace emoji icons

**Files:**
- Modify: `web/package.json`
- Modify: `web/src/pages/ThreadView.svelte:168-188`
- Modify: `web/src/components/inbox/MessageRow.svelte:95`
- Modify: `web/src/pages/Search.svelte:167`

**Step 1: Install lucide-svelte**

```bash
cd web && npm install lucide-svelte
```

**Step 2: Replace thread action emoji with Lucide icons**

In `ThreadView.svelte`, replace the emoji icons (lines 168-188):
- `&#9734;` (star) → `<Star size={16} />`
- `&#128230;` (archive) → `<Archive size={16} />`
- `&#9993;` (envelope) → `<MailOpen size={16} />`
- `&#128465;` (trash) → `<Trash2 size={16} />`
- `&#10024;` (sparkles) → `<Sparkles size={14} />`

Import at top:
```typescript
import { Star, Archive, MailOpen, Trash2, Sparkles } from 'lucide-svelte';
```

**Step 3: Replace attachment emoji**

In `MessageRow.svelte` line 95 and `Search.svelte` line 167:
- `&#128206;` (paperclip) → `<Paperclip size={12} />`

Import `Paperclip` from lucide-svelte.

**Step 4: Commit**
```bash
git add web/package.json web/package-lock.json web/src/pages/ThreadView.svelte web/src/components/inbox/MessageRow.svelte web/src/pages/Search.svelte
git commit -m "feat(ui): replace emoji icons with lucide-svelte icon library"
```

---

### Task 6: Add folder navigation (Sent, Drafts, Starred, Archive, Trash)

**Files:**
- Create: `web/src/pages/FolderView.svelte`
- Modify: `web/src/App.svelte` (add routes)
- Modify: `web/src/lib/api.ts` (no changes needed — folder param already supported)

**Step 1: Create FolderView page**

Create `web/src/pages/FolderView.svelte` — a reusable page that takes a folder name prop and uses `api.messages.list({ folder })` to load messages. Structure identical to Inbox but without category tabs.

Key props: `folder: string`, `title: string`

**Step 2: Add routes in App.svelte**

```typescript
import FolderView from './pages/FolderView.svelte';

const routes = {
  '/': Inbox,
  '/search': Search,
  '/thread/:id': ThreadView,
  '/setup': AccountSetup,
  '/setup/*': AccountSetup,
  '/settings': Settings,
  '/drafts': wrap({ component: FolderView, props: { folder: 'Drafts', title: 'Drafts' } }),
  '/starred': wrap({ component: FolderView, props: { folder: 'Starred', title: 'Starred' } }),
  '/sent': wrap({ component: FolderView, props: { folder: 'Sent', title: 'Sent' } }),
  '/archive': wrap({ component: FolderView, props: { folder: 'Archive', title: 'Archive' } }),
  '/trash': wrap({ component: FolderView, props: { folder: 'Trash', title: 'Trash' } }),
};
```

Import `wrap` from `svelte-spa-router/wrap`.

**Step 3: Add Sent to TopNav**

In `TopNav.svelte`, update `navItems` (line 45-49) to include Sent:
```typescript
const navItems = [
  { path: '/', label: 'Inbox', icon: 'inbox' },
  { path: '/search', label: 'Search', icon: 'search' },
  { path: '/sent', label: 'Sent', icon: 'send' },
  { path: '/drafts', label: 'Drafts', icon: 'file-text' },
];
```

**Step 4: Verify backend folder filtering**

The backend at `src/api/messages.rs` already supports `?folder=X`. Verify it works:
```bash
curl -s -H "X-Session-Token: $TOKEN" "http://localhost:3000/api/messages?folder=Sent"
```

**Step 5: Commit**
```bash
git add web/src/pages/FolderView.svelte web/src/App.svelte web/src/components/TopNav.svelte
git commit -m "feat: add folder navigation — Sent, Drafts, Starred, Archive, Trash"
```

---

### Task 7: Add inbox pagination

**Files:**
- Modify: `web/src/pages/Inbox.svelte`

**Step 1: Add pagination state**

Add to Inbox.svelte script:
```typescript
let page = $state(1);
const PAGE_SIZE = 25;
```

**Step 2: Update loadMessages to use limit/offset**

```typescript
const res = await api.messages.list({
  account_id: filterAccountId || undefined,
  category: activeCategory || undefined,
  limit: PAGE_SIZE,
  offset: (page - 1) * PAGE_SIZE,
});
```

**Step 3: Add pagination controls**

After the message list, add prev/next buttons:
```svelte
{#if total > PAGE_SIZE}
  <div class="flex items-center justify-center gap-4 py-3 border-t" style="border-color: var(--iris-color-border-subtle);">
    <button disabled={page <= 1} onclick={() => { page--; loadMessages(); }}>Previous</button>
    <span class="text-xs" style="color: var(--iris-color-text-faint);">
      {(page - 1) * PAGE_SIZE + 1}-{Math.min(page * PAGE_SIZE, total)} of {total}
    </span>
    <button disabled={page * PAGE_SIZE >= total} onclick={() => { page++; loadMessages(); }}>Next</button>
  </div>
{/if}
```

**Step 4: Reset page on filter change**

When activeCategory changes or account switches, reset `page = 1`.

**Step 5: Commit**
```bash
git add web/src/pages/Inbox.svelte
git commit -m "feat: add inbox pagination with 25 messages per page"
```

---

### Task 8: Add quick actions on message row hover

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte`

**Step 1: Add hover action buttons**

Add a hover-revealed action bar to the right side of each message row. On hover, show: Archive, Trash, Star, Mark Read/Unread.

Use Lucide icons: `Archive`, `Trash2`, `Star`, `Mail`/`MailOpen`.

These call `api.messages.batch([message.id], action)` directly, then dispatch an event to reload the inbox.

**Step 2: Style with opacity transition**

Actions hidden by default (`opacity-0`), revealed on row hover (`group-hover:opacity-100`). Add `group` class to the parent button.

**Step 3: Commit**
```bash
git add web/src/components/inbox/MessageRow.svelte
git commit -m "feat(ui): add hover quick actions on inbox message rows"
```

---

### Task 9: Add refresh button + background polling

**Files:**
- Modify: `web/src/pages/Inbox.svelte`
- Modify: `web/src/components/TopNav.svelte`

**Step 1: Add refresh button next to SyncStatus**

In Inbox header, add a refresh icon button that calls `loadMessages()`:
```svelte
<button onclick={loadMessages} title="Refresh">
  <RefreshCw size={14} />
</button>
```

**Step 2: Add background polling**

In Inbox.svelte `$effect`, add an interval:
```typescript
const pollInterval = setInterval(() => { loadMessages(); }, 60000);
return () => {
  clearInterval(pollInterval);
  offNewEmail();
  // ... other cleanup
};
```

Poll every 60 seconds as fallback when IDLE isn't connected.

**Step 3: Commit**
```bash
git add web/src/pages/Inbox.svelte
git commit -m "feat: add manual refresh button and 60s background polling"
```

---

### Task 10: Add AI retrigger button

**Files:**
- Modify: `web/src/pages/Inbox.svelte` or `web/src/pages/Settings.svelte`
- Modify: `src/api/queue_status.rs` (add reprocess endpoint)

**Step 1: Add backend endpoint**

Add `POST /api/ai/reprocess` endpoint that finds all messages with `ai_priority_label IS NULL` and enqueues `ai_classify` jobs for them.

**Step 2: Add API client method**

```typescript
reprocess: () => request<{ enqueued: number }>('/api/ai/reprocess', { method: 'POST' }),
```

**Step 3: Add button in Settings AI section**

Add a "Reprocess untagged messages" button that calls the endpoint and shows the count.

**Step 4: Commit**
```bash
git add src/api/queue_status.rs web/src/pages/Settings.svelte web/src/lib/api.ts
git commit -m "feat: add AI reprocess button for untagged messages"
```

---

## P2 — Polish

### Task 11: Fix checkbox + unread dot spacing

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte:62-80`

**Step 1: Adjust spacing**

Change gap from `gap-3` to `gap-2` on the row, and adjust the unread dot container width from `w-3` to `w-2.5`. Add `items-center` instead of `items-start` for vertical alignment.

**Step 2: Commit**
```bash
git add web/src/components/inbox/MessageRow.svelte
git commit -m "fix(ui): improve checkbox and unread indicator alignment"
```

---

### Task 12: Make chat panel resizable

**Files:**
- Modify: `web/src/components/ChatPanel.svelte`

**Step 1: Add drag handle**

Add a vertical resize handle on the left border of the chat panel. Track mouse drag to adjust width between 280px and 600px. Store width in localStorage.

**Step 2: Replace fixed w-80 with dynamic width**

Change `class="w-80"` to `style="width: {chatWidth}px"` with reactive state.

**Step 3: Commit**
```bash
git add web/src/components/ChatPanel.svelte
git commit -m "feat(ui): make chat panel resizable with drag handle"
```

---

### Task 13: Improve compose button states

**Files:**
- Modify: `web/src/components/compose/ComposeModal.svelte`

**Step 1: Add hover/active transitions**

Add hover brightness, active scale, and disabled opacity to Send, Save Draft, and AI Assist buttons. Use CSS transitions matching design token `--iris-transition-normal`.

**Step 2: Commit**
```bash
git add web/src/components/compose/ComposeModal.svelte
git commit -m "fix(ui): add hover and active states to compose buttons"
```

---

### Task 14: Improve search UX

**Files:**
- Modify: `web/src/pages/Search.svelte`

**Step 1: Add keyboard shortcut**

Cmd+K or / to focus search input from anywhere (via window keydown listener).

**Step 2: Better empty state**

Replace the plain text with search suggestions (recent searches, example queries).

**Step 3: Add result count in header**

Show "X results for 'query'" in the results area header.

**Step 4: Commit**
```bash
git add web/src/pages/Search.svelte
git commit -m "feat(ui): improve search with keyboard shortcut and better empty state"
```

---

### Task 15: Add labels display

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte`
- Modify: `src/imap/sync.rs` (parse Gmail X-GM-LABELS)

**Step 1: Parse labels from IMAP**

Gmail provides `X-GM-LABELS` in FETCH. Parse these during sync and store in the `labels` JSON column.

**Step 2: Display labels as pills**

In MessageRow, if `message.labels` is non-empty, show colored pill badges next to the subject.

**Step 3: Commit**
```bash
git add src/imap/sync.rs web/src/components/inbox/MessageRow.svelte
git commit -m "feat: parse and display Gmail labels"
```

---

### Task 16: New email notification indicator

**Files:**
- Modify: `web/src/pages/Inbox.svelte`

**Step 1: Show toast on new email**

When WebSocket `NewEmail` event fires, show a brief toast notification with the sender and subject before auto-dismissing.

**Step 2: Update unread badge in nav**

Pass unread count up to TopNav and show it next to the Inbox nav item.

**Step 3: Commit**
```bash
git add web/src/pages/Inbox.svelte web/src/components/TopNav.svelte
git commit -m "feat(ui): add new email toast notification and nav unread badge"
```

---

### Task 17: Add Sent folder link to TopNav overflow

Already handled in Task 6 — Sent added to primary nav items.

---

## Execution Order

1. Task 1 (dates) — 2 min, immediate visual fix
2. Task 3 (white bg + styles) — 2 min, immediate visual fix
3. Task 2 (RFC 2047) — 10 min, backend fix
4. Task 4 (send diagnosis) — 10 min, debug + fix
5. Task 5 (lucide icons) — 10 min, visual consistency
6. Task 11 (row spacing) — 2 min, visual fix
7. Task 6 (folders) — 15 min, new page + routes
8. Task 7 (pagination) — 10 min, inbox enhancement
9. Task 8 (quick actions) — 10 min, inbox enhancement
10. Task 9 (refresh + polling) — 5 min, reliability
11. Task 10 (AI retrigger) — 10 min, backend + UI
12. Task 12 (chat resize) — 10 min, UX improvement
13. Task 13 (compose polish) — 5 min, button states
14. Task 14 (search UX) — 10 min, improvements
15. Task 15 (labels) — 15 min, IMAP + UI
16. Task 16 (notifications) — 10 min, toast + badge
