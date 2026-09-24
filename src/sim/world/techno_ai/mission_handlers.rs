//! Evidence-backed Foot/Unit mission-handler cadence evaluation.
//!
//! The object-AI host and its ordering remain in the parent module; this module
//! owns only handler inputs, results, and the single timer epilogue.

use super::{PASSIVE_SCAN_DELAY_JITTER_MAX, Simulation, can_acquire_target, passive_target_scan};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

/// Re-arm the evidence-backed Foot/Unit handler subset without duplicating the
/// legacy movement, combat, or target-selection systems.
///
/// YR `MissionClass::AI` at `0x005B3060` gates the current handler on the
/// dispatch timer and writes `(current frame, handler return)` afterward. The
/// existing movement/combat phases remain the sole owners of their path and
/// target side effects; this absorbs only proven handler-return cadence. Harvest
/// has its own full handler and epilogue, so miners are excluded to avoid a
/// second write. Target acquisition and approach-result producers are still
/// absent; their native routes remain explicit no-ops rather than guessed AI.
pub(super) fn dispatch_supported_foot_mission_cadence(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) {
    let now = sim.session.binary_frame;
    let input = {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return;
        };
        if entity.dying {
            return;
        }
        let category = entity.category;
        if !matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
            return;
        }
        let mission = entity.mission.current().known();
        // A miner's dispatch is owned by the absorbed Harvest handler, which
        // writes its own epilogue — except on Guard, which the Harvest handler
        // now declines. That is the native split: a vehicle on Guard enters the
        // harvester Guard override (`UnitClass::Mission_Guard @ 0x00740810`,
        // the human chrono arms live on the Unit Guard arm below), which
        // layers the slave/refinery checks and then tail-calls the same
        // FootClass Guard handler every other unit uses; a vehicle on Harvest
        // enters the Harvest handler. Exactly one of the two runs, so the
        // timer keeps a single writer.
        //
        // Move is the other split: a player Move order puts the miner on
        // `UnitClass::Mission_Move` (slot `+0x22C` = 0x00740A90, tail
        // `FootClass::Mission_Move @ 0x004D4200`), whose arrival
        // `Enter_Idle_Mode(0,1)` at 0x004D4242 returns a harvester to Harvest
        // (or Guard) — see `harvester_enter_idle_mode_evaluation`; the same
        // slot is also entered from the owner change (0x00701849) and the
        // depot's repaired-BREAK exit (0x004D92E2). The Harvest handler
        // declines Move for the same single-writer reason.
        //
        // Enter with a repair-depot `DockState` is the third split: a
        // harvester ordered to a depot (`Command::RepairAtDepot`) dispatches
        // through `FootClass::Mission_Enter @ 0x004D9290` exactly like every
        // other Foot object — `UnitClass`'s vtable `0x007F5C70 + 0x240` =
        // `0x007F5EB0` holds `0x004D9290` (`read_memory`, 2026-09-06), so the
        // Unit leaf does not override the slot, and `UnitClass::Mission_Harvest
        // @ 0x0073E5E0` is only reached when the committed selector is
        // Harvest(10). The Harvest handler declines Enter-with-depot for the
        // same single-writer reason (`harvest_mission.rs`).
        let depot_dock_state = entity.dock_state.is_some();
        let miner_enter_depot = mission == Some(MissionType::Enter) && depot_dock_state;
        if entity.miner.is_some()
            && !miner_enter_depot
            && !matches!(mission, Some(MissionType::Guard) | Some(MissionType::Move))
        {
            return;
        }
        let moving = entity.movement_target.is_some()
            || entity.navigation.nav_com.is_some()
            || crate::sim::movement::track_head::committed_track_head(entity).is_some();
        MissionHandlerInput {
            category,
            mission,
            harvester_miner: entity.miner.is_some(),
            depot_dock_state,
            timer_due: entity.mission.dispatch_timer().due(now),
            moving_or_queued: moving || entity.mission.queued() != MissionId::NONE,
            bunker_delegate: entity.bunker_link.installed_in().is_some(),
            has_attack_target: entity.attack_target.is_some(),
            // The destination slot alone, NOT the wider "is this object in
            // motion" test above: the idle-mode selector branches on exactly
            // that one field.
            has_destination: entity.navigation.nav_com.is_some(),
            effective_mission: entity.mission.effective().known(),
            unit_deploy_begin_active: entity
                .mission_leaf
                .as_unit()
                .is_some_and(|leaf| leaf.deploy_begin_active() != 0),
            unit_deploy_reverse_active: entity
                .mission_leaf
                .as_unit()
                .is_some_and(|leaf| leaf.deploy_reverse_active() != 0),
            // `InfantryClass::Mission_Attack @ 0x0051F3E0` branches at
            // `0x0051F4D3` on `[this+0x6C4] ∈ {0x1B, 0x1C, 0x1D, 0x1E}` —
            // Deploy, Deployed, DeployedFire, DeployedIdle in the sequence-name
            // table at `0x008255C8`, bounded by
            // `InfantryTypeClass::ReadSequenceData @ 0x00523D00`. `0x1F`
            // (Undeploy) is OUTSIDE the set, which is why this is not
            // `GameEntity::is_deployed()`, which admits the undeploying phase.
            //
            // The native gate also requires the owner to pass
            // `HouseClass::IsControlledByHuman @ 0x0050B730`. Every house in
            // VERA is human today, so that test is vacuously true; whoever
            // lands an AI opponent must add it here rather than inherit a
            // silent divergence.
            infantry_deployed_do_type: category == EntityCategory::Infantry
                && matches!(
                    entity.deploy_state,
                    Some(crate::sim::deploy::DeployPhase::Deploying { .. })
                        | Some(crate::sim::deploy::DeployPhase::Deployed)
                ),
            // Only resolved for an already-deployed infantryman: that is the
            // only branch of `FUN_00521320` these three flags gate, and the
            // type lookup is not free.
            infantry_deploy_fire_stance: category == EntityCategory::Infantry
                && entity.deploy_state.is_some()
                && sim
                    .interner
                    .try_resolve(entity.type_ref())
                    .and_then(|name| rules.object(name))
                    .is_some_and(|obj| {
                        obj.deploy_fire && !obj.immune_to_radiation && obj.undeploy_delay < 0
                    }),
        }
    };
    if !input.timer_due {
        return;
    }

    let evaluation = match (input.category, input.mission) {
        // `FootClass::Mission_Move` is the native named location for this
        // handler-return cadence; movement execution remains in movement/.
        // **Infantry take a leaf override first, and VERA does not model it.**
        // `InfantryClass`'s Move slot is `+0x22C` = `0x0051F660`, which gates on
        // `[this+0x6C4] ∈ {0x1B, 0x1C, 0x1D, 0x1E}` — a small per-infantry state
        // enum, identity UNCHECKED, the same field `0x00521320` reads — and for
        // a human-owned unit (`vtable+0x3C` → `HouseClass::IsControlledByHuman`
        // @ `0x0050B730`) calls `Set_Destination(NULL, true)` through `+0x480`
        // and returns 1, never entering `FootClass::Mission_Move` @ `0x004D4200`
        // and never drawing its jitter. Trigger: a deployed infantryman ordered
        // to move. Player effect: retail drops the destination and re-dispatches
        // next frame; VERA keeps the destination and draws the cadence jitter.
        // Frequency: GI, Guardian GI and Desolator deploy is routine, so this is
        // not rare. Downstream risk: it is one RNG draw per occurrence, so the
        // stream diverges too. **Note the frame trap**: `[this+0x6C4]` is a
        // `UnitTypeClass*` on UnitClass and this state enum on InfantryClass,
        // which caches its own type at `+0x6C0`.
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Move)) => {
            if input.moving_or_queued {
                MissionHandlerEvaluation::cadence(jittered_mission_cadence(
                    sim,
                    rules,
                    MissionType::Move,
                ))
            } else if input.category == EntityCategory::Unit && input.harvester_miner {
                // The same arrival hook, harvester arm of
                // `UnitClass::Enter_Idle_Mode @ 0x00738970`.
                harvester_enter_idle_mode_evaluation(sim, id, rules)
            } else {
                // The arrival branch. `FootClass::Mission_Move` calls the class
                // arrival hook and returns one frame; the hook is the ONLY
                // thing that takes an object back off Move. Without it a
                // finished move order leaves the unit on Move for the rest of
                // the match, re-dispatched every single frame instead of
                // settling onto Guard's cadence — and never eligible for the
                // Guard-only arm of the passive-acquire gate.
                // The arrival hook is the class Enter_Idle_Mode, whose Foot
                // and Techno bases let a Temporal link go first (0x004D82D9
                // -> 0x00709A54).
                sim.temporal_release_if_warping(id);
                // Unit EnterIdle 0x738AB4 retains pending Deploy intent;
                // arrival must not invent Guard after a replacement Move.
                if sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|e| e.mcv_deploy_pending)
                {
                    MissionHandlerEvaluation::cadence(1)
                } else {
                    move_arrival_evaluation(rules, input)
                }
            }
        }
        // A repair-depot waiter: `FootClass::Mission_Enter @ 0x004D9290` run
        // from the unit's own dispatch slot so its epilogue `RandomRanged(0,2)`
        // lands in object-AI order (the building-side servicing stays in
        // `tick_building_docks`).
        (EntityCategory::Unit, Some(MissionType::Enter)) if input.depot_dock_state => {
            MissionHandlerEvaluation::cadence(
                crate::sim::docking::building_dock::mission_enter_dispatch(sim, rules, id),
            )
        }
        // `UnitClass::Mission_Attack @ 0x007447A0` is a tail jump to
        // `FootClass::Mission_Attack`, so vehicles belong on this path.
        //
        // RESIDUAL (GSI-07.06) — **Infantry do NOT**, and that is not modelled.
        // `InfantryClass`'s Attack slot `+0x210` is `0x0051F3E0`, a real
        // override with three branches ahead of the Foot body:
        // - Human-owned infantry whose DoType `[this+0x6C4]` is in
        //   `{0x1B, 0x1C, 0x1D, 0x1E}` call vtable `+0x428` (`0x0051F330`, an
        //   in-place re-acquire: keep firing if the target is still legal, else
        //   rescan in range, else go idle — it never walks), then return the
        //   PLAIN `ftol(Rate) + RandomRanged(0,2)`, skipping the whole Foot
        //   body. So no half-cadence gate and no idle-mode exit. The same field
        //   and value set gate `InfantryClass::Mission_Move` (`0x0051F660`),
        //   recorded on the Move arm above.
        //
        //   The DoType identity is PROVED, not assumed. The sequence-name
        //   pointer table is at `0x008255C8`, 42 entries, bounded by
        //   `InfantryTypeClass::ReadSequenceData @ 0x00523D00`'s
        //   `while (ptr < 0x825670)`. Indices `0x1B`..`0x1E` are `Deploy`,
        //   `Deployed`, `DeployedFire`, `DeployedIdle`. Corroborated by
        //   `InfantryClass::Do_Action @ 0x0051D6F0`, whose land-to-water remap
        //   pairs (Walk/Crawl->Swim, Ready/Prone->Tread, Die1/Die2->WetDie1/2,
        //   FireUp/FireProne->WetAttack) all decode exactly under this table,
        //   and which plays `DeploySound=` on `0x1B` and `UndeploySound=` on
        //   `0x1F`.
        //
        //   Trigger: an EXPLICIT player attack order given to an
        //   already-deployed infantryman. Not merely "a deployed unit firing" —
        //   auto-acquire while deployed does not reach here, because
        //   `Mission_Guard` (`0x0051F620`) and `Mission_AreaGuard`
        //   (`0x0051F640`) both route through `FUN_00521320`, which handles the
        //   deployed state itself and calls `+0x428` directly without changing
        //   mission.
        //   Player effect: VERA runs the half-cadence test and the idle exit
        //   that retail skips, so the dispatch cadence and the scenario-RNG
        //   draw rate diverge for that engagement.
        //   Frequency: several times per match in any game with Allied GIs or
        //   Soviet Desolators — routine micro, not continuous. An earlier draft
        //   of this note said continuous; that was wrong, and the correction is
        //   the auto-acquire route above.
        //
        //   Stock infantry that can enter the set (`Deployer=` on
        //   `InfantryTypeClass+0xEC8`): `E1`, `GGI`, `DESO`, `YURI`, `YURIPR`.
        //   `CAOS` also carries `Deployer=` but is a voxel `UnitClass` and
        //   never reaches these paths. `0x1E` is unreachable for every stock
        //   type — `GISequence`/`GuardianGISequence` author
        //   `DeployedIdle=0,0,0` and `Do_Action` rejects a zero frame count —
        //   and `0x1D` is reachable only for `E1`/`GGI`/`DESO`.
        //
        //   **The frame trap has THREE meanings at this offset, not two.**
        //   `InfantryClass` instance `+0x6C4` is this DoType (init `-1`);
        //   `UnitClass` instance `+0x6C4` is a `UnitTypeClass*`; and
        //   `TechnoTypeClass+0x6C4` is `UndeployDelay=` (stock `YURI=150`,
        //   `YURIPR=75`). `Mission_Move` itself reads the TYPE one at
        //   `0x0051F6A4` — loading `[ESI+0x6C0]` first — to decide whether an
        //   AI-owned deployed unit undeploys immediately, so a port that reads
        //   the instance field there gets Yuri Clone undeploy wrong.
        // - **This arm is FIRST in the override and is NOT AI-gated.** A
        //   demolition infantryman — `InfantryType->C4` (`+0xEC2`, key `"C4"`
        //   at `0x00825978`, store `0x00524559`) or `HasWeaponAbility(0xE)` —
        //   holding a BuildingClass target takes
        //   `Set_Destination(target, 1); Queue_Mission(0x11 Sabotage, 0);
        //   return 1` with **no RNG draw** (`decompile_function 0x0051F3E0`,
        //   `0x0051F400`-`0x0051F44A`). The two building-type gates are now
        //   named: `+0x1577` is **`CanC4=`** (key `"CanC4"` at `0x0081ADFC`,
        //   `BuildingTypeClass::ReadINI` store `0x0046005D`, constructor
        //   default **1** at `0x0045E063`) and `+0x1701` is
        //   **`InvisibleInGame=`** (key at `0x0081A8CC`, store `0x00460E01`) —
        //   both already parsed here as `can_c4` and `invisible_in_game`, and
        //   both already used by the player Sabotage order in
        //   `world_commands.rs`. What is still missing is the *handler* arm:
        //   VERA drives Sabotage from that order path's `c4_plant` goal state
        //   and its own movement issue, and the Sabotage selector has no
        //   dispatch arm, so queueing it from here would park the object on a
        //   selector whose timer nothing re-arms. Trigger: a force-fire or
        //   retarget onto a building by Tanya, a Navy SEAL, a Crazy Ivan or a
        //   Psi-Corps Trooper. Player effect: VERA shoots the building where
        //   retail walks in and plants. Frequency: low-to-moderate — the
        //   ordinary right-click resolver issues the enter action directly.
        // - An AI-owned `Infiltrate=`(`+0xEBE`) / `Occupier=`(`+0xEB4`) /
        //   `Assaulter=`(`+0xEB5`) infantryman converts to
        //   `Assign_Mission(Capture)` and returns 1. Frequency: zero today,
        //   because this project has no AI opponent.
        //
        // RESIDUAL (GSI-07.06) — two further Foot-body steps are absent:
        // - Step 1, the `HoverAttack` re-anchor. When `TechnoType+0x390`
        //   (`HoverAttack`, NOT `DefaultToGuardArea` — the research corpus has
        //   those crossed) is set and `GetHeight() == 0`, native finds a nearby
        //   passable cell and takes it as a destination every dispatch. Live
        //   FootClass carriers in stock: `JUMPJET` (Rocketeer) and
        //   `SCHP`/`SCHD` (Siege Chopper). Trigger: a landed Rocketeer or a
        //   grounded Siege Chopper on Attack. Player effect: retail nudges them
        //   off the spot; VERA's stay put. Frequency: routine in Allied and
        //   Soviet mid-game. The key IS parsed, for locomotor selection only.
        // - Step 2, the `[this+0x68E]` re-acquire, which runs
        //   `Greatest_Threat` once and clears the flag. Frequency: ZERO today —
        //   its only producers are the tank-bunker adjacency scans in
        //   `Mission_Guard @ 0x004D51C5` and `Mission_AreaGuard @ 0x004D7018`,
        //   both already recorded as residuals on their own arms. Downstream
        //   risk is the reason it is written down: the consumer must land in
        //   the same slice as the bunker scan, or a bunker-acquired unit keeps
        //   the bunker as its target instead of re-picking one dispatch later.
        // Deployed infantry never reach the Foot body — `InfantryClass`'s
        // Attack slot `+0x210` is a real override at `0x0051F3E0` whose
        // deployed arm runs the in-place re-acquire and returns the PLAIN
        // Rate epilogue, with no half-cadence gate.
        (EntityCategory::Infantry, Some(MissionType::Attack))
            if input.infantry_deployed_do_type =>
        {
            // Order is load-bearing: `0x0051F4E2` calls `[vtable+0x428]`
            // FIRST and only then computes `ftol(Rate) + RandomRanged(0, 2)`.
            // The re-acquire can install or clear a target, so running it
            // after the draw would both reorder the state writes and move the
            // scenario-stream position for anything the scan itself consumes.
            let queue = infantry_deployed_attack_reacquire(sim, id, rules, input, ctx);
            let delay = jittered_mission_cadence(sim, rules, MissionType::Attack);
            MissionHandlerEvaluation {
                delay,
                clear_stale_attack_target: input.has_attack_target
                    && attack_target_is_stale(sim, id),
                clear_attack_target: false,
                queue,
            }
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Attack)) => {
            let cadence = jittered_mission_cadence(sim, rules, MissionType::Attack);
            let delay = if foot_dispatch_in_cadence_band(sim, rules, id) {
                cadence / 2
            } else {
                cadence
            };
            // The handler's ONLY exit. With no shoot-at target installed it
            // runs the idle-mode selector, which picks a replacement mission
            // and queues it; with one installed it takes the firing step
            // instead, which the combat phase already owns.
            //
            // This is NOT an "is my target still reachable" or "is my target
            // dead" test — the original has neither. A blocker that simply
            // walks away and stays alive never releases its attacker, by any
            // route: the attacker keeps Attack, keeps closing, and only stops
            // when something outside the handler nulls its target. What does
            // null it is the two detach sweeps (target destroyed, target
            // detached alive) and a fresh player order.
            //
            // Without this arm an object parked on Attack never returns to a
            // mission the passive-acquire gate admits, so it stops scanning for
            // targets permanently. Both branches draw the cadence jitter, so
            // this adds no RNG draw; the half-cadence band needs a live target
            // and is already skipped here.
            let idle_queue = if input.has_attack_target {
                None
            } else {
                sim.temporal_release_if_warping(id);
                foot_enter_idle_mode_queue(rules, input)
            };
            MissionHandlerEvaluation {
                delay,
                // Stale entity IDs are an authoritative target-loss input, and
                // clearing an invalid handle is not the native selector: the
                // clear lands this dispatch, the idle exit reads the target as
                // it stood at entry and fires on the next one.
                clear_stale_attack_target: input.has_attack_target
                    && attack_target_is_stale(sim, id),
                clear_attack_target: false,
                queue: idle_queue,
            }
        }
        // `FUN_00521320`, the deploy shim both infantry Guard-family slots run
        // BEFORE the Foot body. `InfantryClass::Mission_Guard @ 0x0051F620`
        // (slot `+0x21C`, shared by Guard(5) and Sticky(6)) and
        // `InfantryClass::Mission_AreaGuard @ 0x0051F640` (slot `+0x220`) are
        // both nine instructions: call it, return its value unless it is `-1`,
        // and only `-1` falls through.
        //
        // `decompile_function 0x00521320`, deployed branch (DoType in
        // `{0x1B..0x1E}`), in order:
        // 1. `if (-1 < Type->UndeployDelay) { Do_Action(Undeploy 0x1F); return
        //    the sequence duration }` — the Yuri arm, see the residual below;
        // 2. `if (Type->DeployFire)`:
        //    - `!Type->ImmuneToRadiation` → `[vtable+0x428]()` (the in-place
        //      re-acquire) then `ftol(Rate * 900) + RandomRanged(0, 2)`;
        //    - otherwise the Desolator radiation arm, see the residual below;
        // 3. `return -1` → the Foot body.
        //
        // So a deployed GI or Guardian GI holding ground **never reaches
        // `FootClass::Mission_AreaGuard`**: it re-acquires where it stands and
        // re-dispatches on `Rate + (0, 2)`, not on the Foot body's scan plus
        // `Rate + (1, 5)`. That is both a cadence and an RNG-stream difference,
        // and it is why this arm sits ahead of the three below.
        //
        // RESIDUAL — the two arms this predicate excludes:
        // - **`UndeployDelay >= 0`** (`YURI` 150, `YURIPR` 75): retail makes a
        //   deployed Yuri undeploy on his own next Guard dispatch and returns
        //   an animation duration read through `Type[+0xE3C] -> [+0x460]`.
        //   VERA parses no sequence-duration table, so the return value cannot
        //   be produced. Trigger: a deployed Yuri or Yuri Prime on Guard,
        //   Sticky or Area Guard. Player effect: retail's stands back up,
        //   VERA's stays deployed. Frequency: every use of Yuri's mind
        //   control in a Yuri-faction match. Downstream risk: none for RNG —
        //   the arm draws nothing.
        // - **`ImmuneToRadiation`** (`DESO`): the Desolator arm re-targets its
        //   own cell, checks the rad level under it against
        //   `GetWeapon(1)->Warhead[+0x158] / 3`, and returns either an
        //   animation duration (`Type[+0xE3C] -> [+0x418]`, after
        //   `Do_Action(DeployedFire 0x1D)`) or `Rate + RandomRanged(10, 20)`.
        //   The `(10, 20)` draw is a scenario-RNG consumption VERA never
        //   makes, but it sits behind a rad-site query and a warhead field
        //   (`+0x158`) that are not modelled, and only one of the two exits
        //   takes it — committing the draw alone would be wrong on the other.
        //   Trigger: a deployed Desolator. Player effect: cadence and stream.
        //   Frequency: continuous wherever a Soviet player deploys one.
        (
            EntityCategory::Infantry,
            Some(MissionType::Guard | MissionType::Sticky | MissionType::AreaGuard),
        ) if input.infantry_deployed_do_type && input.infantry_deploy_fire_stance => {
            // Order is native's: `[vtable+0x428]` runs FIRST, then the shim's
            // tail computes `ftol(Rate * 900)` and draws `RandomRanged(0, 2)`.
            let queue = infantry_deployed_attack_reacquire(sim, id, rules, input, ctx);
            // `MissionClass::GetMissionTimerEntry @ 0x005B3A00` indexes the
            // control table on `[this+0xAC]`, the object's OWN committed
            // selector — so the shim re-arms a Guard man at `[Guard] Rate` and
            // an Area Guard man at `[Area Guard] Rate`.
            let delay =
                jittered_mission_cadence(sim, rules, input.mission.unwrap_or(MissionType::Guard));
            MissionHandlerEvaluation {
                delay,
                clear_stale_attack_target: input.has_attack_target
                    && attack_target_is_stale(sim, id),
                clear_attack_target: false,
                queue,
            }
        }
        // The transport branch of `UnitClass::Mission_Unload @ 0x0073D630`
        // (`Type+0x5E0 Passengers > 0`, gate `0x0073D6EC`). The handler owns
        // its side effects and every return value — the `return 10` / `return
        // 1` early exits and the `[Unload] Rate + RandomRanged(0, 2)`
        // epilogue — so it is committed as a plain cadence here. The refinery
        // (`Harvester=`), `DeploysInto=` and `IsSimpleDeployer=` branches of
        // the same slot are other lanes and fall through to the default arm.
        (EntityCategory::Unit, Some(MissionType::Unload))
            if sim.substrate.entities.get(id).is_some_and(|entity| {
                crate::sim::transport_unload::is_vehicle_transport_type(sim, entity, rules)
            }) =>
        {
            MissionHandlerEvaluation::cadence(crate::sim::transport_unload::unit_mission_unload(
                sim,
                rules,
                ctx.path_grid,
                ctx.overlay_registry,
                id,
            ))
        }
        (EntityCategory::Unit, Some(MissionType::Unload))
            if sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| crate::sim::mcv_deploy::is_mcv(sim, e, rules)) =>
        {
            MissionHandlerEvaluation::cadence(crate::sim::mcv_deploy::mission_unload(
                sim, id, rules,
            ))
        }
        (EntityCategory::Unit, Some(MissionType::Guard)) => {
            // **VERA-internal, gamemd equivalent UNCHECKED — this mapping is
            // wrong and the arm is dead.** The "three byte latches, then
            // `Assign_Mission(5, 0)`, then `return 1`" shape lives at
            // `0x00740A90`, which is vtable `+0x22C` — the **Move** slot, not
            // Guard — reads `[this+0x6E0]`/`+0x6E1`/`+0x6E2`, and queues
            // **Guard**, not Harvest or Unload. `UnitClass`'s real Guard
            // override `0x00740810` gates its `Queue_Mission(10)`/`return 1` on
            // `UnitTypeClass+0xE0E`/`+0xE0F` plus house and refinery checks, and
            // its `Queue_Mission(0x10)` path returns `ftol(Rate) + Rand(0, 2)`
            // rather than 1.
            //
            // Trigger: none today — both latch bytes have only `#[cfg(test)]`
            // writers (`sim::mission::leaf`), so production never reaches
            // either arm. Player effect: none. Frequency: zero. Downstream
            // risk: the wrong native mapping would be carried straight into any
            // future deploy work; the shape belongs on the Move arm.
            if harvester_guard_override_requeues_harvest(sim, id, rules) {
                // `Queue_Mission(10, 0); return 1` at `0x0074092C` /
                // `0x00740960` — no RNG draw, the Foot body is not reached.
                MissionHandlerEvaluation::queue(1, MissionType::Harvest)
            } else if input.unit_deploy_begin_active {
                MissionHandlerEvaluation::queue(1, MissionType::Harvest)
            } else if input.unit_deploy_reverse_active {
                MissionHandlerEvaluation::queue(1, MissionType::Unload)
            } else {
                evaluate_foot_guard_cadence(
                    sim,
                    rules,
                    id,
                    MissionType::Guard,
                    input.bunker_delegate,
                )
            }
        }
        (EntityCategory::Infantry, Some(MissionType::Guard)) => {
            evaluate_foot_guard_cadence(sim, rules, id, MissionType::Guard, input.bunker_delegate)
        }
        // Sticky dispatches through the SAME slot as Guard — one handler, two
        // selectors — so it runs the Guard body. The cadence still comes from
        // the object's own mission slot (the timer lookup indexes on the
        // committed mission id, not on the handler's identity), and `[Sticky]
        // Rate=.016` is 14 frames against Guard's 26. Stock skirmish maps park
        // neutral civilian traffic on this.
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Sticky)) => {
            evaluate_foot_guard_cadence(sim, rules, id, MissionType::Sticky, input.bunker_delegate)
        }
        // Area Guard is NOT a Guard alias — it has its own slot and its own
        // handler, and that handler owns its acquisition. The common Techno AI
        // body's passive-acquire block admits missions {Move, Harvest, Guard}
        // and nothing else, so an Area Guard object is deliberately never
        // scanned there; this arm is its single acquisition route.
        //
        // GSI-07.16 — the infantry leaf override
        // `InfantryClass::Mission_AreaGuard @ 0x0051F640` is now taken by the
        // deploy-shim arm above; only the arms it excludes remain recorded
        // there.
        //
        // RESIDUAL (GSI-07.16) — `UnitClass::Mission_AreaGuard @ 0x00744100` is
        // the slave-miner recall and returns `RandomRanged(0, 2)` where the Foot
        // body returns `RandomRanged(1, 5)` — an RNG fork, not just a different
        // delay. Frequency: zero today (no slave miners), continuous once they
        // land. The Foot body's own slave-recall arm is absent for the same
        // reason.
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::AreaGuard)) => {
            evaluate_foot_area_guard(sim, id, rules, ctx)
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Hunt)) => {
            evaluate_foot_hunt(sim, id, rules, ctx)
        }
        // SKIP/PROVE (GSI-07.09) — Mission 4 Retreat is a dead slot for foot
        // objects, and pass 1's reason was wrong. `FootClass::Mission_Retreat @
        // 0x004DA2C0` (slot `+0x230`, proven from the dispatch jump table at
        // `0x005B34E8` entry 4) is a two-state destination oscillator returning
        // `ftol(.1 * 900) + RandomRanged(0, 2)` unconditionally — but a bounded
        // assigner sweep (every `Queue_Mission` two-push site, all 29
        // `Assign_Mission` sites, and every direct `mov [this+0xAC], 4`) finds
        // mission 4 queued at exactly four addresses, ALL of them
        // `AircraftClass` (ParaDropApproach, Open, Rescue, Receive_Radio) — and
        // aircraft dispatch through `0x00415A50`, not this body. So the
        // frequency is zero because no foot assigner exists, not because this
        // project lacks an AI. Keep the enum slot; the only cost of the missing
        // arm is that a Retreat-committed foot object would leave its timer
        // untouched, which nothing can produce today.
        // Everything else: the object still reaches a handler and still re-arms
        // its timer. Where that handler is the un-overridden base one, the
        // return value is a verified constant and no RNG is drawn; where the
        // leaf class overrides the slot with a real handler VERA has not
        // absorbed yet, leave the timer alone rather than install a value the
        // original never writes.
        (category, mission) => match base_mission_handler_delay(category, mission) {
            Some(delay) => MissionHandlerEvaluation::cadence(delay),
            None => return,
        },
    };

    if evaluation.clear_stale_attack_target || evaluation.clear_attack_target {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            if evaluation.clear_attack_target {
                // Unit EnterIdle738AF5/738C75 dispatches the real target setter.
                // Keep the same burst owner as the locomotor arrival receiver.
                crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
            } else {
                // A missing handle is expiry cleanup, not Assign_Target(NULL).
                entity.attack_target = None;
            }
        }
    }
    if let Some(queued_mission) = evaluation.queue {
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(queued_mission),
            0,
            now,
            &EntityReadyInputProvider,
        );
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .mission
            .write_dispatch_epilogue(now as i32, evaluation.delay);
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MissionHandlerInput {
    pub(super) category: EntityCategory,
    /// The committed selector, or `None` for the idle sentinel. The native
    /// dispatcher's bounds test on the mission id is UNSIGNED, so the sentinel
    /// and every out-of-range id take the switch's default arm rather than
    /// being skipped.
    pub(super) mission: Option<MissionType>,
    /// The object carries the miner component (`Harvester=`/`Weeder=` type
    /// flags `UnitTypeClass+0xE0E`/`+0xE0F`): its Move arrival takes the
    /// harvester arm of `UnitClass::Enter_Idle_Mode`.
    pub(super) harvester_miner: bool,
    /// The object holds a repair-depot `DockState`: its Enter dispatch is
    /// `building_dock::mission_enter_dispatch`.
    pub(super) depot_dock_state: bool,
    pub(super) timer_due: bool,
    pub(super) moving_or_queued: bool,
    pub(super) bunker_delegate: bool,
    pub(super) has_attack_target: bool,
    /// The destination slot on its own. The idle-mode selector reads this one
    /// field, not the broader in-motion test [`Self::moving_or_queued`] uses.
    pub(super) has_destination: bool,
    /// Current when present, otherwise queued — the selector the idle-mode
    /// early returns and the control-entry lookups read.
    pub(super) effective_mission: Option<MissionType>,
    pub(super) unit_deploy_begin_active: bool,
    pub(super) unit_deploy_reverse_active: bool,
    /// This is an infantryman whose DoType sits in native's deployed set, so
    /// its Attack slot takes `InfantryClass::Mission_Attack`'s own override
    /// instead of the Foot body.
    pub(super) infantry_deployed_do_type: bool,
    /// The type half of the deploy shim `FUN_00521320`'s live arm:
    /// `DeployFire=` set (`TechnoTypeClass+0x6AC`, key `"DeployFire"` at
    /// `0x00843AA0`, store `0x007147FC`), `ImmuneToRadiation=` clear
    /// (`+0xD37`, key `0x00843854`, store `0x00714D67`) and `UndeployDelay=`
    /// negative (`+0x6C4`, key `0x008438F4`, store `0x00714BBA`, ctor default
    /// `-1` from `0x00710CED`/`0x00711187`).
    ///
    /// Stock `DeployFire=yes`: `E1`, `GGI`, `DESO`, `YURI`, `YURIPR`, `CAOS`.
    /// `DESO` is radiation-immune and `YURI`/`YURIPR` carry an
    /// `UndeployDelay`, so this predicate selects the GI and the Guardian GI —
    /// and `CAOS`, which is a voxel `UnitClass` and never reaches an infantry
    /// arm.
    pub(super) infantry_deploy_fire_stance: bool,
}

