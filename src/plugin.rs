//! Plugins: one input file adds nodes to the gallery, external apps (human-owned
//! work) to Connections, a settings section, and tools for the AI.
//!
//! Policy (enforced in `validate`, documented in `docs/PLUGINS.md`):
//! - One UTF-8 JSON file, at most 256 KB, `"format": 1`.
//! - It declares what it touches in `permissions` (`nodes`, `connections`,
//!   `settings`, `tools`); content outside the declared list is rejected.
//! - Plugin, node, connection, tool and setting ids are lowercase slugs, never
//!   equal to a built-in id. Counts and text sizes are capped.
//! - No secrets in the file: a key is the *name* of an environment variable.
//! - Connections carry an `https://` link and a key env name, never an install
//!   command: Ventosa does not run code a plugin brings for Connect.
//! - `tools` run shell commands, so a plugin that declares them can only be
//!   installed by a human (Settings ▸ Plugins), never by the AI.
//! - A plugin the AI installs starts **disabled**; a human enables it.
//! - Installing, enabling and removing land in the audit log.
//!
//! Stored in `$XDG_DATA_HOME/ventosa/plugins/<id>/`: `plugin.json` (the file as
//! installed) and `state.json` (enabled, hash, who, setting values, which
//! connections the human marked connected).

use crate::gallery::NodeDef;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 256 * 1024;
pub const MAX_NODES: usize = 40;
pub const MAX_CONNECTIONS: usize = 20;
pub const MAX_FIELDS: usize = 20;
pub const MAX_TOOLS: usize = 10;
pub const MAX_SKILL: usize = 8 * 1024;
pub const PERMISSIONS: [&str; 4] = ["nodes", "connections", "settings", "tools"];
const FIELD_KINDS: [&str; 4] = ["text", "toggle", "choice", "number"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PNode {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub need: String,
    pub skill: String,
    pub kind: String,
    pub app: String,
    pub per: String,
    pub path: String,
}

/// An external app a human-owned node works in (Figma, Blender, a DAW…).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PConnection {
    pub id: String,
    pub name: String,
    pub what: String,
    pub link: String,
    pub key_env: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PField {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub default: Value,
    pub options: Vec<String>,
    pub help: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PSettings {
    pub title: String,
    pub fields: Vec<PField>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PTool {
    pub name: String,
    pub description: String,
    pub command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Plugin {
    pub format: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub nodes: Vec<PNode>,
    pub connections: Vec<PConnection>,
    pub settings: PSettings,
    pub tools: Vec<PTool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    pub enabled: bool,
    pub sha256: String,
    pub installed: u64,
    pub by: String,
    /// Settings values by field key.
    pub values: serde_json::Map<String, Value>,
    /// Connections a human marked as set up (no key env to check).
    pub connected: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub plugin: Plugin,
    pub state: State,
}

pub fn dir() -> PathBuf {
    std::env::var_os("VENTOSA_PLUGINS").map(PathBuf::from).unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("plugins"))
}

fn slug(s: &str, extra: &[char], max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra.contains(&c)) && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
}

fn clean(s: &str, max: usize) -> bool {
    s.len() <= max && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
}

fn env_name(s: &str) -> bool {
    s.is_empty() || (s.len() <= 64 && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && !s.starts_with(|c: char| c.is_ascii_digit()))
}

/// SHA-256 as hex (the file's fingerprint in `state.json`).
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Every reason the file breaks the policy; empty = it may be installed. `by_ai` adds the AI-only rules.
pub fn validate(p: &Plugin, by_ai: bool) -> Vec<String> {
    let mut e = vec![];
    if p.format != 1 {
        e.push("format must be 1".into());
    }
    if !slug(&p.id, &['-'], 40) {
        e.push("id: lowercase letters, digits and '-', starting with a letter, at most 40".into());
    }
    if p.name.trim().is_empty() || !clean(&p.name, 60) {
        e.push("name is required (at most 60 characters)".into());
    }
    if p.version.is_empty() || p.version.len() > 20 || !p.version.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c.is_ascii_alphabetic()) {
        e.push("version is required, like 1.0.0".into());
    }
    if !clean(&p.description, 600) || !clean(&p.author, 80) {
        e.push("description (600) and author (80) must be plain text".into());
    }
    for perm in &p.permissions {
        if !PERMISSIONS.contains(&perm.as_str()) {
            e.push(format!("unknown permission {perm}; allowed: {}", PERMISSIONS.join(", ")));
        }
    }
    let has = |x: &str| p.permissions.iter().any(|q| q == x);
    for (n, label, used) in [("nodes", "nodes", !p.nodes.is_empty()), ("connections", "connections", !p.connections.is_empty()), ("settings", "settings", !p.settings.fields.is_empty()), ("tools", "tools", !p.tools.is_empty())] {
        if used && !has(n) {
            e.push(format!("{label} are present but the permission \"{n}\" is not declared"));
        }
    }
    if by_ai && has("tools") {
        e.push("a plugin with tools runs shell commands: only a human installs it (Settings ▸ Plugins)".into());
    }
    if p.nodes.len() > MAX_NODES || p.connections.len() > MAX_CONNECTIONS || p.settings.fields.len() > MAX_FIELDS || p.tools.len() > MAX_TOOLS {
        e.push(format!("too many entries (max {MAX_NODES} nodes, {MAX_CONNECTIONS} connections, {MAX_FIELDS} setting fields, {MAX_TOOLS} tools)"));
    }
    let common: Vec<String> = crate::gallery::common().into_iter().map(|n| n.id).collect();
    let mut seen: Vec<&str> = vec![];
    for n in &p.nodes {
        if !slug(&n.id, &['_'], 40) {
            e.push(format!("node id {:?}: lowercase letters, digits and '_', at most 40", n.id));
        }
        if common.contains(&n.id) || seen.contains(&n.id.as_str()) {
            e.push(format!("node id {} is taken (a built-in or a duplicate)", n.id));
        }
        seen.push(&n.id);
        if n.owner != "ai" && n.owner != "human" {
            e.push(format!("node {}: owner must be ai or human", n.id));
        }
        if n.need.trim().is_empty() || !clean(&n.need, 160) {
            e.push(format!("node {}: need is required (one line, at most 160)", n.id));
        }
        if n.skill.len() > MAX_SKILL || !clean(&n.skill, MAX_SKILL) {
            e.push(format!("node {}: skill is longer than {} bytes or has control characters", n.id, MAX_SKILL));
        }
        if !["", "folder", "research"].contains(&n.kind.as_str()) || !["", "clip"].contains(&n.per.as_str()) {
            e.push(format!("node {}: kind must be empty or folder; per empty or clip", n.id));
        }
        if n.path.contains("..") || n.path.starts_with('/') || n.path.len() > 300 || !clean(&n.app, 40) {
            e.push(format!("node {}: path must stay inside the workspace", n.id));
        }
    }
    let builtin: Vec<&str> = crate::recommend::RECS.iter().map(|r| r.id).collect();
    let mut seen: Vec<&str> = vec![];
    for c in &p.connections {
        if !slug(&c.id, &['-'], 40) || builtin.contains(&c.id.as_str()) || seen.contains(&c.id.as_str()) {
            e.push(format!("connection id {:?}: a free lowercase slug (not a built-in or duplicate)", c.id));
        }
        seen.push(&c.id);
        if c.name.trim().is_empty() || !clean(&c.name, 60) || !clean(&c.what, 300) {
            e.push(format!("connection {}: name and what must be plain text", c.id));
        }
        if !c.link.is_empty() && (!c.link.starts_with("https://") || c.link.len() > 300 || c.link.contains(char::is_whitespace)) {
            e.push(format!("connection {}: link must be an https:// URL", c.id));
        }
        if !env_name(&c.key_env) {
            e.push(format!("connection {}: key_env is the NAME of an environment variable (A-Z, 0-9, _), never the key", c.id));
        }
    }
    let mut seen: Vec<&str> = vec![];
    for f in &p.settings.fields {
        if !slug(&f.key, &['_'], 40) || seen.contains(&f.key.as_str()) {
            e.push(format!("setting key {:?}: a free lowercase slug", f.key));
        }
        seen.push(&f.key);
        if !FIELD_KINDS.contains(&f.kind.as_str()) {
            e.push(format!("setting {}: kind must be one of {}", f.key, FIELD_KINDS.join(", ")));
        }
        if f.kind == "choice" && f.options.is_empty() {
            e.push(format!("setting {}: a choice needs options", f.key));
        }
        if f.label.trim().is_empty() || !clean(&f.label, 60) || !clean(&f.help, 200) {
            e.push(format!("setting {}: label is required", f.key));
        }
    }
    let mut seen: Vec<&str> = vec![];
    for t in &p.tools {
        if !slug(&t.name, &['_'], 40) || seen.contains(&t.name.as_str()) {
            e.push(format!("tool name {:?}: a free lowercase slug", t.name));
        }
        seen.push(&t.name);
        if t.command.trim().is_empty() || t.command.len() > 2000 || !clean(&t.description, 300) {
            e.push(format!("tool {}: a command (at most 2000 bytes) and a plain description are required", t.name));
        }
    }
    e
}

/// Parse the one input file. Errors name what is wrong.
pub fn parse(text: &str) -> Result<Plugin, Vec<String>> {
    if text.len() > MAX_BYTES {
        return Err(vec![format!("the file is larger than {} KB", MAX_BYTES / 1024)]);
    }
    serde_json::from_str::<Plugin>(text).map_err(|e| vec![format!("not a plugin file: {e}")])
}

fn plugin_dir(id: &str) -> PathBuf {
    dir().join(id)
}

fn read_state(id: &str) -> State {
    std::fs::read_to_string(plugin_dir(id).join("state.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_state(id: &str, s: &State) -> Result<(), String> {
    crate::save(&plugin_dir(id).join("state.json"), s).map_err(|e| e.to_string())
}

/// Every installed plugin, by id.
pub fn list() -> Vec<Installed> {
    let mut out: Vec<Installed> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().to_string();
            let plugin = serde_json::from_str::<Plugin>(&std::fs::read_to_string(e.path().join("plugin.json")).ok()?).ok()?;
            Some(Installed { state: read_state(&id), plugin })
        })
        .collect();
    out.sort_by(|a, b| a.plugin.id.cmp(&b.plugin.id));
    out
}

/// Install (or upgrade) from the text of the one input file. `by_ai` installs it disabled and refuses shell tools.
pub fn install(text: &str, actor: &str, by_ai: bool) -> Result<Installed, String> {
    let plugin = parse(text).map_err(|e| e.join("; "))?;
    let errs = validate(&plugin, by_ai);
    if !errs.is_empty() {
        crate::identity::audit(actor, "plugin.install", &plugin.id, false);
        return Err(format!("plugin {} rejected:\n- {}", if plugin.id.is_empty() { "?" } else { &plugin.id }, errs.join("\n- ")));
    }
    let hash = sha256(text.as_bytes());
    let prev = list().into_iter().find(|i| i.plugin.id == plugin.id);
    if let Some(p) = &prev {
        if p.plugin.version == plugin.version && p.state.sha256 != hash {
            return Err(format!("{} {} is installed with other content: raise the version to upgrade, or remove it first", plugin.id, plugin.version));
        }
    }
    let d = plugin_dir(&plugin.id);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join("plugin.json"), text).map_err(|e| e.to_string())?;
    let prev_is_none = prev.is_none();
    let mut state = prev.map(|p| p.state).unwrap_or_default();
    state.sha256 = hash;
    state.installed = crate::now();
    state.by = actor.to_string();
    if by_ai {
        state.enabled = false;
    } else if prev_is_none {
        state.enabled = true;
    }
    // defaults for new fields
    for f in &plugin.settings.fields {
        state.values.entry(f.key.clone()).or_insert_with(|| f.default.clone());
    }
    write_state(&plugin.id, &state)?;
    crate::identity::audit(actor, if by_ai { "plugin.install.ai" } else { "plugin.install" }, &plugin.id, true);
    Ok(Installed { plugin, state })
}

