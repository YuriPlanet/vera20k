# Raised ClearTile preview colors

This additive mode shares the existing `tmp_clear_render` physical archive,
theater reader, PaletteReader VM, native TMP constructors and full lazy file
loader. It contains no new VM, parser, TMP decoder, interpolation, brightness or
packed-color implementation.

```sh
python -m tools.spatial_oracle.tmp_clear_render --preview --write
python -m tools.spatial_oracle.tmp_clear_render --preview --check
python -m tools.spatial_oracle.tmp_clear_render --check
```

The last command checks the unchanged ordinary ClearTile raster payload. Its
historical `bridge_shadow_render --tile --check` entry also remains available.
The preview reference is consumed independently by the production map-preview
and shared numeric owners; a successful native check alone establishes no Rust
test, thumbnail surface, GPU or complete RMG result.

Original General/theater ordinal publication and TileSet0000 readers resolve
ClearTile0, TilesInSet1 and FileNameClear from physical TEMPERATMD.INI. Original
5447C0 constructors register the descriptor and 547020 lazy loading binds actual
`ra2.mix/isotemp.mix/clear01.tem` bytes. The payload retains immutable source
identities, original file IO and the native registered head/header join. The
physical file is 1×1, with RGB pair `[162,168,80]` / `[163,167,80]`. The eight
contiguous family links/counts are the existing supplied registration boundary;
this is not the complete native theater-loader or arbitrary archive-precedence
proof. The native warm selector chooses the base file at recorded coordinates;
the mode never substitutes a selector answer or rewrites the selector table.

Full original TMP preview producer **549E90** rebuilds the table. Its present
subtile allocates 0x34 bytes: 13 levels of two packed words. Original **661190**
scales each physical RGB by float bits `3fb33333` (the native 1.4 constant), and
**661020** interpolates toward those brightened RGBs. The producer computes its
fraction with original FILD/multiply-by-double/FSTP-float instructions before
calling interpolation. The actual VM x87 control word and multiplier bits are
recorded. Every expected RGB and packed word comes from original instructions.

`NativeCallTrace` observes the real 661020 return PC and cleaned stack pointer.
The payload retains the returned three RGB bytes for both left/right calls,
the original level register and actual float fraction. `rows[].output` exposes
`preview_rgb_pair` before packing, `fraction_f32_bits`, original **549E50**
`lookup_words`, and complete selected **Cell47BDB0** `cell_packed` output.
Cell's first lookup word is the high16 half; the second is low16. Its only
immediate caller 641308 in thumbnail641140 draws that first/left half before
the second/right half. This mode does not execute the thumbnail surface writer.

The selected Cell has supplied tile0/subtile0, each level0..12, flags0,
overlay-1, empty ground FirstObject and bypass-building0. Full Cell47BDB0 executes
its no-Building/no-bridge path, original 4814F0 warm selection, 544E00 file walk
and 549E50 lookup. No Cell return is supplied. Building/remap, bridge colors,
overlay/Terrain overrides in the outer preview, damaged variants, negative or
out-of-range levels, whole generated map population and thumbnail resizing are
outside this selected component.

RGB565 is the existing **conditional enrolled device format**, not an intrinsic
native color convention. Original **4BA9CD..4BAA89** derives all six shift/loss
globals from declared descriptor masks F800/07E0/001F. Full DirectDraw descriptor
negotiation is not executed. The table producer performs the actual packing
using those globals; no Python packing formula supplies expected words. RGB555
and other device formats are not claimed by this payload.

Six direct 661190 controls cover physical scaling, upper clamp, factors1.0 and
0.8 (the existing radar caller's scale), half-value truncation and negative
clamp. Six direct 661020 controls cover physical RGBs,
half interpolation, adjacent float fractions around one-third, and extrapolated
lower/upper clamps. These are declared numeric inputs, independently executed
through the original full functions. In particular, physical blue80 scales to
111; producer level12 uses fraction `3f7fffff`, so its returned RGB is one step
below the brightened target. A host `level / 12.0` or endpoint shortcut is not
the retained native result.

Five Particle controls additionally execute original **62BAC5..62BB06**, from
the declared returned RandomRanged sample through FILD, multiplication by the
original double at7E3570, FSTP-float, full661020 and actual Particle+B0 stores.
This is the post-draw numeric prefix, not a complete Particle constructor or a
replacement RNG call. Inputs include zero/one/mid/upper and the retained first
Welding child: sample19015573, StartColor1 `[80,255,255]`, StartColor2
`[255,255,100]`, output `[81,255,253]`. That last input/result originated in the
existing native `welding.json` live_case first produced particle and boundary0
rng_requests6 (caller62BAC5); this mode independently executes its numeric
prefix. Its exact narrowed fraction is recorded, rather than treating the
double sample scale as661020's float argument. The shared numeric helpers
themselves draw no RNG.

The existing preparer executes original Main seed0 and 128 draws to initialize
the 64-entry selector table. Preview begins at that warm continuation; Scenario
and MapGen receive separately declared original seeds0. Complete 0x3F4-byte
states for all three are stored before and after producer, lookup, Cell and
numeric controls, with no reached random calls or state changes. This does not
certify whole-process/world RNG startup. The selected preview paths write no
gameplay timer or detach. Rebuilding the table calls its original frees/vector
reset/allocations; existing bounded allocator/free/TLS/CRT and physical RawFile
Windows transport remain storage/IO services rather than claimed OS lifetime.

The sidecar pins the replay command. Source hashes,
validation logs, disassembly and screenshots are not part of the corpus.