/// The handler result is evaluated before the one common MissionClass timer
/// write, which prevents branch-local epilogues from double-rearming it.
#[derive(Debug, Clone, Copy)]
pub(super) struct MissionHandlerEvaluation {
    delay: i32,
    clear_stale_attack_target: bool,
    /// The handler itself dropped the shoot-at target (the arrival hook's
    /// `Assign_Target(NULL)`), as opposed to the stale-handle cleanup above.
    clear_attack_target: bool,
    queue: Option<MissionType>,
}

impl MissionHandlerEvaluation {
    const fn cadence(delay: i32) -> Self {
        Self {
            delay,
            clear_stale_attack_target: false,
            clear_attack_target: false,
            queue: None,
        }
    }

    const fn queue(delay: i32, mission: MissionType) -> Self {
        Self {
            delay,
            clear_stale_attack_target: false,
            clear_attack_target: false,
            queue: Some(mission),
        }
    }
}

/// The Move handler's arrival hook, reduced to the parts VERA can commit.
///
/// `UnitClass`'s override, ordinary-vehicle arm: drop the shoot-at target,
/// clear the destination, then queue **Guard**. `InfantryClass`'s override: a
/// live target queues **Attack** and the target is *kept*; otherwise Guard —
/// and the whole infantry selector is skipped when the *current* mission's
/// control entry carries `Zombie=` or `Paralyzed=` (both absent from `[Move]`,
/// so on this path they never fire; they are read anyway because the gate is on
/// the object's own mission slot and a later caller may arrive on another one).
///
/// The destination clear is a no-op here by construction — this branch is only
/// taken when nothing is moving or queued, which is exactly the state
/// `Set_Destination(NULL, true)` produces.
///
/// Deliberately NOT represented, each recorded rather than guessed:
/// - the vehicle Unload / Harvest arms and the Area-Guard promotion, which key
///   off house-threat, deploy and harvester type fields VERA does not model;
/// - the infantry Guard-vs-Area-Guard choice, same reason — the ordinary arm
///   for a player-controlled infantryman with no veteran self-heal ability is
///   Guard, which is what this returns;
/// - the vehicle suppression byte that skips the queue entirely (its writer is
///   UNKNOWN, so modelling it would be inventing a gate);
/// - the base hook's NavQueue pop and locomotor piggyback unwind. These are
///   **not** early returns that suppress the selector — that reading was wrong.
///   `FootClass::Enter_Idle_Mode` @ `0x004D82B0` pops `[this+0x598]` /
///   `[this+0x58C]`, calls `Assign_Destination(next, 0)` and returns 1, but both
///   leaves discard that return for control flow (`0x00738970` assigns it to a
///   local and then unconditionally calls `+0x4AC`; `0x0051CBA0` the same).
///   Waypointed movement continues because the pop **installs a destination**,
///   so the leaf then reads `[this+0x5A4] != 0` and picks Move(2) on its own.
///   Both are inert here — `nav_queue` has no production writer and the
///   piggyback unwind runs in the movement phase — but a future NavQueue writer
///   must pop *before* the selector reads the destination, not restore an early
///   return that does not exist.
pub(super) fn move_arrival_evaluation(
    rules: &RuleSet,
    input: MissionHandlerInput,
) -> MissionHandlerEvaluation {
    let infantry = input.category == EntityCategory::Infantry;
    if infantry {
        let frozen = rules
            .mission_control
            .entry(MissionType::Move)
            .is_some_and(|entry| entry.zombie || entry.paralyzed);
        if frozen {
            return MissionHandlerEvaluation::cadence(1);
        }
        let next = if input.has_attack_target {
            MissionType::Attack
        } else {
            MissionType::Guard
        };
        return MissionHandlerEvaluation::queue(1, next);
    }
    MissionHandlerEvaluation {
        delay: 1,
        clear_stale_attack_target: false,
        clear_attack_target: true,
        queue: Some(MissionType::Guard),
    }
}

