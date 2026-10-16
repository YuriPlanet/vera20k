//! Category-specific raw state consumed by Mission readiness and policy.
//!
//! The native Unit, Infantry, Aircraft, and Building leaves own different
//! bytes. Keeping them in sealed variants prevents those bytes from collapsing
//! into a generic busy flag while still allowing read-only readiness queries.

use crate::map::entities::EntityCategory;

/// Entity-owned Mission state whose layout depends on the concrete Techno
/// family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MissionLeafState {
    Unit(UnitMissionLeaf),
    Infantry(InfantryMissionLeaf),
    Aircraft(AircraftMissionLeaf),
    Building(BuildingMissionLeaf),
}

/// Unit readiness bytes and its inherited Foot firing byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnitMissionLeaf {
    /// Inherited Foot+6B3, constructed clear at4D3438. Foot4D82B0
    /// admits one base idle entry until the live FootAI4DA54E reset.
    #[serde(default)]
    idle_entry_latch: u8,
    /// Foot+68D: ctor4D33C6 clears it; Unit736DF0 only clears it, while raw
    /// Object5F5E80/Abstract410380 load retains the full Unit8E8 record.
    firing_sequence_latch: u8,
    /// Unit+6E0, constructed clear and written by739AC0/739CD0.
    deployed: u8,
    deploy_begin_active: u8,
    deploy_reverse_active: u8,
    tracker_byte_18: u8,
    tracker_byte_19: u8,
}

/// Infantry readiness inputs owned by the firing and Doing authorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InfantryMissionLeaf {
    #[serde(default)]
    idle_entry_latch: u8,
    firing_sequence_latch: u8,
    doing: i32,
    /// Infantry+6E8: ctor517AC2 and Limbo51DF38 write2. DoAction51D8B8
    /// stores0 for Water/Beach offbridge, otherwise1, before its action
    /// admission.
    /// Native load retains the signed dword, including the initial sentinel.
    #[serde(default = "initial_infantry_water_state")]
    water_state: i32,
    /// Infantry+6E4: ctor517ABC clears it; Guard52167C and the
    /// approach/deploy request producers set it after locomotor Stop.
    /// Stop callback521B40 consumes it before requesting unforced Deploy27.
    #[serde(default)]
    pending_deploy: u8,
}

const fn initial_infantry_water_state() -> i32 {
    2
}

/// Aircraft policy and readiness bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AircraftMissionLeaf {
    #[serde(default)]
    idle_entry_latch: u8,
    /// The same inherited Foot+68D, retained by the raw Aircraft6D8 load.
    firing_sequence_latch: u8,
    /// Aircraft+6D2: shared by Mission_Attack, Fly and ReadyToCommence41B5E0.
    action_latch: u8,
    /// Aircraft+6D4, independent of the pending-ammunition byte+6C8.
    transition_ready_latch: u8,
    /// Aircraft `+0x6D3`, the paradrop passes left: the constructor writes 5
    /// (`0x00413D74`), each Mission_ParadropApproach turn into Overfly takes
    /// one (`0x00415950`), and each drop restores 5 (`0x00415E93`).
    /// Overfly comes round again only while it is above 0 (`0x0041599F`).
    #[serde(default = "initial_paradrop_passes")]
    paradrop_passes: i8,
}

const fn initial_paradrop_passes() -> i8 {
    5
}

/// Building reusable mission-ready latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingMissionLeaf {
    ready_latch: u8,
    /// Independent Building+620 StageClass (ctor43B7F5; Repair44BBF2).
    repair_progress: crate::sim::stage::StageClass,
    /// Building `+0x5F8`: the super weapon a Missile mission fires.
    /// `SuperClass::Launch` stores the type's `Type=` value (`+0xB4`,
    /// `0x006CDDD7`), which Mission_Missile then indexes the SuperWeaponType
    /// array with (`0x0044CABA`); retail lists every type at the index of its
    /// `Type=`. Constructor -1 (`0x0043B7B9`).
    #[serde(default = "no_firing_super_weapon")]
    firing_super_weapon: i32,
}

const fn no_firing_super_weapon() -> i32 {
    -1
}

/// A Doing value rejected by the verified Infantry writer domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Infantry Doing value is outside the verified writer domain: {0}")]
pub struct InvalidInfantryDoing(pub(crate) i32);

