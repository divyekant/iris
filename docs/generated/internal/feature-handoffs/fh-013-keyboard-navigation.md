---
id: fh-013
title: "Keyboard-First Navigation"
feature: keyboard-navigation
audience: internal
type: feature-handoff
generated: 2026-03-15
status: draft
source-tier: direct
hermes-version: 1.0.0
context-files: [web/src/lib/keyboard.ts, web/src/components/CommandPalette.svelte, web/src/components/ModeIndicator.svelte, web/src/components/HelpOverlay.svelte]
---

# Feature Handoff: Keyboard-First Navigation

## What It Does

Keyboard-First Navigation centralizes all keyboard shortcut handling into a single manager (`keyboard.ts`), adds a command palette (Cmd+K) for discoverability, introduces new shortcuts (`b` for snooze, `m` for mute), displays the current keyboard mode in a bottom-left indicator, and provides a dynamic help overlay (`?`) that reflects all registered shortcuts grouped by context.

Prior to this feature, shortcuts were registered inline across multiple components. This produced conflicts, made it difficult to add or modify shortcuts without risking regressions, and provided no user-facing way to discover available bindings. The centralized manager solves all three problems.

---

## How It Works

### Centralized Keyboard Manager (`keyboard.ts`)

`keyboard.ts` is the single source of truth for all keyboard bindings in the Iris frontend. It exposes:

- `registerShortcut(binding, handler, options)` — registers a binding with an optional mode scope, description, and suppression predicate.
- `unregisterShortcut(binding)` — removes a binding (used on component unmount).
- `setMode(mode: KeyboardMode)` — updates the active mode, scoping which shortcuts fire.
- `getShortcuts()` — returns the full list of registered shortcuts (used by HelpOverlay).

**Mode scoping**: Each shortcut can be registered with an optional `modes` array. If `modes` is omitted, the shortcut is global. If `modes` is provided, the shortcut only fires when `currentMode` matches one of the listed modes.

**Input suppression**: By default, shortcuts are suppressed when keyboard focus is inside an `<input>`, `<textarea>`, `[contenteditable]>`, or `<select>`. Individual shortcuts can override this with `allowInInput: true`.

**Chord detection**: Two-key chords (e.g., `g`+`i`) are implemented with a 1-second window. After the first key of a known chord prefix is pressed, the manager enters chord-pending state and awaits the second key. If the second key does not arrive within 1 second, chord state is reset silently.

---

### Command Palette (`CommandPalette.svelte`)

The command palette opens with `Cmd+K` (macOS) or `Ctrl+K` (other platforms). It is a modal overlay with an autofocused search input.

**Command registry**: Commands are registered as an array of `{ label, description, action, keywords? }` objects. Commands include:
- Navigation: "Go to Inbox", "Go to Sent", "Go to Drafts", "Go to Settings"
- Composition: "Compose New Message"
- Account: "Switch Account", "Add Account"
- Appearance: "Toggle Dark/Light Mode"

**Filtering**: Typing in the search input filters commands by fuzzy-matching the label, description, and optional keywords fields. The match is case-insensitive; substring matches rank higher than keyword matches.

**Execution**: Pressing `Enter` executes the highlighted command. Arrow keys (`↑`/`↓`) move the highlight. Clicking a command row executes it. In all cases, the palette closes immediately after execution.

**Closing**: `Escape` or clicking the backdrop closes the palette without executing any command.

---

### Shortcut Migration (Inline → Centralized)

Before this feature, the following shortcuts were registered inline in component `onMount` / `keydown` handlers:

| Shortcut | Previous location | Migrated to |
|---|---|---|
| `j` / `k` | `InboxView.svelte` onMount | `keyboard.ts` — mode: Inbox |
| `e` | `InboxView.svelte` onMount | `keyboard.ts` — mode: Inbox |
| `s` | `InboxView.svelte` onMount | `keyboard.ts` — mode: Inbox |
| `#` | `InboxView.svelte` onMount | `keyboard.ts` — mode: Inbox |

All migrated shortcuts have identical behavior to their inline predecessors. The migration is a refactor only — no user-visible behavior changed for existing shortcuts.

---

### New Shortcuts

**`b` — Snooze focused message**

Registered in `keyboard.ts` for mode: `Inbox`. Opens a snooze picker for the currently focused inbox row. The picker offers quick-snooze presets (Later today, Tomorrow morning, Next week, Custom date/time). On selection, calls the snooze API (`PUT /api/messages/{id}/snooze` or the follow-up reminders snooze endpoint) and removes the message from the inbox view.

**`m` — Mute focused thread**

Registered in `keyboard.ts` for mode: `Inbox`. Mutes the thread of the currently focused inbox row. After muting, future replies to the thread are silently archived and do not surface in the inbox. Calls `PUT /api/threads/{thread_id}/mute`. A brief toast confirms the action.

---

### Mode Indicator (`ModeIndicator.svelte`)

A subtle fixed badge in the bottom-left corner of the app viewport. It reads the `currentMode` value from the keyboard manager store and renders a small pill with the mode name.

**Modes and labels**:
| `KeyboardMode` enum value | Displayed label |
|---|---|
| `Inbox` | Inbox |
| `Thread` | Thread |
| `Compose` | Compose |
| `Settings` | Settings |
| `CommandPalette` | Command |
| `Search` | Search |

The indicator uses `--iris-color-text-faint` for text and a semi-transparent background — intentionally low-contrast to avoid drawing attention. It disappears in contexts where keyboard navigation is not meaningful (e.g., when an OAuth popup is active).

---

