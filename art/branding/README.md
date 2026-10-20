# Approved WERHD artwork for the iOS and macOS clients

These custom assets were reviewed as images and a complete video preview before
the user approved game integration on 2026-10-20. They replace the earlier direct
reuse of the web project's startup image and menu video.

- `wannafire-app-icon.png`: approved 1254×1254 opaque app-icon master. It derives
  from the current WannaFire website emblem: a W formed by two clashing abstract
  arms, with a flame in the middle. The approved imagegen interpretation uses
  rough silver/gunmetal bas-relief and an ember-red copper flame. SHA-256:
  `9732240fba3d38ec2831d93360c1c3fecb5b2872c645cd2af8442072d1cf6cbc`.
  Packaging resizes the complete master without cropping: `iconutil` builds the
  macOS ICNS; `actool` builds the iPhone/iPad icons and matching metadata. The
  ordinary Mac Dock uses this same master through the existing image setter.
  Chinese Apple builds display 王二火大 as their app name and window title.
  Bundle IDs, executable names, settings and save directories retain their
  existing internal identity. `tools/macos/package.py` packages the local Mac
  build, and `tools/ios/package.py` packages the device or simulator build.

- `werhd-loading.png`: 1448×1086, 4:3. The original Yuri splash's composition and
  silver winged crest were adapted with image_gen: seven yellow milk-frogs,
  cream bellies, rough skin, the original low-angle poses and red eye-covering
  helmet rings. The title is 王二火大, above it 全平台即时战略 / 合家欢游戏;
  the lower-right wordmark is 尤里的复仇 / YOULIDEFUCHOU.COM. SHA-256:
  `57fac5c77a9ccc8b8962d390141fe164fe9393d9f7c48a38e92d8e52b7901358`.
  iOS/macOS takes the inner `(4,3,1440,1080)` rectangle before scaling: this removes
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
temporary second frame during decoding. This replaces interactive iOS/macOS startup
and menu artwork only. Country loading images, other fullscreen/campaign movies,
retail capture and other-platform sources retain their existing paths. The startup
footer matches `ra2web-werhd/public/res/locale/zh-CN.json`'s copyright, brand and
fan-project disclaimer text. Left/right blocks are bottom-aligned with 10px
margins, at 13px text height using the same CoreText Medium glyph rasterizer as
the in-game UI; long lines wrap within their own columns. Their
text floats over a translucent bottom scrim inside the full-height 4:3 picture.
The loading label sits above that overlay; no separate footer shrinks the art.

Format: `VWMV0001`, six little-endian u32 values (width, height, FPS numerator,
FPS denominator, frame count, key interval), then frame-count-plus-one u64
offsets relative to the following PNG payload. Transparent delta pixels retain
significant RGB bytes; they must not be optimized away.
