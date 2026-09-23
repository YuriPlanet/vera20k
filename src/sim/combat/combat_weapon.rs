//! Weapon selection — the native `TechnoClass::What_Weapon_Should_I_Use`
//! ladder, its Infantry/Unit class overrides, the `NavalTargeting=` selector,
//! `TechnoClass::GetWeapon` (elite tier), and the target-legality subset of
//! `TechnoClass::GetFireError` that turns a selected slot into "can engage /
//! cannot engage".
//!
//! Shape: `what_weapon_should_i_use` reproduces the native predicate ladder in
//! native order as one function with the same early returns and returns a
//! weapon-slot INDEX (never "no weapon"). `select_weapon_for_target` then
//! resolves that index through `GetWeapon` and applies the GetFireError
//! targeting verdicts (AA vs high-flying, naval `-1`, `LandTargeting=1`,
//! `Verses == 0`) so callers keep the existing `Option<SelectedWeapon>`
//! contract: `None` means the shot is ILLEGAL, not that no slot was chosen.
//!
//! gamemd-derived (all read live this session):
//! - `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330` (vtable `+0x2E4` in
//!   the Techno/Foot/Building/Aircraft tables; `0x007F4C44` read back).
//! - `InfantryClass` override `@ 0x005218E0` (`0x007EB33C`), `UnitClass`
//!   override `@ 0x00746CD0` (`0x007F5F54`).
//! - `TechnoClass::SelectNavalTargetingWeapon @ 0x006F3820` (vtable `+0x2E8`).
//! - `TechnoClass::GetWeapon @ 0x0070E140` (elite tier).
//! - `TechnoClass::GetFireError @ 0x006FC0B0` (targeting subset only).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ terrain facts, and sim entity state.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use super::TargetKind;
use super::armor_index;
use super::combat_targeting::AttackerSnapshot;
use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::object_type::{ObjectType, WEAPON_SLOT_COUNT};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::warhead_type::WarheadType;
use crate::rules::weapon_type::WeaponType;
use crate::sim::animation::SequenceKind;
use crate::sim::deploy::DeployPhase;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::MissionType;
use crate::util::lepton::HIGH_FLIGHT_THRESHOLD_LEPTONS;

/// Which weapon slot the unit is using for this engagement.
///
/// Used to resolve the correct FLH (firing offset) from art.ini:
/// Primary → `PrimaryFireFLH`, Secondary → `SecondaryFireFLH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WeaponSlot {
    Primary,
    Secondary,
}

/// Weapon-selection override carried by a transport firing on a passenger's
/// behalf. Both variants feed native inputs of the selection ladder:
///
/// - **`IfvSlot(idx)`** — `Gunner=yes` transports (IFV). The passenger's
///   `IFVMode` becomes the transport's `CurrentWeaponNumber`
///   (`TechnoClass+0x138`, written by `TechnoClass::SetGunnerWeapon @
///   0x0070DC70` from the receive-gunner path at `0x007464CE`).
/// - **`OpenTransport(slot)`** — VERA-internal bridge for open-topped
///   passenger fire: the transport entity stands in for the passenger whose
///   `InOpenToppedTransport` (`TechnoClass+0x82`) and `OpenTransportWeapon`
///   would natively be read on the passenger itself (ladder arm G @
///   `0x006F33D9`). The slot value is the passenger's `OpenTransportWeapon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WeaponOverride {
    /// Transport's `CurrentWeaponNumber`, used when transport is `Gunner=yes`.
    IfvSlot(u32),
    /// Passenger's `OpenTransportWeapon` slot (0 primary / 1 secondary).
    OpenTransport(u32),
}

/// Result of weapon selection: the chosen weapon, its warhead, and the
/// effective Verses percentage against the target's armor.
pub(crate) struct SelectedWeapon<'a> {
    /// Section id of the selected weapon.
    pub weapon_id: &'a str,
    pub weapon: &'a WeaponType,
    pub warhead: &'a WarheadType,
    /// Damage percentage for target armor (0–200). Already looked up from Verses.
    /// 100 = full damage, 0 = immune. Cell targets report 100 (native reads
    /// no Verses for a non-Techno target).
    pub verses_pct: u8,
    /// FLH slot: index 1 is `Secondary`, every other index is `Primary`.
    pub slot: WeaponSlot,
    /// Native weapon-array index returned by the selection ladder.
    pub index: i32,
}

/// Behavioral gate derived from the Verses damage percentage.
/// Controls whether a weapon can passively acquire or retaliate against a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersesGate {
    /// 0% — weapon cannot target this armor type at all, even force-fire.
    Blocked,
    /// 1% — no passive acquire, no retaliation. Force-fire works at 1% damage.
    Suppressed,
    /// >1% — normal engagement allowed.
    Normal,
}

/// Classify a Verses percentage into its behavioral gate.
///
/// RA2 uses these thresholds to control targeting:
/// - 0 blocks the weapon entirely (falls back to Secondary).
/// - 1 (1%) suppresses auto-targeting but allows force-fire.
/// - >1 is normal combat.
pub(crate) fn verses_gate(verses_pct: u8) -> VersesGate {
    match verses_pct {
        0 => VersesGate::Blocked,
        1 => VersesGate::Suppressed,
        _ => VersesGate::Normal,
    }
}

/// `DeployFireWeapon=` constructor default (`TechnoTypeClass` ctor @
/// `0x0071113A`): slot 1.
const DEFAULT_DEPLOY_FIRE_WEAPON_INDEX: i32 = 1;

/// Elite veterancy threshold used by `VeterancyClass::IsElite` inside
/// `GetWeapon`.
const ELITE_VETERANCY: u16 = 200;

/// Native `AbstractClass::WhatAmI` families the ladder distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TechnoKind {
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
pub(crate) struct AttackerFacts {
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
    /// The firer's CaptureManager half of CanCapture, which GetFireError asks
    /// for a MindControl warhead (`0x006FCB24..0x006FCB50`).
    pub capture: Option<crate::sim::capture_manager::CaptureControllerFacts>,
}

