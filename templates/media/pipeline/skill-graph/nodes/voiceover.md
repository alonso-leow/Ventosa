# Node skill: voiceover [AI]
Generate the voiceover audio for the final script. Skip it when the video is on-camera only.

**Inputs:** the active script in `scripts/`, voice presets in Ventosa ▸ settings ▸ voice (ChatGPT, ElevenLabs, DeepInfra, OpenRouter), `tone.md`.

**Do:** read the script line by line; render one audio file per paragraph to `assets/voice/<ID>-<n>.wav` with the configured preset (keys only in env vars); list them in `assets/voice/<ID>.json` with the script line each covers. Never send private text to a provider the human has not enabled; ask with `decision_ask` when a paid call goes beyond the batch limits.

**Done:** attach to `voiceover`, then `ventosa-graph write step-done <ID> voiceover "n files"` (note `skipped: on camera` when not needed).


**Writing for synthetic speech (`K-explainer`).** Sentences under 15 words; period = normal pause, comma = short, ellipsis = long, exclamation = energy, question mark = lifted end. Pace 150 wpm normal, 120 technical, 170 short-form. Read the line against the take's rhythm before rendering.
