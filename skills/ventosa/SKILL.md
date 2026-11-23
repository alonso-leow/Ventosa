---
name: ventosa
description: Drive a Ventosa content pipeline through its MCP tools. Use to advance a Ventosa workspace (skill-graph steps), take the next item from the AI queue, turn a title into thesis or script options (phase 1), or edit a workspace's graph. Needs the ventosa-mcp server.
---

# Ventosa

Ventosa is a content manager. Each workspace is a skill graph: step nodes, each with its own skill. The MCP tools below are the same ones the built-in Ventosa chat uses.

Default workspace: `{{WORKSPACE}}`. Other workspaces: `workspaces_list`.

## Protocol

0. `help` (no arguments): read the soul and the index before acting on a new request.

1. `workspaces_list` gives the workspaces and their roots. `workspace_status` (with the workspace) shows what waits on AI steps.
2. `queue_claim` with `node` (for example `options`) and `agent` set to your name. It returns the oldest queued item and marks it running. `null` means nothing waits. Work only items you claimed.
3. `graph_node` (workspace and node) returns the node's skill text, the knowledge files it names, and its write command. Read all of it before working.
4. Attach: `VENTOSA_AGENT=<your name> ventosa-graph -w <root> attach <node> <ID>`. Its output is the skill and the only write command. Attaching is the second lock.
5. Do the node's work. Write only with the node's write command, for example `ventosa-graph -w <root> write option <ID> '...'`. Never edit `queue.json`, `tasks.json` or a workspace's state files by hand.
6. `queue_set_status` with `done`. Use `review` when a human must choose next, and `failed` on error.

A workspace with its own engine (for example `metalvj-media`) may use its own `pipeline/skill.py` and `ai_write.py`. Follow that node's skill.

## Phase 1: title to options

`pipeline_intake` with `title` and `workspace` registers the subject (for example `E02`) and queues an AI item on node `options`. Then follow the protocol above at node `options`.

## Apps for the human

For a step that needs a person (a review, a checklist), build a small phone app wired to it: `app_create` (workspace, id, title, node, effect, next, optional html), then `app_write_file` for extra files and `app_deploy` to start it and get the URL. Effect `human-done` marks the node done when the human finishes; `step-done` is for an AI step you have attached to. The page calls `ventosa.done(subject, note)` from `/common/ventosa-app.js`. `app_list` shows what exists and whether it runs.

## Video

`media_list` (workspace, optional subject) lists edits, takes and renders. `media_play` opens one in a player: a `path` inside the workspace, or a `subject` (its newest edit, else its newest media). Use it to show the human a cut.

## Callbacks (going back a step)

When something already done broke after a change (subtitles after a re-cut): `stale` (workspace, subject) lists nodes whose input files changed since they ran. Go back to the latest node whose own inputs changed: `callback` (workspace, subject, node, reason, mode). `fast` (default) re-runs that node, then each next node only if its inputs changed. `full` re-runs everything downstream, in order. Use `dry_run` first to show the plan. Inside a run, pass `caller` = your node. Human nodes are reopened as to-dos; a published or uploaded subject needs `decision_ask` first. A refusal (depth, cycle, repeat within the hour) goes to `decision_ask`, never a retry. `callback_stop` ends a walk.

## Link media

The `link_media` node (after `record`): `teleprompter_log` and `media_list` show which text line each camera file holds (time overlap, confidence). Report unmatched files and clock skew; Ventosa also refreshes the mapping every 10 minutes.

## Editing a graph

Use `graph_nodes` before and after. Changes go through `ventosa-graph -w <root> new|edit|index` and the `graph_step_add|set|remove` tools.

### The meta graph (read it first)
`help` returns the soul (why Ventosa exists, what to decide and what to ask) and an index of the ring: principles, types, views, tools, recipes, patterns, testing, failures, policies. `help` with `topic=<id>` reads one node and its edges (informs, informed by); `graph_nodes` and `graph_node` with workspace `meta` read the same graph. Follow edges outward from `soul`. Start there on a new request; for "make N videos about X" read `recipe_bulk`, for a broken later node `recipe_callback`, for a decision `decide_vs_ask`.

## Rules

- Claim before working. Never work an item you did not claim.
- Human steps (label, record, publish, upload) cannot be attached. Set `review` and tell the user which step is next.
- Report what you wrote and the final queue status.
