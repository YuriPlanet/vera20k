//! A vehicle's own parts of `UnitClass::ReceiveDamage @ 0x00737C90`: the
//! war-factory return at its head and the computer-owner response at its
//! tail, after the `Harvester=` ping (`combat::world_receiver`).
//!
//! - `0x00737CB6..0x00737D2E`: a unit still inside the war factory it is in
//!   radio contact with takes no hit ([`Simulation::unit_inside_war_factory`]).
//! - `0x00738535..0x0073866E`: a computer's vehicle charges an attacker it
//!   can crush, or, as a damaged miner, heads for a dock
//!   ([`Simulation::unit_damage_response`]).
//!
//! Evidence: instruction reading of both blocks and of
//! `UnitClass::Should_Crush_It @ 0x007438F0` (Ghidra `UnitClass__ShouldCrashIt`).
//! Native execution: `tools/unit_crush_oracle.py` runs `0x007438F0` on scratch
//! objects; `unit_damage_response_tests` replays its rows through
//! [`CrushCharge::admits`]. The response body draws no random number and
//! writes no timer; the destination setter, the radio and the mission queue it
//! calls keep their own.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{Ability, ObjectType, PipScale};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::has_weapon_ability;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::bump_crush::{CrushTarget, object_is_crushable_by};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::{MaskedX87Ordering, sqrt_approx_length};

/// What `UnitClass::Should_Crush_It @ 0x007438F0` asks about a computer's
/// unit and the object that hit it, in its order.
///
/// Its human-owner arm (`0x007439B9..0x007439E6`: `[CombatDamage]
/// PlayerAutoCrush=`, `Rules+0x17EB`, or the type's `AutoCrush=`, `+0xD2D`)
/// is unreachable: its only caller (`0x00738583`) runs after
/// `0x0073856D..0x0073857A` has turned away an owner a human controls, and
/// both ask `IsControlledByHuman @ 0x0050B730` of the same house.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CrushCharge {
    /// UnitType `Crusher=` (`+0xD28`), or the rank's CRUSHER ability
    /// (`HasWeaponAbility(0x11) @ 0x0070D0D0`).
    pub(crate) crusher: bool,
    /// `ObjectClass::Is_Crushable_By @ 0x005F6CD0` of the source, the unit
    /// crushing. Its ally refusal is the response's own gate
    /// (`HouseClass::IsAlliedWith @ 0x004F9A90` of the unit's owner about the
    /// source, `0x0073855F`), already passed.
    pub(crate) crushable: bool,
    /// Leptons between both objects' GetCoords ([`crush_distance`]).
    pub(crate) distance: i32,
    /// `[CombatDamage] Crush=` (`Rules+0x1728`).
    pub(crate) reach: i32,
    /// The owner's difficulty (`+0x184`) is Easy (2).
    pub(crate) easy: bool,
    /// The owner's CurrentIQ (`+0x24C`).
    pub(crate) iq: i32,
    /// `[IQ] AutoCrush=` (`Rules+0x1448`).
    pub(crate) iq_auto_crush: i32,
    /// The source's `IsDisguisedTo(owner)` (vt+0xC8).
    pub(crate) disguised: bool,
}

impl CrushCharge {
    /// `0x007438F0` for an owner no human controls: a crusher, a crushable
    /// source within `Crush=` (`JG` refuses, `0x007439B3`), not Easy
    /// (`0x007439F5`), an IQ of at least `AutoCrush=` (`JL` refuses,
    /// `0x00743A19`), and a source not disguised to the owner (`SETE` on
    /// vt+0xC8, `0x00743A30`).
    pub(crate) fn admits(self) -> bool {
        self.crusher
            && self.crushable
            && self.distance <= self.reach
            && !self.easy
            && self.iq >= self.iq_auto_crush
            && !self.disguised
    }
}

