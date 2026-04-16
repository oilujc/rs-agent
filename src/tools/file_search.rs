use std::path::PathBuf;

use async_trait::async_trait;
use glob::glob;
use regex::Regex;
use serde_json::{json, Value};

use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct FileSearchTool {
    workdir: Option<PathBuf>,
}

impl FileSearchTool {
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
impl ToolExecutor for FileSearchTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern. Supports wildcards like *.rs, **/*.md, etc."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '**/*.rs' for all Rust files)"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory to search from (defaults to current directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("pattern must be a string".to_string()))?;

        let base_path = args["path"]
            .as_str()
            .unwrap_or(".");

        let base = self.resolve(base_path)?;

        let search_pattern = if pattern.starts_with('/') {
            pattern.to_string()
        } else {
            format!("{}/{}", base.display(), pattern)
        };

        let matches: Vec<String> = glob(&search_pattern)
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Invalid glob pattern: {}", e)))?
            .filter_map(|entry| entry.ok())
            .map(|path| path.to_string_lossy().to_string())
            .collect();

        if matches.is_empty() {
            Ok("No files found matching the pattern.".to_string())
        } else {
            Ok(matches.join("\n"))
        }
    }
}

pub struct GrepContentTool {
    workdir: Option<PathBuf>,
}

impl GrepContentTool {
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
impl ToolExecutor for GrepContentTool {
    fn name(&self) -> &str {
        "grep_content"
    }

    fn description(&self) -> &str {
        "Search for text content within files using regular expressions. Returns matching lines with file paths."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Path to search in (file or directory)"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Whether search should be case sensitive (default: false)"
                }
            },
            "required": ["pattern", "path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("pattern must be a string".to_string()))?;

        let path = args["path"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("path must be a string".to_string()))?;

        let case_sensitive = args["case_sensitive"]
            .as_bool()
            .unwrap_or(false);

        let regex_pattern = if case_sensitive {
            pattern.to_string()
        } else {
            format!("(?i){}", pattern)
        };

        let re = Regex::new(&regex_pattern)
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Invalid regex pattern: {}", e)))?;

        let search_path = self.resolve(path)?;
        let mut results = Vec::new();

        if search_path.is_file() {
            if let Ok(content) = tokio::fs::read_to_string(&search_path).await {
                for (line_num, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        results.push(format!("{}:{}: {}", search_path.display(), line_num + 1, line));
                    }
                }
            }
        } else if search_path.is_dir() {
            let mut stack = vec![search_path.clone()];

            while let Some(current_dir) = stack.pop() {
                let mut entries = tokio::fs::read_dir(&current_dir)
                    .await
                    .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to read directory: {}", e)))?;

                while let Some(entry) = entries.next_entry().await.map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to read entry: {}", e)))? {
                    let entry_path = entry.path();

                    if entry_path.is_dir() {
                        if let Some(name) = entry_path.file_name().map(|n| n.to_string_lossy().to_string()) {
                            if !name.starts_with('.') {
                                stack.push(entry_path);
                            }
                        }
                    } else if let Ok(content) = tokio::fs::read_to_string(&entry_path).await {
                        for (line_num, line) in content.lines().enumerate() {
                            if re.is_match(line) {
                                results.push(format!("{}:{}: {}", entry_path.display(), line_num + 1, line));
                            }
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok("No matches found.".to_string())
        } else {
            Ok(results.join("\n"))
        }
    }
}