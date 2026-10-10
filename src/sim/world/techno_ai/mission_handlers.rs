//! Foot/Unit mission handlers and their ordered target/navigation effects.
//!
//! The object-AI host and its ordering remain in the parent module; this module
//! owns only handler inputs, results, and the single timer epilogue.

use super::{Simulation, can_acquire_target};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::util::direction_tables::CELL_DELTAS;

#[cfg(test)]
#[path = "foot_mission_oracle_tests.rs"]
pub(in crate::sim::world::techno_ai) mod foot_mission_oracle_tests;

#[cfg(test)]
#[path = "deployed_guard_oracle_tests.rs"]
mod deployed_guard_oracle_tests;

#[cfg(test)]
#[path = "automatic_deploy_oracle_tests.rs"]
mod automatic_deploy_oracle_tests;

#[cfg(test)]
#[path = "../infantry_fire_oracle_tests.rs"]
mod infantry_fire_oracle_tests;

/// Re-arm the evidence-backed Foot/Unit handler subset without duplicating the
/// legacy movement, combat, or target-selection systems.
///
/// YR `MissionClass::AI` at `0x005B3060` gates the current handler on the
/// dispatch timer and writes `(current frame, handler return)` afterward.
/// Each represented handler calls the existing movement/combat owners for its
/// side effects. Ordinary Unit Cell Attack calls Foot Approach before the
/// mission cadence draw and the same object's movement/fire slots. Harvest
/// returns its delay through this same epilogue. The committed selector alone
/// chooses the handler for miners as for every other Foot. Unrepresented
/// acquisition and approach branches remain
/// explicit residuals rather than guessed AI.
pub(crate) fn dispatch_foot_mission(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> bool {
    let mut bridge_changed = false;
    let now = sim.session.binary_frame;
    let input = {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return bridge_changed;
        };
        // Mission5B306C..5B30AC admits only an alive, positive-health receiver.
        // Pin direct receiver calls as well as the enclosing object-AI host:
        // harvest_attack_return.json dead_source / zero_health_source.
        if entity.dying || !entity.is_object_alive() || !super::mission_handlers_run(sim, id) {
            return bridge_changed;
        }
        let category = entity.category;
        if !matches!(category, EntityCategory::Unit | EntityCategory::Infantry) {
            return bridge_changed;
        }
        let mission = entity.mission.current().known();
        let depot_dock_state =
            crate::sim::docking::building_dock::depot_owns_enter(sim, Some(rules), id);
        let refinery_dock_miner = !depot_dock_state
            && matches!(
                mission,
                Some(MissionType::Enter) | Some(MissionType::Unload)
            )
            && !matches!(
                entity.passenger_role,
                crate::sim::passenger::PassengerRole::Boarding { .. }
            )
            && sim
                .substrate
                .entities
                .get(id)
                .is_some_and(crate::sim::game_entity::GameEntity::is_harvester);
        // `FootClass::Mission_Move @ 0x004D4200` keeps its cadence while the
        // NavCom is set (`0x004D4203`) or the locomotor's `Is_Moving` holds
        // (`0x004D422A`); the order itself is not an input.
        let moving = entity.navigation.nav_com.is_some()
            || crate::sim::movement::motion_query::is_moving(entity) == Some(true);
        MissionHandlerInput {
            category,
            mission,
            depot_dock_state,
            refinery_dock_miner,
            timer_due: entity.mission.dispatch_timer().due(now),
            moving_or_queued: moving || entity.mission.queued() != MissionId::NONE,
            bunker_delegate: entity.bunker_link.installed_in().is_some(),
            has_attack_target: entity.attack_target.is_some(),
            // The destination slot alone, NOT the wider "is this object in
            // motion" test above: the idle-mode selector branches on exactly
            // that one field.
            has_destination: entity.navigation.nav_com.is_some(),
            effective_mission: entity.mission.effective().known(),
            // Infantry Mission_Attack51F4D3 and Guard shim521320 read
            // the native Doing27..30 family. Undeploy31 is outside it.
            infantry_deployed_do_type: entity.infantry_deploy_doing(),
            // Resolve the type only for the deployed Guard-family arm.
            infantry_deploy_fire_stance: entity.infantry_deploy_doing()
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
        return bridge_changed;
    }

    let mut infantry_guard_handled = false;
    let evaluation = match (input.category, input.mission) {
        // Unit vtable7F5C70+224 -> MissionHarvest73E5E0. No other mission
        // can reach this body or interpret its committed handler cursor.
        (EntityCategory::Unit, Some(MissionType::Harvest)) => {
            let Some(delay) = crate::sim::miner::mission_harvest(
                sim,
                rules,
                ctx.miner_config,
                ctx.overlay_registry,
                id,
            ) else {
                return bridge_changed;
            };
            MissionHandlerEvaluation::cadence(delay)
        }
        // Unit740A90 checks the existing +6E0/+6E1/+6E2 deployment owner
        // before Door/Foot work. Any nonzero byte queues Guard(5,0) and
        // returns1 at740AEF..740B03, without cadence RNG or idle/navigation
        // effects. Native comparisons: anytown_damage/unit_unlimbo.md.
        // Its +6D2 clear is folded into miner::unit_ai_clear_harvesting at
        // the existing UnitAI boundary; no intervening reader observes it.
        (EntityCategory::Unit, Some(MissionType::Move))
            if sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.is_deployed()) =>
        {
            MissionHandlerEvaluation {
                delay: 1,
                clear_stale_attack_target: false,
                queue: Some(MissionType::Guard),
            }
        }
        // `FootClass::Mission_Move` is the native named location for this
        // handler-return cadence; movement execution remains in movement/.
        // **Infantry take a leaf override first, and VERA does not model it.**
        // `InfantryClass`'s Move slot is `+0x22C` = `0x0051F660`, which gates on
        // `[this+0x6C4] ∈ {0x1B, 0x1C, 0x1D, 0x1E}` — the shared
        // Infantry Doing owner, also read by `0x00521320` — and for
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
            //Unit740AB8..740AEC closes its own Techno+350 Door before the
            //shared Foot4D4200 cadence. DeployTime is the same native type
            //reader used by factory and Gate Door requests.
            if input.category == EntityCategory::Unit {
                let ticks = sim
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|entity| sim.object_type(entity.type_ref(), rules))
                    .map_or(0, |object| object.deploy_time_ticks);
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    entity.close_door(ticks, now);
                }
            }
            if input.moving_or_queued {
                MissionHandlerEvaluation::cadence(jittered_mission_cadence(
                    sim,
                    rules,
                    MissionType::Move,
                ))
            } else if input.category == EntityCategory::Unit {
                // Foot4D4242 calls Unit738970(0,1), then returns1. Use the
                // same receiver as Attack, locomotion and refinery/depot exits.
                sim.unit_enter_idle_mode(id, Some(rules), false);
                MissionHandlerEvaluation::cadence(1)
            } else {
                sim.infantry_enter_idle_mode(id, rules, ctx.overlay_registry);
                MissionHandlerEvaluation::cadence(1)
            }
        }
        // Unit does not override Foot Enter4D9290. Depot and refinery
        // admissions use the same body and the unit's one dispatch timer.
        (EntityCategory::Unit, Some(MissionType::Enter))
            if input.depot_dock_state || input.refinery_dock_miner =>
        {
            MissionHandlerEvaluation::cadence(crate::sim::mission::enter::mission_enter(
                sim, rules, id,
            ))
        }
        // `UnitClass::Mission_Attack @ 0x007447A0` is a tail jump to
        // `FootClass::Mission_Attack`, so vehicles belong on this path.
        //
        // Infantry51F3E0 first handles the human-owned Doing27..30 arm
        // below:51F4D3 tests House50B730,51F500 calls51F330 in-place
        // reacquisition, then the plain Rate+Scenario(0,2) epilogue bypasses
        // Foot's half cadence and idle exit. Guard/Sticky/AreaGuard instead
        // enter521320 (native-compared in infantry_deployed_guard.json).
        // Doing is the instance+6C4 action index; Type+6C4 is the separate
        // UndeployDelay. Shared522510 establishes Doing27..30 exactly.
        //
        // RESIDUAL (GSI-07.06): the following preceding C4/infiltration
        // overrides remain outside this deployed/action-clock chain.
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
        //   `Assign_Mission(Capture)` and returns 1. Frequency: every such
        //   dispatch of a computer house's infantryman.
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
        // - Step 2, Foot+68E re-acquisition. Guard4D51C5 and AreaGuard4D7018
        //   produce it for a secondary ElectricAssault warhead and an allied
        //   Overpowerable building (stock SHK/TESLA). Attack4D4E0D clears it
        //   after a non-null pick. The producer, consumer and team-coordinate
        //   reader remain one required Tesla charging chain; none has a Rust
        //   latch owner yet. This is unrelated to tank-bunker containment.
        // Deployed infantry never reach the Foot body — `InfantryClass`'s
        // Attack slot `+0x210` is a real override at `0x0051F3E0` whose
        // deployed arm runs the in-place re-acquire and returns the PLAIN
        // Rate epilogue, with no half-cadence gate.
        (EntityCategory::Infantry, Some(MissionType::Attack))
            if input.infantry_deployed_do_type
                && sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|actor| sim.owner_is_human(actor.owner())) =>
        {
            //51F4D3's House50B730 gate precedes this deployed arm.
            // Order is load-bearing: `0x0051F500` calls `[vtable+0x428]`
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
                queue,
            }
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Attack)) => {
            // MissionAttack4D4E6A calls Approach before its Scenario cadence
            // draw. The accepted destination is visible to this object's
            // subsequent Drive Process, and is retained while it fires.
            if !sim.approach_balloon_target(id, rules, ctx.overlay_registry)
                && sim.owns_unit_cell_approach(id, rules)
            {
                sim.approach_unit_cell_target(id, rules, ctx.overlay_registry)
                    .expect("ordinary Cell approach requires valid live map/navigation state");
            }
            // Foot tests TarCom before calling Approach. Only a null target
            // at entry takes the idle exit; an Approach that clears the target
            // does not retroactively enter this branch. Destruction, detach or
            // Stop also clear the target through their owners, leaving the
            // next due Attack dispatch to queue an idle mission. Firing belongs
            // to the concrete class's separate combat host. Both branches draw
            // cadence jitter; the half-cadence band needs a live target.
            let idle_queue = if input.has_attack_target {
                None
            } else {
                // Foot4D4E72 -> Techno709A54 releases a held Temporal victim
                // before the cadence draw at4D4EA6. Stop only clears TarCom;
                // this due Attack dispatch owns the release.
                if input.category == EntityCategory::Unit {
                    // Unit738970 owns the Foot base, harvester selector,
                    // concrete setters and deferred queue before this draw.
                    sim.unit_enter_idle_mode(id, Some(rules), false);
                    None
                } else {
                    sim.infantry_enter_idle_mode(id, rules, ctx.overlay_registry);
                    None
                }
            };
            let cadence = jittered_mission_cadence(sim, rules, MissionType::Attack);
            let delay = if foot_dispatch_in_cadence_band(sim, rules, id) {
                cadence / 2
            } else {
                cadence
            };
            MissionHandlerEvaluation {
                delay,
                // Stale entity IDs are an authoritative target-loss input, and
                // clearing an invalid handle is not the native selector: the
                // clear lands this dispatch, the idle exit reads the target as
                // it stood at entry and fires on the next one.
                clear_stale_attack_target: input.has_attack_target
                    && attack_target_is_stale(sim, id),
                queue: idle_queue,
            }
        }
        // Undeployed Guard/Sticky51F620 and AreaGuard51F640 share the
        // automatic-deploy branch5214F7 before either Foot continuation.
        (
            EntityCategory::Infantry,
            Some(mission @ (MissionType::Guard | MissionType::Sticky | MissionType::AreaGuard)),
        ) if !input.infantry_deployed_do_type => {
            infantry_guard_handled = true;
            MissionHandlerEvaluation::cadence(infantry_automatic_guard_delay(
                sim, id, rules, mission,
            ))
        }
        // Guard/Sticky51F620 and AreaGuard51F640 first call521320.
        // Its deployed special arms return signed sequence COUNT, without
        // depending on whether the following DoAction request was admitted.
        (
            EntityCategory::Infantry,
            Some(MissionType::Guard | MissionType::Sticky | MissionType::AreaGuard),
        ) if input.infantry_deployed_do_type
            && sim
                .substrate
                .entities
                .get(id)
                .and_then(|actor| sim.object_type(actor.type_ref(), rules))
                .is_some_and(|object| {
                    object.undeploy_delay >= 0 || (object.deploy_fire && object.immune_to_radiation)
                }) =>
        {
            infantry_guard_handled = true;
            let object = sim
                .substrate
                .entities
                .get(id)
                .and_then(|actor| sim.object_type(actor.type_ref(), rules))
                .expect("deployed shim type resolved");
            if object.undeploy_delay >= 0 {
                //521355..52137D: unforced Undeploy31, then raw Count31.
                let _ = sim.infantry_do_action(id, 31, false, rules);
                let count = rules
                    .animation_sequence(&object.id)
                    .and_then(|set| set.infantry_action(31))
                    .map_or(0, |record| record.frames_per_facing);
                MissionHandlerEvaluation::cadence(count)
            } else {
                //52139A..5213EE: native GetCell/MapCell, Cell's site and
                // fixed GetWeapon1. This lookup never substitutes the deck.
                let coord = match sim.foot_navigation_coordinate(id) {
                    Ok(coord) => coord,
                    Err(_) => return bridge_changed,
                };
                let Some(terrain) = sim.resolved_terrain.as_ref() else {
                    return bridge_changed;
                };
                let cell =
                    terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
                let at = terrain.native_cell_coord(cell);
                let target = crate::sim::combat::TargetKind::Cell(at.0 as u16, at.1 as u16);
                let actor = sim.substrate.entities.get(id).expect("deployed shim actor");
                let Some(weapon) = crate::sim::combat::combat_weapon::resolve_weapon_index(
                    rules,
                    actor,
                    object,
                    1,
                    &sim.substrate.entities,
                    &sim.interner,
                ) else {
                    return bridge_changed;
                };
                let below = sim
                    .radiation
                    .site_at((at.0 as u16, at.1 as u16))
                    .is_none_or(|site| {
                        crate::sim::radiation::RadiationState::current_site_level(site)
                            < weapon.weapon.rad_level / 3
                    });
                let mut fired = false;
                if below {
                    //521411 setter precedes521426's fixed-index legality.
                    let _ = sim.assign_target_represented(id, Some(target), Some(rules));
                    let actor = sim.substrate.entities.get(id).expect("deployed shim actor");
                    let actual_target = actor.attack_target.as_ref().map(|attack| attack.target);
                    let error = crate::sim::combat::fire_error_world::FireSubject {
                        world: sim,
                        rules,
                        overlay_registry: ctx.overlay_registry,
                        fog: None,
                        firer: actor,
                        obj: object,
                        target: actual_target,
                        weapon_index: 1,
                    }
                    .fire_error(true);
                    if error == crate::sim::combat::fire_error::FireError::Ok {
                        if let Some(target) = actual_target {
                            //52143D calls51DF60 directly: no Stage equality
                            // or Fire_At_Target action admission runs here.
                            bridge_changed |= sim
                                .commit_fire_visit(
                                    crate::sim::combat::world_receiver::FireVisit::Direct {
                                        id,
                                        target,
                                        weapon_index: 1,
                                    },
                                    rules,
                                    ctx.overlay_registry,
                                )
                                .bridge_state_changed;
                            fired = true;
                        }
                    }
                    //521449/52147E: only the attempted-fire arms clear TarCom.
                    let _ = sim.assign_target_represented(id, None, Some(rules));
                }
                if fired {
                    let _ = sim.infantry_do_action(id, 29, false, rules);
                    let count = rules
                        .animation_sequence(&object.id)
                        .and_then(|set| set.infantry_action(29))
                        .map_or(0, |record| record.frames_per_facing);
                    MissionHandlerEvaluation::cadence(count)
                } else {
                    //521484..5214B8: Rate*900 and one Scenario draw10..20.
                    let rate = mission_cadence(rules, input.mission.unwrap_or(MissionType::Guard));
                    MissionHandlerEvaluation::cadence(
                        rate.wrapping_add(sim.scenario_rng.next_range_u32_inclusive(10, 20) as i32),
                    )
                }
            }
        }
        //5214B9: deployed non-radiation DeployFire reacquires in place and
        // uses the common Rate plus0..2 epilogue.
        (
            EntityCategory::Infantry,
            Some(MissionType::Guard | MissionType::Sticky | MissionType::AreaGuard),
        ) if input.infantry_deployed_do_type && input.infantry_deploy_fire_stance => {
            infantry_guard_handled = true;
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
                queue,
            }
        }
        // Infantry51F6E0 precedes the FootUnload4DA2B0 fallback. The
        // existing class owner performs action/weapon/AssignGuard/NULLNav
        // effects synchronously; its signed return reaches this epilogue.
        (EntityCategory::Infantry, Some(MissionType::Unload)) => {
            let Ok(delay) = sim.infantry_mission_unload(id, rules) else {
                return bridge_changed;
            };
            MissionHandlerEvaluation::cadence(delay)
        }
        // The transport branch of `UnitClass::Mission_Unload @ 0x0073D630`
        // (`Type+0x5E0 Passengers > 0`, gate `0x0073D6EC`). The handler owns
        // its side effects and every return value — the `return 10` / `return
        // 1` early exits and the `[Unload] Rate + RandomRanged(0, 2)`
        // epilogue — so it is committed as a plain cadence here. The refinery
        // (`Harvester=`), `DeploysInto=` and `IsSimpleDeployer=` branches of
        // the same slot are dispatched to their own owners below.
        (EntityCategory::Unit, Some(MissionType::Unload))
            if sim.substrate.entities.get(id).is_some_and(|entity| {
                crate::sim::transport_unload::is_vehicle_transport_type(sim, entity, rules)
            }) =>
        {
            MissionHandlerEvaluation::cadence(crate::sim::transport_unload::unit_mission_unload(
                sim,
                rules,
                id,
                ctx.overlay_registry,
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
                sim,
                id,
                rules,
                ctx.overlay_registry,
            ))
        }
        (EntityCategory::Unit, Some(MissionType::Unload))
            if sim.substrate.entities.get(id).is_some_and(|entity| {
                crate::sim::unit_simple_deploy::is_simple_deployer(sim, entity, rules)
            }) =>
        {
            let Ok(delay) = sim.unit_simple_mission_unload(id, rules) else {
                return bridge_changed;
            };
            MissionHandlerEvaluation::cadence(delay)
        }
        // The harvester branch of `UnitClass::Mission_Unload @ 0x0073D630`
        // (`0x0073D672` → `0x0073DEE0`) for a harvester on its refinery pad.
        (EntityCategory::Unit, Some(MissionType::Unload)) if input.refinery_dock_miner => {
            MissionHandlerEvaluation::cadence(crate::sim::miner::mission_unload(sim, rules, id))
        }
        // Guard and Sticky share the UnitClass slot (`MissionClass::AI`'s
        // table `0x005B34E8` sends both to `+0x21C` = `0x00740810`).
        (EntityCategory::Unit, Some(mission @ (MissionType::Guard | MissionType::Sticky))) => {
            // `UnitClass::Mission_Guard @ 0x00740810` opens with the Slave
            // Miner's kick (`0x00740815..0x0074084F`, `sim::slave_manager`),
            // then the harvester arms and the Construction Yard maker's Unload
            // arm, in that order.
            if let Some(delay) =
                sim.slave_master_mission_kick(id, mission, rules, ctx.overlay_registry)
            {
                MissionHandlerEvaluation::cadence(delay)
            } else if harvester_guard_override_requeues_harvest(sim, id, rules) {
                // `Queue_Mission(10, 0); return 1` at `0x0074092C` /
                // `0x00740960` — no RNG draw, the Foot body is not reached.
                MissionHandlerEvaluation::queue(1, MissionType::Harvest)
            } else if crate::sim::mcv_deploy::guard_queues_unload(sim, id, rules) {
                // `Queue_Mission(0x10, 0)` at `0x00740A19`, then the Guard
                // epilogue `0x00740A1F`.
                MissionHandlerEvaluation::queue(
                    jittered_mission_cadence(sim, rules, mission),
                    MissionType::Unload,
                )
            } else {
                evaluate_foot_guard_cadence(sim, rules, id, mission, input.bunker_delegate)
            }
        }
        (EntityCategory::Infantry, Some(MissionType::Guard)) => {
            evaluate_foot_guard_cadence(sim, rules, id, MissionType::Guard, input.bunker_delegate)
        }
        // `InfantryClass::Mission_Harvest @ 0x00522E70` (Infantry `vt+0x224`),
        // the slave's dig (`sim::slave_manager`).
        (EntityCategory::Infantry, Some(MissionType::Harvest)) => {
            match sim.infantry_mission_harvest(id, rules, ctx.overlay_registry) {
                (delay, true) => MissionHandlerEvaluation::queue(delay, MissionType::Guard),
                (delay, false) => MissionHandlerEvaluation::cadence(delay),
            }
        }
        // Sticky dispatches through the SAME slot as Guard — one handler, two
        // selectors — so it runs the Guard body (a Unit's is the override
        // above). The cadence still comes from the object's own mission slot
        // (the timer lookup indexes on the committed mission id, not on the
        // handler's identity), and `[Sticky] Rate=.016` is 14 frames against
        // Guard's 26. Stock skirmish maps park neutral civilian traffic on
        // this.
        (EntityCategory::Infantry, Some(MissionType::Sticky)) => {
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
        // `UnitClass::Mission_AreaGuard @ 0x00744100` is the Slave Miner's
        // kick (`sim::slave_manager`), whose Rate epilogue draws
        // `RandomRanged(0, 2)`; anything else runs the Foot body. The Foot
        // body's own hunt start (`0x004D6D69..0x004D6D73`, on its guard-area
        // return path) is absent with that path (see
        // `evaluate_foot_area_guard`).
        (EntityCategory::Unit, Some(MissionType::AreaGuard)) => {
            match sim.slave_master_mission_kick(
                id,
                MissionType::AreaGuard,
                rules,
                ctx.overlay_registry,
            ) {
                Some(delay) => MissionHandlerEvaluation::cadence(delay),
                None => evaluate_foot_area_guard(sim, id, rules, ctx),
            }
        }
        (EntityCategory::Infantry, Some(MissionType::AreaGuard)) => {
            evaluate_foot_area_guard(sim, id, rules, ctx)
        }
        // `UnitClass::Mission_Hunt @ 0x0073EFC0`'s deploy arm; every other
        // Unit tail-calls the Foot body (`0x0073F08C`).
        (EntityCategory::Unit, Some(MissionType::Hunt))
            if crate::sim::mcv_deploy::hunt_deploys(sim, id, rules) =>
        {
            MissionHandlerEvaluation::cadence(crate::sim::mcv_deploy::mission_hunt_deploy(
                sim,
                id,
                rules,
                ctx.overlay_registry,
            ))
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Hunt)) => {
            evaluate_foot_hunt(sim, id, rules, ctx)
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Rescue)) => {
            evaluate_foot_rescue(sim, id, rules, ctx)
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
            None => return bridge_changed,
        },
    };

    #[cfg(test)]
    if infantry_guard_handled {
        crate::sim::combat::receiver_fixture::observe_fire_visit(sim, id, "guard-shim-return");
    }

    //51F620/51F640 compare the actual shim return with signed-1. Even
    // a deployed arm's raw Count=-1 (or cadence=-1) resumes its Foot body
    // after the shim's target/action effects, rather than writing timer-1.
    let evaluation = if infantry_guard_handled && evaluation.delay == -1 {
        match input.mission {
            Some(MissionType::AreaGuard) => evaluate_foot_area_guard(sim, id, rules, ctx),
            Some(mission @ (MissionType::Guard | MissionType::Sticky)) => {
                evaluate_foot_guard_cadence(sim, rules, id, mission, input.bunker_delegate)
            }
            _ => unreachable!("Infantry Guard shim belongs to Guard-family slots"),
        }
    } else {
        evaluation
    };

    if evaluation.clear_stale_attack_target
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        // A missing handle is expiry cleanup, not Assign_Target(NULL).
        entity.attack_target = None;
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
    bridge_changed
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MissionHandlerInput {
    pub(super) category: EntityCategory,
    /// The committed selector, or `None` for the idle sentinel. The native
    /// dispatcher's bounds test on the mission id is UNSIGNED, so the sentinel
    /// and every out-of-range id take the switch's default arm rather than
    /// being skipped.
    pub(super) mission: Option<MissionType>,
    /// The object holds a repair-depot contact or independent pending entry: its Enter dispatch is
    /// the shared `mission::enter::mission_enter`.
    pub(super) depot_dock_state: bool,
    /// A harvester on Enter or Unload without a depot contact/entry: its
    /// refinery dock missions (`miner::refinery_dock`).
    pub(super) refinery_dock_miner: bool,
    pub(super) timer_due: bool,
    /// `Mission_Move`'s keep-going test (`0x004D4203..0x004D4238`): a NavCom,
    /// the locomotor's `Is_Moving`, or a queued mission.
    pub(super) moving_or_queued: bool,
    pub(super) bunker_delegate: bool,
    pub(super) has_attack_target: bool,
    /// The destination slot on its own. The idle-mode selector reads this one
    /// field, not the broader in-motion test [`Self::moving_or_queued`] uses.
    pub(super) has_destination: bool,
    /// Current when present, otherwise queued — the selector the idle-mode
    /// early returns and the control-entry lookups read.
    pub(super) effective_mission: Option<MissionType>,
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
    queue: Option<MissionType>,
}

