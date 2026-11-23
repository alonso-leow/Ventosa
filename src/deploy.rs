//! Publish an app to a server over SSH, so a page made for the human (a form,
//! a recipe callback, a gallery) is reachable from the internet.
//!
//! - `static`: copy `apps/<id>/` (plus Ventosa's `common/` scripts) to
//!   `<remote_dir>/<id>/`; the server's web root serves it at `<public_url>/<id>/`.
//! - `service`: also copy `serve_app.py` and start it on the server
//!   (`nohup python3 serve_app.py <dir> <port>`), so `ventosa.done()` and other
//!   POSTs work. Results are written on the server in `<remote_dir>/<id>/events.jsonl`;
//!   `pull` brings them back into the workspace's `apps/<id>/remote-events.jsonl`.
//!
//! Uses the system `ssh` and `scp` (keys from `key_path`, the agent or ~/.ssh/config).

use crate::apps;
use crate::config::{Server, Workspace};
use crate::graph;
use std::path::Path;
use std::process::Command;

fn target(s: &Server) -> String {
    if s.user.is_empty() { s.host.clone() } else { format!("{}@{}", s.user, s.host) }
}

fn base_args(s: &Server, scp: bool) -> Vec<String> {
    let mut a = vec!["-o".into(), "BatchMode=yes".into(), "-o".into(), "ConnectTimeout=15".into()];
    if s.port != 0 && s.port != 22 {
        a.push(if scp { "-P".into() } else { "-p".into() });
        a.push(s.port.to_string());
    }
    if !s.key_path.trim().is_empty() {
        a.push("-i".into());
        a.push(expand(&s.key_path));
    }
    a
}

