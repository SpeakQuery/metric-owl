use crate::App;
use guard_core::{aggregate::add, model::*, rules::Engine};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use sysinfo::{Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};
fn process_refresh_kind() -> ProcessRefreshKind {
    // sysinfo includes Linux /proc/<pid>/task entries even with `nothing()`.
    // Counting those as processes duplicates their owner's RSS and CPU totals.
    ProcessRefreshKind::nothing()
        .without_tasks()
        .with_cpu()
        .with_memory()
}
pub fn host(id: String) -> anyhow::Result<Host> {
    let mut s = System::new()?;
    s.refresh_cpu_all();
    s.refresh_memory();
    Ok(Host {
        id,
        name: System::host_name().unwrap_or("Unknown".into()),
        os: format!(
            "{} {}",
            System::name().unwrap_or_default(),
            System::os_version().unwrap_or_default()
        ),
        cpu_model: s
            .cpus()
            .first()
            .map(|c| c.brand())
            .unwrap_or("Unknown")
            .into(),
        cores: s.cpus().len() as u32,
        memory_total: s.total_memory() as f64,
        version: env!("CARGO_PKG_VERSION").into(),
    })
}
pub fn rate(previous: Option<(u64, u64)>, current: (u64, u64), dt: f64) -> (Reading, Reading) {
    if let Some((a, b)) = previous
        && dt > 0.
        && dt < 2.5
        && current.0 >= a
        && current.1 >= b
    {
        (
            Reading::good((current.0 - a) as f64 / dt, "B/s"),
            Reading::good((current.1 - b) as f64 / dt, "B/s"),
        )
    } else {
        (
            Reading::unavailable("B/s", "Counter is warming up or has reset"),
            Reading::unavailable("B/s", "Counter is warming up or has reset"),
        )
    }
}
pub(crate) const ABSOLUTE_DISK_MIN_BYTES: u64 = 10 * 1024 * 1024 * 1024;
pub(crate) fn disk_defaults_for_partition(id: &str, total: u64) -> Vec<Rule> {
    let mut rules = vec![];
    for (metric, threshold, op, severity, recover) in [
        ("disk_used", 85., ">=", "warning", 82.),
        ("disk_used", 95., ">=", "critical", 92.),
        ("disk_free", 1073741824., "<", "critical", 1395864371.),
    ] {
        if metric == "disk_free" && total < ABSOLUTE_DISK_MIN_BYTES {
            continue;
        }
        rules.push(Rule {
            id: format!(
                "disk-{}-{}-{severity}",
                &crate::auth::hash(id)[..24],
                metric
            ),
            name: format!(
                "Disk {} · {}",
                id,
                if metric == "disk_free" {
                    "Below 1 GiB"
                } else {
                    severity
                }
            ),
            source: id.into(),
            metric: metric.into(),
            operator: op.into(),
            threshold,
            window_seconds: 30,
            fraction: 1.,
            severity: severity.into(),
            recovery_threshold: recover,
            recovery_seconds: 30,
            enabled: true,
            notify: true,
        });
    }
    rules
}
pub(crate) fn resource_leaders(
    rows: &[Process],
    cpu_count: usize,
    memory_count: usize,
) -> Vec<Process> {
    let mut by_cpu = rows.to_vec();
    by_cpu.sort_by(|a, b| b.cpu.unwrap_or(-1.).total_cmp(&a.cpu.unwrap_or(-1.)));
    let mut selected: Vec<_> = by_cpu.into_iter().take(cpu_count).collect();
    let mut by_memory = rows.to_vec();
    by_memory.sort_by(|a, b| b.memory.unwrap_or(-1.).total_cmp(&a.memory.unwrap_or(-1.)));
    for p in by_memory.into_iter().take(memory_count) {
        if !selected.iter().any(|s| s.id == p.id) {
            selected.push(p);
        }
    }
    selected.sort_by(|a, b| b.cpu.unwrap_or(-1.).total_cmp(&a.cpu.unwrap_or(-1.)));
    selected
}
pub fn disk_defaults() -> anyhow::Result<Vec<Rule>> {
    let disks = Disks::new_with_refreshed_list()?;
    Ok(disks
        .iter()
        .flat_map(|disk| {
            disk_defaults_for_partition(&disk.mount_point().to_string_lossy(), disk.total_space())
        })
        .collect())
}
pub fn start(app: App) {
    tokio::spawn(async move {
        if let Err(e) = run(app.clone()).await {
            app.error(format!("Metrics collection stopped: {e}")).await;
        }
    });
}
async fn run(app: App) -> anyhow::Result<()> {
    let mut system = System::new()?;
    system.refresh_cpu_all();
    let mut networks = Networks::new_with_refreshed_list().ok();
    let mut disks = Disks::new_with_refreshed_list().ok();
    let started = Instant::now();
    let mut last = started;
    let mut previous = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut n = 0u64;
    let mut disk_rows = vec![];
    let mut process_rows = vec![];
    let diagnostic_feed = crate::diagnostics::start();
    let mut diagnostic_rows = vec![];
    let mut self_mem = 0.;
    let mut self_cpu = 0.;
    let mut ring: VecDeque<Sample> = VecDeque::with_capacity(301);
    let mut engine = Engine::default();
    engine.restore(app.store.active_incidents().await?);
    let mut aggregates: HashMap<(String, String), Aggregate> = HashMap::new();
    let mut detail_until = 0.;
    let mut spike_until = 0.;
    let mut previous_cpu: Option<f64> = None;
    let mut bucket = 0.;
    let mut pending_rows = Vec::new();
    let mut pending_events: VecDeque<Incident> = VecDeque::new();
    let mut previous_utc = 0.;
    loop {
        tick.tick().await;
        let instant = Instant::now();
        let dt = instant.duration_since(last).as_secs_f64();
        last = instant;
        let mono = started.elapsed().as_secs_f64();
        let utc = guard_core::now();
        system.refresh_cpu_usage();
        system.refresh_memory();
        let warm = n > 0 && dt < 2.5;
        let collection_gap = n > 0 && (!warm || (utc - previous_utc - dt).abs() >= 1.);
        let cpu = if warm {
            Reading::good(system.global_cpu_usage() as f64, "%")
        } else {
            Reading::unavailable("%", "Measurement is warming up")
        };
        let memory_total = system.total_memory() as f64;
        // macOS available_memory includes active pages (XNU reclaimability concept).
        // Use native used-memory accounting there; Linux uses MemAvailable.
        let memory_used = if cfg!(target_os = "macos") {
            system.used_memory()
        } else {
            system
                .total_memory()
                .saturating_sub(system.available_memory())
        } as f64;
        let memory = if memory_total > 0. {
            Reading::good(100. * memory_used / memory_total, "%")
        } else {
            Reading::unavailable("%", "Memory measurement unavailable")
        };
        let (net_in, net_out) = if let Some(net) = &mut networks {
            net.refresh(true);
            let current = net
                .iter()
                .filter(|(name, _)| {
                    !name.to_string_lossy().starts_with("lo")
                        && !name.to_string_lossy().starts_with("utun")
                })
                .fold((0u64, 0u64), |a, (_, v)| {
                    (
                        a.0.saturating_add(v.total_received()),
                        a.1.saturating_add(v.total_transmitted()),
                    )
                });
            let rates = rate(previous, current, dt);
            previous = Some(current);
            rates
        } else {
            (
                Reading::unavailable("B/s", "Network counters unavailable"),
                Reading::unavailable("B/s", "Network counters unavailable"),
            )
        };
        if n.is_multiple_of(5) {
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                process_refresh_kind(),
            );
            process_rows = system
                .processes()
                .iter()
                .filter(|(_, process)| process.thread_kind() != Some(sysinfo::ThreadKind::Userland))
                .map(|(pid, p)| Process {
                    id: format!("{}:{}", pid.as_u32(), p.start_time()),
                    name: p.name().to_string_lossy().into(),
                    pid: pid.as_u32(),
                    cpu: (n >= 5).then_some(p.cpu_usage() as f64),
                    memory: (p.memory() > 0).then_some(p.memory() as f64),
                })
                .collect();
            if let Some(p) = process_rows.iter().find(|p| p.pid == std::process::id()) {
                self_mem = p.memory.unwrap_or(0.);
                self_cpu = p.cpu.unwrap_or(0.);
            }
            {
                let mut inventory = app.inventory.write().await;
                inventory.services = crate::discovery::services(&process_rows);
                inventory.services_checked_at = Some(utc);
            }
            process_rows = resource_leaders(&process_rows, 20, 5);
        }
        if n.is_multiple_of(15)
            && let Some(ds) = &mut disks
        {
            ds.refresh(true);
            disk_rows = ds
                .iter_mut()
                .map(|d| {
                    let ok = d.refresh();
                    let total = d.total_space() as f64;
                    let free = d.available_space() as f64;
                    Disk {
                        id: d.mount_point().to_string_lossy().into(),
                        mount: d.mount_point().to_string_lossy().into(),
                        total,
                        available: free,
                        used_percent: if ok && total > 0. {
                            Reading::good((1. - free / total) * 100., "%")
                        } else {
                            Reading::unavailable("%", "Disk unavailable")
                        },
                    }
                })
                .collect();
        }
        if n.is_multiple_of(5) {
            diagnostic_rows = crate::diagnostics::swap(system.total_swap(), system.used_swap());
            diagnostic_rows.extend(crate::diagnostics::current(&diagnostic_feed));
        }
        if collection_gap {
            for diagnostic in &mut diagnostic_rows {
                diagnostic.reading = Reading::unavailable(
                    &diagnostic.reading.unit,
                    "Collection was interrupted or the system clock changed; awaiting a fresh diagnostic sample",
                );
            }
        }
        let sample = Sample {
            source_id: app.host.id.clone(),
            at: utc,
            uptime: System::uptime().unwrap_or(0) as f64,
            cpu: cpu.clone(),
            cores: if warm {
                system.cpus().iter().map(|c| c.usage() as f64).collect()
            } else {
                vec![]
            },
            memory: memory.clone(),
            memory_used,
            memory_total,
            network_in: net_in.clone(),
            network_out: net_out.clone(),
            disks: disk_rows.clone(),
            diagnostics: diagnostic_rows.clone(),
            processes: process_rows.clone(),
            agent_memory: self_mem,
            agent_cpu: self_cpu,
        };
        ring.push_back(sample.clone());
        while ring.len() > 300 || ring.front().is_some_and(|s| s.at < utc - 300.) {
            ring.pop_front();
        }
        *app.ring.write().await = ring.clone();
        *app.latest.write().await = Some(sample.clone());
        let _ = app.events.send(serde_json::to_string(&sample)?);
        let current_bucket = (utc / 60.).floor() * 60.;
        let valid_duration = if warm && (utc - previous_utc - dt).abs() < 1. {
            dt
        } else {
            0.
        };
        let mut values = vec![
            ("host".to_string(), "cpu", cpu.value),
            ("host".into(), "memory", memory.value),
            ("host".into(), "network_in", net_in.value),
            ("host".into(), "network_out", net_out.value),
        ];
        for d in &disk_rows {
            values.push((d.id.clone(), "disk_used", d.used_percent.value));
            values.push((
                d.id.clone(),
                "disk_free",
                d.used_percent.value.map(|_| d.available),
            ));
            values.push((
                d.id.clone(),
                "disk_total",
                d.used_percent.value.map(|_| d.total),
            ));
        }
        for diagnostic in &diagnostic_rows {
            values.push((
                diagnostic.source.clone(),
                diagnostic.metric.as_str(),
                diagnostic.reading.value,
            ));
        }
        if current_bucket != bucket && !aggregates.is_empty() {
            let previous_part = if current_bucket > bucket && valid_duration > 0. {
                (current_bucket - previous_utc).clamp(0., valid_duration)
            } else {
                0.
            };
            for (source, metric, value) in &values {
                if let Some(a) = aggregates.get_mut(&(source.clone(), metric.to_string())) {
                    add(a, *value, previous_part, metric.starts_with("network"));
                }
            }
            pending_rows.extend(aggregates.drain().map(|(_, a)| a));
        }
        bucket = current_bucket;
        for (source, metric, value) in values {
            let a = aggregates
                .entry((source.clone(), metric.into()))
                .or_insert_with(|| Aggregate {
                    source,
                    metric: metric.into(),
                    start: bucket,
                    end: bucket + 60.,
                    resolution: 60,
                    ..Default::default()
                });
            add(
                a,
                value,
                valid_duration.min((utc - bucket).max(0.)),
                metric.starts_with("network"),
            );
        }
        previous_utc = utc;
        if !pending_rows.is_empty() {
            if let Err(e) = app.store.aggregates(pending_rows.clone()).await {
                app.error(format!("History queue is waiting: {e}")).await;
                if pending_rows.len() > 2000 {
                    pending_rows.drain(..pending_rows.len() - 2000);
                    app.error(
                        "The oldest unsaved summaries were lost during a storage outage".into(),
                    )
                    .await;
                }
            } else {
                pending_rows.clear();
            }
        }
        let rules = app.rules.read().await.clone();
        if n >= 5
            && n.is_multiple_of(60)
            && app.store.size_bytes() < app.settings.read().await.quota_bytes
        {
            let selected = resource_leaders(&process_rows, 5, 5);
            if let Err(e) = app.store.process_snapshot(utc, selected).await {
                app.error(format!("Unable to save process snapshot: {e}"))
                    .await;
            }
        }
        let mut events = vec![];
        for (metric, value) in [
            ("cpu", cpu.value),
            ("memory", memory.value),
            ("network_in", net_in.value),
            ("network_out", net_out.value),
        ] {
            events.extend(engine.evaluate(&rules, "host", metric, mono, utc, value, 1.));
        }
        if n.is_multiple_of(15) {
            for d in &disk_rows {
                let disk_rules = rules.clone();
                for (metric, value) in [
                    ("disk_used", d.used_percent.value),
                    ("disk_free", d.used_percent.value.map(|_| d.available)),
                ] {
                    events.extend(engine.evaluate(
                        &disk_rules,
                        &d.id,
                        metric,
                        mono,
                        utc,
                        value,
                        15.,
                    ));
                }
            }
        }
        if n.is_multiple_of(5) {
            for diagnostic in &diagnostic_rows {
                events.extend(engine.evaluate(
                    &rules,
                    &diagnostic.source,
                    &diagnostic.metric,
                    mono,
                    utc,
                    diagnostic.reading.value,
                    5.,
                ));
            }
        }
        let spike = cpu
            .value
            .zip(previous_cpu)
            .is_some_and(|(a, b)| a - b >= 30. && a >= 80.);
        previous_cpu = cpu.value;
        if spike && mono > spike_until {
            spike_until = mono + 60.;
            events.push(Incident {
                id: uuid::Uuid::new_v4().to_string(),
                key: "host:cpu-spike".into(),
                title: "Brief CPU spike recorded".into(),
                severity: "info".into(),
                started_at: utc,
                updated_at: utc,
                resolved_at: Some(utc),
                evidence: format!(
                    "CPU {:.1}% — no email sent because the alert duration was not reached.",
                    cpu.value.unwrap_or_default()
                ),
                recommendation: "Review processes and history around this incident.".into(),
                notify: false,
            });
        }
        if !events.is_empty() {
            detail_until = mono + 120.;
            if app.store.size_bytes() < app.settings.read().await.quota_bytes
                && let Err(e) = app.store.details(ring.iter().cloned().collect()).await
            {
                app.error(format!("Unable to save pre-incident details: {e}"))
                    .await;
            }
        }
        pending_events.extend(events);
        while let Some(event) = pending_events.front() {
            if let Err(e) = crate::integrations::incident(&app, event).await {
                app.error(format!(
                    "Atomic incident and notification write is pending: {e}"
                ))
                .await;
                break;
            }
            pending_events.pop_front();
        }
        if pending_events.len() > 1000 {
            pending_events.pop_front();
            app.error(
                "The incident buffer overflowed during a storage outage; the oldest incident was lost".into(),
            )
            .await;
        }
        if mono < detail_until {
            if app.store.size_bytes() < app.settings.read().await.quota_bytes {
                if let Err(e) = app.store.details(vec![sample]).await {
                    app.error(format!("Unable to save incident details: {e}"))
                        .await;
                }
            } else {
                app.error("Storage quota reached: detailed incident recording is paused".into())
                    .await;
            }
        }
        if n.is_multiple_of(300) {
            if let Err(e) = app.store.retain(utc).await {
                app.error(format!("Aggregation or retention error: {e}"))
                    .await;
            }
            if let Err(e) = app
                .store
                .enforce_quota(app.settings.read().await.quota_bytes)
                .await
            {
                app.error(format!("Quota check: {e}")).await;
            }
        }
        n += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn process_collection_does_not_count_linux_threads_as_processes() {
        let (send, receive) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let tid = unsafe { libc::gettid() } as u32;
            send.send(tid).unwrap();
            std::thread::park();
        });
        let tid = receive.recv().unwrap();
        let mut system = System::new().unwrap();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh_kind());
        let owner_present = system
            .process(sysinfo::Pid::from_u32(std::process::id()))
            .is_some();
        let task_present = system.process(sysinfo::Pid::from_u32(tid)).is_some();
        thread.thread().unpark();
        thread.join().unwrap();
        assert!(owner_present, "The real process must remain visible");
        assert!(
            !task_present,
            "A process thread must not duplicate its owner's RSS"
        );
    }
    #[test]
    fn counter_reset_is_unavailable() {
        assert!(rate(Some((100, 100)), (10, 100), 1.).0.value.is_none());
        assert_eq!(rate(Some((100, 100)), (200, 300), 2.).0.value, Some(50.));
    }
}
