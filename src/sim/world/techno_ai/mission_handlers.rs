//! Foot/Unit mission handlers and their ordered target/navigation effects.
//!
//! The object-AI host and its ordering remain in the parent module; this module
//! owns only handler inputs, results, and the single timer epilogue.

use super::{Simulation, can_acquire_target};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::authority::{LiveReadyInputProvider, rate_epilogue};
use crate::sim::mission::{MISSION_STUB_FRAMES, MissionId, MissionType};
use crate::sim::world::FrameEffects;
use crate::util::direction::DIRECTION_DELTAS;

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
#[path = "infantry_move_oracle_tests.rs"]
mod infantry_move_oracle_tests;

#[cfg(test)]
#[path = "../infantry_fire_oracle_tests.rs"]
mod infantry_fire_oracle_tests;

#[cfg(test)]
#[path = "hover_attack_tests.rs"]
mod hover_attack_tests;

#[cfg(test)]
#[path = "guard_sabotage_tests.rs"]
mod guard_sabotage_tests;

#[cfg(test)]
#[path = "hunt_idle_tests.rs"]
mod hunt_idle_tests;

#[cfg(test)]
#[path = "campaign_hunt_oracle_tests.rs"]
mod campaign_hunt_oracle_tests;

#[cfg(test)]
#[path = "area_guard_post_tests.rs"]
mod area_guard_post_tests;

