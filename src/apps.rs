//! Apps the AI builds: small web apps in the workspace (`apps/<id>/`), served by
//! `engine/serve_app.py` and wired to one graph node through `effect`.

use crate::config::{Config, Workspace};
use crate::graph;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const FIRST_PORT: u16 = 8790;
pub const EFFECTS: [&str; 3] = ["human-done", "step-done", "none"];
const TEMPLATE: &str = include_str!("../templates/app/index.html");

/// `apps/<id>/app.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppDef {
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Graph step this app serves ("" = none).
    #[serde(default)]
    pub node: String,
    /// What finishing in the app does to the graph: human-done | step-done | none.
    #[serde(default = "default_effect")]
    pub effect: String,
    /// One line telling the human what happens next.
    #[serde(default)]
    pub next: String,
    pub port: u16,
    #[serde(default)]
    pub created: u64,
}

fn default_effect() -> String {
    "none".into()
}

/// `[a-z0-9-]`, 1 to 40 characters.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn apps_dir(ws: &Workspace) -> PathBuf {
    graph::root(ws).join("apps")
}

pub fn app_dir(ws: &Workspace, id: &str) -> PathBuf {
    apps_dir(ws).join(id)
}

fn read_def(dir: &Path) -> Option<AppDef> {
    serde_json::from_str(&fs::read_to_string(dir.join("app.json")).ok()?).ok()
}

/// Every app in the workspace, sorted by id.
pub fn list(ws: &Workspace) -> Vec<AppDef> {
    let mut out: Vec<AppDef> = fs::read_dir(apps_dir(ws))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| read_def(&e.path()))
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Every app of every workspace in the config, with the pid of its running server.
pub fn all(cfg: &Config) -> Vec<(String, AppDef, Option<u32>)> {
    cfg.workspaces
        .iter()
        .flat_map(|ws| list(ws).into_iter().map(move |d| {
            let pid = running(ws, &d.id);
            (ws.name.clone(), d, pid)
        }))
        .collect()
}

pub fn get(ws: &Workspace, id: &str) -> Option<AppDef> {
    read_def(&app_dir(ws, id))
}

