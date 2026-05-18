use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct VirtualDesktop {
    pub index: u32,
    pub guid: Vec<u8>,
    pub name: String,
}

// ── COM helpers (Windows only) ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod com {
    // CLSID_VirtualDesktopManager: {AA509086-5CA9-4C25-8F95-589D3C07B48A}
    // Mixed-endian COM byte order: first DWORD + two WORDs in LE, rest in BE
    pub const CLSID: [u8; 16] = [
        0x86, 0x90, 0x50, 0xAA, 0xA9, 0x5C, 0x25, 0x4C,
        0x8F, 0x95, 0x58, 0x9D, 0x3C, 0x07, 0xB4, 0x8A,
    ];
    // IID_IVirtualDesktopManager: {A5CD92FF-29BE-454C-8D04-D82879FB3F1B}
    pub const IID: [u8; 16] = [
        0xFF, 0x92, 0xCD, 0xA5, 0xBE, 0x29, 0x4C, 0x45,
        0x8D, 0x04, 0xD8, 0x28, 0x79, 0xFB, 0x3F, 0x1B,
    ];
    pub const CLSCTX_INPROC_SERVER: u32 = 1;
    pub const COINIT_APARTMENTTHREADED: u32 = 0x2;

    #[repr(C)]
    pub struct IVdmVtbl {
        pub query_interface: *const std::ffi::c_void,
        pub add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        pub release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
        pub is_on_current: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut i32) -> i32,
        pub get_desktop_id: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *mut [u8; 16]) -> i32,
        pub move_to_desktop: unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, *const [u8; 16]) -> i32,
    }

    #[repr(C)]
    pub struct IVdm {
        pub vtbl: *const IVdmVtbl,
    }

    extern "system" {
        pub fn CoInitializeEx(reserved: *mut std::ffi::c_void, co_init: u32) -> i32;
        pub fn CoCreateInstance(
            rclsid: *const [u8; 16],
            punk_outer: *mut std::ffi::c_void,
            dwclsctx: u32,
            riid: *const [u8; 16],
            ppv: *mut *mut std::ffi::c_void,
        ) -> i32;
    }

    /// Create IVirtualDesktopManager. Returns null on failure; caller must Release.
    pub unsafe fn create_vdm() -> *mut IVdm {
        CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        let mut ptr: *mut IVdm = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID,
            &mut ptr as *mut *mut IVdm as *mut *mut std::ffi::c_void,
        );
        if hr != 0 { std::ptr::null_mut() } else { ptr }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns the 16-byte GUID of the virtual desktop the window is on, or None.
pub fn get_window_virtual_desktop(hwnd: *mut std::ffi::c_void) -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    unsafe {
        let vdm = com::create_vdm();
        if vdm.is_null() { return None; }
        let mut guid = [0u8; 16];
        let hr = ((*(*vdm).vtbl).get_desktop_id)(vdm as *mut _, hwnd, &mut guid);
        ((*(*vdm).vtbl).release)(vdm as *mut _);
        if hr != 0 { return None; }
        Some(guid.to_vec())
    }
    #[cfg(not(target_os = "windows"))]
    None
}

/// Moves a window to the virtual desktop identified by the 16-byte GUID.
/// Calls MoveWindowToDesktop directly without minimizing — the window stays
/// visible and in place; Windows moves it to the target desktop.
/// Returns true if MoveWindowToDesktop returned S_OK.
pub fn move_window_to_virtual_desktop(hwnd: *mut std::ffi::c_void, guid: &[u8]) -> bool {
    if guid.len() != 16 { return false; }
    #[cfg(target_os = "windows")]
    unsafe {
        let vdm = com::create_vdm();
        if vdm.is_null() { return false; }
        let mut arr = [0u8; 16];
        arr.copy_from_slice(guid);
        let hr = ((*(*vdm).vtbl).move_to_desktop)(vdm as *mut _, hwnd, &arr);
        ((*(*vdm).vtbl).release)(vdm as *mut _);
        hr == 0
    }
    #[cfg(not(target_os = "windows"))]
    false
}

