//! Stable per-machine fingerprint, used to name a license activation.
//!
//! Activations used to be labelled with just `COMPUTERNAME`. That is not unique
//! — people name machines "PC", "Desktop", "Gaming-PC", and corporate imaging
//! produces collisions routinely. Since the licensing Worker reclaims a stale
//! activation by matching on that label, two machines sharing a name would
//! repeatedly deactivate each other: each activation silently kicked the other
//! machine down to the free tier. Appending a fingerprint makes the label
//! genuinely unique so that reclaim only ever matches the same physical machine.
//!
//! Preference order:
//!   1. SMBIOS system UUID — burned into the motherboard firmware, so it is
//!      unique per machine AND survives a Windows reinstall. Read directly via
//!      `GetSystemFirmwareTable`, so there is no process to spawn and no
//!      measurable cost at activation time.
//!   2. Registry `MachineGuid` — unique per Windows *installation*. Used when
//!      the firmware reports no usable UUID, which happens on cheap or
//!      virtualised hardware that ships all-zero or all-0xFF values.
//!
//! If neither is available the caller still gets a well-formed (if less useful)
//! fingerprint derived from the computer name, so activation never hard-fails
//! on fingerprinting alone.

/// FNV-1a, 64-bit.
///
/// Deliberately hand-rolled rather than using `DefaultHasher`: the std hasher is
/// randomly seeded per process, so it would produce a different fingerprint on
/// every launch and the reclaim match would never fire. This must be stable
/// forever — changing it would orphan every existing activation.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// True for UUIDs that carry no identifying information — all zero bytes or all
/// 0xFF. Some OEMs and hypervisors ship these, and every such machine would
/// otherwise share a fingerprint, recreating the collision this module exists
/// to prevent.
fn is_degenerate_uuid(uuid: &[u8]) -> bool {
    uuid.iter().all(|&b| b == 0x00) || uuid.iter().all(|&b| b == 0xFF)
}

/// Extracts the 16-byte system UUID from a raw SMBIOS table.
///
/// `data` is the table body (i.e. the caller has already skipped the 8-byte
/// `RawSMBIOSData` header). Structures are laid out as a 4-byte header —
/// type, length, 2-byte handle — followed by a formatted area of
/// `length - 4` bytes, then a string set terminated by a double NUL. The System
/// Information structure is type 1, and holds the UUID at offset 0x08.
fn parse_smbios_uuid(data: &[u8]) -> Option<[u8; 16]> {
    let mut pos = 0usize;
    while pos + 4 <= data.len() {
        let struct_type = data[pos];
        let struct_len = data[pos + 1] as usize;
        // A structure shorter than its own header means the table is malformed;
        // continuing would loop forever.
        if struct_len < 4 || pos + struct_len > data.len() {
            return None;
        }
        if struct_type == 1 && struct_len >= 0x18 {
            let start = pos + 0x08;
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&data[start..start + 16]);
            return if is_degenerate_uuid(&uuid) { None } else { Some(uuid) };
        }
        if struct_type == 127 {
            return None; // end-of-table
        }
        // Skip the formatted area, then walk the string set to the double NUL.
        let mut next = pos + struct_len;
        while next + 1 < data.len() && !(data[next] == 0 && data[next + 1] == 0) {
            next += 1;
        }
        pos = next + 2;
    }
    None
}

#[cfg(target_os = "windows")]
fn smbios_uuid() -> Option<[u8; 16]> {
    extern "system" {
        fn GetSystemFirmwareTable(
            provider: u32,
            table_id: u32,
            buffer: *mut u8,
            buffer_size: u32,
        ) -> u32;
    }
    // 'RSMB' as a big-endian four-character code, matching the C literal.
    const RSMB: u32 = 0x5253_4D42;

    unsafe {
        let needed = GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0);
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let written = GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr(), needed);
        if written == 0 || written as usize > buf.len() {
            return None;
        }
        buf.truncate(written as usize);
        // RawSMBIOSData: 4 bytes of version info, then a u32 length, then the table.
        if buf.len() <= 8 {
            return None;
        }
        let table_len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
        let end = (8 + table_len).min(buf.len());
        parse_smbios_uuid(&buf[8..end])
    }
}

#[cfg(not(target_os = "windows"))]
fn smbios_uuid() -> Option<[u8; 16]> {
    None
}

/// Reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`, which Windows
/// generates once per installation. Stable for the life of that install, but it
/// is regenerated by a Windows reinstall — hence the SMBIOS UUID being tried first.
#[cfg(target_os = "windows")]
fn machine_guid() -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    let key_path = HSTRING::from("SOFTWARE\\Microsoft\\Cryptography");
    let value_name = HSTRING::from("MachineGuid");
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, &key_path, 0, KEY_READ, &mut hkey).is_err() {
            return None;
        }
        // First call with a null buffer just reports the required size.
        let mut size: u32 = 0;
        let probe = RegQueryValueExW(hkey, &value_name, None, None, None, Some(&mut size));
        if probe.is_err() || size == 0 {
            let _ = RegCloseKey(hkey);
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let read = RegQueryValueExW(
            hkey,
            &value_name,
            None,
            None,
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if read.is_err() {
            return None;
        }
        // REG_SZ comes back as UTF-16 including its NUL terminator.
        let wide: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        let s = String::from_utf16_lossy(&wide);
        if s.trim().is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn machine_guid() -> Option<String> {
    None
}

/// Short, stable, machine-unique identifier — 8 lowercase hex characters.
///
/// Kept short because it is shown to Doug in the Lemon Squeezy dashboard beside
/// the computer name; 64 bits of FNV is ample to separate one customer's
/// handful of machines.
pub fn machine_fingerprint() -> String {
    if let Some(uuid) = smbios_uuid() {
        return format!("{:08x}", fnv1a_64(&uuid) & 0xFFFF_FFFF);
    }
    if let Some(guid) = machine_guid() {
        return format!("{:08x}", fnv1a_64(guid.as_bytes()) & 0xFFFF_FFFF);
    }
    // Last resort — no worse than the old computer-name-only behaviour.
    let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".to_string());
    format!("{:08x}", fnv1a_64(name.as_bytes()) & 0xFFFF_FFFF)
}

/// The label sent to Lemon Squeezy as `instance_name`. Human-readable so the
/// dashboard stays legible, with the fingerprint appended for uniqueness.
pub fn activation_label() -> String {
    let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Unknown PC".to_string());
    format!("{} [{}]", name, machine_fingerprint())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_is_stable_and_deterministic() {
        // Locked-in expected value: if this ever changes, every existing
        // activation label is orphaned and reclaim silently stops working.
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), fnv1a_64(b"a"));
        assert_ne!(fnv1a_64(b"a"), fnv1a_64(b"b"));
    }

    #[test]
    fn degenerate_uuids_are_rejected() {
        assert!(is_degenerate_uuid(&[0x00; 16]));
        assert!(is_degenerate_uuid(&[0xFF; 16]));
        let mut mixed = [0u8; 16];
        mixed[3] = 0x42;
        assert!(!is_degenerate_uuid(&mixed));
    }

    #[test]
    fn parses_uuid_from_a_type_1_structure() {
        // Type 1, length 0x1B, handle 0x0001, then padding out to offset 0x08
        // where the UUID begins, then a terminating double NUL for the string set.
        let mut data = vec![0u8; 0x1B];
        data[0] = 1; // type
        data[1] = 0x1B; // length
        for i in 0..16 {
            data[0x08 + i] = (i as u8) + 1;
        }
        data.push(0);
        data.push(0);
        let uuid = parse_smbios_uuid(&data).expect("should find the type 1 structure");
        assert_eq!(uuid[0], 1);
        assert_eq!(uuid[15], 16);
    }

    #[test]
    fn returns_none_for_all_zero_uuid() {
        let mut data = vec![0u8; 0x1B];
        data[0] = 1;
        data[1] = 0x1B;
        data.push(0);
        data.push(0);
        assert!(parse_smbios_uuid(&data).is_none());
    }

    #[test]
    fn malformed_table_does_not_loop_forever() {
        // A structure claiming a length shorter than its own header.
        let data = vec![1u8, 2u8, 0u8, 0u8, 0u8, 0u8];
        assert!(parse_smbios_uuid(&data).is_none());
    }

    #[test]
    fn stops_at_end_of_table_marker() {
        let data = vec![127u8, 4u8, 0u8, 0u8, 0u8, 0u8];
        assert!(parse_smbios_uuid(&data).is_none());
    }

    #[test]
    fn fingerprint_is_eight_hex_chars_and_repeatable() {
        let a = machine_fingerprint();
        let b = machine_fingerprint();
        assert_eq!(a, b, "fingerprint must not vary between calls");
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
