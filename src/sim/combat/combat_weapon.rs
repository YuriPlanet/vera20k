//! Weapon selection — the native `TechnoClass::What_Weapon_Should_I_Use`
//! ladder, its Infantry/Unit class overrides, the `NavalTargeting=` selector
//! and `TechnoClass::GetWeapon` (elite tier), with a garrison's occupant
//! weapon.
//!
//! Shape: `what_weapon_should_i_use` reproduces the native predicate ladder in
//! native order as one function with the same early returns and returns a
//! weapon-slot INDEX (never "no weapon"). `resolve_selected_weapon` resolves
//! that index through `GetWeapon`; its `None` means the slot names no weapon.
//! Neither asks whether the shot is legal: that is GetFireError
//! ([`super::fire_error`]), which each caller asks where native asks it.
//!
//! gamemd-derived (all read live this session):
//! - `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330` (vtable `+0x2E4` in
//!   the Techno/Foot/Building/Aircraft tables; `0x007F4C44` read back).
//! - `InfantryClass` override `@ 0x005218E0` (`0x007EB33C`), `UnitClass`
//!   override `@ 0x00746CD0` (`0x007F5F54`).
//! - `TechnoClass::SelectNavalTargetingWeapon @ 0x006F3820` (vtable `+0x2E8`).
//! - `TechnoClass::GetWeapon @ 0x0070E140` (elite tier) and the occupied arm
//!   of `BuildingClass::GetWeapon @ 0x004526F0`, with `BuildingClass::IsOccupied
//!   @ 0x00458DD0` and the occupied reach (`HalfFoundation @ 0x00458E00` plus
//!   `OccupyWeaponRange=`) that InRange and `Greatest_Threat` measure with.
//!
//! Executed query goldens: `tools/garrison_oracle/weapon_range.json` and its
//! identity/coverage sidecar; occupied InRange: `tools/spatial_oracle/walk_cell_range.json`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ terrain facts, and sim entity state.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::TargetKind;
use super::veterancy::RANK_ELITE_U16;
use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::armor::ArmorId;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::object_type::{ObjectType, WEAPON_SLOT_SECONDARY};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

/// Which weapon slot the unit is using for this engagement.
///
/// Used to resolve the correct FLH (firing offset) from art.ini:
/// Primary → `PrimaryFireFLH`, Secondary → `SecondaryFireFLH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WeaponSlot {
    Primary,
    Secondary,
}

/// The modeled fields of a native WeaponStruct, borrowed at the query point.
/// An existing record may have no WeaponType and still retain its FLH.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WeaponRecord<'a> {
    pub weapon_id: Option<&'a str>,
    pub flh: crate::rules::flh::Flh,
}

/// Result of weapon selection: the chosen weapon and its warhead.
pub struct SelectedWeapon<'a> {
    /// Section id of the selected weapon.
    pub weapon_id: &'a str,
    pub weapon: &'a WeaponType,
    pub warhead: &'a WarheadType,
    /// FLH slot: index 1 is `Secondary`, every other index is `Primary`.
    pub slot: WeaponSlot,
    /// Native weapon-array index returned by the selection ladder.
    pub index: i32,
}

/// Native `AbstractClass::WhatAmI` families the ladder distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TechnoKind {
    Infantry,
    Unit,
    Aircraft,
    Building,
}

impl TechnoKind {
    pub(crate) fn from_category(category: EntityCategory) -> Self {
        match category {
            EntityCategory::Infantry => Self::Infantry,
            EntityCategory::Unit => Self::Unit,
            EntityCategory::Aircraft => Self::Aircraft,
            EntityCategory::Structure => Self::Building,
        }
    }
}

/// Attacker-side instance state read by the selection ladder. Type data is
/// read from the attacker's `ObjectType` separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackerFacts {
    pub kind: TechnoKind,
    pub veterancy: u16,
    /// `TechnoClass+0x138 CurrentWeaponNumber` — the gunner slot. Constructor
    /// state 0; `SetGunnerWeapon` writes the passenger's `IFVMode`.
    pub current_weapon_number: i32,
    /// `Some(slot)` iff `InOpenToppedTransport (+0x82)` and the passenger
    /// type's `OpenTransportWeapon != -1` — the exact pair arm G tests.
    pub open_transport_weapon: Option<i32>,
    /// `TechnoClass+0x140 CurrentGattlingStage`.
    pub gattling_stage: i32,
    /// Infantry: `SequenceAnim (+0x6C4)` in {Deploy, Deployed, DeployedFire,
    /// DeployedIdle} (`0x005218E0`; `Undeploy` excluded). Unit: `+0x6E0`
    /// deployed flag, set only when the deploy animation finishes
    /// (`0x00746CD0`). False for every other class.
    pub deploy_fire_active: bool,
    /// `BuildingClass::IsOccupied @ 0x00458DD0` (vtable `+0x400`; the base
    /// `0x0041BFB0` returns 0 for every non-building class).
    pub is_occupied_building: bool,
    /// `TechnoClass+0x1CC DrainTarget != NULL`.
    pub drain_target_active: bool,
    /// `MissionClass::GetCurrentMission (vt+0x184, 0x005B3040) == 0x10
    /// (Unload)` — current mission, or the queued one when none is current.
    pub mission_is_unload: bool,
    /// `BuildingClass+0x661 IsOverpowered`.
    pub is_overpowered_building: bool,
    /// `AircraftClass+0x6CA` spawn retreat/collision flag
    /// (`SpawnRetreat__Push 0x0054E47D`).
    pub aircraft_spawn_collision: bool,
}

/// Flight virtuals are queried only where the selection ladder asks them.
/// Gathering target facts must not eagerly move the canonical fallback cell.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TargetFlight<'a> {
    Live {
        entity: &'a GameEntity,
        terrain: Option<&'a ResolvedTerrainGrid>,
        rules: &'a RuleSet,
        interner: &'a StringInterner,
    },
    InQuery {
        entity: &'a GameEntity,
        cells: &'a NativeCellQuery<'a>,
        rules: &'a RuleSet,
        interner: &'a StringInterner,
    },
    #[cfg(any(test, feature = "test-support"))]
    Supplied { high: bool },
}

impl TargetFlight<'_> {
    fn high(self) -> bool {
        match self {
            Self::Live {
                entity,
                terrain,
                rules,
                interner,
            } => crate::sim::movement::air_movement::is_high_flying(
                entity,
                terrain,
                Some((rules, interner)),
            ),
            Self::InQuery {
                entity,
                cells,
                rules,
                interner,
            } => crate::sim::movement::air_movement::is_high_flying_in_query(
                entity,
                cells,
                Some((rules, interner)),
            ),
            #[cfg(any(test, feature = "test-support"))]
            Self::Supplied { high } => high,
        }
    }
}

/// Target-side facts read by the ladder.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TargetFacts<'a> {
    /// Real TerrainClass Object target: neither CellClass nor TechnoClass.
    /// Used by Unit73FBAC ->746CD0/6F3330 for repair's Wood-warhead query.
    Terrain,
    /// `CellClass` target (force-fire on terrain).
    Cell {
        /// `CellClass+0xEC LandType`.
        land_type: u8,
        /// The cell's tile is in the theater's WaterSet: the negation of
        /// CellClass vt+0x50 (`0x004867E0`, see
        /// [`ResolvedTerrainGrid::native_cell_is_water_set_tile`]).
        tile_in_water_set: bool,
        /// `CellClass+0x140 Flags & 0x100` (structural bridge cell).
        bridge_flag: bool,
    },
    /// Any `TechnoClass` target.
    Techno {
        obj: &'a ObjectType,
        kind: TechnoKind,
        /// `ObjectClass::IsHighFlying @ 0x005F6B90` (vtable `+0x54`):
        /// marked on the map and `GetHeight() >= 2 * LeptonsPerLevel`.
        flight: TargetFlight<'a>,
        /// `FootClass+0x8C OnBridge`.
        on_bridge: bool,
        /// `LandType` of the target's occupied cell (`vt+0x1BC` → `+0xEC`).
        cell_land_type: u8,
        /// `TechnoClass+0x220 CloakState != 0` — a submerged/cloaking sub.
        submerged: bool,
        /// `HouseClass::Is_Ally_ByObject @ 0x004F9A90` of the attacker's
        /// owner against this target.
        is_ally: bool,
    },
}

/// ParasiteClass CanInfect `0x0062A8E0`, the one implementation both callers
/// use (AttachTo and GetFireError): a live, unlimboed, uninfected,
/// Parasiteable Foot outside a tank bunker, plus the water-set cell a Naval
/// owner requires. GetFireError returns ILLEGAL (5) otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParasiteVictimFacts {
    pub(crate) infectable: bool,
    pub(crate) on_water_set: bool,
}

