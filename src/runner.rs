//! Scheduler (cron) and runners.
//!
//! A schedule fires every N minutes. It claims the oldest queued `ai` item at its
//! node for its workspace and runs it:
//! - `command`: a shell command (an MCP client or CLI agent such as `claude -p …`,
//!   `opencode run …`), cwd = workspace root, output → `runs/<item>.log`.
//!   Exit 0 → `review` (the next sync marks it done once the subject moves on).
//! - `provider`: one API call (Anthropic Messages or an OpenAI-compatible chat
//!   endpoint). Prompt = node skill + knowledge files + subject. Output →
//!   `runs/<item>.md`, item → `review`: a human or agent applies it with the
//!   node's write command (the engine's write gate stays in charge).

use crate::config::{Config, Provider, Schedule, Workspace};
use crate::graph::{self, runs_dir};
use crate::queue::{self, Item};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const ANTHROPIC_VERSION: &str = "2026-10-21";
const MAX_KNOWLEDGE_BYTES: usize = 200_000;

fn state_path() -> PathBuf {
    crate::default_path().with_file_name("schedules-state.json")
}

/// Last run time per schedule id (kept apart from config so the daemon never rewrites config).
pub fn last_runs() -> HashMap<String, u64> {
    crate::load(&state_path())
}

fn mark_run(id: &str) {
    let mut m = last_runs();
    m.insert(id.to_string(), crate::now());
    let _ = crate::save(&state_path(), &m);
}

/// The shortest period a schedule may have: agent runs cost a model loop, so 5 minutes (as zeroclaw warns).
pub fn min_minutes(s: &Schedule) -> u64 {
    if s.runner == "agent" { 5 } else { 1 }
}

pub fn due(s: &Schedule, last: &HashMap<String, u64>) -> bool {
    s.enabled && crate::now().saturating_sub(*last.get(&s.id).unwrap_or(&0)) >= s.every_minutes.max(min_minutes(s)) * 60
}

fn runs_path() -> PathBuf {
    state_path().with_file_name("schedule-runs.jsonl")
}

/// One finished run, appended to the run history (zeroclaw's cron_runs): who, which item, how it ended.
fn record_run(s: &Schedule, item: &Item, started: u64, status: &Result<String, String>) {
    use std::io::Write;
    let (st, out) = match status {
        Ok(t) => ("ok", t.as_str()),
        Err(e) => ("error", e.as_str()),
    };
    let row = json!({"at": started, "schedule": s.id, "item": item.id, "title": item.title, "step": item.node, "status": st, "secs": crate::now().saturating_sub(started), "output": out.chars().take(2000).collect::<String>()});
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(runs_path()) {
        let _ = writeln!(f, "{row}");
    }
    // keep the last 500 rows
    if let Ok(text) = std::fs::read_to_string(runs_path()) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > 600 {
            let _ = std::fs::write(runs_path(), lines[lines.len() - 500..].join("\n") + "\n");
        }
    }
}

