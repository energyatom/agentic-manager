-- 0005: audit notes on a config intent (story 2.1; AD-15, AD-16, AD-25). A
-- config write may carry extra audit entries (a widening record such as
-- `account_added`: ids, hosts, ports and paths; never content or a secret).
-- They are kept with the pending intent as a JSON list and written to the
-- audit log only in the transaction that commits or reconciles it; a
-- dropped intent's notes go with its row. History entries hold no notes:
-- the audit log is their record. This version also brings the `create`
-- kind (a ConfigCreate), which needs no schema change.
ALTER TABLE config_intents ADD COLUMN audit_notes TEXT NOT NULL DEFAULT '[]';
