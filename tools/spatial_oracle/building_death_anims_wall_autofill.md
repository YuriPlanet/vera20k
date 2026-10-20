# GAWALL autofill lifecycle component

`building_death_anims.py --wall-autofill` is an additive native comparison
component. Its JSON is generated
only into an explicitly selected external scratch directory. The compact sibling
JSON and metadata are the installed controls consumed by
`sim::world::wall_autofill_oracle_tests`; replay into scratch and compare before
replacing them. Existing modes and corpus paths retain their defaults.

Original whole Map autofill `588750..588952` calls BuildingType `45E880`, whole
Building constructor `43B740` and Building Unlimbo `440580` for each gap. The
native caller does not test the Unlimbo result. The wall arm constructs a
transient Overlay, calls radio and both `5678E0` sight passes, and UnInits the
successful Building before returning. Constructor `6F3254` draws from Scenario
and `6F3259` stores AX. These are original instructions, not Python answers.

The comparison composes the existing `joined_inputs`, `joined_fixture`,
`joined_stock_flat_map` and shared `initialize_discovery_geometry`. It adds no
VM, allocator, INI parser, RNG implementation, native algorithm or result stub.
Physical RULESMD, optional LANGRULE, Battle mode and inner Hills map sections
enter original Building/Overlay/Rules readers; fixed ART remains separate.
Original OverlayType constructors retain physical registry-name order before
the ToOverlay lookup. Native selected readbacks include GAWALL Strength,
ToOverlay/index, GuardRange, SpeedType, Sight, Insignificant and the actual
initialized/read RevealByHeight flag. TEMPERATMD's clear Morphable reader and
the existing original clear-tile constructor supply the declared flat map.
Full Overlay theater `5FE620` binds physical `GTWALL.SHP` through the existing
file/cache owner. An absent selected image fails the input admission guard;
no raster output is claimed. The source directory therefore needs both physical
`TEMPERATMD.INI` and `GTWALL.SHP` in addition to the retained joined inputs.

The clear component reuses `tmp_clear_render.theater_tile_files`: complete original theater
General/default block `545535..545C3F` and ordinal projection
`545CEF..545FA3` produce the LAT/cliff/Clear roles. Native stores initialize
unassigned roles to their sentinels; cold zero incorrectly aliases clear0 to
LAT ranges. Actual reader-produced roles are retained in the payload and
imported into the joined runtime. No literal sentinel reset, tile result or
callback answer is supplied. The runtime must retain one ClearTile0 head,
matching native Clear base0, with its original `544CB0` image getter.

The declared clear head has **no resident TMP**. Original `5471B0` executes;
its real null-image return0 branch at `5471DC` is observed when reached, along
with its getter result. This bounds a flat clear component, not physical TMP,
Hills/XMP loading or rendering parity. If a selected consumer requires resident
TMP fields, use the existing physical loader before accepting a reference.
Diagnostics retain actual `47D1E7` registers, Cell tile, registry count/slot,
head/vtable and callback slot; failures print the same state without changing it.

The map is the existing joined 32x32 Cell/House/target/MTNK component prior,
with Size16x16 and supplied class/height plane, not a native Hills load. All
selected cells are level0/slope0. Origin `(20,14)` and endpoint coordinates are
declared inputs, not Rust outputs. Full native Building creation and wall
Unlimbo establish each endpoint Overlay and owner, followed by their deferred
drain. A fresh origin wall then executes the same entry and stays pending while
whole `588750` runs, matching its immediate HousePlace seam. The final drain
includes the origin and successful fillers. These are independently created
objects, not paid Factory products. Earlier HousePlace, production,
payment/build timers, player input and whole-world scheduling are excluded.

Three intended branch controls fail closed unless actually reached:

- `no_gap`: adjacent endpoint `(20,13)`; no filler construction.
- `two_empty`: endpoint `(20,11)`; two successful native filler attempts.
- `middle_refusal`: endpoints `(20,10)` and `(22,14)`; a single declared custom
  Rough cell `(20,12)` has Buildable=yes/Track=0%, read by original `674000`.
  Only that reader-produced land-table row and Cell land prior are supplied
  before the loop. Native GAWALL SpeedType must read back -1. The scan's
  Buildable gate can then admit this cell while Overlay Mark's Track1 gate
  refuses it. The intended original results are success/refusal/success, then
  success in a later direction. These assertions admit the branch; they are
  not stored expected state. This custom input is **not a stock-map witness**.

