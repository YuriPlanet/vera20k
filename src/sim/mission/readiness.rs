//! Pure category-specific Mission ReadyToCommence predicates.
//!
//! These functions preserve native short-circuit order over explicit raw
//! inputs. World-dependent Unit lookups are borrowed through a read-only trait;
//! unavailable authoritative inputs are errors rather than guessed state.

use super::leaf::{
    AircraftMissionLeaf, BuildingMissionLeaf, InfantryMissionLeaf, InvalidInfantryDoing,
    UnitMissionLeaf,
};
use super::{MissionCom, MissionId};
use crate::sim::movement::locomotor_ready::LocomotorReadyState;

const MISSION_ATTACK: MissionId = MissionId::from_raw(1);
const MISSION_MOVE: MissionId = MissionId::from_raw(2);
const MISSION_GUARD: MissionId = MissionId::from_raw(5);
const MISSION_STICKY: MissionId = MissionId::from_raw(6);
const MISSION_ENTER: MissionId = MissionId::from_raw(7);
const MISSION_HUNT: MissionId = MissionId::from_raw(15);
const MISSION_RESCUE: MissionId = MissionId::from_raw(21);
const MISSION_AIRCRAFT_ACTION_EXCEPTION: MissionId = MissionId::from_raw(0x1e);

/// An exact input that the current Rust owner could not supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadyUnavailable {
    #[error("exact locomotor readiness input is unavailable")]
    Locomotor,
    #[error("Infantry Doing value is outside the verified writer domain: {0}")]
    InvalidDoing(i32),
    #[error("required entity/type/occupancy lookup is unavailable")]
    WorldLookup,
}

pub(crate) type ReadyResult = Result<bool, ReadyUnavailable>;

impl From<InvalidInfantryDoing> for ReadyUnavailable {
    fn from(value: InvalidInfantryDoing) -> Self {
        Self::InvalidDoing(value.0)
    }
}

/// Signed lepton coordinates used by Unit's factory-anchor fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadyLeptonPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl ReadyLeptonPoint {
    pub(crate) const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Classification of exactly Radio contact slot zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitReadyContact {
    Building { weapons_factory: bool },
    Other,
}

/// The Building selected by the native-style building-under lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UnitReadyBuilding {
    pub(crate) weapons_factory: bool,
    pub(crate) anchor: ReadyLeptonPoint,
}

impl UnitReadyBuilding {
    pub(crate) const fn new(weapons_factory: bool, anchor: ReadyLeptonPoint) -> Self {
        Self {
            weapons_factory,
            anchor,
        }
    }
}

/// Read-only world operations used by Unit readiness.
///
/// `building_under_in_stored_order` must return the Building selected by the
/// existing occupancy-list order. Implementations must not sort IDs, construct
/// a replacement footprint query, or allocate a fresh candidate list.
pub(crate) trait UnitReadyWorld {
    fn contact_slot_zero(&self) -> Result<Option<UnitReadyContact>, ReadyUnavailable>;

    fn building_under_in_stored_order(
        &self,
        unit_position: ReadyLeptonPoint,
    ) -> Result<Option<UnitReadyBuilding>, ReadyUnavailable>;

    /// `Get_Height` (vt+0x1C8, `ObjectClass::GetHeight @ 0x005F5F40`), asked
    /// only on the moving path (`0x007442E9`).
    fn signed_height(&self) -> i32;
}

/// Complete borrowed/scalar inputs for the Unit leaf.
pub(crate) struct UnitReadyView<'a, W: UnitReadyWorld + ?Sized> {
    pub(crate) mission: &'a MissionCom,
    pub(crate) leaf: &'a UnitMissionLeaf,
    pub(crate) unload_active: bool,
    pub(crate) locomotor: Option<LocomotorReadyState>,
    pub(crate) attack_target_present: bool,
    pub(crate) position: ReadyLeptonPoint,
    pub(crate) world: &'a W,
}

/// Complete borrowed/scalar inputs for the Infantry leaf.
pub(crate) struct InfantryReadyView<'a> {
    pub(crate) mission: &'a MissionCom,
    pub(crate) leaf: &'a InfantryMissionLeaf,
    pub(crate) object_is_falling_down: u8,
    pub(crate) locomotor: Option<LocomotorReadyState>,
    pub(crate) attack_target_present: bool,
}

