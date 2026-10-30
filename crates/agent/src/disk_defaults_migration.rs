//! Retire the original absolute-space default on small, currently detected
//! partitions. This changes monitoring metadata only, never filesystem contents.
use anyhow::Result;
use guard_core::{
    model::{Incident, Rule},
    store::Store,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::HashMap;
use sysinfo::Disks;

const RETIRE_CAPACITY_LIMIT: u64 = 10 * 1024 * 1024 * 1024;
const MARKER: &str = "small_partition_disk_free_defaults_v1";
const NOTE: &str = "default rule retired for small partition; no disk cleanup performed";

pub async fn run(store: &Store) -> Result<()> {
    if store.get(MARKER).await?.is_some() {
        return Ok(());
    }
    let partitions = Disks::new_with_refreshed_list()?
        .iter()
        .map(|disk| {
            (
                disk.mount_point().to_string_lossy().into_owned(),
                disk.total_space(),
            )
        })
        .collect::<HashMap<_, _>>();
    store
        .write(move |connection| migrate(connection, &partitions, guard_core::now()))
        .await
}

fn legacy_default(source: &str) -> Rule {
    // Frozen preview.1 signature: future default changes must not broaden this migration.
    Rule {
        id: format!(
            "disk-{}-disk_free-critical",
            &crate::auth::hash(source)[..24]
        ),
        name: format!("Disk {source} · Below 1 GiB"),
        source: source.into(),
        metric: "disk_free".into(),
        operator: "<".into(),
        threshold: 1073741824.,
        window_seconds: 30,
        fraction: 1.,
        severity: "critical".into(),
        recovery_threshold: 1395864371.,
        recovery_seconds: 30,
        enabled: true,
        notify: true,
    }
}

fn is_original_rule(value: &Value, rule: &Rule) -> Result<bool> {
    let expected = serde_json::to_value(legacy_default(&rule.source))?;
    // Compare every Rule field (including enabled, notify, all windows and names).
    // Unknown extension fields count as customization and are preserved as well.
    Ok(serde_json::to_value(rule)? == expected
        && value.as_object().map(|v| v.len()) == expected.as_object().map(|v| v.len()))
}

fn matches_original_incident(event: &Incident, retired: &Rule) -> bool {
    if event.resolved_at.is_some()
        || event.key != format!("{}:disk_free", retired.source)
        || event.title != retired.name
        || event.severity != retired.severity
        || !event.notify
        || event.recommendation != guard_core::rules::recommendation("disk_free")
    {
        return false;
    }
    // Incidents do not record rule IDs. Require the original engine evidence
    // shape, not just the source/metric key, before resolving one automatically.
    event.evidence.strip_prefix("disk_free: Some(")
        .and_then(|text|text.strip_suffix("); condition met for at least 100% of the 30-second window, with at least 90% data coverage."))
        .and_then(|value|value.parse::<f64>().ok())
        .is_some_and(|value|value.is_finite() && value>=0. && value<retired.threshold)
}

fn migrate(connection: &mut Connection, partitions: &HashMap<String, u64>, now: f64) -> Result<()> {
    let tx = connection.transaction()?;
    let already_done: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM kv WHERE key=?1)",
        [MARKER],
        |row| row.get(0),
    )?;
    if already_done {
        return Ok(());
    }
    let original: Option<String> = tx
        .query_row("SELECT value FROM kv WHERE key='rules'", [], |row| {
            row.get(0)
        })
        .optional()?;
    let mut retained = vec![];
    let mut retained_rules = vec![];
    let mut retired = vec![];
    if let Some(raw) = &original {
        for value in serde_json::from_str::<Vec<Value>>(raw)? {
            let rule: Rule = serde_json::from_value(value.clone())?;
            let small = partitions
                .get(&rule.source)
                .is_some_and(|total| *total > 0 && *total < RETIRE_CAPACITY_LIMIT);
            if small && is_original_rule(&value, &rule)? {
                retired.push(rule);
            } else {
                retained.push(value);
                retained_rules.push(rule);
            }
        }
    }
    if !retired.is_empty() {
        tx.execute(
            "UPDATE kv SET value=?1 WHERE key='rules'",
            [serde_json::to_string(&retained)?],
        )?;
        let mut cursor = 0i64;
        loop {
            // Bound memory without dropping history. Only active incidents are candidates.
            let rows: Vec<(i64, String)> = {
                let mut statement=tx.prepare("SELECT rowid,json FROM incidents WHERE rowid>?1 AND resolved IS NULL ORDER BY rowid LIMIT 100")?;
                statement
                    .query_map([cursor], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?
            };
            if rows.is_empty() {
                break;
            }
            for (rowid, raw) in rows {
                cursor = rowid;
                let mut value: Value = serde_json::from_str(&raw)?;
                let Ok(event) = serde_json::from_value::<Incident>(value.clone()) else {
                    continue;
                };
                let matching = retired
                    .iter()
                    .find(|rule| matches_original_incident(&event, rule));
                let Some(rule) = matching else {
                    continue;
                };
                // A surviving custom/disabled rule on the same key makes ownership
                // ambiguous; preserve that event for normal engine/user review.
                if retained_rules.iter().any(|remaining| {
                    remaining.source == rule.source && remaining.metric == rule.metric
                }) {
                    continue;
                }
                let at = now.max(event.started_at).max(event.updated_at);
                value["resolved_at"] = Value::from(at);
                value["updated_at"] = Value::from(at);
                value["notify"] = Value::Bool(false);
                value["evidence"] = Value::String(format!("{}; {NOTE}", event.evidence));
                tx.execute(
                    "UPDATE incidents SET updated=?1,resolved=?1,json=?2 WHERE rowid=?3",
                    params![at, serde_json::to_string(&value)?, rowid],
                )?;
            }
        }
    }
    tx.execute("INSERT INTO kv(key,value) VALUES(?1,'true')", [MARKER])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database(rules: Vec<Value>, events: Vec<Incident>) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE kv(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE incidents(id TEXT PRIMARY KEY,started REAL NOT NULL,updated REAL NOT NULL,resolved REAL,json TEXT NOT NULL);").unwrap();
        connection
            .execute(
                "INSERT INTO kv VALUES('rules',?1)",
                [serde_json::to_string(&rules).unwrap()],
            )
            .unwrap();
        for event in events {
            connection
                .execute(
                    "INSERT INTO incidents VALUES(?1,?2,?3,?4,?5)",
                    params![
                        event.id,
                        event.started_at,
                        event.updated_at,
                        event.resolved_at,
                        serde_json::to_string(&event).unwrap()
                    ],
                )
                .unwrap();
        }
        connection
    }
    fn saved_rules(connection: &Connection) -> Vec<Value> {
        serde_json::from_str(
            &connection
                .query_row::<String, _, _>("SELECT value FROM kv WHERE key='rules'", [], |row| {
                    row.get(0)
                })
                .unwrap(),
        )
        .unwrap()
    }
    fn event(rule: &Rule, id: &str) -> Incident {
        Incident {id:id.into(),key:format!("{}:{}",rule.source,rule.metric),title:rule.name.clone(),severity:rule.severity.clone(),started_at:1.,updated_at:2.,resolved_at:None,evidence:"disk_free: Some(536870912.0); condition met for at least 100% of the 30-second window, with at least 90% data coverage.".into(),recommendation:guard_core::rules::recommendation("disk_free").into(),notify:true}
    }
    fn saved_event(connection: &Connection, id: &str) -> Incident {
        serde_json::from_str(
            &connection
                .query_row::<String, _, _>("SELECT json FROM incidents WHERE id=?1", [id], |row| {
                    row.get(0)
                })
                .unwrap(),
        )
        .unwrap()
    }
    fn small() -> HashMap<String, u64> {
        HashMap::from([("/boot".into(), 1024 * 1024 * 1024)])
    }

    #[test]
    fn tiny_partitions_keep_percentage_defaults_and_large_partitions_keep_absolute_default() {
        let cutoff = crate::collect::ABSOLUTE_DISK_MIN_BYTES;
        for total in [0, 512 * 1024 * 1024, cutoff - 1] {
            let rules = crate::collect::disk_defaults_for_partition("/boot", total);
            assert_eq!(rules.len(), 2);
            assert!(rules.iter().all(|rule| rule.metric == "disk_used"));
            assert_eq!((rules[0].threshold, rules[1].threshold), (85., 95.));
        }
        for total in [cutoff, 100 * 1024 * 1024 * 1024] {
            let rules = crate::collect::disk_defaults_for_partition("/", total);
            assert_eq!(rules.len(), 3);
            assert_eq!(rules[2].threshold, 1073741824.);
        }
    }
    #[test]
    fn retires_exact_default_resolves_its_incident_and_keeps_history_idempotently() {
        let old = legacy_default("/boot");
        let other = legacy_default("/");
        let percentage = crate::collect::disk_defaults_for_partition("/boot", 1).remove(0);
        let original_events = vec![
            event(&old, "small"),
            event(&other, "root"),
            event(&percentage, "percentage"),
        ];
        let mut connection = database(
            vec![
                serde_json::to_value(&old).unwrap(),
                serde_json::to_value(&other).unwrap(),
                serde_json::to_value(&percentage).unwrap(),
            ],
            original_events.clone(),
        );
        migrate(&mut connection, &small(), 100.).unwrap();
        assert_eq!(
            saved_rules(&connection),
            vec![
                serde_json::to_value(other).unwrap(),
                serde_json::to_value(percentage).unwrap()
            ]
        );
        let resolved = saved_event(&connection, "small");
        assert_eq!(resolved.resolved_at, Some(100.));
        assert_eq!(resolved.started_at, 1.);
        assert!(!resolved.notify);
        assert!(resolved.evidence.ends_with(NOTE));
        for (id, index) in [("root", 1), ("percentage", 2)] {
            assert_eq!(
                serde_json::to_value(saved_event(&connection, id)).unwrap(),
                serde_json::to_value(&original_events[index]).unwrap()
            );
        }
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT count(*) FROM incidents", [], |row| row.get(0))
                .unwrap(),
            3
        );
        let before = serde_json::to_value(resolved).unwrap();
        migrate(&mut connection, &small(), 200.).unwrap();
        assert_eq!(
            serde_json::to_value(saved_event(&connection, "small")).unwrap(),
            before
        );
        // Explicit re-addition after migration is a user choice, not another default to retire.
        connection
            .execute(
                "UPDATE kv SET value=?1 WHERE key='rules'",
                [serde_json::to_string(&vec![old]).unwrap()],
            )
            .unwrap();
        migrate(&mut connection, &small(), 300.).unwrap();
        assert_eq!(saved_rules(&connection).len(), 1);
    }
    #[test]
    fn changing_any_original_rule_field_or_adding_an_extension_preserves_it() {
        let original = serde_json::to_value(legacy_default("/boot")).unwrap();
        for (field, value) in [
            ("id", Value::from("user-rule")),
            ("name", Value::from("My disk policy")),
            ("source", Value::from("/boot/efi")),
            ("metric", Value::from("disk_used")),
            ("operator", Value::from("<=")),
            ("threshold", Value::from(1000.)),
            ("window_seconds", Value::from(60)),
            ("fraction", Value::from(0.8)),
            ("severity", Value::from("warning")),
            ("recovery_threshold", Value::from(2000.)),
            ("recovery_seconds", Value::from(60)),
            ("enabled", Value::Bool(false)),
            ("notify", Value::Bool(false)),
            ("custom_note", Value::from("preserve this")),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            let mut connection = database(vec![changed.clone()], vec![]);
            migrate(&mut connection, &small(), 100.).unwrap();
            assert_eq!(
                saved_rules(&connection),
                vec![changed],
                "changed field: {field}"
            );
        }
    }
    #[test]
    fn absent_unknown_and_large_partitions_are_preserved() {
        let rule = serde_json::to_value(legacy_default("/boot")).unwrap();
        for partitions in [
            HashMap::new(),
            HashMap::from([("/boot".into(), 0)]),
            HashMap::from([("/boot".into(), crate::collect::ABSOLUTE_DISK_MIN_BYTES)]),
        ] {
            let mut connection = database(vec![rule.clone()], vec![]);
            migrate(&mut connection, &partitions, 100.).unwrap();
            assert_eq!(saved_rules(&connection), vec![rule.clone()]);
        }
    }
    #[test]
    fn customized_or_ambiguous_incidents_are_not_resolved() {
        let old = legacy_default("/boot");
        let mut custom = old.clone();
        custom.id = "custom-disk-policy".into();
        custom.name = "Custom disk rule".into();
        let incident = event(&old, "ambiguous");
        let mut connection = database(
            vec![
                serde_json::to_value(&old).unwrap(),
                serde_json::to_value(&custom).unwrap(),
            ],
            vec![incident.clone()],
        );
        migrate(&mut connection, &small(), 100.).unwrap();
        assert_eq!(
            serde_json::to_value(saved_event(&connection, "ambiguous")).unwrap(),
            serde_json::to_value(&incident).unwrap()
        );
        let mut changed = event(&old, "custom-title");
        changed.title = "Investigate this filesystem".into();
        let mut unrelated = event(&old, "custom-evidence");
        unrelated.evidence = "A user-defined observation".into();
        let mut resolved = event(&old, "already-resolved");
        resolved.resolved_at = Some(3.);
        let originals = vec![changed, unrelated, resolved];
        let mut connection = database(vec![serde_json::to_value(old).unwrap()], originals.clone());
        migrate(&mut connection, &small(), 100.).unwrap();
        for original in originals {
            assert_eq!(
                serde_json::to_value(saved_event(&connection, &original.id)).unwrap(),
                serde_json::to_value(original).unwrap()
            );
        }
    }
    #[test]
    fn migration_failure_rolls_back_rules_incidents_and_marker_together() {
        let old = legacy_default("/boot");
        let incident = event(&old, "small");
        let rules = vec![serde_json::to_value(&old).unwrap()];
        let mut connection = database(rules.clone(), vec![incident.clone()]);
        connection.execute_batch("CREATE TRIGGER fail_marker BEFORE INSERT ON kv WHEN NEW.key='small_partition_disk_free_defaults_v1' BEGIN SELECT RAISE(ABORT,'simulated marker failure'); END;").unwrap();
        assert!(migrate(&mut connection, &small(), 100.).is_err());
        assert_eq!(saved_rules(&connection), rules);
        assert_eq!(
            serde_json::to_value(saved_event(&connection, "small")).unwrap(),
            serde_json::to_value(incident).unwrap()
        );
        assert!(
            !connection
                .query_row::<bool, _, _>(
                    "SELECT EXISTS(SELECT 1 FROM kv WHERE key=?1)",
                    [MARKER],
                    |row| row.get(0)
                )
                .unwrap()
        );
    }
}