/// Whether [`dispatch_foot_mission`] runs `id`'s handler at `now`: a Unit or
/// Infantry that passes `MissionClass::AI`'s gate
/// ([`GameEntity::mission_handler_due`]; harvest_attack_return.json
/// dead_source / zero_health_source pin direct receiver calls as well as the
/// object-AI host).
///
/// [`GameEntity::mission_handler_due`]: crate::sim::game_entity::GameEntity::mission_handler_due
fn foot_mission_handler_due(sim: &Simulation, id: u64, now: u32) -> bool {
    sim.substrate.entities.get(id).is_some_and(|entity| {
        matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry
        ) && entity.mission_handler_due(now)
    })
}

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
    let now = sim.session.binary_frame();
    if !foot_mission_handler_due(sim, id, now) {
        return bridge_changed;
    }
    let input = {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return bridge_changed;
        };
        let category = entity.category();
        let mission = entity.mission.current().known();
        let depot_dock_state = crate::sim::docking::building_dock::depot_owns_enter(sim, rules, id);
        let refinery_dock_miner = !depot_dock_state
            && matches!(
                mission,
                Some(MissionType::Enter) | Some(MissionType::Unload)
            )
            && sim
                .substrate
                .entities
                .get(id)
                .is_some_and(crate::sim::game_entity::GameEntity::is_harvester);
        // `FootClass::Mission_Move @ 0x004D4200` keeps its cadence while the
        // NavCom is set (`0x004D4203`) or the locomotor's `Is_Moving` holds
        // (`0x004D422A`); the order itself is not an input.
        let moving = entity.navigation.nav_com().is_some()
            || crate::sim::movement::motion_query::is_moving(entity) == Some(true);
        MissionHandlerInput {
            category,
            mission,
            refinery_dock_miner,
            moving_or_queued: moving || entity.mission.queued() != MissionId::NONE,
            has_attack_target: entity.attack_target.target().is_some(),
            // The destination slot alone, NOT the wider "is this object in
            // motion" test above: the idle-mode selector branches on exactly
            // that one field.
            #[cfg(any(test, feature = "test-support"))]
            has_destination: entity.navigation.nav_com().is_some(),
            #[cfg(any(test, feature = "test-support"))]
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

    // `UnitClass::Mission_Unload @ 0x0073D630` opens by emptying the tank
    // bunker of a linked Unit (`0x0073D63B..0x0073D66D`), then resumes past its
    // deploy-fire and transport arms (`0x0073D672`).
    let bunker_unload = input.category == EntityCategory::Unit
        && input.mission == Some(MissionType::Unload)
        && crate::sim::docking::bunker_link::unit_unload_empties_bunker(
            sim,
            id,
            rules,
            ctx.effects,
        );
    let mut infantry_guard_handled = false;
    let evaluation = match (input.category, input.mission) {
        // Unit vtable+24C -> Mission_Repair740EF0; infantry keeps the base
        // stub. Map placements can assign Repair through Unit ReadINI743270.
        (EntityCategory::Unit, Some(MissionType::Repair)) => {
            MissionHandlerEvaluation::cadence(sim.unit_mission_repair(id, rules, ctx.effects))
        }
        // Unit vtable7F5C70+224 -> MissionHarvest73E5E0. No other mission
        // can reach this body or interpret its committed handler cursor.
        (EntityCategory::Unit, Some(MissionType::Harvest)) => {
            let Some(delay) = crate::sim::miner::mission_harvest(
                sim,
                rules,
                ctx.overlay_registry,
                id,
                ctx.effects,
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
                queue: Some(MissionType::Guard),
            }
        }
        // Infantry51F660's Doing27..30 wrapper precedes Foot4D4200.
        // Human51F68F calls the original class NULL setter, which refuses
        // while deployed (51AA49..51AA7E); NavCom therefore survives. The
        // handler still returns1 without the Foot cadence draw. Native
        // controls: infantry_deploy_action --mission-move --check.
        (EntityCategory::Infantry, Some(MissionType::Move))
            if input.infantry_deployed_do_type
                && sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|actor| sim.owner_is_human(actor.owner())) =>
        {
            sim.set_infantry_null_destination(id, rules, ctx.overlay_registry, ctx.effects);
            MissionHandlerEvaluation::cadence(1)
        }
        // AI51F6A4: UndeployDelay<=-1 requests unforced Undeploy31. Read
        // raw signed Count31 AFTER the action, even when it refused; doing
        // Deploy27 cannot be interrupted and a missing record returns0.
        // The existing action/sequencer owns Stage, sound and immunity
        // cleanup. Other delays/Doing inherit the Foot body below.
        (EntityCategory::Infantry, Some(MissionType::Move))
            if input.infantry_deployed_do_type
                && sim
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|actor| sim.object_type(actor.type_ref(), rules))
                    .is_some_and(|object| object.undeploy_delay < 0) =>
        {
            let _ = sim.infantry_do_action(
                id,
                crate::sim::movement::infantry_action::DO_UNDEPLOY,
                false,
                rules,
                ctx.effects,
            );
            let count = sim
                .substrate
                .entities
                .get(id)
                .and_then(|actor| sim.object_type(actor.type_ref(), rules))
                .and_then(|object| rules.animation_sequence(&object.id))
                .and_then(|set| set.infantry_action(31))
                .map_or(0, |record| record.frames_per_facing);
            MissionHandlerEvaluation::cadence(count)
        }
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
                MissionHandlerEvaluation::cadence(rate_epilogue(
                    rules,
                    MissionType::Move,
                    &mut sim.scenario_rng,
                ))
            } else if input.category == EntityCategory::Unit {
                // Foot4D4242 calls Unit738970(0,1), then returns1. Use the
                // same receiver as Attack, locomotion and refinery/depot exits.
                sim.unit_enter_idle_mode(id, rules, false, ctx.effects);
                MissionHandlerEvaluation::cadence(1)
            } else {
                sim.infantry_enter_idle_mode(id, rules, ctx.overlay_registry, ctx.effects);
                MissionHandlerEvaluation::cadence(1)
            }
        }
        // Unit does not override Foot Enter4D9290, and Infantry inherit it
        // too (InfantryClass vt+0x240 is `0x004D9290`). Depot, refinery,
        // transport, absorber and tank-bunker entries use the same body
        // and the one dispatch timer.
        (EntityCategory::Unit, Some(MissionType::Enter)) => {
            MissionHandlerEvaluation::cadence(crate::sim::mission::enter::mission_enter(
                sim,
                rules,
                ctx.overlay_registry,
                id,
                ctx.effects,
            ))
        }
        (EntityCategory::Infantry, Some(MissionType::Enter)) => {
            MissionHandlerEvaluation::cadence(crate::sim::mission::enter::mission_enter(
                sim,
                rules,
                ctx.overlay_registry,
                id,
                ctx.effects,
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
        // `InfantryClass::Mission_Attack @ 0x0051F3E0` opens with its C4 arm,
        // for every house.
        (EntityCategory::Infantry, Some(MissionType::Attack))
            if c4_sabotage_building(sim, id, rules).is_some() =>
        {
            infantry_attack_c4(sim, id, rules, ctx)
        }
        // `InfantryClass::Mission_Attack @ 0x0051F3E0` and `Mission_Hunt @
        // 0x0051F540` share their computer capture arm.
        (EntityCategory::Infantry, Some(MissionType::Attack | MissionType::Hunt))
            if computer_capture_building(sim, id, rules).is_some() =>
        {
            infantry_computer_capture(sim, id, rules, ctx)
        }
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
            let delay = rate_epilogue(rules, MissionType::Attack, &mut sim.scenario_rng);
            MissionHandlerEvaluation { delay, queue }
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Attack)) => {
            hover_attack_reanchor(sim, rules, id, ctx);
            // Foot4D4E0D..4D4E53 asks Greatest_Threat(mask1) while charging,
            // without passive scan gates or timer writes. A failed scan keeps
            // its coil; a successful scan clears the flag even on setter refusal.
            if sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.tesla_charger.is_active())
                && let Some(target) = super::target_scan::direct_greatest_threat(
                    sim,
                    rules,
                    ctx.overlay_registry,
                    id,
                    crate::sim::combat::ScanMission::Guard,
                )
            {
                let _ = sim.assign_target_represented(
                    id,
                    Some(crate::sim::combat::TargetKind::Entity(target)),
                    rules,
                    ctx.overlay_registry,
                    ctx.effects,
                );
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .tesla_charger
                    .clear();
            }
            let has_attack_target = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.attack_target.target().is_some());
            // MissionAttack4D4E6A calls Approach before its Scenario cadence
            // draw. The accepted destination is visible to this object's
            // subsequent Drive Process, and is retained while it fires.
            sim.approach_target(id, rules, ctx.overlay_registry, ctx.effects);
            // Foot tests TarCom before calling Approach. Only a null target
            // at entry takes the idle exit; an Approach that clears the target
            // does not retroactively enter this branch. Destruction, detach or
            // Stop also clear the target through their owners, leaving the
            // next due Attack dispatch to queue an idle mission. Firing belongs
            // to the concrete class's separate combat host. Both branches draw
            // cadence jitter; the half-cadence band needs a live target.
            let idle_queue = if has_attack_target {
                None
            } else {
                // Foot4D4E72 -> Techno709A54 releases a held Temporal victim
                // before the cadence draw at4D4EA6. Stop only clears TarCom;
                // this due Attack dispatch owns the release.
                if input.category == EntityCategory::Unit {
                    // Unit738970 owns the Foot base, harvester selector,
                    // concrete setters and deferred queue before this draw.
                    sim.unit_enter_idle_mode(id, rules, false, ctx.effects);
                    None
                } else {
                    sim.infantry_enter_idle_mode(id, rules, ctx.overlay_registry, ctx.effects);
                    None
                }
            };
            let cadence = rate_epilogue(rules, MissionType::Attack, &mut sim.scenario_rng);
            let delay = if foot_dispatch_in_cadence_band(sim, rules, id) {
                cadence / 2
            } else {
                cadence
            };
            MissionHandlerEvaluation {
                delay,
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
                sim,
                id,
                rules,
                mission,
                ctx.effects,
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
                let _ = sim.infantry_do_action(id, 31, false, rules, ctx.effects);
                let count = rules
                    .animation_sequence(&object.id)
                    .and_then(|set| set.infantry_action(31))
                    .map_or(0, |record| record.frames_per_facing);
                MissionHandlerEvaluation::cadence(count)
            } else {
                //52139A..5213EE: native GetCell/MapCell, Cell's site and
                // fixed GetWeapon1. This lookup never substitutes the deck.
                let Some(at) = sim.own_map_cell(id) else {
                    return bridge_changed;
                };
                let target = crate::sim::combat::TargetKind::Cell(at.0, at.1);
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
                let below = sim.radiation.site_at(at).is_none_or(|site| {
                    crate::sim::radiation::RadiationState::current_site_level(site)
                        < weapon.weapon.rad_level / 3
                });
                let mut fired = false;
                if below {
                    //521411 setter precedes521426's fixed-index legality.
                    let _ = sim.assign_target_represented(
                        id,
                        Some(target),
                        rules,
                        ctx.overlay_registry,
                        ctx.effects,
                    );
                    let actor = sim.substrate.entities.get(id).expect("deployed shim actor");
                    let actual_target = actor.attack_target.target();
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
                    if error == crate::sim::combat::fire_error::FireError::Ok
                        && let Some(target) = actual_target
                    {
                        //52143D calls51DF60 directly: no Stage equality
                        // or Fire_At_Target action admission runs here.
                        bridge_changed |= sim
                            .commit_fire_visit(
                                crate::sim::combat::world_fire::FireVisit::Direct {
                                    id,
                                    target,
                                    weapon_index: 1,
                                },
                                rules,
                                ctx.overlay_registry,
                                ctx.effects,
                            )
                            .bridge_state_changed;
                        fired = true;
                    }
                    //521449/52147E: only the attempted-fire arms clear TarCom.
                    let _ = sim.assign_target_represented(
                        id,
                        None,
                        rules,
                        ctx.overlay_registry,
                        ctx.effects,
                    );
                }
                if fired {
                    let _ = sim.infantry_do_action(id, 29, false, rules, ctx.effects);
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
            let delay = rate_epilogue(
                rules,
                input.mission.unwrap_or(MissionType::Guard),
                &mut sim.scenario_rng,
            );
            MissionHandlerEvaluation { delay, queue }
        }
        // Infantry51F6E0 precedes the FootUnload4DA2B0 fallback. The
        // existing class owner performs action/weapon/AssignGuard/NULLNav
        // effects synchronously; its signed return reaches this epilogue.
        (EntityCategory::Infantry, Some(MissionType::Unload)) => {
            let Ok(delay) = sim.infantry_mission_unload(id, rules, ctx.effects) else {
                return bridge_changed;
            };
            MissionHandlerEvaluation::cadence(delay)
        }
        // `UnitClass::Mission_Unload @ 0x0073D630` opens with its deploy-fire
        // arm, which returns its own delay.
        (EntityCategory::Unit, Some(MissionType::Unload))
            if !bunker_unload
                && sim.substrate.entities.get(id).is_some_and(|entity| {
                    crate::sim::deploy::unit_fires_on_deploy(sim, entity, rules)
                }) =>
        {
            let (delay, changed) =
                sim.unit_deploy_fire_unload(id, rules, ctx.overlay_registry, ctx.effects);
            bridge_changed |= changed;
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
            if !bunker_unload
                && sim.substrate.entities.get(id).is_some_and(|entity| {
                    crate::sim::transport_unload::is_vehicle_transport_type(sim, entity, rules)
                }) =>
        {
            MissionHandlerEvaluation::cadence(crate::sim::transport_unload::unit_mission_unload(
                sim,
                rules,
                id,
                ctx.overlay_registry,
                ctx.effects,
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
                ctx.effects,
            ))
        }
        (EntityCategory::Unit, Some(MissionType::Unload))
            if sim.substrate.entities.get(id).is_some_and(|entity| {
                crate::sim::unit_simple_deploy::is_simple_deployer(sim, entity, rules)
            }) =>
        {
            let Ok(delay) = sim.unit_simple_mission_unload(id, rules, ctx.effects) else {
                return bridge_changed;
            };
            MissionHandlerEvaluation::cadence(delay)
        }
        // The harvester branch of `UnitClass::Mission_Unload @ 0x0073D630`
        // (`0x0073D672` → `0x0073DEE0`) for a harvester on its refinery pad.
        (EntityCategory::Unit, Some(MissionType::Unload)) if input.refinery_dock_miner => {
            MissionHandlerEvaluation::cadence(crate::sim::miner::mission_unload(
                sim,
                rules,
                id,
                ctx.effects,
            ))
        }
        // A bunkered Unit with no harvester, DeploysInto= or IsSimpleDeployer=
        // arm (every retail `Bunkerable=` type) ends in FootClass::Mission_Unload
        // (`0x0073DED3`, `0x004DA2B0` -> the `0x005B2EF0` stub). Its
        // EmptyBunker has already queued Move.
        (EntityCategory::Unit, Some(MissionType::Unload)) if bunker_unload => {
            MissionHandlerEvaluation::cadence(crate::sim::mission::MISSION_STUB_FRAMES)
        }
        // Guard and Sticky share the UnitClass slot (`MissionClass::AI`'s
        // table `0x005B34E8` sends both to `+0x21C` = `0x00740810`).
        (EntityCategory::Unit, Some(mission @ (MissionType::Guard | MissionType::Sticky))) => {
            // `UnitClass::Mission_Guard @ 0x00740810` opens with the Slave
            // Miner's kick (`0x00740815..0x0074084F`, `sim::slave_manager`),
            // then the harvester arms and the Construction Yard maker's Unload
            // arm, in that order.
            if let Some(delay) =
                sim.slave_master_mission_kick(id, mission, rules, ctx.overlay_registry, ctx.effects)
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
                    rate_epilogue(rules, mission, &mut sim.scenario_rng),
                    MissionType::Unload,
                )
            } else {
                evaluate_foot_guard_cadence(sim, rules, id, mission, ctx)
            }
        }
        (EntityCategory::Infantry, Some(MissionType::Guard)) => {
            evaluate_foot_guard_cadence(sim, rules, id, MissionType::Guard, ctx)
        }
        // `InfantryClass::Mission_Harvest @ 0x00522E70` (Infantry `vt+0x224`),
        // the slave's dig (`sim::slave_manager`).
        (EntityCategory::Infantry, Some(MissionType::Harvest)) => {
            match sim.infantry_mission_harvest(id, rules, ctx.overlay_registry, ctx.effects) {
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
            evaluate_foot_guard_cadence(sim, rules, id, MissionType::Sticky, ctx)
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
        // `RandomRanged(0, 2)`; anything else runs the Foot body, which
        // starts an idle manager's hunt itself (`evaluate_foot_area_guard`).
        (EntityCategory::Unit, Some(MissionType::AreaGuard)) => {
            match sim.slave_master_mission_kick(
                id,
                MissionType::AreaGuard,
                rules,
                ctx.overlay_registry,
                ctx.effects,
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
                ctx.effects,
            ))
        }
        // `InfantryClass::Mission_Hunt`'s second arm (`0x0051F5C0..0x0051F609`):
        // a computer house's deployed infantryman (Doing `0x1B..0x1E`) with no
        // Target undeploys (Do_Action(0x1F, 0, 0)) and returns 1, drawing
        // nothing. Any other infantryman takes the Foot body (`0x0051F60C`).
        // A stale Target is one the native detach has already cleared.
        (EntityCategory::Infantry, Some(MissionType::Hunt))
            if input.infantry_deployed_do_type
                && (!input.has_attack_target || attack_target_is_stale(sim, id))
                && sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|actor| !sim.owner_is_human(actor.owner())) =>
        {
            if let Err(cause) = sim.infantry_do_action(
                id,
                crate::sim::movement::infantry_action::DO_UNDEPLOY,
                false,
                rules,
                ctx.effects,
            ) {
                log::debug!("infantry {id} Hunt undeploy: {cause}");
            }
            MissionHandlerEvaluation {
                delay: 1,
                queue: None,
            }
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Hunt)) => {
            evaluate_foot_hunt(sim, id, rules, ctx)
        }
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Rescue)) => {
            evaluate_foot_rescue(sim, id, rules, ctx)
        }
        // Foot Mission_Eaten4D4CB0, dispatched through vt+218.
        (EntityCategory::Unit | EntityCategory::Infantry, Some(MissionType::Eaten)) => {
            MissionHandlerEvaluation::cadence(crate::sim::mission::eaten::mission_eaten(
                sim,
                rules,
                ctx.overlay_registry,
                id,
                ctx.effects,
            ))
        }
        // `FootClass::Mission_Capture @ 0x004D4B20` (vt+0x214), which neither
        // class overrides. Sabotage dispatches to the same slot (jump table
        // `0x005B34E8`, entries 8 and 17).
        (
            EntityCategory::Unit | EntityCategory::Infantry,
            Some(MissionType::Capture | MissionType::Sabotage),
        ) => MissionHandlerEvaluation::cadence(crate::sim::mission::capture::mission_capture(
            sim,
            rules,
            ctx.overlay_registry,
            id,
            ctx.effects,
        )),
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
                evaluate_foot_guard_cadence(sim, rules, id, mission, ctx)
            }
            _ => unreachable!("Infantry Guard shim belongs to Guard-family slots"),
        }
    } else {
        evaluation
    };

    if let Some(queued_mission) = evaluation.queue {
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(queued_mission),
            0,
            now,
            &LiveReadyInputProvider { rules },
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
    /// A harvester on Enter or Unload without a depot contact/entry: its
    /// refinery dock missions (`miner::refinery_dock`).
    pub(super) refinery_dock_miner: bool,
    /// `Mission_Move`'s keep-going test (`0x004D4203..0x004D4238`): a NavCom,
    /// the locomotor's `Is_Moving`, or a queued mission.
    pub(super) moving_or_queued: bool,
    pub(super) has_attack_target: bool,
    /// The destination slot on its own. The idle-mode selector reads this one
    /// field, not the broader in-motion test [`Self::moving_or_queued`] uses.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) has_destination: bool,
    /// Current when present, otherwise queued — the selector the idle-mode
    /// early returns and the control-entry lookups read.
    #[cfg(any(test, feature = "test-support"))]
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
    queue: Option<MissionType>,
}

