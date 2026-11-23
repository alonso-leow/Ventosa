//! White-label configuration: brand and AI provider. Lives in
//! `$XDG_CONFIG_HOME/ventosa/config.toml`. API keys are never stored here;
//! `api_key_env` names the environment variable that holds the key.

use crate::tools::Cap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Brand {
    pub name: String,
    pub tagline: String,
    /// Accent colour as `#rrggbb`. Used for the emphasis colour in the UI.
    pub accent: String,
    /// The secondary emphasis (to-do, warnings of state) as `#rrggbb`; empty = stock orange.
    pub accent2: String,
}

impl Default for Brand {
    fn default() -> Self {
        Self {
            name: "ventosa".into(),
            tagline: "to do".into(),
            accent: "#8b3dff".into(),
            accent2: "#ff8a1f".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Provider {
    /// `anthropic`, `openai-compatible`, or `none` (manual queue only).
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub api_key_env: String,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            kind: "none".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-opus-5-5".into(),
            api_key_env: "ANTHROPIC_API_KEY".into(),
        }
    }
}

/// A workspace: a folder with the user's skill graph, knowledge and data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Workspace {
    pub name: String,
    pub root: String,
    /// `ventosa` = run Ventosa's engine on the folder; `local` = the folder ships
    /// its own `pipeline/*.py` (e.g. a workspace that predates Ventosa).
    pub engine: String,
    /// Sync this workspace into the queue.
    pub enabled: bool,
}

impl Default for Workspace {
    fn default() -> Self {
        Self { name: String::new(), root: String::new(), engine: "ventosa".into(), enabled: true }
    }
}

/// A cron-like job: every N minutes, run one AI step for the oldest queued item at a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Schedule {
    pub id: String,
    pub workspace: String,
    pub node: String,
    pub every_minutes: u64,
    /// `command` (an MCP client / CLI agent) or `provider` (direct API call, output goes to review).
    pub runner: String,
    /// Shell command for `command` runners. Placeholders: {step} {subject} {title} {root} {workspace} {item}.
    pub command: String,
    pub enabled: bool,
    /// What a run does: `node` (execute the skill of `node` for the oldest queued item) or
    /// `graph` (no item: work on the graph itself, editing steps and skills).
    pub task: String,
    /// The job's own internal prompt, attached to its one phase (`node`) on top of the meta graph.
    pub prompt: String,
    /// What the final output must look like so the human's work is easy (the `agent` runner appends it).
    pub output_policy: String,
    /// Tools the `agent` runner may call; empty = the unattended default set.
    pub allowed_tools: Vec<String>,
    /// Also fire right after a human entry lands in the workspace (the entry hook), when an item is ready at the phase.
    pub on_entry: bool,
}

pub const DEFAULT_OUTPUT_POLICY: &str = "Finish with a HANDOFF note of at most 8 lines: what you did; the files or ids you wrote; what the human must check; the single next action and where (page or app). Put work in the workspace, never publish, upload or delete. If a human decision is needed, raise decision_ask and stop.";

impl Default for Schedule {
    fn default() -> Self {
        Self {
            id: String::new(),
            workspace: String::new(),
            node: "options".into(),
            every_minutes: 60,
            runner: "command".into(),
            command: "claude -p \"Use the ventosa MCP tools: attach to {step} {subject} with ventosa-graph and do the node skill.\"".into(),
            enabled: false,
            task: "node".into(),
            prompt: String::new(),
            output_policy: DEFAULT_OUTPUT_POLICY.into(),
            allowed_tools: vec![],
            on_entry: true,
        }
    }
}

/// Policy per capability: `allow`, `ask` or `deny`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Permissions {
    pub read: String,
    pub write: String,
    pub graph: String,
    pub install: String,
    pub apps: String,
    pub run: String,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            read: "allow".into(),
            write: "allow".into(),
            graph: "ask".into(),
            install: "ask".into(),
            apps: "ask".into(),
            run: "ask".into(),
        }
    }
}

/// Speech: one provider for speech-to-text (the speech instance listens) and one
/// for text-to-speech (it answers aloud). `kind`: `openai` (OpenAI-compatible
/// `/audio/transcriptions` + `/audio/speech`: OpenAI, DeepInfra, OpenRouter) or
/// `elevenlabs`. Keys come from `api_key_env`, never from this file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Voice {
    /// The preset this was filled from (see `voice_presets`).
    pub preset: String,
    pub kind: String,
    pub base_url: String,
    pub stt_model: String,
    pub tts_model: String,
    /// A voice name (OpenAI) or voice id (ElevenLabs).
    pub voice: String,
    pub api_key_env: String,
    /// Speak the AI's replies in speech mode.
    pub speak_replies: bool,
}

impl Default for Voice {
    fn default() -> Self {
        voice_presets().into_iter().next().map(|(_, v)| v).unwrap_or_else(|| Self {
            preset: String::new(),
            kind: "openai".into(),
            base_url: String::new(),
            stt_model: String::new(),
            tts_model: String::new(),
            voice: String::new(),
            api_key_env: String::new(),
            speak_replies: true,
        })
    }
}

fn voice(preset: &str, kind: &str, base: &str, stt: &str, tts: &str, voice: &str, env: &str) -> Voice {
    Voice { preset: preset.into(), kind: kind.into(), base_url: base.into(), stt_model: stt.into(), tts_model: tts.into(), voice: voice.into(), api_key_env: env.into(), speak_replies: true }
}

