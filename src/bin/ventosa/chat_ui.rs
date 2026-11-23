//! The AI sidebar: chat dropdown, message list, approval card, composer with `@`
//! references, plus the small AI surfaces the pages share (custom app buttons,
//! preview rows, the media card and the settings permissions card).
//! Engine work (`chat::advance`) runs on a thread; the UI polls its slot.

use super::kit::{self, small_text};
use super::theme::Theme;
use super::{App, ChatRequest};
use eframe::egui::{self, text::{CCursor, CCursorRange}, text_edit::TextEditState, Context, Id, Key, Modifiers, RichText, TextEdit, Ui};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ventosa::apps::{self, AppDef};
use ventosa::chat::{self, Chat, Step, ToolCall, ToolResult};
use ventosa::config::{Config, Workspace};
use ventosa::preview;

const COMPOSER: &str = "ai-composer";
const POPUP_ROWS: usize = 8;
const KINDS: [&str; 6] = ["step", "skill", "item", "task", "file", "help"];

/// A finished engine run: the chat as the engine left it, and its step.
type Done = Arc<Mutex<Option<(Chat, Step, Option<(ToolCall, bool)>)>>>;

/// The `@` token under the caret: byte index of the `@` and the text after it.
#[derive(Clone, Debug, PartialEq)]
struct Mention {
    start: usize,
    query: String,
}

#[derive(Default)]
pub struct ChatState {
    /// The open chat (None = nothing open yet).
    pub chat: Option<Chat>,
    /// Saved chats, newest first (refreshed on each run and on switch).
    chats: Vec<Chat>,
    chats_loaded: bool,
    pub composer: String,
    /// Caret as a char index into `composer`, from the last frame.
    caret: usize,
    mention: Option<Mention>,
    dismissed: Option<usize>,
    pop_sel: usize,
    cands: Vec<(String, String)>,
    job: Option<Done>,
    approval: Option<ToolCall>,
    error: Option<String>,
    expanded: HashSet<String>,
    focus: bool,
    /// `text` or `speech`: the mode new chats get (one ChatState per AI instance).
    pub mode: String,
    /// The history list is open instead of the conversation.
    history: bool,
    hist_page: usize,
    hist_batches: bool,
    /// How many of the latest messages are shown (grows with "older").
    msg_limit: usize,
    /// The last final reply, for the speech instance to speak.
    pub last_reply: Option<String>,
    /// A tool grant every run of this instance is held to (the launcher's chat:
    /// the user's role). None = every tool the settings allow.
    pub grant: Option<Vec<String>>,
    /// The prompt drawer (MetalVJ's): where it rests and a drag in progress.
    drawer: Snap,
    drawer_drag: Option<f32>,
    /// Height the composer took last frame (the messages get the rest).
    composer_used: f32,
}

/// Where the prompt drawer rests: the composer's own height, half the panel, the whole panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum Snap {
    #[default]
    Collapsed,
    Half,
    Full,
}

pub fn snap_height(snap: Snap, collapsed: f32, sheet: f32) -> f32 {
    match snap {
        Snap::Collapsed => collapsed,
        Snap::Half => (sheet * 0.5).max(collapsed),
        Snap::Full => sheet.max(collapsed),
    }
}

/// The snap nearest a height (ties go to the smaller).
#[allow(dead_code)]
pub fn nearest_snap(h: f32, collapsed: f32, sheet: f32) -> Snap {
    [Snap::Collapsed, Snap::Half, Snap::Full]
        .into_iter()
        .min_by(|a, b| (snap_height(*a, collapsed, sheet) - h).abs().total_cmp(&(snap_height(*b, collapsed, sheet) - h).abs()))
        .unwrap_or_default()
}

const HIST_PAGE: usize = 20;
const MSG_PAGE: usize = 40;

