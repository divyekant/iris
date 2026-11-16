# Iris specification 2: changeset, policy, ledger and the iris command

- Status: draft, for the approval of the maintainer
- Applies to: release 2 ("Apply"). Parts for releases 3 to 5 are marked.
- Open model version: 0 (draft)
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
| `draft` | thread, or none for new mail | file with the text | a later specification (section 7.5) | a later specification | 3 |
| `send` | a draft | none | `drafts.send`, after the hold (section 7.5) | none: final after the hold | 3 |
| `nudge` | commitment `owed_to_you` | none | makes a `draft` action, then a `send` action | as `draft` and `send` | 3 |
| `wait` | thread, or a date | until | none | not applicable | 5 |
| `ask` | none | question, options | none | not applicable | 5 |

Iris has no verb for a permanent delete, and no verb to empty the trash.
Gmail deletes a message in the trash for good after 30 days. After that time,
the undo of `trash` is not possible, and the ledger shows the entry as final.

`label` and `unlabel` accept only a label of kind `user` (specification 1,
section 5.5). At submit, the engine refuses a label name that is the same as
the name or the ID of a system label, with no regard to case. A system label (`INBOX`, `UNREAD`, `SPAM`, `TRASH` and the
others) changes only through its own verb, so that no `label` action can go
past a rule or a hard limit that names that verb.

A verb that changes Iris state (`close`, `defer`, `decline`, `correct`) goes
through a changeset too. This gives each change one path, one review and one
ledger. The state verbs set these values:

| Verb | Sets |
|---|---|
| `close` | `status` `done` |
| `decline` | `status` `declined` |
| `defer` | `status` `deferred` and `deferred_until` |
| `correct` | The field of the item. `correct` changes only items. A person changes the kind of a sender only in the web client (specification 1, section 6.7). |

When an action of a state verb applies, the engine writes in one SQLite
transaction:

- A `correction` row with `by` set to the changeset ID (specification 1,
  section 5.5). So the item is pinned, and a rebuild keeps the change.
- A ledger row with the state `applied`, the old values as `pre` and the new
  values as `post` (section 7.1).

The undo of the action sets the old values and deletes the correction row, in
one transaction, with its own ledger row. If the current value is not the
`post` value of the entry (a later action changed it), the undo changes
nothing and lists the item. After the undo of a `correct` on a record with no
correction left, the status of the record goes back to `kept`.

`correct` cannot change `status`, a common field of section 6.1 of
specification 1, or another field that the engine makes from the store
(specification 1, section 7.5). The engine validates the corrected document against the
schemas at submit, and denies an action that gives a document that is not
valid.

## 5. The changeset

### 5.1 Format

A changeset is one JSON document. This is the format of version 0. The JSON
Schema of the changeset comes with the release 2 code, from this section.

