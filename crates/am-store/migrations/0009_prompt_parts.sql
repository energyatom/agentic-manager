-- 0009: the journal keeps what a model was sent (epic 3 story 5; AD-7,
-- AD-8, AD-25, AD-27, AD-35, AD-36, AD-43).
--
-- Additive and forward-only. Nothing is rewritten, moved or dropped:
-- every old attempt simply has no parts (the reader says "not recorded"),
-- and every old call is a `turn`.
--
-- `prompt_parts`: one row per part of an attempt's assembled prompt
-- (`call_seq` 0) or of a repair call's own parts (`call_seq` = that call's),
-- in request order (`ord` 1..n). `slot` is AD-27's slot (`reply` and
-- `repair` are a repair call's own); `kind` is the part's type (AD-10);
-- `taint` is marked at rest. `text` holds what has no other home: config
-- text, a plan input, a repair call's reply and instruction. An input from
-- a step output, a loop-back or a gate note has no text here, because it
-- stays on its producer's row or its item (AD-43: never stored twice).
-- `sha256` is for a native file part (epic 12), bytes are never stored.
CREATE TABLE prompt_parts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    step_run_id TEXT NOT NULL REFERENCES step_runs(id),
    call_seq INTEGER NOT NULL,
    ord INTEGER NOT NULL,
    slot TEXT NOT NULL CHECK (slot IN ('role', 'goal', 'backstory', 'skill', 'ceo_goal', 'guidance', 'memory', 'reference_files', 'saved_links', 'step', 'trusted', 'untrusted', 'reply', 'repair')),
    kind TEXT NOT NULL CHECK (kind IN ('instruction', 'trusted', 'untrusted', 'assistant')),
    label TEXT NOT NULL,
    taint INTEGER NOT NULL CHECK (taint IN (0, 1)),
    text TEXT,
    -- The stored text's length in bytes: 0 for a part whose text stays on its
    -- producer's row or Inbox item (drill-down joins that row for the size).
    bytes INTEGER NOT NULL,
    sha256 TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (step_run_id, call_seq, ord)
);

-- Append-only, like the audit log: the journal is a record of what was sent.
CREATE TRIGGER prompt_parts_no_update BEFORE UPDATE ON prompt_parts
BEGIN SELECT RAISE(ABORT, 'prompt_parts is append-only'); END;
CREATE TRIGGER prompt_parts_no_delete BEFORE DELETE ON prompt_parts
BEGIN SELECT RAISE(ABORT, 'prompt_parts is append-only'); END;

-- Nor is a part replaced: an `INSERT OR REPLACE` over an existing
-- (step run, call, ord) deletes without firing a DELETE trigger, so an insert
-- that meets an existing row is refused (as 0008's `inbox_items_never_replaced`).
CREATE TRIGGER prompt_parts_never_replaced BEFORE INSERT ON prompt_parts
WHEN EXISTS (
    SELECT 1 FROM prompt_parts
    WHERE id = NEW.id OR (step_run_id = NEW.step_run_id AND call_seq = NEW.call_seq AND ord = NEW.ord)
)
BEGIN SELECT RAISE(ABORT, 'prompt_parts is append-only: a part is never replaced'); END;

-- Whether a call is a step's own turn or its one schema repair (AD-36).
ALTER TABLE model_calls ADD COLUMN purpose TEXT NOT NULL DEFAULT 'turn' CHECK (purpose IN ('turn', 'repair'));