impl MissionHandlerEvaluation {
    const fn cadence(delay: i32) -> Self {
        Self { delay, queue: None }
    }

    const fn queue(delay: i32, mission: MissionType) -> Self {
        Self {
            delay,
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
    effects: FrameEffects<'_>,
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
    if house.is_controlled_by_human(sim.session.game_mode_nonzero())
        || !object.deployer
        || !object.deploy_fire
        || object.undeploy_delay > -1
        || actor.navigation.nav_com().is_some()
    {
        return -1;
    }
    //521570..521574: ADD wraps before the signed CMP/JGE. A subtraction
    // of unsigned elapsed frames changes admission across the sign boundary.
    let deadline = (actor.mission.mission_start_frame() as i32)
        .wrapping_add(house.difficulty_value(&rules.general.ai_auto_deploy_frame_delay));
    if deadline >= sim.session.binary_frame() as i32 {
        return -1;
    }
    if let Some(archive) = actor.archive_target() {
        use crate::sim::movement::ground_pose::{object_get_coords, target_get_coords};
        use crate::util::lepton::lepton_to_cell_packed;
        let terrain = sim.resolved_terrain.as_ref();
        let here = object_get_coords(actor, terrain);
        let Some(post) = target_get_coords(
            archive,
            sim,
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
        let _ = sim.infantry_do_action(id, 27, false, rules, effects);
        return rules
            .animation_sequence(&object.id)
            .and_then(|set| set.infantry_action(27))
            .map_or(0, |record| record.frames_per_facing);
    }
    // Stock GI/GGI use Walk. Stop75ADA0 retains a paid head; with no head
    // it synchronously invokes521B40 before this producer writes6E4=1.
    if sim.walk_stop_moving(id, rules, effects).is_err() {
        return -1;
    }
    sim.substrate
        .entities
        .get_mut(id)
        .expect("Guard Stop retains its Infantry receiver")
        .mission_leaf
        .set_infantry_pending_deploy(1);
    rate_epilogue(rules, mission, &mut sim.scenario_rng)
}

/// Scalar native-policy fixtures use the same selector as the concrete idle
/// receivers. Production enters through `Simulation::infantry_enter_idle_mode`
/// or `Simulation::unit_enter_idle_mode`, including the shared Foot base and
/// their destination, timer and radio effects.
#[cfg(any(test, feature = "test-support"))]
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
        MissionType::Guard,
    )
    .queued_mission()
}

/// `InfantryClass::Enter_Idle_Mode`'s guard pick (`0x0051CCB9..0x0051CD75`),
/// reached with no NavCom or TarCom and a mission the early returns pass:
/// - an owner a computer controls (`0x0050B730`) and no team (`+0x5D4`):
///   Guard below `[IQ] GuardArea=` (owner `+0x24C`, `Rules+0x1440`) or as a
///   slave (`+0x2DC`); otherwise Area Guard when armed (vt+0x2AC),
///   `Engineer=` (`+0xEC3`) or `VehicleThief=` (`+0xEC6`), else Guard.
/// - a human's, or a teamed infantryman: Area Guard outside a team with the
///   veteran GUARD_AREA ability (`0x0070D0D0`, 0x10) or
///   `DefaultToGuardArea=` (`+0xD39`), else Guard.
///
/// Both arms also test for a current Area Guard (`+0xAC`), whose final gate
/// (`0x0051CD85`) queues nothing either way, so the pick omits it. A missing
/// House counts as human.
fn infantry_idle_guard_pick(sim: &Simulation, id: u64, rules: &RuleSet) -> MissionType {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return MissionType::Guard;
    };
    let house = sim.houses.get(&entity.owner());
    let human =
        house.is_none_or(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero()));
    let teamed = sim.team_script_vm.team_for_member(id).is_some();
    let object = sim.object_type(entity.type_ref(), rules);
    let area_guard = if !human && !teamed {
        house.is_some_and(|house| house.current_iq() >= rules.general.iq_guard_area)
            && entity.slave.owner().is_none()
            && object.is_some_and(|object| {
                crate::sim::combat::combat_weapon::is_armed(entity, object)
                    || object.engineer
                    || object.vehicle_thief
            })
    } else {
        !teamed
            && object.is_some_and(|object| {
                crate::sim::combat::veterancy::has_weapon_ability(
                    entity.veterancy.rank_kind(),
                    object,
                    crate::rules::object_type::Ability::GuardArea,
                ) || object.default_to_guard_area
            })
    };
    if area_guard {
        MissionType::AreaGuard
    } else {
        MissionType::Guard
    }
}

