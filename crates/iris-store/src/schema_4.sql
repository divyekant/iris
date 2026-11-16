-- Spec 2 section 6.3: the active policy, and each rule ID that an active
-- policy used.

-- One row (id 1): the text of the policy file that the person confirmed, and
-- its SHA-256 hash in lower-case hex. No row: the policy of the first start.
CREATE TABLE policy_active (
  id           INTEGER PRIMARY KEY CHECK (id = 1),
  text         TEXT NOT NULL,
  sha256       TEXT NOT NULL,
  confirmed_at INTEGER NOT NULL
);

-- Each rule ID of each active policy. `removed_at` is set when a confirmed
-- policy no longer has the ID. Iris never uses a removed ID again, so a
-- ledger entry always names one rule.
CREATE TABLE policy_rule_id (
  id         INTEGER PRIMARY KEY CHECK (id > 0),
  removed_at INTEGER
);
