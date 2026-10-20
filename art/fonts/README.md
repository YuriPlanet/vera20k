# iOS UI font

`FiraSansCondensed-Medium.ttf` is the unmodified Google Fonts upstream face
used to match the WERHD web UI's Fira Sans Condensed, weight 500. Its SIL Open
Font License 1.1 and copyright notice are included in
`FiraSansCondensed-OFL` and remain embedded in the font metadata.

- Font: https://github.com/google/fonts/blob/main/ofl/firasanscondensed/FiraSansCondensed-Medium.ttf
- License: https://github.com/google/fonts/blob/main/ofl/firasanscondensed/OFL.txt
- Font SHA-256: `6385967fe921a6ee837221cde461e695e77f8931a3ca4bc82c7c1c1a5fe47f86`

The web font has no Chinese glyphs. iOS CoreText resolves Chinese through this
Medium face's system sans-serif cascade, normally PingFang SC Medium. The app
uses the installed Apple font on the device; no Apple font is copied into the
repository or app. Other missing glyphs retain GAME.FNT coverage as a fallback.

The shared atlas keeps GAME.FNT's character widths, spacing and line height.
This changes ordinary iOS UI glyphs, while desktop rendering and sealed retail
captures retain their existing bitmap font. The license is also referenced by
`include_str!` in the iOS rasterizer so it accompanies the embedded font bytes.
