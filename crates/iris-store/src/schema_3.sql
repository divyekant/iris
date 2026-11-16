-- docs/specs/2026-11-16-cost-control.md: the model of understanding, the
-- budget, the counts of each run and the estimate receipts.

-- One row (id 1). Prices are micro-USD for one million tokens; NULL is no
-- price (a local model). `api_key` is sealed with the store key.
-- `budget_micro_usd` is NULL with no approval. It can go below zero once,
-- when a provider bills more input tokens than the request body bytes.
CREATE TABLE model_setting (
  id               INTEGER PRIMARY KEY CHECK (id = 1),
  provider         TEXT NOT NULL CHECK (provider IN ('anthropic', 'local')),
  base_url         TEXT NOT NULL,
  model            TEXT NOT NULL,
  version          INTEGER NOT NULL,
  price_in         INTEGER CHECK (price_in >= 0),
  price_out        INTEGER CHECK (price_out >= 0),
  api_key          BLOB,
  max_tokens       INTEGER NOT NULL CHECK (max_tokens > 0),
  timeout_s        INTEGER NOT NULL CHECK (timeout_s > 0),
  budget_micro_usd INTEGER,
  -- A hosted model always has both prices, so each of its calls needs a
  -- reservation. Only a local model has no price.
  CHECK ((provider = 'local' AND price_in IS NULL AND price_out IS NULL)
      OR (provider <> 'local' AND price_in IS NOT NULL AND price_out IS NOT NULL))
);

CREATE TABLE understand_run (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  messages INTEGER NOT NULL DEFAULT 0,
  calls INTEGER NOT NULL DEFAULT 0,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_micro_usd INTEGER,
  unknown_calls INTEGER NOT NULL DEFAULT 0,
  unknown_cost_micro_usd INTEGER,
  -- For a priced model, the runner inserts cost_micro_usd and
  -- unknown_cost_micro_usd as 0. They stay NULL only with no price.
  reserved_micro_usd INTEGER NOT NULL DEFAULT 0,
  stop TEXT
);

CREATE TABLE understand_estimate (
  code TEXT PRIMARY KEY,          -- 16 random bytes, hex
  model_version INTEGER NOT NULL, -- model_setting.version
  messages INTEGER NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cost_micro_usd INTEGER,
  created_at INTEGER NOT NULL
);
