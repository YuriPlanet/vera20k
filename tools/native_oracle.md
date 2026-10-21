# Native comparison workflow

### Ordinary animation translucency

`python -m tools.spatial_oracle.anim_translucency --check` runs physical FlakWH
selection and SMKPUFF's original reader, constructor, AI, DrawIt and RGB565 raster.
Three supplied background/A surfaces at four actual stages retain full color
outputs and checked unchanged Z/A planes. The actual Display-effect emitter,
lowering and GPU consumer is `anim_translucency_gpu_tests`; building-slot
temporal state and complete battles are outside the comparison.
See [inputs and boundaries](spatial_oracle/anim_translucency.md).

### Building-slot animation translucency

`python -m tools.spatial_oracle.building_anim_translucency --check` runs
physical GAPOWR/GAPOWR_A readers and slot construction, a populated temporal
notice, whole LetGo, AI and complete no-shadow animation draws. Three supplied
surfaces cover all eight loop frames and replacement during erasure: 78 full
color/Z/A outputs. Compact lossless planes accompany a complete raw archive.
The ordinary CLEG Attack/Stop lifecycle and actual slot-emitter GPU comparison
are separate tests. See [inputs and boundaries](spatial_oracle/building_anim_translucency.md).

### Mind-control attacked-object events

`python -m tools.spatial_oracle.mind_control_tags --check` runs eleven original
`BulletClass::DetonateAtCoord` controls through attached-object events 6 and 44.
The ordinary rows stop before CaptureUnit; two rejection controls execute its
refusal. The fixture inputs, callback state and excluded world lifecycles are
documented in [`mind_control_tags.md`](spatial_oracle/mind_control_tags.md).

The Rust regression
`retail_all02_mind_control_attack_starts_the_authored_stallone_script` uses the
physical Allied mission 2 map's Lasher/Stallone actors and unchanged trigger
definitions. A stock YURI attack must start the authored ChangeHouse, CreateTeam
and Voice chain without damaging the tank. It supplies a flat arena and an
already-acquired YURI; it does not replay the Engineer/base-building opening or
compare a complete original campaign. Set `RA2_DIR` to the retail install and
`VERA20K_REQUIRE_RETAIL_ASSETS=1` so absent archives fail instead of skipping.

### Computer sale of captured buildings

`python -m tools.spatial_oracle.captured_building_sale --check` executes the
original Engineer order/Walk/capture path through Building ChangeOwner's sale
handoff, with additional bounded controls for its predicates, C4, Buildup and
upgrade refunds. The real Building constructor establishes the initial House;
the current owner may subsequently change without replacing that identity.
The input setup, original call sites and explicit fixture boundaries are in
[`captured_building_sale.md`](spatial_oracle/captured_building_sale.md).

The corpus stops before the Selling mission visits. The retail Rust tests in
`world::ai_captured_building_sale_tests` supply an ordinary AI Capture order,
observe Engineer movement, and run the captured plant's sale through its
refund/removal. A second test captures, saves/loads, and recaptures the plant
for its constructor House, which keeps it. These compose the rebuilt engine;
they do not execute autonomous native Team/Hunt planning or original SAV load.

### Selected-weapon target-scan bound

`python -m tools.spatial_oracle.threat_scan_bound --check` compares 65 original
`Greatest_Threat` bound calculations at `0x6F9042..0x6F9148`. The preceding
`Threat_Range` result and runtime type, weapon, veterancy and cargo state are
supplied. Original getters execute without substitutions; the bound is read
from `ESP+0x38`. Checked completion requires the relevant branch visits,
unchanged object storage and stack pointer, and no writes outside the stack.

The controls cover nonzero hard bounds, current weapon slots for non-Gattling
types with `TurretCount > 0`, the underwater/organic/self-healing `GuardRange`
branch, fallback slots 0/1, signed cell truncation, `AirRangeBonus`, elite
fallback and open-topped cargo limits. The Rust comparison is
`scan_bound_matches_original_weapon_selection_slice` in
`src/sim/combat/greatest_threat.rs`. The slice ends before occupied-building
adjustments, target traversal and scoring; it is not a whole-game comparison.

The related stock regression,
`retail_sniper_ifv_guard_scan_uses_its_boarded_weapon_range`, uses retail Battle
rules and the production boarding command to put `SNIPE` in an `FV`. It checks
Guard acquisition at 12 cells with `AWPE`, rejection at 15 cells, and rejection
after unloading restores `HoverMissile`. Set `RA2_DIR` to the retail install
and `VERA20K_REQUIRE_RETAIL_ASSETS=1` so missing archive inputs fail instead
of skipping. This joins stock boarding to the rebuilt scanner; it does not
run a matching whole native scenario.

### Terror Drone firing, impulse and Drive controls

`tools.spatial_oracle.gattling_unit_fire --shp-frames --check` compares the
original Unit SHP firing selector for every facing byte, eight countdowns and
signed counter arithmetic edges. Its DRON layout is independently read by
`--shp-history --check`, which reuses the joined physical input/runtime owners.
Set `VERA20K_BUILDING_DEATH_ASSETS` to the joined input directory, including
physical `DRON.SHP`, `DRONP.SHP`, `GTNK.VXL`, `GTNK.HVA` and `VOXELS.VPL`, and
use `PYTHONPATH=.:tools/spatial_oracle`. The history starts at real constructor
and Unlimbo admissions, executes whole Unit FireAtTarget, Bullet flight and
attachment, then direct ExitUnit and whole released Unit AI visits. It retains
the firing count across limbo, frame indices, RNG requests/continuations and
selected timer stores. Direct ExitUnit is an explicit boundary; a native Sonic
hit producer, whole Logic schedule and scene pixels are not covered. The older
default gattling corpus remains separate.

`tools.spatial_oracle.unit_rocking_update --impulse` adds eight whole original
`70B280` controls through the existing physical GTNK loader VM. Supplied inputs
cover opposite sides, 3D distance, actual stock default Weight 1, explicit
3.5/zero/negative weights, weak/zero-distance gates and replacing two impulses.
Ordered native field stores and follow-up `70B570` states are retained. One
60-visit history keeps eight checkpoints, including amplification of a tiny
nonzero rate and eventual settlement. Native f32 state remains unmodified
between visits; these are component inputs rather than a whole native scenario.

`tools.spatial_oracle.naval_draw_bounds --drone-tilt` adds eight full original
Drive `4AFF60` controls using actual GTNK body/turret/barrel files. It reuses
the loader/postpass, startup slope tables, camera, gun-arm and raster owners.
Two rows retain all three physical indexed rasters. Set
`VERA20K_SHRAPNEL_INPUTS` to their extracted physical directory and use
`PYTHONPATH=.:tools/spatial_oracle`. Both opt-ins use `--check` by default and
`--write` only for deliberate new-corpus regeneration. Historical update,
Ship, slope and voxel corpora remain separate.