impl ParasiteVictimFacts {
    /// The water gate (`0x0062A950..0x0062A96D`) tests the victim's GetCell
    /// (vt+0x1BC, `0x005F6960` for every Foot): the cell at its Location, or
    /// off the map the shared dummy, whose tile 0xFFFF is never WaterSet. A
    /// fixture without a map passes. Native comparison:
    /// `tools/spatial_oracle/parasite_water_gate.json`.
    pub(crate) fn of(
        target: &GameEntity,
        target_obj: &ObjectType,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Self {
        let on_water_set =
            terrain.is_none_or(|grid| {
                grid.native_cell_is_water_set_tile(grid.native_cell_identity((
                    target.position.rx() as i16,
                    target.position.ry() as i16,
                )))
            });
        Self {
            infectable: matches!(
                target.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            ) && !target.lifecycle.in_limbo()
                && target.lifecycle.object_alive()
                && target.health.current() != 0
                && target.parasite.eater().is_none()
                && target_obj.parasiteable
                && target.bunker.link().installed_in().is_none(),
            on_water_set,
        }
    }

    /// The owner half: a Naval owner also needs the water-set victim cell.
    pub(crate) fn admits(self, naval_owner: bool) -> bool {
        self.infectable && (!naval_owner || self.on_water_set)
    }
}

impl TargetFacts<'_> {
    fn techno(&self) -> Option<&Self> {
        match self {
            TargetFacts::Techno { .. } => Some(self),
            TargetFacts::Cell { .. } | TargetFacts::Terrain => None,
        }
    }

    fn is_high_flying(&self) -> bool {
        match self {
            TargetFacts::Techno { flight, .. } => flight.high(),
            // `CellClass` vtable `+0x54` @ `0x00410530` returns 0.
            TargetFacts::Cell { .. } | TargetFacts::Terrain => false,
        }
    }

    fn kind(&self) -> Option<TechnoKind> {
        match self {
            TargetFacts::Techno { kind, .. } => Some(*kind),
            TargetFacts::Cell { .. } | TargetFacts::Terrain => None,
        }
    }
}

/// [`fire_error::weapon_damage_value`](super::fire_error::weapon_damage_value)
/// (`0x006F3970(-1)`) of a live object, through GetWeapon ([`get_weapon`],
/// vt+0x3F8) as native reads it, so an occupied building answers with its
/// occupant's weapon. GetFireError asks it through
/// `FireSubject::weapon_damage_value`.
pub fn weapon_damage_value(
    entity: &GameEntity,
    obj: &ObjectType,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> i32 {
    super::fire_error::weapon_damage_value(
        obj.turret_count,
        obj.is_gattling,
        attacker_facts(entity, obj).current_weapon_number,
        |slot| {
            get_weapon(entity, obj, slot, entities, rules, interner)
                .map(|weapon| weapon.damage.wrapping_add(weapon.ambient_damage))
        },
    )
}

/// The house (`+0x21C`) of whoever mind-controls (`+0x2C0`) the
/// `OpenTopped=` transport (`+0x11C`, TechnoType `+0x5E4`) `rider` rides:
/// the threat scan's (`0x006F7EA2..0x006F7ED8`, `0x006F89D1..0x006F8A0A`)
/// and the allied-target drop's (`0x006FA30C..0x006FA342`).
pub(crate) fn controlled_transport_house(
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    rider: &GameEntity,
) -> Option<InternedId> {
    let transport = entities.get(rider.passenger_role.inside_transport_id()?)?;
    let open_topped = interner
        .object_type(transport.type_ref(), rules)
        .is_some_and(|obj| obj.open_topped);
    open_topped
        .then(|| transport.mind_control.controller())
        .flatten()
        .and_then(|controller| entities.get(controller))
        .map(|controller| controller.owner())
}

/// `TechnoClass::GetWeapon @ 0x0070E140`: `-1` → no weapon; elite objects use
/// `EliteWeapon[idx]` when that slot names a weapon, else `Weapon[idx]`.
/// Veteran tier does not swap. Indices outside the 18-slot array are treated
/// as no weapon (native would read past the array — UNCHECKED, unreachable
/// with stock data).
pub(crate) fn weapon_for_index(
    obj: &ObjectType,
    veterancy: u16,
    index: i32,
) -> Option<WeaponRecord<'_>> {
    if index < 0 {
        return None;
    }
    let slot_index = usize::try_from(index).ok()?;
    // Ares7177C0/7177E0 ->1003DBC0 exposes the extension WeaponStruct
    // vectors beyond the original eighteen. The type owns their lifetime.
    let slot_count = obj.weapon_record_count();
    if slot_index >= slot_count {
        return None;
    }
    let elite = uses_elite_weapon(obj, veterancy, slot_index);
    let weapon_id = if elite {
        obj.elite_weapon_at(slot_index)
    } else {
        obj.weapon_at(slot_index)
    };
    Some(WeaponRecord {
        weapon_id,
        flh: obj.weapon_flh(index, elite),
    })
}

/// `TechnoClass::IsNotSprayAttack @ 0x0070DD70`: the type's `SprayAttack`
/// byte at `+0x691`, negated. `GetDeployWeapon @ 0x0070E120` consumes this
/// full 0/1 result as its `GetWeapon @ 0x0070E140` index.
pub(crate) fn is_not_spray_attack(obj: &ObjectType) -> bool {
    !obj.spray_attack
}

/// `TechnoClass::GetDeployWeapon @ 0x0070E120`: choose slot 1 unless the type
/// is `SprayAttack`, then forward that index through the sole `GetWeapon`
/// owner above. The index remains available when the selected WeaponStruct's
/// WeaponType pointer is null, as it is in native's fixed slot array.
pub(crate) fn deploy_weapon(obj: &ObjectType, veterancy: u16) -> (i32, Option<(&str, WeaponSlot)>) {
    let index = if is_not_spray_attack(obj) { 1 } else { 0 };
    let weapon = weapon_for_index(obj, veterancy, index)
        .and_then(|record| record.weapon_id)
        .map(|id| {
            (
                id,
                if index == 1 {
                    WeaponSlot::Secondary
                } else {
                    WeaponSlot::Primary
                },
            )
        });
    (index, weapon)
}

/// Whether `TechnoClass::GetWeapon @ 0x0070E140` answers slot `index` with the
/// elite record: an elite object (`VeterancyClass::IsElite`) whose
/// `EliteWeapon[index]` names a weapon. Callers that read the record's other
/// fields (the elite FLH) ask this too.
pub(crate) fn uses_elite_weapon(obj: &ObjectType, veterancy: u16, index: usize) -> bool {
    veterancy >= RANK_ELITE_U16 && obj.elite_weapon_at(index).is_some()
}

/// `GetWeapon(0)` weapon id at the given veterancy.
pub(crate) fn primary_for_tier(obj: &ObjectType, veterancy: u16) -> Option<&str> {
    weapon_for_index(obj, veterancy, 0).and_then(|record| record.weapon_id)
}

/// TechnoType query `0x00712130`: normal primary is required; either the
/// two records share a WeaponType or one has signed `Burst > 1`.
/// Building Mission_Attack44B2D0 asks the first installed upgrade's type,
/// independently of the host's rank, selected index and actual GetWeapon.
pub(crate) fn has_shared_or_bursting_weapons(obj: &ObjectType, rules: &RuleSet) -> bool {
    let Some(primary) = obj.primary() else {
        return false;
    };
    let secondary = obj.weapon_at(WEAPON_SLOT_SECONDARY);
    secondary == Some(primary)
        || rules.weapon(primary).is_some_and(|weapon| weapon.burst > 1)
        || secondary
            .and_then(|id| rules.weapon(id))
            .is_some_and(|weapon| weapon.burst > 1)
}

/// `GetWeapon(1)` weapon id at the given veterancy.
pub(crate) fn secondary_for_tier(obj: &ObjectType, veterancy: u16) -> Option<&str> {
    weapon_for_index(obj, veterancy, 1).and_then(|record| record.weapon_id)
}

