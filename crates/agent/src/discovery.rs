//! Bounded inventory of known web config directories. Never executes or rewrites config.
use crate::App;
use guard_core::model::Process;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Default, Serialize)]
pub struct Inventory {
    pub sites: Vec<Site>,
    pub services: Vec<Service>,
    pub site_notes: Vec<String>,
    pub sites_checked_at: Option<f64>,
    pub services_checked_at: Option<f64>,
}
#[derive(Clone, Serialize)]
pub struct Site {
    pub hostname: String,
    pub web_server: String,
    pub config_file: String,
    pub wildcard: bool,
}
#[derive(Clone, Serialize)]
pub struct Service {
    pub name: String,
    pub kind: String,
    pub process_count: usize,
    pub cpu_percent: Option<f64>,
    pub memory_rss_bytes: Option<f64>,
    pub memory_note: String,
    pub database_details: Option<String>,
}

pub fn services(processes: &[Process]) -> Vec<Service> {
    let mut grouped: BTreeMap<(&str, &str), Vec<&Process>> = BTreeMap::new();
    for process in processes {
        let name = process.name.to_lowercase();
        let kind = match name.as_str() {
            "nginx" => Some(("Nginx", "web")),
            "apache2" | "httpd" => Some(("Apache", "web")),
            "postgres" | "postmaster" => Some(("PostgreSQL", "database")),
            "mysqld" => Some(("MySQL", "database")),
            "mariadbd" => Some(("MariaDB", "database")),
            "mongod" => Some(("MongoDB", "database")),
            "redis-server" => Some(("Redis", "database")),
            "node" | "nodejs" => Some(("Node.js", "application")),
            "gunicorn" | "uvicorn" => Some(("Python application server", "application")),
            "dockerd" => Some(("Docker engine", "container")),
            "containerd" => Some(("containerd", "container")),
            n if n.starts_with("php-fpm") => Some(("PHP-FPM", "application")),
            _ => None,
        };
        if let Some(kind) = kind {
            grouped.entry(kind).or_default().push(process);
        }
    }
    grouped.into_iter().map(|((name,kind),processes)|Service {
        name:name.into(),kind:kind.into(),process_count:processes.len(),
        cpu_percent:processes.iter().map(|p|p.cpu).collect::<Option<Vec<_>>>().map(|v|v.iter().sum()),
        memory_rss_bytes:processes.iter().map(|p|p.memory).collect::<Option<Vec<_>>>().map(|v|v.iter().sum()),
        memory_note:"Sum of process RSS; shared pages may be counted more than once. CPU is measured per core.".into(),
        database_details:(kind=="database").then(||"Engine detected from running processes. Database names, sizes and connection counts require a separately configured read-only database connection; none is configured.".into()),
    }).collect()
}

fn hostname(value: &str) -> Option<(String, bool)> {
    let name = value
        .trim_matches(['\'', '"', ';'])
        .trim_end_matches('.')
        .to_lowercase();
    let wildcard = name.starts_with("*.");
    let check = name.strip_prefix("*.").unwrap_or(&name);
    if check.len() > 253
        || !check.contains('.')
        || check.split('.').any(|part| {
            part.is_empty()
                || part.len() > 63
                || part.starts_with('-')
                || part.ends_with('-')
                || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
    {
        return None;
    }
    Some((name, wildcard))
}
fn declarations(text: &str, server: &str) -> Vec<(String, bool)> {
    let mut names = vec![];
    if server == "Nginx" {
        let clean = text
            .lines()
            .map(|line| line.split('#').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        for statement in clean.split(';') {
            // A block boundary can precede the first directive.
            let tokens: Vec<_> = statement
                .rsplit(['{', '}'])
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if tokens.first() == Some(&"server_name") {
                names.extend(tokens.iter().skip(1).filter_map(|token| hostname(token)));
            }
        }
    } else {
        for line in text.lines() {
            let tokens: Vec<_> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if tokens.first().is_some_and(|t| {
                t.eq_ignore_ascii_case("ServerName") || t.eq_ignore_ascii_case("ServerAlias")
            }) {
                names.extend(tokens.iter().skip(1).filter_map(|token| hostname(token)));
            }
        }
    }
    names
}

fn scan() -> (Vec<Site>, Vec<String>) {
    let mut sites = BTreeMap::new();
    let mut notes = vec![];
    let start = Instant::now();
    let mut count = 0;
    for (root, server) in [
        ("/etc/nginx", "Nginx"),
        ("/etc/apache2", "Apache"),
        ("/etc/httpd", "Apache"),
    ] {
        let base = Path::new(root);
        if !base.exists() {
            continue;
        }
        let mut stack = vec![base.to_path_buf()];
        while let Some(path) = stack.pop() {
            count += 1;
            if count > 500 || start.elapsed() > Duration::from_secs(2) {
                notes
                    .push("Discovery stopped at its file/time budget; results are partial.".into());
                return (sites.into_values().collect(), notes);
            }
            let canonical = match path.canonicalize() {
                Ok(p) if p.starts_with(base) => p,
                _ => continue,
            };
            if canonical.is_dir() {
                match std::fs::read_dir(&canonical) {
                    Ok(entries) => stack.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                        !p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                    })),
                    Err(_) => notes.push(format!("Cannot read {}", canonical.display())),
                }
                continue;
            }
            // Read textual config only; never private keys or certificate stores.
            if canonical.extension().is_some_and(|ext| ext != "conf") {
                continue;
            }
            let metadata = match std::fs::metadata(&canonical) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !metadata.is_file() || metadata.len() > 256 * 1024 {
                continue;
            }
            match std::fs::read_to_string(&canonical) {
                Ok(text) => {
                    for (name, wildcard) in declarations(&text, server) {
                        sites
                            .entry((name.clone(), server.to_owned()))
                            .or_insert(Site {
                                hostname: name,
                                web_server: server.into(),
                                config_file: canonical.to_string_lossy().into(),
                                wildcard,
                            });
                    }
                }
                Err(_) => notes.push(format!("Cannot read configuration {}", canonical.display())),
            }
        }
    }
    if sites.is_empty() {
        notes.push("No hostname declarations found in readable standard Nginx/Apache directories. This does not prove that no websites are hosted here; containers and custom configurations may need manual sources.".into());
    }
    notes.push("Configuration declarations are suggestions. Enable monitoring explicitly; DNS and certificate ownership are not inferred.".into());
    (sites.into_values().collect(), notes)
}

pub fn start(app: App) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match tokio::task::spawn_blocking(scan).await {
                Ok((sites, notes)) => {
                    let mut inventory = app.inventory.write().await;
                    inventory.sites = sites;
                    inventory.site_notes = notes;
                    inventory.sites_checked_at = Some(guard_core::now());
                }
                Err(error) => {
                    app.error(format!("Hosted-site discovery unavailable: {error}"))
                        .await
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_hostname_directives_are_discovered() {
        let nginx = "server {\n server_name example.com\n api.example.com *.example.com;\n # server_name fake.invalid;\n ssl_certificate_key /private/key.pem;\n}";
        assert_eq!(
            declarations(nginx, "Nginx"),
            vec![
                ("example.com".into(), false),
                ("api.example.com".into(), false),
                ("*.example.com".into(), true)
            ]
        );
        assert_eq!(
            declarations(
                "ServerName example.com\nServerAlias www.example.com\n# ServerName ignored.test",
                "Apache"
            )
            .len(),
            2
        );
        assert!(hostname("$variable").is_none());
        assert!(hostname("../../secret").is_none());
    }
}
