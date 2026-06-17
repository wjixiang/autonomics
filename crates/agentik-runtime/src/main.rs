//! `agentik-runtime` — the agent system portal binary.
//!
//! Two faces, one binary:
//! - `serve`   — run the long-lived daemon: embedded skill server + agent
//!               process manager + control-plane gRPC. Other CLI invocations
//!               (and, later, the TUI) connect to it.
//! - `agent` / `daemon` / `skill` — client subcommands that talk to a running
//!               daemon (or, for `skill`, operate on the DB directly).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agentik_api::{self as api, ContentBlock};
use agentik_control_client::ControlClient;
use agentik_runtime::{Runtime, RuntimeConfig, http, kinds};
use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    name = "agentik-runtime",
    version,
    about = "Agent system portal: long-lived daemon + control-plane CLI"
)]
struct Cli {
    /// Tracing filter (RUST_LOG-style), e.g. `info`, `debug`, `agentik_runtime=trace`.
    #[arg(long, global = true, default_value = "info")]
    log: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the runtime daemon: embedded skill server + agent process manager +
    /// control-plane gRPC, until interrupted.
    Serve {
        /// Address to bind the embedded skill server on (`127.0.0.1:0` = OS-assigned).
        #[arg(long, default_value = "127.0.0.1:0")]
        addr: SocketAddr,

        /// Path to the SQLite database backing the skill store.
        #[arg(long)]
        db: PathBuf,

        /// Skill directories to import on startup and to seed the coder skill tree from.
        #[arg(long = "skill-dir", num_args = 0..)]
        skill_dirs: Vec<PathBuf>,

        /// Optional path to a `ModelConfig` JSON file used to configure the pool.
        #[arg(long)]
        config: Option<PathBuf>,
    },

    /// Manage a running daemon.
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Control agents on a running daemon.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Manage the SQLite-backed skill store directly on the database file.
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Check whether the daemon is running (Ping).
    Status,
    /// Gracefully shut the daemon down.
    Stop,
}

#[derive(Subcommand)]
enum AgentAction {
    /// Spawn (and start) an agent by registered kind.
    Spawn {
        #[arg(long, default_value = "coder")]
        kind: String,
        /// Optional initial user message.
        #[arg(long)]
        message: Option<String>,
    },
    /// List managed agents.
    List,
    /// Inject a user message into an agent.
    Send {
        /// Agent id (UUID).
        id: String,
        /// Message text.
        text: String,
    },
    /// Stop a running agent.
    Stop { id: String },
    /// Get an agent's lifecycle status.
    Status { id: String },
    /// Subscribe to the event stream, filtered to one agent, until it exits.
    Follow { id: String },
    /// List models in the shared pool.
    Models,
    /// List registered agent kinds.
    Kinds,
}

#[derive(Subcommand)]
enum SkillAction {
    /// List skills in the store (with their dotpath).
    List {
        #[arg(long)]
        user_invocable: bool,
        #[arg(long)]
        model_invocable: bool,
    },
    /// Get a single skill by name/alias.
    Get { name: String },
    /// Print the full skill tree (root with children summary).
    Tree,
    /// Reload a skill from its source.
    Reload { name: String },
    /// Import all skills from a directory into the store (full replace),
    /// then refresh agent kinds.
    Import { dir: PathBuf },
    /// Export all skills from the store to a directory.
    Export { dir: PathBuf },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&cli.log).unwrap_or_else(|_| "info".into()),
        )
        .init();

    match cli.command {
        Command::Serve {
            addr,
            db,
            skill_dirs,
            config,
        } => serve(addr, db, skill_dirs, config).await,
        Command::Daemon { action } => daemon(action).await,
        Command::Agent { action } => agent(action).await,
        Command::Skill { action } => skill(action).await,
    }
}

// ── Daemon ─────────────────────────────────────────────────────

