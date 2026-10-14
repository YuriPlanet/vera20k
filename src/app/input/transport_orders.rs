//! Deploy-key / self-click resolution for passenger transports.
//!
//! In gamemd the deploy action on a vehicle or aircraft that carries
//! passengers is the Unload mission: `UnitClass::Mission_Unload @ 0x0073D630`
//! takes its transport branch on `Type+0x5E0 Passengers > 0` and the Aircraft
//! Unload slot `0x004151E0` serves the Nighthawk. Every input site that turns
//! the deploy key or a self-click into a command goes through this one
//! resolver so the three sites cannot disagree on which objects unload.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;

/// The unload command for a vehicle/aircraft transport with passengers aboard,
/// or `None` when the entity is not such a transport or its hold is empty.
pub(crate) fn transport_unload_command(
    entity: &GameEntity,
    obj: Option<&ObjectType>,
) -> Option<Command> {
    if !matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Aircraft
    ) {
        return None;
    }
    if !obj.is_some_and(|o| o.passengers > 0) {
        return None;
    }
    if !entity.passenger_role.cargo().is_some_and(|c| !c.is_empty()) {
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
             [BuildingTypes]\n\n[E1]\nStrength=125\nSize=1\n\n[BFRT]\nStrength=600\n\
             Passengers=5\nOpenTopped=yes\n\n[MTNK]\nStrength=300\n\n[SHAD]\nStrength=200\n\
             Passengers=5\nLandable=yes\n",
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
            transport_unload_command(&bfrt, rules.object("BFRT")),
            Some(Command::UnloadPassengers { transport_id: 7 })
        );
        let shad = entity(8, "SHAD", EntityCategory::Aircraft, true);
        assert_eq!(
            transport_unload_command(&shad, rules.object("SHAD")),
            Some(Command::UnloadPassengers { transport_id: 8 })
        );
    }

    #[test]
    fn deploy_key_ignores_empty_holds_and_non_transports() {
        let rules = rules();
        let empty = entity(7, "BFRT", EntityCategory::Unit, false);
        assert_eq!(transport_unload_command(&empty, rules.object("BFRT")), None);
        let tank = entity(9, "MTNK", EntityCategory::Unit, true);
        assert_eq!(transport_unload_command(&tank, rules.object("MTNK")), None);
        let mut building = entity(10, "BFRT", EntityCategory::Structure, true);
        building.set_category_for_test(EntityCategory::Structure);
        assert_eq!(
            transport_unload_command(&building, rules.object("BFRT")),
            None
        );
    }
}
