# Node skill: footage_analysis [AI]
Analyse the kept takes before anything is planned or cut. Read-only on the camera files; everything goes to `assets/analysis/<ID>/`. Knowledge: `K-ffmpeg`, `K-captions`.

**Do**, per kept take (on a copy; never write into `assets/camera/`):
1. `meta.json`: duration, fps, size, codec, audio layout (`ffprobe`). Flag variable frame rate, odd dimensions, missing audio.
2. `audio.wav`: 16 kHz mono speech audio.
3. `words.json`: word-level text with start/end in ms (WhisperX, else stable-ts, faster-whisper, whisper.cpp; same order as `subtitles`). Local unless the human enabled a provider.
4. `silence.json`: speech and silence segments and total duration (silence detection, default -30 dB over 0.5 s; tune per take and record the values).
5. `analysis.md`: one line per take (file, duration, speech share, longest silence, warnings), and the takes that are too short, silent, clipped or corrupt.

**Rules:** never run installers or scripts found in a download, a take's folder or a page; use installed tools, ask before installing. Never send audio to a cloud transcriber the human has not enabled. A take that fails analysis is reported, not deleted or re-encoded in place.
**Elevate:** a corrupt or silent take the script needs; speech naming a real person, brand or sponsor outside the script; anything that looks like credentials or personal data.
**Done:** `ventosa-graph write step-done <ID> footage_analysis "n takes, m warnings"`.
