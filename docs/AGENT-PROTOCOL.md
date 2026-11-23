# Agent protocol: driving the pipeline from an MCP client

`ventosa-mcp` is a stdio MCP server. Any client that can run a local stdio server can drive the pipeline: Claude Code, OpenCode, Hermes, ZeroClaw, OpenClaw.

## Register the server

Set `VENTOSA_FILE`/`VENTOSA_QUEUE` only if you don't want the defaults (`~/.local/share/ventosa/`).

- **Claude Code:** `claude mcp add ventosa -- ventosa-mcp`
- **OpenCode:** add to `opencode.json` under `mcp`: `{"ventosa": {"type": "local", "command": ["ventosa-mcp"]}}`
- **Hermes, ZeroClaw, OpenClaw:** register a stdio server with command `ventosa-mcp`. Check each client's own config format; this repo does not verify them.

## Install

To wire yourself: `ventosa-install --client <you> [--workspace NAME]`, then restart your client. It registers `ventosa-mcp` and installs the `ventosa` skill (`skills/ventosa/SKILL.md`). `ventosa-install --print` shows what was done.

## Workspaces

Each workspace is a skill graph (`workspaces_list`). Ask for its state with `workspace_status`, its steps with `graph_nodes`, and one step's skill, knowledge and write command with `graph_node`. Ventosa syncs every workspace into the queue every 20 s (`sync_now` forces it): one live item per subject, `ai` for AI steps and `todo` for human steps.

## Phase 1: title → thesis options (node `options`)

1. Human (overview ▸ phase 1) or client calls `pipeline_intake` with a `title` and a `workspace`. The subject is registered in the workspace (next id, e.g. `E02`) and the sync queues an `ai` item on node `options`.
2. Agent calls `queue_claim` with `node: "options"` and its own name in `agent`. It gets the oldest queued item and the item becomes `running`. `null` means nothing is waiting.
3. Agent calls `graph_node` for the skill, then `VENTOSA_AGENT=<name> ventosa-graph -w <root> attach options <ID>` and writes only with `ventosa-graph -w <root> write option <ID> '…'`. (A workspace with its own engine, like `metalvj-media`, uses its own `python3 pipeline/skill.py` / `ai_write.py` instead.)
4. Agent calls `queue_set_status` with `done` (or `review` if a human must choose, `failed` on error).

Only one agent should hold an item: `queue_claim` is the lock. The attach rule in the media repo is the second lock.

## Tools and permissions

The MCP server and the built-in chat share one tool layer (`src/tools.rs`). Each tool has a capability; `config.toml` `[permissions]` sets each to `allow`, `ask` or `deny` (defaults: read/write allow, graph/install/apps/run ask). The MCP server refuses `deny` tools with an error result and otherwise lets the client decide; the built-in chat asks the user for `ask` tools.

| Capability | Tools |
|---|---|
| read | `list_tasks`, `queue_list`, `workspaces_list`, `workspace_status`, `graph_nodes`, `graph_node`, `graph_steps`, `install_info`, `media_list`, `app_list` |
| write | `add_task`, `set_done`, `delete_task`, `queue_enqueue`, `queue_claim`, `queue_set_status`, `pipeline_intake`, `sync_now` |
| graph | `graph_step_set` (owner, need, done_when, command, attach, write, reads, writes, after, skill), `graph_step_add`, `graph_step_remove`, `skill_install` |
| install | `tool_install` (stores a custom shell tool in `pipeline/tools.json`), `install_client` (wires ventosa-mcp and the skill into a client; `dry_run` writes nothing) |
| apps | `app_create`, `app_write_file`, `app_deploy` (starts the app's server, returns its URL), `app_stop` |
| run | `tool_run` (runs a custom tool with `sh -c` in the workspace root; the optional `args` string is `$1`; output capped at 8 KB), `media_play` (opens a workspace file in an external player; paths outside the workspace root are refused) |

## Rules for every client

- Claim before working; never work an item you did not claim.
- Set `review` when a human decision is next (the label node is always human).
- Don't edit `queue.json`, `tasks.json` or media-repo state files by hand; use the tools.

## Apps the AI builds

An agent can give a workspace a small web app for one graph step: a form, a checklist or a review page the human opens on a phone. Finishing in the app changes the graph, and the next 20 s sync puts the human's next step (or the AI's next item) in the queue.

- **Folder:** `<workspace>/apps/<id>/` with `app.json` and `index.html` (plus any files).
- **app.json:** `{"id","title","node","effect","next","port","created"}`. `node` is a step in `pipeline/skill-graph/steps.json` (or empty). `effect` is `human-done` (the human finished `node`), `step-done` (the AI finished `node`, the agent must be attached) or `none`. `next` is one line telling the human what happens next. `port` is auto-assigned from 8790 up.
- **Library (`src/apps.rs`):** `list`, `get`, `create(ws, id, title, node, effect, next, html)` (an empty `html` uses `templates/app/index.html`), `update_file(ws, id, rel_path, text)` (no `..`, no absolute paths, `app.json` excluded), `deploy`, `stop`, `running`, `url`.
- **Server (`engine/serve_app.py <app_dir> <port>`, stdlib only):** serves the folder, `/common/*` from the web kit, `GET /api/brand`, `GET /api/state`, `GET /api/app`, `POST /api/done {"subject","note"}` and `POST /api/event {"kind","subject","data"}`. `/api/done` runs the effect through `ai_write.py human-done` or `step-done` (the write gate, so the same rules apply), then logs `app.done`. It returns `{ok, subject, stage, owner, need}`.
- **Page helper:** `apps/web/common/ventosa-app.js` exposes `ventosa.brand()`, `ventosa.state()`, `ventosa.app()`, `ventosa.done(subject, note)` and `ventosa.event(kind, subject, data)`.
- **Run:** `ventosa-app custom <id> [-w DIR] [--stop] [--no-open]`. The pid file is `$XDG_RUNTIME_DIR/ventosa-apps/<workspace>-<id>.pid`, the same one `apps::deploy` uses.

Rules: an app's `effect` is the only way it changes state. The app never writes `pipeline/` or `labelling/` files itself. A `step-done` app fails with 409 unless the agent is attached to that step, the same as the CLI.

## Video preview

`src/preview.rs`: `media(ws)` lists `assets/edits`, `assets/camera` and `renders` (mp4, mov, mkv, webm, weba, wav, m4a, mp3), newest first, with the subject taken from the filename prefix (`V03-final.mp4` → `V03`). `media_for(ws, subject)` filters it. `play(path, title, start)` opens the file in mpv, then vlc, ffplay or xdg-open, detached. `VENTOSA_PLAYER` overrides the command. The MCP tools are `media_list` (optionally one `subject`) and `media_play` (a `path` inside the workspace, or a `subject`: its newest edit, else its newest media; `start` in seconds).
