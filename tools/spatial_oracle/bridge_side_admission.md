# Bridge side-cell admission

The original bridge constructor produces structural side cells that have no
overlay of their own. This corpus executes six Walk/Infantry admission controls
and two radar branch controls against that native-produced topology. It
establishes that these consumers use raw structural bit `0x100`, with the
appropriate height and occupancy plane, rather than requiring a local bridge
sprite overlay.

Canonical payload SHA-256:
`1cd1a816908f017e1de7e761f16ec330401408ad1b0a15bb2ad7bd5493d5987c`.
The `.meta.json` records the native version, supplied state, boundaries and
entry points. The output records actual native return classes and instructions
that read candidate fields. No admission return is supplied or substituted.

```sh
# Directory containing the applicable extracted retail layers:
export VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted/retail/inputs
python -m tools.spatial_oracle.bridge_side_admission --check
# Explicit regeneration after reviewing changes:
python -m tools.spatial_oracle.bridge_side_admission --write
```

Default invocation checks. `VERA20K_GAMEMD_EXE` or `RA2_DIR` selects the pinned
binary through the [shared native runner](../native_oracle.md). The asset
directory must contain `RULESMD.INI`; the fixture processes `LANGRULE.INI` when
present, then `MPBattleMD.ini` and `Hills.map`. The checked extraction has the
latter two files and no `LANGRULE.INI`. Native land-speed reader `0x00674000`
reads the physical lexical strings; its setup uses the existing bounded INI
cache/allocator fixture, not the full archive loader. All helper imports are
repository-local.

## Constructor and supplied query state

Each regeneration executes original Overlay constructor `0x005FC380` using the
existing [bridge constructor fixture](bridge_constructor.py). Overlay 25 at
`(16,16)` produces side `(15,16)` with flags `0x11300`, overlay -1, state 9,
anchor `(16,16)`, ground level 6 and slope 0. Source `(14,16)` has flags
`0x11100`, overlay -1, state 9 and the same anchor and level. All constructor
topology fields are checked against its independently saved constructor corpus;
the reference file digest is recorded in `constructor_reference`.

Those resulting topology fields are projected into separate query machines.
This is a boundary between native-produced cell data and supplied runtime
state, not execution of the entire constructor-to-movement lifetime. The
ordinary Infantry receiver uses original vtable `0x007EB058`, supplied Foot
SpeedType 0 and Move mission 2, with no Team, slave, object-list members or
tubes. Raw occupation owner indices are -1. Source physical position is
`(3712,4224,1040)` on deck or `(3712,4224,624)` on ground. Candidate is `(15,16)`
and retained direction is east, 2. The unused fixture hut is not a target or
list member. Land, level, slope, flags, state and anchor come from the native
constructor result; the scalar actor/occupation inputs are supplied explicitly.

Original cell-direction initializer `0x0049F2F0`, lepton-direction initializer
`0x0049F3A0` and Walk constructor `0x0075AA90` execute. The measured interior
caller `0x0075B59C` produces prospective coordinates and all five query
arguments, including physical height through `0x005F5F00` and original
`Object::GetCell @ 0x005F6960`. Concrete Infantry `0x0051BF90` and its Foot
height gate `0x004D9C60` run unchanged. Previous Cell is null and the fifth
argument is 1. Execution stops before head selection or the first refusal
callback.

## Executed admission results

| Case | Candidate flags | Produced height | Ground/deck occupation | Native class |
| --- | --- | --- | --- | --- |
| `intact_side_deck` | `0x11300` | 10 | 0 / 0 | 0, reaches head selection |
| `intact_side_deck_ground_occupied` | `0x11300` | 10 | `0x20` / 0 | 0, reaches head selection |
| `intact_side_deck_deck_occupied` | `0x11300` | 10 | 0 / `0x20` | 2, refusal response |
| `side_raw100_clear_deck_height` | `0x11200` | 10 | 0 / 0 | 7, height refusal |
| `intact_side_ground_deck_occupied` | `0x11300` | 6 | 0 / `0x20` | 0, reaches head selection |
| `side_raw100_clear_ground_height` | `0x11200` | 6 | 0 / 0 | 0, reaches head selection |

The raw-100-clear controls change only the candidate structural bit and retain
state 9, overlay -1 and the anchor. They isolate the consumer; they do not
execute a collapse or claim that all collapsed cells retain those other fields.

Infantry `0x0051BFA2` identifies the deck list using raw `0x100` and the
height-to-ground difference. At `0x0051C104..0x0051C136`, raw `0x100` and
height equal to signed ground level plus 4 select upper occupation and owner
fields. The executed deck cases reach `0x0051C237` (upper object list); ground
cases reach `0x0051C243`. Own overlay is read later at `0x0051C17C`; -1 skips
the optional overlay checks. Candidate overlay state is not read in these
admission cases.

## Radar branch

Original `Cell::GetRadarColors @ 0x0047C060` first executes the empty-ground
Building lookup `0x0047C4D0`. The structural test at `0x0047C0AE` precedes the
own-overlay lookup. For side flags `0x11300`, overlay -1 and state 9, the
original code selects `OverlayTypes[24]`, calls `0x005FED00` with frame 0, and
the harness stops at that original color-function entry. Clearing raw `0x100`
while retaining overlay -1 and state 9 instead reaches ground TMP branch
`0x0047C24A`.

The harness supplies the OverlayTypes[24] identity and minimal tile-table
storage needed to observe these branches. It neither supplies a color-function
return nor computes a color. SHP palette sampling, actual colors, raster output
and arbitrary overlay types are outside this comparison. The admission and
radar traces reach no RNG entry. Original code bytes remain unchanged.

## Validation and bounds

The exploratory corpus passed independent `--write` then `--check` before
promotion. The promoted payload is byte-identical and independently checks
through repository imports. Constructor topology, original query/admission
results and the radar branch are native-established within these supplied
states. Full pathfinding, paid movement, collapse/repair lifecycle, constructor
ordering with live movers, object occupancy callbacks, persistence and rendered
parity remain outside this corpus. Production and Rust comparisons belong to
the affected consumers' validation.
