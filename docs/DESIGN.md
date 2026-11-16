# Design system

This file is the source of truth for each visual decision: colors, type, spacing
and the shared parts of the user interface (UI). Read it before you change a
screen. If the product has no UI, write "No UI." below and keep the file.

## Colors

`.kalos.yaml` holds the tokens. The web client is in `client/`. Dark is the
active brand. The light palette follows the viewer.

| Token | Dark | Light | Use |
|---|---|---|---|
| `primary` | `#d4af37` | `#b8960f` | Buttons, active states, the unread indicator |
| `secondary` | `#f5d060` | `#d4af37` | Second accent |
| `background` | `#0a0a0a` | `#FAF9F6` | Page background |
| `text` | `#e5e5e5` | `#2C2418` | Main text |
| `success` | `#16A34A` | `#16A34A` | Priority normal |
| `warning` | `#CA8A04` | `#CA8A04` | Priority high |
| `error` | `#DC2626` | `#DC2626` | Priority urgent |
| `info` | `#2563EB` | `#2563EB` | Information |

At most 12 unique colors. The unread indicator uses the brand gold, not blue.

## Type

- Font: `Inter, system-ui, sans-serif`. Mono font: `'SF Mono', 'Cascadia Mono', 'Menlo', monospace`.
- Scale: 1.25, from a 16px base. At most 3 font families.

## Spacing and layout

- Base unit 4px. Use multiples only: 0, 4, 8, 12, 16, 24, 32, 48, 64, 96, 128.
- Radii: none 0, sm 4, md 8, lg 12, full 9999.
- Transitions: fast 120ms ease, normal 200ms ease.

## Parts

Add each shared part here when it is built. The parts are in `client/src/views/`.

| Part | File | Use |
|---|---|---|
| Navigation bar | `App.svelte` | One link for each view. The active link has a gold line under it. |
| Line | `Line.svelte` | One item on one line: the title opens the item, the other party and the date in muted text, and "Mail" opens the source mail. |
| Section | `Brief.svelte` | A heading with a muted count, up to 7 lines, and an "All N" link to the full list. |
| Field list | `Item.svelte`, `Mail.svelte` | Labels in muted text and values in a grid of two columns. |
| Record table | `RecordTable.svelte` | One column for each field of the record type, and a "Mail" column. |
| Type choice | `Records.svelte` | Round buttons. The chosen button has a gold border. |
| Warning | `Brief.svelte`, `Mail.svelte` | A border of 4px on the left: `warning` for a notice, `error` for hostile mail. |
| Large code | `Item.svelte` | A login code in mono type at `font-size-4`. Never for an expired code. |
| Button link | `Setup.svelte` | A gold border of 2px and text in the `text` color. |

Gold is a border or a line, never the text color and never the background of
text, because light gold on a light background fails the contrast rule.

## Rules

- Each color comes from a token.
- Text meets the WCAG 2.1 AA contrast level (Web Content Accessibility Guidelines): a ratio of 4.5 or more.
- Each image has alt text.
- Motion shows a change of state. It respects the reduced-motion setting.
