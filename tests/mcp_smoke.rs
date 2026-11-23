//! Stdio smoke test of `ventosa-mcp`: tools/list count and a media_list call against
//! a temp workspace. Uses VENTOSA_CONFIG so the real config.toml is never touched.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn mcp_lists_tools_and_media_list_works() {
    let base = std::env::temp_dir().join(format!("ventosa-mcp-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let ws_root = base.join("ws");
    std::fs::create_dir_all(ws_root.join("assets/edits")).unwrap();
    std::fs::write(ws_root.join("assets/edits/V03-final.mp4"), b"x").unwrap();
    let cfg = base.join("config.toml");
    std::fs::write(
        &cfg,
        format!(
            "[[workspaces]]\nname = \"smoke\"\nroot = \"{}\"\nengine = \"ventosa\"\nenabled = true\n",
            ws_root.display()
        ),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ventosa-mcp"))
        .env("VENTOSA_CONFIG", &cfg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ventosa-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"media_list","arguments":{"workspace":"smoke"}}}),
    ];
    let mut replies = vec![];
    for req in requests {
        writeln!(stdin, "{req}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        replies.push(serde_json::from_str::<Value>(&line).expect("json reply"));
    }
    drop(stdin);
    let _ = child.wait();

    let tools = replies[0]["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), ventosa::tools::TOOLS.len());
    assert!(tools.len() >= 30, "expected the full registry, got {}", tools.len());

    let text = replies[1]["result"]["content"][0]["text"].as_str().expect("text result");
    assert!(replies[1]["result"]["isError"].is_null(), "media_list failed: {text}");
    let rows: Vec<Value> = serde_json::from_str(text).expect("media_list json");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["subject"], "V03");
    assert_eq!(rows[0]["kind"], "edit");

    let _ = std::fs::remove_dir_all(&base);
}
