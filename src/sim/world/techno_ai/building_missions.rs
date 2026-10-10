//! A building's mission work in `BuildingClass::Update` (`0x0043FB20`),
//! around and inside `TechnoClass::AI_Update`:
//!
//! - the ready check before the Techno AI (`0x0043FE27..0x0043FE54`) and the
//!   one after it (`0x0043FF91..0x0043FFB4`): with `+0x6DD` set, a successful
//!   Commence of the queued mission clears the byte
//!   ([`Simulation::mission_building_ready_commence`]). The first also needs
//!   BState (`+0x534`) out of 0, the construction body. UpdateAnimation
//!   (`0x004509D0`) steps the one body StageClass before these checks;
//!   construction and sale consume its shared ready byte.
//! - `MissionClass::AI` (`0x005B3060`, called at `0x006FA655`): when the
//!   dispatch timer (`+0xC8`) is due and Health is above zero, the current
//!   mission's handler runs and its return is the next delay
//!   (`0x005B32F2..0x005B3302`). Guard and Sticky run Mission_Guard
//!   (`0x004496B0`, dispatch table `0x005B34E8`), Area Guard too (`0x00449A40`
//!   jumps to it), Attack runs Mission_Attack (`0x0044ACF0`), Selling runs
//!   Sell ([`Simulation::visit_building_down`]), Construction runs
//!   [`mission_construction`] (`0x00449A50`), and a mission the building
//!   has no handler for, or none, a MissionClass stub (450 frames).
//! - the Gattling block after the Techno AI (`0x0043FE5B..0x0043FF8B`): the
//!   idle decay of a Gattling type and its turret-animation steps
//!   ([`gattling_idle`]).
//! - `ProcessDelayedFire` (`0x004503F0`, called at `0x004400F4` while Health
//!   is above zero): the countdown of a delayed fire Mission_Attack armed,
//!   and what it does when the countdown ends ([`process_delayed_fire`]).
//! - the range drop at the end of the Update (`0x00440378..0x004403C6`).
//!
//! A Gattling type (`IsGattling=`, the Gattling Cannon) charges and decays its
//! stages inside both handlers (`combat::gattling`), each call taking the
//! mission's `+0xC4` count, the frames since it was last zeroed, and zeroing
//! it: Mission_Guard decays first thing on every dispatch (`0x004496C1..
//! 0x004496DF`), Mission_Attack charges after its FireAt and on FACING and
//! REARM, and decays on its drop tail, BUSY and CLOAKED. In Attack the count
//! is the previous handler's return, so a cannon charges RateUp per frame it
//! spends attacking; outside Attack, Update's idle decay takes RateDown per
//! frame once `GuardAreaTargetingDelay + 5` frames have passed since its last
//! shot.
//!
//! The Update's two FireAts, Mission_Attack's (`0x0044B6D0`) and
//! ProcessDelayedFire's (`0x00450492`), are VERA's combat emission: the visit
//! asks the combat phase for the shot
//! ([`crate::sim::combat::FireRequests::buildings`]), which emits it without
//! asking GetFireError again. A building fires no shot without that request.
//!
//! A Prism tower (`[General] PrismType=`) never takes Mission_Attack's FireAt
//! arm: it recruits one charged tower of its House per visit to beam at it,
//! then arms its own delayed shot, whose damage grows with the count
//! ([`prism_arm`], [`Simulation::take_support_bonus`]).
//!
//! Evidence: `tools/spatial_oracle/building_guard_attack.json`, replayed in
//! the tests below: Mission_Guard's returns and Scenario draws per arm,
//! Mission_Attack's return and actions per fire-error code,
//! BuildingClass::SetTarget's decisions, Unlimbo's mission (`0x0044D6A0`) and
//! the dispatch cadence of Update's mission pieces run natively frame by
//! frame (the shot every ROF + 1 frames for an even ROF, every ROF frames
//! for an odd one). `tools/spatial_oracle/building_prism.json`: the Prism
//! arm's recruitment and master arm, ProcessDelayedFire's two modes, the
//! support bonus and its damage, the multi-tower cadence and the `[General]`
//! Prism reader, run natively. `tools/spatial_oracle/building_gattling.json`
//! (`building_gattling_tests`): every Gattling call of both handlers and of
//! the idle decay, and whole engagements frame by frame.
//!
//! RESIDUALS, each with its later owner:
//! - The frame position of a building's shot. Native fires both FireAts
//!   inside the building's own Logic visit; VERA emits them in the combat
//!   phase after the Logic pass, like every class's FireAt (the draw-order
//!   residual `docs/plans/2026-09-21-combat-parity.md` records for all of
//!   them). Trigger: every building shot. Effect: the shot's Scenario draws
//!   (GetROF's `RandomRanged(0, 2)` at `0x006FD09E`, the bullet's own) and
//!   its rearm and ammo writes follow the Logic visits of the objects after
//!   the building, not precede them; the bullet still takes its first AI at
//!   the tail of the same pass ([`Simulation::visit_combat_tail`]).
//!   Frequency: every frame a building fires while a later object draws.
//!   Downstream: the Scenario stream's order in that frame. The pass's one
//!   reader of another building's rearm, the Prism walk, counts a requested
//!   shot as rearming ([`prism_supporter`]); the support count the shot
//!   clears (`0x004504CD`) is read only in its own tower's visits. The
//!   request carries the visit's target and weapon ([`BuildingShot`]), so a
//!   Gattling charge after the FireAt or a later retarget leaves the shot as
//!   the visit made it. FireAt (`0x006FDD50`) draws `g_MainRng` only through
//!   callees off the Gattling Cannon's path (SpawnRadEruption `0x006FD800`,
//!   EBolt::Init `0x004C2A60`, audio; an instruction scan), so the charge's
//!   loop draw keeps its native place in that stream. Later owner: FireAt
//!   moving into each object's Logic visit.
//! The house-color Prism support/main lasers now emit ordered copied births
//! through `combat::laser` and share the app LaserDraw/DSurface owner. The
//! remaining non-house randomized spread path belongs to DrawBeam550260.
//! - UnitReload/Bunker Guard contact admission remains with those separate
//!   families (449817..499B5). UnitRepair's raw-distance/NEED_MOVE/Repair
//!   queue is ported here; DOCK_NOW independently supplies its ready latch.
//!   Trigger: airfield reload or bunker entry. Effect: Guard's queued mission
//!   and Scenario draw count still differ on those paths. Foundation waiters
//!   and the full arrival position producer are bounded by the depot corpus.
//!   WeaponsFactory ClearBibArea449540 is dormant with retail data because
//!   every stock WeaponsFactory retains HasStupidGuardMode.
//! - Voxel HVA frames now reach `emit_building_turret_vxl`: original43DA80
//!   selects +148 modulo the turret's frames (or the barrel's frames on the
//!   barrel-only arm); a separate barrel uses frame0. Native component
//!   comparisons live in tools/voxel_oracle/building_barrel.json. This does
//!   not close the other building Attack residuals in #757.
//! - Dormant with retail data: the SAM arm (`0x0044AD07`, `SAM=` unset), the
//!   upgrade arm (`0x0044B2BC`, no `PowersUpBuilding=`), Mission_Guard's
//!   SuperWeapon gate (`0x00449716..0x00449753`, no armed type sets
//!   `SuperWeapon=`), the waypoint-planning hook (`0x0044AFB1`, VERA has no
//!   planning mode) and BuildingClass::SetTarget's TickTank/Artillary
//!   undeploy (`0x00443C07..0x00443C54`, neither key set).
//!
//! ## Dependency rules
//! - Part of sim/; sim/ never depends on render/, ui/, audio/, net/.

