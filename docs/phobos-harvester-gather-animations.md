# Optional Phobos custom gather animations

Build with `--features phobos` to read Phobos `UnitTypeExt`'s three
`OreGathering.*` keys on a `VehicleTypes` entry. Default builds draw the literal
`OREGATH.SHP` with its 15 frames per facing, exactly as the original does.

In `rulesmd.ini`:

```ini
[HARV]                             ; VehicleType
OreGathering.Anims=                ; Animation list
OreGathering.FramesPerDir=15       ; integer list
OreGathering.Tiberiums=0           ; integer list, [Tiberiums] indices
```

`UnitTypeExt::LoadFromINI` reads the three vectors in that order at the end of
the UnitType body. Each `ValueableVector::Read` replaces its whole list when the
key is present and retains it when the key is absent, so a later Rules, mode or
map layer either swaps a list out completely or leaves it alone. `Anims=`
resolves each comma-separated token against the registered `AnimType` array and
drops a token that array does not hold; `Tiberiums=` and `FramesPerDir=` scan
each token as an `Parser<int>` value and drop the ones that do not convert.

`UnitClass_DrawIt_OreGath @ 0x73D223` replaces the gather overlay's draw point
inside `UnitClass::DrawIt @ 0x0073CEC0`. The overlay appears under the same
gates as the stock one — a `Harvester=` type, its harvest latch raised, its
locomotor not moving now, and no temporal warp in flight. Then:

- the harvester's own cell answers `CellClass::GetContainedTiberiumIndex @
  0x00485010`; a cell holding no resource takes the stock draw;
- `OreGathering.Tiberiums=` is searched for that index, and an empty list means
  slot 0 for every resource;
- the slot selects `OreGathering.Anims=`'s animation image and
  `OreGathering.FramesPerDir=`'s count, defaulting to `OREGATH.SHP` and 15 when
  the respective list is empty;
- the frame is `frames * facingRow + (Foot+0x538 + CurrentFrame) % frames`, the
  same row and signed remainder the original computes at `0x0073D1E2..0x0073D273`
  with `frames` in place of the literal 15.

The lists are parallel by index, and Phobos keeps no length relation between
them: a `Tiberiums=` longer than `Anims=` indexes past the animation vector, and
a `FramesPerDir=` shorter than the slot it is asked for indexes past itself.
VERA keeps the stock image and the stock 15 frames for any slot its list does
not hold, and for a frame count that is not positive, where the native `%`
faults.

The sprite atlas preloads the registered AnimType identities from the rules,
not from harvesters currently on the map. The existing animation image owner
resolves each type's loaded Shape, including `Image=`, theater filenames and
retained load requests. The hook selects a raw SHP frame, so every physical
frame remains available even when the image exceeds 120 frames or has shadows.

Gather drawing uses `anim.pal` and the unit's gather point without the
AnimType's `XDrawOffset` / `YDrawOffset`; ordinary AnimClass drawing keeps its
own placement and palette context. Phobos can select `CustomPalette=` through
AnimTypeExt's palette; that separate custom-animation-palette mechanism is not
implemented here, so an authored custom palette remains a residual.

RESIDUALS (trigger, effect, frequency): a missing parallel-list slot or a
non-positive frame count keeps the stock fallback described above; an authored
`CustomPalette=` keeps `anim.pal` instead. Only custom data reaches these cases.

This feature does not load Phobos DLLs or establish complete mod compatibility.