/// Run the runtime daemon until Ctrl-C or a `Shutdown` RPC.
async fn serve(
    addr: SocketAddr,
    db: PathBuf,
    skill_dirs: Vec<PathBuf>,
    config: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut rt_config = RuntimeConfig {
        skill_server_addr: Some(addr),
        db_path: Some(db),
        skill_dirs: skill_dirs.clone(),
        ..Default::default()
    };
    if let Some(path) = config {
        match load_model_config(&path) {
            Some(cfg) => rt_config = rt_config.with_model_config(cfg),
            None => tracing::warn!(path = %path.display(), "skipping model config"),
        }
    }

    let runtime = Runtime::new(rt_config).await?;

    // Register the built-in coder kind. Its skill tree is sourced from the
    // shared store (single source of truth); skill activation is wired up when
    // a skill server is running.
    let skill_client = runtime.skill_client().cloned();
    if let Some(store) = runtime.skill_store() {
        match kinds::coder_kind(&store, skill_client.clone()).await {
            Ok(blueprint) => runtime.registry().register(blueprint),
            Err(e) => tracing::warn!(error = %e, "failed to build coder kind from store"),
        }
    }

    let skill_addr = runtime.skill_server_addr();
    let pool_models = runtime.process_manager().pool_model_names().await;
    let pm = runtime.process_manager().clone();
    let store = runtime.skill_store();

    // ── Control-plane HTTP (REST + SSE) server ──
    let shutdown = CancellationToken::new();
    let control_listener = TcpListener::bind("127.0.0.1:0").await?;
    let control_addr = control_listener.local_addr()?;
    let state = http::HttpState::new(pm, store, skill_client, shutdown.clone());
    let app = http::router(state);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(control_listener, app).await {
            tracing::error!(error = %e, "control HTTP server error");
        }
    });

    // ── Write discovery file ──
    let info = api::DaemonInfo {
        control_addr,
        skill_addr,
        pid: std::process::id(),
        started_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    write_daemon_info(&info)?;

    println!("agentik-runtime daemon");
    println!("  control: http://{control_addr}");
    println!("  docs   : http://{control_addr}/docs");
    if let Some(sa) = skill_addr {
        println!("  skills : http://{sa}");
    }
    println!("  pool   : {} model(s)", pool_models.len());
    println!("  pid    : {}", info.pid);
    tracing::info!("daemon running — Ctrl-C or `daemon stop` to shut down");

    // ── Run until interrupted ──
    tokio::select! {
        _ = tokio::signal::ctrl_c() => println!("\nCtrl-C received, shutting down…"),
        _ = shutdown.cancelled() => println!("shutdown requested via control plane…"),
    }

    // ── Cleanup ──
    let _ = remove_daemon_info();
    let results = runtime.shutdown().await;
    println!("shutdown complete — {} agent(s) exited", results.len());
    Ok(())
}

/// Persist the discovery file.
fn write_daemon_info(info: &api::DaemonInfo) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(dir) = api::agentik_dir() {
        std::fs::create_dir_all(&dir)?;
    }
    let path = api::daemon_json_path()
        .ok_or_else(|| -> Box<dyn std::error::Error> { "no state dir".into() })?;
    std::fs::write(&path, serde_json::to_string_pretty(info)?)?;
    Ok(())
}

