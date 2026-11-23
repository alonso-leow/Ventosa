//! Wire Ventosa into AI clients: MCP server registration plus the `ventosa` skill.
//! `ventosa-install` is the CLI. `run` is the entry point for other callers (an MCP
//! tool, the launcher). Config merges keep every other key, are idempotent, write
//! atomically (tmp + rename) and keep one `.bak` of the original file.

use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CLIENTS: [&str; 6] = ["claude", "opencode", "codex", "hermes", "zeroclaw", "openclaw"];
const SKILL_TEMPLATE: &str = include_str!("../skills/ventosa/SKILL.md");
const SUBTITLES_SKILL: &str = include_str!("../skills/ventosa-subtitles/SKILL.md");
const WORKSPACE_MARK: &str = "{{WORKSPACE}}";

/// The MCP tool list, from the shared registry (same tools as the built-in chat).
fn mcp_tools() -> String {
    let names: Vec<&str> = crate::tools::TOOLS.iter().map(|t| t.name).collect();
    format!("MCP tools (ventosa-mcp, also used by the built-in chat): {}", names.join(", "))
}

pub const USAGE: &str = "usage: ventosa-install [--client claude|opencode|codex|hermes|zeroclaw|openclaw|all]
                     [--scope user|project] [--project DIR] [--workspace NAME]
                     [--mcp-bin PATH] [--dry-run] [--print] [--uninstall]
  default client   every client whose config dir exists
  --print          show snippets, usage and the current INSTALL.md; changes nothing
  --dry-run        show what would be written; changes nothing
  --uninstall      remove the ventosa MCP entry and skill for the chosen clients
  after an install, restart the client";

#[derive(Debug, Clone, Default)]
pub struct Opts {
    /// Empty means every detected client.
    pub clients: Vec<String>,
    /// `Some` is project scope (writes inside this directory).
    pub project: Option<PathBuf>,
    pub workspace: Option<String>,
    pub mcp_bin: Option<PathBuf>,
    pub dry_run: bool,
    pub print: bool,
    pub uninstall: bool,
    /// Home directory override (tests, fake installs). `None` is the user's home.
    pub home: Option<PathBuf>,
    /// Data dir override (`$XDG_DATA_HOME/ventosa`). `None` is the default.
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub lines: Vec<String>,
}

/// Parse CLI args. `Ok(None)` means `--help` was asked for.
pub fn parse_args(args: &[String]) -> Result<Option<Opts>, String> {
    let mut o = Opts::default();
    let mut scope_user = false;
    let mut scope_project = false;
    let mut i = 0;
    let value = |i: &mut usize| -> Result<String, String> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| format!("{} needs a value", args[*i - 1]))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--client" => {
                let v = value(&mut i)?;
                for c in v.split(',').map(str::trim).filter(|c| !c.is_empty()) {
                    if c == "all" {
                        o.clients = CLIENTS.iter().map(|s| s.to_string()).collect();
                    } else if CLIENTS.contains(&c) {
                        o.clients.push(c.to_string());
                    } else {
                        return Err(format!("unknown client `{c}`"));
                    }
                }
            }
            "--scope" => match value(&mut i)?.as_str() {
                "user" => scope_user = true,
                "project" => scope_project = true,
                other => return Err(format!("unknown scope `{other}`")),
            },
            "--project" => {
                let p = PathBuf::from(value(&mut i)?);
                let abs = if p.is_absolute() { p } else { std::env::current_dir().map_err(|e| e.to_string())?.join(p) };
                o.project = Some(abs);
            }
            "--workspace" => o.workspace = Some(value(&mut i)?),
            "--mcp-bin" => o.mcp_bin = Some(PathBuf::from(value(&mut i)?)),
            "--dry-run" => o.dry_run = true,
            "--print" => o.print = true,
            "--uninstall" => o.uninstall = true,
            "--help" | "-h" => return Ok(None),
            other => return Err(format!("unknown argument `{other}`")),
        }
        i += 1;
    }
    if scope_user && (scope_project || o.project.is_some()) {
        return Err("--scope user conflicts with --project".into());
    }
    if scope_project && o.project.is_none() {
        o.project = Some(std::env::current_dir().map_err(|e| e.to_string())?);
    }
    Ok(Some(o))
}