impl ChatState {
    pub fn with_mode(mode: &str) -> Self {
        Self { mode: mode.into(), msg_limit: MSG_PAGE, ..Default::default() }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn fresh(&self, ws: &str) -> Chat {
        let mut c = chat::new_chat(ws);
        c.mode = if self.mode.is_empty() { "text".into() } else { self.mode.clone() };
        if let Some(g) = &self.grant {
            c.tools = g.clone();
        }
        c
    }
}

/// Custom apps of the header's workspace, refreshed at most every 5 s.
#[derive(Default)]
pub struct AppsCache {
    ws: String,
    at: Option<Instant>,
    pub list: Vec<(AppDef, bool)>,
}

impl AppsCache {
    pub fn refresh(&mut self, cfg: &Config, ws_name: &str) {
        let fresh = self.at.is_some_and(|t| t.elapsed() < Duration::from_secs(5));
        if fresh && self.ws == ws_name {
            return;
        }
        self.ws = ws_name.to_string();
        self.at = Some(Instant::now());
        self.list = cfg
            .workspace(ws_name)
            .map(|w| apps::list(w).into_iter().map(|a| {
                let running = apps::running(w, &a.id).is_some();
                (a, running)
            }).collect())
            .unwrap_or_default();
    }
}

// ---------------------------------------------------------------- sidebar

/// The AI sidebar (graph view, wide windows).
pub fn sidebar(app: &mut App, ui: &mut Ui) {
    panel(app, ui, false);
}

/// The AI view: the same chat over the whole screen, in a centred column.
pub fn full(app: &mut App, ui: &mut Ui) {
    let w = ui.available_width();
    let col = w.min(960.0);
    ui.horizontal_top(|ui| {
        ui.add_space(((w - col) * 0.5).max(0.0));
        ui.allocate_ui_with_layout(egui::vec2(col, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(egui::vec2(col, ui.available_height()));
            panel(app, ui, true);
        });
    });
}

/// Head (title, history, new), then the history list or the conversation and the composer.
pub fn panel(app: &mut App, ui: &mut Ui, _full: bool) {
    let t = app.theme.clone();
    let ctx = ui.ctx().clone();
    if app.chat.msg_limit == 0 {
        app.chat.msg_limit = MSG_PAGE;
    }
    poll(app);
    take_request(app, &ctx);
    if !app.chat.chats_loaded {
        reload_list(app);
        // the newest saved chat of this instance's mode opens by default
        if app.chat.chat.is_none() {
            let mode = if app.chat.mode.is_empty() { "text".to_string() } else { app.chat.mode.clone() };
            app.chat.chat = app.chat.chats.iter().find(|c| mode_of(c) == mode).cloned();
        }
    }
    app.chat_has_session = app.chat.chat.as_ref().is_some_and(|c| !c.messages.is_empty());

    egui::Frame::new().fill(t.panel).stroke(t.rule_stroke()).inner_margin(egui::Margin::ZERO).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        let busy = app.chat.job.is_some();
        kit::panel_head(ui, &t, |ui| {
            let name = if app.chat.mode == "speech" { "speech" } else { "ai" };
            ui.label(RichText::new(name).font(t.font_heading.clone()).color(t.primary));
            ui.add_space(6.0);
            let title = app.chat.chat.as_ref().map(label_of).unwrap_or_else(|| "new chat".into());
            ui.add(egui::Label::new(small_text(&t, title).color(t.ink_weak)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::link(ui, &t, "New", t.primary_lit).clicked() && !busy {
                    app.chat.chat = Some(app.chat.fresh(&app.ws_sel));
                    app.chat.approval = None;
                    app.chat.error = None;
                    app.chat.history = false;
                    app.chat.msg_limit = MSG_PAGE;
                }
                let h = if app.chat.history { "back" } else { "history" };
                if kit::link(ui, &t, h, t.ink_weak).clicked() {
                    app.chat.history = !app.chat.history;
                    reload_list(app);
                }
            });
        });
        egui::Frame::new().inner_margin(t.panel_margin()).show(ui, |ui| {
            if app.chat.history {
                history(app, ui, &t, busy);
                return;
            }
            let h = ui.available_height();
            let row_h = ui.text_style_height(&egui::TextStyle::Body).max(t.font_small.size + 4.0);
            let base = row_h * MAX_ROWS as f32 + t.field_pad.y * 2.0;
            let chrome = 74.0;
            let sheet = (h - chrome).max(base);
            let target = app.chat.drawer_drag.unwrap_or_else(|| snap_height(app.chat.drawer, base, sheet));
            let settle = if app.chat.drawer_drag.is_some() { 0.0 } else { 0.18 };
            let field_h = ctx.animate_value_with_time(Id::new(("ai-drawer", app.chat.mode.clone())), target, settle);
            let used = if app.chat.composer_used > 0.0 { app.chat.composer_used } else { base + chrome };
            egui::ScrollArea::vertical()
                .id_salt(("ai-messages", app.chat.mode.clone()))
                .max_height((h - used - 4.0).max(0.0))
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| messages(app, ui, &t, &ctx));
            ui.add_space(4.0);
            let top = ui.cursor().top();
            context_bar(app, ui, &t);
            composer(app, ui, &t, &ctx, field_h);
            app.chat.composer_used = ui.cursor().top() - top;
        });
    });
}

fn mode_of(c: &Chat) -> &str {
    if c.mode.is_empty() { "text" } else { c.mode.as_str() }
}

