# Ventosa — plan

Ventosa is a content manager for human + AI production. The AI sees the workflow as a **skill graph** (one step node at a time, each with its own skill). The human sees it as a **to-do list and queues** of what the AI generated. Internal example: MetalVJ video production. Second brand: a podcast.

This file is the single plan. When a phase lands, mark it done in the same commit.

## The split: product vs. user

| Product (this repo, open source) | Attached to the user (a *workspace* folder) |
|---|---|
| Tasks, queue, MCP server, launcher | `workspace.json`: brand, unit (video/episode), platform, sponsor line, ports |
| Graph engine: stage computation, attach lock, AI write gate (`engine/`) | `pipeline/skill-graph/steps.json` + `nodes/*.md`: the steps and each node's skill |
| White-label apps: teleprompter, labelling (`apps/`) | Knowledge: tone, meta script, research, structures |
| Sync engine (workspace → queue), scheduler (cron), provider runner | Data: scripts, labels, clips, assets, events |
| Templates: `templates/media`, `templates/podcast`; `ventosa-init` | Rendering paths (HyperFrames, model-demo, filmcraft) named in knowledge/nodes |
| Onboarding page, settings (brand, provider) | API keys (env vars only, never in files) |

A workspace has the same layout as `~/Projects/metalvj-media`, so that repo is already a valid workspace. Ventosa never edits it; it only reads its state and runs its declared commands.

## Phases

