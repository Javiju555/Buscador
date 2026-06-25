//! Almacén de vectores basado en SQLite.
//! Almacena embeddings generados por EmbeddingEngine y permite búsqueda
//! por cosine similarity.
//!
//! Base de datos: ~/.local/share/buscador/vectors.db

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::embedding_engine::EMBEDDING_DIM;

/// Item almacenado en el vector store.
#[derive(Debug, Clone)]
pub struct VectorItem {
    /// ID único: "app:gimp", "file:/path/to/doc.rs"
    pub id: String,
    /// Tipo de item: "app", "file", "email", "code", etc.
    pub kind: String,
    /// Título visible
    pub title: String,
    /// Subtítulo / descripción
    pub subtitle: String,
    /// Ruta o valor primario (para ejecución)
    pub path: String,
    /// Embedding de 384 dimensiones
    pub embedding: Vec<f32>,
    /// Metadata extra (JSON serializado)
    pub metadata: String,
    /// Timestamp de última actualización
    pub updated_at: i64,
}

/// Resultado de búsqueda semántica.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub item: VectorItem,
    /// Cosine similarity (0.0 .. 1.0)
    pub similarity: f32,
}

/// Almacén de vectores con búsqueda por cosine similarity.
///
/// Usa SQLite con almacenamiento BLOB para los vectores.
/// Para menos de ~50k items, brute-force cosine es suficiente
/// y no necesita HNSW/FAISS.
pub struct VectorStore {
    db: Connection,
}

impl VectorStore {
    /// Abre o crea la base de datos en la ruta especificada.
    pub fn open(db_path: &Path) -> Result<Self> {
        // Asegurar que el directorio padre existe
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("Creando directorio del vector store")?;
        }

        let db = Connection::open(db_path)
            .with_context(|| format!("Abriendo vector store en {}", db_path.display()))?;

        // Habilitar WAL para mejor concurrencia
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .context("Configurando WAL mode")?;

        // Crear tabla si no existe
        db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS vector_items (
                id          TEXT PRIMARY KEY,
                kind        TEXT NOT NULL,
                title       TEXT NOT NULL,
                subtitle    TEXT DEFAULT '',
                path        TEXT DEFAULT '',
                embedding   BLOB NOT NULL,
                metadata    TEXT DEFAULT '{}',
                updated_at  INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_vector_items_kind
                ON vector_items(kind);

            CREATE INDEX IF NOT EXISTS idx_vector_items_updated
                ON vector_items(updated_at);
            ",
        )
        .context("Creando tabla vector_items")?;

        log::info!("VectorStore abierto en {}", db_path.display());

