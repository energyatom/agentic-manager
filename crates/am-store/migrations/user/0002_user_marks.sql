-- user 0002: the user store's marks (story 2.13; AD-15 *Migrate*, AD-17
-- *Recovery*). One row. `recovered` is 1 when am-app created this store
-- while the user Accounts file or `backups/accounts-migrate/` already
-- existed: such a store carries no project's confirmation over. A store
-- created normally, and every store made before this migration, is 0.
CREATE TABLE user_marks (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    recovered INTEGER NOT NULL DEFAULT 0 CHECK (recovered IN (0, 1)),
    recovered_at TEXT
);
INSERT INTO user_marks (id, recovered) VALUES (1, 0);
CREATE TRIGGER user_marks_no_delete BEFORE DELETE ON user_marks
BEGIN SELECT RAISE(ABORT, 'user_marks keeps its one row'); END;
CREATE TRIGGER user_marks_no_unmark BEFORE UPDATE ON user_marks
WHEN OLD.recovered = 1 AND NEW.recovered = 0
BEGIN SELECT RAISE(ABORT, 'a recovered store stays recovered'); END;
