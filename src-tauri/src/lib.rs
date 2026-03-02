mod app_catalog;
mod calculator;
mod command_catalog;
mod file_catalog;
mod icon;
mod models;
mod search_service;
mod settings_store;
mod text_matcher;
mod web_search;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use arboard::Clipboard;
use models::{ExecutePayload, LauncherSettings, SearchResponse, SearchResultKind};
use search_service::SearchService;
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const MAIN_WINDOW_LABEL: &str = "main";
const KEEPALIVE_WINDOW_LABEL: &str = "__buscador_keepalive";
const AGGRESSIVE_IDLE_MODE: bool = true;
const FOCUS_HIDE_DEBOUNCE_MS: u64 = 140;
const FOCUS_RETRY_DELAYS_MS: [u64; 3] = [20, 75, 140];
const FOCUS_GUARD_POLL_MS: u64 = 120;
const FOCUS_GUARD_HIDE_AFTER_MS: u64 = 900;
const FOCUS_GUARD_MAX_MS: u64 = 12_000;
const FRONTEND_FOCUS_EVENT_RETRIES_MS: [u64; 2] = [90, 240];
const BLUR_CLOSE_GRACE_AFTER_SHOW_MS: u64 = 950;

struct AppState {
    search_service: Arc<SearchService>,
    icon_cache: Mutex<HashMap<String, Option<String>>>,
    window_booting: AtomicBool,
    last_show_millis: AtomicU64,
    focused_since_show: AtomicBool,
}

#[tauri::command]
fn search(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> SearchResponse {
    state
        .search_service
        .search(&query, limit.unwrap_or(10).min(24))
}

#[tauri::command]
fn search_fast(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> SearchResponse {
    state
        .search_service
        .search_fast(&query, limit.unwrap_or(10).min(24))
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> LauncherSettings {
    state.search_service.launcher_settings()
}

#[tauri::command]
fn save_settings(
    settings: LauncherSettings,
    state: tauri::State<'_, AppState>,
) -> Result<LauncherSettings, String> {
    let normalized = state.search_service.update_launcher_settings(settings);
    apply_windows_autostart_setting(normalized.start_with_windows)
        .map_err(|error| error.to_string())?;
    settings_store::save_settings(&normalized)?;
    Ok(normalized)
}

#[tauri::command]
fn reindex_files(state: tauri::State<'_, AppState>) {
    state.search_service.reindex_files();
}

#[tauri::command]
fn execute(payload: ExecutePayload) -> Result<(), String> {
    execute_payload(payload).map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_launcher(app: tauri::AppHandle) -> Result<(), String> {
    hide_main_window(&app).map_err(|error| error.to_string())
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard.set_text(text).map_err(|error| error.to_string())
}

#[tauri::command]
fn system_theme() -> String {
    detect_windows_theme().unwrap_or("dark").to_string()
}

#[tauri::command]
fn resolve_icon(path: String, state: tauri::State<'_, AppState>) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }

    if let Ok(cache) = state.icon_cache.lock() {
        if let Some(cached) = cache.get(path) {
            return cached.clone();
        }
    }

    let resolved = icon::resolve_icon(path);
    if let Ok(mut cache) = state.icon_cache.lock() {
        cache.insert(path.to_string(), resolved.clone());
    }
    resolved
}

#[tauri::command]
fn resize_launcher(app: tauri::AppHandle, height: f64) -> Result<(), String> {
    resize_main_window(&app, height).map_err(|error| error.to_string())
}

#[tauri::command]
fn request_launcher_focus(app: tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Ok(());
    };

    window
        .set_focus()
        .map_err(|error| format!("No se pudo enfocar la ventana: {error}"))?;
    window.emit("launcher-focus", ()).ok();
    Ok(())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn cursor_inside_window(window: &WebviewWindow) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let outer_position = match window.outer_position() {
            Ok(value) => value,
            Err(_) => return false,
        };
        let outer_size = match window.outer_size() {
            Ok(value) => value,
            Err(_) => return false,
        };

        let width = i32::try_from(outer_size.width).unwrap_or(i32::MAX);
        let height = i32::try_from(outer_size.height).unwrap_or(i32::MAX);
        let left = outer_position.x;
        let top = outer_position.y;
        let right = left.saturating_add(width);
        let bottom = top.saturating_add(height);

        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return false;
        }

        return cursor.x >= left && cursor.x <= right && cursor.y >= top && cursor.y <= bottom;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        false
    }
}

fn execute_payload(payload: ExecutePayload) -> Result<()> {
    match payload.kind {
        SearchResultKind::App | SearchResultKind::File => {
            open_path(&payload.primary_value)?;
        }
        SearchResultKind::Web => {
            open_url(&payload.primary_value)?;
        }
        SearchResultKind::Command => {
            let args = resolve_command_arguments(&payload.raw_query, &payload.title);
            run_command(&payload.primary_value, &args)?;
        }
        SearchResultKind::Calculation | SearchResultKind::Info => {}
    }
    Ok(())
}

fn open_path(path: &str) -> Result<()> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .context("No se pudo abrir el recurso seleccionado")?;
    Ok(())
}

