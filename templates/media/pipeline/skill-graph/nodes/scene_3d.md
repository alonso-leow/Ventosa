# Node skill: scene_3d [AI]
The 3D specialist. Builds models, stills and short clips for shots the plan marks 3D. Skip with `step-done … "skipped: no 3D shots"` when none. Knowledge: `K-3d` (read first), `K-motion`, `K-render`, `K-animations`.

**Inputs:** the 3D lines of `assets/plans/<ID>-shots.md`; style sheet `assets/refs/<ID>-style.md`; generated meshes or references in `assets/generated/<ID>/`; the human's models.
**Do:** pick the lightest tool that fits (three.js inside the motion scene; headless Blender or the Blender MCP; a generated mesh cleaned and staged). Block out, then materials, then motion; low-sample preview before the final. Write models to `assets/models/`, scene sources to `assets/scenes3d/<ID>/`, renders to `assets/renders/<ID>-3d-<beat>.<mp4|png>` (alpha when it will be composited), and `assets/scenes3d/<ID>/notes.md` with tool, version, seed, camera, units and every asset's licence.
**Rules and gates:** as in `K-3d`: licences recorded; installs through `decision_ask`; untrusted `.blend` files opened with auto-run off; paid or cloud 3D and hosted rendering asked first with estimate and what is sent; no private images or real people's faces; ask before a render longer than about 10 minutes.
**Done:** `ventosa-graph write step-done <ID> scene_3d "n models, m renders"`. `motion_source` composites these; `motion_ok` reviews.
**Suite helpers (`K-suite`):** texturecraft and materialcraft (headless MCP/CLI) can paint, bake and render textures and materials for the models; install only after `decision_ask`.
