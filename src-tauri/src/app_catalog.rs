use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;

use walkdir::WalkDir;

use crate::models::{SearchResult, SearchResultKind};
use crate::text_matcher::{normalize, score, split_terms};

pub struct AppCatalog {
    apps: RwLock<Vec<AppEntry>>,
}

impl AppCatalog {
    pub fn new() -> Self {
        Self {
            apps: RwLock::new(build_catalog()),
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

    pub fn refresh(&self) {
        let next = build_catalog();
        if next.is_empty() {
            return;
        }

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

fn build_catalog() -> Vec<AppEntry> {
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

    found.into_values().collect()
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
        let output = match Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "Get-StartApps | Sort-Object Name | ConvertTo-Json -Compress",
            ])
            .output()
        {
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
