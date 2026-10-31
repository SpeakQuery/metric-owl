//! Operator-configured email transports. Credentials never enter API payloads or errors.
use anyhow::{Result, bail};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
};
use serde_json::{Value, json};
use std::{io::Read, path::Path, time::Duration};

#[derive(Clone)]
pub enum Provider {
    LocalTest,
    Postmark {
        token: String,
        sender: String,
    },
    Smtp {
        transport: AsyncSmtpTransport<Tokio1Executor>,
        sender: Mailbox,
    },
}

impl Provider {
    pub fn from_env(local_test: bool) -> Result<Self> {
        if local_test {
            return Ok(Self::LocalTest);
        }
        match std::env::var("RELAY_PROVIDER")
            .as_deref()
            .unwrap_or("postmark")
        {
            "postmark" => {
                let token = env_required("POSTMARK_SERVER_TOKEN")?;
                let sender = env_required("POSTMARK_FROM")?;
                sender
                    .parse::<Mailbox>()
                    .map_err(|_| anyhow::anyhow!("Invalid POSTMARK_FROM address"))?;
                Ok(Self::Postmark { token, sender })
            }
            "smtp" => {
                let host = env_required("SMTP_HOST")?;
                let username = env_required("SMTP_USERNAME")?;
                let sender = std::env::var("SMTP_FROM").unwrap_or_else(|_| username.clone());
                let security = std::env::var("SMTP_SECURITY").unwrap_or_else(|_| "tls".into());
                let port = std::env::var("SMTP_PORT")
                    .unwrap_or_else(|_| if security == "starttls" { "587" } else { "465" }.into())
                    .parse::<u16>()
                    .map_err(|_| anyhow::anyhow!("SMTP_PORT must be 465 or 587"))?;
                validate_smtp(&host, &username, &sender, &security, port)?;
                let password = read_password(Path::new(&env_required("SMTP_PASSWORD_FILE")?))?;
                let transport = match security.as_str() {
                    "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&host),
                    "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host),
                    _ => unreachable!("security validated"),
                }
                .map_err(|_| anyhow::anyhow!("Unable to configure SMTP TLS"))?
                .port(port)
                .timeout(Some(Duration::from_secs(15)))
                .credentials(Credentials::new(username, password))
                .build();
                Ok(Self::Smtp {
                    transport,
                    sender: sender
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid SMTP_FROM address"))?,
                })
            }
            _ => bail!("RELAY_PROVIDER must be postmark or smtp"),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::LocalTest => "local-test",
            Self::Postmark { .. } => "postmark",
            Self::Smtp { .. } => "smtp",
        }
    }

    /// Performs TLS, authentication and a NOOP. Does not submit a message.
    pub async fn check_smtp(&self) -> Result<()> {
        let Self::Smtp { transport, .. } = self else {
            bail!("smtp-check requires RELAY_PROVIDER=smtp")
        };
        match tokio::time::timeout(Duration::from_secs(25), transport.test_connection()).await {
            Ok(Ok(true)) => Ok(()),
            _ => bail!("SMTP TLS/authentication check failed; no message was sent"),
        }
    }

    pub async fn send(&self, mut payload: Value) -> Result<()> {
        match self {
            Self::LocalTest => bail!("Local test messages must use the test mailbox"),
            Self::Postmark { token, sender } => {
                postmark_fields(&mut payload, sender);
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(15))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?;
                match client
                    .post("https://api.postmarkapp.com/email")
                    .header("X-Postmark-Server-Token", token)
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => Ok(()),
                    Ok(r) => bail!("Postmark HTTP {}", r.status()),
                    Err(_) => bail!("Postmark connection failed; delivery status is unknown"),
                }
            }
            Self::Smtp { transport, sender } => {
                let message = build_message(sender, &payload)?;
                match tokio::time::timeout(Duration::from_secs(30), transport.send(message)).await {
                    Ok(Ok(_)) => Ok(()),
                    _ => bail!("SMTP submission failed; delivery status may be unknown"),
                }
            }
        }
    }
}

fn postmark_fields(payload: &mut Value, sender: &str) {
    payload["From"] = json!(sender);
    payload["MessageStream"] = json!("outbound");
    // Do not inherit provider defaults that could insert tracking pixels/links.
    payload["TrackOpens"] = json!(false);
    payload["TrackLinks"] = json!("None");
}

fn env_required(key: &str) -> Result<String> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{key} is required"))
}

