/// Splits a `"file|index"` icon reference. Returns None for a plain path.
///
/// The separator is `|`, which is illegal in Windows filenames, so it cannot
/// collide with a real path. The index is only meaningful alongside the file,
/// hence packing them into one string rather than widening every call site.
pub(crate) fn split_icon_ref(s: &str) -> Option<(String, i32)> {
    let (file, idx) = s.rsplit_once('|')?;
    if file.is_empty() { return None; }
    Some((file.to_string(), idx.parse().unwrap_or(0)))
}

pub fn get_file_icon(path: String) -> Option<String> {
    #[cfg(target_os = "windows")]
    return get_file_icon_windows(&path);
    #[cfg(not(target_os = "windows"))]
    return None;
}

#[cfg(target_os = "windows")]
fn get_file_icon_windows(path: &str) -> Option<String> {
    // SHGetFileInfoW relies on shell extension COM objects for some file
    // types' icons. Now that get_file_icon runs via spawn_blocking, multiple
    // calls can execute concurrently on fresh threads with no COM apartment
    // initialized — those icons fail silently and fall back to generic ones.
    // Same pattern already used in apps.rs's get_installed_apps for the same
    // reason. The actual logic (with its several early-return paths) is
    // split into a helper so cleanup always runs, however it exits.
    let should_uninit = unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok()
    };

    let result = get_file_icon_windows_inner(path);

    if should_uninit {
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    }

    result
}