use super::target_scan::{can_fire_at, fire_error_with_overlay, select_weapon, weapon_at_index};
use super::{ObjectAiCtx, mission_handlers_run};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::building_art::requested_damage_state;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::{self, WeaponSlot};
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::gattling::{StageCall, is_elite};
use crate::sim::combat::{BuildingShot, fire_coord};
use crate::sim::game_entity::{DelayedFire, PendingBuildingFire};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::projectile::ProjectilePayload;
use crate::sim::world::Simulation;

/// The MissionClass stubs a building's table holds for every mission it has
/// no handler of its own for (`0x005B2E10..0x005B2FC0`; its Capture,
/// Sabotage and Harvest slots jump to them, `0x0044B760`, `0x0044B770`):
/// 450 frames.
const DEFAULT_MISSION_DELAY: i32 = 450;
/// An unarmed HasStupidGuardMode building's Guard return (`0x004497F4`).
const STUPID_GUARD_DELAY: i32 = 100;
/// The building anim slots of `ActiveAnim=` and `SpecialAnim=` (Building
/// `+0x55C` + 4 x slot; their names at BuildingType `+0x1018` and `+0x11F4`).
const ACTIVE_ANIM_SLOT: u8 = 3;
const SPECIAL_ANIM_SLOT: u8 = 10;

///4509D0 runs once before Techno AI. The native type/control owner decides
/// Stage/ready, including idle and sale; neither mission steps its own clock.
pub(super) fn update_animation(sim: &mut Simulation, id: u64, rules: Option<&RuleSet>) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(rules) = rules else {
        return;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let has_turret = object.has_turret;
    let archive_less_sale =
        crate::sim::production::archive_less_sale(rules.into(), &object.id, entity);
    let now = sim.session.binary_frame as i32;
    let options = &sim.session.game_options;
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.advance_building_body(now, archive_less_sale, has_turret, options);
    }
    // 450F9E..451145, between the step and the ready admission, which it
    // does not touch.
    sim.update_super_weapon_anims(id, rules);
}

///43FE27's first check requires BState!=0;43FF91's second does not.
pub(super) fn ready_commence(sim: &mut Simulation, id: u64, before_techno: bool) {
    if before_techno
        && sim
            .substrate
            .entities
            .get(id)
            .is_none_or(|entity| entity.in_construction_bstate())
    {
        return;
    }
    let _ = sim.mission_building_ready_commence(id, sim.session.binary_frame);
}

pub(super) fn apply_queued_body(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity
            .apply_queued_building_body(sim.session.binary_frame as i32, &sim.session.game_options);
    }
}

/// `MissionClass::AI` (`0x005B3060`) for a building (module doc).
pub(super) fn dispatch(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let current = entity.mission.current().known();
    let now = sim.session.binary_frame;
    if !entity.mission.dispatch_timer().due(now) || !mission_handlers_run(sim, id) {
        return;
    }
    if current == Some(MissionType::Selling) {
        // Sell shares the same dispatch timer and Health gate as Construction.
        sim.visit_building_down(id, rules, ctx.overlay_registry);
        return;
    }
    let Some(rules) = rules else {
        return;
    };
    let delay = match current {
        Some(MissionType::Guard | MissionType::Sticky | MissionType::AreaGuard) => {
            mission_guard(sim, id, rules)
        }
        Some(MissionType::Attack) => mission_attack(sim, id, rules, ctx),
        Some(MissionType::Construction) => mission_construction(sim, id, rules, ctx),
        Some(MissionType::Unload) => match mission_unload(sim, id, rules, ctx) {
            Some(delay) => delay,
            None => return,
        },
        Some(MissionType::Open) => crate::sim::gate_runtime::mission_open(sim, id, rules),
        // BuildingRepair44B780 runs at this visit before the Factory sweep;
        // unsupported repair families leave dispatch with their current owner.
        Some(MissionType::Repair) => match crate::sim::docking::building_dock::mission_repair(
            sim,
            rules,
            id,
            ctx.overlay_registry,
        ) {
            Some(delay) => delay,
            None => return,
        },
        Some(MissionType::Missile) => super::building_missile::mission_missile(sim, id, rules),
        // Every other slot of the building's table, and no mission (above
        // `0x1F`, `0x005B30BB`), is a MissionClass stub.
        _ => DEFAULT_MISSION_DELAY,
    };
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.write_dispatch_epilogue(now as i32, delay);
    }
}

/// Original449A50, with the native first-contact calls (vt27465ACB0),
/// queued body handoff and shared445F80 opening, inside this object's visit.
fn mission_construction(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> i32 {
    use crate::sim::building_construction::BuildingBodyMode;
    use crate::sim::radio::{RadioMessage, transmit_to_contact};
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let status = entity.mission.handler_state();
    let now = sim.session.binary_frame;
    if status == 0 {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .begin_building_body(BuildingBodyMode::Construction, now as i32);
        transmit_to_contact(sim, id, RadioMessage::DockApproach, Some(rules));
        let sound = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| sim.object_type(entity.type_ref(), rules))
            .and_then(|object| object.buildup_sound.as_deref())
            .or(rules.general.construction_sound.as_deref());
        if let Some(sound) = sound {
            let sound_id = sim.interner.intern(sound);
            if let Some(world) = sim.anim_owner_coords(id) {
                sim.sound_events
                    .push(crate::sim::world::SimSoundEvent::ObjectSoundStarted {
                        owner: id,
                        sound_id,
                        world,
                    });
            }
        }
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.mission.set_handler_state(1);
        }
    } else if status == 1 && entity.building_ready_latch() != 0 {
        transmit_to_contact(sim, id, RadioMessage::DockArrived, Some(rules));
        transmit_to_contact(sim, id, RadioMessage::Break, Some(rules));
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.begin_building_body(BuildingBodyMode::Idle, now as i32);
        }
        sim.grand_opening(id, false, false, rules, ctx.overlay_registry);
        let facing = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| sim.object_type(entity.type_ref(), rules))
            .filter(|object| !object.laser_fence)
            .map(|_| crate::rules::object_type::ObjectType::BUILDING_FACING);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                MissionId::from_known(MissionType::Guard),
            );
            if let Some(facing) = facing {
                entity.body_facing.snap(u16::from(facing) << 8, now);
            }
        }
        //465AF0 clears the shared type's loaded Buildup cache in native; VERA
        //keeps immutable retail metadata, while drawing reads BState/Stage.
        sim.sound_events
            .push(crate::sim::world::SimSoundEvent::ObjectSoundReleased { owner: id });
    }
    1
}