The related two-bite original RNG/coordinate comparison is
`parasite_water_gate --drone`; its supplied model-binding and direct-Limbo
boundaries are recorded with the
[shared runtime](spatial_oracle/building_death_anims_joined.md#terror-drone-numeric-component).

### Squid-grapple Ship presentation controls

`tools.spatial_oracle.naval_draw_bounds --squid-tilt` adds eight original
`69F670` matrix controls without changing the historical default corpus. Set
`VERA20K_SHRAPNEL_INPUTS` to the physical input directory containing
`DEST.VXL`, `DEST.HVA` and `VOXELS.VPL`, and run with
`PYTHONPATH=.:tools/spatial_oracle`. Use `--check` to compare or `--write` to
deliberately regenerate `spatial_oracle/naval_draw_squid_tilt.json`.

The existing raster VM loads the physical model, executes original loaded-model
validity and half-size postpass, constructs Ship locomotion, initializes native
slope/quaternion tables and runs the original draw matrix. Inputs cover the
double 0.005 tilt gate, opposite rolls, two angles/facing, retained slope and a
slope transition. Two rows retain physical hull raster pixels; their explicit
gun-component matrices reuse `barrel_pitch.native_arm`. Retail DEST has
`Turret=no` and no turret/barrel models, so those component matrices do not
claim physical DEST gun rendering. Frame, pose and timer are supplied numerical
inputs; native scenario motion, shadow pixels and whole-scene RGB are excluded.

`python -m tools.native_slope --blend --check` compares the same startup's
21 matrices and quaternions plus seven full `755A40` interpolation controls.
The existing VM/startup owner supplies both tables; original `646590` and
`646980` compute each blend. Differing indices still interpolate at phase 0
and 1, so those endpoints can differ from the stable matrix table. Equal
indices copy the stable matrix directly. The small `native_slope_blend.json`
corpus records actual live and cached CRT controls and finite supplied phases;
it does not establish a locomotor timer or cell-transition history.

`tools.spatial_oracle.parasite_water_gate --grapple` adds the related bounded
RNG/angle comparison through full original `6297F0`. Its fresh physical host
and explicit direct-Limbo/Attach boundary are documented with the
[shared runtime](spatial_oracle/building_death_anims_joined.md#squid-grapple-numeric-component).

For original byte reads, disassembly and whole-code scans, use the indexed
[native inspection tool](native_inspect.md). It shares this runner's executable
selection and PE owner; static inspection is separate from checked execution.

Native comparison guidance; adapt as useful and distinguish findings from hypotheses.

Use original `gamemd.exe` instructions to produce reference outputs, then compare
the Rust function used by the engine against them. The shared Python runner handles
executable selection, bounded execution, fresh function-call state, diagnostics, and
reference-file checks. It does not establish that a chosen function or fixture
represents the game's active path.

### Capstone

Static inspection uses Capstone **5.0.7**, pinned in `tools/requirements-test.txt`.
Install those dependencies for the [shared inspection CLI](native_inspect.md) and
its portable tests. Mechanism-specific runtime-memory consumers retain their own
interpretation; a static file scan does not observe initialized memory.

## Run an existing comparison

From the repository root, with Python 3.10+ and Unicorn **2.1.4** installed:

```powershell
python -m pip install unicorn==2.1.4
$env:VERA20K_GAMEMD_EXE = 'C:/your-retail-install/gamemd.exe'
python -m tools.color_oracle.hsv_to_rgb
cargo test -p vera20k-core --lib rules::color_scheme::tests::hsv_to_rgb_matches_native_oracle_all_hues_at_sv_boundaries
```

`RA2_DIR` is an alternative; an explicit executable path takes precedence and never
silently falls back. The loader maps whichever `gamemd.exe` that selects, with no
identity check or refusal: every copy (the original 1.001, Steam and other releases)
is treated as the same game.

No executable is distributed here.
The harnesses run code at fixed retail addresses, so a copy whose code differs
there produces different outputs and fails `--check`. The game reads no `gamemd.exe`
bytes. The Rust table checks read the `RA2_DIR` executable's math tables at the same
retail addresses and compare every byte, without checking the file hash.

### Evidence identity

Every `gamemd.exe` build is the same game, so references record what the executable
computes, never which build ran it; `replay_command` records how the payload was
made. `finish_vectors` refuses, under `--check` and `--write` alike, output that would
publish a `gamemd.exe` hash: the configured executable's SHA-256 anywhere in the
payload, its sidecars or the raw archive, or the `NATIVE_SHA256` label at a JSON path
where the existing reference does not already carry it. It recognizes only those two
digests, so record no other hash of the executable or its sections either. Producers
with their own writers bypass the check; new comparisons use `finish_vectors`.

Older evidence keeps its label fields as inert history until its producer drops them.
The `homing_impact`, `ordinary_collision` and `shared_collision` projectile producers
still hash the executable into such a field, so on another build they stop with this
refusal instead of a payload mismatch.
`--check` ignores a legacy `native_sha256` in an older sidecar and `--write` drops it.
`image_sha256()` returns `NATIVE_SHA256`, a fixed label that older payload schemas
carry; it is not a hash of the selected file. To compare two releases' code, use
`python -m tools.native_inspect sections`.
Ghidra annotation ledgers replay on any `gamemd.exe` program; a legacy
`program_sha256` in an older ledger is ignored.

The factory packet's registered helper profiles and inspection-owner seal are
unchanged. Only the explicitly unregistered candidate helper
map is refreshed for the changed shared sources; it is not a compatibility verdict.
Saved-receipt unit tests validate retained profile claims against the sealed
registration independently of today's source admission. Native factory controls
have not been replayed or rebaselined by this change.

Any `gamemd.exe` checks the committed references directly:

```powershell
$env:VERA20K_GAMEMD_EXE = 'C:/Games/RA2/gamemd.exe'
python -m tools.spatial_oracle.shroud_current_sight --check
```

[`shroud_current_sight.json`](spatial_oracle/shroud_current_sight.json)'s 23 histories
and 9 timer cases cover selected cell reveal/gap and timer transitions, not renderer
pixels or full-game parity.
Keep execution receipts and validation results outside the source tree and report
them in the PR description.

Both halves matter: the Python command checks current native output against the
reference; the Rust test checks production conversion against that reference.
Python success alone does not show Rust parity. Normal Rust tests use committed
outputs and do not require Unicorn or an executable. Follow `AGENTS.md` before Cargo.

### Tank-bunker range comparison

`python -m tools.spatial_oracle.walk_cell_range --bunker --check` replays the
separate `walk_cell_range_bunker.json` corpus. Select a supported executable with
`VERA20K_GAMEMD_EXE` and set `VERA20K_BUNKER_ASSETS` to a directory containing
extracted retail `RULESMD.INI` and `MPBattleMD.ini`. Use the existing `asset` tool
to extract them from the selected installation; include `LANGRULE.INI` only if
that installation supplies it. The recorded Steam inputs had no LANGRULE layer.
No executable or retail asset is distributed with this fixture.

The final `supplied_empty_map_rules` layer explicitly supplies no rule overrides.
It tests original reader admission and retained values; it does not replay a
complete map. No custom map, ART, SHP or sound file is required. The 20 range rows
cover the Unit bunker bonus, direct/arcing boundaries and declared ordering and
overflow controls. Their native input readers establish LTNK/ATGUN/Cannon fields;
installation, acquisition, firing and rendering remain separate validation.

### Tank-bunker entry

`python -m tools.spatial_oracle.bunker_entry --check` executes eight joined
original histories through the Unit destination, Building radio and Foot Enter
owners, including busy, cancellation, replacement and AI seek controls. A supplied
arrival exercises PerCell DOCK_NOW; a prepared state3 control pins the native
wait for a retained Drive destination. Rust consumes these outputs separately
from stock world-frame installation tests. See [scope and reproduction](spatial_oracle/bunker_entry.md).

### Stationary attack actions and building control

`python -m tools.spatial_oracle.bunker_attack_action --check` executes original
DetermineAction, Unit/Foot/Techno action dispatch and Display4AAE90 through the
normal cursor setter boundary. Its 16 ordinary/bunker controls and 24
armed-harvester/deployed-Unit controls supply weapon-selection, range and
GetFireError verdicts. The Display prefix resolves valid empty ground to
CellClass* at4AAFF1, and the attack arm at4AB4EE consumes it: single-selection
in-range ground or Object targets use row20. Four additional display controls
set selected count2/3 and use row21 even in range; they supply the action actor,
not a complete multi-object selection lifecycle. The former switch-only fixture
incorrectly supplied NULL for empty ground and has been replaced. Empty House
waypoint containers and palette storage support the unmodified prefix; renderer
and cursor SHP output are outside this harness. These controls establish
caller ordering and action/cursor values, not weapon arithmetic, reload cadence
or the full input window. In particular, a bunkered Unit's out-of-range Action0
is distinct from a fully deployed SimpleDeployer's Action2. Rust input tests in
`app::input::cursor::tests::bunker_` consume this corpus; command integration also
checks an occupied building through queued commands and actual firing ticks.

`python -m tools.procedural_drawing_oracle.factory_destination --control --check`
uses a separate corpus for Building44F5C0's virtual control capability and the
Object5F4520 selection prefix. It preserves the original destination corpus.
`sim::world::world_orders::building_control_tests::` covers 29 nonpositive-EMP
controls and 58 selection-prefix variants. Eight positive-EMP native controls
remain unported because this chain has no EMP-state producer; the prefix stops
before selection commit. Construction-yard controls use the existing House,
session, mind-control and undeploy owners.

The input action/range probes add no simulation RNG draws, timer writes or detach
calls. Existing command, deployment and occupied-fire owners retain those effects;
these fixtures do not establish their complete scheduling parity. The isolated
query regression covers this attack probe only: the older ordinary-hover/click
GetFireError path still uses the canonical map query, so missing-cell Dummy writes
there are a separate residual, not a claim of complete input-query purity.

These generators now default to **read-only checks**, also spelled `--check`:

`python -m tools.spatial_oracle.turret_rotate_sound --check` executes Building
rotation sampling, Techno start/continued-turn/idle controls and Limbo sound
cleanup on the supported historical/Steam layout. Rust consumes the corpus in
`sim::world::turret_rotate_sound::tests`. Audio is device-disabled: accepted
playback, source retention, audio RNG, attack/decay and whole native Load are not
covered. The metadata records the executed binary, command and supplied type getter.

| Python module | Sampled native behavior | Rust consumer/test location |
| --- | --- | --- |
| `tools.spatial_oracle.vehicle_passenger_escape` | Six resolved-input Unit passenger controls through the original death-escape caller, full Unit Unlimbo, map occupation, virtual Scatter and destination setter. Two-passenger rows retain the first passenger's native writes: its new NavCom admits the second before movement. Includes Sleep-to-Guard Unlimbo, frame, retained destination and blocked-neighbor controls. Boarding, damage prefix, full constructors/readers and later movement are excluded. | `world/vehicle_passenger_escape_oracle_tests.rs` compares placement, navigation, occupancy and RNG; `passenger/gunner_tests.rs` exercises stock LCRF/MTNK boarding and destruction through production commands and damage. |
| `tools.rmg_oracle.gen_rng_vectors` | Seeded state and 16 draws for five seeds | `src/map/rmg/rng.rs` |
| `tools.rmg_oracle.gen_x87_vectors` | Eight Gaussian draws for two seeds | `src/map/rmg/x87.rs` |
| `tools.storage_oracle.sed_description` | 28 fresh-file Description fixtures and one cached-section diagnostic; original reader with supplied INI indexes | `src/map/rmg/description.rs`; disk-load regression in `saved_seeds.rs` |
| `tools.projectile_oracle.ordinary_motion` | 120 cases, eight gravity/candidate blocks each | `src/sim/projectile.rs`, `src/sim/world/projectile_collision.rs` |
| `tools.projectile_oracle.vertical_motion` | 110 cases, eight velocity/candidate blocks each | Same projectile consumers |
| `tools.projectile_oracle.load_timers` | 877 late-Fire/Save/global-frame-prefix/Load/check cases, five admission probes each | `sim::projectile::tests::projectile_load_timers_match_original_fire_save_load_and_check`; [fixture scope and substitutions](projectile_oracle/README.md#bullet-timer-saveload) |
| `tools.rules_oracle.weapon_speed` / `weapon_speed_order` | Thirty original Speed reader controls and two fourteen-pass full Process histories, including retained Gravity and Weapon postpass order | `rules/weapon_speed_tests.rs`, shared `util/native_ballistics.rs` and existing359 GetSpeed rows; [bounds and reproduction](rules_oracle/weapon_speed.md) |
| `tools.projectile_oracle.bridge_render_inputs` / `bridge_render_inputs_palette` | Full original Cannon constructor/readers, selected MTNK/105mm producer, physical image and palette startup/Convert construction | `sprite_atlas_projectile_tests.rs`; physical lexical file and device-format boundaries supplied. [Input evidence](projectile_oracle/bridge_render_inputs.md) |
| `tools.projectile_oracle.bridge_render_art_state` | Original retained Object/Bullet ART fields and image-loading branches across admitted and omitted rules layers | `rules/projectile_art_tests.rs`; original preceding Anim reader supplies the active-retail ART-cache boundary; referenced retail type selection and archive IO are bounded fixtures |
| `tools.projectile_oracle.bridge_render` | 149 original Object/Bullet draw-argument rows plus six retained-object live bridge-flag transitions | `presentation/instances/projectile_render_tests.rs`; retained world/lifetime supplied, final shape is an argument sink. [Drawing evidence](projectile_oracle/bridge_render.md) |
| `tools.projectile_oracle.bridge_render_shape` | 40 complete original physical120MM draws through palette construction, clipping, plain rowwalker and RGB565 leaves | `presentation/render/projectile_shape_gpu_tests.rs`; explicit BSurface/A/Z inputs, not a full native scene |
| `tools.projectile_oracle.bridge_render_pixels` | 400 signed-depth/transparency/repeat cases, four zero-run stencils and both shadow leaves over all65536 destination words | `render/projectile_draw_gpu_tests.rs`; prepared leaf inputs, source body color deliberately synthetic |
| `tools.projectile_oracle.bridge_render_flight` | Selected native105mm speed/launch, eight original ordinary-motion blocks and draw captures | `presentation/instances/projectile_flight_tests.rs`; upstream coordinates, admitted collision commits and world/Display seams supplied |
| `tools.color_oracle.hsv_to_rgb` | All 256 hues at nine saturation/value pairs | `src/rules/color_scheme.rs` |
| `tools.spatial_oracle.shroud_current_sight` | 23 ordinary-cell reveal/gap/fire/Psychic/SpySat-bulk/120-frame sequences plus9 signed Foot timer gates; actual MapCell, selected Gap/bulk blocks and complete periodic sweep | `src/sim/vision/vision_tests.rs`; world, restore and tactical CPU regressions |
| `tools.spatial_oracle.map_queries` | 114 lookup cases, 15 LocalSize prefixes, 2,010 playfield queries and five retained-dummy calls | `src/sim/cell_rect_native_tests.rs`, production `cell_rect.rs` / `map/playfield.rs` |
| `tools.spatial_oracle.anytown_damage.mission --unit-unlimbo` | Nine original MTNK Unlimbo controls, eight whole factory443C60 controls, eight actual Unit Limbo/reused Stage controls, six UnitType bool-reader histories, seven factory radio8 fixtures/nine calls and six authored HIGH/scope controls; initialized geometry, nested scope, true returns and full RNG | `world/unit_unlimbo_tests.rs`; [inputs, ordering and seams](spatial_oracle/anytown_damage/unit_unlimbo.md). Single supplied House/crop; bridge pose/poisoned Stage and parsed HIGH supplied. Inactive Scenario, invalid GAYARD/MTNK and NULL-contact/sender rows are bounded controls. Excludes complete map-token/House lookup, legal Ship/FNPC naval completion, docking/repair and full UnitPerCell. Default Mission corpus is preserved. |
| `tools.spatial_oracle.unit_entry --counter-mode` | Eight original Unit73F0A0 repeat-call controls separating A8E7AC nested scope from actual GameMode A8B238 in retained-boundary and nonhuman crate branches; unchanged scratch owners and three full RNG objects | `world/unit_unlimbo_tests.rs`; [schema and native identities](spatial_oracle/anytown_damage/unit_unlimbo.md). Existing Unit entry fixture owns construction; branch inputs are supplied. Legacy `game_mode_nonzero` remains an A8E7AC compatibility alias; default payloads are preserved. |
| `tools.bridge_click_oracle`, `tools.bridge_click_state_oracle` | 1,782 bridge candidate cases, 572 original tactical inverse calls and 36 same-cell intact/collapsed/repaired flag queries; synthetic mapped real cells, original lookup/projection leaves | `map/terrain_bridge_click_tests.rs`, `app/input/bridge_live_click_tests.rs`; live flags after runtime setters and `PreparedLoad`, excluding off-map dummy behavior, event delivery and GPU output. [Reader evidence](bridge_click_state_oracle/README.md) |
| `tools.spatial_oracle.bridge_records` | 83 ordered high/Tube record cases, CellIterator first-null traversal and shared dummy coordinates | `src/sim/bridge_state/record_native_tests.rs`, production `record_scan.rs` / `map/resolved_terrain/native_cell.rs` |
| `tools.spatial_oracle.fv_cell_attack.paid_conditional` | Six original empty-FV Cell-attack continuations: Approach, paid Drive, missiles, collapse, Stop and effect retirement; declared outside RNG calls, not full Scenario scheduling | `world/bridge_fv_pursuit_tests.rs`, `bridge_fv_persistence_tests.rs`; [reproduction, prerequisites and coverage limits](spatial_oracle/fv_cell_attack/README.md) |
| `tools.spatial_oracle.temporal_stop` | Five retained Attack timer controls: target-clear setters, due dispatch, Temporal LetGo and cadence RNG; supplied already-warping state | `src/sim/temporal_tests.rs`; full Infantry admission, firing initialization and per-frame AI remain outside this probe |
| `tools.spatial_oracle.infantry_ai_order` | 22 original Logic/Infantry caller controls: live Ready/Commence, alive/warp/Thief gates, fear/fire/sequencer order and retained latch/timer branches; declared callback bodies | `world/object_turn.rs`, `combat/world_receiver.rs`, mixed Infantry/Unit and Temporal production regressions; [scope and reproduction](spatial_oracle/infantry_ai_order.md). Does not establish a full shot, retained firing-latch lifecycle or full Scenario |
| `tools.spatial_oracle.infantry_deploy_action --automatic-guard` | 90 original Guard/Sticky/AreaGuard automatic-deploy controls and ten deployed SelectWeapon(NULL)/range/cleanup controls; supplied prior state, unchanged original bodies/vtables and empty scans | `world/techno_ai/automatic_deploy_oracle_tests.rs`, `deployed_guard_oracle_tests.rs`, shared `deploy_tests.rs`; chain, bounds and reproduction |
| `tools.spatial_oracle.bridge_damage_admission` | 284 rows: 149 original bridge admission/callback/RNG cases, 63 live driver selections, 10 Cell launch aims, 14 impact ladders, and 48 signed BridgeStrength reads; driver returns/writes and detach/dirty sinks supplied | `bridge_state/damage_dispatch_tests.rs`, `combat/bridge_launch_tests.rs`, `world/projectile_collision.rs`; evidence and boundaries. Excludes driver bodies, collapse lifecycle and whole-bridge parity |
| `tools.rules_oracle.bridge_anim_lists` | 24 original constructor/retained ReadGeneral animation-vector cases, including the retail 127-byte truncation to 15 metallic references ending in unread `D`; cached INI indexes supplied | Production `RulesPassProcessor` and retail-reader tests in `native_processing_tests.rs`; exact retained lists, not native physical file loading |
| `tools.rules_oracle.bridge_anim_inputs` | Original full AnimType ctor/ART/image reader for24 retail types plus2 asymmetric Scorch/Crater controls, supplied physical ART strings and whole SHP bytes | `world/bridge_debris_tests.rs` compares actual production ART configuration to independent native inputs; physical INI/MIX loading and audio remain outside this corpus. [Input evidence](rules_oracle/bridge_anim_inputs.md) |
| `tools.rules_oracle.bridge_landing_inputs` | Original selected Rules/Warhead/Terrain constructors and layered readers, original ART Foundation;13 complete Verses forms,4 native null-token fault cases and198 native damage sensitivity cases | `warhead_type.rs` and `world/bridge_debris_flight_tests.rs` compare production input readers and damage; physical INI/MIX traversal is supplied, malformed short-list Rust recovery is not native equivalence. [Reader evidence](rules_oracle/bridge_landing_inputs.md) |
| `tools.spatial_oracle.radiation_damage_boundary` | Six original percentage/Radiation reader, flat-site spread, Foot admission and Object receiver rows explaining the shared Verses regression | `combat_tests::rad_damage_fires_on_application_delay_boundary_only`; final health/admission only, with native base59 versus Rust60 explicitly unresolved. [Bounds and required radiation follow-up](spatial_oracle/radiation_damage_boundary.md) |
| `tools.rules_oracle.bridge_child_sound` | Original selected SOUNDMD registry/Voc readers and three ART Report bindings; native Release versus hard-stop on two playing one-shot events | `sound_dispatch::bridge_child_sound_tests` exercises the existing Anim event/audio owner; device channels, waveform equivalence and whole child AI order remain outside native execution. [Sound evidence](rules_oracle/bridge_child_sound.md) |
| `tools.rules_oracle.anim_image` | 11 original Image25 reads and loader fallback selections; exact section and current-ID default supplied from the caller | `art_data_tests.rs`, asset catalog and both atlas consumers preserve literal animation type identity |
| `tools.rules_oracle.anim_theater_flags` | 26 original fresh AnimType reader/theater-postpass controls: Image-section flag defaults, own overrides, invalid/missing values, one-hop aliases and literal-file availability; declared header-only SHPs | `art_data_tests.rs::animation_theater_flags_match_original_fresh_read_and_postpass`; [scope and reproduction](rules_oracle/anim_theater_flags.md). Retained INI rereads, instance lifetime and the complete Theater filename policy are excluded |
| `tools.spatial_oracle.bridge_debris_producer` | 266 original Cell47DD70 producer/Anim constructors plus 12 shared death-debris loops; production-exported 15/4 animation pools and SHP frame counts | `world/bridge_debris_tests.rs` and combat death-debris tests; synchronous constructor/RNG order, excluding later AI |
| `tools.spatial_oracle.bridge_debris_flight` | 32 joined original producer/primary AnimAI/Bounce histories across dry, water, deck and discontinuous ground; per-visit RNG and child constructors; duplicate pending-delete queue drain executes once | `world/bridge_debris_flight_tests.rs`; only primary AI scheduled, empty contact/observer lists, supplied audio boundaries and false bridge drivers. See chain evidence |
| `tools.spatial_oracle.terrain_debris_receiver` | 24 original Bouncer contact/Terrain ReceiveDamage/unlink/Logic/retirement rows; actual retail TREE01 occupation bits | `anim_debris_tests.rs` compares 19 single-tree receiver cases. Synthetic multi-tree, custom successful death-animation and populated observer chains are excluded |
| `tools.spatial_oracle.terrain_coordinate` | 41 original Terrain placement/retained-location rows: signed ground levels, slopes and structural flags, followed by changed ground | `terrain_coordinate_tests.rs` compares 40 authored input-Z-zero cases through production construction, damage collection, hashing and snapshot restoration; the extra above-ground input characterizes a separate caller |
| `tools.spatial_oracle.terrain_render` | 82 static and 44 animated original placement/retained-coordinate/Render-suffix/DrawIt rows; separate original format-3 decoding hashes for all 22 frames of both stock TIBTRE byte variants across 18 assets | `app/presentation/instances/terrain_render_tests.rs` compares drawing arguments; `render/overlay_atlas_tibtre_tests.rs` compares retail decoding and atlas pairs. Caller rows exclude visibility admission and native pixels; stock decoding uses an identity palette and all-pass depth. GPU output and full render scheduling need separate validation |
| `tools.spatial_oracle.tibtre` | 39 probability boundaries, four constructor controls, eight selected type reads, 14 timelines/660 AI visits and 24 forced-spread controls through original Terrain AI, Spread, admission, Place, growth insert and RNG | `terrain_spawn.rs` and `world/tibtre_oracle_tests.rs` compare retained timers, cells, queue priorities and full RNG state through the production object turn. Separate integration checks follow emitted ore through growth/harvest/deposit and snapshot/limbo. Whole native Logic scheduling and complete resource economy remain outside these fixtures |
| `tools.spatial_oracle.ore_queue` | 128 original body cases: 52 enqueue/rebuild, three Reduce_Tiberium, five GrowthProcessor/Grow/Place, 47 natural SpreadProcessor, 16 SpreadDriver and five growth-before-spread histories; 16 selected original constructor/INI reader cases. Cells, heap/array/bitmap state, signed timers, source/target gates, chopped priorities and full RNG continuation; Scenario ID wrap, Overlay registration and two original deferred drains | `sim/ore_growth/queue_oracle_tests.rs`, `rules/tiberium_type.rs` and `world/ore_spread_oracle_tests.rs`; 21 app-frame driver histories, two constructor cleanup controls and publication before object AI. Supplied Size4x4 allocation and prior queue state exclude whole Scenario startup and long-session history. [Reproduction and bounds](spatial_oracle/ore_queue.md) |
| `tools.spatial_oracle.terrain_strength` | 69 original constructor-store, ReadInt/store and successful-reader sentinel suffix rows; cached INI indexes, resolved TreeStrength and base-reader success supplied | `terrain_object_type.rs` compares 68 successful fresh reads through RuleSet plus actual retail TREE01 inputs; only stored Strength=-1 invokes TreeStrength. Re-read retention remains outside this fixture |
| `tools.projectile_oracle.bridge_cluster_order` | 72 complete original impact → ordinary detonation → area/bridge → cluster cases; signed damage and RNG continuation; empty receivers and false bridge drivers bound the comparison | `combat/bridge_cluster_tests.rs`; bridge admission precedes cluster distance/angle even for negative nonzero damage; excludes driver/collapse effects |
| `tools.spatial_oracle.bridge_gap_flags` | 20 ordered fresh-load inactive high-record restamps and48 original setter anchor prefixes | `src/sim/bridge_state/gap_restamp_tests.rs`; real flags/save/hash and production building placement |
| `tools.ai_base_building_oracle` | 94 EconomyStateMachine steps with their `RandomRanged(0,1)` draws, 45 AI_Choose_Building node/draw/power-splice/wall cases on the native node vector, 86 whole AI_BuildWalls runs (wall type by side and list order, every foundation, node growth, placed buildings' cells), 13 PlacementDelay waits, the away direction for all 48 neighbour sums, 160 site keys, 39 qsort orders, reserved-near bounds and 68 whole FindBaseBuildingSite transcripts on synthetic maps; Find_Node, Get_Node_Building, ChooseNextProduction (failure), CheckOccupancy and CanPlaceAt answers supplied | `src/sim/ai_base_building_tests.rs`, `src/sim/ai_base_site_tests.rs` |
| `tools.ai_base_defense_oracle` | 36 coverage spreads (Sqrt_Approx falloff), 787 defense site keys, 44 enemy threat ratios with their `RandomRanged(-r, r)` draws, 57 whole AI_ChooseNextProduction runs without a perimeter vector (coverage, weakest quadrant, category, candidate lists with prerequisites, weighted `RandomRanged(1, total)` pick, site-search call with its grid, node writes) and 34 whole FindBaseBuildingSite transcripts with the defense key; RandomRanged, FindIndexOfName and the choice's site answer supplied | `src/sim/ai_base_defense_tests.rs`, `src/sim/ai_base_site_tests.rs` |
| `tools.ai_strategy_oracle` | 22 runs of HouseClass::Update's Strategy block (the `+0x5634` timer, human and MultiplayPassive gates, restart), 188 whole AI_Building_Strategy runs with the original UpdateAngerNodes (first-eligible enemy search, defeated-enemy clear, superweapon gate, every emergency transition, the no-factory urgency and its dispatch, the `RandomRanged(1,7)` reschedule), 6 Fire_Sale and 4 All_To_Hunt runs; money, Fire_Sale/All_To_Hunt (in Strategy), AI_TryFireSW, Check_Build_Need, Manage_Build_Queue and every object callee recorded | `src/sim/house_strategy_tests.rs` |
| `tools.superweapon_oracle` | The nuclear missile's chain: 122 SuperClass::ClickFire runs (0x006CB920: admission, the Lightning Storm deferment refusal, the Launch call, readiness and recharge-timer writes; no charge drain or one-time grant), 75 computer-house launch alerts (0x004FAF00: gates, Distance3D of base minus cell, the Scenario `RandomRanged(0,99)` draw, stores), 15 BuildingClass::Mission_Missile visits (0x0044C980: each status of a silo and a non-silo, Begin_Mode, the PSIWARN and take-off anims, the missile's construction, coordinate and velocity bits, delays), 5 BulletClass::NukeMaker runs (0x0046B310: the falling NukePayload bullet) and 96 SuperAnim slot runs (UpdateAnimation 0x00450F9E..0x00451145, OnConstructionComplete 0x004463F0..0x00446580); one fixture cell answers every map lookup; Launch, HasDeferment, PsyDom::Active, Begin_Mode, Queue_Mission, the anim constructor and setters, CreateBullet, BulletClass::Construct, Fire and PlayAnim are recorded stubs and RandomRanged answers from the row | `superweapon/fire_tests.rs`, `world/techno_ai/building_missile.rs`, `projectile/launch.rs`, `combat/nuke_maker_tests.rs`, `building_art_super.rs`; production frames on retail rules in `superweapon/nuke_tests.rs` |
| `tools.ai_team_oracle` | 93 whole Unit/Infantry/Aircraft chooser runs (0x004FEA60, 0x004FEEE0, 0x004FF210) with CanBuild, Cost_Of, Available_Money, IsRecruitable and their `RandomRanged` draws hooked; 41 harvester-branch runs including the IMUL wrap; 73 selector runs (0x006F0AB0: the `RandomRanged(1,100)` gate, team and defense counts, the oldest defense team's eviction, `ftol` weights, the 5000 tier, the weighted draw and the cancel pass; eligibility supplied); 210 eligibility rows (the minimum-defense gate, comparator masks, power and money conditions); 214 Iron Curtain readiness rows; 18 HouseClass::Update team-block runs (timer, human and passive gates, restart); 264 AI trigger weight feedback rows (RegisterSuccess 0x0041FD60, RegisterFailure 0x0041FE20: x87 adjustment, deltas, coefficient, min/max clamps, counts); 10 ~TeamClass trigger-loop runs (0x006E8E02..0x006E8E3E); 32 empty-team dissolve rows (0x006E929B..0x006E92D8) | `src/sim/ai_unit_choice_tests.rs`, `src/sim/ai_team_creation_tests.rs`, `src/sim/team_script_vm/oracle_tests.rs` |
| `tools.team_recruit_oracle` | 528 TeamClass::Recalc runs (0x006EA3E0: the TaskForce total, full/under-strength bytes, a Reinforce= third, the empty team's destruction); 124 Calc_Center runs off action 10 (0x006EAEE0: member tests, GuardSlower= double count, the mean's IDIV, the closest member by 0x005F6500 including wraps and ties, the head fallback; GetCellAt and Can_Enter_Cell supplied); 153 Recruit runs over each class array (0x006EAA90: group filter and penalty, the 0x005F6560 key, ties, the Unit search's house/type test, the entry gate; Can_Add supplied); 168 ObjectClass::Distance runs (0x005F6360, every foundation's deduction); 12 action 5 guard timers; 97 action 54 seeds up to FNPC (0x006EFA10: atan2 or `RandomRanged(0,255)` facing, sin/cos step, quotients); 62 FindOwnBuilding runs (0x006EEEA0; threat map supplied as 0) | `src/sim/team_script_vm/recruit_oracle_tests.rs` |
| `tools.spatial_oracle.bridge_restamp_retail` | Deadman original producer/restamp/ore-admission composition; pre-fix export regenerated at4f72210f | `src/sim/movement/bridge_restamp_retail_probe.rs`; current loader compares80 native post-state cells |
| `tools.spatial_oracle.bridge_base_edges` | 86 signed/clamped endpoint, canonical pair, reverse bucket order and deduplication cases | `src/sim/pathfinding/bridge_base_native_tests.rs`, production `zone_build.rs`; cache/restore regression in `world/navigation_tests.rs` |
| `tools.spatial_oracle.tube_hierarchy` | 65 high/Tube hierarchy helper cases, five ReadTubes writes, six raw-zone gate cases, five constant-walk endpoints and seventeen DWORD zone queries; raw data domain remains bounded | `src/sim/pathfinding/tube_hierarchy_native_tests.rs`, production `zone_build/hierarchy.rs`; route/transaction regressions in `zone_search_tests.rs` and `app/persistence/tube_hierarchy_restore_tests.rs` |
| `tools.spatial_oracle.path_entry` | 19 original endpoint lookup/query/retained projection and conditional playfield cases; final mover predicate supplied | `src/sim/pathfinding/zone_search/native_path_entry.rs`, production wrappers and tests in `zone_search_tests.rs` |
| `tools.spatial_oracle.foot_path_window` | 10 original Unit/Drive-or-Ship/Find_Path wrapper cases: short and 23/24/25-word routes, an in-window tube word, copy canaries, terminator and reference-cell observations; complete AStar output supplied, search and later movement excluded. [Reproduction, bounds and manual check](spatial_oracle/foot_path_window.md) | `src/sim/movement/foot_path_window_tests.rs::installed_path_window_matches_original_find_path_copy` compares live words and reference cell; `retail_tank_and_infantry_replace_path_windows_and_reach_their_move_destination` separately runs stock MTNK/E1 long Move orders through serial and batched whole-frame Rust continuation, without a native route or timing claim |
| `tools.spatial_oracle.astar_structural_height` (`--finishing`, `--hills-inputs`, `--mtnk-inputs`, `--threat-inputs`) | Original successful reconstruction/both finishing passes, physical Hills/MTNK inputs, Team coefficient readers/getter and signed House adjustment kernel | Shared movement/pathfinding and retained threat owners; [evidence bounds and commands](spatial_oracle/astar_path_finishing.md). Whole native route and production validation are separate requirements. |
| `tools.spatial_oracle.bounce_height` | 48 ground/query/surface cases through original Bounce update; flat contacts and airborne slopes, velocity signed zero excluded from Rust comparison | `src/sim/world/bounce_terrain_tests.rs`, live `bounce_terrain.rs` and `bounce.rs`; separate cliff/slope physics remains open |
| `tools.spatial_oracle.bounce_startup_capture` | Original process captures Bounce104/416 scalars and x87 control at four startup boundaries; executable section integrity | Inputs to `bounce_height`; constructor/startup evidence, not complete debris physics |
| `tools.spatial_oracle.locomotor_can_use_track` | ILocomotion `+0xA4` `Can_Use_Track` (Drive `0x004B4B00`, Ship `0x006A4130`): 6,400 Drive and 5,920 Ship cases over every turn selector, cursors on/beside each raw chain index, both short-track states and ten path-queue heads | `src/sim/movement/drive_track_tests.rs` `occupant_can_use_track_matches_native_oracle`; production `drive_track.rs::occupant_can_use_track`, consumed by `drive_track.rs::occupant_slot_a4_answers_true` |
| `tools.spatial_oracle.track_fresh_response` | 106 Drive/Ship rows (53 each): the real Unit setter `0x741970`, then the outer Process through `Process_Movement(&out, 1, 0)` (`0x4B2630` / `0x6A1C80`) and every recursion it makes, stopping where it returns to the outer Process: the facing gate, code-0 straight/turning/extension/straight-forcing acceptance and finalize, second-candidate codes, the code-2 latch/timer/urgency ladder, the code-3 gate tail, the code-1/4/5/6/7 retries, the code-6 CloseEnough stop (with a Tunnel row) and forced scatter, the wall Override, the accepted target speed (Track/Wheel slopes, the Road row, the zero row, the clamp and ConditionYellow), core Find_Path failures, the early selector write, the ParalysisTimer gate, the Foot+68B latch and the crusher paths. Unit `Can_Enter_Cell` and `Find_Path` answers are supplied (`core_null` runs the original wrapper with only the AStar core 0x4CBBA0 answering NULL); `Scatter_Objects` and `Override_Mission` are recorded | `src/sim/world/track_fresh_response_tests.rs` (104 rows; the two far-zone rows need a zone split); production `sim/movement/track_fresh.rs`, `pathfinding/terrain_speed.rs::fresh_track_speed_fraction` |
| `tools.spatial_oracle.tube_startup_capture` | Unpatched Windows process startup FPCW/adjacent data at four hardware breakpoints; full executable section integrity | Original initializer inputs for `tools.spatial_oracle.tube_hierarchy`; startup-only evidence, not runtime immutability |
| `tools.spatial_oracle.walk_paid_step` | 40 original paid Walk steps through the coordinate branch, including facing equality, zero speed, cell boundaries and five chained Slice6 production steps; movement-speed integer supplied | `src/sim/movement/walk_step_tests.rs`; production `walk_step.rs`, Slice6 frame assertions and move-to-fire/save-restore regression |
| `tools.spatial_oracle.facing_class` | 61 original retained histories through both constructors, signed ROT, Set/Snap/Current/IsRotating, live rate changes and frame boundaries | `movement/facing_class.rs`; production Unit spawn/combat facing, rotation latch and full save/restore continuation in `world/world_spawn/tests.rs` |
| `tools.spatial_oracle.aircraft_approach_range` | 235 original Object Distance_To queries and selected strafe range branches: strict raw-lepton boundary, sqrt ties, elite fallback, cell targets and all 22 foundations with Bib both ways | `combat::object_distance_to`; `aircraft/approach_range_tests.rs` compares every distance/range predicate and production in-range dispatch/save-restore. Native execution stops before the out-of-range setter; it does not prove full approach behavior |
| `tools.spatial_oracle.aircraft_approach` | 49 full Mission_Attack calls: 8 initial transitions and 41 approach branches, actual speed versus request, strafe/Fighter precedence, NavCom thresholds, primary/elite FLH, facings, setters, ammo/timers and RNG | `world/aircraft_approach_tests.rs` compares production dispatch and restored continuation. Three FLH samples retain documented one-lepton f32 differences with identical steering histories. Supplied flat airborne fixtures exclude tilt, full flight and remaining destination lifecycle effects |
| `tools.spatial_oracle.aircraft_states` | The whole original Mission_Attack `0x00417FE0` per visit: states 4..9 over every code, class, Ammo, Target, CurleyShuffle and IsClose; the state-9 delay; state 10 over pending, Ammo, Target, `+3D4`, house control, Airstrike and edge, plus 20 seeded edge scans. 368 stored rows stand for about 9,400 executed visits. SelectWeapon, IsClose, GetFireError, FireAt, Uncloak, Assign_Destination, Queue_Mission and Enter_Idle_Mode stubbed; Scatter entry-hooked | `aircraft/attack_mission_tests.rs` replays every covered combination through the pure states and state 10 through the production edge picker; the stubbed callees keep their own evidence |
| `tools.spatial_oracle.aircraft_attack_release` | 316 original admitted state4 loops/suffixes, 72 mission and 144 AI pending-ammo prefixes, 21 initialization and 7 ammo gates; selection/FireAt supplied and reveal excluded | `combat/aircraft_release_tests.rs`, `aircraft/release_tests.rs`, constructor/fire-gate tests; bounded control/state parity, not complete projectile, damage, navigation or scheduling parity |
| `tools.spatial_oracle.techno_burst_index` | 54 original FireAt signed increment/remainder cases; GetROF callback records the incremented index and returns20 | `combat/burst.rs`; retained index used by shared emission, not complete rearm or FireAt parity |
| `tools.spatial_oracle.techno_target_burst` | 108 original Unit target/burst/passive-state cases through the setter and Event, EnterIdle and Sticky call boundaries; original vtables, no substituted calls | `combat/burst.rs` compares retained state; production command, movement arrival, mission arrival and pursuit regressions. Excludes preceding native admission, Infantry override and full setter effects |
| `tools.spatial_oracle.aircraft_fire_location` | 51 full original FindFireLocation calls: target identity, native rings/ranking, map/visibility, live reservations, Spawned/Carryall/AirportBound admission and RNG continuation; supplied runtime state, no substituted calls | `util/native_trig.rs` compares candidate geometry and ranked distances against existing deterministic math; production search, destination assignment and state1 integration remain required |
| `tools.spatial_oracle.fly_destination` | 26 original non-null Fly MoveTo calls: retained XYZ, signed-cell landing refusal, power gate, signed Ammo/Target height substitution, FlightLevel fallback, mode/readiness suffix; no substituted calls | `world/fly_height_tests.rs` compares retained destination and refusal through the air order boundary, plus production cell orders and save/restore; moving/mode suffix, null/Stop and full Aircraft/Foot navigation remain unported |
| `tools.spatial_oracle.track_destination --null-boundary` | 84 original Foot/Aircraft NULL setter calls over constructed Drive/Ship locomotors: current/queued Attack and target gate, clear/Stop/timer order, NULL admission bypass, flags, power, already-null NavCom, skip latch and signed timer edges; RNG continuation | `track_destination_null_boundary.{json,meta.json}` bounds evidence to Foot/class NULL control and timers. The Aircraft vtable with Drive/Ship is an explicit boundary probe; native Fly Stop and stock Aircraft flight are excluded. |
| `tools.spatial_oracle.magnetron_aircraft_lift` | Ten original hit/Imbue/class-destination/Jumpjet histories, bounded release and restore gates, two Aircraft landing gates, and five terminal impact/Notice controls | `sim/locomotor_lift_native_tests.rs`, `world/crash_tests.rs` and Jumpjet landing tests consume the bounded native results. [Scope and reproduction](spatial_oracle/magnetron_aircraft_lift.md) separates supplied states and impact effect leaves from the physical ORCA/BEAG whole-frame Rust regression. |
| `tools.spatial_oracle.fly_takeoff` | 80 original callbacks with live facing histories, strict height thresholds, Carryall landing base, bridge/slope, signed ROT and both flag clears; no substituted calls | `world/fly_height_tests.rs` compares production callback fields; existing FacingClass owns both turns. Excludes BeginTakeoff, landing and full flight |
| `tools.spatial_oracle.fly_takeoff_phase` | 75 full original phase-dispatch calls with actual Foot/Techno/Object Mark and Display, health/flag gates and same-layer reordering | `world/fly_height_tests.rs` compares the production pure-takeoff transaction and save/restore continuation; excludes preceding Process motion, landing and non-Landable branches |
| `tools.spatial_oracle.fly_landing_phase` | 29 full original accepted ordinary landing phases: retained Top-to-Ground resubmission, threshold/latch, air-tracker removal, neighbor-counter migration, destination clear/path timer and unchanged RNG | Evidence for the pending Rust landing callback; no substituted gameplay calls. Covers already-OnBridge state, not changed-layer phase suffixes, refusal/search/destruction, AirportBound docking, Carryall/type+C95 effects or full flight/rendered parity |
| `tools.spatial_oracle.fly_paid_step` | 199 original paid-motion ranges with real Primary.Current, Fly speed getter, type Speed conversion and trig; signed/Q16 speeds, full headings, active turns and boundaries | `world/fly_height_tests.rs` compares all numeric candidates and196 in-grid production steps, plus save/restore; map-edge correction, IsMoving admission, slowdown/navigation/landing remain required |
| `tools.spatial_oracle.fly_target_speed` | 374 original Process target-speed writes (`4CE145..4CE2E5`) with real Aircraft/Unit, IFlyControl, GetWeapon and GetHeight: the SlowdownDistance ratio and cap (zero and negative distances included), the 0.1 floor and its 85-lepton stop-and-halve, the zero-distance clamp and 0.05 creep; `4D0180` lock/landing/cruise/FlyBy/strafe/fighter/Inviso/unarmed/Ammo precedence; HunterSeeker; the health, landing, half-height takeoff and null-destination gate. Distance and type locals supplied; no substituted calls | `movement/air_movement.rs::tests::native_target_speed_rows` replays every row through the production writer (each speed within one Q16 quantum); `spawn_manager_tests::every_hornet_of_a_carrier_wing_flies_a_whole_strafe_pass` drives it through `advance_tick`. Excludes the ramp, Horizontal_Step's arrival arm and the landing trigger |
| `tools.spatial_oracle.aircraft_mission_only` | 96 original Aircraft Unlimbo flag suffix / empty-selection click-gate pairs; retained history, failure, type gates, base/elite Camera | `world/aircraft_deployment_tests.rs` and `app/input/entity_pick.rs` compare production reveal and click selection; excludes full Unlimbo, reinforcement bodies, nonempty/forced selection and flight navigation |
| `tools.spatial_oracle.fly_map_edge` | 78 original Fly candidate-admission ranges with Aircraft/Team/Script/map/RNG callees; FlyBy, raw/queued missions, cursor/waypoint height, nudge, single scatter and coordinate refusal | Native evidence for the pending production port; supplied Team states exclude creation/activation, and execution stops before SetCoords/Mark/height effects. Pins zero/one RNG draw and continuation; no Rust parity claim |
| `tools.spatial_oracle.team_creation` | 39 original TeamType creation paths, including trigger4 dispatch and complete Team/Script/Tag/Trigger constructors; owner/limit gates, initial state, waypoint, registry publication and timer RNG | Supplies allocation storage only. Native evidence for pending Team/Trigger lifecycle integration; excludes definition loading, recruitment/activation, Spring and deletion. No Rust parity claim |
| `tools.spatial_oracle.trigger_type_flags` | 64 original TriggerType flag-reader cases with ReadString512, strtok and atoi: disabled polarity, missing/empty tokens, nonzero values, overflow and prior-field retention | Rust compares all 29 applied fresh-definition cases for flags9C..9F. Reload retention and fieldA0 remain evidence only. Excludes House/link/name resolution, Events/Actions and live Tag/Trigger lifecycle |
| `tools.spatial_oracle.trigger_action_values` | 61 original TAction constructor/read/global-local dispatch cases: parameter type versus operand, token8 overrides, empty tokens, signed overflow and variable bounds | `sim/trigger_runtime_tests.rs::parsed_variable_actions_match_native_reader_dispatch_and_restore` compares parsed actions through production frames and save/restore. Excludes named sound/theme/speech lookup and live timer-reset fanout |
| `tools.spatial_oracle.trigger_event_records` | 54 original counted reader cases: variable record widths, numeric/type-name materialization, reversed event list, timer/elapsed/global/local predicate samples | `sim/trigger_runtime_tests.rs::parsed_event_records_match_native_list_and_production_predicates` compares parsed records and supported predicates through production frames. Excludes resolved Team references, nonempty TechnoType scans and live Trigger completion/timers |
| `tools.spatial_oracle.tag_lifecycle` | 49 original histories: shared/fresh Tags, reversed linked execution, repeat modes/refcounts, Object/cell attachment and detach, completion bits, timer/RNG/reset fanout, force/enable/disable, deferred destructors and Logic polling/list mutation | Evidence for the pending live-instance migration. Supplies allocation/free storage, OS pointer probing and initialized registries; no gameplay calls replaced. Excludes scenario registration classification/countdown expiry, Team/physical Object destruction and gated House-win/cursor updates; no Rust live-instance parity claim |
| `tools.spatial_oracle.building_sale_triggers` | Original sale-completion sentinel, live destruction callbacks and post-callback refund on a physically constructed Building | [Boundaries and reproduction](spatial_oracle/building_sale_triggers.md). Rust compares the shared callback and accounting owners; a separate physical ALL01 regression drives ordinary SELL through build-down and the authored SpySat_Boom map actions. Full native campaign execution and rendered parity are excluded. |
| `tools.spatial_oracle.techno_rearm` | 267 original GetROF numeric/RNG cases with explicit virtual query substitutions; original RTTI leaves separately establish Unit1, Aircraft2, Building6, Infantry15 | Evidence for rearm migration; Unit delay and Building one-frame shortcut identities corrected; no complete production rearm parity claim |
| `tools.spatial_oracle.building_fire_turn` | 140 original voxel-building facing retry decisions using raw Type ROT; stops before Snap/GetFireError | `combat/combat_turret_facing_tests.rs` drives the production fire receiver against every decision |
| `tools.spatial_oracle.building_voxel_flh` | 13 original `BuildingClass::GetFLH 0x00453840` coordinate cases covering its `+0x16C5` arm and disabled/primary-pixel precedence controls: the type's TurretAnimIsVoxel/X/Y, PrimaryFirePixelOffset/DualOffset and merged FLH, the `+0x388` heading snapped by `0x004C9300`, burst parities, the stock GTGCAN 3/28 and YAGGUN 0/15 inputs, and the `+0x16C6`-clear prefix; each row records GetFLH's coordinate, the building's own `GetCoords 0x00459EF0` and the reached addresses (the disabled control never reaches `IsometricPixelToWorld 0x006D2070`) | `combat/fire_coord_building_tests.rs::building_voxel_turret_shot_origin_matches_the_original_flh` replays every row through the production rules/ART readers; `retail_voxel_turret_inputs_match_the_native_rows` checks the stock inputs against the retail layers and `app/presentation/fire_effects.rs::stock_voxel_turret_shot_positions_bullet_flash_and_report_together` drives a real Grand Cannon shot through `advance_tick` and checks its bullet, bound muzzle AnimClass and positioned report sound |
| `tools.spatial_oracle.walk_direction_table` | All 65,536 heading words: original sine/cosine indexes and table bits, plus the complete retail table bytes | `src/util/native_trig.rs::tests::every_walk_heading_uses_the_original_trig_entries`; full-heading paid Walk displacement |
| `tools.spatial_oracle.paradrop_coordinates` | 393,216 original DropPayload coordinate prefixes: all 16-bit headings, both sides and three world origins; original full-word turn, trig and final-coordinate truncation | `sim/aircraft/drop_payload.rs` compares every native digest plus landing-cell/admission regressions. [Boundary and limits](spatial_oracle/paradrop_coordinates.md); excludes native downstream placement and complete mission parity |
| `tools.spatial_oracle.crate_speed_effect` | 23 original speed-crate recipient loops followed by live Foot speed queries; class/owner/factor gates, native 3-D distance and announcement flag | Native evidence for the open pickup/Foot speed dependency; no Rust production parity claim |
| `tools.spatial_oracle.flight_level` | 30 original type FlightLevel reads and effective-height queries, including the exact -1 fallback | `rules/object_type.rs`; Fly construction, attack recovery and paradrop carrier initialization use the resolved type value |
| `tools.spatial_oracle.fly_height` | 144 bounded original vertical steps with real GetHeight/SetHeight, Aircraft interface and type getter | `movement/fly_height.rs` compares all vectors; `world/fly_height_tests.rs` exercises healthy production height updates. Excludes complete Process admission, crash, descent drift and phase/Display transactions |
| `tools.spatial_oracle.refinery_dock` | 94 original War Miner refinery rows on real Unit/Building vtables: the Radio core HELLO/OVER_OUT and the Techno/Foot/Unit/Building receivers (18), Building DOCKING `0x0E` through NEED_TO_MOVE, MOVE_HERE, TETHER and PREPARE_TO_DOCK (19), `FootClass::Mission_Enter` (9), Mission_Harvest states 2 and 3 (9), the harvester branch of `UnitClass::Mission_Unload` through its dumps, purifier/AI-virtual/IncomeMult payouts, state 4 and contact loss (30), the PerCellProcess DOCK_NOW arm (6) and the StageClass tick (3), with every transmit reply and RNG draw. Enter_Idle_Mode, Scatter and the animation producers are recorded, not run; Find_Docking_Bay, Find_Nearby_Passable_Cell and Ready_To_Commence answers are supplied | `world/refinery_dock_oracle_tests.rs` replays every row through production `radio::receive`, `miner::refinery_dock` and Mission_Harvest states 2/3 (IncomeMult 0.9 pays 899 natively and 900 in VERA's integer economy, the documented residual); `world/refinery_dock_cycle_tests.rs` runs a whole visit through `advance_tick` |
| `tools.spatial_oracle.jumpjet_infantry_actions --default-motion` | 54 original Teleport constructor/MoveTo/Stop controls followed by the whole Infantry sequencer and DoAction, using physical ClegSequence; retained Doing/Stage/timers and all three complete RNG states | `movement/infantry_action_tests.rs::retail_teleport_default_action_matches_the_native_sequencer` uses retail rules/ART and the common motion query. [Scope and reproduction](spatial_oracle/infantry_default_motion.md); excludes full Teleport request lifecycle, subcell resolution parity, Chronosphere and full Infantry AI. Default mode retains its existing 527 Jumpjet rows |
| `tools.spatial_oracle.cmin_dock` | 50 original Chrono Miner refinery-return rows on the refinery_dock fixture with a Teleporter type, an original TeleportLocomotionClass and Drive piggybacks: the Unit setter `0x741970` and its Teleporter arm (17), Teleport Move_To's refusals (4), the Teleport Process warp with its delay tick (6), Mission_Harvest states 0, 2 and 3 with the Teleporter branches (15), `FootClass::Mission_Enter` (5), Mission_Unload's turn (1) and the PerCellProcess DOCK_NOW arm (2), with every transmit, CoCreate, piggyback swap and RNG draw. AnimClass, VocClass::PlayAt, crate pickup, Per_Cell_Process inside Process, Search_For_Tiberium and the zone lookup are recorded, not run | `world/cmin_dock_oracle_tests.rs` replays every row through the production Unit setter, `teleport_move_to`, the object turn's warp, Mission_Harvest and `miner::refinery_dock` (five rows whose prestate or Move_To destination resolution VERA does not represent are listed and skipped); `world/cmin_dock_cycle_tests.rs` runs close, far and retail-rules visits through `advance_tick` |
| `tools.spatial_oracle.harvest_field` | 69 original ore-field rows on the refinery_dock fixture with supplied overlay/Tiberium tables: `CellClass::Reduce_Tiberium` with the real TiberiumClass spread/growth queue inserts and their Scenario draws (16), `FootClass::Scan_For_Tiberium` with `Is_Cell_Harvestable` and the live Unit Can_Enter_Cell (17), `Search_For_Tiberium_And_Move` (4), `UnitClass::Harvest_Ore_Tick` (11) and `UnitClass::Mission_Harvest` states 0 and 1 (21), with every Rate epilogue draw. Can_Reach_Zone answers are supplied (reachable unless listed); RecalcAttributes, radar and tactical redraw calls are recorded, not run | `world/harvest_field_oracle_tests.rs` replays every row through production `tiberium::reduce_tiberium`, `miner::ore_scan` and Mission_Harvest states 0/1 with the Scenario RNG seeded as the oracle's, so spread priorities, cursors and delays compare exactly (two rows whose supplied answers the scene cannot hold are listed and skipped); `world/harvest_field_cycle_tests.rs` runs fields through `advance_tick` |
| `tools.spatial_oracle.harvest_attack_return` | 53 histories extending the same field owner: original Event/Stop construction and execute, Unit pointer expiry, Attack tail dispatch and targetless Unit idle, current-mission cadence/RNG/epilogue, Ready/Commence, selected TechnoAI Stage and UnitAI harvesting-latch prefixes, resumed ore consumption, timer/gate/retask controls, non-harvester Harvest hold, linked-contact BREAK then Enter idle with retained NavCom, and ten armed/unarmed idle setter controls. Selected physical HARV/General/Mission readers and Dock factories run separately; live Approach return, map/House prestate, full damage/world-detach/UnitAI and scheduling remain bounded. [Schema, native boundaries and reproduction](spatial_oracle/harvest_attack_return.md) | `world/harvest_attack_return_oracle_tests.rs`; production `world/harvest_field_cycle_tests.rs` command/combat integration. Native corpus checks and Rust replay validation are separate claims. |
| `tools.spatial_oracle.time_to_build` | 332 original `TechnoClass::Time_To_Build` (`0x6F47A0`) totals on fixture objects: costs and BuildTimeMultipliers per class, BuildSpeeds (including percent-only doubles), country multipliers (including the defense slot), power ratios and low-power clamps, factory counts x MultipleFactory, walls, and the retail test world's MTNK, FV and E1 | `sim/production/factory.rs` `time_to_build_matches_the_native_oracle`; which objects reach it, the rate division and the cadence are out of scope |
| `tools.spatial_oracle.factory_cadence` | The original build start `0x4C9EA0` (Ghidra `FactoryClass__SetRate`) for 14 Time_To_Build values across both rate clamps, and 10 whole builds under `FactoryClass::AI` (`0x4C9B20`) once per frame with the real Available_Money/Spend_Money: every step attempt, holds for want of money, a deposit retry and completion | `sim/production/factory.rs` `start_rate_matches_the_native_start` / `step_all_matches_the_native_cadence`; `production_replay_tests.rs` `retail_builds_step_at_the_native_frames` (retail MTNK, FV, E1 through `advance_tick`). What starts, suspends or resumes a factory and later rate rewrites are out of scope |
| `tools.spatial_oracle.group_spread_gates` | 524 target/candidate Cell pairs through two slices of the original group spread `0x0064CDA0`: the target height (`0x0064D296..0x0064D2E2`, with both real `0x005657A0` lookups) and the reservation/height-band exits from `0x0064D53D`; signed levels, flag `0x100` on either Cell, `0x8000`, and every other flag bit set. The zone compare, Can_Enter_Cell and the distributor are not executed | `world/group_spread_gate_tests.rs` compares `spread_height` and `within_spread_band` on production terrain cells |

Run them as modules (`python -m ...`). Imports do not emulate or write files;
`--help` works without retail configuration. `--output <path>` selects another
reference. A missing reference or mismatch fails; checking never creates or fixes it.
`--write` explicitly replaces the payload and its `.meta.json` sidecar. Review any
changed values against native evidence before accepting them. This is not a way to
make failing Rust tests green.

The NULL boundary corpus preserves the ordinary `track_destination` corpus. Run
`python -m tools.spatial_oracle.track_destination --null-boundary --check` to replay
it. Aircraft `0x41AA80(NULL,flag)` jumps straight to Foot `0x4D94B0`; Foot clears
NavComAux at `0x4D94C7`, clears NavCom at `0x4D9510`, and calls the active locomotor's
Stop at `0x4D96B9` unless its Aircraft/current-or-queued-Attack/live-target gate
skips it. After a Stop, `0x4D96BC` clears a re-target's NavCom again. Both paths reach
the timer tail `0x4D96C2..0x4D9707`: blocked false, blockage timer started from
Rules `+0x1768`, and movement timer started with duration zero, preserving retries.
The corpus observes each boundary and executes the timer writes, including frame
and signed-duration extremes. Its paired original Drive/Ship is intentionally a
supplied locomotor boundary: it does not establish Aircraft Fly Stop parity.
Fly `0x4CCFD0` re-enters the class setter through airfield `0x41A160` or landing-cell
`0x418E20` selection, whose docking, occupancy and Scenario RNG effects require
their own complete mechanism. Jumpjet Stop `0x54B4D0` reads owner location
(`0x54B53E..0x54B583`) and searches from that cell; it does not read old NavCom.
Accepted Jumpjet touchdown clears its locomotor destination/moving byte before
PerCell (`0x54C8CB..0x54C8F0`), then calls the class NULL setter unconditionally
(`0x54C8FF`). The original instruction packet is
`spatial_oracle/track_destination_touchdown_control.json`; reproduce it with
`python -m tools.native_inspect disasm 0x54C8CB --bytes 58`.
The no-target production regression is in `world/per_cell_object_turn_tests.rs`;
the shared NULL corpus provides the timer arithmetic comparison.

Sidecars identify the Unicorn Python binding and native core versions,
entries, fixture assumptions, substitutions, coverage, and canonical payload hash.
Metadata changes fail checks even if outputs match, prompting review of the changed
environment. Existing references without sidecars remain checkable, with an explicit
message that their historical environment is unknown. Regeneration can record the
current environment; it cannot recover historical provenance.

Sidecars pin no hashes of repository sources (AGENTS.md), so an edit to a shared
helper fails no `finish_vectors` check whose native output is unchanged. The runner
still hashes the sources a producer declares in `source_paths` and refuses to compare
or publish if one changed while it ran. `replay_command` records how the payload was
made; it is not evidence. A sidecar from another branch that still carries the old
`source_normalized_lf_sha256` pin passes; `--write` drops the field.

Some older producers still keep source hashes in fields of their own, such as
`harness_sha256`, `sources` or `source_pins`, and compare their whole sidecar. An edit
to such a source fails their `--check` until those fields are removed (#1137).

## Add a comparison

Start with `tools/color_oracle/hsv_to_rgb.py` for a complete example. Establish the
body, active callers, calling convention, input/state ownership, and observable
outputs before constructing the fixture. Select cases that exercise meaningful
branches and boundaries. Avoid a second Python implementation of the algorithm.

* `call(address, ecx=..., stack_args=[...], writes={...}, dumps={...})` maps verified
  code in a fresh emulator for each invocation. Entries must lie in an original
  executable section; fixture writes cannot replace native instructions. Carry only
  the native state your comparison deliberately preserves between calls.
* For an interior block, use `load_image(uc)` and `run_checked(uc, begin, end)` after
  establishing its live registers, memory and x87 state. Multiple legitimate stop
  boundaries can be a tuple. This low-level API supports custom fixtures; it cannot
  certify their initialization or detect every patch a caller makes.
* Declare necessary intermediate addresses with `required_addresses=[...]` when
  reaching an endpoint alone could conceal a bypass. They record instruction-hook
  visits, not proof of instruction effects or of full path equivalence. Endpoints
  stop **before** executing their instruction and cannot be required addresses.
* Use `finish_vectors(generate, path, provenance=lambda: provenance(...))`. Lazy
  callables keep help and argument errors independent of native work. Record the
  scope, assumptions, substitutions (including none), and native entries explicitly.
  Pass `source_paths={label: Path(...)}` for the producer and its relevant helpers:
  the shared publisher hashes them before generation and refuses to compare or
  publish if one changed through metadata construction. This is a run consistency
  check; nothing is recorded. Do not hash sources into metadata fields: sidecars
  pin no repository sources, and `--check` compares every field but
  `replay_command`. Do not list the Rust consumers either: the native run executes
  no Rust, and their own tests compare them with the corpus.
* Record no hash of `gamemd.exe` in the payload or provenance: not `image_sha256()`,
  `NATIVE_SHA256` or a digest of `image_bytes()` (see Evidence identity).
* Consume native outputs in a focused Rust test of the actual production function.
  Assert case identity/coverage as well as output equality, and review the connection
  to callers independently. The HSV example checks 2,304 inputs, not all 256³ inputs
  or final rendered colors.

## Investigating a failed native run

`run_checked` raises `NativeExecutionError` (an `OracleError`) on unsuccessful
execution. Its `diagnostics` dictionary preserves the immediate Unicorn timeout
flag, observed instruction count/budget, entry and expected endpoints, last 16
observed instruction addresses, required/missing visits, and entry/failure
registers. Each register snapshot includes FPCW and up to 16 readable words at
ESP; unmapped or partially readable stacks are reported without masking the
original failure.

The reason distinguishes `timeout`, `fault`, `early_stop`,
`instruction_limit_reached` and `required_addresses_missing`. The instruction
count is hook observations, not a retirement counter. Reaching the budget does
not rule out an external stop on that same instruction; a timeout flag does not
explain why the operation was slow. Reports diagnose a rejected run, never prove
native output or a complete initialized game state.

Set a directory to save each failure as a new JSON file, including failures in
older generators that call `run_checked` before `finish_vectors`:

```sh
VERA20K_NATIVE_FAILURE_DIR=/absolute/diagnostics python -m tools.color_oracle.hsv_to_rgb --check
```

In PowerShell, set `$env:VERA20K_NATIVE_FAILURE_DIR = 'C:/diagnostics'` first.
Success writes nothing. The directory is created when needed; unique files use
exclusive creation and do not replace earlier failures or golden files. The
exception names its `report_path`. A failure to write diagnostics is reported
alongside the original native failure. Without the environment variable, the
structured diagnostics remain on the exception and no report file is written.

Custom fixture owners should name the case and relevant supplied inputs:

```python
run_checked(machine, entry, stop, context={
    "case": "unfunded-repair",
    "inputs": {"seed": seed, "frame": frame, "credits": 0},
})
```

Context must be a JSON object and is frozen before execution. `call` accepts the
same argument and adds a reserved `call` object containing ECX/EDX, stack
arguments, FPCW, observation mode and fixture write ranges. Reports do not copy
the full heap, pointed-to data or executable and cannot restore an emulator.
Include the fixture selectors needed to reproduce the case. `run_checked` also
accepts synthetic tests.

The shared `Reader.invoke(..., context=...)` forwards this context. Anytown
`Mission.setup` supplies its fixture class, phase/frame, continuation inputs and
the current physical INI layer's name/path/SHA-256 for General and Radiation
Rules reads. A repeated-case driver can call
`fixture.setup(context={"case": "unit-unlimbo", "placement_index": 7})` to add
the precise case selector; `setup` and `rules_layer` are filled by the owner.
Other custom generators must supply their own selectors; registers alone cannot
identify a case whose input data lives behind pointers.

## Execution guarantees and limits

Unicorn can return normally when its instruction or time budget expires. The runner
therefore requires a declared endpoint and rejects timeouts, faults, unexpected
stops, and missing required visits. Failures include recent instruction addresses.
Existing custom exit configuration is disabled so it cannot override those boundaries.
Budgets default to five million instructions and ten seconds; choose a tighter
instruction budget for small leaves. Existing hooks remain the fixture owner's
responsibility. No zero-return or unmapped-memory fallback silently invents an answer.

The loader preserves the legacy broad RWX image mapping and zero-filled BSS. It is
**not a Windows loader**: imports, constructors, TLS, OS services, and runtime global
state are not initialized. A mapped read is not evidence that its value is realistic:
the cell-offset table `0xABD490`, for one, reads all zero until its static initializer
`0x561910` runs (`tools/superweapon_oracle.py` runs it first).
Declare supplied state and hooks; substituted function results validate only the
remaining computation. Whole-game, scheduling, device, and GPU behavior require
additional evidence.

The legacy default x87 control word is `0x0E7F` (53-bit precision, truncation); verify
the appropriate ambient state for each native entry. `capture_st0=True` executes an
external FSTP observation stub and stores **binary64**, not the full 80-bit register.
The stub must finish before a result is returned. Unicorn transcendental instructions
such as FYL2X may differ from physical x87 hardware. Gaussian vectors retain that
caveat; equality is not hardware parity. Floating-point comparisons preserve signed
zero, and native bit dumps remain preferable where precise representation matters.

`tools/rmg_oracle/harness.py` keeps legacy imports working through the verified loader
and checked `call`. Other scripts that directly invoke `emu_start` have **not** all
been migrated. In particular, do not infer completion checking or read-only CLI
behavior for the entire oracle directory from the listed examples.

Run the runner's synthetic failure checks with:

```powershell
python -m tools.run_tests
# Focus only the shared native-runner failure contracts:
python -m unittest tools.test_native_oracle -v
```

These test runner behavior, not retail behavior. The tests cover limits, faults,
premature stops, alternate exits, missing paths, fresh state, FSTP completion,
executable selection, legacy `native_sha256` handling, executable-hash refusal,
reference preservation, provenance mismatches, and diagnostics.

## Saved-map browser comparisons

The browser evidence
links three additional bounded comparisons: `tools.storage_oracle.seed_order`
(original qsort and timestamp comparator, 36 cases), `tools.storage_oracle.crt_random`
(supplied TLS seeds, 224 draws), and `tools.storage_oracle.saved_scrollbar`
(original x87 thumb arithmetic, 450 supplied geometries). Each supports `--check`
and explicit `--write` through the common runner and records provenance beside
its JSON payload. None establishes native full-window visual parity or a live
first-save RNG state.

## Upstream references

* [Official Unicorn tutorial](https://www.unicorn-engine.org/docs/tutorial.html):
  explicit memory mapping, CPU state and emulation setup.
* [Unicorn 2.1.4 public API](https://github.com/unicorn-engine/unicorn/blob/2.1.4/include/unicorn/unicorn.h):
  `uc_emu_start` budgets, `UC_QUERY_TIMEOUT`, hooks, and exits overriding `until`.
* [Unicorn 2.1.4 execution implementation](https://github.com/unicorn-engine/unicorn/blob/2.1.4/uc.c):
  the instruction-count hook stops emulation normally; success alone does not
  establish that a native return was reached.


### Building animation slot display order

`python -m tools.spatial_oracle.building_slot_replacement --display --check`
replays the existing owner's separate `building_slot_replacement.display.json`
corpus. Set `VERA20K_BUILDING_ANIM_ART` only when the physical `ARTMD.INI` is
outside the checkout's `ini/`; the result records its content hash, not its path.

The original BuildingType constructor and ART slot reader establish NATBNK's
four normal/damaged wall names, slot ZAdjust/YSort, CAOILD's Active YSort and
GAHPAD's nonzero Active Y offset. Original `45197B..4519A8` copies slot values
into supplied Anim objects, and `422BC0` queries their retained sort key.
Two insertion histories then execute Display submission with actual Anim/Unit
vtables: back wall, installed Unit, front wall. The scene coordinates, Ground
layer, initial objects and vector capacities are supplied controls. This is
not full Anim construction, frame-timer or whole-scene pixel parity.

Rust tests
`building_art::slot_tests::bunker_and_active_slot_sort_adjustments_match_original_read_and_copy`
and `app::presentation::instances::shp::building_anim_tests::` connect those
values to slot construction and the production renderer's independent Anim
Display output, including normal/damaged names, atlas frame Start, offsets,
hidden/removed objects and replacement. The explicit ignored GPU regression
`render::depth_gpu_tests::bunker_front_wall_interleaves_with_indexed_voxel_without_writing_z`
runs the production SHP/VXL shaders on synthetic texels and reads the output.
It proves bounded GPU ordering/occlusion, not retail RGB equality; a production
NATBNK/LTNK played comparison remains necessary.


### Stock cash-crate joined control

`python -m tools.spatial_oracle.crate_pickup --cash --check` consumes
[`cash_crate.json`](spatial_oracle/cash_crate.json) and its binary/command
sidecar. `--write` deliberately regenerates this separate corpus; the default
28-row pickup/prefix corpus remains independent.

The existing `AnimationFixture`, source-map fixture, Overlay constructor and
Sound reader compose one VM. Original `481A00` reaches weighted/fixed Money,
`56C020` removal, optional `56BD40` replacement/nearby search, amount RNG,
`4F9950` credits, separate retained-Cell sound/tail height queries, and MONEY
Anim construction, AI expiry and deferred cleanup. Physical RULESMD Powerups
and CrateRules readers, the exact Overlay Crate/CrateTrigger reader block,
SOUNDMD CrateMoney binding and ARTMD MONEY reader run before pickup.

Inputs declare a neutral local Unit/House owning a building, no attached Tag,
a small allocated flat map, Size/LocalSize, resident TMP metadata and spare
registry capacity. Original Resize field stores derive MapRect; each row retains
its observed raw/final LocalSize, geometry and runtime-ID cursor boundaries.
Full original `567230` normalizes LocalSize before the independently chosen
interior pickup cell `(11,11)` is placed. The input records the inherited
AnimationFixture stored game speed read back from `A8EB60`; MONEY normalization
uses that supplied option through original `5FB2E0`. Supplied slot storage starts zeroed;
only its coordinate sentinel determines emptiness.
Initial placement runs native code; pickup seeds are independent priors.
Full RNG states and ranged/raw draws are observed at removal, replacement,
credit and cleanup boundaries. Seed2 directly selects Money from stock weights.
The sound reader uses its existing fixture-local sample-slot boundary for
physical `ubonus`; native lookup chooses the Voc identity. Playback ends at
`7509E0`, so device admission/channel scheduling and its RNG are outside this
control. Cash opts into original `Object::Mark5F5850` admission and state writes for
Overlay and Anim, recording before/after flags and native refusal controls. Consecutive identical frame
Mark observations retain their native call count in one compact row.
The historical animation-only Mark seam remains its default. Display submission
`4A9720` still returns supplied success; the selected Object Unlimbo caller
ignores that result. Screen rectangle/dirty outputs remain supplied. This is a
bounded native comparison, not a complete scenario/House/Unit initialization,
rendering, audio playback or whole crate-function equivalence proof.

### Sonic Wave packed-surface controls

The existing Wave runtime/pixel owner also replays Sonic type0 controls:

```sh
RA2_DIR=/path/to/retail \
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted/rules-art-mode \
PYTHONPATH=.:tools/spatial_oracle \
python -m tools.procedural_drawing_oracle.magnetic_wave --sonic --check
```

The consumed `sonic_wave.json`/sidecar retains original DrawIt75F9F0 →
DrawSonic75FA90 → Pixel75EDF0, the initialized tables, integer spans, displaced
RGB565/RGB555 stores, declared phase/clip/depth/A controls and ordered overlap.
Its actor shells are the existing TELE/MTNK component setup; it does not claim
a legal Dolphin FireAt, full Sonic AI/damage or original scene pixels. The
small full-surface direction5 input reaches outside its supplied RGB allocation
and is an explicit domain-refusal control. A valid cropped control retains that
direction for GPU comparison; no out-of-allocation color is a GPU promise.
The production Dolphin route uses the separate map-observation recipe.
