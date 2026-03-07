#[cfg(target_os = "linux")]
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "linux")]
use walkdir::WalkDir;

pub fn resolve_icon(path: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return resolve_windows_icon(path);
    }

    #[cfg(target_os = "linux")]
    {
        return resolve_linux_icon(path);
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
    {
        let _ = path;
        None
    }
}

pub fn mime_type_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(OsStr::to_str)
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

#[cfg(target_os = "linux")]
static LINUX_ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<PathBuf>>>> = OnceLock::new();

#[cfg(target_os = "linux")]
fn resolve_linux_icon(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = Path::new(trimmed);
    let resolved = if is_desktop_file(candidate) {
        resolve_desktop_entry_icon(candidate)
    } else if is_supported_icon_file(candidate) {
        Some(candidate.to_path_buf())
    } else {
        None
    }?;

    Some(build_custom_icon_url(&resolved))
}

#[cfg(target_os = "linux")]
fn resolve_desktop_entry_icon(path: &Path) -> Option<PathBuf> {
    let icon = parse_desktop_entry_icon(path)?;
    resolve_icon_token(&icon, path.parent())
}

#[cfg(target_os = "linux")]
fn parse_desktop_entry_icon(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;

    let mut in_desktop_entry = false;
    let mut icon: Option<String> = None;
    let mut no_display = false;
    let mut hidden = false;
    let mut entry_type: Option<String> = None;

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

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim();
        let value = value.trim();

        if key.eq_ignore_ascii_case("Icon") {
            if !value.is_empty() {
                icon = Some(value.to_string());
            }
            continue;
        }

        if key.eq_ignore_ascii_case("Type") {
            entry_type = Some(value.to_string());
            continue;
        }

        if key.eq_ignore_ascii_case("NoDisplay") {
            no_display = value.eq_ignore_ascii_case("true") || value == "1";
            continue;
        }

        if key.eq_ignore_ascii_case("Hidden") {
            hidden = value.eq_ignore_ascii_case("true") || value == "1";
        }
    }

    if hidden || no_display {
        return None;
    }
    if !entry_type
        .unwrap_or_else(|| "Application".to_string())
        .eq_ignore_ascii_case("Application")
    {
        return None;
    }

    icon
}

#[cfg(target_os = "linux")]
fn resolve_icon_token(icon: &str, desktop_dir: Option<&Path>) -> Option<PathBuf> {
    let trimmed = icon.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(explicit_path) = resolve_explicit_icon_path(trimmed, desktop_dir) {
        return Some(explicit_path);
    }

    let cache = LINUX_ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(trimmed) {
            return cached.clone();
        }
    }

    let resolved = search_themed_icon(trimmed);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(trimmed.to_string(), resolved.clone());
    }

    resolved
}