/// Saved chats, a page at a time; batch runs are hidden unless asked for.
fn history(app: &mut App, ui: &mut Ui, t: &Theme, busy: bool) {
    // a granted instance (the launcher's chat) sees only its own chats
    let own = app.chat.grant.is_some().then(|| app.chat.mode.clone());
    let rows: Vec<Chat> = app
        .chat
        .chats
        .iter()
        .filter(|c| match &own {
            Some(m) => mode_of(c) == m,
            None => (app.chat.hist_batches || mode_of(c) != "batch") && !mode_of(c).starts_with("launcher:"),
        })
        .cloned()
        .collect();
    let pages = rows.len().div_ceil(HIST_PAGE).max(1);
    app.chat.hist_page = app.chat.hist_page.min(pages - 1);
    ui.horizontal(|ui| {
        ui.label(small_text(t, format!("{} chats · page {}/{pages}", rows.len(), app.chat.hist_page + 1)).color(t.ink_faint));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(app.chat.hist_page + 1 < pages, egui::Button::new(small_text(t, "Next \u{25b8}"))).clicked() {
                app.chat.hist_page += 1;
            }
            if ui.add_enabled(app.chat.hist_page > 0, egui::Button::new(small_text(t, "\u{25c2} prev"))).clicked() {
                app.chat.hist_page -= 1;
            }
            ui.checkbox(&mut app.chat.hist_batches, small_text(t, "Batch runs"));
        });
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt(("ai-history", app.chat.mode.clone())).auto_shrink([false, false]).show(ui, |ui| {
        let now = ventosa::now();
        for c in rows.iter().skip(app.chat.hist_page * HIST_PAGE).take(HIST_PAGE) {
            let open = app.chat.chat.as_ref().is_some_and(|o| o.id == c.id);
            let mode = mode_of(c);
            let badge = match mode { "speech" => "\u{25c9} speech", "batch" => "\u{25b6} batch", _ => "\u{25a4} text" };
            let ago = now.saturating_sub(c.updated);
            let when = if ago < 3600 { format!("{}m", ago / 60) } else if ago < 86400 { format!("{}h", ago / 3600) } else { format!("{}d", ago / 86400) };
            let text = format!("{badge}  {}  · {} · {} msg · {when}", if c.title.is_empty() { "untitled" } else { &c.title }, c.workspace, c.messages.len());
            let r = ui.add(egui::Button::selectable(open, small_text(t, text).color(if mode == "speech" { t.primary_lit } else { t.ink })).min_size(egui::vec2(ui.available_width(), t.cell_height())));
            if r.clicked() && !busy {
                app.chat.chat = chat::load(&c.id);
                app.chat.approval = None;
                app.chat.error = None;
                app.chat.history = false;
                app.chat.msg_limit = MSG_PAGE;
            }
        }
        if rows.is_empty() {
            kit::notice(ui, t, "no chats yet");
        }
    });
}

fn label_of(c: &Chat) -> String {
    let title = if c.title.is_empty() { "untitled" } else { c.title.as_str() };
    format!("{title} · {}", c.workspace)
}

fn reload_list(app: &mut App) {
    app.chat.chats = chat::list_chats();
    app.chat.chats_loaded = true;
}

/// Poll this instance's engine run without drawing (the speech instance runs in parallel).
pub fn tick(app: &mut App) {
    poll(app);
}

/// Send `text` as a user message now (speech transcripts, onboarding's graph request).
pub fn submit(app: &mut App, ctx: &Context, text: &str) {
    if app.chat.job.is_some() || text.trim().is_empty() {
        return;
    }
    app.chat.composer = text.to_string();
    app.chat.history = false;
    send(app, ctx);
}

/// Pick up a finished run from the engine thread.
fn poll(app: &mut App) {
    let done = app.chat.job.as_ref().and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()));
    let Some((chat, step, _)) = done else { return };
    app.chat.job = None;
    app.chat.chat = Some(chat);
    match step {
        Step::Reply(text) => {
            if app.chat.chat.as_ref().is_some_and(|c| c.mode == "speech") && !text.trim().is_empty() {
                app.chat.last_reply = Some(text);
            }
        }
        Step::NeedsApproval(call) => app.chat.approval = Some(call),
        Step::Error(e) => app.chat.error = Some(e),
    }
    reload_list(app);
}

/// Run `chat::advance` (after an optional approval) on a thread and keep the result in a slot.
fn start(app: &mut App, ctx: &Context, mut chat: Chat, decision: Option<(ToolCall, bool)>) {
    if let Some(g) = &app.chat.grant {
        // a granted instance never runs wider than its grant (an empty grant = no tools)
        chat.tools = if g.is_empty() { vec!["\u{2205}".into()] } else { g.clone() };
        chat.scope = app.access.user.as_ref().map(|u| u.workspaces.clone()).unwrap_or_default();
    }
    // a role runs only on the AI providers it may use (access ▸ roles ▸ connections)
    let pid = ventosa::models::CATALOG.iter().find(|i| i.key_env == app.cfg.provider.api_key_env).map_or("custom", |i| i.id);
    if !app.access.can_connection(&format!("ai-{pid}")) {
        app.chat.error = Some(format!("your role may not use the {pid} provider (access ▸ roles ▸ connections)"));
        return;
    }
    app.chat.chat = Some(chat.clone());
    app.chat.error = None;
    let cfg = app.cfg.clone();
    let slot: Done = Arc::new(Mutex::new(None));
    let (out, repaint) = (slot.clone(), ctx.clone());
    let mut work = chat;
    std::thread::spawn(move || {
        if let Some((call, allow)) = &decision {
            chat::approve(&mut work, call, *allow);
        }
        let step = chat::advance(&mut work, &cfg);
        let _ = chat::save(&mut work);
        if let Ok(mut g) = out.lock() {
            *g = Some((work, step, decision));
        }
        repaint.request_repaint();
    });
    app.chat.job = Some(slot);
}

