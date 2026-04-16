use serde_json::Value;
use uuid::Uuid;

macro_rules! define_id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn random() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }
    };
}

define_id_type!(ThreadId);
define_id_type!(RunId);
define_id_type!(MessageId);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub String);

impl ToolCallId {
    pub fn random() -> Self {
        let id = format!("call_{}", &Uuid::new_v4().to_string()[..8]);
        Self(id)
    }
}

impl std::fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for ToolCallId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Assistant,
    Tool,
}

pub struct RunStartedEvent {
    pub thread_id: ThreadId,
    pub run_id: RunId,
}

pub struct RunFinishedEvent {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub result: Option<Value>,
}

pub struct RunErrorEvent {
    pub message: String,
    pub code: Option<String>,
}

pub struct TextMessageStartEvent {
    pub message_id: MessageId,
    pub role: Role,
}

pub struct TextMessageContentEvent {
    pub message_id: MessageId,
    pub delta: String,
}

pub struct TextMessageEndEvent {
    pub message_id: MessageId,
}

pub struct ThinkingTextMessageStartEvent;

pub struct ThinkingTextMessageContentEvent {
    pub delta: String,
}

pub struct ThinkingTextMessageEndEvent;

pub struct ToolCallStartEvent {
    pub tool_call_id: ToolCallId,
    pub tool_call_name: String,
    pub parent_message_id: Option<MessageId>,
}

pub struct ToolCallArgsEvent {
    pub tool_call_id: ToolCallId,
    pub delta: String,
}

pub struct ToolCallEndEvent {
    pub tool_call_id: ToolCallId,
}

pub struct ToolCallResultEvent {
    pub message_id: MessageId,
    pub tool_call_id: ToolCallId,
    pub content: String,
    pub role: Role,
}

pub struct StateSnapshotEvent {
    pub snapshot: Value,
}

pub struct StateDeltaEvent {
    pub delta: Vec<Value>,
}

pub enum Event {
    RunStarted(RunStartedEvent),
    RunFinished(RunFinishedEvent),
    RunError(RunErrorEvent),
    TextMessageStart(TextMessageStartEvent),
    TextMessageContent(TextMessageContentEvent),
    TextMessageEnd(TextMessageEndEvent),
    ThinkingTextMessageStart(ThinkingTextMessageStartEvent),
    ThinkingTextMessageContent(ThinkingTextMessageContentEvent),
    ThinkingTextMessageEnd(ThinkingTextMessageEndEvent),
    ToolCallStart(ToolCallStartEvent),
    ToolCallArgs(ToolCallArgsEvent),
    ToolCallEnd(ToolCallEndEvent),
    ToolCallResult(ToolCallResultEvent),
    StateSnapshot(StateSnapshotEvent),
    StateDelta(StateDeltaEvent),
}
