use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentForgeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Tool execution failed: {0}")]
    ToolExecution(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Ollama error: {0}")]
    Ollama(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, AgentForgeError>;