/// Returns the list of virtual desktops from the registry, in Task View order.
pub fn get_virtual_desktops() -> Vec<VirtualDesktop> {
    #[cfg(target_os = "windows")]
    return get_virtual_desktops_windows();
    #[cfg(not(target_os = "windows"))]
    return vec![];
}

/// Returns the 16-byte GUID of the currently active virtual desktop, or None.
pub fn get_current_virtual_desktop_guid() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    return get_current_vd_windows();
    #[cfg(not(target_os = "windows"))]
    return None;
}

/// Switches to the desktop with the given 16-byte GUID using Win+Ctrl+Arrow keyboard simulation.
/// Reads the ordered desktop list from the registry to determine direction and step count.
/// Waits for the switch to complete before returning.
pub fn switch_virtual_desktop(target_guid: &[u8]) -> bool {
    if target_guid.len() != 16 { return false; }
    #[cfg(target_os = "windows")]
    return switch_vd_windows(target_guid);
    #[cfg(not(target_os = "windows"))]
    { let _ = target_guid; false }
}

#[cfg(target_os = "windows")]
fn get_current_vd_windows() -> Option<Vec<u8>> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }
    extern "system" {
        fn RegOpenKeyExW(hkey: *mut std::ffi::c_void, sub_key: *const u16, options: u32, desired: u32, result: *mut *mut std::ffi::c_void) -> i32;
        fn RegQueryValueExW(hkey: *mut std::ffi::c_void, value_name: *const u16, reserved: *mut u32, typ: *mut u32, data: *mut u8, data_size: *mut u32) -> i32;
        fn RegCloseKey(hkey: *mut std::ffi::c_void) -> i32;
    }
    const HKEY_CURRENT_USER: *mut std::ffi::c_void = 0x8000_0001usize as *mut _;
    const KEY_READ: u32 = 0x2_0019;
    unsafe {
        let sub_key = to_wide("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops");
        let value_name = to_wide("CurrentVirtualDesktop");
        let mut hkey: *mut std::ffi::c_void = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let mut buf = [0u8; 16];
        let mut size = 16u32;
        let mut typ = 0u32;
        let ret = RegQueryValueExW(hkey, value_name.as_ptr(), std::ptr::null_mut(), &mut typ, buf.as_mut_ptr(), &mut size);
        RegCloseKey(hkey);
        if ret != 0 || size != 16 { return None; }
        Some(buf.to_vec())
    }
}

#[cfg(target_os = "windows")]
fn switch_vd_windows(target_guid: &[u8]) -> bool {
    use std::thread;
    use std::time::Duration;

    let current_guid = match get_current_vd_windows() {
        Some(g) => g,
        None => return false,
    };
    if current_guid.as_slice() == target_guid { return true; }

    let desktops = get_virtual_desktops();
    let current_idx = match desktops.iter().position(|d| d.guid == current_guid) {
        Some(i) => i,
        None => return false,
    };
    let target_idx = match desktops.iter().position(|d| d.guid.as_slice() == target_guid) {
        Some(i) => i,
        None => return false,
    };
    if current_idx == target_idx { return true; }

    let (vk, steps) = if target_idx > current_idx {
        (0x27u16, target_idx - current_idx) // VK_RIGHT
    } else {
        (0x25u16, current_idx - target_idx) // VK_LEFT
    };

    for i in 0..steps {
        send_vd_key(vk);
        // Allow extra settle time on the final step
        let ms = if i + 1 == steps { 250 } else { 150 };
        thread::sleep(Duration::from_millis(ms));
    }
    true
}

