use super::*;
use serde_json::json;

#[test]
fn severity_recovery_and_legacy_reports_have_distinct_meanings() {
    for (payload, expected) in [
        (
            json!({"severity":"critical","title":"CPU above its limit"}),
            "Critical alert",
        ),
        (
            json!({"severity":"warning","title":"Disk filling up"}),
            "Warning",
        ),
        (
            json!({"kind":"recovery","severity":"critical","resolved":true,"title":"CPU above its limit"}),
            "Recovered",
        ),
        (
            json!({"severity":"critical","resolved":true,"title":"Existing queued recovery"}),
            "Recovered",
        ),
        (json!({"title":"Daily server report"}), "Scheduled summary"),
        (
            json!({"kind":"report","title":"Server summary"}),
            "Scheduled summary",
        ),
        (json!({"title":"Connection test"}), "Connection test"),
        (
            json!({"severity":"injected","title":"Update"}),
            "Server update",
        ),
    ] {
        let mail = notification(&payload, None);
        assert!(mail.subject.contains(expected));
        assert!(mail.html.contains(expected));
        assert!(mail.text.contains(expected));
        assert!(!mail.html.contains("href="));
    }
}

#[test]
fn custom_rule_titles_cannot_downgrade_warning_or_critical_messages() {
    for title in [
        "Connection test",
        "Daily server report",
        "Scheduled server summary",
        "Recovered CPU load",
    ] {
        for (severity, expected) in [("critical", "Critical alert"), ("warning", "Warning")] {
            for explicit in [None, Some("incident"), Some("test"), Some("report")] {
                let mut payload = json!({"title":title,"severity":severity});
                if let Some(explicit) = explicit {
                    payload["kind"] = json!(explicit);
                }
                let mail = notification(&payload, None);
                assert!(
                    mail.subject
                        .starts_with(&format!("[Metric Owl] {expected}:"))
                );
                assert!(mail.text.starts_with(&format!("{expected}:")));
                let html_status = format!(">{expected}</td>");
                assert!(mail.html.contains(&html_status));
            }
        }
    }
    assert!(notification(&json!({"kind":"incident","resolved":true,"title":"Recovered CPU load","severity":"critical"}),None).subject.starts_with("[Metric Owl] Critical alert:"));
    assert!(
        notification(
            &json!({"kind":"incident","title":"Connection test","severity":"info"}),
            None
        )
        .subject
        .starts_with("[Metric Owl] Server update:")
    );
    assert!(
        notification(
            &json!({"title":"Connection test","severity":"injected"}),
            None
        )
        .subject
        .starts_with("[Metric Owl] Server update:")
    );
}

#[test]
fn all_untrusted_content_is_escaped_bounded_and_redacted() {
    let payload = json!({
        "alias":"<img src=x onerror=alert(1)>\r\nBcc:other@example.com",
        "title":"<script>bad()</script> & \"quoted\"",
        "summary":"<a href='javascript:bad()'>click</a>\npassword=private-token Bearer abc.def sk-12345678901234567890",
        "recommendation":"</td><iframe src=x> API_KEY=provider-secret",
        "raw_logs":"private raw log must not be included",
        "secret":"never-use-this-field","HtmlBody":"<script>trusted?</script>",
        "panel_origin":"https://payload-controlled.example.com"
    });
    let mail = notification(&payload, None);
    assert!(mail.html.contains("&lt;img"));
    assert!(
        mail.html
            .contains("&lt;script&gt;bad()&lt;/script&gt; &amp; &quot;quoted&quot;")
    );
    for needle in [
        "<img",
        "<script",
        "<iframe",
        "<a href='javascript",
        "private-token",
        "abc.def",
        "12345678901234567890",
        "provider-secret",
        "private raw log",
        "never-use-this-field",
        "payload-controlled",
    ] {
        assert!(!mail.html.contains(needle), "{needle}");
    }
    assert!(!mail.subject.chars().any(char::is_control));
    assert!(!mail.text.contains("private-token"));
    assert!(mail.text.contains("<script>bad()</script>")); // text remains readable, not HTML entities.
    let worst = notification(
        &json!({"alias":"&".repeat(1000),"title":"\"".repeat(1000),"summary":"&".repeat(10000),"recommendation":"\"".repeat(10000)}),
        None,
    );
    assert!(worst.html.len() < 64 * 1024);
    assert!(worst.text.len() < 16 * 1024);
}