/// Complete borrowed inputs for the Aircraft leaf.
pub(crate) struct AircraftReadyView<'a> {
    pub(crate) mission: &'a MissionCom,
    pub(crate) leaf: &'a AircraftMissionLeaf,
}

/// Complete borrowed input for the Building leaf.
pub(crate) struct BuildingReadyView<'a> {
    pub(crate) leaf: &'a BuildingMissionLeaf,
}

/// Base MissionClass, RadioClass, and FootClass add no readiness gate.
#[inline]
#[cfg(any(test, feature = "test-support"))]
pub(crate) const fn base_ready_to_commence() -> ReadyResult {
    Ok(true)
}

/// Evaluate Unit readiness in active-gamemd branch order.
pub(crate) fn unit_ready_to_commence<W: UnitReadyWorld + ?Sized>(
    view: UnitReadyView<'_, W>,
) -> ReadyResult {
    let current = view.mission.current();
    if current == MISSION_STICKY || current == MISSION_RESCUE {
        return Ok(false);
    }
    if view.leaf.deploy_begin_active() != 0 {
        return Ok(false);
    }
    if view.leaf.deploy_reverse_active() != 0 {
        return Ok(false);
    }
    if view.unload_active {
        return Ok(false);
    }

    let queued = view.mission.queued();
    if queued != MISSION_ENTER {
        let moving = view
            .locomotor
            .ok_or(ReadyUnavailable::Locomotor)?
            .is_moving_now();
        if moving {
            if view.world.signed_height() >= 0 {
                let effective = view.mission.effective();
                if effective != MISSION_GUARD
                    && !(effective == MISSION_ATTACK && !view.attack_target_present)
                    && view.mission.movement_bypass_latch() == 0
                {
                    return Ok(false);
                }
            }
        }
    }

    if view.leaf.tracker_byte_18() != 0 || view.leaf.tracker_byte_19() != 0 {
        return Ok(false);
    }

    if let Some(contact) = view.world.contact_slot_zero()? {
        if matches!(
            contact,
            UnitReadyContact::Building {
                weapons_factory: true
            }
        ) && queued != MISSION_MOVE
            && queued != MISSION_ENTER
        {
            return Ok(false);
        }
    } else if let Some(building) = view.world.building_under_in_stored_order(view.position)?
        && building.weapons_factory
        && crate::util::lepton::lepton_to_cell_packed(view.position.x)
            == crate::util::lepton::lepton_to_cell_packed(building.anchor.x)
        && crate::util::lepton::lepton_to_cell_packed(view.position.y).wrapping_sub(
            crate::util::lepton::lepton_to_cell_packed(building.anchor.y),
        ) == 1
    {
        return Ok(false);
    }

    Ok(true)
}

/// Evaluate Infantry readiness in active-gamemd branch order.
pub(crate) fn infantry_ready_to_commence(view: InfantryReadyView<'_>) -> ReadyResult {
    let current = view.mission.current();
    if current == MISSION_STICKY || current == MISSION_RESCUE {
        return Ok(false);
    }
    if view.leaf.firing_sequence_latch() != 0 {
        return Ok(false);
    }
    if view.object_is_falling_down != 0 {
        return Ok(false);
    }

    let moving = view
        .locomotor
        .ok_or(ReadyUnavailable::Locomotor)?
        .is_moving_now();
    let effective = view.mission.effective();
    if moving && effective != MISSION_GUARD && effective != MISSION_HUNT {
        if effective != MISSION_ATTACK {
            return Ok(false);
        }
        if view.attack_target_present {
            return Ok(false);
        }
    }

    infantry_doing_allows(view.leaf.doing())
}

/// Evaluate Aircraft readiness from its two raw latches.
pub(crate) fn aircraft_ready_to_commence(view: AircraftReadyView<'_>) -> ReadyResult {
    let current = view.mission.current();
    if current == MISSION_STICKY || current == MISSION_RESCUE {
        return Ok(false);
    }
    if current != MISSION_AIRCRAFT_ACTION_EXCEPTION && view.leaf.action_latch() != 0 {
        return Ok(false);
    }
    Ok(view.leaf.transition_ready_latch() != 0)
}

