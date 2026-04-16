use std::sync::Arc;

use crate::client::LlmClient;
use crate::error::Result;
use crate::session::SessionStore;
use crate::tools::ToolRegistry;

use super::Agent;

pub struct AgentBuilder {
    client: Arc<dyn LlmClient>,
    model: Option<String>,
    system_prompt: Option<String>,
    tools: Option<ToolRegistry>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    store: Option<Arc<dyn SessionStore>>,
    summarize: bool,
    context_messages: u32,
    summary_model: Option<String>,
    think: bool,
}

impl AgentBuilder {
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        Self {
            client,
            model: None,
            system_prompt: None,
            tools: None,
            temperature: None,
            max_tokens: None,
            store: None,
            summarize: true,
            context_messages: 3,
            summary_model: None,
            think: false,
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn summarize(mut self, enabled: bool) -> Self {
        self.summarize = enabled;
        self
    }

    pub fn context_messages(mut self, n: u32) -> Self {
        self.context_messages = n;
        self
    }

    pub fn summary_model(mut self, model: impl Into<String>) -> Self {
        self.summary_model = Some(model.into());
        self
    }

    pub fn think(mut self, enabled: bool) -> Self {
        self.think = enabled;
        self
    }

    pub fn build(self) -> Result<Agent> {
        let model = self.model.unwrap_or_else(|| "llama3.2".to_string());
        let tools = self.tools.unwrap_or_default();
        let summary_model = self.summary_model.unwrap_or_else(|| model.clone());

        Ok(Agent {
            client: self.client,
            model,
            system_prompt: self.system_prompt,
            tools,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            store: self.store,
            summarize: self.summarize,
            context_messages: self.context_messages,
            summary_model,
            think: self.think,
        })
    }
}
