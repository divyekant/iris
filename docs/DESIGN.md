# Design system

This file is the source of truth for each visual decision: colors, type, spacing
and the shared parts of the user interface (UI). Read it before you change a
screen. If the product has no UI, write "No UI." below and keep the file.

## Colors

`.kalos.yaml` holds the tokens. The web client is in `client/`. Dark is the
active brand. The light palette follows the viewer.

The palette is "Iris": iris violet on cool ink neutrals (card R1-27,
decision D4). The violet is the color of the product. It marks the active view,
focus, the main button, information (`info`) and a conversation that waits
for you (`unread`). It is never `success`, `warning` or `error`, so it does
not look like a good or a bad state.

| Token | Dark | Light | Use |
|---|---|---|---|
| `primary` | `#a394ff` | `#5b47d6` | Buttons, the active view, focus, the Iris mark |
| `background` | `#0e0f14` | `#f7f7fa` | Page background |
| `background-elevated` | `#15161d` | `#ffffff` | Raised areas, for example the login code |
| `background-surface` | `#1c1e27` | `#eeeef4` | Chips, initials, inline code |
| `text` | `#e7e8ee` | `#1b1c24` | Main text |
| `text-muted` | `#a4a7b5` | `#555867` | Detail lines |
| `text-faint` | `#8c8fa0` | `#62657a` | The least important text |
| `border` | `#2c2e3b` | `#dcdde6` | Lines under headings, boxes |
| `border-subtle` | `rgba(255,255,255,0.06)` | `rgba(0,0,0,0.06)` | Quiet separators |
| `success` | `#5fd38d` | `#15703a` | Done, priority normal. It can be text. |
| `warning` | `#f2b84b` | `#b7750c` | Notices, due soon, priority high. Lines and dots only. |
| `error` | `#f47c7c` | `#c0262d` | Hostile mail, late, priority urgent. Lines and dots only. |
| `info` | = `primary` | = `primary` | Information |
| `unread` | = `primary` | = `primary` | A conversation that waits for you |
| `on-primary` | = `background` | = `background-elevated` | Text on a `primary` background |