impl Simulation {
    /// Infantry EnterIdle51CBA0 after the sole Foot4D82B0 base.
    /// P2/P5 actual human E1 controls execute constructor-empty planning and
    /// stock-false DefaultToGuardArea/GUARD_AREA; the guard pick is
    /// [`infantry_idle_guard_pick`]. A resumed attack-move
    /// (HaveMegaMission, vt+0x4AC at `0x0051CBBC`) returns the base's answer
    /// before the selector. Without a Wait mission or a TarCom, the
    /// navigation list may send it on first (Handle_Navigation_List,
    /// `0x0051CC21`), before the selector reads NavCom.
    pub(crate) fn infantry_enter_idle_mode(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let saved_base_return = self.foot_enter_idle_base(id, rules, registry, effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return saved_base_return;
        };
        if entity.mega_mission.is_attack_move() {
            return saved_base_return;
        }
        if entity.mission.effective().known() != Some(MissionType::Deliberate)
            && entity.attack_target.target().is_none()
        {
            self.handle_navigation_list(id, rules, registry, effects);
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return saved_base_return;
        };
        let selection = foot_enter_idle_mode_selection(
            rules,
            entity.category(),
            entity.mission.current().known(),
            entity.attack_target.target().is_some(),
            entity.navigation.nav_com().is_some(),
            entity.mission.effective().known(),
            infantry_idle_guard_pick(self, id, rules),
        );
        match selection {
            FootIdleSelection::KeepBaseReturn => {}
            FootIdleSelection::ReturnFalse => return false,
            FootIdleSelection::Queue(mission) => {
                let _ = self.mission_queue_exact(
                    id,
                    MissionId::from_known(mission),
                    0,
                    self.session.binary_frame(),
                    &LiveReadyInputProvider { rules },
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
/// [`techno_unlimbo_idle_mode`].
pub(crate) fn enter_idle_mode(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    match sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.category())
    {
        Some(EntityCategory::Unit) => {
            sim.unit_enter_idle_mode(id, rules, false, effects);
        }
        Some(EntityCategory::Infantry) => {
            sim.infantry_enter_idle_mode(id, rules, registry, effects);
        }
        Some(EntityCategory::Aircraft) => {
            crate::sim::aircraft::enter_idle_mode_for(sim, id, rules, registry, effects);
        }
        Some(EntityCategory::Structure) => {
            sim.building_enter_idle_mode(id, false, rules);
        }
        None => {}
    }
}

/// `TechnoClass::Unlimbo @ 0x006F6E2A..0x006F6E4F`: `Enter_Idle_Mode(1, 1)`
/// (`InfantryClass::Enter_Idle_Mode @ 0x0051CBA0`, `UnitClass::
/// Enter_Idle_Mode @ 0x00738970`, `AircraftClass::Enter_Idle_Mode @
/// 0x004176F0`, `BuildingClass::Enter_Idle_Mode @ 0x0044D6A0`), then
/// Ready_To_Commence and Commence, so the mission it picks is current at once
/// when the object is ready.
/// A fresh object with nowhere to go takes Guard; a map placement then assigns
/// its authored mission over it. A factory-built vehicle used to keep no
/// mission at all, so its dispatch took the missionless 450-frame arm instead
/// of Mission_Guard's cadence. A placed building queues Construction; its
/// ready byte (`+0x6DD`, `0x00454250`) is clear unless its cell-put discovery
/// opened it, so Construction stays queued: a computer yard's exit commences
/// it (`0x00445329..0x0044533F`), and otherwise a later Update's ready check
/// does.
///
/// RESIDUALS, beside those on [`foot_enter_idle_mode_selection`]:
/// - a Harvester or Weeder vehicle takes its own arm (`0x00738BD8`,
///   `Simulation::unit_enter_idle_mode`).
pub(crate) fn techno_unlimbo_idle_mode(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    match sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.category())
    {
        Some(EntityCategory::Aircraft) => {
            crate::sim::aircraft::enter_idle_mode_for(sim, id, rules, registry, effects);
        }
        Some(EntityCategory::Unit) => {
            sim.unit_enter_idle_mode(id, rules, true, effects);
        }
        Some(EntityCategory::Infantry) => {
            sim.infantry_enter_idle_mode(id, rules, registry, effects);
        }
        Some(EntityCategory::Structure) => {
            sim.building_enter_idle_mode(id, true, rules);
        }
        None => return,
    }
    sim.mission_host_promote(id, sim.session.binary_frame(), rules);
}

/// One ordered selector shared by the live class receivers and scalar
/// Mission fixtures. Native Infantry51CBCE..51CD9D; Unit's armed Guard
/// branch uses the same mission-control/committed-selector policy. Past
/// the early returns, `guard_pick` is the class's pick.
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
    guard_pick: MissionType,
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
        // The class's own guard pick: Infantry's [`infantry_idle_guard_pick`],
        // the Unit armed branch's `unit_idle_guard_pick`.
        guard_pick
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
/// 3. **The infantry arms**, taken only by an infantryman (`What_Am_I` 0xF,
///    `0x004D53A0..0x004D53B4`) whose scan came back with a target: the
///    engineer's, the C4 one, the VehicleThief's and the no-flag arm are
///    [`hunt_infantry_arm`].
/// 4. **No target**: an object whose owner is controlled by a human, or any
///    object when `g_GameMode` is nonzero (every non-campaign match), runs
///    the idle-action virtual (`vt+0x478`, `0x004D557C`); a campaign
///    computer's hunter marches on the player's base (described below).
/// 5. **The tail** at `0x004D5582`, reached from every arm:
///    `ftol(MissionControl[current].Rate * 900.0) + RandomRanged(0, 2)`;
///    `0x005B3A00` indexes the current mission, Capture or Sabotage once a
///    type arm has commenced it.
///
/// So the normal path draws the scenario RNG **twice** — once inside the
/// scanner, once in the tail — and the `StupidHunt` path draws once. An earlier
/// note on this arm said "BOTH exits draw `RandomRanged(0, 2)`", counting one
/// draw; that was the tail only.
///
/// The approach itself is Approach (`vt+0x53C`, whose Foot body is
/// `FootClass::Approach_Target @ 0x004D5690`), which the handler calls for a
/// non-infantry object at `0x004D54E3` and for an infantryman with none of the
/// three type flags at `0x004D54D2`. That routine is the "drive to somewhere I
/// can shoot my target from" search and ends in `Set_Destination`
/// (`vt+0x480`); VERA runs it ([`Simulation::approach_target`]) for every
/// mover the Foot body represents.
///
/// The campaign computer's march on the player
/// (`0x004D5506`-`0x004D5576`), taken when `!IsControlledByHuman(Owner)` and
/// `g_GameMode (0x00A8B238) == 0`; every non-campaign match has a nonzero
/// game mode, so it is dormant there. It asks the player's House
/// (`[0x00A83D4C]`) for its base or starting cell
/// (`HouseClass::Get_Base_Or_Starting_Cell @ 0x0050DEF0`); unless that is the
/// empty cell, a Slave Miner abandons its hunt
/// (`SlaveManagerClass::AbandonHunt @ 0x006B0C80`), then `Set_Destination`
/// (`vt+0x480`) takes that cell and `vt+0x1E8(2, 1)` queues Move. The
/// destination and queue with immediate readiness check use their class
/// receivers before the cadence draw. The other no-target arm's
/// `UpdateIdleAction` uses its one Infantry receiver synchronously.
///
/// Infantry reach this body past `InfantryClass::Mission_Hunt @ 0x0051F540`
/// (slot `+0x228`, single DATA xref `0x007EB280`), whose two computer arms
/// are their own dispatch arms: the capture arm
/// ([`infantry_computer_capture`]) and the deployed man's undeploy.
fn evaluate_foot_hunt(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    let stupid_hunt = !hunt_scans(sim, id, rules);
    // 4D5367 and 4D539A: StupidHunt, or a scan that finds nothing, take the
    // no-target arm at 4D54EE; every other arm jumps to the tail at 4D5582.
    let mut no_target_arm = stupid_hunt;
    if !stupid_hunt {
        // The return value selects between the approach and infantry arms
        // and the idle / return-to-base arm (already covered elsewhere). The
        // call itself is the mission: it is what installs the target the
        // approach then closes on.
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
        let infantry = sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Infantry);
        if acquired && infantry {
            hunt_infantry_arm(sim, id, rules, ctx);
        } else if acquired {
            // `0x004D54DD..0x004D54E3`: any other mover approaches what the
            // scan holds.
            sim.approach_target(id, rules, ctx.overlay_registry, ctx.effects);
        }
        no_target_arm = !acquired;
    }
    // 0x004D54EE..0x004D557C: IsControlledByHuman (0x0050B730) or a nonzero
    // g_GameMode (0x00A8B238) takes the class idle receiver before the
    // mission-rate jitter, so every owner in a non-campaign match idles. Only
    // a campaign computer house reaches the march on the player.
    if no_target_arm {
        if sim.session.game_mode_nonzero()
            || sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| sim.owner_is_human(entity.owner()))
        {
            let _ = sim.infantry_idle_action(id, rules, ctx.overlay_registry, ctx.effects);
        } else {
            hunt_march_on_player(sim, id, rules, ctx);
        }
    }
    MissionHandlerEvaluation::cadence(Simulation::mission_rate_epilogue_for(
        &mut sim.scenario_rng,
        &sim.substrate,
        rules,
        id,
        MissionType::Hunt,
    ))
}

/// Foot4D5506..5576: a campaign hunter with no target heads for the current
/// player's base. House50DEF0 owns alternate/primary selection; Map5657A0
/// resolves the returned cell before the class destination setter runs.
fn hunt_march_on_player(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) {
    let Some(cell) = sim
        .session
        .current_house()
        .and_then(|owner| sim.houses.get(&owner))
        .map(|house| house.base_origin())
        .filter(|cell| *cell != (0, 0))
    else {
        return;
    };
    sim.reset_slave_manager(id, rules, ctx.effects);
    let (rx, ry) = sim.resolved_terrain.as_ref().map_or(cell, |terrain| {
        let identity = terrain.native_cell_identity((cell.0 as i16, cell.1 as i16));
        let (x, y) = terrain.native_cell_coord(identity);
        (x as u16, y as u16)
    });
    let _ = sim.assign_destination_represented(
        id,
        Some(crate::sim::components::NavTargetRef::cell(rx, ry)),
        rules,
        ctx.overlay_registry,
        ctx.effects,
    );
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Move),
        1,
        sim.session.binary_frame(),
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    );
}

/// Whether Mission_Hunt scans (`0x004D5367`): its type is not `StupidHunt=`.
fn hunt_scans(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    !sim.substrate
        .entities
        .get(id)
        .and_then(|entity| sim.interner.try_resolve(entity.type_ref()))
        .and_then(|name| rules.object(name))
        .is_some_and(|obj| obj.stupid_hunt)
}

/// Whether `id`'s Hunt handler ([`evaluate_foot_hunt`]) reaches
/// Greatest_Threat this frame as the world stands, for the batched target
/// scans (`techno_ai::scan_batch`): its handler is due, it holds no target
/// (Retaliate_And_Scan scans only then), its type scans ([`hunt_scans`]),
/// it does not take the Unit deploy arm (`mcv_deploy::hunt_deploys`), and it
/// is not a deployed infantryman: a computer house's undeploys, and a human
/// house's, which scans, is left out with it. A held target the drop step
/// would drop, and the computer capture arm, are not foreseen; those objects
/// scan live, if at all.
pub(super) fn foot_hunt_scans_ahead(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    entity.mission.current().known() == Some(MissionType::Hunt)
        && entity.attack_target.target().is_none()
        && !entity.infantry_deploy_doing()
        && foot_mission_handler_due(sim, id, sim.session.binary_frame())
        && hunt_scans(sim, id, rules)
        && !(entity.category() == EntityCategory::Unit
            && crate::sim::mcv_deploy::hunt_deploys(sim, id, rules))
}