/// The native "does this object have a weapon" predicate. **Exactly one weapon
/// slot decides.**
///
/// gamemd-derived: `TechnoClass::Is_Armed @ 0x00701120` (vtable `+0x2AC`;
/// `read_memory 0x007F4C0C`, and the Aircraft/Foot/Infantry/Unit tables at
/// `0x007E2550`/`0x007E8F40`/`0x007EB304`/`0x007F5F1C` hold the same body) is
/// `w = GetCurrentWeapon(); w != NULL && w->WeaponType != NULL`.
/// `TechnoClass::GetCurrentWeapon @ 0x0070E1A0` (vtable `+0x3F4`) asks
/// `GetWeapon(CurrentWeaponNumber (+0x138))` when
/// `TechnoTypeClass::HasTurrets @ 0x00717880` (`TurretCount (+0x808) > 0`) and
/// `GetWeapon(0)` otherwise. `BuildingClass::Is_Armed @ 0x00458DB0`
/// (`read_memory 0x007E4168`) overrides the slot: an occupied building
/// (`vt+0x400`, `BuildingClass::IsOccupied 0x00458DD0`) is armed
/// unconditionally, otherwise it falls through to the Techno test.
///
/// A non-turret type is armed only through slot 0; a `TurretCount>0`
/// type is armed only through its current gunner slot — which is why this is
/// still a distinct predicate from reading `obj.primary()`, even now that
/// `obj.primary()` is the native `Primary` field for every type (see
/// `native_processing::WeaponSlots`). The elite tier is applied by `GetWeapon`,
/// so the read goes through the live [`current_weapon`] query.
///
/// Verified call sites of this predicate, all reached in ordinary play:
/// - `TechnoClass::CanAcquireTarget @ 0x007091D0` ends with
///   `vt+0x2AC` — the gate behind every passive target scan
///   (`TechnoClass::PassiveAcquireGate 0x00709290` consumes it).
/// - `TechnoClass::What_Action_OnCell @ 0x00700600` inlines the whole body at
///   `0x007008BD..0x007008CE` (`CALL [EDX+0x3F4]` / `TEST EAX,EAX` /
///   `CMP dword [EAX],0x0`, both misses jumping past the ATTACK arms to
///   `0x00700AB7`) — the force-fire cursor and order gate.
/// - `TechnoClass::RespondToBaseAttack @ 0x00708080` victim/candidate gates and
///   `FootClass::Evaluate_Target_Threat @ 0x004D97A0`.
/// - `UnitClass::Can_Enter_Cell @ 0x0073F0A0` wall-overlay and enemy-gate arms.
///
pub fn is_armed(
    entity: &GameEntity,
    obj: &ObjectType,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> bool {
    // Building458DB0 returns true before GetCurrentWeapon for an occupied
    // host, even when the current firing occupant has no weapon.
    is_occupied(entity, obj) || current_weapon(entity, obj, entities, rules, interner).is_some()
}

/// Aircraft auxiliary+18/41B7F0: the primary projectile selects the attack
/// approach and release branches. All callers use the live weapon tier.
pub(crate) fn aircraft_strafes(rules: &RuleSet, obj: &ObjectType, veterancy: u16) -> bool {
    primary_for_tier(obj, veterancy)
        .and_then(|name| rules.weapon(name))
        .and_then(|weapon| weapon.projectile.as_deref())
        .and_then(|name| rules.projectile(name))
        .is_some_and(|projectile| projectile.rot <= 1 && !projectile.inviso)
}

/// `TechnoClass::GetWeaponRange @ 0x007012C0` (vt+0x168, no class
/// override): the raw signed range of `GetWeapon(index)`, 0 without one,
/// capped by [`open_topped_cargo_range`]. It deliberately excludes InRange's
/// bonuses and range overrides. Read by the flak scatter, the aircraft fire
/// location, the scan radius (`Threat_Range`, `Greatest_Threat`), the
/// base-defence threat score and an open-topped mover's per-cell stop.
pub(crate) fn weapon_range(
    entity: &GameEntity,
    obj: &ObjectType,
    index: i32,
    entities: &crate::sim::entity_store::EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> i32 {
    let Some(weapon) = get_weapon(entity, obj, index, entities, rules, interner) else {
        return 0;
    };
    let source =
        crate::sim::attach_effect::range_modifier_source(entity, Some(entities), rules, interner);
    let range = crate::sim::attach_effect::range_with_attach_effects(
        source,
        &weapon.id,
        weapon.range_leptons,
        rules,
    );
    open_topped_cargo_range(entity, obj, entities, rules, interner)
        .map_or(range, |cargo| range.min(cargo))
}

/// GetWeaponRange's `OpenTopped=` arm (`0x007012ED..`, the same walk as
/// `0x00710590`): the shortest range among the passengers' current weapons
/// (`GetCurrentWeapon`, vt+0x3F4), walked from the cargo head. `None` when
/// `obj` is not open-topped or no passenger is armed: native starts from
/// `INT_MAX`, which caps nothing.
pub(crate) fn open_topped_cargo_range(
    entity: &GameEntity,
    obj: &ObjectType,
    entities: &crate::sim::entity_store::EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> Option<i32> {
    if !obj.open_topped {
        return None;
    }
    let mut shortest = None;
    // Cargo owns the native head/+30 sequence. Neither limbo nor death
    // filters belong to this reader; boarded passengers are in limbo.
    for &id in entity.passenger_role.cargo()?.passengers() {
        let Some(passenger) = entities.get(id) else {
            break;
        };
        let Some(kind) = interner.object_type(passenger.type_ref(), rules) else {
            continue;
        };
        if let Some(weapon) = current_weapon(passenger, kind, entities, rules, interner) {
            let source = crate::sim::attach_effect::range_modifier_source(
                passenger,
                Some(entities),
                rules,
                interner,
            );
            let range = crate::sim::attach_effect::range_with_attach_effects(
                source,
                &weapon.id,
                weapon.range_leptons,
                rules,
            );
            shortest = Some(shortest.map_or(range, |shortest: i32| shortest.min(range)));
        }
    }
    shortest
}

fn current_weapon_index(obj: &ObjectType, facts: AttackerFacts) -> i32 {
    // GetCurrentWeapon70E1A0 reads CurrentWeaponNumber only with HasTurrets.
    if obj.turret_count > 0 {
        facts.current_weapon_number
    } else {
        0
    }
}

/// `TechnoClass::GetCurrentWeapon @ 0x0070E1A0` (vt+0x3F4): GetWeapon
/// (vt+0x3F8, [`get_weapon`]) of the gunner slot of a `TurretCount>0` type,
/// slot 0 otherwise, so an occupied building answers its firing occupant's
/// weapon.
pub fn current_weapon<'r>(
    entity: &GameEntity,
    obj: &'r ObjectType,
    entities: &EntityStore,
    rules: &'r RuleSet,
    interner: &StringInterner,
) -> Option<&'r WeaponType> {
    let index = current_weapon_index(obj, attacker_facts(entity, obj));
    get_weapon(entity, obj, index, entities, rules, interner)
}

/// `TechnoClass::SelectNavalTargetingWeapon @ 0x006F3820` (vtable `+0x2E8`,
/// no class overrides). Returns `-1` (cannot engage on water), `0` or `1`.
///
/// Switches on the ATTACKER type's `NavalTargeting=`; reads the target
/// type's `Underwater`, `Organic`, `SpeedType`, `Unnatural` and the target's
/// `CloakState`. A non-Techno target (cell, null) returns `-1`.
pub(crate) fn select_naval_targeting_weapon(obj: &ObjectType, target: Option<&TargetFacts>) -> i32 {
    let Some(TargetFacts::Techno {
        obj: target_obj,
        submerged,
        ..
    }) = target
    else {
        return -1;
    };
    match obj.naval_targeting {
        // Default: anything surfaced is fair game; a submerged sub is not.
        0 => {
            if !target_obj.underwater {
                0
            } else if *submerged {
                -1
            } else {
                0
            }
        }
        // Destroyer: depth charge against subs, deck gun otherwise.
        1 => i32::from(target_obj.underwater),
        // ASW: subs only.
        2 => {
            if target_obj.underwater {
                0
            } else {
                -1
            }
        }
        // Squid: punch organics and unnaturals, grab the rest.
        3 => i32::from(target_obj.organic || target_obj.unnatural),
        // Tanya/Boris/Ghost: C4 unless hover-naval or organic. SpeedType 3 is
        // `Hover` per VERA's binary-ordered enum table (`0x0081DA58`).
        4 => i32::from(target_obj.speed_type != SpeedType::Hover && !target_obj.organic),
        // Dogs / AEGIS / flak / SAM / Chaos drone / Terror drone: never.
        6 => -1,
        // 5 (`break`) and every unlisted value fall out to slot 0.
        _ => 0,
    }
}

fn projectile_aa(rules: &RuleSet, weapon: &WeaponType) -> bool {
    weapon
        .projectile
        .as_ref()
        .and_then(|id| rules.projectile(id))
        .is_some_and(|projectile| projectile.aa)
}

/// `0x00772A90`: a weapon's class bits in a threat mask, from its
/// projectile (`Weapon+0xA0`): `AA=` (`+0x2A4`) gives `4`, `AG=` (`+0x2A5`)
/// ORs `0xB8`. A weapon naming no projectile section reads the reader's
/// defaults (AA=no, AG=yes), as weapon selection does; retail weapons always
/// name one.
fn weapon_class_bits(rules: &RuleSet, weapon: &WeaponType) -> u32 {
    let (aa, ag) = weapon
        .projectile
        .as_ref()
        .and_then(|id| rules.projectile(id))
        .map_or((false, true), |projectile| (projectile.aa, projectile.ag));
    let mut bits = 0;
    if aa {
        bits = 4;
    }
    if ag {
        bits |= 0xB8;
    }
    bits
}

/// The weapon class bits a scanner's `Greatest_Threat` override ORs into its
/// mask ([`super::threat_mask`]). `UnitClass @ 0x007431C9..0x0074324D` takes
/// only the CurrentWeaponNumber weapon when `TurretCount>0` and not
/// `IsGattling`, otherwise slots 0 and 1; `InfantryClass @
/// 0x0051E2C7..0x0051E319` and `BuildingClass @ 0x00445F00` take slots 0
/// and 1. Each slot is the scanner's GetWeapon ([`get_weapon`]), so an
/// occupied building answers its firing occupant's weapon for either index.
pub(crate) fn passive_scan_class_bits<'r>(
    rules: &'r RuleSet,
    obj: &ObjectType,
    attacker: AttackerFacts,
    get_weapon: impl Fn(i32) -> Option<&'r WeaponType>,
) -> u32 {
    let slot_bits = |index| get_weapon(index).map_or(0, |weapon| weapon_class_bits(rules, weapon));
    if attacker.kind == TechnoKind::Unit && obj.turret_count > 0 && !obj.is_gattling {
        slot_bits(attacker.current_weapon_number)
    } else {
        slot_bits(0) | slot_bits(1)
    }
}

