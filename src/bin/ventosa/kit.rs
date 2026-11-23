//! Components, ported from MetalVJ's shell kit (`components.rs`): panel and
//! header frames, the panel head strip, outlined cards, label|control rows on
//! fixed columns, dropdowns and the small text face. Pages are built only from
//! these, so every page reads like a MetalVJ properties panel.

use super::theme::Theme;
use eframe::egui::{self, Color32, Frame, Id, RichText, Sense, Ui, Vec2};

pub fn ground_frame(t: &Theme) -> Frame {
    Frame::new().fill(t.tube).inner_margin(t.panel_margin())
}

pub fn header_frame(t: &Theme) -> Frame {
    Frame::new().fill(t.tube_raised).stroke(t.rule_stroke()).inner_margin(t.header_margin())
}

pub fn panel_frame(t: &Theme) -> Frame {
    Frame::new().fill(t.panel).stroke(t.rule_stroke()).inner_margin(t.panel_margin()).corner_radius(t.corner_radius())
}

/// The strip a panel paints above its body: darker than the panel, lit by the glow.
pub fn panel_head<R>(ui: &mut Ui, t: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    let full = ui.available_width() - 2.0 * t.pad as f32;
    Frame::new()
        .fill(t.panel_head)
        .inner_margin(t.panel_margin())
        .shadow(t.card_shadow())
        .show(ui, |ui| {
            let size = Vec2::new(full.max(ui.available_width()), t.panel_head_height);
            ui.set_min_size(size);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), add).inner
        })
        .inner
}

/// The wordmark: a gradient from the secondary (orange) to the primary (purple), letter by letter.
pub fn wordmark(ui: &mut Ui, t: &Theme, name: &str) {
    let mut job = egui::text::LayoutJob::default();
    let n = name.chars().count().max(2) as f32 - 1.0;
    for (i, ch) in name.chars().enumerate() {
        let c = t.secondary.lerp_to_gamma(t.primary, i as f32 / n);
        job.append(&ch.to_string(), 0.0, egui::TextFormat { font_id: t.logo_font(), color: c, ..Default::default() });
    }
    ui.label(job);
}

/// First letter upper-case: every phrase the client shows starts in caps.
pub fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn small_text(t: &Theme, text: impl Into<String>) -> RichText {
    RichText::new(text).font(t.font_small.clone())
}

/// The outlined group every properties section sits in.
pub fn card<R>(ui: &mut Ui, t: &Theme, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_tinted(ui, t, title, t.rule, add)
}

/// A card whose rule takes a colour (orange for to-do, purple for AI).
pub fn card_tinted<R>(ui: &mut Ui, t: &Theme, title: Option<&str>, rule: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = Frame::new()
        .stroke(egui::Stroke::new(t.hairline, rule))
        .corner_radius(t.corner_radius())
        .inner_margin(t.card_pad)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(title) = title {
                ui.label(RichText::new(cap(title)).font(t.font_small.clone()).color(t.ink_weak).strong());
            }
            add(ui)
        })
        .inner;
    ui.add_space(t.card_gap);
    out
}

/// A grid cell (MetalVJ master `cell`): fixed height, its wash slotted behind the
/// content once the rect is known, its outline painted over the content.
fn cell<R>(
    ui: &mut Ui,
    t: &Theme,
    width: Option<f32>,
    main: egui::Align,
    fill: impl FnOnce(egui::Rect) -> Vec<egui::Shape>,
    outline: impl FnOnce(egui::Rect) -> Vec<egui::Shape>,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let slot = ui.painter().add(egui::Shape::Noop);
    let width = width.unwrap_or_else(|| ui.available_width()).max(0.0);
    let out = ui.allocate_ui_with_layout(
        egui::vec2(width, t.cell_height()),
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(main),
        |ui| {
            ui.set_width(width);
            ui.set_min_height(t.cell_height());
            add(ui)
        },
    );
    ui.painter().set(slot, egui::Shape::Vec(fill(out.response.rect)));
    ui.painter().add(egui::Shape::Vec(outline(out.response.rect)));
    out.inner
}

