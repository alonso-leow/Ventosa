//! Link-media watcher: every 10 minutes, if a teleprompter ran (new or changed session logs) or the
//! watched media folders changed, recompute the take -> file mapping (`tplog::refresh`).
//! Cheap when nothing changed: one directory listing per watched folder, no ffprobe.

use crate::config::{Config, Workspace};
use crate::{graph, tplog};
use std::path::Path;
use std::sync::Mutex;

/// Seconds between checks.
pub const EVERY: u64 = 600;
const WATCHED: [&str; 4] = ["assets/camera", "assets/camera/inbox", "assets/edits", "recording/sessions"];

static LAST: Mutex<u64> = Mutex::new(0);

fn stamp(p: &Path, acc: &mut u64) {
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            continue;
        }
        let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0);
        for b in e.file_name().to_string_lossy().bytes().chain(m.len().to_le_bytes()).chain(t.to_le_bytes()) {
            *acc = (*acc ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
}

/// Fingerprint of the session logs and the watched folders.
pub fn fingerprint(ws: &Workspace) -> String {
    let root = graph::root(ws);
    let mut acc = 0xcbf29ce484222325u64;
    for d in WATCHED {
        stamp(&root.join(d), &mut acc);
    }
    format!("{acc:016x}")
}

fn state_path(ws: &Workspace) -> std::path::PathBuf {
    graph::root(ws).join("recording").join(".linkmedia")
}

/// One workspace: when a teleprompter session exists and the fingerprint changed, refresh the mapping.
/// Returns a log line when it ran.
pub fn check(ws: &Workspace) -> Option<String> {
    let root = graph::root(ws);
    if !tplog::sessions_dir(ws).is_dir() {
        return None;
    }
    let fp = fingerprint(ws);
    if std::fs::read_to_string(state_path(ws)).ok().as_deref() == Some(fp.as_str()) {
        return None;
    }
    let map = tplog::refresh(ws);
    let _ = std::fs::create_dir_all(root.join("recording"));
    let _ = std::fs::write(state_path(ws), &fp);
    let linked = map.matches.iter().filter(|m| m.method != "unmatched").count();
    Some(format!("link-media {}: {} take(s), {linked}/{} file(s) linked", ws.name, map.takes.len(), map.matches.len()))
}

/// Due every `EVERY` seconds (the first call runs at once). Safe to call from several loops.
pub fn tick(cfg: &Config) -> Vec<String> {
    tick_at(cfg, crate::now())
}

pub fn tick_at(cfg: &Config, now: u64) -> Vec<String> {
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if *last != 0 && now < *last + EVERY {
            return vec![];
        }
        *last = now;
    }
    cfg.workspaces.iter().filter(|w| w.enabled).filter_map(check).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(tag: &str) -> Workspace {
        let root = std::env::temp_dir().join(format!("ventosa-lm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/camera")).unwrap();
        Workspace { name: tag.into(), root: root.display().to_string(), engine: "ventosa".into(), enabled: true }
    }

    #[test]
    fn runs_only_when_something_changed() {
        let w = ws("chg");
        assert!(check(&w).is_none(), "no teleprompter session yet");
        let root = graph::root(&w);
        std::fs::create_dir_all(root.join("recording/sessions")).unwrap();
        std::fs::write(root.join("recording/sessions/s1.jsonl"), "{\"t\":\"2026-11-19T10:00:00Z\",\"ev\":\"rec_start\"}\n{\"t\":\"2026-11-19T10:00:20Z\",\"ev\":\"rec_stop\"}\n").unwrap();
        let first = check(&w).expect("new session runs");
        assert!(first.contains("1 take"), "{first}");
        assert!(root.join("recording/media_map.json").is_file());
        assert!(check(&w).is_none(), "idempotent: nothing changed");
        std::fs::write(root.join("assets/camera/V01-a.mp4"), b"x").unwrap();
        assert!(check(&w).is_some(), "a new media file triggers it");
        assert!(check(&w).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_graphs_have_the_link_media_node_after_record() {
        for t in [include_str!("../templates/media/pipeline/skill-graph/steps.json"), include_str!("../templates/podcast/pipeline/skill-graph/steps.json")] {
            let g: serde_json::Value = serde_json::from_str(t).unwrap();
            let ids = crate::callback::order(&g);
            let r = ids.iter().position(|n| n == "record").unwrap();
            assert_eq!(ids[r + 1], "link_media");
        }
    }

    #[test]
    fn tick_is_throttled() {
        let cfg = Config::default();
        *LAST.lock().unwrap() = 0;
        tick_at(&cfg, 1000);
        *LAST.lock().unwrap() = 1000;
        assert!(tick_at(&cfg, 1000 + EVERY - 1).is_empty());
        let _ = tick_at(&cfg, 1000 + EVERY);
        assert_eq!(*LAST.lock().unwrap(), 1000 + EVERY);
    }
}
