//! Bounded, read-only pressure and storage diagnostics. Linux counter semantics:
//! https://docs.kernel.org/admin-guide/iostats.html
//! https://docs.kernel.org/accounting/psi.html
//! https://docs.kernel.org/filesystems/proc.html
use guard_core::model::{Diagnostic, Reading, Rule};
#[cfg(any(target_os = "linux", test))]
use std::collections::VecDeque;
use std::time::Instant;
#[cfg(target_os = "linux")]
use std::{collections::HashMap, time::Duration};
use tokio::sync::watch;

const CADENCE: f64 = 5.;
const MAX_GAP: f64 = CADENCE * 2.5;
#[cfg(target_os = "linux")]
const MAX_DEVICES: usize = 32;
#[cfg(any(target_os = "linux", test))]
const MAX_MOUNTS: usize = 32;
fn item(source: &str, metric: &str, label: &str, reading: Reading) -> Diagnostic {
    Diagnostic {
        source: source.into(),
        metric: metric.into(),
        label: label.into(),
        reading,
    }
}
pub fn swap(total: u64, used: u64) -> Vec<Diagnostic> {
    let percent = if total == 0 {
        Reading::unavailable("%", "No swap capacity is currently allocated")
    } else if used > total {
        Reading::unavailable("%", "Swap counters are inconsistent")
    } else {
        Reading::good(100. * used as f64 / total as f64, "%")
    };
    vec![
        item(
            "host",
            "swap_used",
            "Swap used",
            Reading::good(used as f64, "B"),
        ),
        item(
            "host",
            "swap_total",
            "Swap capacity",
            Reading::good(total as f64, "B"),
        ),
        item("host", "swap_used_percent", "Swap usage", percent),
    ]
}
fn unavailable_linux(reason: &str) -> Vec<Diagnostic> {
    [
        ("memory_pressure", "Memory pressure · PSI some avg10", "%"),
        ("cpu_iowait", "CPU I/O wait", "%"),
        (
            "oom_kills_5m",
            "Observed OOM kills · last 5 minutes",
            "count",
        ),
        ("disk_io_latency", "Disk read/write latency", "ms"),
        ("disk_io_utilization", "Disk I/O busy time", "%"),
        ("inode_used", "Filesystem inodes used", "%"),
    ]
    .into_iter()
    .map(|(metric, label, unit)| item("host", metric, label, Reading::unavailable(unit, reason)))
    .collect()
}
#[derive(Clone)]
pub struct Snapshot {
    at: Instant,
    rows: Vec<Diagnostic>,
}
pub fn current(receiver: &watch::Receiver<Snapshot>) -> Vec<Diagnostic> {
    let snapshot = receiver.borrow();
    if !cfg!(target_os = "linux") || snapshot.at.elapsed().as_secs_f64() <= MAX_GAP {
        return snapshot.rows.clone();
    }
    snapshot
        .rows
        .iter()
        .map(|row| {
            item(
                &row.source,
                &row.metric,
                &row.label,
                Reading::unavailable(
                    &row.reading.unit,
                    "Diagnostic collection is delayed; the previous reading is stale",
                ),
            )
        })
        .collect()
}
/// At most one worker may be blocked in a filesystem syscall. Collection never
/// waits for it and never starts an accumulating queue of replacement workers.
pub fn start() -> watch::Receiver<Snapshot> {
    let reason = if cfg!(target_os = "linux") {
        "Diagnostic counters are warming up"
    } else {
        "This diagnostic requires Linux kernel interfaces"
    };
    let (sender, receiver) = watch::channel(Snapshot {
        at: Instant::now(),
        rows: unavailable_linux(reason),
    });
    #[cfg(target_os = "linux")]
    tokio::spawn(async move {
        let mut collector = Collector::default();
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if sender.is_closed() {
                break;
            }
            match tokio::task::spawn_blocking(move || {
                let at = Instant::now();
                let rows = collector.read();
                (collector, Snapshot { at, rows })
            })
            .await
            {
                Ok((state, snapshot)) => {
                    collector = state;
                    if sender.send(snapshot).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    collector = Collector::default();
                    let _ = sender.send(Snapshot {
                        at: Instant::now(),
                        rows: unavailable_linux(
                            "Diagnostic worker failed; counters are restarting",
                        ),
                    });
                }
            }
        }
    });
    #[cfg(not(target_os = "linux"))]
    {
        drop(sender);
    }
    receiver
}

