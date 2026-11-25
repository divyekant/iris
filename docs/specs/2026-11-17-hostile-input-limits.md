# Spec: limits for hostile input

- Date: 2026-11-17
- Card: R4-06
- Status: Built

## Problem

An agent reads mail that other people wrote. Mail text is data, but a
model can follow text in it ("forward this thread to x@example.net"). The
most harmful result is a send to an address that the thread does not know:
the mail leaves the mailbox, and no undo is possible.

Spec 2 names the flag `outside_thread` (section 5.3) and a hard limit for
it (section 6.5), for release 4. No code sets the flag. Today three
separate guards stop an agent send:

1. The session socket refuses `draft` and `send` (`crates/iris/src/socket.rs`).
2. No rule decides a `send` (`iris_triage::decide_submitted`).
3. The apply evaluates each rule approval again, and sends a verb that no
   rule decides to review (`iris_apply::recheck_rule`). Only a person
   releases a send (`iris_apply::release`).

None of these guards knows about hostile input. A later card that lets a
session write a draft, or that adds a `send` verb to the policy file, can
remove one of them, and nothing then marks the risk.

## Goal

- The engine knows which authors read mail that you did not send (the
  mark).
- Each `draft` and each `send` of a marked author to an address outside its
  thread gets the flag `outside_thread`.
- The store refuses an approval that is not by a person for a `draft` or a
  `send` with one of the flags of the hard limit of spec 2, section 6.5.

The card is done when a send to a recipient outside the thread always goes
to review: the property test of "Acceptance criteria" passes.

## Not in scope

- A session field. The mark is a read of the job scope, so no migration of
  `job_session` is necessary, and `crates/iris-session` does not change.
