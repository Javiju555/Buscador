use std::collections::BTreeMap;
use std::env;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(not(target_os = "windows"))]
use std::path::Path;

use crate::models::{SearchResult, SearchResultKind};
use crate::text_matcher::{normalize, score, split_terms};

pub struct CommandCatalog {
    commands: Vec<CommandEntry>,
}

impl CommandCatalog {
    pub fn new() -> Self {
        Self {
            commands: build_catalog(),
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

        let mut ranked: Vec<(i32, &CommandEntry)> = self
            .commands
            .iter()
            .filter_map(|entry| {
                let points = score(
                    &normalized_query,
                    &query_terms,
                    &[&entry.name_normalized, &entry.path_normalized],
                );
                if points <= 0 {
                    return None;
                }

                if entry
                    .path
                    .to_ascii_lowercase()
                    .contains("\\windows\\system32\\")
                {
                    #[cfg(target_os = "windows")]
                    {
                        points -= 34;
                    }
                }

                Some((points + 42, entry))
            })
            .collect();

        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        ranked.truncate(limit);

        ranked
            .into_iter()
            .map(|(points, entry)| SearchResult {
                kind: SearchResultKind::Command,
                title: entry.name.clone(),
                subtitle: entry.path.clone(),
                primary_value: entry.path.clone(),
                score: points,
            })
            .collect()
    }
}

#[derive(Clone)]
struct CommandEntry {
    name: String,
    name_normalized: String,
    path: String,
    path_normalized: String,
}

fn build_catalog() -> Vec<CommandEntry> {
    let path_value = env::var("PATH").unwrap_or_default();

    #[cfg(target_os = "windows")]
    let path_ext = env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM;.PS1".to_string());

    #[cfg(target_os = "windows")]
    let allowed_extensions: HashSet<String> = path_ext
        .split(';')
        .filter(|part| !part.trim().is_empty())
        .map(|extension| {
            let ext = extension.trim().to_ascii_lowercase();
            if ext.starts_with('.') {
                ext
            } else {
                format!(".{ext}")
            }
        })
        .collect();

    let mut names = BTreeMap::<String, CommandEntry>::new();

    for folder in env::split_paths(&path_value).filter(|path| path.exists()) {
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            #[cfg(target_os = "windows")]
            {
                let extension = match path.extension().and_then(|e| e.to_str()) {
                    Some(ext) => format!(".{}", ext.to_ascii_lowercase()),
                    None => continue,
                };

                if !allowed_extensions.contains(&extension) {
                    continue;
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                if !is_unix_executable(&path) {
                    continue;
                }
            }

            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };

            let path_string = path.to_string_lossy().to_string();
            names
                .entry(name.to_ascii_lowercase())
                .or_insert(CommandEntry {
                    name: name.to_string(),
                    name_normalized: normalize(name),
                    path: path_string.clone(),
                    path_normalized: normalize(&path_string),
                });
        }
    }

    names.into_values().collect()
}

#[cfg(not(target_os = "windows"))]
fn is_unix_executable(path: &Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(value) => value,
        Err(_) => return false,
    };

    metadata.permissions().mode() & 0o111 != 0
}
