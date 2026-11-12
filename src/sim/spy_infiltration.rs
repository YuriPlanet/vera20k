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
//! With Ares, `BuildingExt::ExtData::InfiltratedBy @ 10012000` wraps the
//! function (hook `0x004571E0`): a building of a house the spy's house is
//! allied to (`IsAlliedWith`, the spy's own included) is left alone, and a
//! `SpyEffect.Custom=` type runs every Ares arm that applies instead of the
//! one native arm ([`Simulation::ares_infiltrated_by`]).
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

#[cfg(test)]
#[path = "spy_infiltration_retail_tests.rs"]
mod retail_tests;

#[cfg(all(test, feature = "ares"))]
#[path = "spy_infiltration_ares_tests.rs"]
mod ares_tests;

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
    /// The Ares arms of a `SpyEffect.Custom=` type.
    #[cfg(feature = "ares")]
    Ares(AresInfiltrationArms),
}

/// The arms `BuildingExt::ExtData::InfiltratedBy @ 10012000` took, which
/// pick its EVA lines (`eva_producers::infiltration_eva_events`).
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AresInfiltrationArms {
    /// `SpyEffect.ResetRadar`; the owner's Spy Satellite (`+0x577A`) after
    /// the reshroud, which gates both radar lines.
    pub radar_reset: Option<bool>,
    /// `SpyEffect.PowerOutageDuration`.
    pub power_outage: bool,
    /// `SpyEffect.StolenTechIndex`.
    pub tech_stolen: bool,
    /// `SpyEffect.UndoReverseEngineer`.
    pub undo_reverse_engineer: bool,
    /// `SpyEffect.ResetSuperweapons` reset at least one Super.
    pub supers_reset: bool,
    /// `SpyEffect.SuperWeapon`'s Grant went through.
    pub super_granted: bool,
    /// The money arm took more than nothing.
    pub cash_stolen: bool,
    /// A veterancy arm applied.
    pub veterancy_stolen: bool,
    /// `SpyEffect.RevealProduction`.
    pub production_revealed: bool,
    /// `SpyEffect.RevealRadar`.
    pub radar_revealed: bool,
}

/// A building's DisplayProductionTo (`BuildingClass+0x210`, Ares
/// `IndexBitfield<HouseClass*>`): the houses whose spies entered it with
/// `SpyEffect.RevealProduction` or `SpyEffect.RevealRadar`. Ares'
/// infiltration adds them ([`Simulation::ares_infiltrated_by`]), the
/// building's destruction clears them (`BuildingClass_Destroy_OldSpy1`, hook
/// `0x0044161C`) and a change of owner drops the new owner
/// (`BuildingClass_ChangeOwnership_OldSpy1`/`2`, hooks `0x00448312` and
/// `0x00448D95`). The selected-building status text reads it (Ares
/// `BuildingClass_DrawVisible`, hook `0x0043E7B0`).
///
/// RESIDUAL: Ares' `UpdateDisplayTo` (`0x0044F7A0`) also folds the bits of
/// `SpyEffect.RevealRadar` buildings, and the `SpyEffect.KeepRadar` houses
/// of HouseExt `RadarPersist` (`+0x44`), into the owner's RadarVisibleTo
/// (`HouseClass+0x54E4`), which the RevealArea gate (`0x00567AC1`,
/// `0x0056757F`) reads to map the owner's reveals for those houses. VERA
/// keeps no RadarVisibleTo (`vision::reveal_admits`), so a radar spy sees
/// nothing of the owner's sight. Trigger: a spy entering a
/// `SpyEffect.RevealRadar=` building; no retail type sets it.
///
/// RESIDUAL: `DrawHealthBar @ 0x006F64A0`'s pip arm tests the bit at the
/// player's HouseType index (`Type+0x34`), which Ares' house-index bits
/// match only where the two indices agree; VERA leaves that arm out, so a
/// spied building's pips stay hidden. Presentation only.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct DisplayProductionTo(std::collections::BTreeSet<InternedId>);

#[cfg(feature = "ares")]
impl DisplayProductionTo {
    pub(crate) fn contains(&self, house: InternedId) -> bool {
        self.0.contains(&house)
    }

    fn add(&mut self, house: InternedId) {
        self.0.insert(house);
    }

    /// `0x0044161C`: the destroyed building shows nothing any more.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    /// `0x00448312`, `0x00448D95`: the new owner's bit goes.
    pub(crate) fn remove(&mut self, house: InternedId) {
        self.0.remove(&house);
    }

