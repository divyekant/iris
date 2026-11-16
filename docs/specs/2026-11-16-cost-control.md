# Spec: cost control for understanding

- Date: 2026-11-16
- Card: R1-15
- Status: Draft

## Problem

`store::understand` (R1-10) understands one message. Nothing runs it on a
mailbox yet. A hosted model costs money for each call. A mailbox of 100,000
messages can cost much more than the person expects. The person must see the
cost before the first run, and a second run must not pay again for the same
mail.

## Goal

1. A batch runner understands the pending messages of the store in the order
   of spec 1 section 7.1, in batches.
2. A message that is understood is never sent to the model again, unless its
   state changes (spec 1 section 7.1). A second run on the same store makes
   zero model calls.
3. The store keeps the calls, the tokens and the cost of each run.
4. Before the first run, the person sees an estimate: the count of messages,
   the tokens and the cost.
5. A run stops at a cost limit that the person sets.
6. The person sets the model and its key with the `iris` command. The key is
   a secret (spec 1 section 5.4).

## Not in scope

- The run after each sync in `iris serve` and `iris mirror sync` (R1-28).
  R1-28 calls the batch runner of this spec.
- The estimate on a screen of the web client. The local API can show the
  run rows later. This spec gives the command and the library function.
- A rate limit for older mail in the background (spec 1 section 7.1). R1-28
  decides when a batch runs. This spec gives the order only.
- Prices from the network. The person gives the price of the model.

## Design

### Pending messages

A message is pending when all of these are true:

- `gone_at` is NULL and `understood_at` is NULL.
- `content_sha256` is set (the mirror has the bytes).
- It has no `DRAFT` label.
- `tries` is less than `MAX_TRIES` (3), and `retry_at` is NULL or not later
  than now.
- Each message of its thread that is not gone has `content_sha256` set
  (spec 1 section 7.1: the engine understands a thread when the mirror has
  each known message of it).
- No older message of its thread waits (the thread rule below).

A held message (`tries` or `retry_at`) is not pending, so a run makes no
call for it. `store::understand` keeps its own check of the retry state.

The thread rule: an older message of the thread waits when it is not gone,
not a draft, `understood_at` is NULL and `tries` is less than `MAX_TRIES`.
While an older message waits (also for its `retry_at`), the later messages
of the thread are not pending. A message with `MAX_TRIES` tries stops for
good (spec 1 section 7.4), so it does not hold its thread. The order of
spec 1 section 7.1 thus holds: a message gets the thread context of each
older message that can still be understood.

The runner reads the pending rule again for each message, just before the
call, not only when it selects the batch. So an older message that failed
in the same batch holds the later messages of its thread.

### Resets

A reset makes a message pending again. One function of `iris-store`,
`reset(tx, message)`, does it in the write of the change:

- It deletes the items whose origin is the message and that are not pinned
  (spec 1 section 7.5), other than the conversation.
- It sets `understood_at`, `tries` and `retry_at` to NULL, 0 and NULL.
- It does the same for each later message of the thread (by
  `internal_date`, then `id`) that is not gone. A later message got the
  earlier one as thread context (spec 1 section 7.4), so its items can
  depend on it.

So an old item of a message that became spam never stays visible, also
when the run stops before it reaches the message. Step 0 of the next run
computes the conversation again for each thread with a message that got a
reset after its conversation was computed (`understood_at` NULL and a
source of the conversation item). R1-28 starts a run after each sync. The retry state belongs
to the old state of the message, so a reset clears it too. Without that, a
message with 3 failed tries stays out of the pending set after a change.

These writes reset the message, in the same transaction as the change:

| Change | Writer |
|---|---|
| New content (another `content_sha256`) | `iris-store` `put_messages` |
| A gone message comes back | `iris-sync` |
| The label `SENT`, `DRAFT` or `SPAM` comes or goes | `iris-store` (label writes of sync) |
| The sender of the message becomes or stops being hostile | `apply_kind_changes` (below) |

The run of `store::understand` itself sets `understood_at` to NULL in write
1. That is not a reset: the retry state stays.

The build adds `SPAM` to the label check of `iris-store`, which now checks
`SENT` and `DRAFT` only, and the clear of `tries` and `retry_at` to each
writer in the table.

### Order

1. Threads whose newest message (by `internal_date`) is not older than 90
   days, the thread with the newest message first.
2. All other threads, in the same order.
3. In a thread, the messages oldest first (`internal_date`, then `id`).

A message with no `internal_date` sorts as the oldest.

### Batch runner

`iris_understand::batch::run(store, model, &Limits, now) -> Report` does
these steps:

One run at a time: the runner first takes the run lock, an OS file lock on
the file `understand.lock` in the data folder (`File::try_lock`, the same
kind of lock as the backup lock of `iris-store`). A second runner, in this
process or in another, gets the error "a run is in progress" and makes no
call. The OS releases the lock when the process ends, also after a crash, so
no stale lock stays. So two runners never pay for the same message, and the
cost limit of the one run covers all calls.

0. Once at the start of each run, also with no pending message and before
   the approval check: `batch::refresh` (below). It makes no model call.
1. Read the next batch: at most `Limits::batch` (default 50) pending message
   IDs, in the order above. Only the IDs stay in memory.
2. For each message that is still pending, call `store::understand`. Then
   add its calls, tokens and cost to the run row, in one small write.
3. After the batch, in one write, call `iris_senders::refresh` and apply its
   kind changes (below).
4. Stop when a stop condition is true. Else go to step 1.

Stop conditions:

- No pending message is left.
- The run cost reached `Limits::max_cost_micro_usd`. The check comes before
  each message, so a run can pass the limit by the cost of one message (at
  most two calls).
- `Limits::max_messages` messages are done.
- The model refuses the call before any charge: a refused key (HTTP 401,
  403) or HTTP 429. `stop = model`. The message stays pending.
- A call has an unknown result or unknown usage: a time-out, a lost
  response, HTTP 5xx, or an answer with no usage. `stop = unknown`. The
  message stays pending (or its try counts, for an answer that is not
  valid).
- Another error of the store.

### Sender refresh

`batch::refresh(store, now)` does, in one write: `iris_senders::refresh`,
then the kind changes (below), then the conversations of the threads with a
reset (see "Resets"). It makes no model call and needs no approval.

