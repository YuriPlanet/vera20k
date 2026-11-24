//! Object properties past the first set (appendix 7.1), the viewpoint
//! properties (`Relation`, with `Revealed` and `Placeable` in [`super::cells`])
//! and type and rules properties (7.7), each read through its owner.
//!
//! | Property | Owner |
//! | --- | --- |
//! | `Layer` | `Simulation::entity_display_layer` (the In_Which_Layer dispatch), native layer numbers 0..4 |
//! | `LandType` | the object's cell, as the cell property reads it |
//! | `Target` | TarCom (`TechnoClass+0x2B4`): an object or cell reference, 0 for none |
//! | `Experience` | the VeterancyClass value (`+0x150`) in percent: 100 is veteran, 200 elite |
//! | `Storage` | the miner's hold, or a slave's, in bales (`StorageClass` amount) |
//! | `Transport` | the carrier relation (`PassengerRole::inside_transport_id`): transport or garrisoned building |
//! | `PassengerCount` | the cargo count (`+0x114`) plus a building's garrison occupants |
//! | `TransportSpaceLeft` | `Passengers=` less the cargo's total `Size=` (CargoTotalSize); a garrison's `MaxNumberOccupants=` less its occupants |
//! | `Team`, `Tag` | the team VM's member table and the trigger runtime's attachments: 0 without one |
//! | `DisguiseType`, `DisguiseOwner` | the disguise runtime, 0 when undisguised |
//! | `Moving` | the locomotor's Is_Moving (`motion_query::is_moving`) |
//! | `Facing`, `TurretFacing` | the body facing (`+0x388`) and the turret's (`+0x370`; a building aims its body), high byte as map facings store them |
//! | `Armor` | the type's bound armor index |
//! | `Relation` | `HouseAlliances::is_allied` of the viewpoint house, `multiplay_passive` for Neutral |
//! | `Destination` | NavCom (`Foot+0x5A4`, `NavigationState::nav_com`): a cell or object reference, 0 for none (always for buildings) |
//! | `WeaponReady` | the rearm timer (`RearmTimer`) has run out; while it runs GetFireError answers Rearm (`0x006FC94F`) |
//! | `Deployed` | the native deployed state (`GameEntity::is_fully_deployed`: Unit `+0x6E0`, infantry Doing 28..30) |
//! | `UnderConstruction` | a building whose BState (`Building+0x534`) is 0, the build-up animation |
//! | `Parasite` | ParasiteEatingMe (`Foot+0x694`): the object whose parasite holds it, 0 for none |
//! | `ControlGroup` | `Unsupported`: client only |
//! | `Visible` | `Simulation::object_seen_by` for the viewpoint house: fog (`FogState::shows_objects_to`, current sight under FogOfWar) and the screen GetVisualState (`0x00703860`) for cloaking and sensors |
//! | `Engage` | `Simulation::engage_check` for the viewpoint object: GetFireError without range (vt+0x3BC) of the weapon SelectWeapon picks, that Warhead's Verses against the target's armor, and CanFireAt's InRange (`0x006F77B0`) |
//! | House `Factory.<Kind>.*` | `FactoryRegistry::owner_view`: the House slot, else the first building-held factory of the category |
//!
//! Type references read the rules' parsed values: TechnoType `Category`,
//! `Strength`, `Cost`, `Soylent`, `Speed` (INI percent), `ROT`, `Crushable`,
//! `Selfhealing`, `Explodes`, `Power`, `Sight`, `Ammo`, `InitialAmmo`,
//! `Storage`, `Passengers`, `SizeLimit`, `TechLevel`, `Armor`; Weapon
//! `Damage`, `ROF`, `Burst`, `MinimumRange`, `Range` (leptons); Warhead
//! `CellSpread` (leptons, truncated), `PercentAtMax` (fixed, truncated),
//! `Wall`, `Wood`, `Sparky`, `Fire`, `Conventional`, `Rocker`, `InfDeath`.
//! All are read-only during a match (the in-match allowlist is empty).
//!
//! A query's `viewpoint` may be an object: `Engage` reads for it, the house
//! viewpoint properties for its owner. Filter clauses keep the trigger house
//! (no object), so `Engage` there answers `Unsupported`.
//!
//! Residuals:
//! - `Engage` takes the target type's own `Armor=` for the Verses test (no
//!   `TargetArmor` override, which is preview). A transient GetFireError
//!   answer (Rearm, Facing, Moving, Busy, Cloaked, MustDeploy, Ammo) reads
//!   as `Ok` when in range, and hides an Illegal check GetFireError makes
//!   after it.
//! - `Visible` follows the translucent fog's rule (current sight under
//!   FogOfWar), the same for every client; retail rendering under FogOfWar
//!   darkens but still draws objects on explored cells. A tunnel unit's
//!   underground ripple counts as visible.
//! - `Team` and `Tag` answer `Unsupported` for an object in a team or with a
//!   tag: teams and map tags have no references yet (teams number from their
//!   own counter, not the creation counter).
//! - `DisguiseType` answers `Unsupported` for a Mirage disguised as terrain,
//!   which has no type reference.
//! - `Cloaked`, `NearbyEnemyCount`, `TargetInRange`, `CanFireAtTarget`,
//!   `DoorOpen` and `Animating` (all `v1?`) are not read; nor are `Name`
//!   (string IDs) and `Buildable` (needs the prerequisite owner's answer for
//!   a viewpoint house). TechnoType `Points` is not parsed by the rules.
//! - `Experience` converts the float with one f32-to-f64 widening and a
//!   multiply by 100 (exact IEEE operations, truncated); it is a report, not
//!   a value the simulation computes with.

