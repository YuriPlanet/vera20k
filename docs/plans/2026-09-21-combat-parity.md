# Combat parity acceptance and checkpoint

## Objective and acceptance

User direction: the dependency map is retired. Do not consult or refresh it;
this overrides stale map instructions on this owned branch.

Make VERA20k combat reproduce active-retail `gamemd.exe`: targeting, attack
execution, weapons, projectiles, damage, special warheads and destruction,
including all required dependencies. Missing behavior and contradictory evidence
keep the goal open. Commit, publish and merge validated increments.

Before implementing each mechanism, identify native entry/callers, retail inputs,
observable outputs and the Rust production route. Acceptance covers the complete
chain: prerequisite state and ordering, action admission, effects and downstream
consumers, cleanup and save/restore continuation where retained state is affected.
Native execution witnesses and production regressions must declare their coverage;
samples and isolated helper tests do not certify the whole combat system.

The final owner audit must account for combat orders/acquisition/retaliation,
movement and facing prerequisites, weapon selection/reload/bursts/fire timing,
launch coordinates/scatter/guidance/collision, damage and armor, each active special
warhead chain, and destruction/detachment/secondary effects. Audit retail infantry,
vehicles, aircraft and buildings and their interactions. Resolve required omissions
and incorrect behavior; recording them is not acceptance. Follow AGENTS.md for
numeric policy, validation and the single pre-PR critic pass.

