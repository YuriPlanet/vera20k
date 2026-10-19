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
//! `superweapon_cameo` owns its live admission and mode changes.

use crate::sim::intern::InternedId;
use crate::sim::production::{BuildQueueState, ProductionCategory, QueueItemView};
use crate::ui::sidebar::cameo_order::CameoRequestState;

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
    /// A finished product: Select6AB31A queries its builder. A building enters
    /// ManualPlace4FB840; a mobile queues PLACE11. No builder queues indexed
    /// ABANDON16 and EVA_UnableToComply instead.
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
/// * Left, own factory stopped: finished — the sound and a builder lookup,
///   followed by building placement, mobile PLACE, or ABANDON plus refusal
///   (`0x006AB2BD..0x006AB3FC`); held — the sound, the start line and PRODUCE, which
///   resumes it with no limit check (`0x006AB463..0x006AB511`).
/// * Left otherwise (`0x006AB602..`): the category is busy when it has any
///   build, running, held, finished or queued (`0x006AB663..0x006AB67D`). A
///   busy structure strip (RTTI 7, both structure tabs) answers
///   `EVA_UnableToComply` and nothing else (`0x006AB689..0x006AB698`); a type
///   at its build limit does nothing (`0x006AB6F5`); otherwise the sound
///   (`0x006AB713`), the start line when the category was idle
///   (`0x006AB69D..0x006AB6C9`) and PRODUCE (`0x006AB76E`).
///
/// With no Factory link, pending request1 takes the sound/OnHold/SUSPEND
/// arm6AB0AC..6AB12B before the queued-copy query. Request2 alone is silent.
/// Shift does not alter that SUSPEND. The combined pending1+queued-copy arm
/// and saturated native OutList are outside the ordinary input domain here.
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
    request: Option<CameoRequestState>,
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
            if request == Some(CameoRequestState::Started) {
                click(Some(EVA_ON_HOLD), CameoOrder::Suspend)
            } else if queued {
                click(None, CameoOrder::Abandon { all: shift })
            } else {
                CameoClick::default()
            }
        }
        (CameoPress::Left, Some(BuildQueueState::Done)) => click(None, CameoOrder::Place),
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

/// How many PRODUCE events one click queues.
///
/// This is VERA's own convenience, not native behavior. gamemd's
/// `SelectClass::Action @ 0x006AAD00` reads the Shift bit for the right button
/// only (`0x006AAD66`); a left press queues exactly one copy whatever the
/// modifier state. So an unmodified press returns 1 and `cameo_click` stays a
/// faithful port of the native decision.
///
/// Shift expands the press only where the native click really appends a copy: a
/// mobile strip (Infantry/Vehicle/Aircraft) whose category is starting from
/// idle. The other arms take their own single event and keep it:
/// a structure strip, whose busy click is `EVA_UnableToComply` and whose
/// `Begin_Production` never queues a second building; and a held or finished
/// build, which resumes or places rather than appending.
///
/// `shift` is the Shift bit only while the player's `[VERA] ShiftQueue=` is on;
/// the caller clears it otherwise. `count` is the configured
/// `[VERA] ShiftQueueCount=`, already clamped by the profile owner. The
/// expansion is deliberately not capped further here: each copy is an
/// ordinary PRODUCE event, so `FactoryClass::StartProduction`'s own
/// `[General] MaximumQueuedObjects=` check still refuses the extras, exactly as
/// repeated fast clicks would.
pub(crate) fn shift_queue_repeat(
    category: ProductionCategory,
    own: Option<BuildQueueState>,
    shift: bool,
    count: u32,
) -> u32 {
    let appends_a_copy = !is_structure_strip(category)
        && !matches!(own, Some(BuildQueueState::Paused | BuildQueueState::Done));
    if shift && appends_a_copy {
        count.max(1)
    } else {
        1
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::intern::StringInterner;

    #[test]
    fn a_right_press_holds_a_requested_infantry_head_without_a_factory_link() {
        let mut interner = StringInterner::new();
        let e2 = interner.intern("E2");
        let first = cameo_click(
            &[],
            ProductionCategory::Infantry,
            Some(e2),
            None,
            None,
            CameoPress::Left,
            false,
        );
        assert_eq!(first.order, Some(CameoOrder::Produce));

        // Original6AB0AC..6AB12B reads the retained request before the
        // PRODUCE event has constructed or linked a Factory. Shift belongs
        // to the later queued-copy arm, not this SUSPEND request.
        for shift in [false, true] {
            let right = cameo_click(
                &[],
                ProductionCategory::Infantry,
                Some(e2),
                None,
                Some(CameoRequestState::Started),
                CameoPress::Right { shift },
                false,
            );
            assert_eq!(
                right.order,
                Some(CameoOrder::Suspend),
                "a pending E2 must hold on arrival even before Factory linkage"
            );
            assert!(right.sound);
            assert_eq!(right.eva, Some(EVA_ON_HOLD));
            assert!(!right.clear_building_placement);
        }
    }

    /// VERA's Shift repeat is an app convenience layered after the native
    /// click, so every arm the native click gives a different single event
    /// keeps exactly one PRODUCE.
    #[test]
    fn shift_repeats_only_where_the_native_click_appends_a_copy() {
        // The caller supplies the configured count, so any value proves the
        // arm decision without pinning the default here.
        let n = 5;
        // A mobile strip starting from idle: the native click appends, so
        // Shift expands it.
        for category in [
            ProductionCategory::Infantry,
            ProductionCategory::Vehicle,
            ProductionCategory::Aircraft,
        ] {
            assert_eq!(
                shift_queue_repeat(category, None, true, n),
                n,
                "{category:?}"
            );
            // Without the modifier the native click is unchanged.
            assert_eq!(
                shift_queue_repeat(category, None, false, n),
                1,
                "{category:?}"
            );
        }

        // Both structure strips: a busy click is EVA_UnableToComply, and
        // Begin_Production never queues a second building, so Shift must not
        // multiply the event.
        for category in [ProductionCategory::Building, ProductionCategory::Defense] {
            assert_eq!(
                shift_queue_repeat(category, None, true, n),
                1,
                "{category:?}"
            );
        }

        // A held build takes the resume arm and a finished one the placement
        // arm; neither appends, so neither expands even on a mobile strip.
        for own in [BuildQueueState::Paused, BuildQueueState::Done] {
            assert_eq!(
                shift_queue_repeat(ProductionCategory::Vehicle, Some(own), true, n),
                1,
                "{own:?}"
            );
        }

        // A running build still appends behind itself, so Shift expands there.
        assert_eq!(
            shift_queue_repeat(
                ProductionCategory::Vehicle,
                Some(BuildQueueState::Building),
                true,
                n
            ),
            n
        );

        // A configured 0 must still send the click's own PRODUCE.
        assert_eq!(
            shift_queue_repeat(ProductionCategory::Infantry, None, true, 0),
            1
        );
    }
}
