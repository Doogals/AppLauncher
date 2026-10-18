# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Dev (hot-reload Rust + Vite frontend together)
npm run tauri dev

# Release build (produces MSI + NSIS in src-tauri/target/release/bundle/)
npm run tauri build

# Type-check the tests. NOTE: `cargo test` COMPILES but CANNOT RUN on Windows.
# tauri-plugin-dialog imports TaskDialogIndirect (comctl32 v6), and the unittest
# harness gets no manifest declaring that assembly, so it aborts at load with
# STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139). No cargo directive can fix it:
# rustc-link-arg-tests covers only [[test]] targets (none exist here), and the
# generic rustc-link-arg would also hit the app binary, whose manifest already
# comes from a resource script. Use check instead:
cargo check --tests --manifest-path src-tauri/Cargo.toml

# Deploy (build → sign → GitHub release → update latest.json → push both repos)
powershell -ExecutionPolicy Bypass -File scripts\release.ps1

# The Cloudflare Worker deploys SEPARATELY — release.ps1 does not touch it:
#   cd cloudflare-worker && npx wrangler deploy
```

### Before every release — two manual steps the script does NOT do

**1. Bump the version in FOUR files.** `release.ps1`'s `-Version` param only
labels the git tag, the MSI filename and `latest.json` — it does not touch the
build. Miss one and you ship a binary reporting the previous version:
`src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, root `package.json`, and
the `$Version` default in `scripts/release.ps1`.

**2. Add the version's entry to `src/whats-new.json`.** That file is the single
source of release notes — the in-app What's New window and the GitHub release
body both read it. `release.ps1` **throws** if the version has no entry. This
replaced a hardcoded here-string that silently went stale: 0.6.3 and 0.6.4 both
shipped to GitHub carrying 0.6.2's notes because nothing read it back.

**Signing credentials are already stored** DPAPI-encrypted at
`%USERPROFILE%\.takeoff\signing.cred`, so a release is one unattended command.
`-VerifySigningSetup` checks the setup without building. Authenticode signing
runs BEFORE `npx tauri signer sign` and that order must not change —
Authenticode rewrites the MSI bytes and the updater signature hashes those
bytes, so reversing it publishes an update every install rejects as corrupt.

**Git remotes must not embed a token.** Both repos' `origin` once carried a
GitHub PAT in the URL; `release.ps1` runs non-interactively, so git could not
prompt and the `latest.json` push failed AFTER the GitHub release was already
public — the updater feed lagged until it was pushed by hand. Remotes are now
plain `https://github.com/...` with `gh auth setup-git` as the credential helper.

**Signing note:** `TAURI_SIGNING_PRIVATE_KEY_PATH` env vars are NOT picked up by `npm run tauri build`. Sign post-build with `npx tauri signer sign`. MSI filename must have no spaces — copy to `app.msi` first. Key at `C:\Users\dougb\.tauri\applauncher.key` (no password).

## Traps that will waste your time — read before debugging

