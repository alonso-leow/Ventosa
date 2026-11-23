//! The overview's node canvas, after MetalVJ's graph view: a dotted grid under a
//! pan/zoom view transform (`screen = origin + pan + graph·zoom`), nodes with a
//! header bar and rows, bezier cables between ports, drag to move a node, drag
//! the ground to pan, wheel to zoom at the pointer.
//!
//! The AI process steps (options → drafts → script → edit → publish_meta) sit on
//! the centre line in the primary purple; human steps (to-do) hang below in the
//! secondary orange. Each step carries "log points": small dots for its recent
//! engine events and scheduler runs, joined to the node by a faint line.
//! Hovering a node grows it into a card (about 2.2x wide) that lists its logs;
//! with no logs it shows the node's skill in the height the logs would take;
//! the card stays open for a moment after the pointer leaves, and while the
//! pointer is inside it.

use super::theme::Theme;
use eframe::egui::{self, epaint::CubicBezierShape, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::time::Duration;

/// A step as an action the node does, not the name of a stage.
pub fn action_label(id: &str) -> String {
    let s = match id {
        "start" => "Manual start",
        "@schedules" => "Cron job",
        "pool" => "Write pool",
        "options" => "Write thesis options",
        "thesis" => "Write thesis",
        "pick" => "Label options",
        "drafts" | "draft" => "Write draft",
        "voiceover" => "Generate voiceover",
        "motion_source" | "create_motion" => "Create Motion",
        "audio_fx" | "organize_audio_fx" => "Organize Audio FX",
        "mount_audio_fx" => "Mount Audio FX",
        "label_media" => "Label Media",
        "trim_clip" => "Trim Clip",
        "script" => "Write script",
        "label" => "Label script",
        "record" => "Record video",
        "edit" => "Edit video",
        "subtitles" => "Write subtitles",
        "publish_meta" => "Write publish text",
        "publish" => "Publish video",
        "upload" => "Upload video",
        other => return ventosa::gallery::display_name(other),
    };
    s.to_string()
}

#[derive(Clone)]
pub struct Step {
    pub id: String,
    pub owner: String,
    pub need: String,
    /// Subjects currently at this step.
    pub count: usize,
    /// Agents attached here (from the engine).
    pub agents: Vec<String>,
    /// The node's skill file; the hover card shows it when the step has no logs.
    pub skill_path: Option<std::path::PathBuf>,
    /// The files this step read changed since it ran.
    pub stale: bool,
}

#[derive(Clone)]
pub struct LogPoint {
    pub step: String,
    pub text: String,
    pub ai: bool,
}

pub struct Canvas {
    pub pan: Vec2,
    pub zoom: f32,
    pub pos: HashMap<String, Pos2>,
    pub selected: Option<String>,
    pub fitted: bool,
    /// The node whose hover card is open (grown or growing).
    card: Option<String>,
    /// When the pointer left the node and card; the card closes after HOLD_SECS.
    leave_at: Option<f64>,
    /// The open card of a node that embeds a component (screen rect), for the caller to fill.
    pub open_embed: Option<(String, Rect)>,
    /// Measured height (graph units) of the embedded form, so the card grows to hold it.
    pub embed_h: f32,
    /// The workspace whose saved view this canvas carries (empty = not restored yet).
    pub view_ws: String,
}

/// Nodes whose hover card hosts a component instead of logs.
fn embeds(id: &str) -> bool {
    id == "start"
}

const NODE_W: f32 = 168.0;
const NODE_W_HUMAN: f32 = 140.0;
const HEADER_H: f32 = 24.0;
const ROW_H: f32 = 16.0;
const SPACING: f32 = 220.0;
const HUMAN_DROP: f32 = 170.0;
const LOG_MAX: usize = 8;
const CARD_W: f32 = NODE_W * 2.2;
const CARD_LOG_ROW: f32 = 14.0;
const GROW_SECS: f32 = 0.15;
const HOLD_SECS: f64 = 0.4;

impl Default for Canvas {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0, pos: HashMap::new(), selected: None, fitted: false, card: None, leave_at: None, open_embed: None, embed_h: 96.0, view_ws: String::new() }
    }
}