/// `BuildingClass::Mission_Unload` (`0x0044D880`). A building with
/// occupants (`vt+0x408`) first hands every one to `SellBuilding(0, 0)`
/// (`0x0044D89C`). An absorber holding passengers (`InfantryAbsorb=` /
/// `UnitAbsorb=`, Type `+0x16AE`/`+0x16AF`, Passengers `+0x114`) and a
/// `WeaponsFactory=` (`+0x16BD`) then take their own arms. The absorber's
/// passenger mechanism remains with its existing owner; the factory arm
/// below uses the shared Techno Door, Drive track and Unit PerCell owners.
/// Every other building queues Guard
/// (`vt+0x1E8(5, 0)` at `0x0044E379`) and returns 1.
///
/// Residual: a gap generator (`+0xCD1` and `+0xCD3`) first re-toggles its
/// gap (`0x0044E2BE..0x0044E365`: vt+0x418, `+0x26C`, vt+0x350), not ported;
/// its Guard tail is. No retail gap generator takes an Unload order (the app
/// offers Unload only to `CanBeOccupied=` types).
fn mission_unload(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> Option<i32> {
    crate::sim::production::sell_building_occupants(sim, rules, ctx.overlay_registry, id);
    let entity = sim.substrate.entities.get(id)?;
    let object = sim.object_type(entity.type_ref(), rules)?;
    let holds_passengers = entity
        .passenger_role
        .cargo()
        .is_some_and(|cargo| !cargo.is_empty());
    if (object.infantry_absorb || object.unit_absorb) && holds_passengers {
        return None;
    }
    if object.weapons_factory {
        return Some(mission_factory_unload(sim, id, rules, ctx));
    }
    let now = sim.session.binary_frame;
    let readiness = LiveReadyInputProvider { rules };
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &readiness,
    );
    Some(1)
}

///WeaponsFactory arm44DCB9..44E293 of the sole Building44D880 receiver.
///The common Drive product crosses the actual foundation on ForceTrack66;
///Unit PerCell owns clearance8/Scatter, and the producer's tether controls
///closing. Native frame/timer/track/RNG controls: unit_unlimbo continuation.
fn mission_factory_unload(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> i32 {
    use crate::sim::door::DoorPhase;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 0;
    };
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return 0;
    };
    let status = entity.mission.handler_state();
    let now = sim.session.binary_frame;
    let ticks = object.deploy_time_ticks;
    let naval = object.naval;
    let phase = entity.door_phase();
    let contact = entity.radio_contacts.slot(0);
    let tethered = entity.dock_entered_with.is_some();
    let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    let track = crate::sim::movement::factory_exit_track_coordinate(location, object);
    let damaged = requested_damage_state(
        entity.health,
        object.strength,
        rules.general.condition_yellow,
    );
    match status {
        0 => {
            //44DD56..44DD75: contact0 queues Guard and Commences before
            //the producer opens its own Door and publishes state1 (4 naval).
            if let Some(product) = contact {
                queue_and_commence(sim, product, MissionType::Guard, rules);
            }
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.open_door(ticks, now);
            entity.mission.set_handler_state(if naval { 4 } else { 1 });
            //44DDBF/44DE16: use the existing animation lifetime owner. The
            //original ART readers leave both names empty for stock GAWEAP.
            sim.clear_building_anim_slot(id, 18);
            let _ = sim.set_building_anim_slot(id, 8, damaged, false, 0, rules);
            //Draw-dirty and activation sound remain presentation dependencies.
        }
        1 => {
            if !clear_factory_exit_bib(sim, id, track, rules, ctx) {
                //44DE3C: no synchronous early fallthrough into state2.
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .mission
                    .set_handler_state(2);
            }
        }
        2 if phase == DoorPhase::OpenStable => {
            if let Some(product) = contact {
                let _ = sim.mission_queue_exact(
                    product,
                    MissionId::from_known(MissionType::Move),
                    0,
                    now,
                    &LiveReadyInputProvider { rules },
                );
                let drive = sim
                    .substrate
                    .entities
                    .get(product)
                    .and_then(|unit| unit.locomotor.as_ref())
                    .is_some_and(|loco| {
                        loco.kind == crate::rules::locomotor_type::LocomotorKind::Drive
                    });
                if drive {
                    sim.force_track(product, 0x42, track, Some(rules), ctx.overlay_registry);
                } else {
                    //44DF1C's normal non-Drive cell setter is represented.
                    //Teleport (and the dormant TS Tunnel) take 44DFAD's
                    //piggyback swap and lifetime cleanup, a separate retained
                    //locomotor mechanism.
                    let cell = (
                        ((location.x / 256) as i16).wrapping_add(4) as u16,
                        ((location.y / 256) as i16).wrapping_add(1) as u16,
                    );
                    sim.set_unit_destination(
                        product,
                        crate::sim::components::NavTargetRef::cell(cell.0, cell.1),
                        rules,
                        true,
                    );
                }
                if let Some(product) = sim.substrate.entities.get_mut(product) {
                    //44E1B8 dispatches the existing Foot4D3710 owner.
                    product
                        .foot_speed
                        .set_speed_fraction(crate::util::fixed_math::SIM_HALF);
                }
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .mission
                    .set_handler_state(3);
            } else {
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.close_door(ticks, now);
                entity.mission.set_handler_state(4);
            }
        }
        3 if !tethered => {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.close_door(ticks, now);
            entity.mission.set_handler_state(4);
        }
        4 if phase == DoorPhase::ClosedStable || naval => {
            sim.building_enter_idle_mode(id, false, Some(rules));
        }
        _ => {}
    }
    sim.mission_rate_epilogue_for(rules, id, MissionType::Unload)
}

