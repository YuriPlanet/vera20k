# Evidence for the revised gameplay goal boundaries

Companion to the [unified gameplay guide](2026-09-11-gameplay-goal-catalog.md), examined on
2026-09-11 against main `ed8f4837910be9329505c3dfc2fc074d9c1f3106`.
This records planning evidence, not a permanent progress ledger.

**Read one relevant E-section as an entry point.** Its source paths and native
reports are leads for the selected action, not a mandatory reading list. The
2026-09-11 source-status statements below describe the inspected commit; check
current source before carrying a missing/implemented claim into a new prompt.
The catalogue's scope definitions are separate from these dated observations.

Current Rust callers/state/consumers were read directly. Existing native reports
and local retail data support intended relationships; their historical Rust
status was not treated as current. No new binary execution, game test, downloads
or installations were performed. Exact timing/numeric/branch parity requires
the selected goal's own evidence. Shared Rust code alone cannot prove the native
engine uses the same boundary.

## E1 Resources and mining

**Current production path.** The Unit branch of
[the object AI host](../../src/sim/world/techno_ai.rs) calls
`dispatch_harvest_for_object`. The
[Harvest dispatcher](../../src/sim/miner/harvest_mission.rs) uses the entity's
mission cursor and timer, then calls the common
[miner state machine](../../src/sim/miner/miner_system.rs).
War/Chrono are branches in that owner, including the return/contact decisions;
they are not independent harvesting systems. Both reach the
[refinery sequence](../../src/sim/miner/miner_dock_sequence.rs), which follows
admission, unload, payout, release and resumption. Payout reads the refinery
owner and purifier/income modifiers; source/refinery loss has explicit abort
and interruption paths.

**The resource connection is an actual data path.**
[Terrain spawning](../../src/sim/terrain_spawn.rs), `place_tiberium_empty`, calls
the shared `tiberium::place_tiberium` in the loaded production context. The
[resource authority](../../src/sim/tiberium/mod.rs) reads and changes live
OverlayGrid type/density, handles growth/dirty effects, and is consumed by the
miner's `handle_harvest` through `reduce_tiberium_at_with_native_context`.
[Growth/spread](../../src/sim/ore_growth.rs) uses the same cell mutation context.
`ResourceNode` is a compatibility surface, not the authoritative loaded-map
resource store. This distinction matters when tracing a goal: an isolated test
of that compatibility store does not exercise TIBTRE-to-miner production.

**Native support.** The
[HARV/CMIN harvest report](../research/HARVEST_ORE_TICK_TIMING_PARTIAL_FULL_EDGE_CASES_ORE_GEMS_GHIDRA_REPORT.md)
identifies the shared `Mission_Harvest` → `Harvest_Ore_Tick` → `Reduce_Tiberium`
path for both stock miners and ore/gems. The
[refinery synthesis](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONO_MINER_REFINERY_DOCK_UNLOAD_SYSTEM_MODEL_SYNTHESIS.md)
records their common Harvest return decision with different type/rules branches.
The [TIBTRE placement report](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_PLACETIBERIUM_DENSITY_OVERLAY_QUEUE_EFFECTS_GHIDRA_REPORT.md)
traces terrain AI → forced spread → permitted new-cell placement → growth queue
and tactical/radar dirty state. It specifically distinguishes that new-cell
branch from growth of an existing field.

**Decision:** R1 joins War/Chrono, source/field behavior, docking and settlement
as a connected workstream. A narrower selected source/dock outcome still reaches
actual resource/miner/income consumers. Do not give the TIBTRE caller every
permissive behavior of its shared placement helper.

**Limits:** the old TIBTRE synthesis describes a detached Rust spawner and missing
live terrain lifecycle; that status is stale against current terrain ownership.
Existing research also contains corrected harvest timing. No new exhaustive
source/field/return/save/visual comparison was run for this catalogue.

## E2 Slave economy

[Slave harvesting](../../src/sim/slave_miner.rs) owns master/worker state and
`SearchOre → MoveToOre → Harvest → ReturnToMaster → Deposit`. It uses shared
`extract_bale`/resource access and purifier/income helpers, but `handle_slave_deposit`
credits whole storage slots through the master relationship. Its nearby native
evidence identifies `SlaveManagerClass::AI_Update` and the slave storage deposit
call. Standard Harvest dispatch explicitly excludes the slave host.

**Decision:** retain R2 as a related complete workforce loop, rather than a third
copy of the standard miner task. Changes to shared resource or payout contracts
require both R1/R2 consumer checks. Matching wallets and ore cells does not erase
worker assignment, liberation, master loss or deployment responsibilities.

