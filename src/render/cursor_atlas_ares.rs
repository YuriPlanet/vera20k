//! Ares 3.0 draws every software cursor through its `[MouseCursors]` list
//! ([`crate::rules::ares_mouse_cursor`]) instead of the compiled table:
//! - `MouseClass_UpdateCursor @ 10007AC0` (hook `0x5BDC8C`): the cursor's
//!   entry, its radar frames when the radar asks (`miniMap`) and the entry has
//!   some (`MiniFrame >= 0`), else its main frames; remembered as current.
//! - `MouseClass_UpdateCursorMinimapState_UseCursor @ 10007B50`
//!   (`0x5BDADF`), `MouseClass_Update_AnimateCursor @ 10007B70` (`0x5BDDC8`)
//!   and `MouseClass_Update_AnimateCursor2 @ 10007BA0` (`0x5BDE64`): the
//!   animation reads the current entry's Interval (0 stays still) and its
//!   Count or MiniCount.
//! - `MouseClass_GetCursorCurrentFrame @ 10007990` (`0x5BDBC4`) and
//!   `MouseClass_GetCursorHotSpot @ 10007A80` (`0x5BDC1B`): the drawn frame
//!   and its hotspot come from the same entry.
//! - `MouseClass_GetCursorFirstFrame_Minimap @ 10007A00` (`0x5BDB90`): the
//!   planning badges' frames, rows 76 and 77.
//!
//! Each stock [`CursorId`] names a row of the compiled table, or that row's
//! radar set ([`vanilla_row`]); drawn through the list it shows that row's
//! entry, so a `[MouseCursors]` line redefining a vanilla name changes the
//! stock cursor too. `0x4AB366`, `0x4AB44A` and `0x5BDD9F` were 0.A hooks
//! that 3.0 no longer installs.

use std::collections::HashMap;

use super::{
    CURSOR_TIMER_UNIT_MS, CursorHotspot, CursorId, PlanningBadge, SoftwareCursor,
    SoftwareCursorFrame, SoftwareCursorSequence, build_cursor_frame, frame_to_canvas_rgba,
    load_mouse_shape, resolve_hotspot,
};
use crate::assets::asset_manager::AssetManager;
use crate::render::batch::BatchRenderer;
use crate::render::gpu::GpuContext;
use crate::rules::ares_mouse_cursor::{HOTSPOT_CENTRE, HOTSPOT_FAR, MouseCursor, MouseCursors};

/// The compiled-table row each stock id draws, and whether it is the row's
/// radar (`mini`) set; `None` for [`CursorId::Ares`]. The ranges in
/// `CURSOR_DEFS` are these rows' (`stock_ids_draw_their_rows_through_the_list`).
pub(crate) const fn vanilla_row(id: CursorId) -> Option<(u16, bool)> {
    use CursorId as Id;
    Some(match id {
        Id::Default => (0, false),
        Id::DefaultMini => (0, true),
        Id::ScrollN => (1, false),
        Id::ScrollNE => (2, false),
        Id::ScrollE => (3, false),
        Id::ScrollSE => (4, false),
        Id::ScrollS => (5, false),
        Id::ScrollSW => (6, false),
        Id::ScrollW => (7, false),
        Id::ScrollNW => (8, false),
        Id::NoMoveN => (9, false),
        Id::NoMoveNE => (10, false),
        Id::NoMoveE => (11, false),
        Id::NoMoveSE => (12, false),
        Id::NoMoveS => (13, false),
        Id::NoMoveSW => (14, false),
        Id::NoMoveW => (15, false),
        Id::NoMoveNW => (16, false),
        Id::Select => (17, false),
        Id::Move => (18, false),
        Id::MinimapMove => (18, true),
        Id::NoMove => (19, false),
        Id::MinimapNoMove => (19, true),
        Id::Attack => (20, false),
        Id::MinimapAttack => (20, true),
        Id::AttackOutOfRange => (21, false),
        Id::GuardArea => (22, false),
        Id::MinimapGuardArea => (22, true),
        Id::DesolatorDeploy => (23, false),
        Id::Enter => (25, false),
        Id::MinimapEnter => (25, true),
        Id::NoEnter => (26, false),
        Id::MinimapNoEnter => (26, true),
        Id::Deploy => (27, false),
        Id::NoDeploy => (28, false),
        Id::Sell => (30, false),
        Id::SellUnit => (31, false),
        Id::NoSell => (32, false),
        Id::Repair => (33, false),
        Id::EngineerRepair => (34, false),
        Id::NoRepair => (35, false),
        Id::Disguise => (37, false),
        Id::IvanBomb => (38, false),
        Id::MindControl => (39, false),
        Id::RemoveSquid => (40, false),
        Id::Crush => (41, false),
        Id::SpyTech => (42, false),
        Id::SpyPower => (43, false),
        Id::GIDeploy => (45, false),
        Id::Paradrop => (47, false),
        Id::ParadropMini => (47, true),
        Id::LightningStorm => (50, false),
        Id::LightningStormMini => (50, true),
        Id::Detonate => (51, false),
        Id::Demolish => (52, false),
        Id::Nuke => (53, false),
        Id::NukeMini => (53, true),
        Id::Tote => (54, false),
        Id::TogglePower => (55, false),
        Id::IronCurtain => (57, false),
        Id::Chronosphere => (58, false),
        Id::Disarm => (59, false),
        Id::NoTogglePower => (60, false),
        Id::Pan => (61, false),
        Id::PanN => (62, false),
        Id::PanNE => (63, false),
        Id::PanE => (64, false),
        Id::PanSE => (65, false),
        Id::PanS => (66, false),
        Id::PanSW => (67, false),
        Id::PanW => (68, false),
        Id::PanNW => (69, false),
        Id::AttackMove => (71, false),
        Id::InfantryAbsorb => (73, false),
        Id::NoMindControl => (74, false),
        Id::Beacon => (78, false),
        Id::ForceShield => (79, false),
        Id::NoForceShield => (80, false),
        Id::GeneticMutator => (81, false),
        Id::AirStrike => (82, false),
        Id::PsychicDominator => (83, false),
        Id::PsychicDominatorMini => (83, true),
        Id::PsychicReveal => (84, false),
        Id::PsychicRevealMini => (84, true),
        Id::SpyPlane => (85, false),
        Id::SpyPlaneMini => (85, true),
        Id::Ares { .. } => return None,
    })
}

