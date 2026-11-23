//! The entry hook and the live monitor.
//!
//! Every human entry (a label save, a recording, an app form, a `step.done`)
//! lands in the workspace's `pipeline/events.jsonl`, written by the engine with
//! the logged-in account in `user`. `poll` reads what is new since the last pass,
//! re-syncs the queue (so the AI item for the next phase exists) and fires the
//! cron jobs with `on_entry` whose phase now has a ready item. A worker in the
//! launcher therefore enables the AI tasks and updates the admin's monitor with
//! no step in between. Presence files (`pipeline/presence/<user>-<level>.json`)
//! say who is in the workspace, at which level (admin graph / worker launcher).

use crate::config::{Config, Workspace};
use crate::{graph, queue_path, runner};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

const PRESENCE_TTL: u64 = 90;

fn offsets_path() -> PathBuf {
    crate::default_path().with_file_name("event-offsets.json")
}

fn events_file(ws: &Workspace) -> PathBuf {
    graph::root(ws).join("pipeline").join("events.jsonl")
}

/// New events of a workspace since the last poll (the first poll only marks the end).
fn new_events(ws: &Workspace, offsets: &mut HashMap<String, u64>) -> Vec<Value> {
    let f = events_file(ws);
    let Ok(mut file) = std::fs::File::open(&f) else { return vec![] };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let Some(&from) = offsets.get(&ws.name) else {
        offsets.insert(ws.name.clone(), len);
        return vec![];
    };
    let from = if from > len { 0 } else { from };
    offsets.insert(ws.name.clone(), len);
    if from == len || file.seek(SeekFrom::Start(from)).is_err() {
        return vec![];
    }
    let mut text = String::new();
    let _ = file.read_to_string(&mut text);
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// One pass of the hook: returns a line per fired job. `block` as in `runner::tick`.
pub fn poll(cfg: &Config, block: bool) -> Vec<String> {
    let mut offsets: HashMap<String, u64> = crate::load(&offsets_path());
    let mut out = vec![];
    let mut moved = false;
    for ws in cfg.workspaces.iter().filter(|w| w.enabled) {
        let evs = new_events(ws, &mut offsets);
        let human: Vec<&Value> = evs.iter().filter(|e| e["actor"] == "human").collect();
        if human.is_empty() {
            continue;
        }
        moved = true;
        let who: Vec<String> = human.iter().map(|e| e["user"].as_str().unwrap_or("someone").to_string()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        if let Err(e) = graph::sync(ws, &queue_path()) {
            out.push(format!("entry hook {}: sync failed: {e}", ws.name));
            continue;
        }
        for s in cfg.schedules.iter().filter(|s| s.enabled && s.on_entry && s.workspace == ws.name) {
            match runner::fire_as(cfg, s, block, "entry") {
                Ok(Some(m)) => out.push(format!("entry from {} → {m}", who.join(", "))),
                Ok(None) => {}
                Err(e) => out.push(format!("{}: {e}", s.id)),
            }
        }
    }
    if moved || !offsets_path().exists() {
        let _ = crate::save(&offsets_path(), &offsets);
    }
    out
}

/// The latest human entries of a workspace, newest first: `{at, kind, vid, user}`.
pub fn recent_entries(ws: &Workspace, limit: usize) -> Vec<Value> {
    let text = std::fs::read_to_string(events_file(ws)).unwrap_or_default();
    text.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|e| e["actor"] == "human").take(limit).collect()
}

fn presence_dir(ws: &Workspace) -> PathBuf {
    graph::root(ws).join("pipeline").join("presence")
}

/// Say that `user` is here, at `level` (`admin` = the graph app, `worker` = the launcher) doing `doing`.
pub fn beat(ws: &Workspace, user: &str, level: &str, doing: &str) {
    let dir = presence_dir(ws);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let name: String = user.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let row = json!({"user": user, "level": level, "doing": doing, "at": crate::now(), "pid": std::process::id()});
    let _ = std::fs::write(dir.join(format!("{name}-{level}.json")), row.to_string());
}

/// Remove this account's presence (logout / exit).
pub fn leave(ws: &Workspace, user: &str, level: &str) {
    let name: String = user.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let _ = std::fs::remove_file(presence_dir(ws).join(format!("{name}-{level}.json")));
}

/// Who is in the workspace now (seen within the TTL), most recent first.
pub fn present(ws: &Workspace) -> Vec<Value> {
    let mut rows: Vec<Value> = std::fs::read_dir(presence_dir(ws))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|r| crate::now().saturating_sub(r["at"].as_u64().unwrap_or(0)) <= PRESENCE_TTL)
        .collect();
    rows.sort_by(|a, b| b["at"].as_u64().cmp(&a["at"].as_u64()));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_poll_marks_the_end_then_reads_only_new_lines() {
        let dir = std::env::temp_dir().join(format!("vh-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pipeline")).unwrap();
        let ws = Workspace { name: "w".into(), root: dir.to_string_lossy().into(), engine: "ventosa".into(), enabled: true };
        let f = dir.join("pipeline").join("events.jsonl");
        std::fs::write(&f, "{\"actor\":\"human\",\"kind\":\"old\"}\n").unwrap();
        let mut off = HashMap::new();
        assert!(new_events(&ws, &mut off).is_empty());
        std::fs::write(&f, "{\"actor\":\"human\",\"kind\":\"old\"}\n{\"actor\":\"human\",\"kind\":\"new\",\"user\":\"ana\"}\n").unwrap();
        let evs = new_events(&ws, &mut off);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["user"], "ana");
        assert!(new_events(&ws, &mut off).is_empty());
    }

    #[test]
    fn presence_expires() {
        let dir = std::env::temp_dir().join(format!("vp-{}", std::process::id()));
        let ws = Workspace { name: "w".into(), root: dir.to_string_lossy().into(), engine: "ventosa".into(), enabled: true };
        beat(&ws, "ana", "worker", "labelling");
        assert_eq!(present(&ws).len(), 1);
        leave(&ws, "ana", "worker");
        assert!(present(&ws).is_empty());
    }
}
