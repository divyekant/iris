-- Migration 69 (issue 455): the header keeps the `Reply-To` addresses of
-- a message (bare, lowercase, comma-separated), as `to_addrs` and
-- `cc_addrs`. Understanding writes the value from the raw message. A row
-- that understanding wrote before this migration has NULL until the next
-- understanding of the message: a reply to it goes to the `From` address.
ALTER TABLE header ADD COLUMN reply_to TEXT;
-- `content_sha256`: the content that the row comes from. A sync of new
-- content writes an empty row with NULL here, so the row is current only
-- while this value is the `content_sha256` of its message. The reply form
-- gives no recipients from a row that is not current.
ALTER TABLE header ADD COLUMN content_sha256 TEXT;
-- A row with a parsed value comes from the content of its message now: a
-- sync of new content writes a row with no value.
UPDATE header SET content_sha256 = (SELECT content_sha256 FROM message WHERE id = header.message_id)
WHERE coalesce(date, from_addr, from_name, to_addrs, cc_addrs, subject, message_id_hdr,
               in_reply_to, list_id, auth) IS NOT NULL;
