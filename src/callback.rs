//! Callback: send a subject back to an earlier skill-graph node and re-run what depends on it.
//! See docs/callback-design.md.
//!
//! - dependency edges come from `reads` / `writes` of the steps (file ids `F-*`); no usable edges
//!   falls back to "every later step"
//! - staleness: `pipeline/runs.json` records, per subject and node, the hashes of the node's input
//!   files when it finished; a node is stale when they differ
//! - `callbacks.json` holds the walks. Reopening goes through the engine (`ai_write reopen`), so the
//!   engine's own queue/state rules apply; `advance` moves a walk on whenever the workspace syncs.

use crate::config::{Limits, Workspace};
use crate::graph;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------- model ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Run {
    pub at: u64,
    pub inputs: BTreeMap<String, String>,
    /// Number of `step.reopen` events for this node when it finished (a higher count means it was re-opened since).
    pub reopens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlanStep {
    pub node: String,
    /// `rerun`, `check` (fast: decided when the walk gets there), `skip-valid`, `human-gate`.
    pub action: String,
    pub why: String,
    /// `pending`, `open`, `done`, `skipped`.
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Callback {
    pub id: u64,
    pub subject: String,
    pub start: String,
    /// `full` or `fast`.
    pub mode: String,
    pub reason: String,
    pub plan: Vec<PlanStep>,
    pub depth: usize,
    pub parent: Option<u64>,
    /// `open`, `waiting` (a child callback runs), `done`, `stopped`, `failed`, `refused`.
    pub status: String,
    pub note: String,
    pub created: u64,
    pub updated: u64,
}

impl Callback {
    pub fn live(&self) -> bool {
        matches!(self.status.as_str(), "open" | "waiting")
    }
    pub fn current(&self) -> Option<&PlanStep> {
        self.plan.iter().find(|p| p.status == "open")
    }
    /// `callback #17 fast: subtitles -> publish_meta (1/3)`
    pub fn summary(&self) -> String {
        let done = self.plan.iter().filter(|p| matches!(p.status.as_str(), "done" | "skipped")).count();
        let nodes: Vec<&str> = self.plan.iter().map(|p| p.node.as_str()).collect();
        format!("callback #{} {} {}: {} ({}/{}) {}", self.id, self.mode, self.subject, nodes.join(" -> "), done, self.plan.len(), self.status)
    }
}

fn pipeline(root: &Path) -> PathBuf {
    root.join("pipeline")
}

fn read_json<T: for<'a> Deserialize<'a> + Default>(p: &Path) -> T {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(p: &Path, v: &T) {
    let _ = std::fs::write(p, serde_json::to_string_pretty(v).unwrap_or_default());
}

type Runs = BTreeMap<String, BTreeMap<String, Run>>;

pub fn load_callbacks(root: &Path) -> Vec<Callback> {
    read_json(&pipeline(root).join("callbacks.json"))
}

fn save_callbacks(root: &Path, c: &[Callback]) {
    write_json(&pipeline(root).join("callbacks.json"), &c.to_vec());
}

fn load_runs(root: &Path) -> Runs {
    read_json(&pipeline(root).join("runs.json"))
}

// ---------- graph ----------

fn steps_of(g: &Value) -> Vec<&Value> {
    g["steps"].as_array().into_iter().flatten().filter(|s| s["id"] != "done").collect()
}

pub fn order(g: &Value) -> Vec<String> {
    steps_of(g).iter().filter_map(|s| s["id"].as_str().map(String::from)).collect()
}

fn ids(v: &Value) -> Vec<&str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// Nodes that depend on `node`, in graph order: those that read a file it writes, and so on
/// transitively. Without any such edge every later node counts (conservative).
pub fn downstream(g: &Value, node: &str) -> Vec<String> {
    let ord = order(g);
    let Some(start) = ord.iter().position(|n| n == node) else { return vec![] };
    let steps = steps_of(g);
    let by_id = |id: &str| steps.iter().find(|s| s["id"] == id).copied();
    let mut set = vec![node.to_string()];
    let mut grew = true;
    while grew {
        grew = false;
        for (i, id) in ord.iter().enumerate() {
            if i <= start || set.contains(id) {
                continue;
            }
            let reads = by_id(id).map(|s| ids(&s["reads"])).unwrap_or_default();
            let hit = set.iter().any(|d| by_id(d).is_some_and(|s| ids(&s["writes"]).iter().any(|w| reads.contains(w))));
            if hit {
                set.push(id.clone());
                grew = true;
            }
        }
    }
    let mut out: Vec<String> = ord.iter().skip(start + 1).filter(|n| set.contains(n)).cloned().collect();
    if out.is_empty() {
        out = ord.iter().skip(start + 1).cloned().collect();
    }
    out
}

/// Every node that `node` depends on (the nodes whose downstream contains it).
pub fn upstream(g: &Value, node: &str) -> Vec<String> {
    order(g).into_iter().filter(|n| n != node && downstream(g, n).iter().any(|d| d == node)).collect()
}

fn owner(g: &Value, node: &str) -> String {
    steps_of(g).iter().find(|s| s["id"] == node).and_then(|s| s["owner"].as_str()).unwrap_or("ai").to_string()
}

// ---------- hashes ----------

fn hex(d: impl AsRef<[u8]>) -> String {
    d.as_ref().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn hash_file(p: &Path) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(p) else { return "missing".into() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut h = Sha256::new();
    h.update(len.to_le_bytes());
    let mut buf = vec![0u8; 64 * 1024];
    if len <= 8 * 1024 * 1024 {
        while let Ok(n) = f.read(&mut buf) {
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
    } else {
        // big media: head, tail and size (a re-render changes at least one of them)
        for pos in [0, len.saturating_sub(64 * 1024)] {
            if f.seek(SeekFrom::Start(pos)).is_ok() {
                if let Ok(n) = f.read(&mut buf) {
                    h.update(&buf[..n]);
                }
            }
        }
    }
    hex(h.finalize())
}

fn hash_path(p: &Path, subject: &str, depth: usize) -> String {
    if p.is_file() {
        return hash_file(p);
    }
    let Ok(rd) = std::fs::read_dir(p) else { return "missing".into() };
    let mut names: Vec<(String, PathBuf)> = rd.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).filter(|(n, _)| !n.starts_with('.')).collect();
    // a folder shared by every subject counts only through this subject's entries, when it has any
    if depth == 0 && names.iter().any(|(n, _)| n.starts_with(subject)) {
        names.retain(|(n, _)| n.starts_with(subject));
    }
    names.sort();
    let mut h = Sha256::new();
    for (n, path) in names {
        h.update(n.as_bytes());
        h.update(if path.is_dir() && depth < 3 { hash_path(&path, subject, depth + 1) } else if path.is_file() { hash_file(&path) } else { String::new() });
    }
    hex(h.finalize())
}

/// Hashes of the per-subject files a node reads, by file id.
pub fn input_hashes(root: &Path, g: &Value, node: &str, subject: &str) -> BTreeMap<String, String> {
    let step = steps_of(g).into_iter().find(|s| s["id"] == node);
    let files: Vec<&Value> = g["files"].as_array().into_iter().flatten().collect();
    let mut out = BTreeMap::new();
    for r in step.map(|s| ids(&s["reads"])).unwrap_or_default() {
        if let Some(path) = files.iter().find(|f| f["id"] == r).and_then(|f| f["path"].as_str()) {
            out.insert(r.to_string(), hash_path(&root.join(path), subject, 0));
        }
    }
    out
}

fn reopen_count(root: &Path, subject: &str, node: &str) -> usize {
    std::fs::read_to_string(pipeline(root).join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e["kind"] == "step.reopen" && e["vid"] == subject && e["data"]["step"] == node)
        .count()
}

/// Remember that `node` finished for `subject` with the files it reads as they are now.
pub fn record_run(root: &Path, g: &Value, subject: &str, node: &str, now: u64) {
    let mut runs = load_runs(root);
    runs.entry(subject.into()).or_default().insert(node.into(), Run { at: now, inputs: input_hashes(root, g, node, subject), reopens: reopen_count(root, subject, node) });
    write_json(&pipeline(root).join("runs.json"), &runs);
}

/// Record a run for every node that is behind the subject's current stage and has none yet, or was
/// reopened and finished again. Idempotent.
pub fn record_runs(root: &Path, g: &Value, state: &Value, now: u64) -> usize {
    let ord = order(g);
    let runs = load_runs(root);
    let mut n = 0;
    for (vid, v) in state["videos"].as_object().into_iter().flatten() {
        let stage = v["stage"].as_str().unwrap_or("");
        let upto = if stage == "done" { ord.len() } else { ord.iter().position(|s| s == stage).unwrap_or(0) };
        for node in &ord[..upto.min(ord.len())] {
            let have = runs.get(vid).and_then(|m| m.get(node));
            if have.is_none() || have.is_some_and(|r| reopen_count(root, vid, node) > r.reopens) {
                record_run(root, g, vid, node, now);
                n += 1;
            }
        }
    }
    n
}

// ---------- staleness ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StaleRow {
    pub subject: String,
    pub node: String,
    pub stale: bool,
    /// Always true for a stale node: its own input files changed. (Kept for clients that expect a root-cause flag.)
    pub root: bool,
    pub reasons: Vec<String>,
    pub downstream: Vec<String>,
}

pub fn stale_rows(root: &Path, g: &Value, subjects: &[String]) -> Vec<StaleRow> {
    let runs = load_runs(root);
    let mut out = vec![];
    for subject in subjects {
        let Some(mine) = runs.get(subject) else { continue };
        for node in order(g) {
            let Some(run) = mine.get(&node) else { continue };
            let mut reasons = vec![];
            let now = input_hashes(root, g, &node, subject);
            for (k, v) in &now {
                if run.inputs.get(k) != Some(v) {
                    reasons.push(format!("input {k} changed since {node} ran"));
                }
            }
            let own = !reasons.is_empty();
            out.push(StaleRow { subject: subject.clone(), node: node.clone(), stale: !reasons.is_empty(), root: own, reasons, downstream: downstream(g, &node) });
        }
    }
    out
}

// ---------- planning ----------

fn is_stale_now(root: &Path, g: &Value, subject: &str, node: &str) -> Option<bool> {
    let run = load_runs(root).get(subject)?.get(node)?.clone();
    Some(input_hashes(root, g, node, subject).iter().any(|(k, v)| run.inputs.get(k) != Some(v)))
}

pub fn plan(root: &Path, g: &Value, subject: &str, node: &str, mode: &str) -> Vec<PlanStep> {
    let mut out = vec![PlanStep { node: node.into(), action: "rerun".into(), why: "named as the starting node".into(), status: "pending".into() }];
    for n in downstream(g, node) {
        let human = owner(g, &n) == "human";
        let (action, why) = if mode == "full" {
            (if human { "human-gate" } else { "rerun" }, "full: everything downstream runs again".to_string())
        } else {
            match is_stale_now(root, g, subject, &n) {
                None => ("skip-valid", "never ran for this subject".to_string()),
                Some(true) => (if human { "human-gate" } else { "rerun" }, "its input already changed".to_string()),
                Some(false) => ("check", "decided when the walk gets here: runs only if its inputs changed".to_string()),
            }
        };
        out.push(PlanStep { node: n, action: action.into(), why, status: "pending".into() });
    }
    out
}

// ---------- start ----------

pub struct StartArgs<'a> {
    pub subject: &'a str,
    pub node: &'a str,
    pub mode: &'a str,
    pub reason: &'a str,
    /// The node the caller is working on, when the callback comes from inside a run.
    pub caller: Option<&'a str>,
    pub dry_run: bool,
}

fn refuse(root: &Path, cbs: &mut Vec<Callback>, mut cb: Callback, why: String, now: u64) -> String {
    cb.status = "refused".into();
    cb.note = why.clone();
    cb.updated = now;
    cbs.push(cb);
    save_callbacks(root, cbs);
    format!("{why}. Raise it with decision_ask instead of retrying.")
}

/// Plan and open a callback. `stage` gives the subject's stage after the reopen; `reopen(node, note)` reopens a node.
pub fn start(
    root: &Path,
    g: &Value,
    a: &StartArgs,
    lim: &Limits,
    now: u64,
    stage: &dyn Fn() -> Option<String>,
    reopen: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Callback, String> {
    let ord = order(g);
    if !ord.iter().any(|n| n == a.node) {
        return Err(format!("no step {}", a.node));
    }
    if !matches!(a.mode, "full" | "fast") {
        return Err("mode is full or fast".into());
    }
    if a.reason.trim().is_empty() {
        return Err("reason is required".into());
    }
    let mut cbs = load_callbacks(root);
    let id = cbs.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    let mut cb = Callback {
        id,
        subject: a.subject.into(),
        start: a.node.into(),
        mode: a.mode.into(),
        reason: a.reason.trim().into(),
        plan: plan(root, g, a.subject, a.node, a.mode),
        status: "open".into(),
        created: now,
        updated: now,
        ..Default::default()
    };
    if a.dry_run {
        return Ok(cb);
    }
    // the open callback of this subject: a child when the caller is its current node, else a conflict
    let open = cbs.iter().position(|c| c.subject == a.subject && c.live() && c.status == "open");
    if let Some(p) = open {
        let parent = cbs[p].clone();
        if a.caller.is_some() && parent.current().map(|c| c.node.as_str()) == a.caller {
            cb.parent = Some(parent.id);
            cb.depth = parent.depth + 1;
        } else {
            return Err(format!("{} is still open; wait for it, stop it, or call back from inside its current node", parent.summary()));
        }
    }
    if cb.depth > lim.callback_depth {
        let why = format!("callback depth {} is over the limit {}", cb.depth, lim.callback_depth);
        return Err(refuse(root, &mut cbs, cb, why, now));
    }
    // a cycle: an ancestor still has this node ahead of it
    let mut up = cb.parent;
    while let Some(pid) = up {
        let p = cbs.iter().find(|c| c.id == pid).cloned();
        if let Some(p) = p {
            if p.plan.iter().any(|s| s.node == a.node && matches!(s.status.as_str(), "pending" | "open")) {
                return Err(refuse(root, &mut cbs, cb, format!("cycle: {} is already ahead in callback #{}", a.node, p.id), now));
            }
            up = p.parent;
        } else {
            break;
        }
    }
    let recent = cbs.iter().filter(|c| c.subject == a.subject && c.start == a.node && c.status != "refused" && now.saturating_sub(c.created) < 3600).count();
    if recent >= lim.callback_per_hour {
        let why = cbs.iter().rev().find(|c| c.subject == a.subject && c.start == a.node && c.status != "refused").map(|c| c.reason.clone()).unwrap_or_default();
        return Err(refuse(root, &mut cbs, cb, format!("{recent} callbacks to {} for {} in the last hour (limit {}); the last reason was: {why}", a.node, a.subject, lim.callback_per_hour), now));
    }
    reopen(a.node, &format!("callback #{id} {}: {}", a.mode, cb.reason))?;
    cb.plan[0].status = "open".into();
    if let Some(p) = cb.parent {
        if let Some(pc) = cbs.iter_mut().find(|c| c.id == p) {
            pc.status = "waiting".into();
            pc.updated = now;
        }
    }
    // the engine must have taken the reopen: the subject now stands at (or before) the node
    if let Some(st) = stage() {
        let at = ord.iter().position(|n| *n == st).unwrap_or(ord.len());
        if at > ord.iter().position(|n| n == a.node).unwrap_or(0) {
            cb.status = "failed".into();
            cb.note = format!("the engine did not reopen {}: the subject is at {st}", a.node);
        }
    }
    cbs.push(cb.clone());
    save_callbacks(root, &cbs);
    Ok(cb)
}

pub fn stop(root: &Path, id: u64, now: u64) -> Result<Callback, String> {
    let mut cbs = load_callbacks(root);
    let c = cbs.iter_mut().find(|c| c.id == id).ok_or_else(|| format!("no callback #{id}"))?;
    if !c.live() {
        return Err(format!("callback #{id} is {}", c.status));
    }
    c.status = "stopped".into();
    c.updated = now;
    let out = c.clone();
    // a waiting parent resumes
    if let Some(p) = out.parent {
        if let Some(pc) = cbs.iter_mut().find(|c| c.id == p && c.status == "waiting") {
            pc.status = "open".into();
        }
    }
    save_callbacks(root, &cbs);
    Ok(out)
}

// ---------- walking ----------

/// Move every open callback on. `stage_of(subject)` is the subject's stage now; `failed(subject, node)`
/// tells whether the queue item for that node ended failed. Returns log lines.
pub fn advance(
    root: &Path,
    g: &Value,
    now: u64,
    stage_of: &dyn Fn(&str) -> Option<String>,
    failed: &dyn Fn(&str, &str) -> bool,
    reopen: &mut dyn FnMut(&str, &str, &str) -> Result<(), String>,
) -> Vec<String> {
    let ord = order(g);
    let mut cbs = load_callbacks(root);
    let mut log = vec![];
    let mut changed = false;
    // children first, so a finished child lets its parent move in the same pass
    let mut idx: Vec<usize> = (0..cbs.len()).filter(|&i| cbs[i].live()).collect();
    idx.sort_by_key(|&i| std::cmp::Reverse(cbs[i].depth));
    for i in idx {
        if cbs[i].status == "waiting" {
            if cbs.iter().any(|c| c.parent == Some(cbs[i].id) && c.live()) {
                continue;
            }
            cbs[i].status = "open".into();
            changed = true;
        }
        let subject = cbs[i].subject.clone();
        let Some(cur) = cbs[i].current().map(|p| p.node.clone()) else {
            cbs[i].status = "done".into();
            changed = true;
            continue;
        };
        if failed(&subject, &cur) {
            cbs[i].status = "failed".into();
            cbs[i].note = format!("{cur} failed");
            cbs[i].updated = now;
            log.push(format!("{} failed at {cur}", cbs[i].summary()));
            changed = true;
            continue;
        }
        let Some(stage) = stage_of(&subject) else { continue };
        let past = stage == "done" || ord.iter().position(|n| *n == stage) > ord.iter().position(|n| *n == cur);
        if !past {
            continue;
        }
        // the open node finished
        record_run(root, g, &subject, &cur, now);
        if let Some(p) = cbs[i].plan.iter_mut().find(|p| p.node == cur) {
            p.status = "done".into();
        }
        changed = true;
        let (mode, id) = (cbs[i].mode.clone(), cbs[i].id);
        loop {
            let Some(k) = cbs[i].plan.iter().position(|p| p.status == "pending") else {
                cbs[i].status = "done".into();
                log.push(format!("{} done", cbs[i].summary()));
                break;
            };
            let node = cbs[i].plan[k].node.clone();
            let go = mode == "full" || (cbs[i].plan[k].action != "skip-valid" && is_stale_now(root, g, &subject, &node) == Some(true));
            if !go {
                cbs[i].plan[k].status = "skipped".into();
                cbs[i].plan[k].action = "skip-valid".into();
                cbs[i].plan[k].why = "its inputs did not change".into();
                // checked against the current files: remember them
                record_run(root, g, &subject, &node, now);
                continue;
            }
            cbs[i].plan[k].action = if owner(g, &node) == "human" { "human-gate" } else { "rerun" }.into();
            match reopen(&subject, &node, &format!("callback #{id} {mode}: {}", cbs[i].reason)) {
                Ok(()) => {
                    cbs[i].plan[k].status = "open".into();
                    log.push(format!("callback #{id}: {node} reopened for {subject}"));
                }
                Err(e) => {
                    cbs[i].status = "failed".into();
                    cbs[i].note = format!("reopen {node}: {e}");
                }
            }
            break;
        }
        cbs[i].updated = now;
    }
    if changed {
        save_callbacks(root, &cbs);
    }
    log
}

// ---------- workspace glue ----------

fn engine_reopen(ws: &Workspace) -> impl FnMut(&str, &str, &str) -> Result<(), String> + '_ {
    move |subject, node, note| {
        let out = graph::engine_cmd(ws, "ai_write").args(["reopen", subject, node, note]).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
    }
}

fn state_stage(state: &Value, subject: &str) -> Option<String> {
    state["videos"][subject]["stage"].as_str().map(String::from)
}

pub fn start_ws(ws: &Workspace, a: &StartArgs, lim: &Limits) -> Result<Callback, String> {
    let root = graph::root(ws);
    let g = graph::steps(ws)?;
    let mut reopen = engine_reopen(ws);
    let wsr = ws.clone();
    let subject = a.subject.to_string();
    let stage = move || graph::refresh(&wsr).ok().and_then(|s| state_stage(&s, &subject));
    let cb = start(&root, &g, a, lim, crate::now(), &stage, &mut |n, note| reopen(a.subject, n, note))?;
    if !a.dry_run {
        let _ = graph::sync(ws, &crate::queue_path());
    }
    Ok(cb)
}

/// Called after every state refresh of a workspace: record runs, move callbacks on. Returns true
/// when it reopened something (the caller refreshes the state again).
pub fn on_state(ws: &Workspace, state: &Value, items: &[crate::queue::Item]) -> (bool, Vec<String>) {
    let root = graph::root(ws);
    let Ok(g) = graph::steps(ws) else { return (false, vec![]) };
    let now = crate::now();
    let had_open = load_callbacks(&root).iter().any(|c| c.live());
    // completed nodes are recorded before a walk judges them
    if !had_open {
        record_runs(&root, &g, state, now);
        return (false, vec![]);
    }
    let failed = |subject: &str, node: &str| {
        let key = format!("{}:{subject}", ws.name);
        items.iter().any(|i| i.status == "failed" && i.node.as_deref() == Some(node) && i.payload["key"] == key.as_str())
    };
    let mut reopen = engine_reopen(ws);
    let log = advance(&root, &g, now, &|s| state_stage(state, s), &failed, &mut reopen);
    // runs of nodes that finished outside any callback
    record_runs(&root, &g, state, now);
    (!log.is_empty(), log)
}

pub fn subjects(ws: &Workspace) -> Vec<String> {
    graph::read_state(ws).ok().and_then(|s| s["videos"].as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default()
}

pub fn stale_ws(ws: &Workspace, subject: Option<&str>) -> Result<Vec<StaleRow>, String> {
    let g = graph::steps(ws)?;
    let subs = match subject {
        Some(s) => vec![s.to_string()],
        None => subjects(ws),
    };
    Ok(stale_rows(&graph::root(ws), &g, &subs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    fn graph_json() -> Value {
        json!({
            "files": [{"id":"F-clips","path":"recording/clips.json"},{"id":"F-edits","path":"assets/edits/"},{"id":"F-subs","path":"assets/subtitles/"}],
            "steps": [
                {"id":"record","owner":"human","reads":[],"writes":["F-clips"]},
                {"id":"edit","owner":"ai","reads":["F-clips"],"writes":["F-edits"]},
                {"id":"subtitles","owner":"ai","reads":["F-edits"],"writes":["F-subs"]},
                {"id":"publish_meta","owner":"ai","reads":["F-subs"],"writes":[]},
                {"id":"thumb","owner":"ai","reads":["F-clips"],"writes":[]},
                {"id":"upload","owner":"human","reads":[],"writes":[]},
                {"id":"done","owner":"-"}
            ]})
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ventosa-cb-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for s in ["pipeline", "recording", "assets/edits", "assets/subtitles"] {
            std::fs::create_dir_all(d.join(s)).unwrap();
        }
        std::fs::write(d.join("recording/clips.json"), "{}").unwrap();
        std::fs::write(d.join("assets/edits/V01-final.mp4"), "cut1").unwrap();
        std::fs::write(d.join("assets/subtitles/V01.srt"), "subs1").unwrap();
        d
    }

    fn lim() -> Limits {
        Limits { callback_depth: 2, callback_per_hour: 2, ..Default::default() }
    }

    fn state(stage: &str) -> Value {
        json!({"videos": {"V01": {"stage": stage}}})
    }

    fn args<'a>(node: &'a str, mode: &'a str) -> StartArgs<'a> {
        StartArgs { subject: "V01", node, mode, reason: "edit re-cut", caller: None, dry_run: false }
    }

    #[test]
    fn downstream_follows_files_and_falls_back() {
        let g = graph_json();
        assert_eq!(downstream(&g, "edit"), ["subtitles", "publish_meta"]);
        assert_eq!(downstream(&g, "record"), ["edit", "subtitles", "publish_meta", "thumb"]);
        assert_eq!(upstream(&g, "publish_meta"), ["record", "edit", "subtitles"]);
        // no reads/writes at all: everything later
        let bare = json!({"steps":[{"id":"a"},{"id":"b"},{"id":"c"},{"id":"done"}]});
        assert_eq!(downstream(&bare, "a"), ["b", "c"]);
    }

    #[test]
    fn hashes_make_nodes_stale() {
        let (g, r) = (graph_json(), tmp("stale"));
        record_runs(&r, &g, &state("done"), 100);
        let rows = stale_rows(&r, &g, &["V01".into()]);
        assert!(rows.iter().all(|x| !x.stale), "{rows:?}");
        std::fs::write(r.join("assets/edits/V01-final.mp4"), "cut2").unwrap();
        let rows = stale_rows(&r, &g, &["V01".into()]);
        let st: Vec<&str> = rows.iter().filter(|x| x.stale).map(|x| x.node.as_str()).collect();
        assert_eq!(st, ["subtitles"]);
        assert!(rows.iter().find(|x| x.node == "subtitles").unwrap().root);
        // another subject's file does not stale this one
        std::fs::write(r.join("assets/edits/V02-final.mp4"), "x").unwrap();
        assert_eq!(stale_rows(&r, &g, &["V01".into()]).iter().filter(|x| x.stale).count(), 1);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn dry_run_changes_nothing() {
        let (g, r) = (graph_json(), tmp("dry"));
        record_runs(&r, &g, &state("done"), 100);
        let mut a = args("edit", "fast");
        a.dry_run = true;
        let cb = start(&r, &g, &a, &lim(), 1000, &|| None, &mut |_, _| panic!("no reopen on dry run")).unwrap();
        assert_eq!(cb.plan.len(), 3);
        assert_eq!(cb.plan[1].action, "check");
        assert!(load_callbacks(&r).is_empty());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn full_walk_reopens_each_node_in_turn() {
        let (g, r) = (graph_json(), tmp("full"));
        record_runs(&r, &g, &state("done"), 100);
        let opened = RefCell::new(vec![]);
        let cb = start(&r, &g, &args("edit", "full"), &lim(), 1000, &|| Some("edit".into()), &mut |n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) }).unwrap();
        assert_eq!(cb.status, "open");
        assert_eq!(*opened.borrow(), ["edit"]);
        let step = |stage: &str| advance(&r, &g, 2000, &|_| Some(stage.into()), &|_, _| false, &mut |_, n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) });
        assert!(step("edit").is_empty(), "still working on edit");
        step("subtitles"); // edit finished; engine moved on, we reopen subtitles
        step("publish_meta");
        step("done");
        assert_eq!(*opened.borrow(), ["edit", "subtitles", "publish_meta"]);
        assert_eq!(load_callbacks(&r)[0].status, "done");
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn fast_walk_skips_valid_nodes() {
        let (g, r) = (graph_json(), tmp("fast"));
        record_runs(&r, &g, &state("done"), 100);
        let opened = RefCell::new(vec![]);
        start(&r, &g, &args("edit", "fast"), &lim(), 1000, &|| Some("edit".into()), &mut |n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) }).unwrap();
        // the re-run edit produced a new cut: subtitles' input changed, publish_meta's did not
        std::fs::write(r.join("assets/edits/V01-final.mp4"), "cut2").unwrap();
        let step = |stage: &str| advance(&r, &g, 2000, &|_| Some(stage.into()), &|_, _| false, &mut |_, n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) });
        step("subtitles");
        assert_eq!(*opened.borrow(), ["edit", "subtitles"]);
        // subtitles regenerate the same srt: nothing downstream changes
        step("done");
        assert_eq!(*opened.borrow(), ["edit", "subtitles"]);
        let cb = &load_callbacks(&r)[0];
        assert_eq!(cb.status, "done");
        assert_eq!(cb.plan[2].status, "skipped");
        // the re-run is remembered: nothing is stale now
        assert!(stale_rows(&r, &g, &["V01".into()]).iter().all(|x| !x.stale), "{:?}", stale_rows(&r, &g, &["V01".into()]));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn fast_stops_when_nothing_changed() {
        let (g, r) = (graph_json(), tmp("same"));
        record_runs(&r, &g, &state("done"), 100);
        let opened = RefCell::new(vec![]);
        start(&r, &g, &args("subtitles", "fast"), &lim(), 1000, &|| Some("subtitles".into()), &mut |n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) }).unwrap();
        advance(&r, &g, 2000, &|_| Some("done".into()), &|_, _| false, &mut |_, n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) });
        assert_eq!(*opened.borrow(), ["subtitles"]);
        assert_eq!(load_callbacks(&r)[0].status, "done");
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn full_mode_stops_at_human_nodes_by_reopening_them() {
        let (g, r) = (graph_json(), tmp("human"));
        record_runs(&r, &g, &state("done"), 100);
        let opened = RefCell::new(vec![]);
        start(&r, &g, &args("record", "full"), &lim(), 1000, &|| Some("record".into()), &mut |n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) }).unwrap();
        advance(&r, &g, 2000, &|_| Some("edit".into()), &|_, _| false, &mut |_, n, _| { opened.borrow_mut().push(n.to_string()); Ok(()) });
        assert_eq!(*opened.borrow(), ["record", "edit"]);
        assert_eq!(load_callbacks(&r)[0].plan[0].action, "rerun");
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn guards_depth_cycle_rate_and_conflict() {
        let (g, r) = (graph_json(), tmp("guard"));
        record_runs(&r, &g, &state("done"), 100);
        let ok = |_: &str, _: &str| Ok(());
        let st = || Some("record".to_string());
        // conflict: one open callback per subject
        start(&r, &g, &args("edit", "fast"), &lim(), 1000, &st, &mut |a, b| ok(a, b)).unwrap();
        assert!(start(&r, &g, &args("record", "fast"), &lim(), 1001, &st, &mut |a, b| ok(a, b)).unwrap_err().contains("still open"));
        // a child from inside the current node is allowed and suspends the parent
        let mut c = args("record", "fast");
        c.caller = Some("edit");
        let child = start(&r, &g, &c, &lim(), 1002, &st, &mut |a, b| ok(a, b)).unwrap();
        assert_eq!((child.depth, child.parent), (1, Some(1)));
        assert_eq!(load_callbacks(&r)[0].status, "waiting");
        // a cycle: the parent still has subtitles ahead
        let mut cy = args("subtitles", "fast");
        cy.caller = Some("record");
        assert!(start(&r, &g, &cy, &lim(), 1003, &st, &mut |a, b| ok(a, b)).unwrap_err().contains("cycle"));
        // depth
        let mut d = args("record", "fast");
        d.caller = Some("record");
        let grand = start(&r, &g, &d, &lim(), 1004, &st, &mut |a, b| ok(a, b));
        assert!(grand.is_err(), "record is ahead in the child (cycle) or too deep");
        // stop resumes the parent
        stop(&r, 2, 1005).unwrap();
        assert_eq!(load_callbacks(&r).iter().find(|c| c.id == 1).unwrap().status, "open");
        stop(&r, 1, 1006).unwrap();
        // rate: edit was asked once already; two more inside the hour hit the limit
        start(&r, &g, &args("edit", "fast"), &lim(), 1010, &st, &mut |a, b| ok(a, b)).unwrap();
        stop(&r, load_callbacks(&r).iter().filter(|c| c.live()).map(|c| c.id).max().unwrap(), 1011).unwrap();
        let e = start(&r, &g, &args("edit", "fast"), &lim(), 1020, &st, &mut |a, b| ok(a, b)).unwrap_err();
        assert!(e.contains("in the last hour"), "{e}");
        assert!(e.contains("decision_ask"));
        // an hour later it is allowed again
        start(&r, &g, &args("edit", "fast"), &lim(), 1000 + 3700, &st, &mut |a, b| ok(a, b)).unwrap();
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn engine_that_ignores_reopen_fails_the_callback() {
        let (g, r) = (graph_json(), tmp("ignore"));
        record_runs(&r, &g, &state("done"), 100);
        let cb = start(&r, &g, &args("edit", "fast"), &lim(), 1000, &|| Some("done".into()), &mut |_, _| Ok(())).unwrap();
        assert_eq!(cb.status, "failed");
        assert!(cb.note.contains("did not reopen"));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn failed_queue_item_fails_the_walk() {
        let (g, r) = (graph_json(), tmp("fail"));
        record_runs(&r, &g, &state("done"), 100);
        start(&r, &g, &args("edit", "fast"), &lim(), 1000, &|| Some("edit".into()), &mut |_, _| Ok(())).unwrap();
        advance(&r, &g, 1500, &|_| Some("edit".into()), &|_, n| n == "edit", &mut |_, _, _| Ok(()));
        assert_eq!(load_callbacks(&r)[0].status, "failed");
        let _ = std::fs::remove_dir_all(&r);
    }
}
