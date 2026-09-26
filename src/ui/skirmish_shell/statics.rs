//! Kind-1 statics of dialog `0x102`: heading `0x694`, game type `0x6EC`, map
//! name `0x5A8` and status line `0x695`.
//!
//! `0x102` owns them for the dialog's lifetime. Choose Map slides the dialog
//! out (`0x00608070`) and hides it (`0x006AD93C`) without destroying it, so
//! they keep their text, started flag (`+0xA8`) and count while `0x6B` shows;
//! only a new `0x102` creates them hidden and blank (`0x0060A5B0` at
//! WM_INITDIALOG). The SHOW completion (`0x4EC`, `0x006230B8` → `0x0060AA60`)
//! sends `0x4EE`, which starts a static only while it has not started
//! (`0x00615FDB`): a re-shown `0x102` repaints them at their count, so a
//! finished reveal's last unit turns plain yellow where the first show left
//! its trail step (retail `cm-ret-steady.png` against `sk-steady.png`). A
//! changed text restarts a started reveal from count 1 (`0x4B2`,
//! `0x00611BC1`), as when Use Map picks another map (`0x006ADA99`).

use std::time::{Duration, Instant};

use crate::ui::shell::static_reveal::{
    DialogStatics, DialogStaticsPaint, Kind1Params, PresentedKind1Static, StaticPaint,
};

/// Game type `0x6EC` and map name `0x5A8` (the executed getters).
pub(crate) const MAP_STATIC_KIND1: Kind1Params = Kind1Params {
    interval: Duration::from_millis(30),
    step: 1,
    range: 8,
};

#[derive(Debug, Clone)]
pub(crate) struct SkirmishStatics {
    /// Heading and status line; the status line holds the last hover help.
    dialog: DialogStatics,
    game_type: PresentedKind1Static,
    map_label: PresentedKind1Static,
}

/// Every static in one recomposition; `None` where nothing paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SkirmishStaticsPaint<'a> {
    pub heading: Option<StaticPaint<'a>>,
    pub game_type: Option<StaticPaint<'a>>,
    pub map_label: Option<StaticPaint<'a>>,
    pub status_line: Option<StaticPaint<'a>>,
}

impl Default for SkirmishStatics {
    fn default() -> Self {
        Self {
            dialog: DialogStatics::default(),
            game_type: PresentedKind1Static::new(MAP_STATIC_KIND1),
            map_label: PresentedKind1Static::new(MAP_STATIC_KIND1),
        }
    }
}

impl SkirmishStatics {
    /// A new `0x102` instance: hidden, no text (the status line's template
    /// text is `GUI:Blank`).
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// The SHOW completion with the texts the dialog holds (`0x4B2`, then
    /// `0x4EE` to each static).
    pub(crate) fn show(&mut self, heading: &str, game_type: &str, map_label: &str, now: Instant) {
        self.game_type.set_text(game_type, now);
        self.map_label.set_text(map_label, now);
        self.dialog.show(heading, now);
        self.game_type.show(now);
        self.map_label.show(now);
    }

    /// A hover message to the status line ([`DialogStatics::hover`]).
    pub(crate) fn hover(&mut self, help: &str, now: Instant) -> bool {
        self.dialog.hover(help, now)
    }

