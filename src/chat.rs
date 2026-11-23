//! Built-in AI chat: a tool-calling loop over the shared tool layer (`tools`).
//! Chats persist as JSON under `$XDG_DATA_HOME/ventosa/chats/`. The caller drives
//! it: `user` a message, `advance` until `Reply`; on `NeedsApproval` show the call,
//! `approve` it, then `advance` again. The caller saves the chat.

use crate::config::{Config, Workspace};
use crate::{default_path, graph, queue, queue_path, tools, Task};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const MAX_ROUNDS: usize = 12;
const MAX_TOKENS: u32 = 16000;
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DENIED_BY_SETTINGS: &str = "denied by ventosa settings ▸ permissions";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub is_error: bool,
}

/// One message. `role` is `user`, `assistant` (text and/or tool calls) or `tool` (results).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Msg {
    pub role: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_results: Vec<ToolResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub workspace: String,
    /// Unix seconds of the last save.
    pub updated: u64,
    /// `text` (sidebar / AI view), `speech` (the speech instance) or `batch` (an executor run).
    pub mode: String,
    pub messages: Vec<Msg>,
    /// The subject and step a batch run works on (usage attribution).
    pub subject: String,
    pub step: String,
    /// Tools this chat may offer; empty = all the settings allow (the launcher's chat sets it from the user's grant).
    pub tools: Vec<String>,
    /// Workspaces the chat's user may touch; empty = every workspace. A tool call naming another one is refused.
    pub scope: Vec<String>,
    /// The last request's reported usage: `{input, output}` tokens (the context bar).
    pub usage: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Reply(String),
    NeedsApproval(ToolCall),
    Error(String),
}

pub fn chats_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("chats")
}

fn chat_path(id: &str) -> PathBuf {
    chats_dir().join(format!("{id}.json"))
}