/// The most recent runs, newest first.
pub fn recent_runs(limit: usize) -> Vec<Value> {
    let text = std::fs::read_to_string(runs_path()).unwrap_or_default();
    text.lines().rev().take(limit).filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Seconds since the scheduler last ran a pass (the app or the daemon), None when it never did.
pub fn heartbeat_age() -> Option<u64> {
    let t: Option<u64> = crate::load(&state_path().with_file_name("schedules-heartbeat.json"));
    t.map(|t| crate::now().saturating_sub(t))
}

/// Fire every due schedule once. Returns a line per fired schedule.
/// `block`: run to completion here (daemon); otherwise each run gets a thread (app).
pub fn tick(cfg: &Config, block: bool) -> Vec<String> {
    let _ = crate::save(&state_path().with_file_name("schedules-heartbeat.json"), &crate::now());
    let last = last_runs();
    let mut out = vec![];
    for s in cfg.schedules.iter().filter(|s| due(s, &last)) {
        mark_run(&s.id);
        match fire(cfg, s, block) {
            Ok(Some(msg)) => out.push(msg),
            Ok(None) => {}
            Err(e) => out.push(format!("{}: {e}", s.id)),
        }
    }
    out
}

/// Run one schedule now (also used by "run now" in the UI).
/// `task: node` claims the oldest queued item at the node whose retry time has come;
/// `task: graph` opens an entry of its own (so the run shows in History like any manual entry).
/// A schedule never runs twice at once: while one of its items is running, a tick does nothing.
/// Fire a job on its schedule (trigger `cron`).
pub fn fire(cfg: &Config, s: &Schedule, block: bool) -> Result<Option<String>, String> {
    fire_as(cfg, s, block, "cron")
}

/// Fire a job for a reason: `cron` (its schedule), `entry` (a human entry hook) or `manual` (Run now).
pub fn fire_as(cfg: &Config, s: &Schedule, block: bool, trigger: &str) -> Result<Option<String>, String> {
    let ws = cfg.workspace(&s.workspace).cloned().ok_or_else(|| format!("no workspace {}", s.workspace))?;
    let qpath = crate::queue_path();
    let mut items: Vec<Item> = crate::load(&qpath);
    let me = format!("schedule:{}", s.id);
    if items.iter().any(|i| i.status == "running" && i.payload["agent"] == me.as_str()) {
        return Ok(None);
    }
    let now = crate::now();
    let pos = if s.task == "graph" {
        let item = queue::enqueue(&mut items, &format!("cron {} · graph", s.id), "ai", &format!("cron:{}", s.id), None, json!({"workspace": ws.name, "subject": ""}));
        items.iter().position(|i| i.id == item.id)
    } else {
        items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.status == "queued" && i.kind == "ai" && i.node.as_deref() == Some(&s.node) && i.payload["workspace"] == ws.name && i.payload["retry_at"].as_u64().unwrap_or(0) <= now)
            .min_by_key(|(_, i)| i.created)
            .map(|(p, _)| p)
    };
    let Some(pos) = pos else { return Ok(None) };
    items[pos].status = "running".into();
    items[pos].updated = now;
    items[pos].payload["agent"] = json!(me);
    items[pos].payload["trigger"] = json!(trigger);
    let item = items[pos].clone();
    crate::save(&qpath, &items).map_err(|e| e.to_string())?;

    let label = format!("{} → item {} ({})", s.id, item.id, item.title);
    let s = s.clone();
    let cfg = cfg.clone();
    let job = move || {
        let started = crate::now();
        let status = match s.runner.as_str() {
            "provider" => run_provider(&node_provider(&cfg, &ws, &item), &ws, &item).map(|_| "provider reply in runs/".to_string()),
            "agent" => run_agent(&cfg, &s, &ws, &item),
            _ => run_template(&s.command, &format!("schedule-{}", s.id), &ws, &item),
        };
        record_run(&s, &item, started, &status);
        let mut items: Vec<Item> = crate::load(&qpath);
        if let Some(i) = items.iter_mut().find(|i| i.id == item.id) {
            i.updated = crate::now();
            match &status {
                Ok(note) => {
                    i.status = "review".into();
                    i.payload["result"] = json!(note.chars().take(2000).collect::<String>());
                    i.payload["error"] = Value::Null;
                }
                Err(e) => {
                    // zeroclaw's retry: exponential backoff with jitter; a policy refusal never retries
                    let attempt = i.payload["attempts"].as_u64().unwrap_or(0) + 1;
                    i.payload["attempts"] = json!(attempt);
                    i.payload["error"] = json!(e);
                    let policy = e.starts_with("blocked by policy");
                    if policy || s.task == "graph" || attempt > cfg.limits.retries as u64 {
                        i.status = "failed".into();
                    } else {
                        let wait = cfg.limits.backoff_secs.max(1) << (attempt - 1).min(10);
                        let jitter = u64::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_millis()).unwrap_or(0) % 250) / 50;
                        i.status = "queued".into();
                        i.payload["retry_at"] = json!(crate::now() + wait + jitter);
                    }
                }
            }
        }
        let _ = crate::save(&qpath, &items);
        let _ = graph::sync(&ws, &qpath);
        status.map(|_| ())
    };
    if block {
        job().map(|_| Some(label))
    } else {
        std::thread::spawn(job);
        Ok(Some(label))
    }
}

