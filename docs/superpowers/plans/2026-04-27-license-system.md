# License System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace local SHA256 license validation with LemonSqueezy per-machine enforcement via a Cloudflare Worker proxy, with a full in-app UX for activation, deactivation, and transfer.

**Architecture:** A Cloudflare Worker sits between the app and LemonSqueezy's Licenses API, keeping the LS API key server-side. The app calls the Worker via `reqwest` blocking HTTP. License state (key + instance_id + machine_name) is stored in config.json. The in-app license section has distinct unlicensed / activating / licensed / transfer / revoked states rendered via JS.

**Tech Stack:** Rust (`reqwest` 0.12 blocking + `serde_json`), Cloudflare Workers (JS), LemonSqueezy Licenses API, Vanilla JS/HTML/CSS.

---

## Prerequisites (Doug does these before running the plan)

1. Create a LemonSqueezy account at lemonsqueezy.com
2. Create a Store → Create a Product → set type **"License Key"** → set **activation limit = 1**
3. Set a price
4. Go to **Settings → API** → generate an API key — keep it handy
5. Create a free Cloudflare account at cloudflare.com

---

## File Map

| File | Change |
|------|--------|
| `cloudflare-worker/worker.js` | New — Worker source (deploy this to Cloudflare) |
| `src-tauri/Cargo.toml` | Add `reqwest` |
| `src-tauri/src/config.rs` | Add `license_instance_id`, `license_machine_name` to AppConfig |
| `src-tauri/src/license.rs` | Remove SHA256 logic; update `is_licensed` and `group_limit` signatures |
| `src-tauri/src/lib.rs` | Replace `activate_license`; add `deactivate_license`, `check_license_status`; update `save_group` call |
| `src/config.html` | Add `id="license-content"` wrapper inside details for JS-driven state rendering |
| `src/config.js` | Rewrite license section: `renderLicenseSection()`, activation/deactivation/transfer flow, startup validation |
| `src/styles.css` | Add `.buy-link` style |

---

## Task 1: Cloudflare Worker

**Files:**
- Create: `cloudflare-worker/worker.js`

- [ ] **Step 1: Create the worker file**

Create directory `cloudflare-worker/` and file `cloudflare-worker/worker.js` with:

```js
const LS_BASE = 'https://api.lemonsqueezy.com/v1/licenses';

export default {
  async fetch(request, env) {
    if (request.method !== 'POST') {
      return new Response('Method not allowed', { status: 405 });
    }

    const url = new URL(request.url);
    const action = url.pathname.slice(1); // 'activate', 'deactivate', or 'validate'

    if (!['activate', 'deactivate', 'validate'].includes(action)) {
      return new Response('Not found', { status: 404 });
    }

    let body;
    try {
      body = await request.json();
    } catch {
      return Response.json({ error: 'Invalid JSON body' }, { status: 400 });
    }

    const lsRes = await fetch(`${LS_BASE}/${action}`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${env.LS_API_KEY}`,
        'Content-Type': 'application/json',
        'Accept': 'application/json',
      },
      body: JSON.stringify(body),
    });

    const data = await lsRes.json();

    if (action === 'activate') {
      if (lsRes.ok && data.activated) {
        return Response.json({
          instance_id: data.instance.id,
          instance_name: data.instance.name,
        });
      }
      return Response.json(
        { error: data.error || data.errors?.[0]?.detail || 'Activation failed' },
        { status: 400 }
      );
    }

    if (action === 'deactivate') {
      if (lsRes.ok && data.deactivated) {
        return Response.json({ ok: true });
      }
      return Response.json(
        { error: data.error || 'Deactivation failed' },
        { status: 400 }
      );
    }

    // validate
    return Response.json({ valid: lsRes.ok && data.valid === true });
  },
};
```

- [ ] **Step 2: Deploy to Cloudflare**

1. Log in to cloudflare.com → Workers & Pages → Create Application → Create Worker
2. Paste the `worker.js` content → Save and Deploy
3. Go to the Worker → Settings → Variables → add `LS_API_KEY` = your LemonSqueezy API key → Save
4. Note the Worker URL (e.g. `https://app-launcher-license.YOUR_SUBDOMAIN.workers.dev`)

