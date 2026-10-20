//! Harvester refinery dock as the native Enter and Unload missions.
//!
//! Mission_Harvest state 3 queues Enter. Each `FootClass::Mission_Enter`
//! dispatch (`0x004D9290`) sends DOCKING (0x0E) to the refinery, whose
//! receiver (`radio::receive`, `0x0043C7E9`) drives the handshake: MOVE_HERE
//! to the pad NW+(3,1), then, once the miner stands in it, TETHER and
//! PREPARE_TO_DOCK, which turns the hull to 0x4000 and sends DOCK_NOW back
//! (or the Drive track end sends it, `Simulation::unit_dock_now`). The refinery
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
//! The shared `mission::enter` owner re-runs that setter on each dispatch.
//! Evidence: tools/spatial_oracle/cmin_dock.json.
//!
//! RESIDUALS (named trigger, effect, frequency):
//! - The shared Enter, pending-entry and destination owners handle depot and
//!   refinery consumers; retained controls/histories compare their native
//!   cadence, contact, cargo and cleanup boundaries in refinery_dock.json.
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
//! A refinery sold (`BuildingClass::Mission_Selling`) or destroyed (the
//! NowDead contact loop, `Simulation::building_now_dead_contacts`) under an
//! unloading miner sends it RUN_AWAY (0x17): the latch drops and Harvest takes
//! over (`radio::receive`, Unit `0x00737A98`).
//!
//! ## Dependency rules
//! - Part of sim/ — sim/radio, sim/mission, sim/movement, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::BaleDepositEvent;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::radio::{self, RadioMessage};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::ResourceType;

/// The type whose model a harvester shows while its unload latch
/// (Unit+0x6D1) is set: the type's `UnloadingClass=`, which raising the latch
/// installs and dropping it clears. Only the latch's two writers here touch
/// it; the unit drawers read it in place of the object's own type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct UnloadImage {
    image: Option<crate::sim::intern::InternedId>,
}

impl UnloadImage {
    /// The unloading type, while the latch is set.
    pub const fn image(self) -> Option<crate::sim::intern::InternedId> {
        self.image
    }

    /// Install a fixture's image.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_for_test(&mut self, image: Option<crate::sim::intern::InternedId>) {
        self.image = image;
    }
}

/// The body facing the Unload window centres on (`0x0073DF8A`).
const UNLOAD_FACING: u16 = crate::sim::radio::receive::DOCK_FACING;

/// Mission_Unload harvester Status (+0xBC) values: dumping and finishing.
const UNLOAD_DUMPING: u32 = 3;
const UNLOAD_FINISHING: u32 = 4;

/// `UnitClass::Mission_Unload @ 0x0073D630`, harvester branch `0x0073DEE0`.
/// Returns the dispatch delay.
pub(crate) fn mission_unload(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame();
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    // A 0x0073DEE0: without any radio contact the unload is abandoned.
    if entity.radio_contacts.is_empty() {
        sim.unit_enter_idle_mode(id, Some(rules), false, effects);
        set_unload_latch(sim, rules, id, false);
        stop_if_moving(sim, id);
        commence_if_ready(sim, rules, id);
        return 1;
    }
    // B 0x0073DF56: the hull must sit in the 8-bit East window
    // (raw 0x3F80..=0x407F); otherwise turn (unless the turret is mid-swing).
    let raw = entity.body_facing_current(now);
    if (((u32::from(raw) >> 7) + 1) & 0x1FE) != 0x80 {
        if !entity.turret_rotation_latch.is_set()
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
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.restart_native_stage(0, now as i32, 1);
        }
        set_unload_latch(sim, rules, id, true);
        if let Some(building) = unload_building(sim, id) {
            crate::sim::world::building_anim::start_refinery_unload(sim, rules, building);
        }
        set_status(sim, id, UNLOAD_DUMPING);
        return epilogue(sim, rules, id);
    }
    match entity.mission.handler_state() {
        UNLOAD_DUMPING => unload_dumping(sim, rules, id, effects),
        UNLOAD_FINISHING => unload_finishing(sim, rules, id, effects),
        _ => epilogue(sim, rules, id),
    }
}

