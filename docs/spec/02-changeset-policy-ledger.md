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

The held outbox (release 3), Stop (release 4), sessions in workspaces
(release 4) and proposed rules (release 5) are here only where they change the
formats. Their full design comes in a later specification.

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
| C2 | No agent holds a mailbox credential, and no agent can approve. | The `iris` command has no approve verb. Session mode has no path to the token. |
| C3 | Each action that can be reversed has an undo in the ledger. | For each verb, a test applies the action and the undo at the fake Gmail server, and compares the label sets. |
| C4 | An action that cannot be reversed waits for review or a delay. | `send` always goes through the held outbox (release 3). |
| C5 | A crash cannot apply an action twice or lose the record of it. | A test stops the engine between the provider call and the ledger update, and checks the state after the start. |
| C6 | The policy file is the only source of trust. | Only the person changes the file. If the file is not valid, no action applies without review. |
| C7 | 100 applied actions are correct in Gmail, and each undo is correct. | The exit criterion of release 2. |

## 4. Verbs

Each verb has a target, a release, an undo and the provider operations. A
thread target means all messages of the thread at the time of submit.

| Verb | Target | Arguments | Gmail operation | Undo | Release |
|---|---|---|---|---|---|
| `archive` | threads or messages | none | `messages.batchModify`, remove `INBOX` | add `INBOX` | 2 |
| `inbox` | threads or messages | none | `messages.batchModify`, add `INBOX` | remove `INBOX` | 2 |
| `label` | threads or messages | label name | `labels.create` if the label is new, then `batchModify`, add the label | remove the label (the label stays) | 2 |
| `unlabel` | threads or messages | label name | `batchModify`, remove the label | add the label | 2 |
| `read` | threads or messages | none | `batchModify`, remove `UNREAD` | add `UNREAD` | 2 |
| `unread` | threads or messages | none | `batchModify`, add `UNREAD` | remove `UNREAD` | 2 |
| `trash` | threads or messages | none | `messages.trash` for each message | `messages.untrash` | 2 |
| `close` | commitment | reason | none (Iris state) | set the old status | 2 |
| `defer` | commitment | date | none (Iris state) | set the old status | 2 |
| `decline` | commitment | reason | none (Iris state) | set the old status | 2 |
| `correct` | any item | field, value | none (Iris state) | remove the correction | 2 |
| `draft` | thread, or none for new mail | file with the text | `drafts.create` or `drafts.update` | `drafts.delete` | 3 |
| `send` | a draft | none | `drafts.send`, after the hold | none: final after the hold | 3 |
| `nudge` | commitment `owed_to_you` | none | makes a `draft` action, then a `send` action | as `draft` and `send` | 3 |
| `wait` | thread, or a date | until | none | not applicable | 5 |
| `ask` | none | question, options | none | not applicable | 5 |

Iris has no verb for a permanent delete, and no verb to empty the trash.
Gmail deletes a message in the trash for good after 30 days. After that time,
the undo of `trash` is not possible, and the ledger shows the entry as final.

A verb that changes Iris state (`close`, `defer`, `decline`, `correct`) goes
through a changeset too. This gives each change one path, one review and one
ledger.

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
      "decision": { "state": "applied", "by": "rule", "rule": 4, "at": "2026-11-16T09:17:41Z" },
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
| `revision` | Starts at 1. A new revision after a review comment replaces the actions that are not decided. |
| `author.kind` | `person`, `agent`, `triage` or `local`. |
| `why` | The reason of the author, in one or two sentences. Required. |
| `state` | `draft` (before submit), `open` (some action is not final), `closed` (each action is final) or `withdrawn`. |
| `actions[].n` | The number of the action in the changeset, from 1. |
| `actions[].targets` | What the author named: `threads`, `messages` or `items`. |
| `actions[].resolved` | The message IDs at submit. Review shows these, and apply uses only these. |
| `actions[].flags` | Why a person must look: see section 5.3. |
| `actions[].decision.state` | See section 5.2. |
| `actions[].ledger` | The ledger entry, after apply. |

### 5.2 States of an action

