-- 0010: rows with their history, what an attempt may write, and a tool
-- call's result kind (epic 3 story 6, the store slice; AD-7, AD-8, AD-10,
-- AD-21, AD-24, AD-25, AD-35).
--
-- Additive and forward-only: two new tables, four new columns (NULL on
-- every old row) and their triggers. Nothing is rewritten, moved or
-- dropped; AD-7's pre-migration copy is taken first. No `from_row` (3.6c's
-- 0011) and no `document_versions` (3.6b's 0012).

-- One row per current row of every config table (AD-21: one generic store,
-- no per-table DDL). `data` is the row's columns as a JSON object;
-- `version` is the version of its newest after-image; `taint` is the row's
-- own taint, stored on write, which only rises (AD-10).
CREATE TABLE table_rows (
    table_id TEXT NOT NULL,
    row_id TEXT NOT NULL,
    data TEXT NOT NULL CHECK (json_valid(data) AND json_type(data) = 'object'),
    version INTEGER NOT NULL CHECK (version >= 1),
    taint INTEGER NOT NULL CHECK (taint IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (table_id, row_id)
);

-- The append-only after-images (AD-21, AD-24): the whole row after each
-- change, its taint after that write, the Actor (as JSON) and, for a
-- seat's write, the run, the step run, the seat, the call sequence and the
-- tool call. `(step_run_id, call_seq)` is the write's key (AD-8): one
-- write per call. A later Human or Import write has no provenance columns.
CREATE TABLE row_history (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id TEXT NOT NULL,
    row_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 1),
    data TEXT NOT NULL CHECK (json_valid(data) AND json_type(data) = 'object'),
    taint INTEGER NOT NULL CHECK (taint IN (0, 1)),
    actor TEXT NOT NULL CHECK (
        json_valid(actor) AND json_type(actor) = 'object' AND json_type(actor, '$.kind') IS 'text'
    ),
    run_id TEXT REFERENCES runs(id),
    step_run_id TEXT REFERENCES step_runs(id),
    seat TEXT,
    call_seq INTEGER,
    tool_call_id TEXT REFERENCES tool_calls(id),
    op TEXT NOT NULL CHECK (op IN ('insert', 'update')),
    created_at TEXT NOT NULL,
    UNIQUE (table_id, row_id, version),
    UNIQUE (step_run_id, call_seq),
    CHECK (
        (run_id IS NOT NULL AND step_run_id IS NOT NULL AND seat IS NOT NULL AND call_seq IS NOT NULL AND tool_call_id IS NOT NULL)
        OR (run_id IS NULL AND step_run_id IS NULL AND seat IS NULL AND call_seq IS NULL AND tool_call_id IS NULL)
    ),
    CHECK ((json_extract(actor, '$.kind') IS 'seat') = (step_run_id IS NOT NULL)),
    CHECK ((op = 'insert') = (version = 1))
);

-- Append-only: an after-image never changes or goes.
CREATE TRIGGER row_history_no_update BEFORE UPDATE ON row_history
BEGIN SELECT RAISE(ABORT, 'row_history is append-only'); END;
CREATE TRIGGER row_history_no_delete BEFORE DELETE ON row_history
BEGIN SELECT RAISE(ABORT, 'row_history is append-only'); END;

-- Nor is one replaced (the 3.5 lesson): an `INSERT OR REPLACE` meeting an
-- existing `seq`, version key or call key would delete it without firing a
-- DELETE trigger, so an insert that meets one is refused.
CREATE TRIGGER row_history_never_replaced BEFORE INSERT ON row_history
WHEN EXISTS (
    SELECT 1 FROM row_history
    WHERE seq = NEW.seq
       OR (table_id = NEW.table_id AND row_id = NEW.row_id AND version = NEW.version)
       OR (step_run_id = NEW.step_run_id AND call_seq = NEW.call_seq)
)
BEGIN SELECT RAISE(ABORT, 'row_history is append-only: an after-image is never replaced'); END;

-- An after-image is the next version of its row: 1 for a new row, the
-- current version + 1 otherwise. It is written before the row changes.
CREATE TRIGGER row_history_next_version BEFORE INSERT ON row_history
WHEN NEW.version <> COALESCE(
    (SELECT version FROM table_rows WHERE table_id = NEW.table_id AND row_id = NEW.row_id), 0
) + 1
BEGIN SELECT RAISE(ABORT, 'an after-image is the next version of its row'); END;

-- Nor does one lower its row's taint (AD-10): it records the row's taint
-- after the write, which only rises.
CREATE TRIGGER row_history_keeps_taint BEFORE INSERT ON row_history
WHEN NEW.taint < COALESCE(
    (SELECT taint FROM table_rows WHERE table_id = NEW.table_id AND row_id = NEW.row_id), 0
)
BEGIN SELECT RAISE(ABORT, 'an after-image never lowers its row''s taint'); END;

-- A row is never deleted (AD-25's later deletion is a Human command with
-- its own migration) and never replaced, by its key or its rowid. The
-- store turns `recursive_triggers` on, so a REPLACE's delete meets the
-- delete guard too (review R3).
CREATE TRIGGER table_rows_no_delete BEFORE DELETE ON table_rows
BEGIN SELECT RAISE(ABORT, 'a row is never deleted'); END;
CREATE TRIGGER table_rows_never_replaced BEFORE INSERT ON table_rows
WHEN EXISTS (
    SELECT 1 FROM table_rows
    WHERE rowid = NEW.rowid OR (table_id = NEW.table_id AND row_id = NEW.row_id)
)
BEGIN SELECT RAISE(ABORT, 'a row is never replaced'); END;

-- Nor re-created (review R3): a row whose history goes past version 1 was
-- there before, so inserting it again would roll back its version and
-- taint.
CREATE TRIGGER table_rows_never_re_created BEFORE INSERT ON table_rows
WHEN EXISTS (
    SELECT 1 FROM row_history
    WHERE table_id = NEW.table_id AND row_id = NEW.row_id AND version > 1
)
BEGIN SELECT RAISE(ABORT, 'a row is never re-created'); END;

-- Nor moved (review R3): an `UPDATE OR REPLACE` onto another row's rowid
-- would delete that row.
CREATE TRIGGER table_rows_rowid_fixed BEFORE UPDATE ON table_rows
WHEN NEW.rowid IS NOT OLD.rowid
BEGIN SELECT RAISE(ABORT, 'a row''s rowid never changes'); END;

-- A row's table, id and creation time are fixed when it is made.
CREATE TRIGGER table_rows_fixed_fields BEFORE UPDATE OF table_id, row_id, created_at ON table_rows
BEGIN SELECT RAISE(ABORT, 'a row''s table, id and creation time never change'); END;

-- Every change raises the version by exactly one.
CREATE TRIGGER table_rows_version_steps BEFORE UPDATE ON table_rows
WHEN NEW.version IS NOT OLD.version + 1
BEGIN SELECT RAISE(ABORT, 'a row changes only to its next version'); END;

-- A row's taint only rises (AD-10).
CREATE TRIGGER table_rows_taint_only_rises BEFORE UPDATE OF taint ON table_rows
WHEN NEW.taint < OLD.taint
BEGIN SELECT RAISE(ABORT, 'table_rows.taint only rises'); END;

-- No row changes without its after-image, written first in the same
-- transaction: the version, the data and the taint the row takes.
CREATE TRIGGER table_rows_insert_needs_after_image BEFORE INSERT ON table_rows
WHEN NOT EXISTS (
    SELECT 1 FROM row_history
    WHERE table_id = NEW.table_id AND row_id = NEW.row_id AND version = NEW.version
      AND data = NEW.data AND taint = NEW.taint
)
BEGIN SELECT RAISE(ABORT, 'a row changes only with its after-image'); END;
CREATE TRIGGER table_rows_update_needs_after_image BEFORE UPDATE ON table_rows
WHEN NOT EXISTS (
    SELECT 1 FROM row_history
    WHERE table_id = NEW.table_id AND row_id = NEW.row_id AND version = NEW.version
      AND data = NEW.data AND taint = NEW.taint
)
BEGIN SELECT RAISE(ABORT, 'a row changes only with its after-image'); END;

-- What an attempt may write, journaled when it starts (AD-21, RULE-8): the
-- seat that runs it and the one table it may write (NULL: none). A fact of
-- the attempt, set only by the update that takes it from `queued` to
-- `running` (review R6), and never over writes already set (review T1), so
-- never changed after; NULL on every attempt from before 0010.
ALTER TABLE step_runs ADD COLUMN seat TEXT;
ALTER TABLE step_runs ADD COLUMN writes_table TEXT;
-- An attempt is never replaced (review T1): an `INSERT OR REPLACE` by its
-- id, its rowid or its (run, step, attempt) would delete it without an
-- UPDATE, resetting its writes and lowering its taint.
CREATE TRIGGER step_runs_never_replaced BEFORE INSERT ON step_runs
WHEN EXISTS (
    SELECT 1 FROM step_runs
    WHERE id = NEW.id OR rowid = NEW.rowid
       OR (run_id = NEW.run_id AND step_key = NEW.step_key AND attempt = NEW.attempt)
)
BEGIN SELECT RAISE(ABORT, 'an attempt is never replaced'); END;

-- Nor does an attempt go back to `queued` (review T1; the owner's
-- decision, 2026-11-24), so a round trip through `queued` cannot reopen its
-- writes. AD-8's one edge back is a fan-in whose join is met
-- (`waiting_branches → queued`, story 3.14), and only for an attempt that
-- never started: no seat, no table, no `started_at`. Its one edge into
-- `waiting_branches` is from `queued`. (Story 3.9 relaxes this in its own
-- migration should resume ever requeue an attempt.)
CREATE TRIGGER step_runs_never_requeued BEFORE UPDATE OF status ON step_runs
WHEN NEW.status = 'queued' AND OLD.status IS NOT 'queued' AND OLD.status IS NOT 'waiting_branches'
BEGIN SELECT RAISE(ABORT, 'an attempt never goes back to queued'); END;
CREATE TRIGGER step_runs_requeued_only_unstarted BEFORE UPDATE OF status ON step_runs
WHEN NEW.status = 'queued' AND OLD.status = 'waiting_branches'
 AND (OLD.seat IS NOT NULL OR OLD.writes_table IS NOT NULL OR OLD.started_at IS NOT NULL)
BEGIN SELECT RAISE(ABORT, 'only an attempt that never started goes back to queued'); END;
CREATE TRIGGER step_runs_waits_for_branches_from_queued BEFORE UPDATE OF status ON step_runs
WHEN NEW.status = 'waiting_branches' AND OLD.status IS NOT 'waiting_branches' AND OLD.status IS NOT 'queued'
BEGIN SELECT RAISE(ABORT, 'an attempt waits for branches only from queued'); END;

CREATE TRIGGER step_runs_writes_set_at_start BEFORE UPDATE OF seat, writes_table ON step_runs
WHEN NOT (OLD.status = 'queued' AND NEW.status = 'running')
  OR OLD.seat IS NOT NULL OR OLD.writes_table IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'an attempt''s writes are set only when it starts'); END;