## E3 Purchases and base lifecycle

[Factory lifecycle](../../src/sim/production/factory_lifecycle.rs) creates and owns
the held object through enqueue, completion and release/consumption.
[Production delivery](../../src/sim/production/production_queue.rs) and
[placement](../../src/sim/production/production_placement.rs) consume that identity
for mobile products, buildings and walls. This supports B1's consolidation of
queue/charging/completion/exit/placement with category/faction variants.
The [FactoryClass study](../research/FACTORYCLASS_PRODUCTION_DEEP_DIVE.md)
provides native owner context; it is not current implementation certification.

[MCV conversion](../../src/sim/world/world_spawn.rs), `deploy_mcv` and reverse
conversion, creates a replacement and transfers/removes its source independently
of the purchase queue. The
[MCV lifecycle synthesis](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MCV_DEPLOY_UNDEPLOY_LIFECYCLE_SYSTEM_MODEL_SYNTHESIS.md)
supports grouping stock MCV variants and distinguishes Slave Miner behavior.

[Repair and sale](../../src/sim/production/production_sell.rs) share a file but
not their transaction: repair changes a live building's HP/flag/wallet; sale
releases dependents, removes the building and settles value/capabilities.
The [native sale/repair report](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_SELL_AND_REPAIR_GHIDRA_REPORT.md)
likewise describes different handlers. The current
[tick owner](../../src/sim/world/mod.rs) records the repair-versus-factory debit
ordering seam. These observations justify B3/B4 separation and joint checks at
the wallet/scheduler boundary, not a merged “money operations” task.

**Limits:** exact pricing/refund rules in older reports have corrections and
qualifications. B1's Industrial Plant integration is grounded in price consumers
and retail `FactoryPlant`/cost-modifier fields; a full Cloning Vat acquisition,
second-product and delivery trace is still required before its implementation
scope is final. Placing it with production is a consumer-based proposal, not a
claim of a completed current clone path.

## E4 Power capture and benefits

[Power state](../../src/sim/power_system.rs) consumes provider/occupant state and
feeds building/radar changes; [factory stepping](../../src/sim/production/factory.rs)
consumes the resulting production conditions. The
[power research](../research/POWER_SYSTEM_GHIDRA_REPORT.md) is a starting reference,
but its generic Robot power explanation does not establish the complete stock
`ROBO PoweredUnit` / `GAROBO PowersUnit=ROBO` provider relationship. The catalogue
keeps that exact gate open for investigation.

[Engineer resolution](../../src/sim/world/world_orders.rs) reaches
`change_owner_with_rules` and consumes the engineer. The central
[ownership transaction](../../src/sim/world/mod.rs), `change_owner_impl`, updates
tracking/counts/sensors/base authority and capture income;
[credit income](../../src/sim/credit_income.rs) handles relevant capture/startup
and timed-cash consumers. Native relationship support is in
[ChangeOwner lifecycle](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_CHANGEOWNER_LIFECYCLE_ORDER_RESWARM_20260528.md).

**Decision:** B6 owns capture → benefit → loss as an observable outcome, but
production, income, unlocks and passive healing retain their actual state owners.
Mind control and slave liberation reuse ownership transfer while keeping their
different initiators and release rules. Hospital/Machine Shop retail fields
establish passive-benefit classification; their complete current consumer census
was not established here. Grinder and remaining special-benefit exactness are
explicitly targeted investigations, not proven equivalent admission paths.

## E5 Movement stance and cargo

[Camera/input state](../../src/app/input/camera.rs) and
[context-order admission](../../src/app/input/context_order.rs) include real
bookmark/selection/action decisions before a simulation command exists. This
supports a selectable battlefield-control goal through actual execution and
replacement. U1/U2 tests that directly inject commands cannot certify that input
loop; the revised catalogue explicitly retains its named interaction variants.

[Movement](../../src/sim/movement/mod.rs) and the
[installed locomotor slot](../../src/sim/movement/locomotion/slot.rs) connect orders,
movement state and lifetime. The
[movement ownership study](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md)
supports the cross-system action boundary; historical Rust status is excluded.
This supports U1, not an instruction to complete every locomotor before combat.

[Deploy commands](../../src/sim/world/world_commands.rs),
[stance state](../../src/sim/deploy.rs) and
[mission handlers](../../src/sim/world/techno_ai/mission_handlers.rs) connect
GI/GGI sustained stance to actual combat. The latter explicitly branches Yuri
pulses and Desolator radiation away from that behavior.
[GGI command evidence](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/GGI_DEPLOY_COMMAND_STANCE_TRACE.md)
supports the shared command/stance relationship. Similar input is insufficient
to merge those other effects or Siege Chopper flight into U3.

