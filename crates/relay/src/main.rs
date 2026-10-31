macro_rules! ensure { ($condition:expr, $($arg:tt)*) => { if !$condition { return Err(anyhow::anyhow!($($arg)*).into()); } }; }
#[cfg(test)]
mod email_tests;
mod provider;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use clap::{Parser, Subcommand};
use guard_core::store::Store;
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};
#[derive(Parser)]
struct Cli {
    #[arg(long, env = "RELAY_DATA_DIR", default_value = ".local-data/relay")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Provision {
        #[arg(long)]
        name: String,
    },
    Revoke {
        #[arg(long)]
        device_id: String,
    },
    Devices,
    /// Verify SMTP TLS/authentication without sending mail or opening the queue.
    SmtpCheck,
    Serve {
        #[arg(long, default_value_t = 4878)]
        port: u16,
        #[arg(long)]
        local_test: bool,
    },
}
#[derive(Clone)]
struct App {
    store: Store,
    dir: PathBuf,
    test: bool,
    provider: Arc<provider::Provider>,
}
struct Error(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            if self.0.to_string().contains("quota") {
                StatusCode::TOO_MANY_REQUESTS
            } else {
                StatusCode::BAD_REQUEST
            },
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
type Api = std::result::Result<Json<Value>, Error>;
fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn token() -> String {
    let mut b = [0; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut b);
    hex::encode(b)
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Command::SmtpCheck) {
        provider::Provider::from_env(false)?.check_smtp().await?;
        println!("SMTP TLS and authentication verified. No email was sent.");
        return Ok(());
    }
    std::fs::create_dir_all(&cli.data_dir)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&cli.data_dir, std::fs::Permissions::from_mode(0o700))?;
    let store = Store::open(&cli.data_dir.join("relay.sqlite"))?;
    store.write(|c|{c.execute_batch("CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY,name TEXT,hash TEXT UNIQUE,revoked INTEGER DEFAULT 0);CREATE TABLE IF NOT EXISTS recipients(device TEXT,email TEXT,verified INTEGER DEFAULT 0,code_hash TEXT,expires REAL,attempts INTEGER DEFAULT 0,PRIMARY KEY(device,email));")?;Ok(())}).await?;
    guard_core::migration::english(&store).await?;
    match cli.command {
        Command::Provision { name } => {
            let key = token();
            let id = uuid::Uuid::new_v4().to_string();
            let h = hash(&key);
            let file = cli.data_dir.join(format!("device-{id}.key"));
            store
                .write(move |c| {
                    c.execute(
                        "INSERT INTO devices(id,name,hash) VALUES(?1,?2,?3)",
                        params![id, name, h],
                    )?;
                    Ok(())
                })
                .await?;
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)?;
            f.write_all(key.as_bytes())?;
            println!("Device key: {}", file.display());
        }
        Command::Revoke { device_id } => {
            store
                .write(move |c| {
                    c.execute("UPDATE devices SET revoked=1 WHERE id=?1", [device_id])?;
                    Ok(())
                })
                .await?;
            println!("Device sending permission revoked.");
        }
        Command::Devices => {
            let devices=store.read(|c|{let mut s=c.prepare("SELECT id,name,revoked FROM devices ORDER BY name")?;Ok(s.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"revoked":r.get::<_,bool>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)}).await?;
            println!("{}", serde_json::to_string_pretty(&devices)?);
        }
        Command::Serve { port, local_test } => {
            let provider = provider::Provider::from_env(local_test)?;
            let provider_name = provider.name();
            let app = App {
                store,
                dir: cli.data_dir,
                test: local_test,
                provider: Arc::new(provider),
            };
            tokio::spawn(worker(app.clone()));
            let router = Router::new()
                .route("/v1/recipient/start", post(start))
                .route("/v1/recipient/verify", post(verify))
                .route("/v1/send", post(send))
                .route("/v1/status", post(status))
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
                .with_state(app);
            let listener =
                tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
            eprintln!(
                "Notification relay localhost:{port}; provider={provider_name}; test={local_test}"
            );
            axum::serve(listener, router).await?;
        }
        Command::SmtpCheck => unreachable!("handled before opening queue"),
    }
    Ok(())
}
async fn device(a: &App, h: &HeaderMap) -> Result<String> {
    let t = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| anyhow::anyhow!("Device authorization required"))?;
    let hash = hash(t);
    a.store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id FROM devices WHERE hash=?1 AND revoked=0",
                [hash],
                |r| r.get(0),
            )?)
        })
        .await
}
#[derive(Deserialize)]
struct Address {
    recipient: String,
    code: Option<String>,
}
fn email(e: &str) -> bool {
    e.len() <= 254
        && e.len() >= 5
        && !e.chars().any(char::is_whitespace)
        && e.matches('@').count() == 1
        && e.split('@').next_back().is_some_and(|s| s.contains('.'))
        && !e.contains(['\r', '\n', '<', '>'])
}
async fn start(State(a): State<App>, headers: HeaderMap, Json(body): Json<Address>) -> Api {
    let d = device(&a, &headers).await?;
    ensure!(email(&body.recipient), "Invalid email address");
    ensure!(
        a.store.budget(&format!("verify:{d}"), 5).await?,
        "Verification sending quota reached"
    );
    use rand::Rng;
    let code = format!("{:06}", rand::thread_rng().gen_range(0..1000000));
    let ch = hash(&code);
    let recipient = body.recipient.to_lowercase();
    let r = recipient.clone();
    let dev = d.clone();
    a.store.write(move|c|{c.execute("INSERT INTO recipients(device,email,code_hash,expires) VALUES(?1,?2,?3,?4) ON CONFLICT(device,email) DO UPDATE SET verified=0,code_hash=excluded.code_hash,expires=excluded.expires,attempts=0",params![dev,r,ch,guard_core::now()+600.])?;Ok(())}).await?;
    let content = guard_core::email::verification(&code)?;
    a.store.enqueue(format!("verify:{}",uuid::Uuid::new_v4()),"email".into(),json!({"To":recipient,"Subject":content.subject,"TextBody":content.text,"HtmlBody":content.html,"device":d})).await?;
    Ok(Json(json!({"status":"queued","test_mode":a.test})))
}
async fn verify(State(a): State<App>, headers: HeaderMap, Json(body): Json<Address>) -> Api {
    let d = device(&a, &headers).await?;
    let code = hash(&body.code.unwrap_or_default());
    let email = body.recipient.to_lowercase();
    let ok=a.store.write(move|c|{let tx=c.transaction()?;let row:Option<(String,f64,u32)>=tx.query_row("SELECT code_hash,expires,attempts FROM recipients WHERE device=?1 AND email=?2",params![d,email],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;let Some((expected,expires,attempts))=row else{return Ok(false)};if attempts>=5||expires<guard_core::now(){return Ok(false);}use subtle::ConstantTimeEq;let valid:bool=expected.as_bytes().ct_eq(code.as_bytes()).into();tx.execute("UPDATE recipients SET attempts=attempts+1,verified=?1 WHERE device=?2 AND email=?3",params![valid,d,email])?;if valid{tx.execute("UPDATE recipients SET expires=0,code_hash='' WHERE device=?1 AND email=?2",params![d,email])?;}tx.commit()?;Ok(valid)}).await?;
    ensure!(ok, "Invalid or expired code");
    Ok(Json(json!({"verified":true})))
}
#[derive(Deserialize)]
struct Message {
    id: String,
    recipient: String,
    notification: Value,
}
async fn send(State(a): State<App>, headers: HeaderMap, Json(m): Json<Message>) -> Api {
    let d = device(&a, &headers).await?;
    ensure!(m.id.len() <= 200 && email(&m.recipient), "Invalid message");
    let recipient = m.recipient.to_lowercase();
    let dev = d.clone();
    let r = recipient.clone();
    let verified = a
        .store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT verified FROM recipients WHERE device=?1 AND email=?2",
                params![dev, r],
                |r| r.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false))
        })
        .await?;
    ensure!(verified, "Recipient not verified");
    let id = format!("{d}:{}", m.id);
    let check = id.clone();
    if a.store
        .read(move |c| {
            Ok(
                c.query_row("SELECT 1 FROM outbox WHERE id=?1", [check], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()?
                .is_some(),
            )
        })
        .await?
    {
        return Ok(Json(json!({"status":"queued","id":id})));
    }
    ensure!(
        a.store
            .reserve_budget(&format!("send:{d}"), &id, 60)
            .await?,
        "Daily device sending quota reached"
    );
    // A relay device's payload is not a verified panel-origin configuration.
    // Rebuild bounded HTML without a CTA; direct SMTP can use local WebAccess.
    let content = guard_core::email::notification(&m.notification, None);
    a.store
        .enqueue(
            id.clone(),
            "email".into(),
            json!({"To":recipient,"Subject":content.subject,"TextBody":content.text,"HtmlBody":content.html,"device":d}),
        )
        .await?;
    Ok(Json(json!({"status":"queued","id":id})))
}
async fn status(State(a): State<App>, headers: HeaderMap, Json(body): Json<Value>) -> Api {
    let device_id = device(&a, &headers).await?;
    let ids: Option<Vec<String>> = body
        .get("ids")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?;
    if let Some(ref ids) = ids {
        ensure!(
            ids.len() <= 100 && ids.iter().all(|s| s.len() <= 200),
            "Status query limit exceeded"
        );
    }
    let rows=a.store.read(move|c|{if let Some(ids)=ids{let mut rows=vec![];for local in ids{let id=format!("{device_id}:{local}");let row=c.query_row("SELECT id,state,attempts,error FROM outbox WHERE id=?1",[&id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"attempts":r.get::<_,u32>(2)?,"error":r.get::<_,Option<String>>(3)?}))).optional()?.unwrap_or(json!({"id":id,"state":"unknown","error":"Relay record not found; delivery status is unknown"}));rows.push(row);}Ok(rows)}else{let mut s=c.prepare("SELECT id,state,attempts,error FROM outbox WHERE id LIKE ?1 ORDER BY created DESC LIMIT 100")?;Ok(s.query_map([format!("{device_id}:%")],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"attempts":r.get::<_,u32>(2)?,"error":r.get::<_,Option<String>>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)}}).await?;
    Ok(Json(json!({"items":rows,"test_mode":a.test})))
}
async fn worker(a: App) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut round = 0u32;
    loop {
        tick.tick().await;
        if round.is_multiple_of(150)
            && let Err(e) = a.store.retain(guard_core::now()).await
        {
            eprintln!("Relay retention: {e}");
        }
        round = round.wrapping_add(1);
        if let Err(e) = a.store.enforce_quota(1024 * 1024 * 1024).await {
            eprintln!("Relay storage quota: {e}");
            continue;
        }
        if let Err(e) = dispatch(&a).await {
            eprintln!("Unable to process the relay queue: {e}");
        }
    }
}
async fn dispatch(a: &App) -> Result<()> {
    let rows:Vec<(String,String,u32)>=a.store.read(|c|{let mut s=c.prepare("SELECT id,payload,attempts FROM outbox WHERE state='pending' AND next_at<=?1 LIMIT 5")?;Ok(s.query_map([guard_core::now()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?)}).await?;
    for (id, payload, attempts) in rows {
        let mut v: Value = serde_json::from_str(&payload)?;
        let device_id = v["device"].as_str().unwrap_or("").to_string();
        let enabled = a
            .store
            .read(move |c| {
                Ok(c.query_row(
                    "SELECT revoked=0 FROM devices WHERE id=?1",
                    [device_id],
                    |r| r.get::<_, bool>(0),
                )
                .optional()?
                .unwrap_or(false))
            })
            .await?;
        if !enabled {
            a.store.write(move|c|{c.execute("UPDATE outbox SET state='failed',error='Device authorization revoked' WHERE id=?1",[id])?;Ok(())}).await?;
            continue;
        }
        v.as_object_mut().unwrap().remove("device");
        let result: Result<()> = if a.test {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let path = a.dir.join("test-mailbox.jsonl");
            if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 8 * 1024 * 1024 {
                std::fs::rename(&path, a.dir.join("test-mailbox.previous.jsonl"))?;
            }
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .open(path)?;
            writeln!(f, "{}", json!({"id":id,"message":v}))?;
            Ok(())
        } else {
            a.provider.send(v).await
        };
        let (state, error) = match result {
            Ok(()) => (
                if a.test { "test_delivered" } else { "accepted" }.to_string(),
                None,
            ),
            Err(e) => (
                if attempts >= 5 { "failed" } else { "pending" }.into(),
                Some(e.to_string()),
            ),
        };
        a.store.write(move|c|{c.execute("UPDATE outbox SET state=?1,attempts=attempts+1,next_at=?2,error=?3 WHERE id=?4",params![state,guard_core::now()+30.*2f64.powi(attempts as i32),error,id])?;Ok(())}).await?;
    }
    Ok(())
}
