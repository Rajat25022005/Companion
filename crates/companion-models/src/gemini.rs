use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use companion_domain::*;
use crate::provider::ModelProvider;

/// Google Gemini model provider.
///
/// Communicates with the Gemini API via `generativelanguage.googleapis.com`.
pub struct GeminiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    models: Vec<String>,
}

impl GeminiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to create HTTP client"),
            api_key: api_key.into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            models: vec![
                "gemini-3.7-flash".into(),
                "gemini-2.5-flash".into(),
                "gemini-2.5-pro".into(),
                "gemini-2.0-flash".into(),
                "gemini-1.5-flash".into(),
                "gemini-1.5-pro".into(),
            ],
        }
    }

    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    fn build_request(&self, request: &ModelRequest) -> GeminiRequest {
        let contents: Vec<GeminiContent> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| GeminiContent {
                role: match m.role {
                    Role::User => "user".into(),
                    Role::Assistant => "model".into(),
                    Role::Tool => "function".into(),
                    Role::System => "user".into(), // filtered above
                },
                parts: vec![GeminiPart::Text {
                    text: m.content.clone(),
                }],
            })
            .collect();

        let system_instruction = request
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text {
                    text: m.content.clone(),
                }],
            });

        let tools = if request.tools.is_empty() {
            None
        } else {
            let declarations: Vec<GeminiFunctionDeclaration> = request
                .tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                })
                .collect();
            Some(vec![GeminiToolConfig {
                function_declarations: declarations,
            }])
        };

        GeminiRequest {
            contents,
            system_instruction,
            tools,
            generation_config: Some(GeminiGenerationConfig {
                temperature: Some(request.temperature),
                max_output_tokens: request.max_tokens,
            }),
        }
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn supported_models(&self) -> Vec<String> {
        self.models.clone()
    }

    async fn generate(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        let effective_model = if request.model == "default" || request.model == "gemini" {
            self.models.first().cloned().unwrap_or_else(|| "gemini-3.7-flash".into())
        } else if let Some(stripped) = request.model.strip_prefix("gemini/") {
            stripped.to_string()
        } else if let Some(stripped) = request.model.strip_prefix("google/") {
            stripped.to_string()
        } else {
            request.model.clone()
        };

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, effective_model, self.api_key
        );

        let gemini_req = self.build_request(&request);

        debug!(provider = "gemini", model = %effective_model, "sending request");

        let response = self
            .client
            .post(&url)
            .json(&gemini_req)
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
            error!(provider = "gemini", status = %status, "request failed");
            if status.as_u16() == 429 {
                return Err(ModelError::RateLimited {
                    retry_after_secs: 60,
                });
            }
            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ModelError::AuthenticationFailed {
                    message: body,
                });
            }
            return Err(ModelError::ProviderError {
                message: format!("HTTP {status}: {body}"),
            });
        }

        let gemini_resp: GeminiResponse = response.json().await.map_err(|e| {
            ModelError::ProviderError {
                message: format!("parse error: {e}"),
            }
        })?;

        // Extract content from first candidate
        let candidate = gemini_resp
            .candidates
            .into_iter()
            .next()
            .ok_or_else(|| ModelError::ProviderError {
                message: "no candidates returned".into(),
            })?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for part in candidate.content.parts {
            match part {
                GeminiPart::Text { text } => content.push_str(&text),
                GeminiPart::FunctionCall { function_call } => {
                    tool_calls.push(ToolCall {
                        id: format!("call_{}", tool_calls.len()),
                        name: function_call.name,
                        arguments: function_call.args,
                    });
                }
            }
        }

        let finish_reason = if !tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        let usage = gemini_resp.usage_metadata.map_or(Usage::default(), |u| Usage {
            prompt_tokens: u.prompt_token_count.unwrap_or(0),
            completion_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count.unwrap_or(0),
        });

        Ok(ModelResponse {
            model: effective_model,
            content,
            tool_calls,
            usage,
            finish_reason,
            provider_metadata: serde_json::json!({"provider": "gemini"}),
        })
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn health_check(&self) -> bool {
        let url = format!(
            "{}/models?key={}",
            self.base_url, self.api_key
        );
        self.client.get(&url).send().await.is_ok()
    }
}

// ---------------------------------------------------------------------------
// Gemini API types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
    FunctionCall { function_call: GeminiFunctionCall },
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolConfig {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsage>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsage {
    prompt_token_count: Option<u64>,
    candidates_token_count: Option<u64>,
    total_token_count: Option<u64>,
}
