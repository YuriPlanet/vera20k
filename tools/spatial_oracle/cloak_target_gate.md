# SUB retained-target cloak gate

`cloak_target_gate.py` composes the existing `BulletReader` and
`bridge_target_composed.setup` owners. It executes the complete original
`TechnoClass::CanAutoCloak` at `6FBDC0..6FBF93`, including the real Unit
`SelectWeapon746CD0` / `TechnoSelectWeapon6F3330`, `GetWeapon70E140`,
`CanFireAtTarget6F7780`, `CanFireAt6F77B0` and `InRange6F7220` when reached.
The target query concerns weapon selection and range; it does not establish
`GetFireError` or full attack legality.

The ten independent controls retain a distant or near live DEST, omit the
target, reject at state2 or active/paused weapon rearm, reach the exact rearm
deadline, or reach progress/recloak only after the target query. `gate_path`
records actual gate visits. `calls` contains nested native entries and measured
returns. `can_fire_at_target` is null when the original short circuit skips it;
it is never filled from an independent or synthetic range calculation.

The native leaf has no transition, timer write or detach. The harness rejects
transition/detach entries, RNG entries, setup transports during measurement,
actor mutations and changes to native code. Each row retains all `0x3F4` bytes
of Scenario, Main and Mapgen before and after, independently initialized by
original `65C6D0` with seeds31/0/0. This is a leaf comparison, not whole-game
RNG or cloak-transition parity.

## Physical inputs and construction

Fresh original Rules `665650` and SUB/DEST UnitType `7470D0` constructors
establish defaults. Whole UnitType `747620`, selected native-discovered
Weapon `772080`, Bullet `46BEE0` and Warhead `75D3A0` readers execute on cached
physical strings from `RULESMD.INI`, optional `LANGRULE.INI`, `MPBattleMD.ini`
and the exact `VERA20K_CLOAK_MAP`, in order. Original General `66D530` executes
with only physical `CloakingStages` and `CloakDelay` keys; other General keys
are outside this component. Constructor values, each retained default argument,
physical sections/line numbers, admitted reader results and final selected
fields are exported. In particular constructor CloakingSpeed7 and
CloakDelay0 are readbacks, not defaults inferred from retail tuning.

The selected normal/elite slots0/1 determine weapon identities through native
pointers. Original factories/registries establish shared Bullet/Warhead
identities. A dependency first encountered in a later layer receives preceding
physical layers too. Complete Rules registry discovery/sweep order is supplied;
this is not whole Rules Process. Fixed ART uses selected type/Bullet names and
their physical Image strings, without parsing scalar values in Python. Visual
type/voxel asset functions are recorded setup boundaries; no rendered output or
asset closure is claimed. Existing native lexical-cache, CRC, GUID transport,
allocation, CRT and SHP owners are reused.

The inherited preparation first establishes an FV/HoverMissile range world;
its provenance remains under `inputs.inherited_setup`. C28 then constructs and
reads fresh selected types and Rules. That earlier setup is not physical
evidence for SUB/DEST. The supplied actors use original Unit vtables, actual
type-to-Ammo/Cloak/HP initialization `735592..7355C6`, constructor sentinel
`735416..73541C`, and Ship `69EC50` constructors. Two hostile human Houses,
rookie rank, actor/world admission, in-playfield/marked flags and sparse flat
water Cells are declared boundaries. Original whole Unit construction,
Unlimbo, actual selected-map terrain loading, visibility/diplomacy admission,
human orders, movement and scenario startup are not executed here.

Source Cell10,20 and target Cell14,20 or24,20 are mapped by the existing sparse
fixture. Flat level6 has groundZ624; bridge flags are zero. Each row explicitly
supplies coordinates, current frame173, target pointer, cloak state/progress,
weapon rearm and recloak timers. These component inputs do not claim production
capture state injection. Whole native range math and map queries execute on
that declared world, with the inherited x87 context recorded.

## Reproduction

Use the existing extracted input directory unchanged, including its physical
Hills inner INI and earlier FV/projectile assets needed by the inherited setup.
`Reader` reads that directory's flat files; freeze the complete directory,
original executable, selected map and source dependency closure before running.
Do not use `RA2_DIR`. `VERA20K_CLOAK_MAP` has no fallback.

The current C28 production layer is the unchanged physical map at
`~/Documents/vera20k-dev/techno-active-retail/C28-cloak-range-production-controls/discovery-v1/ordinary-sub-cloak.map`.
The consumed fixture is its byte-identical copy at
`tools/spatial_oracle/cloak_target_gate/ordinary-sub-cloak.map`, installed by
the chain owner. Its identity belongs in generated input records and scratch receipts;
generated sidecars pin the original executable and command, not repository
source hashes. This map's INI layer is used here; its packed terrain is not
decoded or loaded by this component.

From the owned checkout, after the owner authorizes a coordinated freeze:

```sh
env -u RA2_DIR PYTHONPATH=. \
  VERA20K_GAMEMD_EXE="$HOME/Documents/vera20k-dev/game/Command and Conquer Red Alert II/gamemd.exe" \
  VERA20K_PROJECTILE_RENDER_ASSETS="$HOME/Documents/vera20k-dev/class-parity/shrapnel-inputs" \
  VERA20K_CLOAK_MAP="$PWD/tools/spatial_oracle/cloak_target_gate/ordinary-sub-cloak.map" \
  VERA20K_NATIVE_FAILURE_DIR="$HOME/Documents/vera20k-dev/techno-active-retail/C28-cloak-native-v1/failures" \
  "$HOME/Documents/vera20k-dev/.venv/bin/python" -B \
  -m tools.spatial_oracle.cloak_target_gate --write \
  --output "$HOME/Documents/vera20k-dev/techno-active-retail/C28-cloak-native-v1/cloak_target_gate.json"
```

Run the same command in an independent process with `--check` replacing
`--write`. The existing `finish_vectors` workflow supports arbitrary
`--output`; its `.meta.json` is beside that output. The default is check only.
No references are generated on import. Root can install accepted, consumed
JSON/meta after terminal readback and freeze release; source is not mutated by
the scratch command.

## Rust consumer scope

`inputs.constructor`, `inputs.layers[*].after` and `inputs.final` expose
SUB/DEST cloak flags/speed, strength/armor, naval/underwater, range-selection
fields, normal/elite weapon references, weapon Range/MinimumRange/ROF/Damage/
DecloakToFire, Bullet flags, Warhead verses bits and General stage/delay bits.
`reads` preserves actual native defaults with exact key/section/reader identity.
`rows[*].input` is the declared world/timer boundary; `before` and `after`
retain raw state. `can_auto` is the measured original result. A Rust consumer
should compare the canonical production range owner and cloak facts, preserving
lazy skips. It must not synthesize expected timer/range/RNG values or claim a
Rust global Main/Mapgen proof from this component's initialized native streams.

The stock production regression and captures independently own ordinary SUB
placement, Attack, CloakingTick/StartCloaking, target retention, completion and
visible output. Factory-contact state0 exclusion, own-field sensor outer
admission, cloak crates, EMP and other stock actors are outside this leaf.
