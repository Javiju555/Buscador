//! HTTP API para que agentes (Nexus, Blazer, Argos) llamen a Buscador.
//!
//! Corre en un background thread dentro del proceso Tauri.
//! Endpoints:
//!   GET  /search?q=...&limit=10&mode=hybrid|fuzzy|semantic
//!   POST /index  — recibir items para indexar (desde Nexus/Argos)
//!   GET  /stats  — estadísticas del vector store
//!   GET  /health — liveness check
//!
//! Puerto por defecto: 8755

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Json, Query, State as AxumState},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::embedding_engine::EmbeddingEngine;
use crate::models::SearchResult;
use crate::vector_store::{self, VectorStore};

/// Estado compartido entre handlers HTTP.
#[derive(Clone)]
pub struct HttpState {
    pub search_fn: Arc<dyn Fn(&str, usize) -> Vec<SearchResult> + Send + Sync>,
    pub list_apps_fn: Arc<dyn Fn() -> Vec<(String, String, String)> + Send + Sync>,
    /// Devuelve las carpetas que el usuario configuró para indexar (semantic_roots).
    pub semantic_roots_fn: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    pub embedding_engine: Arc<std::sync::Mutex<Option<EmbeddingEngine>>>,
    pub vector_store: Arc<std::sync::Mutex<VectorStore>>,
}

/// Request body para POST /index.
#[derive(Deserialize)]
pub struct IndexRequest {
    pub items: Vec<IndexItem>,
}

#[derive(Deserialize)]
pub struct IndexItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub path: String,
    pub embedding: Option<Vec<f32>>,
    /// Texto para generar embedding (si embedding no se provee).
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Response de búsqueda.
#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultJson>,
    pub total: usize,
    pub mode: String,
}

#[derive(Serialize)]
pub struct SearchResultJson {
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub path: String,
    pub score: i32,
    pub similarity: Option<f32>,
}

/// Response de stats.
#[derive(Serialize)]
pub struct StatsResponse {
    pub total_items: usize,
    pub apps: usize,
    pub files: usize,
    pub engine_available: bool,
    pub model_file: Option<String>,
}

/// Response de health.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

// ── Handlers ──────────────────────────────────────────────

/// GET /health
async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// GET /stats
async fn stats(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    let store = state.vector_store.lock().unwrap();
    let model_file = state
        .embedding_engine
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|engine| engine.model_file_name().map(str::to_string));
    let configured = (state.semantic_roots_fn)();

    Json(serde_json::json!({
        "total_items": store.count(),
        "apps": store.count_by_kind("app"),
        "files": store.count_by_kind("file"),
        "emails": store.count_by_kind("email"),
        "engine_available": model_file.is_some(),
        "model_file": model_file,
        "configured_folders": configured,
    }))
}

/// Parámetros de query para GET /search.
#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_limit() -> usize {
    10
}

fn default_mode() -> String {
    "hybrid".to_string()
}

