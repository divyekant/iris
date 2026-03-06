# UI Overhaul Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current sidebar layout with Option C (top nav), implement a CSS custom property token system via Kalos, and refactor all components to use design tokens for full skinability.

**Architecture:** CSS custom properties in `kalos-tokens.css` define all visual tokens (colors, spacing, radii, typography). Tailwind references these via `var()`. Components never use hardcoded hex values — only token classes or CSS vars. Brand switching via `data-brand` attribute on `<html>`.

**Tech Stack:** Svelte 5, Tailwind CSS 4, CSS custom properties, Kalos token system

**Design Reference:** `docs/plans/2026-03-06-ui-overhaul-design.md` and `docs/designs/iris-ui-overhaul.pen` (14 screens)

---

## Task 1: Token Foundation — CSS Custom Properties

**Files:**
- Create: `web/src/tokens.css`
- Modify: `web/src/app.css`

**Step 1: Create the token CSS file**

Create `web/src/tokens.css` with all design tokens as CSS custom properties:

```css
/* Design tokens — generated from .kalos.yaml */
/* Do not edit manually. Regenerate with: /kalos sync */

:root {
  /* Dark theme (default) */
  --iris-color-primary: #d4af37;
  --iris-color-secondary: #f5d060;
  --iris-color-bg: #0a0a0a;
  --iris-color-bg-elevated: #111111;
  --iris-color-bg-surface: #1a1a1a;
  --iris-color-text: #e5e5e5;
  --iris-color-text-muted: #a3a3a3;
  --iris-color-text-faint: #666666;
  --iris-color-border: rgba(212, 175, 55, 0.15);
  --iris-color-border-subtle: rgba(255, 255, 255, 0.06);
  --iris-color-accent-border: rgba(212, 175, 55, 0.15);
  --iris-color-overlay: rgba(0, 0, 0, 0.6);
  --iris-color-unread: #d4af37;

  /* Semantic */
  --iris-color-success: #16A34A;
  --iris-color-warning: #CA8A04;
  --iris-color-error: #DC2626;
  --iris-color-info: #2563EB;

  /* Typography */
  --iris-font-family: 'Inter', system-ui, sans-serif;
  --iris-font-mono: 'SF Mono', 'Cascadia Mono', 'Menlo', monospace;
  --iris-font-base-size: 16px;

  /* Spacing */
  --iris-spacing-base: 4px;

  /* Radii */
  --iris-radius-none: 0px;
  --iris-radius-sm: 4px;
  --iris-radius-md: 8px;
  --iris-radius-lg: 12px;
  --iris-radius-full: 9999px;

  /* Transitions */
  --iris-transition-fast: 120ms ease;
  --iris-transition-normal: 200ms ease;
}

[data-brand="light"] {
  --iris-color-primary: #b8960f;
  --iris-color-secondary: #d4af37;
  --iris-color-bg: #FAF9F6;
  --iris-color-bg-elevated: #FFFEF9;
  --iris-color-bg-surface: #F0EDE6;
  --iris-color-text: #2C2418;
  --iris-color-text-muted: #7A7060;
  --iris-color-text-faint: #A69E90;
  --iris-color-border: #E8E2D6;
  --iris-color-border-subtle: rgba(0, 0, 0, 0.06);
  --iris-color-accent-border: rgba(184, 150, 15, 0.18);
  --iris-color-overlay: rgba(0, 0, 0, 0.3);
  --iris-color-unread: #b8960f;
}
```

**Step 2: Import tokens in app.css**

Replace the contents of `web/src/app.css`:

```css
@import "tailwindcss";
@import "./tokens.css";

/* Base styles using tokens */
html {
  font-family: var(--iris-font-family);
  font-size: var(--iris-font-base-size);
  background-color: var(--iris-color-bg);
  color: var(--iris-color-text);
}

body {
  margin: 0;
  min-height: 100vh;
}

/* Utility classes for tokens */
@utility iris-bg { background-color: var(--iris-color-bg); }
@utility iris-bg-elevated { background-color: var(--iris-color-bg-elevated); }
@utility iris-bg-surface { background-color: var(--iris-color-bg-surface); }
@utility iris-text { color: var(--iris-color-text); }
@utility iris-text-muted { color: var(--iris-color-text-muted); }
@utility iris-text-faint { color: var(--iris-color-text-faint); }
@utility iris-text-primary { color: var(--iris-color-primary); }
@utility iris-border { border-color: var(--iris-color-border); }
@utility iris-border-subtle { border-color: var(--iris-color-border-subtle); }
```

