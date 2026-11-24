# Ares PCX sidebar cameo controls

[`pcx_cameos.py`](pcx_cameos.py) executes the original Ares 3.0 filename
reader and surface selectors, plus gamemd's palette conversion and packed-word
blit. [`pcx_cameos.json`](pcx_cameos.json) contains 23 controls;
[`pcx_cameos.meta.json`](pcx_cameos.meta.json) records the Ares DLL identity,
substitutions, execution bounds and the canonical payload hash. Inputs contain
only synthetic names, object shells, palette colors and pixels. No mod data,
images, executable bytes or local installation paths are distributed.

Set `VERA20K_ARES_DLL` and `VERA20K_GAMEMD_EXE` to the enrolled local binaries,
then run from the checkout:

```sh
python -m tools.sidebar_oracle.pcx_cameos --check
```

`--write` deliberately regenerates the corpus. Explicit `--ares-dll` and
`--gamemd-exe` arguments are also accepted. The Ares address fixture requires
the DLL identity recorded in the sidecar. The supporting executable supplies
the original palette and blit bodies; their observed outputs must reproduce
the recorded controls. The public corpus does not record its binary hash.
Mapping and bounded execution belong to `tools.native_oracle`; this producer
adds no PCX file decoder or independent PE mapper.

## Established readers and ownership

Instruction reading of the enrolled DLL establishes these exact keys:

| Consumer | Source and section | Reader call | Extension field / surface |
| --- | --- | --- | --- |
| Ordinary object | ART INI, TechnoType `ImageFile` at `+0x1F8` | `1003E81F..1003E83B`, `CameoPCX` | `+0x208` / `+0x228` |
| Promoted object | Same ART image section | `1003E840..1003E856`, `AltCameoPCX` | `+0x22C` / `+0x24C` |
| Superweapon | Rules SW type section | `10033B50..10033B66`, `SidebarPCX` | `+0x4B0` / `+0x4D0` |

The ART reads use the rules image section directly, without following that ART
section's own `Image=` or retrying the type's section. Each extension owns its
chosen surface; filename equality does not identify two types' fallback images.
The [Ares PCX documentation](https://ares-developers.github.io/Ares-docs/new/pcxcameos.html)
independently describes these file/section assignments and the filename including
its extension. Public source is a lead; the enrolled DLL establishes these addresses.

Nine executed `readers` controls establish the shared `100762C0` behavior.
Its `10006010` caller requests an empty default and a 2048-byte buffer from
original `528A10`. The supplied ReadString service returns an already parsed
ASCII value; it does not parse physical INI bytes. Original bounded-copy CRT
`100039A0 -> 1007B2E0` truncates the filename to 31 bytes plus terminator.
Original `1007F1D7` lowercases it in an explicitly supplied C locale.

Missing and empty returns preserve both the previous filename and surface.
Present values replace the filename and clear/re-resolve the surface. `none`
and `<none>` are attempted filenames, not special clear tokens. No extension
is appended. Cache hit, successful load/relookup and failed load/log branches
execute the original reader; cache/file success and logging are declared services.
The fixture's previous state is supplied, not constructed by a full native type
constructor. Constructor read-only evidence includes normal/promoted empty-name
and null-surface stores at `1003CA3B..1003CA4D` and SW stores at
`1003197E..10031984`; these constructor spans are not execution controls here.

## Selection, transparency and dimensions

Eight `selectors` controls execute `1005E430`: an eligible promoted surface is
chosen only when non-null; otherwise the normal PCX surface is selected. The
elite eligibility predicate itself is supplied. Two `superweapons` controls
execute `1005E410` with a supplied ExtMap result. `1005E3E0` skips the original
SHP branch exactly when the selected surface is non-null. Consequently a
successfully loaded all-transparent PCX remains selected, even though it draws
no opaque pixels. Missing or failed PCX can fall back to the existing SHP path.

The `palette` row executes the original 256-entry `6B9DAC..6B9E15` conversion
loop with supplied RGB565 component losses/shifts. Six colors are recorded and
the other 250 entries are supplied black. The two colors `[255,0,255]` and
`[248,3,248]` both produce word `0xF81F`; `[255,4,255]` produces a different
word. The production decoder must use the embedded PCX palette; transparency
is determined after packing, rather than by palette index or full RGB equality.

Three `blits` execute the whole original `6BA580` routine. Only BSurface
Lock/Unlock/Pitch/Width/Height interfaces are supplied. Every key comparison,
destination offset, row stride and store executes original instructions.
Black word zero is opaque; key word `0xF81F` preserves the destination; a
fully keyed source still returns success. The two-row control supplies a
60x48 destination rectangle with a 2x2 source and proves this routine uses
source dimensions rather than that rectangle's width/height for its loops.

The documented common format is 256-color 60x48. These controls do not establish
the complete original PCX decoder, malformed inputs, oversized surfaces,
24-bit conversion, clipping, device presentation, House promotion calculation
or whole-game parity. Reusing synthetic converted words is not evidence that
a Rust decoder has parsed real PCX bytes correctly. Rust regressions should
separately pass synthetic PCX bytes through the existing production decoder,
reader, atlas and view paths, including valid transparent icons, promoted
fallback and two SWs sharing an invalid filename with distinct SHP fallbacks.
No RNG draw, simulation timer write or detach call occurs in the bounded bodies.
