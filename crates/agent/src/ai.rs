//! Provider presets and a single, verified connection operation.
use crate::{App, Settings, integrations};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    #[serde(skip)]
    pub base_url: &'static str,
    #[serde(skip)]
    pub model: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "xai",
        name: "xAI (Grok)",
        base_url: "https://api.x.ai/v1",
        model: "grok-4.3",
    },
    Provider {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-6-luna",
    },
    Provider {
        id: "anthropic",
        name: "Anthropic (Claude)",
        base_url: "https://api.anthropic.com/v1",
        model: "claude-haiku-5-5",
    },
    Provider {
        id: "gemini",
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        model: "gemini-3.5-flash-lite",
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        model: "deepseek-flash",
    },
    Provider {
        id: "mistral",
        name: "Mistral AI",
        base_url: "https://api.mistral.ai/v1",
        model: "mistral-small-2603",
    },
];

// Keep existing saved connections usable without offering specialist providers
// as additional setup choices. They are never silently moved to another vendor.
const LEGACY_PROVIDERS: &[Provider] = &[
    Provider {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        model: "openai/gpt-oss-20b",
    },
    Provider {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        model: "openai/gpt-4.1-mini",
    },
    Provider {
        id: "nvidia",
        name: "NVIDIA NIM",
        base_url: "https://integrate.api.nvidia.com/v1",
        model: "nvidia/nemotron-3-nano-30b-a3b",
    },
    Provider {
        id: "siliconflow",
        name: "SiliconFlow",
        base_url: "https://api.siliconflow.com/v1",
        model: "Qwen/Qwen3-8B",
    },
];

pub fn provider(id: &str) -> Result<&'static Provider> {
    PROVIDERS
        .iter()
        .chain(LEGACY_PROVIDERS.iter())
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Choose a supported AI provider"))
}

// Older installations used `openai` for every OpenAI-compatible service.
pub fn selected(s: &Settings) -> Option<&'static Provider> {
    if s.ai_provider == "gemini" {
        return provider("gemini").ok();
    }
    PROVIDERS
        .iter()
        .chain(LEGACY_PROVIDERS.iter())
        .find(|p| p.id != "gemini" && p.base_url == s.ai_base_url.trim_end_matches('/'))
}

pub fn identity(s: &Settings) -> String {
    crate::auth::hash(&format!(
        "{}\n{}\n{}",
        s.ai_provider, s.ai_base_url, s.ai_model
    ))
}

pub async fn status(app: &App) -> Result<Value> {
    let _config = app.config_lock.lock().await;
    let s = app.settings.read().await.clone();
    let saved = app.store.get("secret:ai").await?.is_some();
    let verified =
        saved && app.store.get("ai:verified").await?.as_deref() == Some(identity(&s).as_str());
    Ok(
        json!({"providers": PROVIDERS, "selected_provider": selected(&s).filter(|p| PROVIDERS.iter().any(|shown| shown.id == p.id)).map(|p| p.id), "key_saved": saved, "verified": verified}),
    )
}

pub fn validate_key(key: &str) -> Result<()> {
    ensure!(!key.is_empty(), "Enter your provider's API key");
    ensure!(
        key.len() <= 4096 && key.bytes().all(|b| b.is_ascii_graphic()),
        "The API key contains invalid characters"
    );
    Ok(())
}

pub async fn connect(app: &App, id: &str, input_key: &str) -> Result<Value> {
    let p = provider(id)?;
    // All settings writes and analyses use these locks in this order. An analysis
    // must never pair the previous provider with the replacement credential.
    let _analysis = app.analysis_lock.lock().await;
    let _config = app.config_lock.lock().await;
    let old = app.settings.read().await.clone();
    let mut next = old.clone();
    next.ai_provider = p.id.into();
    next.ai_base_url = p.base_url.into();
    next.ai_model = p.model.into();
    let key = if input_key.trim().is_empty() {
        ensure!(
            selected(&old).is_some_and(|old| old.id == p.id),
            "Enter an API key for the selected provider"
        );
        // Preserve a working legacy model when checking its existing credential.
        if !old.ai_model.is_empty() {
            next.ai_model = old.ai_model.clone();
        }
        integrations::secret(app, "ai").await?
    } else {
        input_key.trim().to_string()
    };
    validate_key(&key)?;
    ensure!(
        app.store.budget("ai_connect", 20).await?,
        "Connection check limit reached. Please try again tomorrow."
    );
    // A short synthetic generation checks authentication AND model access. Model
    // catalogs can be public, so a successful GET /models is not proof of either.
    generate(
        &next,
        &key,
        "This is a connection check. Reply only with OK.",
        "OK",
        1200,
    )
    .await?;
    persist(app, next, &key).await?;
    Ok(json!({"ok":true,"message":format!("{} connected. You can now analyze events.", p.name)}))
}

