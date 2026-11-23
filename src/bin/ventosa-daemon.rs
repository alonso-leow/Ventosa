//! Headless sync + scheduler + executions (batches). The app does the same work while it is open;
//! run this from systemd/cron when the app is closed.
//!   ventosa-daemon          loop every 20 s
//!   ventosa-daemon --once   one sync + one scheduler tick, then exit (for cron)

use std::time::Duration;
use ventosa::{config, graph, queue_path, runner};

fn pass(block: bool) {
    let cfg = config::load(&config::default_path());
    for ws in cfg.workspaces.iter().filter(|w| w.enabled) {
        match graph::sync(ws, &queue_path()) {
            Ok(n) if n > 0 => println!("sync {}: {n} change(s)", ws.name),
            Ok(_) => {}
            Err(e) => eprintln!("sync {}: {e}", ws.name),
        }
    }
    for line in runner::tick(&cfg, block) {
        println!("schedule {line}");
    }
    for line in ventosa::hooks::poll(&cfg, block) {
        println!("{line}");
    }
    for line in ventosa::linkmedia::tick(&cfg) {
        println!("{line}");
    }
}

fn main() {
    if std::env::args().any(|a| a == "--once") {
        pass(true);
        return;
    }
    // executions: start batch runs every 3 s; sync + schedules every 20 s
    let mut k = 0u32;
    loop {
        if k % 7 == 0 {
            pass(false);
        }
        let cfg = config::load(&config::default_path());
        for line in ventosa::batch::tick(&cfg) {
            println!("{line}");
        }
        for line in ventosa::hooks::poll(&cfg, false) {
            println!("{line}");
        }
        for line in ventosa::linkmedia::tick(&cfg) {
            println!("{line}");
        }
        k = k.wrapping_add(1);
        std::thread::sleep(Duration::from_secs(3));
    }
}
