//! Shared stores used by the egui app, the MCP server and the importer.
//! JSON files, written atomically (temp file + rename).

pub mod apps;
pub mod batch;
pub mod callback;
pub mod bots;
pub mod chat;
pub mod config;
pub mod deploy;
pub mod gallery;
pub mod graph;
pub mod help;
pub mod hooks;
pub mod library;
pub mod linkmedia;
pub mod identity;
pub mod install;
pub mod models;
pub mod plugin;
pub mod preview;
pub mod queue;
pub mod recommend;
pub mod research;
pub mod runner;
pub mod tplog;
pub mod tools;
pub mod trusted;
pub mod usage;
pub mod voice;
pub mod wiki;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub done: bool,
    pub created: u64,
    #[serde(default)]
    pub source: String,
}

fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa")
}

/// `$XDG_DATA_HOME/ventosa/tasks.json` unless `VENTOSA_FILE` is set.
pub fn default_path() -> PathBuf {
    std::env::var("VENTOSA_FILE").map(PathBuf::from).unwrap_or_else(|_| data_dir().join("tasks.json"))
}

/// `queue.json` beside the task file (or `VENTOSA_QUEUE`).
pub fn queue_path() -> PathBuf {
    std::env::var("VENTOSA_QUEUE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_path().with_file_name("queue.json"))
}

pub fn load<T: DeserializeOwned + Default>(path: &PathBuf) -> T {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save<T: Serialize + ?Sized>(path: &PathBuf, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path)
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn add(tasks: &mut Vec<Task>, title: &str) -> Task {
    add_from(tasks, title, "ui")
}

pub fn add_from(tasks: &mut Vec<Task>, title: &str, source: &str) -> Task {
    let id = tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let task = Task {
        id,
        title: title.trim().to_string(),
        done: false,
        created: now(),
        source: source.to_string(),
    };
    tasks.push(task.clone());
    task
}

pub fn set_done(tasks: &mut [Task], id: u64, done: bool) -> bool {
    match tasks.iter_mut().find(|t| t.id == id) {
        Some(t) => {
            t.done = done;
            true
        }
        None => false,
    }
}

pub fn delete(tasks: &mut Vec<Task>, id: u64) -> bool {
    let before = tasks.len();
    tasks.retain(|t| t.id != id);
    tasks.len() != before
}
