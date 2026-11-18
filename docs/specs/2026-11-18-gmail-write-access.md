# Spec: Ask Gmail for write access

- Date: 2026-11-18
- Card: R2-17
- Status: Built

## Problem

Iris asks Google only for the scope `gmail.readonly`
(`crates/iris-auth/src/lib.rs`, `SCOPE`). Spec 1 section 4.9 says that
release 2 asks for `gmail.modify` when the person turns on writes. No code
does this. So each Gmail write fails with HTTP 403 and the reason
`insufficientPermissions`: `archive`, `inbox`, `read`, `unread`, `trash`,
`label`, `unlabel`, `drafts.create` and `drafts.send`. The send check on the
Mac of the owner (issue #302) found the defect: the test draft `cs-2` failed
with this raw 403.

The person sees a raw provider error and no next step.

## Goal

- The person can give Iris write access with one command:
  `iris account add <client.json> --write`.
- A plain `iris account add` keeps the read-only sign-in. A read-only account
  syncs and verifies as before.
- An account can write only if the person asked for write access and Google
  granted `gmail.modify`. Iris never trusts the asked scope alone.
- A read-only account makes no Gmail write call. Its approved and held
  actions wait, and the account state says why.
- A Gmail 403 for a missing scope gives a clear error with the command that
  fixes it.
- Spec 1 section 4.9, the README, the sign-in record and the privacy page say
  how write access works, with the consent screen step for the own OAuth
  client of the person.

## Not in scope

- A sign-in from the web client. The sign-in stays a command
  (`iris account add`).
- A change to the Accounts page of the web client (issue #681). The account state
  `read_only` is in `GET /api/status` at once. The words on the page come in
  a follow-up card with a before/after page, because the owner picks each
  screen change.
- A command that only drops write access. The person runs `iris account add`
  without `--write` (the account is then read-only in Iris, also when Google
  keeps the earlier grant), or removes the access at
  `myaccount.google.com/permissions`.
- Outlook. Its sign-in asks for `Mail.ReadWrite` and `Mail.Send` already.

## Design

### Scopes

- `iris_auth::SCOPE` stays `https://www.googleapis.com/auth/gmail.readonly`.
- New `iris_auth::WRITE_SCOPE`:
  `https://www.googleapis.com/auth/gmail.modify`. `gmail.modify` covers read,
  labels, trash, drafts and send. It does not cover the permanent delete,
  which Iris does not use.
- `Client::sign_in()` asks for `SCOPE`. New `Client::sign_in_write()` asks
  for `WRITE_SCOPE` only. One scope gives one check box on the consent screen,
  so the person cannot give a part of it.
- The check of a Google token answer accepts `SCOPE` or `WRITE_SCOPE`
  (`gmail.modify` includes read). An answer with neither gives
  `Error::ScopeMissing`, as now.
- A write sign-in whose answer does not hold `WRITE_SCOPE` gives the new
  `Error::WriteScopeMissing`. Iris saves nothing. The person asked for write
  access, so a silent read-only account would hide the refusal.
- `Tokens` gets `write: bool`: true only if the sign-in asked for write
  access and the granted scope text of the code exchange holds
  `WRITE_SCOPE`. Google can return an earlier `gmail.modify` grant to a plain
  sign-in. Then `write` is false: the plain sign-in is the way to make an
  account read-only in Iris.

### Storage

- The sealed grant in `account.token` (`engine::Grant`) gets
  `write: bool`, with the serde default `false`. A grant of an older Iris has
  no field, so it reads as read-only. That is true: all older grants have
  `gmail.readonly` only. No migration.
- `engine::save_sign_in` writes `write` from `Tokens.write`, and sets
  `account.state` to `ok` for a write grant or to the new state `read_only`.
- `read_only` is not `needs_sign_in`, so `engine::sync_accounts` keeps the
  account. Sync and verify read only and run as before.

### Apply

- `Worker::apply_step` reads the grant first. If `write` is false:
  - It sets `account.state` to `read_only`, but only while `account.token`
    is the grant that it read (the same guard as `mark_needs_sign_in`). This
    covers an account of an older Iris, whose state is `ok`.
  - It does not call `iris_apply::step`. It calls
    `iris_apply::recover_step`: only the recovery of each ledger row in
    `writing` (held-outbox spec, issue #400; issue #684), with no write
    call (`iris_apply::recover_read_only`). The reads end what they can
    decide. A mailbox call that never started does not start: its row stays
    in `writing` until a write sign-in, and then continues. So Iris makes no Gmail write for the account. Approved
    actions stay `approved` and held sends stay `held`.
  - Known limit (issue #683): a read-only sign-in that another process saves
    while a step runs does not stop that step. The next step makes no write.
  - The 14-day expiry of spec 2 does not change: the engine tick calls
    `iris_changeset::expire` for all accounts, with no apply step. So an
    approved action or a held send of a read-only account expires 14 days
    after its submit, as for any other account.
  - It gives `Error::ReadOnly`. Its text: "the account has read access only:
    run `iris account add <client.json> --write` to let Iris change mail".
    The engine prints the text once for each account, when the state changes,
    not each turn.
- A Gmail answer of HTTP 403 with the reason `insufficientPermissions` (or
  `ACCESS_TOKEN_SCOPE_INSUFFICIENT`) is a definite rejection: no change was
  made. This can happen when the person removes the access at Google, or
  when a grant says `write` and Google disagrees. Then:
  - `iris_provider::Error::is_scope_refusal()` is true for it.
  - The `Display` text of the error is "Gmail refused the change: Iris has
    read access only. Run `iris account add <client.json> --write`." in
    place of the raw provider text. The failed action keeps this text.
  - `iris_apply::Outcome::is_scope_refusal()` reads the outcome of one
    action. It is the only place that knows the form of the refusal.
  - The apply step (`iris_apply::step`) ends at the first scope refusal,
    as it ends at a refused token. So no other action of the same step
    (approved actions, sends of the write view, releases of held sends)
    makes a Gmail call. `Step.scope_refused` tells the engine.
  - `apply_step` then sets `account.state` to `read_only` (same guard).
  - Each later apply step checks the grant and the state: `write` must be
    true and the state must not be `read_only`. Only `save_sign_in` sets
    the state again, so the actions wait until a new write sign-in.

### Status

- `iris status` prints the state of each account already. It prints
  `read_only` with the command above.

## Options not chosen

- Ask for `gmail.modify` at each sign-in: simpler, but each read-only user
  gives Iris more access than it uses. Spec 1 says writes are a choice.
- Ask for `gmail.readonly gmail.modify` together: with the granular consent
  screen the person can give only one of them. One scope is clearer.
- Ask for `gmail.send` and `gmail.compose` in place of `gmail.modify`: the
  label verbs need `gmail.modify`, and `gmail.modify` covers drafts and send.
- A new column for the access, with a migration: the grant already holds the
  per-account sign-in facts, and `account.state` already shows them to the
  web client.
- Let the apply call Gmail and fail each action with the clear text: the
  person must then submit each action again after the new sign-in. Waiting
  keeps the approvals.
- Libraries: the change adds no parser or protocol code. `iris-auth` keeps
  its OAuth code (decision record `docs/decisions/2026-11-google-sign-in.md`).

## Failure list

Supported inputs: the granted scope text of a Google token answer (a list of
scope URLs with one space between them), and the Gmail error answer of
section 5 of spec 1.

| Input | Result | Test |
|---|---|---|
| Write sign-in, answer holds `gmail.modify` | Account saved, `write` true, state `ok` | `iris-auth` flow test, engine test |
| Write sign-in, answer holds only `gmail.readonly` | `WriteScopeMissing`, nothing saved | `iris-auth` flow test |
| Read sign-in, answer holds `gmail.modify` (an earlier write grant) | Saved, `write` false: the person asked for read only | `iris-auth` flow test |
| Any sign-in, answer holds neither scope | `ScopeMissing`, nothing saved | existing test |
| Refresh answer with no `scope` | Accepted, `write` of the grant unchanged | existing test |
| Grant of an older Iris (no `write` field) | Read-only: state `read_only`, no apply step | engine test |
| Gmail 403 `insufficientPermissions` on a write | Action `failed` with the clear text, the step ends, state `read_only`, later steps make no write | provider unit test, `iris-apply` step test, engine test |
| 14 days pass while the account is read-only | The approved action expires, as for any account | engine test |
| A ledger row in `writing` when the account becomes read-only | The recovery reads and ends the row; no write | engine test |
| A crash left a call that never started (`call` 0, a message for call 1) | Read-only recovery starts no call; the row waits; a write sign-in continues it | engine test `read_only_recovery_starts_no_call` |
| Gmail 403 with another reason | As now: the raw text | existing test |
| A new sign-in between the read of the grant and the state write | The guard on `account.token` keeps the new state | engine test |

All other scope text: the scope check finds no known scope and gives
`ScopeMissing` (fail closed).

## Impact

- `crates/iris-auth`: `WRITE_SCOPE`, `sign_in_write`, `Tokens.write`,
  `Error::WriteScopeMissing`, the scope check.
- `crates/iris-provider`: `Error::is_scope_refusal`, the `Display` text.
- `crates/iris`: `--write` on `iris account add`, `Grant.write`,
  `save_sign_in`, `STATE_READ_ONLY`, `Worker::apply_step`, `Error::ReadOnly`.
- Data: the sealed grant gets one field. `account.state` gets one value. No
  migration.
- API: `GET /api/status` can give `state: "read_only"`. The web client shows
  the generic "Up to date" line for it until the follow-up card.
- Docs: spec 1 section 4.9, `README.md`, the sign-in record (one setup
  step), `docs/privacy.html`, `docs/index.html`.

## Assumptions

1. `gmail.modify` allows `messages.batchModify`, `messages.trash`,
   `messages.untrash`, `labels.create`, `drafts.create` and `drafts.send`.
   Evidence: the "Authorization" section of each method in the Gmail API
   reference lists `gmail.modify`.
2. The code exchange answer of Google holds the granted scopes in `scope`.
   Evidence: the existing check `check_scope` and its tests; the real
   sign-ins of R1-02 passed it.
3. A Gmail call with too few scopes gives HTTP 403 with the reason
   `insufficientPermissions`. Evidence: the failed draft `cs-2` on the Mac
   of the owner, 2026-11-18 03:33Z.
4. A new sign-in replaces the refresh token and keeps the mirror: `save_sign_in`
   finds the account by its address. Evidence: `engine::save_sign_in`. So the
   7-day verify check of R1-07 continues.
5. The own OAuth client of the person needs `gmail.modify` on its Data Access
   page, or Google does not show the scope. Evidence: pending, the Mac check.
   If Google shows it without the step, the step does no harm.
6. Google keeps an unverified "In production" app with a second restricted
   scope as it keeps it with `gmail.readonly`. Evidence: pending, the Mac
   check. If Google blocks it, `--write` fails at the consent screen and the
   read-only sign-in still works.

## External writes

The change adds no new write call. It makes the existing Gmail writes of
spec 2 and the send spec possible. Their success, rejection, unknown result
and retry rules do not change. The new rule: a 403 for a missing scope is a
definite rejection, so the ledger frees the claim as for any other definite
rejection.

## Open questions

- None.

## Acceptance criteria

1. `iris account add client.json --write` asks Google for `gmail.modify`
   only. Red if the authorization address has another scope.
2. A write sign-in that comes back with `gmail.readonly` only saves nothing.
   Red if an account row changes.
3. A read-only account syncs, and its approved action stays `approved` with
   no Gmail call. Red if the fake Gmail gets a write request.
4. A grant of an older Iris reads as read-only. Red if its apply step calls
   Gmail.
5. A 403 `insufficientPermissions` on a write fails the action with the clear
   text, ends the step and sets `read_only`. Red if the raw text stays, the
   state stays `ok`, or a second approved action gets a Gmail call.
6. A plain sign-in makes the account read-only, also when Google gives
   `gmail.modify`. Red if `write` is true.
7. After `--write` on the Mac, the draft and send check of #302 passes.

## Verify

```bash
cargo test -p iris-auth -p iris-provider -p iris
```

Criterion 6 runs on the Mac of the owner (runbook `mac-gates.md`).

## Rollout

1. Merge. The next release has the change.
2. The owner adds `gmail.modify` on the Data Access page of the own OAuth
   client (Google Auth Platform, Data Access, Add or remove scopes).
3. The owner runs `iris account add <client.json> --write` for each account
   that must change mail. The sync of the account continues.

## Risks

- A write grant gives Iris more access. The policy, the review and the send
  switch still gate each write. Rollback: `iris account add` without
  `--write` (read-only in Iris at once), and remove the access at Google to
  take back the grant.
- An account of an older Iris shows `read_only` after the upgrade. That is
  the truth, and the state tells the person the command.

## Plan

1. R2-17: this spec.
2. R2-17: the code and the docs, one PR.

## Test plan

See the failure list. Each row has a test in the code PR.

## Spec checks

- [x] Each assumption holds with a second caller, worker or tab: the state
  write uses the token guard, so a new sign-in wins.
- [x] Each assumption holds when an outcome is unknown: a 403 is a definite
  answer. A lost answer stays an unknown result, as now.
- [x] Each dropped mechanism was dropped for a reason that makes it
  unnecessary: no migration, because older grants are all read-only.
- [x] Each "idempotent" step is not check-then-act: the state write is one
  guarded `UPDATE`.
- [x] Each property that the design depends on has a test or a command that
  shows it.
- [x] The spec addresses the hardest problem of the change: knowing the real
  access of each account, from the granted scopes.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported inputs and the safe result for all
  other input.
