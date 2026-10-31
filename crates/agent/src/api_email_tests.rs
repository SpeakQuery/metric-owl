//! Isolated real-router acceptance. No monitoring worker or external send runs.
use crate::{App, Settings, api, auth, smtp, web_access};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request},
};
use guard_core::{model::Host, store::Store};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tokio::sync::RwLock;
use tower::ServiceExt;

const HOST: &str = "203.0.113.10:8443";
const ORIGIN: &str = "https://203.0.113.10:8443";
const PROXY: &str = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";
const PASSWORD: &str = "isolated-password-only-test";
const SMTP_PASSWORD: &str = "smtp-private-fixture-credential";

#[tokio::test]
async fn mcp_permission_edits_require_session_csrf_and_preserve_key_identity() {
    let dir = tempfile::tempdir().unwrap();
    let app = fixture(dir.path()).await;
    let router = api::router(app.clone());
    let id = "c53d79dd-2584-46aa-984d-6949797b033a";
    app.store.write(move|db|{db.execute("INSERT INTO mcp_clients(id,name,hash,scopes) VALUES(?1,'fixture-client','unchanged-hash','[\"history\"]')",[id])?;Ok(())}).await.unwrap();
    let path = format!("mcp/clients/{id}");
    assert_eq!(
        call(
            &router,
            Method::PUT,
            &path,
            Some(json!({"scopes":["history","metric_report"]})),
            "",
            ""
        )
        .await
        .0,
        401
    );
    let (_, headers, login) = call(
        &router,
        Method::POST,
        "auth/login",
        Some(json!({"username":"owner","password":PASSWORD})),
        "",
        "",
    )
    .await;
    let cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let csrf = login["session"]["csrf"].as_str().unwrap();
    assert_eq!(
        call(
            &router,
            Method::PUT,
            &path,
            Some(json!({"scopes":["history","metric_report"]})),
            cookie,
            ""
        )
        .await
        .0,
        403
    );
    assert_eq!(
        call(
            &router,
            Method::PUT,
            &path,
            Some(json!({"scopes":["execute"]})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    assert_eq!(
        call(
            &router,
            Method::PUT,
            &path,
            Some(json!({"scopes":["history","metric_report"]})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    let (hash, scopes) = app
        .store
        .read(move |db| {
            Ok(db.query_row(
                "SELECT hash,scopes FROM mcp_clients WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(hash, "unchanged-hash");
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&scopes).unwrap(),
        vec!["history", "metric_report"]
    );
    app.store
        .write(move |db| {
            db.execute("UPDATE mcp_clients SET revoked=1 WHERE id=?1", [id])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        call(
            &router,
            Method::PUT,
            &path,
            Some(json!({"scopes":["history"]})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
}

#[tokio::test]
async fn browser_session_survives_reload_retention_and_elapsed_time_until_logout() {
    let dir = tempfile::tempdir().unwrap();
    let app = fixture(dir.path()).await;
    let router = api::router(app.clone());
    let (_, _, anonymous) = call(&router, Method::GET, "auth/status", None, "", "").await;
    assert!(anonymous["session"].is_null());
    assert_eq!(
        call(&router, Method::GET, "security", None, "", "").await.0,
        401
    );
    let (status, headers, login) = call(
        &router,
        Method::POST,
        "auth/login",
        Some(json!({"username":"owner","password":PASSWORD})),
        "",
        "",
    )
    .await;
    assert_eq!(status, 200);
    let raw = headers["set-cookie"].to_str().unwrap();
    assert!(raw.starts_with("__Host-guard_session="));
    for attribute in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(raw.contains(attribute));
    }
    for attribute in ["Max-Age", "Expires", "Domain="] {
        assert!(!raw.contains(attribute));
    }
    let cookie = raw.split(';').next().unwrap();
    let csrf = login["session"]["csrf"].as_str().unwrap();
    // Cross the former 5-minute verification, 30-minute idle and 12-hour limits.
    app.store
        .write(|db| {
            db.execute(
                "UPDATE sessions SET created=?1,last=?1,step_up=0",
                [guard_core::now() - 7. * 86400.],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    app.store.retain(guard_core::now()).await.unwrap();

    // A new panel load restores the session and its original CSRF token. It also
    // replaces a legacy persistent cookie with a browser-session cookie.
    let (status, refreshed_headers, refreshed) =
        call(&router, Method::GET, "auth/status", None, cookie, "").await;
    assert_eq!(status, 200);
    assert_eq!(refreshed["session"]["csrf"], csrf);
    assert_eq!(refreshed_headers["set-cookie"], headers["set-cookie"]);
    let (status, _, security) = call(&router, Method::GET, "security", None, cookie, "").await;
    assert_eq!(status, 200);
    assert_eq!(security["sessions"].as_array().unwrap().len(), 1);

    let settings = app.settings.read().await.clone();
    for token in ["", "csrf-from-another-session"] {
        assert_eq!(
            call(
                &router,
                Method::PUT,
                "settings",
                Some(json!({"settings":settings})),
                cookie,
                token,
            )
            .await
            .0,
            403
        );
    }
    // Matching credentials and CSRF still cannot authorize a cross-origin write.
    let evil = router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/v1/settings")
                .header("host", HOST)
                .header("origin", "https://unrelated.example")
                .header("x-forwarded-proto", "https")
                .header("x-guard-proxy-token", PROXY)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("content-type", "application/json")
                .body(Body::from(json!({"settings":settings}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(evil.status().as_u16(), 403);
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":settings})),
            cookie,
            csrf,
        )
        .await
        .0,
        200
    );
    assert_eq!(
        call(&router, Method::POST, "auth/logout", None, cookie, "")
            .await
            .0,
        403
    );
    let (status, logout_headers, _) =
        call(&router, Method::POST, "auth/logout", None, cookie, csrf).await;
    assert_eq!(status, 200);
    assert!(
        logout_headers["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        call(&router, Method::GET, "security", None, cookie, "")
            .await
            .0,
        401
    );
    let (_, headers, status) = call(&router, Method::GET, "auth/status", None, cookie, "").await;
    assert!(status["session"].is_null());
    assert!(!headers.contains_key("set-cookie"));
}

#[tokio::test]
async fn ai_connection_requires_session_csrf_and_matching_provider_credential() {
    let dir = tempfile::tempdir().unwrap();
    let app = fixture(dir.path()).await;
    let router = api::router(app.clone());
    assert_eq!(
        call(&router, Method::GET, "ai/providers", None, "", "")
            .await
            .0,
        401
    );
    let (_, headers, login) = call(
        &router,
        Method::POST,
        "auth/login",
        Some(json!({"username":"owner","password":PASSWORD})),
        "",
        "",
    )
    .await;
    let cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let csrf = login["session"]["csrf"].as_str().unwrap();
    let (_, _, catalog) = call(&router, Method::GET, "ai/providers", None, cookie, "").await;
    assert_eq!(catalog["providers"].as_array().unwrap().len(), 6);
    assert_eq!(catalog["selected_provider"], "xai");
    assert_eq!(catalog["verified"], false);
    let request = json!({"provider":"openai", "key":"synthetic-key"});
    assert_eq!(
        call(
            &router,
            Method::POST,
            "ai/connect",
            Some(request),
            cookie,
            ""
        )
        .await
        .0,
        403
    );
    for body in [
        json!({"provider":"unknown","key":"synthetic-key"}),
        json!({"provider":"openai","key":"bad\nkey"}),
    ] {
        let result = call(
            &router,
            Method::POST,
            "ai/connect",
            Some(body),
            cookie,
            csrf,
        )
        .await;
        assert_eq!(result.0, 400);
        assert!(!result.2.to_string().contains("synthetic-key"));
    }
    assert!(app.store.get("secret:ai").await.unwrap().is_none());
    app.store
        .put("secret:ai", app.auth.seal("previous-provider-key").unwrap())
        .await
        .unwrap();
    let saved = app.store.get("secret:ai").await.unwrap();
    assert_eq!(
        call(
            &router,
            Method::POST,
            "ai/connect",
            Some(json!({"provider":"openai"})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    let mut settings = app.settings.read().await.clone();
    settings.ai_base_url = "https://api.openai.com/v1".into();
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":settings,"keys":{}})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    assert_eq!(app.store.get("secret:ai").await.unwrap(), saved);
    assert_eq!(app.settings.read().await.ai_base_url, "https://api.x.ai/v1");
}

pub(crate) async fn fixture(dir: &Path) -> App {
    let store = Store::open(&dir.join("db")).unwrap();
    let auth = auth::Auth::new(store.clone(), dir).await.unwrap();
    auth.prepare_admin("owner".into(), PASSWORD.into(), dir)
        .await
        .unwrap();
    let proxy = dir.join("proxy");
    auth::private_file(&proxy, PROXY.as_bytes()).unwrap();
    App {
        store,
        auth,
        dir: dir.into(),
        port: 4877,
        web_access: web_access::WebAccess::from_file(ORIGIN, &proxy).unwrap(),
        host: Host {
            id: "test".into(),
            name: "test".into(),
            os: "Test".into(),
            cpu_model: "Test".into(),
            cores: 1,
            memory_total: 1024.,
            version: "test".into(),
        },
        latest: Default::default(),
        ring: Default::default(),
        events: tokio::sync::broadcast::channel(8).0,
        rules: Default::default(),
        settings: Arc::new(RwLock::new(Settings::default())),
        targets: Default::default(),
        target_status: Default::default(),
        inventory: Default::default(),
        errors: Default::default(),
        config_lock: Default::default(),
        analysis_lock: Default::default(),
    }
}
async fn call(
    router: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: &str,
    csrf: &str,
) -> (u16, HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("host", HOST)
        .header("origin", ORIGIN)
        .header("x-forwarded-proto", "https")
        .header("x-guard-proxy-token", PROXY)
        .header("content-type", "application/json");
    if !cookie.is_empty() {
        request = request.header("cookie", cookie);
    }
    if !csrf.is_empty() {
        request = request.header("x-csrf-token", csrf);
    }
    let response = router
        .clone()
        .oneshot(
            request
                .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn ip_https_settings_secret_and_recipient_contract_without_external_services() {
    let dir = tempfile::tempdir().unwrap();
    let app = fixture(dir.path()).await;
    let router = api::router(app.clone());
    let (status, _, body) = call(&router, Method::GET, "auth/status", None, "", "").await;
    assert_eq!(status, 200);
    assert_eq!(body["mfa_required"], false);
    let (status, headers, login) = call(
        &router,
        Method::POST,
        "auth/login",
        Some(json!({"username":"owner","password":PASSWORD})),
        "",
        "",
    )
    .await;
    assert_eq!(status, 200);
    let raw = headers["set-cookie"].to_str().unwrap();
    assert!(raw.starts_with("__Host-guard_session="));
    assert!(raw.contains("; Secure"));
    assert!(!raw.contains("Domain="));
    let cookie = raw.split(';').next().unwrap();
    let csrf = login["session"]["csrf"].as_str().unwrap();
    let (_, _, initial) = call(&router, Method::GET, "settings", None, cookie, "").await;
    assert_eq!(initial["settings"]["email_transport"], "disabled");
    assert_eq!(initial["keys"]["smtp"], false);
    let mut s: Settings = serde_json::from_value(initial["settings"].clone()).unwrap();
    s.email_transport = "smtp".into();
    s.smtp_host = "smtp.example.com".into();
    s.smtp_username = "owner@example.com".into();
    s.smtp_from = "owner@example.com".into();
    s.recipient = "alerts@example.com".into();
    s.recipient_verified = true;
    let payload = json!({"settings":s,"keys":{"smtp":SMTP_PASSWORD}});
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(payload.clone()),
            cookie,
            ""
        )
        .await
        .0,
        403
    );
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(payload),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    let (_, _, saved) = call(&router, Method::GET, "settings", None, cookie, "").await;
    assert_eq!(saved["keys"]["smtp"], true);
    assert_eq!(saved["settings"]["recipient_verified"], false);
    assert!(!saved.to_string().contains(SMTP_PASSWORD));
    let sealed = app.store.get("secret:smtp").await.unwrap().unwrap();
    assert!(!sealed.contains(SMTP_PASSWORD));
    assert_eq!(app.auth.unseal(&sealed).unwrap(), SMTP_PASSWORD);
    s.recipient_verified = false;
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":s,"keys":{"smtp":""}})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    assert_eq!(app.store.get("secret:smtp").await.unwrap().unwrap(), sealed);
    // Changing the destination cannot silently forward the previous password there.
    let mut changed = s.clone();
    changed.smtp_host = "other.example.com".into();
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":changed})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    assert_eq!(app.settings.read().await.smtp_host, s.smtp_host);
    // A rejected key batch must not partially replace any secret.
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":s,"keys":{"smtp":"replacement-secret","unknown":"bad"}})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    assert_eq!(app.store.get("secret:smtp").await.unwrap().unwrap(), sealed);
    assert_eq!(
        call(
            &router,
            Method::POST,
            "notifications/test",
            Some(json!({})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    // Test-only challenge fixture; no verification message is sent.
    let identity = smtp::identity(&s);
    app.store.put("smtp_verification",json!({"identity":identity,"hash":auth::hash(&format!("{identity}:123456")),"expires":guard_core::now()+600.,"attempts":0}).to_string()).await.unwrap();
    assert_eq!(
        call(
            &router,
            Method::POST,
            "recipients/verify",
            Some(json!({"code":"123456"})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    assert!(app.settings.read().await.recipient_verified);
    assert_eq!(
        call(
            &router,
            Method::POST,
            "notifications/test",
            Some(json!({"recipient":"ignored@example.com"})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    let count = app
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM outbox WHERE kind='email' AND state='pending'",
                [],
                |r| r.get::<_, u32>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(count, 1);
    // Editing destination invalidates verification; submitting code again cannot restore it.
    s.recipient = "changed@example.com".into();
    s.recipient_verified = true;
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":s})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    assert!(!app.settings.read().await.recipient_verified);
    assert_eq!(
        call(
            &router,
            Method::POST,
            "recipients/verify",
            Some(json!({"code":"123456"})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    // Network restrictions are enforced before opening a socket or authenticating.
    s.smtp_host = "127.0.0.1".into();
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":s,"keys":{"smtp":SMTP_PASSWORD}})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    let (code, _, error) = call(
        &router,
        Method::POST,
        "connections/test",
        Some(json!({"kind":"smtp","recipient":"ignored@example.com"})),
        cookie,
        csrf,
    )
    .await;
    assert_eq!(code, 400);
    assert!(error["error"].as_str().unwrap().contains("public network"));
    assert!(!error.to_string().contains(SMTP_PASSWORD));
    assert_eq!(
        call(
            &router,
            Method::POST,
            "connections/test",
            Some(json!({"kind":"smtp"})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
    // The initial login authorizes sensitive settings throughout this browser session.
    app.store
        .write(|c| {
            c.execute("UPDATE sessions SET step_up=0", [])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        call(
            &router,
            Method::PUT,
            "settings",
            Some(json!({"settings":s})),
            cookie,
            csrf
        )
        .await
        .0,
        200
    );
    assert_eq!(
        call(
            &router,
            Method::POST,
            "connections/test",
            Some(json!({"kind":"smtp"})),
            cookie,
            csrf
        )
        .await
        .0,
        400
    );
}
