use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use std::sync::RwLock;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::process::Command;

use walkdir::WalkDir;

use crate::models::{SearchResult, SearchResultKind};
use crate::text_matcher::{normalize, score, split_terms};

pub struct AppCatalog {
    apps: RwLock<Vec<AppEntry>>,
    /// Rutas personalizadas del usuario para escanear EXEs (p.ej. E:\\, E:\\Sandboxie).
    custom_exe_roots: RwLock<Vec<PathBuf>>,
}

impl AppCatalog {
    pub fn new(user_roots: &[String]) -> Self {
        let custom_roots = parse_user_roots(user_roots);
        let apps = build_catalog(&custom_roots);
        Self {
            apps: RwLock::new(apps),
            custom_exe_roots: RwLock::new(custom_roots),
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        if query.trim().is_empty() || limit == 0 {
            return vec![];
        }

        let normalized_query = normalize(query);
        if normalized_query.is_empty() {
            return vec![];
        }
        let query_terms = split_terms(&normalized_query);

        let Ok(apps) = self.apps.read() else {
            return vec![];
        };

        let mut ranked: Vec<(i32, &AppEntry)> = apps
            .iter()
            .filter_map(|entry| {
                let mut points = score(
                    &normalized_query,
                    &query_terms,
                    &[
                        &entry.name_normalized,
                        &entry.alias_normalized,
                        &entry.subtitle_normalized,
                        &entry.path_normalized,
                    ],
                );
                if points <= 0 {
                    return None;
                }

                if entry.name_normalized == normalized_query {
                    points += 960;
                } else {
                    if entry.alias_normalized == normalized_query {
                        points += 640;
                    }

                    if entry.name_normalized.starts_with(&normalized_query)
                        && entry.name_normalized.len() > normalized_query.len()
                    {
                        let extra = (entry.name_normalized.len() - normalized_query.len()) as i32;
                        points -= extra.min(44);
                    }
                }

                if entry.name_normalized.contains("visual studio code")
                    && (normalized_query.contains("vscode") || normalized_query.contains("vs code"))
                {
                    points += 230;
                }

                if entry.name_normalized.contains("visual studio")
                    && !entry.name_normalized.contains("code")
                    && normalized_query.contains("code")
                {
                    points -= 58;
                }

                Some((points + 120, entry))
            })
            .collect();

        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        ranked.truncate(limit);

        ranked
            .into_iter()
            .map(|(points, entry)| SearchResult {
                kind: SearchResultKind::App,
                title: entry.name.clone(),
                subtitle: entry.subtitle.clone(),
                primary_value: entry.path.clone(),
                score: points,
            })
            .collect()
    }

    /// Devuelve todos los entries para el grid/Launchpad
    pub fn list_all(&self) -> Vec<(String, String, String)> {
        let Ok(apps) = self.apps.read() else {
            return vec![];
        };
        apps.iter()
            .map(|e| (e.name.clone(), e.path.clone(), e.subtitle.clone()))
            .collect()
    }

    /// Re-escanea el catálogo con las mismas raíces.
    pub fn refresh(&self) {
        let roots = self
            .custom_exe_roots
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let next = build_catalog(&roots);
        if next.is_empty() {
            return;
        }
        if let Ok(mut current) = self.apps.write() {
            *current = next;
        }
    }

    /// Re-escanea el catálogo con nuevas raíces de usuario.
    pub fn refresh_with_roots(&self, user_roots: &[String]) {
        let new_roots = parse_user_roots(user_roots);
        if let Ok(mut guard) = self.custom_exe_roots.write() {
            *guard = new_roots.clone();
        }
        let next = build_catalog(&new_roots);
        if let Ok(mut current) = self.apps.write() {
            *current = next;
        }
    }
}

#[derive(Clone)]
struct AppEntry {
    name: String,
    name_normalized: String,
    alias_normalized: String,
    subtitle: String,
    subtitle_normalized: String,
    path: String,
    path_normalized: String,
}

fn build_catalog(custom_roots: &[PathBuf]) -> Vec<AppEntry> {
    #[cfg(target_os = "linux")]
    {
        let mut found = BTreeMap::<String, AppEntry>::new();
        collect_linux_desktop_entries(&mut found);
        return found.into_values().collect();
    }

    #[cfg(not(target_os = "linux"))]
    {
        let mut roots: Vec<(PathBuf, &'static str)> = vec![];
        if let Some(common_start) = env::var_os("ProgramData") {
            let mut path = PathBuf::from(common_start);
            path.push("Microsoft\\Windows\\Start Menu\\Programs");
            if path.exists() {
                roots.push((path, "Start Menu"));
            }
        }

        if let Some(user_profile) = env::var_os("APPDATA") {
            let mut path = PathBuf::from(&user_profile);
            path.push("Microsoft\\Windows\\Start Menu\\Programs");
            if path.exists() {
                roots.push((path, "Start Menu"));
            }

            let mut taskbar_pins = PathBuf::from(&user_profile);
            taskbar_pins.push("Microsoft\\Internet Explorer\\Quick Launch\\User Pinned\\TaskBar");
            if taskbar_pins.exists() {
                roots.push((taskbar_pins, "Barra de tareas"));
            }
        }

        if let Some(home) = env::var_os("USERPROFILE") {
            let mut desktop = PathBuf::from(home);
            desktop.push("Desktop");
            if desktop.exists() {
                roots.push((desktop, "Escritorio"));
            }
        }

        if let Some(public_home) = env::var_os("PUBLIC") {
            let mut desktop = PathBuf::from(public_home);
            desktop.push("Desktop");
            if desktop.exists() {
                roots.push((desktop, "Escritorio publico"));
            }
        }

        let mut found = BTreeMap::<String, AppEntry>::new();
        for (root, source_name) in roots {
            collect_root_entries(&root, source_name, &mut found);
        }
        collect_start_apps_entries(&mut found);
        collect_program_files_entries(&mut found, custom_roots);

        found.into_values().collect()
    }
}

/// Convierte las rutas configuradas por el usuario (strings) en PathBufs válidos.
fn parse_user_roots(roots: &[String]) -> Vec<PathBuf> {
    roots
        .iter()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .collect()
}

fn collect_root_entries(root: &Path, source_name: &str, found: &mut BTreeMap<String, AppEntry>) {
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if extension != "lnk" && extension != "url" && extension != "exe" {
            continue;
        }

        let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.eq_ignore_ascii_case("desktop") || name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }

        let relative_folder = path
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
            .map(|relative| relative.to_string_lossy().to_string())
            .unwrap_or_else(|| source_name.to_string());
        let subtitle = if relative_folder.is_empty() {
            source_name.to_string()
        } else {
            relative_folder
        };

        let alias = build_alias(name);
        let path_string = path.to_string_lossy().to_string();
        found
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| AppEntry {
                name: name.to_string(),
                name_normalized: normalize(name),
                alias_normalized: normalize(&alias),
                subtitle: subtitle.clone(),
                subtitle_normalized: normalize(&subtitle),
                path: path_string.clone(),
                path_normalized: normalize(&path_string),
            });
    }
}

