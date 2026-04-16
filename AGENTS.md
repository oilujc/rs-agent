# AGENTS.md — agent-forge

## Build & Check

```bash
cargo build          # build (edition 2024, MSRV matches latest stable)
cargo check          # fast type-check
```

No tests, linter, or formatter configured yet.

## Architecture

Rust binary crate. Single package, no workspace.

**Module responsibilities:**

| Module | What it does |
|---|---|
| `client/` | `LlmClient` trait, `OllamaClient`, `OpenRouterClient` — stream responses from LLM providers. `create_client()` factory dispatches on `provider.name` to instantiate the right backend. Ollama uses NDJSON streaming; OpenRouter uses SSE (OpenAI-compatible) streaming with tool call accumulation by index. |
| `agent/` | `Agent` (factory) + `AgentBuilder` — creates `Session` instances |
| `session/` | `Session` — spawns the agentic loop as a `tokio::spawn` task, returns event channel immediately for real-time streaming. Also contains `SessionStore` trait, `InMemoryStore`, and `SqliteSessionStore`. Free functions `build_request_messages`, `build_augmented_system_prompt`, `run_agentic_loop` handle the core loop logic. |
| `event.rs` | Local event types (`Event` enum, `ThreadId`, `MessageId`, `ToolCallId`, `Role`, payload structs) — replaces the `ag-ui-core` dependency with simpler, purpose-built types without `BaseEvent` overhead. |
| `memory/` | `MemorySetTool` / `MemoryGetTool` — session-scoped KV state, injected per-session via `Arc<RwLock<Value>>` |
| `tools/` | `ToolExecutor` trait, `ToolRegistry`, built-in file tools (`read_file`, `write_file`, `list_directory`, `search_files`, `grep_content`, `create_directory`). All file tools respect `workdir` sandboxing when configured. |
| `summarizer.rs` | `Summarizer` — generates conversation summaries by calling the LLM after each run. Runs in a background `tokio::spawn` task so it doesn't block the event stream. Injects `## Context` (summary) and `## Last messages` (last N messages) into system prompt on subsequent runs. |
| `config.rs` | `Config` + `ProviderConfig` structs with serde deserialization — loaded from `--config` JSON file, merged with CLI overrides |
| `agent_prompt.rs` | Parses `## Section`-structured `.md` files into system prompts |
| `error.rs` | `AgentForgeError` enum with `Sqlite` variant for rusqlite errors |

**Key flow:** `Agent::session_with_id()` → `Session` → `session.run(msg)` → returns `UnboundedReceiver<Result<Event>>` **immediately**. The agentic loop runs inside a `tokio::spawn` task, streaming events through the channel in real-time. After the loop completes, summarization and session persistence run as background tasks.

**Critical details:**
- `memory_set`/`memory_get` tools are NOT in `ToolRegistry::with_defaults()`. They are added per-session inside `Agent::session_with_id()` because they hold an `Arc<RwLock<Value>>` pointing to the session's state. Do not add them as global tools.
- Tool call deduplication: `Session` tracks executed tool signatures across rounds. Duplicate calls return "Action already completed" messages. A `file_creation_count` cap (default 2) prevents the agent from creating more than 2 files per run, breaking recursive file-creation loops.
- Consecutive dedup round detection: If all tool calls in a round are deduplicated, a counter increments. After 2 consecutive dedup-only rounds, the loop breaks to prevent infinite looping.
- Streaming stdout: `main.rs` uses `StdoutLock` with explicit `flush()` after each event delta for real-time output.
- Thinking/reasoning: `--think` flag and `provider.think` config enable thinking output for supported models. Ollama sends `think: true` in the request body; OpenRouter streams `reasoning_content` in SSE deltas. Thinking text is displayed with gray ANSI color.

## Key Dependencies

- **No `ag-ui-core` dependency** — all event types are defined locally in `event.rs`. No external protocol crate needed.
- Two LLM backends: **Ollama** (NDJSON streaming at `/api/chat`) and **OpenRouter** (SSE/OpenAI-compatible streaming at `/chat/completions`). Messages are sent as raw `serde_json::Value`.
- `client::create_client(provider)` — factory that matches `provider.name` (`"ollama"` or `"openrouter"`) and returns the appropriate `Arc<dyn LlmClient>`. OpenRouter requires an `api_key`. Unknown provider names produce a `Config` error.
- Ollama `ChatRequest` sends `options: { temperature, num_predict }` for temperature and max tokens; OpenRouter sends top-level `max_tokens` and `temperature`.
- OpenRouter tool calls are accumulated by index across SSE chunks before execution, fixing the streaming fragmentation bug.
- `parking_lot::RwLock` for sync `SessionStore`; `tokio::sync::RwLock` for async session state.
- `rusqlite` (bundled) — SQLite for persistent session and memory storage. `SqliteSessionStore` wraps `rusqlite::Connection` in `Mutex<Connection>` to satisfy `Send + Sync`.

## CLI

```bash
agent-forge --agent examples/agent.md --model llama3.2 "message"
agent-forge --no-tools --temperature 0.5 "message"
agent-forge --thread-id <uuid> "follow-up message"
agent-forge --config config.json "message"
agent-forge --config config.json --thread-id <uuid> "follow-up message"
agent-forge --config config.json --workdir ./workspace "message"
agent-forge --provider ollama --model llama3.2 "message"
agent-forge --provider openrouter --api-key sk-or-... --model anthropic/claude-3.5-sonnet "message"
agent-forge --context-messages 5 "message"
agent-forge --no-summarize "message"
agent-forge --think --model deepseek-r1 "message"
agent-forge --max-tokens 4096 "message"
```