impl MissionLeafState {
    /// One inherited byte per Foot; Building does not inherit Foot6B3.
    pub(crate) const fn foot_idle_entry_latch(&self) -> u8 {
        match self {
            Self::Unit(leaf) => leaf.idle_entry_latch,
            Self::Infantry(leaf) => leaf.idle_entry_latch,
            Self::Aircraft(leaf) => leaf.idle_entry_latch,
            Self::Building(_) => 0,
        }
    }

    /// Original Foot4D82D2 / FootAI4DA54E. The class idle tail still
    /// runs when the base receiver returns false for a nonzero latch.
    pub(crate) fn set_foot_idle_entry_latch(&mut self, raw: u8) {
        match self {
            Self::Unit(leaf) => leaf.idle_entry_latch = raw,
            Self::Infantry(leaf) => leaf.idle_entry_latch = raw,
            Self::Aircraft(leaf) => leaf.idle_entry_latch = raw,
            Self::Building(_) => panic!("Foot idle writer used for a Building"),
        }
    }

    /// Frame-zero constructor used by prepared test fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn for_entity_category(category: EntityCategory) -> Self {
        Self::constructed(category, 0)
    }

    /// Each native Building constructs its independent progress timer at Frame.
    pub(crate) const fn constructed(category: EntityCategory, now: i32) -> Self {
        match category {
            EntityCategory::Unit => Self::Unit(UnitMissionLeaf::initial()),
            EntityCategory::Infantry => Self::Infantry(InfantryMissionLeaf::initial()),
            EntityCategory::Aircraft => Self::Aircraft(AircraftMissionLeaf::initial()),
            EntityCategory::Structure => Self::Building(BuildingMissionLeaf {
                ready_latch: 0,
                repair_progress: crate::sim::stage::StageClass::constructed(now),
                firing_super_weapon: no_firing_super_weapon(),
            }),
        }
    }

    /// Repair44C577 starts the building-owned progress clock.
    pub(crate) fn start_building_repair_progress(&mut self, now: i32) {
        self.expect_building_mut()
            .repair_progress
            .restart(0, now, 1);
    }

    /// Repair44BC18 arms a stopped stage;44BC35 advances at most once per visit.
    pub(crate) fn advance_building_repair_progress(&mut self, now: i32) -> i32 {
        let stage = &mut self.expect_building_mut().repair_progress;
        if stage.rate() == 0 {
            stage.set_rate(1);
        }
        stage.advance(now);
        stage.value()
    }

    ///44BD5F resets only the value, retaining rate/timer/changed/increment.
    pub(crate) fn reset_building_repair_progress(&mut self) {
        self.expect_building_mut().repair_progress.set_value(0);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_building_repair_progress_fixture(
        &mut self,
        stage: crate::sim::stage::StageClass,
    ) {
        self.expect_building_mut().repair_progress = stage;
    }

    /// Borrow the Unit inputs without permitting mutation.
    pub const fn as_unit(&self) -> Option<&UnitMissionLeaf> {
        match self {
            Self::Unit(leaf) => Some(leaf),
            _ => None,
        }
    }

    /// Borrow the Infantry inputs without permitting mutation.
    pub const fn as_infantry(&self) -> Option<&InfantryMissionLeaf> {
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

    pub(crate) fn set_unit_deployed(&mut self, raw: u8) {
        self.expect_unit_mut().deployed = raw;
    }

    pub(crate) fn set_unit_deploy_begin_active(&mut self, raw: u8) {
        self.expect_unit_mut().deploy_begin_active = raw;
    }

    pub(crate) fn set_unit_deploy_reverse_active(&mut self, raw: u8) {
        self.expect_unit_mut().deploy_reverse_active = raw;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_unit_tracker_byte_18(&mut self, raw: u8) {
        self.expect_unit_mut().tracker_byte_18 = raw;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_unit_tracker_byte_19(&mut self, raw: u8) {
        self.expect_unit_mut().tracker_byte_19 = raw;
    }

    /// The one inherited Foot+68D view used by Foot handlers. Infantry's
    /// concrete readiness view reads that same variant field. Buildings do
    /// not inherit this byte.
    pub const fn foot_firing_sequence_latch(&self) -> u8 {
        match self {
            Self::Unit(leaf) => leaf.firing_sequence_latch,
            Self::Infantry(leaf) => leaf.firing_sequence_latch,
            Self::Aircraft(leaf) => leaf.firing_sequence_latch,
            Self::Building(_) => 0,
        }
    }

    /// Raw Foot+68D state. Its active nonzero producer is Infantry5206B0's
    /// store520912; class target-change51B20E and Unit736DF0 clear it.
    /// No ordinary Unit/Aircraft nonzero producer is claimed here. Native
    /// load retains this byte; do not infer it from a pending shot or Doing.
    #[track_caller]
    pub(crate) fn set_foot_firing_sequence(&mut self, raw: u8) {
        match self {
            Self::Unit(leaf) => leaf.firing_sequence_latch = raw,
            Self::Infantry(leaf) => leaf.firing_sequence_latch = raw,
            Self::Aircraft(leaf) => leaf.firing_sequence_latch = raw,
            Self::Building(_) => panic!("Foot firing writer used for a Building"),
        }
    }

    /// Write only values accepted by the verified 42-entry Doing table.
    pub fn set_infantry_doing_verified(&mut self, doing: i32) -> Result<(), InvalidInfantryDoing> {
        let leaf = self.expect_infantry_mut();
        if doing != -1 && !(0..=41).contains(&doing) {
            return Err(InvalidInfantryDoing(doing));
        }
        leaf.doing = doing;
        Ok(())
    }

    /// Original51D8B8 is a pre-admission store: even an unchanged or
    /// noninterruptible action retains the new land/water state.
    pub(crate) fn set_infantry_water_state(&mut self, on_land: bool) {
        self.expect_infantry_mut().water_state = i32::from(on_land);
    }

    /// `InfantryClass::Limbo` stores the constructor's sentinel again
    /// (`0x0051DF38`), so the next `Do_Action` plays no water sound.
    pub(crate) fn reset_infantry_water_state(&mut self) {
        self.expect_infantry_mut().water_state = initial_infantry_water_state();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_infantry_water_state_fixture(&mut self, raw: i32) {
        self.expect_infantry_mut().water_state = raw;
    }

    /// Aircraft `+0x6D4`, Ready_To_Commence's answer (`0x0041B5E0`):
    /// Mission_Guard and Mission_Enter write it.
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

    /// Mission_ParadropApproach's `DEC byte [+0x6D3]` (`0x00415950`).
    pub(crate) fn take_paradrop_pass(&mut self) {
        let leaf = self.expect_aircraft_mut();
        leaf.paradrop_passes = leaf.paradrop_passes.wrapping_sub(1);
    }

    /// Drop_Payload's `MOV byte [+0x6D3], 5` (`0x00415E93`).
    pub(crate) fn restore_paradrop_passes(&mut self) {
        self.expect_aircraft_mut().paradrop_passes = initial_paradrop_passes();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_paradrop_passes_for_test(&mut self, passes: i8) {
        self.expect_aircraft_mut().paradrop_passes = passes;
    }

    /// Building `+0x6DD`, written by Mission_Guard (`0x00449701`),
    /// Mission_Attack (`0x0044B008`), the first opening (`0x004467C9`) and
    /// cleared by Update's ready checks (`0x0043FE4D`, `0x0043FFAD`).
    pub(crate) fn set_building_ready_latch(&mut self, raw: u8) {
        self.expect_building_mut().ready_latch = raw;
    }

    /// Building `+0x5F8`, written by `SuperClass::Launch` (`0x006CDDD7`).
    pub(crate) fn set_building_firing_super_weapon(&mut self, super_weapon_type: i32) {
        self.expect_building_mut().firing_super_weapon = super_weapon_type;
    }

    #[track_caller]
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

    /// Infantry Guard52167C writes this after Stop_Moving returns. The
    /// synchronous callback521B52 may consume an earlier byte during Stop.
    pub(crate) fn set_infantry_pending_deploy(&mut self, raw: u8) {
        self.expect_infantry_mut().pending_deploy = raw;
    }

    /// Native521B52 clears the byte before the class Do_Action call, even
    /// when that call refuses. A recursive Stop observes the consumed byte.
    pub(crate) fn take_infantry_pending_deploy(&mut self) -> bool {
        let leaf = self.expect_infantry_mut();
        let pending = leaf.pending_deploy != 0;
        leaf.pending_deploy = 0;
        pending
    }

    #[track_caller]
    fn expect_aircraft_mut(&mut self) -> &mut AircraftMissionLeaf {
        match self {
            Self::Aircraft(leaf) => leaf,
            _ => panic!("Aircraft Mission leaf writer used for another category"),
        }
    }

    #[track_caller]
    fn expect_building_mut(&mut self) -> &mut BuildingMissionLeaf {
        match self {
            Self::Building(leaf) => leaf,
            _ => panic!("Building Mission leaf writer used for another category"),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn unit_raw_for_test(
        deploy_begin_active: u8,
        deploy_reverse_active: u8,
        tracker_byte_18: u8,
        tracker_byte_19: u8,
    ) -> Self {
        Self::Unit(UnitMissionLeaf {
            idle_entry_latch: 0,
            firing_sequence_latch: 0,
            deployed: 0,
            deploy_begin_active,
            deploy_reverse_active,
            tracker_byte_18,
            tracker_byte_19,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn infantry_raw_for_test(firing_sequence_latch: u8, doing: i32) -> Self {
        Self::Infantry(InfantryMissionLeaf {
            idle_entry_latch: 0,
            firing_sequence_latch,
            doing,
            water_state: initial_infantry_water_state(),
            pending_deploy: 0,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn aircraft_raw_for_test(
        action_latch: u8,
        transition_ready_latch: u8,
    ) -> Self {
        Self::Aircraft(AircraftMissionLeaf {
            idle_entry_latch: 0,
            firing_sequence_latch: 0,
            action_latch,
            transition_ready_latch,
            paradrop_passes: initial_paradrop_passes(),
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn building_raw_for_test(ready_latch: u8) -> Self {
        let mut leaf = Self::constructed(EntityCategory::Structure, 0);
        leaf.set_building_ready_latch(ready_latch);
        leaf
    }
}

impl UnitMissionLeaf {
    const fn initial() -> Self {
        Self {
            idle_entry_latch: 0,
            firing_sequence_latch: 0,
            deployed: 0,
            deploy_begin_active: 0,
            deploy_reverse_active: 0,
            tracker_byte_18: 0,
            tracker_byte_19: 0,
        }
    }

    pub const fn deployed(&self) -> u8 {
        self.deployed
    }

    pub const fn deploy_begin_active(&self) -> u8 {
        self.deploy_begin_active
    }

    pub const fn deploy_reverse_active(&self) -> u8 {
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
            idle_entry_latch: 0,
            firing_sequence_latch: 0,
            doing: -1,
            water_state: initial_infantry_water_state(),
            pending_deploy: 0,
        }
    }

    pub(crate) const fn firing_sequence_latch(&self) -> u8 {
        self.firing_sequence_latch
    }

    pub const fn doing(&self) -> i32 {
        self.doing
    }

    pub(crate) const fn water_state(&self) -> i32 {
        self.water_state
    }

    pub(crate) const fn pending_deploy(&self) -> u8 {
        self.pending_deploy
    }
}

impl AircraftMissionLeaf {
    const fn initial() -> Self {
        Self {
            idle_entry_latch: 0,
            firing_sequence_latch: 0,
            action_latch: 0,
            transition_ready_latch: 1,
            paradrop_passes: initial_paradrop_passes(),
        }
    }

    pub(crate) const fn paradrop_passes(&self) -> i8 {
        self.paradrop_passes
    }

    pub(crate) const fn action_latch(&self) -> u8 {
        self.action_latch
    }

    pub(crate) const fn transition_ready_latch(&self) -> u8 {
        self.transition_ready_latch
    }
}

impl BuildingMissionLeaf {
    pub(crate) const fn repair_progress(&self) -> &crate::sim::stage::StageClass {
        &self.repair_progress
    }

    pub(crate) const fn ready_latch(&self) -> u8 {
        self.ready_latch
    }

    /// Building `+0x5F8` (see the field).
    pub(crate) const fn firing_super_weapon(&self) -> i32 {
        self.firing_super_weapon
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
        infantry.set_foot_firing_sequence(11);
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

        let mut aircraft = MissionLeafState::aircraft_raw_for_test(12, 13);
        aircraft.set_aircraft_transition_ready(14);
        aircraft.clear_aircraft_action_for_commence();
        let aircraft_view = aircraft.as_aircraft().expect("Aircraft view");
        assert_eq!(aircraft_view.action_latch(), 0);
        assert_eq!(aircraft_view.transition_ready_latch(), 14);

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
        // A paradrop plane past its last pass: `+0x6D3` below zero.
        let mut paradrop = MissionLeafState::aircraft_raw_for_test(1, 2);
        paradrop.set_paradrop_passes_for_test(-1);
        let fixtures = [
            MissionLeafState::unit_raw_for_test(1, 2, 3, u8::MAX),
            MissionLeafState::infantry_raw_for_test(u8::MAX, 41),
            MissionLeafState::aircraft_raw_for_test(u8::MAX, 0),
            paradrop,
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
