# Node skill: export_formats [AI]
Make the platform files from the signed (or graded) master after subtitles. Nothing is published. Knowledge: `K-platforms`, `K-ffmpeg`, `K-captions`.

**Do:** read the workspace's `platform` and the reels list. For each target in `K-platforms` write `assets/exports/<ID>-<platform>.mp4` from the master with the matching recipe. Burn captions only for vertical targets (the `--burn` track, `K-captions` look); the long video keeps its sidecar `.srt`.
Check every file: duration and size within the limit, even dimensions, audio -14 LUFS in AAC at the platform's rate, plays from frame 0 with no frozen start, captions inside the safe margin. Write `assets/exports/<ID>-exports.md` (file, spec, size, duration, checks).

**Rules:** start from the master, never another export; never write into `assets/edits/`; a file over the limit is re-encoded, not truncated, unless the human approves a shorter cut.
**Elevate:** a platform not in `K-platforms`; a target length that needs content cut; any upload or scheduling (the human's `upload` step).
**Done:** `ventosa-graph write step-done <ID> export_formats "n files"`.
