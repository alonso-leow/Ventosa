//! Recommended tools for AI video production: what the AI needs to make good
//! videos, how Ventosa checks each one is present, and how to get it.
//! `critical` tools raise a caution popup in the app while missing.

use crate::config::Config;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Rec {
    pub id: &'static str,
    pub name: &'static str,
    /// `app`, `mcp`, `cli` or `service`.
    pub kind: &'static str,
    pub what: &'static str,
    /// Without it the AI cannot finish a video well.
    pub critical: bool,
    pub link: &'static str,
    /// Shell command that installs or wires it (empty = manual, see link).
    pub install: &'static str,
}

const fn rec(id: &'static str, name: &'static str, kind: &'static str, what: &'static str, critical: bool, link: &'static str, install: &'static str) -> Rec {
    Rec { id, name, kind, what, critical, link, install }
}

pub static RECS: &[Rec] = &[
    rec("provider", "AI provider", "service", "The model the built-in AI, batches and schedules run on. Ventosa is AI first: nothing runs without it.", true, "https://console.anthropic.com/", ""),
    rec("ffmpeg", "ffmpeg", "cli", "Cuts, encodes, transcodes and probes every edit, take and render.", true, "https://ffmpeg.org/download.html", "sudo pacman -S --needed ffmpeg"),
    rec("hyperframes", "HyperFrames", "cli", "HTML → video: animated scenes, lower thirds and explainers rendered from code the AI writes.", true, "https://github.com/heygen-com/hyperframes", "npm i -g --prefix ~/.local hyperframes"),
    rec("metalvj", "MetalVJ", "app", "Audio-reactive visuals: gen nodes, stage and reel renders (Ventosa's own simple visualizer needs only ffmpeg).", false, "https://github.com/alonso-leow/MetalVJ", ""),
    rec("metal-mcp", "MetalVJ MCP", "mcp", "MetalVJ's tool catalog for AI clients: scenes, gen nodes, renders, reels.", false, "https://github.com/alonso-leow/MetalVJ", "claude mcp add metalvj -- ~/.cache/metalvj/target/release/metal-mcp"),
    rec("remotion", "Remotion", "cli", "React to video: typed, data-driven motion scenes and many variants of one template (HyperFrames is the default engine). Check its licence terms for companies.", false, "https://www.remotion.dev/", "npm i -g --prefix ~/.local @remotion/cli"),
    rec("filmcraft", "filmcraft", "mcp", "Suite app (Apache-2.0, local, no cost): nonlinear video editor: cut, trim, grade, mix, export (650+ commands). Headless MCP server over stdio: `filmcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/filmcraft", "claude mcp add filmcraft -- filmcraft-cli mcp"),
    rec("effectcraft", "effectcraft", "mcp", "Suite app (Apache-2.0, local, no cost): motion graphics and compositing: comps, keyframes, headless render to video. Headless MCP server over stdio: `effectcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/effectcraft", "claude mcp add effectcraft -- effectcraft-cli mcp"),
    rec("anicraft", "anicraft", "mcp", "Suite app (Apache-2.0, local, no cost): 2D animation editor: draw, animate, export GIF/video. Headless MCP server over stdio: `anicraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/anicraft", "claude mcp add anicraft -- anicraft-cli mcp"),
    rec("vectorcraft", "vectorcraft", "mcp", "Suite app (Apache-2.0, local, no cost): vector art: titles, logos, shapes, export SVG/PDF/PNG. Headless MCP server over stdio: `vectorcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/vectorcraft", "claude mcp add vectorcraft -- vectorcraft-cli mcp"),
    rec("photocraft", "photocraft", "mcp", "Suite app (Apache-2.0, local, no cost): photo editor: grades, covers, batch actions. Headless MCP server over stdio: `photocraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/photocraft", "claude mcp add photocraft -- photocraft-cli mcp"),
    rec("designcraft", "designcraft", "mcp", "Suite app (Apache-2.0, local, no cost): page and layout design: thumbnails, covers, multi-page render to PNG. Headless MCP server over stdio: `designcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/designcraft", "claude mcp add designcraft -- designcraft-cli mcp"),
    rec("figcraft", "figcraft", "mcp", "Suite app (Apache-2.0, local, no cost): interface design and prototyping: screen mockups and UI inserts, render to PNG. Headless MCP server over stdio: `figcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/figcraft", "claude mcp add figcraft -- figcraft-cli mcp"),
    rec("deckcraft", "deckcraft", "mcp", "Suite app (Apache-2.0, local, no cost): presentations: slide beats for screen shots, export. Headless MCP server over stdio: `deckcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/deckcraft", "claude mcp add deckcraft -- deckcraft-cli mcp"),
    rec("texturecraft", "texturecraft", "mcp", "Suite app (Apache-2.0, local, no cost): 3D texture painting and baking, headless render (for 3D scenes). Headless MCP server over stdio: `texturecraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/texturecraft", "claude mcp add texturecraft -- texturecraft-cli mcp"),
    rec("materialcraft", "materialcraft", "mcp", "Suite app (Apache-2.0, local, no cost): node-based material design, headless render (for 3D scenes). Headless MCP server over stdio: `materialcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/materialcraft", "claude mcp add materialcraft -- materialcraft-cli mcp"),
    rec("soundcraft", "soundcraft", "mcp", "Suite app (Apache-2.0, local, no cost): digital audio workstation: mix, bounce, scripted audio. Headless MCP server over stdio: `soundcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/soundcraft", "claude mcp add soundcraft -- soundcraft-cli mcp"),
    rec("cadcraft", "cadcraft", "mcp", "Suite app (Apache-2.0, local, no cost): CAD and drafting: technical drawings, floor plans, DXF/PNG export (diagram inserts). Headless MCP server over stdio: `cadcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/cadcraft", "claude mcp add cadcraft -- cadcraft-cli mcp"),
    rec("gridcraft", "gridcraft", "mcp", "Suite app (Apache-2.0, local, no cost): spreadsheet: data tables behind charts and numbers on screen. Headless MCP server over stdio: `gridcraft-cli mcp`; also a CLI and a JSON control channel.", false, "https://github.com/storytold/gridcraft", "claude mcp add gridcraft -- gridcraft-cli mcp"),
    rec("blender-mcp", "Blender + MCP", "mcp", "3D scenes and renders driven by the AI through Blender.", false, "https://github.com/ahujasid/blender-mcp", "claude mcp add blender -- uvx blender-mcp"),
    rec("mpv", "mpv", "cli", "Fast preview of edits with a playback bar.", false, "https://mpv.io/installation/", "sudo pacman -S --needed mpv"),
    rec("gcloud", "Google Cloud CLI", "cli", "gcloud: Google Cloud projects, APIs, service accounts and keys (YouTube Data API, Vertex AI, Cloud Storage for renders).", false, "https://cloud.google.com/sdk/docs/install", "yay -S --needed google-cloud-cli || paru -S --needed google-cloud-cli"),
    rec("gws", "Google Workspace CLI", "cli", "gws: Drive, Docs, Sheets, Gmail and Calendar from the shell and for the AI (scripts to Docs, schedules to Calendar, briefs from Sheets).", false, "https://github.com/googleworkspace/cli", "npm i -g --prefix ~/.local @googleworkspace/cli"),
    rec("whisperx", "WhisperX", "cli", "Subtitles: speech to text with forced-aligned word timestamps, so cues land on the right frame (stable-ts, faster-whisper and whisper.cpp also work).", false, "https://github.com/m-bain/whisperX", "python3 -m venv ~/.local/share/ventosa/stt && ~/.local/share/ventosa/stt/bin/pip install whisperx"),
    rec("cron", "Schedules (cron with nodes)", "service", "Ventosa schedules: run an AI step every N minutes; batches run whole graphs.", false, "", ""),
    rec("voice", "Voice provider", "service", "Speech mode: speech-to-text and spoken replies (ChatGPT, ElevenLabs, DeepInfra, OpenRouter).", false, "https://elevenlabs.io/app/settings/api-keys", ""),
];

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