/// Install (or uninstall, or print) for the chosen clients.
pub fn run(opts: &Opts) -> Result<Report, String> {
    let home = match &opts.home {
        Some(h) => h.clone(),
        None => dirs::home_dir().ok_or("no home directory")?,
    };
    let data = opts
        .data_dir
        .clone()
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| home.join(".local/share")).join("ventosa"));
    let bin = resolve_bin(opts, &home);
    let mut rep = Report::default();

    if opts.print {
        rep.lines.push(USAGE.into());
        rep.lines.push(String::new());
        let list: Vec<String> = if opts.clients.is_empty() { CLIENTS.iter().map(|s| s.to_string()).collect() } else { opts.clients.clone() };
        for c in &list {
            rep.lines.push(format!("# {c}"));
            rep.lines.extend(snippet(c, &bin, opts.workspace.as_deref()).lines().map(String::from));
            rep.lines.push(String::new());
        }
        if let Ok(text) = fs::read_to_string(data.join("INSTALL.md")) {
            rep.lines.push("# INSTALL.md".into());
            rep.lines.extend(text.lines().map(String::from));
        }
        return Ok(rep);
    }

    let clients = if opts.clients.is_empty() { detect(&home) } else { opts.clients.clone() };
    if clients.is_empty() {
        rep.lines.push("no AI client config found (looked in ~/.claude, ~/.config/opencode, ~/.codex, ~/.hermes, ~/.zeroclaw, ~/.openclaw); use --client NAME".into());
        return Ok(rep);
    }
    if !bin.exists() {
        rep.lines.push(format!("warning: {} does not exist yet; the entry is written anyway", bin.display()));
    }

    let mut ctx = Ctx {
        home: home.clone(),
        data: data.clone(),
        bin: bin.clone(),
        ws: opts.workspace.clone(),
        project: opts.project.clone(),
        dry: opts.dry_run,
        uninstall: opts.uninstall,
        // The claude CLI is only used against the real user home, never in dry runs or tests.
        cli: !opts.dry_run && opts.project.is_none() && is_real_home(&home) && find_in_path("claude").is_some(),
        lines: vec![],
    };

    let mut rows: Vec<(String, Vec<String>)> = vec![];
    let mut errors = vec![];
    for c in &clients {
        ctx.lines.push(format!("{c}:"));
        let res = match c.as_str() {
            "claude" => claude(&mut ctx),
            "opencode" => opencode(&mut ctx),
            "codex" => codex(&mut ctx),
            g => generic(&mut ctx, g),
        };
        match res {
            Ok(details) => rows.push((c.clone(), details)),
            Err(e) => {
                ctx.lines.push(format!("  FAILED: {e}"));
                errors.push(format!("{c}: {e}"));
            }
        }
    }
    rep.lines.extend(ctx.lines.drain(..));

    if opts.uninstall {
        rep.lines.push(if opts.dry_run { "dry run: nothing removed".into() } else { "uninstalled".into() });
    } else {
        let text = install_md(&rows, &bin, opts);
        if opts.dry_run {
            rep.lines.push("dry run: INSTALL.md not written".into());
        } else {
            ctx.write(&data.join("INSTALL.md"), &text, false)?;
            rep.lines.extend(ctx.lines.drain(..));
        }
        rep.lines.push(String::new());
        rep.lines.extend(text.lines().map(String::from));
    }

    if !errors.is_empty() {
        return Err(format!("{}\n{}", rep.lines.join("\n"), errors.join("\n")));
    }
    Ok(rep)
}

fn resolve_bin(opts: &Opts, home: &Path) -> PathBuf {
    let p = opts
        .mcp_bin
        .clone()
        .or_else(|| find_in_path("ventosa-mcp"))
        .unwrap_or_else(|| home.join(".local/bin/ventosa-mcp"));
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// True when `home` is the account's real home directory (read from /proc and /etc/passwd).
fn is_real_home(home: &Path) -> bool {
    real_home().is_some_and(|r| same_path(&r, home))
}

fn real_home() -> Option<PathBuf> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let uid = status.lines().find(|l| l.starts_with("Uid:"))?.split_whitespace().nth(1)?.to_string();
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 6 && f[2] == uid).then(|| PathBuf::from(f[5]))
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Clients whose config dir (or file) already exists under `home`.
fn detect(home: &Path) -> Vec<String> {
    let checks = [
        ("claude", home.join(".claude").is_dir() || home.join(".claude.json").is_file()),
        ("opencode", home.join(".config/opencode").is_dir()),
        ("codex", home.join(".codex").is_dir()),
        ("hermes", home.join(".hermes").is_dir()),
        ("zeroclaw", home.join(".zeroclaw").is_dir()),
        ("openclaw", home.join(".openclaw").is_dir()),
    ];
    checks.iter().filter(|(_, ok)| *ok).map(|(n, _)| n.to_string()).collect()
}