**Step 3: Verify the build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds, no errors

**Step 4: Commit**

```bash
git add web/src/tokens.css web/src/app.css
git commit -m "feat(ui): add CSS custom property token system for skinnable theming"
```

---

## Task 2: TopNav Component

**Files:**
- Create: `web/src/components/TopNav.svelte`

**Step 1: Create TopNav component**

This replaces both Sidebar and Header. It's a horizontal nav bar with: logo, account switcher, nav items, spacer, status indicators, action buttons.

```svelte
<script lang="ts">
  import { push, location } from 'svelte-spa-router';
  import { api } from '../lib/api';

  interface Props {
    accounts?: any[];
    activeAccountId?: string | null;
    unreadCount?: number;
    syncStatus?: string | null;
    syncError?: boolean;
    chatOpen?: boolean;
    onchatToggle?: () => void;
    oncompose?: () => void;
    onaccountSwitch?: (accountId: string | null) => void;
  }

  let {
    accounts = [],
    activeAccountId = null,
    unreadCount = 0,
    syncStatus = null,
    syncError = false,
    chatOpen = false,
    onchatToggle,
    oncompose,
    onaccountSwitch,
  }: Props = $props();

  let accountDropdownOpen = $state(false);
  let overflowOpen = $state(false);

  const navItems = [
    { path: '/', label: 'Inbox', icon: 'inbox' },
    { path: '/search', label: 'Search', icon: 'search' },
    { path: '/drafts', label: 'Drafts', icon: 'file-text' },
  ];

  const overflowItems = [
    { path: '/starred', label: 'Starred', icon: 'star' },
    { path: '/archive', label: 'Archive', icon: 'archive' },
    { path: '/trash', label: 'Trash', icon: 'trash-2' },
    { divider: true },
    { path: '/settings', label: 'Settings', icon: 'settings' },
  ];

  function isActive(path: string): boolean {
    if (path === '/') return $location === '/' || $location === '';
    return $location.startsWith(path);
  }

  function navigate(path: string) {
    push(path);
    overflowOpen = false;
  }

  function selectAccount(id: string | null) {
    onaccountSwitch?.(id);
    accountDropdownOpen = false;
  }

  const activeAccount = $derived(
    accounts.find(a => a.id === activeAccountId) || accounts[0]
  );

  function handleClickOutside(e: MouseEvent) {
    accountDropdownOpen = false;
    overflowOpen = false;
  }
</script>

<svelte:window onclick={handleClickOutside} />

<nav
  class="flex items-center h-14 px-6 gap-4"
  style="background-color: var(--iris-color-bg-elevated); border-bottom: 1px solid var(--iris-color-border-subtle);"
>
  <!-- Logo -->
  <span
    class="text-xl font-bold cursor-pointer select-none"
    style="color: var(--iris-color-primary);"
    onclick={() => push('/')}
  >Iris</span>

  <!-- Account Switcher -->
  {#if accounts.length > 0}
    <div class="relative">
      <button
        class="flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-md"
        style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
               border: 1px solid var(--iris-color-border);"
        onclick={(e) => { e.stopPropagation(); accountDropdownOpen = !accountDropdownOpen; }}
      >
        <span
          class="w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-semibold"
          style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent);
                 color: var(--iris-color-primary);"
        >{activeAccount?.email?.[0]?.toUpperCase() || '?'}</span>
        <span style="color: var(--iris-color-text);">{activeAccount?.email || 'No account'}</span>
        <svg class="w-3.5 h-3.5" style="color: var(--iris-color-text-faint);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/></svg>
      </button>

      {#if accountDropdownOpen}
        <div
          class="absolute top-full left-0 mt-1 w-72 rounded-xl p-2 z-50"
          style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
          onclick={(e) => e.stopPropagation()}
        >
          <p class="text-[11px] font-medium px-2 py-1" style="color: var(--iris-color-text-faint);">Switch account</p>
          {#each accounts as account}
            <button
              class="flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-left"
              style={account.id === activeAccountId
                ? 'background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);'
                : ''}
              onclick={() => selectAccount(account.id)}
            >
              <span
                class="w-7 h-7 rounded-full flex items-center justify-center text-xs font-semibold shrink-0"
                style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent);
                       color: var(--iris-color-primary);"
              >{account.email?.[0]?.toUpperCase()}</span>
              <div class="flex-1 min-w-0">
                <p class="text-[13px] truncate" style="color: var(--iris-color-text); font-weight: {account.id === activeAccountId ? '500' : '400'};">{account.email}</p>
                <p class="text-[11px]" style="color: var(--iris-color-text-faint);">{account.provider || 'IMAP'}</p>
              </div>
              {#if account.id === activeAccountId}
                <svg class="w-3.5 h-3.5 shrink-0" style="color: var(--iris-color-primary);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/></svg>
              {/if}
            </button>
          {/each}
          <button
            class="flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-left"
            onclick={() => selectAccount(null)}
          >
            <svg class="w-4 h-4" style="color: var(--iris-color-text-muted);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6.5v0M12 12v0M12 17.5v0M4 12h16"/></svg>
            <span class="text-[13px]" style="color: var(--iris-color-text-muted);">All accounts</span>
          </button>
          <div class="my-1" style="border-top: 1px solid var(--iris-color-border);"></div>
          <button
            class="flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-left"
            onclick={() => { accountDropdownOpen = false; push('/setup'); }}
          >
            <svg class="w-4 h-4" style="color: var(--iris-color-text-faint);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 5v14M5 12h14"/></svg>
            <span class="text-[13px]" style="color: var(--iris-color-text-faint);">Add account</span>
          </button>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Nav Items -->
  <div class="flex items-center gap-0.5">
    {#each navItems as item}
      <button
        class="flex items-center gap-1.5 px-2.5 h-8 rounded-lg text-[13px] font-medium transition-colors"
        style={isActive(item.path)
          ? 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary);'
          : 'color: var(--iris-color-text-muted);'}
        style:transition="var(--iris-transition-fast)"
        onclick={() => navigate(item.path)}
      >{item.label}</button>
    {/each}

    <!-- Overflow Menu -->
    <div class="relative">
      <button
        class="flex items-center gap-1.5 px-2.5 h-8 rounded-lg text-[13px] font-medium"
        style="color: var(--iris-color-text-muted);"
        onclick={(e) => { e.stopPropagation(); overflowOpen = !overflowOpen; }}
      >More</button>

      {#if overflowOpen}
        <div
          class="absolute top-full left-0 mt-1 w-48 rounded-xl p-1.5 z-50"
          style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
          onclick={(e) => e.stopPropagation()}
        >
          {#each overflowItems as item}
            {#if item.divider}
              <div class="my-1" style="border-top: 1px solid var(--iris-color-border);"></div>
            {:else}
              <button
                class="flex items-center gap-2 w-full px-2.5 h-9 rounded-lg text-[13px] text-left"
                style="color: var(--iris-color-text-muted);"
                onclick={() => navigate(item.path!)}
              >{item.label}</button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  </div>

  <!-- Spacer -->
  <div class="flex-1"></div>

  <!-- Sync Status -->
  {#if syncStatus}
    <span
      class="flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-medium"
      style={syncError
        ? 'background: color-mix(in srgb, var(--iris-color-error) 10%, transparent); color: var(--iris-color-error);'
        : 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary);'}
    >{syncStatus}</span>
  {/if}

  <!-- AI Chat Toggle -->
  <button
    class="flex items-center gap-1.5 px-3 h-8 rounded-lg text-[13px] font-medium"
    style={chatOpen
      ? 'background: var(--iris-color-primary); color: var(--iris-color-bg);'
      : 'background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);'}
    onclick={onchatToggle}
  >AI Chat</button>

  <!-- Compose Button -->
  <button
    class="flex items-center gap-1.5 px-3.5 h-8 rounded-lg text-[13px] font-medium"
    style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);"
    onclick={oncompose}
  >Compose</button>
</nav>
```