/// Mission_Hunt's infantry arms, for the target its scan installed. They draw
/// nothing themselves; a Teleport man's destination setter draws for his spot
/// in the cell (`place_infantry_in_native_cell`).
/// - An `Engineer=` infantryman that is neither `C4=` nor holds weapon
///   ability 14 (C4) queues Capture (`0x004D53BA..0x004D53F4`, joining the
///   VehicleThief arm's tail at `0x004D548F..0x004D54B9`).
/// - Otherwise a `C4=` one, or one holding ability 14, whose target is a
///   building queues Sabotage (`0x004D53F9..0x004D5467`). Unlike
///   Mission_Attack's C4 arm it asks no `CanC4=`, and neither does the plant
///   ([`Simulation::infantry_per_cell_c4`]).
/// - Otherwise a `VehicleThief=` one (`+0xEC6`) queues Capture whatever the
///   target is (`0x004D546C..0x004D548F`, joining the Engineer arm's tail).
///
/// Each of those takes the target as its destination (vt+0x480(target, 1)),
/// queues its mission (vt+0x1E8(mission, 0)) and, when Ready_To_Commence
/// (vt+0x200), commences it (vt+0x1EC); [`crate::sim::mission::capture`]
/// then walks it in. Any other man drops his destination (vt+0x480(NULL, 1),
/// `0x004D54C6`) and approaches the target (`0x004D54D2`), so he picks his
/// approach cell afresh on every dispatch.
///
/// RESIDUAL: the theft a `VehicleThief=` man commits on reaching a vehicle
/// (`InfantryClass::PerCellProcess`, `0x0051A0DA`, `0x0051A1C3`) is not
/// ported: he arrives and idles on Capture. Frequency: zero on stock data, as
/// no retail section sets the key.
///
/// Neither call runs for a man out of a deploy action whom the Foot body does
/// not represent ([`Simulation::owns_foot_approach`]): his Target has left
/// the store, or he stands on no map.
fn hunt_infantry_arm(sim: &mut Simulation, id: u64, rules: &RuleSet, ctx: super::ObjectAiCtx<'_>) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let c4 = crate::sim::combat::veterancy::holds_c4(entity.veterancy.rank_kind(), object);
    let Some(target) = entity.attack_target.target() else {
        return;
    };
    let building_target = match target {
        crate::sim::combat::TargetKind::Entity(target) => sim
            .substrate
            .entities
            .get(target)
            .is_some_and(|target| target.category() == EntityCategory::Structure),
        crate::sim::combat::TargetKind::Cell(..) => false,
    };
    let mission = if object.engineer && !c4 {
        MissionType::Capture
    } else if c4 && building_target {
        MissionType::Sabotage
    } else if object.vehicle_thief {
        MissionType::Capture
    } else if !entity.infantry_deploy_doing() && !sim.owns_foot_approach(id, rules) {
        return;
    } else {
        sim.assign_null_destination(id, rules, ctx.overlay_registry, ctx.effects);
        sim.approach_target(id, rules, ctx.overlay_registry, ctx.effects);
        return;
    };
    let _ = sim.assign_destination_represented(
        id,
        Some(target.into()),
        rules,
        ctx.overlay_registry,
        ctx.effects,
    );
    let now = sim.session.binary_frame();
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(mission),
        0,
        now,
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    );
    if sim.mission_ready_to_commence(id, rules) {
        let _ = sim.substrate.mission_commence_exact(id, now);
    }
}

/// Infantry Mission_Attack's C4 arm (`0x0051F3E3..0x0051F455`): a
/// demolition infantryman holding a building it may plant on heads for it
/// (vt+0x480(target, 1)), queues Sabotage (vt+0x1E8(17, 0)) and returns 1,
/// with no draw. The plant itself is the arrival's
/// ([`Simulation::infantry_per_cell_c4`]).
fn infantry_attack_c4(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    if let Some(building) = c4_sabotage_building(sim, id, rules) {
        let _ = sim.assign_destination_represented(
            id,
            Some(crate::sim::components::NavTargetRef::object(building)),
            rules,
            ctx.overlay_registry,
            ctx.effects,
        );
    }
    MissionHandlerEvaluation::queue(1, MissionType::Sabotage)
}

/// The building the C4 arm takes: the Target (`+0x2B4`) of an infantryman
/// whose type is `C4=` (`+0xEC2`) or whose rank holds weapon ability 14
/// (`HasAbility @ 0x0070D0D0`), when it is a building whose type is
/// `CanC4=` (`+0x1577`, constructor default yes) and not
/// `InvisibleInGame=` (`+0x1701`). The arm asks no house. A stale Target
/// ([`attack_target_is_stale`]) is one the native detach has already cleared.
fn c4_sabotage_building(sim: &Simulation, id: u64, rules: &RuleSet) -> Option<u64> {
    let entity = sim.substrate.entities.get(id)?;
    let object = sim.object_type(entity.type_ref(), rules)?;
    if !crate::sim::combat::veterancy::holds_c4(entity.veterancy.rank_kind(), object) {
        return None;
    }
    let crate::sim::combat::TargetKind::Entity(building) = entity.attack_target.target()? else {
        return None;
    };
    let target = sim.substrate.entities.get(building)?;
    if target.category() != EntityCategory::Structure
        || target.death.is_dying()
        || !target.is_alive()
    {
        return None;
    }
    sim.object_type(target.type_ref(), rules)
        .is_some_and(|kind| kind.can_c4 && !kind.invisible_in_game)
        .then_some(building)
}

/// The computer capture arm `InfantryClass::Mission_Attack`
/// (`0x0051F456..0x0051F4D2`) and `Mission_Hunt` (`0x0051F540..0x0051F5BF`)
/// share, on the building [`computer_capture_building`] answers. The
/// infantryman takes it as its destination (vt+0x480(Target, 1)) and Capture
/// as its mission (Assign_Mission, vt+0x1F0 `0x005B2FD0`), and the arm
/// returns 1, drawing nothing. [`crate::sim::mission::capture`] then walks it
/// in, and PerCellProcess garrisons it or lets the spy infiltrate.
fn infantry_computer_capture(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    if let Some(building) = computer_capture_building(sim, id, rules) {
        let _ = sim.assign_destination_represented(
            id,
            Some(crate::sim::components::NavTargetRef::object(building)),
            rules,
            ctx.overlay_registry,
            ctx.effects,
        );
        let now = sim.session.binary_frame();
        let _ = sim.substrate.mission_assign_exact(
            id,
            MissionId::from_known(MissionType::Capture),
            now,
        );
    }
    MissionHandlerEvaluation::cadence(1)
}

/// The building the computer capture arm takes: the Target (`+0x2B4`) of a
/// computer house's infantryman (`HouseClass::IsControlledByHuman @
/// 0x0050B730` false), when it is a building and the infantryman is
/// `Infiltrate=` (`+0xEBE`), or an `Occupier=` (`+0xEB4`) or `Assaulter=`
/// (`+0xEB5`) that the building `CanBeOccupiedBy` (`0x00457CE0`). A stale
/// Target ([`attack_target_is_stale`]) is one the native detach has already
/// cleared. The infiltrator's arm asks no building flag; only its target
/// setter asks `Spyable=` or `Capturable=`
/// ([`Simulation::infiltrator_target_building`]).
fn computer_capture_building(sim: &Simulation, id: u64, rules: &RuleSet) -> Option<u64> {
    let entity = sim.substrate.entities.get(id)?;
    if sim.owner_is_human(entity.owner()) {
        return None;
    }
    let crate::sim::combat::TargetKind::Entity(building) = entity.attack_target.target()? else {
        return None;
    };
    let target = sim.substrate.entities.get(building)?;
    if target.category() != EntityCategory::Structure
        || target.death.is_dying()
        || !target.is_alive()
    {
        return None;
    }
    let object = sim.object_type(entity.type_ref(), rules)?;
    (object.infiltrate
        || ((object.occupier || object.assaulter)
            && crate::sim::passenger::building_can_be_occupied_by(sim, rules, building, id)))
    .then_some(building)
}

/// `FootClass::Mission_AreaGuard`'s building-post arm (`0x004D6B8D..0x004D6D1A`),
/// after the navigation-list head. A computer Foot (`0x0050B730`), other than
/// a harvester Unit, whose post (ArchiveTarget `+0x218`) is a Cell under an own
/// or allied building (the cell's first ground-list Building, `0x0047C520`;
/// `HouseClass::IsAlliedWith @ 0x004F9A50`) moves the post to
/// Find_Nearby_Passable_Cell (`0x0056DC20`) from that cell: Track, Normal, the
/// zone of its own cell (Location / 256, `GetZoneID @ 0x0056D230`), one cell,
/// bridges allowed and no target cell. A zero cell leaves the post. The arm
/// draws nothing and writes no destination; the rest of the visit uses the
/// new post.
fn area_guard_post_off_building(sim: &mut Simulation, id: u64) {
    use crate::rules::locomotor_type::{MovementZone, SpeedType};
    use crate::sim::combat::TargetKind;
    use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(TargetKind::Cell(px, py)) = entity.archive_target() else {
        return;
    };
    let Some(building_owner) = sim
        .substrate
        .occupancy
        .first_building_on_layer(
            px,
            py,
            crate::sim::movement::locomotor::MovementLayer::Ground,
        )
        .and_then(|building| sim.substrate.entities.get(building))
        .map(|building| building.owner())
    else {
        return;
    };
    let owner = entity.owner();
    if !crate::sim::combat::combat_weapon::is_ally_by_object(
        Some(sim.house_alliances.as_map()),
        &sim.interner,
        owner,
        building_owner,
    ) || sim.owner_is_human(owner)
        || (entity.category() == EntityCategory::Unit && entity.is_harvester())
    {
        return;
    }
    // `0x004D6C1E..0x004D6C71`: signed, truncating `/ 256` of the Location.
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    let own_cell = ((location.x / 256) as u16, (location.y / 256) as u16);
    let zone = sim
        .resolved_terrain
        .as_ref()
        .zip(sim.zone_grid.as_ref())
        .and_then(|(terrain, zones)| {
            zones.get_zone_id_native(terrain, own_cell, MovementZone::Normal, false)
        });
    let Some((x, y)) = sim
        .nearby_passable_cell(
            (i32::from(px), i32::from(py)),
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Track,
                    required_zone_id: zone,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::SINGLE,
            ),
        )
        .filter(|cell| *cell != (0, 0))
    else {
        return;
    };
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.set_archive_target(Some(TargetKind::Cell(x, y)));
    }
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

