//! A spy's infiltration of a building: `BuildingClass::Infiltrate @
//! 0x004571E0` (DB label `BuildingClass__OnSpyInfiltrate`).
//!
//! Its one caller is `InfantryClass::Per_Cell_Process`: an `Agent=`
//! infiltrator that enters the building its mission sent it to
//! (`0x00519FF8..0x0051A00B`, `Simulation::infantry_per_cell_building_entry`).
//! A building of the spy's own house is left alone (`0x004571F4`). Otherwise
//! its type picks one effect, the first of these that applies:
//!
//! 1. `Radar=` (`+0x16A4`): the owner's map is shrouded again, unless its
//!    Spy Satellite reveals it (`HouseClass @ 0x0050BD10` tests `+0x577A`,
//!    then `MapClass::Reset_Shroud`).
//! 2. `Power=` above zero (`+0xEE0`): the owner's power blacks out for
//!    `[General] SpyPowerBlackout=` frames (`HouseClass @ 0x0050BC90`).
//! 3. A type in `[AI] BuildTech=` (`RulesClass+0x91C`): the spy's house has
//!    the technology of the type's `AIBasePlanningSide=`
//!    ([`HouseState::steal_tech`]), which `RequiresStolen*Tech=` types need.
//! 4. `SuperWeapon=` (`+0x16F0`): the owner's Super starts its charge over
//!    (`SuperClass::Reset @ 0x006CE0B0`).
//! 5. `Storage=` above zero (`+0x800`): the spy's house takes
//!    [`stolen_money`] of the owner's money.
//! 6. `Factory=UnitType` or `InfantryType` (`+0xEB8`, RTTI `0x28`/`0x10`):
//!    the spy's house's later vehicles or infantry start veteran.
//!
//! The tech and factory arms also set the house's RecheckTechTree byte
//! (`+0x1FC`) through the same gain owner, including repeated gains. The
//! ordinary House suffix consumes it and emits construction-option admissions
//! before recalculating the retained sidebar. The factory arms' sidebar byte
//! (`0x00884B8E`) and the closing `Mark(2)` (vt+0x124) only redraw. The local
//! player's radar event and EVA lines are the app's
//! ([`SimSoundEvent::BuildingInfiltrated`]).
//!
//! Native execution: `tools/spy_infiltration_oracle.py` runs `0x004571E0`
//! over every arm (`spy_infiltration_tests` replays it).
//!
//! [`HouseState::steal_tech`]: crate::sim::house_state::HouseState::steal_tech

use crate::rules::object_type::FactoryType;
use crate::rules::ruleset::RuleSet;
use crate::sim::credit_income;
use crate::sim::intern::InternedId;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::native_x87::NativeF32Bits;

#[cfg(any(test, feature = "test-support"))]
#[path = "spy_infiltration_tests.rs"]
pub mod spy_infiltration_tests;

/// The arm `BuildingClass::Infiltrate` took, which picks its EVA lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfiltrationEffect {
    /// `Radar=`; `owner_spy_sat` is the owner's `+0x577A`, which also gates
    /// both radar lines.
    Radar {
        owner_spy_sat: bool,
    },
    Power,
    Tech,
    SuperWeapon,
    Cash,
    /// Either factory arm.
    Factory,
    /// No arm applied.
    None,
}

/// The cash arm's take (`0x00457422..0x00457445`): the owner's
/// `Available_Money` (the money interface's slot `+0x18`) loaded as an
/// integer (`FILD`), times `[General] SpyMoneyStealPercent=` (`FMUL` of the
/// stored float), truncated by `Math::ftol` (`0x007C5F00`), all in PC53/chop.
pub(crate) fn stolen_money(available: i32, percent: NativeF32Bits) -> i32 {
    use crate::util::native_x87::MaskedX87Chop53 as X87;
    X87::ftol_i32_low_masked(X87::mul(X87::load_i32(available), X87::load_f32(percent)))
}

impl Simulation {
    /// `BuildingClass::Infiltrate @ 0x004571E0` of `building_id` by a spy of
    /// `infiltrator`'s house (the module doc lists the arms).
    pub(crate) fn infiltrate_building(
        &mut self,
        building_id: u64,
        infiltrator: InternedId,
        rules: &RuleSet,
    ) {
        let Some(building) = self.substrate.entities.get(building_id) else {
            return;
        };
        let owner = building.owner();
        if owner == infiltrator {
            return;
        }
        // vt+0x1B8 (`0x00457221`), the cell `ChangeOwner`'s capture event
        // uses too.
        let radar = RadarEventRequest::new(
            RadarEventType::SpyInfiltration,
            building.position.rx(),
            building.position.ry(),
        );
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return;
        };
        let effect = if object.radar {
            let owner_spy_sat = self
                .houses
                .get(&owner)
                .is_some_and(|house| house.spy_sat_active);
            if !owner_spy_sat {
                self.reset_shroud_for_owner(owner, rules);
            }
            InfiltrationEffect::Radar { owner_spy_sat }
        } else if object.power > 0 {
            let frame = self.session.binary_frame;
            self.power_states
                .entry(owner)
                .or_default()
                .start_blackout(frame, rules.general.spy_power_blackout);
            InfiltrationEffect::Power
        } else if rules.is_build_tech(&object.id) {
            if let Some(house) = self.houses.get_mut(&infiltrator) {
                house.steal_tech(object.ai_base_planning_side);
            }
            InfiltrationEffect::Tech
        } else if let Some(super_weapon) = object
            .super_weapon
            .as_deref()
            .filter(|name| rules.super_weapon(name).is_some())
        {
            crate::sim::superweapon::reset_super(self, rules, owner, super_weapon);
            InfiltrationEffect::SuperWeapon
        } else if object.storage > 0 {
            let amount = stolen_money(
                credit_income::available_money(self, owner),
                rules.general.spy_money_steal_percent,
            );
            credit_income::spend_money(self, owner, amount);
            credit_income::add_credits(self, infiltrator, amount);
            InfiltrationEffect::Cash
        } else if let Some(factory @ (FactoryType::UnitType | FactoryType::InfantryType)) =
            object.factory_type()
        {
            if let Some(house) = self.houses.get_mut(&infiltrator) {
                if factory == FactoryType::UnitType {
                    house.grant_veteran_units();
                } else {
                    house.grant_veteran_infantry();
                }
            }
            InfiltrationEffect::Factory
        } else {
            InfiltrationEffect::None
        };
        if self.owner_is_human(owner) || self.owner_is_human(infiltrator) {
            self.sound_events.push(SimSoundEvent::BuildingInfiltrated {
                owner,
                infiltrator,
                effect,
                radar,
            });
        }
    }
}
