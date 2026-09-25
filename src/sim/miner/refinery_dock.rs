//! Harvester refinery dock as the native Enter and Unload missions.
//!
//! Mission_Harvest state 3 queues Enter. Each `FootClass::Mission_Enter`
//! dispatch (`0x004D9290`) sends DOCKING (0x0E) to the refinery, whose
//! receiver (`radio::receive`, `0x0043C7E9`) drives the handshake: MOVE_HERE
//! to the pad NW+(3,1), then, once the miner stands in it, TETHER and
//! PREPARE_TO_DOCK, which turns the hull to 0x4000 and sends DOCK_NOW back
//! (or the Drive track end sends it, [`per_cell_dock_now`]). The refinery
//! answers DOCK_NOW by queueing Unload, whose harvester branch
//! ([`mission_unload`], `0x0073DEE0`) dumps the cargo on the Unit+0xF8
//! StageClass gate and hands back to Harvest.
//!
//! Native evidence: tools/spatial_oracle/refinery_dock.json — `mission_enter`,
//! `mission_unload`, `per_cell` and `stage_tick` rows (original executions of
//! 0x004D9290, 0x0073D630, 0x0073A31F..0x0073A5EA and 0x006FABC4).
//!
//! Both harvester kinds run it. A `Teleporter=` type (the Chrono Miner)
//! reaches the pad by the Unit setter's Teleporter arm (`0x007423CD`): while
//! the refinery is its radio contact the MOVE_HERE keeps the Teleport
//! locomotor and warps onto the pad; every other destination drives.
//! Mission_Enter re-runs that setter on each dispatch
//! ([`teleporter_reassign`]). Evidence: tools/spatial_oracle/cmin_dock.json.
//!
//! RESIDUALS (named trigger, effect, frequency):
//! - `0x0070D8F0` (Unit+0x500 pending entry) is unrepresented: Mission_Enter
//!   with no target always reaches Enter_Idle_Mode. Frequency: zero in the
//!   dock chain (no represented writer).
//! - Mission_Enter re-assigns only a Cell or NULL NavCom (`0x004D93E8`); an
//!   object NavCom has no setter here and is left as it is. Frequency: zero
//!   in the dock chain (MOVE_HERE and Mission_Harvest set cells).
//! - Weeder= types (`GiveWeed 0x004F9700`) are not represented; no stock type.
//! - The payout keeps VERA's integer economy (`economy.rs`): identical to
//!   native for every retail value (whole bales, IncomeMult 1.0, PurifierBonus
//!   .25); a non-exact IncomeMult differs (native 0.9f pays 899 per 1000 ore,
//!   VERA 900 — `unload_gate_income_mult` row).
//! - Storage is VERA's two resource kinds, not native's four tiberium slots
//!   (`miner_system::harvest_ore_tick`); no stock map places TIB2/TIB3.
//! - The Per_Cell DOCK_NOW refusal's Unit Scatter (`0x0073A5CE..0x0073A5E4`,
//!   answered by a refinery in its Selling mission) is not wired. Frequency:
//!   zero today — VERA's sale is synchronous, so no refinery is ever seen
//!   mid-sell-down; the sale's RUN_AWAY broadcast carries the miner instead.
//!
//! A refinery sold (`BuildingClass::Sell`) or destroyed (the NowDead contact
//! loop, `Simulation::building_now_dead_contacts`) under an unloading miner
//! sends it RUN_AWAY (0x17): the latch drops and Harvest takes over
//! (`radio::receive`, Unit `0x00737A98`).
//!
//! ## Dependency rules
//! - Part of sim/ — sim/radio, sim/mission, sim/movement, sim/world.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{BaleDepositEvent, NavTargetRef};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::Simulation;

use super::{MinerKind, ResourceType};

/// The body facing the Unload window centres on (`0x0073DF8A`).
const UNLOAD_FACING: u16 = crate::sim::radio::receive::DOCK_FACING;

