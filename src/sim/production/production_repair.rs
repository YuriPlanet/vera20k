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
//! - Presentation: the repair step's redraw byte (`+0x80`, from
//!   `BuildingClass::GetCurrentFrame 0x0043EF90`).
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

/// A building's repair byte, `BuildingClass+0x6E8`: the repair step pays for
/// each `RepairStep=` of health while it is on. The constructor clears it;
/// [`toggle_repair`] switches it and the repair step (`0x00450813..0x004509BB`)
/// stops it when the owner cannot pay or the building is whole.
/// `BuildingClass::ChangeOwner` stops it without a sound (`0x00448CE8`,
/// [`Self::stop_on_capture`]). The other reader is the wrench
/// `TechnoClass::DrawExtras` draws over a repairing building.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RepairByte {
    on: bool,
}

impl RepairByte {
    /// The building is repairing.
    pub const fn is_on(self) -> bool {
        self.on
    }

    /// `BuildingClass::ChangeOwner`'s silent stop (`0x00448CE8`).
    pub(crate) fn stop_on_capture(&mut self) {
        self.on = false;
    }

    /// Install a fixture's byte.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, on: bool) {
        self.on = on;
    }
}

/// The computer's building sale and repair inputs, which
/// [`update_repair_and_power`] reads:
/// - `TechnoClass+0x3D1`, hit by an enemy: the constructor clears it
///   (`0x006F2F43`) and `TechnoClass::ReceiveDamage` sets it after a hostile
///   hit the object survives (`0x00702812`, [`Self::hit_by_enemy`]). It is
///   independent of retaliation's transient attacker pointer; the
///   low-credit sale reads it.
/// - `BuildingClass+0x6DC`, AI sellable, which the computer's low-credit sale
///   requires in a campaign (`0x00450781`). The constructor sets it
///   (`0x0043B93D`) and Init_Managers clears it for a type without a Buildup
///   (`0x00442CBC`, [`Self::construct`]); the map's `[Structures]` reader then
///   writes its AI Sellable field (`0x0044FB5B`, [`Self::read_map_line`]).
/// - `BuildingClass+0x6CB`, AI repairable, which lets the computer's
///   auto-repair start repair it (`0x004506DE`). The constructor clears it
///   (`0x0043B91F`); the map's `[Structures]` reader writes its AI Repairable
///   field (`0x0044FB70`), then the building's Unlimbo sets it outside a
///   campaign for a house no human controls (`0x00440B7A`), and so does an
///   MCV deploy for a computer house (`0x007397F4`, [`Self::make_repairable`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AiSellRepair {
    was_attacked_by_enemy: bool,
    ai_sellable: bool,
    ai_repairable: bool,
}

impl AiSellRepair {
    /// `+0x3D1`: a hostile hit landed and the object survived.
    pub(crate) const fn was_attacked_by_enemy(self) -> bool {
        self.was_attacked_by_enemy
    }

    /// `+0x6DC`: the computer may sell the building for credits.
    pub(crate) const fn ai_sellable(self) -> bool {
        self.ai_sellable
    }

    /// `+0x6CB`: the computer's auto-repair may start on the building.
    pub(crate) const fn ai_repairable(self) -> bool {
        self.ai_repairable
    }

    /// `TechnoClass::ReceiveDamage`'s latch (`0x00702812`).
    pub(crate) fn hit_by_enemy(&mut self) {
        self.was_attacked_by_enemy = true;
    }

    /// The constructor's sale byte after Init_Managers: set for a building
    /// whose type has a Buildup (`0x0043B93D`, `0x00442CBC`).
    pub(crate) fn construct(&mut self, sellable: bool) {
        self.ai_sellable = sellable;
    }

    /// The map line's AI Sellable and AI Repairable (`0x0044FB5B`,
    /// `0x0044FB70`).
    pub(crate) fn read_map_line(&mut self, sellable: bool, repairable: bool) {
        self.ai_sellable = sellable;
        self.ai_repairable = repairable;
    }