fn collect_start_apps_entries(found: &mut BTreeMap<String, AppEntry>) {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = found;
        return;
    }

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut command = Command::new("powershell.exe");
        command.creation_flags(CREATE_NO_WINDOW).args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "Get-StartApps | Sort-Object Name | ConvertTo-Json -Compress",
        ]);

        let output = match command.output() {
            Ok(value) if value.status.success() => value,
            _ => return,
        };

        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if raw.is_empty() {
            return;
        }

        let parsed = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(value) => value,
            Err(_) => return,
        };

        let entries = match parsed {
            serde_json::Value::Array(items) => items,
            serde_json::Value::Object(_) => vec![parsed],
            _ => return,
        };

        for item in entries {
            let Some(name) = item.get("Name").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(app_id) = item.get("AppID").and_then(|value| value.as_str()) else {
                continue;
            };

            let clean_name = name.trim();
            let clean_app_id = app_id.trim();
            if clean_name.is_empty() || clean_app_id.is_empty() {
                continue;
            }

            let dedupe_key = clean_name.to_ascii_lowercase();
            found.entry(dedupe_key).or_insert_with(|| {
                let shell_path = format!("shell:AppsFolder\\{clean_app_id}");
                let alias = build_alias(clean_name);
                let app_id_normalized = normalize(clean_app_id);
                let subtitle = subtitle_for_windows_app(clean_app_id, clean_name);

                AppEntry {
                    name: clean_name.to_string(),
                    name_normalized: normalize(clean_name),
                    alias_normalized: if app_id_normalized.is_empty() {
                        normalize(&alias)
                    } else if alias.is_empty() {
                        app_id_normalized
                    } else {
                        format!("{} {}", normalize(&alias), app_id_normalized)
                    },
                    subtitle: subtitle.clone(),
                    subtitle_normalized: normalize(&subtitle),
                    path: shell_path.clone(),
                    path_normalized: normalize(&shell_path),
                }
            });
        }
    }
}

