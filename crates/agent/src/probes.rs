use anyhow::{Result, ensure};
use guard_core::model::{Target, TargetOptions};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::IpAddr,
    time::{Duration, Instant},
};

pub struct Probe {
    pub status: String,
    pub detail: String,
    pub value: Option<f64>,
    pub measurements: BTreeMap<String, f64>,
}
impl Probe {
    pub fn basic((status, detail, value): (String, String, Option<f64>)) -> Self {
        Self {
            status,
            detail,
            value,
            measurements: BTreeMap::new(),
        }
    }
    fn failure(detail: &str) -> Self {
        Self::basic(("error".into(), detail.into(), Some(1.)))
    }
}

pub fn validate(options: &TargetOptions) -> Result<()> {
    ensure!(
        options
            .expected_status
            .is_none_or(|x| (100..=599).contains(&x)),
        "Invalid expected HTTP status"
    );
    ensure!(
        options
            .max_latency_ms
            .is_none_or(|x| (50..=8000).contains(&x)),
        "Response time must be 50–8000 ms"
    );
    ensure!(
        options
            .expected_text
            .as_ref()
            .is_none_or(|x| !x.is_empty() && x.len() <= 1024),
        "Expected text must be 1–1024 bytes"
    );
    ensure!(
        options
            .json_pointer
            .as_ref()
            .is_none_or(|x| x.len() <= 256 && (x.is_empty() || x.starts_with('/'))),
        "Use a JSON pointer, such as /status"
    );
    if let Some(expected) = &options.json_expected {
        ensure!(
            expected.len() <= 1024 && options.json_pointer.is_some(),
            "A JSON pointer is required"
        );
        serde_json::from_str::<serde_json::Value>(expected)?;
    }
    ensure!(
        options
            .job_interval_seconds
            .is_none_or(|x| (60..=31 * 86400).contains(&x)),
        "Job interval must be 1 minute–31 days"
    );
    ensure!(
        options.job_grace_seconds.is_none_or(|x| x <= 86400),
        "Job grace period must be at most one day"
    );
    ensure!(
        options
            .job_max_duration_seconds
            .is_none_or(|x| (60..=7 * 86400).contains(&x)),
        "Job runtime limit must be 1 minute–7 days"
    );
    Ok(())
}

#[derive(Debug)]
struct DnsFailure;
impl std::fmt::Display for DnsFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DNS lookup failed or timed out")
    }
}
impl std::error::Error for DnsFailure {}
fn tls_failure(detail: &str) -> Probe {
    let mut result = Probe::failure(detail);
    result.value = None; // A failed check cannot invent certificate expiry.
    result.measurements.insert("tls_failure".into(), 1.);
    result
}
async fn client(address: &str) -> Result<reqwest::Client> {
    let url = url::Url::parse(address)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Host is required"))?;
    // URL host_str retains brackets for IPv6; the resolver takes an unbracketed IP.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("Unsupported scheme"))?;
    let addresses: Vec<_> = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| DnsFailure)?
    .map_err(|_| DnsFailure)?
    .collect();
    ensure!(!addresses.is_empty(), DnsFailure);
    // Explicitly configured local application checks may use private/loopback IPs.
    // Metadata/link-local, unspecified and multicast addresses are never allowed.
    ensure!(
        !addresses.is_empty() && addresses.iter().all(|a| match a.ip() {
            IpAddr::V4(ip) =>
                crate::integrations::public_ip(ip.into()) || ip.is_private() || ip.is_loopback(),
            IpAddr::V6(ip) =>
                crate::integrations::public_ip(ip.into())
                    || ip.is_loopback()
                    || ip.is_unique_local(),
        }),
        "Metadata, link-local or unsupported network destinations are blocked"
    );
    Ok(reqwest::Client::builder()
        .no_proxy()
        .resolve_to_addrs(host, &addresses)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .tls_info(true)
        .build()?)
}

