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

Checkpoint (2026-09-23): the landing PR merged as #444 (`9bab8fba`), Parasite (attack dogs,
Terror Drones) as #446 (`24ff63b7`), the Techno death broadcast and Stun as #447 (`ab80dd3f`),
the SpawnManager slot guard as #448 (`35149218`), the Scenario-stream death draws as #449
(`280a9243`), crew survival (building SpawnSurvivors, vehicle crew) as #450 (`bf55a511`), the
death anims (building DestructionEffects, Unit Death_Explosion with the ship-sinking gate, the
Aircraft arm) as #451 (`1aa3041b`), mind control (CaptureManagerClass) as #457 (`32fc312b`).
The Chrono Legionnaire's erase (TemporalClass) is on `feature/combat-temporal`. A production
sortie showed the Carrier wing never attacks: every manager pass re-issues each Hornet's attack
from sub-state 0 (`assign_child_attack`), so the aircraft attack run never completes and the wing
never lands. Next, by player visibility: the house defeat blow-up (below), the remaining special
warhead bodies (IvanBomb, Magnetron and the rest), the ship sink, that wing cycle together with the
aircraft attack loop (Mission_Attack 5..9, GetFireError), the Foot Enter_Idle_Mode leaves (Infantry
`51CBA0`, Unit `738970`, with the Mission_AreaGuard post and leash `4D6AA0` they need), open-topped
passenger fire (a Chrono Legionnaire or Yuri in a Battle Fortress), and constructing each death
object at its native call.

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

Parasite (`feature/combat-parasite`, snapshot/hash 193), owner `sim/combat/parasite.rs`:
- Allocation `6F40FC..6F414E` (weapon-0 Parasite warhead, non-buildings); LimboLaunch in Fire
  `6FF749..6FF872` (reselect memo, Limbo, Foot+698 lock `6FF81F`); detonation arm `4693D3` ->
  AttachTo `62A980` (CanInfect `62A8E0`, Force_Track, Foot+694 link; refusal returns the owner
  to its Foot+55C cell). Bites: ParasiteClass AI `629FD0` at the victim's FootClass::AI tail
  `4DAEE1` (ROF cadence, Paralyzes, Infantry take Health ignoring defenses, others sparks, anim,
  one Scenario draw, weapon Damage). Release: PointerExpired `62A260` via the victim's forward
  `4D99AA` (GetReleaseCoords `62AC30`, CanPlaceAtVictim `62AB40`; suppressed owner dies with the
  host) and ExitUnit `62A4A0` (placement 90 degrees off the host, 3xROF paralysis; suppressed
  non-Naval owner dies).
- Forced releases: FootClass::ReceiveDamage prefix `4D7330` (Sonic eject + source target clear,
  `2*dmg-threshold` suppression, heal eject), repair Radio 0x1C `6F4D70`, FootClass::IronCurtain
  `4DEAE0` (also C4-kills Organic Units/Aircraft), Teleport warp `7195BF`.
- Restrictions: GetFireError CanInfect (5), launch lock and Iron Curtain (refuse this frame),
  transport load `737602`, bunker `70FBB9` (command and radio), DeploysInto `700EB4` (clears
  Unit+68C), retaliation `708ABD`. IsParalyzed `4DE770` feeds GetFireError `6FC623`/`6FCCD5` and
  both cloak gates.
- Tests `combat::parasite::tests` (15, flat-map production frames): dog kill/release, deck
  release, drone cadence (50 every 60 frames) and release, suppression death, heal, Sonic eject
  (facing 64, paralysis 180, archive cleared), Iron Curtain (drone killed, dolphin C4, orders on a
  curtained host dropped), teleport eject, lost-host return (archive kept), one parasite per
  host, load/bunker/deploy refusals, Load timer restart (`6295DB..6295F3`), reselect memo out of
  the peer hash. Rust regression only; no native executable comparison.
- Critic (one pass, `127a0a50`): fixed peer-hash `limbo_reselect` (process-local, saved not
  hashed), deck release Z, launch-lock/IC gates now drop the target like CanInfect (residual:
  native waits for the 16-frame check `6FA472..6FA4CB`), Load restarts both timers, refusal
  keeps ArchiveTarget (setter on `BaseDefenseResponseState`), squid attach guard, one CanInfect
  (`ParasiteVictimFacts::of/admits`), stale docs. Recorded: Area Guard arm of Enter_Idle_Mode,
  Unlimbo Can_Enter_Cell, the death-Stun timing (ported next). Declined: skipping limboed
  attackers in the combat pass (unit fixtures fire from never-revealed entities; follow-up).

Techno death broadcast and Stun (`feature/combat-death-stun`, no schema change), owner
`world/lifecycle.rs`:
- ObjectClass::ReceiveDamage's exact-zero arm runs Destroy = Detach_All(1) (`5F57AF`) after the
  kill callback. `object_destroy_callback`: class prelude (Building `44EBF0` OVER_OUT to every
  contact; Foot `4D9720` OVER_OUT to contact 0), Deselect, the SpawnManager owner arm (`6B7CBC`,
  reached because the announce loop `725947` does not skip the announcer and ObjectClass's
  constructor `5F3900` enrols every object), then the expiry broadcast. The receiver calls it at
  the killing hit; the CausesDelayKill PostMortem stage calls it after its bookkeeping.