#[test]
fn panel_links_require_a_canonical_configured_https_origin() {
    for origin in [
        "http://panel.example.com",
        "javascript:alert(1)",
        "https://localhost",
        "https://test.localhost",
        "https://127.0.0.1",
        "https://[::1]",
        "https://[::ffff:127.0.0.1]",
        "https://0.0.0.0",
        "https://user:secret@panel.example.com",
        "https://panel.example.com/path",
        "https://panel.example.com/",
        "https://panel.example.com?token=secret",
        "https://panel.example.com#fragment",
        "https://panel.example.com\r\nBcc:bad",
        "https://panel.example.com:443",
        "https://panel.example.com\" onclick=\"bad",
        "data:text/html,bad",
        "https://singlelabel",
        "",
    ] {
        assert!(panel_url(Some(origin)).is_none(), "{origin}");
        assert!(
            !notification(&json!({}), Some(origin))
                .html
                .contains("href=")
        );
    }
    assert_eq!(
        panel_url(Some("https://panel.example.com:9443")).as_deref(),
        Some("https://panel.example.com:9443/admin")
    );
    let mail = notification(
        &json!({"panel_origin":"https://untrusted.example.com"}),
        Some("https://panel.example.com"),
    );
    assert!(
        mail.html
            .contains("href=\"https://panel.example.com/admin\"")
    );
    assert!(mail.text.contains("https://panel.example.com/admin"));
    assert!(!mail.html.contains("untrusted.example.com"));
}

#[test]
fn missing_fields_and_invalid_times_never_invent_healthy_readings() {
    let mail = notification(&json!({"at":-1,"alias":"\n"}), None);
    assert!(mail.text.contains("Your server"));
    assert!(mail.text.contains("No measurement summary was included"));
    assert!(mail.text.contains("Time: Not recorded"));
    assert!(!mail.text.contains("0.0%"));
    let mail = notification(
        &json!({"kind":"report","at":1793480943,"sample_at":null,"readings":{"cpu_percent":120,"memory_percent":null}}),
        None,
    );
    assert!(mail.text.contains("CPU usage: Not available"));
    assert!(mail.text.contains("Memory usage: Not available"));
    assert!(mail.text.contains("31 Oct 2026, 21:09:03 UTC"));
    assert!(mail.html.contains("not averages for the report period"));
    assert!(!mail.html.contains("120.0%"));
}

#[test]
fn report_values_and_plain_text_describe_the_same_measurements() {
    let mail = notification(
        &json!({"kind":"report","alias":"Example server","title":"Scheduled server summary","summary":"1 recent recorded incident remains open.","at":1793480943,"sample_at":1793480940,"readings":{"cpu_percent":42.75,"memory_percent":61.2}}),
        Some("https://panel.example.com"),
    );
    for value in [
        "Example server",
        "42.8%",
        "61.2%",
        "1 recent recorded incident remains open.",
        "31 Oct 2026, 21:09:00 UTC",
    ] {
        assert!(mail.text.contains(value));
        assert!(mail.html.contains(value));
    }
    for disallowed in ["<script", "<form", "<img", "@import", "<link", "url("] {
        assert!(!mail.html.contains(disallowed), "{disallowed}");
    }
    assert!(mail.html.contains("max-width:600px"));
    assert!(mail.html.contains("role=\"presentation\""));
}

#[test]
fn verification_is_fixed_and_has_no_invented_panel_link() {
    let mail = verification("123456").unwrap();
    assert!(mail.html.contains("123456"));
    assert_eq!(mail.subject, "Your Metric Owl verification code");
    assert!(mail.text.starts_with("Your verification code: 123456"));
    assert!(mail.text.ends_with("Valid for 10 minutes."));
    assert!(mail.text.contains("10 minutes"));
    assert!(!mail.html.contains("href="));
    for invalid in ["12", "1234567", "<12345", "12\n345", "１２３４５６"] {
        assert!(verification(invalid).is_err());
    }
}
