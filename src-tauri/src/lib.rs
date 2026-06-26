mod app_catalog;
mod calculator;
mod command_catalog;
mod embedding_engine;
mod file_catalog;
mod freq_store;
mod http_server;
mod icon;
mod indexer;
mod models;
mod search_service;
mod settings_store;
mod text_matcher;
mod vector_store;
mod web_search;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn dirs() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("buscador"))
}

use embedding_engine::EmbeddingEngine;
use vector_store::VectorStore;

use anyhow::{bail, Context, Result};
use arboard::Clipboard;
use freq_store::FreqStore;
use models::{ExecutePayload, LauncherSettings, SearchResponse, SearchResultKind};
use search_service::SearchService;
use tauri::{Emitter, LogicalSize, Manager, Size, WebviewWindow, Position, PhysicalPosition};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg(target_os = "linux")]
pub mod portal_shortcut;

const MAIN_WINDOW_LABEL: &str = "main";
const KEEPALIVE_WINDOW_LABEL: &str = "__buscador_keepalive";
const AGGRESSIVE_IDLE_MODE: bool = false;
const FOCUS_HIDE_DEBOUNCE_MS: u64 = 140;
const FOCUS_RETRY_DELAYS_MS: [u64; 1] = [50];
const FOCUS_GUARD_POLL_MS: u64 = 120;
const FOCUS_GUARD_HIDE_AFTER_MS: u64 = 900;
const FOCUS_GUARD_MAX_MS: u64 = 12_000;
const BLUR_CLOSE_GRACE_AFTER_SHOW_MS: u64 = 200;

struct AppState {
    search_service: Arc<SearchService>,
    freq_store: Arc<Mutex<FreqStore>>,
    icon_cache: Mutex<HashMap<String, Option<String>>>,
    window_booting: AtomicBool,
    show_main_window_when_ready: AtomicBool,
    main_window_crashed: AtomicBool,
    last_show_millis: AtomicU64,
    focused_since_show: AtomicBool,
    /// Motor de embeddings (Granite 97M ONNX). None si el modelo no está disponible.
    embedding_engine: Arc<Mutex<Option<EmbeddingEngine>>>,
    /// Almacén de vectores para búsqueda semántica.
    vector_store: Arc<Mutex<VectorStore>>,
    /// Shutdown sender para el HTTP server.
    http_shutdown: Mutex<Option<tokio::sync::mpsc::Sender<()>>>,
    /// Puerto del HTTP server.
    http_port: u16,
}