/// The harvester arm of `UnitClass::Enter_Idle_Mode @ 0x00738970` (vtable
/// `+0x484`), reached from the Move handler's arrival branch.
///
/// Callers a player Move order on a war/chrono miner reaches (all verified by
/// `search_instructions CALL [+0x484]`, 85 sites): `FootClass::Mission_Move @
/// 0x004D4200` at 0x004D4242 — `NavCom == 0 && !Is_Moving && Queued == -1 ⇒
/// Enter_Idle_Mode(0,1); return 1` — and the locomotor IDLE event
/// `DriveLocomotionClass::Process @ 0x004B0763` (destination reached with an
/// empty NavQueue: `Stop_Moving`, then `Enter_Idle_Mode(0,1)`). Both land in
/// the same body; the Unit slot `0x00740A90` only precedes the Foot body
/// with its deploy latches. The selector itself is
/// [`harvester_enter_idle_mode_selector`], shared with the other stock
/// callers of the same slot (owner change, depot exit).
///
/// So a human's miner moved onto bare ground parks on Guard (the harvester
/// Guard override's chrono arms or a player order put it back); moved onto
/// ore it resumes Harvest from state 0. The whole arm draws no RNG; the
/// `Mission_Move` caller returns 1. VERA commits through the queue like
/// [`move_arrival_evaluation`] (promoted by the next Ready-to-Commence step,
/// which zeroes the Harvest cursor exactly as native's `+0xBC = 0`).
fn harvester_enter_idle_mode_evaluation(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
) -> MissionHandlerEvaluation {
    let Some(selector) = harvester_enter_idle_mode_selector(sim, id, rules, false) else {
        return MissionHandlerEvaluation::cadence(1);
    };
    MissionHandlerEvaluation {
        delay: 1,
        clear_stale_attack_target: false,
        clear_attack_target: true,
        queue: Some(selector),
    }
}

/// The no-destination harvester selector of `UnitClass::Enter_Idle_Mode @
/// 0x00738970`, for a unit whose `Harvester=`/`Weeder=` flag
/// (`UnitType+0xE0E`/`+0xE0F`) is set. Returns the mission the body commits
/// through `Queue_Mission(selector, 0)` (`+0x1E8` = 0x005B35E0), or `None`
/// on one of its early returns (nothing assigned).
///
/// Body, decompiled 2026-09-06:
/// - `RadioClass::In_Radio_Contact` ⇒ return (nothing assigned);
/// - current (`+0xAC`) or queued (`+0xB4`) == Harvest(10) ⇒ return;
/// - selector = Harvest; when the FIRST explicit argument is 0 AND the OWNER passes
///   `HouseClass::IsControlledByHuman @ 0x0050B730`: the cell under the unit
///   (`MapClass::Get_CellClass_At_Coord`) has `LandType` (`CellClass+0xEC`)
///   ≠ 5 (Tiberium; 0xB Weeds for a Weeder) ⇒ selector = Guard(5). An AI
///   house always takes Harvest; so does every caller passing first explicit argument 1
///   (`TechnoClass::Unlimbo @ 0x006F6E2A` calls `+0x484(1, 1)`, which is why
///   a freshly built miner always leaves the factory on Harvest);
/// - `Assign_Target(0)` (`+0x3C8`), `Assign_Destination(0, 1)` (`+0x480`) —
///   the callers own those writes;
/// - tail gate: current ∉ {Patrol 0x19, AreaGuard 0xB, Unload 0x10, Eaten 9}
///   ⇒ `+0x1E8(selector, 0)`.
///
/// Stock callers of the slot on a miner and where VERA runs them: the Move
/// arrival (`FootClass::Mission_Move` 0x004D4242, [`harvester_enter_idle_mode_evaluation`]);
/// `TechnoClass::ChangeOwner @ 0x00701849` after the house swap
/// (`Simulation::change_owner_harvester_idle_arm`); `FootClass::Mission_Enter
/// @ 0x004D92E2` after a depot's "already repaired" BREAK
/// (`building_dock::mission_enter_dispatch`). The destination-installed
/// branch (`NavCom != 0 ⇒ Move`) is not modelled here: every VERA caller
/// reaches the selector with the destination already cleared.
///
/// `skip_human_land_check` is the FIRST explicit argument != 0.
/// Original738970 loads caller arg1 into BL; miner gate738C0A tests BL.
/// Decompiler parameter numbering included implicit this. Terminal +484(0,1)
/// therefore retains the human land check.
pub(crate) fn harvester_enter_idle_mode_selector(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
    skip_human_land_check: bool,
) -> Option<MissionType> {
    let entity = sim.substrate.entities.get(id)?;
    if !entity.radio_contacts.is_empty() {
        return None;
    }
    let current = entity.mission.current().known();
    if current == Some(MissionType::Harvest)
        || entity.mission.queued() == MissionId::from_known(MissionType::Harvest)
    {
        return None;
    }
    if matches!(
        current,
        Some(MissionType::Patrol)
            | Some(MissionType::AreaGuard)
            | Some(MissionType::Unload)
            | Some(MissionType::Eaten)
    ) {
        return None;
    }
    let human = !skip_human_land_check
        && sim
            .houses
            .get(&entity.owner())
            .is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero));
    let weeder = sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|obj| !obj.harvester && obj.weeder);
    let wanted_land = if weeder {
        crate::rules::terrain_rules::LandType::Weeds
    } else {
        crate::rules::terrain_rules::LandType::Tiberium
    };
    let land_matches = cell_land_type_is(sim, entity.position.rx, entity.position.ry, wanted_land);
    Some(if human && !land_matches {
        MissionType::Guard
    } else {
        MissionType::Harvest
    })
}

