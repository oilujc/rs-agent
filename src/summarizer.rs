use std::sync::Arc;

use futures::StreamExt;

use crate::client::{user_message, ChatRequest, LlmClient};
use crate::error::Result;

const SUMMARY_SYSTEM_PROMPT: &str = "You are a conversation summarizer. Your task is to produce a concise summary of the conversation. Focus on key facts, decisions, user intents, and any important context. Do not add information that was not discussed. Keep the summary brief and informative.";

const SUMMARY_PROMPT_TEMPLATE: &str = "Summarize the following conversation concisely. Include key facts, decisions, and important context.";

const SUMMARY_UPDATE_TEMPLATE: &str = "Here is the previous summary of the conversation:\n\n{}\n\nNow update it with the new messages below. Produce an updated concise summary that incorporates both the previous context and the new information.";

#[derive(Clone)]
pub struct Summarizer {
    client: Arc<dyn LlmClient>,
    model: String,
    temperature: f32,
}

impl Summarizer {
    pub fn new(client: Arc<dyn LlmClient>, model: String, temperature: f32) -> Self {
        Self {
            client,
            model,
            temperature,
        }
    }

    pub async fn summarize(&self, messages: &[serde_json::Value], existing_summary: Option<&str>) -> Result<String> {
        let mut prompt_messages = vec![crate::client::system_message(SUMMARY_SYSTEM_PROMPT)];

        let user_content = match existing_summary {
            Some(summary) => format!("{}\n\n---\n\nConversation messages:\n{}", SUMMARY_UPDATE_TEMPLATE.replace("{}", summary), format_messages(messages)),
            None => format!("{}\n\n---\n\nConversation messages:\n{}", SUMMARY_PROMPT_TEMPLATE, format_messages(messages)),
        };

        prompt_messages.push(user_message(&user_content));

        let request = ChatRequest {
            model: self.model.clone(),
            messages: prompt_messages,
            tools: Vec::new(),
            stream: true,
            temperature: Some(self.temperature),
            max_tokens: None,
            think: None,
        };

        let stream = self.client.chat_stream(request).await?;

        let mut summary = String::new();
        let mut stream = Box::pin(stream);

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            if let Some(message) = chunk.message {
                if !message.content.is_empty() {
                    summary.push_str(&message.content);
                }
            }
        }

        Ok(summary.trim().to_string())
    }
}

fn format_messages(messages: &[serde_json::Value]) -> String {
    let mut formatted = String::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("unknown");
        if role == "system" {
            continue;
        }
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        if content.is_empty() {
            if let Some(tool_calls) = msg.get("tool_calls") {
                formatted.push_str(&format!("{}: [tool call: {}]\n", role, tool_calls));
                continue;
            }
            continue;
        }
        formatted.push_str(&format!("{}: {}\n", role, content));
    }
    formatted
}