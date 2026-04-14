mod agent;
mod agent_prompt;
mod client;
mod error;
mod memory;
mod session;
mod tools;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use futures::StreamExt;

use agent::Agent;
use client::ollama::OllamaClient;
use tools::ToolRegistry;

#[derive(Parser, Debug)]
#[command(name = "agent-forge")]
#[command(about = "AI Agent Forge - Build and run agents with Ollama")]
struct Cli {
    #[arg(long, help = "Path to agent prompt markdown file")]
    agent: Option<PathBuf>,

    #[arg(long, default_value = "llama3.2", help = "Model to use")]
    model: String,

    #[arg(long, help = "Ollama base URL")]
    url: Option<String>,

    #[arg(long, help = "Disable built-in file tools")]
    no_tools: bool,

    #[arg(long, help = "Temperature for generation")]
    temperature: Option<f32>,

    #[arg(long, help = "Thread ID for session persistence")]
    thread_id: Option<String>,

    #[arg(help = "Message to send to the agent")]
    message: String,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let ollama_url = cli.url.unwrap_or_else(|| "http://localhost:11434".to_string());
    let client = Arc::new(OllamaClient::new().with_base_url(ollama_url));

    let system_prompt = if let Some(agent_path) = &cli.agent {
        let prompt = agent_prompt::AgentPrompt::from_file(agent_path)?;
        prompt.to_system_prompt()
    } else {
        String::new()
    };

    let tools = if cli.no_tools {
        ToolRegistry::new()
    } else {
        ToolRegistry::with_defaults()
    };

    let mut agent_builder = Agent::builder(client)
        .model(&cli.model)
        .system_prompt(&system_prompt)
        .tools(tools);

    if let Some(temp) = cli.temperature {
        agent_builder = agent_builder.temperature(temp);
    }

    let agent = agent_builder.build()?;

    let thread_id = cli.thread_id
        .map(|s| ag_ui_core::types::ids::ThreadId::from(uuid::Uuid::parse_str(&s).unwrap_or_else(|_| uuid::Uuid::new_v4())))
        .unwrap_or_else(ag_ui_core::types::ids::ThreadId::random);

    let mut session = agent.session_with_id(thread_id);

    let mut rx = session.run(&cli.message).await?;

    while let Some(event_result) = rx.next().await {
        match event_result {
            Ok(event) => {
                use ag_ui_core::event::*;
                match event {
                    Event::RunStarted(_) => {}
                    Event::StateSnapshot(e) => {
                        eprintln!("[State: {}]", serde_json::to_string_pretty(&e.snapshot).unwrap_or_default());
                    }
                    Event::StateDelta(_) => {
                        eprintln!("[State updated]");
                    }
                    Event::TextMessageStart(_) => {}
                    Event::TextMessageContent(e) => {
                        print!("{}", e.delta);
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
                    _ => {}
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}