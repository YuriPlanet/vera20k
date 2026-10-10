//! Building repair: a building's repair byte (`BuildingClass+0x6E8`), what
//! starts and stops it, and the repair step it pays for.
//!
//! - [`toggle_repair`] (`BuildingClass::ToggleRepair @ 0x00446FF0`, vt+0x19C)
//!   switches the byte: the player's REPAIR event toggles it
//!   (`EventClass::Execute 0x004C6EFD`), the computer's auto-repair start
//!   starts it (`0x00450708`) and each Selling visit stops it
//!   (`BuildingClass::Mission_Selling 0x00449C41`).
//! - [`update_repair_and_power`] (`BuildingClass::UpdateRepairAndPower @
//!   0x00450630`) runs in each building's LogicVector visit, from
//!   `BuildingClass::Update` (`0x004401B6`) after its Techno AI: a building a
//!   house may repair ([`can_repair_building`]) takes the computer's
//!   low-credit sale while the owner's money is below `[AI] CreditReserve=`,
//!   else the computer's auto-repair start; then the repair step.
//! - `HouseClass::Update` releases the auto-repair latch on its timer
//!   ([`HouseState::release_repair_latch`](crate::sim::house_state::HouseState::release_repair_latch)).
//!
//! Evidence: `tools/spatial_oracle/building_repair.json` (the step cost in
//! `cost` rows, ToggleRepair in `toggle` rows, UpdateRepairAndPower from its
//! entry in `update` rows, the latch release in `release` rows), replayed by
//! `building_repair_oracle_tests`; the low-credit sale in
//! `building_sale.json`'s `ai_sale` rows (`building_sale_oracle_tests`); the
//! damage-state and smoke tail in `building_art_transition.json`'s repair
//! rows.
//!
//! The repair byte's other reader is the wrench `TechnoClass::DrawExtras`
//! draws over a repairing building (`app::presentation::ui_overlays`).
//!
//! RESIDUALS:
//! - The wrench byte (`+0x6DE`: set when a repair starts below Strength,
//!   flipped by every repair step) is read only by the Building CRC
//!   (`0x00454260`); VERA does not keep it.
//! - Presentation: ToggleRepair's flash for the owner's player (vt+0x148 =
//!   `0x00456E00` -> `TechnoClass::Flash(7)`) and the repair step's redraw
//!   byte (`+0x80`, from `BuildingClass::GetCurrentFrame 0x0043EF90`).
//! - Combat order: native fire and its damage land inside each attacker's
//!   own LogicVector visit, VERA's in the combat phase after the object
//!   pass. Trigger: a repairing building under fire. Effect: a hit landing
//!   before the building's visit natively lands after its repair step in
//!   VERA, so within the frame the step reads the pre-hit health and a
//!   building killed that frame has still paid for its step. Frequency: every
//!   repair step of a building under fire. Risk: the owner's credits and the
//!   frame a kill lands; the fire order is the combat pass's own mechanism.
//! - ToggleRepair's sounds and EVA play only for the owner's local player
//!   (the app's `audible_to` gate), which is `IsHumanPlayer` outside
//!   campaigns; in a campaign it reads `+0x1EC`/`+0x1ED` instead (dormant
//!   while campaigns do not launch).
//! - The REPAIR event resolves any target (`0x004C6ED5`) and needs only its
//!   `+0x90`; VERA's command also requires the sender to own the building.
//! - `UnitClass::Deploy` marks the deployed building AI-repairable when its
//!   owner is not the local player (`IsHumanPlayer`, `0x007397E4`); VERA asks
//!   whether a human controls the owner, the same in a skirmish. Another
//!   human's building differs, which only the Building CRC reads: the
//!   auto-repair start admits a human-controlled owner anyway.
//! - The IDIVs of the step period (`0x0045083F`) and the step cost
//!   (`0x007120EC`, `0x007120F7`) fault natively on a zero divisor; VERA
//!   panics there.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{FactoryType, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::credit_income::{available_money, spend_money};
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

