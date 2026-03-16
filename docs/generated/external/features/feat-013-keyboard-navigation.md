---
id: feat-013
title: "Keyboard Shortcuts & Command Palette"
feature: keyboard-navigation
audience: external
type: feature-doc
generated: 2026-03-15
status: draft
source-tier: direct
hermes-version: 1.0.0
---

# Keyboard Shortcuts & Command Palette

## Overview

Iris is built for keyboard-first use. Every common action — reading, archiving, starring, navigating, composing — is available without touching the mouse. A command palette (`Cmd+K`) gives you instant access to any command by typing a few characters. A mode indicator in the bottom-left corner tells you which keyboard context you are in, and pressing `?` opens a full help overlay listing every available shortcut.

---

## Command Palette

Press **`Cmd+K`** (macOS) or **`Ctrl+K`** (Windows/Linux) from anywhere in the app to open the command palette.

Start typing to filter commands. Press `Enter` to execute the highlighted command. Press `Escape` to close without acting.

**Example commands available in the palette:**

| Command | What it does |
|---|---|
| Compose New Message | Opens the compose modal |
| Go to Inbox | Navigates to your inbox |
| Go to Sent | Navigates to sent mail |
| Go to Drafts | Navigates to your drafts |
| Toggle Dark / Light Mode | Switches the app theme |
| Switch Account | Opens the account switcher |

You can also reach any of these with the shortcuts listed below — the palette is for discoverability and for actions you don't yet have memorized.

---

## Shortcuts by Context

### Anywhere (Global)

| Key | Action |
|---|---|
| `Cmd+K` / `Ctrl+K` | Open command palette |
| `?` | Open keyboard shortcuts help overlay |

### Inbox Navigation

These shortcuts work when the inbox is active and your cursor is not inside a text field.

| Key | Action |
|---|---|
| `j` | Move focus to the next message |
| `k` | Move focus to the previous message |
| `e` | Archive the focused message |
| `s` | Star / unstar the focused message |
| `#` | Move the focused message to trash |
| `b` | Snooze the focused message |
| `m` | Mute the focused message's thread |

**Using `b` (snooze):** A quick-select menu appears with options like "Later today", "Tomorrow morning", and "Next week". Pick one and the message disappears from your inbox until that time.

**Using `m` (mute):** Muting a thread means future replies will be silently archived and will not appear in your inbox. Useful for high-volume threads you have been CC'd on but do not need to follow.

### Navigation Chords

These are two-key sequences. Press the first key, then the second key within one second.

| Keys | Action |
|---|---|
| `g` → `i` | Go to Inbox |
| `g` → `s` | Go to Sent |
| `g` → `d` | Go to Drafts |

---

## Mode Indicator

A subtle badge in the **bottom-left corner** of the app shows your current keyboard context:

| Indicator | Meaning |
|---|---|
| **Inbox** | Inbox shortcuts (`j`, `k`, `e`, `s`, `#`, `b`, `m`) are active |
| **Thread** | A thread is open; thread-level shortcuts are active |
| **Compose** | Compose modal is open |
| **Settings** | Settings page is active |
| **Command** | Command palette is open |

The indicator is intentionally understated — it is a reference, not an alert. Shortcuts specific to the current mode only fire when that mode is active.

---

## Help Overlay

Press **`?`** at any time to open the shortcuts help overlay. It shows all currently available shortcuts grouped by context — exactly what `keyboard.ts` has registered at that moment, so it is always up to date.

To close the overlay: press `Escape`, press `?` again, or click outside the dialog.

---

## Tips

**Shortcuts only fire outside of text inputs.** If you are typing in the search bar, compose area, or any text field, pressing `j` types a "j" — it does not navigate. Click or `Tab` out of the input first.

**No memorization required.** If you forget a shortcut, press `?` for the full list or `Cmd+K` and type what you want to do.

**Chords have a one-second window.** After pressing `g`, you have one second to press the second key (`i`, `s`, or `d`). If you pause longer, the chord resets silently.

---

## FAQ

**Do shortcuts work in all views?**
Global shortcuts (`Cmd+K`, `?`) work everywhere. Inbox shortcuts (`j`, `k`, `e`, `s`, `#`, `b`, `m`) only work when the inbox view is active. The mode indicator shows you which context you are in.

**Can I customize or remap shortcuts?**
Not in the current version. Shortcut remapping is planned for a future release.

**I pressed `b` but nothing happened.**
Make sure a message row is selected first — press `j` once to focus the top message, then press `b`. If focus is not on a message row, the snooze shortcut is a no-op.

**Does `Cmd+K` conflict with my browser?**
In most cases no, because Iris intercepts `Cmd+K` at the app level before the browser handles it. In rare cases a browser extension may intercept the key first. If this happens, use the command palette button in the top navigation bar as a fallback.

---

## Related

- [Follow-Up Reminders](feat-022-followup-reminders.md) — snooze is also available from the follow-up reminders panel
- [Mute Thread](feat-mute-thread.md) — full details on muting behavior and re-enabling muted threads