/// The cron job's skill file: its own instructions, including the policy on which themes it
/// chooses. One markdown file per job, next to the config.
pub fn cron_skill_path(id: &str) -> std::path::PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    crate::default_path().with_file_name("cron-skills").join(format!("{safe}.md"))
}

pub const DEFAULT_CRON_SKILL: &str = "# Cron skill\n\nWhat this job does each time it runs, in its one phase.\n\n## Theme policy\nThemes we usually choose (the job picks from these, in this order of preference):\n- (one theme per line: the topics, angles and formats that work for this channel)\n\nAvoid:\n- (themes to skip: sensitive, overdone, off-brand)\n\nWhen to pick a theme:\n- Only take a theme when the subject at this phase does not already carry one.\n- Two subjects must not share a theme in the same batch.\n- If none of the themes fits, call decision_ask with the closest options; never invent a theme outside this policy.\n";

pub fn cron_skill(id: &str) -> String {
    std::fs::read_to_string(cron_skill_path(id)).unwrap_or_else(|_| DEFAULT_CRON_SKILL.to_string())
}

pub fn save_cron_skill(id: &str, text: &str) -> Result<(), String> {
    let p = cron_skill_path(id);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
}

/// The provider a node runs on: its own `model` (provider:model) when set, else the app's.
pub fn node_provider(cfg: &Config, ws: &Workspace, item: &Item) -> Provider {
    item.node
        .as_deref()
        .and_then(|n| graph::node(ws, n).ok())
        .and_then(|n| n["model"].as_str().and_then(|m| crate::models::resolve(cfg, m)))
        .unwrap_or_else(|| cfg.provider.clone())
}

/// The autonomous run: an isolated chat loop (the same loop as the chat interface, with nobody
/// to ask) whose first message is built from the meta graph, the job's own prompt, its one phase
/// and the output policy. Tools: the unattended default set, narrowed by `allowed_tools`;
/// graph edits only for `task: graph`.
fn run_agent(cfg: &Config, s: &Schedule, ws: &Workspace, item: &Item) -> Result<String, String> {
    let mut cfg = cfg.clone();
    cfg.provider = node_provider(&cfg, ws, item);
    let p = &mut cfg.permissions;
    p.read = "allow".into();
    p.write = "allow".into();
    p.graph = if s.task == "graph" { "allow".into() } else { "deny".into() };
    p.install = "deny".into();
    p.apps = "deny".into();
    p.run = "deny".into();
    let agent = format!("{}-{}", match item.payload["trigger"].as_str() { Some("entry") => "entry", Some("manual") => "manual", _ => "cron" }, s.id);
    let step = item.node.clone().unwrap_or_default();
    let subject = item.payload["subject"].as_str().unwrap_or("").to_string();
    let phase = if s.task == "graph" {
        format!("Phase: the graph itself. 1. help (workspace {ws}): read the meta graph first, it is the zero ring. 2. graph_steps and graph_node: read the steps, owners, needs and skills. 3. Improve what your job prompt asks: change steps or skills only through graph_step_set / graph_step_add / graph_step_remove, one small change at a time. 4. For anything that removes a step or changes an owner, raise decision_ask instead of changing it.", ws = ws.name)
    } else {
        format!("Phase: node {step}, subject {subject} ({title}). 1. help (workspace {ws}): read the meta graph first, it is the zero ring. 2. pipeline_attach (workspace {ws}, step {step}, subject {subject}, agent {agent}). 3. graph_node {step}: follow its skill exactly and read the knowledge it lists. 4. Finish with pipeline_write using the node's write command (args as a list, without `ventosa-graph write`). Do nothing outside this one phase. If the skill's elevation rules put a decision with the human, call decision_ask and stop. Queue detail: {detail}",
            ws = ws.name, title = item.title, detail = item.payload["detail"].as_str().unwrap_or(""))
    };
    let policy = if s.output_policy.trim().is_empty() { crate::config::DEFAULT_OUTPUT_POLICY } else { s.output_policy.trim() };
    let prompt = format!(
        "[cron:{id}] You run alone inside a scheduled job: an isolated session, no human present, nobody to approve calls.\n\n{phase}\n\nJob skill (its policy, including the themes it may choose):\n{skill}\n\nJob prompt: {job}\n\nFinal output policy: {policy}",
        id = s.id,
        skill = cron_skill(&s.id).trim(),
        job = if s.prompt.trim().is_empty() { "(none: follow the node skill)" } else { s.prompt.trim() },
    );
    let mut chat = crate::chat::new_chat(&ws.name);
    chat.mode = "cron".into();
    chat.subject = subject.clone();
    chat.step = step.clone();
    chat.tools = s.allowed_tools.clone();
    chat.title = format!("cron {} · {} {}", s.id, subject, step).trim().to_string();
    crate::chat::user(&mut chat, &prompt);
    let r = crate::chat::run_unattended(&mut chat, &cfg);
    let _ = crate::chat::save(&mut chat);
    r
}

