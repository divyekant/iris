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

The client is an empty shell in release 0. Add each shared part here when it is
built.

## Rules

- Each color comes from a token.
- Text meets the WCAG 2.1 AA contrast level (Web Content Accessibility Guidelines): a ratio of 4.5 or more.
- Each image has alt text.
- Motion shows a change of state. It respects the reduced-motion setting.
