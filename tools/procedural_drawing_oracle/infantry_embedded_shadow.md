# Prepared Infantry embedded shadows

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.procedural_drawing_oracle.infantry_embedded_shadow --check
```

Configure `VERA20K_GAMEMD_EXE` or `RA2_DIR` using the shared
[native setup](../native_oracle.md). `--write` regenerates both the JSON and its
metadata from original execution. No game assets are written into this corpus.

The existing [GI owner](infantry_berserk_body.md) runs a prepared whole
`Infantry518F90 -> Foot41C090 -> Techno705E00` draw. Stock GI paired frames are
empty, so this control changes one input frame to an authored 4×2 raw stencil.
The native image decoder, body, shadow dispatcher and blitter remain original.
The resulting `ordinary` row records body frame 0, a later embedded shadow and
the pixels changed after the body. `no_shadow` changes only the retained
`NoShadow` byte and records a body without that later shadow.

Both rows start with the same CPU/FPU context, body point, caller clip and
Surface/A/Z planes. They verify unchanged input image bytes, original code,
A/Z planes and all three complete RNG buffers, with no observed RNG calls.
The script temporarily substitutes the existing input catalog and invocation
observer, restores them on every exit, and stops that owner before its unrelated
viewport controls. Run it as its own single-threaded oracle entry point.

This establishes the selected native ground Infantry dispatch and a positive
shadow-store control. The small pixel crop is not a stock image comparison,
GPU comparison, whole-scene equivalence claim or coverage of airborne Infantry,
bridges, alternate stances, special visual states and other unit classes.
