# Callback: send the pipeline back to an earlier step

Status: implemented (phases 0-3, parts of 4) in `src/callback.rs`, tools `callback`, `stale`, `callback_stop`, UI in the node panel and canvas. Where the implementation differs from the first design, a **Implemented:** note says so; the rest below is the original design.

## 0. Implemented: summary of differences

- **Where**: all in Rust (`src/callback.rs`), engine-agnostic. Hashes are recorded by the workspace sync (`graph::sync` -> `callback::on_state`), not by the Python write gate: a node counts as run once the subject's stage is past it. `pipeline_write` already syncs, so this happens right after a node finishes.
- **Staleness is hash-only**: a node is stale when the hashes of the per-subject files it `reads` differ from those recorded when it finished (`pipeline/runs.json`). The "an upstream node ran after it" rule was dropped: with a fallback edge set it produced false positives. A reopened-and-finished node is re-recorded (count of `step.reopen` events). Folders are hashed through entries starting with the subject id when there are any; files over 8 MiB by size + head + tail. A shared file such as `recording/clips.json` stales every subject when any changes (conservative).
- **Edges**: `reads`/`writes` file ids; with none, every later step counts as downstream (so fast mode only checks hashes and full mode is the safe choice for such graphs).
- **Tools**: `callback(workspace, subject, node, reason, mode=fast, caller?, dry_run?)`, `stale(workspace, subject?)` (stale rows, fresh nodes, open callbacks), `callback_stop(workspace, id)`.
- **Reopen** uses the engine's own `ai_write reopen` (`step.reopen` event), so the engine's queue/state rules apply. After the reopen the state is refreshed; if the subject is not at or before the node the callback is marked `failed` ("the engine did not reopen"). Flows whose stage is purely file-based (the media flow's `edit`) may not honour a reopen; the walk then fails visibly instead of looping.
- **Walk**: `pipeline/callbacks.json`; on each sync `advance` finishes the open node (stage moved past it), records its run, then reopens the next planned node (full: always; fast: only when its inputs hash changed, else `skip-valid` and its hashes are refreshed). A failed queue item fails the walk. Human nodes are reopened (to-do) in full mode, and in fast mode when their inputs changed.
- **Loop guard**: `limits.callback_depth` (3) and `limits.callback_per_hour` (2, same subject+node) in settings limits; cycle check against the pending nodes of ancestors; one open callback per subject (a child is allowed only when `caller` equals the parent's current node and suspends the parent, `waiting`). Refusals are stored with status `refused` and tell the AI to use `decision_ask` (not created automatically).
- **UI**: node panel card "callback" (stale reasons, running walks with Stop, subject + fast/full + Plan/Run); stale nodes get an orange outline and `stale` label on the canvas (hash check every 15 s). Not built: highlighting the downstream edges of the selection, the banner on the subject, the to-do reason text, `batch` kind `heal`, `stale_accept`.
- **Skill**: "Callbacks" section in `skills/ventosa/SKILL.md`.


## 1. Problem

A subject (V03) is already past `subtitles`, and something upstream changed (the edit was re-cut, the script was fixed). The subtitles no longer fit. Today the human or AI has to know that, hand-reopen the step, and re-run every later step by hand. The pipeline must be able to return to the right node and carry on forward, by itself, through tool calls.

## 2. What exists today (study)

- **Graph**: `pipeline/skill-graph/steps.json` holds steps in order, each with `owner` (ai|human), `reads`, `writes` (knowledge/file ids such as `F-edits`), `command` (the write command), `done_when`, optional `per`. `after` exists on steps (`graph_step_remove` also strips it), but ordering is mostly the array order. A node lives in `src/graph.rs` (`node`, `steps`, `set_step_field`).
- **State**: the engine's `status.py` recomputes `pipeline.json` per subject (`stage`, `owner`, `detail`). Two flavours:
  - media flow: stage derived from files and events (`edit_signed`, `subs_signed`, "a newer take was kept after the edit" -> edit reopens). Staleness is already file/mtime based here, but only for a few hard-coded pairs.
  - `flow: generic`: stage = first step in `ORDER` without a `step.done`; `step.reopen` events remove a step from the done set (`generic_stage`, `step_done`).
- **Events**: `pipeline/events.jsonl` (`append_event`) is the log; `graph::event_step` maps events to steps.
- **Queue**: `graph::sync_state` keeps one live queue item per subject at its current stage (`ai` kind for AI steps, `todo` for human). When the stage moves, the old item becomes `done`. So the queue follows the state; it does not drive it.
- **MCP tools**: `graph_nodes/node/steps` (read), `pipeline_attach` + `pipeline_write` (the only way an AI finishes a step, through the engine's write gate), `queue_claim/list/set_status`, `batch_start` (kind start / drain, limits: concurrency, rpm, retries, backoff), `decision_ask` (escalate to human), `workspace_status`.
- **Skill** (`skills/ventosa/SKILL.md`): claim -> `graph_node` -> attach -> write -> `queue_set_status`. Steps `ventosa-subtitles` says "a new edit reopens the step", but only the engine's hand-written rule does that.

Gap: there is a reopen event but no tool for it, no notion of "what is downstream of this node and depends on its output", and no executor that walks back down the graph.

## 3. Model

### 3.1 Dependency graph

Derive edges from data, not from array order:

`A -> B` when `B.reads` contains a file id that is in `A.writes` (knowledge ids `K-*` are static, ignore them). Array order breaks ties and gives the forward order for the full walk. Example (metalvj-media): `edit -writes F-edits-> publish_meta (reads F-edits)`; `subtitles` reads `F-edits` and writes `F-subtitles`. A graph without usable `reads/writes` falls back to "everything after in array order" (conservative = more nodes re-run, never fewer).

`downstream(n)` = transitive closure over those edges, returned in topological order. `human` nodes in that set are not re-run; they are flagged (section 4.3).

### 3.2 Staleness via input hashes

Record per (subject, node) when a node finishes:

```
pipeline/runs.json  { "V03": { "subtitles": { "at": ..., "run": 17,
    "inputs":  { "F-edits": "sha256:...", "F-scripts": "sha256:..." },
    "outputs": { "F-subtitles": "sha256:..." } } } }
```

- `inputs` hashes cover the files the node declared in `reads` that are per-subject files (hash of path+content, or size+mtime+first/last 64 KiB for big media, with the true content hash computed lazily and cached).
- Written by the engine's write gate (`ai_write` / `step-done` / `human-done`) at completion; it already sees the node and subject.
- A node is **stale** when its recorded input hash differs from the current hash, **or** when any upstream node is stale or was re-run after this node's `at` (transitive, so a stale `edit` makes `subtitles` stale even before the new edit file exists).
- Text edits to the node's own skill/`done_when` also stale it (`skill` hash is an input).
- This replaces the hard-coded `subs_signed` / `edit_signed` checks with one generic rule; keep them as extra stale reasons until migrated.

`affected(n)` = `n` plus every node in `downstream(n)` that is stale or would become stale.

## 4. Tool signature

New MCP tool (Cap::Write, because it requeues work), plus a read-only companion.

```
callback(
  workspace: string,
  subject:   string,          // V03
  node:      string,          // where to go back to
  mode:      "full" | "fast", // default "fast"
  reason:    string,          // required, free text, logged and shown to the human
  caller?:   string,          // node/agent that asked (for loop guard); engine fills it from the attach
  dry_run?:  bool,            // return the plan only
) -> { callback_id, plan: [ { node, action: "rerun"|"skip-valid"|"human-gate", why } ],
       depth, queued_item }

stale(workspace, subject?) -> [ { subject, node, stale: bool, reasons: [..], downstream: [..] } ]   // Cap::Read
```

### 4.1 Full mode

The caller names the start node. Reopen it and **every** node in `downstream(node)` (topological order), regardless of hashes. They run strictly one after another: node k+1 is queued only when node k reaches `done`. Use when the cause is unclear, or hashes are not trustworthy (files edited outside the app).

### 4.2 Fast mode

Re-run `node` only. When it finishes, look at the next node in `downstream(node)`: if its inputs hash is unchanged (the re-run produced identical output, or its inputs never changed) it is `skip-valid` and the walk moves on; if its inputs changed it re-runs, and so on to the end. Nodes that are already valid are never repeated. Example: subtitles broke only because a cue style changed -> re-run `subtitles`, `publish_meta` reads `F-edits` not `F-subtitles` so it is skipped, `done`.

If the start node's output hash is identical to before, the walk stops there: nothing downstream was affected.

### 4.3 Human nodes in the path

`label`, `record`, `publish`, `upload` are never auto-run. In full mode they become a `todo` queue item at that point and the walk pauses (same as a `batch` pausing on a human step); it resumes when the human finishes. In fast mode a human node is reopened only if its inputs hash changed; a node already `upload`ed is never reopened without a `decision_ask` (matches the graph's elevate rule: do not overwrite or republish without the human).

## 5. Execution: "recurse via tool calling"

The state machine lives in the engine, not in the agent's head, so a crash or context loss cannot lose the walk.

1. `callback` writes `step.reopen` events (existing mechanism) for the nodes it reopens, appends a `callback` record `{id, subject, start, mode, plan, depth, parent, status}` to `pipeline/callbacks.jsonl`, and calls `graph::sync` so the queue item for the first node appears (existing `sync_state`).
2. An agent (or `batch` drain) claims it exactly as today: `queue_claim` -> `graph_node` -> attach -> work -> `pipeline_write`. The item payload gains `callback: {id, reason, mode, plan}` and the `graph_node` result shows it, so the node's skill is told it is a re-run and why (for subtitles: "the edit changed; regenerate from the new cut").
3. When `pipeline_write` completes a node that belongs to an open callback, the gate itself computes the next planned node (fast: re-check hashes; full: next in list) and reopens it. That is the recursion: each completed node calls back into the engine, which hands the next one to the queue. No agent needs to remember the plan.
4. A node may itself call `callback` (e.g. the edit step finds the script is wrong and sends the pipeline back to `script`). That creates a child callback (`parent` = current id, `depth + 1`). When the child finishes, the parent plan continues from where it was; steps the child already re-ran are `skip-valid` by hash.
5. `batch` integration: a batch run executes callback items like any AI item (same limits). New batch kind `heal` is optional: `batch_start kind=heal` runs `stale` for every subject and queues fast callbacks for the stale ones.

### 5.1 Loop guard

- `max_depth` (default 3): chain of callbacks started from inside a callback. Exceeded -> refuse and `decision_ask`.
- `max_per_node` (default 2 per subject per hour): the same (subject, node) callback again within the window is refused with the previous run's `reason`s, to catch a node that keeps invalidating itself.
- Cycle check: a callback whose start node is already in the open plan of an ancestor (`parent` chain) is a cycle: refused.
- Progress rule: if the re-run's output hash equals the previous output hash, the fast walk ends (no change = no downstream work).
- Settings: new keys in `Limits` (`callback_depth`, `callback_per_hour`), editable in settings.
- Every refusal becomes a `decision_ask` so the human sees it in the to-do instead of the pipeline silently stopping.

## 6. How the AI picks the target node

The AI should not guess from node names. Give it evidence:

1. `stale(workspace, subject)` returns for each node the reasons ("input F-edits changed 12:04 > subtitles run 11:20"). The earliest stale node whose own inputs changed (not just inherited staleness) is the **root cause**; that is the default target.
2. For a reported symptom ("subtitles drifted"), the AI starts at the node that owns the symptom (`graph_node` `writes`/`done_when` tell which node makes subtitles) and walks `reads` upstream until it finds a node whose output hash changed since the symptom node last ran. Rule of thumb in the skill: go back to the **latest node whose inputs changed**, not the earliest; use `full` only when `stale` shows nothing (hashes unavailable) or two roots.
3. Dry run first (`dry_run: true`) and show the plan; anything touching a human node or an uploaded subject goes through `decision_ask`.
4. Add a short "Callbacks" section to `skills/ventosa/SKILL.md` with steps 1-3, and a `callback` hint line to each node's skill ("if your input looks wrong, call `callback` to the node that makes it, do not patch it here").

## 7. UI

- Overview canvas: a stale node gets an orange outline and a small "stale" label with the reason on hover; the edges of the downstream closure are highlighted when a node is selected. Selected node panel: "re-run from here" with a Fast / Full switch, shows the plan (dry run) with each node marked rerun / skip-valid / needs human, then a confirm button.
- A running callback shows as a thin banner on the subject: `callback #17 fast: subtitles -> publish_meta (2/3)`, with Stop (marks the callback `stopped`, queue items `failed`).
- To-do: the pause at a human node shows the reason ("edit changed, re-check the label").
- Activity feed: `callback.start/step/skip/end/refused` events.
- Chat: `@node` mention gains "send back here" as an action.

## 8. Failure and safety

- A failed re-run (`queue_set_status failed`, after retries) leaves the callback `failed`; downstream nodes stay stale (hash mismatch), nothing is marked done falsely.
- The engine never deletes outputs on reopen; the old file stays until the node's write replaces it (so a failed re-run can be rolled back by the human).
- Permissions: `callback` is Write; `dry_run` and `stale` are Read. Elevate rules apply to published/uploaded subjects.
- Locking: one open callback per (subject); a second request while one is open is merged (extends the plan) or refused.

## 9. Phased plan

**Phase 0 (small, ~1 day): reopen as a tool.** `callback` with `mode: full` only, no hashes: expand `downstream` from `reads/writes` (fallback array order), write `step.reopen` events for those nodes, `graph::sync`. Plus `dry_run`. Tests: closure on the metalvj-media graph, reopen -> queue item at the start node, human node pause. Skill text.

**Phase 1: hashes + `stale` + fast mode.** `runs.json` written by the write gate (Python engine: `ai_write.py`, `skill.py`; generic and media flow). `stale` tool, fast walk with `skip-valid`, replace hard-coded `subs_signed/edit_signed` with the generic rule. Tests: output-unchanged stops the walk; changed input stales only the right nodes.

**Phase 2: execution loop.** `callbacks.jsonl`, plan advance in the write gate, payload shown in `graph_node`, loop guard (depth, per-hour, cycle), `decision_ask` on refusal, `batch` kind `heal`. Tests: child callbacks, cycle refused, failed re-run keeps downstream stale.

**Phase 3: UI.** Stale outlines, plan preview and Fast/Full switch, running-callback banner, to-do reason, activity events.

**Phase 4: AI guidance.** Skill "Callbacks" section, per-node hint lines, cheap eval: replay "subtitles broke after a re-cut" and check the AI picks `subtitles` (fast) and not `edit` (full).

## 10. Open questions

- Hashing big media: content hash is slow; accept size+mtime+head/tail sample by default, full hash on demand.
- Nodes with `per` (per-reel, per-language outputs): hash key must include the sub-item; assume one run record per (subject, node, per-key).
- External engines (`ws.engine == "local"`, e.g. metalvj-media's own `pipeline/*.py`) need the same write-gate change; until then `callback` can only do Phase 0 behaviour for them.
- Should a human be able to mark "this change does not matter" (accept stale) to clear a node without re-running? Probably yes, as `stale_accept(node)` writing the new input hash.
