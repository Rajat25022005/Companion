use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use companion_domain::RuntimeError;

/// Trait for generating vector embeddings from text strings.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embedding vector for a single text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RuntimeError>;

    /// Generate embeddings for a batch of texts.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RuntimeError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// Vector dimension produced by this provider.
    fn dimension(&self) -> usize;
}

// ---------------------------------------------------------------------------
// Ollama Embedding Provider
// ---------------------------------------------------------------------------

pub struct OllamaEmbeddingProvider {
    client: Client,
    base_url: String,
    model: String,
    dimension: usize,
}

impl OllamaEmbeddingProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            base_url: base_url.into(),
            model: model.into(),
            dimension: 768,
        }
    }
}

#[derive(Serialize)]
struct OllamaEmbedRequest {
    model: String,
    prompt: String,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RuntimeError> {
        let url = format!("{}/api/embeddings", self.base_url);
        let req = OllamaEmbedRequest {
            model: self.model.clone(),
            prompt: text.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| RuntimeError::Internal(format!("Ollama embed network error: {e}")))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Internal(format!(
                "Ollama embed failed with status {}",
                response.status()
            )));
        }

        let body: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| RuntimeError::Internal(format!("Ollama embed parse error: {e}")))?;

        Ok(body.embedding)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

// ---------------------------------------------------------------------------
// Google Gemini Embedding Provider
// ---------------------------------------------------------------------------

pub struct GeminiEmbeddingProvider {
    client: Client,
    api_key: String,
    model: String,
    dimension: usize,
}

impl GeminiEmbeddingProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            api_key: api_key.into(),
            model: "text-embedding-004".into(),
            dimension: 768,
        }
    }
}

#[derive(Serialize)]
struct GeminiEmbedRequest {
    content: GeminiEmbedContent,
}

#[derive(Serialize)]
struct GeminiEmbedContent {
    parts: Vec<GeminiEmbedPart>,
}

#[derive(Serialize)]
struct GeminiEmbedPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiEmbedResponse {
    embedding: GeminiEmbeddingValue,
}

#[derive(Deserialize)]
struct GeminiEmbeddingValue {
    values: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for GeminiEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RuntimeError> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent?key={}",
            self.model, self.api_key
        );

        let req = GeminiEmbedRequest {
            content: GeminiEmbedContent {
                parts: vec![GeminiEmbedPart {
                    text: text.to_string(),
                }],
            },
        };

        let response = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| RuntimeError::Internal(format!("Gemini embed network error: {e}")))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Internal(format!(
                "Gemini embed failed with status {}",
                response.status()
            )));
        }

        let body: GeminiEmbedResponse = response
            .json()
            .await
            .map_err(|e| RuntimeError::Internal(format!("Gemini embed parse error: {e}")))?;

        Ok(body.embedding.values)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

// ---------------------------------------------------------------------------
// Fast Deterministic Mock Vectorizer (For testing and offline use)
// ---------------------------------------------------------------------------

/// Generates deterministic 128-dimensional unit vectors using word hashing and bag-of-words.
pub struct MockEmbeddingProvider {
    dimension: usize,
}

impl MockEmbeddingProvider {
    pub fn new() -> Self {
        Self { dimension: 128 }
    }
}

impl Default for MockEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RuntimeError> {
        let mut vector = vec![0.0f32; self.dimension];
        let words = text.to_lowercase();
        let tokens: Vec<&str> = words.split_whitespace().collect();

        if tokens.is_empty() {
            return Ok(vector);
        }

        for token in tokens {
            let mut hasher = Sha256::new();
            hasher.update(token.as_bytes());
            let hash = hasher.finalize();

            // Project hash onto dimensions
            for i in 0..self.dimension {
                let byte = hash[i % hash.len()];
                vector[i] += (byte as f32 / 255.0) - 0.5;
            }
        }

        // L2 Normalization (unit length)
        let norm: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut vector {
                *val /= norm;
            }
        }

        Ok(vector)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