/// Mission_Unload harvester Status (+0xBC) values: dumping and finishing.
const UNLOAD_DUMPING: u32 = 3;
const UNLOAD_FINISHING: u32 = 4;

/// Whether `id` runs the native dock missions: a harvester (War or Chrono
/// Miner). The Slave Miner deploys instead.
pub(crate) fn native_dock_miner(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.miner.as_ref())
        .is_some_and(|miner| miner.kind != MinerKind::Slave)
}

/// `FootClass::Mission_Enter @ 0x004D9290` for a harvester. Returns the
/// dispatch delay.
pub(crate) fn mission_enter(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    // 0x004D9294..0x004D92AC: Contacts[0], else the Techno behind ArchiveTarget.
    let target = entity
        .radio_contacts
        .slot(0)
        .or(match entity.archive_target() {
            Some(crate::sim::combat::TargetKind::Entity(archived)) => Some(archived),
            _ => None,
        });
    match target {
        None => {
            // 0x004D9425..0x004D9466: unless the NavCom is a Unit or an
            // Aircraft, Enter_Idle_Mode(0, 1); then Commence.
            let nav_is_mover = match entity.navigation.nav_com {
                Some(
                    NavTargetRef::Entity { id: nav }
                    | NavTargetRef::Object { id: nav }
                    | NavTargetRef::Building { id: nav },
                ) => sim.substrate.entities.get(nav).is_some_and(|other| {
                    matches!(
                        other.category,
                        EntityCategory::Unit | EntityCategory::Aircraft
                    )
                }),
                _ => false,
            };
            if !nav_is_mover {
                sim.unit_enter_idle_mode(id, Some(rules));
            }
            let _ = sim.mission_commence_exact(id, now);
        }
        Some(target) => {
            let reply = radio::transmit(
                sim,
                id,
                target,
                RadioMessage::CanDock,
                RadioPayload::default(),
                Some(rules),
            );
            let tethered = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.dock_entered_with.is_some());
            if reply != RadioResponse::Roger && !tethered {
                // 0x004D92CE..0x004D92E8.
                radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules));
                sim.unit_enter_idle_mode(id, Some(rules));
            } else if !pop_nav_queue(sim, rules, id) {
                teleporter_reassign(sim, rules, id);
            }
        }
    }
    // 0x004D946C..0x004D9497: the CURRENT mission's Rate (a Commence above may
    // have changed it).
    sim.mission_rate_epilogue_for(rules, id, MissionType::Enter)
}

/// `0x004D92ED..0x004D93E3`: with no NavCom and a waypoint queued, a
/// piggyback that `Is_Ok_To_End` allows ends first (`0x004D9309..0x004D937C`,
/// no `Is_Piggybacking` test), then the class setter takes `NavQueue[0]` with
/// the flag 0 (the queue is kept) and the first entry is deleted. Only Cell
/// waypoints are represented. Returns whether this arm ran.
fn pop_nav_queue(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    if entity.navigation.nav_com.is_some() || entity.navigation.nav_queue.is_empty() {
        return false;
    }
    let _ = crate::sim::movement::locomotor_owner::try_restore_primary(entity);
    let queue = entity.navigation.nav_queue.clone();
    if let NavTargetRef::Cell { rx, ry } = queue[0] {
        sim.set_unit_cell_destination(id, (rx, ry), rules);
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.navigation.nav_queue = queue[1..].to_vec();
    }
    true
}

/// `0x004D93E8..0x004D941D`: a `Teleporter=` type drops NavCom and NavComAux
/// raw and hands the old NavCom back to the class setter with the flag 1. On
/// the pad approach that re-runs the Teleporter arm and the Teleport Move_To;
/// after the warp NavCom is NULL and the setter returns at once (`0x00741A80`).
fn teleporter_reassign(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if !sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.teleporter)
    {
        return;
    }
    let nav = entity.navigation.nav_com;
    if !matches!(nav, None | Some(NavTargetRef::Cell { .. })) {
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        crate::sim::movement::foot_stop_moving(entity);
    }
    match nav {
        Some(NavTargetRef::Cell { rx, ry }) => {
            sim.set_unit_cell_destination(id, (rx, ry), rules);
        }
        _ => {
            sim.set_unit_null_destination(id, Some(rules));
        }
    }
}

