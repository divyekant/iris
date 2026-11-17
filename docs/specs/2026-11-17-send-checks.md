# Spec: checks before the send

- Date: 2026-11-17
- Card: R3-03
- Status: Draft

## Problem

R3-01 lets an agent or a person write a draft and approve a send. Each
`draft` and `send` action goes to a person, but the review screen shows
only the flags `first_contact` and `bulk` for it. The person does not see
that a draft goes to a new address, promises money or a date, mentions an
attachment that is not there, or copies text from a message of another
person. These are the mistakes that a person cannot undo after the send.

## Goal

Each `draft` and each `send` action gets the draft flags of spec 2,
section 5.3, from the content of its draft, at submit and again at the
release. The review screen already shows each flag in words
(`client/src/review.ts`, `FLAGS`), so a flag is the warning. The card is
done when each case of the test set (section "Acceptance criteria") gives
its flag before the send.

## Not in scope

- `outside_thread`: it needs the session of release 4 ("after the session
  read external mail"). No action gets it in this card.
- A new screen, a new flag name or a reason text for each flag. The flag
  names and their words exist (schema `$defs/flag`, `FLAGS`).
- A check that uses the model. The checks are fixed rules (see "Options not
  chosen").
- The held state and the cancel (R3-02).

## Design

### 1. The draft content of an action

| Verb | Content | Account |
|---|---|---|
| `draft` | The arguments of the action, after `Content::check` (`to`, `cc`, `subject`, `body`). A reply has the subject of the parent. | The account target, or the account of the thread. |
| `send` | The `content` of the `draft` row of the target. | The account of the `draft` row. |

An action with no content that passes `Content::check` gets no draft flag.
Submit denies it already (R3-01).

### 2. The flags

One pure function, `iris_changeset::draft_flags(c, account, content)`,
gives the flags in this order. Submit and the release call the same
function.

| Flag | Rule |
|---|---|
| `new_recipient` | An address of `to` or `cc` has no `sender_evidence` row with the rule `sent`, and is not the account address. A typing error in a known address (a wrong recipient) is a new address, so it gets this flag. |
| `money` | The body or the subject has an amount: a currency sign (`$`, `€`, `£`, `¥`, `₹`) next to a digit, or a number next to a currency code or word (`USD`, `EUR`, `GBP`, `INR`, `JPY`, `CAD`, `AUD`, `CHF`, `dollar(s)`, `euro(s)`, `pound(s)`, `rupee(s)`). |
| `date` | The body or the subject has a date: a month name or its short form next to a number, a weekday name, `today`, `tonight`, `tomorrow`, `next week`, `next month`, `end of the day`, `end of the week`, `EOD`, `EOW`, an ISO date (`2026-11-17`) or a numeric date (`17/11`, `11/17/2026`). |
| `attachment` | The body or the subject has a word of the attachment group: `attach`, `attached`, `attachment(s)`, `enclosed`. Iris drafts have no attachments (issue #235), so a mention always means a missing attachment. |
| `untrusted_content` | The body has a run of 8 words that is a phrase in the clean text of a message that the account did not send (`message_fts`, the sender is not the account address). |

Words match with no case difference and on word bounds. A match inside a
longer word does not count (`attachments` counts, `reattach` does not).

The `untrusted_content` check splits the body into words as the full-text
index does (letters and digits; each other character ends a word). It
queries each run of 8 words that starts at word 0, 8, 16 and so on, as an
FTS5 phrase, and stops at the first match. So a copied run of 15 words or
more always matches. A shorter copied run can match or not. The words are
letters and digits only, so the phrase text needs no escape.

### 3. Submit

Pass 2 of submit (the flags) calls `draft_flags` for each `draft` and
`send` action that is not denied, and adds the flags to `first_contact`.
The flags stay in `changeset_action.flags`, as for each other verb. They
are not in the hash: the hash binds to the content, and the flags come
from the content and the store.

### 4. The release

After the draft at Gmail reads back with the approved hash, the release
calls `draft_flags` again on the stored content. If it gives a flag that
the approved action does not have, the release denies the send with the
reason "The warnings of the draft changed after the approval." and makes
no `drafts.send` call. A new send action of the draft goes to review with
the new flags. A flag that went away (for example, the owner wrote to the
address in the meantime) does not deny the send.

## Options not chosen

- The model for `money` and `date`: it costs a call for each draft, needs a
  budget, and gives a different answer for the same text. Fixed rules are
  the same each time and cost nothing. A false flag costs the person one
  look. A missed flag is the risk, so the rules flag each amount and each
  date, not only a promise.
- A comparison only with the thread of a reply for `untrusted_content`: a
  new mail can carry text from any message. The full-text index covers
  each message with clean text.
- Each run of 8 words (stride 1): up to 8 times the queries for no gain on
  a copied run of 15 words or more.
- A regex library: `regex` is not a dependency of `iris-changeset`. The
  rules are word lists and short digit shapes, and a word scanner of the
  standard library covers them. The property test covers the scanner.

## Failure list

Supported input: a content that passes `Content::check` (UTF-8, a body of
at most 256 KiB, at most 50 addresses).

| Input | Result | Test |
|---|---|---|
| Each case of the test set | Its flag | `each_case_of_the_test_set_gives_its_flag` |
| A text with no case of the test set | No draft flag | same test, negative cases |
| Random text, random Unicode, a body of 256 KiB | No panic; the same flags for the same input; the `untrusted_content` check makes at most one query for each 8 words | property test `draft_flags_hold_for_random_text` |
| A body with FTS5 syntax (`"`, `*`, `NEAR`, `AND`, `^`) | The words are letters and digits only; the query is a phrase of words | property test (the generator writes these) |
| A store with no `sender_evidence` row (no sync of sent mail yet) | Each address is new: `new_recipient` | example test |
| A message with no clean text yet (not understood) | Not compared; `untrusted_content` can be missed | known limit, issue |
| The full-text query fails (a store error) | Submit fails with the error; no action changes | the store error path of submit |

## Impact

- `crates/iris-changeset`: `draft_flags`, the call in submit.
- `crates/iris-apply`: the second call in the release.
- No migration, no schema change, no screen change.

## Assumptions

- The review screen shows each flag of an action with its words. Evidence:
  `client/src/views/Request.svelte` (`{#each a.flags ...}`) and
  `client/src/review.ts` (`FLAGS`).
- `sender_evidence` has a row with the rule `sent` for each recipient of
  each message that the account sent and that the sync copied. Evidence:
  `crates/iris-senders/src/lib.rs` (the insert of the rule `sent`).
- `message_fts` holds the clean text of each understood message, with the
  tokenizer `unicode61 remove_diacritics 2`. Evidence:
  `crates/iris-store/src/schema_1.sql`.

## External writes

None. The release keeps the external write of R3-01; this card adds one
check before it, with no call.

## Open questions

None.

## Acceptance criteria

1. The test set gives each flag: a mention of an attachment, a typing
   error in a known address, a new address, an amount, a date, and a run
   of 15 words from a message of another person. Red if a rule is removed.
2. A draft with none of these gives no draft flag. Red if a rule matches
   too much (for example, a digit alone as money).
3. A send whose flags grow after the approval is denied at the release,
   and no `drafts.send` goes out. Red if the release does not check again.

## Verify

```bash
cargo test -p iris-changeset --test flags
cargo test -p iris-apply --test send
scripts/local-ci/run.sh
```

## Rollout

None. No migration. Existing actions keep their flags.

## Risks

- A false flag: the person looks once more. No send goes out without a
  person, so a flag never blocks a send by itself (only a change after the
  approval does).
- A missed flag: the rules are English words. A draft in another language
  gets no `money` or `date` flag (known limit, issue).
- Rollback: revert the code PR. No data changes.

## Plan

1. R3-03 part 1: this spec.
2. R3-03 part 2: `draft_flags`, submit and the release, with the tests.
