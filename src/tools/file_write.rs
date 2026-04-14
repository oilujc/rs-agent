use async_trait::async_trait;
use serde_json::{json, Value};
use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct FileWriteTool;

impl FileWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolExecutor for FileWriteTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, overwrites if it does."
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

        tokio::fs::write(path, content)
            .await
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to write file {}: {}", path, e)))?;

        Ok(format!("Successfully wrote {} bytes to {}", content.len(), path))
    }
}