/// `UnitClass::Mission_Unload @ 0x0073D630`, harvester branch `0x0073DEE0`.
/// Returns the dispatch delay.
pub(crate) fn mission_unload(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    // A 0x0073DEE0: without any radio contact the unload is abandoned.
    if entity.radio_contacts.is_empty() {
        sim.unit_enter_idle_mode(id, Some(rules));
        set_unload_latch(sim, rules, id, false);
        stop_if_moving(sim, id);
        commence_if_ready(sim, rules, id);
        return 1;
    }
    // B 0x0073DF56: the hull must sit in the 8-bit East window
    // (raw 0x3F80..=0x407F); otherwise turn (unless the turret is mid-swing).
    let raw = entity
        .body_facing
        .as_ref()
        .map_or(u16::from(entity.facing) << 8, |body| body.current(now));
    if (((u32::from(raw) >> 7) + 1) & 0x1FE) != 0x80 {
        if !entity.turret_rotation_latch
            && let Some(entity) = sim.substrate.entities.get_mut(id)
        {
            crate::sim::movement::drive_do_turn(entity, UNLOAD_FACING, now);
        }
        return 5;
    }
    // C 0x0073DFBD: the first pass arms the stage and the latch.
    let unloading = entity
        .miner
        .as_ref()
        .is_some_and(|miner| miner.unload_active);
    if !unloading {
        if let Some(miner) = sim
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.miner.as_mut())
        {
            miner.stage_value = 0;
            miner.stage_rate = 1;
            miner.stage_timer.arm(now, 1);
        }
        set_unload_latch(sim, rules, id, true);
        if let Some(building) = unload_building(sim, id) {
            crate::sim::world::building_anim::start_refinery_unload(sim, rules, building);
        }
        set_status(sim, id, UNLOAD_DUMPING);
        return epilogue(sim, rules, id);
    }
    match entity.mission.handler_state() {
        UNLOAD_DUMPING => unload_dumping(sim, rules, id),
        UNLOAD_FINISHING => unload_finishing(sim, rules, id),
        _ => epilogue(sim, rules, id),
    }
}

/// E 0x0073E2BF: the dump state, re-reading the building west of the miner
/// on every dispatch.
fn unload_dumping(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    let now = sim.session.binary_frame;
    let Some(building) = unload_building(sim, id) else {
        // 0x0073E311..0x0073E350: no building → OVER_OUT, Queue(Harvest, 1).
        radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules));
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Harvest),
            1,
            now,
            &LiveReadyInputProvider { rules },
        );
        return epilogue(sim, rules, id);
    };
    let stage = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.miner.as_ref())
        .map_or(0, |miner| miner.stage_value);
    // 0x0073E355..0x0073E374: `HarvesterDumpRate × 900 <= Value`. The
    // integer stage crosses at ceil(rate × 900) (`GeneralRules`).
    if stage >= i32::from(rules.general.harvester_dump_frames) {
        crate::sim::world::building_anim::begin_refinery_unload_gate(sim, rules, building);
        let drained = sim
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.miner.as_mut())
            .and_then(|miner| drain_first_slot(&mut miner.cargo));
        if let Some((value, bales)) = drained {
            pay_refinery_owner(sim, rules, building, value, bales);
            if let Some(miner) = sim
                .substrate
                .entities
                .get_mut(id)
                .and_then(|entity| entity.miner.as_mut())
            {
                miner.stage_value = 0;
            }
            sim.bale_events.push(BaleDepositEvent {
                building_id: building,
                tick: sim.session.tick,
                drained: true,
                empty: false,
            });
        } else {
            // 0x0073E4DC..0x0073E534: empty → ProductionAnim, state 4,
            // SpecialAnim destroyed.
            crate::sim::world::building_anim::end_refinery_unload_empty(sim, rules, building);
            set_status(sim, id, UNLOAD_FINISHING);
            sim.bale_events.push(BaleDepositEvent {
                building_id: building,
                tick: sim.session.tick,
                drained: false,
                empty: true,
            });
        }
    }
    // X 0x0073E539..0x0073E5AC: a new order (NavCom and a queued mission
    // other than Harvest) ends the unload early, gate or not.
    if new_order_pending(sim, id) {
        crate::sim::world::building_anim::end_refinery_unload_empty(sim, rules, building);
        set_status(sim, id, UNLOAD_FINISHING);
    }
    1
}

