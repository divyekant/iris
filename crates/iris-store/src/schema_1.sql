-- Migration 1: the tables of release 1 (specification 1, section 5.5).
-- Times are milliseconds since the Unix epoch, UTC.

CREATE TABLE account (
  id INTEGER PRIMARY KEY,
  provider TEXT NOT NULL,
  address TEXT NOT NULL UNIQUE,
  history_id TEXT,
  state TEXT,
  token BLOB,
  mirror_ready_at INTEGER,
  sync_kind TEXT,
  sync_start TEXT,
  sync_step INTEGER
);

CREATE TABLE sync_seen (
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  message_id TEXT NOT NULL,
  labels TEXT NOT NULL,
  PRIMARY KEY (account_id, message_id)
) WITHOUT ROWID;

CREATE TABLE label (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  provider_id TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('system', 'user')),
  UNIQUE (account_id, provider_id)
);

CREATE TABLE thread (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  provider_id TEXT NOT NULL,
  viewed_at INTEGER,
  UNIQUE (account_id, provider_id)
);

CREATE TABLE message (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  thread_id INTEGER NOT NULL REFERENCES thread(id),
  provider_id TEXT NOT NULL,
  content_sha256 TEXT,
  size INTEGER,
  internal_date INTEGER,
  fetched_at INTEGER,
  gone_at INTEGER,
  understood_at INTEGER,
  hostile TEXT,
  tries INTEGER NOT NULL DEFAULT 0,
  retry_at INTEGER,
  UNIQUE (account_id, provider_id)
);

CREATE TABLE message_label (
  message_id INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  label_id INTEGER NOT NULL REFERENCES label(id) ON DELETE CASCADE,
  PRIMARY KEY (message_id, label_id)
) WITHOUT ROWID;

CREATE TABLE header (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  date INTEGER,
  from_addr TEXT,
  from_name TEXT,
  to_addrs TEXT,
  cc_addrs TEXT,
  subject TEXT,
  message_id_hdr TEXT,
  in_reply_to TEXT,
  list_id TEXT,
  auth TEXT
);

CREATE TABLE body (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  subject TEXT NOT NULL,
  sender TEXT NOT NULL,
  text TEXT NOT NULL
);

CREATE VIRTUAL TABLE message_fts USING fts5(
  subject, sender, text,
  content = 'body',
  content_rowid = 'message_id',
  tokenize = 'unicode61 remove_diacritics 2',
  prefix = '2 3'
);

CREATE TRIGGER body_ai AFTER INSERT ON body BEGIN
  INSERT INTO message_fts(rowid, subject, sender, text)
  VALUES (new.message_id, new.subject, new.sender, new.text);
END;

CREATE TRIGGER body_ad AFTER DELETE ON body BEGIN
  INSERT INTO message_fts(message_fts, rowid, subject, sender, text)
  VALUES ('delete', old.message_id, old.subject, old.sender, old.text);
END;

CREATE TRIGGER body_au AFTER UPDATE ON body BEGIN
  INSERT INTO message_fts(message_fts, rowid, subject, sender, text)
  VALUES ('delete', old.message_id, old.subject, old.sender, old.text);
  INSERT INTO message_fts(rowid, subject, sender, text)
  VALUES (new.message_id, new.subject, new.sender, new.text);
END;

CREATE TABLE sender (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  address TEXT NOT NULL UNIQUE,
  name TEXT,
  kind TEXT,
  basis TEXT,
  decided_by TEXT,
  decided_at INTEGER
);

CREATE TABLE item (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  form TEXT NOT NULL,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  status_before_gone TEXT,
  doc TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  derived_by TEXT,
  duplicate_of INTEGER REFERENCES item(id)
);

CREATE TABLE item_source (
  item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  message_id INTEGER NOT NULL REFERENCES message(id),
  role TEXT NOT NULL,
  PRIMARY KEY (item_id, message_id)
) WITHOUT ROWID;

CREATE TABLE correction (
  id INTEGER PRIMARY KEY,
  item_id INTEGER NOT NULL REFERENCES item(id),
  field TEXT NOT NULL,
  value TEXT NOT NULL,
  by TEXT NOT NULL,
  at INTEGER NOT NULL
);

CREATE TABLE sync_log (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  result TEXT
);

-- The indexes for the queries of the brief (section 5.5).
CREATE INDEX item_form_status ON item(form, status);
CREATE INDEX message_thread_date ON message(thread_id, internal_date);
CREATE INDEX header_from_addr ON header(from_addr);
-- Search sorts by date, and the content cleanup (section 4.5) counts the
-- messages that refer to one content file.
CREATE INDEX message_date ON message(internal_date);
CREATE INDEX message_content ON message(content_sha256);
-- The foreign keys of item_source and correction need an index on the child
-- column, or a delete of a message or an item scans the full table.
CREATE INDEX item_source_message ON item_source(message_id);
CREATE INDEX correction_item ON correction(item_id);
