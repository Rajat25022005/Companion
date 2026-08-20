use std::collections::HashMap;
use std::sync::Arc;

use tracing::debug;

use companion_domain::{ModelError, ModelRequest, ModelResponse};
use crate::provider::ModelProvider;

/// Routes model requests to the appropriate provider based on model name.
pub struct ModelRouter {
    providers: HashMap<String, Arc<dyn ModelProvider>>,
    /// Maps model name → provider name (e.g., "gemma3:12b" → "ollama").
    model_to_provider: HashMap<String, String>,
    default_provider: Option<String>,
}

impl ModelRouter {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            model_to_provider: HashMap::new(),
            default_provider: None,
        }
    }

    /// Register a provider and its supported models.
    pub fn register(&mut self, provider: Arc<dyn ModelProvider>) {
        let name = provider.name().to_string();
        for model in provider.supported_models() {
            self.model_to_provider.insert(model, name.clone());
        }
        if self.default_provider.is_none() {
            self.default_provider = Some(name.clone());
        }
        self.providers.insert(name, provider);
    }

    /// Set the default provider (used when model name doesn't match any registered mapping).
    pub fn set_default(&mut self, provider_name: impl Into<String>) {
        self.default_provider = Some(provider_name.into());
    }

    /// Route a request to the appropriate provider.
    pub async fn route(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        let provider_name = if let Some(p) = self.model_to_provider.get(&request.model) {
            p
        } else if (request.model.starts_with("gemini") || request.model.starts_with("google/")) && self.providers.contains_key("gemini") {
            "gemini"
        } else if (request.model.starts_with("nvidia") || request.model.starts_with("meta/") || request.model.starts_with("mistralai/")) && self.providers.contains_key("nvidia") {
            "nvidia"
        } else {
            self.default_provider.as_deref().ok_or_else(|| ModelError::ModelNotFound {
                model: request.model.clone(),
            })?
        };

        let provider = self.providers.get(provider_name).ok_or_else(|| {
            ModelError::ProviderUnavailable {
                provider: provider_name.to_string(),
            }
        })?;

        debug!(
            provider = %provider_name,
            model = %request.model,
            "routing model request"
        );

        provider.generate(request).await
    }

    /// Check health of all registered providers.
    pub async fn health(&self) -> HashMap<String, bool> {
        let mut results = HashMap::new();
        for (name, provider) in &self.providers {
            results.insert(name.clone(), provider.health_check().await);
        }
        results
    }

    /// List all registered providers and their models.
    pub fn list_providers(&self) -> Vec<(&str, Vec<String>)> {
        self.providers
            .iter()
            .map(|(name, p)| (name.as_str(), p.supported_models()))
            .collect()
    }
}