/// Warhead `+0x149`, which the Verses parse sets (`0x0075DE5A..0x0075DE9A`)
/// when the exact doubles `Verses[4]` and `Verses[6]` are both `0.0`, read
/// through slot 0's weapon at the live tier (`0x0051E329..0x0051E343`).
/// Ares3.0 reader hook75DDCC ->10054410 skips that derivation and clears
/// the flag at75DE98. The rule owner retains this distinction from its table.
pub(crate) fn primary_warhead_spares_medium_and_wood(
    rules: &RuleSet,
    obj: &ObjectType,
    veterancy: u16,
) -> bool {
    let warhead = primary_for_tier(obj, veterancy)
        .and_then(|id| rules.weapon(id))
        .and_then(|weapon| warhead_of(rules, weapon));
    warhead.is_some_and(WarheadType::uses_medium_wood_target_mask)
}

pub(crate) fn warhead_of<'a>(rules: &'a RuleSet, weapon: &WeaponType) -> Option<&'a WarheadType> {
    weapon.warhead.as_ref().and_then(|id| rules.warhead(id))
}

/// Native `FCOMP Verses[armor], 0.0` (`0x006F3705` / `0x006F3738`): the
/// double is compared for exact equality with zero.
fn verses_is_zero(warhead: Option<&WarheadType>, armor: ArmorId) -> bool {
    warhead.is_some_and(|warhead| {
        // Ares 3.0 hook10054360 retains numeric zero and secondary-first
        // ordering. Its independent fire/acquisition flags do not select slots.
        f64::from_bits(warhead.interaction(armor).multiplier().bits()) == 0.0
    })
}

/// Weapon-slot selection with the class overrides that run BEFORE the base
/// ladder. Returns the weapon-array index native returns; it never returns
/// "no weapon" — legality is `GetFireError`'s job ([`super::fire_error`]).
///
/// - `InfantryClass @ 0x005218E0`: a `DeployFire=yes` infantry never enters
///   the ladder — deployed sequences fire `DeployFireWeapon`, otherwise
///   `OpenTransportWeapon` inside an open-topped transport, otherwise slot 0.
/// - `UnitClass @ 0x00746CD0`: a deployed (`+0x6E0`) `DeployFire=yes` vehicle
///   fires `DeployFireWeapon`; everything else runs the ladder.
pub(crate) fn what_weapon_should_i_use<'r>(
    rules: &'r RuleSet,
    obj: &ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetFacts>,
    get_weapon: impl Fn(i32) -> Option<&'r WeaponType>,
) -> i32 {
    match attacker.kind {
        TechnoKind::Infantry if obj.deploy_fire => {
            if attacker.deploy_fire_active {
                return obj.deploy_fire_weapon;
            }
            return attacker.open_transport_weapon.unwrap_or(0);
        }
        TechnoKind::Unit if attacker.deploy_fire_active && obj.deploy_fire => {
            return obj.deploy_fire_weapon;
        }
        _ => {}
    }
    techno_what_weapon_should_i_use(rules, obj, attacker, target, get_weapon)
}

/// Whether the airstrike secondary is selected for a techno target.
///
/// Phobos gates it twice (`TechnoClass_WhatWeaponShouldIUse_Airstrike`,
/// `0x6F348F`): the warhead's `AirstrikeTargets` must admit the kind, and the
/// target type's `AllowAirstrike` must permit the type, defaulting to `CanC4`
/// for a building and to targetable for every other kind. Without the feature
/// this is the vanilla arm: a C4-able building that is not both a resource
/// destination and a resource gatherer.
fn airstrike_secondary_selected(
    warhead: &WarheadType,
    target_obj: &ObjectType,
    kind: TechnoKind,
) -> bool {
    #[cfg(feature = "phobos")]
    {
        use crate::rules::airstrike_targets::AirstrikeKind;
        let target_kind = match kind {
            TechnoKind::Infantry => AirstrikeKind::Infantry,
            TechnoKind::Unit => AirstrikeKind::Unit,
            TechnoKind::Aircraft => AirstrikeKind::Aircraft,
            TechnoKind::Building => AirstrikeKind::Building,
        };
        warhead.airstrike_targets.allows(target_kind)
            && target_obj.allows_airstrike_weapon(kind == TechnoKind::Building)
    }
    #[cfg(not(feature = "phobos"))]
    {
        let _ = warhead;
        kind == TechnoKind::Building
            && target_obj.can_c4
            && (!target_obj.resource_destination || !target_obj.resource_gatherer)
    }
}

/// `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330`, arm by arm in native
/// order. Addresses name the first instruction of each arm.
fn techno_what_weapon_should_i_use<'r>(
    rules: &'r RuleSet,
    obj: &ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetFacts>,
    get_weapon: impl Fn(i32) -> Option<&'r WeaponType>,
) -> i32 {
    // A @ 0x006F333B: turreted non-gattling types fire the gunner slot,
    // unconditionally (`-1` collapses to 0).
    if obj.turret_count > 0 && !obj.is_gattling {
        return if attacker.current_weapon_number == -1 {
            0
        } else {
            attacker.current_weapon_number
        };
    }
    // B @ 0x006F337D: an occupied building always reports slot 0
    // (`BuildingClass::GetWeapon` then substitutes the occupant weapon).
    if attacker.is_occupied_building {
        return 0;
    }
    // C @ 0x006F3391 / D @ 0x006F33AB: both slots must resolve to a weapon.
    let Some(secondary) = get_weapon(1) else {
        return 0;
    };
    let Some(primary) = get_weapon(0) else {
        return 0;
    };
    // E @ 0x006F33BF.
    if secondary.never_use {
        return 0;
    }
    // F @ 0x006F33CD.
    let Some(target) = target else {
        return 0;
    };
    // G @ 0x006F33D9: open-topped passengers fire their OpenTransportWeapon.
    if let Some(slot) = attacker.open_transport_weapon {
        return slot;
    }
    let techno = target.techno();
    let secondary_aa = projectile_aa(rules, secondary);
    let secondary_warhead = warhead_of(rules, secondary);
    let primary_warhead = warhead_of(rules, primary);
    // H @ 0x006F3422: gattling stage pair. Note the AA test reads
    // `GetWeapon(1)`'s projectile regardless of the current stage.
    if obj.is_gattling {
        // `LEA` at `0x006F345C`/`0x006F346A`: no clamp, and `2s` wraps.
        let stage = attacker.gattling_stage;
        if secondary_aa && techno.is_some_and(TargetFacts::is_high_flying) {
            return stage.wrapping_mul(2).wrapping_add(1);
        }
        return stage.wrapping_mul(2);
    }
    // I @ 0x006F3477: Airstrike secondary. Phobos adds two gates here
    // (`TechnoClass_WhatWeaponShouldIUse_Airstrike`, `0x6F348F`).
    if let Some(warhead) = secondary_warhead.filter(|warhead| warhead.airstrike) {
        let TargetFacts::Techno {
            obj: target_obj,
            kind,
            ..
        } = target
        else {
            return 0;
        };
        return i32::from(airstrike_secondary_selected(warhead, target_obj, *kind));
    }
    // J @ 0x006F3528: IsLocomotor primary (Magnetron) vs a building.
    if primary_warhead.is_some_and(|warhead| warhead.is_locomotor)
        && target.kind() == Some(TechnoKind::Building)
    {
        return 1;
    }
    // K @ 0x006F3558: DrainWeapon secondary vs a Drainable non-ally while not
    // already draining.
    if secondary.drain_weapon
        && let Some(TargetFacts::Techno {
            obj: target_obj,
            is_ally,
            ..
        }) = techno
        && target_obj.drainable
        && !attacker.drain_target_active
        && !is_ally
    {
        return 1;
    }
    // L @ 0x006F35A8: AreaFire secondary while unloading.
    if secondary.area_fire && attacker.mission_is_unload {
        return 1;
    }
    // M @ 0x006F35D4: an overpowered building fires its secondary.
    if attacker.kind == TechnoKind::Building && attacker.is_overpowered_building {
        return 1;
    }
    // N @ 0x006F35F9: ElectricAssault secondary charges an allied
    // Overpowerable building.
    if let TargetFacts::Techno {
        obj: target_obj,
        kind: TechnoKind::Building,
        is_ally: true,
        ..
    } = target
        && secondary_warhead.is_some_and(|warhead| warhead.electric_assault)
        && target_obj.overpowerable
    {
        return 1;
    }
    // O @ 0x006F3648: an aircraft pushed to spawn-retreat/collision.
    if attacker.kind == TechnoKind::Aircraft && attacker.aircraft_spawn_collision {
        return 1;
    }
    // P @ 0x006F3671: cell target on land (or a bridge cell for a Naval
    // type) with LandTargeting=2. The cell IsHighFlying test is always false.
    if let TargetFacts::Cell {
        land_type,
        tile_in_water_set,
        bridge_flag,
    } = *target
    {
        let on_land = land_type != LandType::Water.as_index() && !tile_in_water_set;
        if (on_land || (bridge_flag && obj.naval)) && obj.land_targeting == 2 {
            return 1;
        }
    }
    // Q @ 0x006F36DB.
    let Some(TargetFacts::Techno {
        obj: target_obj,
        flight,
        on_bridge,
        cell_land_type,
        ..
    }) = techno
    else {
        return 0;
    };
    let armor = target_obj.armor_id();
    // R @ 0x006F36E3 / S @ 0x006F3716: the only Verses reads, both `== 0.0`.
    if verses_is_zero(secondary_warhead, armor) {
        return 0;
    }
    if verses_is_zero(primary_warhead, armor) {
        return 1;
    }
    // T @ 0x006F3754: a target on water (and not high-flying, not on a
    // bridge deck) goes through the naval selector; `-1` collapses to 0 here
    // and only GetFireError turns it into ILLEGAL.
    let mut on_water = *cell_land_type == LandType::Water.as_index()
        || *cell_land_type == LandType::Beach.as_index();
    if flight.high() {
        on_water = false;
    }
    if !*on_bridge && on_water {
        let naval = select_naval_targeting_weapon(obj, Some(target));
        if naval != -1 {
            return naval;
        }
        return 0;
    }
    // U @ 0x006F37B9.
    if !flight.high() && obj.land_targeting == 2 {
        return 1;
    }
    // V @ 0x006F37E7: unconditional AA-secondary against a high-flying target.
    if secondary_aa && flight.high() {
        return 1;
    }
    0
}

