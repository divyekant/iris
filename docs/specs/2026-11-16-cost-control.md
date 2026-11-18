# Spec: cost control for understanding

- Date: 2026-11-16
- Card: R1-30 (this spec). The build: R1-31 (budget wrapper), R1-32 (batch
  runner), R1-15 (commands).
- Status: Draft

## Problem

`store::understand` (R1-10) understands one message. Nothing runs it on a
mailbox yet. A hosted model costs money for each call. A mailbox of 100,000
messages can cost much more than the person expects. The person must see the
cost before the first paid run and approve a budget. A second run must not
pay again for the same mail.

## Goal

1. A batch runner understands the pending messages of the store in the order
   of spec 1 section 7.1.
2. A message that a run understood is never sent to the model again while
   its mail does not change. A second run on the same store, with no change
   of the mail, makes zero calls for these messages. A message that is not
   understood keeps the retries of spec 1 section 7.4 (`tries`,
   `retry_at`).
3. The store keeps the calls, the tokens and the cost of each run.
4. Before the first paid run, the person sees an estimate and approves a
   budget. No paid call starts unless the budget holds its worst case. The
   computed cost can pass the budget only when the provider bills more
   input tokens than the bytes of the request body: by at most that one
   call, after which the runs stop until a new approval.
5. The person sets the model and its key with the `iris` command. The key is
   a secret (spec 1 section 5.4).

## Not in scope

These subjects change when a message must be understood again. They belong
to the sender axis and to sync, not to cost control. The runner takes the
pending state that the store gives. Each one has an issue:

