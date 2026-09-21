//! Category-specific raw state consumed by Mission readiness and policy.
//!
//! The native Unit, Infantry, Aircraft, and Building leaves own different
//! bytes. Keeping them in sealed variants prevents those bytes from collapsing
//! into a generic busy flag while still allowing read-only readiness queries.

use crate::map::entities::EntityCategory;

/// Entity-owned Mission state whose layout depends on the concrete Techno
/// family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum MissionLeafState {
    Unit(UnitMissionLeaf),
    Infantry(InfantryMissionLeaf),
    Aircraft(AircraftMissionLeaf),
    Building(BuildingMissionLeaf),
}

/// Unit readiness bytes, stored independently in native declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct UnitMissionLeaf {
    deploy_begin_active: u8,
    deploy_reverse_active: u8,
    tracker_byte_18: u8,
    tracker_byte_19: u8,
}

/// Infantry readiness inputs owned by the firing and Doing authorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InfantryMissionLeaf {
    firing_sequence_latch: u8,
    doing: i32,
}

/// Aircraft policy and readiness bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AircraftMissionLeaf {
    /// Aircraft+6D2: shared by Mission_Attack, Fly and ReadyToCommence41B5E0.
    action_latch: u8,
    /// Aircraft+6D4, independent of the pending-ammunition byte+6C8.
    transition_ready_latch: u8,
    airstrike_manager_present: bool,
}

/// Building reusable mission-ready latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingMissionLeaf {
    ready_latch: u8,
}

/// A Doing value rejected by the verified Infantry writer domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Infantry Doing value is outside the verified writer domain: {0}")]
pub(crate) struct InvalidInfantryDoing(pub(crate) i32);

impl MissionLeafState {
    /// Construct the exact initial leaf for an entity's concrete category.
    pub(crate) const fn for_entity_category(category: EntityCategory) -> Self {
        match category {
            EntityCategory::Unit => Self::Unit(UnitMissionLeaf::initial()),
            EntityCategory::Infantry => Self::Infantry(InfantryMissionLeaf::initial()),
            EntityCategory::Aircraft => Self::Aircraft(AircraftMissionLeaf::initial()),
            EntityCategory::Structure => Self::Building(BuildingMissionLeaf::initial()),
        }
    }

