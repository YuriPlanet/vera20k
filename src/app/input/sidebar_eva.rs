//! The sidebar's own click decision and the EVA lines it speaks —
//! `SelectClass::Action @ 0x006AAD00` (cameo clicks) and
//! `SidebarClass::AddCameo @ 0x006A63E0` / `StripClass::AddEntry @ 0x006A87F0`
//! (a new cameo). The handler plays its sound and EVA line on the clicking
//! machine before it queues the `EventClass`, so they are app events, not sim
//! events.
//!
//! Every call site passes type `-1` (`OR EDX,0xffffffff` at `0x006AAE31`,
//! `0x006AAF9F`, `0x006AAFFF`, `0x006AB100`, `0x006AB3A9`, `0x006AB490`,
//! `0x006AB68B`, `0x006AB6C1`, `0x006A640D`, `0x006A882F`): the entry's own
//! `Type=` / `Priority=` route them (stock: all STANDARD LOW except
//! `EVA_NewConstructionOptions`, QUEUE LOW). Strings: `0x83FB38` Building,
//! `0x83FB48` Training, `0x83FB58` UnableToComply, `0x83FB6C` OnHold,
//! `0x83FB78` SelectTarget, `0x83FB8C` Canceled, `0x83FA64`
//! NewConstructionOptions.
//!
//! `EVA_SelectTarget` (`0x006AAFA7`) is the superweapon cameo click — VERA's
//! `SidebarAction::ArmSuperWeapon` (`select_target_line`).

use crate::sim::intern::InternedId;
use crate::sim::production::{BuildQueueState, ProductionCategory, QueueItemView};

pub(crate) const EVA_BUILDING: &str = "EVA_Building";
pub(crate) const EVA_TRAINING: &str = "EVA_Training";
pub(crate) const EVA_ON_HOLD: &str = "EVA_OnHold";
pub(crate) const EVA_CANCELED: &str = "EVA_Canceled";
pub(crate) const EVA_UNABLE_TO_COMPLY: &str = "EVA_UnableToComply";
pub(crate) const EVA_SELECT_TARGET: &str = "EVA_SelectTarget";
pub(crate) const EVA_NEW_CONSTRUCTION_OPTIONS: &str = "EVA_NewConstructionOptions";
pub(crate) const EVA_CANNOT_DEPLOY_HERE: &str = "EVA_CannotDeployHere";

/// A press on a build cameo: the left button, or the right one with Shift's
/// state (modifier bit 0, read at `0x006AAD66`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CameoPress {
    Left,
    Right { shift: bool },
}

/// The event a cameo click queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CameoOrder {
    /// PRODUCE (event 14): start, queue or resume a build of this type.
    Produce,
    /// SUSPEND (event 15): hold the category's running build.
    Suspend,
    /// ABANDON (event 16), or ABANDON_ALL (event 46) with Shift.
    Abandon { all: bool },
    /// A finished building: `HouseClass::Manual_Place @ 0x004FB840` enters
    /// placement mode (`0x006AB3CF`).
    Place,
}

/// What one cameo click does on the clicking machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CameoClick {
    /// Select6AADC9..6AADF8 drops any active building placement before sound
    /// or event handling when this right-clicked cameo owns a factory.
    pub clear_building_placement: bool,
    /// `[AudioVisual] GUIBuildSound=` (`Rules+0x18C`).
    pub sound: bool,
    pub eva: Option<&'static str>,
    pub order: Option<CameoOrder>,
}

