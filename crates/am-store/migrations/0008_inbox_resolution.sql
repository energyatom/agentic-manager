-- 0008: an Inbox item can be resolved (epic 3 story 4; AD-7, AD-15,
-- AD-28, AD-36, AD-43).
--
-- 0007's `inbox_items.status` takes `open` only, and SQLite cannot alter a
-- CHECK, so the table is rebuilt with SQLite's own procedure (create,
-- copy, drop, rename), as 0007 did for `step_runs`; the migrator runs the
-- step with foreign keys off, counts broken references before and after,
-- and AD-7's pre-migration copy is taken first.
--
-- Every 0007 column and constraint is kept. `status` takes `open` or
-- `resolved`. New: `max_returns` (a Review's, from its gate, 0 to 5),
-- `decision` and `note` (a return's note or a Question's answer, stored
-- here once and nowhere else). A table CHECK ties the resolution fields to
-- the status: an open row has none, a resolved row has its decision, its
-- resolver (the actor as JSON) and its time. `decision` has no spelling
-- CHECK: the writer checks it against the item's kind, so later stories
-- add their decisions without another rebuild.
--
-- Every row is copied as it is, `open`, with no decision or note; a
-- Review takes `max_returns` 2 (the default its gate had: 3.3 had no
-- `max_returns` key). `resolved_by` and `resolved_at` are copied as they
-- are: no 3.3 command wrote them, so a forged row that has them fails the
-- table CHECK and the step fails closed.
--
-- Triggers: the taint only rises (0007's, made again), a resolved row
-- never changes, no row is ever replaced (review R4, S2: an `INSERT OR
-- REPLACE` over an existing row is refused), and an item's own fields
-- never change.
CREATE TABLE inbox_items_0008 (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('review', 'approval', 'draft', 'question', 'budget_ask', 'failure', 'proposal')),
    status TEXT NOT NULL CHECK (status IN ('open', 'resolved')),
    run_id TEXT NOT NULL REFERENCES runs(id),
    step_run_id TEXT NOT NULL REFERENCES step_runs(id),
    payload TEXT REFERENCES step_runs(id),
    taint INTEGER NOT NULL DEFAULT 0 CHECK (taint IN (0, 1)),
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    resolved_by TEXT,
    max_returns INTEGER CHECK (max_returns BETWEEN 0 AND 5),
    decision TEXT,
    note TEXT,
    UNIQUE (step_run_id, kind),
    CHECK (
        (status = 'open' AND decision IS NULL AND note IS NULL AND resolved_by IS NULL AND resolved_at IS NULL)
        OR (status = 'resolved' AND decision IS NOT NULL AND resolved_by IS NOT NULL AND resolved_at IS NOT NULL)
    )
);
INSERT INTO inbox_items_0008 (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by, max_returns, decision, note)
    SELECT id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by,
           CASE WHEN kind = 'review' THEN 2 END, NULL, NULL
    FROM inbox_items;
DROP TABLE inbox_items;
ALTER TABLE inbox_items_0008 RENAME TO inbox_items;

-- Dropping the table dropped 0007's trigger: it is made again, beside the
-- two new ones.
CREATE TRIGGER inbox_items_taint_only_rises BEFORE UPDATE OF taint ON inbox_items
WHEN NEW.taint < OLD.taint
BEGIN SELECT RAISE(ABORT, 'inbox_items.taint only rises'); END;

-- A resolved item never changes again: any UPDATE of it aborts.
CREATE TRIGGER inbox_items_resolved_never_change BEFORE UPDATE ON inbox_items
WHEN OLD.status = 'resolved'
BEGIN SELECT RAISE(ABORT, 'a resolved Inbox item never changes'); END;

-- Nor is any item replaced (review R4, S2): an `INSERT OR REPLACE` (or
-- `REPLACE`) over an existing row, open or resolved, by its id or by its
-- (step run, kind), would delete it without firing an UPDATE trigger
-- (SQLite fires no delete trigger on a REPLACE unless recursive triggers
-- are on), lowering an open Review's taint or rewriting its words. So an
-- insert that meets an existing row is refused. Made after the copy above,
-- so the copy itself is never refused.
CREATE TRIGGER inbox_items_never_replaced BEFORE INSERT ON inbox_items
WHEN EXISTS (
    SELECT 1 FROM inbox_items
    WHERE id = NEW.id OR (step_run_id = NEW.step_run_id AND kind = NEW.kind)
)
BEGIN SELECT RAISE(ABORT, 'an Inbox item is never replaced'); END;

-- An item's own fields are the engine's, fixed when it is made.
CREATE TRIGGER inbox_items_fixed_fields BEFORE UPDATE OF id, kind, run_id, step_run_id, payload, title, body, created_at, max_returns ON inbox_items
BEGIN SELECT RAISE(ABORT, 'an Inbox item''s own fields never change'); END;

CREATE INDEX inbox_items_by_status ON inbox_items (status, created_at);

-- A step input may come from an item (a `gate_note`: a Human return note,
-- whose text stays on the item; AD-43: stored once).
ALTER TABLE step_inputs ADD COLUMN from_item TEXT REFERENCES inbox_items(id);
