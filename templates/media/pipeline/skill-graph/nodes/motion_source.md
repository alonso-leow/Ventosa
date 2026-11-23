# Node skill: motion_source [AI]
Make the motion sources the cut will use: the moving picture that carries the argument (renders, screen captures, animations, diagrams).

**Inputs:** the script's beats, `research/` references, the knowledge animation list, HyperFrames and MetalVJ renders, `assets/screen/`.

**Do:** for each beat that needs a visual, produce or pick a clip into `hyperframes/renders/<name>.mp4`, `assets/renders/` or `assets/inserts/<ID>-<beat>.<mp4|png>`; write `assets/inserts/<ID>-sources.md` mapping beat → file → why it shows the claim. Prefer the real thing over a stock card. List the beats without a source; the edit turns them into act-coloured cards.

**Done:** attach to `motion_source`, then `ventosa-graph write step-done <ID> motion_source "n sources, m missing"`.


**Render backends (`K-render`).** Two engines, HyperFrames (HTML, seekable timelines; default) and Remotion (React, typed props; for many variants of one template or an existing project). Pick by the decision list in `K-render`; one engine per scene, mix only at clip level. Both are workspace tools (`tool_install`, `tool_run`); installing either, or any package they need, goes through `decision_ask` once per workspace; cloud or distributed rendering is asked every time with provider, estimate and what is sent.
**Inputs from other nodes:** 3D renders from `scene_3d`, generated clips from `gen_media`, style sheet from `visual_refs`. Composite them; never regenerate them here.
**Craft (`K-animlib`, `K-motion`):** pick tools from the `K-animlib` table; the house catalogue wins. Gates: transform and opacity only, 60 fps, at most 3 flashes per second, animations pure functions of time, palette of 3-5 colours, headline over subhead over body, exits shorter than entrances, one visual per message, on-screen text readable for 3-4 s.
**Extras and their gates:** cut-out of a subject from an image (background removal tools can carry copyleft or commercial licences: check, and ask before using one in a product; only on the host's own images; write a new `-nobg.png`, never over the source). Third-party components, Lottie, stock: licence per file in `<ID>-sources.md`. Package installs never automatic.
**Local suite apps (`K-suite`):** effectcraft, anicraft, vectorcraft, figcraft and others are callable through their MCP servers or CLIs when `tools_recommended` shows them present. Use them as extra engines for the shots they fit; ask before installing; they run offline.
