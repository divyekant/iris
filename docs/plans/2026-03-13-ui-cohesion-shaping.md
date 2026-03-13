---
shaping: true
---

# UI Cohesion Pass — Shaping

## Source

> Same goes for what we built in wave1 as well. So how will we go about it? first we will write all options, bifurcate them in areas needed for improvement and then prototype them on Pencil before making any code changes.

48 Svelte components built across V1–V11 + Wave 2 (51 features in 9 batches) by parallel agents prioritizing velocity over consistency. Settings is a 2,096-line monolith. Message rows have 6+ inline badges competing for attention. 4+ dialogs each implement their own backdrop/card/button pattern. No shared form or modal components.

## Problem

The UI works but feels like a patchwork — each feature was built in isolation by different agents. Settings is unwieldy, inbox rows are cluttered with metadata, dialogs look slightly different from each other, and there's no shared component library for common patterns (forms, modals, badges). The app needs a cohesion pass that makes it feel like one product, not 51 features bolted together.

## Outcome

A unified UI where every surface follows the same interaction patterns, badge density is intentional not accidental, settings is navigable, and shared components eliminate visual drift.

---

## Requirements (R)

| ID | Requirement | Status |
|----|-------------|--------|
| R0 | Every surface feels like one product — consistent spacing, typography, interaction patterns | Core goal |
| R1 | Settings is navigable — users can find what they need without scrolling 2,000 lines | Must-have |
| R2 | Message rows show the right metadata at the right time — not everything at once | Must-have |
| R3 | Dialogs/modals share a common pattern — backdrop, card, buttons, escape-to-close | Must-have |
| R4 | Form inputs have consistent styling — borders, focus rings, error states, save feedback | Must-have |
| R5 | All colors reference design tokens — zero hardcoded hex values | Must-have |
| R6 | Shared component library for badges, pills, toggles, section headers | Nice-to-have |
| R7 | ThreadView action bar is organized — not a growing row of icon buttons | Nice-to-have |
| R8 | Quick actions on hover don't obscure message content | Nice-to-have |

---

## Area 1: Settings Architecture

**Current:** Single 2,096-line Settings.svelte with 11 inline sections, ~60 state variables, all loaded eagerly on mount.

### A: Tab Navigation (Grouped Sections)

