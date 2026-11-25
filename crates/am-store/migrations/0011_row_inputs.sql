-- 0011: a step input may name the after-image of a row it read (epic 3
-- story 22, "Rows reaching later steps"; AD-7, AD-10, AD-43).
--
-- Additive and forward-only: one new column, NULL on every old input, and
-- five triggers that keep the writer's rules for a `row` input and keep
-- any input from being replaced. Nothing is rewritten, moved or dropped;
-- AD-7's pre-migration copy is taken first.
-- No `document_versions` (3.6b's 0012). No delete guard: deleting a run
-- deletes its content (AD-25).

-- A `row` input names the after-image it read (`row_history.seq`); the
-- row's text stays on that image (AD-43: never stored twice).
ALTER TABLE step_inputs ADD COLUMN from_row INTEGER REFERENCES row_history(seq);

-- Only a `row` input names an after-image, and every `row` input names
-- one; it comes from no step run and no Inbox item; and its taint is never
-- below the after-image's (AD-10: the row's mark is a floor).
CREATE TRIGGER step_inputs_row_edge BEFORE INSERT ON step_inputs
WHEN (NEW.kind IS 'row') <> (NEW.from_row IS NOT NULL)
  OR (NEW.from_row IS NOT NULL AND (
        NEW.from_step_run IS NOT NULL
        OR NEW.from_item IS NOT NULL
        OR NEW.taint < COALESCE((SELECT taint FROM row_history WHERE seq = NEW.from_row), 2)
     ))
BEGIN SELECT RAISE(ABORT, 'a row input names one after-image, comes from no step run or Inbox item, and carries at least its taint'); END;

-- Its label is its after-image's own, `row:<table>/<row id>@<version>`, so
-- a prompt part that names it names that image.
CREATE TRIGGER step_inputs_row_label BEFORE INSERT ON step_inputs
WHEN NEW.from_row IS NOT NULL AND NEW.label IS NOT (
    SELECT 'row:' || table_id || '/' || row_id || '@' || version
    FROM row_history WHERE seq = NEW.from_row
)
BEGIN SELECT RAISE(ABORT, 'a row input''s label names its own after-image'); END;

-- Reading any row taints the reader (AD-10): the attempt's taint rises
-- with the edge, whatever the row's mark.
CREATE TRIGGER step_inputs_row_taints_reader AFTER INSERT ON step_inputs
WHEN NEW.kind IS 'row'
BEGIN
    UPDATE step_runs SET taint = 1 WHERE id = NEW.step_run_id AND taint IS NOT 1;
END;

-- A row input never changes once journaled, and no other input becomes
-- one or names an after-image.
CREATE TRIGGER step_inputs_row_fixed BEFORE UPDATE ON step_inputs
WHEN OLD.kind IS 'row' OR NEW.kind IS 'row'
  OR OLD.from_row IS NOT NULL OR NEW.from_row IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'a row input never changes, and no other input becomes one'); END;

-- Nor is any input replaced (the 3.5 lesson; review triage round 2, T4):
-- an `INSERT OR REPLACE` by an existing id would delete it without an
-- UPDATE, with or without `recursive_triggers`, and could swap a tainted
-- row edge for a clean input. An insert that meets an existing id aborts.
CREATE TRIGGER step_inputs_never_replaced BEFORE INSERT ON step_inputs
WHEN EXISTS (SELECT 1 FROM step_inputs WHERE id = NEW.id)
BEGIN SELECT RAISE(ABORT, 'a step input is never replaced'); END;
