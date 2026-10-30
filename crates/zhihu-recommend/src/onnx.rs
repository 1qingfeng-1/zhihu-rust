//! Bundled ONNX embedding provider (`bge-small-zh-v1.5`, int8 quantized).
//!
//! The model and tokenizer are embedded in the binary, so inference needs no
//! files, no network and no platform tokenizer. Inference runs through the
//! ONNX Runtime bundled by the `ort` crate. If the runtime or model cannot be
//! loaded, callers fall back to [`super::HashEmbeddingProvider`].

use std::sync::{Mutex, OnceLock};

use ort::environment::Environment;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use super::{normalize_vector, EmbeddingError, EmbeddingProvider, EmbeddingResult};

const MODEL_BYTES: &[u8] = include_bytes!("../assets/bge-small-zh-v1.5/model_quantized.onnx");
const TOKENIZER_JSON: &[u8] = include_bytes!("../assets/bge-small-zh-v1.5/tokenizer.json");
/// bge-small-zh hidden size.
const DIMENSION: usize = 512;
/// Titles and excerpts are short; 256 tokens keeps batches small.
const MAX_TOKENS: usize = 256;
const BATCH_SIZE: usize = 8;

/// ONNX Runtime allows one environment per process; tests and the app share it.
static ENVIRONMENT: OnceLock<Environment> = OnceLock::new();

fn environment() -> EmbeddingResult<&'static Environment> {
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment);
    }
    match ort::init().build() {
        Ok(environment) => Ok(ENVIRONMENT.get_or_init(|| environment)),
        Err(error) => ENVIRONMENT
            .get()
            .ok_or_else(|| EmbeddingError(format!("ort environment: {error}"))),
    }
}

pub struct OnnxEmbeddingProvider {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl OnnxEmbeddingProvider {
    pub const MODEL_ID: &'static str = "bge-small-zh-v1.5-q8";

    pub fn from_bundled() -> EmbeddingResult<Self> {
        Self::from_bytes(MODEL_BYTES, TOKENIZER_JSON)
    }

