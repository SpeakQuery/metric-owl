mod ai;
mod analysis;
mod api;
#[cfg(test)]
mod api_email_tests;
mod auth;
mod boot;
mod chat;
mod collect;
mod diagnostics;
mod discovery;
mod disk_defaults_migration;
mod docker_helper;
mod integrations;
mod mcp;
mod probes;
mod recovery;
mod smtp;
mod targets;
mod web_access;
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
    /// Create the first administrator from bounded JSON stdin; MFA can be enabled later.
    PrepareAdmin,
    /// Recover an existing account locally as root. Passwords are read from a hidden terminal prompt.
    RecoverAdmin {
        /// Display the administrator username without changing the account.
        #[arg(long)]
        show_username: bool,
    },
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
    #[serde(default = "legacy_email_transport")]
    pub email_transport: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: String,
    pub smtp_from: String,
    pub recipient: String,
    pub recipient_verified: bool,
    pub webhook_url: String,
    pub ai_provider: String,
    pub ai_model: String,
    pub ai_base_url: String,
    pub ai_auto: bool,
}
fn legacy_email_transport() -> String {
    "relay".into()
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
            email_transport: "disabled".into(),
            smtp_host: String::new(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            smtp_username: String::new(),
            smtp_from: String::new(),
            recipient: String::new(),
            recipient_verified: false,
            webhook_url: String::new(),
            ai_provider: "xai".into(),
            ai_model: String::new(),
            ai_base_url: "https://api.x.ai/v1".into(),
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
    pub web_access: web_access::WebAccess,
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
    if let Some(Command::RecoverAdmin { show_username }) = &cli.command {
        return recovery::run(&cli.data_dir, *show_username).await;
    }
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
    if matches!(cli.command, Some(Command::PrepareAdmin)) {
        use std::io::{IsTerminal, Read};
        anyhow::ensure!(
            !std::io::stdin().is_terminal(),
            "prepare-admin requires JSON through stdin; do not type credentials into a terminal"
        );
        let mut bytes = Vec::new();
        std::io::stdin().take(4097).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 4096, "Enrollment input exceeds 4096 bytes");
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EnrollmentInput {
            username: String,
            password: String,
        }
        let input: EnrollmentInput = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Expected JSON with username and password"))?;
        auth.prepare_admin(input.username, input.password, &cli.data_dir)
            .await?;
        println!("{{\"configured\":true,\"mfa_required\":false}}");
        return Ok(());
    }
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
    let web_access = web_access::WebAccess::from_env()?;
    // A held exclusive socket prevents two collectors writing the same data directory.
    let ipc = mcp::bind_ipc(&cli.data_dir).await?;
    auth.bootstrap(&cli.data_dir).await?;
    guard_core::migration::english(&store).await?;
    disk_defaults_migration::run(&store).await?;
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
        web_access,
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
    if let Some(origin) = app.web_access.public_origin() {
        eprintln!("Metric Owl: {origin}/admin (authenticated HTTPS proxy)");
    } else {
        eprintln!("Metric Owl: http://127.0.0.1:{port}/admin (local access only)");
    }
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