Each palette has 12 unique colors or fewer. `info`, `unread` and `on-primary`
reuse a color of the palette. The semantic colors are set for each palette,
because one value cannot have enough contrast on both a dark and a light
background.

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
| Header | `App.svelte` | The Iris mark (a `primary` ring around a dot, like an iris), and one link for each view. The active link has a `primary` line under it. On a phone the links scroll in one row. |
| Opening | `Brief.svelte` | The greeting and the date in muted text, then one sentence in `font-size-4` that says what needs you. |
| Line | `Line.svelte` | The round mark with the initials of the other party, the title, and one muted line of detail. The whole line opens the item. The envelope opens the source mail. An `unread` dot marks a conversation that waits for you. |
| Bundle line | `Bundle.svelte` | One `bundle` rule on one line, in the section "Bundled by your rules" of the brief: a stack mark (a round mark with the count, and a second mark behind it), the rule name, and one muted line that names up to 3 senders. A chevron takes the place of the envelope. The whole line opens the list of the bundle. |
| Handled count | `Brief.svelte` | The section "Handled without you" of the brief: the count, one muted sentence with the hidden items and the messages that a rule decided (an approved change can still wait to apply, so the brief does not say "changed"), and the link "See what and why". It does not show when the count is 0. |
| Handled list | `Handled.svelte` | The actions that a rule decided, under "Decided by your rules" (the verb in bold, the message count, the reason in muted text, and the state and time in faint text), then the hidden items as Lines with the reason as a note. Each list loads 50 at a time. |
| Line note | `Line.svelte` | One more muted line under the detail, for example the reason of the rule that hides the item. |
| Due tone | `Line.svelte` | A dot after the due date: `warning` for 2 days or less, `error` when late. The words say the same, so color is not the only sign. |
| Section | `Brief.svelte` | A heading with the count on the right, a line under it, at most 7 lines, and an "All N" link. An empty section does not show. |
| Code | `Code.svelte` | A login code in mono type, a Copy button, and the time left. It goes away when the code expires. |
| Quote | `Item.svelte` | The words of the mail that made the item, with a `primary` line on the left and the link to the mail. |
| Chips | `Brief.svelte`, `Records.svelte` | Round links or buttons with a count. The chosen chip and a chip under the pointer have a `primary` border. |
| Primary button | `Code.svelte`, `Setup.svelte` | A `primary` background with the `on-primary` text color. One for each screen at most. |
| Warning | `Brief.svelte`, `Mail.svelte` | A border of 4px on the left: `warning` for a notice, `error` for hostile mail or an error. |
| Access notice | `App.svelte` | Above every view: why the API cannot answer now (no sign-in, a used sign-in address, or no engine). It goes away at the next good answer. It uses the `warning` border. |
| Setup link | `App.svelte` | Above each view other than setup and the brief, when Iris has no account: a link to setup. The Brief link opens setup in that case. It uses the `warning` border. |
| Sign-in notice | `SignInNotice.svelte` | Above every view: the address of each account that needs sign-in, from the last status answer, and the command that signs in again. It uses the `warning` border. |
| Policy notice | `PolicyNotice.svelte` | Above the brief: the policy file changed and waits for a confirmation (`warning` border), or it has an error (`error` border). A link opens the rules. |
| Mode choice | `Rules.svelte` | Two cards side by side (one column on a phone), each a radio button. The chosen card has a primary border and a ring with a primary center, the mark of Iris. A change asks for one more click on the primary button. |
| Rule line | `Rules.svelte` | The name, the verbs as words, each condition as a chip, and the rule number and line in small muted text. The Remove button stays quiet until the pointer or the focus is on the line. A remove asks one question in place, with no dialog. |
| Request | `Request.svelte` | One changeset in the review: the mark of the author (a `primary` ring, or the Iris mark for Iris triage), the reason of the author as a quote with a `primary` line, then each change as a sentence (the verb in bold, then the real count), its reason and its flags (a `warning` dot and words). The changes that have a decision are behind one quiet line, "N more changes have a decision". When nothing waits, the line opens. |
| Decide | `Request.svelte` | Design B (the owner's choice): a round tick box before each waiting change, ticked at the start (`primary` fill, an `on-primary` tick). An unticked change goes to half opacity and says "Iris will deny this change.". One `primary` pill button says what happens ("Approve all 3", "Approve 2, deny 1", "Deny all 3"). With no tick, the pill has no fill. The button acts only when each row of each waiting change loaded; until then a quiet line says that Iris loads them. After the decision, one quiet line says what the person approved and denied. |
| Withdraw | `Request.svelte` | A quiet "Withdraw" in faint text after the request ID, in the head of an open request (option A, the owner's pick). A click opens one question under the head, with a `warning` line on the left: "Withdraw this request?", what Iris cancels, what stays, and "You cannot undo a withdraw." An outline pill withdraws, and a quiet "Keep it" closes the question. A withdrawn request says so in one quiet line, and each cancelled change reads "Withdrawn". |
| Message box | `Request.svelte` | The real messages of one change: sender, subject and date in one raised box with quiet lines. On a phone the subject goes under the sender. A gone message shows as words. Each row of the change shows: after about six rows, the box scrolls. |
| Change panel | `Rules.svelte` | A raised panel with a primary border on the left: each change of the file, with a dot of color and a word (New `success`, Changed `warning`, Removed `error`), the file text behind "Show the file", and Confirm and Discard. An error uses the `error` border. |

`client/test/tokens.test.mjs` checks each palette: the count of colors, the
contrast of each text color, of `primary` and of each semantic color on each
background, and the contrast of `on-primary` on `primary`. It also checks that
no view has a color literal.

## Rules

- Each color comes from a token.
- Text meets the WCAG 2.1 AA contrast level (Web Content Accessibility Guidelines): a ratio of 4.5 or more. `primary` and `success` also meet 4.5, so they can be text.
- A line or a dot that shows a state (`warning`, `error`, `info`) has a ratio of 3 or more (WCAG 1.4.11).
- Each image has alt text.
- Motion shows a change of state. It respects the reduced-motion setting.