/// KEY cell: header grey, sharp corners, a tinted shadow behind it.
pub fn key_cell<R>(ui: &mut Ui, t: &Theme, width: Option<f32>, tint: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    cell(ui, t, width, egui::Align::Min, |r| t.key_fill_tinted(r, tint), |r| t.key_outline(r), add)
}

/// PAYLOAD cell: glass wash and black outline; controls inside take the payload style.
pub fn payload_cell<R>(ui: &mut Ui, t: &Theme, width: Option<f32>, add: impl FnOnce(&mut Ui) -> R) -> R {
    cell(ui, t, width, egui::Align::Center, |r| t.payload_fill(r), |r| t.payload_outline(r), |ui| {
        t.payload_style(&mut ui.style_mut().visuals);
        let was = IN_PAYLOAD.with(|c| c.replace(true));
        let out = add(ui);
        IN_PAYLOAD.with(|c| c.set(was));
        out
    })
}

thread_local! {
    /// Set while a payload cell's content is being laid out (dropdowns centre themselves there).
    static IN_PAYLOAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn key_label(ui: &mut Ui, t: &Theme, label: &str, tint: Color32) {
    key_cell(ui, t, Some(t.label_width), tint, |ui| {
        let rect = ui.max_rect().shrink2(egui::vec2(t.field_pad.x * 0.6, 0.0));
        ui.put(rect, egui::Label::new(RichText::new(cap(label)).font(t.font_small.clone()).color(t.ink_weak).strong()).truncate());
    });
}

/// KEY | PAYLOAD row on the kit's columns, the key shadow in the primary.
pub fn label_row<R>(ui: &mut Ui, t: &Theme, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    label_row_tinted(ui, t, label, t.primary, add)
}

/// KEY | PAYLOAD row whose key shadow takes `tint` (orange rows in to-do).
pub fn label_row_tinted<R>(ui: &mut Ui, t: &Theme, label: &str, tint: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        key_label(ui, t, label, tint);
        payload_cell(ui, t, None, add)
    })
    .inner
}

/// Equal sub-cells across a payload cell (W×H, several buttons …).
pub fn payload_split<R>(ui: &mut Ui, t: &Theme, n: usize, mut add: impl FnMut(&mut Ui, usize) -> R) -> Vec<R> {
    if n == 0 {
        return vec![];
    }
    let each = (ui.available_width() / n as f32).max(0.0);
    let h = t.cell_height();
    let mut out = Vec::with_capacity(n);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        for i in 0..n {
            let sub = ui.allocate_ui_with_layout(egui::vec2(each, h), egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                ui.set_width(each);
                ui.set_min_height(h);
                add(ui, i)
            });
            ui.painter().add(egui::Shape::Vec(t.payload_outline(sub.response.rect)));
            out.push(sub.inner);
        }
    });
    out
}

/// Several buttons as equal sub-cells of one payload; returns which were clicked.
pub fn payload_buttons(ui: &mut Ui, t: &Theme, labels: Vec<RichText>) -> Vec<bool> {
    payload_split(ui, t, labels.len(), |ui, i| {
        let w = ui.available_width().max(t.interact_size.x);
        ui.add_sized([w, t.cell_height()], egui::Button::new(labels[i].clone())).clicked()
    })
}

/// A number field spanning its payload cell, value centred.
pub fn drag_cell(ui: &mut Ui, t: &Theme, dv: egui::DragValue<'_>) -> egui::Response {
    let w = ui.available_width().max(t.interact_size.x);
    ui.add_sized([w, t.cell_height()], dv)
}

/// A button spanning its payload cell.
pub fn payload_button(ui: &mut Ui, t: &Theme, text: RichText) -> egui::Response {
    let w = ui.available_width().max(t.interact_size.x);
    ui.add_sized([w, t.cell_height()], egui::Button::new(text))
}