/// `CellClass+0xEC` (`LandType`) of one cell: the resolved terrain's land
/// type, which the overlay recompute keeps current when ore is placed or
/// removed. Without resolved terrain no cell has a land type.
fn cell_land_type_is(
    sim: &Simulation,
    rx: u16,
    ry: u16,
    wanted: crate::rules::terrain_rules::LandType,
) -> bool {
    sim.resolved_terrain.as_ref().is_some_and(|terrain| {
        terrain
            .cell(rx, ry)
            .is_some_and(|cell| cell.land_type == wanted.as_index())
    })
}

/// The idle-mode selector reached from the Attack handler's no-target exit.
///
/// `Enter_Idle_Mode` is the shared "you have nothing to do; commit the mission
/// that says so" virtual, and both leaf overrides on this path — the Infantry
/// one and the Unit one — begin by running the base arrival hook and then pick
/// a replacement selector. VERA already models the *arrival* entry into it as
/// [`move_arrival_evaluation`]; this is the same virtual entered from the other
/// direction, so only the arms that differ are re-derived here.
///
/// Reached only with no shoot-at target, so the two leaves agree on the whole
/// remaining selection and it collapses to one function:
/// - **a destination is installed** → `Move`. Both leaves take it; the Infantry
///   one substitutes Capture or Sabotage when that is the effective selector,
///   which cannot happen from the Attack handler.
/// - **no destination** → `Guard`, after two early returns that suppress the
///   assignment entirely.
///
/// The Unit leaf additionally nulls its (already null) target and destination
/// on the no-destination arm, and the Infantry leaf's own already-null
/// destination write is likewise inert.
///
/// The early returns, each read from the leaf bodies rather than assumed:
/// - the effective selector is already `Guard` or `Area Guard` — the object is
///   idle, and re-assigning would restart its mission timer for free;
/// - the effective selector's control entry carries `Zombie=` or `Paralyzed=`.
///   `[Attack]` carries neither in stock rules, so this cannot fire from the
///   Attack handler; it is read anyway because the gate is on the object's own
///   selector and the same virtual is entered from other missions.
/// - the *committed* selector is `Patrol` or `Area Guard` (the Unit leaf also
///   excludes `Unload` and `Eaten`) — the tail gate that skips the assign.
///
/// Deliberately NOT represented, recorded rather than guessed:
/// - the head gate both leaves share, an early return on a Foot field whose
///   writer and meaning are UNKNOWN. Modelling it would be inventing a gate;
///   leaving it out can only make the selector run where the original skipped
///   it, and the skip case is unidentified.
/// - the `Area Guard` arm of the no-destination branch. Its inputs are now
///   identified: the GUARD_AREA ability (`HasAbility(0x10)` at `0x0051CD4B`),
///   Type+0xD39 `DefaultToGuardArea` (`0x0051CD5A`, Unit `0x00738B96`), team
///   membership, and for AI houses CurrentIQ against Rules+0x1440 plus the
///   slave links. Porting it is its own mechanism (recorded in
///   `combat/parasite.rs`); until then this commits `Guard`, consistent with
///   [`move_arrival_evaluation`].
/// - the AI-only sub-arms, which need a live team and a house-threat field.
pub(super) fn foot_enter_idle_mode_queue(
    rules: &RuleSet,
    input: MissionHandlerInput,
) -> Option<MissionType> {
    foot_enter_idle_mode_selection(
        rules,
        input.category,
        input.mission,
        input.has_destination,
        input.effective_mission,
    )
}

/// The Foot Enter_Idle_Mode(0,1) selection on exactly the fields it reads,
/// applied as a deferred Queue_Mission. Used by release paths outside the
/// mission dispatcher (a parasite owner leaving its victim, `0x0062A7A3`).
pub(crate) fn queue_foot_enter_idle_mode(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    // `FootClass::Enter_Idle_Mode @ 0x004D82D9` -> `TechnoClass::
    // Enter_Idle_Mode @ 0x00709A54`: a held Temporal target goes first.
    sim.temporal_release_if_warping(id);
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let selection = foot_enter_idle_mode_selection(
        rules,
        entity.category,
        entity.mission.current().known(),
        entity.navigation.nav_com.is_some(),
        entity.mission.effective().known(),
    );
    if let Some(mission) = selection
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        crate::sim::mission::authority::queue_entity_mission_deferred(
            entity,
            MissionId::from_known(mission),
        );
    }
}

fn foot_enter_idle_mode_selection(
    rules: &RuleSet,
    category: EntityCategory,
    mission: Option<MissionType>,
    has_destination: bool,
    effective_mission: Option<MissionType>,
) -> Option<MissionType> {
    // The tail gate, evaluated on the committed selector.
    let committed_blocks_assign = matches!(
        mission,
        Some(MissionType::Patrol) | Some(MissionType::AreaGuard)
    ) || (category == EntityCategory::Unit
        && matches!(mission, Some(MissionType::Unload) | Some(MissionType::Eaten)));
    if committed_blocks_assign {
        return None;
    }

    if has_destination {
        return Some(MissionType::Move);
    }

    if matches!(
        effective_mission,
        Some(MissionType::Guard) | Some(MissionType::AreaGuard)
    ) {
        return None;
    }
    // **VERA-internal, gamemd equivalent UNCHECKED — two ordering/indexing
    // differences, both inert today.** Native is
    // `if (effective != -1) { entry = MissionControl[*(this+0xAC)];
    // if (entry[+7] || entry[+5]) return; }` — the *effective* mission only
    // gates whether to look at all, while the *committed* one at `+0xAC` indexes
    // the table. This looks the entry up on `effective_mission`. And the
    // infantry leaf tests the destination `[this+0x5A4]` **before** the
    // Guard/AreaGuard and Zombie/Paralyzed gates, where this tests them first.
    //
    // Trigger: a caller whose committed and effective missions differ, or one
    // that reaches here with both a destination and a frozen mission. Player
    // effect: none today. The live entries are the Attack handler's no-target
    // exit and the parasite releases (committed == effective == Attack, and
    // `[Attack]` carries neither key) and ChangeOwner's Enter_Idle_Mode
    // (capture, release, engineer and garrison transfers). ChangeOwner queues
    // Guard first, so committed can differ from effective there, but then the
    // Guard return above answers before this gate, and native returns too: the
    // only stock frozen entries are `[Sleep]` (Zombie) and `[Sticky]`
    // (Paralyzed). Without the Guard queue (Selling, a Simple Deployer's
    // Unload) the two missions agree. Frequency: zero. Downstream risk: a new
    // producer would inherit both. (Curiosity for whoever ports it: with a
    // current of -1 and only a queued mission, native indexes
    // `MissionControl[-1]` — an out-of-bounds read one entry below the array.)
    let frozen = effective_mission.is_some_and(|mission| {
        rules
            .mission_control
            .entry(mission)
            .is_some_and(|entry| entry.zombie || entry.paralyzed)
    });
    if frozen {
        return None;
    }

    Some(MissionType::Guard)
}

/// `TechnoClass::Retaliate_And_Scan @ 0x00709820`, vtable `+0x39C` — the
/// scanner the mission handlers call directly, as opposed to the passive block
/// in the common Techno AI body.
///
/// `disassemble_function 0x00709820`. Entry order, all of it load-bearing:
/// 1. `[this+0x4FC] = frame` (`0x0070982E`);
/// 2. **one unconditional `RandomRanged(0, 2)`** on the scenario RNG at
///    `*(0x00A8B230)+0x218` — the `PUSH 0x2` / `PUSH 0x0` pair at `0x0070982C`
///    and `0x0070983D` is set up before the very first branch, so the draw
///    happens on every call whatever the routine goes on to do;
/// 3. the scan timer re-arms to `Rules->GuardAreaTargetingDelay` (`+0xE04`)
///    when the object's committed mission is Area Guard (`CMP EAX,0xB` at
///    `0x0070983A`) and `Rules->NormalTargetingDelay` (`+0xE08`) otherwise,
///    **plus that same draw**;
/// 4. a target the object's own scanner installed may be dropped;
/// 5. `if (Target == 0)` the threat scan runs and its result is committed
///    through `Assign_Target` (`vt+0x3C8`, called at `0x00709960`).
///
/// **The one thing that separates this from [`super::passive_target_scan`]:
/// step 5 does NOT set the passively-acquired byte `[this+0x50C]`.** The whole
/// body only ever *reads* it (`0x007098C3`); the write lives in the passive
/// block's caller inside `TechnoClass::AI_Update`. So a target a mission
/// handler acquires through this routine is an ordinary target — VERA's
/// pursuit pass will close on it, which is exactly how a hunting object gets
/// to what it found. Installing through the shared target setter reproduces
/// that: `RepresentedConcreteMissionEffects::apply_target` clears the byte.
///
/// Returns the native return value: `this->Target != 0` (`SETNZ` at
/// `0x007099C4`).
///
/// RESIDUAL — step 4's drop is not modelled, exactly as
/// [`super::passive_target_scan`] records for the same three action codes
/// (`vt+0x3C0` returning 5, 6 or 8) whose meanings are UNCHECKED. Here it can
/// only matter for an object that walked onto Hunt still holding a target its
/// own scanner picked up on Guard: retail may re-evaluate it, VERA keeps it.
/// Trigger: a passive target surviving a mission change into Hunt — Hunt is not
/// one of the twelve missions that strip one. Frequency: uncommon; the berserk
/// path that produces most Hunt assignments hits idle and engaged units alike.
/// Downstream risk: none beyond target choice.
///
/// RESIDUAL — the post-install debit at `0x00709966`-`0x007099B5` is not
/// modelled. After `Assign_Target` native calls
/// `vt+0x2E4` (`0x00709971`) and `vt+0x3F8` (`0x0070998C`), then — gated on the
/// newly acquired target's `[+0x14] & 1` (`0x0070997B`-`0x00709985`; EDI is the
/// scan result, `MOV EDI,EAX @ 0x0070993C`) and on the selected weapon's
/// `[[weapon+0xA0]+0x2A2] == 0` (`0x0070999C`-`0x007099AA`) — calls
/// `0x006FDB80` and does `SUB dword ptr [EDI+0x70], EAX` (`0x007099B5`) — a
/// **write into another object**. `TechnoClass+0x70` is the same field
/// `Evaluate_Candidate` scores candidates on (`[ESI+0x70] < 1` at `0x006F872A`
/// and `[ESI+0x70]` against `TechnoType->[0xA0] / 2` at `0x006F874B`-
/// `0x006F8755`), so acquisition debits a running claim on the target that
/// later scanners then read. What `0x006FDB80` returns is UNCHECKED — not
/// decompiled — so the amount cannot be reproduced and none of this is
/// implemented. Trigger: every scan through this routine that installs a
/// target, i.e. every Hunt or Area Guard acquisition. Player effect: with the
/// claim never debited, VERA's scanners keep seeing the full value and can
/// over-commit several attackers onto one target instead of spreading. Player-
/// visible only where three or more units acquire freely at once. Frequency:
/// common once several hunting units share a battlefield; nil in a duel.
/// Downstream risk: target distribution only — VERA has no `+0x70` analogue,
/// so nothing else reads or writes it, and neither lifecycle nor determinism
/// is touched.
///
/// RESIDUAL — ordering, inert today. Native runs the scan (`vt+0x3C4` at
/// `0x00709932`) and only then reads `TechnoType+0x6B0` (`DistributedFire`,
/// `0x00709944`) to decide between the spread-fire assignment and
/// `Assign_Target`; the early return below tests the type first and so skips
/// the scan. No stream divergence: `Evaluate_Candidate`'s only draw is the
/// disguise-blink `RandomRanged(0,99)`, which short-circuits on
/// `IsControlledByHuman` for every VERA house. Trigger: a `DistributedFire=`
/// type dispatching Hunt or Area Guard with no target. Frequency: rare on stock
/// data. Downstream risk: it would surface the moment a non-human-controlled
/// house exists. This mirrors the shape [`super::passive_target_scan`] already
/// has rather than introducing a new one.
///
/// `scan_mask` is `Greatest_Threat`'s argument 2, forwarded from whoever
/// dispatched this routine. No callsite derives it from the object it is
/// scanning for: `FootClass::Mission_Hunt` pushes the literal `0` at
/// `0x004D5373`, and mask 0 is not a wider radius — it is a different scan
/// topology (see [`crate::sim::combat::ScanMission::Hunt`]). What the `+0x3C4`
/// overrides then do to that literal before `TechnoClass::Greatest_Threat` sees
/// it is documented there and on
/// [`crate::sim::combat::greatest_threat::greatest_threat`]; VERA forwards the
/// literal unchanged.
fn retaliate_and_scan(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    mission: MissionType,
    scan_mask: crate::sim::combat::ScanMission,
    ctx: super::ObjectAiCtx<'_>,
) -> bool {
    let now = sim.session.binary_frame;
    let base_delay = if mission == MissionType::AreaGuard {
        rules.general.guard_area_targeting_delay
    } else {
        rules.general.normal_targeting_delay
    };
    let jitter = sim
        .scenario_rng
        .next_range_u32_inclusive(0, PASSIVE_SCAN_DELAY_JITTER_MAX);
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.last_target_scan_frame = now;
        entity
            .passive_scan_timer
            .arm(now, base_delay.saturating_add(jitter));
    }

    let Some(has_target) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.attack_target.is_some())
    else {
        return false;
    };
    // Step 5 is `if (Target == 0)`: a live target ends the routine and is
    // reported back as success.
    if has_target {
        return true;
    }
    // `if (GetTechnoType()[0x6B0]) FUN_00709550(this)` at `0x00709944` — a
    // `DistributedFire` type takes the spread-fire assignment instead of the
    // single-target one, which VERA does not implement (recorded on
    // `passive_target_scan`). It installs nothing here, as there.
    let spreads_fire = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.interner.try_resolve(entity.type_ref()))
        .and_then(|name| rules.object(name))
        .is_some_and(|obj| obj.distributed_fire);
    if spreads_fire {
        return false;
    }
    let pick = crate::sim::combat::acquire_best_target_for_entity(
        &sim.substrate.entities,
        &sim.substrate.occupancy,
        rules,
        &sim.interner,
        id,
        Some(&sim.fog),
        sim.resolved_terrain.as_ref(),
        sim.playfield_bounds.is_some(),
        // The mask the CALLER pushes. `FootClass::Mission_Hunt` pushes the
        // literal `0` at `0x004D5373` and this routine forwards whatever it was
        // handed; mask 0 is what makes the scan enumerate the global object list
        // with no distance cutoff instead of walking cell rings.
        scan_mask,
        // `MapClass` itself in native — `MOV ECX,0x87f7e8` at `0x006F8EBA`.
        // Mask 0 asks it for the hunter's own movement-zone component and
        // refuses every candidate outside it.
        sim.zone_grid.as_ref(),
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            overlay_grid: sim.overlay_grid.as_ref(),
            overlay_registry: ctx.overlay_registry,
            alliances: Some(&sim.fog.alliances),
        },
        Some(&*sim),
    );
    if let Some(sid) = pick {
        let _ = sim
            .set_archive_target_represented(id, Some(crate::sim::combat::TargetKind::Entity(sid)));
    }
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.is_some())
}