fn validate_smtp(
    host: &str,
    username: &str,
    sender: &str,
    security: &str,
    port: u16,
) -> Result<()> {
    anyhow::ensure!(
        host.len() <= 253
            && !host.is_empty()
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }),
        "SMTP_HOST must be a DNS hostname without a URL or port"
    );
    anyhow::ensure!(
        matches!((security, port), ("tls", 465) | ("starttls", 587)),
        "Only verified TLS on port 465 or required STARTTLS on port 587 is supported"
    );
    anyhow::ensure!(
        username.len() <= 254 && !username.is_empty() && !username.chars().any(char::is_control),
        "Invalid SMTP_USERNAME"
    );
    anyhow::ensure!(
        sender.len() <= 320
            && !sender.chars().any(char::is_control)
            && sender.parse::<Mailbox>().is_ok(),
        "Invalid SMTP_FROM address"
    );
    Ok(())
}

fn read_password(path: &Path) -> Result<String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    anyhow::ensure!(
        path.is_absolute(),
        "SMTP_PASSWORD_FILE must be an absolute private file path"
    );
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| anyhow::anyhow!("Unable to open SMTP_PASSWORD_FILE"))?;
    let metadata = file.metadata()?;
    // systemd may expose credentials as root:root 0440 with a service-specific ACL,
    // inside its private mount. Do not relax ordinary file permissions to match it.
    let systemd_credential = std::env::var_os("CREDENTIALS_DIRECTORY").is_some_and(|dir| {
        let dir = Path::new(&dir);
        dir.starts_with("/run/credentials") && path.parent() == Some(dir)
    });
    anyhow::ensure!(
        metadata.is_file()
            && private_permissions(
                metadata.mode(),
                metadata.uid(),
                metadata.gid(),
                systemd_credential
            ),
        "SMTP_PASSWORD_FILE must be private or a read-only systemd credential"
    );
    anyhow::ensure!(
        metadata.len() <= 4096,
        "SMTP password file exceeds the size limit"
    );
    let mut bytes = Vec::new();
    (&mut file).take(4097).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 4096,
        "SMTP password file exceeds the size limit"
    );
    let password = String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid SMTP password file encoding"))?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();
    anyhow::ensure!(
        !password.is_empty() && !password.chars().any(char::is_control),
        "SMTP password is empty or contains control characters"
    );
    Ok(password)
}

fn private_permissions(mode: u32, uid: u32, gid: u32, systemd_credential: bool) -> bool {
    mode & 0o077 == 0 || (systemd_credential && uid == 0 && gid == 0 && mode & 0o777 == 0o440)
}

