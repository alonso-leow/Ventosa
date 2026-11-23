//! The harness surfaces: the left nav bar (graph · to-do · apps · ai · access ·
//! docs · settings · files · speech), the speech widget, the file explorer, executions (batches),
//! decision cards, the caution popup for missing tools, the settings cards for
//! voice / limits / servers / recommended tools, and the onboarding wizard.

use super::kit::{self, small_text};
use super::{chat_ui, pages, App, ChatRequest, Page, View};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use ventosa::batch::{self, Batch};
use ventosa::config::{self, Server, Workspace};
use ventosa::{graph, recommend, voice};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn take<T>(slot: &mut Option<Slot<T>>) -> Option<Result<T, String>> {
    let r = slot.as_ref().and_then(|s| s.lock().ok().and_then(|mut g| g.take()));
    if r.is_some() {
        *slot = None;
    }
    r
}

fn spawn<T: Send + 'static>(ctx: &egui::Context, f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Slot<T> {
    let slot: Slot<T> = Arc::new(Mutex::new(None));
    let (out, ctx) = (slot.clone(), ctx.clone());
    std::thread::spawn(move || {
        let r = f();
        if let Ok(mut g) = out.lock() {
            *g = Some(r);
        }
        ctx.request_repaint();
    });
    slot
}

// ---------------------------------------------------------------- state

#[derive(Default)]
pub struct SpeechState {
    rec: Option<voice::Recording>,
    stt: Option<Slot<String>>,
    tts: Option<Slot<PathBuf>>,
    player: Option<std::process::Child>,
    pub status: String,
}

impl SpeechState {
    pub fn listening(&self) -> bool {
        self.rec.is_some()
    }
}

#[derive(Default)]
pub struct FilesState {
    ws: String,
    /// Folder shown, relative to the workspace root.
    dir: PathBuf,
    selected: Option<PathBuf>,
    preview: String,
    /// Editing a text file: the working copy, against `preview` (what is on disk).
    editing: bool,
    draft: String,
    folder_q: String,
    file_q: String,
    /// The picked video/audio: (path, info) once probed, and the frame shown.
    media: Option<MediaPrev>,
    /// The folder under the pointer while files are dragged over the view (this frame only).
    drop_hover: Option<String>,
}

/// The inline preview of a picked video or audio file.
pub struct MediaPrev {
    path: PathBuf,
    info: Option<ventosa::preview::Info>,
    at: f64,
    asked: f64,
    frame: Option<PathBuf>,
    probing: Option<Slot<Option<ventosa::preview::Info>>>,
    framing: Option<Slot<PathBuf>>,
}

#[allow(dead_code)]
pub struct ExecState {
    count: usize,
    brief: String,
    runner: String,
    open_log: Option<u64>,
}

impl Default for ExecState {
    fn default() -> Self {
        Self { count: 3, brief: String::new(), runner: String::new(), open_log: None }
    }
}

#[derive(Default)]
pub struct HarnessState {
    pub speech: SpeechState,
    pub files: FilesState,
    pub exec: ExecState,
    pub decision_text: std::collections::HashMap<u64, String>,
    /// The caution popup was answered this session.
    pub caution_done: bool,
    pub onboard_step: usize,
    server_test: Option<Slot<String>>,
    pub limits_open: bool,
    /// When the speech widget last had something to show (it fades out after a few seconds).
    pub speech_seen: Option<f64>,
    /// The settings index pick ("" = the first section).
    pub settings_sel: String,
    /// Path typed in Settings ▸ Plugins.
    pub plugin_path: String,
}

// ---------------------------------------------------------------- nav bar

const NAV: [(View, &str); 10] = [
    (View::Graph, "graph"),
    (View::Todo, "to-do"),
    (View::History, "history"),
    (View::Apps, "apps"),
    (View::Connections, "connect"),
    (View::Files, "files"),
    (View::Ai, "ai"),
    (View::Access, "access"),
    (View::Settings, "settings"),
    (View::Speech, "speech"),
];

/// The left nav bar. Graph first, files last; the speech item glows while listening.
pub fn navbar(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let w = ui.available_width();
    // eight items: shrink them on short windows so speech (last) stays reachable
    let item_h = ((ui.available_height() - 12.0) / NAV.len() as f32).clamp(34.0, if w > 50.0 { 58.0 } else { 46.0 });
    ui.add_space(6.0);
    let time = ui.input(|i| i.time) as f32;
    let open_todo = app.items.iter().filter(|i| i.kind == "todo" && matches!(i.status.as_str(), "queued" | "review")).count() + app.tasks.iter().filter(|x| !x.done).count();
    for (view, label) in NAV {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, item_h), Sense::click());
        let on = app.view == view && view != View::Speech;
        let hot = resp.hovered();
        let listening = view == View::Speech && app.harness.speech.listening();
        let p = ui.painter();
        if on {
            p.rect_filled(rect, 0.0, t.stripe);
            p.rect_filled(Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, t.primary);
        } else if hot {
            p.rect_filled(rect, 0.0, t.hover_card());
        }
        let color = if on || listening { t.primary_lit } else if hot { t.ink } else { t.ink_weak };
        let with_label = w > 50.0 && item_h >= 44.0;
        let icon = Rect::from_center_size(rect.center() - egui::vec2(0.0, if with_label { 7.0 } else { 0.0 }), egui::vec2(18.0, 18.0));
        if view == View::Todo && open_todo > 0 {
            p.circle_filled(icon.right_top() + egui::vec2(2.0, 0.0), 6.5, t.secondary);
            p.text(icon.right_top() + egui::vec2(2.0, 0.0), egui::Align2::CENTER_CENTER, open_todo.min(99).to_string(), egui::FontId::monospace(8.0), Color32::BLACK);
        }
        if listening {
            // MetalVJ's AI highlight, breathing: outer glow ring + inner outline
            let k = 0.5 + 0.5 * (time * 4.0).sin();
            p.rect_stroke(rect.shrink(3.0).expand(2.0 * k), 0.0, t.ai_glow_stroke(), egui::StrokeKind::Outside);
            p.rect_stroke(rect.shrink(3.0), 0.0, t.ai_stroke(), egui::StrokeKind::Inside);
            ui.ctx().request_repaint();
        }
        paint_icon(p, view, icon, Stroke::new(1.4, color));
        if with_label {
            p.text(egui::pos2(rect.center().x, rect.max.y - 10.0), egui::Align2::CENTER_CENTER, label, t.font_small.clone(), color);
        }
        let tip = match view {
            View::Graph => "graph: the node canvas; hover a node for its card",
            View::History => "history: every AI job and run, with status and filters",
            View::Todo => "to-do: your steps, decisions and tasks",
            View::Apps => "apps: the launcher of every workspace",
            View::Connections => "connections: AI, search, MCP and tools as nodes; click to install and wire",
            View::Ai => "ai: the harness chat, full screen",
            View::Access => "access: users, roles, tool grants, trusted sources, bots, audit",
            View::Settings => "settings: brand, AI, voice, tools, servers, workspaces, docs",
            View::Files => "files of the workspace",
            View::Speech => "speech: click to talk, click again to send (a parallel AI instance)",
        };
        if resp.on_hover_text(tip).clicked() {
            if view == View::Speech {
                if app.harness.speech.stt.is_none() {
                    let ctx = ui.ctx().clone();
                    toggle_listen(app, &ctx);
                }
                continue;
            }
            app.view = view;
            if app.page == Page::Onboarding {
                app.page = Page::Overview;
            }
        }
    }
}

