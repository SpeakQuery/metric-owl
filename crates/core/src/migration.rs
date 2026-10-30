//! One-time translation of application-owned text saved by the early local build.
//! User names, paths, secrets, raw logs, identifiers and measurements are untouched.
use crate::store::Store;
use anyhow::Result;
use regex::Regex;
use rusqlite::params;
use serde_json::Value;
use std::{collections::BTreeMap, sync::LazyLock};

static CATALOG: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("legacy_messages.json")).expect("valid migration catalog")
});
static SLOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^{}]*\}").unwrap());
static TEMPLATES: LazyLock<Vec<(Regex, String, bool)>> = LazyLock::new(|| {
    let mut messages: Vec<_> = CATALOG.iter().collect();
    messages.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
    messages
        .into_iter()
        .filter(|(old, _)| old.contains('{'))
        .map(|(old, new)| {
            let mut pattern = String::from("(?s)^");
            let mut end = 0;
            for m in SLOT.find_iter(old) {
                pattern.push_str(&regex::escape(&old[end..m.start()]));
                pattern.push_str("(.*?)");
                end = m.end();
            }
            pattern.push_str(&regex::escape(&old[end..]));
            pattern.push('$');
            (
                Regex::new(&pattern).unwrap(),
                new.clone(),
                new.contains("condition met for at least"),
            )
        })
        .collect()
});
fn translate(text: &str) -> String {
    if let Some(value) = CATALOG.get(text) {
        return value.clone();
    }
    // Built-in sensitive-rule labels may wrap another built-in name.
    if let Some(rest) = text.strip_prefix("Hassas · ") {
        return format!("Sensitive · {}", translate(rest));
    }
    if text.starts_with("Disk ")
        && let Some(rest) = text.strip_suffix(" · 1 GiB altı")
    {
        return format!("{rest} · Below 1 GiB");
    }
    for (pattern, template, reorder) in TEMPLATES.iter() {
        if let Some(captures) = pattern.captures(text) {
            let mut i = 0;
            return SLOT
                .replace_all(template, |_: &regex::Captures<'_>| {
                    i += 1;
                    let index = if *reorder {
                        match i {
                            3 => 4,
                            4 => 3,
                            _ => i,
                        }
                    } else {
                        i
                    };
                    captures
                        .get(index)
                        .map(|m| translate(m.as_str()))
                        .unwrap_or_default()
                })
                .into_owned();
        }
    }
    text.to_owned()
}
fn translate_json(value: &mut Value, rule_names: bool) {
    match value {
        Value::Array(items) => {
            for item in items {
                translate_json(item, rule_names);
            }
        }
        Value::Object(items) => {
            for (key, item) in items {
                if [
                    "title",
                    "evidence",
                    "recommendation",
                    "reason",
                    "summary",
                    "error",
                    "Subject",
                    "TextBody",
                ]
                .contains(&key.as_str())
                    || (rule_names && key == "name")
                {
                    if let Some(text) = item.as_str() {
                        *item = Value::String(translate(text));
                    }
                } else if item.is_array() || item.is_object() {
                    translate_json(item, rule_names);
                }
            }
        }
        _ => {}
    }
}
pub async fn english(store: &Store) -> Result<()> {
    if store.get("english_text_v1").await?.is_some() {
        return Ok(());
    }
    store.write(|c| {
        let tx = c.transaction()?;
        // Fixed identifiers only. Batches bound memory for existing installations.
        for (table, column) in [("incidents", "json"), ("details", "json"), ("outbox", "payload")] {
            let mut cursor = 0i64;
            loop {
                let rows: Vec<(i64, String)> = {
                    let mut s = tx.prepare(&format!("SELECT rowid,{column} FROM {table} WHERE rowid>?1 ORDER BY rowid LIMIT 100"))?;
                    s.query_map([cursor], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
                };
                if rows.is_empty() { break; }
                for (id, raw) in rows {
                    cursor = id;
                    let mut value: Value = serde_json::from_str(&raw)?;
                    let before = value.clone();
                    translate_json(&mut value, false);
                    if value != before { tx.execute(&format!("UPDATE {table} SET {column}=?1 WHERE rowid=?2"), params![value.to_string(), id])?; }
                }
            }
        }
        let rows: Vec<(String, String)> = {
            let mut s = tx.prepare("SELECT key,value FROM kv WHERE key IN ('rules','settings','retention_warning')")?;
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (key, raw) in rows {
            let translated = if key == "retention_warning" { translate(&raw) } else {
                let mut value: Value = serde_json::from_str(&raw)?;
                if key == "rules" { translate_json(&mut value, true); }
                if key == "settings" && value["alias"] == "Sunucum" { value["alias"] = Value::String("My server".into()); }
                value.to_string()
            };
            if translated != raw { tx.execute("UPDATE kv SET value=?1 WHERE key=?2", params![translated, key])?; }
        }
        let errors: Vec<(String, String)> = {
            let mut s = tx.prepare("SELECT id,error FROM outbox WHERE error IS NOT NULL")?;
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (id, error) in errors { let translated = translate(&error); if translated != error { tx.execute("UPDATE outbox SET error=?1 WHERE id=?2", params![translated,id])?; } }
        tx.execute("INSERT INTO kv(key,value) VALUES('english_text_v1','true')", [])?;
        tx.commit()?;
        Ok(())
    }).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn translates_saved_builtins_preserves_user_data_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("test.sqlite")).unwrap();
        store.put("rules", r#"[{"name":"CPU uzun süre yüksek","source":"/Sunucum"},{"name":"Custom rule","source":"host"}]"#.into()).await.unwrap();
        store
            .put(
                "settings",
                r#"{"alias":"Sunucum","recipient":"owner@example.invalid"}"#.into(),
            )
            .await
            .unwrap();
        store
            .put("admin", "private auth value".into())
            .await
            .unwrap();
        store.write(|c| { c.execute("INSERT INTO incidents VALUES('i',1,1,NULL,?1)", [r#"{"title":"Kısa CPU sıçraması kaydedildi","evidence":"CPU 95.0% — alarm süresi dolmadığı için e-posta gönderilmedi.","recommendation":"Olay çevresindeki süreçleri ve geçmişi inceleyin."}"#])?; Ok(()) }).await.unwrap();
        english(&store).await.unwrap();
        english(&store).await.unwrap();
        let rules: Value =
            serde_json::from_str(&store.get("rules").await.unwrap().unwrap()).unwrap();
        assert_eq!(rules[0]["name"], "Sustained high CPU");
        assert_eq!(rules[0]["source"], "/Sunucum");
        assert_eq!(rules[1]["name"], "Custom rule");
        assert_eq!(
            store.get("admin").await.unwrap().unwrap(),
            "private auth value"
        );
        let evidence: String = store
            .read(|c| Ok(c.query_row("SELECT json FROM incidents WHERE id='i'", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert!(evidence.contains("CPU 95.0% — no email sent"));
        assert_eq!(
            translate(
                "cpu: Some(95.0); 180 saniyelik pencerede en az %80 koşul, en az %90 veri kapsamı."
            ),
            "cpu: Some(95.0); condition met for at least 80% of the 180-second window, with at least 90% data coverage."
        );
    }
}
