//! Bounded, escaped transactional emails shared by direct SMTP and the relay.
//! The renderer accepts monitoring summaries, never raw HTML, settings or logs.
use anyhow::{Result, ensure};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub struct Email {
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Critical,
    Warning,
    Recovery,
    Report,
    Test,
    Info,
    Verification,
}

impl Kind {
    fn appearance(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Critical => ("Critical alert", "#973c2c", "#fff0ea", "#e7b8a9"),
            Self::Warning => ("Warning", "#805313", "#fff5e1", "#e7cf9e"),
            Self::Recovery => ("Recovered", "#19553e", "#e3f4eb", "#b2d8c4"),
            Self::Report => ("Scheduled summary", "#335b77", "#eaf2f8", "#bed3e3"),
            Self::Test => ("Connection test", "#335b77", "#eaf2f8", "#bed3e3"),
            Self::Info => ("Server update", "#335b77", "#eaf2f8", "#bed3e3"),
            Self::Verification => ("Email verification", "#19553e", "#e3f4eb", "#b2d8c4"),
        }
    }
}

static KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:xai-|sk-|gsk_)[A-Za-z0-9_-]{12,}").expect("static key pattern")
});

fn clean(input: &str, limit: usize, multiline: bool) -> String {
    let text = crate::redact::redact(input, limit);
    let text = KEY.replace_all(&text, "[REDACTED]");
    text.chars()
        .map(|c| {
            if c.is_control() && !(multiline && c == '\n') {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn field(payload: &Value, key: &str, limit: usize) -> String {
    clean(payload[key].as_str().unwrap_or_default(), limit, true)
}

fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn paragraphs(input: &str) -> String {
    escape(input).replace('\n', "<br>")
}

fn timestamp(value: &Value) -> String {
    value
        .as_f64()
        .filter(|t| t.is_finite() && *t >= 0.)
        .and_then(|t| chrono::DateTime::from_timestamp(t.floor() as i64, 0))
        .map(|t| t.format("%d %b %Y, %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Not recorded".into())
}

/// A link is emitted only from the caller's configured canonical HTTPS origin.
/// Payload text/URLs never supply their own link, and loopback is not advertised.
pub fn panel_url(origin: Option<&str>) -> Option<String> {
    let origin = origin?;
    if origin.len() > 512 || origin.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(origin).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.origin().ascii_serialization() != origin
    {
        return None;
    }
    let invalid = match url.host()? {
        url::Host::Domain(host) => {
            host == "localhost"
                || host.ends_with(".localhost")
                || host == "localhost.localdomain"
                || !host.contains('.')
        }
        url::Host::Ipv4(ip) => ip.is_loopback() || ip.is_unspecified() || ip.is_multicast(),
        url::Host::Ipv6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v| v.is_loopback() || v.is_unspecified() || v.is_multicast())
        }
    };
    (!invalid).then(|| format!("{origin}/admin"))
}

fn kind(payload: &Value) -> Kind {
    let title = payload["title"].as_str().unwrap_or_default();
    if payload["kind"] == "recovery"
        || (payload.get("kind").is_none() && payload["resolved"] == true)
    {
        return Kind::Recovery;
    }
    // A custom rule name must never downgrade an alert to a test or report.
    match payload["severity"].as_str() {
        Some("critical") => return Kind::Critical,
        Some("warning") => return Kind::Warning,
        _ => {}
    }
    match payload["kind"].as_str() {
        Some("report") => Kind::Report,
        Some("test") => Kind::Test,
        Some(_) => Kind::Info,
        None if payload.get("kind").is_none()
            && matches!(payload["severity"].as_str(), None | Some("info")) =>
        {
            match title {
                "Daily server report" | "Scheduled server summary" => Kind::Report,
                "Connection test" => Kind::Test,
                _ => Kind::Info,
            }
        }
        None => Kind::Info,
    }
}

/// Unknown fields (credentials, logs, HTML and arbitrary URLs included) are ignored.
pub fn notification(payload: &Value, configured_origin: Option<&str>) -> Email {
    let kind = kind(payload);
    let alias = clean(payload["alias"].as_str().unwrap_or_default(), 100, false);
    let alias = if alias.is_empty() {
        "Your server"
    } else {
        &alias
    };
    let title = clean(payload["title"].as_str().unwrap_or_default(), 160, false);
    let title = if title.is_empty() {
        "Server notification"
    } else {
        &title
    };
    let summary = field(payload, "summary", 2048);
    let summary = if summary.is_empty() {
        "No measurement summary was included. Review the recorded evidence in your dashboard."
    } else {
        &summary
    };
    let recommendation = field(payload, "recommendation", 1024);
    let recommendation = if recommendation.is_empty() {
        "Review the recorded measurements before making changes to your server."
    } else {
        &recommendation
    };
    let at = timestamp(&payload["at"]);
    let mut rows = vec![];
    if kind == Kind::Report && payload["readings"].is_object() {
        for (key, name) in [
            ("cpu_percent", "CPU usage"),
            ("memory_percent", "Memory usage"),
        ] {
            let value = payload["readings"][key]
                .as_f64()
                .filter(|v| v.is_finite() && (0. ..=100.).contains(v))
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Not available".into());
            rows.push((name.to_string(), value));
        }
        rows.push(("Reading time".into(), timestamp(&payload["sample_at"])));
    }
    let label = if kind == Kind::Report {
        "Summary"
    } else if kind == Kind::Recovery {
        "Recovery evidence"
    } else {
        "Observed condition"
    };
    render(
        kind,
        alias,
        title,
        summary,
        recommendation,
        &at,
        label,
        &rows,
        panel_url(configured_origin).as_deref(),
        None,
    )
}

/// Verification is a deliberate recipient challenge, not an incident.
pub fn verification(code: &str) -> Result<Email> {
    ensure!(
        code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
        "Invalid verification code"
    );
    let mut email = render(
        Kind::Verification,
        "Your Metric Owl installation",
        "Verify your notification email",
        "Enter this code in the Metric Owl control center where you requested email verification.",
        "The code expires in 10 minutes and can be used once. If you did not request it, ignore this message. Never share the code with anyone.",
        "Valid for 10 minutes",
        "Confirm this email address",
        &[],
        None,
        Some(code),
    );
    // Preserve existing relay/plain-text challenge parsing while adding HTML.
    email.subject = "Your Metric Owl verification code".into();
    email.text = format!(
        "Your verification code: {code}\n\nEnter this code in your own Metric Owl control center. It can be used once. If you did not request it, ignore this message.\n\nValid for 10 minutes."
    );
    Ok(email)
}

#[allow(clippy::too_many_arguments)]
fn render(
    kind: Kind,
    alias: &str,
    title: &str,
    summary: &str,
    recommendation: &str,
    at: &str,
    label: &str,
    rows: &[(String, String)],
    panel: Option<&str>,
    code: Option<&str>,
) -> Email {
    let (status, color, tint, border) = kind.appearance();
    let mut text =
        format!("{status}: {title}\nServer: {alias}\nTime: {at}\n\n{label}\n{summary}\n");
    let mut readings = String::new();
    if !rows.is_empty() {
        text.push_str("\nLatest collected readings (not period averages)\n");
        readings.push_str("<h2 style=\"margin:24px 0 10px;font-size:15px;color:#213b32;\">Latest collected readings</h2><p style=\"margin:0 0 12px;font-size:12px;color:#52665d;\">These are the latest measurements, not averages for the report period.</p><table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"border-collapse:collapse;\">");
        for (label, value) in rows {
            text.push_str(&format!("{label}: {value}\n"));
            readings.push_str(&format!("<tr><td style=\"padding:11px 0;border-bottom:1px solid #dbe5dd;font-size:14px;color:#52665d;\">{}</td><td align=\"right\" style=\"padding:11px 0 11px 12px;border-bottom:1px solid #dbe5dd;font-size:14px;font-weight:bold;color:#213b32;\">{}</td></tr>",escape(label),escape(value)));
        }
        readings.push_str("</table>");
    }
    let code_html = if let Some(code) = code {
        text.push_str(&format!("\nVerification code: {code}\n"));
        format!(
            "<p style=\"margin:24px 0;padding:20px;text-align:center;border:1px solid #b2d8c4;border-radius:8px;background:#edf7f1;font-size:32px;letter-spacing:8px;font-weight:bold;color:#19553e;\">{code}</p>"
        )
    } else {
        String::new()
    };
    text.push_str(&format!("\nWhat to do next\n{recommendation}\n"));
    let action = if let Some(panel) = panel {
        text.push_str(&format!("\nOpen your dashboard: {panel}\n"));
        format!(
            "<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin-top:24px;\"><tr><td bgcolor=\"#19553e\" style=\"border-radius:6px;text-align:center;\"><a href=\"{}\" style=\"display:inline-block;padding:14px 22px;font-size:14px;font-weight:bold;color:#ffffff;text-decoration:none;\">Open your dashboard</a></td></tr></table>",
            escape(panel)
        )
    } else {
        "<p style=\"margin:24px 0 0;font-size:13px;color:#52665d;\">Open the dashboard address provided by your server administrator to review details.</p>".into()
    };
    text.push_str("\nMetric Owl · Your self-hosted server monitor\nNotifications are configured on your own server. Manage recipients and schedules in Notifications & connections.\n");
    let html = format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light"><title>{page_title}</title></head>
<body style="margin:0;padding:0;background:#edf2ed;color:#213b32;font-family:Arial,Helvetica,sans-serif;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;color:#edf2ed;">{preheader}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" bgcolor="#edf2ed" style="border-collapse:collapse;"><tr><td align="center" style="padding:24px 12px;">
<!--[if mso]><table role="presentation" width="600" cellpadding="0" cellspacing="0"><tr><td><![endif]-->
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="width:100%;max-width:600px;border-collapse:separate;border-spacing:0;">
<tr><td bgcolor="#112b22" style="padding:24px 28px;border-radius:12px 12px 0 0;"><p style="margin:0;color:#a9dfc6;font-size:23px;font-weight:bold;letter-spacing:-.5px;">Metric Owl</p><p style="margin:7px 0 0;color:#d2e3d8;font-size:11px;letter-spacing:1px;">YOUR SELF-HOSTED SERVER MONITOR</p></td></tr>
<tr><td bgcolor="#ffffff" style="padding:28px;border-left:1px solid #d5e1d8;border-right:1px solid #d5e1d8;">
<table role="presentation" cellpadding="0" cellspacing="0"><tr><td bgcolor="{tint}" style="padding:7px 11px;border:1px solid {border};border-radius:5px;color:{color};font-size:11px;font-weight:bold;letter-spacing:.4px;">{status}</td></tr></table>
<h1 style="margin:19px 0 12px;font-size:26px;line-height:1.25;letter-spacing:-.5px;color:#173d2d;overflow-wrap:anywhere;">{page_title}</h1>
<p style="margin:0;font-size:14px;line-height:1.6;color:#52665d;overflow-wrap:anywhere;"><strong style="color:#213b32;">{alias}</strong><br>{at}</p>
<div style="margin-top:25px;padding:18px;background:{tint};border-left:3px solid {color};border-radius:3px;">
<h2 style="margin:0 0 10px;font-size:12px;letter-spacing:.6px;color:{color};">{label}</h2><p style="margin:0;font-size:15px;line-height:1.7;color:#243e33;overflow-wrap:anywhere;word-break:break-word;">{summary}</p></div>
{code_html}{readings}
<h2 style="margin:24px 0 9px;font-size:15px;color:#213b32;">What to do next</h2><p style="margin:0;font-size:14px;line-height:1.75;color:#52665d;overflow-wrap:anywhere;word-break:break-word;">{recommendation}</p>
{action}
</td></tr><tr><td bgcolor="#f7faf7" style="padding:20px 28px;border:1px solid #d5e1d8;border-top:0;border-radius:0 0 12px 12px;"><p style="margin:0;color:#52665d;font-size:11px;line-height:1.8;">You receive this message because notifications are configured on your own server. Manage recipients and report times in <strong>Notifications &amp; connections</strong>. Metric Owl provides monitoring evidence; it does not run server commands.</p></td></tr>
</table><!--[if mso]></td></tr></table><![endif]-->
</td></tr></table></body></html>"##,
        page_title = escape(title),
        preheader = escape(&format!("{status} · {alias} · {title}")),
        status = escape(status),
        alias = escape(alias),
        at = escape(at),
        label = escape(label),
        summary = paragraphs(summary),
        recommendation = paragraphs(recommendation)
    );
    Email {
        subject: format!("[Metric Owl] {status}: {title} — {alias}"),
        text,
        html,
    }
}

#[cfg(test)]
mod tests;