fn expand(p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest).display().to_string(),
        None => p.to_string(),
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn ssh(s: &Server, remote_cmd: &str) -> Result<String, String> {
    let out = Command::new("ssh").args(base_args(s, false)).arg(target(s)).arg(remote_cmd).output().map_err(|e| format!("ssh: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() { Ok(text.trim().to_string()) } else { Err(format!("ssh failed ({}): {}", out.status, text.trim())) }
}

fn scp(s: &Server, from: &Path, to_remote: &str) -> Result<(), String> {
    let out = Command::new("scp")
        .args(base_args(s, true))
        .arg("-r")
        .arg(from)
        .arg(format!("{}:{}", target(s), to_remote))
        .output()
        .map_err(|e| format!("scp: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("scp failed: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

/// Check the server answers and the remote folder is writable.
pub fn test(s: &Server) -> Result<String, String> {
    let dir = s.remote_dir.trim_end_matches('/');
    ssh(s, &format!("mkdir -p {d} && test -w {d} && echo ok: $(hostname) {d}", d = quote(dir)))
}

/// Publish an app. Returns the public URL.
pub fn publish(ws: &Workspace, s: &Server, id: &str, mode: &str) -> Result<String, String> {
    if !s.enabled {
        return Err(format!("server {} is disabled in settings ▸ servers", s.name));
    }
    if s.host.trim().is_empty() || s.remote_dir.trim().is_empty() {
        return Err(format!("server {} needs host and remote_dir", s.name));
    }
    let app = apps::get(ws, id).ok_or_else(|| format!("no app {id}"))?;
    let local = graph::root(ws).join("apps").join(id);
    let dir = format!("{}/{id}", s.remote_dir.trim_end_matches('/'));
    ssh(s, &format!("mkdir -p {}", quote(&dir)))?;
    // the app's files, then Ventosa's shared page scripts next to them
    for e in std::fs::read_dir(&local).map_err(|e| e.to_string())?.flatten() {
        scp(s, &e.path(), &format!("{dir}/"))?;
    }
    let common = graph::runtime_dir().join("apps/web/common");
    if common.is_dir() {
        scp(s, &common, &format!("{dir}/"))?;
    }
    let url = if s.public_url.trim().is_empty() {
        format!("http://{}:{}/", s.host, app.port)
    } else {
        format!("{}/{id}/", s.public_url.trim_end_matches('/'))
    };
    if mode == "service" {
        let server = graph::runtime_dir().join("engine/serve_app.py");
        scp(s, &server, &format!("{dir}/"))?;
        let start = format!(
            "cd {d} && (pkill -f {pat} || true) && VENTOSA_REMOTE=1 nohup python3 serve_app.py . {port} > serve.log 2>&1 & sleep 1; echo started",
            d = quote(&dir),
            pat = quote(&format!("serve_app.py . {}", app.port)),
            port = app.port
        );
        ssh(s, &start)?;
    }
    Ok(url)
}

/// Bring the server's events for an app back into the workspace.
pub fn pull(ws: &Workspace, s: &Server, id: &str) -> Result<String, String> {
    let dir = format!("{}/{id}", s.remote_dir.trim_end_matches('/'));
    let text = ssh(s, &format!("cat {}/events.jsonl 2>/dev/null || true", quote(&dir)))?;
    let dest = graph::root(ws).join("apps").join(id).join("remote-events.jsonl");
    std::fs::write(&dest, &text).map_err(|e| e.to_string())?;
    Ok(format!("{} line(s) → {}", text.lines().count(), dest.display()))
}

/// Stop a published service.
pub fn unpublish(s: &Server, id: &str, port: u16) -> Result<String, String> {
    let _ = id;
    ssh(s, &format!("pkill -f {} || true; echo stopped", quote(&format!("serve_app.py . {port}"))))
}

// ---------------------------------------------------------------- live backend

fn rsync(s: &Server, args: &[&str], from: &str, to: &str) -> Result<(), String> {
    let mut ssh_cmd = String::from("ssh -o BatchMode=yes -o ConnectTimeout=15");
    if s.port != 0 && s.port != 22 {
        ssh_cmd.push_str(&format!(" -p {}", s.port));
    }
    if !s.key_path.trim().is_empty() {
        ssh_cmd.push_str(&format!(" -i {}", quote(&expand(&s.key_path))));
    }
    let out = Command::new("rsync").args(["-az", "-e", &ssh_cmd]).args(args).arg(from).arg(to).output().map_err(|e| format!("rsync: {e} (install rsync on both ends)"))?;
    if out.status.success() { Ok(()) } else { Err(format!("rsync failed: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

fn live_base(ws: &Workspace, s: &Server) -> String {
    format!("{}/live/{}", s.remote_dir.trim_end_matches('/'), ws.name)
}

/// Replicate the workspace and the engine to the server and start the same apps there
/// (labelling, teleprompter, media-labelling, dashboard), so the work goes on with the admin
/// end closed. Refuses while there are no users: the apps bind to the network and must ask for a login.
pub fn go_live(ws: &Workspace, s: &Server) -> Result<String, String> {
    if !s.enabled || s.host.trim().is_empty() || s.remote_dir.trim().is_empty() {
        return Err(format!("server {} needs host, remote dir and AI may use on", s.name));
    }
    let id = crate::identity::load();
    if id.open_mode() {
        return Err("add at least one user in Access first: the live apps listen on the network and ask for a login".into());
    }
    let base = live_base(ws, s);
    ssh(s, &format!("mkdir -p {b}/ws {b}/runtime", b = quote(&base)))?;
    let root = format!("{}/", graph::root(ws).display());
    rsync(s, &["--delete", "--exclude=.git", "--exclude=node_modules", "--exclude=assets/camera", "--exclude=assets/inbox", "--exclude=hyperframes/renders", "--exclude=pipeline/attached", "--exclude=pipeline/presence"], &root, &format!("{}:{base}/ws/", target(s)))?;
    let rt = graph::runtime_dir();
    for d in ["engine", "apps"] {
        rsync(s, &["--delete", "--exclude=__pycache__"], &format!("{}/{d}", rt.display()), &format!("{}:{base}/runtime/", target(s)))?;
    }
    rsync(s, &[], &crate::identity::path().display().to_string(), &format!("{}:{base}/identity.json", target(s)))?;
    let script = format!(
        "#!/bin/sh\nB={b}\nexport VENTOSA_RUNTIME=$B/runtime VENTOSA_WORKSPACE=$B/ws VENTOSA_IDENTITY=$B/identity.json VENTOSA_BIND=0.0.0.0\ncd $B/ws\nfor s in serve_label serve_record serve_media serve_dash; do\n  pkill -f \"$B/runtime/engine/$s.py\" 2>/dev/null\n  nohup python3 $B/runtime/engine/$s.py > $B/$s.log 2>&1 &\ndone\nsleep 1; echo started\n",
        b = quote(&base)
    );
    ssh(s, &format!("cat > {b}/run.sh <<'EOS'\n{script}EOS\nchmod +x {b}/run.sh && {b}/run.sh", b = quote(&base)))?;
    let ports = graph::workspace_json(ws);
    let p = |k: &str, d: u64| ports["ports"][k].as_u64().unwrap_or(d);
    Ok(format!("live on {}: labelling :{} · teleprompter :{} · media-labelling :{}", s.host, p("labelling", 8766), p("teleprompter", 8765), p("media-labelling", 8771)))
}

/// Push the graph (steps, skills, knowledge) up and pull what the live end collected (events,
/// labels, clips) down; the newer file wins.
pub fn sync_live(ws: &Workspace, s: &Server) -> Result<String, String> {
    let base = live_base(ws, s);
    let root = graph::root(ws);
    let t = target(s);
    for d in ["pipeline/skill-graph", "meta", "scripts", "research"] {
        if root.join(d).is_dir() {
            rsync(s, &["-u"], &format!("{}/{d}/", root.display()), &format!("{t}:{base}/ws/{d}/"))?;
        }
    }
    for f in ["pipeline/events.jsonl", "labelling/labels.json", "recording/clips.json"] {
        let _ = rsync(s, &["-u"], &format!("{t}:{base}/ws/{f}"), &format!("{}/{f}", root.display()));
    }
    Ok(format!("synced with {}", s.host))
}

pub fn stop_live(ws: &Workspace, s: &Server) -> Result<String, String> {
    let base = live_base(ws, s);
    ssh(s, &format!("pkill -f {} || true; echo stopped", quote(&format!("{base}/runtime/engine/serve_"))))
}

pub fn live_status(ws: &Workspace, s: &Server) -> Result<String, String> {
    let base = live_base(ws, s);
    ssh(s, &format!("pgrep -fc {} || true", quote(&format!("{base}/runtime/engine/serve_")))).map(|n| format!("{} app process(es) running on {}", n.lines().last().unwrap_or("0"), s.host))
}
