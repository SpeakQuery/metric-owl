//! Direct, owner-configured SMTP submission. No central account or local mail server.
use crate::{App, Settings};
use anyhow::{Result, bail, ensure};
use guard_core::store::Store;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::Mailbox,
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{net::IpAddr, time::Duration};

pub fn enabled(s: &Settings) -> bool {
    match s.email_transport.as_str() {
        "smtp" => !s.smtp_host.is_empty(),
        "relay" => !s.relay_url.is_empty(),
        _ => false,
    }
}

pub fn address(value: &str) -> Result<Mailbox> {
    ensure!(
        !value.is_empty() && value.len() <= 254 && !value.chars().any(char::is_control),
        "Enter one valid email address"
    );
    // Display names and lists are deliberately excluded from the recipient control.
    let parsed = value
        .parse::<lettre::Address>()
        .map_err(|_| anyhow::anyhow!("Enter one valid email address"))?;
    Ok(Mailbox::new(None, parsed))
}

pub fn validate(s: &Settings) -> Result<()> {
    ensure!(
        matches!(s.email_transport.as_str(), "disabled" | "smtp" | "relay"),
        "Invalid email transport"
    );
    if !s.recipient.is_empty() {
        address(&s.recipient)?;
    }
    if s.email_transport != "smtp" && s.smtp_host.is_empty() {
        return Ok(());
    }
    let host = &s.smtp_host;
    ensure!(
        !host.is_empty()
            && host.len() <= 253
            && host.contains('.')
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }),
        "SMTP host must be a DNS hostname without a URL, path or port"
    );
    ensure!(
        matches!(
            (s.smtp_security.as_str(), s.smtp_port),
            ("tls", 465) | ("starttls", 587)
        ),
        "SMTP requires verified TLS on port 465 or required STARTTLS on port 587"
    );
    ensure!(
        !s.smtp_username.is_empty()
            && s.smtp_username.len() <= 254
            && !s.smtp_username.chars().any(char::is_control),
        "Invalid SMTP username"
    );
    address(&s.smtp_from)?;
    Ok(())
}

pub fn validate_password(password: &str) -> Result<()> {
    ensure!(
        !password.is_empty() && password.len() <= 4096 && !password.chars().any(char::is_control),
        "Invalid SMTP password"
    );
    Ok(())
}

/// Binds verification to the complete mail identity, not just its destination.
pub fn identity(s: &Settings) -> String {
    crate::auth::hash(
        &json!([
            s.email_transport,
            s.smtp_host,
            s.smtp_port,
            s.smtp_security,
            s.smtp_username,
            s.smtp_from,
            s.recipient,
            s.relay_url
        ])
        .to_string(),
    )
}

struct Client {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    sender: Mailbox,
}

fn transport(
    s: &Settings,
    password: String,
    ip: IpAddr,
    tls: TlsParameters,
) -> AsyncSmtpTransport<Tokio1Executor> {
    // The connection uses the validated address literally, while SNI and certificate
    // verification retain the original hostname. No second DNS lookup can rebind it.
    AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(ip.to_string())
        .port(s.smtp_port)
        .tls(if s.smtp_security == "tls" {
            Tls::Wrapper(tls)
        } else {
            Tls::Required(tls)
        })
        .credentials(Credentials::new(s.smtp_username.clone(), password))
        .timeout(Some(Duration::from_secs(10)))
        .build()
}

async fn client(app: &App, s: &Settings) -> Result<Client> {
    validate(s)?;
    ensure!(s.email_transport == "smtp", "Select direct SMTP first");
    let password = crate::integrations::secret(app, "smtp").await?;
    validate_password(&password)?;
    let addresses: Vec<_> = tokio::time::timeout(
        Duration::from_secs(4),
        tokio::net::lookup_host((s.smtp_host.as_str(), s.smtp_port)),
    )
    .await
    .map_err(|_| anyhow::anyhow!("SMTP DNS lookup timed out"))?
    .map_err(|_| anyhow::anyhow!("SMTP hostname could not be resolved"))?
    .take(33)
    .collect();
    ensure!(
        !addresses.is_empty()
            && addresses.len() <= 32
            && addresses.iter().all(|a| public_destination(a.ip())),
        "SMTP must resolve only to public network addresses"
    );
    let tls = TlsParameters::new(s.smtp_host.clone())
        .map_err(|_| anyhow::anyhow!("Unable to configure SMTP TLS"))?;
    Ok(Client {
        transport: transport(s, password, addresses[0].ip(), tls),
        sender: address(&s.smtp_from)?,
    })
}

