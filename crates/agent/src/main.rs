mod api;
mod auth;
mod boot;
mod collect;
mod diagnostics;
mod discovery;
mod docker_helper;
mod integrations;
mod mcp;
mod probes;
mod targets;
use clap::{Parser, Subcommand};
use guard_core::{model::*, store::Store};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf, sync::Arc};
use tokio::sync::{RwLock, broadcast};
#[derive(Parser)]
#[command(version, about = "Metric Owl — read-only server monitoring")]
struct Cli {
    #[arg(long, env = "GUARD_DATA_DIR", default_value = ".local-data")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long, default_value_t = 4877)]
        port: u16,
    },
    Mcp {
        #[arg(
            long,
            env = "GUARD_MCP_KEY",
            required_unless_present = "key_file",
            conflicts_with = "key_file"
        )]
        key: Option<String>,
        #[arg(long, conflicts_with = "key")]
        key_file: Option<PathBuf>,
    },
    DockerHelper {
        #[arg(long)]
        socket: PathBuf,
    },
    Doctor,
    ResetSetup,
    ReportJob {
        #[arg(long)]
        id: String,
        #[arg(long, value_parser = ["running", "success", "failure"])]
        state: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub alias: String,
    pub timezone: String,
    pub report_times: Vec<String>,
    pub instant: bool,
    pub recovery: bool,
    pub reports: bool,
    pub quota_bytes: u64,
    pub relay_url: String,
    pub recipient: String,
    pub recipient_verified: bool,
    pub webhook_url: String,
    pub ai_provider: String,
    pub ai_model: String,
    pub ai_base_url: String,
    pub ai_auto: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            alias: "My server".into(),
            timezone: iana_time_zone::get_timezone().unwrap_or("Europe/Istanbul".into()),
            report_times: vec!["09:00".into()],
            instant: true,
            recovery: true,
            reports: true,
            quota_bytes: 1024 * 1024 * 1024,
            relay_url: String::new(),
            recipient: String::new(),
            recipient_verified: false,
            webhook_url: String::new(),
            ai_provider: "openai".into(),
            ai_model: String::new(),
            ai_base_url: "https://api.groq.com/openai/v1".into(),
            ai_auto: false,
        }
    }
}
#[derive(Clone)]
pub struct App {
    pub store: Store,
    pub auth: auth::Auth,
    pub dir: PathBuf,
    pub port: u16,
    pub host: Host,
    pub latest: Arc<RwLock<Option<Sample>>>,
    pub ring: Arc<RwLock<VecDeque<Sample>>>,
    pub events: broadcast::Sender<String>,
    pub rules: Arc<RwLock<Vec<Rule>>>,
    pub settings: Arc<RwLock<Settings>>,
    pub targets: Arc<RwLock<Vec<Target>>>,
    pub target_status: Arc<RwLock<Vec<TargetStatus>>>,
    pub inventory: Arc<RwLock<discovery::Inventory>>,
    pub errors: Arc<RwLock<Vec<String>>>,
    pub config_lock: Arc<tokio::sync::Mutex<()>>,
    pub analysis_lock: Arc<tokio::sync::Mutex<()>>,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("server_guard=info,guard_core=info")
        .init();
    let cli = Cli::parse();
    if let Some(Command::ReportJob { id, state }) = &cli.command {
        return probes::report_job(&cli.data_dir, id, state).await;
    }
    if let Some(Command::Mcp { key, key_file }) = &cli.command {
        let key = match (key, key_file) {
            (Some(key), None) => key.clone(),
            (None, Some(path)) => mcp::read_key_file(path)?,
            _ => anyhow::bail!("Provide exactly one MCP key or private key file"),
        };
        return mcp::bridge(cli.data_dir.clone(), key).await;
    }
    std::fs::create_dir_all(&cli.data_dir)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&cli.data_dir, std::fs::Permissions::from_mode(0o700))?;
    if let Some(Command::DockerHelper { socket }) = cli.command {
        return docker_helper::run(socket).await;
    }
    let store = Store::open(&cli.data_dir.join("guard.sqlite"))?;
    let auth = auth::Auth::new(store.clone(), &cli.data_dir).await?;
    if matches!(cli.command, Some(Command::ResetSetup)) {
        anyhow::ensure!(
            !auth.configured().await?,
            "An administrator already exists; the setup key cannot be renewed"
        );
        store
            .put("bootstrap", "{\"hash\":\"\",\"expires\":0}".into())
            .await?;
        auth.bootstrap(&cli.data_dir).await?;
        return Ok(());
    }
    let id = match store.get("host_id").await? {
        Some(x) => x,
        None => {
            let x = uuid::Uuid::new_v4().to_string();
            store.put("host_id", x.clone()).await?;
            x
        }
    };
    let host = collect::host(id)?;
    if matches!(cli.command, Some(Command::Doctor)) {
        println!("{}", serde_json::to_string_pretty(&host)?);
        println!(
            "SQLite: {} bytes; administrator configured: {}",
            store.size_bytes(),
            auth.configured().await?
        );
        return Ok(());
    }
    let port = match cli.command {
        Some(Command::Serve { port }) => port,
        _ => 4877,
    };
    // A held exclusive socket prevents two collectors writing the same data directory.
    let ipc = mcp::bind_ipc(&cli.data_dir).await?;
    auth.bootstrap(&cli.data_dir).await?;
    guard_core::migration::english(&store).await?;
    let mut rules = store
        .get("rules")
        .await?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_else(default_rules);
    if store.get("disk_defaults_seeded").await?.is_none() {
        rules.extend(collect::disk_defaults()?);
        store.put("rules", serde_json::to_string(&rules)?).await?;
        store.put("disk_defaults_seeded", "true".into()).await?;
    }
    if store.get("diagnostic_defaults_v1_seeded").await?.is_none() {
        for rule in diagnostics::default_rules() {
            if rules.len() >= 200 {
                tracing::warn!(
                    "Some diagnostic defaults were not added because the 200-rule limit was reached"
                );
                break;
            }
            if !rules.iter().any(|existing| existing.id == rule.id) {
                rules.push(rule);
            }
        }
        store.put("rules", serde_json::to_string(&rules)?).await?;
        store
            .put("diagnostic_defaults_v1_seeded", "true".into())
            .await?;
    }
    let settings = store
        .get("settings")
        .await?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    let targets = store
        .get("targets")
        .await?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default();
    let (events, _) = broadcast::channel(32);
    let app = App {
        store,
        auth,
        dir: cli.data_dir,
        port,
        host,
        latest: Default::default(),
        ring: Default::default(),
        events,
        rules: Arc::new(RwLock::new(rules)),
        settings: Arc::new(RwLock::new(settings)),
        targets: Arc::new(RwLock::new(targets)),
        target_status: Default::default(),
        inventory: Default::default(),
        errors: Default::default(),
        config_lock: Default::default(),
        analysis_lock: Default::default(),
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    if let Err(error) = boot::check(&app).await {
        app.error(format!("Restart detection unavailable: {error}"))
            .await;
    }
    collect::start(app.clone());
    discovery::start(app.clone());
    tokio::spawn(targets::run(app.clone()));
    tokio::spawn(integrations::run(app.clone()));
    tokio::spawn(mcp::serve_ipc(ipc, app.clone()));
    eprintln!("Metric Owl: http://127.0.0.1:{port}/admin (local access only)");
    axum::serve(listener, api::router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
impl App {
    pub async fn error(&self, message: String) {
        tracing::warn!("{}", message);
        let mut errors = self.errors.write().await;
        if errors.last() != Some(&message) {
            errors.push(message);
            if errors.len() > 20 {
                errors.remove(0);
            }
        }
    }
}