#[tauri::command]
fn search(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> SearchResponse {
    let limit = limit.unwrap_or(10).min(24);
    let mut response = state.search_service.search(&query, limit);

    // Las "funciones especiales" (cálculo, web, info) son independientes y tienen
    // prioridad: si la respuesta ya trae un cálculo, NO corremos vector search
    // (evita que los resultados semánticos pisen o tapen el resultado matemático).
    let has_special = response.results.iter().any(|r| {
        matches!(
            r.kind,
            SearchResultKind::Calculation | SearchResultKind::Web
        )
    });

    // Mezclar resultados semánticos si el motor está disponible y no hay función especial
    if !has_special {
        if let Ok(mut engine_guard) = state.embedding_engine.lock() {
            if let Some(ref mut engine) = *engine_guard {
                if let Ok(store_guard) = state.vector_store.lock() {
                    if let Ok(query_emb) = engine.embed(&query) {
                        if let Ok(semantic_results) = store_guard.search(&query_emb, limit, None) {
                            merge_semantic_results(&mut response, &semantic_results);
                        }
                    }
                }
            }
        }
    }

    apply_freq_bonus(&state.freq_store, &mut response);
    response
}

#[tauri::command]
fn search_fast(
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> SearchResponse {
    let mut response = state
        .search_service
        .search_fast(&query, limit.unwrap_or(10).min(24));
    apply_freq_bonus(&state.freq_store, &mut response);
    response
}

fn apply_freq_bonus(freq_store: &Arc<Mutex<FreqStore>>, response: &mut SearchResponse) {
    let Ok(store) = freq_store.lock() else { return };
    let mut changed = false;
    for result in &mut response.results {
        if matches!(
            result.kind,
            SearchResultKind::Calculation | SearchResultKind::Info
        ) {
            continue;
        }
        let bonus = store.score_bonus(&result.primary_value);
        if bonus > 0 {
            result.score = result.score.saturating_add(bonus);
            changed = true;
        }
    }
    if changed {
        // Re-sort non-special results keeping Calculation/Info at end
        let mut regular: Vec<_> = response
            .results
            .drain(..)
            .filter(|r| {
                !matches!(
                    r.kind,
                    SearchResultKind::Calculation | SearchResultKind::Info
                )
            })
            .collect();
        let special: Vec<_> = response.results.drain(..).collect();
        regular.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
        response.results = regular;
        response.results.extend(special);
    }
}

/// Mezcla resultados semánticos en la respuesta existente.
///
/// Los resultados semánticos se fusionan con los fuzzy:
/// - Si un item ya existe (por primary_value), se actualiza su score con el máximo
/// - Si es nuevo, se añade con un score basado en la similaridad
fn merge_semantic_results(
    response: &mut SearchResponse,
    semantic_results: &[vector_store::SearchResult],
) {
    use models::{SearchResult, SearchResultKind};

    // Separar las "funciones especiales" (cálculo, info/hints) ANTES de mezclar.
    // Son independientes del vector search y deben mantenerse al final intactas,
    // sin que el sort por score las pise ni las empuje fuera del límite.
    let special: Vec<SearchResult> = response
        .results
        .iter()
        .filter(|r| {
            matches!(
                r.kind,
                SearchResultKind::Calculation | SearchResultKind::Info
            )
        })
        .cloned()
        .collect();
    response.results.retain(|r| {
        !matches!(
            r.kind,
            SearchResultKind::Calculation | SearchResultKind::Info
        )
    });

    for sr in semantic_results {
        // Mapear similaridad a score entero (0..600)
        let semantic_score = (sr.similarity * 600.0) as i32;

        // Solo incluir si la similaridad es razonable (> 0.3)
        if semantic_score < 180 {
            continue;
        }

        // Buscar si ya existe en la respuesta
        let existing = response
            .results
            .iter_mut()
            .find(|r| r.primary_value == sr.item.path || r.title == sr.item.title);

        if let Some(existing) = existing {
            // Actualizar score con el máximo entre fuzzy y semántico
            existing.score = existing.score.max(semantic_score);
        } else {
            // Determinar el kind basado en el tipo del vector store
            let kind = match sr.item.kind.as_str() {
                "app" => SearchResultKind::App,
                "file" => SearchResultKind::File,
                "code" => SearchResultKind::File,
                _ => SearchResultKind::Info,
            };

            response.results.push(SearchResult {
                kind,
                title: sr.item.title.clone(),
                subtitle: sr.item.subtitle.clone(),
                primary_value: sr.item.path.clone(),
                score: semantic_score,
            });
        }
    }

    // Re-sort solo los resultados regulares por score.
    response
        .results
        .sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));

    // Re-anexar las funciones especiales al final (el frontend las extrae por kind).
    response.results.extend(special);
}

/// Búsqueda puramente semántica (sin fuzzy).
///
/// Retorna resultados con su cosine similarity.
#[tauri::command]
fn semantic_search(
    query: String,
    limit: Option<usize>,
    kind_filter: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut engine_guard = state
        .embedding_engine
        .lock()
        .map_err(|e| format!("Error leyendo embedding engine: {e}"))?;

    let engine = engine_guard
        .as_mut()
        .ok_or("Embedding engine no disponible. Verifica que el modelo esté instalado.")?;

    let store_guard = state
        .vector_store
        .lock()
        .map_err(|e| format!("Error leyendo vector store: {e}"))?;

    let query_emb = engine
        .embed(&query)
        .map_err(|e| format!("Error generando embedding: {e}"))?;

    let results = store_guard
        .search(&query_emb, limit.unwrap_or(10), kind_filter.as_deref())
        .map_err(|e| format!("Error en búsqueda semántica: {e}"))?;

    Ok(results
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.item.id,
                "kind": r.item.kind,
                "title": r.item.title,
                "subtitle": r.item.subtitle,
                "path": r.item.path,
                "similarity": r.similarity,
                "score": (r.similarity * 600.0) as i32,
            })
        })
        .collect())
}

