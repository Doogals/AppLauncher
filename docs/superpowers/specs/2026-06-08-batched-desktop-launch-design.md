# Batched Desktop Launch + Desktop-1 Default — Design

## Problem

Two related issues in the virtual desktop launch flow:

1. **Back-and-forth desktop switching**: The launcher visits desktops in the exact order items appear
   in the group, causing redundant switches when multiple items target the same desktop but
   aren't adjacent. E.g., items [D2, D1, D2] requires 3 switches: →D2, →D1, →D2 — visiting
   Desktop 2 twice.

2. **"Any desktop" ambiguity**: The layout editor allows items to have no pinned desktop. This
   makes launch behavior unpredictable (items land wherever the current desktop happens to be),
   and creates a special-case in the batching logic (what bucket does an unpinned item go into?).

## Design

### Part 1: Batch launches by target desktop

**Location:** `src-tauri/src/launcher.rs` — `launch_group` function (lines 459–495)

**When it activates:** Only when `needs_vd` is true (at least one item has an explicit
`launch_virtual_desktop`). Groups with no desktop targets are unchanged — they launch
sequentially without any VD switching, same as today.

**Algorithm — two phases:**

Phase 1 — Build ordered batches:
Walk the group's items in their saved order. For each item that belongs in a desktop batch
(non-URL items always; URL items only if they have a saved position or explicit desktop),
determine its target: `launch_virtual_desktop` if `Some`, otherwise Desktop 1's GUID
(first entry from `get_virtual_desktops()`). Bucket items by target GUID, preserving the
order in which each distinct GUID first appears. Result: `Vec<(target_guid, Vec<&Item>)>`.

Phase 2 — Execute batches:
For each batch in order: switch to that desktop once (skip if already there), then launch
every item in the batch back-to-back in their original relative order. Existing
restore-to-original-desktop logic at the end of `launch_group` is unchanged.

**Effect on the reported example:**

Items [item1→D2, item2→D1, item3→D2]:
- Batches: D2=[item1, item3] (first seen at position 1), D1=[item2] (first seen at position 2)
- Execution: switch to D2 → launch item1, item3 → switch to D1 → launch item2
- Total switches: 2 (was 3). Desktop 2 visited once; both its items launch consecutively.

**URL items:** URL items with a saved position or explicit desktop are folded into the
batch loop (previously handled in a separate second loop). URL items with neither position
nor desktop remain in the multi-tab collect-and-batch pass at the end — that behavior is
unchanged.

**Non-Windows:** On non-Windows, `get_virtual_desktops()` returns an empty list and no
`switch_virtual_desktop` calls fire. The code retains the original sequential loops via
`#[cfg(not(target_os = "windows"))]`.

### Part 2: Remove "Any desktop", default to Desktop 1

**Locations:** `src/layout-item.html`, `src/layout-item.js`

**UI change:** Delete the blank `<option value="">Any desktop</option>` from the desktop
dropdown in the layout editor. The dropdown always has a concrete selection.

**Default for new/null items:** When `initDesktopDropdown` runs with no `vd` URL param
(item has `launch_virtual_desktop: null`), the dropdown defaults to the first entry from
`get_virtual_desktops()` (Desktop 1). `set_layout_item_desktop` is called immediately with
Desktop 1's GUID so that:
- The layout-item window visually moves to Desktop 1
- The GUID is stored in `LayoutDesktops` so `complete_layout_save` picks it up without
  requiring the user to touch the dropdown

**Forward effect:** Any item saved via the layout editor after this change will have an
explicit Desktop-1 GUID instead of null. New items always land in the Desktop-1 batch.

**Backward compatibility:** `launch_virtual_desktop: Option<Vec<u8>>` stays in the data
model. Existing items with `null` are treated as Desktop 1 in the batch-launch logic
(target = `desktop_1_guid` via `unwrap_or_else`). No startup migration runs.

## Out of scope

- No changes to how desktops are enumerated (`get_virtual_desktops`, `virtual_desktop.rs`)
- No data migration: existing configs with `null` remain valid; `null` and Desktop-1 GUID
  are semantically equivalent going forward
- No changes to non-desktop-targeted groups (groups where `needs_vd = false` continue
  launching sequentially, identical to today)
- No changes to multi-tab URL batching logic (URLs without position/desktop still batch
  together at end of `launch_group`)
