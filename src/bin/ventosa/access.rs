//! The access view: identities and grants for hybrid human + AI pipelines.
//! Left, an index: users, roles, trusted sources, bots, audit. Right, the
//! editor of what is picked. The tool picker is an overlay after MetalVJ's
//! "+ add" menu: categories on the left, the tools with their descriptions on
//! the right, a search over both.

use super::kit::{self, small_text};
use super::{pages, App};
use eframe::egui::{self, RichText, Ui};
use ventosa::identity::{self, Role, User};
use ventosa::tools::{self, Cap};

#[derive(Default, Clone, PartialEq)]
pub enum Sel {
    #[default]
    Users,
    User(String),
    Role(String),
    Trusted,
    Bots,
    Audit,
    Mcp,
}

/// Where the tool picker writes its choice.
#[derive(Clone, PartialEq)]
enum PickFor {
    Role(String),
    User(String),
    Bot(usize),
}

#[derive(Default)]
pub struct AccessState {
    sel: Sel,
    new_name: String,
    new_pass: String,
    new_role: String,
    new_role_id: String,
    pass_reset: String,
    msg: String,
    picker: Option<(PickFor, Vec<String>)>,
    pick_q: String,
    pick_cat: Option<usize>,
    trusted_new: String,
    trusted_kind: String,
}

const CATS: [(Cap, &str); 6] = [(Cap::Read, "read"), (Cap::Write, "write"), (Cap::Graph, "graph"), (Cap::Install, "install"), (Cap::Apps, "apps"), (Cap::Run, "run")];

fn tools_label(v: &[String]) -> String {
    if v.iter().any(|t| t == "*") {
        "all tools".into()
    } else if v.is_empty() {
        "no tools".into()
    } else {
        format!("{} tools \u{00b7} {}", v.len(), v.iter().take(3).cloned().collect::<Vec<_>>().join(", "))
    }
}

/// A section heading of the index, padded off the rows and the panel edge.
fn section_label(ui: &mut Ui, t: &super::theme::Theme, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(small_text(t, text).color(t.ink_faint).strong());
    });
    ui.add_space(6.0);
}

