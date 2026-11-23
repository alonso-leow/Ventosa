# Node skill: options (Thesis options) [AI]
Turn the pool (`research/<VID>-pool.md`, or the title and notes when there is none) into thesis options. This node is the thesis preset: each option defines its objectives logically before any script exists.

**Inputs:** `labelling/labels.json` → `title`, `notes`, existing `options`, `want_more`; the pool; knowledge: `tone.md`, `meta/meta-script.md`, `meta/addenda.md`, `research/codebase-refs.md`, `research/papers.md`, `meta/structures.md` (expected shape).

**Do:**
1. Write ≥2 options, or exactly one more when `want_more` is true. Each is a different angle taken from the pool, not a reword.
2. Each option: `name` (2–5 words), `style` (one word), `thesis` (one sentence the whole piece proves), `paras` (7–12 lines, one per paragraph, saying what it argues).
3. Objectives, logically: in the first `paras` line write `Objectives: …` = what the viewer should know, feel and do at the end, as 2–3 claims where each follows from the one before. Name the counter-argument and the one piece of evidence that answers it.
4. Frame: para 2 `Hook: …` (≤40 s, curious/sensory), `Sponsor line.`, three acts (perception → numbers → pixels), last para the CTA linked to the next video.
5. Only claims backed by codebase-refs; nothing unbuilt promised as built. Follow the content rules in tone.md.

**Write (one call per option):** `ventosa-graph write option <VID> '{"name":"…","style":"…","thesis":"…","paras":["Objectives: …","Hook: …","Sponsor line.",…]}'`
New video from chat: `ventosa-graph write new <VID> "Title" "notes"` (no attachment needed), then attach to `pool`.

**Done when:** ≥2 options and `want_more` false → `thesis` (the human selects the thesis in the Label app before any draft).