fn send(app: &mut App, ctx: &Context) {
    let text = app.chat.composer.trim().to_string();
    if text.is_empty() || app.chat.job.is_some() {
        return;
    }
    let mut chat = app.chat.chat.take().unwrap_or_else(|| app.chat.fresh(&app.ws_sel));
    chat::user(&mut chat, &text);
    app.chat.composer.clear();
    app.chat.caret = 0;
    app.chat.mention = None;
    app.chat.approval = None;
    start(app, ctx, chat, None);
}

fn decide(app: &mut App, ctx: &Context, allow: bool) {
    let Some(call) = app.chat.approval.take() else { return };
    let Some(chat) = app.chat.chat.clone() else { return };
    start(app, ctx, chat, Some((call, allow)));
}

/// A `ChatRequest` from another page: open the sidebar, start or pick a chat, insert the reference.
fn take_request(app: &mut App, ctx: &Context) {
    let Some(req) = app.chat_req.take() else { return };
    app.chat_visible = true;
    let ChatRequest { mention, workspace, new_chat } = req;
    app.chat.history = false;
    if app.chat.job.is_none() && (new_chat || app.chat.chat.is_none()) {
        app.chat.chat = Some(app.chat.fresh(&workspace));
        app.chat.approval = None;
        app.chat.error = None;
    }
    let mut text = std::mem::take(&mut app.chat.composer);
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    text.push_str(&mention);
    text.push(' ');
    let caret = text.chars().count();
    app.chat.composer = text;
    app.chat.caret = caret;
    app.chat.mention = None;
    app.chat.focus = true;
    set_caret(ctx, caret);
    reload_list(app);
}

// ---------------------------------------------------------------- messages

fn messages(app: &mut App, ui: &mut Ui, t: &Theme, ctx: &Context) {
    let Some(chat) = app.chat.chat.clone() else {
        kit::notice(ui, t, "no chat open — type below to start one, or @ to reference a step, skill, item or task");
        return;
    };
    let results: Vec<ToolResult> = chat.messages.iter().flat_map(|m| m.tool_results.clone()).collect();
    if chat.messages.is_empty() {
        kit::notice(ui, t, &format!("new chat in {} — ask about the workspace", chat.workspace));
    }
    let mut decided = None;
    let skip = chat.messages.len().saturating_sub(app.chat.msg_limit.max(1));
    if skip > 0 && ui.add(egui::Button::new(small_text(t, format!("\u{25b4} {skip} older message(s)")).color(t.ink_weak)).frame(false)).clicked() {
        app.chat.msg_limit += MSG_PAGE;
    }
    for m in chat.messages.iter().skip(skip) {
        match m.role.as_str() {
            "user" => {
                ui.add_space(4.0);
                ui.label(small_text(t, "You").color(t.ink_faint));
                rich_text(ui, t, &m.text);
            }
            "assistant" => {
                if !m.text.is_empty() {
                    ui.add_space(4.0);
                    ui.label(small_text(t, "Ai").color(t.primary_lit));
                    ui.add(egui::Label::new(RichText::new(&m.text).font(t.font_body.clone()).color(t.ink)).wrap());
                }
                for call in &m.tool_calls {
                    let res = results.iter().find(|r| r.id == call.id);
                    tool_card(app, ui, t, call, res);
                }
            }
            _ => {}
        }
    }
    if let Some(call) = app.chat.approval.clone() {
        ui.add_space(6.0);
        kit::card_tinted(ui, t, Some("approval needed"), t.primary_dim, |ui| {
            let cap = ventosa::tools::cap(&call.name);
            kit::field_row(ui, t, "tool", &call.name);
            kit::field_row(ui, t, "capability", &format!("{cap:?}").to_lowercase());
            ui.add(egui::Label::new(small_text(t, call.args.to_string()).color(t.ink_weak)).wrap());
            ui.horizontal(|ui| {
                if kit::ai_button(ui, t, "allow").clicked() {
                    decided = Some(true);
                }
                if kit::ai_button(ui, t, "deny").clicked() {
                    decided = Some(false);
                }
            });
        });
    }
    if app.chat.job.is_some() {
        ui.add_space(4.0);
        ui.label(small_text(t, "Thinking…").color(t.primary_lit));
    }
    if let Some(e) = app.chat.error.clone() {
        ui.add_space(4.0);
        ui.add(egui::Label::new(small_text(t, e).color(t.secondary)).wrap());
    }
    if let Some(allow) = decided {
        decide(app, ctx, allow);
    }
}