| State | Meaning | Next states |
|---|---|---|
| `proposed` | In a draft changeset, or in observe mode. Nothing is decided. | `applied`, `in_review`, `denied` |
| `in_review` | Waits for a person. | `approved`, `denied` |
| `approved` | A person approved. The engine applies it next. | `applied`, `failed`, `held` |
| `held` | A send in the held outbox (release 3). | `applied`, `cancelled` |
| `applied` | The provider accepted the change. The ledger has the entry. | `undone` |
| `denied` | The policy or a person refused it. `reason` says why. | none |
| `failed` | The provider refused it, or the target changed. `reason` says why. | none |
| `cancelled` | A person cancelled a held send. | none |
| `undone` | The undo is applied. The undo has its own ledger entry. | none |

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
| `bulk` | The action changes more than 500 messages. |

### 5.4 Submit

1. The author adds actions to a draft changeset. Nothing leaves the engine.
2. The author submits the changeset with a reason.
3. The engine resolves each target to message IDs, from the mirror.
4. The engine adds the flags.
5. The engine evaluates the policy for each action (section 6.4). Each action
   gets `applied` (after apply), `in_review` or `denied`.
6. The engine returns one result for each action.

If the policy mode is `observe`, step 5 records what the policy would do, and
each action stays `proposed`. The triage changeset of release 1 works in this
way.

### 5.5 Review

- The person reviews in the web client. The review shows each action fully
  resolved: the real messages, the real recipients and the final text.
- The person can approve all, approve one, edit the text of a draft, comment
  or deny.
- A comment goes back to the author. In release 4, the comment arrives as a
  file in the workspace. The author submits a new revision.
- A changeset with actions in review for 14 days goes to the state `closed`.
  Each action in review becomes `denied` with the reason "no review in 14
  days".

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
added = 2026-11-20

[[rule]]
id = 2
name = "Hide Acme Deals"
effect = "hide"
when = { sender = "deals@acme.example" }
by = "proposal"
added = 2026-11-22

[[rule]]
id = 3
name = "Bundle the feed"
effect = "bundle"
when = { form = "feed" }
by = "person"
added = 2026-11-20

[[rule]]
id = 4
name = "Never touch mail from the bank without review"
effect = "review"
when = { sender = "@bank.example" }
by = "person"
added = 2026-11-20
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
| `sender_kind` | `person`, `service`, `stranger` or `hostile`. |
| `label` | A label on the message. |
| `record_type` | The type of a record. |
| `commitment_kind` | The kind of a commitment. |
| `author` | `person`, `agent`, `triage` or `local`. |
| `older_than_days` | The age of the message in days. |

Version 0 has no other condition, and no expression language. A new condition
needs a change to this specification.

### 6.3 Who changes the file

- Only the person changes the policy. The person edits the file, or uses the
  web client, which writes the file.
- The engine writes the file only for an action of the person in the web
  client: a change of `mode`, an accepted proposal, or the removal of a rule.
  The engine changes only the lines of that rule and keeps all other text and
  comments.
- No verb of the `iris` command changes the policy.
- The engine reads the file again when it changes on disk.
- If the file is not valid, the engine uses an empty policy in mode `observe`.
  Each action goes to review, and nothing is hidden. The brief shows the error
  and the line number. This is fail closed: a defect in the file can never
  allow more.

### 6.4 Evaluation

For each action:

1. Check the hard limits (section 6.5). A hard limit can send an action to
   review or deny it. No rule can change that result.
2. Find each rule with an action effect (`allow`, `review`, `deny`) whose
   `verbs` include the verb and whose `when` matches.
3. If a `deny` rule matches, the action is `denied`.
4. Else, if a `review` rule matches, the action is `in_review`.
5. Else, if an `allow` rule matches, the engine applies the action.
6. Else, the action is `in_review`. The default is review.

An `allow` rule must match each message of the action. If one message does not
match, the action goes to review.

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
| An action from a session on a message from a `hostile` sender. | Only `archive`, `trash` and `label`. Other verbs are denied. |
| Stop is on (release 4). | No provider write. Actions in the outbox wait. |
| Mode is `observe`. | Nothing applies. |
| A permanent delete. | No verb exists. |

### 6.6 Proposed rules (release 5)

Iris proposes a rule after a pattern: for example, the person approved 14 of
14 `archive` actions for the same sender kind. The proposal is a row in the
store, not a line in the file. When the person accepts it, the engine adds the
rule to the file with `by = "proposal"`. A proposal never changes the file
without the person.

## 7. The ledger

### 7.1 What the ledger holds

