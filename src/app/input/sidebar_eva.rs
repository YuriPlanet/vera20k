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
/// gamemd's literal at `0x006AAF9D`; Ares builds play the type's
/// `EVA.SelectTarget` instead, whose constructor default is this name.
#[cfg(not(feature = "ares"))]
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
///   category's build (`FactoryClass::IsInQueue @ 0x004CA6B0` at
///   `0x006AB1B0`): the sound and the abandon, no line
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
    cameo_click_with_queue(
        queue,
        category,
        type_id,
        own,
        request,
        press,
        at_build_limit,
        false,
    )
}

/// [`cameo_click`] with Phobos `[General] BuildingProductionQueue=`. When
/// enabled, a busy structure strip accepts a left press instead of refusing it
/// with `EVA_UnableToComply`; the sim owner appends the queued building.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cameo_click_with_queue(
    queue: &[QueueItemView],
    category: ProductionCategory,
    type_id: Option<InternedId>,
    own: Option<BuildQueueState>,
    request: Option<CameoRequestState>,
    press: CameoPress,
    at_build_limit: bool,
    building_production_queue: bool,
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
            if busy && is_structure_strip(category) && !building_production_queue {
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

/// VERA's own Shift + right click on a running build: abandon it and every
/// queued copy at once (`EVA_Canceled`, ABANDON_ALL), where gamemd's
/// `0x006AAFE1..0x006AB02A` only holds it and a second press cancels.
///
/// Not native. The caller applies it only while the player's
/// `[VERA] ShiftQueue=` option is on, so the default stays the ported click.
pub(crate) fn shift_right_cancels_running(click: CameoClick, press: CameoPress) -> CameoClick {
    if press == (CameoPress::Right { shift: true }) && click.order == Some(CameoOrder::Suspend) {
        CameoClick {
            eva: Some(EVA_CANCELED),
            order: Some(CameoOrder::Abandon { all: true }),
            ..click
        }
    } else {
        click
    }
}

/// VERA's own Alt + left click on a unit cameo: put the new copies at the
/// front of the strip's waiting queue, right behind the running build.
///
/// Answers the waiting builds the copies must jump, in queue order, and how
/// many copies fit; the caller abandons one copy of each (ABANDON takes the
/// first waiting copy, `HouseClass::Abandon_Production @ 0x004FAA10`
/// `0x004FAAEE`, and a waiting copy was never charged), produces the new
/// copies and then produces the jumped ones again, all as ordinary events in
/// one frame. Waiting copies of the clicked type at the front already stand
/// there and are not jumped. `None`: nothing to jump, or no room in
/// `[General] MaximumQueuedObjects=` for one more copy, where the plain
/// PRODUCE keeps its native answer.
///
/// Not native: `SelectClass::Action @ 0x006AAD00` ignores modifiers on a left
/// press. The caller applies it only while `[VERA] ShiftQueue=` is on.
pub(crate) fn front_of_queue(
    queue: &[QueueItemView],
    category: ProductionCategory,
    type_id: InternedId,
    copies: u32,
    max_queued: i32,
) -> Option<(Vec<InternedId>, u32)> {
    let waiting: Vec<InternedId> = queue
        .iter()
        .filter(|item| item.queue_category == category && item.state == BuildQueueState::Queued)
        .map(|item| item.type_id)
        .collect();
    let ahead = waiting
        .iter()
        .take_while(|&&queued| queued == type_id)
        .count();
    let jumped = waiting[ahead..].to_vec();
    let room = u32::try_from(i64::from(max_queued) - waiting.len() as i64).unwrap_or(0);
    let copies = copies.max(1).min(room);
    (!jumped.is_empty() && copies > 0).then_some((jumped, copies))
}

/// RTTI 7 (BuildingType): the structure and defense strips.
pub(crate) fn is_structure_strip(category: ProductionCategory) -> bool {
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
    shift_queue_repeat_with_queue(category, own, shift, count, false)
}

/// [`shift_queue_repeat`] with the Phobos building queue switch. A structure
/// strip appends copies only when the queue is enabled; otherwise it keeps
/// the native single PRODUCE.
pub(crate) fn shift_queue_repeat_with_queue(
    category: ProductionCategory,
    own: Option<BuildQueueState>,
    shift: bool,
    count: u32,
    building_production_queue: bool,
) -> u32 {
    let appends_a_copy = !matches!(own, Some(BuildQueueState::Paused | BuildQueueState::Done))
        && (building_production_queue || !is_structure_strip(category));
    if shift && appends_a_copy {
        count.max(1)
    } else {
        1
    }
}

/// Copies of the PRODUCE event Ares 3.0 queues for a Shift press:
/// `((modifier & 1) << 2) | 1`.
#[cfg(feature = "ares")]
pub(crate) const ARES_SHIFT_PRODUCE_COPIES: u32 = 5;

/// How many PRODUCE events a left press on a build cameo queues, from the
/// press's Shift state and the player's `[VERA] ShiftQueue=` options.
///
/// In a default build Shift repeats only when the player ticked Shift-Click
/// Batch Queue (`vera_shift_queue`), `vera_count` times. An `ares` build also
/// queues copies without the option: Ares 3.0
/// `SelectClass_ProcessInput_ProduceUnsuspended` (Ares.dll 10064520, hook
/// 0x6AB773) replaces the single insertion of `SelectClass::Action` 0x006AAD00's
/// fresh/append PRODUCE event (the one after `CheckBuildLimit`) with a loop of
/// [`ARES_SHIFT_PRODUCE_COPIES`] copies when the key modifier's Shift bit is
/// set. The resume of a held build (0x006AB4E8) and the placement arms are other
/// events and keep their single copy, as in [`shift_queue_repeat_with_queue`].
/// The player's own count, when the option is on, replaces Ares' five.
pub(crate) fn produce_copies(
    category: ProductionCategory,
    own: Option<BuildQueueState>,
    shift: bool,
    vera_shift_queue: bool,
    vera_count: u32,
    building_production_queue: bool,
) -> u32 {
    #[cfg(feature = "ares")]
    let (shift_repeats, count) = (
        shift,
        if vera_shift_queue {
            vera_count
        } else {
            ARES_SHIFT_PRODUCE_COPIES
        },
    );
    #[cfg(not(feature = "ares"))]
    let (shift_repeats, count) = (shift && vera_shift_queue, vera_count);
    shift_queue_repeat_with_queue(
        category,
        own,
        shift_repeats,
        count,
        building_production_queue,
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

    /// Without the Options tick a default build never repeats; an `ares` build
    /// queues Ares' five on Shift (Ares.dll 10064520), in the same arms only.
    #[test]
    fn shift_queues_without_the_option_only_in_an_ares_build() {
        let copies = |category, own, shift| produce_copies(category, own, shift, false, 8, false);
        let expected = if cfg!(feature = "ares") { 5 } else { 1 };
        assert_eq!(copies(ProductionCategory::Infantry, None, true), expected);
        assert_eq!(copies(ProductionCategory::Infantry, None, false), 1);
        // A held build resumes and a structure strip never appends.
        assert_eq!(
            copies(
                ProductionCategory::Vehicle,
                Some(BuildQueueState::Paused),
                true
            ),
            1
        );
        assert_eq!(copies(ProductionCategory::Building, None, true), 1);
        // The ticked option keeps the player's own count in every build.
        assert_eq!(
            produce_copies(ProductionCategory::Vehicle, None, true, true, 8, false),
            8
        );
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

    /// Phobos `BuildingProductionQueue`: a busy building strip accepts a
    /// left press and lets the sim queue; without it the stock
    /// `EVA_UnableToComply` refusal remains.
    #[test]
    fn building_queue_switch_admits_a_busy_structure_press() {
        use crate::sim::production::{BuildQueueState, QueueItemView};
        let mut interner = StringInterner::new();
        let running = interner.intern("GAPOWR");
        let clicked = interner.intern("GAPILL");
        let queue = [QueueItemView {
            type_id: running,
            display_name: "GAPOWR".to_owned(),
            queue_category: ProductionCategory::Building,
            state: BuildQueueState::Building,
            progress: 10,
        }];
        let refused = cameo_click(
            &queue,
            ProductionCategory::Building,
            Some(clicked),
            None,
            None,
            CameoPress::Left,
            false,
        );
        assert_eq!(refused.order, None);
        assert_eq!(refused.eva, Some(EVA_UNABLE_TO_COMPLY));
        assert!(!refused.sound);

        let queued = cameo_click_with_queue(
            &queue,
            ProductionCategory::Building,
            Some(clicked),
            None,
            None,
            CameoPress::Left,
            false,
            true,
        );
        assert_eq!(queued.order, Some(CameoOrder::Produce));
        assert_eq!(queued.eva, None);
        assert!(queued.sound);

        assert_eq!(
            shift_queue_repeat_with_queue(ProductionCategory::Building, None, true, 5, true),
            5
        );
        assert_eq!(
            shift_queue_repeat_with_queue(ProductionCategory::Building, None, true, 5, false),
            1
        );
    }

    #[test]
    fn shift_right_on_a_running_build_abandons_every_copy() {
        let running = cameo_click(
            &[],
            ProductionCategory::Infantry,
            None,
            Some(BuildQueueState::Building),
            None,
            CameoPress::Right { shift: true },
            false,
        );
        assert_eq!(running.order, Some(CameoOrder::Suspend), "native holds");
        let vera = shift_right_cancels_running(running, CameoPress::Right { shift: true });
        assert_eq!(vera.order, Some(CameoOrder::Abandon { all: true }));
        assert_eq!(vera.eva, Some(EVA_CANCELED));
        assert!(vera.sound && vera.clear_building_placement);

        // A plain right press still only holds.
        let plain = CameoPress::Right { shift: false };
        assert_eq!(shift_right_cancels_running(running, plain), running);
    }

    #[test]
    fn alt_click_jumps_the_waiting_builds_of_other_types() {
        let mut interner = StringInterner::new();
        let (gi, dog, eng) = (
            interner.intern("E1"),
            interner.intern("ADOG"),
            interner.intern("ENGINEER"),
        );
        let item = |type_id, state| QueueItemView {
            type_id,
            display_name: String::new(),
            queue_category: ProductionCategory::Infantry,
            state,
            progress: 0,
        };
        let queue = [
            item(dog, BuildQueueState::Building),
            item(gi, BuildQueueState::Queued),
            item(eng, BuildQueueState::Queued),
            item(gi, BuildQueueState::Queued),
        ];
        let infantry = ProductionCategory::Infantry;
        // The running dog stays; the leading G.I. already stands at the front.
        assert_eq!(
            front_of_queue(&queue, infantry, gi, 1, 5),
            Some((vec![eng, gi], 1))
        );
        assert_eq!(
            front_of_queue(&queue, infantry, eng, 5, 5),
            Some((vec![gi, eng, gi], 2)),
            "Shift's copies are cut to the free places"
        );
        assert_eq!(
            front_of_queue(&queue, infantry, eng, 1, 3),
            None,
            "full queue"
        );
        assert_eq!(
            front_of_queue(&queue[..2], infantry, gi, 1, 5),
            None,
            "nothing to jump"
        );
    }
}