/// `FootClass::Mission_Hunt @ 0x004D5350` — "go find something and kill it".
///
/// Verified by `decompile_function 0x004D5350` + `disassemble_function
/// 0x004D5350`. Boundary `0x004D5350`-`0x004D55B5`, returns the next dispatch
/// delay in EAX.
///
/// Native order, and what VERA commits for each step:
///
/// 1. **`GetTechnoType()->StupidHunt` (`+0x6D4`, `0x004D535F`).** Set → the
///    handler never scans and falls straight through to step 4. Six stock
///    types carry it and the INI says why: "this guy can't handle a hunt
///    command, so he should just run towards the player".
/// 2. **`Retaliate_And_Scan(&this->Coords, 0)`** (`0x004D5373` pushes the
///    literal mask `0`, `0x004D5392` makes the call). Mask 0 is the whole
///    mechanism of the mission: `TechnoClass::Greatest_Threat @ 0x006F8FE0`
///    opens with `TEST AL,0x3 ; JZ 0x006F9B6E` and jumps past the radius block,
///    the airborne pre-pass and the expanding-ring cell walk alike, landing in
///    a flat walk of the global object array that passes a literal `-1` where
///    the ring path passes its computed radius. So **a hunting object has no
///    distance cutoff** and can pick up an enemy anywhere on the map. It is
///    a scan topology, not a radius, and is modelled as such — the mask travels
///    as [`crate::sim::combat::ScanMission::Hunt`] and
///    `combat::greatest_threat` branches on it. That same walk switches a
///    movement-zone gate ON (`0x006F8EC4` computes the hunter's zone id and
///    `0x006F9D69` hands it to `Evaluate_Candidate`, which rejects any
///    candidate outside it at `0x006F7E9C`), so the hunter reaches the far side
///    of the map but not the far side of a river.
/// 3. **The type arms**, taken only when the scan came back with a target —
///    recorded below rather than committed.
/// 4. **No target**: a human-owned object runs the idle-action virtual
///    (`vt+0x478`, `0x004D557C`); an AI-owned one walks home to its base cell.
/// 5. **The tail** at `0x004D5582`, reached from every arm:
///    `ftol(MissionControl[Hunt].Rate * 900.0) + RandomRanged(0, 2)`.
///
/// So the normal path draws the scenario RNG **twice** — once inside the
/// scanner, once in the tail — and the `StupidHunt` path draws once. An earlier
/// note on this arm said "BOTH exits draw `RandomRanged(0, 2)`", counting one
/// draw; that was the tail only.
///
/// The approach itself is `vt+0x53C` (`FootClass::Greatest_Threat_Scan @
/// 0x004D5690`), which the handler calls for a non-infantry object at
/// `0x004D54E3` and for an infantryman with none of the three type flags at
/// `0x004D54D2`. That routine is the "walk to somewhere I can shoot my target
/// from" search and ends in `Set_Destination` (`vt+0x480`); VERA's equivalent
/// is the pursuit pass, which runs for any object holding a target that is not
/// flagged passively-acquired — and the scanner above deliberately leaves the
/// flag clear, as native does.
///
/// RESIDUAL — **the three type arms are not committed.** All three end in a
/// `Set_Destination(Target, 1)` plus a `Queue_Mission` that VERA cannot yet
/// execute: `Capture` and `Sabotage` are driven here by the order path's
/// `capture_target` / `c4_plant` goal state and its own movement issue, not by
/// a mission handler, and neither mission has a dispatch arm — queueing one
/// from here would park the object on a selector whose timer nothing re-arms.
/// - `Engineer` and not `C4` and no weapon ability 14 (`0x004D53C0`) →
///   `Queue_Mission(Capture)`. Trigger: a berserked or Hunt-ordered engineer
///   that acquires a target. Stock carriers: `ENGINEER`, `SENGINEER`,
///   `YENGINEER`. Frequency: rare — engineers are rarely in a Chaos Drone's
///   splash and are never given a Hunt order by a player.
/// - `C4` or weapon ability 14, with a **BuildingClass** target
///   (`0x004D5416`) → `Set_Destination(target)`, `Queue_Mission(Sabotage)`.
///   Trigger: a berserked demolition infantryman holding a building.
///   Frequency: rare, same reason.
/// - `VehicleThief` (`+0xEC6`, `0x004D546C`) → `Queue_Mission(Capture)`.
///   Frequency: **zero on stock data** — no retail section sets the key.
/// Player effect while they are absent: such a unit shoots what it found
/// instead of walking in to capture or plant. Downstream risk: closing them
/// means giving Capture and Sabotage real handler arms, which is the engineer /
/// terrorist mission work, not this row.
///
/// RESIDUAL — **the infantry no-flags arm's `Set_Destination(NULL, 1)`**
/// (`0x004D54C6`) is not committed: it is a movement-owned write, and the
/// pursuit pass re-derives a destination on the next tick anyway. Effect: a
/// berserked infantryman already walking somewhere keeps that destination for
/// one dispatch where retail drops it first. Frequency: only while such a unit
/// is mid-move.
///
/// RESIDUAL — **the AI return-to-base arm** (`0x004D54EE`-`0x004D5576`) is
/// dead: it is gated on `!HouseClass::IsControlledByHuman(this->Owner)` and
/// `g_GameMode (0x00A8B238) == 0`, and every house in VERA is human. The human
/// arm's `UpdateIdleAction` is already covered — `sim::infantry::tick_idle_actions`
/// admits Hunt and gates on "no target", the same condition — from a different
/// position in the tick, which is that function's own recorded residual.
///
/// RESIDUAL — **the two leaf overrides above this body**, neither of which
/// changes anything today:
/// - `InfantryClass::Mission_Hunt @ 0x0051F540` (slot `+0x228`, single DATA
///   xref `0x007EB280`; `0x007EB280 - 0x007EB058 = 0x228`). Both of its arms
///   open with `HouseClass::IsControlledByHuman(...) == 0`, so both are dead
///   without an AI opponent. They matter when one lands: an AI-owned
///   `Infiltrate`/`Occupier`/`Assaulter` infantryman holding a building
///   `Assign_Mission(Capture)`s and returns 1, and an AI-owned deployed man
///   with no target plays `Do_Action(0x1F)` and returns 1 — **both consume no
///   RNG at all**, so the stream forks the day an AI house ships.
/// - `UnitClass::Mission_Hunt @ 0x0073EFC0` (verified plate comment on the
///   function). A type with `DeploysInto` set (`UnitTypeClass+0x404`) deploys
///   in place instead of hunting and returns `ftol(Rate*900) + RandomRanged(0, 2)`
///   with **no scan**; a type without it tail-calls this body. Trigger: a
///   berserked or Hunt-ordered MCV or deployable vehicle. Player effect: retail
///   unpacks it, VERA sends it hunting. Frequency: low — it needs a Chaos Drone
///   on an MCV. Downstream risk: the arm also consults a `RulesClass` list at
///   `+0x8B0`/`+0x8BC` whose identity is UNCHECKED, so it cannot be closed by
///   guessing.
/// RESIDUAL — **a miner never reaches this body at all**, because the miner
/// exclusion at the head of [`dispatch_supported_foot_mission_cadence`] admits
/// only Guard. Retail's four `StupidHunt=yes` miners (`CMIN`, `SMIN`, `YHVR`,
/// and the `SAPC` transport) would take the idle arm and re-arm on
/// `Rate + RandomRanged(0, 2)`; VERA leaves their timer alone. Trigger: a Chaos
/// Drone on a miner. Frequency: uncommon, and the visible effect is nil either
/// way — a `StupidHunt` type does nothing on Hunt in retail either. Downstream
/// risk: one missing draw per dispatch.
fn evaluate_foot_hunt(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    let type_ref = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.type_ref());
    let stupid_hunt = type_ref
        .and_then(|type_ref| sim.interner.try_resolve(type_ref))
        .and_then(|name| rules.object(name))
        .is_some_and(|obj| obj.stupid_hunt);
    if !stupid_hunt {
        // The return value selects between the type arms (all recorded above)
        // and the idle / return-to-base arm (already covered elsewhere), so
        // nothing branches on it here — but the call itself is the mission:
        // it is what installs the target the pursuit pass then closes on.
        let _acquired = retaliate_and_scan(
            sim,
            id,
            rules,
            MissionType::Hunt,
            // `PUSH 0x0` at `0x004D5373` — the literal threat mask Hunt hands
            // the scanner.
            crate::sim::combat::ScanMission::Hunt,
            ctx,
        );
    }
    MissionHandlerEvaluation::cadence(jittered_mission_cadence(sim, rules, MissionType::Hunt))
}

/// Smallest value of the Area Guard cadence jitter draw (`RandomRanged(1, 5)`).
/// Every other absorbed handler draws `(0, 2)`; this one does not.
const AREA_GUARD_CADENCE_JITTER_MIN: u32 = 1;
/// Largest value of the Area Guard cadence jitter draw (`RandomRanged(1, 5)`).
const AREA_GUARD_CADENCE_JITTER_MAX: u32 = 5;

/// `FootClass::Mission_AreaGuard` 0x004D6AA0 — "hold this spot and cover it".
///
/// With no target installed and the base can-acquire predicate satisfied
/// (`TechnoClass::CanAcquireTarget` 0x007091D0 at 0x004D6EDB), the handler runs
/// the SAME shared target scanner the common AI body runs for
/// Move/Guard/Harvest — slot `+0x39C`, 0x00709820, called at 0x004D6F06 — but
/// with the Area Guard threat mask (which is what widens the acquisition radius
/// — see `combat::threat_range`). If that installs a target the handler returns
/// one frame. Otherwise the cadence is the object's own `[Area Guard] Rate`
/// plus a `RandomRanged(1, 5)` draw (0x004D7059).
///
/// Deliberately NOT represented, recorded:
/// - **the head hand-off arm.** Before anything else, `if ([this+0x2E4]) {
///   Queue_Mission(Guard, commence=1); return 1; }` (0x004D6AAB). VERA has no
///   field for `+0x2E4` and its identity is UNCHECKED. Trigger: whatever sets
///   that dword. Player effect: retail drops such an object straight back onto
///   plain Guard on its next dispatch; VERA keeps it area-guarding. Frequency:
///   unknown pending the field's identity. Downstream risk: none — it is one
///   queue call away once the field is named.
/// - **the three containment latches** the head shares verbatim with
///   `Mission_Guard` (0x004D6ACC-0x004D6B22) — same residual as recorded on
///   [`evaluate_foot_guard_cadence`].
/// - **the harvester-resume arm** (0x004D6D1A-0x004D6D69): for a `UnitClass`
///   (`What_Am_I()` 1) whose type carries byte `+0xE0E`, the handler runs
///   `Queue_Mission(Harvest, 0)`, calls `[vtable+0x1EC]` (Commence) and returns
///   `RandomRanged(1, 10) + 1` — a different cadence AND a different draw from
///   the `(1, 5)` above. Trigger: a miner on Area Guard. Player effect: retail
///   puts it straight back to work; VERA's keeps area-guarding. Frequency:
///   invisible today — the miner exclusion at the head of
///   [`dispatch_supported_foot_mission_cadence`] keeps any miner not on Guard
///   out of the handler, and the only route onto Area Guard is the Move-arrival
///   promotion that is itself a recorded residual. Downstream risk is why it is
///   written down: the `(1, 10) + 1` return is an RNG-consumption difference
///   that lands the moment either of those two closes. `+0xE0E` is UNCHECKED.
/// - **the tank-bunker adjacency scan** at 0x004D6F44, byte-for-byte the block
///   recorded on [`evaluate_foot_guard_cadence`].
/// - **the infantry idle action's call POSITION.** With still no target the
///   handler calls `[vtable+0x478]` at 0x004D6F25 = `InfantryClass::UpdateIdleAction`
///   0x0051CDB0, which fidgets the facing, can play an idle `VocClass` line, and
///   draws its wait re-arm plus up to two more values. `Mission_Guard` calls it
///   from the same no-target position. **VERA does have that system** —
///   `sim::infantry::tick_idle_actions`, which admits Guard, Area Guard and
///   Hunt and gates on "no target", the same condition — but it runs as its own
///   per-tick pass rather than from inside the handler. Trigger: any idle
///   infantryman on Guard, Sticky or Area Guard. Player effect: none visible;
///   the wait timer is the real gate in both. Frequency: continuous.
///   Downstream risk: the draws land a few ticks earlier than the handler's own
///   cadence would take them, which that function already records.
/// - **the guard post.** The original anchors the scan on a stored guard-post
///   target, defaulting it to the object's own cell the first time the handler
///   runs with none. VERA has no such field, so the scan is always anchored on
///   the object — identical while the object is standing on its post, which is
///   the state Area Guard exists to hold.
/// - **the post leash.** When the object drifts further from its post than its
///   own area-guard range, the original drops the target and sends it home.
///   Nothing in VERA moves an Area Guard object off its post today.
/// - **the `What_Am_I() == 2` cadence doubling** at 0x004D7048. The RTTI id is
///   UNCHECKED and no object of that kind reaches this arm; the doubling sits
///   between the `ftol` and the `(1, 5)` draw, so adding it later moves the
///   delay without moving the stream.
/// - one further per-object predicate the original checks between can-acquire
///   and the scan, whose identity is UNKNOWN.
fn evaluate_foot_area_guard(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    let needs_target = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.is_none());
    if needs_target && can_acquire_target(sim, id, rules) {
        passive_target_scan(sim, id, rules, MissionType::AreaGuard, ctx);
        let acquired = sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.attack_target.is_some());
        if acquired {
            // The original returns one frame the moment the scan installs a
            // target, ahead of the cadence tail — so this path draws the
            // scanner's jitter and NOT the `(1, 5)` cadence jitter.
            return MissionHandlerEvaluation::cadence(1);
        }
    }
    let base = mission_cadence(rules, MissionType::AreaGuard);
    let jitter = sim
        .scenario_rng
        .next_range_u32_inclusive(AREA_GUARD_CADENCE_JITTER_MIN, AREA_GUARD_CADENCE_JITTER_MAX)
        as i32;
    let jittered = base.saturating_add(jitter);
    // The `/6` twin of `FootClass::Mission_Attack`'s `/2` band, at
    // `0x004D70D3`-`0x004D7158`: the same type gate, the same
    // `Sqrt_Approx`+`ftol` distance and the same `[282, 768]` window, dividing
    // the ALREADY-JITTERED value (`IMUL 0x2AAAAAAB` plus the sign fixup at
    // `0x004D714A` is a signed divide by 6). The draw is never skipped — the
    // band only scales what it produced.
    //
    // The `if (What_Am_I() == 2) EBP += EBP` doubling at `0x004D7048` sits
    // between the ftol and the draw and is deliberately absent: RTTI 2 is an
    // object kind that does not reach this arm in VERA, and its identity is
    // UNCHECKED.
    let delay = if foot_dispatch_in_cadence_band(sim, rules, id) {
        jittered / 6
    } else {
        jittered
    };
    MissionHandlerEvaluation::cadence(delay)
}