async fn persist(app: &App, next: Settings, key: &str) -> Result<()> {
    let sealed = app.auth.seal(key)?;
    let encoded = serde_json::to_string(&next)?;
    let verified = identity(&next);
    app.store.write(move |c| {
        let tx = c.transaction()?;
        for (name, value) in [("settings", encoded), ("secret:ai", sealed), ("ai:verified", verified)] {
            tx.execute("INSERT INTO kv(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![name,value])?;
        }
        tx.commit()?;
        Ok(())
    }).await?;
    *app.settings.write().await = next;
    app.store.audit("admin".into(), "ai.connect".into()).await?;
    Ok(())
}

fn payload(s: &Settings, system: &str, user: &str, limit: u32) -> (String, Value) {
    match s.ai_provider.as_str() {
        "gemini" => {
            let mut body = json!({"systemInstruction":{"parts":[{"text":system}]},"contents":[{"parts":[{"text":user}]}],"generationConfig":{"maxOutputTokens":limit}});
            if s.ai_model == "gemini-3.5-flash-lite" {
                body["generationConfig"]["thinkingConfig"] = json!({"thinkingLevel":"minimal"});
            } else if s.ai_model == "gemini-2.5-flash" {
                body["generationConfig"]["thinkingConfig"] = json!({"thinkingBudget":0});
            }
            (
                format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                    s.ai_model
                ),
                body,
            )
        }
        "anthropic" => (
            format!("{}/messages", s.ai_base_url.trim_end_matches('/')),
            json!({"model":s.ai_model,"system":system,"messages":[{"role":"user","content":user}],"max_tokens":limit}),
        ),
        _ => {
            let mut body = json!({"model":s.ai_model,"messages":[{"role":"system","content":system},{"role":"user","content":user}],"max_tokens":limit});
            match selected(s).map(|p| p.id) {
                Some("openai") if s.ai_model.starts_with("gpt-6") => {
                    body.as_object_mut().unwrap().remove("max_tokens");
                    body["max_completion_tokens"] = json!(limit);
                    body["reasoning_effort"] = json!("none");
                }
                Some("groq") if s.ai_model.starts_with("openai/gpt-oss-") => {
                    body["reasoning_effort"] = json!("low")
                }
                Some("xai") if s.ai_model == "grok-4.3" => body["reasoning_effort"] = json!("none"),
                Some("deepseek") if s.ai_model == "deepseek-flash" => {
                    body["thinking"] = json!({"type":"disabled"})
                }
                Some("siliconflow") if s.ai_model == "Qwen/Qwen3-8B" => {
                    body["enable_thinking"] = json!(false)
                }
                Some("nvidia") if s.ai_model == "nvidia/nemotron-3-nano-30b-a3b" => {
                    body["chat_template_kwargs"] = json!({"enable_thinking":false})
                }
                _ => {}
            }
            (
                format!("{}/chat/completions", s.ai_base_url.trim_end_matches('/')),
                body,
            )
        }
    }
}

