# Spec: rules from your preferences

- Date: 2026-11-17
- Card: R5-02
- Status: Draft

## Problem

R5-01 proposes an `allow` rule after the person approves the same change
10 times. The person also tells Iris what they do not want to see, but
Iris has no verb for it. The open model reserves three statuses for this
choice (spec 1, section 6.8): a feed item `skipped`, a conversation
`muted` and a signal `dismissed`. No verb sets them. So the person cannot
take an item out of the brief, and Iris cannot learn from that choice.

The card also names opens. The store of the measures page (R5-12) is not
merged, and it counts opens for each day, not for each item. An open
cannot tell which sender the person wants to see.

## Goal

- The person can skip a feed item, mute a conversation and dismiss a
  signal. Each one is a state verb of spec 2, section 4, with a ledger
  entry, as `close` is.
- After 10 such choices for the items of one form from one sender, with
  no refusal, Iris proposes a `hide` rule for that sender and that form.
- A proposal goes to the person on the Rules screen of R5-01. Only an
  accept writes the rule to `policy.toml`, with `by = "proposal"`.
- The card is done when no preference is stored outside the policy file:
  a choice changes one item, and only a rule in the file changes what
  Iris does with later mail.

## Not in scope

- A proposal with a `sender_kind` condition. A `hide` rule for each
  service hides too much mail. The proposal names one address, or one
  domain (decision D11, 2026-11-24).
- A `hide` rule for commitments or records. An obligation or a receipt
  that a learned rule hides is a loss that the person cannot see.
- A verb of the `iris` command for skip, mute and dismiss. A changeset
  of any author can hold the verbs, as for `close`. Only the actions of
  the person count for a proposal.

## Design

### 1. The verbs

| Verb | Form | From | To |
|---|---|---|---|
| `skip` | Feed | `new`, `read` | `skipped` |
| `mute` | Conversation | `active`, `quiet` | `muted` |
| `dismiss` | Signal | `active` | `dismissed` |

- Each verb is a state verb (spec 2, section 4): one item target, the
  argument `reason`, and no provider call. As for `close`, a changeset
  action must have `reason` (500 characters or fewer, not blank), and submit denies an
  action without it. The request body of the API can leave it out: the
  API then writes a default reason ("Skipped.", "Muted." or "Dismissed.")
  in the action.
- The web client calls `POST /api/items/<id>/<verb>`, as for `close`. The
  engine makes a changeset of the author `person` with one action, and
  applies it in the same request through `iris_apply::apply_state`.
- An item of another form: the submit denies the action, nothing stays,
  and the API answers 400 with the reason, as for `close` on a feed item.
- An item of another status: the ledger entry fails, and the API answers
  409 with the reason.
