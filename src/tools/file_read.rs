use async_trait::async_trait;
use serde_json::{json, Value};
use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct FileReadTool;

impl FileReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolExecutor for FileReadTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the filesystem. Returns the file's text content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("path must be a string".to_string()))?;

        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to read file {}: {}", path, e)))
    }
}