fn paint_icon(p: &egui::Painter, view: View, r: Rect, s: Stroke) {
    let c = r.center();
    match view {
        View::Graph => {
            let a = c + egui::vec2(-8.0, -5.0);
            let b = c + egui::vec2(8.0, -5.0);
            let d = c + egui::vec2(0.0, 7.0);
            p.line_segment([a, b], s);
            p.line_segment([a, d], s);
            p.line_segment([b, d], s);
            for q in [a, b, d] {
                p.rect_filled(Rect::from_center_size(q, egui::vec2(6.0, 6.0)), 0.0, s.color);
            }
        }
        View::Ai => {
            // a four-point spark
            for (dx, dy) in [(0.0, -10.0), (10.0, 0.0), (0.0, 10.0), (-10.0, 0.0)] {
                let tip = c + egui::vec2(dx, dy);
                let side = egui::vec2(dy, dx) * 0.22;
                p.line_segment([tip, c + side], s);
                p.line_segment([tip, c - side], s);
            }
        }
        View::Speech => {
            let body = Rect::from_center_size(c - egui::vec2(0.0, 3.0), egui::vec2(8.0, 13.0));
            p.rect_stroke(body, 4.0, s, egui::StrokeKind::Middle);
            let arc: Vec<Pos2> = (0..=12).map(|k| {
                let a = std::f32::consts::PI * (k as f32 / 12.0);
                c + egui::vec2(-8.0 * a.cos(), 1.0 + 7.0 * a.sin())
            }).collect();
            p.add(egui::Shape::line(arc, s));
            p.line_segment([c + egui::vec2(0.0, 8.0), c + egui::vec2(0.0, 11.0)], s);
        }
        View::Settings => {
            // sliders: three rules with a knob each
            for (k, kx) in [(-7.0, -4.0), (0.0, 4.0), (7.0, -1.0)] {
                p.line_segment([c + egui::vec2(-10.0, k), c + egui::vec2(10.0, k)], s);
                p.rect_filled(Rect::from_center_size(c + egui::vec2(kx, k), egui::vec2(4.0, 6.0)), 0.0, s.color);
            }
        }
        View::History => {
            // a clock: face and two hands
            p.circle_stroke(c, 9.0, s);
            p.line_segment([c, c + egui::vec2(0.0, -6.0)], s);
            p.line_segment([c, c + egui::vec2(4.5, 2.5)], s);
        }
        View::Todo => {
            for k in 0..3 {
                let y = c.y - 7.0 + k as f32 * 7.0;
                p.rect_stroke(Rect::from_center_size(egui::pos2(c.x - 7.0, y), egui::vec2(4.0, 4.0)), 0.0, s, egui::StrokeKind::Middle);
                p.line_segment([egui::pos2(c.x - 2.0, y), egui::pos2(c.x + 10.0, y)], s);
            }
        }
        View::Apps => {
            for (dx, dy) in [(-5.0, -5.0), (5.0, -5.0), (-5.0, 5.0), (5.0, 5.0)] {
                p.rect_stroke(Rect::from_center_size(c + egui::vec2(dx, dy), egui::vec2(7.0, 7.0)), 0.0, s, egui::StrokeKind::Middle);
            }
        }
        View::Access => {
            p.circle_stroke(c - egui::vec2(0.0, 5.0), 4.5, s);
            let arc: Vec<Pos2> = (0..=12).map(|k| {
                let a = std::f32::consts::PI * (k as f32 / 12.0);
                c + egui::vec2(-9.0 * a.cos(), 11.0 - 7.0 * a.sin())
            }).collect();
            p.add(egui::Shape::line(arc, s));
        }
        View::Connections => {
            let a = c + egui::vec2(-8.0, 5.0);
            let b = c + egui::vec2(8.0, -5.0);
            p.line_segment([a, b], s);
            p.circle_stroke(a, 3.5, s);
            p.circle_filled(b, 3.5, s.color);
        }
        View::Files => {
            let pts = [c + egui::vec2(-10.0, -7.0), c + egui::vec2(-3.0, -7.0), c + egui::vec2(-1.0, -4.0), c + egui::vec2(10.0, -4.0), c + egui::vec2(10.0, 8.0), c + egui::vec2(-10.0, 8.0)];
            p.add(egui::Shape::closed_line(pts.to_vec(), s));
        }
    }
}

// ---------------------------------------------------------------- speech

/// Runs every frame, whatever view is shown: the speech instance works in parallel.
pub fn speech_tick(app: &mut App, ctx: &egui::Context) {
    let h = &mut app.harness.speech;
    if let Some(r) = take(&mut h.stt) {
        match r {
            Ok(text) => {
                h.status = format!("heard: {text}");
                std::mem::swap(&mut app.chat, &mut app.speech_chat);
                chat_ui::submit(app, ctx, &text);
                std::mem::swap(&mut app.chat, &mut app.speech_chat);
            }
            Err(e) => h.status = format!("could not transcribe: {e}"),
        }
    }
    let h = &mut app.harness.speech;
    if let Some(r) = take(&mut h.tts) {
        match r {
            Ok(path) => match voice::play(&path) {
                Ok(child) => {
                    h.player = Some(child);
                    h.status = "speaking".into();
                }
                Err(e) => h.status = format!("play: {e}"),
            },
            Err(e) => h.status = format!("voice reply: {e}"),
        }
    }
    if let Some(p) = &mut app.harness.speech.player {
        if matches!(p.try_wait(), Ok(Some(_))) {
            app.harness.speech.player = None;
            app.harness.speech.status = "ready".into();
        }
    }
    // the speech chat's engine run: poll it and speak the reply
    std::mem::swap(&mut app.chat, &mut app.speech_chat);
    chat_ui::tick(app);
    let reply = app.chat.last_reply.take();
    std::mem::swap(&mut app.chat, &mut app.speech_chat);
    if let Some(text) = reply {
        if app.cfg.voice.speak_replies {
            let v = app.cfg.voice.clone();
            app.harness.speech.status = "voicing the reply…".into();
            app.harness.speech.tts = Some(spawn(ctx, move || voice::synthesize(&v, &text)));
        }
    }
}

fn toggle_listen(app: &mut App, ctx: &egui::Context) {
    let h = &mut app.harness.speech;
    if let Some(rec) = h.rec.take() {
        h.status = "transcribing…".into();
        let v = app.cfg.voice.clone();
        h.stt = Some(spawn(ctx, move || {
            let path = rec.stop()?;
            voice::transcribe(&v, &path)
        }));
    } else {
        if let Some(mut p) = h.player.take() {
            let _ = p.kill();
        }
        match voice::record() {
            Ok(r) => {
                h.rec = Some(r);
                h.status = "listening… click again (or space) to send".into();
            }
            Err(e) => h.status = e,
        }
    }
}

/// The speech instance has no view: the nav bar's last item toggles listening
/// and this small widget (bottom right) shows the inference state. The speech
/// chats are kept like text chats, but not shown here.
pub fn speech_widget(app: &mut App, ctx: &egui::Context) {
    let t = app.theme.clone();
    let h = &app.harness.speech;
    let listening = h.listening();
    let busy = h.stt.is_some() || app.speech_chat.busy() || h.tts.is_some() || h.player.is_some();
    let recent = app.harness.speech_seen.is_some_and(|at| ctx.input(|i| i.time) - at < 6.0);
    if listening || busy {
        app.harness.speech_seen = Some(ctx.input(|i| i.time));
    }
    if !(listening || busy || recent) {
        return;
    }
    let stage = if listening {
        "listening"
    } else if h.stt.is_some() {
        "transcribing"
    } else if app.speech_chat.busy() {
        "thinking"
    } else if h.tts.is_some() {
        "voicing"
    } else if h.player.is_some() {
        "speaking"
    } else {
        "ready"
    };
    let status = h.status.clone();
    let mut toggle = false;
    egui::Area::new(egui::Id::new("speech-widget"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new().fill(t.panel).stroke(if listening { t.ai_stroke() } else { t.rule_stroke() }).inner_margin(t.panel_margin()).show(ui, |ui| {
                ui.set_width(260.0);
                ui.horizontal(|ui| {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), Sense::click());
                    let time = ui.input(|i| i.time) as f32;
                    let k = 0.5 + 0.5 * (time * 4.0).sin();
                    let p = ui.painter();
                    p.circle_filled(rect.center(), 11.0 + if listening { 2.0 * k } else { 0.0 }, if listening { t.primary } else { t.primary_deep });
                    paint_icon(p, View::Speech, rect.shrink(4.0), Stroke::new(1.4, if listening { Color32::WHITE } else { t.primary_lit }));
                    if resp.on_hover_text(if listening { "send" } else { "talk" }).clicked() {
                        toggle = true;
                    }
                    ui.vertical(|ui| {
                        ui.label(small_text(&t, format!("speech \u{00b7} {stage}")).color(t.primary_lit).strong());
                        let line = if status.is_empty() { "click the mic to talk".to_string() } else { status.clone() };
                        ui.add(egui::Label::new(small_text(&t, line).color(t.ink_weak)).truncate());
                    });
                });
                if busy || listening {
                    ui.ctx().request_repaint();
                }
            });
        });
    if toggle && app.harness.speech.stt.is_none() {
        toggle_listen(app, ctx);
    }
}

// ---------------------------------------------------------------- files

fn is_text(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str(),
        "md" | "txt" | "json" | "toml" | "yaml" | "yml" | "csv" | "vtt" | "srt" | "html" | "css" | "js" | "py" | "rs" | "sh" | "jsonl" | "log" | "kdl")
}

fn is_media(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str(),
        "mp4" | "mov" | "webm" | "mkv" | "mp3" | "wav" | "m4a" | "flac" | "ogg" | "opus")
}

fn row_icon(p: &Path, dir: bool) -> kit::RowIcon {
    if dir {
        return kit::RowIcon::Folder;
    }
    match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "mov" | "webm" | "mkv" => kit::RowIcon::Video,
        "mp3" | "wav" | "m4a" | "flac" | "ogg" | "opus" => kit::RowIcon::Audio,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" => kit::RowIcon::Image,
        "md" | "txt" | "json" | "toml" | "yaml" | "yml" | "csv" | "vtt" | "srt" => kit::RowIcon::Doc,
        _ => kit::RowIcon::File,
    }
}

/// The file explorer, in MetalVJ's library look: a narrow left column of
/// folders (an index: search, the path, `..`, one level), the files of that
/// folder on the right as library rows; a picked file opens its preview
/// under the files.
pub fn files_view(app: &mut App, ui: &mut Ui) {
    app.harness.files.drop_hover = None;
    files_columns(app, ui);
    handle_drop(app, ui);
}