/// Read a file (size checked first) and install it.
pub fn install_file(path: &Path, actor: &str, by_ai: bool) -> Result<Installed, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len as usize > MAX_BYTES {
        return Err(format!("the file is larger than {} KB", MAX_BYTES / 1024));
    }
    install(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?, actor, by_ai)
}

pub fn remove(id: &str, actor: &str) -> Result<(), String> {
    if !slug(id, &['-'], 40) {
        return Err(format!("bad plugin id {id}"));
    }
    let d = plugin_dir(id);
    if !d.is_dir() {
        return Err(format!("no plugin {id}"));
    }
    std::fs::remove_dir_all(&d).map_err(|e| e.to_string())?;
    crate::identity::audit(actor, "plugin.remove", id, true);
    Ok(())
}

pub fn set_enabled(id: &str, on: bool, actor: &str) -> Result<(), String> {
    let mut s = list().into_iter().find(|i| i.plugin.id == id).ok_or_else(|| format!("no plugin {id}"))?.state;
    s.enabled = on;
    write_state(id, &s)?;
    crate::identity::audit(actor, if on { "plugin.enable" } else { "plugin.disable" }, id, true);
    Ok(())
}

/// Save one setting value, checked against the plugin's declared field.
pub fn set_value(id: &str, key: &str, value: Value) -> Result<(), String> {
    let i = list().into_iter().find(|i| i.plugin.id == id).ok_or_else(|| format!("no plugin {id}"))?;
    let f = i.plugin.settings.fields.iter().find(|f| f.key == key).ok_or_else(|| format!("{id} has no setting {key}"))?;
    let ok = match f.kind.as_str() {
        "toggle" => value.is_boolean(),
        "number" => value.is_number(),
        "choice" => value.as_str().is_some_and(|v| f.options.iter().any(|o| o == v)),
        _ => value.as_str().is_some_and(|v| v.len() <= 500),
    };
    if !ok {
        return Err(format!("{key}: wrong value for a {} setting", f.kind));
    }
    let mut s = i.state;
    s.values.insert(key.to_string(), value);
    write_state(id, &s)
}