/// All saved chats, most recently updated first.
pub fn list_chats() -> Vec<Chat> {
    let mut out: Vec<Chat> = fs::read_dir(chats_dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| load_file(&e.path()))
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

fn load_file(path: &PathBuf) -> Option<Chat> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

pub fn load(id: &str) -> Option<Chat> {
    load_file(&chat_path(id))
}

/// Stamp `updated` and write the chat to its file.
pub fn save(chat: &mut Chat) -> io::Result<()> {
    chat.updated = crate::now();
    crate::save(&chat_path(&chat.id), &*chat)
}

pub fn new_chat(workspace: &str) -> Chat {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    Chat { id: format!("chat-{nanos}"), workspace: workspace.into(), ..Chat::default() }
}

/// Append a user message. The first one names an untitled chat.
pub fn user(chat: &mut Chat, text: &str) {
    if chat.title.is_empty() {
        chat.title = text.trim().chars().take(48).collect();
    }
    chat.messages.push(Msg { role: "user".into(), text: text.into(), tool_calls: vec![], tool_results: vec![] });
}

/// Run until the model gives a final text, a tool needs approval, or something fails.
pub fn advance(chat: &mut Chat, cfg: &Config) -> Step {
    let Some(ws) = cfg.workspace(&chat.workspace).cloned() else {
        return Step::Error(format!("no workspace {}; pick one first", chat.workspace));
    };
    for _ in 0..MAX_ROUNDS {
        if let Some(call) = run_pending(chat, cfg) {
            return Step::NeedsApproval(call);
        }
        let (text, calls, usage) = match request(cfg, &ws, chat) {
            Ok(v) => v,
            Err(e) => return Step::Error(e),
        };
        if let Some((i, o)) = crate::usage::of_reply(&json!({"usage": usage})) {
            chat.usage = json!({"input": i, "output": o});
        }
        let done = calls.is_empty();
        chat.messages.push(Msg { role: "assistant".into(), text: text.clone(), tool_calls: calls, tool_results: vec![] });
        if done {
            return Step::Reply(text);
        }
    }
    Step::Error(format!("stopped after {MAX_ROUNDS} tool rounds"))
}

/// Run with nobody to ask: calls whose policy is `ask` are refused and the run goes on.
/// Returns the final reply, or the error.
pub fn run_unattended(chat: &mut Chat, cfg: &Config) -> Result<String, String> {
    for _ in 0..MAX_ROUNDS {
        match advance(chat, cfg) {
            Step::Reply(t) => return Ok(t),
            Step::Error(e) => return Err(e),
            Step::NeedsApproval(call) => {
                let r = ToolResult { id: call.id.clone(), name: call.name.clone(), text: "nobody is here to approve this in an unattended run; continue without it or raise a decision".into(), is_error: true };
                record(chat, r);
            }
        }
    }
    Err("unattended run gave no final reply".into())
}

/// Record the user's decision on a pending call. Allowed calls run now.
pub fn approve(chat: &mut Chat, call: &ToolCall, allow: bool) {
    if !pending_calls(chat).iter().any(|c| c.id == call.id) {
        return;
    }
    let result = if allow {
        run_tool(call)
    } else {
        ToolResult { id: call.id.clone(), name: call.name.clone(), text: "the user denied this".into(), is_error: true }
    };
    record(chat, result);
}

/// Tool calls of the last assistant message that have no result yet.
fn pending_calls(chat: &Chat) -> Vec<ToolCall> {
    let Some(idx) = chat.messages.iter().rposition(|m| m.role == "assistant") else {
        return vec![];
    };
    let answered: Vec<&str> = chat.messages[idx + 1..].iter().flat_map(|m| m.tool_results.iter().map(|r| r.id.as_str())).collect();
    chat.messages[idx].tool_calls.iter().filter(|c| !answered.contains(&c.id.as_str())).cloned().collect()
}

/// Execute pending calls in order: `allow` runs, `deny` records a refusal, `ask` stops and returns the call.
fn run_pending(chat: &mut Chat, cfg: &Config) -> Option<ToolCall> {
    for call in pending_calls(chat) {
        if !granted(chat, &call.name) {
            record(chat, ToolResult { id: call.id.clone(), name: call.name.clone(), text: "this tool is not granted to this user (access ▸ roles)".into(), is_error: true });
            continue;
        }
        if let Some(w) = call.args["workspace"].as_str() {
            if !chat.scope.is_empty() && !chat.scope.iter().any(|s| s == w) {
                record(chat, ToolResult { id: call.id.clone(), name: call.name.clone(), text: format!("workspace {w} is not shared with this user (access \u{25b8} users)"), is_error: true });
                continue;
            }
        }
        match cfg.policy(tools::cap(&call.name)) {
            "deny" => record(chat, ToolResult { id: call.id.clone(), name: call.name.clone(), text: DENIED_BY_SETTINGS.into(), is_error: true }),
            "ask" => return Some(call),
            _ => record(chat, run_tool(&call)),
        }
    }
    None
}

fn record(chat: &mut Chat, result: ToolResult) {
    chat.messages.push(Msg { role: "tool".into(), text: String::new(), tool_calls: vec![], tool_results: vec![result] });
}

fn run_tool(call: &ToolCall) -> ToolResult {
    let r = tools::call(&call.name, &call.args);
    ToolResult {
        id: call.id.clone(),
        name: call.name.clone(),
        text: r["content"][0]["text"].as_str().unwrap_or("").to_string(),
        is_error: r["isError"] == true,
    }
}

/// Tools offered to the model: everything except `deny`.
fn offered(cfg: &Config, chat: &Chat) -> Vec<&'static tools::Tool> {
    tools::TOOLS.iter().filter(|t| cfg.policy(t.cap) != "deny" && granted(chat, t.name)).collect()
}

/// The chat's own tool grant: empty or `*` = every tool.
pub fn granted(chat: &Chat, name: &str) -> bool {
    chat.tools.is_empty() || chat.tools.iter().any(|t| t == "*" || t == name)
}

fn schema_of(t: &tools::Tool) -> Value {
    serde_json::from_str(t.schema).expect("tool schema is valid JSON")
}

/// One provider round trip. Returns the text and the tool calls of the reply.
fn note_usage(model: &str, resp: &Value) {
    if let Some((i, o)) = crate::usage::of_reply(resp) {
        crate::usage::record(model, i, o);
    }
}