/// Native SelectWeapon (vt+0x2E4) and GetWeapon (vt+0x3F8, [`get_weapon`])
/// of a live object: an occupied building's index resolves to its firing
/// occupant's weapon. It asks no legality: Evaluate owns the subsequent full
/// fire-error query; Mission_Attack 418432..418476 selects on each burst
/// iteration after one admission only.
pub(crate) fn resolve_selected_weapon<'a>(
    rules: &'a RuleSet,
    entity: &GameEntity,
    obj: &'a ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetFacts>,
    entities: &EntityStore,
    interner: &StringInterner,
) -> Option<SelectedWeapon<'a>> {
    let index = what_weapon_should_i_use(rules, obj, attacker, target, |index| {
        get_weapon(entity, obj, index, entities, rules, interner)
    });
    resolve_weapon_index(rules, entity, obj, index, entities, interner)
}

/// GetWeapon (vt+0x3F8, [`get_weapon`]) of an index SelectWeapon answered,
/// with its warhead: an occupied building's firing occupant's weapon,
/// otherwise the object's own slot at its live rank. A delayed building
/// shot resolves its saved index here without re-running the ladder.
pub(crate) fn resolve_weapon_index<'a>(
    rules: &'a RuleSet,
    entity: &GameEntity,
    obj: &'a ObjectType,
    index: i32,
    entities: &EntityStore,
    interner: &StringInterner,
) -> Option<SelectedWeapon<'a>> {
    let weapon = get_weapon(entity, obj, index, entities, rules, interner)?;
    Some(SelectedWeapon {
        weapon_id: &weapon.id,
        weapon,
        warhead: warhead_of(rules, weapon)?,
        slot: if index == 1 {
            WeaponSlot::Secondary
        } else {
            WeaponSlot::Primary
        },
        index,
    })
}

/// `HouseClass::Is_Ally_ByObject @ 0x004F9A90` reduced to house identity:
/// same house, or the asker's alliance bit for the other house. Without an
/// alliance map only the same-house case is knowable.
pub fn is_ally_by_object(
    alliances: Option<&HouseAllianceMap>,
    interner: &StringInterner,
    asker: InternedId,
    other: InternedId,
) -> bool {
    asker == other
        || alliances.is_some_and(|map| {
            is_allied_with(map, interner.resolve(asker), interner.resolve(other))
        })
}

/// Arm G's pair (`0x006F33D9`) and the DeployFire infantry arm's
/// (`0x005218E0`): `+0x82` and the type's `OpenTransportWeapon (+0xD50) != -1`.
fn open_transport_weapon(in_open_transport: bool, obj: &ObjectType) -> Option<i32> {
    (in_open_transport && obj.open_transport_weapon != -1).then_some(obj.open_transport_weapon)
}

/// Read the attacker facts from live entity state.
///
/// `drain_target_active` reads the live drain link (`TechnoClass+0x1CC`,
/// [`Drain`](crate::sim::credit_income::Drain)).
///
/// RESIDUAL (GSI-08.02, row 122) — inputs whose native state VERA does not
/// yet represent are pinned at their constructor value:
/// - `aircraft_spawn_collision` = false (`AircraftClass+0x6CA`): the kamikaze
///   tracker's membership (`sim::kamikaze`), which Push sets only on a
///   `MissileSpawn=` child (`0x0054E47D`; any other child crashes). Dormant:
///   arms C/D return 0 first for a type without both weapons, as every retail
///   `MissileSpawn=` type is.
pub fn attacker_facts(entity: &GameEntity, obj: &ObjectType) -> AttackerFacts {
    let kind = TechnoKind::from_category(entity.category());
    let deploy_fire_active = match kind {
        // Infantry5218F3..52190B reads Doing27..30, as predicate522510. The
        // separate Unit deployment controller has no Infantry writer.
        TechnoKind::Infantry => entity.infantry_deploy_doing(),
        TechnoKind::Unit => entity.is_fully_deployed(),
        TechnoKind::Aircraft | TechnoKind::Building => false,
    };
    AttackerFacts {
        kind,
        veterancy: entity.veterancy.rank(),
        current_weapon_number: entity.current_weapon_number(),
        open_transport_weapon: open_transport_weapon(
            entity.passenger_role.in_open_transport(),
            obj,
        ),
        gattling_stage: entity.gattling.stage(),
        deploy_fire_active,
        is_occupied_building: is_occupied(entity, obj),
        // `TechnoClass+0x1CC DrainTarget`, the live drain link (GSI-09.01).
        drain_target_active: entity.drain.target().is_some(),
        // `MissionClass::GetCurrentMission @ 0x005B3040`: current, else queued.
        mission_is_unload: entity.mission.effective().known() == Some(MissionType::Unload),
        is_overpowered_building: entity.tesla_charge.is_overpowered(),
        aircraft_spawn_collision: false,
    }
}

/// Target facts for a Techno target. `terrain == None` (headless fixtures)
/// reads the occupied cell as clear land.
pub(crate) fn techno_target_facts<'a>(
    target: &'a GameEntity,
    target_obj: &'a ObjectType,
    terrain: Option<&'a ResolvedTerrainGrid>,
    is_ally: bool,
    rules: &'a RuleSet,
    interner: &'a StringInterner,
) -> TargetFacts<'a> {
    let cell = terrain.and_then(|grid| grid.cell(target.position.rx(), target.position.ry()));
    let cell_land_type = cell
        .map(|cell| cell.land_type)
        .unwrap_or(LandType::Clear.as_index());
    TargetFacts::Techno {
        obj: target_obj,
        kind: TechnoKind::from_category(target.category()),
        flight: TargetFlight::Live {
            entity: target,
            terrain,
            rules,
            interner,
        },
        on_bridge: target.on_bridge(),
        cell_land_type,
        submerged: target
            .cloak
            .as_ref()
            .is_some_and(|cloak| cloak.state() != 0),
        is_ally,
    }
}

/// Target facts for a force-fire cell.
pub(crate) fn cell_target_facts(
    rx: u16,
    ry: u16,
    terrain: Option<&ResolvedTerrainGrid>,
) -> TargetFacts<'static> {
    match terrain.and_then(|grid| grid.cell(rx, ry).map(|cell| (grid, cell))) {
        Some((grid, cell)) => TargetFacts::Cell {
            land_type: cell.land_type,
            tile_in_water_set: grid.tile_is_water_set(cell.final_tile_index),
            bridge_flag: cell.bridge_facts.has_structural_bridge(),
        },
        None => TargetFacts::Cell {
            land_type: LandType::Clear.as_index(),
            tile_in_water_set: false,
            bridge_flag: false,
        },
    }
}