fn stdio_entry(bin: &Path, ws: Option<&str>) -> Value {
    let mut e = json!({"command": bin.display().to_string(), "args": [], "env": {}});
    if let Some(w) = ws {
        e["env"] = json!({"VENTOSA_WORKSPACE": w});
    }
    e
}

fn opencode_entry(bin: &Path, ws: Option<&str>) -> Value {
    let mut e = json!({"type": "local", "command": [bin.display().to_string()], "enabled": true});
    if let Some(w) = ws {
        e["environment"] = json!({"VENTOSA_WORKSPACE": w});
    }
    e
}

fn codex_entry(bin: &Path, ws: Option<&str>) -> toml::Value {
    let mut t = toml::Table::new();
    t.insert("command".into(), toml::Value::String(bin.display().to_string()));
    if let Some(w) = ws {
        let mut env = toml::Table::new();
        env.insert("VENTOSA_WORKSPACE".into(), toml::Value::String(w.into()));
        t.insert("env".into(), toml::Value::Table(env));
    }
    toml::Value::Table(t)
}

fn skill_text(ws: Option<&str>) -> String {
    SKILL_TEMPLATE.replace(WORKSPACE_MARK, ws.unwrap_or("none set (ask, or call workspaces_list)"))
}

/// Snippet a user or AI can paste by hand.
fn snippet(client: &str, bin: &Path, ws: Option<&str>) -> String {
    match client {
        "claude" => format!(
            "claude mcp add --scope user ventosa -- {}\n# or merge into ~/.claude.json:\n{}",
            bin.display(),
            json!({"mcpServers": {"ventosa": stdio_entry(bin, ws)}})
        ),
        "opencode" => json!({"mcp": {"ventosa": opencode_entry(bin, ws)}}).to_string(),
        "codex" => {
            let mut root = toml::Table::new();
            let mut servers = toml::Table::new();
            servers.insert("ventosa".into(), codex_entry(bin, ws));
            root.insert("mcp_servers".into(), toml::Value::Table(servers));
            toml::to_string_pretty(&root).unwrap_or_default()
        }
        g => format!("{}   (format unverified for {g})", json!({"mcpServers": {"ventosa": stdio_entry(bin, ws)}})),
    }
}

// ---- pure merge functions -------------------------------------------------

fn parse_json(s: Option<&str>) -> Result<Value, String> {
    match s {
        Some(s) if !s.trim().is_empty() => serde_json::from_str(s).map_err(|e| format!("invalid JSON: {e}")),
        _ => Ok(json!({})),
    }
}

