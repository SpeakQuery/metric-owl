use crate::App;
use anyhow::{Result, ensure};
use guard_core::{
    model::{Rule, Target, TargetStatus},
    rules::Engine,
};
use std::os::unix::fs::MetadataExt;
use std::{collections::HashMap, path::Path, time::Duration};
use tokio::io::AsyncReadExt;
pub fn validate(t: &Target) -> Result<()> {
    if let Some(options) = &t.options {
        crate::probes::validate(options)?;
    }
    ensure!(
        !t.id.is_empty() && t.id.len() < 80 && t.name.len() < 160 && t.address.len() < 2048,
        "Invalid target"
    );
    match t.kind.as_str() {
        "folder" | "log" | "job" => {
            ensure!(
                Path::new(&t.address).is_absolute(),
                "An absolute file path is required"
            );
            ensure!(!t.address.contains('\0'), "Invalid path");
        }
        "http" | "tls" => {
            let u = url::Url::parse(&t.address)?;
            ensure!(
                ["http", "https"].contains(&u.scheme())
                    && u.username().is_empty()
                    && u.password().is_none(),
                "Invalid HTTP URL"
            );
            ensure!(
                t.kind != "tls" || u.scheme() == "https",
                "Certificate monitoring requires an HTTPS URL"
            );
            ensure!(u.fragment().is_none(), "URL fragments are not supported");
        }
        "service" | "journal" => ensure!(
            t.address.len() < 200
                && t.address
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.@-".contains(c))
                && !t.address.starts_with('-'),
            "Invalid service name"
        ),
        "docker" => ensure!(
            Path::new(&t.address).is_absolute(),
            "Helper socket path is required"
        ),
        _ => anyhow::bail!("Unsupported source"),
    };
    Ok(())
}
#[derive(Default)]
struct Cursor {
    inode: u64,
    offset: u64,
}
pub fn default_rule(t: &Target) -> Rule {
    Rule {
        id: format!("target-{}", t.id),
        name: format!("{} is not responding", t.name),
        source: t.id.clone(),
        metric: t.kind.clone(),
        operator: ">=".into(),
        threshold: 1.,
        window_seconds: if t.kind == "http" {
            90
        } else if t.kind == "job" {
            60
        } else {
            10
        },
        fraction: 1.,
        severity: "critical".into(),
        recovery_threshold: 0.,
        recovery_seconds: if ["http", "job"].contains(&t.kind.as_str()) {
            60
        } else {
            10
        },
        enabled: true,
        notify: true,
    }
}
pub fn default_rules(t: &Target) -> Vec<Rule> {
    if t.kind != "tls" {
        return vec![default_rule(t)];
    }
    let mut rules: Vec<_> = [30., 14., 7.]
        .into_iter()
        .map(|days| Rule {
            id: format!("tls-{}-{days}", t.id),
            name: format!("{} certificate expires within {days} days", t.name),
            source: t.id.clone(),
            metric: "tls_days".into(),
            operator: "<=".into(),
            threshold: days,
            window_seconds: 600,
            fraction: 1.,
            severity: if days <= 7. { "critical" } else { "warning" }.into(),
            recovery_threshold: days + 1.,
            recovery_seconds: 600,
            enabled: true,
            notify: true,
        })
        .collect();
    let mut failure = default_rule(t);
    failure.id = format!("tls-failure-{}", t.id);
    failure.metric = "tls_failure".into();
    failure.name = format!("{} TLS verification failed", t.name);
    failure.window_seconds = 600;
    failure.recovery_seconds = 600;
    rules.push(failure);
    rules
}
pub async fn run(app: App) {
    let mut workers: HashMap<String, (String, tokio::task::JoinHandle<()>)> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tick.tick().await;
        let targets = app.targets.read().await.clone();
        workers.retain(|id, (serialized, task)| {
            let keep = targets.iter().any(|t| {
                t.enabled
                    && t.id == *id
                    && serde_json::to_string(t).ok().as_ref() == Some(serialized)
            });
            if !keep {
                task.abort();
            }
            keep
        });
        app.target_status
            .write()
            .await
            .retain(|s| targets.iter().any(|t| t.enabled && t.id == s.id));
        for t in targets.into_iter().filter(|t| t.enabled) {
            if !workers.contains_key(&t.id) {
                let a = app.clone();
                let serialized = serde_json::to_string(&t).unwrap_or_default();
                let id = t.id.clone();
                workers.insert(
                    id,
                    (serialized, tokio::spawn(async move { monitor(a, t).await })),
                );
            }
        }
    }
}
async fn monitor(app: App, t: Target) {
    let cadence = match t.kind.as_str() {
        "folder" => 900,
        "http" | "job" => 30,
        "tls" => 300,
        _ => 5,
    };
    let mut tick = tokio::time::interval(Duration::from_secs(cadence));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cursor = Cursor::default();
    let mut engine = Engine::default();
    if let Ok(events) = app.store.active_incidents().await {
        engine.restore(events);
    }
    let start = std::time::Instant::now();
    let mut pending = std::collections::VecDeque::new();
    loop {
        tick.tick().await;
        let result = check(&t, &mut cursor).await;
        let probe = match result {
            Ok(x) => x,
            Err(e) => crate::probes::Probe::basic((
                "unavailable".into(),
                format!("Measurement unavailable: {e}"),
                None,
            )),
        };
        let row = TargetStatus {
            id: t.id.clone(),
            kind: t.kind.clone(),
            name: t.name.clone(),
            at: guard_core::now(),
            status: probe.status,
            detail: guard_core::redact::redact(&probe.detail, 8192),
            value: probe.value,
            measurements: probe.measurements,
        };
        {
            let mut rows = app.target_status.write().await;
            rows.retain(|s| s.id != row.id);
            rows.push(row.clone());
        }
        if ["http", "service", "tls", "job"].contains(&t.kind.as_str()) {
            let rules = app.rules.read().await.clone();
            let metric = if t.kind == "tls" {
                "tls_days"
            } else {
                t.kind.as_str()
            };
            pending.extend(engine.evaluate(
                &rules,
                &t.id,
                metric,
                start.elapsed().as_secs_f64(),
                guard_core::now(),
                row.value,
                cadence as f64,
            ));
            if t.kind == "tls" {
                pending.extend(engine.evaluate(
                    &rules,
                    &t.id,
                    "tls_failure",
                    start.elapsed().as_secs_f64(),
                    guard_core::now(),
                    row.measurements.get("tls_failure").copied(),
                    cadence as f64,
                ));
            }
        }
        while let Some(event) = pending.front() {
            if let Err(e) = crate::integrations::incident(&app, event).await {
                app.error(format!("Unable to persist target incident: {e}"))
                    .await;
                break;
            }
            pending.pop_front();
        }
        if pending.len() > 100 {
            pending.pop_front();
            app.error("Target incident buffer is full; the oldest record was lost".into())
                .await;
        }
        if let Err(e) = app
            .store
            .put(
                &format!("target_summary:{}", t.id),
                serde_json::to_string(&row).unwrap_or_default(),
            )
            .await
        {
            app.error(format!("Source summary: {e}")).await;
        }
    }
}
async fn check(t: &Target, cursor: &mut Cursor) -> Result<crate::probes::Probe> {
    match t.kind.as_str() {
        "http" => return crate::probes::http(t).await,
        "tls" => return crate::probes::tls(t).await,
        "job" => return crate::probes::job(t).await,
        _ => (),
    }
    let value: Result<(String, String, Option<f64>)> = match t.kind.as_str() {
        "folder" => {
            let path = t.address.clone();
            tokio::task::spawn_blocking(move || scan_folder(&path)).await?
        }
        "log" => {
            let metadata = tokio::fs::symlink_metadata(&t.address).await?;
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Only regular files can be read"
            );
            if metadata.ino() != cursor.inode || metadata.len() < cursor.offset {
                cursor.inode = metadata.ino();
                cursor.offset = metadata.len().saturating_sub(65536);
            }
            let mut f = tokio::fs::File::open(&t.address).await?;
            use tokio::io::AsyncSeekExt;
            f.seek(std::io::SeekFrom::Start(cursor.offset)).await?;
            let mut buf = vec![];
            f.take(65536).read_to_end(&mut buf).await?;
            cursor.offset += buf.len() as u64;
            let text = String::from_utf8_lossy(&buf);
            let lines: Vec<_> = text.lines().rev().take(40).collect();
            Ok((
                "ok".into(),
                lines.into_iter().rev().collect::<Vec<_>>().join("\n"),
                Some(metadata.len() as f64),
            ))
        }
        "service" | "journal" => {
            ensure!(
                cfg!(target_os = "linux"),
                "Linux systemd sources are unavailable on this platform"
            );
            let mut cmd = if t.kind == "service" {
                let mut c = tokio::process::Command::new("systemctl");
                c.args([
                    "show",
                    "--property=ActiveState",
                    "--value",
                    "--",
                    &t.address,
                ]);
                c
            } else {
                let mut c = tokio::process::Command::new("journalctl");
                c.args([
                    "--no-pager",
                    "-n",
                    "40",
                    "--output=short-iso",
                    "--unit",
                    &t.address,
                ]);
                c
            };
            cmd.kill_on_drop(true);
            let out = tokio::time::timeout(Duration::from_secs(4), cmd.output()).await??;
            ensure!(out.status.success(), "Service log unavailable");
            let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if t.kind == "service" {
                let ok = text == "active";
                Ok((
                    if ok { "ok" } else { "error" }.into(),
                    text,
                    Some(if ok { 0. } else { 1. }),
                ))
            } else {
                Ok(("ok".into(), text, None))
            }
        }
        "docker" => {
            use tokio::io::AsyncWriteExt;
            let mut stream = tokio::net::UnixStream::connect(&t.address).await?;
            stream.write_all(b"containers\n").await?;
            let mut data = vec![];
            tokio::time::timeout(
                Duration::from_secs(5),
                stream.take(65536).read_to_end(&mut data),
            )
            .await??;
            let text = String::from_utf8(data)?;
            Ok(("ok".into(), text, None))
        }
        _ => anyhow::bail!("Unknown source"),
    };
    value.map(crate::probes::Probe::basic)
}
fn scan_folder(path: &str) -> Result<(String, String, Option<f64>)> {
    let start = std::time::Instant::now();
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "A regular directory is required"
    );
    let mut stack = vec![std::path::PathBuf::from(path)];
    let mut count = 0;
    let mut bytes = 0u64;
    let mut incomplete = false;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(dir) {
            Ok(x) => x,
            Err(_) => {
                incomplete = true;
                continue;
            }
        };
        for entry in entries {
            count += 1;
            if count > 20000 || start.elapsed() > Duration::from_secs(2) {
                incomplete = true;
                stack.clear();
                break;
            }
            let entry = match entry {
                Ok(x) => x,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            let m = match std::fs::symlink_metadata(entry.path()) {
                Ok(x) => x,
                Err(_) => {
                    incomplete = true;
                    continue;
                }
            };
            if m.file_type().is_symlink() {
                continue;
            }
            if m.is_dir() {
                stack.push(entry.path());
            } else if m.is_file() {
                bytes = bytes.saturating_add(m.len());
            }
        }
    }
    Ok((
        if incomplete { "partial" } else { "ok" }.into(),
        format!(
            "{count} entries; {bytes} bytes{}. Symbolic links were not followed.",
            if incomplete {
                "; incomplete due to time, file count, or permission limits"
            } else {
                ""
            }
        ),
        if incomplete { None } else { Some(bytes as f64) },
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_does_not_follow_symlinks() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("file"), b"abc").unwrap();
        std::os::unix::fs::symlink("/", d.path().join("root")).unwrap();
        let r = scan_folder(d.path().to_str().unwrap()).unwrap();
        assert_eq!(r.2, Some(3.));
    }
    #[tokio::test]
    async fn rotation_and_permission_failure() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("log");
        std::fs::write(&p, "first\n").unwrap();
        let t = Target {
            id: "x".into(),
            kind: "log".into(),
            name: "x".into(),
            address: p.display().to_string(),
            enabled: true,
            options: None,
        };
        let mut c = Cursor::default();
        assert!(check(&t, &mut c).await.unwrap().detail.contains("first"));
        std::fs::rename(&p, d.path().join("old")).unwrap();
        std::fs::write(&p, "second\n").unwrap();
        assert!(check(&t, &mut c).await.unwrap().detail.contains("second"));
        std::fs::remove_file(&p).unwrap();
        assert!(check(&t, &mut c).await.is_err());
    }
}
