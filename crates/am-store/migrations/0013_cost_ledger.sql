-- 0013: the cost ledger (epic 3 story 7, "Cost settles per call and rolls
-- up"; AD-7, AD-8, AD-9; owner Q4, Q5, Q11, Q13, Q15).
--
-- Additive and forward-only: new columns, NULL on every old row (no
-- backfill, owner Q13 (a)), and the ledger's integrity triggers. Nothing is
-- rewritten, moved or dropped; AD-7's pre-migration copy is taken first.

-- What a reservation journals beside its USD amount, so the row reads
-- alone and the writer settles from its own terms (owner Q4): the company
-- currency and amounts, the price terms (micro-USD per million tokens),
-- the rate (company micro-units per USD), the price file's `updated:`
-- date, the counted prompt tokens and `max_output_tokens`, whether the call
-- ran on this Mac (`local`, 0 or 1), the company-local budget week it was
-- reserved in (the week's start date, owner Q11) and the seat's team when
-- it was reserved (owner Q15).
ALTER TABLE ledger ADD COLUMN currency TEXT;
ALTER TABLE ledger ADD COLUMN reserved_micro_company INTEGER;
ALTER TABLE ledger ADD COLUMN settled_micro_company INTEGER;
ALTER TABLE ledger ADD COLUMN input_micro_usd_per_mtok INTEGER;
ALTER TABLE ledger ADD COLUMN output_micro_usd_per_mtok INTEGER;
ALTER TABLE ledger ADD COLUMN company_micro_per_usd INTEGER;
ALTER TABLE ledger ADD COLUMN prices_updated TEXT;
ALTER TABLE ledger ADD COLUMN counted_input_tokens INTEGER;
ALTER TABLE ledger ADD COLUMN max_output_tokens INTEGER;
ALTER TABLE ledger ADD COLUMN local INTEGER;
ALTER TABLE ledger ADD COLUMN week TEXT;
ALTER TABLE ledger ADD COLUMN team TEXT;

-- When a call settled (or was marked lost): its time is `settled_at` minus
-- `created_at` (Conventions, Money: tokens and time for every call).
ALTER TABLE model_calls ADD COLUMN settled_at TEXT;

-- Money rows are as append-only as the journals (owner Q13): never
-- deleted,
CREATE TRIGGER ledger_no_delete BEFORE DELETE ON ledger
BEGIN SELECT RAISE(ABORT, 'ledger rows are never deleted'); END;

-- never replaced (an `INSERT OR REPLACE` meeting an existing row would
-- delete it without firing the DELETE trigger),
CREATE TRIGGER ledger_never_replaced BEFORE INSERT ON ledger
WHEN EXISTS (
    SELECT 1 FROM ledger
    WHERE id = NEW.id OR rowid = NEW.rowid OR model_call_id = NEW.model_call_id
)
BEGIN SELECT RAISE(ABORT, 'a ledger row is never replaced'); END;

-- inserted only as a reservation (review M2: `reserved`, with no settled
-- amount; it settles later, once), never below zero, nothing for a call on
-- this Mac, and on valid terms (review M3: prices from 0 up, a rate above
-- 0 when there is one),
CREATE TRIGGER ledger_amounts_on_insert BEFORE INSERT ON ledger
WHEN NEW.state IS NOT 'reserved'
  OR NEW.settled_micro_usd IS NOT NULL
  OR NEW.settled_micro_company IS NOT NULL
  OR NEW.reserved_micro_usd < 0
  OR NEW.reserved_micro_company < 0
  OR NEW.input_micro_usd_per_mtok < 0
  OR NEW.output_micro_usd_per_mtok < 0
  OR NEW.company_micro_per_usd <= 0
  OR (NEW.local IS NOT NULL AND NEW.local NOT IN (0, 1))
  OR (NEW.local = 1 AND (
        NEW.reserved_micro_usd <> 0
     OR COALESCE(NEW.reserved_micro_company, 0) <> 0))
BEGIN SELECT RAISE(ABORT, 'a ledger row is inserted as a reservation, on terms from 0 up and a rate above 0, never below zero, and a local call costs nothing'); END;

-- what was reserved never changes, nor the row's rowid (review M2: an
-- `UPDATE OR REPLACE` onto another row's rowid would delete that row
-- without firing the DELETE trigger),
CREATE TRIGGER ledger_reservation_fixed BEFORE UPDATE ON ledger
WHEN NEW.rowid IS NOT OLD.rowid
  OR NEW.id IS NOT OLD.id
  OR NEW.model_call_id IS NOT OLD.model_call_id
  OR NEW.run_id IS NOT OLD.run_id
  OR NEW.kind IS NOT OLD.kind
  OR NEW.reserved_micro_usd IS NOT OLD.reserved_micro_usd
  OR NEW.rate_snapshot IS NOT OLD.rate_snapshot
  OR NEW.created_at IS NOT OLD.created_at
  OR NEW.currency IS NOT OLD.currency
  OR NEW.reserved_micro_company IS NOT OLD.reserved_micro_company
  OR NEW.input_micro_usd_per_mtok IS NOT OLD.input_micro_usd_per_mtok
  OR NEW.output_micro_usd_per_mtok IS NOT OLD.output_micro_usd_per_mtok
  OR NEW.company_micro_per_usd IS NOT OLD.company_micro_per_usd
  OR NEW.prices_updated IS NOT OLD.prices_updated
  OR NEW.counted_input_tokens IS NOT OLD.counted_input_tokens
  OR NEW.max_output_tokens IS NOT OLD.max_output_tokens
  OR NEW.local IS NOT OLD.local
  OR NEW.week IS NOT OLD.week
  OR NEW.team IS NOT OLD.team
BEGIN SELECT RAISE(ABORT, 'what a ledger row reserved never changes'); END;

-- a settled row never changes (it is never settled again or un-settled),
CREATE TRIGGER ledger_settles_once BEFORE UPDATE ON ledger
WHEN OLD.state <> 'reserved'
BEGIN SELECT RAISE(ABORT, 'a settled ledger row never changes'); END;

-- and an open row settles whole: `state` goes to `settled` with its USD
-- amount and, when its reservation has one, its company amount (an old row
-- from before 0013 settles in USD only), never below zero, and at 0 for a
-- call on this Mac.
CREATE TRIGGER ledger_settles_whole BEFORE UPDATE ON ledger
WHEN OLD.state = 'reserved' AND NOT (
    (NEW.state = 'reserved'
        AND NEW.settled_micro_usd IS NULL
        AND NEW.settled_micro_company IS NULL)
    OR (NEW.state = 'settled'
        AND NEW.settled_micro_usd IS NOT NULL
        AND NEW.settled_micro_usd >= 0
        AND ((OLD.reserved_micro_company IS NULL AND NEW.settled_micro_company IS NULL)
          OR (OLD.reserved_micro_company IS NOT NULL
              AND NEW.settled_micro_company IS NOT NULL
              AND NEW.settled_micro_company >= 0))
        AND (COALESCE(OLD.local, 0) = 0
          OR (NEW.settled_micro_usd = 0 AND COALESCE(NEW.settled_micro_company, 0) = 0)))
)
BEGIN SELECT RAISE(ABORT, 'a ledger row settles once, with its state and both amounts, never below zero'); END;

-- A call's tokens and its settle time never change once set (review M10),
-- and nothing of them once the call settled. No DELETE trigger: deleting a
-- run deletes its calls (AD-25, as DR-162).
CREATE TRIGGER model_calls_settlement_fixed BEFORE UPDATE ON model_calls
WHEN (OLD.input_tokens IS NOT NULL AND NEW.input_tokens IS NOT OLD.input_tokens)
  OR (OLD.output_tokens IS NOT NULL AND NEW.output_tokens IS NOT OLD.output_tokens)
  OR (OLD.settled_at IS NOT NULL AND (
        NEW.settled_at IS NOT OLD.settled_at
     OR NEW.input_tokens IS NOT OLD.input_tokens
     OR NEW.output_tokens IS NOT OLD.output_tokens))
BEGIN SELECT RAISE(ABORT, 'a model call''s tokens and settle time never change once set'); END;
