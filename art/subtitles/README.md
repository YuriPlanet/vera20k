# Optional local Simplified Chinese movie subtitles

The Apple client accepts a local `subtitlemd.txt` sidecar. It does not replace,
transcode or modify the original Bink movies or their English soundtrack.

The inspected local experience uses the text-only subtitle source from
[Translate-with-LOVE/Ra2-zh_hans-main](https://github.com/Translate-with-LOVE/Ra2-zh_hans-main)
at commit `9945562ad6d0be7e84750ebe8d50cee778e5ebff`:

- [subtitle/subtitlemd.txt](https://github.com/Translate-with-LOVE/Ra2-zh_hans-main/blob/9945562ad6d0be7e84750ebe8d50cee778e5ebff/subtitle/subtitlemd.txt)
- [credits/creditsmd.txt](https://github.com/Translate-with-LOVE/Ra2-zh_hans-main/blob/9945562ad6d0be7e84750ebe8d50cee778e5ebff/credits/creditsmd.txt)
- [Project README](https://github.com/Translate-with-LOVE/Ra2-zh_hans-main/blob/9945562ad6d0be7e84750ebe8d50cee778e5ebff/README.md)

Copyright and attribution belong to the original game's rights holders and the
localization contributors listed in that project's credits. The inspected
repository has no explicit redistribution license. Accordingly, its subtitle
text and credits are **not checked into this repository**. Keep the original
source, credits and this provenance with optional local packages; review
redistribution rights before any public release.

The inspected UTF-16LE file is 106406 bytes, SHA-256
`304b300b96050fdd4e2e3b97b1587de489d251fc6d81ad4d78929316f238d499`.
It has 495 cues in 56 movie sections. All 56 stems match the local YR
`movmd03.mix`: 20 fullscreen movies and 36 radar movies. The current overlay
is attached only to the shared fullscreen movie presentation. `subtitle.txt`
is for the base RA2 game: its overlapping movie names must not be substituted
for YR's `subtitlemd.txt`.

Local bundle paths:

- macOS: `王二火大.app/Contents/Resources/subtitlemd.txt`
- iOS: `王二火大.app/subtitlemd.txt`, next to the executable

Captures and non-Apple clients retain the original movie-only rendering.
Missing/invalid optional sidecars leave movie playback intact.

Timing follows the inspected original `gamemd.exe`, rather than an independent
wall clock. `6CA5A0` reads `HH:MM:SS:centiseconds` into 60 Hz ticks, quantizing
the last field with `floor(centiseconds * 60 / 100)`. `6C9900` initializes cue
duration to 1200 ticks; `6CA680` overwrites it only for a positive authored
duration. `6C9C60` selects the latest cue whose start has passed and clears it
at start plus duration. This makes an authored zero duration last up to
20 seconds, replaced sooner by the next cue. The overlay derives that same
tick value from the displayed Bink frame and its exact FPS ratio.