/// `FootClass::Mission_Guard` 0x004D5070, the body Guard(5) and Sticky(6) share.
///
/// **Shared for units. Infantry reach it only through a leaf override.**
/// `InfantryClass`'s slot `+0x21C` is `0x0051F620`, nine instructions:
/// `CALL 0x00521320; CMP EAX,-1; JNZ` returns that value directly, and only
/// `-1` falls through to the Foot body. `0x00521320` owns infantry deploy and
/// undeploy on Guard; its live GI / Guardian GI arm is now taken by the
/// deploy-shim arm in [`dispatch_supported_foot_mission_cadence`], and the two
/// arms that one excludes (`UndeployDelay >= 0`, `ImmuneToRadiation`) are
/// recorded there.
///
/// RESIDUAL — the shim's own **undeployed** branch is still absent. It is
/// gated on `HouseClass::IsControlledByHuman(...) == 0` plus `Deployer=`
/// (`InfantryTypeClass+0xEC8`), `DeployFire=`, a negative `UndeployDelay` and a
/// `Rules[+0xE30]`-indexed frame gate, and makes an AI-owned deployer sit down
/// on its own. Frequency: **zero today** — this project has no AI opponent.
/// Everything else in that branch returns `-1`, which is the Foot body below.
///
/// The Sticky half of the shared-slot claim is confirmed:
/// `MissionClass::GetMissionTimerEntry` @ `0x005B3A00` is
/// `&MissionControl + *(this+0xAC) * 8`, indexed on the **committed** mission —
/// the same field the dispatcher switches on — so `[Sticky] Rate` reaches the
/// shared body exactly as the arm below assumes, and nothing else in the binary
/// distinguishes Sticky from Guard.
///
/// `mission` is the object's OWN committed selector, not the handler's: the
/// native timer lookup indexes the control table on the committed mission id,
/// so the same handler re-arms a Guard object at `[Guard] Rate` and a Sticky
/// object at `[Sticky] Rate`.
///
/// **The handler performs no target acquisition.** Its only no-target tail call
/// is `[vtable+0x478]` (0x004D51A6 and 0x004D51D8), which is the idle-action
/// virtual — `InfantryClass::UpdateIdleAction` 0x0051CDB0 for infantry and the
/// `XOR AL,AL; RET` stub 0x0041C040 for units — not the scanner. The scanner
/// slot is `+0x39C` (0x00709820), which this handler never calls, so a guarding
/// object acquires solely through the common AI body's block. VERA matches.
///
/// **Cadence.** Bunker delegation returns the base cadence with no draw. Past
/// it, while the object's rearm timer runs (`+0x2EC`, which FireAt arms with
/// GetROF each shot:
/// [`GameEntity::rearm_timer`](crate::sim::game_entity::GameEntity::rearm_timer)),
/// the handler returns its remaining frames and draws nothing
/// (0x004D52A9..0x004D52F4): a guarding object that just fired next wakes when
/// it can fire again. Otherwise it is `[Rate] + RandomRanged(0, 2)`
/// (0x004D532F).
///
/// Deliberately NOT represented, recorded:
/// - **the whole target-present arm** (0x004D51E0-0x004D5225). With `[this+0x2B4]`
///   holding a target the handler skips both the bunker scan and the idle action
///   and instead does `if (GetTechnoType()->[+0x390] && ObjectClass::GetHeight()
///   0x005F5F40 == 0) TechnoClass::Set_Destination 0x00741970
///   (FootClass::Find_Nearby_Passable_Cell(...), 1)`. That is a **destination
///   write**, not a cadence value, so it is a behaviour contract rather than a
///   timing detail. Trigger: every Guard dispatch of a grounded object that
///   already holds a target and whose type carries `+0x390`. Player effect:
///   retail shuffles such a guard onto a nearby passable cell while it engages;
///   VERA's stands still. Frequency: the arm itself is entered continuously
///   during any firefight, so until `+0x390` is named — its identity is
///   UNCHECKED — the frequency cannot honestly be called low. Downstream risk:
///   a destination write from the mission handler crosses into movement's
///   ownership, so closing it needs the movement owner in the loop.
/// - **the AI-only Sabotage queue** (0x004D523A-0x004D52A9): for an
///   `InfantryClass` (`What_Am_I()` 0xF) whose house is not human-controlled
///   (0x0050B730 on `[this+0x21C]`), that either carries type byte `+0xEC2` or
///   passes `TechnoClass::HasWeaponAbility(0xE)` 0x0070D0D0, whose current
///   mission is not already Sabotage, and whose target is a `BuildingClass`,
///   `Queue_Mission(0x11, 0)`. Trigger: an AI demolition infantryman standing
///   guard that picks up an enemy building. Player effect: retail's walks in and
///   plants; VERA's shoots it instead. Frequency: **zero today** — the arm is
///   gated on a non-human house and VERA has no AI opponent — and continuous
///   once one lands, which is why it is recorded rather than left unwritten.
///   `+0xEC2` is UNCHECKED.
/// - **the two further containment latches.** The head takes three byte
///   latches in order — `+0x68F` → `[vtable+0x340]` (0x004DFB70), `+0x690` →
///   `[+0x348]`, `+0x691` → `[+0x34C]` — and each delegates, discards the
///   result and returns the flat `ftol(Rate × 900)` with no jitter draw
///   (0x004D5076-0x004D50CD). VERA models the first as `bunker_delegate`; the
///   identity of the other two is UNCHECKED, and VERA carries no state that
///   could be their equivalent, so there is nothing to gate on. The identical
///   three-way head opens `FootClass::Mission_AreaGuard` at 0x004D6ACC, so this
///   is a FootClass-wide containment cluster, not a bunker special case.
///   Trigger: whichever containment those two bytes denote. Player effect:
///   cadence only — a contained object re-dispatches on the flat rate instead
///   of rate-plus-jitter. Frequency: unknown until the bytes are identified,
///   which is why this cannot be closed by guessing. Downstream risk: one
///   scenario-RNG draw per dispatch on those paths.
/// - **the tank-bunker adjacency scan** (0x004D5116-0x004D51D2, repeated
///   verbatim in Mission_AreaGuard at 0x004D6F44). Behind
///   `TechnoClass::GetWeapon(1)` 0x0070E140 returning a live weapon whose
///   warhead byte `+0x158` is set, the handler walks the eight-neighbour offset
///   table at 0x0089F688 for a building whose type byte `+0x1575` is set and
///   whose owner matches its own, then `Assign_Target` (`[+0x3C8]`), sets
///   `+0x68E` and `Queue_Mission(1, 0)`. Trigger: a unit sitting on Guard in a
///   cell orthogonally or diagonally adjacent to a tank bunker its own house
///   owns. Player effect: retail garrisons the bunker by itself; VERA's unit
///   stays outside. Frequency: near zero in ordinary skirmish — tank bunkers
///   are pre-placed map objects on a minority of stock maps and are rarely
///   captured. Downstream risk: none; `bunker_link` already exists, but the two
///   gating flags (`+0x158` on the warhead, `+0x1575` on the building type) are
///   UNCHECKED and would have to be resolved to INI keys first.
/// - **the second cadence-tail short-circuit ahead of the jitter draw**
///   (past the rearm return). `GetTechnoType()->[+0x6B0]` — the **`DistributedFire`**
///   bool, key string 0x00843A64, read by `TechnoTypeClass::ReadINI` at
///   0x00714850/0x00714864 — combined with the object counter `+0x468 > 0`
///   returns **0**, re-dispatching on the next frame and again drawing nothing.
///   Trigger: a `DistributedFire` type on Guard with a live spread-fire count.
///   Player effect: it re-evaluates every frame instead of every ~26. Frequency:
///   the Aegis Cruiser is the only stock `DistributedFire` type, so naval maps
///   with an Allied player only. Downstream risk: VERA implements no
///   distributed-fire mechanism at all (recorded at `passive_target_scan`), so
///   the counter this gate reads has no VERA counterpart to bind to.
fn evaluate_foot_guard_cadence(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    mission: MissionType,
    bunker_delegate: bool,
) -> MissionHandlerEvaluation {
    if bunker_delegate {
        return MissionHandlerEvaluation::cadence(mission_cadence(rules, mission));
    }
    let rearm = sim.substrate.entities.get(id).map_or(0, |entity| {
        entity
            .rearm_timer
            .remaining(sim.session.binary_frame as i32)
    });
    if rearm != 0 {
        return MissionHandlerEvaluation::cadence(rearm);
    }
    MissionHandlerEvaluation::cadence(jittered_mission_cadence(sim, rules, mission))
}

/// The harvester arms of `UnitClass::Mission_Guard @ 0x00740810` (decompiled
/// 2026-09-05), run ahead of the `FootClass::Mission_Guard @ 0x004D5070`
/// tail-call for a `Harvester=`/`Weeder=` type (`UnitType+0xE0E`/`+0xE0F`):
///
/// - (ii) **AI house** (`HouseClass::IsControlledByHuman @ 0x0050B730`
///   false): walk the type's `Dock=` list (`Type+0x3EC`, count `+0x3F8`)
///   and stop at the FIRST entry with `HouseClass::CountOwnedInstances >
///   0`; there, NOT (`Harvester` `+0xE0E` && `House+0x242`) →
///   `Queue_Mission(Harvest, 0); return 1`, else `break` (later Dock
///   entries are not consulted — irrelevant here, the test is
///   entry-independent). No owned instance of any entry → fall through.
///   The `+0x242` latch is `HouseState::harvester_no_ore`, written
///   native-true by the Harvest scan miss and never cleared; the Rust
///   ownership count is `EntityStore::count_owned_of_type` (see its doc for
///   the Unlimbo..Limbo vs insert..remove window).
/// - (iii) **human house && `Teleporter=`** (`UnitType+0xCD4`): walk the 8
///   neighbour cells (`MapCoord_StepByDir_GetCell`); a building there whose
///   type has `+0x16BB` (`Refinery=`) and whose owner `+0x21C` is this house
///   → queue Harvest, return 1. Else `Get_Storage_Percentage() == 1.0` and
///   the locomotor's `Is_Moving` (ILocomotion slot `+0x10`) true → queue
///   Harvest, return 1. `TeleportLocomotionClass::Is_Moving @ 0x00718080`
///   is `byte +0x30 == 1`, true only for the Relocate tick — the
///   `ready_producer` Teleport producer (`is_moving_now_for`), never "a
///   teleport state exists". A human WAR miner has no arm here: once on
///   Guard it stays until a player order.
///
/// Neither arm draws RNG. The slave-recall gate (i) ahead of both, the
/// `DeploysInto` AI arm (iv) and the weeder latch (v) are outside this lane.
/// A house missing from the registry reads as human (VERA fixtures register
/// no houses by default).
fn harvester_guard_override_requeues_harvest(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(miner) = entity.miner.as_ref() else {
        return false;
    };
    let Some(unit_type) = sim.object_type(entity.type_ref(), rules) else {
        return false;
    };
    if !unit_type.harvester {
        return false;
    }
    let human = sim
        .houses
        .get(&entity.owner())
        .is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero));
    if !human {
        // Arm (ii), `0x00740880..0x0074092C`.
        let owns_dock = unit_type.dock.iter().any(|dock_type| {
            sim.interner.get(dock_type).is_some_and(|type_ref| {
                sim.substrate
                    .entities
                    .count_owned_of_type(entity.owner(), type_ref)
                    > 0
            })
        });
        if !owns_dock {
            return false;
        }
        let no_ore = sim
            .houses
            .get(&entity.owner())
            .is_some_and(|house| house.harvester_no_ore);
        return !(unit_type.harvester && no_ore);
    }
    if !unit_type.teleporter {
        return false;
    }
    let (rx, ry) = (i32::from(entity.position.rx), i32::from(entity.position.ry));
    for (dx, dy) in NEIGHBOUR_8 {
        let (Ok(cx), Ok(cy)) = (u16::try_from(rx + dx), u16::try_from(ry + dy)) else {
            continue;
        };
        let Some(cell) = sim.substrate.occupancy.get(cx, cy) else {
            continue;
        };
        let own_refinery = cell
            .blockers(crate::sim::movement::locomotor::MovementLayer::Ground)
            .any(|sid| {
                sim.substrate.entities.get(sid).is_some_and(|building| {
                    building.category == EntityCategory::Structure
                        && !building.dying
                        && building.owner() == entity.owner()
                        && sim
                            .object_type(building.type_ref(), rules)
                            .is_some_and(|obj| obj.refinery)
                })
            });
        if own_refinery {
            return true;
        }
    }
    // `Is_Moving` on the teleport locomotor: the Relocate tick only.
    miner.is_full()
        && crate::sim::movement::ready_producer::is_moving_now_for(entity, sim.session.binary_frame)
}

