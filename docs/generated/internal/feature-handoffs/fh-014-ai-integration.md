---
status: draft
generated: 2026-03-15
source-tier: direct
hermes-version: 1.0.0
feature: ai-integration
slug: fh-014-ai-integration
id: fh-014
audience: internal
type: feature-handoff
---

# Feature Handoff: Woven AI Integration

## What It Does

The AI integration layer weaves AI capabilities directly into the thread reading experience rather than keeping them behind a separate panel. It surfaces actionable intelligence inline — a compact strip below the thread subject shows message count, extracted action items, and deadlines at a glance. For threads flagged as needing a reply, a second strip offers an AI-generated draft preview with a single button to open Compose pre-filled. Opening AI Chat while viewing a thread automatically loads that thread as context. Settings tab navigation gains a smooth crossfade transition. All remaining hardcoded color values are replaced with design tokens.

## How It Works

### Intelligence Strip

The intelligence strip reads from the message's existing AI metadata columns (`ai_summary`, `ai_action_items`, `ai_deadline`, `ai_category`) which are populated by the background classification pipeline. On ThreadView mount, if the thread has more than one message, the strip renders in collapsed state showing: message count, up to two action item excerpts, and the deadline if present. Clicking the strip toggles an expanded state that renders the full `ai_summary` text. State is local to the component (no API call needed on expand — data already loaded with the thread).

### AI Reply Suggestion Strip

When the last message in a thread has `ai_needs_reply = true`, the suggestion strip renders above the reply compose area. The suggested reply text is fetched via the existing AI assist endpoint (`POST /api/messages/{id}/ai-assist` with action `reply_suggestion`) or sourced from a pre-computed field if the classification pipeline stores it. The strip shows a truncated preview (first 120 characters). Clicking "Reply with this" calls the ComposeModal with `mode: "reply"`, pre-populating the body with the full suggestion text and the To/Subject fields from the thread.

### Contextual Chat

When the user clicks the Chat button from within ThreadView, the ChatPanel receives the current thread's subject and ID as initial context. The chat system prompt is extended with a thread context injection: `You are currently helping the user with the thread: "[subject]". Thread ID: {id}.` The panel header renders a "Chatting about: [subject]" label. When Chat is opened from the inbox (no thread active), no thread context is injected and the label is absent.

### Settings Tab Crossfade

The Settings page tab switcher uses a CSS transition on the tab panel container — `opacity` transitions over `var(--iris-transition-normal)` (200ms ease) with a brief stagger on the outgoing panel. The implementation uses Svelte's `transition:fade` directive or equivalent CSS class toggling so that the outgoing panel fades to 0 before the incoming panel fades to 1, preventing content overlap.

### Token Compliance

A sweep of all `.svelte` component files replaced remaining hardcoded `#` hex values and inline `rgb()`/`hsl()` color literals with the appropriate `var(--iris-color-*)` tokens from `web/src/tokens.css`. The sandboxed email iframe content is explicitly excluded — it renders third-party HTML and intentionally bypasses the token system.

## User-Facing Behavior

- Opening any thread with 2+ messages shows the intelligence strip immediately below the subject.
- For needs-reply threads, the suggestion strip appears in the lower thread area. One click opens Compose pre-filled.
- Chat opened from a thread shows the thread subject in the panel header and uses thread context in responses.
- Switching settings tabs produces a visible but fast fade transition.
- Brand switching (dark/light) applies consistently across all surfaces.

## Configuration

No new configuration variables. Relies on existing AI classification pipeline being active (`ai_enabled = true` in config).

## Common Questions

**Q: What if `ai_needs_reply` is true but no suggestion text is available yet?**
The suggestion strip is conditionally rendered — it only appears when suggestion text is non-empty. If the AI pipeline hasn't produced a suggestion yet, the strip is hidden. A future enhancement could show a "Generating..." placeholder.

**Q: Does the contextual chat send the full thread body to the AI, or just the subject?**
The current implementation injects subject and thread ID into the system prompt. The agentic chat tools (`search_emails`, `list_emails`) can then retrieve the full thread content on demand during the tool-calling loop. This avoids front-loading the entire thread into the prompt on every chat open.

**Q: Are the intelligence strip and suggestion strip visible in both dark and light brand modes?**
Yes. Both strips use design tokens for all color values, so they automatically adapt to the active brand palette. Strip backgrounds use `--iris-color-surface-raised` and borders use `--iris-color-border-subtle`.

## Files Affected

- `web/src/pages/ThreadView.svelte` — intelligence strip, suggestion strip, contextual chat wiring
- `web/src/components/ChatPanel.svelte` — context label, thread ID prop
- `web/src/pages/Settings.svelte` — tab crossfade transition
- `web/src/tokens.css` — no changes (tokens already defined)
- All `.svelte` files with remaining hardcoded colors (token compliance sweep)
