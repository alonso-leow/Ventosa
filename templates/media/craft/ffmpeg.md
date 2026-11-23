# Encoding recipes (K-ffmpeg)

Work on copies. Never write over camera files, takes or a signed edit; `ventosa-graph edit` stays the assembler.

## Rules
- Re-encode when trimming. Stream copy cuts only at keyframes and can leave frozen or missing frames at the start; use it only for sources with frequent keyframes.
- Even width and height (`scale=trunc(iw/2)*2:trunc(ih/2)*2`); web-safe output is H.264, yuv420p, faststart.
- Constant speed can be done in the renderer; variable speed is baked with FFmpeg (`setpts` for video, chained `atempo` for audio, each link between 0.5 and 2.0).
- After every run check: exit status, file plays, size is not zero.

## Recipes
```bash
ffmpeg -i in.mp4 -ss 00:00:30 -t 15 -c:v libx264 -c:a aac out.mp4                         # trim
ffmpeg -i in.mp4 -vf "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2" out.mp4   # fit with bars
ffmpeg -i in.mp4 -vf "scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080" out.mp4                      # fill and crop
ffmpeg -i in.mp4 -c:v libx264 -crf 23 -preset medium -c:a aac -b:a 128k out.mp4            # compress
ffmpeg -i in.mp4 -vn -ac 1 -ar 16000 audio.wav                                            # speech audio
ffmpeg -i in.mp4 -filter_complex "[0:v]setpts=0.5*PTS[v];[0:a]atempo=2.0[a]" -map "[v]" -map "[a]" out.mp4   # 2x
ffmpeg -f concat -safe 0 -i list.txt -c:v libx264 -c:a aac out.mp4                         # join
ffmpeg -i in.mp4 -vf "fade=t=in:st=0:d=1,fade=t=out:st=9:d=1" out.mp4                     # picture fades
ffprobe -v quiet -print_format json -show_format -show_streams in.mp4                      # facts
ffmpeg -i in.mp4 -af silencedetect=noise=-30dB:d=0.5 -f null - 2>&1                        # silence
```
To fit X seconds into Y seconds: speed = X / Y; `setpts` factor = 1 / speed; `atempo` = speed. Screen recordings: drop audio and speed up.

## Quality by use
Master CRF 18 slow · production 20-22 medium · web or preview 23-25 fast · draft 28+ veryfast.

## Failures
Missing file: check the path and quoting. Invalid data: the source is damaged; ask the human before re-recording. Odd dimensions: trunc scale. Missing encoder: install a full FFmpeg build. Zero-byte output: read the whole log. Audio drift after a speed change: use `filter_complex` with `atempo`. File too large: raise CRF or lower resolution.