use super::production_sell::{SellOrder, sell_back, undeploys};
use crate::rules::foundation::foundation_dimensions;

/// ToggleRepair's control argument. No caller passes any other value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairControl {
    /// `-1`: the REPAIR event's toggle.
    Toggle,
    /// `0`: stop; a building not repairing stays silent.
    Stop,
    /// `1`: start; a building already repairing stays silent.
    Start,
}

/// `BuildingClass::ToggleRepair @ 0x00446FF0`. After the control changes the
/// repair byte: a repair switched on at exactly Strength (`CMP/JNZ`,
/// `0x0044705B`) plays `[AudioVisual] ScoldSound=`; one switched on below it
/// announces `EVA_Repairing` (`0x004470B7`) and, like a repair switched off,
/// plays `GenericClick=` — each at the building's Location (`VocClass::PlayAt
/// 0x007509E0`) and only for the owner's player (`HouseClass::IsHumanPlayer
/// @ 0x0050B6F0`; the app applies that gate). Returns whether the building
/// took the control.
pub fn toggle_repair(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    control: RepairControl,
) -> bool {
    let Some(strength) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
        .map(|object| object.strength)
    else {
        return false;
    };
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    if entity.category != EntityCategory::Structure {
        return false;
    }
    match control {
        RepairControl::Toggle => entity.repairing = !entity.repairing,
        RepairControl::Stop if !entity.repairing => return true,
        RepairControl::Stop => entity.repairing = false,
        RepairControl::Start if entity.repairing => return true,
        RepairControl::Start => entity.repairing = true,
    }
    let owner = entity.owner();
    let position = entity.position;
    let sound = if entity.repairing && entity.health.current == strength {
        rules.general.scold_sound.clone()
    } else {
        if entity.repairing {
            sim.sound_events.push(SimSoundEvent::Repairing { owner });
        }
        rules.general.generic_click_sound.clone()
    };
    if let Some(sound) = sound {
        sim.sound_events.push(SimSoundEvent::voc_at_for(
            sound,
            Some([owner, owner]),
            &position,
        ));
    }
    true
}

/// `TechnoClass::EngineerRepair @ 0x00701410` (vt+40C). Restore actual+6C
/// and estimated+70 independently to live Strength. The Building-only tail
/// stops paid repair, transitions its owned art slots and plays the resolved
/// AudioVisual/BuildingRepairedSound at GetCoords(false), in that order.
///
/// This does not sample Building+544 or invalidate House power: original
/// 440042..440074 owns that later building-visit boundary. Mark(2) at70141A
/// requests presentation redraw; it does not write either health sample or
/// House dirty flag. Executable comparisons: engineer_repair_joined.
pub(crate) fn engineer_repair(sim: &mut Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(strength) = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
        .map(|object| object.strength)
    else {
        return false;
    };
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    entity.health.current = strength;
    entity.estimated_health.reset(strength);
    if entity.category != EntityCategory::Structure {
        return true;
    }
    toggle_repair(sim, rules, id, RepairControl::Stop);
    sim.refresh_building_damage_state(id, rules);
    if let Some(sound) = rules.general.building_repaired_sound.clone()
        && let Some(entity) = sim.substrate.entities.get(id)
    {
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let mut position = entity.position;
        crate::sim::movement::ground_pose::set_position_world_xy(&mut position, [coord.x, coord.y]);
        position.exact_z_leptons = Some(coord.z);
        sim.sound_events
            .push(SimSoundEvent::voc_at(sound, &position));
    }
    true
}

/// `BuildingClass::Can_Repair @ 0x00452630` (vt+0x94): a building with
/// Health (`+0x6C`), of a `ClickRepairable=` type (`+0x157A`) that is not a
/// 1x1 `UndeploysInto=` type (`BuildingTypeClass 0x00465D40`), whose
/// `Repairable=` type (TechnoType `+0xCCC`) has it below Strength
/// (`0x00701140`). Every building of a computer house asks each frame, so
/// the undeploy test's type lookup runs last.
pub(crate) fn can_repair_building(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.category != EntityCategory::Structure || entity.health.current == 0 {
        return false;
    }
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return false;
    };
    object.click_repairable
        && object.repairable
        && entity.health.current != object.strength
        && !(foundation_dimensions(&object.foundation) == (1, 1) && undeploys(rules, object))
}