- Assign_Target's object-liveness refusal (`6FCDB0`, `6FCEF8..6FCF03`: NULL for a !IsAlive or
  Health-0 object, after the same-target early-out) is now in the Target-write owner
  (`assign_target_commits`, `concrete_effects.rs`), for every writer that can name an object:
  mission transactions (Restore), the three Overrides, ToProtect and the cloak re-assign. Without
  it the Destroy walk's own Restore re-installed the corpse for a listener whose current and
  archived targets were both the victim (AI retaliation). Infantry Assign_Target's idle switch is
  gated on the receiver's Health (`51B203`).
- Death-arm Stun (`702210`): `techno_death_stun` = FootClass::Stun `4D5660` (navigation stop) +
  TechnoClass::Stun `6FCD40` (Assign_Target and Assign_Destination NULL, OVER_OUT to every
  contact, Kill_All_Spawns + ClearAllTargets, Deselect). Its Detach_All(1) repeats a broadcast
  the killing hit already made; every Target write since refused the Health-0 object, so the
  roster is walked once.
- Effect: listeners drop a dying object at the killing hit instead of at corpse removal. Visible
  on infantry die sequences (~15 frames): dogs and drones leave inside the killing bite, bullets
  retarget the corpse's cell, and attackers hold no target until their passive scan (re-armed to
  4..8 frames when more than 10 were left) picks the next one, where VERA used to switch at once
  through its dead-target compensation. Units and buildings reorder within the tick: the
  passive-scan re-arm draw now precedes a DeathWeapon's draws.
- Deleted: the death-path refinery adapter (`undock_refinery_unit_on_death`; the Destroy drops
  the reservation and the miner's own dock visit aborts to Approach), the animated-corpse BREAK
  in `finish_concrete_death`, the invented inside-transport gate of the garrison exit-cell check
  (SellBuilding `457DE0` asks Occupants[0]; `7078C0..7078D5` has already cleared its Transporter),
  and the parasite "release at the killing hit" residual. Corrected: the spawn-manager census
  (`710021` is the Magnetron lift, not PerformDeploy).
- Tests: late-timing pins flipped to the native order (dog released on the kill frame,
  attack-move target expired at the kill, nested DeathWeapon RNG order, refinery death); new
  production-frame checks for the refused Restore, the Stun's own effects and a spawner's
  children dying inside its Destroy. Rust regression only; no native executable comparison.
- Residuals: Foot Team removal `4D9744` (`TeamScriptVm` never removes a dying member); the
  Building prelude's production abandon `44EC01..44EEC8` (VERA drops it in the same tick's
  production phase); the owner arm runs ahead of the listener walk instead of at the owner's
  roster slot (rare Scenario-order swap); the Limbo self-visit owner arm `5F4D61`
  (`object_conceal_with_context`); the building NowDead contact loop `442511..442601`, which walks
  the pre-hit contact copy (`4422C1..4422DB`): a C4Warhead kill for every contact of a
  Helipad=yes building (aircraft landed on an airfield) or within 0x100 leptons of the centre,
  else radio 0x17 and contact+0x500 = 0. It belongs to building destruction effects and must
  snapshot contacts before the receiver. Infantry second Stun `518108`, naval sinking `737E58`,
  FootClass::Crash `4DEC90` and the Sell, exit-map, Deploy and teleport Stuns stay with their
  mechanisms.
- Critic (one pass): fixed the Restore re-install (above), stale facing-test contract, missing
  production coverage and provenance; recorded the owner-arm order. Follow-ups: the carrier slot
  defect (next mechanism: a docking Hornet's Limbo broadcast frees its own slot because VERA lacks
  the alive-child guard `6B7CDD..6B7CF4`, so the Hornet stays in limbo); a third roster walk per
  death at the 20k scale (cache the order or index reverse references); ToProtect on the killing
  hit (`702D24` table).

SpawnManager slot guard (`feature/combat-carrier-dock`), owner `sim/spawn_manager.rs`:
- `SpawnManagerClass::PointerExpired` `6B7C60` keeps a slot while its child has Health, is not on
  the retreat tracker (+6CA: cleared by the aircraft constructor, set only by
  `SpawnRetreat__Push` `54E47D`, whose callers all free the slot straight after) and the slot is
  not a missile slot (`6B7CDD..6B7CF2`). VERA freed the slot on any expiry, and `step_landing`'s
  Limbo broadcasts (`5F4D61`), so a docking Hornet would free its own slot, stay in limbo for good
  and leave the slot to a full `SpawnRegenRate` rebuild (600 frames) instead of the 150-frame
  reload.
- Latent in play: VERA does not land a recalled Hornet yet (the Fly arrival BeginLanding call
  `4CF520` is unported; the recall Move ends in Idle, then Guard), so no production frame reaches
  the dock today. The test stages the landing step by hand: the Hornet keeps its slot, reloads on
  `SpawnReloadRate` and is ready again (fails without the guard). Rust regression only.
- Critic (one pass): guard matches native; wording fixed (latency, +6CA writers). Follow-ups for
  the Carrier wing mechanism: native state 3 re-issues Assign_Target and QueueMission(Attack) each
  pass as no-ops when unchanged (`6B7718`, `6B772C`), where VERA restarts the attack run from
  sub-state 0; Kill_All_Spawns crashes airborne MissileSpawn=no children (`SpawnRetreat__Push` ->
  vt+3DC `AircraftClass::Crash` `4DEBB0`), where VERA lets them fly on; the Fly arrival landing.