fn remove_daemon_info() -> std::io::Result<()> {
    if let Some(path) = api::daemon_json_path() {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

// ── Daemon client subcommands ──────────────────────────────────

async fn daemon(action: DaemonAction) -> Result<(), Box<dyn std::error::Error>> {
    let info = api::read_daemon_info().ok_or("daemon not running (no discovery file)")?;
    let mut client = ControlClient::connect(&format!("http://{}", info.control_addr)).await?;
    match action {
        DaemonAction::Status => {
            client.ping().await?;
            println!(
                "daemon up — control http://{} pid {} (started {})",
                info.control_addr, info.pid, info.started_at
            );
        }
        DaemonAction::Stop => {
            client.shutdown().await?;
            println!("shutdown requested");
        }
    }
    Ok(())
}

// ── Agent client subcommands ───────────────────────────────────

async fn agent(action: AgentAction) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = agentik_control_client::connect_to_daemon()
        .await
        .map_err(|e| format!("{e}"))?;
    match action {
        AgentAction::Spawn { kind, message } => {
            use agentik_api::AgentSpawnOpts;
            let opts = AgentSpawnOpts {
                initial_message: message.map(|t| vec![ContentBlock::Text { text: t }]),
                ..Default::default()
            };
            let id = client
                .spawn_agent(&kind, &opts)
                .await
                .map_err(|e| format!("{e}"))?;
            client.start_agent(id).await.map_err(|e| format!("{e}"))?;
            println!("spawned + started {kind} agent: {id}");
        }
        AgentAction::List => {
            let agents = client.list_agents().await.map_err(|e| format!("{e}"))?;
            if agents.is_empty() {
                println!("(no agents)");
            }
            for a in agents {
                println!("{}  {:?}  {}", a.agent_id, a.status, a.kind);
            }
        }
        AgentAction::Send { id, text } => {
            let id = uuid::Uuid::parse_str(&id)?;
            client
                .inject_message(id, vec![ContentBlock::Text { text }])
                .await
                .map_err(|e| format!("{e}"))?;
            println!("sent");
        }
        AgentAction::Stop { id } => {
            let id = uuid::Uuid::parse_str(&id)?;
            client.stop_agent(id).await.map_err(|e| format!("{e}"))?;
            println!("stopped");
        }
        AgentAction::Status { id } => {
            let id = uuid::Uuid::parse_str(&id)?;
            let status = client.get_status(id).await.map_err(|e| format!("{e}"))?;
            println!("{status:?}");
        }
        AgentAction::Follow { id } => {
            let id = uuid::Uuid::parse_str(&id)?;
            use tokio_stream::StreamExt as _;
            let mut stream = client.stream_events().await.map_err(|e| format!("{e}"))?;
            while let Some(item) = stream.next().await {
                let event = item.map_err(|e| format!("{e}"))?;
                if !event_concerns(&event, id) {
                    continue;
                }
                print_event(&event);
                if let agentik_api::ProcessEvent::ProcessExited { agent_id, .. } = &event {
                    if *agent_id == id {
                        break;
                    }
                }
            }
        }
        AgentAction::Models => {
            let models = client.get_pool_models().await.map_err(|e| format!("{e}"))?;
            if models.is_empty() {
                println!("(pool not configured)");
            }
            for m in models {
                println!("- {m}");
            }
        }
        AgentAction::Kinds => {
            let kinds = client.list_kinds().await.map_err(|e| format!("{e}"))?;
            for k in kinds {
                println!("- {k}");
            }
        }
    }
    Ok(())
}

/// Does this `ProcessEvent` concern the given agent?
fn event_concerns(event: &agentik_api::ProcessEvent, id: uuid::Uuid) -> bool {
    use agentik_api::ProcessEvent;
    match event {
        ProcessEvent::Agent { agent_id, .. }
        | ProcessEvent::StateChanged { agent_id, .. }
        | ProcessEvent::ProcessExited { agent_id, .. } => *agent_id == id,
    }
}

/// Print a one-line summary of a `ProcessEvent`.
fn print_event(event: &agentik_api::ProcessEvent) {
    use agentik_api::{AgentEvent, ProcessEvent};
    match event {
        ProcessEvent::StateChanged { new_status, .. } => {
            println!("[state] {new_status:?}");
        }
        ProcessEvent::ProcessExited { status, .. } => {
            println!("[exited] {status:?}");
        }
        ProcessEvent::Agent { event, .. } => match event {
            AgentEvent::TextDelta(t) => print!("{t}"),
            AgentEvent::ThinkingDelta(t) => print!("\x1b[2m{t}\x1b[0m"),
            AgentEvent::LlmResponse(s) => println!("[llm] {s}"),
            AgentEvent::Thinking(s) => println!("[think] {s}"),
            AgentEvent::ToolCall { name, input } => {
                println!(
                    "[tool] {name} {}",
                    serde_json::to_string(input).unwrap_or_default()
                );
            }
            AgentEvent::ToolResult { ok, content } => {
                println!("[result ok={ok}] {content}");
            }
            AgentEvent::Requesting => println!("[requesting]"),
            AgentEvent::Done => println!("[done]"),
            AgentEvent::Error(e) => println!("[error] {e}"),
            other => println!("[event] {other:?}"),
        },
    }
}

// ── Skill management subcommands (via the control plane) ───────

async fn skill(action: SkillAction) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = agentik_control_client::connect_to_daemon()
        .await
        .map_err(|e| format!("{e}"))?;
    match action {
        SkillAction::List {
            user_invocable,
            model_invocable,
        } => {
            let skills = client
                .list_skills(user_invocable, model_invocable)
                .await
                .map_err(|e| format!("{e}"))?;
            if skills.is_empty() {
                println!("(no skills)");
            }
            for s in &skills {
                println!("- {} : {}", s.dotpath, s.name);
                if !s.description.is_empty() {
                    println!("    {}", s.description);
                }
            }
            println!("{} skill(s) total", skills.len());
        }
        SkillAction::Get { name } => {
            match client.get_skill(&name).await.map_err(|e| format!("{e}"))? {
                Some(s) => {
                    println!("{} ({})", s.name, s.dotpath);
                    println!("  {}", s.description);
                    if !s.allowed_tools.is_empty() {
                        println!("  tools: {}", s.allowed_tools.join(", "));
                    }
                    if !s.body.is_empty() {
                        println!("---");
                        println!("{}", s.body);
                    }
                }
                None => println!("skill '{name}' not found"),
            }
        }
        SkillAction::Tree => match client.get_skill_tree().await.map_err(|e| format!("{e}"))? {
            Some(root) => print_skill_tree(&root, 0),
            None => println!("(skill tree is empty)"),
        },
        SkillAction::Reload { name } => {
            match client
                .reload_skill(&name)
                .await
                .map_err(|e| format!("{e}"))?
            {
                Some(_) => println!("reloaded '{name}'"),
                None => println!("'{name}' unchanged or not found"),
            }
        }
        SkillAction::Import { dir } => {
            let n = client
                .import_skills(&dir.display().to_string())
                .await
                .map_err(|e| format!("{e}"))?;
            println!(
                "imported {n} skill(s) from {} (coder kind refreshed)",
                dir.display()
            );
        }
        SkillAction::Export { dir } => {
            let n = client
                .export_skills(&dir.display().to_string())
                .await
                .map_err(|e| format!("{e}"))?;
            println!("exported {n} skill(s) to {}", dir.display());
        }
    }
    Ok(())
}

/// Recursively print a skill tree node with indentation.
fn print_skill_tree(node: &agentik_control_client::SkillTreeNodeWire, depth: usize) {
    let indent = "  ".repeat(depth);
    println!("{indent}- {} ({})", node.skill.name, node.dotpath);
    for child in &node.children {
        print_skill_tree(child, depth + 1);
    }
}

/// Read and parse a `ModelConfig` JSON file. Returns `None` on any error.
fn load_model_config(path: &Path) -> Option<agentik_runtime::ModelConfig> {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to read model config");
            return None;
        }
    };
    match serde_json::from_str::<agentik_runtime::ModelConfig>(&data) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "failed to parse model config");
            None
        }
    }
}
