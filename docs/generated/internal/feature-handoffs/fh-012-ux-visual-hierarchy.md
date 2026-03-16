---
id: fh-012
title: "Visual Hierarchy & Interaction Polish"
feature: ux-visual-hierarchy
audience: internal
type: feature-handoff
generated: 2026-03-15
---

# Visual Hierarchy & Interaction Polish

## What Changed

Three improvements were shipped in Wave 3 Layer 1 Chunk 2:

1. **Smart Badge Priority System** — Inbox message rows previously displayed multiple AI-classification badges inline (intent, priority, sentiment, category, needs-reply, deadline). Each row now shows exactly one primary badge, chosen by a fixed priority algorithm, with a "+N" chip indicating how many additional badges are hidden. New utility module: `web/src/lib/badge-priority.ts`. Modified component: `web/src/components/inbox/MessageRow.svelte`. Rows that are archived or deleted now play an `irisCollapse` exit animation before disappearing.

2. **Grouped ThreadView Action Bar** — The flat row of 10+ buttons in `web/src/pages/ThreadView.svelte` has been reorganized into four zones: Primary actions (Reply, Reply All, Forward — always visible), an Organize dropdown (Star, Snooze, Archive, Delete), an AI dropdown (Summarize, Extract Tasks, Generate Replies), and a More dropdown (Spam, Mute, Redirect). Dropdowns are powered by the new shared `DropdownMenu` component.

3. **Staggered Hover Animations** — The four per-row hover action buttons (Archive, Delete, Star, Snooze) now appear with a 30 ms staggered fade-in. Previously they all appeared simultaneously on hover. The animation comes from a `staggeredFade` function in a new shared transitions library.

## How It Works

### Badge Priority Algorithm
The algorithm is implemented in `web/src/lib/badge-priority.ts` and runs client-side at render time. Given the full set of AI metadata on a message, it selects the single highest-priority badge using this precedence order:

1. `needs_reply` — the message is waiting on a response from the user
2. `deadline` — a deadline was extracted from the message body
3. `intent` — the detected intent (e.g., action request, question)
4. `sentiment` — detected sentiment (e.g., urgent, frustrated)
5. `category` — AI-assigned category (e.g., Primary, Updates)

The remaining badges are counted and rendered as a `+N` chip. Clicking the chip expands to show all badges inline (behavior unchanged from the pre-Wave 3 multi-badge display).

### Action Bar Grouping
`ThreadView.svelte` now renders four sections:
- **Primary** (always visible): Reply, Reply All, Forward
- **Organize** dropdown: Star/Unstar, Snooze, Archive, Delete
- **AI** dropdown: Summarize (calls `/api/threads/:id/summarize`), Extract Tasks (calls AI assist endpoint), Generate Replies
- **More** dropdown: Report Spam, Mute Thread, Redirect

The `DropdownMenu` component is a new shared component in `web/src/components/shared/`. It handles keyboard navigation, focus trap, and outside-click dismiss.

### Staggered Fade Animation
`staggeredFade` accepts an index parameter and computes `delay = index * 30ms`. The hover action buttons (index 0–3) appear at 0 ms, 30 ms, 60 ms, and 90 ms respectively. This gives the perception of a smooth sweep rather than a sudden block of controls appearing.

## Impact on Users

- **Cleaner inbox rows** — one badge instead of three to five eliminates visual clutter on dense inboxes. The `+N` chip preserves access to full metadata without hiding it entirely.
- **Faster thread actions** — Reply, Reply All, and Forward no longer compete visually with a dozen other buttons. Power users who use archive/delete/star regularly find them one click away in the Organize dropdown.
- **Smoother feel** — the collapse animation on archive/delete gives clear feedback that the row is being processed and prevents the jarring "row disappeared instantly" effect. The staggered hover fade makes the UI feel more responsive and polished.

## Common Questions

**Q: Why is the badge my user sees different from what they expect?**
A: The badge shown is determined by the priority algorithm — `needs_reply` always wins if the AI detected a reply is expected. If the user thinks the wrong badge is shown, ask them to use the AI feedback thumbs-down on the classification; corrections feed back into the model over time.

**Q: A user says their reply/forward buttons are missing — how do I help them?**
A: The Primary actions (Reply, Reply All, Forward) are always visible and cannot be collapsed or hidden. If they are not seeing them, the most likely cause is a display resolution issue or a stale browser cache. Ask them to hard-refresh (Cmd+Shift+R on Mac).

**Q: A user says the Organize/AI/More dropdowns aren't opening.**
A: These use the new `DropdownMenu` component. If a dropdown is unresponsive, check whether the user is on a very old browser (pre-ES2022 support). Also confirm their Iris instance is on the Wave 3 Layer 1 build — earlier builds don't have the grouped action bar.

**Q: The "+N" chip shows but clicking it doesn't expand the badges.**
A: This would be a client-side JavaScript error. Ask the user to open the browser console (F12) and look for errors in the `badge-priority` module. Report to engineering with the message ID if reproducible.

**Q: Does the staggered animation affect accessibility (reduced-motion preferences)?**
A: Yes — the `staggeredFade` transition respects the `prefers-reduced-motion` media query. When reduced motion is enabled in the OS, all hover actions appear instantly with no animation.

## Related Features

- **AI Classification** (`fh-006`) — The badge data comes from the AI pipeline that runs on message ingest. Priority badges (urgent, high, normal, low) feed into the badge priority algorithm.
- **Thread Actions** (`fh-004`) — Archive, delete, star, snooze, and mute actions are unchanged in behavior; only their UI placement has changed.
- **AI On-Demand** (`fh-007`) — The AI dropdown in the action bar surfaces thread summarization and task extraction, which were previously only accessible via the collapsible summary panel.
- **Wave 2 Batch 4** (`fh-010`) — Snooze and follow-up reminder features are now surfaced directly in the Organize dropdown for faster access.