**Step 2: Verify the build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add web/src/components/TopNav.svelte
git commit -m "feat(ui): add TopNav component replacing sidebar + header"
```

---

## Task 3: AppShell Refactor — New Layout

**Files:**
- Modify: `web/src/components/AppShell.svelte`
- Modify: `web/src/App.svelte`

**Step 1: Rewrite AppShell to use TopNav**

Replace the existing AppShell content. The new layout is: TopNav (full width) → content area (with optional ChatPanel).

```svelte
<script lang="ts">
  import type { Snippet } from 'svelte';
  import TopNav from './TopNav.svelte';
  import ChatPanel from './ChatPanel.svelte';
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  let accounts: any[] = $state([]);
  let activeAccountId: string | null = $state(null);
  let chatOpen = $state(false);
  let showCompose = $state(false);
  let syncStatus: string | null = $state(null);
  let syncError = $state(false);
  let unreadCount = $state(0);

  $effect(() => {
    loadAccounts();
    const unsub1 = wsClient.on('SyncStatus', (data: any) => {
      syncStatus = data.message || 'Syncing...';
      syncError = false;
    });
    const unsub2 = wsClient.on('SyncComplete', () => {
      syncStatus = null;
      syncError = false;
      loadMessages();
    });
    return () => { unsub1(); unsub2(); };
  });

  async function loadAccounts() {
    try {
      const res = await api.accounts.list();
      accounts = res;
      if (!activeAccountId && accounts.length > 0) {
        activeAccountId = accounts[0].id;
      }
    } catch {}
  }

  async function loadMessages() {
    try {
      const res = await api.messages.list({ account_id: activeAccountId, limit: 1 });
      unreadCount = res.unread_count || 0;
    } catch {}
  }

  function handleAccountSwitch(id: string | null) {
    activeAccountId = id;
    window.dispatchEvent(new CustomEvent('account-switch', { detail: { accountId: id } }));
  }

  function handleCompose() {
    window.dispatchEvent(new CustomEvent('open-compose', { detail: { mode: 'new', accountId: activeAccountId } }));
  }
