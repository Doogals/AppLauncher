//! Anonymous weekly usage ping.
//!
//! **The rule this module exists to enforce: counts, never content.**
//! `12 app items` is a count. `C:\Users\dougb\Projects\thing.exe` is personal
//! data, and so are group names, URLs, Steam titles and the computer name. If a
//! field needs a judgement call about whether it is personal, it does not go in.
//!
//! `build_payload` is pure and returns a fixed set of keys, so the test at the
//! bottom can assert that no path, name or URL from a realistic config appears
//! anywhere in the serialised JSON. That test is the actual guarantee — the
//! Worker independently drops unknown fields as a second line of defence.
//!
//! Nothing here may block startup or surface an error. Analytics failing is
//! never a reason for the app to misbehave.

use crate::config::{AppConfig, ItemType};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ItemTypeCounts {
    pub app: usize,
    pub file: usize,
    pub url: usize,
    pub folder: usize,
    pub script: usize,
    pub steam: usize,
    pub uwp: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Payload {
    pub install_id: String,
    pub version: String,
    pub licensed: bool,
    pub os: String,
    pub group_count: usize,
    pub item_count: usize,
    pub items_by_type: ItemTypeCounts,
    pub startup_group_count: usize,
    pub detached_count: usize,
    /// Whether any item carries a saved launch position — the signal that the
    /// layout/capture features are actually being used.
    pub positioned_item_count: usize,
}

/// Builds the ping body. Pure: no I/O, no clock, no network.
pub fn build_payload(config: &AppConfig, install_id: &str, version: &str, os: &str) -> Payload {
    let mut counts = ItemTypeCounts {
        app: 0, file: 0, url: 0, folder: 0, script: 0, steam: 0, uwp: 0,
    };
    let mut item_count = 0usize;
    let mut positioned_item_count = 0usize;

    for group in &config.groups {
        for item in &group.items {
            item_count += 1;
            match item.item_type {
                ItemType::App => counts.app += 1,
                ItemType::File => counts.file += 1,
                ItemType::Url => counts.url += 1,
                ItemType::Folder => counts.folder += 1,
                ItemType::Script => counts.script += 1,
                ItemType::Steam => counts.steam += 1,
                ItemType::Uwp => counts.uwp += 1,
            }
            if item.launch_x.is_some() || item.launch_virtual_desktop.is_some() {
                positioned_item_count += 1;
            }
        }
    }

    Payload {
        install_id: install_id.to_string(),
        version: version.to_string(),
        licensed: crate::license::is_licensed(&config.license_key, &config.license_instance_id),
        os: os.to_string(),
        group_count: config.groups.len(),
        item_count,
        items_by_type: counts,
        startup_group_count: config.groups.iter().filter(|g| g.launch_at_startup).count(),
        detached_count: config.groups.iter().filter(|g| g.detached).count(),
        positioned_item_count,
    }
}

/// Whether a ping is due. `last_sent` is `YYYY-MM-DD`.
///
/// Unparseable or absent means due — a corrupted date should not silently
/// disable reporting forever.
pub fn is_due(last_sent: Option<&str>, today: &str) -> bool {
    let Some(last) = last_sent else { return true };
    let (Some(a), Some(b)) = (days_from_ymd(last), days_from_ymd(today)) else {
        return true;
    };
    b.saturating_sub(a) >= 7
}

/// Days since an arbitrary epoch, for date subtraction only. Civil-from-days
/// (Howard Hinnant's algorithm); correct across month and year boundaries,
/// which naive day-of-year arithmetic is not.
fn days_from_ymd(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(target_os = "windows")]
pub fn os_version() -> String {
    // Major.minor.build only. No edition, no install id, nothing identifying.
    #[repr(C)]
    struct OsVersionInfoW {
        dw_os_version_info_size: u32,
        dw_major_version: u32,
        dw_minor_version: u32,
        dw_build_number: u32,
        dw_platform_id: u32,
        sz_csd_version: [u16; 128],
    }
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }
    unsafe {
        let mut info = OsVersionInfoW {
            dw_os_version_info_size: std::mem::size_of::<OsVersionInfoW>() as u32,
            dw_major_version: 0,
            dw_minor_version: 0,
            dw_build_number: 0,
            dw_platform_id: 0,
            sz_csd_version: [0u16; 128],
        };
        if RtlGetVersion(&mut info) != 0 {
            return "unknown".to_string();
        }
        format!(
            "{}.{}.{}",
            info.dw_major_version, info.dw_minor_version, info.dw_build_number
        )
    }
}

#[cfg(not(target_os = "windows"))]
pub fn os_version() -> String {
    "non-windows".to_string()
}

#[cfg(target_os = "windows")]
pub fn today_ymd() -> String {
    #[repr(C)]
    struct SystemTime { year: u16, month: u16, dow: u16, day: u16, hour: u16, min: u16, sec: u16, ms: u16 }
    extern "system" { fn GetLocalTime(t: *mut SystemTime); }
    let mut t = SystemTime { year: 0, month: 0, dow: 0, day: 0, hour: 0, min: 0, sec: 0, ms: 0 };
    unsafe { GetLocalTime(&mut t); }
    format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
}

#[cfg(not(target_os = "windows"))]
pub fn today_ymd() -> String {
    "1970-01-01".to_string()
}

