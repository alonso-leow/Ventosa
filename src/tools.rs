//! The tool layer shared by the MCP server and the built-in chat. Each tool is
//! one row of `TOOLS`: name, capability, description, input schema, handler.
//! Handlers return the text of a successful result or the error text.

use crate::config::{self, Workspace};
use crate::{apps, callback, graph, install, preview, tplog};
use crate::queue::{self, Item};
use crate::{add_from, default_path, delete, load, queue_path, save, set_done, Task};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What a tool touches. Policies in `config::Permissions` are per capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Read,
    Write,
    Graph,
    Install,
    Apps,
    Run,
}

/// One tool: its MCP definition plus the handler.
pub struct Tool {
    pub name: &'static str,
    pub cap: Cap,
    pub description: &'static str,
    /// JSON Schema for the arguments.
    pub schema: &'static str,
    pub run: fn(&Value) -> Result<String, String>,
}

type R = Result<String, String>;

/// Output limit for `tool_run`, in bytes.
const OUTPUT_LIMIT: usize = 8 * 1024;

pub static TOOLS: &[Tool] = &[
    Tool { name: "list_tasks", cap: Cap::Read, description: "List the human's to-do tasks (not the AI queue). filter: open | done | all (default open).",
        schema: r#"{"type":"object","properties":{"filter":{"type":"string","enum":["open","done","all"]}}}"#, run: list_tasks },
    Tool { name: "add_task", cap: Cap::Write, description: "Add a to-do task for the human to the Ventosa task list.",
        schema: r#"{"type":"object","required":["title"],"properties":{"title":{"type":"string"},"source":{"type":"string"}}}"#, run: add_task },
    Tool { name: "set_done", cap: Cap::Write, description: "Mark a to-do task done by id (or reopen it with done=false).",
        schema: r#"{"type":"object","required":["id"],"properties":{"id":{"type":"integer"},"done":{"type":"boolean"}}}"#, run: set_done_task },
    Tool { name: "delete_task", cap: Cap::Write, description: "Delete a to-do task by id.",
        schema: r#"{"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}"#, run: delete_task },
    Tool { name: "queue_list", cap: Cap::Read, description: "List queue items, optionally by status (queued, running, review, done, failed) or node.",
        schema: r#"{"type":"object","properties":{"status":{"type":"string"},"node":{"type":"string"}}}"#, run: queue_list },
    Tool { name: "queue_enqueue", cap: Cap::Write, description: "Add an item to the work queue. kind: todo | ai. node: skill-graph node id, if any.",
        schema: r#"{"type":"object","required":["title"],"properties":{"title":{"type":"string"},"kind":{"type":"string","enum":["todo","ai"]},"source":{"type":"string"},"node":{"type":"string"},"payload":{"type":"object"}}}"#, run: queue_enqueue },
    Tool { name: "request_human", cap: Cap::Write, description: "Ask a human to do a task you cannot: a special review, an approval, a recording, a decision outside a graph step. It appears in the To-do view of the person asked. assignee is a role id (admin, labeller…) or a user name; default admin.",
        schema: r#"{"type":"object","required":["title"],"properties":{"title":{"type":"string","description":"What the human must do, in one line."},"detail":{"type":"string","description":"Where to look and what a good result is."},"assignee":{"type":"string","description":"Role id or user name; default admin."},"workspace":{"type":"string"},"subject":{"type":"string"},"node":{"type":"string"}}}"#, run: request_human },
    Tool { name: "queue_claim", cap: Cap::Write, description: "Claim the oldest queued item for a skill-graph node (sets it running, records agent). Use this before doing the work.",
        schema: r#"{"type":"object","required":["node","agent"],"properties":{"node":{"type":"string"},"agent":{"type":"string"}}}"#, run: queue_claim },
    Tool { name: "pipeline_intake", cap: Cap::Write, description: "Phase 1: give a workspace a title (video, episode…). Registers the subject in the workspace's graph and queues it on the 'options' step (thesis options for scripting). Without a workspace, only queues an ai item.",
        schema: r#"{"type":"object","required":["title"],"properties":{"title":{"type":"string"},"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"notes":{"type":"string"},"video":{"type":"string","description":"subject id when no workspace is given"}}}"#, run: pipeline_intake },
    Tool { name: "workspaces_list", cap: Cap::Read, description: "List workspaces (each is a skill graph with its own brand, knowledge and apps).",
        schema: r#"{"type":"object","properties":{}}"#, run: workspaces_list },
    Tool { name: "workspace_status", cap: Cap::Read, description: "Where every subject of a workspace is in its graph, what waits on AI steps, attached agents and recent events.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."}}}"#, run: workspace_status },
    Tool { name: "graph_nodes", cap: Cap::Read, description: "The step nodes of a workspace's skill graph (workspace `meta`: the meta graph about Ventosa itself, start at soul), in order, with owner (ai/human) and need.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."}}}"#, run: graph_nodes },
    Tool { name: "graph_node", cap: Cap::Read, description: "One step node: its skill text, knowledge files to read, the attach command and the only write command. Read this before working a claimed item.",
        schema: r#"{"type":"object","required":["workspace","node"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"node":{"type":"string"}}}"#, run: graph_node },
    Tool { name: "sync_now", cap: Cap::Write, description: "Recompute every enabled workspace's graph and update the queue now. Ventosa also does this every 20 s while open.",
        schema: r#"{"type":"object","properties":{}}"#, run: sync_now },
    Tool { name: "queue_set_status", cap: Cap::Write, description: "Set the status of one AI queue item by id: queued, running, review, done or failed.",
        schema: r#"{"type":"object","required":["id","status"],"properties":{"id":{"type":"integer"},"status":{"type":"string","enum":["queued","running","review","done","failed"]}}}"#, run: queue_set_status },
    Tool { name: "graph_step_set", cap: Cap::Graph, description: "Set one field of a step: owner (ai|human), model (provider:model), input (accepted|topic|draft|reference: what the value given to the node counts as), search_* (research nodes), need, done_when, command, attach, write, reads, writes, after (step ids this step depends on), or skill (writes the node's skill text).",
        schema: r#"{"type":"object","required":["workspace","step","field"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"step":{"type":"string"},"field":{"type":"string","enum":["owner","need","done_when","command","attach","write","reads","writes","after","skill","model","input","search_provider","search_key_env","search_query","search_max"]},"value":{"description":"A string; for reads, writes and after, a list or comma-separated ids."}}}"#, run: graph_step_set },
    Tool { name: "graph_step_add", cap: Cap::Graph, description: "Add a step to a workspace's skill graph before another step (default: before done). owner is ai or human. Then graph_step_set its skill, reads, writes, command and after; add a human *_ok gate after anything to approve. Every subject walks every step, so a per-video node needs no loop (help topic recipe_bulk).",
        schema: r#"{"type":"object","required":["workspace","id","owner","need"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"id":{"type":"string"},"owner":{"type":"string","enum":["ai","human"]},"need":{"type":"string"},"skill":{"type":"string"},"before":{"type":"string"}}}"#, run: graph_step_add },
    Tool { name: "web_research", cap: Cap::Read, description: "Internet research with a configurable provider (tavily, brave, exa, serper, perplexity, searxng, duckduckgo). Give workspace and step to use that research node's provider and query template ({title} is the subject), or provider and query directly. Returns numbered sources to cite.",
        schema: r#"{"type":"object","properties":{"workspace":{"type":"string"},"step":{"type":"string"},"subject":{"type":"string","description":"Title or topic filling {title}."},"provider":{"type":"string"},"query":{"type":"string"},"max":{"type":"integer"}}}"#, run: web_research },
    Tool { name: "graph_step_remove", cap: Cap::Graph, description: "Remove a step from a workspace's skill graph (not the done step) and its dependencies on other steps.",
        schema: r#"{"type":"object","required":["workspace","step"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"step":{"type":"string"}}}"#, run: graph_step_remove },
    Tool { name: "skill_install", cap: Cap::Graph, description: "Save a new skill file under the workspace and wire it to a step (listed in files, added to the step's reads).",
        schema: r#"{"type":"object","required":["workspace","step","name","text"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"step":{"type":"string"},"name":{"type":"string"},"text":{"type":"string"}}}"#, run: skill_install },
    Tool { name: "plugin_list", cap: Cap::Read, description: "List the installed plugins (id, version, enabled, what each adds: gallery nodes, connections, settings, tools).",
        schema: r#"{"type":"object","properties":{}}"#, run: plugin_list },
    Tool { name: "plugin_validate", cap: Cap::Read, description: "Check a plugin file against the plugin policy without installing it. Give the file's path or its JSON text. Lists every reason it would be rejected.",
        schema: r#"{"type":"object","properties":{"path":{"type":"string","description":"Path of the plugin .json file."},"json":{"type":"string","description":"The plugin file's text."}}}"#, run: plugin_validate },
    Tool { name: "plugin_install", cap: Cap::Install, description: "Install a plugin from ONE input file (path or JSON text): gallery nodes, external-app connections, a settings section. It installs DISABLED: a human enables it in Settings > Plugins. A plugin with shell tools cannot be installed by the AI. See docs/PLUGINS.md for the format.",
        schema: r#"{"type":"object","properties":{"path":{"type":"string","description":"Path of the plugin .json file."},"json":{"type":"string","description":"The plugin file's text."}}}"#, run: plugin_install },
    Tool { name: "plugin_remove", cap: Cap::Install, description: "Remove an installed plugin by id (its nodes leave the gallery; nodes already in a graph stay).",
        schema: r#"{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}"#, run: plugin_remove },
    Tool { name: "tool_install", cap: Cap::Install, description: "Store a custom shell tool for a workspace (pipeline/tools.json). Run it with tool_run.",
        schema: r#"{"type":"object","required":["workspace","name","command"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"name":{"type":"string"},"description":{"type":"string"},"command":{"type":"string"}}}"#, run: tool_install },
    Tool { name: "tool_run", cap: Cap::Run, description: "Run a custom tool with sh -c in the workspace root. The optional args string is passed as $1. Returns the exit code and output (8 KB max).",
        schema: r#"{"type":"object","required":["workspace","name"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"name":{"type":"string"},"args":{"type":"string"}}}"#, run: tool_run },
    Tool { name: "graph_steps", cap: Cap::Read, description: "The raw steps.json of a workspace's skill graph (start here before editing a graph or fanning out many videos): every step with owner, need, done_when, reads, writes and after. workspace is a name from workspaces_list.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."}}}"#, run: graph_steps },
    Tool { name: "install_client", cap: Cap::Install, description: "Install Ventosa's MCP server (ventosa-mcp) and its ventosa skill into an AI client. client is claude, opencode, codex, hermes, zeroclaw, openclaw or all. Returns the report lines; dry_run shows what would change without writing.",
        schema: r#"{"type":"object","required":["client"],"properties":{"client":{"type":"string","description":"claude, opencode, codex, hermes, zeroclaw, openclaw, a comma list, or all."},"scope":{"type":"string","enum":["user","project"],"description":"user (default) or project."},"project":{"type":"string","description":"Project directory for scope project (default: current directory)."},"workspace":{"type":"string","description":"Default workspace name for the skill, from workspaces_list."},"dry_run":{"type":"boolean"}}}"#, run: install_client },
    Tool { name: "install_info", cap: Cap::Read, description: "How Ventosa is installed here and how to wire AI clients: the INSTALL.md that ventosa-install wrote, or its usage text when none exists.",
        schema: r#"{"type":"object","properties":{}}"#, run: install_info },
    Tool { name: "media_list", cap: Cap::Read, description: "List the video and audio files of a workspace (edits, camera takes, renders), newest first, with subject ids such as V03. Each row has a teleprompter field: the text line the teleprompter log says was being read when the file was recorded (line, text, confidence 0-1, method), or null. subject filters to one subject. workspace is a name from workspaces_list.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"subject":{"type":"string","description":"Subject id, e.g. V03."}}}"#, run: media_list },
    Tool { name: "teleprompter_log", cap: Cap::Read, description: "What the teleprompter recorded and when: every record take with the text lines on screen, and which media file each take was matched to by time overlap (confidence, clock offset, unmatched files). refresh (default true) recomputes the mapping first. video filters to one video id such as V03.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"video":{"type":"string","description":"Video id, e.g. V03."},"refresh":{"type":"boolean"}}}"#, run: teleprompter_log },
    Tool { name: "callback", cap: Cap::Write, description: "Send a subject back to an earlier skill-graph node and re-run from there. mode full: that node and every node downstream run again, one after another. mode fast (default): only that node runs, then each next node runs only if its input files changed; valid nodes are skipped. reason is required. Use stale first to find the node whose inputs changed. caller = the node you are working on, when you call this from inside a run. dry_run returns the plan only. Refusals (depth, cycle, repeats) must go to decision_ask.",
        schema: r#"{"type":"object","required":["workspace","subject","node","reason"],"properties":{"workspace":{"type":"string"},"subject":{"type":"string","description":"Subject id, e.g. V03."},"node":{"type":"string","description":"The step to go back to."},"mode":{"type":"string","enum":["full","fast"]},"reason":{"type":"string"},"caller":{"type":"string"},"dry_run":{"type":"boolean"}}}"#, run: callback_tool },
    Tool { name: "stale", cap: Cap::Read, description: "Which nodes of a workspace's graph are out of date: per subject and node, whether the files the node read changed since it ran (root cause) or an upstream node ran after it, with the reasons and the nodes downstream. Also lists open callbacks. subject is optional.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string"},"subject":{"type":"string"}}}"#, run: stale_tool },
    Tool { name: "callback_stop", cap: Cap::Write, description: "Stop an open callback by id (its reopened step stays open).",
        schema: r#"{"type":"object","required":["workspace","id"],"properties":{"workspace":{"type":"string"},"id":{"type":"integer"}}}"#, run: callback_stop },
    Tool { name: "media_play", cap: Cap::Run, description: "Open a workspace video or audio file in an external player (mpv, vlc, ffplay or xdg-open) with a playback bar. Give path (relative to the workspace root, or absolute inside it) or subject: that subject's newest edit, else its newest media. start is seconds.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"path":{"type":"string","description":"File inside the workspace root."},"subject":{"type":"string","description":"Subject id, e.g. V03."},"start":{"type":"number","description":"Start position in seconds."}}}"#, run: media_play },
    Tool { name: "app_list", cap: Cap::Read, description: "List the small web apps an AI built for a workspace (apps/<id>/): id, title, graph node, effect, next line, url and pid (null when the server is not running). workspace is a name from workspaces_list.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."}}}"#, run: app_list },
    Tool { name: "app_create", cap: Cap::Apps, description: "Create a small web app the human opens on a phone, wired to one graph step. effect: human-done (the human finished node), step-done (the AI finished node; the agent must be attached) or none. next: one line telling the human what happens next. html: optional page, empty uses the starter. The page calls ventosa.done(subject, note) from /common/ventosa-app.js, which applies effect to node through the engine write gate; the next 20 s sync turns the new stage into the human's to-do or an AI queue item. Then app_write_file for more files and app_deploy to serve it.",
        schema: r#"{"type":"object","required":["workspace","id","title","node","effect","next"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"id":{"type":"string","description":"a-z, 0-9 and '-', up to 40 characters."},"title":{"type":"string"},"node":{"type":"string","description":"Step id in the graph this app serves; empty only with effect none."},"effect":{"type":"string","enum":["human-done","step-done","none"]},"next":{"type":"string","description":"One line the human sees about what happens next."},"html":{"type":"string","description":"Full index.html; empty uses the starter template."}}}"#, run: app_create },
    Tool { name: "app_write_file", cap: Cap::Apps, description: "Write one text file inside an app's folder (apps/<id>/), such as css or js that its index.html references. The path is relative, with no '..'; app.json is set by app_create.",
        schema: r#"{"type":"object","required":["workspace","id","path","text"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"id":{"type":"string","description":"App id."},"path":{"type":"string","description":"Relative path inside the app folder, e.g. js/app.js."},"text":{"type":"string"}}}"#, run: app_write_file },
    Tool { name: "app_deploy", cap: Cap::Apps, description: "Start an app's local server (once) and return its URL on 127.0.0.1 for the human to open. The app must exist (app_create).",
        schema: r#"{"type":"object","required":["workspace","id"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"id":{"type":"string","description":"App id."}}}"#, run: app_deploy },
    Tool { name: "app_stop", cap: Cap::Apps, description: "Stop an app's local server. The app's files stay on disk.",
        schema: r#"{"type":"object","required":["workspace","id"],"properties":{"workspace":{"type":"string","description":"Workspace name, from workspaces_list."},"id":{"type":"string","description":"App id."}}}"#, run: app_stop },
    Tool { name: "pipeline_attach", cap: Cap::Write, description: "Attach an agent to one AI step for one subject. Obligatory before pipeline_write: the engine refuses writes from agents that are not attached to that step.",
        schema: r#"{"type":"object","required":["workspace","step","subject"],"properties":{"workspace":{"type":"string"},"step":{"type":"string","description":"Step id (AI step)."},"subject":{"type":"string","description":"Subject id, e.g. V03."},"agent":{"type":"string","description":"Agent name recorded on the lock (default ventosa-ai)."}}}"#, run: pipeline_attach },
    Tool { name: "pipeline_write", cap: Cap::Write, description: "Write through the workspace engine's write gate (ai_write): the only way an AI finishes a step. args = the node's write command after `ventosa-graph write`, e.g. [\"step-done\",\"V03\",\"research\",\"wrote research/V03.md\"] or [\"script\",\"V03\",\"scripts/V03/draft.md\"]. See graph_node ▸ write.",
        schema: r#"{"type":"object","required":["workspace","args"],"properties":{"workspace":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"agent":{"type":"string","description":"Agent name (must match the attach)."}}}"#, run: pipeline_write },
    Tool { name: "decision_ask", cap: Cap::Write, description: "Elevate a decision to the human instead of guessing: it shows in their to-do as a decision card with your options (they pick one or write their own). Use it for anything the graph's elevate rules list (see graph_node ▸ elevate), edge cases and dangerous scenarios. Then stop working on that subject; read the answer later with decision_list.",
        schema: r#"{"type":"object","required":["workspace","question","options"],"properties":{"workspace":{"type":"string"},"question":{"type":"string","description":"One question, answerable by picking an option."},"options":{"type":"array","items":{"type":"string"},"description":"2-5 concrete options, the recommended one first."},"context":{"type":"string","description":"What the human needs to know to decide, briefly."},"risk":{"type":"string","description":"low | edge | dangerous"},"step":{"type":"string"},"subject":{"type":"string"}}}"#, run: decision_ask },
    Tool { name: "decision_list", cap: Cap::Read, description: "Decisions raised to the human, with their answers. status: open | answered | all (default all).",
        schema: r#"{"type":"object","properties":{"workspace":{"type":"string"},"subject":{"type":"string"},"status":{"type":"string"}}}"#, run: decision_list },
    Tool { name: "decision_answer", cap: Cap::Write, description: "Answer a decision (for the human's own client): the chosen option or free text.",
        schema: r#"{"type":"object","required":["id","answer"],"properties":{"id":{"type":"integer"},"answer":{"type":"string"}}}"#, run: decision_answer },
    Tool { name: "graph_elevate_set", cap: Cap::Graph, description: "Set the graph-wide elevation rules: which decisions go to the human, edge cases, dangerous scenarios. Per-step rules: graph_step_set field elevate.",
        schema: r#"{"type":"object","required":["workspace","text"],"properties":{"workspace":{"type":"string"},"text":{"type":"string"}}}"#, run: graph_elevate_set },
    Tool { name: "trusted_sources_list", cap: Cap::Read, description: "The workspace's trusted sources: the approved refs (characters, assets, locations, styles, voices, brand) every scene must build over, plus open proposals. Read before any scene work.",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string"}}}"#, run: trusted_list },
    Tool { name: "trusted_source_add", cap: Cap::Write, description: "Propose a trusted source (approved=false until a human approves it). kind: character|asset|location|style|voice|brand. refs: workspace paths or URLs; locked: traits that must never change; recipe: seeds/prompt fragments/models that reproduce it. Then raise decision_ask for approval.",
        schema: r#"{"type":"object","required":["workspace","kind","name"],"properties":{"workspace":{"type":"string"},"id":{"type":"string"},"kind":{"type":"string","enum":["character","asset","location","style","voice","brand"]},"name":{"type":"string"},"refs":{"type":"array","items":{"type":"string"}},"locked":{"type":"array","items":{"type":"string"}},"recipe":{"type":"string"},"notes":{"type":"string"},"agent":{"type":"string"}}}"#, run: trusted_add },
    Tool { name: "trusted_source_used", cap: Cap::Write, description: "Record that a scene used these trusted sources (scene as SUBJECT#scene, e.g. V03#s2).",
        schema: r#"{"type":"object","required":["workspace","ids","scene"],"properties":{"workspace":{"type":"string"},"ids":{"type":"array","items":{"type":"string"}},"scene":{"type":"string"}}}"#, run: trusted_used },
    Tool { name: "trusted_source_approve", cap: Cap::Graph, description: "Approve (or un-approve) a trusted source. Normally the human does this in access ▸ trusted sources.",
        schema: r#"{"type":"object","required":["workspace","id"],"properties":{"workspace":{"type":"string"},"id":{"type":"string"},"approved":{"type":"boolean"}}}"#, run: trusted_approve },
    Tool { name: "help", cap: Cap::Read, description: "Call this first. Ventosa's help: the soul (why Ventosa exists, what to decide vs ask) and the meta graph index; topic=<node id> reads one node and its edges. The meta graph (how a video goes from idea to published, who does what and where) and, for a workspace (and optionally a step), what to do now and where.",
        schema: r#"{"type":"object","properties":{"workspace":{"type":"string"},"step":{"type":"string"},"topic":{"type":"string","description":"A meta graph node id (soul, decide_vs_ask, recipe_bulk, tools_callbacks...). No topic: the soul and the index."}}}"#, run: help },
    Tool { name: "batch_start", cap: Cap::Run, description: "Start an execution. kind start: create count subjects from brief and run every AI step until done (human steps pause them). kind drain: run the AI items waiting at node. Respects settings ▸ limits (concurrency, requests/min, retries).",
        schema: r#"{"type":"object","required":["workspace","kind"],"properties":{"workspace":{"type":"string"},"kind":{"type":"string","enum":["start","drain"]},"count":{"type":"integer"},"brief":{"type":"string"},"node":{"type":"string"},"runner":{"type":"string","enum":["builtin","command"]},"command":{"type":"string"}}}"#, run: batch_start },
    Tool { name: "batch_list", cap: Cap::Read, description: "Executions (batches) with status, subjects, runs, failures and their latest log lines.",
        schema: r#"{"type":"object","properties":{"workspace":{"type":"string"}}}"#, run: batch_list },
    Tool { name: "batch_stop", cap: Cap::Run, description: "Stop a batch (runs in flight finish; nothing new starts).",
        schema: r#"{"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}"#, run: batch_stop },
    Tool { name: "servers_list", cap: Cap::Read, description: "Servers configured for publishing apps (settings ▸ servers): name, host, public URL, enabled.",
        schema: r#"{"type":"object","properties":{}}"#, run: servers_list },
    Tool { name: "app_publish", cap: Cap::Apps, description: "Publish an app to a server over SSH so the public can use it (forms, recipe callbacks). mode static: files only; service: also runs the app server there so ventosa.done() and POSTs work; results are pulled back with app_pull. Returns the public URL.",
        schema: r#"{"type":"object","required":["workspace","id","server"],"properties":{"workspace":{"type":"string"},"id":{"type":"string"},"server":{"type":"string"},"mode":{"type":"string","enum":["static","service"]}}}"#, run: app_publish },
    Tool { name: "app_pull", cap: Cap::Apps, description: "Bring a published app's events (form posts, done calls) back from the server into apps/<id>/remote-events.jsonl.",
        schema: r#"{"type":"object","required":["workspace","id","server"],"properties":{"workspace":{"type":"string"},"id":{"type":"string"},"server":{"type":"string"}}}"#, run: app_pull },
    Tool { name: "tools_recommended", cap: Cap::Read, description: "Recommended tools for making videos (MetalVJ, MetalVJ MCP, HyperFrames, storytold craft MCPs, Blender MCP, ffmpeg, mpv, schedules, voice) with present/missing, links and install commands.",
        schema: r#"{"type":"object","properties":{}}"#, run: tools_recommended },
    Tool { name: "visualizer_render", cap: Cap::Run, description: "Simple visualizer: render an elegant YouTube video for a song (gradient of the palette, the notes as coloured bars from a MIDI file or a note spectrum from the audio, song info placeholders, a progress line). Paths are relative to the workspace. seconds > 0 renders a preview of that length.",
        schema: r#"{"type":"object","required":["workspace","audio"],"properties":{"workspace":{"type":"string"},"audio":{"type":"string"},"midi":{"type":"string"},"out":{"type":"string","description":"Default assets/renders/<audio name>-visualizer.mp4"},"title":{"type":"string"},"artist":{"type":"string"},"lines":{"type":"array","items":{"type":"string"}},"palette":{"type":"string","description":"Comma-separated hex colours; the last is the ground."},"size":{"type":"string"},"seconds":{"type":"number"}}}"#, run: visualizer_render },
    Tool { name: "files_list", cap: Cap::Read, description: "List a folder of a workspace (relative path, default the root).",
        schema: r#"{"type":"object","required":["workspace"],"properties":{"workspace":{"type":"string"},"path":{"type":"string"}}}"#, run: files_list },
    Tool { name: "file_read", cap: Cap::Read, description: "Read a text file of a workspace (relative path; first 64 KB).",
        schema: r#"{"type":"object","required":["workspace","path"],"properties":{"workspace":{"type":"string"},"path":{"type":"string"}}}"#, run: file_read },
];

/// Tool definitions in MCP shape: `{name, description, inputSchema}`.
pub fn list() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            let schema: Value = serde_json::from_str(t.schema).expect("tool schema is valid JSON");
            json!({"name": t.name, "description": t.description, "inputSchema": schema})
        })
        .collect()
}

/// Capability of a tool. Unknown names count as `Run`, the most guarded.
pub fn cap(name: &str) -> Cap {
    TOOLS.iter().find(|t| t.name == name).map_or(Cap::Run, |t| t.cap)
}

/// Run a tool and return an MCP result: `{content:[{type:"text",text}], isError?}`.
pub fn call(name: &str, args: &Value) -> Value {
    let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
        return err(format!("unknown tool {name}"));
    };
    match (tool.run)(args) {
        Ok(s) => text(s),
        Err(e) => err(e),
    }
}

fn text(s: String) -> Value {
    json!({"content": [{"type": "text", "text": s}]})
}

fn err(s: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": s.into()}], "isError": true})
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

fn req<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("{key} is required"))
}

fn id_arg(args: &Value) -> Result<u64, String> {
    args.get("id").and_then(Value::as_u64).ok_or_else(|| "id is required".to_string())
}

fn json_text<T: Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn save_to<T: Serialize + ?Sized>(path: &PathBuf, v: &T) -> Result<(), String> {
    save(path, v).map_err(|e| format!("save failed: {e}"))
}

/// The workspace named by the `workspace` argument.
fn ws_arg(args: &Value) -> Result<Workspace, String> {
    let name = req(args, "workspace")?;
    let cfg = config::load(&config::default_path());
    cfg.workspace(name).cloned().ok_or_else(|| format!("no workspace {name}; see workspaces_list"))
}

fn list_tasks(a: &Value) -> R {
    let tasks: Vec<Task> = load(&default_path());
    let filter = str_arg(a, "filter").unwrap_or("open");
    let rows: Vec<&Task> = tasks
        .iter()
        .filter(|t| match filter {
            "done" => t.done,
            "all" => true,
            _ => !t.done,
        })
        .collect();
    Ok(json_text(&rows))
}

fn add_task(a: &Value) -> R {
    let title = req(a, "title")?;
    let path = default_path();
    let mut tasks: Vec<Task> = load(&path);
    let task = add_from(&mut tasks, title, str_arg(a, "source").unwrap_or("mcp"));
    save_to(&path, &tasks)?;
    Ok(json_text(&task))
}

fn set_done_task(a: &Value) -> R {
    let id = id_arg(a)?;
    let done = a.get("done").and_then(Value::as_bool).unwrap_or(true);
    let path = default_path();
    let mut tasks: Vec<Task> = load(&path);
    if !set_done(&mut tasks, id, done) {
        return Err(format!("no task with id {id}"));
    }
    save_to(&path, &tasks)?;
    Ok(format!("task {id} done={done}"))
}

fn delete_task(a: &Value) -> R {
    let id = id_arg(a)?;
    let path = default_path();
    let mut tasks: Vec<Task> = load(&path);
    if !delete(&mut tasks, id) {
        return Err(format!("no task with id {id}"));
    }
    save_to(&path, &tasks)?;
    Ok(format!("task {id} deleted"))
}

fn queue_list(a: &Value) -> R {
    let items: Vec<Item> = load(&queue_path());
    let status = str_arg(a, "status");
    let node = str_arg(a, "node");
    let rows: Vec<&Item> = items
        .iter()
        .filter(|i| status.is_none_or(|s| i.status == s))
        .filter(|i| node.is_none_or(|n| i.node.as_deref() == Some(n)))
        .collect();
    Ok(json_text(&rows))
}

fn queue_enqueue(a: &Value) -> R {
    let title = req(a, "title")?;
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    let kind = str_arg(a, "kind").unwrap_or("todo");
    let source = str_arg(a, "source").unwrap_or("mcp");
    let node = str_arg(a, "node").map(String::from);
    let payload = a.get("payload").cloned().unwrap_or(json!({}));
    let item = queue::enqueue(&mut items, title, kind, source, node, payload);
    save_to(&path, &items)?;
    Ok(json_text(&item))
}

fn request_human(a: &Value) -> R {
    let title = req(a, "title")?;
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    let payload = json!({
        "assignee": str_arg(a, "assignee").unwrap_or("admin"),
        "detail": str_arg(a, "detail").unwrap_or(""),
        "workspace": str_arg(a, "workspace").unwrap_or(""),
        "subject": str_arg(a, "subject").unwrap_or(""),
        "asked_by": "ai",
    });
    let item = queue::enqueue(&mut items, title, "todo", "ai-request", str_arg(a, "node").map(String::from), payload);
    save_to(&path, &items)?;
    Ok(format!("asked {} (to-do #{})", item.payload["assignee"].as_str().unwrap_or("admin"), item.id))
}

fn queue_claim(a: &Value) -> R {
    let (Some(node), Some(agent)) = (str_arg(a, "node"), str_arg(a, "agent")) else {
        return Err("node and agent are required".into());
    };
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    match queue::claim(&mut items, node, agent) {
        Some(item) => {
            save_to(&path, &items)?;
            Ok(json_text(&item))
        }
        None => Ok("null".into()),
    }
}

fn pipeline_intake(a: &Value) -> R {
    if str_arg(a, "workspace").is_some() {
        let ws = ws_arg(a)?;
        let title = req(a, "title")?;
        let id = graph::intake(&ws, title, str_arg(a, "notes").unwrap_or(""))?;
        let _ = graph::sync(&ws, &queue_path());
        return Ok(format!("{id} registered in {}; queued on its first step. Next: workspace_status, then queue_claim for that step and graph_node(workspace={}).", ws.name, ws.name));
    }
    let title = req(a, "title")?;
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    let payload = match str_arg(a, "video") {
        Some(v) => json!({"title": title, "video": v}),
        None => json!({"title": title}),
    };
    let item = queue::enqueue(&mut items, title, "ai", "pipeline", Some("options".into()), payload);
    save_to(&path, &items)?;
    Ok(json_text(&item))
}

fn queue_set_status(a: &Value) -> R {
    let id = id_arg(a)?;
    let status = req(a, "status")?;
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    if !queue::set_status(&mut items, id, status) {
        return Err(format!("bad id {id} or status {status}"));
    }
    save_to(&path, &items)?;
    Ok(format!("item {id} -> {status}"))
}

fn workspaces_list(_: &Value) -> R {
    let cfg = config::load(&config::default_path());
    let rows: Vec<Value> = cfg
        .workspaces
        .iter()
        .map(|w| json!({"name": w.name, "root": graph::root(w), "engine": w.engine, "sync": w.enabled, "brand": graph::workspace_json(w)["brand"]}))
        .collect();
    Ok(json_text(&rows))
}

fn sync_now(_: &Value) -> R {
    let cfg = config::load(&config::default_path());
    let out: Vec<String> = cfg
        .workspaces
        .iter()
        .filter(|w| w.enabled)
        .map(|w| match graph::sync(w, &queue_path()) {
            Ok(n) => format!("{}: {n} change(s)", w.name),
            Err(e) => format!("{}: {e}", w.name),
        })
        .collect();
    Ok(out.join("\n"))
}

fn workspace_status(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let st = graph::refresh(&ws)?;
    let subjects_of = |only_ai: bool| -> Vec<Value> {
        st["videos"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, v)| !only_ai || v["owner"] == "ai")
            .map(|(k, v)| {
                if only_ai {
                    json!({"id": k, "title": v["title"], "step": v["stage"], "detail": v["detail"]})
                } else {
                    json!({"id": k, "title": v["title"], "step": v["stage"], "owner": v["owner"]})
                }
            })
            .collect()
    };
    let events: Vec<&Value> = st["recent_events"].as_array().into_iter().flatten().rev().take(15).collect();
    Ok(json_text(&json!({
        "waiting_on_ai": subjects_of(true),
        "subjects": subjects_of(false),
        "attached": st["attached"],
        "human_changes_since_ai": st["human_changes_since_ai"],
        "recent_events": events
    })))
}

fn is_meta(a: &Value) -> bool {
    matches!(str_arg(a, "workspace"), Some("meta") | Some("ventosa-meta"))
}

fn graph_nodes(a: &Value) -> R {
    if is_meta(a) {
        return Ok(json_text(&crate::help::meta_graph_nodes()));
    }
    let ws = ws_arg(a)?;
    let g = graph::steps(&ws)?;
    let rows: Vec<Value> = g["steps"].as_array().into_iter().flatten().map(|s| json!({"id": s["id"], "owner": s["owner"], "need": s["need"]})).collect();
    Ok(json_text(&json!({"rule": g["rule"], "steps": rows})))
}

fn graph_node(a: &Value) -> R {
    if is_meta(a) {
        return crate::help::meta_topic(req(a, "node")?);
    }
    let ws = ws_arg(a)?;
    let node = req(a, "node")?;
    let v = graph::node(&ws, node)?;
    Ok(serde_json::to_string_pretty(&v).unwrap_or_default())
}

fn graph_step_set(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let step = req(a, "step")?;
    let field = req(a, "field")?;
    let value = a.get("value").cloned().unwrap_or(Value::Null);
    graph::set_step_field(&ws, step, field, &value)?;
    Ok(format!("{step}.{field} set"))
}

fn graph_step_add(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let id = req(a, "id")?;
    let owner = req(a, "owner")?;
    let need = req(a, "need")?;
    let before = str_arg(a, "before");
    graph::add_step(&ws, id, owner, need, str_arg(a, "skill"), before)?;
    Ok(format!("step {id} added before {}", before.unwrap_or("done")))
}

fn web_research(a: &Value) -> R {
    let subject = str_arg(a, "subject").unwrap_or("");
    let (mut provider, mut env, mut query, mut max) = (str_arg(a, "provider").unwrap_or("").to_string(), String::new(), str_arg(a, "query").unwrap_or("").to_string(), a["max"].as_u64().unwrap_or(0) as usize);
    if let (Some(_), Some(step)) = (str_arg(a, "workspace"), str_arg(a, "step")) {
        let ws = ws_arg(a)?;
        let n = graph::node(&ws, step)?;
        let sr = &n["search"];
        if sr.is_null() {
            return Err(format!("{step} is not a research node"));
        }
        if provider.is_empty() {
            provider = sr["provider"].as_str().unwrap_or("").into();
        }
        env = sr["key_env"].as_str().unwrap_or("").into();
        if query.is_empty() {
            query = sr["query"].as_str().unwrap_or("{title}").into();
        }
        if max == 0 {
            max = sr["max"].as_u64().unwrap_or(8) as usize;
        }
    }
    let query = query.replace("{title}", subject);
    if provider.is_empty() || query.trim().is_empty() {
        return Err("give workspace + step of a research node, or provider + query".into());
    }
    crate::research::search(&provider, &env, query.trim(), if max == 0 { 8 } else { max })
}

fn graph_step_remove(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let step = req(a, "step")?;
    graph::remove_step(&ws, step)?;
    Ok(format!("step {step} removed"))
}

fn skill_install(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let step = req(a, "step")?;
    let name = req(a, "name")?;
    let text = req(a, "text")?;
    graph::install_skill(&ws, step, name, text)?;
    Ok(format!("skill {name} saved and wired to {step}"))
}

fn plugin_text(a: &Value) -> Result<String, String> {
    if let Some(t) = str_arg(a, "json") {
        return Ok(t.to_string());
    }
    let p = req(a, "path")?;
    let path = PathBuf::from(p);
    let len = fs::metadata(&path).map_err(|e| format!("{p}: {e}"))?.len();
    if len as usize > crate::plugin::MAX_BYTES {
        return Err(format!("the file is larger than {} KB", crate::plugin::MAX_BYTES / 1024));
    }
    fs::read_to_string(&path).map_err(|e| format!("{p}: {e}"))
}

fn plugin_list(_a: &Value) -> R {
    let all: Vec<Value> = crate::plugin::list().iter().map(crate::plugin::summary).collect();
    Ok(serde_json::to_string_pretty(&all).unwrap_or_default())
}

fn plugin_validate(a: &Value) -> R {
    let p = crate::plugin::parse(&plugin_text(a)?).map_err(|e| e.join("; "))?;
    let errs = crate::plugin::validate(&p, true);
    if errs.is_empty() {
        Ok(format!("plugin {} {} is valid: {} nodes, {} connections, {} settings, {} tools", p.id, p.version, p.nodes.len(), p.connections.len(), p.settings.fields.len(), p.tools.len()))
    } else {
        Err(format!("would be rejected:\n- {}", errs.join("\n- ")))
    }
}

fn plugin_install(a: &Value) -> R {
    let i = crate::plugin::install(&plugin_text(a)?, "ai", true)?;
    Ok(format!("plugin {} {} installed, disabled: ask the human to enable it in Settings > Plugins", i.plugin.id, i.plugin.version))
}

fn plugin_remove(a: &Value) -> R {
    let id = req(a, "id")?;
    crate::plugin::remove(id, "ai")?;
    Ok(format!("plugin {id} removed"))
}

fn tools_file(ws: &Workspace) -> PathBuf {
    graph::root(ws).join("pipeline").join("tools.json")
}

fn tool_install(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let name = req(a, "name")?;
    if !graph::valid_name(name) {
        return Err(format!("bad tool name {name}"));
    }
    let command = req(a, "command")?;
    let description = str_arg(a, "description").unwrap_or("");
    let path = tools_file(&ws);
    let mut defs: Vec<Value> = load(&path);
    let entry = json!({"name": name, "description": description, "command": command});
    match defs.iter_mut().find(|t| t["name"] == name) {
        Some(t) => *t = entry,
        None => defs.push(entry),
    }
    save_to(&path, &defs)?;
    Ok(format!("tool {name} installed in {}", ws.name))
}

fn truncate(mut s: String) -> String {
    if s.len() > OUTPUT_LIMIT {
        let mut end = OUTPUT_LIMIT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n[truncated]");
    }
    s
}

fn tool_run(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let name = req(a, "name")?;
    let defs: Vec<Value> = load(&tools_file(&ws));
    let tool = defs.iter().find(|t| t["name"] == name).ok_or_else(|| format!("no custom tool {name}; see tool_install"))?;
    let command = tool["command"].as_str().unwrap_or("");
    let root = graph::root(&ws);
    let out = Command::new("sh")
        .args(["-c", command, "sh", str_arg(a, "args").unwrap_or("")])
        .current_dir(&root)
        .env("VENTOSA_WORKSPACE", &root)
        .output()
        .map_err(|e| format!("cannot run {name}: {e}"))?;
    let code = out.status.code().map_or("signal".to_string(), |c| c.to_string());
    Ok(truncate(format!(
        "exit {code}\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )))
}

fn install_client(a: &Value) -> R {
    let mut args = vec!["--client".to_string(), req(a, "client")?.to_string()];
    for (key, flag) in [("scope", "--scope"), ("project", "--project"), ("workspace", "--workspace")] {
        if let Some(v) = str_arg(a, key) {
            args.push(flag.into());
            args.push(v.into());
        }
    }
    if a.get("dry_run").and_then(Value::as_bool).unwrap_or(false) {
        args.push("--dry-run".into());
    }
    let opts = install::parse_args(&args)?.ok_or("install_client takes client and optional scope, project, workspace, dry_run")?;
    Ok(install::run(&opts)?.lines.join("\n"))
}

fn install_info(_: &Value) -> R {
    let file = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("INSTALL.md");
    if let Ok(text) = fs::read_to_string(&file) {
        return Ok(text);
    }
    let opts = install::Opts { print: true, ..Default::default() };
    Ok(install::run(&opts)?.lines.join("\n"))
}

fn media_json(m: &preview::Media) -> Value {
    json!({"subject": m.subject, "kind": m.kind, "path": m.path.display().to_string(), "modified": m.modified, "size": m.size})
}

fn media_json_with(m: &preview::Media, map: &tplog::MediaMap) -> Value {
    let mut v = media_json(m);
    v["teleprompter"] = tplog::match_json(map, &m.path.display().to_string());
    v
}

fn media_list(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let map = tplog::load(&ws);
    let rows: Vec<Value> = match str_arg(a, "subject") {
        Some(s) => preview::media_for(&ws, s),
        None => preview::media(&ws),
    }
    .iter()
    .map(|m| media_json_with(m, &map))
    .collect();
    Ok(json_text(&rows))
}

fn callback_tool(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let cfg = config::load(&config::default_path());
    let args = callback::StartArgs {
        subject: req(a, "subject")?,
        node: req(a, "node")?,
        mode: str_arg(a, "mode").unwrap_or("fast"),
        reason: req(a, "reason")?,
        caller: str_arg(a, "caller"),
        dry_run: a.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
    };
    let cb = callback::start_ws(&ws, &args, &cfg.limits)?;
    Ok(json_text(&cb))
}

fn stale_tool(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let _ = graph::refresh(&ws);
    let rows = callback::stale_ws(&ws, str_arg(a, "subject"))?;
    let open: Vec<callback::Callback> = callback::load_callbacks(&graph::root(&ws)).into_iter().filter(|c| c.live()).collect();
    Ok(json_text(&json!({"stale": rows.iter().filter(|r| r.stale).collect::<Vec<_>>(), "fresh": rows.iter().filter(|r| !r.stale).map(|r| format!("{} {}", r.subject, r.node)).collect::<Vec<_>>(), "open_callbacks": open})))
}

fn callback_stop(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let id = a.get("id").and_then(Value::as_u64).ok_or("id is required")?;
    Ok(json_text(&callback::stop(&graph::root(&ws), id, crate::now())?))
}

fn teleprompter_log(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let map = if a.get("refresh").and_then(Value::as_bool).unwrap_or(true) { tplog::refresh(&ws) } else { tplog::load(&ws) };
    let video = str_arg(a, "video");
    let takes: Vec<&tplog::Take> = map.takes.iter().filter(|t| video.is_none_or(|v| t.hits.iter().any(|h| h.video == v))).collect();
    let matches: Vec<&tplog::Match> = map.matches.iter().filter(|m| video.is_none_or(|v| m.takes.iter().any(|r| r.segments.iter().any(|h| h.video == v)) || m.method == "unmatched")).collect();
    Ok(json_text(&json!({"clock_offset_s": map.offset_s, "takes": takes, "files": matches})))
}

/// A path argument resolved inside the workspace root. Refuses anything outside it.
fn inside_root(root: &Path, p: &str) -> Result<PathBuf, String> {
    let raw = Path::new(p);
    let joined = if raw.is_absolute() { raw.to_path_buf() } else { root.join(raw) };
    let real = fs::canonicalize(&joined).map_err(|e| format!("{p}: {e}"))?;
    if !real.starts_with(root) {
        return Err(format!("{p} is outside the workspace root"));
    }
    Ok(real)
}

fn media_play(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let root = fs::canonicalize(graph::root(&ws)).map_err(|e| format!("workspace root: {e}"))?;
    let (path, title) = match str_arg(a, "path") {
        Some(p) => {
            let real = inside_root(&root, p)?;
            let title = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (real, title)
        }
        None => {
            let subject = req(a, "subject")?;
            let list = preview::media_for(&ws, subject);
            let m = list.iter().find(|m| m.kind == "edit").or(list.first()).ok_or_else(|| format!("no media for {subject}"))?;
            (inside_root(&root, &m.path.display().to_string())?, subject.to_string())
        }
    };
    let start = a.get("start").and_then(Value::as_f64);
    let player = preview::play(&path, &title, start)?;
    Ok(format!("playing {} in {player}", path.display()))
}

fn app_json(ws: &Workspace, d: &apps::AppDef) -> Value {
    json!({"id": d.id, "title": d.title, "node": d.node, "effect": d.effect, "next": d.next, "url": apps::url(d), "pid": apps::running(ws, &d.id)})
}

fn app_list(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let rows: Vec<Value> = apps::list(&ws).iter().map(|d| app_json(&ws, d)).collect();
    Ok(json_text(&rows))
}

fn app_create(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let def = apps::create(
        &ws,
        req(a, "id")?,
        str_arg(a, "title").unwrap_or(""),
        a.get("node").and_then(Value::as_str).unwrap_or(""),
        req(a, "effect")?,
        str_arg(a, "next").unwrap_or(""),
        str_arg(a, "html").unwrap_or(""),
    )?;
    Ok(json_text(&app_json(&ws, &def)))
}

fn app_write_file(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let (id, path) = (req(a, "id")?, req(a, "path")?);
    let text = a.get("text").and_then(Value::as_str).ok_or("text is required")?;
    apps::update_file(&ws, id, path, text)?;
    Ok(format!("wrote {path} in app {id}"))
}

fn app_deploy(a: &Value) -> R {
    let ws = ws_arg(a)?;
    apps::deploy(&ws, req(a, "id")?)
}

fn app_stop(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let id = req(a, "id")?;
    Ok(if apps::stop(&ws, id)? { format!("app {id} stopped") } else { format!("app {id} was not running") })
}

fn graph_steps(a: &Value) -> R {
    let ws = ws_arg(a)?;
    Ok(serde_json::to_string_pretty(&graph::steps(&ws)?).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_schema_parses_and_names_are_unique() {
        assert_eq!(list().len(), TOOLS.len());
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
    }

    #[test]
    fn capabilities_follow_the_brief() {
        assert_eq!(cap("list_tasks"), Cap::Read);
        assert_eq!(cap("graph_node"), Cap::Read);
        assert_eq!(cap("add_task"), Cap::Write);
        assert_eq!(cap("sync_now"), Cap::Write);
        assert_eq!(cap("graph_step_set"), Cap::Graph);
        assert_eq!(cap("skill_install"), Cap::Graph);
        assert_eq!(cap("tool_install"), Cap::Install);
        assert_eq!(cap("tool_run"), Cap::Run);
        assert_eq!(cap("nope"), Cap::Run);
        assert_eq!(cap("graph_steps"), Cap::Read);
        assert_eq!(cap("install_client"), Cap::Install);
        assert_eq!(cap("install_info"), Cap::Read);
        assert_eq!(cap("media_list"), Cap::Read);
        assert_eq!(cap("media_play"), Cap::Run);
        assert_eq!(cap("app_list"), Cap::Read);
        for name in ["app_create", "app_write_file", "app_deploy", "app_stop"] {
            assert_eq!(cap(name), Cap::Apps, "{name}");
        }
    }

    #[test]
    fn every_row_has_a_description_and_an_object_schema() {
        for t in TOOLS {
            assert!(!t.description.trim().is_empty(), "{} has no description", t.name);
            assert!(t.name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{} must be snake_case", t.name);
            let schema: Value = serde_json::from_str(t.schema).unwrap_or_else(|e| panic!("{}: {e}", t.name));
            assert_eq!(schema["type"], "object", "{}", t.name);
            if let Some(req) = schema.get("required").and_then(Value::as_array) {
                for r in req {
                    let key = r.as_str().unwrap_or("");
                    assert!(schema["properties"].get(key).is_some(), "{}: required {key} is not a property", t.name);
                }
            }
        }
    }

    #[test]
    fn new_tools_are_listed_for_mcp() {
        let names: Vec<String> = list().iter().map(|t| t["name"].as_str().unwrap_or("").to_string()).collect();
        for n in ["install_client", "install_info", "media_list", "teleprompter_log", "callback", "stale", "media_play", "app_list", "app_create", "app_write_file", "app_deploy", "app_stop", "graph_steps"] {
            assert!(names.iter().any(|x| x == n), "{n} missing");
        }
    }

    #[test]
    fn media_play_refuses_paths_outside_the_root() {
        let root = std::env::temp_dir().join(format!("ventosa-tools-root-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("ventosa-tools-outside-{}.mp4", std::process::id()));
        let _ = fs::create_dir_all(&root);
        fs::write(&outside, b"x").unwrap();
        let real_root = fs::canonicalize(&root).unwrap();
        assert!(inside_root(&real_root, &outside.display().to_string()).is_err());
        assert!(inside_root(&real_root, "../../../etc/passwd").is_err());
        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_tool_is_an_error_result() {
        let r = call("nope", &json!({}));
        assert_eq!(r["isError"], true);
    }
}

// ---------------------------------------------------------------- pipeline, decisions, help, batches, servers, files

fn pipeline_attach(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let (step, subject) = (req(a, "step")?, req(a, "subject")?);
    let agent = str_arg(a, "agent").unwrap_or("ventosa-ai");
    let out = graph::engine_cmd(&ws, "skill").args(["attach", step, subject]).env("VENTOSA_AGENT", agent).output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() { Ok(text) } else { Err(text) }
}

fn pipeline_write(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let args = graph::list_arg(a.get("args").unwrap_or(&Value::Null))?;
    if args.is_empty() {
        return Err("args is required".into());
    }
    let agent = str_arg(a, "agent").unwrap_or("ventosa-ai");
    let out = graph::engine_cmd(&ws, "ai_write").args(&args).env("VENTOSA_AGENT", agent).output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() {
        let _ = graph::sync(&ws, &queue_path());
        Ok(if text.is_empty() { "written".into() } else { text })
    } else {
        Err(text)
    }
}

fn decision_ask(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let question = req(a, "question")?;
    let options = graph::list_arg(a.get("options").unwrap_or(&Value::Null))?;
    if options.is_empty() {
        return Err("give at least one option".into());
    }
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    let subject = str_arg(a, "subject").unwrap_or("");
    let title = if subject.is_empty() { format!("decide · {question}") } else { format!("{subject} · decide · {question}") };
    let payload = json!({
        "workspace": ws.name, "subject": subject,
        "decision": {"question": question, "options": options, "context": str_arg(a, "context").unwrap_or(""), "risk": str_arg(a, "risk").unwrap_or("low")},
        "answer": Value::Null,
    });
    let item = queue::enqueue(&mut items, &title, "todo", "decision", str_arg(a, "step").map(String::from), payload);
    save_to(&path, &items)?;
    Ok(format!("decision {} is in the human's to-do; read the answer later with decision_list", item.id))
}

fn decision_list(a: &Value) -> R {
    let items: Vec<Item> = load(&queue_path());
    let status = str_arg(a, "status").unwrap_or("all");
    let rows: Vec<Value> = items
        .iter()
        .filter(|i| i.source == "decision")
        .filter(|i| str_arg(a, "workspace").is_none_or(|w| i.payload["workspace"] == w))
        .filter(|i| str_arg(a, "subject").is_none_or(|s| i.payload["subject"] == s))
        .filter(|i| match status {
            "open" => i.payload["answer"].is_null(),
            "answered" => !i.payload["answer"].is_null(),
            _ => true,
        })
        .map(|i| json!({"id": i.id, "step": i.node, "subject": i.payload["subject"], "question": i.payload["decision"]["question"], "options": i.payload["decision"]["options"], "answer": i.payload["answer"], "status": i.status}))
        .collect();
    Ok(json_text(&rows))
}

/// Record an answer and close the decision (also used by the to-do page).
pub fn answer_decision(id: u64, answer: &str) -> R {
    let path = queue_path();
    let mut items: Vec<Item> = load(&path);
    let i = items.iter_mut().find(|i| i.id == id && i.source == "decision").ok_or_else(|| format!("no decision {id}"))?;
    i.payload["answer"] = json!(answer.trim());
    i.payload["answered_at"] = json!(crate::now());
    i.status = "done".into();
    i.updated = crate::now();
    save_to(&path, &items)?;
    Ok(format!("decision {id}: {}", answer.trim()))
}

fn decision_answer(a: &Value) -> R {
    answer_decision(id_arg(a)?, req(a, "answer")?)
}

fn graph_elevate_set(a: &Value) -> R {
    let ws = ws_arg(a)?;
    graph::set_elevate(&ws, req(a, "text")?)?;
    Ok("elevation rules saved".into())
}

fn trusted_list(a: &Value) -> R {
    let ws = ws_arg(a)?;
    Ok(crate::trusted::prompt_block(&ws))
}

fn trusted_add(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let strs = |k: &str| a[k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>();
    let s = crate::trusted::Source {
        id: str_arg(a, "id").unwrap_or("").into(),
        kind: str_arg(a, "kind").unwrap_or("").into(),
        name: str_arg(a, "name").unwrap_or("").into(),
        refs: strs("refs"),
        locked: strs("locked"),
        recipe: str_arg(a, "recipe").unwrap_or("").into(),
        notes: str_arg(a, "notes").unwrap_or("").into(),
        added_by: str_arg(a, "agent").unwrap_or("ai").into(),
        ..Default::default()
    };
    let id = crate::trusted::propose(&ws, s)?;
    Ok(format!("proposed {id} (not binding until approved); raise decision_ask so the human approves it"))
}

fn trusted_used(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let ids: Vec<String> = a["ids"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    let scene = str_arg(a, "scene").ok_or("scene is required")?;
    crate::trusted::mark_used(&ws, &ids, scene)?;
    Ok(format!("{scene}: {} source(s) recorded", ids.len()))
}

fn trusted_approve(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let id = str_arg(a, "id").ok_or("id is required")?;
    let on = a["approved"].as_bool().unwrap_or(true);
    crate::trusted::set_approved(&ws, id, on)?;
    Ok(format!("{id} {}", if on { "approved" } else { "back to proposed" }))
}

fn help(a: &Value) -> R {
    let cfg = config::load(&config::default_path());
    if let Some(t) = str_arg(a, "topic") {
        return crate::help::meta_topic(t);
    }
    let mut out = format!("{}\n\nMeta graph (soul first):\n{}", crate::help::INTERNAL_PROMPT, crate::help::meta_overview());
    if str_arg(a, "workspace").is_some() {
        let ws = ws_arg(a)?;
        out.push_str("\n\nNow:\n");
        out.push_str(&crate::help::explain(&cfg, &ws, str_arg(a, "step").unwrap_or(""))?);
    }
    Ok(out)
}

fn batch_start(a: &Value) -> R {
    let cfg = config::load(&config::default_path());
    let b = crate::batch::Batch {
        workspace: req(a, "workspace")?.into(),
        kind: req(a, "kind")?.into(),
        node: str_arg(a, "node").unwrap_or("").into(),
        count: a.get("count").and_then(Value::as_u64).unwrap_or(1) as usize,
        brief: str_arg(a, "brief").unwrap_or("").into(),
        runner: str_arg(a, "runner").unwrap_or("").into(),
        command: str_arg(a, "command").unwrap_or("").into(),
        ..Default::default()
    };
    let b = crate::batch::create(&cfg, b)?;
    Ok(format!("batch {} started; the Ventosa app (or ventosa-daemon) runs it. {}", b.id, json_text(&crate::batch::summary(&b))))
}

fn batch_list(a: &Value) -> R {
    let rows: Vec<Value> = crate::batch::list().iter().filter(|b| str_arg(a, "workspace").is_none_or(|w| b.workspace == w)).map(crate::batch::summary).collect();
    Ok(json_text(&rows))
}

fn batch_stop(a: &Value) -> R {
    let id = id_arg(a)?;
    crate::batch::stop(id)?;
    Ok(format!("batch {id} stopped"))
}

fn servers_list(_: &Value) -> R {
    let cfg = config::load(&config::default_path());
    let rows: Vec<Value> = cfg.servers.iter().map(|s| json!({"name": s.name, "host": s.host, "public_url": s.public_url, "remote_dir": s.remote_dir, "enabled": s.enabled})).collect();
    Ok(json_text(&rows))
}

fn server_of(a: &Value) -> Result<config::Server, String> {
    let cfg = config::load(&config::default_path());
    let name = req(a, "server")?;
    cfg.server(name).cloned().ok_or_else(|| format!("no server {name}; add it in settings ▸ servers"))
}

fn app_publish(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let url = crate::deploy::publish(&ws, &server_of(a)?, req(a, "id")?, str_arg(a, "mode").unwrap_or("static"))?;
    Ok(format!("published: {url}"))
}

fn app_pull(a: &Value) -> R {
    let ws = ws_arg(a)?;
    crate::deploy::pull(&ws, &server_of(a)?, req(a, "id")?)
}

fn tools_recommended(_: &Value) -> R {
    let cfg = config::load(&config::default_path());
    Ok(json_text(&crate::recommend::to_json(&cfg)))
}

/// A path inside the workspace root (no `..`, no absolute paths).
fn inside(ws: &Workspace, rel: &str) -> Result<PathBuf, String> {
    if Path::new(rel).components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
        return Err(format!("{rel}: must be a relative path inside the workspace"));
    }
    Ok(graph::root(ws).join(rel))
}

/// Files an AI read never returns: keys, credentials, identities, the app's own state.
pub fn secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with(".env") || n == "identity.json" || n == "audit.jsonl" || n == "id_rsa" || n == "id_ed25519"
        || n.contains("secret") || n.contains("credential") || n.contains("password") || n.contains("token")
        || [".pem", ".key", ".p12", ".pfx", ".kdbx"].iter().any(|e| n.ends_with(e))
}

/// A path inside the workspace after symlinks resolve, never a secret file or one in a hidden folder.
fn readable(ws: &Workspace, rel: &str) -> Result<PathBuf, String> {
    let p = inside(ws, rel)?;
    let root = fs::canonicalize(graph::root(ws)).map_err(|e| format!("workspace root: {e}"))?;
    let real = fs::canonicalize(&p).map_err(|e| format!("{rel}: {e}"))?;
    if !real.starts_with(&root) {
        return Err(format!("{rel} leaves the workspace (a link?)"));
    }
    let hidden = real.strip_prefix(&root).unwrap_or(&real).components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    if hidden || real.file_name().is_some_and(|n| secret_name(&n.to_string_lossy())) {
        return Err(format!("{rel} is not readable by the AI (hidden, key or credential file)"));
    }
    Ok(real)
}

fn files_list(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let dir = readable(&ws, str_arg(a, "path").unwrap_or("."))?;
    let mut rows: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.') && !secret_name(&e.file_name().to_string_lossy()))
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir() { format!("{name}/") } else { format!("{name} ({} B)", e.metadata().map(|m| m.len()).unwrap_or(0)) }
        })
        .collect();
    rows.sort();
    Ok(rows.join("\n"))
}

fn file_read(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let p = readable(&ws, req(a, "path")?)?;
    let bytes = fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let cut = &bytes[..bytes.len().min(64 * 1024)];
    Ok(String::from_utf8_lossy(cut).to_string())
}

fn visualizer_render(a: &Value) -> R {
    let ws = ws_arg(a)?;
    let audio = inside(&ws, req(a, "audio")?)?;
    let stem = audio.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "song".into());
    let out = match str_arg(a, "out") {
        Some(o) => inside(&ws, o)?,
        None => graph::root(&ws).join("assets/renders").join(format!("{stem}-visualizer.mp4")),
    };
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut c = Command::new("python3");
    c.arg(graph::runtime_dir().join("engine/visualizer.py")).arg("--audio").arg(&audio).arg("--out").arg(&out);
    if let Some(m) = str_arg(a, "midi") {
        c.arg("--midi").arg(inside(&ws, m)?);
    }
    for (k, flag) in [("title", "--title"), ("artist", "--artist"), ("palette", "--palette"), ("size", "--size")] {
        if let Some(v) = str_arg(a, k) {
            c.args([flag, v]);
        }
    }
    for l in graph::list_arg(a.get("lines").unwrap_or(&Value::Null))? {
        c.args(["--line", &l]);
    }
    if let Some(s) = a.get("seconds").and_then(Value::as_f64).filter(|s| *s > 0.0) {
        c.args(["--seconds", &s.to_string()]);
    }
    let o = c.current_dir(graph::root(&ws)).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(format!("rendered {}", out.display()))
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().chars().rev().take(1500).collect::<String>().chars().rev().collect())
    }
}
