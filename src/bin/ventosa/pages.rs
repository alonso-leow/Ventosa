//! Pages. Each is a panel (head strip + body) built only from `kit` rows, so
//! everything sits on the KEY | PAYLOAD grid like a MetalVJ properties panel.

use super::canvas::{LogPoint, Step};
use super::kit::{self, small_text};
use super::theme::Theme;
use super::{chat_ui, harness, App, ChatRequest, Page, View};
use eframe::egui::{self, Id, RichText, Sense, Ui};
use serde_json::Value;
use std::process::Command;
use ventosa::config::{Schedule, Workspace};
use ventosa::queue::{self, STATUSES};
use ventosa::{add, delete, gallery, graph, hooks, runner, set_done};

const PROVIDERS: [&str; 3] = ["none", "anthropic", "openai-compatible"];

/// Two regions side by side with a draggable divider (the split is remembered
/// per `id` in `ui.json`); under 760 px they stack and scroll, as on a phone.
pub fn two_columns(ui: &mut Ui, app: &mut App, id: &str, left_share: f32, left_fn: impl FnOnce(&mut Ui, &mut App), right_fn: impl FnOnce(&mut Ui, &mut App)) {
    let w = ui.available_width();
    let h = ui.available_height();
    let gap = 8.0;
    if w < 760.0 {
        // one pane at a time, with a switch: the index, or the section it opened
        let right = app.narrow_right.get(id).copied().unwrap_or(false);
        ui.horizontal(|ui| {
            let t = app.theme.clone();
            if kit::tab(ui, &t, !right, "index", t.primary) {
                app.narrow_right.insert(id.to_string(), false);
            }
            if kit::tab(ui, &t, right, "section", t.primary) {
                app.narrow_right.insert(id.to_string(), true);
            }
        });
        let h = ui.available_height();
        ui.allocate_ui_with_layout(egui::vec2(w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(egui::vec2(w, h));
            ui.set_max_width(w);
            if right { right_fn(ui, app) } else { left_fn(ui, app) }
        });
        return;
    }
    let share = app.prefs.split(id, left_share);
    let lw = ((w - gap) * share).clamp(200.0_f32.min(w * 0.5), (w - gap - 220.0).max(200.0));
    let origin = ui.cursor().min;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.allocate_ui_with_layout(egui::vec2(lw, h), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(egui::vec2(lw, h));
            ui.set_max_width(lw);
            left_fn(ui, app)
        });
        let rw = ui.available_width();
        ui.allocate_ui_with_layout(egui::vec2(rw, h), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(egui::vec2(rw, h));
            ui.set_max_width(rw);
            right_fn(ui, app)
        });
    });
    let bar = egui::Rect::from_min_size(egui::pos2(origin.x + lw, origin.y), egui::vec2(gap, h));
    if let Some(s) = divider(ui, app, id, bar, true, w - gap, origin.x) {
        app.prefs.set_split(id, s);
    }
}

/// A region above another, split by a draggable divider (remembered per `id`).
pub fn two_rows(ui: &mut Ui, app: &mut App, id: &str, top_share: f32, top: impl FnOnce(&mut Ui, &mut App), bottom: impl FnOnce(&mut Ui, &mut App)) {
    let w = ui.available_width();
    let h = ui.available_height();
    let gap = 8.0;
    let share = app.prefs.split(id, top_share);
    let th = ((h - gap) * share).clamp(140.0_f32.min(h * 0.5), (h - gap - 120.0).max(140.0));
    let origin = ui.cursor().min;
    ui.allocate_ui_with_layout(egui::vec2(w, th), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_min_size(egui::vec2(w, th));
        ui.set_max_height(th);
        top(ui, app)
    });
    ui.add_space(gap);
    let bh = ui.available_height();
    ui.allocate_ui_with_layout(egui::vec2(w, bh), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_min_size(egui::vec2(w, bh));
        bottom(ui, app)
    });
    let bar = egui::Rect::from_min_size(egui::pos2(origin.x, origin.y + th), egui::vec2(w, gap));
    if let Some(s) = divider(ui, app, id, bar, false, h - gap, origin.y) {
        app.prefs.set_split(id, s);
    }
}

/// The gap between two regions as a grab bar: drag to resize, double-click resets.
/// Returns the new share while dragging (`span` = both regions, `start` = their origin).
fn divider(ui: &mut Ui, app: &mut App, id: &str, bar: egui::Rect, vertical: bool, span: f32, start: f32) -> Option<f32> {
    let r = ui.interact(bar, Id::new(("divider", id)), Sense::click_and_drag());
    let r = r.on_hover_cursor(if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
    let t = &app.theme;
    if r.hovered() || r.dragged() {
        let line = if vertical { egui::Rect::from_center_size(bar.center(), egui::vec2(2.0, bar.height())) } else { egui::Rect::from_center_size(bar.center(), egui::vec2(bar.width(), 2.0)) };
        ui.painter().rect_filled(line, 0.0, t.primary_dim);
    }
    if r.double_clicked() {
        app.prefs.reset_split(id);
        return None;
    }
    if r.drag_stopped() {
        app.prefs.save();
    }
    let p = r.interact_pointer_pos().filter(|_| r.dragged())?;
    let at = if vertical { p.x } else { p.y } - start;
    Some((at / span.max(1.0)).clamp(0.15, 0.85))
}

pub fn panel<R>(ui: &mut Ui, t: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    kit::panel_frame(t)
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        })
        .inner
}

pub fn body<R>(ui: &mut Ui, t: &Theme, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(t.panel_margin())
        .show(ui, |ui| egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, add).inner)
        .inner
}

fn ws_options(app: &App) -> Vec<(String, String)> {
    app.cfg.workspaces.iter().map(|w| (w.name.clone(), w.name.clone())).collect()
}

/// Workspaces whose graph differs from the others': the graph switcher lists only those
/// (and the selected one), and hides itself when every workspace shares one graph.
fn graph_options(app: &App) -> Vec<(String, String)> {
    let mut seen: Vec<String> = vec![];
    let mut out = vec![];
    for w in &app.cfg.workspaces {
        let g = graph::steps(w).map(|v| v["steps"].to_string()).unwrap_or_default();
        if w.name == app.ws_sel || !seen.contains(&g) {
            if !seen.contains(&g) {
                seen.push(g);
            }
            out.push((w.name.clone(), w.name.clone()));
        }
    }
    out
}

fn selected_ws(app: &App) -> Option<Workspace> {
    app.cfg.workspace(&app.ws_sel).cloned().or_else(|| app.cfg.workspaces.first().cloned())
}

/// The white-label web apps `ventosa-app` serves: (id, label, hint).
pub const WEB_APPS: &[(&str, &str, &str)] = &[
    ("dashboard", "dashboard", "uploads per platform, token spend per video, phase KPIs"),
    ("uploads", "uploads", "what is ready to upload, per platform"),
    ("teleprompter", "teleprompter", "record takes from the script"),
    ("labelling", "script labels", "label scripts and thesis options"),
    ("studio", "timeline studio", "label rendered timelines from video files; bulk render + cache"),
    ("media-labelling", "media labelling", "watch each edited result and approve it or send it back"),
];

/// Start the web portal (login + the apps a user may open) and open it.
pub fn open_portal() -> String {
    match Command::new("ventosa-app").arg("portal").spawn() {
        Ok(_) => "opening the web portal".into(),
        Err(e) => format!("ventosa-app: {e}"),
    }
}

pub fn open_app(ws: &Workspace, which: &str, stop: bool) -> String {
    let mut c = Command::new("ventosa-app");
    c.args([which, "-w", &graph::root(ws).display().to_string()]);
    // the dashboard matches the usage log by Ventosa's workspace name
    c.env("VENTOSA_WS_NAME", &ws.name);
    if stop {
        c.arg("--stop");
    }
    match c.spawn() {
        Ok(_) => format!("{} {which} · {}", if stop { "stopping" } else { "opening" }, ws.name),
        Err(e) => format!("ventosa-app: {e}"),
    }
}

/// Which white-label app a human step happens in.
fn app_for_step(step: &str) -> &'static str {
    if step == "record" { "teleprompter" } else { "labelling" }
}

// ---------------------------------------------------------------- overview