fn open_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        bail!("URL web vacia");
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("rundll32")
            .arg("url.dll,FileProtocolHandler")
            .arg(trimmed)
            .spawn()
            .context("No se pudo abrir la URL en el navegador predeterminado")?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(trimmed)
            .spawn()
            .context("No se pudo abrir la URL en el navegador predeterminado")?;
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(trimmed)
            .spawn()
            .context("No se pudo abrir la URL en el navegador predeterminado")?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        open_path(trimmed)?;
    }

    Ok(())
}

fn run_command(command_path: &str, arguments: &[String]) -> Result<()> {
    let mut command = Command::new(command_path);
    command.args(arguments);
    if let Some(home) = std::env::var_os("USERPROFILE") {
        command.current_dir(home);
    }
    command
        .spawn()
        .with_context(|| format!("No se pudo ejecutar {command_path}"))?;
    Ok(())
}

fn resolve_command_arguments(raw_query: &str, command_name: &str) -> Vec<String> {
    let mut query = raw_query.trim();
    if let Some(rest) = query.strip_prefix('>') {
        query = rest.trim_start();
    }

    let Some(split_index) = query.find(char::is_whitespace) else {
        return Vec::new();
    };

    let token = &query[..split_index];
    if !token.eq_ignore_ascii_case(command_name) {
        return Vec::new();
    }

    let tail = query[split_index..].trim();
    shlex::split(tail).unwrap_or_else(|| vec![tail.to_string()])
}

fn clear_icon_cache(app: &tauri::AppHandle) {
    if let Ok(mut cache) = app.state::<AppState>().icon_cache.lock() {
        cache.clear();
    }
}

fn enter_idle_mode(app: &tauri::AppHandle) {
    clear_icon_cache(app);
    std::thread::spawn(|| {
        trim_webview_memory();
        std::thread::sleep(std::time::Duration::from_millis(320));
        trim_webview_memory();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        trim_webview_memory();
    });
}

fn attach_main_window_handlers(window: &WebviewWindow, app: &tauri::AppHandle) {
    let window_ref = window.clone();
    let app_handle = app.clone();

    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Focused(true)) {
            app_handle
                .state::<AppState>()
                .focused_since_show
                .store(true, Ordering::Release);
            return;
        }

        if matches!(event, tauri::WindowEvent::Focused(false)) {
            let window = window_ref.clone();
            let app = app_handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(FOCUS_HIDE_DEBOUNCE_MS));
                let elapsed_from_show = now_millis().saturating_sub(
                    app.state::<AppState>()
                        .last_show_millis
                        .load(Ordering::Acquire),
                );
                if elapsed_from_show < BLUR_CLOSE_GRACE_AFTER_SHOW_MS {
                    return;
                }

                let focused_once = app
                    .state::<AppState>()
                    .focused_since_show
                    .load(Ordering::Acquire);
                if !focused_once {
                    return;
                }

                let still_visible = window.is_visible().unwrap_or(false);
                let still_unfocused = !window.is_focused().unwrap_or(false);
                if still_visible && still_unfocused {
                    if cursor_inside_window(&window) {
                        return;
                    }

                    if AGGRESSIVE_IDLE_MODE {
                        window.destroy().ok();
                    } else {
                        window.hide().ok();
                    }
                    enter_idle_mode(&app);
                }
            });
        }
    });
}