- [ ] **Step 3: Commit the worker source**

```bash
git add cloudflare-worker/worker.js
git commit -m "feat: add Cloudflare Worker proxy for LemonSqueezy license API"
```

---

## Task 2: Add reqwest to Cargo.toml

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml`, add after the `lettre` line:

```toml
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
```

- [ ] **Step 2: Verify compile**

```bash
cd src-tauri && cargo check
```

Expected: success (may take a minute to download reqwest).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "chore: add reqwest for LemonSqueezy HTTP calls"
```

---

## Task 3: Update AppConfig

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Add failing test**

In `src-tauri/src/config.rs`, add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_config_has_license_instance_fields() {
    let mut config = AppConfig::default();
    assert!(config.license_instance_id.is_none());
    assert!(config.license_machine_name.is_none());
    config.license_instance_id = Some("inst-123".to_string());
    config.license_machine_name = Some("My PC".to_string());
    let loaded = tmp_config_roundtrip(&config);
    assert_eq!(loaded.license_instance_id, Some("inst-123".to_string()));
    assert_eq!(loaded.license_machine_name, Some("My PC".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd src-tauri && cargo test test_config_has_license_instance_fields
```

Expected: compile error — fields don't exist yet.

- [ ] **Step 3: Add fields to AppConfig**

In `src-tauri/src/config.rs`, update `AppConfig` to:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct AppConfig {
    pub preferred_browser: Option<String>,
    pub license_key: Option<String>,
    pub license_instance_id: Option<String>,
    pub license_machine_name: Option<String>,
    pub groups: Vec<Group>,
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd src-tauri && cargo test test_config_has_license_instance_fields
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat: add license_instance_id and license_machine_name to AppConfig"
```

---

## Task 4: Rewrite license.rs

**Files:**
- Modify: `src-tauri/src/license.rs`

- [ ] **Step 1: Write new tests**

Replace the entire `src-tauri/src/license.rs` with the stub + new tests:

```rust
pub fn is_licensed(license_key: &Option<String>, instance_id: &Option<String>) -> bool {
    todo!()
}

pub fn group_limit(license_key: &Option<String>, instance_id: &Option<String>) -> usize {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_licensed_requires_both_key_and_instance() {
        assert!(!is_licensed(&None, &None));
        assert!(!is_licensed(&Some("key".to_string()), &None));
        assert!(!is_licensed(&None, &Some("inst".to_string())));
        assert!(is_licensed(&Some("key".to_string()), &Some("inst".to_string())));
    }

    #[test]
    fn test_group_limit_unlicensed() {
        assert_eq!(group_limit(&None, &None), 2);
    }

    #[test]
    fn test_group_limit_licensed() {
        assert_eq!(
            group_limit(&Some("key".to_string()), &Some("inst".to_string())),
            usize::MAX
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd src-tauri && cargo test license
```

Expected: compile errors from `todo!()`.

- [ ] **Step 3: Implement**

Replace the `todo!()` stubs with implementations:

```rust
pub fn is_licensed(license_key: &Option<String>, instance_id: &Option<String>) -> bool {
    license_key.is_some() && instance_id.is_some()
}

pub fn group_limit(license_key: &Option<String>, instance_id: &Option<String>) -> usize {
    if is_licensed(license_key, instance_id) { usize::MAX } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_licensed_requires_both_key_and_instance() {
        assert!(!is_licensed(&None, &None));
        assert!(!is_licensed(&Some("key".to_string()), &None));
        assert!(!is_licensed(&None, &Some("inst".to_string())));
        assert!(is_licensed(&Some("key".to_string()), &Some("inst".to_string())));
    }

    #[test]
    fn test_group_limit_unlicensed() {
        assert_eq!(group_limit(&None, &None), 2);
    }

    #[test]
    fn test_group_limit_licensed() {
        assert_eq!(
            group_limit(&Some("key".to_string()), &Some("inst".to_string())),
            usize::MAX
        );
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd src-tauri && cargo test license
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/license.rs
git commit -m "refactor: replace SHA256 license validation with instance-based check"
```

---

## Task 5: New Rust Commands in lib.rs

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add WORKER_URL constant and LicenseStatus type**

At the top of `src-tauri/src/lib.rs`, after the `use` statements, add:

```rust
// Update this URL after deploying the Cloudflare Worker
const WORKER_URL: &str = "https://app-launcher-license.YOUR_SUBDOMAIN.workers.dev";

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum LicenseStatus {
    Licensed,
    Revoked,
    Unlicensed,
    Unreachable,
}
```

- [ ] **Step 2: Fix save_group to use updated group_limit signature**

In `save_group`, change:
```rust
let limit = license::group_limit(&config.license_key);
```
to:
```rust
let limit = license::group_limit(&config.license_key, &config.license_instance_id);
```

- [ ] **Step 3: Replace activate_license command**

Replace the existing `activate_license` function with:

```rust
#[tauri::command]
fn activate_license(key: String, state: State<AppState>) -> Result<(), String> {
    let machine_name = std::env::var("COMPUTERNAME")
        .unwrap_or_else(|_| "Unknown PC".to_string());

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .post(format!("{}/activate", WORKER_URL))
        .json(&serde_json::json!({
            "license_key": key,
            "instance_name": machine_name,
        }))
        .send()
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
        return Err(body["error"].as_str().unwrap_or("Activation failed").to_string());
    }

    let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
    let instance_id = body["instance_id"]
        .as_str()
        .ok_or("Invalid response from server")?
        .to_string();

    let mut config = state.0.lock().unwrap();
    config.license_key = Some(key);
    config.license_instance_id = Some(instance_id);
    config.license_machine_name = Some(machine_name);
    config::save_config(&config)
}
```

- [ ] **Step 4: Add deactivate_license command**

Add after `activate_license`:

```rust
#[tauri::command]
fn deactivate_license(state: State<AppState>) -> Result<(), String> {
    let (key, instance_id) = {
        let config = state.0.lock().unwrap();
        (config.license_key.clone(), config.license_instance_id.clone())
    };
    let (key, instance_id) = match (key, instance_id) {
        (Some(k), Some(i)) => (k, i),
        _ => return Err("No active license to deactivate.".to_string()),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .post(format!("{}/deactivate", WORKER_URL))
        .json(&serde_json::json!({
            "license_key": key,
            "instance_id": instance_id,
        }))
        .send()
        .map_err(|e| format!("Network error: {}", e))?;

    if !res.status().is_success() {
        let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
        return Err(body["error"].as_str().unwrap_or("Deactivation failed").to_string());
    }

    let mut config = state.0.lock().unwrap();
    config.license_key = None;
    config.license_instance_id = None;
    config.license_machine_name = None;
    config::save_config(&config)
}
```

- [ ] **Step 5: Add check_license_status command**

Add after `deactivate_license`:

```rust
#[tauri::command]
fn check_license_status(state: State<AppState>) -> LicenseStatus {
    let (key, instance_id) = {
        let config = state.0.lock().unwrap();
        (config.license_key.clone(), config.license_instance_id.clone())
    };
    let (key, instance_id) = match (key, instance_id) {
        (Some(k), Some(i)) => (k, i),
        _ => return LicenseStatus::Unlicensed,
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return LicenseStatus::Unreachable,
    };

    let res = match client
        .post(format!("{}/validate", WORKER_URL))
        .json(&serde_json::json!({
            "license_key": key,
            "instance_id": instance_id,
        }))
        .send()
    {
        Ok(r) => r,
        Err(_) => return LicenseStatus::Unreachable,
    };

    let body: serde_json::Value = match res.json() {
        Ok(b) => b,
        Err(_) => return LicenseStatus::Unreachable,
    };

    if body["valid"].as_bool() == Some(true) {
        LicenseStatus::Licensed
    } else {
        LicenseStatus::Revoked
    }
}
```

- [ ] **Step 6: Register new commands in invoke_handler**

Update the `invoke_handler` in `run()` to add `deactivate_license` and `check_license_status`:

```rust
.invoke_handler(tauri::generate_handler![
    get_config,
    save_group,
    delete_group,
    launch_group,
    set_preferred_browser,
    activate_license,
    deactivate_license,
    check_license_status,
    reorder_items,
    save_widget_position,
    resize_widget,
    get_installed_apps,
    show_group_context_menu,
    get_installed_browsers,
    get_browser_bookmarks,
    send_feedback,
])
```

- [ ] **Step 7: Verify compile and run tests**

```bash
cd src-tauri && cargo test
```

Expected: all tests pass. Two pre-existing dead-code warnings are fine.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: LemonSqueezy license commands (activate, deactivate, check_status)"
```

---

## Task 6: In-App License UX

**Files:**
- Modify: `src/config.html`
- Modify: `src/config.js`
- Modify: `src/styles.css`

The license `<details>` section shows different content depending on license state. JS drives all state changes by replacing the inner HTML of `#license-content`.

- [ ] **Step 1: Update config.html — add license-content wrapper**

Replace the `<details class="license-details">` block in `src/config.html` with:

```html
    <div class="config-bottom">
      <details class="license-details" id="license-details">
        <summary id="license-summary">🔑 License</summary>
        <div id="license-content"></div>
      </details>
      <button class="feedback-btn" id="feedback-btn">💬 Feedback</button>
    </div>
```

- [ ] **Step 2: Add .buy-link style to styles.css**

Append to the bottom of `src/styles.css`:

```css
.buy-link {
  display: inline-block;
  margin-top: 6px;
  font-size: 0.75rem;
  color: #888;
  text-decoration: none;
}
.buy-link:hover { color: #e94560; }
```

- [ ] **Step 3: Move fitWindow to module scope in config.js**

`fitWindow` is currently defined inside `init()`. `renderLicenseSection()` (added next) lives at module scope and needs it. Find `fitWindow` inside `init()`:

```js
  async function fitWindow() {
    await new Promise(resolve => requestAnimationFrame(resolve));
    const h = document.documentElement.scrollHeight;
    await getCurrentWindow().setSize(new LogicalSize(420, h));
  }
```

Cut it out of `init()` and paste it as a standalone module-level function just before `init()`. The `toggle` listener in `init()` that calls it stays where it is — it still works since `fitWindow` is now in scope everywhere.

- [ ] **Step 4: Rewrite license section in config.js**

Replace the existing license section code in `src/config.js`. The existing code is the `activateBtn` block and `updateLicenseStatus` function. Replace them entirely with:

```js
// Store URL — update after creating your LemonSqueezy product
const STORE_URL = 'https://app-launcher.lemonsqueezy.com/buy/YOUR_PRODUCT_ID';

async function renderLicenseSection() {
  const config = await invoke('get_config');
  const content = document.getElementById('license-content');
  const summary = document.getElementById('license-summary');

  if (config.license_key && config.license_instance_id) {
    // Licensed state
    summary.textContent = '✓ Licensed';
    content.innerHTML = `
      <div class="license-row" style="margin-top: 7px; align-items: center;">
        <span style="flex:1; font-size:0.78rem; color:#4caf50;">
          Active on ${config.license_machine_name || 'this machine'}
        </span>
        <button class="btn btn-cancel license-activate" id="transfer-btn">Transfer</button>
      </div>
    `;
    document.getElementById('transfer-btn').addEventListener('click', async () => {
      const btn = document.getElementById('transfer-btn');
      btn.textContent = 'Deactivating...';
      btn.disabled = true;
      try {
        await invoke('deactivate_license');
        await renderLicenseSection();
        fitWindow();
      } catch (e) {
        btn.textContent = 'Transfer';
        btn.disabled = false;
        showLicenseError(e);
      }
    });
  } else {
    // Unlicensed state
    summary.textContent = '🔑 License';
    content.innerHTML = `
      <div class="license-row">
        <input type="text" class="license-input" id="license-input"
          placeholder="XXXX-XXXX-XXXX-XXXX" autocomplete="off" />
        <button class="btn btn-save license-activate" id="activate-btn">Activate</button>
      </div>
      <p id="license-status" class="license-status"></p>
      <a href="${STORE_URL}" target="_blank" class="buy-link">Buy a license →</a>
    `;
    document.getElementById('activate-btn').addEventListener('click', async () => {
      const key = document.getElementById('license-input').value.trim();
      if (!key) return;
      const btn = document.getElementById('activate-btn');
      btn.textContent = 'Activating...';
      btn.disabled = true;
      try {
        await invoke('activate_license', { key });
        await renderLicenseSection();
        fitWindow();
      } catch (e) {
        btn.textContent = 'Activate';
        btn.disabled = false;
        showLicenseError(e);
      }
    });
  }

  fitWindow();
}

function showLicenseError(msg) {
  const status = document.getElementById('license-status');
  if (status) {
    status.textContent = typeof msg === 'string' ? msg : 'Something went wrong.';
    status.style.color = '#e94560';
  }
}
```

- [ ] **Step 5: Call renderLicenseSection from init() and set up startup validation**

In `init()`, replace the `await updateLicenseStatus()` call with:

```js
  await renderLicenseSection();

  // Silent background validation — check if license is still valid on LS
  invoke('check_license_status').then(status => {
    if (status === 'revoked') {
      const summary = document.getElementById('license-summary');
      if (summary) summary.textContent = '⚠ License Revoked';
      const content = document.getElementById('license-content');
      if (content) content.innerHTML = `
        <p style="font-size:0.78rem; color:#e94560; margin-top:6px;">
          Your license has been revoked. Please contact support.
        </p>
      `;
      fitWindow();
    }
  }).catch(() => {}); // Unreachable = offline, ignore silently
```

- [ ] **Step 6: Remove the old updateLicenseStatus function**

Delete the `async function updateLicenseStatus() { ... }` function entirely from `config.js` — it has been replaced by `renderLicenseSection()`.

Also delete the old `const activateBtn = document.getElementById('activate-btn'); if (activateBtn) { ... }` block — activation is now handled inside `renderLicenseSection()`.

- [ ] **Step 7: Verify manually**

Run `npm run tauri dev`. Open the config window (click + on widget).

Check unlicensed state:
- License section shows `🔑 License` summary
- Expanding shows key input + Activate button + "Buy a license →" link
- Clicking Activate with empty field does nothing
- Clicking Activate with an invalid key shows an error

Check licensed state (enter a real LS key after LemonSqueezy is set up, or test with: try activating, observe "Activating..." state).

Check Transfer:
- In licensed state, Transfer button deactivates and returns to unlicensed state.

- [ ] **Step 8: Commit**

```bash
git add src/config.html src/config.js src/styles.css
git commit -m "feat: in-app license UX (unlicensed/activating/licensed/transfer/revoked states)"
```

---

## Task 7: Update WORKER_URL constant

After deploying the Cloudflare Worker (Task 1 Step 2), update the placeholder URL.

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Update the constant**

In `src-tauri/src/lib.rs`, find:

```rust
const WORKER_URL: &str = "https://app-launcher-license.YOUR_SUBDOMAIN.workers.dev";
```

Replace `YOUR_SUBDOMAIN` with your actual Cloudflare subdomain from the Worker deployment URL.

- [ ] **Step 2: Verify compile**

```bash
cd src-tauri && cargo check
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "chore: update WORKER_URL to deployed Cloudflare Worker"
```

---

## Done

Full end-to-end test:
1. Set up LemonSqueezy product + get a test license key from LS dashboard
2. Run `npm run tauri dev`
3. Open config window → expand License → enter key → Activate
4. Verify: summary shows "✓ Licensed", machine name shown, Transfer button visible
5. Click Transfer → verify returns to unlicensed state
6. Verify group limit: unlicensed = max 2 groups, licensed = unlimited
