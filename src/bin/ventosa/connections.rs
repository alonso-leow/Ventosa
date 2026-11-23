//! Connections: AI providers, search providers, MCP servers, CLIs and services as
//! nodes. Clicking a node shows its state; Connect installs what it needs (its
//! own dependencies first) in a terminal, or points at the key to set.

use super::kit::{self, small_text};
use super::{pages, App, View};
use eframe::egui::{self, RichText, Sense, Ui};
use ventosa::recommend::{self, Rec};

/// A binary a connection needs, and the command that installs it.
fn deps(id: &str) -> &'static [(&'static str, &'static str)] {
    match id {
        "hyperframes" | "remotion" | "gws" => &[("npm", "sudo pacman -S --needed nodejs npm")],
        "blender-mcp" => &[("uvx", "sudo pacman -S --needed uv")],
        "whisperx" => &[("python3", "sudo pacman -S --needed python")],
        "metal-mcp" => &[("claude", "npm i -g --prefix ~/.local @anthropic-ai/claude-code")],
        _ => &[],
    }
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// The full command for a connection: missing dependencies first, then its own installer.
fn connect_cmd(r: &Rec) -> String {
    let mut parts: Vec<&str> = deps(r.id).iter().filter(|(b, _)| !on_path(b)).map(|(_, c)| *c).collect();
    if !r.install.is_empty() {
        parts.push(r.install);
    }
    parts.join(" && ")
}

struct Node {
    id: String,
    name: String,
    section: &'static str,
    what: String,
    ok: bool,
    rec: Option<Rec>,
    key_env: String,
    link: String,
    /// (plugin id, connection id) for an external app a plugin brought.
    plugin: Option<(String, String)>,
}

fn nodes(app: &App) -> Vec<Node> {
    let mut v = vec![];
    for i in ventosa::models::CATALOG {
        let ok = std::env::var(i.key_env).is_ok_and(|x| !x.is_empty());
        v.push(Node { id: format!("ai-{}", i.id), name: i.name.into(), section: "AI providers", what: format!("{} models for nodes, chat and cron jobs ({} models listed)", i.name, i.models.len()), ok, rec: None, key_env: i.key_env.into(), link: String::new(), plugin: None });
    }
    for (id, name, env) in ventosa::research::PROVIDERS {
        let ok = env.is_empty() || std::env::var(env).is_ok_and(|x| !x.is_empty());
        v.push(Node { id: format!("search-{id}"), name: name.to_string(), section: "Internet research", what: "search provider for research nodes".into(), ok, rec: None, key_env: env.to_string(), link: String::new(), plugin: None });
    }
    for (r, ok) in recommend::report(&app.cfg) {
        let section = match r.kind {
            "mcp" => "MCP servers",
            "cli" => "Command-line tools",
            "app" => "Apps",
            _ => "Services",
        };
        v.push(Node { id: r.id.into(), name: r.name.into(), section, what: r.what.into(), ok, key_env: String::new(), link: r.link.into(), rec: Some(r), plugin: None });
    }
    // external apps a plugin declared: what a human-owned node works in
    for (pid, c, ok) in ventosa::plugin::connections() {
        v.push(Node { id: format!("plugin-{pid}-{}", c.id), name: c.name.clone(), section: "External apps (plugins)", what: c.what.clone(), ok, rec: None, key_env: c.key_env.clone(), link: c.link.clone(), plugin: Some((pid, c.id)) });
    }
    v
}

/// Every connection a role can be granted: (id, name, section).
pub fn catalog(app: &App) -> Vec<(String, String, &'static str)> {
    nodes(app).into_iter().map(|n| (n.id, n.name, n.section)).collect()
}