/// Files and folders dropped from the OS are copied into the folder under the pointer (else the
/// folder shown); names that exist get a counter. Files hovering paint a highlight (see the rows).
fn handle_drop(app: &mut App, ui: &mut Ui) {
    let dropped: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
    if dropped.is_empty() {
        return;
    }
    let Some(ws) = app.cfg.workspace(&app.ws_sel).cloned().or_else(|| app.cfg.workspaces.first().cloned()) else { return };
    let f = &app.harness.files;
    let mut dir = graph::root(&ws).join(&f.dir);
    if let Some(sub) = &f.drop_hover {
        dir = dir.join(sub);
    }
    let (mut ok, mut errs) = (0, vec![]);
    for src in &dropped {
        match ventosa::library::copy_into(src, &dir) {
            Ok(_) => ok += 1,
            Err(e) => errs.push(e),
        }
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    app.set_flash(if errs.is_empty() { format!("copied {ok} item(s) into {name}") } else { format!("copied {ok}, failed: {}", errs.join("; ")) });
}

fn files_columns(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let Some(ws) = app.cfg.workspace(&app.ws_sel).cloned().or_else(|| app.cfg.workspaces.first().cloned()) else {
        kit::notice(ui, &t, "No workspace yet");
        return;
    };
    if app.harness.files.ws != ws.name {
        app.harness.files = FilesState { ws: ws.name.clone(), ..Default::default() };
    }
    let root = graph::root(&ws);
    let here = root.join(&app.harness.files.dir);
    let mut entries: Vec<(String, bool, u64)> = std::fs::read_dir(&here)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path().is_dir(), e.metadata().map(|m| m.len()).unwrap_or(0)))
        .collect();
    entries.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    // narrow window: one panel that navigates, folders and files together, with the preview
    if ui.available_width() < 760.0 {
        return files_single(app, ui, &ws, &root, &entries);
    }
    pages::two_columns(ui, app, "files", 0.26, |ui, app| {
        pages::panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                let mut sel = ws.name.clone();
                let opts: Vec<(String, String)> = app.cfg.workspaces.iter().map(|w| (w.name.clone(), w.name.clone())).collect();
                if kit::dropdown(ui, &t, "files-ws", &mut sel, &opts) {
                    app.ws_sel = sel;
                }
                ui.add_space(6.0);
                kit::search_box(ui, &t, &mut app.harness.files.folder_q, "search folder\u{2026}");
            });
            kit::subheader(ui, &t, |ui| {
                let shown = format!("/{}", app.harness.files.dir.display());
                ui.add(egui::Label::new(small_text(&t, shown).color(t.ink_weak)).truncate());
            });
            pages::body(ui, &t, "files-dirs", |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let f = &mut app.harness.files;
                let (dragging, pointer) = ui.ctx().input(|i| (!i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty(), i.pointer.latest_pos().unwrap_or_default()));
                if f.dir.components().next().is_some() && kit::library_row(ui, &t, kit::RowIcon::Up, "..", None, false).clicked() {
                    f.dir.pop();
                    f.selected = None;
                }
                let q = f.folder_q.trim().to_lowercase();
                let mut walk = None;
                for (name, _, _) in entries.iter().filter(|e| e.1).filter(|e| q.is_empty() || e.0.to_lowercase().contains(&q)) {
                    let n = std::fs::read_dir(here.join(name)).map(|d| d.count()).unwrap_or(0);
                    let row = kit::library_row(ui, &t, kit::RowIcon::Folder, name, Some(&format!("{n} item{}", if n == 1 { "" } else { "s" })), false);
                    if row.clicked() {
                        walk = Some(name.clone());
                    }
                    if dragging && row.rect.contains(pointer) {
                        f.drop_hover = Some(name.clone());
                        ui.painter().rect_stroke(row.rect, 0.0, egui::Stroke::new(2.0, t.secondary), egui::StrokeKind::Inside);
                    }
                }
                if let Some(name) = walk {
                    f.dir.push(name);
                    f.selected = None;
                }
                if !entries.iter().any(|e| e.1) {
                    ui.add_space(6.0);
                    ui.label(small_text(&t, "No folders").color(t.ink_faint));
                }
            });
        });
    }, |ui, app| {
        let has_sel = app.harness.files.selected.is_some();
        let files = |ui: &mut Ui, app: &mut App| {
            pages::panel(ui, &t, |ui| {
                kit::panel_head(ui, &t, |ui| {
                    ui.label(RichText::new("Files").font(t.font_heading.clone()).color(t.ink));
                    ui.add_space(6.0);
                    kit::search_box(ui, &t, &mut app.harness.files.file_q, "search file\u{2026}");
                });
                pages::body(ui, &t, "files-list", |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    // files dragged over the list (and not over a folder): they land in this folder
                    if ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) && app.harness.files.drop_hover.is_none() {
                        ui.painter().rect_stroke(ui.max_rect(), 0.0, egui::Stroke::new(2.0, t.secondary), egui::StrokeKind::Inside);
                    }
                    let f = &mut app.harness.files;
                    let q = f.file_q.trim().to_lowercase();
                    let mut open = None;
                    for (name, _, len) in entries.iter().filter(|e| !e.1).filter(|e| q.is_empty() || e.0.to_lowercase().contains(&q)) {
                        let rel = f.dir.join(name);
                        let on = f.selected.as_ref() == Some(&rel);
                        let ext = name.rsplit('.').next().filter(|e| *e != name).unwrap_or("file");
                        if kit::library_row(ui, &t, row_icon(&rel, false), name, Some(&format!("{ext} \u{00b7} {}", human(*len))), on).clicked() {
                            open = Some(rel);
                        }
                    }
                    if !entries.iter().any(|e| !e.1) {
                        kit::notice(ui, &t, "No files in this folder");
                    }
                    if let Some(rel) = open {
                        if f.selected.as_ref() == Some(&rel) {
                            f.selected = None;
                        } else {
                            select_file(f, &root, rel);
                        }
                    }
                });
            });
        };
        let preview = |ui: &mut Ui, app: &mut App| {
            let Some(rel) = app.harness.files.selected.clone() else { return };
            preview_panel(app, ui, &ws, &root, &rel, false);
        };
        if has_sel {
            pages::two_rows(ui, app, "files-preview", 0.5, files, preview);
        } else {
            files(ui, app);
        }
    });
}


fn select_file(f: &mut FilesState, root: &Path, rel: PathBuf) {
    let full = root.join(&rel);
    f.preview = if is_text(&rel) {
        std::fs::read(&full).map(|b| String::from_utf8_lossy(&b[..b.len().min(96 * 1024)]).to_string()).unwrap_or_default()
    } else {
        String::new()
    };
    f.editing = false;
    f.draft.clear();
    f.media = None;
    f.selected = Some(rel);
}

/// Open the Files view in a file's folder with the file selected (the Folder node's button).
pub fn open_file(app: &mut App, ws: &Workspace, rel: &str) {
    let rel = PathBuf::from(rel);
    app.ws_sel = ws.name.clone();
    app.view = View::Files;
    if app.harness.files.ws != ws.name {
        app.harness.files = FilesState { ws: ws.name.clone(), ..Default::default() };
    }
    let root = graph::root(ws);
    let f = &mut app.harness.files;
    f.dir = if root.join(&rel).is_dir() { rel.clone() } else { rel.parent().map(Path::to_path_buf).unwrap_or_default() };
    if root.join(&rel).is_file() {
        select_file(f, &root, rel);
    } else {
        f.selected = None;
    }
}

/// Inline preview of a video or audio file: a frame (the middle by default) with a time slider, the facts, and Play.
fn media_preview(app: &mut App, ui: &mut Ui, full: &Path) {
    let t = app.theme.clone();
    let ctx = ui.ctx().clone();
    let f = &mut app.harness.files;
    if f.media.as_ref().is_none_or(|m| m.path != full) {
        let p = full.to_path_buf();
        f.media = Some(MediaPrev { path: p.clone(), info: None, at: -1.0, asked: -2.0, frame: None, probing: Some(spawn(&ctx, move || Ok(ventosa::preview::probe(&p)))), framing: None });
    }
    let Some(m) = f.media.as_mut() else { return };
    if let Some(Ok(info)) = take(&mut m.probing) {
        if let Some(i) = &info {
            m.at = if i.video { i.duration * 0.5 } else { 0.0 };
        }
        m.info = info;
    }
    if let Some(r) = take(&mut m.framing) {
        if let Ok(p) = r {
            m.frame = Some(p);
        }
    }
    let Some(info) = m.info.clone() else {
        ui.label(small_text(&t, if m.probing.is_some() { "reading the file\u{2026}" } else { "ffprobe could not read this file (is ffmpeg installed?)" }).color(t.ink_faint));
        return;
    };
    kit::field_row(ui, &t, "media", &format!("{:.1}s{}", info.duration, if info.video { format!(" \u{00b7} {}\u{00d7}{}", info.width, info.height) } else { " \u{00b7} audio".into() }));
    if info.video {
        if m.framing.is_none() && (m.asked - m.at).abs() > 0.05 {
            let (p, at) = (m.path.clone(), m.at.max(0.0));
            m.asked = m.at;
            m.framing = Some(spawn(&ctx, move || ventosa::preview::frame(&p, at)));
        }
        if let Some(fr) = &m.frame {
            ui.add(egui::Image::new(format!("file://{}", fr.display())).max_width(ui.available_width()).corner_radius(0.0));
        } else {
            kit::notice(ui, &t, "loading a frame\u{2026}");
        }
        let mut at = m.at;
        ui.horizontal(|ui| {
            ui.label(small_text(&t, format!("{at:.1}s")).color(t.ink_faint));
            ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
            ui.add(egui::Slider::new(&mut at, 0.0..=info.duration.max(0.1)).show_value(false));
        });
        m.at = at;
    }
}

