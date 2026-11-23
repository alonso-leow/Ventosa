# Render backends for motion sources (K-render)

Two engines turn code into video. Both render frame by frame, so every animation must be a pure function of time.

| | HyperFrames | Remotion |
|---|---|---|
| Source | one HTML file; clips carry `data-start`, `data-duration`, `data-track-index`; root has `data-composition-id`, `data-width`, `data-height` | React components with `<Composition>`, `<Sequence>`, `useCurrentFrame()` |
| Animation | seekable timelines: GSAP (paused, registered for the renderer), CSS, Lottie, three.js, Anime.js, WAAPI | frame-driven maths: `interpolate`, `spring` |
| Build step | none; preview the HTML directly | Node project, bundler, registered compositions |
| Needs | Node 22+, FFmpeg, headless Chrome | Node 20+, FFmpeg, browser; company licence for larger companies (check its terms) |
| CLI | `hyperframes init / preview / render` (lint available) | `remotion render / still`, Studio for preview |
| Licence | open source (Apache-2.0 reported; confirm in the repo) | source-available with a commercial threshold |
| Strength | agent-written scenes with little boilerplate; web animation ecosystem; house catalogue exists | typed props and schemas, large community, data-driven batch renders |

## Which to use
1. A catalogue entry exists in `K-animations` -> HyperFrames.
2. Mostly typography, overlays, lower thirds, explainers, web-style motion -> HyperFrames (default).
3. The scene needs typed parameters over many variants (20 videos from one template), or an existing Remotion project -> Remotion.
4. Heavy 3D or shaders -> render the 3D separately (`K-3d`), composite as a clip in either engine.
5. Do not mix engines inside one scene; mix at clip level through `assets/renders/`.
6. Unsure, or both licences matter -> `decision_ask`.

## Rules for both
Render at the master size and fps from the plan; write to `hyperframes/renders/<name>.mp4` or `assets/renders/`; keep sources next to the render; never hit the network during a render (assets are local); fonts local or preloaded; no wall-clock timers; no CSS transitions in a frame-seek engine.
Remotion specifics: drive everything from the current frame, write times in seconds times fps, clamp interpolations, premount sequences, register each composition, keep playback rate constant (bake variable speed with FFmpeg), media from the public folder.
HyperFrames specifics: create the timeline paused and register it, set every clip's start/duration/track, lint before rendering, preview before the full render.

## Wiring and install
Both are workspace tools: `tool_install` stores the command (for example `npx hyperframes render ...` or `npx remotion render <id> out/x.mp4`), `tool_run` runs it. The tool list shows whether each is present (`tools_recommended`).
**Gate:** installing any package, global CLI or `npx` download is an elevated action. Ask once per workspace with `decision_ask` (package, version, source, why), never run install scripts from anything you downloaded, and record the choice in the workspace. Cloud or distributed rendering (Lambda, hosted render APIs) sends code and assets to a third party and may cost money: ask with provider, estimate and what is sent.
Skills the HyperFrames project ships for agents are theirs; do not copy them into the workspace, read the project's docs instead.