Crew survival (`feature/combat-survivors`, snapshot 194), owner `sim/crew_survival.rs`:
- `BuildingClass::SpawnSurvivors` `442D90` from DestructionEffects `441F1B`, while the building is
  still on the map: Phase A releases InfantryAbsorb/UnitAbsorb passengers (the Bio Reactor), each
  advancing the foundation cursor; Phase B rolls `RandomRanged(0, chanceMax)` per remaining
  foundation cell (1 or 2, +6 once captured) while survivors are owed, then commits that cell's
  scorch/crater mark. No survivor owed (NoSurvivor, uncrewed, a house outside the three sides)
  means no per-cell marks at all. NoSurvivor is the killing call's IgnoreDefenses (`441F0B`), so
  a C4 expiry (`440345` pushes 1) spawns nobody; survivors of a building still carrying an enemy
  C4 charge Attack the planter.
- Count `451330` = GetRefund `711F60` (x87 chop, Soylent, RefundPercent for a human owner,
  FactoryPlant factor) / side divisor (doubled once captured), clamped 1..5. Crew type `44EB10`
  (25% Engineer roll on an uncaptured building, winnable only by a ConYard) over GetCrew `707D20`
  (side crew, 15% Technician when armed). HasBeenCaptured (Building+6E3) is set by every
  BuildingClass::ChangeOwner (`448723`), saved and hashed.
- Vehicle crew `7381BC..73838A`: after the dying unit's Mark(UP) (`737F7A`), a Crewed=yes type
  with no passenger capacity rolls `RandomRanged(0, 0x7FFFFFFE)` against CrewEscape in x87 order
  (`r < 0x40000000` at 50%); arg6 skips it. Health `RandomRanged(5, Strength/2)`, Guard (human) or
  Hunt. InfantryClass::Unlimbo's Z gate (`51E01B`) is modelled: a vehicle on a bridge deck (or
  lifted) leaves its crewman at its exact coordinate with no placement draw. Aircraft have no
  crew path (Pilot= has no gameplay reader).
- Placement is PlaceInfantryInCell `481180` over the raw occupation bytes (the requested plane's
  vehicle bit refuses; the ground byte's 0x40 refuses either plane unless a passable Gate,
  `+16B7`/`4525F0`; the building bit is not read; the centre/NW-row draw is spent before the scan). Its transport, paradrop and parasite callers
  moved to it; exit is the shared forced Scatter arm (hut and crew).
- Deleted: the invented one-E1 destruction survivor (`eject_destruction_survivors`,
  `DestroyedCrewedBuilding`) that ran after the building's UnInit, and the per-cell marks every
  building death drew. The fatal passenger purge now skips absorbing buildings.
- Tests: exact Scenario draw ledgers replayed on a cloned stream (Phase A/B interleave, captured
  yard, C4 odds and Attack, vehicle crew, arg6/capacity skips), receiver-level kills, a retail
  `rulesmd.ini` binding check, and an ignored retail Dustbowl run (8 seeds: 6 plant and 4 MCV
  crewmen scatter through FNPC off their spawn cells). Rust regression only; no native executable
  comparison.
- Residuals (module doc): the second SpawnSurvivors after Limbo for `Explodes=` (TechnoType
  `+D15`) or Selling deaths (`4400D4`; every NANRCT death; needs a breakpoint); the sale crew
  (`Mission_Selling` `44A2EE`); passenger escape from a dying transport (`737FB0..7381B6`; VERA
  still kills the cargo); FNPC failure's eight-neighbour Scatter fallback and a blocked start cell
  losing its destination (the crewman stays put); IsToDie; the survivor's Doing at Scatter; the
  Unlimbo usable-area arm (map rim); HijackerType; selection/tag transfer; Phase A kill
  credit/counters; Nominal; a Bio Reactor holding more infantry than cells. The walk
  FindSubCellDest and tube-exit callers keep the cell-list allocator (movement ledger); the
  landed-aircraft unload moves with its Unlimbo Z gate (aircraft unload port).
- Critic (one pass): fixed the bridge-deck crew (Unlimbo Z gate), the deck request's ground 0x40
  test, the NW-draw doc, the unrecorded residuals (second SpawnSurvivors, passenger escape, FNPC
  failure) and the `+D15` misreading (it is `Explodes=`, not walls). Noted: parasite releases next
  to a building now succeed (0x80 is not read), matching `481180`.

Death anims (`feature/combat-destruction-anims`, snapshot 195), owner
`combat/destruction_effects.rs`:
- DestructionEffects `4415F0` steps 1, 7, 8 and 13 at the building's death, before SpawnSurvivors:
  the damage fires go out; the centre mark; per foundation cell (list order) a `49F420` jitter
  draw (radius 0x40, one Scenario Next), `RandomRanged(0, 3)` delay and `Next % count` pick
  (`4419CE..441A1F`); then the DestroyAnim pick (drawn even for an empty entry) at vt+AC
  (`459EF0`, Location - 0x80). Every building death used to skip these 3 draws per foundation
  cell plus the DestroyAnim pick, desynchronising the Scenario stream from there on; the building
  now shows its multi-explosion death.
- Unit `Death_Explosion` `738680`: the `Explodes=`/EXPLODES-ability override takes the last
  `Explosion=` entry when the unit has ammo (`7386C3..7386FF`; `+684` is `Ammo=`, `+2FC` the
  current ammo): the Apocalypse and the Demolition Truck always die with `S_TUMU60`. The Aircraft
  arm (`41663C`) picks `Explosion=` only; VERA had also drawn `DestroyAnim=` (no stock aircraft
  has one).
