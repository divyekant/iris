-- Migration 24: the send mark of the write view (card R3-04,
-- docs/specs/2026-11-17-write.md, section 3).
--
-- One press of Send in the write view approves a `draft` action of the
-- person and asks for the send of the draft that the action makes. The
-- mark keeps that ask until the draft exists at Gmail. Step 2a of the
-- apply step then makes the send and writes its ID here.

CREATE TABLE write_send (
  -- The approved `draft` action of a write.
  action_id INTEGER PRIMARY KEY REFERENCES changeset_action(id),
  -- The `send` action that step 2a made. NULL before step 2a.
  send_action_id INTEGER UNIQUE REFERENCES changeset_action(id),
  -- The key of the press. A second try with the same key and the same mail
  -- finds this write and makes no second mail.
  request_key TEXT NOT NULL UNIQUE
    CHECK (length(request_key) BETWEEN 16 AND 64 AND request_key NOT GLOB '*[^A-Za-z0-9-]*'),
  -- The SHA-256 of the target and the args of the press.
  request_hash TEXT NOT NULL CHECK (length(request_hash) = 64),
  -- The warnings that the person saw at the press, as a JSON list. A send
  -- with another warning goes to review.
  flags TEXT NOT NULL CHECK (json_valid(flags) AND json_type(flags) = 'array'),
  created_at INTEGER NOT NULL CHECK (typeof(created_at) = 'integer' AND created_at >= 0)
);

-- Only the write path makes a mark: an approved `draft` action that the
-- person decided, in a changeset of the author `person`, with no send yet.
CREATE TRIGGER write_send_insert BEFORE INSERT ON write_send
WHEN new.send_action_id IS NOT NULL
  OR NOT EXISTS (SELECT 1 FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
                 WHERE a.id = new.action_id AND a.verb = 'draft' AND a.state = 'approved'
                   AND a.decided_by = 'person' AND c.author_kind = 'person')
BEGIN
  SELECT RAISE(ABORT, 'iris: a send mark needs an approved draft action of the person');
END;

-- A mark changes once: `send_action_id` from NULL to a `send` action of
-- the person. The other fields do not change.
CREATE TRIGGER write_send_update BEFORE UPDATE ON write_send
WHEN new.action_id IS NOT old.action_id OR new.created_at IS NOT old.created_at
  OR new.request_key IS NOT old.request_key OR new.request_hash IS NOT old.request_hash
  OR new.flags IS NOT old.flags
  OR old.send_action_id IS NOT NULL OR new.send_action_id IS NULL
  OR NOT EXISTS (SELECT 1 FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
                 WHERE a.id = new.send_action_id AND a.verb = 'send' AND c.author_kind = 'person')
BEGIN
  SELECT RAISE(ABORT, 'iris: a send mark changes only once, to a send of the person');
END;

CREATE TRIGGER write_send_delete BEFORE DELETE ON write_send
BEGIN
  SELECT RAISE(ABORT, 'iris: a send mark is not deleted');
END;

-- The sync thread looks for a new mark each second (write spec, section 7).
CREATE INDEX write_send_created ON write_send(created_at);
