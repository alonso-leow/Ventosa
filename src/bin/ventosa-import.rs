//! Import todos from an existing system's JSON export into the task list.
//! Input: a JSON array of `{"title": "...", "done": false?, "source": "..."?}`
//! or of plain strings. Usage: `ventosa-import <file.json> [--source name]`.

use serde::Deserialize;
use std::process::ExitCode;
use ventosa::{add_from, default_path, load, save, set_done, Task};

#[derive(Deserialize)]
#[serde(untagged)]
enum Row {
    Text(String),
    Obj {
        title: String,
        #[serde(default)]
        done: bool,
        #[serde(default)]
        source: Option<String>,
    },
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else {
        eprintln!("usage: ventosa-import <file.json> [--source name]");
        return ExitCode::FAILURE;
    };
    let source = args
        .iter()
        .position(|a| a == "--source")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| format!("import:{file}"));

    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let rows: Vec<Row> = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("invalid JSON array: {e}");
            return ExitCode::FAILURE;
        }
    };

    let path = default_path();
    let mut tasks: Vec<Task> = load(&path);
    let mut added = 0;
    for row in rows {
        let (title, done, src) = match row {
            Row::Text(t) => (t, false, None),
            Row::Obj { title, done, source } => (title, done, source),
        };
        if title.trim().is_empty() {
            continue;
        }
        let task = add_from(&mut tasks, &title, src.as_deref().unwrap_or(&source));
        if done {
            set_done(&mut tasks, task.id, true);
        }
        added += 1;
    }
    if let Err(e) = save(&path, &tasks) {
        eprintln!("save failed: {e}");
        return ExitCode::FAILURE;
    }
    println!("imported {added} task(s) into {}", path.display());
    ExitCode::SUCCESS
}