#[derive(Clone, Debug)]
#[cfg(any(target_os = "linux", test))]
struct Cpu([u64; 8]);
#[cfg(any(target_os = "linux", test))]
fn parse_cpu(text: &str) -> Option<Cpu> {
    let line = text.lines().find(|line| line.starts_with("cpu "))?;
    let fields = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(Cpu(fields.try_into().ok()?))
}
#[cfg(any(target_os = "linux", test))]
fn cpu_wait(before: Option<&Cpu>, after: &Cpu, elapsed: f64) -> Reading {
    let unavailable = || {
        Reading::unavailable(
            "%",
            "CPU I/O wait counters are warming up, reset, or separated by a collection gap",
        )
    };
    if !(0. < elapsed && elapsed <= MAX_GAP) {
        return unavailable();
    }
    let Some(before) = before else {
        return unavailable();
    };
    let Some(delta) = after
        .0
        .iter()
        .zip(before.0)
        .map(|(a, b)| a.checked_sub(b))
        .collect::<Option<Vec<_>>>()
    else {
        return unavailable();
    };
    let Some(total) = delta.iter().try_fold(0u64, |sum, n| sum.checked_add(*n)) else {
        return unavailable();
    };
    if total == 0 {
        return Reading::unavailable("%", "CPU accounting counters did not advance");
    }
    Reading::good(delta[4] as f64 / total as f64 * 100., "%")
}
#[cfg(any(target_os = "linux", test))]
fn pressure(text: &str) -> Option<f64> {
    let value = text
        .lines()
        .find(|line| line.starts_with("some "))?
        .split_whitespace()
        .find_map(|field| field.strip_prefix("avg10="))?
        .parse::<f64>()
        .ok()?;
    (value.is_finite() && (0. ..=100.).contains(&value)).then_some(value)
}
#[cfg(any(target_os = "linux", test))]
fn oom_counter(text: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next() == Some("oom_kill"))
            .then(|| words.next()?.parse().ok())
            .flatten()
    })
}
#[derive(Default)]
#[cfg(any(target_os = "linux", test))]
struct Oom {
    previous: Option<(f64, u64)>,
    events: VecDeque<(f64, u64)>,
}
#[cfg(any(target_os = "linux", test))]
impl Oom {
    fn sample(&mut self, now: f64, current: Option<u64>) -> Reading {
        let unavailable = || {
            Reading::unavailable(
                "count",
                "OOM counter is missing, warming up, reset, or separated by a collection gap",
            )
        };
        while self.events.front().is_some_and(|(at, _)| *at <= now - 300.) {
            self.events.pop_front();
        }
        let Some(counter) = current else {
            self.previous = None;
            return unavailable();
        };
        let previous = self.previous.replace((now, counter));
        let Some((before, value)) = previous else {
            return unavailable();
        };
        if now <= before || counter < value {
            self.events.clear();
            return unavailable();
        }
        if now - before > MAX_GAP {
            return unavailable();
        }
        if counter > value {
            self.events.push_back((now, counter - value));
        }
        while self.events.front().is_some_and(|(at, _)| *at <= now - 300.) {
            self.events.pop_front();
        }
        Reading::good(
            self.events.iter().map(|(_, count)| *count as f64).sum(),
            "count",
        )
    }
}
#[derive(Clone, Debug)]
#[cfg(any(target_os = "linux", test))]
struct Block {
    reads: u64,
    read_ms: u64,
    writes: u64,
    write_ms: u64,
    io_ms: u64,
}
#[cfg(any(target_os = "linux", test))]
fn parse_block(text: &str) -> Option<Block> {
    let values = text
        .split_whitespace()
        .take(11)
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 11).then(|| Block {
        reads: values[0],
        read_ms: values[3],
        writes: values[4],
        write_ms: values[7],
        io_ms: values[9],
    })
}
#[cfg(any(target_os = "linux", test))]
fn block_delta(before: Option<&Block>, after: &Block, elapsed: f64) -> (Reading, Reading) {
    let unavailable = || {
        (
            Reading::unavailable(
                "ms",
                "Disk counters are warming up, reset, or separated by a collection gap",
            ),
            Reading::unavailable(
                "%",
                "Disk counters are warming up, reset, or separated by a collection gap",
            ),
        )
    };
    if !(elapsed > 0. && elapsed <= MAX_GAP) {
        return unavailable();
    }
    let Some(before) = before else {
        return unavailable();
    };
    let Some(deltas) = [
        (after.reads, before.reads),
        (after.writes, before.writes),
        (after.read_ms, before.read_ms),
        (after.write_ms, before.write_ms),
        (after.io_ms, before.io_ms),
    ]
    .into_iter()
    .map(|(a, b)| a.checked_sub(b))
    .collect::<Option<Vec<_>>>() else {
        return unavailable();
    };
    let Some(ops) = deltas[0].checked_add(deltas[1]) else {
        return unavailable();
    };
    let Some(milliseconds) = deltas[2].checked_add(deltas[3]) else {
        return unavailable();
    };
    let latency = if ops == 0 {
        Reading::unavailable("ms", "No completed read/write operations in this interval")
    } else {
        Reading::good(milliseconds as f64 / ops as f64, "ms")
    };
    (
        latency,
        Reading::good((deltas[4] as f64 / (elapsed * 1000.) * 100.).min(100.), "%"),
    )
}
#[cfg(any(target_os = "linux", test))]
fn inode_percent(total: u64, free: u64) -> Reading {
    if total == 0 {
        Reading::unavailable("%", "Filesystem does not expose a fixed inode capacity")
    } else if free > total {
        Reading::unavailable("%", "Filesystem inode counters are inconsistent")
    } else {
        Reading::good(100. * (total - free) as f64 / total as f64, "%")
    }
}

