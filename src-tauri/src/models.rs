use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchResultKind {
    App,
    Command,
    File,
    Web,
    Calculation,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub kind: SearchResultKind,
    pub title: String,
    pub subtitle: String,
    pub primary_value: String,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub file_indexing: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutePayload {
    pub kind: SearchResultKind,
    pub title: String,
    pub primary_value: String,
    pub raw_query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettings {
    #[serde(default)]
    pub start_with_windows: bool,
    pub roots: Vec<String>,
    pub max_files: usize,
    #[serde(default)]
    pub web_provider: String,
    #[serde(default)]
    pub web_api_key: String,
}
