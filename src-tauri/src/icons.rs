pub fn get_file_icon(path: String) -> Option<String> {
    #[cfg(target_os = "windows")]
    return get_file_icon_windows(&path);
    #[cfg(not(target_os = "windows"))]
    return None;
}

#[cfg(target_os = "windows")]
fn get_file_icon_windows(path: &str) -> Option<String> {
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
    struct IconInfo {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        h_bm_mask: *mut std::ffi::c_void,
        h_bm_color: *mut std::ffi::c_void,
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

    extern "system" {
        fn SHGetFileInfoW(
            psz_path: *const u16,
            dw_file_attributes: u32,
            psfi: *mut ShFileInfoW,
            cb_file_info: u32,
            u_flags: u32,
        ) -> usize;
        fn DestroyIcon(h_icon: *mut std::ffi::c_void) -> i32;
        fn GetIconInfo(h_icon: *mut std::ffi::c_void, p_icon_info: *mut IconInfo) -> i32;
        fn CreateCompatibleDC(hdc: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GetDIBits(
            hdc: *mut std::ffi::c_void,
            hbm: *mut std::ffi::c_void,
            start: u32,
            c_lines: u32,
            lpv_bits: *mut std::ffi::c_void,
            lpbmi: *mut BitmapInfoHeader,
            usage: u32,
        ) -> i32;
        fn DeleteDC(hdc: *mut std::ffi::c_void) -> i32;
        fn DeleteObject(hgdiobj: *mut std::ffi::c_void) -> i32;
    }

    const SHGFI_ICON: u32 = 0x0000_0100;
    const DIB_RGB_COLORS: u32 = 0;
    const SIZE: u32 = 32;

    let wide: Vec<u16> = OsStr::new(path)
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

    let result = unsafe {
        SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut sfi,
            std::mem::size_of::<ShFileInfoW>() as u32,
            SHGFI_ICON,
        )
    };

    if result == 0 || sfi.h_icon.is_null() {
        return None;
    }

    let h_icon = sfi.h_icon;

    let rgba_pixels = unsafe {
        let mut icon_info = IconInfo {
            f_icon: 0,
            x_hotspot: 0,
            y_hotspot: 0,
            h_bm_mask: std::ptr::null_mut(),
            h_bm_color: std::ptr::null_mut(),
        };

        if GetIconInfo(h_icon, &mut icon_info) == 0 {
            DestroyIcon(h_icon);
            return None;
        }

        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            DestroyIcon(h_icon);
            DeleteObject(icon_info.h_bm_mask);
            if !icon_info.h_bm_color.is_null() {
                DeleteObject(icon_info.h_bm_color);
            }
            return None;
        }

        let mut bmi = BitmapInfoHeader {
            bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            bi_width: SIZE as i32,
            bi_height: -(SIZE as i32),
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: 0,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        };

        let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
        let dibits_result = GetDIBits(
            dc,
            icon_info.h_bm_color,
            0,
            SIZE,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        DeleteDC(dc);
        DeleteObject(icon_info.h_bm_mask);
        if !icon_info.h_bm_color.is_null() {
            DeleteObject(icon_info.h_bm_color);
        }

        if dibits_result == 0 {
            DestroyIcon(h_icon);
            return None;
        }

        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        pixels
    };

    unsafe { DestroyIcon(h_icon); }

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
