# Iris specification 2: changeset, policy, ledger and the iris command

- Status: draft, for the approval of the maintainer
- Applies to: release 2 ("Apply"). Parts for releases 3 to 5 are marked.
- Open model version: 1.2
- Related: [Specification 1](01-mirror-store-forms.md), the manifesto and the
  experience document "Iris: the ideal experience"

## 1. Scope

This specification defines how a change reaches the mailbox, and the command
that people and agents use:

1. The changeset: the format of a proposal and of its results.
2. The policy file: the rules that decide which actions apply without review.
3. The ledger: the record of each applied action, and its undo.
4. The `iris` command: the folder plus command interface. MCP is the second
   interface and uses the same verbs.

This specification defines release 2 in full. The verbs `draft`, `send` and
`nudge` and the held outbox (release 3), Stop and sessions in workspaces
(release 4), and MCP and proposed rules (release 5) are here only as formats
and as constraints (sections 7.5, 8.1 and 8.5). Their full design comes in a
later specification. The code of release 2 has no verb of a later release.

## 2. Terms

| Term | Meaning |
|---|---|
| Action | One change in a changeset, for example "archive thread `t-7712`". |
| Verb | The kind of an action: `archive`, `label`, `send` and the others in section 4. |
| Changeset | A list of actions from one author with one reason. |
| Author | Who proposes a changeset: `person`, `agent`, `triage` (the engine) or `local` (the `iris` command on the host). |
| Review | A person decides each action that the policy does not allow. |
| Policy | The rules in `policy.toml`. |
| Hard limit | A limit in the engine that no rule can change (section 6.5). |
| Ledger | The table of each action that the engine sent to the provider. |
| Undo | The action that returns the mailbox to the state before an action. |
| Host mode | The `iris` command on the machine of the engine, outside a workspace. |
| Session mode | The `iris` command inside a workspace of an agent session (release 4). |

## 3. Requirements

| ID | Requirement | How we check it |
|---|---|---|
| C1 | Each write to the mailbox goes through a changeset. | The provider client has write functions only in the apply module. A test fails if another module calls them. |
| C2 | No agent holds a mailbox credential, and no agent can approve. | The `iris` command has no approve verb. The web API refuses a request without the web secret (section 8.6). Session mode has no path to the token. Section 8.6 gives the trust boundary. |
| C3 | Each action that can be reversed has an undo in the ledger. | For each verb, a test applies the action and the undo at the fake Gmail server, and compares the label sets. |
| C4 | An action that cannot be reversed waits for review or a delay. | `send` always goes through the held outbox (release 3). |
| C5 | A crash cannot apply an action twice or lose the record of it. | A test stops the engine between the provider call and the ledger update, and checks the state after the start. |
| C6 | The policy file is the only source of trust. | A change of the file applies only after the person confirms it in the web client (section 6.3). A test changes the file on disk and checks that no new rule applies before the confirmation. |
| C7 | 100 applied actions are correct in Gmail, and each undo is correct. | The exit criterion of release 2. |

## 4. Verbs

Each verb has a target, a release, an undo and the provider operations. A
thread target means all messages of the thread at the time of submit that do
not have the label `SPAM` or `TRASH`.

| Verb | Target | Arguments | Gmail operation | Undo | Release |
|---|---|---|---|---|---|
| `archive` | threads or messages | none | `messages.batchModify`, remove `INBOX` | add `INBOX` | 2 |
| `inbox` | threads or messages | none | `messages.batchModify`, add `INBOX` | remove `INBOX` | 2 |
| `label` | threads or messages | user label name | `labels.create` if the label is new (section 7.2), then `batchModify`, add the label | remove the label (the label stays) | 2 |
| `unlabel` | threads or messages | user label name | `batchModify`, remove the label | add the label | 2 |
| `read` | threads or messages | none | `batchModify`, remove `UNREAD` | add `UNREAD` | 2 |
| `unread` | threads or messages | none | `batchModify`, add `UNREAD` | remove `UNREAD` | 2 |
| `trash` | threads or messages | none | `messages.trash` for each message | `messages.untrash`, then `archive` if `messages.untrash` added `INBOX` (section 7.4) | 2 |
| `close` | commitment | reason | none (Iris state) | set the old value (below) | 2 |
| `defer` | commitment | date | none (Iris state) | set the old value (below) | 2 |
| `decline` | commitment | reason | none (Iris state) | set the old value (below) | 2 |
| `correct` | any item | field, value | none (Iris state) | set the old value (below) | 2 |
| `skip` | feed item | reason | none (Iris state) | set the old value (below) | 5 |
| `mute` | conversation | reason | none (Iris state) | set the old value (below) | 5 |
| `dismiss` | signal | reason | none (Iris state) | set the old value (below) | 5 |
| `draft` | one thread (a reply) or one account (`a-N`, a new mail) | recipients, subject, text | `drafts.create` (section 7.5) | none: Iris never deletes a draft. The change of a draft and its undo come in a later card | 3 |
| `send` | one draft (`d-N`) | none | `drafts.send`, after the hold (section 7.5) | none: final after the hold | 3 |
| `nudge` | commitment `owed_to_you` | none | makes a `draft` action, then a `send` action | as `draft` and `send` | 3 |
| `wait` | thread, or a date | until | none | not applicable | 5 |
| `ask` | none | question, options | none | not applicable | 5 |

Two verbs come only from an undo (section 7.4), never from an author: `untrash`
(the undo of `trash`) and `restore` (the undo of a state verb). `restore`
takes its ledger entry as the target and no arguments. It changes no mailbox.

Iris has no verb for a permanent delete, and no verb to empty the trash.
Gmail deletes a message in the trash for good after 30 days. After that time,
the undo of `trash` is not possible, and the ledger shows the entry as final:
the view `ledger_can_undo` of migration 23 no longer lists it (section 7.1).

`label` and `unlabel` accept only a label of kind `user` (specification 1,
section 5.5). At submit, the engine refuses a label name that is the same as
the name or the ID of a system label, with no regard to case. A system label (`INBOX`, `UNREAD`, `SPAM`, `TRASH` and the
others) changes only through its own verb, so that no `label` action can go
past a rule or a hard limit that names that verb.

A verb that changes Iris state (`close`, `defer`, `decline`, `correct`,
`skip`, `mute`, `dismiss`) goes
through a changeset too. This gives each change one path, one review and one
ledger. The state verbs set these values:

| Verb | Sets |
|---|---|
| `close` | `status` `done` |
| `decline` | `status` `declined` |
| `defer` | `status` `deferred` and `deferred_until` |
| `skip` | `status` `skipped`, from `new` or `read` |
| `mute` | `status` `muted`, from `active` or `quiet` |
| `dismiss` | `status` `dismissed`, from `active` |
| `correct` | The field of the item. `correct` changes only items. A person changes the kind of a sender only in the web client (specification 1, section 6.7). |

When an action of a state verb applies, the engine writes in one SQLite
transaction:

- A `correction` row with `by` set to the changeset ID (specification 1,
  section 5.5). So the item is pinned, and a rebuild keeps the change.
- A ledger row with the state `applied`, the old values as `pre` and the new
  values as `post` (section 7.1). `pre` and `post` map each item ID to its
  fields, for example `{"c-7": {"status": "open"}}`. For `correct`, the
  fields are the corrected field, its top-level value (for example `due` for
  `due.date`) and `status`: a correction can change the siblings of the
  field, and the undo sets them back.

The engine applies each approved action of a state verb at the next tick,
with no provider call. These rules apply:

- A state verb changes no mailbox. So, in the mode `observe`, an action that
  a person approved applies, as an undo does. An action that a rule approved
  waits in the mode `observe`.
- The engine evaluates an action that a rule approved again before the
  apply, as for a mailbox verb (section 5.4), from the origin messages of
  its items. A state verb has no `bulk`, also not from the count of the
  last hour.
- `close`, `decline` and `defer` change only a commitment with the status
  `open` or `deferred` (specification 1, section 6.8). `close` and `decline`
  remove `deferred_until`. If an item does not agree (for example,
  understanding closed it after the submit), the ledger row is `failed` with
  the reason and the undo `none`, and no item changes.

When the person closes, declines or defers a commitment in the web client,
the engine makes a changeset with the author `person` and one action, as for
an undo (section 7.4). The choice of the person is the approval. The engine
submits and approves the action in one transaction, and then applies it in
the same request. If the submit denies the action (for example, a date that
is not valid), nothing stays. If the engine stops before the apply, the next
tick applies the approved action.