fn home(rel: &str) -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(rel)
}

/// MCP server names registered in Claude Code or OpenCode.
pub fn mcp_names() -> Vec<String> {
    let mut out = vec![];
    let read = |p: &Path| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    if let Some(v) = read(&home(".claude.json")) {
        out.extend(v["mcpServers"].as_object().into_iter().flat_map(|m| m.keys().cloned()));
        for p in v["projects"].as_object().into_iter().flat_map(|m| m.values()) {
            out.extend(p["mcpServers"].as_object().into_iter().flat_map(|m| m.keys().cloned()));
        }
    }
    for f in [".config/opencode/opencode.json", ".config/opencode/config.json"] {
        if let Some(v) = read(&home(f)) {
            out.extend(v["mcp"].as_object().into_iter().flat_map(|m| m.keys().cloned()));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Is the tool present? `mcp` lists the registered MCP servers (see `mcp_names`).
pub fn present(r: &Rec, cfg: &Config, mcp: &[String]) -> bool {
    let has_mcp = |names: &[&str]| mcp.iter().any(|m| names.iter().any(|n| m.eq_ignore_ascii_case(n)));
    match r.id {
        "provider" => cfg.provider.kind != "none" && std::env::var(&cfg.provider.api_key_env).is_ok_and(|v| !v.is_empty()),
        "voice" => !cfg.voice.api_key_env.is_empty() && std::env::var(&cfg.voice.api_key_env).is_ok_and(|v| !v.is_empty()),
        "hyperframes" => on_path("hyperframes") || home(".npm-global/bin/hyperframes").is_file() || Path::new("/usr/lib/node_modules/hyperframes").is_dir(),
        "metalvj" => on_path("metalvj-launcher") || on_path("metal-dash"),
        "metal-mcp" => has_mcp(&["metalvj", "metal-mcp"]),
        "remotion" => on_path("remotion") || home(".npm-global/bin/remotion").is_file(),
        "blender-mcp" => has_mcp(&["blender", "blender-mcp"]),
        "cron" => cfg.schedules.iter().any(|s| s.enabled),
        "filmcraft" | "effectcraft" | "anicraft" | "vectorcraft" | "photocraft" | "designcraft" | "figcraft" | "deckcraft" | "texturecraft" | "materialcraft" | "soundcraft" | "cadcraft" | "gridcraft" => has_mcp(&[r.id]) || on_path(&format!("{}-cli", r.id)),
        "whisperx" => on_path("whisperx") || on_path("whisper-cli") || home(".local/share/ventosa/stt/bin/python").is_file(),
        bin => on_path(bin),
    }
}

/// Every recommendation with its state.
pub fn report(cfg: &Config) -> Vec<(Rec, bool)> {
    let mcp = mcp_names();
    RECS.iter().map(|r| (r.clone(), present(r, cfg, &mcp))).collect()
}

/// Critical tools that are missing and not dismissed.
pub fn missing_critical(cfg: &Config) -> Vec<Rec> {
    report(cfg).into_iter().filter(|(r, ok)| r.critical && !ok && !cfg.dismissed_tools.iter().any(|d| d == r.id)).map(|(r, _)| r).collect()
}

pub fn to_json(cfg: &Config) -> Value {
    json!(report(cfg)
        .into_iter()
        .map(|(r, ok)| json!({"id": r.id, "name": r.name, "kind": r.kind, "what": r.what, "critical": r.critical, "present": ok, "link": r.link, "install": r.install}))
        .collect::<Vec<_>>())
}

/// Featured AI providers for onboarding: (name, kind, base_url, model, key env, link).
pub fn featured_providers() -> Vec<(&'static str, &'static str, &'static str, &'static str, &'static str, &'static str)> {
    vec![
        ("Anthropic (Claude)", "anthropic", "https://api.anthropic.com", "claude-opus-5-5", "ANTHROPIC_API_KEY", "https://console.anthropic.com/settings/keys"),
        ("OpenAI (ChatGPT)", "openai-compatible", "https://api.openai.com/v1", "gpt-5", "OPENAI_API_KEY", "https://platform.openai.com/api-keys"),
        ("OpenRouter", "openai-compatible", "https://openrouter.ai/api/v1", "anthropic/claude-opus-5.5", "OPENROUTER_API_KEY", "https://openrouter.ai/settings/keys"),
        ("DeepInfra", "openai-compatible", "https://api.deepinfra.com/v1/openai", "deepseek-ai/DeepSeek-V3", "DEEPINFRA_API_KEY", "https://deepinfra.com/dash/api_keys"),
    ]
}

/// Open an install command in a terminal so the user sees it run.
pub fn run_in_terminal(cmd: &str) -> Result<(), String> {
    let term = std::env::var("TERMINAL").ok().filter(|t| on_path(t)).unwrap_or_else(|| if on_path("alacritty") { "alacritty".into() } else { "xterm".into() });
    std::process::Command::new(term)
        .args(["-e", "sh", "-c", &format!("{cmd}; echo; echo '[enter to close]'; read _")])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