use vera20k_wasm_ext::{
    ENGAGE_ILLEGAL, ENGAGE_NO_DAMAGE, ENGAGE_OK, ENGAGE_OUT_OF_RANGE, Invalid, NAME_TECHNO_TYPE,
    NAME_WARHEAD, NAME_WEAPON, Query, SCOPE_CELLS, Status,
};

use super::{Host, Ref, cell_ref, decode_ref, house_ref, techno_type, type_ref};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::fire_error::FireError;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;

/// `Relation` values (appendix 8.3, enum 9).
const RELATION_SELF: i64 = 0;
const RELATION_ALLY: i64 = 1;
const RELATION_ENEMY: i64 = 2;
const RELATION_NEUTRAL: i64 = 3;

/// Whom a viewpoint property is read for (appendix 7, "Viewpoint"): the
/// trigger house in a filter clause, the query's `viewpoint` in `find`, no
/// one in a direct `get` (the property then answers `Unsupported`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct View {
    /// The viewpoint house's array index: the query's house, or its
    /// viewpoint object's owner.
    pub(super) house: Option<usize>,
    /// The query's viewpoint object, which object viewpoint properties
    /// (`Engage`) read for.
    pub(super) object: Option<u64>,
    /// The query's TechnoType ID, which `Placeable` places (cells queries
    /// only).
    pub(super) placing: Option<u32>,
}

impl View {
    /// A filter clause's view: the trigger house.
    pub(super) fn of_trigger_house(house: Option<u32>) -> Self {
        Self {
            house: house.map(|index| index as usize),
            object: None,
            placing: None,
        }
    }
}

impl Host<'_, '_> {
    /// A query's view: its `viewpoint`, which must be 0, a house or an
    /// object (whose owner is then the viewpoint house; a gone object leaves
    /// the house unset), and for a cells query its TechnoType.
    pub(super) fn query_view(&self, query: &Query) -> Result<View, Invalid> {
        let (house, object) = match decode_ref(query.viewpoint)? {
            None => (None, None),
            Some(Ref::House(index)) => (Some(index), None),
            Some(Ref::Object(id)) => (
                self.sim
                    .entities()
                    .get(id)
                    .and_then(|entity| self.sim.houses.index_of(entity.owner())),
                Some(id),
            ),
            Some(_) => {
                return Err(Invalid(format!(
                    "viewpoint {:#x} is not a house or an object",
                    query.viewpoint
                )));
            }
        };
        let placing = match decode_ref(query.type_ref)? {
            Some(Ref::Type {
                kind: NAME_TECHNO_TYPE,
                id,
            }) if query.scope == SCOPE_CELLS => Some(id),
            _ => None,
        };
        Ok(View {
            house,
            object,
            placing,
        })
    }

    /// The viewpoint house; `Unsupported` without one, `Gone` past the
    /// houses.
    pub(super) fn viewpoint(&self, view: View) -> Result<InternedId, Status> {
        let index = view.house.ok_or(Status::Unsupported)?;
        self.sim
            .houses
            .order()
            .get(index)
            .copied()
            .ok_or(Status::Gone)
    }