/// Target-side facts read by the ladder and the GetFireError subset.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TargetFacts<'a> {
    /// Real TerrainClass Object target: neither CellClass nor TechnoClass.
    /// Used by Unit73FBAC ->746CD0/6F3330 for repair's Wood-warhead query.
    Terrain,
    /// `CellClass` target (force-fire on terrain).
    Cell {
        /// `CellClass+0xEC LandType`.
        land_type: u8,
        /// `CellClass+0x38` tile index inside the theater water tile set
        /// (`[0x00AA0738] .. +14`; body at `0x004867E0`, CellClass vtable
        /// `+0x50`).
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
        is_high_flying: bool,
        /// `FootClass+0x8C OnBridge`.
        on_bridge: bool,
        /// `LandType` of the target's occupied cell (`vt+0x1BC` → `+0xEC`).
        cell_land_type: u8,
        /// `TechnoClass+0x220 CloakState != 0` — a submerged/cloaking sub.
        submerged: bool,
        /// `HouseClass::Is_Ally_ByObject @ 0x004F9A90` of the attacker's
        /// owner against this target.
        is_ally: bool,
        /// CanInfect `0x0062A8E0`'s victim-side gates, which GetFireError
        /// `0x006FCA81..0x006FCAC2` applies to a Parasite warhead.
        parasite: ParasiteVictimFacts,
        /// CanCapture `0x00471C90`'s victim-side gates, which GetFireError
        /// applies to a MindControl warhead (the Iron Curtain gate reads the
        /// frame and is applied at fire admission).
        capture: crate::sim::capture_manager::CaptureVictimFacts,
        /// `ObjectClass+0x38`: it carries a Crazy Ivan bomb.
        has_bomb: bool,
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
    /// An ordinary infectable land victim; fixtures that do not model parasites.
    #[cfg(test)]
    pub(crate) const INFECTABLE_ON_LAND: Self = Self {
        infectable: true,
        on_water_set: false,
    };

    /// A NULL victim cell passes the water gate (`0x00485060` is not reached).
    pub(crate) fn of(
        target: &GameEntity,
        target_obj: &ObjectType,
        terrain: Option<&ResolvedTerrainGrid>,
    ) -> Self {
        let on_water_set = terrain
            .and_then(|grid| grid.cell(target.position.rx, target.position.ry))
            .is_none_or(|cell| cell.is_water);
        Self {
            infectable: matches!(
                target.category,
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            ) && !target.lifecycle.in_limbo
                && target.lifecycle.object_alive
                && target.health.current != 0
                && target.parasite_eating_me.is_none()
                && target_obj.parasiteable
                && target.bunker_link.installed_in().is_none(),
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
            TargetFacts::Techno { is_high_flying, .. } => *is_high_flying,
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

/// `Weapon[index]` (`TechnoTypeClass+0x898 + index*0x1C`).
///
/// `ObjectType::weapon_list` **is** that native array, and `obj.primary` /
/// `obj.secondary` are slots 0 and 1 of it — `TechnoTypeClass::ReadINI
/// @ 0x007128B2` writes `Weapon1=` and `Primary=` to the same `+0x898`, so the
/// `TurretCount` branch is decided once, at parse time, in
/// `rules::object_type::ObjectType::read_weapon_arrays`. Nothing downstream has
/// to know which INI keys a type authored.
fn base_weapon_at(obj: &ObjectType, index: usize) -> Option<&str> {
    obj.weapon_list.get(index).and_then(|slot| slot.as_deref())
}

/// `EliteWeapon[index]` (`TechnoTypeClass+0xA94 + index*0x1C`).
fn elite_weapon_at(obj: &ObjectType, index: usize) -> Option<&str> {
    obj.elite_weapon_list
        .get(index)
        .and_then(|slot| slot.as_deref())
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
) -> Option<(&str, WeaponSlot)> {
    if index < 0 {
        return None;
    }
    let slot_index = usize::try_from(index).ok()?;
    if slot_index >= WEAPON_SLOT_COUNT {
        return None;
    }
    let weapon_id = if veterancy >= ELITE_VETERANCY {
        elite_weapon_at(obj, slot_index).or_else(|| base_weapon_at(obj, slot_index))
    } else {
        base_weapon_at(obj, slot_index)
    }?;
    let slot = if slot_index == 1 {
        WeaponSlot::Secondary
    } else {
        WeaponSlot::Primary
    };
    Some((weapon_id, slot))
}

/// Alias kept for the fatal-receiver Suicide gate, which stores the last
/// selected index as a `u8`.
pub(crate) fn weapon_for_slot_index(
    obj: &ObjectType,
    veterancy: u16,
    index: i32,
) -> Option<(&str, WeaponSlot)> {
    weapon_for_index(obj, veterancy, index)
}

/// `GetWeapon(0)` weapon id at the given veterancy.
pub(crate) fn primary_for_tier(obj: &ObjectType, veterancy: u16) -> Option<&str> {
    weapon_for_index(obj, veterancy, 0).map(|(weapon_id, _)| weapon_id)
}

/// `GetWeapon(1)` weapon id at the given veterancy.
pub(crate) fn secondary_for_tier(obj: &ObjectType, veterancy: u16) -> Option<&str> {
    weapon_for_index(obj, veterancy, 1).map(|(weapon_id, _)| weapon_id)
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
/// So the `Secondary` slot never makes an object armed, and a `TurretCount>0`
/// type is armed only through its *current gunner* slot — which is why this is
/// still a distinct predicate from reading `obj.primary`, even now that
/// `obj.primary` is the native `Primary` field for every type (see
/// `ObjectType::read_weapon_arrays`). The elite tier is applied by `GetWeapon`,
/// so the read goes through `weapon_for_index`.
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
/// RESIDUAL (UNCHECKED, zero stock frequency): `BuildingClass::GetWeapon
/// @ 0x004526F0` resolves a non-occupied building's slot through an upgrade
/// loop (`count [+0x702]` over `[+0x5EC]`) that VERA does not model. No stock
/// section pairs a weaponised upgrade with a building; only `[GAPOWR]`/
/// `[YAPOWR]` declare `Upgrades=`, and those are capacity upgrades. Native also
/// tests the resolved `WeaponType` pointer where VERA tests whether the slot
/// names an id, so a type naming a weapon absent from `[WeaponTypes]` is armed
/// here and unarmed in gamemd — a data-error-only divergence.
pub(crate) fn is_armed(entity: &GameEntity, obj: &ObjectType) -> bool {
    is_armed_from_facts(obj, attacker_facts(entity, obj))
}

/// The `Wall=` and `Wood=` flags of the mover's **slot 0** warhead.
///
/// gamemd-derived: the weapon route of the wall arm in
/// `UnitClass::Can_Enter_Cell @ 0x0073F0A0` resolves its warhead by pushing a
/// literal `0` into vtable `+0x3F8` (`TechnoClass::GetWeapon @ 0x0070E140`) at
/// `0x0073F497`, then reads `+0xAC` for the warhead and tests `+0x144` (`Wall=`)
/// at `0x0073F4A9` and `+0x147` (`Wood=`) at `0x0073F4B3`.
///
/// Slot 0 is **unconditional** there. [`is_armed`] resolves the turret-aware
/// current weapon instead (`GetCurrentWeapon @ 0x0070E1A0`: the gunner slot iff
/// `HasTurrets`), so the two gates of the same arm deliberately read different
/// slots and this cannot reuse that choice.
///
/// `Wood=` additionally requires the overlay's own `Armor` to be wood
/// (`0x0073F4BD` compares 6) and is Unit-only - the infantry arm's
/// `FUN_00772AC0` tests `+0x144` alone. Both of those gates live in the wall arm
/// itself; this returns the raw warhead flags.
pub(crate) fn primary_warhead_wall_flags(
    entity: &GameEntity,
    obj: &ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
) -> (bool, bool) {
    let facts = attacker_facts(entity, obj);
    let Some((weapon_id, _)) = weapon_for_index(obj, facts.veterancy, 0) else {
        return (false, false);
    };
    let Some(warhead_id) = rules.weapon(weapon_id).and_then(|w| w.warhead.as_deref()) else {
        return (false, false);
    };
    rules
        .warhead(warhead_id)
        .map_or((false, false), |wh| (wh.wall, wh.wood))
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

/// Techno GetRange7012C0, used by Aircraft FindFireLocation4197C0. This is
/// raw signed weapon range, capped by the shortest armed cargo weapon when
/// OpenTopped. It deliberately excludes InRange's bonuses and range overrides.
pub(crate) fn weapon_range(
    entity: &GameEntity,
    obj: &ObjectType,
    index: i32,
    entities: &crate::sim::entity_store::EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
) -> i32 {
    let Some(weapon) =
        weapon_for_index(obj, entity.veterancy, index).and_then(|(name, _)| rules.weapon(name))
    else {
        return 0;
    };
    let mut range = weapon.range_leptons;
    if obj.open_topped
        && let Some(cargo) = entity.passenger_role.cargo()
    {
        // Cargo owns the native head/+30 sequence. Neither limbo nor death
        // filters belong to this reader; boarded passengers are in limbo.
        for &id in &cargo.passengers {
            let Some(passenger) = entities.get(id) else {
                break;
            };
            let Some(kind) = rules.object(interner.resolve(passenger.type_ref())) else {
                continue;
            };
            let index = current_weapon_index(kind, attacker_facts(passenger, kind));
            if let Some(weapon) = weapon_for_index(kind, passenger.veterancy, index)
                .and_then(|(name, _)| rules.weapon(name))
            {
                range = range.min(weapon.range_leptons);
            }
        }
    }
    range
}

fn current_weapon_index(obj: &ObjectType, facts: AttackerFacts) -> i32 {
    // GetCurrentWeapon70E1A0 reads CurrentWeaponNumber only with HasTurrets.
    if obj.turret_count > 0 {
        facts.current_weapon_number
    } else {
        0
    }
}

fn is_armed_from_facts(obj: &ObjectType, facts: AttackerFacts) -> bool {
    // `BuildingClass::Is_Armed 0x00458DB0`: `IsOccupied() → 1`.
    if facts.is_occupied_building {
        return true;
    }
    // `GetCurrentWeapon 0x0070E1A0`: gunner slot iff `HasTurrets`, else slot 0.
    let index = current_weapon_index(obj, facts);
    weapon_for_index(obj, facts.veterancy, index).is_some()
}

fn deploy_fire_weapon_index(obj: &ObjectType) -> i32 {
    obj.deploy_fire_weapon
        .unwrap_or(DEFAULT_DEPLOY_FIRE_WEAPON_INDEX)
}

/// Weapon ID the unit will fire while deployed (`DeployFireWeapon=` slot,
/// default Secondary), without target-compatibility checks. Used by the
/// deployed self-irradiator gate, which needs the weapon's RadLevel before
/// any target exists.
pub(crate) fn deploy_fire_weapon_id(obj: &ObjectType, veterancy: u16) -> Option<&str> {
    weapon_for_index(obj, veterancy, deploy_fire_weapon_index(obj)).map(|(weapon_id, _)| weapon_id)
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

/// Ordinary passive-scan class-mask bit4: Weapon772A90 projectile AA.
/// Unit7431C9..24D uses only CurrentWeaponNumber when TurretCount>0 and
/// !IsGattling, otherwise slots0/1; Building445F00 and Infantry51E2C7..319
/// also union slots0/1. GetWeapon70E140 supplies elite fallback. This does
/// not port the Infantry wrapper's separate special mission/type masks.
/// Building4526F0's installed-upgrade substitution remains unrepresented:
/// the audited184 stock maps have no authored upgrade selectors, and the
/// existing installer is authored-only. This is not generic upgrade parity.
pub(crate) fn passive_scan_has_aa(
    rules: &RuleSet,
    obj: &ObjectType,
    attacker: AttackerFacts,
    garrison: Option<(&str, u16)>,
) -> bool {
    if attacker.kind == TechnoKind::Building
        && let Some((occupant_type, veterancy)) = garrison
    {
        // Building4526F0's occupied override returns this same identity for
        // either index. It does not test target legality or fall back after
        // refusing a target. Use the actual firing-occupant snapshot.
        return rules
            .object(occupant_type)
            .and_then(|occupant| {
                let occupy = if veterancy >= ELITE_VETERANCY {
                    occupant.elite_occupy_weapon.as_deref()
                } else {
                    occupant.occupy_weapon.as_deref()
                };
                occupy.or_else(|| primary_for_tier(occupant, veterancy))
            })
            .and_then(|id| rules.weapon(id))
            .is_some_and(|weapon| projectile_aa(rules, weapon));
    }
    let slot_has_aa = |index| {
        weapon_for_index(obj, attacker.veterancy, index)
            .and_then(|(id, _)| rules.weapon(id))
            .is_some_and(|weapon| projectile_aa(rules, weapon))
    };
    let has_aa = if attacker.kind == TechnoKind::Unit && obj.turret_count > 0 && !obj.is_gattling {
        slot_has_aa(attacker.current_weapon_number)
    } else {
        slot_has_aa(0) || slot_has_aa(1)
    };
    // Infantry51E31B..347: IsArmed first, then primary GetWeapon(0)'s
    // Warhead+149 removes AA4. ReadINI75DE5A..9A derives that byte from
    // exact double Verses[4] and[6] ==0 (finite retail values). DOG's AA
    // VirtualScanner secondary therefore does not grant an air prepass.
    if has_aa && attacker.kind == TechnoKind::Infantry && is_armed_from_facts(obj, attacker) {
        let primary_warhead = primary_for_tier(obj, attacker.veterancy)
            .and_then(|id| rules.weapon(id))
            .and_then(|weapon| warhead_of(rules, weapon));
        if verses_is_zero(primary_warhead, 4) && verses_is_zero(primary_warhead, 6) {
            return false;
        }
    }
    has_aa
}

fn projectile_ag(rules: &RuleSet, weapon: &WeaponType) -> bool {
    weapon
        .projectile
        .as_ref()
        .and_then(|id| rules.projectile(id))
        .is_none_or(|projectile| projectile.ag)
}

pub(crate) fn warhead_of<'a>(rules: &'a RuleSet, weapon: &WeaponType) -> Option<&'a WarheadType> {
    weapon.warhead.as_ref().and_then(|id| rules.warhead(id))
}

/// Native `FCOMP Verses[armor], 0.0` (`0x006F3705` / `0x006F3738`): the
/// double is compared for exact equality with zero.
fn verses_is_zero(warhead: Option<&WarheadType>, armor: usize) -> bool {
    warhead.is_some_and(|warhead| {
        warhead
            .verses_f64
            .get(armor)
            .is_some_and(|verses| *verses == 0.0)
    })
}

/// Weapon-slot selection with the class overrides that run BEFORE the base
/// ladder. Returns the weapon-array index native returns; it never returns
/// "no weapon" — legality is `GetFireError`'s job (`select_weapon_for_target`).
///
/// - `InfantryClass @ 0x005218E0`: a `DeployFire=yes` infantry never enters
///   the ladder — deployed sequences fire `DeployFireWeapon`, otherwise
///   `OpenTransportWeapon` inside an open-topped transport, otherwise slot 0.
/// - `UnitClass @ 0x00746CD0`: a deployed (`+0x6E0`) `DeployFire=yes` vehicle
///   fires `DeployFireWeapon`; everything else runs the ladder.
pub(crate) fn what_weapon_should_i_use(
    rules: &RuleSet,
    obj: &ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetFacts>,
) -> i32 {
    match attacker.kind {
        TechnoKind::Infantry if obj.deploy_fire => {
            if attacker.deploy_fire_active {
                return deploy_fire_weapon_index(obj);
            }
            return attacker.open_transport_weapon.unwrap_or(0);
        }
        TechnoKind::Unit if attacker.deploy_fire_active && obj.deploy_fire => {
            return deploy_fire_weapon_index(obj);
        }
        _ => {}
    }
    techno_what_weapon_should_i_use(rules, obj, attacker, target)
}

/// `TechnoClass::What_Weapon_Should_I_Use @ 0x006F3330`, arm by arm in native
/// order. Addresses name the first instruction of each arm.
fn techno_what_weapon_should_i_use(
    rules: &RuleSet,
    obj: &ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetFacts>,
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
    let Some(secondary) =
        secondary_for_tier(obj, attacker.veterancy).and_then(|id| rules.weapon(id))
    else {
        return 0;
    };
    let Some(primary) = primary_for_tier(obj, attacker.veterancy).and_then(|id| rules.weapon(id))
    else {
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
        let stage = attacker.gattling_stage;
        if secondary_aa && techno.is_some_and(TargetFacts::is_high_flying) {
            return stage * 2 + 1;
        }
        return stage * 2;
    }
    // I @ 0x006F3477: Airstrike secondary — only against a C4-able building
    // that is not BOTH a resource gatherer and a resource destination.
    if secondary_warhead.is_some_and(|warhead| warhead.airstrike) {
        let TargetFacts::Techno {
            obj: target_obj,
            kind: TechnoKind::Building,
            ..
        } = target
        else {
            return 0;
        };
        if !target_obj.can_c4 {
            return 0;
        }
        if !target_obj.resource_destination {
            return 1;
        }
        if !target_obj.resource_gatherer {
            return 1;
        }
        return 0;
    }
    // J @ 0x006F3528: IsLocomotor primary (Magnetron) vs a building.
    if primary_warhead.is_some_and(|warhead| warhead.is_locomotor)
        && target.kind() == Some(TechnoKind::Building)
    {
        return 1;
    }
    // K @ 0x006F3558: DrainWeapon secondary vs a Drainable non-ally while not
    // already draining.
    if secondary.drain_weapon {
        if let Some(TargetFacts::Techno {
            obj: target_obj,
            is_ally,
            ..
        }) = techno
        {
            if target_obj.drainable && !attacker.drain_target_active && !is_ally {
                return 1;
            }
        }
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
    {
        if secondary_warhead.is_some_and(|warhead| warhead.electric_assault)
            && target_obj.overpowerable
        {
            return 1;
        }
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
        is_high_flying,
        on_bridge,
        cell_land_type,
        ..
    }) = techno
    else {
        return 0;
    };
    let armor = armor_index(&target_obj.armor);
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
    if *is_high_flying {
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
    if !*is_high_flying && obj.land_targeting == 2 {
        return 1;
    }
    // V @ 0x006F37E7: unconditional AA-secondary against a high-flying target.
    if secondary_aa && *is_high_flying {
        return 1;
    }
    0
}

/// The target-legality subset of `TechnoClass::GetFireError @ 0x006FC0B0`
/// that depends only on the selected weapon and the target. Returns true when
/// the shot is ILLEGAL (native 5) or CANNOT (native 6). Ammo, ROF, cloak,
/// busy-effect and range verdicts live elsewhere.
fn targeting_fire_error_blocks(
    rules: &RuleSet,
    obj: &ObjectType,
    weapon: &WeaponType,
    warhead: &WarheadType,
    target: &TargetFacts,
    capture: Option<crate::sim::capture_manager::CaptureControllerFacts>,
) -> bool {
    let aa = projectile_aa(rules, weapon);
    match *target {
        // Terrain is admitted only to the index-selection owner above. No
        // Terrain firing caller is delivered by the repair query; retain an
        // explicit unsupported legality result instead of treating it as Cell.
        TargetFacts::Terrain => true,
        TargetFacts::Cell { land_type, .. } => {
            // 0x006FCA81: CanInfect(NULL) refuses every cell target.
            if warhead.parasite {
                return true;
            }
            // 0x006FC7EB..0x006FC812: a non-Techno target that is not
            // high-flying needs an AG projectile. `CellClass` vtable `+0x54`
            // (`0x00410530`) always returns 0, so for a cell this is exactly
            // "no AG projectile → ILLEGAL"; the AA-only Flak Cannon and Aegis
            // cannot force-fire terrain at all.
            if !projectile_ag(rules, weapon) {
                return true;
            }
            // 0x006FC815..0x006FC868: a cell whose LandType is neither
            // Water(2) nor Beach(6), fired at by a `LandTargeting=1` type.
            if land_type != LandType::Water.as_index()
                && land_type != LandType::Beach.as_index()
                && obj.land_targeting == 1
            {
                return true;
            }
            false
        }
        TargetFacts::Techno {
            obj: target_obj,
            is_high_flying,
            on_bridge,
            cell_land_type,
            parasite,
            capture: capture_victim,
            has_bomb,
            ..
        } => {
            // 0x006FCA81..0x006FCAC2: a Parasite warhead is ILLEGAL unless the
            // firer's ParasiteClass could infect this target (CanInfect
            // 0x0062A8E0; buildings reach it as NULL). The launch lock
            // (0x006FCAE1) and Iron Curtain (0x006FCB21) gates read the frame
            // and are applied at fire admission instead.
            if warhead.parasite && !parasite.admits(obj.naval) {
                return true;
            }
            // 0x006FCB24..0x006FCB50: a MindControl warhead is ILLEGAL unless
            // the firer's CaptureManager could capture this target.
            if warhead.mind_control
                && !capture.is_some_and(|controller| {
                    crate::sim::capture_manager::can_capture(controller, capture_victim)
                })
            {
                return true;
            }
            // 0x006FC705..0x006FC739: `IsHighFlying && !AA` → 5 (3 when the
            // target is this object's `DeployedFrom`; both block the shot).
            //
            // RESIDUAL (UNCHECKED) — 0x006FC73C..0x006FC75C is a second gate:
            // a Foot target whose `InWhichLayer (vt+0x78) != 2` also needs an
            // AA projectile. For a Foot target that slot is NOT
            // `ObjectClass::InWhichLayer @ 0x005F4260` (which would make the
            // gate identical to `IsHighFlying`) — `FootClass 0x004DB7E0` and
            // `AircraftClass 0x0041ADC0` both forward it to the attached
            // locomotor's own vtable `+0x74`, which VERA does not model.
            // Trigger: a ground weapon shooting an airborne unit whose
            // locomotor reports a non-ground layer while its altitude is still
            // under two levels — a Rocketeer or Kirov just after lift-off, an
            // aircraft mid-landing. Player effect: VERA lets the shot through
            // where gamemd may answer ILLEGAL. Frequency: brief windows at the
            // start and end of every flight. Downstream: none — the verdict is
            // recomputed every tick.
            if is_high_flying && !aa {
                return true;
            }
            // 0x006FC76A..0x006FC7CA: the naval `-1` verdict, under the same
            // water/not-high-flying/not-on-bridge gate ladder step T used.
            let mut on_water = (cell_land_type == LandType::Water.as_index()
                || cell_land_type == LandType::Beach.as_index())
                && !is_high_flying;
            if !on_bridge {
                if on_water && select_naval_targeting_weapon(obj, Some(target)) == -1 {
                    return true;
                }
            } else {
                on_water = false;
            }
            // 0x006FC7D0..0x006FC868: `target->IsLowFlying (vt+0x50)` and not
            // on water, fired at by a `LandTargeting=1` type.
            //
            // RESIDUAL (UNCHECKED) — `ObjectClass::IsLowFlying @ 0x005F6B60`
            // is `marked-on-map (+0x74) && height < 2 * LeptonsPerLevel`, so
            // an unmarked object is neither low- nor high-flying and native
            // skips this gate; VERA reads `!is_high_flying`, which is true for
            // an unmarked object. Trigger: only a target that is not marked on
            // the map, i.e. in limbo inside a transport — never a fire target
            // in ordinary play. Frequency: unreachable as written.
            if !is_high_flying && !on_water && obj.land_targeting == 1 {
                return true;
            }
            // 0x006FCB6A: `FLD Warhead.Verses[armor]`, `FCOMP 0.0` → 5.
            if verses_is_zero(Some(warhead), armor_index(&target_obj.armor)) {
                return true;
            }
            // 0x006FCB8D..0x006FCBCD: a BombDisarm weapon needs a bombed
            // target and an IvanBomb weapon an unbombed one (→ 5).
            if (warhead.bomb_disarm && !has_bomb) || (warhead.ivan_bomb && has_bomb) {
                return true;
            }
            false
        }
    }
}

/// Turn a ladder index into the weapon it names and apply the GetFireError
/// targeting verdicts.
///
/// RESIDUAL (UNCHECKED) — `BuildingClass::GetWeapon @ 0x004526F0` (vtable
/// `+0x3F8`) is a real override: when `IsOccupied (vt+0x400)` is set and a
/// firing occupant is selected, it returns the occupant type's `OccupyWeapon`
/// (`+0xE04`, elite `+0xE20`), falling back to that occupant's `GetWeapon(0)`,
/// instead of the building's own slot. Ladder arm B therefore resolves to the
/// occupant's weapon natively, not to the building's `Primary=`. VERA performs
/// the substitution only on the fire path (`select_garrison_weapon`) and in the
/// dedicated garrison auto-acquire scan (`combat::tick_combat`, the
/// `can_be_occupied && can_occupy_fire` block); `resolve_index` here reads the
/// building's own slot 0. *Trigger:* a garrisoned building reached through
/// `can_retaliate` or `calculate_ai_threat_score`. *Player effect:* a garrisoned
/// civilian building (no `Primary=` of its own) resolves to no weapon, so it
/// does not retaliate in the same tick it is shot and scores no AI threat; the
/// auto-acquire scan still picks the shooter up on a later tick, so the shot
/// itself is not lost. *Frequency:* every garrisoned building on a city map that
/// takes fire. *Downstream:* AI threat ranking only; no deterministic state.
fn resolve_index<'a>(
    rules: &'a RuleSet,
    obj: &'a ObjectType,
    veterancy: u16,
    index: i32,
    target: &TargetFacts,
    capture: Option<crate::sim::capture_manager::CaptureControllerFacts>,
) -> Option<SelectedWeapon<'a>> {
    let selected = resolve_index_for_emission(rules, obj, veterancy, index, Some(target))?;
    (!targeting_fire_error_blocks(
        rules,
        obj,
        selected.weapon,
        selected.warhead,
        target,
        capture,
    ))
    .then_some(selected)
}

/// GetWeapon/warhead resolution without repeating GetFireError. Mission_Attack
/// 418432..418476 selects on each burst iteration after one admission only.
pub(crate) fn select_weapon_for_emission<'a>(
    rules: &'a RuleSet,
    obj: &'a ObjectType,
    attacker: &AttackerFacts,
    target: Option<&TargetFacts>,
) -> Option<SelectedWeapon<'a>> {
    let index = what_weapon_should_i_use(rules, obj, attacker, target);
    resolve_index_for_emission(rules, obj, attacker.veterancy, index, target)
}

fn resolve_index_for_emission<'a>(
    rules: &'a RuleSet,
    obj: &'a ObjectType,
    veterancy: u16,
    index: i32,
    target: Option<&TargetFacts>,
) -> Option<SelectedWeapon<'a>> {
    let (weapon_id, slot) = weapon_for_index(obj, veterancy, index)?;
    let weapon = rules.weapon(weapon_id)?;
    let warhead = warhead_of(rules, weapon)?;
    let verses_pct = match target {
        Some(TargetFacts::Techno {
            obj: target_obj, ..
        }) => warhead
            .verses
            .get(armor_index(&target_obj.armor))
            .copied()
            .unwrap_or(100),
        _ => 100,
    };
    Some(SelectedWeapon {
        weapon_id,
        weapon,
        warhead,
        verses_pct,
        slot,
        index,
    })
}

