//! Login and the launcher. With users in `identity.json` the app asks for a
//! login first. People whose role has no graph control (and everyone in
//! `ventosa-portal`) land in the launcher: the apps their role grants, the
//! docs, and — when granted — a chat whose AI may call only their tools.

use super::kit::{self, small_text};
use super::{chat_ui, pages, App, View};
use eframe::egui::{self, RichText, Ui};
use ventosa::identity::{self, Access};

#[derive(Default)]
pub struct LoginState {
    pub name: String,
    pub password: String,
    pub new_password: String,
    pub error: String,
    /// Logged in but the account must change its password first.
    pub must_change: bool,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Pane {
    #[default]
    Apps,
    Chat,
    Docs,
}

#[derive(Default)]
pub struct LauncherState {
    pub pane: Pane,
}

pub fn logout(app: &mut App) {
    if let Some(u) = app.access.user.clone() {
        let level = if app.launcher_only || !app.access.can_graph() { "worker" } else { "admin" };
        for ws in app.cfg.workspaces.iter() {
            ventosa::hooks::leave(ws, &u.name, level);
        }
        app.beat_at = 0;
    }
    identity::audit(app.access.name(), "logout", "app", true);
    app.access = Access::open();
    app.identity = identity::load();
    app.login = LoginState::default();
    app.chat.grant = None;
    app.view = View::Graph;
}

fn centred<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let w = ui.available_width();
    let col = width.min(w);
    ui.horizontal_top(|ui| {
        ui.add_space(((w - col) * 0.5).max(0.0));
        ui.allocate_ui_with_layout(egui::vec2(col, ui.available_height()), egui::Layout::top_down(egui::Align::Min), add).inner
    })
    .inner
}

/// The login screen.
pub fn login(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    ui.add_space((ui.available_height() * 0.18).max(12.0));
    centred(ui, 380.0, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - 90.0) * 0.5).max(0.0));
            kit::wordmark(ui, &t, &app.cfg.brand.name.clone());
        });
        ui.add_space(14.0);
        kit::card(ui, &t, Some(if app.login.must_change { "choose a new password" } else { "log in" }), |ui| {
            let mut submit = false;
            if app.login.must_change {
                let r = kit::label_row(ui, &t, "new password", |ui| ui.add(egui::TextEdit::singleline(&mut app.login.new_password).password(true).font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY)));
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            } else {
                let r = kit::text_row(ui, &t, "user", &mut app.login.name, "name");
                if app.login.name.is_empty() && !r.has_focus() && app.login.error.is_empty() {
                    r.request_focus();
                }
                let r = kit::label_row(ui, &t, "password", |ui| ui.add(egui::TextEdit::singleline(&mut app.login.password).password(true).font(t.font_small.clone()).horizontal_align(egui::Align::Center).desired_width(f32::INFINITY)));
                submit |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            }
            submit |= kit::label_row(ui, &t, "", |ui| kit::payload_button(ui, &t, small_text(&t, "Continue").color(t.primary_lit))).clicked();
            if !app.login.error.is_empty() {
                kit::label_row(ui, &t, "", |ui| kit::centered_label(ui, small_text(&t, &app.login.error).color(t.warn)));
            }
            if submit {
                try_login(app);
            }
        });
        kit::notice(ui, &t, "Accounts are managed by an admin in access");
    });
}

fn try_login(app: &mut App) {
    if app.login.must_change {
        let name = app.login.name.trim().to_string();
        match app.identity.set_password(&name, &name, &app.login.new_password) {
            Ok(()) => {
                app.identity = identity::load();
                app.login.must_change = false;
                app.login.new_password.clear();
                if let Some(u) = app.identity.user(&name).cloned() {
                    enter(app, u);
                }
            }
            Err(e) => app.login.error = e,
        }
        return;
    }
    app.identity = identity::load();
    let (name, pw) = (app.login.name.clone(), std::mem::take(&mut app.login.password));
    match app.identity.login(&name, &pw) {
        Ok(u) if std::env::var("VENTOSA_LEVEL").as_deref() == Ok("admin") && !app.identity.role(&u.role).is_some_and(|r| r.graph) => {
            identity::audit(&u.name, "login", "admin level refused", false);
            app.login.error = "this is the admin level: use an account with graph control (workers log in at the worker launcher)".into();
        }
        Ok(u) if u.reset => {
            app.login.must_change = true;
            app.login.error.clear();
        }
        Ok(u) => enter(app, u),
        Err(e) => app.login.error = e,
    }
}

