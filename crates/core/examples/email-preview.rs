//! Synthetic fixtures only. This example writes files; it never sends email.
use guard_core::email::{self, Email};
use serde_json::json;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let directory = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("artifacts/validation/email-templates"));
    std::fs::create_dir_all(&directory)?;
    let common = json!({"alias":"Example server","at":1793480943,"title":"Sustained high CPU","severity":"warning","summary":"CPU exceeded 80% during at least 4 minutes of a 5-minute window. The recorded condition has persisted long enough to raise an alert.","recommendation":"Review the processes and performance history around this interval. Compare the rise with expected scheduled jobs before making changes."});
    let mut fixtures: Vec<(&str, Email)> = vec![];
    fixtures.push((
        "warning",
        email::notification(&common, Some("https://panel.example.com")),
    ));
    let mut critical = common.clone();
    critical["severity"] = json!("critical");
    critical["title"] = json!("Main disk is almost full");
    critical["summary"] = json!(
        "Main disk usage is 96.2%, above the configured 95% limit for 30 seconds. Low free space can interrupt applications."
    );
    critical["recommendation"] = json!(
        "Review the disk's recorded usage and your retention policy. Confirm which application owns large files before taking action."
    );
    fixtures.push((
        "critical",
        email::notification(&critical, Some("https://panel.example.com")),
    ));
    let mut recovered = common.clone();
    recovered["resolved"] = json!(true);
    recovered["kind"] = json!("recovery");
    recovered["summary"] = json!(
        "CPU returned below the configured recovery threshold for 2 minutes. The recorded incident is now resolved."
    );
    recovered["recommendation"] = json!(
        "No immediate action is indicated by this recovered condition. Review the event history if the pattern repeats."
    );
    fixtures.push((
        "recovery",
        email::notification(&recovered, Some("https://panel.example.com")),
    ));
    fixtures.push(("summary",email::notification(&json!({"kind":"report","alias":"Example server","title":"Scheduled server summary","at":1793480943,"sample_at":1793480940,"summary":"Recent recorded incidents still open: 1. The readings below describe the latest collected sample.","readings":{"cpu_percent":34.1,"memory_percent":58.2},"recommendation":"Review the open incident and compare earlier measurements in your dashboard. Unmonitored services and unmeasured time remain unknown."}),Some("https://panel.example.com"))));
    fixtures.push(("verification", email::verification("123456")?));
    fixtures.push(("test",email::notification(&json!({"kind":"test","alias":"Example server","title":"Connection test","at":1793480943,"summary":"Metric Owl test notification.","recommendation":"This is a connection test, not an alert. A provider accepting this message is separate from confirming that it reached your inbox."}),None)));
    let mut index = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Metric Owl email previews</title><body style=\"font:16px Arial;background:#edf2ed;color:#213b32;padding:28px\"><h1>Metric Owl email previews</h1><p>Synthetic example data. No email was sent.</p><ul>",
    );
    for (name, content) in fixtures {
        std::fs::write(directory.join(format!("{name}.html")), content.html)?;
        std::fs::write(directory.join(format!("{name}.txt")), content.text)?;
        index.push_str(&format!("<li><a href=\"{name}.html\">{name} HTML</a> · <a href=\"{name}.txt\">plain text</a></li>"));
    }
    index.push_str("</ul></body></html>");
    std::fs::write(directory.join("index.html"), index)?;
    println!(
        "Synthetic email previews written to {}. No email was sent.",
        directory.display()
    );
    Ok(())
}