[Passenger admission](../../src/sim/passenger.rs),
[departure](../../src/sim/passenger/departure.rs) and
[vehicle unloading](../../src/sim/transport_unload.rs) connect cargo membership,
concealment, weapon changes, movement and release. Native
[IFV/OpenTopped evidence](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md)
supports keeping IFV/BFRT inside the complete passenger loop while preserving
host-versus-passenger firing responsibilities. One shared current representation
is not proof that those native firing owners can be collapsed.

Garrison admission, ownership reconciliation and combat use a building occupant
relationship, supported by the
[garrison synthesis](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_MODEL_SYNTHESIS.md).
By contrast, [BunkerLink](../../src/sim/docking/bunker_link.rs) and
[entity state](../../src/sim/game_entity.rs) define a reciprocal single-unit link
for `NATBNK`, explicitly distinct from cargo. The
[Tank Bunker entry/exit report](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TANK_BUNKER_ENTRY_EXIT_VISIBLE_LIFECYCLE_GHIDRA_REPORT.md)
supports U6's separate full loop. Soviet `NABNKR` remains an infantry garrison
variant. The original catalogue's generic “bunker” wording missed this scope.

## E6 Sorties pools and attached effects

[Aircraft state](../../src/sim/aircraft/mod.rs) and
[attack missions](../../src/sim/aircraft/attack_mission.rs) connect Harrier/Black
Eagle attacks to pads, return and service. Native context is in the
[aircraft mission family](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AIRCRAFT_MISSION_VERB_OVERRIDE_FAMILY_GHIDRA_REPORT.md).
Two coexisting current docking state machines require investigation; the grouping
does not bless duplicate authority. Jumpjet users have different continuing
flight/landing rules and are not airfield-sortie variants.

[SpawnManager](../../src/sim/spawn_manager.rs) owns pool creation, target handoff,
launch, returning/expendable child state and regeneration; its
[native report](../research/SPAWN_MANAGER_CLASS_GHIDRA_REPORT.md) supports grouping
Carrier/Destroyer with V3/Dreadnought/Boomer. Parent/child expiry reaches
[common lifecycle](../../src/sim/world/lifecycle.rs). Boris instead enters an
AirstrikeClass path identified in [projectile dispatch](../../src/sim/projectile.rs).
That is a substantive owner difference, not just an aircraft-type distinction.

[CaptureManager](../../src/sim/capture_manager.rs), construction and pointer
expiry provide current state seams, while
[mind-control research](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MIND_CONTROL_SYSTEM_GHIDRA_REPORT.md)
distinguishes reversible controller-owned capture from the Dominator's permanent
ownership effect. Group reversible controllers with their capacity/overload
variants. Acquisition and the full effect path are not currently established:
[special detonation](../../src/sim/combat/world_receiver.rs) routes several claimed
effects to an unimplemented endpoint.

The same limitation applies to using current code as proof of complete Parasite,
Temporal or Magnetron behavior. Their intended distinct relationships come from
the [Parasite](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARASITE_CLASS_GHIDRA_REPORT.md),
[Temporal](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TEMPORAL_WEAPON_SYSTEM_GHIDRA_REPORT.md) and
[Magnetron](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAGNETRON_SYSTEM_GHIDRA_REPORT.md) owner studies. Drone/Squid
share the parasite/host family; erasure and locomotor replacement have different
progress/termination. Older broad reports contain incorrect identifiers/offset
tables; this revision uses their owner distinctions, not those details as new
implementation specifications.

## E7 World and information

[Engineer/hut interaction](../../src/sim/world/world_orders.rs) reaches
[the bridge walker](../../src/sim/bridge_state/walker.rs), low/high repair,
overlay/zone/radar refresh and engineer removal. The
[native bridge trace](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/08-traces/ENGINEER_HIGH_BRIDGE_REPAIR_MUTATION_TRACE.md)
supports that chain; its old missing-Rust admission/radar findings are stale.
High/low/orientation are variant coverage of the integrated bridge lifecycle.

[Terrain lifecycle](../../src/sim/terrain_object.rs), combat finalization and
[terrain presentation](../../src/app/presentation/instances/overlays.rs) connect
ordinary scenery occupation and removal to its actual visual/spatial result.
Source flags determine interactions; no generic tree-crushing claim is justified.