impl MissionHandlerEvaluation {
    const fn cadence(delay: i32) -> Self {
        Self {
            delay,
            clear_stale_attack_target: false,
            queue: None,
        }
    }

    const fn queue(delay: i32, mission: MissionType) -> Self {
        Self {
            delay,
            clear_stale_attack_target: false,
            queue: Some(mission),
        }
    }

    /// The frames the handler returns. The aircraft Guard bodies
    /// (`aircraft_guard`) tail into the Foot ones, which queue nothing and
    /// keep their Target.
    pub(super) const fn delay(&self) -> i32 {
        self.delay
    }
}

/// Undeployed half of Infantry Guard521320, original5214F7..5216B6.
/// The shared class callers treat only signed-1 as the Foot fallback. Native
/// executions and stop-before-latch observations: infantry_auto_deploy.json.
fn infantry_automatic_guard_delay(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    mission: MissionType,
) -> i32 {
    let Some(actor) = sim.substrate.entities.get(id) else {
        return -1;
    };
    let Some(house) = sim.houses.get(&actor.owner()) else {
        return -1;
    };
    let Some(object) = sim.object_type(actor.type_ref(), rules) else {
        return -1;
    };
    if house.is_controlled_by_human(sim.session.game_mode_nonzero)
        || !object.deployer
        || !object.deploy_fire
        || object.undeploy_delay > -1
        || actor.navigation.nav_com.is_some()
    {
        return -1;
    }
    //521570..521574: ADD wraps before the signed CMP/JGE. A subtraction
    // of unsigned elapsed frames changes admission across the sign boundary.
    let deadline = (actor.mission.mission_start_frame() as i32)
        .wrapping_add(house.difficulty_value(&rules.general.ai_auto_deploy_frame_delay));
    if deadline >= sim.session.binary_frame as i32 {
        return -1;
    }
    if let Some(archive) = actor.archive_target() {
        use crate::sim::movement::ground_pose::{object_get_coords, target_get_coords};
        use crate::util::lepton::lepton_to_cell_packed;
        let terrain = sim.resolved_terrain.as_ref();
        let here = object_get_coords(actor, terrain);
        let Some(post) = target_get_coords(
            archive,
            &sim.substrate.entities,
            terrain
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        ) else {
            return -1;
        };
        //521584..5215F3 uses Object virtual+48, truncates each XY/256,
        // then compares the packed signed WORDs. Z is not a gate.
        if lepton_to_cell_packed(here.x) != lepton_to_cell_packed(post.x)
            || lepton_to_cell_packed(here.y) != lepton_to_cell_packed(post.y)
        {
            return -1;
        }
    }
    if object.immune_to_radiation {
        return -1;
    }
    let Some(moving) = crate::sim::movement::motion_query::is_moving(actor) else {
        return -1;
    };
    if !moving {
        //521631..521659 returns raw signed Count27 even when DoAction refuses.
        let _ = sim.infantry_do_action(id, 27, false, rules);
        return rules
            .animation_sequence(&object.id)
            .and_then(|set| set.infantry_action(27))
            .map_or(0, |record| record.frames_per_facing);
    }
    // Stock GI/GGI use Walk. Stop75ADA0 retains a paid head; with no head
    // it synchronously invokes521B40 before this producer writes6E4=1.
    if sim.walk_stop_moving(id, Some(rules)).is_err() {
        return -1;
    }
    sim.substrate
        .entities
        .get_mut(id)
        .expect("Guard Stop retains its Infantry receiver")
        .mission_leaf
        .set_infantry_pending_deploy(1);
    jittered_mission_cadence(sim, rules, mission)
}

