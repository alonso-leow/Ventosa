//! Watch edited videos fast: hand a file to an external player with a
//! playback bar. Order: mpv, vlc, ffplay, xdg-open. `VENTOSA_PLAYER` overrides.

use crate::config::Workspace;
use crate::graph;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

const PLAYERS: [&str; 4] = ["mpv", "vlc", "ffplay", "xdg-open"];
const EXTS: [&str; 8] = ["mp4", "mov", "mkv", "webm", "weba", "wav", "m4a", "mp3"];

/// Players found in PATH, in preference order.
pub fn players() -> Vec<&'static str> {
    PLAYERS.into_iter().filter(|p| on_path(p)).collect()
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file())).unwrap_or(false)
}

/// Open `path` in an external player. Returns the player used.
pub fn play(path: &Path, title: &str, start: Option<f64>) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("no such file: {}", path.display()));
    }
    let file = path.display().to_string();
    let s = start.filter(|v| *v > 0.0).map(|v| format!("{v:.3}"));
    if let Ok(custom) = std::env::var("VENTOSA_PLAYER") {
        let mut parts = custom.split_whitespace();
        let bin = parts.next().ok_or("VENTOSA_PLAYER is empty")?;
        let mut args: Vec<String> = parts.map(String::from).collect();
        args.push(file);
        return spawn(bin, &args).map(|_| bin.to_string());
    }
    let mut last = String::from("no player found: install mpv, vlc, ffplay or xdg-open");
    for p in players() {
        match spawn(p, &args_for(p, &file, title, s.as_deref())) {
            Ok(()) => return Ok(p.to_string()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn args_for(p: &str, file: &str, title: &str, start: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = match p {
        "mpv" => vec!["--force-window=yes".into(), "--keep-open=yes".into(), "--osc=yes".into(), format!("--title={title}"), "--geometry=960x540".into()],
        "vlc" => vec!["--play-and-exit".into(), "--no-video-title-show".into(), format!("--meta-title={title}")],
        "ffplay" => vec!["-window_title".into(), title.into(), "-x".into(), "960".into(), "-y".into(), "540".into()],
        _ => vec![],
    };
    if let Some(s) = start {
        match p {
            "mpv" => a.push(format!("--start={s}")),
            "vlc" => a.push(format!("--start-time={s}")),
            "ffplay" => {
                a.push("-ss".into());
                a.push(s.into());
            }
            _ => {}
        }
    }
    a.push(file.into());
    a
}

fn spawn(bin: &str, args: &[String]) -> Result<(), String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{bin}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// What ffprobe says about a media file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub video: bool,
}

/// Duration and frame size via ffprobe (`None` when ffprobe is missing or the file is not media).
pub fn probe(path: &Path) -> Option<Info> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let video = v["streams"].as_array()?.iter().find(|s| s["codec_type"] == "video");
    Some(Info {
        duration: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0.0),
        width: video.and_then(|s| s["width"].as_u64()).unwrap_or(0) as u32,
        height: video.and_then(|s| s["height"].as_u64()).unwrap_or(0) as u32,
        video: video.is_some(),
    })
}

/// One frame of a video at `t` seconds as a JPEG in the thumbnail cache (made once per file, mtime and time).
pub fn frame(path: &Path, t: f64) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mtime = meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let key = format!("{}|{}|{}|{:.1}", path.display(), meta.len(), mtime, t);
    let hash = key.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ventosa").join("thumbs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(format!("{hash:016x}.jpg"));
    if out.is_file() {
        return Ok(out);
    }
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-ss", &format!("{t:.2}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", "scale='min(960,iw)':-2", "-q:v", "4"])
        .arg(&out)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    if status.success() && out.is_file() { Ok(out) } else { Err("ffmpeg could not read a frame".into()) }
}

/// One playable file: an edit, a camera take or a render.
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub subject: String,
    pub kind: &'static str,
    pub path: PathBuf,
    pub modified: u64,
    pub size: u64,
}

/// Media under `assets/edits`, `assets/camera` and `renders`, newest first.
pub fn media(ws: &Workspace) -> Vec<Media> {
    let root = graph::root(ws);
    let mut out = vec![];
    for (dir, kind) in [("assets/edits", "edit"), ("assets/camera", "take"), ("renders", "render")] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if !path.is_file() || !is_media(&path) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            let subject = name.split(|c| matches!(c, '-' | '_' | '.')).next().unwrap_or("").to_string();
            if subject.is_empty() {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            out.push(Media { subject, kind, path, modified, size: meta.len() });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    out
}

/// The media of one subject (`V03`, `E01` …), newest first.
pub fn media_for(ws: &Workspace, subject: &str) -> Vec<Media> {
    media(ws).into_iter().filter(|m| m.subject == subject).collect()
}

fn is_media(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| {
        let l = e.to_ascii_lowercase();
        EXTS.contains(&l.as_str())
    }).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_scans_edits_takes_and_renders() {
        let root = std::env::temp_dir().join(format!("ventosa-preview-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["assets/edits", "assets/camera", "renders"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["assets/edits/V03-final.mp4", "assets/camera/V03_take1.MOV", "assets/camera/V02.webm", "renders/E01.mp3", "assets/edits/V03-notes.txt"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let ws = Workspace { name: "t".into(), root: root.display().to_string(), engine: "ventosa".into(), enabled: true };
        let all = media(&ws);
        assert_eq!(all.len(), 4);
        assert!(all.iter().all(|m| m.size == 1));
        assert!(all.iter().any(|m| m.subject == "E01" && m.kind == "render"));
        let v03 = media_for(&ws, "V03");
        assert_eq!(v03.len(), 2);
        assert!(v03.iter().any(|m| m.kind == "edit") && v03.iter().any(|m| m.kind == "take"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