        Ok(Self { db })
    }

    /// Inserta o actualiza un item con su embedding.
    pub fn upsert(&self, item: &VectorItem) -> Result<()> {
        let embedding_bytes = f32_vec_to_bytes(&item.embedding);

        self.db
            .execute(
                "INSERT OR REPLACE INTO vector_items
             (id, kind, title, subtitle, path, embedding, metadata, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    item.id,
                    item.kind,
                    item.title,
                    item.subtitle,
                    item.path,
                    embedding_bytes,
                    item.metadata,
                    item.updated_at,
                ],
            )
            .context("Insertando item en vector store")?;

        Ok(())
    }

    /// Inserta o actualiza múltiples items en una transacción (más rápido).
    pub fn upsert_batch(&self, items: &[VectorItem]) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO vector_items
                 (id, kind, title, subtitle, path, embedding, metadata, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;

            for item in items {
                let embedding_bytes = f32_vec_to_bytes(&item.embedding);
                stmt.execute(params![
                    item.id,
                    item.kind,
                    item.title,
                    item.subtitle,
                    item.path,
                    embedding_bytes,
                    item.metadata,
                    item.updated_at,
                ])?;
            }
        }

        tx.commit()?;
        log::info!("Batch upsert: {} items", items.len());

        Ok(())
    }

    /// Elimina un item por ID.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM vector_items WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Elimina todos los items de un tipo específico.
    pub fn remove_by_kind(&self, kind: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM vector_items WHERE kind = ?1", params![kind])?;
        Ok(())
    }

    /// Busca los items más similares al query embedding.
    ///
    /// Retorna los `limit` items con mayor cosine similarity.
    /// Opcionalmente filtra por kind.
    pub fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        kind_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        if query_embedding.len() != EMBEDDING_DIM {
            anyhow::bail!(
                "Embedding query tiene dimensión {} esperaba {}",
                query_embedding.len(),
                EMBEDDING_DIM
            );
        }

        // Para pocos items, brute-force es rápido y simple
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match kind_filter {
            Some(kind) => (
                "SELECT id, kind, title, subtitle, path, embedding, metadata, updated_at
                 FROM vector_items WHERE kind = ?1",
                vec![Box::new(kind.to_string()) as Box<dyn rusqlite::types::ToSql>],
            ),
            None => (
                "SELECT id, kind, title, subtitle, path, embedding, metadata, updated_at
                 FROM vector_items",
                vec![],
            ),
        };

        let mut stmt = self.db.prepare(sql)?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt.query_map(&*param_refs, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;

        let mut results: Vec<SearchResult> = Vec::new();

        for row_result in rows {
            let (id, kind, title, subtitle, path, embedding_bytes, metadata, updated_at) =
                row_result?;

            let embedding = bytes_to_f32_vec(&embedding_bytes);

            if embedding.len() != EMBEDDING_DIM {
                log::warn!(
                    "Item {} tiene embedding de dimensión incorrecta: {}",
                    id,
                    embedding.len()
                );
                continue;
            }

            let similarity = cosine_similarity(query_embedding, &embedding);

            results.push(SearchResult {
                item: VectorItem {
                    id,
                    kind,
                    title,
                    subtitle,
                    path,
                    embedding,
                    metadata,
                    updated_at,
                },
                similarity,
            });
        }

        // Ordenar por similaridad descendente
        results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

        // Truncar a limit
        results.truncate(limit);

        Ok(results)
    }

    /// Retorna el número de items almacenados.
    pub fn count(&self) -> usize {
        self.db
            .query_row("SELECT COUNT(*) FROM vector_items", [], |row| row.get(0))
            .unwrap_or(0)
    }

    /// Retorna el número de items de un tipo específico.
    pub fn count_by_kind(&self, kind: &str) -> usize {
        self.db
            .query_row(
                "SELECT COUNT(*) FROM vector_items WHERE kind = ?1",
                params![kind],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    /// Limpia todos los items.
    pub fn clear(&self) -> Result<()> {
        self.db.execute("DELETE FROM vector_items", [])?;
        Ok(())
    }
}

// ── Funciones de conversión ───────────────────────────────

/// Convierte un Vec<f32> a bytes para almacenamiento BLOB.
fn f32_vec_to_bytes(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &f in vec {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

/// Convierte bytes BLOB a Vec<f32>.
fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Calcula la cosine similarity entre dos vectores normalizados.
///
/// Asume vectores normalizados (unit length): simplemente producto punto.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ── Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_upsert_and_search() {
        let db_path = temp_dir().join("test_vector_store.db");
        let _ = std::fs::remove_file(&db_path);

        let store = VectorStore::open(&db_path).unwrap();
        let mut gimp_embedding = vec![0.0; EMBEDDING_DIM];
        gimp_embedding[0] = 1.0;
        let mut firefox_embedding = vec![0.0; EMBEDDING_DIM];
        firefox_embedding[1] = 1.0;

        // Crear items de prueba
        let items = vec![
            VectorItem {
                id: "app:gimp".to_string(),
                kind: "app".to_string(),
                title: "GIMP".to_string(),
                subtitle: "GNU Image Manipulation Program".to_string(),
                path: "/usr/share/applications/gimp.desktop".to_string(),
                embedding: gimp_embedding,
                metadata: "{}".to_string(),
                updated_at: 1,
            },
            VectorItem {
                id: "app:firefox".to_string(),
                kind: "app".to_string(),
                title: "Firefox".to_string(),
                subtitle: "Web Browser".to_string(),
                path: "/usr/share/applications/firefox.desktop".to_string(),
                embedding: firefox_embedding,
                metadata: "{}".to_string(),
                updated_at: 1,
            },
        ];

        store.upsert_batch(&items).unwrap();
        assert_eq!(store.count(), 2);

        // Buscar: query cercano a GIMP
        let mut query = vec![0.0; EMBEDDING_DIM];
        query[0] = 0.9;
        query[1] = 0.1;
        let results = store.search(&query, 5, None).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].item.id, "app:gimp");
        assert!(results[0].similarity > results[1].similarity);

        // Buscar con filtro de kind
        let results = store.search(&query, 5, Some("app")).unwrap();
        assert_eq!(results.len(), 2);

        let results = store.search(&query, 5, Some("file")).unwrap();
        assert_eq!(results.len(), 0);

        // Limpiar
        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 0.001);

        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.707, 0.707, 0.0];
        assert!((cosine_similarity(&a, &b) - 0.707).abs() < 0.01);
    }
}