/// F 0x0073E17F: wait out the refinery's ProductionAnim, drop the latch and
/// hand back to Harvest (or to the new order).
fn unload_finishing(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    let now = sim.session.binary_frame;
    if let Some(building) = unload_building(sim, id)
        && let Some(building) = sim.substrate.entities.get(building)
        && sim
            .object_type(building.type_ref(), rules)
            .is_some_and(|object| object.refinery)
        && building.building_anim_slots[8].is_some()
    {
        return 1;
    }
    set_unload_latch(sim, rules, id, false);
    if new_order_pending(sim, id) {
        stop_if_moving(sim, id);
        commence_if_ready(sim, rules, id);
    } else {
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::Harvest),
            0,
            now,
            &LiveReadyInputProvider { rules },
        );
        // Not ready (Unit Ready_To_Commence `0x00744270` refuses): no
        // OVER_OUT, so the contact outlives the unload until the AI host
        // commences Harvest. A Chrono Miner's ore destination then keeps the
        // Teleport (the Unit setter's Teleporter arm) and warps, and the
        // warp's Per_Cell release drops the contact. Native reading; no row.
        if sim.mission_ready_to_commence(id, rules) {
            radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules));
            let _ = sim.mission_commence_exact(id, now);
        }
    }
    epilogue(sim, rules, id)
}

/// The Unit+0xF8 StageClass tick of `TechnoClass::AI` (`0x006FABC4..
/// 0x006FAC31`), after the mission dispatch: an expired timer with a nonzero
/// rate adds the step (1) and restarts at the rate. Mission_Harvest state 1
/// counts its cut gate and Mission_Unload its dumps on it.
pub(crate) fn tick_stage(sim: &mut Simulation, id: u64) {
    let now = sim.session.binary_frame;
    let Some(miner) = sim
        .substrate
        .entities
        .get_mut(id)
        .and_then(|entity| entity.miner.as_mut())
    else {
        return;
    };
    if miner.stage_rate == 0 || !miner.stage_timer.due(now) {
        return;
    }
    miner.stage_value = miner.stage_value.saturating_add(1);
    miner.stage_timer.arm(now, miner.stage_rate);
}

/// Unit `Per_Cell_Process(2)` Enter arm at a Drive track end
/// (`0x0073A558..0x0073A5E4`): a tethered unit on Enter whose first contact
/// is the building in the cell north of it sends that building DOCK_NOW. The
/// depot arm before it (`0x0073A31F..0x0073A54A`, unit standing in the
/// GetDockCoord cell) belongs to the depot flow and is not represented.
pub(crate) fn per_cell_dock_now(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.dock_entered_with.is_none()
        || entity.mission.effective() != MissionId::from_known(MissionType::Enter)
    {
        return;
    }
    let Some(contact) = entity.radio_contacts.slot(0) else {
        return;
    };
    let (x, y) = (entity.position.rx, entity.position.ry);
    let north = sim.substrate.occupancy.first_building_on_layer(
        x,
        (y as i16).wrapping_sub(1) as u16,
        MovementLayer::Ground,
    );
    if north != Some(contact) {
        return;
    }
    let reply = radio::transmit(
        sim,
        id,
        contact,
        RadioMessage::DockNow,
        RadioPayload::default(),
        Some(rules),
    );
    // 0x0073A5CE..0x0073A5E4: an answer other than 1 or 5 scatters the unit
    // (a refinery being sold) — RESIDUAL, module doc.
    let _ = reply;
}

