-- Migration 5: the changeset (specification 2, section 5).
--
-- A changeset has one row. Each action of each revision has one row, and each
-- message that an action resolved to at submit has one `action_message` row
-- (section 9: the message IDs are rows, not one list in memory).

CREATE TABLE changeset (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  -- The newest revision. Its actions are a draft while submitted_at is NULL.
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  author_kind TEXT NOT NULL CHECK (author_kind IN ('person', 'agent', 'triage', 'local')),
  author_name TEXT,
  author_session TEXT,
  author_job TEXT,
  why TEXT,
  created_at INTEGER NOT NULL,
  submitted_at INTEGER,
  -- 'draft' until an action of the changeset has a state. Then 'open'
  -- while an action is open, else 'closed'. The trigger action_closes keeps
  -- it current, and changeset_state refuses any other value.
  state TEXT NOT NULL DEFAULT 'draft'
    CHECK (state IN ('draft', 'open', 'closed', 'withdrawn')),
  -- coalesce: a CHECK that gives NULL passes.
  CHECK (submitted_at IS NULL OR length(trim(coalesce(why, ''))) > 0)
);

CREATE TABLE changeset_action (
  id INTEGER PRIMARY KEY,
  changeset_id INTEGER NOT NULL REFERENCES changeset(id),
  revision INTEGER NOT NULL,
  n INTEGER NOT NULL CHECK (n >= 1),
  -- untrash: only the undo of a trash makes it (section 7.4).
  verb TEXT NOT NULL CHECK (verb IN ('archive', 'inbox', 'label', 'unlabel', 'read',
    'unread', 'trash', 'untrash', 'close', 'defer', 'decline', 'correct')),
  -- JSON, as the author named them: {"threads": ["t-1"]}.
  targets TEXT NOT NULL,
  -- JSON object. The approval hash covers this exact text.
  args TEXT NOT NULL,
  why TEXT,
  -- JSON array of flags (section 5.3).
  flags TEXT NOT NULL DEFAULT '[]',
  -- NULL while the revision is a draft. The trigger action_state allows only
  -- the moves of section 5.2.
  state TEXT CHECK (state IN ('proposed', 'in_review', 'approved', 'held', 'applied',
    'failed', 'denied', 'expired', 'cancelled', 'replaced', 'undone')),
  decided_by TEXT CHECK (decided_by IN ('rule', 'person', 'policy', 'limit', 'engine',
    'author', 'expiry')),
  rule INTEGER,
  -- The confirmed policy version of a rule decision (migration 4). The
  -- policy of the first start has no rule.
  policy INTEGER REFERENCES policy_version(version),
  reason TEXT,
  decided_at INTEGER,
  -- SHA-256 (hex) of resolved and args at submit (section 5.5).
  hash TEXT,
  -- The submit of the revision of this action. Expiry counts from it.
  submitted_at INTEGER,
  -- The document of section 5.1 copies these three as JSON text.
  CHECK (json_valid(targets) AND json_valid(args) AND json_valid(flags)),
  -- An allow rule or a person approves (section 5.2).
  CHECK (state IS NOT 'approved' OR coalesce(decided_by, '') IN ('rule', 'person')),
  -- A draft or proposed action has no decision. Each other state names who
  -- decided.
  CHECK (CASE WHEN coalesce(state, 'proposed') = 'proposed'
              THEN decided_by IS NULL AND rule IS NULL AND policy IS NULL
                   AND reason IS NULL AND decided_at IS NULL
              ELSE decided_by IS NOT NULL END),
  -- A person approves or denies, but does not send an action to review.
  CHECK (state IS NOT 'in_review' OR coalesce(decided_by, '') IN ('rule', 'policy', 'limit',
    'engine')),
  -- A rule decision names its rule; only a rule decision names a rule.
  CHECK ((coalesce(decided_by, '') = 'rule') = (rule IS NOT NULL)),
  CHECK ((rule IS NULL) = (policy IS NULL)),
  -- A denial says why (section 5.2).
  CHECK (state IS NOT 'denied' OR length(trim(coalesce(reason, ''))) > 0),
  UNIQUE (changeset_id, revision, n)
);

