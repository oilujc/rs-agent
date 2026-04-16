pub mod sqlite_store;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::event::{
    Event, MessageId, Role, RunErrorEvent, RunFinishedEvent, RunId, RunStartedEvent,
    StateDeltaEvent, StateSnapshotEvent, TextMessageContentEvent, TextMessageEndEvent,
    TextMessageStartEvent, ThinkingTextMessageContentEvent, ThinkingTextMessageEndEvent,
    ThinkingTextMessageStartEvent, ThreadId, ToolCallArgsEvent, ToolCallEndEvent,
    ToolCallId, ToolCallResultEvent, ToolCallStartEvent,
};

use crate::client::{ChatRequest, LlmClient, OllamaFunction, OllamaToolCall};
use crate::error::{AgentForgeError, Result};
use crate::summarizer::Summarizer;
use crate::tools::ToolRegistry;

pub trait AgentState:
    Debug + Clone + Send + Sync + serde::de::DeserializeOwned + Serialize + Default + 'static
{
}

impl AgentState for Value {}
impl AgentState for () {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub messages: Vec<Value>,
    pub state: Value,
    #[serde(default)]
    pub summary: Option<String>,
}

impl Default for SessionData {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            state: Value::Object(serde_json::Map::new()),
            summary: None,
        }
    }
}

pub trait SessionStore: Send + Sync {
    fn save(&self, thread_id: &str, data: &SessionData) -> Result<()>;
    fn load(&self, thread_id: &str) -> Result<Option<SessionData>>;
}

pub struct InMemoryStore {
    sessions: parking_lot::RwLock<HashMap<String, SessionData>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            sessions: parking_lot::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore for InMemoryStore {
    fn save(&self, thread_id: &str, data: &SessionData) -> Result<()> {
        self.sessions.write().insert(thread_id.to_string(), data.clone());
        Ok(())
    }

    fn load(&self, thread_id: &str) -> Result<Option<SessionData>> {
        Ok(self.sessions.read().get(thread_id).cloned())
    }
}

fn emit(tx: &mpsc::UnboundedSender<Result<Event>>, event: Event) -> Result<()> {
    tx.unbounded_send(Ok(event))
        .map_err(|e| AgentForgeError::Agent(e.to_string()))?;
    Ok(())
}

fn compute_tool_signature(name: &str, args: &Value) -> String {
    match name {
        "write_file" | "read_file" | "create_directory" | "list_directory" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("{}:{}", name, path.to_lowercase())
        }
        "search_files" => {
            let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("search_files:{}:{}", pattern.to_lowercase(), path.to_lowercase())
        }
        "grep_content" => {
            let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            format!("grep_content:{}:{}", pattern, path.to_lowercase())
        }
        "memory_set" | "memory_get" => {
            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
            format!("{}:{}", name, key)
        }
        _ => {
            let args_str = serde_json::to_string(args).unwrap_or_default();
            format!("{}:{}", name, args_str)
        }
    }
}

fn format_message_content(msg: &Value) -> String {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("unknown");
    match role {
        "system" => String::new(),
        _ => {
            let mut parts = Vec::new();
            if let Some(thinking) = msg.get("thinking").and_then(|v| v.as_str()) {
                if !thinking.is_empty() {
                    parts.push(format!("[thinking]: {}", thinking));
                }
            }
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if !content.is_empty() {
                parts.push(format!("{}: {}", role, content));
            }
            parts.join(" ")
        }
    }
}

pub struct Session {
    thread_id: ThreadId,
    client: Arc<dyn LlmClient>,
    model: String,
    system_prompt: Option<String>,
    tools: ToolRegistry,
    temperature: Option<f32>,
    messages: Vec<Value>,
    state: Arc<RwLock<Value>>,
    state_snapshot_emitted: bool,
    store: Option<Arc<dyn SessionStore>>,
    max_rounds: usize,
    summarize: bool,
    context_messages: u32,
    summary: Option<String>,
    summarizer: Option<Summarizer>,
    think: bool,
    max_tokens: Option<u32>,
}

