# Wave 3 Layer 1: UX Foundation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform Iris from a functional-but-mechanical email client into a polished, responsive, keyboard-first experience with woven AI integration.

**Architecture:** Pure frontend work across 62 Svelte components. New utilities (`transitions.ts`, `feedback.ts`, `keyboard.ts`, `commands.ts`) provide shared behavior. Existing shared components (Modal, Form*, Badge) get wired into new systems. Three new components (DropdownMenu, CommandPalette, Tooltip) fill gaps. All changes use existing CSS custom properties from `tokens.css`.

**Tech Stack:** Svelte 5, TypeScript, Tailwind CSS 4, CSS custom properties

**Spec:** `docs/superpowers/specs/2026-03-15-wave3-design.md` (Layer 1)

---

## Chunk 1: Foundation Utilities & Shared Components

### Task 0: Setup Path Aliases

**Files:**
- Modify: `web/vite.config.ts`
- Modify: `web/tsconfig.app.json`

The codebase uses relative paths for all imports. Adding `$lib` and `$components` aliases simplifies the new utility imports and follows SvelteKit conventions that developers expect.

- [ ] **Step 1: Add Vite resolve aliases**

In `web/vite.config.ts`, add:
```typescript
import path from 'path';
// Inside defineConfig:
resolve: {
  alias: {
    '$lib': path.resolve(__dirname, 'src/lib'),
    '$components': path.resolve(__dirname, 'src/components'),
  }
}
```

- [ ] **Step 2: Add TypeScript path mappings**

In `web/tsconfig.app.json`, add under `compilerOptions`:
```json
"paths": {
  "$lib/*": ["./src/lib/*"],
  "$components/*": ["./src/components/*"]
}
```

- [ ] **Step 3: Verify existing build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds — aliases are additive, existing relative imports still work.

- [ ] **Step 4: Commit**

```bash
git add web/vite.config.ts web/tsconfig.app.json
git commit -m "chore: add $lib and $components path aliases for cleaner imports"
```

---

### Task 1: Transition System

**Files:**
- Create: `web/src/lib/transitions.ts`
- Modify: `web/src/tokens.css` (add transition duration/easing split tokens if needed)

- [ ] **Step 1: Create transitions.ts with core primitives**

```typescript
// web/src/lib/transitions.ts
import { fade as svelteFade, slide as svelteSlide, scale as svelteScale } from 'svelte/transition';

// Duration constants (match tokens.css values)
const FAST = 120;
const NORMAL = 200;

// Named transitions for consistent use across all components
export function irisFade(node: HTMLElement, params: { duration?: number; delay?: number } = {}) {
  return svelteFade(node, { duration: params.duration ?? FAST, delay: params.delay ?? 0 });
}

export function irisSlide(node: HTMLElement, params: { duration?: number; delay?: number; axis?: 'x' | 'y' } = {}) {
  return svelteSlide(node, { duration: params.duration ?? NORMAL, delay: params.delay ?? 0, axis: params.axis ?? 'y' });
}

export function irisScale(node: HTMLElement, params: { duration?: number; delay?: number; start?: number } = {}) {
  return svelteScale(node, { duration: params.duration ?? 150, delay: params.delay ?? 0, start: params.start ?? 0.95 });
}

// Collapse: height auto-animate for list item removal
export function irisCollapse(node: HTMLElement, params: { duration?: number; delay?: number } = {}) {
  const height = node.offsetHeight;
  const paddingTop = parseFloat(getComputedStyle(node).paddingTop);
  const paddingBottom = parseFloat(getComputedStyle(node).paddingBottom);
  const marginTop = parseFloat(getComputedStyle(node).marginTop);
  const marginBottom = parseFloat(getComputedStyle(node).marginBottom);

  return {
    duration: params.duration ?? NORMAL,
    delay: params.delay ?? 0,
    css: (t: number) => `
      height: ${t * height}px;
      padding-top: ${t * paddingTop}px;
      padding-bottom: ${t * paddingBottom}px;
      margin-top: ${t * marginTop}px;
      margin-bottom: ${t * marginBottom}px;
      opacity: ${t};
      overflow: hidden;
    `
  };
}

// Staggered fade for lists of action buttons
export function staggeredFade(node: HTMLElement, params: { index?: number; stagger?: number } = {}) {
  const index = params.index ?? 0;
  const stagger = params.stagger ?? 30;
  return svelteFade(node, { duration: FAST, delay: index * stagger });
}
```

- [ ] **Step 2: Verify build compiles**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds with no errors

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/transitions.ts
git commit -m "feat: add shared transition system (fade, slide, scale, collapse)"
```

---

### Task 2: Feedback System

**Files:**
- Create: `web/src/lib/feedback.ts`
- Modify: `web/src/components/Toast.svelte` (wire to feedback store, add undo support, add animations)

- [ ] **Step 1: Create feedback.ts store**

```typescript
// web/src/lib/feedback.ts
import { writable, get } from 'svelte/store';

