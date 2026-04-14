pub mod file_read;
pub mod file_write;
pub mod file_list;
pub mod file_search;

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use file_list::FileListTool;
pub use file_search::{FileSearchTool, GrepContentTool};

use crate::error::{AgentForgeError, Result};

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value) -> Result<String>;
}

#[derive(Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDefinition {
    pub fn from_executor(executor: &Arc<dyn ToolExecutor>) -> Self {
        Self {
            name: executor.name().to_string(),
            description: executor.description().to_string(),
            parameters: executor.parameters_schema(),
        }
    }
}

#[derive(Clone)]
pub struct ToolRegistry {
    executors: HashMap<String, Arc<dyn ToolExecutor>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
        }
    }

    pub fn register<E: ToolExecutor + 'static>(&mut self, executor: E) -> &mut Self {
        self.executors.insert(executor.name().to_string(), Arc::new(executor));
        self
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry
            .register(FileReadTool::new())
            .register(FileWriteTool::new())
            .register(FileListTool::new())
            .register(FileSearchTool::new())
            .register(GrepContentTool::new());
        registry
    }

    pub fn get(&self, name: &str) -> Option<&dyn ToolExecutor> {
        self.executors.get(name).map(|arc| arc.as_ref())
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.executors
            .values()
            .map(|arc| ToolDefinition::from_executor(arc))
            .collect()
    }

    pub async fn execute(&self, name: &str, args: Value) -> Result<String> {
        let executor = self
            .executors
            .get(name)
            .ok_or_else(|| AgentForgeError::ToolNotFound(name.to_string()))?;
        executor.execute(args).await
    }

    pub fn contains(&self, name: &str) -> bool {
        self.executors.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
