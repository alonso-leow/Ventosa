//! Ventosa — content manager for human + AI production.
//! The AI sees the work as a skill graph; the human sees it as a to-do list and
//! the AI's queue. egui only; MetalVJ's component look in purple and orange.

mod access;
mod canvas;
mod connections;
mod chat_ui;
mod docs;
mod harness;
mod launcher;
mod kit;
mod pages;
mod prefs;
mod theme;

use eframe::egui;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use theme::Theme;
use ventosa::config::{self, Config};
use ventosa::queue::Item;
use ventosa::{default_path, graph, load, queue_path, runner, Task};

/// A request to open the AI sidebar with a reference already in the prompt.
/// Set by any page (Ask AI, Edit with AI); the chat sidebar consumes it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatRequest {
    /// `@` reference to put in the prompt, e.g. `@step:options.need` or `@skill:options`.
    pub mention: String,
    /// Workspace the reference belongs to.
    pub workspace: String,
    /// true: start a new chat; false: add to the open chat.
    pub new_chat: bool,
}

/// The nav bar's views. Graph is the node canvas with its focused panel and the
/// graph canvas; the others fill the screen. Speech is
/// the nav bar's last item but no view: it toggles listening, and a small widget
/// shows the inference state.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum View {
    Graph,
    Todo,
    History,
    Apps,
    Connections,
    Ai,
    Access,
    Settings,
    Files,
    Speech,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Page {
    Overview,
    Onboarding,
}

pub struct App {
    pub theme: Theme,
    pub tasks_path: PathBuf,
    pub queue_path: PathBuf,
    pub cfg_path: PathBuf,
    pub tasks: Vec<Task>,
    pub items: Vec<Item>,
    pub cfg: Config,
    stamps: [Option<SystemTime>; 3],
    pub page: Page,
    // to-do
    pub input: String,
    pub todo_group: String,
    // queue ai
    pub queue_group: String,
    pub queue_ws: String,
    /// The queue's time window (1h, 24h, 7d, 30d, all).
    pub queue_window: String,
    // overview
    pub ws_sel: String,
    pub canvas: canvas::Canvas,
    pub states: std::collections::HashMap<String, Value>,
    pub steps_cache: std::collections::HashMap<String, Value>,
    /// Stale nodes per workspace (hash check, every 15 s) and live callbacks (every 3 s).
    pub stale: std::collections::HashMap<String, Vec<ventosa::callback::StaleRow>>,
    pub callbacks: std::collections::HashMap<String, Vec<ventosa::callback::Callback>>,
    pub last_stale: Instant,
    last_state_read: Instant,
    pub intake_title: String,
    pub intake_comment: String,
    pub intake_vars: Vec<(String, String)>,
    pub skill_open: bool,
    // settings / onboarding
    pub new_ws_template: String,
    pub new_ws_dir: String,
    pub new_ws_name: String,
    pub builder: Vec<ventosa::gallery::NodeDef>,
    pub builder_preset: String,
    pub builder_brand: String,
    pub gallery: Vec<ventosa::gallery::NodeDef>,
    pub gallery_source: String,
    /// Graph edit history for undo: (workspace, steps.json text before the edit).
    pub undo: Vec<(String, String)>,
    pub node_clip: Option<serde_json::Value>,
    pub gallery_open: bool,
    /// Narrow windows show one pane of a two-column page: id → showing the right (detail) pane.
    pub narrow_right: std::collections::HashMap<String, bool>,
    pub gallery_tab: String,
    pub flash: Arc<Mutex<String>>,
    pub dirty_cfg: bool,
    // AI sidebar handoff (see ChatRequest)
    pub chat_req: Option<ChatRequest>,
    /// The sidebar is shown (default on).
    pub chat_visible: bool,
    /// A chat with at least one message is open (Edit with AI shows the open/new dropdown).
    pub chat_has_session: bool,
    /// The sidebar's chat state (see chat_ui).
    pub chat: chat_ui::ChatState,
    /// Custom apps of the header's workspace.
    pub custom_apps: chat_ui::AppsCache,
    /// The nav bar's view.
    pub view: View,
    /// The speech instance's chat state (parallel to `chat`).
    pub speech_chat: chat_ui::ChatState,
    pub harness: harness::HarnessState,
    /// A message the AI view sends as soon as it is shown (onboarding's graph request).
    pub pending_send: Option<String>,
    /// The graph view is wide enough for the AI sidebar next to it.
    pub sidebar_fits: bool,
    /// Region splits (ui.json).
    pub prefs: prefs::Prefs,
    /// Who is logged in and what they may do (open mode when there are no users).
    pub access: ventosa::identity::Access,
    pub identity: ventosa::identity::Identity,
    pub login: launcher::LoginState,
    /// The launcher-only app (`ventosa-portal`): never the graph.
    pub launcher_only: bool,
    pub launcher: launcher::LauncherState,
    pub docs: docs::DocsState,
    pub access_ui: access::AccessState,
    /// Last presence beat (unix seconds).
    pub beat_at: u64,
}