pub fn overview(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    if app.cfg.workspaces.is_empty() {
        panel(ui, &t, |ui| {
            body(ui, &t, "ov-empty", |ui| {
                kit::card(ui, &t, Some("no workspace yet"), |ui| {
                    kit::notice(ui, &t, "Create one from a template or register a folder.");
                    if kit::payload_button(ui, &t, small_text(&t, "Open onboarding").color(t.primary)).clicked() {
                        app.page = Page::Onboarding;
                    }
                });
            });
        });
        return;
    }
    let Some(ws) = selected_ws(app) else { return };
    let state = app.states.get(&ws.name).cloned().unwrap_or(Value::Null);
    let steps_json = app.steps_cache.get(&ws.name).cloned().unwrap_or(Value::Null);
    let attached: Vec<(String, String, String)> = state["attached"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| (a["step"].as_str().unwrap_or("").into(), a["vid"].as_str().unwrap_or("").into(), a["agent"].as_str().unwrap_or("").into()))
        .collect();
    let steps: Vec<Step> = steps_json["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["id"] != "done")
        .map(|s| {
            let id = s["id"].as_str().unwrap_or("").to_string();
            Step {
                count: state["queues"][&id].as_array().map(|a| a.len()).unwrap_or(0),
                agents: attached.iter().filter(|a| a.0 == id).map(|a| format!("{} {}", a.2, a.1)).collect(),
                owner: s["owner"].as_str().unwrap_or("human").into(),
                need: s["need"].as_str().unwrap_or("").into(),
                skill_path: s["skill"].as_str().map(|f| graph::root(&ws).join("pipeline/skill-graph").join(f)),
                stale: app.stale.get(&ws.name).is_some_and(|r| r.iter().any(|x| x.node == id)),
                id,
            }
        })
        .collect();
    let mut steps = steps;
    let busy = app.items.iter().filter(|i| i.kind == "ai" && i.payload["workspace"] == ws.name.as_str() && matches!(i.status.as_str(), "queued" | "running" | "review")).count();
    let crons = app.cfg.schedules.iter().filter(|c| c.workspace == ws.name && c.enabled).count();
    steps.insert(0, Step { id: SCHED_SEL.into(), owner: "start".into(), need: "Run AI steps every N minutes".into(), count: crons, agents: vec![], stale: false, skill_path: app.cfg.schedules.iter().find(|j| j.workspace == ws.name).map(|j| runner::cron_skill_path(&j.id)).filter(|p| p.is_file()) });
    steps.insert(0, Step { id: START_SEL.into(), owner: "start".into(), need: "Type the idea, queue the AI".into(), count: busy, agents: vec![], stale: false, skill_path: None });
    let mut logs: Vec<LogPoint> = state["recent_events"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .filter_map(|e| {
            let kind = e["kind"].as_str()?;
            let step = graph::event_step(kind, &e["data"])?;
            Some(LogPoint {
                step,
                ai: e["actor"] == "ai",
                text: format!("{} · {} {}\n{}", e["at"].as_str().unwrap_or(""), kind, e["vid"].as_str().unwrap_or(""), e["data"]),
            })
        })
        .collect();
    for i in app.items.iter().filter(|i| i.payload["workspace"] == ws.name.as_str()) {
        if let Some(node) = &i.node {
            if !graph::run_logs(&ws.name, node, std::slice::from_ref(i)).is_empty() {
                logs.insert(0, LogPoint { step: node.clone(), ai: true, text: format!("run · item {} · {}\n{}", i.id, i.status, i.title) });
            }
        }
    }

    // The graph takes the whole width; the properties of what is selected sit under
    // it at half the height. Nothing selected (a click on empty ground) → no panel.
    let graph = |ui: &mut Ui, app: &mut App| {
        panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                let mut sel = ws.name.clone();
                // only when another workspace has a different graph to switch to
                let options = graph_options(app);
                if options.len() > 1 && kit::dropdown(ui, &t, "ov-ws", &mut sel, &options) {
                    app.ws_sel = sel;
                    app.canvas = Default::default();
                }
                ui.add_space(8.0);
                if ui.available_width() > 520.0 {
                    let hint_w = ui.available_width() - 170.0;
                    ui.add_sized([hint_w, 16.0], egui::Label::new(small_text(&t, "AI steps on the line · to-do below · hover a node for its card · click empty ground to close").color(t.ink_faint)).truncate());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::link(ui, &t, "Sync", t.primary).clicked() {
                        app.sync_now();
                    }
                    let on = app.canvas.selected.as_deref() == Some(WS_SEL);
                    if kit::link(ui, &t, "Workspace \u{25be}", if on { t.primary_lit } else { t.ink_weak }).on_hover_text("workspace, executions and phase 1").clicked() {
                        app.canvas.selected = if on { None } else { Some(WS_SEL.into()) };
                    }
                });
            });
            graph_bar(app, ui, &ws);
            if steps.is_empty() {
                body(ui, &t, "ov-nosteps", |ui| kit::notice(ui, &t, &format!("{}: no pipeline/skill-graph/steps.json yet", ws.name)));
            } else {
                // the zoom and pan come back as the user left them, per workspace
                if app.canvas.view_ws != ws.name {
                    app.canvas.view_ws = ws.name.clone();
                    if let Some(v) = app.prefs.views.get(&ws.name) {
                        app.canvas.zoom = v[0];
                        app.canvas.pan = egui::vec2(v[1], v[2]);
                        app.canvas.fitted = true;
                    }
                }
                if let Some(id) = app.canvas.show(ui, &t, &steps, &logs) {
                    app.canvas.selected = Some(id);
                }
                let now = [app.canvas.zoom, app.canvas.pan.x, app.canvas.pan.y];
                let still = !ui.input(|i| i.pointer.any_down());
                if still && app.prefs.views.get(&ws.name).is_none_or(|v| (v[0] - now[0]).abs() > 0.01 || (v[1] - now[1]).abs() > 1.0 || (v[2] - now[2]).abs() > 1.0) {
                    app.prefs.views.insert(ws.name.clone(), now);
                    app.prefs.save();
                }
            }
            // the manual start lives in its node's hover card
            if let Some((id, r)) = app.canvas.open_embed.clone() {
                if id == START_SEL {
                    let top = r.min + egui::Vec2::new(0.0, (24.0 + 16.0 * 2.0) * app.canvas.zoom);
                    let shown = egui::Area::new(Id::new("start-embed")).order(egui::Order::Foreground).fixed_pos(top).show(ui.ctx(), |ui| {
                        ui.set_width(r.width());
                        start_form(app, ui, &ws);
                    });
                    let h = shown.response.rect.height() / app.canvas.zoom.max(0.1);
                    if (h - app.canvas.embed_h).abs() > 1.0 {
                        app.canvas.embed_h = h;
                        ui.ctx().request_repaint();
                    }
                }
            }
        });
    };
    let props = |ui: &mut Ui, app: &mut App| {
        match app.canvas.selected.as_deref() {
            Some(SCHED_SEL) => return schedules(app, ui),
            Some(START_SEL) => return start_idea(app, ui, &ws),
            _ => {}
        }
        panel(ui, &t, |ui| {
            let sel = app.canvas.selected.clone().unwrap_or_default();
            kit::panel_head(ui, &t, |ui| {
                let title = if sel == WS_SEL { ws.name.clone() } else { super::canvas::action_label(&sel) };
                ui.label(RichText::new(title).font(t.font_heading.clone()).color(t.ink));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if kit::link(ui, &t, "\u{00d7}", t.ink_weak).on_hover_text("close").clicked() {
                        app.canvas.selected = None;
                    }
                });
            });
            body(ui, &t, "ov-props", |ui| {
                if sel == WS_SEL {
                    ws_props(app, ui, &ws, &state, &steps);
                } else {
                    node_props(app, ui, &ws, &state, &sel);
                }
            });
        });
    };
    if app.canvas.selected.is_some() {
        two_rows(ui, app, "overview", 0.5, graph, props);
    } else {
        graph(ui, app);
    }
}

/// The canvas selection that stands for the workspace itself (not a step).
const WS_SEL: &str = "@workspace";
/// Focus targets of the panel under the graph.
pub const SCHED_SEL: &str = "@schedules";
/// The first node: the idea goes in here; its panel merges the AI queue and the schedules.
pub const START_SEL: &str = "start";

fn ago(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// When a schedule fires next: `Some(seconds)` (0 = due now), None when disabled.
fn next_run(s: &Schedule, last: &std::collections::HashMap<String, u64>) -> Option<u64> {
    if !s.enabled {
        return None;
    }
    let at = last.get(&s.id).copied().unwrap_or(0) + s.every_minutes.max(1) * 60;
    Some(at.saturating_sub(ventosa::now()))
}

fn ws_props(app: &mut App, ui: &mut Ui, ws: &Workspace, state: &Value, steps: &[Step]) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("workspace"), |ui| {
        kit::field_row(ui, &t, "root", &graph::root(ws).display().to_string());
        kit::field_row(ui, &t, "engine", &ws.engine);
        kit::field_row(ui, &t, "subjects", &state["videos"].as_object().map(|v| v.len()).unwrap_or(0).to_string());
        kit::field_row(ui, &t, "human news", &state["human_changes_since_ai"].to_string());
        kit::field_row(ui, &t, "computed", state["generated"].as_str().unwrap_or("—"));
    });
    kit::card(ui, &t, Some("where things are"), |ui| {
        for s in steps.iter().filter(|s| s.count > 0) {
            let tint = if s.owner == "ai" { t.primary } else { t.secondary };
            kit::label_row_tinted(ui, &t, &super::canvas::action_label(&s.id), tint, |ui| {
                ui.label(small_text(&t, format!("{} · {}", s.count, if s.owner == "ai" { "AI" } else { "to-do" })).color(tint));
            });
        }
        if steps.iter().all(|s| s.count == 0) {
            kit::notice(ui, &t, "Nothing in flight");
        }
    });
}

/// Remember the graph as it is, so the back arrow can restore it.
fn snapshot(app: &mut App, ws: &Workspace) {
    if let Some(text) = graph::steps_text(ws) {
        if app.undo.last().is_none_or(|(w, t)| w != &ws.name || t != &text) {
            app.undo.push((ws.name.clone(), text));
            if app.undo.len() > 60 {
                app.undo.remove(0);
            }
        }
    }
}

/// The step the canvas has selected, if it is a real graph step (not a start node or the workspace).
fn selected_step(app: &App, steps: &[Step]) -> Option<String> {
    let s = app.canvas.selected.clone()?;
    steps.iter().any(|x| x.id == s && x.owner != "start").then_some(s)
}

/// MetalVJ's graph bars: back arrow, "+" (the gallery), and for the selected node delete,
/// duplicate and copy (paste when something is copied). Keys: Ctrl+Z, Delete, Ctrl+D, Ctrl+C, Ctrl+V.
fn graph_bar(app: &mut App, ui: &mut Ui, ws: &Workspace) {
    let t = app.theme.clone();
    let steps: Vec<Step> = app.steps_cache.get(&ws.name).and_then(|g| g["steps"].as_array().cloned()).unwrap_or_default().iter().filter_map(|s| s["id"].as_str().map(|i| Step { id: i.into(), owner: s["owner"].as_str().unwrap_or("human").into(), need: String::new(), count: 0, agents: vec![], stale: false, skill_path: None })).collect();
    let sel = selected_step(app, &steps);
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let (undo_k, del_k, dup_k, copy_k, paste_k) = ui.input(|i| {
        let c = i.modifiers.command;
        (c && i.key_pressed(egui::Key::Z), i.key_pressed(egui::Key::Delete), c && i.key_pressed(egui::Key::D), c && i.key_pressed(egui::Key::C), c && i.key_pressed(egui::Key::V))
    });
    let mut do_undo = false;
    let mut do_del = false;
    let mut do_dup = false;
    let mut do_copy = false;
    let mut do_paste = false;
    // MetalVJ's tool strip: a floating card of glyph buttons over the canvas corner
    let can_undo = app.undo.iter().any(|(w, _)| w == &ws.name);
    let at = ui.cursor().min + t.item_gap + egui::vec2(2.0, 4.0);
    let mut toggle_gallery = false;
    egui::Area::new(Id::new("graph-tools")).order(egui::Order::Middle).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(t.node_body).stroke(t.rule_bright_stroke()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
            ui.vertical(|ui| {
                if kit::icon_button(ui, &t, '\u{e2a1}', false, can_undo, "undo the last graph edit (Ctrl+Z)").clicked() {
                    do_undo = true;
                }
                if kit::icon_button(ui, &t, '\u{e141}', app.gallery_open, true, "add a node from the gallery").clicked() {
                    toggle_gallery = true;
                }
                if sel.is_some() {
                    if kit::icon_button(ui, &t, '\u{e402}', false, true, "duplicate (Ctrl+D)").clicked() {
                        do_dup = true;
                    }
                    if kit::icon_button(ui, &t, '\u{e0a2}', false, true, "copy (Ctrl+C)").clicked() {
                        do_copy = true;
                    }
                    if kit::icon_button(ui, &t, '\u{e18e}', false, true, "delete (Delete key)").clicked() {
                        do_del = true;
                    }
                }
                if app.node_clip.is_some() && kit::icon_button(ui, &t, '\u{e3ec}', false, true, "paste (Ctrl+V)").clicked() {
                    do_paste = true;
                }
            });
        });
    });
    if toggle_gallery {
        app.gallery_open = !app.gallery_open;
        if app.gallery.is_empty() {
            app.gallery = gallery::all(&app.cfg.workspaces);
        }
    }
    if !typing {
        do_undo |= undo_k;
        do_del |= del_k && sel.is_some();
        do_dup |= dup_k && sel.is_some();
        do_copy |= copy_k && sel.is_some();
        do_paste |= paste_k && app.node_clip.is_some();
    }
    if do_undo {
        if let Some(i) = app.undo.iter().rposition(|(w, _)| w == &ws.name) {
            let (_, text) = app.undo.remove(i);
            match graph::restore_steps(ws, &text) {
                Ok(()) => {
                    invalidate_steps(app, ws);
                    app.set_flash("undone".to_string());
                }
                Err(e) => app.set_flash(format!("undo: {e}")),
            }
        }
    }
    if let Some(id) = sel.clone() {
        if do_copy {
            match graph::copy_step(ws, &id) {
                Ok(c) => {
                    app.node_clip = Some(c);
                    app.set_flash(format!("{id} copied"));
                }
                Err(e) => app.set_flash(e),
            }
        }
        if do_dup {
            snapshot(app, ws);
            match graph::copy_step(ws, &id).and_then(|c| graph::paste_step(ws, &c, Some(&id))) {
                Ok(n) => {
                    invalidate_steps(app, ws);
                    app.canvas.selected = Some(n.clone());
                    app.set_flash(format!("{id} duplicated as {n}"));
                }
                Err(e) => app.set_flash(e),
            }
        }
        if do_del {
            snapshot(app, ws);
            match graph::remove_step(ws, &id) {
                Ok(()) => {
                    invalidate_steps(app, ws);
                    app.canvas.selected = None;
                    app.set_flash(format!("{id} deleted \u{2014} the back arrow restores it"));
                }
                Err(e) => app.set_flash(e),
            }
        }
    }
    if do_paste {
        if let Some(clip) = app.node_clip.clone() {
            snapshot(app, ws);
            match graph::paste_step(ws, &clip, sel.as_deref()) {
                Ok(n) => {
                    invalidate_steps(app, ws);
                    app.canvas.selected = Some(n.clone());
                    app.set_flash(format!("pasted {n}"));
                }
                Err(e) => app.set_flash(format!("paste: {e}")),
            }
        }
    }
    gallery_window(app, ui.ctx(), ws, sel.as_deref());
}