pub fn view(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let admin = app.access.is_admin();
    pages::two_columns(ui, app, "access", 0.26, |ui, app| {
        pages::panel(ui, &t, |ui| {
            kit::panel_head(ui, &t, |ui| {
                ui.label(RichText::new("Access").font(t.font_heading.clone()).color(t.primary));
                ui.add_space(6.0);
                ui.label(small_text(&t, if app.identity.open_mode() { "open mode" } else { "login on" }).color(t.ink_faint));
            });
            pages::body(ui, &t, "access-index", |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let sel = app.access_ui.sel.clone();
                let mut pick = None;
                section_label(ui, &t, "Users");
                if kit::library_row(ui, &t, kit::RowIcon::People, "+ add a user", None, sel == Sel::Users).clicked() {
                    pick = Some(Sel::Users);
                }
                let now = ventosa::now();
                for u in &app.identity.users {
                    let state = if !u.enabled { "disabled".to_string() } else if u.expires != 0 && u.expires < now { "expired".into() } else { u.role.clone() };
                    if kit::library_row(ui, &t, kit::RowIcon::Badge, &u.name, Some(&state), sel == Sel::User(u.name.clone())).clicked() {
                        pick = Some(Sel::User(u.name.clone()));
                    }
                }
                ui.add_space(8.0);
                section_label(ui, &t, "Roles");
                for r in &app.identity.roles {
                    if kit::library_row(ui, &t, kit::RowIcon::Doc, &r.id, Some(&r.title), sel == Sel::Role(r.id.clone())).clicked() {
                        pick = Some(Sel::Role(r.id.clone()));
                    }
                }
                ui.add_space(8.0);
                section_label(ui, &t, "Pipelines");
                for (s, label, sub, icon) in [(Sel::Trusted, "trusted sources", "consistency across scenes", kit::RowIcon::Shield), (Sel::Bots, "bots", "telegram, on this app", kit::RowIcon::Bot), (Sel::Audit, "audit", "logins, refusals, changes", kit::RowIcon::Log)] {
                    if kit::library_row(ui, &t, icon, label, Some(sub), sel == s).clicked() {
                        pick = Some(s);
                    }
                }
                section_label(ui, &t, "Connect");
                if kit::library_row(ui, &t, kit::RowIcon::Plug, "connect agents", Some("MCP (stdio) for Claude Code, OpenCode, Hermes\u{2026}"), sel == Sel::Mcp).clicked() {
                    pick = Some(Sel::Mcp);
                }
                if let Some(s) = pick {
                    app.access_ui.sel = s;
                    app.access_ui.msg.clear();
                    app.narrow_right.insert("access".into(), true);
                }
            });
        });
    }, |ui, app| {
        pages::panel(ui, &t, |ui| {
            let sel = app.access_ui.sel.clone();
            kit::panel_head(ui, &t, |ui| {
                let title = match &sel {
                    Sel::Users => "add a user".to_string(),
                    Sel::User(n) => format!("user \u{00b7} {n}"),
                    Sel::Role(r) => format!("role \u{00b7} {r}"),
                    Sel::Trusted => "trusted sources".into(),
                    Sel::Bots => "bots".into(),
                    Sel::Audit => "audit".into(),
                    Sel::Mcp => "connect agents (MCP, stdio)".into(),
                };
                ui.label(RichText::new(title).font(t.font_heading.clone()).color(t.ink));
                if !app.access_ui.msg.is_empty() {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(small_text(&t, &app.access_ui.msg).color(t.ink_weak)).truncate());
                }
            });
            pages::body(ui, &t, "access-edit", |ui| {
                if !admin && !matches!(sel, Sel::Trusted | Sel::Audit | Sel::Mcp) {
                    kit::notice(ui, &t, "Only admins manage users, roles and bots");
                    return;
                }
                match sel {
                    Sel::Users => add_user(app, ui),
                    Sel::User(n) => user_editor(app, ui, &n),
                    Sel::Role(r) => role_editor(app, ui, &r),
                    Sel::Trusted => trusted(app, ui),
                    Sel::Bots => bots(app, ui),
                    Sel::Audit => audit(app, ui),
                    Sel::Mcp => pages::mcp_card(app, ui),
                }
            });
        });
    });
    tool_picker(app, ui.ctx());
}

fn role_options(app: &App) -> Vec<(String, String)> {
    app.identity.roles.iter().map(|r| (r.id.clone(), r.title.clone())).collect()
}

fn add_user(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    if app.identity.open_mode() {
        kit::card_tinted(ui, &t, Some("open mode"), t.secondary_dim, |ui| {
            kit::notice(ui, &t, "No users yet: the app is open. The first user is an admin; creating it turns login on for the app, the launcher and every web app.");
        });
        app.access_ui.new_role = "admin".into();
    } else if app.access_ui.new_role.is_empty() {
        app.access_ui.new_role = "labeller".into();
    }
    kit::card(ui, &t, Some("new user"), |ui| {
        kit::text_row(ui, &t, "name", &mut app.access_ui.new_name, "no spaces");
        kit::label_row(ui, &t, "password", |ui| ui.add(egui::TextEdit::singleline(&mut app.access_ui.new_pass).password(true).hint_text("8+ characters, temporary").font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY)));
        if !app.identity.open_mode() {
            let opts = role_options(app);
            kit::label_row(ui, &t, "role", |ui| kit::dropdown(ui, &t, "new-role", &mut app.access_ui.new_role, &opts));
        }
        if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Create").color(t.primary_lit))).clicked() {
            let first = app.identity.open_mode();
            let (n, p, r) = (app.access_ui.new_name.clone(), app.access_ui.new_pass.clone(), app.access_ui.new_role.clone());
            match app.identity.add_user(app.access.name(), &n, &p, &r) {
                Ok(()) => {
                    if let Some(u) = app.identity.users.iter_mut().find(|u| u.name == n.trim()) {
                        u.reset = !first;
                    }
                    let _ = identity::save(&app.identity);
                    app.access_ui.msg = if first { format!("{n} is the admin; login is on") } else { format!("{n} created; they choose a password at the first login") };
                    if first {
                        if let Some(u) = app.identity.user(n.trim()).cloned() {
                            app.access = identity::Access::for_user(&app.identity, u);
                        }
                    }
                    app.access_ui.sel = Sel::User(n.trim().to_string());
                    app.access_ui.new_name.clear();
                    app.access_ui.new_pass.clear();
                }
                Err(e) => app.access_ui.msg = e,
            }
        }
    });
}

