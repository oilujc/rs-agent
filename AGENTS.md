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
| `client/` | `LlmClient` trait + `OllamaClient` — streams NDJSON from Ollama `/api/chat` |
| `agent/` | `Agent` (factory) + `AgentBuilder` — creates `Session` instances |
| `session/` | `Session` — the core agentic loop (multi-turn, tool calls, state). Owns message history and emits AG-UI `Event`s via `mpsc` channel |
| `memory/` | `MemorySetTool` / `MemoryGetTool` — session-scoped KV state, injected per-session via `Arc<RwLock<Value>>` |
| `tools/` | `ToolExecutor` trait, `ToolRegistry`, built-in file tools (`read_file`, `write_file`, `list_directory`, `search_files`, `grep_content`) |
| `agent_prompt.rs` | Parses `## Section`-structured `.md` files into system prompts |

**Key flow:** `Agent::session_with_id()` → `Session` → `session.run(msg)` → returns `UnboundedReceiver<Result<Event>>`. The `Session` runs the full agentic loop inline (up to `max_rounds=10`): stream LLM response → execute tool calls → feed results back → emit AG-UI events.

**Critical detail:** `memory_set`/`memory_get` tools are NOT in `ToolRegistry::with_defaults()`. They are added per-session inside `Agent::session_with_id()` because they hold an `Arc<RwLock<Value>>` pointing to the session's state. Do not add them as global tools.

## Key Dependencies

- `ag-ui-core = "0.1.0"` — AG-UI protocol types (events, IDs). Types are nested: `ag_ui_core::types::ids::*`, `ag_ui_core::types::message::*`, `ag_ui_core::types::tool::*`, `ag_ui_core::event::*`. Import paths are not flat; use the nested modules.
- Ollama API is the only LLM backend. Messages are sent as raw `serde_json::Value` (not `ag_ui_core` `Message` types) because Ollama's schema differs.
- `parking_lot::RwLock` for sync `SessionStore`; `tokio::sync::RwLock` for async session state.

## CLI

```bash
agent-forge --agent examples/agent.md --model llama3.2 "message"
agent-forge --no-tools --temperature 0.5 "message"
agent-forge --thread-id <uuid> "follow-up message"
```

Requires a running Ollama server at `http://localhost:11434` (override with `--url`).

## Session & State

- `Session::run(&mut self, msg)` — appends to conversation history, runs agentic loop, returns event stream
- `Session::state()` — returns current `serde_json::Value` state (async)
- `Session::save()` — persists to `SessionStore` if configured (default: `InMemoryStore`, no persistence across process restarts)
- State diffing: first state change emits `StateSnapshot`, subsequent changes emit `StateDelta`

## Agent Prompt Files

Markdown files with `## Section` headers (e.g. `## Role`, `## Context`, `## Instructions`). Plain markdown with no headers falls back to raw system prompt. Passed via `--agent path.md`.