/// Mark an external app as set up (or not) by the human.
pub fn set_connected(id: &str, conn: &str, on: bool) -> Result<(), String> {
    let mut s = list().into_iter().find(|i| i.plugin.id == id).ok_or_else(|| format!("no plugin {id}"))?.state;
    s.connected.retain(|c| c != conn);
    if on {
        s.connected.push(conn.to_string());
    }
    write_state(id, &s)
}

/// Gallery nodes of the enabled plugins (source `plugin:<id>`).
pub fn nodes() -> Vec<NodeDef> {
    list()
        .into_iter()
        .filter(|i| i.state.enabled)
        .flat_map(|i| {
            let src = format!("plugin:{}", i.plugin.id);
            i.plugin.nodes.into_iter().map(move |n| {
                let mut extras = serde_json::Map::new();
                for (k, v) in [("kind", &n.kind), ("app", &n.app), ("per", &n.per), ("path", &n.path)] {
                    if !v.is_empty() {
                        extras.insert(k.into(), json!(v));
                    }
                }
                let skill = format!("# Node skill: {} [{}]\n{}\n\n{}\n", n.id, if n.owner == "ai" { "AI" } else { "human" }, n.need, n.skill.trim());
                NodeDef { id: n.id, owner: n.owner, need: n.need, skill, source: src.clone(), extras: if extras.is_empty() { Value::Null } else { Value::Object(extras) } }
            })
        })
        .collect()
}

