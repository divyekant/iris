-- Migration 13: the rules that Iris proposes from the approvals of the
-- person (specification 2, section 6.6, card R5-01).
--
-- A proposal is a row here, never a line in `policy.toml`. Only an accept
-- by the person adds the rule to the file, with `by = "proposal"`. The
-- engine adds, updates and deletes `open` rows on each pass. An `accepted`
-- or a `dismissed` row stays, so that Iris does not propose the same rule
-- again.

CREATE TABLE rule_proposal (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  verb TEXT NOT NULL CHECK (verb IN ('archive', 'inbox', 'read', 'unread', 'trash')),
  -- The one condition of the rule, and its value.
  cond TEXT NOT NULL CHECK (cond IN ('sender', 'sender_kind')),
  value TEXT NOT NULL,
  -- The number of approvals that the rule would have decided.
  approvals INTEGER NOT NULL CHECK (approvals >= 1),
  state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'accepted', 'dismissed')),
  -- The ID of the rule in the file, after the accept.
  rule INTEGER CHECK ((state = 'accepted') = (rule IS NOT NULL)),
  -- Seconds since 1970-01-01 UTC.
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (verb, cond, value)
);

-- The person decides a proposal one time.
CREATE TRIGGER rule_proposal_state BEFORE UPDATE OF state ON rule_proposal
WHEN old.state <> 'open' AND new.state IS NOT old.state
BEGIN
  SELECT RAISE(ABORT, 'iris: a decided proposal does not change');
END;

CREATE TRIGGER rule_proposal_delete BEFORE DELETE ON rule_proposal
WHEN old.state <> 'open'
BEGIN
  SELECT RAISE(ABORT, 'iris: a decided proposal does not change');
END;