/// The preview of a picked file, in a panel: actions, path, image or text, and for
/// text files an Edit toggle with a Save that lights up while there are unsaved changes.
/// `single` adds a back arrow (the one-panel layout).
fn preview_panel(app: &mut App, ui: &mut Ui, ws: &Workspace, root: &Path, rel: &Path, single: bool) {
    let t = app.theme.clone();
    let full = root.join(rel);
    let size = std::fs::metadata(&full).map(|m| m.len()).unwrap_or(0);
    let editable = is_text(rel) && size <= 96 * 1024;
    pages::panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            if single && kit::link(ui, &t, "\u{2190} back", t.primary_lit).clicked() {
                app.harness.files.selected = None;
                app.harness.files.editing = false;
                return;
            }
            let name = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            ui.add(egui::Label::new(RichText::new(name).font(t.font_heading.clone()).color(t.ink)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !single && kit::link(ui, &t, "\u{00d7}", t.ink_weak).clicked() {
                    app.harness.files.selected = None;
                    app.harness.files.editing = false;
                }
                if editable {
                    let f = &mut app.harness.files;
                    if f.editing {
                        let dirty = f.draft != f.preview;
                        // orange + bold while unsaved, calm green once on disk
                        let label = if dirty { "Save \u{25cf} unsaved" } else { "Saved \u{2713}" };
                        let col = if dirty { t.secondary } else { t.ok };
                        let mut b = egui::Button::new(small_text(&t, label).color(if dirty { egui::Color32::BLACK } else { col }).strong());
                        if dirty {
                            b = b.fill(t.secondary);
                        }
                        if ui.add(b).clicked() && dirty {
                            match std::fs::write(&full, &f.draft) {
                                Ok(()) => {
                                    f.preview = f.draft.clone();
                                    app.set_flash(format!("{} saved", rel.display()));
                                }
                                Err(e) => app.set_flash(format!("save: {e}")),
                            }
                        }
                        let f = &mut app.harness.files;
                        if kit::link(ui, &t, "Close editor", t.ink_weak).clicked() {
                            f.editing = false;
                        }
                    } else if kit::link(ui, &t, "Edit", t.secondary).clicked() {
                        f.editing = true;
                        f.draft = f.preview.clone();
                    }
                }
            });
        });
        pages::body(ui, &t, "files-preview", |ui| {
            kit::label_row(ui, &t, "actions", |ui| {
                let mut labels = vec![small_text(&t, "@ ask AI").color(t.primary_lit), small_text(&t, "Open").color(t.ink)];
                if is_media(rel) {
                    labels.push(small_text(&t, "\u{25b6} preview").color(t.secondary));
                }
                let b = kit::payload_buttons(ui, &t, labels);
                if b[0] {
                    app.chat_req = Some(ChatRequest { mention: format!("@file:{}", rel.display()), workspace: ws.name.clone(), new_chat: false });
                }
                if b[1] {
                    let _ = std::process::Command::new("xdg-open").arg(&full).spawn();
                }
                if b.get(2).copied().unwrap_or(false) {
                    let m = ventosa::preview::play(&full, &rel.display().to_string(), None).unwrap_or_else(|e| e);
                    app.set_flash(m);
                }
            });
            kit::field_row(ui, &t, "path", &full.display().to_string());
            if row_icon(rel, false) == kit::RowIcon::Image {
                ui.add(egui::Image::new(format!("file://{}", full.display())).max_width(ui.available_width()));
            } else if is_media(rel) {
                media_preview(app, ui, &full);
            }
            if app.harness.files.editing {
                let f = &mut app.harness.files;
                let dirty = f.draft != f.preview;
                egui::Frame::new().fill(t.inset).stroke(egui::Stroke::new(if dirty { 1.5 } else { 1.0 }, if dirty { t.secondary } else { t.rule_bright })).inner_margin(t.panel_margin()).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut f.draft).font(t.font_small.clone()).desired_width(f32::INFINITY).desired_rows(18).frame(egui::Frame::NONE));
                });
            } else if !app.harness.files.preview.is_empty() {
                egui::Frame::new().fill(t.inset).stroke(t.rule_stroke()).inner_margin(t.panel_margin()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(&app.harness.files.preview).font(t.font_small.clone()).color(t.ink_weak)).wrap());
                });
            }
        });
    });
}

/// One panel for a narrow window: the folder's folders and files as rows, a click on a
/// folder goes in, on a file shows its preview in the same panel (back returns).
fn files_single(app: &mut App, ui: &mut Ui, ws: &Workspace, root: &Path, entries: &[(String, bool, u64)]) {
    let t = app.theme.clone();
    if let Some(rel) = app.harness.files.selected.clone() {
        return preview_panel(app, ui, ws, root, &rel, true);
    }
    pages::panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            let f = &mut app.harness.files;
            if f.dir.components().next().is_some() && kit::link(ui, &t, "\u{2190}", t.primary_lit).on_hover_text("up one folder").clicked() {
                f.dir.pop();
            }
            ui.add(egui::Label::new(RichText::new(format!("/{}", f.dir.display())).font(t.font_heading.clone()).color(t.ink)).truncate());
        });
        kit::subheader(ui, &t, |ui| {
            kit::search_box(ui, &t, &mut app.harness.files.file_q, "search here\u{2026}");
        });
        pages::body(ui, &t, "files-one", |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let here = root.join(&app.harness.files.dir);
            let q = app.harness.files.file_q.trim().to_lowercase();
            let mut go: Option<(String, bool)> = None;
            // folders first, then files
            for dir_pass in [true, false] {
                for (name, is_dir, len) in entries.iter().filter(|e| e.1 == dir_pass).filter(|e| q.is_empty() || e.0.to_lowercase().contains(&q)) {
                    let sub = if *is_dir {
                        let n = std::fs::read_dir(here.join(name)).map(|d| d.count()).unwrap_or(0);
                        format!("{n} item{}", if n == 1 { "" } else { "s" })
                    } else {
                        let ext = name.rsplit('.').next().filter(|e| e != name).unwrap_or("file");
                        format!("{ext} \u{00b7} {}", human(*len))
                    };
                    if kit::library_row(ui, &t, row_icon(&PathBuf::from(name), *is_dir), name, Some(&sub), false).clicked() {
                        go = Some((name.clone(), *is_dir));
                    }
                }
            }
            if entries.is_empty() {
                kit::notice(ui, &t, "Empty folder");
            }
            if let Some((name, is_dir)) = go {
                let f = &mut app.harness.files;
                if is_dir {
                    f.dir.push(name);
                    f.file_q.clear();
                } else {
                    let rel = f.dir.join(name);
                    select_file(f, root, rel);
                }
            }
        });
    });
}

fn human(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{} KB", n >> 10),
        n => format!("{n} B"),
    }
}

// ---------------------------------------------------------------- executions