    /// Folded only while a house is in it.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        if !self.0.is_empty() {
            std::hash::Hash::hash(b"ares-display-production-to-v1", hasher);
            std::hash::Hash::hash(&self.0, hasher);
        }
    }
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
        // Ares 3.0 10012000 (hook 0x004571E0): the spy's house answers
        // IsAlliedWith for the owner first, its own house included.
        #[cfg(feature = "ares")]
        if self
            .house_alliances
            .is_allied(&self.interner, infiltrator, owner)
        {
            return;
        }
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
        #[cfg(feature = "ares")]
        if object.infiltration_rules().custom {
            let arms = self.ares_infiltrated_by(building_id, owner, infiltrator, object, rules);
            self.announce_infiltration(owner, infiltrator, InfiltrationEffect::Ares(arms), radar);
            return;
        }
        let effect = if object.radar {
            let owner_spy_sat = self
                .houses
                .get(&owner)
                .is_some_and(|house| house.spy_sat_active());
            if !owner_spy_sat {
                self.reset_shroud_for_owner(owner, rules);
            }
            InfiltrationEffect::Radar { owner_spy_sat }
        } else if object.power > 0 {
            let frame = self.session.binary_frame();
            self.power_states
                .house_mut(owner)
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
            crate::sim::superweapon::reset_super(
                &self.interner,
                &self.session,
                &mut self.substrate,
                &mut self.sound_events,
                &mut self.super_weapons,
                rules,
                owner,
                super_weapon,
            );
            InfiltrationEffect::SuperWeapon
        } else if object.storage > 0 {
            let amount = stolen_money(
                credit_income::available_money(&self.houses, owner),
                rules.general.spy_money_steal_percent,
            );
            credit_income::spend_money(&mut self.houses, owner, amount);
            credit_income::add_credits(&mut self.houses, infiltrator, amount);
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
        self.announce_infiltration(owner, infiltrator, effect, radar);
    }

    /// The radar event and EVA lines, asked for only when the owner or the
    /// spy's house is a player's (`0x004571FE`, `0x0045720E`; Ares
    /// `ControlledByPlayer` at `10012000`).
    fn announce_infiltration(
        &mut self,
        owner: InternedId,
        infiltrator: InternedId,
        effect: InfiltrationEffect,
        radar: RadarEventRequest,
    ) {
        if self.owner_is_human(owner) || self.owner_is_human(infiltrator) {
            self.sound_events.emit(SimSoundEvent::BuildingInfiltrated {
                owner,
                infiltrator,
                effect,
                radar,
            });
        }
    }

    /// The Ares arms of `BuildingExt::ExtData::InfiltratedBy @ 10012000`
    /// (hook `0x004571E0`) for a `SpyEffect.Custom=` building of `owner`
    /// entered by a spy of `infiltrator`, in its order. Each applies on its
    /// own key:
    /// - `ResetRadar`: `HouseClass::ReshroudMap @ 0x0050BD10`, which leaves
    ///   a house with an active Spy Satellite alone.
    /// - `PowerOutageDuration` above zero: `CreatePowerOutage @ 0x0050BC90`
    ///   for that many frames.
    /// - `StolenTechIndex` nonempty: its bits join the spy's house's
    ///   StolenTech, then that house's House+1FC
    ///   ([`crate::sim::house_state::HouseState::steal_ares_tech`]).
    /// - `UndoReverseEngineer`: the owner's HouseExt `ReverseEngineered`
    ///   list (`+0x34`, `sim::reverse_engineering`) is emptied and House+1FC
    ///   set.
    /// - `ResetSuperweapons`: `SuperClass::Reset @ 0x006CE0B0` of each
    ///   Super of the type and of its three upgrade slots
    ///   (`GetSuperWeapons @ 1000D700`) the owner has available
    ///   (`SWTypeExt::GetSuperWeapon @ 100329E0`).
    /// - `SuperWeapon`: the spy's house gets it
    ///   ([`crate::sim::superweapon::spy_grant_super`]).
    /// - `SabotageDelay` nonzero: the building's C4 latch and fuse, for
    ///   `[General] C4Delay=` when negative; C4AppliedBy is not written.
    /// - The money: `StolenMoneyPercentage` above zero takes that share of
    ///   the owner's available money ([`stolen_money`]), capped by a positive
    ///   `StolenMoneyAmount`; otherwise a positive amount alone. The take is
    ///   at most the available money, `TakeMoney @ 0x004F9790` from the
    ///   owner and `GiveMoney @ 0x004F9950` to the spy's house.
    /// - The veterancy: `UnitVeterancy` by the type's `Factory=` (UnitType
    ///   the vehicles, InfantryType the infantry) and each of the five
    ///   `*Veterancy` keys, then House+1FC
    ///   ([`crate::sim::house_state::HouseState::grant_ares_spy_veterancy`]).
    /// - `RevealProduction` and `RevealRadar`: the spy's house joins the
    ///   building's [`DisplayProductionTo`].
    ///
    /// No arm draws random numbers or detaches anything; the power outage,
    /// the Super's timers and the C4 fuse are its timer writes.
    #[cfg(feature = "ares")]
    pub(crate) fn ares_infiltrated_by(
        &mut self,
        building_id: u64,
        owner: InternedId,
        infiltrator: InternedId,
        object: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
    ) -> AresInfiltrationArms {
        let spy = object.infiltration_rules();
        let mut arms = AresInfiltrationArms::default();
        let frame = self.session.binary_frame();
        let spy_sat = |sim: &Self| {
            sim.houses
                .get(&owner)
                .is_some_and(|house| house.spy_sat_active())
        };
        if spy.reset_radar {
            if !spy_sat(self) {
                self.reset_shroud_for_owner(owner, rules);
            }
            arms.radar_reset = Some(spy_sat(self));
        }
        if spy.power_outage_duration > 0 {
            self.power_states
                .house_mut(owner)
                .start_blackout(frame, spy.power_outage_duration);
            arms.power_outage = true;
        }
        if spy.stolen_tech != 0 {
            if let Some(house) = self.houses.get_mut(&infiltrator) {
                house.steal_ares_tech(spy.stolen_tech);
            }
            arms.tech_stolen = true;
        }
        if spy.undo_reverse_engineer {
            if let Some(house) = self.houses.get_mut(&owner) {
                house.undo_ares_reverse_engineering();
            }
            arms.undo_reverse_engineer = true;
        }
        if spy.reset_super_weapons {
            arms.supers_reset = self.reset_infiltrated_supers(building_id, owner, object, rules);
        }
        if let Some(type_id) = spy
            .super_weapon
            .as_deref()
            .and_then(|name| self.interner.get(name))
        {
            arms.super_granted = crate::sim::superweapon::spy_grant_super(
                self,
                rules,
                infiltrator,
                type_id,
                spy.super_weapon_permanent,
            );
        }
        if spy.sabotage_delay != 0 {
            let delay = if spy.sabotage_delay < 0 {
                rules.c4_delay_frames()
            } else {
                spy.sabotage_delay
            };
            if delay >= 0
                && let Some(building) = self.substrate.entities.get_mut(building_id)
            {
                building
                    .building
                    .pending_c4_detonation
                    .arm_sabotage(frame as i32, delay);
            }
        }
        let available = credit_income::available_money(&self.houses, owner);
        let amount = spy.stolen_money_amount;
        let take = if f32::from_bits(spy.stolen_money_percentage.bits()) > 0.0 {
            let share = stolen_money(available, spy.stolen_money_percentage);
            if amount > 0 { share.min(amount) } else { share }
        } else if amount > 0 {
            amount
        } else {
            0
        };
        if take > 0 {
            let take = take.min(available);
            credit_income::spend_money(&mut self.houses, owner, take);
            credit_income::add_credits(&mut self.houses, infiltrator, take);
            arms.cash_stolen = true;
        }
        let factory = object.factory_type();
        let veterancy = crate::sim::house_state::AresSpyVeterancy {
            infantry: spy.infantry_veterancy
                || spy.unit_veterancy && factory == Some(FactoryType::InfantryType),
            units: spy.vehicle_veterancy
                || spy.unit_veterancy && factory == Some(FactoryType::UnitType),
            naval: spy.naval_veterancy,
            aircraft: spy.aircraft_veterancy,
            buildings: spy.building_veterancy,
        };
        arms.veterancy_stolen = self
            .houses
            .get_mut(&infiltrator)
            .is_some_and(|house| house.grant_ares_spy_veterancy(veterancy));
        // RevealProduction, then RevealRadar: both add the spy's house to
        // DisplayProductionTo ([`DisplayProductionTo`] has the radar
        // residual).
        arms.production_revealed = spy.reveal_production;
        arms.radar_revealed = spy.reveal_radar;
        if (spy.reveal_production || spy.reveal_radar)
            && let Some(building) = self.substrate.entities.get_mut(building_id)
        {
            building.display_production_to.add(infiltrator);
        }
        arms
    }

    /// `SpyEffect.ResetSuperweapons`: [`crate::sim::superweapon::reset_super`]
    /// of each Super of the building's type and its upgrade slots (`+0x5EC`)
    /// that `owner` has available. Answers whether one was reset.
    #[cfg(feature = "ares")]
    fn reset_infiltrated_supers(
        &mut self,
        building_id: u64,
        owner: InternedId,
        object: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
    ) -> bool {
        let upgrades = self
            .substrate
            .entities
            .get(building_id)
            .map(|building| *building.building_upgrades().types())
            .unwrap_or_default();
        let supers: Vec<String> = std::iter::once(Some(object))
            .chain(
                upgrades
                    .into_iter()
                    .map(|upgrade| upgrade.and_then(|id| self.object_type(id, rules))),
            )
            .flatten()
            .flat_map(crate::sim::superweapon::type_super_weapons)
            .filter(|name| {
                crate::sim::superweapon::super_weapon_available(self, rules, owner, name)
            })
            .map(str::to_owned)
            .collect();
        for name in &supers {
            crate::sim::superweapon::reset_super(
                &self.interner,
                &self.session,
                &mut self.substrate,
                &mut self.sound_events,
                &mut self.super_weapons,
                rules,
                owner,
                name,
            );
        }
        !supers.is_empty()
    }
}
