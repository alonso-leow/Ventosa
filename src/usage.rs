//! Token usage the providers report, one line per request in
//! `$XDG_DATA_HOME/ventosa/usage.jsonl`: `{at, source, workspace, subject, step,
//! model, input, output}`. The dashboard sums it per video and per step.

use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;

thread_local! {
    /// (source, workspace, subject, step) of the request about to run on this thread.
    static CTX: RefCell<(String, String, String, String)> = RefCell::new(Default::default());
}

pub fn path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("usage.jsonl")
}

/// Name what the next provider requests on this thread are for.
pub fn context(source: &str, workspace: &str, subject: &str, step: &str) {
    CTX.with(|c| *c.borrow_mut() = (source.into(), workspace.into(), subject.into(), step.into()));
}

/// (input, output) tokens of a provider reply (Anthropic `usage.input_tokens`
/// + cache reads/writes; OpenAI `usage.prompt_tokens`/`completion_tokens`).
pub fn of_reply(resp: &Value) -> Option<(u64, u64)> {
    let u = &resp["usage"];
    if let Some(i) = u["input_tokens"].as_u64() {
        let cached = u["cache_read_input_tokens"].as_u64().unwrap_or(0) + u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
        return Some((i + cached, u["output_tokens"].as_u64().unwrap_or(0)));
    }
    Some((u["prompt_tokens"].as_u64()?, u["completion_tokens"].as_u64().unwrap_or(0)))
}

/// Append one request's usage, under the thread's context.
pub fn record(model: &str, input: u64, output: u64) {
    let (source, ws, subject, step) = CTX.with(|c| c.borrow().clone());
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let line = json!({"at": crate::now(), "source": source, "workspace": ws, "subject": subject, "step": step, "model": model, "input": input, "output": output});
        let _ = writeln!(f, "{line}");
    }
}