State (2026-09-22): the Claude session now owns the goal. Codex's 67 commits
(`feature/combat-foot-speed` @ `3df1f957`, Codex worktree left untouched, its `.local/`
holds the cited logs/probes) land through `feature/combat-branch-landing`, the single
exception to per-mechanism PRs. Its only critic is
`Documents/vera20k-handoff/2026-09-22-combat-branch-review.md` (+ `coverage.md`); the
Codex ledger's hold-merge and rendered-flight gates are void. Codex's uncommitted
deferred Drive/Ship path request is parked, not landed, on `wip/track-order-deferred-path`
(`5966799c`; 31 failing tests, triage in the review's `wip.md`). After landing, choose
mechanisms by player-visible combat gaps; one PR and one critic pass per mechanism.

## Landed mechanisms

Merged to main:

- **Infantry fire-start facing, PR #439** (merge `d47a9247`, source `c0b854a9`). `InfantryClass::AI`
  calls `Fire_At_Target @ 0x005206B0` at `0x0051BF59`; `0x00520904..0x00520925` sets pending byte
  `+0x68D`, derives direction via `0x005F3DB0` and calls `FacingClass::UpdateFacing @ 0x004C9300`
  on body `+0x388`; vtable `007EB058+3CC -> 0051DF60`, where `0051DF70` clears `+68D` before
  `TechnoClass::Fire_At`. `GetFireError @ 0051C8B0` refuses Foot `+578 > binary64 0.1`
  (`0051C9B8..0051C9C9`, error 7 at `0051CAFA`). Rust: the fire-start receiver owns the facing
  update and `FootSpeedState.applied_fraction` gate; FacingClass::snap equality cancels the timer,
  keeping the old target. Evidence: `infantry_fire_start` 59, `infantry_fire_speed` 11. Tests: ten
  regressions incl. full-frame fire/damage and immediate/pending save/restore.
- **Paid Walk step, PR #440** (merge `a37e8118`, head `20bf1af0`). `WalkLocomotionClass::ProcessMovement
  @ 0075AEC0` arm `0075BD25`: `0075BFA9` sets Foot speed 1, speed `+538 -> 00521D80`, facing
  setter `0075C035`, signed sin/cos with truncation `0075C067..0075C0CB`; completion
  `75BD70..75BF82` has no turn. Rust `walk_step::advance` with shared `foot_speed` and prone override;
  normalized Walk arm deleted. Evidence: `walk_paid_step` 40, `walk_direction_table` 65,536, `walk_completion` 42.

On `feature/combat-foot-speed` (unmerged; runtime increments passed full lib + Clippy when committed,
evidence-only increments ran only their native checks):

- **Foot speed +580** (snapshot 181). `FootSpeedState` owns binary64 +580 (init 1.0), used before
  FASTER; Drive/Ship, Walk, Hover and tube exit use the live getter. Evidence: `crate_speed_effect`
  23, `crate_pickup` 28, `track_speed_native` 75+116. Pickup helpers have no production caller.
- **Facing semantics** (`be8e2d62`). Signed SetROT `4C9680`/ctor `4C91E0`: clamp only >=127, shift
  low byte; -1 instant, -255 turns at 256; epoch FFFFFFFF keeps full duration. Building retry
  `44B068` uses unclamped abs signed16 raw ROT<<8; Infantry lazy body `517BBD` rate 127.
  Evidence: `facing_class` 61, `building_fire_turn` 140, `walk_first_step` 10.
- **Display layers** (`17520993`, `c3e4876f`, `abc880e9`; snapshot 184). `world/display_layers.rs`
  owns five ordered vectors: Submit `4A9720`, Remove `4A9770`, one Ground sort `551A30` at MainTick
  `55DBC8` before Logic `55DC9E`. Anim +104 YSortAdjust (`422137`), GetYSort `422BC0`, SetOwner
  `424B50`, expiry `425150`, Layer lookup `477050`/`48E050` (default Air, `4276D4`); damage fires
  expire before Destroy `43BDE8`. Draw `6D3D10 -> 6D8DB0` reads `8A0360` forward, unsorted. Also Bullet ART
  Flat (`3aee9c65`, `46C1E8`/`46C28D`), CruiseHeight (`665C3A` 400, `67447E`, retail 500). Evidence: `display_anim_owner` 24,
  `anim_layer_rules` 19, `anim_damage_fire_expiry` 8, `display_entity_layer` 88, `display_non_entity` 9, `crate_ground_membership` 6, `flat_art` 38.
  Tests: `terrain_ground_gpu_tests::` (4, `--ignored --test-threads=1`).
- **Fly height, FlightLevel, landing base** (`bae495bf`, `fa615a67`, `450c408b`; snapshot 185).
  `FlyRuntime` owns +38, +50/+51; object Z authoritative. Step `4CDD0D..4CDFBC/4CE145`: climb
  min(delta, IsDropship?16:passenger?10:20), descent 20..50; `4CF3D4..4CF4CF` attack height.
  FlightLevel Type+618 (`717800`, exact -1 -> Rules+7B4); IsDropship Type+C95; landing base
  `41B6A0` (`aircraft/landing_base.rs`), Carryall Type+DFC, UnitRepair +16A9, Helipad +16CB.
  Evidence: `fly_height` 144, `flight_level` 30, `fly_landing_base` 9+249.
- **Aircraft ammo, release, burst** (`07c48b07`, `4a32f4c3`, `9d3be110`, `cd0015a4`, `6ef2fc7a`;
  snapshots 186/187). `AircraftAmmo` owns signed count + pending +6C8; InitialAmmo Type+680
  (`71474C`, -1 at `41403A..41404B`); `6FCA0D` rejects exactly zero; AI `41505E`.
  `combat/aircraft_release.rs`: pending before Burst (`418403`), reselect per FireAt, shared
  `world_receiver::emit_admitted_fire`, `FireCommitBoundary`, suffix `4184C2`. `WeaponBurst` owns
  Techno+3B8 (Assign_Target `6FCF5B`). Removed `AircraftReleaseTail`, `has_fired`/`is_strafe`,
  `burst_remaining`. Evidence: `aircraft_attack_release` 316+72+144+21+7, `techno_burst_index` 54,
  `techno_target_burst` 108, `techno_rearm` 267.
- **Approach range** (`d7c15550`). `5F6440` planar distance, strict vs slot0 raw Range
  (`4180F4..418117`); building centre minus (w+h)*64, clamped 0; 0x0 foundation anchor fixed to
  (-128,-128) in the shared projection. Evidence: `aircraft_approach_range` 235.
- **Attack states 0/1/3**. `world/aircraft_fire_location.rs` ports `4197C0` (one Scenario draw
  0..99); `combat_weapon::weapon_range` owns `7012C0` + OpenTopped cargo minimum.
  `world/aircraft_attack.rs`: 0 -> 1/10; 1 searches, assigns via
  `fly_orders::assign_aircraft_attack_destination` (`41AA80 -> 4D94B0 -> 4CCC80`, live Attack only),
  selects 3/10, writes Rate*900+ScenarioRandom(0,2); 3 classifies strafe (aux+18 `41B7F0`) before
  Fighter (aux+1C `41B840`, Type+E0E). Fly matrix `4CF659` reads Secondary, GetFLH `6F3BCA`
  Primary. Evidence: `aircraft_fire_location` 63, `aircraft_reengagement` 16, `aircraft_approach`
  49. Test: `util::native_trig::tests::aircraft_fire_location_candidates_and_distances_match_native`.
- **Fly destination, takeoff, paid motion** (`16b609e1`; snapshot 188). `FlyRuntime` owns
  +1C/+20/+24 (`4CCC80`, `4CCE25..4CCE6E`). Takeoff `4CE680` runs inside `4CD2A0`'s
  Mark/Display transaction; Aircraft facings from Type+71C ROT (`413F80`), Unlimbo snaps
  `6F6DAA`/`414417`. Paid step `4CDA3C..4CDB4C`, speed `4CFE20`: min(Speed[0..100]*256/100,255) x fraction,
  truncated once (`native_trig::facing_step_world_xy`, shared with Walk). Evidence:
  `fly_destination` 26, `fly_takeoff` 80, `fly_takeoff_phase` 75, `fly_paid_step` 199. Test:
  `fly_paid_step_matches_native_math_and_production_type_speed`.
- **Retained Techno+3D4** (`9de592ea`, snapshot 189). GameEntity `mission_only`: ctor `6F2F55`
  clears; Unlimbo `4143A0..4143F2` sets; paradrop `65E6BE`; empty-selection click
  `69261B -> 692762..692778`. Evidence: `aircraft_mission_only` 96. Pins set at 189 (no later
  rebaseline reported): global 6AA2FA03DF444115, bridge 2395935886825451856, slice6 3C8EAEF3C7685EC1.
- **Fly landing and phases** (`48f56593`, `1ab75310`; snapshot 192). `world/fly_landing.rs` owns
  landing `4CE840`/changed-layer `4CD380`; `world/fly_orders.rs` owns non-null MoveTo and
  BeginTakeoff; explicit AirTracker registration; FlyRuntime keeps moving+34, +52, AirportBound
  (Link `4CCA20`); FlightAttitude +2E8; CanEnter `4196B0`; landing space `4DDC60`. Evidence:
  `fly_landing_phase` 41, `fly_takeoff_entry` 26, `fly_attitude` 65, `fly_can_enter` 102,
  `fly_instance_link` 4, `fly_landing_space` 26.
- **Scatter admission**. `bump_crush::ScatterTechno` reads House CurrentIQ and HasWeaponAbility
  (`481670`); Infantry `51D0D0..51D226` final Fraidycat gate and Foot.Team+5D4 via
  `TeamScriptVm::team_for_member`. Evidence: `cell_scatter` 62, `infantry_damage_scatter` 278.
- **Locomotor motion queries** (`d17dad7c`..`3df1f957`). `movement/motion_query.rs` dispatches
  ILocomotion+10 over Drive/Ship/Walk/Fly (`4CCA90`)/Jumpjet (`54AE50`) owners for cell entry,
  tube exit, forced-blocker and hut Scatter. Evidence: `air_locomotor_moving` 44,
  `unit_entry_air_motion` 288, `jumpjet_scatter_gates` 64, plus the Unit entry/Scatter set below.
- **Trigger flags; Team/Tag evidence** (`7b83fc6c`, `9202149b`). TriggerRuntime on Simulation (definition-wide);
  `map::triggers` fixes token4/difficulty polarity (`7273B9..72749B`). Evidence: `trigger_type_flags` 64;
  evidence only: `trigger_event_records` 54, `tag_lifecycle` 49, `team_creation` 39, `fly_map_edge` 78.

## Native evidence inventory

Run `python -m tools.spatial_oracle.<stem> --check` (`flat_art`: `tools/projectile_oracle`).
Sidecars record binary identity; landing-era SHA-256 `1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

| Stems | Executes | Rows | Coverage limit |
|---|---|---|---|
| infantry_fire_start / infantry_fire_speed | fire prefix; 0.1 compare | 59 / 11 | legality receivers supplied |
| walk_paid_step / walk_direction_table | paid step; lookup | 40 / 65,536 | supplied speed; lookup exhaustive |
| walk_completion / walk_first_step | completion; first step | 42 / 10 | no Process/placement |
| facing_class / building_fire_turn | Facing histories; `44B068` | 61 / 140 | — |
| crate_speed_effect / crate_pickup / track_speed_native | speed; dispatch; getters | 23/28/75+116 | no production pickup/House |
| display_anim_owner / anim_layer_rules / anim_damage_fire_expiry | Anim Display | 24/19/8 | no full AI, destructors, sound |
| display_entity_layer / display_non_entity / crate_ground_membership | GetLayer; Ground | 88/9/6 | Map+140 length fixed |
| flat_art | Bullet ART Flat | 38 | other ART readers uncertified |
| flight_level / fly_height / fly_landing_base | reader; step; MoveTo | 30/144/9+249 | old 136-case height corpus invalid |
| aircraft_approach_range | `5F6440` + range arm | 235 | selected range arm only |
| aircraft_attack_release | release/entry/AI/init/error | 316/72/144/21/7 | NULL FireAt; no Scatter/damage |
| techno_burst_index / techno_target_burst / techno_rearm | burst; setters; GetROF | 54/108/267 | GetROF supplied |
| aircraft_fire_location / aircraft_reengagement | `4197C0`; state1 | 63 / 16 | Unit receivers, flat airborne |
| aircraft_approach | `417FE0` entry/approach | 49 | FLH f32 1-lepton in 3 poses |
| fly_destination / fly_takeoff / fly_takeoff_phase | `4CCC80`; `4CE680`; `4CD2A0` | 26/80/75 | pure takeoff only |
| fly_paid_step | `4CDA3C..4CDB4C` | 199 | stops before placement |
| aircraft_mission_only | +3D4 regions | 96 | no full ObjectSelect/Unlimbo |
| fly_map_edge | `4CDB4C..4CDCFD/4CDD0D` | 78 | supplied Team; Rust not wired |
| team_creation / tag_lifecycle | creation; Tag histories | 39 / 49 | no Rust instance parity |
| trigger_type_flags / trigger_event_records | flags; events | 64 / 54 | no House/link/lifecycle |
| fly_landing_phase / fly_takeoff_entry / fly_attitude | `4CD2A0`; entry; pitch | 41/26/65 | no arbitrary angles |
| fly_can_enter / fly_instance_link / fly_landing_space | `4196B0`; `4CCA20`; `4DDC60` | 102/4/26 | no Team/missing-slot shroud |
| fly_mission_mode / fly_nonlandable_phase / foot_neighbors | `48f56593`, `1ab75310` | 30/64/88 | counts read from tree |
| cell_scatter / infantry_damage_scatter | `481670`; `51D0D0` | 62 / 278 | admission only; no RNG |
| air_locomotor_moving / unit_entry_air_motion / jumpjet_scatter_gates | +10; `73F0A0`; gates | 44/288/64 | no Scatter suffix |
| locomotor_moving / unit_entry_motion / unit_entry | Drive/Ship/Walk; entry | 84/56/150 | — |
| unit_entry_boundary / unit_entry_traversal / track_destination | `4DA1D0`; `4D9C60`; MoveTo | 100/328/126 | — |
| unit_scatter_state / unit_source_scatter | Scatter prefix; selector | 69 / 56 | — |

Also `tools/infantry_scatter_oracle`, `tools/mcv_deploy_oracle`. Pre-branch main harnesses (review): techno_target_scan 171,
vhp_scan 498, distributed_fire 151, foot_attack_move 638, estimated_damage 1066, object_health 718, cell_entry_crush_tail 68.

## Ghidra annotations

All saved and read back; no byte, prototype or boundary edits.

- `005F3DB0` ObjectClass__DirectionToTarget (+ plate); `005206B0` InfantryClass__Fire_At_Target plate
- `0051DF70` raw-byte writer label (clears +68D); `004E0150` body-facing getter label
- `0051C8B0` InfantryClass::GetFireError speed-gate label; `004C937B` snap comment
- `4238B0` Anim Mark (was ProcessCloakMode); `414290` QueryInterface
- `468B90`/`62FE80`/`75F890`/`74A960` non-entity GetLayer; `4CCB40` Fly ILoco_Process
- `54CA90` JumpjetLocomotionClass__State5_Crash (was State5_Touchdown); `54AE50` JumpjetLocomotionClass__Is_Moving
- Missions: `417300` Patrol, `4158E0` ParadropApproach, `415960` ParadropOverfly, `4155F0` SpyplaneApproach, `4157C0` SpyplaneOverfly
- `4197C0` FindFireLocation (was Find_Approach_Cell)
- `4CE680` Takeoff_Facing_Callback (was Ascent_Step); `4CD2A0` Process_Phase_Transitions; `4CFE20` Get_Current_Speed
- `6F1090` TeamTypeClass__ReadINI (was TeamTypeClass__AI); `6F09C0` TeamTypeClass__Create_Team
- `7265C0` TriggerClass__Spring (was a voice-only label)
- `518C00` InfantryReceiveDamage__SurvivorPostludeFragment (was InfantryClass__SetFear)
- `7012C0` TechnoClass__Get_Weapon_Range
- `41B840` AircraftAux__Is_Fighter (23-byte function created)
- Techno `+304` pFireParticleSystem added; `+308` DamageSparkSystem -> SparkParticleSystem
- `4196B0` AircraftClass__Can_Enter_Cell (was ActionOnCell)
- `4CCA20` FlyLocomotionClass__Link_To_Object
- `6385C0` Techno_PlanningPathArrival_QueueWaitEvent (was FUN_006385c0) + plate; plates on
  `4CFA70` Begin_Landing (gates, callers, vtable 7E22A4 +484/+480) and `417FE0` Mission_Attack
  (all states incl. 5..9 bodies)
- Comments, Walk/Display: `75BD97`, `75BFA9`, `75C067`, `746C90` plate, `6D8F39`, `4276D4`,
  `427DF2`, `422961`, `428147`, `425180`, `43BDE8`, `44EA45`, `424801`, `54CC0D`, `4CD4E7`, `41CC67`
- Comments, Fly/facing: `4CF4B6`, `4CE756`, `4142C1`, `4CDE64`, `712336`, `41C8D0`, `41B6A0`,
  `460929`, `4604E0`, `4CF9A5`, `4CFADF`, Foot ctor `4D31EF`, `4C93DB`, `4C93EC`, `517BBD`,
  `44B08E`, `41514C`, `416041`, `413FDE`, `4C9680`, `4CCC80`, `4CCE1C`, `4CCED9`, `4CDA68`, `4CCA90`
- Comments, attack: `41810F`, `418037`, `41B849`, `41505E`, `41403A`, `4143FC`, `41840E`, `6FF27F`,
  `6FCF5B`, `4197EF`, `41988E`, `419A40`, `419C13`, `4184BD`, `41801B`, `418175`, `418229`,
  `4CF659`, `6F3BCA`, `418087`
- Comments, other: `4143EB`, `65E6BE`, `692766`, `41CD6E`, `4CDBE1`, `4CDC37`, `4CDCFB`, `566332`,
  `6EA089`, `6EC300`, `6E53A0`, `726C9C`, `71F4E0`, `55AFB0`, `481670`, `518C56`, `51D200`,
  `51D212`, Teleport `718080`, Foot `4DDC60` (EOL; no function), Foot `4DB800`

## Landing PR: review must-fix resolution

1. Dock/reload: both drivers now call world owners. New `Simulation::begin_fly_landing`
   (`world/fly_orders.rs`, BeginLanding `4CFA70`: planning gate via `6385C0`/Techno+514 always
   passes without planning mode; live-type AirportBound needs radio contact with the building in
   the current cell, else Enter_Idle_Mode `4176F0` via vtable+484) and the existing
   `begin_fly_takeoff` (`4CF950`, AirTracker/facing/AuxSound1). `reserve_airfield_pad` /
   `release_airfield_pad` (`docking/aircraft_dock.rs`) pair every pad reservation with the
   airfield radio HELLO/BREAK that Process_Landing `4CE840` admits on. Landing starts on
   `air_movement::fly_landing_arrival` (Horizontal_Step `4CF520` distance <0x80, speed <0.05),
   because the legacy horizontal adapter never clears its path inside 86 leptons. Production test
   `aircraft::dock_cycle_tests` runs Guard -> RTB -> contact landing -> reload -> BREAK ->
   relaunch into the AirTracker through `advance_tick`.
2. Dead aircraft: `complete_fly_phase` keeps native's unconditional Submit (`4A9720` has no limbo
   gate) but skips Mark(PUT) for a Limbo owner (`5F5850` refuses); store removal
   (`finalize_and_remove_common`) now expires any Display registration. Test:
   `lifecycle_tests::display_registration_expires_with_a_resubmitted_uninit_owner`.