/// The node gallery as a window opened by "+": sections for the nodes of your workspaces and
/// templates, the common production steps and internet research providers. Click a card to add it
/// after the selected node (or before done).
fn gallery_window(app: &mut App, ctx: &egui::Context, ws: &Workspace, after: Option<&str>) {
    if !app.gallery_open {
        return;
    }
    let t = app.theme.clone();
    let mut open = true;
    let mut pick: Option<gallery::NodeDef> = None;
    egui::Window::new("Add a node").open(&mut open).default_width(560.0).collapsible(false).show(ctx, |ui| {
        let tabs = [("existing", "From your graphs"), ("common", "Common steps"), ("research", "Internet research"), ("plugins", "Plugins")];
        ui.horizontal(|ui| {
            for (id, label) in tabs {
                if kit::tab(ui, &t, app.gallery_tab == id, label, t.primary) {
                    app.gallery_tab = id.into();
                }
            }
        });
        ui.separator();
        let defs: Vec<gallery::NodeDef> = match app.gallery_tab.as_str() {
            "research" => ventosa::research::PROVIDERS
                .iter()
                .map(|p| gallery::NodeDef { id: format!("research_{}", p.0), owner: "ai".into(), need: format!("research the subject on the internet ({}) and write sourced notes", p.1), skill: graph::RESEARCH_SKILL.into(), source: p.0.into(), extras: serde_json::Value::Null })
                .collect(),
            "common" => app.gallery.iter().filter(|n| n.source == "common").cloned().collect(),
            "plugins" => app.gallery.iter().filter(|n| n.source.starts_with("plugin:")).cloned().collect(),
            _ => app.gallery.iter().filter(|n| n.source != "common" && !n.source.starts_with("plugin:")).cloned().collect(),
        };
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for node in &defs {
                    let tint = if node.owner == "ai" { t.primary } else { t.secondary };
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(170.0, 64.0), egui::Sense::click());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 0.0, if resp.hovered() { t.hover_card() } else { t.stripe });
                    p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, tint);
                    p.text(rect.min + egui::vec2(10.0, 8.0), egui::Align2::LEFT_TOP, gallery::display_name(&node.id), t.font_body.clone(), tint);
                    let need: String = node.need.chars().take(30).collect();
                    p.text(rect.min + egui::vec2(10.0, 30.0), egui::Align2::LEFT_TOP, need, t.font_small.clone(), t.ink_weak);
                    p.text(rect.min + egui::vec2(10.0, 46.0), egui::Align2::LEFT_TOP, &node.source, t.font_small.clone(), t.ink_faint);
                    if resp.on_hover_text(&node.need).clicked() {
                        pick = Some(node.clone());
                    }
                }
            });
            if defs.is_empty() {
                kit::notice(ui, &t, "Nothing here");
            }
        });
    });
    if !open {
        app.gallery_open = false;
    }
    if let Some(n) = pick {
        snapshot(app, ws);
        let before = after.and_then(|a| {
            let g = graph::steps(ws).ok()?;
            let ids: Vec<String> = g["steps"].as_array()?.iter().filter_map(|s| s["id"].as_str().map(String::from)).collect();
            let p = ids.iter().position(|x| x == a)?;
            ids.get(p + 1).cloned()
        });
        let r = graph::add_def(ws, &n.id, &n.owner, &n.need, &n.skill, before.as_deref()).and_then(|id| {
            if let Some(prov) = n.source.strip_prefix("").filter(|_| n.id.starts_with("research_")) {
                let p = prov.to_string();
                for (f, v) in [("search_provider", p), ("search_query", "{title}".to_string()), ("search_max", "8".to_string())] {
                    graph::set_step_field(ws, &id, f, &serde_json::json!(v))?;
                }
                graph::mark_research(ws, &id)?;
            }
            graph::set_extras(ws, &id, &n.extras)?;
            Ok(id)
        });
        match r {
            Ok(id) => {
                invalidate_steps(app, ws);
                app.canvas.selected = Some(id.clone());
                app.set_flash(format!("{id} added"));
            }
            Err(e) => app.set_flash(format!("add node: {e}")),
        }
    }
}

/// Open the AI sidebar with a reference in the prompt.
fn request_chat(app: &mut App, ws: &Workspace, mention: String, new_chat: bool) {
    app.chat_req = Some(ChatRequest { mention, workspace: ws.name.clone(), new_chat });
    app.chat_visible = true;
    if !app.sidebar_fits {
        app.view = View::Ai;
    }
}

/// Re-read a workspace's steps so the canvas and props show the saved edit now.
fn invalidate_steps(app: &mut App, ws: &Workspace) {
    app.steps_cache.remove(&ws.name);
    if let Ok(s) = graph::steps(ws) {
        app.steps_cache.insert(ws.name.clone(), s);
    }
}

/// A single-line field. Returns the new value when it is committed (Enter or focus lost).
fn edit_field(ui: &mut Ui, t: &Theme, id: Id, current: &str) -> Option<String> {
    let mut buf = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY));
    if r.lost_focus() && buf.trim() != current {
        ui.data_mut(|d| d.remove::<String>(id));
        return Some(buf.trim().to_string());
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    None
}

/// A derived value shown as copyable text (click copies).
fn copy_text(ui: &mut Ui, t: &Theme, text: &str) {
    let shown = if text.is_empty() { "\u{2014}" } else { text };
    let r = ui.add(egui::Label::new(small_text(t, shown).color(t.primary_lit)).truncate().sense(Sense::click())).on_hover_text("click to copy");
    if r.clicked() && !text.is_empty() {
        ui.ctx().copy_text(text.to_string());
    }
}

fn node_props(app: &mut App, ui: &mut Ui, ws: &Workspace, state: &Value, node: &str) {
    let t = app.theme.clone();
    let info = graph::node(ws, node).unwrap_or(Value::Null);
    let ai = info["owner"] == "ai";
    let tint = if ai { t.primary } else { t.secondary };
    let local = ws.engine == "local";
    // (field, value) to save; (field) asked of the AI
    let mut save: Option<(&str, String)> = None;
    let mut ask: Option<&str> = None;
    kit::card(ui, &t, Some("step"), |ui| {
        if local {
            ui.label(small_text(&t, "Edits the workspace's own graph").color(t.ink_faint));
        }
        // AI nodes pick the model they run on (the owner is implicit: AI nodes are AI,
        // the rest are to-dos); the input kind says what the value given before counts as
        if ai {
            let mut model = info["model"].as_str().unwrap_or("").to_string();
            let changed = kit::label_row_tinted(ui, &t, "model", tint, |ui| kit::model_picker(ui, &t, &app.cfg, &mut model));
            if changed {
                save = Some(("model", model));
            }
            let kinds: Vec<(String, String)> = graph::INPUT_KINDS.iter().map(|(k, m)| (k.to_string(), if k.is_empty() { "automatic \u{2014} by the subject's state".to_string() } else { format!("{k} \u{2014} {}", m.split(':').next().unwrap_or(m)) })).collect();
            let mut input = info["input"].as_str().unwrap_or("").to_string();
            let c = kit::label_row_tinted(ui, &t, "input is", tint, |ui| kit::dropdown(ui, &t, ("node-input", node), &mut input, &kinds));
            if c {
                save = Some(("input", input));
            }
        }
        if info["kind"] == "research" {
            let provs: Vec<(String, String)> = ventosa::research::PROVIDERS.iter().map(|p| (p.0.to_string(), p.1.to_string())).collect();
            let mut prov = info["search"]["provider"].as_str().unwrap_or("tavily").to_string();
            let c = kit::label_row_tinted(ui, &t, "search with", tint, |ui| kit::dropdown(ui, &t, ("node-search", node), &mut prov, &provs));
            if c {
                save = Some(("search_provider", prov.clone()));
            }
            let env = info["search"]["key_env"].as_str().unwrap_or("").to_string();
            let hint = ventosa::research::default_key_env(&prov);
            let shown = if env.is_empty() { hint.to_string() } else { env.clone() };
            let edited = kit::label_row_tinted(ui, &t, "key env", tint, |ui| edit_field(ui, &t, Id::new(("edit", node, "key_env")), &shown));
            if let Some(v) = edited {
                save = Some(("search_key_env", v));
            }
            let key_set = std::env::var(&shown).is_ok_and(|v| !v.is_empty());
            if !shown.is_empty() {
                kit::label_row(ui, &t, "key", |ui| {
                    ui.label(small_text(&t, if key_set { "found in environment" } else { "not set \u{2014} export it before starting ventosa" }).color(if key_set { t.ok } else { t.warn }));
                });
            }
            let query = info["search"]["query"].as_str().unwrap_or("{title}").to_string();
            let edited = kit::label_row_tinted(ui, &t, "query", tint, |ui| edit_field(ui, &t, Id::new(("edit", node, "query")), &query));
            if let Some(v) = edited {
                save = Some(("search_query", v));
            }
        }

        if info["kind"] == "folder" {
            let path = info["path"].as_str().unwrap_or("").to_string();
            let edited = kit::label_row_tinted(ui, &t, "path", tint, |ui| edit_field(ui, &t, Id::new(("edit", node, "path")), &path));
            if let Some(v) = edited {
                match graph::set_extras(ws, node, &serde_json::json!({"path": v})) {
                    Ok(()) => invalidate_steps(app, ws),
                    Err(e) => app.set_flash(e),
                }
            }
            let go = kit::label_row_tinted(ui, &t, "", tint, |ui| kit::payload_button(ui, &t, small_text(&t, "Open in Files").color(t.primary)));
            if go.clicked() {
                // <ID> = the subject waiting at this step (else the first subject)
                let subject = app.states.get(&ws.name).and_then(|st| st["videos"].as_object().and_then(|v| v.iter().find(|(_, x)| x["stage"] == node).or_else(|| v.iter().next()).map(|(k, _)| k.clone()))).unwrap_or_default();
                harness::open_file(app, ws, &path.replace("<ID>", &subject));
            }
        }
        if info["per"].as_str().is_some_and(|p| !p.is_empty()) {
            kit::label_row_tinted(ui, &t, "runs per", tint, |ui| ui.label(small_text(&t, info["per"].as_str().unwrap_or("")).color(t.ink)));
        }
        if let Some(a) = info["app"].as_str().filter(|a| !a.is_empty()) {
            let go = kit::label_row_tinted(ui, &t, "app", tint, |ui| kit::payload_button(ui, &t, small_text(&t, format!("open {a} \u{2197}")).color(t.secondary)));
            if go.clicked() {
                let id = ventosa::graph::app_ids(ws).into_iter().find(|x| a.contains(x.as_str()) || x == a).unwrap_or_else(|| a.to_string());
                let m = open_app(ws, &id, false);
                app.set_flash(m);
            }
        }

        let need = info["need"].as_str().unwrap_or("").to_string();
        let (asked, edited) = kit::label_row_tinted(ui, &t, "need", tint, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let asked = kit::ai_button(ui, &t, "Ask AI").clicked();
                (asked, edit_field(ui, &t, Id::new(("edit", node, "need")), &need))
            })
            .inner
        });
        if asked {
            ask = Some("need");
        }
        if let Some(v) = edited {
            save = Some(("need", v));
        }

        let done = info["done_when"].as_str().unwrap_or("").to_string();
        let (asked, edited) = kit::label_row_tinted(ui, &t, "done when", tint, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let asked = kit::ai_button(ui, &t, "Ask AI").clicked();
                (asked, edit_field(ui, &t, Id::new(("edit", node, "done_when")), &done))
            })
            .inner
        });
        if asked {
            ask = Some("done_when");
        }
        if let Some(v) = edited {
            save = Some(("done_when", v));
        }

        if ai {
            let attach = info["attach"].as_str().unwrap_or("").to_string();
            let asked = kit::label_row(ui, &t, "attach", |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let asked = kit::ai_button(ui, &t, "Ask AI").clicked();
                    copy_text(ui, &t, &attach);
                    asked
                })
                .inner
            });
            if asked {
                ask = Some("attach");
            }
        }
        let write = info["write"].as_str().unwrap_or("").to_string();
        let asked = kit::label_row(ui, &t, "write", |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let asked = kit::ai_button(ui, &t, "Ask AI").clicked();
                copy_text(ui, &t, &write);
                asked
            })
            .inner
        });
        if asked {
            ask = Some("write");
        }
        if !ai {
            let which = app_for_step(node);
            let open = kit::label_row_tinted(ui, &t, "app", t.secondary, |ui| kit::payload_button(ui, &t, small_text(&t, format!("open {which}")).color(t.secondary)));
            if open.clicked() {
                let m = open_app(ws, which, false);
                app.set_flash(m);
            }
        }
    });
    if let Some(field) = ask {
        request_chat(app, ws, format!("@step:{node}.{field}"), false);
    }
    if let Some((field, value)) = save {
        snapshot(app, ws);
        match graph::set_step_field(ws, node, field, &serde_json::json!(value)) {
            Ok(()) => {
                invalidate_steps(app, ws);
                app.set_flash(format!("{node} · {field} saved"));
            }
            Err(e) => app.set_flash(format!("save: {e}")),
        }
    }
    kit::card(ui, &t, Some("subjects here"), |ui| {
        let mut any = false;
        for vid in state["queues"][node].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
            any = true;
            let v = &state["videos"][vid];
            let text = format!("{} — {}", v["title"].as_str().unwrap_or(""), v["detail"].as_str().filter(|d| !d.is_empty()).unwrap_or(""));
            kit::label_row_tinted(ui, &t, vid, tint, |ui| {
                ui.add(egui::Label::new(small_text(&t, text).color(t.ink)).truncate());
            });
        }
        if !any {
            kit::notice(ui, &t, "None");
        }
    });
    kit::card(ui, &t, Some("logs"), |ui| {
        let mut n = 0;
        for e in state["recent_events"].as_array().into_iter().flatten().rev() {
            if e["kind"].as_str().and_then(|k| graph::event_step(k, &e["data"])).as_deref() != Some(node) {
                continue;
            }
            n += 1;
            let at = e["at"].as_str().unwrap_or("");
            let key = at.get(5..16).unwrap_or(at).replace('T', " ");
            let c = if e["actor"] == "ai" { t.primary_lit } else { t.secondary };
            kit::label_row_tinted(ui, &t, &key, c, |ui| {
                ui.add(egui::Label::new(small_text(&t, format!("{} {}", e["kind"].as_str().unwrap_or(""), e["vid"].as_str().unwrap_or(""))).color(c)).truncate());
            });
            if n >= 12 {
                break;
            }
        }
        for (_, label, path) in graph::run_logs(&ws.name, node, &app.items).into_iter().take(6) {
            kit::label_row(ui, &t, "run", |ui| {
                if kit::payload_button(ui, &t, small_text(&t, &label).color(t.primary_lit)).on_hover_text(path.display().to_string()).clicked() {
                    let _ = Command::new("xdg-open").arg(&path).spawn();
                }
            });
            n += 1;
        }
        if n == 0 {
            kit::notice(ui, &t, "No events yet");
        }
    });
    node_media(app, ui, ws, node);
    node_library_button(app, ui, ws, node);
    callback_card(app, ui, ws, node);
    let skill = info["skill"].as_str().unwrap_or("").to_string();
    if !skill.is_empty() {
        skill_section(app, ui, ws, node, &skill);
    }
}