/// Run the selection ladder against a resolved target and apply the
/// GetFireError targeting verdicts. `None` = the selected weapon cannot
/// legally fire at this target.
pub(crate) fn select_weapon_for_target<'a>(
    rules: &'a RuleSet,
    obj: &'a ObjectType,
    attacker: &AttackerFacts,
    target: &TargetFacts,
) -> Option<SelectedWeapon<'a>> {
    let index = what_weapon_should_i_use(rules, obj, attacker, Some(target));
    resolve_index(
        rules,
        obj,
        attacker.veterancy,
        index,
        target,
        attacker.capture,
    )
}

/// Resolve exactly one saved static weapon slot against the current target.
///
/// Unlike normal selection, this never re-runs the ladder. The delayed
/// Building fire path stores `CurrentWeaponNumber` while arming and asks that
/// same slot for its live tier/legality when the delay expires.
pub(crate) fn select_weapon_slot<'a>(
    rules: &'a RuleSet,
    obj: &'a ObjectType,
    veterancy: u16,
    slot: WeaponSlot,
    target: &TargetFacts,
    capture: Option<crate::sim::capture_manager::CaptureControllerFacts>,
) -> Option<SelectedWeapon<'a>> {
    let index = match slot {
        WeaponSlot::Primary => 0,
        WeaponSlot::Secondary => 1,
    };
    resolve_index(rules, obj, veterancy, index, target, capture)
}

/// `HouseClass::Is_Ally_ByObject @ 0x004F9A90` reduced to house identity:
/// same house, or the asker's alliance bit for the other house. Without an
/// alliance map only the same-house case is knowable.
pub(crate) fn is_ally_by_object(
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

/// `ObjectClass::IsHighFlying` for a represented entity: locomotor altitude
/// at or above two levels. Category is irrelevant — a landed aircraft is a
/// ground target and an airborne jumpjet infantry is an air target.
pub(crate) fn target_is_high_flying(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .map(|locomotor| locomotor.altitude.to_num::<i64>())
        .unwrap_or(0)
        >= HIGH_FLIGHT_THRESHOLD_LEPTONS
}

fn open_transport_weapon_from_override(weapon_override: Option<WeaponOverride>) -> Option<i32> {
    match weapon_override {
        Some(WeaponOverride::OpenTransport(slot)) => i32::try_from(slot).ok(),
        _ => None,
    }
}

fn current_weapon_number_from_override(weapon_override: Option<WeaponOverride>) -> i32 {
    match weapon_override {
        Some(WeaponOverride::IfvSlot(index)) => i32::try_from(index).unwrap_or(0),
        _ => 0,
    }
}

/// Read the attacker facts from live entity state.
///
/// RESIDUAL (GSI-08.02, row 122) — inputs whose native state VERA does not
/// yet represent are pinned at their constructor value:
/// - `gattling_stage` = 0 (`TechnoClass+0x140`, stepped by
///   `TechnoClass::UpdateGattlingStage @ 0x0070E000` — row 123). Trigger:
///   every Gattling Cannon/Tank shot after spin-up; player effect: the
///   stage-0 weapon pair (AGGattling/AAGattCann) is always used, so the
///   later-stage damage/ROF never arrives; frequency: continuous in any Yuri
///   match; downstream: none beyond row 123.
/// - `drain_target_active` = false (`TechnoClass+0x1CC`, written by the
///   drain link @ `0x0070FD70`). Trigger: a Floating Disc already draining
///   one building being ordered onto a second Drainable one; effect: the
///   drain beam is selected again instead of the laser; frequency: rare.
/// - `is_overpowered_building` = false (`BuildingClass+0x661`, writer
///   `BuildingClass::Update 0x0044015B`). This is a DEFERRAL, not an
///   approximation: false is the genuine constructor state, so VERA behaves as
///   a game in which no coil is ever charged rather than guessing a charge
///   model. Trigger: a player deliberately parking three Tesla Troopers on an
///   `Overpowerable=true` building (stock: TESLA, CAEAST01/02, CAPARS01), or
///   one while the house is at full power ratio; effect: the coil never fires
///   `OPCoilBolt`; frequency: only that micro, which is far rarer than the
///   gattling residual above.
/// - `aircraft_spawn_collision` = false (`AircraftClass+0x6CA`, writer
///   `SpawnRetreat__Push 0x0054E47D`). Trigger: Hornet/ASW pushed to
///   retreat; effect: the collision secondary is never chosen; frequency:
///   every carrier/destroyer spawn cycle end.
/// - `open_transport_weapon` is read from the transport-side override, not
///   from a passenger's `+0x82` flag, because VERA's open-topped fire is
///   emitted by the transport (passengers never reach the fire path).
pub(crate) fn attacker_facts(entity: &GameEntity, obj: &ObjectType) -> AttackerFacts {
    let kind = TechnoKind::from_category(entity.category);
    let deploy_fire_active = match kind {
        TechnoKind::Infantry => matches!(
            entity.deploy_state,
            Some(DeployPhase::Deploying { .. } | DeployPhase::Deployed)
        ),
        TechnoKind::Unit => matches!(entity.deploy_state, Some(DeployPhase::Deployed)),
        TechnoKind::Aircraft | TechnoKind::Building => false,
    };
    let is_occupied_building = kind == TechnoKind::Building
        && obj.can_be_occupied
        && obj.can_occupy_fire
        && entity
            .passenger_role
            .cargo()
            .is_some_and(|cargo| !cargo.is_empty());
    AttackerFacts {
        kind,
        veterancy: entity.veterancy,
        current_weapon_number: current_weapon_number_from_override(entity.weapon_override),
        open_transport_weapon: open_transport_weapon_from_override(entity.weapon_override),
        gattling_stage: 0,
        deploy_fire_active,
        is_occupied_building,
        // `TechnoClass+0x1CC DrainTarget`, the live drain link (GSI-09.01).
        drain_target_active: entity.drain_target.is_some(),
        // `MissionClass::GetCurrentMission @ 0x005B3040`: current, else queued.
        mission_is_unload: entity.mission.effective().known() == Some(MissionType::Unload),
        is_overpowered_building: false,
        aircraft_spawn_collision: false,
        capture: crate::sim::capture_manager::CaptureControllerFacts::of(entity),
    }
}

/// Attacker facts from a combat snapshot, for scan paths that hold no entity
/// borrow. The snapshot carries no mission, so the AreaFire/Unload arm reads
/// false here; every production caller prefers `attacker_facts` when the
/// entity is resolvable.
pub(crate) fn attacker_facts_from_snapshot(
    snap: &AttackerSnapshot,
    obj: &ObjectType,
) -> AttackerFacts {
    let kind = TechnoKind::from_category(snap.category);
    let deploy_fire_active = match kind {
        TechnoKind::Infantry => {
            snap.is_fully_deployed || snap.animation_sequence == Some(SequenceKind::Deploy)
        }
        TechnoKind::Unit => snap.is_fully_deployed,
        TechnoKind::Aircraft | TechnoKind::Building => false,
    };
    AttackerFacts {
        kind,
        veterancy: snap.veterancy,
        current_weapon_number: current_weapon_number_from_override(snap.weapon_override),
        open_transport_weapon: open_transport_weapon_from_override(snap.weapon_override),
        gattling_stage: 0,
        deploy_fire_active,
        is_occupied_building: kind == TechnoKind::Building
            && snap.garrison.is_some()
            && obj.can_be_occupied
            && obj.can_occupy_fire,
        drain_target_active: false,
        mission_is_unload: false,
        is_overpowered_building: false,
        aircraft_spawn_collision: false,
        // No entity, no manager: a MindControl weapon reads as illegal.
        capture: None,
    }
}

/// Target facts for a Techno target. `terrain == None` (headless fixtures)
/// reads the occupied cell as clear land.
pub(crate) fn techno_target_facts<'a>(
    target: &GameEntity,
    target_obj: &'a ObjectType,
    terrain: Option<&ResolvedTerrainGrid>,
    is_ally: bool,
) -> TargetFacts<'a> {
    let cell = terrain.and_then(|grid| grid.cell(target.position.rx, target.position.ry));
    let cell_land_type = cell
        .map(|cell| cell.yr_cell_land_type)
        .unwrap_or(LandType::Clear.as_index());
    TargetFacts::Techno {
        obj: target_obj,
        kind: TechnoKind::from_category(target.category),
        is_high_flying: target_is_high_flying(target),
        on_bridge: target.on_bridge,
        cell_land_type,
        submerged: target.cloak.as_ref().is_some_and(|cloak| cloak.state != 0),
        is_ally,
        parasite: ParasiteVictimFacts::of(target, target_obj, terrain),
        capture: crate::sim::capture_manager::CaptureVictimFacts::of(target, target_obj, None),
        has_bomb: target.bomb.is_some(),
    }
}