3. RNG order: `aircraft::dispatch_aircraft_mission` runs in each aircraft's LogicVector slot from
   `object_ai_visit_one`, before Fly Process (FootClass::AI `4DA530` runs TechnoAI before
   locomotor +40). Receipts go through frame-local `Simulation::aircraft_fire_requests`.
   Residual: the state4 release itself still runs in VERA's combat phase, like every FireAt.
4. States 5..9: gated. Recorded residual at `attack_mission.rs` (native bodies documented on
   Ghidra `417FE0`); needs the GetFireError producer. First post-landing mechanism.
5. Ground sort: type terms resolve only for Structures through `TypeHandleTable::object` (no
   allocation); Submit reads the new key once; the adjacent pass reuses the carried key.
6. Crates: gated. Foot+580 residual recorded at `components.rs`; pickup is its own mechanism.
7. Duplicate Unit CanEnterCell: scoped. Residual at `cell_entry.rs`; Drive/Ship still classify
   there, `foot_entry` reaches Infantry and repair ==7 only. Consolidate with the Drive/Ship
   Process path owner.
Minor fixed: aircraft Mission+BC single owner (`AircraftMission::Attack`, no handler_state
mirror); FindFireLocation claims read once per search; `ftol_scale` uses the chop53 model like
the speed arm (stock f32-widened inputs unchanged; binary64 0.6 now 29, pinned); 191->192
changelog; overlay_grid comments; three stale docs; spawned children and paradropped passengers
reveal with rules. Follow-up: two Foot->Fly destination owners, fifth ad-hoc VisionConfig,
unloaded passengers' rules-less reveal (`passenger/departure.rs`).