/// Scalar native-policy fixtures use the same selector as the concrete idle
/// receivers. Production enters through `Simulation::infantry_enter_idle_mode`
/// or `Simulation::unit_enter_idle_mode`, including the shared Foot base and
/// their destination, timer and radio effects.
#[cfg(test)]
pub(super) fn foot_enter_idle_mode_queue(
    rules: &RuleSet,
    input: MissionHandlerInput,
) -> Option<MissionType> {
    foot_enter_idle_mode_selection(
        rules,
        input.category,
        input.mission,
        input.has_attack_target,
        input.has_destination,
        input.effective_mission,
    )
    .queued_mission()
}

impl Simulation {
    /// Infantry EnterIdle51CBA0 after the sole Foot4D82B0 base.
    /// P2/P5 actual human E1 controls execute constructor-empty planning and
    /// destination history, and stock-false DefaultToGuardArea/GUARD_AREA.
    /// History4DA030, retained AttackMove4DF1C0 and specialized AI/slave
    /// AreaGuard selection remain explicit sibling mechanisms, not defaults
    /// claimed to reproduce those branches.
    pub(crate) fn infantry_enter_idle_mode(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let saved_base_return = self.foot_enter_idle_base(id, Some(rules), registry);
        let Some(entity) = self.substrate.entities.get(id) else {
            return saved_base_return;
        };
        let selection = foot_enter_idle_mode_selection(
            rules,
            entity.category,
            entity.mission.current().known(),
            entity.attack_target.is_some(),
            entity.navigation.nav_com.is_some(),
            entity.mission.effective().known(),
        );
        match selection {
            FootIdleSelection::KeepBaseReturn => {}
            FootIdleSelection::ReturnFalse => return false,
            FootIdleSelection::Queue(mission) => {
                let _ = self.mission_queue_exact(
                    id,
                    MissionId::from_known(mission),
                    0,
                    self.session.binary_frame,
                    &EntityReadyInputProvider,
                );
            }
        }
        saved_base_return
    }
}