The ledger is the table `ledger` in `iris.db`. The engine adds rows. It
changes a row only to move its state forward.

| Column | Meaning |
|---|---|
| `id` | `l-` and a number. |
| `changeset`, `revision`, `action` | The action that the entry applies. |
| `verb` | The verb. |
| `author` | The author of the changeset. |
| `decided_by` | `rule` with its ID, or `person`. |
| `ops` | The provider operations, as JSON (for example the message IDs and the labels to add and remove). |
| `pre` | For each message, the labels that the action changes, before the action. |
| `post` | The same labels after the action. |
| `state` | `writing`, `applied`, `failed` or `undone`. |
| `history_id` | The Gmail history ID after the apply. |
| `undo` | `inverse` (an undo exists) or `none` (final, for example a sent message). |
| `undone_by` | The ledger entry of the undo. |
| `at` | The time. |

### 7.2 Apply

1. In one SQLite transaction, add the ledger row with the state `writing`, the
   `ops` and the `pre` state from the mirror.
2. Call the provider.
3. If the call succeeds, set the state `applied` and save `post` and the
   history ID. Update the mirror with the new labels.
4. If the call fails, set the state `failed` and save the error. Do not try a
   send again.

`batchModify` adds and removes labels, so a second call with the same `ops`
has the same result. For these verbs, a repeat is safe.

### 7.3 Start after a crash

At start, the engine looks for each ledger row in the state `writing`:

- For a label verb, the engine reads the labels of each message at Gmail
  (`format=minimal`). If each message has the `post` state, the row becomes
  `applied`. Else, the row becomes `failed`, and the action is shown to the
  person.
- For a `send`, the engine looks for the message in `SENT` by the
  `Message-ID` header that Iris set in the draft. If it is there, the row
  becomes `applied`. Else, the row becomes `failed`. The engine never sends
  again without a new approval. Two sends of one message are worse than a
  failed send.

### 7.4 Undo

1. The person selects undo on a ledger entry, in the web client.
2. The engine makes a changeset with author `person` and one action: the
   inverse of the entry.
3. The inverse changes only the messages that still have the `post` state of
   the entry. A message that changed after the entry (for example, the person
   moved it in Gmail) stays as it is. The result lists these messages.
4. The undo gets its own ledger entry. The first entry gets `undone_by`.

An undo of an undo is a new action, not a delete of the record.

### 7.5 Held outbox (release 3)

- After approval, a `send` goes to the outbox with a release time: the
  approval time plus the hold delay. The default delay is 60 seconds. The
  person sets it in the web client.
- Before the release time, undo cancels the send. The action becomes
  `cancelled`.
- At the release time, the engine sends. The ledger entry gets `undo = none`,
  and the client shows the send as final.

## 8. The iris command

### 8.1 One program, two modes

`iris` is the engine and the command, in one program. The command does not
read the store. It sends each request to the engine through a Unix domain
socket, and the engine does the work. Unix domain sockets work on macOS and on
Linux.

| Mode | When | Socket | Who |
|---|---|---|---|
| Host mode | No `.iris/` folder in the current folder or above it. | `iris.sock` in the data folder, mode `0600` | The OS user that runs the engine. |
| Session mode (release 4) | The current folder or a folder above it has `.iris/`. | `.iris/socket`, one for each session | The agent session of one job. |

The socket tells the engine who calls. In session mode, the engine limits each
verb to the job of the session. The socket file is the only capability that a
session gets: it holds no token and no credential.

The command has no verb to approve, to deny or to change the policy. Only the
web client does these steps, for the person. The person has no terminal client
(decision of the maintainer). In host mode, a changeset has the author `local`
and goes through the policy as an agent changeset does.

### 8.2 Verbs of the command