fn fill(template: &str, ws: &Workspace, item: &Item) -> String {
    template
        .replace("{step}", item.node.as_deref().unwrap_or(""))
        .replace("{subject}", item.payload["subject"].as_str().unwrap_or(""))
        .replace("{title}", &item.title)
        .replace("{root}", &graph::root(ws).display().to_string())
        .replace("{workspace}", &ws.name)
        .replace("{item}", &item.id.to_string())
}

/// Run a command template for one item (cwd = workspace root), logging to `runs/<item>.log`.
/// Returns the log's last lines on success.
pub fn run_template(template: &str, agent: &str, ws: &Workspace, item: &Item) -> Result<String, String> {
    let dir = runs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let log_path = dir.join(format!("{}.log", item.id));
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let cmd = fill(template, ws, item);
    let status = Command::new("sh")
        .args(["-c", &cmd])
        .current_dir(graph::root(ws))
        .env("VENTOSA_WORKSPACE", graph::root(ws))
        .env("VENTOSA_AGENT", agent)
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .status()
        .map_err(|e| e.to_string())?;
    let tail = std::fs::read_to_string(&log_path).unwrap_or_default();
    let tail: String = tail.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    if status.success() {
        Ok(tail)
    } else {
        Err(format!("exit {status}; log {}\n{tail}", log_path.display()))
    }
}

/// The prompt for a provider run: node skill + knowledge + the subject's state.
pub fn build_prompt(ws: &Workspace, item: &Item) -> Result<(String, String), String> {
    let node_id = item.node.clone().unwrap_or_default();
    let node = graph::node(ws, &node_id)?;
    let root = graph::root(ws);
    let mut system = String::new();
    system.push_str("You are one step of a content pipeline described as a skill graph. Do exactly this node's skill.\n");
    system.push_str("You cannot write files: reply with the exact content the node's write command needs (for example the JSON for each option, or the full script file), ready for a human or agent to apply.\n\n");
    system.push_str(&format!("# Node skill: {node_id}\n\n{}\n", node["skill"].as_str().unwrap_or("")));
    if let Some(m) = node["input_meaning"].as_str().filter(|m| !m.is_empty()) {
        system.push_str(&format!("\n# Input\nWhat was handed to this node counts as {m}.\n"));
    }
    system.push_str(&format!("\n# Consistency\n\n{}\n", crate::trusted::prompt_block(ws)));
    let mut budget = MAX_KNOWLEDGE_BYTES;
    for r in node["reads"].as_array().into_iter().flatten() {
        let rel = r["path"].as_str().unwrap_or("");
        let p = root.join(rel);
        if !p.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let take = text.len().min(budget);
        if take == 0 {
            break;
        }
        let end = (0..=take).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        system.push_str(&format!("\n# Knowledge: {rel}\n\n{}\n", &text[..end]));
        budget -= end;
    }
    let subject = item.payload["subject"].as_str().unwrap_or("");
    let labels: Value = crate::load::<Option<Value>>(&root.join("labelling").join("labels.json")).unwrap_or(json!({}));
    let state = &labels["videos"][subject];
    let comment = graph::comment_block(state["notes"].as_str().unwrap_or(""));
    let user = format!(
        "{comment}\nSubject {subject}: {}\nStep: {node_id}\nDetail: {}\nWrite command: {}\n\nCurrent labelling state for this subject:\n{}",
        item.title,
        item.payload["detail"].as_str().unwrap_or(""),
        node["write"].as_str().unwrap_or(""),
        serde_json::to_string_pretty(state).unwrap_or_default()
    );
    Ok((system, user))
}