## Open required work

Aircraft attack loop (owner `world/aircraft_attack.rs` + shared admission/rearm):
- States 5..9 collapse to 10 (`attack_mission.rs:124`) though release writes 5/6. 6..8 accept
  GetFireError 0/2/8/9, fire once, Scatter, assign Target, advance, return raw ROF; 9 enters 3 and
  returns signed `(Range+1024)/AircraftType.Speed`; 5 needs error reasons, CurleyShuffle (+17E1),
  busy-3 retry, uncloak on 9, range `6F7780 -> +3A8/6F77B0 -> 6F7220`. Probe
  `.local/probe_aircraft_error.py` (`41A9E0` codes).
- State 10: zero-ammo/targetless return, conditional clear `418C21..418C3D`, RNG/NavCom suffix.
- Release: call `481670(Aircraft+9C copy,1,0,0)` after the FireAt loop, before the suffix.
- Admission: GetFireError codes lack producers; Aircraft+6C9 (`4143FC` sets, `413D47` clears,
  `41A9FF` refuses; writers `65DCE9`/`65E7B8`/`65EA0B`); DropPayload arm blocked.
- Air FireAt velocity, ROT0/1/homing and 6CA suffix; GetROF House/bunker/authored delay; SpawnManager
  burst writes `6B73F6`/`6B7585` (`746493`/`74656C` unverified).
