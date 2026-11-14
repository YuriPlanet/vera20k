//! The decision half of the six remaining section-B Phobos HUD keys.
//!
//! Native identity: `Phobos.INI.cpp:99-101,117,189-190` and
//! `Ext/Side/Body.cpp:41`. The consumers are three screen-fixed drawing
//! sites — `Misc/MessageColumn.cpp:299,462-463,1112`,
//! `src/Phobos.cpp:392` (`GScreenClass_DrawText @ 0x4F4583`) and
//! `Ext/Sidebar/Hooks.cpp:36-42` (`SidebarClass_DrawIt_ProducingProgress
//! @ 0x6A6EB1`). Keeping the arithmetic here lets the production draw sites
//! stay one call away from their native constants.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything, but this module needs
//!   only `std` so its oracles stay independent of the renderer.

/// The six section-B `[Phobos]` / `[Sidebar]` client values one match keeps.
/// Defaults are the Phobos statics (`Phobos.INI.cpp:57-59,116`) and the
/// `ShowGameTime`/`MessageDisplayInCenter` switch defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HudBoardsUi {
    pub(crate) message_display_in_center: bool,
    pub(crate) message_board_opacity: i32,
    pub(crate) message_labels: i32,
    pub(crate) message_records: i32,
    pub(crate) show_game_time: bool,
    pub(crate) game_time_board_opacity: i32,
}

impl Default for HudBoardsUi {
    fn default() -> Self {
        Self {
            message_display_in_center: false,
            message_board_opacity: 40,
            message_labels: 6,
            message_records: 12,
            show_game_time: false,
            game_time_board_opacity: 40,
        }
    }
}

/// `DSurface::FillRectTrans`'s opacity argument is a 0..100 percent. The
/// engine clamps with an unsigned compare against 100
/// (`0x4BB94A..0x4BB958` is the tooltip instance of the same helper), so a
/// negative integer reads as full opacity and anything above 100 as full
/// opacity as well. VERA's sprite quads take a 0.0..1.0 alpha.
pub(crate) fn board_alpha(opacity: i32) -> f32 {
    let percent = if (opacity as u32) > 100 { 100 } else { opacity };
    percent as f32 / 100.0
}

/// `MessageColumnClass::Init` (`Misc/MessageColumn.cpp:455-465`): the authored
/// label/record counts are clamped to the geometry's `maxLines` before
/// `Initialize(posX, posY, maxCount, maxRecord, maxChars, width)`. VERA has no
/// toggle button column, so the caller supplies its own line budget.
///
/// Native order is `maxRecord = clamp(RecordsCount, 4, maxLines)` then
/// `maxCount = clamp(LabelsCount, 1, maxLines)`. A panel shorter than the
/// native minimum caps the minimum itself (`maxLines` wins), which keeps the
/// two bounds ordered without the undefined `std::clamp(lo > hi)` case.
pub(crate) fn column_capacities(labels: i32, records: i32, max_lines: usize) -> (usize, usize) {
    let hi = max_lines.max(1);
    let cap = |value: i32, low: usize| -> usize {
        let low = low.min(hi);
        if value <= 0 {
            low
        } else {
            (value as usize).min(hi).max(low)
        }
    };
    (cap(labels, 1), cap(records, 4))
}

/// `src/Phobos.cpp:355-377`: `TXT_GAMETIME` (fallback `L"Time:"`) followed by
/// `H:MM:SS` when there is an hour, else `MM:SS`. VERA's elapsed clock is in
/// whole seconds; the native `ElapsedTimer`/`SystemTimer` blend is a residual.
pub(crate) fn game_time_text(seconds: i32) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds / 60) % 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("Time: {hours}:{minutes:02}:{secs:02}")
    } else {
        format!("Time: {minutes:02}:{secs:02}")
    }
}

/// `Ext/Sidebar/Hooks.cpp:39-42`: the four progress strips start at
/// `XBase = (Sidebar_GDIPositions ? 26 : 20) + offset.X` and
/// `YBase = 197 + offset.Y`. VERA's indicator lane anchors against the
/// sidebar surface centre, so this returns the native absolute base plus the
/// authored offset for the lane to place.
pub(crate) fn producing_progress_anchor(offset: [i32; 2], gdi_positions: bool) -> (i32, i32) {
    let x_base: i32 = if gdi_positions { 26 } else { 20 };
    (
        x_base.wrapping_add(offset[0]),
        197i32.wrapping_add(offset[1]),
    )
}

/// `0x6A6EB1` picks one of the four `TabProducingProgress` SHP frames from the
/// factory's 0..54 step progress. VERA has no `tab0xpp.shp` asset, so the
/// indicator lane prints the same completion as a percentage instead.
pub(crate) fn producing_progress_label(progress: u16, steps: u16) -> String {
    let percent = if steps == 0 {
        0
    } else {
        (u32::from(progress.min(steps)) * 100) / u32::from(steps)
    };
    format!("{percent}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_match_defaults_are_the_phobos_statics() {
        let b = HudBoardsUi::default();
        assert!(!b.message_display_in_center);
        assert!(!b.show_game_time);
        assert_eq!(b.message_board_opacity, 40);
        assert_eq!(b.game_time_board_opacity, 40);
        assert_eq!((b.message_labels, b.message_records), (6, 12));
    }

    #[test]
    fn fill_rect_trans_opacity_is_a_clamped_zero_to_one_alpha() {
        assert_eq!(board_alpha(0), 0.0);
        assert_eq!(board_alpha(40), 0.4);
        assert_eq!(board_alpha(100), 1.0);
        assert_eq!(board_alpha(200), 1.0, "above 100 clamps to full");
        assert_eq!(board_alpha(-5), 1.0, "negative reads as full opacity");
    }

    #[test]
    fn the_message_column_clamps_both_counts_to_its_line_budget() {
        assert_eq!(column_capacities(6, 12, 14), (6, 12));
        assert_eq!(column_capacities(0, 0, 14), (1, 4), "native minima");
        assert_eq!(column_capacities(99, 99, 14), (14, 14), "maxLines wins");
        assert_eq!(column_capacities(6, 12, 3), (3, 3), "short panel");
        assert_eq!(column_capacities(6, 12, 0), (1, 1));
    }

    #[test]
    fn the_game_time_label_uses_the_native_hour_and_padding() {
        assert_eq!(game_time_text(0), "Time: 00:00");
        assert_eq!(game_time_text(65), "Time: 01:05");
        assert_eq!(game_time_text(3661), "Time: 1:01:01");
        assert_eq!(game_time_text(-10), "Time: 00:00");
    }

    #[test]
    fn producing_progress_uses_the_gdi_base_and_the_side_offset() {
        assert_eq!(producing_progress_anchor([0, 0], false), (20, 197));
        assert_eq!(producing_progress_anchor([0, 0], true), (26, 197));
        assert_eq!(producing_progress_anchor([12, -7], true), (38, 190));
    }

    #[test]
    fn producing_progress_percent_clamps_at_the_completed_step() {
        assert_eq!(producing_progress_label(0, 54), "0%");
        assert_eq!(producing_progress_label(27, 54), "50%");
        assert_eq!(producing_progress_label(54, 54), "100%");
        assert_eq!(producing_progress_label(90, 54), "100%");
        assert_eq!(producing_progress_label(7, 0), "0%", "no zero divide");
    }
}