-- A tool call's result kind on its journal row (AD-8; AD-10 as amended
-- 2026-11-24): `engine` for a succeeded `table_write` whose row the
-- writer wrote, `untrusted` for every other finished call; the label names
-- the tool. Set when the call finishes, once.
ALTER TABLE tool_calls ADD COLUMN result_kind TEXT CHECK (result_kind IN ('engine', 'untrusted'));
ALTER TABLE tool_calls ADD COLUMN result_label TEXT;

-- A call is never replaced (the 3.5 lesson): an `INSERT OR REPLACE` by its
-- id, its rowid (review T4) or its (step run, call sequence) would delete
-- it without an UPDATE,
-- clearing a kind that is set once.
CREATE TRIGGER tool_calls_never_replaced BEFORE INSERT ON tool_calls
WHEN EXISTS (
    SELECT 1 FROM tool_calls
    WHERE id = NEW.id OR rowid = NEW.rowid
       OR (step_run_id = NEW.step_run_id AND call_seq = NEW.call_seq)
)
BEGIN SELECT RAISE(ABORT, 'a tool call is never replaced'); END;

-- A call's identity never changes (review R2b): its id, step run, call
-- sequence, tool and rowid (an `UPDATE OR REPLACE` onto another call's
-- would delete it). Finishing a call sets only its status, finish time and
-- result.
CREATE TRIGGER tool_calls_identity_fixed BEFORE UPDATE ON tool_calls
WHEN OLD.result_kind IS NULL AND (
    NEW.rowid IS NOT OLD.rowid OR NEW.id IS NOT OLD.id OR NEW.step_run_id IS NOT OLD.step_run_id
    OR NEW.call_seq IS NOT OLD.call_seq OR NEW.tool IS NOT OLD.tool
)
BEGIN SELECT RAISE(ABORT, 'a tool call''s id, step run, call sequence and tool never change'); END;