/// External apps of the enabled plugins: (plugin id, connection, set up?).
pub fn connections() -> Vec<(String, PConnection, bool)> {
    list()
        .into_iter()
        .filter(|i| i.state.enabled)
        .flat_map(|i| {
            let (pid, connected) = (i.plugin.id.clone(), i.state.connected.clone());
            i.plugin.connections.into_iter().map(move |c| {
                let ok = if c.key_env.is_empty() { connected.contains(&c.id) } else { std::env::var(&c.key_env).is_ok_and(|v| !v.is_empty()) };
                (pid.clone(), c, ok)
            })
        })
        .collect()
}

/// Copy a plugin's tools into a workspace's `pipeline/tools.json` as `<plugin>_<tool>`, run later with `tool_run`.
pub fn install_tools(id: &str, ws: &crate::config::Workspace) -> Result<usize, String> {
    let i = list().into_iter().find(|i| i.plugin.id == id).ok_or_else(|| format!("no plugin {id}"))?;
    if !i.state.enabled {
        return Err(format!("{id} is disabled: enable it first"));
    }
    let path = crate::graph::root(ws).join("pipeline").join("tools.json");
    let mut defs: Vec<Value> = crate::load(&path);
    for t in &i.plugin.tools {
        let entry = json!({"name": format!("{}_{}", id.replace('-', "_"), t.name), "description": t.description, "command": t.command, "plugin": id});
        match defs.iter_mut().find(|d| d["name"] == entry["name"]) {
            Some(d) => *d = entry,
            None => defs.push(entry),
        }
    }
    crate::save(&path, &defs).map_err(|e| e.to_string())?;
    Ok(i.plugin.tools.len())
}

