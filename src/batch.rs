//! Executions: batches that drive a graph without a human pressing each step.
//!
//! - `start`: the graph begins with an automated (AI) step. A batch creates
//!   `count` subjects through the workspace's intake (title = brief + number) and
//!   then runs every AI step of those subjects until each reaches `done`. Human
//!   steps pause that subject (it shows in the to-do); the batch picks it up
//!   again when the human finishes.
//! - `drain`: run the AI items already waiting at one step (`node`), e.g. the
//!   human labelled a proposal and the script should now be written.
//!
//! Every tick (`tick`) the executor syncs the workspaces, requeues stuck runs,
//! and starts runs up to `limits.concurrency`, at most `limits.requests_per_minute`.
//! A failed run is retried after `backoff_secs · 2^attempt` up to `limits.retries`
//! times; then the item is marked failed and the batch carries on with the rest.
//!
//! A run is either the built-in agent (the chat engine on the configured
//! provider, with graph/install/apps/run denied and write allowed, so it can
//! attach, write through the engine's gate and raise decisions) or a shell
//! `command` (e.g. `claude -p …`), with the schedule placeholders.

use crate::config::{Config, Workspace};
use crate::graph;
use crate::queue::Item;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Batch {
    pub id: u64,
    pub workspace: String,
    /// `start` or `drain`.
    pub kind: String,
    /// Drain: the step whose waiting items run.
    pub node: String,
    /// Start: subjects to create.
    pub count: usize,
    /// Start: what the subjects are about; the titles are `<brief> <n>`.
    pub brief: String,
    /// `builtin` or `command`.
    pub runner: String,
    /// Command template for `command` runners ({step} {subject} {title} {root} {workspace} {item}).
    pub command: String,
    /// `running`, `waiting` (only human steps left), `done`, `stopped`.
    pub status: String,
    pub subjects: Vec<String>,
    /// Item ids this batch has run (drain batches stop when none are left).
    pub items: Vec<u64>,
    pub runs: u64,
    pub failures: u64,
    pub log: Vec<String>,
    pub created: u64,
    pub updated: u64,
}

pub fn path() -> PathBuf {
    crate::default_path().with_file_name("batches.json")
}

pub fn list() -> Vec<Batch> {
    crate::load(&path())
}

fn save(b: &[Batch]) {
    let _ = crate::save(&path(), &b.to_vec());
}

/// One lock for every read-modify-write of batches.json and queue.json inside this process.
static LOCK: Mutex<()> = Mutex::new(());
/// Start times of recent runs (rate limit window).
static STARTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());
/// Runs in flight in this process.
static RUNNING: Mutex<usize> = Mutex::new(0);

fn log(b: &mut Batch, line: impl Into<String>) {
    b.log.push(format!("{} {}", crate::now(), line.into()));
    if b.log.len() > 200 {
        let cut = b.log.len() - 200;
        b.log.drain(..cut);
    }
    b.updated = crate::now();
}

/// The first step of a workspace's graph is an AI step: the graph can start itself.
pub fn self_starting(ws: &Workspace) -> bool {
    graph::steps(ws).ok().and_then(|g| g["steps"].as_array().and_then(|s| s.first().cloned())).is_some_and(|s| s["owner"] == "ai")
}

/// AI steps with items waiting right now: (step, count).
pub fn waiting(ws: &Workspace, items: &[Item]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = vec![];
    for i in items.iter().filter(|i| i.kind == "ai" && i.status == "queued" && i.payload["workspace"] == ws.name.as_str()) {
        let node = i.node.clone().unwrap_or_default();
        match out.iter_mut().find(|(n, _)| *n == node) {
            Some(e) => e.1 += 1,
            None => out.push((node, 1)),
        }
    }
    out
}