///Building449540: ask the head Cell's nearest object excluding the producer.
///A busy head is scattered synchronously and the handler remains at state1.
///The empty-head path executes no Scatter and advances on its next dispatch.
fn clear_factory_exit_bib(
    sim: &mut Simulation,
    producer: u64,
    track: crate::sim::components::DriveCoord,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> bool {
    let cell = ((track.x / 256) as i16 as u16, (track.y / 256) as i16 as u16);
    if sim
        .nearest_cell_object(
            cell,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            Some(producer),
        )
        .is_none()
    {
        return false;
    }
    if let Err(error) = sim.scatter_cell_objects(
        cell,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        crate::sim::movement::ScatterFlags::new(true, true),
        rules,
        ctx.overlay_registry,
    ) {
        log::warn!("factory {producer} bib Scatter failed: {error}");
    }
    //44962B..44968F's eight adjacent Cell requests pass a non-null head
    //coordinate. Unit743A50's non-null directional Scatter arm is not yet
    //ported; it cannot be replaced with null Scatter. Trigger: a busy exit
    //whose neighbours also contain objects; effect: neighbours are not
    //pushed away, so clearance may take longer. Head's own Scatter is real.
    true
}

/// `Queue_Mission(mission, false)` then Commence, as both handlers switch
/// missions (`0x00449792`/`0x0044979C`, `0x0044B13E`/`0x0044B148`) and
/// `SuperClass::Launch` starts a silo's Missile (`0x006CDDAD`/`0x006CDDB7`).
pub(crate) fn queue_and_commence(
    sim: &mut Simulation,
    id: u64,
    mission: MissionType,
    rules: &RuleSet,
) {
    let now = sim.session.binary_frame;
    let readiness = LiveReadyInputProvider { rules };
    let _ = sim.mission_queue_exact(id, MissionId::from_known(mission), 0, now, &readiness);
    let _ = sim.mission_commence_exact(id, now);
}

/// `ftol(rate x 900) + RandomRanged(0, 2)` on the Scenario stream, the rate
/// being the MissionControl entry of the building's current mission
/// (`0x005B3A00`).
fn rate_delay(sim: &mut Simulation, frames: i32, multiplier: i32) -> i32 {
    let base = frames;
    let jitter = sim.scenario_rng.next_range_u32_inclusive(0, 2) as i32;
    base.wrapping_mul(multiplier).wrapping_add(jitter)
}

/// `BuildingClass::Mission_Guard` (`0x004496B0`).
fn mission_guard(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    // The head (`0x004496C1..0x004496DF`), armed or not, before anything else
    // the handler reads or draws.
    gattling_step(sim, id, rules, StageCall::Update);
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 0;
    };
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return 0;
    };
    let current = entity
        .mission
        .current()
        .known()
        .unwrap_or(MissionType::Guard);
    // vt+0x2AC, `BuildingClass::Is_Armed 0x00458DB0`.
    if combat_weapon::is_armed(entity, obj) {
        let empty_garrison = obj.can_be_occupied
            && entity
                .passenger_role
                .cargo()
                .is_none_or(|cargo| cargo.is_empty());
        let has_target = entity.attack_target.is_some();
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            // `0x00449701`.
            entity.mission_leaf.set_building_ready_latch(1);
        }
        if !obj.emp_pulse_cannon && !empty_garrison && has_target {
            queue_and_commence(sim, id, MissionType::Attack, rules);
            return 1;
        }
        // `0x004497AF..0x004497DA`: the AARate delay.
        let frames = rules.mission_control.aa_rate_frames(current);
        return rate_delay(sim, frames, 1);
    }
    if obj.has_stupid_guard_mode {
        return STUPID_GUARD_DELAY;
    }
    let status = entity.mission.handler_state();
    if status == 0
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        // `0x0044995D..0x00449966`: the idle body, then status 1.
        entity.begin_building_body(
            crate::sim::building_construction::BuildingBodyMode::Idle,
            sim.session.binary_frame as i32,
        );
        entity.mission.set_handler_state(1);
    }
    if status == 1 && obj.unit_repair {
        //449815..44994A: scan contacts in slot order, raw GetCoords 3D
        //distance <64 (no Building foundation discount), then NEED_MOVE.
        let contacts: Vec<_> = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .radio_contacts
            .iter_live()
            .collect();
        for contact in contacts {
            let Some(unit) = sim.substrate.entities.get(contact) else {
                continue;
            };
            if unit.mission.effective() != MissionId::from_known(MissionType::Enter) {
                continue;
            }
            let center = crate::sim::movement::ground_pose::object_get_coords(
                sim.substrate.entities.get(id).unwrap(),
                sim.resolved_terrain.as_ref(),
            );
            let position = crate::sim::movement::ground_pose::object_get_coords(
                unit,
                sim.resolved_terrain.as_ref(),
            );
            if crate::util::native_x87::distance_3d_leptons(
                [center.x, center.z, center.y],
                [position.x, position.z, position.y],
            ) >= 64
            {
                continue;
            }
            if crate::sim::radio::transmit(
                sim,
                id,
                contact,
                crate::sim::radio::RadioMessage::NeedToMove,
                crate::sim::radio::RadioPayload::default(),
                Some(rules),
            ) == crate::sim::radio::RadioResponse::Roger
            {
                let _ = sim.mission_queue_exact(
                    id,
                    MissionId::from_known(MissionType::Repair),
                    0,
                    sim.session.binary_frame,
                    &LiveReadyInputProvider { rules },
                );
                return 1;
            }
        }
    }
    // `0x004499BB..0x00449A36`: Rate for a depot, three times it otherwise.
    let frames = rules.mission_control.rate_frames(current);
    rate_delay(sim, frames, if obj.unit_repair { 1 } else { 3 })
}

/// `BuildingClass::Mission_Attack` (`0x0044ACF0`).
fn mission_attack(sim: &mut Simulation, id: u64, rules: &RuleSet, ctx: ObjectAiCtx<'_>) -> i32 {
    let Some((target, weapon)) = attack_prelude(sim, id, rules) else {
        return 1;
    };
    let mut code = fire_error_with_overlay(sim, rules, id, target, weapon, ctx.overlay_registry);
    if code == FireError::Facing && voxel_turret_snaps(sim, id, rules, target) {
        code = fire_error_with_overlay(sim, rules, id, target, weapon, ctx.overlay_registry);
    }
    attack_arm(sim, id, rules, target, weapon, code)
}