fn request(cfg: &Config, ws: &Workspace, chat: &Chat) -> Result<(String, Vec<ToolCall>, Value), String> {
    crate::usage::context(if chat.mode.is_empty() { "chat" } else { &chat.mode }, &chat.workspace, &chat.subject, &chat.step);
    if cfg.provider.kind == "none" {
        return Err("provider is none; pick anthropic or openai-compatible in settings".into());
    }
    // every key of the provider, then the fallback models (settings ▸ AI provider)
    crate::models::with_failover(cfg, &cfg.provider, |p, key| request_with(cfg, ws, chat, p, key))
}

fn request_with(cfg: &Config, ws: &Workspace, chat: &Chat, p: &crate::config::Provider, key: &str) -> Result<(String, Vec<ToolCall>, Value), String> {
    let system = format!("{}\n\n{}", system_prompt(ws, cfg), crate::trusted::prompt_block(ws));
    let msgs: Vec<Msg> = chat
        .messages
        .iter()
        .map(|m| if m.role == "user" { Msg { text: resolve_mentions(&m.text, ws), ..m.clone() } } else { m.clone() })
        .collect();
    let base = p.base_url.trim_end_matches('/');
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(600)).build();
    match p.kind.as_str() {
        "anthropic" => {
            let defs: Vec<Value> = offered(cfg, chat)
                .iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": schema_of(t)}))
                .collect();
            let mut body = json!({"model": p.model, "max_tokens": MAX_TOKENS, "system": system, "messages": anthropic_messages(&msgs)});
            if !defs.is_empty() {
                body["tools"] = json!(defs);
            }
            let resp: Value = agent
                .post(&format!("{base}/v1/messages"))
                .set("x-api-key", key)
                .set("anthropic-version", ANTHROPIC_VERSION)
                .set("content-type", "application/json")
                .send_json(body)
                .map_err(http_err)?
                .into_json()
                .map_err(|e| e.to_string())?;
            note_usage(&p.model, &resp);
            parse_anthropic(&resp).map(|(a, b)| (a, b, resp["usage"].clone()))
        }
        "openai-compatible" => {
            let defs: Vec<Value> = offered(cfg, chat)
                .iter()
                .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": schema_of(t)}}))
                .collect();
            let mut body = json!({"model": p.model, "messages": openai_messages(&system, &msgs)});
            if !defs.is_empty() {
                body["tools"] = json!(defs);
            }
            let resp: Value = agent
                .post(&format!("{base}/chat/completions"))
                .set("authorization", &format!("Bearer {key}"))
                .set("content-type", "application/json")
                .send_json(body)
                .map_err(http_err)?
                .into_json()
                .map_err(|e| e.to_string())?;
            note_usage(&p.model, &resp);
            parse_openai(&resp).map(|(a, b)| (a, b, resp["usage"].clone()))
        }
        "none" => Err("provider is none; pick anthropic or openai-compatible in settings".into()),
        other => Err(format!("provider kind '{other}' cannot chat; pick anthropic or openai-compatible in settings")),
    }
}

fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => format!("HTTP {code}: {}", r.into_string().unwrap_or_default()),
        other => other.to_string(),
    }
}

/// Messages in Anthropic form. Consecutive tool results share one user message.
fn anthropic_messages(msgs: &[Msg]) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for m in msgs {
        match m.role.as_str() {
            "tool" => {
                for r in &m.tool_results {
                    let block = json!({"type": "tool_result", "tool_use_id": r.id, "content": r.text, "is_error": r.is_error});
                    match out.last_mut() {
                        Some(last) if last["role"] == "user" && last["content"].is_array() => {
                            if let Some(arr) = last["content"].as_array_mut() {
                                arr.push(block);
                            }
                        }
                        _ => out.push(json!({"role": "user", "content": [block]})),
                    }
                }
            }
            "assistant" if !m.tool_calls.is_empty() => {
                let mut blocks: Vec<Value> = vec![];
                if !m.text.is_empty() {
                    blocks.push(json!({"type": "text", "text": m.text}));
                }
                for c in &m.tool_calls {
                    blocks.push(json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.args}));
                }
                out.push(json!({"role": "assistant", "content": blocks}));
            }
            role => out.push(json!({"role": role, "content": m.text})),
        }
    }
    out
}