fn free_port(ws: &Workspace) -> Result<u16, String> {
    let used: Vec<u16> = list(ws).iter().map(|a| a.port).collect();
    (FIRST_PORT..FIRST_PORT + 500)
        .find(|p| !used.contains(p) && TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .ok_or_else(|| "no free app port".to_string())
}

/// Create `apps/<id>/` with app.json and index.html (the starter template when `html` is empty).
pub fn create(ws: &Workspace, id: &str, title: &str, node: &str, effect: &str, next: &str, html: &str) -> Result<AppDef, String> {
    if !valid_id(id) {
        return Err(format!("bad app id '{id}': use a-z, 0-9 and '-'"));
    }
    if !EFFECTS.contains(&effect) {
        return Err(format!("effect must be one of {}", EFFECTS.join(", ")));
    }
    if effect != "none" && node.is_empty() {
        return Err("an effect needs a node".into());
    }
    let dir = app_dir(ws, id);
    if dir.exists() {
        return Err(format!("app {id} exists"));
    }
    if !node.is_empty() {
        let g = graph::steps(ws).map_err(|e| format!("cannot check node: {e}"))?;
        let known = g["steps"].as_array().into_iter().flatten().any(|s| s["id"] == node);
        if !known {
            return Err(format!("no step '{node}' in steps.json"));
        }
    }
    let def = AppDef {
        id: id.into(),
        title: if title.trim().is_empty() { id.into() } else { title.trim().into() },
        node: node.into(),
        effect: effect.into(),
        next: next.trim().into(),
        port: free_port(ws)?,
        created: crate::now(),
    };
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(&def).map_err(|e| e.to_string())?;
    fs::write(dir.join("app.json"), body + "\n").map_err(|e| e.to_string())?;
    let page = if html.trim().is_empty() { TEMPLATE } else { html };
    fs::write(dir.join("index.html"), page).map_err(|e| e.to_string())?;
    Ok(def)
}

/// Write one file inside the app folder. No absolute paths, no `..`, no symlink escape.
pub fn update_file(ws: &Workspace, id: &str, rel_path: &str, text: &str) -> Result<(), String> {
    let dir = app_dir(ws, id);
    if !dir.join("app.json").is_file() {
        return Err(format!("no app {id}"));
    }
    let rel = Path::new(rel_path);
    if rel_path.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("path must be relative to the app folder, without '..'".into());
    }
    if rel == Path::new("app.json") {
        return Err("app.json is written by create, not update_file".into());
    }
    let target = dir.join(rel);
    let parent = target.parent().ok_or("bad path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let (real_parent, real_dir) = (fs::canonicalize(parent).map_err(|e| e.to_string())?, fs::canonicalize(&dir).map_err(|e| e.to_string())?);
    if !real_parent.starts_with(&real_dir) {
        return Err("path leaves the app folder".into());
    }
    fs::write(&target, text).map_err(|e| e.to_string())
}

fn pid_file(ws: &Workspace, id: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("ventosa-apps").join(format!("{}-{id}.pid", ws.name))
}

fn engine_script() -> Result<PathBuf, String> {
    let rt = graph::runtime_dir().join("engine").join("serve_app.py");
    if rt.is_file() {
        return Ok(rt);
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("engine").join("serve_app.py");
    if repo.is_file() {
        Ok(repo)
    } else {
        Err("serve_app.py not found (run the launcher)".into())
    }
}

/// Pid of the app's server if it is still alive (checked against /proc).
pub fn running(ws: &Workspace, id: &str) -> Option<u32> {
    let pid: u32 = fs::read_to_string(pid_file(ws, id)).ok()?.trim().parse().ok()?;
    let cmd = String::from_utf8_lossy(&fs::read(format!("/proc/{pid}/cmdline")).ok()?).to_string();
    (cmd.contains("serve_app.py") && cmd.contains(&app_dir(ws, id).display().to_string())).then_some(pid)
}

/// Start the app's server (once) and return its URL.
pub fn deploy(ws: &Workspace, id: &str) -> Result<String, String> {
    let app = get(ws, id).ok_or_else(|| format!("no app {id}"))?;
    if running(ws, id).is_some() {
        return Ok(url(&app));
    }
    let script = engine_script()?;
    let pf = pid_file(ws, id);
    fs::create_dir_all(pf.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let log = fs::File::create(pf.with_extension("log")).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    let root = graph::root(ws);
    let mut child = Command::new("python3")
        .arg(&script)
        .arg(app_dir(ws, id))
        .arg(app.port.to_string())
        .env("VENTOSA_WORKSPACE", &root)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|e| format!("cannot start server: {e}"))?;
    fs::write(&pf, child.id().to_string()).map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let until = Instant::now() + Duration::from_secs(3);
    while TcpStream::connect(("127.0.0.1", app.port)).is_err() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(url(&app))
}

/// Stop the app's server. Returns false when it was not running.
pub fn stop(ws: &Workspace, id: &str) -> Result<bool, String> {
    let Some(pid) = running(ws, id) else {
        let _ = fs::remove_file(pid_file(ws, id));
        return Ok(false);
    };
    let st = Command::new("kill").arg(pid.to_string()).status().map_err(|e| e.to_string())?;
    let _ = fs::remove_file(pid_file(ws, id));
    if st.success() {
        Ok(true)
    } else {
        Err(format!("kill {pid} failed"))
    }
}

pub fn url(app: &AppDef) -> String {
    format!("http://127.0.0.1:{}/", app.port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(tag: &str) -> Workspace {
        let root = std::env::temp_dir().join(format!("ventosa-apps-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("pipeline/skill-graph")).unwrap();
        fs::write(
            root.join("pipeline/skill-graph/steps.json"),
            r#"{"steps":[{"id":"label","owner":"human"},{"id":"script","owner":"ai"}]}"#,
        )
        .unwrap();
        Workspace { name: "t".into(), root: root.display().to_string(), engine: "ventosa".into(), enabled: true }
    }

    #[test]
    fn create_and_list_with_validation() {
        let ws = temp_ws("create");
        let a = create(&ws, "review-v", "Review", "label", "human-done", "Next: script", "").unwrap();
        assert!(a.port >= FIRST_PORT);
        assert_eq!(list(&ws).len(), 1);
        let html = fs::read_to_string(app_dir(&ws, "review-v").join("index.html")).unwrap();
        assert!(html.contains("/common/ventosa-app.js"));
        assert!(create(&ws, "review-v", "", "label", "none", "", "").is_err(), "duplicate");
        assert!(create(&ws, "Bad_ID", "", "", "none", "", "").is_err(), "bad id");
        assert!(create(&ws, "x1", "", "nope", "none", "", "").is_err(), "unknown node");
        assert!(create(&ws, "x2", "", "label", "bogus", "", "").is_err(), "bad effect");
        assert!(create(&ws, "x3", "", "", "human-done", "", "").is_err(), "effect without node");
        let _ = fs::remove_dir_all(graph::root(&ws));
    }

    #[test]
    fn update_file_stays_inside_the_app() {
        let ws = temp_ws("files");
        create(&ws, "p", "", "", "none", "", "<p>hi</p>").unwrap();
        update_file(&ws, "p", "js/app.js", "1").unwrap();
        assert_eq!(fs::read_to_string(app_dir(&ws, "p").join("js/app.js")).unwrap(), "1");
        for bad in ["../x.js", "/etc/x", "a/../../x", "", "app.json"] {
            assert!(update_file(&ws, "p", bad, "x").is_err(), "{bad} must be refused");
        }
        assert!(update_file(&ws, "missing", "a.js", "x").is_err());
        let _ = fs::remove_dir_all(graph::root(&ws));
    }
}
