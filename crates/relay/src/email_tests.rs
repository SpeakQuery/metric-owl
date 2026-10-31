use super::*;

struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn relay_send_ignores_an_unverified_panel_origin_from_the_notification() {
    let directory = TestDirectory(
        std::env::temp_dir().join(format!("metricowl-relay-email-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let store = Store::open(&directory.0.join("relay.sqlite")).unwrap();
    let key_hash = hash("synthetic-device-key");
    store.write(move|db| {
        db.execute_batch("CREATE TABLE devices(id TEXT PRIMARY KEY,name TEXT,hash TEXT UNIQUE,revoked INTEGER DEFAULT 0);CREATE TABLE recipients(device TEXT,email TEXT,verified INTEGER DEFAULT 0,code_hash TEXT,expires REAL,attempts INTEGER DEFAULT 0,PRIMARY KEY(device,email));")?;
        db.execute("INSERT INTO devices(id,name,hash) VALUES('fixture','Fixture',?1)",[key_hash])?;
        db.execute("INSERT INTO recipients(device,email,verified) VALUES('fixture','alerts@example.com',1)",[])?;
        Ok(())
    }).await.unwrap();
    let app = App {
        store: store.clone(),
        dir: directory.0.clone(),
        test: true,
        provider: Arc::new(provider::Provider::LocalTest),
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer synthetic-device-key".parse().unwrap(),
    );
    // Deserialize the real request contract, then execute the real send handler.
    let request:Message=serde_json::from_value(json!({"id":"safe-origin","recipient":"alerts@example.com","notification":{"alias":"Example server","severity":"critical","title":"Connection test","summary":"Recorded CPU condition exceeded its limit.","panel_origin":"https://attacker-controlled.example.com","HtmlBody":"<a href='https://attacker-controlled.example.com'>Open</a>"}})).unwrap();
    assert!(send(State(app), headers, Json(request)).await.is_ok());
    let payload: Value = store
        .read(|db| {
            let raw: String = db.query_row(
                "SELECT payload FROM outbox WHERE id='fixture:safe-origin'",
                [],
                |row| row.get(0),
            )?;
            Ok(serde_json::from_str(&raw)?)
        })
        .await
        .unwrap();
    assert!(
        payload["Subject"]
            .as_str()
            .unwrap()
            .starts_with("[Metric Owl] Critical alert:")
    );
    let html = payload["HtmlBody"].as_str().unwrap();
    assert!(!html.contains("href="));
    assert!(!html.contains("attacker-controlled"));
    assert!(
        !payload["TextBody"]
            .as_str()
            .unwrap()
            .contains("attacker-controlled")
    );
    assert_eq!(payload["To"], "alerts@example.com");
}
