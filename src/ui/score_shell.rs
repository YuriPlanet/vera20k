//! End-of-match score screen: dialog `0x108`.
//!
//! A skirmish win or loss (`GameExit__Victory` `0x00685670`,
//! `GameExit__Defeat` `0x00685DC0`) runs `ScoreDialog__Run` `0x005C9720`: it
//! loads the local side's art (`0x0072D730`), presents the right panel and the
//! art, and runs the modal family dialog `0x108` (proc `0x005C9B10`). Continue
//! (`0x6D1`) is its only button; its result is 1 (`0x005CA06C`) and no key
//! closes the dialog.
//!
//! `0x108` is a family page. It slides (0, 1, 0, 0) in and out; its heading
//! `0x694`, status line `0x695` and all 47 table texts are kind-1 statics
//! whose reveals start together at the entry slide's end (`0x4EC`). The proc's
//! init (`0x497`, `0x005C9B70..0x005CA04A`) sets the texts and each row's
//! colour and gives the band statics their PCX bars; its `WM_PAINT` darkens
//! the table's rectangle to 37.5 % (`0x005CA07F..0x005CA0FB`).
//!
//! Render-agnostic: geometry, the resolved model and the statics' reveal
//! state. The app resolves strings and colours when it opens the page.

use std::time::{Duration, Instant};

use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, child_window};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::static_reveal::{Kind1Params, Kind1RevealWindow, PresentedKind1Static};

pub const SCORE_DIALOG: DialogId = DialogId(0x0108);
pub const CONTINUE_BUTTON: u16 = 0x06D1;

/// RT_DIALOG `0x108`'s right panel: Continue on the bottom tile.
pub const SCORE_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: SCORE_DIALOG,
    // The template caption; the proc replaces it (`0x005C9BEC..0x005C9C34`).
    title_key: "GUI:MultiplayerScore",
    stacked: &[],
    back: MenuPageButtonSpec {
        id: CONTINUE_BUTTON,
        dlu_top: 326,
        csf_key: "GUI:Continue",
        tooltip_key: "STT:MPScoreButtonContinue",
        result: Some(1),
    },
};

/// Player rows the template declares.
pub const SCORE_ROW_SLOTS: usize = 8;

/// Game `0x6D2` and Time `0x3EA`.
const GAME_LABEL: u16 = 0x06D2;
const TIME_LABEL: u16 = 0x03EA;
/// The header statics in column order: Player, Kills, Losses, Built, Score.
const HEADER_IDS: [u16; 5] = [0x069D, 0x069F, 0x069E, 0x06A0, 0x078B];
/// Each row's name, Kills, Losses, Built and Score statics (the proc's table
/// at `0x0082FD5C`).
const ROW_CELL_IDS: [[u16; 5]; SCORE_ROW_SLOTS] = [
    [0x0411, 0x0419, 0x06A3, 0x06A5, 0x06A6],
    [0x0412, 0x041A, 0x06A4, 0x06A7, 0x06A8],
    [0x0413, 0x041B, 0x06A9, 0x06AA, 0x06AB],
    [0x0414, 0x041C, 0x06AC, 0x06AD, 0x06AE],
    [0x0415, 0x041D, 0x06AF, 0x06B0, 0x06B1],
    [0x0416, 0x041E, 0x06B2, 0x06B3, 0x06B4],
    [0x0417, 0x041F, 0x06B5, 0x06B6, 0x06B7],
    [0x0418, 0x0420, 0x06B8, 0x06B9, 0x06BA],
];

/// Native ceiling on the displayed time: `99:59:59`. gamemd clamps the raw
/// second count to this before splitting it (`0x005C9CB3..0x005C9CB8`).
pub const MAX_DISPLAY_SECONDS: u32 = 359_999;

/// Kind-1 reveal of the Game and Time labels, the headers and the names
/// (`0x00600CA0`, `0x006015E0`, `0x00601D20`; executed by
/// `tools/storage_oracle/shell_static_timers.py`).
const LABEL_KIND1: Kind1Params = Kind1Params {
    interval: Duration::from_millis(30),
    step: 1,
    range: 32,
};

/// Kind-1 reveal of the Kills, Losses, Built and Score cells.
const NUMBER_KIND1: Kind1Params = Kind1Params {
    interval: Duration::from_millis(60),
    step: 1,
    range: 64,
};