-- No foreign key to message: the cleanup of gone messages deletes message
-- rows, and the apply skips a message that is not there (section 10).
CREATE TABLE action_message (
  action_id INTEGER NOT NULL REFERENCES changeset_action(id),
  message_id INTEGER NOT NULL,
  PRIMARY KEY (action_id, message_id)
) WITHOUT ROWID;

-- Expiry (section 5.5) finds the open actions.
CREATE INDEX changeset_action_state ON changeset_action(state);

-- The moves of an action between states (section 5.2). NULL is a draft
-- action: submit gives 'proposed', or 'denied' for an action that is not
-- valid; a withdraw gives 'cancelled'.
CREATE TABLE action_transition (
  from_state TEXT,
  to_state TEXT NOT NULL,
  UNIQUE (from_state, to_state)
);
INSERT INTO action_transition VALUES
  (NULL, 'proposed'), (NULL, 'denied'), (NULL, 'cancelled'),
  ('proposed', 'approved'), ('proposed', 'in_review'), ('proposed', 'denied'),
  ('proposed', 'expired'), ('proposed', 'cancelled'), ('proposed', 'replaced'),
  ('in_review', 'approved'), ('in_review', 'denied'), ('in_review', 'expired'),
  ('in_review', 'cancelled'), ('in_review', 'replaced'),
  ('approved', 'applied'), ('approved', 'failed'),
  ('approved', 'in_review'), ('approved', 'denied'), ('approved', 'expired'),
  ('approved', 'cancelled'),
  ('applied', 'undone');
-- Release 3 adds approved to held and the moves of held, for a send.
-- Section 5.2 also allows failed to undone, but only when the ledger lists
-- a changed message. The ledger (R2-03) adds that move with its check.

-- A new changeset is a draft of revision 1. No insert replaces a changeset.
CREATE TRIGGER changeset_insert BEFORE INSERT ON changeset
WHEN new.revision <> 1 OR new.submitted_at IS NOT NULL OR new.state <> 'draft'
  OR EXISTS (SELECT 1 FROM changeset WHERE id = new.id)
BEGIN
  SELECT RAISE(ABORT, 'iris: a new changeset is a draft of revision 1');
END;

-- The author and the time of a changeset do not change. A revision moves
-- only in two ways: the submit of the draft revision, and a new draft
-- revision after the submit. A submitted revision does not become a draft
-- again, and a withdrawn changeset does not change.
CREATE TRIGGER changeset_revision BEFORE UPDATE OF revision, submitted_at, why,
  author_kind, author_name, author_session, author_job, created_at ON changeset
WHEN new.author_kind IS NOT old.author_kind OR new.author_name IS NOT old.author_name
  OR new.author_session IS NOT old.author_session OR new.author_job IS NOT old.author_job
  OR new.created_at IS NOT old.created_at OR old.state = 'withdrawn'
  OR NOT ((new.revision = old.revision AND old.submitted_at IS NULL)
          OR (new.revision = old.revision + 1 AND old.submitted_at IS NOT NULL
              AND new.submitted_at IS NULL AND new.why IS old.why))
BEGIN
  SELECT RAISE(ABORT, 'iris: this change of a changeset is not allowed');
END;

-- An action enters only the draft revision of its changeset, with no state.
-- A BEFORE trigger runs before INSERT OR REPLACE deletes a row, so no insert
-- replaces an action.
CREATE TRIGGER action_insert BEFORE INSERT ON changeset_action
WHEN new.state IS NOT NULL
  OR EXISTS (SELECT 1 FROM changeset_action WHERE id = new.id
             OR (changeset_id = new.changeset_id AND revision = new.revision
                 AND n = new.n))
  OR NOT EXISTS (SELECT 1 FROM changeset WHERE id = new.changeset_id
                 AND revision = new.revision AND submitted_at IS NULL
                 AND state <> 'withdrawn')
BEGIN
  SELECT RAISE(ABORT, 'iris: a new action goes into the draft revision, with no state');
END;

