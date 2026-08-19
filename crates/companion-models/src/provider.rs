use async_trait::async_trait;
use companion_domain::{ModelError, ModelRequest, ModelResponse};

/// Provider-agnostic model interface.
///
/// Every model provider (Ollama, Gemini, NVIDIA NIM) implements this trait.
/// The runtime never calls a provider directly — it goes through the ModelRouter.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Human-readable provider name (e.g., "ollama", "gemini", "nvidia").
    fn name(&self) -> &str;

    /// List of model identifiers this provider can serve.
    fn supported_models(&self) -> Vec<String>;

    /// Generate a response from the model.
    async fn generate(&self, request: ModelRequest) -> Result<ModelResponse, ModelError>;

    /// Whether this provider supports tool/function calling.
    fn supports_tools(&self) -> bool;

    /// Whether this provider supports streaming responses.
    fn supports_streaming(&self) -> bool;

    /// Health check — returns true if the provider is reachable.
    async fn health_check(&self) -> bool;
}