/// `open library`: the Files view with this node's folder open (for nodes that work in a folder).
fn node_library_button(app: &mut App, ui: &mut Ui, ws: &Workspace, node: &str) {
    let Some(folder) = graph::node_folder(ws, node) else { return };
    let t = app.theme.clone();
    let mut open = false;
    kit::card(ui, &t, Some("library"), |ui| {
        open = kit::label_row(ui, &t, "folder", |ui| kit::payload_button(ui, &t, small_text(&t, format!("open library \u{00b7} {folder}")).color(t.primary_lit))).clicked();
    });
    if open {
        harness::open_file(app, ws, &folder);
    }
}

/// Go back to this node: stale reasons, the plan (dry run), fast or full, and the callbacks running.
fn callback_card(app: &mut App, ui: &mut Ui, ws: &Workspace, node: &str) {
    let t = app.theme.clone();
    let subjects: Vec<String> = app.states.get(&ws.name).and_then(|s| s["videos"].as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default();
    let stale: Vec<ventosa::callback::StaleRow> = app.stale.get(&ws.name).cloned().unwrap_or_default().into_iter().filter(|r| r.node == node).collect();
    let live = app.callbacks.get(&ws.name).cloned().unwrap_or_default();
    if subjects.is_empty() && live.is_empty() {
        return;
    }
    let id = Id::new(("callback", &ws.name, node));
    let (mut subject, mut full, mut plan): (String, bool, String) = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| (stale.first().map(|r| r.subject.clone()).or_else(|| subjects.first().cloned()).unwrap_or_default(), false, String::new()));
    let (mut dry, mut run, mut stop) = (false, false, None);
    kit::card(ui, &t, Some("callback"), |ui| {
        for r in &stale {
            kit::label_row_tinted(ui, &t, &format!("stale {}", r.subject), t.secondary, |ui| {
                ui.add(egui::Label::new(small_text(&t, r.reasons.join("; ")).color(t.secondary)).truncate());
            });
        }
        for c in &live {
            kit::label_row(ui, &t, "running", |ui| {
                ui.add(egui::Label::new(small_text(&t, format!("{}{}", c.summary(), if c.note.is_empty() { String::new() } else { format!(" \u{00b7} {}", c.note) })).color(t.ink)).truncate());
                if c.live() && kit::payload_button(ui, &t, small_text(&t, "Stop").color(t.secondary)).clicked() {
                    stop = Some(c.id);
                }
            });
        }
        kit::label_row(ui, &t, "go back here", |ui| {
            let opts: Vec<(String, String)> = subjects.iter().map(|s| (s.clone(), s.clone())).collect();
            kit::dropdown(ui, &t, ("cb-subject", node), &mut subject, &opts);
            let mode = vec![(false, "fast".to_string()), (true, "full".to_string())];
            kit::dropdown(ui, &t, ("cb-mode", node), &mut full, &mode);
            dry = kit::payload_button(ui, &t, small_text(&t, "Plan").color(t.primary_lit)).clicked();
            run = kit::payload_button(ui, &t, small_text(&t, "Run").color(t.secondary)).clicked();
        });
        if !plan.is_empty() {
            ui.add(egui::Label::new(small_text(&t, plan.clone()).color(t.ink_weak)).wrap());
        }
    });
    let mode = if full { "full" } else { "fast" };
    if dry || run {
        let args = ventosa::callback::StartArgs { subject: &subject, node, mode, reason: "started from the graph", caller: None, dry_run: dry };
        match ventosa::callback::start_ws(ws, &args, &app.cfg.limits) {
            Ok(cb) => {
                plan = cb.plan.iter().map(|p| format!("{} {}", p.node, p.action)).collect::<Vec<_>>().join(" \u{2192} ");
                if run {
                    app.set_flash(cb.summary());
                }
            }
            Err(e) => plan = e,
        }
    }
    if let Some(cid) = stop {
        let m = match ventosa::callback::stop(&graph::root(ws), cid, ventosa::now()) {
            Ok(c) => format!("stopped {}", c.summary()),
            Err(e) => e,
        };
        app.set_flash(m);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, (subject, full, plan)));
}

/// The media this node made, replayable here: the files of its finished runs, newest first, and
/// the app that approves them.
fn node_media(app: &mut App, ui: &mut Ui, ws: &Workspace, node: &str) {
    let t = app.theme.clone();
    let rows: Vec<graph::Completed> = graph::completed(ws, 60).into_iter().filter(|c| c.step == node).take(4).collect();
    if rows.is_empty() {
        return;
    }
    let mut flash = None;
    let tmap = ventosa::tplog::load(ws);
    kit::card(ui, &t, Some("media"), |ui| {
        for c in &rows {
            for (rel, _) in &c.files {
                let full = graph::root(ws).join(rel);
                let media = full.extension().and_then(|e| e.to_str()).is_some_and(|e| MEDIA_EXT.contains(&e.to_ascii_lowercase().as_str()));
                let name = std::path::Path::new(rel).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                kit::label_row(ui, &t, &c.vid, |ui| {
                    let b = kit::payload_buttons(ui, &t, vec![small_text(&t, name).color(t.ink), small_text(&t, if media { "\u{25b6} play" } else { "Open" }).color(t.primary_lit)]);
                    if b[1] {
                        if media {
                            flash = Some(ventosa::preview::play(&full, &format!("{} {}", c.vid, node), None).unwrap_or_else(|e| e));
                        } else {
                            let _ = Command::new("xdg-open").arg(&full).spawn();
                        }
                    }
                });
                if let Some(m) = tmap.matches.iter().find(|m| m.file == full.display().to_string() && m.method != "unmatched") {
                    kit::label_row(ui, &t, "read", |ui| {
                        ui.label(small_text(&t, format!("{} {} ({:.0}%, {})", m.line, m.text.chars().take(70).collect::<String>(), m.confidence * 100.0, m.method)).color(t.ink));
                    });
                }
            }
        }
        if kit::label_row(ui, &t, "approve in", |ui| kit::payload_button(ui, &t, small_text(&t, "open media-labelling").color(t.secondary))).clicked() {
            flash = Some(open_app(ws, "media-labelling", false));
        }
    });
    if let Some(m) = flash {
        app.set_flash(m);
    }
}

