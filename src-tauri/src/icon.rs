pub fn resolve_icon(path: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return resolve_windows_icon(path);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        None
    }
}

#[cfg(target_os = "windows")]
fn resolve_windows_icon(path: &str) -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHParseDisplayName, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_PIDL,
        SHGFI_USEFILEATTRIBUTES,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    if path.starts_with("shell:") {
        let wide_path: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        let mut shell_attributes = 0_u32;
        if unsafe {
            SHParseDisplayName(
                PCWSTR(wide_path.as_ptr()),
                None,
                &mut pidl,
                0,
                Some(&mut shell_attributes),
            )
        }
        .is_err()
            || pidl.is_null()
        {
            return None;
        }

        let mut info = SHFILEINFOW::default();
        let loaded = unsafe {
            SHGetFileInfoW(
                PCWSTR(pidl.cast()),
                FILE_ATTRIBUTE_NORMAL,
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON | SHGFI_PIDL,
            )
        };
        unsafe {
            CoTaskMemFree(Some(pidl.cast()));
        }

        if loaded == 0 || info.hIcon.0.is_null() {
            return None;
        }

        let encoded = encode_hicon_as_data_url(info.hIcon);
        unsafe {
            let _ = DestroyIcon(info.hIcon);
        }
        return encoded;
    }

    let wide_path: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
    let mut info = SHFILEINFOW::default();
    let mut flags = SHGFI_ICON | SHGFI_LARGEICON;
    if !Path::new(path).exists() {
        flags |= SHGFI_USEFILEATTRIBUTES;
    }

    let loaded = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if loaded == 0 || info.hIcon.0.is_null() {
        return None;
    }

    let encoded = encode_hicon_as_data_url(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    encoded
}

#[cfg(target_os = "windows")]
fn encode_hicon_as_data_url(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<String> {
    use windows::Win32::Graphics::Gdi::DeleteObject;
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut icon_info = ICONINFO::default();
    if unsafe { GetIconInfo(icon, &mut icon_info) }.is_err() {
        return None;
    }

    let bitmap = if !icon_info.hbmColor.0.is_null() {
        icon_info.hbmColor
    } else {
        icon_info.hbmMask
    };
    let encoded = encode_hbitmap_as_data_url(bitmap);

    unsafe {
        if !icon_info.hbmColor.0.is_null() {
            let _ = DeleteObject(icon_info.hbmColor.into());
        }
        if !icon_info.hbmMask.0.is_null() {
            let _ = DeleteObject(icon_info.hbmMask.into());
        }
    }

    encoded
}

#[cfg(target_os = "windows")]
fn encode_hbitmap_as_data_url(bitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<String> {
    use base64::Engine;
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS,
    };

    let mut details = BITMAP::default();
    let object_size = unsafe {
        GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut details as *mut _ as *mut _),
        )
    };
    if object_size == 0 {
        return None;
    }

    let width = details.bmWidth.abs();
    let height = details.bmHeight.abs();
    if width <= 0 || height <= 0 {
        return None;
    }

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut pixels = vec![0_u8; (width * height * 4) as usize];
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return None;
    }

    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(dc);
    }
    if lines == 0 {
        return None;
    }

    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    let mut png = Vec::new();
    if PngEncoder::new(&mut png)
        .write_image(
            &pixels,
            width as u32,
            height as u32,
            ExtendedColorType::Rgba8,
        )
        .is_err()
    {
        return None;
    }

    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}