fn create_main_window(app: &tauri::AppHandle) -> Result<WebviewWindow> {
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|item| item.label == MAIN_WINDOW_LABEL)
        .cloned()
        .context("Configuracion de ventana principal no disponible")?;

    let builder = tauri::WebviewWindowBuilder::from_config(app, &window_config)
        .context("No se pudo preparar la ventana principal")?
        .focused(true)
        .focusable(true)
        .accept_first_mouse(true);

    let window = builder
        .build()
        .context("No se pudo crear la ventana principal")?;

    attach_main_window_handlers(&window, app);
    Ok(window)
}

fn ensure_keepalive_window(app: &tauri::AppHandle) -> Result<()> {
    if app.get_window(KEEPALIVE_WINDOW_LABEL).is_some() {
        return Ok(());
    }

    tauri::window::WindowBuilder::new(app, KEEPALIVE_WINDOW_LABEL)
        .title("")
        .inner_size(1.0, 1.0)
        .min_inner_size(1.0, 1.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .visible(false)
        .skip_taskbar(true)
        .focusable(false)
        .build()
        .context("No se pudo crear ventana keepalive")?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle, window: &WebviewWindow) {
    app.state::<AppState>()
        .last_show_millis
        .store(now_millis(), Ordering::Release);
    app.state::<AppState>()
        .focused_since_show
        .store(false, Ordering::Release);

    window.set_focusable(true).ok();
    center_on_active_monitor(window).ok();
    window.show().ok();
    window.set_focus().ok();
    window.emit("launcher-focus", ()).ok();

    let search_service = Arc::clone(&app.state::<AppState>().search_service);
    std::thread::spawn(move || {
        search_service.refresh_apps();
    });

    for delay in FOCUS_RETRY_DELAYS_MS {
        let window_clone = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            window_clone.set_focus().ok();
        });
    }

    for delay in FRONTEND_FOCUS_EVENT_RETRIES_MS {
        let window_clone = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(delay));
            window_clone.emit("launcher-focus", ()).ok();
        });
    }

    let window_clone = window.clone();
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let started_at = Instant::now();
        let mut unfocused_since: Option<Instant> = None;
        let mut saw_focus_once = false;

        loop {
            std::thread::sleep(Duration::from_millis(FOCUS_GUARD_POLL_MS));
            if started_at.elapsed() >= Duration::from_millis(FOCUS_GUARD_MAX_MS) {
                break;
            }

            let visible = window_clone.is_visible().unwrap_or(false);
            if !visible {
                break;
            }

            let focused = window_clone.is_focused().unwrap_or(false);
            if focused {
                saw_focus_once = true;
                app_handle
                    .state::<AppState>()
                    .focused_since_show
                    .store(true, Ordering::Release);
                unfocused_since = None;
                continue;
            }

            if !saw_focus_once {
                continue;
            }

            if cursor_inside_window(&window_clone) {
                unfocused_since = None;
                continue;
            }

            let elapsed_from_show = now_millis().saturating_sub(
                app_handle
                    .state::<AppState>()
                    .last_show_millis
                    .load(Ordering::Acquire),
            );
            if elapsed_from_show < BLUR_CLOSE_GRACE_AFTER_SHOW_MS {
                continue;
            }

            let since = unfocused_since.get_or_insert_with(Instant::now);
            if since.elapsed() < Duration::from_millis(FOCUS_GUARD_HIDE_AFTER_MS) {
                continue;
            }

            if AGGRESSIVE_IDLE_MODE {
                window_clone.destroy().ok();
            } else {
                window_clone.hide().ok();
            }
            enter_idle_mode(&app_handle);
            break;
        }
    });
}

fn hide_main_window(app: &tauri::AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if AGGRESSIVE_IDLE_MODE {
            window
                .destroy()
                .context("No se pudo destruir la ventana principal")?;
        } else {
            window.hide().context("No se pudo ocultar la ventana")?;
        }
        enter_idle_mode(app);
    }
    Ok(())
}