- ~17 raw `attack_target = None` sites skip `WeaponBurst::clear_target`; classify against
  PointerExpired/Detach. Legacy per-burst ammo debit still hits non-Attack Fly aircraft.

Fly flight (owners `FlyRuntime`, `world/fly_orders.rs`, `world/fly_landing.rs`):
- BeginLanding `4CFA70` has its world owner; its Fly callers `4CE43C`/`4CF520` (arrival arm with
  cruise+5C, type+D27, `4D0180` gates)/`4CCDDB` are unported: a player-moved aircraft never
  attempts BeginLanding (native refuses AirportBound ones into Enter_Idle_Mode, i.e. home).
  Observed in the dock trace: once the legacy horizontal adapter stalls inside 86 leptons
  (creep floor 0.05 alternating with the ramp to 0, zero step speed) it never clears
  `movement_target`, so `AircraftMission::Move` does not reach Idle through arrival there. Port `4CEFB0`/`4CE145` arrival with the landing arm. BeginTakeoff callers
  `4CCED4`/`4CE9DD`; its refusals `70EFD0` (+504 EMP) and `4DE770` (Foot timer +6A0/+6A8) lack
  producers. Mission_Enter HELLO and contact-slot pad offsets must absorb `AirfieldDocks`.
- Process horizontal/slowdown/drift/arrival; `4CE3C0` landing gate; `4CEFB0` navigation (read instructions).
- Map edge: `568300` shape, `41B890` predicate, FlyBy Type+E0B, `565660` ±128 via Map+12C
  (`566332`: width+height-1), one `49F420` scatter (one draw via `65C780`; reuse
  `inviso_scatter::random_direction_coord`), failed check skips SetCoords `4CDCFB`.