```json
{
  "model": "iris/0",
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
      "verb": "archive",
      "targets": { "threads": ["t-7712", "t-7713"] },
      "resolved": { "messages": ["m-50101", "m-50102", "m-50140"] },
      "args": {},
      "why": "Two newsletters from last week.",
      "flags": [],
      "decision": { "state": "applied", "by": "rule", "rule": 1, "policy": 3, "at": "2026-11-16T09:17:41Z" },
      "ledger": "l-901"
    },
    {
      "n": 2,
      "verb": "label",
      "targets": { "threads": ["t-8841"] },
      "resolved": { "messages": ["m-55210", "m-55211"] },
      "args": { "label": "Finance" },
      "flags": [],
      "decision": { "state": "in_review", "reason": "No rule allows label for this sender." }
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
| `state` | `draft` (before submit), `open` (one or more actions are open, section 5.2), `closed` (no action is open) or `withdrawn`. |
| `actions[].n` | The number of the action in the changeset, from 1. |
| `actions[].targets` | What the author named: `threads`, `messages` or `items`. |
| `actions[].resolved` | The message IDs at submit. Review shows these, and apply uses only these. The engine keeps them in a child table, and writes the document by streaming (section 9). |
| `actions[].flags` | Why a person must look: see section 5.3. |
| `actions[].decision.state` | See section 5.2. |
| `actions[].decision.rule`, `actions[].decision.policy` | For a decision by a rule: the rule `id` and the policy version (section 6.3). Together they name one rule text. |
| `actions[].ledger` | The ledger entry, after apply. |

### 5.2 States of an action

| State | Meaning | Next states |
|---|---|---|
| `proposed` | Submitted, and the policy did not decide it, or the mode is `observe`. | `approved`, `in_review`, `denied`, `expired`, `cancelled`, `replaced` |
| `in_review` | Waits for a person. | `approved`, `denied`, `expired`, `cancelled`, `replaced` |
| `approved` | An `allow` rule (`decision.by` `rule`) or a person approved it. The engine applies it next. | `applied`, `failed`, `held`, `in_review`, `denied`, `expired`, `cancelled` |
| `held` | A send in the held outbox (release 3). | `applied`, `failed`, `cancelled` |
| `applied` | The provider accepted the change. The ledger has the entry. | `undone` |
| `failed` | The provider refused it, or recovery could not confirm each message. The ledger lists the messages that changed. | `undone`, only when the ledger lists one or more changed messages |
| `denied` | The policy, a hard limit or a person refused it. `reason` says why. | none |
| `expired` | No decision or no apply in 14 days after submit (section 5.5). | none |
| `cancelled` | The author withdrew the changeset, or a person cancelled a held send. | none |
| `replaced` | A new revision replaced the action before a decision. | none |
| `undone` | The undo is applied. The undo has its own ledger entry. | none |

The states `proposed`, `in_review`, `approved` and `held` are open. Each other
state is decided.

### 5.3 Flags

The engine adds flags. An author cannot remove a flag.

| Flag | When |
|---|---|
| `new_recipient` | A `send` goes to an address that the owner never sent mail to. |
| `outside_thread` | A `send` goes to an address that is not in the thread, after the session read external mail (release 4). |
| `money` | The text of a draft promises an amount of money. |
| `date` | The text of a draft promises a date. |
| `attachment` | The text refers to an attachment, and none is attached, or an attachment is attached. |
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

If the policy mode is `observe`, step 5 records what the policy would do, and
each action stays `proposed`. The triage changeset of release 2 works in this
way. Release 1 computes the triage proposal and does not store it
(specification 1, section 8).

A change of the mode does not evaluate open actions again:

- From `observe` to `apply`: each `proposed` action stays `proposed`. The
  person can approve or deny it in the web client.
- From `apply` to `observe`: an `approved` action waits, and applies after a
  change back to `apply`. A ledger row in the state `writing` continues to its
  end (section 7.3).

The engine applies one action at a time, on the one write connection. Before
the engine applies an action that a rule approved, it computes the flag
`bulk` again and evaluates the action again against the active policy
(section 6.4). For the count of the last hour, `bulk` counts the messages of
each ledger row of the same author that a rule decided in that hour, in the
state `writing`, `applied` or `failed`. If the result is not `allow`, the
action gets the new result: `in_review` or `denied`. So a rule that the
person removed while the action waited applies nothing, and two changesets
that a rule approved at the same time cannot together go past the limit of
500 messages.

### 5.5 Review, revision, withdraw and expiry

- The person reviews in the web client. The review shows each action fully
  resolved: the real messages, the real recipients and the final text.
- The person can approve all, approve one, edit the text of a draft, comment
  or deny.
- An approval names the changeset, the revision, the action number, and the
  SHA-256 hash of `resolved` and `args` that the person saw. The engine
  refuses an approval that does not match the current revision and hash.
- A comment goes back to the author. In release 4, the comment arrives as a
  file in the workspace. The author submits a new revision.
- `iris withdraw` withdraws a changeset. The engine accepts it only from the
  author of the changeset: the same `kind`, `name` and `session`. The person
  can withdraw each changeset in the web client. Each open action becomes
  `cancelled`, except an action with a ledger row in the state `writing`,
  which continues to its end. Applied actions stay.
- 14 days after submit, each open action that is not `held`, and has no
  ledger row in the state `writing`, becomes `expired`. The changeset is `closed` when no action is open.

## 6. The policy file

### 6.1 Place and format

The policy is the file `policy.toml` in the data folder. TOML is a plain text
format that a person can read and edit, and comments in the file stay.

```toml
# Iris policy. Version 0 (draft).
version = 0

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
| `effect` | yes | `allow`, `review` or `deny` (actions); `hide` or `bundle` (attention). |
| `verbs` | for `allow`, `review` and `deny` | The verbs of section 4. |
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