/// The summary the AI and the settings page show for one plugin.
pub fn summary(i: &Installed) -> Value {
    json!({
        "id": i.plugin.id, "name": i.plugin.name, "version": i.plugin.version, "enabled": i.state.enabled,
        "permissions": i.plugin.permissions, "by": i.state.by,
        "nodes": i.plugin.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
        "connections": i.plugin.connections.iter().map(|c| &c.id).collect::<Vec<_>>(),
        "settings": i.plugin.settings.fields.iter().map(|f| &f.key).collect::<Vec<_>>(),
        "tools": i.plugin.tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        json!({
            "format": 1, "id": "pixel-art", "name": "Pixel Art", "version": "1.0.0", "author": "me",
            "permissions": ["nodes", "connections", "settings"],
            "nodes": [{"id": "pixelate", "owner": "ai", "need": "redraw stills as pixel art", "skill": "**Do:** pixelate each still."}],
            "connections": [{"id": "aseprite", "name": "Aseprite", "what": "pixel editor", "link": "https://www.aseprite.org", "key_env": ""}],
            "settings": {"title": "Pixel Art", "fields": [{"key": "palette", "label": "Palette", "kind": "choice", "default": "pico8", "options": ["pico8", "gameboy"]}]}
        })
        .to_string()
    }

    #[test]
    fn a_good_file_validates_and_a_broken_one_lists_every_reason() {
        let p = parse(&sample()).unwrap();
        assert!(validate(&p, false).is_empty());
        let mut bad = p.clone();
        bad.id = "Bad Id".into();
        bad.nodes[0].id = "pool".into();
        bad.connections[0].link = "http://x".into();
        bad.connections[0].key_env = "sk-123abc".into();
        bad.permissions.retain(|x| x != "nodes");
        let e = validate(&bad, false);
        assert!(e.iter().any(|m| m.contains("id:")) && e.iter().any(|m| m.contains("taken")) && e.iter().any(|m| m.contains("https")) && e.iter().any(|m| m.contains("NAME")) && e.iter().any(|m| m.contains("not declared")), "{e:?}");
    }

    #[test]
    fn the_shipped_example_is_valid() {
        let p = parse(include_str!("../docs/examples/pixel-art.ventosa-plugin.json")).unwrap();
        assert!(validate(&p, true).is_empty(), "{:?}", validate(&p, true));
    }

    #[test]
    fn the_ai_cannot_bring_shell_tools_and_starts_disabled() {
        let mut p = parse(&sample()).unwrap();
        p.permissions.push("tools".into());
        p.tools.push(PTool { name: "t".into(), description: "d".into(), command: "echo".into() });
        assert!(validate(&p, true).iter().any(|m| m.contains("only a human")));
        assert!(validate(&p, false).is_empty());
    }

    #[test]
    fn install_enable_nodes_and_remove() {
        let root = std::env::temp_dir().join(format!("ventosa-plugins-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        unsafe {
            std::env::set_var("VENTOSA_PLUGINS", &root);
            // the audit log of the test goes to the temp dir, not to the user's
            std::env::set_var("XDG_DATA_HOME", root.join("data"));
        }
        let ai = install(&sample(), "ai", true).unwrap();
        assert!(!ai.state.enabled);
        assert!(nodes().is_empty(), "disabled plugins add nothing");
        set_enabled("pixel-art", true, "me").unwrap();
        assert_eq!(nodes()[0].source, "plugin:pixel-art");
        assert!(set_value("pixel-art", "palette", json!("gameboy")).is_ok());
        assert!(set_value("pixel-art", "palette", json!("neon")).is_err());
        let again = sample().replace("redraw stills", "redraw frames");
        assert!(install(&again, "me", false).unwrap_err().contains("raise the version"));
        remove("pixel-art", "me").unwrap();
        assert!(list().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