/// Unit `Per_Cell_Process(2)` after Ready/Commence (`0x0073ACD7..0x0073AD48`)
/// and its Weeder twin (`0x0073AD4E..0x0073ADC4`): a `Harvester=` unit
/// (`Type+0xE0E`) whose effective mission (vt+0x184) is not Unload, whose
/// Mission (+0xAC) and MissionQueue (+0xB4) are not Enter and whose +0x6D1
/// latch is clear sends OVER_OUT (`PUSH 3; CALL [vt+0x274]`) when
/// `Contacts[0]` is a `Refinery=` building (`Type+0x16BB`); a `Weeder=` unit
/// (`Type+0xE0F`) does the same toward a `Weeder=` building (`Type+0x16BC`).
/// So a miner that radioed its refinery while still driving (Mission_Harvest
/// state 2) loses that contact at the next track end unless state 3 has
/// queued Enter first. Evidence: tools/spatial_oracle/refinery_dock.json
/// `per_cell_release` rows.
pub(crate) fn per_cell_release_dock_contact(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let unload = MissionId::from_known(MissionType::Unload);
    let enter = MissionId::from_known(MissionType::Enter);
    // The Harvester arm, then the Weeder arm; each reads Contacts[0] afresh.
    for weeder in [false, true] {
        let Some(entity) = sim.substrate.entities.get(id) else {
            return;
        };
        let armed = sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                if weeder {
                    object.weeder
                } else {
                    object.harvester
                }
            })
            && entity.mission.effective() != unload
            && entity.mission.current() != enter
            && entity.mission.queued() != enter
            && !entity
                .miner
                .as_ref()
                .is_some_and(|miner| miner.unload_active);
        let dock = armed
            && entity
                .radio_contacts
                .slot(0)
                .and_then(|contact| sim.substrate.entities.get(contact))
                .filter(|contact| contact.category == EntityCategory::Structure)
                .and_then(|contact| sim.object_type(contact.type_ref(), rules))
                .is_some_and(|building| {
                    if weeder {
                        building.weeder
                    } else {
                        building.refinery
                    }
                });
        if dock {
            radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules));
        }
    }
}

/// Unit+0x6D1 with the image it selects: `UnitClass::Draw` (`0x0073D2BA`)
/// draws `UnloadingClass=` while the latch is set.
fn set_unload_latch(sim: &mut Simulation, rules: &RuleSet, id: u64, active: bool) {
    if !active {
        clear_unload_latch(sim, id);
        return;
    }
    let image = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| {
            sim.object_type(entity.type_ref(), rules)
                .and_then(|object| object.unloading_class.clone())
        })
        .map(|name| sim.interner.intern(&name));
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if let Some(miner) = entity.miner.as_mut() {
        miner.unload_active = true;
    }
    entity.display_type_override = image;
}

/// Drop Unit+0x6D1: the ordinary image returns. The StageClass keeps
/// ticking at its rate until Mission_Harvest re-arms it. Returns whether the
/// latch was set.
pub(crate) fn clear_unload_latch(sim: &mut Simulation, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    let Some(miner) = entity.miner.as_mut() else {
        return false;
    };
    let was_set = miner.unload_active;
    miner.unload_active = false;
    entity.display_type_override = None;
    was_set
}

/// The building `0x0047C520` finds in the cell west of the miner
/// (`AdjacentCell[W]`, `0x0089F6A0`): the refinery for a miner on its pad.
fn unload_building(sim: &Simulation, id: u64) -> Option<u64> {
    let entity = sim.substrate.entities.get(id)?;
    let x = (entity.position.rx as i16).wrapping_sub(1) as u16;
    sim.substrate
        .occupancy
        .first_building_on_layer(x, entity.position.ry, MovementLayer::Ground)
}

/// `0x0073E539` / `0x0073E1F0`: a NavCom and a queued mission other than
/// none or Harvest.
fn new_order_pending(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).is_some_and(|entity| {
        let queued = entity.mission.queued();
        entity.navigation.nav_com.is_some()
            && queued != MissionId::NONE
            && queued != MissionId::from_known(MissionType::Harvest)
    })
}