/// `Enter_Idle_Mode(0, 1)` (vt+0x484) through the object's class receiver,
/// with deferred Mission assignment: Unit `0x00738970`
/// ([`Simulation::unit_enter_idle_mode`]), Infantry `0x0051CBA0`
/// ([`Simulation::infantry_enter_idle_mode`]), Aircraft `0x004176F0`
/// ([`crate::sim::aircraft::enter_idle_mode_for`]) and Building `0x0044D6A0`
/// ([`Simulation::building_enter_idle_mode`]). Unlimbo's `(1, 1)` is
/// [`foot_unlimbo_idle_mode`].
pub(crate) fn enter_idle_mode(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) {
    match sim.substrate.entities.get(id).map(|entity| entity.category) {
        Some(EntityCategory::Unit) => {
            sim.unit_enter_idle_mode(id, Some(rules), false);
        }
        Some(EntityCategory::Infantry) => {
            sim.infantry_enter_idle_mode(id, rules, registry);
        }
        Some(EntityCategory::Aircraft) => {
            crate::sim::aircraft::enter_idle_mode_for(sim, id, rules, registry);
        }
        Some(EntityCategory::Structure) => {
            sim.building_enter_idle_mode(id, false, Some(rules));
        }
        None => {}
    }
}

/// `TechnoClass::Unlimbo @ 0x006F6E2A..0x006F6E4F` for a Foot:
/// `Enter_Idle_Mode(1, 1)` (`InfantryClass::Enter_Idle_Mode @ 0x0051CBA0`,
/// `UnitClass::Enter_Idle_Mode @ 0x00738970`, `AircraftClass::
/// Enter_Idle_Mode @ 0x004176F0`), then Ready_To_Commence and Commence, so
/// the mission it picks is current at once.
/// A fresh object with nowhere to go takes Guard; a map placement then assigns
/// its authored mission over it. A factory-built vehicle used to keep no
/// mission at all, so its dispatch took the missionless 450-frame arm instead
/// of Mission_Guard's cadence.
///
/// RESIDUALS, beside those on [`foot_enter_idle_mode_selection`]:
/// - the Area Guard arm (a computer object outside a team, or with a slave
///   link, whose house IQ reaches `[IQ] GuardArea=`, or a
///   `DefaultToGuardArea=`/GUARD_AREA type) is committed as Guard: VERA's
///   `Mission_AreaGuard` lacks the guard post, the leash and the approach.
///   Trigger: every computer-built infantryman and vehicle, and every dog.
///   Effect: they hold their ground instead of covering an area.
/// - a Harvester or Weeder vehicle takes its own arm (`0x00738BD8`,
///   `Simulation::unit_enter_idle_mode`).
/// - an unarmed vehicle's Unload arm (`0x00738A7C..0x00738AAF`: `+0x3D4`,
///   `Passengers=` and cargo, outside a team) is not taken; it gets Guard.
///   Trigger: an unarmed transport leaving the factory loaded. Effect: it
///   keeps its cargo aboard.
/// - buildings have their own Unlimbo/mission owner.
pub(crate) fn foot_unlimbo_idle_mode(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.category == EntityCategory::Aircraft {
        crate::sim::aircraft::enter_idle_mode_for(sim, id, rules, registry);
        sim.mission_host_promote(id, sim.session.binary_frame, rules);
        return;
    }
    let vehicle = entity.category == EntityCategory::Unit;
    if !vehicle && entity.category != EntityCategory::Infantry {
        return;
    }
    if vehicle {
        sim.unit_enter_idle_mode(id, Some(rules), true);
        sim.mission_host_promote(id, sim.session.binary_frame, rules);
        return;
    }
    sim.infantry_enter_idle_mode(id, rules, registry);
    sim.mission_host_promote(id, sim.session.binary_frame, rules);
}

