-- Migration 36: the IMAP account (docs/specs/2026-11-17-imap-provider.md,
-- section 8, card R6-03). The server and the user name of an account with
-- the provider `imap`. The password is the sealed secret in `token`, as the
-- Gmail refresh token is. A Gmail account has NULL in the three columns.

ALTER TABLE account ADD COLUMN imap_host TEXT;
ALTER TABLE account ADD COLUMN imap_port INTEGER;
ALTER TABLE account ADD COLUMN imap_user TEXT;
