use serde::{Deserialize, Serialize};

use crate::capability::{ToolCall, ToolDefinition};

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// A single message in a conversation with a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Tool calls proposed by the assistant (only for Role::Assistant).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Tool call ID this message is responding to (only for Role::Tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn assistant_with_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: String::new(),
            tool_calls,
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Message role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

// ---------------------------------------------------------------------------
// Model Request
// ---------------------------------------------------------------------------

/// A provider-agnostic request to a language model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    /// The model to use (e.g., "gemma3:12b", "gemini-2.5-flash").
    pub model: String,

    /// Conversation messages.
    pub messages: Vec<Message>,

    /// Available tools.
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,

    /// Tool calling policy.
    #[serde(default)]
    pub tool_choice: ToolChoice,

    /// Sampling temperature.
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,

    /// Whether to stream the response.
    #[serde(default)]
    pub stream: bool,
}

fn default_temperature() -> f32 {
    0.7
}

/// Tool calling policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    /// Model decides whether to call tools.
    #[default]
    Auto,
    /// Model must call at least one tool.
    Required,
    /// Model must not call any tools.
    None,
    /// Model must call a specific tool.
    Specific(String),
}

// ---------------------------------------------------------------------------
// Model Response
// ---------------------------------------------------------------------------

/// A provider-agnostic response from a language model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    /// The model that generated this response.
    pub model: String,

    /// The text content of the response.
    pub content: String,

    /// Tool calls proposed by the model.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,

    /// Usage statistics.
    pub usage: Usage,

    /// Whether the model finished generating.
    pub finish_reason: FinishReason,

    /// Provider-specific metadata.
    #[serde(default)]
    pub provider_metadata: serde_json::Value,
}

impl ModelResponse {
    /// Whether the model proposed any tool calls.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// Whether the response is only text (no tool calls).
    pub fn is_text_only(&self) -> bool {
        self.tool_calls.is_empty() && !self.content.is_empty()
    }
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Model finished naturally.
    Stop,
    /// Model wants to call tools.
    ToolCalls,
    /// Hit the token limit.
    Length,
    /// Content was filtered.
    ContentFilter,
    /// Unknown / provider-specific reason.
    Other(String),
}

// ---------------------------------------------------------------------------
// Model Error
// ---------------------------------------------------------------------------

/// Errors from model provider operations.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum ModelError {
    #[error("provider not available: {provider}")]
    ProviderUnavailable { provider: String },

    #[error("model not found: {model}")]
    ModelNotFound { model: String },

    #[error("authentication failed: {message}")]
    AuthenticationFailed { message: String },

    #[error("rate limited, retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("request timeout after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    #[error("invalid request: {message}")]
    InvalidRequest { message: String },

    #[error("provider error: {message}")]
    ProviderError { message: String },

    #[error("content filtered: {message}")]
    ContentFiltered { message: String },

    #[error("network error: {message}")]
    NetworkError { message: String },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_constructors() {
        let sys = Message::system("You are helpful.");
        assert_eq!(sys.role, Role::System);
        assert_eq!(sys.content, "You are helpful.");

        let user = Message::user("Hello");
        assert_eq!(user.role, Role::User);
    }

    #[test]
    fn test_model_response_helpers() {
        let resp = ModelResponse {
            model: "test".into(),
            content: "Hello".into(),
            tool_calls: vec![],
            usage: Usage::default(),
            finish_reason: FinishReason::Stop,
            provider_metadata: serde_json::Value::Null,
        };
        assert!(resp.is_text_only());
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn test_serde_roundtrip() {
        let req = ModelRequest {
            model: "gemma3:12b".into(),
            messages: vec![Message::user("Hi")],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            temperature: 0.7,
            max_tokens: Some(1000),
            stream: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let restored: ModelRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.model, "gemma3:12b");
    }
}