fn user_editor(app: &mut App, ui: &mut Ui, name: &str) {
    let t = app.theme.clone();
    let Some(mut u) = app.identity.user(name).cloned() else { return };
    let mut before = u.clone();
    let opts = role_options(app);
    let mut pick: Option<PickFor> = None;
    let mut delete = false;
    kit::card(ui, &t, Some("identity"), |ui| {
        kit::text_row(ui, &t, "display", &mut u.display, "full name");
        kit::text_row(ui, &t, "email", &mut u.email, "name@example.com");
        kit::label_row(ui, &t, "role", |ui| kit::dropdown(ui, &t, ("u-role", name), &mut u.role, &opts));
        let mut ws = u.workspaces.join(", ");
        if kit::text_row(ui, &t, "workspaces", &mut ws, "empty = all").lost_focus() {
            u.workspaces = ws.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
    });
    kit::card(ui, &t, Some("lifecycle"), |ui| {
        kit::toggle_row(ui, &t, "enabled", &mut u.enabled);
        let mut days = if u.expires == 0 { 0 } else { (u.expires.saturating_sub(ventosa::now()) / 86_400) as i64 };
        kit::label_row(ui, &t, "expires in", |ui| {
            if ui.add(egui::DragValue::new(&mut days).range(0..=3650).suffix(" days")).changed() {
                u.expires = if days == 0 { 0 } else { ventosa::now() + days as u64 * 86_400 };
            }
            ui.label(small_text(&t, if u.expires == 0 { "  (0 = never)" } else { "" }).color(t.ink_faint));
        });
        kit::toggle_row(ui, &t, "new password at login", &mut u.reset);
        kit::field_row(ui, &t, "last login", &if u.last_login == 0 { "never".to_string() } else { format!("{} ago", ago(ventosa::now() - u.last_login)) });
        kit::label_row(ui, &t, "password", |ui| ui.add(egui::TextEdit::singleline(&mut app.access_ui.pass_reset).password(true).hint_text("set a new one").font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY)));
        let b = kit::label_row(ui, &t, "", |ui| kit::payload_buttons(ui, &t, vec![small_text(&t, "Set password").color(t.primary_lit), small_text(&t, "Delete user").color(t.warn)]));
        if b[0] {
            let p = std::mem::take(&mut app.access_ui.pass_reset);
            app.access_ui.msg = match app.identity.set_password(app.access.name(), name, &p) {
                Ok(()) => "password set".into(),
                Err(e) => e,
            };
            if let Some(x) = app.identity.user(name) {
                u.pass = x.pass.clone();
            }
            u.reset = false;
        }
        delete = b[1];
    });
    kit::card(ui, &t, Some("chat tools"), |ui| {
        let role_tools = app.identity.role(&u.role).map(|r| r.tools.clone()).unwrap_or_default();
        kit::field_row(ui, &t, "role grants", &tools_label(&role_tools));
        kit::label_row(ui, &t, "this user", |ui| {
            let label = if u.tools.is_empty() { "the role's set".to_string() } else { tools_label(&u.tools) };
            if kit::payload_button(ui, &t, small_text(&t, format!("{label} \u{25be}")).color(t.primary_lit)).clicked() {
                pick = Some(PickFor::User(name.into()));
            }
        });
        kit::notice(ui, &t, "A user's tools narrow the role's, never widen them");
    });
    if delete {
        if app.identity.users.iter().filter(|x| app.identity.role(&x.role).is_some_and(|r| r.admin)).count() <= 1 && app.identity.role(&u.role).is_some_and(|r| r.admin) {
            app.access_ui.msg = "the last admin can't be deleted".into();
        } else {
            app.identity.users.retain(|x| x.name != name);
            identity::audit(app.access.name(), "user.delete", name, true);
            let _ = identity::save(&app.identity);
            app.access_ui.sel = Sel::Users;
            return;
        }
    }
    if u.pass != before.pass {
        // set_password saved and audited already
        before.pass = u.pass.clone();
        before.reset = u.reset;
    }
    if u != before {
        let what = if u.enabled != before.enabled { if u.enabled { "user.enable" } else { "user.disable" } } else if u.role != before.role { "user.role" } else { "user.update" };
        if let Some(x) = app.identity.users.iter_mut().find(|x| x.name == name) {
            *x = u;
        }
        identity::audit(app.access.name(), what, name, true);
        let _ = identity::save(&app.identity);
    }
    if let Some(p) = pick {
        let cur = app.identity.user(name).map(|x| x.tools.clone()).unwrap_or_default();
        app.access_ui.picker = Some((p, cur));
    }
}