/// Shared TechnoType repair step cost, vt+0xB0 (`0x007120D0`):
/// its virtual GetCost (vt+0xAC, [`RuleSet::type_cost`]) over the
/// `Strength / RepairStep` steps (both IDIV), times `RepairPercent=` under the
/// ambient PC53/chop x87 word, through `ftol`, and at least 1. Retail 15% is
/// just below .15, so per-step shares of 20, 40 and 100 cost 2, 5 and 14.
pub(crate) fn repair_step_cost(rules: &RuleSet, object: &ObjectType) -> i32 {
    let step = rules.general.repair_step;
    let steps = object.strength.checked_div(step).unwrap_or_else(|| {
        panic!(
            "native repair step cost IDIV fault at 007120EC: strength={} step={step}",
            object.strength
        )
    });
    let cost = rules.type_cost(object);
    let share = cost.checked_div(steps).unwrap_or_else(|| {
        panic!("native repair step cost IDIV fault at 007120F7: cost={cost} steps={steps}")
    });
    X87::ftol_i32_low_masked(X87::mul(
        X87::load_i32(share),
        X87::load_f64(NativeF64Bits::from_bits(
            rules.general.repair_percent.to_bits(),
        )),
    ))
    .max(1)
}

/// `BuildingClass::UpdateRepairAndPower @ 0x00450630`, in the building's
/// LogicVector visit. A building of an owner whose CurrentIQ (`+0x24C`)
/// reaches `[IQ] RepairSell=`, on neither Construction nor Selling
/// ([`GameEntity::constructing_or_selling`]), that the house can repair
/// ([`can_repair_building`]) takes the computer's low-credit sale while the
/// owner's available money is below `[AI] CreditReserve=` (`0x00450781`),
/// else the computer's auto-repair start (`0x004506B2`). Every building then
/// takes the repair step (`0x00450813`).
pub(crate) fn update_repair_and_power(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let owner = entity.owner();
    let admitted = sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.current_iq >= rules.general.iq_repair_sell)
        && !entity.constructing_or_selling()
        && can_repair_building(sim, rules, id);
    if admitted {
        if available_money(sim, owner) < rules.general.credit_reserve {
            low_credit_sale(sim, rules, id, registry, effects);
        } else {
            auto_repair_start(sim, rules, id);
        }
    }
    repair_step(sim, rules, id);
}

/// The computer's low-credit sale (`0x00450781..0x0045080D`): a campaign
/// building carries its AI sale byte (`+0x6DC`); an enemy has hit it
/// (`+0x3D1`); the owner's authored IQ (`+0x1D0`, unsigned) reaches
/// `[IQ] SellBack=`. A skirmish house's authored IQ is zero, so under the
/// retail `SellBack=2` no skirmish building gets this far and none draws.
/// Then `RandomRanged(0, 0x32)` on the Scenario stream (`0x004507C4`) must
/// fall below the owner's TechLevel (unsigned), and a tagged building
/// (`+0x34`, `0x004507D7`; VERA has no per-object tags), a construction yard
/// (`Factory=BuildingType`, `Type+0xEB8 == 7`, `0x004507DE`) and one at or
/// above ConditionRed (`0x004507ED`) stay. The rest take [`sell_back`]'s
/// computer order (`0x0045080D`).
fn low_credit_sale(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(house) = sim.houses.get(&entity.owner()) else {
        return;
    };
    if !((sim.session.game_mode_nonzero || entity.ai_sellable)
        && entity.was_attacked_by_enemy
        && house.authored_iq as u32 >= rules.general.iq_sell_back as u32)
    {
        return;
    }
    let tech_level = house.tech_level;
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let yard = object.factory == Some(FactoryType::BuildingType);
    // 4507F7..450805 tests x87 C0: less and unordered both sell.
    let below_red = matches!(
        entity
            .health
            .compare_ratio(object.strength, rules.general.condition_red),
        MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
    );
    let roll = sim.scenario_rng.next_range_u32_inclusive(0, 0x32);
    if roll >= tech_level as u32 || yard || !below_red {
        return;
    }
    let _ = sell_back(sim, rules, id, SellOrder::Computer, registry, effects);
}