/// Target facts for a force-fire cell.
///
/// RESIDUAL: the native water-set predicate compares the cell's tile index
/// against `[0x00AA0738] .. +14`; VERA reads the cell's base `LandType ==
/// Water` instead. Trigger: a Boomer force-firing a cell whose water tile
/// carries a non-water LandType override; effect: torpedo vs cruise slot;
/// frequency: negligible in ordinary play.
pub(crate) fn cell_target_facts(
    rx: u16,
    ry: u16,
    terrain: Option<&ResolvedTerrainGrid>,
) -> TargetFacts<'static> {
    match terrain.and_then(|grid| grid.cell(rx, ry)) {
        Some(cell) => TargetFacts::Cell {
            land_type: cell.yr_cell_land_type,
            tile_in_water_set: cell.is_water,
            bridge_flag: cell.bridge_facts.has_structural_bridge(),
        },
        None => TargetFacts::Cell {
            land_type: LandType::Clear.as_index(),
            tile_in_water_set: false,
            bridge_flag: false,
        },
    }
}

/// Production entry: resolve the target, build both fact sets, run the
/// ladder and the GetFireError targeting subset.
#[allow(clippy::too_many_arguments)]
pub(crate) fn select_weapon_against<'a>(
    rules: &'a RuleSet,
    attacker_obj: &'a ObjectType,
    attacker: &AttackerFacts,
    attacker_owner: InternedId,
    target: &TargetKind,
    entities: &EntityStore,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<SelectedWeapon<'a>> {
    let target_facts = match *target {
        TargetKind::Entity(target_id) => {
            let target_entity = entities.get(target_id)?;
            let target_obj = rules.object(interner.resolve(target_entity.type_ref()))?;
            let is_ally =
                is_ally_by_object(alliances, interner, attacker_owner, target_entity.owner());
            techno_target_facts(target_entity, target_obj, terrain, is_ally)
        }
        TargetKind::Cell(rx, ry) => cell_target_facts(rx, ry, terrain),
    };
    select_weapon_for_target(rules, attacker_obj, attacker, &target_facts)
}

/// Select the weapon used by a garrisoned occupant firing from a building.
///
/// Priority chain (matching gamemd `BuildingClass::GetWeapon` 0x004526F0):
/// 1. Elite occupant → `EliteOccupyWeapon`
/// 2. Normal occupant → `OccupyWeapon`
/// 3. Fallback → occupant's Primary weapon
///
/// Returns None if no weapon can engage the target type.
pub(crate) fn select_garrison_weapon<'a>(
    rules: &'a RuleSet,
    occupant_type_ref: &str,
    occupant_veterancy: u16,
    target_category: EntityCategory,
    target_armor: &str,
) -> Option<SelectedWeapon<'a>> {
    let occupant_obj = rules.object(occupant_type_ref)?;
    let is_elite = occupant_veterancy >= ELITE_VETERANCY;

    // Elite missing EliteOccupyWeapon falls directly to Primary; it does not
    // reuse the normal OccupyWeapon path.
    let occupy_weapon_id = if is_elite {
        occupant_obj.elite_occupy_weapon.as_deref()
    } else {
        occupant_obj.occupy_weapon.as_deref()
    };

    if let Some(wid) = occupy_weapon_id {
        if let Some(sw) = try_garrison_weapon(rules, wid, target_category, target_armor) {
            return Some(sw);
        }
    }

    // Fallback: occupant's primary weapon.
    if let Some(ref primary) = occupant_obj.primary {
        return try_garrison_weapon(rules, primary, target_category, target_armor);
    }
    None
}