fn ago(s: u64) -> String {
    if s < 3600 { format!("{}m", s / 60) } else if s < 86_400 { format!("{}h", s / 3600) } else { format!("{}d", s / 86_400) }
}

fn role_editor(app: &mut App, ui: &mut Ui, id: &str) {
    let t = app.theme.clone();
    let Some(mut r) = app.identity.role(id).cloned() else { return };
    let before = r.clone();
    let mut pick = false;
    kit::card(ui, &t, Some("role"), |ui| {
        kit::text_row(ui, &t, "title", &mut r.title, "what this role does");
        kit::toggle_row(ui, &t, "graph control", &mut r.graph);
        kit::toggle_row(ui, &t, "admin", &mut r.admin);
        kit::toggle_row(ui, &t, "edit docs", &mut r.docs_edit);
        kit::toggle_row(ui, &t, "chat", &mut r.chat);
    });
    kit::card(ui, &t, Some("apps"), |ui| {
        let mut all = r.apps.iter().any(|a| a == "*");
        if kit::toggle_row(ui, &t, "every app", &mut all).changed() {
            r.apps = if all { vec!["*".into()] } else { vec![] };
        }
        if !all {
            let mut ids: Vec<(String, String)> = pages::WEB_APPS.iter().map(|(i, l, _)| (i.to_string(), l.to_string())).collect();
            ids.push(("docs".into(), "docs".into()));
            for ws in &app.cfg.workspaces {
                for a in ventosa::apps::list(ws) {
                    ids.push((a.id.clone(), format!("{} (AI app)", a.title)));
                }
            }
            for (i, l) in ids {
                let mut on = r.apps.contains(&i);
                if kit::toggle_row(ui, &t, &l, &mut on).changed() {
                    if on {
                        r.apps.push(i);
                    } else {
                        r.apps.retain(|a| a != &i);
                    }
                }
            }
        }
        if r.graph {
            kit::notice(ui, &t, "Graph control opens every app");
        }
    });
    kit::card(ui, &t, Some("connections"), |ui| {
        if r.graph {
            kit::notice(ui, &t, "Graph control uses every connection (installing one stays admin-only)");
        } else {
            let mut all = r.connections.iter().any(|c| c == "*");
            if kit::toggle_row(ui, &t, "every connection", &mut all).changed() {
                r.connections = if all { vec!["*".into()] } else { vec![] };
            }
            if !all {
                let mut section = "";
                for (cid, name, sec) in super::connections::catalog(app) {
                    if sec != section {
                        section = sec;
                        ui.label(small_text(&t, sec).color(t.ink_faint).strong());
                    }
                    let group = cid.split_once('-').filter(|(g, _)| *g == "ai" || *g == "search").map(|(g, _)| format!("{g}-*"));
                    let mut on = r.connections.contains(&cid) || group.as_ref().is_some_and(|g| r.connections.contains(g));
                    if kit::toggle_row(ui, &t, &name, &mut on).changed() {
                        // turning one off out of a whole group leaves the others of that group on
                        if let Some(g) = group.as_ref().filter(|g| r.connections.contains(*g)) {
                            r.connections.retain(|c| c != g);
                            for (oid, _, _) in super::connections::catalog(app).into_iter().filter(|(o, ..)| o.starts_with(&g[..g.len() - 1]) && *o != cid) {
                                r.connections.push(oid);
                            }
                        } else if on {
                            r.connections.push(cid);
                        } else {
                            r.connections.retain(|c| c != &cid);
                        }
                    }
                }
            }
            kit::notice(ui, &t, "The role's chat runs only on a listed AI provider and sees only the listed connections; installing is admin-only");
        }
    });
    kit::card(ui, &t, Some("chat tools"), |ui| {
        kit::label_row(ui, &t, "tools", |ui| {
            if kit::payload_button(ui, &t, small_text(&t, format!("{} \u{25be}", tools_label(&r.tools))).color(t.primary_lit)).clicked() {
                pick = true;
            }
        });
        kit::notice(ui, &t, "The launcher's chat may call only these; settings \u{25b8} permissions still apply (ask = refused when nobody can approve)");
    });
    kit::card(ui, &t, Some("new role"), |ui| {
        kit::text_row(ui, &t, "id", &mut app.access_ui.new_role_id, "e.g. editor");
        let b = kit::label_row(ui, &t, "", |ui| kit::payload_buttons(ui, &t, vec![small_text(&t, "Copy this role").color(t.primary_lit), small_text(&t, "Delete this role").color(t.warn)]));
        if b[0] {
            let nid = app.access_ui.new_role_id.trim().to_string();
            if nid.is_empty() || app.identity.role(&nid).is_some() {
                app.access_ui.msg = "pick a new, unused id".into();
            } else {
                app.identity.roles.push(Role { id: nid.clone(), title: format!("{nid} (from {id})"), ..r.clone() });
                identity::audit(app.access.name(), "role.create", &nid, true);
                let _ = identity::save(&app.identity);
                app.access_ui.sel = Sel::Role(nid);
                app.access_ui.new_role_id.clear();
                return;
            }
        }
        if b[1] {
            if app.identity.users.iter().any(|u| u.role == id) {
                app.access_ui.msg = "users still have this role".into();
            } else {
                app.identity.roles.retain(|x| x.id != id);
                identity::audit(app.access.name(), "role.delete", id, true);
                let _ = identity::save(&app.identity);
                app.access_ui.sel = Sel::Users;
                return;
            }
        }
    });
    if r != before {
        if let Some(x) = app.identity.roles.iter_mut().find(|x| x.id == id) {
            *x = r.clone();
        }
        identity::audit(app.access.name(), "role.update", id, true);
        let _ = identity::save(&app.identity);
    }
    if pick {
        app.access_ui.picker = Some((PickFor::Role(id.into()), r.tools));
    }
}

