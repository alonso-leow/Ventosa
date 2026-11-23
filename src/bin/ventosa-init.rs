//! Create a workspace from a template and register it.
//!   ventosa-init <media|podcast> <dir> [--name N] [--brand "Name"] [--accent #rrggbb]
//!                [--tagline T] [--skills-dir ~/.claude/skills]
//!   ventosa-init --register <dir> [--name N] [--local]   register an existing workspace
//! Writes <dir>/SKILL.md (the graph skill for agents that use skill folders) and,
//! with --skills-dir, installs it as <skills-dir>/<name>-graph/SKILL.md.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use ventosa::config::{self, Workspace};
use ventosa::graph::runtime_dir;

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &dst)?;
        } else if !dst.exists() {
            std::fs::copy(e.path(), dst)?;
        }
    }
    Ok(())
}

fn skill_text(name: &str, root: &Path) -> String {
    format!(
        "---\nname: {name}-graph\ndescription: {name} content pipeline as a skill graph of step nodes in {root}. Use to advance the pipeline, see what is pending, or when given a title to turn into a script. Attaching to one step node is obligatory before any work.\n---\n\n\
# {name} skill graph\n\n\
The pipeline is a graph of step nodes (`pipeline/skill-graph/steps.json`); each node has its own skill in `pipeline/skill-graph/nodes/`.\n\
Work through Ventosa: MCP tools `queue_claim`, `graph_node`, `workspace_status`, `pipeline_intake`, or the CLI below.\n\n\
## Loop\n\
1. `ventosa-graph -w {root} status --ai` — subjects waiting on AI steps (or MCP `queue_claim` with node + agent name).\n\
2. `VENTOSA_AGENT=<you> ventosa-graph -w {root} attach <step> <ID>` — prints the node skill, knowledge to read and the only write command. Obligatory.\n\
3. Do the node skill; write only with `ventosa-graph -w {root} write <command> <ID> …`. The node detaches itself when done.\n\
4. Human steps (label, record, publish, upload) can't be attached: point the human to `ventosa-labelling -w {root}` / `ventosa-teleprompter -w {root}`.\n\n\
A new title: MCP `pipeline_intake` with `workspace: \"{name}\"`, or `ventosa-graph -w {root} new <ID> \"Title\"`, then attach to `options`.\n",
        root = root.display()
    )
}

fn register(name: &str, root: &Path, engine: &str) -> Result<(), String> {
    let path = config::default_path();
    let mut cfg = config::load(&path);
    cfg.workspaces.retain(|w| w.name != name);
    cfg.workspaces.push(Workspace { name: name.into(), root: root.display().to_string(), engine: engine.into(), enabled: true });
    config::save(&path, &cfg).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || {
        eprintln!("usage: ventosa-init <media|podcast> <dir> [--name N] [--brand Name] [--accent #hex] [--tagline T] [--skills-dir DIR]\n       ventosa-init --register <dir> [--name N] [--local]");
        ExitCode::FAILURE
    };
    if args.first().map(String::as_str) == Some("--register") {
        let Some(dir) = args.get(1) else { return usage() };
        let root = std::fs::canonicalize(dir).unwrap_or_else(|_| PathBuf::from(dir));
        let name = arg(&args, "--name").unwrap_or_else(|| root.file_name().unwrap_or_default().to_string_lossy().into());
        let engine = if args.iter().any(|a| a == "--local") { "local" } else { "ventosa" };
        return match register(&name, &root, engine) {
            Ok(()) => {
                println!("registered workspace {name} ({engine} engine) at {}", root.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let (Some(template), Some(dir)) = (args.first(), args.get(1)) else { return usage() };
    let src = runtime_dir().join("templates").join(template);
    if !src.is_dir() {
        eprintln!("no template {template} in {}", src.parent().unwrap().display());
        return ExitCode::FAILURE;
    }
    let root = PathBuf::from(dir);
    if let Err(e) = copy_dir(&src, &root) {
        eprintln!("copy failed: {e}");
        return ExitCode::FAILURE;
    }
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let name = arg(&args, "--name").unwrap_or_else(|| root.file_name().unwrap_or_default().to_string_lossy().into());
    let wj = root.join("workspace.json");
    let mut ws: Value = std::fs::read_to_string(&wj).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    ws["name"] = name.clone().into();
    if let Some(b) = arg(&args, "--brand") {
        ws["brand"]["name"] = b.into();
    }
    if let Some(a) = arg(&args, "--accent") {
        ws["brand"]["accent"] = a.into();
    }
    if let Some(t) = arg(&args, "--tagline") {
        ws["brand"]["tagline"] = t.into();
    }
    let _ = std::fs::write(&wj, serde_json::to_string_pretty(&ws).unwrap_or_default() + "\n");
    let skill = skill_text(&name, &root);
    let _ = std::fs::write(root.join("SKILL.md"), &skill);
    if let Some(sd) = arg(&args, "--skills-dir") {
        let d = PathBuf::from(sd.replacen('~', &dirs::home_dir().unwrap_or_default().display().to_string(), 1)).join(format!("{name}-graph"));
        if std::fs::create_dir_all(&d).and_then(|_| std::fs::write(d.join("SKILL.md"), &skill)).is_ok() {
            println!("skill installed: {}", d.join("SKILL.md").display());
        }
    }
    match register(&name, &root, "ventosa") {
        Ok(()) => {
            println!("workspace {name} created at {} and registered", root.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("created, but could not register: {e}");
            ExitCode::FAILURE
        }
    }
}
