use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use companion_domain::*;
use crate::provider::ModelProvider;

/// NVIDIA NIM model provider.
///
/// Uses the OpenAI-compatible chat completions API at `integrate.api.nvidia.com`.
pub struct NvidiaProvider {
    client: Client,
    api_key: String,
    base_url: String,
    models: Vec<String>,
}

impl NvidiaProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            api_key: api_key.into(),
            base_url: "https://integrate.api.nvidia.com/v1".into(),
            models: vec![
                "nvidia/llama-3.1-nemotron-70b-instruct".into(),
                "meta/llama-3.3-70b-instruct".into(),
            ],
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    fn build_request(&self, request: &ModelRequest) -> OpenAIChatRequest {
        let messages: Vec<OpenAIMessage> = request
            .messages
            .iter()
            .map(|m| {
                let tool_calls = if m.tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        m.tool_calls
                            .iter()
                            .enumerate()
                            .map(|(_i, tc)| OpenAIToolCall {
                                id: tc.id.clone(),
                                r#type: "function".into(),
                                function: OpenAIFunctionCall {
                                    name: tc.name.clone(),
                                    arguments: serde_json::to_string(&tc.arguments)
                                        .unwrap_or_default(),
                                },
                            })
                            .collect(),
                    )
                };

                OpenAIMessage {
                    role: match m.role {
                        Role::System => "system".into(),
                        Role::User => "user".into(),
                        Role::Assistant => "assistant".into(),
                        Role::Tool => "tool".into(),
                    },
                    content: Some(m.content.clone()),
                    tool_calls,
                    tool_call_id: m.tool_call_id.clone(),
                }
            })
            .collect();

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| OpenAITool {
                        r#type: "function".into(),
                        function: OpenAIFunctionDef {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        };

        OpenAIChatRequest {
            model: request.model.clone(),
            messages,
            tools,
            temperature: Some(request.temperature),
            max_tokens: request.max_tokens,
            stream: false,
        }
    }
}

#[async_trait]
impl ModelProvider for NvidiaProvider {
    fn name(&self) -> &str {
        "nvidia"
    }

    fn supported_models(&self) -> Vec<String> {
        self.models.clone()
    }

    async fn generate(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        let url = format!("{}/chat/completions", self.base_url);
        let oai_req = self.build_request(&request);

        debug!(provider = "nvidia", model = %request.model, "sending request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&oai_req)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ModelError::Timeout { timeout_secs: 120 }
                } else {
                    ModelError::NetworkError {
                        message: e.to_string(),
                    }
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!(provider = "nvidia", status = %status, "request failed");
            if status.as_u16() == 429 {
                return Err(ModelError::RateLimited {
                    retry_after_secs: 60,
                });
            }
            if status.as_u16() == 401 {
                return Err(ModelError::AuthenticationFailed {
                    message: body,
                });
            }
            return Err(ModelError::ProviderError {
                message: format!("HTTP {status}: {body}"),
            });
        }

        let oai_resp: OpenAIChatResponse = response.json().await.map_err(|e| {
            ModelError::ProviderError {
                message: format!("parse error: {e}"),
            }
        })?;

        let choice = oai_resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ModelError::ProviderError {
                message: "no choices returned".into(),
            })?;

        let tool_calls: Vec<ToolCall> = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let args: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::json!({}));
                ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments: args,
                }
            })
            .collect();

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("tool_calls") => FinishReason::ToolCalls,
            Some("length") => FinishReason::Length,
            Some("content_filter") => FinishReason::ContentFilter,
            _ => {
                if !tool_calls.is_empty() {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                }
            }
        };

        let usage = oai_resp.usage.map_or(Usage::default(), |u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(ModelResponse {
            model: oai_resp.model.unwrap_or_else(|| request.model.clone()),
            content: choice.message.content.unwrap_or_default(),
            tool_calls,
            usage,
            finish_reason,
            provider_metadata: serde_json::json!({"provider": "nvidia"}),
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/models", self.base_url);
        self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .is_ok()
    }
}

// ---------------------------------------------------------------------------
// OpenAI-compatible API types (shared by NVIDIA NIM)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIToolCall {
    id: String,
    r#type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    r#type: String,
    function: OpenAIFunctionDef,
}

#[derive(Debug, Serialize)]
struct OpenAIFunctionDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    model: Option<String>,
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}