- The ledger entry has `pre` and `post` of the field `status`. Its undo
  is `restore`, as for `close` (issue #226, migration 26): migration 29
  lets the undo link and the view `ledger_can_undo` take the three verbs.
  An undone choice is `undone`, so the proposal pass counts it as a
  refusal.
- The hold: the web client sends a verb only after the Undo time of
  `close` (`HOLD` in `client/src/act.ts`). In that time, the person can
  click Undo, and the client sends nothing. The client PR (R5-02d) shows
  the three verbs only with this hold. The local API does not hold, as
  for `close`.
- The engine keeps a `muted` conversation muted when a new message
  arrives (`iris-talk`, already built). A `skipped` feed item and a
  `dismissed` signal keep their status. Only `gone` replaces each one.
- Store migration 29: the three verbs join the verb CHECK of
  `changeset_action`, with the method of migration 18.
- The open model schemas of the changeset and of the policy
  (`docs/spec/schema/`) add the three verbs. They are verbs of the policy
  (`iris_policy::Verb`): a rule can name them, and the hard limits of
  spec 2, section 6.5 apply to them, as to `close`.
- Each of the three verbs takes exactly one item. Submit denies an action
  with two or more targets.
- The choice holds after a new run of understanding. The ledger pins the
  item (spec 1, section 7.5). After a run writes its items, a new item takes
  the status of a `skipped` feed item or a `dismissed` signal only on a
  one-to-one match: exactly one pinned item and exactly one new item of the
  message with the same form and the same content (the document without
  `id`, `status`, `created`, `updated`, `derived`, `model` and
  `attention`). Each other new item stays visible, also one that differs
  only in a code or a title, and also when two new items match: the worst
  result is a
  duplicate in front of the person, never a hidden item that the person
  did not choose. A conversation keeps
  `muted` at each new message (spec 1, section 6.8).

### 2. The proposals

The pass of R5-01 (`iris_policy::propose::pass`) reads a second list,
in the same read transaction:

- The newest 2,000 actions of the author `person` with the verb `skip`,
  `mute` or `dismiss`, in the state `applied` (a choice) or `undone` (a
  refusal).
- For each action: its item, the form of the item, and the facts of the
  origin message of the item (`attention::facts`, the facts that the
  attention of the item reads).
- The candidates are two `hide` rules:
  `when = { sender = "<address>", form = "<form>" }` and
  `when = { sender = "@<domain>", form = "<form>" }` (decision D11). The
  address is the `sender` of the origin message, and the domain is the
  domain of that address. An action whose origin message has no
  address, a hostile mark or a `hostile` sender gives no candidate. Its
  choice and its refusal do not count.
- An origin message from the person (sender kind `you`: the label `SENT`
  or `DRAFT`, spec 1 section 6.7) gives no candidate, and its choice and
  its refusal do not count. A conversation that the person started has
  the person's own message as its origin, so a rule from it would hide
  each conversation that the person starts, not the mail of one
  correspondent.
- The candidate rule is added to the active policy, with the next rule
  ID. A choice counts only when the rule changes the attention of the
  item from `shown` to `hidden` (`Policy::attention`). A choice on an
  item that another rule hides or bundles does not count.
- The engine proposes the rule when 10 or more choices count and no
  refusal counts. A refusal is an `undone` action whose item the rule
  would hide, or an open of a source mail with an item that the rule
  would hide (decision D12, spec 2 section 6.6). The opens are the rows
  of `use_open` (store migration 27) of each day from the day of the
  2,000th newest open. An open of mail from the person, of hostile mail
  or of mail whose header is gone refuses nothing.
- A domain candidate also needs counted choices on two or more addresses.
  A domain proposal covers the address proposals of its form. If the
  person dismisses the domain, the address proposals come back.
- An action is unreadable when its item, the origin message of the item,
  the `header` row of that message or a sender address in it is missing
  (a cleanup of a gone message deletes the header, spec 1 section 5.5,
  and keeps the message row). An unreadable choice is not counted. An
  unreadable refusal stops each `hide` proposal of its form, because the
  engine cannot tell which sender it refused.

This is the invariant of R5-01 for attention: the count is exactly the
number of choices that the rule, in the active policy, would have made.
Both read `Policy::attention` with the same facts, so the proposal and
the rule cannot disagree.

### 3. The table

Migration 30 (the proposal PR) makes `rule_proposal` hold a `hide` proposal:

- The column `verb` accepts `hide`. For a `hide` row, the verb is the
  effect of the rule.
- A new column `form`, NOT NULL: `feed`, `conversation` or `signal` for
  a `hide` row, and the empty text `''` for an `allow` row. The CHECK
  enforces both. The column is never NULL, because SQLite treats two NULL
  values as different in a unique key.
- The unique key becomes `(verb, cond, value, form)`. SQLite cannot
  change a key in place, so the migration makes a new table, copies each
  row with its ID, drops the old table, renames the new one, and makes
  the two triggers again.
- No ID is used again: before the drop, the migration reads the `seq` of
  the old table in `sqlite_sequence` (the highest ID ever given, also when
  that row was deleted). After the rename, it writes the larger of that
  value and the highest copied ID as the `seq` of the new table.
- The column `approvals` keeps its name. For a `hide` row it is the
  number of choices.

`Proposal` gets the effect and the form. `Proposal::rule` gives a `hide`
rule with no `verbs`. Its name is "Hide feed items from X", "Hide
conversations with X" or "Hide signals from X".

### 4. The accept

The accept (`Change::Accept`) does not change. It counts again with
`propose::current` in its transaction, and adds the rule at the end of
the file. `propose::accepted` also marks a `hide` row accepted when a
newly active policy has the same rule: `effect = "hide"`,
`by = "proposal"`, no `verbs`, no job, and exactly the two conditions
`sender` and `form`.

### 5. The API and the screen

`GET /api/policy` already lists the open proposals with the rule and the
count. Each proposal also gets `kind`: `approvals` or `choices`. The
Rules screen shows a `choices` proposal with the sentence "You hid all N
items like this." The before/after page shows the verbs on the brief and
the item view, and the new sentence, for dk to pick a layout.

## Options not chosen

- One verb `hide` for each form: the model has a status for each form,
  and three names keep one word with one meaning.
- A preference table (for example, a list of muted senders): the card
  says that no preference is stored outside the policy file.
- Mute without a rule (a muted sender list in `iris-talk`): the same
  reason.
- Count opens from the browser count of R1-22: it has no item.
- A `sender_kind` candidate: it hides each service or each person.

## Failure list

Supported input: the person's own actions of the three verbs, through the
local API. All other input fails closed: the API refuses an unknown verb
(404), a body with an unknown field (400), an item of another form (400)
and an item of another status (409). None of these changes an item.

| Input or event | Failure mode | Safe result | Test |
|---|---|---|---|
| A verb on an item of another form | A wrong status | The submit denies it, 400, nothing stays | API test for each verb |
| A verb on an item of another status | A wrong status | The ledger entry fails, 409, the item does not change | API test for each verb |
| Two requests for the same item at once | Two entries | The store write lock orders them. The second sees the new status and fails with 409 | API test, second call |
| A crash after the apply | No answer | The entry and the status commit in one transaction, or neither does | Existing `apply_state` test |
| A choice after the last pass | An accept with a stale count | `current` counts again in the accept transaction (R5-01) | Unit test of `current` with a new refusal |
| A refusal whose item, origin message, header or sender is gone | The refusal is lost | It stops each `hide` proposal of its form | Unit test |
| Two passes with the same decisions | A second open row for the same rule | `form` is never NULL, so the unique key holds and the upsert updates the row | Unit test |
| Migration 30 after a deleted proposal | A new proposal gets the ID of a deleted one, and a stale accept names another rule | The migration keeps the `seq` of `sqlite_sequence` | Migration test |
| A choice on hostile mail | A rule learned from an attack | No candidate, no count | Unit test |
| A mute of conversations that the person started | A rule that hides each conversation that the person starts | An origin from the person gives no candidate | Unit test |
| A click on a verb by mistake | An item leaves the brief | The client hold: Undo in the Undo time sends nothing. After the apply, the undo of the entry (`restore`) sets the status back | Client test (R5-02d), apply test |
| A sender address that a rule cannot hold | A proposal that the file refuses | `check_sender` refuses it; no candidate | Unit test |
| An old program reads the new table | It reads a `hide` row as a verb | `open` skips a row that it cannot read (R5-01) | Existing behavior |

## Impact

- `crates/iris-store`: migrations 29 (the verbs) and 30 (the proposals).
- `crates/iris-changeset`: the three verbs in `Verb` and `STATE_VERBS`.
- `crates/iris-apply`: `state.rs`, the status moves of section 1.
- `crates/iris-api`: the verb route and `VerbBody` rules; the `kind` of
  a proposal.
- `crates/iris-policy`: `propose.rs`.
- `client/`: the verb buttons and the Rules sentence (one PR, with a
  before/after page).
- `docs/spec/01-mirror-store-forms.md` section 6.8,
  `docs/spec/02-changeset-policy-ledger.md` sections 4 and 6.6, the
  changeset schema.

## Assumptions

- The person's state verbs are changeset actions with the author
  `person`. Evidence: `crates/iris-api/src/verbs.rs`, module comment.
- A muted conversation stays muted when a new message arrives. Evidence:
  `crates/iris-talk/tests/talk.rs`, the test at "A muted conversation
  stays muted".
- The brief shows a conversation only when it is `active`, a feed item
  only when it is `new`, and a signal only when it is `active`. So each
  verb takes its item out of the brief. Evidence: `base_sql` in
  `crates/iris-api/src/read.rs`.
- `Policy::attention` picks the `hide` or `bundle` rule with the lowest
  ID. A new rule has the highest ID, so it changes only an item that no
  rule hides or bundles. Evidence: `crates/iris-policy/src/eval.rs`.

## External writes

None. The verbs change Iris state only. An accept writes `policy.toml`
on the machine of the person, with the procedure of R2-08.

## Open questions

- The words of the buttons ("Skip", "Mute", "Dismiss" or one word for
  all). dk picks them on the before/after page.

## Acceptance criteria

- `POST /api/items/<feed id>/skip` makes the item `skipped`, and the
  brief no longer shows it. Red if the status or the brief does not
  change.
- 10 skips of feed items from one sender give one open `hide` proposal
  with `form = "feed"`. Red at 9 skips, or with one refusal.
- An accept adds a `hide` rule with the two conditions to the file, and
  the next item of that sender and form is hidden. Red if the file has
  no rule, or the item shows.
- No table but `policy.toml` and `policy_version` holds a rule that
  hides mail. Red if a verb writes a sender list.

## Verify

```bash
cargo test -p iris-policy propose
cargo test -p iris-api verbs
scripts/local-ci/run.sh
```

## Rollout

Migrations 29 and 30 run on the first start of the new program. No flag.

## Risks

- A `hide` rule hides mail that the person later wants. The rule is in
  the file, with its name, and the hidden list of the brief shows each
  hidden item with the rule. The person removes the rule on the Rules
  screen.
- Rollback: an older program cannot open a store at a newer migration
  (`iris-store`). Roll back with the backup of R1-26.

## Plan

1. R5-02a: this spec.
2. R5-02b: the three verbs (store, changeset, apply, API).
3. R5-02c: the `hide` proposals (store table, `propose.rs`, API `kind`).
4. R5-02d: the client: the verbs and the Rules sentence, with a
   before/after page for dk.

## Test plan

- Unit tests in `propose.rs`: 10 choices give a proposal, 9 do not, a
  refusal stops it, a hostile origin gives no candidate, an item that a
  rule hides already does not count, the count equals the number of
  items that the rule hides (property test over random choices).
- API tests for each verb: the status move, 409 for another form or
  status, 404 for an unknown verb.
- A migration test: a store at migration 29 with proposals keeps each
  row and ID. With IDs 1 and 2 kept and ID 3 deleted before the
  migration, the next proposal gets ID 4.
- A repeated pass test: two passes with the same decisions leave one
  open row for each `allow` and each `hide` proposal.
- A cleanup test: an undone choice whose origin header is gone stops
  each `hide` proposal of its form.

## Spec checks

- [x] Each assumption holds with a second caller: the write lock orders
  two verbs; the accept counts again in its transaction.
- [x] Each assumption holds when an outcome is unknown: the apply and the
  ledger commit together.
- [x] Each dropped mechanism was dropped for a reason that makes it
  unnecessary. At first, opens had no item, so they could not name a
  sender. The store of R5-12 (`use_open`) now names the message of each
  open, and decision D12 counts it as a refusal.
- [x] No step is called idempotent.
- [x] Each property has a test: the count invariant has a property test.
- [x] The spec addresses the hardest problem: a learned rule that hides
  mail is only ever a line that the person adds.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported inputs and the safe result.
