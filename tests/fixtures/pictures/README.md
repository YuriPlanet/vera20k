# Picture fixtures

- `dispose_then_opaque.webp`: a 16x16 lossy animated WebP (written by libwebp through Pillow). Frame 0 is a
  yellow band with alpha (`ALPH` + `VP8 `), no blend, disposed to the background; frame 1 is an opaque 4x4
  green square at (6,6) (`VP8 ` alone, no alpha). Frame 1 shows the 16 green pixels only: `image-webp`
  0.2.4's own compositing clears frame 0's region with RGB strides before an alpha-less frame and leaves
  some of it (`src/assets/picture_file.rs`, `WebpFrames`).
