//! Separate process with Docker permission. The web process never opens docker.sock.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub async fn run(path: PathBuf) -> Result<()> {
    ensure!(
        !path.exists(),
        "Helper socket already exists; check the running service"
    );
    let listener = tokio::net::UnixListener::bind(&path)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;
    let snapshot = std::sync::Arc::new(tokio::sync::RwLock::new(
        json!({"status":"warming"}).to_string(),
    ));
    let cache = snapshot.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            tick.tick().await;
            let result = collect().await;
            *cache.write().await = match result {
                Ok(v) => v.to_string(),
                Err(_) => json!({"status":"unavailable","reason":"Docker read failed"}).to_string(),
            };
        }
    });
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    loop {
        let (mut socket, _) = listener.accept().await?;
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            continue;
        };
        let cache = snapshot.clone();
        tokio::spawn(async move {
            let _p = permit;
            let mut buf = [0u8; 11];
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                socket.read_exact(&mut buf),
            )
            .await;
            if matches!(result, Ok(Ok(_))) && &buf == b"containers\n" {
                let data = cache.read().await.clone();
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    socket.write_all(data.as_bytes()),
                )
                .await;
            }
        });
    }
}
async fn command(args: &[&str]) -> Result<Vec<Value>> {
    let mut c = tokio::process::Command::new("docker");
    c.args(args)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = c.spawn()?;
    let mut out = vec![];
    let mut stdout = child.stdout.take().unwrap().take(256 * 1024 + 1);
    tokio::time::timeout(
        std::time::Duration::from_secs(8),
        stdout.read_to_end(&mut out),
    )
    .await??;
    ensure!(
        out.len() <= 256 * 1024,
        "Container list size limit exceeded"
    );
    let status = tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await??;
    ensure!(status.success(), "Docker unavailable");
    String::from_utf8(out)?
        .lines()
        .take(200)
        .map(|s| Ok(serde_json::from_str(s)?))
        .collect()
}
async fn collect() -> Result<Value> {
    let containers = command(&["ps", "-a", "--no-trunc", "--format", "{{json .}}"]).await?;
    let stats = command(&[
        "stats",
        "--no-stream",
        "--no-trunc",
        "--format",
        "{{json .}}",
    ])
    .await?;
    let sanitized:Vec<_>=containers.iter().map(|c|{let stat=stats.iter().find(|s|s["ID"]==c["ID"]);json!({"instance_id":c["ID"],"name":c["Names"],"image":c["Image"],"state":c["State"],"status":c["Status"],"cpu":stat.map(|s|&s["CPUPerc"]),"memory":stat.map(|s|&s["MemUsage"])})}).collect();
    Ok(json!({"status":"ok","at":guard_core::now(),"containers":sanitized}))
}
