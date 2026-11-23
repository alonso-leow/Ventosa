//! MCP server (stdio, JSON-RPC 2.0, newline-delimited). A thin wrapper over
//! `ventosa::tools`, the tool layer it shares with the built-in chat.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use ventosa::{config, tools};

const PROTOCOL: &str = "2024-11-05";

/// The MCP server is unrestricted except for capabilities set to `deny`.
fn denied(name: &str) -> Option<Value> {
    let cfg = config::load(&config::default_path());
    (cfg.policy(tools::cap(name)) == "deny").then(|| {
        json!({"content": [{"type": "text", "text": "denied by ventosa settings ▸ permissions"}], "isError": true})
    })
}

fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "ventosa", "version": env!("CARGO_PKG_VERSION")}
        }),
        "tools/list" => json!({"tools": tools::list()}),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            denied(name).unwrap_or_else(|| tools::call(name, &args))
        }
        "ping" => json!({}),
        _ if id.is_none() => return None,
        _ => {
            return Some(json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": format!("method not found: {method}")}}))
        }
    };
    id.as_ref()?;
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&req),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": format!("parse error: {e}")}})),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}
