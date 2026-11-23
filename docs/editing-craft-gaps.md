# Editing craft: gap analysis and what was added

A reference project for AI-driven video editing (a code-first editor with a pipeline for analysis, transcription, silence cuts, captions, motion templates and renders) was read for ideas only. Nothing from it is copied: every rule below was rewritten for Ventosa's skill-graph model.

Where it lands: the media template (`templates/media/pipeline/skill-graph/`) and its knowledge files in `templates/media/craft/` (`K-motion`, `K-explainer`, `K-ffmpeg`, `K-platforms`, `K-captions`, `K-animlib`, `K-render`, `K-3d`, `K-genmedia`, `K-browser`, `K-review`). The podcast template is untouched. Existing workspaces keep the graph they were created with.

| Capability | Class | Node | What it gives |
|---|---|---|---|
| Take analysis (metadata, speech audio, word timings, silence) | new | `footage_analysis` | read-only report per take, local transcription |
| Silence and dead-air cutting, clip extraction | new | `rough_cut` | non-destructive cut list: padding, minimum length, gap merge, protected lines |
| Visual reference browsing | new | `visual_refs` | style sheet from public pages, read-only browsing gates |
| Platform exports | new | `export_formats` | per-platform files, limits and checks, nothing published |
| 3D scenes | new | `scene_3d` | models, stills and clips from three.js, Blender or generated meshes |
| Generated media | new | `gen_media` | external generation with estimate-then-ask |
| Review before the human | new | `auto_review` | measured checks, sampled frames, findings routed to the owning node |
| Motion planning (spec format, timing budgets, quality gate) | merge | `shot_plan` | |
| Script structures and pacing | merge | `script`, `drafts`, `voiceover` | |
| Audio layering and ducking | extend | `audio_fx` | |
| Render engines (HTML and React) | merge | `motion_source` | decision list, install gate |
| Encoding rules and caption looks | extend | `edit`, `color_grade`, `subtitles` | |

Rejected: any instruction to install packages, start servers or open pages without asking. The graph's `elevate` text now says the opposite: installs, found scripts, paid or cloud calls, providers, licences, destructive edits and unusual cuts are always asked.

Open: `edit` does not yet read `cuts.json` in code (the node text tells the agent to); the licences of copied component kits and any Remotion use must be checked per workspace.