/// The compact subheader under a panel head: a row of the panel's controls on the glass wash.
pub fn subheader<R>(ui: &mut Ui, t: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    cell(ui, t, None, egui::Align::Min, |r| t.payload_fill(r), |_| vec![], |ui| {
        ui.add_space(t.field_pad.x);
        ui.horizontal(add).inner
    })
}

/// Centred bold notice over the glass (MetalVJ `notice`).
pub fn notice(ui: &mut Ui, t: &Theme, text: &str) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(RichText::new(cap(text)).font(t.font_small.clone()).color(t.ink).strong()).wrap());
    });
}

/// A read-only key/value row.
pub fn field_row(ui: &mut Ui, t: &Theme, label: &str, value: &str) {
    label_row(ui, t, label, |ui| centered_label(ui, small_text(t, value).color(t.ink)));
}

/// A payload's text, centred in its cell (truncated, the whole text on hover).
pub fn centered_label(ui: &mut Ui, text: RichText) -> egui::Response {
    ui.centered_and_justified(|ui| ui.add(egui::Label::new(text).truncate())).inner
}

/// A single-line text field on the control column.
pub fn text_row(ui: &mut Ui, t: &Theme, label: &str, value: &mut String, hint: &str) -> egui::Response {
    label_row(ui, t, label, |ui| {
        ui.add(egui::TextEdit::singleline(value).hint_text(small_text(t, hint).color(t.ink_faint)).font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY))
    })
}

pub fn toggle_row(ui: &mut Ui, t: &Theme, label: &str, value: &mut bool) -> egui::Response {
    label_row(ui, t, label, |ui| {
        let text = small_text(t, if *value { "on" } else { "off" });
        let w = ui.spacing().interact_size.y + ui.spacing().icon_spacing + 24.0;
        ui.add_space(((ui.available_width() - w) * 0.5).max(0.0));
        ui.checkbox(value, text)
    })
}

/// The AI button (MetalVJ `ai_button`): a compact payload cell with the label in the AI
/// colour. On hover it takes the AI outline and a soft halo; corners stay square.
pub fn ai_button(ui: &mut Ui, t: &Theme, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(cap(label), t.font_small.clone(), t.primary_lit);
    let size = Vec2::new(galley.size().x + 2.0 * t.field_pad.x, t.cell_height());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let hot = resp.hovered();
    if hot {
        painter.rect_stroke(rect.expand(2.0), 0.0, t.ai_glow_stroke(), egui::StrokeKind::Outside);
    }
    painter.extend(t.payload_fill(rect));
    if hot {
        painter.rect_stroke(rect, 0.0, t.ai_stroke(), egui::StrokeKind::Inside);
    } else {
        painter.extend(t.payload_outline(rect));
    }
    let at = rect.center() - galley.size() * 0.5;
    painter.galley(at, galley, t.primary_lit);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A dropdown over `(value, label)` options.
pub fn dropdown<T: PartialEq + Clone>(ui: &mut Ui, t: &Theme, id: impl std::hash::Hash + std::fmt::Debug, current: &mut T, options: &[(T, String)]) -> bool {
    let shown = options.iter().find(|(v, _)| v == current).map(|(_, l)| l.clone()).unwrap_or_default();
    let mut picked = None;
    // in a payload cell the dropdown takes the whole cell; elsewhere it stays compact
    let in_payload = IN_PAYLOAD.with(|c| c.get());
    let w = if in_payload { ui.available_width() } else { ui.available_width().min(260.0) };
    egui::ComboBox::from_id_salt(Id::new(id))
        .width(w)
        .selected_text(small_text(t, cap(&shown)))
        .show_ui(ui, |ui| {
            for (v, l) in options {
                if ui.selectable_label(v == current, small_text(t, cap(l))).clicked() {
                    picked = Some(v.clone());
                }
            }
        });
    match picked {
        Some(v) if v != *current => {
            *current = v;
            true
        }
        _ => false,
    }
}

/// A dropdown of count groups: groups with 0 items disappear; the label shows the count.
/// If the current group empties, it falls back to the first non-empty one.
pub fn group_dropdown(ui: &mut Ui, t: &Theme, id: &str, current: &mut String, groups: &[(&str, usize)]) {
    let visible: Vec<(String, String)> = groups
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(g, n)| (g.to_string(), format!("{g} · {n}")))
        .collect();
    if visible.is_empty() {
        ui.label(small_text(t, "empty").color(t.ink_faint));
        return;
    }
    if !visible.iter().any(|(g, _)| g == current) {
        *current = visible[0].0.clone();
    }
    dropdown(ui, t, id, current, &visible);
}