/// One player's line on the score screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreRow {
    pub name: String,
    /// `HSV_To_RGB` of the house colour scheme's base HSV, sent with `0x498`
    /// to every cell of the row (`0x005C9E0C..0x005C9E58`).
    pub rgb: [u8; 3],
    pub kills: u32,
    pub losses: u32,
    pub built: u32,
    pub score: i32,
}

/// Everything the score screen displays, resolved once when the match ends.
///
/// Built before the simulation is torn down, because every value it carries
/// comes off the houses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScoreScreenModel {
    /// CSF key of the heading: `GUI:SkirmishScore` in a skirmish
    /// (`0x005C9BEC`).
    pub title_key: &'static str,
    /// Sequence number of the finished game within this session (`Game: n`).
    pub game_number: u32,
    /// Match length in whole seconds.
    pub elapsed_seconds: u32,
    /// The local side, `ScenarioClass +0x34B8`: in a skirmish the side of
    /// session player 0's country (`0x0068779A..0x0068782D`). It picks the
    /// art (`0x0072D730`) and the bars (`0x005CA110`).
    pub side: u8,
    /// Rows in display order, at most [`SCORE_ROW_SLOTS`].
    pub rows: Vec<ScoreRow>,
}

impl ScoreScreenModel {
    /// Split the elapsed time into the `h, m, s` triple the time format string
    /// consumes, applying the native clamp first.
    pub fn elapsed_hms(&self) -> (u32, u32, u32) {
        let total = self.elapsed_seconds.min(MAX_DISPLAY_SECONDS);
        (total / 3600, (total % 3600) / 60, total % 60)
    }
}

/// Format the elapsed time through `TXT_TIME_FORMAT_HOURS`, whose entry
/// carries the `Time:` prefix and three `%02d` fields.
pub fn format_elapsed(model: &ScoreScreenModel, format: &str) -> String {
    let (h, m, s) = model.elapsed_hms();
    substitute_numbers(format, &[h, m, s], 2)
}

/// Format the `Game: n` label through `TXT_GAME`.
pub fn format_game_number(model: &ScoreScreenModel, format: &str) -> String {
    substitute_numbers(format, &[model.game_number], 0)
}

/// Substitute successive integers into a printf-style CSF format string.
///
/// Only the two directives these strings use are honoured (`%d` and `%02d`);
/// any other `%` run is copied through verbatim so a translated string that
/// does something unexpected degrades to visible text instead of a panic.
/// `default_pad` is the zero-pad width applied to a bare `%d` in a format
/// whose fields are all padded.
fn substitute_numbers(format: &str, values: &[u32], default_pad: usize) -> String {
    let mut out = String::with_capacity(format.len() + values.len() * 2);
    let mut next = values.iter();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut pad = String::new();
        while chars.peek().is_some_and(|p| p.is_ascii_digit()) {
            pad.push(chars.next().unwrap_or_default());
        }
        match chars.peek() {
            Some('d') => {
                chars.next();
                let width = pad.parse::<usize>().unwrap_or(default_pad);
                match next.next() {
                    Some(value) => out.push_str(&format!("{value:0width$}")),
                    None => out.push_str(&format!("%{pad}d")),
                }
            }
            Some('%') => {
                chars.next();
                out.push('%');
            }
            _ => {
                out.push('%');
                out.push_str(&pad);
            }
        }
    }
    out
}

/// A row's five cells: name, Kills, Losses, Built and Score (the proc's table
/// at `0x0082FD5C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreRowRects {
    pub name: RectPx,
    pub kills: RectPx,
    pub losses: RectPx,
    pub built: RectPx,
    pub score: RectPx,
}

impl ScoreRowRects {
    /// Template columns at DLU x 66, 147, 202, 257 and 308.
    fn at(dlu_y: i32) -> Self {
        Self {
            name: child_window(66, dlu_y, 75, 10),
            kills: child_window(147, dlu_y, 45, 10),
            losses: child_window(202, dlu_y, 45, 10),
            built: child_window(257, dlu_y, 45, 10),
            score: child_window(308, dlu_y, 45, 10),
        }
    }

    fn cells(self) -> [RectPx; 5] {
        [self.name, self.kills, self.losses, self.built, self.score]
    }
}