/// The node's skill: a collapsible card with Edit with AI in its header, and a
/// code editor that shows Save only when the text changed.
fn skill_section(app: &mut App, ui: &mut Ui, ws: &Workspace, node: &str, skill: &str) {
    let t = app.theme.clone();
    let key = Id::new(("skill-draft", ws.name.as_str(), node));
    let mut draft = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| skill.to_string());
    let mut open = app.skill_open;
    let has_session = app.chat_has_session;
    // Some(new_chat) when Edit with AI picked a target this frame
    let mut pick: Option<bool> = None;
    let mut save = false;
    kit::card(ui, &t, None, |ui| {
        ui.horizontal(|ui| {
            let arrow = if open { "\u{25be}" } else { "\u{25b8}" };
            let head = ui.add(egui::Label::new(RichText::new(format!("{arrow} node skill")).font(t.font_heading.clone()).color(t.ink).strong()).sense(Sense::click()));
            if head.clicked() {
                open = !open;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if has_session { "Edit with AI \u{25be}" } else { "Edit with AI" };
                let edit = kit::ai_button(ui, &t, label);
                if !has_session {
                    if edit.clicked() {
                        pick = Some(true);
                    }
                } else {
                    egui::Popup::menu(&edit).show(|ui| {
                        ui.set_min_width(140.0);
                        if kit::button(ui, &t, "In open chat", Some(t.primary_lit)).clicked() {
                            pick = Some(false);
                            egui::Popup::close_all(ui.ctx());
                        }
                        if kit::button(ui, &t, "In new chat", Some(t.primary_lit)).clicked() {
                            pick = Some(true);
                            egui::Popup::close_all(ui.ctx());
                        }
                    });
                }
            });
        });
        if open {
            ui.add(
                egui::TextEdit::multiline(&mut draft)
                    .font(t.font_body.clone())
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            );
            if draft != skill && kit::button(ui, &t, "Save", Some(t.primary_lit)).clicked() {
                save = true;
            }
        }
    });
    app.skill_open = open;
    if let Some(new_chat) = pick {
        request_chat(app, ws, format!("@skill:{node}"), new_chat);
    }
    if save {
        match graph::save_skill(ws, node, &draft) {
            Ok(()) => {
                ui.data_mut(|d| d.remove::<String>(key));
                invalidate_steps(app, ws);
                app.set_flash(format!("{node} · skill saved"));
            }
            Err(e) => app.set_flash(format!("save skill: {e}")),
        }
    } else {
        ui.data_mut(|d| {
            if draft == skill {
                d.remove::<String>(key);
            } else {
                d.insert_temp(key, draft);
            }
        });
    }
}

// ---------------------------------------------------------------- to-do

/// Tasks the AI asked a human for (request_human): the admin sees all, anyone else those
/// addressed to their user name or role.
fn asked_of_you(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let me = app.access.user.clone();
    let admin = app.access.is_admin() || me.is_none();
    let mine: Vec<_> = app
        .items
        .iter()
        .filter(|i| i.source == "ai-request" && matches!(i.status.as_str(), "queued" | "running" | "review"))
        .filter(|i| {
            let to = i.payload["assignee"].as_str().unwrap_or("admin");
            admin || me.as_ref().is_some_and(|u| u.name == to || u.role == to)
        })
        .cloned()
        .collect();
    if mine.is_empty() {
        return;
    }
    let mut done = None;
    kit::card_tinted(ui, &t, Some(&format!("asked of you by the AI \u{00b7} {}", mine.len())), t.secondary, |ui| {
        for i in &mine {
            let to = i.payload["assignee"].as_str().unwrap_or("admin");
            let ws = i.payload["workspace"].as_str().unwrap_or("");
            kit::field_row(ui, &t, &format!("for {to}"), &format!("{}{}", i.title, if ws.is_empty() { String::new() } else { format!(" \u{00b7} {ws} {}", i.payload["subject"].as_str().unwrap_or("")) }));
            if let Some(d) = i.payload["detail"].as_str().filter(|d| !d.is_empty()) {
                ui.add(egui::Label::new(small_text(&t, d).color(t.ink_weak)).wrap());
            }
            if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Done").color(t.secondary))).clicked() {
                done = Some(i.id);
            }
        }
    });
    if let Some(id) = done {
        queue::set_status(&mut app.items, id, "done");
        app.save_items();
    }
}

pub fn todo(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let open = app.tasks.iter().filter(|x| !x.done).count();
    let done = app.tasks.len() - open;
    let human: Vec<_> = app.items.iter().filter(|i| i.kind == "todo" && i.source != "decision" && i.source != "ai-request" && matches!(i.status.as_str(), "queued" | "running" | "review")).cloned().collect();
    panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new("To-do").font(t.font_heading.clone()).color(t.secondary));
            ui.add_space(8.0);
            let mut g = app.todo_group.clone();
            kit::group_dropdown(ui, &t, "todo-group", &mut g, &[("open", open), ("done", done), ("all", app.tasks.len())]);
            app.todo_group = g;
        });
        kit::subheader(ui, &t, |ui| {
            ui.add_space(4.0);
            let w = (ui.available_width() - 110.0).max(80.0);
            let edit = ui.add_sized([w, 26.0], egui::TextEdit::singleline(&mut app.input).hint_text("new task").margin(egui::Margin::symmetric(8, 4)).font(t.font_small.clone()));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(6.0);
            if (kit::button(ui, &t, "Add", Some(t.secondary)).clicked() || enter) && !app.input.trim().is_empty() {
                add(&mut app.tasks, &app.input);
                app.input.clear();
                app.save_tasks();
                edit.request_focus();
            }
        });
        body(ui, &t, "todo-body", |ui| {
            harness::decisions_card(app, ui);
            asked_of_you(app, ui);
            if !human.is_empty() {
                kit::card_tinted(ui, &t, Some("from the pipeline · your steps"), t.secondary_dim, |ui| {
                    for i in &human {
                        let step = i.node.clone().unwrap_or_default();
                        let ws = app.cfg.workspace(i.payload["workspace"].as_str().unwrap_or("")).cloned();
                        let generic = ws.as_ref().is_some_and(|w| graph::workspace_json(w)["flow"] == "generic");
                        let action = if generic { "done".to_string() } else { format!("open {}", app_for_step(&step)) };
                        let clicked = kit::label_row_tinted(ui, &t, &step.replace('_', " "), t.secondary, |ui| {
                            let res = kit::payload_split(ui, &t, 2, |ui, k| {
                                if k == 0 {
                                    ui.add(egui::Label::new(small_text(&t, format!("{} · {}", i.title, i.payload["detail"].as_str().unwrap_or(""))).color(t.ink)).truncate());
                                    false
                                } else {
                                    let w = ui.available_width();
                                    ui.add_sized([w, t.cell_height()], egui::Button::new(small_text(&t, &action).color(t.secondary))).clicked()
                                }
                            });
                            res.get(1).copied().unwrap_or(false)
                        });
                        if let Some(w) = &ws {
                            chat_ui::preview_row(app, ui, w, i.payload["subject"].as_str().unwrap_or(""));
                        }
                        if clicked {
                            if let Some(ws) = ws {
                                let m = if generic {
                                    let subject = i.payload["subject"].as_str().unwrap_or("").to_string();
                                    match graph::engine_cmd(&ws, "ai_write").args(["human-done", &subject, &step]).output() {
                                        Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).trim().to_string(),
                                        Err(e) => e.to_string(),
                                    }
                                } else {
                                    open_app(&ws, app_for_step(&step), false)
                                };
                                app.set_flash(m);
                                if generic {
                                    app.sync_now();
                                }
                            }
                        }
                    }
                });
            }
            let group = app.todo_group.clone();
            let mut toggle = None;
            let mut remove = None;
            kit::card_tinted(ui, &t, Some("tasks"), t.secondary_dim, |ui| {
                let rows: Vec<_> = app
                    .tasks
                    .iter()
                    .rev()
                    .filter(|x| match group.as_str() {
                        "open" => !x.done,
                        "done" => x.done,
                        _ => true,
                    })
                    .cloned()
                    .collect();
                if rows.is_empty() {
                    kit::notice(ui, &t, "Nothing here");
                }
                for x in rows {
                    let key = if x.source.is_empty() || x.source == "ui" { format!("#{}", x.id) } else { x.source.clone() };
                    kit::label_row_tinted(ui, &t, &key, t.secondary, |ui| {
                        let mark = if x.done { "●" } else { "○" };
                        if ui.add(egui::Button::new(small_text(&t, mark).color(if x.done { t.secondary } else { t.ink_faint })).frame(false)).clicked() {
                            toggle = Some((x.id, !x.done));
                        }
                        let title = if x.done { small_text(&t, &x.title).color(t.ink_faint).strikethrough() } else { small_text(&t, &x.title).color(t.ink) };
                        ui.add(egui::Label::new(title).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            if kit::link(ui, &t, "×", t.ink_faint).on_hover_text("delete").clicked() {
                                remove = Some(x.id);
                            }
                        });
                    });
                }
            });
            if let Some((id, d)) = toggle {
                set_done(&mut app.tasks, id, d);
                app.save_tasks();
            }
            if let Some(id) = remove {
                delete(&mut app.tasks, id);
                app.save_tasks();
            }
        });
    });
}

// ---------------------------------------------------------------- queue ai

/// The time windows the queue filters by: (id, label, seconds; 0 = all).
const WINDOWS: [(&str, &str, u64); 5] = [("1h", "last hour", 3600), ("24h", "last 24 h", 86_400), ("7d", "last 7 days", 7 * 86_400), ("30d", "last 30 days", 30 * 86_400), ("all", "all time", 0)];

/// The AI queue as jobs, cron-style: one row per item with its step, when it
/// last moved, and when the schedule on that step runs it next. Filters:
/// status, workspace, and a time window on the last update.
/// Live monitor: who is in each workspace and at which level, what runs now, the latest entries.
const MEDIA_EXT: &[&str] = &["mp4", "mov", "webm", "mkv", "mp3", "wav", "m4a", "flac", "ogg", "opus"];

/// What started a completed phase: its label and whether a human did it.
fn origin_label(o: &str) -> (&'static str, bool) {
    match o {
        "cron" => ("Cron job", false),
        "entry" => ("Entry hook", false),
        "manual" => ("Manual start", false),
        "human" => ("Human entry", true),
        _ => ("Agent", false),
    }
}

/// A completed phase as a card: the phase and subject first, what started it and when beside,
/// then the one line that matters for this kind of node, then its files with play/open.
/// AI work is purple, human entries orange; the date is the weakest thing on the card.
fn completed_card(ui: &mut Ui, t: &Theme, w: &Workspace, c: &graph::Completed, flash: &mut Option<String>) {
    let (origin, human) = origin_label(&c.origin);
    let tint = if human { t.secondary } else { t.primary };
    let media_phase = matches!(c.step.as_str(), "voiceover" | "motion_source" | "audio_fx" | "edit" | "color_grade" | "auto_review" | "scene_3d" | "gen_media" | "subtitles" | "record");
    let total: u64 = c.files.iter().map(|f| f.1).sum();
    let summary = if human {
        let who = if c.user.is_empty() { String::new() } else { format!("{} \u{00b7} ", c.user) };
        format!("{who}{}", if c.note.is_empty() { "approved" } else { &c.note })
    } else if media_phase {
        format!("{} file{} \u{00b7} {}{}", c.files.len(), if c.files.len() == 1 { "" } else { "s" }, human_size(total), if c.note.is_empty() { String::new() } else { format!(" \u{00b7} {}", c.note) })
    } else if !c.note.is_empty() {
        c.note.clone()
    } else {
        format!("{} file{}", c.files.len(), if c.files.len() == 1 { "" } else { "s" })
    };
    ui.add_space(8.0);
    egui::Frame::new().fill(t.inset).stroke(egui::Stroke::new(t.hairline, t.rule_bright)).inner_margin(egui::Margin { left: 14, right: 10, top: 8, bottom: 8 }).show(ui, |ui| {
        let bar = egui::Rect::from_min_size(ui.max_rect().left_top() - egui::vec2(14.0, 8.0), egui::vec2(3.0, ui.max_rect().height() + 16.0));
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(c.step.replace('_', " ")).font(t.font_heading.clone()).color(tint));
            ui.label(RichText::new(&c.vid).font(t.font_body.clone()).color(t.ink));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(small_text(t, origin).color(tint));
            });
        });
        ui.add(egui::Label::new(small_text(t, summary).color(t.ink_weak)).truncate());
        for (rel, size) in &c.files {
            let full = graph::root(w).join(rel);
            let exists = std::fs::metadata(&full).ok();
            let state = match (&exists, *size) {
                (None, _) => Some("missing"),
                (Some(m), s) if s > 0 && m.len() != s => Some("changed since"),
                _ => None,
            };
            let name = std::path::Path::new(rel).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let media = full.extension().and_then(|e| e.to_str()).is_some_and(|e| MEDIA_EXT.contains(&e.to_ascii_lowercase().as_str()));
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(small_text(t, format!("{name}{}", state.map(|s| format!(" \u{00b7} {s}")).unwrap_or_default())).color(if state.is_some() { t.warn } else { t.ink })).truncate());
                if state != Some("missing") {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::link(ui, t, "Open", t.ink_weak).clicked() {
                            let _ = Command::new("xdg-open").arg(&full).spawn();
                        }
                        if media && kit::link(ui, t, "\u{25b6} play", t.primary_lit).clicked() {
                            *flash = Some(ventosa::preview::play(&full, &format!("{} {}", c.vid, c.step), None).unwrap_or_else(|e| e));
                        }
                    });
                }
            });
        }
        ui.add(egui::Label::new(small_text(t, format!("{} \u{00b7} {}", c.at.replace('T', " "), w.name)).color(t.ink_faint)));
        ui.painter().rect_filled(bar, 0.0, tint);
    });
}