### Help Overlay (`HelpOverlay.svelte`)

Triggered by pressing `?` from any keyboard-navigable view (suppressed inside inputs). Renders a modal displaying all shortcuts currently registered in `keyboard.ts`, grouped by mode.

**Rendering logic**: The overlay calls `getShortcuts()` on mount to get the live shortcut list. It groups entries by `mode` (global entries are listed under "Anywhere"). Within each group, entries are sorted alphabetically by key binding.

**Chord display**: Chord shortcuts show both keys with an arrow connector (e.g., `g` → `i`).

**Closing**: `Escape`, clicking the backdrop, or pressing `?` again closes the overlay.

---

## User-Facing Behavior Summary

| Trigger | Action |
|---|---|
| `Cmd+K` | Opens command palette |
| `j` | Focus next inbox message |
| `k` | Focus previous inbox message |
| `e` | Archive focused message |
| `s` | Toggle star on focused message |
| `#` | Delete (trash) focused message |
| `b` | Open snooze picker for focused message |
| `m` | Mute focused message's thread |
| `g` → `i` | Navigate to Inbox |
| `g` → `s` | Navigate to Sent |
| `g` → `d` | Navigate to Drafts |
| `?` | Open help overlay |

All shortcuts above are suppressed when focus is inside a text input.

---

## Configuration

No user-configurable settings. The chord timeout (1 second) is a constant in `keyboard.ts`. Custom shortcut remapping is not supported in this iteration.

---

## Edge Cases & Limitations

- **Chord timeout**: If the user presses `g` and then pauses for more than 1 second, the chord resets. The key `g` is not forwarded as a character because chord prefixes are swallowed by the manager. This is intentional.
- **Mode synchronization**: The keyboard manager mode is updated by each route/view on mount via `setMode()`. If a view fails to call `setMode()`, the indicator and mode-scoped shortcuts may show stale state.
- **Snooze and mute require focus**: `b` and `m` act on the "focused" inbox row (the row with the keyboard cursor, not mouse hover). If no row is focused (e.g., inbox just loaded with no `j`/`k` navigation), the shortcuts do nothing — a toast should inform the user to select a message first.
- **Command palette conflict with browser Cmd+K**: On some browsers, `Cmd+K` opens the address bar or a link dialog. `keyboard.ts` calls `event.preventDefault()` on `Cmd+K` in the app context. This should work in full-page app mode but may fail inside iframes.
- **Help overlay is read-only**: Users cannot customize or remap shortcuts from the overlay. The overlay is discovery-only.

---

## Common Questions

**Q: A user says pressing `j` or `k` types into the search bar instead of navigating.**
The shortcuts have input-suppression logic. If `j`/`k` are firing inside an input, the suppression predicate in `keyboard.ts` may not be catching that input element. Verify the element has a standard `tagName` of INPUT, TEXTAREA, or SELECT, or that it has `contenteditable="true"`. Non-standard input widgets (e.g., rich text editors implemented as `<div>`) need explicit `allowInInput: false` or a suppression hook. Check the component for missing `data-no-shortcuts` attributes if the project uses that pattern.

**Q: The command palette does not open on `Cmd+K`.**
First check whether another browser extension or the OS is intercepting `Cmd+K`. In Chrome, `Cmd+K` is used to insert links in some contexts — verify the app is the focused document, not a DevTools panel or extension popup. If the shortcut works in other browsers, it is likely an extension conflict. If it fails in all browsers, verify `keyboard.ts` is initialized on app mount and that the `CommandPalette` component is mounted in the app root layout.

**Q: A user reports that `b` (snooze) does nothing when they press it.**
`b` only fires when a message row is focused (highlighted by keyboard navigation). If the user has not yet used `j`/`k` to navigate to a message, no row is focused and the shortcut is a no-op. Advise the user to press `j` once to focus the first message, then press `b`. If the shortcut still does nothing after focusing a row, check that the `KeyboardMode` is correctly set to `Inbox` and that the shortcut registration in `keyboard.ts` is scoped to the `Inbox` mode.

---

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Shortcut fires inside compose area | Input suppression not catching compose element | Verify compose input uses standard `<input>` or `<textarea>` tag; add explicit suppression if using `contenteditable` |
| Mode indicator shows wrong mode | View not calling `setMode()` on mount | Inspect the view component for a `keyboard.setMode(KeyboardMode.X)` call in `onMount` |
| Help overlay shows duplicate entries | Shortcut registered twice (e.g., in both a parent and child component) | Ensure `registerShortcut` is only called once per binding; check for duplicate `onMount` calls |
| Chord `g+i` navigates but `g+s`/`g+d` do not | Partial chord registration | Verify all three chord handlers are registered in `keyboard.ts` init; check for typos in binding strings |
| Command palette search returns no results | Command registry empty or not initialized | Verify `CommandPalette.svelte` imports and iterates the command registry on mount |

---

## Related Links

- Frontend: `web/src/lib/keyboard.ts`
- Components: `web/src/components/CommandPalette.svelte`, `web/src/components/ModeIndicator.svelte`, `web/src/components/HelpOverlay.svelte`
- Snooze API: `src/api/followups.rs` (PUT /api/followups/{id}/snooze)
- Mute API: `src/api/threads.rs` (PUT /api/threads/{id}/mute)
- Related: FH-025 (Follow-up Reminders — snooze endpoint), FH-011 (Mute Thread — mute endpoint)
- Design tokens: `web/src/tokens.css` — `--iris-color-text-faint` (mode indicator), `--iris-color-primary` (command palette highlight)
