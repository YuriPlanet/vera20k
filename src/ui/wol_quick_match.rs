//! The Quick Match page `0x117` (proc `0x007A2110`), which the Quick Match
//! runner `0x0079FC40` shows while it talks to the matchbot: a family page
//! (`0x0060C540` accepts it) with the heading, the "Match Status" caption,
//! the status transcript `0x53F`, the status line and Cancel `0x5C0`.
//!
//! The model owns the transcript and Cancel; the runner, its stage machine
//! and what Cancel does belong to the app's WOL owner.

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use crate::ui::lan_shell::LanChatLine;
use crate::ui::shell::descriptor::{AnchorRule, DialogId, TALL_HEADING_ANCHOR};
use crate::ui::shell::geom::{RectPx, ShellScale, child_window};
use crate::ui::shell::layout::Placer;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page;

pub(crate) const DIALOG: DialogId = DialogId(0x0117);
pub(crate) const CANCEL: u16 = 0x05C0;
pub(crate) const TRANSCRIPT: u16 = 0x053F;

/// RT_DIALOG `0x117`: no stacked buttons, Cancel on the bottom row. Help
/// keys from the executed `0x006040B0`; Cancel's `WOL:WDTMatchBack`
/// variant (`0x0060583B`) is for mode 3, which no welcome writes.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:Matching",
    stacked: &[],
    back: MenuPageButtonSpec::new(CANCEL, 257, "GUI:Cancel", "STT:WOLMatchCancel"),
};

/// The caption over the transcript (a left static, `0x50020000`).
pub(crate) const MATCH_STATUS_KEY: &str = "GUI:MatchStatus";

/// `0x005E2AC0`'s limit: a last row shorter than this takes the next dot.
pub(crate) const DOT_ROW_LIMIT: usize = 0x24;

/// Status help of `0x117`'s children (executed `0x006040B0`).
#[cfg(all(test, tests_ui))]
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    match control {
        CANCEL => Some(PAGE.back.tooltip_key),
        TRANSCRIPT => Some(TRANSCRIPT_HELP),
        _ => None,
    }
}

const TRANSCRIPT_HELP: &str = "STT:WOLMatchOutput";

/// `0x117`'s widgets: Cancel and the transcript (`LBS_NOSEL`: a row press
/// selects nothing it shows, and plays nothing).
pub(crate) fn spec() -> NodeSpec {
    NodeSpec::new(NodeKind::Dialog)
        .children(page::menu_buttons(&PAGE))
        .child(
            NodeSpec::new(NodeKind::List)
                .id(TRANSCRIPT.into())
                .tooltip(TRANSCRIPT_HELP)
                .with(Prop::RowCount, Value::Int(0))
                .with(Prop::ListWindow, Value::Bool(true)),
        )
}

/// Dialog `0x117` while it exists: the transcript's rows (its scroll is the
/// list widget's).
#[derive(Debug, Clone, Default)]
pub(crate) struct WolQuickMatchPage {
    pub(crate) lines: Vec<LanChatLine>,
}

impl WolQuickMatchPage {
    /// `0x005E2AC0` on the transcript, for each 2000 ms tick: a last row
    /// shorter than [`DOT_ROW_LIMIT`] is deleted and added again with a
    /// `"."` appended; otherwise a new `"."` row is added. Both go through
    /// the list adder with colour -1, as `add` does here.
    pub(crate) fn dot(&mut self, add: impl FnOnce(&mut Self, Vec<u16>)) {
        let text = match self.lines.last() {
            Some(last) if last.text.len() < DOT_ROW_LIMIT => {
                let mut text = self.lines.pop().expect("checked").text;
                text.push(u16::from(b'.'));
                text
            }
            _ => vec![u16::from(b'.')],
        };
        add(self, text);
    }
}

/// `0x117` at the current screen: the family right panel and status line,
/// and the left children at their template places.
#[derive(Debug, Clone)]
pub(crate) struct WolQuickMatchLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) status_help: RectPx,
    pub(crate) caption: RectPx,
    pub(crate) transcript: RectPx,
}

