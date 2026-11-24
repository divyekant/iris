# Decisions

One entry for each decision that changes the product or how it is built: a
direction, a tool, a data model, a rule. Put the newest entry at the top. Do not
edit an old entry. If a decision changes, add a new entry that names the old one.

## Template

```markdown
## D<n> (<YYYY-MM-DD>): <the decision in a few words>

- Decided by: <name>
- Why: <the reason, in one or two sentences>
- Options not chosen: <each option, and why not>
- Links: <card, spec, PR, before/after page>
```

## D12 (2026-11-24): The answers to 38 open product questions

- Decided by: dk (accepted each recommended option)
- Why: 43 open limit issues waited on a product choice. The list gave each
  question its options, their effect and a recommendation. The table gives
  the chosen option of each question. The item IDs (D1 to D38) are the IDs
  of the list and of the issue comments, not entries of this log.
- Options not chosen: the last column of the table.
- Links: https://claude.ai/artifact/9yamkrHxBSMLRruRGsWg5q, and a comment
  on each issue.

| Item | Issues | Question | Chosen | Not chosen |
|---|---|---|---|---|
| D1 | #121, #579, #241 | Use your time zone for defer dates and due dates? | Add a time zone setting. The default comes from the computer. A deferred item opens at your midnight, and a promise is late at the end of your day. Fixes all three issues. | B. Keep UTC |
| D2 | #239 | Can a policy rule allow draft or send? | Rules can allow draft only. An agent can put a draft in the held outbox with no review. You still approve each send. | B. Rules can allow draft and send; C. No rules for draft or send |
| D3 | #386 | Can a job grant allow replies to the people in the thread? | Yes, for drafts. A job grant can allow drafts to addresses already in the thread. Each send still needs you. Needs D2 option A or B. | B. No |
| D4 | #235 | Can sent mail have attachments or HTML? | Attachments yes, HTML no. You can attach files. The body stays plain text. | B. Both; C. Neither for now |
| D5 | #236 | Add Bcc, your display name and send-as aliases? | Display name and Bcc. Recipients see your name. Aliases come later, when someone asks. | B. All three; C. None for now |
| D6 | #238 | Can you change a draft and undo the change? | Yes, in the write view. Build the draft change and its undo together, as spec 2 section 7.5 says. | B. No |
| D7 | #278 | Keep one reply draft per thread? | One draft, one automatic retry. Keep one draft per thread. Iris tries again once when the model answer is not valid. | B. Several versions per thread |
| D8 | #578, #580 | How far do nudges go? | Prepared reply, all late promises. The nudge stays a prepared reply. One draft names each late promise of the thread. Close #578. | B. Also a nudge command; C. Keep as is |
| D9 | #168 | Can an action that a rule approved apply with no review by you? | Yes, within the limits. Before the apply, Iris checks the active policy again and the hourly bulk count from the ledger. | B. No |
| D10 | #193 | In observe mode, show what the policy would do? | Yes. Review shows "a rule would approve this" before you turn on apply mode. Needs a store migration. | B. No |
| D11 | #297, #442 | Can rule proposals name a domain? | Add a domain condition. A proposal can name a domain, with the same count rule as for one address. | B. Keep one address |
| D12 | #433 | Learn from the items that you open? | Yes. Iris records the item of each open, on your computer only. An open stops a hide proposal for that sender. | B. No |
| D13 | #435 | Does an address in Cc from a stranger count as part of the thread? | No, use a stricter rule. An address is in the thread only if you sent to it, or if it wrote from its own address. | B. Yes, keep the header rule |
| D14 | #437 | Limit trash and label for an agent that read outside mail? | Yes, send them to review. Trash and label by a marked agent always go to review. | B. No |
| D15 | #538 | Can agents with no job read your review comments? | Yes. Show comments in iris show cs-ID for MCP and local agents. Minor schema version. | B. No |
| D16 | #221 | Record who withdrew a changeset? | Yes. Store the author or you, and show it in Review. Add it with the next changeset migration. | B. No |
| D17 | #225 | Undo after you change the same message again in Gmail? | Accept. This is rare, you see it, and you can archive again. Close the issue. | B. Detect it |
| D18 | #463 | What if an undo changes no message? | Keep the entry open. Show the skipped messages. You can undo the entry again later. | B. Mark it undone |
| D19 | #284, #286 | Can you edit matters and make one by hand? | Both. Rename, add and remove items, delete, and make a matter by hand. | B. Edit only; C. Not now |
| D20 | #285 | Can a new item join a confirmed matter? | Yes, as a proposal. Iris proposes the addition and you confirm. | B. No |
| D21 | #287 | Can feeds and signals be members of a matter? | Signals yes, feeds no. For example the delivery signal of an order joins the order. | B. Both; C. Neither |
| D22 | #288 | What can agents do with matters? | Propose. An agent can propose a matter. Only you confirm. | B. Read only |
| D23 | #289 | Members of a declined matter stay pinned? | Accept. Close the issue. Change it only if duplicates show in the brief. | B. Find a new identity |
| D24 | #248 | What happens on the due date of a job? | Needs you. The job moves to needs you, with a notice. | B. Mark late only; C. End the job |
| D25 | #384 | Start a job from a typed goal, and edit a job after the start? | Both. Start a job with no mail, and edit the goal, the due date and the scope (add a thread from a search). | B. Edits only; C. Neither for now |
| D26 | #558 | Can a job that starts from an item wait for a reply? | Yes. The start adds the thread of the item as a thread ref. | B. No |
| D27 | #574 | Add a way to clear the notes of an open job? | Accept. You edit notes.md. Close the issue. | B. Add a clear action |
| D28 | #523 | Add iris list with filters in host mode? | Yes. Add a card. Agents on the host need lists of items. | B. No |
| D29 | #263 | Skip the model for mail from a declined sender? | Yes. No model call. Understanding runs again when you reopen the sender. | B. No |
| D30 | #264 | Where does a group thread with a known person and a declined sender go? | Conversations. It stays in Conversations when a known person is in it. | B. Declined senders |
| D31 | #111 | Check mail again after a sender kind or a known name changes? | Fixed rules only. Run the code-signal rule and the spoof mark again. No model call. | B. Full understanding; C. Keep |
| D32 | #371 | Mark an address in the display name that is not the From address? | Yes. Add a spoof mark. Known list forms such as "x via List" are exceptions. | B. Wait |
| D33 | #257 | Check a watch notice against marks that come later? | Yes. Check the hostile marks again before each notice goes out. | B. Accept |
| D34 | #626 | Can you remove an account? | Yes. A command and a button remove the account and its mail from Iris, not from the provider. Spec first. | B. Not now |
| D35 | #136 | The wait after a model error can reach 1 hour. Change it? | Add Try now. A button in the web client tries the model at once. The backoff stays. | B. Cap at 10 minutes; C. Keep |
| D36 | #716 | What does Accounts say while a sync tries again after a failure? | New text. "The last try failed. Iris tries again now." | B. Keep |
| D37 | #451 | Support phone browsers with no Popover API? | Accept. Modern browsers only. Close the issue. | B. Add a fallback |
| D38 | #452 | How do notices show on a phone? | One line. One line with a count. Tap it to open the notices. | B. Brief only; C. Keep |