    /// `Relation` of house `owner` to the viewpoint house: itself, an ally
    /// (the viewpoint house's IsAlliedWith `0x004F9A50`), a passive house
    /// (civilians), or an enemy.
    pub(super) fn relation(&self, owner: InternedId, view: View) -> (i64, Status) {
        let viewer = match self.viewpoint(view) {
            Ok(viewer) => viewer,
            Err(status) => return (0, status),
        };
        let value = if owner == viewer {
            RELATION_SELF
        } else if self
            .sim
            .house_alliances
            .is_allied(&self.sim.interner, viewer, owner)
        {
            RELATION_ALLY
        } else if self
            .sim
            .houses
            .get(&owner)
            .is_some_and(|house| house.multiplay_passive())
        {
            RELATION_NEUTRAL
        } else {
            RELATION_ENEMY
        };
        (value, Status::Ok)
    }

    /// The appendix 7.1 properties `object_property` leaves to this module.
    pub(super) fn more_object_property(&self, entity: &GameEntity, name: &str) -> (i64, Status) {
        let object = || self.sim.entity_object_type(entity, self.rules);
        let frame = self.sim.session.binary_frame();
        let id = entity.stable_id();
        let value = match name {
            "Layer" => match self.sim.entity_display_layer(id, self.rules) {
                Some(layer) => i64::from(layer.index()),
                None => return (0, Status::Gone),
            },
            "LandType" => {
                return self.cell_property(
                    entity.position.rx(),
                    entity.position.ry(),
                    name,
                    View::default(),
                );
            }
            "Target" => match entity.attack_target.target() {
                None => 0,
                Some(TargetKind::Entity(target)) => target as i64,
                Some(TargetKind::Cell(x, y)) => cell_ref(x, y),
            },
            "Experience" => {
                let raw = f32::from_bits(entity.veterancy.raw().bits());
                (f64::from(raw) * 100.0) as i64
            }
            "Storage" => match entity.miner.as_ref() {
                Some(miner) => miner.cargo().len() as i64,
                None => entity.seldom.slave.cargo().len() as i64,
            },
            "Transport" => entity
                .passenger_role()
                .inside_transport_id()
                .map_or(0, |transport| transport as i64),
            "PassengerCount" => {
                let role = entity.passenger_role();
                i64::from(role.passenger_count()) + i64::from(role.occupants().count().max(0))
            }
            "TransportSpaceLeft" => {
                let role = entity.passenger_role();
                let cargo = role
                    .cargo()
                    .filter(|cargo| cargo.capacity() > 0)
                    .map(|cargo| i64::from(cargo.capacity()) - i64::from(cargo.total_size()));
                let garrison = || {
                    let occupants = object()
                        .filter(|_| entity.category() == EntityCategory::Structure)
                        .map_or(0, |o| o.max_number_occupants);
                    i64::from(occupants) - i64::from(role.occupants().count().max(0))
                };
                cargo.unwrap_or_else(garrison).max(0)
            }
            "Team" => match self.sim.team_script_vm.team_for_member(id) {
                None => 0,
                Some(_) => return (0, Status::Unsupported),
            },
            "Tag" => match self.sim.trigger_runtime.object_tag(id) {
                None => 0,
                Some(_) => return (0, Status::Unsupported),
            },
            "DisguiseType" | "DisguiseOwner" => {
                let Some(disguise) = entity
                    .seldom
                    .disguise
                    .as_ref()
                    .filter(|disguise| disguise.is_disguised())
                else {
                    return (0, Status::Ok);
                };
                if name == "DisguiseOwner" {
                    disguise
                        .house()
                        .and_then(|house| self.sim.houses.index_of(house))
                        .map_or(0, house_ref)
                } else {
                    match disguise
                        .type_id()
                        .and_then(|ty| self.sim.interner.type_handle(ty, self.rules))
                    {
                        Some(handle) => type_ref(NAME_TECHNO_TYPE, handle.0),
                        None => return (0, Status::Unsupported),
                    }
                }
            }
            "Moving" => {
                i64::from(crate::sim::movement::motion_query::is_moving(entity) == Some(true))
            }
            "Facing" => i64::from(entity.body_facing_byte(frame)),
            "TurretFacing" => {
                if !object().is_some_and(|o| o.has_turret) {
                    return (0, Status::Unsupported);
                }
                match (&entity.barrel_facing, entity.category()) {
                    (_, EntityCategory::Structure) => i64::from(entity.body_facing_byte(frame)),
                    (Some(turret), _) => i64::from(turret.current(frame) >> 8),
                    (None, _) => return (0, Status::Unsupported),
                }
            }
            "Armor" => match object() {
                Some(o) => o.armor_id().index() as i64,
                None => return (0, Status::NotFound),
            },
            "Destination" => match entity.navigation.nav_com() {
                None => 0,
                Some(NavTargetRef::Cell { rx, ry }) => cell_ref(rx, ry),
                Some(NavTargetRef::Object { id }) => id as i64,
            },
            "WeaponReady" => i64::from(entity.seldom.rearm_timer.remaining(frame as i32) == 0),
            "Deployed" => i64::from(entity.is_fully_deployed()),
            "UnderConstruction" => i64::from(
                entity.category() == EntityCategory::Structure && entity.in_construction_bstate(),
            ),
            "Parasite" => entity.parasite.eater().map_or(0, |eater| eater as i64),
            // The local player's control group exists only on the client.
            "ControlGroup" => return (0, Status::Unsupported),
            _ => return (0, Status::Unsupported),
        };
        (value, Status::Ok)
    }

