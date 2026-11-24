# Spec: matters that the person edits, and proposals of additions

- Date: 2026-11-24
- Card: none (package P8 of the product decisions of 2026-11-24: D19, D20,
  D21, D22)
- Status: Draft
- Changes: `docs/specs/2026-11-16-matters.md` (card R6-01)

## Problem

Card R6-01 made matters: Iris proposes a matter, and the person confirms or
declines it. Five limits of that card stay open:

| Issue | Limit | Decision of dk (2026-11-24) |
|---|---|---|
| #284 | After the confirm, the person cannot rename a matter, add or remove an item, or delete the matter. | D19: build each one. |
| #286 | The person cannot make a matter by hand. | D19: build it. |
| #285 | A new item never joins a confirmed matter. | D20: Iris proposes the addition, and the person confirms it. |
| #287 | Feed items and signals are never members. | D21: signals can be members. Feeds cannot. |
| #288 | Agents read matters only. | D22: an agent can propose a matter. Only the person confirms. |

A mistake at the confirm cannot be fixed now, and a trip misses its new
booking. That breaks trust in matters.

## Goal

1. The person renames a confirmed matter, adds an item, removes an item,
   and deletes the matter.
2. The person makes a matter by hand from one or more items.
3. Iris proposes to add a new item to a confirmed matter. The person adds
   it or says no. A "no" does not come back for the same item.
4. A signal can be a member of a matter, and a candidate of the proposal
   step. A feed item cannot.
5. An agent proposes a matter with the `iris` command, the MCP server or
   a job session. The proposal waits for the person, as a proposal of the
   model does.
6. Each rule of the store still holds by a trigger, so no write path can
   break it.

## Not in scope

- A merge of two matters. The person can move the items of one matter to
  the other (remove, then add) and delete the empty one. Limit issue.
- An agent proposal of an addition to a confirmed matter. An agent
  proposes a new matter only. Limit issue.
- Policy rules that read the matter of an item (spec 2 section 6). This
  part of #288 stays a limit; D22 asks for agent proposals only. Limit
  issue.
- Feed items as members (D21).

## Design

### Terms

The terms of `docs/specs/2026-11-16-matters.md` apply. New terms:

| Term | Meaning |
|---|---|
| Author | Who made the matter: `model` (the proposal step), `agent` (the socket verb `matters.propose`) or `person` (the web client). |
| Addition | A proposal of the model to add one item to one confirmed matter. |
| Refusal | A member row with `kept` = 0 of a confirmed matter: the person removed the item, did not keep it at the confirm, or said no to its addition. |
| Delete | The person ends a confirmed matter. The store keeps it as `declined`. |

### Member forms (D21)

The member forms are `commitment`, `conversation`, `record` and `signal`.
The ID prefix of a signal member is `s-`. The view `matter_live`, the
eligibility of step 1 of the proposal step, the member insert trigger and
the local API use the four forms. A live signal has the status `active`
only. A signal that is `used`, `dismissed`, `expired` or `gone` stops to
matter (spec 1, section 6.6), so it is not a candidate, not an addition and
not a live member. The engine sets `expired` at `expires`, so the views need
no clock. The other forms keep the live rule of R6-01: status not `gone`,
`cancelled` or `declined`, no `duplicate_of`, attention not `hidden`, and no
hostile origin mail.

A signal member of a confirmed matter that stops to be `active` stays a
kept member, as an ended commitment does. The matter view shows it with
its status. The person can remove it.

The data block line of a signal has the fields of every line (ID, form,
title, thread, date, names). The prompt version is `matters/0.2`, so the
fingerprint changes and the next step makes a new call.

### Store: migration 46

The migration number is the next free number of `develop` at the build.
This spec calls it 46.

```sql
ALTER TABLE matter ADD COLUMN author TEXT NOT NULL DEFAULT 'model'
  CHECK (author IN ('model', 'agent', 'person'));

-- One pending addition for each item: Iris proposes an item to one
-- confirmed matter at most.
CREATE TABLE matter_add (
  item_id INTEGER PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
  matter_id INTEGER NOT NULL REFERENCES matter(id) ON DELETE CASCADE,
  run_id INTEGER REFERENCES understand_run(id),
  created_at INTEGER NOT NULL
);
CREATE INDEX matter_add_matter ON matter_add(matter_id);
```

