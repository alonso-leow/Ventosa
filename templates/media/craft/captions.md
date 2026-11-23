# Open captions (K-captions)

Timing, wrapping and checks come from the `subtitles` policy. This file adds the look of burnt-in captions and the word-highlight mode.

## Looks
- **plain:** white, soft drop shadow.
- **bold:** heavy double shadow (default for shorts).
- **outline:** black outline on all four sides.
- **glow:** brand-colour halo.
- **box:** dark translucent box; the active word sits on a brand-colour chip.
Parameters: position (top, centre, bottom), size (about 64 px at 1080 wide), active-word colour, text colour, page window (words within about 1.2 s share a page), time offset for clips cut from a longer take (subtract the clip start, drop what falls before zero).

## Rules
- Brand colour comes from `tone.md`, not a default accent.
- Word highlight only on reels and shorts; the long video ships a sidecar `.srt`.
- Social video is mostly watched muted: shorts always carry captions.
- Never cover the face or the platform interface band; stay inside the safe margin.
- Text is what was said, in the script's spelling; never add words.
- Word-level timestamps are stored as JSON with start and end in ms.