fn build_message(sender: &Mailbox, payload: &Value) -> Result<Message> {
    let recipient = payload["To"].as_str().unwrap_or_default();
    anyhow::ensure!(super::email(recipient), "Invalid email recipient");
    let recipient: Mailbox = recipient
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid email recipient"))?;
    let subject = payload["Subject"].as_str().unwrap_or_default();
    anyhow::ensure!(
        subject.len() <= 1024 && !subject.chars().any(char::is_control),
        "Invalid email subject"
    );
    let text = payload["TextBody"].as_str().unwrap_or_default();
    anyhow::ensure!(text.len() <= 16 * 1024, "Email body exceeds the size limit");
    let builder = Message::builder()
        .from(sender.clone())
        .to(recipient)
        .subject(subject);
    let result = if let Some(html) = payload["HtmlBody"].as_str() {
        anyhow::ensure!(
            !html.is_empty() && html.len() <= 64 * 1024,
            "Email HTML exceeds the size limit"
        );
        builder.multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(text.to_owned()))
                .singlepart(SinglePart::html(html.to_owned())),
        )
    } else {
        builder
            .header(lettre::message::header::ContentType::TEXT_PLAIN)
            .body(text.to_owned())
    };
    result.map_err(|_| anyhow::anyhow!("Unable to build email message"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postmark_submission_disables_open_and_link_tracking() {
        let mut payload = json!({"TextBody":"Plain text","HtmlBody":"<p>HTML</p>","TrackOpens":true,"TrackLinks":"HtmlAndText","From":"untrusted@example.com"});
        postmark_fields(&mut payload, "configured@example.com");
        assert_eq!(payload["TrackOpens"], false);
        assert_eq!(payload["TrackLinks"], "None");
        assert_eq!(payload["From"], "configured@example.com");
        assert_eq!(payload["TextBody"], "Plain text");
        assert_eq!(payload["HtmlBody"], "<p>HTML</p>");
    }
    #[test]
    fn html_relay_mail_is_multipart_and_legacy_plain_mail_still_works() {
        let sender: Mailbox = "sender@example.com".parse().unwrap();
        let content = guard_core::email::notification(
            &json!({"kind":"recovery","resolved":true,"severity":"critical","title":"CPU load","summary":"The recovery condition was met."}),
            Some("https://panel.example.com"),
        );
        let payload = json!({"To":"alerts@example.com","Subject":content.subject,"TextBody":content.text,"HtmlBody":content.html});
        let wire =
            String::from_utf8(build_message(&sender, &payload).unwrap().formatted()).unwrap();
        assert!(wire.contains("multipart/alternative"));
        assert!(wire.contains("text/plain"));
        assert!(wire.contains("text/html"));
        let mut legacy = payload.clone();
        legacy.as_object_mut().unwrap().remove("HtmlBody");
        let wire = String::from_utf8(build_message(&sender, &legacy).unwrap().formatted()).unwrap();
        assert!(wire.contains("text/plain"));
        assert!(!wire.contains("multipart/alternative"));
        let mut oversized = payload;
        oversized["HtmlBody"] = json!("x".repeat(64 * 1024 + 1));
        assert!(build_message(&sender, &oversized).is_err());
    }
    #[test]
    fn smtp_security_is_mandatory_and_paired_with_submission_port() {
        for (security, port, valid) in [
            ("tls", 465, true),
            ("starttls", 587, true),
            ("none", 25, false),
            ("tls", 25, false),
            ("starttls", 465, false),
            ("opportunistic", 587, false),
        ] {
            assert_eq!(
                validate_smtp(
                    "smtp.example.com",
                    "sender@example.com",
                    "sender@example.com",
                    security,
                    port
                )
                .is_ok(),
                valid
            );
        }
        assert!(
            validate_smtp(
                "https://smtp.example.com",
                "sender@example.com",
                "sender@example.com",
                "tls",
                465
            )
            .is_err()
        );
        assert!(
            validate_smtp(
                "smtp.example.com",
                "sender@example.com",
                "sender@example.com\r\nBcc:other@example.com",
                "tls",
                465
            )
            .is_err()
        );
    }
    #[test]
    fn password_file_rejects_public_permissions_symlinks_and_oversized_content() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = std::env::temp_dir().join(format!("metricowl-smtp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("secret");
        std::fs::write(&path, "fixture-only\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_password(&path).unwrap(), "fixture-only");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_password(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        symlink(&path, dir.join("link")).unwrap();
        assert!(read_password(&dir.join("link")).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, "x".repeat(4097)).unwrap();
        assert!(read_password(&path).is_err());
        assert!(read_password(Path::new("relative-file")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn systemd_read_only_credentials_are_narrowly_accepted() {
        assert!(private_permissions(0o600, 1000, 1000, false));
        assert!(private_permissions(0o400, 1000, 1000, false));
        assert!(private_permissions(0o440, 0, 0, true));
        for (mode, uid, gid, credential) in [
            (0o440, 0, 0, false),
            (0o640, 0, 0, true),
            (0o444, 0, 0, true),
            (0o440, 1000, 0, true),
            (0o440, 0, 1000, true),
            (0o644, 1000, 1000, false),
        ] {
            assert!(!private_permissions(mode, uid, gid, credential));
        }
    }
    #[test]
    fn smtp_message_cannot_inject_headers_or_extra_recipients() {
        let sender = "Metric Owl <services@example.com>".parse().unwrap();
        let payload = json!({"To":"owner@example.com","Subject":"[Metric Owl] Memory pressure","TextBody":"Example notification."});
        let rendered =
            String::from_utf8(build_message(&sender, &payload).unwrap().formatted()).unwrap();
        assert!(
            rendered
                .lines()
                .any(|line| line.starts_with("From:") && line.contains("<services@example.com>"))
        );
        assert!(rendered.contains("To: owner@example.com"));
        assert!(build_message(&sender,&json!({"To":"owner@example.com,other@example.com","Subject":"Test","TextBody":"Test"})).is_err());
        assert!(build_message(&sender,&json!({"To":"owner@example.com","Subject":"Test\r\nBcc:other@example.com","TextBody":"Test"})).is_err());
    }
}