[Vision reconciliation](../../src/sim/vision/mod.rs),
[sensor lifecycle](../../src/sim/sensor_lifecycle.rs) and
[radar availability](../../src/sim/radar.rs) have related consumers but different
state: shroud knowledge/source latches, counted detection circles, and powered
radar providers. Native references are
[Gap/radar interaction](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GAP_RADAR_SHROUD_MINIMAP_INTERACTION_GHIDRA_REPORT.md)
and [Psychic Sensor intent lines](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PSYCHIC_SENSOR_ENEMY_ACTION_LINES_RECHECK_GHIDRA_REPORT.md).
Psychic intent warnings require their own eligibility; they are not interchangeable
with cloak detection or map reveal. Whole information-building goals still
include the shared viewer/targeting consumers.

## E8 Strategic powers

[Superweapon instances](../../src/sim/superweapon/mod.rs) own grant/revoke/charge;
[command dispatch](../../src/sim/world/world_commands.rs) hands off the selected
effect and recharge. The
[native lifecycle study](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_SYSTEM_CONSOLIDATED_REPORT.md)
supports shared support ownership with power-specific effects.

Retail `ChronoWarp` explicitly has `PostClick=yes` and
`PreDependent=ChronoSphere`; the
[two-stage native handler](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md)
supports an inseparable complete teleport action. American/Tech Airport drops
are provider/payload variants requiring both checks. Distinct storm, mutation and protection owners require separate effect evidence
and comparisons, but do not establish separate goal-session boundaries. A retail
strategic-powers goal can own shared lifecycle and all those effects through
several increments. Full current effect coverage was not audited in this revision.

## E9 AI scenarios and session flows

[Current AI](../../src/sim/ai.rs) emits ordinary gameplay commands through the
[production tick](../../src/sim/world/mod.rs); actual outcomes feed later
decisions. Native [base placement](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_HOUSECLASS_ORDINARY_BASE_PLACEMENT_005060B0_GHIDRA_REPORT.md)
and [Team reachability](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PHASE3_TEAM_PRODUCTION_REACHABILITY_GHIDRA_REPORT.md)
support grouping decisions with their real production/recruitment/action
consumers. Existing attack waves are playable behavior, not proof of native Team
equivalence. [The Team VM](../../src/sim/team_script_vm.rs) supports a bounded
action set; advancing a cursor does not establish the actual scripted action.

[Trigger runtime](../../src/sim/trigger_runtime.rs) instead owns persistent event
conditions/latches and action ordering, reaching real
[simulation/app consumers](../../src/app/match_runtime/sim_tick.rs). It shares
Team/command capabilities with AI while retaining a distinct scenario lifecycle.

[Shell routing](../../src/app/shell_main_menu.rs),
[skirmish session](../../src/app/frontend/skirmish_session.rs),
[load transitions](../../src/app/loading/transitions.rs) and
[session exit](../../src/app/match_runtime/scenario_exit.rs) establish full
launch/play/return as a coherent flow. Native
[Single Player dispatch](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SINGLE_PLAYER_SUBMENU_DIALOG_CASE1_GHIDRA_REPORT.md)
establishes destination structure, not complete campaign semantics. Current
campaign selection lacks launch; Network/WOL routes report unimplemented.
A shell-route task is therefore bounded separately from destination backend
completion, while an “all destinations work” claim remains open.

## E10 Persistence networking and presentation

[Persistence commands](../../src/app/persistence/commands.rs),
[prepared replacement](../../src/app/persistence/mod.rs) and
[restore](../../src/app/match_runtime/restore.rs) form one save/resume path,
including clearing old effects and rebuilding visual/UI state. Current restoration
requires an existing matching simulation/map/rules context; this is not proof of
cold-start save loading. Native context:
[save/load frontier](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/core-services-map/frontier-saveload.md).

[Replay](../../src/sim/replay.rs) has a separate identity/initialization/playback
flow. Native [replay/save distinction](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/REPLAY_ACTIVE_VECTOR_RESTORE_CORNER_RESWARM_20260528.md)
explicitly distinguishes scenario reinitialization from saved-object restoration.
[Net staging](../../src/net/mod.rs) and offline
[live session selection](../../src/app/match_runtime/sim_tick.rs) do not provide a
complete host/join protocol. [Native frame scheduling](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NETWORK_FRAME_SCHEDULING_GHIDRA_REPORT.md)
is support evidence for LAN, not an implementation claim. Save, replay and LAN
share deterministic foundations but have different inputs/state/termination.

