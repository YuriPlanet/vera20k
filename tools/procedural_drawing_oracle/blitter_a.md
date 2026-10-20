# Per-blitter tactical A prerequisite

`blitter_a.py` executes original active-retail `gamemd.exe` (identity in the
metadata) through the existing native image/palette owners. Run:

```sh
python -m tools.procedural_drawing_oracle.blitter_a --check
```

The 144 rows contain body and shadow outputs over an existing raw RGB565
surface word. Every row has one admitted source pixel, one actual compressed
zero run, and one rejected Z pixel. Inputs include A0/1/2/63/126/127/128/254/255,
N1/N27/N53, plain Convert and MMX LightConvert, ColorScheme index240, and
brightness1000/1500. Palette/LUT tables come from original `004BBB00`,
`00556090` and `00420196`; the selected pixel bodies execute unchanged.
The synthetic palette, row seed, A and Z inputs are stated fixture boundaries.

Instruction/caller evidence establishes the common stock-building route:

- Building43D290 body43D85F and bib43D8E9/43D9C9 call Techno705E00.
  Its Z/depth-write/extended flags select Convert490E50's `+158` blitter,
  vtable7E53A0, leaf4990E0.
- 4991DC..4991EC applies signed shape subtraction and strict Z admission;
  4991F4 reads A; 4991FF selects the LUT entry; 49921A reads the palette;
  499222 stores color and 499230 stores Z. Compressed zero runs skip both.
- Ordinary493DF0 reads A at493E52, and494B60 reads it at494BDD after its
  strict depth test. These source-color operations do not multiply the
  destination after drawing.
- Shadow497390 reads the existing destination, shifts it at49742C and masks
  it at49742F before storing color/Z. It does not read A.
- Building6D9920/43CEA0/43D290 paints base color/Z before Tactical6D4648
  rally pass0. Display6D916C then passes1 to43CEA0 and draws only43DA80's
  dynamic voxel turret, without replaying the base. Mobile bodies and
  independently registered Anim slots still follow the first rally.
  Native4C0750 stores raw packed color when its A admission passes. A partial
  value shades an overlapping later body while preserving uncovered line
  pixels. A framebuffer multiply cannot implement that ordering.

`ShroudBuffer` remains the sole source of CPU and GPU A bytes. Its retained
texture view identifies the derived Batch binding cache; resize/map replacement
rebinds it, while camera/fog updates write pixels and the source origin in place.
The cache also keys the viewer and invalidates on restored fog state. GPU
`textureLoad` addresses `floor(fragment/zoom + uploaded_camera - source_origin)`;
it does not stretch normalized UVs. Source origin is the same floored camera
used by CPU surface admission, while the existing camera owner rounds geometry.
The constant disabled source is only for surface/UI operations and sandbox.

The shared palette resolver keeps N1, ColorScheme special indices, source-hole
and Z rules. Raw surface stores use neutral A with the same world camera (or
the UI camera for already scaled native pixels). Destination shadows keep their
own packed owner. The old `shroud_multiply.wgsl` and its pipeline are removed.

Precomposed RGBA, translucent and FX sources still lack native packed palette /
composition metadata. They retain their compatibility source shading using A,
so they no longer multiply an earlier raw surface pixel. Their blend/FX results
are a separate unresolved mechanism, not established native parity. Existing
voxel shadow alpha blending remains such a residual. SHROUD frame generation
is separately executed by `shroud.py`; this corpus supplies its A inputs.

The focused GPU tests are `render::shroud_gpu_tests::` (explicit `--ignored`):

- The production Building, Terrain and packed-shadow routes compare both
  BGRA/RGBA sRGB attachments and depth with all native rows, preserving the
  preexisting raw surface through holes and rejects.
- Batch/TMP/VXL world paths compare A at fractional positive/negative scroll
  and zoom0.75/1/1.5/2 with the authoritative CPU source and native colors.
  Source replacement, binding reuse and disabled/sandbox state are included.
- `production_a_blitter_workload_timing` measures 1000/20000 world sources at
  1280x720, two warmups and20 measured frames. It reports CPU encode, GPU pass
  timestamps when valid, and submit-to-completion wall time independently.
  Zero, sentinel and nonincreasing GPU intervals are unavailable, never free.
  Run optimized; this is synthetic source-overdraw cost, not whole-game timing.

Native fixture replay, the focused Rust/GPU checks and optimized timing passed.
The [shared validation report](README.md#measured-rendering-cost) links preserved
samples, logs and production captures. These bounded comparisons do not close
the whole drawing goal.
