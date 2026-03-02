use crate::app_catalog::AppCatalog;
use crate::calculator::evaluate;
use crate::command_catalog::CommandCatalog;
use crate::file_catalog::FileCatalog;
use crate::models::{LauncherSettings, SearchResponse, SearchResult, SearchResultKind};

pub struct SearchService {
    app_catalog: AppCatalog,
    command_catalog: CommandCatalog,
    file_catalog: FileCatalog,
}

impl SearchService {
    pub fn new(settings: LauncherSettings) -> Self {
        Self {
            app_catalog: AppCatalog::new(),
            command_catalog: CommandCatalog::new(),
            file_catalog: FileCatalog::new(settings),
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
            return SearchResponse {
                results: build_calculation(&query, true),
                file_indexing: self.file_catalog.is_indexing(),
            };
        }

        let mut bag: Vec<SearchResult> = vec![];
        if mode == SearchMode::Mixed {
            bag.extend(self.app_catalog.search(&query, limit));
        }
        if mode == SearchMode::Mixed || mode == SearchMode::Command {
            bag.extend(self.command_catalog.search(&query, limit));
        }
        if mode == SearchMode::File || (mode == SearchMode::Mixed && include_files_in_mixed) {
            bag.extend(self.file_catalog.search(&query, limit));
        }
        if mode == SearchMode::Mixed && looks_like_math(&query) {
            bag.extend(build_calculation(&query, false));
        }

        bag.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
        bag.truncate(limit);

        SearchResponse {
            results: bag,
            file_indexing: self.file_catalog.is_indexing(),
        }
    }
}

#[derive(PartialEq, Eq)]
enum SearchMode {
    Mixed,
    Command,
    File,
    Calculation,
}

fn parse_mode(raw_query: &str) -> (SearchMode, String) {
    let query = raw_query.trim();
    if let Some(stripped) = query.strip_prefix('>') {
        return (SearchMode::Command, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix('/') {
        return (SearchMode::File, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix('=') {
        return (SearchMode::Calculation, stripped.trim().to_string());
    }
    (SearchMode::Mixed, query.to_string())
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
    let mut has_operator = false;
    for character in query.chars() {
        if matches!(character, '+' | '-' | '*' | '/' | '%' | '^') {
            has_operator = true;
            continue;
        }
        if character.is_ascii_digit()
            || character.is_whitespace()
            || matches!(character, '(' | ')' | '.' | ',')
        {
            continue;
        }
        return false;
    }
    has_operator
}