fn run_provider(p: &Provider, ws: &Workspace, item: &Item) -> Result<(), String> {
    let (system, user) = build_prompt(ws, item)?;
    crate::usage::context("schedule", &ws.name, item.payload["subject"].as_str().unwrap_or(""), item.node.as_deref().unwrap_or(""));
    let text = call_provider(p, &system, &user)?;
    let dir = runs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{}.md", item.id)), text).map_err(|e| e.to_string())
}

/// One non-streaming request. Anthropic: Messages API with the server-side
/// refusal fallback; OpenAI-compatible: chat completions.
pub fn call_provider(p: &Provider, system: &str, user: &str) -> Result<String, String> {
    let cfg = crate::config::load(&crate::config::default_path());
    crate::models::with_failover(&cfg, p, |p, key| call_provider_with(p, key, system, user))
}

fn call_provider_with(p: &Provider, key: &str, system: &str, user: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(600)).build();
    let base = p.base_url.trim_end_matches('/');
    match p.kind.as_str() {
        "anthropic" => {
            let body = json!({
                "model": p.model,
                "max_tokens": 16000,
                "system": system,
                "messages": [{"role": "user", "content": user}],
                "fallbacks": "default",
            });
            let resp: Value = agent
                .post(&format!("{base}/v1/messages"))
                .set("x-api-key", key)
                .set("anthropic-version", ANTHROPIC_VERSION)
                .set("anthropic-beta", "server-side-fallback-2026-07-01")
                .set("content-type", "application/json")
                .send_json(body)
                .map_err(http_err)?
                .into_json()
                .map_err(|e| e.to_string())?;
            if let Some((i, o)) = crate::usage::of_reply(&resp) {
                crate::usage::record(&p.model, i, o);
            }
            if resp["stop_reason"] == "refusal" {
                return Err(format!("refused: {}", resp["stop_details"]));
            }
            let text: String = resp["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if resp["stop_reason"] == "max_tokens" {
                return Ok(format!("{text}\n\n[truncated: max_tokens]"));
            }
            Ok(text)
        }
        "openai-compatible" => {
            let body = json!({
                "model": p.model,
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            });
            let resp: Value = agent
                .post(&format!("{base}/chat/completions"))
                .set("authorization", &format!("Bearer {key}"))
                .set("content-type", "application/json")
                .send_json(body)
                .map_err(http_err)?
                .into_json()
                .map_err(|e| e.to_string())?;
            if let Some((i, o)) = crate::usage::of_reply(&resp) {
                crate::usage::record(&p.model, i, o);
            }
            resp["choices"][0]["message"]["content"].as_str().map(String::from).ok_or_else(|| format!("unexpected reply: {resp}"))
        }
        other => Err(format!("provider kind '{other}' cannot run steps; pick anthropic or openai-compatible in settings")),
    }
}

fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => format!("HTTP {code}: {}", r.into_string().unwrap_or_default()),
        other => other.to_string(),
    }
}
