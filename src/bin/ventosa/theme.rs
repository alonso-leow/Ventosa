//! The Ventosa theme: MetalVJ's "phosphor" theme at the component level
//! (same surfaces, rules, radii, spacing and monospace type scale), with the
//! emphasis moved from red/blue to deep purple (primary) and orange (secondary).
//! Every colour and size the pages use comes from here.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Pos2, Rect, Shadow, Shape, Stroke, StrokeKind, TextStyle, Vec2};

#[derive(Clone)]
pub struct Theme {
    // surfaces (MetalVJ phosphor)
    pub tube: Color32,
    pub tube_raised: Color32,
    pub panel: Color32,
    pub panel_head: Color32,
    pub inset: Color32,
    pub rule: Color32,
    pub rule_bright: Color32,
    pub stripe: Color32,
    // ink
    pub ink: Color32,
    pub ink_weak: Color32,
    pub ink_faint: Color32,
    // primary: deep purple (MetalVJ's red role)
    pub primary: Color32,
    pub primary_lit: Color32,
    pub primary_dim: Color32,
    pub primary_deep: Color32,
    // secondary: orange (MetalVJ's blue role) — the to-do colour
    pub secondary: Color32,
    pub secondary_dim: Color32,
    pub secondary_deep: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub glow: Color32,
    // node canvas
    pub node_canvas: Color32,
    pub node_body: Color32,
    pub node_header: Color32,
    pub node_cable: Color32,
    pub node_dot_off: Color32,
    // geometry
    pub hairline: f32,
    /// Outline width of an accent (AI hover, selected node).
    pub accent_stroke: f32,
    pub radius: u8,
    pub field_radius: u8,
    pub menu_radius: u8,
    pub glow_blur: u8,
    pub pad: i8,
    pub header_pad: i8,
    pub header_pad_y: i8,
    pub card_pad: i8,
    pub card_gap: f32,
    pub item_gap: Vec2,
    pub field_pad: Vec2,
    pub interact_size: Vec2,
    pub label_width: f32,
    pub control_width: f32,
    pub panel_head_height: f32,
    pub graph_grid: f32,
    // type
    pub font_heading: FontId,
    pub font_body: FontId,
    pub font_small: FontId,
}

impl Theme {
    pub fn ventosa() -> Self {
        Self {
            tube: Color32::from_rgb(0x08, 0x08, 0x0a),
            tube_raised: Color32::from_rgb(0x0d, 0x0d, 0x10),
            panel: Color32::from_rgb(0x11, 0x11, 0x14),
            panel_head: Color32::from_rgb(0x0a, 0x0a, 0x0d),
            inset: Color32::from_rgb(0x05, 0x05, 0x06),
            rule: Color32::from_rgb(0x1c, 0x1c, 0x20),
            rule_bright: Color32::from_rgb(0x2c, 0x2c, 0x33),
            stripe: Color32::from_rgb(0x16, 0x16, 0x1a),
            ink: Color32::from_rgb(0xe6, 0xe6, 0xe8),
            ink_weak: Color32::from_rgb(0x9a, 0x9a, 0xa2),
            ink_faint: Color32::from_rgb(0x62, 0x62, 0x6b),
            primary: Color32::from_rgb(0x8b, 0x3d, 0xff),
            primary_lit: Color32::from_rgb(0xb3, 0x8b, 0xff),
            primary_dim: Color32::from_rgb(0x5e, 0x24, 0xb8),
            primary_deep: Color32::from_rgb(0x34, 0x12, 0x6e),
            secondary: Color32::from_rgb(0xff, 0x8a, 0x1f),
            secondary_dim: Color32::from_rgb(0xb3, 0x5c, 0x10),
            secondary_deep: Color32::from_rgb(0x4a, 0x24, 0x06),
            ok: Color32::from_rgb(0x6f, 0xbf, 0x8f),
            warn: Color32::from_rgb(0xd9, 0xa4, 0x41),
            glow: Color32::from_rgba_unmultiplied(0x8b, 0x3d, 0xff, 0x24),
            node_canvas: Color32::from_rgb(0x05, 0x05, 0x08),
            node_body: Color32::from_rgb(0x1b, 0x1b, 0x20),
            node_header: Color32::from_rgb(0x2a, 0x2a, 0x31),
            node_cable: Color32::from_rgb(0x7f, 0x7f, 0x88),
            node_dot_off: Color32::from_rgb(0x6a, 0x6a, 0x72),
            hairline: 1.0,
            accent_stroke: 1.5,
            radius: 0,
            field_radius: 0,
            menu_radius: 0,
            glow_blur: 24,
            pad: 12,
            header_pad: 8,
            header_pad_y: 5,
            card_pad: 8,
            card_gap: 8.0,
            item_gap: Vec2::new(8.0, 6.0),
            field_pad: Vec2::new(10.0, 6.0),
            interact_size: Vec2::new(40.0, 30.0),
            label_width: 84.0,
            control_width: 180.0,
            panel_head_height: 26.0,
            graph_grid: 40.0,
            font_heading: FontId::new(13.0, FontFamily::Monospace),
            font_body: FontId::new(12.0, FontFamily::Monospace),
            font_small: FontId::new(10.0, FontFamily::Monospace),
        }
    }