/// Garrison-only legality: projectile AA/AG against the target category and
/// a non-zero Verses entry. VERA-internal shape retained for the occupant
/// path; the building's own `GetFireError` verdicts for occupant fire are
/// UNCHECKED here.
fn try_garrison_weapon<'a>(
    rules: &'a RuleSet,
    weapon_id: &'a str,
    target_category: EntityCategory,
    target_armor: &str,
) -> Option<SelectedWeapon<'a>> {
    let weapon: &WeaponType = rules.weapon(weapon_id)?;
    let legal = match target_category {
        EntityCategory::Aircraft => projectile_aa(rules, weapon),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Structure => {
            projectile_ag(rules, weapon)
        }
    };
    if !legal {
        return None;
    }
    let warhead: &WarheadType = warhead_of(rules, weapon)?;
    let idx: usize = armor_index(target_armor);
    let verses_pct: u8 = warhead.verses.get(idx).copied().unwrap_or(100);
    if verses_gate(verses_pct) == VersesGate::Blocked {
        return None;
    }
    Some(SelectedWeapon {
        weapon_id,
        weapon,
        warhead,
        verses_pct,
        slot: WeaponSlot::Primary,
        index: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

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
        let mut building = GameEntity::test_default(1, "HOUSE", "Americans", 5, 5);
        building.category = EntityCategory::Structure;
        let facts = attacker_facts(&building, obj);
        assert!(passive_scan_has_aa(&rules, obj, facts, None));
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
                passive_scan_has_aa(&rules, obj, facts, Some((occupant, veterancy))),
                expected
            );
        }
    }

    #[test]
    fn passive_scan_infantry_primary_warhead_clears_aa_after_resolved_slot_union() {
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
            dog.category = EntityCategory::Infantry;
            assert_eq!(
                passive_scan_has_aa(&rules, obj, attacker_facts(&dog, obj), None),
                medium == "0.001"
            );
            dog.veterancy = 200;
            assert!(
                passive_scan_has_aa(&rules, obj, attacker_facts(&dog, obj), None),
                "elite primary is resolved first"
            );
            dog.veterancy = 0;
            dog.category = EntityCategory::Unit;
            assert!(
                passive_scan_has_aa(&rules, obj, attacker_facts(&dog, obj), None),
                "the clear belongs to Infantry"
            );
        }
    }

    #[test]
    fn test_weapon_override_variants() {
        let ifv = WeaponOverride::IfvSlot(16);
        let open = WeaponOverride::OpenTransport(1);
        assert_ne!(ifv, open);
        assert_eq!(ifv, WeaponOverride::IfvSlot(16));
        assert_eq!(open, WeaponOverride::OpenTransport(1));
    }

    #[test]
    fn test_verses_gate_thresholds() {
        assert_eq!(verses_gate(0), VersesGate::Blocked);
        assert_eq!(verses_gate(1), VersesGate::Suppressed);
        assert_eq!(verses_gate(2), VersesGate::Normal);
        assert_eq!(verses_gate(100), VersesGate::Normal);
        assert_eq!(verses_gate(200), VersesGate::Normal);
    }

    /// Stock-shaped fixture: every type/weapon/projectile/warhead value below
    /// is copied from retail `rulesmd.ini` for the keys this selector reads.
    fn stock_rules() -> RuleSet {
        let ini_str: &str = "\
[InfantryTypes]
0=GGI
1=E1
2=TANY
3=BORIS
4=SHK
5=ROCK
6=ENGINEER
[VehicleTypes]
0=DEST
1=SUB
2=SQD
3=AEGIS
4=BSUB
5=FV
6=SREF
7=YTNK
8=DISK
9=HTNK
10=MGTK
11=LCRF
[AircraftTypes]
0=ORCA
1=ASW
[BuildingTypes]
0=GAPOWR
1=TESLA
2=NAREFN
3=YAREFN
4=CAMISC01
5=NAFLAK
6=YAGGUN
[General]
MissileROTVar=.25

[GGI]
Strength=100
Armor=none
Primary=M60
Secondary=MissileLauncher
ElitePrimary=M60E
EliteSecondary=MissileLauncherE
OpenTransportWeapon=1
DeployFire=yes

[E1]
Strength=125
Armor=none
Primary=M60
Secondary=Para
DeployFire=yes
OpenTransportWeapon=1

[TANY]
Strength=125
Armor=none
Primary=DoublePistols
Secondary=Sapper
ElitePrimary=DoublePistolsE
EliteSecondary=Sapper
NavalTargeting=4
SpeedType=Amphibious
OpenTransportWeapon=0

[BORIS]
Strength=125
Armor=none
Primary=AKM
Secondary=Flare
NavalTargeting=4
OpenTransportWeapon=0

[SHK]
Strength=125
Armor=plate
Primary=ElectricBolt
Secondary=AssaultBolt

[ROCK]
Strength=125
Armor=none
ConsideredAircraft=yes
Primary=RocketeerGun

[ENGINEER]
Strength=75
Armor=none
IFVMode=1

[DEST]
Strength=800
Armor=heavy
Primary=155mm
Secondary=ASWLauncher
NavalTargeting=1
SpeedType=Float
Naval=yes

[SUB]
Strength=600
Armor=light
Primary=SubTorpedo
NavalTargeting=5
LandTargeting=1
Underwater=yes
SpeedType=Float
Naval=yes

[SQD]
Strength=400
Armor=light
Primary=SquidGrab
Secondary=SquidPunch
NavalTargeting=3
LandTargeting=1
Underwater=yes
Organic=yes
SpeedType=Float
Naval=yes

[AEGIS]
Strength=800
Armor=heavy
Primary=Medusa
NavalTargeting=6
LandTargeting=1
SpeedType=Float
Naval=yes

[BSUB]
Strength=800
Armor=heavy
Primary=BoomerTorpedo
Secondary=CruiseLauncher
NavalTargeting=7
LandTargeting=2
Underwater=yes
Unnatural=yes
SpeedType=Float
Naval=yes

[FV]
Strength=200
Armor=light
Primary=HoverMissile
Gunner=yes
TurretCount=4
WeaponCount=17
Weapon1=HoverMissile
EliteWeapon1=HoverMissileE
Weapon2=RepairBullet
EliteWeapon2=RepairBullet
Weapon3=CRM60

[SREF]
Strength=300
Armor=heavy
TurretCount=4
WeaponCount=1
Weapon1=Comet
EliteWeapon1=SuperComet

[YTNK]
Strength=210
Armor=light
IsGattling=yes
TurretCount=1
WeaponCount=6
Primary=AGGattling
Secondary=AAGattling
Weapon1=AGGattling
EliteWeapon1=AGGattlingE
Weapon2=AAGattling
EliteWeapon2=AAGattlingE
Weapon3=AGGattling2
Weapon4=AAGattling2
Weapon5=AGGattling3
Weapon6=AAGattling3

[DISK]
Strength=400
Armor=light
Primary=DiskLaser
Secondary=DiskDrain
SpeedType=Hover
ConsideredAircraft=yes

[HTNK]
Strength=400
Armor=heavy
Primary=120mm

[MGTK]
Strength=200
Armor=light
Primary=MagneticBeam
Secondary=MagneticBeam2

[LCRF]
Strength=400
Armor=light
SpeedType=Hover
Naval=yes

[ORCA]
Strength=150
Armor=light
Primary=Maverick

[ASW]
Strength=100
Armor=light
Primary=ASWBomb
Secondary=ASWCollision
NavalTargeting=2
LandTargeting=1

[GAPOWR]
Strength=750
Armor=wood
Drainable=yes

[TESLA]
Strength=600
Armor=steel
Primary=CoilBolt
Secondary=OPCoilBolt
Overpowerable=true

[NAREFN]
Strength=1000
Armor=wood
ResourceDestination=yes

[YAREFN]
Strength=1000
Armor=wood
ResourceDestination=yes
ResourceGatherer=yes

[CAMISC01]
Strength=100
Armor=none
CanC4=no

[NAFLAK]
Strength=900
Armor=steel
Primary=FlakWeapon
NavalTargeting=6
LandTargeting=1

[YAGGUN]
Strength=1000
Armor=steel
IsGattling=yes
TurretCount=1
WeaponCount=6
Weapon1=AGGattling
EliteWeapon1=AGGattlingE
Weapon2=AAGattCann
EliteWeapon2=AAGattlingE
Weapon3=AGGattling2
Weapon4=AAGattCann2
Weapon5=AGGattling3
Weapon6=AAGattCann3

[M60]
Damage=15
Projectile=InvisibleLow
Warhead=SA
[M60E]
Damage=25
Projectile=InvisibleLow
Warhead=SA
[MissileLauncher]
Damage=40
Projectile=AAHeatSeeker2
Warhead=GUARDWH
[MissileLauncherE]
Damage=50
Projectile=AAHeatSeeker2
Warhead=GUARDWH
[Para]
Damage=15
Projectile=InvisibleLow
Warhead=SA
[DoublePistols]
Damage=125
Projectile=InvisibleLow
Warhead=HollowPoint3
[DoublePistolsE]
Damage=125
Projectile=InvisibleLow
Warhead=HollowPoint3
[Sapper]
Damage=2500
Projectile=Invisible
Warhead=Mechanical
[AKM]
Damage=125
Projectile=InvisibleLow
Warhead=HollowPoint3
[Flare]
Damage=1
Projectile=Invisible
Warhead=AirstrikeFlare
[ElectricBolt]
Damage=30
Projectile=Invisible
Warhead=Electric
[AssaultBolt]
Damage=10
Projectile=InvisibleLow
Warhead=ElectricAssault
[RocketeerGun]
Damage=20
Projectile=InvisibleLow
Warhead=SA
[155mm]
Damage=100
Projectile=Cannon
Warhead=HE
[ASWLauncher]
Damage=1
Projectile=ASWVirt
Warhead=Special
[SubTorpedo]
Damage=100
Projectile=Torpedo
Warhead=APSplash
[SquidGrab]
Damage=1
Projectile=Invisible
Warhead=Squiddy
[SquidPunch]
Damage=100
Projectile=InvisibleLow
Warhead=HE
[Medusa]
Damage=100
Projectile=MedusaProjectile
Warhead=SAMWH
[BoomerTorpedo]
Damage=175
Projectile=Torpedo
Warhead=APSplash
[CruiseLauncher]
Damage=25
Projectile=InvisibleHigh
Warhead=Special
[HoverMissile]
Damage=30
Projectile=AAHeatSeeker2
Warhead=SA
[HoverMissileE]
Damage=40
Projectile=AAHeatSeeker2
Warhead=SA
[RepairBullet]
Damage=-50
Projectile=Invisible
Warhead=Mechanical
[CRM60]
Damage=15
Projectile=InvisibleLow
Warhead=SA
[Comet]
Damage=100
Projectile=Invisible
Warhead=CometWH
[SuperComet]
Damage=150
Projectile=Invisible
Warhead=CometWH
[AGGattling]
Damage=20
Projectile=Invisible
Warhead=SA
[AGGattlingE]
Damage=25
Projectile=Invisible
Warhead=SA
[AAGattling]
Damage=25
Projectile=AAHeatSeeker
Warhead=Flak
[AAGattlingE]
Damage=30
Projectile=AAHeatSeeker
Warhead=Flak
[AGGattling2]
Damage=30
Projectile=Invisible
Warhead=SA
[AAGattling2]
Damage=35
Projectile=AAHeatSeeker
Warhead=Flak
[AGGattling3]
Damage=40
Projectile=Invisible
Warhead=SA
[AAGattling3]
Damage=45
Projectile=AAHeatSeeker
Warhead=Flak
[AAGattCann]
Damage=25
Projectile=AAHeatSeeker
Warhead=Flak
[AAGattCann2]
Damage=35
Projectile=AAHeatSeeker
Warhead=Flak
[AAGattCann3]
Damage=45
Projectile=AAHeatSeeker
Warhead=Flak
[DiskLaser]
Damage=100
Projectile=Invisible
Warhead=DiskLaserWH
[DiskDrain]
Damage=1
Projectile=Invisible
Warhead=DiskDrainWH
DrainWeapon=yes
[120mm]
Damage=90
Projectile=Cannon
Warhead=AP
[MagneticBeam]
Damage=1
Projectile=Invisible
Warhead=LocomotorBeam
[MagneticBeam2]
Damage=100
Projectile=Invisible
Warhead=AP
[Maverick]
Damage=100
Projectile=Cannon
Warhead=HE
[ASWBomb]
Damage=150
Projectile=Cannon
Warhead=HE
[ASWCollision]
Damage=100
Projectile=AAHeatSeeker2
Warhead=AP
[CoilBolt]
Damage=200
Projectile=Invisible
Warhead=Electric
[OPCoilBolt]
Damage=350
Projectile=Invisible
Warhead=Electric
[FlakWeapon]
Damage=40
Projectile=FlakProj
Warhead=Flak

[InvisibleLow]
AG=yes
AA=no
[Invisible]
Inviso=yes
[InvisibleHigh]
Inviso=yes
[AAHeatSeeker2]
AA=yes
AG=yes
[AAHeatSeeker]
AA=yes
AG=no
[Cannon]
AG=yes
AA=no
[ASWVirt]
AA=no
AG=no
[Torpedo]
AG=yes
AA=no
[MedusaProjectile]
AA=yes
AG=no
[FlakProj]
AA=yes
AG=no

[SA]
Verses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%
[GUARDWH]
Verses=20%,20%,20%,100%,50%,100%,10%,10%,10%,100%,100%
[HollowPoint3]
Verses=100%,100%,100%,2%,2%,2%,1%,1%,1%,100%,100%
[Mechanical]
Verses=0%,0%,0%,100%,100%,100%,0%,0%,0%,100%,100%
[AirstrikeFlare]
Verses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%
Airstrike=yes
[Electric]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[ElectricAssault]
Verses=0%,0,0%,0%,0%,0%,100%,100%,100%,50%,100%
ElectricAssault=yes
[HE]
Verses=100%,100%,100%,80%,60%,40%,100%,40%,20%,100%,100%
[Special]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[APSplash]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[Squiddy]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[SAMWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[CometWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[Flak]
Verses=100%,100%,100%,80%,60%,40%,100%,40%,20%,0%,0%
[DiskLaserWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[DiskDrainWH]
Verses=0%,0%,0%,0%,0%,0%,100%,100%,100%,100%,100%
[AP]
Verses=10%,25%,50%,100%,100%,100%,50%,60%,100%,100%,100%
[LocomotorBeam]
Verses=0%,0%,0%,100%,100%,100%,0%,0%,0%,100%,100%
IsLocomotor=yes
";
        let ini: IniFile = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("Should parse stock-shaped rules")
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
            capture: None,
        }
    }

    fn techno<'a>(obj: &'a ObjectType, kind: TechnoKind) -> TargetFacts<'a> {
        TargetFacts::Techno {
            obj,
            kind,
            is_high_flying: false,
            on_bridge: false,
            cell_land_type: LandType::Clear.as_index(),
            submerged: false,
            is_ally: false,
            parasite: ParasiteVictimFacts::INFECTABLE_ON_LAND,
            capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
            has_bomb: false,
        }
    }

    fn on_water<'a>(target: TargetFacts<'a>) -> TargetFacts<'a> {
        match target {
            TargetFacts::Techno {
                obj,
                kind,
                is_high_flying,
                on_bridge,
                submerged,
                is_ally,
                parasite,
                ..
            } => TargetFacts::Techno {
                obj,
                kind,
                is_high_flying,
                on_bridge,
                cell_land_type: LandType::Water.as_index(),
                submerged,
                is_ally,
                parasite: ParasiteVictimFacts {
                    on_water_set: true,
                    ..parasite
                },
                capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
                has_bomb: false,
            },
            cell => cell,
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
                parasite,
                ..
            } => TargetFacts::Techno {
                obj,
                kind,
                is_high_flying: true,
                on_bridge,
                cell_land_type,
                submerged,
                is_ally,
                parasite,
                capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
                has_bomb: false,
            },
            cell => cell,
        }
    }

    fn allied<'a>(target: TargetFacts<'a>) -> TargetFacts<'a> {
        match target {
            TargetFacts::Techno {
                obj,
                kind,
                is_high_flying,
                on_bridge,
                cell_land_type,
                submerged,
                parasite,
                ..
            } => TargetFacts::Techno {
                obj,
                kind,
                is_high_flying,
                on_bridge,
                cell_land_type,
                submerged,
                is_ally: true,
                parasite,
                capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
                has_bomb: false,
            },
            cell => cell,
        }
    }

    fn slot(
        rules: &RuleSet,
        attacker: &str,
        facts: &AttackerFacts,
        target: Option<&TargetFacts>,
    ) -> i32 {
        what_weapon_should_i_use(rules, rules.object(attacker).unwrap(), facts, target)
    }

    fn selected<'a>(
        rules: &'a RuleSet,
        attacker: &str,
        facts: &AttackerFacts,
        target: &TargetFacts,
    ) -> Option<&'a str> {
        select_weapon_for_target(rules, rules.object(attacker).unwrap(), facts, target)
            .map(|selected| selected.weapon_id)
    }

    // ---- GetWeapon ---------------------------------------------------------

    #[test]
    fn get_weapon_elite_tier_and_weapon_list_fallbacks() {
        let rules = stock_rules();
        let ggi = rules.object("GGI").unwrap();
        assert_eq!(
            weapon_for_index(ggi, 0, 0).unwrap(),
            ("M60", WeaponSlot::Primary)
        );
        assert_eq!(
            weapon_for_index(ggi, 199, 0).unwrap(),
            ("M60", WeaponSlot::Primary)
        );
        assert_eq!(
            weapon_for_index(ggi, 200, 0).unwrap(),
            ("M60E", WeaponSlot::Primary)
        );
        assert_eq!(
            weapon_for_index(ggi, 200, 1).unwrap(),
            ("MissileLauncherE", WeaponSlot::Secondary)
        );
        assert!(weapon_for_index(ggi, 0, -1).is_none());
        assert!(weapon_for_index(ggi, 0, 2).is_none());
        assert!(weapon_for_index(ggi, 0, 18).is_none());
        // FV: no Secondary=, so slot 1 is Weapon2; elite slot with the same
        // weapon still resolves; slot 2 is Weapon3.
        let fv = rules.object("FV").unwrap();
        assert_eq!(
            weapon_for_index(fv, 0, 1).unwrap(),
            ("RepairBullet", WeaponSlot::Secondary)
        );
        assert_eq!(
            weapon_for_index(fv, 200, 0).unwrap(),
            ("HoverMissileE", WeaponSlot::Primary)
        );
        assert_eq!(
            weapon_for_index(fv, 0, 2).unwrap(),
            ("CRM60", WeaponSlot::Primary)
        );
        // TANY: elite secondary names the same Sapper.
        let tany = rules.object("TANY").unwrap();
        assert_eq!(weapon_for_index(tany, 200, 1).unwrap().0, "Sapper");
        // SREF: Weapon1 only, no Primary=. WeaponCount=1 stops the loop, so
        // slot 1 stays empty even though the array has room.
        let sref = rules.object("SREF").unwrap();
        assert_eq!(primary_for_tier(sref, 0), Some("Comet"));
        assert_eq!(primary_for_tier(sref, 200), Some("SuperComet"));
        assert_eq!(secondary_for_tier(sref, 0), None);
    }

    #[test]
    fn weapon_list_and_primary_are_mutually_exclusive_on_turret_count() {
        // `TechnoTypeClass::ReadINI @ 0x007128B2`: `TurretCount > 0` fills the
        // slots from `WeaponN=` and jumps past the `Primary=` block; otherwise
        // `Primary=`/`Secondary=` are read and `WeaponN=` is ignored. Neither
        // side ever falls back to the other.
        //
        // The *keys* are mutually exclusive; the *storage* is shared. Whichever
        // branch runs writes weapon-array slots 0/1 at `+0x898`/`+0x8B4`, which
        // are the fields gamemd calls `Primary`/`Secondary` — so `obj.primary`
        // must read as the resolved slot 0 either way.
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TURRETED\n1=PLAINWEP\n\
             [TURRETED]\nStrength=100\nArmor=none\nTurretCount=2\nWeaponCount=2\n\
             Primary=PrimGun\nSecondary=SecGun\nElitePrimary=PrimGun\n\
             Weapon1=SlotGun\nWeapon2=SlotGun2\nEliteWeapon1=SlotGunE\n\
             [PLAINWEP]\nStrength=100\nArmor=none\nPrimary=PrimGun\n\
             Weapon1=SlotGun\nWeapon2=SlotGun2\nWeaponCount=2\n\
             [PrimGun]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [SecGun]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [SlotGun]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [SlotGun2]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [SlotGunE]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [Inv]\nAG=yes\nAA=no\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        // TurretCount=2: the `WeaponN=` keys win outright, and they land in the
        // `Primary`/`Secondary` FIELDS — the `Primary=`/`Secondary=` keys are
        // dead but the fields they name are not.
        let turreted = rules.object("TURRETED").unwrap();
        assert_eq!(turreted.primary.as_deref(), Some("SlotGun"));
        assert_eq!(turreted.secondary.as_deref(), Some("SlotGun2"));
        assert_eq!(turreted.elite_primary.as_deref(), Some("SlotGunE"));
        assert_eq!(turreted.elite_secondary, None);
        assert_eq!(primary_for_tier(turreted, 0), Some("SlotGun"));
        assert_eq!(secondary_for_tier(turreted, 0), Some("SlotGun2"));
        assert_eq!(primary_for_tier(turreted, 200), Some("SlotGunE"));
        // EliteWeapon2 is unauthored, so the elite tier falls back to Weapon2
        // inside GetWeapon — not to EliteSecondary.
        assert_eq!(secondary_for_tier(turreted, 200), Some("SlotGun2"));
        // Slot 2 is past WeaponCount.
        assert!(weapon_for_index(turreted, 0, 2).is_none());
        // TurretCount unauthored (0): WeaponN is dead, and there is no
        // secondary even though Weapon2 names one.
        let plain = rules.object("PLAINWEP").unwrap();
        assert_eq!(plain.primary.as_deref(), Some("PrimGun"));
        assert_eq!(plain.secondary, None);
        assert_eq!(primary_for_tier(plain, 0), Some("PrimGun"));
        assert_eq!(secondary_for_tier(plain, 0), None);
        assert!(weapon_for_index(plain, 0, 1).is_none());
    }

    // ---- Naval selector ----------------------------------------------------

    #[test]
    fn naval_targeting_truth_table() {
        let rules = stock_rules();
        let sub = rules.object("SUB").unwrap();
        let sqd = rules.object("SQD").unwrap();
        let bsub = rules.object("BSUB").unwrap();
        let lcrf = rules.object("LCRF").unwrap();
        let dest = rules.object("DEST").unwrap();
        let sub_t = techno(sub, TechnoKind::Unit);
        let submerged_sub = TargetFacts::Techno {
            obj: sub,
            kind: TechnoKind::Unit,
            is_high_flying: false,
            on_bridge: false,
            cell_land_type: LandType::Water.as_index(),
            submerged: true,
            is_ally: false,
            parasite: ParasiteVictimFacts {
                on_water_set: true,
                ..ParasiteVictimFacts::INFECTABLE_ON_LAND
            },
            capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
            has_bomb: false,
        };
        let sqd_t = techno(sqd, TechnoKind::Unit);
        let bsub_t = techno(bsub, TechnoKind::Unit);
        let lcrf_t = techno(lcrf, TechnoKind::Unit);
        let dest_t = techno(dest, TechnoKind::Unit);
        let cell = TargetFacts::Cell {
            land_type: LandType::Water.as_index(),
            tile_in_water_set: true,
            bridge_flag: false,
        };
        // Null / cell target → -1.
        assert_eq!(select_naval_targeting_weapon(dest, None), -1);
        assert_eq!(select_naval_targeting_weapon(dest, Some(&cell)), -1);
        // 0 (default, HTNK): surfaced sub 0, submerged sub -1, ship 0.
        let htnk = rules.object("HTNK").unwrap();
        assert_eq!(select_naval_targeting_weapon(htnk, Some(&sub_t)), 0);
        assert_eq!(
            select_naval_targeting_weapon(htnk, Some(&submerged_sub)),
            -1
        );
        assert_eq!(select_naval_targeting_weapon(htnk, Some(&dest_t)), 0);
        // 1 (DEST): sub 1, ship 0, even submerged.
        assert_eq!(select_naval_targeting_weapon(dest, Some(&submerged_sub)), 1);
        assert_eq!(select_naval_targeting_weapon(dest, Some(&dest_t)), 0);
        // 2 (ASW): sub 0, ship -1.
        let asw = rules.object("ASW").unwrap();
        assert_eq!(select_naval_targeting_weapon(asw, Some(&sub_t)), 0);
        assert_eq!(select_naval_targeting_weapon(asw, Some(&dest_t)), -1);
        // 3 (SQD): organic/unnatural 1, else 0.
        assert_eq!(select_naval_targeting_weapon(sqd, Some(&sqd_t)), 1);
        assert_eq!(select_naval_targeting_weapon(sqd, Some(&bsub_t)), 1);
        assert_eq!(select_naval_targeting_weapon(sqd, Some(&dest_t)), 0);
        // 4 (TANY): C4 unless hover or organic.
        let tany = rules.object("TANY").unwrap();
        assert_eq!(select_naval_targeting_weapon(tany, Some(&dest_t)), 1);
        assert_eq!(select_naval_targeting_weapon(tany, Some(&lcrf_t)), 0);
        assert_eq!(select_naval_targeting_weapon(tany, Some(&sqd_t)), 0);
        // 5 (SUB) and 7 (BSUB): 0.
        assert_eq!(select_naval_targeting_weapon(sub, Some(&dest_t)), 0);
        assert_eq!(select_naval_targeting_weapon(bsub, Some(&dest_t)), 0);
        // 6 (AEGIS): -1.
        let aegis = rules.object("AEGIS").unwrap();
        assert_eq!(select_naval_targeting_weapon(aegis, Some(&dest_t)), -1);
    }

    // ---- Ladder arms -------------------------------------------------------

    #[test]
    fn arm_a_turret_count_returns_gunner_slot_unconditionally() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let tank = techno(htnk, TechnoKind::Unit);
        let mut fv = facts(TechnoKind::Unit);
        assert_eq!(slot(&rules, "FV", &fv, Some(&tank)), 0);
        fv.current_weapon_number = 1; // Engineer aboard → RepairBullet slot.
        assert_eq!(slot(&rules, "FV", &fv, Some(&tank)), 1);
        fv.current_weapon_number = -1;
        assert_eq!(slot(&rules, "FV", &fv, Some(&tank)), 0);
        // Null target still reports the gunner slot (arm A precedes arm F).
        fv.current_weapon_number = 2;
        assert_eq!(slot(&rules, "FV", &fv, None), 2);
        // SREF: TurretCount=4 and no gunner → 0.
        assert_eq!(
            slot(&rules, "SREF", &facts(TechnoKind::Unit), Some(&tank)),
            0
        );
        // Gattling types bypass arm A even with TurretCount=1.
        let mut ytnk = facts(TechnoKind::Unit);
        ytnk.current_weapon_number = 7;
        assert_eq!(slot(&rules, "YTNK", &ytnk, Some(&tank)), 0);
    }

    #[test]
    fn ifv_engineer_repair_slot_is_returned_even_against_an_illegal_target() {
        // Arm A returns the gunner slot with no target check at all: an
        // Engineer IFV asked to attack a building still reports slot 1
        // (RepairBullet) and never falls back to HoverMissile. GetFireError
        // then rejects it — RepairBullet's Mechanical warhead is 0% vs `wood`.
        let rules = stock_rules();
        let powr = rules.object("GAPOWR").unwrap();
        let building = techno(powr, TechnoKind::Building);
        let mut fv = facts(TechnoKind::Unit);
        fv.current_weapon_number = 1;
        assert_eq!(slot(&rules, "FV", &fv, Some(&building)), 1);
        assert_eq!(selected(&rules, "FV", &fv, &building), None);
        // Against a vehicle with repairable armor the repair slot fires.
        let fv_obj = rules.object("FV").unwrap();
        let light = techno(fv_obj, TechnoKind::Unit);
        assert_eq!(selected(&rules, "FV", &fv, &light), Some("RepairBullet"));
    }

    #[test]
    fn arm_b_occupied_building_reports_slot_zero() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let tank = techno(htnk, TechnoKind::Unit);
        let mut tesla = facts(TechnoKind::Building);
        tesla.is_occupied_building = true;
        tesla.is_overpowered_building = true; // would otherwise reach arm M
        assert_eq!(slot(&rules, "TESLA", &tesla, Some(&tank)), 0);
    }

    #[test]
    fn arms_c_d_f_missing_slot_or_null_target_return_zero() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let tank = high_flying(techno(htnk, TechnoKind::Unit));
        // HTNK has no secondary → 0 even against an air target.
        assert_eq!(
            slot(&rules, "HTNK", &facts(TechnoKind::Unit), Some(&tank)),
            0
        );
        // Null target → 0 for a dual-weapon unit.
        assert_eq!(slot(&rules, "DEST", &facts(TechnoKind::Unit), None), 0);
    }

    #[test]
    fn arm_e_never_use_secondary_returns_zero() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=DOGGY\n[DOGGY]\nStrength=100\nArmor=none\nPrimary=Bite\nSecondary=VirtualScanner\n\
             [Bite]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [VirtualScanner]\nDamage=0\nProjectile=AAInv\nWarhead=WH\nNeverUse=yes\n\
             [Inv]\nAG=yes\nAA=no\n[AAInv]\nAG=yes\nAA=yes\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let doggy = rules.object("DOGGY").unwrap();
        let air = high_flying(techno(doggy, TechnoKind::Unit));
        assert_eq!(
            slot(&rules, "DOGGY", &facts(TechnoKind::Unit), Some(&air)),
            0
        );
    }

    #[test]
    fn arm_g_open_transport_weapon_wins_over_target_facts() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let tank = techno(htnk, TechnoKind::Unit);
        // Tanya in a Battle Fortress fires slot 0 even at a ship on water.
        let dest = rules.object("DEST").unwrap();
        let ship = on_water(techno(dest, TechnoKind::Unit));
        let mut tany = facts(TechnoKind::Infantry);
        tany.open_transport_weapon = Some(0);
        assert_eq!(slot(&rules, "TANY", &tany, Some(&ship)), 0);
        // A non-DeployFire vehicle attacker with the transport bridge: slot 1.
        let mut bridge = facts(TechnoKind::Unit);
        bridge.open_transport_weapon = Some(1);
        assert_eq!(slot(&rules, "DEST", &bridge, Some(&tank)), 1);
    }

    #[test]
    fn arm_h_gattling_stage_pair() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let orca = rules.object("ORCA").unwrap();
        let ground = techno(htnk, TechnoKind::Unit);
        let air = high_flying(techno(orca, TechnoKind::Aircraft));
        let landed = techno(orca, TechnoKind::Aircraft);
        for attacker in ["YTNK", "YAGGUN"] {
            let mut gat = facts(if attacker == "YTNK" {
                TechnoKind::Unit
            } else {
                TechnoKind::Building
            });
            for stage in 0..3 {
                gat.gattling_stage = stage;
                assert_eq!(slot(&rules, attacker, &gat, Some(&ground)), stage * 2);
                assert_eq!(slot(&rules, attacker, &gat, Some(&landed)), stage * 2);
                assert_eq!(slot(&rules, attacker, &gat, Some(&air)), stage * 2 + 1);
            }
            // A cell target is not a Techno: AG slot.
            gat.gattling_stage = 1;
            let cell = TargetFacts::Cell {
                land_type: 0,
                tile_in_water_set: false,
                bridge_flag: false,
            };
            assert_eq!(slot(&rules, attacker, &gat, Some(&cell)), 2);
        }
        // Stage 2 against air resolves Weapon6 (AAGattling3) for the tank.
        let mut gat = facts(TechnoKind::Unit);
        gat.gattling_stage = 2;
        assert_eq!(selected(&rules, "YTNK", &gat, &air), Some("AAGattling3"));
        // Elite stage 0 resolves EliteWeapon1/EliteWeapon2.
        gat.gattling_stage = 0;
        gat.veterancy = 200;
        assert_eq!(selected(&rules, "YTNK", &gat, &ground), Some("AGGattlingE"));
        assert_eq!(selected(&rules, "YTNK", &gat, &air), Some("AAGattlingE"));
    }

    #[test]
    fn arm_i_boris_airstrike_target_table() {
        let rules = stock_rules();
        let boris = facts(TechnoKind::Infantry);
        let powr = rules.object("GAPOWR").unwrap();
        let narefn = rules.object("NAREFN").unwrap();
        let yarefn = rules.object("YAREFN").unwrap();
        let barrel = rules.object("CAMISC01").unwrap();
        let htnk = rules.object("HTNK").unwrap();
        // Ordinary building: flare.
        assert_eq!(
            slot(
                &rules,
                "BORIS",
                &boris,
                Some(&techno(powr, TechnoKind::Building))
            ),
            1
        );
        // Refinery (ResourceDestination only): flare.
        assert_eq!(
            slot(
                &rules,
                "BORIS",
                &boris,
                Some(&techno(narefn, TechnoKind::Building))
            ),
            1
        );
        // Slave miner refinery (both flags): never airstruck.
        assert_eq!(
            slot(
                &rules,
                "BORIS",
                &boris,
                Some(&techno(yarefn, TechnoKind::Building))
            ),
            0
        );
        // CanC4=no: rifle.
        assert_eq!(
            slot(
                &rules,
                "BORIS",
                &boris,
                Some(&techno(barrel, TechnoKind::Building))
            ),
            0
        );
        // Non-building: rifle, and the ladder stops here (no naval arm).
        assert_eq!(
            slot(
                &rules,
                "BORIS",
                &boris,
                Some(&on_water(techno(htnk, TechnoKind::Unit)))
            ),
            0
        );
        let cell = TargetFacts::Cell {
            land_type: 0,
            tile_in_water_set: false,
            bridge_flag: false,
        };
        assert_eq!(slot(&rules, "BORIS", &boris, Some(&cell)), 0);
    }

    #[test]
    fn arm_j_locomotor_primary_vs_building_picks_secondary() {
        let rules = stock_rules();
        let powr = rules.object("GAPOWR").unwrap();
        let htnk = rules.object("HTNK").unwrap();
        let mgtk = facts(TechnoKind::Unit);
        assert_eq!(
            slot(
                &rules,
                "MGTK",
                &mgtk,
                Some(&techno(powr, TechnoKind::Building))
            ),
            1
        );
        assert_eq!(
            slot(&rules, "MGTK", &mgtk, Some(&techno(htnk, TechnoKind::Unit))),
            0
        );
    }

    #[test]
    fn arm_k_drain_weapon_requires_drainable_non_ally_and_no_active_drain() {
        let rules = stock_rules();
        let powr = rules.object("GAPOWR").unwrap();
        let tesla = rules.object("TESLA").unwrap();
        let mut disk = facts(TechnoKind::Unit);
        let power = techno(powr, TechnoKind::Building);
        assert_eq!(slot(&rules, "DISK", &disk, Some(&power)), 1);
        assert_eq!(slot(&rules, "DISK", &disk, Some(&allied(power))), 0);
        // Not Drainable: laser.
        assert_eq!(
            slot(
                &rules,
                "DISK",
                &disk,
                Some(&techno(tesla, TechnoKind::Building))
            ),
            0
        );
        disk.drain_target_active = true;
        assert_eq!(slot(&rules, "DISK", &disk, Some(&power)), 0);
    }

    #[test]
    fn arm_l_area_fire_while_unloading() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=CAOSX\n[CAOSX]\nStrength=100\nArmor=none\nPrimary=Gun\nSecondary=Wave\n\
             [Gun]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [Wave]\nDamage=10\nProjectile=Inv\nWarhead=WH\nAreaFire=yes\n\
             [Inv]\nAG=yes\nAA=no\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let obj = rules.object("CAOSX").unwrap();
        let target = techno(obj, TechnoKind::Unit);
        let mut caos = facts(TechnoKind::Unit);
        assert_eq!(slot(&rules, "CAOSX", &caos, Some(&target)), 0);
        caos.mission_is_unload = true;
        assert_eq!(slot(&rules, "CAOSX", &caos, Some(&target)), 1);
    }

    #[test]
    fn arm_m_overpowered_building_fires_secondary() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let tank = techno(htnk, TechnoKind::Unit);
        let mut tesla = facts(TechnoKind::Building);
        assert_eq!(slot(&rules, "TESLA", &tesla, Some(&tank)), 0);
        tesla.is_overpowered_building = true;
        assert_eq!(slot(&rules, "TESLA", &tesla, Some(&tank)), 1);
        assert_eq!(selected(&rules, "TESLA", &tesla, &tank), Some("OPCoilBolt"));
    }

    #[test]
    fn arm_n_tesla_trooper_charges_only_allied_overpowerable_buildings() {
        let rules = stock_rules();
        let tesla = rules.object("TESLA").unwrap();
        let powr = rules.object("GAPOWR").unwrap();
        let shk = facts(TechnoKind::Infantry);
        let coil = techno(tesla, TechnoKind::Building);
        assert_eq!(slot(&rules, "SHK", &shk, Some(&allied(coil))), 1);
        // Enemy coil: arm N needs the ally relation, so the ladder runs on to
        // arm V (AssaultBolt's projectile is not AA) and reports the bolt.
        assert_eq!(slot(&rules, "SHK", &shk, Some(&coil)), 0);
        // Allied but not Overpowerable: bolt.
        assert_eq!(
            slot(
                &rules,
                "SHK",
                &shk,
                Some(&allied(techno(powr, TechnoKind::Building)))
            ),
            0
        );
    }

    #[test]
    fn arm_o_aircraft_spawn_collision_flag() {
        let rules = stock_rules();
        let dest = rules.object("DEST").unwrap();
        let ship = techno(dest, TechnoKind::Unit);
        let mut asw = facts(TechnoKind::Aircraft);
        assert_eq!(slot(&rules, "ASW", &asw, Some(&ship)), 0);
        asw.aircraft_spawn_collision = true;
        assert_eq!(slot(&rules, "ASW", &asw, Some(&ship)), 1);
    }

    #[test]
    fn arm_p_boomer_cell_targets() {
        let rules = stock_rules();
        let bsub = facts(TechnoKind::Unit);
        let land = TargetFacts::Cell {
            land_type: LandType::Clear.as_index(),
            tile_in_water_set: false,
            bridge_flag: false,
        };
        let water = TargetFacts::Cell {
            land_type: LandType::Water.as_index(),
            tile_in_water_set: true,
            bridge_flag: false,
        };
        let bridge_over_water = TargetFacts::Cell {
            land_type: LandType::Water.as_index(),
            tile_in_water_set: true,
            bridge_flag: true,
        };
        let beach = TargetFacts::Cell {
            land_type: LandType::Beach.as_index(),
            tile_in_water_set: false,
            bridge_flag: false,
        };
        assert_eq!(slot(&rules, "BSUB", &bsub, Some(&land)), 1);
        assert_eq!(slot(&rules, "BSUB", &bsub, Some(&beach)), 1);
        assert_eq!(slot(&rules, "BSUB", &bsub, Some(&water)), 0);
        assert_eq!(slot(&rules, "BSUB", &bsub, Some(&bridge_over_water)), 1);
        // Non-naval LandTargeting=2 type: bridge clause does not apply.
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=LANDX\n[LANDX]\nStrength=100\nArmor=none\nPrimary=Gun\nSecondary=Gun2\nLandTargeting=2\n\
             [Gun]\nDamage=10\nProjectile=Inv\nWarhead=WH\n[Gun2]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [Inv]\nAG=yes\nAA=no\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let land_rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(
            slot(
                &land_rules,
                "LANDX",
                &facts(TechnoKind::Unit),
                Some(&bridge_over_water)
            ),
            0
        );
        assert_eq!(
            slot(&land_rules, "LANDX", &facts(TechnoKind::Unit), Some(&land)),
            1
        );
        // DEST (LandTargeting=0) force-firing land: slot 0.
        assert_eq!(
            slot(&rules, "DEST", &facts(TechnoKind::Unit), Some(&land)),
            0
        );
    }

    #[test]
    fn arms_r_s_verses_zero_pair() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let powr = rules.object("GAPOWR").unwrap();
        let ggi = rules.object("GGI").unwrap();
        // Tanya vs infantry: Sapper's Mechanical warhead is 0% vs `none`, so
        // arm R stops the ladder at the pistols.
        assert_eq!(
            slot(
                &rules,
                "TANY",
                &facts(TechnoKind::Infantry),
                Some(&techno(ggi, TechnoKind::Infantry))
            ),
            0
        );
        // Tanya vs a building: Mechanical is 0% vs `wood` too → arm R → 0.
        // (C4 on buildings is the C4 mission, not this selector.)
        assert_eq!(
            slot(
                &rules,
                "TANY",
                &facts(TechnoKind::Infantry),
                Some(&techno(powr, TechnoKind::Building))
            ),
            0
        );
        // Custom: X's primary is immune to `heavy` (armor index 5) and its
        // secondary is not → arm S returns 1. Y's secondary is immune too, and
        // arm R runs first → 0, even though the primary would have worked.
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=X\n1=Y\n\
             [X]\nStrength=100\nArmor=heavy\nPrimary=A\nSecondary=B\n\
             [Y]\nStrength=100\nArmor=heavy\nPrimary=B\nSecondary=A\n\
             [A]\nDamage=10\nProjectile=Inv\nWarhead=WA\n[B]\nDamage=10\nProjectile=Inv\nWarhead=WB\n\
             [Inv]\nAG=yes\nAA=no\n\
             [WA]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n\
             [WB]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let custom = RuleSet::from_ini(&ini).unwrap();
        let heavy_target = techno(custom.object("X").unwrap(), TechnoKind::Unit);
        assert_eq!(
            slot(&custom, "X", &facts(TechnoKind::Unit), Some(&heavy_target)),
            1
        );
        // Y: secondary immune → arm R short-circuits to 0 before arm S is
        // reached, so the working primary is the answer for the other reason.
        assert_eq!(
            slot(&custom, "Y", &facts(TechnoKind::Unit), Some(&heavy_target)),
            0
        );
        // Sanity: the stock-rules HTNK target has the same `heavy` armor.
        assert_eq!(
            slot(
                &custom,
                "X",
                &facts(TechnoKind::Unit),
                Some(&techno(htnk, TechnoKind::Unit))
            ),
            1
        );
    }

    #[test]
    fn arm_t_destroyer_depth_charges_typhoon_on_water_only() {
        let rules = stock_rules();
        let sub = rules.object("SUB").unwrap();
        let dest_obj = rules.object("DEST").unwrap();
        let dest = facts(TechnoKind::Unit);
        let typhoon = on_water(techno(sub, TechnoKind::Unit));
        assert_eq!(slot(&rules, "DEST", &dest, Some(&typhoon)), 1);
        assert_eq!(
            selected(&rules, "DEST", &dest, &typhoon),
            Some("ASWLauncher")
        );
        // A ship on water: deck gun.
        let ship = on_water(techno(dest_obj, TechnoKind::Unit));
        assert_eq!(selected(&rules, "DEST", &dest, &ship), Some("155mm"));
        // A sub standing on a bridge deck (impossible in stock, but the gate
        // is literal): land path → 0.
        let TargetFacts::Techno {
            obj,
            kind,
            is_high_flying,
            cell_land_type,
            submerged,
            is_ally,
            parasite,
            ..
        } = typhoon
        else {
            unreachable!()
        };
        let bridged = TargetFacts::Techno {
            obj,
            kind,
            is_high_flying,
            on_bridge: true,
            cell_land_type,
            submerged,
            is_ally,
            parasite,
            capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
            has_bomb: false,
        };
        assert_eq!(slot(&rules, "DEST", &dest, Some(&bridged)), 0);
    }

    #[test]
    fn arm_t_naval_minus_one_collapses_to_zero_and_fire_error_makes_it_illegal() {
        let rules = stock_rules();
        let dest_obj = rules.object("DEST").unwrap();
        let sub = rules.object("SUB").unwrap();
        let ship = on_water(techno(dest_obj, TechnoKind::Unit));
        // AEGIS (NavalTargeting=6) vs a ship: selector 0, GetFireError ILLEGAL.
        assert_eq!(
            slot(&rules, "AEGIS", &facts(TechnoKind::Unit), Some(&ship)),
            0
        );
        assert_eq!(
            selected(&rules, "AEGIS", &facts(TechnoKind::Unit), &ship),
            None
        );
        // Rhino vs a submerged sub: ILLEGAL; vs a surfaced sub: legal.
        let submerged = TargetFacts::Techno {
            obj: sub,
            kind: TechnoKind::Unit,
            is_high_flying: false,
            on_bridge: false,
            cell_land_type: LandType::Water.as_index(),
            submerged: true,
            is_ally: false,
            parasite: ParasiteVictimFacts {
                on_water_set: true,
                ..ParasiteVictimFacts::INFECTABLE_ON_LAND
            },
            capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
            has_bomb: false,
        };
        assert_eq!(
            selected(&rules, "HTNK", &facts(TechnoKind::Unit), &submerged),
            None
        );
        assert_eq!(
            selected(
                &rules,
                "HTNK",
                &facts(TechnoKind::Unit),
                &on_water(techno(sub, TechnoKind::Unit))
            ),
            Some("120mm")
        );
        // Beach counts as water for the gate.
        let TargetFacts::Techno { obj, kind, .. } = ship else {
            unreachable!()
        };
        let beach = TargetFacts::Techno {
            obj,
            kind,
            is_high_flying: false,
            on_bridge: false,
            cell_land_type: LandType::Beach.as_index(),
            submerged: false,
            is_ally: false,
            parasite: ParasiteVictimFacts::INFECTABLE_ON_LAND,
            capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
            has_bomb: false,
        };
        assert_eq!(
            selected(&rules, "AEGIS", &facts(TechnoKind::Unit), &beach),
            None
        );
    }

    #[test]
    fn tanya_c4_and_squid_punch_against_ships() {
        let rules = stock_rules();
        let dest_obj = rules.object("DEST").unwrap();
        let sqd_obj = rules.object("SQD").unwrap();
        let lcrf = rules.object("LCRF").unwrap();
        let ship = on_water(techno(dest_obj, TechnoKind::Unit));
        let tany = facts(TechnoKind::Infantry);
        assert_eq!(selected(&rules, "TANY", &tany, &ship), Some("Sapper"));
        // Hovercraft: pistols.
        assert_eq!(
            selected(
                &rules,
                "TANY",
                &tany,
                &on_water(techno(lcrf, TechnoKind::Unit))
            ),
            Some("DoublePistols")
        );
        // Squid vs Destroyer: grab; vs Squid: punch.
        let sqd = facts(TechnoKind::Unit);
        assert_eq!(selected(&rules, "SQD", &sqd, &ship), Some("SquidGrab"));
        assert_eq!(
            selected(
                &rules,
                "SQD",
                &sqd,
                &on_water(techno(sqd_obj, TechnoKind::Unit))
            ),
            Some("SquidPunch")
        );
        // Squid vs a tank on land: LandTargeting=1 → ILLEGAL.
        let htnk = rules.object("HTNK").unwrap();
        assert_eq!(
            selected(&rules, "SQD", &sqd, &techno(htnk, TechnoKind::Unit)),
            None
        );
    }

    #[test]
    fn arm_u_boomer_land_targeting_two_against_land_objects() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let orca = rules.object("ORCA").unwrap();
        let bsub = facts(TechnoKind::Unit);
        assert_eq!(
            slot(&rules, "BSUB", &bsub, Some(&techno(htnk, TechnoKind::Unit))),
            1
        );
        // High-flying target: arm U skipped, arm V needs an AA secondary → 0.
        assert_eq!(
            slot(
                &rules,
                "BSUB",
                &bsub,
                Some(&high_flying(techno(orca, TechnoKind::Aircraft)))
            ),
            0
        );
        // Ship on water: naval selector (7 → 0) → torpedo.
        let dest_obj = rules.object("DEST").unwrap();
        assert_eq!(
            slot(
                &rules,
                "BSUB",
                &bsub,
                Some(&on_water(techno(dest_obj, TechnoKind::Unit)))
            ),
            0
        );
    }

    #[test]
    fn arm_v_aa_secondary_is_unconditional_on_the_primary() {
        let rules = stock_rules();
        let orca = rules.object("ORCA").unwrap();
        let air = high_flying(techno(orca, TechnoKind::Aircraft));
        // Deployed GGI: DeployFireWeapon regardless.
        let mut ggi = facts(TechnoKind::Infantry);
        ggi.deploy_fire_active = true;
        assert_eq!(selected(&rules, "GGI", &ggi, &air), Some("MissileLauncher"));
        // A dual-AA type: secondary still wins against air.
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=DUAL\n[DUAL]\nStrength=100\nArmor=none\nPrimary=A\nSecondary=B\n\
             [A]\nDamage=10\nProjectile=AAInv\nWarhead=WH\n[B]\nDamage=10\nProjectile=AAInv\nWarhead=WH\n\
             [AAInv]\nAG=yes\nAA=yes\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let dual = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(slot(&dual, "DUAL", &facts(TechnoKind::Unit), Some(&air)), 1);
        let dual_obj = dual.object("DUAL").unwrap();
        assert_eq!(
            slot(
                &dual,
                "DUAL",
                &facts(TechnoKind::Unit),
                Some(&techno(dual_obj, TechnoKind::Unit))
            ),
            0
        );
    }

    // ---- Class overrides ---------------------------------------------------

    #[test]
    fn infantry_override_deploy_fire_types_never_enter_the_ladder() {
        let rules = stock_rules();
        let orca = rules.object("ORCA").unwrap();
        let air = high_flying(techno(orca, TechnoKind::Aircraft));
        let mut ggi = facts(TechnoKind::Infantry);
        // Undeployed GGI vs aircraft: slot 0 → M60 has no AA → ILLEGAL.
        assert_eq!(slot(&rules, "GGI", &ggi, Some(&air)), 0);
        assert_eq!(selected(&rules, "GGI", &ggi, &air), None);
        // Deployed (Deploy..DeployedIdle): DeployFireWeapon (default 1).
        ggi.deploy_fire_active = true;
        assert_eq!(slot(&rules, "GGI", &ggi, None), 1);
        assert_eq!(selected(&rules, "GGI", &ggi, &air), Some("MissileLauncher"));
        // Undeployed inside a Battle Fortress: OpenTransportWeapon.
        ggi.deploy_fire_active = false;
        ggi.open_transport_weapon = Some(1);
        assert_eq!(slot(&rules, "GGI", &ggi, Some(&air)), 1);
        // Deployed wins over the transport slot (impossible in play, but the
        // sequence test precedes the transport test natively).
        ggi.deploy_fire_active = true;
        ggi.open_transport_weapon = Some(0);
        assert_eq!(slot(&rules, "GGI", &ggi, Some(&air)), 1);
        // A non-DeployFire infantry runs the ladder: Tanya vs air → 0.
        assert_eq!(
            slot(&rules, "TANY", &facts(TechnoKind::Infantry), Some(&air)),
            0
        );
    }

    #[test]
    fn unit_override_only_when_deployed_flag_and_deploy_fire() {
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=SIEGE\n1=PLAIN\n\
             [SIEGE]\nStrength=100\nArmor=none\nPrimary=A\nSecondary=B\nDeployFire=yes\n\
             [PLAIN]\nStrength=100\nArmor=none\nPrimary=A\nSecondary=B\n\
             [A]\nDamage=10\nProjectile=Inv\nWarhead=WH\n[B]\nDamage=10\nProjectile=Inv\nWarhead=WH\n\
             [Inv]\nAG=yes\nAA=no\n[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).unwrap();
        let plain = rules.object("PLAIN").unwrap();
        let target = techno(plain, TechnoKind::Unit);
        let mut siege = facts(TechnoKind::Unit);
        assert_eq!(slot(&rules, "SIEGE", &siege, Some(&target)), 0);
        siege.deploy_fire_active = true;
        assert_eq!(slot(&rules, "SIEGE", &siege, Some(&target)), 1);
        assert_eq!(slot(&rules, "PLAIN", &siege, Some(&target)), 0);
    }

    /// Unit746CD0 deploy override precedes Techno6F3330; a real Terrain
    /// target skips the Cell LandTargeting arm and keeps nonnull-target arms.
    #[test]
    fn terrain_index_query_preserves_deploy_and_non_techno_branches() {
        let rules=RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=SIEGE\n[SIEGE]\nPrimary=A\nSecondary=B\nDeployFire=yes\nLandTargeting=2\n[A]\nDamage=10\nProjectile=Inv\nWarhead=SA\n[B]\nDamage=20\nProjectile=Inv\nWarhead=WOOD\n[Inv]\nAG=yes\n[SA]\nWood=no\n[WOOD]\nWood=yes\n"
        )).unwrap();
        let mut attacker = facts(TechnoKind::Unit);
        let terrain = TargetFacts::Terrain;
        let cell = TargetFacts::Cell {
            land_type: 0,
            tile_in_water_set: false,
            bridge_flag: false,
        };
        assert_eq!(slot(&rules, "SIEGE", &attacker, Some(&terrain)), 0);
        assert_eq!(slot(&rules, "SIEGE", &attacker, Some(&cell)), 1);
        attacker.deploy_fire_active = true;
        assert_eq!(slot(&rules, "SIEGE", &attacker, Some(&terrain)), 1);
        let obj = rules.object("SIEGE").unwrap();
        let (weapon, _) = weapon_for_index(
            obj,
            0,
            what_weapon_should_i_use(&rules, obj, &attacker, Some(&terrain)),
        )
        .unwrap();
        assert!(
            rules
                .warhead(rules.weapon(weapon).unwrap().warhead.as_deref().unwrap())
                .unwrap()
                .wood
        );
        attacker.deploy_fire_active = false;
        attacker.open_transport_weapon = Some(1);
        assert_eq!(slot(&rules, "SIEGE", &attacker, Some(&terrain)), 1);
        assert_eq!(slot(&rules, "SIEGE", &attacker, None), 0);
    }

    // ---- GetFireError subset ----------------------------------------------

    #[test]
    fn fire_error_aa_only_matters_for_high_flying_targets() {
        let rules = stock_rules();
        let orca = rules.object("ORCA").unwrap();
        let rhino = facts(TechnoKind::Unit);
        // Landed aircraft is a ground target: the 120mm fires.
        assert_eq!(
            selected(&rules, "HTNK", &rhino, &techno(orca, TechnoKind::Aircraft)),
            Some("120mm")
        );
        assert_eq!(
            selected(
                &rules,
                "HTNK",
                &rhino,
                &high_flying(techno(orca, TechnoKind::Aircraft))
            ),
            None
        );
        // AEGIS vs high-flying aircraft: Medusa (AA=yes, AG=no); vs a landed
        // aircraft on land: LandTargeting=1 → ILLEGAL.
        assert_eq!(
            selected(
                &rules,
                "AEGIS",
                &rhino,
                &high_flying(techno(orca, TechnoKind::Aircraft))
            ),
            Some("Medusa")
        );
        assert_eq!(
            selected(&rules, "AEGIS", &rhino, &techno(orca, TechnoKind::Aircraft)),
            None
        );
        // Destroyer's ASWLauncher (AG=no) is not rejected for a sub: no AG
        // test exists for object targets.
        let sub = rules.object("SUB").unwrap();
        assert_eq!(
            selected(
                &rules,
                "DEST",
                &rhino,
                &on_water(techno(sub, TechnoKind::Unit))
            ),
            Some("ASWLauncher")
        );
    }

    #[test]
    fn fire_error_cell_rules() {
        let rules = stock_rules();
        let land = TargetFacts::Cell {
            land_type: LandType::Clear.as_index(),
            tile_in_water_set: false,
            bridge_flag: false,
        };
        let water = TargetFacts::Cell {
            land_type: LandType::Water.as_index(),
            tile_in_water_set: true,
            bridge_flag: false,
        };
        let beach = TargetFacts::Cell {
            land_type: LandType::Beach.as_index(),
            tile_in_water_set: false,
            bridge_flag: false,
        };
        // NAFLAK (LandTargeting=1, FlakProj AG=no): every cell is ILLEGAL —
        // the land cell twice over (no AG projectile, then LandTargeting=1),
        // the water and beach cells on the AG test alone. The AG gate at
        // 0x006FC7EB is unconditional for a cell target: a cell is never
        // high-flying, so an AA-only defense cannot force-fire terrain.
        let flak = facts(TechnoKind::Building);
        assert_eq!(selected(&rules, "NAFLAK", &flak, &land), None);
        assert_eq!(selected(&rules, "NAFLAK", &flak, &water), None);
        assert_eq!(selected(&rules, "NAFLAK", &flak, &beach), None);
        // AEGIS: Medusa is AA-only and there is no secondary, so the same
        // holds even on the water it sits in.
        let aegis = facts(TechnoKind::Unit);
        assert_eq!(selected(&rules, "AEGIS", &aegis, &water), None);
        assert_eq!(selected(&rules, "AEGIS", &aegis, &beach), None);
        // Destroyer force-firing a beach cell: the ladder picks slot 0 (the
        // AG 155mm), not the AG-less ASWLauncher, so the shot is legal.
        assert_eq!(
            selected(&rules, "DEST", &facts(TechnoKind::Unit), &beach),
            Some("155mm")
        );
        // Rhino: either cell.
        assert_eq!(
            selected(&rules, "HTNK", &facts(TechnoKind::Unit), &land),
            Some("120mm")
        );
        assert_eq!(
            selected(&rules, "HTNK", &facts(TechnoKind::Unit), &water),
            Some("120mm")
        );
        // Cell targets read no Verses.
        let sel = select_weapon_for_target(
            &rules,
            rules.object("HTNK").unwrap(),
            &facts(TechnoKind::Unit),
            &land,
        )
        .unwrap();
        assert_eq!(sel.verses_pct, 100);
        assert_eq!(sel.index, 0);
        assert_eq!(sel.slot, WeaponSlot::Primary);
    }

    #[test]
    fn select_weapon_slot_reuses_saved_slot_without_the_ladder() {
        let rules = stock_rules();
        let htnk = rules.object("HTNK").unwrap();
        let tank = techno(htnk, TechnoKind::Unit);
        let tesla = rules.object("TESLA").unwrap();
        let saved =
            select_weapon_slot(&rules, tesla, 0, WeaponSlot::Secondary, &tank, None).unwrap();
        assert_eq!(saved.weapon_id, "OPCoilBolt");
        assert_eq!(saved.slot, WeaponSlot::Secondary);
        assert_eq!(saved.index, 1);
    }

    // ---- Facts builders ----------------------------------------------------

    #[test]
    fn attacker_facts_read_deploy_state_per_class_and_override_slots() {
        let rules = stock_rules();
        let ggi_obj = rules.object("GGI").unwrap();
        let mut ggi = GameEntity::test_default(1, "GGI", "Americans", 1, 1);
        ggi.category = EntityCategory::Infantry;
        assert!(!attacker_facts(&ggi, ggi_obj).deploy_fire_active);
        ggi.deploy_state = Some(DeployPhase::Deploying { ticks_remaining: 3 });
        assert!(attacker_facts(&ggi, ggi_obj).deploy_fire_active);
        ggi.deploy_state = Some(DeployPhase::Deployed);
        assert!(attacker_facts(&ggi, ggi_obj).deploy_fire_active);
        ggi.deploy_state = Some(DeployPhase::Undeploying { ticks_remaining: 3 });
        assert!(!attacker_facts(&ggi, ggi_obj).deploy_fire_active);
        // Vehicles: only the finished Deployed flag.
        let mut siege = GameEntity::test_default(2, "HTNK", "Americans", 1, 1);
        siege.category = EntityCategory::Unit;
        siege.deploy_state = Some(DeployPhase::Deploying { ticks_remaining: 3 });
        assert!(!attacker_facts(&siege, ggi_obj).deploy_fire_active);
        siege.deploy_state = Some(DeployPhase::Deployed);
        assert!(attacker_facts(&siege, ggi_obj).deploy_fire_active);
        // Overrides.
        siege.weapon_override = Some(WeaponOverride::IfvSlot(3));
        let f = attacker_facts(&siege, ggi_obj);
        assert_eq!(f.current_weapon_number, 3);
        assert_eq!(f.open_transport_weapon, None);
        siege.weapon_override = Some(WeaponOverride::OpenTransport(1));
        let f = attacker_facts(&siege, ggi_obj);
        assert_eq!(f.current_weapon_number, 0);
        assert_eq!(f.open_transport_weapon, Some(1));
    }

    #[test]
    fn target_facts_high_flying_ignores_category_and_reads_cloak() {
        let rules = stock_rules();
        let rock_obj = rules.object("ROCK").unwrap();
        let mut rock = GameEntity::test_default(3, "ROCK", "Soviet", 2, 2);
        rock.category = EntityCategory::Infantry;
        let TargetFacts::Techno {
            is_high_flying,
            kind,
            submerged,
            ..
        } = techno_target_facts(&rock, rock_obj, None, false)
        else {
            unreachable!()
        };
        assert!(!is_high_flying);
        assert_eq!(kind, TechnoKind::Infantry);
        assert!(!submerged);
        assert!(!target_is_high_flying(&rock));
    }

    #[test]
    fn is_ally_by_object_same_house_or_alliance() {
        let mut interner = StringInterner::new();
        let a = interner.intern("Americans");
        let s = interner.intern("Soviet");
        assert!(is_ally_by_object(None, &interner, a, a));
        assert!(!is_ally_by_object(None, &interner, a, s));
        // `is_allied_with` looks the map up through `normalize_house_name`
        // (upper-case), so the map must be keyed the same way.
        let mut map = HouseAllianceMap::default();
        map.entry("AMERICANS".to_string())
            .or_default()
            .insert("SOVIET".to_string());
        assert!(is_ally_by_object(Some(&map), &interner, a, s));
        assert!(!is_ally_by_object(Some(&map), &interner, s, a));
    }

    // ---- Garrison ----------------------------------------------------------

    fn make_garrison_rules(include_elite_occupy_weapon: bool) -> RuleSet {
        let elite_occupy_weapon = if include_elite_occupy_weapon {
            "EliteOccupyWeapon=EliteGarrisonRifle\n"
        } else {
            ""
        };
        let ini_str = format!(
            "\
[InfantryTypes]
0=E1
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]

[E1]
Name=GI
Cost=200
Strength=125
Armor=none
Primary=PrimaryRifle
OccupyWeapon=GarrisonRifle
{}

[PrimaryRifle]
Damage=15
ROF=20
Range=4
Projectile=InvisibleLow
Warhead=SA

[GarrisonRifle]
Damage=20
ROF=20
Range=5
Projectile=InvisibleLow
Warhead=SA

[EliteGarrisonRifle]
Damage=30
ROF=20
Range=6
Projectile=InvisibleLow
Warhead=SA

[InvisibleLow]
AG=yes
AA=no

[SA]
Verses=100%,100%,100%,80%,60%,40%,100%,40%,20%,100%,100%
",
            elite_occupy_weapon
        );
        let ini = IniFile::from_str(&ini_str);
        RuleSet::from_ini(&ini).expect("Should parse garrison test rules")
    }

    #[test]
    fn normal_garrison_uses_occupy_weapon() {
        let rules = make_garrison_rules(false);
        let sel =
            select_garrison_weapon(&rules, "E1", 0, EntityCategory::Infantry, "none").unwrap();
        assert_eq!(sel.weapon_id, "GarrisonRifle");
        assert_eq!(sel.slot, WeaponSlot::Primary);
    }

    #[test]
    fn elite_garrison_uses_elite_occupy_weapon_when_present() {
        let rules = make_garrison_rules(true);
        let sel =
            select_garrison_weapon(&rules, "E1", 200, EntityCategory::Infantry, "none").unwrap();
        assert_eq!(sel.weapon_id, "EliteGarrisonRifle");
        assert_eq!(sel.slot, WeaponSlot::Primary);
    }

    #[test]
    fn elite_garrison_missing_elite_occupy_weapon_falls_back_to_primary() {
        let rules = make_garrison_rules(false);
        let sel =
            select_garrison_weapon(&rules, "E1", 200, EntityCategory::Infantry, "none").unwrap();
        assert_eq!(sel.weapon_id, "PrimaryRifle");
        assert_eq!(sel.slot, WeaponSlot::Primary);
    }

    /// GetFireError's bomb gates (0x006FCB8D..0x006FCBCD) against the
    /// `fire_error` rows of `tools/spatial_oracle/bomb_class.json`, produced by
    /// running the original body under Unicorn: 5 is ILLEGAL, "continue"
    /// passes on to the next gate. Weapons and warheads are retail's.
    #[test]
    fn bomb_fire_error_gates_match_native() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]
