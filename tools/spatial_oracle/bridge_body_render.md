# High bridge body caller and raster reference

The body mode of the existing
[`bridge_shadow_render.py`](bridge_shadow_render.py) executes original
`CellClass::DrawOverlay_Body` (`0x47F6A0`) through physical stock bridge artwork,
the native Cell LightConvert, shape clipping, selector, row walker and RGB565/Z
pixel stores. It shares the shadow oracle's archive/lexical preparation, VM,
surface setup, caller execution and output-run owner.

```sh
python -m tools.spatial_oracle.bridge_shadow_render --body --check
python -m tools.spatial_oracle.bridge_shadow_render --body --write
```

The default remains the existing shadow corpus. `--body` selects
[`bridge_body_render.json`](bridge_body_render.json) and its
[sidecar](bridge_body_render.meta.json). Both modes default to checking without
writing. The sidecar pins the original executable and replay command, without
repository-source hashes. Native setup is described in
[`../native_oracle.md`](../native_oracle.md).

## Inputs and converter ownership

The existing archive owner supplies physical `RULESMD.INI`, `MPBattleMD.ini`,
`ARTMD.INI`, the `BRIDGE`/`BRIDGB` theater images and palettes. Original
OverlayType constructors and full rules readers bind all four high bridge types;
the exact physical source identities and selected lexical layers are retained in
`inputs`. This executes the selected component readers, not a full map/scenario
loader or arbitrary archive/layer precedence.

Body mode additionally reads physical `ISOTEM.PAL` and `TEMPERAT.PAL`. Native
`0x5454DB..0x5454EB` expands their bytes in place. The original default theater
converter initializer `0x545000..0x5450EF` constructs and publishes a
LightConvert through `0x555DA0`, `0x556090` and `0x48EBF0`; its source and
destination pointers must be `0xABBED0` and `0x885780` respectively. The original
`CellClass::InitLightConvert` (`0x483E30`) then binds that converter and writes
the supplied unity lighting scalars. This is a Cell LightConvert, rather than
the unrelated `PALETTE.PAL` ordinary Convert retained by the shadow fixture.

RGB565 format and MMX dispatch are explicit runtime inputs established by the
existing palette owner. The native constructor must reach `0x7DE200`, retain
53 shade rows and `(1000,1000,1000)`, and produce a table byte-identical to
independent original execution through `tools.palette_oracle.oracle` with the
same expanded physical ISO palette. No table or expected pixel is injected.
Full scenario lighting, alternate light-controller history and other theater
body pixel equivalence remain outside this corpus.

## Coverage and output

`rows` retains the same input/draw/rectangle/output schema as the shadow
reference. Eight raster controls cover all four stock type identities, both
orientations, healthy/damaged/partial states, strict near/equal/far Z admission
and an actual second draw. The first four retain the complete native cropped
body rectangle. Depth controls clip to one scanline to stay small; their old-Z
candidate comes from an observed original probe raster, never a Rust formula.
Repeated draws explicitly invalidate the Cell's redundant-draw cache before
each call, so the second invocation reaches the native raster rather than
returning from its cache gate. Its two snapshots remain available in `steps`.

The body uses original flags `0x4E00`, the Cell converter and brightness,
gradient0, a null remap and no caller-supplied Z shape. Shape and row-walker
instructions choose the original extended opaque leaf `0x4990E0`. That leaf
compares the signed candidate with the zero-extended stored u16 Z using strict
`<` (`0x4991EA..0x4991EC`), then stores the accepted candidate's low16 bits.
The reference records actual final RGB565 and Z runs; unchanged background/Z
pixels are omitted losslessly.

`controls` contains 41 caller-boundary records: every raw state byte0..17,
signed levels-1/2/6, independent flag inputs0/0x80/0x100/0x400, and all16
modulo4 coordinate positions of the raw-state0 LatinSquare lookup. These stop
**before** `CC_Draw_Shape` and carry no pixel claim. Their purpose is to keep
raw0/9-only Latin jitter, direct remaining bytes, the0x80 branch gate and
signed-level arithmetic distinct from a simulation damage-state interpretation.
Nonstandard flag combinations and signed levels are supplied controls; a
naturally occurring stock producer for each is not asserted.

`scenes` adds one ordinary adjacent-anchor body composition. It reuses
`bridge_constructor.OriginalBridgeConstructor` for two original constructor/
Mark/setter calls, then transports their produced overlay/state/flag facts to
the same body raster VM. The producer's existing sparse map/type ordinal25/
resident synthetic Road TMP boundary and supplied level1 are recorded explicitly;
it is not a native retail map loader. The existing original Tactical traversal
suffix supplies the selected cells' call points and their order with a declared
view origin. Native body calls then execute in that observed order while retaining
the same color and depth buffers. The observed per-pixel tie certificate records
different source colors at equal Z and verifies the original first body survives.
No Rust projector, hand-sorted native order or expected-composition implementation
produces the reference pixels. Production tests must retain that order when moving
the body to the strict shared SHP path.

The live Cell fields, projected z0 point, viewport, clip, A127, background and
old Z are explicit component inputs. No complete frame, visible play scene,
gameplay lifecycle or GPU parity follows from the native corpus alone. Production
tests must consume it through their actual source loader, live-cell builder and
renderer. The ordinary0x80-clear fallback and FoggedObjectClass's `0x4D1AB5`
caller remain separate open branches of the wider Cell body mechanism.