fn public_destination(ip: IpAddr) -> bool {
    crate::integrations::public_ip(ip)
        && match ip {
            IpAddr::V4(v) => !v.is_documentation(),
            IpAddr::V6(v) => {
                if let Some(v4) = v.to_ipv4_mapped() {
                    public_destination(IpAddr::V4(v4))
                } else {
                    v.segments()[..2] != [0x2001, 0xdb8]
                }
            }
        }
}

/// TLS + AUTH + NOOP only; it cannot choose a recipient or submit a message.
pub async fn check(app: &App, s: &Settings) -> Result<()> {
    cooldown(&app.store, "smtp_test", 30.).await?;
    let client = client(app, s).await?;
    match tokio::time::timeout(Duration::from_secs(20), client.transport.test_connection()).await {
        Ok(Ok(true)) => Ok(()),
        _ => bail!("SMTP TLS/authentication check failed; no email was sent"),
    }
}

fn message(sender: Mailbox, recipient: &str, subject: &str, body: &str) -> Result<Message> {
    ensure!(
        subject.len() <= 512 && !subject.chars().any(char::is_control),
        "Invalid email subject"
    );
    ensure!(
        body.len() <= 16 * 1024,
        "Email summary exceeds the size limit"
    );
    Message::builder()
        .from(sender)
        .to(address(recipient)?)
        .subject(subject)
        .header(lettre::message::header::ContentType::TEXT_PLAIN)
        .body(body.to_owned())
        .map_err(|_| anyhow::anyhow!("Unable to build email message"))
}

async fn send(app: &App, s: &Settings, subject: &str, body: &str) -> Result<()> {
    let client = client(app, s).await?;
    let message = message(client.sender, &s.recipient, subject, body)?;
    match tokio::time::timeout(Duration::from_secs(25), client.transport.send(message)).await {
        Ok(Ok(_)) => Ok(()),
        _ => bail!("SMTP submission failed; delivery status may be unknown"),
    }
}

pub async fn notification(app: &App, s: &Settings, payload: &Value) -> Result<()> {
    ensure!(s.recipient_verified, "Verify your email address first");
    let title = guard_core::redact::redact(
        payload["title"].as_str().unwrap_or("Server notification"),
        256,
    )
    .replace(['\r', '\n'], " ");
    let text = format!(
        "Server: {}\n\n{}\n\n{}\n\nMetric Owl · Your self-hosted server monitor",
        guard_core::redact::redact(&s.alias, 100),
        guard_core::redact::redact(payload["summary"].as_str().unwrap_or(""), 4096),
        guard_core::redact::redact(payload["recommendation"].as_str().unwrap_or(""), 2048)
    );
    send(app, s, &format!("[Metric Owl] {title}"), &text).await
}

#[derive(Serialize, Deserialize)]
struct Verification {
    identity: String,
    hash: String,
    expires: f64,
    attempts: u8,
}

pub async fn start_verification(app: &App, s: &Settings) -> Result<Value> {
    validate(s)?;
    address(&s.recipient)?;
    cooldown(&app.store, "smtp_verification", 60.).await?;
    let id = format!("verify:{}", uuid::Uuid::new_v4());
    ensure!(
        app.store.reserve_budget("email", &id, 60).await?,
        "Daily email quota reached"
    );
    use rand::Rng;
    let code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32));
    put_verification(&app.store, s, &code).await?;
    let sent = send(app,s,"Your Metric Owl verification code",&format!("Your verification code: {code}\n\nValid for 10 minutes. Enter this code in your own Metric Owl control center.")).await;
    let (state, error) = if sent.is_ok() {
        ("provider_accepted", None)
    } else {
        (
            "failed",
            Some("SMTP verification submission failed; delivery status may be unknown"),
        )
    };
    // Never put the code or credentials in outbox, audit, error text or API responses.
    app.store.write(move|c|{c.execute("INSERT INTO outbox(id,kind,payload,state,attempts,next_at,created,error) VALUES(?1,'email','{}',?2,1,?3,?3,?4)",params![id,state,guard_core::now(),error])?;Ok(())}).await?;
    sent?;
    Ok(
        json!({"ok":true,"status":"provider_accepted","test_mode":false,"message":"Verification email accepted by your SMTP provider. Check your inbox for the code."}),
    )
}

