use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub struct WebSearchService {
    client: Client,
}

enum WebSearchProvider {
    Disabled,
    Brave { api_key: String },
}

impl WebSearchService {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_millis(1800))
            .connect_timeout(Duration::from_millis(1200))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        provider: &str,
        api_key: &str,
    ) -> Vec<WebSearchHit> {
        if query.trim().is_empty() || limit == 0 {
            return vec![];
        }

        let provider = resolve_provider(provider, api_key);

        match &provider {
            WebSearchProvider::Disabled => vec![],
            WebSearchProvider::Brave { api_key } => self
                .search_brave(query, limit, api_key)
                .unwrap_or_else(|_| vec![]),
        }
    }

    fn search_brave(&self, query: &str, limit: usize, api_key: &str) -> Result<Vec<WebSearchHit>> {
        let response = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", &limit.to_string())])
            .header("Accept", "application/json")
            .header("X-Subscription-Token", api_key)
            .header("User-Agent", "Buscador/0.1.1")
            .send()
            .context("No se pudo consultar Brave Search")?;

        if !response.status().is_success() {
            anyhow::bail!("Brave Search devolvio estado {}", response.status());
        }

        let payload: Value = response
            .json()
            .context("Respuesta JSON invalida en Brave Search")?;

        let mut hits = Vec::new();
        let Some(items) = payload
            .get("web")
            .and_then(|web| web.get("results"))
            .and_then(Value::as_array)
        else {
            return Ok(hits);
        };

        for item in items {
            if hits.len() >= limit {
                break;
            }

            let Some(url) = item.get("url").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if !url.starts_with("http://") && !url.starts_with("https://") {
                continue;
            }

            let title = item
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or(url)
                .to_string();

            let snippet = item
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string();

            hits.push(WebSearchHit {
                title,
                url: url.to_string(),
                snippet,
            });
        }

        Ok(hits)
    }
}

fn resolve_provider(provider_name: &str, api_key: &str) -> WebSearchProvider {
    let provider_name = provider_name.trim().to_ascii_lowercase();
    let api_key = api_key.trim();
    match provider_name.as_str() {
        "brave" => {
            if api_key.is_empty() {
                WebSearchProvider::Disabled
            } else {
                WebSearchProvider::Brave {
                    api_key: api_key.to_string(),
                }
            }
        }
        _ => WebSearchProvider::Disabled,
    }
}
