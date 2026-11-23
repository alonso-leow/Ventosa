# Node skill: audio_fx [AI]
Plan and prepare the audio around the voice: music bed, ducking, sound effects, loudness.

**Do:** from the script's acts write `assets/audio/<ID>-audio.md`: per act the bed (file, level), the cues (sfx, hit points on beats), the ducking under speech, and the loudness target (−14 LUFS). Put only licensed or generated audio in `assets/audio/`. Never use copyrighted music; ask with `decision_ask` when unsure.

**Done:** attach to `audio_fx`, then `ventosa-graph write step-done <ID> audio_fx "bed + n cues"`.


**Levels and rules (`K-motion`, `K-explainer`).** Bed 40-60% (20-30% under narration), effects 60-80%, ambience 20-30%, voice 80-100%; duck 6-12 dB under voice and effects using the speech segments in `assets/analysis/<ID>/silence.json`. Bed shape: swell in the first 3 s, quiet loop under speech, swell at the call to action; no vocals under narration; fade, never hard-cut; silence before impacts. Layer 2-3 effects; land hits on the beat and with the action, not before it. Effect families: interface (whoosh, pop, click, swipe), emphasis (impact, boom, ding, chime), atmosphere (hum, texture, riser, tail).
**Gate:** every file has a recorded licence or is generated (`gen_media` rules); no copyrighted music; final loudness -14 LUFS.