async fn put_verification(store: &Store, s: &Settings, code: &str) -> Result<()> {
    let identity = identity(s);
    store
        .put(
            "smtp_verification",
            serde_json::to_string(&Verification {
                hash: crate::auth::hash(&format!("{identity}:{code}")),
                identity,
                expires: guard_core::now() + 600.,
                attempts: 0,
            })?,
        )
        .await
}

pub async fn verify(store: &Store, s: &Settings, code: &str) -> Result<()> {
    let identity = identity(s);
    let hash = crate::auth::hash(&format!("{identity}:{code}"));
    let shape = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
    let valid = store
        .write(move |c| {
            let tx = c.transaction()?;
            let raw: Option<String> = tx
                .query_row(
                    "SELECT value FROM kv WHERE key='smtp_verification'",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(raw) = raw else { return Ok(false) };
            let mut pending: Verification = serde_json::from_str(&raw)?;
            if pending.attempts >= 5
                || pending.expires < guard_core::now()
                || pending.identity != identity
            {
                return Ok(false);
            };
            let valid = shape && crate::auth::constant(&pending.hash, &hash);
            pending.attempts += 1;
            if valid {
                tx.execute("DELETE FROM kv WHERE key='smtp_verification'", [])?;
            } else {
                tx.execute(
                    "UPDATE kv SET value=?1 WHERE key='smtp_verification'",
                    [serde_json::to_string(&pending)?],
                )?;
            }
            tx.commit()?;
            Ok(valid)
        })
        .await?;
    ensure!(valid, "Invalid or expired verification code");
    Ok(())
}

async fn cooldown(store: &Store, name: &'static str, seconds: f64) -> Result<()> {
    let allowed = store.write(move|c|{
        let tx=c.transaction()?;
        let key=format!("cooldown:{name}");
        let last:Option<String>=tx.query_row("SELECT value FROM kv WHERE key=?1",[&key],|r|r.get(0)).optional()?;
        let now=guard_core::now();
        if last.and_then(|v|v.parse::<f64>().ok()).is_some_and(|last|now<last+seconds) {return Ok(false)};
        tx.execute("INSERT INTO kv(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,now.to_string()])?;
        tx.commit()?;Ok(true)
    }).await?;
    ensure!(allowed, "Please wait before trying this email action again");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            email_transport: "smtp".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_username: "owner@example.com".into(),
            smtp_from: "owner@example.com".into(),
            recipient: "alerts@example.com".into(),
            ..Settings::default()
        }
    }
    #[test]
    fn new_install_is_local_without_mail_and_legacy_relay_survives() {
        assert_eq!(Settings::default().email_transport, "disabled");
        assert!(!enabled(&Settings::default()));
        let old: Settings =
            serde_json::from_value(json!({"relay_url":"https://relay.example.com"})).unwrap();
        assert_eq!(old.email_transport, "relay");
        assert!(enabled(&old));
        let stored: Settings =
            serde_json::from_value(serde_json::to_value(Settings::default()).unwrap()).unwrap();
        assert_eq!(stored.email_transport, "disabled");
    }
    #[test]
    fn only_tls_submission_and_single_addresses_are_accepted() {
        let mut s = settings();
        assert!(validate(&s).is_ok());
        s.smtp_port = 587;
        s.smtp_security = "starttls".into();
        assert!(validate(&s).is_ok());
        for (security, port) in [
            ("tls", 587),
            ("starttls", 465),
            ("none", 25),
            ("starttls", 25),
            ("opportunistic", 587),
        ] {
            s.smtp_port = port;
            s.smtp_security = security.into();
            assert!(validate(&s).is_err());
        }
        for host in [
            "localhost",
            "https://smtp.example.com",
            "smtp.example.com:465",
            "smtp.example.com/path",
            "a\r\nb.example.com",
        ] {
            let mut s = settings();
            s.smtp_host = host.into();
            assert!(validate(&s).is_err());
        }
        for recipient in [
            "a@example.com,b@example.com",
            "User <a@example.com>",
            "a@example.com\r\nBcc:b@example.com",
            "",
        ] {
            assert!(address(recipient).is_err());
        }
        for password in ["", "value\r\nAUTH", "bad\0password"] {
            assert!(validate_password(password).is_err());
        }
        assert!(validate_password("ordinary-password").is_ok());
    }
    #[test]
    fn smtp_cannot_reach_private_metadata_or_documentation_networks() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.100.100.200",
            "192.168.0.1",
            "::1",
            "fc00::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "203.0.113.1",
        ] {
            assert!(!public_destination(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_destination(ip.parse().unwrap()));
        }
    }
    #[test]
    fn message_rejects_header_injection_and_unbounded_payloads() {
        let sender = address("owner@example.com").unwrap();
        let mail = message(sender.clone(), "alerts@example.com", "A test", "Summary").unwrap();
        assert_eq!(mail.envelope().to().len(), 1);
        assert!(
            message(
                sender.clone(),
                "alerts@example.com",
                "Test\r\nBcc:other@example.com",
                "text"
            )
            .is_err()
        );
        assert!(
            message(
                sender,
                "alerts@example.com",
                "Test",
                &"a".repeat(16 * 1024 + 1)
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn verification_is_single_use_expiring_identity_bound_and_attempt_limited() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        let s = settings();
        put_verification(&store, &s, "123456").await.unwrap();
        assert!(
            !store
                .get("smtp_verification")
                .await
                .unwrap()
                .unwrap()
                .contains("123456")
        );
        let mut changed = s.clone();
        changed.recipient = "other@example.com".into();
        assert!(verify(&store, &changed, "123456").await.is_err());
        assert!(verify(&store, &s, "bad").await.is_err());
        verify(&store, &s, "123456").await.unwrap();
        assert!(verify(&store, &s, "123456").await.is_err());
        put_verification(&store, &s, "654321").await.unwrap();
        for _ in 0..5 {
            assert!(verify(&store, &s, "000000").await.is_err());
        }
        assert!(verify(&store, &s, "654321").await.is_err());
        put_verification(&store, &s, "123456").await.unwrap();
        let mut row: Verification =
            serde_json::from_str(&store.get("smtp_verification").await.unwrap().unwrap()).unwrap();
        row.expires = 0.;
        store
            .put("smtp_verification", serde_json::to_string(&row).unwrap())
            .await
            .unwrap();
        assert!(verify(&store, &s, "123456").await.is_err());
    }
    #[tokio::test]
    async fn email_action_cooldown_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = Store::open(&path).unwrap();
        cooldown(&store, "smtp_test", 30.).await.unwrap();
        assert!(
            cooldown(&Store::open(&path).unwrap(), "smtp_test", 30.)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn required_starttls_never_sends_credentials_to_plaintext_server() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut s = settings();
        s.smtp_port = listener.local_addr().unwrap().port();
        s.smtp_security = "starttls".into();
        let peer = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket
                .write_all(b"220 mock.example ESMTP\r\n")
                .await
                .unwrap();
            let mut socket = BufReader::new(socket);
            let mut commands = String::new();
            socket.read_line(&mut commands).await.unwrap();
            assert!(commands.starts_with("EHLO "));
            // Advertise AUTH but deliberately omit STARTTLS. A secure client stops.
            socket
                .get_mut()
                .write_all(b"250-mock.example\r\n250 AUTH PLAIN LOGIN\r\n")
                .await
                .unwrap();
            let mut rest = String::new();
            let _ = tokio::time::timeout(Duration::from_secs(2), socket.read_line(&mut rest)).await;
            assert!(!rest.contains("AUTH") && !rest.contains("MAIL FROM"));
        });
        let tls = TlsParameters::new(s.smtp_host.clone()).unwrap();
        let transport = transport(
            &s,
            "test-password-never-sent".into(),
            "127.0.0.1".parse().unwrap(),
            tls,
        );
        assert!(transport.test_connection().await.is_err());
        peer.await.unwrap();
    }
}
