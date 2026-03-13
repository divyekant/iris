-- migrations/041_extracted_data.sql

CREATE TABLE IF NOT EXISTS extracted_data (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL,
    account_id INTEGER NOT NULL,
    data_type TEXT NOT NULL, -- 'date', 'amount', 'address', 'tracking', 'flight', 'order', 'contact', 'link'
    data_key TEXT NOT NULL, -- e.g. 'delivery_date', 'total_amount', 'tracking_number'
    data_value TEXT NOT NULL, -- the extracted value
    confidence REAL NOT NULL DEFAULT 1.0, -- 0.0 to 1.0
    source TEXT NOT NULL DEFAULT 'ai', -- 'ai' or 'regex'
    extracted_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_extracted_data_message ON extracted_data(message_id);
CREATE INDEX IF NOT EXISTS idx_extracted_data_account ON extracted_data(account_id);
CREATE INDEX IF NOT EXISTS idx_extracted_data_type ON extracted_data(data_type);

INSERT OR IGNORE INTO schema_version (version) VALUES (7);
