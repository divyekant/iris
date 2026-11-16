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
  -- The first submit sets 'open'. Then the trigger action_closes keeps
  -- 'open' and 'closed' current.
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

CREATE TRIGGER action_insert BEFORE INSERT ON changeset_action
WHEN new.state IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a new action has no state');
END;

-- Only submit gives a draft action a state other than 'cancelled': it sets
-- the hash and the submit time in the same write.
CREATE TRIGGER action_state BEFORE UPDATE OF state ON changeset_action
WHEN NOT EXISTS (SELECT 1 FROM action_transition
                 WHERE from_state IS old.state AND to_state = new.state)
  OR (old.state IS NULL AND new.state <> 'cancelled'
      AND (new.hash IS NULL OR new.submitted_at IS NULL))
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
  -- A rule decision names a rule ID of the active policy (section 6.3).
  OR (new.decided_by IS 'rule' AND new.state IN ('in_review', 'approved', 'denied')
      AND (new.policy IS NOT (SELECT max(version) FROM policy_version)
           OR NOT EXISTS (SELECT 1 FROM policy_rule_id
                          WHERE id = new.rule AND removed_at IS NULL)))
BEGIN
  SELECT RAISE(ABORT, 'iris: this state change of an action is not allowed');
END;

-- A changeset is 'open' while one or more actions are open, else 'closed'.
CREATE TRIGGER action_closes AFTER UPDATE OF state ON changeset_action
BEGIN
  UPDATE changeset SET state = CASE WHEN EXISTS (
      SELECT 1 FROM changeset_action
      WHERE changeset_id = new.changeset_id
        AND state IN ('proposed', 'in_review', 'approved', 'held'))
    THEN 'open' ELSE 'closed' END
  WHERE id = new.changeset_id AND state IN ('open', 'closed');
END;

-- A submitted action does not change: its approval hash covers the targets,
-- the arguments and the resolved messages (section 5.5).
CREATE TRIGGER action_fixed BEFORE UPDATE OF changeset_id, revision, n, verb, targets,
  args, why, flags, hash, submitted_at ON changeset_action
WHEN old.state IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
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
