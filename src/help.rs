//! Ventosa's own help: an internal prompt and a meta graph that explain the
//! pipeline (how a video goes from an idea to published, who does what, where),
//! and `explain` that says what to do at a given step of a workspace.

use crate::config::{Config, Workspace};
use crate::graph;
use serde_json::{json, Value};

/// The internal prompt every Ventosa AI instance (chat, speech, batches) starts with.
pub const INTERNAL_PROMPT: &str = "Ventosa is a harness for hybrid workflows: human and AI steps in one graph, \
and the meta graph (the help tool) is the zero ring every AI run starts from. From it you either execute one node's \
skill for one subject, or edit the graph itself (steps, owners, skills) when your job says so; never both in one run. \
Everything that enters the workflow is an entry a person can also make by hand: a manual start, a to-do, a decision, \
a queue item, a cron job; automations (cron jobs, batches) only add the same entries and show in history, so the human \
can read, edit, run or remove each one. A cron job is an isolated, unattended run: attach to its one phase, follow the \
job prompt, and end with the job's output policy (a short HANDOFF note that makes the human's next action obvious). \
Ventosa is an AI harness for media production. Work runs on skill graphs: \
each step is a node with an owner (ai or human), a need, a skill and a done_when. The AI attaches to one node \
for one subject, follows that node's skill and finishes through the engine's write gate (pipeline_write). \
Human steps become the human's to-do; what the AI produced waits in the AI queue. Executions run graphs in \
batches (start: create N subjects from a brief; drain: run the items waiting at a step) within the limits in \
settings. Decisions that belong to the human are raised with decision_ask, never guessed: follow the graph's \
`elevate` rules (decisions, edge cases, dangerous scenarios). Apps are small pages wired to a node; they can be \
published to a server for the public. When the user asks for help, use the help tool and answer with the next \
concrete action and where to do it (which page, which button, which app).

