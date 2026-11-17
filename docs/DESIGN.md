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
- Touch target: `--size-touch`, 44px. On a phone (600px wide or less) or a touch screen, each button, summary, select and text input is at least 44px high and wide. A link that stands alone (not in a sentence) has the class `touch`, which gives it the same size. A link in a sentence keeps its text size (WCAG 2.5.8). A checkbox keeps its mark size: a 44px label around it, or a transparent border with a negative margin, makes the target.
- Safe areas: `index.html` sets `viewport-fit=cover`, so the header, the page, the footer and the tabs add the `safe-area-inset-*` of each edge to their padding.
- Phone width: from 360px, no view scrolls sideways. `client/test/phone.test.mjs` checks the brief, the review, an item and a mail at 360, 390 and 768px.

## Parts

Add each shared part here when it is built. The parts are in `client/src/views/`.

| Part | File | Use |
|---|---|---|
| Header | `App.svelte` | The Iris mark (a `primary` ring around a dot, like an iris), and one link for each view. The active link has a `primary` line under it. On a phone the header has the mark only, and it scrolls away with the page. |
| Tabs | `App.svelte` | Phone only (card R6-05). A bar at the bottom of the screen with the first four views of `NAV` and "More". Each tab is a 24px line icon over a `font-size--2` label; the active tab is `primary`. "More" is a native popover above the bar with the other views, one 44px row each. It closes when a view opens. The bar keeps clear of the home indicator (`safe-area-inset-bottom`). Its height is `--tabs-height` (0 on a wide screen): an element that sticks to the bottom of the screen uses `bottom: var(--tabs-height)`, as the Ask question box does. |
| Opening | `Brief.svelte` | The greeting and the date in muted text, then one sentence in `font-size-4` (`font-size-2` on a phone) that says what needs you. |
| Line | `Line.svelte` | The round mark with the initials of the other party, the title, and one muted line of detail. The whole line opens the item. The envelope opens the source mail. An `unread` dot marks a conversation that waits for you. |
| Bundle line | `Bundle.svelte` | One `bundle` rule on one line, in the section "Bundled by your rules" of the brief: a stack mark (a round mark with the count, and a second mark behind it), the rule name, and one muted line that names up to 3 senders. A chevron takes the place of the envelope. The whole line opens the list of the bundle. |
| Handled count | `Brief.svelte` | The section "Handled without you" of the brief: the count, one muted sentence with the hidden items and the messages that a rule decided (an approved change can still wait to apply, so the brief does not say "changed"), and the link "See what and why". It does not show when the count is 0. |
| Handled list | `Handled.svelte` | The actions that a rule decided, under "Decided by your rules" (the verb in bold, the message count, the reason in muted text, and the state and time in faint text), then the hidden items as Lines with the reason as a note. Each list loads 50 at a time. |
| Quick Done | `Line.svelte`, `Brief.svelte` | On the brief, each open or deferred commitment line starts with a round tick box in place of the initials (card R2-16, option A of the owner). The ring uses the `text-muted` color, so the control has a contrast of 3 or more. Hover or focus shows a `primary` ring and tick. The party stays in the detail line. A click shows one raised line in place of the line: a `primary` tick, the title struck through, "Done. It leaves your brief." and a `primary` Undo, with the shrinking `primary` line of Commitment actions for 5 seconds. Then Iris records `close` and the brief loads again. If the person leaves the brief or the page in that time, Iris records it at once. A brief refresh that removes the row keeps the hold. A refusal uses the `error` border and the reason of the engine, with Close. After a 409, the brief loads again. With no answer, the line says that Iris does not know, and asks for a reload. |
| Ready mark | `Line.svelte` | What Iris prepared for an item (card R3-09, option A, the owner's pick): a row under the detail with a pill for each prepared thing. A pill has a `primary` border of 1px, `primary` text in `font-size--1`, `radius-full`, and a pen icon before "Draft ready". It is no control: a click on it opens the item, as the line does. It shows only when the engine gives `ready` with `draft`, so the draft answers the newest message of the thread. |
| Skip, Mute, Dismiss | `Line.svelte`, `Act.svelte` | One quiet word button for each feed item (Skip), active or quiet conversation (Mute) and signal (Dismiss) (card R5-02, option B of the owner). On the item page it is a quiet pill under the title. On a brief line, also in the full list of a section or a bundle of the brief, it is a `text-muted` text button at the end of the line, shown on hover or focus, and always on a touch screen, where it is `size-touch` high. A click holds the choice for 5 seconds with the same raised line and Undo as the quick Done: "Skipped. It leaves your brief." ("Muted.", "Dismissed."). Then Iris records the verb and the line leaves. The full list of a section of the brief has the quick Done too. The list of a form has no actions. Ten such choices for one sender and one form, with no undo, give a hide suggestion on the Rules page. |
| History line | `Ledger.svelte` | One change that Iris made, in the History view (card R2-07, option A): the changes are newest first, under a small faint capital heading for each local day ("Today", "Yesterday", the date). A round `background-surface` mark of 32px with a muted line icon of the verb (a box, a bin, a tag, an envelope, a tick), then the verb in the past tense in bold with the count or the item title, the reason of the author in muted text, a failure line with a `warning` dot, and one faint line with who decided it ("You", "An agent, job …, approved by you", "Iris triage, allowed by your rule …") and the time. An outline round "Undo" sits at the right while an undo can start; hover or focus gives it a `primary` border and text. After Undo, a `primary` dot and "Iris undoes the change. A message (or an item) that changed again, or that is gone, stays as it is. This can take a minute." take its place. The view reads the list again every 3 seconds while a change or an undo is open. Each read, Show more and Undo run one at a time. An undone change shows at 55% opacity with "Undone". A refusal or no answer loads the list again and says why, with the `error` border. The list loads 50 at a time. |
| Line note | `Line.svelte` | One more muted line under the detail, for example the reason of the rule that hides the item. |
| Due tone | `Line.svelte` | A dot after the due date: `warning` for 2 days or less, `error` when late. The words say the same, so color is not the only sign. |
| Section | `Brief.svelte` | A heading with the count on the right, a line under it, at most 7 lines, and an "All N" link. An empty section does not show. |
| Commitment actions | `Act.svelte` | On an open or deferred commitment, under the title: Done (the primary button, with a tick), Later (an outline pill) and Decline (quiet text). Later opens outline chips in place: Tomorrow, Next week (the next Monday) and Pick a date, each with its day in muted text. The engine opens a deferred commitment again when its day starts in UTC, so Later offers only a day that starts after now in UTC: late in the evening west of UTC, "Tomorrow" becomes "In 2 days". A click shows one raised line in place of the actions: a `primary` tick, what happens ("Done.", "Declined.", "Later, on Mon 23 Nov."), "It leaves your brief." and a `primary` Undo. A `primary` line under it shrinks for 5 seconds. Then Iris records the change. If the person leaves the view, reloads or closes the tab in that time, Iris records it at once. After the change, the line says "Back to the brief". A refusal uses the `error` border and the reason of the engine. After a 409, the page loads the item again, so it shows no stale actions. With no answer, the line says that Iris does not know, and asks for a reload. |
| Watch notice | `Arrived.svelte` | At the top of the brief, before the login codes: one raised card for each watch whose mail came (card R5-07, option A of the owner). The full Iris mark (a `primary` ring and center; the center grows one time), the watch text in muted text, the subject as the title, and the sender and date in muted text. The whole card opens the mail. An outline pill "Done" removes the watch. A mail that left the mailbox shows as words, with no link. A refusal uses the `error` border. |
| Notice by rule | `Notices.svelte` | Above every view, under the sign-in notice: one card for each notice of a `notify` rule that is not done (card R5-08). A `primary` edge of 4px and a `primary` dot; the rule name in muted text; the sender and subject as the title; the time in muted text. The whole card opens the mail. An outline pill "Done" closes it. Three cards show; "Show N more" opens the rest. |
| Watch list | `Watches.svelte` | Near the end of the brief, under "Iris watches for": each watch that waits, with a `primary` ring with no center, the text, "since" and the date in faint text, and a quiet "Stop" that shows on hover or focus (always on a touch screen). With no watch that waits, there is no heading. A quiet outline pill with the ring, "Watch for a mail", opens one field in place, with a hint on the search words. Enter or "Watch" starts the watch, and Escape closes the field. |
| Job line | `Jobs.svelte` | One job in the Jobs view (card R4-09): a round mark of the state (a `primary` dot when the agent asks you, a `primary` ring while it works, a faint ring while it waits, a faint dot when it ended), the goal, and one muted line: the question, or the state in words, then the due date. The groups are "Asks you", "Working", "Waiting" and "Ended". Each group loads its own state, 50 at a time (20 for "Ended"), so a job that asks you is never behind newer jobs. The whole line opens the job. |
| Matter line | `Matters.svelte` | One matter in the Matters view (card R6-01): a stack mark (two `background-surface` squares with `radius-md`, one on top of the other, with the count of items in muted tabular figures), the title, one muted line of the counts by form ("Two records, one conversation"), and a faint chevron. The groups are "Iris proposes" and "Yours", in small faint capitals. The whole line opens the matter. |
| Matter page | `Matter.svelte` | One matter (option A, the owner's pick): the title in `font-size-3`, one muted line with the count of items and the day that you kept it together, then the sections "You owe", "Owed to you", "Records" and "Conversations" with item lines. A proposal shows its matter card with each member. A declined matter shows one line with a `primary` line on the left above the sections. |
| Matter card | `MatterCard.svelte` | The newest proposal of a matter in the brief (option A, the owner's pick), in the section "A matter to confirm" after "Due soon": a raised card (`background-elevated`, `radius-lg`) with a small Iris mark and "Iris sees a matter", the title in `font-size-1`, the counts by form, up to 5 members with a form icon in a `background-surface` square, then the `primary` pill "Keep together" and the quiet "Not a matter". "Keep together" opens the confirm step in the same card: the title in a text field, a `primary` tick box on each member (a cleared member is struck and faded), and the pill "Keep together" ("Keep 2 together" when the person clears some) with a quiet "Cancel". Each choice holds for Undo with the same time and shrinking `primary` line as the quick Done, because the engine cannot undo a decision. |
| Handle this | `Item.svelte`, `Act.svelte` | On an open commitment, or an active or quiet conversation: an outline pill link with a `primary` border and `primary` text, "Handle this". On a commitment it sits at the end of the row of Commitment actions; on a conversation it is alone under the title. It opens the start page of the item (`#/item/<id>/job`). |
| Start page | `StartPage.svelte` | The start of a job on its own page (card R4-10, option B of the owner): "New job" in faint text, the heading "Give this to an agent", then the item title (a link back to the item) and the quote of its mail, with a `primary` line on the left, then the start form. Cancel goes back to the item. |
| Start form | `Start.svelte` | A raised card (`background-elevated`, `radius-lg`): the goal field (empty, with a hint; no text of the mail fills it), the due date field with a quiet "No due date", then "The agent reads N messages in M threads, and no other mail." and each message of the grant, grouped by thread under the subject of its first message, with the sender, the time, and its own subject in muted text when that subject is another one. A message that an agent cannot read yet says "Hidden from the agent for now". The due field follows the item until the person changes it. The `primary` pill "Start the job" acts only with a goal. A refusal of the engine uses the `error` border. After a 409, the item and the list load again, so the page shows them as they are now. A job of the item that is open already opens at once. |
| Now card | `Job.svelte` | Under the goal and its limits on the page of one job (option A, the owner's pick): one raised card with a 4px line on the left that says what the job does now. The line is `primary` while the agent works or asks you, `success` when the job ended, and `border` while it waits. A question of the agent shows in `font-size-1`; a wait links the newest mail of its thread, and names no sender, because the job does not say who must reply. |
| Job steps | `Job.svelte` | Each step of a job in order, oldest first: the time in faint tabular figures, a quiet rail with one mark for each step (a faint dot for you, a faint ring for the agent, a `primary` ring for Iris, `primary` for a question and for the newest step, `success` for the end), the day above the first step of each day, one sentence, and the text of the agent or the person as a quote with a line on the left. A changeset that waits for you has a `primary` Review link. |
| Job grants | `Grants.svelte`, `Job.svelte` | On the page of one job, between the now card and the steps (card R4-10, option A of the owner): the heading "Without asking you", one muted sentence (and the observe mode, when Iris is in it), then three switches: "Tidy the mail", "Update commitments" and "Move mail to the trash", each with its verbs in muted words. A switch is a native check box with `role="switch"`: a `border` track, a `primary` track when on, a `background` knob. On adds one allow rule of this job to `policy.toml`; off removes it. A switch is on only for its exact rule (its verbs, `author = "agent"`, no other condition). Each other grant of the job (other verbs, a condition, a second copy) shows as its own line with its conditions and a quiet Remove. The allow rules of the whole policy that can reach an agent show under "Also allowed by your whole policy", with a link to the rules page. When the job ended, the switches that are off cannot go on. A refusal uses the `error` border, and the switches show the policy as Iris reads it again. If the policy cannot be read, the section shows the reason and "Try again". |
| Code | `Code.svelte` | A login code in mono type, a Copy button, and the time left. It goes away when the code expires. |
| Quote | `Item.svelte` | The words of the mail that made the item, with a `primary` line on the left and the link to the mail. |
| Chips | `Brief.svelte`, `Records.svelte` | Round links or buttons with a count. The chosen chip and a chip under the pointer have a `primary` border. |
| Primary button | `Code.svelte`, `Setup.svelte` | A `primary` background with the `on-primary` text color. One for each screen at most. |
| Due soon | `Brief.svelte` | The first section of the brief, after the live login codes: each commitment that you owe in 2 days or less, or that is late, in the local day of the viewer. It has the `warning` border on the left. You owe counts and lists the rest, and its "All" link opens the full list. |
| Warning | `Brief.svelte`, `Mail.svelte` | A border of 4px on the left: `warning` for a notice, `error` for hostile mail or an error. |
| Access notice | `App.svelte` | Above every view: why the API cannot answer now (no sign-in, a used sign-in address, or no engine). It goes away at the next good answer. It uses the `warning` border. |
| Setup link | `App.svelte` | Above each view other than setup and the brief, when Iris has no account: a link to setup. The Brief link opens setup in that case. It uses the `warning` border. |
| Sign-in notice | `SignInNotice.svelte` | Above every view: the address of each account that needs sign-in, from the last status answer, and the command that signs in again. It uses the `warning` border. |
| Policy notice | `PolicyNotice.svelte` | Above the brief: the policy file changed and waits for a confirmation (`warning` border), or it has an error (`error` border). A link opens the rules. |
| Mode choice | `Rules.svelte` | Two cards side by side (one column on a phone), each a radio button. The chosen card has a primary border and a ring with a primary center, the mark of Iris. A change asks for one more click on the primary button. |
| Rule line | `Rules.svelte` | The name, the verbs as words, each condition as a chip, and the rule number and line in small muted text. The Remove button stays quiet until the pointer or the focus is on the line. A remove asks one question in place, with no dialog. |
| Suggested rule | `Rules.svelte` | A rule that Iris proposes from your approvals, or a hide rule from your skips, mutes and dismisses (R5-02), in the section "Suggested by Iris" above the mode. It reads like a Rule line, with one muted line that says why ("You approved all 14 archive changes like this.", or "You hid all 12 items like this." for a hide rule). An outline button with a `primary` border adds the rule, and a quiet Dismiss button removes the suggestion. On a phone the buttons go under the words. |
| Suggestion notice | `PolicyNotice.svelte` | Above the brief, when no notice of the file shows and a suggestion is open: one line with a `primary` border, "Iris suggests N rules from your approvals.", and a link to the rules. The source follows the kinds of the open suggestions: "your approvals", "what you hide" (only hide suggestions, R5-02) or "what you approve and hide" (both). |
| Request | `Request.svelte` | One changeset in the review: the mark of the author (a `primary` ring, or the Iris mark for Iris triage), the reason of the author as a quote with a `primary` line, then each change as a sentence (the verb in bold, then the real count), its reason and its flags (a `warning` dot and words). A `draft` or `send` change shows its mail in a box (To, Cc, subject, body). Each part of the mail that gave a flag has a 2px `warning` line under it and a tag beside it (`warning` border, muted text: "amount", "date", "new address", "no file attached", "copied from another mail", which links to the source mail). A flag that the mail does not mark keeps its dot and words (card R4-07, option C). The changes that have a decision are behind one quiet line, "N more changes have a decision". When nothing waits, the line opens. |
| Decide | `Request.svelte` | Design B (the owner's choice): a round tick box before each waiting change, ticked at the start (`primary` fill, an `on-primary` tick). An unticked change goes to half opacity and says "Iris will deny this change.". One `primary` pill button says what happens ("Approve all 3", "Approve 2, deny 1", "Deny all 3"). With no tick, the pill has no fill. The button acts only when each row of each waiting change loaded; until then a quiet line says that Iris loads them. After the decision, one quiet line says what the person approved and denied. |
| Withdraw | `Request.svelte` | A quiet "Withdraw the request" in faint text at the end of the decision row of an open request (option B, the owner's pick). If no change waits, the row says "No change of this request waits for your decision." and keeps the control. A click opens one question under the row, with a `warning` line on the left: "Withdraw this request?", what Iris cancels, what stays, and "You cannot undo a withdraw." The question gives no count, because the page cannot see which approved change Iris started. An outline pill withdraws, and a quiet "Keep it" closes the question. After the withdraw, one quiet line counts the cancelled changes, and the changes that Iris started, from the states that the store gives back. Each cancelled change stays in view, also one of an older revision, and reads "Withdrawn". |
| Stranger gate | `Gate.svelte` | Option A (the owner's pick, card R5-03): one row for each sender that you do not know. The row has the initials in a ring with a dashed `border` line (a sender that you do not know yet), the name, the address in faint text, and the titles of the newest 3 items as quiet links. A quiet "Ignore" and an outline `primary` pill "Accept" sit at the end of the row (under the text on a phone). After an answer, the row keeps its place and shows one quiet line with "Undo". Under the list: "All N" when the brief shows only the first 20 senders, "Ignored N" to the list of ignored senders, and "Let a rule answer" to the rules. On the list of ignored senders, "Ask again" takes the place of "Ignore". |
| Message box | `Request.svelte` | The real messages of one change: sender, subject and date in one raised box with quiet lines. On a phone the subject goes under the sender. A gone message shows as words. Each row of the change shows: after about six rows, the box scrolls. |
| Measures | `Measures.svelte` | Design A (the owner's pick, card R5-12): one sentence in `font-size-2` with the opens of today in `primary`, then the main measure between two lines, with a bar for each of the last 14 days (`border` bars, today in `primary`, a dotted base line for a day with no use). Then one line for each other measure: a dot, the name, the value on the right, and one muted line with the target. The dot: `success` meets the target, `warning` misses it, `text-faint` has no fixed target, a `text-faint` ring has no data yet. The dot has a label in words. |
| Change panel | `Rules.svelte` | A raised panel with a primary border on the left: each change of the file, with a dot of color and a word (New `success`, Changed `warning`, Removed `error`), the file text behind "Show the file", and Confirm and Discard. An error uses the `error` border. |
| Ask thread | `Ask.svelte` | Option B (the owner's pick): the questions and answers stack as a conversation. A question is a `background-surface` bubble on the right. An answer starts with the Iris mark on the left, which pulses while Iris reads the mail. The older questions and answers use `text-muted`. The thread stays while the page is open. |
| Source mark | `Ask.svelte` | A small round mark with the number of a source, in `primary`, close after its fact. It opens the source mail. Under the answer, one round chip for each source: the number mark, the sender and the subject. A muted line counts the mails that Iris read, and gives the cost. |
| Question box | `Ask.svelte` | A round field at the bottom of the Ask view, sticky, with a round `primary` button with an arrow. The button stays faint while the box is empty or a question waits. After an error, the question comes back to the box. |
| Outbox | `Outbox.svelte` | Option A (the owner's pick, card R3-02): one raised card for each send that waits to leave. The card has the recipients in muted text, the subject, and one line for each warning with a `warning` dot and the words of the flag. At the end of the card: "Leaves in 0:42" and a thin line in `primary` that fills as the one-minute hold ends, with an outline round "Cancel" button at the right. While sending is off, a banner with a `warning` line on the left says "Sending is off. Sends wait here." and names `iris sending on`; each card says "Waits until sending is on" in faint text and has no line. The cards show only the sends that the engine gives, so a new send of a cancelled draft shows again. After each cancel, one quiet line above the cards says what happened ("… is cancelled. It does not leave." or "… started to send. It cannot be cancelled."), and the line stays after the card goes. |
| Thread with reply | `Write.svelte` | The write view, option C "In the thread" (card R3-04). A reply shows the subject of the thread as the title, then the newest 20 mails of the conversation, oldest first, one card each (`border-subtle`, `radius-lg`): the initials, the sender ("You" for your mail), the date and the text. The older mails are muted small text; the mail that the reply answers reads in full size. A hostile mail has an `error` line on the left and a warning. A cut text links to the whole mail. A line says how many older mails are not shown. The reply form is the last card: `background-elevated` with a 2px `primary` line at the top. Its first line names the recipients ("Reply to Priya Raman") with a quiet "Change" that opens the chips. Under it: the text with no border, the round `primary` "Send" button, a faint hint ("It waits 60 seconds in the Outbox, so you can still cancel it.") and a quiet "Discard". A new mail uses the same form under the title "New mail", with the "To" line, a quiet "Add copy" and the subject in `font-size-1`. |
| Intent line | `Write.svelte` | In a reply only (card R3-04, intent path): one line between the recipients and the text, with a `border-subtle` line under it. A text field with no border ("Tell Iris what to say") and an outline pill, "Write it for me" ("Writing" while the engine works). The draft of the engine goes in the text field for the person to edit. While the call runs, the form is disabled. A refusal shows under the line with a `warning` line on the left. |
| Address chip | `Write.svelte` | One `background-surface` round chip for each address, with a faint "×" to remove it. A comma, a space, Enter or a move out of the field makes a chip. Backspace in an empty field removes the last chip. A pasted list with names (`Priya Shah <priya@example.com>`) gives one chip for each address, once. A chip whose form is not an address has a 1px `error` ring. The engine decides the address check. |
| Warnings before Send | `Write.svelte` | After a 409 with warnings, one line for each warning above the Send button, with a `warning` line on the left and the words of the flag. The next press sends these warnings. |
| Send state | `Write.svelte` | After the press, the mail shows as the last card of the thread ("You", the recipients and the text), and its state is a card with a dashed `border` under it: a title ("Saving your mail", "On its way to …", "Look at this mail before it leaves" with a `warning` line, "Sent to …", "This mail did not leave" in muted text), a muted line, and for a send that waits, "Leaves in 0:49" with the thin `primary` line of the outbox. An outline pill stops the mail ("Stop this mail" while saving, "Cancel the send" while it waits). A quiet "Write another" starts a new mail. |

`client/test/tokens.test.mjs` checks each palette: the count of colors, the
contrast of each text color, of `primary` and of each semantic color on each
background, and the contrast of `on-primary` on `primary`. It also checks that
no view has a color literal.

## Project site

The project site is the static HTML in `docs/` (`index.html`, `privacy.html`
and the builder guide in `docs/guide/`). GitHub Pages serves it from
`develop` `/docs`. `docs/site.css` holds its styles. The site has no script.

The site uses only the dark palette. Each site variable is a token of this
file:

| Site variable | Token | Use |
|---|---|---|
| `--bg` | `background` | Page background |
| `--text` | `text` | Main text, the current page in the menu |
| `--muted` | `text-muted` | The date of the privacy policy |
| `--link` | `primary` | Links, the line of a caution |
| `--surface` | `background-surface` | Code blocks, field names |
| `--line` | `border` | Lines between table rows |

Site parts:

- The menu (`nav`) at the top of each guide page. The current page has
  `aria-current="page"` and the `text` color.
- A code block (`pre`), with the mono font, radius `md` and a side scroll.
- A table, with a side scroll on a narrow screen.
- A caution (`.caution`): a 4px line of `primary` on the left.
- A field list (`ul.fields`): the required fields of a document, as chips
  with radius `sm`.

## Rules

- Each color comes from a token.
- Text meets the WCAG 2.1 AA contrast level (Web Content Accessibility Guidelines): a ratio of 4.5 or more. `primary` and `success` also meet 4.5, so they can be text.
- A line or a dot that shows a state (`warning`, `error`, `info`) has a ratio of 3 or more (WCAG 1.4.11).
- Each image has alt text.
- Motion shows a change of state. It respects the reduced-motion setting.