A matter of the author `agent` or `person` has `model` = '' and
`prompt` = ''.

The migration drops and makes again each trigger and view of migration 16
and 44 that it changes. The rules after the migration:

1. **Status.** A matter starts as an unsealed proposal (as now). The
   status moves from `proposed` to `confirmed` or `declined`, and from
   `confirmed` to `declined` (the delete). No other move. Only a
   `proposed` matter, or a `confirmed` matter with fewer than 2 member
   rows, can be deleted as a row.
2. **Decision.** A decision on a proposal of the author `model` or
   `agent` needs a valid proposal (`matter_valid`), as now. A confirm
   of the author `person` (a matter by hand) needs no `matter_valid`: it
   needs the seal and a `keep` of 1 or more distinct live members.
3. **Changes after the confirm.** A confirmed matter changes only its
   `title` and `updated_at`, or moves to `declined` with `decided_at` and
   `updated_at` set to the time of the delete and `keep` set to NULL (the
   CHECK of migration 16). A declined matter does not change. `id`,
   `author`, `model`, `prompt`, `run_id` and `created_at` never change.
4. **Members of a proposal.** As now: members go in only while the
   proposal is unsealed, with `kept` = 1.
5. **Members of a confirmed matter.** A new member row can go in with
   `kept` = 1 (an add) or `kept` = 0 (a refusal). `kept` can change from 1
   to 0 (a remove, and the confirm of migration 16) and from 0 to 1 (an
   add of an item that the person refused before). A confirmed matter
   keeps 1 to 50 kept members: a change that leaves 0 kept members, or
   gives 51, is refused. A declined matter gets no new member row and no
   change of `kept`.
6. **One open matter for each item.** An item is a kept member of at most
   one matter that is `proposed` or `confirmed` (rule 2 of migration 16),
   for each insert and each change of `kept` to 1.
7. **Forms.** Each member and each addition is an item of a member form.
8. **Member rows go only with a cascade** (as now). A remove is `kept` =
   0, not a delete. A proposal that loses a member is deleted whole (as
   now).
9. **Additions.** A `matter_add` row goes in only for a `confirmed`
   matter, for an item that has no member row in that matter and is not
   a kept member of an open matter. When a member row goes in for the item
   of an addition (an add, or a refusal of the same matter), or the item
   becomes a kept member of any matter, a trigger deletes its `matter_add`
   row in the same statement. So a refused addition never stays visible. When a matter
   leaves `confirmed`, a trigger deletes its `matter_add` rows. A
   `matter_add` row does not change.

The view `item_pin` does not change: it pins each kept member of a
confirmed matter and each member of a declined matter. So a delete pins
each member of the matter, as a decline does, and a rebuild keeps their
IDs. The proposal step never proposes the same grouping or a subset of it
again (check of step 5, "a subset of a declined matter").

A new view `matter_add_live` gives each addition whose item is live (the
eligibility of `matter_live` without `kept`) and whose matter is
confirmed. The API reads additions only through it. Step 0 of the
proposal step deletes each `matter_add` row that is not in it.

### The writes of the person (D19)

Each write is one transaction in `iris_matters`. Each write checks its
rules in Rust first, for a clear error, and the triggers hold the same
rules for each other path.

| Function | Effect | Errors |
|---|---|---|
| `create(store, title, items, now)` | Inserts a proposal of the author `person`, inserts the members, then seals and confirms it in one `UPDATE` with `keep` = all items. Gives the new matter ID. | 400: a bad title, 0 or more than 50 items, an item two times. 409: an item that is not live, or that is a kept member of an open matter. |
| `edit(store, matter, edit, now)` | `edit` has an optional `title`, a list `add` and a list `remove`. The title changes. Each `add` item becomes a kept member: a new row, or `kept` from 0 to 1. Each `remove` item gets `kept` = 0: a kept member, or a pending addition (a new refusal row). `updated_at` = `now`. | 400: a bad title, an item in `add` and `remove`, an item two times, an empty edit. 404: no such matter. 409: the matter is not `confirmed`, an `add` item that is not live or is a kept member of an open matter, a `remove` item that is not a kept member or a pending addition, 0 or more than 50 kept members after the edit. |
| `delete(store, matter, now)` | A confirmed matter with 2 or more member rows (kept or not) moves to `declined`. A confirmed matter with 1 member row is deleted as a row: a grouping of one item needs no decline memory (the model never proposes one item), and a `declined` document needs 2 or more items (the schema). | 404: no such matter. 409: the matter is not `confirmed`. |