## D11 (2026-11-24): The engine removes the grant rules of a done job

- Decided by: dk
- Why: A grant that the person gives inside one job ends with that job. A
  rule of a done job matches nothing, but it stays in the Rules view and in
  the file. The engine removes it as a normal policy change with a new
  version, so the person can see each removal. This is the one exception to
  "Only the person changes the policy" (spec 2, section 6.3).
- Options not chosen: Keep the rule, and show a notice that asks the person
  to remove it.
- Links: issue #385, PR #803

## D10 (2026-11-18): Each user signs in to Outlook with an own Entra app registration

- Decided by: Claude, as the technical owner (Outlook spec, PR #637)
- Why: As for Gmail (D2), a shared Iris app makes the project responsible
  for each install. A public client with PKCE needs no secret, and the
  account type "any tenant and personal accounts" works with the tenant
  `common` for Outlook.com and for work or school accounts.
- Options not chosen: A shared, verified Iris app (publisher verification
  needs a Microsoft Partner Network account). A client secret (not secret
  in a self-hosted program). The device code flow (more steps).
- Links: card R6-13, `docs/decisions/2026-11-microsoft-sign-in.md`

## D9 (2026-11-17): Open model 1.1 adds the account of each item

- Decided by: dk
- Why: With more than one account, each item must name its mailbox. IMAP and
  Outlook (R6-03, R6-04) make the account a basic fact of each item. Agents
  through MCP and the export see it too.
- Options not chosen: Keep model 1.0 and put an item-to-account map next to
  the items in the brief answer only.
- Links: card R6-02, `docs/specs/2026-11-17-accounts.md`, PR #627

## D8 (2026-11-17): Agent sessions on macOS use Seatbelt and an exact end check

- Decided by: dk (decision card of the R4-05m thread)
- Why: A probe on the owner's Mac showed that a `deny default` Seatbelt
  profile blocks each file, network, socket and service path, and that the
  counters of a launchd coalition tell exactly when each process of a
  session is gone. macOS gives no group kill to a normal user, so the
  Linux lifetime rule cannot hold. In its place, the engine reaps a macOS
  session only when the coalition is empty. A process that escapes a stop
  can use CPU, but it sees no later data and reaches no other session.
  This keeps the lightest memory use.
- Options not chosen: A Linux virtual machine on the Mac (a stop with no
  race, but hundreds of MB for each session and Linux agent tools). Stay
  Linux only (the earlier decision of R4-05, which this entry replaces for
  macOS).
- Links: card R4-05m, `docs/specs/2026-11-17-session-sandbox-macos.md`,
  issues #599, #600, #601

## D7 (2026-11-17): Review comment files are a part of open model 1.0

- Decided by: dk
- Why: Spec 2, section 5.5 of version 1.0 already says that a comment
  arrives as a file in the workspace. The folder `comments/` is there only
  when it holds a file, so each workspace of a job with no comment stays the
  same, and no document changes its `model` value.
- Options not chosen: Version 1.1 (each document moves to `iris/1.1`, about
  140 files). Comments in a private Iris path (another tool cannot build the
  same workspace).
- Links: card R4-08, `docs/specs/2026-11-17-review-comments.md`,
  `docs/spec/03-job-workspace.md` section 7a, PR #539

## D6 (2026-11-16): Ask answers from a full-text search and one model call

- Decided by: the build thread of card R5-09 (Claude, owner of the code)
- Why: One call keeps the cost of a question low and its worst case known
  before the call. The FTS5 index exists, and `bm25` ranks the matches. The
  call uses the one budget of understanding, with its own `understand_run`
  row (`prompt` = `ask/0.2`), so the person approves one budget for all model
  use. The call holds the run lock, so the model row and its budget do not
  change during the call. The model gives claims in JSON, each with the
  numbers of its messages, so Iris reads no sentence boundaries in free
  text. Each claim must cite a message of the data, and a quote stays only
  if its message has it, so the person can check each answer.
  Hostile mail, a hostile sender, SPAM, TRASH and gone mail are not data.
- Options not chosen: A first call that writes the search (two calls for each
  question, and a second cost). Vector search (an embedding model and an
  index, more memory). A budget for questions only (a second approval).
- Links: card R5-09, `crates/iris-understand/src/ask.rs`

## D5 (2026-11-16): The state of the ledger entry moves the action

- Decided by: Claude (technical owner), card R2-03
- Why: The trigger `ledger_state` writes the state of a ledger entry to its
  action, and the trigger `action_state` refuses `applied`, `failed` and
  `undone` without an entry in the same state. So the action and its entry
  always agree, for each writer. The entry keeps only the action ID: the
  changeset, the verb, the author and the decision come from the action row,
  which does not change after the submit. A gone message does not fail an
  entry; only a message that the recovery cannot confirm (`unknown`) does.
- Options not chosen: The engine writes both states (two writes that can
  disagree after a defect). A copy of the action columns in each entry (more
  bytes, and two copies of one fact).
- Links: card R2-03, migration 11 (`schema_11.sql`), specification 2,
  section 7

## D4 (2026-11-16): The client uses the "Iris" palette, not the gold

- Decided by: dk, on the before/after page of card R1-27
- Why: The violet gives Iris its own color and an iris for the mark. It is
  not a `success`, `warning` or `error` color, so the accent does not look
  like a good or a bad state. It also marks `info` and `unread`. Cool ink neutrals keep the brief quiet. The semantic colors are now
  set for each palette: the old light `success` was text with a contrast of
  2.8 to 3.1.
- Options not chosen: "Sage" (sage green, near the success color), "Ink" (no
  hue), and the old gold.
- Links: card R1-27, https://claude.ai/artifact/3kpABqULbCJqgjzTkHSD7k

## D3 (2026-11-16): A library reads MIME and mail addresses

- Decided by: dk
- Why: 150 of 427 review findings on PRs 11 to 35 were in the hand-written
  parsers of hostile input. `mail-parser` 1.0.1 (Apache-2.0 or MIT, used by
  Stalwart Mail Server, fuzzed) reads MIME, headers, encoded words and
  address lists, and decodes bodies only on access. The trial removed about
  680 lines and kept the memory bounds of `iris-clean` (largest peak 9.56 MB,
  before 9.44 MB). Iris keeps its own HTML and PDF code.
- Cost: two new crates (`encodify`, `hashify`), Rust 1.98 or newer, about
  70 lines of Iris code for the limits that the library does not have, and
  three address forms outside `<>` that give no address (spec
  `docs/specs/2026-11-16-clean-text.md`, "Known limits").
- Options not chosen: keep the hand-written parsers (each new edge case is one
  more review round). Use the library for MIME only (the address parsers had
  the same class of findings).
- Links: card R1-29

## D2 (2026-11-15): Each user signs in to Gmail with an own Google OAuth client

- Decided by: dk
- Why: Google needs no app verification for personal use with fewer than
  100 users. The path costs nothing, keeps the read-only scope
  `gmail.readonly`, and keeps spec 1. The client must be "In production",
  because "Testing" tokens expire after 7 days. The domain test of the record
  is a condition.
- Options not chosen: A verified Iris app (weeks of review, a yearly security
  assessment, a public client secret). IMAP with an app password (full mail
  access, no read-only option, new sync design). IMAP with OAuth, the macOS
  Mail app, Google Takeout and forwarding.
- Links: card R1-01, `docs/decisions/2026-11-google-sign-in.md`, PR #13

## D1 (2026-11-15): Iris keeps its own tracker as the board

- Decided by: dk
- Why: The Iris tracker holds the 94 work items of the seven releases, with
  their release, part and done condition. A move to the kit board adds risk and
  gains little now.
- Options not chosen: Publish the kit board and copy the 94 items to it.
- Links: https://claude.ai/artifact/VWqsmpqHNgiop14TpoecjV