/// Create and register a batch. Start batches create their subjects now.
pub fn create(cfg: &Config, mut b: Batch) -> Result<Batch, String> {
    let ws = cfg.workspace(&b.workspace).cloned().ok_or_else(|| format!("no workspace {}", b.workspace))?;
    if b.runner.is_empty() {
        b.runner = if cfg.provider.kind == "none" { "command".into() } else { "builtin".into() };
    }
    if b.runner == "command" && b.command.trim().is_empty() {
        b.command = cfg.schedules.iter().find(|s| s.workspace == ws.name && s.runner == "command").map(|s| s.command.clone()).unwrap_or_else(|| crate::config::Schedule::default().command);
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = list();
    b.id = all.iter().map(|x| x.id).max().unwrap_or(0) + 1;
    b.status = "running".into();
    b.created = crate::now();
    match b.kind.as_str() {
        "start" => {
            let n = b.count.clamp(1, cfg.limits.max_batch.max(1));
            let brief = if b.brief.trim().is_empty() { format!("batch {}", b.id) } else { b.brief.trim().to_string() };
            for k in 1..=n {
                let title = if n == 1 { brief.clone() } else { format!("{brief} {k}") };
                match graph::intake(&ws, &title, &format!("batch {} · {brief}", b.id)) {
                    Ok(id) => b.subjects.push(id),
                    Err(e) => log(&mut b, format!("intake {k}: {e}")),
                }
            }
            if b.subjects.is_empty() {
                return Err(b.log.last().cloned().unwrap_or_else(|| "intake made no subjects".into()));
            }
            let line = format!("created {} subject(s): {}", b.subjects.len(), b.subjects.join(" "));
            log(&mut b, line);
        }
        "drain" => {
            if b.node.is_empty() {
                return Err("drain needs a step".into());
            }
            let line = format!("draining {}", b.node);
            log(&mut b, line);
        }
        k => return Err(format!("unknown batch kind {k}")),
    }
    let _ = graph::sync(&ws, &crate::queue_path());
    all.push(b.clone());
    save(&all);
    Ok(b)
}

pub fn stop(id: u64) -> Result<(), String> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = list();
    let b = all.iter_mut().find(|b| b.id == id).ok_or_else(|| format!("no batch {id}"))?;
    b.status = "stopped".into();
    log(b, "stopped by the user");
    save(&all);
    Ok(())
}

fn owns(b: &Batch, i: &Item) -> bool {
    if i.payload["workspace"] != b.workspace.as_str() {
        return false;
    }
    match b.kind.as_str() {
        "start" => i.payload["subject"].as_str().is_some_and(|s| b.subjects.iter().any(|x| x == s)),
        _ => i.node.as_deref() == Some(b.node.as_str()) || b.items.contains(&i.id),
    }
}

fn rate_ok(cfg: &Config) -> bool {
    let rpm = cfg.limits.requests_per_minute;
    let now = crate::now();
    let mut s = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    s.retain(|t| now.saturating_sub(*t) < 60);
    rpm == 0 || (s.len() as u32) < rpm
}