fn trusted(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let Some(ws) = app.cfg.workspace(&app.ws_sel).cloned().or_else(|| app.cfg.workspaces.first().cloned()) else {
        kit::notice(ui, &t, "No workspace");
        return;
    };
    kit::notice(ui, &t, &format!("{} \u{00b7} {}: every scene builds over the approved sources (same refs, recipe, locked traits); the AI proposes, a person approves", ws.name, ventosa::trusted::path(&ws).display()));
    let mut list = ventosa::trusted::load(&ws);
    let mut changed = false;
    let mut remove = None;
    for (k, s) in list.iter_mut().enumerate() {
        let tint = if s.approved { t.primary } else { t.secondary };
        kit::card_tinted(ui, &t, Some(&format!("{} \u{00b7} {} \u{00b7} {}", s.kind, s.name, if s.approved { "approved" } else { "proposed" })), tint, |ui| {
            ui.push_id(("trusted", k), |ui| {
                changed |= kit::text_row(ui, &t, "name", &mut s.name, "").changed();
                let mut refs = s.refs.join(", ");
                if kit::text_row(ui, &t, "refs", &mut refs, "paths or urls, comma separated").changed() {
                    s.refs = refs.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                    changed = true;
                }
                let mut locked = s.locked.join("; ");
                if kit::text_row(ui, &t, "locked", &mut locked, "traits that never change; separated by ;").changed() {
                    s.locked = locked.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                    changed = true;
                }
                changed |= kit::text_row(ui, &t, "recipe", &mut s.recipe, "seed, prompt fragment, model").changed();
                changed |= kit::text_row(ui, &t, "notes", &mut s.notes, "").changed();
                kit::field_row(ui, &t, "used in", &if s.used_in.is_empty() { "\u{2014}".to_string() } else { s.used_in.join(", ") });
                let b = kit::label_row(ui, &t, "", |ui| kit::payload_buttons(ui, &t, vec![small_text(&t, if s.approved { "back to proposed" } else { "approve" }).color(t.primary_lit), small_text(&t, "Remove").color(t.ink_faint)]));
                if b[0] {
                    s.approved = !s.approved;
                    changed = true;
                    identity::audit(app.access.name(), if s.approved { "trusted.approve" } else { "trusted.unapprove" }, &s.id, true);
                }
                if b[1] {
                    remove = Some(k);
                }
            });
        });
    }
    if let Some(k) = remove {
        list.remove(k);
        changed = true;
    }
    if changed {
        let _ = ventosa::trusted::save(&ws, &list);
    }
    kit::card(ui, &t, Some("add a source"), |ui| {
        if app.access_ui.trusted_kind.is_empty() {
            app.access_ui.trusted_kind = "character".into();
        }
        let opts: Vec<(String, String)> = ventosa::trusted::KINDS.iter().map(|k| (k.to_string(), k.to_string())).collect();
        kit::label_row(ui, &t, "kind", |ui| kit::dropdown(ui, &t, "trusted-kind", &mut app.access_ui.trusted_kind, &opts));
        kit::text_row(ui, &t, "name", &mut app.access_ui.trusted_new, "e.g. Lola (host)");
        if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Add (approved)").color(t.primary_lit))).clicked() {
            let s = ventosa::trusted::Source { kind: app.access_ui.trusted_kind.clone(), name: app.access_ui.trusted_new.trim().to_string(), added_by: app.access.name().into(), ..Default::default() };
            match ventosa::trusted::propose(&ws, s) {
                Ok(id) => {
                    let _ = ventosa::trusted::set_approved(&ws, &id, true);
                    app.access_ui.trusted_new.clear();
                }
                Err(e) => app.access_ui.msg = e,
            }
        }
    });
}

