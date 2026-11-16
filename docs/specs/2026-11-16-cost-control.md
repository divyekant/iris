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

### Order

1. Threads whose newest message (by `internal_date`) is not older than 90
   days, the thread with the newest message first.
2. All other threads, in the same order.
3. In a thread, the messages oldest first (`internal_date`, then `id`).

A message with no `internal_date` sorts as the oldest.

### Batch runner

`iris_understand::batch::run(store, model, &Limits, now) -> Report` does
these steps:

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
- The model gives an error that stops the batch (spec 1 section 7.4: a
  refused key, HTTP 401 or 403). A time-out, HTTP 429 or 5xx ends the run
  too. The message stays pending.
- Another error of the store.

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
  threads, loses its items that are not pinned and gets `understood_at`
  NULL. These messages are pending again, and their hostile check runs
  again. A run that stops after this write leaves no stale item: the
  messages show as not understood.
- For each other kind change (for example stranger to person): the
  conversation of each thread with mail from that address is computed again
  (issue #106). The items from the model keep the sender kind of their run.
  Iris does not pay again for these messages.

So `iris_senders::decide` and `refresh` give each row whose kind changed,
with the old and the new kind. A crash cannot lose a change, because there
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
  stop TEXT
);
```

- `cost_micro_usd` is NULL when the model has no price (a local model).
- `stop` is the stop condition: `done`, `cost`, `messages`, `model` or
  `error`. A run that the process did not end (a crash) keeps `ended_at`
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

The estimate counts one call for each message. A second call after a bad
answer and the hostile mail that makes no call move the real cost. The
command says that the number is an estimate.

### Model settings

A new migration row adds the table `model_setting` with one row:
`provider`, `base_url`, `model`, `price_in`, `price_out` (micro-USD for one
million tokens, NULL for no price), `api_key` (BLOB, sealed with the store
key, NULL for a local model), `max_tokens`, `timeout_s`.

### Commands

- `iris model set anthropic <model> --price-in <usd> --price-out <usd>`
  reads the API key from standard input, with no echo. It never takes the
  key as an argument, so the key is not in the shell history.
- `iris model set local <base-url> <model>` sets a local model with no key
  and no price.
- `iris model show` shows the settings without the key.
- `iris understand --estimate` prints the estimate and makes no call.
- `iris understand [--max-cost <usd>] [--max-messages <n>] [--yes]` prints
  the estimate first. Before the first run of the store (no row in
  `understand_run`), it starts only with `--yes`. Then it runs the batch
  runner and prints the run row.

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
| A crash between the message and its count write | The items are saved, the count is not | The run row misses the tokens of one message. | Documented limit. |
| A second run | All messages understood | Zero calls, zero cost. | Counting fake model, two runs. |
| A sync during a run | A message changes between two writes | `store::understand` gives `changed`. The message stays pending for the next batch. | Existing tests of the store step. |
| A refused key (401, 403) | No call can work | The run stops with `stop = model`. No try counts. | Fake model error. |
| A time-out, 429, 5xx | Result not known | The run stops with `stop = model`. The message stays pending. | Fake model error. |
| The cost limit | The run reaches it | The run stops before the next message, with `stop = cost`. | Fake model with a price. |
| A sent message makes a domain known, and an understood look-alike sender becomes hostile | Old items of the look-alike mail stay | In write 1 of the sent message, its items go and its messages are pending again. A run that stops at the cost limit right after it leaves no stale item. | Store test: look-alike first, then the sent message, run stops with `--max-messages 1`. |
| Another kind change (stranger to person) | The conversations of other threads use the old kind | The same transaction computes them again. No model call. | Store test. |
| An older message of a thread fails two answers | A later reply runs without its context | The later reply waits while the older message waits. It runs after the older message is understood or stops after 3 tries. | Batch test: failed older message and a newer reply in one batch. |
| A message that is pending again for each batch | A loop with no end | A message is pending again only after its sender becomes or stops being hostile. A kind changes only when the evidence changes. | Run with no change: zero calls. |
| All other errors | | The run stops with `stop = error`, and the error goes to the caller. | |

## Impact

- `crates/iris-understand`: new module `batch` (pending query, order,
  runner, estimate). `Error::Model` carries usage.
- `crates/iris-senders`: `decide` and `refresh` give each changed row, with
  the old and the new kind.
- `crates/iris-understand/src/store.rs`: write 1 and the gone branch call
  `apply_kind_changes`.
- `crates/iris-store`: `schema_3.sql` (`understand_run`, `model_setting`).
- `crates/iris`: the commands `iris model` and `iris understand`.
- Spec 1: section 7.1 links this spec; section 5.5 lists the new tables.
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
3. The first `iris understand` without `--yes` makes zero calls. Red if it
   starts.
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
  says that it is an estimate, and `--max-cost` bounds the spend.
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

- [x] Each assumption holds with a second caller: two runners on one store
  both call `store::understand`, whose checked writes save one result.
  Both can pay for the same message. R1-28 runs one runner only.
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
