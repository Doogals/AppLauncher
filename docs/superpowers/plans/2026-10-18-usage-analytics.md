# Weekly usage reporting

**Status: SHIPPED in 0.6.7 (2026-10-18).** Worker deployed, D1 live, policy live. Targeting 0.6.7.
**Goal:** Doug gets one email a week: how many people used TakeOff, and which
features they used.

## State of play — read this first

Written and in the tree:

- `src-tauri/src/analytics.rs` — payload builder (pure, tested), 7-day gate,
  background send
- `AppConfig`: `analytics_enabled` (default true), `analytics_id`,
  `analytics_last_sent`
- `lib.rs`: `mod analytics`, `maybe_send_weekly` called last in `setup()`,
  commands `get_analytics_enabled` / `set_analytics_enabled` /
  `get_analytics_preview`
- Settings → Privacy: toggle + "See exactly what's sent" modal
- `cloudflare-worker/`: `POST /ping`, `schema.sql`, cron + `sendWeeklyReport()`,
  `wrangler.toml` triggers and D1 binding
- `src/whats-new.json`: entries for 0.6.7 **and 0.6.6** (0.6.6 had none, which
  would have made `release.ps1` throw)
- Version bumped to 0.6.7 in all four places
- `privacy.html` in the **tonic-tech-site** repo (new file, footer link added on
  that page only — the other pages still need one)

**Not done:**

1. ~~`cargo check --tests`~~ — passes cleanly; dev run checked by Doug (2026-10-18).
2. ~~D1 database~~ — created and schema applied 2026-10-18. (Was: `database_id` in `wrangler.toml` was the
   placeholder, and the Worker accepted and dropped pings while it was, on
   purpose, so a half-finished deploy could not make the app look broken.)
3. ~~`privacy.html`~~ — reviewed and published 2026-10-18. Linked from every
   site footer, and from Settings → Privacy from 0.6.8 (0.6.7 shipped without it).

## Deployment order — this matters

1. `cargo check --tests` and a dev run.
2. `npx wrangler d1 create takeoff-analytics` → paste the id into
   `wrangler.toml` → `npx wrangler d1 execute takeoff-analytics --remote
   --file=schema.sql` → `npx wrangler deploy`.
3. **Review and publish `privacy.html`.** The Settings toggle and the What's New
   text both promise a policy exists. **Do not release the app before the
   policy is live** — that is the one ordering constraint that is not just
   tidiness.
4. `release.ps1`.

## Known limitation, by design

`analytics_last_sent` is written **before** the request, mirroring
`last_seen_version`. So a failed send (offline at startup, which is common)
loses that week's ping rather than retrying. That prevents pinging on every
launch after a failure, at the cost of undercounting users who are frequently
offline at boot. Undercounting is the safe direction for a metric, but the
number is not exact and should not be quoted as though it were.

## Decisions already made

| Question | Decision |
|---|---|
| What to collect | Active install count + feature usage |
| Consent | Disclosed, ON by default, opt-out toggle in Settings |
| Delivery | Weekly email (reuses the Resend setup already in the Worker) |

## What already exists and is NOT enough

- **LemonSqueezy** — paid activations only. Free users invisible.
- **Worker `/downloads`** — sums MSI `download_count` across GitHub releases.
  Counts *installs*, not *use*. Someone who installed once in July and deleted it
  counts forever.
- **The updater** hits `raw.githubusercontent.com`, which yields no analytics.
  Every running copy already phones home on a schedule and the signal is thrown
  away.

## 1. The data — an allow-list, not a judgement call

**Sent:**

| Field | Example | Why |
|---|---|---|
| `install_id` | random UUID v4 | counting unique installs |
| `version` | `"0.6.7"` | is auto-update working |
| `licensed` | `true` | free vs paid split |
| `os` | `"10.0.26200"` | how much Win10 still matters |
| `group_count` | `4` | feature usage |
| `item_count` | `23` | feature usage |
| `items_by_type` | `{app:12, url:6, steam:5}` | which item types matter |
| `startup_group_count` | `2` | is the paid feature used |
| `detached_count` | `1` | is detaching used |
| `captured_ever` | `true` | did Capture Current get adopted |

**Never sent — this is the line that must not move:**

group names, item paths, URLs, bookmark contents, Steam titles, computer name,
username, license key, IP-derived location, anything the user typed.

The rule: **counts, never content.** `C:\Users\dougb\...` is personal data.
"12 app items" is not. If a field needs a judgement call about whether it's
personal, it doesn't go in.

### install_id must be random — do NOT reuse `machine_fingerprint()`

`machine_id.rs` derives its fingerprint from the SMBIOS motherboard UUID. That
is correct for license activation (it must survive a Windows reinstall) and
exactly wrong here: it is pseudonymous, hardware-derived, and ties analytics to
the same identifier as a purchase.

Generate a fresh UUID v4 on first run, store it in config, and let it reset on
uninstall. It identifies an install, not a person or a machine.

## 2. Client changes

`AppConfig` gains three fields, all `#[serde(default)]`:

```rust
pub analytics_enabled: bool,       // default TRUE via #[serde(default = "default_true")]
pub analytics_id: Option<String>,  // random UUID, created on first ping
pub analytics_last_sent: Option<String>,  // YYYY-MM-DD
```

**`save_group` does not touch these** (they are not group fields) but the
`AppConfig` read-modify-write pattern applies — see CLAUDE.md.

New `analytics.rs`:
- `maybe_send_weekly(app)` — called from `setup()` on a background thread,
  AFTER the widget is shown. Never blocks startup.
- Sends only if: `analytics_enabled`, and `analytics_last_sent` is absent or
  more than 7 days ago.
- Failure is silent and does not retry in-session. Analytics must never be a
  reason the app misbehaves.
- Writes `analytics_last_sent` **before** the request, same reasoning as
  `last_seen_version`: a failed write must not cause a send every launch.

## 3. Settings UI

A toggle under a new "Privacy" section:

```
[x] Share anonymous usage data
    Helps me see which features people actually use. Never includes your
    group names, file paths, or web addresses.    [See exactly what's sent]
```

**"See exactly what's sent" opens a window showing the real payload** — the
actual JSON that would be sent right now, from this install. It costs almost
nothing to build and it is the difference between "trust me" and "look for
yourself". Reuse the What's New window pattern.

## 4. Worker

New `POST /ping`. Validate the body against the allow-list and **drop unknown
fields** — that way a future client bug cannot start sending something it
shouldn't without the Worker also being changed.

Storage: **Cloudflare D1** (SQL, trivial weekly aggregates). KV would work but
counting distinct installs means listing keys, which gets awkward.

```sql
CREATE TABLE pings (
  install_id TEXT NOT NULL,
  week       TEXT NOT NULL,   -- ISO year-week, e.g. 2026-W42
  version    TEXT,
  licensed   INTEGER,
  os         TEXT,
  stats      TEXT,            -- JSON blob of the counts
  PRIMARY KEY (install_id, week)
);
```

`PRIMARY KEY (install_id, week)` with `INSERT OR REPLACE` makes repeat pings in
the same week idempotent — so weekly actives is just `COUNT(*) WHERE week = ?`.

**Retention:** delete rows older than ~26 weeks on each cron run. Keeping
per-install rows forever is neither needed nor defensible.

## 5. Weekly email

Cloudflare cron trigger — `wrangler.toml` needs:

```toml
[triggers]
crons = ["0 13 * * 1"]   # Mondays 13:00 UTC
```

The Worker currently uses the old `addEventListener('fetch')` service-worker
syntax; cron needs an `addEventListener('scheduled')` alongside it.

Email body (Resend, same path as feedback):

```
TakeOff — week of 12 Oct 2026

Active installs:   142   (+11 vs last week)
  Licensed:         38
  Free:            104

Versions:  0.6.6  121    0.6.5  18    older  3

Median groups per user:      3
Median items per group:      5
Using startup groups:       22  (58% of licensed)
Have used Capture Current:  61  (43%)
Detached a group:           14  (10%)
```

The week-on-week delta is the number that will actually tell you something.

## 6. Privacy obligations — not optional

1. **Privacy policy on tonic-tech.com** stating what is collected, that it is
   anonymous, how long it is kept, and how to turn it off.
2. **Disclose it in the What's New window** for the release that introduces it.
   Existing users get this ON by default, so telling them plainly in the window
   they already see on update is the honest move — and we built that window, so
   it costs nothing.
3. **The toggle must work immediately** — no "takes effect next launch".

Selling through LemonSqueezy means EU customers by default, so treat GDPR as
in scope. Anonymous aggregate counts with disclosure and an opt-out is a
defensible position; silent collection is not.

## 7. Rollout

**Phase 1** — client fields, `analytics.rs`, Settings toggle, Worker `/ping`,
D1 table. No email yet; just confirm data arrives and the toggle stops it.

**Phase 2** — cron + weekly email, once a week of real data exists to format
against.

**Phase 3 (optional)** — a Worker-served page for digging into the numbers.

Ship Phase 1 and 2 in the same release if the privacy policy is ready; the
toggle must not ship before the policy it points at.

## 8. Testing

`cargo test` cannot run here — `cargo check --tests` (see CLAUDE.md).

Unit-testable:
- the 7-day gate, including a missing/garbage `analytics_last_sent`
- payload construction: assert it contains ONLY allow-listed keys, and
  explicitly assert no group name, path or URL appears anywhere in the JSON
- `install_id` is generated once and reused

Manual:
1. Fresh config → first launch sends one ping, `analytics_id` and
   `analytics_last_sent` written
2. Relaunch same day → no second ping
3. Toggle off → no ping even when due
4. Worker unreachable → app behaves normally, no error surfaced
5. "See exactly what's sent" matches what the Worker actually received
6. Two installs → D1 shows two rows for the week; same install twice → one row

## 9. Deliberately out of scope

- Anything identifying an individual user
- Crash reporting (worth doing, different system, different consent)
- Counting which *specific* apps people launch — that is content, not a count,
  and it is exactly the line in §1