/// The table's windows. They keep their template places at every screen
/// size (`0x0060B950`, executed for 640x480, 800x600 and 1024x768).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreTableLayout {
    pub game_label: RectPx,
    pub time_label: RectPx,
    pub header: ScoreRowRects,
    pub rows: [ScoreRowRects; SCORE_ROW_SLOTS],
    /// The band statics in the proc's band order (`0x0082FDFC`): the Game
    /// and Time row (`0x790`), the header row (`0x791`), then the player rows
    /// (`0x78F`, `0x792..0x798`). The executed relayout stacks them on a
    /// 36-pixel pitch from (96, 117), 441x37; the retail still at 800x600
    /// shows the first four bars exactly there.
    pub bands: [RectPx; 10],
}

impl ScoreTableLayout {
    pub fn template() -> Self {
        Self {
            game_label: child_window(66, 76, 115, 10),
            time_label: child_window(248, 76, 105, 10),
            header: ScoreRowRects::at(98),
            rows: std::array::from_fn(|row| ScoreRowRects::at(120 + 22 * row as i32)),
            bands: std::array::from_fn(|band| RectPx::new(96, 117 + 36 * band as i32, 441, 37)),
        }
    }

    /// The rectangle the proc darkens: from Game's window's top-left to the
    /// bottom-right of the last player row's Score cell (`0x0082FD58` indexed
    /// by the row count), grown by 4 pixels sideways and 8 vertically
    /// (`0x0072AA10`). With no rows the proc looks up a non-control id and
    /// darkens nothing.
    pub fn shaded_rect(&self, rows: usize) -> Option<RectPx> {
        let last = self.rows.get(rows.checked_sub(1)?)?.score;
        let left = self.game_label.x - 4;
        let top = self.game_label.y - 8;
        let right = last.x + last.w + 4;
        let bottom = last.y + last.h + 8;
        Some(RectPx::new(left, top, right - left, bottom - top))
    }
}

/// Dialog `0x108` at one screen size: the right panel, heading, monitor,
/// status line and Continue follow the family rules; the table does not move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreLayout {
    pub page: MenuPageLayout,
    pub table: ScoreTableLayout,
}

pub fn compute_layout(screen_w: u32, screen_h: u32) -> ScoreLayout {
    let mut page = menu_page::compute_layout(&SCORE_PAGE, screen_w, screen_h);
    // 0x695 is 303x14 DLU here; the menu pages' is 303x12.
    page.status_help = status_line_rect(
        RectPx::new(2, 353, 303, 14),
        screen_w as i32,
        screen_h as i32,
    );
    ScoreLayout {
        page,
        table: ScoreTableLayout::template(),
    }
}

impl ScoreLayout {
    pub fn continue_button(&self) -> RectPx {
        self.page
            .button_rect(CONTINUE_BUTTON)
            .expect("the score page lays out Continue")
    }
}

/// Text alignment of a table static: `SS_LEFT` or `SS_RIGHT`. Kind-1 text is
/// top-aligned (`0x00615A81..0x00615AE8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreAlign {
    Left,
    Right,
}

/// One kind-1 table static: its window, alignment, colour and text reveal.
#[derive(Debug, Clone)]
pub struct ScoreTableStatic {
    /// The template control id.
    pub id: u16,
    pub window: RectPx,
    pub align: ScoreAlign,
    /// The row colour from `0x498`, or `None` for the shell's text colour.
    pub rgb: Option<[u8; 3]>,
    reveal: PresentedKind1Static,
}

impl ScoreTableStatic {
    fn new(
        id: u16,
        window: RectPx,
        align: ScoreAlign,
        rgb: Option<[u8; 3]>,
        params: Kind1Params,
        text: &str,
        now: Instant,
    ) -> Self {
        let mut reveal = PresentedKind1Static::new(params);
        reveal.set_text(text, now);
        Self {
            id,
            window,
            align,
            rgb,
            reveal,
        }
    }

    pub fn text(&self) -> &str {
        self.reveal.text()
    }

    /// Reveal window for this recomposition, `None` before the slide ends.
    pub(crate) fn paint(&mut self, now: Instant) -> Option<Kind1RevealWindow> {
        self.reveal.paint(now)
    }
}