/// One ordered selector shared by the live class receivers and scalar
/// Mission fixtures. Native Infantry51CBCE..51CD9D; Unit's armed Guard
/// branch uses the same mission-control/committed-selector policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FootIdleSelection {
    KeepBaseReturn,
    ReturnFalse,
    Queue(MissionType),
}

impl FootIdleSelection {
    pub(crate) fn queued_mission(self) -> Option<MissionType> {
        match self {
            Self::Queue(mission) => Some(mission),
            _ => None,
        }
    }
}

pub(crate) fn foot_enter_idle_mode_selection(
    rules: &RuleSet,
    category: EntityCategory,
    mission: Option<MissionType>,
    has_attack_target: bool,
    has_destination: bool,
    effective_mission: Option<MissionType>,
) -> FootIdleSelection {
    let infantry = category == EntityCategory::Infantry;
    if effective_mission == Some(MissionType::Deliberate) {
        return FootIdleSelection::KeepBaseReturn;
    }
    let selection = if infantry && has_attack_target || has_destination {
        if infantry
            && matches!(
                effective_mission,
                Some(MissionType::Capture | MissionType::Sabotage)
            )
        {
            effective_mission.expect("matched known effective mission")
        } else if infantry && has_attack_target {
            MissionType::Attack
        } else {
            MissionType::Move
        }
    } else {
        // The destination/target gates precede these early returns. The
        // committed mission indexes native5B3A00; effective only admits it.
        let frozen = effective_mission.is_some()
            && mission.is_some_and(|mission| {
                rules
                    .mission_control
                    .entry(mission)
                    .is_some_and(|entry| entry.zombie || entry.paralyzed)
            });
        if matches!(
            effective_mission,
            Some(MissionType::Guard | MissionType::AreaGuard)
        ) || frozen
        {
            return if infantry {
                FootIdleSelection::ReturnFalse
            } else {
                FootIdleSelection::KeepBaseReturn
            };
        }
        // Human E1's original stock data/constructor takes Guard. AI IQ,
        // GUARD_AREA/DefaultToGuardArea and slave selection remain recorded
        // sibling mechanisms; this branch does not certify those inputs.
        MissionType::Guard
    };
    let committed_blocks = matches!(mission, Some(MissionType::Patrol | MissionType::AreaGuard))
        || category == EntityCategory::Unit
            && matches!(mission, Some(MissionType::Unload | MissionType::Eaten));
    if committed_blocks {
        FootIdleSelection::KeepBaseReturn
    } else {
        FootIdleSelection::Queue(selection)
    }
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
/// NavCom / `c4_plant` goal state and its own movement issue, not by
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
/// gated on `!HouseClass::IsControlledByHuman(this->Owner)` and
/// `g_GameMode (0x00A8B238) == 0`, a campaign computer house; skirmish runs
/// with a nonzero game mode, so it is dormant there. The human arm's
/// `UpdateIdleAction` now uses its one Infantry receiver synchronously, before
/// the cadence draw in that human no-target arm.
///
/// RESIDUAL — **`InfantryClass::Mission_Hunt @ 0x0051F540`** (slot `+0x228`,
/// single DATA xref `0x007EB280`; `0x007EB280 - 0x007EB058 = 0x228`), the
/// override above this body for infantry. Both of its arms open with
/// `HouseClass::IsControlledByHuman(...) == 0`: a computer house's
/// `Infiltrate`/`Occupier`/`Assaulter` infantryman holding a building
/// `Assign_Mission(Capture)`s and returns 1, and its deployed man with no
/// target plays `Do_Action(0x1F)` and returns 1 — **both consume no RNG at
/// all**. Trigger: a computer house's infantry on Hunt. Player effect: such an
/// infantryman scans and fights where retail captures or undeploys.
/// Frequency: every Hunt dispatch of those infantry. Downstream risk: one RNG
/// draw per dispatch that retail does not make.

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
        // and the idle / return-to-base arm (already covered elsewhere). The
        // call itself is the mission: it is what installs the target the
        // pursuit pass then closes on.
        let acquired = super::target_scan::scan(
            sim,
            id,
            rules,
            // `PUSH 0x0` at `0x004D5373` — the literal threat mask Hunt hands
            // the scanner.
            crate::sim::combat::ScanMission::Hunt,
            ctx,
            None,
        );
        // `0x004D54DD`: a Unit approaches what the scan holds; the balloon
        // arm takes only a Unit.
        if acquired {
            sim.approach_balloon_target(id, rules, ctx.overlay_registry);
        }
    }
    //4D54EE..4D5576: the human no-target arm invokes the class idle
    //receiver before the mission-rate jitter. Campaign AI return-to-base
    //still belongs to its documented House/mission continuation below.
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.is_none() && sim.owner_is_human(entity.owner()))
    {
        let _ = sim.infantry_idle_action(id, rules);
    }
    MissionHandlerEvaluation::cadence(jittered_mission_cadence(sim, rules, MissionType::Hunt))
}

