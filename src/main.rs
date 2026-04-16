mod agent;
mod agent_prompt;
mod client;
mod config;
mod error;
mod event;
mod memory;
mod session;
mod summarizer;
mod tools;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use futures::StreamExt;

use agent::Agent;
use session::sqlite_store::SqliteSessionStore;
use session::SessionStore;
use tools::ToolRegistry;

#[derive(Parser, Debug)]
#[command(name = "agent-forge")]
#[command(about = "AI Agent Forge - Build and run agents with LLM providers")]
struct Cli {
    #[arg(long, help = "Path to agent prompt markdown file")]
    agent: Option<PathBuf>,

    #[arg(long, help = "Path to config JSON file")]
    config: Option<PathBuf>,

    #[arg(long, help = "Provider name (overrides config)")]
    provider: Option<String>,

    #[arg(long, help = "Model to use (overrides config)")]
    model: Option<String>,

    #[arg(long, help = "Provider base URL (overrides config)")]
    url: Option<String>,

    #[arg(long, help = "API key for the provider (overrides config)")]
    api_key: Option<String>,

    #[arg(long, help = "Disable built-in file tools")]
    no_tools: bool,

    #[arg(long, help = "Temperature for generation (overrides config)")]
    temperature: Option<f32>,

    #[arg(long, help = "Thread ID for session persistence")]
    thread_id: Option<String>,

    #[arg(long, help = "Path to SQLite database (overrides config)")]
    db_path: Option<String>,

    #[arg(long, help = "Working directory for file tools (overrides config)")]
    workdir: Option<PathBuf>,

    #[arg(long, help = "Number of recent messages to include in context (overrides config)")]
    context_messages: Option<u32>,

    #[arg(long, help = "Disable conversation summarization")]
    no_summarize: bool,

    #[arg(long, help = "Enable thinking/reasoning for supported models")]
    think: bool,

    #[arg(long, help = "Maximum number of tokens to generate (overrides config)")]
    max_tokens: Option<u32>,

    #[arg(help = "Message to send to the agent")]
    message: String,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let config = match &cli.config {
        Some(path) => config::Config::from_file(path)?,
        None => config::Config::default(),
    };

    let config = config.merge_cli_overrides(
        cli.provider.clone(),
        cli.model.clone(),
        cli.url.clone(),
        cli.temperature,
        cli.api_key.clone(),
        cli.db_path.clone(),
        cli.workdir.clone(),
        cli.context_messages,
        cli.no_summarize,
        cli.think,
        cli.max_tokens,
    );

    let llm_client = client::create_client(&config.provider)?;

    let system_prompt = if let Some(agent_path) = &cli.agent {
        let prompt = agent_prompt::AgentPrompt::from_file(agent_path)?;
        prompt.to_system_prompt()
    } else {
        String::new()
    };

    let mut tools = if cli.no_tools {
        ToolRegistry::new()
    } else {
        ToolRegistry::with_defaults()
    };

    if let Some(ref workdir) = config.resolved_workdir() {
        tools = tools.with_workdir(workdir.clone());
    }

    let mut agent_builder = Agent::builder(llm_client)
        .model(&config.provider.model)
        .system_prompt(&system_prompt)
        .tools(tools)
        .summarize(config.summarize)
        .context_messages(config.context_messages)
        .think(config.provider.think);

    if let Some(temp) = config.provider.temperature {
        agent_builder = agent_builder.temperature(temp);
    }

    if let Some(max_tokens) = config.provider.max_tokens {
        agent_builder = agent_builder.max_tokens(max_tokens);
    }

    if let Some(ref summary_model) = config.provider.summary_model {
        agent_builder = agent_builder.summary_model(summary_model);
    }

    let store: Arc<dyn SessionStore> = match &config.db_path {
        Some(path) => Arc::new(SqliteSessionStore::open(path)?),
        None => Arc::new(session::InMemoryStore::new()),
    };
    agent_builder = agent_builder.store(store);

    let agent = agent_builder.build()?;

    let thread_id = cli.thread_id
        .map(|s| event::ThreadId::from(uuid::Uuid::parse_str(&s).unwrap_or_else(|_| uuid::Uuid::new_v4())))
        .unwrap_or_else(event::ThreadId::random);

    let mut session = agent.session_with_id(thread_id);

    let mut rx = session.run(&cli.message).await?;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    while let Some(event_result) = rx.next().await {
        match event_result {
            Ok(event) => {
                use event::Event;
                match event {
                    Event::RunStarted(_) => {}
                    Event::StateSnapshot(e) => {
                        eprintln!("[State: {}]", serde_json::to_string_pretty(&e.snapshot).unwrap_or_default());
                    }
                    Event::StateDelta(_) => {
                        eprintln!("[State updated]");
                    }
                    Event::ThinkingTextMessageStart(_) => {
                        eprint!("\x1b[90m");
                        let _ = out.flush();
                    }
                    Event::ThinkingTextMessageContent(e) => {
                        eprint!("{}", e.delta);
                        let _ = out.flush();
                    }
                    Event::ThinkingTextMessageEnd(_) => {
                        eprintln!("\x1b[0m");
                        let _ = out.flush();
                    }
                    Event::TextMessageStart(_) => {}
                    Event::TextMessageContent(e) => {
                        write!(out, "{}", e.delta).unwrap();
                        let _ = out.flush();
                    }
                    Event::TextMessageEnd(_) => {}
                    Event::ToolCallStart(e) => {
                        eprintln!("\n[Calling tool: {}]", e.tool_call_name);
                    }
                    Event::ToolCallArgs(e) => {
                        eprintln!("  args: {}", e.delta);
                    }
                    Event::ToolCallEnd(_) => {}
                    Event::ToolCallResult(e) => {
                        let content = if e.content.len() > 200 {
                            format!("{}...", &e.content[..200])
                        } else {
                            e.content
                        };
                        eprintln!("\n[Tool result: {}]", content);
                    }
                    Event::RunFinished(_) => {
                        println!("\n[Done]");
                    }
                    Event::RunError(e) => {
                        eprintln!("[Error: {}]", e.message);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}