No hook alters output fields or returns gameplay decisions. Existing joined
platform, file/cache, bump allocator/delete, COM, audio and presentation
boundaries remain explicit in metadata. Native executable sections must remain
unchanged. Unsupported transport/accesses or a different admitted path fail
rather than installing a partial corpus. No new callback, border or iterator
substitution is introduced.

Default schema2 is a pure projection after the same complete producer:

- `transitions.origin_unlimbo`, `autofill`, `drain` each contain exact observed
  `before`, `after`, `result`. Origin's input product is **already constructed**;
  its transition does not spend the constructor draw again.
- Top-level `rng_streams` interns complete0x3F4 buffers. Each state `rng`
  maps `main`, `scenario`, `mapgen` to indexes; no state is rebuilt from draws.
  Ordered requests retain native entry/site/stream address/arguments/result;
  explicit origin/autofill request ranges separate the selected transitions.
- `native_ids` maps B0/O0 etc to original IDs in constructor order, including
  endpoint setup. `origin_ref` identifies the already constructed origin.
  `objects`, constructor `state` and Unlimbo `state` index top-level
  `object_states`: AX, health/sample, passive/raw timer words, Location,
  Alive/Limbo/Marked/Logic/type/owner/radio. Constructors also select actual
  C8/CC/D0 words from their recorded body; full bodies/writes are omitted.
- Ordered registry/pending refs retain Building, Overlay, generic Object,
  Logic and House-live membership; unrelated Abstract registry is excluded.
  Preexisting identities remain `prior:<native_id>`; unknown values stay raw.
- `semantic_calls` covers origin Unlimbo, whole autofill and final drain.
  Endpoint constructor/Unlimbo observations remain separately retained.
  Rows follow `semantic_call_fields`: kind index, phase index, typed receiver,
  native return PC, raw result, then optional observed fields if present.
  `semantic_call_kinds` and `phases` supply labels/PCs. Actual sight XYZ replaces
  only its coordinate pointer; House/type/object pointers become typed refs.
  Raw results remain unchanged, with `result_ref` when the identity is known.
- `patch_coordinates` is the observed changed-cell union at retained states,
  plus origin/endpoints. State `patch` indexes top-level `patch_values` rows
  ordered by `patch_fields`. This selects observed values, without calculating
  neighbor, owner, connection, damage or visibility answers.
- House counts/credits/dirty fields/writes, raw `[8333BC]`/`[880994]` priors and
  ScenarioInit are retained. No semantics are invented for those two globals.

Physical identities, selected GAWALL layers/ART, Overlay registry and native
General roles remain. `--autofill-detail` instead writes an external-only
`building_death_anims_wall_autofill_detail.json` with all original observations;
it does not change producer execution. Detailed bodies/field writes/repeated
snapshots are not installed in the repository. `observation_gaps` must be empty
for a new accepted projection. Freed-arena readbacks are historical bytes, not
live objects. Callback/reentrancy/capacity/shutdown and whole-map claims remain
outside this component.

After author freeze and explicit execution authorization, run from the checkout
with the existing physical extracted input directory (including TEMPERATMD):

```sh
VERA20K_GAMEMD_EXE=/path/to/gamemd.exe \
VERA20K_BUILDING_DEATH_ASSETS=/path/to/retained-joined-extract \
python -m tools.spatial_oracle.building_death_anims --wall-autofill \
  --autofill-output-dir /absolute/external/scratch/autofill --write
```

An independent identical invocation with `--check` is owed after native results
are reviewed. `--output` and a scratch path inside this checkout are rejected.
Only a later explicitly reviewed, consumed subset may become an installed
reference. Default, `--joined`, `--constructed-cancellation` and
`--joined-stock-smudges` checks, plus the Python suite, remain owed on the final
candidate; static syntax parsing does not demonstrate unchanged output.
