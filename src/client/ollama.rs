use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;

use crate::client::{ChatRequest, LlmClient, OllamaChunk};
use crate::error::{AgentForgeError, Result};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";

pub struct OllamaClient {
    base_url: String,
    http: Client,
}

impl OllamaClient {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            http: Client::new(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    fn build_request(&self, request: ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": request.messages,
            "stream": request.stream,
        });

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(request.tools);
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        body
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<OllamaChunk>> + Send + 'static>>> {
        let url = format!("{}/api/chat", self.base_url);
        let body = self.build_request(request);

        let response = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentForgeError::Http(e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AgentForgeError::Ollama(format!(
                "HTTP {}: {}",
                status, text
            )));
        }

        let stream = response
            .bytes_stream()
            .map(|chunk_result| {
                chunk_result
                    .map_err(|e| AgentForgeError::Http(e))
                    .and_then(|bytes| {
                        let line = String::from_utf8(bytes.to_vec())
                            .map_err(|e| AgentForgeError::Agent(e.to_string()))?;
                        if line.trim().is_empty() {
                            return Ok(None);
                        }
                        let chunk: OllamaChunk = serde_json::from_str(&line)
                            .map_err(|e| AgentForgeError::Json(e))?;
                        Ok(Some(chunk))
                    })
            })
            .filter_map(|item| async { item.transpose() });

        Ok(Box::pin(stream))
    }
}