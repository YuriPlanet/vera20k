# Approved WERHD artwork for the iOS client

These custom assets were reviewed as images and a complete video preview before
the user approved game integration on 2026-10-20. They replace the earlier direct
reuse of the web project's startup image and menu video.

- `werhd-loading.png`: 1448×1086, 4:3. The original Yuri splash's composition and
  silver winged crest were adapted with image_gen: seven yellow milk-frogs,
  cream bellies, rough skin, the original low-angle poses and red eye-covering
  helmet rings. The title is 王二火大, above it 全平台即时战略 / 合家欢游戏;
  the lower-right wordmark is 尤里的复仇 / YOULIDEFUCHOU.COM. SHA-256:
  `57fac5c77a9ccc8b8962d390141fe164fe9393d9f7c48a38e92d8e52b7901358`.
  iOS takes the inner `(4,3,1440,1080)` rectangle before scaling: this removes
  the generated 2-3px cyan outer edge while preserving the 4:3 composition.
- `werhd-menu-logo.png`: the approved silver winged crest extracted with
  image_gen as a transparent PNG for the menu overlay. It retains both gold
  subtitle lines and the red/gold 王二火大 title. SHA-256:
  `63e7822925af6eaf2fe2fe9a759016ae8c94cb2eb10bf669ac819374e83e0b6a`.
- `werhd-menu.wmvframes`: the approved menu composition, 632×570, 15 fps,
  431 frames / 28.733 seconds, no audio track. The source is the original Yuri
  `langmd.mix → RA2TS_L.BIK`, SHA-256
  `1e700e4b0489b5b484d71a863d0d9f3fb15399002c40df22cf924ee79f600d49`.
  Only the stationary logo is overlaid; the original CRT frame, animated globe,
  footage, timing and loop boundary are retained. Independent menu music stays
  on its existing playback path.

To reproduce the lossless menu master with ffmpeg and the original BIK:

```sh
ffmpeg -i RA2TS_L.BIK -loop 1 -framerate 15 -i art/branding/werhd-menu-logo.png \
  -filter_complex '[1:v]crop=2142:662:16:31,scale=500:155:flags=lanczos,format=yuva420p[logo];[0:v][logo]overlay=x=66:y=174:format=yuv420:shortest=1[out]' \
  -map '[out]' -frames:v 431 -an -c:v ffv1 -level 3 -pix_fmt yuv420p menu-master.mkv
python3 tools/ios/encode_menu.py --source menu-master.mkv \
  --output art/branding/werhd-menu.wmvframes --ffmpeg /path/to/ffmpeg --replace
```

The archive keeps decoded RGBA pixels without further compression loss, resizing,
cropping or retiming. It has PNG keyframes every 15 frames and bytewise RGBA XOR
deltas in between. The converter checks all 431 restored frames against ffmpeg's
decoded bytes. Archive SHA-256:
`87755d05f56a8ecb2e027543378edf8b8711992d4ec8d53fcef0e97e9f3f132e`.
Concatenated decoded RGBA SHA-256:
`81c3279506f84de3dad9ae2e4d8cc979eb894187cb4acfdcb3d8c8c00e81e054`.

The player borrows the compressed archive and keeps one restored frame, with a
temporary second frame during decoding. This replaces interactive iOS startup
and menu artwork only. Country loading images, other fullscreen/campaign movies,
desktop and retail capture sources retain their existing paths. Startup legal
notices remain; the loading label moves to the lower left above those notices to
leave the approved crest clear.

Format: `VWMV0001`, six little-endian u32 values (width, height, FPS numerator,
FPS denominator, frame count, key interval), then frame-count-plus-one u64
offsets relative to the following PNG payload. Transparent delta pixels retain
significant RGB bytes; they must not be optimized away.
