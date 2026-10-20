# Shrapnel wooden bridge navigation

`navigation.py` reuses the shared `anytown_damage/navigation.py` builder and
`navigation_inputs.py` readers. It executes original code from
retail `gamemd.exe`; no Rust class, slope, height, zone ID or graph is supplied.
The selected physical map is `XShrapnel.MAP` (SHA256
`c32e412e938e1b9a0b29f89ff9bc0e6f410f75bc76701fb4acef6581fd19b438`).
Its 13,366 allocated cells span a 164-square logical plane with native stride165.

Original `Full_Init 687631..68764F` reads `[Map] Theater` with default0 through
`475870 → 528A10 → 48DBE0` and stores Scenario+1258. Physical `SNOW` produces1;
that native result is copied into the navigation VM before Terrain `71C110`
and Cell `483DDF` select snow occupation fields. This prerequisite matters:
leaving the Scenario field zero selected temperate occupation for 196 cells
and produced304 base zones. The corrected native SNOW path produces388.
That discarded temperate run is diagnostic evidence, not the published golden.

The shared reader executes the active RULESMD, absent LANGRULE, MPBattleMD and
map layers, plus ARTMD and SNOWMD readers. All250 overlay types, the24 mapped
Terrain types, all15 mapped building class gates, land costs, tiberium fields,
and the selected MTNK inputs are retained. Original Cell constructors and
initial/final Recalc sweeps surround487 original Terrain constructors and direct
Place_Down calls. Full Scenario loading, Terrain Unlimbo/Logic registration,
and the complete Rules load chronology remain outside this bounded composition.

The theater input has964 native-derived tile ordinals. Physical primary TMP
bytes come from `ra2.mix/isosnow.mix` plus88 disjoint names in
`ra2md.mix/isosnomd.mix`; those88 have no base-name overlap and none are authored
on this map. The extractor rejects ambiguous duplicate winners and checks the
selected other archives and loose root. Original tile constructors run and only
TMP pointers are relocated. Ordinal iteration, cumulative counts and FileName
selection remain explicit host-side theater-loader boundaries; the integer,
General, tile-property and object readers execute original code. Every selected
archive/file hash and numeric reader output is preserved. Proprietary source
MAP, INI, TMP and SHP bodies are not included.
The selected archives lack417 primary entries, including editor/marble slots;
none is used by an allocated cell. The216 non-sentinel tile IDs have primary
data; cells also retain the65535 sentinel. Every one of the216 reached native
primary-TMP getter heads asserts its presence.

After original `568BB0`, `56C510` and `581F90(2,1,0)`, the same VM executes:

| Stage | Native entry | Graph records, levels0/1/2 |
| --- | --- | --- |
| Authored broken | initial construction | 3587 /1545 /857 |
| First repair | `570050`, hut117,56 | 3595 /1556 /885 |
| First damage | `57BAA0`,115,59 | 3595 /1556 /885 |
| Collapse | `57BAA0`,115,59 | 3604 /1567 /913 |
| Repair again | `570050`, hut117,56 | 3612 /1578 /941 |
| Repeat repair | `570050`, hut117,56 | unchanged |

All raw base/hierarchy planes,13 movement rows, three complete ordered graphs,
physical cell facts,33 span cells, Dummy, caller traces and three complete RNG
states are saved. First damage and collapse draw no RNG in the empty-occupant
composition. The two repairs consume three MapGen draws each:1/1/2 then2/0/1.
Main/Scenario do not change during these transitions. Original startup establishes
FPCW through the existing owner; complete seed0 stream states are supplied before
initialization. The Unit constructor prefix advances Scenario once, recorded
separately. This does not establish full native scenario-load RNG chronology.

Original MTNK `73F0A0` samples all nine affected cells from a supplied alive
receiver at115,58, height2, direction4, previousCellNULL, arg5=1. The broken
center row returns7; all nine return0 after repair and first damage; collapse
restores7 on the center row. This is an actual receiver comparison, not a route,
spawn or movement-command proof. Bridge occupants and CellTags are absent.
Display/radar/shroud and bounded allocation seams are inherited and declared in
metadata. No native raster, Engineer prefix, projectile or outer damage-admission
claim follows from this packet.

For reproduction, install the shared native oracle runtime and provide retail
inputs as described in [the existing Shrapnel packet](../shrapnel_repair/README.md).
Set `VERA20K_SHRAPNEL_INPUTS` to its flat asset directory and `RA2_DIR` to the
physical game directory; that flat directory also needs `SNOWMD.INI` and ARTMD.
The archive extractor reads the installed MIX files directly. Then run from the
repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.navigation --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.navigation --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.navigation_compare /path/to/wood-export --output /path/to/live-comparison.json
```

Omitting the flag checks without writing. The promotion hash refuses changed
native values even under `--write`. The comparison consumes `.loaded`, `.healthy`,
`.damaged`, `.collapsed` and `.repaired.json` exports. It checks every class,
height, base ID, movement row, ordered graph/ID/edge, allocated live slope/height,
and every exported representative cell. Runtime objects and RNG are separate
boundaries. The new native planes and graphs also reproduce the older supplied
Shrapnel packet at all six stages, strengthening its initial-input evidence.

`setup()` returns the native readers, theater, asset receipt and full VM.
`repaired_donor(create_actor=False)` additionally runs the actual first repair
and returns its result for consumers that need this same initialized world.
Joined occupant packets must declare whether they use this full donor or the
older bounded ResidentRepair input; this packet does not change their coverage.

`navigation_restore.py` reuses `anytown_navigation_restore.py` for original
`581F50` at four independently reconstructed, exact frozen boundaries. It
clears and rebuilds levels2/1/0 and refreshes pathfinder scratch while preserving
native source planes, cells, RNG and frame. It is the post-load reconstruction
called by `LoadContent 67E8CD`, not a full SaveGame/MouseLoad or pointer-swizzle
emulation. The common source-byte evidence is
[the retained Load/MouseLoad receipt](../anytown_navigation_restore.native_bytes.json).

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.navigation_restore --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.navigation_restore --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.shrapnel_damage.navigation_restore --compare-prefix /path/to/wood-export --comparison-output /path/to/restored-comparison.json
```

The restore comparison consumes `.restored_healthy`, `.restored_damaged`,
`.restored_collapsed` and `.restored_repaired.json`. It checks complete saved
source planes and ordered rebuilt graphs, not equality with pre-save accumulated
hierarchy IDs. The existing Anytown default factories and frozen outputs remain
unchanged by the parameterization.

[The validation receipt](navigation_receipt.json) pins the independent native
write/check runs and the production export hashes. The `wood-v2` comparison
passes all five live stages, all four restored stages, and the ordinary MTNK
firing chain through damage, collapse and Engineer repair. These checks reuse
the same complete-plane and ordered-graph comparators; they do not compare
actor/RNG state across different caller boundaries. Both shared Anytown native
packets replay without changing their saved payloads.