/// Overview ▸ executions: start a self-starting graph in a batch, drain the
/// steps that have AI items waiting, and follow the batches.
#[allow(dead_code)]
pub fn executions_card(app: &mut App, ui: &mut Ui, ws: &Workspace) {
    let t = app.theme.clone();
    let batches: Vec<Batch> = batch::list().into_iter().filter(|b| b.workspace == ws.name).collect();
    let active = batches.iter().filter(|b| matches!(b.status.as_str(), "running" | "waiting")).count();
    kit::card_tinted(ui, &t, Some(&format!("executions · {active} active")), t.primary_dim, |ui| {
        let ai_ok = app.cfg.provider.kind != "none";
        let runners = vec![("builtin".to_string(), "built-in AI (provider)".to_string()), ("command".to_string(), "command (schedule's agent)".to_string())];
        if app.harness.exec.runner.is_empty() {
            app.harness.exec.runner = if ai_ok { "builtin".into() } else { "command".into() };
        }
        let first = app.steps_cache.get(&ws.name).and_then(|g| g["steps"].as_array().and_then(|s| s.first().cloned()));
        let self_start = first.as_ref().is_some_and(|s| s["owner"] == "ai");
        if self_start {
            let step = first.as_ref().and_then(|s| s["id"].as_str()).unwrap_or("").to_string();
            kit::text_row(ui, &t, "brief", &mut app.harness.exec.brief, "what the batch is about (titles: brief 1, brief 2…)");
            kit::label_row(ui, &t, "occurrences", |ui| {
                let max = app.cfg.limits.max_batch.max(1);
                kit::drag_cell(ui, &t, egui::DragValue::new(&mut app.harness.exec.count).range(1..=max).suffix(" subjects"));
            });
            kit::label_row(ui, &t, "runner", |ui| {
                kit::dropdown(ui, &t, "exec-runner", &mut app.harness.exec.runner, &runners);
            });
            let go = kit::label_row(ui, &t, "", |ui| kit::ai_button(ui, &t, &format!("\u{25b6} start batch × {} from {step}", app.harness.exec.count)));
            if go.clicked() {
                let b = Batch { workspace: ws.name.clone(), kind: "start".into(), count: app.harness.exec.count, brief: app.harness.exec.brief.clone(), runner: app.harness.exec.runner.clone(), ..Default::default() };
                let m = match batch::create(&app.cfg, b) {
                    Ok(b) => format!("batch {} started · {} subject(s)", b.id, b.subjects.len()),
                    Err(e) => format!("batch: {e}"),
                };
                app.set_flash(m);
                app.sync_now();
            }
        } else if let Some(s) = &first {
            kit::notice(ui, &t, &format!("this graph starts with a human step ({}): add subjects in phase 1; AI steps below run as their items arrive", s["id"].as_str().unwrap_or("")));
        }
        let waiting = batch::waiting(ws, &app.items);
        if waiting.is_empty() {
            kit::label_row(ui, &t, "ready", |ui| {
                ui.label(small_text(&t, "No AI step has items waiting").color(t.ink_faint));
            });
        }
        for (step, n) in waiting {
            let r = kit::label_row_tinted(ui, &t, &step.replace('_', " "), t.primary, |ui| kit::ai_button(ui, &t, &format!("\u{25b6} run {n} waiting")));
            if r.clicked() {
                let b = Batch { workspace: ws.name.clone(), kind: "drain".into(), node: step.clone(), runner: app.harness.exec.runner.clone(), ..Default::default() };
                let m = match batch::create(&app.cfg, b) {
                    Ok(b) => format!("batch {} draining {step}", b.id),
                    Err(e) => format!("batch: {e}"),
                };
                app.set_flash(m);
            }
        }
        if !ai_ok {
            kit::notice(ui, &t, "No AI provider: built-in runs cannot start (settings ▸ ai provider)");
        }
        let l = &app.cfg.limits;
        kit::label_row(ui, &t, "limits", |ui| {
            let txt = format!("{} at once · {}/min · {} retries · backoff {}s", l.concurrency, l.requests_per_minute, l.retries, l.backoff_secs);
            if ui.add(egui::Button::new(small_text(&t, txt).color(t.ink_weak)).frame(false)).on_hover_text("change in settings ▸ limits").clicked() {
                app.view = View::Settings;
            }
        });
        for b in batches.iter().rev().take(6) {
            let tint = match b.status.as_str() { "running" => t.primary, "waiting" => t.secondary, "done" => t.ok, _ => t.ink_faint };
            let what = if b.kind == "start" { format!("{} × {}", b.subjects.len(), if b.brief.is_empty() { "start" } else { &b.brief }) } else { format!("drain {}", b.node) };
            let mut stop = false;
            let mut log = false;
            kit::label_row_tinted(ui, &t, &format!("#{} {}", b.id, b.status), tint, |ui| {
                ui.add(egui::Label::new(small_text(&t, format!("{what} · {} runs · {} failed", b.runs, b.failures)).color(t.ink)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if matches!(b.status.as_str(), "running" | "waiting") && kit::link(ui, &t, "Stop", t.ink_faint).clicked() {
                        stop = true;
                    }
                    if kit::link(ui, &t, "Log", t.ink_weak).clicked() {
                        log = true;
                    }
                });
            });
            if stop {
                let _ = batch::stop(b.id);
            }
            if log {
                app.harness.exec.open_log = if app.harness.exec.open_log == Some(b.id) { None } else { Some(b.id) };
            }
            if app.harness.exec.open_log == Some(b.id) {
                egui::Frame::new().fill(t.inset).inner_margin(t.panel_margin()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for line in b.log.iter().rev().take(14) {
                        ui.add(egui::Label::new(small_text(&t, line).color(t.ink_weak)).wrap());
                    }
                });
            }
        }
    });
}

// ---------------------------------------------------------------- decisions

/// To-do ▸ decisions: what the AI elevated, with its options to pick or a free answer.
pub fn decisions_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let open: Vec<_> = app.items.iter().filter(|i| i.source == "decision" && i.payload["answer"].is_null() && i.status != "done").cloned().collect();
    if open.is_empty() {
        return;
    }
    let mut answered: Option<(u64, String)> = None;
    kit::card_tinted(ui, &t, Some(&format!("decisions for you · {}", open.len())), t.secondary, |ui| {
        for i in &open {
            let d = &i.payload["decision"];
            let risk = d["risk"].as_str().unwrap_or("low");
            let tint = match risk { "dangerous" => t.warn, "edge" => t.secondary, _ => t.primary_lit };
            let head = format!("{}{}", i.payload["workspace"].as_str().unwrap_or(""), i.node.as_deref().map(|n| format!(" · {n}")).unwrap_or_default());
            kit::label_row_tinted(ui, &t, &head, tint, |ui| {
                ui.add(egui::Label::new(small_text(&t, d["question"].as_str().unwrap_or(&i.title)).color(t.ink)).wrap());
            });
            if let Some(ctx) = d["context"].as_str().filter(|c| !c.is_empty()) {
                ui.add(egui::Label::new(small_text(&t, ctx).color(t.ink_weak)).wrap());
            }
            if risk != "low" {
                ui.label(small_text(&t, format!("risk: {risk}")).color(tint));
            }
            ui.horizontal_wrapped(|ui| {
                for (k, opt) in d["options"].as_array().into_iter().flatten().filter_map(|o| o.as_str()).enumerate() {
                    let label = if k == 0 { format!("{opt}  (recommended)") } else { opt.to_string() };
                    if kit::ai_button(ui, &t, &label).clicked() {
                        answered = Some((i.id, opt.to_string()));
                    }
                }
            });
            let text = app.harness.decision_text.entry(i.id).or_default();
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 70.0).max(80.0);
                ui.add_sized([w, 24.0], egui::TextEdit::singleline(text).hint_text("or answer in your words").font(t.font_small.clone()));
                if kit::button(ui, &t, "Answer", Some(t.secondary)).clicked() && !text.trim().is_empty() {
                    answered = Some((i.id, text.trim().to_string()));
                }
            });
            ui.add_space(6.0);
        }
    });
    if let Some((id, a)) = answered {
        let m = ventosa::tools::answer_decision(id, &a).unwrap_or_else(|e| e);
        app.harness.decision_text.remove(&id);
        app.items = ventosa::load(&app.queue_path);
        app.set_flash(m);
    }
}

// ---------------------------------------------------------------- caution popup

/// Missing critical tools: one popup per session, until dismissed for good.
pub fn caution(app: &mut App, ctx: &egui::Context) {
    // VENTOSA_PAGE opens straight on a page (screenshots): no popup over it
    if app.harness.caution_done || !app.cfg.onboarded || app.page == Page::Onboarding || std::env::var_os("VENTOSA_PAGE").is_some() {
        return;
    }
    let missing = recommend::missing_critical(&app.cfg);
    if missing.is_empty() {
        app.harness.caution_done = true;
        return;
    }
    let t = app.theme.clone();
    let modal = egui::Modal::new(egui::Id::new("caution-tools")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new("Caution · missing tools").font(t.font_heading.clone()).color(t.warn));
        ui.label(small_text(&t, "Without these the AI cannot make good videos.").color(t.ink_weak));
        ui.add_space(6.0);
        for r in &missing {
            kit::label_row_tinted(ui, &t, r.name, t.warn, |ui| {
                ui.add(egui::Label::new(small_text(&t, r.what).color(t.ink)).wrap());
            });
            ui.horizontal(|ui| {
                if !r.link.is_empty() {
                    ui.hyperlink_to(small_text(&t, r.link).color(t.primary_lit), r.link);
                }
                if !r.install.is_empty() && kit::link(ui, &t, "Install", t.secondary).on_hover_text(r.install).clicked() {
                    let _ = recommend::run_in_terminal(r.install);
                }
                if r.id == "provider" && kit::link(ui, &t, "Settings", t.secondary).clicked() {
                    app.view = View::Settings;
                }
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if kit::button(ui, &t, "Later", None).clicked() {
                app.harness.caution_done = true;
            }
            // white call to action: runs every install command in one terminal, opens
            // Settings for the provider, and the popup closes
            let cta = ui.add(egui::Button::new(small_text(&t, "Install and configure").color(egui::Color32::WHITE).strong()).stroke(egui::Stroke::new(1.5, egui::Color32::WHITE)));
            if cta.clicked() {
                let cmds: Vec<&str> = missing.iter().map(|r| r.install).filter(|c| !c.is_empty()).collect();
                if !cmds.is_empty() {
                    let _ = recommend::run_in_terminal(&cmds.join(" ; "));
                }
                if missing.iter().any(|r| r.id == "provider") {
                    app.view = View::Settings;
                }
                app.harness.caution_done = true;
            }
        });
    });
    if modal.should_close() {
        app.harness.caution_done = true;
    }
}