Multi-scene consistency (characters, assets, places, styles, voices): every workspace keeps an array of \
trusted sources (trusted_sources_list). Before you write, generate or render any scene, load it and build over \
it: reuse the approved refs, recipe (seeds, prompt fragments, model names) and locked traits verbatim, in every \
scene and every video. Never invent or drift the look of a recurring element. When a scene needs an element \
with no approved source, propose one (trusted_source_add, with refs and locked traits) and raise decision_ask \
for the human to approve it; do not render that element until it is approved. After a scene is done, record the \
sources it used (trusted_source_used, scene as SUBJECT#scene). A new source must not contradict an approved one; \
if the brief asks for a change to an approved source, raise a decision instead of changing it.

Making many videos from one request (\"make 20 videos about X\"): read the graph (graph_steps), get the titles approved, \
register one subject per title with pipeline_intake (theme-level work on one theme subject), fit the graph with \
graph_step_add/set/remove (per-video nodes for every subject, shared knowledge and tool installs per workspace), \
elevate paid generation, installs and publishing with decision_ask, then run the batch. The auto_review node checks \
each cut before the human (ventosa-graph review <ID>, at most 3 automatic rounds per cut). Playbook: help topic recipe_bulk. \
Subtitles are a separate step after the edit is signed (edit → subtitles → publish_meta): run \
`ventosa-graph subtitles <ID>`, review report.json, then `ventosa-graph write subtitles-done <ID>`. Follow the \
subtitles policy (subtitles_policy in the meta graph); never invent words that were not said, never retime by hand.";

/// The consistency policy as rules (the meta graph's `policies`).
pub fn consistency_policy() -> Value {
    json!({
        "id": "multi-scene-consistency",
        "store": "knowledge/trusted-sources.json (array; one entry per character, asset, location, style, voice or brand)",
        "rules": [
            "load the trusted sources before any scene work (trusted_sources_list)",
            "build over approved sources only: same refs, same recipe, same locked traits, in every scene and every video",
            "no approved source for a recurring element → propose it (trusted_source_add) + decision_ask; wait for approval",
            "never edit an approved source; a requested change is a decision for the human",
            "after a scene: trusted_source_used with the ids and SUBJECT#scene",
            "review steps check each scene against the locked traits and reject drift"
        ],
        "tools": ["trusted_sources_list", "trusted_source_add", "trusted_source_used", "trusted_source_approve"]
    })
}

/// The subtitles policy (the meta graph's `policies`): a separate pass after the edit.
pub fn subtitles_policy() -> Value {
    json!({
        "id": "subtitles",
        "when": "after the edit is signed off, before publish_meta; a new edit reopens it",
        "store": "assets/subtitles/<ID>/ (<ID>.srt|.vtt|.ass, reels/, words.json, report.json); never assets/edits/",
        "pipeline": [
            "16 kHz mono audio from the signed edit",
            "word timestamps: WhisperX (forced alignment) > stable-ts (align to the script) > faster-whisper > whisper.cpp",
            "script spelling where it matches 1:1; the audio keeps the timing",
            "cues by policy, in frames at the edit's fps, snapped to the EDL's shot changes",
            "SRT + VTT + ASS, re-timed 9:16 reel tracks, QC report; --burn for open captions"
        ],
        "rules": [
            "≤42 characters per line (reels 26), ≤2 balanced lines, no weak word at a line end",
            "≤17 characters per second; 5/6 s to 7 s per cue",
            "≥2 frames between cues; gaps under 12 frames close to 2",
            "a cue does not span a shot change it can end on; edges within 7 frames snap onto the cut",
            "timestamps floored to the unit so a cue shows on its first frame",
            "what was said, in the script's spelling; never invent words; unclear words → decision_ask",
            "translations derive from the master track with the same timings",
            "burn in only for platforms without sidecar captions"
        ],
        "commands": ["ventosa-graph subtitles <ID> [--burn] [--lang L]", "ventosa-graph write subtitles-done <ID>"],
        "overrides": "workspace.json → subtitles {lang, max_chars, max_lines, max_cps, min_dur, max_dur, gap_frames, snap_frames, font, backend, model}"
    })
}

/// The cron policy (the meta graph's `policies`): how an unattended run behaves.
pub fn cron_policy() -> Value {
    json!({
        "id": "cron-run",
        "rules": [
            "start from the meta graph (help), then do exactly one phase: one node's skill for one subject, or an edit of the graph",
            "isolated session: no human is present; calls that need approval are refused, so raise decision_ask and stop",
            "use only the job's allowed tools; graph edits only when the job's task is `graph`, one small change at a time",
            "never publish, upload, delete or spend beyond the limits; leave work in the workspace",
            "finish with the output policy: a HANDOFF note (done · files · what to check · next action and where)",
            "failures retry with exponential backoff and jitter up to limits.retries, then stay failed in history; a policy refusal never retries",
            "a schedule never runs twice at once; agent jobs run at most every 5 minutes"
        ]
    })
}

/// The meta graph as data: a graph of nodes about Ventosa itself (soul, principles, node types, views,
/// tools, recipes, patterns, testing, failures, policies). `informs` are the edges.
const META_JSON: &str = include_str!("meta_graph.json");

pub fn meta_nodes() -> Vec<Value> {
    serde_json::from_str::<Value>(META_JSON).ok().and_then(|v| v["nodes"].as_array().cloned()).unwrap_or_default()
}

pub const META_SOUL: &str = "soul";

fn meta_find(id: &str) -> Option<Value> {
    meta_nodes().into_iter().find(|n| n["id"] == id)
}

/// One meta node with its edges: what it informs (outward) and what informs it (inward).
pub fn meta_topic(id: &str) -> Result<String, String> {
    let all = meta_nodes();
    let n = all.iter().find(|n| n["id"] == id).ok_or_else(|| format!("no meta node {id}; call help with no topic for the index"))?;
    let title = |x: &str| all.iter().find(|n| n["id"] == x).and_then(|n| n["title"].as_str()).unwrap_or("").to_string();
    let out: Vec<String> = n["informs"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(|x| format!("{x} ({})", title(x))).collect();
    let inn: Vec<String> = all.iter().filter(|m| m["informs"].as_array().is_some_and(|a| a.iter().any(|v| v == id))).filter_map(|m| m["id"].as_str().map(|x| format!("{x} ({})", title(x)))).collect();
    Ok(format!(
        "[{}] {} ({})\n\n{}\n\ninforms: {}\ninformed by: {}\n",
        n["group"].as_str().unwrap_or(""), n["title"].as_str().unwrap_or(""), id, n["text"].as_str().unwrap_or(""),
        if out.is_empty() { "-".into() } else { out.join(", ") },
        if inn.is_empty() { "-".into() } else { inn.join(", ") }
    ))
}

/// The entry point: the soul first, then the index of the ring by group.
pub fn meta_overview() -> String {
    let all = meta_nodes();
    let mut out = String::new();
    if let Ok(soul) = meta_topic(META_SOUL) {
        out.push_str(&soul);
    }
    out.push_str("\nMeta graph index (call help with topic=<id> to read a node and follow its edges):\n");
    let mut groups: Vec<String> = vec![];
    for n in &all {
        let g = n["group"].as_str().unwrap_or("").to_string();
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    for g in groups.iter().filter(|g| *g != "soul") {
        let ids: Vec<String> = all.iter().filter(|n| n["group"] == g.as_str()).filter(|n| !n["id"].as_str().unwrap_or("").starts_with("hub_")).filter_map(|n| n["id"].as_str().map(String::from)).collect();
        out.push_str(&format!("- {g}: {}\n", ids.join(", ")));
    }
    out
}

/// The meta graph in the shape of a workspace graph (graph_nodes / graph_node with workspace `meta`).
pub fn meta_graph_nodes() -> Value {
    let rows: Vec<Value> = meta_nodes().iter().map(|n| json!({"id": n["id"], "owner": "ai", "group": n["group"], "need": n["title"], "informs": n["informs"]})).collect();
    json!({"rule": "The meta graph: how Ventosa works and how to decide. Start at `soul`, follow `informs`.", "steps": rows})
}

/// The meta graph: Ventosa's own pipeline, from setup to publishing (the human-facing stages), plus the node graph.
pub fn meta_graph() -> Value {
    json!({
        "name": "ventosa-meta-graph",
        "soul": META_SOUL,
        "nodes": meta_nodes().iter().map(|n| json!({"id": n["id"], "group": n["group"], "title": n["title"], "informs": n["informs"]})).collect::<Vec<_>>(),
        "stages": [
            {"id": "setup", "who": "human", "where": "onboarding · settings", "what": "brand, AI provider (required), voice, recommended tools, what we make (bio)", "next": "graph"},
            {"id": "graph", "who": "ai + human", "where": "overview ▸ node canvas · AI", "what": "the skill graph: steps, owners, skills, dependencies (after), elevate rules. The AI drafts it from the bio; the human edits any field (Ask AI / Edit with AI)", "next": "intake"},
            {"id": "consistency", "who": "ai proposes · human approves", "where": "access ▸ trusted sources · to-do ▸ decisions", "what": "the trusted sources every scene builds over: characters, assets, places, styles, voices (refs, recipe, locked traits)", "next": "intake"},
            {"id": "intake", "who": "human or batch", "where": "overview ▸ manual start (hover the node) · executions ▸ start", "what": "a subject enters: a title by hand, or N subjects from a brief in a batch", "next": "ai-steps"},
            {"id": "cron", "who": "ai (autonomous) · human configures", "where": "overview ▸ cron job node", "what": "a schedule runs an isolated agent loop on one phase (a node's skill for the oldest queued subject, or an edit of the graph itself), with its own prompt, an allowed-tools list and a final output policy (HANDOFF note); retries with backoff, never two runs at once, runs recorded in history", "next": "ai-steps"},
            {"id": "ai-steps", "who": "ai", "where": "history · executions · cron job", "what": "each AI step attaches, follows the node skill, writes through the gate; batches run all of them within the API limits, with retries", "next": "human-steps"},
            {"id": "human-steps", "who": "human", "where": "to-do · header apps (label, rec, up, custom apps)", "what": "label, record, review: the to-do says what and where; finishing moves the subject on", "next": "decisions"},
            {"id": "decisions", "who": "human", "where": "to-do ▸ decision cards", "what": "the AI elevates choices it must not take alone; pick an option or write an answer", "next": "ai-steps"},
            {"id": "subtitles", "who": "ai", "where": "graph ▸ subtitles step · docs ▸ subtitles", "what": "after the edit, a separate pass: speech to text, script-corrected, frame-exact SRT/VTT/ASS and reel tracks; signed with subtitles-done", "next": "publish"},
            {"id": "apps", "who": "ai + human", "where": "apps · header buttons · servers", "what": "AI-made pages wired to a node; published over SSH for forms and callbacks", "next": "publish"},
            {"id": "publish", "who": "human", "where": "to-do ▸ publish / upload", "what": "title, description, chapters, thumbnail; the human uploads and schedules", "next": "done"}
        ],
        "policies": [consistency_policy(), subtitles_policy(), cron_policy()],
        "views": {
            "graph": "overview (canvas, manual start, cron job, executions), to-do, history, apps",
            "ai": "full-screen chat with tools; @ references steps, fields, skills, items, tasks, files, help",
            "speech": "the same AI, by voice; chats are saved like text chats",
            "settings": "provider, voice, permissions, limits, servers, recommended tools, MCP clients",
            "files": "the workspace's files; open, preview, reference with @file"
        }
    })
}

/// What to do now: for one step (or the whole workspace when `step` is empty).
pub fn explain(cfg: &Config, ws: &Workspace, step: &str) -> Result<String, String> {
    let g = graph::steps(ws)?;
    let steps: Vec<Value> = g["steps"].as_array().cloned().unwrap_or_default();
    let state = graph::read_state(ws).unwrap_or(Value::Null);
    let mut out = String::new();
    if !cfg.bio.trim().is_empty() {
        out.push_str(&format!("We make: {}\n\n", cfg.bio.trim()));
    }
    let line = |s: &Value| {
        let id = s["id"].as_str().unwrap_or("");
        let n = state["queues"][id].as_array().map_or(0, |a| a.len());
        let owner = s["owner"].as_str().unwrap_or("human");
        let place = if owner == "ai" {
            "AI: cron job / executions run it, history lists it"
        } else if id.contains("record") {
            "you, in the teleprompter (header ▸ rec ↗)"
        } else if id.contains("label") || id.contains("review") {
            "you, in labelling (header ▸ label ↗) then to-do ▸ done"
        } else if id.contains("upload") || id.contains("publish") {
            "you, in uploads (header ▸ up ↗)"
        } else {
            "you, in to-do"
        };
        format!("- {id} [{owner}] {n} waiting · {} · where: {place}", s["need"].as_str().unwrap_or(""))
    };
    if step.is_empty() {
        out.push_str(&format!("Workspace {} · graph {}:\n", ws.name, g["name"].as_str().unwrap_or("")));
        for s in steps.iter().filter(|s| s["id"] != "done") {
            out.push_str(&line(s));
            out.push('\n');
        }
        let human: Vec<String> = steps.iter().filter(|s| s["owner"] != "ai").filter(|s| state["queues"][s["id"].as_str().unwrap_or("")].as_array().is_some_and(|a| !a.is_empty())).filter_map(|s| s["id"].as_str().map(String::from)).collect();
        out.push_str(&if human.is_empty() { "\nNothing waits on you: start or drain a batch in overview ▸ executions.\n".to_string() } else { format!("\nYour next step: {} (see to-do).\n", human.join(", ")) });
    } else {
        let s = steps.iter().find(|s| s["id"] == step).ok_or_else(|| format!("no step {step}"))?;
        out.push_str(&line(s));
        out.push_str(&format!("\ndone when: {}\n", s["done_when"].as_str().unwrap_or("")));
        let idx = steps.iter().position(|x| x["id"] == step).unwrap_or(0);
        if let Some(n) = steps.get(idx + 1) {
            out.push_str(&format!("then: {} [{}]\n", n["id"].as_str().unwrap_or(""), n["owner"].as_str().unwrap_or("")));
        }
        let node = graph::node(ws, step)?;
        if let Some(sk) = node["skill"].as_str().filter(|s| !s.is_empty()) {
            out.push_str(&format!("\nskill:\n{}\n", sk.chars().take(1500).collect::<String>()));
        }
    }
    out.push_str(&format!("\nElevate to the human:\n{}\n", graph::elevate_text(ws, step)));
    Ok(out)
}

#[cfg(test)]
mod meta_tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    #[test]
    fn meta_graph_is_connected_from_the_soul() {
        let nodes = meta_nodes();
        let ids: BTreeSet<String> = nodes.iter().map(|n| n["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids.len(), nodes.len(), "duplicate ids");
        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        for n in &nodes {
            for t in n["informs"].as_array().unwrap() {
                let t = t.as_str().unwrap();
                assert!(ids.contains(t), "edge to unknown node {t}");
                edges.entry(n["id"].as_str().unwrap().to_string()).or_default().push(t.to_string());
            }
        }
        let mut seen = BTreeSet::from([META_SOUL.to_string()]);
        let mut todo = vec![META_SOUL.to_string()];
        while let Some(x) = todo.pop() {
            for t in edges.get(&x).into_iter().flatten() {
                if seen.insert(t.clone()) {
                    todo.push(t.clone());
                }
            }
        }
        let lost: Vec<_> = ids.difference(&seen).collect();
        assert!(lost.is_empty(), "unreachable from soul: {lost:?}");
    }

    #[test]
    fn meta_graph_has_the_required_rings() {
        let nodes = meta_nodes();
        let groups: BTreeSet<&str> = nodes.iter().filter_map(|n| n["group"].as_str()).collect();
        for g in ["soul", "principles", "types", "views", "tools", "recipes", "patterns", "testing", "failures", "policies"] {
            assert!(groups.contains(g), "missing group {g}");
        }
        for id in ["decide_vs_ask", "recipe_bulk", "recipe_callback", "view_canvas", "view_library", "app_teleprompter", "app_labelling", "app_recorder", "view_queue", "view_media_play", "view_history", "test_review"] {
            assert!(meta_find(id).is_some(), "missing node {id}");
        }
        assert!(meta_topic("soul").unwrap().contains("IN BULK"));
        assert!(meta_overview().starts_with("[soul]"));
        assert!(meta_topic("nope").is_err());
        assert_eq!(meta_graph_nodes()["steps"].as_array().unwrap().len(), nodes.len());
    }

    #[test]
    fn every_registered_tool_is_covered_by_a_tool_family_node() {
        let text: String = meta_nodes().iter().filter(|n| n["group"] == "tools").map(|n| n["text"].as_str().unwrap().to_string()).collect::<Vec<_>>().join(" ");
        let skip = ["trusted_source_approve"];
        for t in crate::tools::TOOLS.iter() {
            if skip.contains(&t.name) || t.name == "help" {
                continue;
            }
            assert!(text.contains(t.name), "tool {} is in no tool family node", t.name);
        }
    }
}
