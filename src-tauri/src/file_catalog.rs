use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

use walkdir::WalkDir;

use crate::models::{LauncherSettings, SearchResult, SearchResultKind};
use crate::text_matcher::{normalize, score, split_terms};

const DEFAULT_MAX_ENTRIES: usize = 25_000;
const HARD_MAX_ENTRIES: usize = 100_000;
const HARD_MIN_ENTRIES: usize = 3_000;
const MAX_DEPTH: usize = 8;

pub struct FileCatalog {
    entries: Arc<RwLock<Vec<FileEntry>>>,
    indexing: Arc<AtomicBool>,
    settings: Arc<RwLock<LauncherSettings>>,
    generation: Arc<AtomicU64>,
}

impl FileCatalog {
    pub fn new(settings: LauncherSettings) -> Self {
        let entries = Arc::new(RwLock::new(Vec::<FileEntry>::new()));
        let indexing = Arc::new(AtomicBool::new(false));
        let settings = Arc::new(RwLock::new(normalize_settings(settings)));
        let generation = Arc::new(AtomicU64::new(0));

        let catalog = Self {
            entries,
            indexing,
            settings,
            generation,
        };
        catalog.reindex();
        catalog
    }

    pub fn is_indexing(&self) -> bool {
        self.indexing.load(Ordering::Relaxed)
    }

    pub fn settings(&self) -> LauncherSettings {
        self.settings
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| default_settings())
    }

    pub fn update_settings(&self, next_settings: LauncherSettings) -> LauncherSettings {
        let normalized = normalize_settings(next_settings);
        if let Ok(mut guard) = self.settings.write() {
            *guard = normalized.clone();
        }
        self.reindex();
        normalized
    }

    pub fn reindex(&self) {
        let generation_id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.indexing.store(true, Ordering::Relaxed);

        let entries = Arc::clone(&self.entries);
        let indexing = Arc::clone(&self.indexing);
        let settings = self.settings();
        let generation = Arc::clone(&self.generation);

        thread::spawn(move || {
            build_index(entries, indexing, settings, generation, generation_id);
        });
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

        let snapshot = self
            .entries
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let mut ranked: Vec<(i32, FileEntry)> = snapshot
            .into_iter()
            .filter_map(|entry| {
                let points = score(
                    &normalized_query,
                    &query_terms,
                    &[&entry.name_normalized, &entry.path_normalized],
                );
                if points <= 0 {
                    return None;
                }
                Some((points + 26, entry))
            })
            .collect();

        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        ranked.truncate(limit);

        ranked
            .into_iter()
            .map(|(points, entry)| SearchResult {
                kind: SearchResultKind::File,
                title: entry.name,
                subtitle: entry.path.clone(),
                primary_value: entry.path,
                score: points,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    name_normalized: String,
    path: String,
    path_normalized: String,
}

fn build_index(
    entries: Arc<RwLock<Vec<FileEntry>>>,
    indexing: Arc<AtomicBool>,
    settings: LauncherSettings,
    generation: Arc<AtomicU64>,
    generation_id: u64,
) {
    let max_entries = settings.max_files.clamp(HARD_MIN_ENTRIES, HARD_MAX_ENTRIES);
    let roots = resolve_roots(&settings.roots);
    let skip_dirs = skip_dir_names();
    let skip_extensions = skip_extensions();

    let mut discovered = Vec::<FileEntry>::with_capacity(max_entries.min(16_000));

    'root: for root in roots {
        for entry in WalkDir::new(root)
            .max_depth(MAX_DEPTH)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                if entry.depth() == 0 {
                    return true;
                }
                should_keep_directory(entry.path(), &skip_dirs)
            })
            .filter_map(Result::ok)
        {
            if generation.load(Ordering::Relaxed) != generation_id {
                return;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            if should_skip_extension(path, &skip_extensions) {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };

            let path_string = path.to_string_lossy().to_string();
            discovered.push(FileEntry {
                name: file_name.to_string(),
                name_normalized: normalize(file_name),
                path_normalized: normalize(&path_string),
                path: path_string,
            });

            if discovered.len() >= max_entries {
                break 'root;
            }
        }
    }

    if generation.load(Ordering::Relaxed) != generation_id {
        return;
    }

    if let Ok(mut guard) = entries.write() {
        *guard = discovered;
    }
    indexing.store(false, Ordering::Relaxed);
}