Version 0 has no other condition, and no expression language. A new condition
needs a change to this specification.

Each condition matches one message. For an action on an item, the message is
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
- A `hide` or a `bundle` rule has `verbs`, `author` or `job`. An item has no
  author and no job, so such a rule could never match.
- The file is larger than 256 KiB, or has more than 1,000 rules (section 9).

An `allow` rule never matches an action with no message.

### 6.3 Who changes the file

- Only the person changes the policy. The person edits the file, or uses the
  web client.
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

### 6.5 Hard limits

The engine enforces these limits before the policy. They are in the code, not
in the file.

| Limit | Result |
|---|---|
| A `send` never goes at once. | It waits in the held outbox for the hold delay, after the approval (release 3). |
| A `send` with the flag `new_recipient`, `outside_thread`, `money` or `date`. | Review, even if a rule allows it. |
| An action with the flag `bulk`. | Review, even if a rule allows it. |
| An action by an author other than `person`, whose target has hostile mail: a message with a hostile mark, or a message from a `hostile` sender (specification 1, section 6.7). For a thread or a draft target, each message of the thread counts. | Only `archive`, `trash` and `label`. Other verbs are denied. |
| Stop is on (release 4). | No provider write. Actions in the outbox wait. |
| Mode is `observe`. | Nothing applies, except an undo by the person (section 7.4). |
| A permanent delete. | No verb exists. |

The hard limits treat `local` and `triage` as agents.

### 6.6 Proposed rules (release 5)

Iris proposes a rule after a pattern: for example, the person approved 14 of
14 `archive` actions for the same sender kind. The proposal is a row in the
store, not a line in the file. When the person accepts it, the engine adds the
rule to the file with `by = "proposal"`. A proposal never changes the file
without the person.

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
| `changeset`, `revision`, `action` | The action that the entry applies. |
| `verb` | The verb. |
| `author` | The author of the changeset. |
| `decided_by` | `rule` with its ID and the policy version (section 6.3), or `person`. |
| `ops` | The provider operations, as JSON: the labels to add and remove, and the number of calls. For a state verb, the field. |
| `pre`, `post` | For a state verb only: the old and the new values. |
| `call` | The number of the last provider call that the engine started, from 1. 0 before the first call. |
| `state` | `writing`, `applied`, `failed` or `undone`. |
| `history_id` | The Gmail history ID after the apply. |
| `undo` | `inverse` (an undo exists) or `none` (final, for example a sent message). |
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

### 7.2 Apply

1. If the action adds a user label that the mirror does not have, call
   `labels.create`. If Gmail answers that the label exists, or the result is
   not known, call `labels.list` and use the user label with that name. If
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
5. After the last call, set the state `applied` if each message has `done`,
   else `failed` (section 7.3). Save the history ID. Update the mirror with
   the new labels.
6. Act on each other result of a call as follows:

| Result | Meaning | The engine |
|---|---|---|
| HTTP 401 | Gmail did not run the request. | Gets a new access token one time, and makes the call again one time. If the second call also gets 401, the engine stops the apply as for a clear refusal, and the account goes to "needs sign-in". |
| HTTP 429, or HTTP 403 with the reason `rateLimitExceeded` or `userRateLimitExceeded` | Gmail did not run the request. | Waits as for a read (specification 1, section 4.7), and makes the call again. |
| HTTP 400, another 403, 404, 409 or another 4xx | A clear refusal. | Sets the state `failed` and saves the error. The messages with `done` keep the change, and the undo applies to them. The engine makes no later call of the action. |
| A timeout, a lost connection or HTTP 5xx | The result is not known. Gmail can have applied the call. | Keeps the row in the state `writing`, and runs the recovery of section 7.3 for the row at once. |

