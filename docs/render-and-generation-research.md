# Render engines and generation services (research note)

Checked 2026-11. Re-verify licences and tool lists before relying on them.

## HyperFrames (HTML to video)
Open-source framework from HeyGen (Apache-2.0 reported). A composition is an HTML file: the root has `data-composition-id`, `data-width`, `data-height`, `data-start`; clips carry `data-start`, `data-duration`, `data-track-index`. Animation is seekable (GSAP timelines created paused and registered; CSS, Lottie, three.js, Anime.js, WAAPI also). Renderer: headless Chrome frame seeking plus FFmpeg, deterministic. CLI: `init`, `preview`, `render`, lint; non-interactive by default. Needs Node 22+ and FFmpeg. Local, Docker or AWS Lambda rendering. It ships agent skills; we do not copy them. Some features (hosted sharing, premium models) are account-gated.

## Remotion (React to video)
React compositions rendered frame by frame; everything driven by the current frame; typed props via schemas; Studio for preview; CLI `render` and `still`; strong for many variants of one template. Source-available licence with a company threshold: check before commercial use.

## Decision
HyperFrames is the default (agent-friendly, no build step, matches the house catalogue). Remotion when parameterised variants or an existing project demand it. Both are workspace tools installed through `tool_install` after an elevate (`K-render`). 3D is rendered separately and composited.

## The Artcraft suite (verified against the public repositories, 2026-11)
One publisher (organisation `storytold`), all Rust, Apache-2.0 (the studio itself MIT or Apache-2.0). Except the studio, each app documents `<app>-cli` with a headless **MCP server over stdio** (`<app>-cli mcp`), a JSON control channel and a CLI where every menu action is an engine command. Re-verify before relying: they are young and move quickly. `efectcraft` (as spelled in the brief) does not exist; the app is `effectcraft`.

| App | Purpose | AI access | Verdict |
|---|---|---|---|
| artcraft | creation studio: image and video generation, 2D compositing, 3D staging, image-to-3D | none documented for the AI (desktop app, generation through the publisher's credits; its web API has MCP-session records, undocumented). A community MCP wrapper is macOS-only, reads local session cookies, listed inactive | manual human app; not wired; backlog: re-check for an official MCP |
| anicraft | 2D animation editor | MCP + CLI + JSON channel | wired as tool: `motion_source` |
| texturecraft | 3D texture painting and baking | MCP + CLI | wired as tool: `scene_3d` |
| materialcraft | node-based material designer | MCP + CLI | wired as tool: `scene_3d` |
| figcraft | interface design and prototyping | MCP + CLI (400+ commands) | wired as tool: UI/screen inserts |
| vectorcraft | vector art | MCP + CLI | wired as tool: titles, assets |
| effectcraft | motion graphics and compositing | MCP + CLI | wired as tool: `motion_source` engine |
| gridcraft | spreadsheet | MCP + CLI | wired as tool, low priority (data behind charts) |
| cadcraft | CAD and drafting (DXF, PNG export) | MCP + CLI, `cadcraft-cli run/convert/commands/mcp` | wired as tool, niche (technical diagrams, plans) |
| filmcraft | nonlinear video editor (650+ commands; releases exist) | MCP + CLI | wired as tool: `edit` alternative |
| photocraft | photo editor (500+ commands, batch actions) | MCP + CLI | wired as tool: covers, thumbnails |
| designcraft | page and layout editor | MCP + CLI | wired as tool: thumbnails, title cards |
| deckcraft | presentations | MCP + CLI | wired as tool: slide beats |
| soundcraft | digital audio workstation | MCP + CLI | wired as tool: `audio_fx` |

"Wired" means: listed in the recommended tools with present/missing detection (an MCP server named for it, or `<app>-cli` on the path), described in `K-suite`, referenced by the nodes it helps. None is an AI node of its own, none is installed without `decision_ask`, and the AI discovers each server's real tool list at run time. No generation backend is wired: generation (`gen_media`) uses whatever generation MCP or API the human enables.