/// One executor step. Returns lines for the status bar.
pub fn tick(cfg: &Config) -> Vec<String> {
    let mut out = vec![];
    let active: Vec<Batch> = list().into_iter().filter(|b| matches!(b.status.as_str(), "running" | "waiting")).collect();
    if active.is_empty() {
        return out;
    }
    let qpath = crate::queue_path();
    let timeout = cfg.limits.timeout_minutes.max(1) * 60;
    // 1. sync each workspace with an active batch; requeue runs that outlived the timeout
    for ws_name in active.iter().map(|b| b.workspace.clone()).collect::<std::collections::BTreeSet<_>>() {
        if let Some(ws) = cfg.workspace(&ws_name) {
            let _ = graph::sync(ws, &qpath);
        }
    }
    {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<Item> = crate::load(&qpath);
        let mut changed = false;
        for i in items.iter_mut().filter(|i| i.status == "running" && i.payload["agent"].as_str().is_some_and(|a| a.starts_with("batch:"))) {
            if crate::now().saturating_sub(i.updated) > timeout + 60 {
                i.status = "queued".into();
                i.payload["retry_at"] = json!(0);
                i.payload["error"] = json!("run timed out or Ventosa stopped; requeued");
                changed = true;
            }
        }
        if changed {
            let _ = crate::save(&qpath, &items);
        }
    }
    // 2. start runs
    let items: Vec<Item> = crate::load(&qpath);
    for b in &active {
        let Some(ws) = cfg.workspace(&b.workspace).cloned() else { continue };
        let now = crate::now();
        let ready: Vec<Item> = items
            .iter()
            .filter(|i| i.kind == "ai" && i.status == "queued" && owns(b, i) && i.payload["retry_at"].as_u64().unwrap_or(0) <= now)
            .cloned()
            .collect();
        for item in ready {
            {
                let running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
                if *running >= cfg.limits.concurrency.max(1) || !rate_ok(cfg) {
                    break;
                }
            }
            if let Some(line) = launch(cfg, b, &ws, &item) {
                out.push(line);
            }
        }
    }
    // 3. batch status: done when every subject reached `done` (start) or nothing is left (drain)
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let items: Vec<Item> = crate::load(&qpath);
    let mut all = list();
    for b in all.iter_mut().filter(|b| matches!(b.status.as_str(), "running" | "waiting")) {
        let mine: Vec<&Item> = items.iter().filter(|i| owns(b, i) && matches!(i.status.as_str(), "queued" | "running")).collect();
        let ai_left = mine.iter().any(|i| i.kind == "ai");
        let human_left = mine.iter().any(|i| i.kind == "todo");
        let next = if ai_left {
            "running"
        } else if b.kind == "drain" {
            "done"
        } else {
            let state = cfg.workspace(&b.workspace).and_then(|w| graph::read_state(w).ok()).unwrap_or(Value::Null);
            let all_done = b.subjects.iter().all(|s| state["videos"][s]["stage"] == "done");
            if all_done {
                "done"
            } else if human_left {
                "waiting"
            } else {
                "running"
            }
        };
        if b.status != next {
            b.status = next.into();
            log(b, format!("status {next}"));
            out.push(format!("batch {} {next}", b.id));
        }
    }
    save(&all);
    out
}

/// Mark the item running and run it on a thread. Returns a status line.
fn launch(cfg: &Config, b: &Batch, ws: &Workspace, item: &Item) -> Option<String> {
    let qpath = crate::queue_path();
    {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<Item> = crate::load(&qpath);
        let it = items.iter_mut().find(|i| i.id == item.id && i.status == "queued")?;
        it.status = "running".into();
        it.updated = crate::now();
        it.payload["agent"] = json!(format!("batch:{}", b.id));
        let _ = crate::save(&qpath, &items);
        let mut all = list();
        if let Some(x) = all.iter_mut().find(|x| x.id == b.id) {
            x.runs += 1;
            if !x.items.contains(&item.id) {
                x.items.push(item.id);
            }
            log(x, format!("run item {} ({} at {})", item.id, item.payload["subject"].as_str().unwrap_or(""), item.node.as_deref().unwrap_or("")));
        }
        save(&all);
    }
    STARTS.lock().unwrap_or_else(|e| e.into_inner()).push(crate::now());
    *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    let line = format!("batch {} · item {}", b.id, item.id);
    let (cfg, b, ws, item) = (cfg.clone(), b.clone(), ws.clone(), item.clone());
    std::thread::spawn(move || {
        let res = match b.runner.as_str() {
            "command" => crate::runner::run_template(&b.command, &format!("batch-{}", b.id), &ws, &item),
            _ => run_builtin(&cfg, &b, &ws, &item),
        };
        finish(&cfg, b.id, &ws, &item, res);
        *RUNNING.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
    });
    Some(line)
}