/// Messages in OpenAI chat form, system prompt first.
fn openai_messages(system: &str, msgs: &[Msg]) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];
    for m in msgs {
        match m.role.as_str() {
            "tool" => {
                for r in &m.tool_results {
                    out.push(json!({"role": "tool", "tool_call_id": r.id, "content": r.text}));
                }
            }
            "assistant" => {
                let mut v = json!({"role": "assistant", "content": m.text});
                if !m.tool_calls.is_empty() {
                    v["tool_calls"] = json!(m
                        .tool_calls
                        .iter()
                        .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.args.to_string()}}))
                        .collect::<Vec<_>>());
                }
                out.push(v);
            }
            role => out.push(json!({"role": role, "content": m.text})),
        }
    }
    out
}

/// Text and tool calls of an Anthropic Messages reply.
fn parse_anthropic(resp: &Value) -> Result<(String, Vec<ToolCall>), String> {
    let blocks = resp["content"].as_array().ok_or_else(|| format!("unexpected reply: {resp}"))?;
    let text: String = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
    let calls = blocks
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .map(|b| ToolCall {
            id: b["id"].as_str().unwrap_or("").to_string(),
            name: b["name"].as_str().unwrap_or("").to_string(),
            args: b["input"].clone(),
        })
        .collect();
    Ok((text, calls))
}

/// Text and tool calls of a chat-completions reply. Arguments arrive as a JSON string.
fn parse_openai(resp: &Value) -> Result<(String, Vec<ToolCall>), String> {
    let msg = &resp["choices"][0]["message"];
    if !msg.is_object() {
        return Err(format!("unexpected reply: {resp}"));
    }
    let text = msg["content"].as_str().unwrap_or("").to_string();
    let calls = msg["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| ToolCall {
            id: c["id"].as_str().unwrap_or("").to_string(),
            name: c["function"]["name"].as_str().unwrap_or("").to_string(),
            args: c["function"]["arguments"].as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null),
        })
        .collect();
    Ok((text, calls))
}

/// What is installed and reachable right now, in a few stable lines the AI can rely on: the graph CLI
/// for this workspace, the apps, the MCP servers and tools present or missing, the keyed providers.
/// Built from live state each turn; short on purpose (no descriptions, only names and states).
pub fn environment_block(ws: &Workspace, cfg: &Config) -> String {
    let wj = graph::workspace_json(ws);
    let apps: Vec<String> = ["labelling", "teleprompter", "dashboard", "uploads", "studio", "media-labelling"]
        .iter()
        .map(|a| match wj["ports"][*a].as_u64() {
            Some(p) => format!("{a} :{p}"),
            None => a.to_string(),
        })
        .collect();
    let report = crate::recommend::report(cfg);
    let present: Vec<&str> = report.iter().filter(|(_, ok)| *ok).map(|(r, _)| r.id).collect();
    let missing: Vec<&str> = report.iter().filter(|(_, ok)| !*ok).map(|(r, _)| r.id).collect();
    let mcp = crate::recommend::mcp_names();
    let models: Vec<&str> = crate::models::configured(cfg).iter().map(|i| i.id).collect();
    let search: Vec<&str> = crate::research::PROVIDERS.iter().filter(|p| p.2.is_empty() || std::env::var(p.2).is_ok_and(|v| !v.is_empty())).map(|p| p.0).collect();
    format!(
        "Environment now:\n- graph CLI: `ventosa-graph -w {root} status | attach <step> <ID> | write <command> <ID> ...` (the only write path for pipeline state)\n- apps of this workspace (humans use them): {apps}\n- MCP servers registered: {mcp}\n- tools present: {present}; missing (do not rely on them): {missing}\n- model providers keyed: {models}; search providers usable: {search}",
        root = graph::root(ws).display(),
        apps = apps.join(", "),
        mcp = if mcp.is_empty() { "none".into() } else { mcp.join(", ") },
        present = present.join(", "),
        missing = if missing.is_empty() { "none".into() } else { missing.join(", ") },
        models = if models.is_empty() { "none".into() } else { models.join(", ") },
        search = search.join(", "),
    )
}