/// `SelectClass::Action @ 0x006AAD00` for a build cameo of `type_id` in
/// `category`, read against the local player's queue. Nothing here checks
/// money.
///
/// The cameo's own factory (its `CurrentFactory`, linked by Begin_Production
/// at `0x004FA6AA`) is the category's active build when that build is this
/// type; it runs when it has a rate and no hold (`+0x38 != 0 && !+0x70`).
///
/// * Right, own factory running: the sound, `EVA_OnHold` and SUSPEND
///   (`0x006AAFE1..0x006AB02A`). Held or finished: the sound,
///   `EVA_Canceled` (`0x006AAE39`) and ABANDON, or ABANDON_ALL with Shift
///   (`0x006AAE4A..0x006AAE68`). No own factory but a copy queued behind the
///   category's build: the sound and the abandon, no line
///   (`0x006AB1A0..0x006AB22A`).
/// * Left, own factory stopped: finished — the sound and, for a building,
///   placement (`0x006AB2BD..0x006AB3FC`; a unit's PLACE retry is VERA's
///   automatic delivery); held — the sound, the start line and PRODUCE, which
///   resumes it with no limit check (`0x006AB463..0x006AB511`).
/// * Left otherwise (`0x006AB602..`): the category is busy when it has any
///   build, running, held, finished or queued (`0x006AB663..0x006AB67D`). A
///   busy structure strip (RTTI 7, both structure tabs) answers
///   `EVA_UnableToComply` and nothing else (`0x006AB689..0x006AB698`); a type
///   at its build limit does nothing (`0x006AB6F5`); otherwise the sound
///   (`0x006AB713`), the start line when the category was idle
///   (`0x006AB69D..0x006AB6C9`) and PRODUCE (`0x006AB76E`).
///
/// Residual: a right click on a cameo whose PRODUCE has not executed yet
/// (cameo status 1, no factory linked) holds it on arrival
/// (`0x006AB0AC..0x006AB12B`); VERA has no pending cameo status, so that
/// click does nothing. Trigger: a right click within the command delay of a
/// build's first left click. Rare.
///
/// `own` resolves the retained strip's Factory link, including an older
/// cameo linked to a rearmed Factory. Right with that link first attempts to
/// drop a placement with a non-NULL product (`0x006AADC9..0x006AADF8`),
/// including another category's building. The queued-copy arm has no prefix. Source:
/// tools/sidebar_oracle/cameo_order.md, ready-building histories.
pub(crate) fn cameo_click(
    queue: &[QueueItemView],
    category: ProductionCategory,
    type_id: Option<InternedId>,
    own: Option<BuildQueueState>,
    press: CameoPress,
    at_build_limit: bool,
) -> CameoClick {
    let in_category = || queue.iter().filter(|item| item.queue_category == category);
    let click = |eva: Option<&'static str>, order: CameoOrder| CameoClick {
        clear_building_placement: false,
        sound: true,
        eva,
        order: Some(order),
    };
    let mut result = match (press, own) {
        (CameoPress::Right { .. }, Some(BuildQueueState::Building)) => {
            click(Some(EVA_ON_HOLD), CameoOrder::Suspend)
        }
        (CameoPress::Right { shift }, Some(_)) => {
            click(Some(EVA_CANCELED), CameoOrder::Abandon { all: shift })
        }
        (CameoPress::Right { shift }, None) => {
            let queued = in_category()
                .any(|item| item.state == BuildQueueState::Queued && Some(item.type_id) == type_id);
            if queued {
                click(None, CameoOrder::Abandon { all: shift })
            } else {
                CameoClick::default()
            }
        }
        (CameoPress::Left, Some(BuildQueueState::Done)) => {
            let structure = is_structure_strip(category);
            CameoClick {
                clear_building_placement: false,
                sound: true,
                eva: None,
                order: structure.then_some(CameoOrder::Place),
            }
        }
        (CameoPress::Left, Some(BuildQueueState::Paused)) => {
            click(Some(start_line_for(category)), CameoOrder::Produce)
        }
        (CameoPress::Left, _) => {
            let busy = in_category().next().is_some();
            if busy && is_structure_strip(category) {
                CameoClick {
                    clear_building_placement: false,
                    sound: false,
                    eva: Some(EVA_UNABLE_TO_COMPLY),
                    order: None,
                }
            } else if at_build_limit {
                CameoClick::default()
            } else {
                click(
                    (!busy).then(|| start_line_for(category)),
                    CameoOrder::Produce,
                )
            }
        }
    };
    result.clear_building_placement = matches!(press, CameoPress::Right { .. }) && own.is_some();
    result
}

/// RTTI 7 (BuildingType): the structure and defense strips.
fn is_structure_strip(category: ProductionCategory) -> bool {
    matches!(
        category,
        ProductionCategory::Building | ProductionCategory::Defense
    )
}

/// `0x006AB484..0x006AB498` / `0x006AB6A1..0x006AB6C9`: infantry trains,
/// everything else builds.
pub(crate) fn start_line_for(category: ProductionCategory) -> &'static str {
    if category == ProductionCategory::Infantry {
        EVA_TRAINING
    } else {
        EVA_BUILDING
    }
}