impl Canvas {
    fn layout(&mut self, steps: &[Step]) {
        // AI steps on the centre line in graph order; each human step under the
        // gap after the AI step before it.
        // Consecutive human steps after the same AI step sit side by side.
        let mut ai_x = 0.0f32;
        let mut human_run = 0usize;
        for s in steps {
            if s.owner == "start" {
                // manual start on the line, the cron job above it; both feed the first step
                let p = Pos2::new(0.0, if s.id == "start" { 0.0 } else { -HUMAN_DROP });
                self.pos.entry(s.id.clone()).or_insert(p);
                ai_x = ai_x.max(SPACING);
                continue;
            }
            if s.owner != "human" {
                human_run = 0;
            }
            if let Some(p) = self.pos.get(&s.id) {
                if s.owner != "human" {
                    ai_x = p.x + SPACING;
                } else {
                    human_run += 1;
                }
                continue;
            }
            let p = if s.owner != "human" {
                let p = Pos2::new(ai_x, 0.0);
                ai_x += SPACING;
                p
            } else {
                let p = Pos2::new(ai_x - SPACING * 0.5 + human_run as f32 * (NODE_W_HUMAN + 30.0), HUMAN_DROP);
                human_run += 1;
                p
            };
            self.pos.insert(s.id.clone(), p);
        }
    }

    /// Height of the hover card in graph units: header, owner, need, the log list.
    fn card_h(&self, embed: bool) -> f32 {
        if embed {
            return HEADER_H + ROW_H * 2.0 + self.embed_h.max(96.0) + 8.0;
        }
        HEADER_H + ROW_H * 2.0 + 14.0 + LOG_MAX as f32 * CARD_LOG_ROW + 10.0
    }

    /// The expanded card of a node, in screen space (it grows from the node's top-left).
    fn card_rect(&self, node: Rect, id: &str) -> Rect {
        Rect::from_min_size(node.min, Vec2::new(CARD_W, self.card_h(embeds(id))) * self.zoom)
    }

    fn to_screen(&self, origin: Pos2, g: Pos2) -> Pos2 {
        origin + self.pan + g.to_vec2() * self.zoom
    }

    fn node_size(&self, s: &Step) -> Vec2 {
        let w = if s.owner != "human" { NODE_W } else { NODE_W_HUMAN };
        let rows = 2 + usize::from(!s.agents.is_empty());
        Vec2::new(w, HEADER_H + ROW_H * rows as f32 + 8.0) * self.zoom
    }

    /// Draw the canvas; returns the clicked step, if any.
    pub fn show(&mut self, ui: &mut Ui, t: &Theme, steps: &[Step], logs: &[LogPoint]) -> Option<String> {
        self.layout(steps);
        let (rect, ground) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, t.node_canvas);
        if !self.fitted && !steps.is_empty() {
            self.fit(rect, steps);
        }