Config file overrides apply when `--config` is specified. CLI flags always override config file values. `--agent` and `--thread-id` are never in the config file.

For Ollama, a running server is required at `http://localhost:11434` (override with `--url` or config file). For OpenRouter, an API key is required (via `--api-key` or config file).

## Config File

The `--config` flag accepts a JSON file with these fields (all optional, defaults shown):

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

- `provider.name` — selects the LLM backend. `"ollama"` or `"openrouter"`. Override with `--provider` CLI flag
- `provider.model`, `provider.url`, `provider.temperature` — override defaults; each can be further overridden by CLI flags (`--model`, `--url`, `--temperature`)
- `provider.max_tokens` — caps generation length. Maps to `num_predict` for Ollama (inside `options`), `max_tokens` for OpenRouter. Override with `--max-tokens`
- `provider.summary_model` — model to use for conversation summarization. Defaults to `provider.model`. Can be a smaller/faster model for efficiency
- `provider.api_key` — API key for the provider. Required for OpenRouter. Omit for Ollama. Override with `--api-key` CLI flag
- `provider.think` — enable thinking/reasoning output for supported models. Sends `"think": true` to Ollama; OpenRouter streams `reasoning_content` automatically for reasoning models. Override with `--think`
- `db_path` — path to SQLite database for session/memory persistence. If omitted, uses in-memory store (no cross-process persistence)
- `workdir` — restricts all file tools to operate within this directory. Paths that escape workdir are rejected. If omitted, no sandboxing is applied
- `context_messages` — number of recent messages to include under `## Last messages` in the system prompt. Default: 3. Override with `--context-messages`
- `summarize` — whether to generate conversation summaries. Default: `true`. Set to `false` or use `--no-summarize` to disable
- `--agent` and `--thread-id` are NOT part of the config file; they are CLI-only

## Session & State

- `Session::run(&mut self, msg)` — appends user message to conversation history, spawns the agentic loop as a `tokio::spawn` task, and returns `UnboundedReceiver<Result<Event>>` immediately for real-time event streaming
- The agentic loop runs inside the spawned task: stream LLM response → execute tool calls → feed results back → repeat up to `max_rounds=10`. After the loop, `RunFinished` is emitted, then summarization and session persistence run as background tasks
- `Session::state()` — returns current `serde_json::Value` state (async)
- `Session::with_initial_messages(msgs)` — pre-populates session with loaded messages (for session restore)
- `Session::with_summary(summary)` — sets the conversation summary (loaded from store)
- `Agent::session_with_id()` — loads prior session data from the store if available, restoring messages, state, and summary
- `SqliteSessionStore` — persists sessions to SQLite. Table: `sessions (thread_id TEXT PK, messages TEXT, state TEXT, summary TEXT)`
- `InMemoryStore` — default store when no `db_path` is configured; no persistence across process restarts
- State diffing: first state change emits `StateSnapshot`, subsequent changes emit `StateDelta`

## Summarization

When `summarize` is enabled (default):

1. After `RunFinished` is emitted, a `tokio::spawn` task calls `Summarizer::summarize()` using the configured `summary_model`
2. If a previous summary exists, it is included in the summarization prompt so the new summary is an update, not a replacement
3. The summary is stored in `SessionData.summary` and persisted to the session store
4. On the next run, the system prompt is augmented with `## Context` (the summary) and `## Last messages` (the last `context_messages` messages)
5. Only the augmented system prompt + last `context_messages` are sent to the LLM, keeping context manageable
6. Full message history is still stored in `SessionData.messages` for summary generation and session persistence
7. Use `--no-summarize` or `"summarize": false` in config to disable

## Workdir Sandboxing

When `workdir` is set (via config or `--workdir` CLI flag):

- All file tools (`read_file`, `write_file`, `list_directory`, `search_files`, `grep_content`, `create_directory`) resolve relative paths against `workdir`
- All paths (relative and absolute) are canonicalized and checked to ensure they stay within `workdir`
- Paths that escape the `workdir` boundary are rejected with an error
- `resolve_path()` (for existing paths) and `resolve_path_allow_create()` (for new files/directories) handle the two sandboxing cases

## Tool Call Deduplication & Loop Prevention

The agentic loop includes two mechanisms to prevent recursive tool-call loops:

1. **Signature-based deduplication** — Each `write_file`/`read_file`/`create_directory`/`list_directory` call is keyed by `(tool_name, path)`. Other tools use their key arguments. If the same signature appears again, the call is blocked with "Action already completed" message.

2. **File creation cap** — A counter tracks successful `write_file` and `create_directory` calls across all rounds. After 2 successful file-creation calls, any subsequent `write_file` or `create_directory` is blocked with "Task completed: you have already created N file(s). Stop creating more files and respond to the user."

3. **Consecutive dedup detection** — If all tool calls in a round are deduplicated, a counter increments. After 2 consecutive dedup-only rounds, the loop breaks early.

These mechanisms prevent the common pattern where the model repeatedly creates files with slightly different names instead of responding to the user.

## Agent Prompt Files

Markdown files with `## Section` headers (e.g. `## Role`, `## Context`, `## Instructions`). Plain markdown with no headers falls back to raw system prompt. Passed via `--agent path.md`. When summarization is active, `## Context` and `## Last messages` sections are appended to the base system prompt automatically.