/// `MapCoord_StepByDir_GetCell(dir)` for dir 0..8 — the eight neighbours in
/// facing order starting north, clockwise. Only membership matters here.
const NEIGHBOUR_8: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// The un-overridden `MissionClass` handler's return value, in frames.
///
/// Every base mission stub in the original is the same two instructions —
/// load 450, return — so a slot no leaf class overrides does nothing and comes
/// back in 30 seconds at 15 fps. It reads no INI: `[Sleep] Rate=1` would be 900
/// frames and is dead data for this path.
pub(super) const BASE_MISSION_HANDLER_FRAMES: i32 = 450;

/// Which committed missions still sit on the un-overridden base handler for a
/// given category, i.e. which ones re-arm with the flat
/// [`BASE_MISSION_HANDLER_FRAMES`] and consume no RNG.
///
/// Read directly out of the `UnitClass` and `InfantryClass` mission-handler
/// vtable blocks (the slots holding the shared 450-frame stubs), so this is a
/// per-category fact, not an inference: `Repair` is a base stub for Infantry
/// and a real override for Units.
///
/// `None` — the idle sentinel — belongs here because the dispatcher's bounds
/// test is unsigned: the sentinel takes the switch default, which calls the
/// same slot `Sleep(0)` does.
///
/// A mission that is NOT in this set has a real leaf handler VERA has not
/// absorbed; returning `None` leaves its timer untouched rather than writing a
/// cadence the original never produces.
pub(super) fn base_mission_handler_delay(
    category: EntityCategory,
    mission: Option<MissionType>,
) -> Option<i32> {
    let Some(mission) = mission else {
        // The `-1` idle sentinel takes the unsigned default arm.
        return Some(BASE_MISSION_HANDLER_FRAMES);
    };
    let shared_base_stub = matches!(
        mission,
        MissionType::Sleep
            | MissionType::QMove
            | MissionType::Return
            | MissionType::Stop
            | MissionType::Ambush
            | MissionType::Construction
            | MissionType::Selling
            | MissionType::Missile
            | MissionType::Harmless
            | MissionType::Open
            | MissionType::ParadropApproach
            | MissionType::ParadropOverfly
            | MissionType::Deliberate
            | MissionType::AttackMove
            | MissionType::SpyplaneApproach
            | MissionType::SpyplaneOverfly
    );
    // Repair is the one slot the two categories disagree on.
    let category_base_stub = category == EntityCategory::Infantry && mission == MissionType::Repair;
    (shared_base_stub || category_base_stub).then_some(BASE_MISSION_HANDLER_FRAMES)
}

#[inline]
fn mission_cadence(rules: &RuleSet, mission: MissionType) -> i32 {
    rules
        .mission_control
        .rate_frames(mission)
        .min(i32::MAX as u32) as i32
}

#[inline]
fn jittered_mission_cadence(sim: &mut Simulation, rules: &RuleSet, mission: MissionType) -> i32 {
    let base = mission_cadence(rules, mission);
    let jitter = sim.scenario_rng.next_range_u32_inclusive(0, 2) as i32;
    base.saturating_add(jitter)
}

/// Whether this dispatch takes the shortened cadence — `FootClass::Mission_Attack`'s
/// `/2` or `FootClass::Mission_AreaGuard`'s `/6`.
///
/// gamemd-derived: `FootClass::Mission_Attack @ 0x004D4DC0` and
/// `FootClass::Mission_AreaGuard @ 0x004D6AA0` carry the **same** gate,
/// instruction for instruction, and differ only in the divisor. Each shortens
/// its return only when a target is installed AND the attacker qualifies by
/// TYPE AND the 2D distance falls in the close band. The type half is
/// `(What_Am_I() == 0xF && InfantryType->CloseRange) || primaryWeapon.Range <=
/// 0x200` — an infantry type carrying `CloseRange=`, or any type whose primary
/// reaches at most 512 leptons. The binary spells the second half
/// `CMP dword ptr [ECX+0xB4], 0x200 ; JG` (`0x004D4F12`, `0x004D70C3`): it
/// takes the shortened cadence when the range is NOT greater than `0x200`.
///
/// That gate was missing, which is the whole point of this function's rewrite:
/// without it every tank, rifle infantryman and artillery piece ran the halved
/// cadence whenever it closed to 1.1–3 cells. 22 of the 93 stock vehicle and
/// infantry types have a short enough primary; the other 71 must return the
/// full cadence. Both branches draw the same jitter, so the cost is not an
/// extra draw — it is that the Attack dispatch, and the scenario-RNG jitter it
/// consumes, ran at roughly double the native rate through every close-quarters
/// engagement.
///
/// The band boundaries are SETTLED, and they are integer tests on the
/// *approximated* distance, not on the squared one. `disassemble_bytes
/// 0x004D4EF0..0x004D4FB1`:
///
/// ```text
/// FILD dy ; FILD dx ; dx*dx ; dy*dy ; FADDP        ; exact in f64
/// CALL 0x004CAC40                                  ; Sqrt_Approx -> f32
/// CALL 0x007C5F00                                  ; Math__ftol  -> EAX, truncating
/// CMP  EAX,0x300 ; JG  skip                        ; require len <= 768
/// FILD len ; FCOMP double [0x007E9228]             ; K
/// FNSTSW AX ; TEST AH,0x1 ; JNZ skip               ; C0 set means len < K
/// ```
///
/// `read_memory 0x007E9228` is `9A 99 99 99 99 99 71 40` = **281.6** exactly
/// (1.1 cells), so with `len` integral the lower bound is `len >= 282` and the
/// band is `282 ..= 768`.
///
/// The previous implementation applied those two numbers to the **squared**
/// distance. That is wrong at the top, but only just: native admits every `len`
/// that truncates to 768, i.e. `d2 < 769² = 591361`, where `d2 <= 768² =
/// 589824` stopped one lepton early. The two predicates therefore disagree on
/// exactly one shell — separations strictly between 768 and 769 leptons, under
/// 1/256 of a cell wide — and agree everywhere else including at 768 itself.
/// *Frequency:* an attacker inside the band re-tests this every Attack or Area
/// Guard dispatch while closing, so a unit crossing 3 cells passes through the
/// shell often; but it occupies the shell for at most one dispatch, and the
/// only consequence of landing in it is which of two cadences that single
/// dispatch returns. Player-visible effect: none observed, one dispatch of
/// re-aim latency at most.
///
/// The bigger reason to reproduce the lookup rather than compare squares is
/// that no squared predicate can be exact at all: `Sqrt_Approx @ 0x004CAC40`
/// is a 16384-entry f32 mantissa lookup, not an IEEE root, so its truncated
/// result is not a function of `d2` that squaring can invert.
fn foot_dispatch_in_cadence_band(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    /// The primary-weapon reach below which any type qualifies, in leptons.
    /// Native is `CMP dword ptr [ECX+0xB4], 0x200 ; JG` (`0x004D4F12`,
    /// `0x004D70C3`) — qualify when `range <= 0x200`. Held here as the
    /// exclusive bound `0x201` because the comparison below is `<`; the two
    /// forms are equivalent on integers.
    const CLOSE_PRIMARY_RANGE_LEPTONS: i64 = 0x201;

    let Some(attacker) = sim.substrate.entities.get(id) else {
        return false;
    };
    if !foot_type_takes_cadence_band(sim, rules, attacker, CLOSE_PRIMARY_RANGE_LEPTONS) {
        return false;
    };
    let Some(crate::sim::combat::AttackTarget {
        target: crate::sim::combat::TargetKind::Entity(target_id),
        ..
    }) = attacker.attack_target.as_ref()
    else {
        return false;
    };
    let Some(target) = sim.substrate.entities.get(*target_id) else {
        return false;
    };
    native_distance_is_in_cadence_band(&attacker.position, &target.position)
}

/// Lower bound of the shared cadence band, in leptons.
///
/// `FCOMP double ptr [0x007E9228]` against **281.6** with an integral `len`,
/// so the first admitted value is 282. Both consumers — Attack's `/2` at
/// `0x004D4F8A` and Area Guard's `/6` at `0x004D7139` — read the same constant.
const CADENCE_BAND_MIN_LEPTONS: i64 = 282;
/// Upper bound of the shared cadence band, in leptons: `CMP EAX,0x300 ; JG`.
const CADENCE_BAND_MAX_LEPTONS: i64 = 768;

/// `len in [282, 768]` on the native approximated 2D distance.
///
/// gamemd-derived: the identical block in `FootClass::Mission_Attack @
/// 0x004D4F22`-`0x004D4F9B` and `FootClass::Mission_AreaGuard @
/// 0x004D70D3`-`0x004D7148`. The two `FILD`s take the **integer lepton**
/// component differences, the products and their sum are exact in f64, and the
/// only inexact step is `Sqrt_Approx`, which is reproduced bit-for-bit by
/// [`sqrt_approx_f32`].
fn native_distance_is_in_cadence_band(
    from: &crate::sim::components::Position,
    to: &crate::sim::components::Position,
) -> bool {
    let lepton = |cell: u16, sub: crate::util::fixed_math::SimFixed| -> i64 {
        cell as i64 * 256 + sub.to_num::<i64>()
    };
    let dx = lepton(from.rx, from.sub_x) - lepton(to.rx, to.sub_x);
    let dy = lepton(from.ry, from.sub_y) - lepton(to.ry, to.sub_y);
    let (Ok(dx), Ok(dy)) = (i32::try_from(dx), i32::try_from(dy)) else {
        // Native holds both differences in 32-bit registers; a map large
        // enough to overflow one cannot exist.
        return false;
    };
    let dx = X87Chop53::load_i32(dx);
    let dy = X87Chop53::load_i32(dy);
    // `FADDP` adds dy*dy (ST0) into dx*dx (ST1); both products are exact, so
    // the order is immaterial, but it is written the native way.
    let sum = X87Chop53::add(X87Chop53::mul(dx, dx), X87Chop53::mul(dy, dy));
    let Ok(root) = sqrt_approx_f32(sum) else {
        return false;
    };
    let Ok(loaded) = X87Chop53::load_f32(root) else {
        return false;
    };
    let Ok(len) = X87Chop53::ftol_i64(loaded) else {
        return false;
    };
    (CADENCE_BAND_MIN_LEPTONS..=CADENCE_BAND_MAX_LEPTONS).contains(&len)
}

/// The type half of the halved-cadence gate.
///
/// `What_Am_I() == 0xF` is InfantryClass, so `CloseRange=` only qualifies an
/// infantry type — a vehicle carrying the key would NOT take the short path in
/// native, and does not here.
fn foot_type_takes_cadence_band(
    sim: &Simulation,
    rules: &RuleSet,
    attacker: &crate::sim::game_entity::GameEntity,
    close_primary_range_leptons: i64,
) -> bool {
    let Some(object) = rules.object(sim.interner.resolve(attacker.type_ref())) else {
        return false;
    };
    if attacker.category == EntityCategory::Infantry && object.close_range {
        return true;
    }
    let Some(primary) = object
        .primary
        .as_deref()
        .and_then(|name| rules.weapon(name))
    else {
        return false;
    };
    (primary.range * crate::util::fixed_math::SimFixed::from_num(256)).to_num::<i64>()
        < close_primary_range_leptons
}