/// `vt+0x500` when the locomotor still moves (`0x0073DF1C` / `0x0073E22C`).
fn stop_if_moving(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id)
        && crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false)
    {
        crate::sim::movement::track_stop_moving(entity);
    }
}

/// `Ready_To_Commence` (`vt+0x200`) then `Commence` (`vt+0x1EC`).
fn commence_if_ready(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    sim.mission_host_promote(id, sim.session.binary_frame, rules);
}

fn set_status(sim: &mut Simulation, id: u64, status: u32) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.set_handler_state(status);
    }
}

fn epilogue(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    sim.mission_rate_epilogue_for(rules, id, MissionType::Unload)
}

/// `FindFirstUsedSlot` (`0x006C9820`) and `RemoveAmount` (`0x006C96B0`) on
/// VERA's two-kind storage: the first kind present in slot order (ore, then
/// gems) leaves the hold whole. Returns its credit value and bale count.
pub(super) fn drain_first_slot(cargo: &mut Vec<super::CargoBale>) -> Option<(i32, i32)> {
    const SLOT_ORDER: [ResourceType; 2] = [ResourceType::Ore, ResourceType::Gem];
    let slot = SLOT_ORDER
        .iter()
        .copied()
        .find(|kind| cargo.iter().any(|bale| bale.resource_type == *kind))?;
    let mut value: i32 = 0;
    let mut bales: i32 = 0;
    cargo.retain(|bale| {
        if bale.resource_type == slot {
            value = value.saturating_add(i32::from(bale.value));
            bales += 1;
            false
        } else {
            true
        }
    });
    Some((value, bales))
}

/// The two `HouseClass::GiveTiberium` calls (`0x004F9610`, at `0x0073E4A9`
/// and `0x0073E4C9`) for one drained slot, paid to the unload building's
/// owner (`vt+0x3C`): the base amount, then the purifier bonus
/// `P × PurifierBonus × amount` where P counts the owner's purifiers plus
/// `AIVirtualPurifiers[difficulty]` for a computer house outside the
/// campaign (`0x0073E3D5..0x0073E408`). A slave's deposit pays its master
/// the same way (`0x00522D71..0x00522E36`).
pub(crate) fn pay_refinery_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    value: i32,
    bales: i32,
) {
    use crate::sim::economy::apply_income_mult;
    use crate::sim::house_state::{house_state_for_owner_mut, income_ppm_for_owner};
    use crate::sim::production::credits_entry_for_owner;
    let Some(owner) = sim
        .substrate
        .entities
        .get(building)
        .map(|b| sim.interner.resolve(b.owner()).to_string())
    else {
        return;
    };
    let income_ppm = income_ppm_for_owner(&sim.houses, &sim.interner, rules, &owner);
    let base_credits = apply_income_mult(value, income_ppm);
    if base_credits > 0 {
        let credits = credits_entry_for_owner(sim, &owner);
        *credits = credits.saturating_add(base_credits);
        if let Some(house) = house_state_for_owner_mut(&mut sim.houses, &owner, &sim.interner) {
            house.economy.add_harvested(bales);
        }
    }
    let purifiers = super::miner_system::effective_purifier_count(sim, rules, &owner);
    let bonus_ppm = rules.general.purifier_bonus_ppm;
    let bonus_credits =
        crate::sim::economy::purifier_bonus_credits(value, purifiers, bonus_ppm, income_ppm);
    if bonus_credits > 0 {
        let credits = credits_entry_for_owner(sim, &owner);
        *credits = credits.saturating_add(bonus_credits);
        let stat = crate::sim::economy::purifier_bonus_harvested(bales, purifiers, bonus_ppm);
        if let Some(house) = house_state_for_owner_mut(&mut sim.houses, &owner, &sim.interner) {
            house.economy.add_harvested_raw(stat);
        }
    }
}
