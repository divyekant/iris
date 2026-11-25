# Spec: change a draft and undo the change

- Date: 2026-11-25
- Card: P4 of the product decisions of 2026-11-24 (D6; issue #238)
- Status: Draft

## Problem

Each press of Send in the write view makes a new Gmail draft
(`docs/specs/2026-11-17-write.md`, "Not in scope"). If the person stops
a mail in the outbox, changes the text and presses Send again, Gmail has
two drafts for one reply. The first one stays, because Iris never
deletes a draft (send spec `docs/specs/2026-11-16-send.md`, R3-01). After
three tries, Gmail has three drafts. Iris made this clutter.

Spec 2 section 7.5 says that no `draft` action can change a draft until
the undo of the change exists. The undo restores the old content and
never deletes a draft of the person.

The owner decided (`docs/decisions.md`, entry D12, item D6, option A):
"Yes, in the write view. Build the draft change and its undo together,
as spec 2 section 7.5 says."

## Goal

1. A `draft` action can name a draft of Iris (`d-N`). Its apply changes
   that draft at Gmail in place (`drafts.update`). It makes no new
   draft.
2. The ledger entry of a change has an undo. The undo restores the
   content of the draft before the change. No step deletes a draft.
3. In the write view, after the person stops a mail, "Change and send"
   opens the form with the content of the draft. The next press changes
   the same draft and sends it.
4. A change does not overwrite text that the person wrote in Gmail
   before the read of the apply. If the draft at Gmail is not the draft
   that Iris wrote, the change and the undo are refused. Gmail has no
   conditional update, so an edit in Gmail in the time of one provider
   call (between the read and `drafts.update`) can be lost. Version 1
   accepts this risk (section 7).
5. Each rule of the send spec stays true: the approval binds to the hash
   of the content, the read back before the release, the hold, the send
   switch and the checks before the send.

## Not in scope

- Outlook and IMAP accounts. Microsoft Graph changes a draft only through
  its JSON fields (`PATCH /me/messages/{id}`), not through the MIME
  bytes that Iris writes, so the read back and the hash check do not
  hold. IMAP has no draft path in Iris. On these accounts, "Change and
  send" makes a new draft, as today. A limit issue records this.
- A change by an agent or by the `iris` command. Only the author
  `person`, through `POST /api/write`, can change a draft in this
  version. Submit denies a change from another author. A limit issue
  records this.
- A change of the thread or the parent of a reply. A change keeps the
  thread and the reply headers of the draft.
- A change of a draft that the person made in Gmail. Iris changes only a
  draft that it made (a `draft` row).
- A list of the old versions of a draft. The ledger keeps one version
  before each change.

## Design

### 1. The target of a change

The verb `draft` gets one more target: `{"drafts": ["d-N"]}`, one draft.
The args are the content of send spec section 1 (with `bcc` and
`attachments` of the send content spec). For a reply draft, the args
have no `subject`: the change keeps the subject of the draft, as a reply
create takes its subject from the parent. For a new mail, the args have
`subject`.

The submit denies a change, with the reason, when one of these is true:

| Check | Reason |
|---|---|
| The author is not `person` | "Only you can change a draft." |
| No `draft` row has the ID | "The draft does not exist." |
| The account of the draft has another provider than `gmail` | "Iris cannot change a draft of this account." |
| The draft is sent (`sent_at` is set) | "The draft is sent." |
| The draft has `foreign_at` | "The draft has a form that Iris does not write. Change it in Gmail." |
| An attachment of the content of the draft row has no file in the attachment folder with its SHA-256 and size (for example, a file that the person added in Gmail, send content spec) | "A file of the draft is not in Iris. Change it in Gmail." |
| A `send` action of the draft is `proposed`, `in_review`, `approved` or `held`, or a `ledger_send` row claims it | "The draft waits to send. Cancel it first." |
| A send mark of another write (write spec, section 3) whose `draft` action made or names this draft has no send action yet, and that write is not `stopped`. The mark of the action that runs the check (its own write) does not count | "The draft waits to send. Cancel it first." |
| Another `draft` action that names the same draft is `approved` or `held`, or has a ledger row in `writing` | "Iris changes this draft now. Try again in a minute." |

The parent of a change of a reply: the submit resolves the target `d-N`
to the messages of the thread of the draft, for the hostile check and
the flags, as a thread target does. The parent is the one message that
the rule of a thread target gives on that thread
(`iris_changeset::parent`: the newest eligible message). The submit
stores it as the parent row of the action, so the approval hash binds
it, as for a create. The write check (section 5) compares this parent
with `parent` of the request. The apply denies the change, with "A new
mail came into this thread. Write a new reply.", when the parent is not
the message whose `Message-ID` is the `In-Reply-To` of the draft row: a
change keeps the reply headers, so it cannot answer another message.

The submit runs each check of the table but the attachment check: the
submit reads only the database, and the files are in the attachment
folder. The apply runs the attachment check (section 2, step 2).

The same checks run again in the apply (section 2, step 1), and a third
time in the claim transaction (section 2, step 4), because the state can
change between the submit and the apply. The flags of the change (R3-03)
come from the new content, as for a create.

The submit of a `send` gets the mirror check: it denies a send of a
draft that has a `draft` change action that is `approved` or `held`, or
that has a ledger row in `writing`, with "Iris changes this draft now.
Try again in a minute.". The one write connection of the store runs
each submit and each claim, so of a change and a send of one draft, the
first transaction that commits wins, and the other is denied. In the
write view, one press makes the change and then its send (section 5),
so the two never race there.

### 2. The apply of a change

The apply module (`crates/iris-apply/src/send.rs`) gets a branch for a
`draft` action with a `drafts` target. The steps:

1. Read the draft row and run the checks of section 1 again. A check that
   fails denies the action with its reason. No provider call.
2. Compose the new message (`iris_compose::compose_with`): the new
   content, the reply headers of the draft row, the owner name, a new
   `Date`, and the `Message-ID` of the draft row. Check the size limit,
   the attachment files and the flags (`grown`) as a create does.
3. Read the draft at Gmail (`draft_raw`) and read it back
   (`iris_compose::read`, the same read back as the release). Compare its
   content hash with the `hash` of the draft row.
   - Not found: deny with "The draft is gone at Gmail.".
   - Another hash, or a form that Iris does not write: do the steps of
     `changed_at_gmail` for the row (refresh, or `foreign_at`), and deny
     with "The draft changed at Gmail. Iris did not change it.".
   - The new content hash is the `hash` of the draft row (the person
     sent the same content again): no provider call. In one transaction,
     the action becomes `applied` with a ledger row `applied`, `undo`
     `none`, no `pre`, and `post` set to the content of the press (so its
     hash is the hash of the press). The send of the write then goes as in the write
     spec. So a claim with a call always has a new hash that is not the
     old hash, and recovery can tell the two apart (section 4).
4. Claim the ledger row in `writing`, in one transaction that runs the
   checks of section 1 again (a check that fails denies the action and
   writes no ledger row). `ops` is
   `{"drafts.update": {"draft", "provider_id", "message_id", "date",
   "pre_hash", "hash", "thread"}}`. `pre` is the stored content of the
   draft row before the change (its `content` JSON and its `Date` in
   seconds), and `post` is the new stored content. `undo` is `inverse`.
5. Call `drafts.update` with the new bytes and the thread ID of a reply.
6. On success, read the draft back (section 7). In one transaction: set
   the row `applied`, and set `content`, `hash`, `raw` and `updated_at`
   of the draft row. The `Message-ID` of the row follows the read back,
   with the rule of `changed_at_gmail` (`sets_draft_message_id`). If the
   read back has another `Message-ID` than the bytes of step 2, `raw` is
   composed again with the `Message-ID` of the read back, as the create
   path does after Gmail sets its own ID, so the release sends and the
   send recovery searches the same ID. The action becomes `applied`. If
   the read back fails, the row stays in `writing`, and recovery
   (section 4) completes it.
7. On a definite rejection: the row is `failed` with the error. The
   draft row does not change.
8. On an unknown result: recovery (section 4).

A change keeps the `id` of the draft row. The `provider_id` of the row
is the draft ID of the answer of `drafts.update`, in step 6 and in
recovery, whether or not it is the old ID, so each later read and send
uses the ID that Gmail gave last. The `action_id` column keeps the
action that made the draft. A send after
the change binds to the new `hash` (write spec, section 3).

The draft of an action is one rule, used by the send step of a write
(`send_written`, write spec section 3, step 2a) and by the state of a
write (`GET /api/write/{cs}`): the `draft` row whose `action_id` is the
action (a create), or the `draft` row that the `drafts` target of the
action names, once the action is `applied` (a change). Each query that
joins `draft.action_id` to a send mark uses this rule. A test runs the
write of a change to its `waiting` state.

A send mark binds to the content of its own press: `send_written` makes
the send of a mark only while the `hash` of the draft row is the hash of
the `post` of the ledger entry of the action of the mark (for a create,
the hash of the create). Another hash means that the draft changed
after the press, and the mark stops with "The draft changed after you
pressed Send. Look at it, then send again.". With the check of section
1 on open marks, a second write cannot change the draft while the mark
of the first one waits, and a stale mark cannot send the content of
another press.

### 3. The undo of a change

`POST /api/ledger/{id}/undo` of an `applied` change makes an approved
changeset of the person with one `draft` action (`iris_apply::undo`).
As each undo of spec 2 section 7.4, its target is the entry
(`{"ledger": ["l-N"]}`), the link of migration 23 (`ledger_undo`) ties
it to the entry, and its args are the `pre` content of the entry. Spec 2
section 7.4 gets this case in its list of inverses. These parts accept a
`draft` action with a `ledger` target only with that link, and only for
an entry whose `ops` is `drafts.update`:

- the submit resolver (`iris_changeset`), which resolves the entry to
  the draft of its `ops` and runs the checks of section 1 on that draft;
- the undo link checks of the store (the trigger `ledger_undo_insert`);
- the undo creation path (`iris_apply::undo`).

A `draft` action with a `ledger` target and no link is denied, as an
`untrash` with no link is. Its apply is section 2, with these
differences:

- Step 2 composes the message from `pre`, with the `Date` of `pre`. The
  attachment files must still be in the attachment folder. If one is
  missing, the undo is denied with "A file of the old draft is gone.".
- Step 3 compares the hash at Gmail with the `hash` of the `post` of the
  entry. If the person changed the draft after the change, the hash is
  different, and the undo is denied. The text of the person stays.
- The entry of the undo has `undo` `none`. The entry of the change gets
  `undone_by` and the state `undone`.
- Only the newest ledger entry of the draft (a change, a send or an
  undo, by row ID) can be undone. An entry with a later entry of the
  same draft that counts is final. A later entry counts when it is in
  `writing`, when it is `applied` or `undone` and made a provider call,
  or when it is `failed` after its provider call started with a result
  that is not known: a change or an undo that recovery failed (section
  4), or a send whose `ledger_send` claim stays. A no-op entry (section
  2, step 3) and an entry that the provider definitely rejected do not
  count. So after A to B, then B to C with a lost answer and an edit of
  the person back to B, the undo of A to B is a 409. So the change A to B stays
  the newest entry after a press of Send with B again, and after a send
  that Gmail refused. So for the changes A to B, B to C and C to B, the
  first entry cannot undo the last B. The hash check alone cannot show
  this, so the rule reads the ledger, not the hash.

The undo is not possible (409 with the reason, nothing written) while a
check of section 1 fails for the draft: for example while a send of the
draft waits in the outbox, or after the send. A sent mail cannot change.

### 4. Recovery

A ledger row of a change in `writing` after a crash or a lost answer:

1. Read the draft at Gmail by the `provider_id` of `ops`, and read it
   back. If Gmail answers "not found", look for the draft by the
   `Message-ID` of `ops` (`drafts_with_id`): one draft with the new hash
   is the changed draft, and recovery stores its ID. No draft, or more
   than one, is the "other result" of step 3.
2. Its hash is the new `hash` of `ops`: the call applied. Do step 6 of
   section 2. The new hash is never the `pre_hash` (section 2, step 3),
   so this result cannot come from a call that did not apply.
3. Any other result (the `pre_hash`, another hash, not found, or a read
   that fails for 24 hours): the row is `failed` with "Iris does not know
   if the draft changed. Look at it in Gmail.", and the draft row
   refreshes from Gmail (`changed_at_gmail`) when the read succeeded.
   Recovery never calls `drafts.update` again. The `pre_hash` at Gmail
   can be a call that did not apply, or a call that applied and then an
   edit of the person back to the old text. Iris cannot tell the two
   apart, so it does not write. A later send reads the draft
   at Gmail again before the release, so a send cannot go with content
   that the person did not approve.

### 5. `POST /api/write` and the write view

`POST /api/write` gets an optional field `draft` (`d-N`). With `draft`:

- The target of the `draft` action is `{"drafts": ["d-N"]}`, not the
  account or the thread. `account`, `thread` and `parent` must still be
  the values of the draft, so the parent check of the write spec (step 4)
  still runs for a reply. A value that is not the value of the draft is a
  400.
- If the account of the draft is not a Gmail account, the engine ignores
  `draft` and makes a new draft, as today (section "Not in scope").
- The send mark, the flags check and the key work as in the write spec.
  The request hash includes `draft`.

`GET /api/write/{cs}` in the state `stopped` with a draft gets the field
`change`:

| Value | When | Next press of Send |
|---|---|---|
| `"update"` | A Gmail account, each check of section 1 passes for the draft now, and for a reply, the newest eligible message of the thread is still the parent of the draft (its `Message-ID` is the `In-Reply-To` of the draft row) | Sends `draft`: the same draft changes |
| `"new"` | An Outlook or IMAP account, each check of section 1 passes for the draft now but the provider check and the attachment check, and for a reply, the newest eligible message of the thread is still the parent of the draft. The create of a new draft does not read the files of the old draft, so a missing file does not stop it: the form shows the attachment, and the person can remove it | No `draft`: a new draft, as today |
| `null` | All other cases (sent, `foreign_at`, a send waits, a missing file of a Gmail draft, or a reply whose thread has a newer eligible message) | No "Change and send". For a reply with a newer message, the state also has `newer_mail: true` (issue #1077), and the view says "A new mail came into this thread. Write a new reply." A newer message sorts after the parent by internal date and row ID. |

The client reads the content of the draft from `GET /api/drafts/{d-N}`
(a new read route: `to`, `cc`, `bcc`, `subject`, `body`, `attachments`,
and the account and the thread). The route also gives `change`, with the
values of the table above for the draft now, and `newer_mail` as in the
stopped state (only when it is true). The engine computes these values
with one function, so the draft answer and the
write state never disagree (issue 941).

A cancel in the Outbox makes a stopped mail too. After a cancel, the
Outbox reads `GET /api/drafts/{d-N}`. If `change` is not `null`, the
cancelled card shows "Change and send". The link opens the write view of
the draft (`#/write/d-N`), with the content of the draft in the form, as
after "Change and send" in the write view. For a reply, the form is under
the conversation of its thread. The form of a draft link has its own
place in the session storage (`iris.write.d-N`), so it never replaces
the unfinished form of a new mail or of a reply, and a reload keeps its
changes. The form never takes an automatic draft of the thread. The next
press sends `draft` only when `change` is `"update"` now. A write that
the engine took from a draft link stays in that place until it ends: a
reload or a new link of the same draft shows the state of that write,
not the form, while it can still leave (for `"new"`, the old draft can
still change). This is also true when the send of that write stops the
change of the draft (`change` is `null`, issue 1098), and after "Write
another" (issue 1101): only the end of the write (sent or stopped) ends
the record. The write shows also when the thread of a reply has no mail
to reply to now (its parent is in Trash): then it shows with no
conversation and no form. When the write ends, the form shows if the draft could change
when the link opened. Else the view shows the end of the write.

"Change and send" in a draft link view opens the form in place only for
the draft of the link. For another draft (the new draft of a `"new"`
change), it opens the link of that draft, so each form stays in the place
of its own draft (issue 1102).

If `change` is `null` when the link opens and no write of the link can
still leave, the view shows no form. It
says why and links to a new write: "A new mail came into this thread.
Write a new reply." with a link to the reply of the thread when
`newer_mail` is true, else "This draft cannot change now." with a link
to a new mail. A saved form of the draft link does not show.

If the draft read of the Outbox fails after a cancel, each later poll of
the Outbox tries it again, until the engine answers.

A send of a request in the review (spec 2 section 5.5, issue 208) whose
draft Iris can read has `change` and `newer_mail` in
`GET /api/changesets/{id}/drafts` (issue 1104). The values are the values
of `GET /api/drafts/{d-N}` after the person denies this send only: the
checks of this section count this one send as denied. The send shows
"Deny this send and change it yourself" only when `change` is not `null`.
When `newer_mail` is true, the send shows "A new mail came into this
thread after this reply was written. To answer it, deny this send and
write a new reply." and no button. The press denies that send only, with
the hash that the review showed, then opens the write view of its draft
(`#/write/d-N`). If the draft cannot change when the write view opens
(the state changed after the review loaded), the view shows no form and
says why, as for each draft link. The next press of Send there is a
new write of the person, with the hold of the Outbox. If the decision is
refused (409) or has no answer, the review loads the request again and the
view does not move. A `draft` action that waits has no Gmail draft yet,
so it has no change link in this version. A limit issue records this.

The write view, in the state `stopped` with `change` not `null`, shows
"Change and send". The press opens the form with the content of the
draft. For `"update"`, it keeps `d-N`, and the next press of Send sends
`draft`. The layout of the button needs a before/after page and the pick
of the owner.

### 6. The ledger view

A change entry shows "Changed the draft of" with the subject and the
first recipient of the draft, and Undo while `can_undo` is true (the
checks of section 3). The undo entry shows "Restored the old text of"
with the same words. The API marks both entries with `change` (an entry
of `drafts.update`); the undo also has `undo_of`. `ledger_can_undo` lists a change entry
only while its draft passes the checks of section 1 and the entry is
the newest entry of its draft.

### 7. The window between the read and the update

Gmail `drafts.update` has no precondition (no ETag and no `If-Match`),
so no call can say "change only if the draft is still X". The design
makes the window small and keeps the evidence:

- The read (step 3) and the update (step 5) are the two calls next to
  each other, with no other provider call between them.
- After the update, the apply reads the draft back once more. If its
  hash is not the new hash, the row is `failed` with "The draft changed
  at Gmail during the change. Look at it in Gmail.", and the draft row
  refreshes from Gmail (`changed_at_gmail`).
- The ledger keeps the content of Iris before the change (`pre`). The
  text of the person in the window is not in the ledger.

This is the same class of risk that spec 2 accepts for a label change
(spec 2, Assumptions, first entry). A limit issue records it.

## Options not chosen

- Delete the old draft and make a new one: spec 2 section 7.5 and the send
  spec forbid each delete of a draft.
- Keep each old version as its own Gmail draft: this is the clutter that
  D6 removes.
- Change an Outlook draft through `PATCH` of its JSON fields: the read
  back of the MIME bytes and the content hash would not cover the change.
- Let an agent change a draft: an agent change would need its own review
  rules for the old content. It waits for a request.
- A new library: the change uses the Gmail client and the compose code
  of Iris (`iris-gmail`, `iris-compose`). It adds no parser and no
  protocol client. `drafts.update` is one more call of the thin client.

## Failure list

| Input or event | Result | Test |
|---|---|---|
| Supported input: a `draft` action of the person on a Gmail draft of Iris that passes the checks of section 1 | The draft changes in place, with an undo | Apply test at the fake Gmail server |
| The person edits the draft in Gmail between the read and the update | The edit can be lost (section 7, an accepted risk). The read back after the update fails the row if the hash is not the new hash | Fake server test of the read back |
| The person presses Send with the same content again | No provider call, an entry with no undo, the send goes | Apply test |
| A send submit and a change claim of one draft at the same time | The first transaction that commits wins. The other is denied | Changeset test |
| The undo of a change that has a later entry of its draft | 409, nothing written | API test |
| The write of a change | Its send step finds the draft by the target and reaches `waiting` | Apply and API test |
| Two writes of one draft: the first change applies, and a second change comes before the send step of the first | The second submit is denied (an open mark). If the draft row changes anyway, the mark of the first stops on the hash | Changeset test, apply test |
| The person changed the draft in Gmail before the apply | Denied, the row refreshes or gets `foreign_at`, Gmail keeps the text of the person | Fake server test |
| The draft is gone at Gmail (deleted or sent outside Iris) | Denied, nothing written at Gmail | Fake server test |
| A send of the draft waits in the outbox, or a send is claimed | Submit and apply deny the change. The undo is a 409 | Changeset test, apply test |
| Two changes of one draft at the same time | The second submit is denied (section 1). The one write connection orders the two submits | Changeset test |
| A change and a send release at the same time | The release needs an approved send, which section 1 refuses while a change is open. The apply and the release run on the one apply lock | Apply test |
| A crash after the claim and before the call | Recovery finds `pre_hash` at Gmail: `failed`, no second call. The send of the write is denied at the release (hash) | Crash test |
| A lost answer after the call applied, then the person changes the text back to the old text in Gmail | Recovery finds `pre_hash`: `failed`, no call, the text of the person stays | Crash test |
| A lost answer after the call applied | Recovery finds the new hash and completes | Crash test |
| A lost answer, then the person changes the draft in Gmail | Recovery finds another hash: `failed`, no call | Crash test |
| An undo after the person changed the draft in Gmail | Denied, the text of the person stays | Fake server test |
| An undo while an attachment file of the old content is gone | Denied, no call | Apply test |
| A `drafts` target on an Outlook or IMAP account | Submit denies it. `POST /api/write` makes a new draft in place of a change | Changeset test, API test |
| A `draft` field of `POST /api/write` with another account, thread or parent | 400, nothing written | API test |
| All other input | Denied at submit with a reason, no provider call | Changeset test |

## Impact

| Part | Change |
|---|---|
| `crates/iris-provider`, `crates/iris-gmail`, `crates/iris-fakegmail` | `update_draft(token, id, raw, thread)` (Gmail `PUT drafts/{id}`). Default for other providers: refused. The fake server keeps the draft ID and changes its message. |
| `crates/iris-changeset` | The `drafts` target of `draft`, the checks of section 1 |
| `crates/iris-apply` | The change branch of the draft apply, its recovery and its undo |
| `crates/iris-api` | `draft` in `POST /api/write`, `change` in `GET /api/write/{cs}`, `GET /api/drafts/{d-N}`, the ledger texts |
| `client/` | "Change and send" in the write view, the ledger texts. A before/after page |
| `docs/spec/02-changeset-policy-ledger.md` | The `draft` row of section 4, and section 7.5 |
| Store | One migration. It changes the view `ledger_can_undo` so that it lists an `applied` draft change entry that is the newest entry of its draft and whose draft passes the checks of section 1 (the SQL of `change_denied` for no action, so the view and the undo agree). It changes the trigger `ledger_undo_insert` so that the undo of a draft change can be a `draft` action on the same draft. The view and the trigger keep each rule for the entries of the other verbs, so each existing entry keeps its state and its undo. `ops`, `pre` and `post` are JSON, and the `draft` row has each column, so no table changes. |
| `crates/iris-apply`, `crates/iris-api` | The rule "the draft of an action" (section 2) in `send_written` and in the state of a write |

Memory: the apply holds two copies of one message (the old read back and
the new bytes), at most 2 times `iris_compose::max_message` (4 MiB, so
about 8 MiB) during one change. A create holds one copy.

## Assumptions

- Gmail `drafts.update` replaces the message of a draft and keeps the
  draft ID (Gmail API reference, `users.drafts.update`). Pending: check
  on the owner's Mac before the change is on for a real mailbox. Step 6
  always stores the ID of the answer (section 2). If the ID changes and
  the answer is lost, recovery looks for the draft by its `Message-ID`
  (section 4). If Gmail also set its own `Message-ID`, recovery finds
  nothing and the row is `failed` with "Look at it in Gmail.": no write
  and no loss, but the person must check the draft.
- Gmail can set its own `Message-ID` on a changed draft, as on a created
  draft (send spec, assumption 3). The rule of `changed_at_gmail`
  already covers this. Evidence: `sets_draft_message_id` in
  `crates/iris-gmail/src/lib.rs`.
- The `gmail.modify` scope allows `drafts.update` (Gmail scopes page).
  Pending: the Mac check of #302 uses the same scope.
- A second `drafts.update` with the same bytes has the same result as one
  call. Pending: the fake server models this. If Gmail makes a second
  message, the read back still finds one draft with the new hash.

## External writes

| Call | Success | Definite rejection | Unknown result | Retry |
|---|---|---|---|---|
| `drafts.update` (change) | The read back has the new hash: the row is `applied`, the draft row has the new content. Another hash: `failed` (section 7) | The row is `failed`, the draft row does not change | Recovery (section 4) reads the draft: the new hash completes, each other result is `failed` | Never. A new press of the person makes a new action |
| `drafts.update` (undo) | The change entry is `undone`, the draft row has the old content | The undo row is `failed`, the change entry stays `applied` | Recovery as section 4, with the hashes swapped | Never. The person can press Undo again |

## Open questions

1. Must the first version of the write view also show "Change and send"
   for a mail that the review denied? The checks of section 1 allow it.
   The before/after page asks the owner.

## Acceptance criteria

1. Stop a reply in the outbox, change the text, and press Send: the fake
   Gmail server has one draft for the reply, with the new text. Red if the
   apply makes a second draft.
2. Undo the change in the ledger view: the fake server has the old text
   in the same draft. Red if the undo makes a draft or deletes one.
3. Change the draft at the fake server, then undo: the undo is denied, and
   the text at the server stays. Red if the undo writes.
4. A crash test for each case of section 4. Red if a case writes twice or
   loses the change.
5. An Outlook account still makes a new draft for "Change and send"
   (`change` is `"new"`). Red if the engine calls a change on Outlook, or
   if the write view hides the control for Outlook.
6. The changes A to B, B to C, C to B of one draft: the undo of the
   first entry is a 409. Red if the undo writes A.
7. The change A to B, then a press of Send with B again, then a send
   that Gmail refuses: the undo of the change A to B is still possible.
   Red if the no-op entry or the refused send blocks it.
9. A crash test: the update applied, the answer was lost, and the text
   at Gmail is the old text again. Recovery makes no call. Red if it
   calls `drafts.update`.
10. One press of "Change and send" reaches `waiting`: the check of open
    marks skips the mark of its own action. Red if the apply denies the
    change because of its own mark.
8. A change of a reply in a thread with two messages binds the newest
   message as its parent. Red if the write gets a 409 with no new mail.
