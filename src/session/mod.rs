use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use ag_ui_core::types::ids::*;
use ag_ui_core::event::{BaseEvent, Event, RunErrorEvent, RunFinishedEvent, RunStartedEvent,
    StateDeltaEvent, StateSnapshotEvent, TextMessageContentEvent, TextMessageEndEvent,
    TextMessageStartEvent, ToolCallArgsEvent, ToolCallEndEvent, ToolCallResultEvent,
    ToolCallStartEvent};
use ag_ui_core::types::message::Role;

use crate::client::{ChatRequest, LlmClient, OllamaToolCall};
use crate::error::{AgentForgeError, Result};
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
}

impl Default for SessionData {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            state: Value::Object(serde_json::Map::new()),
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

fn base_event() -> BaseEvent {
    BaseEvent {
        timestamp: None,
        raw_event: None,
    }
}

fn emit(tx: &mpsc::UnboundedSender<Result<Event>>, event: Event) -> Result<()> {
    tx.unbounded_send(Ok(event))
        .map_err(|e| AgentForgeError::Agent(e.to_string()))?;
    Ok(())
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
}

impl Session {
    pub(crate) fn new(
        thread_id: ThreadId,
        client: Arc<dyn LlmClient>,
        model: String,
        system_prompt: Option<String>,
        tools: ToolRegistry,
        temperature: Option<f32>,
        initial_state: Value,
        store: Option<Arc<dyn SessionStore>>,
    ) -> Self {
        let mut messages = Vec::new();
        if let Some(ref system) = system_prompt {
            if !system.is_empty() {
                messages.push(crate::client::system_message(system));
            }
        }

        Self {
            thread_id,
            client,
            model,
            system_prompt,
            tools,
            temperature,
            messages,
            state: Arc::new(RwLock::new(initial_state)),
            state_snapshot_emitted: false,
            store,
            max_rounds: 10,
        }
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
            };
            store.save(&self.thread_id.to_string(), &data)?;
        }
        Ok(())
    }

    pub async fn run(&mut self, user_message: &str) -> Result<mpsc::UnboundedReceiver<Result<Event>>> {
        let (tx, rx) = mpsc::unbounded();

        self.messages.push(crate::client::user_message(user_message));

        let run_id = RunId::random();
        let thread_id = self.thread_id.clone();

        emit(&tx, Event::RunStarted(RunStartedEvent {
            base: base_event(),
            thread_id: thread_id.clone(),
            run_id: run_id.clone(),
        }))?;

        let tools_json: Vec<Value> = self.tools.tool_definitions().into_iter()
            .map(|td| crate::client::tool_definition_to_ollama(&td.name, &td.description, &td.parameters))
            .collect();

        let initial_state = self.state.read().await.clone();

        for _round in 0..self.max_rounds {
            let request = ChatRequest {
                model: self.model.clone(),
                messages: self.messages.clone(),
                tools: tools_json.clone(),
                stream: true,
                temperature: self.temperature,
            };

            let stream = match self.client.chat_stream(request).await {
                Ok(s) => s,
                Err(e) => {
                    emit(&tx, Event::RunError(RunErrorEvent {
                        base: base_event(),
                        message: e.to_string(),
                        code: None,
                    }))?;
                    emit(&tx, Event::RunFinished(RunFinishedEvent {
                        base: base_event(),
                        thread_id: thread_id.clone(),
                        run_id: run_id.clone(),
                        result: None,
                    }))?;
                    drop(tx);
                    return Err(e);
                }
            };

            let mut full_content = String::new();
            let mut tool_calls: Vec<OllamaToolCall> = Vec::new();
            let message_id = MessageId::random();

            emit(&tx, Event::TextMessageStart(TextMessageStartEvent {
                base: base_event(),
                message_id: message_id.clone(),
                role: Role::Assistant,
            }))?;

            let mut stream = Box::pin(stream);
            while let Some(chunk_result) = stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        emit(&tx, Event::RunError(RunErrorEvent {
                            base: base_event(),
                            message: e.to_string(),
                            code: None,
                        }))?;
                        drop(tx);
                        return Err(e);
                    }
                };

                if let Some(message) = chunk.message {
                    if !message.content.is_empty() {
                        full_content.push_str(&message.content);
                        emit(&tx, Event::TextMessageContent(TextMessageContentEvent {
                            base: base_event(),
                            message_id: message_id.clone(),
                            delta: message.content,
                        }))?;
                    }

                    if let Some(calls) = message.tool_calls {
                        tool_calls.extend(calls);
                    }
                }
            }

            emit(&tx, Event::TextMessageEnd(TextMessageEndEvent {
                base: base_event(),
                message_id: message_id.clone(),
            }))?;

            let mut assistant_msg = serde_json::json!({
                "role": "assistant",
                "content": full_content,
            });

            if !tool_calls.is_empty() {
                let calls_json: Vec<Value> = tool_calls
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
            self.messages.push(assistant_msg);

            if tool_calls.is_empty() {
                break;
            }

            let state_before = self.state.read().await.clone();

            for call in &tool_calls {
                let tc_id = ToolCallId::random();
                let args_json = call.function.arguments.clone();

                emit(&tx, Event::ToolCallStart(ToolCallStartEvent {
                    base: base_event(),
                    tool_call_id: tc_id.clone(),
                    tool_call_name: call.function.name.clone(),
                    parent_message_id: None,
                }))?;

                let args_str = serde_json::to_string(&args_json).unwrap_or_default();
                emit(&tx, Event::ToolCallArgs(ToolCallArgsEvent {
                    base: base_event(),
                    tool_call_id: tc_id.clone(),
                    delta: args_str,
                }))?;

                emit(&tx, Event::ToolCallEnd(ToolCallEndEvent {
                    base: base_event(),
                    tool_call_id: tc_id.clone(),
                }))?;

                let result = self.tools
                    .execute(&call.function.name, args_json.clone())
                    .await
                    .unwrap_or_else(|e| format!("Error: {}", e));

                let result_msg_id = MessageId::random();
                emit(&tx, Event::ToolCallResult(ToolCallResultEvent {
                    base: base_event(),
                    message_id: result_msg_id.clone(),
                    tool_call_id: tc_id,
                    content: result.clone(),
                    role: Role::Tool,
                }))?;

                self.messages.push(crate::client::tool_result_message(
                    &call.function.name,
                    &result,
                ));
            }

            let state_after = self.state.read().await.clone();
            if state_before != state_after {
                self.emit_state_change(&tx, &state_after).await?;
            }
        }

        let final_state = self.state.read().await.clone();
        if initial_state != final_state && !self.state_snapshot_emitted {
            emit(&tx, Event::StateSnapshot(StateSnapshotEvent {
                base: base_event(),
                snapshot: final_state,
            }))?;
            self.state_snapshot_emitted = true;
        }

        emit(&tx, Event::RunFinished(RunFinishedEvent {
            base: base_event(),
            thread_id,
            run_id,
            result: None,
        }))?;

        drop(tx);

        let _ = self.save().await;

        Ok(rx)
    }

    async fn emit_state_change(
        &mut self,
        tx: &mpsc::UnboundedSender<Result<Event>>,
        new_state: &Value,
    ) -> Result<()> {
        if !self.state_snapshot_emitted {
            self.state_snapshot_emitted = true;
            emit(tx, Event::StateSnapshot(StateSnapshotEvent {
                base: base_event(),
                snapshot: new_state.clone(),
            }))?;
        } else {
            let patch = vec![serde_json::json!({
                "op": "replace",
                "path": "",
                "value": new_state,
            })];
            emit(tx, Event::StateDelta(StateDeltaEvent {
                base: base_event(),
                delta: patch,
            }))?;
        }
        Ok(())
    }
}