An item to add by hand needs no 90-day window: the window only limits
the candidates of the model. A kept member of a proposal cannot be added
by hand (409). The person declines the proposal first.

### Additions of the proposal step (D20)

Step 3 of the proposal step writes, before the candidate lines, one line
for each confirmed matter, newest `updated_at` first, at most 20:

```json
{"matter": "g-3", "title": "Tokyo trip", "members": ["Flight to Tokyo", "Hotel Shinjuku"]}
```

`members` holds the titles of at most 5 kept members, each cut to 60
characters, newest first. The matter lines count toward the same request
bound of 64,000 bytes. A matter line that does not fit stops the matter
lines; the candidate lines follow.

The instruction asks for one more key in the answer:

```json
{"matters": [...], "additions": [{"matter": "g-3", "items": ["s-9"]}]}
```

The checks of step 5 apply to each new matter as now, and to each
addition:

- `matter` is the ID of a matter that the call sent;
- each item ID is a candidate that the call sent, with the prefix of its
  form;
- no item is two times in the answer: not in two additions, and not in an
  addition and a new matter;
- the item has no member row in that matter (the person removed or
  refused it before);
- 1 to 50 items, and the kept members and the additions of the matter
  stay at 50 or fewer;
- at most 10 additions count.

A failed check drops that addition only. The write of step 6 reads the
state again in its transaction: the matter is still `confirmed` and each
item is still a candidate. It then inserts one `matter_add` row for each
item. `iris matters propose` also prints the number of additions.

The candidates of step 1 leave out each item with a `matter_add` row. So
an item waits for the person in one place only.

The fingerprint of step 2 also holds the sorted pairs of matter ID and
`updated_at` of the sent matters. An edit of a matter changes it.

Step 2 stops with no call when no candidate was sent, or when the
candidates come from fewer than 2 threads and no matter was sent.

### Agent proposals (D22)

A new socket verb `matters.propose`, with the arguments `title` and
`items` (a list of IDs). The command `iris matters propose --title
"Tokyo trip" c-1 r-4` and the MCP tool `matters_propose` send it. With
no `--title` and no items, `iris matters propose` runs the proposal step
of the model, as now.

The verb checks the same rules as a matter of the model answer (step 5),
with these changes:

- each item must be live now (the eligibility of step 1 without the
  90-day window), and not a kept member of an open matter or an item
  with a pending addition;
- the items come from 2 or more threads, 2 to 50 items;
- the set is not a subset of a declined matter.

It writes a sealed proposal of the author `agent`, with no run row and no
model call. The answer is `{"matter": <the matter document>}`, with no
member document: a member document can hold model text from mail outside
the grant of a session. A refusal gives exit code
2 (usage) with the reason, and writes nothing.

In a job session, each item must also be in the grant of the job now
(`iris_workspace::grant_in`). The check and the write are one
transaction. An item outside the grant gives the answer "`<id>` is not in
this job", the same as `show`, so a session learns nothing of the mail
outside its grant.

Only the web client of the person confirms or declines a proposal, as
now. A proposal of an agent shows in the brief as a proposal of the
model does.

### The open model: version 1.2

The matter document gets these changes. Each one is allowed in a minor
version (specification 4, rule 2.2):

| Change | Since |
|---|---|
| The `s-` prefix in `items`. | 1.2 |
| The optional field `counts.signal`. | 1.2 |
| `derived.by` is `model`, `agent` or `person`. `model` and `prompt` are present only for `model`. | 1.2 |
| The optional field `additions`: the item IDs of the live additions of a confirmed matter, 1 to 50. | 1.2 |

A document that claims `iris/1.0` or `iris/1.1` and holds `counts.signal`
or `additions` is not valid (rule 2.8). `iris_store::MODEL` becomes
`iris/1.2`, and `docs/spec/VERSION` becomes `1.2`.

### The local API

| Route | Body | Effect |
|---|---|---|
| `POST /api/matters` | `{"title": "...", "items": ["c-1", ...]}` | `create`. 201 with the matter view. |
| `POST /api/matters/{id}/edit` | `{"title"?: "...", "add"?: [...], "remove"?: [...]}` | `edit`. The matter view. |
| `POST /api/matters/{id}/delete` | none | `delete`. 204 with no body: the matter is no longer in the list. |