/// SelectWeapon (vt+0x2E4) against a `TargetKind`, then GetWeapon of its
/// index ([`resolve_weapon_index`]): resolve the target, build both fact sets
/// and run the ladder. No legality is asked. The index is retained even when
/// GetWeapon has no weapon, so the caller can still ask GetFireError. `None`
/// means a supplied target is gone. A null target still enters the native
/// selection ladder.
pub(crate) fn select_weapon_against<'a>(
    rules: &'a RuleSet,
    attacker_entity: &GameEntity,
    attacker_obj: &'a ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetKind>,
    world: &Simulation,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<(i32, Option<SelectedWeapon<'a>>)> {
    let cells = terrain.map(NativeCellQuery::canonical);
    select_weapon_against_in_query(
        rules,
        attacker_entity,
        attacker_obj,
        attacker,
        target,
        world,
        interner,
        cells.as_ref(),
        alliances,
    )
}

/// The SelectWeapon owner with a caller-owned native query identity. Input
/// probes pass an isolated identity; simulation retains the canonical Dummy.
pub fn select_weapon_against_in_query<'a>(
    rules: &'a RuleSet,
    attacker_entity: &GameEntity,
    attacker_obj: &'a ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetKind>,
    world: &Simulation,
    interner: &StringInterner,
    cells: Option<&NativeCellQuery<'_>>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<(i32, Option<SelectedWeapon<'a>>)> {
    let index = select_weapon_index_against_in_query(
        rules,
        attacker_entity,
        attacker_obj,
        attacker,
        target,
        world,
        interner,
        cells,
        alliances,
    )?;
    Some((
        index,
        resolve_weapon_index(
            rules,
            attacker_entity,
            attacker_obj,
            index,
            &world.substrate.entities,
            interner,
        ),
    ))
}

/// The same SelectWeapon target adapter, before GetWeapon/warhead resolution.
/// VoiceAttack7090AD..7090C6 consumes the index and raw WeaponType damage;
/// unlike firing consumers it must not require a resolved Warhead. Both
/// consumers enter the single class/Techno selection ladder below.
pub(crate) fn select_weapon_index_against_in_query(
    rules: &RuleSet,
    attacker_entity: &GameEntity,
    attacker_obj: &ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetKind>,
    world: &Simulation,
    interner: &StringInterner,
    cells: Option<&NativeCellQuery<'_>>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<i32> {
    let terrain = cells.map(NativeCellQuery::terrain);
    let target_facts = match target.copied() {
        Some(TargetKind::Entity(target_id)) => {
            // SelectWeapon6F3330 distinguishes Techno bit0 from Terrain
            // WhatAmI24 before its shared ladder. The Object-ID arm retains
            // the actual Terrain identity rather than treating it as NULL.
            if let Some(target_entity) = world.substrate.entities.get(target_id) {
                let target_obj = interner.object_type(target_entity.type_ref(), rules)?;
                let is_ally = is_ally_by_object(
                    alliances,
                    interner,
                    attacker_entity.owner(),
                    target_entity.owner(),
                );
                let mut facts = techno_target_facts(
                    target_entity,
                    target_obj,
                    terrain,
                    is_ally,
                    rules,
                    interner,
                );
                if let Some(cells) = cells
                    && let TargetFacts::Techno { flight, .. } = &mut facts
                {
                    *flight = TargetFlight::InQuery {
                        entity: target_entity,
                        cells,
                        rules,
                        interner,
                    };
                }
                Some(facts)
            } else {
                world.terrain_objects.objects().get(&target_id)?;
                Some(TargetFacts::Terrain)
            }
        }
        Some(TargetKind::Cell(rx, ry)) => Some(cell_target_facts(rx, ry, terrain)),
        None => None,
    };
    Some(what_weapon_should_i_use(
        rules,
        attacker_obj,
        attacker,
        target_facts.as_ref(),
        |index| {
            get_weapon(
                attacker_entity,
                attacker_obj,
                index,
                &world.substrate.entities,
                rules,
                interner,
            )
        },
    ))
}

/// The occupied arm of `BuildingClass::GetWeapon @ 0x004526F0`, which answers
/// every index: the firing occupant's `EliteOccupyWeapon` (`+0xE20`) when it
/// is elite, else its `OccupyWeapon` (`+0xE04`); when that names no weapon,
/// the occupant's own GetWeapon(0) (vt+0x3F8, `0x0070E140`) at its rank. An
/// elite occupant without `EliteOccupyWeapon=` therefore fires its elite
/// primary, never the normal `OccupyWeapon`. No target is read; whether the
/// weapon may fire is GetFireError's.
fn occupant_weapon_record(occupant: &ObjectType, veterancy: u16) -> Option<WeaponRecord<'_>> {
    let occupy = if veterancy >= RANK_ELITE_U16 {
        occupant.elite_occupy_weapon.as_deref()
    } else {
        occupant.occupy_weapon.as_deref()
    };
    if let Some(weapon_id) = occupy {
        // InfantryType5236A0 clears E08/E0C/E10 and E24/E28/E2C;
        // ReadINI5240A0 writes only the E04/E20 WeaponType pointers.
        Some(WeaponRecord {
            weapon_id: Some(weapon_id),
            flh: crate::rules::flh::Flh::default(),
        })
    } else {
        weapon_for_index(occupant, veterancy, 0)
    }
}

/// `BuildingClass::IsOccupied @ 0x00458DD0` (vt+0x400; every other class
/// answers false through `0x0041BFB0`): a building whose type is
/// `CanBeOccupied=` (`+0x157B`) and `CanOccupyFire=` (`+0x157C`), holding an
/// occupant (vt+0x408, `+0x694`).
pub(crate) fn is_occupied(entity: &GameEntity, obj: &ObjectType) -> bool {
    entity.category() == EntityCategory::Structure
        && obj.can_be_occupied
        && obj.can_occupy_fire
        && entity.passenger_role.occupants().count() > 0
}

/// `BuildingClass @ 0x00458E00` (vt+0x404): half the shorter side of the
/// type's foundation (width `0x0045EC90`, height `0x0045ECA0(0)`), truncated.
pub(crate) fn half_foundation(obj: &ObjectType) -> i32 {
    let (width, height) = obj.foundation.dimensions();
    i32::from(width.min(height)) / 2
}

/// An occupied building's reach in cells: [`half_foundation`] plus
/// `[CombatDamage] OccupyWeaponRange=` (`Rules+0xF48`). InRange replaces the
/// range with it shifted to leptons (`0x006F7288..0x006F729F`), and
/// Greatest_Threat walks one ring beyond it (`0x006F9189..0x006F91A3`).
pub(crate) fn occupied_reach_cells(obj: &ObjectType, rules: &RuleSet) -> i32 {
    half_foundation(obj).wrapping_add(rules.garrison_rules.occupy_weapon_range)
}

/// The occupant an occupied building ([`is_occupied`]) fires through,
/// `Occupants[+0x69C]`, while that fire index is below the occupant count
/// (`+0x694`). Past it, none: `BuildingClass::GetWeapon @ 0x004526F0` then
/// answers the building's own slots, with no wrap.
pub(crate) fn firing_occupant_id(entity: &GameEntity, obj: &ObjectType) -> Option<u64> {
    if !is_occupied(entity, obj) {
        return None;
    }
    let occupants = entity.passenger_role.occupants();
    // Native452742 uses signed DWORD. Unsafe negative/stale slots are not
    // representable object references; do not narrow a valid index to u8.
    occupants
        .ids()
        .get(usize::try_from(occupants.fire_index()).ok()?)
        .copied()
}

/// GetWeapon's WeaponType projection. Record metadata is resolved by the
/// same owner that GetFLH consumes, [`get_weapon_record`].
pub fn get_weapon<'r>(
    entity: &GameEntity,
    obj: &'r ObjectType,
    index: i32,
    entities: &EntityStore,
    rules: &'r RuleSet,
    interner: &StringInterner,
) -> Option<&'r WeaponType> {
    rules.weapon(get_weapon_record(entity, obj, index, entities, rules, interner)?.weapon_id?)
}