-- An action stays in its changeset, revision and place.
CREATE TRIGGER action_place BEFORE UPDATE OF changeset_id, revision, n ON changeset_action
BEGIN
  SELECT RAISE(ABORT, 'iris: an action stays in its changeset and revision');
END;

-- Only submit gives a draft action a state other than 'cancelled': it sets
-- the hash and the submit time in the same write.
CREATE TRIGGER action_state BEFORE UPDATE OF state ON changeset_action
WHEN NOT EXISTS (SELECT 1 FROM action_transition
                 WHERE from_state IS old.state AND to_state = new.state)
  -- A withdrawn changeset applies nothing more (section 5.5).
  OR (new.state IN ('in_review', 'approved', 'applied', 'failed')
      AND (SELECT state FROM changeset WHERE id = new.changeset_id) = 'withdrawn')
  OR (old.state IS NULL AND new.state <> 'cancelled'
      AND (new.hash IS NULL OR new.submitted_at IS NULL
           OR NOT EXISTS (SELECT 1 FROM changeset WHERE id = new.changeset_id
                          AND revision = new.revision
                          AND submitted_at IS new.submitted_at)))
  -- No approval while a new revision is a draft (section 5.5): its submit
  -- replaces the older actions with no decision.
  OR (new.state = 'approved'
      AND (SELECT submitted_at FROM changeset WHERE id = new.changeset_id) IS NULL)
  -- An action is cancelled only with a withdraw of its changeset.
  OR (new.state = 'cancelled'
      AND (SELECT state FROM changeset WHERE id = new.changeset_id) <> 'withdrawn')
  -- Replaced only by the submit of a newer revision.
  OR (new.state = 'replaced'
      AND NOT EXISTS (SELECT 1 FROM changeset WHERE id = new.changeset_id
                      AND revision > new.revision AND submitted_at IS NOT NULL))
  -- Expired only 14 days after its submit (section 5.5).
  OR (new.state = 'expired'
      AND (new.decided_at IS NULL OR new.decided_at - old.submitted_at < 1209600000))
  -- Applied and failed keep the decision that approved the action: the
  -- ledger names it (section 7.1).
  OR (new.state IN ('applied', 'failed')
      AND (new.decided_by IS NOT old.decided_by OR new.rule IS NOT old.rule
           OR new.policy IS NOT old.policy OR new.reason IS NOT old.reason
           OR new.decided_at IS NOT old.decided_at))
  -- A rule decision names a rule ID of the active policy (section 6.3).
  OR (new.decided_by IS 'rule' AND new.state IN ('in_review', 'approved', 'denied')
      AND (new.policy IS NOT (SELECT max(version) FROM policy_version)
           OR NOT EXISTS (SELECT 1 FROM policy_rule_id
                          WHERE id = new.rule AND removed_at IS NULL)))
BEGIN
  SELECT RAISE(ABORT, 'iris: this state change of an action is not allowed');
END;

-- A withdraw cancels each open action and each draft action, in the same
-- write (section 5.5). Release 3 adds held, with its moves. A withdrawn changeset stays withdrawn.
CREATE TRIGGER changeset_withdraw AFTER UPDATE OF state ON changeset
WHEN new.state = 'withdrawn' AND old.state <> 'withdrawn'
BEGIN
  UPDATE changeset_action SET state = 'cancelled', decided_by = 'author', rule = NULL,
    policy = NULL, reason = NULL, decided_at = CAST(unixepoch('subsec') * 1000 AS INTEGER)
  WHERE changeset_id = new.id
    AND (state IS NULL OR state IN ('proposed', 'in_review', 'approved'));
END;

CREATE TRIGGER changeset_withdrawn BEFORE UPDATE OF state ON changeset
WHEN old.state = 'withdrawn' AND new.state <> 'withdrawn'
BEGIN
  SELECT RAISE(ABORT, 'iris: a withdrawn changeset stays withdrawn');
END;

