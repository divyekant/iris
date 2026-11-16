# iris-api

The local API of Iris (tracker item R1-17). The API gives the web client
and the read verbs of the `iris` program their data from the store. The API
reads only, except the policy changes of the web client (see
[Policy](#policy)), the review of the person: decisions and withdraws
(see [Review](#review)), and the state verbs of the person (see
[State verbs](#state-verbs)).

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
| `/api/status` | `{"mode": "observe", "accounts": [...], "messages": {"total", "fetched", "understood", "gone"}}`. Each account has `id`, `address`, `provider`, `state`, `mirror_ready_at`, `sync` (`{"kind", "step"}` of an unfinished first sync or resync, or `null`) and `last_sync` (`{"kind", "started", "ended", "result"}` or `null`). With no account, `accounts` is `[]`. |
| `/api/brief` | The first brief of specification 1, section 8. See [Brief](#brief). |
| `/api/brief/{section}?limit=&after=` | One page of a list section of the brief (`you_owe`, `owed_to_you`, `conversations`, `feed`, `signals` or `strangers`), of the bundle of rule N (`bundle-N`, newest first), or of the items that a `hide` rule leaves out (`hidden`, newest first; each item has the rule and the reason in `attention`): `{"count", "items", "next"}`, in the same order and with the same filter as the brief. The page has `duplicates` only when an item of the page has a duplicate, as in the brief. `limit` is 1 to 200, default 50. `after` is the `next` value of the page before. With no `after`, the page starts at the first item. `next` is a text value that holds the section name and the sort keys of the last item of the page. Do not make or change it. The API does not sign it, so a made cursor gives the items after its keys. The next page starts after these keys in the order of the section. An item that leaves the section, or that changes its sort key behind these keys, moves no other item. An item whose sort key moves across these keys between two pages can show twice or not show. For a new view, read the section again from the start. An empty `after`, or an `after` that is not a cursor of the same section, is a 400. An unknown section is a 404. `bundle-N` is a 404 when the active policy has no `bundle` rule N and no item of the brief is in its bundle. |
| `/api/handled?limit=&after=` | `{"count", "actions": [...], "next"}`: each action that a rule decided without the person, in its state now (`approved`, `held`, `applied`, `failed`, `undone` or `denied`; not `in_review`, which the review lists), the newest first. A `failed` action can have changed some of its messages. `count` is the count of all such actions. Each action is `{"changeset", "revision", "n", "verb", "args", "messages", "state", "rule", "policy", "why", "reason", "at"}`. `messages` is the count of its messages. `why` is the reason of the author, and `reason` is the reason of the decision. `limit` is 1 to 200, default 50. `after` is the `next` value of the page before. An `after` that is not such a value is a 400. |
| `/api/changesets` | `{"changesets": [...]}`: each open changeset (specification 2, section 5.1), the newest changeset first. A changeset is open while one or more of its actions is `proposed`, `in_review`, `approved` or `held`. The answer streams in parts of 64 KiB. |
| `/api/changesets/{id}` | One changeset (`cs-77`) in each state, as the document of specification 2, section 5.1. The answer streams. An ID that no changeset has is a 404. |
| `/api/messages?ids=` | `{"messages": [...]}`: the summary of each message of `ids` (`m-1,m-2`, 1 to 200 IDs), in the order of `ids`, with `"gone": false`. A message that is gone, or that is not in the mirror, is `{"id", "gone": true}`. A text that is not a message ID is a 400. The review shows the real messages of each action with it. |
| `/api/items?form=&status=&type=&before=&limit=` | `{"items": [...], "next"}`. Items newest first, by the date of the origin message. `form` is one of the five forms. `type` is a record type, and needs `form=record`. `before` is the `next` value of the page before: an item ID of the form of the filter. `limit` is 1 to 200, default 50. Items with the status `gone` are not in the list, unless `status=gone`. A `status` must be a status of the form (of any form, with no `form`). A `type` must be a record type of the schema. |
| `/api/items/{id}` | One item (`c-`, `v-`, `r-`, `f-` or `s-`), also when it is gone. |
| `/api/search?q=&limit=` | `{"messages": [summary]}`, newest first. `q` is the search text of specification 1, section 5.6. `limit` is 1 to 200, default 50. |
| `/api/messages/{id}` | One message: the summary, and `to`, `cc`, `text` (the clean text, or `null`), `gone`, `hostile` (the hostile marks, for example `["dmarc"]`), `sender_kind` (the kind of the sender row of the `From` address: `person`, `service`, `stranger` or `hostile`; a sender with no row or no kind is a `stranger`; if two rows have the address in a different case, the first kind of `hostile`, `stranger`, `service`, `person` is used, and a row with no kind counts as `stranger`) and `items` (the IDs of the items that are not gone and that have the message as a source). |
| `/api/messages/{id}/raw` | The raw source (RFC 5322) of the message, as a download (`message/rfc822`, `Content-Disposition: attachment`). |

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
| `conversations` | Active conversations with `new` greater than 0 or `your_turn` true, and with no stranger participant. A conversation with a stranger is only in `strangers`. Newest `latest.at` first. |
| `records` | The count of each record type that is not gone, for example `{"receipt": 3}`. |
| `feed` | Feed items with the status `new`, in the order of the publisher address, then newest first. |
| `signals` | Active signals whose `expires` time is later than now. The time that expires first comes first. |
| `strangers` | Open commitments whose counterparty or owner is a stranger, and active conversations with a stranger participant. Newest first. |
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

The attention of an item (specification 2, section 6.4) decides where it
shows. Each item that a list field or `records` would have is in exactly one
place: its list field or `records` when its `attention.state` is `shown` or
missing, the bundle of its rule when it is `bundled`, and only the count
`hidden` when it is `hidden`. The feed sorts by the address of
the sender row of the publisher, so the items of one publisher stay together. A sender with no sender row, or with no kind yet, is
a stranger.

## Policy

The policy view of the web client (specification 2, section 6.3). Each
endpoint gives the policy object below. `POST` endpoints take a JSON body.

| Endpoint | Does |
|---|---|
| `GET /api/policy` | Gives the policy object. Changes nothing. |
| `POST /api/policy/confirm` | Body `{"sha256"}`: the `file.sha256` that the person saw. Makes the file on disk the active policy. |
| `POST /api/policy/discard` | Body `{}`. Keeps the file on disk as a copy, then writes the text of the active policy to `policy.toml`. With no active version, no new file is written. The answer has `saved`: the name of the copy in the data folder (`policy.toml.saved-<seconds>`), or `null` when there was no file. |
| `POST /api/policy/mode` | Body `{"version", "mode"}`. Writes the new mode to the file and makes it active. |
| `POST /api/policy/remove` | Body `{"version", "id"}`. Removes the rule from the file and makes the file active. The ID is never used again. |

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
| `POST /api/changesets/{id}/withdraw` | No body. The person withdraws the changeset, in any state but `withdrawn` (section 5.5: "each changeset"): each open action and each draft action becomes `cancelled`, and applied and decided actions stay. A second withdraw is a 409. The answer is the changeset document after the withdraw. |

The changeset must be open for each decision. Each action of a decision must wait for a person (`proposed` or `in_review`). The
decisions write in one transaction. If one is refused (a hash that changed,
an action that has a decision, a new revision that is a draft, or an action
that is not in the changeset), nothing is written and the answer is a 409.

## State verbs

The person closes, declines or defers one commitment (specification 2,
section 4). The engine makes a changeset with the author `person` and one
action, submits and approves it in one transaction, and applies it in the
same request.

| Endpoint | Does |
|---|---|
| `POST /api/items/{id}/close` | Body `{}` or `{"reason"}`. Sets the status `done`. The default reason is "Done.". |
| `POST /api/items/{id}/decline` | Body `{}` or `{"reason"}`. Sets the status `declined`. The default reason is "Declined.". |
| `POST /api/items/{id}/defer` | Body `{"date": "YYYY-MM-DD"}`, with an optional `reason`. Sets the status `deferred` and `deferred_until`. The commitment opens again on that date (UTC). |

A `reason` is 1 to 500 characters and not blank. The answer is the item
after the change. The verb changes only a commitment with the status `open`
or `deferred`. For another item, the answer is a 409 with the reason, and the
ledger keeps a `failed` entry. A body that is not valid (for example, a date
that is not valid) is a 400, and nothing is written. An ID that no item has,
or another verb, is a 404.

## Errors

An error is `{"error": "<reason>"}`:

| Status | Cause |
|---|---|
| 400 | The search text, an ID, a filter or a request body is not valid. |
| 401 | The request has no `Authorization: Bearer` token of the sign-in, or a wrong one. |
| 403 | The `Host` is not the bound address, the `Origin` is another origin, or the sign-in code is not valid or was used. |
| 405 | The code exchange was not a `POST`. |
| 404 | No object has the ID, or the path is not an endpoint. |
| 409 | A policy change cannot apply: the policy or the file changed after the person saw it, the file is not valid, or the rule does not exist. Read the policy again. A review decision cannot apply: read the changeset again. |
| 500 | The store failed. |

## Sign-in

The API follows specification 2, section 8.6. At each start, `Auth::new(addr)`
makes a random web secret of 32 bytes and a one-time code. `iris serve`
prints (and opens) the sign-in address `http://<addr>/#code=<code>`.

1. The client page reads the code from the fragment of the address. The
   browser does not send a fragment to a server.
2. The client sends `POST /api/session` with `{"code": "<code>"}`. The answer
   is `{"token": "<secret>"}`, one time only. A wrong or used code gives 403.
3. The client keeps the token in the storage of its origin (for example
   `sessionStorage`), and sends `Authorization: Bearer <token>` with each
   request. A request without it gives 401.

The API does not use a cookie. A browser sends a cookie to each port of a host,
so a server of another OS user on another loopback port could get it. Storage
belongs to one origin, with its port. A download of `/raw` needs the header
too: fetch it, then save the blob. For a new code, start `iris serve` again.

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
| `answer` | The answer, or `null` when the mail does not answer the question. Each sentence has one or more marks `[n]`: the sources of its facts. The engine drops a sentence with no mark that names a message of the data. |
| `sources` | `{"n", "quote", "message"}` for each source, in the order of `n` from 1. `message` is a message summary. `quote` is words of the message that support the answer, or `null`. The engine checks that the message has the words. |
| `read` | The count of messages that the model read. 0 means that no message matched, and no model call went out. |
| `cost_micro_usd` | The cost of the call, or `null` with no call or for a local model. |

The search leaves out gone mail, hostile mail, the mail of a hostile sender,
SPAM and TRASH. The call takes from the one model budget (see
`docs/specs/2026-11-16-cost-control.md`). With no model, or with a budget
that cannot pay the worst case of the call, the answer is a 503. A failed
model call, or a model answer that is not valid, is a 502. The text of the
answer is data: show it as text, never as markup.