    pub fn from_bytes(model: &[u8], tokenizer_json: &[u8]) -> EmbeddingResult<Self> {
        let tokenizer = Tokenizer::from_bytes(tokenizer_json)
            .map_err(|error| EmbeddingError(format!("tokenizer: {error}")))?;
        let session = Session::builder(environment()?)
            .map_err(|error| EmbeddingError(format!("ort builder: {error}")))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|error| EmbeddingError(format!("ort optimization: {error}")))?
            .with_intra_threads(1)
            .map_err(|error| EmbeddingError(format!("ort threads: {error}")))?
            .commit_from_memory(model)
            .map_err(|error| EmbeddingError(format!("ort model: {error}")))?;
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    fn encode(&self, text: &str) -> EmbeddingResult<(Vec<i64>, Vec<i64>, Vec<i64>)> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|error| EmbeddingError(format!("tokenize: {error}")))?;
        let len = encoding.get_ids().len().min(MAX_TOKENS);
        if len == 0 {
            return Err(EmbeddingError("empty tokenization".to_string()));
        }
        let ids = encoding.get_ids()[..len]
            .iter()
            .map(|id| *id as i64)
            .collect();
        let mask = encoding.get_attention_mask()[..len]
            .iter()
            .map(|value| *value as i64)
            .collect();
        let types = encoding.get_type_ids()[..len]
            .iter()
            .map(|value| *value as i64)
            .collect();
        Ok((ids, mask, types))
    }

    fn embed_chunk(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        let encoded: Vec<(Vec<i64>, Vec<i64>, Vec<i64>)> = texts
            .iter()
            .map(|text| self.encode(text))
            .collect::<EmbeddingResult<Vec<_>>>()?;
        let batch = encoded.len();
        let seq_len = encoded
            .iter()
            .map(|(ids, _, _)| ids.len())
            .max()
            .unwrap_or(1)
            .max(1);
        let mut ids = vec![0i64; batch * seq_len];
        let mut mask = vec![0i64; batch * seq_len];
        let mut types = vec![0i64; batch * seq_len];
        for (row, (row_ids, row_mask, row_types)) in encoded.iter().enumerate() {
            let start = row * seq_len;
            ids[start..start + row_ids.len()].copy_from_slice(row_ids);
            mask[start..start + row_mask.len()].copy_from_slice(row_mask);
            types[start..start + row_types.len()].copy_from_slice(row_types);
        }

        let input_ids = Tensor::from_array(([batch, seq_len], ids))
            .map_err(|error| EmbeddingError(format!("input_ids tensor: {error}")))?;
        let attention_mask = Tensor::from_array(([batch, seq_len], mask))
            .map_err(|error| EmbeddingError(format!("attention_mask tensor: {error}")))?;
        let token_type_ids = Tensor::from_array(([batch, seq_len], types))
            .map_err(|error| EmbeddingError(format!("token_type_ids tensor: {error}")))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| EmbeddingError("onnx session poisoned".to_string()))?;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => attention_mask,
                "token_type_ids" => token_type_ids,
            ])
            .map_err(|error| EmbeddingError(format!("onnx run: {error}")))?;
        let hidden = outputs
            .get("last_hidden_state")
            .ok_or_else(|| EmbeddingError("missing last_hidden_state".to_string()))?
            .try_extract_array::<f32>()
            .map_err(|error| EmbeddingError(format!("onnx output: {error}")))?;
        let shape = hidden.shape().to_vec();
        if shape.len() != 3 || shape[0] != batch || shape[2] != DIMENSION {
            return Err(EmbeddingError(format!("unexpected output shape {shape:?}")));
        }
        let data = hidden
            .as_slice()
            .ok_or_else(|| EmbeddingError("non-contiguous output".to_string()))?;
        let row_stride = seq_len * DIMENSION;
        let mut vectors = Vec::with_capacity(batch);
        for row in 0..batch {
            // bge uses CLS pooling.
            let start = row * row_stride;
            let mut vector = data[start..start + DIMENSION].to_vec();
            normalize_vector(&mut vector);
            vectors.push(vector);
        }
        Ok(vectors)
    }
}

impl EmbeddingProvider for OnnxEmbeddingProvider {
    fn model_id(&self) -> &str {
        Self::MODEL_ID
    }

    fn dimension(&self) -> usize {
        DIMENSION
    }

    fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(BATCH_SIZE) {
            vectors.extend(self.embed_chunk(chunk)?);
        }
        Ok(vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::cosine;

    fn provider() -> OnnxEmbeddingProvider {
        OnnxEmbeddingProvider::from_bundled().expect("bundled model loads")
    }

    #[test]
    fn embeds_similar_titles_closer_than_unrelated() {
        let provider = provider();
        let vectors = provider
            .embed_batch(&[
                "Rust 移动端性能优化".to_string(),
                "Rust 移动端 UI 实践".to_string(),
                "今天中午吃什么".to_string(),
            ])
            .unwrap();
        assert_eq!(vectors.len(), 3);
        assert_eq!(vectors[0].len(), DIMENSION);
        let related = cosine(&vectors[0], &vectors[1]);
        let unrelated = cosine(&vectors[0], &vectors[2]);
        assert!(related > unrelated, "related {related} vs {unrelated}");
    }

    #[test]
    fn batch_matches_single_inference() {
        let provider = provider();
        let texts = vec![
            "如何评价 Rust 在移动端 UI 领域的现状？".to_string(),
            "中国的小偷为什么断崖式地下降？".to_string(),
        ];
        let batch = provider.embed_batch(&texts).unwrap();
        let single = provider.embed_batch(&texts[..1]).unwrap();
        // Quantized kernels can differ slightly with longer padded sequences;
        // the direction must still match.
        let similarity = cosine(&batch[0], &single[0]);
        assert!(similarity > 0.99, "cosine {similarity}");
        let norm = batch[1]
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-3);
    }
}