/// Mission_Attack up to its GetFireError (`0x0044B00F`): with no target, the
/// null-target tail (`0x0044AF86..0x0044AFE0`, `+0x664 = 0` at `0x0044AF9F`),
/// which answers `None`;
/// otherwise SelectWeapon (`0x0044AFF2`) and `+0x6DD = 1` (`0x0044B008`),
/// answering the target and weapon.
fn attack_prelude(sim: &mut Simulation, id: u64, rules: &RuleSet) -> Option<(TargetKind, i32)> {
    let entity = sim.substrate.entities.get(id)?;
    let Some(target) = entity.attack_target.as_ref().map(|attack| attack.target) else {
        let _ = sim.assign_target_represented(id, None, Some(rules));
        clear_support_count(sim, id);
        if !waits(sim, id) {
            queue_and_commence(sim, id, MissionType::Guard, rules);
        }
        return None;
    };
    let weapon = select_weapon(sim, rules, id, Some(target));
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission_leaf.set_building_ready_latch(1);
    }
    Some((target, weapon))
}

/// Mission_Attack's arm for GetFireError's `code` (the table at
/// `0x0044B728`): its actions, then its return.
fn attack_arm(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    target: TargetKind,
    weapon: i32,
    code: FireError,
) -> i32 {
    match code {
        FireError::Ok => {
            fire_arm(sim, id, rules, target, weapon);
            // The tail every OK arm reaches (`0x0044B6D6..0x0044B724`).
            if !gattling_step(sim, id, rules, StageCall::Increase) {
                advance_turret_anim(sim, id);
            }
            1
        }
        // `0x0044B0DE`: the drop tail (`+0x664 = 0` at `0x0044B0ED`).
        FireError::Ammo | FireError::Illegal | FireError::Cant | FireError::Range => {
            let _ = sim.assign_target_represented(id, None, Some(rules));
            clear_support_count(sim, id);
            if waits(sim, id) {
                return 1;
            }
            // `0x0044B113..0x0044B131`, before Commence zeroes the count.
            gattling_step(sim, id, rules, StageCall::Update);
            queue_and_commence(sim, id, MissionType::Guard, rules);
            clear_ai_counter(sim, id);
            1
        }
        // `0x0044B187` and `0x0044B1DE`: after Set_Desired, a Gattling type
        // charges (`0x0044B1C6`, `0x0044B21D`); any other keeps the count, and
        // on REARM advances `+0x148` (`0x0044B235..0x0044B23C`).
        FireError::Facing | FireError::Rearm => {
            aim_turret(sim, id, rules, target);
            if !gattling_step(sim, id, rules, StageCall::Increase) && code == FireError::Rearm {
                advance_turret_anim(sim, id);
            }
            2
        }
        // `0x0044B284`: uncloak, a Gattling type's decay (`0x0044B2AC`), then
        // the `0x0044B14E` tail.
        FireError::Cloaked => {
            crate::sim::combat::world_receiver::start_uncloaking_to_fire(sim, rules, id);
            gattling_step(sim, id, rules, StageCall::Update);
            aim_turret(sim, id, rules, target);
            clear_ai_counter(sim, id);
            1
        }
        // `0x0044B24F`: a Gattling type decays (`0x0044B26C`); any other
        // keeps the count.
        FireError::Busy => {
            gattling_step(sim, id, rules, StageCall::Update);
            1
        }
        // Codes 4, 7 and above 10 (`0x0044B14E`).
        FireError::Rotating | FireError::Moving | FireError::MustDeploy => {
            aim_turret(sim, id, rules, target);
            clear_ai_counter(sim, id);
            1
        }
    }
}

/// `Get_Mission() == Wait` (vt+0x184, `0x0044AFBA`/`0x0044B108`): the current
/// mission, else the queued one.
fn waits(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).is_some_and(|entity| {
        entity.mission.effective() == MissionId::from_known(MissionType::Deliberate)
    })
}

/// `+0x664 = 0`, the Prism support count, after both tails' SetTarget(0).
fn clear_support_count(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.prism_support_count = 0;
    }
}

/// `+0xC4 = 0`: Mission_Attack's zeroes (`0x0044B131`, `0x0044B174`,
/// `0x0044B1CB`, `0x0044B222`, `0x0044B271`, `0x0044B2B1`, `0x0044B6F4`) and
/// Mission_Guard's (`0x004496DF`).
fn clear_ai_counter(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.clear_ai_counter();
    }
}

/// Whether the building's type is a Gattling one (TechnoType `+0xCD5`).
fn is_gattling(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    sim.substrate.entities.get(id).is_some_and(|entity| {
        sim.object_type(entity.type_ref(), rules)
            .is_some_and(|obj| obj.is_gattling)
    })
}

/// A Gattling type's stage call with the mission's `+0xC4` count, then
/// `+0xC4 = 0`, as both handlers make it. Answers whether the type is a
/// Gattling one; any other makes no call and keeps the count.
fn gattling_step(sim: &mut Simulation, id: u64, rules: &RuleSet, call: StageCall) -> bool {
    if !is_gattling(sim, id, rules) {
        return false;
    }
    let Some(ticks) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.mission.ai_counter() as i32)
    else {
        return false;
    };
    match call {
        StageCall::Increase => sim.gattling_increase(id, rules, ticks),
        StageCall::Update => sim.gattling_update(id, rules, ticks),
    }
    clear_ai_counter(sim, id);
    true
}

/// `+0x148 += 1`, the voxel turret's animation counter.
fn advance_turret_anim(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.turret_anim_frame = entity.turret_anim_frame.wrapping_add(1);
    }
}

/// BuildingClass::Update's Gattling block once the Techno AI has returned
/// with the building alive (`0x0043FE5B`; a dead one returns from the Update),
/// for a Gattling type only:
/// - a value above 0 advances `+0x148` (`0x0043FE69..0x0043FE88`);
/// - unless the building's mission, else its queued one, is Attack
///   (`0x0043FEBE..0x0043FECB`): the idle decay
///   ([`crate::sim::combat::gattling::GattlingState::idle_decay`]) once more
///   than `GuardAreaTargetingDelay + 5` frames have passed since the last
///   shot (`0x0043FEE9..0x0043FF67`: `Frame - LastFireFrame`, signed, whatever
///   the target), then `+0x148` again while the value is above 0
///   (`0x0043FF6C..0x0043FF8B`).
pub(super) fn gattling_idle(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    if !super::ai_alive(sim, id) {
        return;
    }
    let Some(obj) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
        .filter(|obj| obj.is_gattling)
    else {
        return;
    };
    let now = sim.session.binary_frame as i32;
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if entity.gattling.value() > 0 {
        entity.turret_anim_frame = entity.turret_anim_frame.wrapping_add(1);
    }
    if entity.mission.effective() == MissionId::from_known(MissionType::Attack) {
        return;
    }
    let since = now.wrapping_sub(entity.last_fire_frame as i32);
    if since > rules.general.guard_area_targeting_delay.wrapping_add(5) {
        let elite = is_elite(entity);
        entity.gattling.idle_decay(&obj.gattling_stages, elite);
    }
    if entity.gattling.value() > 0 {
        entity.turret_anim_frame = entity.turret_anim_frame.wrapping_add(1);
    }
}