[Instance projection](../../src/app/presentation/render/build_instances.rs),
[sound dispatch](../../src/app/match_runtime/sound_dispatch.rs) and
[lighting](../../src/app/presentation/lighting.rs) consume real gameplay state
and events. [Light lifetime research](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LIGHTSOURCE_LIFECYCLE_POWER_DAMAGE_SAVELOAD_GHIDRA_REPORT.md)
connects power/death/sale/save with presentation and retains an outer restore-call
uncertainty. This supports per-goal presentation/cleanup obligations, not a later
isolated “all rendering” phase. Shared cross-consumer defects and media playback
can still be complete separately selected outcomes.

## E11 Implementation grouping

This additional source inspection used the same game-source baseline as E1–E10
(main HEAD was still `ed8f4837910be9329505c3dfc2fc074d9c1f3106`; the documentation
branch contained only catalogue revisions). These are implementation-leverage
inferences, not new native-parity findings or measured productivity gains.

- **Standard miners and resource supply:** re-read
  [Harvest dispatch](../../src/sim/miner/harvest_mission.rs),
  [miner state machine](../../src/sim/miner/miner_system.rs) and
  [slave harvesting](../../src/sim/slave_miner.rs). War/Chrono decisions live in
  the common owner, including distinct far-return paths; slave hosts are excluded
  from that dispatcher. E1 traces resource production/consumption; E2 distinguishes
  the worker lifecycle. This supports common miner work and conditional shared
  resource fixes, not automatic full Slave Miner implementation in every R1 goal.
- **Deployed infantry:** the
  [mission handlers](../../src/sim/world/techno_ai/mission_handlers.rs) carry the
  `infantry_deploy_fire_stance` predicate and shared stance consumers. The predicate
  distinguishes GI/GGI sustained behavior from radiation/pulse variants. Together
  with E5 this supports joint GI/GGI work, with consumer checks for shared changes.
- **Factory products:** [factory lifecycle](../../src/sim/production/factory_lifecycle.rs)
  owns enqueue/cancel, completion and held-object disposition, with separate mobile
  release and placement terminal paths. E3's conversion and repair/sale owners
  prevent turning that concrete reuse into an all-building-operations assignment.
- **Spawn pools:** [SpawnManager](../../src/sim/spawn_manager.rs) owns common
  parent/slot state and target handoff, with missile-family/return branches. Child
  flight/firing remains delegated. The reuse is strongest for pool lifecycle;
  a complete attack still requires the actual child to work. E6 covers Boris's
  separate owner and other aircraft relationships.
- **Protection:** [Iron Curtain](../../src/sim/superweapon/iron_curtain.rs) and
  [Force Shield](../../src/sim/superweapon/force_shield.rs) both call
  [apply_invulnerability](../../src/sim/superweapon/invulnerability.rs).
  [Damage reception](../../src/sim/combat/receiver_health.rs) consumes the shared
  timer state. Their launch bodies select recipients differently; Force Shield
  also writes house blackout state. Joint protection research/change/validation
  is therefore plausible, without assuming either selection algorithm is parity.
- **Paradrops:** [command dispatch](../../src/sim/world/world_commands.rs) routes
  `ParaDrop` and `AmerParaDrop` into the same
  [launch handler](../../src/sim/superweapon/paradrop.rs). `ParaDropKind` selects
  the payload list; carrier construction continues through `spawn_pdplane`.
  This is stronger implementation overlap than simply sharing an aircraft theme.
- **Other strategic effects:** that dispatch shares ready admission and successful
  recharge but calls separate storm, mutation and reveal owners. At this baseline
  remaining kinds reach its unimplemented fallback; that is a finding about this
  command path, not proof no related code exists anywhere. No complete current
  nuke/Chronosphere/Dominator/Spy Plane implementation trace was established here.
  [Psychic Reveal](../../src/sim/superweapon/psychic_reveal.rs) reaches vision;
  [mutation](../../src/sim/superweapon/genetic_converter.rs) reaches replacement
  work. Their common launch surface alone does not establish cheap joint effects.

Use these candidates to inspect the actual remaining gap before committing to a
prompt scope. Existing implementation overlap can itself contain incorrect native
assumptions; retail evidence still determines behavior, and no new runtime checks
were performed for this documentation revision.

## E12 Further implementation groups

Second-pass inspection uses the same source baseline as E11. Findings below
support implementation scope selection, not retail equivalence or measured effort.

