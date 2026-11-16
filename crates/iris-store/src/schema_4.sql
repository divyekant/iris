-- Spec 2 section 6.3: each policy that the person confirmed, and each rule ID
-- that a confirmed policy used.

-- One row for each confirmed text of the policy file, with its SHA-256 hash
-- in lower-case hex. The row with the largest `version` is the active policy.
-- No row: the policy of the first start. Rows are never changed or deleted,
-- so a decision that names a version and a rule ID names one rule text.
CREATE TABLE policy_version (
  version      INTEGER PRIMARY KEY AUTOINCREMENT,
  text         TEXT NOT NULL,
  sha256       TEXT NOT NULL,
  confirmed_at INTEGER NOT NULL
);

-- Each rule ID of each confirmed policy. `removed_at` is set when a confirmed
-- policy no longer has the ID. Iris never uses a removed ID again.
CREATE TABLE policy_rule_id (
  id         INTEGER PRIMARY KEY CHECK (id > 0),
  removed_at INTEGER
);
