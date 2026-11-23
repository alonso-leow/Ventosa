//! Trusted sources: the references every scene must be built from so that
//! characters, assets, places, styles and voices stay the same across scenes
//! and across videos. One array per workspace in `knowledge/trusted-sources.json`
//! (it travels with the workspace, beside the rest of its knowledge).
//!
//! A source is proposed (by the AI or a human) with `approved: false`; only
//! approved sources are binding. The AI never invents the look of a recurring
//! element: it builds over the approved refs, and when none exists it proposes
//! one and raises a decision.

use crate::config::Workspace;
use crate::graph;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const KINDS: [&str; 6] = ["character", "asset", "location", "style", "voice", "brand"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Source {
    pub id: String,
    /// character | asset | location | style | voice | brand
    pub kind: String,
    pub name: String,
    /// Files under the workspace (reference sheets, turnarounds, model files, LUTs, voice samples) or URLs.
    pub refs: Vec<String>,
    /// Traits that must not change between scenes ("red scarf, left side", "logo 8% of width, top right").
    pub locked: Vec<String>,
    /// Seeds, prompt fragments, model/LoRA names that reproduce it.
    pub recipe: String,
    pub notes: String,
    pub approved: bool,
    pub added_by: String,
    pub added: u64,
    /// Subjects (videos) and scenes that used it, as `V03#s2`.
    pub used_in: Vec<String>,
}

pub fn path(ws: &Workspace) -> PathBuf {
    graph::root(ws).join("knowledge").join("trusted-sources.json")
}

pub fn load(ws: &Workspace) -> Vec<Source> {
    std::fs::read_to_string(path(ws)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(ws: &Workspace, list: &[Source]) -> Result<(), String> {
    crate::save(&path(ws), &list.to_vec()).map_err(|e| e.to_string())
}

/// Add a proposal (or replace one with the same id that is not approved yet).
pub fn propose(ws: &Workspace, mut s: Source) -> Result<String, String> {
    if s.name.trim().is_empty() {
        return Err("a trusted source needs a name".into());
    }
    if !KINDS.contains(&s.kind.as_str()) {
        return Err(format!("kind is one of {}", KINDS.join(", ")));
    }
    if s.id.is_empty() {
        s.id = format!("{}-{}", s.kind, s.name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"));
    }
    s.added = crate::now();
    let mut list = load(ws);
    match list.iter_mut().find(|x| x.id == s.id) {
        Some(x) if x.approved => return Err(format!("{} is approved; a human changes it (or propose a new id)", s.id)),
        Some(x) => *x = s.clone(),
        None => list.push(s.clone()),
    }
    save(ws, &list)?;
    Ok(s.id)
}

pub fn set_approved(ws: &Workspace, id: &str, approved: bool) -> Result<(), String> {
    let mut list = load(ws);
    let s = list.iter_mut().find(|x| x.id == id).ok_or_else(|| format!("no trusted source {id}"))?;
    s.approved = approved;
    save(ws, &list)
}

pub fn mark_used(ws: &Workspace, ids: &[String], scene: &str) -> Result<(), String> {
    let mut list = load(ws);
    for s in list.iter_mut().filter(|s| ids.contains(&s.id)) {
        if !s.used_in.iter().any(|u| u == scene) {
            s.used_in.push(scene.to_string());
        }
    }
    save(ws, &list)
}

/// The binding block a prompt gets: every approved source, then the proposals.
pub fn prompt_block(ws: &Workspace) -> String {
    let list = load(ws);
    if list.is_empty() {
        return "Trusted sources: none yet. Before the first scene with a recurring character or asset, propose one (trusted_source_add) and raise a decision.".into();
    }
    let mut out = String::from("Trusted sources (binding when approved):\n");
    for s in &list {
        out.push_str(&format!(
            "- [{}] {} · {} · {}\n  refs: {}\n  locked: {}\n  recipe: {}\n",
            if s.approved { "approved" } else { "proposed" },
            s.id,
            s.kind,
            s.name,
            s.refs.join(", "),
            s.locked.join("; "),
            s.recipe
        ));
    }
    out
}
