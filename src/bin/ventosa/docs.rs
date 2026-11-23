//! The docs view: the wiki's index on a narrow left column (groups, articles,
//! their sections), the article on the right, rendered from Markdown; with
//! docs-edit rights, an editor, save, revert and new article.

use super::kit::{self, small_text};
use super::{pages, App};
use eframe::egui::{self, RichText, Ui};
use ventosa::wiki;

#[derive(Default)]
pub struct DocsState {
    pub slug: String,
    /// The section to scroll to once rendered.
    jump: Option<String>,
    query: String,
    editing: bool,
    draft: String,
    new_slug: String,
}

pub fn view(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let articles = wiki::all();
    if app.docs.slug.is_empty() || !articles.iter().any(|a| a.slug == app.docs.slug) {
        app.docs.slug = articles.first().map(|a| a.slug.clone()).unwrap_or_default();
    }
    pages::two_columns(ui, app, "docs", 0.24, |ui, app| {
        pages::panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                ui.label(RichText::new("Docs").font(t.font_heading.clone()).color(t.primary));
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut app.docs.query).hint_text(small_text(&t, "Search\u{2026}").color(t.ink_faint)).font(t.font_small.clone()).desired_width(f32::INFINITY));
            });
            pages::body(ui, &t, "docs-index", |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if !app.docs.query.trim().is_empty() {
                    for (slug, line) in wiki::search(app.docs.query.trim()) {
                        if kit::library_row(ui, &t, kit::RowIcon::Doc, &slug, Some(&line), app.docs.slug == slug).clicked() {
                            app.docs.slug = slug;
                            app.docs.editing = false;
                        }
                    }
                    return;
                }
                let mut groups: Vec<String> = articles.iter().map(|a| a.group.clone()).collect();
                groups.dedup();
                let mut seen = std::collections::HashSet::new();
                groups.retain(|g| seen.insert(g.clone()));
                for g in groups {
                    ui.add_space(6.0);
                    ui.label(small_text(&t, g.as_str()).color(t.ink_faint).strong());
                    ui.add_space(2.0);
                    for a in articles.iter().filter(|a| a.group == g) {
                        let on = app.docs.slug == a.slug;
                        let sub = if a.custom { Some(if a.builtin { "edited" } else { "yours" }) } else { None };
                        if kit::library_row(ui, &t, kit::RowIcon::Doc, &a.title, sub, on).clicked() {
                            app.docs.slug = a.slug.clone();
                            app.docs.editing = false;
                        }
                        if on {
                            for s in wiki::sections(&a.body) {
                                let r = ui.add(egui::Button::new(small_text(&t, format!("   \u{00b7} {s}")).color(t.ink_weak)).frame(false));
                                if r.clicked() {
                                    app.docs.jump = Some(s.clone());
                                }
                            }
                        }
                    }
                }
                if app.access.can_edit_docs() {
                    ui.add_space(10.0);
                    kit::text_row(ui, &t, "new", &mut app.docs.new_slug, "article-slug");
                    if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Create article").color(t.primary_lit))).clicked() {
                        let slug = app.docs.new_slug.trim().to_lowercase().replace(' ', "-");
                        let body = format!("# {}\n<!-- group: team -->\n\nWrite here.\n", slug.replace('-', " "));
                        match wiki::save(&slug, &body) {
                            Ok(()) => {
                                app.docs.slug = slug;
                                app.docs.draft = body;
                                app.docs.editing = true;
                                app.docs.new_slug.clear();
                            }
                            Err(e) => app.set_flash(e),
                        }
                    }
                }
            });
        });
    }, |ui, app| {
        let Some(a) = wiki::get(&app.docs.slug) else {
            kit::notice(ui, &t, "No article");
            return;
        };
        pages::panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                ui.add(egui::Label::new(RichText::new(&a.title).font(t.font_heading.clone()).color(t.ink)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !app.access.can_edit_docs() {
                        return;
                    }
                    if app.docs.editing {
                        if kit::link(ui, &t, "Save", t.primary).clicked() {
                            match wiki::save(&a.slug, &app.docs.draft) {
                                Ok(()) => {
                                    app.docs.editing = false;
                                    ventosa::identity::audit(app.access.name(), "docs.save", &a.slug, true);
                                }
                                Err(e) => app.set_flash(e),
                            }
                        }
                        if kit::link(ui, &t, "Cancel", t.ink_weak).clicked() {
                            app.docs.editing = false;
                        }
                    } else {
                        if kit::link(ui, &t, "Edit", t.primary_lit).clicked() {
                            app.docs.draft = a.body.clone();
                            app.docs.editing = true;
                        }
                        if a.custom && kit::link(ui, &t, if a.builtin { "revert" } else { "delete" }, t.ink_faint).on_hover_text("drop your copy").clicked() {
                            let _ = wiki::revert(&a.slug);
                        }
                    }
                });
            });
            pages::body(ui, &t, &format!("doc-{}", a.slug), |ui| {
                if app.docs.editing {
                    ui.add(egui::TextEdit::multiline(&mut app.docs.draft).font(egui::FontId::monospace(12.0)).code_editor().desired_width(f32::INFINITY).desired_rows(30));
                } else {
                    let jump = app.docs.jump.take();
                    markdown(ui, &t, &a.body, jump.as_deref());
                }
            });
        });
    });
}

