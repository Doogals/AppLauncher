-- Weekly usage pings.
--
-- PRIMARY KEY (install_id, week) with INSERT OR REPLACE makes repeat pings in
-- the same week idempotent, so "active installs this week" is a plain COUNT —
-- no de-duplication logic to get wrong, and a client that pings twice (clock
-- change, reinstall of the same config) cannot inflate the number.
--
-- There is deliberately no timestamp column beyond the week. Knowing WHEN in
-- the week somebody used TakeOff is not needed for any question being asked,
-- and the less that is stored the less there is to get wrong.
CREATE TABLE IF NOT EXISTS pings (
  install_id TEXT NOT NULL,
  week       TEXT NOT NULL,   -- ISO year-week, e.g. "2026-W42"
  version    TEXT,
  licensed   INTEGER,         -- 0/1
  os         TEXT,
  stats      TEXT,            -- JSON: the count fields
  PRIMARY KEY (install_id, week)
);

CREATE INDEX IF NOT EXISTS idx_pings_week ON pings(week);