- `batch::run` calls it at step 0.
- Sync calls it after each sync batch (spec 1 section 6.7: "After each
  batch of sync or understanding, a refresh"). R1-28 wires this call into
  `iris serve` and `iris mirror sync`. So the sender kinds stay current also
  when no paid run is approved.

### Kind changes

A sender kind changes in three places: `iris_senders::decide` in write 1 of
`store::understand`, `iris_senders::refresh` in the gone branch of
`store::understand`, and `refresh` at the end of each batch. A change made
for one message can change the kind of another sender, for example a sent
message that makes a domain known, so a look-alike sender becomes hostile.

Invariant: a kind change and its effects commit in the same transaction. One
function, `apply_kind_changes(tx, changes, now)`, runs right after each
`decide` and each `refresh`, in the same transaction:

- For a sender that became hostile or stopped being hostile: each message
  from that address that is not gone, and each later message of the same
  threads, gets a reset. These messages are pending again, and their
  hostile check runs again. A run that stops after this write leaves no stale item: the
  messages show as not understood.
- For a change of the known persons (a row becomes or stops being `person`,
  or the name of a `person` row changes): each message whose `From`
  display name is the old or the new name (case does not matter) and whose
  `From` address is not the address of that row, and each later message of
  the same threads, gets a reset. The spoof mark of these messages can
  change (spec 1 section 6.7).
- For each other kind change (for example stranger to service): the
  conversation of each thread with mail from that address is computed again
  (issue #106). The items from the model keep the sender kind of their run.
  Iris does not pay again for these messages.

So `iris_senders::decide` and `refresh` give each row whose kind or name
changed, with the old and the new kind and name. The name writes of
`iris-senders` (`upsert`) report a name change too. A crash cannot lose a change, because there
is no list to carry from one transaction to the next.

### Run rows

A new migration (`schema_3.sql`) adds this table:

```sql
CREATE TABLE understand_run (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  messages INTEGER NOT NULL DEFAULT 0,
  calls INTEGER NOT NULL DEFAULT 0,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_micro_usd INTEGER,
  unknown_calls INTEGER NOT NULL DEFAULT 0,
  unknown_cost_micro_usd INTEGER,
  stop TEXT
);
```

- `cost_micro_usd` is NULL when the model has no price (a local model).
- `unknown_calls` counts each call that the provider can charge for but
  whose usage Iris does not know: a time-out, a lost response, HTTP 5xx, and
  an HTTP 200 answer with no usage or a usage that does not parse
  (`BadAnswer`). A call with usage counts its usage, not a worst case. Each
  one adds its worst case to `unknown_cost_micro_usd`, at the price of the
  model.
- The worst case of one call comes from the request itself, not from an
  estimate: the byte count of the instruction and the data block that the
  call sent, as input tokens (a token covers at least one byte), and
  `max_tokens` as output tokens.
- After a call with unknown usage, the run stops with `stop = unknown`. So
  the cost check can pass the limit by at most one message, whose worst
  case is in `unknown_cost_micro_usd`.

### Usage of each call

- `iris-model`: `Usage` gets `known: bool`. A reply with no usage, or a
  usage that does not parse, gives `known = false`, not zero tokens. An
  error after the request went out (`BadAnswer`, a time-out, a lost
  response, 5xx) carries the byte count of the request.
- `iris-understand`: `Decision` and `Outcome` carry, over all calls of the
  message (also a bad first answer before a good second one): the known
  usage, `unknown_calls` and `unknown_bound` (the sum of the worst cases).
  `Error::Model` carries the same three values for the calls before the
  error and the call that failed.
- The cost check of the run uses `cost_micro_usd + unknown_cost_micro_usd`.
  The command shows both numbers: "spent" and "up to ... more for calls with
  no answer".
- `stop` is the stop condition: `done`, `cost`, `messages`, `model`,
  `unknown`, `approval` or `error`. A run that the process did not end (a crash) keeps `ended_at`
  NULL and `stop` NULL.
- A model error that has no answer still has calls. `Error::Model` gets the
  usage of the calls before the error, so that the run row counts them.

### Estimate

`iris_understand::batch::estimate(store, price, now) -> Estimate` reads the
pending messages and gives:

- The count of pending messages, in the 90-day group and in all.
- The input tokens: for each pending message without the label `SPAM`,
  `PROMPT_TOKENS + min(size, MAX_DATA) / 4`. `size` is the size of the raw
  message in bytes.
- The output tokens: `OUTPUT_TOKENS` (400) for each such message.
- The cost from the price, or none.

The estimate first does step 0 of the run (refresh and kind changes) in a
transaction that it rolls back. Then it counts the pending messages in that
transaction. So the estimate counts the messages that step 0 resets, and it
changes nothing.

The estimate counts one call for each message. A second call after a bad
answer and the hostile mail that makes no call move the real cost. The
command says that the number is an estimate.

### Model settings

A new migration row adds the table `model_setting` with one row:
`provider`, `base_url`, `model`, `price_in`, `price_out` (micro-USD for one
million tokens, NULL for no price), `api_key` (BLOB, sealed with the store
key, NULL for a local model), `max_tokens`, `timeout_s`.

### Approval of paid runs

A run that can cost money needs the approval of the person, on every path:

- `model_setting.approved_at` and `model_setting.run_max_cost_micro_usd`
  hold the approval and the cost limit of each run.
- `batch::run` checks them first, also before step 0. With a price and no
  approval, it stops with `stop = approval` and makes no call. A model with
  no price (local) needs no approval.
- `batch::run` uses the smaller of the cost limit of the caller and
  `run_max_cost_micro_usd`. So an automatic run (R1-28, after each sync)
  has the limit that the person approved, and cannot pass it.
- `iris understand --yes` records the approval and the limit
  (`--max-cost`, default 1 USD for each run). The limit applies to the cost
  that Iris computes from the price in `model_setting` (see Risks). `iris model set` clears the
  approval, because the price or the model changed.

### Rebuild

`iris rebuild` (spec 1 section 7.5) resets the messages that it selects, in
one write, and then calls `batch::run`. So a rebuild has the run lock, the
approval check, the cost limit and the run row of every other run. No path
calls `store::understand` with a priced model outside `batch::run`.

### Commands

- `iris model set anthropic <model> --price-in <usd> --price-out <usd>`
  reads the API key from standard input, with no echo. It never takes the
  key as an argument, so the key is not in the shell history.
- `iris model set local <base-url> <model>` sets a local model with no key
  and no price.
- `iris model show` shows the settings without the key.
- `iris understand --estimate` prints the estimate and makes no call.
- `iris understand [--max-cost <usd>] [--max-messages <n>] [--yes]` prints
  the estimate first. With no approval (above), it starts only with
  `--yes`, which records the approval. Then it runs the batch runner and
  prints the run row.

## Options not chosen

- Batch API of the provider (one request with many messages): it is
  cheaper, but the answer comes after up to 24 hours, and a local server has
  no such API. One call for each message keeps one path for both.
- Counts in memory only: a crash loses them, and the person cannot see the
  cost of the last run.
- A cost limit that reserves the worst case of a message before the call:
  the worst case (two calls, 16,000 output tokens each) stops most runs
  early. The run can pass the limit by one message, and the command says so.
- Prices built into Iris: prices change, and a built-in table goes out of
  date without a release.

No parser or protocol code is new. The model client is `iris-model`.

## Failure list

| Input or event | Failure mode | Result | Test |
|---|---|---|---|
| A crash during a message | The model call ended, write 3 did not commit | The message stays pending. The next run calls the model for it again: the cost of one message. The run row keeps `stop` NULL. | Fake model, stop after the call, run again: one more call. |
| A crash between the message and its count write | The items are saved, the count is not | The run row misses the tokens of one message. A limit issue records it. | Documented limit. |
| A second run | All messages understood | Zero calls, zero cost. | Counting fake model, two runs. |
| A sync during a run | A message changes between two writes | `store::understand` gives `changed`. The message stays pending for the next batch. | Existing tests of the store step. |
| A refused key (401, 403) | No call can work | The run stops with `stop = model`. No try counts. | Fake model error. |
| A time-out, a lost response, 5xx | Result not known; the provider can charge | The run stops with `stop = unknown`. The message stays pending. `unknown_calls` and `unknown_cost_micro_usd` grow by one worst case. | Fake model error: the run row has the worst case. |
| HTTP 200 with no usage or a body that does not parse | The provider can charge, Iris has no count | The call counts in `unknown_calls` with its worst case from the request bytes, also when a second try succeeds. The run stops with `stop = unknown`. | Fake model reply with no usage. |
| Sent mail makes Alice a known person, and an understood message from another address has the display name Alice | The spoof mark is stale | The same transaction resets that message and the later messages of its thread. | Store test. |
| A sync removes the domain that made a look-alike sender hostile | The estimate shows zero, the run pays | The estimate does step 0 in a rolled-back transaction, so it counts the reset messages. | Batch test. |
| A second `iris understand` while a run goes on | Two runs pay twice | The second run gets "a run is in progress" and makes no call. | Two runners on one store: one gets the error, the fake model sees each message once. |
| A first sync calls the runner (R1-28) before the person saw an estimate | Paid calls with no approval | `batch::run` stops with `stop = approval`, no call. | Batch test with a priced fake model and no approval. |
| An earlier message of an understood thread gets new content or `SPAM` | Later items used it as context | The reset covers the later messages of the thread. | Store test. |
| Sync removes the only sent evidence of a person, and no paid run is approved | The kind stays `person` | Sync calls `batch::refresh`, which needs no approval. | Batch test: refresh with a priced model and no approval. |
| `iris rebuild` with a priced model | Paid calls outside the guards | Rebuild calls `batch::run`. | R1 rebuild test. |
| HTTP 429 | The provider refused the call | The run stops with `stop = model`. The message stays pending. No cost. | Fake model error. |
| A sync marks the only sent evidence of a person gone, with no new mail | No message is pending | Step 0 refreshes the kinds at the start of the next run and applies the changes. | Batch test with zero pending messages. |
| A `SPAM` label comes or goes on an understood message | The hostile mark is stale | The label write resets the message, so the next run checks it again. | Store test. |
| New content or a label change on a message with 3 tries | The message stays out of the pending set | The reset clears `tries` and `retry_at`. | Store test. |
| The cost limit | The run reaches it | The run stops before the next message, with `stop = cost`. | Fake model with a price. |
| A sent message makes a domain known, and an understood look-alike sender becomes hostile | Old items of the look-alike mail stay | In write 1 of the sent message, its items go and its messages are pending again. A run that stops at the cost limit right after it leaves no stale item. | Store test: look-alike first, then the sent message, run stops with `--max-messages 1`. |
| Another kind change (stranger to person) | The conversations of other threads use the old kind | The same transaction computes them again. No model call. | Store test. |
| An older message of a thread fails two answers | A later reply runs without its context | The later reply waits while the older message waits. It runs after the older message is understood or stops after 3 tries. | Batch test: failed older message and a newer reply in one batch. |
| A message that is pending again for each batch | A loop with no end | A message is pending again only after its sender becomes or stops being hostile. A kind changes only when the evidence changes. | Run with no change: zero calls. |
| All other errors | | The run stops with `stop = error`, and the error goes to the caller. | |

## Impact

- `crates/iris-understand`: new module `batch` (pending query, order,
  runner, estimate, run lock). `Decision`, `Outcome` and `Error::Model`
  carry the known usage, `unknown_calls` and `unknown_bound`.
- `crates/iris-model`: `Usage::known`; errors after the request carry the
  request bytes.
- `crates/iris-senders`: `decide` and `refresh` give each changed row, with
  the old and the new kind.
- `crates/iris-understand/src/store.rs`: write 1 and the gone branch call
  `apply_kind_changes`.
- `crates/iris-store`: `schema_3.sql` (`understand_run`, `model_setting`);
  resets in `put_messages` and the label writes.
- `crates/iris-sync`: the reset when a gone message comes back.
- `crates/iris`: the commands `iris model` and `iris understand`.
- Spec 1: section 7.1 links this spec; section 5.5 lists the new tables
  and the reset rule of the `message` row. Spec 2 section 8.2 lists the
  new commands.
- Memory: one batch of IDs (50 integers) and one message at a time.

## Assumptions

- About 4 bytes of mail text give one token. Evidence: pending. The
  estimate tolerates a wrong value: it is a number to show, not a limit. The
  build measures the ratio on the synthetic mailbox with the eval set and
  sets the constant.
- The instruction and the headers take about 1,500 tokens
  (`PROMPT_TOKENS`). Evidence: pending; the build measures it with the
  prompt `understand/0.2`.
- An answer has about 400 output tokens. Evidence: pending; measured on the
  eval set.
- `message.size` is the size of the raw message in bytes. Evidence:
  `crates/iris-sync/src/lib.rs` writes `raw.size`, the count of decoded
  bytes of the raw fetch (`crates/iris-gmail/src/lib.rs`). A NULL size
  counts as `MAX_DATA`.

## External writes

The model call is the only external call. It reads data and changes nothing
outside the product, but it costs money.

- Success: the answer goes to `store::understand`, and the tokens go to the
  run row.
- A definite rejection (an answer that is not valid, a refusal, a 4xx for the
  data): spec 1 section 7.4. The try counts. The tokens count.
- An unknown result (time-out, lost response, crash): the message stays
  pending. A retry calls the model again and pays again. The cost of a
  repeat is at most one message for each event.
- Retry: the next run.

## Open questions

- The batch size: 50 is a first value. The build can change it if a test
  shows a better memory or time result.
- The exact `iris_talk::update` call for a thread in step 3.3 (with no
  current message).

## Acceptance criteria

1. Two runs on the synthetic mailbox with a counting fake model: the second
   run makes zero calls. Red if a message is pending after the first run.
2. `iris understand --estimate` on a store with pending mail prints the
   count, the tokens and a cost, and the fake model gets zero calls. Red if
   it calls the model.
3. The first `iris understand` without `--yes` makes zero calls, and so does
   `batch::run` from any caller with a priced model and no approval. Red if
   either starts.
4. A run with `--max-cost` stops before the next message when the run cost
   reaches the limit. Red if it continues.
5. The run row has the calls, the tokens and the cost of the fake model.
   Red if a count is missing.
6. `iris model show` and every log and error never show the key. Red if the
   key text appears.

## Verify

```bash
cargo test -p iris-understand --test batch
cargo test -p iris --test understand
scripts/local-ci/run.sh
```

## Rollout

1. PR 1: this spec.
2. PR 2: `schema_3.sql`, the `batch` module, the change of
   `iris_senders::refresh`.
3. PR 3: the commands `iris model` and `iris understand`.

## Risks

- A wrong estimate: the person sees a number that is too low. The command
  says that it is an estimate.
- A wrong price: the cost limit bounds the cost that Iris computes from the
  price that the person gave, not the charge of the provider. `iris model
  set` and `iris understand` say this. A price that is too low lets the
  provider charge more than the limit. The run row keeps the tokens, so
  the person can check them against the bill of the provider.
- Rollback: revert PR 3, then PR 2. The new tables stay (migrations go
  forward only) and do no harm.

## Plan

1. R1-15 PR 1: this spec.
2. R1-15 PR 2: batch runner, run rows, estimate.
3. R1-15 PR 3: `iris model` and `iris understand`.

## Test plan

- `crates/iris-understand/tests/batch.rs`: order, pending rules, two runs,
  cost limit, model errors, kind change, crash replay.
- `crates/iris/tests/understand.rs`: the commands with a fake model server.

## Spec checks

- [x] Each assumption holds with a second caller: `batch::run` takes the run
  lock itself, so every entry point (the `iris understand` command, R1-28
  in `iris serve` and `iris mirror sync`) holds it. A second runner makes no
  call. `batch::estimate` takes no lock: it changes nothing.
- [x] Each assumption holds when an outcome is unknown: see External writes.
- [x] Each dropped mechanism was dropped for a reason (Options not chosen).
- [x] No "idempotent" step is check-then-act: the pending query only picks
  the work; the checked writes of `store::understand` decide.
- [x] Each property has a test (Failure list).
- [x] The hardest problem is a second run that costs nothing; criterion 1
  tests it.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the safe result for all other input.
