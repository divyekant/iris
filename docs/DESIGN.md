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
| Header | `App.svelte` | The Iris mark (a gold ring), and one link for each view. The active link has a gold line under it. On a phone the links scroll in one row. |
| Opening | `Brief.svelte` | The greeting and the date in muted text, then one sentence in `font-size-4` that says what needs you. |
| Line | `Line.svelte` | The round mark with the initials of the other party, the title, and one muted line of detail. The whole line opens the item. The envelope opens the source mail. A gold dot marks a conversation that waits for you. |
| Due tone | `Line.svelte` | A dot after the due date: `warning` for 2 days or less, `error` when late. The words say the same, so color is not the only sign. |
| Section | `Brief.svelte` | A heading with the count on the right, a line under it, at most 7 lines, and an "All N" link. An empty section does not show. |
| Code | `Code.svelte` | A login code in mono type, a Copy button, and the time left. It goes away when the code expires. |
| Quote | `Item.svelte` | The words of the mail that made the item, with a gold line on the left and the link to the mail. |
| Chips | `Brief.svelte`, `Records.svelte` | Round links or buttons with a count. The chosen chip and a chip under the pointer have a gold border. |
| Primary button | `Code.svelte`, `Setup.svelte` | A gold background with the `on-primary` text color. One for each screen at most. |
| Warning | `Brief.svelte`, `Mail.svelte` | A border of 4px on the left: `warning` for a notice, `error` for hostile mail or an error. |
| Access notice | `App.svelte` | Above every view: why the API cannot answer now (no sign-in, a used sign-in address, or no engine). It goes away at the next good answer. It uses the `warning` border. |
| Setup link | `App.svelte` | Above each view other than setup and the brief, when Iris has no account: a link to setup. The Brief link opens setup in that case. It uses the `warning` border. |
| Sign-in notice | `SignInNotice.svelte` | Above every view: the address of each account that needs sign-in, from the last status answer, and the command that signs in again. It uses the `warning` border. |

The token `on-primary` is the text color on gold: `#0a0a0a` in the dark
palette and `#2C2418` in the light palette. Both are colors of the palette, so
the count of colors does not change. `test/tokens.test.mjs` checks the contrast.

Gold is never the text color, because light gold on a light background fails
the contrast rule.

## Rules

- Each color comes from a token.
- Text meets the WCAG 2.1 AA contrast level (Web Content Accessibility Guidelines): a ratio of 4.5 or more.
- Each image has alt text.
- Motion shows a change of state. It respects the reduced-motion setting.