impl Session {
    pub(crate) fn new(
        thread_id: ThreadId,
        client: Arc<dyn LlmClient>,
        model: String,
        system_prompt: Option<String>,
        tools: ToolRegistry,
        temperature: Option<f32>,
        state: Arc<RwLock<Value>>,
        store: Option<Arc<dyn SessionStore>>,
    ) -> Self {
        let messages = Vec::new();

        Self {
            thread_id,
            client,
            model,
            system_prompt,
            tools,
            temperature,
            messages,
            state,
            state_snapshot_emitted: false,
            store,
            max_rounds: 10,
            summarize: false,
            context_messages: 3,
            summary: None,
            summarizer: None,
            think: false,
            max_tokens: None,
        }
    }

    pub(crate) fn with_initial_messages(mut self, messages: Vec<Value>) -> Self {
        let has_system = messages.iter().any(|m| m.get("role").and_then(|v| v.as_str()) == Some("system"));
        if let Some(ref system) = self.system_prompt {
            if !system.is_empty() && !has_system {
                self.messages.push(crate::client::system_message(system));
            }
        }
        self.messages.extend(messages);
        self
    }

    pub(crate) fn with_summary(mut self, summary: Option<String>) -> Self {
        self.summary = summary;
        self
    }

    pub(crate) fn with_summarization(
        mut self,
        summarize: bool,
        context_messages: u32,
        summarizer: Option<Summarizer>,
    ) -> Self {
        self.summarize = summarize;
        self.context_messages = context_messages;
        self.summarizer = summarizer;
        self
    }

    pub(crate) fn with_think(mut self, think: bool) -> Self {
        self.think = think;
        self
    }

    pub(crate) fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub(crate) fn with_max_tokens_opt(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn thread_id(&self) -> &ThreadId {
        &self.thread_id
    }

    pub async fn state(&self) -> Value {
        self.state.read().await.clone()
    }

    pub fn messages(&self) -> &[Value] {
        &self.messages
    }

    pub fn with_max_rounds(mut self, max: usize) -> Self {
        self.max_rounds = max;
        self
    }

    pub async fn save(&self) -> Result<()> {
        if let Some(ref store) = self.store {
            let data = SessionData {
                messages: self.messages.clone(),
                state: self.state.read().await.clone(),
                summary: self.summary.clone(),
            };
            store.save(&self.thread_id.to_string(), &data)?;
        }
        Ok(())
    }
}

fn build_augmented_system_prompt(system_prompt: &Option<String>, summary: &Option<String>, context_messages: u32, messages: &[Value]) -> String {
    let mut prompt = String::new();

    if let Some(base) = system_prompt {
        if !base.is_empty() {
            prompt.push_str(base);
        }
    }

    if let Some(summary_text) = summary {
        if !summary_text.is_empty() {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str("## Context\n");
            prompt.push_str(summary_text);
        }
    }

    let n = context_messages as usize;
    let non_system: Vec<&Value> = messages.iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) != Some("system"))
        .collect();

    if !non_system.is_empty() {
        let last_n: Vec<&Value> = non_system.iter().rev().take(n).rev().cloned().collect();
        let last_messages: Vec<String> = last_n.iter()
            .filter_map(|m| {
                let s = format_message_content(m);
                if s.is_empty() { None } else { Some(s) }
            })
            .collect();

        if !last_messages.is_empty() {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str("## Last messages\n");
            prompt.push_str(&last_messages.join("\n"));
        }
    }

    prompt
}