/// Strings `0x497` resolves through the string table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreTexts {
    /// `TXT_GAME` with the game counter.
    pub game: String,
    /// `TXT_TIME_FORMAT_HOURS` with the elapsed time.
    pub time: String,
    /// The header captions: Player, Kills, Losses, Built, Score.
    pub headers: [String; 5],
}

/// The open `0x108` dialog.
#[derive(Debug, Clone)]
pub struct ScorePage {
    pub model: ScoreScreenModel,
    table: Vec<ScoreTableStatic>,
}

impl ScorePage {
    /// `0x497`: set every text and row colour before the page shows. Rows
    /// past the model keep `GUI:Blank` and draw nothing, so they get no
    /// static here.
    pub fn open(model: ScoreScreenModel, texts: &ScoreTexts, now: Instant) -> Self {
        let layout = ScoreTableLayout::template();
        let mut table = vec![
            ScoreTableStatic::new(
                GAME_LABEL,
                layout.game_label,
                ScoreAlign::Left,
                None,
                LABEL_KIND1,
                &texts.game,
                now,
            ),
            ScoreTableStatic::new(
                TIME_LABEL,
                layout.time_label,
                ScoreAlign::Right,
                None,
                LABEL_KIND1,
                &texts.time,
                now,
            ),
        ];
        for (column, ((id, window), text)) in HEADER_IDS
            .into_iter()
            .zip(layout.header.cells())
            .zip(&texts.headers)
            .enumerate()
        {
            let align = if column == 0 {
                ScoreAlign::Left
            } else {
                ScoreAlign::Right
            };
            table.push(ScoreTableStatic::new(
                id,
                window,
                align,
                None,
                LABEL_KIND1,
                text,
                now,
            ));
        }
        for ((row, rects), ids) in model.rows.iter().zip(layout.rows).zip(ROW_CELL_IDS) {
            let rgb = Some(row.rgb);
            let [name, kills, losses, built, score] = rects.cells();
            table.push(ScoreTableStatic::new(
                ids[0],
                name,
                ScoreAlign::Left,
                rgb,
                LABEL_KIND1,
                &row.name,
                now,
            ));
            for ((id, window), value) in ids[1..].iter().zip([kills, losses, built, score]).zip([
                row.kills.to_string(),
                row.losses.to_string(),
                row.built.to_string(),
                row.score.to_string(),
            ]) {
                table.push(ScoreTableStatic::new(
                    *id,
                    window,
                    ScoreAlign::Right,
                    rgb,
                    NUMBER_KIND1,
                    &value,
                    now,
                ));
            }
        }
        Self { model, table }
    }

    /// The entry slide's end (`0x4EC -> 0x0060AA60 -> 0x4EE`) starts every
    /// table reveal at once.
    pub fn start_reveals(&mut self, now: Instant) {
        for text in &mut self.table {
            text.reveal.start(now);
        }
    }

    pub fn table_mut(&mut self) -> &mut [ScoreTableStatic] {
        &mut self.table
    }

    /// The latest recomposition was presented.
    pub fn commit_presented(&mut self) {
        for text in &mut self.table {
            text.reveal.commit_presented();
        }
    }

    /// Every table reveal ran to completion and its final paint is shown.
    pub fn reveals_terminal(&self) -> bool {
        self.table.iter().all(|text| text.reveal.is_terminal())
    }
}

/// MSVC `qsort` as `0x005C9D7A` calls it (`0x007C8B48`): up to eight
/// elements it runs `shortsort`, which repeatedly swaps the first-found
/// greatest element under `compare` to the end. Ties therefore land in a
/// fixed permutation rather than in input order. Larger arrays take the
/// median partition, which a skirmish cannot reach (at most eight players;
/// passive houses and the observer get no row); they fall back to the same
/// selection here.
pub fn msvc_shortsort<T>(items: &mut [T], compare: impl Fn(&T, &T) -> std::cmp::Ordering) {
    let mut hi = items.len();
    while hi > 1 {
        let mut max = 0;
        for p in 1..hi {
            if compare(&items[p], &items[max]) == std::cmp::Ordering::Greater {
                max = p;
            }
        }
        items.swap(max, hi - 1);
        hi -= 1;
    }
}

