# ventosa

A content manager for human + AI production. The AI sees the work as a **skill graph**: step nodes, each with its own skill. The human sees the same work as a **to-do list** and the **AI's queue**. Native egui app in MetalVJ's component look (purple primary, orange for to-do), plus white-label web apps for labelling, recording and uploads.

## What's in the box

| | |
|---|---|
| `ventosa` | The app, an AI harness for media production. Nav bar: **graph** · **ai** (full-screen chat with tools) · **speech** (the same AI by voice) · **settings** · **files**. **Overview**: the graph on a node canvas (AI steps on the line, your steps below, log points around each step) and phase 1 (title → script). **to-do**: your tasks plus the pipeline steps waiting on you. **queue ai**: what the AI generated or holds. **schedules**: cron for AI steps. **Executions** (overview): batches that run a graph N times or drain a step, within the limits in settings. **Decisions**: what the AI elevates shows in the to-do with options. **apps**, **settings**, first-run **onboarding**. Header buttons ↗ open the three web pages. |
| `ventosa-mcp` | MCP server (stdio) for any client: Claude Code, OpenCode, Hermes, ZeroClaw, OpenClaw. Tools: tasks, queue (`queue_claim` is the lock), `pipeline_intake`, `workspaces_list`, `workspace_status`, `graph_nodes`, `graph_node`, `sync_now`. |
| `ventosa-graph` | The engine CLI on a workspace: `status`, `attach <step> <ID>`, `write …` (the only AI write path), `new`, `edit`, `index`. |
| `ventosa-app` | Opens a white-label web app for a workspace: `labelling`, `teleprompter` (in-house recording: camera + mic, mic only, or an external camera), `uploads`. `custom <id>` runs an app the AI built in `apps/<id>/` (see `docs/AGENT-PROTOCOL.md`, "Apps the AI builds"). |
| `ventosa-init` | Creates a workspace from a template (`media`, `podcast`) or registers an existing folder. |
| `ventosa-daemon` | Sync + schedules without the window (`--once` for cron). |
| `ventosa-import` | One-off import of todos from another system's JSON export. |
| `ventosa-launcher` | Pulls `main` from GitHub, rebuilds only when sources changed, installs everything, starts the app once. |

## Product vs. your workspace

The product (this repo) is generic. A **workspace** is a folder that holds what belongs to one user or brand:

- `workspace.json`: brand, unit (video/episode), platform, sponsor line, ports, rules
- `pipeline/skill-graph/`: the steps and each node's skill
- `tone.md`, `meta/`, `research/`: knowledge the AI reads
- data: labels, clips, scripts, assets, events

Ventosa never writes a workspace's state directly. It runs the workspace's engine commands and reads `pipeline/pipeline.json`. Every 20 s it syncs each workspace into the queue, one live item per subject: AI steps become `queue ai` items and human steps become to-dos. See `plan.md` for the full split and `docs/AGENT-PROTOCOL.md` for agents.

## Install into your AI client

One command wires the MCP server and the Ventosa graph skill into an AI client:

    ventosa-install                          # every client whose config dir exists
    ventosa-install --client claude --workspace metalvj
    ventosa-install --client all --scope project --project ~/Projects/app
    ventosa-install --print                  # snippets, usage and what was installed
    ventosa-install --uninstall --client opencode

Clients: `claude`, `opencode`, `codex` (merged into their config, other keys kept), and `hermes`, `zeroclaw`, `openclaw` (generic snippet in `~/.local/share/ventosa/clients/`, format unverified). Options: `--dry-run`, `--mcp-bin PATH`. Re-running is safe. Restart the client afterwards. The result is written to `$XDG_DATA_HOME/ventosa/INSTALL.md`. The launcher runs it once on first start.

## Simple visualizer

`ventosa-graph visualizer --audio song.wav [--midi song.mid] --title … --artist … --out video.mp4`: a palette gradient, the notes as coloured bars (MIDI piano roll, or a note spectrum from the audio), song info placeholders, a progress line. Also the gallery node `simple_visualizer` and the tool `visualizer_render`.

## Video preview

`preview.rs` finds the workspace's edits, camera takes and renders and opens them in mpv (with a playback bar), vlc, ffplay or xdg-open. `VENTOSA_PLAYER` overrides the player.

## Files

| What | Where |
|---|---|
| tasks, queue, run logs | `$XDG_DATA_HOME/ventosa/` (`tasks.json`, `queue.json`, `runs/`) |
| config (brand, provider, workspaces, schedules) | `$XDG_CONFIG_HOME/ventosa/config.toml` |
| engine, web apps, templates (installed) | `$XDG_DATA_HOME/ventosa/runtime/` |

API keys are never written to config. `provider.api_key_env` names the environment variable that holds the key.

## Build

    cargo build --release
    cargo test --lib

The engine and web apps use only the Python standard library and plain HTML/JS. The app uses egui only.

License: AGPL-3.0-or-later.
