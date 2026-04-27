# Design: Complete License System

**Date:** 2026-04-27
**Status:** Approved

---

## Overview

Replace the current local SHA256 license validation with a LemonSqueezy-backed per-machine enforcement system. A Cloudflare Worker proxy keeps the LS API key server-side. The app calls the Worker to activate, deactivate, and validate licenses.

---

## Architecture

```
User buys on LemonSqueezy → gets key via email automatically
       ↓
User enters key in App Launcher config window
       ↓
App → POST https://app-launcher-license.<name>.workers.dev/activate
       ↓
Worker → LemonSqueezy API (with secret API key as env var)
       ↓
LS checks: key valid? not already activated on another machine?
       ↓
Returns instance_id → App stores key + instance_id in config.json
       ↓
Shows "✓ Licensed" state
```

---

## Part 1 — LemonSqueezy Setup (external — Doug does this)

Steps:
1. Create account at lemonsqueezy.com
2. Create a Store
3. Create a Product → set type to **"License Key"** → set **activation limit = 1**
4. Set the price
5. Go to Settings → API → generate an API key
6. The API key goes into the Cloudflare Worker environment (never in the app binary)

LemonSqueezy automatically generates a unique key and emails it to every buyer after purchase. No key generation tool needed — LS handles it.

---

## Part 2 — Cloudflare Worker (proxy)

A single `worker.js` file deployed to Cloudflare Workers (free tier). Exposes three endpoints. The LS API key is stored as a Worker environment variable (`LS_API_KEY`).

**File:** `cloudflare-worker/worker.js` (committed to the repo for reference)

**Endpoints:**

`POST /activate`
- Request body: `{ "license_key": "...", "instance_name": "..." }`
- Forwards to: `POST https://api.lemonsqueezy.com/v1/licenses/activate`
- Returns on success: `{ "instance_id": "...", "instance_name": "..." }`
- Returns on failure: `{ "error": "..." }` with appropriate HTTP status

`POST /deactivate`
- Request body: `{ "license_key": "...", "instance_id": "..." }`
- Forwards to: `POST https://api.lemonsqueezy.com/v1/licenses/deactivate`
- Returns: `{ "ok": true }` or `{ "error": "..." }`

`POST /validate`
- Request body: `{ "license_key": "...", "instance_id": "..." }`
- Forwards to: `POST https://api.lemonsqueezy.com/v1/licenses/validate`
- Returns: `{ "valid": true/false }` or `{ "error": "..." }`

**Worker URL** is a constant in Rust (`WORKER_URL`). After deploying, update this constant with the actual URL.

**Deployment steps (Doug does once):**
1. Create a Cloudflare account (free) at cloudflare.com
2. Go to Workers → Create Worker → paste the worker.js content
3. Add environment variable `LS_API_KEY` = your LemonSqueezy API key
4. Deploy — get the worker URL
5. Paste the worker URL into `WORKER_URL` constant in `lib.rs`

---

## Part 3 — App Changes

### Config (config.rs)

Add `license_instance_id: Option<String>` to `AppConfig`:

```rust
pub struct AppConfig {
    pub preferred_browser: Option<String>,
    pub license_key: Option<String>,
    pub license_instance_id: Option<String>, // NEW — LemonSqueezy instance ID
    pub groups: Vec<Group>,
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
}
```

### Remove SHA256 validation (license.rs)

- Remove `validate_key()` and `generate_key()` — no longer needed
- Keep `is_licensed()` (now checks if both `license_key` AND `license_instance_id` are stored)
- Keep `group_limit()` (same behavior: 2 free, unlimited licensed)

### New/replaced Rust commands (lib.rs)

**`activate_license(key: String) -> Result<(), String>`** (replaces current SHA256 version)
- Gets machine hostname via `std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Unknown PC".to_string())` — Windows-native, no extra crate needed
- POSTs `{ license_key, instance_name }` to Worker `/activate`
- On success: stores key + instance_id in config, saves config
- On failure: returns error string to display in UI

**`deactivate_license() -> Result<(), String>`** (new)
- POSTs `{ license_key, instance_id }` from stored config to Worker `/deactivate`
- On success: clears `license_key` and `license_instance_id` from config, saves
- On failure: returns error string

**`check_license_status() -> LicenseStatus`** (new)
- Called from `widget.js` during `render()` init — fire-and-forget via `.catch(console.error)`
- POSTs `{ license_key, instance_id }` to Worker `/validate`
- Returns `LicenseStatus` enum: `Licensed`, `Revoked`, `Unreachable`
- Widget.js stores the result in a module-level variable; config window reads it via `invoke('check_license_status')` when it opens to decide whether to show the revoked warning

```rust
#[derive(Serialize)]
pub enum LicenseStatus {
    Licensed,
    Revoked,
    Unreachable,
}
```

**HTTP client:** Add `reqwest` crate with `blocking` feature (synchronous, appropriate for Tauri commands which run on the thread pool).

**Worker URL constant:**
```rust
const WORKER_URL: &str = "https://app-launcher-license.YOUR_NAME.workers.dev";
```

### Dependencies to add (Cargo.toml)

```toml
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
```

---

## Part 4 — In-App UX (config.js + config.html)

The license section in the config window bottom bar shows different states:

**Unlicensed state:**
```
🔑 License  [expand]
  ┌─────────────────────────────────┐
  │ XXXX-XXXX-XXXX-XXXX   [Activate]│
  └─────────────────────────────────┘
  Buy a license →  (link to LS store)
```

**Activating state:**
- Button shows "Activating..." and is disabled

**Licensed state:**
```
✓ Licensed — [Machine Name]   [Transfer]
```
- No key input shown
- "Transfer" button triggers deactivation and returns to unlicensed state

**Error state:**
- Inline error message below the input (e.g. "Already activated on another machine", "Invalid key")

**Startup validation:**
- On widget init, if license is stored, call `check_license_status()` silently
- If `Revoked`: next time config window opens, show warning banner at top: "⚠ License revoked. Please contact support."
- If `Unreachable`: do nothing (assume valid, user is offline)
- If `Licensed`: no change

**Buy link:** A small text link below the input when unlicensed:
`Buy a license →` pointing to the LemonSqueezy store URL.
Store URL is a constant in `config.js`: `const STORE_URL = 'https://YOUR_STORE.lemonsqueezy.com/buy/YOUR_PRODUCT';`

---

## Part 5 — Files Changed

| File | Change |
|------|--------|
| `cloudflare-worker/worker.js` | New — Cloudflare Worker source (for reference + deployment) |
| `src-tauri/Cargo.toml` | Add `reqwest` |
| `src-tauri/src/config.rs` | Add `license_instance_id` to AppConfig |
| `src-tauri/src/license.rs` | Remove SHA256 logic, update `is_licensed` |
| `src-tauri/src/lib.rs` | Replace `activate_license`, add `deactivate_license`, add `check_license_status` |
| `src/config.js` | License section states (unlicensed/activating/licensed/transfer), startup validation call |
| `src/config.html` | Update license section HTML for new states |

---

## Out of Scope

- Multiple license tiers (single price only)
- License management portal for users (handled by LemonSqueezy)
- Offline activation (requires internet on first activation)
- Admin key generation tool (LemonSqueezy dashboard handles this)