/// A tool call as a compact card: name, one-line args, status; the result folds open.
fn tool_card(app: &mut App, ui: &mut Ui, t: &Theme, call: &ToolCall, res: Option<&ToolResult>) {
    let open = app.chat.expanded.contains(&call.id);
    egui::Frame::new().stroke(egui::Stroke::new(t.hairline, t.rule_bright)).inner_margin(t.card_pad).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
            if ui.add(egui::Button::new(small_text(t, format!("{arrow} {}", call.name)).color(t.primary_lit)).frame(false)).clicked() {
                if open {
                    app.chat.expanded.remove(&call.id);
                } else {
                    app.chat.expanded.insert(call.id.clone());
                }
            }
            let (status, color) = match res {
                Some(r) if r.is_error => ("error", t.secondary),
                Some(_) => ("done", t.ink_faint),
                None if app.chat.approval.as_ref().is_some_and(|c| c.id == call.id) => ("awaiting approval", t.warn),
                None => ("running", t.ink_faint),
            };
            ui.add(egui::Label::new(small_text(t, status).color(color)));
        });
        let args = call.args.to_string();
        let args = if args == "null" || args == "{}" { String::new() } else { args };
        if !args.is_empty() {
            ui.add(egui::Label::new(small_text(t, args).color(t.ink_weak)).truncate());
        }
        if open {
            if let Some(r) = res {
                let color = if r.is_error { t.secondary } else { t.ink_weak };
                ui.add(egui::Label::new(RichText::new(&r.text).font(t.font_small.clone()).color(color)).wrap());
            }
        }
    });
    ui.add_space(4.0);
}

/// A user message with `@` references drawn as chips.
fn rich_text(ui: &mut Ui, t: &Theme, text: &str) {
    for line in text.lines() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
            for word in line.split(' ').filter(|w| !w.is_empty()) {
                let core = word.trim_end_matches('.');
                let tail = &word[core.len()..];
                if is_ref(core) {
                    kit::chip(ui, t, core);
                    if !tail.is_empty() {
                        ui.label(small_text(t, tail).color(t.ink));
                    }
                } else {
                    ui.label(small_text(t, word).color(t.ink));
                }
            }
        });
    }
}

fn is_ref(tok: &str) -> bool {
    if tok == "@workspace" || tok == "@help" {
        return true;
    }
    match tok.strip_prefix('@').and_then(|r| r.split_once(':')) {
        Some((kind, rest)) => KINDS.contains(&kind) && !rest.is_empty(),
        None => false,
    }
}

// ---------------------------------------------------------------- composer

/// MetalVJ's prompt box: one inset, square, outlined surface holding the `@` list,
/// a frameless multiline field with a `+` / gear column on its right, and a bottom
/// bar with the send arrow (white, AI colour on hover) or stop while a turn runs.
/// Tokens the conversation takes now: what the provider last reported (input +
/// output) when there is a report, else an estimate at four characters a token.
/// The prompt being typed is added on top.
pub fn used_tokens(chat: Option<&Chat>, pending_chars: usize) -> (u64, bool) {
    let pending = (pending_chars as u64).div_ceil(4);
    let Some(c) = chat else { return (pending, false) };
    match (c.usage["input"].as_u64(), c.usage["output"].as_u64()) {
        (Some(i), Some(o)) => (i + o + pending, true),
        _ => {
            let chars: usize = c.messages.iter().map(|m| m.text.len() + m.tool_results.iter().map(|r| r.text.len()).sum::<usize>() + m.tool_calls.iter().map(|x| x.args.to_string().len()).sum::<usize>()).sum();
            // the system prompt and tool schemas, roughly
            (((chars + 14_000) as u64).div_ceil(4) + pending, false)
        }
    }
}

/// The context bar over the composer: how much of the model's window the chat uses.
fn context_bar(app: &mut App, ui: &mut Ui, t: &Theme) {
    let model = app.cfg.provider.model.clone();
    let window: u64 = if model.contains("claude") { 200_000 } else { 128_000 };
    let (used, reported) = used_tokens(app.chat.chat.as_ref(), app.chat.composer.len());
    let share = (used as f32 / window as f32).clamp(0.0, 1.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0 + t.font_small.size + 4.0), egui::Sense::hover());
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 3.0));
    let p = ui.painter();
    p.rect_filled(bar, 0.0, t.inset);
    let col = if share > 0.85 { t.warn } else if share > 0.6 { t.secondary } else { t.primary_lit };
    p.rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * share, 3.0)), 0.0, col);
    let k = |n: u64| if n >= 1000 { format!("{:.1}k", n as f32 / 1000.0) } else { n.to_string() };
    let text = format!("{} / {} tokens \u{00b7} {}", k(used), k(window), if reported { "reported" } else { "estimate" });
    p.text(egui::pos2(rect.left(), bar.bottom() + 2.0), egui::Align2::LEFT_TOP, text, t.font_small.clone(), t.ink_faint);
    resp.on_hover_text("the provider's reported usage of the last step (input + output) when there is one; otherwise an estimate at 4 characters a token");
}