/// A button in the small face; `accent` paints its text.
pub fn button(ui: &mut Ui, t: &Theme, text: &str, accent: Option<Color32>) -> egui::Response {
    let mut rt = small_text(t, cap(text));
    if let Some(c) = accent {
        rt = rt.color(c);
    }
    ui.add(egui::Button::new(rt))
}

/// A frameless text button (tabs, row actions).
pub fn link(ui: &mut Ui, t: &Theme, text: &str, color: Color32) -> egui::Response {
    ui.add(egui::Button::new(small_text(t, cap(text)).color(color)).frame(false))
}

/// The header's tab strip: the selected tab in its own colour, the rest weak.
pub fn tab(ui: &mut Ui, t: &Theme, on: bool, text: &str, color: Color32) -> bool {
    let rt = RichText::new(cap(text)).font(t.font_body.clone());
    let rt = if on { rt.color(color).strong() } else { rt.color(t.ink_weak) };
    let r = ui.add(egui::Button::new(rt).frame(false));
    if on {
        let rect = r.rect;
        ui.painter().hline(rect.x_range(), rect.bottom() + 2.0, egui::Stroke::new(2.0, color));
    }
    r.clicked()
}


// chat kit
/// An `@` reference drawn as a small purple chip inside a message.
pub fn chip(ui: &mut Ui, t: &Theme, text: &str) {
    Frame::new()
        .fill(t.primary_deep)
        .stroke(egui::Stroke::new(t.hairline, t.primary_dim))
        .inner_margin(egui::Margin::symmetric(4, 0))
        .show(ui, |ui| {
            ui.label(small_text(t, text).color(t.primary_lit));
        });
}

// library kit (MetalVJ `library.rs` file_like_row): one row per folder or file

/// The glyph a library row paints in its badge.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RowIcon {
    Up,
    Folder,
    File,
    Video,
    Audio,
    Image,
    Doc,
    Shield,
    Plug,
    People,
    Badge,
    Bot,
    Log,
    /// A Lucide glyph (MetalVJ's icon pack, `assets/lucide.ttf`) by codepoint.
    Lucide(char),
}

/// The Lucide icon font, vendored from MetalVJ (ISC; see `assets/LUCIDE-LICENSE`).
pub const LUCIDE_FONT: &[u8] = include_bytes!("../../../assets/lucide.ttf");
/// The family the Lucide glyphs are registered under.
pub const LUCIDE: &str = "lucide";

/// Registers the Lucide face on `ctx`. Call once, before the first frame.
pub fn install_icon_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(LUCIDE.into(), std::sync::Arc::new(egui::FontData::from_static(LUCIDE_FONT)));
    fonts.families.insert(egui::FontFamily::Name(LUCIDE.into()), vec![LUCIDE.into()]);
    ctx.set_fonts(fonts);
}

pub const LIBRARY_ROW_H: f32 = 48.0;
const LIBRARY_BADGE: f32 = 24.0;
const LIBRARY_PAD: f32 = 8.0;

