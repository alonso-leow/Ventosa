//! Providers and their common model lists. A node picks `provider:model`; the
//! providers offered are the ones configured (their key is in the environment,
//! or they are the app's own provider).

use crate::config::{Config, Provider};
use serde_json::json;

pub struct Info {
    pub id: &'static str,
    pub name: &'static str,
    /// `anthropic` or `openai-compatible`.
    pub kind: &'static str,
    pub base_url: &'static str,
    pub key_env: &'static str,
    pub models: &'static [&'static str],
}

pub static CATALOG: &[Info] = &[
    Info { id: "anthropic", name: "Anthropic", kind: "anthropic", base_url: "https://api.anthropic.com", key_env: "ANTHROPIC_API_KEY",
        models: &["claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"] },
    Info { id: "openai", name: "OpenAI", kind: "openai-compatible", base_url: "https://api.openai.com/v1", key_env: "OPENAI_API_KEY",
        models: &["gpt-5", "gpt-5-mini", "gpt-5-nano", "gpt-4.1", "gpt-4.1-mini", "o4-mini", "o3"] },
    Info { id: "openrouter", name: "OpenRouter", kind: "openai-compatible", base_url: "https://openrouter.ai/api/v1", key_env: "OPENROUTER_API_KEY",
        models: &["anthropic/claude-opus-5.5", "anthropic/claude-sonnet-5.5", "openai/gpt-5", "google/gemini-2.5-pro", "google/gemini-2.5-flash", "deepseek/deepseek-chat", "meta-llama/llama-3.3-70b-instruct", "qwen/qwen3-235b-a22b"] },
    Info { id: "deepinfra", name: "DeepInfra", kind: "openai-compatible", base_url: "https://api.deepinfra.com/v1/openai", key_env: "DEEPINFRA_API_KEY",
        models: &["deepseek-ai/DeepSeek-V3", "deepseek-ai/DeepSeek-R1", "meta-llama/Llama-3.3-70B-Instruct", "Qwen/Qwen3-235B-A22B", "moonshotai/Kimi-K2-Instruct"] },
    Info { id: "google", name: "Google Gemini", kind: "openai-compatible", base_url: "https://generativelanguage.googleapis.com/v1beta/openai", key_env: "GEMINI_API_KEY",
        models: &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite"] },
    Info { id: "groq", name: "Groq", kind: "openai-compatible", base_url: "https://api.groq.com/openai/v1", key_env: "GROQ_API_KEY",
        models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "openai/gpt-oss-120b", "qwen/qwen3-32b"] },
    Info { id: "mistral", name: "Mistral", kind: "openai-compatible", base_url: "https://api.mistral.ai/v1", key_env: "MISTRAL_API_KEY",
        models: &["mistral-large-latest", "mistral-medium-latest", "mistral-small-latest", "codestral-latest"] },
];

fn set(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| !v.is_empty())
}

/// The providers offered in a node's model menu: every catalog provider whose key is
/// set, plus the app's own provider (as `default`) when it is not a catalog one.
pub fn configured(cfg: &Config) -> Vec<&'static Info> {
    CATALOG.iter().filter(|i| set(i.key_env) || (cfg.provider.api_key_env == i.key_env && cfg.provider.kind != "none")).collect()
}

/// The models of a provider: the common list, plus the app's own model when it is this provider's.
pub fn models_of(cfg: &Config, info: &Info) -> Vec<String> {
    let mut v: Vec<String> = info.models.iter().map(|m| m.to_string()).collect();
    if cfg.provider.api_key_env == info.key_env && !cfg.provider.model.is_empty() && !v.contains(&cfg.provider.model) {
        v.insert(0, cfg.provider.model.clone());
    }
    v
}

/// `provider:model` → the provider settings to run with; `None` for the app default.
pub fn resolve(cfg: &Config, spec: &str) -> Option<Provider> {
    let (pid, model) = spec.split_once(':')?;
    let info = CATALOG.iter().find(|i| i.id == pid)?;
    Some(Provider { kind: info.kind.into(), base_url: info.base_url.into(), model: model.into(), api_key_env: info.key_env.into() })
        .map(|p| if cfg.provider.api_key_env == info.key_env && !cfg.provider.base_url.is_empty() { Provider { base_url: cfg.provider.base_url.clone(), ..p } } else { p })
}

/// How a spec reads in a node: `Anthropic · claude-opus-5-5`, or `default`.
pub fn label(spec: &str) -> String {
    match spec.split_once(':').and_then(|(p, m)| CATALOG.iter().find(|i| i.id == p).map(|i| format!("{} · {m}", i.name))) {
        Some(s) => s,
        None => "app default".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_spec_and_falls_back() {
        let cfg = Config::default();
        let p = resolve(&cfg, "openai:gpt-5").unwrap();
        assert_eq!((p.kind.as_str(), p.model.as_str(), p.api_key_env.as_str()), ("openai-compatible", "gpt-5", "OPENAI_API_KEY"));
        assert!(resolve(&cfg, "").is_none());
        assert!(resolve(&cfg, "nope:x").is_none());
        assert_eq!(label("anthropic:claude-opus-5-5"), "Anthropic · claude-opus-5-5");
    }
}

// ---------------------------------------------------------------- failover (after OpenClaw's model-failover)

/// A key env may name several variables ("ANTHROPIC_API_KEY,ANTHROPIC_API_KEY_2"): the keys that are set, in order.
pub fn keys(env_list: &str) -> Vec<String> {
    env_list.split(',').map(str::trim).filter(|e| !e.is_empty()).filter_map(|e| std::env::var(e).ok()).filter(|v| !v.is_empty()).collect()
}

/// How a failed request counts: another key may help (`rate limit`, `auth`), another model may help
/// (`overloaded`, `timeout`, `model`), or nothing will (`None`: bad request, refusal, bad config).
pub fn failure(err: &str) -> Option<&'static str> {
    let e = err.to_lowercase();
    if e.contains("http 429") || e.contains("rate limit") || e.contains("quota") || e.contains("insufficient") {
        Some("rate limit")
    } else if e.contains("http 401") || e.contains("http 403") || e.contains("invalid api key") || e.contains("authentication") {
        Some("auth")
    } else if e.contains("http 404") || e.contains("model_not_found") || e.contains("model not found") || e.contains("does not exist") {
        Some("model")
    } else if e.contains("http 5") || e.contains("overloaded") {
        Some("overloaded")
    } else if e.contains("timed out") || e.contains("timeout") || e.contains("connection") {
        Some("timeout")
    } else {
        None
    }
}

