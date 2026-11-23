//! Workspace skill graphs: run the engine, read pipeline state, and keep the
//! queue in step with it (the active pipeline).
//!
//! Ventosa never writes a workspace's state files. It runs the workspace's
//! engine commands (`status`, `ai_write new`) and reads `pipeline/pipeline.json`
//! and `pipeline/skill-graph/steps.json`.

use crate::config::Workspace;
use crate::queue::{self, Item};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the launcher installs the engine, apps and templates.
pub fn runtime_dir() -> PathBuf {
    if let Ok(p) = std::env::var("VENTOSA_RUNTIME") {
        return PathBuf::from(p);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("runtime")
}

/// Run logs and provider outputs: `$XDG_DATA_HOME/ventosa/runs/`.
pub fn runs_dir() -> PathBuf {
    crate::default_path().with_file_name("runs")
}

pub fn root(ws: &Workspace) -> PathBuf {
    let r = ws.root.trim();
    if let Some(rest) = r.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    PathBuf::from(r)
}

/// Build the command for one engine script (`status`, `ai_write`, `skill` …).
pub fn engine_cmd(ws: &Workspace, script: &str) -> Command {
    let root = root(ws);
    let mut c = Command::new("python3");
    if ws.engine == "local" {
        c.arg(root.join("pipeline").join(format!("{script}.py")));
    } else {
        c.arg(runtime_dir().join("engine").join(format!("{script}.py")));
        c.env("VENTOSA_WORKSPACE", &root);
    }
    c.current_dir(&root);
    c
}

/// Recompute the workspace's pipeline state (writes its pipeline.json) and read it.
pub fn refresh(ws: &Workspace) -> Result<Value, String> {
    let out = engine_cmd(ws, "status").output().map_err(|e| format!("cannot run engine: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    read_state(ws)
}

/// The last computed state, without running anything.
pub fn read_state(ws: &Workspace) -> Result<Value, String> {
    let f = root(ws).join("pipeline").join("pipeline.json");
    let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))
}

/// `pipeline/skill-graph/steps.json`: steps, knowledge, files, apps.
pub fn steps(ws: &Workspace) -> Result<Value, String> {
    let f = root(ws).join("pipeline").join("skill-graph").join("steps.json");
    let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))
}