-- A changeset is 'draft' until an action has a state, then 'open' while one
-- or more actions are open, else 'closed'. changeset_state checks the same.
CREATE TRIGGER action_closes AFTER UPDATE OF state ON changeset_action
BEGIN
  UPDATE changeset SET state =
    CASE WHEN EXISTS (SELECT 1 FROM changeset_action WHERE changeset_id = new.changeset_id
                      AND state IN ('proposed', 'in_review', 'approved', 'held'))
           THEN 'open'
         WHEN EXISTS (SELECT 1 FROM changeset_action WHERE changeset_id = new.changeset_id
                      AND state IS NOT NULL)
           THEN 'closed'
         ELSE 'draft' END
  WHERE id = new.changeset_id AND state <> 'withdrawn';
END;

-- The state of a changeset comes from its actions. Only a withdraw writes
-- another value.
CREATE TRIGGER changeset_state BEFORE UPDATE OF state ON changeset
WHEN new.state <> 'withdrawn' AND new.state IS NOT (
  CASE WHEN EXISTS (SELECT 1 FROM changeset_action WHERE changeset_id = new.id
                    AND state IN ('proposed', 'in_review', 'approved', 'held'))
         THEN 'open'
       WHEN EXISTS (SELECT 1 FROM changeset_action WHERE changeset_id = new.id
                    AND state IS NOT NULL)
         THEN 'closed'
       ELSE 'draft' END)
BEGIN
  SELECT RAISE(ABORT, 'iris: the state of a changeset comes from its actions');
END;

-- A submitted action does not change: its approval hash covers the targets,
-- the arguments and the resolved messages (section 5.5).
CREATE TRIGGER action_fixed BEFORE UPDATE OF verb, targets, args, why, hash,
  submitted_at ON changeset_action
WHEN old.state IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- Flags are a JSON array of the names of section 5.3.
CREATE TRIGGER action_flags_insert BEFORE INSERT ON changeset_action
WHEN json_type(new.flags) IS NOT 'array'
  OR EXISTS (SELECT 1 FROM json_each(new.flags)
             WHERE type <> 'text' OR value NOT IN ('new_recipient', 'outside_thread', 'money', 'date', 'attachment',
                   'first_contact', 'untrusted_content', 'bulk'))
BEGIN
  SELECT RAISE(ABORT, 'iris: the flags are not valid');
END;

-- The engine can add a flag to an open action before apply, for example
-- bulk (section 5.4). A flag does not go away.
CREATE TRIGGER action_flags BEFORE UPDATE OF flags ON changeset_action
WHEN json_type(new.flags) IS NOT 'array'
  OR EXISTS (SELECT 1 FROM json_each(new.flags)
             WHERE type <> 'text' OR value NOT IN ('new_recipient', 'outside_thread', 'money', 'date', 'attachment',
                   'first_contact', 'untrusted_content', 'bulk'))
  OR (old.state IS NOT NULL
      AND (old.state NOT IN ('proposed', 'in_review', 'approved')
           OR new.state NOT IN ('proposed', 'in_review', 'approved')
           OR EXISTS (SELECT 1 FROM json_each(old.flags) o
                      WHERE NOT EXISTS (SELECT 1 FROM json_each(new.flags) f
                                        WHERE f.value IS o.value))))
BEGIN
  SELECT RAISE(ABORT, 'iris: the flags are not valid, or a flag of a submitted action goes away');
END;

CREATE TRIGGER action_message_insert BEFORE INSERT ON action_message
WHEN (SELECT state FROM changeset_action WHERE id = new.action_id) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

CREATE TRIGGER action_message_delete BEFORE DELETE ON action_message
WHEN (SELECT state FROM changeset_action WHERE id = old.action_id) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

CREATE TRIGGER action_delete BEFORE DELETE ON changeset_action
WHEN old.state IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- Submit inserts and deletes these rows. Nothing updates them.
CREATE TRIGGER action_message_update BEFORE UPDATE ON action_message
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- The record of a decision changes only with a move of the state.
CREATE TRIGGER action_decision BEFORE UPDATE OF decided_by, rule, policy, reason,
  decided_at ON changeset_action
WHEN old.state IS NOT NULL AND new.state IS old.state
BEGIN
  SELECT RAISE(ABORT, 'iris: a decision changes only with its state');
END;
