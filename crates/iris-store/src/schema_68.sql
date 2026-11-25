-- Migration 68 (issue 455): the header keeps the `Reply-To` addresses of
-- a message (bare, lowercase, comma-separated), as `to_addrs` and
-- `cc_addrs`. Understanding writes the value from the raw message. A row
-- that understanding wrote before this migration has NULL until the next
-- understanding of the message: a reply to it goes to the `From` address.
ALTER TABLE header ADD COLUMN reply_to TEXT;
