-- The local API and the attention of each item look up a sender with
-- `address = ? COLLATE NOCASE`. This index serves that lookup, so it does
-- not scan the sender table once for each message.
CREATE INDEX sender_address_nocase ON sender(address COLLATE NOCASE);