While a ledger row is in the state `writing`, the engine applies no other
action that has a message of that row. The other action waits.

### 7.3 Recovery

The engine runs the recovery for a row in the state `writing` after an
unknown result, and at start for each such row. The recovery reads labels at
Gmail. If a read fails, the engine waits and reads again, as for each read
(specification 1, section 4.7). A read is safe to repeat.

For each message of the row:

- `done` is true: skip the message. The provider confirmed it, and a later
  change by the person must stay.
- Its `call` is greater than `call` of the row: the engine never started its
  call. Read its labels again as in step 2 of section 7.2, save the new
  `pre`, and keep the message for step 4 of section 7.2.
- Each other message: its call started, and its result is not known. Read
  the labels at Gmail (`format=minimal`).
  - Gmail returns HTTP 404: the message is gone. It is not in `done`, and the
    engine lists it as gone.
  - The message has the `post` state: set `done`.
  - Each other state, `pre` included: the engine cannot tell if the call did
    not reach the message, or if the person reversed the change in Gmail
    after the call. The engine never calls the provider again for the
    message. It lists the message for the person, who can approve a new
    changeset for it.

Then, if messages remain whose call never started, the apply continues with
their calls (section 7.2, step 4). Else, the row becomes `applied` when each
message has `done`, and `failed` when one or more messages are listed. In
both cases, the undo applies only to each message with `done`.

The brief shows the listed messages in one line, until the person dismisses
it. A state verb never stays in the state `writing`, because one transaction
applies it.

### 7.4 Undo

1. The person selects undo on a ledger entry, in the web client.
2. The engine makes a changeset with author `person` and one action: the
   inverse of the entry. The choice of the person is the approval. The rules,
   the mode `observe` and the flag `bulk` do not apply to an undo. Only Stop
   (release 4) holds it.
3. The engine reads the labels at Gmail as in step 2 of section 7.2. The
   inverse changes only the messages with `done` that still have the `post`
   state of the entry. A message that changed after the entry (for example,
   the person moved it in Gmail) stays as it is. The result lists these
   messages.
4. For `trash`, the undo changeset has two actions, and each one is an
   ordinary action with its own ledger entry and recovery:
   1. `messages.untrash` for each message that still has `TRASH`. Only an
      undo makes this action, with the verb `untrash`. Its `post` is "no
      `TRASH`".
   2. After action 1, `archive` for each message that has `INBOX` now and
      had no `INBOX` in `pre` (for example an archived message that
      `messages.untrash` put in the inbox).

   The undo never sets a full label set, so a label that the person added
   while the message was in the trash stays. If the engine stops between the
   two actions, action 2 is still `approved`, and it applies after the start
   (section 7.2, step 2 reads the labels again).
5. The undo gets its own ledger entry. The first entry gets `undone_by`, and
   its action gets the state `undone`.

An undo of an undo is a new action, not a delete of the record.

### 7.5 Release 3: draft, send and the held outbox

A later specification defines the verbs `draft`, `send` and `nudge`, and the
held outbox. That specification must meet these constraints:

- A `send` never goes at once. After the approval, it waits in the held
  outbox for the hold delay (default 60 seconds). The outbox survives a
  restart.
- A cancel of a held send succeeds only while the send is held. Cancel and
  release run on the one write connection, so only one of the two can win.
- The approval of a send binds to a hash of the draft content: the
  recipients, the subject, the body and the attachments. Before the release,
  the engine reads the draft at Gmail again. If the hash is different, the
  send goes back to review. No `draft` action can change a draft that has an
  approved or a held send.
- The flags of a draft (section 5.3) come from the content of the draft, at
  submit and again at the release.
- `send` accepts only a draft that Iris made. Iris saves the `Message-ID` that
  it sets in the draft before the call.
- The engine never makes the `drafts.send` call again. If the result is not
  known, the engine searches at Gmail, not in the mirror. Until it finds the
  sent message or a set time passes, the send has a state that blocks a new
  send of the same draft.