// Sends Win+Ctrl+Arrow via SendInput to trigger virtual desktop switching.
// INPUT layout on x64 Windows (40 bytes total):
//   [0..4]   type = 1 (INPUT_KEYBOARD)
//   [4..8]   padding
//   [8..10]  wVk
//   [10..12] wScan = 0
//   [12..16] dwFlags
//   [16..20] time = 0
//   [20..24] padding (alignment for dwExtraInfo)
//   [24..32] dwExtraInfo = 0
//   [32..40] padding (union with MOUSEINPUT which is larger)
#[cfg(target_os = "windows")]
fn send_vd_key(vk: u16) {
    extern "system" {
        fn SendInput(n_inputs: u32, p_inputs: *const u8, cb_size: i32) -> u32;
    }
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_LWIN: u16 = 0x5B;
    const VK_CTRL: u16 = 0x11;

    fn key(vk: u16, flags: u32) -> [u8; 40] {
        let mut b = [0u8; 40];
        b[0..4].copy_from_slice(&1u32.to_le_bytes());       // INPUT_KEYBOARD
        b[8..10].copy_from_slice(&vk.to_le_bytes());
        b[12..16].copy_from_slice(&flags.to_le_bytes());
        b
    }

    let inputs = [
        key(VK_LWIN, 0),
        key(VK_CTRL, 0),
        key(vk, 0),
        key(vk, KEYEVENTF_KEYUP),
        key(VK_CTRL, KEYEVENTF_KEYUP),
        key(VK_LWIN, KEYEVENTF_KEYUP),
    ];
    let flat: Vec<u8> = inputs.iter().flat_map(|a| a.iter().copied()).collect();
    unsafe { SendInput(6, flat.as_ptr(), 40); }
}

#[cfg(target_os = "windows")]
fn get_virtual_desktops_windows() -> Vec<VirtualDesktop> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    extern "system" {
        fn RegOpenKeyExW(
            hkey: *mut std::ffi::c_void, sub_key: *const u16,
            options: u32, desired: u32, result: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn RegQueryValueExW(
            hkey: *mut std::ffi::c_void, value_name: *const u16,
            reserved: *mut u32, typ: *mut u32, data: *mut u8, data_size: *mut u32,
        ) -> i32;
        fn RegCloseKey(hkey: *mut std::ffi::c_void) -> i32;
    }

    const HKEY_CURRENT_USER: *mut std::ffi::c_void = 0x8000_0001usize as *mut _;
    const KEY_READ: u32 = 0x2_0019;

    unsafe {
        let sub_key = to_wide(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops"
        );
        let value_name = to_wide("VirtualDesktopIDs");
        let mut hkey: *mut std::ffi::c_void = std::ptr::null_mut();

        if RegOpenKeyExW(HKEY_CURRENT_USER, sub_key.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return vec![];
        }

        let mut buf = vec![0u8; 1024];
        let mut size = buf.len() as u32;
        let mut typ = 0u32;
        let ret = RegQueryValueExW(
            hkey, value_name.as_ptr(), std::ptr::null_mut(), &mut typ,
            buf.as_mut_ptr(), &mut size,
        );
        RegCloseKey(hkey);

        if ret != 0 { return vec![]; }

        // REG_BINARY: packed 16-byte GUIDs in Task View order
        buf[..size as usize]
            .chunks_exact(16)
            .enumerate()
            .map(|(i, chunk)| VirtualDesktop {
                index: (i + 1) as u32,
                guid: chunk.to_vec(),
                name: format!("Desktop {}", i + 1),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_virtual_desktops_returns_vec_without_panic() {
        let desktops = get_virtual_desktops();
        let _ = desktops.len();
    }

    #[test]
    fn test_get_window_virtual_desktop_null_hwnd_does_not_panic() {
        let _ = get_window_virtual_desktop(std::ptr::null_mut());
    }

    #[test]
    fn test_move_window_to_virtual_desktop_wrong_guid_length_is_noop() {
        move_window_to_virtual_desktop(std::ptr::null_mut(), &[1, 2, 3]);
    }

    #[test]
    fn test_virtual_desktop_desktops_have_16_byte_guids() {
        for d in get_virtual_desktops() {
            assert_eq!(d.guid.len(), 16, "Desktop {} GUID should be 16 bytes", d.index);
        }
    }
}