1. **[done] Core** — tasks, queue, MCP (`list/add/set_done/delete`, `queue_*`, `queue_claim`, `pipeline_intake`), config (brand, provider), importer, launcher (sync master → build on input change → install → start once).
2. **[done] Engine + templates** — `engine/` (Python, stdlib only): `status`, `skill`, `ai_write`, `scripts_lib`, `edit`, `build_options`, `build_index`, app servers; root = `$VENTOSA_WORKSPACE` (or nearest `workspace.json` up from cwd). Brand strings, sponsor line, title limit, agent command, ports come from `workspace.json`. `templates/media` and `templates/podcast` hold generic graphs, node skills and knowledge stubs. `ventosa-graph` CLI wraps the engine. `ventosa-init <template> <dir>` creates and registers a workspace.
3. **[done] White-label apps** — `ventosa-teleprompter` and `ventosa-labelling`: the MetalVJ Record/Label apps, served for any workspace; title, accent, unit and attach command come from `GET /api/brand`.
4. **[done] Active sync** — config `[[workspaces]]` (name, root, refresh command, state file). Every 20 s (app thread or `ventosa-daemon`): refresh, read state, upsert one live queue item per subject (key `ws:subject:stage`); AI steps → kind `ai`, human steps → kind `todo`; when a subject moves on, the old item becomes `done`.
5. **[done] Intake (phase 1 for the AI)** — `pipeline_intake(title, workspace)` registers a new subject with the workspace's intake command (`ai_write.py new <ID> "<title>"`); the next sync puts it on node `options`. Agents claim it with `queue_claim` and attach with `ventosa-graph attach options <ID>`.
6. **[done] Graph over MCP** — `workspaces_list`, `graph_nodes`, `graph_node` (skill text, knowledge paths, write command), `workspace_status`. Any MCP client gets the skill without a skills folder. `ventosa-init` also writes a `SKILL.md` for clients that use skills.
7. **[done] Scheduler (cron)** — config `[[schedules]]` (workspace, node, every N minutes, runner `command` or `provider`, enabled). On tick: claim the oldest queued item at that node, run, log to `runs/<id>.log`, set `review`/`done`/`failed`. UI page to add, toggle and run now. `ventosa-daemon` runs schedules headless.
8. **[done] Provider runner** — Anthropic Messages API or OpenAI-compatible chat. Prompt = node skill + knowledge files + item payload. Output → `runs/<id>.md`, item → `review` (a human or agent applies it with the node's write command).
9. **[done] UI** — pages: overview (node canvas, default) · to-do (orange) · queue ai (purple) · schedules · apps · settings; MetalVJ master component kit (key|payload cells, square corners, monospace 10/12/13); header ↗ buttons open the three web pages. Onboarding on first run: brand → provider → workspace from template → MCP registration snippets per client.
10. **[done] Podcast brand** — strategy v2 with a proposed name; a podcast workspace created from the template with its own brand; registered in config.
11. **[done] Your setup** — register `~/Projects/metalvj-media` as workspace `metalvj` (sync only); launcher installs app scripts; Strata tab for the apps.
12. **[done] AI in the app** — shared tool layer (`tools.rs`) for MCP and the built-in chat; chat sidebar (default on) with `@` references and allow/ask/deny permissions (settings ▸ ai permissions); graph edit tools (step fields, dependencies `after`, add/remove steps, install skills, custom tools); `ventosa-install` wires the MCP server + skill into Claude Code, OpenCode, Codex (Hermes/ZeroClaw/OpenClaw: generic snippet) and writes `INSTALL.md`; launcher runs it once. Overview: hover card with step logs, editable step fields and skill, Ask AI / Edit with AI. Video preview in mpv → vlc → ffplay. AI-made apps (`apps/<id>/` in the workspace, `app_create`/`app_deploy`) wired to a node: `ventosa.done()` applies the app's effect through the write gate; header buttons per app.
13. **[done] AI harness** — nav bar (graph · ai · speech · settings · files); the AI full screen, a sidebar next to the graph only on wide windows; a speech instance (voice providers: ChatGPT, ElevenLabs, DeepInfra, OpenRouter) saved as chats like text; chat history in pages. Executions (`batch.rs`): start a self-starting graph N times from a brief, or drain the AI items waiting at a step; limits (concurrency, requests/min, retries with backoff, timeout); the app and `ventosa-daemon` run them. Decisions: `decision_ask` puts a card with options in the to-do; every graph has `elevate` rules (decisions, edge cases, dangerous scenarios). Internal prompt + meta graph (`help.rs`, `@help`). Recommended tools with a caution popup (HyperFrames, MetalVJ + MCP, storytold craft MCPs, Blender MCP, ffmpeg…). Onboarding wizard with progress, AI required, bio → the AI drafts the first graph. Servers: `app_publish` over SSH (static or service with events pulled back). File explorer. Simple visualizer (`engine/visualizer.py`, gallery node, `visualizer_render`). `ventosa-app` keeps a workspace's own label/record servers (metalvj-media); niri starts MetalVJ and those pages at login.

14. **[done] Governance + dashboards** — Ventosa's focus: identities, lifecycle and access for AI-oriented business processes and in-house apps. `identity.rs` (`identity.json`: roles, users, PBKDF2-SHA256, enabled/expiry/last login, `audit.jsonl`); with users, login is required everywhere: the desktop app, `ventosa-portal` (new launcher-only egui app: the role's apps, docs, and a chat limited to the role's tools), and every web app (HTTP Basic auth in `engine/auth.py`, per-app grants, `VENTOSA_BIND` for the LAN). Roles without graph control land in the launcher. Navigation: nav bar graph · to-do · apps · ai · access · docs · settings · files · speech (speech has no view: a status widget); header tabs gone; the graph takes the full width, the focused item's panel opens under it at half height (nodes, workspace, queue ai, schedules), a click on empty ground closes it; the activity widget over the graph merges queue ai + schedules with quick-view numbers. Regions are resizable (splits in `ui.json`); payloads centred; one "open apps ▾" dropdown. Queue ai as cron-style jobs with a time window. Access view: users, roles, tool picker overlay (after MetalVJ's + add), trusted sources, Telegram bots (`bots.rs`, run in the app), audit. Docs: an egui wiki (`wiki.rs`, built-in `docs/wiki/*.md`, edits in the user folder, image slots per provider). Files view in MetalVJ's library look. Chat: MetalVJ's prompt drawer and reported-usage context bar. Usage log (`usage.rs`, provider-reported tokens per workspace/subject/step). New web apps: dashboard (uploads per platform + total, tokens per video by step with a total line, phase KPIs; time filter) and label studio (dataset timelines labelled on rendered video files; bulk render queue + content-hash cache; verdict, notes, marks; writes the trainer's status file). Meta graph: multi-scene consistency policy over an array of trusted sources (`trusted.rs`, `knowledge/trusted-sources.json`, tools `trusted_*`). Recommended: Google Cloud CLI, Google Workspace CLI.

15. **[done] Plugins, gallery rework, access limits** — `plugin.rs` + `docs/PLUGINS.md`: one JSON file adds gallery nodes, external-app connections (human-owned work), a settings section and shell tools; policy enforced in `validate` (declared permissions, slugs, caps, no secrets or installers, shell tools human-only, AI installs disabled, audit); Settings ▸ Plugins; AI tools `plugin_list/validate/install/remove`. Manual start has a **comment** carried to every phase (`<<name>>` variables, `graph::comment_block` in provider prompts and `skill.py attach`). The launcher and the apps menu list only the apps a graph uses (`graph::app_ids`). The graph bar is MetalVJ's floating glyph strip. Input kind "not stated" is now "automatic". Gallery: pool absorbs thesis; Create Motion; Organize/Mount Audio FX; Trim Clip (per clip); Transcript with exact phrase times (WhisperX); clearer Chapters; Schedule Publication; **Label Media** (media-labelling app) and **Folder** (opens Files with the file selected) replace publish/upload in the gallery. Files view previews video/audio inline (ffprobe facts, scrub frame). Access: roles list their `connections`, installs are admin-only, chat tool calls stay inside the user's workspaces, `file_read` refuses secrets/hidden/symlinked paths. Providers after OpenClaw: several keys per provider rotate on rate limit/auth, `fallbacks` model chain, failure classes, Test connection.

## Rules

- Ventosa never writes a workspace's state files directly; only through the workspace's commands (the engine's write gate).
- API keys only from environment variables.
- One target dir: the launcher clone's (`~/.cache/ventosa/target`); dev builds use `CARGO_TARGET_DIR` pointing there (disk is tight).
- The engine is stdlib-only Python; the app is egui-only Rust.

## Next

- Hardware/live pass of phase 14: a labeller account end to end (launcher → studio), a bot on a real Telegram chat, the portal on the LAN behind Caddy.
- Label studio replaces MetalVJ's in-app dataset labelling: point `studio.json ▸ render` at a built `example-proof`, then retire the in-app panel in MetalVJ.
- Docs screenshots: the image slots (`img/<provider>/*.png`) are empty until real captures are dropped in.
- Tauri fallback for the web apps is documented, not built.

- Hardware pass on in-house recording (camera + mic, mic only) in the teleprompter, on a real show.
- Audio edit path for the podcast template (`edit` node is manual today: DAW or ffmpeg).
- Provider runs: an "apply" button that feeds a reviewed provider output to the node's write command.
- Verify Hermes / ZeroClaw / OpenClaw MCP config formats and add exact snippets.
- MetalVJ gen node for the simple visualizer: needs note input (basic-pitch notes) and text in metal-render; today the visualizer is Ventosa's ffmpeg renderer.
- Batches when the app and `ventosa-daemon` both run: the claim is per process; run one of them.