/// [`SoftwareCursor::get`] for a cursor built from the list: the id's entry
/// set, its main set when the entry has no radar frames (`0x5BDC8C`), else
/// the arrow.
pub(super) fn lookup(
    sequences: &HashMap<CursorId, SoftwareCursorSequence>,
    id: CursorId,
) -> Option<&SoftwareCursorSequence> {
    let (index, mini) = match id {
        CursorId::Ares { index, mini } => (index, mini),
        stock => vanilla_row(stock)?,
    };
    sequences
        .get(&CursorId::Ares { index, mini })
        .or_else(|| sequences.get(&CursorId::Ares { index, mini: false }))
        .or_else(|| {
            sequences.get(&CursorId::Ares {
                index: 0,
                mini: false,
            })
        })
}

/// The frames an entry draws: its first frame, frame count and interval in
/// milliseconds, for its main set or its radar set. A count below one (the
/// static `-1` radar sets of rows 47 and 83) draws one still frame; Ares
/// divides by a zero count. A negative interval steps on every timer unit.
pub(super) fn layout(cursor: &MouseCursor, mini: bool) -> Option<(usize, usize, u64)> {
    let (frame, count) = if mini && cursor.mini_frame >= 0 {
        (cursor.mini_frame, cursor.mini_count)
    } else {
        (cursor.frame, cursor.count)
    };
    let count = usize::try_from(count).unwrap_or(0).max(1);
    let interval = match cursor.interval {
        0 => 0,
        interval => u64::try_from(interval).unwrap_or(1),
    };
    Some((
        usize::try_from(frame).ok()?,
        count,
        interval * CURSOR_TIMER_UNIT_MS,
    ))
}

/// The entry's hotspot sentinels (`0x5BDC1B`), each 0 (near), 12345 (centre)
/// or 54321 (far); any other value takes the near edge.
pub(super) fn hotspot(cursor: &MouseCursor) -> CursorHotspot {
    use CursorHotspot as H;
    let axis = |value: i32| match value {
        HOTSPOT_CENTRE => 1,
        HOTSPOT_FAR => 2,
        _ => 0,
    };
    match (axis(cursor.hot_x), axis(cursor.hot_y)) {
        (0, 0) => H::TopLeft,
        (1, 0) => H::CenterTop,
        (2, 0) => H::RightTop,
        (2, 1) => H::RightMiddle,
        (2, 2) => H::RightBottom,
        (1, 2) => H::CenterBottom,
        (0, 2) => H::LeftBottom,
        (0, 1) => H::LeftMiddle,
        _ => H::CenterMiddle,
    }
}