fn build_request_messages(system_prompt: &Option<String>, summary: &Option<String>, context_messages: u32, messages: &[Value]) -> Vec<Value> {
    let augmented_prompt = build_augmented_system_prompt(system_prompt, summary, context_messages, messages);

    let mut request_messages = Vec::new();

    if !augmented_prompt.is_empty() {
        request_messages.push(crate::client::system_message(&augmented_prompt));
    }

    let n = context_messages as usize;
    let non_system: Vec<&Value> = messages.iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) != Some("system"))
        .collect();

    if non_system.len() <= n {
        request_messages.extend(non_system.iter().cloned().cloned());
    } else {
        let last_n: Vec<&Value> = non_system.iter().rev().take(n).rev().cloned().collect();
        request_messages.extend(last_n.iter().cloned().cloned());
    }

    request_messages
}

async fn run_agentic_loop(
    tx: mpsc::UnboundedSender<Result<Event>>,
    thread_id: ThreadId,
    run_id: RunId,
    client: Arc<dyn LlmClient>,
    model: String,
    system_prompt: Option<String>,
    tools: ToolRegistry,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    think: bool,
    summarize: bool,
    context_messages: u32,
    summary: Option<String>,
    summarizer: Option<Summarizer>,
    max_rounds: usize,
    initial_state: Value,
    state: Arc<RwLock<Value>>,
    store: Option<Arc<dyn SessionStore>>,
    mut messages: Vec<Value>,
    tools_json: Vec<Value>,
) {
    let mut executed_tool_signatures: HashSet<String> = HashSet::new();
    let mut consecutive_dedup_rounds: u32 = 0;
    let mut file_creation_count: u32 = 0;
    let mut state_snapshot_emitted = false;

    let request_messages = if summarize {
        build_request_messages(&system_prompt, &summary, context_messages, &messages)
    } else {
        messages.clone()
    };

    for _round in 0..max_rounds {
        let request = ChatRequest {
            model: model.clone(),
            messages: request_messages.clone(),
            tools: tools_json.clone(),
            stream: true,
            temperature,
            max_tokens,
            think: if think { Some(true) } else { None },
        };

        let stream = match client.chat_stream(request).await {
            Ok(s) => s,
            Err(e) => {
                let _ = emit(&tx, Event::RunError(RunErrorEvent {
                    message: e.to_string(),
                    code: None,
                }));
                let _ = emit(&tx, Event::RunFinished(RunFinishedEvent {
                    thread_id: thread_id.clone(),
                    run_id: run_id.clone(),
                    result: None,
                }));
                drop(tx);
                return;
            }
        };

        let mut full_content = String::new();
        let mut full_thinking = String::new();
        let mut thinking_active = false;
        let mut streaming_tool_calls: BTreeMap<usize, (Option<String>, String)> = BTreeMap::new();
        let mut complete_tool_calls: Vec<OllamaToolCall> = Vec::new();
        let message_id = MessageId::random();

        if let Err(_) = emit(&tx, Event::TextMessageStart(TextMessageStartEvent {
            message_id: message_id.clone(),
            role: Role::Assistant,
        })) {
            return;
        }

        let mut stream = Box::pin(stream);
        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    if thinking_active {
                        let _ = emit(&tx, Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {}));
                        thinking_active = false;
                    }
                    let _ = emit(&tx, Event::RunError(RunErrorEvent {
                        message: e.to_string(),
                        code: None,
                    }));
                    drop(tx);
                    return;
                }
            };

            if let Some(message) = chunk.message {
                if let Some(ref thinking_delta) = message.thinking {
                    if !thinking_delta.is_empty() {
                        if !thinking_active {
                            let _ = emit(&tx, Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent {}));
                            thinking_active = true;
                        }
                        full_thinking.push_str(thinking_delta);
                        let _ = emit(&tx, Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
                            delta: thinking_delta.clone(),
                        }));
                    }
                }

                if !message.content.is_empty() {
                    if thinking_active {
                        let _ = emit(&tx, Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {}));
                        thinking_active = false;
                    }
                    full_content.push_str(&message.content);
                    let _ = emit(&tx, Event::TextMessageContent(TextMessageContentEvent {
                        message_id: message_id.clone(),
                        delta: message.content,
                    }));
                }

                if let Some(calls) = message.tool_calls {
                    for call in calls {
                        if let Some(idx) = call.index {
                            let entry = streaming_tool_calls.entry(idx).or_insert((None, String::new()));
                            if !call.function.name.is_empty() {
                                entry.0 = Some(call.function.name.clone());
                            }
                            let args_str = match &call.function.arguments {
                                Value::String(s) => s.clone(),
                                other => serde_json::to_string(other).unwrap_or_default(),
                            };
                            if !args_str.is_empty() {
                                entry.1.push_str(&args_str);
                            }
                        } else {
                            complete_tool_calls.push(call);
                        }
                    }
                }
            }
        }

        if thinking_active {
            let _ = emit(&tx, Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {}));
            thinking_active = false;
        }

        let mut finalized_calls: Vec<OllamaToolCall> = complete_tool_calls;
        for (idx, (name, args_str)) in streaming_tool_calls {
            let args_value: Value = serde_json::from_str(&args_str)
                .unwrap_or_else(|_| serde_json::json!({}));
            finalized_calls.push(OllamaToolCall {
                index: Some(idx),
                function: OllamaFunction {
                    name: name.unwrap_or_default(),
                    arguments: args_value,
                },
            });
        }

        let _ = emit(&tx, Event::TextMessageEnd(TextMessageEndEvent {
            message_id: message_id.clone(),
        }));

        let mut assistant_msg = serde_json::json!({
            "role": "assistant",
            "content": full_content,
        });

        if !full_thinking.is_empty() {
            assistant_msg["thinking"] = serde_json::Value::String(full_thinking);
        }

        if !finalized_calls.is_empty() {
            let calls_json: Vec<Value> = finalized_calls
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "function": {
                            "name": c.function.name,
                            "arguments": c.function.arguments,
                        }
                    })
                })
                .collect();
            assistant_msg["tool_calls"] = serde_json::json!(calls_json);
        }
        messages.push(assistant_msg);

        if finalized_calls.is_empty() {
            break;
        }

        let mut all_deduped = true;
        let state_before = state.read().await.clone();

        for call in &finalized_calls {
            let tc_id = ToolCallId::random();
            let args_json = call.function.arguments.clone();
            let signature = compute_tool_signature(&call.function.name, &args_json);

            let _ = emit(&tx, Event::ToolCallStart(ToolCallStartEvent {
                tool_call_id: tc_id.clone(),
                tool_call_name: call.function.name.clone(),
                parent_message_id: None,
            }));

            let args_str = serde_json::to_string(&args_json).unwrap_or_default();
            let _ = emit(&tx, Event::ToolCallArgs(ToolCallArgsEvent {
                tool_call_id: tc_id.clone(),
                delta: args_str,
            }));

            let _ = emit(&tx, Event::ToolCallEnd(ToolCallEndEvent {
                tool_call_id: tc_id.clone(),
            }));

            let result = if executed_tool_signatures.contains(&signature) {
                format!(
                    "Action already completed: {} was already called with similar arguments. Do not repeat this action. Respond to the user now.",
                    call.function.name
                )
            } else if matches!(call.function.name.as_str(), "write_file" | "create_directory") && file_creation_count >= 2 {
                format!(
                    "Task completed: you have already created {} file(s). Stop creating more files and respond to the user with a summary of what was done.",
                    file_creation_count
                )
            } else {
                all_deduped = false;
                executed_tool_signatures.insert(signature);
                let res = tools
                    .execute(&call.function.name, args_json.clone())
                    .await
                    .unwrap_or_else(|e| format!("Error: {}", e));
                if matches!(call.function.name.as_str(), "write_file" | "create_directory") && !res.starts_with("Error") {
                    file_creation_count += 1;
                }
                res
            };

            let result_msg_id = MessageId::random();
            let _ = emit(&tx, Event::ToolCallResult(ToolCallResultEvent {
                message_id: result_msg_id.clone(),
                tool_call_id: tc_id,
                content: result.clone(),
                role: Role::Tool,
            }));

            messages.push(crate::client::tool_result_message(
                &call.function.name,
                &result,
            ));
        }