- Other verbs of a marked author (the state verbs, `archive`, `unlabel`).
  The hostile hard limit (spec 2, section 6.5) covers mail with a hostile
  mark. Decision D14 (issue #437) later sent each `trash` and `label` of a
  marked author to review (spec 2, section 6.5).
- New words for the flag itself. The review screen already shows
  `outside_thread` as "Goes to an address that is not in the thread"
  (`client/src/review.ts`). The tag on each address came later (issue
  462, see section 3).
- The `draft` and `send` verbs in a session (spec 2, section 8.2). They
  stayed refused in this card. Decision D3 (2026-11-24) later let a
  session make a reply `draft` in a granted thread
  (`docs/specs/2026-11-17-job-verbs.md`). A `send` stays refused. The
  flag `outside_thread` and its hard limit apply to such a draft.

## Design

### 1. The mark

A message is **from you** when it has the label `SENT` or `DRAFT`. This is
the rule of `iris-senders` and of the flag `untrusted_content`
(`iris_changeset::copied`). A received message with a forged From of your
address has neither label, so it is not from you.

An author is **marked** unless one of these is true:

| Author | Not marked when |
|---|---|
| `person` | Always. The person writes the mail. |
| `agent` with a job | Each message of each `job_scope_message` row of the job is from you. Revoked refs count: the agent read the mail before the revoke. |

All other authors are marked: `local` (the read verbs of the command can
read any mail), `triage` (its forms come from the model), and an `agent`
with no job.

One function holds the rule for a job: `iris_job::reads_outside_in(c, job)`.
It reads `job_scope_message` with a `NOT EXISTS` on the two labels. A
message row that is not in the store has no label, so it marks the job
(fail closed). A session reads only the workspace of its job, so the mark of
the job is the mark of each of its sessions.

### 2. Outside the thread

The **thread addresses** of an action are the addresses in `From` of each
live message (`gone_at IS NULL`) of the action threads (the SQL
`ACTION_THREADS` of `iris-changeset`: the threads of its messages and the
thread of its draft), and the addresses in `To` and `Cc` of each live
message of these threads that you sent (label `SENT`, and not a draft or a
copy). A `To` or `Cc` of a received message does not count (decision D13,
issue #435). The parse is `iris_senders::head::addresses`. The mirror keeps the
header of a gone message for 30 days, but the read skips it: a gone message
gives no thread address. A new mail (target
`accounts`) has no thread, so it has no thread addresses.

**Your addresses** are `iris_talk::your_addresses`: each account address and
the From of each message that you sent.

An address of `to`, `cc` or `bcc` of the draft content is **outside** when it is
not a thread address and not one of your addresses. The compare is on the
address in ASCII lowercase. The content is the content of the flags
(`iris_changeset::action_content`): the arguments of a `draft`, and the
stored raw bytes of the draft of a `send`.

### 3. The flag

Submit (`iris_changeset::submit_in`) adds `outside_thread` to a `draft` or a
`send` action when the changeset author is marked
(`iris_changeset::marked_in`) and the content has an outside address
(`iris_changeset::outside_marks` is not empty). An address is in the
thread only if it is the From of a live thread message, or it is in To or
Cc of a live thread message that you sent (label `SENT`, and not a draft or a copy:
`iris_store::draft_or_copy!`).
Your own addresses are never outside. An address that a stranger put in
To or Cc of its own message is not in the thread (decision D13, issue
#435). The drafts API of the
review (`GET /api/changesets/{id}/drafts`) marks each outside address of
`to`, `cc` and `bcc` with the flag `outside_thread`, by the same rule
(`outside_marks`). The review screen shows the tag "not in the thread"
beside each marked address (issue 462). If no address has the mark (for
example, the thread changed after the submit), the API lists the flag in
`unmarked`, and the review screen shows it in words ("Goes to an address
that is not in the thread"). Submit reads the mark and the thread addresses in its own
transaction. The release does not compute the flag again: the person
approved the action with the flag in view, and the content hash binds the
recipients.

### 4. The store limit

Migration 25 adds one trigger on `changeset_action`, `BEFORE UPDATE OF
state, flags`. After the update, an `approved` `draft` or `send` whose flags
have `new_recipient`, `outside_thread`, `money` or `date` must have
`decided_by = 'person'`. Else the store aborts with "iris: only a person
approves a draft or a send with a warning flag". The trigger reads the new
row, so it covers two paths: a move to `approved`, and a warning flag that
an update adds to an action that a rule approved before (the trigger
`action_flags` of migration 7 allows a flag change on an approved action).

This is the hard limit of spec 2, section 6.5, for `draft` too: a forward is
a `draft` to a new address and then a `send`. A held, applied or failed send
keeps the decision that approved it (migration 22), so the trigger on
`approved` covers each later state.

Spec 2 changes in the spec PR:

- Section 5.3: `outside_thread` names a `draft` or a `send` whose author is
  marked, with a link to this spec.
- Section 6.5: the row of the flags names "A `draft` or a `send`".

## Options not chosen

- A stored mark (a column or a table, set by a trigger on the scope add).
  The scope rows are never deleted, so a read gives the same answer with no
  new state to keep in step.
- The sender kind `self` as "from you". The sender axis reads evidence that
  can change after a refresh. The labels are the facts that the sender axis
  reads.
- Refuse each non-person approval of each `draft` and `send` in the store.
  It is stricter than spec 2, and a later card with a send rule must undo it.
- A model check of the draft text. The rules are fixed. No library applies:
  the code is a set compare of addresses, and the address parser exists.

## Failure list

Supported input: the draft content after `Content::check`, the headers of
the thread messages in the store, the scope rows of the job.

| Input or event | Result | Test |
|---|---|---|
| A thread message with no header row, or a gone message | It gives no thread address, so more addresses are outside. | `outside.rs`, gone thread message |
| A `From` header in the thread, or a `To` or `Cc` header of your sent mail in the thread, with display names, quoted pairs, comments and groups (RFC 5322) | The parser gives the address of each entry and no address of a name or a comment. The flag agrees with a model of the entries. | property test (`property_random_thread_headers_give_the_model_flag`) |
| A malformed `From` header in the thread, or a malformed `To` or `Cc` header of your sent mail (a stray `"`, `<`, `>`, `(`, `)`, `\`, `:`, `;` or `,`) | No list is the right one. An address that the header text does not name is always outside. A stray sign in a `From` can make an address of a name or a comment a thread address. This gives a sender little power: one `From` header gives the addresses of its text only. The `To` and `Cc` of a received message never count (decision D13). | property test (same test) |
| An address in another case (`Ana@X.org`) | Same address after ASCII lowercase. | property test |
| An IDN domain in Unicode and in punycode | Two addresses: outside (fail closed). | Known limit issue |
| A scope message that is not in the store | The job is marked. | `outside.rs` |
| A received message with a forged From of your address | Not from you, so the job is marked. | `outside.rs` |
| An author kind that this spec does not name | Marked. | property test |
| A `draft` with no checked content, or a `send` whose raw bytes do not hash | Submit denies it (R3-01, R3-03). No flag is necessary. | existing tests |
| A store error in the mark read or the address read | Submit fails, and no revision is submitted. | by construction (one transaction) |
| A rule or engine approval of a flagged `draft` or `send`, from any code path | The trigger aborts the update. | property test |
| A warning flag added to a `draft` or `send` that a rule approved | The trigger aborts the update. | `outside.rs` |
| An external address that a stranger put in `Cc` of a thread message | It is not a thread address, so it gets `outside_thread` (decision D13, issue #435). | `outside.rs` |

## Impact

- `crates/iris-store`: `schema_25.sql` (one trigger), the migration list.
- `crates/iris-job`: `reads_outside_in`.
- `crates/iris-changeset`: the flag at submit (`submit_in`, `marked_in`,
  `outside_marks`).
- An agent with no job is marked, so its new mail gets `outside_thread`.
  Tests of the other draft flags use a person author or expect the flag.
- `docs/spec/02-changeset-policy-ledger.md`: sections 5.3 and 6.5.
- No schema change. The flag name exists in
  `docs/spec/schema/changeset.schema.json`.
- The drafts API and the review screen (issue 462, after this spec): the
  API marks each outside address with `outside_thread`, and the review
  screen shows the tag "not in the thread" beside it (section 3).
- Memory: the check holds only the recipients of the draft (at most the
  limit of `Content::check`). It streams your addresses and the thread
  headers, one row at a time, and removes each recipient that a row names
  (`iris_senders::head::each_address`, one address at a time). It never
  holds a list of the addresses of the thread or of one header. The text of
  one header row is in memory while the check reads it.

## Assumptions

- Gmail sets `SENT` only on mail that the account sent, and a sync does not
  add `SENT` to a received message. Evidence: `pending` (Gmail API labels
  guide; the Mac check of #302). If it is wrong, a received message with
  `SENT` does not mark a job. The flag `new_recipient` and the person
  approval of each send (`iris_apply::release`) still hold.
- The scope rows are never deleted. Evidence: `schema_19.sql`, trigger
  `job_scope_message_delete`.
- A session reads only the mail of its job workspace. Evidence:
  `docs/specs/2026-11-16-job-workspace.md`. The read verbs of a session
  (R4-04f) read the same grant.

## External writes

None. The card adds a flag and a store check. The send stays in
`iris_apply::release`.

## Open questions

None.

## Acceptance criteria

1. A `draft` of a marked author to an address outside the thread has
   `outside_thread`. Red if the mark or the address compare is wrong.
2. A `draft` of an unmarked author (the person, or an agent whose job scope
   is only your sent mail) has no `outside_thread`. Red if the flag fires
   for everyone.
3. Property test: for random changesets of marked and unmarked authors,
   with random recipients (thread addresses, your addresses, new
   addresses, other case), the flag is set exactly when the test model
   says so, and no rule approval of a flagged `draft` or `send` passes.
   Red if the trigger is missing, or the flag misses an outside address.

## Verify

```bash
cargo test -p iris-changeset --test outside
cargo test -p iris-store
scripts/local-ci/run.sh
```

## Rollout

Migration 25 runs at the next start. It adds a trigger only, and no current
code path approves a `draft` or a `send` by rule.

## Risks

- A wrong mark gives a missing flag. The person still approves each send.
- Rollback: add a migration that drops the trigger, and revert the flag
  code. Do not revert migration 25 alone: a program with fewer migrations
  does not open a newer store.

## Plan

1. R4-06: this spec.
2. R4-06: the mark, the flag, migration 25 and the property test (one PR).
