# WERHD branding for the iOS client

The startup and menu use the same existing 王二火大 artwork as ra2web-werhd.
Both CDN responses were fetched and SHA-256 checked on 2026-10-20 against the
sibling checkout. These are reused project assets, not newly generated images.

- `werhd-loading.png`: https://res2.wangerhuoda.cn/glsl.png
  — original 800×600 PNG, SHA-256
  `138511a70934b526ed3389733523fb53b3c17b111739bbee713c510b83a94720`.
- Menu source: https://res2.wangerhuoda.cn/ra2ts_l.mp4?menu=werhd
  — 632×570, 15 fps, 431 frames, SHA-256
  `b9e6d9bc45e66b79efa9d222df70b4d69924bec4942145feaf55d0bcb2186ac0`.

`werhd-menu.wmvframes` stores that decoded video without changing any RGBA
pixels, timing or framing. It has PNG keyframes every 15 frames and bytewise
RGBA XOR deltas in between. The encoder checks all 431 restored frames against
ffmpeg's decoded bytes. Archive SHA-256:
`7d3e80167489002b8d212fa57fa666590338c9359c0175b18fe222ec283626e8`.
Concatenated decoded RGBA SHA-256:
`39a8b0189a0e63a5e8c0b68ed0d9ab6e923019c57da18431c74f69ecfceb6e9a`.

The menu player borrows the compressed archive and keeps one restored frame,
temporarily a second frame during decoding. It does not preload 431 RGBA images.
This only replaces interactive iOS startup and menu artwork. Other movies,
country loading images, desktop and retail capture sources retain their existing
paths. Startup loading text and original legal notices remain over the new image.

To reproduce the format conversion, install Pillow and numpy for the encoder,
then provide ffmpeg and the verified source MP4:

```sh
python3 tools/ios/encode_menu.py --source /path/to/ra2ts_l.mp4 \
  --output art/branding/werhd-menu.wmvframes --ffmpeg /path/to/ffmpeg --replace
```

Format: `VWMV0001`, six little-endian u32 values (width, height, FPS numerator,
FPS denominator, frame count, key interval), then frame-count-plus-one u64
offsets relative to the following PNG payload. Transparent delta pixels retain
significant RGB bytes; they must not be optimized away.