    /// Every static for this recomposition; the frame loop commits them
    /// after present. While a slide runs it blits over the right panel's
    /// three, and the status line gets no timer ([`DialogStatics::paint`]).
    pub(crate) fn paint(&mut self, now: Instant, sliding: bool) -> SkirmishStaticsPaint<'_> {
        let DialogStaticsPaint {
            heading,
            status_line,
        } = self.dialog.paint(now, sliding);
        let (game_type, map_label) = if sliding {
            (None, None)
        } else {
            (
                self.game_type.paint_text(now, true),
                self.map_label.paint_text(now, true),
            )
        };
        SkirmishStaticsPaint {
            heading,
            game_type,
            map_label,
            status_line,
        }
    }

    pub(crate) fn commit_presented(&mut self) {
        self.dialog.commit_presented();
        self.game_type.commit_presented();
        self.map_label.commit_presented();
    }

    /// Every reveal ran to completion and its last paint is on screen.
    pub(crate) fn is_terminal(&self) -> bool {
        self.dialog.is_terminal() && self.game_type.is_terminal() && self.map_label.is_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paint and present once: heading, game type, map name, status line.
    fn present(statics: &mut SkirmishStatics, now: Instant) -> [Option<u32>; 4] {
        let shown = statics.paint(now, false);
        let counts = [
            shown.heading,
            shown.game_type,
            shown.map_label,
            shown.status_line,
        ]
        .map(|shown| shown.map(|shown| shown.window.count));
        statics.commit_presented();
        counts
    }

    /// Present on each 15 ms tick until every reveal is terminal.
    fn settle(statics: &mut SkirmishStatics, from: Instant) -> Instant {
        let mut now = from;
        for _ in 0..200 {
            present(statics, now);
            if statics.is_terminal() {
                return now;
            }
            now += Duration::from_millis(15);
        }
        panic!("the statics never settled");
    }

    #[test]
    fn map_statics_take_the_native_getters_parameters() {
        let cases = crate::ui::shell::static_reveal::tests::native_static_timer_cases();
        for control in [0x6EC, 0x5A8] {
            let case = cases
                .iter()
                .find(|case| case["dialog_id"] == 0x102 && case["control_id"] == control)
                .unwrap_or_else(|| panic!("0x102/{control:#x}"));
            assert_eq!(case["kind1"], 1, "{control:#x} is kind 1");
            let params = Kind1Params {
                interval: Duration::from_millis(case["interval_ms"].as_u64().unwrap()),
                step: case["step"].as_u64().unwrap() as u32,
                range: case["range"].as_u64().unwrap() as u32,
            };
            assert_eq!(params, MAP_STATIC_KIND1, "{control:#x}");
        }
    }

    #[test]
    fn statics_stay_hidden_until_the_show_completion() {
        let t0 = Instant::now();
        let mut statics = SkirmishStatics::default();
        statics.hover("Start Game help", t0);
        assert_eq!(present(&mut statics, t0), [None; 4]);
        statics.show("Skirmish Game", "Battle", "DC Uprising (2-4)", t0);
        let shown = statics.paint(t0, false);
        assert_eq!(shown.heading.map(|shown| shown.text), Some("Skirmish Game"));
        assert_eq!(
            shown
                .status_line
                .map(|shown| (shown.text, shown.window.count)),
            Some(("Start Game help", 1))
        );
        assert_eq!(present(&mut statics, t0), [Some(1); 4]);
    }

    #[test]
    fn a_reshown_dialog_repaints_its_finished_statics_at_the_final_count() {
        let t0 = Instant::now();
        let mut statics = SkirmishStatics::default();
        statics.show("Skirmish Game", "Battle", "Map", t0);
        let settled = settle(&mut statics, t0);
        // The last timer paint leaves "Skirmish Game" at count 21 of 22.
        assert_eq!(present(&mut statics, settled)[0], Some(21));
        // Back from Choose Map with the same texts: no restart, one repaint
        // at the count the last timer paint left, at or past the target,
        // where every unit is plain (the status line steps by 3 past 17).
        statics.show("Skirmish Game", "Battle", "Map", settled);
        assert_eq!(
            present(&mut statics, settled),
            [Some(22), Some(15), Some(12), Some(19)]
        );
    }

    #[test]
    fn another_map_restarts_only_the_map_name() {
        let t0 = Instant::now();
        let mut statics = SkirmishStatics::default();
        statics.show("Skirmish Game", "Battle", "Map", t0);
        let settled = settle(&mut statics, t0);
        statics.show("Skirmish Game", "Battle", "Other Map", settled);
        assert_eq!(present(&mut statics, settled)[1..3], [Some(15), Some(1)]);
    }

    #[test]
    fn every_hover_repaints_the_status_line_and_new_help_restarts_it() {
        let t0 = Instant::now();
        let mut statics = SkirmishStatics::default();
        statics.show("Skirmish Game", "Battle", "Map", t0);
        present(&mut statics, t0);
        assert!(statics.hover("Help", t0));
        assert_eq!(present(&mut statics, t0)[3], Some(1));
        // The same help between timer ticks paints and advances the count.
        assert!(!statics.hover("Help", t0));
        assert_eq!(present(&mut statics, t0)[3], Some(4));
        assert!(statics.hover("Other help", t0));
        assert_eq!(present(&mut statics, t0)[3], Some(1));
    }

    #[test]
    fn a_new_instance_hides_every_static_and_forgets_the_help() {
        let t0 = Instant::now();
        let mut statics = SkirmishStatics::default();
        statics.show("Skirmish Game", "Battle", "Map", t0);
        statics.hover("Start Game help", t0);
        settle(&mut statics, t0);
        statics.reset();
        assert_eq!(present(&mut statics, t0), [None; 4]);
        statics.show("Skirmish Game", "Battle", "Map", t0);
        assert_eq!(
            statics.paint(t0, false).status_line.map(|shown| shown.text),
            Some("")
        );
    }

    #[test]
    fn a_reshown_dialog_keeps_its_status_line_paint_through_the_slide() {
        let t0 = Instant::now();
        let mut statics = SkirmishStatics::default();
        statics.show("Skirmish Game", "Battle", "Map", t0);
        statics.hover("Help", t0);
        let settled = settle(&mut statics, t0);
        // "Help": target 4 + 1 + 16 = 21; the last timer paint drew 19.
        assert_eq!(present(&mut statics, settled)[3], Some(19));
        // Shown again after Choose Map: the entry slide shows only the status
        // line, at its last paint ...
        let later = settled + Duration::from_millis(500);
        let shown = statics.paint(later, true);
        assert_eq!([shown.heading, shown.game_type, shown.map_label], [None; 3]);
        assert_eq!(
            shown
                .status_line
                .map(|shown| (shown.text, shown.window.count)),
            Some(("Help", 19))
        );
        statics.commit_presented();
        // ... until the SHOW completion repaints every static at its count.
        statics.show("Skirmish Game", "Battle", "Map", later);
        assert_eq!(
            present(&mut statics, later),
            [Some(22), Some(15), Some(12), Some(22)]
        );
    }
}
