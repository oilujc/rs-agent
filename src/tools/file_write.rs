use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct FileWriteTool {
    workdir: Option<PathBuf>,
}

impl FileWriteTool {
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
impl ToolExecutor for FileWriteTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist. Set 'append' to true to add content to the end of an existing file instead of overwriting."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to write the file to"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append content to the end of the file instead of overwriting. Defaults to false."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("path must be a string".to_string()))?;

        let content = args["content"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("content must be a string".to_string()))?;

        let append = args["append"].as_bool().unwrap_or(false);

        let resolved = self.resolve(path)?;

        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to create parent directory: {}", e)))?;
        }

        if append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&resolved)
                .await
                .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to open file for append {}: {}", path, e)))?;

            file.write_all(content.as_bytes())
                .await
                .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to append to file {}: {}", path, e)))?;

            Ok(format!("Appended {} bytes to {}", content.len(), path))
        } else {
            tokio::fs::write(&resolved, content)
                .await
                .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to write file {}: {}", path, e)))?;

            Ok(format!("Successfully wrote {} bytes to {}", content.len(), path))
        }
    }
}