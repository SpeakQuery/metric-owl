use crate::model::{Aggregate, Incident, Process, Sample};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};
type Job = Box<dyn FnOnce(&mut Connection) + Send>;
#[derive(Clone)]
pub struct Store {
    tx: mpsc::SyncSender<Job>,
    path: Arc<PathBuf>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;")?;
        let vacuum: u32 = conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
        if vacuum == 0 && std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) < 10 * 1024 * 1024 {
            conn.execute_batch("VACUUM;")?;
        }
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA journal_size_limit=8388608; PRAGMA wal_autocheckpoint=256; PRAGMA foreign_keys=ON; PRAGMA auto_vacuum=INCREMENTAL;
   CREATE TABLE IF NOT EXISTS kv(key TEXT PRIMARY KEY,value TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS aggregates(source TEXT NOT NULL,metric TEXT NOT NULL,start REAL NOT NULL,resolution INTEGER NOT NULL,json TEXT NOT NULL,PRIMARY KEY(source,metric,start,resolution));
   CREATE INDEX IF NOT EXISTS agg_time ON aggregates(resolution,start);
   CREATE TABLE IF NOT EXISTS incidents(id TEXT PRIMARY KEY,started REAL NOT NULL,updated REAL NOT NULL,resolved REAL,json TEXT NOT NULL);
   CREATE INDEX IF NOT EXISTS incident_time ON incidents(updated);
   CREATE TABLE IF NOT EXISTS details(at REAL PRIMARY KEY,json TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS process_snapshots(at REAL PRIMARY KEY,json TEXT NOT NULL);
   CREATE TABLE IF NOT EXISTS outbox(id TEXT PRIMARY KEY,kind TEXT NOT NULL,payload TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'pending',attempts INTEGER NOT NULL DEFAULT 0,next_at REAL NOT NULL,created REAL NOT NULL,error TEXT);
   CREATE INDEX IF NOT EXISTS outbox_due ON outbox(state,next_at);
   CREATE TABLE IF NOT EXISTS audit(at REAL NOT NULL,actor TEXT NOT NULL,action TEXT NOT NULL);
   CREATE INDEX IF NOT EXISTS audit_time ON audit(at);
   CREATE TABLE IF NOT EXISTS sessions(hash TEXT PRIMARY KEY,created REAL NOT NULL,last REAL NOT NULL,csrf TEXT NOT NULL,step_up REAL NOT NULL);
   CREATE TABLE IF NOT EXISTS mcp_clients(id TEXT PRIMARY KEY,name TEXT NOT NULL,hash TEXT NOT NULL UNIQUE,scopes TEXT NOT NULL,revoked INTEGER NOT NULL DEFAULT 0);
   CREATE TABLE IF NOT EXISTS budget_reservations(day TEXT,kind TEXT,id TEXT,PRIMARY KEY(day,kind,id));
   CREATE TABLE IF NOT EXISTS daily_budget(day TEXT NOT NULL,kind TEXT NOT NULL,used INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(day,kind));
   PRAGMA user_version=1;")?;
        let (tx, rx) = mpsc::sync_channel::<Job>(128);
        std::thread::Builder::new()
            .name("sqlite-writer".into())
            .spawn(move || {
                for job in rx {
                    job(&mut conn);
                }
            })?;
        Ok(Self {
            tx,
            path: Arc::new(path.into()),
        })
    }
    pub async fn write<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |c| {
            let _ = tx.send(f(c));
        });
        self.tx
            .try_send(job)
            .map_err(|_| anyhow::anyhow!("Database queue is full or closed"))?;
        rx.await?
    }
    pub async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let c = Connection::open_with_flags(
                &*path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            c.busy_timeout(std::time::Duration::from_secs(3))?;
            f(&c)
        })
        .await?
    }
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_owned();
        self.read(move |c| {
            use rusqlite::OptionalExtension;
            Ok(
                c.query_row("SELECT value FROM kv WHERE key=?1", [key], |r| r.get(0))
                    .optional()?,
            )
        })
        .await
    }
    pub async fn put(&self, key: &str, value: String) -> Result<()> {
        let key = key.to_owned();
        self.write(move |c| {
            c.execute(
                "INSERT INTO kv VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn aggregates(&self, rows: Vec<Aggregate>) -> Result<()> {
        self.write(move |c| {
            let tx = c.transaction()?;
            for a in rows {
                insert_aggregate(&tx, &a)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn history(
        &self,
        source: String,
        metric: String,
        from: f64,
        to: f64,
        resolution: u32,
    ) -> Result<Vec<Aggregate>> {
        anyhow::ensure!(
            from.is_finite() && to.is_finite() && from < to && to - from <= 366. * 86400.,
            "Invalid time range"
        );
        self.read(move|c|{let mut stmt=c.prepare("SELECT json FROM aggregates WHERE source=?1 AND metric=?2 AND start>=?3 AND start<?4 AND resolution=?5 ORDER BY start LIMIT 2000")?;let rows=stmt.query_map(params![source,metric,from,to,resolution],|r|r.get::<_,String>(0))?;rows.map(|r|Ok(serde_json::from_str(&r?)?)).collect()}).await
    }
    pub async fn incident(&self, i: Incident) -> Result<()> {
        self.write(move|c|{c.execute("INSERT INTO incidents VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET updated=excluded.updated,resolved=excluded.resolved,json=excluded.json",params![i.id,i.started_at,i.updated_at,i.resolved_at,serde_json::to_string(&i)?])?;Ok(())}).await
    }
    pub async fn incidents(&self, offset: u32) -> Result<Vec<Incident>> {
        self.read(move |c| {
            let mut s =
                c.prepare("SELECT json FROM incidents ORDER BY updated DESC LIMIT 100 OFFSET ?1")?;
            s.query_map([offset], |r| r.get::<_, String>(0))?
                .map(|r| Ok(serde_json::from_str(&r?)?))
                .collect()
        })
        .await
    }
    pub async fn active_incidents(&self) -> Result<Vec<Incident>> {
        self.read(|c| {
            let mut s = c.prepare(
                "SELECT json FROM incidents WHERE resolved IS NULL ORDER BY started LIMIT 10000",
            )?;
            s.query_map([], |r| r.get::<_, String>(0))?
                .map(|r| Ok(serde_json::from_str(&r?)?))
                .collect()
        })
        .await
    }
    pub async fn details(&self, samples: Vec<Sample>) -> Result<()> {
        self.write(move |c| {
            let tx = c.transaction()?;
            for s in samples {
                tx.execute(
                    "INSERT OR IGNORE INTO details VALUES(?1,?2)",
                    params![s.at, serde_json::to_string(&s)?],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn enqueue(
        &self,
        id: String,
        kind: String,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.write(move|c|{let now=crate::now();c.execute("INSERT OR IGNORE INTO outbox(id,kind,payload,next_at,created) VALUES(?1,?2,?3,?4,?4)",params![id,kind,payload.to_string(),now])?;Ok(())}).await
    }
    /// Small prospective snapshots complement incident-only full detail recording.
    pub async fn process_snapshot(&self, at: f64, processes: Vec<Process>) -> Result<()> {
        anyhow::ensure!(
            at.is_finite() && processes.len() <= 25,
            "Invalid process snapshot"
        );
        self.write(move |c| {
            c.execute(
                "INSERT OR REPLACE INTO process_snapshots(at,json) VALUES(?1,?2)",
                params![at, serde_json::to_string(&processes)?],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn budget(&self, kind: &str, limit: u32) -> Result<bool> {
        let kind = kind.to_owned();
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        self.write(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT OR IGNORE INTO daily_budget(day,kind) VALUES(?1,?2)",
                params![day, kind],
            )?;
            let count: u32 = tx.query_row(
                "SELECT used FROM daily_budget WHERE day=?1 AND kind=?2",
                params![day, kind],
                |r| r.get(0),
            )?;
            if count >= limit {
                return Ok(false);
            }
            tx.execute(
                "UPDATE daily_budget SET used=used+1 WHERE day=?1 AND kind=?2",
                params![day, kind],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await
    }
    pub async fn reserve_budget(&self, kind: &str, id: &str, limit: u32) -> Result<bool> {
        let kind = kind.to_owned();
        let id = id.to_owned();
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        self.write(move|c|{let tx=c.transaction()?;let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM budget_reservations WHERE day=?1 AND kind=?2 AND id=?3)",params![day,kind,id],|r|r.get(0))?;if exists{return Ok(true);}tx.execute("INSERT OR IGNORE INTO daily_budget(day,kind) VALUES(?1,?2)",params![day,kind])?;let count:u32=tx.query_row("SELECT used FROM daily_budget WHERE day=?1 AND kind=?2",params![day,kind],|r|r.get(0))?;if count>=limit{return Ok(false);}tx.execute("UPDATE daily_budget SET used=used+1 WHERE day=?1 AND kind=?2",params![day,kind])?;tx.execute("INSERT INTO budget_reservations VALUES(?1,?2,?3)",params![day,kind,id])?;tx.commit()?;Ok(true)}).await
    }
    pub async fn audit(&self, actor: String, action: String) -> Result<()> {
        self.write(move |c| {
            c.execute(
                "INSERT INTO audit VALUES(?1,?2,?3)",
                params![crate::now(), actor, action],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn retain(&self, now: f64) -> Result<()> {
        self.write(move|c|{
  for (old,new,age) in [(60,300,86400.),(300,3600,7.*86400.),(3600,86400,90.*86400.)]{
   // Only complete destination buckets are eligible. Merge and delete in one transaction.
   let cutoff=((now-age)/new as f64).floor()*new as f64;
   loop {let rows:Vec<Aggregate>={let mut s=c.prepare("SELECT json FROM aggregates WHERE resolution=?1 AND start<?2 ORDER BY start LIMIT 10000")?;s.query_map(params![old,cutoff],|r|r.get::<_,String>(0))?.map(|r|Ok(serde_json::from_str(&r?)?)).collect::<Result<_>>()?};if rows.is_empty(){break;}
    // Limit to full buckets; use one boundary batch to avoid replacing partial aggregates.
    let boundary=if rows.len()==10000 {(rows.last().unwrap().start/new as f64).floor()*new as f64}else{cutoff};
    let rows:Vec<_>=rows.into_iter().filter(|r|r.start<boundary).collect();if rows.is_empty(){break;}
    let tx=c.transaction()?;for a in crate::aggregate::merge(&rows,new){insert_aggregate(&tx,&a)?;}tx.execute("DELETE FROM aggregates WHERE resolution=?1 AND start<?2",params![old,boundary])?;tx.commit()?;
   }
  }
  let tx=c.transaction()?;tx.execute("DELETE FROM aggregates WHERE start<?1",[now-366.*86400.])?;tx.execute("DELETE FROM details WHERE at<?1",[now-30.*86400.])?;tx.execute("DELETE FROM incidents WHERE resolved IS NOT NULL AND updated<?1",[now-366.*86400.])?;tx.execute("DELETE FROM audit WHERE at<?1",[now-90.*86400.])?;tx.execute("DELETE FROM outbox WHERE state IN ('accepted','provider_accepted','test_delivered','delivered','failed','unknown') AND created<?1",[now-30.*86400.])?;tx.execute("DELETE FROM daily_budget WHERE day<?1",[(chrono::Utc::now()-chrono::Duration::days(40)).format("%Y-%m-%d").to_string()])?;tx.execute("DELETE FROM budget_reservations WHERE day<?1",[(chrono::Utc::now()-chrono::Duration::days(40)).format("%Y-%m-%d").to_string()])?;tx.commit()?;
  c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum(256);")?;Ok(())}).await
    }
    pub fn size_bytes(&self) -> u64 {
        let dir = self.path.parent().unwrap();
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum()
    }
    pub async fn enforce_quota(&self, quota: u64) -> Result<bool> {
        let path = self.path.clone();
        let initial = self.size_bytes();
        self.write(move|c|{
        // ai:verified is a non-JSON connection fingerprint, not an analysis cache.
        // CASE also protects JSON extraction from malformed legacy cache records.
        c.execute("DELETE FROM kv WHERE key LIKE 'ai:%' AND CASE WHEN json_valid(value) THEN json_type(value,'$.analysis')='text' AND json_type(value,'$.at') IN ('integer','real') AND CAST(json_extract(value,'$.at') AS REAL) < ?1 ELSE 0 END",[crate::now()-30.*86400.])?;
        c.execute("DELETE FROM process_snapshots WHERE at<?1", [crate::now()-7.*86400.])?;
        if initial>=quota*7/10 {
          c.execute("INSERT INTO kv VALUES('retention_warning',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[format!("Storage pressure: retention has been shortened. Some older details or summaries may have been removed. {}",chrono::Utc::now())])?;
          // Preserve pending delivery, identity, credentials, and unresolved incidents.
          // A bounded batch makes storage pressure visible instead of looping indefinitely.
          for _ in 0..200 {
            compact(c)?;
            let size=std::fs::metadata(&*path).map(|m|m.len()).unwrap_or(0)+std::fs::metadata(format!("{}-wal",path.display())).map(|m|m.len()).unwrap_or(0);
            if size<quota*6/10{break;}
            let mut removed=c.execute("DELETE FROM details WHERE at IN (SELECT at FROM details WHERE at<?1 ORDER BY at LIMIT 1000)",[crate::now()-120.])?;
            if removed==0{removed=c.execute("DELETE FROM process_snapshots WHERE at IN (SELECT at FROM process_snapshots WHERE at<?1 ORDER BY at LIMIT 1000)",[crate::now()-120.])?;}
            if removed==0{removed=c.execute("DELETE FROM kv WHERE key IN (SELECT key FROM kv WHERE key LIKE 'ai:%' AND CASE WHEN json_valid(value) THEN json_type(value,'$.analysis')='text' AND json_type(value,'$.at') IN ('integer','real') ELSE 0 END ORDER BY CASE WHEN json_valid(value) THEN CAST(json_extract(value,'$.at') AS REAL) END LIMIT 100)",[])?;}
            if removed==0{removed=c.execute("DELETE FROM audit WHERE rowid IN (SELECT rowid FROM audit ORDER BY at LIMIT 5000)",[])?;}
            if removed==0{removed=c.execute("DELETE FROM outbox WHERE id IN (SELECT id FROM outbox WHERE state NOT IN ('pending','relay_queued') ORDER BY created LIMIT 1000)",[])?;}
            if removed==0{removed=c.execute("DELETE FROM aggregates WHERE rowid IN (SELECT rowid FROM aggregates ORDER BY start LIMIT 2000)",[])?;}
            if removed==0{removed=c.execute("DELETE FROM incidents WHERE id IN (SELECT id FROM incidents WHERE resolved IS NOT NULL ORDER BY updated LIMIT 500)",[])?;}
            if removed==0{break;}
          }
        }
        // Reserve 15% for WAL, queue overhead, master key, and diagnostics.
        let page_size:u32=c.query_row("PRAGMA page_size",[],|r|r.get(0))?;
        c.execute_batch(&format!("PRAGMA max_page_count={};",quota*85/100/page_size as u64))?;compact(c)?;
        Ok(())
      }).await?;
        if self.size_bytes() > quota {
            anyhow::bail!(
                "Storage quota is still exceeded; pending messages and open incidents are preserved. New details are paused."
            );
        }
        Ok(initial >= quota * 7 / 10)
    }
}
fn compact(c: &Connection) -> Result<()> {
    // incremental_vacuum emits one row per removed page; consume every row.
    {
        let mut statement = c.prepare("PRAGMA incremental_vacuum(2048)")?;
        let mut rows = statement.query([])?;
        while rows.next()?.is_some() {}
    }
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}
fn insert_aggregate(c: &Connection, a: &Aggregate) -> Result<()> {
    c.execute("INSERT INTO aggregates VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source,metric,start,resolution) DO UPDATE SET json=excluded.json",params![a.source,a.metric,a.start,a.resolution,serde_json::to_string(a)?])?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rollup_transaction_and_history() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("x.sqlite")).unwrap();
        let rows = (0..60)
            .map(|i| Aggregate {
                source: "host".into(),
                metric: "cpu".into(),
                start: i as f64 * 60.,
                end: (i + 1) as f64 * 60.,
                resolution: 60,
                min: Some(5.),
                max: Some(99.),
                first: Some(5.),
                last: Some(5.),
                weighted_sum: 300.,
                observed_seconds: 60.,
                total: 0.,
            })
            .collect();
        s.aggregates(rows).await.unwrap();
        s.retain(2. * 86400.).await.unwrap();
        let r = s
            .history("host".into(), "cpu".into(), 0., 3600., 300)
            .await
            .unwrap();
        assert_eq!(r.len(), 12);
        assert_eq!(r[0].max, Some(99.));
        assert_eq!(r[0].observed_seconds, 300.);
        assert!(
            s.history("host".into(), "cpu".into(), 0., 3600., 60)
                .await
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn budget_is_atomic() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("x.sqlite")).unwrap();
        assert!(s.budget("ai", 1).await.unwrap());
        assert!(!s.budget("ai", 1).await.unwrap());
    }
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    #[tokio::test]
    async fn failed_rollup_does_not_delete_input() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        s.aggregates(vec![Aggregate {
            source: "host".into(),
            metric: "cpu".into(),
            start: 0.,
            end: 60.,
            resolution: 60,
            min: Some(1.),
            max: Some(90.),
            observed_seconds: 60.,
            ..Default::default()
        }])
        .await
        .unwrap();
        s.write(|c|{c.execute_batch("CREATE TRIGGER fail_rollup BEFORE INSERT ON aggregates WHEN NEW.resolution=300 BEGIN SELECT RAISE(ABORT,'injected failure'); END;")?;Ok(())}).await.unwrap();
        assert!(s.retain(2. * 86400.).await.is_err());
        assert_eq!(
            s.history("host".into(), "cpu".into(), 0., 100., 60)
                .await
                .unwrap()
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn message_retry_reserves_budget_once() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        assert!(s.reserve_budget("mail", "one", 1).await.unwrap());
        assert!(s.reserve_budget("mail", "one", 1).await.unwrap());
        assert!(!s.reserve_budget("mail", "two", 1).await.unwrap());
    }
    #[tokio::test]
    async fn quota_reclaims_cache_and_preserves_credentials() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        s.put("admin", "preserve".into()).await.unwrap();
        s.write(|c| {
            let tx = c.transaction()?;
            for i in 0..6500 {
                tx.execute(
                    "INSERT INTO kv VALUES(?1,?2)",
                    params![
                        format!("ai:{i}"),
                        serde_json::json!({"at":crate::now(),"analysis":"x".repeat(12000)})
                            .to_string()
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
        s.enforce_quota(64 * 1024 * 1024).await.unwrap();
        assert!(s.size_bytes() <= 64 * 1024 * 1024);
        assert_eq!(s.get("admin").await.unwrap().as_deref(), Some("preserve"));
    }
    #[tokio::test]
    async fn quota_does_not_parse_or_evict_non_cache_ai_settings() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        s.put("ai:verified", "opaque-connection-fingerprint".into())
            .await
            .unwrap();
        s.put("ai:legacy", "{broken".into()).await.unwrap();
        s.put(
            "ai:expired",
            serde_json::json!({"analysis":"old","at":crate::now()-31.*86400.}).to_string(),
        )
        .await
        .unwrap();
        s.put(
            "ai:fresh",
            serde_json::json!({"analysis":"new","at":crate::now()}).to_string(),
        )
        .await
        .unwrap();
        s.put("secret:ai", "encrypted-credential".into())
            .await
            .unwrap();
        s.enforce_quota(64 * 1024 * 1024).await.unwrap();
        assert_eq!(
            s.get("ai:verified").await.unwrap().as_deref(),
            Some("opaque-connection-fingerprint")
        );
        assert_eq!(
            s.get("ai:legacy").await.unwrap().as_deref(),
            Some("{broken")
        );
        assert!(s.get("ai:expired").await.unwrap().is_none());
        assert!(s.get("ai:fresh").await.unwrap().is_some());
        assert_eq!(
            s.get("secret:ai").await.unwrap().as_deref(),
            Some("encrypted-credential")
        );
    }
    #[tokio::test]
    async fn prospective_process_snapshots_have_bounded_retention() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        s.process_snapshot(crate::now() - 8. * 86400., vec![])
            .await
            .unwrap();
        s.process_snapshot(crate::now(), vec![]).await.unwrap();
        s.enforce_quota(64 * 1024 * 1024).await.unwrap();
        assert_eq!(
            s.read(|db| Ok(db
                .query_row("SELECT COUNT(*) FROM process_snapshots", [], |r| r
                    .get::<_, u32>(0))?))
                .await
                .unwrap(),
            1
        );
        assert!(s.process_snapshot(f64::NAN, vec![]).await.is_err());
    }
    #[tokio::test]
    async fn representative_90_days_roll_up_and_query() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        let now = 100. * 86400.;
        let rows = (0..90 * 24 * 60)
            .map(|i| {
                let start = 10. * 86400. + i as f64 * 60.;
                Aggregate {
                    source: "host".into(),
                    metric: "cpu".into(),
                    start,
                    end: start + 60.,
                    resolution: 60,
                    min: Some(10.),
                    max: Some(if i % 1000 == 0 { 99. } else { 20. }),
                    first: Some(10.),
                    last: Some(20.),
                    weighted_sum: 900.,
                    observed_seconds: 60.,
                    total: 0.,
                }
            })
            .collect();
        s.aggregates(rows).await.unwrap();
        s.retain(now).await.unwrap();
        let started = std::time::Instant::now();
        let hours = s
            .history(
                "host".into(),
                "cpu".into(),
                10. * 86400.,
                90. * 86400.,
                3600,
            )
            .await
            .unwrap();
        assert!(!hours.is_empty());
        assert!(hours.iter().any(|a| a.max == Some(99.)));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            s.history("host".into(), "cpu".into(), 99. * 86400., now, 60)
                .await
                .unwrap()
                .len(),
            1440
        );
    }
}
#[cfg(test)]
mod headroom_regression {
    use super::*;
    #[tokio::test]
    async fn full_database_below_quota_can_recover() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("db")).unwrap();
        let quota = 64 * 1024 * 1024;
        s.enforce_quota(quota).await.unwrap();
        let inserted = s
            .write(|c| {
                let mut count = 0;
                for i in 0..7000 {
                    if c.execute(
                        "INSERT INTO kv VALUES(?1,?2)",
                        params![
                            format!("ai:{i}"),
                            serde_json::json!({"at":crate::now(),"analysis":"x".repeat(12000)})
                                .to_string()
                        ],
                    )
                    .is_err()
                    {
                        break;
                    }
                    count += 1;
                }
                Ok(count)
            })
            .await
            .unwrap();
        assert!(inserted > 1000 && inserted < 7000);
        s.enforce_quota(quota).await.unwrap();
        s.put("after_full", "writable".into()).await.unwrap();
        assert!(s.size_bytes() < quota);
    }
}
