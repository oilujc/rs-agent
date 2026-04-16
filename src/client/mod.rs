pub mod ollama;
pub mod openrouter;

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use serde::Deserialize;
use serde_json::Value;

use crate::config::ProviderConfig;
use crate::error::{AgentForgeError, Result};

pub fn create_client(provider: &ProviderConfig) -> Result<Arc<dyn LlmClient>> {
    match provider.name.as_str() {
        "ollama" => {
            let client = ollama::OllamaClient::new().with_base_url(provider.url.clone());
            Ok(Arc::new(client))
        }
        "openrouter" => {
            let api_key = provider.api_key.as_ref().ok_or_else(|| {
                AgentForgeError::Config("OpenRouter provider requires an 'api_key'".to_string())
            })?;
            let client = openrouter::OpenRouterClient::new(api_key.clone())
                .with_base_url(provider.url.clone());
            Ok(Arc::new(client))
        }
        name => Err(AgentForgeError::Config(format!(
            "Unknown provider: '{}'. Available providers: ollama, openrouter",
            name
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub stream: bool,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub think: Option<bool>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Value>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: Vec::new(),
            stream: true,
            temperature: None,
            max_tokens: None,
            think: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<Value>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_think(mut self, think: bool) -> Self {
        self.think = Some(think);
        self
    }
}

pub fn user_message(content: &str) -> Value {
    serde_json::json!({"role": "user", "content": content})
}

pub fn system_message(content: &str) -> Value {
    serde_json::json!({"role": "system", "content": content})
}

pub fn assistant_message(content: &str) -> Value {
    serde_json::json!({"role": "assistant", "content": content})
}

pub fn tool_result_message(name: &str, content: &str) -> Value {
    serde_json::json!({"role": "tool", "name": name, "content": content})
}

pub fn tool_definition_to_ollama(
    name: &str,
    description: &str,
    parameters: &Value,
) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaChunk {
    pub model: String,
    #[serde(default)]
    pub message: Option<OllamaMessage>,
    #[serde(default)]
    pub done: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(rename = "tool_calls", default)]
    pub tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaToolCall {
    #[serde(default)]
    pub index: Option<usize>,
    pub function: OllamaFunction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaFunction {
    pub name: String,
    pub arguments: Value,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<OllamaChunk>> + Send + 'static>>>;
}