# Node skill: link_media [ai]
Link what the teleprompter recorded to the media files, after the files are in (recorded, imported or dropped in the library).

**Do:** 1) `teleprompter_log` (workspace, video=<ID>, refresh true): takes with the text lines on screen, and the matched file per take with confidence and the clock offset. 2) `media_list` (workspace, subject=<ID>): every file with its `teleprompter` match. 3) Files with `unmatched`, a confidence under 0.5, or two takes on one file: say which, with the nearest take and the time gap. A constant offset of minutes means a camera clock skew: tell the human to set `clock_offset_s` in `recording/config.json`.
**Elevate:** a take with no file, or a file that matches two different lines equally (decision_ask with the options).
**Done:** `ventosa-graph write step-done <ID> link_media "N files linked, M unmatched"`. The mapping itself is stored by Ventosa in `recording/media_map.json`; do not edit it.
