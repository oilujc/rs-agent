pub mod builder;

use std::sync::Arc;

use ag_ui_core::types::ids::ThreadId;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::client::LlmClient;
use crate::error::Result;
use crate::memory::{MemoryGetTool, MemorySetTool};
use crate::session::{Session, SessionStore};
use crate::tools::ToolRegistry;

pub struct Agent {
    client: Arc<dyn LlmClient>,
    model: String,
    system_prompt: Option<String>,
    tools: ToolRegistry,
    temperature: Option<f32>,
    store: Option<Arc<dyn SessionStore>>,
}

impl Agent {
    pub fn builder(client: Arc<dyn LlmClient>) -> builder::AgentBuilder {
        builder::AgentBuilder::new(client)
    }

    pub fn session(&self) -> Session {
        self.session_with_id(ThreadId::random())
    }

    pub fn session_with_id(&self, thread_id: ThreadId) -> Session {
        let state = Arc::new(RwLock::new(Value::Object(serde_json::Map::new())));

        let mut tools = self.tools.clone();
        tools.register(MemorySetTool::new(state.clone()));
        tools.register(MemoryGetTool::new(state.clone()));

        Session::new(
            thread_id,
            self.client.clone(),
            self.model.clone(),
            self.system_prompt.clone(),
            tools,
            self.temperature,
            Value::Object(serde_json::Map::new()),
            self.store.clone(),
        )
    }

    pub async fn run_once(
        &self,
        user_message: &str,
    ) -> Result<futures::channel::mpsc::UnboundedReceiver<Result<ag_ui_core::event::Event>>> {
        let mut session = self.session();
        session.run(user_message).await
    }
}