impl WolQuickMatchLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let place = Placer::on(scale, width, height);
        let mut page = menu_page::compute_layout_for(&PAGE, scale, width, height);
        let status_help = place.at(AnchorRule::StatusLine, 2, 355, 316, 12);
        // Heading 0x694 is 106x11 DLU, centred by 0x0060B1D0 where the
        // family's sits and one row taller.
        page.title = place.at(TALL_HEADING_ANCHOR, 425, 1, 106, 11);
        let px = |[x, y, w, h]: [i32; 4]| scale.scale_rect(child_window(x, y, w, h));
        Self {
            page,
            status_help,
            caption: px([7, 9, 146, 13]),
            transcript: px([22, 26, 377, 297]),
        }
    }

    /// Where control `id` sits.
    pub(crate) fn rect_of(&self, id: u16) -> Option<RectPx> {
        match id {
            TRANSCRIPT => Some(self.transcript),
            id => self.page.button_rect(id),
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn rect(value: &serde_json::Value) -> RectPx {
        let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
        RectPx::new(a[0], a[1], a[2], a[3])
    }

    #[test]
    fn layout_help_and_slide_follow_the_executed_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x117"]["sizes"].as_object().unwrap() {
            let (width, height) = size.split_once('x').unwrap();
            let layout = WolQuickMatchLayout::new(width.parse().unwrap(), height.parse().unwrap());
            for (control, child) in case["controls"].as_object().unwrap() {
                let actual = match control.as_str() {
                    "0x71C" => layout.page.warning_monitor,
                    "0x694" => layout.page.title,
                    "0x695" => layout.status_help,
                    "0x53F" => layout.transcript,
                    "0xFFFF" => {
                        assert_eq!(child["caption"].as_str(), Some(MATCH_STATUS_KEY));
                        layout.caption
                    }
                    "0x5C0" => layout.page.button_rect(CANCEL).unwrap(),
                    other => panic!("{size} {other}"),
                };
                assert_eq!(actual, rect(&child["rect"]), "{size} {control}");
                checked += 1;
            }
        }
        assert_eq!(checked, 3 * 6);
        for (control, help) in corpus["help"]["0x117"].as_object().unwrap() {
            let id = u16::from_str_radix(&control[2..], 16).unwrap();
            assert_eq!(tooltip_key(id), help.as_str(), "{control}");
        }
        let record = &corpus["slides"]["0x117"];
        let slide = crate::ui::shell::slide::slide_spec_for(DIALOG).unwrap();
        assert_eq!(
            slide.top_buttons as usize,
            record["top"].as_array().unwrap().len()
        );
        assert_eq!(
            slide.bottom_button,
            !record["bottom"].as_array().unwrap().is_empty()
        );
        assert_eq!(
            (slide.top_panel, slide.map_button),
            (record["d5"] != 0, record["d6"] != 0)
        );
    }

    #[test]
    fn dots_grow_the_last_short_row_then_start_a_new_one() {
        let mut page = WolQuickMatchPage::default();
        let add = |page: &mut WolQuickMatchPage, text: Vec<u16>| {
            page.lines.push(LanChatLine {
                text,
                color: u32::MAX,
            })
        };
        page.dot(add);
        assert_eq!(page.lines.len(), 1);
        page.lines[0].text = vec![u16::from(b'x'); DOT_ROW_LIMIT - 1];
        page.lines[0].color = 0x8080FF;
        page.dot(add);
        assert_eq!(page.lines.len(), 1);
        assert_eq!(page.lines[0].text.len(), DOT_ROW_LIMIT);
        assert_eq!(page.lines[0].color, u32::MAX);
        page.dot(add);
        assert_eq!(page.lines.len(), 2);
        assert_eq!(page.lines[1].text, [u16::from(b'.')]);
    }
}
