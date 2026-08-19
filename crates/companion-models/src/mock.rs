use std::sync::Mutex;
use async_trait::async_trait;
use companion_domain::{FinishReason, ModelError, ModelRequest, ModelResponse, ToolCall, Usage};
use crate::provider::ModelProvider;

/// Mock model provider that returns pre-configured responses or closures.
pub struct MockModelProvider {
    name: String,
    responses: Mutex<Vec<Result<ModelResponse, ModelError>>>,
}

impl MockModelProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            responses: Mutex::new(Vec::new()),
        }
    }

    /// Add a text response to the queue.
    pub fn push_text_response(&self, text: impl Into<String>) {
        let resp = ModelResponse {
            model: "mock-model".into(),
            content: text.into(),
            tool_calls: Vec::new(),
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
            provider_metadata: serde_json::Value::Null,
        };
        self.responses.lock().unwrap().push(Ok(resp));
    }

    /// Add a tool call response to the queue.
    pub fn push_tool_call_response(&self, tool_calls: Vec<ToolCall>) {
        let resp = ModelResponse {
            model: "mock-model".into(),
            content: String::new(),
            tool_calls,
            usage: Usage::default(),
            finish_reason: FinishReason::ToolCalls,
            provider_metadata: serde_json::Value::Null,
        };
        self.responses.lock().unwrap().push(Ok(resp));
    }

    /// Add an error response to the queue.
    pub fn push_error(&self, error: ModelError) {
        self.responses.lock().unwrap().push(Err(error));
    }
}

#[async_trait]
impl ModelProvider for MockModelProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".into(), "default".into()]
    }

    async fn generate(&self, _request: ModelRequest) -> Result<ModelResponse, ModelError> {
        let mut queue = self.responses.lock().unwrap();
        if queue.is_empty() {
            // Default fallback
            Ok(ModelResponse {
                model: "mock-model".into(),
                content: "Default mock response".into(),
                tool_calls: Vec::new(),
                usage: Usage::default(),
                finish_reason: FinishReason::Stop,
                provider_metadata: serde_json::Value::Null,
            })
        } else {
            queue.remove(0)
        }
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn health_check(&self) -> bool {
        true
    }
}
