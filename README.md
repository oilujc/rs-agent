# agent-forge

A Rust framework for building and running AI agents powered by LLMs via [Ollama](https://ollama.com) or [OpenRouter](https://openrouter.ai). agent-forge streams agent events in real-time through a channel and provides a modular, extensible architecture for tool-augmented conversational agents.

## Features

- **Multi-provider** — connect to Ollama (local) or OpenRouter (cloud API) with a single config switch
- **Real-time streaming** — events stream through an `mpsc` channel as they happen (text deltas, thinking, tool calls, state changes)
- **Thinking/reasoning support** — `--think` flag enables reasoning output for models like `deepseek-r1` (Ollama) or reasoning models on OpenRouter
- **Multi-turn agentic loop** — automatically streams LLM responses, executes tool calls, feeds results back, and repeats up to a configurable round limit
- **Session persistence** — thread-scoped `Session` with pluggable `SessionStore` (in-memory or SQLite)
- **Conversation summarization** — automatic summarization using a configurable `summary_model`, injected as `## Context` on subsequent runs
- **Agent prompts from Markdown** — define agent behavior with structured `## Section` Markdown files
- **Built-in file tools** — read, write, list, search, grep, and create directories; all sandboxed to an optional `workdir`
- **Session-scoped memory** — `memory_set`/`memory_get` tools let agents persist key-value state across turns
- **Tool deduplication & loop prevention** — signature-based dedup, file creation cap, and consecutive dedup detection prevent infinite tool-call loops
- **Config file & CLI** — full configuration via JSON file with CLI flag overrides
- **Builder pattern** — `AgentBuilder` and `Session` API for clean programmatic configuration

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024, latest stable)
- [Ollama](https://ollama.com) running locally for local models, or an OpenRouter API key for cloud models

## Getting Started

### Build

```bash
cargo build
```

### Run via CLI

```bash
# Simple one-shot message (Ollama)
agent-forge "Explain ownership in Rust"

# Use a specific model
agent-forge --model codellama "Write a Fibonacci function"

# Load a custom agent prompt from a Markdown file
agent-forge --agent examples/agent.md --model llama3.2 "Refactor main.rs"

# Use OpenRouter with a cloud model
agent-forge --provider openrouter --api-key sk-or-... --model anthropic/claude-3.5-sonnet "message"

# Enable thinking/reasoning for supported models
agent-forge --think --model deepseek-r1 "Solve this puzzle"

# Limit generation length
agent-forge --max-tokens 4096 "Write a short poem"

# Continue a conversation with a thread ID
agent-forge --thread-id <uuid> "Now add error handling"

# Use a config file
agent-forge --config config.json "message"

# Disable built-in file tools
agent-forge --no-tools "What is 2+2?"

# Set generation temperature
agent-forge --temperature 0.3 "Write a haiku about programming"
```

### All CLI Options

| Flag | Default | Description |
|---|---|---|
| `--agent` | — | Path to agent prompt Markdown file |
| `--config` | — | Path to JSON config file |
| `--provider` | `ollama` | LLM provider (`ollama` or `openrouter`) |
| `--model` | `llama3.2` | Model name |
| `--url` | `http://localhost:11434` | Provider base URL |
| `--api-key` | — | API key for the provider (required for OpenRouter) |
| `--temperature` | — | Sampling temperature |
| `--max-tokens` | — | Maximum tokens to generate |
| `--think` | off | Enable thinking/reasoning for supported models |
| `--no-tools` | off | Disable built-in file system tools |
| `--thread-id` | random | UUID thread ID for session persistence |
| `--db-path` | — | Path to SQLite database for session persistence |
| `--workdir` | — | Working directory for file tool sandboxing |
| `--context-messages` | 3 | Number of recent messages to include in context |
| `--no-summarize` | off | Disable conversation summarization |
| `<message>` | required | The user message to send |

### Config File

```json
{
  "provider": {
    "name": "ollama",
    "model": "llama3.2",
    "url": "http://localhost:11434",
    "temperature": 0.7,
    "max_tokens": null,
    "summary_model": "llama3.2",
    "api_key": null,
    "think": false
  },
  "db_path": "./sessions.db",
  "workdir": "./workspace",
  "context_messages": 3,
  "summarize": true
}
```

CLI flags always override config file values. `--agent` and `--thread-id` are CLI-only.

## Architecture

```
src/
├── main.rs              # CLI entry point (clap), event loop, stdout streaming
├── config.rs           # Config + ProviderConfig, CLI override merging
├── error.rs            # AgentForgeError enum
├── event.rs            # Local Event enum, ID types (ThreadId, MessageId, ToolCallId), Role
├── agent_prompt.rs     # Parses ## Section Markdown into system prompts
├── summarizer.rs       # Summarizer — background summarization task
├── client/
│   ├── mod.rs          # LlmClient trait, ChatRequest, create_client(), Ollama types
│   ├── ollama.rs       # OllamaClient — NDJSON streaming, think support
│   └── openrouter.rs   # OpenRouterClient — SSE streaming, tool call accumulation, reasoning_content
├── agent/
│   ├── mod.rs          # Agent struct, session_with_id()
│   └── builder.rs      # AgentBuilder (fluent API)
├── session/
│   ├── mod.rs          # Session, run_agentic_loop, deduplication, real-time event streaming
│   └── sqlite_store.rs # SqliteSessionStore with summary column + migration
├── memory/
│   └── mod.rs           # MemorySetTool / MemoryGetTool (session-scoped KV)
└── tools/
    ├── mod.rs           # ToolExecutor trait, ToolRegistry, resolve_path(), resolve_path_allow_create()
    ├── file_read.rs     # read_file tool with offset/limit/binary detection
    ├── file_write.rs    # write_file tool with append mode
    ├── file_list.rs     # list_directory tool with workdir sandboxing
    ├── file_search.rs   # search_files (glob) + grep_content (regex) with workdir sandboxing
    └── dir_create.rs    # create_directory tool with workdir sandboxing
```

### Key Flow

1. `Agent::builder(client).model(...).tools(...).build()` creates an `Agent`
2. `agent.session_with_id(thread_id)` creates a `Session` with per-session state and memory tools
3. `session.run(user_message)` pushes the user message, spawns the agentic loop as a `tokio::spawn` task, and returns the event channel **immediately**
4. The spawned task streams events in real-time: LLM response → tool calls → loop → `RunFinished`
5. After `RunFinished`, summarization and session persistence run as background tasks

### Events

The session emits these event types during a run:

| Event | When |
|---|---|
| `RunStarted` | Beginning of the run |
| `ThinkingTextMessageStart` | Start of thinking/reasoning output |
| `ThinkingTextMessageContent` | Each thinking text delta |
| `ThinkingTextMessageEnd` | End of thinking output |
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

## Built-in Tools

| Tool | Description |
|---|---|
| `read_file` | Read a file's contents (supports offset/limit, binary detection) |
| `write_file` | Write content to a file (creates, overwrites, or appends) |
| `list_directory` | List files and subdirectories in a directory |
| `search_files` | Search for files matching a glob pattern |
| `grep_content` | Search file contents with regex |
| `create_directory` | Create a directory and any missing parent directories |
| `memory_set` | Store a key-value pair in session memory |
| `memory_get` | Retrieve a value from session memory by key |

Note: `memory_set` and `memory_get` are injected per-session (not in `ToolRegistry::with_defaults()`) because they hold an `Arc<RwLock<Value>>` pointing to the session's state.

## Tool Call Deduplication & Loop Prevention

1. **Signature-based deduplication** — Tool calls are keyed by `(tool_name, path)` or key arguments. Duplicate calls return "Action already completed."
2. **File creation cap** — After 2 successful `write_file`/`create_directory` calls, further file creation is blocked with "Task completed."
3. **Consecutive dedup detection** — After 2 consecutive rounds where all calls are deduplicated, the loop breaks early.

## Agent Prompt Files

Markdown files with `## Section` headers (e.g. `## Role`, `## Context`, `## Instructions`). Plain markdown with no headers falls back to raw system prompt. When summarization is active, `## Context` and `## Last messages` sections are appended automatically.

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

Sessions can be persisted using the `SessionStore` trait with a built-in SQLite backend:

```bash
# Persist sessions to a SQLite database
agent-forge --config config.json --db-path ./sessions.db "message"

# Continue a previous session
agent-forge --config config.json --thread-id <uuid> "follow-up message"
```

Or programmatically:

```rust
use agent_forge::session::sqlite_store::SqliteSessionStore;
use std::sync::Arc;

let store = Arc::new(SqliteSessionStore::open("./sessions.db")?);
let agent = Agent::builder(client)
    .store(store)
    .build()?;
```

## Dependencies

| Crate | Purpose |
|---|---|
| `reqwest` | HTTP client for Ollama/OpenRouter APIs |
| `tokio` | Async runtime, `tokio::spawn` for background tasks |
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
| `rusqlite` | SQLite for persistent session storage (bundled) |

## License

This project is licensed under the terms included in the repository.