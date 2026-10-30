//! Local text embeddings (M8-3).
//!
//! Ranking accepts any [`EmbeddingProvider`]. The bundled provider is a
//! deterministic character/word hashing model: it projects text tokens into a
//! fixed-size signed vector and L2-normalizes it, so titles with shared words
//! and bigrams land close together. It needs no model file, no network and no
//! platform runtime, which makes it the always-available fallback described in
//! the M8 plan.
//!
//! An ONNX provider (e.g. a quantized `bge-small-zh`) can implement the same
//! trait once the model license, size and distribution path are confirmed; the
//! cache is keyed by `model_id`, so switching providers invalidates old rows.

use std::collections::HashMap;

use crate::title_tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingError(pub String);

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for EmbeddingError {}

pub type EmbeddingResult<T> = Result<T, EmbeddingError>;

/// Anything that turns text into normalized vectors. Implementations must be
/// deterministic for the same `model_id` and input.
pub trait EmbeddingProvider: Send + Sync {
    fn model_id(&self) -> &str;
    fn dimension(&self) -> usize;
    fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>>;
}

/// Built-in hashing provider (`hash-bigram-v1`).
///
/// Tokens come from [`title_tokens`] (lowercase words plus CJK bigrams). Each
/// token is FNV-1a hashed to an index and a ±1 sign, accumulated, then
/// L2-normalized. This is the statistical baseline the plan requires when no
/// ONNX model is available.
#[derive(Clone, Debug)]
pub struct HashEmbeddingProvider {
    dimension: usize,
}

impl HashEmbeddingProvider {
    pub const MODEL_ID: &'static str = "hash-bigram-v1";
    pub const DEFAULT_DIMENSION: usize = 192;

    pub fn new(dimension: usize) -> Self {
        Self {
            dimension: dimension.clamp(32, 1024),
        }
    }

    pub fn embed_text(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; self.dimension];
        for token in title_tokens(text) {
            let hash = fnv1a(token.as_bytes());
            let index = (hash % self.dimension as u64) as usize;
            let sign = if hash & (1 << 63) != 0 { -1.0 } else { 1.0 };
            vector[index] += sign;
        }
        normalize_vector(&mut vector);
        vector
    }
}

impl Default for HashEmbeddingProvider {
    fn default() -> Self {
        Self::new(Self::DEFAULT_DIMENSION)
    }
}

impl EmbeddingProvider for HashEmbeddingProvider {
    fn model_id(&self) -> &str {
        Self::MODEL_ID
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed_batch(&self, texts: &[String]) -> EmbeddingResult<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|text| self.embed_text(text)).collect())
    }
}

/// Weighted interest center plus the item vectors it was built from.
///
/// `vector` may be all zeros when no feedback item has an embedding yet; in
/// that case [`Self::similarity`] returns `None` and callers keep the
/// statistical score.
#[derive(Clone, Debug)]
pub struct VectorProfile {
    pub model_id: String,
    pub vector: Vec<f32>,
    items: HashMap<String, Vec<f32>>,
}

