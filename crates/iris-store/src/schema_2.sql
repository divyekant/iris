-- Specification 1 section 6.7: the messages that prove the sender kind of
-- an address. The sender step writes the rows. A `sent` row says that the
-- sent message names the address in To, Cc or Bcc. A `replied` row says
-- that the sent message comes after a message from the address in its
-- thread. A `service` row says that the message from the address is
-- automated and has a Gmail DKIM pass. A row counts only while its message
-- is there with the same content hash.
CREATE TABLE sender_evidence (
  message_id     INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  address        TEXT NOT NULL,
  rule           TEXT NOT NULL CHECK (rule IN ('sent', 'replied', 'service')),
  content_sha256 TEXT,
  PRIMARY KEY (address, rule, message_id)
) WITHOUT ROWID;
CREATE INDEX sender_evidence_message ON sender_evidence(message_id);