// ---------------------------------------------------------------- settings cards

pub fn voice_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("voice (speech mode)"), |ui| {
        let presets = config::voice_presets();
        let opts: Vec<(String, String)> = presets.iter().map(|(l, v)| (v.preset.clone(), l.to_string())).collect();
        let mut pick = app.cfg.voice.preset.clone();
        kit::label_row(ui, &t, "preset", |ui| {
            if kit::dropdown(ui, &t, "voice-preset", &mut pick, &opts) {
                if let Some((_, v)) = presets.iter().find(|(_, v)| v.preset == pick) {
                    app.cfg.voice = v.clone();
                    app.dirty_cfg = true;
                }
            }
        });
        let v = &mut app.cfg.voice;
        let mut c = false;
        let kinds = vec![("openai".to_string(), "OpenAI-compatible".to_string()), ("elevenlabs".to_string(), "ElevenLabs".to_string())];
        kit::label_row(ui, &t, "api", |ui| c |= kit::dropdown(ui, &t, "voice-kind", &mut v.kind, &kinds));
        c |= kit::text_row(ui, &t, "base url", &mut v.base_url, "").changed();
        c |= kit::text_row(ui, &t, "listen model", &mut v.stt_model, "speech-to-text model").changed();
        c |= kit::text_row(ui, &t, "speak model", &mut v.tts_model, "text-to-speech model").changed();
        c |= kit::text_row(ui, &t, "voice", &mut v.voice, "voice name or id").changed();
        c |= kit::text_row(ui, &t, "key env", &mut v.api_key_env, "ELEVENLABS_API_KEY").changed();
        c |= kit::toggle_row(ui, &t, "speak replies", &mut v.speak_replies).changed();
        let set = std::env::var(&v.api_key_env).is_ok_and(|k| !k.is_empty());
        kit::label_row(ui, &t, "key", |ui| {
            ui.label(small_text(&t, if set { "found in environment" } else { "not set — export it before starting ventosa" }).color(if set { t.ok } else { t.warn }));
        });
        if c {
            app.dirty_cfg = true;
        }
    });
}

pub fn limits_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("limits (executions)"), |ui| {
        let l = &mut app.cfg.limits;
        let mut c = false;
        kit::label_row(ui, &t, "at once", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut l.concurrency).range(1..=32).suffix(" runs")).changed());
        kit::label_row(ui, &t, "per minute", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut l.requests_per_minute).range(0..=600).suffix(" starts (0 = no limit)")).changed());
        kit::label_row(ui, &t, "retries", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut l.retries).range(0..=10)).changed());
        kit::label_row(ui, &t, "backoff", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut l.backoff_secs).range(1..=3600).suffix(" s, doubles")).changed());
        kit::label_row(ui, &t, "timeout", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut l.timeout_minutes).range(1..=240).suffix(" min per run")).changed());
        kit::label_row(ui, &t, "max batch", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut l.max_batch).range(1..=1000).suffix(" subjects")).changed());
        if c {
            app.dirty_cfg = true;
        }
    });
}

pub fn servers_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let ctx = ui.ctx().clone();
    if let Some(r) = take(&mut app.harness.server_test) {
        app.set_flash(r.unwrap_or_else(|e| e));
    }
    kit::card(ui, &t, Some("Server \u{00b7} live backend and published apps over SSH"), |ui| {
        kit::notice(ui, &t, "Go live copies the selected workspace and the engine here over SSH and starts the same apps (labelling, teleprompter, media-labelling, dashboard) behind the login, so work goes on with this admin end closed. Sync pushes the graph up and pulls entries down. The key stays in its file or your ssh agent.");
        let mut remove = None;
        let mut test = None;
        let mut live: Option<(usize, Server)> = None;
        for (k, s) in app.cfg.servers.iter_mut().enumerate() {
            let mut c = false;
            ui.push_id(("server", k), |ui| {
                c |= kit::text_row(ui, &t, "name", &mut s.name, "vps").changed();
                c |= kit::text_row(ui, &t, "host", &mut s.host, "203.0.113.10").changed();
                c |= kit::text_row(ui, &t, "user", &mut s.user, "deploy").changed();
                kit::label_row(ui, &t, "port", |ui| c |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut s.port).range(0..=65535)).changed());
                c |= kit::text_row(ui, &t, "key file", &mut s.key_path, "~/.ssh/id_ed25519 (empty = agent)").changed();
                c |= kit::text_row(ui, &t, "remote dir", &mut s.remote_dir, "/var/www/apps").changed();
                c |= kit::text_row(ui, &t, "public url", &mut s.public_url, "https://apps.example.com").changed();
                c |= kit::toggle_row(ui, &t, "AI may use", &mut s.enabled).changed();
                kit::label_row(ui, &t, "", |ui| {
                    let b = kit::payload_buttons(ui, &t, vec![small_text(&t, "Test connection").color(t.primary_lit), small_text(&t, "Remove").color(t.ink_faint)]);
                    if b[0] {
                        test = Some(s.clone());
                    }
                    if b[1] {
                        remove = Some(k);
                    }
                });
                kit::label_row_tinted(ui, &t, "live backend", t.primary, |ui| {
                    let b = kit::payload_buttons(ui, &t, vec![small_text(&t, "Go live").color(t.primary), small_text(&t, "Sync").color(t.primary_lit), small_text(&t, "Status").color(t.ink_weak), small_text(&t, "Stop").color(t.ink_faint)]);
                    for (n, hit) in b.iter().enumerate() {
                        if *hit {
                            live = Some((n, s.clone()));
                        }
                    }
                });
            });
            if c {
                app.dirty_cfg = true;
            }
            ui.add_space(6.0);
        }
        if let Some(k) = remove {
            app.cfg.servers.remove(k);
            app.dirty_cfg = true;
        }
        if let Some(s) = test {
            app.set_flash(format!("testing {}…", s.host));
            app.harness.server_test = Some(spawn(&ctx, move || ventosa::deploy::test(&s)));
        }
        if let Some((n, s)) = live {
            if let Some(ws) = app.cfg.workspace(&app.ws_sel).cloned().or_else(|| app.cfg.workspaces.first().cloned()) {
                app.set_flash(format!("{} on {}\u{2026}", ["going live", "syncing", "checking", "stopping"][n], s.host));
                app.harness.server_test = Some(spawn(&ctx, move || match n {
                    0 => ventosa::deploy::go_live(&ws, &s),
                    1 => ventosa::deploy::sync_live(&ws, &s),
                    2 => ventosa::deploy::live_status(&ws, &s),
                    _ => ventosa::deploy::stop_live(&ws, &s),
                }));
            }
        }
        if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "+ add server").color(t.primary))).clicked() {
            app.cfg.servers.push(Server { name: format!("server{}", app.cfg.servers.len() + 1), port: 22, ..Default::default() });
            app.dirty_cfg = true;
        }
    });
}

/// Recommended tools with their state, links and an install / wire button.
pub fn recommended_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let report = recommend::report(&app.cfg);
    let missing = report.iter().filter(|(_, ok)| !ok).count();
    kit::card(ui, &t, Some(&format!("recommended tools · {missing} missing")), |ui| {
        for (r, ok) in report {
            let tint = if ok { t.ok } else if r.critical { t.warn } else { t.ink_faint };
            let key = format!("{} {}", if ok { "\u{2713}" } else if r.critical { "!" } else { "\u{00b7}" }, r.kind);
            kit::label_row_tinted(ui, &t, &key, tint, |ui| {
                ui.label(small_text(&t, r.name).color(if ok { t.ink } else { tint }));
                ui.add(egui::Label::new(small_text(&t, r.what).color(t.ink_faint)).truncate()).on_hover_text(r.what);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !ok && !r.install.is_empty() && kit::link(ui, &t, if r.kind == "mcp" { "wire" } else { "install" }, t.secondary).on_hover_text(r.install).clicked() {
                        let _ = recommend::run_in_terminal(r.install);
                    }
                    if !r.link.is_empty() {
                        ui.hyperlink_to(small_text(&t, "\u{2197}").color(t.primary_lit), r.link);
                    }
                });
            });
        }
        if !app.cfg.dismissed_tools.is_empty() && kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Warn me again about missing tools").color(t.ink_weak))).clicked() {
            app.cfg.dismissed_tools.clear();
            app.harness.caution_done = false;
            app.dirty_cfg = true;
        }
    });
}

