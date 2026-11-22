-- 0006: the run's pin and the journal's times (epic 3 story 1; AD-8, AD-16).
-- A run records the hash of the config snapshot it was built from
-- (`config_hash`, written by the command that creates it) and when it
-- finished; a step run records when it started and when it finished. The
-- writer stamps the times itself. Rows from before this version keep NULL
-- in all four: nothing is made up for them.
ALTER TABLE runs ADD COLUMN config_hash TEXT;
ALTER TABLE runs ADD COLUMN finished_at TEXT;
ALTER TABLE step_runs ADD COLUMN started_at TEXT;
ALTER TABLE step_runs ADD COLUMN finished_at TEXT;