**Cargo crosses the previous rows.**
[Passenger state](../../src/sim/passenger.rs) owns head-first membership and size;
[departure](../../src/sim/passenger/departure.rs), `depart_cargo_head`, owns removal
and failed-attempt restoration. Actual callers select `Garrison` in passenger.rs,
`Vehicle`/`LandedAircraft` in
[transport unloading](../../src/sim/transport_unload.rs), and `Paradrop` in
[drop payload](../../src/sim/aircraft/drop_payload.rs). Route-specific rollback and
weapon-reset policies remain explicit. This is concrete joint work across U4/U5/S8,
not evidence their complete lifecycles are one state machine. Passenger boarding
also branches on Gunner/OpenTopped; [weapon selection](../../src/sim/combat/combat_weapon.rs)
selects host gunner slots separately from occupied-building substitution. Membership
changes should check these consumers; a full passenger-fire implementation is more
than changing the common cargo list. E5's NATBNK link remains outside this group.

**U9 is broad implementation scope.**
[Air movement](../../src/sim/movement/air_movement.rs) calls
[Jumpjet altitude/acceleration/landing decisions](../../src/sim/movement/jumpjet_movement.rs).
That supports joint active-locomotor variants, while cargo and weapon/deployment
consumers remain additional work. For example, Disc
[drain links](../../src/sim/credit_income.rs) have establishment, continuing effects
and expiry outside flight. Do not use legacy helper comments to admit unreachable
TS variants; the active-YR gate still applies.

**Information has a concrete cross-row consumer, but not one universal effect.**
[Psychic Reveal](../../src/sim/superweapon/psychic_reveal.rs) calls
[vision](../../src/sim/vision/mod.rs), `reveal_radius_for_direct_allies`, which
publishes knowledge for admitted viewers and invalidates the merged cache. W4/S10
therefore overlap for changes to that propagation/publication. By contrast,
[sensor lifecycle](../../src/sim/sensor_lifecycle.rs) deposits/removes detection
counts and reevaluates residents; it is not the same knowledge-producing action.
The native interpretation of each source remains a selected goal's evidence work.

**Power is not permission to assume working effect admission.**
[Power](../../src/sim/power_system.rs) consumes occupant counts for absorb-building
output, and owns blackout countdown. Force Shield writes its blackout field, but
`trigger_spy_blackout` has only a test caller in the inspected source search.
Disc drain's current link helper records power integration as residual. Thus B5,
X19/X20 and S4 have potential consumer overlap, but full Spy/Disc effects cannot be
called cheap variants of an already-complete power path. Trace their admission and
missing handoffs before expanding a power goal.

**Options are shared across screens, navigation is additional work.**
[Options persistence](../../src/app/persistence/options.rs),
[launcher projection](../../src/app/persistence/options/launcher.rs) and
[shell menu](../../src/app/shell_main_menu.rs) use `RetailOptionsProfile` and
`persist_options_profile`. Profile/persistence and affected consumers belong
together across those entries, with distinct preview/commit/close policies.
This does not establish shared implementation of campaign/network destination flows.

**Save entry points converge after their admission.**
[Persistence commands](../../src/app/persistence/commands.rs) route quickload and
panel loading through `load_save_file` into
[load preparation](../../src/app/persistence/mod.rs), `PreparedLoad::from_repository`,
and [restore commit](../../src/app/match_runtime/restore.rs), `commit_prepared_load`.
`LoadPreparationView::from_runtime`
requires existing rules/simulation/terrain. This supports joint same-context restore
work and explains why cold-start loading is a separate required focus for full F8.

**Fixed/generated map launch has a bounded overlap.**
[Skirmish start](../../src/app/shell_skirmish.rs), `start_skirmish_session`, attaches
accepted random-map artifacts to the common loading request.
[Random-map state](../../src/app/shell_random_map.rs), `RandomMapGenerationRetention`
and `AcceptedRandomMapLaunch`, distinguishes accepted staging from preview; gameplay
regenerates from the seed path. Check both map sources when changing shared launch,
without treating generator algorithms or match results as part of the same change.

**AI contains distinct implementation owners.**
[AI decisions](../../src/sim/ai.rs), `tick_ai`, emits production/deployment/placement
commands. [World Team dispatch](../../src/sim/world/mod.rs), `run_team_script_pass`,
invokes `TeamScriptVm::tick_effects` and applies those effects separately. F4 needs
both for the whole opponent result; they are distinct implementation focuses,
not evidence that one broad AI pass automatically finishes both.

All coverage rows and their full-scope obligations remain available. Unchanged
groupings are not newly certified by this pass. No runtime/native execution or
installation was performed; exact semantics and current remaining gaps still need
targeted validation when a goal is selected.

## E13 Movement implementation evidence