/// Archive the raw physical Map565730 Cell receiver, never Foot's HeadTo.
/// Real map cells retain their canonical coordinates. The tagged Cell adapter
/// does not yet preserve a retained Dummy pointer across subsequent stamps.
fn foot_physical_cell_target(sim: &Simulation, id: u64) -> Option<crate::sim::combat::TargetKind> {
    let actor = sim.substrate.entities.get(id)?;
    let coord = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
    let cell = sim.resolved_terrain.as_ref().map_or(
        ((coord.x / 256) as i16, (coord.y / 256) as i16),
        |terrain| {
            let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            terrain.native_cell_coord(query.lookup_world(coord.x, coord.y))
        },
    );
    Some(crate::sim::combat::TargetKind::Cell(
        cell.0 as u16,
        cell.1 as u16,
    ))
}

fn foot_threat_range(sim: &Simulation, id: u64, rules: &RuleSet) -> i32 {
    let Some(actor) = sim.substrate.entities.get(id) else {
        return 0;
    };
    let Some(object) = sim.object_type(actor.type_ref(), rules) else {
        return 0;
    };
    let ranges = if object.guard_range.is_none_or(|range| range.to_bits() == 0) {
        std::array::from_fn(|index| {
            crate::sim::combat::combat_weapon::weapon_range(
                actor,
                object,
                index as i32,
                &sim.substrate.entities,
                rules,
                &sim.interner,
            )
        })
    } else {
        [0, 0]
    };
    crate::sim::combat::threat_range::threat_range_leptons(object, 1, ranges)
}

fn scaled_area_guard_leash(range: i32) -> i32 {
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
    //4D6E55..4D6E5F multiplies whole leptons by double7E9258 (1.1),
    //then signed ftol. Native boundary vectors pin the resulting move order.
    X87::ftol_i32_low_masked(X87::mul(
        X87::load_i32(range),
        X87::load_f64(NativeF64Bits::from_bits(0x3ff1_9999_9999_999a)),
    ))
}

fn foot_distance_to_target(
    sim: &Simulation,
    id: u64,
    target: crate::sim::combat::TargetKind,
    rules: &RuleSet,
) -> Option<i32> {
    let coords = |target| {
        crate::sim::movement::ground_pose::target_get_coords(
            target,
            &sim.substrate.entities,
            sim.resolved_terrain
                .as_ref()
                .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                .as_ref(),
        )
    };
    let from = coords(crate::sim::combat::TargetKind::Entity(id))?;
    let to = coords(target)?;
    let foundation = match target {
        crate::sim::combat::TargetKind::Entity(id) => sim
            .substrate
            .entities
            .get(id)
            .filter(|entity| entity.category == EntityCategory::Structure)
            .and_then(|entity| sim.object_type(entity.type_ref(), rules))
            .map(|object| {
                let (width, height) =
                    crate::rules::foundation::foundation_dimensions(&object.foundation);
                (i32::from(width), i32::from(height))
            }),
        crate::sim::combat::TargetKind::Cell(..) => None,
    };
    Some(crate::util::native_x87::object_distance(
        [from.x, from.y, from.z],
        [to.x, to.y, to.z],
        foundation,
    ))
}

/// `FootClass::Mission_Rescue @ 0x004DDF90`, ordinary Unit/Infantry.
/// Its direct Greatest_Threat uses the archive's+48 XYZ, no passive timer or
/// health debit. Admission is strict distance < mode1 range*1.5. Empty results
/// clear688, set status1, use the House500200 home owner, assign a destination,
/// then clear the archive before the current-mission cadence draw. Status1
/// without NavCom queues AreaGuard and calls Commence directly.
/// Native execution: foot_missions Rescue rows and house_base_return.
/// The target-present Approach_Target4D5690 dependency is still required;
/// its sole existing Rust port belongs to PR#798 and must be integrated there.
fn evaluate_foot_rescue(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    let Some(actor) = sim.substrate.entities.get(id) else {
        return MissionHandlerEvaluation::cadence(1);
    };
    let state = actor.mission.handler_state();
    if state == 1 && actor.navigation.nav_com.is_none() {
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .set_archive_target(None);
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::AreaGuard),
            0,
            sim.session.binary_frame,
            &EntityReadyInputProvider,
        );
        let _ = sim.mission_commence_exact(id, sim.session.binary_frame);
    } else if state == 0 && actor.attack_target.is_none() {
        if actor.archive_target().is_none() {
            let post = foot_physical_cell_target(sim, id);
            sim.substrate
                .entities
                .get_mut(id)
                .expect("Rescue receiver")
                .set_archive_target(post);
        }
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .clear_rescue_retarget_latch();
        let archive = sim
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.archive_target());
        let coords = |sim: &Simulation, target| {
            crate::sim::movement::ground_pose::target_get_coords(
                target,
                &sim.substrate.entities,
                sim.resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )
            .map(|coord| [coord.x, coord.y, coord.z])
        };
        let point = archive.and_then(|post| coords(sim, post));
        let pick = point.and_then(|point| {
            sim.greatest_threat_represented(
                rules,
                ctx.overlay_registry,
                id,
                crate::sim::combat::ScanMission::Hunt,
                Some(point),
                crate::sim::combat::combat_targeting::greatest_threat_for_entity,
            )
        });
        if let (Some(point), Some(pick)) = (point, pick)
            && let Some(target) = coords(sim, crate::sim::combat::TargetKind::Entity(pick))
        {
            //1.5 is exact for the native integer range, so this signed
            //scaled comparison preserves its strict FCOMP result without
            //rounding a fractional threshold or discounting foundations.
            let distance = crate::util::native_x87::object_distance(point, target, None);
            if i64::from(distance) * 2 < i64::from(foot_threat_range(sim, id, rules)) * 3 {
                let _ = sim.assign_target_represented(
                    id,
                    Some(crate::sim::combat::TargetKind::Entity(pick)),
                    Some(rules),
                );
            }
        }
        if sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| actor.attack_target.is_some())
        {
            return MissionHandlerEvaluation::cadence(1);
        }
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .mission
            .set_handler_state(1);
        let destination = sim
            .house_return_cell(id)
            .map(|(rx, ry)| crate::sim::components::NavTargetRef::cell(rx, ry));
        let _ =
            sim.assign_destination_represented(id, destination, Some(rules), ctx.overlay_registry);
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .set_archive_target(None);
    } else if state == 0 {
        // `0x004DDFDB..0x004DDFEB`: with a Target, approach it.
        sim.approach_balloon_target(id, rules, ctx.overlay_registry);
    }
    let mission = sim
        .substrate
        .entities
        .get(id)
        .and_then(|actor| actor.mission.current().known())
        .unwrap_or(MissionType::Rescue);
    MissionHandlerEvaluation::cadence(jittered_mission_cadence(sim, rules, mission))
}

/// Smallest value of the Area Guard cadence jitter draw (`RandomRanged(1, 5)`).
/// Every other absorbed handler draws `(0, 2)`; this one does not.
const AREA_GUARD_CADENCE_JITTER_MIN: u32 = 1;
/// Largest value of the Area Guard cadence jitter draw (`RandomRanged(1, 5)`).
const AREA_GUARD_CADENCE_JITTER_MAX: u32 = 5;

