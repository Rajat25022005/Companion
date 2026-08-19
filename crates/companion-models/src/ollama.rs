use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use companion_domain::*;
use crate::provider::ModelProvider;

/// Ollama model provider.
///
/// Communicates with a local Ollama instance via `/api/chat`.
pub struct OllamaProvider {
    client: Client,
    base_url: String,
    models: Vec<String>,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            base_url: base_url.into(),
            models: vec![],
        }
    }

    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    fn build_request(&self, request: &ModelRequest) -> OllamaChatRequest {
        let messages: Vec<OllamaMessage> = request
            .messages
            .iter()
            .map(|m| OllamaMessage {
                role: match m.role {
                    Role::System => "system".into(),
                    Role::User => "user".into(),
                    Role::Assistant => "assistant".into(),
                    Role::Tool => "tool".into(),
                },
                content: m.content.clone(),
                tool_calls: if m.tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        m.tool_calls
                            .iter()
                            .map(|tc| OllamaToolCall {
                                function: OllamaFunction {
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.clone(),
                                },
                            })
                            .collect(),
                    )
                },
            })
            .collect();

        let tools: Option<Vec<OllamaTool>> = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| OllamaTool {
                        r#type: "function".into(),
                        function: OllamaToolDef {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        };

        let model = if request.model == "default" || request.model.is_empty() {
            self.models
                .first()
                .cloned()
                .unwrap_or_else(|| "minimax-m3:cloud".into())
        } else {
            request.model.clone()
        };

        OllamaChatRequest {
            model,
            messages,
            tools,
            stream: false,
            options: Some(OllamaOptions {
                temperature: Some(request.temperature),
                num_ctx: Some(8192),
            }),
        }
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn supported_models(&self) -> Vec<String> {
        self.models.clone()
    }

    async fn generate(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        let url = format!("{}/api/chat", self.base_url);
        let ollama_req = self.build_request(&request);

        debug!(provider = "ollama", model = %request.model, "sending request");

        let response = self
            .client
            .post(&url)
            .json(&ollama_req)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ModelError::Timeout { timeout_secs: 120 }
                } else if e.is_connect() {
                    ModelError::ProviderUnavailable {
                        provider: "ollama".into(),
                    }
                } else {
                    ModelError::NetworkError {
                        message: e.to_string(),
                    }
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(provider = "ollama", status = %status, body = %body, "request failed");
            return Err(ModelError::ProviderError {
                message: format!("HTTP {status}: {body}"),
            });
        }

        let ollama_resp: OllamaChatResponse = response.json().await.map_err(|e| {
            ModelError::ProviderError {
                message: format!("failed to parse response: {e}"),
            }
        })?;

        let tool_calls: Vec<ToolCall> = ollama_resp
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, tc)| ToolCall {
                id: format!("call_{i}"),
                name: tc.function.name,
                arguments: tc.function.arguments,
            })
            .collect();

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        Ok(ModelResponse {
            model: ollama_resp.model.unwrap_or_else(|| request.model.clone()),
            content: ollama_resp.message.content,
            tool_calls,
            usage: Usage {
                prompt_tokens: ollama_resp.prompt_eval_count.unwrap_or(0),
                completion_tokens: ollama_resp.eval_count.unwrap_or(0),
                total_tokens: ollama_resp
                    .prompt_eval_count
                    .unwrap_or(0)
                    .saturating_add(ollama_resp.eval_count.unwrap_or(0)),
            },
            finish_reason,
            provider_metadata: serde_json::json!({
                "provider": "ollama",
                "total_duration_ns": ollama_resp.total_duration,
            }),
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client.get(&url).send().await.is_ok()
    }
}

// ---------------------------------------------------------------------------
// Ollama API types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaToolCall {
    function: OllamaFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    r#type: String,
    function: OllamaToolDef,
}

#[derive(Debug, Serialize)]
struct OllamaToolDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    model: Option<String>,
    message: OllamaMessage,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
    #[serde(default)]
    total_duration: Option<u64>,
}