/// E 0x0073E2BF: the dump state, re-reading the building west of the miner
/// on every dispatch.
fn unload_dumping(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame();
    let Some(building) = unload_building(sim, id) else {
        // 0x0073E311..0x0073E350: no building → OVER_OUT, Queue(Harvest, 1).
        radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules), effects);
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
        .map_or(0, |entity| entity.native_stage().value());
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
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                // `Unit+0xF8 = 0` (`0x0073E493`).
                entity.set_native_stage_value(0);
            }
            sim.bale_events.push(BaleDepositEvent {
                building_id: building,
                tick: sim.session.tick(),
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
                tick: sim.session.tick(),
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
fn unload_finishing(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame();
    if let Some(building) = unload_building(sim, id)
        && let Some(building) = sim.substrate.entities.get(building)
        && sim
            .object_type(building.type_ref(), rules)
            .is_some_and(|object| object.refinery)
        && building.building_art.anim_slots()[8].is_some()
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
            radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules), effects);
            let _ = sim.mission_commence_exact(id, now);
        }
    }
    epilogue(sim, rules, id)
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
pub(crate) fn per_cell_release_dock_contact(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    effects: FrameEffects<'_>,
) {
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
                .filter(|contact| contact.category() == EntityCategory::Structure)
                .and_then(|contact| sim.object_type(contact.type_ref(), rules))
                .is_some_and(|building| {
                    if weeder {
                        building.weeder
                    } else {
                        building.refinery
                    }
                });
        if dock {
            radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules), effects);
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
    entity.display_type_override.image = image;
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
    entity.display_type_override.image = None;
    was_set
}

/// The Unit setter's unload step (`UnitClass::Set_Destination`
/// `0x00741BC7..0x00741C4A`), run for every destination but NULL: while the
/// Unit's mission (vt+0x184) or MissionQueue (`+0xB4`) is Unload, a
/// `Harvester=` type (`+0xE0E`) has the building west of it
/// ([`unload_building`]) delete its Special animation (slot 10,
/// `ClearAnimSlot @ 0x00451E40`), the one the unload gate starts.
pub(crate) fn clear_unload_anim_for_destination(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let unload = MissionId::from_known(MissionType::Unload);
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.mission.effective() != unload && entity.mission.queued() != unload {
        return;
    }
    let harvester = sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.harvester);
    if let Some(building) = unload_building(sim, id)
        && harvester
    {
        sim.clear_building_anim_slot(building, 10);
    }
}

/// The building `0x0047C520` finds in the cell west of the miner
/// (`AdjacentCell[W]`, `0x0089F6A0`): the refinery for a miner on its pad.
fn unload_building(sim: &Simulation, id: u64) -> Option<u64> {
    let entity = sim.substrate.entities.get(id)?;
    let x = (entity.position.rx() as i16).wrapping_sub(1) as u16;
    sim.substrate
        .occupancy
        .first_building_on_layer(x, entity.position.ry(), MovementLayer::Ground)
}

/// `0x0073E539` / `0x0073E1F0`: a NavCom and a queued mission other than
/// none or Harvest.
fn new_order_pending(sim: &Simulation, id: u64) -> bool {
    sim.substrate.entities.get(id).is_some_and(|entity| {
        let queued = entity.mission.queued();
        entity.navigation.nav_com().is_some()
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
    sim.mission_host_promote(id, sim.session.binary_frame(), rules);
}

/// Mission+0xBC: the harvester branch stores 3 at `0x0073E093` and 4 at
/// `0x0073E51C` and `0x0073E594`.
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

/// The two `HouseClass::Add_Tiberium_Credits` calls (`0x004F9610`, at
/// `0x0073E4A9` and `0x0073E4C9`) for one drained slot, paid to the unload
/// building's owner (`vt+0x3C`): the slot's amount, then the purifier bonus
/// `P × PurifierBonus × amount` when it is above zero, where P counts the
/// owner's purifiers plus `AIVirtualPurifiers[difficulty]` for a computer
/// house outside the campaign (`0x0073E3D5..0x0073E408`). A slave's deposit
/// pays its master the same way (`0x00522D71..0x00522E36`). The unloader's
/// `Weeder=` arm (`0x0073E474`) is not ported; no retail unit sets it.
pub(crate) fn pay_refinery_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    value: i32,
    bales: i32,
) {
    use crate::rules::ruleset::INCOME_PPM_SCALE;
    use crate::sim::house_state::income_ppm_for_owner;
    let Some(owner) = sim.substrate.entities.get(building).map(|b| b.owner()) else {
        return;
    };
    let owner_name = sim.interner.resolve(owner).to_string();
    let income_ppm = income_ppm_for_owner(&sim.houses, &sim.interner, rules, &owner_name);
    let purifiers = super::miner_system::effective_purifier_count(sim, rules, &owner_name);
    let bonus_ppm = i64::from(purifiers) * rules.general.purifier_bonus_ppm;
    let Some(house) = sim.houses.get_mut(&owner) else {
        return;
    };
    house
        .economy_mut()
        .add_tiberium_credits(value, bales, INCOME_PPM_SCALE, income_ppm);
    if bonus_ppm > 0 {
        house
            .economy_mut()
            .add_tiberium_credits(value, bales, bonus_ppm, income_ppm);
    }
}