/// `FootClass::Mission_AreaGuard @ 0x004D6AA0`, ordinary MTNK/E1 path.
/// The archive owns the guard post. Cell+48 uses ground height even for a
/// deck-standing actor; an archived Foot instead supplies its live world XYZ
/// and selects General.GuardModeStray. A crossed leash clears Target before
/// assigning that SAME archive through the concrete destination owner.
/// Retaliate_And_Scan receives the post XYZ, and synchronous Infantry idle
/// work precedes the cadence draw. Native execution: foot_missions ordinary
/// post/leash, scan-point and retail idle controls.
///
/// Required larger chains: target-present Approach_Target4D5690 (sole port in
/// PR#798), Tesla ElectricAssault adjacency4D6F44 (+68E, Overpowerable), the
/// AI C4/Sabotage arm4D6DDA and the early containment/waypoint/harvester arms.
/// They retain their existing boundaries; these controls do not certify them.
pub(super) fn evaluate_foot_area_guard(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    let needs_post = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.archive_target().is_none() && entity.mission.queued() == MissionId::NONE
    });
    if needs_post {
        let post = foot_physical_cell_target(sim, id);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.set_archive_target(post);
        }
    }
    let archive = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.archive_target());
    let mut leash = scaled_area_guard_leash(foot_threat_range(sim, id, rules));
    if let Some(crate::sim::combat::TargetKind::Entity(post)) = archive
        && sim.substrate.entities.get(post).is_some_and(|entity| {
            matches!(
                entity.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        })
    {
        //4D6E74 tests AbstractFlags bit2, not the target's display layer.
        leash = rules.general.guard_mode_stray;
    }
    let may_return = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.navigation.nav_com.is_none() && entity.mission_leaf.foot_firing_sequence_latch() == 0
    });
    if let Some(post) = archive
        && may_return
        && foot_distance_to_target(sim, id, post, rules).is_some_and(|distance| distance > leash)
    {
        //4D6EB8 precedes4D6ECB, even when the class target setter refuses.
        let _ = sim.assign_target_represented(id, None, Some(rules));
        let destination = match post {
            crate::sim::combat::TargetKind::Cell(rx, ry) => {
                crate::sim::components::NavTargetRef::cell(rx, ry)
            }
            crate::sim::combat::TargetKind::Entity(id) => {
                crate::sim::components::NavTargetRef::Entity { id }
            }
        };
        let _ = sim.assign_destination_represented(
            id,
            Some(destination),
            Some(rules),
            ctx.overlay_registry,
        );
    }
    let needs_target = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.is_none());
    // `0x004D6ED1..0x004D6F32`: with a Target the body approaches it instead
    // of scanning; without a post it does neither (`0x004D6E66`).
    if !needs_target && archive.is_some() {
        sim.approach_balloon_target(id, rules, ctx.overlay_registry);
    }
    let timer_due = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.passive_scan_timer.due(sim.session.binary_frame));
    if needs_target && can_acquire_target(sim, id, rules) && timer_due {
        let point = archive
            .and_then(|post| {
                crate::sim::movement::ground_pose::target_get_coords(
                    post,
                    &sim.substrate.entities,
                    sim.resolved_terrain
                        .as_ref()
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                        .as_ref(),
                )
            })
            .map(|coord| [coord.x, coord.y, coord.z]);
        //4D6EF5..4D6F06 uses archive+48. A queued mission can leave no
        // archive; that invalid native pointer is outside this ordinary lane.
        if point.is_some()
            && super::target_scan::scan(
                sim,
                id,
                rules,
                crate::sim::combat::ScanMission::AreaGuard,
                ctx,
                point,
            )
        {
            return MissionHandlerEvaluation::cadence(1);
        }
    }
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.is_none())
    {
        let _ = sim.infantry_idle_action(id, rules);
    }
    let base = mission_cadence(rules, MissionType::AreaGuard);
    // `0x004D7040..0x004D7048`: an aircraft (What_Am_I 2) doubles the Rate
    // before the draw.
    let base = if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.category == EntityCategory::Aircraft)
    {
        base.wrapping_add(base)
    } else {
        base
    };
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
    // Target-present Approach_Target remains required.
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
/// undeploy on Guard. Its deployed reacquire, nonnegative UndeployDelay and
/// immune self-fire arms run in [`dispatch_foot_mission`].
/// Their actual -1 sentinel reaches this shared Foot continuation afterward.
///
/// Its undeployed producer runs in [`infantry_automatic_guard_delay`], with
/// the same signed-1 continuation after any action or Stop callback effects.
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
///   VERA's stands still. `+0x390` is HoverAttack: stock Rocketeer and Siege
///   Chopper use it. Frequency: grounded engagement on those types. Downstream risk:
///   a destination write from the mission handler crosses into movement's
///   ownership, so closing it needs the movement owner in the loop.
/// - **the AI-only Sabotage queue** (0x004D523A-0x004D52A9): for an
///   `InfantryClass` (`What_Am_I()` 0xF) whose house is not human-controlled
///   (0x0050B730 on `[this+0x21C]`), that either carries type byte `+0xEC2` or
///   passes `TechnoClass::HasWeaponAbility(0xE)` 0x0070D0D0, whose current
///   mission is not already Sabotage, and whose target is a `BuildingClass`,
///   `Queue_Mission(0x11, 0)`. Trigger: an AI demolition infantryman standing
///   guard that picks up an enemy building. Player effect: retail's walks in and
///   plants; VERA's shoots it instead. Frequency: every Guard dispatch of such
///   a computer-owned infantryman with a building target.
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
/// - **Tesla charging adjacency** (0x004D5116-0x004D51D2; AreaGuard4D6F44).
///   A secondary weapon's ElectricAssault warhead (`+0x158`)
///   admits an adjacent allied Overpowerable building (`+0x1575`, reader460029).
///   The handler assigns that building, sets Foot+68E, then queues Attack.
///   Stock SHK/TESLA exercise this when a trooper stands beside its coil.
///   Charging and the Attack/team consumers of +68E remain required; using
///   bunker_link here would introduce the wrong state owner and effect.
/// - **the second cadence-tail short-circuit ahead of the jitter draw**
///   (past the rearm return). `GetTechnoType()->[+0x6B0]` — the **`DistributedFire`**
///   bool, key string 0x00843A64, read by `TechnoTypeClass::ReadINI` at
///   0x00714850/0x00714864 — combined with the object counter `+0x468 > 0`
///   returns **0**, re-dispatching on the next frame and again drawing nothing.
///   Trigger: a `DistributedFire` type on Guard with a live spread-fire count.
///   Player effect: it re-evaluates every frame instead of every ~26. Frequency:
///   the Aegis Cruiser is the only stock `DistributedFire` type, so naval maps
///   with an Allied player only. Downstream risk: VERA implements no
///   distributed-fire mechanism at all (recorded at `target_scan`), so
///   the counter this gate reads has no VERA counterpart to bind to.
pub(super) fn evaluate_foot_guard_cadence(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    mission: MissionType,
    bunker_delegate: bool,
) -> MissionHandlerEvaluation {
    if bunker_delegate {
        return MissionHandlerEvaluation::cadence(mission_cadence(rules, mission));
    }
    //4D51A6 /4D51D8 precede even the rearm-timer early return. Unit's
    //receiver is a false stub; Infantry's owner decides actual admission.
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.is_none())
    {
        let _ = sim.infantry_idle_action(id, rules);
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
///   native-true by the Harvest scan miss and never cleared; the count is
///   the house's tracked BuildingType count (`+0x5500`, `0x007408A2`,
///   `HouseTracking::owns_any_building`).
/// - (iii) **human house && `Teleporter=`** (`UnitType+0xCD4`): walk the 8
///   neighbour cells (`MapCoord_StepByDir_GetCell`); a building there whose
///   type has `+0x16BB` (`Refinery=`) and whose owner `+0x21C` is this house
///   → queue Harvest, return 1. Else `Get_Storage_Percentage() == 1.0` and
///   the locomotor's `Is_Moving` (ILocomotion slot `+0x10`, `0x007409D6`)
///   true → queue Harvest, return 1. `TeleportLocomotionClass::Is_Moving`
///   (`0x00718080`) is `byte +0x30 == 1`, true only for the Relocate tick,
///   never "a teleport state exists"; a Drive that a move installed answers
///   its own `Is_Moving`. A human WAR miner has no arm here: once on Guard it
///   stays until a player order.
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
    if let Some(house) = sim
        .houses
        .get(&entity.owner())
        .filter(|house| !house.is_controlled_by_human(sim.session.game_mode_nonzero))
    {
        // Arm (ii), `0x00740880..0x0074092C`.
        return house.tracking.owns_any_building(
            unit_type
                .dock
                .iter()
                .filter_map(|name| sim.interner.get(name)),
        ) && !house.harvester_no_ore;
    }
    if !unit_type.teleporter {
        return false;
    }
    let (rx, ry) = (i32::from(entity.position.rx), i32::from(entity.position.ry));
    for (dx, dy) in CELL_DELTAS {
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
    // The active locomotor's `Is_Moving`; on Teleport the Relocate tick only.
    miner.is_full() && crate::sim::movement::motion_query::is_moving(entity) == Some(true)
}

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
    rules.mission_control.rate_frames(mission)
}