pub async fn http(target: &Target) -> Result<Probe> {
    let options = target.options.clone().unwrap_or_default();
    let client = match client(&target.address).await {
        Ok(client) => client,
        Err(error) if error.is::<DnsFailure>() => {
            return Ok(Probe::failure(
                "HTTP check failed: DNS lookup failed or timed out",
            ));
        }
        Err(error) => return Err(error),
    };
    let start = Instant::now();
    let mut response = match client.get(&target.address).send().await {
        Ok(response) => response,
        Err(_) => {
            return Ok(Probe::failure(
                "HTTP connection failed, TLS verification failed, or the request timed out",
            ));
        }
    };
    let status = response.status();
    let mut reasons = vec![];
    if !options
        .expected_status
        .map_or(status.is_success(), |expected| status.as_u16() == expected)
    {
        reasons.push(format!("Unexpected HTTP status {}", status.as_u16()));
    }
    if options.expected_text.is_some() || options.json_pointer.is_some() {
        let mut bytes = vec![];
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 65536 => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(Some(_)) => {
                    return Ok(Probe::failure(
                        "HTTP content check failed: response exceeds the 64 KiB inspection limit",
                    ));
                }
                Ok(None) => break,
                Err(_) => {
                    return Ok(Probe::failure(
                        "HTTP content check failed: response could not be read within the time limit",
                    ));
                }
            }
        }
        reasons.extend(content_failures(&options, &bytes));
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.;
    if options
        .max_latency_ms
        .is_some_and(|limit| elapsed > limit as f64)
    {
        reasons.push("Response time exceeded the configured limit".into());
    }
    let ok = reasons.is_empty();
    let detail = format!(
        "HTTP {} · {:.0} ms · {}",
        status.as_u16(),
        elapsed,
        if ok {
            "All configured checks passed".into()
        } else {
            reasons.join("; ")
        }
    );
    let mut result = Probe::basic((
        if ok { "ok" } else { "error" }.into(),
        detail,
        Some(if ok { 0. } else { 1. }),
    ));
    result.measurements.insert("http_latency".into(), elapsed);
    result
        .measurements
        .insert("http_status".into(), status.as_u16() as f64);
    Ok(result)
}

fn content_failures(options: &TargetOptions, bytes: &[u8]) -> Vec<String> {
    let mut failures = vec![];
    if options
        .expected_text
        .as_ref()
        .is_some_and(|expected| !String::from_utf8_lossy(bytes).contains(expected))
    {
        failures.push("Expected text was not found".into());
    }
    if let Some(pointer) = &options.json_pointer {
        match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(value) => match value.pointer(pointer) {
                None => failures.push("Expected JSON field was not found".into()),
                Some(actual) => {
                    if let Some(expected) = &options.json_expected
                        && serde_json::from_str::<serde_json::Value>(expected)
                            .ok()
                            .as_ref()
                            != Some(actual)
                    {
                        failures.push("JSON value did not match".into());
                    }
                }
            },
            Err(_) => failures.push("Response is not valid JSON".into()),
        }
    }
    failures
}

pub async fn tls(target: &Target) -> Result<Probe> {
    let client = match client(&target.address).await {
        Ok(client) => client,
        Err(error) if error.is::<DnsFailure>() => {
            return Ok(tls_failure(
                "Certificate check failed: DNS lookup failed or timed out",
            ));
        }
        Err(error) => return Err(error),
    };
    let response = match client.head(&target.address).send().await {
        Ok(response) => response,
        Err(_) => {
            return Ok(tls_failure(
                "Certificate could not be verified: connection, expiry, hostname or trust-chain failure. No certificate files were changed.",
            ));
        }
    };
    let der = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(|info| info.peer_certificate())
        .ok_or_else(|| anyhow::anyhow!("TLS peer certificate was unavailable"))?;
    let (_, certificate) = x509_parser::parse_x509_certificate(der)
        .map_err(|_| anyhow::anyhow!("TLS certificate could not be parsed"))?;
    let expires = certificate.validity().not_after.timestamp() as f64;
    let days = (expires - guard_core::now()) / 86400.;
    let date = chrono::DateTime::from_timestamp(expires as i64, 0)
        .ok_or_else(|| anyhow::anyhow!("Certificate expiry is invalid"))?
        .to_rfc3339();
    let mut result = Probe::basic((
        if days <= 7. {
            "critical"
        } else if days <= 30. {
            "warning"
        } else {
            "ok"
        }
        .into(),
        format!(
            "Verified certificate · expires {date} · {:.1} days remaining",
            days
        ),
        Some(days),
    ));
    result.measurements.insert("tls_days".into(), days);
    result.measurements.insert("tls_expires_at".into(), expires);
    result.measurements.insert("tls_failure".into(), 0.);
    Ok(result)
}

