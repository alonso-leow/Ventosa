# Node skill: script [AI]
Only reached when the human approved a **paragraph stack** in the Thesis tab (activating a version skips this node).
Produce the version that matches the stack and activate it.

**Inputs:** `labelling/labels.json` → `thesis`, `stack` (ordered; `opt`/`i` = which option paragraph it came from, `text` = the human's intent, possibly edited), `notes`, `label.rev`; the drafted versions in `scripts/<ID>/` (human-edited ones included); `recording/clips.json` (lines already recorded); knowledge as in `nodes/drafts.md`.

**Do:**
1. If the stack is exactly one option's paragraphs, unchanged, its version already matches: activate that file (step 4) — don't rewrite it.
2. Otherwise write `scripts/<ID>/L<rev>-<slug>.md` in the version format of `nodes/drafts.md` with `option: mix`, `thesis:` = the label thesis. Build it from the drafted versions: for each stack paragraph reuse the beats of the version it came from (`opt`), in stack order; write new beats for blank/edited paragraphs; smooth the joins so it reads as one script.
3. Rules: hook first, SP second (exact sponsor text), CTA last; ≤7 HF animations; content rules from tone.md; facts only from codebase-refs/addenda. Keep beat ids (and narration) of lines that already have kept takes unless the stack changed them. Update `## Notes` and `## Questions for the host`.
4. Activate: `ventosa-graph write script-done <ID> scripts/<ID>/<file>.md` — validates, copies to `episodes/` (`status: final`, `label_rev`, `version`), rebuilds `recording/clips.json`.

**Done when:** the active copy in `episodes/` is final with `label_rev` = `label.rev` → `record` (human). If the human re-opens and re-approves the label, rev rises and the episode comes back here.
