-- 0012: document versions, and the document an attempt versions (epic 3
-- story 21, "Document versions and Draft items"; AD-7, AD-10, AD-29,
-- AD-35, AD-43).
--
-- Additive and forward-only: one new table, one new column (NULL on every
-- old attempt) and their triggers. Nothing is rewritten, moved or dropped;
-- AD-7's pre-migration copy is taken first.

-- The document an attempt versions, journaled when it starts (AD-21,
-- AD-29): set only by the update that takes it from `queued` to
-- `running`, never over one already set, so never changed after; NULL on
-- every attempt from before 0012 and on a new attempt.
ALTER TABLE step_runs ADD COLUMN writes_document TEXT;

-- One row per version of a document (AD-29 as amended 2026-11-24, owner
-- Q7: one version per succeeded attempt, numbered per document in commit
-- order; a run's version is its latest). The Markdown is not copied: it
-- stays once, on the producing attempt's `step_runs.output` (AD-43: never
-- stored twice). `taint` is the attempt's at its finish, marked at rest
-- (AD-10 (c)); `bytes` is the Markdown's size, for lists that read no text.
CREATE TABLE document_versions (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 1),
    run_id TEXT NOT NULL REFERENCES runs(id),
    attempt INTEGER NOT NULL CHECK (attempt >= 1),
    step_run_id TEXT NOT NULL UNIQUE REFERENCES step_runs(id),
    taint INTEGER NOT NULL CHECK (taint IN (0, 1)),
    bytes INTEGER NOT NULL CHECK (bytes >= 1),
    created_at TEXT NOT NULL,
    UNIQUE (document_id, version)
);

-- Append-only: a version never changes or goes.
CREATE TRIGGER document_versions_no_update BEFORE UPDATE ON document_versions
BEGIN SELECT RAISE(ABORT, 'document_versions is append-only'); END;
CREATE TRIGGER document_versions_no_delete BEFORE DELETE ON document_versions
BEGIN SELECT RAISE(ABORT, 'document_versions is append-only'); END;

-- Nor is one replaced (the 3.5 lesson): an `INSERT OR REPLACE` meeting an
-- existing id, rowid, (document, version) or step run would delete it
-- without firing a DELETE trigger, so an insert that meets one is refused.
CREATE TRIGGER document_versions_never_replaced BEFORE INSERT ON document_versions
WHEN EXISTS (
    SELECT 1 FROM document_versions
    WHERE id = NEW.id OR rowid = NEW.rowid
       OR (document_id = NEW.document_id AND version = NEW.version)
       OR step_run_id = NEW.step_run_id
)
BEGIN SELECT RAISE(ABORT, 'document_versions is append-only: a version is never replaced'); END;

-- A version is the next one of its document: 1 for a new document, the
-- newest version + 1 otherwise.
CREATE TRIGGER document_versions_next_version BEFORE INSERT ON document_versions
WHEN NEW.version <> COALESCE(
    (SELECT MAX(version) FROM document_versions WHERE document_id = NEW.document_id), 0
) + 1
BEGIN SELECT RAISE(ABORT, 'a version is the next version of its document'); END;

-- A version names its own attempt: one of its run, with its number, that
-- journaled this document when it started and succeeded; and it carries at
-- least that attempt's taint (AD-10: a mark never lowers).
CREATE TRIGGER document_versions_name_their_attempt BEFORE INSERT ON document_versions
WHEN NOT EXISTS (
    SELECT 1 FROM step_runs
    WHERE id = NEW.step_run_id AND run_id = NEW.run_id AND attempt = NEW.attempt
      AND writes_document = NEW.document_id AND status = 'succeeded'
      AND taint <= NEW.taint
)
BEGIN SELECT RAISE(ABORT, 'a version names its own succeeded attempt, which journaled its document, and carries at least its taint'); END;

CREATE TRIGGER step_runs_inserted_without_document BEFORE INSERT ON step_runs
WHEN NEW.writes_document IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'a new attempt is inserted with no document; it journals one when it starts'); END;

CREATE TRIGGER step_runs_document_set_at_start BEFORE UPDATE OF writes_document ON step_runs
WHEN NEW.writes_document IS NOT OLD.writes_document AND (
    NOT (OLD.status = 'queued' AND NEW.status = 'running')
    OR OLD.writes_document IS NOT NULL
)
BEGIN SELECT RAISE(ABORT, 'an attempt''s document is set only when it starts, and never changed'); END;

-- Nor does an attempt that journaled a document go back to `queued`
-- (0010's rule for its seat and table, kept for the document).
CREATE TRIGGER step_runs_requeued_without_document BEFORE UPDATE OF status ON step_runs
WHEN NEW.status = 'queued' AND OLD.status = 'waiting_branches' AND OLD.writes_document IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'only an attempt that never started goes back to queued'); END;

-- A version's text stays at rest (AD-29, AD-43): the Markdown is its
-- attempt's `output`, so once a version names an attempt, neither that
-- attempt's output nor its status ever changes (review round 2).
CREATE TRIGGER step_runs_versioned_output_fixed BEFORE UPDATE OF output, status ON step_runs
WHEN EXISTS (SELECT 1 FROM document_versions WHERE step_run_id = OLD.id)
BEGIN SELECT RAISE(ABORT, 'a versioned attempt''s output and status never change'); END;