/// Re-indexa items en el vector store.
///
/// Genera embeddings para todas las apps indexadas.
#[tauri::command]
fn reindex_vectors(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let mut engine_guard = state
        .embedding_engine
        .lock()
        .map_err(|e| format!("Error leyendo embedding engine: {e}"))?;

    let engine = engine_guard
        .as_mut()
        .ok_or("Embedding engine no disponible. Verifica que el modelo esté instalado.")?;

    // Indexar apps (name, primary path, subtitle) con más contexto para mejorar
    // el recall semántico en queries no exactas.
    let apps = state.search_service.list_apps();
    let mut items = Vec::new();

    for (name, primary_path, subtitle) in &apps {
        let searchable = build_app_searchable_text(name, subtitle, primary_path);
        let embedding = engine
            .embed(&searchable)
            .map_err(|e| format!("Error embeddeando '{}': {e}", name))?;

        items.push(vector_store::VectorItem {
            id: format!("app:{}", name.to_lowercase()),
            kind: "app".to_string(),
            title: name.clone(),
            subtitle: subtitle.clone(),
            path: primary_path.clone(),
            embedding,
            metadata: serde_json::json!({
                "semantic_text": searchable,
            })
            .to_string(),
            updated_at: chrono_now_millis(),
        });
    }

    let count = items.len();

    let store_guard = state
        .vector_store
        .lock()
        .map_err(|e| format!("Error leyendo vector store: {e}"))?;

    // Limpiar items de tipo "app" y re-insertar
    store_guard
        .remove_by_kind("app")
        .map_err(|e| format!("Error limpiando vector store: {e}"))?;

    store_guard
        .upsert_batch(&items)
        .map_err(|e| format!("Error insertando en vector store: {e}"))?;

    Ok(format!(
        "Re-indexados {} apps en vector store ({} total)",
        count,
        store_guard.count()
    ))
}

/// Retorna estadísticas del vector store.
#[tauri::command]
fn vector_store_stats(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let store_guard = state
        .vector_store
        .lock()
        .map_err(|e| format!("Error leyendo vector store: {e}"))?;

    let model_file = state
        .embedding_engine
        .lock()
        .map(|e| {
            e.as_ref()
                .and_then(|engine| engine.model_file_name().map(str::to_string))
        })
        .unwrap_or(None);

    Ok(serde_json::json!({
        "total_items": store_guard.count(),
        "apps": store_guard.count_by_kind("app"),
        "files": store_guard.count_by_kind("file"),
        "engine_available": model_file.is_some(),
        "model_file": model_file,
    }))
}

fn build_app_searchable_text(name: &str, subtitle: &str, primary_path: &str) -> String {
    let mut parts = Vec::with_capacity(3);

    if !name.trim().is_empty() {
        parts.push(name.trim());
    }
    if !subtitle.trim().is_empty() {
        parts.push(subtitle.trim());
    }
    if !primary_path.trim().is_empty() {
        parts.push(primary_path.trim());
    }

    parts.join(" | ")
}

/// Timestamp en milisegundos (helper para reindex_vectors).
fn chrono_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Retorna el puerto del HTTP server para que agentes puedan conectarse.
#[tauri::command]
fn get_http_port(state: tauri::State<'_, AppState>) -> u16 {
    state.http_port
}

