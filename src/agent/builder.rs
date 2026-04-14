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
    store: Option<Arc<dyn SessionStore>>,
}

impl AgentBuilder {
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        Self {
            client,
            model: None,
            system_prompt: None,
            tools: None,
            temperature: None,
            store: None,
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

    pub fn store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn build(self) -> Result<Agent> {
        let model = self.model.unwrap_or_else(|| "llama3.2".to_string());
        let tools = self.tools.unwrap_or_default();

        Ok(Agent {
            client: self.client,
            model,
            system_prompt: self.system_prompt,
            tools,
            temperature: self.temperature,
            store: self.store,
        })
    }
}