/// `0x00743934..0x007439A6`: both objects' GetCoords (vt+0x48) differenced
/// unit minus source, `(dx² + dz²) + dy²` on the x87 stack, `Sqrt_Approx
/// @ 0x004CAC40` and the truncating ftol.
pub(crate) fn crush_distance(unit: [i32; 3], source: [i32; 3]) -> i32 {
    sqrt_approx_length([
        unit[0].wrapping_sub(source[0]),
        unit[2].wrapping_sub(source[2]),
        unit[1].wrapping_sub(source[1]),
    ])
}

/// vt+0x2B8 above zero for a `Harvester=` or `Weeder=` unit (the Unit thunk
/// `0x00740E50` to `TechnoClass 0x00708C30`): its pips under the type's
/// `PipScale=` (`+0x3D4`). `None` has none (retail's unloading classes HORV,
/// CMON and SMON). For `Tiberium` they are `ftol(total / Storage × pips +
/// 0.5)`, or 1 when that rounds to 0 with a total above zero, over the
/// storage total `0x006C9650` truncates as it sums. Harvesting stores whole
/// levels (`Harvest_Ore_Tick`), so the total is VERA's bale count, and with a
/// positive `Storage=` (`+0x800`) the pips are positive exactly when a bale
/// is aboard; a zero `Storage=` divides into no pips natively too.
///
/// Not represented, none in retail (`retail_harvesters_carry_tiberium_pips`):
/// a type whose `PipScale=` counts passengers or ammunition (cases 1 and 3)
/// answers no pips here, and so does a negative `Storage=`, whose lone level
/// natively can still round to one pip. A `Weeder=` unit carries no VERA
/// cargo (its harvest arm is a dormant residual in `miner::miner_system`).
fn pips_positive(entity: &GameEntity, object: &ObjectType) -> bool {
    object.pip_scale == PipScale::Tiberium
        && object.storage > 0
        && entity
            .miner
            .as_ref()
            .is_some_and(|miner| !miner.cargo().is_empty())
}

impl Simulation {
    /// `0x00737CB6..0x00737D2E`, unless the hit ignores defences: a unit
    /// whose radio contact 0 (`0x0065AD30(0)`) is a `WeaponsFactory=`
    /// building (`BuildingType+0x16BD`) standing on the unit's cell
    /// (`MapClass[Location]`, then the cell's first ground building,
    /// `0x0047C520`) returns 0 before `FootClass::ReceiveDamage`: no damage,
    /// team report or response. A vehicle still leaving its factory is the
    /// case.
    pub(crate) fn unit_inside_war_factory(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(unit) = self.substrate.entities.get(id) else {
            return false;
        };
        if unit.category() != EntityCategory::Unit {
            return false;
        }
        let Some(factory) = unit.radio_contacts.slot(0) else {
            return false;
        };
        self.substrate
            .entities
            .get(factory)
            .is_some_and(|building| {
                building.category() == EntityCategory::Structure
                    && self
                        .interner
                        .object_type(building.type_ref(), rules)
                        .is_some_and(|object| object.weapons_factory)
            })
            && self.substrate.occupancy.first_building_on_layer(
                unit.position.rx(),
                unit.position.ry(),
                MovementLayer::Ground,
            ) == Some(factory)
    }