#[inline]
fn jittered_mission_cadence(sim: &mut Simulation, rules: &RuleSet, mission: MissionType) -> i32 {
    sim.mission_rate_epilogue(rules, mission)
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
/// component differences; the length is
/// [`sqrt_approx_length`](crate::util::native_x87::sqrt_approx_length).
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
    // `FADDP` adds dy*dy (ST0) into dx*dx (ST1).
    let len = crate::util::native_x87::sqrt_approx_length([dx, dy]);
    (CADENCE_BAND_MIN_LEPTONS..=CADENCE_BAND_MAX_LEPTONS).contains(&i64::from(len))
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
    let Some(primary) = object.primary().and_then(|name| rules.weapon(name)) else {
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
/// Native SelectWeapon(NULL) and CanFireAt/InRange use their existing owners;
/// the retained target does not pass through GetFireError (ammo/rearm/cloak).
/// Range-boundary executions: infantry_auto_deploy.json.
fn infantry_deployed_attack_reacquire(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    input: MissionHandlerInput,
    ctx: super::ObjectAiCtx<'_>,
) -> Option<MissionType> {
    let had_target = input.has_attack_target;
    let weapon_index = super::target_scan::select_weapon(sim, rules, id, None);
    let target = sim
        .substrate
        .entities
        .get(id)
        .and_then(|actor| actor.attack_target.as_ref().map(|attack| attack.target));
    if target.is_some_and(|target| {
        super::target_scan::can_fire_at(sim, rules, id, target, weapon_index, ctx.overlay_registry)
    }) {
        return None;
    }
    // The raw scan, NOT `Retaliate_And_Scan`: that routine also stamps the
    // scan frame and re-arms the acquisition cadence with its own
    // `RandomRanged(0, 2)` draw, and `0x0051F330` calls `Greatest_Threat`
    // directly through `[vtable+0x3C4]` without either.
    let pick = sim.greatest_threat_represented(
        rules,
        ctx.overlay_registry,
        id,
        crate::sim::combat::ScanMission::Guard,
        None,
        crate::sim::combat::combat_targeting::greatest_threat_for_entity,
    );
    // `0x0051F38A..0x0051F39D`: Assign_Target(pick) when a target is held or
    // one was found. It writes no `+0x50C`, so the setter leaves the flag
    // clear.
    if had_target || pick.is_some() {
        let _ = sim.assign_target_represented(
            id,
            pick.map(crate::sim::combat::TargetKind::Entity),
            Some(rules),
        );
    }
    if pick.is_some() {
        return None;
    }
    // `decompile_function 0x0051F330`: the idle exit is `if (this->[0xAC] != 5
    // && GetTechnoType()[0xD94] == 0) Enter_Idle_Mode(0, 1)` — skipped when the
    // COMMITTED mission is Guard. That was vacuous while the Attack handler was
    // the only caller; the deploy shim `FUN_00521320` also reaches this virtual
    // from Guard, Sticky and Area Guard, so the gate is now live.
    // Type+D94 is JumpJet: ctor710B00/711601 defaults false and the
    // exact-case reader7151E5..715200 retains that default. E1/GGI are false.
    if input.mission == Some(MissionType::Guard)
        || sim
            .substrate
            .entities
            .get(id)
            .and_then(|actor| sim.object_type(actor.type_ref(), rules))
            .is_some_and(|object| object.jumpjet)
    {
        return None;
    }
    sim.infantry_enter_idle_mode(id, rules, ctx.overlay_registry);
    None
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
             [GAREFN]\nFoundation=4x3\nRefinery=yes\nDockUnload=yes\n",
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
        // The class constructor's `Add_Tracking`, which a direct insert skips.
        sim.update_house_tracking(
            REFINERY_ID,
            crate::sim::house_tracking::HouseTracking::add_tracking,
        );
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
        dispatch_foot_mission(
            sim,
            MINER_ID,
            rules,
            super::super::ObjectAiCtx {
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
            .install_teleport_state_for_test(Some(TeleportState::for_test(phase, 14, 11, 30)));
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

    /// The same test asks the active locomotor's `Is_Moving` (ILocomotion
    /// +0x10), not Is_Moving_Now: a full Chrono miner driving on the Drive
    /// installed over its Teleport, with a destination and no speed yet, is
    /// moving (Drive `0x004AFB80`), so it requeues Harvest.
    #[test]
    fn full_chrono_miner_on_its_drive_asks_is_moving_not_is_moving_now() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_refinery(&mut sim, "Americans");
        spawn_guard_miner(&mut sim, MinerKind::Chrono, (40, 40));
        fill_cargo(&mut sim);
        let entity = sim.substrate.entities.get_mut(MINER_ID).expect("miner");
        assert!(crate::sim::movement::locomotor_owner::begin_drive_for_teleporter(entity, 0));
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state();
        entity.locomotor.as_mut().unwrap().store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Drive,
            Some(crate::sim::components::DriveCoord::cell(45, 40, 0)),
        );
        entity
            .foot_speed
            .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);

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
mod move_arrival_tests {
    //! `FootClass::Mission_Move @ 0x004D4200`: with no NavCom, a still
    //! locomotor and nothing queued, the unit arrives (`Enter_Idle_Mode` at
    //! `0x004D4242`, Guard queued, return 1); otherwise it keeps the cadence.
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::locomotor::LocomotorState;

    const UNIT_ID: u64 = 1;

    fn rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n1=JJV\n[AircraftTypes]\n\
             [BuildingTypes]\n[MTNK]\nSpeed=6\n\
             [JJV]\nSpeed=14\nLocomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n",
        ))
        .expect("move arrival rules")
    }

    /// A Unit of `type_name` on Move with a due timer, no NavCom and no order.
    fn spawn_on_move(sim: &mut Simulation, type_name: &str, kind: LocomotorKind) {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_name);
        let mut ge = GameEntity::new_at_frame_zero_for_test(
            UNIT_ID,
            20,
            20,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 300 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        ge.locomotor = Some(LocomotorState::for_test_kind(kind));
        sim.substrate.entities.insert(ge);
        sim.mission_assign_exact(UNIT_ID, MissionId::from_known(MissionType::Move), 0)
            .expect("assign Move");
    }

    fn dispatch(sim: &mut Simulation, rules: &RuleSet) -> Option<MissionType> {
        dispatch_foot_mission(
            sim,
            UNIT_ID,
            rules,
            super::super::ObjectAiCtx {
                overlay_registry: None,
                terrain_spawner_cells: None,
                miner_config: None,
            },
        );
        sim.substrate
            .entities
            .get(UNIT_ID)
            .expect("unit")
            .mission
            .queued()
            .known()
    }

    /// The order is not an input: a tank whose Drive is still and whose
    /// NavCom is null arrives even while VERA still holds its order.
    #[test]
    fn an_order_alone_does_not_keep_a_unit_on_move() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_on_move(&mut sim, "MTNK", LocomotorKind::Drive);
        sim.substrate
            .entities
            .get_mut(UNIT_ID)
            .expect("unit")
            .movement_target = Some(crate::sim::components::MovementTarget::default());

        assert_eq!(dispatch(&mut sim, &rules), Some(MissionType::Guard));
    }

    /// A Jumpjet that a stop sent down to the floor has no NavCom but its
    /// moving byte (`Is_Moving` `0x0054AE50`) holds until it lands, so it keeps
    /// the Move cadence.
    #[test]
    fn a_landing_jumpjet_keeps_its_move_cadence() {
        let rules = rules();
        let mut sim = Simulation::new();
        spawn_on_move(&mut sim, "JJV", LocomotorKind::Jumpjet);
        let runtime = sim
            .substrate
            .entities
            .get_mut(UNIT_ID)
            .and_then(|e| e.locomotor.as_mut())
            .and_then(|loco| loco.jumpjet_runtime_mut())
            .expect("Jumpjet runtime");
        *runtime = runtime.clone().with_moving_for_test(true);

        assert_eq!(dispatch(&mut sim, &rules), None);
        let entity = sim.substrate.entities.get(UNIT_ID).expect("unit");
        assert!(
            entity.mission.dispatch_timer().delay() > 1,
            "the Move cadence, not the arrival's return 1"
        );
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
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/rearm_timer.json",
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