| Part | Mechanism |
|------|-----------|
| **A1** | Vertical sidebar tabs: General, AI, Identity, Security, Advanced |
| **A2** | Each tab is a standalone component (SettingsGeneral.svelte, SettingsAI.svelte, etc.) |
| **A3** | Active tab loaded on click, others not rendered — reduces initial load |
| **A4** | Tab state in URL hash (#settings/ai) for direct linking |
| **A5** | Section headers within each tab for sub-grouping |

### B: Accordion Sections (Collapsible)

| Part | Mechanism |
|------|-----------|
| **B1** | Each of the 11 sections becomes a collapsible accordion panel |
| **B2** | One section open at a time (or multi-open mode) |
| **B3** | Still a single file but with a SettingsSection wrapper component |
| **B4** | No URL state — just scroll position |

### C: Searchable Settings (Modern App Style)

| Part | Mechanism |
|------|-----------|
| **C1** | Search bar at top of settings — filters sections by keyword |
| **C2** | Tab groups (like A) + search overlay |
| **C3** | Each setting item is a discrete component with metadata (name, description, keywords) |
| **C4** | Jump-to from search results |

---

## Area 2: Message Row Information Density

**Current:** MessageRow shows up to 6 badges inline: category pill, sentiment pill, needs-reply pill, intent badge, labels, plus priority dot and attachment icon. On a narrow viewport this overflows.

### D: Progressive Disclosure (Show Less, Expand More)

| Part | Mechanism |
|------|-----------|
| **D1** | Default row shows: sender, subject, snippet, date, unread dot, attachment icon |
| **D2** | Primary badge slot: show ONE most important signal (needs-reply > intent > category) |
| **D3** | Secondary signals shown on hover/focus as a tooltip or mini-popover |
| **D4** | Sentiment, labels, priority shown in ThreadView header instead of row |

### E: Density Modes (User Choice)

| Part | Mechanism |
|------|-----------|
| **E1** | Compact mode: sender + subject + date only (Gmail dense style) |
| **E2** | Default mode: sender + subject + snippet + 1 badge + date |
| **E3** | Expanded mode: all badges visible (current behavior) |
| **E4** | Density toggle in Settings > General |

### F: Smart Badge Priority (Algorithmic)

| Part | Mechanism |
|------|-----------|
| **F1** | Badge priority ranking: needs_reply > deadline > intent > sentiment > category > labels |
| **F2** | Show max 2 badges based on priority ranking |
| **F3** | If more than 2 qualify, show "+N" overflow indicator |
| **F4** | Click overflow to see all badges in a mini-popover |

---

## Area 3: Modal/Dialog Pattern

**Current:** SpamDialog, RedirectDialog, SnoozePicker, ContactTopicsPanel each implement their own fixed-inset-0 backdrop, escape handler, card styling.

### G: Shared Modal Component

| Part | Mechanism |
|------|-----------|
| **G1** | `Modal.svelte` — backdrop (click-outside-close), escape handler, card container with consistent radii/padding/border |
| **G2** | Props: `size` (sm/md/lg), `title`, `onclose` |
| **G3** | Slot-based content — each dialog provides its own body |
| **G4** | Shared `ModalActions.svelte` — consistent button row (cancel left, primary right) |
| **G5** | Refactor SpamDialog, RedirectDialog, SnoozePicker, ContactTopicsPanel to use Modal wrapper |

---

## Area 4: Form Components

**Current:** Every form section in Settings duplicates: input styling, save button, loading state, error feedback. No shared patterns.

### H: Shared Form Primitives

| Part | Mechanism |
|------|-----------|
| **H1** | `FormInput.svelte` — styled text input with label, placeholder, error state, disabled state |
| **H2** | `FormSelect.svelte` — styled select with consistent border/focus |
| **H3** | `FormToggle.svelte` — iOS-style toggle (replace the manual toggle in Appearance section) |
| **H4** | `FormSection.svelte` — section header (h3 uppercase tracking-wider) + description + content slot |
| **H5** | `SaveButton.svelte` — button with loading spinner, "Saved ✓" flash on success |
| **H6** | Consistent focus ring: `focus:ring-2 ring-[var(--iris-color-primary)]` via shared CSS class |

---

## Area 5: Token Compliance

**Current:** 2 files with hardcoded hex (Search.svelte operatorColors, ContactTopicsPanel pillColors).

### I: Fix Hardcoded Colors

| Part | Mechanism |
|------|-----------|
| **I1** | Search.svelte: replace `operatorColors` hex values with `var(--iris-color-info)`, `var(--iris-color-success)`, etc. — read from CSS custom properties at runtime |
| **I2** | ContactTopicsPanel.svelte: replace `pillColors` hex array with token references |
| **I3** | Add missing semantic tokens to tokens.css if needed (e.g., `--iris-color-info` if not present) |

---

## Area 6: ThreadView Action Organization

**Current:** ThreadView has a growing action bar with 10+ icon buttons: Star, Archive, Mark Read, Delete, Summarize, Snooze, Spam, Mute, Redirect, Tasks, Multi-Reply. No grouping.

### J: Grouped Action Bar

| Part | Mechanism |
|------|-----------|
| **J1** | Primary actions visible: Reply, Reply All, Forward (most used) |
| **J2** | Organize actions: group {Star, Snooze, Archive, Delete} as "Organize" |
| **J3** | AI actions: group {Summarize, Tasks, Multi-Reply} under AI dropdown |
| **J4** | Safety actions: group {Spam, Mute, Redirect} under "More" menu |
| **J5** | Dividers between groups for visual separation |

---

## Area 7: Quick Actions Refinement

**Current:** MessageRow has 7 hover actions (Archive, Delete, Mark Read, Star, Snooze, Topics, Spam) that can obscure message content on narrow rows.

### K: Streamlined Quick Actions

| Part | Mechanism |
|------|-----------|
| **K1** | Reduce to 4 primary actions: Archive, Delete, Star, Snooze |
| **K2** | Secondary actions (Topics, Spam, Mark Read) accessible via right-click context menu or overflow "..." |
| **K3** | Actions appear on the right, overlaying the date (not the subject/sender) |
| **K4** | Consistent icon sizing (14px) and spacing (gap-1) |

---

## Fit Check

| Req | Requirement | Status | A | B | C | D | E | F | G | H | I | J | K |
|-----|-------------|--------|---|---|---|---|---|---|---|---|---|---|---|
| R0 | Every surface feels like one product | Core goal | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| R1 | Settings is navigable | Must-have | ✅ | ✅ | ✅ | — | — | — | — | — | — | — | — |
| R2 | Right metadata at the right time | Must-have | — | — | — | ✅ | ✅ | ✅ | — | — | — | — | — |
| R3 | Dialogs share common pattern | Must-have | — | — | — | — | — | — | ✅ | — | — | — | — |
| R4 | Consistent form inputs | Must-have | — | — | — | — | — | — | — | ✅ | — | — | — |
| R5 | All colors use tokens | Must-have | — | — | — | — | — | — | — | — | ✅ | — | — |
| R6 | Shared component library | Nice-to-have | — | — | — | — | — | — | ✅ | ✅ | — | — | — |
| R7 | ThreadView actions organized | Nice-to-have | — | — | — | — | — | — | — | — | — | ✅ | — |
| R8 | Quick actions don't obscure content | Nice-to-have | — | — | — | — | — | — | — | — | — | — | ✅ |

**Notes:**
- Areas are independent — each addresses a different requirement subset
- Within each area, options ARE mutually exclusive (pick one per area)
- — means the option doesn't address that requirement (expected)

## Recommended Composition

**Settings:** A (Tab Navigation) — best navigability, enables direct linking, proper code splitting
**Message Rows:** F (Smart Badge Priority) — algorithmic, no user config needed, graceful degradation
**Modals:** G (only option, and it's the right one)
**Forms:** H (only option, foundational)
**Tokens:** I (only option, straightforward fix)
**ThreadView:** J (Grouped Action Bar) — prevents toolbar from growing unbounded
**Quick Actions:** K (Streamlined) — 4 primary actions is the sweet spot

## Next Steps

1. Prototype on Pencil: Settings tabs (A), message row density (F), ThreadView action bar (J)
2. Review prototypes
3. Build shared components first: Modal (G), Form primitives (H), Token fixes (I)
4. Then Settings refactor (A), message row (F), ThreadView (J), quick actions (K)