/// `FootClass::Mission_Rescue @ 0x004DDF90`, ordinary Unit/Infantry.
/// Its direct Greatest_Threat uses the archive's+48 XYZ, no passive timer or
/// health debit. Admission is strict distance < mode1 range*1.5. Empty results
/// clear688, set status1, use the House500200 home owner, assign a destination,
/// then clear the archive before the current-mission cadence draw. Status1
/// without NavCom queues AreaGuard and calls Commence directly.
/// Native execution: foot_missions Rescue rows and house_base_return.
/// With a Target in state 0 it approaches through
/// [`Simulation::approach_target`] (`0x004DDFEB`).
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
    if state == 1 && actor.navigation.nav_com().is_none() {
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .set_archive_target(None);
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::AreaGuard),
            0,
            sim.session.binary_frame(),
            &LiveReadyInputProvider { rules },
        );
        let _ = sim
            .substrate
            .mission_commence_exact(id, sim.session.binary_frame());
    } else if state == 0 && actor.attack_target.target().is_none() {
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
            .foot_retarget_after_stop
            .clear_for_rescue();
        let archive = sim
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.archive_target());
        let coords = |sim: &Simulation, target| {
            crate::sim::movement::ground_pose::target_get_coords(
                target,
                sim,
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
                    rules,
                    ctx.overlay_registry,
                    ctx.effects,
                );
            }
        }
        if sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| actor.attack_target.target().is_some())
        {
            return MissionHandlerEvaluation::cadence(1);
        }
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .mission
            .set_handler_state(1);
        let owner = sim
            .substrate
            .entities
            .get(id)
            .expect("Rescue receiver")
            .owner();
        let destination = sim
            .house_return_cell(owner, id)
            .map(|(rx, ry)| crate::sim::components::NavTargetRef::cell(rx, ry));
        let _ = sim.assign_destination_represented(
            id,
            destination,
            rules,
            ctx.overlay_registry,
            ctx.effects,
        );
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .set_archive_target(None);
    } else if state == 0 {
        // `0x004DDFDB..0x004DDFEB`: with a Target, approach it.
        sim.approach_target(id, rules, ctx.overlay_registry, ctx.effects);
    }
    let mission = sim
        .substrate
        .entities
        .get(id)
        .and_then(|actor| actor.mission.current().known())
        .unwrap_or(MissionType::Rescue);
    MissionHandlerEvaluation::cadence(rate_epilogue(rules, mission, &mut sim.scenario_rng))
}

/// Where Area Guard's Retaliate_And_Scan looks for an object holding no
/// target: `post`'s coordinate, once the object can acquire a target and its
/// targeting timer has run out; `None` when the handler does not scan.
/// `0x004D6EF5..0x004D6F06` uses archive+48. A queued mission can leave no
/// archive; that invalid native pointer is outside this ordinary lane. Every
/// term is a pure read, so their order is not observable.
fn area_guard_scan_point(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
    post: Option<crate::sim::combat::TargetKind>,
) -> Option<[i32; 3]> {
    let timer_due = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.passive_scan.timer().due(sim.session.binary_frame()));
    if !(can_acquire_target(sim, id, rules) && timer_due) {
        return None;
    }
    let coord = crate::sim::movement::ground_pose::target_get_coords(
        post?,
        sim,
        sim.resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    )?;
    Some([coord.x, coord.y, coord.z])
}

/// Where `id`'s Area Guard handler ([`evaluate_foot_area_guard`]) scans this
/// frame as the world stands, for the batched target scans
/// (`techno_ai::scan_batch`): its handler is due, it holds no target, it is
/// not a deployed infantryman (whose Area Guard is the deploy shim's,
/// `0x00521320`: its arms act or re-acquire in place), and
/// [`area_guard_scan_point`] answers for its post. The heads that return
/// before the scan, a post the handler archives at this run and a target the
/// leash drops are not foreseen; those objects scan live, if at all.
pub(super) fn foot_area_guard_scan_ahead(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
) -> Option<[i32; 3]> {
    let entity = sim.substrate.entities.get(id)?;
    if entity.mission.current().known() != Some(MissionType::AreaGuard)
        || entity.attack_target.target().is_some()
        || entity.infantry_deploy_doing()
        || entity.is_harvester()
        || !foot_mission_handler_due(sim, id, sim.session.binary_frame())
    {
        return None;
    }
    area_guard_scan_point(sim, id, rules, entity.archive_target())
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
/// The heads (tank bunker, [`foot_building_entry_head`], then a computer
/// Foot's navigation list, [`Simulation::area_guard_navigation_head`])
/// precede it, then a computer Foot's post under a friendly building moves
/// beside it ([`area_guard_post_off_building`]). With a Target and a post it approaches through
/// [`Simulation::approach_target`] (`0x004D6F32`). ElectricAssault adjacency
/// follows the idle block. A harvester instead commences Harvest before the
/// post/leash/scan work (4D6D1A..4D6D68), preserving its target and navigation.
/// Native controls: `harvest_attack_return --area-guard`.
/// Every other Foot with a SlaveManager starts its hunt there
/// (`0x004D6D69..0x004D6D73`, [`Simulation::begin_slave_hunt`]).
/// After establishing the post, the computer demolition arm queues Sabotage
/// and returns 1 before leash/Approach/cadence work (4D6DBE..4D6E44), using
/// the same admission predicate as Guard ([`guard_queues_sabotage`]).
pub(super) fn evaluate_foot_area_guard(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    // `0x004D6AAB..0x004D6ACB`: a Foot in a tank bunker (`+0x2E4`) queues
    // Guard with the immediate flag and returns 1.
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.bunker.link().installed_in().is_some())
    {
        let now = sim.session.binary_frame();
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Guard),
            1,
            now,
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        return MissionHandlerEvaluation::cadence(1);
    }
    if let Some(head) = foot_building_entry_head(sim, rules, id, MissionType::AreaGuard, ctx) {
        return head;
    }
    sim.area_guard_navigation_head(id, rules, ctx.overlay_registry, ctx.effects);
    area_guard_post_off_building(sim, id);
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.category() == EntityCategory::Unit && entity.is_harvester())
    {
        // Foot4D6D34..4D6D68: Queue(Harvest,0), unconditional Commence,
        // then Scenario RandomRanged(1,10)+1. Commence is deliberately not
        // gated by ReadyToCommence; the shared dispatcher writes this delay
        // AFTER the mission change. No idle/target/destination setter runs.
        let now = sim.session.binary_frame();
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Harvest),
            0,
            now,
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        let _ = sim.substrate.mission_commence_exact(id, now);
        return MissionHandlerEvaluation::cadence(
            sim.scenario_rng.next_range_u32_inclusive(1, 10) as i32 + 1,
        );
    }
    // `0x004D6D69..0x004D6D73`: a SlaveManager (`+0x2D8`) starts its hunt
    // (StartHunt `0x006B0CC0`, which acts only on an idle manager).
    sim.begin_slave_hunt(id, rules, ctx.effects);
    let needs_post = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.archive_target().is_none() && entity.mission.queued() == MissionId::NONE
    });
    if needs_post {
        let post = foot_physical_cell_target(sim, id);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.set_archive_target(post);
        }
    }
    // Retail gamemd4D6DBE..4D6E44: Queue(Sabotage,0), return1; no Commence
    // or RNG draw. The common dispatcher queues and writes the timer once.
    if guard_queues_sabotage(sim, id, rules) {
        return MissionHandlerEvaluation::queue(1, MissionType::Sabotage);
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
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            )
        })
    {
        //4D6E74 tests AbstractFlags bit2, not the target's display layer.
        leash = rules.general.guard_mode_stray;
    }
    let may_return = sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.navigation.nav_com().is_none()
            && entity.mission_leaf.foot_firing_sequence_latch() == 0
    });
    if let Some(post) = archive
        && may_return
        && sim
            .object_distance_to(id, post)
            .is_some_and(|distance| distance > leash)
    {
        //4D6EB8 precedes4D6ECB, even when the class target setter refuses.
        let _ = sim.assign_target_represented(id, None, rules, ctx.overlay_registry, ctx.effects);
        let destination = post.into();
        let _ = sim.assign_destination_represented(
            id,
            Some(destination),
            rules,
            ctx.overlay_registry,
            ctx.effects,
        );
    }
    let needs_target = sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.target().is_none());
    // `0x004D6ED1..0x004D6F32`: with a Target the body approaches it instead
    // of scanning; without a post it does neither (`0x004D6E66`).
    if !needs_target && archive.is_some() {
        sim.approach_target(id, rules, ctx.overlay_registry, ctx.effects);
    }
    if needs_target
        && let Some(point) = area_guard_scan_point(sim, id, rules, archive)
        && super::target_scan::scan(
            sim,
            id,
            rules,
            crate::sim::combat::ScanMission::AreaGuard,
            ctx,
            Some(point),
        )
    {
        return MissionHandlerEvaluation::cadence(1);
    }
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.target().is_none())
    {
        let _ = sim.infantry_idle_action(id, rules, ctx.overlay_registry, ctx.effects);
    }
    //4D6F38..4D701F re-reads TarCom after idle and queues before the cadence
    //draw. An acquired enemy already returned above.
    sim.acquire_adjacent_tesla_coil(id, rules, ctx.overlay_registry, ctx.effects);
    // FootClass::Mission_AreaGuard4D7025 calls MissionControl5B3A00 on
    // the live current mission: Aircraft Idle may already have entered Enter.
    let current = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.mission.current().known())
        .unwrap_or(MissionType::AreaGuard);
    let base = mission_cadence(rules, current);
    // `0x004D7040..0x004D7048`: an aircraft (What_Am_I 2) doubles the Rate
    // before the draw.
    let base = if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.category() == EntityCategory::Aircraft)
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
    let delay = if foot_dispatch_in_cadence_band(sim, rules, id) {
        jittered / 6
    } else {
        jittered
    };
    MissionHandlerEvaluation::cadence(delay)
}