fn bots(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    kit::notice(ui, &t, "Each bot runs inside this app: receive (Telegram long polling) \u{2192} ai reply (its tools only, unattended) \u{2192} send. docs \u{25b8} Telegram bots");
    let status = ventosa::bots::status();
    let ws_opts: Vec<(String, String)> = app.cfg.workspaces.iter().map(|w| (w.name.clone(), w.name.clone())).collect();
    let mut changed = false;
    let mut remove = None;
    let mut pick = None;
    for (k, b) in app.cfg.bots.iter_mut().enumerate() {
        kit::card(ui, &t, Some(&b.name.clone()), |ui| {
            ui.push_id(("bot", k), |ui| {
                changed |= kit::text_row(ui, &t, "name", &mut b.name, "").changed();
                changed |= kit::text_row(ui, &t, "token env", &mut b.token_env, "TELEGRAM_BOT_TOKEN").changed();
                kit::label_row(ui, &t, "workspace", |ui| changed |= kit::dropdown(ui, &t, ("bot-ws", k), &mut b.workspace, &ws_opts));
                kit::label_row(ui, &t, "tools", |ui| {
                    if kit::payload_button(ui, &t, small_text(&t, format!("{} \u{25be}", tools_label(&b.tools))).color(t.primary_lit)).clicked() {
                        pick = Some((k, b.tools.clone()));
                    }
                });
                let mut chats = b.allowed_chats.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
                if kit::text_row(ui, &t, "allowed chats", &mut chats, "telegram chat ids, comma separated").changed() {
                    b.allowed_chats = chats.split(',').filter_map(|c| c.trim().parse().ok()).collect();
                    changed = true;
                }
                changed |= kit::text_row(ui, &t, "prompt", &mut b.prompt, "how it answers").changed();
                changed |= kit::toggle_row(ui, &t, "enabled", &mut b.enabled).changed();
                let st = &status[&b.name];
                let key_ok = std::env::var(&b.token_env).is_ok_and(|v| !v.is_empty());
                let line = if !key_ok {
                    format!("{} is not set", b.token_env)
                } else if let Some(e) = st["error"].as_str() {
                    format!("error: {e}")
                } else if let Some(p) = st["polled"].as_u64() {
                    format!("polled {} ago", ago(ventosa::now().saturating_sub(p)))
                } else {
                    "not started".into()
                };
                kit::field_row(ui, &t, "status", &line);
                let refused: Vec<String> = st["refused"].as_array().into_iter().flatten().filter_map(|v| v.as_i64()).filter(|c| !b.allowed_chats.contains(c)).map(|c| c.to_string()).collect();
                if !refused.is_empty() {
                    kit::field_row(ui, &t, "refused chats", &refused.join(", "));
                }
                if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Remove").color(t.ink_faint))).clicked() {
                    remove = Some(k);
                }
            });
        });
    }
    if let Some(k) = remove {
        app.cfg.bots.remove(k);
        changed = true;
    }
    if kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "+ telegram bot").color(t.primary_lit))).clicked() {
        let ws = app.cfg.workspaces.first().map(|w| w.name.clone()).unwrap_or_default();
        app.cfg.bots.push(ventosa::config::Bot { name: format!("bot-{}", app.cfg.bots.len() + 1), workspace: ws, ..Default::default() });
        changed = true;
    }
    if changed {
        app.save_cfg();
    }
    if let Some((k, cur)) = pick {
        app.access_ui.picker = Some((PickFor::Bot(k), cur));
    }
}