-- And once its result kind is set (review R2), nothing of it changes: its
-- identity, its status and its times. (A change of the kind or the label
-- itself meets `tool_calls_result_kind_set_once`.)
CREATE TRIGGER tool_calls_with_a_kind_never_change BEFORE UPDATE ON tool_calls
WHEN OLD.result_kind IS NOT NULL AND (
    NEW.rowid IS NOT OLD.rowid OR NEW.id IS NOT OLD.id OR NEW.step_run_id IS NOT OLD.step_run_id
    OR NEW.call_seq IS NOT OLD.call_seq OR NEW.tool IS NOT OLD.tool OR NEW.status IS NOT OLD.status
    OR NEW.created_at IS NOT OLD.created_at OR NEW.finished_at IS NOT OLD.finished_at
)
BEGIN SELECT RAISE(ABORT, 'a tool call with a result kind never changes'); END;

CREATE TRIGGER tool_calls_result_kind_not_on_insert BEFORE INSERT ON tool_calls
WHEN NEW.result_kind IS NOT NULL OR NEW.result_label IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'a tool call''s result kind is set when it finishes'); END;

CREATE TRIGGER tool_calls_result_kind_set_once BEFORE UPDATE OF result_kind, result_label ON tool_calls
WHEN OLD.result_kind IS NOT NULL OR OLD.result_label IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'a tool call''s result kind is set once'); END;

CREATE TRIGGER tool_calls_result_kind_on_finish BEFORE UPDATE OF result_kind, result_label ON tool_calls
WHEN (NEW.result_kind IS NULL) <> (NEW.result_label IS NULL)
  OR (NEW.result_kind IS NOT NULL AND NEW.status NOT IN ('succeeded', 'failed'))
BEGIN SELECT RAISE(ABORT, 'a tool call''s result kind and label are set together, when it finishes'); END;

CREATE TRIGGER tool_calls_engine_result_needs_its_write BEFORE UPDATE OF result_kind, result_label ON tool_calls
WHEN NEW.result_kind = 'engine' AND (
    NEW.tool <> 'table_write'
    OR NEW.status <> 'succeeded'
    OR NEW.result_label IS NOT 'tool:table_write'
    OR NOT EXISTS (
        SELECT 1 FROM row_history
        WHERE step_run_id = NEW.step_run_id AND call_seq = NEW.call_seq AND tool_call_id = NEW.id
    )
)
BEGIN SELECT RAISE(ABORT, 'an engine result answers only a table_write call whose row was written'); END;
