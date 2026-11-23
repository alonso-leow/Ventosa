//! Internet research for research nodes: one query to a configurable search
//! provider, returned as numbered sources the AI can cite. Keys come from the
//! environment variable named in the node, never from files.

use serde_json::{json, Value};
use std::time::Duration;

/// (id, name, key env var, needs a base url)
pub static PROVIDERS: &[(&str, &str, &str)] = &[
    ("tavily", "Tavily", "TAVILY_API_KEY"),
    ("brave", "Brave Search", "BRAVE_API_KEY"),
    ("exa", "Exa", "EXA_API_KEY"),
    ("serper", "Serper (Google)", "SERPER_API_KEY"),
    ("perplexity", "Perplexity", "PERPLEXITY_API_KEY"),
    ("searxng", "SearXNG (self-hosted)", "SEARXNG_URL"),
    ("duckduckgo", "DuckDuckGo (no key)", ""),
];

pub fn default_key_env(provider: &str) -> &'static str {
    PROVIDERS.iter().find(|p| p.0 == provider).map(|p| p.2).unwrap_or("")
}

struct Hit {
    title: String,
    url: String,
    text: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build()
}

fn err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(c, r) => format!("HTTP {c}: {}", r.into_string().unwrap_or_default().chars().take(300).collect::<String>()),
        o => o.to_string(),
    }
}

fn key(env: &str) -> Result<String, String> {
    std::env::var(env).ok().filter(|v| !v.is_empty()).ok_or_else(|| format!("environment variable {env} is not set"))
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// Search `query` with `provider` (the key read from `key_env`, default per provider).
pub fn search(provider: &str, key_env: &str, query: &str, max: usize) -> Result<String, String> {
    let max = max.clamp(1, 20);
    let env = if key_env.is_empty() { default_key_env(provider) } else { key_env };
    let a = agent();
    let hits: Vec<Hit> = match provider {
        "tavily" => {
            let r: Value = a.post("https://api.tavily.com/search").send_json(json!({"api_key": key(env)?, "query": query, "max_results": max})).map_err(err)?.into_json().map_err(|e| e.to_string())?;
            r["results"].as_array().into_iter().flatten().map(|x| Hit { title: s(&x["title"]), url: s(&x["url"]), text: s(&x["content"]) }).collect()
        }
        "brave" => {
            let r: Value = a.get("https://api.search.brave.com/res/v1/web/search").query("q", query).query("count", &max.to_string()).set("X-Subscription-Token", &key(env)?).call().map_err(err)?.into_json().map_err(|e| e.to_string())?;
            r["web"]["results"].as_array().into_iter().flatten().map(|x| Hit { title: s(&x["title"]), url: s(&x["url"]), text: s(&x["description"]) }).collect()
        }
        "exa" => {
            let r: Value = a.post("https://api.exa.ai/search").set("x-api-key", &key(env)?).send_json(json!({"query": query, "numResults": max, "contents": {"text": {"maxCharacters": 1200}}})).map_err(err)?.into_json().map_err(|e| e.to_string())?;
            r["results"].as_array().into_iter().flatten().map(|x| Hit { title: s(&x["title"]), url: s(&x["url"]), text: s(&x["text"]) }).collect()
        }
        "serper" => {
            let r: Value = a.post("https://google.serper.dev/search").set("X-API-KEY", &key(env)?).send_json(json!({"q": query, "num": max})).map_err(err)?.into_json().map_err(|e| e.to_string())?;
            r["organic"].as_array().into_iter().flatten().map(|x| Hit { title: s(&x["title"]), url: s(&x["link"]), text: s(&x["snippet"]) }).collect()
        }
        "perplexity" => {
            let r: Value = a.post("https://api.perplexity.ai/chat/completions").set("authorization", &format!("Bearer {}", key(env)?)).send_json(json!({"model": "sonar", "messages": [{"role": "user", "content": query}]})).map_err(err)?.into_json().map_err(|e| e.to_string())?;
            let answer = s(&r["choices"][0]["message"]["content"]);
            let mut v = vec![Hit { title: "answer".into(), url: String::new(), text: answer }];
            v.extend(r["citations"].as_array().into_iter().flatten().take(max).map(|c| Hit { title: "source".into(), url: s(c), text: String::new() }));
            v
        }
        "searxng" => {
            let base = key(env)?;
            let r: Value = a.get(&format!("{}/search", base.trim_end_matches('/'))).query("q", query).query("format", "json").call().map_err(err)?.into_json().map_err(|e| e.to_string())?;
            r["results"].as_array().into_iter().flatten().take(max).map(|x| Hit { title: s(&x["title"]), url: s(&x["url"]), text: s(&x["content"]) }).collect()
        }
        "duckduckgo" => {
            let r: Value = a.get("https://api.duckduckgo.com/").query("q", query).query("format", "json").query("no_html", "1").call().map_err(err)?.into_json().map_err(|e| e.to_string())?;
            let mut v = vec![];
            if !s(&r["AbstractText"]).is_empty() {
                v.push(Hit { title: s(&r["Heading"]), url: s(&r["AbstractURL"]), text: s(&r["AbstractText"]) });
            }
            v.extend(r["RelatedTopics"].as_array().into_iter().flatten().filter(|x| x["Text"].is_string()).take(max).map(|x| Hit { title: String::new(), url: s(&x["FirstURL"]), text: s(&x["Text"]) }));
            v
        }
        other => return Err(format!("unknown search provider {other}")),
    };
    if hits.is_empty() {
        return Ok(format!("No results for \"{query}\" from {provider}."));
    }
    Ok(hits
        .iter()
        .take(max + 1)
        .enumerate()
        .map(|(i, h)| format!("[{}] {}\n{}\n{}", i + 1, h.title, h.url, h.text.chars().take(1200).collect::<String>()))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_and_unknown_provider_are_errors() {
        assert!(search("tavily", "VENTOSA_TEST_NO_SUCH_KEY", "x", 3).unwrap_err().contains("VENTOSA_TEST_NO_SUCH_KEY"));
        assert!(search("nope", "", "x", 3).unwrap_err().contains("unknown"));
        assert_eq!(default_key_env("brave"), "BRAVE_API_KEY");
    }
}