fn mtime(p: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl App {
    /// Tell the workspaces who is here and at which level, every 30 s while logged in.
    fn presence(&mut self) {
        let now = ventosa::now();
        if now.saturating_sub(self.beat_at) < 30 {
            return;
        }
        self.beat_at = now;
        let Some(u) = self.access.user.clone() else { return };
        let level = if self.launcher_only || !self.access.can_graph() { "worker" } else { "admin" };
        let doing = if level == "worker" { format!("launcher \u{00b7} {}", match self.launcher.pane { launcher::Pane::Apps => "apps", launcher::Pane::Chat => "chat", launcher::Pane::Docs => "docs" }) } else { format!("graph \u{00b7} {}", self.ws_sel) };
        for ws in self.cfg.workspaces.iter().filter(|w| w.enabled && self.access.sees_workspace(&w.name)) {
            ventosa::hooks::beat(ws, &u.name, level, &doing);
        }
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg_path = config::default_path();
        let cfg = config::load(&cfg_path);
        let theme = Self::theme_for(&cfg);
        theme.apply(&cc.egui_ctx);
        kit::install_icon_font(&cc.egui_ctx);
        let (tp, qp) = (default_path(), queue_path());
        let flash = Arc::new(Mutex::new(String::new()));
        // the launcher-only app (ventosa-portal) runs no sync, executions or bots
        if std::env::var_os("VENTOSA_LAUNCHER_ONLY").is_none() {
            background(flash.clone());
            executor(flash.clone());
            let bot_flash = flash.clone();
            ventosa::bots::run(move |m| {
                if let Ok(mut f) = bot_flash.lock() {
                    *f = m;
                }
            });
        }
        // VENTOSA_PAGE=overview|todo|queue|schedules|apps|settings|onboarding|ai|speech|files opens there.
        let page_env = std::env::var("VENTOSA_PAGE").unwrap_or_default();
        let view = match page_env.as_str() {
            "ai" => View::Ai,
            "todo" => View::Todo,
            "queue" | "history" => View::History,
            "apps" => View::Apps,
            "access" => View::Access,
            "files" => View::Files,
            "settings" | "docs" => View::Settings,
            _ => View::Graph,
        };
        let page = match page_env.as_str() {
            "onboarding" => Page::Onboarding,
            "overview" | "queue" | "history" | "start" | "schedules" | "todo" | "apps" | "ai" | "access" | "docs" | "files" | "settings" => Page::Overview,
            _ if cfg.onboarded => Page::Overview,
            _ => Page::Onboarding,
        };
        let identity = ventosa::identity::load();
        let mut canvas = canvas::Canvas::default();
        match page_env.as_str() {
            "schedules" => canvas.selected = Some("@schedules".into()),
            "start" => canvas.selected = Some("start".into()),
            _ => {}
        }
        let ws_sel = cfg.workspaces.first().map(|w| w.name.clone()).unwrap_or_default();
        let mut harness = harness::HarnessState::default();
        if page_env == "docs" {
            harness.settings_sel = "docs".into();
        }
        Self {
            theme,
            stamps: [mtime(&tp), mtime(&qp), mtime(&cfg_path)],
            tasks: load(&tp),
            items: load(&qp),
            tasks_path: tp,
            queue_path: qp,
            cfg_path,
            cfg,
            page,
            input: String::new(),
            todo_group: "open".into(),
            queue_group: "queued".into(),
            queue_ws: "all".into(),
            queue_window: "7d".into(),
            ws_sel,
            canvas,
            states: Default::default(),
            steps_cache: Default::default(),
            stale: Default::default(),
            callbacks: Default::default(),
            last_stale: Instant::now() - Duration::from_secs(60),
            last_state_read: Instant::now() - Duration::from_secs(60),
            intake_title: String::new(),
            intake_comment: String::new(),
            intake_vars: vec![],
            skill_open: false,
            new_ws_template: "media".into(),
            new_ws_dir: String::new(),
            new_ws_name: String::new(),
            builder: ventosa::gallery::preset("media"),
            builder_preset: "media".into(),
            builder_brand: String::new(),
            gallery: Vec::new(),
            gallery_source: "all".into(),
            undo: vec![],
            node_clip: None,
            gallery_open: false,
            narrow_right: Default::default(),
            gallery_tab: "existing".into(),
            flash,
            dirty_cfg: false,
            chat_req: None,
            chat_visible: true,
            chat_has_session: false,
            chat: chat_ui::ChatState::with_mode("text"),
            custom_apps: Default::default(),
            view,
            speech_chat: chat_ui::ChatState::with_mode("speech"),
            harness,
            pending_send: None,
            sidebar_fits: true,
            prefs: prefs::Prefs::load(),
            access: ventosa::identity::Access::open(),
            identity,
            login: Default::default(),
            launcher_only: false,
            launcher: Default::default(),
            docs: Default::default(),
            access_ui: Default::default(),
            beat_at: 0,
        }
    }

    fn theme_for(cfg: &Config) -> Theme {
        let mut base = Theme::ventosa();
        match theme::hex(&cfg.brand.accent) {
            // the stock red accent from older configs maps to the Ventosa purple
            Some(c) if !cfg.brand.accent.eq_ignore_ascii_case("#ff2e2e") => base = base.with_primary(c),
            _ => {}
        }
        if let Some(c) = theme::hex(&cfg.brand.accent2) {
            base = base.with_secondary(c);
        }
        base
    }

    fn reload(&mut self) {
        let now = [mtime(&self.tasks_path), mtime(&self.queue_path), mtime(&self.cfg_path)];
        if now[0] != self.stamps[0] {
            self.tasks = load(&self.tasks_path);
        }
        if now[1] != self.stamps[1] {
            self.items = load(&self.queue_path);
        }
        if now[2] != self.stamps[2] && !self.dirty_cfg {
            self.cfg = config::load(&self.cfg_path);
        }
        self.stamps = now;
        if self.last_state_read.elapsed() > Duration::from_secs(3) {
            self.last_state_read = Instant::now();
            for ws in &self.cfg.workspaces {
                if let Ok(s) = graph::read_state(ws) {
                    self.states.insert(ws.name.clone(), s);
                }
                if let Ok(s) = graph::steps(ws) {
                    self.steps_cache.insert(ws.name.clone(), s);
                }
                let live: Vec<_> = ventosa::callback::load_callbacks(&graph::root(ws)).into_iter().filter(|c| c.live() || c.status == "failed").rev().take(5).collect();
                self.callbacks.insert(ws.name.clone(), live);
            }
        }
        if self.last_stale.elapsed() > Duration::from_secs(15) {
            self.last_stale = Instant::now();
            for ws in &self.cfg.workspaces {
                let rows = ventosa::callback::stale_ws(ws, None).unwrap_or_default().into_iter().filter(|r| r.stale).collect();
                self.stale.insert(ws.name.clone(), rows);
            }
        }
    }

    pub fn save_tasks(&mut self) {
        if ventosa::save(&self.tasks_path, &self.tasks).is_ok() {
            self.stamps[0] = mtime(&self.tasks_path);
        }
    }

    pub fn save_items(&mut self) {
        if ventosa::save(&self.queue_path, &self.items).is_ok() {
            self.stamps[1] = mtime(&self.queue_path);
        }
    }

    pub fn save_cfg(&mut self) {
        match config::save(&self.cfg_path, &self.cfg) {
            Ok(()) => {
                self.stamps[2] = mtime(&self.cfg_path);
                self.dirty_cfg = false;
                self.set_flash(format!("saved {}", self.cfg_path.display()));
                self.theme = Self::theme_for(&self.cfg);
            }
            Err(e) => self.set_flash(format!("save failed: {e}")),
        }
    }

    pub fn set_flash(&self, msg: String) {
        if let Ok(mut f) = self.flash.lock() {
            *f = msg;
        }
    }

    pub fn sync_now(&mut self) {
        let mut n = 0;
        let mut errs = vec![];
        for ws in self.cfg.workspaces.iter().filter(|w| w.enabled) {
            match graph::sync(ws, &self.queue_path) {
                Ok(c) => n += c,
                Err(e) => errs.push(format!("{}: {e}", ws.name)),
            }
        }
        self.items = load(&self.queue_path);
        self.stamps[1] = mtime(&self.queue_path);
        self.last_state_read = Instant::now() - Duration::from_secs(60);
        self.set_flash(if errs.is_empty() { format!("synced · {n} change(s)") } else { errs.join(" · ") });
    }
}

impl App {
    /// One "open apps" dropdown: the workspace's web pages, the docs, and the
    /// AI-made apps (↗ = opens in the browser).
    fn external_buttons(&mut self, ui: &mut egui::Ui, t: &Theme, narrow: bool) {
        let ws = self.cfg.workspace(&self.ws_sel).cloned().or_else(|| self.cfg.workspaces.first().cloned());
        if let Some(w) = &ws {
            self.custom_apps.refresh(&self.cfg, &w.name);
        }
        let btn = ui.add(egui::Button::new(kit::small_text(t, if narrow { "apps \u{25be}" } else { "open apps \u{25be}" }).color(t.secondary)));
        let btn = btn.on_hover_text("the web apps of the workspace, the docs, and the AI-made apps");
        let mut pick: Option<String> = None;
        egui::Popup::menu(&btn).show(|ui| {
            ui.set_min_width(230.0);
            let row = |ui: &mut egui::Ui, id: &str, label: &str, hint: &str, c: egui::Color32, enabled: bool| -> Option<String> {
                let r = ui.add_enabled(enabled, egui::Button::new(kit::small_text(t, label).color(c)).frame(false).min_size(egui::vec2(ui.available_width(), 22.0)));
                r.on_hover_text(hint).clicked().then(|| id.to_string())
            };
            ui.label(kit::small_text(t, "Docs").color(t.ink_faint));
            pick = pick.take().or(row(ui, "docs", "docs \u{00b7} wiki", "guides: deploy, providers, team setup, integrations (editable)", t.primary_lit, true));
            if self.access.can_graph() {
                pick = pick.take().or(row(ui, "portal", "web portal \u{2197}", "the login page for the web apps (basic auth)", t.primary_lit, true));
            }
            ui.separator();
            ui.label(kit::small_text(t, ws.as_ref().map(|w| format!("apps \u{00b7} {}", w.name)).unwrap_or_else(|| "register a workspace first".into())).color(t.ink_faint));
            let in_graph = ws.as_ref().map(ventosa::graph::app_ids).unwrap_or_default();
            for (id, label, hint) in pages::WEB_APPS {
                if in_graph.iter().any(|a| a == id) && self.access.can_app(id) {
                    let c = if *id == "dashboard" || *id == "uploads" { t.primary_lit } else { t.secondary };
                    pick = pick.take().or(row(ui, id, &format!("{label} \u{2197}"), hint, c, ws.is_some()));
                }
            }
            let custom = self.custom_apps.list.clone();
            if !custom.is_empty() {
                ui.separator();
                ui.label(kit::small_text(t, "Made by the AI").color(t.ink_faint));
                for (def, running) in custom {
                    if self.access.can_app(&def.id) {
                        let hint = format!("serves node {}{}", if def.node.is_empty() { "(none)" } else { def.node.as_str() }, if running { " \u{00b7} running" } else { "" });
                        pick = pick.take().or(row(ui, &format!("custom:{}", def.id), &format!("{} \u{2197}", def.title), &hint, t.primary_lit, true));
                    }
                }
            }
        });
        if let Some(id) = pick {
            if id == "docs" {
                self.view = View::Settings;
                self.harness.settings_sel = "docs".into();
            } else if id == "portal" {
                self.set_flash(pages::open_portal());
            } else if let Some(cid) = id.strip_prefix("custom:") {
                if let Some(w) = ws.clone() {
                    let cid = cid.to_string();
                    self.set_flash(format!("starting {cid}\u{2026}"));
                    std::thread::spawn(move || {
                        if let Ok(url) = ventosa::apps::deploy(&w, &cid) {
                            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                        }
                    });
                }
            } else if let Some(w) = &ws {
                let m = pages::open_app(w, &id, false);
                self.set_flash(m);
            }
        }
        if !narrow {
            ui.add_space(10.0);
            let msg = self.flash.lock().map(|f| f.clone()).unwrap_or_default();
            ui.add(egui::Label::new(kit::small_text(t, msg).color(t.ink_faint)).truncate());
        }
    }
}

/// Sync every enabled workspace and fire due schedules every 20 s.
fn background(flash: Arc<Mutex<String>>) {
    std::thread::spawn(move || loop {
        let cfg = config::load(&config::default_path());
        for ws in cfg.workspaces.iter().filter(|w| w.enabled) {
            if let Err(e) = graph::sync(ws, &queue_path()) {
                if let Ok(mut f) = flash.lock() {
                    *f = format!("sync {}: {e}", ws.name);
                }
            }
        }
        for line in runner::tick(&cfg, false) {
            if let Ok(mut f) = flash.lock() {
                *f = format!("schedule {line}");
            }
        }
        ventosa::batch::prune();
        std::thread::sleep(Duration::from_secs(20));
    });
}

/// Executions: start batch runs every 3 s within the limits.
fn executor(flash: Arc<Mutex<String>>) {
    std::thread::spawn(move || loop {
        let cfg = config::load(&config::default_path());
        for line in ventosa::batch::tick(&cfg) {
            if let Ok(mut f) = flash.lock() {
                *f = line;
            }
        }
        // link-media: every 10 minutes, when a teleprompter ran or the media folders changed
        for line in ventosa::linkmedia::tick(&cfg) {
            if let Ok(mut f) = flash.lock() {
                *f = line;
            }
        }
        // the entry hook: a human entry anywhere (worker launcher, web apps) enables the next AI task
        for line in ventosa::hooks::poll(&cfg, false) {
            if let Ok(mut f) = flash.lock() {
                *f = line;
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.reload();
        self.presence();
        let ctx = ui.ctx().clone();
        self.theme.apply(&ctx);
        ctx.request_repaint_after(Duration::from_millis(700));
        let t = self.theme.clone();

        // login: required as soon as one user exists; people without graph control get the launcher
        if !self.identity.open_mode() && self.access.user.is_none() {
            egui::CentralPanel::default().frame(kit::ground_frame(&t)).show(ui, |ui| launcher::login(self, ui));
            return;
        }
        if self.launcher_only || !self.access.can_graph() {
            harness::speech_tick(self, &ctx);
            egui::CentralPanel::default().frame(kit::ground_frame(&t)).show(ui, |ui| launcher::launcher(self, ui));
            return;
        }

        harness::speech_tick(self, &ctx);
        harness::caution(self, &ctx);
        let full_w = ctx.content_rect().width();
        let onboarding = self.page == Page::Onboarding;

        // the nav bar: graph · to-do · apps · ai · access · docs · settings · files · speech (hidden during onboarding)
        if !onboarding {
            let nav_w = if full_w < 700.0 { 46.0 } else { 70.0 };
            egui::Panel::left("nav")
                .resizable(false)
                .exact_size(nav_w)
                .frame(egui::Frame::new().fill(t.tube_raised).stroke(t.rule_stroke()))
                .show(ui, |ui| harness::navbar(self, ui));
        }

        // the top bar: the wordmark, the apps dropdown, the ai sidebar toggle
        if !matches!(self.view, View::Ai) || onboarding {
            egui::Panel::top("header").frame(kit::header_frame(&t)).show(ui, |ui| {
                let narrow = ui.available_width() < 820.0;
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    kit::wordmark(ui, &t, &self.cfg.brand.name.clone());
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !onboarding && self.sidebar_fits {
                            let label = if self.chat_visible { "ai \u{25b8}" } else { "ai \u{25c2}" };
                            if kit::link(ui, &t, label, t.primary_lit).on_hover_text("show or hide the ai sidebar").clicked() {
                                self.chat_visible = !self.chat_visible;
                            }
                            ui.add_space(8.0);
                        }
                        if !onboarding {
                            if let Some(u) = self.access.user.clone() {
                                if kit::link(ui, &t, &format!("{} \u{00b7} log out", u.name), t.ink_faint).clicked() {
                                    launcher::logout(self);
                                }
                                ui.add_space(6.0);
                            }
                            self.external_buttons(ui, &t, narrow)
                        }
                    });
                });
            });
        }

        // the AI: full screen in its own view; a sidebar next to the graph when the window is wide
        let content_w = full_w - if onboarding { 0.0 } else { 70.0 };
        self.sidebar_fits = content_w >= 1100.0;
        if self.chat_req.is_some() {
            if self.view != View::Ai && self.sidebar_fits {
                self.chat_visible = true;
            } else if self.view != View::Ai {
                self.view = View::Ai;
            }
        }
        if let Some(text) = self.pending_send.take() {
            if self.view == View::Ai {
                chat_ui::submit(self, &ctx, &text);
            } else {
                self.pending_send = Some(text);
            }
        }
        if self.view != View::Ai && !onboarding && self.sidebar_fits && self.chat_visible {
            egui::Panel::right("ai")
                .resizable(true)
                .default_size(360.0)
                .min_size(280.0)
                .frame(egui::Frame::new())
                .show(ui, |ui| chat_ui::sidebar(self, ui));
        }

        egui::CentralPanel::default().frame(kit::ground_frame(&t)).show(ui, |ui| {
            if onboarding {
                return harness::onboarding(self, ui);
            }
            match self.view {
                View::Graph | View::Speech => pages::overview(self, ui),
                View::Todo => pages::todo(self, ui),
                View::History => pages::queue(self, ui),
                View::Apps => pages::apps(self, ui),
                View::Connections => connections::view(self, ui),
                View::Ai => chat_ui::full(self, ui),
                View::Access => access::view(self, ui),
                View::Settings => harness::settings_view(self, ui),
                View::Files => harness::files_view(self, ui),
            }
        });
        harness::speech_widget(self, &ctx);
    }
}

/// Window options shared by `ventosa` and `ventosa-portal`.
pub fn run(launcher_only: bool) -> eframe::Result {
    if launcher_only {
        // set before any thread is spawned
        std::env::set_var("VENTOSA_LAUNCHER_ONLY", "1");
    }
    let (title, id, size) = if launcher_only { ("ventosa launcher", "ventosa-portal", [720.0, 640.0]) } else { ("ventosa", "ventosa", [1180.0, 760.0]) };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(title).with_app_id(id).with_inner_size(size).with_min_inner_size([420.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let mut app = App::new(cc);
            app.launcher_only = launcher_only;
            Ok(Box::new(app))
        }),
    )
}

#[allow(dead_code)]
fn main() -> eframe::Result {
    run(false)
}