fn answer(v: &Value) -> Result<String> {
    let text = if let Some(text) = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    {
        text.to_owned()
    } else if let Some(parts) = v
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
    {
        parts
            .iter()
            .filter(|p| p["thought"] != true)
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else if let Some(parts) = v["content"].as_array() {
        parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };
    ensure!(
        !text.trim().is_empty(),
        "The provider did not return a text response."
    );
    Ok(text)
}

pub async fn generate(
    s: &Settings,
    key: &str,
    system: &str,
    user: &str,
    limit: u32,
) -> Result<String> {
    if s.ai_provider == "gemini" {
        ensure!(
            !s.ai_model.is_empty()
                && s.ai_model
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
            "Invalid model name"
        );
    }
    let (url, body) = payload(s, system, user, limit);
    let c = integrations::client_for(&url, false).await?;
    let req = c
        .post(url)
        .timeout(std::time::Duration::from_secs(45))
        .json(&body);
    let req = match s.ai_provider.as_str() {
        "gemini" => req.header("x-goog-api-key", key),
        "anthropic" => req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        _ => req.bearer_auth(key),
    };
    let response = req
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Could not reach the AI provider. Please try again."))?;
    let status = response.status().as_u16();
    let message = match status {
        401 | 403 => {
            "The provider could not authorize this key and model. Check the key's access permissions."
        }
        402 => "The provider requires available API credit. Check your provider account.",
        404 | 410 => "The automatic model is unavailable on this provider account.",
        429 => "The provider's usage limit was reached. Check your API quota or try again later.",
        _ => "The provider could not complete the connection. Please try again.",
    };
    ensure!(response.status().is_success(), "{message}");
    // Never expose the provider error body: it can contain credentials or input.
    answer(&integrations::bounded_json(response).await?)
}

/// Chat reuses only an existing, known provider. No caller-selected endpoint,
/// model or tool definitions enter this path.
pub async fn chat_generate(
    settings: &Settings,
    key: &str,
    system: &str,
    user: &str,
    limit: u32,
) -> Result<String> {
    ensure!(selected(settings).is_some(), "Unsupported AI connection");
    ensure!(
        limit <= 500 && user.len() <= 60_000,
        "Chat request exceeds its limit"
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(50),
        generate(settings, key, system, user, limit),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Could not reach the AI provider. Please try again."))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_compatible_provider_is_detected_by_endpoint() {
        let mut s = Settings::default();
        assert_eq!(selected(&s).unwrap().id, "xai");
        s.ai_base_url = "https://api.groq.com/openai/v1".into();
        assert_eq!(selected(&s).unwrap().id, "groq");
        s.ai_base_url = "https://api.x.ai/v1/".into();
        assert_eq!(selected(&s).unwrap().id, "xai");
        s.ai_base_url = "https://example.com/v1".into();
        assert!(selected(&s).is_none());
    }
    #[test]
    fn all_presets_have_distinct_ids_and_valid_https_requests() {
        assert_eq!(PROVIDERS.len(), 6);
        assert_eq!(PROVIDERS[0].id, "xai");
        let mut ids = std::collections::HashSet::new();
        for p in PROVIDERS {
            assert!(ids.insert(p.id));
            let s = Settings {
                ai_provider: p.id.into(),
                ai_base_url: p.base_url.into(),
                ai_model: p.model.into(),
                ..Default::default()
            };
            let (url, body) = payload(&s, "system", "evidence", 1200);
            assert_eq!(url::Url::parse(&url).unwrap().scheme(), "https");
            assert!(!body.to_string().contains("api_key"));
            assert_eq!(selected(&s).unwrap().id, p.id);
            if p.id == "openai" {
                assert_eq!(body["model"], "gpt-6-luna");
                assert_eq!(body["max_completion_tokens"], 1200);
                assert_eq!(body["reasoning_effort"], "none");
                assert!(body.get("max_tokens").is_none());
            }
            if p.id == "xai" {
                assert_eq!(body["reasoning_effort"], "none");
            }
            if p.id == "anthropic" {
                assert_eq!(body["system"], "system");
                assert!(url.ends_with("/messages"));
            }
            if p.id == "gemini" {
                assert_eq!(body["generationConfig"]["maxOutputTokens"], 1200);
                assert_eq!(
                    body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
                    "minimal"
                );
            }
        }
        assert!(provider("custom").is_err());
    }
    #[test]
    fn extracts_visible_text_from_all_protocols() {
        assert_eq!(
            answer(&json!({"choices":[{"message":{"content":"OK"}}]})).unwrap(),
            "OK"
        );
        assert_eq!(answer(&json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"private thinking"},{"text":"OK"}]}}]})).unwrap(),"OK");
        assert_eq!(answer(&json!({"content":[{"type":"thinking","thinking":"private"},{"type":"text","text":"OK"}]})).unwrap(),"OK");
        assert!(answer(&json!({"choices":[{"message":{"content":""}}]})).is_err());
    }
    #[tokio::test]
    async fn invalid_connect_preserves_previous_encrypted_connection() {
        let dir = tempfile::tempdir().unwrap();
        let app = crate::api_email_tests::fixture(dir.path()).await;
        let old = app.settings.read().await.clone();
        persist(&app, old.clone(), "test-only-key").await.unwrap();
        let sealed = app.store.get("secret:ai").await.unwrap();
        assert!(connect(&app, "openai", "").await.is_err());
        assert!(connect(&app, "openai", "invalid\nkey").await.is_err());
        assert!(connect(&app, "unknown", "test-only-key").await.is_err());
        assert_eq!(app.store.get("secret:ai").await.unwrap(), sealed);
        assert_eq!(identity(&*app.settings.read().await), identity(&old));
        assert_eq!(status(&app).await.unwrap()["verified"], true);
        assert_eq!(
            integrations::secret(&app, "ai").await.unwrap(),
            "test-only-key"
        );
    }
}