#[cfg(windows)]
fn run_ps1_script(script_path: &std::path::Path) -> Result<String, String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            script_path.to_str().ok_or("Ruta de script inválida")?,
        ])
        .output()
        .map_err(|e| format!("Error ejecutando powershell: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[cfg(not(windows))]
fn run_sh_script(script_path: &std::path::Path) -> Result<String, String> {
    let output = std::process::Command::new("sh")
        .arg(script_path.to_str().ok_or("Ruta de script inválida")?)
        .output()
        .map_err(|e| format!("Error ejecutando sh: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
async fn download_embedding_model() -> Result<String, String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe_path.parent().ok_or("No se pudo obtener el directorio del ejecutable")?;

    #[cfg(windows)]
    {
        let script_path = exe_dir.join("fetch-embedding-model.ps1");
        if !script_path.exists() {
            let dev_script_path = exe_dir.join("../../../scripts/fetch-embedding-model.ps1");
            if dev_script_path.exists() {
                return run_ps1_script(&dev_script_path);
            }
            return Err(format!("No se encontró el script de descarga en: {}", script_path.display()));
        }
        run_ps1_script(&script_path)
    }

    #[cfg(not(windows))]
    {
        let script_path = exe_dir.join("fetch-embedding-model.sh");
        if !script_path.exists() {
            let dev_script_path = exe_dir.join("../../../scripts/fetch-embedding-model.sh");
            if dev_script_path.exists() {
                return run_sh_script(&dev_script_path);
            }
            return Err(format!("No se encontró el script de descarga en: {}", script_path.display()));
        }
        run_sh_script(&script_path)
    }
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
    apply_autostart_setting(normalized.start_with_windows).map_err(|error| error.to_string())?;
    settings_store::save_settings(&normalized)?;

    // Reindexar archivos semánticos en background según las carpetas configuradas.
    // No bloquea la UI: se hace en un thread con el engine y el store compartidos.
    let engine = Arc::clone(&state.embedding_engine);
    let store = Arc::clone(&state.vector_store);
    let semantic_roots = normalized.semantic_roots.clone();
    std::thread::spawn(move || {
        let roots = indexer::roots_from_settings(&semantic_roots);
        let Ok(mut engine_guard) = engine.lock() else {
            return;
        };
        let Some(engine) = engine_guard.as_mut() else {
            return;
        };
        let Ok(store_guard) = store.lock() else {
            return;
        };
        if roots.is_empty() {
            // Sin carpetas: limpiar archivos viejos del índice.
            let _ = store_guard.remove_by_kind("file");
        } else if let Err(e) = indexer::index_files(&roots, &store_guard, engine, 20_000, 10) {
            log::error!("Error reindexando archivos semánticos: {e}");
        }
    });

    Ok(normalized)
}

#[tauri::command]
fn reindex_files(state: tauri::State<'_, AppState>) {
    state.search_service.reindex_files();
}

#[tauri::command]
fn execute(payload: ExecutePayload, state: tauri::State<'_, AppState>) -> Result<(), String> {
    if matches!(
        payload.kind,
        SearchResultKind::App | SearchResultKind::Command | SearchResultKind::File
    ) {
        if let Ok(mut store) = state.freq_store.lock() {
            store.increment(&payload.primary_value);
        }
    }
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

#[derive(serde::Serialize)]
struct GridAppEntry {
    name: String,
    exec: String,
    desktop_path: String,
}

#[tauri::command]
fn get_apps(state: tauri::State<'_, AppState>) -> Vec<GridAppEntry> {
    state
        .search_service
        .list_apps()
        .into_iter()
        .map(|(name, exec, desktop_path)| GridAppEntry {
            name,
            exec,
            desktop_path,
        })
        .collect()
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
        SearchResultKind::App => {
            execute_app(&payload.primary_value)?;
        }
        SearchResultKind::File => {
            open_path(&payload.primary_value)?;
        }
        SearchResultKind::Web => {
            open_url(&payload.primary_value)?;
            #[cfg(target_os = "linux")]
            hyprland_focus_default_browser();
        }
        SearchResultKind::Command => {
            let args = resolve_command_arguments(&payload.raw_query, &payload.title);
            run_command(&payload.primary_value, &args)?;
        }
        SearchResultKind::Calculation | SearchResultKind::Info => {}
    }
    Ok(())
}

fn execute_app(target: &str) -> Result<()> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        bail!("Ruta/comando de app vacio");
    }

    if trimmed.starts_with("shell:") {
        return open_path(trimmed);
    }

    #[cfg(target_os = "linux")]
    {
        if trimmed.ends_with(".desktop") {
            return launch_desktop_entry(trimmed);
        }
    }

    if PathBuf::from(trimmed).exists() {
        return open_path(trimmed);
    }

    let parts = shlex::split(trimmed).unwrap_or_else(|| vec![trimmed.to_string()]);
    let (command, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("No se pudo resolver comando de app"))?;

    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new(command);
        cmd.args(args.to_vec());
        if let Some(home) = std::env::var_os("HOME") {
            cmd.current_dir(home);
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("No se pudo ejecutar {command}"))?;
        hyprland_focus_pid(child.id());
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    run_command(command, args)
}

#[cfg(target_os = "linux")]
fn launch_desktop_entry(desktop_path: &str) -> Result<()> {
    let text =
        std::fs::read_to_string(desktop_path).context("No se pudo leer el archivo .desktop")?;

    let mut in_desktop_entry = false;
    let mut exec_line: Option<String> = None;
    let mut terminal = false;

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
            let key = key.trim();
            let value = value.trim();
            if key.eq_ignore_ascii_case("Exec") && !value.is_empty() {
                exec_line = Some(value.to_string());
            }
            if key.eq_ignore_ascii_case("Terminal") {
                terminal = value.eq_ignore_ascii_case("true") || value == "1";
            }
        }
    }

    let raw_exec =
        exec_line.ok_or_else(|| anyhow::anyhow!("No se encontro Exec en {desktop_path}"))?;

    let mut cleaned = raw_exec.to_string();
    for token in [
        "%f", "%F", "%u", "%U", "%i", "%c", "%k", "%d", "%D", "%n", "%N", "%v", "%m",
    ] {
        cleaned = cleaned.replace(token, "");
    }
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");

    if cleaned.is_empty() {
        bail!("Exec vacio en {desktop_path}");
    }

    let parts = shlex::split(&cleaned).unwrap_or_else(|| vec![cleaned.clone()]);
    let (command, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("No se pudo parsear Exec de {desktop_path}"))?;

    let mut child = if terminal {
        Command::new("xdg-terminal-exec")
            .arg("-e")
            .arg(command)
            .args(args)
            .spawn()
            .or_else(|_| Command::new(command).args(args).spawn())
            .context("No se pudo lanzar la app de terminal")?
    } else {
        Command::new(command)
            .args(args)
            .spawn()
            .with_context(|| format!("No se pudo lanzar {command}"))?
    };

    hyprland_focus_pid(child.id());
    let _ = child.try_wait();
    Ok(())
}

