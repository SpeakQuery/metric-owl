//! OS restarts are distinct from agent restarts. Persist a baseline before reporting.
use crate::App;
use anyhow::{Result, ensure};
use guard_core::{model::Incident, store::Store};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct BootRecord {
    identity: String,
    observed_at: f64,
}

fn identity() -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        let id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        let parsed = uuid::Uuid::parse_str(id.trim())?;
        Ok(format!("linux:{parsed}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let at = sysinfo::System::boot_time()?;
        ensure!(at > 0, "OS boot time unavailable");
        Ok(format!("boot:{at}"))
    }
}

async fn observe(store: &Store, identity: String, now: f64) -> Result<Option<Incident>> {
    ensure!(
        !identity.is_empty() && identity.len() < 100,
        "Invalid boot identity"
    );
    store.write(move |c| {
        use rusqlite::{OptionalExtension, params};
        let tx = c.transaction()?;
        let previous: Option<String> = tx.query_row("SELECT value FROM kv WHERE key='os_boot'", [], |r| r.get(0)).optional()?;
        let previous = previous.map(|p| serde_json::from_str::<BootRecord>(&p)).transpose()?;
        let changed = previous.as_ref().is_some_and(|p| p.identity != identity);
        let mut event = None;
        if changed {
            let id = format!("boot-{}", crate::auth::hash(&identity));
            event = Some(Incident {
                id: id.clone(), key: "host:restart".into(), title: "Server restarted".into(),
                severity: "info".into(), started_at: now, updated_at: now, resolved_at: Some(now),
                evidence: "The operating system boot identity changed since the last observation. The server is running again. The shutdown time and cause are unknown.".into(),
                recommendation: "If the restart was unexpected, review permitted system logs and scheduled maintenance. Metric Owl did not restart the server.".into(),
                notify: true,
            });
            // Persist a pending report atomically with the new boot identity, so a
            // crash before notification enqueue does not lose the restart event.
            tx.execute("INSERT INTO kv VALUES('pending_boot_report',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [serde_json::to_string(&event)?])?;
        }
        tx.execute("INSERT INTO kv VALUES('os_boot',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![serde_json::to_string(&BootRecord{identity,observed_at:now})?])?;
        tx.commit()?;
        Ok(event)
    }).await
}

pub async fn check(app: &App) -> Result<()> {
    let _ = observe(&app.store, identity()?, guard_core::now()).await?;
    flush(app).await
}

pub async fn flush(app: &App) -> Result<()> {
    if let Some(raw) = app.store.get("pending_boot_report").await?
        && let Some(event) = serde_json::from_str::<Option<Incident>>(&raw)?
    {
        crate::integrations::incident(app, &event).await?;
        app.store
            .write(|c| {
                c.execute("DELETE FROM kv WHERE key='pending_boot_report'", [])?;
                Ok(())
            })
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn first_install_agent_restart_and_os_restart_are_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("guard.sqlite")).unwrap();
        assert!(
            observe(&store, "boot-a".into(), 100.)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            observe(&store, "boot-a".into(), 200.)
                .await
                .unwrap()
                .is_none()
        );
        let event = observe(&store, "boot-b".into(), 300.)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.title, "Server restarted");
        assert_eq!(event.resolved_at, Some(300.));
        assert!(
            observe(&store, "boot-b".into(), 400.)
                .await
                .unwrap()
                .is_none()
        );
        assert!(store.get("pending_boot_report").await.unwrap().is_some());
    }
}
