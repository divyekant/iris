# Spec: the held outbox, the cancel and the send switch

- Date: 2026-11-17
- Card: R3-02
- Status: Draft

## Problem

R3-01 gives the release step of a `send` (`iris_apply::release`), and
R3-03 gives the checks before it. No engine path calls the release, and no
engine path calls the apply of an approved Gmail action (issue #360). So an
approved `draft` does not reach Gmail, and an approved `send` never goes
out. Spec 2 also needs the state `held` (C4: "`send` always goes through
the held outbox") and a cancel of a held send. These do not exist.

A send cannot be undone. The Gmail assumptions of the send spec are not
checked on a real mailbox yet (issue #302). So the first real send must
wait for the owner.

## Goal

1. The engine applies each approved Gmail action of an account (the
   mailbox verbs and `draft`), with no command from the person.
2. An approved `send` moves to `held`. It waits there for the hold delay
   (60 seconds), and the person can cancel it until the release claims it.
3. The engine releases each held send whose hold ended, only while the send
   switch is on. The switch is off until the owner turns it on.
4. The outbox view shows each held send, the time that is left, its
   warnings and a cancel button.

The card is done when the board `doneWhen` holds: "No send leaves before the
delay ends. A cancel removes it."

## Not in scope

- A hold delay for each recipient (spec 2, open question 3). Version 0 has
  one delay of 60 seconds.
- Undo of a sent mail. Gmail has no undo of a sent message. The cancel
  before the release is the undo of a send.
- Stop (release 4). The send switch is the first stop for sends only.
- The write view (R3-04).

## Design

### 1. The states of a send

| From | To | Who | When |
|---|---|---|---|
| `approved` | `held` | engine | The apply step of the account, at its next turn. The decision fields of the approval stay. |
| `held` | `applied`, `failed` | engine | The release (R3-01, section 5). The claim moves the action only if it is still `held`. |
| `held` | `denied` | engine | The release denies it: the draft changed or is gone at Gmail, or its warnings grew (R3-03). |
| `approved`, `held` | `denied` | person | The cancel in the outbox, only while no ledger row of the action exists. The reason is "Cancelled in the outbox." |
| `approved`, `held` | `cancelled` | author | The withdraw of the changeset, only while no ledger row of the action exists. |
| `approved`, `held` | `expired` | expiry | 14 days after the submit, at the move to `held`, at the claim, or at the expiry tick, if no ledger row exists. |

The cancel uses `denied` by `person`, not `cancelled`. The table allows
`cancelled` only by `author` (a CHECK of migration 7), and a change of a
CHECK needs a rebuild of the table. A cancel is a decision of the person
against the send, so `denied` with its reason says what happened. The
outbox and the review screen show the reason.

Each open send expires 14 days after its submit, also a held one. So a
send that waits with the switch off for a long time cannot leave later.

The hold starts at the later of the approval (`decided_at`) and the time
the switch went on (`send_switch.on_at`). The release step already waits
for `decided_at + HOLD_MS`. It changes to the later of the two times, so a
send that waited while the switch was off gets a full hold after the switch
goes on.

### 2. Migration 22

- Trigger `action_state` (spec R3-01, section 5.1):
  - accepts `approved>held`, `held>applied`, `held>failed`, `held>denied`
    and `held>cancelled`;
  - accepts `held>expired` with the rule of `approved>expired` (14 days
    after the submit);
  - accepts `held>denied` by `engine` (the release) and by `person` (the
    cancel), and `held>cancelled` only with a withdraw of the changeset;
  - refuses each move out of `approved` or `held` other than to `applied`
    or `failed` while a ledger row of the action is in `writing`;
  - accepts `>held` only for the verb `send`, from `approved`.
- Trigger `changeset_withdraw` cancels each `held` action that has no
  ledger row, as it cancels an `approved` one. A claimed send continues to
  its end (recovery of R3-01).
- Trigger `ledger_insert` accepts an action in `held` for the verb `send`.
- Table `send_switch`: one row (`id = 1`), `on_at` (milliseconds, `NULL`
  when off), `changed_at`. The migration writes the row with `on_at = NULL`.

### 3. The send switch

- `iris sending on` sets `on_at` to the time now. `iris sending off` sets
  it to `NULL`. `iris sending` shows the state. The commands run on the
  owner's machine only. They open the store directly, not through the
  socket, so no socket time limit applies to them.
- `iris sending on` needs a terminal and the typed word `yes`. A script or
  an agent without a terminal cannot turn sending on.
- The release reads the switch in the claim transaction. If the switch is
  off, the release makes no call and gives `Outcome::SendingOff`, and the send
  stays `held`. A switch that goes off during a hold stops the send.
- The release holds the apply lock (`apply.lock`) from before its claim to
  the end of its `drafts.send` call. `iris sending off` takes the same lock
  before it writes the switch, and tries for up to 20 seconds (`OFF_WAIT`).
  This limit stays under the 30 seconds of `ANSWER_WAIT` in
  `crates/iris/src/socket.rs`, so a later socket form of the command also
  answers in time. So when the command says "Sending is off.", no send call is in progress, and each
  later claim reads the switch as off. If the lock stays busy, the command
  writes nothing and says "A send is in progress. Run the command again."
  `iris sending on` needs no lock: the next claim reads it.
- The web client shows the state of the switch in the outbox. It cannot
  change the switch in this card, so an agent that can reach the API cannot
  turn sending on.

### 4. The apply step of the engine

The sync thread of each account runs the apply step after each sync:

1. Apply each `approved` action of the account whose verb is a Gmail verb
   (`archive`, `inbox`, `read`, `unread`, `trash`, `label`, `unlabel`,
   `draft`), oldest first, with `iris_apply::apply`. The apply keeps all
   its checks: the policy mode, the expiry, the lock, the ledger and the
   recovery.
2. Move each `approved` send of the account to `held`, in one
   transaction. A send whose submit is 14 days old or older becomes
   `expired` in place of `held`. The claim of the release checks the same
   expiry again.
3. If the switch is on, call `iris_apply::release` for each `held` send of
   the account whose hold ended, oldest first.

The thread sleeps until the earlier of the next sync (60 seconds) and the
end of the next hold. An error of one action goes to standard error, and
the step goes on with the next action. `Busy` (another apply holds the
lock) ends the step: the next turn tries again.

### 5. The cancel

- `iris_changeset::cancel(store, action, now)` moves a `send` in `approved`
  or `held` to `denied` by `person` with the reason "Cancelled in the
  outbox.", in one transaction with `UPDATE ... WHERE id = ?1 AND verb =
  'send' AND state IN ('approved', 'held') AND NOT EXISTS (ledger row)`. It
  gives `Cancel::Done` if the row changed, `Cancel::Started` if a ledger
  row of the send exists, and `Cancel::NotOpen` for all other cases.
- The release claims with `UPDATE ... WHERE state = 'held'` in its claim
  transaction. Both run on the one write connection of the store, so only
  one of the two commits. A cancel after the claim changes nothing and
  says "The send started. It cannot be cancelled."
- The command `iris cancel <d-N>` takes the ID of a draft, finds the send
  action of the draft and calls it. The API
  `POST /api/actions/<id>/cancel` calls it with the action ID. Each is
  local only, as each command and each API call.

### 6. The outbox view

`#/outbox` lists each `approved` and `held` send: the recipients, the
subject, the warnings (the words of `FLAGS`), the time left, and a Cancel
button. A banner says when the switch is off: "Sending is off. Sends wait
here." The owner picks the layout from a before/after page before the
client PR merges.

## External writes

| Call | Success | Definite rejection | Unknown result | Retry |
|---|---|---|---|---|
| `drafts.create` (apply of `draft`) | R3-01, section 4 | R3-01 | Recovery of R3-01 | Never a second create: recovery searches |
| `drafts.send` (release) | R3-01, section 6 | R3-01, section 5 | Recovery of R3-01 | Never a second send |
| The mailbox verbs | R2-02 | R2-02 | R2-02 | R2-02 |

This card adds no new call. It adds the caller.

## Failure list

Supported input: actions of the store that passed submit.

| Input | Result | Test |
|---|---|---|
| A send before the end of its hold | No `drafts.send`; the send stays `held` | engine test |
| The switch is off | No `drafts.send`; the send stays `held` | engine test |
| The switch goes on after a long wait | A full hold after `on_at` | release test |
| A cancel and a release at the same time | One commits: the cancel (no call) or the claim (the cancel says it started) | concurrency test |
| A cancel after the claim | No change; the answer says the send started | changeset test |
| A cancel of another verb, or of a closed action | No change; an error | changeset test |
| A withdraw of the changeset while its send is held | The send becomes `cancelled`; no call | store test |
| A withdraw after the claim | The send continues to its end (recovery) | store test |
| `iris sending off` while a release calls `drafts.send` | The command waits for the lock; it says off only after the call ended | engine test |
| An approved send 14 days after its submit | `expired`, not `held`; no call | changeset test |
| A held send 14 days after its submit (the switch was off) | `expired` at the claim or the tick; no call | release test |
| A crash while held | The send stays `held` after the restart, and the hold counts from `decided_at` or `on_at` | restart test |
| A crash after the claim | Recovery of R3-01 | existing tests |
| The clock goes back | The hold is longer. The clock goes forward: the hold is shorter (known limit, issue #399) | none |
| An apply error of one action | Logged; the step goes on | engine test |

## Impact

- `crates/iris-store`: migration 22.
- `crates/iris-changeset`: `cancel`.
- `crates/iris-apply`: the gate and the claim accept `held`; the switch; the
  hold start.
- `crates/iris`: the apply step in the sync thread; `iris sending`,
  `iris cancel`.
- `crates/iris-api`: the cancel route; the outbox read.
- `client/`: the outbox view.
- Spec 2, section 7.5: the switch.

## Assumptions

- The sync thread holds the token of its account. Evidence:
  `crates/iris/src/engine.rs` (`Worker`).
- One write connection serializes each write of the store. Evidence:
  `crates/iris-store/src/lib.rs` (`Store::write`).
- The Gmail assumptions of the send spec hold on a real mailbox: `pending`
  (issue #302). The switch tolerates a wrong assumption: no send goes out
  until the owner checked them and turned the switch on.

## Open questions

None.

## Acceptance criteria

1. A held send does not leave before its hold ends. Red if the engine
   releases a send at once.
2. A cancel of a held send removes it, and no `drafts.send` goes out. Red if
   the claim does not check `held`. A withdraw of its changeset does the
   same.
3. `iris sending off` returns only when no send call is in progress. Red if
   the command does not take the apply lock.
4. With the switch off, no send leaves. Red if the release does not read
   the switch.
5. An expired approval never moves to `held` and never leaves.
6. An approved `draft` reaches Gmail with no command. Red if the engine
   does not call the apply.

## Verify

```bash
cargo test -p iris-apply --test send
cargo test -p iris-changeset --test write
cargo test -p iris --test engine
scripts/local-ci/run.sh
```

## Rollout

Migration 22 adds the switch in the off state. Nothing is sent until the
owner turns it on after the Mac check of issue #302.

## Risks

- A send goes out that the owner did not mean: each send needs a person
  approval (R3-01), the warnings (R3-03), the hold and the switch.
- The engine now applies approved mailbox actions with no command. The
  policy mode `observe` still stops each one.
- Rollback: run `iris sending off` first, so no send leaves. A program
  without migration 22 refuses the database (`UnknownSchema`), so a revert
  of the code is not a rollback. Fix forward with a new PR. A restore of
  the backup from before the migration loses each change after it.

## Plan

1. R3-02 part 1: this spec.
2. R3-02 part 2: migration 22, `cancel`, the gate, the switch and the
   commands.
3. R3-02 part 3: the apply step of the engine.
4. R3-02 part 4: the API and the outbox view, with the layout pick.