fn system_prompt(ws: &Workspace, cfg: &Config) -> String {
    let bio = if cfg.bio.trim().is_empty() { String::new() } else { format!("\nGlobal internal prompt (the user's words): {}\n", cfg.bio.trim()) };
    format!(
        "{}\n\nYou are the assistant built into Ventosa. \
The selected workspace is \"{}\" at {}. Your tools read and change that workspace through Ventosa's tool layer. \
Calls the user has set to ask need their approval; if one is refused, say so and continue without it. \
In user messages, @kind:id or @kind:id.field is a reference to a Ventosa object (step, skill, item, task, file, help, workspace); its content is given below the message under \"References:\". \
Use it as the source, do not ask the user to paste it. Answer briefly.{bio}\n\n{env}\n\nElevate to the human (decision_ask):\n{}",
        crate::help::INTERNAL_PROMPT,
        ws.name,
        graph::root(ws).display(),
        graph::elevate_text(ws, ""),
        env = environment_block(ws, cfg),
    )
}

/// Resolve `@step:<id>[.field]`, `@skill:<id>`, `@item:<id>`, `@task:<id>` and `@workspace`.
/// Appends a "References:" block with each distinct reference's content.
pub fn resolve_mentions(text: &str, ws: &Workspace) -> String {
    let mut seen: Vec<String> = vec![];
    let mut blocks: Vec<String> = vec![];
    for tok in mention_tokens(text) {
        if seen.contains(&tok) {
            continue;
        }
        let body = lookup(&tok, ws);
        blocks.push(format!("{tok}:\n{body}"));
        seen.push(tok);
    }
    if blocks.is_empty() {
        return text.to_string();
    }
    format!("{text}\n\nReferences:\n{}", blocks.join("\n\n"))
}

/// Valid `@…` references in a text, in order.
fn mention_tokens(text: &str) -> Vec<String> {
    text.match_indices('@')
        .filter_map(|(i, _)| {
            let tail: String = text[i + 1..].chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.' | '/')).collect();
            let tok = tail.trim_end_matches('.');
            valid_token(tok).then(|| format!("@{tok}"))
        })
        .collect()
}

fn valid_token(t: &str) -> bool {
    if t == "workspace" || t == "help" {
        return true;
    }
    match t.split_once(':') {
        Some((kind, rest)) => matches!(kind, "step" | "skill" | "item" | "task" | "file" | "help") && !rest.is_empty(),
        None => false,
    }
}

