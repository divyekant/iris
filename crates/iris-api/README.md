# iris-api

The local API of Iris (tracker item R1-17). The API gives the web client
and the read verbs of the `iris` program their data from the store. The API
reads only, except the policy changes of the web client (see
[Policy](#policy)), the review of the person: decisions, withdraws and comments
(see [Review](#review)), the state verbs of the person (see
[State verbs](#state-verbs)), the cancel of a send in the outbox (see
[Outbox](#outbox)), the days of use of the measures page (see
[Measures](#measures)), and the press of Send in the write view (see
[Write](#write)).

## Use

```rust
let auth = Arc::new(iris_api::Auth::new(addr)?);
// Print (and open) the one-time sign-in address.
println!("{}", auth.sign_in_url().unwrap());
let app = iris_api::router(Arc::new(store), auth).merge(iris_web::router());
```

Merge the API routes before the fallback of `iris_web::router()`. The read
verbs call the same functions as the routes: `status`, `search`, `message`,
`raw`, `brief`, `items` and `item`. So the verbs and the client get the same
data.

## Endpoints

All endpoints in this table use `GET` and give JSON, except `/raw`. Times are RFC 3339 in
UTC. IDs have the format of specification 1, section 5.7 (`m-42`).

| Endpoint | Gives |
|---|---|
| `/api/status` | `{"mode": "observe", "accounts": [...], "messages": {"total", "fetched", "understood", "gone"}, "model": false, "waits_model": 0}`. `model` is `true` when a model is set (`iris model set`). `waits_model` counts the live messages that the understanding step marked as waiting for a model or a budget. A draft, a copy, gone mail and understood mail never count. Each account has `id`, `address`, `provider`, `state`, `mirror_ready_at`, `sync` (`{"kind", "step"}` of an unfinished first sync or resync, or `null`) and `last_sync` (`{"kind", "started", "ended", "result", "after_failure"}` or `null`). `after_failure` is `true` when the sync before this one failed or stopped. With no account, `accounts` is `[]`. `time_zone` is the IANA name of the time zone of the owner (decision D1), or null when the time zone of the computer has no IANA name. `time_zone_set` is `true` when the owner set it (`iris zone <name>` or `POST /api/zone`), and `false` when it is the time zone of the computer. |
| `POST /api/zone` | Body `{"zone"}`: the person sets the time zone of the owner, as `iris zone` does (issue 891). `zone` is an IANA name (`Europe/Berlin`), or null for the time zone of the computer. The answer is `{"time_zone", "time_zone_set"}`, as in the status. A name that the computer does not know, a missing `zone` or another key is a 400, and nothing is written. |
| `/api/brief` | The first brief of specification 1, section 8. See [Brief](#brief). The first brief with content writes the time of the first brief (see [Measures](#measures)). |
| `/api/brief/{section}?limit=&after=` | One page of a list section of the brief (`you_owe`, `owed_to_you`, `conversations`, `feed`, `signals` or `strangers`), of the bundle of rule N (`bundle-N`, newest first), or of the items that a `hide` rule leaves out (`hidden`, newest first; each item has the rule and the reason in `attention`): `{"count", "items", "next"}`, in the same order and with the same filter as the brief. The page has `duplicates` only when an item of the page has a duplicate, as in the brief. `limit` is 1 to 200, default 50. `after` is the `next` value of the page before. With no `after`, the page starts at the first item. `next` is a text value that holds the section name and the sort keys of the last item of the page. Do not make or change it. The API does not sign it, so a made cursor gives the items after its keys. The next page starts after these keys in the order of the section. An item that leaves the section, or that changes its sort key behind these keys, moves no other item. An item whose sort key moves across these keys between two pages can show twice or not show. For a new view, read the section again from the start. An empty `after`, or an `after` that is not a cursor of the same section, is a 400. An unknown section is a 404. `bundle-N` is a 404 when the active policy has no `bundle` rule N and no item of the brief is in its bundle. |
| `/api/handled?limit=&after=` | `{"count", "actions": [...], "next"}`: each action that a rule decided without the person, in its state now (`approved`, `held`, `applied`, `failed`, `undone` or `denied`; not `in_review`, which the review lists), the newest first. A `failed` action can have changed some of its messages. `count` is the count of all such actions. Each action is `{"changeset", "revision", "n", "verb", "args", "messages", "state", "rule", "policy", "why", "reason", "at"}`. `messages` is the count of its messages. `why` is the reason of the author, and `reason` is the reason of the decision. `limit` is 1 to 200, default 50. `after` is the `next` value of the page before. An `after` that is not such a value is a 400. |
| `/api/gate?limit=&after=` | The senders that wait at the stranger gate. See [Stranger gate](#stranger-gate). |
| `/api/gate/declined?limit=&after=` | The senders that the person declined at the gate, with their items that would wait there. See [Stranger gate](#stranger-gate). |
| `/api/changesets` | `{"changesets": [...]}`: each open changeset (specification 2, section 5.1), the newest changeset first. A changeset is open while one or more of its actions is `proposed`, `in_review`, `approved` or `held`. The answer streams in parts of 64 KiB. |
| `/api/changesets/{id}` | One changeset (`cs-77`) in each state, as the document of specification 2, section 5.1. The answer streams. An ID that no changeset has is a 404. |
| `/api/messages?ids=` | `{"messages": [...]}`: the summary of each message of `ids` (`m-1,m-2`, 1 to 200 IDs), in the order of `ids`, with `"gone": false`. A message that is gone, or that is not in the mirror, is `{"id", "gone": true}`. A text that is not a message ID is a 400. The review shows the real messages of each action with it. |
| `/api/items?form=&status=&type=&before=&limit=` | `{"items": [...], "next"}`. Items newest first, by the date of the origin message. `form` is one of the five forms. `type` is a record type, and needs `form=record`. `before` is the `next` value of the page before: an item ID of the form of the filter. `limit` is 1 to 200, default 50. Items with the status `gone` are not in the list, unless `status=gone`. A `status` must be a status of the form (of any form, with no `form`). A `type` must be a record type of the schema. An item has `ready` as in the brief (issue 478). |
| `/api/items/{id}` | One item (`c-`, `v-`, `r-`, `f-` or `s-`), also when it is gone. |
| `/api/search?q=&limit=` | `{"messages": [summary]}`, newest first. `q` is the search text of specification 1, section 5.6. `limit` is 1 to 200, default 50. |
| `/api/messages/{id}` | One message: the summary, and `to`, `cc`, `text` (the clean text, or `null`), `gone`, `hostile` (the hostile marks, for example `["dmarc"]`), `sender_kind` (the kind of the sender row of the `From` address: `person`, `service`, `stranger` or `hostile`; a sender with no row or no kind is a `stranger`; if two rows have the address in a different case, the first kind of `hostile`, `stranger`, `service`, `person` is used, and a row with no kind counts as `stranger`) and `items` (the IDs of the items that are not gone and that have the message as a source). |
| `/api/messages/{id}/raw` | The raw source (RFC 5322) of the message, as a download (`message/rfc822`, `Content-Disposition: attachment`). |
| `/api/jobs?state=&limit=&after=` | `{"jobs": [...], "next", "sessions"}`: the jobs of the agents (card R4-09), newest first. `sessions` maps the ID of each `working` job of the page to its newest session, in the form of `session` of `/api/jobs/{id}`, or to null when the job has no session (issue 791). A job in another state has no entry. `state` is `open` (the default: `working`, `waiting` and `needs_you`) or one state: `working`, `waiting`, `needs_you` or `done`. Each job is the job document of `docs/spec/schema/job.schema.json` without `events`. `limit` is 1 to 200, default 50. `after` is the `next` value of the page before (a job ID, `j-12`); the page holds the jobs with a lower ID. Another `state`, or an `after` that is not a job ID, is a 400. |
| `/api/items/{id}/summary` | `{"text", "based_on", "created", "model"}`: the summary of the thread of the origin message of the item (issue 478). `based_on` is the newest message that the summary read. The conditions are those of `"summary"` in `ready`: the item needs the person, and the summary is not stale. Else the answer is a 404. An ID that is not an item ID is a 400. Iris makes the summary in the background. This route only reads it. |
| `/api/items/{id}/joinable` | `{"joinable": bool}`: true when the item (`c-`, `v-`, `r-` or `s-`) can join a matter now. The item is live (the rule of `matter_live`) and in no open matter. Another kind of ID is a 400. |
| `/api/items/{id}/job` | The mail that a start of a job from the commitment or conversation `id` grants now: `{"messages": [...]}`. Each entry is the summary of a message (`id`, `thread`, `date`, `from`, `subject`) with `readable`: true when the message can go into a workspace now. The list holds each source message of the item and each message of the thread of each source, in the order of the thread and then the date. The checks and the error codes are those of `POST /api/jobs`. |
| `POST /api/jobs` | Body `{"item", "goal", "due", "messages"}`: starts a job from one commitment (`c-4`) or conversation (`v-2`) for the person ("Handle this", `docs/specs/2026-11-17-start-job.md`). `goal` is 1 to 2,000 characters after the trim. `due` is an RFC 3339 time, or null. `messages` is the list of message IDs of `/api/items/{id}/job` that the person saw; if it is not the grant of now as a set, the answer is a 409 and no job starts. The scope of the job is the item and the thread of each source message of the item. The answer is 201 with the job document. Another kind of ID, a goal or a due date that is not valid, or another field is a 400. An ID that no item of that form has is a 404. A gone item, an item with no source, or a source that an agent cannot read (hostile mail, a draft, gone, not understood, the source of a signal) is a 409. If a job that is not done has the item in its scope, the answer is a 409 `{"error", "job"}` with that job ID, and no job starts. |
| `/api/jobs/{id}` | One job (`j-12`): `{"job", "changesets", "threads", "session"}`. `job` is the job document with its events. `changesets` holds each submitted changeset whose author names the job, the first submit first: `{"id", "at", "state", "why", "actions"}`, where `at` is the first submit and `actions` counts the actions of the newest revision in each state (`{"applied": 2}`). A changeset that was never submitted is not in the list. `threads` has one entry for each thread of a wait of the job: the summary of the newest message of the thread that is not gone, with `"gone": false`, or `{"id", "gone": true}`. `session` is the newest session of the job, or null: `{"id", "agent", "state", "end", "exit", "ended"}`. `state` is `starting`, `running`, `ending` (ended, with its processes not reaped yet) or `ended`. `end` is the end reason (`exit`, `stop`, `revoke`, `job_state`, `time_limit`, `engine`, `launch_failed` or `helper_failed`) or null, `exit` is the exit code of an `exit` end or null, and `ended` is the end time or null. The answer streams. If a row of the job does not parse, the answer is a 500 with no part of the job. An ID that no job has is a 404. |
| `/api/watches` | `[watch]`, newest first. A watch is `{"id", "text", "created", "state", "arrived", "message"}`. `state` is `waits`, or `arrived` after its one notice. `message` is the summary of the first new message that matched, or `null` while the watch waits or when the message is gone. See [Watches](#watches). |
| `/api/notices` | `[notice]`, newest first, at most 50: the notices by rule that are not done (card R5-08). A notice is `{"id", "rule", "rule_name", "title", "text", "created", "message"}`. `title` is the sender and `text` the subject, one line each. `rule` and `rule_name` are the `notify` rule at the time of the notice. `message` is the summary of the mail, or `null` when the mail is gone. |

A message summary is `{"id", "thread", "date", "from": {"name", "address"},
"subject", "labels"}`.

An item is the form document of `docs/spec/schema/`. The store row gives the
`id` and the `status` of the document.

The text of a message is data. Show it as text, never as markup.

## Brief

| Field | Content |
|---|---|
| `you_owe` | Open commitments with the direction `you_owe`, where the counterparty is a `person` or a `service`. Order: by `due`, an item with no `due` last, then by created. |
| `owed_to_you` | Open commitments with the direction `owed_to_you`, where the owner is a `person` or a `service`. Oldest first. |
| `conversations` | Active conversations with `new` greater than 0 or `your_turn` true, and with no stranger participant and no declined participant. A conversation with a stranger is only in `strangers`. A conversation with a declined stranger and no other stranger is in no section. Newest `latest.at` first. |
| `records` | The count of each record type that is not gone, for example `{"receipt": 3}`. |
| `feed` | Feed items with the status `new`, in the order of the publisher address, then newest first. |
| `signals` | Active signals whose `expires` time is later than now. The time that expires first comes first. |
| `strangers` | Open commitments whose counterparty or owner is a stranger, and active conversations with a stranger participant. A stranger that the person declined at the gate does not count here. Newest first. |
| `gate` | The first 20 senders of `/api/gate`: `{"count", "senders", "next"}`. |
| `declined` | The count of senders of `/api/gate/declined`. |
| `hostile` | The count of hostile mail of the last 7 days. |
| `not_understood` | The count of messages that the model failed 3 times. |
| `proposal` | The triage proposal: `{"count", "verbs": {"archive": 412}}`, the count of messages of the actions of the open triage changesets that are `proposed` or `in_review` (specification 2, section 5.4). The engine stores these changesets (`iris-triage`), and `/api/changesets` lists them. |
| `handled` | What Iris did without the person: `{"count", "verbs"}`. `verbs` counts the messages of the actions that a rule decided, for each verb (`/api/handled` lists them). `count` is `hidden` and these messages together. |
| `review` | `{"changesets", "actions"}`: the count of the actions that wait for a person (`proposed` or `in_review`, specification 2, section 5.5) in the open changesets of `/api/changesets`, and of those changesets. A changeset whose new revision is a draft is not open, so it is not counted. |
| `oldest_fetched` | The date of the oldest fetched message that is not gone, or `null`. While the first sync continues, the brief covers only the mail from this date (specification 1, section 4.2). |
| `bundles` | One entry for each `bundle` rule that bundles one or more items of the brief, in the order of the rule ID: `{"rule", "name", "count", "items"}`. `name` is the name of the rule in the active policy, or `Rule <id>` if the active policy has no such rule. `items` holds the newest 20 items. `/api/brief/bundle-<rule>` gives all. |
| `hidden` | The count of items of the brief that a `hide` rule leaves out. `/api/brief/hidden` lists them. |

Each list field is `{"count", "items"}`. `count` is the full count. `items`
holds at most 200 items. Times sort as instants, not as text, so a time
offset does not change the order. A list field can also have `duplicates`:
for an item in `items`, the IDs of the newer items whose `duplicate_of` is that
item (specification 1, section 7.5). The brief shows such a pair once, so the
newer item is not in `items` and not in `count`.

An item of a list field, of a bundle or of a page of `/api/brief/{section}`
has `"ready": ["draft"]` when Iris prepared a reply for it (card R3-09): the
thread of the mail that made the item has a reply draft of `iris write`, and
the draft is not stale. A draft is stale when the thread has a newer message
than the one that the draft answers, when its newest message is gone, or when
a message of the thread is hostile or waits for understanding. `ready` holds
`"summary"` when the item needs the person (You owe, Owed to you or
Conversations) and the thread of the mail that made the item has a summary
that is not stale (issue 478). A summary is stale for the same reasons as a
draft, and also when a message that it read gets a new text, label or date,
or goes away. An item at the stranger gate never has `"summary"`. With
nothing ready, the item has no `ready` field.

The attention of an item (specification 2, section 6.4) decides where it
shows. Each item that a list field or `records` would have is in exactly one
place: its list field or `records` when its `attention.state` is `shown` or
missing, the bundle of its rule when it is `bundled`, and only the count
`hidden` when it is `hidden`. The feed sorts by the address of
the sender row of the publisher, so the items of one publisher stay together. A sender with no sender row, or with no kind yet, is
a stranger.

## Policy

The policy view of the web client (specification 2, section 6.3). Each
endpoint gives the policy object below. `POST` endpoints take a JSON body. The `mode`, `remove`, `grant` and `accept` endpoints need the field `version`: the active version that the person saw, or `null` for the policy of the first start. A body with no `version` field gets 400, and nothing changes. A route with no body (the sender answers, the withdraw of a changeset, the policy discard, the notice done, the watch remove and the matter decline) takes zero bytes or the JSON object `{}`, and gives 400 for any other body (a body of only spaces too) before it changes anything. The route keeps at most 64 bytes of the body for the check: a longer body gets 400. The read stops at the first transport frame past the limit, so one received frame can be larger.

| Endpoint | Does |
|---|---|
| `GET /api/policy` | Gives the policy object. Changes nothing. |
| `POST /api/policy/confirm` | Body `{"sha256"}`: the `file.sha256` that the person saw. Makes the file on disk the active policy. |
| `POST /api/policy/discard` | Body `{}`. Keeps the file on disk as a copy, then writes the text of the active policy to `policy.toml`. With no active version, no new file is written. The answer has `saved`: the name of the copy in the data folder (`policy.toml.saved-<seconds>`), or `null` when there was no file. |
| `POST /api/policy/mode` | Body `{"version", "mode"}`. Writes the new mode to the file and makes it active. |
| `POST /api/policy/remove` | Body `{"version", "id"}`. Removes the rule from the file and makes the file active. The ID is never used again. |
| `POST /api/policy/grant` | Body `{"version", "job", "verbs"}`: a grant inside one job (`docs/specs/2026-11-17-start-job.md`). Adds one rule at the end of the file: `effect = "allow"`, the `verbs`, `when = { author = "agent" }`, `by = "person"`, `job`, the next rule ID and the date of today, and makes the file active. `verbs` has 1 to 11 verb names of spec 2 section 4 (not `draft` or `send`), each one time; else a 400. A job ID that no job has is a 404. A job in the state `done` is a 409. |
| `POST /api/policy/accept` | Body `{"version", "id"}`: the ID of an open proposal. Adds its rule at the end of the file, with `by = "proposal"`, the next rule ID and the date of today, and makes the file active. A proposal that is not open is a 409. |
| `POST /api/policy/dismiss` | Body `{"id"}`. Dismisses an open proposal and changes no file. Iris does not propose the same rule again. A proposal that is not open is a 409. |

`version` is the active version that the person saw, `null` for the policy
of the first start. If it is not the active version, or if the file on disk
is not the active policy (also no file after a confirmation), the change is a
409. A file that the person saves during a change or a discard is never
replaced. A change keeps the other text
and the comments of the file. A comment directly above a removed rule goes
with the rule. A comment that a blank line separates from the rule stays.

The policy object:

| Field | Content |
|---|---|
| `mode` | The mode of the active policy: `observe` or `apply`. |
| `version` | The active version, or `null` at the first start. |
| `next_id` | One more than the highest rule ID that a rule had. A new rule can use it, because the ID of a removed rule is never used again. |
| `rules` | The rules of the active policy, in the order of the file. |
| `proposals` | The open rules that Iris proposes from your approvals (spec 2, section 6.6), the oldest first. Each is `{"id", "approvals", "created_at", "rule"}`. `rule` is the rule that an accept adds, with `next_id` and the date of today; its `line` is 0. |
| `error` | `{"line", "message"}` when the active text is not valid any more (for example after a program update), else `null`. The active policy then applies nothing. |
| `file` | The file on disk, compared to the active policy. `state` is `same`, `missing`, `invalid` (with `error`) or `changed`. A `changed` file has `sha256`, `text`, `mode`, and the rules that it `added`, `removed` and `changed` (`{"before", "after"}`). A rule that keeps its ID and changes any field other than `line` is changed. |

A rule is `{"id", "name", "effect", "verbs", "when", "by", "added", "job",
"line"}`. `when` holds only the conditions that the rule sets. `line` is the
line of the rule in its file, from 1.

## Review

The person decides the actions of one changeset in one request
(specification 2, section 5.5).

| Endpoint | Does |
|---|---|
| `POST /api/changesets/{id}/decide` | Body `{"actions": [{"revision", "n", "approve", "hash"}]}`: 1 to 500 decisions, each action once. `approve` `true` approves the action and needs the `hash` that the review showed. `false` denies it, with the reason "The person denied it in the review.". The answer is the changeset document after the decision. |
| `POST /api/changesets/{id}/withdraw` | No body. The person withdraws the changeset, in any state but `withdrawn` (section 5.5: "each changeset"): each open action and each draft action becomes `cancelled`, and applied and decided actions stay. A second withdraw is a 409. The withdraw of a write also withdraws the changeset of its send, so the mail does not leave; if the send left or is leaving now, the withdraw is a 409 and changes nothing. The answer is the changeset document after the withdraw. |
| `POST /api/changesets/{id}/comments` | Body `{"revision", "text", "key"}`. The person comments on the newest revision of an open changeset of a job (`docs/specs/2026-11-17-review-comments.md`). `text` has 1 to 4,000 characters, not only white space, and no NUL. `key` has 16 to 64 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`; the client makes a new key for each new comment. A retry with the same key and the same body gives the first comment and writes nothing. A comment on a `waiting` job wakes the job. The next session of the job reads the comment in `comments/cs-<id>.md`. The answer is the comment: `{"id", "changeset", "revision", "text", "at"}`. A bad body is a 400. A changeset with no job, not open, of a done job, or with a newer revision is a 409, and nothing is written. |
| `GET /api/changesets/{id}/revisions` | `{"revisions": [{"revision", "why", "submitted"}]}`: the reason of each submitted revision of the changeset, oldest first (spec 2 section 5.6). A draft revision is not in the list. An unknown changeset is a 404. |
| `GET /api/changesets/{id}/comments` | `{"comments": [...]}`: each comment of the changeset, oldest first, in the form of the `POST` answer. |

The changeset must be open for each decision. Each action of a decision must wait for a person (`proposed` or `in_review`). The
decisions write in one transaction. If one is refused (a hash that changed,
an action that has a decision, a new revision that is a draft, or an action
that is not in the changeset), nothing is written and the answer is a 409.

## Ledger

Each change that Iris made, and its undo (card R2-07, specification 2,
section 7).

| Endpoint | Does |
|---|---|
| `GET /api/ledger?limit=&after=` | `{"entries": [...], "next"}`: each ledger entry, the newest first. `limit` is 1 to 200, default 50. `after` is the `next` value of the page before. An `after` that is not such a value is a 400. |
| `POST /api/ledger/{id}/undo` | No body. The person undoes the entry `id` (`l-12`): the engine makes an approved changeset of the person with the inverse action (section 7.4), and applies it at its next step. The answer is the entry after the start, with `undo_open` true. An entry with no undo now (an open undo, the state `writing` or `undone`, the undo `none`, or no changed message that is not gone) is a 409 with the reason, and nothing is written. An ID that no entry has, or that is not in the form `l-<n>`, is a 404. |

Each entry is `{"id", "verb", "args", "state", "at", "error", "messages",
"changed", "items", "title", "changeset", "author", "decided_by", "rule",
"policy", "reason", "why", "can_undo", "undo_open", "undone_by", "undo_of",
"skipped", "tried", "retry", "mail"}`:

- `args` holds only the short arguments that the words of a verb need: `label`, `until`, `date` and `field`. Each other argument (for example the body of a draft) stays out.
- Each text has at most 2,000 characters, so a page stays small.
- `state` is `writing`, `applied`, `failed` or `undone`. `error` is the reason of a failure.
- `messages` counts the messages of the entry. `changed` counts the messages that Gmail confirmed. A `failed` entry can have changed some messages.
- `items` counts the items of a state verb (`close`, `decline`, `defer`, `correct`). `title` is the title of the first item.
- `author` is `{"kind", "name", "job", "goal"}`: `goal` is the goal of the job of an agent.
- `decided_by` is `rule` (with `rule` and `policy`) or `person`. `reason` is the reason of the decision, and `why` is the reason of the author.
- `can_undo` is true when an undo can start now (the view `ledger_can_undo`). `undo_open` is true while an undo waits or applies. `undone_by` is the entry of the undo. `undo_of` is the entry that this entry undoes.
- `mail` is `{"subject", "to", "recipients"}` for a `send` or a `draft` entry (issue 490): the subject of the draft (at most 200 characters), its first `to` address, and the count of all its recipients (to, cc and bcc). The words are the mail as it was at that entry: a create or a change of a draft gives the content that it wrote (its `post`). A send gives the content with the hash that it sent: the post of a ledger row with that hash, or else the stored bytes of its draft, only while they have that hash. A later change of the draft never changes the words of an older entry. `mail` is null for each other entry, and for a mail whose content Iris cannot read.
- `tried` counts the targets that an undo entry tries to restore: each message that the undone entry changed (also a message that is gone now) and each item of a state entry. Step 2 of the undo of `trash` (`archive`) tries only the messages that had no `INBOX` before the trash. `skipped` counts the targets of `tried` that an applied undo entry left as they were, because an earlier undo restored them, they changed again after the undone entry, or they are gone. `retry` is true when an undo of the undone entry can start now. For each other entry, `tried` and `skipped` are 0 and `retry` is false. An undo that changed nothing keeps the undone entry open (D18).

## Measures

The measures of the manifesto, "How we measure Iris" (card R5-12). The
engine computes them from the store. Nothing leaves the machine.

| Endpoint | Does |
|---|---|
| `GET /api/measures` | `{"days", "first_brief", "mirror", "memory", "approvals", "undo", "sends"}`. See below. |

- `days`: the newest 28 days of use, the newest first, each
  `{"day", "opens"}`. `opens` counts the different source mails that the
  person opened on that day. A day with no entry is a day with no use.
- `first_brief`: `{"connect", "brief", "seconds"}`, or `null` before the
  first brief with content. `connect` is the time of the first account (the
  connect step). `seconds` is the time between the two, rounded up to whole
  seconds. `brief` is the time of the first brief with content: an
  item of a section, a record or a bundle. The engine checks the brief every
  10 seconds in `iris serve`, after `iris mirror` and `iris understand`, and
  at each `/api/brief`. The store
  writes each time once (migration 27). A store from before migration
  27 that has an account has no record of the two times: each field is
  `null`, and the measure stays not known.
- `mirror`: the newest mirror check (`iris mirror verify`) of each account,
  each `{"account", "checked", "differences", "complete", "skipped",
  "stopped"}`. An account with no check has `checked`, `differences` and
  `skipped` `null`, and `complete` and `stopped` `false`.
  `differences` and `skipped` are `null` if the log has no count. `skipped`
  counts the messages that changed during the check: the check does not
  compare them. `stopped` is `true` when an error stopped the check.
  `complete` is `false` when an error stopped the check, or when the check
  skipped a message.
- `memory`: `{"rss_kib", "messages"}`. `rss_kib` is the resident memory of
  the engine from `ps`, or `null`.
- `approvals`: `{"decided", "approved"}`, the decisions of the person on the
  actions of an agent or of triage. The state verbs of the person are not in
  the count.
- `undo`: `{"applied", "undone"}`. `applied` counts the applied actions, and
  not an undo. `undone` counts the actions of them that an undo reversed.
- `sends`: `{"sent", "unreviewed_after_hostile"}` (issue 472). `sent` counts
  the applied sends with a hostile record. `unreviewed_after_hostile` counts the applied sends
  that no person decided and whose draft thread had hostile evidence at the
  claim of the send (`ledger.hostile_before`, migration 59). A send from
  before migration 59 has no record and counts in neither field (issue
  1059). The target is zero.

Each request of the web client while the person is active (an input of the
person in the last 5 minutes) has the header `x-iris-day: YYYY-MM-DD`, the
local date of the person. A poll of a tab that nobody uses has no header.
The API counts the measures from it:

- The first request of a day counts a use of that day, before the request
  runs. So a read of the measures already has the day.
- `GET /api/messages/{id}` (the mail view) with success counts an open of
  that mail on that day. A mail counts once for each day, so a repeated
  request changes nothing.

The day must be the date of the engine in UTC, or the day before or after
it. A request with no header, or with another day, counts nothing and runs
as usual.

## State verbs

The person closes, declines or defers one commitment (specification 2,
section 4). The engine makes a changeset with the author `person` and one
action, submits and approves it in one transaction, and applies it in the
same request.

| Endpoint | Does |
|---|---|
| `POST /api/items/{id}/close` | Body `{}` or `{"reason"}`. Sets the status `done`. The default reason is "Done.". |
| `POST /api/items/{id}/decline` | Body `{}` or `{"reason"}`. Sets the status `declined`. The default reason is "Declined.". |
| `POST /api/items/{id}/defer` | Body `{"date": "YYYY-MM-DD"}`, with an optional `reason`. Sets the status `deferred` and `deferred_until`. The commitment opens again at the start of that date in the time zone of the owner (`iris zone`; the default is the time zone of the computer). |

A `reason` is 1 to 500 characters and not blank. The answer is the item
after the change. The verb changes only a commitment with the status `open`
or `deferred`. For another commitment, the answer is a 409 with the reason,
and the ledger keeps a `failed` entry. For an item of another form (for
example `POST /api/items/r-1/close`), the answer is a 400, and nothing is
written: no changeset and no ledger entry. A body that is not valid (for example, a date
that is not valid) is a 400, and nothing is written. An ID that no item has,
or another verb, is a 404.

## Outbox

The outbox holds each send that a person approved and that waits for its
hold or for the send switch (spec `docs/specs/2026-11-17-held-outbox.md`,
sections 5 and 6). The API reads the switch. It cannot change it: only
`iris sending on|off` at the terminal changes it.

| Endpoint | Does |
|---|---|
| `GET /api/outbox?limit=&after=` | Gives `{"sending": {"on", "since"}, "count", "sends", "next"}`. `count` is the number of all sends that wait. `sends` is one page in the order of the action ID: `limit` 1 to 200 (default 200). `next` is the `after` of the next page, or `null` on the last page. Each send has `draft` (`d-N`), `state` (`approved` or `held`), `approved`, `leaves` (`null` while sending is off), `flags`, and `to`, `cc`, `bcc`, `subject` and `attachments` (`name`, `type`, `size`) from the stored raw bytes of the draft. `bcc_check` is the provider (`gmail` or `outlook`) whose closed Bcc gate keeps the send held (spec `docs/specs/2026-11-24-send-content.md`, section 6.2), or `null`. A draft that Iris cannot read has `"readable": false` and no recipients. One answer is one snapshot of the store. |
| `POST /api/outbox/{d-N}/cancel` | No body. Cancels the open send of the draft: the send becomes `denied` by `person` with the reason "Cancelled in the outbox.". The answer is `{"cancelled": "d-N"}`. If the release claimed the send, the answer is a 409 "The send started. It cannot be cancelled.", and nothing changes. A draft with no send that waits, or an ID that is not a draft, is a 404. |

## Write

The write view of the web client (spec `docs/specs/2026-11-17-write.md`).
One press of Send writes a new mail or a reply. The engine makes the draft,
then the send. The send leaves after the hold, and only while sending is on.

| Endpoint | Does |
|---|---|
| `POST /api/write` | The body is `account` (`a-N`, a new mail) or `thread` (`t-N`) with `parent` (`m-N`, a reply), and `to`, `cc`, the optional `bcc` and `attachments` (entries of `POST /api/attachments`, with `name` and `type`), `subject` (a new mail only), `body`, `key` (16 to 64 letters, digits or hyphens) `flags` (the warnings that the person saw), and the optional `draft` (`d-N`, a change of that draft in place: it must be a draft of `account` or `thread`, else a 400; a draft of an account that is not Gmail is ignored, and the press makes a new draft). The request hash includes `draft`. In one transaction: the changeset of the author `person`, the `draft` action, its submit, the approval and the send mark. A new write is a 201 `{"write": "cs-N"}`. A second try with the same key and the same mail is a 200 with the same `cs-N`, and writes nothing. A 409 `{"error", "flags"}` writes nothing: the warnings are not the warnings that the person saw (`flags` holds the new warnings), a new mail came into the thread, or the key belongs to another mail. A body that is not valid (a target that is not an ID of its kind too), or a mail that the submit denies, is a 400. |
| `POST /api/attachments` | The body is the bytes of one file, at most 4 MiB. Writes the file to the attachment folder of the data folder and gives a 201 `{"sha256", "size"}`. A larger body is a 413, an empty body a 400. Nothing deletes a file of the folder (`docs/specs/2026-11-24-send-content.md`, section 2). |
| `GET /api/write/{cs-N}` | The state of the write: `saving`, `waiting` (`draft`, `sending`, `leaves_at`), `review` (`reason`), `sent` or `stopped` (`reason`, `draft`, and with a draft `change`: `update` when the next press can change the draft in place, `new` when it makes a new draft (an account that is not Gmail), or null). A write that got no send in 24 hours after the press is `stopped`: the engine makes no send for it. One answer is one snapshot of the store. A changeset that is not a write is a 404. |
| `GET /api/drafts/{d-N}` | The content of a draft for the form of a change: `draft`, `account`, `thread` (null for a new mail), `to`, `cc`, `bcc`, `subject`, `body` and `attachments`. A draft that does not exist, of an account that Iris removes, or with content that Iris does not write is a 404. |
| `GET /api/threads/{t-N}/draft` | The draft of the thread that the person can use now: `thread`, `based_on` (the newest mail when Iris wrote it), `nudge` (true for a reminder that Iris prepared because a person is late on an ask, card R3-07), `to` (for a nudge, the address of the person that owes, else null), `items` (for a nudge, each late commitment `c-N` that it names, at most 10, else an empty list) and `body`. One nudge goes to one person and names each late commitment of that person in the thread (decision D8). On the brief, a nudge marks only its commitments as `ready`, not the other items of its thread. A draft is stale after a newer mail or a hostile mark. A nudge is also stale when its commitment is done, hidden, changed or not late. A stale draft is a 404. A thread with no draft is a 404. Nothing is sent. |
| `GET /api/threads/{t-N}/reply` | The start values of a reply: `account`, `parent` (`id`, `from`, `date`, and `text` cut to 600 characters), `to`, `cc`, `subject`, `topic` (the subject of the parent), `conversation` and `earlier`. `conversation` is the newest 20 mails of the thread that a reply can answer, oldest first, with the parent last: `id`, `from`, `date`, `to`, `you` (the mail has the label `SENT`), `hostile` (a hostile mark, a link check that waits, or a hostile sender), `text` (cut to 4,000 characters) and `cut`. `earlier` is the count of the older mails. The parent is the newest mail of the thread that is not spam, trash or a draft. If the person wrote it (it has the label `SENT`), the reply goes to its recipients. Otherwise it goes to its sender, also when its `From` is the address of the person. A thread with no mail to reply to is a 409. A thread that does not exist is a 404. |
| `POST /api/threads/{t-N}/draft` | A draft of the reply in the voice of the person (R3-04 intent path, `iris_voice::draft`). Body `{"intent": "...", "parent": "m-N"}`; the intent can be empty, and `parent` is the mail that the form shows (409 before the call if the reply parent is another mail now). One model call through the budget of understanding, and one more call if the answer is not a valid draft (not a refusal). The draft is saved in `reply_draft` and comes back as `body`, with `based_on`, `samples`, `unknown_cost` and `overrun`. It writes nothing to Gmail. 404 for a thread that does not exist; 400 for an intent over 2,000 characters; 409 for a thread with hostile mail, with mail that understanding did not read, or that changed during the call; 503 with no model, no budget, or while an understanding run holds the lock; 502 if the model fails. |

## Stranger gate

The gate is the `strangers` section of the brief, by sender (specification
1, section 6.7). A `hide` or a `bundle` rule takes an item out of the
section, so a rule decides before the gate asks the person.

`GET /api/gate` and `GET /api/gate/declined` give one page of senders:
`{"count", "senders": [...], "next"}`. `count` is the count of all senders of
the list. Each sender is `{"id", "address", "name", "count", "items"}`:
`count` is the count of its items in the list, and `items` holds its newest 3
items. An item has `ready` as in the brief (`["draft"]` when its thread has a
reply draft that the person can use now, issue 478). A gate item never has
`"summary"`. The newest sender comes first, by the time of its newest item: the
`latest.at` of a conversation, else the date of the origin message. `limit`
is 1 to 200, default 50. `after` is the `next` value of the page before of
the same list. Another `after` is a 400.

- `/api/gate`: each sender of the kind `stranger` (or with no kind yet) that
  the person did not decline, with its items of `strangers`. A conversation
  is under each of its strangers.
- `/api/gate/declined`: each declined sender, with its open commitments and
  active conversations that would wait at the gate. A conversation that has
  another stranger waits at the gate under that stranger, not here. These
  items are in no section of the brief.

| Endpoint | Does |
|---|---|
| `POST /api/senders/{id}/accept` | The kind becomes `person`, decided by the person. The engine never changes it. Its items go to their sections. A second accept is no change. |
| `POST /api/senders/{id}/decline` | The sender leaves the gate, and its items leave the brief. The engine keeps the kind, so a declined sender can become `hostile` or known. The decline applies only while the kind is `stranger`. |
| `POST /api/senders/{id}/reopen` | Removes the accept or the decline. The engine decides the kind again from the mail of now. |

There is no body. The answer is the sender row: `{"id", "address", "name",
"kind", "decided_by", "declined"}`. Accept and decline apply only to a
sender whose kind is `stranger` (or no kind yet) and whose mail of now gives
`stranger` too. For another sender, the answer is a 409 and nothing is
written. A `p-` ID that no sender has, or another answer, is a 404.

An accept makes the domain of the sender known, and a reopen of an accept
can make it unknown. So the engine decides the near copies of the domain
again (specification 1, section 6.7). In the same transaction, it deletes
the items from the mail of each sender that becomes `hostile` or stops being
`hostile`, and marks that mail for understanding again. The understanding
worker runs the model later.

## Errors

An error is `{"error": "<reason>"}`:

| Status | Cause |
|---|---|
| 400 | The search text, an ID, a filter or a request body is not valid. |
| 401 | The request has no `Authorization: Bearer` token of the sign-in, or a wrong one. |
| 403 | The `Host` is not the bound address, the `Origin` is another origin, or the sign-in code is not valid, was used or ended. |
| 405 | The code exchange was not a `POST`. |
| 404 | No object has the ID, or the path is not an endpoint. |
| 409 | A gate answer is for a sender that is not at the gate now: its kind is not `stranger`, or a rule took its items out. A policy change cannot apply: the policy or the file changed after the person saw it, the file is not valid, or the rule does not exist. Read the policy again. A review decision cannot apply: read the changeset again. |
| 500 | The store failed. |

## Sign-in

The API follows specification 2, section 8.6. At each start, `Auth::new(addr)`
makes a random web secret of 32 bytes and a one-time code. `iris serve`
prints the sign-in address `http://<addr>/#code=<code>`. `Auth::sign_in()`
makes one more code, for 2 minutes (`CODE_LIFE`). At most 8 codes wait
(`CODES_MAX`): a new code ends the oldest code of `sign_in`. `iris open`
gets such a code through the engine socket.

1. The client page reads the code from the fragment of the address. The
   browser does not send a fragment to a server.
2. The client sends `POST /api/session` with `{"code": "<code>"}`. The answer
   is `{"token": "<secret>"}`, one time only. A wrong, used or ended code gives 403.
3. The client keeps the token in the storage of its origin (for example
   `sessionStorage`), and sends `Authorization: Bearer <token>` with each
   request. A request without it gives 401.

The API does not use a cookie. A browser sends a cookie to each port of a host,
so a server of another OS user on another loopback port could get it. Storage
belongs to one origin, with its port. A download of `/raw` needs the header
too: fetch it, then save the blob. For a new code, run `iris open`.

## Bound address only

The API refuses each request whose `Host` is not the bound address (for
`127.0.0.1:4800`: `127.0.0.1:4800` or `localhost:4800`), and each request
with an `Origin` that is not `http://` and the same `Host`. So a web page on
another site cannot read the API through DNS rebinding. A request with no
`Origin` header is allowed. Each answer, also a refusal, has
`X-Content-Type-Options: nosniff` and `Cache-Control: no-store`.

A dev server of the client must forward `/api` with the `Host` and the
`Origin` of the engine (in Vite: `changeOrigin: true`, and set `Origin` to the
target in `headers`).

## Ask

A question of the person about their mail (card R5-09). The engine searches
the mail for the words of the question, gives the best 12 messages to the
model as data, and answers with the messages that the answer comes from.

| Endpoint | Does |
|---|---|
| `POST /api/ask` | Body `{"question"}`, 1 to 500 characters. The answer is `{"answer", "sources", "read", "cost_micro_usd"}`. |

| Field | Content |
|---|---|
| `answer` | The answer, or `null` when the mail does not answer the question. The model gives the answer as claims, each with the numbers of its messages. A claim is one fact, in one or more sentences. One or more marks `[n]` come before the end mark of its last sentence, and name the sources of all of its text. The engine drops a claim with a number that names no message of the data. The text of a claim has no other mark. |
| `sources` | `{"n", "quote", "message"}` for each source, in the order of `n` from 1. `message` is a message summary. `quote` is words of the message that support the answer, or `null`. The engine checks that the message has the words. |
| `read` | The count of messages that the model read. 0 means that no message matched, and no model call went out. |
| `cost_micro_usd` | The cost of the call, or `null` with no call or for a local model. |

The search leaves out gone mail, hostile mail, the mail of a hostile sender,
SPAM and TRASH. After the call, the engine checks each source again: against
these rules, and for a change of the date, sender, subject or text that the
model saw. A claim with a source that fails the check goes. The call
takes from the one model budget (see
`docs/specs/2026-11-16-cost-control.md`) and holds the run lock. With no
model, with a budget that cannot pay the worst case of the call, or while a
run of understanding holds the lock, the answer is a 503. A failed
model call, or a model answer that is not valid, is a 502. The text of the
answer is data: show it as text, never as markup.

## Watches

A watch is a search text (specification 1, section 5.6) that gives one notice
when a new mail matches (card R5-07, `docs/specs/2026-11-16-watch.md`).

| Route | Effect |
|---|---|
| `POST /api/watches` | Body `{"text"}`. Adds a watch and gives it with the status 201. An empty text, a text over 500 characters, a text that is not a valid search, or a 101st watch that waits is a 400. |
| `POST /api/watches/{id}/remove` | Removes the watch (`w-3`) and gives `{"removed"}`. A watch that is not there is a 404. An ID that is not a `w-` ID is a 400. |
| `POST /api/notices/{id}/done` | Marks the notice (`n-3`) done and gives `{"done"}`. A notice that is done already gives 200. A notice that is not there is a 404. An ID that is not an `n-` ID is a 400. |

Only a message that the mirror gets after the watch can match. It must also
have its clean text, no hostile mark, no hostile sender, no label `SENT` or
`DRAFT`, and a date at most 1 hour before the watch. The engine checks each
minute. Each watch matches one time only.