/// The score dialog's comparator `0x005C9AE0`: a higher score sorts first.
pub fn sort_rows(rows: &mut [ScoreRow]) {
    msvc_shortsort(rows, |a, b| b.score.cmp(&a.score));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_window_matches_the_executed_relayout() {
        // tools/storage_oracle/shell_relayout.py: the right-panel children
        // move with the screen, the table keeps its template windows.
        use crate::ui::shell::layout::tests::executed_child_window as executed;
        const BANDS: [u16; 10] = [
            0x790, 0x791, 0x78F, 0x792, 0x793, 0x794, 0x795, 0x796, 0x797, 0x798,
        ];
        for (w, h) in [(640, 480), (800, 600), (1024, 768)] {
            let layout = compute_layout(w as u32, h as u32);
            let at = |control| executed(0x108, control, w, h);
            assert_eq!(layout.page.title, at(0x694), "{w}x{h}");
            assert_eq!(layout.page.warning_monitor, at(0x71C), "{w}x{h}");
            assert_eq!(layout.page.status_help, at(0x695), "{w}x{h}");
            assert_eq!(layout.continue_button(), at(CONTINUE_BUTTON), "{w}x{h}");
            let table = layout.table;
            assert_eq!(table.game_label, at(GAME_LABEL), "{w}x{h}");
            assert_eq!(table.time_label, at(TIME_LABEL), "{w}x{h}");
            assert_eq!(
                table.header.cells(),
                HEADER_IDS.map(|control| at(control)),
                "{w}x{h}"
            );
            for (row, ids) in table.rows.iter().zip(ROW_CELL_IDS) {
                assert_eq!(row.cells(), ids.map(|control| at(control)), "{w}x{h}");
            }
            assert_eq!(table.bands, BANDS.map(|control| at(control)), "{w}x{h}");
        }
    }

    #[test]
    fn the_shaded_rect_ends_eight_pixels_under_the_last_row() {
        let layout = ScoreTableLayout::template();
        // Two rows: x 95..534 and y 116..255, as the retail still measures.
        assert_eq!(layout.shaded_rect(2), Some(RectPx::new(95, 116, 440, 140)));
        assert_eq!(layout.shaded_rect(0), None);
        // Row 7's Score cell ends at y 462.
        assert_eq!(layout.shaded_rect(8), Some(RectPx::new(95, 116, 440, 354)));
    }

    #[test]
    fn every_table_static_reveals_with_its_native_parameters() {
        // tools/storage_oracle/shell_static_timers.py executes the getters for
        // each 0x108 control; the page's statics must carry what they return.
        let cases = crate::ui::shell::static_reveal::tests::native_static_timer_cases();
        let native = |control: u16| {
            let case = cases
                .iter()
                .find(|case| case["dialog_id"] == 0x108 && case["control_id"] == u64::from(control))
                .unwrap_or_else(|| panic!("0x108/{control:#x}"));
            assert_eq!(case["kind1"], 1, "{control:#x} is kind 1");
            Kind1Params {
                interval: Duration::from_millis(case["interval_ms"].as_u64().unwrap()),
                step: case["step"].as_u64().unwrap() as u32,
                range: case["range"].as_u64().unwrap() as u32,
            }
        };
        let full = ScoreScreenModel {
            rows: (0..SCORE_ROW_SLOTS as i32)
                .map(|slot| row("P", slot))
                .collect(),
            ..Default::default()
        };
        let page = ScorePage::open(
            full,
            &ScoreTexts {
                game: "Game: 1".into(),
                time: "Time".into(),
                headers: ["a", "b", "c", "d", "e"].map(String::from),
            },
            Instant::now(),
        );
        assert_eq!(page.table.len(), 47);
        let mut ids: Vec<u16> = page.table.iter().map(|text| text.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 47, "every template static once");
        for text in &page.table {
            assert_eq!(text.reveal.params(), native(text.id), "{:#x}", text.id);
        }
    }

    fn row(name: &str, score: i32) -> ScoreRow {
        ScoreRow {
            name: name.into(),
            rgb: [0, 0, 0],
            kills: 0,
            losses: 0,
            built: 0,
            score,
        }
    }

    #[test]
    fn rows_sort_in_the_native_qsort_order() {
        // The original qsort and comparator, executed by
        // tools/storage_oracle/score_sort.py: scores in house order -> rows.
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/storage_oracle/score_sort.json"))
                .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 12);
        for case in cases {
            let scores: Vec<i32> = case["scores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|score| score.as_i64().unwrap() as i32)
                .collect();
            let native: Vec<usize> = case["native_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| index.as_u64().unwrap() as usize)
                .collect();
            let mut rows: Vec<ScoreRow> = scores
                .iter()
                .enumerate()
                .map(|(house, score)| row(&house.to_string(), *score))
                .collect();
            sort_rows(&mut rows);
            let order: Vec<usize> = rows.iter().map(|row| row.name.parse().unwrap()).collect();
            assert_eq!(order, native, "{scores:?}");
        }
    }

    #[test]
    fn page_texts_follow_the_template_and_the_rows() {
        let model = ScoreScreenModel {
            title_key: "GUI:SkirmishScore",
            game_number: 1,
            elapsed_seconds: 79,
            side: 0,
            rows: vec![
                ScoreRow {
                    name: "Computer".into(),
                    rgb: [255, 25, 25],
                    kills: 0,
                    losses: 0,
                    built: 3,
                    score: 0,
                },
                ScoreRow {
                    name: "[New Player]".into(),
                    rgb: [34, 107, 212],
                    kills: 0,
                    losses: 15,
                    built: 0,
                    score: 0,
                },
            ],
        };
        let texts = ScoreTexts {
            game: format_game_number(&model, "Game: %d"),
            time: format_elapsed(&model, "Time: %02d:%02d:%02d"),
            headers: ["Player", "Kills", "Losses", "Built", "Score"].map(String::from),
        };
        let page = ScorePage::open(model, &texts, Instant::now());
        let shown: Vec<(&str, RectPx, ScoreAlign, Option<[u8; 3]>)> = page
            .table
            .iter()
            .map(|text| (text.text(), text.window, text.align, text.rgb))
            .collect();
        assert_eq!(shown.len(), 2 + 5 + 2 * 5);
        assert_eq!(
            shown[0],
            (
                "Game: 1",
                RectPx::new(99, 124, 174, 17),
                ScoreAlign::Left,
                None
            )
        );
        assert_eq!(
            shown[1],
            (
                "Time: 00:01:19",
                RectPx::new(372, 124, 159, 17),
                ScoreAlign::Right,
                None
            )
        );
        assert_eq!(shown[7].0, "Computer");
        assert_eq!(shown[7].3, Some([255, 25, 25]));
        // Row 1: name, Kills, then Losses.
        assert_eq!(
            shown[14],
            (
                "15",
                RectPx::new(303, 231, 69, 17),
                ScoreAlign::Right,
                Some([34, 107, 212])
            )
        );
    }

    #[test]
    fn reveals_wait_for_the_slide_end() {
        let t0 = Instant::now();
        let mut page = ScorePage::open(
            ScoreScreenModel {
                rows: vec![row("P", 5)],
                ..Default::default()
            },
            &ScoreTexts {
                game: "Game: 1".into(),
                time: "T".into(),
                headers: ["a", "b", "c", "d", "e"].map(String::from),
            },
            t0,
        );
        assert!(page.table.iter_mut().all(|text| text.paint(t0).is_none()));
        page.start_reveals(t0);
        assert!(page.table.iter_mut().all(|text| text.paint(t0).is_some()));
        assert!(!page.reveals_terminal());
    }

    #[test]
    fn elapsed_clamps_to_the_native_ceiling() {
        let mut model = ScoreScreenModel {
            elapsed_seconds: 3661,
            ..Default::default()
        };
        assert_eq!(model.elapsed_hms(), (1, 1, 1));
        model.elapsed_seconds = 10_000_000;
        assert_eq!(model.elapsed_hms(), (99, 59, 59));
        assert_eq!(
            format_elapsed(&model, "Time: %02d:%02d:%02d"),
            "Time: 99:59:59"
        );
    }

    #[test]
    fn format_substitution_survives_a_translation_with_missing_or_extra_fields() {
        let model = ScoreScreenModel {
            elapsed_seconds: 61,
            game_number: 2,
            ..Default::default()
        };
        assert_eq!(
            format_elapsed(&model, "%02d:%02d:%02d:%02d"),
            "00:01:01:%02d"
        );
        assert_eq!(format_game_number(&model, "%d%% %s"), "2% %s");
        assert_eq!(format_game_number(&model, "Game: %d"), "Game: 2");
    }
}