/// GET /search?q=...&limit=10&mode=hybrid|fuzzy|semantic
async fn search_handler(
    AxumState(state): AxumState<HttpState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    if params.q.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "q parameter is required"})),
        )
            .into_response();
    }

    let limit = params.limit.min(50);
    let mode = params.mode.as_str();

    match mode {
        "semantic" => {
            // Búsqueda puramente semántica
            let mut engine_guard = state.embedding_engine.lock().unwrap();
            if let Some(ref mut engine) = *engine_guard {
                match engine.embed(&params.q) {
                    Ok(query_emb) => {
                        drop(engine_guard); // Liberar lock antes de otro lock
                        let store = state.vector_store.lock().unwrap();
                        match store.search(&query_emb, limit, None) {
                            Ok(results) => {
                                let json_results: Vec<SearchResultJson> = results
                                    .into_iter()
                                    .map(|r| SearchResultJson {
                                        kind: r.item.kind,
                                        title: r.item.title,
                                        subtitle: r.item.subtitle,
                                        path: r.item.path,
                                        score: (r.similarity * 600.0) as i32,
                                        similarity: Some(r.similarity),
                                    })
                                    .collect();

                                let resp = SearchResponse {
                                    total: json_results.len(),
                                    results: json_results,
                                    mode: "semantic".to_string(),
                                };
                                (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
                                    .into_response()
                            }
                            Err(e) => (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({"error": e.to_string()})),
                            )
                                .into_response(),
                        }
                    }
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                        .into_response(),
                }
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "Embedding engine not available"})),
                )
                    .into_response()
            }
        }
        "fuzzy" => {
            // Búsqueda fuzzy pura
            let results = (state.search_fn)(&params.q, limit);
            let json_results: Vec<SearchResultJson> = results
                .into_iter()
                .map(|r| SearchResultJson {
                    kind: format!("{:?}", r.kind).to_lowercase(),
                    title: r.title,
                    subtitle: r.subtitle,
                    path: r.primary_value,
                    score: r.score,
                    similarity: None,
                })
                .collect();

            let resp = SearchResponse {
                total: json_results.len(),
                results: json_results,
                mode: "fuzzy".to_string(),
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
        }
        _ => {
            // hybrid: fuzzy + semantic merge (como el search normal de Tauri)
            let fuzzy_results = (state.search_fn)(&params.q, limit);

            // Funciones especiales (cálculo/web) tienen prioridad: si las hay,
            // NO corremos vector search para no taparlas.
            let has_special = fuzzy_results.iter().any(|r| {
                matches!(
                    r.kind,
                    crate::models::SearchResultKind::Calculation
                        | crate::models::SearchResultKind::Web
                )
            });

            // Intentar semantic solo si no hay función especial
            let semantic_results = if has_special {
                None
            } else {
                let emb = {
                    let mut engine_guard = state.embedding_engine.lock().unwrap();
                    if let Some(ref mut engine) = *engine_guard {
                        engine.embed(&params.q).ok()
                    } else {
                        None
                    }
                }; // engine_guard dropped here
                if let Some(query_emb) = emb {
                    let store = state.vector_store.lock().unwrap();
                    store.search(&query_emb, limit, None).ok()
                } else {
                    None
                }
            };

            // Merge
            let mut all_results: Vec<SearchResultJson> = fuzzy_results
                .into_iter()
                .map(|r| SearchResultJson {
                    kind: format!("{:?}", r.kind).to_lowercase(),
                    title: r.title,
                    subtitle: r.subtitle,
                    path: r.primary_value,
                    score: r.score,
                    similarity: None,
                })
                .collect();

            if let Some(sem) = semantic_results {
                for sr in sem {
                    let semantic_score = (sr.similarity * 600.0) as i32;
                    if semantic_score < 180 {
                        continue;
                    }

                    let exists = all_results
                        .iter_mut()
                        .find(|r| r.path == sr.item.path || r.title == sr.item.title);

                    if let Some(existing) = exists {
                        existing.score = existing.score.max(semantic_score);
                    } else {
                        all_results.push(SearchResultJson {
                            kind: sr.item.kind,
                            title: sr.item.title,
                            subtitle: sr.item.subtitle,
                            path: sr.item.path,
                            score: semantic_score,
                            similarity: Some(sr.similarity),
                        });
                    }
                }
            }

            all_results.sort_by(|a, b| b.score.cmp(&a.score));
            all_results.truncate(limit);

            let resp = SearchResponse {
                total: all_results.len(),
                results: all_results,
                mode: "hybrid".to_string(),
            };
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
        }
    }
}