    /// Borrow the Unit inputs without permitting mutation.
    pub(crate) const fn as_unit(&self) -> Option<&UnitMissionLeaf> {
        match self {
            Self::Unit(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// Borrow the Infantry inputs without permitting mutation.
    pub(crate) const fn as_infantry(&self) -> Option<&InfantryMissionLeaf> {
        match self {
            Self::Infantry(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// Borrow the Aircraft inputs without permitting mutation.
    pub(crate) const fn as_aircraft(&self) -> Option<&AircraftMissionLeaf> {
        match self {
            Self::Aircraft(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// Borrow the Building input without permitting mutation.
    pub(crate) const fn as_building(&self) -> Option<&BuildingMissionLeaf> {
        match self {
            Self::Building(leaf) => Some(leaf),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_unit_deploy_begin_active(&mut self, raw: u8) {
        self.expect_unit_mut().deploy_begin_active = raw;
    }

    #[cfg(test)]
    pub(crate) fn set_unit_deploy_reverse_active(&mut self, raw: u8) {
        self.expect_unit_mut().deploy_reverse_active = raw;
    }

    #[cfg(test)]
    pub(crate) fn set_unit_tracker_byte_18(&mut self, raw: u8) {
        self.expect_unit_mut().tracker_byte_18 = raw;
    }

    #[cfg(test)]
    pub(crate) fn set_unit_tracker_byte_19(&mut self, raw: u8) {
        self.expect_unit_mut().tracker_byte_19 = raw;
    }

    pub(crate) fn set_infantry_firing_sequence(&mut self, raw: u8) {
        self.expect_infantry_mut().firing_sequence_latch = raw;
    }

    /// Write only values accepted by the verified 42-entry Doing table.
    pub(crate) fn set_infantry_doing_verified(
        &mut self,
        doing: i32,
    ) -> Result<(), InvalidInfantryDoing> {
        let leaf = self.expect_infantry_mut();
        if doing != -1 && !(0..=41).contains(&doing) {
            return Err(InvalidInfantryDoing(doing));
        }
        leaf.doing = doing;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_aircraft_transition_ready(&mut self, raw: u8) {
        self.expect_aircraft_mut().transition_ready_latch = raw;
    }

    /// Aircraft Commence clears the action latch before the common base call.
    pub(crate) fn clear_aircraft_action_for_commence(&mut self) {
        self.set_aircraft_action_latch(false);
    }

    pub(crate) fn set_aircraft_action_latch(&mut self, active: bool) {
        self.expect_aircraft_mut().action_latch = u8::from(active);
    }

    #[cfg(test)]
    pub(crate) fn set_building_ready_latch(&mut self, raw: u8) {
        self.expect_building_mut().ready_latch = raw;
    }

    #[track_caller]
    #[cfg(test)]
    fn expect_unit_mut(&mut self) -> &mut UnitMissionLeaf {
        match self {
            Self::Unit(leaf) => leaf,
            _ => panic!("Unit Mission leaf writer used for another category"),
        }
    }

    #[track_caller]
    fn expect_infantry_mut(&mut self) -> &mut InfantryMissionLeaf {
        match self {
            Self::Infantry(leaf) => leaf,
            _ => panic!("Infantry Mission leaf writer used for another category"),
        }
    }

    #[track_caller]
    fn expect_aircraft_mut(&mut self) -> &mut AircraftMissionLeaf {
        match self {
            Self::Aircraft(leaf) => leaf,
            _ => panic!("Aircraft Mission leaf writer used for another category"),
        }
    }

    #[track_caller]
    #[cfg(test)]
    fn expect_building_mut(&mut self) -> &mut BuildingMissionLeaf {
        match self {
            Self::Building(leaf) => leaf,
            _ => panic!("Building Mission leaf writer used for another category"),
        }
    }

    #[cfg(test)]
    pub(crate) const fn unit_raw_for_test(
        deploy_begin_active: u8,
        deploy_reverse_active: u8,
        tracker_byte_18: u8,
        tracker_byte_19: u8,
    ) -> Self {
        Self::Unit(UnitMissionLeaf {
            deploy_begin_active,
            deploy_reverse_active,
            tracker_byte_18,
            tracker_byte_19,
        })
    }

    #[cfg(test)]
    pub(crate) const fn infantry_raw_for_test(firing_sequence_latch: u8, doing: i32) -> Self {
        Self::Infantry(InfantryMissionLeaf {
            firing_sequence_latch,
            doing,
        })
    }

    #[cfg(test)]
    pub(crate) const fn aircraft_raw_for_test(
        action_latch: u8,
        transition_ready_latch: u8,
        airstrike_manager_present: bool,
    ) -> Self {
        Self::Aircraft(AircraftMissionLeaf {
            action_latch,
            transition_ready_latch,
            airstrike_manager_present,
        })
    }

    #[cfg(test)]
    pub(crate) const fn building_raw_for_test(ready_latch: u8) -> Self {
        Self::Building(BuildingMissionLeaf { ready_latch })
    }
}

impl UnitMissionLeaf {
    const fn initial() -> Self {
        Self {
            deploy_begin_active: 0,
            deploy_reverse_active: 0,
            tracker_byte_18: 0,
            tracker_byte_19: 0,
        }
    }

    pub(crate) const fn deploy_begin_active(&self) -> u8 {
        self.deploy_begin_active
    }

    pub(crate) const fn deploy_reverse_active(&self) -> u8 {
        self.deploy_reverse_active
    }

    pub(crate) const fn tracker_byte_18(&self) -> u8 {
        self.tracker_byte_18
    }

    pub(crate) const fn tracker_byte_19(&self) -> u8 {
        self.tracker_byte_19
    }
}

impl InfantryMissionLeaf {
    const fn initial() -> Self {
        Self {
            firing_sequence_latch: 0,
            doing: -1,
        }
    }

    pub(crate) const fn firing_sequence_latch(&self) -> u8 {
        self.firing_sequence_latch
    }

    pub(crate) const fn doing(&self) -> i32 {
        self.doing
    }
}

impl AircraftMissionLeaf {
    const fn initial() -> Self {
        Self {
            action_latch: 0,
            transition_ready_latch: 1,
            airstrike_manager_present: false,
        }
    }

    pub(crate) const fn action_latch(&self) -> u8 {
        self.action_latch
    }

    pub(crate) const fn transition_ready_latch(&self) -> u8 {
        self.transition_ready_latch
    }

    pub(crate) const fn airstrike_manager_present(&self) -> bool {
        self.airstrike_manager_present
    }
}

impl BuildingMissionLeaf {
    const fn initial() -> Self {
        Self { ready_latch: 0 }
    }

    pub(crate) const fn ready_latch(&self) -> u8 {
        self.ready_latch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_leaf_category_defaults_are_exact() {
        let unit = MissionLeafState::for_entity_category(EntityCategory::Unit);
        let unit = unit.as_unit().expect("Unit view");
        assert_eq!(unit.deploy_begin_active(), 0);
        assert_eq!(unit.deploy_reverse_active(), 0);
        assert_eq!(unit.tracker_byte_18(), 0);
        assert_eq!(unit.tracker_byte_19(), 0);

        let infantry = MissionLeafState::for_entity_category(EntityCategory::Infantry);
        let infantry = infantry.as_infantry().expect("Infantry view");
        assert_eq!(infantry.firing_sequence_latch(), 0);
        assert_eq!(infantry.doing(), -1);

        let aircraft = MissionLeafState::for_entity_category(EntityCategory::Aircraft);
        let aircraft = aircraft.as_aircraft().expect("Aircraft view");
        assert_eq!(aircraft.action_latch(), 0);
        assert_eq!(aircraft.transition_ready_latch(), 1);
        assert!(!aircraft.airstrike_manager_present());

        let building = MissionLeafState::for_entity_category(EntityCategory::Structure);
        assert_eq!(
            building.as_building().expect("Building view").ready_latch(),
            0
        );
    }

    #[test]
    fn mission_leaf_accessors_do_not_cross_categories() {
        let fixtures = [
            MissionLeafState::for_entity_category(EntityCategory::Unit),
            MissionLeafState::for_entity_category(EntityCategory::Infantry),
            MissionLeafState::for_entity_category(EntityCategory::Aircraft),
            MissionLeafState::for_entity_category(EntityCategory::Structure),
        ];

        assert!(fixtures[0].as_unit().is_some());
        assert!(fixtures[0].as_infantry().is_none());
        assert!(fixtures[0].as_aircraft().is_none());
        assert!(fixtures[0].as_building().is_none());

        assert!(fixtures[1].as_unit().is_none());
        assert!(fixtures[1].as_infantry().is_some());
        assert!(fixtures[1].as_aircraft().is_none());
        assert!(fixtures[1].as_building().is_none());

        assert!(fixtures[2].as_unit().is_none());
        assert!(fixtures[2].as_infantry().is_none());
        assert!(fixtures[2].as_aircraft().is_some());
        assert!(fixtures[2].as_building().is_none());

        assert!(fixtures[3].as_unit().is_none());
        assert!(fixtures[3].as_infantry().is_none());
        assert!(fixtures[3].as_aircraft().is_none());
        assert!(fixtures[3].as_building().is_some());
    }

    #[test]
    fn mission_leaf_narrow_writers_preserve_other_raw_fields() {
        let mut unit = MissionLeafState::unit_raw_for_test(1, 2, 3, 4);
        unit.set_unit_deploy_begin_active(5);
        unit.set_unit_deploy_reverse_active(6);
        unit.set_unit_tracker_byte_18(7);
        unit.set_unit_tracker_byte_19(8);
        let unit_view = unit.as_unit().expect("Unit view");
        assert_eq!(unit_view.deploy_begin_active(), 5);
        assert_eq!(unit_view.deploy_reverse_active(), 6);
        assert_eq!(unit_view.tracker_byte_18(), 7);
        assert_eq!(unit_view.tracker_byte_19(), 8);

        let mut infantry = MissionLeafState::infantry_raw_for_test(9, 10);
        infantry.set_infantry_firing_sequence(11);
        infantry
            .set_infantry_doing_verified(41)
            .expect("verified Doing");
        assert_eq!(
            infantry
                .as_infantry()
                .expect("Infantry view")
                .firing_sequence_latch(),
            11
        );
        assert_eq!(infantry.as_infantry().expect("Infantry view").doing(), 41);

        let mut aircraft = MissionLeafState::aircraft_raw_for_test(12, 13, true);
        aircraft.set_aircraft_transition_ready(14);
        aircraft.clear_aircraft_action_for_commence();
        let aircraft_view = aircraft.as_aircraft().expect("Aircraft view");
        assert_eq!(aircraft_view.action_latch(), 0);
        assert_eq!(aircraft_view.transition_ready_latch(), 14);
        assert!(aircraft_view.airstrike_manager_present());

        let mut building = MissionLeafState::building_raw_for_test(15);
        building.set_building_ready_latch(16);
        assert_eq!(
            building.as_building().expect("Building view").ready_latch(),
            16
        );
    }

    #[test]
    fn mission_leaf_invalid_doing_does_not_mutate() {
        for invalid in [i32::MIN, -2, 42, i32::MAX] {
            let mut leaf = MissionLeafState::infantry_raw_for_test(7, 5);
            let before = leaf;
            assert_eq!(
                leaf.set_infantry_doing_verified(invalid),
                Err(InvalidInfantryDoing(invalid))
            );
            assert_eq!(leaf, before);
        }
    }

    #[test]
    #[should_panic(expected = "Unit Mission leaf writer used for another category")]
    fn mission_leaf_wrong_category_writer_fails_loudly() {
        let mut leaf = MissionLeafState::for_entity_category(EntityCategory::Infantry);
        leaf.set_unit_deploy_begin_active(1);
    }

    #[test]
    fn mission_leaf_serde_round_trip_preserves_every_raw_field() {
        let fixtures = [
            MissionLeafState::unit_raw_for_test(1, 2, 3, u8::MAX),
            MissionLeafState::infantry_raw_for_test(u8::MAX, 41),
            MissionLeafState::aircraft_raw_for_test(u8::MAX, 0, true),
            MissionLeafState::building_raw_for_test(u8::MAX),
        ];

        for fixture in fixtures {
            let bytes = bincode::serialize(&fixture).expect("serialize Mission leaf");
            let restored: MissionLeafState =
                bincode::deserialize(&bytes).expect("deserialize Mission leaf");
            assert_eq!(restored, fixture);
        }
    }
}