- The Unit NowDead block gates `Death_Explosion` (`737DA7..737F6F`): a surface ship
  (`Naval=`, not `Underwater=`/`Organic=`) at least `ShipSinkingWeight=` (Rules `+630`, default
  3.0) heavy, on a water cell and not being warped, sinks instead (`737DE2..737E5E`): no pick, no
  anim. Every DEST, AEGIS, CARRIER, DRED (and campaign VLAD, CRUISE, CDEST) death on water used to
  draw an extra Scenario Next. The sink itself is a residual: VERA removes the hull at once.
- All death producers construct `AnimClass(type, coord, delay, 1, 0x600, 0, 0)`; VERA had used the
  warhead impact's `(0, 1, 0x2600, -15)` at a level-rounded coordinate. The draws stay in the
  receiver; the constructions stay on the transaction's ordered anim list
  (`ExplosionEffect::death`), constructed at the consequence boundary in push order (the
  damage-consequence ID-order pins hold). Snapshot 194 -> 195: stored death anims change their
  arguments and building deaths add AnimStore members.
- Tests: exact draw ledgers for the building, unit override, aircraft and empty/single lists; the
  former `gsi_08_11` fixture test plus production-receiver kills (the anims' draws precede
  SpawnSurvivors' on the same cells; the ship-sinking gate on and off water, below the weight and
  underwater); an ignored retail Dustbowl run pinned to its seed (GAPOWR: `S_CLSN58`, `S_TUMU60`
  and two art-less `gtpowexp` picks; MCV: its own `S_CLSN58`). Rust regression only.
- Residuals (module doc): construction order (the transaction's anims and voxel debris construct
  at the consequence boundary while crew, survivors and damaged-art anims construct inline, so
  natively the first crewman follows its death's anims in the live order and here precedes them;
  the frame the last-constructed anim expires skips a different object; routine for crewed
  deaths); the ship sink, the DeathFrames deferral and the water splash; the other
  `Death_Explosion` callers (a crushed vehicle `7418E5` -> `746D60`, a `Crashable=` crash
  `7461D1`, the DeathFrames completion); AnimClass::Middle `424F00` (the scorch/crater a
  multi-frame explosion leaves at its middle frame); art-less `gtpowexp`/`tstlexp`; DestroyAnim
  palette; `RevealToAll=` (step 5, stock, shroud only); steps 2-4, 9-12, 14 (dead or absent on
  stock); a vehicle's spent ammo. Corrected: the FIRE3 trigger text (a neighbour cell's
  `Explodes=yes` overlay; none in stock), the stale warhead-debris residual, and the crush doc
  (`bump_crush`: a crushed vehicle does run `Death_Explosion`). The fatal prelude's order against
  the Techno death arm is recorded at `BeforeDeathEffects`.
- Critic (one pass): gated the ship sink; recorded the other callers and corrected the crush doc;
  rewrote the construction-order residual (the live-order skip, not only identities); added the
  integrated order test and the retail pin; bumped the snapshot; fixed the `ExplosionEffect`,
  effect-catalog, FIRE3, debris and address docs. Deferred with a residual: constructing each
  death object at its native call.

Mind control, PR #457 (merge `32fc312b`; snapshot 196, hash feature 196), owner
`capture_manager.rs`:
- Capture: the MindControl arm of `BulletClass::DetonateAtCoord` (`46920B`) claims the impact
  (no area damage) and calls CaptureUnit `471D40` on the bullet's own target. Stock
  `[PsychicControl]` is Inviso, so VERA's immediate delivery now runs that arm too; before, every
  Yuri, Yuri Prime, Psychic Commando, Mastermind and Psychic Tower shot dealt 1-3 ordinary damage.
  CaptureUnit: CanCapture `471C90` (house, ImmuneToPsionics, tank bunker, already controlled,
  Iron Curtain, capacity with the single-link replacement, Selling/Construction), ChangeOwner,
  the node with the original house, `+2C0`, the `70F850` reset to Guard, DecideUnitFate `4723B0`
  (one Scenario `RandomRanged(1, 100)` for a computer-owned result; the controller's house picks
  the AICapture table), the MINDANIM ring attached at the victim's GetCoords raised by
  `MindControlRingOffset=`, or for a building by `Height=` levels of 104 leptons.
- Release: FreeUnit `471FF0` / FreeAll `472140` from the controller's death arm (`702112`, before
  its death sounds), Foot UnInit (`4DE5DD`), a drain link on it (`70FDBD`), a Psychic Tower's
  operational off edge (FreeAll `454B47`, which a sale reaches through Selling; VERA's synchronous
  sale frees at its start), the single-link replacement and a captive boarding a building (the
  PerCellProcess sites): ring UnInit, MindClearedSound, ChangeOwner back, DecideUnitFate. A
  captive's death only drops its node. The victim's `+2C0` also bars deploying an MCV (`700ED0`)
  and repacking a Construction Yard (`449C15`, `44F614`).
- Gates: GetFireError's CanCapture (`6FCB24`) in the weapon ladder (frame-free gates) and at fire
  admission (the live check, with the Iron Curtain), IsFull in ShouldRetaliate `70882F` and
  CanAcquireTarget `709230`, the transport and garrison radio refusals (`7375F3`, `43C4A0`,
  `43C5CB`, CanDock `457D98` on the occupier).
- The Mastermind overload (`471A50`, from AI_Update `6FA730` before the IsAlive gate): above three
  captives, C4 self-damage per `Overload*=` row, the voice once per episode, five spark systems
  (ten Scenario draws), the lean's sign draw.