0=ENGINEER
1=IVAN
[VehicleTypes]
0=HTNK
[Warheads]
0=BombDisarm
1=IvanBomb
2=AP
[ENGINEER]
Primary=DefuseKit
[IVAN]
Primary=IvanBomber
[HTNK]
Strength=900
Armor=heavy
Primary=120mm
[DefuseKit]
Damage=1
Range=1.5
Projectile=InvisibleAll
Warhead=BombDisarm
[IvanBomber]
Damage=400
Range=1.5
Projectile=Invisible
Warhead=IvanBomb
[120mm]
Damage=90
Range=5.75
Projectile=Cannon
Warhead=AP
[InvisibleAll]
Inviso=yes
[Invisible]
Inviso=yes
[Cannon]
AG=yes
[BombDisarm]
BombDisarm=yes
[IvanBomb]
IvanBomb=yes
[AP]
Verses=100%,100%,90%,75%,50%,50%,100%,50%,25%,100%,100%
",
        ))
        .unwrap();
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/bomb_class.json"
        ))
        .unwrap();
        let engineer = rules.object("ENGINEER").unwrap();
        let tank = rules.object("HTNK").unwrap();
        let mut compared = 0;
        for case in cases
            .iter()
            .filter(|case| case["input"]["section"] == "fire_error")
        {
            let input = &case["input"];
            let flag = |key: &str| input[key].as_bool() == Some(true);
            let weapon = rules
                .weapon(if flag("bomb_disarm") {
                    "DefuseKit"
                } else if flag("ivan_bomb") {
                    "IvanBomber"
                } else {
                    "120mm"
                })
                .unwrap();
            let target = TargetFacts::Techno {
                obj: tank,
                kind: TechnoKind::Unit,
                is_high_flying: false,
                on_bridge: false,
                cell_land_type: LandType::Clear.as_index(),
                submerged: false,
                is_ally: false,
                parasite: ParasiteVictimFacts::INFECTABLE_ON_LAND,
                capture: crate::sim::capture_manager::CaptureVictimFacts::capturable_for_test(),
                has_bomb: flag("bombed"),
            };
            let blocked = targeting_fire_error_blocks(
                &rules,
                engineer,
                weapon,
                warhead_of(&rules, weapon).unwrap(),
                &target,
                None,
            );
            let native = &case["result"];
            assert!(
                (blocked && native == 5) || (!blocked && native == "continue"),
                "{}: {native}",
                input["name"]
            );
            compared += 1;
        }
        assert_eq!(compared, 5);
    }
}