fn human_size(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{} KB", n >> 10),
        n => format!("{n} B"),
    }
}

/// Completed phases, newest first, by what started them: a Cron job, the Entry hook, a Manual
/// start, a Human entry or another agent. Each file is replayable from here.
fn completed_phases(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let mut rows: Vec<(Workspace, graph::Completed)> = vec![];
    for w in app.cfg.workspaces.iter().filter(|w| w.enabled) {
        rows.extend(graph::completed(w, 40).into_iter().map(|c| (w.clone(), c)));
    }
    rows.sort_by(|a, b| b.1.at.cmp(&a.1.at));
    let key = Id::new("completed-filter");
    let mut filter: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| "all".into());
    let count = |o: &str| rows.iter().filter(|r| o == "all" || r.1.origin == o).count();
    let groups: Vec<(&str, usize)> = vec![("all", count("all")), ("cron", count("cron")), ("entry", count("entry")), ("manual", count("manual")), ("human", count("human")), ("agent", count("agent"))];
    kit::card(ui, &t, Some("Completed phases"), |ui| {
        kit::label_row(ui, &t, "started by", |ui| kit::group_dropdown(ui, &t, "completed-origin", &mut filter, &groups));
        let shown: Vec<&(Workspace, graph::Completed)> = rows.iter().filter(|r| filter == "all" || r.1.origin == filter).take(12).collect();
        if shown.is_empty() {
            kit::notice(ui, &t, "No phase has finished here yet \u{2014} what the AI makes shows here, replayable");
        }
        let mut flash = None;
        for (w, c) in shown {
            completed_card(ui, &t, w, c, &mut flash);
        }
        if let Some(m) = flash {
            app.set_flash(m);
        }
    });
    ui.data_mut(|d| d.insert_temp(key, filter));
}

fn live_monitor(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let now = ventosa::now();
    let running: Vec<_> = app.items.iter().filter(|i| i.kind == "ai" && i.status == "running").cloned().collect();
    let wss: Vec<_> = app.cfg.workspaces.iter().filter(|w| w.enabled).cloned().collect();
    let mut people: Vec<(String, serde_json::Value)> = vec![];
    let mut entries: Vec<(String, serde_json::Value)> = vec![];
    for w in &wss {
        people.extend(hooks::present(w).into_iter().map(|p| (w.name.clone(), p)));
        entries.extend(hooks::recent_entries(w, 4).into_iter().map(|e| (w.name.clone(), e)));
    }
    entries.sort_by(|a, b| b.1["at"].as_str().cmp(&a.1["at"].as_str()));
    kit::card(ui, &t, Some("Live"), |ui| {
        if people.is_empty() && running.is_empty() {
            kit::notice(ui, &t, "Nobody is working right now");
        }
        for (w, p) in &people {
            let lvl = p["level"].as_str().unwrap_or("");
            let line = format!("{} \u{00b7} {} \u{00b7} {} ago", w, p["doing"].as_str().unwrap_or(""), ago(now.saturating_sub(p["at"].as_u64().unwrap_or(0))));
            kit::field_row(ui, &t, &format!("{} \u{00b7} {lvl}", p["user"].as_str().unwrap_or("")), &line);
        }
        for i in &running {
            let who = i.payload["agent"].as_str().unwrap_or("agent");
            let line = format!("{} \u{00b7} {} \u{00b7} {}", i.payload["workspace"].as_str().unwrap_or(""), i.node.clone().unwrap_or_default().replace('_', " "), i.title);
            kit::field_row(ui, &t, &format!("{who} \u{00b7} {}", ago(now.saturating_sub(i.updated))), &line);
        }
        for (w, e) in entries.iter().take(5) {
            let user = e["user"].as_str().unwrap_or("someone");
            kit::field_row(ui, &t, "entry", &format!("{user} \u{00b7} {} {} \u{00b7} {w} \u{00b7} {}", e["kind"].as_str().unwrap_or(""), e["vid"].as_str().unwrap_or(""), e["at"].as_str().unwrap_or("").replace('T', " ")));
        }
    });
}

pub fn queue(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let now = ventosa::now();
    let window = WINDOWS.iter().find(|w| w.0 == app.queue_window).map_or(0, |w| w.2);
    let in_time = |i: &queue::Item| window == 0 || now.saturating_sub(i.updated) <= window;
    let ai: Vec<_> = app.items.iter().filter(|i| i.kind == "ai" && in_time(i)).cloned().collect();
    let in_ws = |i: &queue::Item, ws: &str| ws == "all" || i.payload["workspace"] == ws;
    let last = runner::last_runs();
    panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new("History").font(t.font_heading.clone()).color(t.primary));
            ui.add_space(8.0);
            let mut win = app.queue_window.clone();
            let opts: Vec<(String, String)> = WINDOWS.iter().map(|w| (w.0.to_string(), w.1.to_string())).collect();
            if kit::dropdown(ui, &t, "queue-window", &mut win, &opts) {
                app.queue_window = win;
            }
            ui.add_space(6.0);
            let ws_now = app.queue_ws.clone();
            let mut counts: Vec<(&str, usize)> = STATUSES.iter().map(|s| (*s, ai.iter().filter(|i| i.status == *s && in_ws(i, &ws_now)).count())).collect();
            counts.push(("every", ai.iter().filter(|i| in_ws(i, &ws_now)).count()));
            let mut g = app.queue_group.clone();
            kit::group_dropdown(ui, &t, "queue-group", &mut g, &counts);
            app.queue_group = g;
            ui.add_space(6.0);
            let mut ws_groups: Vec<(String, String)> = vec![("all".into(), format!("all \u{00b7} {}", ai.len()))];
            for w in &app.cfg.workspaces {
                let n = ai.iter().filter(|i| i.payload["workspace"] == w.name.as_str()).count();
                if n > 0 {
                    ws_groups.push((w.name.clone(), format!("{} \u{00b7} {n}", w.name)));
                }
            }
            let mut ws = app.queue_ws.clone();
            if ws_groups.len() > 2 && kit::dropdown(ui, &t, "queue-ws", &mut ws, &ws_groups) {
                app.queue_ws = ws;
            }
        });
        body(ui, &t, "queue-body", |ui| {
            live_monitor(app, ui);
            completed_phases(app, ui);
            let runs = runner::recent_runs(5);
            if !runs.is_empty() {
                kit::card(ui, &t, Some("Cron runs"), |ui| {
                    for r in &runs {
                        let when = ago(ventosa::now().saturating_sub(r["at"].as_u64().unwrap_or(0)));
                        let line = format!("{} \u{00b7} {} \u{00b7} {} \u{00b7} {}s \u{00b7} {when} ago", r["schedule"].as_str().unwrap_or(""), r["title"].as_str().unwrap_or(""), r["status"].as_str().unwrap_or(""), r["secs"]);
                        kit::field_row(ui, &t, if r["status"] == "ok" { "ok" } else { "failed" }, &line);
                    }
                });
            }
            let group = app.queue_group.clone();
            let ws_now = app.queue_ws.clone();
            let mut rows: Vec<_> = ai.iter().filter(|i| (group == "every" || i.status == group) && in_ws(i, &ws_now)).cloned().collect();
            rows.sort_by(|a, b| b.updated.cmp(&a.updated));
            if rows.is_empty() {
                kit::notice(ui, &t, "No AI jobs in this window \u{2014} titles from phase 1, schedules, batches and other agents land here");
            }
            let mut set: Option<(u64, String)> = None;
            let open_key = Id::new("queue-open");
            let mut opened: Option<u64> = ui.data(|d| d.get_temp(open_key));
            for i in rows {
                let node = i.node.clone().unwrap_or_default();
                let ws_name = i.payload["workspace"].as_str().unwrap_or(&i.source).to_string();
                let sched = app.cfg.schedules.iter().find(|s| s.workspace == ws_name && s.node == node && s.enabled);
                let cadence = match sched {
                    Some(s) => match next_run(s, &last) {
                        Some(0) => format!("every {}m \u{00b7} due", s.every_minutes),
                        Some(n) => format!("every {}m \u{00b7} in {}", s.every_minutes, ago(n)),
                        None => "manual".into(),
                    },
                    None => "manual / batch".into(),
                };
                let tint = match i.status.as_str() {
                    "failed" => t.warn,
                    "running" => t.primary_lit,
                    "review" => t.secondary,
                    "done" => t.ink_faint,
                    _ => t.primary,
                };
                let clicked = kit::label_row_tinted(ui, &t, &i.status, tint, |ui| {
                    let cells = [i.title.clone(), node.replace('_', " "), cadence.clone(), format!("{} ago", ago(now.saturating_sub(i.updated)))];
                    kit::payload_split(ui, &t, 4, |ui, k| {
                        let r = ui.centered_and_justified(|ui| ui.add(egui::Label::new(small_text(&t, &cells[k]).color(if k == 0 { t.ink } else { t.ink_weak })).truncate().sense(Sense::click()))).inner;
                        r.clicked()
                    })
                    .into_iter()
                    .any(|c| c)
                });
                if clicked {
                    opened = if opened == Some(i.id) { None } else { Some(i.id) };
                }
                if opened != Some(i.id) {
                    continue;
                }
                kit::card(ui, &t, Some(&format!("job #{} \u{00b7} {}", i.id, i.title)), |ui| {
                    kit::field_row(ui, &t, "workspace", &ws_name);
                    kit::field_row(ui, &t, "created", &format!("{} ago", ago(now.saturating_sub(i.created))));
                    if let Some(d) = i.payload["detail"].as_str() {
                        kit::field_row(ui, &t, "detail", d);
                    }
                    if let Some(a) = i.payload["agent"].as_str() {
                        kit::field_row(ui, &t, "agent", a);
                    }
                    if let Some(e) = i.payload["error"].as_str() {
                        kit::label_row(ui, &t, "error", |ui| kit::centered_label(ui, small_text(&t, e).color(t.warn)));
                    }
                    kit::label_row(ui, &t, "status", |ui| {
                        let mut s = i.status.clone();
                        let opts: Vec<(String, String)> = STATUSES.iter().map(|x| (x.to_string(), x.to_string())).collect();
                        if kit::dropdown(ui, &t, ("q-status", i.id), &mut s, &opts) {
                            set = Some((i.id, s));
                        }
                    });
                    if let Some(w) = app.cfg.workspace(&ws_name) {
                        chat_ui::preview_row(app, ui, w, i.payload["subject"].as_str().unwrap_or(""));
                    }
                    for ext in ["md", "log"] {
                        let p = graph::runs_dir().join(format!("{}.{ext}", i.id));
                        if p.exists() {
                            let label = if ext == "md" { "provider output" } else { "run log" };
                            if kit::label_row(ui, &t, label, |ui| kit::payload_button(ui, &t, small_text(&t, "Open").color(t.primary_lit))).clicked() {
                                let _ = Command::new("xdg-open").arg(&p).spawn();
                            }
                        }
                    }
                });
            }
            ui.data_mut(|d| match opened {
                Some(id) => {
                    d.insert_temp(open_key, id);
                }
                None => d.remove::<u64>(open_key),
            });
            if let Some((id, s)) = set {
                queue::set_status(&mut app.items, id, &s);
                app.save_items();
            }
        });
    });
}