- TechnoClass::ChangeOwner's mission half (`7014A0`) now covers every class (Guard queue,
  archive clear, Rescue, Enter_Idle_Mode with the Foot selector and the Building Guard), which a
  release needs so the returned unit stops fighting its own side; it also changes engineer
  capture and garrison transfer. It drops VERA's legacy `order_intent` with TarCom and NavCom, so
  no attack-move goal or guard anchor survives an owner change.
- Corrected consumers: CanAcquireTarget (was the victim flag), ShouldRetaliate (dropped the victim
  flag), CanDock (tests the occupier, not the building), CanAutoCloak (the term is the Magnetron's
  `+6AD`), the detach-sweep note (`+294` is the Airstrike link), the miner note (miners are
  immune).
- Native execution (Unicorn, `tools/spatial_oracle/`), pinned in `capture_manager::tests::native_*`:
  `capture_decide_fate.py` runs the original DecideUnitFate over 156 rows (each reason and its
  boundaries, including negative power totals and a 0/0 health ratio, which native reads as
  Wounded because FCOMP sets C0 when unordered; VERA read it as Normal and was fixed; the draw;
  the walk through choices 1-6, an exhausted table and a negative weight); `capture_overload_update.py`
  runs the original constructor and Update frame by frame over 22 cases (first check on the 31st
  frame, each row and its boundaries, the damage arguments, the voice latch and its re-arm, spark
  offsets `{X+r2, Y+r1, Z+100}` with a zero target, the lean draw's gates, a finite manager, zero
  frames, a killing overload); `capture_ring_height.py` runs the static initializer that sets a
  building ring's level height to 104. Parity demonstrated for those functions within those
  inputs; the fixtures stub ReceiveDamage, PlayAt, operator new, the ParticleSystem constructor,
  the grinder/absorber seeks and Queue_Mission.
- Tests `capture_manager::tests` (25): the three native corpora, weapon-0 managers (rookie
  weapon), capture with its draw, the building ring, single-link release, CanCapture gates (an
  ally too), the ladder gate, death release newest-first with its draw, silent victim death, Foot
  UnInit, tower off edge, a tower sale with a save/load after it, the MCV and Construction Yard
  refusals, the build-up gate, the dropped order, a captive boarding an absorber, the Unit
  transport refusals, the Iron Curtain at fire time, a killing overload's draw order, the fate
  walk, the overload cadence, snapshot and hash, a production Yuri attack order, retail binding.
  Everything outside the native corpora is Rust regression only.
- Critic (one pass): two blockers fixed (a captured MCV deployed and a captured Construction Yard
  repacked; a sold Psychic Tower kept its captives and broke later saves), plus the build-up and
  repack gate, the legacy order, the rookie manager, the local-player capture sound, stale docs
  and the aircraft-transport refusal. Follow-ups outside the mechanism: AI_Update's allied-target
  drop (`6FA30C..6FA46C`); open-topped passengers fire from the transport, which has no manager,
  so a Yuri inside a Battle Fortress cannot capture; the unused mind-control cursors; the house
  defeat blow-up below.
- Residuals (module doc): the Psychic Dominator and `+2C4`; DecideUnitFate's Team, Grinder and
  Bio Reactor arms (hunt instead); link lines and the overload flash (presentation); the overload
  lean (no rocking producer); the crushed controller's release timing;
  object tag events 6/0x2C; the Iron Curtain gate absent from the frame-free weapon ladder; Inviso
  special warheads other than MindControl (the squid grapple) still take area damage on the
  immediate path.

Temporal (`feature/combat-temporal`, snapshot 197, hash feature 197), owner `temporal.rs`:
- Before: all three stock Chrono weapons (`[NeutronRifle]` 8, `[NeutronRifleE]` 16, the IFV's
  `[CRNeutronRifle]` 5) are Inviso, and the immediate delivery ran ordinary area damage for them,
  so a Chrono Legionnaire plinked for 8 instead of erasing. Now the Temporal arm of
  `DetonateAtCoord` (`469423`) runs on both deliveries.
- State: the firer's TemporalClass (`+274`: target, chain Prev/Next, WarpRemaining), created at
  construction when rookie weapon 0 is `Temporal=` (`6F4154`); the target's chain head (`+278`),
  whose presence is the temporal half of `+270`. Only `temporal.rs` writes either.
- InitiateWarp `71AF20`: kills the target's spawns, frees its captives, drops the firer's previous
  victim, CanWarpTarget `71AE50` (`Warpable=`, Iron Curtain / Force Shield, a unit still in its war
  factory), refuses a warped firer, heads the chain with `Strength*10` or inserts after the head, the
  harvester / building under-attack notice, the victim's own release, Deselect. A Unit in a Tank
  Bunker hands the firer its bunker.
- The target owns the tick: each class's AI runs its head's Update `71A760` first (Unit `736204`,
  Infantry `51BB6E` after its sparkle, Aircraft `414BDB`, Building `43FCF9` after damage fire),
  then sparkles every 24th frame (a building per MuzzleFlash port) and freezes: TarCom and a Foot's
  NavCom and path drop, and the leaf returns before FootClass::AI (`73647B`, `51BC9F`, `414DA3`) or
  TechnoClass::AI_Update (`43FE56`). VERA's out-of-shell phases consult `GameEntity::ai_frozen`:
  idle actions (Scenario draws), fear, deploy, repair and the AI low-credit sale (`450630` is
  called only past the frozen jump), depot service, spawn managers, boarding and unloading,
  construction, unit facing and turrets, the sprite stage (`6FAC4D`), the legacy order intents,
  engineer, bridge-repair and C4 orders, gates, aircraft docks, bunker installs; the locomotor does
  not run. Update: the corrupt-head release,
  the open-topped distance release (`Sqrt_Approx` + ftol vs `OpenToppedWarpDistance*256`),
  `WarpRemaining -= own Damage + SumChainDamage` (depth 0x32), and at `<= 0` the erase: WarpAway,
  VeterancyStruct::Add for a Trainable firer, the bunker release, Death_Announcement,
  Record_The_Kill (kill, loss and score for a full-health victim through
  `combat::record_kill_credit`), UnInit (no death, no survivors), idle.