/// The head `FootClass::Mission_Guard` (`0x004D5076..0x004D50CD`) and
/// `Mission_AreaGuard` (`0x004D6ACC..0x004D6B22`) share: with `+0x68F` set
/// the Foot runs its Bio Reactor seek (vt+0x340,
/// [`Simulation::foot_enter_bio_reactor`]), else with `+0x690` set its
/// garrison seek (vt+0x348, [`Simulation::foot_enter_battle_bunker`]);
/// either answer is unread, and the handler returns the then-current
/// mission's `ftol(Rate × 900)` (`0x005B3A00`) with no draw: Capture's,
/// once a garrison seek has commenced it. `+0x691` (vt+0x34C) follows,
/// seeking across the entire Building registry for script64's civilian
/// garrison teams (`sim::building_entry`).
fn foot_building_entry_head(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    mission: MissionType,
    ctx: super::ObjectAiCtx<'_>,
) -> Option<MissionHandlerEvaluation> {
    let latches = sim.substrate.entities.get(id)?.building_entry;
    if latches.should_enter_absorber() {
        let _ = sim.foot_enter_bio_reactor(id, rules, ctx.overlay_registry, ctx.effects);
    } else if latches.should_enter_occupiable() {
        let _ = sim.foot_enter_battle_bunker(id, rules, ctx.overlay_registry, ctx.effects);
    } else if latches.should_enter_nearest_occupiable() {
        let _ = sim.foot_enter_nearest_occupiable(id, rules, ctx.overlay_registry, ctx.effects);
    } else {
        return None;
    }
    let current = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.mission.current().known())
        .unwrap_or(mission);
    Some(MissionHandlerEvaluation::cadence(mission_cadence(
        rules, current,
    )))
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
/// **Cadence.** The building-entry head returns the base cadence with no
/// draw. Past it, while the object's rearm timer runs (`+0x2EC`, which
/// FireAt arms with GetROF each shot:
/// [`GameEntity::rearm_timer`](crate::sim::game_entity::GameEntity::rearm_timer)),
/// the handler returns its remaining frames and draws nothing
/// (0x004D52A9..0x004D52F4): a guarding object that just fired next wakes when
/// it can fire again. Otherwise it is `[Rate] + RandomRanged(0, 2)`
/// (0x004D532F).
///
/// **Arms.** A held Target takes the HoverAttack re-anchor
/// ([`hover_attack_reanchor`]); a computer demolition infantryman's building
/// Target queues Sabotage ([`guard_queues_sabotage`]) with every return.
pub(super) fn evaluate_foot_guard_cadence(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    mission: MissionType,
    ctx: super::ObjectAiCtx<'_>,
) -> MissionHandlerEvaluation {
    if let Some(head) = foot_building_entry_head(sim, rules, id, mission, ctx) {
        return head;
    }
    let effects = ctx.effects;
    // FootGuard4D50DB: a held Target takes the HoverAttack arm (4D51E0);
    // none, the adjacency scan and idle action. Both precede the rearm-timer
    // early return.
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.attack_target.target().is_some())
    {
        hover_attack_reanchor(sim, rules, id, ctx);
    } else if !sim.acquire_adjacent_tesla_coil(id, rules, ctx.overlay_registry, effects) {
        //4D51A6 /4D51D8: Unit's receiver is a false stub; Infantry's owner
        //decides actual admission.
        let _ = sim.infantry_idle_action(id, rules, ctx.overlay_registry, effects);
    }
    let queue = guard_queues_sabotage(sim, id, rules).then_some(MissionType::Sabotage);
    let rearm = sim.substrate.entities.get(id).map_or(0, |entity| {
        entity
            .rearm_timer
            .remaining(sim.session.binary_frame() as i32)
    });
    if rearm != 0 {
        return MissionHandlerEvaluation {
            delay: rearm,
            queue,
        };
    }
    // FootGuard4D530B..4D5338: after the rearm early return, a retained
    // DistributedFire candidate count skips the mission jitter and returns0.
    // This does not alter709820's separate scan jitter or targeting timer.
    if sim.substrate.entities.get(id).is_some_and(|entity| {
        !entity.distributed_fire_candidates().is_empty()
            && sim
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|kind| kind.distributed_fire)
    }) {
        return MissionHandlerEvaluation { delay: 0, queue };
    }
    MissionHandlerEvaluation {
        delay: rate_epilogue(rules, mission, &mut sim.scenario_rng),
        queue,
    }
}

/// `FootClass::Mission_Guard`'s computer demolition arm (`0x004D523A..
/// 0x004D52A9`) and `Mission_AreaGuard`'s (`0x004D6DBE..0x004D6E44`):
/// an Infantry whose house is not human-controlled (`0x0050B730`), whose
/// type is `C4=` (`+0xEC2`) or whose rank holds weapon ability 14
/// (`0x0070D0D0`), not already on Sabotage (vt+0x184), holding a building
/// Target queues Sabotage, vt+0x1E8(17, 0). Unlike Mission_Attack's arm it
/// asks no `CanC4=` and writes no destination: Sabotage's Mission_Capture
/// heads for the Target. A computer's Tanya or Navy SEAL holding that
/// building Target thus walks in to plant. No draw. Guard continues to its rearm
/// timer/cadence; AreaGuard returns 1 immediately after queueing.
fn guard_queues_sabotage(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.category() != EntityCategory::Infantry
        || sim.owner_is_human(entity.owner())
        || entity.mission.current().known() == Some(MissionType::Sabotage)
        || !sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                crate::sim::combat::veterancy::holds_c4(entity.veterancy.rank_kind(), object)
            })
    {
        return false;
    }
    // A dying building is the null Target native's detach leaves.
    matches!(
        entity.attack_target.target(),
        Some(crate::sim::combat::TargetKind::Entity(target)) if sim
            .substrate
            .entities
            .get(target)
            .is_some_and(|target| target.category() == EntityCategory::Structure
                && !target.death.is_dying()
                && target.is_alive())
    )
}