fn to_json(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

/// Set `keys` (a path into the object) to `value`, creating objects on the way.
/// Returns the new text and whether anything changed. Other keys are kept.
pub fn json_set(existing: Option<&str>, keys: &[&str], value: Value) -> Result<(String, bool), String> {
    let (last, parents) = keys.split_last().ok_or("empty key path")?;
    let mut doc = parse_json(existing)?;
    let before = doc.clone();
    let mut cur = &mut doc;
    for k in parents {
        let obj = cur.as_object_mut().ok_or_else(|| format!("{k}: parent is not an object"))?;
        cur = obj.entry((*k).to_string()).or_insert_with(|| json!({}));
    }
    cur.as_object_mut()
        .ok_or("parent is not an object")?
        .insert((*last).to_string(), value);
    let changed = doc != before;
    Ok((to_json(&doc), changed))
}

/// Remove the key at `keys`. Returns the new text and whether anything changed.
pub fn json_unset(existing: &str, keys: &[&str]) -> Result<(String, bool), String> {
    let (last, parents) = keys.split_last().ok_or("empty key path")?;
    let mut doc = parse_json(Some(existing))?;
    let before = doc.clone();
    let mut cur = Some(&mut doc);
    for k in parents {
        cur = cur.and_then(|v| v.get_mut(*k));
    }
    if let Some(obj) = cur.and_then(Value::as_object_mut) {
        obj.remove(*last);
    }
    let changed = doc != before;
    Ok((to_json(&doc), changed))
}

/// TOML version of `json_set`: keeps every other table and key.
pub fn toml_set(existing: Option<&str>, keys: &[&str], value: toml::Value) -> Result<(String, bool), String> {
    let (last, parents) = keys.split_last().ok_or("empty key path")?;
    let mut doc: toml::Table = match existing {
        Some(s) if !s.trim().is_empty() => toml::from_str(s).map_err(|e| format!("invalid TOML: {e}"))?,
        _ => toml::Table::new(),
    };
    let before = doc.clone();
    let mut cur = &mut doc;
    for k in parents {
        let entry = cur
            .entry((*k).to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        cur = entry.as_table_mut().ok_or_else(|| format!("{k} is not a table"))?;
    }
    cur.insert((*last).to_string(), value);
    let changed = doc != before;
    Ok((toml::to_string_pretty(&doc).map_err(|e| e.to_string())?, changed))
}

/// TOML version of `json_unset`.
pub fn toml_unset(existing: &str, keys: &[&str]) -> Result<(String, bool), String> {
    let (last, parents) = keys.split_last().ok_or("empty key path")?;
    let mut doc: toml::Table = toml::from_str(existing).map_err(|e| format!("invalid TOML: {e}"))?;
    let before = doc.clone();
    let mut cur = Some(&mut doc);
    for k in parents {
        cur = cur.and_then(|t| t.get_mut(*k)).and_then(toml::Value::as_table_mut);
    }
    if let Some(t) = cur {
        t.remove(*last);
    }
    let changed = doc != before;
    Ok((toml::to_string_pretty(&doc).map_err(|e| e.to_string())?, changed))
}

// ---- file helpers and per-client work --------------------------------------

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = with_suffix(path, ".tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

fn read_opt(path: &Path) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(path).map(Some).map_err(|e| format!("{}: {e}", path.display()))
}

struct Ctx {
    home: PathBuf,
    data: PathBuf,
    bin: PathBuf,
    ws: Option<String>,
    project: Option<PathBuf>,
    dry: bool,
    uninstall: bool,
    cli: bool,
    lines: Vec<String>,
}

impl Ctx {
    /// Write `content`. `keep_original` saves the existing file to `.bak` once.
    fn write(&mut self, path: &Path, content: &str, keep_original: bool) -> Result<(), String> {
        if self.dry {
            self.lines.push(format!("  would write {}", path.display()));
            return Ok(());
        }
        let io_err = |e: io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        if keep_original && path.exists() {
            let bak = with_suffix(path, ".bak");
            if !bak.exists() {
                fs::copy(path, &bak).map_err(io_err)?;
            }
        }
        write_atomic(path, content).map_err(io_err)?;
        self.lines.push(format!("  wrote {}", path.display()));
        Ok(())
    }

    /// Set (or remove on uninstall) one key in a JSON config file.
    fn json_key(&mut self, path: &Path, keys: &[&str], entry: Value) -> Result<(), String> {
        let existing = read_opt(path)?;
        let (new, changed) = if self.uninstall {
            match existing.as_deref() {
                None => return Ok(()),
                Some(s) => json_unset(s, keys)?,
            }
        } else {
            json_set(existing.as_deref(), keys, entry)?
        };
        if !changed {
            self.lines.push(format!("  unchanged {}", path.display()));
            return Ok(());
        }
        self.write(path, &new, true)
    }

    /// Set (or remove on uninstall) one key in a TOML config file.
    fn toml_key(&mut self, path: &Path, keys: &[&str], entry: toml::Value) -> Result<(), String> {
        let existing = read_opt(path)?;
        let (new, changed) = if self.uninstall {
            match existing.as_deref() {
                None => return Ok(()),
                Some(s) => toml_unset(s, keys)?,
            }
        } else {
            toml_set(existing.as_deref(), keys, entry)?
        };
        if !changed {
            self.lines.push(format!("  unchanged {}", path.display()));
            return Ok(());
        }
        self.write(path, &new, true)
    }

    /// Write `<root>/ventosa/SKILL.md` and the companion skills (or remove those folders on uninstall).
    fn skill(&mut self, root: &Path) -> Result<PathBuf, String> {
        let main = root.join("ventosa").join("SKILL.md");
        let all = [("ventosa", skill_text(self.ws.as_deref())), ("ventosa-subtitles", SUBTITLES_SKILL.to_string())];
        for (name, text) in all {
            let dir = root.join(name);
            let path = dir.join("SKILL.md");
            if self.uninstall {
                if dir.exists() {
                    if !self.dry {
                        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    }
                    self.lines.push(format!("  removed {}", dir.display()));
                }
                continue;
            }
            let existing = read_opt(&path)?;
            if existing.as_deref() == Some(text.as_str()) {
                self.lines.push(format!("  unchanged {}", path.display()));
            } else {
                self.write(&path, &text, false)?;
            }
        }
        Ok(main)
    }

    fn remove_file(&mut self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        if !self.dry {
            fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        self.lines.push(format!("  removed {}", path.display()));
        Ok(())
    }
}

fn claude(c: &mut Ctx) -> Result<Vec<String>, String> {
    let mut out = vec![];
    let skills_root = match c.project.clone() {
        Some(p) => {
            let f = p.join(".mcp.json");
            c.json_key(&f, &["mcpServers", "ventosa"], stdio_entry(&c.bin, c.ws.as_deref()))?;
            out.push(format!("MCP: {} (project)", f.display()));
            p.join(".claude/skills")
        }
        None => {
            if c.cli {
                claude_cli(c)?;
                out.push("MCP: `claude mcp` user scope".into());
            } else {
                let f = c.home.join(".claude.json");
                c.json_key(&f, &["mcpServers", "ventosa"], stdio_entry(&c.bin, c.ws.as_deref()))?;
                out.push(format!("MCP: {} (mcpServers)", f.display()));
            }
            c.home.join(".claude/skills")
        }
    };
    let skill = c.skill(&skills_root)?;
    out.push(format!("skill: {}", skill.display()));
    Ok(out)
}

fn claude_cli(c: &mut Ctx) -> Result<(), String> {
    // Remove first so re-runs replace the entry instead of failing; a missing entry is fine.
    let _ = Command::new("claude").args(["mcp", "remove", "--scope", "user", "ventosa"]).output();
    if c.uninstall {
        c.lines.push("  claude mcp remove --scope user ventosa".into());
        return Ok(());
    }
    let mut args: Vec<String> = vec!["mcp".into(), "add".into(), "--scope".into(), "user".into()];
    if let Some(w) = &c.ws {
        args.push("-e".into());
        args.push(format!("VENTOSA_WORKSPACE={w}"));
    }
    args.extend(["ventosa".into(), "--".into(), c.bin.display().to_string()]);
    let out = Command::new("claude").args(&args).output().map_err(|e| format!("cannot run claude: {e}"))?;
    if !out.status.success() {
        return Err(format!("claude mcp add failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    c.lines.push("  claude mcp add --scope user ventosa".into());
    Ok(())
}

fn opencode(c: &mut Ctx) -> Result<Vec<String>, String> {
    let (cfg, skills_root) = match c.project.clone() {
        Some(p) => (p.join("opencode.json"), p.join(".opencode/skills")),
        None => {
            let d = c.home.join(".config/opencode");
            (d.join("opencode.json"), d.join("skills"))
        }
    };
    c.json_key(&cfg, &["mcp", "ventosa"], opencode_entry(&c.bin, c.ws.as_deref()))?;
    let skill = c.skill(&skills_root)?;
    Ok(vec![format!("MCP: {} (mcp.ventosa)", cfg.display()), format!("skill: {}", skill.display())])
}

fn codex(c: &mut Ctx) -> Result<Vec<String>, String> {
    let base = c.project.clone().map(|p| p.join(".codex")).unwrap_or_else(|| c.home.join(".codex"));
    let cfg = base.join("config.toml");
    c.toml_key(&cfg, &["mcp_servers", "ventosa"], codex_entry(&c.bin, c.ws.as_deref()))?;
    let skill = c.skill(&base.join("skills"))?;
    Ok(vec![format!("MCP: {} ([mcp_servers.ventosa])", cfg.display()), format!("skill: {}", skill.display())])
}

fn generic(c: &mut Ctx, client: &str) -> Result<Vec<String>, String> {
    let dir = c.data.join("clients");
    let snippet_path = dir.join(format!("{client}.json"));
    let skills_root = dir.join("skills");
    if c.uninstall {
        c.remove_file(&snippet_path)?;
        c.skill(&skills_root)?;
        return Ok(vec![]);
    }
    let body = to_json(&json!({"mcpServers": {"ventosa": stdio_entry(&c.bin, c.ws.as_deref())}}));
    if read_opt(&snippet_path)?.as_deref() == Some(body.as_str()) {
        c.lines.push(format!("  unchanged {}", snippet_path.display()));
    } else {
        c.write(&snippet_path, &body, false)?;
    }
    let skill = c.skill(&skills_root)?;
    Ok(vec![
        format!(
            "format unverified: paste {} into the {client} MCP servers config (command {}), then restart {client}",
            snippet_path.display(),
            c.bin.display()
        ),
        format!("skill: {}", skill.display()),
    ])
}

fn install_md(rows: &[(String, Vec<String>)], bin: &Path, opts: &Opts) -> String {
    let scope = match &opts.project {
        Some(p) => format!("project {}", p.display()),
        None => "user".into(),
    };
    let ws = opts.workspace.as_deref().unwrap_or("none (agents call workspaces_list)");
    let mut s = String::from("# Ventosa install\n\nWritten by `ventosa-install`. Restart each client after an install.\n\n");
    s += &format!("- MCP server: `{}`\n- Scope: {scope}\n- Default workspace: {ws}\n\n", bin.display());
    s += "## Clients\n\n";
    for (client, details) in rows {
        s += &format!("- **{client}**\n");
        for d in details {
            s += &format!("  - {d}\n");
        }
    }
    let mut flags = String::new();
    if let Some(p) = &opts.project {
        flags += &format!(" --project {}", p.display());
    }
    if let Some(w) = &opts.workspace {
        flags += &format!(" --workspace {w}");
    }
    s += &format!(
        "\n## Commands\n\n```\nventosa-install --print                         # snippets and this file\nventosa-install --client NAME{flags}   # re-run (idempotent)\nventosa-install --uninstall --client NAME{flags}\n```\n\n{}\n",
        mcp_tools()
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ventosa-install-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn json_set_is_idempotent_and_keeps_other_keys() {
        let src = r#"{"mcpServers":{"other":{"command":"x"}},"theme":"dark"}"#;
        let entry = json!({"command": "/bin/vm"});
        let (out, changed) = json_set(Some(src), &["mcpServers", "ventosa"], entry.clone()).unwrap();
        assert!(changed);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["ventosa"], entry);
        let (out2, changed2) = json_set(Some(&out), &["mcpServers", "ventosa"], entry).unwrap();
        assert!(!changed2);
        assert_eq!(out2, out);
    }

    #[test]
    fn json_set_creates_missing_file() {
        let (out, changed) = json_set(None, &["mcp", "ventosa"], json!({"enabled": true})).unwrap();
        assert!(changed);
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["mcp"]["ventosa"]["enabled"], true);
    }

    #[test]
    fn json_unset_removes_only_the_ventosa_key() {
        let src = r#"{"mcp":{"ventosa":{"x":1},"other":{"y":2}}}"#;
        let (out, changed) = json_unset(src, &["mcp", "ventosa"]).unwrap();
        assert!(changed);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["mcp"].get("ventosa").is_none());
        assert_eq!(v["mcp"]["other"]["y"], 2);
        let (_, again) = json_unset(&out, &["mcp", "ventosa"]).unwrap();
        assert!(!again);
    }

    #[test]
    fn toml_set_keeps_tables_and_is_idempotent() {
        let src = "model = \"gpt\"\n\n[profiles.fast]\nx = 1\n\n[mcp_servers.other]\ncommand = \"y\"\n";
        let entry = codex_entry(Path::new("/bin/vm"), Some("metalvj"));
        let (out, changed) = toml_set(Some(src), &["mcp_servers", "ventosa"], entry.clone()).unwrap();
        assert!(changed);
        let v: toml::Table = toml::from_str(&out).unwrap();
        assert_eq!(v["model"].as_str(), Some("gpt"));
        assert_eq!(v["profiles"]["fast"]["x"].as_integer(), Some(1));
        assert_eq!(v["mcp_servers"]["other"]["command"].as_str(), Some("y"));
        assert_eq!(v["mcp_servers"]["ventosa"]["command"].as_str(), Some("/bin/vm"));
        assert_eq!(v["mcp_servers"]["ventosa"]["env"]["VENTOSA_WORKSPACE"].as_str(), Some("metalvj"));
        let (out2, changed2) = toml_set(Some(&out), &["mcp_servers", "ventosa"], entry).unwrap();
        assert!(!changed2);
        assert_eq!(out2, out);
        let (out3, changed3) = toml_unset(&out, &["mcp_servers", "ventosa"]).unwrap();
        assert!(changed3);
        assert!(!out3.contains("/bin/vm"));
        assert!(out3.contains("mcp_servers"));
    }

    #[test]
    fn skill_text_substitutes_workspace() {
        assert!(skill_text(Some("metalvj")).contains("`metalvj`"));
        assert!(!skill_text(None).contains(WORKSPACE_MARK));
        assert!(skill_text(None).contains("none set"));
    }

    #[test]
    fn parse_args_handles_clients_and_rejects_unknown() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let o = parse_args(&a(&["--client", "claude,opencode", "--dry-run"])).unwrap().unwrap();
        assert_eq!(o.clients, vec!["claude", "opencode"]);
        assert!(o.dry_run);
        assert_eq!(parse_args(&a(&["--client", "all"])).unwrap().unwrap().clients.len(), 6);
        assert!(parse_args(&a(&["--client", "emacs"])).is_err());
        assert!(parse_args(&a(&["--bogus"])).is_err());
        assert!(parse_args(&a(&["--help"])).unwrap().is_none());
    }

    #[test]
    fn full_run_is_idempotent_backs_up_once_and_preserves_keys() {
        let home = tmp("home");
        let data = home.join(".local/share/ventosa");
        fs::create_dir_all(home.join(".config/opencode")).unwrap();
        let oc = home.join(".config/opencode/opencode.json");
        let original = r#"{"model":"x","mcp":{"other":{"type":"local","command":["foo"]}}}"#;
        fs::write(&oc, original).unwrap();
        fs::write(home.join(".claude.json"), r#"{"numStartups":3}"#).unwrap();
        let opts = Opts {
            clients: vec!["claude".into(), "opencode".into(), "codex".into(), "hermes".into()],
            workspace: Some("metalvj".into()),
            mcp_bin: Some(PathBuf::from("/opt/ventosa/ventosa-mcp")),
            home: Some(home.clone()),
            data_dir: Some(data.clone()),
            ..Default::default()
        };
        run(&opts).unwrap();
        let first_oc = fs::read_to_string(&oc).unwrap();
        let first_claude = fs::read_to_string(home.join(".claude.json")).unwrap();
        let v: Value = serde_json::from_str(&first_oc).unwrap();
        assert_eq!(v["model"], "x");
        assert_eq!(v["mcp"]["other"]["command"][0], "foo");
        assert_eq!(v["mcp"]["ventosa"]["command"][0], "/opt/ventosa/ventosa-mcp");
        assert_eq!(v["mcp"]["ventosa"]["environment"]["VENTOSA_WORKSPACE"], "metalvj");
        assert_eq!(fs::read_to_string(with_suffix(&oc, ".bak")).unwrap(), original);
        let c: Value = serde_json::from_str(&first_claude).unwrap();
        assert_eq!(c["numStartups"], 3);
        assert_eq!(c["mcpServers"]["ventosa"]["command"], "/opt/ventosa/ventosa-mcp");
        assert!(home.join(".claude/skills/ventosa/SKILL.md").is_file());
        assert!(home.join(".codex/config.toml").is_file());
        assert!(data.join("clients/hermes.json").is_file());
        assert!(data.join("INSTALL.md").is_file());

        run(&opts).unwrap();
        assert_eq!(fs::read_to_string(&oc).unwrap(), first_oc);
        assert_eq!(fs::read_to_string(home.join(".claude.json")).unwrap(), first_claude);
        assert_eq!(fs::read_to_string(with_suffix(&oc, ".bak")).unwrap(), original);

        let un = Opts { uninstall: true, ..opts.clone() };
        run(&un).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&oc).unwrap()).unwrap();
        assert!(v["mcp"].get("ventosa").is_none());
        assert_eq!(v["mcp"]["other"]["command"][0], "foo");
        assert!(!home.join(".claude/skills/ventosa").exists());
        let _ = fs::remove_dir_all(&home);
    }
}
