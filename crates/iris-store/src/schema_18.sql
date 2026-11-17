-- Migration 18: the hostile mark `link` (card R5-10, specification 1,
-- section 6.7).
--
-- Mail that the engine understood before this migration had no link check.
-- One row for each such message. The batch runner reads each message once
-- (`iris_understand::store::recheck_links`): if the message has a deceptive
-- link, the runner understands it again, with no model call, so it goes to
-- quarantine. Then the row goes.

CREATE TABLE link_check (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE
);

INSERT INTO link_check(message_id)
  SELECT id FROM message WHERE understood_at IS NOT NULL AND gone_at IS NULL;
