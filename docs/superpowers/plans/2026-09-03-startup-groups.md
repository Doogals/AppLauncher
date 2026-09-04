# Launch groups at startup (paid feature)

**Status:** preliminary plan, awaiting approval
**Target:** 0.6.4

## Summary

Let a licensed user mark individual groups to launch automatically when Windows
starts TakeOff. Free users see the control but cannot enable it.

## Decisions already made

| Question | Decision |
|---|---|
| When does it fire? | **Only when Windows launched TakeOff**, never on a manual launch |
| Delay before launching | **None** |
| Escape hatch | **Hold Shift to skip** |
| Launch order | Widget order (`reorder_group` already lets users change it) |
| Startup speed investigation | Separate task, after this |

## 1. Data model

Add to `Group` in `config.rs`:

```rust
/// Launch this group automatically when Windows starts TakeOff.
/// Licensed users only; enforced in Rust at fire time, not just in the UI.
#[serde(default)]
pub launch_at_startup: bool,
```

`#[serde(default)]` means existing `config.json` files load unchanged.

**Gotcha:** `save_group` currently rebuilds the group from the frontend payload
and preserves `color` / `detached` / `detached_x` / `detached_y` / `hidden`
explicitly. `launch_at_startup` must be added to that preserve list, or saving a
group from the editor silently clears the flag — the exact bug that caused the
group-colour revert in 0.5.9.

## 2. Detecting a Windows-initiated launch

`register_autostart` writes an HKCU `Run` entry. Append a flag:

```
"C:\Program Files\TakeOff\TakeOff.exe" --autostart
```

At startup, `std::env::args().any(|a| a == "--autostart")` decides whether the
batch runs. Because `register_autostart` already re-runs on every launch, every
existing install migrates on its next boot with no user action.

Two consequences worth stating up front:

- A dev build (`npm run tauri dev`) never has the flag, so testing needs either a
  debug-only override or running the built exe with the flag by hand. Plan
  assumes a `--autostart` accepted in debug builds too, so it can be tested
  directly.
- If a user's Run entry is stale (registered by an older version), the feature
  quietly does nothing until the first launch re-registers it. Acceptable —
  it fails closed, not open.

## 3. The launch sequence

Runs on a background thread so the widget still appears immediately.

```
if !--autostart            -> stop
if Shift held              -> stop, log "skipped by user"
if !is_licensed            -> stop, log "not licensed"
for group in groups (widget order):
    skip if !launch_at_startup
    skip if hidden
    launch_group(group)     <-- BLOCKING, fully completes
```

**Groups must launch strictly sequentially. This is a correctness constraint,
not a preference.** `launch_group_inner` calls `clear_claimed_hwnds()` at its top
and tracks the current virtual desktop manually across the whole sequence,
because the registry lags behind keyboard-simulated switches. Two concurrent
launches would wipe each other's HWND claims and fire Win+Ctrl+Arrow presses
into each other's desktop tracking. Windows would land essentially at random.

`launch_group` is already synchronous, so this is a plain `for` loop on one
thread — but nothing may "helpfully" parallelise it later.

## 4. Hold Shift to skip

Checked once, immediately before the loop:

```rust
// Same convention Windows itself uses for skipping startup items.
unsafe { GetAsyncKeyState(VK_SHIFT) as u16 & 0x8000 != 0 }
```

Surfaced as helper text under the toggle in the group editor, otherwise nobody
would ever discover it.

## 5. Licensing

`config.json` is plain text in `%LOCALAPPDATA%`, so a UI-only gate is not a gate.
Enforcement goes in the startup path, next to the existing `license::is_licensed`.

Add to `license.rs`:

```rust
pub fn startup_groups_allowed(key: &Option<String>, id: &Option<String>) -> bool {
    is_licensed(key, id)
}
```

A named function rather than calling `is_licensed` inline, so the paid-feature
list is greppable in one place as it grows.

Interaction with existing free-tier enforcement:

- Flagged groups that are `hidden` are skipped. Free-tier enforcement hides all
  but one group, so a lapsed user does not get a surprise mass-launch.
- On revocation the flags **stay in the file** and simply stop firing, matching
  the "hide, don't delete" behaviour already chosen for groups. Re-activating
  restores the previous startup set with no reconfiguration.

## 6. UI

**Group editor (`config.html` / `config.js`)** — primary home, since this is a
per-group setting and that is where groups are configured. A checkbox near the
group name and colour:

```
[ ] Launch this group when Windows starts
    Hold Shift during startup to skip.        (helper text)
```

For unlicensed users: rendered but disabled, with a lock glyph and a link to
buy — consistent with the existing "Buy a license" affordance, and it advertises
the paid tier rather than hiding it.

**Widget right-click menu** — a checkable "Launch at startup" item on each
group's context menu, for quick toggling without opening the editor. Same
disabled-with-lock treatment when unlicensed.

**Settings tab** — a read-only summary listing the groups that will launch, in
order, so the whole startup sequence is visible in one place. Cheap to add and
the only view that answers "what actually happens when I boot?".

## 7. Risks

| Risk | Mitigation |
|---|---|
| Startup launch competes with a slow login | Windows already delays Run entries ~30s (`StartupDelayInMSec`), so login is finished by the time the process exists |
| A group misbehaves and the user is stuck every boot | Hold Shift to skip |
| Two groups targeting different virtual desktops interleave | Strict sequential launch (§3) |
| `save_group` drops the flag | Add to the preserve list (§1) |
| Free user hand-edits `config.json` | Enforced in Rust at fire time (§5) |
| Group with many items delays the widget | Batch runs on a background thread |

## 8. Files touched

- `src-tauri/src/config.rs` — `launch_at_startup` field
- `src-tauri/src/license.rs` — `startup_groups_allowed`
- `src-tauri/src/lib.rs` — `--autostart` flag in `register_autostart`, arg
  detection, Shift check, the startup batch, `save_group` preserve list, a
  `set_group_launch_at_startup` command, context-menu item
- `src/config.html` / `src/config.js` — toggle + helper text + Settings summary
- `src/widget.js` — context-menu item
- `src/styles.css` — disabled/locked toggle styling

## 9. Testing

`cargo test` cannot run on this project (see CLAUDE.md); use
`cargo check --tests`.

Unit-testable pure logic:

- `startup_groups_allowed` across all four key/instance combinations
- The group-selection filter: flagged + not hidden, in widget order

Manual, on the built exe (dev build won't have the Run entry):

1. Two flagged groups on different virtual desktops → both land correctly, and
   the second only starts after the first finishes
2. Manual launch → nothing auto-launches
3. Hold Shift through startup → nothing launches, log says skipped
4. Unlicensed → nothing launches even with flags set in `config.json` by hand
5. Free-tier enforcement active → hidden flagged groups skipped
6. Toggle a group, save, reopen the editor → flag survived (`save_group` check)

## 10. Out of scope

- Per-group startup delay or scheduling
- Launching groups at a time of day rather than at startup
- Reducing TakeOff's own 30–60s startup delay (separate task, agreed)
