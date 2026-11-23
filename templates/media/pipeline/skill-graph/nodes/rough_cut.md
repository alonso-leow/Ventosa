# Node skill: rough_cut [AI]
Turn the analysis into a cut list for `edit`. A plan only; nothing is rendered. Knowledge: `K-ffmpeg`, `K-explainer`, `K-platforms`. Inputs: `assets/analysis/<ID>/silence.json` and `words.json`, the active script, `assets/plans/<ID>-shots.md`.

**Do:** write `assets/plans/<ID>-cuts.json` (`take`, `segments` with start/end seconds, `params`) per take:
1. Pad each speech segment (default 0.1 s; 0.15 s for dialogue so breaths survive) and drop segments under a minimum length (default 0.3 s).
2. Merge segments whose gap is within a threshold (default 0.3 s) so cuts do not stutter.
3. Total the length against the script's target and the platform limit.
4. Decide by speech, not silence alone: keep deliberate pauses before a punchline or after a key claim, remove false starts, repeated takes of one line and dead air. Prefer the take the human kept.
5. Clips for shorts: start and end on sentence boundaries, never mid-word; caption times shift by the clip start.
6. Record the parameters and `removed_seconds`.

**Rules:** do not cut inside a quote, a sponsor or disclosure line (exact text in `tone.md`) or a number. The list is non-destructive: no take or label is deleted. At least a 2-frame audio crossfade at joins; no jump cut across a shot-type change without a cover insert.
**Elevate:** more than 40% of a take removed; a cut that changes a sentence's meaning; a sponsor or disclosure line that would be dropped.
**Done:** `ventosa-graph write step-done <ID> rough_cut "n segments, X s removed"`. `edit` follows the list.