if all_deduped {
                consecutive_dedup_rounds += 1;
                if consecutive_dedup_rounds >= 2 {
                    break;
                }
            } else {
                consecutive_dedup_rounds = 0;
            }

            if file_creation_count >= 2 && all_deduped {
                break;
            }

        let state_after = state.read().await.clone();
        if state_before != state_after {
            if !state_snapshot_emitted {
                state_snapshot_emitted = true;
                let _ = emit(&tx, Event::StateSnapshot(StateSnapshotEvent {
                    snapshot: state_after.clone(),
                }));
            } else {
                let patch = vec![serde_json::json!({
                    "op": "replace",
                    "path": "",
                    "value": state_after,
                })];
                let _ = emit(&tx, Event::StateDelta(StateDeltaEvent {
                    delta: patch,
                }));
            }
        }
    }

    let final_state = state.read().await.clone();
    if initial_state != final_state && !state_snapshot_emitted {
        let _ = emit(&tx, Event::StateSnapshot(StateSnapshotEvent {
            snapshot: final_state,
        }));
    }

    let _ = emit(&tx, Event::RunFinished(RunFinishedEvent {
        thread_id: thread_id.clone(),
        run_id,
        result: None,
    }));

    drop(tx);

    let should_summarize = summarize;
    let summarizer_clone = summarizer.clone();
    let existing_summary = summary.clone();
    let state_for_save = state.read().await.clone();
    let thread_id_str = thread_id.to_string();

    tokio::spawn(async move {
        let mut final_summary = existing_summary;

        if should_summarize {
            if let Some(ref summ) = summarizer_clone {
                match summ.summarize(&messages, final_summary.as_deref()).await {
                    Ok(new_summary) => {
                        final_summary = Some(new_summary);
                    }
                    Err(e) => {
                        eprintln!("[Warning: summarization failed: {}]", e);
                    }
                }
            }
        }

        if let Some(ref s) = store {
            let data = SessionData {
                messages,
                state: state_for_save,
                summary: final_summary,
            };
            if let Err(e) = s.save(&thread_id_str, &data) {
                eprintln!("[Warning: session save failed: {}]", e);
            }
        }
    });
}