export interface FeedbackItem {
  id: string;
  message: string;
  type: 'success' | 'error' | 'info';
  undoFn?: () => void;
  autoDismissMs: number;
  createdAt: number;
}

function createFeedbackStore() {
  const { subscribe, update } = writable<FeedbackItem[]>([]);

  function add(item: Omit<FeedbackItem, 'id' | 'createdAt'>) {
    const id = `fb-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    const entry: FeedbackItem = { ...item, id, createdAt: Date.now() };
    update(items => [...items, entry]);

    if (item.autoDismissMs > 0) {
      setTimeout(() => dismiss(id), item.autoDismissMs);
    }
    return id;
  }

  function dismiss(id: string) {
    update(items => items.filter(i => i.id !== id));
  }

  function undo(id: string) {
    let fn: (() => void) | undefined;
    update(items => {
      const item = items.find(i => i.id === id);
      fn = item?.undoFn;
      return items.filter(i => i.id !== id);
    });
    fn?.(); // Call after store update to avoid mutation during iteration
  }

  return {
    subscribe,
    success: (message: string, opts?: { undo?: () => void }) =>
      add({ message, type: 'success', undoFn: opts?.undo, autoDismissMs: opts?.undo ? 6000 : 4000 }),
    error: (message: string) =>
      add({ message, type: 'error', autoDismissMs: 6000 }),
    info: (message: string) =>
      add({ message, type: 'info', autoDismissMs: 4000 }),
    dismiss,
    undo,
  };
}

export const feedback = createFeedbackStore();
```

- [ ] **Step 2: Rewrite Toast.svelte to use feedback store**

Read current `web/src/components/Toast.svelte`, then rewrite to subscribe to `feedback` store, render multiple toasts, support undo button, use `irisSlide` transition for entry/exit.

Key changes:
- Remove props-based API (`message`, `visible`, `ondismiss`)
- Subscribe to `feedback` store directly
- Render each `FeedbackItem` as a toast card
- Add "Undo" button when `undoFn` exists
- Use `irisSlide` from `transitions.ts` for enter/exit
- Stack toasts vertically (bottom-right, gap-2)

- [ ] **Step 3: Verify build compiles**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Build succeeds

- [ ] **Step 4: Find and update all existing Toast usages**

Search for Toast component imports/usages across all .svelte files. Update callers to use `feedback.success()` / `feedback.error()` instead of setting props on Toast.

Run: `grep -r "Toast" web/src/ --include="*.svelte" -l` to find all files that reference Toast.

- [ ] **Step 5: Commit**

```bash
git add web/src/lib/feedback.ts web/src/components/Toast.svelte
# Also add any modified files that were using Toast
git commit -m "feat: add feedback system with undo support and animated toasts"
```

---

### Task 3: Tooltip Component

**Files:**
- Create: `web/src/components/shared/Tooltip.svelte`

- [ ] **Step 1: Create Tooltip.svelte**

A lightweight tooltip that replaces `title` attributes. Positioned above by default, flips if near viewport edge.

```svelte
<!-- web/src/components/shared/Tooltip.svelte -->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { irisFade } from '$lib/transitions';

  let { text = '', position = 'top', children }: {
    text: string;
    position?: 'top' | 'bottom' | 'left' | 'right';
    children: Snippet;
  } = $props();

  let visible = $state(false);

  // Centering logic: horizontal for top/bottom, vertical for left/right
  const isVertical = $derived(position === 'top' || position === 'bottom');
</script>

<span
  onmouseenter={() => visible = true}
  onmouseleave={() => visible = false}
  onfocus={() => visible = true}
  onblur={() => visible = false}
  class="inline-flex relative"
  role="group"
>
  {@render children()}
  {#if visible && text}
    <span
      transition:irisFade
      role="tooltip"
      class="absolute z-50 px-2 py-1 text-xs rounded whitespace-nowrap pointer-events-none"
      class:bottom-full={position === 'top'}
      class:top-full={position === 'bottom'}
      class:right-full={position === 'left'}
      class:left-full={position === 'right'}
      class:mb-1={position === 'top'}
      class:mt-1={position === 'bottom'}
      class:mr-1={position === 'left'}
      class:ml-1={position === 'right'}
      style="
        background: var(--iris-color-bg-elevated);
        color: var(--iris-color-text);
        border: 1px solid var(--iris-color-border);
        {isVertical ? 'left: 50%; transform: translateX(-50%);' : 'top: 50%; transform: translateY(-50%);'}
      "
    >
      {text}
    </span>
  {/if}
</span>
```

- [ ] **Step 2: Verify build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`

- [ ] **Step 3: Commit**

```bash
git add web/src/components/shared/Tooltip.svelte
git commit -m "feat: add Tooltip component replacing title attributes"
```

---

### Task 4: DropdownMenu Component

**Files:**
- Create: `web/src/components/shared/DropdownMenu.svelte`

- [ ] **Step 1: Create DropdownMenu.svelte**

A reusable dropdown menu that handles trigger button, floating menu, keyboard nav, and click-outside-close.

```svelte
<!-- web/src/components/shared/DropdownMenu.svelte -->
<script lang="ts">
  import { irisScale } from '$lib/transitions';

  interface MenuItem {
    label: string;
    icon?: string;
    shortcut?: string;
    onClick: () => void;
    dividerAfter?: boolean;
    disabled?: boolean;
  }

  let { items = [], triggerLabel = '', triggerIcon = '' }: {
    items: MenuItem[];
    triggerLabel?: string;
    triggerIcon?: string;
  } = $props();

  let open = $state(false);
  let focusedIndex = $state(-1);
  let menuEl: HTMLElement;

  // Single reactive source for enabled items — used by both template and keyboard handler
  const enabledItems = $derived(items.filter(i => !i.disabled));

  function toggle() { open = !open; focusedIndex = -1; }
  function close() { open = false; focusedIndex = -1; }

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') { close(); e.preventDefault(); }
    else if (e.key === 'ArrowDown') { focusedIndex = Math.min(focusedIndex + 1, enabledItems.length - 1); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { focusedIndex = Math.max(focusedIndex - 1, 0); e.preventDefault(); }
    else if (e.key === 'Enter' && focusedIndex >= 0) { enabledItems[focusedIndex]?.onClick(); close(); e.preventDefault(); }
  }
</script>

<svelte:window onclick={(e) => { if (open && menuEl && !menuEl.contains(e.target as Node)) close(); }} />

<div class="relative inline-block" bind:this={menuEl} onkeydown={handleKeydown}>
  <button
    onclick={toggle}
    class="flex items-center gap-1 px-2 py-1 rounded text-sm transition-colors"
    style="color: var(--iris-color-text-muted); background: transparent;"
    onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--iris-color-bg-hover)'}
    onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
  >
    {#if triggerIcon}<span>{triggerIcon}</span>{/if}
    {#if triggerLabel}<span>{triggerLabel}</span>{/if}
    <span class="text-xs">&#x25BE;</span>
  </button>

  {#if open}
    <div
      transition:irisScale
      class="absolute right-0 mt-1 min-w-[160px] rounded-lg shadow-lg z-50 py-1"
      style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
    >
      {#each enabledItems as item, i}
        <button
          onclick={() => { item.onClick(); close(); }}
          class="w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-4 transition-colors"
          style="color: {focusedIndex === i ? 'var(--iris-color-text)' : 'var(--iris-color-text-muted)'}; background: {focusedIndex === i ? 'var(--iris-color-bg-hover)' : 'transparent'};"
          onmouseenter={() => focusedIndex = i}
        >
          <span class="flex items-center gap-2">
            {#if item.icon}<span>{item.icon}</span>{/if}
            {item.label}
          </span>
          {#if item.shortcut}
            <span class="text-xs" style="color: var(--iris-color-text-faint);">{item.shortcut}</span>
          {/if}
        </button>
        {#if item.dividerAfter}
          <hr class="my-1" style="border-color: var(--iris-color-border);" />
        {/if}
      {/each}
    </div>
  {/if}
</div>
```

- [ ] **Step 2: Verify build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`

- [ ] **Step 3: Commit**

```bash
git add web/src/components/shared/DropdownMenu.svelte
git commit -m "feat: add DropdownMenu component with keyboard navigation"
```

---

### Task 5: Wire Transitions into Modal

**Files:**
- Modify: `web/src/components/shared/Modal.svelte`

- [ ] **Step 1: Read current Modal.svelte**

Read `web/src/components/shared/Modal.svelte` to understand current implementation.

- [ ] **Step 2: Add scale transition to modal and fade to backdrop**

Import `irisScale` and `irisFade` from `$lib/transitions`. Apply `irisFade` to the backdrop overlay and `irisScale` to the modal card.

- [ ] **Step 3: Verify dialogs still work**

Manually test: open SpamDialog, RedirectDialog, SnoozePicker (if they use Modal). Check that enter/exit animations are smooth.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/shared/Modal.svelte
git commit -m "feat: add scale/fade transitions to Modal component"
```

---

## Chunk 2: Visual Hierarchy & Message Row Redesign

### Task 6: Message Row Badge Priority System

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte`
- Create: `web/src/lib/badge-priority.ts`

- [ ] **Step 1: Create badge priority logic**

```typescript
// web/src/lib/badge-priority.ts

export interface BadgeInfo {
  label: string;
  type: 'warning' | 'error' | 'info' | 'success' | 'neutral';
}

// Priority order: needs_reply > deadline > intent > sentiment > category
export function getPrimaryBadges(message: {
  needs_reply?: boolean;
  deadline?: string;
  intent?: string;
  sentiment?: string;
  category?: string;
  labels?: string;
}): { primary: BadgeInfo | null; overflow: number } {
  const badges: BadgeInfo[] = [];

  if (message.needs_reply) {
    badges.push({ label: 'Needs Reply', type: 'warning' });
  }
  if (message.deadline) {
    badges.push({ label: `Due ${message.deadline}`, type: 'error' });
  }
  if (message.intent && message.intent !== 'informational') {
    badges.push({ label: message.intent, type: 'info' });
  }
  if (message.sentiment && message.sentiment !== 'neutral') {
    badges.push({ label: message.sentiment, type: message.sentiment === 'negative' ? 'error' : 'success' });
  }
  if (message.category && message.category !== 'primary') {
    badges.push({ label: message.category, type: 'neutral' });
  }

  return {
    primary: badges[0] ?? null,
    overflow: Math.max(0, badges.length - 1),
  };
}
```

- [ ] **Step 2: Read current MessageRow.svelte**

Read `web/src/components/inbox/MessageRow.svelte` to understand current badge rendering.

- [ ] **Step 3: Refactor MessageRow to use badge priority**

Replace inline badge rendering with `getPrimaryBadges()`. Show only the primary badge. Add "+N" overflow chip when more badges exist. Move secondary badges to hover popover.

Key changes:
- Import `getPrimaryBadges` from `$lib/badge-priority`
- Replace multi-badge inline rendering with single primary badge slot
- Add hover state that shows all badges in a mini-popover
- Use `staggeredFade` for hover action buttons

- [ ] **Step 4: Add row exit animation for archive/delete**

When a message is archived or deleted, animate it out using `irisSlide` + `irisCollapse` before removing from the list.

- [ ] **Step 5: Add keyboard focus highlight**

When a row is focused via `j/k` navigation, show a brief gold flash on the left border (200ms animation).

- [ ] **Step 6: Verify build and test visually**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Then use Pencil to prototype the new message row layout before committing.

- [ ] **Step 7: Commit**

```bash
git add web/src/lib/badge-priority.ts web/src/components/inbox/MessageRow.svelte
git commit -m "feat: smart badge priority system — show 1 primary badge, overflow count"
```

---

### Task 7: ThreadView Action Bar Grouping

**Files:**
- Modify: `web/src/pages/ThreadView.svelte`

- [ ] **Step 1: Read current ThreadView.svelte action bar section**

Read `web/src/pages/ThreadView.svelte` to find the action buttons section.

- [ ] **Step 2: Refactor action bar into groups using DropdownMenu**

Replace the flat row of 10+ icon buttons with:
- Primary (always visible): Reply, Reply All, Forward buttons
- "Organize" DropdownMenu: Star, Snooze, Archive, Delete
- "AI" DropdownMenu: Summarize, Tasks, Multi-Reply
- "More" DropdownMenu: Spam, Mute, Redirect

Import `DropdownMenu` from `$components/shared/DropdownMenu.svelte`. Add visual dividers between groups.

Keyboard shortcuts continue to work directly (bypass dropdown).

- [ ] **Step 3: Verify build and test**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Prototype on Pencil to validate grouped layout.

- [ ] **Step 4: Commit**

```bash
git add web/src/pages/ThreadView.svelte
git commit -m "feat: group ThreadView actions into organized dropdowns"
```

---

### Task 8: Quick Actions Animation & Positioning

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte` (hover actions section)

Note: MessageRow already has 4 hover actions (Archive, Delete, Star, Snooze) — streamlining was done in Wave 2. This task focuses on animation and positioning polish.

- [ ] **Step 1: Verify action button positioning**

Read `web/src/components/inbox/MessageRow.svelte` to confirm action buttons overlay the right side (date column), not subject/sender. Adjust positioning if needed.

- [ ] **Step 2: Apply staggered fade animation**

Use `staggeredFade` from `$lib/transitions` on each action button with `index` parameter for 30ms stagger on hover reveal.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/inbox/MessageRow.svelte
git commit -m "feat: add staggered fade animation to message row hover actions"
```

---

## Chunk 3: Keyboard-First Navigation

### Task 9: Keyboard Manager Utility

**Files:**
- Create: `web/src/lib/keyboard.ts`

- [ ] **Step 1: Create centralized keyboard manager**

Extract keyboard handling from Inbox.svelte into a reusable utility. Support:
- Context-aware shortcuts (different per page/mode)
- Chord sequences (g+i, g+s, g+d)
- Input field detection (skip shortcuts when typing)
- Mode tracking (Inbox, Thread, Compose, Chat, Settings)

```typescript
// web/src/lib/keyboard.ts
import { writable, get } from 'svelte/store';

export type KeyboardMode = 'inbox' | 'thread' | 'compose' | 'chat' | 'settings' | 'global';

export const currentMode = writable<KeyboardMode>('inbox');

interface ShortcutDef {
  key: string;
  mode: KeyboardMode | 'global';
  shift?: boolean;
  meta?: boolean;
  ctrl?: boolean;
  action: () => void;
  description: string;
}

const shortcuts: ShortcutDef[] = [];
let pendingChord = '';
let chordTimer: ReturnType<typeof setTimeout> | null = null;

export function registerShortcut(def: ShortcutDef) {
  shortcuts.push(def);
}

export function registerShortcuts(defs: ShortcutDef[]) {
  defs.forEach(d => shortcuts.push(d));
}

export function getShortcutsForMode(mode: KeyboardMode): ShortcutDef[] {
  return shortcuts.filter(s => s.mode === mode || s.mode === 'global');
}

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || (el as HTMLElement).isContentEditable;
}

export function handleGlobalKeydown(e: KeyboardEvent) {
  // Meta/Ctrl shortcuts always fire (e.g. Cmd+K)
  if (e.metaKey || e.ctrlKey) {
    const match = shortcuts.find(s =>
      s.key === e.key && (s.meta || s.ctrl) &&
      (s.mode === get(currentMode) || s.mode === 'global')
    );
    if (match) {
      e.preventDefault();
      match.action();
      return;
    }
  }

  // Skip non-meta shortcuts when input focused
  if (isInputFocused() && !e.metaKey && !e.ctrlKey) return;

  const mode = get(currentMode);

  // Handle chord (g+key)
  if (pendingChord === 'g') {
    pendingChord = '';
    if (chordTimer) clearTimeout(chordTimer);
    const match = shortcuts.find(s => s.key === `g+${e.key}` && (s.mode === mode || s.mode === 'global'));
    if (match) { e.preventDefault(); match.action(); return; }
  }

  if (e.key === 'g' && !e.metaKey && !e.ctrlKey) {
    pendingChord = 'g';
    chordTimer = setTimeout(() => { pendingChord = ''; }, 1000);
    return;
  }

  // Single-key shortcuts
  const match = shortcuts.find(s =>
    s.key === e.key &&
    !s.meta && !s.ctrl &&
    (s.shift === undefined || s.shift === e.shiftKey) &&
    (s.mode === mode || s.mode === 'global')
  );
  if (match) {
    e.preventDefault();
    match.action();
  }
}

// All registered shortcuts for help overlay
export function getAllShortcuts(): { mode: string; key: string; description: string }[] {
  return shortcuts.map(s => ({
    mode: s.mode,
    key: s.meta ? `Cmd+${s.key}` : s.shift ? `Shift+${s.key}` : s.key,
    description: s.description,
  }));
}
```

- [ ] **Step 2: Verify build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`

- [ ] **Step 3: Commit**

```bash
git add web/src/lib/keyboard.ts
git commit -m "feat: centralized keyboard manager with mode tracking and chords"
```

---

### Task 10: Command Palette

**Files:**
- Create: `web/src/components/shared/CommandPalette.svelte`
- Create: `web/src/lib/commands.ts`

- [ ] **Step 1: Create command registry**

```typescript
// web/src/lib/commands.ts
import { writable } from 'svelte/store';

export interface Command {
  id: string;
  label: string;
  category: string;  // 'Navigation', 'Actions', 'AI', 'Settings'
  keywords: string[]; // extra search terms
  action: () => void;
  shortcut?: string;
}

const commands: Command[] = [];

export const paletteOpen = writable(false);

export function registerCommand(cmd: Command) {
  // Deduplicate by id
  const idx = commands.findIndex(c => c.id === cmd.id);
  if (idx >= 0) commands[idx] = cmd;
  else commands.push(cmd);
}

export function registerCommands(cmds: Command[]) {
  cmds.forEach(registerCommand);
}

export function searchCommands(query: string): Command[] {
  if (!query.trim()) return commands.slice(0, 10);
  const q = query.toLowerCase();
  return commands.filter(c =>
    c.label.toLowerCase().includes(q) ||
    c.category.toLowerCase().includes(q) ||
    c.keywords.some(k => k.toLowerCase().includes(q))
  ).slice(0, 10);
}

export function togglePalette() {
  paletteOpen.update(v => !v);
}
```

- [ ] **Step 2: Create CommandPalette.svelte**

A fullscreen overlay with search input, fuzzy-matched command list, keyboard navigation (arrow keys + Enter), and Escape to close. Style uses design tokens.

Key behavior:
- `Cmd+K` opens/closes (registered as global shortcut in keyboard.ts)
- Type to filter commands
- Arrow up/down to navigate, Enter to execute
- Escape to close
- Shows category labels and shortcuts
- Uses `irisScale` transition for entry

- [ ] **Step 3: Wire into AppShell.svelte**

Add `<CommandPalette />` to `AppShell.svelte`. Register core commands (navigate to inbox/sent/drafts/settings, compose, toggle theme, toggle chat).

- [ ] **Step 4: Register Cmd+K shortcut**

In `AppShell.svelte`, register the global `Cmd+K` shortcut via `keyboard.ts` that toggles the palette.

- [ ] **Step 5: Verify build and test**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`

- [ ] **Step 6: Commit**

```bash
git add web/src/lib/commands.ts web/src/components/shared/CommandPalette.svelte web/src/components/AppShell.svelte
git commit -m "feat: command palette (Cmd+K) with fuzzy search and keyboard navigation"
```

---

### Task 11: Migrate Inbox Keyboard Shortcuts to Keyboard Manager

**Files:**
- Modify: `web/src/pages/Inbox.svelte` (remove inline keyboard handler)
- Modify: `web/src/components/AppShell.svelte` (attach global keydown listener)

- [ ] **Step 1: Read Inbox.svelte keyboard handling (lines 38-196)**

Read `web/src/pages/Inbox.svelte` to understand all current shortcuts.

- [ ] **Step 2: Register Inbox shortcuts via keyboard.ts**

Move all shortcuts from the inline `handleKeydown` in Inbox.svelte to `registerShortcuts()` calls. Set `currentMode` to 'inbox' when Inbox mounts, restore on unmount.

- [ ] **Step 3: Attach global keydown listener in AppShell**

In `AppShell.svelte`, add `<svelte:window onkeydown={handleGlobalKeydown} />` using the imported handler from `keyboard.ts`.

- [ ] **Step 4: Remove old Cmd+K handler and resolve conflict**

The existing Inbox.svelte has `Cmd+K` → `focusSearch()`. Remove this since `Cmd+K` now opens the CommandPalette. Add a "Focus Search" command to the command palette so users can still reach search via `Cmd+K` → type "search" → Enter.

- [ ] **Step 5: Add new shortcuts**

Register these new shortcuts for inbox mode:
- `b` → snooze focused message
- `m` → mute focused thread

Register for settings mode:
- `h` → previous tab
- `l` → next tab

- [ ] **Step 6: Add mode indicator**

Add a subtle mode badge in bottom-left of AppShell showing current keyboard mode. Style: small pill with `--iris-color-text-faint`, `--iris-color-bg-elevated`.

- [ ] **Step 7: Verify all existing shortcuts still work**

Manually test: j/k navigation, e archive, s star, # delete, r reply, g+i go inbox, g+s go sent, g+d go drafts, / focus search, c compose, ? help overlay.

- [ ] **Step 8: Commit**

```bash
git add web/src/pages/Inbox.svelte web/src/components/AppShell.svelte
git commit -m "feat: migrate keyboard shortcuts to centralized manager, add mode indicator"
```

---

### Task 12: Keyboard Help Overlay Update

**Files:**
- Modify: `web/src/pages/Inbox.svelte` (or wherever help overlay is rendered)

- [ ] **Step 1: Update help overlay to use getAllShortcuts()**

Replace the hardcoded shortcut list in the `?` help overlay with `getAllShortcuts()` from `keyboard.ts`. Group by mode. Show keyboard mode in overlay header.

- [ ] **Step 2: Commit**

```bash
git commit -m "feat: dynamic keyboard help overlay from shortcut registry"
```

---

## Chunk 4: AI Integration (Woven) & Settings

### Task 13: Thread Intelligence Strip

**Files:**
- Modify: `web/src/pages/ThreadView.svelte`

- [ ] **Step 1: Read ThreadView.svelte to find summary panel**

Identify the current collapsible AI summary panel implementation.

- [ ] **Step 2: Replace with intelligence strip**

Replace the separate summary panel with a compact single-line strip below the thread subject:
- Format: "5 messages, 2 action items, deadline Mar 20"
- Click to expand full summary (uses `irisCollapse` transition)
- Always visible for multi-message threads
- Fetches summary lazily on mount (existing API: `api.threads.summarize`)

- [ ] **Step 3: Commit**

```bash
git add web/src/pages/ThreadView.svelte
git commit -m "feat: replace summary panel with compact thread intelligence strip"
```

---

### Task 14: Inline AI Suggestion Strip

**Files:**
- Modify: `web/src/pages/ThreadView.svelte`

- [ ] **Step 1: Add AI suggestion strip for needs-reply threads**

Below the thread header (after the intelligence strip), show an "AI suggests..." strip when the thread is flagged as `needs_reply`:
- Calls `api.ai.assist` with action 'shorter' to generate a brief reply suggestion
- Shows 1-2 sentence preview text
- "Reply with this" button opens compose pre-filled with the suggestion
- "Dismiss" button hides the strip for this session
- Uses `irisSlide` transition for appearance
- Subtle styling: `--iris-color-bg-elevated` background, `--iris-color-text-muted` text

- [ ] **Step 2: Commit**

```bash
git add web/src/pages/ThreadView.svelte
git commit -m "feat: inline AI reply suggestion strip for needs-reply threads"
```

---

### Task 15: Contextual Chat Panel

**Files:**
- Modify: `web/src/components/ChatPanel.svelte`

- [ ] **Step 1: Read ChatPanel.svelte**

Understand current implementation.

- [ ] **Step 2: Add context injection**

When ChatPanel opens while ThreadView is active, automatically inject the current thread ID as context. Modify the chat API call to include `thread_context: threadId` so the AI has the thread loaded.

Add a context indicator at the top of the chat: "Chatting about: [thread subject]" with a clear button to remove context.

- [ ] **Step 3: Add slide transition**

Apply `irisSlide` (axis 'x') to the chat panel's enter/exit.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/ChatPanel.svelte
git commit -m "feat: contextual chat panel — auto-loads current thread as context"
```

---

### Task 15b: Smart Compose Hints

**Files:**
- Modify: `web/src/components/compose/AutocompleteTextarea.svelte`
- Modify: `web/src/components/compose/ComposeModal.svelte`

- [ ] **Step 1: Read AutocompleteTextarea.svelte**

Understand the existing autocomplete dropdown behavior — trigger, API call, suggestion display.

- [ ] **Step 2: Enhance autocomplete with thread context**

Modify the autocomplete to inject the current thread context (if replying) into the API call. This gives the AI better context for suggestions.

- [ ] **Step 3: Add 500ms debounce trigger**

Ensure autocomplete triggers on a 500ms typing pause (not on every keystroke). Verify cancellation of in-flight requests when user continues typing.

- [ ] **Step 4: Verify arrow key navigation and Tab/Enter acceptance**

Test that arrow keys navigate suggestions, Tab/Enter inserts the selected suggestion, and Escape dismisses the dropdown.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/compose/AutocompleteTextarea.svelte web/src/components/compose/ComposeModal.svelte
git commit -m "feat: enhance smart compose with thread context and debounced trigger"
```

---

### Task 15c: Contextual AI Actions (Right-Click)

**Files:**
- Modify: `web/src/components/inbox/MessageRow.svelte`
- Create: `web/src/components/shared/ContextMenu.svelte`

- [ ] **Step 1: Create ContextMenu component**

A minimal right-click context menu reusing DropdownMenu's styling. Positioned at click coordinates, dismissed on click-outside or Escape.

- [ ] **Step 2: Add context menu to MessageRow**

On right-click of a message row, show context menu with:
- "Summarize" → calls `api.threads.summarize`, shows result in a toast or inline popover
- "Extract tasks" → calls `api.ai.extractTasks`, shows result inline
- "Draft reply" → calls `api.ai.draftFromIntent`, opens compose with result

Results appear inline (beneath the row or as a temporary expansion), not in the chat panel.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/shared/ContextMenu.svelte web/src/components/inbox/MessageRow.svelte
git commit -m "feat: right-click context menu with AI actions on message rows"
```

---

### Task 16: Settings Transitions & Form Verification

**Files:**
- Modify: `web/src/pages/Settings.svelte`

Note: Settings already has hash routing (`getTabFromHash`, `history.replaceState`) and `{#if}` conditional rendering (lazy loading). This task adds visual polish and verifies shared component usage.

- [ ] **Step 1: Verify existing hash routing and lazy loading work correctly**

Read `web/src/pages/Settings.svelte`. Confirm hash routing and back/forward navigation function. No changes needed if working.

- [ ] **Step 2: Add fade crossfade between tabs**

Use `irisFade` transition on tab content containers for smooth switching when changing tabs.

- [ ] **Step 3: Verify all tabs use shared Form components**

Check each settings tab (SettingsGeneral, SettingsAI, etc.) uses `FormInput`, `FormSelect`, `FormToggle` from `$components/shared/`. Flag any inline form implementations that should be migrated.

- [ ] **Step 4: Commit**

```bash
git add web/src/pages/Settings.svelte
git commit -m "feat: add fade transitions to settings tab switching"
```

---

### Task 17: Token Compliance Audit & Fix

**Files:**
- Modify: any `.svelte` files with hardcoded hex colors

- [ ] **Step 1: Audit for hardcoded hex values**

Run: `grep -r '#[0-9a-fA-F]\{3,8\}' web/src/ --include='*.svelte'`

Review results. Exclude CSS comments and legitimate uses (e.g., inside `tokens.css` definitions).

- [ ] **Step 2: Replace all hardcoded hex with token references**

For each file, replace hex values with appropriate `var(--iris-color-*)` references.

- [ ] **Step 3: Commit**

```bash
git commit -m "fix: replace remaining hardcoded hex colors with design tokens"
```

---

## Chunk 5: Integration & Verification

### Task 18: Wire Feedback into Existing Actions

**Files:**
- Modify: `web/src/pages/Inbox.svelte` (archive, delete, star actions)
- Modify: `web/src/pages/ThreadView.svelte` (thread actions)
- Modify: `web/src/components/compose/ComposeModal.svelte` (send action)

- [ ] **Step 1: Add feedback calls to inbox bulk actions**

When archive/delete/star completes, call `feedback.success("Archived", { undo: () => undoAction() })` or similar. Wire undo function to the batch API to reverse the action.

- [ ] **Step 2: Add feedback calls to thread actions**

Same pattern for ThreadView's action bar (archive, delete, star, mute).

- [ ] **Step 3: Add feedback to compose send**

After successful send (post undo-send window), show `feedback.success("Email sent")`.

- [ ] **Step 4: Commit**

```bash
git commit -m "feat: wire feedback system into inbox, thread, and compose actions"
```

---

### Task 19: Pencil Prototype Validation

**Files:**
- Open: `docs/designs/iris.pen` (or create new screens)

- [ ] **Step 1: Prototype message row with badge priority**

Use Pencil to create a mockup of the new message row: single primary badge, "+N" overflow, hover actions on right, staggered animation representation.

- [ ] **Step 2: Prototype ThreadView grouped action bar**

Show Reply/Reply All/Forward + 3 dropdown menus with dividers.

- [ ] **Step 3: Prototype command palette**

Show the `Cmd+K` overlay with search input and command list.

- [ ] **Step 4: Prototype thread intelligence strip**

Show the compact "5 messages, 2 action items, deadline Mar 20" strip with expanded view.

- [ ] **Step 5: Screenshot and verify against design spec**

Use `get_screenshot` to capture each prototype. Verify colors match design tokens, spacing uses 4px base unit, typography follows scale.

- [ ] **Step 6: Commit any prototype updates**

```bash
git add docs/designs/
git commit -m "design: Pencil prototypes for Layer 1 UX components"
```

---

### Task 20: Full Build Verification

- [ ] **Step 1: Full frontend build**

Run: `cd /Users/divyekant/Projects/iris/web && npm run build`
Expected: Zero errors, zero warnings about unused imports.

- [ ] **Step 2: Backend build (ensure no breakage)**

Run: `cd /Users/divyekant/Projects/iris && cargo build`
Expected: Build succeeds (frontend changes should not affect backend).

- [ ] **Step 3: Run all existing tests**

Run: `cd /Users/divyekant/Projects/iris && cargo test`
Expected: All 975 tests pass. Layer 1 is frontend-only, so backend tests should be unaffected.

- [ ] **Step 4: Visual smoke test**

Start the server and manually verify:
- Inbox loads with new badge priority system
- Keyboard shortcuts work (j/k, Cmd+K, ?, e, s, #)
- Command palette opens and searches
- Toast notifications appear with animations
- ThreadView shows grouped action bar
- Settings tabs navigate via hash
- Chat panel slides in/out with animation

- [ ] **Step 5: Final commit if any fixes needed**

```bash
git commit -m "fix: address issues found in Layer 1 verification"
```

---

## Summary

| Chunk | Tasks | Key Deliverables |
|-------|-------|-----------------|
| 1: Foundation Utilities | 0-5 | Path aliases, transitions.ts, feedback.ts, Tooltip, DropdownMenu, Modal transitions |
| 2: Visual Hierarchy | 6-8 | Badge priority system, grouped action bar, hover action animations |
| 3: Keyboard-First | 9-12 | keyboard.ts, commands.ts, CommandPalette, migrated shortcuts, Cmd+K palette |
| 4: AI & Settings | 13-17 | Intelligence strip, AI suggestion strip, contextual chat, smart compose hints, right-click AI actions, settings transitions, token audit |
| 5: Integration | 18-20 | Feedback wiring, Pencil prototypes, full verification |

**Estimated commits:** ~22
**Dependency order:** Chunk 1 → Chunks 2, 3, 4 (parallel) → Chunk 5