#[cfg(target_os = "linux")]
fn bounded_read(path: impl AsRef<std::path::Path>, limit: usize) -> std::io::Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut bytes = vec![];
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::other("Kernel file exceeded read budget"));
    }
    String::from_utf8(bytes).map_err(std::io::Error::other)
}
#[cfg(target_os = "linux")]
fn devices() -> std::io::Result<Vec<String>> {
    let mut result = vec![];
    for entry in std::fs::read_dir("/sys/block")?.take(256).flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
            continue;
        }
        let Ok(path) = entry.path().canonicalize() else {
            continue;
        };
        if path.starts_with("/sys/devices/virtual") {
            continue;
        }
        if name.len() < 80
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
        {
            result.push(name);
        }
        if result.len() >= MAX_DEVICES {
            break;
        }
    }
    result.sort();
    Ok(result)
}
#[cfg(any(target_os = "linux", test))]
fn mounts(text: &str) -> Vec<String> {
    let mut result = vec![];
    for line in text.lines().take(4096) {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        // Do not stat potentially blocking network/FUSE mounts.
        let filesystem = right.split_whitespace().next().unwrap_or("");
        if ![
            "ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "zfs", "bcachefs", "overlay",
        ]
        .contains(&filesystem)
        {
            continue;
        }
        let Some(path) = left.split_whitespace().nth(4) else {
            continue;
        };
        let path = path
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\");
        if path.starts_with('/') && path.len() <= 300 && !result.contains(&path) {
            result.push(path);
        }
        if result.len() >= MAX_MOUNTS {
            break;
        }
    }
    result.sort();
    result
}
#[cfg(target_os = "linux")]
fn inode(path: &str) -> Reading {
    let Ok(path) = std::ffi::CString::new(path) else {
        return Reading::unavailable("%", "Invalid mount path");
    };
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated and stats points to writable statvfs storage.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Reading::unavailable(
            "%",
            &format!(
                "Cannot read filesystem inode counters: {}",
                std::io::Error::last_os_error()
            ),
        );
    }
    // SAFETY: a successful statvfs initialized the structure.
    let stats = unsafe { stats.assume_init() };
    inode_percent(stats.f_files, stats.f_ffree)
}
#[cfg(target_os = "linux")]
struct Collector {
    started: Instant,
    last: Option<f64>,
    wall: Option<f64>,
    cpu: Option<Cpu>,
    oom: Oom,
    blocks: HashMap<String, Block>,
}
#[cfg(target_os = "linux")]
impl Default for Collector {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            last: None,
            wall: None,
            cpu: None,
            oom: Oom::default(),
            blocks: HashMap::new(),
        }
    }
}
#[cfg(target_os = "linux")]
impl Collector {
    fn read(&mut self) -> Vec<Diagnostic> {
        let began = Instant::now();
        let now = self.started.elapsed().as_secs_f64();
        let elapsed = self.last.replace(now).map(|last| now - last).unwrap_or(0.);
        let utc = guard_core::now();
        let discontinuity = self
            .wall
            .replace(utc)
            .is_some_and(|wall| (utc - wall - elapsed).abs() >= 1.);
        if discontinuity {
            self.cpu = None;
            self.blocks.clear();
            self.oom = Oom::default();
        }
        let cpu = bounded_read("/proc/stat", 1024 * 1024)
            .ok()
            .and_then(|text| parse_cpu(&text));
        let waiting = cpu
            .as_ref()
            .map(|next| cpu_wait(self.cpu.as_ref(), next, elapsed))
            .unwrap_or_else(|| {
                Reading::unavailable("%", "Cannot read CPU accounting from /proc/stat")
            });
        self.cpu = cpu;
        let psi = bounded_read("/proc/pressure/memory", 4096)
            .ok()
            .and_then(|text| pressure(&text))
            .map(|v| Reading::good(v, "%"))
            .unwrap_or_else(|| {
                Reading::unavailable(
                    "%",
                    "Memory PSI is unavailable; kernel support or read permission is missing",
                )
            });
        let oom = self.oom.sample(
            now,
            bounded_read("/proc/vmstat", 128 * 1024)
                .ok()
                .and_then(|text| oom_counter(&text)),
        );
        let mut rows = vec![
            item("host", "cpu_iowait", "CPU I/O wait", waiting),
            item(
                "host",
                "memory_pressure",
                "Memory pressure · PSI some avg10",
                psi,
            ),
            item(
                "host",
                "oom_kills_5m",
                "Observed OOM kills · last 5 minutes",
                oom,
            ),
        ];
        match devices() {
            Ok(devices) => {
                if devices.is_empty() {
                    rows.push(item(
                        "host",
                        "disk_io_latency",
                        "Disk read/write latency",
                        Reading::unavailable(
                            "ms",
                            "No readable physical block devices were discovered",
                        ),
                    ));
                    rows.push(item(
                        "host",
                        "disk_io_utilization",
                        "Disk I/O busy time",
                        Reading::unavailable(
                            "%",
                            "No readable physical block devices were discovered",
                        ),
                    ));
                }
                self.blocks.retain(|name, _| devices.contains(name));
                for name in devices {
                    let (latency, busy) = if began.elapsed() < Duration::from_millis(200) {
                        let next = bounded_read(format!("/sys/block/{name}/stat"), 4096)
                            .ok()
                            .and_then(|text| parse_block(&text));
                        match next {
                            Some(next) => {
                                let values = block_delta(self.blocks.get(&name), &next, elapsed);
                                self.blocks.insert(name.clone(), next);
                                values
                            }
                            None => {
                                self.blocks.remove(&name);
                                (
                                    Reading::unavailable("ms", "Cannot read disk counters"),
                                    Reading::unavailable("%", "Cannot read disk counters"),
                                )
                            }
                        }
                    } else {
                        self.blocks.remove(&name);
                        (
                            Reading::unavailable("ms", "Diagnostic time budget reached"),
                            Reading::unavailable("%", "Diagnostic time budget reached"),
                        )
                    };
                    rows.push(item(
                        &format!("/dev/{name}"),
                        "disk_io_latency",
                        "Disk read/write latency",
                        latency,
                    ));
                    rows.push(item(
                        &format!("/dev/{name}"),
                        "disk_io_utilization",
                        "Disk I/O busy time",
                        busy,
                    ));
                }
            }
            Err(_) => {
                self.blocks.clear();
                rows.push(item(
                    "host",
                    "disk_io_latency",
                    "Disk read/write latency",
                    Reading::unavailable("ms", "Cannot discover physical block devices"),
                ));
                rows.push(item(
                    "host",
                    "disk_io_utilization",
                    "Disk I/O busy time",
                    Reading::unavailable("%", "Cannot discover physical block devices"),
                ));
            }
        }
        match bounded_read("/proc/self/mountinfo", 1024 * 1024) {
            Ok(text) => {
                let paths = mounts(&text);
                if paths.is_empty() {
                    rows.push(item(
                        "host",
                        "inode_used",
                        "Filesystem inodes used",
                        Reading::unavailable(
                            "%",
                            "No permitted local filesystem mounts were discovered",
                        ),
                    ));
                }
                for path in paths {
                    let reading = if began.elapsed() < Duration::from_millis(200) {
                        inode(&path)
                    } else {
                        Reading::unavailable("%", "Diagnostic time budget reached")
                    };
                    rows.push(item(&path, "inode_used", "Filesystem inodes used", reading));
                }
            }
            Err(_) => rows.push(item(
                "host",
                "inode_used",
                "Filesystem inodes used",
                Reading::unavailable("%", "Cannot discover permitted local filesystem mounts"),
            )),
        }
        if discontinuity {
            for row in &mut rows {
                row.reading = Reading::unavailable(
                    &row.reading.unit,
                    "Collection was interrupted or the system clock changed; counters are warming up",
                );
            }
        }
        rows
    }
}