fn composer(app: &mut App, ui: &mut Ui, t: &Theme, ctx: &Context, drawer_h: f32) {
    let id = Id::new(COMPOSER);
    let busy = app.chat.job.is_some();
    let focused = ctx.memory(|m| m.has_focus(id));
    // Esc, while the field has focus, folds an open drawer back down
    if focused && app.chat.drawer != Snap::Collapsed && app.chat.mention.is_none() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        app.chat.drawer = Snap::Collapsed;
    }
    let rows = popup_rows(app);
    let popup = !rows.is_empty() && !busy;
    let mut chosen: Option<String> = None;
    let mut send_key = false;

    // Keys first, so Enter/Tab/arrows never reach the text edit when they mean something here.
    ui.input_mut(|i| {
        if popup {
            let n = rows.len();
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                app.chat.pop_sel = (app.chat.pop_sel + 1) % n;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                app.chat.pop_sel = (app.chat.pop_sel + n - 1) % n;
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter) {
                chosen = Some(rows[app.chat.pop_sel.min(n - 1)].0.clone());
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                app.chat.dismissed = app.chat.mention.as_ref().map(|m| m.start);
            }
        } else if focused && i.consume_key(Modifiers::NONE, Key::Enter) {
            send_key = true;
        }
    });

    let row_h = ui.text_style_height(&egui::TextStyle::Body).max(t.font_small.size + 4.0);
    let fit = ((drawer_h - t.field_pad.y * 2.0) / row_h).floor().max(1.0) as usize;
    let lines = app.chat.composer.split('\n').count().clamp(MIN_ROWS, fit.max(MIN_ROWS));
    let max_h = drawer_h;
    let boxed = egui::Frame::new().fill(t.inset).stroke(t.rule_stroke()).corner_radius(0).inner_margin(t.panel_margin()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if popup {
            let sel = app.chat.pop_sel.min(rows.len() - 1);
            for (i, (tok, label)) in rows.iter().enumerate() {
                let text = format!("{tok}  {label}");
                let color = if i == sel { t.primary_lit } else { t.ink_weak };
                let r = ui.add(egui::Button::selectable(i == sel, small_text(t, text).color(color)).min_size(egui::vec2(ui.available_width(), 0.0)));
                if r.clicked() {
                    chosen = Some(tok.clone());
                }
            }
            ui.add_space(2.0);
        }
        let gap = ui.spacing().item_spacing.x;
        let btn_w = ui.spacing().interact_size.x.max(24.0);
        let bar_h = ui.spacing().interact_size.y.max(22.0);
        let row_w = ui.available_width();
        let field_w = (row_w - btn_w - gap).max(0.0);
        let field_h = if app.chat.drawer == Snap::Collapsed && app.chat.drawer_drag.is_none() {
            (row_h * lines as f32 + t.field_pad.y * 2.0).min(max_h).max(bar_h * 2.0)
        } else {
            max_h.max(bar_h * 2.0)
        };
        let (row, _) = ui.allocate_exact_size(egui::vec2(row_w, field_h), egui::Sense::hover());
        let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(row.min, egui::vec2(field_w, field_h))));
        let out = egui::ScrollArea::vertical().id_salt("ai-composer-scroll").max_height(field_h).auto_shrink([false, true]).show(&mut field_ui, |ui| {
            TextEdit::multiline(&mut app.chat.composer)
                .id(id)
                .frame(egui::Frame::NONE)
                .font(t.font_body.clone())
                .desired_rows(lines)
                .desired_width(field_w - 4.0)
                .hint_text(RichText::new("Ask about the graph… @ to reference").font(t.font_body.clone()).color(t.ink_faint))
                .show(ui)
        }).inner;
        if app.chat.focus {
            out.response.request_focus();
            app.chat.focus = false;
        }
        if let Some(r) = out.cursor_range {
            app.chat.caret = r.primary.index.into();
        }
        // right column: `+` (start a reference) over the gear (AI settings), frameless
        let col = egui::Rect::from_min_size(egui::pos2(row.min.x + field_w + gap, row.min.y), egui::vec2(btn_w, bar_h * 2.0));
        let mut col_ui = ui.new_child(egui::UiBuilder::new().max_rect(col));
        let plus = col_ui.add_sized([btn_w, bar_h], egui::Button::new(RichText::new("+").font(t.font_heading.clone()).color(t.ink_weak)).frame(false)).on_hover_text("reference a step, field, skill, item or task");
        if plus.clicked() {
            if !app.chat.composer.is_empty() && !app.chat.composer.ends_with(char::is_whitespace) {
                app.chat.composer.push(' ');
            }
            app.chat.composer.push('@');
            let caret = app.chat.composer.chars().count();
            app.chat.caret = caret;
            app.chat.focus = true;
            set_caret(ctx, caret);
        }
        let (gear, gr) = col_ui.allocate_exact_size(egui::vec2(btn_w, bar_h), egui::Sense::click());
        paint_sliders(col_ui.painter(), gear, egui::Stroke::new(1.2, if gr.hovered() { t.primary_lit } else { t.ink_weak }));
        if gr.on_hover_text("AI settings: provider and permissions").clicked() {
            app.view = super::View::Settings;
        }

        // bottom bar: hint on the left, send / stop at the right edge
        let (bar, _) = ui.allocate_exact_size(egui::vec2(row_w, bar_h), egui::Sense::hover());
        let hint = if busy { "thinking…" } else { "enter sends · shift+enter newline" };
        ui.painter().text(bar.left_center(), egui::Align2::LEFT_CENTER, hint, t.font_small.clone(), if busy { t.primary_lit } else { t.ink_faint });
        let send_rect = egui::Rect::from_min_size(egui::pos2(bar.max.x - btn_w, bar.min.y), egui::vec2(btn_w, bar_h));
        if busy {
            let stop = ui.put(send_rect, egui::Button::new(RichText::new("\u{25a0}").font(t.font_body.clone()).color(t.warn)).corner_radius(0)).on_hover_text("stop: drop this turn");
            if stop.clicked() {
                app.chat.job = None;
                app.chat.error = Some("stopped".into());
            }
        } else {
            let can = !app.chat.composer.trim().is_empty();
            let send_btn = ui.put(send_rect, egui::Button::new("").fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE).corner_radius(0));
            let hot = send_btn.hovered() && can;
            let fill = if hot { t.primary } else if can { egui::Color32::WHITE } else { t.ink_faint };
            ui.painter().rect_filled(send_rect, 0.0, fill);
            paint_send_arrow(ui.painter(), send_rect, egui::Stroke::new(1.6, if hot { egui::Color32::WHITE } else { t.tube }));
            if (send_btn.on_hover_text("send").clicked() || send_key) && can {
                send(app, ctx);
            }
        }
    });
    // the box lights its outline in the AI colour while the field has focus
    if focused {
        ui.painter().rect_stroke(boxed.response.rect, 0.0, t.ai_stroke(), egui::StrokeKind::Inside);
    }
    if let Some(tok) = chosen {
        insert_token(app, ctx, id, &tok);
    } else {
        refresh_mention(app);
    }
}

