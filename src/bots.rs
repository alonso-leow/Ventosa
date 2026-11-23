//! Telegram bots, run by the desktop app: each enabled bot long-polls
//! `getUpdates`, answers allowed chats with an unattended AI run limited to the
//! bot's tools, and sends the reply with `sendMessage`. One Ventosa chat per
//! Telegram chat (`bot-<name>-<chat id>`, mode `bot`), saved like the others.
//! Status per bot (last update, refused chat ids) in `bots-status.json`.

use crate::chat;
use crate::config::{self, Bot, Config};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub fn status_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("bots-status.json")
}

pub fn status() -> Value {
    crate::load::<Option<Value>>(&status_path()).unwrap_or(json!({}))
}

fn set_status(name: &str, key: &str, v: Value) {
    let mut s = status();
    s[name][key] = v;
    let _ = crate::save(&status_path(), &s);
}

fn api(token: &str, method: &str, body: Value) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(40)).build();
    let r: Value = agent
        .post(&format!("https://api.telegram.org/bot{token}/{method}"))
        .send_json(body)
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    if r["ok"] == true {
        Ok(r["result"].clone())
    } else {
        Err(r["description"].as_str().unwrap_or("telegram error").to_string())
    }
}

fn reply(cfg: &Config, bot: &Bot, chat_id: i64, text: &str) -> String {
    let id = format!("bot-{}-{chat_id}", bot.name);
    let mut c = chat::load(&id).unwrap_or_else(|| chat::Chat { id: id.clone(), workspace: bot.workspace.clone(), mode: "bot".into(), ..Default::default() });
    c.tools = if bot.tools.is_empty() { vec!["\u{2205}".into()] } else { bot.tools.clone() };
    c.title = format!("telegram {} \u{00b7} {chat_id}", bot.name);
    let msg = if c.messages.is_empty() && !bot.prompt.trim().is_empty() { format!("{}\n\n{text}", bot.prompt.trim()) } else { text.to_string() };
    chat::user(&mut c, &msg);
    // keep the bot chats short: the last 30 messages
    if c.messages.len() > 30 {
        let cut = c.messages.len() - 30;
        let start = (cut..c.messages.len()).find(|&i| c.messages[i].role == "user").unwrap_or(cut);
        c.messages.drain(..start);
    }
    let out = chat::run_unattended(&mut c, cfg).unwrap_or_else(|e| format!("(error: {e})"));
    let _ = chat::save(&mut c);
    out
}

/// One poll of one bot. `offset` is the next update id.
fn poll(cfg: &Config, bot: &Bot, offset: &mut i64) -> Result<usize, String> {
    let token = std::env::var(&bot.token_env).map_err(|_| format!("{} is not set", bot.token_env))?;
    let ups = api(&token, "getUpdates", json!({"offset": *offset, "timeout": 20, "allowed_updates": ["message"]}))?;
    let mut n = 0;
    for u in ups.as_array().into_iter().flatten() {
        *offset = u["update_id"].as_i64().unwrap_or(*offset) + 1;
        let m = &u["message"];
        let (Some(chat_id), Some(text)) = (m["chat"]["id"].as_i64(), m["text"].as_str()) else { continue };
        if !bot.allowed_chats.contains(&chat_id) {
            let mut refused: Vec<i64> = status()[&bot.name]["refused"].as_array().into_iter().flatten().filter_map(|v| v.as_i64()).collect();
            if !refused.contains(&chat_id) {
                refused.push(chat_id);
            }
            set_status(&bot.name, "refused", json!(refused));
            let _ = api(&token, "sendMessage", json!({"chat_id": chat_id, "text": format!("This chat is not allowed yet. Ask an admin to add chat id {chat_id} in Ventosa (access ▸ bots).")}));
            continue;
        }
        let _ = api(&token, "sendChatAction", json!({"chat_id": chat_id, "action": "typing"}));
        let answer = reply(cfg, bot, chat_id, text);
        let text: String = answer.chars().take(4000).collect();
        api(&token, "sendMessage", json!({"chat_id": chat_id, "text": if text.trim().is_empty() { "(no reply)".to_string() } else { text }}))?;
        n += 1;
    }
    set_status(&bot.name, "polled", json!(crate::now()));
    Ok(n)
}

/// The bots thread: one long poll per enabled bot, in turn, forever.
pub fn run(flash: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        let mut offsets: HashMap<String, i64> = HashMap::new();
        loop {
            let cfg = config::load(&config::default_path());
            let bots: Vec<Bot> = cfg.bots.iter().filter(|b| b.enabled).cloned().collect();
            if bots.is_empty() {
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
            for b in &bots {
                let off = offsets.entry(b.name.clone()).or_insert(0);
                match poll(&cfg, b, off) {
                    Ok(0) => {}
                    Ok(n) => flash(format!("bot {} answered {n}", b.name)),
                    Err(e) => {
                        set_status(&b.name, "error", json!(e));
                        flash(format!("bot {}: {e}", b.name));
                        std::thread::sleep(Duration::from_secs(15));
                    }
                }
            }
        }
    });
}