pub fn view(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let all: Vec<Node> = nodes(app).into_iter().filter(|n| app.access.can_connection(&n.id)).collect();
    let can_install = app.access.can_install_connection();
    let sel_key = egui::Id::new("conn-sel");
    let q_key = egui::Id::new("conn-q");
    let mut sel: String = ui.data(|d| d.get_temp(sel_key)).unwrap_or_default();
    let mut q: String = ui.data(|d| d.get_temp(q_key)).unwrap_or_default();
    pages::panel(ui, &t, |ui| {
        kit::panel_head(ui, &t, |ui| {
            ui.label(RichText::new("Connections").font(t.font_heading.clone()).color(t.primary));
            ui.add_space(8.0);
            let on = all.iter().filter(|n| n.ok).count();
            ui.label(small_text(&t, format!("{on} of {} connected", all.len())).color(t.ink_faint));
        });
        kit::subheader(ui, &t, |ui| {
            kit::search_box(ui, &t, &mut q, "search connections\u{2026}");
        });
        pages::body(ui, &t, "conn-body", |ui| {
            let ql = q.to_lowercase();
            let mut sections: Vec<&str> = vec![];
            for n in &all {
                if !sections.contains(&n.section) {
                    sections.push(n.section);
                }
            }
            for sec in sections {
                let items: Vec<&Node> = all.iter().filter(|n| n.section == sec && (ql.is_empty() || n.name.to_lowercase().contains(&ql))).collect();
                if items.is_empty() {
                    continue;
                }
                ui.add_space(6.0);
                ui.label(small_text(&t, sec).color(t.ink_faint).strong());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    for n in items {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(168.0, 62.0), Sense::click());
                        let p = ui.painter_at(rect);
                        let on = sel == n.id;
                        p.rect_filled(rect, 0.0, if on { t.node_header } else if resp.hovered() { t.hover_card() } else { t.node_body });
                        let tint = if n.ok { t.ok } else if n.rec.as_ref().is_some_and(|r| r.critical) { t.warn } else { t.ink_faint };
                        p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, tint);
                        p.rect_stroke(rect, 0.0, egui::Stroke::new(if on { 1.5 } else { 1.0 }, if on { t.primary } else { t.rule_bright }), egui::StrokeKind::Inside);
                        p.text(rect.min + egui::vec2(12.0, 9.0), egui::Align2::LEFT_TOP, &n.name, t.font_body.clone(), t.ink);
                        p.text(rect.min + egui::vec2(12.0, 36.0), egui::Align2::LEFT_TOP, if n.ok { "connected" } else { "not connected" }, t.font_small.clone(), tint);
                        p.circle_filled(rect.right_top() + egui::vec2(-12.0, 14.0), 4.0, tint);
                        if resp.clicked() {
                            sel = if on { String::new() } else { n.id.clone() };
                        }
                    }
                });
            }
            if let Some(n) = all.iter().find(|n| n.id == sel) {
                ui.add_space(10.0);
                kit::card_tinted(ui, &t, Some(&n.name), if n.ok { t.ok } else { t.warn }, |ui| {
                    kit::field_row(ui, &t, "state", if n.ok { "connected" } else { "not connected" });
                    ui.add(egui::Label::new(small_text(&t, &n.what).color(t.ink_weak)).wrap());
                    if !n.key_env.is_empty() {
                        let set = std::env::var(&n.key_env).is_ok_and(|x| !x.is_empty());
                        kit::field_row(ui, &t, "key env", &format!("{} \u{00b7} {}", n.key_env, if set { "found" } else { "not set \u{2014} export it before starting ventosa" }));
                    }
                    if let Some(r) = &n.rec {
                        let cmd = connect_cmd(r);
                        let missing: Vec<&str> = deps(r.id).iter().filter(|(b, _)| !on_path(b)).map(|(b, _)| *b).collect();
                        if !missing.is_empty() {
                            kit::field_row(ui, &t, "needs first", &missing.join(", "));
                        }
                        if !n.ok {
                            if cmd.is_empty() {
                                kit::notice(ui, &t, "No automatic install: follow the link");
                            } else if !can_install {
                                kit::notice(ui, &t, "An admin installs this");
                            } else if kit::button(ui, &t, "Connect (install)", Some(t.primary)).on_hover_text(&cmd).clicked() {
                                app.set_flash(match recommend::run_in_terminal(&cmd) {
                                    Ok(()) => format!("installing {} in a terminal", r.name),
                                    Err(e) => format!("terminal: {e}"),
                                });
                            }
                        }
                    } else if let Some((pid, cid)) = &n.plugin {
                        if n.key_env.is_empty() {
                            let label = if n.ok { "Mark as not set up" } else { "Mark as set up" };
                            if kit::button(ui, &t, label, Some(t.primary)).on_hover_text("a human-owned app: Ventosa cannot check it, so you say when it is ready").clicked() {
                                if let Err(e) = ventosa::plugin::set_connected(pid, cid, !n.ok) {
                                    app.set_flash(e);
                                }
                            }
                        }
                    } else if !n.ok && kit::button(ui, &t, "Set up in settings", Some(t.primary)).clicked() {
                        app.view = View::Settings;
                        app.harness.settings_sel = "ai".into();
                    }
                    if !n.link.is_empty() {
                        ui.hyperlink_to(small_text(&t, &n.link).color(t.primary_lit), &n.link);
                    }
                });
            }
        });
    });
    ui.data_mut(|d| {
        d.insert_temp(sel_key, sel);
        d.insert_temp(q_key, q);
    });
}
