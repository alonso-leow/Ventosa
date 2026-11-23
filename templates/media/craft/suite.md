# Local creation apps the AI can call (K-suite)

A family of open-source (Apache-2.0, Rust) creation apps from one publisher. Each ships a headless command line and a **headless MCP server over stdio** (`<app>-cli mcp`), plus a JSON control channel; every menu action is an engine command. They run locally and cost nothing. That is the only reason they appear as tools: nothing is wired for an app the AI cannot call.

| App | Does | Use in the graph |
|---|---|---|
| filmcraft | nonlinear video editing: cut, trim, grade, mix, export | `edit`, `color_grade` as an alternative assembler when the human wants an editable project |
| effectcraft | motion graphics and compositing, headless render | `motion_source` (third engine, local render; compare with `K-render`) |
| anicraft | 2D animation, GIF and video export | `motion_source` (hand-drawn or character animation inserts) |
| vectorcraft | vector art: titles, logos, shapes | `motion_source` (assets), thumbnails |
| photocraft | photo editing, batch actions | covers and thumbnails, `publish_meta` assets |
| designcraft | page and layout design, render pages to PNG | thumbnails, title cards, diagrams |
| figcraft | interface design and prototypes | screen mockups and UI inserts for `motion_source` |
| deckcraft | slide decks | slide-style beats for screen shots |
| texturecraft | 3D texture painting and baking | `scene_3d` |
| materialcraft | node-based materials, headless render | `scene_3d` |
| soundcraft | digital audio workstation | `audio_fx` (mixing, stems, bounce) |
| cadcraft | CAD and drafting, DXF/PNG export | technical drawings and plans as diagram inserts |
| gridcraft | spreadsheet | tables and figures behind charts and numbers on screen |

## How to use
1. Check `tools_recommended`: present means an MCP server of that name is registered or `<app>-cli` is on the path. Missing: installing (building from source, registering the MCP server) is an elevated action: `decision_ask` once per workspace.
2. Ask the MCP server for its tool list and the CLI for `commands`; use what is there, do not assume. Prefer headless runs; keep project files in the subject's folder (`assets/scenes3d/<ID>/`, `assets/inserts/<ID>-*`), outputs in `assets/renders/` or `assets/inserts/`.
3. Everything they produce is source material: record the app, version and project file in `<ID>-sources.md`; the human reviews at the node's gate as for any motion source.
4. The apps are young and move fast: a missing or changed command is reported, not worked around silently.

## Gates
Install and register: ask. Network: these apps run offline; refuse any command that uploads or shares a file unless the human approved it. Third-party fonts, models and assets inside a project keep their own licences. Never open untrusted project files with scripting enabled.
Related manual app: Artcraft (generation studio) has no AI-callable surface, see `K-genmedia`.