impl Session {
    pub async fn run(&mut self, user_message: &str) -> Result<mpsc::UnboundedReceiver<Result<Event>>> {
        let (tx, rx) = mpsc::unbounded();

        self.messages.push(crate::client::user_message(user_message));

        let run_id = RunId::random();
        let thread_id = self.thread_id.clone();

        emit(&tx, Event::RunStarted(RunStartedEvent {
            thread_id: thread_id.clone(),
            run_id: run_id.clone(),
        }))?;

        let tools_json: Vec<Value> = self.tools.tool_definitions().into_iter()
            .map(|td| crate::client::tool_definition_to_ollama(&td.name, &td.description, &td.parameters))
            .collect();

        let initial_state = self.state.read().await.clone();

        let client = self.client.clone();
        let model = self.model.clone();
        let system_prompt = self.system_prompt.clone();
        let tools = self.tools.clone();
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let think = self.think;
        let summarize = self.summarize;
        let context_messages = self.context_messages;
        let summary = self.summary.take();
        let summarizer = self.summarizer.take();
        let max_rounds = self.max_rounds;
        let state = self.state.clone();
        let store = self.store.clone();
        let messages = std::mem::take(&mut self.messages);

        tokio::spawn(async move {
            run_agentic_loop(
                tx, thread_id, run_id, client, model, system_prompt, tools,
                temperature, max_tokens, think, summarize, context_messages,
                summary, summarizer, max_rounds, initial_state, state, store,
                messages, tools_json,
            ).await;
        });

        Ok(rx)
    }
}