</script>

<div class="flex flex-col h-screen" style="background: var(--iris-color-bg);">
  <TopNav
    {accounts}
    {activeAccountId}
    {unreadCount}
    {syncStatus}
    {syncError}
    chatOpen={chatOpen}
    onchatToggle={() => chatOpen = !chatOpen}
    oncompose={handleCompose}
    onaccountSwitch={handleAccountSwitch}
  />

  <div class="flex flex-1 overflow-hidden">
    <main class="flex-1 overflow-y-auto">
      {@render children()}
    </main>

    {#if chatOpen}
      <ChatPanel open={chatOpen} onclose={() => chatOpen = false} />
    {/if}
  </div>
</div>
```

**Step 2: Update App.svelte**

The App.svelte should continue wrapping routes in AppShell. Verify the router still works:

Read `web/src/App.svelte` — it should already wrap `<Router>` inside `<AppShell>`. No changes needed unless the slot pattern changed (it uses `{#snippet children()}` in Svelte 5).

**Step 3: Verify the build and test in browser**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds. The app now shows TopNav instead of sidebar.

**Step 4: Commit**

```bash
git add web/src/components/AppShell.svelte
git commit -m "feat(ui): replace sidebar layout with top nav in AppShell"
```

---

## Task 4: Tokenize Inbox Page

**Files:**
- Modify: `web/src/pages/Inbox.svelte`
- Modify: `web/src/components/inbox/MessageRow.svelte`
- Modify: `web/src/components/inbox/MessageList.svelte`

**Step 1: Refactor Inbox.svelte colors**

Replace all hardcoded Tailwind color classes with token-based inline styles. Key replacements:
- `bg-gray-900` → `style="background: var(--iris-color-bg);"`
- `text-white` → `style="color: var(--iris-color-text);"`
- `text-gray-400` → `style="color: var(--iris-color-text-muted);"`
- `text-gray-500` → `style="color: var(--iris-color-text-faint);"`
- `bg-blue-500` → `style="background: var(--iris-color-primary);"`
- `border-gray-700` → `style="border-color: var(--iris-color-border);"`

Also update:
- Category tabs to use gold tint for active (`color-mix(in srgb, var(--iris-color-primary) 10%, transparent)`)
- Unread badge to use `--iris-color-primary`
- Bulk action bar to use gold tint background with primary border

**Step 2: Refactor MessageRow.svelte colors**

Replace:
- Unread dot: `bg-blue-500` → `background: var(--iris-color-unread)`
- Priority dots: use `--iris-color-error` (urgent), `--iris-color-warning` (high), `--iris-color-success` (normal), `--iris-color-text-faint` (low)
- Sender text: `text-white` / `text-gray-400` → `var(--iris-color-text)` / `var(--iris-color-text-muted)`
- Selection highlight: gold tint background
- Category pills: `--iris-color-primary` tint

**Step 3: Refactor MessageList.svelte**

Replace background colors and border colors with tokens.

**Step 4: Verify the build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`

**Step 5: Commit**

```bash
git add web/src/pages/Inbox.svelte web/src/components/inbox/MessageRow.svelte web/src/components/inbox/MessageList.svelte
git commit -m "feat(ui): tokenize inbox page and message components"
```

---

## Task 5: Tokenize ThreadView + Message Components

**Files:**
- Modify: `web/src/pages/ThreadView.svelte`
- Modify: `web/src/components/thread/MessageCard.svelte`
- Modify: `web/src/components/thread/EmailBody.svelte`
- Modify: `web/src/components/TrustBadge.svelte`

**Step 1: Refactor ThreadView.svelte**

Replace all hardcoded colors:
- Back button, subject heading, participant text
- AI summary panel: use `var(--iris-color-bg-surface)` background, `var(--iris-color-primary)` for sparkles icon
- Star/archive/delete action buttons: use token colors
- Reply/Reply All/Forward bar: use token borders and text

**Step 2: Refactor MessageCard.svelte**

- Card background: `var(--iris-color-bg-elevated)`
- Sender name: `var(--iris-color-text)`
- Date, recipients: `var(--iris-color-text-faint)`
- Expand/collapse arrow: `var(--iris-color-text-muted)`

**Step 3: Refactor TrustBadge.svelte**

- Verified: `var(--iris-color-success)` tint
- Unverified: `var(--iris-color-error)` tint
- Tracking pixel alert: `var(--iris-color-warning)` tint

**Step 4: Update EmailBody iframe styles**

In the sandboxed iframe CSS, add token-aware styles for dark/light. The iframe doesn't inherit CSS vars, so detect the brand attribute and inject appropriate colors.

**Step 5: Verify and commit**

```bash
git add web/src/pages/ThreadView.svelte web/src/components/thread/MessageCard.svelte web/src/components/thread/EmailBody.svelte web/src/components/TrustBadge.svelte
git commit -m "feat(ui): tokenize thread view and message components"
```

---

## Task 6: Tokenize Compose Modal

**Files:**
- Modify: `web/src/components/compose/ComposeModal.svelte`

**Step 1: Refactor ComposeModal colors**

- Overlay: `var(--iris-color-overlay)`
- Modal background: `var(--iris-color-bg-elevated)`
- Modal border: `var(--iris-color-border)`
- Field labels (To, Cc, Subject): `var(--iris-color-text-faint)`
- Field values: `var(--iris-color-text)`
- Dividers: `var(--iris-color-border)`
- Body textarea: `var(--iris-color-bg-elevated)` bg, `var(--iris-color-text)` text
- Send button: `var(--iris-color-primary)` bg, `var(--iris-color-bg)` text
- AI Assist button: bordered with `var(--iris-color-primary)` sparkles icon
- Attach button: bordered with `var(--iris-color-text-muted)` icon
- Shortcut hint: `var(--iris-color-text-faint)`

**Step 2: Verify and commit**

```bash
git add web/src/components/compose/ComposeModal.svelte
git commit -m "feat(ui): tokenize compose modal"
```

---

## Task 7: Tokenize Search Page

**Files:**
- Modify: `web/src/pages/Search.svelte`

**Step 1: Refactor Search.svelte**

- Search bar: `var(--iris-color-bg-elevated)` bg, `var(--iris-color-border)` stroke, `var(--iris-color-text)` input
- Filter chips: active = gold tint (`color-mix`), inactive = bordered
- Result cards: `var(--iris-color-bg-elevated)` bg
- Result sender, subject, snippet: use text token hierarchy
- "No results" empty state: `var(--iris-color-text-faint)` icon, `var(--iris-color-text)` title

**Step 2: Verify and commit**

```bash
git add web/src/pages/Search.svelte
git commit -m "feat(ui): tokenize search page"
```

---

## Task 8: Tokenize Settings Page

**Files:**
- Modify: `web/src/pages/Settings.svelte`

**Step 1: Refactor Settings.svelte**

This is the largest page. Replace all hardcoded colors in each section:

- **Settings nav (convert to sidebar within content area):** Active item = gold tint, inactive = muted text
- **Account cards:** `var(--iris-color-bg-elevated)` bg, status badges use semantic colors
- **Theme toggle:** Dark/Light/System cards with preview rectangles, active = gold border
- **AI section:** Toggle, URL input, model picker, connection indicator
- **API Keys section:** Table rows, permission badges, create/revoke buttons
- **Audit Log section:** Table with status indicators

**Step 2: Update theme switching logic**

Replace the current `.dark` class toggle with `data-brand` attribute:

```typescript
function setTheme(theme: string) {
  if (theme === 'system') {
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    document.documentElement.setAttribute('data-brand', prefersDark ? 'dark' : 'light');
  } else {
    document.documentElement.setAttribute('data-brand', theme === 'dark' ? 'dark' : 'light');
  }
  // Remove data-brand for dark (it's the :root default)
  if (document.documentElement.getAttribute('data-brand') === 'dark') {
    document.documentElement.removeAttribute('data-brand');
  }
  api.config.setTheme(theme);
}
```

**Step 3: Verify and commit**

```bash
git add web/src/pages/Settings.svelte
git commit -m "feat(ui): tokenize settings page and update theme switching to data-brand"
```

---

## Task 9: Tokenize ChatPanel

**Files:**
- Modify: `web/src/components/ChatPanel.svelte`

**Step 1: Refactor ChatPanel colors**

- Panel background: `var(--iris-color-bg-elevated)`
- Left border: `var(--iris-color-border)`
- Header: sparkles icon `var(--iris-color-primary)`, title `var(--iris-color-text)`, close `var(--iris-color-text-faint)`
- AI messages: `var(--iris-color-bg-surface)` bg
- User messages: `color-mix(in srgb, var(--iris-color-primary) 10%, transparent)` bg, right-aligned
- Citations: faint bg with mail icon
- Suggestion chips: bordered pills, `var(--iris-color-text-muted)`
- Input field: `var(--iris-color-bg-surface)` bg, `var(--iris-color-border)` stroke
- Send button: `var(--iris-color-primary)` bg

**Step 2: Verify and commit**

```bash
git add web/src/components/ChatPanel.svelte
git commit -m "feat(ui): tokenize chat panel"
```

---

## Task 10: Empty States & Loading Components

**Files:**
- Create: `web/src/components/EmptyState.svelte`
- Create: `web/src/components/SkeletonRow.svelte`
- Create: `web/src/components/ErrorBanner.svelte`

**Step 1: Create EmptyState component**

A reusable centered empty state with icon, title, and subtitle.

```svelte
<script lang="ts">
  interface Props {
    icon?: string;
    title: string;
    subtitle?: string;
  }
  let { icon = 'inbox', title, subtitle }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center flex-1 gap-4 py-24">
  <div
    class="w-16 h-16 rounded-full flex items-center justify-center"
    style="background: var(--iris-color-bg-surface);"
  >
    <span class="text-2xl" style="color: var(--iris-color-text-faint);">
      {#if icon === 'inbox'}📥{:else if icon === 'search'}🔍{:else}📄{/if}
    </span>
  </div>
  <h2 class="text-xl font-semibold" style="color: var(--iris-color-text);">{title}</h2>
  {#if subtitle}
    <p class="text-sm text-center max-w-xs" style="color: var(--iris-color-text-muted);">{subtitle}</p>
  {/if}
</div>
```

**Step 2: Create SkeletonRow component**

```svelte
<script lang="ts">
  interface Props { widths?: number[]; }
  let { widths = [140, 320, 240] }: Props = $props();
</script>

<div class="flex items-center gap-3 px-6 h-[72px]">
  <div class="w-2 h-2 rounded-full" style="background: var(--iris-color-bg-surface);"></div>
  <div class="flex flex-col gap-2 flex-1">
    {#each widths as w, i}
      <div
        class="rounded animate-pulse"
        style="background: var(--iris-color-bg-surface); width: {w}px; height: {i < 2 ? 12 : 10}px;"
      ></div>
    {/each}
  </div>
</div>
```

**Step 3: Create ErrorBanner component**

```svelte
<script lang="ts">
  interface Props {
    message: string;
    actionLabel?: string;
    onaction?: () => void;
    ondismiss?: () => void;
  }
  let { message, actionLabel = 'Reconnect', onaction, ondismiss }: Props = $props();
</script>

<div
  class="flex items-center gap-3 px-6 h-12"
  style="background: color-mix(in srgb, var(--iris-color-error) 8%, transparent);"
>
  <span style="color: var(--iris-color-error);">⚠</span>
  <p class="text-[13px] flex-1" style="color: var(--iris-color-text);">{message}</p>
  {#if onaction}
    <button
      class="px-3.5 py-1.5 rounded-lg text-xs font-semibold text-white"
      style="background: var(--iris-color-error);"
      onclick={onaction}
    >{actionLabel}</button>
  {/if}
  {#if ondismiss}
    <button
      class="text-sm"
      style="color: var(--iris-color-text-faint);"
      onclick={ondismiss}
    >✕</button>
  {/if}
</div>
```

**Step 4: Wire empty states into Inbox and Search**

In `Inbox.svelte`, when messages array is empty and not loading, render `<EmptyState title="Inbox zero" subtitle="You've read everything. Time to go outside." />`.

In `Search.svelte`, when results are empty after a search, render `<EmptyState icon="search" title="No results found" subtitle="Try different keywords or remove some filters." />`.

**Step 5: Wire loading skeleton into Inbox**

In `Inbox.svelte`, when loading, render 4 `<SkeletonRow>` components with varying widths.

**Step 6: Verify and commit**

```bash
git add web/src/components/EmptyState.svelte web/src/components/SkeletonRow.svelte web/src/components/ErrorBanner.svelte web/src/pages/Inbox.svelte web/src/pages/Search.svelte
git commit -m "feat(ui): add empty states, skeleton loading, and error banner components"
```

---

## Task 11: Theme Initialization & Persistence

**Files:**
- Modify: `web/src/main.ts`
- Modify: `web/src/App.svelte`

**Step 1: Update main.ts theme initialization**

Replace the current dark mode class logic with data-brand attribute:

```typescript
// Apply saved theme before first render to prevent flash
const savedTheme = localStorage.getItem('iris-theme') || 'dark';
if (savedTheme === 'system') {
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (!prefersDark) document.documentElement.setAttribute('data-brand', 'light');
} else if (savedTheme === 'light') {
  document.documentElement.setAttribute('data-brand', 'light');
}
// Dark is the default (:root), so no attribute needed
```

**Step 2: Fetch theme from API on mount in App.svelte**

In the existing `$effect` that runs on mount, after `initSession()`, fetch the saved theme config and apply it:

```typescript
const config = await api.config.get();
if (config.theme) {
  localStorage.setItem('iris-theme', config.theme);
  applyTheme(config.theme);
}
```

**Step 3: Verify and commit**

```bash
git add web/src/main.ts web/src/App.svelte
git commit -m "feat(ui): update theme initialization to use data-brand attribute"
```

---

## Task 12: Remove Old Sidebar & Header

**Files:**
- Delete: `web/src/components/Sidebar.svelte`
- Delete: `web/src/components/Header.svelte`
- Modify: `web/src/components/AppShell.svelte` (remove imports if still present)

**Step 1: Remove Sidebar.svelte and Header.svelte**

These are fully replaced by TopNav. Delete both files.

**Step 2: Verify no remaining imports**

Search for any imports of Sidebar or Header across the codebase and remove them.

Run: `grep -r "Sidebar\|Header" web/src/ --include="*.svelte" --include="*.ts"`

Remove any found references.

**Step 3: Verify the build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds with no errors

**Step 4: Commit**

```bash
git rm web/src/components/Sidebar.svelte web/src/components/Header.svelte
git add web/src/components/AppShell.svelte
git commit -m "refactor(ui): remove old Sidebar and Header components"
```

---

## Task 13: Tokenize AccountSetup Page

**Files:**
- Modify: `web/src/pages/AccountSetup.svelte`

**Step 1: Replace hardcoded colors**

- Step indicators, provider cards, OAuth buttons: use token colors
- Form inputs: `var(--iris-color-bg-surface)` bg, `var(--iris-color-border)` stroke
- Success/error states: use semantic token colors
- Progress indicator: `var(--iris-color-primary)`

**Step 2: Verify and commit**

```bash
git add web/src/pages/AccountSetup.svelte
git commit -m "feat(ui): tokenize account setup page"
```

---

## Task 14: Tokenize SyncStatus Component

**Files:**
- Modify: `web/src/components/inbox/SyncStatus.svelte`

**Step 1: Replace hardcoded colors with tokens**

The SyncStatus indicator should use:
- Syncing state: `var(--iris-color-primary)` tint
- Error state: `var(--iris-color-error)` tint
- Text: appropriate token text colors

**Step 2: Verify and commit**

```bash
git add web/src/components/inbox/SyncStatus.svelte
git commit -m "feat(ui): tokenize sync status component"
```

---

## Task 15: Final Verification & Cleanup

**Files:**
- All modified files

**Step 1: Full grep for hardcoded colors**

Search for any remaining hardcoded color values:

Run: `grep -rn "bg-gray\|bg-blue\|bg-red\|text-gray\|text-white\|text-blue\|border-gray\|#[0-9a-fA-F]\{6\}" web/src/ --include="*.svelte" | grep -v tokens.css | grep -v node_modules`

Fix any remaining hardcoded values found.

**Step 2: Full build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Clean build, no warnings

**Step 3: Run existing tests**

Run: `cd /Users/divyekant/Projects/iris && cargo test`
Expected: All 102 tests pass (backend unchanged)

**Step 4: Final commit**

```bash
git add -A
git commit -m "refactor(ui): complete token migration — remove all hardcoded colors"
```

---

## Summary

| Task | What | Commits |
|------|------|---------|
| 1 | Token CSS foundation | 1 |
| 2 | TopNav component | 1 |
| 3 | AppShell refactor | 1 |
| 4 | Tokenize Inbox | 1 |
| 5 | Tokenize ThreadView | 1 |
| 6 | Tokenize Compose | 1 |
| 7 | Tokenize Search | 1 |
| 8 | Tokenize Settings + theme switch | 1 |
| 9 | Tokenize ChatPanel | 1 |
| 10 | Empty/Loading/Error components | 1 |
| 11 | Theme init & persistence | 1 |
| 12 | Remove old Sidebar/Header | 1 |
| 13 | Tokenize AccountSetup | 1 |
| 14 | Tokenize SyncStatus | 1 |
| 15 | Final verification & cleanup | 1 |

**Total: 15 tasks, ~15 commits**

**Execution order matters:** Tasks 1-3 must be sequential (foundation → component → layout). Tasks 4-14 can be done in any order after Task 3. Task 15 is always last.