    /// `Visible`: the viewpoint house sees the object now, as its screen
    /// shows it (`Simulation::object_seen_by`).
    pub(super) fn visible(&self, entity: &GameEntity, view: View) -> (i64, Status) {
        match self.viewpoint(view) {
            Ok(viewer) => (
                i64::from(self.sim.object_seen_by(self.rules, entity, viewer)),
                Status::Ok,
            ),
            Err(status) => (0, status),
        }
    }

    /// `Engage` (`EngageResult`) of the viewpoint object as the shooter
    /// against object `target` (`Simulation::engage_check`): GetFireError's
    /// Illegal and Cant are `Illegal`; a Warhead with Verses 0 against the
    /// target's armor is `NoDamage`; out of InRange is `OutOfRange`; else
    /// `Ok` (the other GetFireError answers, rearming, facing, moving, busy
    /// and the like, pass with time). `Unsupported` without a viewpoint
    /// object, `Gone` once the shooter is gone.
    pub(super) fn engage(&self, target: u64, view: View) -> (i64, Status) {
        let Some(shooter) = view.object else {
            return (0, Status::Unsupported);
        };
        if self.sim.entities().get(shooter).is_none() {
            return (0, Status::Gone);
        }
        let (error, no_damage, in_range) = self.sim.engage_check(self.rules, shooter, target);
        let value = match error {
            FireError::Illegal | FireError::Cant => ENGAGE_ILLEGAL,
            _ if no_damage => ENGAGE_NO_DAMAGE,
            _ if !in_range => ENGAGE_OUT_OF_RANGE,
            _ => ENGAGE_OK,
        };
        (value, Status::Ok)
    }

    /// A house's `Factory.<Kind>.*` property (appendix 7.5) from the factory
    /// producing that kind for it (`FactoryRegistry::owner_view`): `Type`
    /// the object in production, `Progress` its stage (`+0x24`, 54 steps)
    /// in percent, `Ready` IsComplete (`0x004CA130`), `Queued` the object in
    /// production plus the entries queued behind it. 0 for each without a
    /// factory.
    pub(super) fn factory_property(&self, house: InternedId, name: &str) -> (i64, Status) {
        use crate::sim::production::{PRODUCTION_STEPS, ProductionCategory};
        let Some((kind, field)) = name
            .strip_prefix("Factory.")
            .and_then(|rest| rest.split_once('.'))
        else {
            return (0, Status::Unsupported);
        };
        let category = match kind {
            "Building" => ProductionCategory::Building,
            "Defense" => ProductionCategory::Defense,
            "Infantry" => ProductionCategory::Infantry,
            "Vehicle" => ProductionCategory::Vehicle,
            "Naval" => ProductionCategory::Ship,
            "Aircraft" => ProductionCategory::Aircraft,
            _ => return (0, Status::Unsupported),
        };
        if !matches!(field, "Type" | "Progress" | "Ready" | "Queued") {
            return (0, Status::Unsupported);
        }
        let Some(factory) = self.sim.production.factories().owner_view(house, category) else {
            return (0, Status::Ok);
        };
        let value = match field {
            "Type" => match factory.object {
                None => 0,
                Some(object) => match self.sim.interner.type_handle(object.type_id, self.rules) {
                    Some(handle) => type_ref(NAME_TECHNO_TYPE, handle.0),
                    None => return (0, Status::NotFound),
                },
            },
            "Progress" if factory.object.is_some() => {
                i64::from(factory.progress.min(PRODUCTION_STEPS)) * 100
                    / i64::from(PRODUCTION_STEPS)
            }
            "Progress" => 0,
            "Ready" => i64::from(factory.complete_object().is_some()),
            _ => i64::from(factory.object.is_some()) + factory.queue.len() as i64,
        };
        (value, Status::Ok)
    }
}