- Stop `4CCFD0`, null MoveTo, FootAssignDestination(NULL) +6AD and Attack skip-Stop
  (`4D9672..4D969C`); queued Enter, lift +2AC/+2B0 detach `70FEE0`, +304 cleanup, +6AC.
- Facing writers `4181BB..4185DF`/Fly steering; body-snap consolidation (`track_host`, `walk_head`,
  `animation`, `infantry`, `jumpjet_cruise`); Fly pitch/roll and slope FLH.
- `4196B0` Team/missing-slot shroud; Jumpjet crash `54CBD4..54CC0D`; DropIn `5F400E`/`5F4196`.

Locomotors, Scatter, Unit setter:
- Hover (`514C30`/`514D90`/`516320`) destination lifecycle; Rocket (`661F50`/`6632E0`) owner;
  Teleport byte+30 vs phase+34 (`7181DB`, `718254`, `719BD2`, `719B0D`).
- Unit source Scatter setter (`.local/unit-scatter-setter-probe.py`): queue/force/+6AC, radio, death
  counter Unit+6D8 (`746C90`; PR #440 had called +6D8 open), EMP, Foot+6A0; `743CA0` gates;
  admission needs numeric==0 (not repair's ==7 projection); unforced Target draws 1..4, only 1 admits.
- Infantry general/NULL Scatter: DoAction31, Garrison `457DE0`, `production_sell` direct move.

Speed, crates, Display, Anim:
- House `50C050` multipliers (+0x128/+0x12C/+0x130); Foot `4DB1A0` flag-carrier halving; Unit+6CC CTF.
- Crate pickup: guards, Tag49/Scenario+34BE, free MCV, water, removal-before-effect; reuse
  `combat_weapon::is_armed` for `701120`; trace Unit+2E8/Infantry+2F4 count producers.
- Display: projectile/particle/wave buckets, parachute canopy slot, Top absent-member fallback
  (the older `entity_draw_band` altitude note predates `abc880e9`).
- Anim AI: `runtime.inactive` conflates +19B; markers `424358`/`424427`; bounce; per-frame damage.

Team, Tag, Trigger (owners `TeamScriptVm`, `TriggerRuntime`):
- Team `6EC300`: +7F, Script `6915D0` cursor, action3, waypoint. Ctor `6E8B11` clears, AI `6E91BE`
  sets, `6EA0E2` clears. `6EA089`: latest checkpoint says clears; the earlier evidence-backed
  correction says SETS (BL=1 at `6EA084`); recheck. Production creation/activation absent.
- Replace definition-ID latches and `MapTrigger.repeating` (token8 sets +A0) with live instances;
  materialize Tag/CellTag/object attachments. Preserve `6E52A0` reuse, prepended instances,
  reset `726400` before gates, `689670`/`689910` resets, poll `55AFB0` order and compaction.

Whole-combat gaps (plan list plus review coverage top 10):
- Special warheads: all 11 bodies no-op (`projectile.rs:856`); Parasite suppresses damage (dogs).
- Destruction: building `4415F0` effects, vehicle/building survivors, VoxelAnim debris, death specials.
- Homing launch/steering non-native; host cos/sin/atan/hypot in Flak, cluster, shrapnel and homing tables.
- Gattling stage pinned 0 (`combat_weapon.rs:1070`); Prism forwarding absent; Tesla overpower.
- Legacy `tick_retaliation` beside `7087C0`; 2-D in_range twin (-512); retarget skips sequence reset.
- FLH slope/building arms; vehicle click (+6E0, `692640`/`692666`, `5F4578`); fire legality.
- Final owner audit; release retail load and rendered takeoff->landing->reload on the final
  candidate. Last load (landing increment): parity-digest `728c32753109a51dca9bc1c996a1219101db050ce3cf0887a9595a375f08471f`,
  two `Dustbowl.mmx` runs, seed `0x00C0FFEE`, 30 frames, identical digests (loading only).
  Fight.MAP `radar-online-v2` capture INVALID on its stale MCV facing contract.

## Validation

Codex HEAD `3df1f957`: `cargo test -p vera20k --lib` 9199 passed, 0 failed, 135 ignored;
Clippy pass, 1024 warnings (logs in the Codex worktree's `.local/`). The same 9199 reproduce
on `3df1f957` merged with main `ef3bf17f` in the landing worktree.
Landing PR candidate: `cargo test -p vera20k --lib` 9201 passed, 0 failed, 135 ignored
(two new regressions: dock cycle, Display expiry); `cargo clippy -p vera20k --lib` pass,
1022 warnings. No replay pin moved. No retail/rendered launch or Linux/macOS execution.