fn resize_main_window(app: &tauri::AppHandle, height: f64) -> Result<()> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Ok(());
    };

    let scale_factor = window
        .scale_factor()
        .context("No se pudo leer el factor de escala de la ventana")?;
    let current_size = window
        .inner_size()
        .context("No se pudo leer tamano actual de la ventana")?;

    let width_logical = f64::from(current_size.width) / scale_factor;
    let target_height = height.clamp(80.0, 680.0);
    window
        .set_size(Size::Logical(LogicalSize::new(
            width_logical,
            target_height,
        )))
        .context("No se pudo cambiar el tamano de la ventana")?;
    Ok(())
}

fn toggle_main_window(app: &tauri::AppHandle) -> Result<()> {
    ensure_keepalive_window(app)?;

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let is_visible = window.is_visible().unwrap_or(false);
        if is_visible {
            hide_main_window(app)?;
        } else {
            show_main_window(app, &window);
        }
        return Ok(());
    }

    let state = app.state::<AppState>();
    if state.window_booting.swap(true, Ordering::AcqRel) {
        return Ok(());
    }

    let app_handle = app.clone();
    std::thread::spawn(move || {
        let result = create_main_window(&app_handle).map(|window| {
            show_main_window(&app_handle, &window);
        });
        if let Err(error) = result {
            log::error!("No se pudo recrear la ventana principal: {error}");
        }
        app_handle
            .state::<AppState>()
            .window_booting
            .store(false, Ordering::Release);
    });

    Ok(())
}

fn center_on_active_monitor(window: &WebviewWindow) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let outer_size = window
            .outer_size()
            .context("No se pudo leer el tamano de la ventana")?;

        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            window.center().ok();
            return Ok(());
        }

        let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        if monitor.0.is_null() {
            window.center().ok();
            return Ok(());
        }

        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
            window.center().ok();
            return Ok(());
        }

        let work = monitor_info.rcWork;
        let width = i32::try_from(outer_size.width).unwrap_or(i32::MAX);
        let height = i32::try_from(outer_size.height).unwrap_or(i32::MAX);
        let work_width = work.right - work.left;
        let work_height = work.bottom - work.top;

        let x = work.left + ((work_width - width).max(0) / 2);
        let y = work.top + ((work_height - height).max(0) / 5);
        window
            .set_position(Position::Physical(PhysicalPosition::new(x, y)))
            .context("No se pudo posicionar la ventana")?;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        window.center().ok();
        Ok(())
    }
}

fn register_hotkey(app: &tauri::AppHandle) {
    let manager = app.global_shortcut();
    let primary = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
    if manager.register(primary).is_ok() {
        log::info!("Hotkey activa: Ctrl+Space");
        return;
    }

    let fallback = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    if manager.register(fallback).is_ok() {
        log::info!("Hotkey activa: Ctrl+Shift+Space");
    } else {
        log::error!("No se pudo registrar hotkey global");
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_theme() -> Option<&'static str> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const VALUE: &str = "AppsUseLightTheme";

    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(KEY).ok()?;
    let light: u32 = key.get_value(VALUE).ok()?;
    if light == 0 {
        Some("dark")
    } else {
        Some("light")
    }
}

#[cfg(target_os = "windows")]
fn maybe_seed_windows_autostart(settings: &LauncherSettings) {
    if !settings.start_with_windows {
        return;
    }

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            log::warn!("No se pudo resolver current_exe para autostart: {error}");
            return;
        }
    };

    let exe_text = current_exe.to_string_lossy().to_string();
    let normalized = exe_text.to_lowercase().replace('/', "\\");
    if !normalized.contains("\\appdata\\local\\programs\\buscador\\") {
        return;
    }

    let seed_marker = autostart_seed_marker_path();
    if seed_marker.exists() {
        return;
    }

    match set_windows_run_autostart(&exe_text) {
        Ok(()) => {
            if let Some(parent) = seed_marker.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(seed_marker, b"seeded");
            log::info!("Autostart inicial configurado");
        }
        Err(error) => {
            log::warn!("No se pudo configurar autostart inicial: {error}");
        }
    }
}

