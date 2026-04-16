pub mod file_read;
pub mod file_write;
pub mod file_list;
pub mod file_search;
pub mod dir_create;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use file_list::FileListTool;
pub use file_search::{FileSearchTool, GrepContentTool};
pub use dir_create::CreateDirectoryTool;

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
    workdir: Option<PathBuf>,
}

pub fn resolve_path(base: &Path, path: &str) -> Result<PathBuf> {
    let requested = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };

    let canonical_base = base.canonicalize()
        .map_err(|e| AgentForgeError::ToolExecution(format!("Invalid workdir '{}': {}", base.display(), e)))?;

    let canonical_requested = requested.canonicalize()
        .map_err(|e| AgentForgeError::ToolExecution(format!("Path '{}' does not exist: {}", path, e)))?;

    if !canonical_requested.starts_with(&canonical_base) {
        return Err(AgentForgeError::ToolExecution(format!(
            "Path '{}' is outside the working directory",
            path
        )));
    }

    Ok(canonical_requested)
}

pub fn resolve_path_allow_create(base: &Path, path: &str) -> Result<PathBuf> {
    let requested = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };

    let canonical_base = base.canonicalize()
        .map_err(|e| AgentForgeError::ToolExecution(format!("Invalid workdir '{}': {}", base.display(), e)))?;

    if requested.exists() {
        let canonical_requested = requested.canonicalize()
            .map_err(|e| AgentForgeError::ToolExecution(format!("Cannot canonicalize path '{}': {}", path, e)))?;
        if !canonical_requested.starts_with(&canonical_base) {
            return Err(AgentForgeError::ToolExecution(format!(
                "Path '{}' is outside the working directory",
                path
            )));
        }
        Ok(canonical_requested)
    } else {
        let parent = requested.parent()
            .ok_or_else(|| AgentForgeError::ToolExecution(format!("Invalid path '{}'", path)))?;

        if parent.exists() {
            let canonical_parent = parent.canonicalize()
                .map_err(|e| AgentForgeError::ToolExecution(format!("Cannot canonicalize parent of '{}': {}", path, e)))?;
            if !canonical_parent.starts_with(&canonical_base) {
                return Err(AgentForgeError::ToolExecution(format!(
                    "Path '{}' is outside the working directory",
                    path
                )));
            }
        } else {
            let mut current = requested.clone();
            let mut ancestors_to_check = Vec::new();
            while let Some(parent) = current.parent() {
                if parent.exists() {
                    ancestors_to_check.push(parent.to_path_buf());
                    break;
                }
                ancestors_to_check.push(parent.to_path_buf());
                current = parent.to_path_buf();
            }

            if let Some(existing_ancestor) = ancestors_to_check.iter().find(|p| p.exists()) {
                let canonical_ancestor = existing_ancestor.canonicalize()
                    .map_err(|e| AgentForgeError::ToolExecution(format!("Cannot verify path: {}", e)))?;
                if !canonical_ancestor.starts_with(&canonical_base) {
                    return Err(AgentForgeError::ToolExecution(format!(
                        "Path '{}' is outside the working directory",
                        path
                    )));
                }
            }
        }

        Ok(requested)
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
            workdir: None,
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry
            .register(FileReadTool::new())
            .register(FileWriteTool::new())
            .register(FileListTool::new())
            .register(FileSearchTool::new())
            .register(GrepContentTool::new())
            .register(CreateDirectoryTool::new());
        registry
    }

    pub fn with_workdir(mut self, workdir: PathBuf) -> Self {
        self.workdir = Some(workdir.clone());
        let w = workdir;
        self.executors.insert(
            "read_file".to_string(),
            Arc::new(FileReadTool::new().with_workdir(w.clone())),
        );
        self.executors.insert(
            "write_file".to_string(),
            Arc::new(FileWriteTool::new().with_workdir(w.clone())),
        );
        self.executors.insert(
            "list_directory".to_string(),
            Arc::new(FileListTool::new().with_workdir(w.clone())),
        );
        self.executors.insert(
            "search_files".to_string(),
            Arc::new(FileSearchTool::new().with_workdir(w.clone())),
        );
        self.executors.insert(
            "grep_content".to_string(),
            Arc::new(GrepContentTool::new().with_workdir(w.clone())),
        );
        self.executors.insert(
            "create_directory".to_string(),
            Arc::new(CreateDirectoryTool::new().with_workdir(w)),
        );
        self
    }

    pub fn workdir(&self) -> Option<&Path> {
        self.workdir.as_deref()
    }

    pub fn register<E: ToolExecutor + 'static>(&mut self, executor: E) -> &mut Self {
        self.executors.insert(executor.name().to_string(), Arc::new(executor));
        self
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