/// The OK arm (`0x0044B2BC`): a Prism tower forwards ([`prism_arm`]); an
/// `IsAnimDelayedFire=` building arms its delayed shot (`0x0044B630..
/// 0x0044B666`: `+0x714` = DelayedFireDelay, `+0x708` = the weapon, `+0x704`
/// = 1) for [`process_delayed_fire`]; any other asks the combat phase for its
/// FireAt (`0x0044B6D0`) at the visit's target with SelectWeapon's weapon,
/// both of which the request carries: the tail's charge may step a Gattling
/// type's stage, and a later object may retarget the building, before the
/// combat phase fires.
fn fire_arm(sim: &mut Simulation, id: u64, rules: &RuleSet, target: TargetKind, weapon: i32) {
    let Some(obj) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| rules.object(sim.interner.resolve(entity.type_ref())))
    else {
        return;
    };
    if rules.is_prism_type(obj) {
        prism_arm(sim, id, rules, obj);
        return;
    }
    match rules
        .art()
        .resolve_metadata_entry(&obj.id, &obj.image)
        .filter(|art| art.is_anim_delayed_fire)
    {
        Some(art) => {
            let slot = if weapon == 1 {
                WeaponSlot::Secondary
            } else {
                WeaponSlot::Primary
            };
            arm_delayed_fire(
                sim,
                id,
                rules,
                obj,
                art.delayed_fire_delay,
                DelayedFire::Weapon(slot),
            );
        }
        None => {
            sim.fire_requests
                .buildings
                .insert(id, BuildingShot::Mission { weapon, target });
        }
    }
}

/// Arms a delayed fire (`+0x714` = `delay`, `+0x704` and `+0x708..+0x710`
/// from `fire`) and swaps the building's anims: the Active anim's slot 3 is
/// emptied (`0x00451E40`) and the SpecialAnim plays in slot 10, its Damaged
/// variant at or below ConditionYellow (`0x00451890`, the Tesla Coil's and
/// the Prism tower's charge, with their `Report=`). Nothing in play gives the
/// Active anim back when the SpecialAnim ends: BuildingClass's vt+0x28
/// (`0x0044E9AA`) replays it only for a `Grinding=` type, so it returns with a
/// power restore.
fn arm_delayed_fire(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    obj: &crate::rules::object_type::ObjectType,
    delay: i32,
    fire: DelayedFire,
) {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    entity.pending_building_fire = Some(PendingBuildingFire {
        remaining_ticks: delay,
        fire,
    });
    let damaged =
        requested_damage_state(entity.health, obj.strength, rules.general.condition_yellow);
    sim.clear_building_anim_slot(id, ACTIVE_ANIM_SLOT);
    let _ = sim.set_building_anim_slot(id, SPECIAL_ANIM_SLOT, damaged, false, 0, rules);
}

/// Mission_Attack's PrismType arm (`0x0044B310..0x0044B62B`), which never
/// reaches FireAt and does not read IsAnimDelayedFire or SelectWeapon's
/// weapon. While the tower's support count (`+0x664`) is below
/// `PrismSupportMax` (`0x0044B349`), the tower its House can recruit
/// ([`prism_supporter`]) is armed with a support beam aimed at this tower's
/// weapon-0 FLH and the count goes up (`0x0044B4CB..0x0044B590`). With none,
/// or at the cap, the tower arms its own delayed shot with weapon 0
/// (`0x0044B595..0x0044B62B`). Both arm through [`arm_delayed_fire`], with
/// the type's `DelayedFireDelay=` (`+0x16EC`; the supporter is a PrismType
/// tower too), and the arm returns 1 either way, so one tower is recruited per
/// visit.
fn prism_arm(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    obj: &crate::rules::object_type::ObjectType,
) {
    let delay = rules
        .art()
        .resolve_metadata_entry(&obj.id, &obj.image)
        .map_or(0, |art| art.delayed_fire_delay);
    let Some(master) = sim.substrate.entities.get(id) else {
        return;
    };
    let supporter = (master.prism_support_count < rules.general.prism_support.max)
        .then(|| prism_supporter(sim, id, rules, obj))
        .flatten();
    let Some(supporter) = supporter else {
        arm_delayed_fire(
            sim,
            id,
            rules,
            obj,
            delay,
            DelayedFire::Weapon(WeaponSlot::Primary),
        );
        return;
    };
    if let Some(master) = sim.substrate.entities.get_mut(id) {
        master.prism_support_count = master.prism_support_count.wrapping_add(1);
    }
    // `vt+0xB0(out, 0, {0, 0, 0})` (`0x0044B4F6`): the master's weapon-0 FLH.
    let Some(master) = sim.substrate.entities.get(id) else {
        return;
    };
    let to = fire_coord::fire_coordinate(
        sim,
        rules,
        &fire_coord::FireSource::of_entity(master),
        obj,
        0,
        (master.weapon_burst.index() & 1) as u8,
        crate::rules::flh::Flh::default(),
    )
    .coord;
    arm_delayed_fire(
        sim,
        supporter,
        rules,
        obj,
        delay,
        DelayedFire::SupportBeam { to },
    );
}