/// Foco automático tras lanzar una app en Hyprland.
/// Reintenta con delays crecientes hasta que la ventana aparece (pid match).
#[cfg(target_os = "linux")]
fn hyprland_focus_pid(pid: u32) {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
        return;
    }
    std::thread::spawn(move || {
        for delay_ms in [250u64, 500, 900, 1500] {
            std::thread::sleep(Duration::from_millis(delay_ms));
            let ok = Command::new("hyprctl")
                .args(["dispatch", "focuswindow", &format!("pid:{pid}")])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return;
            }
        }
    });
}

/// Foco automático al browser por defecto tras abrir una URL en Hyprland.
/// Lee la clase WM del browser default via xdg-settings + archivo .desktop.
#[cfg(target_os = "linux")]
fn hyprland_focus_default_browser() {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_err() {
        return;
    }
    std::thread::spawn(|| {
        let wm_class = default_browser_wm_class();
        std::thread::sleep(Duration::from_millis(350));
        let _ = Command::new("hyprctl")
            .args(["dispatch", "focuswindow", &format!("class:{wm_class}")])
            .output();
    });
}

/// Resuelve la clase WM del browser predeterminado.
/// Orden: StartupWMClass en .desktop → nombre del .desktop sin extensión → "brave-browser".
#[cfg(target_os = "linux")]
fn default_browser_wm_class() -> String {
    let desktop_file = Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let desktop_name = desktop_file.trim().trim_end_matches(".desktop");

    // Buscar StartupWMClass en /usr/share/applications o ~/.local/share/applications
    let search_paths = [
        format!("/usr/share/applications/{}.desktop", desktop_name),
        format!(
            "{}/.local/share/applications/{}.desktop",
            std::env::var("HOME").unwrap_or_default(),
            desktop_name
        ),
    ];
    for path in &search_paths {
        if let Ok(contents) = std::fs::read_to_string(path) {
            for line in contents.lines() {
                if let Some(class) = line.strip_prefix("StartupWMClass=") {
                    return class.trim().to_string();
                }
            }
        }
    }

    // Fallback: nombre del .desktop (suele coincidir con la clase WM)
    if !desktop_name.is_empty() {
        return desktop_name.to_string();
    }
    "brave-browser".to_string()
}

fn open_path(path: &str) -> Result<()> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("Ruta vacia");
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(trimmed)
            .spawn()
            .context("No se pudo abrir el recurso seleccionado")?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(trimmed)
            .spawn()
            .context("No se pudo abrir el recurso seleccionado")?;
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(trimmed)
            .spawn()
            .context("No se pudo abrir el recurso seleccionado")?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        bail!("Sistema operativo no soportado para abrir rutas");
    }

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
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
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

#[cfg(target_os = "linux")]
fn attach_main_window_crash_recovery(window: &WebviewWindow, app: &tauri::AppHandle) {
    let tracked_window = window.clone();
    let app_handle = app.clone();
    let window_label = tracked_window.label().to_string();

    if let Err(error) = window.with_webview(move |webview| {
        use webkit2gtk::WebViewExt;

        let tracked_window = tracked_window.clone();
        let app_handle = app_handle.clone();
        let window_label = window_label.clone();
        webview
            .inner()
            .connect_web_process_terminated(move |_, reason| {
                let app_handle = app_handle.clone();
                let tracked_window = tracked_window.clone();
                let window_label = window_label.clone();
                let was_visible = tracked_window.is_visible().unwrap_or(false);
                let recovery_app_handle = app_handle.clone();
                log::warn!(
                    "WebKit termino para {window_label} ({reason:?}); recreando ventana principal"
                );
                let _ = app_handle.run_on_main_thread(move || {
                    recover_main_window_after_webview_crash(
                        &recovery_app_handle,
                        Some(tracked_window),
                        was_visible,
                    );
                });
            });
    }) {
        log::warn!("No se pudo enganchar recuperacion del webview en Linux: {error}");
    }
}