/// `workspace.json` (brand, unit, id prefix …); empty object when absent.
pub fn workspace_json(ws: &Workspace) -> Value {
    std::fs::read_to_string(root(ws).join("workspace.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

/// One step node, ready for an agent: its skill text, knowledge paths and write command.
pub fn node(ws: &Workspace, id: &str) -> Result<Value, String> {
    let g = steps(ws)?;
    let step = g["steps"]
        .as_array()
        .and_then(|s| s.iter().find(|x| x["id"] == id))
        .cloned()
        .ok_or_else(|| format!("no step {id}"))?;
    let skill_rel = step["skill"].as_str().unwrap_or("");
    let skill = if skill_rel.is_empty() {
        String::new()
    } else {
        std::fs::read_to_string(root(ws).join("pipeline").join("skill-graph").join(skill_rel)).unwrap_or_default()
    };
    let known: Vec<&Value> = g["knowledge"].as_array().into_iter().chain(g["files"].as_array()).flatten().collect();
    let reads: Vec<Value> = step["reads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| known.iter().find(|k| k["id"] == *r).map(|k| json!({"id": r, "path": k["path"], "what": k["what"]})))
        .collect();
    let write = match step["command"].as_str() {
        Some(c) if !c.is_empty() => format!("ventosa-graph write {c} <ID> …"),
        _ => String::new(),
    };
    Ok(json!({
        "workspace": ws.name, "id": id, "owner": step["owner"], "need": step["need"],
        "done_when": step["done_when"], "skill": skill, "reads": reads, "write": write,
        "elevate": elevate_text(ws, id),
        "model": step["model"], "input": step["input"], "kind": step["kind"], "per": step["per"], "app": step["app"], "path": step["path"],
        "input_meaning": INPUT_KINDS.iter().find(|(k, _)| step["input"].as_str() == Some(*k)).map(|(_, m)| *m).unwrap_or(""),
        "search": if step["kind"] == "research" { json!({"provider": step["search_provider"], "key_env": step["search_key_env"], "query": step["search_query"], "max": step["search_max"]}) } else { Value::Null },
        "attach": if step["owner"] == "ai" { format!("ventosa-graph -w {} attach {id} <ID>", root(ws).display()) } else { String::new() },
    }))
}

/// The workspace folder a node works in (relative): the first folder among its writes, then its reads.
pub fn node_folder(ws: &Workspace, node: &str) -> Option<String> {
    let g = steps(ws).ok()?;
    let step = g["steps"].as_array()?.iter().find(|s| s["id"] == node)?;
    let files = g["files"].as_array()?;
    let root = root(ws);
    ["writes", "reads"].iter().flat_map(|k| step[*k].as_array().into_iter().flatten()).filter_map(Value::as_str).find_map(|id| {
        let p = files.iter().find(|f| f["id"] == id)?["path"].as_str()?;
        (p.ends_with('/') && root.join(p).is_dir()).then(|| p.trim_end_matches('/').to_string())
    })
}

/// Default rules for what the AI must raise to the human (graph `elevate`).
pub const DEFAULT_ELEVATE: &str = "Decisions to elevate (decision_ask with options, never guess): \
the subject's angle or thesis when the labels leave it open; anything said about a real person, brand or sponsor; \
claims without a source in the knowledge; spending money or calling a paid API beyond the batch limits; \
publishing, uploading or scheduling anything public; deleting or overwriting the human's takes, edits or labels. \
Edge cases: missing or contradictory knowledge files; a take or edit that is too short, silent or corrupt; \
two subjects with the same title; a step whose input is older than its output. \
Dangerous scenarios (stop and elevate): legal, medical or financial advice; copyrighted music or footage; \
personal data of anyone but the host; credentials or keys appearing in any file.";

/// The elevation rules for a step: the graph's `elevate` (or the default), plus the step's own.
pub fn elevate_text(ws: &Workspace, step: &str) -> String {
    let g = steps(ws).unwrap_or(Value::Null);
    let base = g["elevate"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or(DEFAULT_ELEVATE).to_string();
    let own = g["steps"].as_array().and_then(|a| a.iter().find(|s| s["id"] == step)).and_then(|s| s["elevate"].as_str()).filter(|s| !s.trim().is_empty());
    match own {
        Some(o) => format!("{base}\nThis step: {o}"),
        None => base,
    }
}

/// Set the graph-wide elevation rules.
pub fn set_elevate(ws: &Workspace, text: &str) -> Result<(), String> {
    let mut g = steps(ws)?;
    g["elevate"] = json!(text.trim());
    write_steps(ws, &g)
}

/// The id the next subject gets: prefix + (max number + 1), zero-padded to 2.
pub fn next_id(ws: &Workspace, state: &Value) -> String {
    let wj = workspace_json(ws);
    let prefix = wj["id_prefix"].as_str().map(String::from).unwrap_or_else(|| {
        state["videos"]
            .as_object()
            .and_then(|v| v.keys().next())
            .map(|k| k.trim_end_matches(|c: char| c.is_ascii_digit()).to_string())
            .unwrap_or_else(|| "V".into())
    });
    let max = state["videos"]
        .as_object()
        .map(|v| v.keys().filter_map(|k| k.strip_prefix(&prefix)?.parse::<u32>().ok()).max().unwrap_or(0))
        .unwrap_or(0);
    format!("{prefix}{:02}", max + 1)
}

/// The web apps (ids of `pages::WEB_APPS`) a workspace's graph really uses: those named in
/// `apps` (by id, port key or launcher) or by a step's `app`/`apps`. Nothing else is offered.
pub fn app_ids(ws: &Workspace) -> Vec<String> {
    let Ok(g) = steps(ws) else { return vec![] };
    let norm = |a: &Value| -> Option<String> {
        let mut keys = vec![];
        for k in ["id", "port_key", "launcher", "page"] {
            if let Some(v) = a[k].as_str() {
                keys.push(v.to_lowercase());
            }
        }
        let has = |w: &str| keys.iter().any(|k| k.contains(w));
        Some(
            if has("media-label") || has("media_label") { "media-labelling" }
            else if has("teleprompter") || has("record") { "teleprompter" }
            else if has("studio") { "studio" }
            else if has("dashboard") { "dashboard" }
            else if has("upload") { "uploads" }
            else if has("label") { "labelling" }
            else { return None },
        )
        .map(String::from)
    };
    let mut out: Vec<String> = vec![];
    for a in g["apps"].as_array().into_iter().flatten() {
        let used = g["steps"].as_array().into_iter().flatten().any(|s| s["app"] == a["id"]);
        // an app declared in the graph counts when a step points at it (or no step names apps at all)
        let any_named = g["steps"].as_array().into_iter().flatten().any(|s| s["app"].is_string());
        if used || !any_named {
            if let Some(id) = norm(a) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    for st in g["steps"].as_array().into_iter().flatten() {
        if let Some(a) = st["app"].as_str() {
            if let Some(id) = norm(&json!({"id": a})) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// Fields a gallery node or plugin may set on a step when it is added.
pub const EXTRA_FIELDS: [&str; 4] = ["kind", "app", "per", "path"];

/// Write the extra step fields (`kind`, `app`, `per`, `path`) of an added node.
pub fn set_extras(ws: &Workspace, id: &str, extras: &Value) -> Result<(), String> {
    let Some(o) = extras.as_object() else { return Ok(()) };
    let mut g = steps(ws)?;
    let s = find_step(&mut g, id)?;
    for (k, v) in o {
        if !EXTRA_FIELDS.contains(&k.as_str()) {
            return Err(format!("field {k} is not allowed on an added node"));
        }
        let t = v.as_str().ok_or_else(|| format!("{k} must be a string"))?;
        if t.len() > 300 || t.contains("..") || t.starts_with('/') {
            return Err(format!("bad value for {k}"));
        }
        s[k.as_str()] = json!(t);
    }
    write_steps(ws, &g)
}

/// The `<<name>>` placeholders of a start comment, in order, each once.
pub fn comment_placeholders(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut rest = text;
    while let Some(a) = rest.find("<<") {
        let Some(b) = rest[a + 2..].find(">>") else { break };
        let name = rest[a + 2..a + 2 + b].trim();
        if !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &rest[a + 2 + b + 2..];
    }
    out
}

/// Replace `<<name>>` by the value given at the start; empty values stay a placeholder for the AI to resolve.
pub fn fill_comment(text: &str, values: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (k, v) in values.iter().filter(|(_, v)| !v.trim().is_empty()) {
        out = out.replace(&format!("<<{k}>>"), v.trim()).replace(&format!("<< {k} >>"), v.trim());
    }
    out
}

/// What every phase's prompt says about the comment given at the start of the subject.
pub fn comment_block(notes: &str) -> String {
    let notes = notes.trim();
    if notes.is_empty() {
        return String::new();
    }
    let open = comment_placeholders(notes);
    let mut s = format!("Comment from the start (it applies to every phase; follow it in this step):\n{notes}\n");
    if !open.is_empty() {
        s.push_str(&format!("Open variables: {}. A <<name>> is a variable nobody filled in: resolve it from the subject's title, notes and earlier outputs; when it cannot be known, ask with decision_ask instead of guessing.\n", open.iter().map(|n| format!("<<{n}>>")).collect::<Vec<_>>().join(", ")));
    }
    s
}

/// Phase 1: register a subject from a title. The next sync queues it on `options`.
pub fn intake(ws: &Workspace, title: &str, notes: &str) -> Result<String, String> {
    let state = refresh(ws).unwrap_or_else(|_| json!({"videos": {}}));
    let id = next_id(ws, &state);
    let out = engine_cmd(ws, "ai_write")
        .args(["new", &id, title, notes])
        .output()
        .map_err(|e| format!("cannot run engine: {e}"))?;
    if !out.status.success() {
        return Err(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string());
    }
    Ok(id)
}

/// `pipeline/skill-graph/`: `steps.json` and the node skill files.
fn skill_graph(ws: &Workspace) -> PathBuf {
    root(ws).join("pipeline").join("skill-graph")
}

fn write_steps(ws: &Workspace, g: &Value) -> Result<(), String> {
    crate::save(&skill_graph(ws).join("steps.json"), g).map_err(|e| format!("save failed: {e}"))
}

fn step_ids(g: &Value) -> Vec<String> {
    g["steps"].as_array().into_iter().flatten().filter_map(|s| s["id"].as_str().map(String::from)).collect()
}

fn find_step<'a>(g: &'a mut Value, id: &str) -> Result<&'a mut Value, String> {
    g["steps"].as_array_mut().and_then(|a| a.iter_mut().find(|s| s["id"] == id)).ok_or_else(|| format!("no step {id}"))
}

/// Ids and names: letters, digits, `-` and `_`.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn text_arg(v: &Value) -> Result<String, String> {
    v.as_str().map(|s| s.trim().to_string()).ok_or_else(|| "value must be a string".into())
}

/// A list of ids: a JSON array, or a comma-separated string.
pub fn list_arg(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::Array(a) => Ok(a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()),
        Value::String(s) => Ok(s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()),
        Value::Null => Ok(vec![]),
        _ => Err("value must be a list or a comma-separated string".into()),
    }
}

/// What the value given to a node before it counts as: (id, meaning for the AI).
pub const INPUT_KINDS: &[(&str, &str)] = &[
    ("", "judged from the subject's state: a topic when nothing exists yet, a draft when there is already work"),
    ("accepted", "an accepted script: final text, use it as written, do not rewrite its argument"),
    ("topic", "a topic or theme: an idea to develop, not text to keep"),
    ("draft", "a draft: improve it, keep what works"),
    ("reference", "reference material: read it, quote only what it supports"),
];

/// Set one field of a step in `steps.json`. `skill` writes the node's skill file instead.
/// `after` lists the step ids this step depends on; `reads`/`writes` are lists.
pub fn set_step_field(ws: &Workspace, step: &str, field: &str, value: &Value) -> Result<(), String> {
    if field == "skill" {
        let text = value.as_str().ok_or("value must be a string")?;
        return save_skill(ws, step, text);
    }
    let mut g = steps(ws)?;
    let ids = step_ids(&g);
    let s = find_step(&mut g, step)?;
    match field {
        "owner" => {
            let v = text_arg(value)?;
            if v != "ai" && v != "human" {
                return Err("owner must be ai or human".into());
            }
            s["owner"] = json!(v);
        }
        "need" | "done_when" | "command" | "attach" | "write" | "elevate" => s[field] = json!(text_arg(value)?),
        // the model this node runs on: `provider:model`, empty = the app's provider
        "model" => {
            let v = text_arg(value)?;
            if !v.is_empty() && !v.contains(':') {
                return Err("model is provider:model, for example anthropic:claude-opus-5-5".into());
            }
            s["model"] = json!(v);
        }
        // what the value handed to this node counts as
        "input" => {
            let v = text_arg(value)?;
            if !INPUT_KINDS.iter().any(|(k, _)| *k == v) {
                return Err(format!("input must be one of {}", INPUT_KINDS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")));
            }
            s["input"] = json!(v);
        }
        // research nodes: which search provider, its key variable, the query template, how many sources
        "search_provider" | "search_key_env" | "search_query" => s[field] = json!(text_arg(value)?),
        "search_max" => s[field] = json!(text_arg(value)?.parse::<u64>().map_err(|_| "search_max is a number")?.clamp(1, 20)),
        "reads" | "writes" => s[field] = json!(list_arg(value)?),
        "after" => {
            let deps = list_arg(value)?;
            if deps.iter().any(|d| d == step) {
                return Err("a step cannot depend on itself".into());
            }
            if let Some(missing) = deps.iter().find(|d| !ids.contains(d)) {
                return Err(format!("no step {missing}"));
            }
            s["after"] = json!(deps);
        }
        other => return Err(format!("unknown field {other}")),
    }
    write_steps(ws, &g)
}

/// Write a step's node skill. The file is `nodes/<id>.md` unless the step names another.
pub fn save_skill(ws: &Workspace, step: &str, text: &str) -> Result<(), String> {
    let mut g = steps(ws)?;
    let existing = find_step(&mut g, step)?["skill"].as_str().filter(|r| !r.is_empty()).map(String::from);
    let rel = match existing {
        Some(r) => r,
        None => {
            let r = format!("nodes/{step}.md");
            find_step(&mut g, step)?["skill"] = json!(r);
            write_steps(ws, &g)?;
            r
        }
    };
    if Path::new(&rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("skill path {rel} must stay inside skill-graph/"));
    }
    let path = skill_graph(ws).join(&rel);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Add an AI research step: it searches the internet with the chosen provider.
pub fn add_research_step(ws: &Workspace, id: &str, provider: &str, before: Option<&str>) -> Result<(), String> {
    if !crate::research::PROVIDERS.iter().any(|p| p.0 == provider) {
        return Err(format!("unknown search provider {provider}"));
    }
    add_step(ws, id, "ai", &format!("research the subject on the internet ({provider}) and write sourced notes"), None, before)?;
    for (f, v) in [("search_provider", provider.to_string()), ("search_query", "{title}".into()), ("search_max", "8".into())] {
        set_step_field(ws, id, f, &json!(v))?;
    }
    let mut g = steps(ws)?;
    find_step(&mut g, id)?["kind"] = json!("research");
    write_steps(ws, &g)?;
    save_skill(ws, id, RESEARCH_SKILL)
}

/// Mark a step as a research node.
pub fn mark_research(ws: &Workspace, id: &str) -> Result<(), String> {
    let mut g = steps(ws)?;
    find_step(&mut g, id)?["kind"] = json!("research");
    write_steps(ws, &g)
}

pub const RESEARCH_SKILL: &str = "# Node skill: research (internet) [ai]\nSearch the internet for the subject with the provider set on this node, then write notes the next steps can rely on.\n\n1. Call `web_research` with this workspace and step; it runs the node's search (provider, query template, number of sources) and returns numbered sources.\n2. Keep only what the sources support. Every claim in the notes carries its [n] source and url.\n3. Prefer primary sources and papers; mark anything you could not verify as unverified.\n4. Note what was handed to this node and how it counts (see its input kind) before you use it.\n5. Anything about a real person, brand or sponsor is elevated with decision_ask.\n";

/// One finished phase with the files it made, from the workspace's event log.
#[derive(Debug, Clone)]
pub struct Completed {
    pub at: String,
    pub vid: String,
    pub step: String,
    pub who: String,
    /// What started the run: `cron`, `entry`, `manual`, `human` or `agent`.
    pub origin: String,
    pub note: String,
    pub user: String,
    /// (path relative to the root, size when the engine recorded it)
    pub files: Vec<(String, u64)>,
}

/// The newest completed phases (ai.*.done and step.done) that name files, newest first.
/// Every file is tied to the subject, the phase and who ran it, by the engine's artifacts policy.
pub fn completed(ws: &Workspace, limit: usize) -> Vec<Completed> {
    let Ok(text) = fs::read_to_string(root(ws).join("pipeline").join("events.jsonl")) else { return vec![] };
    let mut out: Vec<Completed> = text
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| {
            let k = e["kind"].as_str().unwrap_or("");
            (k.starts_with("ai.") && k.ends_with(".done")) || k == "step.done"
        })
        .filter_map(|e| {
            let d = &e["data"];
            let mut files: Vec<(String, u64)> = d["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str().map(|p| (p.to_string(), f["size"].as_u64().unwrap_or(0)))).collect();
            for k in ["file", "srt"] {
                if let Some(p) = d[k].as_str().filter(|p| !p.is_empty()) {
                    files.push((p.to_string(), 0));
                }
            }
            if let Some(p) = d["edit"].as_str().filter(|p| !p.is_empty()) {
                files.push((p.to_string(), 0));
            }
            let kind = e["kind"].as_str().unwrap_or("");
            if files.is_empty() && kind != "step.done" {
                return None;
            }
            let step = d["step"].as_str().map(String::from).or_else(|| event_step(kind, d)).unwrap_or_else(|| kind.trim_start_matches("ai.").trim_end_matches(".done").to_string());
            let agent = d["agent"].as_str().unwrap_or("");
            let origin = if e["actor"] == "human" { "human" } else if agent.starts_with("cron-") { "cron" } else if agent.starts_with("entry-") { "entry" } else if agent.starts_with("manual-") { "manual" } else { "agent" };
            Some(Completed { origin: origin.into(), note: d["note"].as_str().unwrap_or("").chars().take(120).collect(), user: e["user"].as_str().unwrap_or("").into(), at: e["at"].as_str().unwrap_or("").into(), vid: e["vid"].as_str().unwrap_or("").into(), step, who: e["actor"].as_str().unwrap_or("").into(), files })
        })
        .take(limit)
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

/// Insert a new step before `before` (default: before `done`, or at the end when there is none).
pub fn add_step(ws: &Workspace, id: &str, owner: &str, need: &str, skill: Option<&str>, before: Option<&str>) -> Result<(), String> {
    if !valid_name(id) || id == "done" {
        return Err(format!("bad step id {id}"));
    }
    if owner != "ai" && owner != "human" {
        return Err("owner must be ai or human".into());
    }
    let mut g = steps(ws)?;
    let ids = step_ids(&g);
    if ids.iter().any(|x| x == id) {
        return Err(format!("step {id} exists"));
    }
    let pos = match before {
        Some(b) => ids.iter().position(|x| x == b).ok_or_else(|| format!("no step {b} to insert before"))?,
        None => ids.iter().position(|x| x == "done").unwrap_or(ids.len()),
    };
    let skill_rel = skill.map_or_else(|| format!("nodes/{id}.md"), String::from);
    let step = json!({"id": id, "owner": owner, "need": need, "skill": skill_rel,
        "command": "", "reads": [], "writes": [], "done_when": "", "after": []});
    g["steps"].as_array_mut().ok_or("steps.json has no steps array")?.insert(pos, step);
    write_steps(ws, &g)
}

/// The steps file as text, for the undo stack.
pub fn steps_text(ws: &Workspace) -> Option<String> {
    fs::read_to_string(root(ws).join("pipeline").join("skill-graph").join("steps.json")).ok()
}

/// Put a saved steps file back (undo).
pub fn restore_steps(ws: &Workspace, text: &str) -> Result<(), String> {
    let g: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    write_steps(ws, &g)
}

fn unique_id(ids: &[String], base: &str) -> String {
    let base: String = base.trim_end_matches(|c: char| c == '_' || c.is_ascii_digit()).to_string();
    let base = if base.is_empty() { "step".to_string() } else { base };
    (2..).map(|n| format!("{base}_{n}")).find(|c| !ids.contains(c)).unwrap_or(base)
}

/// A step and its skill text, to copy.
pub fn copy_step(ws: &Workspace, id: &str) -> Result<Value, String> {
    let mut g = steps(ws)?;
    let step = find_step(&mut g, id)?.clone();
    let skill = step["skill"].as_str().filter(|r| !r.is_empty()).and_then(|r| fs::read_to_string(skill_graph(ws).join(r)).ok()).unwrap_or_default();
    Ok(json!({"step": step, "skill": skill}))
}

/// Paste a copied step under a fresh id, `after` another step (default: before done). Returns the id.
pub fn paste_step(ws: &Workspace, clip: &Value, after: Option<&str>) -> Result<String, String> {
    let mut g = steps(ws)?;
    let ids = step_ids(&g);
    let mut step = clip["step"].clone();
    let id = if ids.iter().any(|x| Some(x.as_str()) == step["id"].as_str()) || step["id"].is_null() { unique_id(&ids, step["id"].as_str().unwrap_or("step")) } else { step["id"].as_str().unwrap_or("step").to_string() };
    step["id"] = json!(id);
    step["skill"] = json!(format!("nodes/{id}.md"));
    step["after"] = json!([]);
    let pos = after.and_then(|a| ids.iter().position(|x| x == a)).map(|p| p + 1).or_else(|| ids.iter().position(|x| x == "done")).unwrap_or(ids.len());
    g["steps"].as_array_mut().ok_or("steps.json has no steps array")?.insert(pos, step);
    write_steps(ws, &g)?;
    let text = clip["skill"].as_str().unwrap_or("");
    if !text.is_empty() {
        save_skill(ws, &id, text)?;
    }
    Ok(id)
}

/// Add a gallery node (owner, need, skill text) before `before`, under a free id.
pub fn add_def(ws: &Workspace, id: &str, owner: &str, need: &str, skill: &str, before: Option<&str>) -> Result<String, String> {
    let g = steps(ws)?;
    let ids = step_ids(&g);
    let id = if ids.iter().any(|x| x == id) { unique_id(&ids, id) } else { id.to_string() };
    add_step(ws, &id, owner, need, None, before)?;
    if !skill.is_empty() {
        save_skill(ws, &id, skill)?;
    }
    Ok(id)
}

/// Remove a step (never `done`) and drop it from every `after` list.
pub fn remove_step(ws: &Workspace, step: &str) -> Result<(), String> {
    if step == "done" {
        return Err("the done step cannot be removed".into());
    }
    let mut g = steps(ws)?;
    let list = g["steps"].as_array_mut().ok_or("steps.json has no steps array")?;
    let before = list.len();
    list.retain(|s| s["id"] != step);
    if list.len() == before {
        return Err(format!("no step {step}"));
    }
    for s in list.iter_mut() {
        if let Some(deps) = s["after"].as_array_mut() {
            deps.retain(|d| d.as_str() != Some(step));
        }
    }
    write_steps(ws, &g)
}

/// Save a skill file under `skills/<name>.md`, list it in `files`, and wire it to a step's `reads`.
pub fn install_skill(ws: &Workspace, step: &str, name: &str, text: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!("bad skill name {name}"));
    }
    let mut g = steps(ws)?;
    if !step_ids(&g).iter().any(|x| x == step) {
        return Err(format!("no step {step}"));
    }
    let rel = format!("pipeline/skill-graph/skills/{name}.md");
    let path = root(ws).join(&rel);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    if !g["files"].is_array() {
        g["files"] = json!([]);
    }
    if let Some(files) = g["files"].as_array_mut() {
        if !files.iter().any(|f| f["id"] == name) {
            files.push(json!({"id": name, "path": rel, "what": "skill"}));
        }
    }
    let s = find_step(&mut g, step)?;
    if let Some(reads) = s["reads"].as_array_mut() {
        if !reads.iter().any(|r| r.as_str() == Some(name)) {
            reads.push(json!(name));
        }
    } else {
        s["reads"] = json!([name]);
    }
    write_steps(ws, &g)
}

fn live(i: &Item) -> bool {
    matches!(i.status.as_str(), "queued" | "running" | "review")
}

/// Upsert one live queue item per subject from the workspace state.
/// AI steps → kind `ai`; human steps → kind `todo`. When a subject moves on,
/// its old item becomes `done`. Returns how many items changed.
pub fn sync_state(ws: &Workspace, state: &Value, items: &mut Vec<Item>) -> usize {
    let mut changed = 0;
    let Some(videos) = state["videos"].as_object() else { return 0 };
    for (vid, v) in videos {
        let key = format!("{}:{vid}", ws.name);
        let stage = v["stage"].as_str().unwrap_or("");
        let owner = v["owner"].as_str().unwrap_or("human");
        let title = v["title"].as_str().unwrap_or(vid);
        let detail = v["detail"].as_str().filter(|d| !d.is_empty()).or(v["need"].as_str()).unwrap_or("");
        let pos = items.iter().position(|i| live(i) && i.payload["key"] == key.as_str());
        if let Some(p) = pos {
            if items[p].node.as_deref() == Some(stage) {
                if items[p].payload["detail"] != detail {
                    items[p].payload["detail"] = json!(detail);
                    changed += 1;
                }
                continue;
            }
            let id = items[p].id;
            queue::set_status(items, id, "done");
            changed += 1;
        }
        if stage == "done" || stage.is_empty() {
            continue;
        }
        let kind = if owner == "ai" { "ai" } else { "todo" };
        let payload = json!({"key": key, "workspace": ws.name, "subject": vid, "detail": detail});
        queue::enqueue(items, &format!("{vid} · {title}"), kind, &format!("graph:{}", ws.name), Some(stage.into()), payload);
        changed += 1;
    }
    changed
}

/// Refresh + sync one workspace into the queue file.
pub fn sync(ws: &Workspace, queue_path: &PathBuf) -> Result<usize, String> {
    let mut state = refresh(ws)?;
    let mut items: Vec<Item> = crate::load(queue_path);
    // record finished nodes (hashes) and move callbacks on; a reopen changes the state
    if crate::callback::on_state(ws, &state, &items).0 {
        state = refresh(ws)?;
    }
    let n = sync_state(ws, &state, &mut items);
    if n > 0 {
        crate::save(queue_path, &items).map_err(|e| e.to_string())?;
    }
    Ok(n)
}

/// Which step an engine event belongs to (for the logs). Every way an entry is made
/// lands in `events.jsonl`: the engine's own kinds map by prefix; generic ones
/// (`step.done`, `step.reopen`, `app.done`, `ai.attach`, `ai.detach`, custom app
/// events) carry the step in `data.step` or `data.node`.
pub fn event_step(kind: &str, data: &Value) -> Option<String> {
    let k = kind;
    let fixed = match () {
        _ if k.starts_with("ai.options") || k == "ai.video.new" || k == "video.new" || k == "label.more_options" => "options",
        _ if k.starts_with("ai.draft") => "drafts",
        _ if k.starts_with("label.") || k.starts_with("script.") => "label",
        _ if k.starts_with("ai.script") => "script",
        _ if k.starts_with("record.") => "record",
        _ if k.starts_with("ai.edit") => "edit",
        _ if k.starts_with("ai.subtitles") => "subtitles",
        _ if k.starts_with("ai.publish_meta") => "publish_meta",
        _ if k == "publish.approved" || k == "publish.unapproved" => "publish",
        _ if k.starts_with("publish.") => "upload",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.into());
    }
    ["step", "node"].iter().find_map(|f| data[*f].as_str().filter(|s| !s.is_empty())).map(String::from)
}

/// Run logs written by the scheduler for items at a node of a workspace.
pub fn run_logs(ws: &str, node: &str, items: &[Item]) -> Vec<(u64, String, PathBuf)> {
    let dir = runs_dir();
    let mut out: Vec<(u64, String, PathBuf)> = items
        .iter()
        .filter(|i| i.payload["workspace"] == ws && i.node.as_deref() == Some(node))
        .flat_map(|i| {
            ["log", "md"].into_iter().filter_map(|ext| {
                let p = dir.join(format!("{}.{ext}", i.id));
                p.exists().then(|| (i.updated, format!("{} · {}", i.title, i.status), p))
            })
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        Workspace { name: "w".into(), root: "/nonexistent".into(), engine: "ventosa".into(), enabled: true }
    }

    fn state(stage: &str, owner: &str) -> Value {
        json!({"videos": {"V01": {"title": "T", "stage": stage, "owner": owner, "detail": "", "need": "n"}}})
    }

    #[test]
    fn sync_creates_one_live_item_per_subject() {
        let mut items = vec![];
        assert_eq!(sync_state(&ws(), &state("options", "ai"), &mut items), 1);
        assert_eq!(sync_state(&ws(), &state("options", "ai"), &mut items), 0, "idempotent");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "ai");
        assert_eq!(items[0].node.as_deref(), Some("options"));
    }

    #[test]
    fn moving_on_closes_the_old_item_and_opens_a_todo() {
        let mut items = vec![];
        sync_state(&ws(), &state("drafts", "ai"), &mut items);
        sync_state(&ws(), &state("label", "human"), &mut items);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].status, "done");
        assert_eq!(items[1].kind, "todo");
        assert_eq!(items[1].status, "queued");
        sync_state(&ws(), &state("done", "-"), &mut items);
        assert!(items.iter().all(|i| i.status == "done"));
    }

    #[test]
    fn next_id_counts_up_from_the_highest() {
        let st = json!({"videos": {"E01": {}, "E07": {}}});
        assert_eq!(next_id(&ws(), &st), "E08");
    }

    #[test]
    fn events_map_to_steps() {
        let none = json!({});
        assert_eq!(event_step("ai.options.done", &none).as_deref(), Some("options"));
        assert_eq!(event_step("label.approved", &none).as_deref(), Some("label"));
        assert_eq!(event_step("publish.uploaded", &none).as_deref(), Some("upload"));
        assert_eq!(event_step("ai.subtitles.done", &none).as_deref(), Some("subtitles"));
        assert_eq!(event_step("ai.attach", &none), None);
        assert_eq!(event_step("step.done", &json!({"step": "voiceover"})).as_deref(), Some("voiceover"));
        assert_eq!(event_step("app.done", &json!({"node": "pick"})).as_deref(), Some("pick"));
    }
}

#[cfg(test)]
mod edit_tests {
    #[test]
    fn comment_placeholders_fill_and_reach_the_prompt() {
        let c = "theme: \"<<global_theme>>\" and this <<specific_topic>> in pixel art, <<global_theme>> again";
        assert_eq!(comment_placeholders(c), ["global_theme", "specific_topic"]);
        let f = fill_comment(c, &[("global_theme".into(), "ocean".into()), ("specific_topic".into(), "".into())]);
        assert!(f.contains("\"ocean\"") && f.contains("<<specific_topic>>") && !f.contains("<<global_theme>>"));
        let b = comment_block(&f);
        assert!(b.contains("applies to every phase") && b.contains("<<specific_topic>>") && b.contains("decision_ask"));
        assert!(comment_block("  ").is_empty());
    }

    use super::*;

    /// A workspace in a fresh temp dir with the given `steps.json`.
    fn temp_ws(name: &str, steps: &Value) -> (Workspace, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ventosa-graph-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sg = dir.join("pipeline").join("skill-graph");
        fs::create_dir_all(&sg).unwrap();
        fs::write(sg.join("steps.json"), steps.to_string()).unwrap();
        let ws = Workspace { name: "t".into(), root: dir.display().to_string(), engine: "ventosa".into(), enabled: false };
        (ws, dir)
    }

    fn sample() -> Value {
        json!({"rule": "r", "steps": [
            {"id": "a", "owner": "ai", "need": "x", "skill": "", "reads": [], "after": []},
            {"id": "b", "owner": "human", "need": "y", "skill": "", "reads": [], "after": []},
            {"id": "done", "owner": "-", "need": "", "skill": ""}
        ]})
    }

    fn ids(ws: &Workspace) -> Vec<String> {
        step_ids(&steps(ws).unwrap())
    }

    #[test]
    fn set_fields_and_after_roundtrip() {
        let (ws, dir) = temp_ws("set", &sample());
        set_step_field(&ws, "b", "owner", &json!("ai")).unwrap();
        set_step_field(&ws, "b", "after", &json!(["a"])).unwrap();
        let g = steps(&ws).unwrap();
        assert_eq!(g["steps"][1]["owner"], "ai");
        assert_eq!(g["steps"][1]["after"], json!(["a"]));
        assert_eq!(g["rule"], "r", "other keys survive");
        assert!(set_step_field(&ws, "b", "owner", &json!("bot")).is_err());
        assert!(set_step_field(&ws, "b", "after", &json!(["b"])).is_err());
        assert!(set_step_field(&ws, "b", "bogus", &json!("v")).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn skill_field_writes_node_file() {
        let (ws, dir) = temp_ws("skill", &sample());
        set_step_field(&ws, "a", "skill", &json!("# A\nDo it.")).unwrap();
        let text = fs::read_to_string(dir.join("pipeline/skill-graph/nodes/a.md")).unwrap();
        assert_eq!(text, "# A\nDo it.");
        assert_eq!(steps(&ws).unwrap()["steps"][0]["skill"], "nodes/a.md");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_before_done_then_remove_cleans_after() {
        let (ws, dir) = temp_ws("addrm", &sample());
        add_step(&ws, "c", "human", "z", None, None).unwrap();
        assert_eq!(ids(&ws), ["a", "b", "c", "done"]);
        set_step_field(&ws, "c", "after", &json!(["b"])).unwrap();
        add_step(&ws, "d", "ai", "w", None, Some("b")).unwrap();
        assert_eq!(ids(&ws), ["a", "d", "b", "c", "done"]);
        assert!(add_step(&ws, "d", "ai", "w", None, None).is_err(), "duplicate");
        assert!(add_step(&ws, "e", "ai", "w", None, Some("nope")).is_err());
        remove_step(&ws, "b").unwrap();
        assert_eq!(ids(&ws), ["a", "d", "c", "done"]);
        assert_eq!(steps(&ws).unwrap()["steps"][2]["after"], json!([]));
        assert!(remove_step(&ws, "done").is_err());
        assert!(remove_step(&ws, "b").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn install_skill_is_listed_and_wired() {
        let (ws, dir) = temp_ws("skillinst", &sample());
        install_skill(&ws, "a", "hook", "Teach the hook.").unwrap();
        let g = steps(&ws).unwrap();
        assert_eq!(g["steps"][0]["reads"], json!(["hook"]));
        assert_eq!(g["files"][0]["path"], "pipeline/skill-graph/skills/hook.md");
        assert!(dir.join("pipeline/skill-graph/skills/hook.md").is_file());
        assert!(install_skill(&ws, "a", "../x", "t").is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
