-- Migration 32: the stop switch (docs/specs/2026-11-17-stop.md, card R4-11).
-- One row. `stopped_at` is NULL while Iris runs. `iris stop` and
-- `POST /api/stop` set it; `iris resume` and `POST /api/resume` clear it.

CREATE TABLE stop_switch (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  stopped_at INTEGER CHECK (stopped_at IS NULL OR (typeof(stopped_at) = 'integer' AND stopped_at >= 0)),
  changed_at INTEGER NOT NULL CHECK (typeof(changed_at) = 'integer' AND changed_at >= 0)
);
INSERT INTO stop_switch(id, stopped_at, changed_at) VALUES (1, NULL, 0);

-- The backstop of the stop: no session registers while stopped. Stop ends
-- each open session in the transaction that sets the switch, so no session
-- is open while the switch is set.
CREATE TRIGGER job_session_stop BEFORE INSERT ON job_session
WHEN (SELECT stopped_at FROM stop_switch WHERE id = 1) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: Iris is stopped: run iris resume first');
END;

-- The backstop of the stop: no new ledger entry while stopped, so no apply
-- and no release claims an action. Only a state verb (no account) of a
-- changeset of the person can apply: it changes only the items of the store.
CREATE TRIGGER ledger_stop BEFORE INSERT ON ledger
WHEN (SELECT stopped_at FROM stop_switch WHERE id = 1) IS NOT NULL
  AND (NEW.account_id IS NOT NULL
       OR coalesce((SELECT c.author_kind FROM changeset_action a
                    JOIN changeset c ON c.id = a.changeset_id
                    WHERE a.id = NEW.action_id), '') <> 'person')
BEGIN
  SELECT RAISE(ABORT, 'iris: Iris is stopped: no action applies until iris resume');
END;