#[cfg(not(target_os = "linux"))]
fn attach_main_window_crash_recovery(_window: &WebviewWindow, _app: &tauri::AppHandle) {}

fn spawn_main_window(app: &tauri::AppHandle, should_show: bool, creation_delay_ms: u64) {
    let state = app.state::<AppState>();
    if should_show {
        state
            .show_main_window_when_ready
            .store(true, Ordering::Release);
    } else if !state.window_booting.load(Ordering::Acquire) {
        state
            .show_main_window_when_ready
            .store(false, Ordering::Release);
    }

    if state.window_booting.swap(true, Ordering::AcqRel) {
        return;
    }

    if !should_show {
        state
            .show_main_window_when_ready
            .store(false, Ordering::Release);
    }

    let app_handle = app.clone();
    std::thread::spawn(move || {
        if creation_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(creation_delay_ms));
        }

        let result = create_main_window(&app_handle).map(|window| {
            let show_window = app_handle
                .state::<AppState>()
                .show_main_window_when_ready
                .swap(false, Ordering::AcqRel);
            app_handle
                .state::<AppState>()
                .main_window_crashed
                .store(false, Ordering::Release);

            if show_window {
                show_main_window(&app_handle, &window);
            } else {
                window.hide().ok();
            }
        });

        if let Err(error) = result {
            log::error!("No se pudo recrear la ventana principal: {error}");
        }

        app_handle
            .state::<AppState>()
            .window_booting
            .store(false, Ordering::Release);
    });
}

fn recover_main_window_after_webview_crash(
    app: &tauri::AppHandle,
    stale_window: Option<WebviewWindow>,
    should_show: bool,
) {
    app.state::<AppState>()
        .main_window_crashed
        .store(true, Ordering::Release);

    let window = stale_window.or_else(|| app.get_webview_window(MAIN_WINDOW_LABEL));
    if let Some(window) = window {
        if let Err(error) = window.destroy() {
            log::warn!("No se pudo destruir la ventana tras caida del webview: {error}");
        }
    }

    if let Err(error) = ensure_keepalive_window(app) {
        log::warn!("No se pudo asegurar la ventana keepalive durante recuperacion: {error}");
    }

    spawn_main_window(app, should_show, 60);
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

    #[cfg(target_os = "linux")]
    {
        // For Dash to Dock (GNOME) and Wayland, PopupMenu is the most reliable hint to avoid the dock
        if let Ok(gtk_window) = window.gtk_window() {
            use gtk::prelude::GtkWindowExt;
            gtk_window.set_type_hint(gtk::gdk::WindowTypeHint::PopupMenu);
            gtk_window.set_skip_taskbar_hint(true);
            gtk_window.set_skip_pager_hint(true);
        }
    }

    attach_main_window_handlers(&window, app);
    attach_main_window_crash_recovery(&window, app);
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
    window.emit("launcher-show", ()).ok();

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

    let window_clone = window.clone();
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let started_at = Instant::now();
        let mut unfocused_since: Option<Instant> = None;

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
                app_handle
                    .state::<AppState>()
                    .focused_since_show
                    .store(true, Ordering::Release);
                unfocused_since = None;
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

    if app
        .state::<AppState>()
        .main_window_crashed
        .load(Ordering::Acquire)
    {
        recover_main_window_after_webview_crash(app, None, true);
        return Ok(());
    }

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let is_visible = window.is_visible().unwrap_or(false);
        if is_visible {
            hide_main_window(app)?;
        } else {
            show_main_window(app, &window);
        }
        return Ok(());
    }

    spawn_main_window(app, true, 0);
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
    #[cfg(target_os = "linux")]
    if is_wayland_session() {
        log::info!("Atajo global nativo omitido en Wayland; usando portal");
        return;
    }

    let manager = app.global_shortcut();
    let primary = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
    if manager.register(primary).is_ok() {
        log::info!("Hotkey activa: Ctrl+Space (Tauri default)");
        return;
    }

    let fallback = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    if manager.register(fallback).is_ok() {
        log::info!("Hotkey activa: Ctrl+Shift+Space (Tauri default)");
    } else {
        log::error!("No se pudo registrar hotkey global nativa");
    }
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland")
}