/// `SelectClass::Action 0x006AAED5..0x006AAFAC`, the superweapon strip
/// (`RTTI == 0x1F`) on a left click (`param_2 & 1`) of a cameo below the
/// house's super count (`[PlayerPtr+0x264]`). `0x006AAEEA CALL 0x006CC360`
/// is the readiness test: `SuperClass+0x70` (IsSuspended) set → false;
/// `Type+0xE5` (`UseChargeDrain=`) → `+0x7C != 0`; else `+0x6F` (IsReady).
/// Not ready → `0x006AAFB1` (a no-op) and nothing is spoken. Then
/// `0x006AAF08 CMP [Type+0xBC], 0` — `Action=` (`SuperWeaponTypeClass::
/// ReadINI 0x006CEA20`, `CCINIClass::ReadAction`, `None` = 0): a targeted
/// weapon arms the pending-super state (`[0x8809A0] = index`),
/// `Unselect_All`, and speaks `EVA_SelectTarget` (`0x006AAFA7`); an
/// untargeted one fires at once through event `0x12` (`0x006AAF3C`) and
/// stays silent. Every stock `[*Special]` carries an `Action=`.
///
/// `is_online` is VERA's `!is_suspended`; `is_ready` stands in for both the
/// charged flag and the charge-drain `+0x7C` state.
pub(crate) fn select_target_line(
    is_ready: bool,
    is_online: bool,
    action: Option<&str>,
) -> Option<&'static str> {
    let targeted = action.is_some_and(|action| !action.eq_ignore_ascii_case("none"));
    (is_ready && is_online && targeted).then_some(EVA_SELECT_TARGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        type_id: InternedId,
        category: ProductionCategory,
        state: BuildQueueState,
    ) -> QueueItemView {
        QueueItemView {
            type_id,
            display_name: String::new(),
            queue_category: category,
            state,
            progress: 0,
        }
    }

    const LEFT: CameoPress = CameoPress::Left;
    const RIGHT: CameoPress = CameoPress::Right { shift: false };
    const SHIFT_RIGHT: CameoPress = CameoPress::Right { shift: true };

    fn order(sound: bool, eva: Option<&'static str>, order: CameoOrder) -> CameoClick {
        CameoClick {
            clear_building_placement: false,
            sound,
            eva,
            order: Some(order),
        }
    }

    fn own_right_order(eva: Option<&'static str>, value: CameoOrder) -> CameoClick {
        CameoClick {
            clear_building_placement: true,
            ..order(true, eva, value)
        }
    }

    #[test]
    fn idle_strip_speaks_the_start_line_and_produces() {
        let a = InternedId::from_index(1);
        for (category, line) in [
            (ProductionCategory::Building, EVA_BUILDING),
            (ProductionCategory::Defense, EVA_BUILDING),
            (ProductionCategory::Infantry, EVA_TRAINING),
            (ProductionCategory::Vehicle, EVA_BUILDING),
            (ProductionCategory::Aircraft, EVA_BUILDING),
            (ProductionCategory::Ship, EVA_BUILDING),
        ] {
            assert_eq!(
                cameo_click(&[], category, Some(a), None, LEFT, false),
                order(true, Some(line), CameoOrder::Produce),
                "{category:?}"
            );
        }
    }

    #[test]
    fn busy_structure_strip_refuses_even_behind_a_finished_building() {
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
        for state in [
            BuildQueueState::Building,
            BuildQueueState::Paused,
            BuildQueueState::Done,
        ] {
            let queue = [item(a, ProductionCategory::Building, state)];
            assert_eq!(
                cameo_click(
                    &queue,
                    ProductionCategory::Building,
                    Some(b),
                    None,
                    LEFT,
                    false
                ),
                CameoClick {
                    clear_building_placement: false,
                    sound: false,
                    eva: Some(EVA_UNABLE_TO_COMPLY),
                    order: None
                },
                "{state:?}"
            );
        }
    }

    #[test]
    fn busy_unit_strip_queues_silently_even_at_a_finished_head() {
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
        for state in [
            BuildQueueState::Building,
            BuildQueueState::Paused,
            BuildQueueState::Done,
        ] {
            let queue = [item(a, ProductionCategory::Vehicle, state)];
            assert_eq!(
                cameo_click(
                    &queue,
                    ProductionCategory::Vehicle,
                    Some(b),
                    None,
                    LEFT,
                    false
                ),
                order(true, None, CameoOrder::Produce),
                "{state:?}"
            );
        }
        // The running build's own cameo queues another copy.
        let queue = [item(
            a,
            ProductionCategory::Vehicle,
            BuildQueueState::Building,
        )];
        assert_eq!(
            cameo_click(
                &queue,
                ProductionCategory::Vehicle,
                Some(a),
                Some(BuildQueueState::Building),
                LEFT,
                false
            ),
            order(true, None, CameoOrder::Produce)
        );
    }

    #[test]
    fn a_type_at_its_build_limit_does_nothing_unless_it_is_held() {
        let a = InternedId::from_index(1);
        assert_eq!(
            cameo_click(&[], ProductionCategory::Infantry, Some(a), None, LEFT, true),
            CameoClick::default()
        );
        let held = [item(
            a,
            ProductionCategory::Infantry,
            BuildQueueState::Paused,
        )];
        assert_eq!(
            cameo_click(
                &held,
                ProductionCategory::Infantry,
                Some(a),
                Some(BuildQueueState::Paused),
                LEFT,
                true
            ),
            order(true, Some(EVA_TRAINING), CameoOrder::Produce),
            "a held build resumes without a limit check"
        );
    }

    #[test]
    fn right_click_holds_the_running_build_then_cancels_it() {
        let a = InternedId::from_index(1);
        let running = [item(
            a,
            ProductionCategory::Vehicle,
            BuildQueueState::Building,
        )];
        assert_eq!(
            cameo_click(
                &running,
                ProductionCategory::Vehicle,
                Some(a),
                Some(BuildQueueState::Building),
                RIGHT,
                false
            ),
            own_right_order(Some(EVA_ON_HOLD), CameoOrder::Suspend)
        );
        assert_eq!(
            cameo_click(
                &running,
                ProductionCategory::Vehicle,
                Some(a),
                Some(BuildQueueState::Building),
                SHIFT_RIGHT,
                false
            ),
            own_right_order(Some(EVA_ON_HOLD), CameoOrder::Suspend),
            "Shift does not skip the hold"
        );
        for state in [BuildQueueState::Paused, BuildQueueState::Done] {
            let stopped = [item(a, ProductionCategory::Vehicle, state)];
            assert_eq!(
                cameo_click(
                    &stopped,
                    ProductionCategory::Vehicle,
                    Some(a),
                    Some(state),
                    RIGHT,
                    false
                ),
                own_right_order(Some(EVA_CANCELED), CameoOrder::Abandon { all: false }),
                "{state:?}"
            );
            assert_eq!(
                cameo_click(
                    &stopped,
                    ProductionCategory::Vehicle,
                    Some(a),
                    Some(state),
                    SHIFT_RIGHT,
                    false
                ),
                own_right_order(Some(EVA_CANCELED), CameoOrder::Abandon { all: true }),
                "{state:?}"
            );
        }
    }

    #[test]
    fn right_click_on_a_queued_copy_abandons_it_without_a_line() {
        let a = InternedId::from_index(1);
        let b = InternedId::from_index(2);
        let queue = [
            item(a, ProductionCategory::Infantry, BuildQueueState::Building),
            item(b, ProductionCategory::Infantry, BuildQueueState::Queued),
        ];
        assert_eq!(
            cameo_click(
                &queue,
                ProductionCategory::Infantry,
                Some(b),
                None,
                SHIFT_RIGHT,
                false
            ),
            order(true, None, CameoOrder::Abandon { all: true })
        );
        let c = InternedId::from_index(3);
        assert_eq!(
            cameo_click(
                &queue,
                ProductionCategory::Infantry,
                Some(c),
                None,
                RIGHT,
                false
            ),
            CameoClick::default(),
            "a cameo with nothing built or queued ignores the right click"
        );
        assert_eq!(
            cameo_click(
                &queue,
                ProductionCategory::Vehicle,
                Some(b),
                None,
                RIGHT,
                false
            ),
            CameoClick::default(),
            "only the cameo's own category counts"
        );
    }

    #[test]
    fn left_click_resumes_a_held_build_and_places_a_finished_building() {
        let a = InternedId::from_index(1);
        let held = [item(
            a,
            ProductionCategory::Vehicle,
            BuildQueueState::Paused,
        )];
        assert_eq!(
            cameo_click(
                &held,
                ProductionCategory::Vehicle,
                Some(a),
                Some(BuildQueueState::Paused),
                LEFT,
                false
            ),
            order(true, Some(EVA_BUILDING), CameoOrder::Produce)
        );
        let ready = [item(a, ProductionCategory::Defense, BuildQueueState::Done)];
        assert_eq!(
            cameo_click(
                &ready,
                ProductionCategory::Defense,
                Some(a),
                Some(BuildQueueState::Done),
                LEFT,
                false
            ),
            order(true, None, CameoOrder::Place)
        );
        let unit = [item(a, ProductionCategory::Vehicle, BuildQueueState::Done)];
        assert_eq!(
            cameo_click(
                &unit,
                ProductionCategory::Vehicle,
                Some(a),
                Some(BuildQueueState::Done),
                LEFT,
                false
            ),
            CameoClick {
                clear_building_placement: false,
                sound: true,
                eva: None,
                order: None
            },
            "a finished unit's PLACE retry is VERA's automatic delivery"
        );
    }

    #[test]
    fn super_weapon_click_speaks_select_target_only_when_ready_and_targeted() {
        assert_eq!(
            select_target_line(true, true, Some("LightningStorm")),
            Some(EVA_SELECT_TARGET)
        );
        assert_eq!(select_target_line(false, true, Some("Nuke")), None);
        assert_eq!(
            select_target_line(true, false, Some("IronCurtain")),
            None,
            "`+0x70` IsSuspended refuses the click"
        );
        assert_eq!(select_target_line(true, true, None), None);
        assert_eq!(
            select_target_line(true, true, Some("None")),
            None,
            "`Action=None` fires through event 0x12 and stays silent"
        );
    }
}
