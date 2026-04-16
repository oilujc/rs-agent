use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::client::{ChatRequest, LlmClient, OllamaChunk, OllamaMessage, OllamaToolCall, OllamaFunction};
use crate::error::{AgentForgeError, Result};

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

pub struct OpenRouterClient {
    api_key: String,
    base_url: String,
    http: Client,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
            http: Client::new(),
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    fn build_request(&self, request: &ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": request.messages,
            "stream": true,
        });

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(request.tools);
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        body
    }
}

#[derive(Debug, Clone, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Debug, Clone, Deserialize)]
struct Choice {
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default, rename = "reasoning_content")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallChunk>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolCallChunk {
    #[serde(default)]
    index: Option<usize>,
    function: ToolFunctionChunk,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolFunctionChunk {
    name: Option<String>,
    arguments: Option<Value>,
}

fn convert_chunk(value: &Value) -> Option<OllamaChunk> {
    let chunk: StreamChunk = serde_json::from_value(value.clone()).ok()?;
    let first = chunk.choices.first()?;

    let mut content = String::new();
    let mut thinking: Option<String> = None;
    let mut tool_calls: Vec<OllamaToolCall> = Vec::new();

    if let Some(c) = &first.delta.content {
        content = c.clone();
    }

    if let Some(rc) = &first.delta.reasoning_content {
        thinking = Some(rc.clone());
    }

    if let Some(tc_chunks) = &first.delta.tool_calls {
        for tc in tc_chunks {
            let name = tc.function.name.clone().unwrap_or_default();
            let arguments = tc.function.arguments.clone().unwrap_or(Value::Object(serde_json::Map::new()));
            let index = tc.index;
            tool_calls.push(OllamaToolCall {
                index,
                function: OllamaFunction {
                    name,
                    arguments,
                },
            });
        }
    }

    Some(OllamaChunk {
        model: String::new(),
        message: Some(OllamaMessage {
            role: "assistant".to_string(),
            content,
            thinking,
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        }),
        done: first.finish_reason.as_ref().map(|r| r == "stop" || r == "tool_calls"),
    })
}

fn parse_sse_bytes(bytes: &[u8]) -> Vec<Option<OllamaChunk>> {
    let text = String::from_utf8_lossy(bytes);
    let mut results = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" {
            continue;
        }
        let data = match trimmed.strip_prefix("data: ") {
            Some(d) => d,
            None => {
                if trimmed.starts_with('{') {
                    trimmed
                } else {
                    continue;
                }
            }
        };
        let parsed: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        results.push(convert_chunk(&parsed));
    }

    results
}

#[async_trait]
impl LlmClient for OpenRouterClient {
    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<OllamaChunk>> + Send + 'static>>> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_request(&request);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
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
                        let chunks = parse_sse_bytes(&bytes);
                        Ok(chunks)
                    })
            })
            .filter_map(|item| async {
                match item {
                    Ok(chunks) => {
                        let results: Vec<Result<OllamaChunk>> = chunks
                            .into_iter()
                            .filter_map(|c| c)
                            .map(Ok)
                            .collect();
                        Some(futures::stream::iter(results))
                    }
                    Err(e) => Some(futures::stream::iter(vec![Err(e)])),
                }
            })
            .flatten();

        Ok(Box::pin(stream))
    }
}