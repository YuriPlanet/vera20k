# Ordinary static Terrain first-reveal drawing

`terrain_render.py --shroud-admission` executes the original ordinary static
Ground display scan `6D97D0`, Terrain Render `71CC50`, native frame rectangle
`71D160`/`69E7E0`, retained-coordinate projection and full DrawIt `71C1B0`.
The existing Terrain renderer oracle owns this extension; its historical
`terrain_render.json` payload is unchanged.

The executed image is the official Steam `gamemd.exe`, SHA-256
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.

```powershell
$env:RA2_DIR = 'D:/steam/steamapps/common/Command & Conquer Red Alert II'
python -B -m tools.spatial_oracle.terrain_render --shroud-admission --check
```

[terrain_reveal.json](terrain_reveal.json) preserves all draw arguments and
reached native landmarks; [the metadata](terrain_reveal.meta.json) records
binary and source identity, prepared inputs and the terminal substitution.

## Result and active callers

Twelve contrasts cross Cell `+12C` flags `0x00/0x08/0x10/0x18` with Cell `+130`
counter `0/1/255`. Each executes both body and shadow with identical arguments.
A native memory-read hook sees no access to these visibility fields throughout
the complete bounded static scan. Seven controls produce no draw: absent
Ground registration, dead, unmarked, limbo, animated, crumbling and offscreen.
The animated and crumbling controls are rejected by the static scan because they use
a different dynamic rendering route; this evidence does not establish their
complete rendering behavior.

The active caller chain, confirmed through Ghidra and original instructions, is:

- Tactical Draw `6D3D10` calls `6D3AC0` at `6D44D3`.
- `6D3AC0` calls the static Ground scan `6D97D0` at `6D3BB9`, `6D3C8E`,
  `6D3CB3`, `6D3CDA` and `6D3CF7`. Its full-redraw call at `6D3CB3`
  supplies the complete viewport rather than an explored-cell filter.
- `6D97D0` walks the Ground Display array `8A0394`/`8A03A0` backwards.
  Terrain vtable `7F522C+2C` dispatches `71D300`, which returns `0x24`.
  The `+44` query is `5F6690`: it returns whether Object `+90` equals zero.
  Object constructor `5F3930` sets that byte to one; UnInit `5F6625`
  clears it. This is liveness, not exploration.
- The scan rejects TerrainType `+2B3` animated and Terrain `+CD` crumbling
  states, then checks the native render rectangle. At `6D98E9` it calls
  vtable `+104 = 71CC50`; that function projects retained coordinates and
  calls `+114 = 71C1B0` at `71CD7B`. Body and shadow call the recorded
  `CC_Draw_Shape` terminal at `71C304` and `71C34E`.

The dynamic Terrain branch `6D9224` is not the ordinary static-tree route.
Its apparent shroud call `6D929F -> 5865E0` reaches the original stub
`xor al,al; ret 4` (bytes `32 c0 c2 04 00`), which never rejects. Terrain
`+CD` is initialized to zero at `71BBE0`/`71BE3C` and written to one by
collapse/death paths such as `71C6F8`; it is not a first-exploration flag.

This establishes that the original ordinary static drawing chain has no
whole-object anchor-cell exploration gate. The previous comment in
`src/app/presentation/instances/overlays.rs` asserting such a gate was not
supported by the historical oracle: that corpus enters at `71CD22`, after
admission, and explicitly excludes admission from its coverage.

## Boundaries

The scan, liveness predicates, native rectangle derivation, Render and DrawIt
execute without instruction patches. Only `CC_Draw_Shape 4AED70` is a
recording terminal with its original 56-byte cleanup. A synthetic physical
84x56, four-frame SHP header and body/shadow rectangles are supplied; actual
retail asset binding and raster pixels are not executed. Map/object/type,
camera, display registration and initial visibility states are prepared.
The retained screen-coordinate cache is seeded from original projection;
its lifetime/update producer is not reproduced. The parent Tactical callers
were read, not executed by this fixture.

These results establish ordinary drawing admission and native draw arguments.
They do not certify native ABuffer/ZBuffer pixels, complete scene parity,
arbitrary slopes or bridges, full FogOfWar/gap behavior, animation/death
lifecycle, or native scenario loading. Rust production integration and GPU
capture validation are recorded by the repair owner.

## Rust integration

The local scenery repair evidence links
production before/after retail captures, regression checks and the saved
identity receipt. Those observations are Rust integration evidence, not native
whole-scene pixel parity.