/// Record a run's result: success → `review` and an immediate sync (the subject
/// moves on); failure → retry later or, after `limits.retries`, `failed`.
fn finish(cfg: &Config, batch: u64, ws: &Workspace, item: &Item, res: Result<String, String>) {
    let qpath = crate::queue_path();
    {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut items: Vec<Item> = crate::load(&qpath);
        let mut all = list();
        let b = all.iter_mut().find(|x| x.id == batch);
        if let Some(i) = items.iter_mut().find(|i| i.id == item.id) {
            match &res {
                Ok(note) => {
                    i.status = "review".into();
                    i.payload["result"] = json!(note.chars().take(2000).collect::<String>());
                    if let Some(b) = b {
                        log(b, format!("item {} done", item.id));
                    }
                }
                Err(e) => {
                    let attempt = i.payload["attempts"].as_u64().unwrap_or(0) + 1;
                    i.payload["attempts"] = json!(attempt);
                    i.payload["error"] = json!(e);
                    if attempt > cfg.limits.retries as u64 {
                        i.status = "failed".into();
                        if let Some(b) = b {
                            b.failures += 1;
                            log(b, format!("item {} failed after {attempt} attempt(s): {e}", item.id));
                        }
                    } else {
                        let wait = cfg.limits.backoff_secs.max(1) << (attempt - 1).min(10);
                        i.status = "queued".into();
                        i.payload["retry_at"] = json!(crate::now() + wait);
                        if let Some(b) = b {
                            log(b, format!("item {} attempt {attempt} failed, retry in {wait}s: {e}", item.id));
                        }
                    }
                }
            }
            i.updated = crate::now();
        }
        let _ = crate::save(&qpath, &items);
        save(&all);
    }
    let _ = graph::sync(ws, &qpath);
}

/// Run one item with the built-in agent. Graph edits, installs, apps and shell
/// runs are denied; reading, writing through the engine and raising decisions are allowed.
fn run_builtin(cfg: &Config, b: &Batch, ws: &Workspace, item: &Item) -> Result<String, String> {
    let mut cfg = cfg.clone();
    let p = &mut cfg.permissions;
    p.read = "allow".into();
    p.write = "allow".into();
    p.graph = "deny".into();
    p.install = "deny".into();
    p.apps = "deny".into();
    p.run = "deny".into();
    let step = item.node.clone().unwrap_or_default();
    let subject = item.payload["subject"].as_str().unwrap_or("").to_string();
    let agent = format!("batch-{}", b.id);
    let prompt = format!(
        "Run one step of the pipeline without a human. Workspace {ws}, step {step}, subject {subject} ({title}).\n\
1. pipeline_attach (workspace {ws}, step {step}, subject {subject}, agent {agent}).\n\
2. graph_node for {step}: follow its skill exactly; read the knowledge it lists.\n\
3. Finish with pipeline_write using the node's write command (args as a list, without `ventosa-graph write`).\n\
If the skill's elevation rules say a decision belongs to the human, call decision_ask instead of guessing and stop.\n\
Detail from the queue: {detail}",
        ws = ws.name,
        title = item.title,
        detail = item.payload["detail"].as_str().unwrap_or(""),
    );
    let mut chat = crate::chat::new_chat(&ws.name);
    chat.mode = "batch".into();
    chat.subject = subject.to_string();
    chat.step = step.to_string();
    chat.title = format!("batch {} · {subject} {step}", b.id);
    crate::chat::user(&mut chat, &prompt);
    let r = crate::chat::run_unattended(&mut chat, &cfg);
    let _ = crate::chat::save(&mut chat);
    r
}

/// Add every batch fact to an item list summary (for tools).
pub fn summary(b: &Batch) -> Value {
    json!({
        "id": b.id, "workspace": b.workspace, "kind": b.kind, "node": b.node, "count": b.count, "brief": b.brief,
        "runner": b.runner, "status": b.status, "subjects": b.subjects, "runs": b.runs, "failures": b.failures,
        "log": b.log.iter().rev().take(10).collect::<Vec<_>>(),
    })
}

/// Drop finished batches older than a week.
pub fn prune() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = list();
    let before = all.len();
    all.retain(|b| matches!(b.status.as_str(), "running" | "waiting") || crate::now().saturating_sub(b.updated) < 7 * 86400);
    if all.len() != before {
        save(&all);
    }
}