/// The settings index: (id, title, subtitle), grouped.
const SETTINGS: [(&str, &[(&str, &str, &str)]); 3] = [
    ("app", &[
        ("brand", "brand", "name, emphasis colours"),
        ("ai", "AI provider", "model, key env"),
        ("voice", "voice", "speech in, spoken replies"),
        ("permissions", "permissions", "what the AI may do"),
        ("limits", "limits", "spend and rate caps"),
        ("bio", "Global Internal Prompt", "goes into every AI prompt"),
    ]),
    ("connect", &[
        ("tools", "tools", "recommended installs"),
        ("servers", "Server", "live backend over ssh"),
        ("workspaces", "workspaces", "folders and graphs"),
        ("plugins", "plugins", "install from one file"),
    ]),
    ("help", &[
        ("docs", "docs", "the wiki"),
        ("onboarding", "onboarding", "run the wizard again"),
    ]),
];

/// A glyph per settings section, chosen for what the section holds.
fn settings_icon(id: &str) -> kit::RowIcon {
    use kit::RowIcon as I;
    // Lucide codepoints (MetalVJ's icon pack): palette, sparkles, mic, shield-check, …
    match id {
        "brand" => I::Lucide('\u{e1dd}'),       // palette
        "ai" => I::Lucide('\u{e417}'),          // sparkles
        "voice" => I::Lucide('\u{e11c}'),       // mic
        "permissions" => I::Lucide('\u{e1ff}'), // shield-check
        "limits" => I::Lucide('\u{e1bf}'),      // gauge
        "bio" => I::Lucide('\u{e134}'),         // pen-line
        "tools" => I::Lucide('\u{e1b1}'),       // wrench
        "mcp" => I::Lucide('\u{e383}'),         // plug
        "servers" => I::Lucide('\u{e157}'),     // server
        "workspaces" => I::Lucide('\u{e104}'),  // layers
        "plugins" => I::Lucide('\u{e29c}'),    // puzzle
        "docs" => I::Lucide('\u{e063}'),        // book-open
        "onboarding" => I::Lucide('\u{e286}'),  // rocket
        _ => I::Lucide('\u{e0d0}'),             // file-text
    }
}

/// Settings: an index on the left (MetalVJ library rows), the picked section on the right.
pub fn settings_view(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    if app.harness.settings_sel.is_empty() {
        app.harness.settings_sel = "brand".into();
    }
    pages::two_columns(ui, app, "settings", 0.2, |ui, app| {
        pages::panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                ui.label(RichText::new("Settings").font(t.font_heading.clone()).color(t.primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let c = if app.dirty_cfg { t.secondary } else { t.ink_faint };
                    if kit::link(ui, &t, "Save", c).clicked() {
                        app.save_cfg();
                    }
                });
            });
            pages::body(ui, &t, "set-index", |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut pick = None;
                for (k, (group, items)) in SETTINGS.iter().enumerate() {
                    if k > 0 {
                        ui.add_space(8.0);
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(small_text(&t, kit::cap(group)).color(t.ink_faint).strong());
                    });
                    ui.add_space(4.0);
                    for (id, title, sub) in items.iter() {
                        let icon = settings_icon(id);
                        if kit::library_row(ui, &t, icon, title, Some(sub), app.harness.settings_sel == *id).clicked() {
                            pick = Some(*id);
                        }
                    }
                }
                // one section per enabled plugin that declares settings
                let sections: Vec<(String, String)> = ventosa::plugin::list().into_iter().filter(|i| i.state.enabled && !i.plugin.settings.fields.is_empty()).map(|i| (format!("plugin:{}", i.plugin.id), if i.plugin.settings.title.is_empty() { i.plugin.name.clone() } else { i.plugin.settings.title.clone() })).collect();
                let mut plugin_pick: Option<String> = None;
                if !sections.is_empty() {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(small_text(&t, "Plugin settings").color(t.ink_faint).strong());
                    });
                    ui.add_space(4.0);
                    for (id, title) in &sections {
                        if kit::library_row(ui, &t, settings_icon("plugins"), title, Some("by a plugin"), app.harness.settings_sel == *id).clicked() {
                            plugin_pick = Some(id.clone());
                        }
                    }
                }
                if let Some(id) = plugin_pick {
                    app.harness.settings_sel = id;
                    app.narrow_right.insert("settings".into(), true);
                }
                if let Some(id) = pick {
                    if id == "onboarding" {
                        app.harness.onboard_step = 0;
                        app.page = Page::Onboarding;
                        app.view = View::Graph;
                    } else {
                        app.harness.settings_sel = id.into();
                        app.narrow_right.insert("settings".into(), true);
                    }
                }
            });
        });
    }, |ui, app| {
        let sel = app.harness.settings_sel.clone();
        if sel == "docs" {
            return super::docs::view(app, ui);
        }
        let title = SETTINGS.iter().flat_map(|(_, v)| v.iter()).find(|(id, ..)| *id == sel).map(|(_, t, _)| t.to_string()).or_else(|| sel.strip_prefix("plugin:").and_then(|id| ventosa::plugin::list().into_iter().find(|i| i.plugin.id == id)).map(|i| if i.plugin.settings.title.is_empty() { i.plugin.name } else { i.plugin.settings.title })).unwrap_or_else(|| "settings".into());
        pages::panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                ui.label(RichText::new(title).font(t.font_heading.clone()).color(t.ink));
            });
            pages::body(ui, &t, "set-section", |ui| {
                if app.dirty_cfg {
                    kit::notice(ui, &t, "Unsaved changes \u{2014} press save");
                }
                match sel.as_str() {
                    "ai" => pages::provider_card(app, ui),
                    "voice" => voice_card(app, ui),
                    "permissions" => chat_ui::permissions_card(app, ui),
                    "limits" => limits_card(app, ui),
                    "bio" => bio_card(app, ui),
                    "tools" => recommended_card(app, ui),
                    "mcp" => pages::mcp_card(app, ui),
                    "servers" => servers_card(app, ui),
                    "workspaces" => pages::workspace_cards(app, ui),
                    "plugins" => plugins_card(app, ui),
                    s if s.starts_with("plugin:") => plugin_settings_card(app, ui, &s["plugin:".len()..]),
                    _ => pages::brand_card(app, ui),
                }
            });
        });
    });
}

/// Settings ▸ Plugins: install from one file, enable, remove. Humans only: the AI can install but never enable.
fn plugins_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    if !app.access.is_admin() {
        kit::notice(ui, &t, "Only an admin manages plugins");
        return;
    }
    let ctx = ui.ctx().clone();
    // a plugin file dropped on the window fills the path
    if let Some(p) = ctx.input(|i| i.raw.dropped_files.iter().next().map(|f| f.path().to_path_buf())) {
        app.harness.plugin_path = p.display().to_string();
    }
    let actor = app.access.name().to_string();
    kit::card(ui, &t, Some("install a plugin"), |ui| {
        ui.add(egui::Label::new(small_text(&t, "One .json file adds gallery nodes, external apps to Connections, a settings section and (human install only) tools. Drop the file here or type its path. Format and policy: docs/PLUGINS.md").color(t.ink_faint)).wrap());
        kit::text_row(ui, &t, "file", &mut app.harness.plugin_path, "/path/to/plugin.json");
        if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Install").color(t.primary))).clicked() && !app.harness.plugin_path.trim().is_empty() {
            let path = PathBuf::from(app.harness.plugin_path.trim());
            match ventosa::plugin::install_file(&path, &actor, false) {
                Ok(i) => {
                    app.set_flash(format!("{} {} installed and enabled", i.plugin.name, i.plugin.version));
                    app.harness.plugin_path.clear();
                    app.gallery.clear();
                }
                Err(e) => app.set_flash(e.lines().next().unwrap_or("rejected").to_string()),
            }
                    }
    });
    let installed = ventosa::plugin::list();
    if installed.is_empty() {
        kit::notice(ui, &t, "No plugins installed");
    }
    for i in installed {
        let id = i.plugin.id.clone();
        kit::card_tinted(ui, &t, Some(&format!("{} \u{00b7} {}", i.plugin.name, i.plugin.version)), if i.state.enabled { t.ok } else { t.warn }, |ui| {
            if !i.plugin.description.is_empty() {
                ui.add(egui::Label::new(small_text(&t, &i.plugin.description).color(t.ink_weak)).wrap());
            }
            kit::field_row(ui, &t, "adds", &format!("{} nodes \u{00b7} {} apps \u{00b7} {} settings \u{00b7} {} tools", i.plugin.nodes.len(), i.plugin.connections.len(), i.plugin.settings.fields.len(), i.plugin.tools.len()));
            kit::field_row(ui, &t, "permissions", &if i.plugin.permissions.is_empty() { "none".to_string() } else { i.plugin.permissions.join(", ") });
            kit::field_row(ui, &t, "installed by", if i.state.by.is_empty() { "?" } else { &i.state.by });
            let mut on = i.state.enabled;
            if kit::toggle_row(ui, &t, "enabled", &mut on).changed() {
                match ventosa::plugin::set_enabled(&id, on, &actor) {
                    Ok(()) => app.gallery.clear(),
                    Err(e) => app.set_flash(e),
                }
            }
            if i.state.enabled && !i.plugin.tools.is_empty() {
                let go = kit::label_row(ui, &t, "tools", |ui| kit::payload_button(ui, &t, small_text(&t, format!("Add {} to workspace {}", i.plugin.tools.len(), app.ws_sel)).color(t.secondary)));
                if go.clicked() {
                    match app.cfg.workspace(&app.ws_sel).cloned() {
                        Some(ws) => {
                            let m = ventosa::plugin::install_tools(&id, &ws).map(|n| format!("{n} tools added; the AI runs them with tool_run")).unwrap_or_else(|e| e);
                            app.set_flash(m);
                        }
                        None => app.set_flash("pick a workspace first".to_string()),
                    }
                }
            }
            if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Remove").color(t.warn))).clicked() {
                match ventosa::plugin::remove(&id, &actor) {
                    Ok(()) => app.gallery.clear(),
                    Err(e) => app.set_flash(e),
                }
            }
        });
    }
}