        // zoom at pointer
        if let Some(hover) = ground.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.35, 2.5);
                let g = (hover - rect.min - self.pan) / old;
                self.pan = hover - rect.min - g * self.zoom;
            }
        }
        if ground.dragged() {
            self.pan += ground.drag_delta();
        }

        // grid dots
        let step = t.graph_grid * self.zoom;
        if step > 6.0 {
            let off = Vec2::new(self.pan.x.rem_euclid(step), self.pan.y.rem_euclid(step));
            let mut y = rect.min.y + off.y;
            while y < rect.max.y {
                let mut x = rect.min.x + off.x;
                while x < rect.max.x {
                    painter.circle_filled(Pos2::new(x, y), 1.0, t.rule_bright);
                    x += step;
                }
                y += step;
            }
        }

        let origin = rect.min;
        let rects: HashMap<String, Rect> = steps
            .iter()
            .map(|s| (s.id.clone(), Rect::from_min_size(self.to_screen(origin, self.pos[&s.id]), self.node_size(s))))
            .collect();

        // cables: graph order
        let first = steps.iter().position(|s| s.owner != "start").unwrap_or(steps.len());
        let mut pairs: Vec<(&Step, &Step)> = steps.windows(2).filter(|p| p[0].owner != "start" && p[1].owner != "start").map(|p| (&p[0], &p[1])).collect();
        if let Some(f) = steps.get(first) {
            pairs.extend(steps[..first].iter().map(|s| (s, f)));
        }
        for pair in pairs.iter().map(|p| [p.0, p.1]) {
            let (a, b) = (&rects[&pair[0].id], &rects[&pair[1].id]);
            let from = Pos2::new(a.max.x, a.min.y + HEADER_H * self.zoom * 0.5);
            let to = Pos2::new(b.min.x, b.min.y + HEADER_H * self.zoom * 0.5);
            let dx = ((to.x - from.x).abs() * 0.5).max(40.0 * self.zoom);
            let lit = self.selected.as_deref().is_some_and(|s| s == pair[0].id || s == pair[1].id);
            let both_ai = pair[0].owner != "human" && pair[1].owner != "human";
            let color = if lit { t.primary } else if both_ai { t.primary_dim } else { t.node_cable };
            painter.add(CubicBezierShape::from_points_stroke(
                [from, from + Vec2::new(dx, 0.0), to - Vec2::new(dx, 0.0), to],
                false,
                Color32::TRANSPARENT,
                Stroke::new(if both_ai { 2.0 } else { 1.0 } * self.zoom.max(0.6), color),
            ));
            painter.circle_filled(from, 3.0 * self.zoom, color);
            painter.circle_filled(to, 3.0 * self.zoom, color);
        }

        // hover: the card of the node under the pointer; held HOLD_SECS after the pointer leaves
        let now = ui.input(|i| i.time);
        let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
        let inside = pointer.and_then(|p| {
            if let Some(id) = &self.card {
                if self.card_rect(rects[id], id).contains(p) {
                    return Some(id.clone());
                }
            }
            steps.iter().find(|s| rects[&s.id].contains(p)).map(|s| s.id.clone())
        });
        // typing into an embedded component keeps its card open
        let inside = inside.or_else(|| self.card.clone().filter(|id| embeds(id) && ui.ctx().memory(|m| m.focused().is_some())));
        match inside {
            Some(id) => {
                self.card = Some(id);
                self.leave_at = None;
            }
            None if self.card.is_some() => {
                let at = *self.leave_at.get_or_insert(now + HOLD_SECS);
                if now >= at {
                    self.card = None;
                    self.leave_at = None;
                } else {
                    ui.ctx().request_repaint_after(Duration::from_secs_f64(at - now));
                }
            }
            None => {}
        }
        let grow: HashMap<String, f32> = steps
            .iter()
            .map(|s| {
                let target = if self.card.as_deref() == Some(s.id.as_str()) { 1.0 } else { 0.0 };
                (s.id.clone(), ui.ctx().animate_value_with_time(ui.id().with(("card", &s.id)), target, GROW_SECS))
            })
            .collect();

        // nodes
        let mut clicked = None;
        for s in steps {
            let r = rects[&s.id];
            let id = ui.id().with(("node", &s.id));
            let resp = ui.interact(r, id, Sense::click_and_drag());
            if resp.dragged() {
                *self.pos.get_mut(&s.id).unwrap() += resp.drag_delta() / self.zoom;
            }
            if resp.clicked() {
                self.selected = Some(s.id.clone());
                clicked = Some(s.id.clone());
            }
            let ai = s.owner != "human";
            let accent = if ai { t.primary } else { t.secondary };
            let selected = self.selected.as_deref() == Some(&s.id);
            if ai {
                painter.add(Shape::rect_filled(r.expand(6.0 * self.zoom), 0.0, t.glow));
            }
            painter.rect_filled(r, 0.0, t.node_body);
            let head = Rect::from_min_size(r.min, Vec2::new(r.width(), HEADER_H * self.zoom));
            painter.rect_filled(head, 0.0, t.node_header);
            painter.rect_filled(Rect::from_min_size(head.min, Vec2::new(3.0 * self.zoom, head.height())), 0.0, accent);
            let fs = |size: f32| egui::FontId::monospace((size * self.zoom).max(6.0));
            painter.text(
                head.left_center() + Vec2::new(10.0 * self.zoom, 0.0),
                Align2::LEFT_CENTER,
                action_label(&s.id),
                fs(12.0),
                accent,
            );
            painter.text(
                head.right_center() - Vec2::new(8.0 * self.zoom, 0.0),
                Align2::RIGHT_CENTER,
                if s.id == "@schedules" { "cron" } else if s.owner == "start" { "manual" } else if ai { "AI" } else { "to-do" },
                fs(9.0),
                t.ink_faint,
            );
            let mut y = head.max.y + 4.0 * self.zoom;
            let row = |painter: &egui::Painter, y: f32, text: String, color: Color32| {
                let max = ((r.width() / (6.2 * self.zoom)) as usize).max(4);
                let text = if text.chars().count() > max { format!("{}…", text.chars().take(max - 1).collect::<String>()) } else { text };
                painter.text(Pos2::new(r.min.x + 8.0 * self.zoom, y + ROW_H * self.zoom * 0.5), Align2::LEFT_CENTER, text, fs(10.0), color);
            };
            let dot = if s.count > 0 { accent } else { t.node_dot_off };
            painter.circle_filled(Pos2::new(r.max.x - 10.0 * self.zoom, y + ROW_H * self.zoom * 0.5), 3.5 * self.zoom, dot);
            row(&painter, y, format!("{} waiting", s.count), if s.count > 0 { t.ink } else { t.ink_faint });
            y += ROW_H * self.zoom;
            row(&painter, y, s.need.clone(), t.ink_weak);
            y += ROW_H * self.zoom;
            if !s.agents.is_empty() {
                row(&painter, y, format!("⚑ {}", s.agents.join(", ")), t.primary_lit);
            }
            let stroke = if selected { Stroke::new(1.5, accent) } else { Stroke::new(1.0, Color32::BLACK) };
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        }

        self.open_embed = None;
        // hover cards, over everything: grown from the node, listing its logs
        for s in steps {
            let p = grow[&s.id];
            if p <= 0.001 {
                continue;
            }
            self.draw_card(&painter, t, s, rects[&s.id], logs, p);
            if p > 0.98 && embeds(&s.id) {
                self.open_embed = Some((s.id.clone(), self.card_rect(rects[&s.id], &s.id)));
            }
        }
        if ground.double_clicked() {
            self.fitted = false;
        }
        // a click on empty ground drops the selection (and the panel under the graph)
        if ground.clicked() && clicked.is_none() {
            self.selected = None;
        }
        clicked
    }

    fn draw_card(&self, painter: &egui::Painter, t: &Theme, s: &Step, node: Rect, logs: &[LogPoint], p: f32) {
        let target = self.card_rect(node, &s.id);
        let rect = Rect::from_min_size(node.min, node.size() + (target.size() - node.size()) * p);
        let painter = painter.with_clip_rect(rect);
        let z = self.zoom;
        let ai = s.owner != "human";
        let accent = if ai { t.primary } else { t.secondary };
        let fs = |size: f32| egui::FontId::monospace((size * z).max(6.0));
        painter.rect_filled(rect, 0.0, t.node_body);
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEADER_H * z));
        painter.rect_filled(head, 0.0, t.node_header);
        painter.rect_filled(Rect::from_min_size(head.min, Vec2::new(3.0 * z, head.height())), 0.0, accent);
        painter.text(head.left_center() + Vec2::new(10.0 * z, 0.0), Align2::LEFT_CENTER, action_label(&s.id), fs(12.0), accent);
        painter.text(
            head.right_center() - Vec2::new(8.0 * z, 0.0),
            Align2::RIGHT_CENTER,
            if s.stale { "stale" } else if ai { "AI" } else { "to-do" },
            fs(9.0),
            if s.stale { t.secondary } else { t.ink_faint },
        );
        let max = ((rect.width() / (6.2 * z)) as usize).max(8);
        let clip = |text: &str| -> String {
            let text = text.replace('\n', " · ");
            if text.chars().count() > max { format!("{}…", text.chars().take(max - 1).collect::<String>()) } else { text }
        };
        let mut y = head.max.y + 4.0 * z;
        let mut field = |key: &str, value: String, color: Color32| {
            let at = Pos2::new(rect.min.x + 8.0 * z, y + ROW_H * z * 0.5);
            painter.text(at, Align2::LEFT_CENTER, key, fs(10.0), t.ink_faint);
            painter.text(at + Vec2::new(56.0 * z, 0.0), Align2::LEFT_CENTER, clip(&value), fs(10.0), color);
            y += ROW_H * z;
        };
        field("owner", if ai { "ai".into() } else { "human".into() }, accent);
        field("need", s.need.clone(), t.ink_weak);
        y += 6.0 * z;
        if embeds(&s.id) {
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, accent), egui::StrokeKind::Inside);
            return;
        }
        let has_logs = logs.iter().any(|l| l.step == s.id);
        if has_logs {
            painter.text(Pos2::new(rect.min.x + 8.0 * z, y), Align2::LEFT_TOP, "logs", fs(10.0), t.ink_faint);
        }
        y += 14.0 * z;
        let mine: Vec<&LogPoint> = logs.iter().filter(|l| l.step == s.id).take(LOG_MAX).collect();
        if mine.is_empty() {
            // no logs: the skill fills the space the log list would take
            painter.text(Pos2::new(rect.min.x + 8.0 * z, y - 14.0 * z), Align2::LEFT_TOP, "logs  none yet · skill", fs(10.0), t.ink_faint);
            let skill = s.skill_path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| "no skill file".into());
            let wrap = (target.width() - 16.0 * z).max(40.0);
            let galley = painter.layout(skill.trim().to_string(), fs(10.0), t.ink_weak, wrap);
            painter.with_clip_rect(Rect::from_min_max(Pos2::new(rect.min.x, y), rect.max - Vec2::new(0.0, 4.0 * z))).galley(Pos2::new(rect.min.x + 8.0 * z, y), galley, t.ink_weak);
        }
        for l in mine {
            let c = if l.ai { t.primary_lit } else { t.secondary };
            painter.text(Pos2::new(rect.min.x + 8.0 * z, y + CARD_LOG_ROW * z * 0.5), Align2::LEFT_CENTER, clip(&l.text), fs(10.0), c);
            y += CARD_LOG_ROW * z;
        }
        painter.rect_stroke(rect, 0.0, Stroke::new(if s.stale { 2.0 } else { 1.0 }, if s.stale { t.secondary } else { accent }), egui::StrokeKind::Inside);
    }

    /// Fit every node in view.
    fn fit(&mut self, rect: Rect, steps: &[Step]) {
        let mut b = Rect::NOTHING;
        for s in steps {
            let p = self.pos[&s.id];
            b = b.union(Rect::from_min_size(p, Vec2::new(NODE_W, HEADER_H + ROW_H * 3.0)));
        }
        let b = b.expand(60.0);
        self.zoom = (rect.width() / b.width()).min(rect.height() / b.height()).clamp(0.35, 1.4);
        self.pan = rect.size() * 0.5 - b.center().to_vec2() * self.zoom;
        self.fitted = true;
    }
}
