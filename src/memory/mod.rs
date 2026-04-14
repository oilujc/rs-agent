use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use std::sync::Arc;

use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct MemorySetTool {
    state: Arc<RwLock<Value>>,
}

impl MemorySetTool {
    pub fn new(state: Arc<RwLock<Value>>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl ToolExecutor for MemorySetTool {
    fn name(&self) -> &str {
        "memory_set"
    }

    fn description(&self) -> &str {
        "Store a key-value pair in the agent's session memory. The value persists across turns within the conversation."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "The key to store the value under"
                },
                "value": {
                    "description": "The value to store (can be string, number, object, or array)",
                }
            },
            "required": ["key", "value"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let key = args["key"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("key must be a string".to_string()))?;

        let value = args.get("value").cloned().unwrap_or(Value::Null);

        {
            let mut state = self.state.write().await;
            if let Some(obj) = state.as_object_mut() {
                obj.insert(key.to_string(), value.clone());
            } else {
                let mut map = serde_json::Map::new();
                map.insert(key.to_string(), value.clone());
                *state = Value::Object(map);
            }
        }

        Ok(format!("Stored '{}' in memory", key))
    }
}

pub struct MemoryGetTool {
    state: Arc<RwLock<Value>>,
}

impl MemoryGetTool {
    pub fn new(state: Arc<RwLock<Value>>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl ToolExecutor for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn description(&self) -> &str {
        "Retrieve a value from the agent's session memory by key. Returns the stored value or 'not found' if the key doesn't exist."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "The key to look up in memory"
                }
            },
            "required": ["key"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let key = args["key"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("key must be a string".to_string()))?;

        let state = self.state.read().await;
        match state.get(key) {
            Some(value) => Ok(serde_json::to_string_pretty(value)
                .unwrap_or_else(|_| value.to_string())),
            None => Ok(format!("Key '{}' not found in memory", key)),
        }
    }
}