/// Run `f` on the primary provider with each of its keys (a key that is rate limited or refused hands over to the
/// next), then on each fallback model (`cfg.fallbacks`, `provider:model`) with that provider's keys. The error
/// of the last attempt is returned when all fail; errors no other key or model can fix stop at once.
pub fn with_failover<T>(cfg: &Config, primary: &Provider, mut f: impl FnMut(&Provider, &str) -> Result<T, String>) -> Result<T, String> {
    let mut chain: Vec<Provider> = vec![primary.clone()];
    chain.extend(cfg.fallbacks.iter().filter_map(|spec| resolve(cfg, spec)));
    let mut last = format!("environment variable {} is not set", primary.api_key_env);
    for (n, p) in chain.iter().enumerate() {
        let keys = keys(&p.api_key_env);
        if keys.is_empty() {
            last = format!("environment variable {} is not set", p.api_key_env);
            continue;
        }
        for key in &keys {
            match f(p, key) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let class = failure(&e);
                    last = if n > 0 { format!("{} (fallback {})", e, p.model) } else { e };
                    match class {
                        Some("rate limit") | Some("auth") => continue, // the next key
                        Some(_) => break,                              // the next model
                        None => return Err(last),
                    }
                }
            }
        }
    }
    Err(last)
}

/// A cheap live check of a provider: one tiny request per key; the answer names the failure class.
pub fn test(p: &Provider) -> Result<String, String> {
    let keys = keys(&p.api_key_env);
    if keys.is_empty() {
        return Err(format!("{} is not set", p.api_key_env));
    }
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).build();
    let base = p.base_url.trim_end_matches('/');
    let t0 = std::time::Instant::now();
    let mut ok = 0;
    let mut bad = vec![];
    for (i, key) in keys.iter().enumerate() {
        let r = match p.kind.as_str() {
            "anthropic" => agent.post(&format!("{base}/v1/messages")).set("x-api-key", key).set("anthropic-version", "2023-06-01").send_json(json!({"model": p.model, "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]})),
            _ => agent.post(&format!("{base}/chat/completions")).set("authorization", &format!("Bearer {key}")).send_json(json!({"model": p.model, "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]})),
        };
        match r {
            Ok(_) => ok += 1,
            Err(ureq::Error::Status(c, resp)) => bad.push(format!("key {}: HTTP {c} ({}) {}", i + 1, failure(&format!("HTTP {c}")).unwrap_or("rejected"), resp.into_string().unwrap_or_default().chars().take(120).collect::<String>())),
            Err(e) => bad.push(format!("key {}: {e}", i + 1)),
        }
    }
    let line = format!("{ok}/{} keys work · {} ms · {}", keys.len(), t0.elapsed().as_millis(), p.model);
    if bad.is_empty() { Ok(line) } else { Err(format!("{line}\n{}", bad.join("\n"))) }
}

#[cfg(test)]
mod failover_tests {
    use super::*;

    #[test]
    fn failures_are_classed() {
        assert_eq!(failure("HTTP 429: slow down"), Some("rate limit"));
        assert_eq!(failure("HTTP 401: bad"), Some("auth"));
        assert_eq!(failure("HTTP 503"), Some("overloaded"));
        assert_eq!(failure("HTTP 400: bad request"), None);
    }

    #[test]
    fn keys_rotate_then_the_fallback_model_runs() {
        unsafe {
            std::env::set_var("VTEST_KEY_A", "a");
            std::env::set_var("VTEST_KEY_B", "b");
            std::env::set_var("VTEST_FB_KEY", "c");
        }
        let primary = Provider { kind: "anthropic".into(), base_url: "x".into(), model: "m1".into(), api_key_env: "VTEST_KEY_A,VTEST_KEY_B".into() };
        let mut cfg = Config::default();
        cfg.fallbacks = vec!["nope:x".into()];
        let mut seen = vec![];
        let r = with_failover(&cfg, &primary, |p, k| {
            seen.push(format!("{}:{k}", p.model));
            if k == "a" { Err("HTTP 429: limit".into()) } else { Ok(k.to_string()) }
        });
        assert_eq!(r.unwrap(), "b");
        assert_eq!(seen, ["m1:a", "m1:b"]);
        let bad = with_failover(&cfg, &primary, |_, _| Err::<(), _>("HTTP 400: nope".into()));
        assert!(bad.is_err());
    }
}