/// The settings section a plugin declared: one row per field, saved on change.
fn plugin_settings_card(app: &mut App, ui: &mut Ui, id: &str) {
    let t = app.theme.clone();
    let Some(i) = ventosa::plugin::list().into_iter().find(|i| i.plugin.id == id) else {
        kit::notice(ui, &t, "Plugin not installed");
        return;
    };
    kit::card(ui, &t, Some(&i.plugin.name), |ui| {
        for f in &i.plugin.settings.fields {
            let cur = i.state.values.get(&f.key).cloned().unwrap_or_else(|| f.default.clone());
            let new = match f.kind.as_str() {
                "toggle" => {
                    let mut b = cur.as_bool().unwrap_or(false);
                    kit::toggle_row(ui, &t, &f.label, &mut b).changed().then(|| serde_json::json!(b))
                }
                "choice" => {
                    let mut v = cur.as_str().unwrap_or("").to_string();
                    let opts: Vec<(String, String)> = f.options.iter().map(|o| (o.clone(), o.clone())).collect();
                    kit::label_row(ui, &t, &f.label, |ui| kit::dropdown(ui, &t, ("plugin-field", id, &f.key), &mut v, &opts)).then(|| serde_json::json!(v))
                }
                "number" => {
                    let mut n = cur.as_f64().unwrap_or(0.0);
                    kit::label_row(ui, &t, &f.label, |ui| kit::drag_cell(ui, &t, egui::DragValue::new(&mut n)).changed()).then(|| serde_json::json!(n))
                }
                _ => {
                    let mut v = cur.as_str().unwrap_or("").to_string();
                    kit::text_row(ui, &t, &f.label, &mut v, &f.help).lost_focus().then(|| serde_json::json!(v))
                }
            };
            if let Some(v) = new {
                if let Err(e) = ventosa::plugin::set_value(id, &f.key, v) {
                    app.set_flash(e);
                }
            }
        }
    });
}

fn bio_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("what we make (every AI prompt gets this)"), |ui| {
        let r = ui.add(egui::TextEdit::multiline(&mut app.cfg.bio).desired_rows(3).desired_width(f32::INFINITY).font(t.font_small.clone()).hint_text("e.g. 10-minute explainers about live visuals for VJs, one a week, YouTube + shorts"));
        if r.changed() {
            app.dirty_cfg = true;
        }
    });
}

// ---------------------------------------------------------------- onboarding

const STEPS: [&str; 7] = ["brand", "AI", "voice", "workspace", "tools", "agents", "what we make"];

/// The onboarding wizard: one step at a time with a progress bar. AI is required.
pub fn onboarding(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let step = app.harness.onboard_step.min(STEPS.len() - 1);
    let ai_ok = app.cfg.provider.kind != "none" && !app.cfg.provider.api_key_env.trim().is_empty();
    pages::panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new(format!("welcome · step {} of {} · {}", step + 1, STEPS.len(), STEPS[step])).font(t.font_heading.clone()).color(t.primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ai_ok && kit::link(ui, &t, "Skip the rest", t.ink_faint).clicked() {
                    app.cfg.onboarded = true;
                    app.save_cfg();
                    app.page = Page::Overview;
                }
            });
        });
        // progress bar: one segment per step, done steps filled
        let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::hover());
        let seg = bar.width() / STEPS.len() as f32;
        for (k, name) in STEPS.iter().enumerate() {
            let r = Rect::from_min_size(bar.min + egui::vec2(seg * k as f32 + 1.0, 4.0), egui::vec2(seg - 2.0, 4.0));
            let c = if k < step { t.primary } else if k == step { t.primary_lit } else { t.rule_bright };
            ui.painter().rect_filled(r, 0.0, c);
            ui.painter().text(egui::pos2(r.center().x, r.max.y + 8.0), egui::Align2::CENTER_CENTER, *name, t.font_small.clone(), if k <= step { t.ink_weak } else { t.ink_faint });
        }
        pages::body(ui, &t, "onb", |ui| {
            match step {
                0 => {
                    kit::notice(ui, &t, "How the app and its white-label pages present themselves");
                    pages::brand_card(app, ui);
                }
                1 => {
                    kit::notice(ui, &t, "Ventosa is AI first: every graph step, batch and help needs a provider. Pick one, then export its key and restart Ventosa.");
                    if app.cfg.provider.kind == "none" {
                        kit::card(ui, &t, Some("featured providers"), |ui| {
                            for (name, kind, base, model, env, link) in recommend::featured_providers() {
                                kit::label_row_tinted(ui, &t, kind, t.primary, |ui| {
                                    ui.label(small_text(&t, name).color(t.ink));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if kit::ai_button(ui, &t, "use").clicked() {
                                            let p = &mut app.cfg.provider;
                                            p.kind = kind.into();
                                            p.base_url = base.into();
                                            p.model = model.into();
                                            p.api_key_env = env.into();
                                            app.dirty_cfg = true;
                                        }
                                        ui.hyperlink_to(small_text(&t, "Get a key \u{2197}").color(t.primary_lit), link);
                                    });
                                });
                            }
                        });
                    }
                    pages::provider_card(app, ui);
                    if !ai_ok {
                        ui.label(small_text(&t, "Required: pick a provider to continue").color(t.warn));
                    }
                }
                2 => {
                    kit::notice(ui, &t, "Optional: talk to the AI in speech mode; replies can be spoken");
                    voice_card(app, ui);
                }
                3 => {
                    kit::notice(ui, &t, "Your skill graph, knowledge and media live in a workspace folder");
                    pages::workspace_cards(app, ui);
                }
                4 => {
                    kit::notice(ui, &t, "What the AI needs to make good videos; critical ones are marked !");
                    recommended_card(app, ui);
                }
                5 => {
                    kit::notice(ui, &t, "Your AI clients (Claude Code, OpenCode…) reach Ventosa over MCP; ventosa-install wires them");
                    pages::mcp_card(app, ui);
                }
                _ => {
                    kit::notice(ui, &t, "What type of videos are we making? In your words: format, length, audience, channels, cadence. The AI builds your first graph from it.");
                    let r = ui.add(egui::TextEdit::multiline(&mut app.cfg.bio).desired_rows(6).desired_width(f32::INFINITY).font(t.font_body.clone()).hint_text("e.g. weekly 10-minute explainers about live visuals for VJs, with shorts cut from each; YouTube + Instagram"));
                    if r.changed() {
                        app.dirty_cfg = true;
                    }
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if step > 0 && kit::button(ui, &t, "\u{25c2} back", None).clicked() {
                    app.harness.onboard_step = step - 1;
                }
                let last = step + 1 == STEPS.len();
                let can = step != 1 || ai_ok;
                let label = if last { "finish · build my graph with AI" } else { "next \u{25b8}" };
                if ui.add_enabled_ui(can, |ui| kit::ai_button(ui, &t, label).clicked()).inner {
                    if app.dirty_cfg {
                        app.save_cfg();
                    }
                    if last {
                        finish_onboarding(app);
                    } else {
                        app.harness.onboard_step = step + 1;
                    }
                }
            });
        });
    });
}

/// Save, then start the AI loop that drafts the first graph from the bio.
fn finish_onboarding(app: &mut App) {
    app.cfg.onboarded = true;
    app.save_cfg();
    app.page = Page::Overview;
    let ws = app.cfg.workspace(&app.ws_sel).cloned().or_else(|| app.cfg.workspaces.first().cloned());
    if app.cfg.bio.trim().is_empty() {
        app.view = View::Graph;
        return;
    }
    let target = match &ws {
        Some(w) => format!("workspace {}", w.name),
        None => "a new workspace (none exists yet: tell me which folder to create it in, then build it)".into(),
    };
    app.pending_send = Some(format!(
        "@help Build the initial skill graph for {target} from what we make: \"{}\". \
Read the current graph (graph_steps), then add, remove and edit steps (graph_step_add / graph_step_remove / graph_step_set: owner, need, done_when, after), \
write each step's skill (graph_step_set field skill), and set the elevate rules for this kind of video (graph_elevate_set). \
Mark which steps are human (recording, review, publishing) and which the AI runs. Explain the graph in five lines when done.",
        app.cfg.bio.trim()
    ));
    if let Some(w) = ws {
        app.ws_sel = w.name;
    }
    app.view = View::Ai;
}