- The effects of a sender kind change on other messages and other threads:
  resets after a change to or from `service`, conversations of other
  threads (#106), and kinds that a person sets (#111).
- The visibility of pinned items of hostile mail (#112).
- Resets after a `SPAM` label change, and the clear of `tries` and
  `retry_at` when a message changes (#110).
- The selection and the reset of `iris rebuild` (spec 1 section 7.5). This
  spec only says that rebuild runs through the runner.
- The run after each sync in `iris serve` and `iris mirror sync` (R1-28). It
  calls the runner of this spec. R1-28 note: `iris serve` runs it in one
  understanding thread apart from the sync threads, each 10 seconds. A run
  with nothing pending reads nothing and leaves no row
  (`Limits::keep_idle`). A run that stopped on `model` or `unknown`,
  or that could not make the model, starts a wait that doubles from 60
  seconds to 1 hour. A store error (`error`) starts no wait, and a refusal
  after a failed store write of the run marks no mail (#156). The model
  row holds the wait (`model_setting.retry_failures` and `retry_at`,
  schema 5), so `iris serve` and each `iris mirror sync` command see the
  same wait, and `iris model set` (a new row) ends it. Only evidence of the
  model changes the wait: each run that can call the model updates it in
  its end write, under the run lock. A model failure doubles it, a run with
  a call and no model failure ends it and clears the marks, and a run with no call
  changes nothing. During the wait, the run after a sync makes no call
  (`Limits::wait_after_error`), and the mail that needs the model gets
  `waits_model = 2`. The mark names its cause: 1 for a refusal of the
  model row or the budget, 2 for the wait. The end of the wait clears only
  the marks 2; the triggers and the approval clear both. The first such run after the wait sets `retry_at` to NULL
  and clears the marks in the write that starts it, under the lock, then
  calls the model. `iris understand` does not wait: it clears the marks 2 in the write
  that starts it, under the lock, and keeps the marks 1 (#155).
- The estimate on a screen of the web client.
- Prices from the network. The person gives the price of the model.

## Design

### Pending messages

A message is pending when all of these are true:

- `gone_at` is NULL and `understood_at` is NULL.
- `content_sha256` is set (the mirror has the bytes).
- It has no `DRAFT` label.
- `tries` is less than `MAX_TRIES` (3), and `retry_at` is NULL or not later
  than now.
- `waits_model` is 0 (R1-28, schema 5). The runner sets it to 1 when the
  call of the message was refused before it went out (no model, or no
  budget). Triggers of the store set it to 0 on each change of an input of
  that refusal: a label, the content, `gone_at`, `hostile` or `tries` of
  the message, or a new or changed model row (`version`). A budget change
  clears nothing, because the runner spends and refunds the budget itself.
  The approval (`iris understand --approve`) clears the marks in its own
  write. So no write path can leave the mark after a change of an input,
  and a run does not read the same waiting mail again. A thread whose only
  unread mail waits gives no thread page. The estimate counts this mail:
  an approval pays for it. The older-message rule below still counts a message that
  waits.
- Each message of its thread that is not gone has `content_sha256` set
  (spec 1 section 7.1).
- No older message of its thread waits. An older message waits when it is
  not gone, not a draft, `understood_at` is NULL and `tries` is less than
  `MAX_TRIES` (also while its `retry_at` is in the future). A message with
  `MAX_TRIES` tries stopped for good (spec 1 section 7.4), so it does not
  hold its thread.

Exception: the thread rule does not hold a message with the label `SPAM`
or whose `From` sender has the kind `hostile`. Such a message is hostile
mail, which never goes to a model and needs no thread context (spec 1
section 6.7), so it gets its hostile marks also while an older message
waits for the budget or for its `retry_at`. A message whose hostile state
depends on its own headers (marks `dmarc`, `spoof`) is not known before
write 1, so it waits like other mail.

So a message gets the thread context of each older message that can still
be understood (spec 1 sections 7.1 and 7.4). `store::understand` keeps its
own check of the retry state.

### Order

1. Threads whose newest message (by `internal_date`) is not older than 90
   days, the thread with the newest message first.
2. All other threads, in the same order.
3. In a thread, the messages oldest first (`internal_date`, then `id`). A
   message with no `internal_date` sorts as the oldest.

The runner works thread by thread. It takes the next thread that has a
pending message and that this run did not visit yet, and runs its pending
messages. In the main pass, a run calls `store::understand` at most one time for
each message. When a message of the thread stays not understood (a refused or
failed call, a change), the later messages of the thread wait (thread
rule), so the runner leaves the thread for this run. Then it takes the next
thread. So a thread that waits for the budget does not stop the free steps
of later threads.

A sender rerun in this run can make a message of a visited thread pending
again. After the last thread, if a rerun reset mail in this run, the
runner makes one more pass over all threads in the same order (it keeps
no list of threads, so the memory stays bounded). In that pass, a message can get its second
call of `store::understand` in this run, never a third. Reruns that this
pass makes stay pending for the next run. So a run always ends, and a run
with no limit and no model error leaves no message pending that it could
reach. It reads the pending rule again just before
each message, so an older message that failed in this run holds the later
messages of its thread. Only the message IDs of the current thread stay in
memory.

### Run lock

`batch::run` first takes the run lock: an OS file lock on `understand.lock`
in the data folder (`File::try_lock`, like the backup lock of `iris-store`).
A second runner, in this process or in another, gets "a run is in progress"
and makes no call. The OS releases the lock when the process ends, also
after a crash. So two runners never pay for the same message.

### Model and budget

The table `model_setting` has one row: `provider`, `base_url`, `model`,
`version`, `price_in`, `price_out` (micro-USD for one million tokens, NULL
for no price), `api_key` (BLOB, sealed with the store key, NULL for a local model),
`max_tokens`, `timeout_s`, and the budget `budget_micro_usd` (NULL for no
approval).

- The caller of `batch::run` gives no model. The runner reads the row and
  makes the model with `connect(&row)`. The program passes a `connect` that
  builds the `iris-model` client; tests pass a fake.
- Numbers: a price is a decimal USD amount from 0 to 1,000 for one million
  tokens, and a budget from 0 to 100,000 USD. The command refuses a
  negative, an infinite or a larger value, and any value with more than 6
  decimals. It converts them to micro-USD as `u64` with checked
  arithmetic. Each cost uses checked multiplication; an overflow refuses
  the call, like a budget that is too small. So a worst case is never
  negative and never wraps.
- `iris model set` takes the run lock too. It writes a new row and sets
  `budget_micro_usd` to NULL. While a run holds the lock, it fails with "a
  run is in progress". So the row cannot change during a run, and a budget
  always belongs to the model and the price that make the calls.
- A model with no price (a local model) needs no budget.
- With a price, the budget is the money that the person approved for all
  runs together, not for one run. The person sets a new budget with
  `iris understand --approve <code> --budget <usd>`, which replaces what
  is left.
- The runner reserves money before each message and settles after it
  (below). The budget never pays for a call that it did not reserve first.

### Reservation

The runner gives `store::understand` a model that wraps the real one
(`Budgeted`). The wrapper reserves money before each call and settles after
it, in its own writes:

For a model with no price (`price_in` and `price_out` NULL), the wrapper
reserves nothing and refuses nothing for the budget: it sends the call and
records its tokens and `unknown_calls` in the run row. The steps below apply
to a priced model only.

1. Before the call, in one write: if `budget_micro_usd` is NULL (no
   approval) or less than the worst case of the call, refuse the call with
   `Error::Budget` (no request goes out). The write is one `UPDATE ... SET
   budget_micro_usd = budget_micro_usd - ?worst WHERE budget_micro_usd IS
   NOT NULL AND budget_micro_usd >= ?worst`; zero changed rows is a refusal. Else take the worst case from `budget_micro_usd` and add it to
   `reserved_micro_usd` of the run row.
2. Send the request.
3. After it, in one write: give back the worst case minus the real cost
   (the known cost, or the worst case for unknown usage) to
   `budget_micro_usd`, take it from `reserved_micro_usd`, and add the counts
   to the run row.

The worst case of one call is the byte count of the whole HTTP request body
that `iris-model` sends (the system note, the instruction, the data block
and the JSON framing), as input tokens, and `max_tokens` as output tokens,
at the price of the row. A token covers at least one byte of the text that
the provider bills, and the body holds all of that text, so the byte count
is an upper bound. `iris-model` gives this count before it sends
(`Client::request_bytes`). The second try of `decide` is reserved on its own.

After a call with unknown usage to a priced model, the wrapper refuses each
later call of the run with `Error::Budget` (no request goes out). The
runner tells an unknown charge from a spent budget by the state of the
wrapper, not by the error. The same refusal follows a known cost that is
larger than its reservation. So the second try of
`decide` in the same message cannot go out: `store::understand` returns
`Error::Model`, no try counts, and the message stays pending for the next
run. The runner then stops with `stop = unknown`.

If a known usage is still larger than the reservation, the settle write
takes the real cost, the budget can go below zero, and the run stops with
`stop = budget`. The run row keeps the overrun, and the command shows it.

A crash between steps 1 and 3 keeps the whole worst case taken from the
budget. Iris never gives it back, because it cannot know the real charge.
So a crash can only make the budget smaller than the real spend.

A message that needs no model call (hostile mail, a fixed rule, a held
message) reserves nothing. So the free steps run also with no budget:

- `store::understand` refuses the model call of a message with
  `Error::Model(Budget)` after write 1. Write 1 already saved the hostile
  marks and dropped the old unpinned items. The message stays pending, and
  no try counts.
- The run goes on with the next messages, so hostile mail and fixed rules
  finish for free. At the end, `stop = budget` if any call was refused.

So an automatic run (R1-28) after each sync can spend only what is left of
the approved budget.

### Sender reruns

A sender that becomes or stops being hostile makes its mail pending again
(spec 1 section 6.7). The function `rerun_senders(tx, rows)` does this for
each message from these addresses that is not gone: it deletes the items
whose origin is the message and that are not pinned, other than the
conversation (spec 1 section 7.3: a conversation is never deleted), and
sets `understood_at`, `tries` and `retry_at` to NULL, 0 and NULL. A new
hostile state is new input, so the old retry state does not hold the
message. Then, in the same transaction, it runs `iris_talk::update` for
each thread of these messages. The conversation then counts only
understood mail that is not hostile, so no conversation shows the reset
mail, also when the run stops before it reaches these threads.

It runs in the transaction of the kind change, so a crash or an error cannot
lose a rerun:

- `store::understand` calls it in write 1, right after `iris_senders::decide`.
- The runner calls it in the write of `iris_senders::refresh` (step 2).

### Batch runner

`iris_understand::batch::run(store, connect, &Limits, now) -> Report`:

1. Take the run lock. Insert the run row. Then the link check of each row
   of `link_check` (spec 1, migration 20), with no model: a deceptive
   message is understood at once with no model call, and counts in
   `messages` and toward `Limits::max_messages`.
2. In one write: `iris_senders::refresh`, then `rerun_senders` for the rows
   that became or stopped being hostile. No model call.
3. Read `model_setting` and make the `Budgeted` model.
4. For each thread in order, and for each pending message of it: check the
   stop conditions, then call `store::understand`.
5. After each `Limits::batch` (default 50) messages, run step 2 again.
6. At the end, write `ended_at` and `stop`. The run after a sync
   (`Limits::keep_idle` false, R1-28) deletes its row in the same write if
   it read no message, made no call and holds no reservation, so a run
   each minute adds no row.

Stop conditions, checked before each message:

- No pending message is left: `stop = done`, or `stop = budget` if the
  wrapper refused a call in this run.
- The run cost reached `Limits::max_cost_micro_usd`: `stop = cost`. A run
  can pass this limit by the cost of one message.
- `Limits::max_messages` messages are done: `stop = messages`. The next run
  goes on in the same thread.
- After a refused key (HTTP 401 or 403): `stop = model`. The message stays
  pending. The provider refuses the key before it runs the model, so this
  is a definite rejection with no charge: the settle write gives back the
  whole reservation, and the call counts in `calls` with zero tokens.
- Every other answer with no reliable usage, also HTTP 429, counts as an
  unknown charge (spec 1 section 7.4 groups 429 with the results that are
  not known): the worst case stays taken, and the call counts in
  `unknown_calls`.
- After a call with an unknown result or unknown usage (a time-out, a lost
  response, HTTP 429, HTTP 5xx, an answer with no usage) to a priced model:
  `stop = unknown`. For a model with no price, no charge is possible: the
  call counts in `unknown_calls` (tokens unknown) and the run goes on. A
  time-out or an error of a local server still stops the run with `stop =
  model`, because the next call would likely fail too.
- Another error: `stop = error`, and the error goes to the caller.

### Counts

A new migration (`schema_3.sql`) adds `model_setting` and these tables:

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
  -- For a priced model, the runner inserts cost_micro_usd and
  -- unknown_cost_micro_usd as 0. They stay NULL only with no price.
  reserved_micro_usd INTEGER NOT NULL DEFAULT 0,
  stop TEXT
);

CREATE TABLE understand_estimate (
  code TEXT PRIMARY KEY,          -- 16 random bytes, hex
  model_version INTEGER NOT NULL, -- model_setting.version
  messages INTEGER NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cost_micro_usd INTEGER,
  created_at INTEGER NOT NULL
);
```

- `iris understand --approve` deletes the receipt in the write of the
  budget, so a code works one time. A receipt older than 24 hours does not
  approve; each `--estimate` deletes such old receipts.
- `cost_micro_usd` and `unknown_cost_micro_usd` start at 0 for a priced
  model and stay NULL for a model with no price. The run cost uses
  `coalesce(…, 0)` for both, so a NULL never hides a cost.
- A call with usage counts its usage.
- A call that the provider can charge for but whose usage Iris does not know
  (a time-out, a lost response, HTTP 5xx, an HTTP 200 answer with no usage
  or a usage that does not parse) counts in `unknown_calls`. Its worst case
  goes to `unknown_cost_micro_usd`. The worst case is the same bound as the
  reservation (see "Reservation"): the bytes of the whole request body as
  input tokens, and `max_tokens` as output tokens. So the settle write of a
  call with unknown usage gives nothing back.
- The run cost for the limit and the budget is
  `cost_micro_usd + unknown_cost_micro_usd`.
- `iris-model`: `Usage` gets `known: bool`. A reply with no usage gives
  `known = false`, not zero tokens.
- The `Budgeted` wrapper sees each call and its result, so it writes the
  counts of each call to the run row in its settle write. It computes the
  byte count of the request before it sends, so an error needs no byte
  count. `Decision`, `Outcome` and `Error::Model` keep their counts for
  one message only; the run row does not use them.
- The cost is the cost that Iris computes from the price that the person
  gave. A price that is too low lets the provider charge more. The commands
  say this. The run row keeps the tokens, so the person can check them
  against the bill of the provider.

### Estimate

`batch::estimate(store, now) -> Estimate` changes nothing. In a transaction
that it rolls back, it does step 2 of the runner, then reads the reachable
messages. A reachable message meets each pending condition except the
thread rule, because a run reaches the later messages of a thread when the
older ones are done. It gives:

- The count of reachable messages, in the 90-day group and in all.
- Input tokens: for each reachable message without the label `SPAM`,
  `PROMPT_TOKENS + min(size, MAX_DATA) / BYTES_PER_TOKEN`. `size` is the
  size of the raw message. A NULL size counts as `MAX_DATA`.
- Output tokens: `OUTPUT_TOKENS` for each such message.
- The cost at the price of `model_setting`, or none.

The estimate counts one call for each message. It is a number to show, not
a limit: the budget and the limits bound the spend.

### Rebuild

`iris rebuild` (later card) calls `batch::run` with a rebuild selection, so
each of its paid calls goes through the run lock and the `Budgeted` wrapper.
No path calls `store::understand` with a priced model outside `batch::run`.
Which items a rebuild deletes, and when, is a decision of the rebuild card
(spec 1 section 7.5), not of this spec.

### Commands

- `iris model set anthropic <model> --price-in <usd> --price-out <usd>`
  reads the API key from standard input with no echo. It never takes the
  key as an argument.
- `iris model set local <base-url> <model>`: no key, no price. The scheme
  of `<base-url>` must be `http`: TLS on loopback adds nothing, and the run
  connects to an IP literal, which a certificate for `localhost` does not
  match. The host of `<base-url>` must be a loopback IP literal (`127.0.0.0/8` or `::1`) or
  `localhost`, so the mail text stays on the machine. Any other host is
  refused. A remote server is not a local model.
- `iris-model` follows no HTTP redirect for any provider: a 3xx answer is an
  error, and the body goes nowhere else. For a local model, `iris` looks up
  the host before each run. Each address must be a loopback address. Then
  the client connects to the first of these addresses that accepts a TCP
  connection (else the first address), as an IP literal, so a later
  name lookup cannot change the address of the connection (R1-15, with the
  host check of `iris model set local`). A local model call uses no HTTP proxy: the client
  ignores `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and any configured proxy
  for it, so no proxy on loopback can forward the mail.
- `iris model show`: the settings and the budget left, never the key.
- `iris understand --estimate`: prints the estimate, no call. It writes an
  estimate receipt (a row of `understand_estimate`: a random ID, the
  `version` of the `model_setting` row, the counts, the cost, the time) and
  prints the ID as the approval code. Each `iris model set` makes `version`
  larger.
- `iris understand --approve <code> --budget <usd>`: approves the budget.
  It takes the run lock. It writes the budget only if a receipt with that
  code exists, its `version` is the `version` of the row now, and it is not
  older than 24 hours. Then it deletes the receipt, so a code works one
  time. `--budget <usd>` replaces the budget that is left: after the
  approval, `budget_micro_usd` is that amount. The confirmation shows the
  budget that is left now and the new budget. It also needs a terminal: if standard input is not a TTY, it
  refuses. It shows the estimate of the receipt again and asks the person
  to type the budget again. The TTY check stops scripts with no terminal.
  It does not prove that a person types: a program that runs as the same OS
  user can make a pseudo terminal. Spec 2 section 8.1 states this exception
  of release 1: host mode is the person, and the person gives no agent host
  access to the data folder.
- `iris understand [--max-cost <usd>] [--max-messages <n>]`: runs the batch
  runner with the budget that is left, and prints the run row. With no
  budget, the free steps run and paid calls are refused (see
  "Reservation").

## Options not chosen

- Batch API of the provider: cheaper, but answers can take 24 hours and a
  local server has no such API.
- A budget for each run: automatic runs after each sync could each spend it
  again.
- Counts in memory: a crash loses them.
- Built-in prices: they go out of date.

The model client is `iris-model`. Two parsers of command input are new
in `crates/iris`: the local model URL uses `http::Uri` (already in the build
through `ureq`), and a USD amount is parsed as decimal text to whole
micro-USD by a short parser with a property test. A decimal crate
(`rust_decimal`, `bigdecimal`) gives more than a bounded amount with 6
decimals needs and adds a dependency.

## Failure list

Supported input: a store of spec 1 and a model of `iris-model`. The safe
result for all other input: the run stops with `stop = error` and makes no
more calls.

| Input or event | Failure mode | Result | Test |
|---|---|---|---|
| A second run | All mail understood | Zero calls. | Counting fake, two runs. |
| A crash after the call, before write 3 | The model ran, no items | The message stays pending; the next run pays for it again (one message). | Fake model, stop after the call. |
| A crash between the reservation and the settle write of a call | The call can be charged, nothing is settled | No item is saved for that call (write 3 comes later). The worst case stays taken from the budget. The run row misses the counts of the call. | Limit #113 (counts only); batch test of the reservation. |
| The budget covers one call but not two | A second try outside the budget | The wrapper reserves each call; it refuses the second call. | Batch test. |
| `iris model set` during a run | Calls on an old row with a new budget | `iris model set` needs the run lock and fails. | Command test. |
| `iris model set` between the estimate and the approval | A budget for a model the person did not see | The approval code is the row version; a changed row refuses the approval. | Command test. |
| `iris model set local https://remote.example/v1 m` | Mail text leaves the machine | Refused: the host is not a loopback address. | Command test. |
| A local server answers 307 or 308 to a remote host | The client sends the mail to the remote host | No redirect is followed: the call fails. | Model client test with a redirecting fake server. |
| A proxy on loopback forwards to a remote host | The mail leaves the machine through the proxy | A local model call uses no proxy. | Model client test: `HTTP_PROXY` set to a loopback proxy, the call goes direct. |
| `--approve` with a code that no estimate made, an old code, or a code of another model version | Approval with no estimate | Refused. | Command test. |
| `--approve` with no terminal | An agent approves spend | Refused. | Command test. |
| A provider bills more input tokens than the body bytes | Settle passes the reservation | The budget can go below zero once; the run stops with `stop = budget` and shows the overrun. | Fake model with a large usage. |
| The budget is spent and a sender becomes hostile | Old items of its mail stay | `rerun_senders` drops them in the same write; the free steps run with no budget. | Batch test. |
| A message with 3 tries whose sender becomes hostile | The message stays out of the pending set | `rerun_senders` clears `tries` and `retry_at`. | Batch test. |
| A new `SPAM` message after an older message that waits for the budget | Its hostile mark is missing | The thread rule does not hold `SPAM` mail or mail from a hostile sender. | Batch test. |
| A sender rerun resets a message of a visited thread | The run ends with mail that it could reach still pending | One more pass, at most two calls of a message in one run. | Batch test. |
| `--price-in -1`, `--budget 1e309`, or a cost that overflows | The budget grows, or a worst case wraps | Refused before any write; checked arithmetic refuses the call. | Command and unit tests. |
| A local model with no price | Every call refused for the NULL budget | The wrapper skips the reservation for a model with no price. | Batch test with a local fake. |
| A local server gives valid answers with no usage | The run stops after one message | No price: the call counts as unknown tokens and the run goes on. | Batch test with a local fake. |
| A message with two bad answers, after its `retry_at` | A second run calls the model again | Expected: spec 1 section 7.4 retries a message that is not understood, at most 3 tries. Goal 2 covers understood mail only. | Batch test. |
| A first reply with no usage and an answer that is not valid | The second try pays again before the run stops | The wrapper refuses each call after a call with unknown usage. The message stays pending, no try counts. | Batch test. |
| The origin message of a conversation becomes hostile mail, and the run stops after one message | The conversation shows hostile mail | `rerun_senders` keeps the conversation row and computes it again in the same write. | Batch test with `--max-messages 1`. |
| A model error after write 1 made another sender hostile | The rerun is lost | `rerun_senders` ran in write 1. | Store test. |
| A priced model and no budget (`budget_micro_usd` NULL) | Paid calls with no approval | The reservation write changes no row, so the wrapper refuses each call; `stop = budget`, also from R1-28. | Batch test with a NULL budget. |
| Automatic runs after each sync | Each run spends again | All runs take from one budget. | Batch test: two runs, one budget. |
| A changed model row after the approval | The approval of another price | `iris model set` clears the budget; the runner makes its own model from the row. | Batch test. |
| A second runner | Two runs pay twice | "a run is in progress", no call. | Two runners on one store. |
| HTTP 401, 403 | The key is refused; no charge | `stop = model`, the message stays pending, the whole reservation goes back, zero tokens. | Fake model error. |
| HTTP 429 with no usage | The provider can charge | Unknown charge: the worst case stays, `stop = unknown`. | Fake model error. |
| A time-out, a lost response, 5xx, an answer with no usage | Unknown charge | Worst case in `unknown_cost_micro_usd`, `stop = unknown`. | Fake model error and a reply with no usage. |
| A bad first answer, a good second one | The first call loses its usage | Both calls count. | Fake model. |
| An older message fails in this run | A later reply runs without its context | The later reply waits. | Batch test. |
| A thread of ten new messages | The estimate counts one | The estimate counts ten. | Batch test. |
| `--max-messages 1`, a thread of two | Both run | One message, `stop = messages`. | Batch test. |

## Impact

- `crates/iris-understand`: module `batch` (pending query, order, lock,
  runner, estimate). `Decision`, `Outcome`, `Error::Model` carry usage
  status.
- `crates/iris-model`: `Usage::known`; `Client::request_bytes` before the
  call (the `Budgeted` wrapper keeps the worst case of a call that fails).
- `crates/iris-store`: `schema_3.sql`.
- `crates/iris`: `iris model`, `iris understand`.
- `crates/iris-model`: no redirects; the loopback check of a local model.
- Spec 2 section 8.1: the release 1 table of local writes lists `model set`
  and `understand`, and says who approves a budget.
- Spec 1 section 5.5 and spec 2 section 8.2 list the tables and commands.
- Memory: the IDs of one thread and one message at a time.

## Assumptions

- `BYTES_PER_TOKEN` (4), `PROMPT_TOKENS` (1,500) and `OUTPUT_TOKENS` (400):
  pending. The build measures them on the synthetic mailbox with the eval
  set. A wrong value moves only the estimate, which the budget bounds.
- `message.size` is the raw size in bytes. Evidence:
  `crates/iris-sync/src/lib.rs` writes `raw.size`, the decoded bytes of the
  raw fetch (`crates/iris-gmail/src/lib.rs`).
- A token covers at least one byte of the request. Evidence: pending; true
  for byte-level BPE tokenizers. A wrong value makes the worst case too low.
  The run then stops anyway after a call with unknown usage.

## External writes

The model call changes nothing outside the product, but it costs money.

- Success: the tokens and the cost go to the run row and the budget.
- A definite rejection: spec 1 section 7.4. The tokens count.
- An unknown result: the worst case counts; the run stops; the message stays
  pending. A retry pays again, at most one message for each event.
- Retry: the next run.

## Open questions

- The batch size of the sender refresh (50).
- The measured token constants.

## Acceptance criteria

1. Two runs on the synthetic mailbox with a counting fake that gives valid
   answers: the second makes zero calls. Red if a message is pending after
   the first run.
2. `iris understand --estimate` prints the count, the tokens and the cost,
   and makes zero calls. Red if it calls the model.
3. With a priced model and no budget, `iris understand` and `batch::run`
   make zero calls. Red if either calls.
4. Two runs share one budget: when it is spent, the second run stops with
   `stop = budget`. Red if it calls.
5. The run row has the calls, tokens and cost of the fake model, also a
   worst case for a reply with no usage. Red if a count is missing.
6. `iris model show`, logs and errors never show the key. Red if they do.

## Verify

```bash
cargo test -p iris-understand --test batch
cargo test -p iris --test understand
scripts/local-ci/run.sh
```

## Rollout

1. R1-30: this spec.
2. R1-31: `schema_3.sql`, the usage status, the `budget` module and
   `rerun_senders`.
3. R1-32: the `batch` module (runner, order, run lock, estimate).
4. R1-15: `iris model` and `iris understand`.

## Risks

- A wrong estimate: the budget bounds the spend.
- A wrong price: the budget bounds the computed cost, not the bill. The
  commands say this.
- Rollback: revert PR 3, then PR 2. The tables stay and do no harm.

## Plan

One PR for each card:

1. R1-30: this spec.
2. R1-31 (depends on R1-30): budget wrapper, counts, sender reruns.
3. R1-32 (depends on R1-31): batch runner, run lock, estimate.
4. R1-15 (depends on R1-32): commands.

## Test plan

- `crates/iris-understand/tests/batch.rs`: the cases of the failure list.
- `crates/iris/tests/understand.rs`: the commands with a fake model server.

## Spec checks

- [x] A second caller: the run lock; the estimate changes nothing.
- [x] An unknown outcome: see External writes.
- [x] Each dropped mechanism has a reason (Options not chosen, Not in
  scope with issues).
- [x] No check-then-act: the pending query picks work; the checked writes
  of `store::understand` decide.
- [x] Each property has a test (Failure list).
- [x] The hardest problem is no paid call outside the budget; criteria 3
  and 4 test it.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the safe result for all other input.