- Releases (LetGo `71ABC0`, progress handed to the next attacker, nothing healed): retarget, a
  counter-warp, cell entry (`6F5090`, including a teleport relocation), idle entry (`709A54` at
  every represented Enter_Idle_Mode: the Attack and deployed-reacquire idle exits, Move arrival,
  `queue_foot_enter_idle_mode`, the Drive idle; Stop on an Attack-mission Foot, whose native release
  is the Attack idle exit on its next dispatch), DecideUnitFate (`4723F3`), the attacker's own
  expiry and its target's (`71AB60`, outside the removal gate), the IFV gunner seat
  (ReceiveGunner `746420` / RemoveGunner `7464E0` move the TemporalClass).
- One `+270` owner: `GameEntity::is_warped_out` (temporal head or teleport warp-out) and
  `is_warping_in` replace eleven direct teleport reads (damage immunity, selection, pick, radar
  SpySat scan, locomotor warp gate, cloak, animation, draw state, infantry and unit cell entry).
- Gates: GetFireError's firer-warped (`6FC109`), linked-target REARM (`6FC14F`), warped target
  with a non-Temporal warhead (`6FC5D5`, the target drops), the IFV MOVING arm (`741206`);
  CanAcquireTarget's first test (`7091D6`); Evaluate_Candidate's GetFireError probe (`6F7CE8`, its
  `6FC5D5` arm keeps a warped candidate out of every passive scan and retarget); CanDock (`457D3E`);
  CanEnter on a warped Unit transport (`7375BA`) or absorber (`43C422`); an engineer turned away from
  a warped building (`519EF2`). The warped building's online latch (`+660`, `4521C0`/`452210`):
  Is_Operational, power output and drain (`44E7C7`/`44E885`), radar, the refinery and absorber
  CanEnter (`43C422`), the depot probe (`43C7FB`).
- Native execution: `temporal_update.py` runs the original Update with SumChainDamage, LetGo,
  ClearLinkedList, Sqrt_Approx and ftol over 29 cases (steps, elite, untrainable, chains, the
  depth cap, the corrupt head, a building, a head that lost its Target, the open-topped boundary:
  1793 holds because `Sqrt_Approx(1793^2)` truncates to 1792, 1794 releases, diagonal, cube and
  vertical cases, a chained hand-off). `temporal_initiate_warp.py` runs the original InitiateWarp
  with CanWarpTarget, LetGo and Contact_With_Whom over 30 cases (`Strength*10` including its wrap,
  0x0CCCCCCD -> -2147483646; the insert after the head; the refusals, radio slot 0 only and the war
  factory in the unit's own cell; the warped attacker; the previous victim released even when the
  new warp is refused or has no Techno; the victim's own release; the notices; the building going
  offline; Deselect only with a player). Pinned in `temporal::tests::native_update_corpus` (links,
  WarpRemaining, `+270`, the erase's callees: WarpAway coordinate and flags, `Add(1500, 900)`, the
  Enter_Idle_Mode calls in order), `native_initiate_warp_corpus` and
  `native_open_topped_boundary_is_distance_3d`. Parity demonstrated within those inputs.
  Reading only: the per-class prologues and the sparkle cadence, the pointer-expiry forward (the
  corpus test adds its idle per chained link), the release sites, the gunner hand-over.
- Tests `temporal::tests` (20): the three native corpora, Init_Managers, retarget and counter-warp
  release, LetGo's three arms, pointer expiry, the IFV hand-over, a warped building offline and
  back, the frozen object (sparkle cadence and position, TarCom, damage immunity and
  ignoreDefenses), the out-of-shell phases (no idle draw, no repair or bill), a warped enemy passed
  over by a GI's scan but not a legionnaire's, non-Temporal fire at a warped target, Stop freeing
  the victim, a mover warped and released not resuming its order, a warped building's capture
  refused, warped transports admitting no one, a production erase (a Rhino gone exactly 500 of
  its AI turns after the shot, no damage, WarpAway, kill and loss, `Add(1500, 900)` twice, no
  survivors), release on a move order, snapshot and hash round trip. Everything outside the native
  corpora is Rust regression only.
- Residuals (module doc): the teleport writer's sparkle, frozen AI and phase gates; `+27C`;
  gattling spin-down; Mark and building anim pause (presentation); house `+1FC`; the online-latch
  readers not wired (FindFactory `5F7900`'s online argument, so production is not suspended; the
  upgrade-prerequisite scan; AI_ManageProduction; CheckDockArrayOccupancy; PowerCheck_Upgrade;
  vt+0x4E0 `4456D0`); the building erase's occupant kill order; slave release; ReceiveGunner's
  ROF-timer hand-over; the layer test (IsHighFlying stands in); VERA's fire phase stepping a new
  warp one AI turn later than native can; Stop releasing with the event; the engineer's scatter;
  WANT_RIDE (dormant); `71AD40` (house defeat); voxel and harvest-overlay frames; the cursor
  readers.

