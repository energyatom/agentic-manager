-- 0007: step attempts, step inputs, tool calls and Inbox items (epic 3
-- story 3; AD-8, AD-10, AD-28, AD-43).
--
-- Every step attempt is a step-run row: `UNIQUE (run_id, step_key)` from
-- 0001 becomes `UNIQUE (run_id, step_key, attempt)`, which SQLite can only
-- do by rebuilding the table (the migrator runs each step with foreign
-- keys off and checks them before it commits; AD-7's pre-migration copy
-- is taken first). Every existing row is kept as attempt 1, untainted.
-- `taint` marks the attempt and its stored output at rest (AD-10 (c)); it
-- only ever rises. A loop-back target's new attempt finds the reasons it
-- receives in its own `loop_back` input (below), written by the loop-back.
CREATE TABLE step_runs_0007 (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    step_key TEXT NOT NULL,
    seq INTEGER NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 1 CHECK (attempt >= 1),
    status TEXT NOT NULL,
    output TEXT,
    taint INTEGER NOT NULL DEFAULT 0 CHECK (taint IN (0, 1)),
    updated_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    UNIQUE (run_id, step_key, attempt)
);
INSERT INTO step_runs_0007 (id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at)
    SELECT id, run_id, step_key, seq, 1, status, output, 0, updated_at, started_at, finished_at FROM step_runs;
DROP TABLE step_runs;
ALTER TABLE step_runs_0007 RENAME TO step_runs;
CREATE TRIGGER step_runs_taint_only_rises BEFORE UPDATE OF taint ON step_runs
WHEN NEW.taint < OLD.taint
BEGIN SELECT RAISE(ABORT, 'step_runs.taint only rises'); END;

-- The inputs of a step attempt (AD-43's `step_inputs`): kind, the
-- producing attempt (none for a plan input), the label and the taint,
-- journaled in the transaction that starts the attempt; a loop-back
-- target's `loop_back` input is written by the loop-back's own transaction
-- (the edge is stored once). Text is never copied here: it stays in the
-- producer's output (AD-43: never stored twice).
CREATE TABLE step_inputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    step_run_id TEXT NOT NULL REFERENCES step_runs(id),
    kind TEXT NOT NULL,
    from_step_run TEXT REFERENCES step_runs(id),
    label TEXT NOT NULL,
    taint INTEGER NOT NULL CHECK (taint IN (0, 1)),
    created_at TEXT NOT NULL
);
CREATE INDEX step_inputs_by_step ON step_inputs (step_run_id);

-- Tool calls are journal rows (AD-8): one per call a step's model asked
-- for, `issued` before the tool runs (then `succeeded` or `failed`), or
-- `refused` (ToolGate or a cap).
CREATE TABLE tool_calls (
    id TEXT PRIMARY KEY,
    step_run_id TEXT NOT NULL REFERENCES step_runs(id),
    call_seq INTEGER NOT NULL,
    tool TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('issued', 'succeeded', 'failed', 'refused')),
    created_at TEXT NOT NULL,
    finished_at TEXT,
    UNIQUE (step_run_id, call_seq)
);

-- The one Inbox item entity (AD-28): AD-28's seven kinds; created only by
-- the engine, one per (step run, kind); `open` in this story (story 3.4
-- resolves). `payload` is the step run whose output the item carries;
-- `taint` copies its taint and only ever rises. The title and body are the
-- engine's words, never model text.
CREATE TABLE inbox_items (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('review', 'approval', 'draft', 'question', 'budget_ask', 'failure', 'proposal')),
    status TEXT NOT NULL CHECK (status IN ('open')),
    run_id TEXT NOT NULL REFERENCES runs(id),
    step_run_id TEXT NOT NULL REFERENCES step_runs(id),
    payload TEXT REFERENCES step_runs(id),
    taint INTEGER NOT NULL DEFAULT 0 CHECK (taint IN (0, 1)),
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    resolved_by TEXT,
    UNIQUE (step_run_id, kind)
);
CREATE TRIGGER inbox_items_taint_only_rises BEFORE UPDATE OF taint ON inbox_items
WHEN NEW.taint < OLD.taint
BEGIN SELECT RAISE(ABORT, 'inbox_items.taint only rises'); END;