fn paint_row_icon(p: &egui::Painter, icon: RowIcon, r: egui::Rect, s: egui::Stroke) {
    use egui::{pos2, vec2};
    let c = r.center();
    match icon {
        RowIcon::Up => {
            p.line_segment([c + vec2(-7.0, 2.0), c + vec2(0.0, -5.0)], s);
            p.line_segment([c + vec2(7.0, 2.0), c + vec2(0.0, -5.0)], s);
            p.line_segment([c + vec2(0.0, -5.0), c + vec2(0.0, 8.0)], s);
        }
        RowIcon::Folder => {
            let pts = vec![c + vec2(-10.0, -7.0), c + vec2(-3.0, -7.0), c + vec2(-1.0, -4.0), c + vec2(10.0, -4.0), c + vec2(10.0, 8.0), c + vec2(-10.0, 8.0)];
            p.add(egui::Shape::closed_line(pts, s));
        }
        RowIcon::Video => {
            p.rect_stroke(egui::Rect::from_center_size(c, vec2(20.0, 14.0)), 0.0, s, egui::StrokeKind::Middle);
            p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.0, -4.0), c + vec2(5.0, 0.0), c + vec2(-3.0, 4.0)], s.color, egui::Stroke::NONE));
        }
        RowIcon::Audio => {
            for (k, h) in [5.0f32, 11.0, 16.0, 9.0, 4.0].iter().enumerate() {
                let x = c.x - 8.0 + k as f32 * 4.0;
                p.line_segment([pos2(x, c.y - h * 0.5), pos2(x, c.y + h * 0.5)], s);
            }
        }
        RowIcon::Image => {
            p.rect_stroke(egui::Rect::from_center_size(c, vec2(20.0, 16.0)), 0.0, s, egui::StrokeKind::Middle);
            p.add(egui::Shape::line(vec![c + vec2(-8.0, 6.0), c + vec2(-2.0, -1.0), c + vec2(2.0, 3.0), c + vec2(5.0, 0.0), c + vec2(8.0, 6.0)], s));
        }
        RowIcon::Shield => {
            p.add(egui::Shape::closed_line(vec![c + vec2(-8.0, -8.0), c + vec2(8.0, -8.0), c + vec2(8.0, 1.0), c + vec2(0.0, 9.0), c + vec2(-8.0, 1.0)], s));
            p.line_segment([c + vec2(-3.0, 0.0), c + vec2(-1.0, 3.0)], s);
            p.line_segment([c + vec2(-1.0, 3.0), c + vec2(4.0, -3.0)], s);
        }
        RowIcon::Plug => {
            p.rect_stroke(egui::Rect::from_center_size(c + vec2(0.0, 2.0), vec2(14.0, 9.0)), 0.0, s, egui::StrokeKind::Middle);
            p.line_segment([c + vec2(-3.0, -2.0), c + vec2(-3.0, -8.0)], s);
            p.line_segment([c + vec2(3.0, -2.0), c + vec2(3.0, -8.0)], s);
            p.line_segment([c + vec2(0.0, 6.5), c + vec2(0.0, 10.0)], s);
        }
        RowIcon::People => {
            p.circle_stroke(c + vec2(-4.0, -4.0), 3.5, s);
            p.add(egui::Shape::line(vec![c + vec2(-10.0, 8.0), c + vec2(-9.0, 2.0), c + vec2(1.0, 2.0), c + vec2(2.0, 8.0)], s));
            p.circle_stroke(c + vec2(6.0, -3.0), 2.8, s);
            p.line_segment([c + vec2(5.0, 2.0), c + vec2(10.0, 3.0)], s);
        }
        RowIcon::Badge => {
            p.rect_stroke(egui::Rect::from_center_size(c, vec2(18.0, 13.0)), 2.0, s, egui::StrokeKind::Middle);
            p.circle_stroke(c + vec2(-4.0, -1.0), 2.5, s);
            p.line_segment([c + vec2(1.0, -2.0), c + vec2(6.0, -2.0)], s);
            p.line_segment([c + vec2(1.0, 2.0), c + vec2(6.0, 2.0)], s);
        }
        RowIcon::Bot => {
            p.rect_stroke(egui::Rect::from_center_size(c + vec2(0.0, 1.0), vec2(16.0, 12.0)), 2.0, s, egui::StrokeKind::Middle);
            p.circle_filled(c + vec2(-3.5, 0.0), 1.5, s.color);
            p.circle_filled(c + vec2(3.5, 0.0), 1.5, s.color);
            p.line_segment([c + vec2(0.0, -5.0), c + vec2(0.0, -9.0)], s);
        }
        RowIcon::Log => {
            p.rect_stroke(egui::Rect::from_center_size(c, vec2(16.0, 19.0)), 0.0, s, egui::StrokeKind::Middle);
            for k in 0..3 {
                let y = c.y - 5.0 + k as f32 * 4.5;
                p.line_segment([pos2(c.x - 4.0, y), pos2(c.x + 5.0, y)], s);
                p.circle_filled(pos2(c.x - 6.0 + 1.0, y), 0.8, s.color);
            }
        }
        RowIcon::Lucide(cp) => {
            p.text(c, egui::Align2::CENTER_CENTER, cp, egui::FontId::new(18.0, egui::FontFamily::Name(LUCIDE.into())), s.color);
        }
        RowIcon::File | RowIcon::Doc => {
            let page = egui::Rect::from_center_size(c, vec2(15.0, 19.0));
            p.rect_stroke(page, 0.0, s, egui::StrokeKind::Middle);
            if icon == RowIcon::Doc {
                for k in 0..3 {
                    let y = page.top() + 5.5 + k as f32 * 4.0;
                    p.line_segment([pos2(page.left() + 3.0, y), pos2(page.right() - 3.0, y)], s);
                }
            }
        }
    }
}