/// The Prism arm's walk (`0x0044B357..0x0044B4BD`) over the master's House's
/// buildings (House+0x68) in vector order. A tower is admitted when it is
/// alive (`+0x90`), of PrismType (the master's own type), its rearm timer
/// has run out, it counts no delayed fire down (`+0x714 == 0`; the mode is
/// not read), no Floating Disc drains it (`0x0070FEC0`), its mission
/// (current, else queued) is not Attack, it is not the master, and its
/// distance from the master (`Distance3D` of the two Locations: Sqrt_Approx,
/// ftol) is at most the master's weapon 1 range (`vt+0x168(1)`, the Secondary
/// `PrismSupport`'s 2048 in retail; inclusive, `0x0044B49A`). The nearest
/// wins; a tie keeps the lower vector index (`0x0044B49E..0x0044B4AA`).
/// Power, EMP, the tower's own target, a build-up or a sale are not asked.
///
/// FireAt starts the shooter's rearm inside its visit
/// (`0x006FF2B2..0x006FF2BB`), before the visits after it; VERA's combat
/// phase starts it after the Logic pass (module doc), so a tower whose shot
/// this pass has already asked for counts as rearming. That is FireAt's
/// answer for every ROF above zero; a zero ROF, which native leaves run out,
/// is not told apart.
fn prism_supporter(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
    obj: &crate::rules::object_type::ObjectType,
) -> Option<u64> {
    let master = sim.substrate.entities.get(id)?;
    let house = sim.houses.get(&master.owner())?;
    let now = sim.session.binary_frame as i32;
    let location = |entity: &crate::sim::game_entity::GameEntity| {
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        [coord.x, coord.y, coord.z]
    };
    let origin = location(master);
    let attack = MissionId::from_known(MissionType::Attack);
    let mut best: Option<(u64, i32)> = None;
    for &candidate_id in house.base_projection.buildings() {
        let Some(candidate) = sim.substrate.entities.get(candidate_id) else {
            continue;
        };
        let admitted = candidate.lifecycle.object_alive
            && candidate.type_ref() == master.type_ref()
            && candidate.rearm_timer.remaining(now) == 0
            && !sim.fire_requests.buildings.contains_key(&candidate_id)
            && candidate
                .pending_building_fire
                .map_or(0, |pending| pending.remaining_ticks)
                == 0
            && candidate.draining_me.is_none()
            && candidate.mission.effective() != attack
            && candidate_id != id;
        if !admitted {
            continue;
        }
        let distance = crate::util::native_x87::distance_3d_leptons(origin, location(candidate));
        let range = combat_weapon::weapon_range(
            master,
            obj,
            1,
            &sim.substrate.entities,
            rules,
            &sim.interner,
        );
        if distance > range {
            continue;
        }
        if best.is_none_or(|(_, nearest)| distance < nearest) {
            best = Some((candidate_id, distance));
        }
    }
    best.map(|(supporter, _)| supporter)
}

/// `BuildingClass::ProcessDelayedFire` (`0x004503F0`), from Update
/// (`0x004400F4`) once Health is above zero, whatever the mission, power or
/// owner. An armed delayed fire (`+0x704` nonzero) counts `+0x714` down with a
/// signed pre-decrement and ends when it reaches zero or below, `+0x714 = 0`
/// (`0x00450401..0x00450419`); every end clears the mode (`0x00450452`,
/// `0x004504D7`).
/// - A shot (mode 1, `0x0045045E..0x00450492`) needs a target and
///   GetFireError(target, `+0x708`, range) answering OK; then its FireAt at
///   that target is asked of the combat phase ([`BuildingShot::Delayed`]),
///   whose bullet takes the support bonus ([`Simulation::take_support_bonus`]).
///   Otherwise the shot is dropped and `+0x664` kept.
/// - A support beam (mode 2, `0x0044ABD0`): construct the beam, then
///   `+0x664 = 0` (`0x0044ACCA`) and the downtime: the rearm timer
///   becomes {Frame, `PrismSupportDelay=`} (`0x0044ACD0..0x0044ACDC`). No
///   check, damage or Scenario draw.
pub(super) fn process_delayed_fire(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) {
    let now = sim.session.binary_frame as i32;
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    // Health 0 returned before the call (`0x00440072`).
    if entity.dying || entity.health.current == 0 {
        return;
    }
    let Some(pending) = entity.pending_building_fire.as_mut() else {
        return;
    };
    pending.remaining_ticks = pending.remaining_ticks.wrapping_sub(1);
    if pending.remaining_ticks > 0 {
        return;
    }
    let fire = pending.fire;
    entity.pending_building_fire = None;
    match fire {
        DelayedFire::Weapon(slot) => {
            let Some(target) = entity.attack_target.as_ref().map(|attack| attack.target) else {
                return;
            };
            let weapon = match slot {
                WeaponSlot::Primary => 0,
                WeaponSlot::Secondary => 1,
            };
            if fire_error_with_overlay(sim, rules, id, target, weapon, ctx.overlay_registry)
                == FireError::Ok
            {
                sim.fire_requests
                    .buildings
                    .insert(id, BuildingShot::Delayed { slot, target });
            }
        }
        DelayedFire::SupportBeam { to } => {
            crate::sim::combat::laser::support(sim, rules, id, to);
            let entity = sim
                .substrate
                .entities
                .get_mut(id)
                .expect("live support source");
            entity.prism_support_count = 0;
            entity
                .rearm_timer
                .start(now, rules.general.prism_support.delay);
        }
    }
}

/// ProcessDelayedFire's support bonus (`0x004504A8..0x004504C7`):
/// `((PrismSupportModifier * count + 100) << 8) / 100`, the `imul` and add
/// wrapping in 32 bits and the division unsigned (`mul 0x51EB851F; shr edx,
/// 5`), in the bullet's 1/256 damage units.
fn support_multiplier(modifier: i32, count: i32) -> i32 {
    let scaled = (modifier.wrapping_mul(count).wrapping_add(100) as u32) << 8;
    (scaled / 100) as i32
}

/// Whether the building's weapon 0 (vt+0x3F8, `0x004526F0`) has a WeaponType
/// whose projectile is not `AA=` (BulletType `+0x2A4`). BuildingClass::SetTarget
/// admits every target when it has not (`0x00443BC0..0x00443BED`), and
/// ReceiveDamage's retaliation block stops (`0x004429B4..0x004429E5`).
pub(super) fn building_weapon0_aims(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    weapon_at_index(sim, rules, id, 0).is_some_and(|weapon| {
        !weapon
            .projectile
            .as_deref()
            .and_then(|projectile| rules.projectile(projectile))
            .is_some_and(|projectile| projectile.aa)
    })
}

/// The voxel-turret retry (`0x0044B017..0x0044B0CC`): a building with a turret
/// whose `TurretAnimIsVoxel=` is set, within one `ROT=` step of the target's
/// direction (vt+0x4E8 at `0x0044B056`, [`fire_coord::building_direction_to`];
/// `abs(low-byte ROT << 8)` as signed16, without FacingClass
/// SetROT's clamp; any miss at ROT 0), snaps its turret (`0x0044B0AC`) and
/// asks GetFireError again. Original decisions: building_fire_turn.json.
fn voxel_turret_snaps(sim: &mut Simulation, id: u64, rules: &RuleSet, target: TargetKind) -> bool {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return false;
    };
    if !(obj.has_turret && obj.turret_anim_is_voxel) {
        return false;
    }
    let Some(direction) = fire_coord::building_direction_to(sim, rules, entity, target) else {
        return false;
    };
    let delta = i32::from(entity.body_facing_current(now).wrapping_sub(direction) as i16);
    let rot_step = i32::from(((obj.turret_rot as u8 as u16) << 8) as i16).abs();
    if obj.turret_rot != 0 && delta.abs() > rot_step {
        return false;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.body_facing.snap(direction, now);
    }
    true
}