House defeat (the next mechanism): `0x004FC6D0` (`HouseClass__Blowup_All`, was ScatterAllUnits)
kills every Techno whose original owner is the house, limbo objects included, with full-health C4
damage and no killer, from HouseClass::Update's defeat gate (`4F8E86..4F8F82`: short game when the
building count skipping Insignificant/DontScore and the MCV count are zero; otherwise every
counted object), then `MPlayer_Defeated`; the `+1F6`/`+298` path (`4FC980`, Flag_To_Die) is
reached only from DESTRUCT, REMOVEPLAYER or an EXIT with no other human. Mind-controlled units of
other houses survive with their release house rewritten to the civilian side. VERA's `check_defeat`
keeps the objects alive, counts Insignificant/DontScore types and YAREFN as buildings, and walks
houses in map-key order; two world tests lock that in. Evidence so far is static reads of the
landing-era binary; the mechanism's own trace and oracles come first.

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
| capture_decide_fate / capture_overload_update / capture_ring_height | `4723B0`; ctor + `471A50`; `471610` | 156 / 22 / 1 | ReceiveDamage, PlayAt, spawn systems stubbed |
| temporal_update | `71A760` with `71AB10`, `71ABC0`, `71ADE0`, Sqrt_Approx, ftol | 29 | UnInit, anim ctor, veterancy Add, idle, occupant kill stubbed |
| temporal_initiate_warp | `71AF20` with `71AE50`, `71ABC0`, `65AD30` | 30 | virtuals, spawn kill, FreeAll, cell lookups, notices, gattling, offline/online stubbed |

Also `tools/infantry_scatter_oracle`, `tools/mcv_deploy_oracle`. Pre-branch main harnesses (review): techno_target_scan 171,
vhp_scan 498, distributed_fire 151, foot_attack_move 638, estimated_damage 1066, object_health 718, cell_entry_crush_tail 68.

## Ghidra annotations

All saved and read back; no byte or prototype edits. One boundary repair (below, Parasite).

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
- Parasite (plates): `62A980` ParasiteClass__AttachTo, `62A8E0` __CanInfect, `62A4A0` __ExitUnit
  (was WarpAttachClass__Detach), `62A260` __PointerExpired (function created), `629FD0` __AI (was
  WarpAttachClass__UpdateAttack), `6297F0` __UpdateSquidGrapple (was TemporalClass__AI), `62AC30`
  __GetReleaseCoords, `62AB40` __CanPlaceAtVictim, `4DE770` FootClass__IsParalyzed (was
  TechnoClass__Fire), `4DEAE0` FootClass__IronCurtain (boundary repaired: was FUN_004deae0 +
  TechnoClass__StartFidget at `4DEAE4`). Comments `4D734F`, `4D7374`, `4D73D4`, `4D998C`,
  `4D99AA`, `4D99C9`, `6F4D70`, `6FC623`, `6FCAAD`, `6FCCD5`, `708ABD`, `70FBB9`, `737602`, `7195BF`.
- Death Stun (plates): `6FCD40` TechnoClass__Stun (was FUN_006fcd40), `4D5660` FootClass__Stun
  (was FootClass__StopFiring), `710000` TechnoClass__ImbueLocomotor (was TechnoClass__PerformDeploy),
  `5F5280` ObjectClass__Detach_All, `4D9720` FootClass__Detach_All, `44EBF0`
  BuildingClass__Detach_All (the three were `__Destroy`). Comments `5F57AF`, `702210`, `6B7CBC`,
  `5F4D61`.
- Crew survival: `451330` BuildingClass__How_Many_Survivors, `44EB10` BuildingClass__Crew_Type,
  `707D20` TechnoClass__GetCrew, `711F60` TechnoTypeClass__GetRefund (were FUN_), plates on those
  and `442D90` SpawnSurvivors; `481180` PlaceInfantryInCell plate corrected (the 0x40 exception is
  a passable Gate, not a garrison building). Comments `441F0B`, `7381BC`.
- Death anims: plates `4415F0` BuildingClass__DestructionEffects (all 17 steps) and `738680`
  UnitClass__Death_Explosion (override, dead ore sum); comment `41663C` (Aircraft arm). `746D60`
  UnitClass__Crushed_vt170 (was Receive_Message_Hook; Unit vt+170: Death_Explosion then
  FreeAllMindControlCaptures) with a plate; comments on the NowDead gates `737DA7`, `737DE2`,
  `737E78`, the crush call `7418E5` and the crash call `7461D1`.
- Temporal (plates): `71AF20` InitiateWarp, `71AE50` CanWarpTarget, `71A760` Update, `71AB10`
  SumChainDamage, `71ABC0` TemporalClass__LetGo (was DetachFromTarget), `71ADE0` ClearLinkedList,
  `71AB60` __PointerExpiredForward, `71AD40` __ReleaseChainNoIdle (were FUN_), `4521C0`
  BuildingClass__TemporalGoOffline and `452210` __TemporalGoOnline (were StartCloaking /
  StopCloaking), `44D760` BuildingClass__Death_Announcement, `660B80` RecentEventCellRing__Push,
  `6F5090` TechnoClass__PerCellTail, `709A40` TechnoClass__Enter_Idle_Mode (were FUN_), and the
  created `746420` UnitClass__ReceiveGunner, `7464E0` __RemoveGunner. After review: `5F7900`
  TechnoTypeClass__FindFactory (was FUN_, plate), plate `456750` (sensor-range circle); comments on
  the frozen branches `7362FB`, `51BBD1`, `43FD14`, AI_Update's stage step `6FAC4D`, Update's
  no-target erase `71A895`, ClearLinkedList `71AE02`/`71AE12`, Evaluate_Candidate's probe `6F7CE8`,
  GetFireError `6FC57D`/`6FC5D5`, UnitClass::Receive_Radio's switch `73743F` (index = message - 3:
  0x0F CanEnter, 0x24 WANT_RIDE) and warp answers `737460`/`7375BA`, PerCellProcess's capture
  refusals `519EB2`/`519EF2`, the IDLE event arm `4C74CB`, Building Receive_Radio `43C422`, and
  `4456D0` (BuildingClass vt+0x4E0, no function).
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