/// The HoverAttack re-anchor `FootClass::Mission_Attack` opens with
/// (`0x004D4DC0..0x004D4E07`) and `FootClass::Mission_Guard`'s held-Target
/// arm runs (`0x004D51E0..0x004D521F`): a `HoverAttack=` type
/// (`TechnoType+0x390`) at GetHeight (`0x005F5F40`) zero takes NearbyLocation
/// anchored on itself (`0x00703590`) as its destination, vt+0x480(cell, 1).
/// Retail carriers (a plain scan of rulesmd.ini) are all Jumpjets: JUMPJET
/// and LUNR infantry, SCHP/SCHD, SHAD and HIND. `BalloonHover=` keeps the
/// Rocketeer aloft, so the gate passes only while one stands on the ground.
/// This step draws nothing; the receiver's Jumpjet Move_To may (its
/// placement draw, `infantry_scatter.rs`). No retail aircraft carries the
/// key, so `AircraftClass`'s Guard path through `0x004D5070` leaves it
/// dormant.
///
/// A failed NearbyLocation still returns packed Empty(0,0). Both callers
/// freshly look up that cell before passing the nonnull receiver to the
/// concrete destination setter (4D4DFF/4D5217). The Jumpjet then owns its
/// own placement search, NavCom rewrite and moving byte.
fn hover_attack_reanchor(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    ctx: super::ObjectAiCtx<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if !sim
        .interner
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.hover_attack)
        || crate::sim::movement::air_movement::current_fly_height(
            entity,
            sim.resolved_terrain.as_ref(),
        ) != 0
    {
        return;
    }
    let (rx, ry) = sim
        .techno_nearby_location(id, Some(id), rules)
        .unwrap_or((0, 0));
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        terrain.native_cell_identity((rx, ry));
    }
    if let Err(error) = sim.assign_destination_represented(
        id,
        Some(crate::sim::components::NavTargetRef::cell(
            rx as u16, ry as u16,
        )),
        rules,
        ctx.overlay_registry,
        ctx.effects,
    ) {
        log::debug!("HoverAttack {id} kept its destination: {error}");
    }
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
        .filter(|house| !house.is_controlled_by_human(sim.session.game_mode_nonzero()))
    {
        // Arm (ii), `0x00740880..0x0074092C`.
        return house.tracking().owns_any_building(
            unit_type
                .dock
                .iter()
                .filter_map(|name| sim.interner.get(name)),
        ) && !house.harvester_no_ore();
    }
    if !unit_type.teleporter {
        return false;
    }
    let (rx, ry) = (
        i32::from(entity.position.rx()),
        i32::from(entity.position.ry()),
    );
    for (dx, dy) in DIRECTION_DELTAS {
        let (Ok(cx), Ok(cy)) = (u16::try_from(rx + dx), u16::try_from(ry + dy)) else {
            continue;
        };
        // 74096F selects the first Building before the refinery/owner tests.
        let own_refinery = sim
            .substrate
            .occupancy
            .first_building_on_layer(
                cx,
                cy,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            )
            .is_some_and(|sid| {
                sim.substrate.entities.get(sid).is_some_and(|building| {
                    building.owner() == entity.owner()
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

/// Which committed missions still sit on the un-overridden base handler for a
/// given category, i.e. which ones re-arm with the flat
/// [`MISSION_STUB_FRAMES`] and consume no RNG.
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
        return Some(MISSION_STUB_FRAMES);
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
    (shared_base_stub || category_base_stub).then_some(MISSION_STUB_FRAMES)
}

#[inline]
fn mission_cadence(rules: &RuleSet, mission: MissionType) -> i32 {
    rules.mission_control.rate_frames(mission)
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
    let Some(target) = attacker.attack_target.target() else {
        return false;
    };
    //4D4F22..4D4F44 asks the target's virtual+48, including Terrain's
    //retained Object coordinate; TarCom's legacy Entity arm is not a class gate.
    let terrain = sim.resolved_terrain.as_ref();
    let Some(target) = crate::sim::movement::ground_pose::target_get_coords(
        target,
        sim,
        terrain
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
            .as_ref(),
    ) else {
        return false;
    };
    let source = crate::sim::movement::ground_pose::object_get_coords(attacker, terrain);
    native_distance_is_in_cadence_band(source, target)
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
    from: crate::sim::components::DriveCoord,
    to: crate::sim::components::DriveCoord,
) -> bool {
    let dx = i64::from(from.x) - i64::from(to.x);
    let dy = i64::from(from.y) - i64::from(to.y);
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
    let Some(object) = sim.interner.object_type(attacker.type_ref(), rules) else {
        return false;
    };
    if attacker.category() == EntityCategory::Infantry && object.close_range {
        return true;
    }
    let Some(primary) = object.primary().and_then(|name| rules.weapon(name)) else {
        return false;
    };
    i64::from(primary.range_leptons) < close_primary_range_leptons
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
        .and_then(|actor| actor.attack_target.target());
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
    );
    // `0x0051F38A..0x0051F39D`: Assign_Target(pick) when a target is held or
    // one was found. It writes no `+0x50C`, so the setter leaves the flag
    // clear.
    if had_target || pick.is_some() {
        let _ = sim.assign_target_represented(
            id,
            pick.map(crate::sim::combat::TargetKind::Entity),
            rules,
            ctx.overlay_registry,
            ctx.effects,
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
    sim.infantry_enter_idle_mode(id, rules, ctx.overlay_registry, ctx.effects);
    None
}

/// VERA-only: a TarCom (`+0x2B4`) naming a dying or Health-0 object counts as the NULL
/// the native detach leaves; the `TerminalLifetime` mark and Health are the truth.
fn attack_target_is_stale(sim: &Simulation, id: u64) -> bool {
    let Some(attacker) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(crate::sim::combat::TargetKind::Entity(target_id)) = attacker.attack_target.target()
    else {
        return false;
    };
    // Native lethal DetachAll clears any Object target, not only a Techno;
    // the shared presence owner reads Terrain's own lifecycle/Health.
    !crate::sim::combat::target_present(
        Some(crate::sim::combat::TargetKind::Entity(target_id)),
        sim,
    )
}

#[cfg(test)]
mod guard_rearm_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;

    /// Full original Guard tail plus native Random2 state, including skipped
    /// jitter and rejection sampling's three raw words in the drawing rows.
    #[test]
    fn distributed_guard_cadence_and_rng_match_original() {
        use crate::sim::combat::TargetKind;
        use crate::sim::game_entity::DistributedFireCandidate;
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/rearm_timer.json",
        ))
        .unwrap();
        let rows = vectors["guard_distributed_rng"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let input = &row["input"];
            let field = |name: &str| input[name].as_i64().unwrap() as i32;
            // The oracle starts after rate conversion with EBP=27, not an
            // authored Rate. Supply that same boundary through the production
            // reader away from an integer edge: .0301 converts to27. Native
            // ReadDouble's float projection makes .03 convert to26 (the
            // established mission_data owner), so that is a different input.
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[VehicleTypes]\n0=TEST\n[TEST]\nDistributedFire={}\n[Guard]\nRate=.0301\n",
                if input["distributed"] == true {
                    "yes"
                } else {
                    "no"
                },
            )))
            .unwrap();
            assert_eq!(
                mission_cadence(&rules, MissionType::Guard),
                field("base_cadence")
            );
            let mut sim = Simulation::with_seed(1);
            sim.session.set_binary_frame_for_test(field("frame") as u32);
            let mut entity = GameEntity::test_default(1, "TEST", "Americans", 5, 5);
            entity.set_type_ref_for_test(sim.interner.intern("TEST"));
            entity.set_owner_for_test(sim.interner.intern("Americans"));
            entity
                .rearm_timer
                .set_for_test(crate::sim::timer::CdTimer::from_raw(
                    field("rearm_start"),
                    field("rearm_duration"),
                ));
            entity.replace_distributed_fire_candidates(
                (0..field("candidate_count"))
                    .map(|_| DistributedFireCandidate {
                        target: Some(TargetKind::Entity(2)),
                        score: 1,
                    })
                    .collect(),
            );
            sim.substrate.entities.insert(entity);
            sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
                row["rng_before_hex"].as_str().unwrap(),
            );
            let main = sim.main_rng.state();
            let mapgen = sim.mapgen_rng.state();
            let evaluation = evaluate_foot_guard_cadence(
                &mut sim,
                &rules,
                1,
                MissionType::Guard,
                super::super::ObjectAiCtx::default(),
            );
            assert_eq!(
                evaluation.delay,
                row["returns"].as_i64().unwrap() as i32,
                "{}",
                row["name"]
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after_hex"].as_str().unwrap(),
                "{}",
                row["name"]
            );
            assert_eq!(sim.main_rng.state(), main);
            assert_eq!(sim.mapgen_rng.state(), mapgen);
        }
    }

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
            sim.session.set_binary_frame_for_test(field("frame") as u32);
            let mut entity = GameEntity::test_default(1, "TEST", "Americans", 5, 5);
            // Guard now reaches the native GetWeapon(1) query before its
            // timer tail. Keep the fixture's null weapon, with valid identities.
            entity.set_type_ref_for_test(sim.interner.intern("TEST"));
            entity.set_owner_for_test(sim.interner.intern("Americans"));
            entity
                .rearm_timer
                .set_for_test(crate::sim::timer::CdTimer::from_raw(
                    field("start"),
                    field("duration"),
                ));
            sim.substrate.entities.insert(entity);
            let before = sim.scenario_rng.logical_state();

            let evaluation = evaluate_foot_guard_cadence(
                &mut sim,
                &rules,
                1,
                MissionType::Guard,
                super::super::ObjectAiCtx::default(),
            );

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

#[cfg(test)]
mod terrain_target_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};

    #[test]
    fn terrain_target_uses_object_presence_and_cadence_coordinates() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\nPrimary=GUN\n\
             [GUN]\nDamage=10\nRange=1\nROF=10\nWarhead=WH\n\
             [Warheads]\n0=WH\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let mut actor = GameEntity::test_default(1, "TEST", "Americans", 5, 5);
        actor.set_type_ref_for_test(sim.interner.intern("TEST"));
        actor
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        sim.substrate.entities.insert(actor);
        let tree_type = sim.interner.intern("TREE01");
        sim.terrain_objects
            .objects_mut_for_test()
            .insert(2, TerrainObjectState::for_test(2, tree_type, 7, 5));

        assert!(!attack_target_is_stale(&sim, 1));
        assert!(foot_dispatch_in_cadence_band(&sim, &rules, 1));
        sim.terrain_objects
            .objects_mut_for_test()
            .get_mut(&2)
            .unwrap()
            .set_health_for_test(0);
        assert!(attack_target_is_stale(&sim, 1));
        // Coordinate virtuals remain readable while expiry still holds the
        // Object; presence and coordinate resolution are different questions.
        assert!(foot_dispatch_in_cadence_band(&sim, &rules, 1));
        let tree = sim
            .terrain_objects
            .objects_mut_for_test()
            .get_mut(&2)
            .unwrap();
        tree.set_health_for_test(100);
        tree.set_lifecycle_for_test(TerrainObjectLifecycle::Limbo);
        // Object Alive remains set in Limbo; only UnInit clears it.
        assert!(!attack_target_is_stale(&sim, 1));
        sim.terrain_objects
            .objects_mut_for_test()
            .get_mut(&2)
            .unwrap()
            .set_lifecycle_for_test(TerrainObjectLifecycle::Destroyed);
        assert!(attack_target_is_stale(&sim, 1));
        sim.terrain_objects.objects_mut_for_test().remove(&2);
        assert!(attack_target_is_stale(&sim, 1));
        assert!(!foot_dispatch_in_cadence_band(&sim, &rules, 1));
    }
}