const MIN_ROWS: usize = 2;
const MAX_ROWS: usize = 8;

/// The send arrow (a shaft with a head) inside `rect`.
fn paint_send_arrow(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let c = rect.center();
    let half = rect.height() * 0.3;
    let (top, bottom) = (c.y - half, c.y + half);
    painter.line_segment([egui::pos2(c.x, top), egui::pos2(c.x, bottom)], stroke);
    painter.line_segment([egui::pos2(c.x - half, c.y), egui::pos2(c.x, top)], stroke);
    painter.line_segment([egui::pos2(c.x + half, c.y), egui::pos2(c.x, top)], stroke);
}

/// A small gear: a ring with six teeth.
#[allow(dead_code)]
pub fn paint_gear(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let c = rect.center();
    let r = rect.height().min(rect.width()) * 0.22;
    painter.circle_stroke(c, r, stroke);
    painter.circle_stroke(c, r * 0.35, stroke);
    for k in 0..6 {
        let a = k as f32 * std::f32::consts::TAU / 6.0;
        let d = egui::vec2(a.cos(), a.sin());
        painter.line_segment([c + d * r, c + d * (r * 1.45)], stroke);
    }
}

/// Candidates that match the `@` query under the caret (none when no popup is due).
fn popup_rows(app: &App) -> Vec<(String, String)> {
    let Some(m) = &app.chat.mention else { return vec![] };
    if app.chat.dismissed == Some(m.start) {
        return vec![];
    }
    let q = m.query.to_lowercase();
    app.chat
        .cands
        .iter()
        .filter(|(tok, label)| q.is_empty() || tok.to_lowercase().contains(&q) || label.to_lowercase().contains(&q))
        .take(POPUP_ROWS)
        .cloned()
        .collect()
}

/// Recompute the `@` token under the caret; load candidates when the list opens.
fn refresh_mention(app: &mut App) {
    let next = active(&app.chat.composer, app.chat.caret);
    if next.is_none() {
        app.chat.dismissed = None;
    }
    if next.is_some() && app.chat.mention.is_none() {
        let ws = app.chat.chat.as_ref().map(|c| c.workspace.clone()).unwrap_or_else(|| app.ws_sel.clone());
        app.chat.cands = chat::mention_candidates(&app.cfg, &ws);
        app.chat.pop_sel = 0;
    }
    app.chat.mention = next;
}

/// The `@` word the caret sits in: it must open a word and hold no whitespace up to the caret.
fn active(text: &str, caret_char: usize) -> Option<Mention> {
    let caret = char_to_byte(text, caret_char);
    let before = &text[..caret];
    let start = before.rfind(char::is_whitespace).map_or(0, |i| i + before[i..].chars().next().map_or(1, char::len_utf8));
    let query = before[start..].strip_prefix('@')?;
    Some(Mention { start, query: query.to_string() })
}

fn char_to_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

fn set_caret(ctx: &Context, char_index: usize) {
    let id = Id::new(COMPOSER);
    let mut st = TextEditState::load(ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(char_index))));
    st.store(ctx, id);
}

/// Replace the `@` word under the caret with `token` and a space.
fn insert_token(app: &mut App, ctx: &Context, id: Id, token: &str) {
    let Some(m) = app.chat.mention.clone() else { return };
    let text = app.chat.composer.clone();
    let caret_b = char_to_byte(&text, app.chat.caret).max(m.start);
    app.chat.composer = format!("{}{} {}", &text[..m.start], token, &text[caret_b..]);
    let caret = text[..m.start].chars().count() + token.chars().count() + 1;
    app.chat.caret = caret;
    app.chat.mention = None;
    app.chat.focus = true;
    let mut st = TextEditState::load(ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(caret))));
    st.store(ctx, id);
}