    /// A brand accent from config overrides the primary (white label).
    pub fn with_primary(mut self, c: Color32) -> Self {
        if c != self.primary {
            self.primary = c;
            self.primary_lit = c.lerp_to_gamma(Color32::WHITE, 0.35);
            self.primary_dim = c.lerp_to_gamma(Color32::BLACK, 0.35);
            self.primary_deep = c.lerp_to_gamma(Color32::BLACK, 0.65);
            self.glow = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 0x24);
        }
        self
    }

    /// The secondary emphasis from config (the to-do / attention colour).
    pub fn with_secondary(mut self, c: Color32) -> Self {
        if c != self.secondary {
            self.secondary = c;
            self.secondary_dim = c.lerp_to_gamma(Color32::BLACK, 0.3);
            self.secondary_deep = c.lerp_to_gamma(Color32::BLACK, 0.71);
        }
        self
    }

    /// MetalVJ `library_card_hover`.
    pub fn hover_card(&self) -> Color32 {
        Color32::from_rgb(0x21, 0x21, 0x28)
    }

    /// The AI accent outline (MetalVJ `ai_stroke`): purple lit.
    pub fn ai_stroke(&self) -> Stroke {
        Stroke::new(self.accent_stroke, self.primary_lit)
    }
    /// The soft halo behind an AI accent (translucent purple).
    pub fn ai_glow_stroke(&self) -> Stroke {
        Stroke::new(self.glow_blur as f32 * 0.25, self.glow)
    }
    pub fn rule_stroke(&self) -> Stroke {
        Stroke::new(self.hairline, self.rule)
    }
    pub fn rule_bright_stroke(&self) -> Stroke {
        Stroke::new(self.hairline, self.rule_bright)
    }
    pub fn corner_radius(&self) -> CornerRadius {
        CornerRadius::same(self.radius)
    }
    pub fn field_corner(&self) -> CornerRadius {
        CornerRadius::same(self.field_radius)
    }
    pub fn panel_margin(&self) -> Margin {
        Margin::same(self.pad)
    }
    pub fn header_margin(&self) -> Margin {
        Margin { left: self.header_pad, right: self.header_pad, top: self.header_pad_y, bottom: self.header_pad_y }
    }
    pub fn card_shadow(&self) -> Shadow {
        Shadow { offset: [0, 2], blur: self.glow_blur, spread: 0, color: self.glow }
    }

    /// The wordmark's face: the heading face, larger (MetalVJ `logo_font`).
    pub fn logo_font(&self) -> FontId {
        FontId::new(self.font_heading.size * 1.5, self.font_heading.family.clone())
    }

    // --- the glass cell classes (MetalVJ master, 2026-10-31) ---
    pub fn cell_height(&self) -> f32 {
        self.interact_size.y
    }
    pub fn glass_fill(&self) -> Color32 {
        let p = self.panel;
        Color32::from_rgba_unmultiplied(p.r(), p.g(), p.b(), 170)
    }
    fn grid_stroke(&self) -> Stroke {
        Stroke::new(self.hairline, Color32::BLACK)
    }
    fn glass_layers(&self, rect: Rect) -> Vec<Shape> {
        let r = CornerRadius::ZERO;
        let h = self.hairline;
        let lower = Rect::from_min_max(Pos2::new(rect.min.x, rect.center().y), rect.max);
        vec![
            Shape::rect_filled(rect, r, self.glass_fill()),
            Shape::rect_filled(lower, r, Color32::from_white_alpha(8)),
            Shape::line_segment([Pos2::new(rect.min.x + h, rect.min.y + h), Pos2::new(rect.max.x - h, rect.min.y + h)], Stroke::new(h, Color32::from_white_alpha(34))),
            Shape::line_segment([Pos2::new(rect.min.x + h, rect.max.y - h), Pos2::new(rect.max.x - h, rect.max.y - h)], Stroke::new(h, Color32::from_black_alpha(70))),
        ]
    }
    pub fn payload_fill(&self, rect: Rect) -> Vec<Shape> {
        self.glass_layers(rect)
    }
    pub fn payload_outline(&self, rect: Rect) -> Vec<Shape> {
        vec![Shape::rect_stroke(rect, CornerRadius::ZERO, self.grid_stroke(), StrokeKind::Inside)]
    }
    /// The KEY cell: header grey over a shadow in `tint` (purple for AI rows, orange for to-do).
    pub fn key_fill_tinted(&self, rect: Rect, tint: Color32) -> Vec<Shape> {
        let shadow = Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 150);
        vec![
            Shape::rect_filled(rect.translate(Vec2::splat(self.hairline * 2.0)), CornerRadius::ZERO, shadow),
            Shape::rect_filled(rect, CornerRadius::ZERO, self.panel_head),
        ]
    }
    pub fn key_outline(&self, rect: Rect) -> Vec<Shape> {
        self.payload_outline(rect)
    }
    pub fn group_shapes(&self, rect: Rect) -> Vec<Shape> {
        let mut shapes = self.glass_layers(rect);
        shapes.push(Shape::rect_stroke(rect, CornerRadius::ZERO, self.rule_stroke(), StrokeKind::Inside));
        shapes
    }
    /// Widgets inside a payload cell: sharp corners, black outline, glass resting fill.
    pub fn payload_style(&self, visuals: &mut egui::Visuals) {
        let outline = self.grid_stroke();
        for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
            w.corner_radius = CornerRadius::ZERO;
            w.bg_stroke = outline;
        }
        visuals.widgets.inactive.weak_bg_fill = self.glass_fill();
        visuals.widgets.inactive.bg_fill = self.glass_fill();
        visuals.menu_corner_radius = CornerRadius::ZERO;
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let t = self.clone();
        ctx.all_styles_mut(move |style| t.write_style(style));
    }

    fn write_style(&self, style: &mut egui::Style) {
        style.spacing.item_spacing = self.item_gap;
        style.spacing.button_padding = self.field_pad;
        style.spacing.interact_size = self.interact_size;
        style.spacing.window_margin = self.panel_margin();
        style.spacing.menu_margin = self.panel_margin();
        style.text_styles = [
            (TextStyle::Heading, self.font_heading.clone()),
            (TextStyle::Body, self.font_body.clone()),
            (TextStyle::Button, self.font_body.clone()),
            (TextStyle::Monospace, self.font_body.clone()),
            (TextStyle::Small, self.font_small.clone()),
        ]
        .into_iter()
        .collect();

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = Some(self.ink);
        v.weak_text_color = Some(self.ink_weak);
        v.hyperlink_color = self.primary_lit;
        v.panel_fill = self.panel;
        v.window_fill = self.tube_raised;
        v.window_stroke = self.rule_stroke();
        v.window_corner_radius = self.corner_radius();
        v.menu_corner_radius = CornerRadius::same(self.menu_radius);
        v.faint_bg_color = self.stripe;
        v.extreme_bg_color = self.inset;
        v.text_edit_bg_color = Some(self.inset);
        v.code_bg_color = self.inset;
        v.warn_fg_color = self.warn;
        v.error_fg_color = self.primary;
        v.selection.bg_fill = self.primary_deep;
        v.selection.stroke = Stroke::new(self.hairline, self.primary_dim);
        v.text_cursor.stroke = Stroke::new(self.hairline, self.primary);
        v.window_shadow = self.card_shadow();
        v.popup_shadow = self.card_shadow();
        let w = &mut v.widgets;
        for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            ws.bg_fill = self.panel;
            ws.weak_bg_fill = self.tube_raised;
            ws.bg_stroke = self.rule_stroke();
            ws.fg_stroke = Stroke::new(self.hairline, self.ink);
            ws.corner_radius = self.field_corner();
            ws.expansion = 0.0;
        }
        w.noninteractive.fg_stroke = Stroke::new(self.hairline, self.ink_weak);
        w.hovered.weak_bg_fill = self.primary_deep;
        w.hovered.bg_stroke = self.rule_bright_stroke();
        w.hovered.expansion = self.hairline;
        w.active.bg_fill = self.primary;
        w.active.weak_bg_fill = self.primary;
        w.active.bg_stroke = Stroke::new(self.hairline, self.primary);
        w.active.fg_stroke = Stroke::new(self.hairline, self.tube);
        w.open.weak_bg_fill = self.primary_deep;
        w.open.bg_stroke = self.rule_bright_stroke();
    }
}

pub fn hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}