/// A library row: full width, 48 px, darker than the panel at rest, the hover
/// wash, a bright hairline outline, the glyph in a badge, a title (truncated,
/// whole on hover) and an optional weak subtitle, centred as a block. The
/// selected row takes the accent wash and a left accent bar. The whole row is the button.
pub fn library_row(ui: &mut Ui, t: &Theme, icon: RowIcon, title: &str, subtitle: Option<&str>, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, LIBRARY_ROW_H), Sense::hover());
    let resp = ui.interact(rect, ui.id().with(("library-row", icon, title)), Sense::click());
    let p = ui.painter();
    let fill = if selected { t.primary_deep } else if resp.hovered() { t.hover_card() } else { t.inset };
    p.rect_filled(rect, 0.0, fill);
    if selected {
        p.rect_filled(egui::Rect::from_min_size(rect.left_top(), Vec2::new(t.accent_stroke.max(2.0), rect.height())), 0.0, t.primary);
    }
    p.rect_stroke(rect, 0.0, egui::Stroke::new(t.hairline, t.rule_bright), egui::StrokeKind::Inside);
    let slot = egui::Rect::from_min_size(egui::pos2(rect.left() + LIBRARY_PAD, rect.center().y - LIBRARY_BADGE / 2.0), Vec2::splat(LIBRARY_BADGE));
    let ink = match icon {
        RowIcon::Folder | RowIcon::Up => t.primary_lit,
        RowIcon::Video | RowIcon::Audio => t.secondary,
        _ => t.ink_weak,
    };
    paint_row_icon(p, icon, slot, egui::Stroke::new(1.3, ink));
    let title_h = ui.ctx().fonts_mut(|f| f.row_height(&t.font_body));
    let block_h = title_h + subtitle.map_or(0.0, |_| 2.0 + ui.ctx().fonts_mut(|f| f.row_height(&t.font_small)));
    let top = (rect.center().y - block_h / 2.0).max(rect.top());
    let text = egui::Rect::from_min_max(egui::pos2(slot.right() + t.item_gap.x + 2.0, top), egui::pos2(rect.right() - LIBRARY_PAD, (top + block_h).min(rect.bottom())));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text).layout(egui::Layout::top_down(egui::Align::LEFT)));
    child.spacing_mut().item_spacing.y = 2.0;
    child.add(egui::Label::new(RichText::new(cap(title)).font(t.font_body.clone()).color(if selected { t.primary_lit } else { t.ink })).truncate().selectable(false));
    if let Some(sub) = subtitle {
        child.add(egui::Label::new(RichText::new(cap(sub)).font(t.font_small.clone()).color(t.ink_weak)).truncate().selectable(false));
    }
    resp.on_hover_text(title).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The library's search box in a panel head.
