use crate::model::{Incident, Rule};
use std::collections::{HashMap, VecDeque};

#[derive(Clone)]
struct Point {
    end: f64,
    duration: f64,
    value: Option<f64>,
}
#[derive(Default)]
pub struct Window {
    points: VecDeque<Point>,
    last: Option<f64>,
}
#[derive(Debug, PartialEq)]
pub struct Evaluation {
    pub coverage: f64,
    pub matching_seconds: f64,
    pub fires: bool,
}
impl Window {
    pub fn push(&mut self, now: f64, value: Option<f64>, cadence: f64) {
        if let Some(last) = self.last {
            let delta = now - last;
            // Never turn sleep, restart, negative time, or stale readings into healthy time.
            if delta <= 0. || delta > cadence * 2.5 {
                self.points.clear();
            } else {
                self.points.push_back(Point {
                    end: now,
                    duration: if cadence > 1. {
                        cadence.min(delta)
                    } else {
                        delta
                    },
                    value: value.filter(|v| v.is_finite()),
                });
            }
        }
        if self.last.is_none() && cadence > 1. {
            self.points.push_back(Point {
                end: now,
                duration: cadence,
                value: value.filter(|v| v.is_finite()),
            });
        }
        self.last = Some(now);
        while self.points.front().is_some_and(|p| p.end < now - 7200.) {
            self.points.pop_front();
        }
    }
    pub fn evaluate(
        &self,
        now: f64,
        seconds: u32,
        threshold: f64,
        operator: &str,
        fraction: f64,
    ) -> Evaluation {
        let start = now - seconds as f64;
        let mut observed = 0.;
        let mut matching = 0.;
        for p in &self.points {
            let duration = (p.end.min(now) - (p.end - p.duration).max(start)).max(0.);
            if let Some(v) = p.value {
                observed += duration;
                if compare(v, threshold, operator) {
                    matching += duration;
                }
            }
        }
        let coverage = observed / seconds as f64;
        Evaluation {
            coverage,
            matching_seconds: matching,
            fires: coverage >= 0.9
                && matching
                    + if self.points.back().is_some_and(|p| p.duration > 2.) {
                        0.1
                    } else {
                        1e-9
                    }
                    >= seconds as f64 * fraction,
        }
    }
}
pub fn compare(v: f64, t: f64, op: &str) -> bool {
    match op {
        ">" => v > t,
        ">=" => v >= t,
        "<" => v < t,
        "<=" => v <= t,
        _ => false,
    }
}
#[derive(Default)]
pub struct Engine {
    windows: HashMap<String, Window>,
    active: HashMap<String, Incident>,
    rule_active: HashMap<String, bool>,
}
impl Engine {
    pub fn restore(&mut self, incidents: Vec<Incident>) {
        for i in incidents {
            if i.resolved_at.is_none() && i.severity != "info" {
                self.active.insert(i.key.clone(), i);
            }
        }
    }
    // Keep monotonic and UTC clocks explicit at the only engine ingestion boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate(
        &mut self,
        rules: &[Rule],
        source: &str,
        metric: &str,
        mono: f64,
        utc: f64,
        value: Option<f64>,
        cadence: f64,
    ) -> Vec<Incident> {
        let key = format!("{source}:{metric}");
        let window = self.windows.entry(key.clone()).or_default();
        window.push(mono, value, cadence);
        let relevant: Vec<_> = rules
            .iter()
            .filter(|r| {
                r.enabled
                    && r.metric == metric
                    && (r.source == source || (r.source == "host" && source == "host"))
            })
            .collect();
        let mut firing: Vec<&Rule> = vec![];
        let tracked = relevant
            .iter()
            .any(|r| self.rule_active.contains_key(&r.id));
        let active_severity = self.active.get(&key).map(|i| i.severity.clone());
        let mut restored_recovered = false;
        for r in &relevant {
            let eval =
                window.evaluate(mono, r.window_seconds, r.threshold, &r.operator, r.fraction);
            if eval.fires {
                self.rule_active.insert(r.id.clone(), true);
            }
            let reverse = if r.operator.starts_with('>') {
                "<="
            } else {
                ">="
            };
            let recovered = window
                .evaluate(mono, r.recovery_seconds, r.recovery_threshold, reverse, 1.0)
                .fires;
            if recovered && self.rule_active.contains_key(&r.id) {
                self.rule_active.insert(r.id.clone(), false);
            }
            if recovered && active_severity.as_deref() == Some(r.severity.as_str()) {
                restored_recovered = true;
            }
            if self.rule_active.get(&r.id) == Some(&true) {
                firing.push(r);
            }
        }
        firing.sort_by_key(|r| if r.severity == "critical" { 0 } else { 1 });
        if let Some(r) = firing.first() {
            if let Some(i) = self.active.get_mut(&key) {
                if i.severity == "warning" && r.severity == "critical" {
                    i.severity = "critical".into();
                    i.title = r.name.clone();
                    i.notify = r.notify;
                    i.evidence = format!(
                        "{}: {:?}; severity escalated to critical, threshold {} {}",
                        metric, value, r.operator, r.threshold
                    );
                    i.updated_at = utc;
                    return vec![i.clone()];
                }
            } else {
                let i = Incident {
                    id: uuid::Uuid::new_v4().to_string(),
                    key: key.clone(),
                    title: r.name.clone(),
                    severity: r.severity.clone(),
                    started_at: utc,
                    updated_at: utc,
                    resolved_at: None,
                    evidence: format!(
                        "{}: {:?}; condition met for at least {:.0}% of the {}-second window, with at least 90% data coverage.",
                        metric,
                        value,
                        r.fraction * 100.,
                        r.window_seconds
                    ),
                    recommendation: recommendation(metric).into(),
                    notify: r.notify,
                };
                self.active.insert(key, i.clone());
                return vec![i];
            }
        } else if (tracked || restored_recovered)
            && !relevant.is_empty()
            && let Some(mut i) = self.active.remove(&key)
        {
            i.resolved_at = Some(utc);
            i.updated_at = utc;
            return vec![i];
        }
        vec![]
    }
}
pub fn recommendation(metric: &str) -> &str {
    match metric {
        "cpu" => {
            "Review the most CPU-intensive processes and recent changes. A single spike does not establish a capacity shortage."
        }
        "memory" => {
            "Check process memory trends and swap usage; growth alone is not evidence of a memory leak."
        }
        "swap_used" | "swap_total" | "swap_used_percent" => {
            "Compare swap use with memory pressure and application latency. Used swap alone does not establish a current memory shortage; do not disable swap to clear an alert."
        }
        "memory_pressure" => {
            "Linux memory PSI measures time when tasks stall for memory. Review process growth, working sets and workload changes before adjusting capacity."
        }
        "cpu_iowait" => {
            "Correlate I/O wait with device latency, I/O busy time and application activity. Linux I/O wait is an approximate CPU accounting signal, not proof of a failing disk."
        }
        "oom_kills_5m" => {
            "The kernel recorded an out-of-memory kill. Review the system journal and memory limits to identify the affected process; the counter alone does not identify the victim or distinguish host and cgroup causes."
        }
        "disk_io_latency" => {
            "Review the device workload and application latency. This is read/write time per completed operation, not end-user response time; compare with throughput and recent changes."
        }
        "disk_io_utilization" => {
            "The device was busy for much of the observed interval. Review latency and workload together; busy time alone does not prove saturation on parallel devices such as NVMe."
        }
        "inode_used" => {
            "File creation can fail when inodes run out even with free bytes remaining. Inspect directories with many small files and retention policies; verify backups before removing anything."
        }
        "disk_used" | "disk_free" => {
            "Review growing log and upload directories. Verify backups before deleting files."
        }
        "http" => {
            "Check application logs, network access, the response time and configured content checks."
        }
        "tls_days" => {
            "Renew the certificate through your existing certificate manager and verify the full certificate chain. Metric Owl does not change certificates."
        }
        "tls_failure" => {
            "Check DNS, connectivity, certificate expiry, hostname matching and certificate trust. Metric Owl never disables TLS verification."
        }
        "job" => {
            "Inspect the scheduled job logs, its last successful run and reporting marker. A job success signal alone does not prove that a backup can be restored."
        }
        _ => "Check service status and logs around the incident.",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_143_144_boundary() {
        let mut w = Window::default();
        w.push(0., Some(0.), 1.);
        for n in 1..=180 {
            w.push(n as f64, Some(if n <= 143 { 99. } else { 0. }), 1.);
        }
        assert!(!w.evaluate(180., 180, 90., ">", 0.8).fires);
        let mut w = Window::default();
        w.push(0., Some(0.), 1.);
        for n in 1..=180 {
            w.push(n as f64, Some(if n <= 144 { 99. } else { 0. }), 1.);
        }
        assert!(w.evaluate(180., 180, 90., ">", 0.8).fires);
    }
    #[test]
    fn missing_is_not_healthy() {
        let mut w = Window::default();
        w.push(0., None, 1.);
        for n in 1..=180 {
            w.push(n as f64, if n <= 160 { Some(99.) } else { None }, 1.);
        }
        assert!(!w.evaluate(180., 180, 90., ">", 0.8).fires);
    }
    #[test]
    fn sleep_resets_window() {
        let mut w = Window::default();
        for n in 0..=200 {
            w.push(n as f64, Some(99.), 1.);
        }
        assert!(w.evaluate(200., 180, 90., ">", 0.8).fires);
        w.push(230., Some(99.), 1.);
        assert!(!w.evaluate(230., 180, 90., ">", 0.8).fires);
    }
    #[test]
    fn escalation_and_recovery_one_incident() {
        let rules = crate::model::default_rules();
        let mut e = Engine::default();
        let mut events = vec![];
        for n in 0..=500 {
            events.extend(e.evaluate(
                &rules,
                "host",
                "cpu",
                n as f64,
                n as f64,
                Some(if n < 340 { 85. } else { 99. }),
                1.,
            ));
        }
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, events[1].id);
        for n in 501..=650 {
            events.extend(e.evaluate(&rules, "host", "cpu", n as f64, n as f64, Some(30.), 1.));
        }
        assert_eq!(events.len(), 3);
        assert!(events[2].resolved_at.is_some());
    }
}
#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn recovery_only_requires_fired_rules() {
        let mut e = Engine::default();
        let rules = crate::model::default_rules();
        let mut events = vec![];
        for n in 0..=320 {
            events.extend(e.evaluate(
                &rules,
                "host",
                "cpu",
                n as f64,
                n as f64,
                Some(if n <= 180 { 95. } else { 80. }),
                1.,
            ));
        }
        assert_eq!(events.len(), 2);
        assert!(events[1].resolved_at.is_some());
    }
    #[test]
    fn disk_fires_after_two_checks() {
        let mut w = Window::default();
        w.push(0., Some(95.), 15.);
        assert!(!w.evaluate(0., 30, 90., ">", 1.).fires);
        w.push(15., Some(95.), 15.);
        assert!(w.evaluate(15., 30, 90., ">", 1.).fires);
    }
}