    /// `0x00738535..0x0073866E`, reached for results 0 to 3 of
    /// `FootClass::ReceiveDamage` (result 0 skips only the ping,
    /// `0x007384BB`). A unit in no team (`+0x5D4`), hit by a source, not
    /// tethered (`+0x418`), whose owner is not allied to the source
    /// (`0x004F9A90`) and is no human's (`0x0050B730`):
    /// - charges a source it should crush ([`Self::unit_should_crush`]):
    ///   `SetDestination(source, 1)` (vt+0x480, `0x00741970`) and
    ///   `Queue_Mission(Move, 0)` (vt+0x1E8);
    /// - otherwise, as a `Harvester=` (`+0xE0E`) or `Weeder=` (`+0xE0F`) with
    ///   pips aboard ([`pips_positive`]) at or below `ConditionYellow`
    ///   (`GetHealthRatio @ 0x005F5C60` against `Rules+0x1700`: below, equal
    ///   or unordered), asks each `Dock=` type in order for a narrow bay
    ///   (vt+0x530, [`find_docking_bay_of_type`]) and takes the first answer;
    ///   a bay not already among its contacts (`0x0065AD50`) that answers its
    ///   HELLO (vt+0x278) with ROGER gets it `Queue_Mission(Enter, 0)`.
    ///
    /// A source no longer represented is a null source.
    ///
    /// [`find_docking_bay_of_type`]: crate::sim::miner::miner_system::find_docking_bay_of_type
    pub(crate) fn unit_damage_response(
        &mut self,
        id: u64,
        source: Option<u64>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        let Some((source, source_owner)) = source.and_then(|source| {
            let owner = self.substrate.entities.get(source)?.owner();
            Some((source, owner))
        }) else {
            return;
        };
        let owner = unit.owner();
        if unit.category() != EntityCategory::Unit
            || self.team_script_vm.team_for_member(id).is_some()
            || unit.tether.partner().is_some()
            || crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                owner,
                source_owner,
            )
            || self.owner_is_human(owner)
        {
            return;
        }
        let now = self.session.binary_frame;
        if self.unit_should_crush(id, source, rules) {
            self.set_unit_destination(
                id,
                NavTargetRef::Entity { id: source },
                rules,
                true,
                effects,
            );
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Move),
                0,
                now,
                &EntityReadyInputProvider,
            );
            return;
        }
        let Some(unit) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.interner.object_type(unit.type_ref(), rules) else {
            return;
        };
        if !(object.harvester || object.weeder)
            || !pips_positive(unit, object)
            || unit
                .health
                .compare_ratio(object.strength, rules.general.condition_yellow)
                == MaskedX87Ordering::Greater
        {
            return;
        }
        let Some(dock) = object.dock.iter().find_map(|dock_type| {
            crate::sim::miner::miner_system::find_docking_bay_of_type(
                self, rules, id, dock_type, effects,
            )
        }) else {
            return;
        };
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|unit| !unit.radio_contacts.contains(dock))
            && radio::transmit(
                self,
                id,
                dock,
                RadioMessage::Hello,
                RadioPayload::default(),
                Some(rules),
                effects,
            ) == RadioResponse::Roger
        {
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Enter),
                0,
                now,
                &EntityReadyInputProvider,
            );
        }
    }

    /// `UnitClass::Should_Crush_It @ 0x007438F0` from its only caller, the
    /// response above, for an owner no human controls ([`CrushCharge`]).
    fn unit_should_crush(&self, id: u64, source: u64, rules: &RuleSet) -> bool {
        let (Some(unit), Some(victim)) = (
            self.substrate.entities.get(id),
            self.substrate.entities.get(source),
        ) else {
            return false;
        };
        let Some(object) = self.interner.object_type(unit.type_ref(), rules) else {
            return false;
        };
        let Some(house) = self.houses.get(&unit.owner()) else {
            return false;
        };
        let terrain = self.resolved_terrain.as_ref();
        let coords = |entity: &GameEntity| {
            let coord = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
            [coord.x, coord.y, coord.z]
        };
        CrushCharge {
            crusher: object.crusher
                || has_weapon_ability(unit.veterancy.rank_kind(), object, Ability::Crusher),
            crushable: object_is_crushable_by(
                object.omni_crusher,
                CrushTarget::from_entity(victim, self.session.binary_frame),
            ),
            distance: crush_distance(coords(unit), coords(victim)),
            reach: rules.combat_damage.crush,
            easy: house.difficulty == crate::sim::house_state::HouseDifficulty::Easy,
            iq: house.current_iq,
            iq_auto_crush: rules.general.iq_auto_crush,
            disguised: crate::sim::cloak_disguise::object_disguised_to(
                victim,
                unit.owner(),
                Some(&self.fog),
                Some(&self.house_alliances),
                &self.interner,
            ),
        }
        .admits()
    }
}