/// `BuildingClass::GetWeapon4526F0`: first armed normal upgrade record in
/// the installed prefix, then the firing occupant's record, then the host's
/// own base/elite record (`Techno70E140`). GetFLH6F3B28 reads this same
/// record's FLH. The host still owns its pose, pixel offsets and garrison gates.
pub(crate) fn get_weapon_record<'r>(
    entity: &GameEntity,
    obj: &'r ObjectType,
    index: i32,
    entities: &EntityStore,
    rules: &'r RuleSet,
    interner: &StringInterner,
) -> Option<WeaponRecord<'r>> {
    if entity.category() == EntityCategory::Structure {
        let upgrades = entity.building_upgrades();
        for &kind in upgrades
            .types()
            .iter()
            .take(upgrades.count().max(0) as usize)
            .flatten()
        {
            let Some(upgrade) = interner.object_type_in_category(
                kind,
                crate::rules::object_type::ObjectCategory::Building,
                rules,
            ) else {
                continue;
            };
            // TypeGetWeapon7177C0 always selects the NORMAL upgrade record,
            // with the original caller index; host veterancy is irrelevant.
            if let Some(record) = weapon_for_index(upgrade, 0, index)
                && record.weapon_id.is_some()
            {
                return Some(record);
            }
        }
    }
    if let Some(id) = firing_occupant_id(entity, obj) {
        let occupant = entities.get(id)?;
        let kind = interner.object_type_in_category(
            occupant.type_ref(),
            crate::rules::object_type::ObjectCategory::Infantry,
            rules,
        )?;
        return occupant_weapon_record(kind, occupant.veterancy.rank());
    }
    weapon_for_index(obj, entity.veterancy.rank(), index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn installed_upgrade_records_select_per_index_and_follow_removal() {
        let ini = IniFile::from_str(
            "[BuildingTypes]\n0=BASE\n1=UP1\n2=UP2\n\
             [BASE]\nStrength=500\nUpgrades=3\nPrimary=HOST\nElitePrimary=ELITE\nLandTargeting=2\n\
             [UP1]\nStrength=100\nPowersUpBuilding=BASE\nPowersUpToLevel=2\nPrimary=FIRST\nElitePrimary=WRONG\n\
             [UP2]\nStrength=100\nPowersUpBuilding=BASE\nPrimary=SECOND\nSecondary=SECOND\n\
             [HOST]\nWarhead=WH\n[ELITE]\nWarhead=WH\n[FIRST]\nWarhead=WH\n\
             [WRONG]\nWarhead=WH\n[SECOND]\nWarhead=WH\n[WH]\n\
             [Structures]\n0=Americans,BASE,256,10,12,0,None,1,0,1,2,0,UP1,UP2,None,0,1\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let mut world = Simulation::new();
        assert_eq!(
            world.spawn_from_map(
                &crate::map::entities::parse_map_entities(&ini),
                Some(&rules)
            ),
            3
        );
        world
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .veterancy
            .set_elite();
        let obj = rules.object("BASE").unwrap();
        let host = world.substrate.entities.get(1).unwrap();
        assert_eq!(host.building_upgrades().count(), 3);
        assert_eq!(host.building_upgrades().types()[0], None);
        assert_eq!(
            get_weapon(
                host,
                obj,
                0,
                &world.substrate.entities,
                &rules,
                &world.interner
            )
            .unwrap()
            .id,
            "FIRST"
        );
        assert_eq!(
            get_weapon(
                host,
                obj,
                1,
                &world.substrate.entities,
                &rules,
                &world.interner
            )
            .unwrap()
            .id,
            "SECOND"
        );
        let selected = resolve_selected_weapon(
            &rules,
            host,
            obj,
            &attacker_facts(host, obj),
            Some(&TargetFacts::Cell {
                land_type: 0,
                tile_in_water_set: false,
                bridge_flag: false,
            }),
            &world.substrate.entities,
            &world.interner,
        )
        .unwrap();
        assert_eq!((selected.index, selected.weapon_id), (1, "SECOND"));
        assert_eq!(
            world.power_down_building_upgrade(1, &rules),
            world.interner.get("UP2")
        );
        let host = world.substrate.entities.get(1).unwrap();
        assert!(
            get_weapon(
                host,
                obj,
                1,
                &world.substrate.entities,
                &rules,
                &world.interner
            )
            .is_none()
        );
        assert_eq!(
            world.power_down_building_upgrade(1, &rules),
            world.interner.get("UP1")
        );
        let host = world.substrate.entities.get(1).unwrap();
        assert_eq!(
            current_weapon(
                host,
                obj,
                &world.substrate.entities,
                &rules,
                &world.interner
            )
            .unwrap()
            .id,
            "ELITE"
        );
    }

    #[test]
    fn occupied_record_keeps_dedicated_and_primary_fallback_offsets_distinct() {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[BuildingTypes]\n0=HOUSE\n[InfantryTypes]\n0=GI\n\
             [HOUSE]\nCanBeOccupied=yes\nCanOccupyFire=yes\n\
             [GI]\nPrimary=PRIMARY\nElitePrimary=ELITE\nOccupyWeapon=OCCUPY\n",
            ),
            &IniFile::from_str(
                "[GI]\nPrimaryFireFLH=1,2,3\nElitePrimaryFireFLH=4,5,6\n\
             [HOUSE]\nSecondaryFireFLH=7,8,9\n",
            ),
        )
        .unwrap();
        let mut world = Simulation::new();
        let mut host = GameEntity::test_default_of_category(
            1,
            "HOUSE",
            "Americans",
            5,
            5,
            EntityCategory::Structure,
        );
        host.passenger_role.occupants_mut().add(2);
        world.substrate.entities.insert(host);
        world
            .substrate
            .entities
            .insert(GameEntity::test_default_of_category(
                2,
                "GI",
                "Americans",
                5,
                5,
                EntityCategory::Infantry,
            ));
        world.interner = crate::sim::intern::test_interner();
        let obj = rules.object("HOUSE").unwrap();
        let read = |world: &Simulation| {
            get_weapon_record(
                world.substrate.entities.get(1).unwrap(),
                obj,
                1,
                &world.substrate.entities,
                &rules,
                &world.interner,
            )
            .unwrap()
        };
        assert_eq!(
            (read(&world).weapon_id, read(&world).flh.height),
            (Some("OCCUPY"), 0)
        );
        world
            .substrate
            .entities
            .get_mut(2)
            .unwrap()
            .veterancy
            .set_elite();
        assert_eq!(
            (read(&world).weapon_id, read(&world).flh.height),
            (Some("ELITE"), 6)
        );
        // No WeaponType in a valid own slot does not erase its retained FLH.
        let empty = weapon_for_index(obj, 0, 1).unwrap();
        assert_eq!((empty.weapon_id, empty.flh.height), (None, 9));
    }

    #[test]
    fn passive_scan_occupied_building_uses_weapon_identity_without_target_fallback() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=HOUSE\n[InfantryTypes]\n0=GI\n1=ELITEGI\n\
             [HOUSE]\nPrimary=AIR\nCanBeOccupied=yes\nCanOccupyFire=yes\n\
             [GI]\nPrimary=AIR\nElitePrimary=AIR\nOccupyWeapon=GROUND\n\
             [ELITEGI]\nPrimary=GROUND\nOccupyWeapon=GROUND\nEliteOccupyWeapon=AIR\n\
             [WeaponTypes]\n0=GROUND\n1=AIR\n[GROUND]\nProjectile=AG\n[AIR]\nProjectile=AA\n\
             [AG]\nAG=yes\nAA=no\n[AA]\nAG=yes\nAA=yes\n",
        ))
        .unwrap();
        let obj = rules.object("HOUSE").unwrap();
        // The building, and the occupant `occupant` at `veterancy` when given.
        let bits = |occupant: Option<(&str, u16)>| {
            let mut entities = EntityStore::new();
            let mut building = GameEntity::test_default_of_category(
                1,
                "HOUSE",
                "Americans",
                5,
                5,
                EntityCategory::Structure,
            );
            if let Some((kind, veterancy)) = occupant {
                let mut inside = GameEntity::test_default_of_category(
                    2,
                    kind,
                    "Americans",
                    5,
                    5,
                    EntityCategory::Infantry,
                );
                inside.veterancy.set_rank_for_test(veterancy);
                entities.insert(inside);
                building.passenger_role.occupants_mut().add(2);
            }
            entities.insert(building);
            let interner = crate::sim::intern::test_interner();
            let building = entities.get(1).unwrap();
            passive_scan_class_bits(&rules, obj, attacker_facts(building, obj), |index| {
                get_weapon(building, obj, index, &entities, &rules, &interner)
            })
        };
        assert_eq!(bits(None), 0xBC);
        // Normal OccupyWeapon=GROUND overrides AA primary even though an AA
        // target would be refused. Elite missing EliteOccupyWeapon uses its
        // resolved elite primary, not the normal OccupyWeapon.
        for (occupant, veterancy, expected) in [
            ("GI", 0, false),
            ("GI", 200, true),
            ("ELITEGI", 0, false),
            ("ELITEGI", 200, true),
        ] {
            assert_eq!(
                bits(Some((occupant, veterancy))) & 4 != 0,
                expected,
                "{occupant} at {veterancy}"
            );
        }
    }

    #[test]
    fn slot_zero_warhead_flag_reads_the_exact_medium_and_wood_verses_at_the_live_tier() {
        for medium in ["0%", "0.1%", "0.001"] {
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[InfantryTypes]\n0=DOG\n[DOG]\nPrimary=BITE\nSecondary=SCANNER\nElitePrimary=ELITEBITE\n\
                 [WeaponTypes]\n0=BITE\n1=SCANNER\n2=ELITEBITE\n\
                 [BITE]\nProjectile=AG\nWarhead=SOFT\n[ELITEBITE]\nProjectile=AG\nWarhead=FULL\n\
                 [SCANNER]\nProjectile=AA\nWarhead=FULL\n[AG]\nAA=no\nAG=yes\n[AA]\nAA=yes\nAG=yes\n\
                 [SOFT]\nVerses=100%,100%,100%,100%,{medium},100%,0%,100%,100%,100%,100%\n\
                 [FULL]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
            ))).unwrap();
            let obj = rules.object("DOG").unwrap();
            let mut dog = GameEntity::test_default(1, "DOG", "Americans", 5, 5);
            dog.set_category_for_test(EntityCategory::Infantry);
            let entities = EntityStore::new();
            let interner = crate::sim::intern::test_interner();
            assert_eq!(
                passive_scan_class_bits(&rules, obj, attacker_facts(&dog, obj), |index| {
                    get_weapon(&dog, obj, index, &entities, &rules, &interner)
                }),
                0xBC,
                "slot 0 AG and slot 1 AA+AG"
            );
            assert_eq!(
                primary_warhead_spares_medium_and_wood(&rules, obj, 0),
                !crate::rules::ARES_ENABLED && medium != "0.001"
            );
            assert!(
                !primary_warhead_spares_medium_and_wood(&rules, obj, 200),
                "elite primary is resolved first"
            );
        }
    }

    fn facts(kind: TechnoKind) -> AttackerFacts {
        AttackerFacts {
            kind,
            veterancy: 0,
            current_weapon_number: 0,
            open_transport_weapon: None,
            gattling_stage: 0,
            deploy_fire_active: false,
            is_occupied_building: false,
            drain_target_active: false,
            mission_is_unload: false,
            is_overpowered_building: false,
            aircraft_spawn_collision: false,
        }
    }

    fn techno<'a>(obj: &'a ObjectType, kind: TechnoKind) -> TargetFacts<'a> {
        TargetFacts::Techno {
            obj,
            kind,
            flight: TargetFlight::Supplied { high: false },
            on_bridge: false,
            cell_land_type: LandType::Clear.as_index(),
            submerged: false,
            is_ally: false,
        }
    }

    fn high_flying<'a>(target: TargetFacts<'a>) -> TargetFacts<'a> {
        match target {
            TargetFacts::Techno {
                obj,
                kind,
                on_bridge,
                cell_land_type,
                submerged,
                is_ally,
                ..
            } => TargetFacts::Techno {
                obj,
                kind,
                flight: TargetFlight::Supplied { high: true },
                on_bridge,
                cell_land_type,
                submerged,
                is_ally,
            },
            cell => cell,
        }
    }

    // ---- GetWeapon ---------------------------------------------------------

    // ---- Naval selector ----------------------------------------------------

    // ---- Ladder arms -------------------------------------------------------

    /// Every row of the original `What_Weapon_Should_I_Use`
    /// (`tools/spatial_oracle/gattling_select.py`: `0x006F3330` through the
    /// Unit `0x00746CD0` and Building vtables), through
    /// `what_weapon_should_i_use`: the gattling stage pair over stage x slot-1
    /// AA x high flier x elite x NeverUse, the arms ahead of it, the target
    /// kinds and the unclamped stages (`2s` wraps).
    #[test]
    fn original_weapon_selection_rows() {
        let payload: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/gattling_select.json",
        ))
        .unwrap();
        let defaults = &payload["defaults"];
        let rows = payload["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 134);
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let group = |key: &str, field: &str| -> serde_json::Value {
                input
                    .get(key)
                    .and_then(|group| group.get(field))
                    .unwrap_or(&defaults[key][field])
                    .clone()
            };
            let scalar = |key: &str| input.get(key).unwrap_or(&defaults[key]).clone();
            let flag = |value: serde_json::Value| {
                value
                    .as_bool()
                    .unwrap_or_else(|| value.as_i64().unwrap() != 0)
            };
            let int = |value: serde_json::Value| value.as_i64().unwrap();
            let building = scalar("class").as_str().unwrap() == "building";
            let turret_count = int(group("type", "turret_count"));
            // Slot 1's AA sits on its projectile, NeverUse on the weapon;
            // `present: false` leaves the slot empty.
            let weapon = |name: &str, key: &str| -> Option<String> {
                flag(group(key, "present")).then(|| {
                    format!(
                        "[{name}]\nDamage=10\nWarhead=WH\nProjectile={}\nNeverUse={}\n",
                        if key.ends_with('1') && flag(group(key, "aa")) {
                            "AAP"
                        } else {
                            "AGP"
                        },
                        if key.ends_with('1') && flag(group(key, "never_use")) {
                            "yes"
                        } else {
                            "no"
                        },
                    )
                })
            };
            let slots = [
                ("W0", "slot0"),
                ("W1", "slot1"),
                ("E0", "elite_slot0"),
                ("E1", "elite_slot1"),
            ]
            .map(|(name, key)| weapon(name, key));
            let mut section = format!(
                "[GAT]\nStrength=100\nArmor=none\nIsGattling={}\nTurretCount={turret_count}\n\
                 WeaponCount=2\nDeployFire={}\nDeployFireWeapon={}\nOpenTransportWeapon={}\n\
                 CanBeOccupied={}\nCanOccupyFire={}\n",
                if flag(group("type", "is_gattling")) {
                    "yes"
                } else {
                    "no"
                },
                if flag(group("type", "deploy_fire")) {
                    "yes"
                } else {
                    "no"
                },
                int(group("type", "deploy_fire_weapon")),
                int(group("type", "open_transport_weapon")),
                if flag(group("type", "can_be_occupied")) {
                    "yes"
                } else {
                    "no"
                },
                if flag(group("type", "can_occupy_fire")) {
                    "yes"
                } else {
                    "no"
                },
            );
            let keys = if turret_count > 0 {
                ["Weapon1", "Weapon2", "EliteWeapon1", "EliteWeapon2"]
            } else {
                ["Primary", "Secondary", "ElitePrimary", "EliteSecondary"]
            };
            for (key, (weapon, slot)) in keys
                .iter()
                .zip(["W0", "W1", "E0", "E1"].iter().zip(slots.iter()))
            {
                if slot.is_some() {
                    section.push_str(&format!("{key}={weapon}\n"));
                }
            }
            let text = format!(
                "[{}]\n0=GAT\n1=TGT\n[AircraftTypes]\n0=AIR\n\
                 [InfantryTypes]\n[{}]\n0=TBLD\n{section}\
                 [TGT]\nStrength=100\nArmor=none\n[AIR]\nStrength=100\nArmor=none\n\
                 [TBLD]\nStrength=100\nArmor=none\n\
                 [AAP]\nAA=yes\nAG=yes\n[AGP]\nAA=no\nAG=yes\n\
                 [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{}",
                if building {
                    "BuildingTypes"
                } else {
                    "VehicleTypes"
                },
                if building {
                    "VehicleTypes"
                } else {
                    "BuildingTypes"
                },
                slots.iter().flatten().cloned().collect::<String>(),
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
            let gat = rules.object("GAT").unwrap();
            let mut attacker = facts(if building {
                TechnoKind::Building
            } else {
                TechnoKind::Unit
            });
            let veterancy = scalar("veterancy").as_f64().unwrap();
            attacker.veterancy = if veterancy >= 2.0 {
                200
            } else if veterancy >= 1.0 {
                100
            } else {
                0
            };
            attacker.current_weapon_number = int(scalar("current_weapon")) as i32;
            attacker.gattling_stage = int(scalar("stage")) as i32;
            attacker.deploy_fire_active = !building && flag(scalar("deployed"));
            attacker.is_occupied_building = building
                && int(scalar("occupants")) > 0
                && gat.can_be_occupied
                && gat.can_occupy_fire;
            let open_transport_weapon = int(group("type", "open_transport_weapon"));
            attacker.open_transport_weapon = (flag(scalar("open_topped"))
                && open_transport_weapon != -1)
                .then_some(open_transport_weapon as i32);
            let high_flying = group("target", "high_flying").as_bool().unwrap_or(false);
            let (tgt, air, tbld) = (
                rules.object("TGT").unwrap(),
                rules.object("AIR").unwrap(),
                rules.object("TBLD").unwrap(),
            );
            let target = match group("target", "kind").as_str().unwrap() {
                "none" => None,
                "cell" => Some(TargetFacts::Cell {
                    land_type: 0,
                    tile_in_water_set: false,
                    bridge_flag: false,
                }),
                "terrain" => Some(TargetFacts::Terrain),
                kind => {
                    let (obj, kind) = match kind {
                        "aircraft" => (air, TechnoKind::Aircraft),
                        "unit" => (tgt, TechnoKind::Unit),
                        "building" => (tbld, TechnoKind::Building),
                        other => panic!("{name}: target kind {other}"),
                    };
                    let target = techno(obj, kind);
                    Some(if high_flying {
                        self::high_flying(target)
                    } else {
                        target
                    })
                }
            };
            assert_eq!(
                i64::from(what_weapon_should_i_use(
                    &rules,
                    gat,
                    &attacker,
                    target.as_ref(),
                    |index| rules
                        .weapon(weapon_for_index(gat, attacker.veterancy, index)?.weapon_id?),
                )),
                row["index"].as_i64().unwrap(),
                "{name}"
            );
        }
    }

    // ---- Class overrides ---------------------------------------------------

    // ---- GetWeapon of the selected slot ------------------------------------

    // ---- Facts builders ----------------------------------------------------

    // ---- Garrison ----------------------------------------------------------
}
