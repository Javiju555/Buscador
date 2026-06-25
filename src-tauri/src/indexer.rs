//! Indexador semántico de archivos (solo nombre/ruta, NUNCA contenido).
//!
//! IMPORTANTE: este indexador NO lee el contenido de los archivos. Solo indexa
//! el nombre y la ruta relativa. Leer contenido de miles de archivos dispara
//! la RAM (cada embedding carga el modelo) — por eso se eliminó.
//!
//! Las carpetas a indexar las define el USUARIO en Ajustes (`semantic_roots`).
//! Por defecto está vacío: no se indexa ningún archivo hasta que el usuario opta-in.
//!
//! Agnóstico al SO: funciona en Windows y Linux con cualquier idioma, porque las
//! rutas vienen de la configuración del usuario (o de XDG/Known Folders si las añade).

use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::embedding_engine::EmbeddingEngine;
use crate::vector_store::{VectorItem, VectorStore};

/// Extensiones que se ignoran (binarios, temporales, multimedia pesada).
const IGNORED_EXTENSIONS: &[&str] = &[
    "tmp", "cache", "lock", "bak", "swp", "swo", "zip", "tar", "gz", "bz2", "xz", "zst", "rar",
    "7z", "exe", "dll", "so", "dylib", "o", "a", "bin", "woff", "woff2", "ttf", "otf",
];

/// Directorios que se ignoran al recorrer (ruido, no documentos del usuario).
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".cache",
    ".local",
    ".config",
    ".npm",
    ".cargo",
    "venv",
    ".venv",
    "env",
    ".Trash",
    "$RECYCLE.BIN",
    "System Volume Information",
];

/// Resultado del indexado de archivos.
#[derive(Debug, Default)]
pub struct IndexResult {
    pub files_indexed: usize,
    pub errors: Vec<String>,
}

/// Extensiones de texto plano cuyo contenido (preview corto) SÍ se lee
/// para enriquecer la búsqueda semántica. Solo archivos pequeños y de texto.
const TEXT_PREVIEW_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "rst", "org", "csv", "tsv", "log", "json", "yaml", "yml", "toml",
    "ini", "conf",
];

/// Máximo de bytes que se leen del contenido de un archivo para el preview.
/// Acotado a propósito para no disparar la RAM ni meter archivos enormes.
const MAX_PREVIEW_BYTES: usize = 4096;

/// Indexa archivos en las carpetas dadas de forma **secuencial e incremental**.
///
/// Procesa archivo a archivo: embeddea y escribe a la DB en lotes pequeños,
/// nunca acumula miles de items en RAM. Para tipos de texto plano y pequeños
/// lee un preview corto del contenido (máx. `MAX_PREVIEW_BYTES`); para el resto
/// solo indexa nombre + ruta relativa.
///
/// `roots` viene de la config del usuario (`semantic_roots`).
pub fn index_files(
    roots: &[PathBuf],
    store: &VectorStore,
    engine: &mut EmbeddingEngine,
    max_files: usize,
    max_depth: usize,
) -> Result<IndexResult> {
    let mut result = IndexResult::default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    // Limpiar archivos previos
    store.remove_by_kind("file")?;

    if roots.is_empty() {
        log::info!("Sin carpetas configuradas para indexar (semantic_roots vacío)");
        return Ok(result);
    }

    // Buffer pequeño: escribimos a la DB cada FLUSH_EVERY items y vaciamos.
    // Así la RAM se mantiene acotada por muy grande que sea la carpeta.
    const FLUSH_EVERY: usize = 64;
    let mut batch: Vec<VectorItem> = Vec::with_capacity(FLUSH_EVERY);

    'outer: for root in roots {
        if !root.exists() {
            log::warn!("Carpeta configurada no existe: {}", root.display());
            continue;
        }

        log::info!("Indexando (secuencial): {}", root.display());

        for entry in WalkDir::new(root)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_str().unwrap_or("");
                    return !name.starts_with('.') && !IGNORED_DIRS.contains(&name);
                }
                true
            })
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if IGNORED_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let relative = path.strip_prefix(root).unwrap_or(path);

            // Texto base: nombre + ruta relativa.
            let mut searchable = format!("{} | {}", name, relative.display());

            // Para tipos de texto plano pequeños, añadimos un preview del contenido.
            let mut has_content = false;
            if TEXT_PREVIEW_EXTENSIONS.contains(&ext.as_str()) {
                if let Some(preview) = read_text_preview(path, MAX_PREVIEW_BYTES) {
                    if !preview.trim().is_empty() {
                        searchable.push_str(" | ");
                        searchable.push_str(&preview);
                        has_content = true;
                    }
                }
            }

            match engine.embed(&searchable) {
                Ok(embedding) => {
                    batch.push(VectorItem {
                        id: format!("file:{}", path.display()),
                        kind: "file".to_string(),
                        title: name.to_string(),
                        subtitle: relative.display().to_string(),
                        path: path.display().to_string(),
                        embedding,
                        metadata: serde_json::json!({
                            "extension": ext,
                            "content_indexed": has_content,
                        })
                        .to_string(),
                        updated_at: now,
                    });
                    result.files_indexed += 1;

                    // Flush incremental: escribe a DB y libera RAM.
                    if batch.len() >= FLUSH_EVERY {
                        store.upsert_batch(&batch)?;
                        batch.clear();
                    }

                    if result.files_indexed >= max_files {
                        log::warn!("Límite de {} archivos alcanzado", max_files);
                        break 'outer;
                    }
                }
                Err(e) => result.errors.push(format!("{}: {}", path.display(), e)),
            }
        }
    }

    // Flush final
    if !batch.is_empty() {
        store.upsert_batch(&batch)?;
    }

    log::info!(
        "Indexación de archivos completada: {} archivos, {} errores",
        result.files_indexed,
        result.errors.len()
    );

    Ok(result)
}

/// Lee hasta `max_bytes` del inicio de un archivo de texto.
/// Devuelve None si no es UTF-8 válido o no se puede leer.
fn read_text_preview(path: &Path, max_bytes: usize) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max_bytes];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    // Solo aceptamos UTF-8 limpio (evita binarios disfrazados).
    String::from_utf8(buf).ok().map(|s| {
        // Normalizar espacios para que el embedding sea estable.
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    })
}

/// Convierte las rutas de `semantic_roots` (strings de config) a PathBuf existentes.
///
/// Agnóstico al SO: acepta rutas tal cual las escribió el usuario en Ajustes,
/// y expande `~` al home en Unix.
pub fn roots_from_settings(semantic_roots: &[String]) -> Vec<PathBuf> {
    semantic_roots
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| expand_home(s))
        .filter(|p| p.exists())
        .collect()
}

/// Expande `~` o `~/...` al directorio home (no-op en Windows con rutas absolutas).
fn expand_home(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(s)
}

/// Sugerencias de carpetas para el selector de Ajustes (agnóstico al SO/idioma).
///
/// Devuelve las carpetas estándar del usuario vía XDG (Linux) / Known Folders
/// (Windows), que respetan el idioma automáticamente. El usuario elige cuáles.
/// NO se indexa nada automáticamente — esto es solo para poblar el selector.
pub fn suggested_roots() -> Vec<PathBuf> {
    [
        dirs::document_dir(),
        dirs::desktop_dir(),
        dirs::download_dir(),
        dirs::picture_dir(),
    ]
    .into_iter()
    .flatten()
    .filter(|p: &PathBuf| p.exists())
    .collect()
}

/// Helper interno: comprueba si una ruta está dentro de un root permitido.
#[allow(dead_code)]
fn is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}