fn enter(app: &mut App, u: identity::User) {
    app.access = Access::for_user(&app.identity, u);
    app.login.error.clear();
    // the chat instance takes the user's grant unless they control the graph
    if !app.access.can_graph() || app.launcher_only {
        app.chat = chat_ui::ChatState::with_mode(&format!("launcher:{}", app.access.name()));
        app.chat.grant = Some(app.access.tools());
    }
    if let Some(ws) = app.cfg.workspaces.iter().find(|w| app.access.sees_workspace(&w.name)) {
        app.ws_sel = ws.name.clone();
    }
}

/// The launcher: a header (who, log out), the panes the role grants.
pub fn launcher(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let user = app.access.user.clone();
    kit::panel_head(ui, &t, |ui| {
        kit::wordmark(ui, &t, &app.cfg.brand.name.clone());
        ui.add_space(12.0);
        let panes = [(Pane::Apps, "apps", true), (Pane::Chat, "chat", app.access.can_chat()), (Pane::Docs, "docs", app.access.can_app("docs"))];
        for (p, label, on) in panes {
            if on && kit::tab(ui, &t, app.launcher.pane == p, label, t.primary) {
                app.launcher.pane = p;
            }
            ui.add_space(4.0);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(u) = &user {
                if kit::link(ui, &t, "Log out", t.ink_weak).clicked() {
                    logout(app);
                }
                ui.label(small_text(&t, format!("{} \u{00b7} {}", if u.display.is_empty() { &u.name } else { &u.display }, u.role)).color(t.ink_faint));
            } else {
                ui.label(small_text(&t, "Open mode (no users yet)").color(t.ink_faint));
            }
        });
    });
    ui.add_space(8.0);
    match app.launcher.pane {
        Pane::Apps => apps_pane(app, ui),
        Pane::Chat if app.access.can_chat() => {
            if app.chat.grant.is_none() {
                app.chat.grant = Some(app.access.tools());
            }
            chat_ui::full(app, ui)
        }
        Pane::Docs => super::docs::view(app, ui),
        _ => apps_pane(app, ui),
    }
}

fn apps_pane(app: &mut App, ui: &mut Ui) {
    let t = app.theme.clone();
    let workspaces: Vec<_> = app.cfg.workspaces.iter().filter(|w| app.access.sees_workspace(&w.name)).cloned().collect();
    centred(ui, 720.0, |ui| {
        egui::ScrollArea::vertical().id_salt("launcher-apps").auto_shrink([false, false]).show(ui, |ui| {
            if workspaces.is_empty() {
                kit::notice(ui, &t, "No workspace is shared with you yet");
            }
            for ws in workspaces {
                kit::card(ui, &t, Some(&ws.name), |ui| {
                    let mut any = false;
                    let in_graph = ventosa::graph::app_ids(&ws);
                    for (id, label, hint) in pages::WEB_APPS {
                        if !in_graph.iter().any(|a| a == id) || !app.access.can_app(id) {
                            continue;
                        }
                        any = true;
                        let open = kit::label_row_tinted(ui, &t, label, t.secondary, |ui| {
                            let r = kit::payload_button(ui, &t, small_text(&t, format!("open \u{2197}")).color(t.secondary));
                            r.on_hover_text(*hint)
                        });
                        if open.clicked() {
                            let m = pages::open_app(&ws, id, false);
                            app.set_flash(m);
                        }
                    }
                    app.custom_apps.refresh(&app.cfg, &ws.name);
                    for (def, _) in app.custom_apps.list.clone() {
                        if !app.access.can_app(&def.id) || app.access.role.as_ref().is_some_and(|r| !r.apps.iter().any(|a| a == &def.id || a == "*")) {
                            continue;
                        }
                        any = true;
                        if kit::label_row(ui, &t, &def.title, |ui| kit::payload_button(ui, &t, small_text(&t, "Open \u{2197}").color(t.primary_lit))).clicked() {
                            let (w, id) = (ws.clone(), def.id.clone());
                            std::thread::spawn(move || {
                                if let Ok(url) = ventosa::apps::deploy(&w, &id) {
                                    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                                }
                            });
                        }
                    }
                    if !any {
                        kit::notice(ui, &t, "Your role opens no apps here");
                    }
                });
            }
            let msg = app.flash.lock().map(|f| f.clone()).unwrap_or_default();
            if !msg.is_empty() {
                ui.label(RichText::new(msg).font(t.font_small.clone()).color(t.ink_faint));
            }
        });
    });
}