The matter view (`GET /api/matters/{id}`) gets the key `additions`: the
documents of the live additions of a confirmed matter, newest first. The
list (`GET /api/matters`) puts `additions` in the document of each
confirmed matter that has one.

The routes refuse requests that are not local, as each route does now.
Only the web client of the person calls them. The `iris` command has no
verb for `create`, `edit` or `delete`: an agent proposes, the person
decides.

### Screens

- The matter view of a confirmed matter: Rename, Remove on each member,
  Delete matter (with a confirm step: Delete, or Keep; there is no Undo
  after the delete, because a declined matter does not change), and a part "Iris thinks these belong here"
  with Add and Not this for each addition.
- The brief: a confirmed matter with additions shows one line, "Add 1 item
  to Tokyo trip?", that opens the matter view.
- The item view: "Add to a matter", with a list of the confirmed matters
  and "New matter". A new matter asks for a title.
- A signal member shows in a part "Signals" of the matter view.

The layout goes to the owner as a before/after page before the client PR
merges.

## Options not chosen

- A third value of `kept` for a pending addition. The CHECK of
  `matter_item` would need a table rebuild, and a pending addition is not
  a member: the views and the pin would each need one more condition.
- A hard delete of a confirmed matter. The members would lose their pin,
  and the model would propose the same grouping again after the delete.
- One route for each edit (rename, add, remove). One route with one
  transaction is simpler for the client and keeps a move of items atomic.
- Agent proposals through the changeset of spec 2 (the reason of the
  matters spec still holds: a matter needs the person, never a policy).
- A `person` proposal that waits for a confirm. The person makes the
  matter, so it is confirmed at once.

## Failure list

Input that the product does not control: the model answer, the request
of an agent, and the request body of the web client.

| Input | Result | Test |
|---|---|---|
| An addition to a matter that the call did not send, or that is not confirmed | Dropped. | unit |
| An addition of an item that was not sent, or with the prefix of another form | Dropped. | property test: random answers never write a `matter_add` row outside the sent set |
| The same item in a new matter and an addition, or in two additions | The later one is dropped. | same property test |
| An addition of an item that the person removed or refused | Dropped. | unit |
| An addition that gives more than 50 | Dropped. | unit |
| The matter is deleted or the item becomes a member during the call | Dropped in the write transaction. | unit |
| An agent proposal with 1 item, items of one thread, an unknown ID, an ID of another form, a feed, an item that is not live or is in an open matter | Refused, exit code 2, no write. | unit for each |
| An agent proposal that is a subset of a declined or deleted matter | Refused. | unit |
| A session proposal with an item outside the grant | Refused as "not in this job". | unit |
| Two edits of one matter at the same time | Each is one transaction: the store serializes the writes. The second sees the first. | unit: an edit after a delete gives 409 |
| An edit that removes the last kept member | 409, no change. The trigger refuses it too. | unit and a trigger test with direct SQL |
| A direct SQL write that breaks a rule of the migration | Refused by a trigger. | a test of each trigger |
| An item to add that is hostile mail, hidden, gone or a duplicate | 409. | unit |
| The delete of a matter while an addition waits | The trigger deletes the addition. | unit |
| The person refuses an addition | The refusal row goes in and the trigger deletes the `matter_add` row in the same statement. The addition is not in the list. | unit |
| The delete of a matter with 1 member row | The row goes, with its member row; the item is a candidate again. | unit |
| A signal that expires, is used or is dismissed | Not a candidate, not an addition, not live. A kept signal member of a confirmed matter stays. | unit |
| A rebuild of the item of an addition | The cascade deletes the addition; the next step can propose the new item. | unit |
| A rebuild of the item of a refusal (`kept` = 0) | The cascade deletes the refusal row: the model can propose the new item again. Same limit as a member that the person did not keep at the confirm (R6-01). | none (known limit) |

All other input is refused with no write.

## Impact

- `docs/specs/2026-11-16-matters.md`: "Not in scope" points to this spec.
- `docs/spec/schema/matter.schema.json`, `docs/spec/VERSION`,
  `docs/spec/04-open-model-versions.md`, `docs/spec/README.md`, a sample.
- `crates/iris-store`: migration 46, `MODEL`.
- `crates/iris-matters`: `create`, `edit`, `delete`, `propose_by_agent`,
  the signal form, the additions of the proposal step.