#[derive(Deserialize, Serialize)]
struct JobMarker {
    state: String,
    last_success_at: Option<f64>,
    started_at: Option<f64>,
}
fn read_job_marker(path: &std::path::Path) -> Result<JobMarker> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    // Validate the opened descriptor, so a replaced path cannot bypass the limit
    // or turn a regular-file check into a FIFO/symlink read.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= 16384,
        "Job report must be a regular file no larger than 16 KiB"
    );
    let mut bytes = Vec::new();
    file.take(16385).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16384, "Job report exceeds 16 KiB");
    Ok(serde_json::from_slice(&bytes)?)
}
pub async fn report_job(directory: &std::path::Path, id: &str, state: &str) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    uuid::Uuid::parse_str(id)?;
    // SAFETY: geteuid has no arguments and returns the process effective UID.
    ensure!(
        std::fs::metadata(directory)?.uid() == unsafe { libc::geteuid() },
        "Run report-job as the same OS account that owns the Metric Owl data directory"
    );
    ensure!(
        ["running", "success", "failure"].contains(&state),
        "Invalid report state"
    );
    let directory = directory.canonicalize()?;
    let path = directory.join("job-reports").join(format!("{id}.json"));
    let store = guard_core::store::Store::open(&directory.join("guard.sqlite"))?;
    let targets: Vec<Target> =
        serde_json::from_str(&store.get("targets").await?.unwrap_or("[]".into()))?;
    ensure!(
        targets.iter().any(|target| target.id == id
            && target.kind == "job"
            && target.enabled
            && std::path::Path::new(&target.address) == path),
        "No enabled managed job matches this ID"
    );
    let parent = path.parent().unwrap();
    if parent.exists() {
        ensure!(
            !std::fs::symlink_metadata(parent)?.file_type().is_symlink(),
            "Job report directory must not be a symlink"
        );
    }
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    let previous = match read_job_marker(&path) {
        Ok(marker) => Some(marker),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    let now = guard_core::now();
    let marker = JobMarker {
        state: state.into(),
        last_success_at: if state == "success" {
            Some(now)
        } else {
            previous.as_ref().and_then(|p| p.last_success_at)
        },
        started_at: if state == "running" {
            Some(now)
        } else {
            previous.and_then(|p| p.started_at)
        },
    };
    let temporary = parent.join(format!("{id}.{}.tmp", uuid::Uuid::new_v4()));
    crate::auth::private_file(&temporary, &serde_json::to_vec(&marker)?)?;
    if let Err(error) = std::fs::rename(&temporary, &path) {
        let _ = std::fs::remove_file(temporary);
        return Err(error.into());
    }
    println!("Job report recorded: {state}. No job was executed or modified.");
    Ok(())
}
pub async fn job(target: &Target) -> Result<Probe> {
    let options = target.options.clone().unwrap_or_default();
    let now = guard_core::now();
    let path = std::path::PathBuf::from(&target.address);
    let marker = match tokio::task::spawn_blocking(move || read_job_marker(&path)).await? {
        Ok(marker) => marker,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            let deadline = options.job_added_at.unwrap_or(now)
                + options.job_interval_seconds.unwrap_or(86400) as f64
                + options.job_grace_seconds.unwrap_or(3600) as f64;
            return Ok(if now > deadline {
                Probe::failure("No job report arrived before the expected deadline")
            } else {
                Probe::basic((
                    "pending".into(),
                    "Waiting for the first job report. No successful backup has been observed."
                        .into(),
                    None,
                ))
            });
        }
        Err(error) => return Err(error),
    };
    job_result(marker, &options, now)
}
fn job_result(marker: JobMarker, options: &TargetOptions, now: f64) -> Result<Probe> {
    ensure!(
        ["pending", "running", "success", "failure"].contains(&marker.state.as_str()),
        "Invalid job report state"
    );
    for timestamp in [marker.last_success_at, marker.started_at]
        .into_iter()
        .flatten()
    {
        ensure!(
            timestamp.is_finite() && timestamp >= 0. && timestamp <= now + 60.,
            "Invalid or future job timestamp"
        );
    }
    if marker.state == "failure" {
        return Ok(Probe::failure("The job reported a failure"));
    }
    if marker.state == "running" {
        let start = marker
            .started_at
            .ok_or_else(|| anyhow::anyhow!("Running job has no start time"))?;
        if now - start > options.job_max_duration_seconds.unwrap_or(3600) as f64 {
            return Ok(Probe::failure("The job exceeded its maximum runtime"));
        }
        return Ok(Probe::basic((
            "running".into(),
            format!("Job running for {:.0} seconds", now - start),
            Some(0.),
        )));
    }
    let last = marker
        .last_success_at
        .unwrap_or(options.job_added_at.unwrap_or(now));
    let age = now - last;
    if age
        > (options.job_interval_seconds.unwrap_or(86400)
            + options.job_grace_seconds.unwrap_or(3600)) as f64
    {
        return Ok(Probe::failure(
            "The expected successful job report is overdue",
        ));
    }
    let mut result = Probe::basic((
        if marker.last_success_at.is_some() {
            "ok"
        } else {
            "pending"
        }
        .into(),
        if marker.last_success_at.is_some() {
            format!(
                "Last success {:.0} seconds ago. A successful job report does not verify backup restorability.",
                age
            )
        } else {
            "Waiting for first successful job report".into()
        },
        marker.last_success_at.map(|_| 0.),
    ));
    result.measurements.insert("job_success_age".into(), age);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_marker_rejects_symlinks_and_oversized_files() {
        let directory =
            std::env::temp_dir().join(format!("metricowl-marker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("report.json");
        std::fs::write(
            &path,
            br#"{"state":"success","last_success_at":100,"started_at":90}"#,
        )
        .unwrap();
        assert_eq!(read_job_marker(&path).unwrap().state, "success");
        std::os::unix::fs::symlink(&path, directory.join("link.json")).unwrap();
        assert!(read_job_marker(&directory.join("link.json")).is_err());
        std::fs::write(&path, vec![b' '; 16385]).unwrap();
        assert!(read_job_marker(&path).is_err());
        assert!(read_job_marker(&directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[tokio::test]
    async fn real_http_probe_checks_status_content_latency_and_size() {
        use axum::{Router, routing::get};
        let app = Router::new()
            .route(
                "/ready",
                get(|| async { axum::Json(serde_json::json!({"ready":true})) }),
            )
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    "ready"
                }),
            )
            .route("/large", get(|| async { "x".repeat(65537) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut target = Target {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "http".into(),
            name: "Test fixture".into(),
            address: format!("http://{address}/ready"),
            enabled: true,
            options: Some(TargetOptions {
                json_pointer: Some("/ready".into()),
                json_expected: Some("true".into()),
                ..Default::default()
            }),
        };
        assert_eq!(http(&target).await.unwrap().status, "ok");
        target.options.as_mut().unwrap().json_expected = Some("false".into());
        assert_eq!(http(&target).await.unwrap().value, Some(1.));
        target.options = Some(TargetOptions {
            max_latency_ms: Some(50),
            ..Default::default()
        });
        target.address = format!("http://{address}/slow");
        assert!(
            http(&target)
                .await
                .unwrap()
                .detail
                .contains("Response time exceeded")
        );
        target.address = format!("http://{address}/missing");
        assert_eq!(
            http(&target).await.unwrap().measurements["http_status"],
            404.
        );
        target.address = format!("http://{address}/large");
        target.options = Some(TargetOptions {
            expected_text: Some("ready".into()),
            ..Default::default()
        });
        assert_eq!(http(&target).await.unwrap().value, Some(1.));
        server.abort();
    }
    #[test]
    fn http_checks_do_not_accept_an_error_page_with_status_200() {
        let mut options = TargetOptions {
            expected_text: Some("healthy".into()),
            ..Default::default()
        };
        assert!(!content_failures(&options, b"maintenance").is_empty());
        options.expected_text = None;
        options.json_pointer = Some("/status/ready".into());
        options.json_expected = Some("true".into());
        assert!(content_failures(&options, br#"{"status":{"ready":true}}"#).is_empty());
        assert!(!content_failures(&options, br#"{"status":{"ready":false}}"#).is_empty());
        assert!(!content_failures(&options, b"not json").is_empty());
    }
    #[tokio::test]
    async fn dns_failures_alert_but_blocked_destinations_remain_unavailable() {
        let mut target = Target {
            id: "dns".into(),
            kind: "http".into(),
            name: "DNS test".into(),
            address: "http://metric-owl-acceptance.invalid/".into(),
            enabled: true,
            options: None,
        };
        assert_eq!(http(&target).await.unwrap().value, Some(1.));
        target.address = "https://metric-owl-acceptance.invalid/".into();
        let result = tls(&target).await.unwrap();
        assert_eq!(result.value, None);
        assert_eq!(result.measurements.get("tls_failure"), Some(&1.));
        target.address = "http://169.254.169.254/".into();
        assert!(http(&target).await.is_err());
    }
    #[tokio::test]
    async fn ipv6_loopback_http_check_uses_unbracketed_dns_input() {
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new().route("/health", axum::routing::get(|| async { "healthy" })),
            )
            .await
            .unwrap();
        });
        let target = Target {
            id: "ipv6".into(),
            kind: "http".into(),
            name: "IPv6 test".into(),
            address: format!("http://{address}/health"),
            enabled: true,
            options: None,
        };
        let result = http(&target).await.unwrap();
        server.abort();
        assert_eq!(result.status, "ok");
    }
    #[test]
    fn job_success_missing_runtime_and_failure_are_distinct() {
        let options = TargetOptions {
            job_interval_seconds: Some(60),
            job_grace_seconds: Some(10),
            job_max_duration_seconds: Some(60),
            job_added_at: Some(0.),
            ..Default::default()
        };
        let marker = |state: &str, last, start| JobMarker {
            state: state.into(),
            last_success_at: last,
            started_at: start,
        };
        assert_eq!(
            job_result(marker("success", Some(100.), None), &options, 150.)
                .unwrap()
                .value,
            Some(0.)
        );
        assert_eq!(
            job_result(marker("success", Some(100.), None), &options, 171.)
                .unwrap()
                .value,
            Some(1.)
        );
        assert_eq!(
            job_result(marker("running", None, Some(100.)), &options, 150.)
                .unwrap()
                .status,
            "running"
        );
        assert_eq!(
            job_result(marker("running", None, Some(100.)), &options, 161.)
                .unwrap()
                .value,
            Some(1.)
        );
        assert_eq!(
            job_result(marker("failure", Some(150.), None), &options, 160.)
                .unwrap()
                .value,
            Some(1.)
        );
        assert!(job_result(marker("success", Some(999.), None), &options, 160.).is_err());
    }
}
