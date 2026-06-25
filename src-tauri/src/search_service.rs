use std::borrow::Cow;
use std::path::Path;

use crate::app_catalog::AppCatalog;
use crate::calculator::evaluate;
use crate::command_catalog::CommandCatalog;
use crate::file_catalog::FileCatalog;
use crate::models::{LauncherSettings, SearchResponse, SearchResult, SearchResultKind};
use crate::web_search::WebSearchService;

pub struct SearchService {
    app_catalog: AppCatalog,
    command_catalog: CommandCatalog,
    file_catalog: FileCatalog,
    web_search: WebSearchService,
}

impl SearchService {
    pub fn new(settings: LauncherSettings) -> Self {
        Self {
            app_catalog: AppCatalog::new(),
            command_catalog: CommandCatalog::new(),
            file_catalog: FileCatalog::new(settings),
            web_search: WebSearchService::new(),
        }
    }

    pub fn launcher_settings(&self) -> LauncherSettings {
        self.file_catalog.settings()
    }

    pub fn update_launcher_settings(&self, settings: LauncherSettings) -> LauncherSettings {
        self.file_catalog.update_settings(settings)
    }

    pub fn reindex_files(&self) {
        self.file_catalog.reindex();
    }

    pub fn refresh_apps(&self) {
        self.app_catalog.refresh();
    }

    /// Lista todas las apps para el grid/Launchpad: (name, primary_path, subtitle)
    pub fn list_apps(&self) -> Vec<(String, String, String)> {
        self.app_catalog.list_all()
    }

    pub fn search(&self, raw_query: &str, limit: usize) -> SearchResponse {
        self.search_internal(raw_query, limit, true)
    }

    pub fn search_fast(&self, raw_query: &str, limit: usize) -> SearchResponse {
        self.search_internal(raw_query, limit, false)
    }