#[cfg(target_os = "windows")]
fn set_windows_run_autostart(exe_path: &str) -> Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .context("No se pudo abrir/crear clave Run")?;

    let command = format!("\"{exe_path}\"");
    run_key
        .set_value("Buscador", &command)
        .context("No se pudo escribir valor Run para Buscador")?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn remove_windows_run_autostart() -> Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .context("No se pudo abrir/crear clave Run")?;

    match run_key.delete_value("Buscador") {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("No se pudo eliminar valor Run de Buscador"),
    }
}

#[cfg(target_os = "windows")]
fn apply_windows_autostart_setting(enabled: bool) -> Result<()> {
    if enabled {
        let exe = std::env::current_exe().context("No se pudo resolver current_exe")?;
        let exe_text = exe.to_string_lossy().to_string();
        return set_windows_run_autostart(&exe_text);
    }

    remove_windows_run_autostart()
}

#[cfg(not(target_os = "windows"))]
fn apply_windows_autostart_setting(_enabled: bool) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn autostart_seed_marker_path() -> PathBuf {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("BuscadorLauncher")
            .join("autostart.seed");
    }

    PathBuf::from("autostart.seed")
}

#[cfg(not(target_os = "windows"))]
fn maybe_seed_windows_autostart(_settings: &LauncherSettings) {}

#[cfg(not(target_os = "windows"))]
fn detect_windows_theme() -> Option<&'static str> {
    None
}

#[cfg(target_os = "windows")]
fn trim_webview_memory() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    };

    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }

    let our_pid = std::process::id();
    let snap = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(h) if !h.is_invalid() => h,
        _ => return,
    };

    let mut all_procs: Vec<(u32, u32)> = Vec::new();
    let mut webview_pids: Vec<u32> = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    unsafe {
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name_end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                let name = OsString::from_wide(&entry.szExeFile[..name_end])
                    .to_string_lossy()
                    .to_lowercase();
                all_procs.push((entry.th32ProcessID, entry.th32ParentProcessID));
                if name == "msedgewebview2.exe" {
                    webview_pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        CloseHandle(snap).ok();
    }

    let access = PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION;
    for pid in webview_pids {
        if is_descendant_of(&all_procs, pid, our_pid) {
            unsafe {
                if let Ok(handle) = OpenProcess(access, false, pid) {
                    let _ = EmptyWorkingSet(handle);
                    CloseHandle(handle).ok();
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn is_descendant_of(procs: &[(u32, u32)], mut pid: u32, ancestor: u32) -> bool {
    let mut visited = std::collections::HashSet::new();
    loop {
        let Some(&(_, parent)) = procs.iter().find(|&&(p, _)| p == pid) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        if parent == 0 || parent == pid || !visited.insert(parent) {
            return false;
        }
        pid = parent;
    }
}

#[cfg(not(target_os = "windows"))]
fn trim_webview_memory() {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let startup_settings = settings_store::load_settings();
    let startup_settings_for_setup = startup_settings.clone();

    tauri::Builder::default()
        .manage(AppState {
            search_service: Arc::new(SearchService::new(startup_settings)),
            icon_cache: Mutex::new(HashMap::new()),
            window_booting: AtomicBool::new(false),
            last_show_millis: AtomicU64::new(0),
            focused_since_show: AtomicBool::new(false),
        })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Err(error) = toggle_main_window(app) {
                            log::error!("Error toggling launcher: {error}");
                        }
                    }
                })
                .build(),
        )
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(move |app| {
            let app_handle = app.handle().clone();
            maybe_seed_windows_autostart(&startup_settings_for_setup);
            ensure_keepalive_window(&app_handle)?;

            if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                attach_main_window_handlers(&window, &app_handle);
                if AGGRESSIVE_IDLE_MODE {
                    window.destroy().ok();
                    enter_idle_mode(&app_handle);
                } else {
                    window.hide().ok();
                }
            }

            register_hotkey(&app_handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search,
            search_fast,
            get_settings,
            save_settings,
            reindex_files,
            execute,
            hide_launcher,
            copy_text,
            system_theme,
            resolve_icon,
            resize_launcher,
            request_launcher_focus
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