pub fn search_box(ui: &mut Ui, t: &Theme, value: &mut String, hint: &str) -> egui::Response {
    ui.add(egui::TextEdit::singleline(value).hint_text(small_text(t, hint).color(t.ink_faint)).font(t.font_small.clone()).desired_width(f32::INFINITY))
}

/// A model menu: a button showing the current `provider:model`; its popup lists the
/// configured providers and each opens a submenu of that provider's common models
/// with a search bar. Returns true when the pick changed (`current` is then updated).
pub fn model_picker(ui: &mut Ui, t: &Theme, cfg: &ventosa::config::Config, current: &mut String) -> bool {
    let provs = ventosa::models::configured(cfg);
    let shown = ventosa::models::label(current);
    let btn = ui.add(egui::Button::new(small_text(t, format!("{shown} \u{25be}")).color(t.primary_lit)));
    let mut picked: Option<String> = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_min_width(220.0);
        if ui.selectable_label(current.is_empty(), small_text(t, "app default")).clicked() {
            picked = Some(String::new());
            ui.close();
        }
        if provs.is_empty() {
            ui.label(small_text(t, "No provider key set \u{2014} export one (settings \u{25b8} AI provider)").color(t.warn));
        }
        for info in provs {
            ui.menu_button(small_text(t, format!("{} \u{25b8}", info.name)), |ui| {
                ui.set_min_width(260.0);
                let qid = Id::new(("model-q", info.id));
                let mut q: String = ui.data_mut(|d| d.get_temp(qid)).unwrap_or_default();
                ui.add(egui::TextEdit::singleline(&mut q).hint_text("search models\u{2026}").font(t.font_small.clone()).desired_width(f32::INFINITY));
                ui.data_mut(|d| d.insert_temp(qid, q.clone()));
                let ql = q.to_lowercase();
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for m in ventosa::models::models_of(cfg, info).into_iter().filter(|m| ql.is_empty() || m.to_lowercase().contains(&ql)) {
                        let spec = format!("{}:{m}", info.id);
                        if ui.selectable_label(*current == spec, small_text(t, &m)).clicked() {
                            picked = Some(spec);
                            ui.close();
                        }
                    }
                });
            });
        }
    });
    match picked {
        Some(v) if v != *current => {
            *current = v;
            true
        }
        _ => false,
    }
}

/// A square Lucide glyph button, the one the floating graph bars are built from (MetalVJ `icon_button`).
pub fn icon_button(ui: &mut Ui, t: &Theme, glyph: char, on: bool, enabled: bool, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(tip);
    let fill = if on { t.primary_deep } else if resp.hovered() && enabled { t.hover_card() } else { egui::Color32::TRANSPARENT };
    let ink = if !enabled { t.ink_faint } else if on { t.primary_lit } else if resp.hovered() { t.ink } else { t.ink_weak };
    let p = ui.painter();
    p.rect_filled(rect, t.field_corner(), fill);
    if on {
        p.rect_stroke(rect, t.field_corner(), egui::Stroke::new(t.hairline, t.primary), egui::StrokeKind::Inside);
    }
    p.text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::new(15.0, egui::FontFamily::Name(LUCIDE.into())), ink);
    resp
}