/// `+0x388.Set_Desired(vt+0x4E8(Target))` (`0x0044B16F`, `0x0044B1A8`,
/// `0x0044B1FF`; [`fire_coord::building_direction_to`]), at the `ROT=` rate
/// `BuildingClass::Init` gave it: the turret of a `Turret=yes` type, the body
/// of any other.
fn aim_turret(sim: &mut Simulation, id: u64, rules: &RuleSet, target: TargetKind) {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(desired) = fire_coord::building_direction_to(sim, rules, entity, target) else {
        return;
    };
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.body_facing.set(desired, now);
    }
}

/// The end of `BuildingClass::Update` (`0x00440378..0x004403C6`), whatever the
/// mission: a target out of range of the weapon SelectWeapon picks for it
/// (`vt+0x3AC`, `0x006F7780`) is dropped; an aircraft only while it is low
/// (`AircraftClass 0x0041B980`, the V3 and Dreadnought rockets asking their
/// locomotor: [`crate::sim::movement::air_movement::is_low_flying`]).
pub(super) fn range_drop(sim: &mut Simulation, id: u64, rules: &RuleSet, ctx: ObjectAiCtx<'_>) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    // Health 0 returned before it (`0x00440072`).
    if entity.dying || entity.health.current == 0 {
        return;
    }
    let Some(target) = entity.attack_target.as_ref().map(|attack| attack.target) else {
        return;
    };
    let weapon = select_weapon(sim, rules, id, Some(target));
    if can_fire_at(sim, rules, id, target, weapon, ctx.overlay_registry) {
        return;
    }
    if let TargetKind::Entity(target_id) = target
        && sim.substrate.entities.get(target_id).is_some_and(|target| {
            target.category == EntityCategory::Aircraft
                && !crate::sim::movement::air_movement::is_low_flying(
                    target,
                    sim.resolved_terrain.as_ref(),
                    Some((rules, &sim.interner)),
                )
        })
    {
        return;
    }
    let _ = sim.assign_target_represented(id, None, Some(rules));
}

impl Simulation {
    /// The bonus ProcessDelayedFire writes on the bullet its FireAt returned
    /// (`0x00450496..0x004504CD`): a building with a support count (`+0x664`)
    /// gives the bullet the [`support_multiplier`] of it (`+0x150`) and
    /// restarts the count; with none the bullet keeps Construct's
    /// [`ProjectilePayload::UNSCALED`]. The combat phase asks it for a
    /// launched delayed shot only ([`BuildingShot::Delayed`]); FireAt's early
    /// exits and a refused launch return no bullet and keep the count.
    pub(crate) fn take_support_bonus(&mut self, id: u64, rules: &RuleSet) -> i32 {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return ProjectilePayload::UNSCALED;
        };
        let count = entity.prism_support_count;
        if count == 0 {
            return ProjectilePayload::UNSCALED;
        }
        entity.prism_support_count = 0;
        support_multiplier(rules.general.prism_support.modifier, count)
    }

    /// `BuildingClass::SetTarget` (vt+0x3C8, `0x00443B90`)'s admission of a
    /// requested target for a building: a Selling building (`+0xAC`) or one
    /// not operational (vt+0x350, `0x004555D0`) takes none; any other keeps a
    /// target its slot-0 weapon cannot aim (none, or an `AA=` projectile,
    /// BulletType `+0x2A4`) or one in range of SelectWeapon's weapon
    /// (vt+0x3AC). Other objects admit every target here.
    ///
    /// RESIDUALS:
    /// - InRange's line of fire reads no OverlayTypeClass table here, because
    ///   `Simulation` holds none, so a wall between them is not seen. Trigger:
    ///   retaliation against, or an order onto, a target behind a wall.
    ///   Effect: the building takes a target native refuses; its next
    ///   Mission_Attack asks GetFireError with the table and drops it, and
    ///   the Guard visit that took it queued Attack without its
    ///   `RandomRanged(0, 2)`. Frequency: rare. Later owner: the overlay table
    ///   moving into `Simulation`.
    /// - The restore after a cell target expires
    ///   (`combat::combat_aoe::expire_cell_target_references`) holds only the
    ///   entity store and puts the suspended target back without this
    ///   admission. Trigger: a building whose retaliation suspended its
    ///   mission and whose later cell target expires. Effect: a Selling,
    ///   unpowered or out-of-range building keeps that target until its next
    ///   Mission_Attack or range drop. Frequency: rare. Later owner: that
    ///   restore moving onto `Simulation`.
    pub(crate) fn building_admits_target(
        &self,
        id: u64,
        requested: Option<TargetKind>,
        rules: &RuleSet,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        if entity.category != EntityCategory::Structure {
            return true;
        }
        if entity.mission.current().known() == Some(MissionType::Selling)
            || self.building_operational_state(id, rules) == Some(false)
        {
            return false;
        }
        let Some(target) = requested else {
            return true;
        };
        if !building_weapon0_aims(self, rules, id) {
            return true;
        }
        let weapon = select_weapon(self, rules, id, Some(target));
        can_fire_at(self, rules, id, target, weapon, None)
    }

    /// The phase-level combat fixture's stand-in for a building's object-pass
    /// visit (`combat::receiver_fixture`): Mission_Attack when the building
    /// holds a target, then ProcessDelayedFire, whose requests the receiver
    /// then serves, as BuildingClass::Update runs them before the frame's
    /// combat. The fixture honours no mission or dispatch timer.
    #[cfg(test)]
    pub(crate) fn fixture_building_visit(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let ctx = ObjectAiCtx {
            overlay_registry,
            ..Default::default()
        };
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.attack_target.is_some())
        {
            let _ = mission_attack(self, id, rules, ctx);
        }
        process_delayed_fire(self, id, rules, ctx);
    }
}

#[cfg(test)]
#[path = "building_missions_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "prism_support_tests.rs"]
mod prism_tests;

#[cfg(test)]
#[path = "building_gattling_tests.rs"]
mod gattling_tests;

#[cfg(test)]
#[path = "building_construction_tests.rs"]
mod construction_tests;

#[cfg(test)]
#[path = "building_opening_oracle_tests.rs"]
mod opening_oracle_tests;

#[cfg(test)]
#[path = "factory_unload_tests.rs"]
mod factory_unload_tests;

#[cfg(test)]
#[path = "../../docking/building_repair_service_oracle_tests.rs"]
mod repair_service_oracle_tests;