- The undo of a `draft` action restores the old content of the draft. It
  never deletes a draft of the person.
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
each verb goes through the socket.

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
| `iris status` | Shows the account, the sync, the mode and the counts. | both | 1 |
| `iris mirror sync` | Runs an incremental sync now, then one run of understanding inside the approved budget. If another run holds the run lock, it waits for that run to end. | host | 1 |
| `iris mirror verify` | Compares the mirror with Gmail (specification 1, section 4.8). | host | 1 |
| `iris rebuild [--since-prompt <v>]` | Runs understanding again. | host | 1 |
| `iris model set`, `iris model show` | Sets the model of understanding and its key (read from standard input), or shows the settings without the key (`docs/specs/2026-11-16-cost-control.md`). | host | 1 |
| `iris understand --estimate`, `iris understand --approve <code> --budget <usd>`, `iris understand [--max-cost <usd>] [--max-messages <n>]` | Shows the estimate and its approval code, approves a budget for paid runs after the estimate, and runs understanding on the pending messages. | host | 1 |
| `iris backup <path>` | Copies the data folder while the engine runs. | host | 1 |
| `iris export <path>` | Writes each item as JSON Lines, and copies the raw messages. | host | 1 |
| `iris brief` | Shows the brief. | both | 1 |
| `iris search <query>` | Full-text search (specification 1, section 5.6). In session mode, it adds the threads that match to `threads/`. | both | 1 |
| `iris show <id>` | Shows one item or message. Release 1 has no read path for a thread or a sender, so a thread and a sender come in a later release. | both | 1 |
| `iris list <form> [filters]` | Lists items of one form. | both | 1 |
| `iris archive`, `inbox`, `label`, `unlabel`, `read`, `unread`, `trash` `<ids> [--changeset <cs-id>]` | Adds an action to a draft changeset. | both | 2 |
| `iris close`, `defer`, `decline`, `correct` `<id> [--changeset <cs-id>]` | Adds a state action to a draft changeset. | both | 2 |
| `iris changeset <cs-id>` | Shows a changeset. | both | 2 |
| `iris submit <cs-id> --why <text>` | Submits a draft changeset, and prints one result for each action. | both | 2 |
| `iris withdraw <cs-id>` | Withdraws a changeset of the same author (section 5.5). Applied actions stay. | both | 2 |
| `iris ledger [filters]` | Lists ledger entries. | host | 2 |
| `iris draft <thread> <file>`, `iris send <draft>`, `iris nudge <c-id>` | Adds write actions. | both | 3 |
| `iris wait`, `iris ask` | Ends the session and waits, or asks the person. | session | 5 |

Without `--changeset`, a command that adds an action makes a new draft
changeset, and prints its ID. Two callers in host mode look the same to the
engine, so each caller names its own draft changeset. In session mode, the
session has one draft changeset, and `--changeset` is not necessary.

Each command that adds an action prints the changeset ID and the action
number, and changes nothing outside the draft changeset. Nothing leaves the
engine before `submit`.

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
  list.
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

- One MCP tool for each verb of the command, with the same name and the same
  arguments.
- The engine makes a session socket for each MCP job. The MCP interface never
  uses `iris.sock`, so the session limits apply. MCP gives no verb that the
  command does not have.
- The results are the same JSON documents as `--json`.

### 8.6 The web client and the trust boundary

The web client is the only place to approve, deny, undo and change the
policy. The one exception is the model budget of release 1 (section 8.1). The engine protects its web interface:

- At each start, the engine makes a random web secret of 32 bytes and a
  one-time code. `iris serve` prints a one-time sign-in address:
  `http://<address>/#code=<code>`. Release 1 does not open the browser,
  because the engine often runs as a background service with no screen
  (`iris service install`). The person opens the address. A later release
  can open the browser when the engine runs in a terminal. The browser does
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
- A test that an approval of revision 1 is refused after revision 2 is
  submitted, and that the `proposed` and `in_review` actions of revision 1
  are `replaced`.
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
3. The format of the hold delay per recipient. Version 0 has one delay for all
   sends.

## Assumptions

- A person can change a message in Gmail between the read of step 2 and the
  provider call of step 4 in section 7.2. The window is the time of one
  provider call. If the person changes a label of the action in that window,
  the undo can reverse that change. Version 0 accepts this risk.
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