fn normalize_settings(settings: LauncherSettings) -> LauncherSettings {
    let roots = normalize_roots(settings.roots);
    let max_files = settings.max_files.clamp(HARD_MIN_ENTRIES, HARD_MAX_ENTRIES);
    let results_limit = settings.results_limit.clamp(3, 20);
    LauncherSettings {
        start_with_windows: settings.start_with_windows,
        roots,
        max_files,
        web_provider: settings.web_provider.trim().to_ascii_lowercase(),
        web_api_key: settings.web_api_key.trim().to_string(),
        theme: settings.theme,
        semantic_roots: normalize_roots(settings.semantic_roots),
        results_limit,
    }
}

fn normalize_roots(roots: Vec<String>) -> Vec<String> {
    let mut dedup = Vec::<String>::new();
    for root in roots {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }

        let normalized = PathBuf::from(trimmed).to_string_lossy().to_string();
        if !dedup
            .iter()
            .any(|item| item.eq_ignore_ascii_case(&normalized))
        {
            dedup.push(normalized);
        }
    }
    dedup
}

fn resolve_roots(roots: &[String]) -> Vec<PathBuf> {
    let explicit_roots: Vec<PathBuf> = roots
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect();
    if !explicit_roots.is_empty() {
        return explicit_roots;
    }
    default_roots()
}

fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::<PathBuf>::new();

    #[cfg(target_os = "windows")]
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        for folder in ["Desktop", "Documents", "Downloads"] {
            let candidate = profile.join(folder);
            if candidate.exists() {
                roots.push(candidate);
            }
        }
    }

    #[cfg(target_os = "linux")]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for folder in ["Desktop", "Documents", "Downloads", "Projects"] {
            let candidate = home.join(folder);
            if candidate.exists() {
                roots.push(candidate);
            }
        }
    }

    roots
}

fn default_settings() -> LauncherSettings {
    LauncherSettings {
        start_with_windows: true,
        roots: vec![],
        max_files: DEFAULT_MAX_ENTRIES,
        web_provider: String::new(),
        web_api_key: String::new(),
        theme: "system".to_string(),
        semantic_roots: vec![],
        results_limit: 6,
    }
}

fn skip_dir_names() -> HashSet<String> {
    [
        "$recycle.bin",
        "system volume information",
        "windows",
        "program files",
        "program files (x86)",
        "programdata",
        "appdata",
        ".git",
        ".vs",
        ".vscode",
        "node_modules",
        ".venv",
        "venv",
        "bin",
        "obj",
        "target",
        "dist",
        ".cache",
    ]
    .iter()
    .map(|value| value.to_string())
    .collect()
}

fn skip_extensions() -> HashSet<String> {
    [".tmp", ".cache", ".lock", ".bak"]
        .iter()
        .map(|value| value.to_string())
        .collect()
}

fn should_keep_directory(path: &Path, skip_dir_names: &HashSet<String>) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    let lowered = name.to_ascii_lowercase();
    if lowered.starts_with('.') || skip_dir_names.contains(&lowered) {
        return false;
    }

    let Ok(_metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let attributes = _metadata.file_attributes();
        if attributes
            & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_REPARSE_POINT)
            != 0
        {
            return false;
        }
    }

    true
}

fn should_skip_extension(path: &Path, skip_extensions: &HashSet<String>) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    skip_extensions.contains(&format!(".{}", extension.to_ascii_lowercase()))
}