pub fn default_rules() -> Vec<Rule> {
    #[cfg(target_os = "linux")]
    {
        rules_for_sources(
            devices().unwrap_or_default(),
            bounded_read("/proc/self/mountinfo", 1024 * 1024)
                .map(|text| mounts(&text))
                .unwrap_or_default(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        vec![]
    }
}
#[cfg(any(target_os = "linux", test))]
fn rules_for_sources(devices: Vec<String>, mounts: Vec<String>) -> Vec<Rule> {
    let mut rules = vec![];
    let mut append = |source: &str,
                      metric: &str,
                      label: &str,
                      threshold: f64,
                      window: u32,
                      severity: &str,
                      recovery: f64| {
        let short_source = source
            .char_indices()
            .take_while(|(index, _)| *index < 60)
            .map(|(_, c)| c)
            .collect::<String>();
        rules.push(Rule {
            id: format!(
                "diagnostic-v1-{}-{metric}-{severity}",
                &crate::auth::hash(source)[..16]
            ),
            name: format!("{label} · {short_source} · {severity}"),
            source: source.into(),
            metric: metric.into(),
            operator: ">".into(),
            threshold,
            window_seconds: window,
            fraction: if metric == "oom_kills_5m" { 1. } else { 0.8 },
            severity: severity.into(),
            recovery_threshold: recovery,
            recovery_seconds: if metric == "oom_kills_5m" { 30 } else { 120 },
            enabled: true,
            notify: true,
        });
    };
    for (metric, label, threshold, window, severity, recovery) in [
        (
            "swap_used_percent",
            "Sustained swap usage",
            80.,
            900,
            "warning",
            60.,
        ),
        ("memory_pressure", "Memory stalls", 10., 300, "warning", 5.),
        (
            "memory_pressure",
            "Severe memory stalls",
            25.,
            180,
            "critical",
            15.,
        ),
        ("cpu_iowait", "Sustained I/O wait", 20., 300, "warning", 10.),
        ("cpu_iowait", "Severe I/O wait", 40., 180, "critical", 20.),
        (
            "oom_kills_5m",
            "Out-of-memory kill observed",
            0.5,
            10,
            "critical",
            0.,
        ),
    ] {
        append("host", metric, label, threshold, window, severity, recovery);
    }
    {
        for device in devices {
            let source = format!("/dev/{device}");
            append(
                &source,
                "disk_io_latency",
                "Sustained disk latency",
                50.,
                300,
                "warning",
                25.,
            );
            append(
                &source,
                "disk_io_latency",
                "Severe disk latency",
                200.,
                180,
                "critical",
                100.,
            );
            append(
                &source,
                "disk_io_utilization",
                "Sustained disk busy time",
                90.,
                600,
                "warning",
                70.,
            );
        }
        for path in mounts {
            append(
                &path,
                "inode_used",
                "Inode capacity",
                85.,
                300,
                "warning",
                80.,
            );
            append(
                &path,
                "inode_used",
                "Critical inode capacity",
                95.,
                120,
                "critical",
                90.,
            );
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_excludes_guest_and_rejects_reset_missing_or_gap() {
        let old = parse_cpu("cpu 10 20 30 40 50 60 70 80 900 900\ncpu0 1 2").unwrap();
        let next = parse_cpu("cpu 20 30 40 50 60 70 80 90 99999 99999").unwrap();
        assert_eq!(cpu_wait(Some(&old), &next, 5.).value, Some(12.5));
        assert!(parse_cpu("cpu 1 2 3").is_none());
        assert!(cpu_wait(None, &next, 5.).value.is_none());
        assert!(cpu_wait(Some(&next), &old, 5.).value.is_none());
        assert!(cpu_wait(Some(&old), &next, 30.).value.is_none());
    }
    #[test]
    fn pressure_and_oom_require_named_valid_fields() {
        assert_eq!(
            pressure("full avg10=99.0\nsome avg10=2.55 avg60=1.00 total=20"),
            Some(2.55)
        );
        for text in ["full avg10=1", "some avg10=NaN", "some avg10=101"] {
            assert!(pressure(text).is_none());
        }
        assert_eq!(oom_counter("pgfault 123\noom_kill 4\n"), Some(4));
        assert_eq!(oom_counter("oom_kill_extra 9"), None);
    }
    #[test]
    fn oom_short_events_are_retained_but_reset_and_gaps_are_unknown() {
        let mut state = Oom::default();
        assert!(state.sample(0., Some(100)).value.is_none());
        assert_eq!(state.sample(5., Some(101)).value, Some(1.));
        for second in (10..305).step_by(5) {
            assert_eq!(state.sample(second as f64, Some(101)).value, Some(1.));
        }
        assert_eq!(state.sample(305., Some(101)).value, Some(0.));
        assert!(state.sample(310., Some(1)).value.is_none());
        assert!(state.sample(315., None).value.is_none());
        assert!(state.sample(320., Some(2)).value.is_none());
        assert!(state.sample(360., Some(3)).value.is_none());
    }
    #[test]
    fn block_latency_is_operation_weighted_and_idle_is_not_zero_latency() {
        let before = parse_block("100 0 0 400 200 0 0 800 0 1000 0").unwrap();
        let after = parse_block("110 0 0 600 220 0 0 900 0 3500 0 0 0").unwrap();
        let (latency, busy) = block_delta(Some(&before), &after, 5.);
        assert_eq!(latency.value, Some(10.));
        assert_eq!(busy.value, Some(50.));
        let (latency, busy) = block_delta(Some(&after), &after, 5.);
        assert!(latency.value.is_none());
        assert_eq!(busy.value, Some(0.));
        assert!(block_delta(Some(&after), &before, 5.).0.value.is_none());
        assert!(block_delta(None, &after, 5.).1.value.is_none());
        assert!(block_delta(Some(&before), &after, 30.).0.value.is_none());
        assert!(parse_block("1 2 3").is_none());
    }
    #[test]
    fn inode_and_swap_zero_capacity_are_unavailable() {
        assert_eq!(inode_percent(100, 15).value, Some(85.));
        assert!(inode_percent(0, 0).value.is_none());
        assert!(inode_percent(100, 101).value.is_none());
        assert!(swap(0, 0)[2].reading.value.is_none());
        assert_eq!(swap(100, 40)[2].reading.value, Some(40.));
        assert!(swap(100, 101)[2].reading.value.is_none());
    }
    #[test]
    fn mounted_network_and_fuse_filesystems_are_not_stat_probed() {
        let text = "1 0 8:1 / / rw - ext4 /dev/sda1 rw\n2 0 0:1 / /remote rw - nfs remote:/x rw\n3 0 0:2 / /fuse rw - fuse.sshfs remote rw\n4 0 8:2 / /with\\040space rw - xfs /dev/sdb rw";
        assert_eq!(mounts(text), vec!["/", "/with space"]);
    }
    #[test]
    fn diagnostic_rules_are_valid_and_do_not_repeat_ids() {
        let rules = rules_for_sources(
            vec!["nvme0n1".into()],
            vec!["/".into(), format!("/{}", "界".repeat(90))],
        );
        assert_eq!(rules.len(), 13);
        let mut ids = std::collections::HashSet::new();
        for rule in rules {
            rule.validate().unwrap();
            assert!(ids.insert(rule.id));
        }
    }
    #[test]
    fn missing_oom_read_does_not_erase_a_known_recent_kill() {
        let mut state = Oom::default();
        state.sample(0., Some(1));
        assert_eq!(state.sample(5., Some(2)).value, Some(1.));
        assert!(state.sample(10., None).value.is_none());
        assert!(state.sample(15., Some(2)).value.is_none());
        assert_eq!(state.sample(20., Some(2)).value, Some(1.));
    }
    #[test]
    fn one_oom_increment_fires_the_default_rule_and_recovers_after_expiry() {
        let rules = rules_for_sources(vec![], vec![]);
        let mut engine = guard_core::rules::Engine::default();
        let mut oom = Oom::default();
        let mut opened = 0;
        let mut recovered = 0;
        for second in (0..=340).step_by(5) {
            let value = oom.sample(second as f64, Some(if second == 0 { 10 } else { 11 }));
            for event in engine.evaluate(
                &rules,
                "host",
                "oom_kills_5m",
                second as f64,
                second as f64,
                value.value,
                5.,
            ) {
                if event.resolved_at.is_some() {
                    recovered += 1;
                } else {
                    opened += 1;
                    assert_eq!(event.severity, "critical");
                }
            }
        }
        assert_eq!((opened, recovered), (1, 1));
    }
}