// ---------------------------------------------------------------- schedules

/// The cron job's skill: a collapsible editor on its file; Save lights up while the text differs.
fn cron_skill_editor(ui: &mut Ui, t: &Theme, id: &str, msg: &mut Option<String>) {
    let key = Id::new(("cron-skill", id));
    let disk = runner::cron_skill(id);
    let mut draft: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| disk.clone());
    let dirty = draft != disk;
    egui::CollapsingHeader::new(small_text(t, if dirty { "skill \u{25cf} unsaved (themes policy)" } else { "skill (themes policy)" }).color(if dirty { t.secondary } else { t.primary_lit }))
        .id_salt(("cron-skill-open", id))
        .show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut draft).font(t.font_small.clone()).desired_width(f32::INFINITY).desired_rows(10));
            let mut b = egui::Button::new(small_text(t, if dirty { "Save skill" } else { "Saved \u{2713}" }).color(if dirty { egui::Color32::BLACK } else { t.ok }));
            if dirty {
                b = b.fill(t.secondary);
            }
            if ui.add(b).clicked() && dirty {
                *msg = Some(match runner::save_cron_skill(id, &draft) {
                    Ok(()) => format!("{id} skill saved"),
                    Err(e) => format!("skill: {e}"),
                });
            }
        });
    ui.data_mut(|d| d.insert_temp(key, draft));
}

pub fn schedules(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let last = runner::last_runs();
    panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new("Cron job").font(t.font_heading.clone()).color(t.primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::link(ui, &t, "Save", t.primary).clicked() {
                    app.save_cfg();
                }
                if kit::link(ui, &t, "+ cron job", t.ink_weak).clicked() {
                    let ws = app.cfg.workspaces.first().map(|w| w.name.clone()).unwrap_or_default();
                    let n = app.cfg.schedules.len() + 1;
                    app.cfg.schedules.push(Schedule { id: format!("job-{n}"), workspace: ws, ..Default::default() });
                    app.dirty_cfg = true;
                }
            });
        });
        body(ui, &t, "sched-body", |ui| {
            if app.cfg.schedules.is_empty() {
                kit::notice(ui, &t, "A cron job runs on its own: every N minutes it takes one phase (a node's skill for the oldest queued item, or an edit of the graph) and leaves a handoff note for you in History.");
            }
            let beat = runner::heartbeat_age().map_or("scheduler has not run yet".to_string(), |a| format!("scheduler alive \u{00b7} last pass {a}s ago"));
            kit::field_row(ui, &t, "scheduler", &beat);
            let ws_opts = ws_options(app);
            let mut remove = None;
            let mut run_now = None;
            let mut skill_msg: Option<String> = None;
            for (k, s) in app.cfg.schedules.iter_mut().enumerate() {
                let steps: Vec<(String, String)> = app
                    .steps_cache
                    .get(&s.workspace)
                    .and_then(|g| g["steps"].as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter(|x| x["owner"] == "ai")
                    .filter_map(|x| x["id"].as_str().map(|i| (i.to_string(), i.replace('_', " "))))
                    .collect();
                let mut changed = false;
                kit::card(ui, &t, Some(&s.id.clone()), |ui| {
                    changed |= kit::text_row(ui, &t, "id", &mut s.id, "name").changed();
                    kit::label_row(ui, &t, "workspace", |ui| changed |= kit::dropdown(ui, &t, ("s-ws", k), &mut s.workspace, &ws_opts));
                    let tasks = vec![("node".to_string(), "run a node's skill".to_string()), ("graph".to_string(), "edit the graph itself".to_string())];
                    kit::label_row(ui, &t, "task", |ui| changed |= kit::dropdown(ui, &t, ("s-task", k), &mut s.task, &tasks));
                    if s.task != "graph" {
                        kit::label_row(ui, &t, "phase", |ui| changed |= kit::dropdown(ui, &t, ("s-node", k), &mut s.node, &steps));
                    }
                    let min_every = runner::min_minutes(s);
                    kit::label_row(ui, &t, "every", |ui| {
                        changed |= kit::drag_cell(ui, &t, egui::DragValue::new(&mut s.every_minutes).range(min_every..=10080).suffix(" min")).changed();
                    });
                    let runners = vec![("agent".to_string(), "agent (autonomous loop)".to_string()), ("command".to_string(), "command (agent CLI)".to_string()), ("provider".to_string(), "provider (API call)".to_string())];
                    kit::label_row(ui, &t, "runner", |ui| changed |= kit::dropdown(ui, &t, ("s-run", k), &mut s.runner, &runners));
                    if s.runner == "command" {
                        changed |= kit::text_row(ui, &t, "command", &mut s.command, "claude -p \"…{step} {subject}…\"").changed();
                    }
                    if s.runner == "agent" {
                        changed |= kit::text_row(ui, &t, "prompt", &mut s.prompt, "this job's own instruction, on top of the node skill").changed();
                        changed |= kit::text_row(ui, &t, "output", &mut s.output_policy, "what the final note must contain for the human").changed();
                        let mut tools = s.allowed_tools.join(", ");
                        if kit::text_row(ui, &t, "tools", &mut tools, "allowed tools, comma separated (empty = default set)").changed() {
                            s.allowed_tools = tools.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                            changed = true;
                        }
                    }
                    cron_skill_editor(ui, &t, &s.id, &mut skill_msg);
                    changed |= kit::toggle_row(ui, &t, "after entry", &mut s.on_entry).changed();
                    changed |= kit::toggle_row(ui, &t, "enabled", &mut s.enabled).changed();
                    let when = last.get(&s.id).map(|ts| format!("{} min ago", ventosa::now().saturating_sub(*ts) / 60)).unwrap_or_else(|| "never".into());
                    kit::field_row(ui, &t, "last run", &when);
                    kit::label_row(ui, &t, "", |ui| {
                        let b = kit::payload_buttons(ui, &t, vec![small_text(&t, "Run now").color(t.primary), small_text(&t, "Delete").color(t.ink_faint)]);
                        if b[0] {
                            run_now = Some(s.clone());
                        }
                        if b[1] {
                            remove = Some(k);
                        }
                    });
                });
                if changed {
                    app.dirty_cfg = true;
                }
            }
            if let Some(m) = skill_msg {
                app.set_flash(m);
            }
            if let Some(k) = remove {
                app.cfg.schedules.remove(k);
                app.dirty_cfg = true;
            }
            if let Some(s) = run_now {
                let msg = match runner::fire_as(&app.cfg, &s, false, "manual") {
                    Ok(Some(m)) => format!("started {m}"),
                    Ok(None) => format!("{}: nothing to run (nothing queued at {}, or still running)", s.id, s.node),
                    Err(e) => e,
                };
                app.set_flash(msg);
            }
            if app.dirty_cfg {
                kit::notice(ui, &t, "Unsaved changes — press save");
            }
        });
    });
}

// ---------------------------------------------------------------- apps

fn mcp_snippets() -> Vec<(&'static str, String)> {
    vec![
        ("claude code", "claude mcp add ventosa -- ventosa-mcp".into()),
        ("opencode", r#"{"mcp": {"ventosa": {"type": "local", "command": ["ventosa-mcp"]}}}"#.into()),
        ("hermes", "stdio server · command: ventosa-mcp".into()),
        ("zeroclaw", "stdio server · command: ventosa-mcp".into()),
        ("openclaw", "stdio server · command: ventosa-mcp".into()),
    ]
}

pub fn mcp_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("connect agents (MCP, stdio)"), |ui| {
        for (client, snip) in mcp_snippets() {
            kit::label_row(ui, &t, client, |ui| {
                if kit::payload_button(ui, &t, small_text(&t, &snip).color(t.primary_lit)).on_hover_text("copy").clicked() {
                    ui.ctx().copy_text(snip.clone());
                }
            });
        }
    });
}

enum Tile {
    None,
    Open,
    Stop,
}

/// One launcher tile: a glyph, the app name, a hint line; click opens, the corner link stops.
fn tile(ui: &mut Ui, t: &Theme, glyph: &str, name: &str, hint: &str, port: &str) -> Tile {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(150.0, 96.0), Sense::click());
    let hot = resp.hovered();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, if hot { t.node_header } else { t.node_body });
    p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, if hot { t.secondary } else { t.rule_bright }), egui::StrokeKind::Inside);
    p.text(rect.min + egui::vec2(12.0, 10.0), egui::Align2::LEFT_TOP, glyph, egui::FontId::proportional(22.0), t.secondary);
    p.text(rect.min + egui::vec2(12.0, 44.0), egui::Align2::LEFT_TOP, name, t.font_small.clone(), t.ink);
    let galley = p.layout(hint.to_string(), t.font_small.clone(), t.ink_faint, rect.width() - 24.0);
    p.with_clip_rect(rect.shrink(2.0)).galley(rect.min + egui::vec2(12.0, 62.0), galley, t.ink_faint);
    p.text(rect.right_top() + egui::vec2(-8.0, 8.0), egui::Align2::RIGHT_TOP, port, t.font_small.clone(), t.ink_faint);
    let stop = egui::Rect::from_min_size(rect.right_top() + egui::vec2(-30.0, 28.0), egui::vec2(26.0, 18.0));
    let stopped = ui.interact(stop, resp.id.with("stop"), Sense::click()).on_hover_text("stop this app");
    p.text(stop.center(), egui::Align2::CENTER_CENTER, "\u{25a0}", t.font_small.clone(), if stopped.hovered() { t.warn } else { t.ink_faint });
    if stopped.clicked() {
        Tile::Stop
    } else if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        Tile::Open
    } else {
        Tile::None
    }
}