// ---------------------------------------------------------------- shared surfaces

/// `▶ preview` row for a subject with edited media (no row when there is none).
pub fn preview_row(app: &App, ui: &mut Ui, ws: &Workspace, subject: &str) {
    if subject.is_empty() {
        return;
    }
    let media = preview::media_for(ws, subject);
    let Some(newest) = media.first() else { return };
    let t = &app.theme;
    kit::label_row(ui, t, "preview", |ui| {
        if kit::payload_button(ui, t, small_text(t, format!("\u{25b6} preview · {}", newest.kind)).color(t.primary_lit)).clicked() {
            let m = play(&newest.path, &format!("{} {subject}", ws.name));
            app.set_flash(m);
        }
    });
    // what the teleprompter says was read in each camera take of this subject
    let map = ventosa::tplog::load(ws);
    for m in media.iter().filter(|m| m.kind == "take").take(4) {
        let Some(x) = map.matches.iter().find(|x| x.file == m.path.display().to_string() && x.method != "unmatched") else { continue };
        let name = m.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        kit::label_row(ui, t, "read", |ui| {
            ui.add(egui::Label::new(small_text(t, format!("{name}: {} {} ({:.0}%)", x.line, x.text.chars().take(60).collect::<String>(), x.confidence * 100.0)).color(t.ink)).truncate());
        });
    }
}

fn play(path: &Path, title: &str) -> String {
    match preview::play(path, title, None) {
        Ok(p) => format!("preview · {p}"),
        Err(e) => e,
    }
}

/// The `media` card of a workspace on the apps page: newest 12 edits, takes and renders.
#[allow(dead_code)]
pub fn media_card(app: &App, ui: &mut Ui, ws: &Workspace) {
    let t = &app.theme;
    let items: Vec<preview::Media> = preview::media(ws).into_iter().take(12).collect();
    kit::card(ui, t, Some(&format!("media · {}", ws.name)), |ui| {
        if items.is_empty() {
            kit::notice(ui, t, "no edits, takes or renders yet");
        }
        for m in &items {
            let name = m.path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let mut play_it = false;
            kit::label_row(ui, t, &m.subject, |ui| {
                let res = kit::payload_split(ui, t, 2, |ui, k| {
                    if k == 0 {
                        ui.add(egui::Label::new(small_text(t, format!("{} · {name}", m.kind)).color(t.ink)).truncate());
                        false
                    } else {
                        kit::payload_button(ui, t, small_text(t, "\u{25b6} play").color(t.primary_lit)).clicked()
                    }
                });
                play_it = res.get(1).copied().unwrap_or(false);
            });
            if play_it {
                let msg = play(&m.path, &format!("{} {}", ws.name, m.subject));
                app.set_flash(msg);
            }
        }
    });
}

/// The header's custom-app buttons for a workspace: `<title> ↗`, lit in the primary.
// ---------------------------------------------------------------- settings

/// Settings ▸ AI permissions: one policy per capability.
pub fn permissions_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let opts: Vec<(String, String)> = ["allow", "ask", "deny"].iter().map(|p| (p.to_string(), p.to_string())).collect();
    let mut changed = false;
    kit::card(ui, &t, Some("ai permissions"), |ui| {
        let p = &mut app.cfg.permissions;
        let rows: [(&str, &str, &mut String); 6] = [
            ("read", "list and read tasks, queue, workspaces and graph state", &mut p.read),
            ("write", "add or change tasks, queue items and sync", &mut p.write),
            ("graph", "change steps and skills of the skill graph", &mut p.graph),
            ("install", "store custom shell tools in the workspace", &mut p.install),
            ("apps", "build and start custom apps", &mut p.apps),
            ("run", "run a custom tool command in the workspace", &mut p.run),
        ];
        for (label, hint, value) in rows {
            kit::label_row(ui, &t, label, |ui| {
                changed |= kit::dropdown(ui, &t, ("perm", label), value, &opts);
            });
            ui.add(egui::Label::new(small_text(&t, hint).color(t.ink_faint)).wrap());
        }
        kit::notice(ui, &t, "Allow runs · ask waits for your approval in the ai sidebar · deny is never offered. the MCP server honours deny only.");
    });
    if changed {
        app.dirty_cfg = true;
    }
}

/// Three rules with a knob each: the settings glyph.
pub fn paint_sliders(p: &egui::Painter, rect: egui::Rect, s: egui::Stroke) {
    let c = rect.center();
    for (k, kx) in [(-6.0, -4.0), (0.0, 4.0), (6.0, -1.0)] {
        p.line_segment([c + egui::vec2(-8.0, k), c + egui::vec2(8.0, k)], s);
        p.rect_filled(egui::Rect::from_center_size(c + egui::vec2(kx, k), egui::vec2(3.0, 5.0)), 0.0, s.color);
    }
}