#[cfg(target_os = "windows")]
fn get_file_icon_windows_inner(path: &str) -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShFileInfoW {
        h_icon: *mut std::ffi::c_void,
        i_icon: i32,
        dw_attributes: u32,
        sz_display_name: [u16; 260],
        sz_type_name: [u16; 80],
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BitmapInfo {
        bmi_header: BitmapInfoHeader,
        bmi_colors: [u32; 1],
    }

    extern "system" {
        fn SHGetFileInfoW(
            psz_path: *const u16,
            dw_file_attributes: u32,
            psfi: *mut ShFileInfoW,
            cb_file_info: u32,
            u_flags: u32,
        ) -> usize;
        fn DestroyIcon(h_icon: *mut std::ffi::c_void) -> i32;
        fn GetDC(hwnd: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn ReleaseDC(hwnd: *mut std::ffi::c_void, hdc: *mut std::ffi::c_void) -> i32;
        fn CreateCompatibleDC(hdc: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn DeleteDC(hdc: *mut std::ffi::c_void) -> i32;
        fn CreateDIBSection(
            hdc: *mut std::ffi::c_void,
            pbmi: *const BitmapInfo,
            usage: u32,
            ppv_bits: *mut *mut std::ffi::c_void,
            h_section: *mut std::ffi::c_void,
            offset: u32,
        ) -> *mut std::ffi::c_void;
        fn SelectObject(
            hdc: *mut std::ffi::c_void,
            h: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn DeleteObject(hgdiobj: *mut std::ffi::c_void) -> i32;
        fn DrawIconEx(
            hdc: *mut std::ffi::c_void,
            x_left: i32,
            y_top: i32,
            h_icon: *mut std::ffi::c_void,
            cx_width: i32,
            cy_width: i32,
            ist_ep_index: u32,
            h_fl_bk: *mut std::ffi::c_void,
            di_flags: u32,
        ) -> i32;
    }

    const SHGFI_ICON: u32 = 0x0000_0100;
    const SHGFI_LARGEICON: u32 = 0x0000_0000;
    const DIB_RGB_COLORS: u32 = 0;
    const DI_NORMAL: u32 = 0x0003;
    const SIZE: u32 = 32;

    // Strip any "|index" suffix — that form is only meaningful to
    // ExtractIconExW below, and SHGetFileInfoW would fail to resolve it.
    let bare_path = split_icon_ref(path).map(|(f, _)| f).unwrap_or_else(|| path.to_string());
    let wide: Vec<u16> = OsStr::new(bare_path.as_str())
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut sfi = ShFileInfoW {
        h_icon: std::ptr::null_mut(),
        i_icon: 0,
        dw_attributes: 0,
        sz_display_name: [0u16; 260],
        sz_type_name: [0u16; 80],
    };

    // "file|index" means the caller knows exactly which icon resource it wants
    // — a Start Menu shortcut's GetIconLocation, typically. SHGetFileInfoW can
    // only ever return resource 0, so an explicit index needs ExtractIconExW.
    //
    // ExtractIconExW is also the only one that can report failure: it returns a
    // count, whereas SHGetFileInfoW always hands back SOMETHING, substituting
    // the generic application glyph when the file has no icon of its own. That
    // is why every row looked "successful" while showing the same blue monitor.
    let mut h_icon: *mut std::ffi::c_void = std::ptr::null_mut();

    if let Some((icon_file, idx)) = split_icon_ref(path) {
        extern "system" {
            fn ExtractIconExW(
                lpsz_file: *const u16,
                n_icon_index: i32,
                phicon_large: *mut *mut std::ffi::c_void,
                phicon_small: *mut *mut std::ffi::c_void,
                n_icons: u32,
            ) -> u32;
        }
        let file_wide: Vec<u16> = OsStr::new(icon_file.as_str())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut large: *mut std::ffi::c_void = std::ptr::null_mut();
        let extracted = unsafe {
            ExtractIconExW(file_wide.as_ptr(), idx, &mut large, std::ptr::null_mut(), 1)
        };
        if extracted > 0 && !large.is_null() {
            h_icon = large;
        }
    }

    // No explicit reference, or extraction found nothing there: fall back to
    // asking the shell about the file itself.
    if h_icon.is_null() {
        let result = unsafe {
            SHGetFileInfoW(
                wide.as_ptr(),
                0,
                &mut sfi,
                std::mem::size_of::<ShFileInfoW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        if result == 0 || sfi.h_icon.is_null() {
            return None;
        }
        h_icon = sfi.h_icon;
    }

    let rgba_pixels = unsafe {
        // Get a reference DC from the screen so CreateCompatibleDC works correctly
        let screen_dc = GetDC(std::ptr::null_mut());
        let dc = CreateCompatibleDC(screen_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);

        if dc.is_null() {
            DestroyIcon(h_icon);
            return None;
        }

        // Create a 32-bit top-down DIB section to draw the icon into.
        // The pixel memory is zeroed by Windows (fully transparent black).
        let bmi = BitmapInfo {
            bmi_header: BitmapInfoHeader {
                bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: SIZE as i32,
                bi_height: -(SIZE as i32), // negative = top-down
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0,
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            bmi_colors: [0],
        };

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(
            dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            std::ptr::null_mut(),
            0,
        );

        if hbm.is_null() || bits_ptr.is_null() {
            DeleteDC(dc);
            DestroyIcon(h_icon);
            return None;
        }

        let old_bm = SelectObject(dc, hbm);

        // DrawIconEx handles both classic DIB icons and modern PNG-based icons
        // (Edge, Chrome, UWP apps, etc.) — unlike GetDIBits which only works
        // for the color bitmap and fails when h_bm_color is null.
        let draw_result = DrawIconEx(
            dc,
            0,
            0,
            h_icon,
            SIZE as i32,
            SIZE as i32,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        );

        let pixels = if draw_result != 0 {
            // Pixels are in BGRA order — swap B and R to get RGBA
            let mut p = vec![0u8; (SIZE * SIZE * 4) as usize];
            std::ptr::copy_nonoverlapping(bits_ptr as *const u8, p.as_mut_ptr(), p.len());
            for chunk in p.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }
            Some(p)
        } else {
            None
        };

        SelectObject(dc, old_bm);
        DeleteObject(hbm);
        DeleteDC(dc);

        pixels
    };

    unsafe { DestroyIcon(h_icon); }

    let rgba_pixels = rgba_pixels?;
    let img = image::RgbaImage::from_raw(SIZE, SIZE, rgba_pixels)?;
    let mut png_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .ok()?;

    Some(base64_encode(&png_bytes))
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(CHARS[b0 >> 2] as char);
        out.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        out.push(if chunk.len() > 1 { CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[b2 & 63] as char } else { '=' });
    }
    out
}