/// Voice presets: (label, settings). Pick one, then adjust.
pub fn voice_presets() -> Vec<(&'static str, Voice)> {
    vec![
        ("ChatGPT (OpenAI)", voice("openai", "openai", "https://api.openai.com/v1", "gpt-4o-transcribe", "gpt-4o-mini-tts", "alloy", "OPENAI_API_KEY")),
        ("ElevenLabs", voice("elevenlabs", "elevenlabs", "https://api.elevenlabs.io", "scribe_v1", "eleven_multilingual_v2", "21m00Tcm4TlvDq8ikWAM", "ELEVENLABS_API_KEY")),
        ("DeepInfra", voice("deepinfra", "openai", "https://api.deepinfra.com/v1/openai", "openai/whisper-large-v3-turbo", "hexgrad/Kokoro-82M", "af_bella", "DEEPINFRA_API_KEY")),
        ("OpenRouter", voice("openrouter", "openai", "https://openrouter.ai/api/v1", "openai/whisper-1", "openai/gpt-4o-mini-tts", "alloy", "OPENROUTER_API_KEY")),
    ]
}

/// How the batch executor uses the AI: concurrency, request rate and recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Limits {
    /// Items run at the same time.
    pub concurrency: usize,
    /// Runs started per minute, across all batches (0 = no limit).
    pub requests_per_minute: u32,
    /// Retries of a failed run before the item is marked failed.
    pub retries: u32,
    /// First wait before a retry; it doubles each time.
    pub backoff_secs: u64,
    /// A run that takes longer is stopped and retried.
    pub timeout_minutes: u64,
    /// Upper bound on subjects one batch may create.
    pub max_batch: usize,
    /// Callbacks started from inside a callback, at most this deep.
    pub callback_depth: usize,
    /// Callbacks to the same node of one subject per hour.
    pub callback_per_hour: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { concurrency: 2, requests_per_minute: 20, retries: 3, backoff_secs: 30, timeout_minutes: 20, max_batch: 50, callback_depth: 3, callback_per_hour: 2 }
    }
}

/// A server the AI may deploy apps to over SSH. The key stays in `key_path`
/// (or the ssh agent); nothing secret is written here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Server {
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    /// Private key file; empty = ssh agent / ~/.ssh/config.
    pub key_path: String,
    /// Remote folder the apps go under (`<remote_dir>/<app id>/`).
    pub remote_dir: String,
    /// Public base URL that serves `remote_dir` (a reverse proxy or static host),
    /// e.g. `https://apps.example.com`. Empty = `http://<host>:<app port>`.
    pub public_url: String,
    /// Allow AI tools to use this server.
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub brand: Brand,
    pub provider: Provider,
    /// Models tried in order when the provider fails (`provider:model`), after every key of the primary was tried.
    pub fallbacks: Vec<String>,
    pub voice: Voice,
    pub workspaces: Vec<Workspace>,
    pub schedules: Vec<Schedule>,
    pub permissions: Permissions,
    pub limits: Limits,
    pub servers: Vec<Server>,
    /// What we make, in the user's words (onboarding's last step); every AI prompt gets it.
    pub bio: String,
    /// Recommended tools the user chose to stop being warned about.
    pub dismissed_tools: Vec<String>,
    /// Set once the first-run onboarding is finished or skipped.
    pub onboarded: bool,
    /// Telegram bots run by the desktop app (access ▸ bots).
    pub bots: Vec<Bot>,
}

/// A Telegram bot: a small graph of its own (receive → ai reply → send) that
/// runs inside the desktop app. The token comes from `token_env`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Bot {
    pub name: String,
    pub token_env: String,
    pub workspace: String,
    /// Tools its AI may call (empty = none: answers from the prompt only).
    pub tools: Vec<String>,
    /// Telegram chat ids that may use it.
    pub allowed_chats: Vec<i64>,
    /// Extra instructions for its replies.
    pub prompt: String,
    pub enabled: bool,
}

impl Default for Bot {
    fn default() -> Self {
        Self {
            name: "bot".into(),
            token_env: "TELEGRAM_BOT_TOKEN".into(),
            workspace: String::new(),
            tools: vec!["workspace_status".into(), "queue_list".into(), "list_tasks".into(), "help".into()],
            allowed_chats: vec![],
            prompt: "Answer briefly; you are the team's pipeline bot on Telegram.".into(),
            enabled: false,
        }
    }
}

impl Config {
    pub fn server(&self, name: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.name == name)
    }

    pub fn workspace(&self, name: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    /// The policy (`allow`, `ask`, `deny`) for a capability.
    pub fn policy(&self, cap: Cap) -> &str {
        let p = &self.permissions;
        match cap {
            Cap::Read => &p.read,
            Cap::Write => &p.write,
            Cap::Graph => &p.graph,
            Cap::Install => &p.install,
            Cap::Apps => &p.apps,
            Cap::Run => &p.run,
        }
    }
}

pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("VENTOSA_CONFIG") {
        return PathBuf::from(p);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ventosa")
        .join("config.toml")
}

pub fn load(path: &PathBuf) -> Config {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &PathBuf, cfg: &Config) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let body = toml::to_string_pretty(cfg).map_err(io::Error::other)?;
    fs::write(path, body)
}

/// Parse `#rrggbb` into RGB bytes; falls back to ventosa red.
pub fn parse_hex(s: &str) -> (u8, u8, u8) {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0xff2e2e);
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
