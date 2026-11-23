# Generated media (K-genmedia)

Image, video, audio and 3D generation through an external service. Used for inserts, backgrounds, concept frames, reference stills, and as source material for motion sources and 3D. Everything here is paid or third-party until proven otherwise.

## Backends (only what the AI can really call)
- A generation MCP or API the human enabled in settings (image, video, audio, 3D). Discover its tools at run time (list its models and estimate tools); never assume names. Paid by default.
- Local generation (ComfyUI, local diffusion) when installed: no cost, same licence and review rules.
- The Artcraft desktop app (image and video generation with 2D compositing and 3D staging) is a **manual human app**: it has no documented CLI or MCP for the AI. A community MCP wrapper exists for its subscription generation, but it is macOS-only, reads the app's local session cookies and was listed inactive: do not wire it; ask the human before ever using it. The human can generate in the app and drop files into `assets/generated/<ID>/` with a sidecar note.
- The suite of local creation apps (video editor, motion graphics, 2D animation, vector, photo, design, UI, slides, textures, materials, audio, CAD, spreadsheet) is **not** generation: they are callable tools, see `K-suite`.

## Flow
1. List models, pick by need (duration, aspect ratio, references, audio, keyframes).
2. Estimate cost. Estimates never spend credit.
3. **Ask** with `decision_ask`: provider, model, prompt, references to upload, estimated cost, number of results. Batch limits in settings decide whether a call is allowed without asking; above them always ask.
4. Generate, wait, fetch to `assets/generated/<ID>/` with a sidecar `<file>.json` (provider, model, prompt, seed, cost, date).
5. Review (human via `motion_ok`); label generated clips as generated in `<ID>-sources.md`.

## Prompting
State subject, setting, camera, light, motion and style in a few clauses; one subject and one motion per clip; vertical or horizontal as the target needs; short clips (a few seconds) and cut them, rather than long ones; for consistency reuse references and seeds; keep text out of the picture and add it in the motion layer.

## Gates
- Paid, cloud or third-party: always ask first (above).
- Prompts and references contain nothing private: no names or faces of private people, no unpublished material, no credentials, no personal data.
- Real people, real brands, logos, copyrighted characters or styles of a living artist: do not generate; ask.
- Provider terms and output licence recorded in the sidecar.
- Credentials come from the provider's own login or env vars; never write them to files, never print them.
- Disclose generated media where the platform or `tone.md` requires it.
- A failed or partial job is reported with its cost; no silent retry loops (at most one retry, then ask).