/// The match cursor of an Ares build: every entry of `cursors` with its main
/// set and, when it has radar frames, its radar set, keyed by
/// [`CursorId::Ares`]. Frames shared by several entries share one texture.
pub(crate) fn build_ares_software_cursor(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
    cursors: &MouseCursors,
) -> Option<SoftwareCursor> {
    let (shp, palette) = load_mouse_shape(asset_manager)?;
    let mut frames: HashMap<usize, Option<SoftwareCursorFrame>> = HashMap::new();
    let mut sequences = HashMap::new();
    for (index, cursor) in cursors.iter() {
        let Ok(index) = u16::try_from(index) else {
            continue;
        };
        for mini in [false, true] {
            if mini && cursor.mini_frame < 0 {
                continue;
            }
            let Some((start, count, interval_ms)) = layout(cursor, mini) else {
                continue;
            };
            let sequence_frames: Vec<SoftwareCursorFrame> = (start..start + count)
                .filter_map(|frame| {
                    frames
                        .entry(frame)
                        .or_insert_with(|| build_cursor_frame(gpu, batch, &shp, &palette, frame))
                        .clone()
                })
                .collect();
            if sequence_frames.is_empty() {
                continue;
            }
            sequences.insert(
                CursorId::Ares { index, mini },
                SoftwareCursorSequence {
                    frames: sequence_frames,
                    interval_ms,
                    hotspot: resolve_hotspot(hotspot(cursor), &shp),
                },
            );
        }
    }
    // The arrow must load, as the stock build requires.
    sequences.get(&CursorId::Ares {
        index: 0,
        mini: false,
    })?;
    let planning_badges = [76, 77].map(|row| {
        let frame = usize::try_from(cursors.get(row)?.frame).ok()?;
        Some(PlanningBadge {
            width: i32::from(shp.width),
            height: i32::from(shp.height),
            rgba: frame_to_canvas_rgba(&shp, frame, &palette).ok()?,
        })
    });
    Some(SoftwareCursor {
        sequences,
        planning_badges,
        ares: true,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{CURSOR_DEFS, hotspot_offset};
    use super::*;
    use crate::rules::ares_mouse_cursor::index;

    /// Through the default list every stock id draws the range, interval and
    /// hotspot `CURSOR_DEFS` gives it, but for `0x52BA78`'s six rows, which
    /// Ares animates, and `Default`, whose stock atlas also packs the small
    /// arrow.
    #[test]
    fn stock_ids_draw_their_rows_through_the_list() {
        let cursors = MouseCursors::default();
        let animated_by_ares = [
            index::PARA_DROP,
            index::CHRONOSPHERE,
            index::IRON_CURTAIN,
            index::DETONATE,
            index::CURSOR_36,
            index::IVAN_BOMB,
        ];
        for &(id, start, count, interval_ms, stock_hotspot) in CURSOR_DEFS {
            let (row, mini) = vanilla_row(id).expect("stock id");
            let cursor = cursors.get(i32::from(row)).expect("vanilla row");
            let (ares_start, ares_count, ares_interval) = layout(cursor, mini).unwrap();
            let count = if id == CursorId::Default { 1 } else { count };
            assert_eq!((ares_start, ares_count), (start, count), "{id:?}");
            let still = |count: usize, interval: u64| count == 1 || interval == 0;
            if still(count, interval_ms) != still(ares_count, ares_interval) {
                assert!(
                    animated_by_ares.contains(&i32::from(row)) && interval_ms == 0,
                    "{id:?} animates differently"
                );
                assert_eq!(ares_interval, 4 * CURSOR_TIMER_UNIT_MS, "{id:?}");
            } else if !still(count, interval_ms) {
                assert_eq!(ares_interval, interval_ms, "{id:?}");
            }
            assert_eq!(
                hotspot_offset(hotspot(cursor), 88, 68),
                hotspot_offset(stock_hotspot, 88, 68),
                "{id:?}"
            );
        }
    }

    /// `0x5BDC8C` draws the main set of an entry without radar frames even
    /// for the radar; a `-1` radar count is one still frame.
    #[test]
    fn radar_sets_fall_back_to_the_main_frames() {
        let cursors = MouseCursors::default();
        let deploy = cursors.get(index::DEPLOY).unwrap();
        assert_eq!(layout(deploy, true), layout(deploy, false));
        let paradrop = cursors.get(index::PARA_DROP).unwrap();
        assert_eq!(layout(paradrop, true), Some((516, 1, 64)));
        let mut sequences = HashMap::new();
        let sequence = || SoftwareCursorSequence {
            frames: Vec::new(),
            interval_ms: 0,
            hotspot: [0.0, 0.0],
        };
        sequences.insert(
            CursorId::Ares {
                index: index::DEPLOY as u16,
                mini: false,
            },
            sequence(),
        );
        sequences.insert(
            CursorId::Ares {
                index: 0,
                mini: false,
            },
            sequence(),
        );
        let deploy_mini = lookup(
            &sequences,
            CursorId::Ares {
                index: index::DEPLOY as u16,
                mini: true,
            },
        );
        assert!(std::ptr::eq(
            deploy_mini.unwrap(),
            lookup(&sequences, CursorId::Deploy).unwrap()
        ));
    }
}
