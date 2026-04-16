pub mod builder;

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

use crate::client::LlmClient;
use crate::error::Result;
use crate::event::{Event, ThreadId};
use crate::memory::{MemoryGetTool, MemorySetTool};
use crate::session::{Session, SessionStore};
use crate::summarizer::Summarizer;
use crate::tools::ToolRegistry;

pub struct Agent {
    client: Arc<dyn LlmClient>,
    model: String,
    system_prompt: Option<String>,
    tools: ToolRegistry,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    store: Option<Arc<dyn SessionStore>>,
    summarize: bool,
    context_messages: u32,
    summary_model: String,
    think: bool,
}

impl Agent {
    pub fn builder(client: Arc<dyn LlmClient>) -> builder::AgentBuilder {
        builder::AgentBuilder::new(client)
    }

    pub fn session(&self) -> Session {
        self.session_with_id(ThreadId::random())
    }

    pub fn session_with_id(&self, thread_id: ThreadId) -> Session {
        let (prior_messages, prior_state, prior_summary) = match self.store.as_ref() {
            Some(store) => match store.load(&thread_id.to_string()) {
                Ok(Some(data)) => (data.messages, data.state, data.summary),
                _ => (Vec::new(), Value::Object(serde_json::Map::new()), None),
            },
            None => (Vec::new(), Value::Object(serde_json::Map::new()), None),
        };

        let state = Arc::new(RwLock::new(prior_state));

        let mut tools = self.tools.clone();
        tools.register(MemorySetTool::new(state.clone()));
        tools.register(MemoryGetTool::new(state.clone()));

        let summarizer = if self.summarize {
            Some(Summarizer::new(
                self.client.clone(),
                self.summary_model.clone(),
                self.temperature.unwrap_or(0.3),
            ))
        } else {
            None
        };

        Session::new(
            thread_id,
            self.client.clone(),
            self.model.clone(),
            self.system_prompt.clone(),
            tools,
            self.temperature,
            state,
            self.store.clone(),
        )
        .with_initial_messages(prior_messages)
        .with_summary(prior_summary)
        .with_summarization(self.summarize, self.context_messages, summarizer)
        .with_think(self.think)
        .with_max_tokens_opt(self.max_tokens)
    }

    pub async fn run_once(
        &self,
        user_message: &str,
    ) -> Result<futures::channel::mpsc::UnboundedReceiver<Result<Event>>> {
        let mut session = self.session();
        session.run(user_message).await
    }
}