/// The computer's auto-repair start (`0x004506B2..0x0045077C`). The owner's
/// latch (`+0x245`) holds it, a building already repairing skips it, and only
/// a captured building (`+0x6E3`), an AI-repairable one (`+0x6CB`) or one
/// whose owner a human controls (`HouseClass::IsControlledByHuman @
/// 0x0050B730`) starts: the latch set, [`toggle_repair`]`(Start)`, and for an
/// owner no human controls the latch timer armed at this frame for
/// `RandomRanged(ftol(RepairDelay * 225), ftol(RepairDelay * 1800))` frames on
/// the Scenario stream (`0x00450759`). Retail `RepairDelay=.02` as ReadDouble
/// stores it draws from 4..35 frames; `.05` from 11..90.
fn auto_repair_start(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let owner = entity.owner();
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if house.repair_start_latch || entity.repairing {
        return;
    }
    let controlled = house.is_controlled_by_human(sim.session.game_mode_nonzero);
    if !(entity.has_been_captured || entity.ai_repairable || controlled) {
        return;
    }
    let delay = house.repair_delay();
    sim.houses
        .get_mut(&owner)
        .expect("the owner was just read")
        .repair_start_latch = true;
    toggle_repair(sim, rules, id, RepairControl::Start);
    if controlled {
        return;
    }
    let scaled = |factor: i32| {
        X87::ftol_i32_low_masked(X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(delay.to_bits())),
            X87::load_i32(factor),
        ))
    };
    let (longest, shortest) = (scaled(1800), scaled(225));
    let time_left = sim.scenario_rng.next_range_i32_inclusive(shortest, longest);
    let now = sim.session.binary_frame as i32;
    if let Some(house) = sim.houses.get_mut(&owner) {
        house.repair_latch_timer.start(now, time_left);
    }
}

/// The repair step (`0x00450813..0x004509BB`): a repairing building, on a
/// frame the period `ftol(RepairRate * 900)` divides (the self-heal period,
/// signed IDIV), pays [`repair_step_cost`] if the owner's available money
/// covers it — otherwise the repair stops — through `HouseClass::Spend_Money
/// 0x004F9790`, and adds `RepairStep=` to Health and its estimate (`+0x70`,
/// both wrapping `ADD`s). At or past Strength (signed) both become Strength
/// and the repair stops. Then the damage state follows the new health
/// (`0x004508D7`) and a building back above ConditionYellow retires its
/// damage smoke (`0x0045096C`).
fn repair_step(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if !entity.repairing {
        return;
    }
    let period =
        crate::sim::combat::veterancy::self_heal_interval_frames(rules.general.repair_rate_minutes);
    let frame = sim.session.binary_frame as i32;
    let remainder = frame.checked_rem(period).unwrap_or_else(|| {
        panic!("native repair step IDIV fault at 0045083F: frame={frame} period={period}")
    });
    if remainder != 0 {
        return;
    }
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let (strength, cost) = (object.strength, repair_step_cost(rules, object));
    let owner = entity.owner();
    if available_money(sim, owner) < cost {
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.repairing = false;
        }
        return;
    }
    spend_money(sim, owner, cost);
    let step = rules.general.repair_step;
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    entity.health.current = entity.health.current.wrapping_add(step);
    entity.estimated_health.add_repair(step);
    if entity.health.current >= strength {
        entity.health.current = strength;
        entity.estimated_health.reset(strength);
        entity.repairing = false;
    }
    sim.refresh_building_damage_state(id, rules);
    sim.retire_damage_smoke_after_heal(id, rules);
}
