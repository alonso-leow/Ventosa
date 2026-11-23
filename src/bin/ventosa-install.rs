//! Install the Ventosa MCP server and the ventosa skill into AI clients.
//!   ventosa-install [--client claude|opencode|codex|hermes|zeroclaw|openclaw|all]
//!                   [--scope user|project] [--project DIR] [--workspace NAME]
//!                   [--mcp-bin PATH] [--dry-run] [--print] [--uninstall]

use std::process::ExitCode;
use ventosa::install::{self, USAGE};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match install::parse_args(&args) {
        Ok(Some(o)) => o,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("ventosa-install: {e}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match install::run(&opts) {
        Ok(rep) => {
            for l in rep.lines {
                println!("{l}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ventosa-install: {e}");
            ExitCode::FAILURE
        }
    }
}
