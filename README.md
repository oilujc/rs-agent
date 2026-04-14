# agent-forge

A Rust framework for building and running AI agents powered by local LLMs via [Ollama](https://ollama.com). agent-forge implements the [AG-UI](https://github.com/ag-ui-protocol/ag-ui) protocol for streaming agent events and provides a modular, extensible architecture for tool-augmented conversational agents.

## Features

- **Local-first** — connects to Ollama running on your machine; no cloud API keys required
- **AG-UI compliant** — emits structured events (`StateSnapshot`, `StateDelta`, `TextMessageContent`, `ToolCallStart`, etc.) via an `mpsc` channel
- **Multi-turn agentic loop** — automatically streams LLM responses, executes tool calls, feeds results back, and repeats up to a configurable round limit
- **Session persistence** — thread-scoped `Session` with pluggable `SessionStore` (in-memory by default)
- **Agent prompts from Markdown** — define agent behavior with structured `## Section` Markdown files
- **Built-in file tools** — read, write, list directories, glob search, and regex grep; all opt-in
- **Session-scoped memory** — `memory_set`/`memory_get` tools let agents persist key-value state across turns
- **Builder pattern** — `AgentBuilder` and `Session` API for clean programmatic configuration
- **CLI** — run agents from the command line with `agent-forge`

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024, latest stable)
- [Ollama](https://ollama.com) running locally with at least one model pulled (e.g. `ollama pull llama3.2`)

## Getting Started

### Build

```bash
cargo build
```

### Run via CLI

```bash
# Simple one-shot message
agent-forge "Explain ownership in Rust"

# Use a specific model
agent-forge --model codellama "Write a Fibonacci function"

# Load a custom agent prompt from a Markdown file
agent-forge --agent examples/agent.md --model llama3.2 "Refactor main.rs"

# Disable built-in file tools
agent-forge --no-tools "What is 2+2?"

# Set generation temperature
agent-forge --temperature 0.3 "Write a haiku about programming"

# Continue a conversation with a thread ID
agent-forge --thread-id <uuid> "Now add error handling"
```

All CLI options:

| Flag | Default | Description |
|---|---|---|
| `--agent` | — | Path to agent prompt Markdown file |
| `--model` | `llama3.2` | Ollama model name |
| `--url` | `http://localhost:11434` | Ollama base URL |
| `--no-tools` | off | Disable built-in file system tools |
| `--temperature` | — | Sampling temperature |
| `--thread-id` | random | UUID thread ID for session persistence |
| `<message>` | required | The user message to send |

## Architecture

```
src/
├── main.rs              # CLI entry point (clap)
├── error.rs             # AgentForgeError enum, Result type alias
├── agent_prompt.rs      # Parses ## Section Markdown into system prompts
├── agent/
│   ├── mod.rs           # Agent struct, session() / session_with_id() / run_once()
│   └── builder.rs       # AgentBuilder (fluent API)
├── client/
│   ├── mod.rs           # LlmClient trait, ChatRequest, Ollama types
│   └── ollama.rs        # OllamaClient — streams NDJSON from /api/chat
├── session/
│   └── mod.rs           # Session — core agentic loop, state, event emission
├── memory/
│   └── mod.rs           # MemorySetTool / MemoryGetTool (session-scoped KV)
└── tools/
    ├── mod.rs           # ToolExecutor trait, ToolRegistry, ToolDefinition
    ├── file_read.rs     # read_file tool
    ├── file_write.rs    # write_file tool
    ├── file_list.rs     # list_directory tool
    └── file_search.rs   # search_files (glob) + grep_content (regex)
```

### Key flow

1. `Agent::builder(client).model(...).tools(...).build()` creates an `Agent`
2. `agent.session_with_id(thread_id)` creates a `Session` with per-session state and memory tools
3. `session.run(user_message)` streams the LLM response, executes tool calls, feeds results back, and repeats for up to `max_rounds` (default: 10)
4. Events are emitted through an `mpsc::UnboundedReceiver<Result<Event>>` channel following the AG-UI protocol

### AG-UI Events

The session emits these event types during a run:

| Event | When |
|---|---|
| `RunStarted` | Beginning of the run |
| `TextMessageStart` | Before streaming assistant text |
| `TextMessageContent` | Each text delta from the LLM |
| `TextMessageEnd` | After text streaming completes |
| `ToolCallStart` | Before executing a tool |
| `ToolCallArgs` | Tool arguments (as JSON string) |
| `ToolCallEnd` | After tool arguments are emitted |
| `ToolCallResult` | Tool execution result |
| `StateSnapshot` | First state change in a run (full state) |
| `StateDelta` | Subsequent state changes (JSON Patch) |
| `RunFinished` | End of the run |
| `RunError` | On errors |

## Programmatic Usage

```rust
use std::sync::Arc;
use agent_forge::client::ollama::OllamaClient;
use agent_forge::agent::Agent;
use agent_forge::tools::ToolRegistry;
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(OllamaClient::new());
    let tools = ToolRegistry::with_defaults();

    let agent = Agent::builder(client)
        .model("llama3.2")
        .system_prompt("You are a helpful coding assistant.")
        .tools(tools)
        .build()?;

    let mut session = agent.session();
    let mut rx = session.run("Explain Rust's borrow checker").await?;

    while let Some(event_result) = rx.next().await {
        match event_result? {
            ag_ui_core::event::Event::TextMessageContent(e) => print!("{}", e.delta),
            ag_ui_core::event::Event::ToolCallStart(e) => eprintln!("\n[Calling: {}]", e.tool_call_name),
            ag_ui_core::event::Event::RunFinished(_) => println!("\n[Done]"),
            _ => {}
        }
    }

    // Access session state
    let state = session.state().await;
    println!("State: {}", serde_json::to_string_pretty(&state)?);

    Ok(())
}
```

## Built-in Tools

| Tool | Description |
|---|---|
| `read_file` | Read a file's contents from the filesystem |
| `write_file` | Write content to a file (creates or overwrites) |
| `list_directory` | List files and subdirectories in a directory |
| `search_files` | Search for files matching a glob pattern |
| `grep_content` | Search file contents with regex (supports case-insensitive) |
| `memory_set` | Store a key-value pair in session memory |
| `memory_get` | Retrieve a value from session memory by key |

Note: `memory_set` and `memory_get` are injected per-session (not in `ToolRegistry::with_defaults()`) because they hold an `Arc<RwLock<Value>>` pointing to the session's state.

## Agent Prompt Files

Agent behavior is defined in Markdown files using `## Section` headers. Sections are concatenated into a system prompt in sorted order.

Example (`examples/agent.md`):

```markdown
## Role
You are an expert software development assistant.

## Context
- You have access to file system tools for reading, writing, listing, and searching files
- You can store and recall information using memory tools across the conversation

## Instructions
- Always read relevant files before suggesting changes
- Use memory_set to store important context about the project
- Keep responses concise and actionable

## Constraints
- Never modify files outside the project directory
- Always explain what a code change does before making it
```

If no `## Section` headers are present, the entire file content is used as a raw system prompt.

## Extending with Custom Tools

Implement the `ToolExecutor` trait and register with a `ToolRegistry`:

```rust
use async_trait::async_trait;
use serde_json::{json, Value};
use agent_forge::tools::ToolExecutor;
use agent_forge::error::Result;

pub struct MyTool;

#[async_trait]
impl ToolExecutor for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn description(&self) -> &str { "Does something custom" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "The input" }
            },
            "required": ["input"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let input = args["input"].as_str().unwrap_or("");
        Ok(format!("Processed: {}", input))
    }
}

// Register:
let mut registry = ToolRegistry::with_defaults();
registry.register(MyTool);
```

## Session Persistence

Sessions can be persisted using the `SessionStore` trait:

```rust
use agent_forge::session::{SessionStore, SessionData, InMemoryStore};

// Implement your own store (e.g., file-based, database):
pub struct FileStore;

impl SessionStore for FileStore {
    fn save(&self, thread_id: &str, data: &SessionData) -> Result<()> {
        // Serialize and write to disk
    }

    fn load(&self, thread_id: &str) -> Result<Option<SessionData>> {
        // Read and deserialize from disk
    }
}
```

Pass a store to the agent builder via `.store(Arc::new(my_store))`. By default, sessions use `InMemoryStore` (no persistence across process restarts).

## Dependencies

| Crate | Purpose |
|---|---|
| `ag-ui-core` | AG-UI protocol types and event definitions |
| `reqwest` | HTTP client for Ollama API |
| `tokio` | Async runtime |
| `serde` / `serde_json` | Serialization |
| `async-trait` | Async trait support |
| `futures` | Stream utilities and MPSC channel |
| `thiserror` | Error derive macros |
| `clap` | CLI argument parsing |
| `uuid` | Thread/run/message ID generation |
| `glob` | File glob pattern matching |
| `regex` | Regular expression search |
| `json-patch` | State delta computation |
| `parking_lot` | Synchronous `RwLock` for `SessionStore` |

## License

This project is licensed under the terms included in the repository.