/// A small Markdown renderer: headings, paragraphs, lists, code fences, tables,
/// images (files under the wiki folders; a slot is drawn when the file is missing).
fn markdown(ui: &mut Ui, t: &super::theme::Theme, src: &str, jump: Option<&str>) {
    let mut lines = src.lines().peekable();
    while let Some(line) = lines.next() {
        let l = line.trim_end();
        if l.trim_start().starts_with("<!--") {
            continue;
        }
        if l.starts_with("```") {
            let mut code = String::new();
            for c in lines.by_ref() {
                if c.starts_with("```") {
                    break;
                }
                code.push_str(c);
                code.push('\n');
            }
            egui::Frame::new().fill(t.inset).stroke(t.rule_stroke()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let r = ui.add(egui::Label::new(RichText::new(code.trim_end()).font(egui::FontId::monospace(12.0)).color(t.ink)).sense(egui::Sense::click()));
                if r.on_hover_text("click to copy").clicked() {
                    ui.ctx().copy_text(code.trim_end().to_string());
                }
            });
            ui.add_space(6.0);
        } else if let Some(h) = l.strip_prefix("# ") {
            ui.label(RichText::new(h).font(egui::FontId::proportional(22.0)).color(t.ink).strong());
            ui.add_space(6.0);
        } else if let Some(h) = l.strip_prefix("## ") {
            ui.add_space(8.0);
            let r = ui.label(RichText::new(h).font(t.font_heading.clone()).color(t.primary_lit).strong());
            if jump == Some(h.trim()) {
                r.scroll_to_me(Some(egui::Align::TOP));
            }
            ui.add_space(2.0);
        } else if let Some(h) = l.strip_prefix("### ") {
            ui.label(RichText::new(h).font(t.font_body.clone()).color(t.ink).strong());
        } else if l.starts_with('|') {
            let mut rows = vec![l.to_string()];
            while lines.peek().is_some_and(|n| n.starts_with('|')) {
                rows.push(lines.next().unwrap_or_default().to_string());
            }
            let cells: Vec<Vec<String>> = rows
                .iter()
                .filter(|r| !r.replace(['|', '-', ' ', ':'], "").is_empty())
                .map(|r| r.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect())
                .collect();
            egui::Grid::new(ui.next_auto_id()).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for (k, row) in cells.iter().enumerate() {
                    for c in row {
                        let rt = RichText::new(c.replace('`', "")).font(t.font_small.clone());
                        ui.label(if k == 0 { rt.color(t.ink_weak).strong() } else { rt.color(t.ink) });
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
        } else if let Some(rest) = l.trim_start().strip_prefix("![") {
            let (alt, path) = rest.split_once("](").map(|(a, p)| (a, p.trim_end_matches(')'))).unwrap_or((rest, ""));
            match wiki::image_path(path) {
                Some(p) => {
                    let w = ui.available_width().min(720.0);
                    ui.add(egui::Image::new(format!("file://{}", p.display())).max_width(w).corner_radius(0.0)).on_hover_text(alt);
                }
                None => {
                    egui::Frame::new().stroke(egui::Stroke::new(1.0, t.rule_bright)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width().min(720.0));
                        ui.label(small_text(t, format!("screenshot \u{00b7} {alt}")).color(t.ink_weak));
                        ui.label(small_text(t, format!("drop a PNG at {}", wiki::user_dir().join(path).display())).color(t.ink_faint));
                    });
                }
            }
            ui.add_space(6.0);
        } else if l.trim().is_empty() {
            ui.add_space(6.0);
        } else {
            let trimmed = l.trim_start();
            let (bullet, text) = if let Some(r) = trimmed.strip_prefix("- ") {
                ("\u{2022} ", r)
            } else if trimmed.split_once(". ").is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty()) {
                let (n, r) = trimmed.split_once(". ").unwrap_or(("", trimmed));
                return_num(ui, t, n, r);
                continue;
            } else {
                ("", trimmed)
            };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if !bullet.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(bullet).font(t.font_body.clone()).color(t.primary_lit));
                }
                inline(ui, t, text);
            });
        }
    }
}

fn return_num(ui: &mut Ui, t: &super::theme::Theme, n: &str, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(8.0);
        ui.label(RichText::new(format!("{n}. ")).font(t.font_body.clone()).color(t.primary_lit));
        inline(ui, t, text);
    });
}

/// `**bold**` and `` `code` `` spans inside a line.
fn inline(ui: &mut Ui, t: &super::theme::Theme, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let next = [rest.find('*'), rest.find('`')].into_iter().flatten().min();
        let Some(at) = next else {
            ui.label(RichText::new(rest).font(t.font_body.clone()).color(t.ink));
            break;
        };
        if at > 0 {
            ui.label(RichText::new(&rest[..at]).font(t.font_body.clone()).color(t.ink));
        }
        let tail = &rest[at..];
        if let Some(body) = tail.strip_prefix("**") {
            let end = body.find("**").unwrap_or(body.len());
            ui.label(RichText::new(&body[..end]).font(t.font_body.clone()).color(t.ink).strong());
            rest = body.get(end + 2..).unwrap_or("");
        } else if let Some(body) = tail.strip_prefix('*') {
            // *a reference to another article*: italic, in the link colour
            let end = body.find('*').unwrap_or(body.len());
            ui.label(RichText::new(&body[..end]).font(t.font_body.clone()).color(t.primary_lit).italics());
            rest = body.get(end + 1..).unwrap_or("");
        } else {
            let body = &tail[1..];
            let end = body.find('`').unwrap_or(body.len());
            ui.label(RichText::new(&body[..end]).font(egui::FontId::monospace(12.0)).color(t.secondary).background_color(t.inset));
            rest = body.get(end + 1..).unwrap_or("");
        }
    }
}