/// POST /reindex — Re-indexa apps (siempre) + archivos por nombre (solo carpetas configuradas).
///
/// NO indexa contenido de archivos ni código fuente (eso disparaba la RAM).
/// Los archivos solo se indexan si el usuario configuró carpetas en Ajustes.
async fn reindex_handler(AxumState(state): AxumState<HttpState>) -> impl IntoResponse {
    let mut engine_guard = state.embedding_engine.lock().unwrap();
    let engine = match engine_guard.as_mut() {
        Some(e) => e,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Embedding engine not available"})),
            )
                .into_response();
        }
    };

    let mut total_files = 0;
    let mut errors = Vec::new();

    // 1. Indexar apps (siempre, es ligero)
    let apps = (state.list_apps_fn)();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut app_items = Vec::new();

    for (name, _exec, desktop_path) in &apps {
        match engine.embed(name) {
            Ok(embedding) => {
                app_items.push(vector_store::VectorItem {
                    id: format!("app:{}", name.to_lowercase()),
                    kind: "app".to_string(),
                    title: name.clone(),
                    subtitle: String::new(),
                    path: desktop_path.clone(),
                    embedding,
                    metadata: "{}".to_string(),
                    updated_at: now,
                });
            }
            Err(e) => {
                errors.push(format!("app {}: {}", name, e));
            }
        }
    }

    let total_apps = app_items.len();

    {
        let store = state.vector_store.lock().unwrap();
        if let Err(e) = store.remove_by_kind("app") {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
        if let Err(e) = store.upsert_batch(&app_items) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    // 2. Indexar archivos POR NOMBRE en las carpetas configuradas por el usuario.
    //    Vacío por defecto → no indexa nada. Sin contenido, sin código.
    let semantic_roots = (state.semantic_roots_fn)();
    let roots = crate::indexer::roots_from_settings(&semantic_roots);
    if roots.is_empty() {
        // Sin carpetas configuradas: limpiar cualquier archivo viejo de la DB.
        let _ = state.vector_store.lock().unwrap().remove_by_kind("file");
    } else {
        match crate::indexer::index_files(
            &roots,
            &state.vector_store.lock().unwrap(),
            engine,
            20000, // max files (solo nombres, es barato)
            10,    // max depth
        ) {
            Ok(result) => {
                total_files = result.files_indexed;
                errors.extend(result.errors);
            }
            Err(e) => {
                errors.push(format!("Error indexando archivos: {}", e));
            }
        }
    }

    let store = state.vector_store.lock().unwrap();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "apps": total_apps,
            "files": total_files,
            "configured_folders": roots.len(),
            "total": store.count(),
            "errors": errors.len(),
            "error_details": if errors.is_empty() { None } else { Some(&errors) }
        })),
    )
        .into_response()
}

/// POST /index — Indexar items enviados desde Nexus/Argos.
///
/// Recibe items con embeddings pre-calculados (o texto para generarlos).
async fn index_handler(
    AxumState(state): AxumState<HttpState>,
    Json(request): Json<IndexRequest>,
) -> impl IntoResponse {
    let mut items = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    for req_item in &request.items {
        let embedding = if let Some(ref emb) = req_item.embedding {
            emb.clone()
        } else if !req_item.text.is_empty() {
            // Generar embedding del texto
            let mut engine_guard = state.embedding_engine.lock().unwrap();
            if let Some(ref mut engine) = *engine_guard {
                match engine.embed(&req_item.text) {
                    Ok(emb) => emb,
                    Err(e) => {
                        log::error!("Error generando embedding para '{}': {}", req_item.id, e);
                        continue;
                    }
                }
            } else {
                log::warn!("Embedding engine no disponible, saltando '{}'", req_item.id);
                continue;
            }
        } else {
            log::warn!("Item '{}' sin embedding ni texto, saltando", req_item.id);
            continue;
        };

        items.push(vector_store::VectorItem {
            id: req_item.id.clone(),
            kind: req_item.kind.clone(),
            title: req_item.title.clone(),
            subtitle: req_item.subtitle.clone(),
            path: req_item.path.clone(),
            embedding,
            metadata: req_item.metadata.to_string(),
            updated_at: now,
        });
    }

    let count = items.len();
    let store = state.vector_store.lock().unwrap();

    if let Err(e) = store.upsert_batch(&items) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "indexed": count,
            "total": store.count()
        })),
    )
        .into_response()
}

// ── Server startup ────────────────────────────────────────

/// Inicia el servidor HTTP en un background thread.
///
/// Retorna un `mpsc::Sender` para enviar el shutdown signal.
pub fn start_http_server(state: HttpState, port: u16) -> mpsc::Sender<()> {
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("No se pudo crear tokio runtime");

        runtime.block_on(async move {
            let app = Router::new()
                .route("/health", get(health))
                .route("/stats", get(stats))
                .route("/search", get(search_handler))
                .route("/reindex", post(reindex_handler))
                .route("/index", post(index_handler))
                .with_state(state);

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            log::info!("Buscador HTTP API escuchando en http://{}", addr);

            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    log::error!("No se pudo bindear puerto {}: {}", port, e);
                    return;
                }
            };

            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.recv().await;
                    log::info!("HTTP server shutting down...");
                })
                .await
                .expect("Error en HTTP server");
        });
    });

    shutdown_tx
}
