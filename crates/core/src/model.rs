use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Reading {
    pub value: Option<f64>,
    pub unit: String,
    pub quality: String,
    pub reason: Option<String>,
}
impl Reading {
    pub fn good(value: f64, unit: &str) -> Self {
        Self {
            value: value.is_finite().then_some(value),
            unit: unit.into(),
            quality: "ok".into(),
            reason: None,
        }
    }
    pub fn unavailable(unit: &str, reason: &str) -> Self {
        Self {
            value: None,
            unit: unit.into(),
            quality: "unavailable".into(),
            reason: Some(reason.into()),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Disk {
    pub id: String,
    pub mount: String,
    pub total: f64,
    pub available: f64,
    pub used_percent: Reading,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Process {
    pub id: String,
    pub name: String,
    pub pid: u32,
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Diagnostic {
    pub source: String,
    pub metric: String,
    pub label: String,
    pub reading: Reading,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Sample {
    pub source_id: String,
    pub at: f64,
    pub uptime: f64,
    pub cpu: Reading,
    pub cores: Vec<f64>,
    pub memory: Reading,
    pub memory_used: f64,
    pub memory_total: f64,
    pub network_in: Reading,
    pub network_out: Reading,
    pub disks: Vec<Disk>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    pub processes: Vec<Process>,
    pub agent_memory: f64,
    pub agent_cpu: f64,
}
impl Sample {
    pub fn metric(&self, name: &str, source: &str) -> Option<f64> {
        match name {
            "cpu" => self.cpu.value,
            "memory" => self.memory.value,
            "network_in" => self.network_in.value,
            "network_out" => self.network_out.value,
            "disk_used" => self
                .disks
                .iter()
                .find(|d| d.id == source)
                .and_then(|d| d.used_percent.value),
            "disk_free" => self
                .disks
                .iter()
                .find(|d| d.id == source)
                .map(|d| d.available),
            _ => self
                .diagnostics
                .iter()
                .find(|d| d.metric == name && d.source == source)
                .and_then(|d| d.reading.value),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Host {
    pub id: String,
    pub name: String,
    pub os: String,
    pub cpu_model: String,
    pub cores: u32,
    pub memory_total: f64,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub source: String,
    pub metric: String,
    pub operator: String,
    pub threshold: f64,
    pub window_seconds: u32,
    pub fraction: f64,
    pub severity: String,
    pub recovery_threshold: f64,
    pub recovery_seconds: u32,
    pub enabled: bool,
    pub notify: bool,
}
impl Rule {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.id.len() <= 80
                && !self.id.is_empty()
                && self.name.len() <= 160
                && self.source.len() <= 300,
            "Invalid rule ID"
        );
        anyhow::ensure!(
            [
                "cpu",
                "memory",
                "disk_used",
                "disk_free",
                "network_in",
                "network_out",
                "http",
                "service",
                "tls_days",
                "tls_failure",
                "job",
                "swap_used",
                "swap_total",
                "swap_used_percent",
                "memory_pressure",
                "cpu_iowait",
                "oom_kills_5m",
                "disk_io_latency",
                "disk_io_utilization",
                "inode_used"
            ]
            .contains(&self.metric.as_str()),
            "Invalid metric"
        );
        anyhow::ensure!(
            [">", ">=", "<", "<="].contains(&self.operator.as_str()),
            "Invalid comparison"
        );
        anyhow::ensure!(
            self.threshold.is_finite()
                && self.recovery_threshold.is_finite()
                && (0.1..=1.0).contains(&self.fraction)
                && (2..=3600).contains(&self.window_seconds)
                && (2..=3600).contains(&self.recovery_seconds),
            "Invalid condition"
        );
        anyhow::ensure!(
            ["warning", "critical"].contains(&self.severity.as_str()),
            "Invalid severity"
        );
        if self.operator.starts_with('>') {
            anyhow::ensure!(
                self.recovery_threshold < self.threshold,
                "Recovery threshold must be lower than the alert threshold"
            );
        } else {
            anyhow::ensure!(
                self.recovery_threshold > self.threshold,
                "Recovery threshold must be higher than the alert threshold"
            );
        }
        Ok(())
    }
}
pub fn default_rules() -> Vec<Rule> {
    [
        (
            "cpu-warning",
            "Sustained high CPU",
            "cpu",
            80.,
            300,
            "warning",
        ),
        (
            "cpu-critical",
            "Critical CPU load",
            "cpu",
            90.,
            180,
            "critical",
        ),
        (
            "memory-warning",
            "Memory pressure",
            "memory",
            90.,
            300,
            "warning",
        ),
        (
            "memory-critical",
            "Critical memory pressure",
            "memory",
            95.,
            180,
            "critical",
        ),
    ]
    .into_iter()
    .map(|(id, name, metric, threshold, window, severity)| Rule {
        id: id.into(),
        name: name.into(),
        metric: metric.into(),
        threshold,
        window_seconds: window,
        severity: severity.into(),
        source: "host".into(),
        operator: ">".into(),
        fraction: 0.8,
        recovery_threshold: threshold - 5.,
        recovery_seconds: 120,
        enabled: true,
        notify: true,
    })
    .collect()
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Incident {
    pub id: String,
    pub key: String,
    pub title: String,
    pub severity: String,
    pub started_at: f64,
    pub updated_at: f64,
    pub resolved_at: Option<f64>,
    pub evidence: String,
    pub recommendation: String,
    pub notify: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Aggregate {
    pub source: String,
    pub metric: String,
    pub start: f64,
    pub end: f64,
    pub resolution: u32,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub first: Option<f64>,
    pub last: Option<f64>,
    pub weighted_sum: f64,
    pub observed_seconds: f64,
    pub total: f64,
}
impl Aggregate {
    pub fn mean(&self) -> Option<f64> {
        (self.observed_seconds > 0.).then(|| self.weighted_sum / self.observed_seconds)
    }
    pub fn coverage(&self) -> f64 {
        (self.observed_seconds / (self.end - self.start).max(1.)).min(1.)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Target {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub address: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub options: Option<TargetOptions>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct TargetOptions {
    pub expected_status: Option<u16>,
    pub expected_text: Option<String>,
    pub json_pointer: Option<String>,
    pub json_expected: Option<String>,
    #[ts(type = "number | null")]
    pub max_latency_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub job_interval_seconds: Option<u64>,
    #[ts(type = "number | null")]
    pub job_grace_seconds: Option<u64>,
    #[ts(type = "number | null")]
    pub job_max_duration_seconds: Option<u64>,
    pub job_added_at: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct TargetStatus {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub at: f64,
    pub status: String,
    pub detail: String,
    pub value: Option<f64>,
    #[serde(default)]
    pub measurements: std::collections::BTreeMap<String, f64>,
}
#[cfg(test)]
mod diagnostics_tests {
    use super::*;
    #[test]
    fn old_samples_without_diagnostics_still_load_and_metric_sources_are_isolated() {
        let reading = serde_json::json!({"value":1.,"unit":"%","quality":"ok","reason":null});
        let old = serde_json::json!({"source_id":"old-host","at":1.,"uptime":2.,"cpu":reading,"cores":[],"memory":reading,"memory_used":1.,"memory_total":2.,"network_in":reading,"network_out":reading,"disks":[],"processes":[],"agent_memory":1.,"agent_cpu":0.});
        let mut sample: Sample = serde_json::from_value(old).unwrap();
        assert!(sample.diagnostics.is_empty());
        sample.diagnostics.push(Diagnostic {
            source: "/dev/sda".into(),
            metric: "disk_io_latency".into(),
            label: "Read/write latency".into(),
            reading: Reading::good(12., "ms"),
        });
        assert_eq!(sample.metric("disk_io_latency", "/dev/sda"), Some(12.));
        assert_eq!(sample.metric("disk_io_latency", "/dev/sdb"), None);
        let round_trip: Sample =
            serde_json::from_str(&serde_json::to_string(&sample).unwrap()).unwrap();
        assert_eq!(round_trip.diagnostics[0].reading.unit, "ms");
    }
}
