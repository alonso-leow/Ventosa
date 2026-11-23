# Node skill: color_grade [ai]
Finish the signed cut: colour and grade consistent across shots, titles and lower thirds in the brand, safe margins, no clipped highlights. Work on a copy named `<ID>-graded.<ext>` in `assets/edits/`; never overwrite takes. The graded file becomes the cut that subtitles and publish use.
**Elevate:** a look the tone.md does not define; a brand colour you are unsure of.
**Done:** `ventosa-graph write step-done <ID> color_grade "graded file name"`. A human approves the whole cut next (`edit_review`).

**Gates (`K-ffmpeg`, `K-platforms`):** grade from the signed master (CRF 18 slow, even dimensions, yuv420p, faststart); check frame 0 is not frozen and audio stays in sync; no clipped highlights; safe margins on every target aspect ratio.
