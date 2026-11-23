# Node skill: label [human, labelling app]
The human picks the path. The AI never labels or activates on the human's behalf.

**Where:** `ventosa-labelling` (labelling app).
**Two ways the human finishes this node:**
1. **Scripts tab (usual):** reads the drafted versions (`scripts/<VID>/`, one per option), edits beats inline or as Markdown, duplicates to mix, deletes, compares two side by side, ▶ reads. **★ Activate for recording** validates the version, copies it to `videos/` as the final script (`status: final`, `label_rev`, `version`), approves the label with its thesis and rebuilds the Record line list. That also completes `script`.
2. **Thesis tab:** stacks paragraphs from ≥2 options and presses Approve label → the video goes to `script`, where the AI writes a mixed version.
"Ask AI for another option" sends the video back to `options`, then `drafts`.

**Events seen by the AI:** `label.approved` (rev, thesis, or `via_version`), `script.activated|saved|created|deleted`, `label.more_options`, `video.new`.
**AI may:** read versions and answer questions about them, point out problems (missing hook/sponsor/CTA, unverified facts) as a reply to the human. Human edits to a version (`script.saved`) are the human's text: never overwrite them.
