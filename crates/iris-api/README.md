# iris-api

The local API of Iris (tracker item R1-17). The API gives the web client
and the read verbs of the `iris` program their data from the store. The API
is read only.

## Use

```rust
let app = iris_api::router(Arc::new(store)).merge(iris_web::router());
```

Merge the API routes before the fallback of `iris_web::router()`. The read
verbs call the same functions as the routes: `status`, `search`, `message`,
`raw`, `brief`, `items` and `item`. So the verbs and the client get the same
data.

## Endpoints

All endpoints use `GET` and give JSON, except `/raw`. Times are RFC 3339 in
UTC. IDs have the format of specification 1, section 5.7 (`m-42`).

| Endpoint | Gives |
|---|---|
| `/api/status` | `{"accounts": [...], "messages": {"total", "fetched", "understood", "gone"}}`. Each account has `id`, `address`, `provider`, `state`, `mirror_ready_at`, `sync` (`{"kind", "step"}` of an unfinished first sync or resync, or `null`) and `last_sync` (`{"kind", "started", "ended", "result"}` or `null`). With no account, `accounts` is `[]`. |
| `/api/brief` | The first brief of specification 1, section 8. See [Brief](#brief). |
| `/api/items?form=&status=&type=&before=&limit=` | `{"items": [...], "next"}`. Items newest first. `form` is one of the five forms. `type` is a record type, and needs `form=record`. `before` is the `next` value of the page before. `limit` is 1 to 200, default 50. Items with the status `gone` are not in the list, unless `status=gone`. A `status` must be a status of the form (of any form, with no `form`). A `type` must be a record type of the schema. |
| `/api/items/{id}` | One item (`c-`, `v-`, `r-`, `f-` or `s-`), also when it is gone. |
| `/api/search?q=&limit=` | `{"messages": [summary]}`, newest first. `q` is the search text of specification 1, section 5.6. `limit` is 1 to 200, default 50. |
| `/api/messages/{id}` | One message: the summary, and `to`, `cc`, `text` (the clean text, or `null`), `gone`, `hostile` (the hostile marks, for example `["dmarc"]`) and `items` (the IDs of the items that are not gone and that have the message as a source). |
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
| `conversations` | Active conversations with `new` greater than 0 or `your_turn` true, and with no stranger participant. Newest `latest.at` first. |
| `records` | The count of each record type that is not gone, for example `{"receipt": 3}`. |
| `feed` | Feed items with the status `new`, in the order of the publisher address, then newest first. |
| `signals` | Active signals whose `expires` time is later than now. The time that expires first comes first. |
| `strangers` | Open commitments whose counterparty or owner is a stranger, and active conversations with a stranger participant. Newest first. |
| `hostile` | The count of hostile mail of the last 7 days. |
| `not_understood` | The count of messages that the model failed 3 times. |
| `oldest_fetched` | The date of the oldest fetched message that is not gone, or `null`. While the first sync continues, the brief covers only the mail from this date (specification 1, section 4.2). |

Each list field is `{"count", "items"}`. `count` is the full count. `items`
holds at most 200 items. Times sort as instants, not as text, so a time
offset does not change the order. A list field can also have `duplicates`:
for an item in `items`, the IDs of the newer items whose `duplicate_of` is that
item (specification 1, section 7.5). The brief shows such a pair once, so the
newer item is not in `items` and not in `count`. The feed sorts by the sender
row of the publisher, so the items of one publisher stay together. A sender with no sender row, or with no kind yet, is
a stranger.

## Errors

An error is `{"error": "<reason>"}`:

| Status | Cause |
|---|---|
| 400 | The search text, an ID or a filter is not valid. |
| 403 | The `Host` or the `Origin` of the request is not local. |
| 404 | No object has the ID, or the path is not an endpoint. |
| 500 | The store failed. |

## Local requests only

Release 1 has no sign-in on the API. So the API refuses each request whose
`Host` is not `127.0.0.1`, `localhost` or `[::1]` (with an optional port), and
each request with an `Origin` that does not have one of these hosts. A web page
on another site cannot read the API through DNS rebinding. A request with no
`Origin` header is allowed. Each answer, also a refusal, has `X-Content-Type-Options: nosniff`
and `Cache-Control: no-store`.
