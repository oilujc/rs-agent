pub mod ollama;

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub stream: bool,
    pub temperature: Option<f32>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Value>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: Vec::new(),
            stream: true,
            temperature: None,
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
    #[serde(rename = "tool_calls", default)]
    pub tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaToolCall {
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