    fn search_internal(
        &self,
        raw_query: &str,
        limit: usize,
        include_files_in_mixed: bool,
    ) -> SearchResponse {
        let (mode, query) = parse_mode(raw_query);
        if query.trim().is_empty() {
            return SearchResponse {
                results: vec![],
                file_indexing: self.file_catalog.is_indexing(),
            };
        }

        if mode == SearchMode::Calculation {
            let mut results = build_calculation(&query, true);
            let remaining = limit.saturating_sub(results.len());
            if remaining > 0 {
                results.extend(build_math_autocomplete(&query, remaining, true));
            }
            return SearchResponse {
                results,
                file_indexing: self.file_catalog.is_indexing(),
            };
        }

        if mode == SearchMode::Web {
            let settings = self.file_catalog.settings();
            return SearchResponse {
                results: build_web_results(
                    &query,
                    limit,
                    &self.web_search,
                    &settings.web_provider,
                    &settings.web_api_key,
                ),
                file_indexing: self.file_catalog.is_indexing(),
            };
        }

        // Temporary placeholder — replaced in Task 4
        if mode == SearchMode::Path {
            return SearchResponse {
                results: vec![],
                file_indexing: false,
            };
        }

        let env_alias = if mode == SearchMode::Mixed || mode == SearchMode::File {
            build_special_path_alias_result(&query)
        } else {
            None
        };

        let is_math = mode == SearchMode::Mixed && looks_like_math(&query);

        let calculation = if is_math {
            build_calculation(&query, false).into_iter().next()
        } else {
            None
        };

        let mut math_hints = if is_math {
            build_math_autocomplete(&query, 3, false)
        } else {
            vec![]
        };

        let mut bag: Vec<SearchResult> = vec![];
        if mode == SearchMode::Mixed {
            bag.extend(self.app_catalog.search(&query, limit));
        }
        // Skip system commands when the query looks like a math expression —
        // they match on numeric substrings (e.g. "mpg123" for "23+23") and add noise.
        if (mode == SearchMode::Mixed && !is_math) || mode == SearchMode::Command {
            bag.extend(self.command_catalog.search(&query, limit));
        }
        if mode == SearchMode::File || (mode == SearchMode::Mixed && include_files_in_mixed) {
            bag.extend(self.file_catalog.search(&query, limit));
        }

        if let Some(alias_result) = env_alias {
            bag.push(alias_result);
        }

        bag.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));

        let reserve_for_calc = usize::from(calculation.is_some());
        let reserve_for_hints = math_hints.len();
        let non_calc_limit = limit.saturating_sub(reserve_for_calc + reserve_for_hints);
        bag.truncate(non_calc_limit);

        bag.append(&mut math_hints);

        if let Some(calc) = calculation {
            bag.push(calc);
        }

        SearchResponse {
            results: bag,
            file_indexing: self.file_catalog.is_indexing(),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
enum SearchMode {
    Mixed,
    Command,
    File,
    Path,
    Web,
    Calculation,
}

fn parse_mode(raw_query: &str) -> (SearchMode, String) {
    let query = raw_query.trim();
    if let Some(stripped) = query.strip_prefix("w ") {
        return (SearchMode::Web, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix("w:") {
        return (SearchMode::Web, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix('>') {
        return (SearchMode::Command, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix('=') {
        return (SearchMode::Calculation, stripped.trim().to_string());
    }

    // Absolute path detection — checked before File mode
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if query.starts_with('/') {
        let after_slash = &query[1..];
        if after_slash.contains('/') || std::path::Path::new(query).exists() {
            return (SearchMode::Path, query.to_string());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let bytes = query.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return (SearchMode::Path, query.to_string());
        }
    }

    if let Some(stripped) = query.strip_prefix('/') {
        return (SearchMode::File, stripped.trim().to_string());
    }
    (SearchMode::Mixed, query.to_string())
}

fn build_web_results(
    query: &str,
    limit: usize,
    web_search: &WebSearchService,
    provider: &str,
    api_key: &str,
) -> Vec<SearchResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return vec![];
    }

    let total_rows = limit.min(6).max(1);
    let top_hits_limit = total_rows.saturating_sub(1).min(5);
    let encoded = encode_query_component(trimmed);
    let url = format!("https://www.bing.com/search?q={encoded}");

    let mut results: Vec<SearchResult> = web_search
        .search(trimmed, top_hits_limit, provider, api_key)
        .into_iter()
        .map(|item| SearchResult {
            kind: SearchResultKind::Web,
            title: item.title,
            subtitle: if item.snippet.is_empty() {
                "Enter para abrir en navegador predeterminado".to_string()
            } else {
                item.snippet
            },
            primary_value: item.url,
            score: 620,
        })
        .collect();

    results.push(SearchResult {
        kind: SearchResultKind::Web,
        title: format!("Abrir busqueda en navegador: {trimmed}"),
        subtitle: "Busca directamente en el motor predeterminado del launcher".to_string(),
        primary_value: url,
        score: 500,
    });

    results.truncate(total_rows);
    results
}

fn build_special_path_alias_result(query: &str) -> Option<SearchResult> {
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = query;
        return None;
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let (title, resolved_path) = resolve_special_path(query)?;
        let normalized = resolved_path.trim();
        let path = Path::new(normalized);
        if normalized.is_empty() || !path.exists() {
            return None;
        }

        Some(SearchResult {
            kind: SearchResultKind::File,
            title,
            subtitle: "Alias/variable especial de Windows (Enter para abrir carpeta)".to_string(),
            primary_value: normalized.to_string(),
            score: 1500,
        })
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn resolve_special_path(query: &str) -> Option<(String, String)> {
    #[cfg(target_os = "windows")]
    {
        return resolve_windows_special_path(query);
    }

    #[cfg(target_os = "linux")]
    {
        return resolve_linux_special_path(query);
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "windows")]
fn resolve_windows_special_path(query: &str) -> Option<(String, String)> {
    if let Some(token) = parse_windows_env_token(query) {
        let resolved = std::env::var(&token).ok()?;
        return Some((format!("%{}%", token.to_ascii_lowercase()), resolved));
    }

    let alias = query.trim().to_ascii_lowercase();
    match alias.as_str() {
        "appdata" => resolve_from_env_alias("APPDATA", "appdata"),
        "localappdata" => resolve_from_env_alias("LOCALAPPDATA", "localappdata"),
        "temp" | "tmp" => std::env::var("TEMP")
            .ok()
            .or_else(|| std::env::var("TMP").ok())
            .map(|path| ("temp".to_string(), path)),
        "userprofile" | "home" => resolve_from_env_alias("USERPROFILE", "userprofile"),
        "programdata" => resolve_from_env_alias("PROGRAMDATA", "programdata"),
        "windir" | "windows" => resolve_from_env_alias("WINDIR", "windir"),
        "startup" => std::env::var("APPDATA").ok().map(|base| {
            (
                "startup".to_string(),
                Path::new(&base)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs")
                    .join("Startup")
                    .to_string_lossy()
                    .to_string(),
            )
        }),
        "commonstartup" => std::env::var("PROGRAMDATA").ok().map(|base| {
            (
                "commonstartup".to_string(),
                Path::new(&base)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs")
                    .join("StartUp")
                    .to_string_lossy()
                    .to_string(),
            )
        }),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn resolve_linux_special_path(query: &str) -> Option<(String, String)> {
    let alias = query.trim().to_ascii_lowercase();
    let home = std::env::var("HOME").ok();

    match alias.as_str() {
        "home" | "~" => home.map(|path| ("home".to_string(), path)),
        "desktop" => home.map(|base| {
            (
                "desktop".to_string(),
                Path::new(&base)
                    .join("Desktop")
                    .to_string_lossy()
                    .to_string(),
            )
        }),
        "documents" | "docs" => home.map(|base| {
            (
                "documents".to_string(),
                Path::new(&base)
                    .join("Documents")
                    .to_string_lossy()
                    .to_string(),
            )
        }),
        "downloads" => home.map(|base| {
            (
                "downloads".to_string(),
                Path::new(&base)
                    .join("Downloads")
                    .to_string_lossy()
                    .to_string(),
            )
        }),
        "config" => std::env::var("XDG_CONFIG_HOME")
            .ok()
            .or_else(|| {
                home.as_ref().map(|base| {
                    Path::new(base)
                        .join(".config")
                        .to_string_lossy()
                        .to_string()
                })
            })
            .map(|path| ("config".to_string(), path)),
        "data" => std::env::var("XDG_DATA_HOME")
            .ok()
            .or_else(|| {
                home.as_ref().map(|base| {
                    Path::new(base)
                        .join(".local")
                        .join("share")
                        .to_string_lossy()
                        .to_string()
                })
            })
            .map(|path| ("data".to_string(), path)),
        "cache" => std::env::var("XDG_CACHE_HOME")
            .ok()
            .or_else(|| {
                home.as_ref()
                    .map(|base| Path::new(base).join(".cache").to_string_lossy().to_string())
            })
            .map(|path| ("cache".to_string(), path)),
        "temp" | "tmp" => std::env::var("TMPDIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some("/tmp".to_string()))
            .map(|path| ("temp".to_string(), path)),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn resolve_from_env_alias(env_name: &str, alias_name: &str) -> Option<(String, String)> {
    std::env::var(env_name)
        .ok()
        .map(|path| (alias_name.to_string(), path))
}

fn parse_windows_env_token(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.len() < 3 || !trimmed.starts_with('%') || !trimmed.ends_with('%') {
        return None;
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.is_empty() {
        return None;
    }

    if !inner
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return None;
    }

    Some(inner.to_ascii_uppercase())
}

fn encode_query_component(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(*byte));
            }
            b' ' => encoded.push('+'),
            value => encoded.push_str(&format!("%{value:02X}")),
        }
    }
    encoded
}

fn build_calculation(expression: &str, explicit: bool) -> Vec<SearchResult> {
    match evaluate(expression) {
        Ok(value) => {
            let formatted = format_number(value);
            vec![SearchResult {
                kind: SearchResultKind::Calculation,
                title: format!("{} = {}", expression.trim(), formatted),
                subtitle: "Enter para copiar resultado al portapapeles".to_string(),
                primary_value: formatted,
                score: if explicit { 550 } else { 290 },
            }]
        }
        Err(error) => {
            if explicit {
                vec![SearchResult {
                    kind: SearchResultKind::Info,
                    title: "Expresion no valida".to_string(),
                    subtitle: error.to_string(),
                    primary_value: String::new(),
                    score: 1,
                }]
            } else {
                vec![]
            }
        }
    }
}

struct MathHint {
    token: &'static str,
    insert: &'static str,
    label: &'static str,
    description: &'static str,
}

const MATH_HINTS: &[MathHint] = &[
    MathHint {
        token: "sin",
        insert: "sin()",
        label: "sin(x)",
        description: "Seno (radianes)",
    },
    MathHint {
        token: "cos",
        insert: "cos()",
        label: "cos(x)",
        description: "Coseno (radianes)",
    },
    MathHint {
        token: "tan",
        insert: "tan()",
        label: "tan(x)",
        description: "Tangente (radianes)",
    },
    MathHint {
        token: "asin",
        insert: "asin()",
        label: "asin(x)",
        description: "Arcoseno",
    },
    MathHint {
        token: "acos",
        insert: "acos()",
        label: "acos(x)",
        description: "Arcocoseno",
    },
    MathHint {
        token: "atan",
        insert: "atan()",
        label: "atan(x)",
        description: "Arcotangente",
    },
    MathHint {
        token: "sqrt",
        insert: "sqrt()",
        label: "sqrt(x)",
        description: "Raiz cuadrada",
    },
    MathHint {
        token: "cbrt",
        insert: "cbrt()",
        label: "cbrt(x)",
        description: "Raiz cubica",
    },
    MathHint {
        token: "ln",
        insert: "ln()",
        label: "ln(x)",
        description: "Logaritmo natural",
    },
    MathHint {
        token: "log",
        insert: "log()",
        label: "log(x,b)",
        description: "Logaritmo base b (1 o 2 args)",
    },
    MathHint {
        token: "log10",
        insert: "log10()",
        label: "log10(x)",
        description: "Logaritmo base 10",
    },
    MathHint {
        token: "pow",
        insert: "pow(,)",
        label: "pow(x,y)",
        description: "Potencia x^y",
    },
    MathHint {
        token: "fact",
        insert: "fact()",
        label: "fact(n)",
        description: "Factorial",
    },
    MathHint {
        token: "perm",
        insert: "perm(,)",
        label: "perm(n,r)",
        description: "Permutaciones nPr",
    },
    MathHint {
        token: "comb",
        insert: "comb(,)",
        label: "comb(n,r)",
        description: "Combinaciones nCr",
    },
    MathHint {
        token: "abs",
        insert: "abs()",
        label: "abs(x)",
        description: "Valor absoluto",
    },
    MathHint {
        token: "min",
        insert: "min(,)",
        label: "min(a,b,...)",
        description: "Minimo de varios valores",
    },
    MathHint {
        token: "max",
        insert: "max(,)",
        label: "max(a,b,...)",
        description: "Maximo de varios valores",
    },
    MathHint {
        token: "clamp",
        insert: "clamp(,,)",
        label: "clamp(x,min,max)",
        description: "Limita x al rango [min,max]",
    },
    MathHint {
        token: "floor",
        insert: "floor()",
        label: "floor(x)",
        description: "Redondeo hacia abajo",
    },
    MathHint {
        token: "ceil",
        insert: "ceil()",
        label: "ceil(x)",
        description: "Redondeo hacia arriba",
    },
    MathHint {
        token: "round",
        insert: "round()",
        label: "round(x)",
        description: "Redondeo al entero mas cercano",
    },
    MathHint {
        token: "exp",
        insert: "exp()",
        label: "exp(x)",
        description: "e^x",
    },
    MathHint {
        token: "deg",
        insert: "deg()",
        label: "deg(x)",
        description: "Convierte radianes a grados",
    },
    MathHint {
        token: "rad",
        insert: "rad()",
        label: "rad(x)",
        description: "Convierte grados a radianes",
    },
    MathHint {
        token: "pi",
        insert: "pi",
        label: "pi",
        description: "Constante PI",
    },
    MathHint {
        token: "e",
        insert: "e",
        label: "e",
        description: "Constante de Euler",
    },
    MathHint {
        token: "tau",
        insert: "tau",
        label: "tau",
        description: "Constante TAU",
    },
];

fn build_math_autocomplete(expression: &str, limit: usize, explicit: bool) -> Vec<SearchResult> {
    if limit == 0 {
        return vec![];
    }

    let token = current_math_token(expression);
    let normalized_token = token.to_ascii_lowercase();

    let mut ranked: Vec<(&MathHint, i32)> = MATH_HINTS
        .iter()
        .filter_map(|hint| {
            if normalized_token.is_empty() {
                return Some((hint, base_hint_score(hint, explicit) - 25));
            }

            if hint.token == normalized_token {
                return Some((hint, base_hint_score(hint, explicit) + 48));
            }

            if hint.token.starts_with(&normalized_token) {
                return Some((
                    hint,
                    base_hint_score(hint, explicit) + 35 - normalized_token.len() as i32,
                ));
            }

            if hint.token.contains(&normalized_token) {
                return Some((hint, base_hint_score(hint, explicit) + 8));
            }

            None
        })
        .collect();

    ranked.sort_by(|(left_hint, left_score), (right_hint, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_hint.token.len().cmp(&right_hint.token.len()))
            .then_with(|| left_hint.token.cmp(right_hint.token))
    });

    ranked
        .into_iter()
        .take(limit)
        .map(|(hint, score)| SearchResult {
            kind: SearchResultKind::Info,
            title: format!("Mates: {}", hint.label),
            subtitle: format!("{} · Tab para autocompletar", hint.description),
            primary_value: format!("math_complete:{}", hint.insert),
            score,
        })
        .collect()
}

fn current_math_token(expression: &str) -> Cow<'_, str> {
    let trimmed = expression.trim_end();
    if trimmed.is_empty() {
        return Cow::Borrowed("");
    }

    let mut start = trimmed.len();
    for (index, character) in trimmed.char_indices().rev() {
        if character.is_ascii_alphabetic() || character == '_' {
            start = index;
            continue;
        }
        break;
    }

    if start >= trimmed.len() {
        Cow::Borrowed("")
    } else {
        Cow::Owned(trimmed[start..].to_string())
    }
}

fn base_hint_score(hint: &MathHint, explicit: bool) -> i32 {
    let baseline = if explicit { 420 } else { 170 };
    baseline + (24 - i32::try_from(hint.token.len()).unwrap_or(0))
}

fn format_number(value: f64) -> String {
    let mut text = format!("{value:.12}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn looks_like_math(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    const FUNCTION_NAMES: &[&str] = &[
        "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "sqrt", "cbrt", "ln",
        "log", "log10", "exp", "abs", "floor", "ceil", "round", "trunc", "sign", "pow", "min",
        "max", "clamp", "rad", "deg", "fact", "perm", "comb", "pi", "tau", "e",
    ];

    let mut has_math_marker = false;
    for character in trimmed.chars() {
        if matches!(
            character,
            '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | ',' | '.' | ';'
        ) {
            has_math_marker = true;
            continue;
        }
        if character.is_ascii_digit()
            || character.is_ascii_alphabetic()
            || character.is_whitespace()
        {
            continue;
        }
        return false;
    }

    if has_math_marker {
        return true;
    }

    let lowered = trimmed.to_lowercase();
    FUNCTION_NAMES.iter().any(|name| {
        lowered == *name
            || lowered.starts_with(&format!("{name}("))
            || lowered.starts_with(&format!("{name} ("))
    })
}

#[cfg(target_os = "linux")]
fn read_desktop_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_section = true;
            continue;
        }
        if in_section && line.starts_with('[') {
            break;
        }
        if in_section {
            if let Some(value) = line.strip_prefix("Name=") {
                let name = value.trim().to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_mode: Path detection ---

    #[test]
    fn parse_mode_path_multi_segment_linux() {
        // Multi-segment always → Path regardless of existence
        let (mode, q) = parse_mode("/home/user/documents");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "/home/user/documents");
    }

    #[test]
    fn parse_mode_path_trailing_slash() {
        let (mode, q) = parse_mode("/home/user/");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "/home/user/");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_mode_path_existing_single_segment() {
        // /tmp always exists on Linux → Path mode even with one segment
        let (mode, q) = parse_mode("/tmp");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "/tmp");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_mode_file_nonexistent_single_segment() {
        // Non-existent single-segment stays as File mode
        let (mode, q) = parse_mode("/zzz_buscador_no_exist");
        assert_eq!(mode, SearchMode::File);
        assert_eq!(q, "zzz_buscador_no_exist");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_mode_path_windows_backslash() {
        let (mode, q) = parse_mode(r"C:\Users\test");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, r"C:\Users\test");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_mode_path_windows_forward_slash() {
        let (mode, q) = parse_mode("D:/Projects/app");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "D:/Projects/app");
    }

    // --- read_desktop_name ---

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_found() {
        let dir = std::env::temp_dir().join("buscador_test_dn1");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.desktop");
        std::fs::write(&p, "[Desktop Entry]\nName=My Cool App\nExec=myapp\n").unwrap();
        assert_eq!(read_desktop_name(&p), Some("My Cool App".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_outside_section_ignored() {
        let dir = std::env::temp_dir().join("buscador_test_dn2");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.desktop");
        std::fs::write(&p, "Name=Orphan\n[Desktop Entry]\nExec=myapp\n").unwrap();
        // Name= before [Desktop Entry] → None
        assert_eq!(read_desktop_name(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_nonexistent_file() {
        assert_eq!(read_desktop_name(Path::new("/no/such/file.desktop")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_stops_at_next_section() {
        let dir = std::env::temp_dir().join("buscador_test_dn3");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.desktop");
        std::fs::write(
            &p,
            "[Desktop Entry]\nExec=myapp\n[Other]\nName=Wrong\n",
        )
        .unwrap();
        assert_eq!(read_desktop_name(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
