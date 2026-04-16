use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::Result;
use crate::tools::ToolExecutor;

pub struct FileReadTool {
    workdir: Option<PathBuf>,
}

impl FileReadTool {
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

fn file_type_label(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".pdf") { return "PDF"; }
    if lower.ends_with(".png") { return "PNG"; }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") { return "JPEG"; }
    if lower.ends_with(".gif") { return "GIF"; }
    if lower.ends_with(".svg") { return "SVG"; }
    if lower.ends_with(".zip") { return "ZIP"; }
    if lower.ends_with(".tar") { return "TAR"; }
    if lower.ends_with(".gz") { return "GZIP"; }
    if lower.ends_with(".7z") { return "7-ZIP"; }
    if lower.ends_with(".exe") || lower.ends_with(".dll") { return "binary executable"; }
    if lower.ends_with(".woff") || lower.ends_with(".woff2") || lower.ends_with(".ttf") || lower.ends_with(".otf") { return "font"; }
    "binary"
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 { return format!("{}B", bytes); }
    if bytes < 1024 * 1024 { return format!("{:.1}KB", bytes as f64 / 1024.0); }
    format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
}

#[async_trait]
impl ToolExecutor for FileReadTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the filesystem. Returns the file's text content. Use 'offset' and 'limit' to read specific line ranges for large files. Binary files (PDF, images, etc.) cannot be read as text."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-indexed line number to start reading from. Defaults to 1 (beginning of file)."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return. Defaults to all lines."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| crate::error::AgentForgeError::InvalidRequest("path must be a string".to_string()))?;

        let offset = args["offset"].as_u64().unwrap_or(1).max(1) as usize;
        let limit = args["limit"].as_u64().map(|l| l as usize);

        let resolved = self.resolve(path)?;
        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|e| crate::error::AgentForgeError::ToolExecution(format!("Failed to read file {}: {}", path, e)))?;

        let content = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(err) => {
                let byte_count = err.as_bytes().len();
                let label = file_type_label(path);
                return Ok(format!(
                    "[file: {} — {} file, cannot display as text. Size: {}]",
                    path, label, format_size(byte_count as u64)
                ));
            }
        };

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();

        if total_lines == 0 {
            return Ok(format!("[file: {}, 0 lines (empty)]\n", path));
        }

        let start = offset.saturating_sub(1);
        if start >= total_lines {
            return Ok(format!(
                "[file: {}, lines {}-{} of {}]\n(offset is past end of file)",
                path, offset, offset, total_lines
            ));
        }

        let end = match limit {
            Some(l) => (start + l).min(total_lines),
            None => total_lines,
        };

        let selected: Vec<&str> = all_lines[start..end].to_vec();
        let selected_text = selected.join("\n");

        let display_start = start + 1;
        let display_end = end;

        let header = if display_start == 1 && display_end == total_lines {
            format!("[file: {}, {} lines]", path, total_lines)
        } else {
            let more = if display_end < total_lines { ", more below" } else { "" };
            format!(
                "[file: {}, lines {}-{} of {}{}]",
                path, display_start, display_end, total_lines, more
            )
        };

        Ok(format!("{}\n{}", header, selected_text))
    }
}