- `crates/iris`: the socket verb `matters.propose`, the command.
- `crates/iris-mcp`: the tool `matters_propose`.
- `crates/iris-api`: three routes, `additions`, the signal form.
- `client/`: the matter view, the brief line, the item view.
- Memory: the matter lines add at most 20 short lines to the request
  body, inside the same 64,000-byte bound. No other change.

## Assumptions

| Assumption | Evidence |
|---|---|
| SQLite can add a column with a CHECK and a NOT NULL default. | SQLite `ALTER TABLE ADD COLUMN` documentation: a CHECK is allowed, and a NOT NULL column needs a default that is not NULL. |
| `UPDATE OF status` triggers do not run for an `UPDATE` that sets only `title`. | SQLite `CREATE TRIGGER` documentation: `UPDATE OF column-list` runs only when a listed column is in the SET list. |
| A job grant holds commitments and records only, so a session proposes matters of these forms. | `crates/iris/src/socket.rs`, the `add` verb comment on `iris_workspace::grant_in`. |
| The model can name additions from member titles. | `pending`. A wrong addition costs the person one "Not this". The quality test of #290 can add a case. |

## External writes

None. The model call reads. The agent verb writes only to the store.

## Open questions

- The exact prompt text of `matters/0.2`.
- The limit of 20 matter lines and 5 member titles. The build can change
  them inside the request bound.

## Acceptance criteria

1. The person renames a confirmed matter, removes a member and adds an
   item. `GET /api/items/{id}` of the removed item has `matter` = null,
   and of the added item the `g-` ID. Red: the old value stays.
2. The person makes a matter by hand with 1 item. It is confirmed at
   once. Red: it is a proposal, or the create fails.
3. A delete moves the matter to `declined`. The list leaves it out. The
   step does not propose the same members again. Red: the matter shows,
   or a proposal with the same members comes back.
4. With a fake model that names an addition of a new signal to a
   confirmed matter, a `matter_add` row is written, the matter view shows
   it, and an edit with `add` makes it a kept member. Red: no row, or the
   item is a member before the person adds it.
5. A refused addition does not come back from the same answer. Red: a
   second `matter_add` row.
6. `iris matters propose --title T c-1 r-2` writes a proposal of the
   author `agent` that the brief shows, and a feed ID is refused. Red: a
   confirmed matter, or a feed member.
7. Each trigger refuses a direct SQL write that breaks its rule. Red: the
   write passes.

## Verify

```bash
cargo test -p iris-matters -p iris-store -p iris-api -p iris-mcp -p iris
python3 docs/spec/conformance/run.py self-test
scripts/local-ci/run.sh
```

## Rollout

Migration 46 runs at the first start, after the backup of spec 1 section
5.5. No flag. Existing matters get the author `model`. The new prompt
version gives one new proposal call at the next step.

## Risks

- A pinned member of a deleted matter stays in the store. The cost is the
  same as a decline of R6-01.
- An agent can make many proposals. Each item is in one open matter at
  most, so the number of open proposals is at most half the number of
  live items, and the person declines them with one click each.
- Rollback: the migration goes forward only. A later migration can drop
  `matter_add` and ignore `author`.

## Plan

1. This spec.
2. Store and writes: migration 46, `create`, `edit`, `delete`,
   `propose_by_agent`, signal members (D19, D21). The proposal step leaves
   out signal candidates until step 5.
3. API routes, open model 1.2, the socket verb, the command and the MCP
   tool (D19, D20, D22). A request does not accept an `s-` ID until step
   5, because the web client shows no signal member before it (#884).
4. Proposal step: additions (D20).
5. Client screens, after the owner picks a layout. This step accepts `s-`
   IDs in requests and signal candidates in the proposal step (D21).

## Spec checks

- [x] Each assumption holds with a second caller: each write is one
  transaction, and the triggers hold each rule for each path.
- [x] Each assumption holds when an outcome is unknown: the model call
  keeps the rules of R6-01; the agent verb is one transaction.
- [x] Each dropped mechanism was dropped for a reason (Options not
  chosen).
- [x] No step is check-then-act: the Rust checks give clear errors, and
  the triggers decide in the write.
- [x] Each property that the design depends on has a test.
- [x] The spec addresses the hardest problem: the store rules after the
  confirm, and untrusted additions.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported inputs and the safe result.