fn render(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// The content of one reference token (`@kind:id[.field]` or `@workspace`).
fn lookup(tok: &str, ws: &Workspace) -> String {
    let body = tok.trim_start_matches('@');
    if body == "workspace" {
        return render(&graph::workspace_json(ws));
    }
    if body == "help" || body.starts_with("help:") {
        let cfg = crate::config::load(&crate::config::default_path());
        let step = body.strip_prefix("help:").unwrap_or("");
        return crate::help::explain(&cfg, ws, step).unwrap_or_else(|e| e);
    }
    if let Some(rel) = body.strip_prefix("file:") {
        if std::path::Path::new(rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return format!("(not inside the workspace: {rel})");
        }
        let p = graph::root(ws).join(rel);
        if p.is_dir() {
            return fs::read_dir(&p).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        }
        return match fs::read(&p) {
            Ok(b) => String::from_utf8_lossy(&b[..b.len().min(64 * 1024)]).to_string(),
            Err(_) => format!("(not found: {tok})"),
        };
    }
    let Some((kind, rest)) = body.split_once(':') else {
        return format!("(bad reference {tok})");
    };
    let (id, field) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    if kind == "skill" {
        let path = graph::root(ws).join("pipeline").join("skill-graph").join("nodes").join(format!("{id}.md"));
        return fs::read_to_string(path).unwrap_or_else(|_| format!("(not found: {tok})"));
    }
    let found: Option<Value> = match kind {
        "step" => graph::steps(ws).ok().and_then(|g| g["steps"].as_array().and_then(|a| a.iter().find(|s| s["id"] == id).cloned())),
        "item" => crate::load::<Vec<queue::Item>>(&queue_path())
            .into_iter()
            .find(|i| i.id.to_string() == id)
            .and_then(|i| serde_json::to_value(i).ok()),
        _ => crate::load::<Vec<Task>>(&default_path())
            .into_iter()
            .find(|t| t.id.to_string() == id)
            .and_then(|t| serde_json::to_value(t).ok()),
    };
    match (found, field) {
        (None, _) => format!("(not found: {tok})"),
        (Some(v), None) => render(&v),
        (Some(v), Some(f)) => v.get(f).map_or_else(|| format!("(no field {f})"), render),
    }
}

/// Autocomplete candidates `(token, label)` for a workspace: steps, step fields,
/// skills, items, tasks and `@workspace`.
pub fn mention_candidates(cfg: &Config, workspace: &str) -> Vec<(String, String)> {
    let mut out = vec![("@workspace".to_string(), "workspace".to_string()), ("@help".to_string(), "help · what to do now".to_string())];
    if let Some(ws) = cfg.workspace(workspace) {
        let root = graph::root(ws);
        for top in fs::read_dir(&root).into_iter().flatten().flatten() {
            let name = top.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            out.push((format!("@file:{name}"), format!("file {name}{}", if top.path().is_dir() { "/" } else { "" })));
        }
        if let Ok(g) = graph::steps(ws) {
            for s in g["steps"].as_array().into_iter().flatten() {
                let Some(id) = s["id"].as_str() else { continue };
                let need = s["need"].as_str().unwrap_or("");
                out.push((format!("@step:{id}"), format!("step {id} · {need}")));
                for f in ["owner", "need", "done_when", "attach", "write"] {
                    out.push((format!("@step:{id}.{f}"), format!("{id}.{f}")));
                }
                out.push((format!("@skill:{id}"), format!("skill {id}")));
                out.push((format!("@help:{id}"), format!("help {id} · what to do here")));
            }
        }
    }
    for i in crate::load::<Vec<queue::Item>>(&queue_path()).into_iter().filter(|i| i.payload["workspace"] == workspace) {
        out.push((format!("@item:{}", i.id), format!("item {} · {}", i.id, i.title)));
    }
    for t in crate::load::<Vec<Task>>(&default_path()) {
        out.push((format!("@task:{}", t.id), format!("task {} · {}", t.id, t.title)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn ws_in(dir: &PathBuf) -> Workspace {
        Workspace { name: "w".into(), root: dir.display().to_string(), engine: "ventosa".into(), enabled: true }
    }

    fn cfg_with(run: &str, key_env: &str) -> Config {
        let mut cfg = Config {
            provider: Provider { kind: "anthropic".into(), api_key_env: key_env.into(), ..Provider::default() },
            workspaces: vec![Workspace { name: "w".into(), root: "/nonexistent".into(), ..Workspace::default() }],
            ..Config::default()
        };
        cfg.permissions.run = run.into();
        cfg
    }

    #[test]
    fn chat_roundtrips_through_json() {
        let mut chat = new_chat("w");
        user(&mut chat, "  hello there  ");
        chat.messages.push(Msg {
            role: "assistant".into(),
            text: String::new(),
            tool_calls: vec![ToolCall { id: "t1".into(), name: "list_tasks".into(), args: json!({}) }],
            tool_results: vec![],
        });
        let back: Chat = serde_json::from_str(&serde_json::to_string(&chat).unwrap()).unwrap();
        assert_eq!(back, chat);
        assert_eq!(back.title, "hello there");
    }

    #[test]
    fn mention_tokens_ignore_plain_at_signs() {
        let toks = mention_tokens("see @step:drafts.need, @skill:x. mail a@b.com @task:4 @workspace @nope:1");
        assert_eq!(toks, ["@step:drafts.need", "@skill:x", "@task:4", "@workspace"]);
    }

    #[test]
    fn references_resolve_steps_fields_and_skills() {
        let dir = std::env::temp_dir().join(format!("ventosa-chat-{}", std::process::id()));
        let sg = dir.join("pipeline").join("skill-graph");
        fs::create_dir_all(sg.join("nodes")).unwrap();
        fs::write(sg.join("steps.json"), r#"{"steps":[{"id":"drafts","owner":"ai","need":"write it"}]}"#).unwrap();
        fs::write(sg.join("nodes").join("drafts.md"), "# Drafts skill").unwrap();
        let ws = ws_in(&dir);
        let out = resolve_mentions("make @step:drafts.need and @skill:drafts", &ws);
        assert!(out.contains("References:"));
        assert!(out.contains("@step:drafts.need:\nwrite it"));
        assert!(out.contains("# Drafts skill"));
        assert_eq!(resolve_mentions("plain", &ws), "plain");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn anthropic_tool_results_share_one_user_message() {
        let msgs = vec![
            Msg { role: "user".into(), text: "go".into(), tool_calls: vec![], tool_results: vec![] },
            Msg {
                role: "assistant".into(),
                text: String::new(),
                tool_calls: vec![ToolCall { id: "a".into(), name: "x".into(), args: json!({}) }, ToolCall { id: "b".into(), name: "y".into(), args: json!({}) }],
                tool_results: vec![],
            },
            Msg { role: "tool".into(), text: String::new(), tool_calls: vec![], tool_results: vec![ToolResult { id: "a".into(), name: "x".into(), text: "1".into(), is_error: false }] },
            Msg { role: "tool".into(), text: String::new(), tool_calls: vec![], tool_results: vec![ToolResult { id: "b".into(), name: "y".into(), text: "2".into(), is_error: true }] },
        ];
        let out = anthropic_messages(&msgs);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1]["content"][1]["type"], "tool_use");
        assert_eq!(out[2]["content"].as_array().unwrap().len(), 2);
        assert_eq!(out[2]["content"][1]["is_error"], true);
    }

    #[test]
    fn parses_both_reply_shapes() {
        let a = json!({"content": [{"type": "text", "text": "hi"}, {"type": "tool_use", "id": "t1", "name": "list_tasks", "input": {"filter": "all"}}]});
        let (text, calls) = parse_anthropic(&a).unwrap();
        assert_eq!(text, "hi");
        assert_eq!(calls, vec![ToolCall { id: "t1".into(), name: "list_tasks".into(), args: json!({"filter": "all"}) }]);

        let o = json!({"choices": [{"message": {"content": null, "tool_calls": [{"id": "c1", "function": {"name": "tool_run", "arguments": "{\"name\":\"x\"}"}}]}}]});
        let (text, calls) = parse_openai(&o).unwrap();
        assert_eq!(text, "");
        assert_eq!(calls[0].args, json!({"name": "x"}));
        assert!(parse_openai(&json!({})).is_err());
    }

    #[test]
    fn denied_calls_are_refused_and_never_offered() {
        let mut cfg = cfg_with("ask", "VENTOSA_TEST_KEY_UNSET");
        cfg.permissions.write = "deny".into();
        assert!(offered(&cfg, &Chat::default()).iter().all(|t| t.cap != tools::Cap::Write));
        let mut chat = new_chat("w");
        chat.messages.push(Msg {
            role: "assistant".into(),
            text: String::new(),
            tool_calls: vec![ToolCall { id: "x".into(), name: "add_task".into(), args: json!({"title": "t"}) }],
            tool_results: vec![],
        });
        assert_eq!(run_pending(&mut chat, &cfg), None);
        let last = chat.messages.last().unwrap();
        assert!(last.tool_results[0].is_error);
        assert_eq!(last.tool_results[0].text, DENIED_BY_SETTINGS);
    }

    #[test]
    fn ask_stops_and_a_refusal_is_recorded() {
        let cfg = cfg_with("ask", "VENTOSA_TEST_KEY_UNSET");
        let mut chat = new_chat("w");
        chat.messages.push(Msg {
            role: "assistant".into(),
            text: String::new(),
            tool_calls: vec![ToolCall { id: "r".into(), name: "tool_run".into(), args: json!({"workspace": "w", "name": "x"}) }],
            tool_results: vec![],
        });
        let call = run_pending(&mut chat, &cfg).expect("ask returns the call");
        approve(&mut chat, &call, false);
        assert!(run_pending(&mut chat, &cfg).is_none());
        assert_eq!(chat.messages.last().unwrap().tool_results[0].text, "the user denied this");
    }

    #[test]
    fn missing_key_is_a_clear_error() {
        let cfg = cfg_with("allow", "VENTOSA_TEST_KEY_UNSET");
        let mut chat = new_chat("w");
        user(&mut chat, "hi");
        match advance(&mut chat, &cfg) {
            Step::Error(e) => assert!(e.contains("VENTOSA_TEST_KEY_UNSET"), "{e}"),
            other => panic!("expected error, got {other:?}"),
        }
    }
}
