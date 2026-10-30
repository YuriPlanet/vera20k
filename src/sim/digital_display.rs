//! Phobos DigitalDisplay value projection.
//!
//! `TechnoExt::GetValuesForDisplay` (`src/Ext/Techno/Body.Visuals.cpp`)
//! resolves each `DisplayInfoType` from live Techno state. This module is the
//! single Rust owner of that mapping: presentation asks for `(value, maximum)`
//! and never reaches into another module's private fields. It only reads;
//! it never advances timers, draws RNG or writes state.
//!
//! Two native info types have no owner in VERA and therefore return `None`:
//! `PassengerKill` (Phobos passenger deletion) and `AutoDeath` (Phobos
//! auto-death). A display whose value is absent draws nothing, matching
//! Phobos's `value <= -1 || maxValue <= 0` skip.

use crate::rules::digital_display::DigitalInfoType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::VeterancyRank;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

/// Phobos `GetValuesForDisplay` for one display entry. `value < 0` or
/// `maximum <= 0` means "do not draw".
pub fn values_for_display(
    sim: &Simulation,
    entity: &GameEntity,
    object: &crate::rules::object_type::ObjectType,
    rules: &RuleSet,
    info_type: DigitalInfoType,
    info_index: i32,
    viewer: Option<InternedId>,
) -> Option<(i32, i32)> {
    let frame = sim.session().binary_frame() as i32;
    let value = match info_type {
        DigitalInfoType::Health => {
            let mut value = entity.health.current();
            let mut maximum = object.strength;
            if entity.disguise.as_ref().is_some_and(|d| d.is_disguised())
                && !viewer.is_some_and(|viewer| {
                    crate::map::houses::is_allied_with(
                        sim.house_alliances(),
                        sim.interner().resolve(entity.owner()),
                        sim.interner().resolve(viewer),
                    )
                })
                && object
                    .digital_display_health_fake_at_disguise()
                    .unwrap_or(rules.digital_display_globals().health_fake_at_disguise)
                && let Some(disguise_type) = entity
                    .disguise
                    .as_ref()
                    .and_then(|d| d.type_id())
                    .and_then(|type_id| sim.object_type(type_id, rules))
            {
                let new_max = disguise_type.strength;
                let ratio = f64::from(value) / f64::from(maximum);
                value = (ratio * f64::from(new_max)) as i32;
                maximum = new_max;
            }
            (value, maximum)
        }
        DigitalInfoType::Shield => {
            let shield = entity.shield()?;
            let ty = rules.shield_type(shield.type_name())?;
            if shield.broken_non_respawning(ty) {
                return None;
            }
            (shield.hp(), ty.strength())
        }
        DigitalInfoType::Ammo => (entity.ammo()?.current(), object.ammo),
        DigitalInfoType::MindControl => {
            let manager = entity.capture_manager.as_ref()?;
            let count = sim
                .capture_effective_count(entity.stable_id(), rules)
                .unwrap_or_else(|| manager.count());
            (count, manager.max_control())
        }
        DigitalInfoType::Spawns => {
            let manager = entity.spawn_manager.as_ref()?;
            if object.spawns.is_none() || object.spawns_number <= 0 {
                return None;
            }
            let value = match info_index {
                1 => manager.count_docked_spawns(),
                2 => manager.count_launching_spawns(),
                _ => manager.count_alive_spawns(),
            };
            (value as i32, object.spawns_number)
        }
        DigitalInfoType::Passengers => (
            entity
                .passenger_role
                .cargo()
                .map_or(0, |cargo| cargo.count()) as i32,
            object.passengers,
        ),
        DigitalInfoType::Tiberium => {
            let bales = entity.storage_bales();
            let amount = if info_index > 0 && info_index as usize <= rules.tiberium_types.len() {
                let wanted = info_index - 1;
                bales
                    .iter()
                    .filter(|bale| i32::from(bale.tiberium_type.0) == wanted)
                    .count()
            } else {
                bales.len()
            };
            (amount as i32, object.storage)
        }
        DigitalInfoType::Experience => {
            let raw = f32::from_bits(entity.veterancy.raw().bits());
            let per_point = rules.general.veteran_ratio * f64::from(object.cost);
            (
                (f64::from(raw) * per_point) as i32,
                (2.0 * per_point) as i32,
            )
        }
        DigitalInfoType::Occupants => {
            if entity.category() != crate::map::entities::EntityCategory::Structure
                || !object.can_be_occupied
            {
                return None;
            }
            (
                entity.passenger_role.occupants().count(),
                i32::try_from(object.max_number_occupants).unwrap_or(i32::MAX),
            )
        }
        DigitalInfoType::GattlingStage => {
            if !object.is_gattling {
                return None;
            }
            (
                if entity.gattling.value() != 0 {
                    entity.gattling.stage() + 1
                } else {
                    0
                },
                object.gattling_stages.weapon_stages(),
            )
        }
        DigitalInfoType::Rof => {
            if !crate::sim::combat::combat_weapon::is_armed(entity, object) {
                return None;
            }
            (
                entity.rearm_timer.remaining(frame),
                entity.rearm_timer.duration(),
            )
        }
        DigitalInfoType::Reload => {
            if object.ammo <= 0 {
                return None;
            }
            let ammo = entity.ammo().map_or(0, |ammo| ammo.current());
            let timer = entity.ammo_reload_timer();
            let maximum = if timer.duration() != 0 {
                timer.duration()
            } else if ammo != 0 || object.empty_reload <= 0 {
                object.reload
            } else {
                object.empty_reload
            };
            let value = if ammo >= object.ammo {
                0
            } else {
                timer.remaining(frame)
            };
            (value, maximum)
        }
        DigitalInfoType::SpawnTimer => {
            let manager = entity.spawn_manager.as_ref()?;
            if object.spawns.is_none() || object.spawns_number <= 0 {
                return None;
            }
            let slots = manager.slots();
            let value = if info_index > 0 && info_index as usize <= slots.len() {
                slots[info_index as usize - 1].timer().remaining(frame)
            } else {
                // Phobos scans `SpawnNodeStatus::Dead` nodes and keeps the
                // smallest nonzero remaining; 0 means "no dead node".
                slots
                    .iter()
                    .filter(|slot| {
                        slot.state() == crate::sim::spawn_manager::SpawnSlotState::Regenerating
                    })
                    .map(|slot| slot.timer().remaining(frame))
                    .fold(
                        0,
                        |best, time| {
                            if best == 0 || time < best { time } else { best }
                        },
                    )
            };
            (value, manager.regen_rate() as i32)
        }
        DigitalInfoType::GattlingTimer => {
            if !object.is_gattling {
                return None;
            }
            let stage = entity.gattling.stage();
            let elite = entity.veterancy.rank_kind() == VeterancyRank::Elite;
            let mut value = entity.gattling.value();
            let mut maximum = object.gattling_stages.threshold(stage, elite);
            if stage > 0 {
                let previous = object.gattling_stages.threshold(stage - 1, elite);
                value -= previous;
                maximum -= previous;
            }
            (value, maximum)
        }
        DigitalInfoType::ProduceCash => {
            if entity.category() != crate::map::entities::EntityCategory::Structure
                || object.produce_cash_amount <= 0
            {
                return None;
            }
            let timer = entity.produce_cash.timer();
            (timer.remaining(frame), timer.duration())
        }
        // No Phobos PassengerDeletion or AutoDeath owner exists in VERA.
        DigitalInfoType::PassengerKill | DigitalInfoType::AutoDeath => return None,
        DigitalInfoType::SuperWeapon => {
            if entity.category() != crate::map::entities::EntityCategory::Structure {
                return None;
            }
            // `SuperWeapons=` is the Ares list; a Phobos-only build has the
            // two stock slots and no additional entries.
            #[cfg(feature = "ares")]
            let additional = object.additional_super_weapons();
            #[cfg(not(feature = "ares"))]
            let additional: &[String] = &[];
            // `getSuperTimer`: an index within `GetSuperWeaponCount()` (the two
            // stock slots plus the list) selects that slot, even an empty one;
            // zero or an index past the end falls back to the first set slot.
            let count = 2 + additional.len();
            let selected = if info_index > 0 && info_index as usize <= count {
                match info_index {
                    1 => object.super_weapon.as_deref(),
                    2 => object.super_weapon2.as_deref(),
                    _ => additional.get(info_index as usize - 3).map(String::as_str),
                }
            } else {
                object
                    .super_weapon
                    .as_deref()
                    .or(object.super_weapon2.as_deref())
                    .or_else(|| additional.first().map(String::as_str))
            };
            let name = selected?;
            let type_id = sim.interner().get(name)?;
            let instance = sim.super_weapons().get(&entity.owner())?.get(&type_id)?;
            (instance.charge_remaining(frame), instance.charge_duration())
        }
        DigitalInfoType::IronCurtain => {
            // `IsIronCurtained` (`0x0041BF40`) only tests the timer, so a Force
            // Shield counts too.
            let state = entity.invulnerability.as_ref()?;
            if !crate::sim::superweapon::invulnerability::is_invulnerable(Some(state), frame as u32)
            {
                return None;
            }
            (state.timer().remaining(frame), state.timer().duration())
        }
        DigitalInfoType::TemporalLife => {
            // `TemporalTargetingMe` names the chain head; `WarpRemaining`
            // lives on that attacker's own TemporalClass link.
            let head = entity.temporal.chain_head()?;
            let attacker = sim.entities().get(head)?;
            let remaining = attacker.temporal.warp_remaining()?;
            (remaining, object.strength.wrapping_mul(10))
        }
        DigitalInfoType::FactoryProcess => {
            if entity.category() != crate::map::entities::EntityCategory::Structure {
                return None;
            }
            // Phobos index 2 selects the human player's separate defense
            // primary; VERA has one primary per factory category, so only the
            // building's own factory is shown.
            if info_index == 2 {
                return None;
            }
            if sim
                .houses()
                .get(&entity.owner())
                .is_some_and(|house| house.is_human())
                && !sim.production().is_primary_factory(entity.stable_id())
            {
                // Human index 0 natively reads the house's primary factory
                // even from a secondary building. This chain keeps the
                // building's own factory and leaves that lookup open.
                return None;
            }
            let factory = sim
                .production()
                .factories()
                .building_factory(entity.stable_id())?;
            factory.object()?;
            (i32::from(factory.progress()), 54)
        }
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    fn fixture(ini: &str) -> (Simulation, RuleSet) {
        let rules = RuleSet::from_ini(&IniFile::from_str(ini)).expect("fixture rules");
        let mut sim = Simulation::with_seed(1);
        let owner = sim.interner.intern("Neutral");
        let type_ref = sim.interner.intern("TEST");
        let mut entity = GameEntity::test_default(1, "TEST", "Neutral", 10, 10);
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(type_ref);
        entity.health.set_for_test(100);
        sim.substrate.entities.insert(entity);
        (sim, rules)
    }

    const VEHICLE: &str = "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=200\nAmmo=10\nReload=100\n";

    #[test]
    fn health_and_ammo_project_the_entity_and_type_values() {
        let (mut sim, rules) = fixture(VEHICLE);
        let type_ref = sim.entities().get(1).unwrap().type_ref();
        let object = sim
            .interner()
            .object_type(type_ref, &rules)
            .unwrap()
            .clone();
        let entity = sim.entities().get(1).unwrap();
        assert_eq!(
            values_for_display(
                &sim,
                entity,
                &object,
                &rules,
                DigitalInfoType::Health,
                0,
                None
            ),
            Some((100, 200))
        );
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.install_ammo_for_test(Some(crate::sim::ammo::TechnoAmmo::new(10)));
        entity.ammo_mut().unwrap().add(-6);
        let entity = sim.entities().get(1).unwrap();
        assert_eq!(
            values_for_display(
                &sim,
                entity,
                &object,
                &rules,
                DigitalInfoType::Ammo,
                0,
                None
            ),
            Some((4, 10))
        );
    }

    #[test]
    fn absent_owners_produce_no_display() {
        let (sim, rules) = fixture(VEHICLE);
        let type_ref = sim.entities().get(1).unwrap().type_ref();
        let object = sim
            .interner()
            .object_type(type_ref, &rules)
            .unwrap()
            .clone();
        let entity = sim.entities().get(1).unwrap();
        for info_type in [
            DigitalInfoType::Shield,
            DigitalInfoType::MindControl,
            DigitalInfoType::Spawns,
            DigitalInfoType::Occupants,
            DigitalInfoType::ProduceCash,
            DigitalInfoType::PassengerKill,
            DigitalInfoType::AutoDeath,
            DigitalInfoType::SuperWeapon,
            DigitalInfoType::IronCurtain,
            DigitalInfoType::TemporalLife,
            DigitalInfoType::FactoryProcess,
        ] {
            assert_eq!(
                values_for_display(&sim, entity, &object, &rules, info_type, 0, None),
                None,
                "{info_type:?}"
            );
        }
    }

    #[test]
    fn zero_maximum_is_kept_for_the_renderer_to_skip() {
        let (sim, rules) = fixture("[VehicleTypes]\n0=TEST\n[TEST]\nStrength=0\nAmmo=0\n");
        let type_ref = sim.entities().get(1).unwrap().type_ref();
        let object = sim
            .interner()
            .object_type(type_ref, &rules)
            .unwrap()
            .clone();
        let entity = sim.entities().get(1).unwrap();
        assert_eq!(
            values_for_display(
                &sim,
                entity,
                &object,
                &rules,
                DigitalInfoType::Health,
                0,
                None
            ),
            Some((100, 0))
        );
    }
}