    /// Unlimbo's (`0x00440B7A`) and an MCV deploy's (`0x007397F4`) grant.
    pub(crate) fn make_repairable(&mut self) {
        self.ai_repairable = true;
    }

    /// Install a fixture's `+0x3D1`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_was_attacked_by_enemy_for_test(&mut self, value: bool) {
        self.was_attacked_by_enemy = value;
    }

    /// Install a fixture's `+0x6DC`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_ai_sellable_for_test(&mut self, value: bool) {
        self.ai_sellable = value;
    }

    /// Install a fixture's `+0x6CB`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_ai_repairable_for_test(&mut self, value: bool) {
        self.ai_repairable = value;
    }
}

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
/// `0x0044705B`) plays `[AudioVisual] ScoldSound=`; one switched on at any
/// other health flashes for the owner's player and announces `EVA_Repairing`
/// (`0x004470B7`) and, like a repair switched off,
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
    let Some((strength, owner_is_player)) = sim.substrate.entities.get(id).and_then(|entity| {
        sim.object_type(entity.type_ref(), rules)
            .map(|object| (object.strength, sim.house_is_human_player(entity.owner())))
    }) else {
        return false;
    };
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    if entity.category() != EntityCategory::Structure {
        return false;
    }
    match control {
        RepairControl::Toggle => entity.repairing.on = !entity.repairing.on,
        RepairControl::Stop if !entity.repairing.on => return true,
        RepairControl::Stop => entity.repairing.on = false,
        RepairControl::Start if entity.repairing.on => return true,
        RepairControl::Start => entity.repairing.on = true,
    }
    let owner = entity.owner();
    let position = entity.position;
    let sound = if entity.repairing.on && entity.health.current() == strength {
        rules.general.scold_sound.clone()
    } else {
        if entity.repairing.on {
            // ToggleRepair447082..447091: House50B6F0 admits Building
            // vt+148 Flash(7), before EVA/GenericClick. Reuse the shared
            // disguise/countdown owner, not a separate repair flash timer.
            // Original call arguments: building_repair.json toggle events.
            if owner_is_player {
                entity.flash_base(7);
            }
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
    entity.health.set(strength);
    entity.estimated_health.reset(strength);
    if entity.category() != EntityCategory::Structure {
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
        position.put_world_xy([coord.x, coord.y]);
        position.put_z(coord.z);
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
pub fn can_repair_building(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.category() != EntityCategory::Structure || entity.health.current() == 0 {
        return false;
    }
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return false;
    };
    object.click_repairable
        && object.repairable
        && entity.health.current() != object.strength
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
    if !((sim.session.game_mode_nonzero() || entity.ai_sell_repair.ai_sellable())
        && entity.ai_sell_repair.was_attacked_by_enemy()
        && house.authored_iq as u32 >= rules.general.iq_sell_back as u32)
    {
        return;
    }
    let tech_level = house.tech_level;
    let Some(object) = sim.object_type(entity.type_ref(), rules) else {
        return;
    };
    let yard = object.factory_type() == Some(FactoryType::BuildingType);
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
    if house.repair_start_latch || entity.repairing.on {
        return;
    }
    let controlled = house.is_controlled_by_human(sim.session.game_mode_nonzero());
    if !(entity.has_been_captured.is_set() || entity.ai_sell_repair.ai_repairable() || controlled) {
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
    let now = sim.session.binary_frame() as i32;
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
    if !entity.repairing.on {
        return;
    }
    let period =
        crate::sim::combat::veterancy::self_heal_interval_frames(rules.general.repair_rate_minutes);
    let frame = sim.session.binary_frame() as i32;
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
            entity.repairing.on = false;
        }
        return;
    }
    spend_money(sim, owner, cost);
    let step = rules.general.repair_step;
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    entity.health.heal(step);
    entity.estimated_health.add_repair(step);
    if entity.health.current() >= strength {
        entity.health.set(strength);
        entity.estimated_health.reset(strength);
        entity.repairing.on = false;
    }
    sim.refresh_building_damage_state(id, rules);
    sim.retire_damage_smoke_after_heal(id, rules);
}