#[cfg(target_os = "linux")]
fn register_wayland_portal_hotkey(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    portal_shortcut::spawn_portal_shortcut_listener(move || {
        let _ = toggle_main_window(&app_handle);
    });
}

#[cfg(target_os = "linux")]
fn register_toggle_socket(app: &tauri::AppHandle) {
    let app_handle = app.clone();
    std::thread::Builder::new()
        .name("toggle-socket".into())
        .spawn(move || {
            use std::io::Read;
            use std::os::unix::net::UnixListener;

            let socket_path = linux_toggle_socket_path();
            if let Some(parent) = socket_path.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    log::error!("No se pudo crear carpeta para socket toggle: {error}");
                    return;
                }
            }

            let _ = std::fs::remove_file(&socket_path);
            let listener = match UnixListener::bind(&socket_path) {
                Ok(listener) => listener,
                Err(error) => {
                    log::error!("No se pudo crear socket toggle {socket_path:?}: {error}");
                    return;
                }
            };

            log::info!("Socket toggle escuchando en {}", socket_path.display());

            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buffer = [0_u8; 32];
                        let _ = stream.read(&mut buffer);
                        log::info!("Solicitud recibida en socket toggle");
                        if let Err(error) = toggle_main_window(&app_handle) {
                            log::error!("Error toggling launcher via socket: {error}");
                        }
                    }
                    Err(error) => {
                        log::error!("Error aceptando conexion de socket toggle: {error}");
                    }
                }
            }
        })
        .ok();
}

#[cfg(target_os = "linux")]
fn linux_toggle_socket_path() -> std::path::PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(runtime_dir).join("com.buscador.launcher.sock");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join(".cache/com.buscador.launcher.sock");
    }

    std::path::PathBuf::from("/tmp/com.buscador.launcher.sock")
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
fn maybe_seed_autostart(settings: &LauncherSettings) {
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
fn apply_autostart_setting(enabled: bool) -> Result<()> {
    if enabled {
        let exe = std::env::current_exe().context("No se pudo resolver current_exe")?;
        let exe_text = exe.to_string_lossy().to_string();
        return set_windows_run_autostart(&exe_text);
    }

    remove_windows_run_autostart()
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

#[cfg(target_os = "linux")]
fn maybe_seed_autostart(settings: &LauncherSettings) {
    if !settings.start_with_windows {
        return;
    }

    if linux_autostart_entry_path().exists() {
        return;
    }

    if let Err(error) = set_linux_autostart() {
        log::warn!("No se pudo configurar autostart inicial en Linux: {error}");
    }
}

#[cfg(target_os = "linux")]
fn apply_autostart_setting(enabled: bool) -> Result<()> {
    if enabled {
        return set_linux_autostart();
    }

    remove_linux_autostart()
}

#[cfg(target_os = "linux")]
fn set_linux_autostart() -> Result<()> {
    let exe = std::env::current_exe().context("No se pudo resolver current_exe")?;
    let desktop_entry = format!(
        "[Desktop Entry]\nType=Application\nName=Buscador\nExec=\"{}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\nStartupWMClass=com.buscador.launcher\n",
        exe.to_string_lossy()
    );

    let path = linux_autostart_entry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("No se pudo crear carpeta de autostart Linux")?;
    }
    remove_linux_legacy_autostart().ok();
    std::fs::write(path, desktop_entry).context("No se pudo escribir entrada autostart Linux")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_linux_autostart() -> Result<()> {
    remove_linux_legacy_autostart().ok();
    let path = linux_autostart_entry_path();
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("No se pudo eliminar entrada autostart Linux"),
    }
}

#[cfg(target_os = "linux")]
fn linux_autostart_entry_path() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("autostart")
            .join("com.buscador.launcher.desktop");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("autostart")
            .join("com.buscador.launcher.desktop");
    }

    PathBuf::from("com.buscador.launcher.desktop")
}

#[cfg(target_os = "linux")]
fn remove_linux_legacy_autostart() -> Result<()> {
    let path = linux_legacy_autostart_entry_path();
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("No se pudo eliminar entrada autostart Linux legacy"),
    }
}

