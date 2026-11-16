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
