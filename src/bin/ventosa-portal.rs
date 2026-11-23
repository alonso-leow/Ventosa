//! `ventosa-portal`: the launcher-only desktop app. It logs in against the same
//! identity file as `ventosa` and shows only what the user's role grants: the
//! apps, the docs and (when granted) a chat with a chosen set of tools. Never
//! the graph.

#[path = "ventosa/main.rs"]
#[allow(dead_code)]
mod app;

fn main() -> eframe::Result {
    app::run(true)
}