/// Escanea %ProgramFiles%, %ProgramFiles(x86)% y %LOCALAPPDATA%\Programs buscando
/// EXEs directos de aplicaciones instaladas que no tienen acceso directo en el Start Menu.
///
/// Solo escanea el primer nivel de subdirectorios dentro de cada raíz (p.ej.
/// `C:\Program Files\Sandman\Sandman.exe`) para evitar recoger helpers internos.
/// Aplica una blocklist de nombres de exe conocidos que son helpers o instaladores.
#[cfg(not(target_os = "linux"))]
fn collect_program_files_entries(found: &mut BTreeMap<String, AppEntry>, custom_roots: &[PathBuf]) {
    /// Fragmentos de nombre de exe (en minúsculas) que indican helpers/instaladores.
    const EXE_BLOCKLIST: &[&str] = &[
        "uninstall",
        "uninst",
        "setup",
        "install",
        "update",
        "updater",
        "crash",
        "crashpad",
        "helper",
        "subprocess",
        "squirrel",
        "cef",
        "launcher_helper",
        "notification_helper",
        "elevated_helper",
        "gpu_helper",
        "renderer",
        "broker",
        "handler",
        "hook",
        "injector",
        "patcher",
        "repair",
        "recovery",
        "diagnostic",
        "cleanup",
        "purge",
        "migrate",
        "register",
        "regsvr",
    ];

    let mut pf_roots: Vec<PathBuf> = vec![];

    if let Some(pf) = env::var_os("ProgramFiles") {
        pf_roots.push(PathBuf::from(pf));
    }
    if let Some(pf86) = env::var_os("ProgramFiles(x86)") {
        let path = PathBuf::from(pf86);
        if !pf_roots.contains(&path) {
            pf_roots.push(path);
        }
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        let programs = PathBuf::from(local).join("Programs");
        if programs.exists() {
            pf_roots.push(programs);
        }
    }

    for root in &pf_roots {
        if !root.exists() {
            continue;
        }

        // Iterar solo los subdirectorios directos de la raíz (profundidad 1)
        let subdir_iter = match std::fs::read_dir(root) {
            Ok(iter) => iter,
            Err(_) => continue,
        };

        for subdir_entry in subdir_iter.filter_map(Result::ok) {
            let subdir_path = subdir_entry.path();
            if !subdir_path.is_dir() {
                continue;
            }

            // Buscar EXEs en el primer nivel del subdirectorio
            let exe_iter = match std::fs::read_dir(&subdir_path) {
                Ok(iter) => iter,
                Err(_) => continue,
            };

            let app_folder_name = subdir_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            for exe_entry in exe_iter.filter_map(Result::ok) {
                let exe_path = exe_entry.path();
                if !exe_path.is_file() {
                    continue;
                }

                let ext = exe_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if ext != "exe" {
                    continue;
                }

                let stem = match exe_path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };

                // Filtrar helpers/uninstallers por nombre del exe
                let stem_lower = stem.to_ascii_lowercase();
                if EXE_BLOCKLIST
                    .iter()
                    .any(|blocked| stem_lower.contains(blocked))
                {
                    continue;
                }

                // Evitar nombres puramente numéricos o de una sola letra
                if stem.len() < 2 || stem.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }

                let dedupe_key = stem.to_ascii_lowercase();

                // Solo insertar si NO existe ya (el Start Menu tiene prioridad)
                if found.contains_key(&dedupe_key) {
                    continue;
                }

                // Usar el nombre de la carpeta como título si difiere del exe
                // (p.ej. carpeta "Sandman" + exe "Sandman.exe" → título "Sandman")
                let display_name = if app_folder_name
                    .to_ascii_lowercase()
                    .starts_with(&stem_lower)
                    || stem_lower.starts_with(&app_folder_name.to_ascii_lowercase())
                {
                    app_folder_name.clone()
                } else {
                    stem.to_string()
                };

                let path_string = exe_path.to_string_lossy().to_string();
                let alias = build_alias(&display_name);
                let subtitle = format!("Program Files · {app_folder_name}");

                found.insert(
                    dedupe_key,
                    AppEntry {
                        name: display_name.clone(),
                        name_normalized: normalize(&display_name),
                        alias_normalized: normalize(&alias),
                        subtitle: subtitle.clone(),
                        subtitle_normalized: normalize(&subtitle),
                        path: path_string.clone(),
                        path_normalized: normalize(&path_string),
                    },
                );
            }
        }
    }

    // Escanear también las rutas personalizadas del usuario (p.ej. E:\Sandboxie, E:\Games\...)
    // Profundidad: root → subcarpeta → exe (mismo patrón que Program Files)
    for custom_root in custom_roots {
        if !custom_root.exists() {
            continue;
        }

        let subdir_iter = match std::fs::read_dir(custom_root) {
            Ok(iter) => iter,
            Err(_) => continue,
        };

        for subdir_entry in subdir_iter.filter_map(Result::ok) {
            let subdir_path = subdir_entry.path();
            if !subdir_path.is_dir() {
                continue;
            }

            let app_folder_name = subdir_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let exe_iter = match std::fs::read_dir(&subdir_path) {
                Ok(iter) => iter,
                Err(_) => continue,
            };

            for exe_entry in exe_iter.filter_map(Result::ok) {
                let exe_path = exe_entry.path();
                if !exe_path.is_file() {
                    continue;
                }

                let ext = exe_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if ext != "exe" {
                    continue;
                }

                let stem = match exe_path.file_stem().and_then(|s| s.to_str()) {
                    Some(s) => s,
                    None => continue,
                };

                let stem_lower = stem.to_ascii_lowercase();
                if EXE_BLOCKLIST
                    .iter()
                    .any(|blocked| stem_lower.contains(blocked))
                {
                    continue;
                }

                if stem.len() < 2 || stem.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }

                let dedupe_key = stem.to_ascii_lowercase();
                if found.contains_key(&dedupe_key) {
                    continue;
                }

                let display_name = if app_folder_name
                    .to_ascii_lowercase()
                    .starts_with(&stem_lower)
                    || stem_lower.starts_with(&app_folder_name.to_ascii_lowercase())
                {
                    app_folder_name.clone()
                } else {
                    stem.to_string()
                };

                let path_string = exe_path.to_string_lossy().to_string();
                let alias = build_alias(&display_name);
                let root_display = custom_root
                    .to_str()
                    .unwrap_or("")
                    .trim_end_matches(['\\', '/']);
                let subtitle = format!("{root_display} · {app_folder_name}");

                found.insert(
                    dedupe_key,
                    AppEntry {
                        name: display_name.clone(),
                        name_normalized: normalize(&display_name),
                        alias_normalized: normalize(&alias),
                        subtitle: subtitle.clone(),
                        subtitle_normalized: normalize(&subtitle),
                        path: path_string.clone(),
                        path_normalized: normalize(&path_string),
                    },
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_linux_desktop_entries(found: &mut BTreeMap<String, AppEntry>) {
    let mut roots: Vec<PathBuf> = vec![PathBuf::from("/usr/share/applications")];
    if let Some(home) = env::var_os("HOME") {
        roots.push(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("applications"),
        );
    }
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("applications"));
    }

    for root in roots.into_iter().filter(|path| path.exists()) {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let is_desktop = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("desktop"));
            if !is_desktop {
                continue;
            }

            let Some((name, _exec, subtitle)) = parse_linux_desktop_entry(path) else {
                continue;
            };

            let desktop_path = path.to_string_lossy().to_string();
            let key = name.to_ascii_lowercase();
            found.entry(key).or_insert_with(|| {
                let alias = build_alias(&name);
                let subtitle_normalized = normalize(&subtitle);

                AppEntry {
                    name: name.clone(),
                    name_normalized: normalize(&name),
                    alias_normalized: normalize(&alias),
                    subtitle,
                    subtitle_normalized,
                    path: desktop_path.clone(),
                    path_normalized: normalize(&desktop_path),
                }
            });
        }
    }
}

