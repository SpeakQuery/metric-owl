macro_rules! ensure { ($condition:expr, $($arg:tt)*) => { if !$condition { return Err(anyhow::anyhow!($($arg)*).into()); } }; }
use crate::{App, Settings};
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post, put},
};
use guard_core::{model::*, rules::Window};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist/"]
struct Assets;
pub struct Error(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
type Api = std::result::Result<Json<Value>, Error>;
pub fn router(a: App) -> Router {
    Router::new()
        .route("/api/v1/auth/status", get(auth_status))
        .route("/api/v1/auth/setup/begin", post(setup_begin))
        .route("/api/v1/auth/setup/enroll", post(setup_enroll))
        .route("/api/v1/auth/setup/finish", post(setup_finish))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/reauth", post(reauth))
        .route("/api/v1/auth/password", put(password))
        .route("/api/v1/auth/mfa/begin", post(mfa_begin))
        .route("/api/v1/auth/mfa/finish", post(mfa_finish))
        .route("/api/v1/auth/mfa/disable", post(mfa_disable))
        .route("/api/v1/host", get(host))
        .route("/api/v1/current", get(current))
        .route("/api/v1/forecasts", get(forecasts))
        .route("/api/v1/live", get(live))
        .route("/api/v1/history", get(history))
        .route("/api/v1/incidents", get(incidents))
        .route("/api/v1/incidents/{id}", get(incident))
        .route("/api/v1/incidents/{id}/analyze", post(analyze))
        .route("/api/v1/rules", get(rules).put(save_rules))
        .route("/api/v1/rules/presets", get(presets))
        .route("/api/v1/rules/preview", post(preview))
        .route("/api/v1/targets", get(targets).put(save_targets))
        .route("/api/v1/inventory", get(inventory))
        .route("/api/v1/settings", get(settings).put(save_settings))
        .route("/api/v1/connections/test", post(connection_test))
        .route("/api/v1/recipients/start", post(recipient_start))
        .route("/api/v1/recipients/verify", post(recipient_verify))
        .route("/api/v1/notifications", get(notifications))
        .route("/api/v1/notifications/test", post(test_email))
        .route("/api/v1/mcp/clients", get(clients).post(create_client))
        .route("/api/v1/mcp/connection", get(mcp_connection))
        .route(
            "/api/v1/mcp/clients/{id}/configuration",
            post(mcp_client_configuration),
        )
        .route(
            "/api/v1/mcp/clients/{id}",
            axum::routing::delete(revoke_client),
        )
        .route("/api/v1/security", get(security))
        .route("/api/v1/security/revoke-sessions", post(revoke_sessions))
        .fallback(asset)
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(a.clone(), guard))
        .with_state(a)
}
// The guard normalizes the HTTPS-only cookie before any handler runs. No
// caller-supplied transport marker is trusted by handlers.
pub fn cookie(h: &HeaderMap) -> String {
    named_cookie(h, "guard_session").unwrap_or_default()
}
fn named_cookie(h: &HeaderMap, name: &str) -> Option<String> {
    let mut token = None;
    for value in h.get_all("cookie") {
        let value = value.to_str().ok()?;
        for item in value.split(';') {
            let Some((key, value)) = item.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if token.is_some() {
                    return None;
                }
                token = Some(value.to_string());
            }
        }
    }
    token
}
fn reject(code: StatusCode, message: &str) -> Response {
    (code, Json(json!({"error":message}))).into_response()
}
fn secured(mut res: Response, mode: crate::web_access::Mode) -> Response {
    let h = res.headers_mut();
    h.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    h.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    h.insert("Cache-Control", HeaderValue::from_static("no-store"));
    h.insert("Content-Security-Policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"));
    if mode == crate::web_access::Mode::Https {
        h.insert(
            "Strict-Transport-Security",
            HeaderValue::from_static("max-age=31536000"),
        );
        // Login/logout are the only cookie writers. The public cookie is
        // host-bound and never sent over HTTP or to a sibling subdomain.
        if let Some(value) = h.get("set-cookie").and_then(|v| v.to_str().ok())
            && let Some(value) = value.strip_prefix("guard_session=")
        {
            let value = format!("__Host-guard_session={value}; Secure");
            if let Ok(value) = HeaderValue::from_str(&value) {
                h.insert("set-cookie", value);
            } else {
                // Do not fall back to an insecure cookie if encoding fails.
                h.remove("set-cookie");
            }
        }
    }
    res
}
async fn guard(State(a): State<App>, mut req: Request, next: Next) -> Response {
    let mode = match a.web_access.validate(req.headers(), req.method()) {
        Ok(mode) => mode,
        Err(message) => {
            return secured(
                reject(StatusCode::FORBIDDEN, message),
                crate::web_access::Mode::Local,
            );
        }
    };
    if mode == crate::web_access::Mode::Https {
        let token = named_cookie(req.headers(), "__Host-guard_session");
        req.headers_mut().remove("cookie");
        if let Some(token) = token
            && let Ok(value) = HeaderValue::from_str(&format!("guard_session={token}"))
        {
            req.headers_mut().insert("cookie", value);
        }
    }
    secured(authorized_request(a, req, next).await, mode)
}
async fn authorized_request(a: App, req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD);
    let path = req.uri().path();
    let public = [
        "/api/v1/auth/status",
        "/api/v1/auth/login",
        "/api/v1/auth/setup/begin",
        "/api/v1/auth/setup/enroll",
        "/api/v1/auth/setup/finish",
    ]
    .contains(&path);
    if path.starts_with("/api/") && !public {
        let token = cookie(req.headers());
        let session = match a.auth.session(&token, !path.ends_with("/live")).await {
            Ok(x) => x,
            Err(_) => return reject(StatusCode::UNAUTHORIZED, "Authentication required"),
        };
        if !safe
            && !crate::auth::constant(
                req.headers()
                    .get("x-csrf-token")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(""),
                &session.csrf,
            )
        {
            return reject(StatusCode::FORBIDDEN, "CSRF validation failed");
        }
    }
    next.run(req).await
}
async fn auth_status(State(a): State<App>, h: HeaderMap) -> Api {
    let session = a.auth.session(&cookie(&h), false).await.ok();
    Ok(Json(
        json!({"configured":a.auth.configured().await?,"enrollment_ready":a.auth.enrollment_ready().await?,"mfa_required":a.auth.mfa_required().await?,"session":session,"process_id":std::process::id()}),
    ))
}
#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
    #[serde(default)]
    code: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentCredentials {
    username: String,
    password: String,
}
async fn setup_enroll(State(a): State<App>, Json(p): Json<EnrollmentCredentials>) -> Api {
    Ok(Json(a.auth.enroll(p.username, p.password).await?))
}
#[derive(Deserialize)]
struct SetupBegin {
    key: String,
    username: String,
    password: String,
}
async fn setup_begin(State(a): State<App>, Json(p): Json<SetupBegin>) -> Api {
    Ok(Json(a.auth.begin(p.key, p.username, p.password).await?))
}
#[derive(Deserialize)]
struct SetupFinish {
    key: String,
    code: String,
}
async fn setup_finish(State(a): State<App>, Json(p): Json<SetupFinish>) -> Api {
    Ok(Json(
        json!({"recovery_codes":a.auth.finish(p.key,p.code,&a.dir).await?}),
    ))
}
async fn login(
    State(a): State<App>,
    Json(p): Json<Credentials>,
) -> std::result::Result<Response, Error> {
    let (token, s) = a.auth.login(p.username, p.password, p.code).await?;
    let mut r = Json(json!({"session":s})).into_response();
    r.headers_mut().insert(
        "set-cookie",
        HeaderValue::from_str(&format!(
            "guard_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200"
        ))?,
    );
    Ok(r)
}
async fn logout(State(a): State<App>, h: HeaderMap) -> std::result::Result<Response, Error> {
    a.auth.logout(&cookie(&h)).await?;
    let mut r = Json(json!({"ok":true})).into_response();
    r.headers_mut().insert(
        "set-cookie",
        HeaderValue::from_static("guard_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    Ok(r)
}
async fn reauth(State(a): State<App>, h: HeaderMap, Json(p): Json<Credentials>) -> Api {
    a.auth
        .reauth(&cookie(&h), p.username, p.password, p.code)
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn password(State(a): State<App>, h: HeaderMap, Json(p): Json<Value>) -> Api {
    a.auth.require_step_up(&cookie(&h)).await?;
    a.auth
        .change_password(&cookie(&h), p["password"].as_str().unwrap_or("").into())
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn mfa_begin(State(a): State<App>, h: HeaderMap) -> Api {
    let token = cookie(&h);
    a.auth.require_step_up(&token).await?;
    Ok(Json(a.auth.begin_mfa_change(&token).await?))
}
async fn mfa_finish(State(a): State<App>, h: HeaderMap, Json(p): Json<Value>) -> Api {
    let token = cookie(&h);
    a.auth.require_step_up(&token).await?;
    Ok(Json(
        json!({"recovery_codes":a.auth.finish_mfa_change(&token,p["code"].as_str().unwrap_or("")).await?}),
    ))
}
async fn mfa_disable(State(a): State<App>, h: HeaderMap) -> Api {
    a.auth.disable_mfa(&cookie(&h)).await?;
    Ok(Json(json!({"ok":true,"mfa_required":false})))
}
async fn host(State(a): State<App>) -> Json<Host> {
    Json(a.host)
}
async fn current(State(a): State<App>) -> Json<Option<Sample>> {
    Json(a.latest.read().await.clone())
}
async fn forecasts(State(a): State<App>) -> Api {
    let disks = a
        .latest
        .read()
        .await
        .as_ref()
        .map(|s| s.disks.clone())
        .unwrap_or_default();
    let mut results = vec![];
    let now = guard_core::now();
    for d in disks {
        let v = history_data(
            &a,
            d.id.clone(),
            "disk_used".into(),
            now - 30. * 86400.,
            now,
        )
        .await?;
        let rows: Vec<Aggregate> = serde_json::from_value(v["rows"].clone())?;
        let days = guard_core::aggregate::forecast_days(&rows, 100.);
        results.push(json!({"source":d.id,"days":days,"reason":if days.is_none(){Some("At least 7 days of sufficient, consistent growth history are required")}else{None},"note":"Estimated time if the current trend continues; this is not a capacity guarantee."}));
    }
    Ok(Json(json!(results)))
}
async fn live(
    State(a): State<App>,
    h: HeaderMap,
) -> Sse<impl futures_util::Stream<Item = std::result::Result<Event, std::convert::Infallible>>> {
    let mut receiver = a.events.subscribe();
    let token = cookie(&h);
    let stream = async_stream::stream! {loop{if a.auth.session(&token,false).await.is_err(){yield Ok(Event::default().event("expired").data("{}"));break;}match receiver.recv().await{Ok(data)=>yield Ok(Event::default().event("sample").data(data)),Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>yield Ok(Event::default().event("gap").data("{}")),Err(_)=>break}}};
    Sse::new(stream).keep_alive(KeepAlive::default())
}
#[derive(Deserialize)]
struct HistoryQuery {
    source: Option<String>,
    metric: Option<String>,
    from: f64,
    to: f64,
}
async fn history(State(a): State<App>, Query(q): Query<HistoryQuery>) -> Api {
    Ok(Json(
        history_data(
            &a,
            q.source.unwrap_or("host".into()),
            q.metric.unwrap_or("cpu".into()),
            q.from,
            q.to,
        )
        .await?,
    ))
}
pub async fn history_data(
    a: &App,
    source: String,
    metric: String,
    from: f64,
    to: f64,
) -> Result<Value> {
    ensure!(
        from.is_finite()
            && to.is_finite()
            && from < to
            && to - from <= 366. * 86400.
            && source.len() <= 300
            && [
                "cpu",
                "memory",
                "network_in",
                "network_out",
                "disk_used",
                "disk_free",
                "swap_used",
                "swap_total",
                "swap_used_percent",
                "memory_pressure",
                "cpu_iowait",
                "oom_kills_5m",
                "disk_io_latency",
                "disk_io_utilization",
                "inode_used"
            ]
            .contains(&metric.as_str()),
        "Invalid history query"
    );
    let requested = if to - from <= 86400. {
        60
    } else if to - from <= 7. * 86400. {
        300
    } else if to - from <= 90. * 86400. {
        3600
    } else {
        86400
    };
    let rows:Vec<Aggregate>=a.store.read(move|c|{let mut s=c.prepare("SELECT json FROM aggregates WHERE source=?1 AND metric=?2 AND start>=?3 AND start<?4 ORDER BY start LIMIT 20000")?;s.query_map(params![source,metric,from,to],|r|r.get::<_,String>(0))?.map(|r|Ok(serde_json::from_str(&r?)?)).collect()}).await?;
    let resolution = rows
        .iter()
        .map(|r| r.resolution)
        .max()
        .unwrap_or(requested)
        .max(requested);
    Ok(
        json!({"resolution":resolution,"rows":guard_core::aggregate::merge(&rows,resolution),"from":from,"to":to}),
    )
}
#[derive(Deserialize)]
struct Page {
    offset: Option<u32>,
}
async fn incidents(State(a): State<App>, Query(p): Query<Page>) -> Api {
    Ok(Json(
        json!({"items":a.store.incidents(p.offset.unwrap_or(0).min(100000)).await?}),
    ))
}
async fn incident(State(a): State<App>, Path(id): Path<String>) -> Api {
    Ok(Json(
        a.store
            .read(move |c| {
                let raw: String =
                    c.query_row("SELECT json FROM incidents WHERE id=?1", [id], |r| r.get(0))?;
                let event: Incident = serde_json::from_str(&raw)?;
                let mut s = c.prepare(
                    "SELECT json FROM details WHERE at>=?1 AND at<=?2 ORDER BY at LIMIT 500",
                )?;
                let details: Vec<Sample> = s
                    .query_map(
                        params![event.started_at - 300., event.updated_at + 120.],
                        |r| r.get::<_, String>(0),
                    )?
                    .map(|r| Ok(serde_json::from_str(&r?)?))
                    .collect::<Result<_>>()?;
                Ok(json!({"event":event,"details":details}))
            })
            .await?,
    ))
}
async fn analyze(State(a): State<App>, Path(id): Path<String>, h: HeaderMap) -> Api {
    a.auth.require_step_up(&cookie(&h)).await?;
    Ok(Json(crate::integrations::analyze(&a, &id).await?))
}
async fn rules(State(a): State<App>) -> Json<Vec<Rule>> {
    Json(a.rules.read().await.clone())
}
async fn save_rules(State(a): State<App>, Json(r): Json<Vec<Rule>>) -> Api {
    let _config = a.config_lock.lock().await;
    ensure!(r.len() <= 200, "At most 200 rules are allowed");
    let mut ids = std::collections::HashSet::new();
    for rule in &r {
        rule.validate()?;
        ensure!(ids.insert(&rule.id), "Duplicate rule ID");
    }
    a.store.put("rules", serde_json::to_string(&r)?).await?;
    *a.rules.write().await = r;
    a.store.audit("admin".into(), "rules.update".into()).await?;
    Ok(Json(json!({"ok":true})))
}
async fn presets() -> Json<Value> {
    let balanced = default_rules();
    let sensitive: Vec<_> = balanced
        .clone()
        .into_iter()
        .map(|mut r| {
            r.id = format!("sensitive-{}", r.id);
            r.name = format!("Sensitive · {}", r.name);
            r.threshold -= 10.;
            r.recovery_threshold -= 10.;
            r.window_seconds = 120;
            r
        })
        .collect();
    let sustained: Vec<_> = [
        ("cpu", "CPU high for one hour", 70.),
        ("memory", "Memory high for 30 minutes", 85.),
    ]
    .into_iter()
    .map(|(m, n, t)| Rule {
        id: format!("sustained-{m}"),
        name: n.into(),
        source: "host".into(),
        metric: m.into(),
        operator: ">".into(),
        threshold: t,
        window_seconds: if m == "cpu" { 3600 } else { 1800 },
        fraction: 0.8,
        severity: "warning".into(),
        recovery_threshold: t - 5.,
        recovery_seconds: 120,
        enabled: true,
        notify: true,
    })
    .collect();
    Json(
        json!([{"id":"balanced","name":"Balanced monitoring","description":"Reliable 3–5 minute defaults for CPU and memory.","rules":balanced},{"id":"sensitive","name":"Sensitive monitoring","description":"Earlier alerts: 2-minute windows and lower thresholds.","rules":sensitive},{"id":"sustained","name":"Sustained load","description":"Alert when CPU stays high for 1 hour or memory for 30 minutes.","rules":sustained}]),
    )
}
async fn preview(State(a): State<App>, Json(r): Json<Rule>) -> Api {
    r.validate()?;
    let ring = a.ring.read().await;
    let mut w = Window::default();
    let mut matches = 0;
    for s in ring.iter() {
        w.push(s.at, s.metric(&r.metric, &r.source), 1.);
        if w.evaluate(s.at, r.window_seconds, r.threshold, &r.operator, r.fraction)
            .fires
        {
            matches += 1;
        }
    }
    Ok(Json(
        json!({"matching_evaluations":matches,"available_seconds":ring.len(),"complete":ring.len()as u32>r.window_seconds,"note":"Actual per-second samples from the last 5 minutes. No email was sent; an incomplete window cannot produce a conclusive result."}),
    ))
}
async fn targets(State(a): State<App>) -> Json<Value> {
    Json(json!({"targets":*a.targets.read().await,"status":*a.target_status.read().await}))
}
async fn inventory(State(a): State<App>) -> Json<crate::discovery::Inventory> {
    Json(a.inventory.read().await.clone())
}
async fn save_targets(State(a): State<App>, h: HeaderMap, Json(mut t): Json<Vec<Target>>) -> Api {
    let _config = a.config_lock.lock().await;
    a.auth.require_step_up(&cookie(&h)).await?;
    ensure!(t.len() <= 30, "At most 30 sources are allowed");
    for target in &mut t {
        if target.kind == "job" && target.address.is_empty() {
            uuid::Uuid::parse_str(&target.id)?;
            target.address = a
                .dir
                .canonicalize()?
                .join("job-reports")
                .join(format!("{}.json", target.id))
                .to_string_lossy()
                .into();
        }
    }
    let mut ids = std::collections::HashSet::new();
    for x in &t {
        crate::targets::validate(x)?;
        ensure!(ids.insert(&x.id), "Duplicate source ID");
    }
    let previous = a.targets.read().await.clone();
    let mut rules = a.rules.read().await.clone();
    // Removing a source also removes its rules; replacing its type gets fresh presets.
    let retired: std::collections::HashSet<_> = previous
        .iter()
        .filter(|old| !t.iter().any(|new| new.id == old.id && new.kind == old.kind))
        .map(|old| old.id.as_str())
        .collect();
    rules.retain(|rule| !retired.contains(rule.source.as_str()));
    for target in &mut t {
        if target.kind == "job" {
            let added = previous
                .iter()
                .find(|old| old.id == target.id)
                .and_then(|old| old.options.as_ref())
                .and_then(|options| options.job_added_at)
                .unwrap_or_else(guard_core::now);
            target
                .options
                .get_or_insert_with(Default::default)
                .job_added_at = Some(added);
        }
        if !previous
            .iter()
            .any(|old| old.id == target.id && old.kind == target.kind)
            && ["http", "service", "tls", "job"].contains(&target.kind.as_str())
        {
            rules.extend(crate::targets::default_rules(target));
        }
    }
    ensure!(
        rules.len() <= 200,
        "At most 200 alarm rules are allowed; remove unused rules before adding sources"
    );
    let encoded_rules = serde_json::to_string(&rules)?;
    let encoded_targets = serde_json::to_string(&t)?;
    a.store.write(move |connection| {
        let transaction = connection.transaction()?;
        for (key, value) in [("rules", encoded_rules), ("targets", encoded_targets)] {
            transaction.execute("INSERT INTO kv(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key,value])?;
        }
        transaction.commit()?;
        Ok(())
    }).await?;
    *a.rules.write().await = rules;
    *a.targets.write().await = t.clone();
    Ok(Json(json!({"ok":true,"targets":t})))
}
async fn settings(State(a): State<App>) -> Api {
    Ok(Json(
        json!({"settings":*a.settings.read().await,"keys":{"ai":a.store.get("secret:ai").await?.is_some(),"relay":a.store.get("secret:relay").await?.is_some(),"webhook":a.store.get("secret:webhook").await?.is_some()}}),
    ))
}
#[derive(Deserialize)]
struct SettingsBody {
    settings: Settings,
    keys: Option<std::collections::HashMap<String, String>>,
}
async fn save_settings(State(a): State<App>, h: HeaderMap, Json(mut b): Json<SettingsBody>) -> Api {
    let _config = a.config_lock.lock().await;
    a.auth.require_step_up(&cookie(&h)).await?;
    crate::integrations::validate_settings(&b.settings)?;
    let old = a.settings.read().await.clone();
    b.settings.recipient_verified = old.recipient_verified
        && old.recipient == b.settings.recipient
        && old.relay_url == b.settings.relay_url;
    for (name, key) in b.keys.unwrap_or_default() {
        ensure!(
            ["ai", "relay", "webhook"].contains(&name.as_str()) && key.len() <= 4096,
            "Invalid connection key"
        );
        if !key.is_empty() {
            a.store
                .put(&format!("secret:{name}"), a.auth.seal(&key)?)
                .await?;
            if name == "relay" {
                b.settings.recipient_verified = false;
            }
        }
    }
    a.store
        .put("settings", serde_json::to_string(&b.settings)?)
        .await?;
    *a.settings.write().await = b.settings;
    a.store
        .audit("admin".into(), "settings.update".into())
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn connection_test(State(a): State<App>, h: HeaderMap, Json(p): Json<Value>) -> Api {
    a.auth.require_step_up(&cookie(&h)).await?;
    let kind = p["kind"].as_str().unwrap_or("");
    let s = a.settings.read().await.clone();
    match kind {
        "relay" => {
            let result = crate::integrations::relay(&a, "status", json!({})).await?;
            Ok(Json(
                json!({"ok":true,"message":if result["test_mode"]==true{"Connection successful. This is a local test provider; no real email is sent."}else{"Notification service key verified."}}),
            ))
        }
        "ai" => {
            let key = crate::integrations::secret(&a, "ai").await?;
            let url = if s.ai_provider == "gemini" {
                "https://generativelanguage.googleapis.com/v1beta/models".into()
            } else {
                format!("{}/models", s.ai_base_url.trim_end_matches('/'))
            };
            let c = crate::integrations::client_for(&url, false).await?;
            let req = if s.ai_provider == "gemini" {
                c.get(url).header("x-goog-api-key", key)
            } else {
                c.get(url).bearer_auth(key)
            };
            crate::integrations::bounded_json(req.send().await?).await?;
            Ok(Json(
                json!({"ok":true,"message":"API key verified; the model list is accessible. Access to the selected model will be verified on the first analysis."}),
            ))
        }
        "webhook" => {
            crate::integrations::client_for(&s.webhook_url, false).await?;
            crate::integrations::secret(&a, "webhook").await?;
            Ok(Json(
                json!({"ok":true,"message":"HTTPS destination and signing key are ready. This test does not send an event to the destination."}),
            ))
        }
        _ => Err(anyhow::anyhow!("Unknown connection").into()),
    }
}
async fn recipient_start(State(a): State<App>, h: HeaderMap) -> Api {
    a.auth.require_step_up(&cookie(&h)).await?;
    let r = a.settings.read().await.recipient.clone();
    Ok(Json(
        crate::integrations::relay(&a, "recipient/start", json!({"recipient":r})).await?,
    ))
}
async fn recipient_verify(State(a): State<App>, h: HeaderMap, Json(p): Json<Value>) -> Api {
    let _config = a.config_lock.lock().await;
    a.auth.require_step_up(&cookie(&h)).await?;
    let mut s = a.settings.read().await.clone();
    let result = crate::integrations::relay(
        &a,
        "recipient/verify",
        json!({"recipient":s.recipient,"code":p["code"]}),
    )
    .await?;
    s.recipient_verified = result["verified"] == true;
    a.store.put("settings", serde_json::to_string(&s)?).await?;
    *a.settings.write().await = s;
    Ok(Json(result))
}
async fn notifications(State(a): State<App>) -> Api {
    let rows=a.store.read(|c|{let mut s=c.prepare("SELECT id,kind,state,attempts,created,error FROM outbox ORDER BY created DESC LIMIT 100")?;Ok(s.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"attempts":r.get::<_,u32>(3)?,"created":r.get::<_,f64>(4)?,"error":r.get::<_,Option<String>>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)}).await?;
    Ok(Json(json!({"items":rows,"limit_per_day":60})))
}
async fn test_email(State(a): State<App>, h: HeaderMap) -> Api {
    a.auth.require_step_up(&cookie(&h)).await?;
    let s = a.settings.read().await;
    ensure!(s.recipient_verified, "Verify your email address first");
    a.store.enqueue(format!("test:{}",uuid::Uuid::new_v4()),"email".into(),json!({"alias":s.alias,"title":"Connection test","summary":"Metric Owl test notification.","recommendation":"This is not an alert."})).await?;
    Ok(Json(
        json!({"ok":true,"message":"Test message queued. Check the queue for delivery status."}),
    ))
}
async fn clients(State(a): State<App>) -> Api {
    Ok(Json(a.store.read(|c|{let mut s=c.prepare("SELECT id,name,scopes,revoked FROM mcp_clients LIMIT 100")?;let rows=s.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"scopes":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(Value::Null),"revoked":r.get::<_,bool>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;Ok(json!(rows))}).await?))
}
async fn mcp_connection(State(a): State<App>) -> Api {
    let command = std::env::current_exe()?.to_string_lossy().into_owned();
    let directory = std::fs::canonicalize(&a.dir)?
        .to_string_lossy()
        .into_owned();
    let hints = crate::mcp::connection_hints(&a.dir);
    Ok(Json(
        json!({"transport":"stdio","command":command,"args":["--data-dir",directory,"mcp"],"key_variable":"GUARD_MCP_KEY","tools":crate::mcp::TOOLS,
        "key_file_supported":true,"suggested_transport":hints["suggested_transport"],"ssh":hints["ssh"]}),
    ))
}
async fn create_client(State(a): State<App>, h: HeaderMap, Json(p): Json<Value>) -> Api {
    let _config = a.config_lock.lock().await;
    a.auth.require_step_up(&cookie(&h)).await?;
    let name = p["name"].as_str().unwrap_or("Agent").to_owned();
    ensure!(
        !name.trim().is_empty() && name.len() <= 100 && !name.chars().any(char::is_control),
        "Enter a connection name of 1–100 characters"
    );
    let scopes: Vec<String> = serde_json::from_value(p["scopes"].clone())?;
    ensure!(
        !scopes.is_empty()
            && scopes
                .iter()
                .all(|s| crate::mcp::TOOLS.contains(&s.as_str())),
        "Invalid scope"
    );
    let requested_connection = p.get("connection").filter(|v| !v.is_null());
    let options: crate::mcp::ConnectionOptions = requested_connection
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default();
    options.validate()?;
    let key = crate::auth::token();
    let hash = crate::auth::hash(&key);
    let id = uuid::Uuid::new_v4().to_string();
    let return_id = id.clone();
    crate::mcp::create_key_file(&a.dir, &id, &key)?;
    let configuration =
        match crate::mcp::configuration(&a.dir, &std::env::current_exe()?, &id, &options) {
            Ok(config) => config,
            Err(error) => {
                let _ = crate::mcp::remove_key_file(&a.dir, &id);
                return Err(error.into());
            }
        };
    let result = a
        .store
        .write(move |c| {
            let count: u32 = c.query_row("SELECT COUNT(*) FROM mcp_clients", [], |r| r.get(0))?;
            ensure!(count < 100, "Client limit reached");
            c.execute(
                "INSERT INTO mcp_clients(id,name,hash,scopes) VALUES(?1,?2,?3,?4)",
                params![id, name, hash, serde_json::to_string(&scopes)?],
            )?;
            Ok(())
        })
        .await;
    if let Err(error) = result {
        let _ = crate::mcp::remove_key_file(&a.dir, &return_id);
        return Err(error.into());
    }
    let mut response = json!({"id":return_id,"configuration":configuration,"note":"Connection created. Its private key stays on this server."});
    // Preserve the existing raw-key API for older clients. The wizard never receives it.
    if requested_connection.is_none() {
        response["key"] = json!(key);
        response["note"] =
            json!("This key is shown only once. A private server key file is also available.");
    }
    Ok(Json(response))
}
async fn mcp_client_configuration(
    State(a): State<App>,
    Path(id): Path<String>,
    Json(options): Json<crate::mcp::ConnectionOptions>,
) -> Api {
    options.validate()?;
    let lookup = id.clone();
    let active = a
        .store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM mcp_clients WHERE id=?1 AND revoked=0)",
                [lookup],
                |row| row.get::<_, bool>(0),
            )?)
        })
        .await?;
    ensure!(active, "This MCP connection is unavailable or revoked");
    Ok(Json(crate::mcp::configuration(
        &a.dir,
        &std::env::current_exe()?,
        &id,
        &options,
    )?))
}
async fn revoke_client(State(a): State<App>, h: HeaderMap, Path(id): Path<String>) -> Api {
    let _config = a.config_lock.lock().await;
    a.auth.require_step_up(&cookie(&h)).await?;
    ensure!(
        uuid::Uuid::parse_str(&id)?.to_string() == id,
        "Invalid MCP client ID"
    );
    let cleanup_id = id.clone();
    a.store
        .write(move |c| {
            c.execute("UPDATE mcp_clients SET revoked=1 WHERE id=?1", [id])?;
            Ok(())
        })
        .await?;
    let cleanup = crate::mcp::remove_key_file(&a.dir, &cleanup_id);
    Ok(Json(
        json!({"ok":true,"key_file_removed":cleanup.as_ref().is_ok_and(|removed| *removed),
        "note":if cleanup.is_err() {"Connection revoked. Its private file could not be removed, but access is disabled."} else {"Connection revoked."}}),
    ))
}
async fn security(State(a): State<App>) -> Api {
    let sessions=a.store.read(|c|{let mut s=c.prepare("SELECT created,last FROM sessions WHERE created>?1 AND last>?2 ORDER BY created DESC LIMIT 100")?;let now=guard_core::now();Ok(s.query_map(params![now-43200.,now-1800.],|r|Ok(json!({"created":r.get::<_,f64>(0)?,"last":r.get::<_,f64>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?)}).await?;
    Ok(Json(
        json!({"sessions":sessions,"storage_bytes":a.store.size_bytes(),"quota_bytes":a.settings.read().await.quota_bytes,"retention_warning":a.store.get("retention_warning").await?,"errors":*a.errors.read().await,"mfa_required":a.auth.mfa_required().await?,"access":if a.web_access.public_origin().is_some(){"HTTPS + loopback + SSH"}else{"loopback + SSH"},"version":env!("CARGO_PKG_VERSION")}),
    ))
}
async fn revoke_sessions(State(a): State<App>, h: HeaderMap) -> Api {
    a.auth.require_step_up(&cookie(&h)).await?;
    a.store
        .write(|c| {
            c.execute("DELETE FROM sessions", [])?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn asset(req: Request) -> Response {
    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = req.uri().path();
    let name = match path {
        "/admin" | "/admin/" => "index.html",
        _ if path.starts_with("/admin/assets/") => &path[7..],
        // Compatibility for open tabs from an older bundled panel.
        _ if path.starts_with("/assets/") => &path[1..],
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match Assets::get(name) {
        Some(data) => (
            [(
                "Content-Type",
                mime_guess::from_path(name).first_or_octet_stream().as_ref(),
            )],
            data.data.to_vec(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod cookie_tests {
    use super::*;
    #[test]
    fn duplicate_session_cookies_fail_closed() {
        let mut headers = HeaderMap::new();
        headers.append("cookie", HeaderValue::from_static("guard_session=one"));
        headers.append(
            "cookie",
            HeaderValue::from_static("other=value; guard_session=two"),
        );
        assert_eq!(cookie(&headers), "");
        headers.clear();
        headers.insert(
            "cookie",
            HeaderValue::from_static("guard_session=one; __Host-guard_session=two"),
        );
        assert_eq!(cookie(&headers), "one");
        assert_eq!(
            named_cookie(&headers, "__Host-guard_session"),
            Some("two".into())
        );
    }
    #[test]
    fn public_session_cookie_is_secure_and_host_bound() {
        for value in [
            "guard_session=token; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200",
            "guard_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        ] {
            let mut response = ().into_response();
            response
                .headers_mut()
                .insert("set-cookie", HeaderValue::from_str(value).unwrap());
            let response = secured(response, crate::web_access::Mode::Https);
            let cookie = response.headers()["set-cookie"].to_str().unwrap();
            assert!(cookie.starts_with("__Host-guard_session="));
            assert!(cookie.ends_with("; Secure"));
            assert!(cookie.contains("; HttpOnly; SameSite=Strict; Path=/;"));
            assert!(!cookie.contains("Domain="));
        }
        let error = secured(
            reject(StatusCode::UNAUTHORIZED, "Authentication required"),
            crate::web_access::Mode::Local,
        );
        assert_eq!(error.headers()["cache-control"], "no-store");
        assert_eq!(error.headers()["x-frame-options"], "DENY");
    }
}