Read-only source audit at the same baseline as E11. Common order admission and
`queue_megamission_with_teardown` live in
[world commands](../../src/sim/world/world_commands.rs); destination/stop handling in
[movement commands](../../src/sim/movement/movement_commands.rs); installation and
restoration span [install](../../src/sim/movement/locomotion/install.rs),
[slot](../../src/sim/movement/locomotion/slot.rs) and
[piggyback](../../src/sim/movement/locomotion/piggyback.rs). Execution is owned by
[object turns](../../src/sim/world/object_turn.rs), not a proposed global sweep.

- **M1 Drive/Ship:** [movement step](../../src/sim/movement/movement_step.rs)
  `shared_track_kind`, `accept_shared_track`, `configure_motion_after_transition`
  and `advance_lepton_position` handle both, retaining separate fields/branches.
- **M2 Walk:** [ground ticking](../../src/sim/movement/movement_tick.rs) shares
  path/crossing infrastructure; movement step and
  [blocked movement](../../src/sim/movement/movement_blocked.rs) retain Walk branches.
- **M3 Hover:** movement step's `hover_steer` and
  [Hover](../../src/sim/movement/hover.rs) `hover_tick_throttle`/`hover_vertical_tick`
  own distinct steering/height work within ground movement.
- **M4 Fly/Jumpjet:** [air movement](../../src/sim/movement/air_movement.rs)
  `tick_air_movement` is called through
  [lifecycle](../../src/sim/world/lifecycle.rs) `tick_air_movement_with_cell_lists_one`.
  Its branches reach distinct Fly handling and Jumpjet altitude/acceleration.
  E6 covers the separate airfield/pool/cargo outcomes; shared motion is not proof
  of a cheap full-aircraft-family implementation.
- **M5 Teleport:** movement commands' `set_destination_for_teleporter_entity`
  and [teleport movement](../../src/sim/movement/teleport_movement.rs)
  `issue_teleport_command`, `issue_active_teleport_head_to_coord`,
  `tick_teleport_movement` connect destination and relocation.
  [Movement owner](../../src/sim/movement/mod.rs) `tick_locomotor_piggyback_restore_one`
  handles restoration. Ordinary installed Teleport and temporary override paths
  must not be conflated; E1 identifies the miner caller.
- **M6 Rockets:** [rocket movement](../../src/sim/movement/rocket_movement.rs)
  `process_rocket_state`/`tick_rocket_movement` owns phases and payload and is
  excluded from ordinary air ticking. Object turns collect completions;
  [world](../../src/sim/world/mod.rs) hands them to SpawnManager detonation.

The inspected `locomotor_end_gate_context` explicitly records non-Drive end gates
as owner-path approximations with native equivalence unchecked. These groupings
therefore establish code overlap, not correctness. Follow E5/E6's native leads,
recheck consequential branches and validate real orders and continuation in the
selected goal. No native execution or movement runtime test was run for this audit.

## E14 Rendering implementation evidence

Read directly at the E11 baseline; implementation overlap is an inference from
these owners, not new proof of native semantics or whole-renderer completeness.

- **Lighting and palette consumers (V1):**
  [MatchLighting and drawer selectors](../../src/app/presentation/lighting.rs)
  own grid replacement and body/building/animation light selection.
  [PaletteLight](../../src/render/palette_light.rs) and its
  [shared shader](../../src/render/palette_light.wgsl) carry the row/scalar/index
  policy into [tactical shader assembly](../../src/render/tactical_shader.rs).
  [Existing LightConvert research](../research/LIGHTCONVERT_ROW_RGB565_ORACLE_2026_09_09.md)
  supplies native arithmetic/fixture context with explicit limits. Cell versus
  house ColorScheme and animation palettes remain different selectors; aircraft
  brightness is explicitly unresolved in the inspected presentation owner.
- **Voxel body and cache (V2):** [raster preparation](../../src/render/vxl_raster.rs)
  routes ordinary encoded VXLs through [native preparation](../../src/render/vxl_native.rs)
  shared with GPU visibility writes. [Unit atlas](../../src/render/unit_atlas.rs)
  keys facing/part/frame/slope, while the
  [transition cache](../../src/render/unit_slope_transition_cache.rs) materializes
  visible blended slopes. Magnified previews retain a distinct compatibility path.
- **Shadows (V3):** [voxel geometry](../../src/render/vxl_shadow.rs) explicitly
  supports flat, frame-zero, one-section ordinary input; other cases retain fallback.
  Atlas and [unit-instance admission](../../src/app/presentation/instances/units.rs)
  additionally restrict this route to ordinary Ground-band, uncloaked Drive
  callers. The atlas owns masking/cache state. [Shadow research](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/VEHICLE_SHADOW_VISIBLE_MATCH.md)
  separates shape/mask fixtures from final destination-darkening limitations.
  Joint destination work is conditional on actual consumers; this does not prove
  SHP, aircraft and voxel shadow geometry are interchangeable.
