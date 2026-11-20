-- 0003: redacted arguments on audit entries (AD-25, story 1.7): ids, content
-- hashes, counts and statuses derived from each writer command, as compact
-- JSON; never content. NULL on entries written before this version and on
-- migration entries, whose hashes keep the spike's formula.
ALTER TABLE audit_log ADD COLUMN args TEXT;