fn audit(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let rows = identity::audit_tail(200);
    if rows.is_empty() {
        kit::notice(ui, &t, "Nothing yet");
    }
    let now = ventosa::now();
    for r in rows {
        let ok = r["ok"] == true;
        let c = if ok { t.ink } else { t.warn };
        let when = format!("{} ago", ago(now.saturating_sub(r["at"].as_u64().unwrap_or(0))));
        kit::label_row_tinted(ui, &t, &when, if ok { t.primary } else { t.warn }, |ui| {
            let cells = [r["actor"].as_str().unwrap_or("").to_string(), r["action"].as_str().unwrap_or("").to_string(), r["target"].as_str().unwrap_or("").to_string(), format!("{} \u{00b7} {}", r["via"].as_str().unwrap_or(""), if ok { "ok" } else { "refused" })];
            kit::payload_split(ui, &t, 4, |ui, k| {
                kit::centered_label(ui, small_text(&t, &cells[k]).color(c));
            });
        });
    }
}

/// The tool picker overlay (after MetalVJ's "+ add" menu): a search, the
/// capability categories on the left, the tools (with what they do) on the right.
fn tool_picker(app: &mut App, ctx: &egui::Context) {
    let Some((target, mut chosen)) = app.access_ui.picker.clone() else { return };
    let t = app.theme.clone();
    let mut close = false;
    let mut apply = false;
    let modal = egui::Modal::new(egui::Id::new("tool-picker")).frame(egui::Frame::new().fill(t.panel).stroke(t.rule_stroke()).inner_margin(t.panel_margin()));
    let resp = modal.show(ctx, |ui| {
        let w = (ctx.content_rect().width() - 80.0).clamp(360.0, 760.0);
        let h = (ctx.content_rect().height() - 120.0).clamp(280.0, 560.0);
        ui.set_width(w);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tools").font(t.font_heading.clone()).color(t.primary));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut app.access_ui.pick_q).hint_text(small_text(&t, "Search tools\u{2026}").color(t.ink_faint)).font(t.font_small.clone()).desired_width(f32::INFINITY));
        });
        ui.add_space(6.0);
        let mut all = chosen.iter().any(|x| x == "*");
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(150.0, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if kit::library_row(ui, &t, kit::RowIcon::Folder, "all", Some(&format!("{} tools", tools::TOOLS.len())), app.access_ui.pick_cat.is_none()).clicked() {
                    app.access_ui.pick_cat = None;
                }
                for (k, (cap, label)) in CATS.iter().enumerate() {
                    let n = tools::TOOLS.iter().filter(|x| x.cap == *cap).count();
                    let on_n = tools::TOOLS.iter().filter(|x| x.cap == *cap && (all || chosen.iter().any(|c| c == x.name))).count();
                    if kit::library_row(ui, &t, kit::RowIcon::Folder, label, Some(&format!("{on_n}/{n} on")), app.access_ui.pick_cat == Some(k)).clicked() {
                        app.access_ui.pick_cat = Some(k);
                    }
                }
            });
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut all, small_text(&t, "Every tool (also future ones)")).changed() {
                        chosen = if all { vec!["*".into()] } else { vec![] };
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::link(ui, &t, "None", t.ink_weak).clicked() {
                            chosen.clear();
                            all = false;
                        }
                        if kit::link(ui, &t, "Read only", t.ink_weak).clicked() {
                            chosen = tools::TOOLS.iter().filter(|x| x.cap == Cap::Read).map(|x| x.name.to_string()).collect();
                            all = false;
                        }
                    });
                });
                let q = app.access_ui.pick_q.trim().to_lowercase();
                egui::ScrollArea::vertical().id_salt("tool-picker-list").auto_shrink([false, false]).show(ui, |ui| {
                    for tool in tools::TOOLS.iter() {
                        if let Some(k) = app.access_ui.pick_cat {
                            if tool.cap != CATS[k].0 {
                                continue;
                            }
                        }
                        if !q.is_empty() && !tool.name.contains(&q) && !tool.description.to_lowercase().contains(&q) {
                            continue;
                        }
                        let mut on = all || chosen.iter().any(|c| c == tool.name);
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!all, |ui| {
                                if ui.checkbox(&mut on, RichText::new(tool.name).font(t.font_small.clone()).color(t.ink).strong()).changed() {
                                    if on {
                                        chosen.push(tool.name.to_string());
                                    } else {
                                        chosen.retain(|c| c != tool.name);
                                    }
                                }
                            });
                            let cat = CATS.iter().find(|c| c.0 == tool.cap).map_or("", |c| c.1);
                            ui.label(small_text(&t, cat).color(if tool.cap == Cap::Read { t.ink_faint } else { t.secondary }));
                        });
                        ui.add(egui::Label::new(small_text(&t, tool.description).color(t.ink_weak)).wrap());
                        ui.add_space(4.0);
                    }
                });
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(small_text(&t, tools_label(&chosen)).color(t.ink_weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::button(ui, &t, "Apply", Some(t.primary_lit)).clicked() {
                    apply = true;
                }
                if kit::button(ui, &t, "Cancel", None).clicked() {
                    close = true;
                }
            });
        });
    });
    if resp.should_close() {
        close = true;
    }
    if apply {
        match &target {
            PickFor::Role(id) => {
                if let Some(r) = app.identity.roles.iter_mut().find(|r| &r.id == id) {
                    r.tools = chosen.clone();
                }
                identity::audit(app.access.name(), "role.tools", id, true);
                let _ = identity::save(&app.identity);
            }
            PickFor::User(n) => {
                if let Some(u) = app.identity.users.iter_mut().find(|u| &u.name == n) {
                    u.tools = chosen.iter().filter(|c| *c != "*").cloned().collect::<Vec<_>>();
                }
                identity::audit(app.access.name(), "user.tools", n, true);
                let _ = identity::save(&app.identity);
            }
            PickFor::Bot(k) => {
                if let Some(b) = app.cfg.bots.get_mut(*k) {
                    b.tools = chosen.clone();
                }
                app.save_cfg();
            }
        }
        close = true;
    }
    app.access_ui.picker = if close { None } else { Some((target, chosen)) };
    let _ = User::default;
}
