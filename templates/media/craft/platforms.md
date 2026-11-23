# Platform targets (K-platforms)

| Target | Size | fps | Length limit | Notes |
|---|---|---|---|---|
| YouTube | 1920x1080 (up to 8K) | 30 | 12 h | AAC 48 kHz; upload high quality, it re-encodes |
| YouTube Short | 1080x1920 | 60 | 60 s | |
| TikTok | 1080x1920 | 30 | 10 min, 287 MB | |
| Instagram Reel | 1080x1920 | 30 | 90 s, 4 GB | |
| Instagram Story | 1080x1920 | 30 | 15 s | |
| Instagram post | 1080x1080 (feed up to 1080x1350) | 30 | 60 s | |
| X | 1080x1080 or 1280x720 (max 1920x1200) | 30 | 140 s, 512 MB | AAC 44.1 kHz |
| LinkedIn | 1920x1080 (max 4096x2304) | 30 | 10 min, 5 GB | AAC 48 kHz |
| Web embed | 1280x720 | 30 | any | small file, progressive |

Flow: master (signed or graded edit) to one export per target to a human upload. Always export from the master, never from another export.

```bash
# YouTube
ffmpeg -i master.mp4 -c:v libx264 -preset slow -crf 18 -profile:v high -level 4.0 -bf 2 -g 30 -c:a aac -b:a 192k -ar 48000 -movflags +faststart yt.mp4
# Vertical (Shorts, Reels, TikTok)
ffmpeg -i master.mp4 -vf "scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2" -c:v libx264 -crf 18 -c:a aac -b:a 192k vertical.mp4
# X, aim for 15 MB
ffmpeg -i master.mp4 -c:v libx264 -preset medium -crf 24 -profile:v main -vf "scale='min(1280,iw)':'min(720,ih)':force_original_aspect_ratio=decrease" -c:a aac -b:a 128k -ar 44100 -movflags +faststart -fs 15M x.mp4
# LinkedIn
ffmpeg -i master.mp4 -c:v libx264 -preset medium -crf 22 -profile:v main -c:a aac -b:a 192k -ar 48000 -movflags +faststart li.mp4
# Web
ffmpeg -i master.mp4 -c:v libx264 -preset medium -crf 26 -vf scale=1280:720 -c:a aac -b:a 128k -movflags +faststart web.mp4
# GIF preview
ffmpeg -i master.mp4 -t 5 -vf "fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse" preview.gif
```
Check each file: `ffprobe -v error -show_entries format=duration,size -of csv=p=0 file`. Keep text inside a safe margin (about 60 px at 1080 wide) and captions out of the platform's interface band on vertical formats.