The undo of the action is one action with the verb `restore` (section 7.4).
Its apply sets the old values, sets the sender ID of each party from its
address again, and deletes the correction rows of the entry (their IDs are in
`ops`), in one transaction, with its own ledger row (the undo `none`). If
the undo changed one or more items, the first entry becomes `undone`. If it
changed no item, the first entry keeps its state, and the person can undo it
again later (decision D18, issue #463). The undo changes nothing on an item,
and lists the item, if one of these is true:

- The current value is not the `post` value of the entry.
- The item is gone.
- A later change of Iris on the item stands: an entry after this entry that
  did not fail and names the item, also with the same values. A state verb
  entry stands unless an undo of it restored the item. A `restore` is a
  change only if it restored the item and undoes an entry before this
  entry. So a `restore` that changed no item, or the undo of the later
  entry, does not block a new undo of this entry. After the undo of a `correct` on a record with no
correction left, the status of the record goes back to `kept`.

`correct` cannot change `status`, a common field of section 6.1 of
specification 1, or another field that the engine makes from the store
(specification 1, section 7.5). The engine validates the corrected document against the
schemas at submit, and denies an action that gives a document that is not
valid.

## 5. The changeset

### 5.1 Format

A changeset is one JSON document. This is the format of version 1. The JSON
Schema of the changeset is `schema/changeset.schema.json`, with sample
documents in `schema/examples/` (files `changeset*.json`).

```json
{
  "model": "iris/1.2",
  "id": "cs-77",
  "revision": 1,
  "author": { "kind": "agent", "name": "Claude Code", "session": "se-12", "job": "j-142" },
  "why": "Priya needs the Q4 numbers by Friday.",
  "created": "2026-11-16T09:17:02Z",
  "submitted": "2026-11-16T09:17:40Z",
  "state": "open",
  "actions": [
    {
      "n": 1,
      "revision": 1,
      "verb": "archive",
      "targets": { "threads": ["t-7712", "t-7713"] },
      "resolved": { "messages": ["m-50101", "m-50102", "m-50140"] },
      "args": {},
      "why": "Two newsletters from last week.",
      "flags": [],
      "hash": "9b2e61d0c4a57f3e8d1b6a0f2c49e7d3a18f5b0c6e2d94a7b3f0c18e5d7a2b46",
      "decision": { "state": "applied", "by": "rule", "rule": 1, "policy": 3, "at": "2026-11-16T09:17:41Z" },
      "ledger": "l-901"
    },
    {
      "n": 2,
      "revision": 1,
      "verb": "label",
      "targets": { "threads": ["t-8841"] },
      "resolved": { "messages": ["m-55210", "m-55211"] },
      "args": { "label": "Finance" },
      "flags": [],
      "hash": "4f8a0c2e7b13d95a6e0f1b8c3d72a4e95b6c0d1f8e3a7b29c4d05e6f1a8b3c72",
      "decision": { "state": "in_review", "by": "policy", "reason": "No rule allows label for this sender." }
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | `cs-` and a number. Unique in one install. |
| `revision` | Starts at 1. The author submits a new revision after a review comment. Each action of the old revision in the state `proposed` or `in_review` becomes `replaced`. The new revision has its own actions and decisions. |
| `author.kind` | `person`, `agent`, `triage` or `local`. |
| `why` | The reason of the author, in one or two sentences. Required. |
| `state` | `draft` (the newest revision is not submitted), `open` (one or more actions are open, section 5.2), `closed` (no action is open) or `withdrawn`. |
| `withdrawn_by` | Since `iris/1.2`. Only on a withdrawn changeset: `author` (its author withdrew it) or `person` (you withdrew it). Absent when a version before store migration 51 withdrew it (decision D16, issue #221). |
| `comments` | Since `iris/1.2`. The review comments of the person, oldest first: `revision`, `text` and `at` of each. Absent when there is none. An author with no job workspace reads them here (decision D15, issue #538). |
| `actions[].n` | The number of the action in the changeset, from 1. |
| `actions[].revision` | The revision of the action. The document lists the actions of each revision, because an old revision can keep decided actions and `approved` actions. |
| `actions[].targets` | What the author named: `threads`, `messages` or `items`. The write verbs of release 3 also name `accounts` (`a-N`, a new mail) or `drafts` (`d-N`), with exactly one target (`docs/specs/2026-11-16-send.md`, section 4). |
| `actions[].args` | The arguments of the verb (section 4), as one JSON object. `label` and `unlabel`: `label`. `close`, `decline`, `skip`, `mute` and `dismiss`: `reason` (500 characters or fewer; `docs/specs/2026-11-17-preference-rules.md`). `defer`: `date`. `correct`: `field` and `value`. `draft`: `to`, `cc`, `subject` and `body` (a reply has no `subject`; `docs/specs/2026-11-16-send.md`, section 1), and the optional `bcc` and `attachments` (`docs/specs/2026-11-24-send-content.md`, section 1). Each other verb, `send` too: none (`{}`). A `denied` or `cancelled` action, and a draft action, can hold arguments that do not fit the verb, because only submit checks them. |
| `actions[].resolved` | The message IDs at submit. Review shows these, and apply uses only these. The engine keeps them in a child table, and writes the document by streaming (section 9). |
| `actions[].flags` | Why a person must look: see section 5.3. |
| `actions[].hash` | `null` in a triage proposal that the engine does not store (release 1). Else the SHA-256 (hex) of the `verb`, the `targets` text, the `args` text and the `resolved` message IDs, from submit. Each of the three texts has its length in bytes first (8 bytes, big-endian). Then each message ID follows, in the order of the store row IDs, with a comma after it. For an action with a draft target (release 3), the content hash of the draft follows (`docs/specs/2026-11-16-send.md`, section 5). An approval names it (section 5.5). A submitted action does not change, but the engine can add a flag before apply (section 5.4). `null` before submit. |
| `actions[].decision.state` | See section 5.2. |
| `actions[].decision.rule`, `actions[].decision.policy` | For a decision by a rule: the rule `id` and the policy version (section 6.3). Together they name one rule text. |
| `actions[].decision.by` | Who decided: `rule` (with `rule`, its ID), `person`, `policy` (the default or the mode), `limit` (a hard limit), `engine` (a target or an argument that is not valid), `author` (a withdraw or a new revision) or `expiry`. A draft action has the decision `null`. An `applied` or `failed` action keeps the decision that approved it. The ledger names it, and records the apply (section 7.1). |
| `actions[].ledger` | The ledger entry, after apply. Required for `applied`, `failed` and `undone`. An action that was applied before the ledger (migration 11) has an entry with no messages and no undo. |

### 5.2 States of an action

| State | Meaning | Next states |
|---|---|---|
| `proposed` | Submitted, and the policy did not decide it, or the mode is `observe`. | `approved`, `in_review`, `denied`, `expired`, `cancelled`, `replaced` |
| `in_review` | Waits for a person. | `approved`, `denied`, `expired`, `cancelled`, `replaced` |
| `approved` | An `allow` rule (`decision.by` `rule`) or a person approved it. The engine applies it next. | `applied`, `failed`, `held`, `in_review`, `denied`, `expired`, `cancelled` |
| `held` | A send in the held outbox (release 3). | `applied`, `failed`, `cancelled` (a withdraw), `expired`, `denied` (by `engine`: the draft changed or is gone, `docs/specs/2026-11-16-send.md`; by `person`: the cancel in the outbox, `docs/specs/2026-11-17-held-outbox.md`) |
| `applied` | The provider accepted the change. The ledger has the entry. | `undone` |
| `failed` | The provider refused it, or recovery could not confirm each message. The ledger lists the messages that changed. | `undone`, only when the ledger lists one or more changed messages |
| `denied` | The policy, a hard limit or a person refused it. `reason` says why. | none |
| `expired` | No decision or no apply in 14 days after submit (section 5.5). | none |
| `cancelled` | The author withdrew the changeset. A person who cancels a held send denies it (`denied` by `person`). | none |
| `replaced` | A new revision replaced the action before a decision. | none |
| `undone` | The undo is applied. The undo has its own ledger entry. | none |

The states `proposed`, `in_review`, `approved` and `held` are open. Each other
state is decided.

### 5.3 Flags

The engine adds flags. An author cannot remove a flag.

| Flag | When |
|---|---|
| `new_recipient` | A `draft` or a `send` goes to an address that the owner never sent mail to. |
| `outside_thread` | A `draft` or a `send` goes to an address that is not in the thread, and its author read mail that you did not send (`docs/specs/2026-11-17-hostile-input-limits.md`). |
| `money` | The text of a draft names an amount of money (`docs/specs/2026-11-17-send-checks.md`). |
| `date` | The text of a draft names a date (`docs/specs/2026-11-17-send-checks.md`). |
| `attachment` | The text refers to an attachment, and the draft has no attached file. |
| `first_contact` | The target thread is with a `stranger`. |
| `untrusted_content` | The text of the draft contains text from an external message. |
| `bulk` | The action changes more than 500 messages. Or the changeset changes more than 500 messages in all. Or, with this action, more than 500 messages of the same author applied without a review by a person in the last hour. |

The flags of a draft (`new_recipient` to `untrusted_content`) come from the
content of the draft (section 7.5).

### 5.4 Submit

1. The author adds actions to a draft changeset. Nothing leaves the engine.
2. The author submits the changeset with a reason.
3. The engine resolves each target to message IDs, from the mirror.
4. The engine adds the flags.
5. The engine evaluates the policy for each action (section 6.4). Each action
   gets `approved` (and then applies), `in_review` or `denied`.
6. The engine returns one result for each action, and the exit code of
   section 8.4.

If the policy mode is `observe`, the rules do not run in step 5, and each
action stays `proposed`, except an action that a hard limit denies (section
6.5): it gets `denied` by `limit` in both modes. A `proposed` action has no decision columns (section 5.6), so the
engine does not store what the policy would do. The review computes it
at each read (decision D10, `GET /api/changesets/{id}/would`): for each
`proposed` action, the decision that a rule of the active policy gives
in the mode `apply`, with the same facts, flags and author. It is empty
in the mode `apply` and with no confirmed policy. A read never changes an
action, and it always follows the active policy of now, so a changed
rule changes the answer at the next read. The triage changeset of
release 2 follows the mode of the active policy, as each other changeset does
(specification 1, section 8). Release 1 computes the triage proposal and does
not store it (specification 1, section 8).

A change of the mode does not evaluate open actions again:

- From `observe` to `apply`: each `proposed` action stays `proposed`. The
  person can approve or deny it in the web client.
- From `apply` to `observe`: an `approved` action waits, and applies after a
  change back to `apply`. A ledger row in the state `writing` continues to its
  end (section 7.3).

The engine applies one action at a time, on the one write connection. Before
the engine applies an action that a rule approved, it computes the flag
`bulk` again and evaluates the action again against the active policy
(section 6.4), in the transaction of the claim (`iris_apply::recheck_rule`,
with the same `iris_triage::evaluate` as the submit).
For the count of the last hour, `bulk` counts the messages of the action and
the messages of each ledger row of that hour whose action a rule decided,
in each state. An undone entry counts, because it changed its messages. The
same author is the same author kind (`person`, `agent`, `triage` or
`local`), as in the policy (section 6.4), so two jobs of agents share one
count. If the result is not `allow`, the action gets the new result,
`in_review` or `denied`, by `engine`, with the reason of the policy. A
`bulk` from the count of the last hour is added to the flags of the action.
A rule approval of `label` whose label Gmail does not have goes to
`in_review`: the create of the label is a write call before the claim, and a
policy change during that call cannot stop it. Only a person approval makes
a new label. So a rule that the
person removed while the action waited applies nothing, and two changesets
that a rule approved at the same time cannot together go past the limit of
500 messages.

### 5.5 Review, revision, withdraw and expiry

- The person reviews in the web client. The review shows each action fully
  resolved: the real messages, the real recipients and the final text.
- The person can approve all, approve one, edit the text of a draft, comment
  or deny.
- An approval names the changeset, the revision, the action number, and the
  hash that the person saw (`actions[].hash`, section 5.1). The engine
  refuses an approval while a new revision is a draft, an approval of a
  replaced action, and an approval with another hash. If an action of an
  older revision stays open after the submit of a new revision, a rule
  approved it before. It can come back to review (section 5.4), and the
  person approves it with its hash.
- A comment goes back to the author. In release 4, the comment arrives as a
  file in the workspace. The author submits a new revision.
- `iris withdraw` withdraws a changeset. The engine accepts it only from the
  author of the changeset: the same `kind`, `name` and `session`. For an
  author with a job (release 4), the author is the job: the same `kind` and
  `job`, so a later session of the job continues its changesets
  (`docs/specs/2026-11-17-job-verbs.md`). The same rule applies to each
  change of a changeset by its author: add, revise and submit. The person
  can withdraw each changeset in the web client. Each open action becomes
  `cancelled`, except an action with a ledger row in the state `writing`,
  which continues to its end. Applied actions stay.
- 14 days after submit, each open action that has no ledger row in the
  state `writing`, and is not an undo (section 7.4), becomes `expired`. A
  `held` send expires too (`docs/specs/2026-11-17-held-outbox.md`). The
  changeset is `closed` when no action is open.

### 5.6 Failure list of the changeset store

The changeset store (crate `iris-changeset`, migration 7) takes requests
from authors that the product does not control. The apply and the ledger
(sections 7.2 and 7.3) get their own failure list with their code.

Supported input: short IDs of section 5.7 of specification 1, an `args`
JSON object, a reason, and the action number, revision and hash of a
decision.

The triggers of migration 7 hold the rules of this section for each write
of the engine code, also for a defect in a code path. They are not a
security boundary. A process that can write `iris.db` can also drop a
trigger, so the file permissions of the data folder (specification 1,
section 5.1) keep other users out.

| Input or event | Failure mode | Safe result |
|---|---|---|
| A text that is not an ID, a sender ID, IDs of two groups in one action (threads, messages and items), or one target two times | The action names nothing that it can change | `add_action` refuses it. Nothing is written. The items of all forms are one group: a `correct` action can name a commitment and a record. |
| An ID that is not in the mirror, a gone message, or a target of the wrong kind for the verb | Apply changes the wrong object | Submit gives the action `denied`, by `engine`, with the reason. |
| No target, or mailbox verb targets that give no message (each message of the thread is gone or in `SPAM` or `TRASH`) | A `proposed` action that changes nothing | Submit gives the action `denied`. A state verb changes its items, so an item with no origin message is a valid target. A write verb (`draft`, `send`, release 3) writes mail, so it needs no message: an account target (`a-N`) gives no message, a draft target (`d-N`) gives the messages of the thread of the draft, for the checks only, and the thread target of a `draft` gives one message, the parent of the reply (`docs/specs/2026-11-16-send.md`, section 3). |
| Targets in two or more accounts | One approval changes two mailboxes, and the apply has no one account | Submit gives the action `denied`. One action changes one account. |
| Arguments that do not fit the verb, or the verb `untrash` from an author | A rule or a limit for one verb is bypassed | Submit gives the action `denied`. Until each verb has its argument checks, submit denies the verb. Only the undo of a `trash` makes `untrash` (section 7.4): submit denies an `untrash` action with no link in `ledger_undo` (migration 23). |
| A row with targets or arguments of a wrong shape, from another writer | A panic of the engine | Submit gives the action `denied`. A text that is not JSON cannot be stored (`CHECK`). |
| A state move that section 5.2 does not allow, from any writer | A decided action changes, or skips the review | The trigger `action_state` refuses the write. The list of moves is in the trigger, not in a table. |
| An expiry before 14 days, or with a time after the clock of the database | An action expires before the person can decide | The trigger `action_state` refuses the write. The routine `expire` (each engine tick) expires each `proposed`, `in_review` and `approved` action 14 days or more after its submit, except an action that has a ledger row in `writing` (migration 11): recovery ends that action as `applied` or `failed`. |
| A rule decision with a policy version that is not active, or a rule ID that the active policy does not have | A decision names no rule text, or a rule that the person removed | The trigger `action_state` refuses the write. The policy of the first start has no rule, so no rule decides under it. |
| A change of a submitted action, from any writer | The person approves one action, and another applies | The triggers refuse the write. Only the engine adds a flag, to an open action, and a flag does not go away. |
| A new action in a submitted revision, an action that changes its ID or moves to another revision, an insert that replaces an action, or a state for an action while its revision is a draft | The changeset state does not agree with its actions, and an action skips the submit | The triggers refuse the write. An action goes only into the draft revision, and gets a state only with the submit of its revision. |
| A submitted revision that becomes a draft again, a change of the author, the ID or the reason after the submit, a submit in the same write as a withdraw, or a change of a withdrawn changeset | Actions enter a revision that the person saw | The trigger `changeset_revision` refuses the write. A revision moves only with its submit, or to a new draft revision after the submit. |
| Flags that are not a JSON array of the names of section 5.3 | A flag that a check does not see | The triggers refuse the write. |
| A draft or `proposed` action with a decision, a decided action that does not name who decided, an end state with another actor (`expired` by `expiry`, `cancelled` and `replaced` by `author`), or a person who sends an action to review | A decision that no one made | A `CHECK` refuses the write. Submit proposes an action with no decision. |
| A rule that approves or denies an action in `in_review` | The action skips the person | The trigger `action_state` refuses the write. Only a person decides an action in review. |
| A submit that writes the changeset but not each action | A submitted revision with a draft action | The submit of the revision proposes each of its actions in the same write (trigger `changeset_submit`). A submit needs one or more actions, each with its hash. The engine then denies the actions that are not valid. |
| A changeset state that does not agree with its actions | A reader sees a closed changeset with an open action | The triggers write the state from the actions, and refuse any other value. Only a withdraw writes `withdrawn`. |
| The submit of a new revision while older actions are `proposed` or `in_review` | An action that the author revised gets an approval | The trigger `changeset_submit` replaces them in the same write. Only that submit replaces an action, with no check of the clock (the marker `changeset.replacing` is set once for each revision, in its submit): an older action that comes back to review after the submit stays for the person. |
| A new revision with a new reason | The reason that the person saw for an older revision is lost | The table `changeset_revision` keeps the reason and the submit time of each submitted revision. Its rows do not change. |
| A withdraw that does not name who withdrew, a `withdrawn_by` on a changeset that is not withdrawn (also at insert), or a later change of `withdrawn_by` | The document names the wrong actor | The CHECK of the column and the triggers `changeset_withdraw_by` and `changeset_withdrawn_by` (migration 51) refuse the write. |
| A withdraw while an action is open | A withdrawn changeset applies an action | The trigger `changeset_withdraw` cancels each open action and each draft action in the same write. A withdrawn changeset stays withdrawn. |
| A withdraw while the ledger has a row in `writing` for an action (section 5.5) | The provider changed the messages, and recovery cannot record the result | The trigger `changeset_withdraw` (migration 11) keeps such an action `approved`. The trigger `action_state` then allows only `applied` or `failed`, as the row ends. Migration 11 replaces the mark `applying` of migration 9 with the row. |
| A move to `applied`, `failed` or `undone` | A state with no ledger entry, or an apply that changes the decision | Only the state of the ledger entry moves the action: the trigger `ledger_state` writes the same state to the action, and the trigger `action_state` refuses the move when the entry has another state. The action keeps the decision that approved it (section 7.1). |
| An approval while a new revision is a draft, of a replaced action, or with another hash | The person approves what they did not see | `approve` refuses it. An older action that a rule approved and that came back to review gets the approval with its hash (section 5.5). |
| Two writers at the same time | Two moves of one action | The engine has one write connection. Each request is one transaction, and the trigger checks the state at the time of the write. |
| A crash in a submit or a decision, or a failed commit | Half a submit, or a success for a change that is not stored | The transaction rolls back, and the call gives the error. |
| All other input | | Refused with an error, and nothing is written. |

## 6. The policy file

### 6.1 Place and format

The policy is the file `policy.toml` in the data folder. TOML is a plain text
format that a person can read and edit, and comments in the file stay.

`schema/policy.schema.json` is the JSON Schema of the file, with sample
files in `schema/examples/` (files `policy*.toml`). A TOML editor that reads
JSON Schema can use it. The schema reads the TOML data as JSON, with each
TOML date as a text `YYYY-MM-DD`. The engine refuses three more cases that
the schema does not see:

- Two rules with the same `id`.
- A file larger than 256 KiB.
- A value of a TOML type that has the same JSON form as the type that the
  field needs: an `added` text in quotes (`added = "2026-11-10"`) in place
  of a TOML date, and a float with no fraction (`id = 1.0`) in place of an
  integer.

The `version` of the file is `1`. Iris also accepts `version = 0`, with the
same meaning, because files of version 0 exist ([specification 4](04-open-model-versions.md), rule 2.7).

```toml
# Iris policy. Version 1.
version = 1

# observe: Iris applies nothing. apply: rules decide, and the rest goes to review.
mode = "apply"

# Each rule has an ID that Iris never uses again.

[[rule]]
id = 1
name = "Archive newsletters after 3 days"
effect = "allow"
verbs = ["archive", "read"]
when = { form = "feed", sender_kind = "service", older_than_days = 3 }
by = "person"
added = 2026-11-10

[[rule]]
id = 2
name = "Hide Acme Deals"
effect = "hide"
when = { sender = "deals@acme.example" }
by = "proposal"
added = 2026-11-12

[[rule]]
id = 3
name = "Bundle the feed"
effect = "bundle"
when = { form = "feed" }
by = "person"
added = 2026-11-10

[[rule]]
id = 4
name = "Never touch mail from the bank without review"
effect = "review"
verbs = ["archive", "inbox", "label", "unlabel", "read", "unread", "trash"]
when = { sender = "@bank.example" }
by = "person"
added = 2026-11-10
```

### 6.2 Fields of a rule

| Field | Required | Values |
|---|---|---|
| `id` | yes | A positive integer, unique in the file. |
| `name` | yes | One line that the brief and the ledger show. |
| `effect` | yes | `allow`, `review` or `deny` (actions); `hide`, `bundle` or `notify` (attention). |
| `verbs` | for `allow`, `review` and `deny` | The verbs of section 4, except `send`. A rule with `send` is refused: a send has no undo, so a person approves each send (decision D2, issue #239). A rule can allow `draft`: after the apply gate, the engine makes the draft at the provider. Only a `send` enters the held outbox, and a person approves each send. |
| `when` | yes | The conditions. Each condition must match. |
| `by` | yes | `person` (written by the person) or `proposal` (proposed by Iris and accepted by the person). |
| `added` | yes | The date. |
| `job` | no | A job ID. The rule applies only in that job (a grant inside one job, release 4). |

Conditions in `when`:

| Condition | Matches |
|---|---|
| `form` | The form of the item that the action or the attention is for. |
| `sender` | An address, or a domain with `@` in front. |
| `sender_kind` | `self`, `person`, `service`, `stranger` or `hostile`. A message that you sent matches `self`. A message with a hostile mark (specification 1, section 6.7) matches only `hostile`, whatever the kind of its sender. |
| `label` | A label on the message. |
| `record_type` | The type of a record. |
| `commitment_kind` | The kind of a commitment. |
| `author` | `person`, `agent`, `triage` or `local`. Each value matches only that author. |
| `older_than_days` | The age of the message in days. |
| `recipients` | `thread`: the action is a reply `draft`, and each address in To and Cc of the draft is in its thread, or is one of your addresses. An address is in the thread only if it is the From of a live message of the thread, or in To or Cc of a live message of the thread that you sent (decision D13, issue #435). A new mail has no thread, so it never matches. A rule with `recipients` has only the verb `draft` (decision D3, issue #386). |

Version 1 has no other condition, and no expression language. A new condition
needs a change to this specification.

Each condition other than `author` and `recipients` matches one message.
`author` and `recipients` are conditions of the action. For an action on an item, the message is
the origin message of the item. `form`, `record_type` and `commitment_kind`
match in two ways, so that each reading is the strict one:

- For an `allow` rule, a message matches only when it has one or more items,
  and each item of the message has that value. So a message with a feed item
  and a receipt does not match `form = "feed"`.
- For a `deny` or a `review` rule, a message matches when one or more of its
  items has that value.

These two readings apply only to `form`, `record_type` and
`commitment_kind`. The other conditions do not read items. So an `allow`
rule with no item condition, for example `sender = "@shop.example"`, matches
a message with no items too: the person wrote a rule about the sender, not
about the items.

A rule without `author` matches each author, `local` included.

The engine refuses a file that is not valid (section 6.3). A file is not
valid when:

- It has a field or a condition that this section does not name. So a typo
  cannot make a rule wider than the person wrote it.
- A value is not one of the values above, or two rules have the same `id`.
- `when` has no condition, or a `name` has more than one line.
- A `hide`, a `bundle` or a `notify` rule has `verbs`, `author`, `recipients` or `job`. An item has no
  author, no recipients and no job, so such a rule could never match.
- A rule has `recipients` and a verb other than `draft`, or has the verb `send`.
- The file is larger than 256 KiB, or has more than 1,000 rules (section 9).

An `allow` rule never matches an action with no message.

### 6.3 Who changes the file

- Only the person changes the policy. The person edits the file, or uses the
  web client. There is one exception: when a job is done, the engine removes
  each rule whose `job` names that job (issue #385). Such a rule matches
  nothing more, because a done job has no session. The removal is a normal
  policy change with a new version, and it keeps the comments of the file.
- The engine keeps the active policy in `iris.db`: the text of the last file
  that the person confirmed, and its SHA-256 hash. The engine evaluates only
  the active policy.
- The engine keeps each confirmed text as a numbered version (table
  `policy_version`), and never changes or deletes a version. The person can
  edit a rule and keep its `id`. A decision by a rule names the version and
  the rule `id`, so it always points to one rule text.
- When the file changes on disk, the engine reads it again. If the file is
  valid, the web client shows the difference to the active policy, and the
  person confirms it. Only then does the file become the active policy.
- A confirmation names the SHA-256 hash of the text that the person saw. The
  engine reads the file again in the transaction of the confirmation. If the
  hash of that text is not the hash of the confirmation, the engine refuses
  the confirmation, and the web client shows the new difference. So a
  confirmation never makes active a text that the person did not see.
- The engine keeps each rule `id` of each active policy (table
  `policy_rule_id`). It refuses a confirmation of a file that has the `id`
  of a rule that an earlier active policy removed, and the brief names the
  line. The status of the file shows the same error before a confirmation.
- When the person changes the policy in the web client (a change of `mode`,
  an accepted proposal or the removal of a rule), the engine writes the file
  and makes it the active policy in one step. The engine starts from the text
  of the active policy, changes only the lines of that change, and keeps all
  other text and comments. If the file on disk is not the same as the active
  policy, the person first confirms or discards that difference.
  A discard writes the text of the active policy to `policy.toml`. At the
  first start there is no active text, so no new file is written. A change
  and a discard keep the old file as a copy (`policy.toml.saved-<seconds>`)
  and never delete it, because an editor can hold the file open and save
  into it later. A file that the person saves during a change or a discard
  is never replaced (specification 1, section 5.1 gives the write).
- If the file is not valid, the active policy stays. The brief shows the
  error and the line number. The person cannot confirm a file that is not
  valid.
- At the first start, the active policy has the mode `observe` and no rules.
- No verb of the `iris` command changes the policy.

This is fail closed: a change on disk that the person did not confirm, or a
defect in the file, can never allow more.

Failure list of the policy file. Supported input: a UTF-8 TOML file of at
most 256 KiB that follows section 6.2.

| Failure | Safe result |
|---|---|
| The file does not exist. | The active policy stays. At the first start it is the mode `observe` with no rules. |
| The engine cannot read the file (permissions, an I/O error). | The active policy stays. The brief shows the error. |
| The file is not UTF-8, is larger than 256 KiB, or is not valid TOML or not valid under section 6.2. | The active policy stays. The brief shows the error and the line, when the error has one. |
| The file changes between the view of the difference and the confirmation. | The engine refuses the confirmation (hash check above). The person reviews the new text. |
| The file changes while the engine reads it, so the engine reads a part of an edit. | The text is not valid, or it is a new text with its own hash. Either way it is not active until the person confirms that text. |
| A crash during a confirmation, or during a change from the web client. | The active policy and its hash change in one SQLite transaction. After the start, the active policy is the old one or the new one, never a mix. If the file on disk is not the active policy, the person confirms or discards the difference. |
| Any other input. | The active policy stays. |

When the active policy changes, the engine evaluates the attention of each
item again (section 6.4).

### 6.4 Evaluation

For each action:

1. Check the hard limits (section 6.5). The result is `deny`, `review` or no
   result.
2. Find each rule with an action effect (`allow`, `review`, `deny`) whose
   `verbs` include the verb.
   - A `deny` or a `review` rule matches when its `when` matches one or more
     messages of the action.
   - An `allow` rule matches when its `when` matches each message of the
     action.
3. The rule result: `deny` if a `deny` rule matches. Else `review` if a
   `review` rule matches. Else `allow` if an `allow` rule matches. Else
   `review`: the default is review.
4. The final result is the stricter of steps 1 and 3, in the order `deny`,
   `review`, `allow`. A rule can make a hard limit stricter, never less
   strict.
5. `deny` gives `denied`. `review` gives `in_review`. `allow` gives
   `approved` with `decision.by` `rule`, and the engine applies the action.

For attention, each item gets the effect of the `hide` or `bundle`
rule that matches and has the lowest `id`. With no match, the item is `shown`. The item
records the rule in its `attention` field (specification 1, section 6.1).
Hidden is never deleted: a `hide` rule changes only what the brief shows.

A `notify` rule does not change the attention of an item. It gives one
notice for each new mail that it matches, in the web client and in the
system (`docs/specs/2026-11-16-notices.md`). It matches with the reading of
a `review` rule: an item condition matches when one or more items of the
message have the value. With no `notify` rule that matches, no mail notice
appears. One system notice comes from no rule: the due date that the person
gave to a job (decision D24, `docs/specs/2026-11-24-job-due-date.md`).

The engine evaluates the attention of an item again at these times:

- When the active policy changes, for each item, in the same transaction.
- When a fact that a rule reads changes: a new item or a new origin of an
  item, the form, the record type or the commitment kind of the item, or the
  labels, a label name, the hostile marks, the date or the sender kind of
  its origin message, also when the cleanup of gone mail deletes the header.
  A store trigger marks the message. `iris serve` evaluates its items within
  about 10 seconds, and `iris mirror` and `iris understand` before they exit.
  Until then, an item keeps its last attention, and a new item has no
  `attention` field.
- One hour after the last evaluation of each item, for each item: an
  `older_than_days` rule matches more items as time goes on, with no change
  in the store. The store keeps the time of that evaluation, so the first
  evaluation after an upgrade is for each item.

### 6.5 Hard limits

The engine enforces these limits before the policy. They are in the code, not
in the file.

| Limit | Result |
|---|---|
| A `send` never goes at once. | It waits in the held outbox for the hold delay, after the approval (release 3). |
| A `draft` or a `send` with the flag `new_recipient`, `outside_thread`, `money` or `date`. | Review, even if a rule allows it. Only a person approves it (store migration 25). The decision is by `limit`. |
| A `trash` or a `label` of a marked author: an author that read mail that you did not send (`iris_changeset::marked_in`, decision D14, issue #437). | Review, even if a rule allows it. A deny rule still denies it. The decision is by `limit`. |
| A `send`. | Review. No rule can allow a send (section 6.2). |
| An action with the flag `bulk`. | Review, even if a rule allows it. |
| An action by an author other than `person`, whose target has hostile mail: a message with a hostile mark, a message that waits for its link check (a `link_check` row), or a message from a `hostile` sender (specification 1, section 6.7). For a thread or a draft target, each message of the thread counts. | Only `archive`, `trash` and `label`. Other verbs are denied. |
| Stop is on (release 4). | No provider write. Actions in the outbox wait. |
| Mode is `observe`. | Nothing applies, except an undo by the person (section 7.4) and a state verb that the person approved (section 4). |
| A permanent delete. | No verb exists. |
| Hostile mail: a message with a hostile mark, a message that waits for its link check, or a message from a `hostile` sender. | No notice, even if a `notify` rule matches. |

The hard limits treat `local` and `triage` as agents.

### 6.6 Proposed rules (release 5)

Iris proposes a rule after a pattern: for example, the person approved 14 of
14 `archive` actions for the same sender kind. The proposal is a row in the
store, not a line in the file. When the person accepts it, the engine adds the
rule to the file with `by = "proposal"`. A proposal never changes the file
without the person.

The proposal is a row of the table `rule_proposal` (store migration 15).
The engine (crate `iris-policy`, module `propose`) writes the open rows
again when a decision of the person, an undo, the active policy or the hour
changes:

- It reads the newest 2,000 actions of an author other than `person`, with
  the verb `archive`, `read`, `unread`, `inbox` or `trash`, that the person
  approved (the states `approved`, `held`, `applied` and `failed`) or
  refused (`denied` by the person, or `undone`). `label` and `unlabel` are
  not in the list, because a rule cannot name the label. The state verbs
  change items, not mail.
- A candidate is one verb and one condition: the `sender` address, the
  `sender` domain (`@` and the domain of the address) or the `sender_kind`
  that each message of one approved action has. An action on
  hostile mail (a hostile mark, or a sender of the kind `hostile`) gives no
  candidate and counts as no approval, and `hostile` is never a candidate.
- The candidate rule is added to the active policy. An action counts for it
  only when the rule changes the decision of that action to approved
  (section 6.4), with the author, the job and the flag `bulk` of the
  action. The engine proposes the rule when 10 or more approvals count and
  no refusal counts.
- A domain candidate also needs counted approvals on two or more
  addresses. With one address, the address proposal covers it. When the
  engine proposes a domain rule, it does not propose a rule of the same
  verb (and form) for an address of that domain. If the person dismisses
  the domain proposal, the address proposals come back.
- A refusal that the engine cannot read again (a message row or its
  header is gone) stops each proposal of its verb.
- The pass holds the facts of one action at a time, and one count for each
  key. It skips a bulk action, because a bulk action stays in review with
  each rule (section 6.5), so it holds the facts of 500 messages or fewer.
  The background pass runs on a read connection and holds no write lock.
  Before it writes, it reads its mark again under the write lock, and it
  drops a pass that a later decision or a new policy made stale.
- Each policy that becomes active (an accept, an edit from the web client,
  a confirmed file) runs the pass again in its own write transaction, so
  the rule removes each open proposal that it covers at once. This pass
  holds the write lock while it reads at most 2,000 actions.
- The person accepts or dismisses a proposal in the web client. An accept
  is a change of section 6.3: the engine adds the rule at the end of the
  file, with the next rule ID, `by = "proposal"`, the date and a comment,
  and marks the row `accepted` in the same transaction. The accept runs the
  same count again in that transaction. If a decision after the last pass
  breaks the pattern, the accept adds nothing and the web client gets a
  409. A dismiss changes
  no file. Iris never proposes an accepted or a dismissed rule again.
- An open row that no longer has the pattern goes away.

The pass also proposes a `hide` rule from the choices of the person
(card R5-02, `docs/specs/2026-11-17-preference-rules.md`):

- It reads the newest 2,000 actions of the author `person` with the verb
  `skip`, `mute` or `dismiss`, `applied` (a choice) or `undone` (a
  refusal).
- A candidate is a `hide` rule with two conditions: the `sender` of the
  origin message of the item (the address, or its domain with the domain
  rule above), and the `form` of the item. Hostile mail
  and an origin message from the person (sender kind `you`) give no
  candidate. A choice counts only when the rule, added to the
  active policy, changes the attention of its item from shown to hidden.
- The engine proposes the rule when 10 or more choices count and no
  refusal counts. A refusal that the engine cannot read again stops each
  `hide` proposal of its form.
- An undone choice whose item, origin message, header or sender cannot
  be read again stops each `hide` proposal of its form.
- An open of a source mail is a refusal too. The pass reads the rows of
  `use_open` (the opens of the measures page, store migration 27, on this
  machine only) of each day from the day of the 2,000th newest open. A row
  has a day but no time, so the window takes whole days. For each item whose origin is the
  opened mail, each candidate whose rule would change the attention of
  the item from shown to hidden gets a refusal. An open of mail from the
  person or of hostile mail refuses nothing, as such mail gives no
  candidate. An open whose header is gone names no sender, so it refuses
  nothing. A new open and a dismiss change the mark of the pass.
- The row has `verb = 'hide'` and the `form` (store migration 30). An
  `allow` row has the form `''`.

Failure list of the proposals:

| Input or event | Failure mode | Safe result |
|---|---|---|
| A decision after the last pass | An accept adds a rule that a new refusal contradicts | The accept counts again in its transaction, and refuses (409). |
| A crash after the file write of an accept, before its commit | The file has the rule, and the row stays open | The file is a change of section 6.3. Each policy that becomes active (an accept, a confirmation) marks as accepted each open row whose rule it has with `by = "proposal"`. A discard leaves the row open. |
| A failed commit of a pass | The pass is lost | The engine keeps the mark of a pass only after its commit, so the next tick runs it again. |
| A message of a decided action that the cleanup changed | A refusal does not count | A refusal that the engine cannot read again stops each proposal of its verb. |
| An open of a mail that the cleanup deleted | An open does not count | The open has no sender and no item, so no rule can come from it. It refuses nothing. A later open of another mail from the sender still refuses. |
| Approvals on one address of a busy domain | A domain rule wider than the evidence | A domain proposal needs approvals on two or more addresses, and each refusal on any address of the domain stops it. |

## 7. The ledger

### 7.1 What the ledger holds

The ledger is the table `ledger` in `iris.db`, with the child table
`ledger_message`. The engine adds rows. It changes a row only to move its
state forward, and to record the progress of the provider calls.

The `ledger` table has one row for each applied action, with a provider call
or with a change of Iris state:

| Column | Meaning |
|---|---|
| `id` | `l-` and a number. |
| `action` | The action that the entry applies. One entry for each action. The changeset, the revision, the verb, the author and the decision (`rule` with its ID and the policy version of section 6.3, or `person`) come from the action row. That row does not change after the submit, and an applied action keeps the decision that approved it. |
| `account` | The mailbox of the provider calls. None for a state verb. |
| `ops` | The provider operations, as JSON: the call (`batchModify` for each 1,000 messages, or `trash` or `untrash` for each message), the labels to add and remove, and the number of calls. Only an undo of `trash` writes `untrash` (section 7.4). For a state verb, the fields and the IDs of the correction rows of the entry. For `draft` and `send` (release 3): the call (`drafts.create` or `drafts.send`), the `Message-ID`, the `Date`, the content hash and, for a send, the Gmail draft ID (`docs/specs/2026-11-16-send.md`). |
| `pre`, `post` | For a state verb: the old and the new values. For `draft`: `post` only, the content with its reply headers. |
| `call` | The number of the last provider call that the engine started, from 1. 0 before the first call. |
| `state` | `writing`, `applied`, `failed` or `undone`. The trigger `ledger_state` moves the action to the same state. |
| `error` | The refusal of the provider, or why the engine could not confirm each message. |
| `history_id` | The Gmail history ID after the apply. |
| `undo` | `inverse` (the verb has an undo) or `none` (final, for example a sent message, or an action applied before the ledger). The column records the verb, and does not change. An undo is available now only if the entry is `applied` or `failed`, has `inverse`, and has a message with `done` that is not gone, or, for a state entry, an item in `post` and the IDs of its correction rows in `ops` (section 7.4). A state entry from before migration 26 has no such IDs, so it has no undo. The view `ledger_can_undo` (migration 23) lists these entries, and `iris_apply::undo` refuses each other entry. Else the ledger shows the entry as final, for example after Gmail deleted each message of a `trash` entry. |
| `undone_by` | The ledger entry of the undo. |
| `at` | The time. |

The `ledger_message` table has one row for each message of the entry:

| Column | Meaning |
|---|---|
| `ledger_id`, `message_id` | The entry and the message. |
| `call` | The number of the provider call that changes the message. |
| `pre` | The labels that the action changes, before the action. For `trash`, also if the message had `INBOX`. |
| `post` | The labels that the action changes, after the action, as the engine expects them. For `trash`: the label `TRASH`. |
| `done` | True when the provider confirmed the change of the message. |
| `listed` | Why a message has no `done` after the recovery (section 7.3): `skipped` (its call never started, and it has the target state now), `gone`, or `unknown` (its call started, and the engine cannot confirm the change). |

### 7.2 Apply

1. If the action adds a user label that Gmail does not have (`labels.list`),
   call `labels.create`. Just before the call, one transaction checks the
   stop, the expiry, the mode, the grant and the state `approved` again, as
   the claim of step 3 does. If the action does not apply now, no call is
   made. A change during the create call itself can still leave an empty
   label at Gmail. If Gmail answers with an error (for example, the
   label exists) or the result is not known, call `labels.list` again and use
   the user label with that name. If
   no label of kind `user` has that name, the action is `failed`. The label
   stays after an undo, so this step has no ledger entry.
2. Read the labels of each message of the action at Gmail (`format=minimal`).
   The mirror can be one sync interval behind Gmail, so the engine does not
   use the mirror for `pre`.
   - If Gmail returns HTTP 404 for a message, the message is gone. Remove it
     from the action, and list it in the result.
   - Remove from the action each message that already has the target state.
     The provider call does not change such a message, and the undo does not
     touch it.
3. In one SQLite transaction, add the ledger row with the state `writing`,
   and one `ledger_message` row for each message, with `pre`, `post` and
   `call`. One `batchModify` call changes at most 1,000 messages. One
   `messages.trash` call changes one message. The commit must be on disk
   (`synchronous=FULL`, specification 1, section 5.2) before step 4.
4. For each call, in order:
   1. Set `call` of the row to the number of the call, and commit.
   2. Call the provider.
   3. If the call succeeds, set `done` for its messages, and commit.
5. After the last call, set the state `failed` after a clear refusal (step
   6), or when one or more messages are `unknown` (section 7.3). Else set
   `applied`: a gone or a skipped message does not fail the row. Save the
   history ID. Update the mirror with the new labels.
6. Act on each other result of a call as follows:

| Result | Meaning | The engine |
|---|---|---|
| HTTP 401 | Gmail did not run the request. | Gets a new access token one time, and makes the call again one time. If the second call also gets 401, the engine stops the apply as for a clear refusal, and the account goes to "needs sign-in". |
| HTTP 429, or HTTP 403 with the reason `rateLimitExceeded` or `userRateLimitExceeded` | Gmail did not run the request. | Waits as for a read (specification 1, section 4.7), and makes the call again. |
| HTTP 400, another 403, 404, 409 or another 4xx | A clear refusal. | Sets the state `failed` and saves the error. The messages with `done` keep the change, and the undo applies to them. The engine makes no later call of the action. |
| A timeout, a lost connection or HTTP 5xx | The result is not known. Gmail can have applied the call. | Keeps the row in the state `writing`, and runs the recovery of section 7.3 for the row at once. |

While a ledger row is in the state `writing`, the engine applies no other
action that has a message of that row. The other action waits.

#### Failure list of the apply and the ledger

The crate `iris-apply` applies the verbs `archive`, `inbox`, `read`,
`unread`, `trash`, `label` and `unlabel`, with the ledger (migration 11).
No provider call of an action is made two times. The ledger row in
`writing` is the claim of the action: the transaction that adds it checks
the expiry and the mode again.

| Input or event | Failure mode | Safe result |
|---|---|---|
| An action 14 days or more after its submit (section 5.5) | An action applies after the person stopped to expect it | The apply sets `expired`, by `expiry`, with no call. An action with a row in `writing`, and an undo, do not expire. |
| The mode is `observe`, also when the person changes it during the reads of step 2 | An action applies without the mode `apply` | The action stays `approved` and waits. The claim transaction checks the mode and the expiry again. A row in `writing` continues to its end (section 5.4). |
| A token of another mailbox | The Gmail IDs of one mailbox change messages of another | Before step 2, and after each new token, the address of the Gmail profile must be the address of the account. Else the apply changes nothing (`OtherMailbox`), or the new token counts as no token. A row in `writing` recovers only with a token of its own account. |
| An action that a rule approved | A rule that the person removed applies the action, or two rule approvals of one author go past the `bulk` limit of 500 messages in one hour (section 5.4) | The gate and the claim transaction compute `bulk` again with the ledger rows of the last hour, and evaluate the action again against the active policy. If the result is not `allow`, the action gets `in_review` or `denied`, by `engine`, with no call. A row in `writing` continues to its end. |
| The hash of the rows does not agree with the hash of the approval | The person approved one action, and another applies | The engine denies the action, by `engine`. |
| A message of another account | One token changes two mailboxes | The engine denies the action, by `engine`. |
| A message that is gone in the mirror, or HTTP 404 in step 2 | A call on a message that does not exist | The apply skips it, and lists it in the result. A message that is gone in the mirror after the claim is listed `gone`, with no call. |
| A message that already has the target state | A call that changes nothing, and an undo that changes the message | The apply skips it, and counts it. It has no ledger message row. |
| HTTP 401 | Gmail did not run the call | The apply gets a new access token one time, and makes the call one time again. If there is no new token or a second 401: before the claim, the action stays as it was; after the claim, the row is `failed`, as for a clear refusal. The apply then sets `Token.refused`, and the caller sets the account to "needs sign-in", as after a sync. |
| A read that fails | No label state for step 2 or for the recovery | Before the claim, the action stays as it was, and the next apply does each check again. In the recovery, the row stays in `writing` with what the earlier reads confirmed, and the next apply of an action of the account continues the recovery. |
| An unknown result of a write (a timeout, a lost connection, HTTP 5xx) | Gmail can have applied the call, and the person can reverse the change before a call again | The recovery of section 7.3 runs at once. It reads each message of the call: the `post` state gives `done`, each other state gives `unknown`, and no call is made again for the message. One or more `unknown` messages make the row `failed`. |
| A stop of the engine after the claim | The result of a call is not known | The row stays in `writing`. The next apply of the action, or of another action of the account, or `iris_apply::recover` at start, runs the recovery. |
| Two applies at the same time, in one process or in two | Two applies send the same call, or one fails the action of the other | Each apply holds the file lock `apply.lock` of the data folder. A second apply gets `Busy` and changes nothing. While the lock is held, a row in `writing` is the row of an apply that stopped. The ledger allows one entry for each action, and only an approved action gets one. |
| A row in `writing` of another action with the same messages | Two applies change one message, and the recovery of the first row reads the change of the second | Before its own checks (the expiry, the mode, the rule and the hash), the apply runs the recovery of each row of the account in `writing`, so such a row ends also when the action waits or is refused. The trigger `ledger_message_insert` refuses a message that a row in `writing` has. |
| A sync of the account during the apply | The apply writes to the mirror labels that it read before the sync saved newer labels | The apply also holds the sync lock of the account (specification 1, section 4.2). While a sync runs, the apply gets `Busy` and changes nothing. |
| A clear refusal (HTTP 400, another 4xx) | The rest of the action is not valid | The row becomes `failed`, with the error. The messages of the earlier calls keep the change and `done`. No later call is made. |
| A withdraw or an expiry during the apply | Gmail changed messages of a cancelled action | A withdraw keeps an action with a row in `writing` `approved`, and it cancels an action with no row. Such an action can only become `applied` or `failed`, as its row ends (section 5.5). |
| A message that changed at Gmail after the last sync | The mirror keeps labels that Gmail does not have | The mirror gets the labels of each read of step 2 and of the recovery. After a `batchModify` call, the mirror gets the change of the call; after a `trash` call, the labels of the answer. The apply changes only the label rows that differ, so the triggers on the labels of a message (the model wait, the attention) see only real changes. |
| A store error after a provider call | The ledger does not have `done`, or the mirror does not have the new labels | The row stays in `writing` at the number of the call, and the recovery reads the messages of the call. The next sync writes the labels from the history of Gmail. |
| An action applied before the ledger (migration 9) | An applied action with no entry, or a marked action whose calls are not known | Migration 11 adds an entry with no messages and the undo `none`. An action with the mark `applying` becomes `failed`, with no call. |
| The label name of `label` or `unlabel` | The mirror can be one sync interval behind Gmail, and Gmail refuses two labels whose names differ only in case | Before step 2, the apply reads `labels.list` and finds the label of the name, with no regard to case (the Unicode case folding of submit). The mirror gets the label row. |
| A label name that is the name or the ID of a system label at Gmail | A `label` action changes a system label | Submit refuses the name. If Gmail has a system label that submit does not know, the engine denies the action, by `engine`, with no call. |
| A `label` action of a label that Gmail does not have | No label to add | Before step 2, the apply calls `labels.create` one time, with no ledger entry: the label stays after an undo (step 1). After an error or an unknown result, it reads `labels.list` and uses the user label of the name. With no such label, the action is `failed`, with a ledger entry of no message. |
| An `unlabel` action of a label that Gmail does not have | A call that changes nothing | Each message has the target state. The action is `applied` with no write call. |
| A label deleted at Gmail between `labels.list` and `batchModify` | Gmail refuses the call (HTTP 400) | A clear refusal: the row is `failed`. |

### 7.3 Recovery

The engine runs the recovery for a row in the state `writing` after an
unknown result, and at start for each such row. The recovery reads labels at
Gmail. If a read fails, the engine waits and reads again, as for each read
(specification 1, section 4.7). A read is safe to repeat.

For each message of the row:

- `done` is true: skip the message. The provider confirmed it, and a later
  change by the person must stay.
- Its `call` is greater than `call` of the row: the engine never started its
  call. Read its labels again as in step 2 of section 7.2. A message that
  has the target state now is `skipped`, and a gone message is `gone`.
  Else save the new `pre`, and keep the message for step 4 of section 7.2.
- Each other message: its call started, and its result is not known. Read
  the labels at Gmail (`format=minimal`).
  - Gmail returns HTTP 404: the message is gone. It is not in `done`, and the
    engine lists it as gone.
  - The message has the `post` state: set `done`.
  - Each other state, `pre` included: the engine cannot tell if the call did
    not reach the message, or if the person reversed the change in Gmail
    after the call. The engine never calls the provider again for the
    message. It lists the message `unknown` for the person, who can approve
    a new changeset for it.

Then, if messages remain whose call never started, the apply continues with
their calls (section 7.2, step 4). Else, the row becomes `failed` when one
or more messages are listed as not known (`unknown`), and `applied` in each
other case. A gone message does not fail the row, as in step 2 of section
7.2. In both cases, the undo applies only to each message with `done`.

The brief shows the listed messages in one line, until the person dismisses
it. A state verb never stays in the state `writing`, because one transaction
applies it.

### 7.4 Undo

1. The person selects undo on a ledger entry, in the web client. The entry
   must be `applied` or `failed`, with the undo `inverse`, and with one or
   more messages with `done` that are not gone (a state entry: one or more
   items in `post` that are in the store). No other undo of the entry is
   open.
2. The engine (`iris_apply::undo`) makes a changeset with author `person`
   and one action: the inverse of the entry (`archive` and `inbox`, `read`
   and `unread`, `label` and `unlabel` of the same name, `trash` and
   `untrash`, and `restore` for a `close`, `decline`, `defer`, `correct`, `skip`, `mute` or `dismiss`
   entry). Its target is the entry (`{"ledger": ["l-N"]}`), and submit
   resolves the messages with `done` that are not gone, in SQL, so no list
   of message IDs is in memory (section 9). For `restore`, submit resolves
   the items of the entry instead (`action_item`), and the apply runs with
   the state verbs (section 4). Only an action with the link
   of migration 23 to that entry can target an entry. The undo of `label`
   or `unlabel` changes the label of the entry by its ID (from `ops`), so a
   rename of the label at Gmail after the entry does not change the undo.
   An undo does not expire (section 5.5): the person approved it, and
   action 2 must apply also after a long stop.
   One transaction submits it, and the person approves it: the choice of
   the person is the approval. The table `ledger_undo` (migration 23) links
   the action to the entry. The rules, the mode `observe` and the flag
   `bulk` do not apply to an undo. Only Stop (release 4) holds it.
3. The apply reads the labels at Gmail as in step 2 of section 7.2. The
   inverse changes only the messages that still have the `post` state of
   the entry, and on which no later change of Iris stands. A later entry
   with the message listed `unknown` blocks. A later entry with the
   message `done` is a change. The change stands if no undo of it with the
   message `done` stands. So an undo that changed the message back cancels
   the change, an undo of that undo makes it stand again, and each retry
   of an undo counts. An undo entry is not a change by itself, unless it
   undoes an entry before this entry. The actions of the undos of this
   entry do not count. A message that changed after the entry (for
   example, the person moved it in Gmail, or a later action of Iris that
   stands changed it) stays as it is. The
   result counts these messages as unchanged. Known limit (issue #225):
   a change in Gmail outside Iris that puts the message back in the `post`
   state is not detected, and the undo reverses it.
4. For `trash`, the undo changeset has one or two actions, and each one is
   an ordinary action with its own ledger entry and recovery. Action 2
   exists only if one or more messages of action 1 had no `INBOX` in `pre`
   of the entry:
   1. `messages.untrash` for each message that still has `TRASH`. Only an
      undo makes this action, with the verb `untrash`: submit denies
      `untrash` with no link. Its `post` is "no `TRASH`".
   2. `archive` of each message whose `pre` in the entry has no `INBOX`
      (for example an archived message that `messages.untrash` put in the
      inbox). It changes only the messages that an `untrash` of an undo of
      the entry changed (`done`), and that have `INBOX` now. Until action 1
      ends, the apply of action 2 waits. After action 1 ends, `applied` or
      `failed`, action 2 applies: the caller applies each action of the
      undo in order.

   The undo never sets a full label set, so a label that the person added
   while the message was in the trash stays. If the engine stops between the
   two actions, action 2 is still `approved`, and it applies after the start
   (section 7.2, step 2 reads the labels again).
5. Each action of the undo gets its own ledger entry. When the last action
   of the undo becomes `applied`, each action of the undo is `applied`,
   and the undo changed one or more messages (a `done` message in an entry
   of the undo), the same transaction sets `undone_by` of the first entry
   to the entry of action 1, and the trigger `ledger_state` moves its
   action to `undone`. If an action of the undo fails, or the undo skipped
   each message (decision D18, issue #463), the first entry keeps its
   state, and the person can undo it again: the next undo skips each
   message that is back, and its action 2 archives each message that an
   earlier `untrash` changed. The result of the undo counts the skipped
   messages as unchanged. The same rule applies to `restore`: a `restore`
   that changed no item keeps the state entry open.

An undo of an undo is a new action, not a delete of the record.

#### Failure list of the undo

| Input or event | Failure mode | Safe result |
|---|---|---|
| The undo of a state entry whose item changed after the entry (a later action, also with the same values, understanding, the tick of a deferred item, or a gone item) | The undo reverses the later change | The apply of `restore` changes only an item that is not gone, on which no later change of Iris stands (section 4), and whose fields have the `post` values of the entry. It counts each other item as unchanged. A `restore` that changed no item keeps the entry open (decision D18), and is no change that blocks a later undo. It deletes only the correction rows whose IDs the entry holds, so another action of the same changeset keeps its rows. |
| The undo of a `correct` of an array element (`fields.items.0`) | The undo changes nothing and still marks the entry `undone` | The apply sets each field with the path rule of a correction (`iris_talk::set_field`), with array indices. If a path does not fit the item, the item stays as it is and counts as unchanged. If no item changed, the entry keeps its state (decision D18). |
| The undo of a `correct` of a field whose parent was missing (`due.date` with no `due`) | The undo leaves an empty parent, and the item is not valid | The apply removes each parent object that becomes empty, and checks the restored item against the schemas. An item that is not valid stays as it is and counts as unchanged. |
| The undo of a `correct` that changes more than its field (an address of the owner makes the whole party `self`) | The undo sets only the field back, and the party stays `self` | The entry holds the top-level value of the field, and the undo sets the whole value back. |
| The undo of a `correct` of a party address | The party keeps the sender ID of the corrected address | The apply of `restore` sets the sender ID of each party from its address, with the rule of a correction. Migration 26 links `restore` with no arguments only to a state entry, as step 1. |
| An undo of an entry in `writing`, `undone`, or with the undo `none` | The undo changes messages of an apply that did not end, or reverses a change two times | `undo` refuses it (`Error::Undo`). The trigger `ledger_undo_insert` refuses the link. |
| A second undo while an undo of the entry is open | Two undos change the same messages, or a one-action undo ends the entry while action 2 of another undo waits | `undo` refuses it until each action of the open undo ended. The trigger refuses a link while an action of another undo changeset of the entry is open, and a second link of the same step in one changeset. Action 2 of a `trash` undo applies also after a failed action 1, so the undo ends. |
| A link of an action whose target is not the whole entry (`{"ledger": ["l-N"]}`), whose verb is not the inverse of the entry, with other arguments, a step 2 of an entry that is not `trash`, or a step 2 with no step 1 of the same entry in the same changeset | A link lets an action skip the mode, or makes `untrash` for an entry that is not a trash | The trigger `ledger_undo_insert` refuses the link. The end of a ledger entry records an undo only when the entry of step 1 exists, so it never fails on the link. The trigger `ledger_undo_action` refuses a change of the verb, the targets or the arguments of a linked action, also in a draft. Submit denies a linked action with another target, and also denies `untrash` unless the action is step 1 of the undo of a `trash` entry. The apply changes only the messages that the entry changed, whatever the targets. |
| An entry with no message with `done`, or each such message gone | An undo that changes nothing | `undo` refuses it. Nothing is written. |
| An undo that skips each message (a later entry of Iris changed it, or it changed at Gmail) | The undo uses up the undo of the entry, and a later undo of the blocker cannot reach the entry again | The undo action is `applied` with 0 changed messages, and the entry keeps its state (decision D18, issue #463). The person can undo the entry again later. |
| A message that changed at Gmail after the entry | The undo reverses a change of the person | The apply reads the labels at Gmail, and skips the message if it does not have the `post` state. A change outside Iris that ends in the `post` state is not detected (issue #225). |
| A message that a later entry of Iris changed (for example `archive`, `inbox`, `archive`, then the undo of the first `archive`) | The undo reverses the later action | The apply skips each message on which a later change of Iris stands, by the rule of step 3. The check reads only the ledger rows and undo links of that message. So an undo of the newest entry, then an undo of the entry before it, returns each message to its first state (card R2-13). |
| A gone message, or HTTP 404 | A call on a message that does not exist | `undo` leaves a gone message out of the targets. The apply lists a message that Gmail does not have as gone. |
| The mode `observe` | The person cannot reverse a change while Iris observes | The mode does not hold an undo. The apply checks the link of migration 23. |
| Action 2 of the undo of `trash` before action 1 ends | An `archive` of a message that is still in the trash, or that `untrash` did not change | The apply of action 2 waits. Action 2 changes only the messages with `done` in the entry of action 1. |
| A failed undo, or an unknown result, in action 1 or action 2 | Some messages are back, others are not, and the entry shows `undone` | The undo entry is `failed`, with its recovery. The first entry becomes `undone` only when each action of the undo is `applied` and the undo changed one or more messages. Else it keeps its state, and a new undo skips each message that is back. |
| A write of `undone_by` with an entry that is not its undo | The ledger shows a wrong undo | The trigger `ledger_undone_by` refuses it: only the entry of action 1 of its own undo makes an entry `undone`. |
| An undo of `label` or `unlabel` whose label Gmail deleted after the entry | The undo makes again a label that the person deleted, or Gmail refuses the call | Before step 2, the apply reads `labels.list`. If the label ID of the entry is not there, the engine denies the undo action, by `engine`, with no write call. The entry keeps its state. |

### 7.5 Release 3: draft, send and the held outbox

The text of a nudge comes first (R3-07). When a person is late on a
commitment `owed_to_you`, the engine writes the text of a reminder in the
voice of the owner, through the budget of understanding, and saves it as
the reply draft of the thread (`iris_voice::late` gives the rule,
`crates/iris/src/nudge.rs` runs the step). It makes no changeset action and
sends nothing. The person sends it as a reply (`draft` and `send`, through
the held outbox). The verb `nudge` and `iris nudge <c-id>` are not built
yet.

A later specification defines the verbs `draft`, `send` and `nudge`, and the
held outbox. `docs/specs/2026-11-16-send.md` (R3-01) defines `draft`, `send`
and the release step. That specification must meet these constraints:

- A `send` never goes at once. After the approval, it waits in the held
  outbox for the hold delay (default 60 seconds). The outbox survives a
  restart.
- A cancel of a send succeeds only while the send is `approved` or `held`
  and has no ledger row (the release did not claim it). It moves the send
  to `denied` by `person`. Cancel and release run on the one write
  connection, so only one of the two can win.
- The send switch (`docs/specs/2026-11-17-held-outbox.md`) is off until the
  owner turns it on with `iris sending on`. While it is off, no send leaves
  the held outbox. The hold starts at the later of the approval and the
  time the switch went on.
- The approval of a send binds to a hash of the draft content: the
  recipients, the subject, the body and the attachments. Before the release,
  the engine reads the draft at Gmail again. If the hash is different, the
  engine denies the send, and a new send action of the draft goes to
  review. No `draft` action can change a draft that has an
  approved or a held send.
- The flags of a draft (section 5.3) come from the content of the draft, at
  submit and again at the release.
- In the write view of the person (`docs/specs/2026-11-17-write.md`),
  one press of Send approves the `draft` action and the `send` of the
  draft that this action makes. The send approval binds to the hash of
  that draft row.
- `send` accepts only a draft that Iris made. Iris saves the `Message-ID` that
  it sets in the draft before the call.
- The engine never makes the `drafts.send` call again. If the result is not
  known, the engine searches at Gmail, not in the mirror. Until it finds the
  sent message or a set time passes, the send has a state that blocks a new
  send of the same draft.
- The undo of a `draft` action that changes a draft restores the old
  content of the draft. It never deletes a draft of the person. So a
  draft create has no undo, and no `draft` action can change a draft
  until its undo exists (R3-01 makes only creates).
- Each verb records its result in the ledger and has a crash test.

## 8. The iris command

### 8.1 One program, two modes

`iris` is the engine and the command, in one program. The command does not
read the store. It sends each request to the engine through a Unix domain
socket, and the engine does the work. Unix domain sockets work on macOS and on
Linux.

Release 1 exception: release 1 has no socket. The release 1 verbs (`status`,
`search`, `show`, `mirror sync`, `mirror verify`, `backup` and `account add`)
open the data folder directly, as the OS user that owns it, and they do not
need a running engine. `status`, `search` and `show` call the same read
functions as the local API, so they give the same data. Some release 1 verbs
write to the local store, but none of them changes the mailbox:

| Verb | Local writes | Rules |
|---|---|---|
| `account add` | The account row and the sealed sign-in | Specification 1, section 4.9 |
| `mirror sync` | Messages, content files and the sync log. Then one run of understanding: headers, bodies, items, senders, the run row, and the cost of model calls from the approved budget | Specification 1, sections 4.2 to 4.7; `docs/specs/2026-11-16-cost-control.md` |
| `mirror verify` | Repairs of the mirror, and the sync log | Specification 1, section 4.8 |
| `backup` | A copy of the data folder at the given path | Specification 1, section 5.1 |
| `model set` | The model setting and its sealed key | `docs/specs/2026-11-16-cost-control.md` |
| `understand` | Items, run rows, estimate receipts and the budget | `docs/specs/2026-11-16-cost-control.md`. The budget approval needs an estimate receipt and a terminal (a TTY). It is not a mailbox action. See the exception below. |

The policy governs mailbox actions (sections 4 to 6). The writes in
the table are not mailbox actions, so they do not go through the policy.
Release 1 has no verb that adds an action, so no request can skip the policy.
The socket comes with the first verb that adds an action (release 2). Then
each verb that adds, submits or withdraws an action goes through the socket
(`docs/specs/2026-11-16-engine-socket.md`). In host mode, the read verbs
(`status`, `search`, `show`, `list` and `brief`) keep the exception of
release 1: they change nothing, so they cannot skip the policy. In session
mode (release 4), each read verb goes through the session socket, and the
engine applies the current grant of the job of the session
(`docs/specs/2026-11-16-job-workspace.md`). A session never opens the store
directly: the direct path is for host mode only.

Release 3 exception (`docs/specs/2026-11-17-held-outbox.md`, sections 3
and 5): `iris sending on|off`, `iris sending` and `iris cancel <d-N>` open
the data folder directly, in host mode only, and do not need a running
engine. They change no mailbox, so they cannot skip the policy:

| Verb | Local writes | Rules |
|---|---|---|
| `sending on` | The send switch, after a typed `yes` at a terminal | Held-outbox spec, section 3 |
| `sending off` | The send switch, after it holds `apply.lock` (at most 20 seconds) | Held-outbox spec, section 3 |
| `cancel <d-N>` | The open send of the draft becomes `denied` by `person` | Held-outbox spec, section 5 |

One engine runs on a data folder: `iris serve` holds the lock `engine.lock` in the data folder while it
runs.

| Mode | When | Socket | Who |
|---|---|---|---|
| Host mode | No `.iris/` folder in the current folder or above it. | `iris.sock` in the data folder, mode `0600` | The OS user that runs the engine. |
| Session mode (release 4) | The current folder or a folder above it has `.iris/`. | `.iris/socket`, one for each session, mode `0600`, in a `.iris/` folder with mode `0700` | The agent session of one job. |

The socket tells the engine who calls. In session mode, the engine limits each
verb to the job of the session. The socket file is the only capability that a
session gets: it holds no token and no credential. The sandbox of release 4
must make sure that a session cannot reach `iris.sock` or the socket of
another session.

Release 1 exception: `iris understand --approve` approves a model budget in
the terminal. Release 1 has no web write path, and the budget limits money,
not the mailbox. Host mode is the OS user that owns the data folder, which
is the person. In release 1, the person gives no agent host access to that
folder (an agent with host access could approve a budget). Release 2 moves
the approval to the web client (section 8.6).

Release 3 exception: `iris write` (card R3-05) opens the data folder
directly, as `understand` does, and needs no running engine. It writes one
reply draft of a thread (table `reply_draft`), a run row, and the cost of one
model call from the approved budget. It holds the run lock of `understand`
during the call. It is not a mailbox action: the draft stays in the store, and
nothing goes to Gmail. So it does not go through the policy or the socket.
The `draft` verb that adds a Gmail draft action (section 8.2) goes through the
socket.

The command has no verb to approve, to deny or to change the policy. Only the
web client does these steps, for the person. The person has no terminal client
(decision of the maintainer). In host mode, a changeset has the author `local`
and goes through the policy as an agent changeset does. A rule with
`author = "agent"` does not match `local` (section 6.2).

### 8.2 Verbs of the command

| Command | Does | Mode | Release |
|---|---|---|---|
| `iris serve [--addr <addr>]` | Starts the engine. The default address is `127.0.0.1:4800`. The engine refuses an address that is not a loopback address, until open question 1 has a decision. | host | 0 |
| `iris service install` | Installs and starts the background service that runs `iris serve`: a launchd agent on macOS, a systemd user unit on Linux. If a call to the service manager fails or its result is not known, the service file stays and the command returns the error. The state of the service manager is then not known: a second `install` finishes the work, and `remove` undoes it. Both are safe to repeat. | host | 1 |
| `iris service remove` | Stops the background service and removes its file. If the stop fails, or if launchd cannot say if the agent runs, the file stays and the command returns the error. A second remove tries again. On Linux with no running systemd (`/run/systemd/system` is absent), nothing can run the unit, so it deletes the file with no `systemctl` call. If systemd runs and the stop fails (also when `systemctl` is not found), the file stays and the command returns the error. After a good stop, a failed `systemctl --user daemon-reload` is not an error, because the unit is stopped and disabled. | host | 1 |
| `iris status` | Host mode: shows the account, the sync, the mode and the counts of the mailbox. Session mode: shows only the job (its ID, goal, state and due date) and the counts of the workspace. It shows no account, sync or mailbox count. | both | 1 |
| `iris mirror sync` | Runs an incremental sync now, then one run of understanding inside the approved budget. If another run holds the run lock, it waits for that run to end. | host | 1 |
| `iris mirror verify` | Compares the mirror with Gmail (specification 1, section 4.8). | host | 1 |
| `iris rebuild [--since-prompt <v>]` | Runs understanding again. | host | 1 |
| `iris model set`, `iris model show` | Sets the model of understanding and its key (read from standard input), or shows the settings without the key (`docs/specs/2026-11-16-cost-control.md`). | host | 1 |
| `iris zone [<name>]`, `iris zone --computer` | Shows or sets the time zone of defer dates and due dates: an IANA name, for example `Europe/Berlin`. `--computer` uses the time zone of the computer again (the default). Decision D1. | host | 1 |
| `iris understand --estimate`, `iris understand --approve <code> --budget <usd>`, `iris understand [--max-cost <usd>] [--max-messages <n>]` | Shows the estimate and its approval code, approves a budget for paid runs after the estimate, and runs understanding on the pending messages. | host | 1 |
| `iris backup <path>` | Copies the data folder while the engine runs. | host | 1 |
| `iris export <path>` | Writes each item as JSON Lines, and copies the raw messages. | host | 1 |
| `iris brief` | Shows the brief. | host | 1 |
| `iris search <query>` | Full-text search (specification 1, section 5.6). In session mode, it searches only the messages that the job grants (`docs/specs/2026-11-16-job-workspace.md`), and it adds nothing to the workspace. | both | 1 |
| `iris show <id>` | Shows one item or message. Release 1 has no read path for a thread or a sender, so a thread and a sender come in a later release. In session mode, only an item or a message that the job grants, and an item is a job item (`id`, `form` and `sources`, with no text of the model and no status). | both | 1 |
| `iris list [<form>] [--status <s>] [--type <t>] [--before <id>] [--limit <n>]` | Lists items, newest first, of one form if given. In host mode, the filters and the page of `GET /api/items` (issue #523): `--type` needs the form `record`, `--before` takes the last ID of the page before, and `--limit` is 1 to 200 (default 50). The text form ends with the `--before` value of the next page. In session mode, only the items of the workspace, with the fields of `show` (card R4-04f), and only the form: a filter is a usage error. | both | 1 |
| `iris archive`, `inbox`, `label`, `unlabel`, `read`, `unread`, `trash` `<ids> [--changeset <cs-id>]` | Adds an action to a draft changeset. | both | 2 |
| `iris close`, `defer`, `decline`, `correct` `<id> [--changeset <cs-id>]` | Adds a state action to a draft changeset. | both | 2 |
| `iris skip`, `mute`, `dismiss` `<id> [--changeset <cs-id>]` | Adds a state action to a draft changeset. A session gets code 3: a job grants no feed, conversation or signal item. | host | 5 |
| `iris changeset <cs-id>` | Shows a changeset. | both | 2 |
| `iris revise <cs-id>` | Starts a new draft revision of a submitted changeset of the same author, after a review comment (section 5.5). | both | 4 |
| `iris submit <cs-id> --why <text>` | Submits a draft changeset, and prints one result for each action. | both | 2 |
| `iris withdraw <cs-id>` | Withdraws a changeset of the same author (section 5.5). Applied actions stay. | both | 2 |
| `iris ledger [filters]` | Lists ledger entries. | host | 2 |
| `iris draft <thread> <file>`, `iris send <draft>`, `iris nudge <c-id>` | Adds write actions. | both | 3 |
| `iris write <t-id or m-id> [--intent <text>]` | Writes a draft of a reply in the voice of the owner with one model call, saves it in the store and prints it. It sends nothing and adds no action. It opens the store directly (section 8.1). | host | 3 |
| `iris wait`, `iris ask` | Ends the session and waits, or asks the person. | session | 5 |

Without `--changeset`, a command that adds an action makes a new draft
changeset, and prints its ID. Two callers in host mode look the same to the
engine, so each caller names its own draft changeset. In session mode, the
session has one draft changeset, and `--changeset` is not necessary.

Each command that adds an action prints the changeset ID and the action
number, and changes nothing outside the draft changeset. Nothing leaves the
engine before `submit`.

In session mode, each message that an action changes must be in the
current grant of the job (`docs/specs/2026-11-16-job-workspace.md`): the
engine checks each message that a target resolves to, at `submit` and again
just before the apply. A message outside the grant at `submit` makes the
submit fail with no change. A message that left the grant before the apply
makes the action refused, with the reason. Each item target must be an item
of the workspace now, by the same two checks. `iris send` and `iris nudge` are
refused in session mode, and `submit` refuses each send action in a
changeset of a session. `iris draft` in session mode makes only a reply in a
granted thread (decision D3): `submit` refuses a new mail and a reply whose
parent message is not in the grant.

### 8.3 Output

- The default output is short text for a person or an agent to read. In a
  list (for example `iris search`), each line starts with an ID, so the next
  command can use it. A command that prints one object or a summary (for
  example `iris show`, `iris status` and `iris mirror`) starts each line with
  a label: a field name, such as `ID:` or `Mode:`, or `account <ID>` for a
  line about one account. The text of a message comes after its fields. A
  program reads these commands with `--json`, where the command has it.
- `--json` prints the open model documents (specification 1, section 6, and
  section 5.1 here). One JSON document for one object, and JSON Lines for a
  list. Exception: in session mode, an item is a job item
  (`jobitem.schema.json`, `docs/specs/2026-11-16-job-workspace.md`), not the
  full item document.
- The command writes errors to standard error. An error message says what
  failed and what to do.

### 8.4 Exit codes

| Code | Meaning |
|---|---|
| 0 | The command succeeded. For `submit`: each action is applied. |
| 1 | An error in the engine or at the provider. For `submit`: one or more actions are `failed`. |
| 2 | The command is not correct (usage). |
| 3 | The policy or a hard limit denied one or more actions. |
| 4 | The engine is not reachable. Release 1 never gives this code, because its verbs do not use the engine (section 8.1). |
| 5 | For `submit`: one or more actions are open (`proposed`, `in_review`, `approved` or `held`), or a ledger row is in recovery. Nothing failed and nothing was denied. |

If more than one code applies, the command exits with the first code of this
order: 2, 4, 1, 3, 5, 0. The printed result gives the state of each action.

### 8.5 MCP, the second interface (release 5)

The engine offers the same verbs as Model Context Protocol (MCP) tools, for an
agent that does not work in a folder. The rules for the MCP interface:

- `iris mcp` serves the tools on standard input and output
  (`crates/iris-mcp`, card R5-11).
- One MCP tool for each verb of the command, with the same name and the same
  arguments. A verb with subcommands gives one tool for each subcommand,
  with an underscore (`watch_add`, `watch_list`, `watch_remove`,
  `matters_list`, `matters_show`, `matters_propose`). `iris changeset --drafts` is the tool
  `drafts`, and the file of `iris draft` is the object `draft`. MCP gives no
  verb that the command does not have. It gives no verb of the person
  (`sending`, `cancel`), no verb of the host (`serve`, `account`,
  `mirror`, `backup`, `export`, `service`, `model`, `understand`, `mcp`),
  and not `write` (the draft in your voice, R3-05) or `matters propose`
  with no title, which spend model budget. `matters_propose` takes a title
  and items only: it proposes a matter of these items for the person
  (docs/specs/2026-11-24-matters-edit.md). A test checks
  that each verb is a tool or is in this list.
- Each tool call takes the path of its verb in the same mode, so MCP adds
  no power. In host mode, these are the paths:

  | Tools | Path |
  |---|---|
  | `status`, `search`, `show`, `list`, `matters_list`, `matters_show` | Read the store directly (section 8.1). |
  | `watch_add`, `watch_list`, `watch_remove` | Read and write the watch rows of the store directly, as `iris watch` does. A watch changes no mailbox, so it does not go through the policy. |
  | All other tools | A request on `iris.sock`, with the author `local`. |

  In
  session mode, the MCP interface uses the session socket, so the session
  limits apply. The command has no session mode yet, so `iris mcp` has
  host mode only (issue #440).
- The result of a call is what its verb prints on standard output: with
  `--json` where the verb has it (section 8.3), else the text of the verb.
  Two exceptions:

  | Tools | Result |
  |---|---|
  | `watch_add`, `watch_remove` | The text of the verb (`w-1: Iris tells you ...`, `w-1 removed.`), because the verb has no `--json`. The line starts with the watch ID. |
  | Each tool that adds an action | The JSON of the engine answer (`{"changeset": ..., "n": ...}`). The verb prints `<cs-id> <n>` and has no `--json`. |

  `matters_list`, `matters_show` and `matters_propose` give JSON, because
  the verb always prints JSON. A verb that ends with a code that is not 0 gives an error
  result: the printed text, then the error line of standard error
  (`iris: <error>`), then `exit code N` (section 8.4).

Failure list of `iris mcp` (the client is input that Iris does not
control):

| Input or event | Result |
|---|---|
| Supported input: JSON-RPC 2.0 messages of MCP, one on each line, at most 2 MiB a line. A tool call names a tool of `tools/list` with the arguments of its schema. | The request of the verb, as above. |
| A line that is not JSON, or a line over 2 MiB | The server drops the line, answers nothing and reads the next line. No request goes to the engine. |
| A tool that does not exist | An error of the protocol (`invalid params`). No request goes to the engine. |
| A missing, unknown or wrong argument | An error result with code 2. No request goes to the engine. |
| Calls at the same time | At most 4 calls run at the same time. The others wait. Each call is one request on the socket, and the engine orders the writes. Each add with no `changeset` makes its own draft. |
| The answer of the engine is lost (time-out, the engine stops) | Code 1, "the result is not known", as for the command. The client reads the state with `changeset` or `drafts`. |
| The client stops or closes standard input during a call | The server stops after the running calls end (at most the answer wait of 30 seconds). The answer of a running call is lost. The client reads the state with `changeset` or `drafts`. |
| A retry of `submit` after a lost answer | The engine refuses a changeset that is not a draft (code 2) and changes nothing. `changeset` gives the state. |
| A retry of an add after a lost answer | Can add a second action. Before a retry, the client reads the draft with `changeset`. Nothing leaves before `submit`. |
| All other input | An error result or an error of the protocol. No request goes to the engine. |

### 8.6 The web client and the trust boundary

The web client is the only place to approve, deny, undo and change the
policy. The one exception is the model budget of release 1 (section 8.1). The engine protects its web interface:

- At each start, the engine makes a random web secret of 32 bytes and a
  one-time code. `iris serve` prints a one-time sign-in address:
  `http://<address>/#code=<code>`. Release 1 does not open the browser,
  because the engine often runs as a background service with no screen
  (`iris service install`). The person opens the address. `iris open` asks
  the engine socket (verb `sign-in`) for a new one-time code with a life of 2
  minutes, and opens its address in the browser
  (`docs/specs/2026-11-17-mac-app.md`). The browser does
  not send the fragment to a server. The client exchanges the code one time
  (`POST /api/session`) for the web secret, keeps the secret in the storage
  of its origin, and sends it as `Authorization: Bearer <secret>`. The engine
  refuses each request of the web interface without that header. The engine
  does not use a cookie: a browser sends a cookie to each port of the host, so
  a server of another OS user on another loopback port could get it.
- The engine refuses a request whose `Host` header is not the bound address,
  and a request whose `Origin` header is another origin. So a web page cannot
  send a request to the engine, also through DNS rebinding.

The engine trusts the OS user that runs it. A process of that user outside a
sandbox can read the data folder: the key, `iris.db`, the policy file and the
web secret. Iris cannot stop such a process. So C2 and C6 hold for an agent
in a session sandbox (release 4), or for an agent that runs as another OS
user. In releases 2 and 3, an agent with a shell must run in a sandbox or as
another OS user, if it must not approve. The documentation must say this.

## 9. Memory

- The policy holds a few hundred rules at most. The engine keeps the parsed
  policy in memory, under 1 MB.
- The ledger and the changesets are rows in SQLite. The engine keeps no list
  of them in memory.
- The message IDs of an action are rows of child tables: one row for each
  message of `resolved`, and one `ledger_message` row for each message of a
  ledger entry. The engine reads them page by page, and writes a changeset
  document by streaming. A change of `done` changes only the rows of one
  call, so the cost of an apply grows with the number of messages, not with
  its square.
- Apply sends at most 1,000 message IDs in one `batchModify` call (the Gmail
  limit) and streams larger actions in pages.

## 10. Test plan

- For each verb of release 2: apply at the fake Gmail server, verify the label
  sets or the item fields, undo, and verify again (requirement C3).
- A crash test for each case of section 7.3 (requirement C5). The tests
  include a provider call that succeeds before the ledger update, a crash
  between two `batchModify` calls of an action with more than 1,000
  messages, and a crash in the middle of a `trash` of more than one message.
  After the start, the calls that never started run, and their messages are
  not listed for the person.
- A policy test for each step of section 6.4, and for each hard limit.
- A test with a thread of 10 old messages, one from a sender of a `deny`
  rule. An `allow` rule matches each message. The action is `denied`.
- A test that `label INBOX` and `unlabel UNREAD` are refused.
- A test that changes the policy file on disk: no new rule applies before the
  person confirms the file. A test with a policy file that is not valid: the
  active policy stays.
- A test where the fake Gmail server applies a `batchModify` call and then
  drops the connection. The recovery sets `done` for each message, the row
  becomes `applied`, and the undo applies to the messages.
- A crash test where the person changes a message with `done` before the
  start: recovery does not change it.
- A crash test where the call applied, and the person reversed the change in
  Gmail before the start, so the message has the `pre` state: recovery lists
  the message and does not call the provider.
- A test that the `iris` command has no verb that approves, denies or changes
  the policy.
- A test that the web interface refuses a request without the cookie, and a
  request with another `Origin`. A test that `iris serve --addr 0.0.0.0:4800`
  does not start.
- A test that an approval of revision 1 is refused while revision 2 is a
  draft and after revision 2 is submitted, and that the `proposed` and
  `in_review` actions of revision 1 are `replaced`. A test that an
  `approved` action of revision 1 that comes back to review after revision 2
  can get the approval of the person, with its hash (section 5.5).
- A test that an undo skips a message that changed after the entry, and that
  an undo applies in the mode `observe`.
- A test that trashes an archived message and undoes the trash: the message
  has no `INBOX` after the undo. A crash test stops the engine between the
  two actions of the undo: after the start, the message has no `INBOX`.
- A test where a `trash` call applies, the answer is lost, and the person
  adds a label before the recovery. After the undo, the label stays.
- A test with an action that an `allow` rule approved. The person changes the
  mode to `observe`, removes the rule, and changes the mode back to `apply`.
  The action goes to `in_review` and does not apply.
- A test that marks a message read at the fake server after the last sync,
  then applies `read` and the undo. The message must stay read.
- A test that deletes a message at the fake server between submit and apply:
  the apply skips it and lists it.
- A test of each exit code of section 8.4, and of the order.
- A test with 10 actions of 100 messages in one changeset: the flag `bulk`
  sends them to review.
- A test with two changesets of the same author, 400 messages each, that an
  `allow` rule approves before either applies. The first applies. The second
  gets the flag `bulk` at apply and goes to `in_review`.
- A test that `close`, then `iris rebuild`, keeps the item with the status
  `done`, and that the undo gives back the old status.

## 11. Open questions

1. Access to the web client from another device. Release 1 binds the engine to
   `127.0.0.1`. A phone on the home network needs a sign-in to the engine.
   The method (a pairing code, a passkey) needs its own decision before the
   engine accepts other addresses.
2. Label names. A label that an agent creates is a new label at Gmail. Version
   0 lets `label` create labels. A rule could limit this later.
3. The format of the hold delay per recipient. Version 1 has one delay for all
   sends.

## Assumptions

- A person can change a message in Gmail between the read of step 2 and the
  provider call of step 4 in section 7.2. The window is the time of one
  provider call. If the person changes a label of the action in that window,
  the undo can reverse that change. Version 1 accepts this risk.
- Gmail `messages.batchModify` accepts up to 1,000 message IDs (Gmail
  reference, checked on 2026-11-15), and a repeat with the same labels has
  the same result.
- Gmail does not run a request that it answers with HTTP 401, HTTP 429 or a rate-limit HTTP 403.
  Verify with the fake server and on the owner's Mac before the apply code is
  written.
- `labels.create` for a name that exists returns an error (HTTP 409). Verify
  before the apply code is written. Step 1 of section 7.2 calls `labels.list`
  for each error, so the design does not depend on the code.
- `messages.trash` and `messages.untrash` change only `TRASH` and `INBOX`.
  Verify on the owner's Mac before the apply code is written. If they change
  another label, the undo of section 7.4 changes.
- Gmail deletes a message in the trash for good after 30 days (Gmail Help
  Center).
- The scope `gmail.modify` allows label changes, `messages.trash`,
  `drafts.create` and `drafts.send`, and does not allow a permanent delete of
  a message (Gmail scopes page, checked on 2026-11-15). It allows
  `drafts.delete`, which is a permanent delete of a draft, so the release 3
  specification must not delete a draft of the person.
- One person uses one install. The policy has no users and no roles.
- The person reviews in the web client. There is no review in the terminal
  and none by mail.
- A Unix domain socket is enough to tell host mode from session mode. The
  sandbox of release 4 must make sure that a session cannot reach
  `iris.sock` in the data folder.
- TOML is a format that the person can edit by hand. If it causes errors in
  practice, a later version can add a form in the web client, and keep the file
  as the source.
