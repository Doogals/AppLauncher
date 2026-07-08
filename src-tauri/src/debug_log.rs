use std::io::Write;

/// Appends a timestamped line to %TEMP%\applauncher-debug.log.
/// Compiled to a no-op in release builds.
pub fn write_debug_log(msg: &str) {
    #[cfg(debug_assertions)]
    {
        let path = std::env::temp_dir().join("applauncher-debug.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "[{}] {}", local_time(), msg);
        }
    }
    // Release build: intentionally empty — zero overhead
    #[cfg(not(debug_assertions))]
    let _ = msg;
}

/// Truncates the log file.
#[allow(dead_code)]
pub fn clear_debug_log() {
    #[cfg(debug_assertions)]
    {
        let path = std::env::temp_dir().join("applauncher-debug.log");
        let _ = std::fs::write(&path, "");
    }
    // Release build: intentionally empty — zero overhead
    #[cfg(not(debug_assertions))]
    {}
}

#[cfg(target_os = "windows")]
fn local_time() -> String {
    #[repr(C)]
    struct SYSTEMTIME { year: u16, month: u16, dow: u16, day: u16, hour: u16, min: u16, sec: u16, ms: u16 }
    extern "system" { fn GetLocalTime(t: *mut SYSTEMTIME); }
    let mut t = SYSTEMTIME { year: 0, month: 0, dow: 0, day: 0, hour: 0, min: 0, sec: 0, ms: 0 };
    unsafe { GetLocalTime(&mut t); }
    format!("{:02}:{:02}:{:02}", t.hour, t.min, t.sec)
}

#[cfg(not(target_os = "windows"))]
fn local_time() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{:02}:{:02}:{:02}", (s % 86400) / 3600, (s % 3600) / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to prevent tests from interfering with each other
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    #[cfg(debug_assertions)]
    fn test_write_debug_log_creates_file_and_appends() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = std::env::temp_dir().join("applauncher-debug.log");
        // Clean start: remove any existing file
        let _ = std::fs::remove_file(&path);
        write_debug_log("TEST_ENTRY_A");
        write_debug_log("TEST_ENTRY_B");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("TEST_ENTRY_A"), "log should contain first entry");
        assert!(content.contains("TEST_ENTRY_B"), "log should contain second entry");
        assert_eq!(content.lines().count(), 2, "should have exactly 2 lines");
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_clear_debug_log_empties_file() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = std::env::temp_dir().join("applauncher-debug.log");
        // Ensure clean state: write something, then clear it
        let _ = std::fs::remove_file(&path);
        write_debug_log("WILL_BE_CLEARED");
        clear_debug_log();
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(content.is_empty(), "log should be empty after clear");
    }
}
