//! Motor de embeddings basado en ONNX Runtime.
//! Usa granite-embedding-97m-multilingual-r2 (97M params, 384 dims, 32k context).
//!
//! Modelo: https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2
//! Licencia: Apache 2.0

use anyhow::{Context, Result};
use ort::session::Session;
use ort::value::Value;
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;

/// Dimensión de los vectores de embedding (Granite 97M = 384).
pub const EMBEDDING_DIM: usize = 384;
const MODEL_REPO_URL: &str =
    "https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2";
const DEFAULT_MODEL_CANDIDATES: &[&str] = &["model_quint8_avx2.onnx", "model.onnx"];

/// Motor de embeddings que carga el modelo ONNX y genera vectores.
pub struct EmbeddingEngine {
    session: Session,
    tokenizer: Tokenizer,
    /// Ruta exacta del modelo cargado.
    model_path: PathBuf,
}

impl EmbeddingEngine {
    /// Crea una nueva instancia cargando el modelo desde `model_dir`.
    ///
    /// Se esperan los siguientes archivos en el directorio:
    /// - `model_quint8_avx2.onnx` o `model.onnx`
    /// - `tokenizer.json` — tokenizer HuggingFace
    pub fn new(model_dir: &Path) -> Result<Self> {
        let model_path = resolve_model_path(model_dir)?;
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !tokenizer_path.exists() {
            anyhow::bail!(
                "Tokenizer no encontrado en {}. Descárgalo de:\n{}",
                tokenizer_path.display(),
                MODEL_REPO_URL
            );
        }

        // Cargar ONNX Runtime
        let session = Session::builder()
            .context("Creando builder de ONNX Runtime")?
            .commit_from_file(&model_path)
            .context("Cargando modelo ONNX")?;

        // Cargar tokenizer
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Error cargando tokenizer: {}", e))?;

        log::info!(
            "EmbeddingEngine cargado: {} (dim={})",
            model_path.display(),
            EMBEDDING_DIM
        );

        Ok(Self {
            session,
            tokenizer,
            model_path,
        })
    }

    /// Nombre del fichero ONNX que se ha cargado.
    pub fn model_file_name(&self) -> Option<&str> {
        self.model_path.file_name().and_then(|value| value.to_str())
    }

    /// Genera un embedding para un texto individual.
    ///
    /// Retorna un vector de 384 dimensiones normalizado (unit length).
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text])?;
        Ok(embeddings.into_iter().next().unwrap_or_default())
    }

    /// Genera embeddings para múltiples textos (batch inference).
    ///
    /// Retorna un Vec de vectores, uno por cada texto de entrada.
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Tokenizar todos los textos
        let texts_vec: Vec<&str> = texts.to_vec();
        let encodings = self
            .tokenizer
            .encode_batch(texts_vec, true)
            .map_err(|e| anyhow::anyhow!("Error tokenizando: {}", e))?;

        // Encontrar longitud máxima para padding
        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0);

        let batch_size = texts.len();

        // Crear arrays para input_ids y attention_mask
        let mut input_ids_flat: Vec<i64> = Vec::with_capacity(batch_size * max_len);
        let mut attention_mask_flat: Vec<i64> = Vec::with_capacity(batch_size * max_len);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();

            // Copiar IDs y máscara
            for &id in ids {
                input_ids_flat.push(id as i64);
            }
            for &m in mask {
                attention_mask_flat.push(m as i64);
            }

            // Padding con ceros
            for _ in ids.len()..max_len {
                input_ids_flat.push(0);
                attention_mask_flat.push(0);
            }
        }

        // Crear tensores using ndarray (ahora con la misma versión que ort)
        let input_ids_array =
            ndarray::Array2::from_shape_vec((batch_size, max_len), input_ids_flat)
                .context("Creando array input_ids")?;

        let attention_mask_array =
            ndarray::Array2::from_shape_vec((batch_size, max_len), attention_mask_flat)
                .context("Creando array attention_mask")?;

        // Convertir a ort::Value
        let input_ids_value =
            Value::from_array(input_ids_array).context("Creando Value input_ids")?;
        let attention_mask_value = Value::from_array(attention_mask_array.clone())
            .context("Creando Value attention_mask")?;

        // Inferencia ONNX - usar SessionInputValue re-exported
        use ort::session::SessionInputValue;

        let input_ids_sv: SessionInputValue<'_> = input_ids_value.into();
        let attention_mask_sv: SessionInputValue<'_> = attention_mask_value.into();

        let inputs = vec![
            (std::borrow::Cow::Borrowed("input_ids"), input_ids_sv),
            (
                std::borrow::Cow::Borrowed("attention_mask"),
                attention_mask_sv,
            ),
        ];

        let outputs = self
            .session
            .run(inputs)
            .context("Ejecutando inferencia ONNX")?;

        // Extraer output - buscar el tensor de salida
        let output = outputs
            .get("last_hidden_state")
            .context("Output 'last_hidden_state' no encontrado")?;

        let (_shape, data) = output
            .try_extract_tensor::<f32>()
            .context("Extrayendo tensor de embeddings")?;

        // shape: [batch_size, max_len, hidden_dim]
        // Calcular dimensiones desde el número de elementos
        let total_elements = data.len();
        let hidden_dim = total_elements / (batch_size * max_len);

        // Mean pooling sobre las posiciones donde attention_mask=1
        let mut result = Vec::with_capacity(batch_size);

        for b in 0..batch_size {
            let mut pooled = vec![0.0f32; hidden_dim];
            let mut count = 0.0f32;

            for t in 0..max_len {
                let mask_val = attention_mask_array[[b, t]] as f32;
                if mask_val > 0.0 {
                    let offset = (b * max_len + t) * hidden_dim;
                    for d in 0..hidden_dim {
                        pooled[d] += data[offset + d];
                    }
                    count += 1.0;
                }
            }

            // Promedio
            if count > 0.0 {
                for x in &mut pooled {
                    *x /= count;
                }
            }

            // L2 normalize
            let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut pooled {
                    *x /= norm;
                }
            }

            result.push(pooled);
        }

        Ok(result)
    }

    /// Indica si el modelo está disponible (archivos existen).
    pub fn is_available(model_dir: &Path) -> bool {
        resolve_model_path(model_dir).is_ok() && model_dir.join("tokenizer.json").exists()
    }
}

fn resolve_model_path(model_dir: &Path) -> Result<PathBuf> {
    if let Ok(preferred) = std::env::var("BUSCADOR_EMBEDDING_MODEL") {
        let preferred = preferred.trim();
        if !preferred.is_empty() {
            let candidate = model_dir.join(preferred);
            if candidate.exists() {
                return Ok(candidate);
            }

            anyhow::bail!(
                "Modelo ONNX '{}' no encontrado en {}. Opciones soportadas: {}. Repo: {}",
                preferred,
                model_dir.display(),
                DEFAULT_MODEL_CANDIDATES.join(", "),
                MODEL_REPO_URL
            );
        }
    }

    for candidate_name in DEFAULT_MODEL_CANDIDATES {
        let candidate = model_dir.join(candidate_name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "No se encontró ningún modelo ONNX en {}. Esperaba uno de: {}. Repo: {}",
        model_dir.display(),
        DEFAULT_MODEL_CANDIDATES.join(", "),
        MODEL_REPO_URL
    )
}