**A dev build can silently never run.** The single-instance mutex is
machine-wide (`Global\`). The installed TakeOff starts at login and holds it, so
`npm run tauri dev` would start, bring the INSTALLED app's widget forward, and
exit — every frontend change appearing to do nothing, with no error at all. Debug
builds now use `Global\TakeOffSingleInstance-dev` to avoid this. **Both still
share `%LOCALAPPDATA%\TakeOff\config.json` and neither re-reads it after startup,
so quit the installed app before dev-testing anything that saves**, or they
overwrite each other.

**Tauri's file-drop handler swallows ALL DOM drag events.** `draggable="true"`
plus dragstart/drop handlers did nothing for months. Any window needing HTML5
drag-and-drop must be built with `.disable_drag_drop_handler()`. If DOM events go
missing in a new window, check this first.

**`save_group` rebuilds the group from the frontend payload.** The editor only
sends `name`, `icon` and `items`, so every other field must be explicitly
preserved: `color`, `detached`, `detached_x/y`, `hidden`, `launch_at_startup`.
Omitting one silently clears it on every save — this caused the group-colour
revert bug, and nearly did the same to startup flags.

**Never iterate a HashSet to pick a window.** Rust seeds its hasher randomly per
process, so iteration order differs every run. This was the single biggest cause
of "window placement works sometimes" — window enumeration returns a Z-ordered
`Vec` and must stay that way.

**`debug_log::write_debug_log` is a no-op in release builds.** Field reports
therefore carry no diagnostics. Worth remembering before asking a customer for a
log that cannot exist.

## Architecture

This is a **Tauri v2** app. Rust handles all system logic; the frontend (Vite/vanilla JS) is purely UI. They communicate exclusively via Tauri's `invoke()` / `emit()` IPC.

### Windows

Three persistent HTML entry points, all under `src/`:
- **`widget.html`** — the always-visible launcher bar. Renders groups as buttons, handles drag, right-click menu, update banner.
- **`config.html`** — group editor. Opens as a separate window via `open_config_window` Rust command. Auto-sizes to content.
- **`layout-item.html`** — one window spawned per item during the layout editor session. User drags it to set launch position; virtual desktop dropdown sets target desktop.

Vite root is `src/`, outDir is `dist/` — this is intentional and must not change.

### Rust modules (`src-tauri/src/`)

- **`lib.rs`** — all Tauri commands, menu handlers, tray, app setup, updater. The `AppState(Mutex<AppConfig>)` is the single source of truth for config, managed by Tauri's state system. `LayoutDesktops(Mutex<HashMap<String, Vec<u8>>>)` is transient state for the layout editor session (maps window label → virtual desktop GUID).
- **`config.rs`** — `AppConfig`, `Group`, `Item` structs (serde serialize/deserialize), `load_config()` / `save_config()`. Config persists at `%LOCALAPPDATA%\TakeOff\config.json` (folder is `TakeOff`, NOT `AppLauncher` — renamed with the rebrand; a stale `AppLauncher` folder may still exist on older machines and is ignored).
- **`launcher.rs`** — all launch logic. `launch_group` orchestrates virtual-desktop switching then `launch_item` per item. Window positioning uses a snapshot-before/poll-after approach (`collect_visible_hwnds` → spawn → `poll_for_new_window`) because PID matching fails for Store/UWP apps.
- **`virtual_desktop.rs`** — Windows virtual desktop support. `get_virtual_desktops()` reads ordered GUID list from registry. `switch_virtual_desktop(from, to)` simulates Win+Ctrl+Arrow via `SendInput`, polls registry to confirm completion. `get_current_virtual_desktop_guid()` reads `CurrentVirtualDesktop` from registry.
- **`license.rs`** — `group_limit()` enforces free tier (1 group). `is_licensed()` checks both key and instance ID are present. `startup_groups_allowed()` gates the paid startup-groups feature; paid features get a named function here so the list stays greppable.
- **`icons.rs`** — `get_file_icon`. A Start Menu `.lnk` names its icon as a **file + INDEX** into a shared resource DLL, which `SHGetFileInfoW` cannot reach (it only ever returns resource 0, and always returns *something* — the generic glyph — so failure is undetectable). `apps.rs` therefore passes `"file|index"` and `icons.rs` uses `ExtractIconExW`, falling back to `SHGetFileInfoW`.
- **`steam.rs`** — installed games from `steamapps/appmanifest_*.acf`. Icons moved between client versions: try flat `librarycache/<appid>_icon.jpg`, then the per-app folder `librarycache/<appid>/`, ranking by `rank_icon_file()`.
- **`capture.rs`** — "Capture Current": turns open windows into group items with their position, size and virtual desktop.
- **`machine_id.rs`** — stable per-machine fingerprint for license activation labels.
- **`apps.rs`**, **`browsers.rs`** — enumerate installed Win32 apps and browsers.

### Key design patterns

**Context menus must run on the main thread.** Always use `app.run_on_main_thread(...)` and defer event handlers to `tauri::async_runtime::spawn`. Never call menu APIs directly from a command handler.

**Config mutations follow a read-modify-write pattern:**
```rust
let mut config = state.0.lock().unwrap().clone(); // clone out
// mutate config
save_config(&config)?;
*state.0.lock().unwrap() = config; // write back
```

**Virtual desktop switching — critical gotchas:**
- `IVirtualDesktopManager::MoveWindowToDesktop` silently fails cross-process — do not use.
- `SendInput` must send each key event **individually** with 15ms delays between them. Batching all 6 events in one `SendInput` call causes Windows to open the Start menu instead of switching desktops.
- Read the current desktop GUID **once** at the start of `launch_group` and track it manually. Re-reading the registry mid-sequence returns stale data after a switch.
- After each keypress, poll the registry (every 50ms, up to 600ms) to confirm the switch completed before launching.
- **Windows 10 and 11 store `CurrentVirtualDesktop` in DIFFERENT registry locations.** Win11: directly under `Explorer\VirtualDesktops`. Win10: per Terminal Services session under `Explorer\SessionInfo\<n>\VirtualDesktops`. `get_current_vd_windows` tries Win11 first, then ENUMERATES SessionInfo — the session id is not always 1 (RDP, fast user switching, recycled sessions). Reading only the Win11 path returned None on every Win10 machine and silently disabled desktop targeting.

**STILL OPEN — Brave lands on the wrong virtual desktop.** Confirmed from the
debug log, not guessed: Brave restores its window onto whatever desktop it
personally remembers, before any of our code touches it. Chrome and Edge are
fine. Four approaches were tried and failed, and `ensure_correct_desktop()` was
removed entirely because its keyboard fallback acted on the foreground window
rather than the target HWND, dragging *later* items to the wrong desktop. Do not
re-add a foreground-based fix.

**Window positioning** uses physical pixels (from `GetWindowRect`) for storage, but `WebviewWindow` creation uses logical pixels (divide by DPI). The layout editor converts on save.

### Capabilities

All windows covered by `src-tauri/capabilities/default.json`. Dynamically-created layout editor windows are covered by the `"layout-item-*"` wildcard. Adding new window-level APIs requires adding the permission here (e.g., `core:window:allow-destroy` was a past gotcha).

### Licensing

Free tier: 1 group (enforced in `license.rs::group_limit()`). Paid: unlimited
groups **and** launching groups at Windows startup. Via LemonSqueezy → Cloudflare
Worker proxy (`WORKER_URL` in `lib.rs`). The Worker handles `/activate`,
`/deactivate`, `/validate`, `/feedback`.

Over the free limit, extra groups are **hidden, never deleted**, and restored on
re-activation. Paid features must be enforced in Rust at the point of use, not
only in the UI — `config.json` is plain text in `%LOCALAPPDATA%`.

Non-obvious: the License API (`/v1/licenses/*`) is **unauthenticated** — it needs
only the key. Only `/v1/license-key-instances` (used by self-healing
reactivation) needs a real account key, and **that key expires 11 Feb 2027**,
after which self-healing goes quiet with no error.

Activation labels are `COMPUTERNAME [8 hex]`. The fingerprint uses a hand-rolled
FNV-1a and **must stay that way** — `DefaultHasher` is randomly seeded per
process, so it would produce a different fingerprint every launch and orphan
every existing activation.

### Startup, updates and What's New

**Startup groups (paid).** `Group.launch_at_startup`. Fires only when *Windows*
started TakeOff: `register_autostart` passes `--autostart` in its command and
`launched_by_autostart()` checks for it.

**Autostart is a user-scoped Task Scheduler task (`TakeOffAutostart`), not the
Run key.** Measured: Run key 56s after the desktop, the task ~0s. The trigger
must carry `<UserId>` (registered via `schtasks /XML`); plain `/SC ONLOGON` is an
any-user trigger and fails "Access is denied" without admin. The Run key is only
a fallback if task creation fails, and exactly one of the two is kept.
Uninstall removes both, in two places that must stay in sync with the names in
`lib.rs`: `NSIS_HOOK_POSTUNINSTALL` in `preinstall.nsh` and the WiX fragment
`src-tauri/wix/uninstall-cleanup.wxs` (skipped during MSI upgrades). Without that, quitting and
reopening would relaunch every flagged group. Hold Shift at startup to skip.

**Groups launch strictly one at a time, and that is a correctness requirement.**
`launch_group_inner` clears the claimed-HWND set at its top and hand-tracks the
current virtual desktop, so two concurrent launches would wipe each other's
claims and fire desktop switches into each other's tracking. `run_startup_groups`
is a plain `for` loop on one thread — do not parallelise it.

**What's New** is driven by `AppConfig.last_seen_version`, shown on the
`app.restart()` the updater performs. `None` means either a fresh install or an
upgrade from before the field existed; `groups.is_empty()` is what distinguishes
them, so long-time users don't get a "Welcome to TakeOff" screen. The version is
recorded **before** the window opens, so a failed save cannot cause it to
reappear on every launch.

**Startup speed: FIXED in 0.6.6**, by moving autostart from the Run key to the
Task Scheduler task described above. Measured on the same machine: Run key 56s
after the desktop appeared, scheduled task ~0s. The delay was never in our code —
the process simply did not exist for 56s. Investigation and method:
`docs/superpowers/plans/2026-09-30-startup-delay-investigation.md`.

### Usage analytics (`analytics.rs`) — 0.6.7

Anonymous weekly ping: active-install count and feature usage, emailed to Doug
by a Cloudflare cron. On by default, disclosed in Settings and in What's New,
with an opt-out that takes effect immediately.

**The rule: counts, never content.** `12 app items` is a count. A file path,
group name, URL or Steam title is personal data and must never be in the
payload. `build_payload` is pure and returns a fixed set of keys; the test
`payload_leaks_no_personal_data` builds a deliberately realistic config and
asserts none of it appears in the serialised JSON. The Worker independently
drops unknown fields, so a client bug alone cannot start sending something new.

**`analytics_id` is a random UUID, NOT `machine_fingerprint()`.** The
fingerprint is derived from the SMBIOS motherboard UUID so it survives a Windows
reinstall — right for license activation, wrong for analytics, and reusing it
would tie usage data to the same identifier as a purchase.

`analytics_last_sent` is written **before** the request (same reasoning as
`last_seen_version`), so a failed send loses that week rather than retrying
every launch. The numbers undercount people often offline at startup.

**The Worker is ES-module format (`export default { fetch, scheduled }`)** —
Cloudflare refuses a D1 binding on the old `addEventListener` format, so do not
revert it. Secrets arrive on `env` and `bindEnv()` copies them to module vars.

Worker: `POST /ping` → D1 `pings` table keyed `(install_id, week)` so repeat
pings are idempotent and weekly actives is a plain `COUNT`. The week is computed
server-side; a wrong clock on a user's PC must not write into another week's
bucket. Rows older than ~26 weeks are pruned on each cron run.

**The Settings toggle links to the privacy policy at tonic-tech.com/privacy.html
(from 0.6.8; 0.6.7 shipped with only the What's New mention).
Never ship a build whose toggle points at a policy that is not live.**

### Item struct

`Item` in `config.rs` has these fields that determine launch behavior:
- `item_type`: App | File | Url | Folder | Script | Steam | Uwp (`path` holds the AUMID, launched via `shell:AppsFolder`)
- `path` / `value` / `urls`: where to launch
- `launch_virtual_desktop`: `Option<Vec<u8>>` — 16-byte GUID, set via layout editor
- `launch_desktop`: `Option<u32>` — monitor index, Steam items only
- `launch_x/y/width/height`: saved window position in physical pixels
- `run_as_admin`: uses `ShellExecuteExW` with "runas" verb
- `run_in_terminal`: script items — run via cmd/powershell vs open in default app

## Capture Current (`capture.rs`, `src/capture.html`)

Scans open windows and offers them as items with their placement. Three things
that are easy to get wrong:

- **`GetWindowPlacement`, NOT `GetWindowRect`.** A minimized window's rect reads
  ~`-32000,-32000`; capturing that saves an item that launches permanently
  off-screen. `rcNormalPosition` is the restored rect.
- **Store apps need their AUMID, not their exe path.** `GetApplicationUserModelId`
  (kernel32) returns it from the process handle. UWP windows are hosted two ways:
  modern MSIX apps own their window, while older ones (Calculator, Photos) are
  owned by `ApplicationFrameHost.exe`, which is NOT packaged — the real app is a
  child `CoreWindow` in a different process, so `aumid_for_hwnd` walks
  `EnumChildWindows` for a foreign PID. Chosen over
  `SHGetPropertyStoreForWindow`/`PKEY_AppUserModel_ID` to avoid IPropertyStore
  and PROPVARIANT.
- **Browser tabs cannot be captured** — a window title is the page title, never
  the URL. The preview says so rather than producing a browser item that opens
  blank. Elevated apps also can't be resolved (`OpenProcess` fails across
  integrity levels) and show an explanatory blocked row.

Deferred: Explorer folder paths via `IShellWindows`; best-effort browser URL
capture via UI Automation.

## Plan docs

`docs/superpowers/plans/` holds design docs for larger features, written before
implementation and kept afterwards. Worth reading before changing startup
groups, Capture Current, or startup timing.