/// Sends the weekly ping if one is due. Returns immediately; the work happens
/// on a background thread so startup is never held up behind a network call.
pub fn maybe_send_weekly(app: tauri::AppHandle) {
    use tauri::Manager;

    std::thread::spawn(move || {
        let today = today_ymd();

        // Decide, mint an id if needed, and record the date — all under one
        // lock, then release before any network work.
        let payload = {
            let state = app.state::<crate::AppState>();
            let mut config = state.0.lock().unwrap();

            if !config.analytics_enabled || !is_due(config.analytics_last_sent.as_deref(), &today) {
                return;
            }

            let install_id = match &config.analytics_id {
                Some(id) => id.clone(),
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    config.analytics_id = Some(id.clone());
                    id
                }
            };

            // Recorded BEFORE sending, same reasoning as last_seen_version: if
            // the save fails we would otherwise retry on every single launch.
            config.analytics_last_sent = Some(today.clone());
            if crate::config::save_config(&config).is_err() {
                return;
            }

            build_payload(&config, &install_id, env!("CARGO_PKG_VERSION"), &os_version())
        };

        let Ok(client) = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        else {
            return;
        };

        // Failure is silent and not retried this session. Analytics must never
        // be a reason the user notices anything.
        let _ = client
            .post(format!("{}/ping", crate::WORKER_URL))
            .json(&payload)
            .send();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Group, Item};

    /// Built through serde rather than a struct literal: `Item` has no
    /// `Default`, and every field except these two already has a serde default,
    /// so this stays correct when fields are added — the exact breakage that
    /// hit the config.rs and launcher.rs tests when `detached` and `hidden`
    /// were introduced.
    fn item(kind: &str, path: &str) -> Item {
        let json = format!(r#"{{"item_type":"{kind}","path":"{path}"}}"#);
        serde_json::from_str(&json).expect("test item should deserialize")
    }

    fn positioned_item(kind: &str, path: &str) -> Item {
        let json = format!(r#"{{"item_type":"{kind}","path":"{path}","launch_x":100}}"#);
        serde_json::from_str(&json).expect("test item should deserialize")
    }

    fn realistic_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.license_key = Some("SECRET-LICENSE-KEY-1234".into());
        config.license_instance_id = Some("instance-abcdef".into());

        let mut work = Group::new("Doug's Work Stuff", "💼");
        work.items = vec![
            positioned_item("app", r"C:\\Users\\dougb\\Desktop\\secret-project.exe"),
            item("url", "https://internal.tonic-tech.com/admin"),
            item("steam", "730"),
        ];
        work.launch_at_startup = true;

        let mut games = Group::new("Gaming", "🎮");
        games.items = vec![item("uwp", "SomePackage_8wekyb3d8bbwe!App")];
        games.detached = true;

        config.groups = vec![work, games];
        config
    }

    #[test]
    fn payload_counts_are_right() {
        let p = build_payload(&realistic_config(), "install-1", "0.6.7", "10.0.26200");
        assert_eq!(p.group_count, 2);
        assert_eq!(p.item_count, 4);
        assert_eq!(p.items_by_type.app, 1);
        assert_eq!(p.items_by_type.url, 1);
        assert_eq!(p.items_by_type.steam, 1);
        assert_eq!(p.items_by_type.uwp, 1);
        assert_eq!(p.startup_group_count, 1);
        assert_eq!(p.detached_count, 1);
        assert!(p.licensed);
    }

    /// The guarantee this whole module exists for. If someone adds a field that
    /// carries content rather than a count, this fails.
    #[test]
    fn payload_leaks_no_personal_data() {
        let config = realistic_config();
        let p = build_payload(&config, "install-1", "0.6.7", "10.0.26200");
        let json = serde_json::to_string(&p).unwrap();

        for forbidden in [
            "Doug",
            "dougb",
            "secret-project",
            "C:\\",
            "https://",
            "tonic-tech",
            "SECRET-LICENSE-KEY-1234",
            "instance-abcdef",
            "Gaming",
            "Work Stuff",
            "8wekyb3d8bbwe",
            "730",
        ] {
            assert!(
                !json.contains(forbidden),
                "payload leaked {forbidden:?}: {json}"
            );
        }
    }

    #[test]
    fn payload_has_only_allow_listed_keys() {
        let p = build_payload(&AppConfig::default(), "id", "0.6.7", "10.0.0");
        let value: serde_json::Value = serde_json::to_value(&p).unwrap();
        let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec![
            "detached_count", "group_count", "install_id", "item_count",
            "items_by_type", "licensed", "os", "positioned_item_count",
            "startup_group_count", "version",
        ]);
    }

    #[test]
    fn unlicensed_config_reports_unlicensed() {
        let p = build_payload(&AppConfig::default(), "id", "0.6.7", "10.0.0");
        assert!(!p.licensed);
    }

    #[test]
    fn due_when_never_sent_or_unparseable() {
        assert!(is_due(None, "2026-10-18"));
        assert!(is_due(Some(""), "2026-10-18"));
        assert!(is_due(Some("not-a-date"), "2026-10-18"));
        assert!(is_due(Some("2026-13-99"), "2026-10-18"));
    }

    #[test]
    fn weekly_gate_boundaries() {
        assert!(!is_due(Some("2026-10-18"), "2026-10-18"));
        assert!(!is_due(Some("2026-10-12"), "2026-10-18")); // 6 days
        assert!(is_due(Some("2026-10-11"), "2026-10-18"));  // 7 days
    }

    #[test]
    fn weekly_gate_crosses_month_and_year_boundaries() {
        assert!(is_due(Some("2026-01-28"), "2026-02-04"));
        assert!(is_due(Some("2025-12-28"), "2026-01-04"));
        assert!(!is_due(Some("2026-02-26"), "2026-03-01")); // leap-year aware
    }

    #[test]
    fn clock_going_backwards_does_not_spam() {
        // A user fixing a wrong system clock must not trigger a ping every
        // launch; saturating_sub keeps the difference at 0.
        assert!(!is_due(Some("2026-10-18"), "2020-01-01"));
    }
}