#[cfg(target_os = "linux")]
fn linux_legacy_autostart_entry_path() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("autostart")
            .join("buscador.desktop");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("autostart")
            .join("buscador.desktop");
    }

    PathBuf::from("buscador.desktop")
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn maybe_seed_autostart(_settings: &LauncherSettings) {}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn apply_autostart_setting(_enabled: bool) -> Result<()> {
    Ok(())
}

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

    // Inicializar vector store
    let vector_db_path = dirs()
        .map(|d| d.join("vectors.db"))
        .unwrap_or_else(|| PathBuf::from("vectors.db"));

    let vector_store = Arc::new(Mutex::new(
        vector_store::VectorStore::open(&vector_db_path).unwrap_or_else(|e| {
            log::error!("Error abriendo vector store: {e}. Usando DB temporal.");
            vector_store::VectorStore::open(&PathBuf::from(":memory:")).unwrap()
        }),
    ));

    // Inicializar embedding engine (puede fallar si el modelo no está)
    let model_dir = dirs()
        .map(|d| d.join("models").join("granite-embedding-97m"))
        .unwrap_or_else(|| PathBuf::from("models/granite-embedding-97m"));

    let embedding_engine = Arc::new(Mutex::new(
        match embedding_engine::EmbeddingEngine::new(&model_dir) {
            Ok(engine) => {
                log::info!("Embedding engine cargado correctamente");
                Some(engine)
            }
            Err(e) => {
                log::warn!(
                    "Embedding engine no disponible: {e}. Búsqueda semántica deshabilitada."
                );
                None
            }
        },
    ));

    // Crear SearchService primero para usarlo en AppState y HTTP server
    let search_service = Arc::new(SearchService::new(startup_settings));

    // Puerto del HTTP server (configurable via variable de entorno)
    let http_port: u16 = std::env::var("BUSCADOR_HTTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8755);

    // Preparar closures para el HTTP server
    let search_service_http = Arc::clone(&search_service);
    let search_service_apps = Arc::clone(&search_service);
    let search_service_roots = Arc::clone(&search_service);

    // Iniciar HTTP server
    let http_state = http_server::HttpState {
        search_fn: Arc::new(move |query: &str, limit: usize| {
            search_service_http.search(query, limit).results
        }),
        list_apps_fn: Arc::new(move || search_service_apps.list_apps()),
        semantic_roots_fn: Arc::new(move || {
            search_service_roots.launcher_settings().semantic_roots
        }),
        embedding_engine: Arc::clone(&embedding_engine),
        vector_store: Arc::clone(&vector_store),
    };

    let http_shutdown = http_server::start_http_server(http_state, http_port);

    let app = tauri::Builder::default()
        .manage(AppState {
            search_service: Arc::clone(&search_service),
            freq_store: Arc::new(Mutex::new(FreqStore::load())),
            icon_cache: Mutex::new(HashMap::new()),
            window_booting: AtomicBool::new(false),
            show_main_window_when_ready: AtomicBool::new(false),
            main_window_crashed: AtomicBool::new(false),
            last_show_millis: AtomicU64::new(0),
            focused_since_show: AtomicBool::new(false),
            embedding_engine,
            vector_store,
            http_shutdown: Mutex::new(Some(http_shutdown)),
            http_port,
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
        .register_uri_scheme_protocol("icon", move |_app_handle, request| {
            let path_str = request.uri().path().trim_start_matches('/');
            let decoded =
                urlencoding::decode(path_str).unwrap_or(std::borrow::Cow::Borrowed(path_str));

            let path = std::path::PathBuf::from(decoded.as_ref());
            if path.exists() {
                if let Ok(bytes) = std::fs::read(&path) {
                    return tauri::http::Response::builder()
                        .header("Access-Control-Allow-Origin", "*")
                        .header("Content-Type", icon::mime_type_for_path(&path))
                        .body(bytes)
                        .unwrap();
                }
            }

            tauri::http::Response::builder()
                .status(404)
                .body(Vec::new())
                .unwrap()
        })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            maybe_seed_autostart(&startup_settings_for_setup);
            ensure_keepalive_window(&app_handle)?;

            if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                attach_main_window_handlers(&window, &app_handle);
                attach_main_window_crash_recovery(&window, &app_handle);
                if AGGRESSIVE_IDLE_MODE {
                    window.destroy().ok();
                    enter_idle_mode(&app_handle);
                } else {
                    window.hide().ok();
                }
            }

            // Register standard global shortcut (X11 / Windows / Mac)
            register_hotkey(&app_handle);

            // Register Wayland specific portal shortcut for GNOME
            #[cfg(target_os = "linux")]
            register_wayland_portal_hotkey(&app_handle);

            #[cfg(target_os = "linux")]
            register_toggle_socket(&app_handle);

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
            request_launcher_focus,
            get_apps,
            semantic_search,
            reindex_vectors,
            vector_store_stats,
            get_http_port,
            download_embedding_model
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            api.prevent_exit();
        }
    });
}