#[cfg(target_os = "linux")]
fn resolve_explicit_icon_path(icon: &str, desktop_dir: Option<&Path>) -> Option<PathBuf> {
    let icon_path = Path::new(icon);
    if icon_path.is_absolute() && is_supported_icon_file(icon_path) {
        return Some(icon_path.to_path_buf());
    }

    if icon.contains(std::path::MAIN_SEPARATOR) {
        let candidate = desktop_dir?.join(icon);
        if is_supported_icon_file(&candidate) {
            return Some(candidate);
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn search_themed_icon(icon: &str) -> Option<PathBuf> {
    let wanted_names = wanted_icon_names(icon);
    let mut best_match: Option<(i32, PathBuf)> = None;

    for root in linux_icon_roots() {
        let Some(found) = find_icon_in_root(&root, &wanted_names) else {
            continue;
        };

        let score = rank_icon_path(&found);
        if best_match
            .as_ref()
            .map_or(true, |(best_score, _)| score > *best_score)
        {
            best_match = Some((score, found));
        }
    }

    best_match.map(|(_, path)| path)
}

#[cfg(target_os = "linux")]
fn wanted_icon_names(icon: &str) -> HashSet<String> {
    let trimmed = icon.trim();
    let mut names = HashSet::new();
    names.insert(trimmed.to_string());

    let icon_path = Path::new(trimmed);
    if let Some(stem) = icon_path.file_stem().and_then(OsStr::to_str) {
        names.insert(stem.to_string());
    }

    if let Some(file_name) = icon_path.file_name().and_then(OsStr::to_str) {
        names.insert(file_name.to_string());
    }

    names
}

#[cfg(target_os = "linux")]
fn linux_icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("icons"));
    }

    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local").join("share").join("icons"));
        roots.push(home.join(".icons"));
        roots.push(
            home.join(".local")
                .join("share")
                .join("flatpak")
                .join("exports")
                .join("share")
                .join("icons"),
        );
    }

    roots.push(PathBuf::from("/usr/local/share/icons"));
    roots.push(PathBuf::from("/usr/share/icons/hicolor"));
    roots.push(PathBuf::from("/usr/share/icons"));
    roots.push(PathBuf::from("/usr/share/pixmaps"));
    roots.push(PathBuf::from("/var/lib/flatpak/exports/share/icons"));

    let mut dedup = HashSet::new();
    roots
        .into_iter()
        .filter(|path| path.exists())
        .filter(|path| dedup.insert(path.clone()))
        .collect()
}

#[cfg(target_os = "linux")]
fn find_icon_in_root(root: &Path, wanted_names: &HashSet<String>) -> Option<PathBuf> {
    let mut best_match: Option<(i32, PathBuf)> = None;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !is_supported_icon_file(path) {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };

        if !wanted_names.contains(file_name) && !wanted_names.contains(stem) {
            continue;
        }

        let score = rank_icon_path(path);
        if best_match
            .as_ref()
            .map_or(true, |(best_score, _)| score > *best_score)
        {
            best_match = Some((score, path.to_path_buf()));
        }
    }

    best_match.map(|(_, path)| path)
}

#[cfg(target_os = "linux")]
fn rank_icon_path(path: &Path) -> i32 {
    let extension_score = match path
        .extension()
        .and_then(OsStr::to_str)
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => 120,
        Some("svg") => 110,
        Some("webp") => 100,
        Some("jpg") | Some("jpeg") => 90,
        Some("ico") => 80,
        Some("bmp") => 70,
        _ => 0,
    };

    let path_text = path.to_string_lossy().to_ascii_lowercase();
    let symbolic_penalty = if path_text.contains("symbolic") { -90 } else { 0 };
    let app_bonus = if path_text.contains("/apps/") { 35 } else { 0 };
    let size_score = match icon_size_hint(path) {
        Some(size) => 220 - (size as i32 - 96).abs().min(180),
        None if path_text.contains("scalable") => 160,
        _ => 0,
    };

    extension_score + symbolic_penalty + app_bonus + size_score
}

#[cfg(target_os = "linux")]
fn icon_size_hint(path: &Path) -> Option<u32> {
    path.components().find_map(|component| {
        let text = component.as_os_str().to_string_lossy();
        let Some((width, height)) = text.split_once('x') else {
            return None;
        };
        if width != height {
            return None;
        }
        width.parse::<u32>().ok()
    })
}

#[cfg(target_os = "linux")]
fn build_custom_icon_url(path: &Path) -> String {
    let path_text = path.to_string_lossy();
    let encoded = urlencoding::encode(path_text.as_ref());
    format!("icon://localhost/{encoded}")
}

#[cfg(target_os = "linux")]
fn is_desktop_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("desktop"))
}

#[cfg(target_os = "linux")]
fn is_supported_icon_file(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension()
                .and_then(OsStr::to_str)
                .map(|value| value.to_ascii_lowercase())
                .as_deref(),
            Some("png" | "svg" | "jpg" | "jpeg" | "webp" | "ico" | "bmp")
        )
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