impl VectorProfile {
    pub fn new(model_id: String, vector: Vec<f32>, items: HashMap<String, Vec<f32>>) -> Self {
        Self {
            model_id,
            vector,
            items,
        }
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn contains(&self, kind: &str, id: &str) -> bool {
        self.items.contains_key(&item_key(kind, id))
    }

    pub fn extend(&mut self, entries: impl IntoIterator<Item = (String, Vec<f32>)>) {
        self.items.extend(entries);
    }

    /// Cosine similarity between the interest center and one item, or `None`
    /// when the profile center is empty or the item is not embedded.
    pub fn similarity(&self, kind: &str, id: &str) -> Option<f32> {
        if self.vector.iter().all(|value| *value == 0.0) {
            return None;
        }
        let item = self.items.get(&item_key(kind, id))?;
        Some(cosine(&self.vector, item))
    }
}

pub fn item_key(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (left, right) in a.iter().zip(b) {
        dot += left * right;
        norm_a += left * left;
        norm_b += right * right;
    }
    if norm_a <= f32::EPSILON || norm_b <= f32::EPSILON {
        return 0.0;
    }
    (dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0)
}

pub fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

/// Pick the best available provider: bundled ONNX model first, deterministic
/// hashing otherwise. The boolean says whether the statistical fallback was
/// used, which the settings page surfaces.
pub fn select_embedding_provider() -> (Box<dyn EmbeddingProvider>, bool) {
    #[cfg(feature = "onnx")]
    {
        match crate::onnx::OnnxEmbeddingProvider::from_bundled() {
            Ok(provider) => return (Box::new(provider), false),
            Err(error) => {
                eprintln!(
                    "[recommend] ONNX embedding unavailable, falling back to hash-bigram: {error}"
                );
            }
        }
    }
    (Box::new(HashEmbeddingProvider::default()), true)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embed(text: &str) -> Vec<f32> {
        HashEmbeddingProvider::default().embed_text(text)
    }

    #[test]
    fn embeddings_are_deterministic_and_normalized() {
        let provider = HashEmbeddingProvider::default();
        let first = provider.embed_text("Rust 移动端 UI 实践");
        let second = provider.embed_text("Rust 移动端 UI 实践");
        assert_eq!(first, second);
        assert_eq!(first.len(), provider.dimension());
        let norm = first.iter().map(|value| value * value).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn empty_text_is_a_zero_vector() {
        let vector = embed("   ");
        assert!(vector.iter().all(|value| *value == 0.0));
        assert_eq!(cosine(&vector, &vector), 0.0);
    }

    #[test]
    fn similar_titles_are_closer_than_unrelated_ones() {
        let profile = embed("Rust 移动端性能优化");
        let similar = embed("Rust 移动端 UI 实践");
        let unrelated = embed("今天中午吃什么");
        assert!(cosine(&profile, &similar) > cosine(&profile, &unrelated));
        assert!(cosine(&profile, &similar) > 0.0);
    }

    #[test]
    fn provider_contract_is_stable() {
        let provider = HashEmbeddingProvider::default();
        assert_eq!(provider.model_id(), "hash-bigram-v1");
        assert_eq!(provider.dimension(), 192);
        let batch = provider
            .embed_batch(&["Rust".to_string(), String::new()])
            .unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].len(), 192);
        assert!(batch[1].iter().all(|value| *value == 0.0));
    }

    #[test]
    fn vector_profile_similarity_requires_center_and_item() {
        let provider = HashEmbeddingProvider::default();
        let center = provider.embed_text("Rust 移动端");
        let mut item = provider.embed_text("Rust 移动端实践");
        // Make the item differ from the center.
        item[0] += 0.5;
        normalize_vector(&mut item);
        let mut items = HashMap::new();
        items.insert(item_key("answer", "1"), item);
        let profile = VectorProfile::new(provider.model_id().to_string(), center, items);

        assert!(profile.contains("answer", "1"));
        assert!(!profile.contains("answer", "2"));
        assert!(profile.similarity("answer", "1").is_some());
        assert!(profile.similarity("answer", "2").is_none());
        assert_eq!(profile.item_count(), 1);

        let empty = VectorProfile::new(
            "m".to_string(),
            vec![0.0; 192],
            HashMap::from([(item_key("answer", "1"), vec![1.0; 192])]),
        );
        assert!(empty.similarity("answer", "1").is_none());
    }

    #[test]
    fn extend_adds_item_vectors() {
        let mut profile = VectorProfile::new("m".to_string(), vec![1.0, 0.0], HashMap::new());
        profile.extend([(item_key("pin", "9"), vec![1.0, 0.0])]);
        assert_eq!(profile.item_count(), 1);
        assert_eq!(profile.similarity("pin", "9"), Some(1.0));
    }

    #[test]
    fn dimension_is_clamped() {
        assert_eq!(HashEmbeddingProvider::new(1).dimension(), 32);
        assert_eq!(HashEmbeddingProvider::new(4096).dimension(), 1024);
    }
}