pub fn apps(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new("Apps").font(t.font_heading.clone()).color(t.primary));
        });
        body(ui, &t, "apps-body", |ui| {
            for ws in app.cfg.workspaces.clone() {
                let wj = graph::workspace_json(&ws);
                let brand = wj["brand"]["name"].as_str().unwrap_or(&ws.name).to_string();
                kit::card(ui, &t, Some(&format!("{brand} · {}", ws.name)), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                        for (which, port_key, glyph, hint) in [
                            ("labelling", "labelling", "\u{270e}", "label scripts and thesis options"),
                            ("teleprompter", "teleprompter", "\u{25b6}", "record takes from the script"),
                            ("dashboard", "dashboard", "\u{25a4}", "uploads, token spend per video, phase KPIs"),
                            ("uploads", "uploads", "\u{2197}", "what is ready to upload"),
                            ("studio", "studio", "\u{25a3}", "label rendered timelines"),
                            ("media-labelling", "media-labelling", "\u{25b7}", "watch results; approve or send back"),
                        ] {
                            let port = wj["ports"][port_key].as_u64().map(|p| format!(":{p}")).unwrap_or_default();
                            match tile(ui, &t, glyph, which, hint, &port) {
                                Tile::Open => {
                                    let m = open_app(&ws, which, false);
                                    app.set_flash(m);
                                }
                                Tile::Stop => {
                                    let m = open_app(&ws, which, true);
                                    app.set_flash(m);
                                }
                                Tile::None => {}
                            }
                        }
                    });
                });
            }
        });
    });
}

// ---------------------------------------------------------------- settings

pub fn brand_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("brand"), |ui| {
        let mut c = false;
        c |= kit::text_row(ui, &t, "name", &mut app.cfg.brand.name, "ventosa").changed();
        c |= kit::text_row(ui, &t, "tagline", &mut app.cfg.brand.tagline, "").changed();
        if c {
            app.dirty_cfg = true;
        }
    });
    if app.cfg.brand.accent.eq_ignore_ascii_case("#ff2e2e") {
        // older configs stored the stock red, which already shows as the Ventosa purple
        app.cfg.brand.accent = "#8b3dff".into();
    }
    kit::card(ui, &t, Some("emphasis colours"), |ui| {
        let mut c = false;
        c |= colour_row(ui, &t, "primary", &mut app.cfg.brand.accent, "#8b3dff");
        c |= colour_row(ui, &t, "secondary", &mut app.cfg.brand.accent2, "#ff8a1f");
        ui.label(small_text(&t, "Primary: selection, AI, headings \u{00b7} secondary: to-do, attention. Previewed live; save keeps them.").color(t.ink_faint));
        if c {
            app.dirty_cfg = true;
            // live preview: rebuild the theme now, not only on save
            app.theme = App::theme_for(&app.cfg);
        }
    });
}

/// A colour cell: swatch picker, the hex, and a reset to the stock colour.
fn colour_row(ui: &mut Ui, t: &Theme, label: &str, value: &mut String, stock: &str) -> bool {
    let mut changed = false;
    kit::label_row(ui, t, label, |ui| {
        let cur = super::theme::hex(value).or_else(|| super::theme::hex(stock)).unwrap_or(egui::Color32::WHITE);
        let mut rgb = [cur.r(), cur.g(), cur.b()];
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *value = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
            changed = true;
        }
        if !value.eq_ignore_ascii_case(stock) && kit::link(ui, t, "reset", t.ink_faint).clicked() {
            *value = stock.into();
            changed = true;
        }
        let r = ui.add(egui::TextEdit::singleline(value).font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY));
        if r.changed() && super::theme::hex(value).is_some() {
            changed = true;
        }
        r
    });
    changed
}

pub fn provider_card(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("ai provider"), |ui| {
        let mut c = false;
        let opts: Vec<(String, String)> = PROVIDERS.iter().map(|p| (p.to_string(), p.to_string())).collect();
        kit::label_row(ui, &t, "kind", |ui| c |= kit::dropdown(ui, &t, "prov-kind", &mut app.cfg.provider.kind, &opts));
        c |= kit::text_row(ui, &t, "base url", &mut app.cfg.provider.base_url, "https://api.anthropic.com").changed();
        c |= kit::text_row(ui, &t, "model", &mut app.cfg.provider.model, "claude-opus-5-5").changed();
        c |= kit::text_row(ui, &t, "key env", &mut app.cfg.provider.api_key_env, "ANTHROPIC_API_KEY, ANTHROPIC_API_KEY_2 (several = rotation)").changed();
        let found = ventosa::models::keys(&app.cfg.provider.api_key_env).len();
        let set = found > 0;
        kit::label_row(ui, &t, "key", |ui| {
            ui.label(small_text(&t, if set { format!("{found} key(s) found in the environment{}", if found > 1 { " \u{2014} a rate-limited or refused key hands over to the next" } else { "" }) } else { "not set — export it before starting ventosa".to_string() }).color(if set { t.ok } else { t.warn }));
        });
        let mut fb = app.cfg.fallbacks.join(", ");
        if kit::text_row(ui, &t, "fallbacks", &mut fb, "openai:gpt-5, google:gemini-2.5-pro (tried in order when this one fails)").changed() {
            app.cfg.fallbacks = fb.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            c = true;
        }
        let go = kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Test connection (one tiny request per key)").color(t.secondary)));
        if go.clicked() {
            let p = app.cfg.provider.clone();
            let flash = app.flash.clone();
            let ctx = ui.ctx().clone();
            if let Ok(mut f) = flash.lock() {
                *f = "testing the provider\u{2026}".into();
            }
            std::thread::spawn(move || {
                let m = match ventosa::models::test(&p) {
                    Ok(m) => format!("provider ok: {m}"),
                    Err(e) => format!("provider: {e}"),
                };
                if let Ok(mut f) = flash.lock() {
                    *f = m;
                }
                ctx.request_repaint();
            });
        }
        if c {
            app.dirty_cfg = true;
        }
    });
}

pub fn workspace_cards(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let mut remove = None;
    for (k, ws) in app.cfg.workspaces.iter_mut().enumerate() {
        let mut c = false;
        kit::card(ui, &t, Some(&format!("workspace · {}", ws.name)), |ui| {
            c |= kit::text_row(ui, &t, "name", &mut ws.name, "").changed();
            c |= kit::text_row(ui, &t, "root", &mut ws.root, "~/Projects/…").changed();
            let engines = vec![("ventosa".to_string(), "ventosa engine".to_string()), ("local".to_string(), "workspace's own pipeline/".to_string())];
            kit::label_row(ui, &t, "engine", |ui| c |= kit::dropdown(ui, &t, ("ws-eng", k), &mut ws.engine, &engines));
            c |= kit::toggle_row(ui, &t, "sync", &mut ws.enabled).changed();
            kit::label_row(ui, &t, "", |ui| {
                if kit::payload_button(ui, &t, small_text(&t, "Remove from ventosa (files stay)").color(t.ink_faint)).clicked() {
                    remove = Some(k);
                }
            });
        });
        if c {
            app.dirty_cfg = true;
        }
    }
    if let Some(k) = remove {
        app.cfg.workspaces.remove(k);
        app.dirty_cfg = true;
    }
    graph_builder(app, ui);
}

/// "create a graph": a name, a folder and a list of step nodes picked from the gallery.
fn graph_builder(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    if app.gallery.is_empty() {
        app.gallery = gallery::all(&app.cfg.workspaces);
    }
    kit::card(ui, &t, Some("create a graph"), |ui| {
        kit::text_row(ui, &t, "name", &mut app.new_ws_name, "my-show");
        kit::text_row(ui, &t, "brand", &mut app.builder_brand, "My Show");
        kit::text_row(ui, &t, "folder", &mut app.new_ws_dir, "~/Projects/my-show");
        let presets = vec![("media".to_string(), "media (video) steps".to_string()), ("podcast".to_string(), "podcast steps".to_string()), ("empty".to_string(), "empty".to_string())];
        let mut p = app.builder_preset.clone();
        kit::label_row(ui, &t, "start from", |ui| {
            if kit::dropdown(ui, &t, "builder-preset", &mut p, &presets) {
                app.builder = if p == "empty" { vec![] } else { gallery::preset(&p) };
            }
        });
        app.builder_preset = p;
        let go = kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Create workspace").color(t.primary)));
        if go.clicked() {
            let msg = create_from_builder(app);
            app.set_flash(msg);
        }
        kit::notice(ui, &t, "Add or change nodes afterwards from the graph's + bar");
    });
}

fn create_from_builder(app: &mut App) -> String {
    let name = app.new_ws_name.trim().to_string();
    let dir = app.new_ws_dir.trim().replacen('~', &dirs::home_dir().unwrap_or_default().display().to_string(), 1);
    if name.is_empty() || dir.is_empty() || app.builder.is_empty() {
        return "a name, a folder and at least one step are needed".into();
    }
    let base = if app.builder_preset == "podcast" { "podcast" } else { "media" };
    let brand = if app.builder_brand.trim().is_empty() { name.clone() } else { app.builder_brand.trim().to_string() };
    let out = match Command::new("ventosa-init").args([base, dir.as_str(), "--name", name.as_str(), "--brand", brand.as_str()]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => return format!("ventosa-init: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => return format!("ventosa-init: {e}"),
    };
    // Unchanged preset: keep the template's own flow. Anything else: the composed graph on the generic flow.
    let unchanged = app.builder_preset != "empty" && app.builder == gallery::preset(&app.builder_preset);
    if !unchanged {
        if let Err(e) = gallery::write_graph(std::path::Path::new(&dir), &name, &app.builder) {
            return format!("{out} · graph: {e}");
        }
    }
    app.cfg = ventosa::config::load(&app.cfg_path);
    app.gallery.clear();
    app.ws_sel = name.clone();
    app.canvas = Default::default();
    format!("{out}{}", if unchanged { "" } else { " · custom graph (generic flow)" })
}

/// The start node's idea box: a title typed here is registered and queued for options.
fn start_idea(app: &mut App, ui: &mut Ui, ws: &Workspace) {
    let t = app.theme.clone();
    panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new("Manual start").font(t.font_heading.clone()).color(t.primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::link(ui, &t, "\u{00d7}", t.ink_weak).on_hover_text("close").clicked() {
                    app.canvas.selected = None;
                }
            });
        });
        body(ui, &t, "start-idea", |ui| start_form(app, ui, ws));
    });
}

/// The manual start: an idea typed by hand enters the graph's first step.
fn start_form(app: &mut App, ui: &mut Ui, ws: &Workspace) {
    let t = app.theme.clone();
    kit::card(ui, &t, Some("Idea"), |ui| {
        kit::text_row(ui, &t, "title", &mut app.intake_title, "a video / episode title");
        kit::label_row(ui, &t, "comment", |ui| {
            ui.add(egui::TextEdit::multiline(&mut app.intake_comment).font(t.font_small.clone()).desired_rows(3).desired_width(f32::INFINITY).hint_text("goes to every phase. Use <<name>> for a variable, e.g. theme: \"<<global_theme>>\""));
        });
        let names = graph::comment_placeholders(&app.intake_comment);
        app.intake_vars.retain(|(k, _)| names.contains(k));
        for n in &names {
            if !app.intake_vars.iter().any(|(k, _)| k == n) {
                app.intake_vars.push((n.clone(), String::new()));
            }
        }
        for (k, v) in app.intake_vars.iter_mut() {
            kit::text_row(ui, &t, &format!("<<{k}>>"), v, "value (empty: the AI resolves it)");
        }
        let go = kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Start").color(t.primary)));
        if go.clicked() && !app.intake_title.trim().is_empty() {
            let notes = graph::fill_comment(app.intake_comment.trim(), &app.intake_vars);
            match graph::intake(ws, app.intake_title.trim(), &notes) {
                Ok(id) => {
                    app.set_flash(format!("{id} registered"));
                    app.intake_title.clear();
                    app.intake_comment.clear();
                    app.intake_vars.clear();
                    app.sync_now();
                }
                Err(e) => app.set_flash(format!("intake: {e}")),
            }
        }
    });
}