| Command | Does | Mode | Release |
|---|---|---|---|
| `iris serve [--addr <addr>]` | Starts the engine. The default address is `127.0.0.1:4800`. | host | 0 |
| `iris status` | Shows the account, the sync, the mode and the counts. | both | 1 |
| `iris sync` | Runs an incremental sync now. | host | 1 |
| `iris mirror verify` | Compares the mirror with Gmail (specification 1, section 4.8). | host | 1 |
| `iris rebuild [--since-prompt <v>]` | Runs understanding again. | host | 1 |
| `iris backup <path>` | Copies the data folder while the engine runs. | host | 1 |
| `iris export <path>` | Writes each item as JSON Lines, and copies the raw messages. | host | 1 |
| `iris brief` | Shows the brief. | both | 1 |
| `iris search <query>` | Full-text search (specification 1, section 5.6). In session mode, it adds the threads that match to `threads/`. | both | 1 |
| `iris show <id>` | Shows one item, message, thread or sender. | both | 1 |
| `iris list <form> [filters]` | Lists items of one form. | both | 1 |
| `iris archive`, `inbox`, `label`, `unlabel`, `read`, `unread`, `trash` `<ids>` | Adds an action to the draft changeset. | both | 2 |
| `iris close`, `defer`, `decline`, `correct` `<id>` | Adds a state action to the draft changeset. | both | 2 |
| `iris changeset` | Shows the draft changeset (in a session: `iris status`). | both | 2 |
| `iris submit --why <text>` | Submits the draft changeset, and prints one result for each action. | both | 2 |
| `iris withdraw <cs-id>` | Withdraws a changeset. Applied actions stay. | both | 2 |
| `iris ledger [filters]` | Lists ledger entries. | host | 2 |
| `iris draft <thread> <file>`, `iris send <draft>`, `iris nudge <c-id>` | Adds write actions. | both | 3 |
| `iris wait`, `iris ask` | Ends the session and waits, or asks the person. | session | 5 |

Each command that adds an action prints the action number and changes nothing
outside the draft changeset. Nothing leaves the engine before `submit`.

### 8.3 Output

- The default output is short text for a person or an agent to read. Each
  line starts with an ID, so the next command can use it.
- `--json` prints the open model documents (specification 1, section 6, and
  section 5.1 here). One JSON document for one object, and JSON Lines for a
  list.
- The command writes errors to standard error. An error message says what
  failed and what to do.

### 8.4 Exit codes

| Code | Meaning |
|---|---|
| 0 | The command succeeded. For `submit`: each action is applied, held or in review. |
| 1 | An error in the engine or at the provider. |
| 2 | The command is not correct (usage). |
| 3 | The policy or a hard limit denied one or more actions. |
| 4 | The engine is not reachable. |

### 8.5 MCP, the second interface (release 5)

The engine offers the same verbs as Model Context Protocol (MCP) tools, for an
agent that does not work in a folder. The rules for the MCP interface:

- One MCP tool for each verb of the command, with the same name and the same
  arguments.
- The same socket and the same session limits. MCP gives no verb that the
  command does not have.
- The results are the same JSON documents as `--json`.

## 9. Memory

- The policy holds a few hundred rules at most. The engine keeps the parsed
  policy in memory, under 1 MB.
- The ledger and the changesets are rows in SQLite. The engine keeps no list
  of them in memory.
- Apply sends at most 1,000 message IDs in one `batchModify` call (the Gmail
  limit) and streams larger actions in pages.

## 10. Test plan

- For each verb of release 2: apply at the fake Gmail server, verify the label
  sets, undo, and verify again (requirement C3).
- A crash test for each state of section 7.3 (requirement C5).
- A policy test for each step of section 6.4, and for each hard limit.
- A test with a policy file that is not valid: each action goes to review.
- A test that the `iris` command has no verb that approves, denies or changes
  the policy.
- A test that an undo skips a message that changed after the entry.

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

- Gmail `messages.batchModify` accepts up to 1,000 message IDs, and a repeat
  with the same labels has the same result. Verify before the apply code is
  written.
- Gmail `drafts.send` keeps the `Message-ID` header that Iris sets in the
  draft, so the engine can find the sent message after a crash. If Gmail
  changes the header, the engine needs another key (for example a header of
  its own), and section 7.3 changes.
- Release 2 asks for the scope `gmail.modify`. It allows label changes, trash
  and drafts, but not a permanent delete. Release 3 adds `gmail.send` if
  `gmail.modify` does not include `drafts.send`. Verify the scopes before
  release 2.
- One person uses one install. The policy has no users and no roles.
- The person reviews in the web client. There is no review in the terminal
  and none by mail.
- A Unix domain socket is enough to tell host mode from session mode. The
  sandbox of release 4 must make sure that a session cannot reach
  `iris.sock` in the data folder.
- TOML is a format that the person can edit by hand. If it causes errors in
  practice, a later version can add a form in the web client, and keep the file
  as the source.
