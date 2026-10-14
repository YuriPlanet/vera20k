//! Deploy-key / self-click resolution for unloadable passenger containers.
//!
//! Mobile transports use `Type+0x5E0 Passengers > 0` and the aircraft unload
//! slot `0x004151E0`; garrisons use `CanBeOccupied`; InfantryAbsorb buildings
//! use their native absorber flag. Every input site routes through this one
//! resolver so the gates agree.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;

/// One resolver for self-click, deploy-key and cursor unload admission.
/// Structures unload only when they are InfantryAbsorb buildings or
/// occupiable garrisons; mobile transports retain their passenger-count gate.
/// Production callers provide the local owner so an enemy building cannot
/// advertise an unload.
pub(crate) fn unload_command(
    entity: &GameEntity,
    obj: Option<&ObjectType>,
    local_owner: Option<InternedId>,
) -> Option<Command> {
    if local_owner.is_some_and(|owner| owner != entity.owner()) {
        return None;
    }
    let obj = obj?;
    let has_passengers = entity
        .passenger_role
        .cargo()
        .is_some_and(|cargo| !cargo.is_empty());
    let can_unload = match entity.category() {
        EntityCategory::Structure => (obj.can_be_occupied || obj.infantry_absorb) && has_passengers,
        EntityCategory::Unit | EntityCategory::Aircraft => obj.passengers > 0 && has_passengers,
        EntityCategory::Infantry => false,
    };
    if !can_unload {
        return None;
    }
    Some(Command::UnloadPassengers {
        transport_id: entity.stable_id(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};

    fn rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=BFRT\n1=MTNK\n[AircraftTypes]\n0=SHAD\n\
             [BuildingTypes]\n0=YAPOWR\n1=GARRISON\n2=UNITABS\n\n[E1]\nStrength=125\nSize=1\n\n[BFRT]\nStrength=600\n\
             Passengers=5\nOpenTopped=yes\n\n[MTNK]\nStrength=300\n\n[SHAD]\nStrength=200\n\
             Passengers=5\nLandable=yes\n\n[YAPOWR]\nStrength=1000\nPassengers=5\n\
             InfantryAbsorb=yes\n\n[GARRISON]\nStrength=1000\nCanBeOccupied=yes\n\
             Passengers=5\n\n[UNITABS]\nStrength=1000\nPassengers=5\nUnitAbsorb=yes\n\
             \n",
        );
        RuleSet::from_ini(&ini).expect("rules")
    }

    fn entity(id: u64, type_id: &str, category: EntityCategory, loaded: bool) -> GameEntity {
        let mut e = GameEntity::test_default(id, type_id, "Americans", 10, 10);
        e.set_category_for_test(category);
        let mut cargo = PassengerCargo::new(5, 0);
        if loaded {
            cargo.board(99, 1);
        }
        e.passenger_role = PassengerRole::transport(cargo);
        e
    }

    #[test]
    fn deploy_key_on_loaded_apc_issues_unload() {
        let rules = rules();
        let bfrt = entity(7, "BFRT", EntityCategory::Unit, true);
        assert_eq!(
            unload_command(&bfrt, rules.object("BFRT"), None),
            Some(Command::UnloadPassengers { transport_id: 7 })
        );
        let shad = entity(8, "SHAD", EntityCategory::Aircraft, true);
        assert_eq!(
            unload_command(&shad, rules.object("SHAD"), None),
            Some(Command::UnloadPassengers { transport_id: 8 })
        );
    }

    #[test]
    fn deploy_key_ignores_empty_holds_and_non_transports() {
        let rules = rules();
        let empty = entity(7, "BFRT", EntityCategory::Unit, false);
        assert_eq!(unload_command(&empty, rules.object("BFRT"), None), None);
        let tank = entity(9, "MTNK", EntityCategory::Unit, true);
        assert_eq!(unload_command(&tank, rules.object("MTNK"), None), None);
        let building = entity(10, "BFRT", EntityCategory::Structure, true);
        assert_eq!(unload_command(&building, rules.object("BFRT"), None), None);
    }

    #[test]
    fn structure_unload_admission_covers_infantry_absorbers_and_garrisons_only() {
        let rules = rules();
        let reactor = entity(11, "YAPOWR", EntityCategory::Structure, true);
        assert_eq!(
            unload_command(&reactor, rules.object("YAPOWR"), Some(reactor.owner())),
            Some(Command::UnloadPassengers { transport_id: 11 })
        );

        let empty_reactor = entity(12, "YAPOWR", EntityCategory::Structure, false);
        assert_eq!(
            unload_command(&empty_reactor, rules.object("YAPOWR"), None),
            None
        );
        let unit_absorber = entity(13, "UNITABS", EntityCategory::Structure, true);
        assert_eq!(
            unload_command(&unit_absorber, rules.object("UNITABS"), None),
            None
        );
        let garrison = entity(14, "GARRISON", EntityCategory::Structure, true);
        assert_eq!(
            unload_command(&garrison, rules.object("GARRISON"), None),
            Some(Command::UnloadPassengers { transport_id: 14 })
        );
        assert_eq!(
            unload_command(
                &reactor,
                rules.object("YAPOWR"),
                Some(InternedId::from_index(reactor.owner().index() + 1))
            ),
            None
        );
    }
}