/// Returns the language code from $LANG (e.g. "es_ES.UTF-8" → "es_ES" and "es").
#[cfg(target_os = "linux")]
fn system_lang_tags() -> (String, String) {
    let lang = std::env::var("LANG").unwrap_or_default();
    // Strip encoding: "es_ES.UTF-8" → "es_ES"
    let lang_territory = lang.split('.').next().unwrap_or("").to_string();
    // Extract language only: "es_ES" → "es"
    let lang_only = lang_territory.split('_').next().unwrap_or("").to_string();
    (lang_territory, lang_only)
}

#[cfg(target_os = "linux")]
fn parse_linux_desktop_entry(path: &Path) -> Option<(String, String, String)> {
    let text = std::fs::read_to_string(path).ok()?;

    let (lang_territory, lang_only) = system_lang_tags();

    let mut in_desktop_entry = false;
    let mut name: Option<String> = None;
    let mut name_localized: Option<String> = None; // Name[es]= or Name[es_ES]=
    let mut exec: Option<String> = None;
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

        // Localized name: Name[es_ES]= takes priority over Name[es]= over Name=
        if key.starts_with("Name[") && key.ends_with(']') {
            let locale = &key[5..key.len() - 1];
            if !value.is_empty() {
                if locale.eq_ignore_ascii_case(&lang_territory) {
                    // Exact match (es_ES) — highest priority, use immediately
                    name_localized = Some(value.to_string());
                } else if locale.eq_ignore_ascii_case(&lang_only) && name_localized.is_none() {
                    // Language-only match (es) — use if no exact match yet
                    name_localized = Some(value.to_string());
                }
            }
            continue;
        }

        if key.eq_ignore_ascii_case("Name") {
            if !value.is_empty() {
                name = Some(value.to_string());
            }
            continue;
        }

        if key.eq_ignore_ascii_case("Exec") {
            let parsed_exec = normalize_linux_exec(value);
            if !parsed_exec.is_empty() {
                exec = Some(parsed_exec);
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

    // Prefer localized name if available
    let name = name_localized.or(name)?;
    let exec = exec?;
    let subtitle = path.to_string_lossy().to_string();
    Some((name, exec, subtitle))
}

#[cfg(target_os = "linux")]
fn normalize_linux_exec(raw_exec: &str) -> String {
    let mut cleaned = raw_exec.to_string();
    for token in [
        "%f", "%F", "%u", "%U", "%i", "%c", "%k", "%d", "%D", "%n", "%N", "%v", "%m",
    ] {
        cleaned = cleaned.replace(token, "");
    }
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_alias(name: &str) -> String {
    let normalized = normalize(name);
    if normalized.contains("visual studio code") {
        return "vscode vs code code".to_string();
    }
    if normalized.contains("visual studio") {
        return "vs visual studio".to_string();
    }
    String::new()
}

fn subtitle_for_windows_app(app_id: &str, name: &str) -> String {
    let family = app_id.split('!').next().unwrap_or(app_id).trim();
    let family_prefix = family.split('_').next().unwrap_or(family).trim();

    if let Some(domain) = infer_pwa_domain(family_prefix) {
        return format!("PWA · {domain}");
    }

    let hint = if !family_prefix.is_empty() {
        humanize_identifier(family_prefix)
    } else {
        humanize_identifier(name)
    };

    if hint.is_empty() {
        "Aplicacion de Windows".to_string()
    } else {
        format!("Microsoft Store · {hint}")
    }
}

fn infer_pwa_domain(identifier: &str) -> Option<String> {
    let trimmed = strip_hex_suffix(identifier.trim()).to_ascii_lowercase();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return None;
    }

    let mut segments = trimmed.split('.').collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }

    while segments.first().is_some_and(|value| *value == "www") {
        segments.remove(0);
    }
    if segments.len() < 2 {
        return None;
    }

    let tld = segments.last().copied().unwrap_or_default();
    if !is_common_tld(tld) {
        return None;
    }

    Some(segments.join("."))
}

fn strip_hex_suffix(value: &str) -> &str {
    let Some((head, tail)) = value.rsplit_once('-') else {
        return value;
    };
    if tail.len() >= 6 && tail.chars().all(|character| character.is_ascii_hexdigit()) {
        head
    } else {
        value
    }
}

fn is_common_tld(value: &str) -> bool {
    matches!(
        value,
        "com" | "org" | "net" | "app" | "dev" | "io" | "es" | "co" | "gg" | "ai" | "tv"
    )
}

fn humanize_identifier(value: &str) -> String {
    let without_vendor_id = value
        .split_once('.')
        .and_then(|(prefix, tail)| {
            if prefix.len() >= 6
                && prefix
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            {
                Some(tail)
            } else {
                None
            }
        })
        .unwrap_or(value);

    let mut normalized = without_vendor_id.replace(['.', '-', '_'], " ");
    normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return normalized;
    }

    let mut result = String::with_capacity(normalized.len() + 4);
    let mut previous_was_lowercase = false;
    for character in normalized.chars() {
        if previous_was_lowercase && character.is_ascii_uppercase() {
            result.push(' ');
        }
        result.push(character);
        previous_was_lowercase = character.is_ascii_lowercase();
    }

    result
        .split_whitespace()
        .map(capitalize_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
