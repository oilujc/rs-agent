use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct FileListTool {
    workdir: Option<PathBuf>,
}

impl FileListTool {
    pub fn new() -> Self {
        Self { workdir: None }
    }

    pub fn with_workdir(mut self, workdir: PathBuf) -> Self {
        self.workdir = Some(workdir);
        self
    }

    fn resolve(&self, path: &str) -> Result<PathBuf> {
        match &self.workdir {
            Some(base) => crate::tools::resolve_path(base, path),
            None => Ok(PathBuf::from(path)),
        }
    }
}

#[async_trait]
impl ToolExecutor for FileListTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List contents of a directory. Shows files and subdirectories."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list"
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

        let mut entries = tokio::fs::read_dir(&resolved)
            .await
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to read directory {}: {}", path, e)))?;

        let mut result = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to read directory entry: {}", e)))? {
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = entry.file_type()
                .await
                .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to get file type: {}", e)))?;
            let type_str = if file_type.is_dir() {
                "[DIR]"
            } else if file_type.is_file() {
                "[FILE]"
            } else {
                "[UNKNOWN]"
            };
            result.push(format!("{} {}", type_str, name));
        }

        Ok(result.join("\n"))
    }
}