- **Ordering (V4):** [draw plan](../../src/render/tactical_draw_plan.rs) owns layer
  and RenderZPolicy; [instance building](../../src/app/presentation/render/build_instances.rs),
  [lowering](../../src/app/presentation/render/draw_plan_lowering.rs) and
  [pass dispatch](../../src/app/presentation/render/draw_passes.rs) preserve ground
  parents and upper-layer submissions. This is concrete cross-format overlap.
- **Modified pixels (V5):** `opaque_palette` in the palette shader disables that
  path for alpha/FX input, and `resolve_palette` retains compatibility branches.
  [Batch shader](../../src/render/batch_shader.wgsl) supplies alpha and effect flags.
  These observations establish a boundary requiring additional native blitter
  investigation, not a proven universal translucency/cloak implementation group.
- **Combat lights (V6):** [render orchestration](../../src/app/presentation/render/mod.rs)
  prepares current combat lights and a composition target;
  [combat-light renderer](../../src/render/combat_light.rs) and
  [mask shader](../../src/render/combat_light.wgsl) operate on scene pixels. This
  differs from per-cell lighting. The shader also distinguishes projected center
  from its fixed surface footprint, so camera/zoom assumptions need caller checks.
- **Searchlight (V7):** [BuildingLight rendering](../../src/render/building_light.rs)
  and pass submission exist, but `build_instances` currently constructs an empty
  `spotlight_type16` vector. That is a concrete missing input on this route, not
  proof no other light implementation exists. Active source/registration and full
  beam behavior require targeted research before claiming production completion.

No new rendering APIs, dependencies or tools were installed. This inspection does
not certify pixel parity; use the native evidence and production-output checks
specified in the selected goal, and recheck this dated source state.

## Scope carried forward from the original catalogue

This is a migration index, not a progress chart. Old identifiers refer to the
catalogue at commit `5937c06c`. A repeated destination means integration or
variant coverage, not competing implementation owners.

| Previous entries | Revised home |
|---|---|
| P1–P4, P9 | F1–F3/F6–F7 and actual session outcomes. |
| P5–P7 | Selectable battlefield-control loop, U1/U2, B1/B3–B5, W4–W6, and shared input/presentation consumers. |
| P8 | Per-loop cues, shared voice/EVA/positional/music consumers, F7 media. |
| W1, W9 | F1/F2 content-to-world launch; every relevant loop's map/presentation consumers; shared composition corrections. |
| W2, W5–W8 | W2, W1, W3, W4/W5 and W7 respectively. |
| W3–W4, E1–E5 | R1/R2, including standard miner variants, terrain supply, docking and income. |
| E6–E9 | B1/B2 with actual placement/activation/product delivery. |
| E10–E11 | B3/B4/B5 as distinct repair, sale and provider lifecycles. |
| E12 | Purifier R1; Industrial Plant/Cloning Vat B1; Bio Reactor B5 with passenger integration; Grinder B8. |
| E13–E14 | B6 tech-benefit acceptance, B5 Robot/provider relationship; Tech Airport with paradrops. |
| U1–U6 | U1/U2, W5, B6; special combat dependencies remain with their actual effect owners. |
| U7–U10 | U4/B7/U7/U9, with applicable U8 spawned-child consumers. |
| A1–A4 | U3, U9, U4/U5. Add explicit Tank Bunker U6. |
| A5–A10 | Named distinct weapon/effect loops with B5/U2/W1 and persistent world consumers. |
| A11–A13 | U11/U10 with explicit parasite/controller variants. |
| A14–A17 | Chrono movement, Temporal, Spy infiltration and Magnetron whole effects. |
| A18–A19 | U8 launcher-owned pools, including returning and expendable variants. |
| A20–A23 | Boris, Dog, Disc and Yuri pulse full-effect goals. |
| S1–S10 | Strategic-power section, preserving all named powers and linked/drop variants. |
| S11 | W4 Gap/SpySat, W6 Psychic warning, with B5 provider dependencies. |
| G1–G4 | F4–F6: opponent, authored events and campaign continuation. |
| N1–N4 | F8–F11: complete save, replay, LAN and online flows. |

Remaining priority decisions require current-match evidence. Remaining exact
branch contracts require selected-goal native work. Neither is inferred from
how many catalogue rows a family now occupies.
