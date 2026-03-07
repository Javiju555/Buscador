pub fn resolve_icon(path: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return resolve_windows_icon(path);
    }

    #[cfg(target_os = "linux")]
    {
        return resolve_linux_icon(path);
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = path;
        None
    }
}

#[cfg(target_os = "linux")]
fn resolve_linux_icon(path: &str) -> Option<String> {
    use base64::Engine;
    use std::path::Path;

    // Si es un .desktop, extraer el nombre del icono
    let icon_name = if path.ends_with(".desktop") {
        parse_desktop_icon(path)?
    } else {
        return None;
    };

    // Si el Icon es una ruta absoluta, usar directamente
    if icon_name.starts_with('/') {
        let icon_path = Path::new(&icon_name);
        if icon_path.exists() {
            return encode_icon_file(icon_path);
        }
        return None;
    }

    // Buscar en temas XDG icon
    let search_sizes = ["48x48", "64x64", "128x128", "256x256", "32x32"];
    let icon_bases = xdg_icon_dirs();
    let extensions = ["png", "svg"];

    // Primero buscar por tamaño en hicolor
    for base in &icon_bases {
        for size in &search_sizes {
            for ext in &extensions {
                let candidate = base.join(size).join("apps").join(format!("{icon_name}.{ext}"));
                if candidate.exists() {
                    return encode_icon_file(&candidate);
                }
            }
        }
        // Luego scalable
        for ext in &extensions {
            let candidate = base.join("scalable").join("apps").join(format!("{icon_name}.{ext}"));
            if candidate.exists() {
                return encode_icon_file(&candidate);
            }
        }
    }

    // Fallback: /usr/share/pixmaps
    for ext in &extensions {
        let candidate = Path::new("/usr/share/pixmaps").join(format!("{icon_name}.{ext}"));
        if candidate.exists() {
            return encode_icon_file(&candidate);
        }
    }

    // Fallback: pixmaps sin extensión (podría ser un .xpm u otro)
    let pixmap_exact = Path::new("/usr/share/pixmaps").join(&icon_name);
    if pixmap_exact.exists() {
        return encode_icon_file(&pixmap_exact);
    }

    None
}

#[cfg(target_os = "linux")]
fn xdg_icon_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;

    let mut dirs = Vec::new();

    // Tema del usuario si existe
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data_home).join("icons").join("hicolor"));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("icons")
                .join("hicolor"),
        );
    }

    dirs.push(PathBuf::from("/usr/share/icons/hicolor"));
    dirs
}

#[cfg(target_os = "linux")]
fn parse_desktop_icon(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_desktop_entry = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("Icon") {
                let icon = value.trim();
                if !icon.is_empty() {
                    return Some(icon.to_string());
                }
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn encode_icon_file(path: &std::path::Path) -> Option<String> {
    use base64::Engine;

    let data = std::fs::read(path).ok()?;
    if data.is_empty() {
        return None;
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mime = match ext.as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "xpm" => return None, // xpm no es soportado en data URLs
        _ => "image/png",     // asumir PNG para otros
    };

    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&data)
    ))
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