/// `InfantryClass::Mission_Attack`'s deployed arm, vtable `+0x428` =
/// `0x0051F330`, in native commit order:
///
/// 1. keep the installed target if it is still legal for the object's weapon
///    (`[vtable+0x3A8]`) — nothing else runs;
/// 2. otherwise rescan in place (`[vtable+0x3C4]`, `Greatest_Threat` with
///    threat flags `1`) and, when the object either had a target or found one,
///    commit the result through `Assign_Target` (`[vtable+0x3C8]`). Note the
///    consequence: a stale target with nothing found is CLEARED here;
/// 3. with nothing found and the committed mission not Guard, run
///    `Enter_Idle_Mode(0, 1)` (`[vtable+0x484]`).
///
/// The object never walks: there is no destination write on any arm.
///
/// RESIDUAL — two approximations, both stated rather than absorbed:
/// - step 1's legality test is `attack_target_is_stale`, which is aliveness,
///   not `[vtable+0x3A8]`'s weapon-vs-target legality. A deployed GI holding a
///   live target its weapon can no longer engage keeps it here, where native
///   rescans. Trigger: a target that changes legality without dying — chiefly
///   one that leaves range or cloaks. Frequency: occasional inside an
///   engagement, and cloak does not exist in VERA yet (GSI-12.05).
/// - the `GetTechnoType()->[+0xD94] == 0` gate on the idle exit is UNCHECKED
///   and left out. Omitting it can only let the idle exit run where native
///   skipped it; no stock type is known to set the byte.
fn infantry_deployed_attack_reacquire(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    input: MissionHandlerInput,
    ctx: super::ObjectAiCtx<'_>,
) -> Option<MissionType> {
    let had_target = input.has_attack_target;
    if had_target && !attack_target_is_stale(sim, id) {
        return None;
    }
    // The raw scan, NOT `passive_target_scan`: that routine also stamps the
    // scan frame and re-arms the acquisition cadence with its own
    // `RandomRanged(0, 2)` draw, and `0x0051F330` calls `Greatest_Threat`
    // directly through `[vtable+0x3C4]` without either.
    let pick = crate::sim::combat::acquire_best_target_for_entity(
        &sim.substrate.entities,
        &sim.substrate.occupancy,
        rules,
        &sim.interner,
        id,
        Some(&sim.fog),
        sim.resolved_terrain.as_ref(),
        sim.playfield_bounds.is_some(),
        // `vt+0x3C4(1, ...)` — `0x0051F330` pushes the literal `1`, the narrow
        // (plain-Guard) mask, EVEN WHEN the infantryman it is re-acquiring for
        // is committed to Area Guard. This is the callsite-vs-mission
        // distinction in its most visible form: reading the mask off the
        // entity's mission would hand a deployed Guardian GI on Area Guard the
        // doubled radius that native reserves for the Area Guard handler's own
        // literal `2`.
        crate::sim::combat::ScanMission::Guard,
        sim.zone_grid.as_ref(),
        crate::sim::combat::line_of_fire::LineOfFireInputs {
            overlay_grid: sim.overlay_grid.as_ref(),
            overlay_registry: ctx.overlay_registry,
            alliances: Some(&sim.fog.alliances),
        },
        Some(&*sim),
    );
    if had_target || pick.is_some() {
        let current = sim
            .substrate
            .entities
            .get(id)
            .and_then(|e| e.attack_target.as_ref().map(|t| t.target));
        match (current, pick) {
            // Swinging onto a different victim keeps the rearm countdown, the
            // burst counter and the inter-shot delay, exactly as the scanner's
            // own retarget does — rebuilding the record would hand out a free
            // shot on every re-pick.
            (Some(held), Some(sid)) if held != crate::sim::combat::TargetKind::Entity(sid) => {
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    crate::sim::combat::retarget_in_place(entity, sid);
                }
            }
            _ => {
                let _ = sim.set_archive_target_represented(
                    id,
                    pick.map(crate::sim::combat::TargetKind::Entity),
                );
            }
        }
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.passively_acquired_target = pick.is_some();
        }
    }
    if pick.is_some() {
        return None;
    }
    // `decompile_function 0x0051F330`: the idle exit is `if (this->[0xAC] != 5
    // && GetTechnoType()[0xD94] == 0) Enter_Idle_Mode(0, 1)` — skipped when the
    // COMMITTED mission is Guard. That was vacuous while the Attack handler was
    // the only caller; the deploy shim `FUN_00521320` also reaches this virtual
    // from Guard, Sticky and Area Guard, so the gate is now live.
    if input.mission == Some(MissionType::Guard) {
        return None;
    }
    sim.temporal_release_if_warping(id);
    foot_enter_idle_mode_queue(rules, input)
}

fn attack_target_is_stale(sim: &Simulation, id: u64) -> bool {
    let Some(attacker) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(crate::sim::combat::AttackTarget {
        target: crate::sim::combat::TargetKind::Entity(target_id),
        ..
    }) = attacker.attack_target.as_ref()
    else {
        return false;
    };
    !sim.substrate
        .entities
        .get(*target_id)
        .is_some_and(|target| !target.dying && target.is_alive())
}

#[cfg(test)]
mod harvester_guard_override_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::miner::{CargoBale, Miner, MinerConfig, MinerKind, ResourceType};
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
    use crate::sim::occupancy::CellListInsertion;

    const MINER_ID: u64 = 1;
    const REFINERY_ID: u64 = 2;
    const REFINERY_NW: (u16, u16) = (10, 10);

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=HARV\n1=CMIN\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAREFN\n\
             [HARV]\nHarvester=yes\nDock=GAREFN\nSpeed=4\n\
             [CMIN]\nHarvester=yes\nTeleporter=yes\nDock=GAREFN\nSpeed=4\n\
             [GAREFN]\nFoundation=4x3\nRefinery=yes\n",
        ))
        .expect("guard override rules")
    }

    fn spawn_refinery(sim: &mut Simulation, owner: &str) {
        let owner = sim.interner.intern(owner);
        let type_ref = sim.interner.intern("GAREFN");
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            REFINERY_ID,
            REFINERY_NW.0,
            REFINERY_NW.1,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 900 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        ge.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(ge);
        for y in REFINERY_NW.1..REFINERY_NW.1 + 3 {
            for x in REFINERY_NW.0..REFINERY_NW.0 + 4 {
                sim.substrate.occupancy.add(
                    x,
                    y,
                    REFINERY_ID,
                    MovementLayer::Ground,
                    None,
                    CellListInsertion::AppendBuilding,
                );
            }
        }
    }

    /// A miner of `kind` at `cell`, committed to Guard with a due timer.
    fn spawn_guard_miner(sim: &mut Simulation, kind: MinerKind, cell: (u16, u16)) {
        let owner = sim.interner.intern("Americans");
        let type_name = match kind {
            MinerKind::Chrono => "CMIN",
            _ => "HARV",
        };
        let type_ref = sim.interner.intern(type_name);
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            MINER_ID,
            cell.0,
            cell.1,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 400 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.locomotor = Some(LocomotorState::for_test_kind(match kind {
            MinerKind::Chrono => LocomotorKind::Teleport,
            _ => LocomotorKind::Drive,
        }));
        ge.miner = Some(Miner::new(kind, &MinerConfig::default(), 0));
        sim.substrate.entities.insert(ge);
        sim.mission_assign_exact(MINER_ID, MissionId::from_known(MissionType::Guard), 0)
            .expect("assign Guard");
    }

    fn fill_cargo(sim: &mut Simulation) {
        let entity = sim.substrate.entities.get_mut(MINER_ID).expect("miner");
        let miner = entity.miner.as_mut().expect("miner");
        let capacity = miner.capacity_bales;
        miner.cargo = (0..capacity)
            .map(|_| CargoBale {
                resource_type: ResourceType::Ore,
                value: 25,
            })
            .collect();
    }

    fn dispatch(sim: &mut Simulation, rules: &RuleSet) {
        dispatch_supported_foot_mission_cadence(
            sim,
            MINER_ID,
            rules,
            super::super::ObjectAiCtx {
                path_grid: None,
                overlay_registry: None,
                terrain_spawner_cells: None,
                miner_config: None,
            },
        );
    }

    fn queued(sim: &Simulation) -> Option<MissionType> {
        sim.substrate
            .entities
            .get(MINER_ID)
            .expect("miner")
            .mission
            .queued()
            .known()
    }

    /// Arm (iii), neighbour refinery: `0x00740922..0x0074092C`.
    #[test]
    fn chrono_miner_on_guard_beside_its_own_refinery_requeues_harvest() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, "Americans");
        // (14, 11) touches footprint cell (13, 11).
        spawn_guard_miner(&mut sim, MinerKind::Chrono, (14, 11));

        let scenario_before = sim.rng_state().scenario;
        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), Some(MissionType::Harvest));
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.dispatch_timer().delay(), 1, "return 1");
        assert_eq!(
            sim.rng_state().scenario,
            scenario_before,
            "the Foot Guard body (and its draw) is never reached"
        );
    }

    #[test]
    fn chrono_miner_beside_another_houses_refinery_stays_on_guard() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, "Russians");
        spawn_guard_miner(&mut sim, MinerKind::Chrono, (14, 11));

        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), None, "`building+0x21C == this->Owner` fails");
    }

    fn set_teleport_phase(sim: &mut Simulation, phase: TeleportPhase) {
        sim.substrate
            .entities
            .get_mut(MINER_ID)
            .expect("miner")
            .teleport_state = Some(TeleportState {
            phase,
            target_rx: 14,
            target_ry: 11,
            being_warped_ticks: 30,
        });
    }

    /// Arm (iii), second test: full storage and the teleport locomotor's
    /// `Is_Moving` (`0x00718080`: `+0x30 == 1`, the Relocate tick only) →
    /// Harvest; full-but-idle and the post-warp ChronoDelay phase both fall
    /// through.
    #[test]
    fn full_chrono_miner_mid_warp_requeues_harvest_away_from_any_refinery() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::Chrono, (40, 40));
        fill_cargo(&mut sim);
        // Full but not moving: falls through to the Foot Guard body.
        dispatch(&mut sim, &rules);
        assert_eq!(queued(&sim), None);

        // A teleport state that is NOT the Relocate tick is not `Is_Moving`.
        set_teleport_phase(&mut sim, TeleportPhase::ChronoDelay);
        sim.session.binary_frame += 40;
        dispatch(&mut sim, &rules);
        assert_eq!(
            queued(&sim),
            None,
            "the post-warp chrono delay is not the locomotor's moving flag"
        );

        set_teleport_phase(&mut sim, TeleportPhase::Relocate);
        sim.session.binary_frame += 40;
        dispatch(&mut sim, &rules);
        assert_eq!(queued(&sim), Some(MissionType::Harvest));
    }

    /// An empty chrono miner on the Relocate tick has no arm: storage must
    /// read 100% first.
    #[test]
    fn empty_chrono_miner_mid_warp_stays_on_guard() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::Chrono, (40, 40));
        set_teleport_phase(&mut sim, TeleportPhase::Relocate);

        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), None);
    }

    fn register_house(sim: &mut Simulation, is_human: bool) {
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, is_human, 0, 10),
        );
    }

    /// Arm (ii): AI house, owns a `Dock=` instance, `House+0x242` clear →
    /// `Queue_Mission(Harvest, 0); return 1` (`0x0074092C`), no RNG draw.
    #[test]
    fn ai_war_miner_on_guard_requeues_harvest_while_house_no_ore_latch_is_clear() {
        let rules = rules();
        let mut sim = Simulation::new();
        register_house(&mut sim, false);
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::War, (40, 40));

        let scenario_before = sim.rng_state().scenario;
        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), Some(MissionType::Harvest));
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.dispatch_timer().delay(), 1, "return 1");
        assert_eq!(sim.rng_state().scenario, scenario_before);
    }

    /// Arm (ii) with `Harvester=yes && House+0x242` set → `break`, the Foot
    /// Guard body runs instead.
    #[test]
    fn ai_war_miner_on_guard_stays_when_house_no_ore_latch_is_set() {
        let rules = rules();
        let mut sim = Simulation::new();
        register_house(&mut sim, false);
        let owner = sim.interner.intern("Americans");
        sim.houses.get_mut(&owner).expect("house").harvester_no_ore = true;
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::War, (40, 40));

        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), None);
    }

    /// Arm (ii) needs `CountOwnedInstances > 0` for some `Dock=` entry.
    #[test]
    fn ai_war_miner_on_guard_without_a_dock_instance_stays_on_guard() {
        let rules = rules();
        let mut sim = Simulation::new();
        register_house(&mut sim, false);
        spawn_guard_miner(&mut sim, MinerKind::War, (40, 40));

        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), None);
    }

    /// A human house never enters arm (ii): a war miner parks.
    #[test]
    fn human_war_miner_on_guard_with_latch_clear_stays_on_guard() {
        let rules = rules();
        let mut sim = Simulation::new();
        register_house(&mut sim, true);
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::War, (40, 40));

        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), None);
    }

    /// A human WAR miner has no arm in `UnitClass::Mission_Guard`: parked is
    /// parked until a player order.
    #[test]
    fn war_miner_on_guard_beside_its_refinery_stays_on_guard() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::War, (14, 11));
        fill_cargo(&mut sim);

        dispatch(&mut sim, &rules);

        assert_eq!(queued(&sim), None);
        let entity = sim.substrate.entities.get(MINER_ID).expect("miner");
        assert_eq!(entity.mission.current().known(), Some(MissionType::Guard));
    }
}

#[cfg(test)]
mod guard_rearm_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;

    /// `FootClass::Mission_Guard 0x004D52A9`: for every timer state the oracle
    /// ran, the handler returns what native returns and draws only where
    /// native falls through to its jitter (`tools/spatial_oracle/rearm_timer.py`,
    /// `guard` rows: paused, running, spent, and wrapping differences).
    #[test]
    fn guard_rearm_return_matches_the_original() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/rearm_timer.json"
        ))
        .unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str("[General]\n\n[Guard]\nRate=.016\n"))
            .expect("guard rate rules");
        let rows = vectors["guard"].as_array().unwrap();
        assert_eq!(rows.len(), 96);
        for row in rows {
            let field = |name: &str| row["input"][name].as_i64().unwrap() as i32;
            let mut sim = Simulation::with_seed(1);
            sim.session.binary_frame = field("frame") as u32;
            let mut entity = GameEntity::test_default(1, "TEST", "Americans", 5, 5);
            entity.rearm_timer =
                crate::sim::timer::CdTimer::from_raw(field("start"), field("duration"));
            sim.substrate.entities.insert(entity);
            let before = sim.scenario_rng.logical_state();

            let evaluation =
                evaluate_foot_guard_cadence(&mut sim, &rules, 1, MissionType::Guard, false);

            match row["returns"].as_i64() {
                Some(returned) => {
                    assert_eq!(evaluation.delay, returned as i32, "{row}");
                    assert_eq!(sim.scenario_rng.logical_state(), before, "{row}");
                }
                None => assert_ne!(sim.scenario_rng.logical_state(), before, "{row}"),
            }
        }
    }
}