/// Evaluate Building readiness from its reusable raw latch.
pub(crate) fn building_ready_to_commence(view: BuildingReadyView<'_>) -> ReadyResult {
    Ok(view.leaf.ready_latch() != 0)
}

fn infantry_doing_allows(doing: i32) -> ReadyResult {
    if doing == -1 {
        return Ok(true);
    }
    crate::rules::infantry_sequence::action_record(doing)
        .map(|record| record.interruptible)
        .ok_or(ReadyUnavailable::InvalidDoing(doing))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::super::MissionDispatchTimer;
    use super::super::leaf::MissionLeafState;
    use super::super::state::MissionTestFixture;
    use super::*;
    use crate::map::entities::EntityCategory;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Lookup {
        ContactSlotZero,
        BuildingUnder(ReadyLeptonPoint),
    }

    struct TestWorld {
        contact: Result<Option<UnitReadyContact>, ReadyUnavailable>,
        building: Result<Option<UnitReadyBuilding>, ReadyUnavailable>,
        height: i32,
        calls: RefCell<Vec<Lookup>>,
    }

    impl Default for TestWorld {
        fn default() -> Self {
            Self {
                contact: Ok(None),
                building: Ok(None),
                height: 0,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl UnitReadyWorld for TestWorld {
        fn contact_slot_zero(&self) -> Result<Option<UnitReadyContact>, ReadyUnavailable> {
            self.calls.borrow_mut().push(Lookup::ContactSlotZero);
            self.contact
        }

        fn building_under_in_stored_order(
            &self,
            unit_position: ReadyLeptonPoint,
        ) -> Result<Option<UnitReadyBuilding>, ReadyUnavailable> {
            self.calls
                .borrow_mut()
                .push(Lookup::BuildingUnder(unit_position));
            self.building
        }

        fn signed_height(&self) -> i32 {
            self.height
        }
    }

    fn mission_fixture(current: i32, queued: i32, movement_bypass_latch: u8) -> MissionCom {
        let mut mission = MissionCom::at_frame(0);
        mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(current),
            suspended: MissionId::NONE,
            queued: MissionId::from_raw(queued),
            movement_bypass_latch,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        mission
    }

    fn stopped() -> LocomotorReadyState {
        LocomotorReadyState::Drive {
            turning_active: false,
            slot_moving: false,
            head_to_nonnull: false,
            owner_speed: 0,
        }
    }

    fn moving() -> LocomotorReadyState {
        LocomotorReadyState::Drive {
            turning_active: true,
            slot_moving: false,
            head_to_nonnull: false,
            owner_speed: 0,
        }
    }

    fn unit_view<'a>(
        mission: &'a MissionCom,
        leaf: &'a UnitMissionLeaf,
        world: &'a TestWorld,
    ) -> UnitReadyView<'a, TestWorld> {
        UnitReadyView {
            mission,
            leaf,
            unload_active: false,
            locomotor: Some(stopped()),
            attack_target_present: false,
            position: ReadyLeptonPoint::new(40 * 256, 51 * 256),
            world,
        }
    }

    fn infantry_view<'a>(
        mission: &'a MissionCom,
        leaf: &'a InfantryMissionLeaf,
    ) -> InfantryReadyView<'a> {
        InfantryReadyView {
            mission,
            leaf,
            object_is_falling_down: 0,
            locomotor: Some(stopped()),
            attack_target_present: false,
        }
    }

    fn unit_leaf(
        deploy_begin_active: u8,
        deploy_reverse_active: u8,
        tracker_byte_18: u8,
        tracker_byte_19: u8,
    ) -> MissionLeafState {
        MissionLeafState::unit_raw_for_test(
            deploy_begin_active,
            deploy_reverse_active,
            tracker_byte_18,
            tracker_byte_19,
        )
    }

    #[test]
    fn unit_ready_to_commence_preserves_early_branch_precedence() {
        for current in [MISSION_STICKY.raw(), MISSION_RESCUE.raw()] {
            let mission = mission_fixture(current, MissionId::NONE.raw(), 0);
            let leaf_state = unit_leaf(0, 0, 0, 0);
            let world = TestWorld {
                contact: Err(ReadyUnavailable::WorldLookup),
                building: Err(ReadyUnavailable::WorldLookup),
                ..TestWorld::default()
            };
            let mut view = unit_view(&mission, leaf_state.as_unit().expect("Unit leaf"), &world);
            view.locomotor = None;
            assert_eq!(unit_ready_to_commence(view), Ok(false));
            assert!(world.calls.borrow().is_empty());
        }

        for leaf_state in [unit_leaf(1, 0, 0, 0), unit_leaf(0, 1, 0, 0)] {
            let mission = mission_fixture(MISSION_MOVE.raw(), MissionId::NONE.raw(), 0);
            let world = TestWorld::default();
            let mut view = unit_view(&mission, leaf_state.as_unit().expect("Unit leaf"), &world);
            view.locomotor = None;
            assert_eq!(unit_ready_to_commence(view), Ok(false));
            assert!(world.calls.borrow().is_empty());
        }

        let mission = mission_fixture(MISSION_MOVE.raw(), MissionId::NONE.raw(), 0);
        let leaf_state = unit_leaf(0, 0, 0, 0);
        let world = TestWorld::default();
        let mut view = unit_view(&mission, leaf_state.as_unit().expect("Unit leaf"), &world);
        view.unload_active = true;
        view.locomotor = None;
        assert_eq!(unit_ready_to_commence(view), Ok(false));
        assert!(world.calls.borrow().is_empty());
    }

    #[test]
    fn unit_ready_to_commence_handles_locomotor_height_and_native_exceptions() {
        let leaf_state = unit_leaf(0, 0, 0, 0);
        let leaf = leaf_state.as_unit().expect("Unit leaf");

        let mission = mission_fixture(MISSION_MOVE.raw(), MissionId::NONE.raw(), 0);
        let world = TestWorld::default();
        let mut view = unit_view(&mission, leaf, &world);
        view.locomotor = None;
        assert_eq!(
            unit_ready_to_commence(view),
            Err(ReadyUnavailable::Locomotor)
        );
        assert!(world.calls.borrow().is_empty());

        let world = TestWorld::default();
        let mut view = unit_view(&mission, leaf, &world);
        view.locomotor = Some(moving());
        assert_eq!(unit_ready_to_commence(view), Ok(false));
        assert!(world.calls.borrow().is_empty());

        let world = TestWorld {
            height: -1,
            ..TestWorld::default()
        };
        let mut view = unit_view(&mission, leaf, &world);
        view.locomotor = Some(moving());
        assert_eq!(unit_ready_to_commence(view), Ok(true));

        for (current, target_present, bypass) in [
            (MISSION_GUARD.raw(), false, 0),
            (MISSION_ATTACK.raw(), false, 0),
            (MISSION_MOVE.raw(), false, 1),
        ] {
            let mission = mission_fixture(current, MissionId::NONE.raw(), bypass);
            let world = TestWorld::default();
            let mut view = unit_view(&mission, leaf, &world);
            view.locomotor = Some(moving());
            view.attack_target_present = target_present;
            assert_eq!(unit_ready_to_commence(view), Ok(true));
        }

        let mission = mission_fixture(MISSION_ATTACK.raw(), MissionId::NONE.raw(), 0);
        let world = TestWorld::default();
        let mut view = unit_view(&mission, leaf, &world);
        view.locomotor = Some(moving());
        view.attack_target_present = true;
        assert_eq!(unit_ready_to_commence(view), Ok(false));

        let mission = mission_fixture(MissionId::NONE.raw(), MISSION_GUARD.raw(), 0);
        let world = TestWorld::default();
        let mut view = unit_view(&mission, leaf, &world);
        view.locomotor = Some(moving());
        assert_eq!(unit_ready_to_commence(view), Ok(true));
    }

    #[test]
    fn unit_ready_to_commence_queued_enter_bypasses_only_locomotor_branch() {
        let mission = mission_fixture(MISSION_MOVE.raw(), MISSION_ENTER.raw(), 0);
        let leaf_state = unit_leaf(0, 0, 0, 0);
        let world = TestWorld::default();
        let mut view = unit_view(&mission, leaf_state.as_unit().expect("Unit leaf"), &world);
        view.locomotor = None;
        let position = view.position;
        assert_eq!(unit_ready_to_commence(view), Ok(true));
        assert_eq!(
            world.calls.borrow().as_slice(),
            &[Lookup::ContactSlotZero, Lookup::BuildingUnder(position)]
        );

        for leaf_state in [unit_leaf(0, 0, 1, 0), unit_leaf(0, 0, 0, 1)] {
            let world = TestWorld::default();
            let mut view = unit_view(&mission, leaf_state.as_unit().expect("Unit leaf"), &world);
            view.locomotor = None;
            assert_eq!(unit_ready_to_commence(view), Ok(false));
            assert!(world.calls.borrow().is_empty());
        }
    }

    #[test]
    fn unit_ready_to_commence_contact_slot_zero_factory_gate_is_exact() {
        let leaf_state = unit_leaf(0, 0, 0, 0);
        let leaf = leaf_state.as_unit().expect("Unit leaf");

        let mission = mission_fixture(MISSION_GUARD.raw(), MISSION_GUARD.raw(), 0);
        let world = TestWorld {
            contact: Ok(Some(UnitReadyContact::Building {
                weapons_factory: true,
            })),
            building: Ok(Some(UnitReadyBuilding::new(
                true,
                ReadyLeptonPoint::new(40 * 256, 50 * 256),
            ))),
            ..TestWorld::default()
        };
        assert_eq!(
            unit_ready_to_commence(unit_view(&mission, leaf, &world)),
            Ok(false)
        );
        assert_eq!(world.calls.borrow().as_slice(), &[Lookup::ContactSlotZero]);

        for queued in [MISSION_MOVE.raw(), MISSION_ENTER.raw()] {
            let mission = mission_fixture(MISSION_GUARD.raw(), queued, 0);
            let world = TestWorld {
                contact: Ok(Some(UnitReadyContact::Building {
                    weapons_factory: true,
                })),
                building: Err(ReadyUnavailable::WorldLookup),
                ..TestWorld::default()
            };
            let mut view = unit_view(&mission, leaf, &world);
            if queued == MISSION_ENTER.raw() {
                view.locomotor = None;
            }
            assert_eq!(unit_ready_to_commence(view), Ok(true));
            assert_eq!(world.calls.borrow().as_slice(), &[Lookup::ContactSlotZero]);
        }

        for contact in [
            UnitReadyContact::Other,
            UnitReadyContact::Building {
                weapons_factory: false,
            },
        ] {
            let mission = mission_fixture(MISSION_GUARD.raw(), MISSION_GUARD.raw(), 0);
            let world = TestWorld {
                contact: Ok(Some(contact)),
                building: Err(ReadyUnavailable::WorldLookup),
                ..TestWorld::default()
            };
            assert_eq!(
                unit_ready_to_commence(unit_view(&mission, leaf, &world)),
                Ok(true)
            );
            assert_eq!(world.calls.borrow().as_slice(), &[Lookup::ContactSlotZero]);
        }
    }

    #[test]
    fn unit_ready_to_commence_factory_fallback_uses_exact_signed_anchor_geometry() {
        let mission = mission_fixture(MISSION_GUARD.raw(), MISSION_GUARD.raw(), 0);
        let leaf_state = unit_leaf(0, 0, 0, 0);
        let leaf = leaf_state.as_unit().expect("Unit leaf");
        let exact_anchor = ReadyLeptonPoint::new(40 * 256, 50 * 256);

        let world = TestWorld {
            building: Ok(Some(UnitReadyBuilding::new(true, exact_anchor))),
            ..TestWorld::default()
        };
        let view = unit_view(&mission, leaf, &world);
        let position = view.position;
        assert_eq!(unit_ready_to_commence(view), Ok(false));
        assert_eq!(
            world.calls.borrow().as_slice(),
            &[Lookup::ContactSlotZero, Lookup::BuildingUnder(position)]
        );

        for position in [
            ReadyLeptonPoint::new(41 * 256, 51 * 256),
            ReadyLeptonPoint::new(40 * 256, 52 * 256),
        ] {
            let world = TestWorld {
                building: Ok(Some(UnitReadyBuilding::new(true, exact_anchor))),
                ..TestWorld::default()
            };
            let mut view = unit_view(&mission, leaf, &world);
            view.position = position;
            assert_eq!(unit_ready_to_commence(view), Ok(true));
        }

        let world = TestWorld {
            building: Ok(Some(UnitReadyBuilding::new(false, exact_anchor))),
            ..TestWorld::default()
        };
        assert_eq!(
            unit_ready_to_commence(unit_view(&mission, leaf, &world)),
            Ok(true)
        );

        let world = TestWorld {
            building: Ok(Some(UnitReadyBuilding::new(
                true,
                ReadyLeptonPoint::new(-256, -256),
            ))),
            ..TestWorld::default()
        };
        let mut view = unit_view(&mission, leaf, &world);
        view.position = ReadyLeptonPoint::new(-511, -1);
        assert_eq!(unit_ready_to_commence(view), Ok(false));

        let wrapped_anchor = ReadyLeptonPoint::new(0, 0);
        let world = TestWorld {
            building: Ok(Some(UnitReadyBuilding::new(true, wrapped_anchor))),
            ..TestWorld::default()
        };
        let mut view = unit_view(&mission, leaf, &world);
        view.position = ReadyLeptonPoint::new(0x0100_0000, 0x0100_0100);
        assert_eq!(
            unit_ready_to_commence(view),
            Ok(false),
            "native compares and subtracts the low 16-bit converted cells"
        );
    }

    #[test]
    fn unit_ready_to_commence_propagates_world_availability_in_lookup_order() {
        let mission = mission_fixture(MISSION_GUARD.raw(), MissionId::NONE.raw(), 0);
        let leaf_state = unit_leaf(0, 0, 0, 0);
        let leaf = leaf_state.as_unit().expect("Unit leaf");

        let world = TestWorld {
            contact: Err(ReadyUnavailable::WorldLookup),
            ..TestWorld::default()
        };
        assert_eq!(
            unit_ready_to_commence(unit_view(&mission, leaf, &world)),
            Err(ReadyUnavailable::WorldLookup)
        );
        assert_eq!(world.calls.borrow().as_slice(), &[Lookup::ContactSlotZero]);

        let world = TestWorld {
            building: Err(ReadyUnavailable::WorldLookup),
            ..TestWorld::default()
        };
        assert_eq!(
            unit_ready_to_commence(unit_view(&mission, leaf, &world)),
            Err(ReadyUnavailable::WorldLookup)
        );
        assert_eq!(
            world.calls.borrow().as_slice(),
            &[
                Lookup::ContactSlotZero,
                Lookup::BuildingUnder(ReadyLeptonPoint::new(40 * 256, 51 * 256))
            ]
        );
    }

    #[test]
    fn infantry_ready_to_commence_preserves_early_branch_precedence() {
        for current in [MISSION_STICKY.raw(), MISSION_RESCUE.raw()] {
            let mission = mission_fixture(current, MissionId::NONE.raw(), 0);
            let leaf_state = MissionLeafState::infantry_raw_for_test(0, 42);
            let mut view =
                infantry_view(&mission, leaf_state.as_infantry().expect("Infantry leaf"));
            view.locomotor = None;
            assert_eq!(infantry_ready_to_commence(view), Ok(false));
        }

        let mission = mission_fixture(MISSION_GUARD.raw(), MissionId::NONE.raw(), 0);
        for (firing, falling) in [(1, 0), (0, 1)] {
            let leaf_state = MissionLeafState::infantry_raw_for_test(firing, 42);
            let mut view =
                infantry_view(&mission, leaf_state.as_infantry().expect("Infantry leaf"));
            view.object_is_falling_down = falling;
            view.locomotor = None;
            assert_eq!(infantry_ready_to_commence(view), Ok(false));
        }

        let leaf_state = MissionLeafState::infantry_raw_for_test(0, 42);
        let mut view = infantry_view(&mission, leaf_state.as_infantry().expect("Infantry leaf"));
        view.locomotor = None;
        assert_eq!(
            infantry_ready_to_commence(view),
            Err(ReadyUnavailable::Locomotor)
        );

        let queued_sticky = mission_fixture(MissionId::NONE.raw(), MISSION_STICKY.raw(), 0);
        let mut view = infantry_view(
            &queued_sticky,
            leaf_state.as_infantry().expect("Infantry leaf"),
        );
        view.locomotor = None;
        assert_eq!(
            infantry_ready_to_commence(view),
            Err(ReadyUnavailable::Locomotor),
            "the initial Sticky/Rescue gate reads raw current, not effective Mission"
        );
    }

    #[test]
    fn infantry_ready_to_commence_moving_mission_exceptions_are_exact() {
        let leaf_state = MissionLeafState::infantry_raw_for_test(0, 0);
        let leaf = leaf_state.as_infantry().expect("Infantry leaf");

        for current in [
            MISSION_GUARD.raw(),
            MISSION_HUNT.raw(),
            MISSION_ATTACK.raw(),
        ] {
            let mission = mission_fixture(current, MissionId::NONE.raw(), 0);
            let mut view = infantry_view(&mission, leaf);
            view.locomotor = Some(moving());
            assert_eq!(infantry_ready_to_commence(view), Ok(true));
        }

        let mission = mission_fixture(MISSION_MOVE.raw(), MissionId::NONE.raw(), 0);
        let mut view = infantry_view(&mission, leaf);
        view.locomotor = Some(moving());
        assert_eq!(infantry_ready_to_commence(view), Ok(false));

        let mission = mission_fixture(MISSION_ATTACK.raw(), MissionId::NONE.raw(), 0);
        let mut view = infantry_view(&mission, leaf);
        view.locomotor = Some(moving());
        view.attack_target_present = true;
        assert_eq!(infantry_ready_to_commence(view), Ok(false));
    }

    #[test]
    fn infantry_moving_exceptions_use_effective_mission_when_current_is_none() {
        let leaf_state = MissionLeafState::infantry_raw_for_test(0, 0);
        let leaf = leaf_state.as_infantry().expect("Infantry leaf");

        for queued in [
            MISSION_GUARD.raw(),
            MISSION_HUNT.raw(),
            MISSION_ATTACK.raw(),
        ] {
            let mission = mission_fixture(MissionId::NONE.raw(), queued, 0);
            let mut view = infantry_view(&mission, leaf);
            view.locomotor = Some(moving());
            assert_eq!(infantry_ready_to_commence(view), Ok(true));
        }

        let mission = mission_fixture(MissionId::NONE.raw(), MISSION_MOVE.raw(), 0);
        let mut view = infantry_view(&mission, leaf);
        view.locomotor = Some(moving());
        assert_eq!(infantry_ready_to_commence(view), Ok(false));

        let mission = mission_fixture(MissionId::NONE.raw(), MISSION_ATTACK.raw(), 0);
        let mut view = infantry_view(&mission, leaf);
        view.locomotor = Some(moving());
        view.attack_target_present = true;
        assert_eq!(infantry_ready_to_commence(view), Ok(false));
    }

    #[test]
    fn infantry_ready_to_commence_sweeps_complete_doing_permission_table() {
        const ALLOWED: [i32; 27] = [
            0, 1, 2, 3, 4, 6, 8, 9, 10, 16, 17, 18, 19, 22, 23, 24, 25, 26, 28, 29, 30, 33, 37, 38,
            39, 40, 41,
        ];
        const BLOCKED: [i32; 15] = [5, 7, 11, 12, 13, 14, 15, 20, 21, 27, 31, 32, 34, 35, 36];
        let mission = mission_fixture(MISSION_GUARD.raw(), MissionId::NONE.raw(), 0);

        for doing in ALLOWED {
            let leaf_state = MissionLeafState::infantry_raw_for_test(0, doing);
            assert_eq!(
                infantry_ready_to_commence(infantry_view(
                    &mission,
                    leaf_state.as_infantry().expect("Infantry leaf")
                )),
                Ok(true),
                "Doing {doing}"
            );
        }
        for doing in BLOCKED {
            let leaf_state = MissionLeafState::infantry_raw_for_test(0, doing);
            assert_eq!(
                infantry_ready_to_commence(infantry_view(
                    &mission,
                    leaf_state.as_infantry().expect("Infantry leaf")
                )),
                Ok(false),
                "Doing {doing}"
            );
        }
    }

    #[test]
    fn infantry_ready_to_commence_doing_minus_one_and_invalid_domain_are_distinct() {
        let mission = mission_fixture(MISSION_GUARD.raw(), MissionId::NONE.raw(), 0);
        let idle = MissionLeafState::infantry_raw_for_test(0, -1);
        assert_eq!(
            infantry_ready_to_commence(infantry_view(
                &mission,
                idle.as_infantry().expect("Infantry leaf")
            )),
            Ok(true)
        );

        for doing in [-2, 42, i32::MAX] {
            let leaf_state = MissionLeafState::infantry_raw_for_test(0, doing);
            assert_eq!(
                infantry_ready_to_commence(infantry_view(
                    &mission,
                    leaf_state.as_infantry().expect("Infantry leaf")
                )),
                Err(ReadyUnavailable::InvalidDoing(doing))
            );
        }
    }

    #[test]
    fn infantry_ready_to_commence_still_applies_doing_after_moving_guard_exception() {
        let mission = mission_fixture(MISSION_GUARD.raw(), MissionId::NONE.raw(), 0);
        let leaf_state = MissionLeafState::infantry_raw_for_test(0, 5);
        let mut view = infantry_view(&mission, leaf_state.as_infantry().expect("Infantry leaf"));
        view.locomotor = Some(moving());
        assert_eq!(infantry_ready_to_commence(view), Ok(false));
    }

    #[test]
    fn aircraft_ready_to_commence_action_and_transition_matrix_is_exact() {
        for current in [MISSION_STICKY.raw(), MISSION_RESCUE.raw()] {
            let mission = mission_fixture(current, MissionId::NONE.raw(), 0);
            let leaf_state = MissionLeafState::aircraft_raw_for_test(0, 1);
            assert_eq!(
                aircraft_ready_to_commence(AircraftReadyView {
                    mission: &mission,
                    leaf: leaf_state.as_aircraft().expect("Aircraft leaf"),
                }),
                Ok(false)
            );
        }

        for (current, action, transition, expected) in [
            (MISSION_GUARD.raw(), 0, 1, true),
            (MISSION_GUARD.raw(), 1, 1, false),
            (MISSION_GUARD.raw(), 0, 0, false),
            (MISSION_AIRCRAFT_ACTION_EXCEPTION.raw(), 1, 1, true),
            (MISSION_AIRCRAFT_ACTION_EXCEPTION.raw(), 255, 0, false),
        ] {
            let mission = mission_fixture(current, MissionId::NONE.raw(), 0);
            let leaf_state = MissionLeafState::aircraft_raw_for_test(action, transition);
            assert_eq!(
                aircraft_ready_to_commence(AircraftReadyView {
                    mission: &mission,
                    leaf: leaf_state.as_aircraft().expect("Aircraft leaf"),
                }),
                Ok(expected)
            );
        }
    }

    #[test]
    fn building_ready_to_commence_uses_only_nonzero_raw_latch() {
        for (raw, expected) in [(0, false), (1, true), (u8::MAX, true)] {
            let leaf_state = MissionLeafState::building_raw_for_test(raw);
            assert_eq!(
                building_ready_to_commence(BuildingReadyView {
                    leaf: leaf_state.as_building().expect("Building leaf"),
                }),
                Ok(expected)
            );
        }
        assert_eq!(base_ready_to_commence(), Ok(true));

        let default_leaf = MissionLeafState::for_entity_category(EntityCategory::Structure);
        assert_eq!(
            building_ready_to_commence(BuildingReadyView {
                leaf: default_leaf.as_building().expect("Building leaf"),
            }),
            Ok(false)
        );
    }
}
