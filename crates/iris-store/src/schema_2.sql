-- Specification 1 section 5.3: the Bcc addresses of a sent message, for the
-- sender rule of section 6.7. The sender step writes the column.
ALTER TABLE header ADD COLUMN bcc_addrs TEXT;