/// `Category` values (appendix 8.3, enum 10) of a TechnoType.
const fn type_category(category: ObjectCategory) -> i64 {
    match category {
        ObjectCategory::Infantry => 0,
        ObjectCategory::Vehicle => 1,
        ObjectCategory::Aircraft => 2,
        ObjectCategory::Building => 3,
    }
}

/// A type's property (appendix 7.7). A wildcard or unknown ID is invalid;
/// a name kind without properties answers `Unsupported`.
pub(super) fn type_property(
    rules: &RuleSet,
    kind: i32,
    id: u32,
    name: &str,
) -> Result<(i64, Status), Invalid> {
    let value = match kind {
        NAME_TECHNO_TYPE => {
            let object = i32::try_from(id)
                .ok()
                .and_then(|id| techno_type(rules, id))
                .ok_or_else(|| Invalid(format!("no TechnoType {id}")))?;
            match name {
                "Category" => type_category(object.category),
                "Strength" => i64::from(object.strength),
                "Cost" => i64::from(object.cost),
                "Soylent" => i64::from(object.soylent),
                "Speed" => i64::from(object.speed),
                "ROT" => i64::from(object.turret_rot),
                "Crushable" => i64::from(object.crushable),
                "Selfhealing" => i64::from(object.self_healing),
                "Explodes" => i64::from(object.explodes),
                "Power" => i64::from(object.power),
                "Sight" => i64::from(object.sight),
                "Ammo" => i64::from(object.ammo),
                "InitialAmmo" => i64::from(object.initial_ammo),
                "Storage" => i64::from(object.storage),
                "Passengers" => i64::from(object.passengers),
                // `SizeLimit=` is a double; the property is its whole part.
                "SizeLimit" => object.size_limit as i64,
                "TechLevel" => i64::from(object.tech_level),
                "Armor" => object.armor_id().index() as i64,
                _ => return Ok((0, Status::Unsupported)),
            }
        }
        NAME_WEAPON => {
            let weapon = rules
                .weapon_at(id)
                .ok_or_else(|| Invalid(format!("no Weapon {id}")))?;
            match name {
                "Damage" => i64::from(weapon.damage),
                "ROF" => i64::from(weapon.rof),
                "Burst" => i64::from(weapon.burst),
                "MinimumRange" => i64::from(weapon.minimum_range_leptons),
                "Range" => i64::from(weapon.range_leptons),
                _ => return Ok((0, Status::Unsupported)),
            }
        }
        NAME_WARHEAD => {
            let warhead = rules
                .warhead_at(id)
                .ok_or_else(|| Invalid(format!("no Warhead {id}")))?;
            match name {
                // Cells as leptons and the fraction as `fixed`, truncated.
                "CellSpread" => (warhead.cell_spread_f64 * 256.0) as i64,
                "PercentAtMax" => (warhead.percent_at_max_f64 * 65536.0) as i64,
                "Wall" => i64::from(warhead.wall),
                "Wood" => i64::from(warhead.wood),
                "Sparky" => i64::from(warhead.sparky),
                "Fire" => i64::from(warhead.fire),
                "Conventional" => i64::from(warhead.conventional),
                "Rocker" => i64::from(warhead.rocker),
                "InfDeath" => i64::from(warhead.inf_death),
                _ => return Ok((0, Status::Unsupported)),
            }
        }
        _ => return Ok((0, Status::Unsupported)),
    };
    Ok((value, Status::Ok))
}