Parasite residuals (recorded in `combat/parasite.rs`): squid grapple `6297F0` (squids do not
LimboLaunch); paralysis in Drive/Ship/Fly/Hover/Teleport movers and player-control +0xA0
(Drive/Ship owner); grinder `73A13E`, ChronoWarp `6CC763` and Magnetron `710026` releases wait
for those mechanisms; Team membership memo `6FF7A3`; Sonic may target allied infected Foot
`700377` (squid). Follow-up spawned: full CanEnterBunker (`70FB50` Turret and +67C gates).

Also queued from the parasite work: Enter_Idle_Mode Area Guard arm (Infantry `51CD3E..`, Unit
`738B67..`; DefaultToGuardArea, GUARD_AREA ability, IQ vs Rules+1440, slave links); Inviso
(instant) deliveries skip the special detonation arms (squid, possibly other Inviso specials);
skip limboed attackers in the combat pass; ToProtect response not gated on the damage result.
Done since: the spawn-manager alive-child guard (#448); the death debris and Unit/Aircraft
Explosion=/DestroyAnim= picks now draw on the Scenario stream (`7022C8`, `70232B`, `7386A7`,
`73881D`, `41663C`), with only the death sounds on the main stream (`feature/combat-death-rng-stream`).

Whole-combat gaps (plan list plus review coverage top 10):
- Special warheads: 8 bodies no-op (`projectile.rs:856`); Parasite ported (squid residual), mind
  control ported (Psychic Dominator residual), Temporal ported (teleport-writer freeze residual).
- Found by the Temporal review, not Temporal defects: VERA's Stop assigns a Stop mission the native
  IDLE event never assigns (`4C74CB`), which every Attack idle exit after Stop depends on;
  Evaluate_Candidate lacks the rest of its GetFireError probe (`6F7CE8`); the null-destination
  helper never clears the NavQueue (`741970` mode 1); production ignores FindFactory's online
  argument (`5F7900`), which the power toggle and triggers (GoOffline `452360`) also clear.
- Open-topped passenger fire: VERA fires a passenger's weapon from the transport, so a Chrono
  Legionnaire or Yuri in a Battle Fortress has no TemporalClass or CaptureManager behind the shot;
  the Temporal Update's OpenToppedWarpDistance release is ported but has no producer yet.
- Destruction: construct each death object at its native call (voxel debris, debris anims,
  death anims, InfDeath anims and the death weapon's impact anim inline in the receiver, the
  outer impact anim after its receivers; the fatal prelude after the Techno death arm; verify the
  InfDeath anim's native constructor arguments); the ship sink (`+3CD`, `vt+3A0`, the
  `UnitClass::AI` sinking); a crushed vehicle's `Death_Explosion` (`746D60`); the `Crashable=`
  crash; the building NowDead contact loop (`442511`, radio 0x17 and the C4 kill of contacts),
  AnimClass::Middle for every explosion anim, the deferred death of `Explodes=`/Selling
  buildings, death specials, the sale crew, passenger escape from dying transports.
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
Parasite candidate (after critic fixes): `cargo test -p vera20k --lib` 9216 passed, 0 failed,
135 ignored (15 new); Clippy pass, 1022 warnings; no replay pin moved (the v193 fold adds only
parasite/paralysis state). Release `parity-digest` Dustbowl.mmx, seed `0x00C0FFEE`, 30 ticks:
two identical runs.
Death-Stun candidate (after critic fixes): `cargo test -p vera20k --lib` 9219 passed, 0 failed,
135 ignored (three new production-frame checks, five late-timing pins flipped, no replay pin
moved; the refused-Restore test fails with the refusal disabled); Clippy pass, 1022 warnings.
Release `parity-digest` (same map, seed, 30 ticks, before the critic fixes): two runs identical
to each other and to the parasite base (no deaths in that window, so this checks the load path
only).
Crew-survival candidate (after critic fixes): `cargo test -p vera20k --lib` 9233 passed, 0 failed,
136 ignored (17 new crew tests plus the ignored retail Dustbowl run: 8 seeds, 6 plant and 4 MCV
crewmen scatter through FNPC off their spawn cells); Clippy pass, 1020 warnings. No replay pin
moved (no pinned fixture kills a crewed building or vehicle). Release `parity-digest` Dustbowl,
seed `0x00C0FFEE`, 30 ticks: identical to the pre-change digest (load path only).
Death-anims candidate (after critic fixes): `cargo test -p vera20k --lib` 9241 passed, 0 failed,
137 ignored (8 new, one of them the ignored retail Dustbowl run pinned to its seed); Clippy pass,
1020 warnings, none new. Release `parity-digest` Dustbowl, seed `0x00C0FFEE`, 30 ticks: two runs
identical to each other and to the crew-survival digest (load path only; no death in the window).
