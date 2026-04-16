use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct CreateDirectoryTool {
    workdir: Option<PathBuf>,
}

impl CreateDirectoryTool {
    pub fn new() -> Self {
        Self { workdir: None }
    }

    pub fn with_workdir(mut self, workdir: PathBuf) -> Self {
        self.workdir = Some(workdir);
        self
    }

    fn resolve(&self, path: &str) -> Result<PathBuf> {
        match &self.workdir {
            Some(base) => crate::tools::resolve_path_allow_create(base, path),
            None => Ok(PathBuf::from(path)),
        }
    }
}

#[async_trait]
impl ToolExecutor for CreateDirectoryTool {
    fn name(&self) -> &str {
        "create_directory"
    }

    fn description(&self) -> &str {
        "Create a directory and any missing parent directories. Returns an error if the directory already exists as a file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path of the directory to create"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("path must be a string".to_string()))?;

        let resolved = self.resolve(path)?;

        if resolved.exists() {
            if resolved.is_dir() {
                return Ok(format!("Directory already exists: {}", path));
            }
            return Err(crate::error::AgentForgeError::ToolExecution(
                format!("Path '{}' exists but is not a directory", path),
            ));
        }

        tokio::fs::create_dir_all(&resolved)
            .await
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to create directory '{}': {}", path, e)))?;

        Ok(format!("Created directory: {}", path))
    }
}