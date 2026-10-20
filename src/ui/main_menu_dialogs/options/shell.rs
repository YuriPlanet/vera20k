//! Physical-pixel presentation and input for launcher resource 0xD5.
//!
//! Retail RT_DIALOG/0xD5/1033 provides the DLU rectangles; common setup
//! 0x00608CD0/0x00609730 anchors the rail, and 0x0060B950 adjusts its title.
//! This module projects the retained OptionsDialogState instead of owning a
//! second set of option values or bypassing its ordered preview events.

use super::{
    DEVELOPER_OPTIONS_LABEL, FULL_SCREEN_HELP, FULL_SCREEN_LABEL, LauncherCheckboxId,
    LauncherComboId, LauncherCue, LauncherOptionsEvent, LauncherParentResult, LauncherTrackbarId,
    ORIGINAL_SHAPE_HELP, ORIGINAL_SHAPE_LABEL, OptionsDialogState, PhysicalControlFrame,
    SHIFT_QUEUE_HELP, SHIFT_QUEUE_LABEL, SIDEBAR_HELP, SIDEBAR_LABEL, SidebarSize,
    WOL_TEST_SERVICE_HELP, WOL_TEST_SERVICE_LABEL, YR_ORIGINAL_STYLE_HELP, YR_ORIGINAL_STYLE_LABEL,
    ZOOM_HELP, ZOOM_LABEL, Zoom,
};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale, center_offset, child_window, dlu_rect};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageSpec};
use crate::ui::shell::trackbar::{authored_track, thumb_left_at};
use crate::ui::skirmish_shell::{COMBO_FACE_H, DropdownMetrics, combo_face_rect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ShellInteraction {
    hovered_button: Option<LauncherParentResult>,
    pressed_button: Option<LauncherParentResult>,
    popup_hovered: Option<usize>,
    /// Status help last written by a dialog hit test (`0x00622CCB`).
    status_help: Option<LauncherStatusHelp>,
}

/// What the status line says for a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherStatusHelp {
    /// A retail `STT:` string-table key.
    Csf(&'static str),
    /// VERA's own text, for the display controls retail never had.
    Text(&'static str),
}

pub(crate) const KEYBOARD_BUTTON: u16 = 0x05CE;
pub(crate) const NETWORK_BUTTON: u16 = 0x05CD;
pub(crate) const MAIN_MENU_BUTTON: u16 = 0x0686;

/// `0xD5` as a family page (Session `+0x30D8` clear at the main menu): the
/// right-panel column is Keyboard and Network (`0x006092CB`), then Main Menu
/// (`0x0060982C`); the heading and status line sit at the family places.
/// Button captions come from the dialog's own labels.
pub(crate) const OPTIONS_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0x00D5),
    title_key: "GUI:OptionsMenu",
    stacked: &[
        MenuPageButtonSpec {
            id: KEYBOARD_BUTTON,
            dlu_top: 122,
            csf_key: "GUI:Keyboard",
            tooltip_key: "STT:MainOptButtonKeyboard",
            result: None,
        },
        MenuPageButtonSpec {
            id: NETWORK_BUTTON,
            dlu_top: 149,
            csf_key: "GUI:Network",
            tooltip_key: "STT:MainOptButtonNetwork",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: MAIN_MENU_BUTTON,
        dlu_top: 346,
        csf_key: "GUI:MainMenu",
        tooltip_key: "STT:MainOptButtonBack",
        result: Some(0x05CB),
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLayout {
    pub(crate) title: RectPx,
    pub(crate) warning: RectPx,
    /// Status line `0x695`.
    pub(crate) status_help: RectPx,
    /// The Zoom combo, in the resolution combo's retail place.
    pub(crate) zoom: RectPx,
    /// The Sidebar combo, beside Difficulty as Zoom is beside Visual Details.
    pub(crate) sidebar: RectPx,
    pub(crate) trackbars: [(LauncherTrackbarId, RectPx); 6],
    pub(crate) checkboxes: [(LauncherCheckboxId, RectPx); 8],
    pub(crate) buttons: [(LauncherParentResult, RectPx); 3],
    /// The mapping the rects are in: [`ShellScale::RETAIL`] centres the
    /// authored dialog in the window, otherwise the authored dialog is
    /// scaled to fit it, centred. Control sub-rects and art fits key on this.
    pub(crate) scale: ShellScale,
    offset_x: i32,
    offset_y: i32,
    /// The window, which a match shares: the zoom list depends on it.
    screen: (u32, u32),
    /// A battle's frame in this window at the chosen zoom and shape: the
    /// sidebar list depends on it.
    battle_frame: (u32, u32),
}

impl LauncherOptionsLayout {
    /// Gamemd's centred `0xD5`: the authored dialog placed in the window at
    /// [`ShellScale::RETAIL`].
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    /// The same dialog scaled to fit the window.
    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        // The scale centres a scaled page itself, so it takes no centring
        // offset of its own and every authored rect goes through the one scale.
        let stretch = !scale.is_retail();
        let offset_x = if stretch {
            0
        } else {
            center_offset(width, 800)
        };
        let offset_y = if stretch {
            0
        } else {
            center_offset(height, 600)
        };
        let px = |rect: RectPx| scale.scale_rect(rect.translate(offset_x, offset_y));
        let dlu = |x, y, w, h| px(dlu_rect(x, y, w, h));
        // The sliders run one pixel wider and taller than the template
        // (retail stills: 181x22 and 129x22).
        let slider = |x, y, w, h| px(child_window(x, y, w, h));
        // The right-panel children follow the panel, not the dialog centring.
        let page = if stretch {
            menu_page::compute_responsive_layout(&OPTIONS_PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_layout(&OPTIONS_PAGE, width as u32, height as u32)
        };
        let button = |id| page.button_rect(id).expect("0xD5 page button");
        Self {
            title: page.title,
            warning: page.warning_monitor,
            status_help: page.status_help,
            zoom: px(RectPx::new(351, 86, 180, COMBO_FACE_H)),
            sidebar: px(RectPx::new(351, 208, 180, COMBO_FACE_H)),
            trackbars: [
                (LauncherTrackbarId::Detail, slider(89, 53, 120, 13)),
                (LauncherTrackbarId::Difficulty, slider(92, 128, 120, 13)),
                (LauncherTrackbarId::Scroll, slider(238, 203, 120, 13)),
                (LauncherTrackbarId::Score, slider(82, 289, 85, 13)),
                (LauncherTrackbarId::Sound, slider(179, 289, 85, 13)),
                (LauncherTrackbarId::Voice, slider(277, 289, 85, 13)),
            ],
            checkboxes: [
                (LauncherCheckboxId::Tooltips, dlu(93, 188, 130, 10)),
                (LauncherCheckboxId::TargetLines, dlu(93, 203, 130, 10)),
                (LauncherCheckboxId::ShowHidden, dlu(93, 218, 130, 10)),
                // VERA's box sits under the Zoom combo, in the Display
                // Options band the template leaves empty there.
                (LauncherCheckboxId::FullScreen, dlu(234, 75, 120, 10)),
                // And this one on the same row, under the Visual Details
                // slider, where the band is empty too.
                (LauncherCheckboxId::OriginalShape, dlu(89, 75, 130, 10)),
                // The Yuri's Revenge style box continues the UI Options column
                // under Show Hidden, where the band is empty above Audio.
                (LauncherCheckboxId::YrOriginalStyle, dlu(93, 233, 130, 10)),
                // Shift-Click Batch Queue sits beside it, under the Scroll
                // Rate slider, where the UI Options band is empty too.
                (LauncherCheckboxId::ShiftQueue, dlu(238, 233, 120, 10)),
                // VERA's Developer Options band under Audio, above the
                // status line, where the template has nothing.
                (LauncherCheckboxId::WolTestService, dlu(93, 323, 200, 10)),
            ],
            buttons: [
                (LauncherParentResult::Keyboard, button(KEYBOARD_BUTTON)),
                (LauncherParentResult::Network, button(NETWORK_BUTTON)),
                (LauncherParentResult::Back, button(MAIN_MENU_BUTTON)),
            ],
            scale,
            offset_x,
            offset_y,
            screen: (width.max(0) as u32, height.max(0) as u32),
            battle_frame: (width.max(0) as u32, height.max(0) as u32),
        }
    }

    /// The same page for a battle frame other than the window: a zoomed or
    /// 4:3 match (`App::match_zoom_geometry`).
    pub(crate) fn with_battle_frame(mut self, battle_frame: (u32, u32)) -> Self {
        self.battle_frame = battle_frame;
        self
    }

    /// The Sidebar list's rows, top to bottom.
    pub(crate) fn sidebar_choices(&self) -> Vec<SidebarSize> {
        SidebarSize::choices(self.battle_frame.0, self.battle_frame.1)
    }

    /// The window size the zoom choices are for.
    pub(crate) fn screen(&self) -> (u32, u32) {
        self.screen
    }

    /// The Zoom list's rows, top to bottom.
    pub(crate) fn zoom_choices(&self) -> Vec<Zoom> {
        Zoom::choices(self.screen.0, self.screen.1)
    }

    fn label_rect(&self, x: i32, y: i32, w: i32, h: i32) -> RectPx {
        self.scale
            .scale_rect(dlu_rect(x, y, w, h).translate(self.offset_x, self.offset_y))
    }

    fn trackbar_rect(&self, id: LauncherTrackbarId) -> RectPx {
        self.trackbars
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .unwrap()
            .1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherLabelAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LauncherShellLabel<'a> {
    pub(crate) rect: RectPx,
    pub(crate) text: &'a str,
    pub(crate) align: LauncherLabelAlign,
    pub(crate) title: bool,
}

/// An open list: one row per [`LauncherOptionsLayout::zoom_choices`] or
/// [`LauncherOptionsLayout::sidebar_choices`], in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherComboPopup {
    pub(crate) combo: LauncherComboId,
    pub(crate) rect: RectPx,
    pub(crate) row_height: i32,
    pub(crate) rows: usize,
    pub(crate) selected: usize,
    pub(crate) hovered: Option<usize>,
}

impl LauncherComboPopup {
    fn row_at(self, x: i32, y: i32) -> Option<usize> {
        self.rect
            .contains(x, y)
            .then(|| ((y - self.rect.y) / self.row_height) as usize)
            .filter(|row| *row < self.rows)
    }

    pub(crate) fn row_rect(self, row: usize) -> RectPx {
        RectPx::new(
            self.rect.x,
            self.rect.y + row as i32 * self.row_height,
            self.rect.w,
            self.row_height,
        )
    }
}

/// The control-relative cursor and window in the authored pixels the native
/// trackbar and checkbox arithmetic runs on, so a stretched control admits the
/// same strip and thumb the paint side draws.
fn frame(rect: RectPx, x: i32, y: i32, scale: ShellScale) -> PhysicalControlFrame {
    let track = authored_track(rect, (x, y), scale);
    PhysicalControlFrame {
        local_x: track.local_x,
        local_y: track.local_y,
        width: track.width,
        height: track.height,
    }
}

impl OptionsDialogState {
    /// Native61E3AF..61E44E drops held/drag state once MK_LBUTTON is absent.
    /// Focus loss is the host's authoritative lost-gesture boundary.
    pub(crate) fn shell_cancel_pointer_gesture(&mut self) {
        self.capture = None;
        self.shell_interaction.pressed_button = None;
        self.shell_interaction.hovered_button = None;
    }
    pub(crate) fn shell_labels(
        &self,
        layout: &LauncherOptionsLayout,
    ) -> Vec<LauncherShellLabel<'_>> {
        use LauncherLabelAlign::{Center, Left, Right};
        let labels = &self.labels;
        let mut result = vec![LauncherShellLabel {
            rect: layout.title,
            text: &labels.options,
            align: Center,
            title: true,
        }];
        for (text, x, y, w, h, align) in [
            (labels.display_options.as_str(), 20, 18, 140, 10, Left),
            (&labels.game_options, 20, 93, 298, 10, Left),
            (&labels.ui_options, 20, 168, 298, 10, Left),
            (&labels.audio_options, 20, 254, 298, 10, Left),
            (&labels.visual_details, 89, 38, 60, 10, Left),
            (&self.detail_caption, 149, 38, 60, 10, Right),
            (ZOOM_LABEL, 234, 38, 120, 10, Left),
            (SIDEBAR_LABEL, 234, 113, 120, 10, Left),
            (&labels.difficulty, 92, 113, 60, 10, Left),
            (&self.difficulty_caption, 152, 113, 60, 10, Right),
            (&labels.scroll_rate, 238, 188, 60, 10, Left),
            (&self.scroll_caption, 298, 188, 60, 10, Right),
            (&labels.music_volume, 82, 274, 85, 10, Center),
            (&labels.sound_volume, 179, 274, 85, 10, Center),
            (&labels.voice_volume, 277, 274, 85, 10, Center),
            (DEVELOPER_OPTIONS_LABEL, 20, 308, 298, 10, Left),
            (&labels.blank, 2, 355, 303, 12, Left),
        ] {
            result.push(LauncherShellLabel {
                rect: layout.label_rect(x, y, w, h),
                text,
                align,
                title: false,
            });
        }
        result
    }

    pub(crate) fn shell_checkbox_label(&self, id: LauncherCheckboxId) -> &str {
        match id {
            LauncherCheckboxId::Tooltips => &self.labels.tooltips,
            LauncherCheckboxId::TargetLines => &self.labels.target_lines,
            LauncherCheckboxId::ShowHidden => &self.labels.show_hidden,
            LauncherCheckboxId::FullScreen => FULL_SCREEN_LABEL,
            LauncherCheckboxId::OriginalShape => ORIGINAL_SHAPE_LABEL,
            LauncherCheckboxId::YrOriginalStyle => YR_ORIGINAL_STYLE_LABEL,
            LauncherCheckboxId::ShiftQueue => SHIFT_QUEUE_LABEL,
            LauncherCheckboxId::WolTestService => WOL_TEST_SERVICE_LABEL,
        }
    }

    pub(crate) fn shell_button_label(&self, id: LauncherParentResult) -> &str {
        match id {
            LauncherParentResult::Keyboard => &self.labels.keyboard,
            LauncherParentResult::Network => &self.labels.network,
            LauncherParentResult::Back => &self.labels.main_menu,
            LauncherParentResult::Terminal => "",
        }
    }

    pub(crate) fn shell_title_text(&self) -> &str {
        &self.labels.options
    }

    pub(crate) fn shell_trackbar_thumb_left(
        &self,
        id: LauncherTrackbarId,
        window: RectPx,
        scale: ShellScale,
    ) -> i32 {
        thumb_left_at(
            i32::from(self.trackbar_position(id)),
            window,
            id.plaque_reserve(),
            i32::from(id.maximum()),
            scale,
        )
    }

    pub(crate) fn shell_checkbox_checked(&self, id: LauncherCheckboxId) -> bool {
        match id {
            LauncherCheckboxId::Tooltips => self.values.tooltips,
            LauncherCheckboxId::TargetLines => self.values.target_lines,
            LauncherCheckboxId::ShowHidden => self.values.show_hidden,
            LauncherCheckboxId::FullScreen => self.values.full_screen,
            LauncherCheckboxId::OriginalShape => self.values.original_shape,
            LauncherCheckboxId::YrOriginalStyle => self.values.yr_original_style,
            LauncherCheckboxId::ShiftQueue => self.values.shift_queue,
            LauncherCheckboxId::WolTestService => self.values.wol_test_service,
        }
    }

    /// The zoom the face shows: a fixed choice this window can't reach shows
    /// as the step it gives way to.
    pub(crate) fn shell_selected_zoom(&self, layout: &LauncherOptionsLayout) -> Zoom {
        match self.zoom {
            Zoom::Auto => Zoom::Auto,
            fixed => Zoom::Percent(fixed.percent(layout.screen.0, layout.screen.1)),
        }
    }
    pub(crate) fn shell_button_hovered(&self, id: LauncherParentResult) -> bool {
        self.shell_interaction.hovered_button == Some(id)
    }
    pub(crate) fn shell_button_pressed(&self, id: LauncherParentResult) -> bool {
        self.shell_interaction.pressed_button == Some(id) && self.shell_button_hovered(id)
    }

    /// The sidebar size the face shows: a size this battle frame has no room
    /// for shows as the size it gives way to.
    pub(crate) fn shell_selected_sidebar(&self, layout: &LauncherOptionsLayout) -> SidebarSize {
        let (width, height) = layout.battle_frame;
        self.values.sidebar.in_effect(width, height)
    }

    pub(crate) fn shell_popup(&self, layout: &LauncherOptionsLayout) -> Option<LauncherComboPopup> {
        let combo = self.open_combo?;
        // The list box hangs under the painted face; its rows use this
        // window's dropdown device metrics, the same the combo pages use.
        // Unlike the template's 74-DLU drop allocation (ComboDropWin
        // `0x006180CF..0x00618205`, five whole rows), it grows with the
        // choices, so it never scrolls: a 4K window's nine zoom rows still end
        // well above the page's bottom edge.
        let (face_rect, rows, selected) = match combo {
            LauncherComboId::Zoom => {
                let choices = layout.zoom_choices();
                let shown = self.shell_selected_zoom(layout);
                let selected = choices.iter().position(|zoom| *zoom == shown);
                (layout.zoom, choices.len(), selected)
            }
            LauncherComboId::Sidebar => {
                let choices = layout.sidebar_choices();
                let shown = self.shell_selected_sidebar(layout);
                let selected = choices.iter().position(|size| *size == shown);
                (layout.sidebar, choices.len(), selected)
            }
        };
        let metrics = DropdownMetrics::scaled(layout.scale);
        let face = combo_face_rect(face_rect, layout.scale);
        Some(LauncherComboPopup {
            combo,
            rect: RectPx::new(
                face_rect.x,
                face.y + face.h + 1,
                face_rect.w,
                rows as i32 * metrics.row_h,
            ),
            row_height: metrics.row_h,
            rows,
            selected: selected.unwrap_or(0),
            hovered: self.shell_interaction.popup_hovered,
        })
    }

    /// The status help the line shows: what the last dialog hit test wrote.
    /// While a slider or button holds the mouse capture, or a list is open,
    /// the dialog gets no hit test and the text stays.
    pub(crate) fn shell_status_help(&self) -> Option<LauncherStatusHelp> {
        self.shell_interaction.status_help
    }

    /// Status help (`0x00604729..0x00604838`) for the control under the
    /// pointer. Hover is enable-unfiltered; an open list is a window of its
    /// own with no entry.
    pub(crate) fn shell_status_help_key(
        &self,
        x: i32,
        y: i32,
        layout: &LauncherOptionsLayout,
    ) -> Option<LauncherStatusHelp> {
        if self
            .shell_popup(layout)
            .is_some_and(|popup| popup.rect.contains(x, y))
        {
            return None;
        }
        if let Some((id, _)) = layout.buttons.iter().find(|(_, rect)| rect.contains(x, y)) {
            let control = match id {
                LauncherParentResult::Keyboard => KEYBOARD_BUTTON,
                LauncherParentResult::Network => NETWORK_BUTTON,
                LauncherParentResult::Back | LauncherParentResult::Terminal => MAIN_MENU_BUTTON,
            };
            return OPTIONS_PAGE
                .button(control)
                .map(|button| LauncherStatusHelp::Csf(button.tooltip_key));
        }
        if let Some((id, _)) = layout
            .trackbars
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
        {
            return Some(LauncherStatusHelp::Csf(match id {
                LauncherTrackbarId::Detail => "STT:MainOptSliderVisual",
                LauncherTrackbarId::Difficulty => "STT:MainOptSliderDifficulty",
                LauncherTrackbarId::Scroll => "STT:MainOptSliderScroll",
                LauncherTrackbarId::Score => "STT:MainOptSliderMusic",
                LauncherTrackbarId::Sound => "STT:MainOptSliderSound",
                LauncherTrackbarId::Voice => "STT:MainOptSliderVoice",
            }));
        }
        if let Some((id, _)) = layout
            .checkboxes
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
        {
            return Some(match id {
                LauncherCheckboxId::Tooltips => LauncherStatusHelp::Csf("STT:MainOptCBoxTooltips"),
                LauncherCheckboxId::TargetLines => {
                    LauncherStatusHelp::Csf("STT:MainOptCBoxTargetLines")
                }
                LauncherCheckboxId::ShowHidden => LauncherStatusHelp::Csf("STT:MainOptCBoxHidden"),
                LauncherCheckboxId::FullScreen => LauncherStatusHelp::Text(FULL_SCREEN_HELP),
                LauncherCheckboxId::OriginalShape => LauncherStatusHelp::Text(ORIGINAL_SHAPE_HELP),
                LauncherCheckboxId::YrOriginalStyle => {
                    LauncherStatusHelp::Text(YR_ORIGINAL_STYLE_HELP)
                }
                LauncherCheckboxId::ShiftQueue => LauncherStatusHelp::Text(SHIFT_QUEUE_HELP),
                LauncherCheckboxId::WolTestService => {
                    LauncherStatusHelp::Text(WOL_TEST_SERVICE_HELP)
                }
            });
        }
        if layout.sidebar.contains(x, y) {
            return Some(LauncherStatusHelp::Text(SIDEBAR_HELP));
        }
        layout
            .zoom
            .contains(x, y)
            .then_some(LauncherStatusHelp::Text(ZOOM_HELP))
    }

    /// The caller routes all pointer messages here while this parent is topmost.
    pub(crate) fn shell_mouse_down(&mut self, x: i32, y: i32, layout: &LauncherOptionsLayout) {
        let scale = layout.scale;
        if self.capture.is_some() {
            return;
        }
        if let Some(popup) = self.shell_popup(layout) {
            // ComboDropWin 0x0060E4E9..0x0060E500 plays Rules+0x1A8
            // (GUIComboCloseSound) before selection or outside dismissal.
            self.pending_events
                .push(LauncherOptionsEvent::Cue(LauncherCue::ComboClose));
            let row = popup.row_at(x, y);
            match popup.combo {
                LauncherComboId::Zoom => {
                    match row.and_then(|row| layout.zoom_choices().get(row).copied()) {
                        Some(zoom) => self.select_zoom(zoom),
                        None => self.open_combo = None,
                    }
                }
                LauncherComboId::Sidebar => {
                    match row.and_then(|row| layout.sidebar_choices().get(row).copied()) {
                        Some(size) => self.select_sidebar(size),
                        None => self.open_combo = None,
                    }
                }
            }
            self.shell_interaction = ShellInteraction::default();
            return;
        }
        self.shell_mouse_move(x, y, layout);
        if let Some((id, _)) = layout.buttons.iter().find(|(_, rect)| rect.contains(x, y)) {
            self.shell_interaction.pressed_button = Some(*id);
            self.main_button_mouse_down();
        } else if let Some((id, rect)) = layout
            .trackbars
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
        {
            self.trackbar_mouse_down(*id, frame(*rect, x, y, scale));
        } else if let Some((id, rect)) = layout
            .checkboxes
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
        {
            self.checkbox_mouse_down(*id, frame(*rect, x, y, scale));
        } else if layout.zoom.contains(x, y) {
            self.combo_mouse_down(LauncherComboId::Zoom, frame(layout.zoom, x, y, scale));
            self.shell_interaction.hovered_button = None;
        } else if layout.sidebar.contains(x, y) {
            self.combo_mouse_down(LauncherComboId::Sidebar, frame(layout.sidebar, x, y, scale));
            self.shell_interaction.hovered_button = None;
        }
    }

    pub(crate) fn shell_mouse_move(&mut self, x: i32, y: i32, layout: &LauncherOptionsLayout) {
        let scale = layout.scale;
        if self.capture.is_none()
            && self.shell_interaction.pressed_button.is_none()
            && self.open_combo.is_none()
        {
            self.shell_interaction.status_help = self.shell_status_help_key(x, y, layout);
        }
        if let Some(hold) = self.capture {
            let rect = layout.trackbar_rect(hold.id);
            self.trackbar_mouse_move(hold.id, frame(rect, x, y, scale));
            self.shell_interaction.hovered_button = None;
            return;
        }
        if self.open_combo.is_some() {
            self.shell_interaction.popup_hovered = self
                .shell_popup(layout)
                .and_then(|popup| popup.row_at(x, y));
            self.shell_interaction.hovered_button = None;
        } else {
            self.shell_interaction.hovered_button = layout
                .buttons
                .iter()
                .find(|(_, rect)| rect.contains(x, y))
                .map(|(id, _)| *id);
        }
    }

    pub(crate) fn shell_mouse_up(&mut self, x: i32, y: i32, layout: &LauncherOptionsLayout) {
        if let Some(hold) = self.capture {
            self.trackbar_mouse_up(hold.id);
        }
        self.shell_mouse_move(x, y, layout);
        if let Some(id) = self.shell_interaction.pressed_button.take()
            && self.shell_interaction.hovered_button == Some(id)
            && self.open_combo.is_none()
        {
            self.request_result(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::main_menu_dialogs::options::{
        LauncherCue, LauncherOptionsEvent, LauncherOptionsLabels, LauncherOptionsValues,
    };
    use crate::ui::shell::trackbar::TrackbarHold;

    fn state() -> OptionsDialogState {
        OptionsDialogState::new(
            LauncherOptionsLabels::resolve(&|_| None),
            LauncherOptionsValues::default(),
            Zoom::Auto,
            true,
        )
    }

    #[test]
    fn a_stretched_slider_maps_the_pointer_back_to_its_authored_track() {
        let mut dialog = state();
        let base = LauncherOptionsLayout::new(800, 600);
        let scale = ShellScale::from_screen(1600, 1200);
        let wide = LauncherOptionsLayout::responsive(1600, 1200);
        let authored = base.trackbar_rect(LauncherTrackbarId::Score);
        let stretched = wide.trackbar_rect(LauncherTrackbarId::Score);
        assert_eq!(stretched, scale.scale_rect(authored));
        // The thumb travels by the same authored span, then stretches.
        let thumb = dialog.shell_trackbar_thumb_left(
            LauncherTrackbarId::Score,
            authored,
            ShellScale::RETAIL,
        );
        let wide_thumb =
            dialog.shell_trackbar_thumb_left(LauncherTrackbarId::Score, stretched, scale);
        assert_eq!(wide_thumb, (thumb as f32 * scale.factor()).round() as i32);
        // The same authored point addressed on either page takes the same
        // thumb and the same position.
        let press = (authored.x + thumb + 5, authored.y + authored.h - 3);
        dialog.shell_mouse_down(press.0, press.1, &base);
        let held = Some(TrackbarHold {
            id: LauncherTrackbarId::Score,
            dragging: true,
        });
        assert_eq!(dialog.capture, held);
        let pressed_position = dialog.trackbar_position(LauncherTrackbarId::Score);
        dialog.shell_mouse_up(press.0, press.1, &base);
        let device = scale.scale_rect(RectPx::new(press.0, press.1, 1, 1));
        let mut stretched_dialog = state();
        stretched_dialog.shell_mouse_down(device.x, device.y, &wide);
        assert_eq!(
            stretched_dialog.capture, held,
            "{device:?} over {stretched:?}"
        );
        assert_eq!(
            stretched_dialog.trackbar_position(LauncherTrackbarId::Score),
            pressed_position
        );
        // A jump press beside the thumb quantizes on the authored width.
        let jump = (authored.x + authored.w - 2, authored.y + authored.h - 3);
        dialog.shell_mouse_down(jump.0, jump.1, &base);
        let jumped = dialog.trackbar_position(LauncherTrackbarId::Score);
        dialog.shell_mouse_up(jump.0, jump.1, &base);
        let device = scale.scale_rect(RectPx::new(jump.0, jump.1, 1, 1));
        let mut stretched_dialog = state();
        stretched_dialog.shell_mouse_down(device.x, device.y, &wide);
        assert_eq!(
            stretched_dialog.trackbar_position(LauncherTrackbarId::Score),
            jumped,
            "{device:?} over {stretched:?}"
        );
    }

    #[test]
    fn status_help_names_the_control_under_the_pointer() {
        let state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let at = |rect: RectPx| state.shell_status_help_key(rect.x + 2, rect.y + 2, &layout);
        let csf = |key| Some(LauncherStatusHelp::Csf(key));
        assert_eq!(at(layout.buttons[0].1), csf("STT:MainOptButtonKeyboard"));
        assert_eq!(at(layout.buttons[2].1), csf("STT:MainOptButtonBack"));
        assert_eq!(
            at(layout.trackbar_rect(LauncherTrackbarId::Score)),
            csf("STT:MainOptSliderMusic")
        );
        // The display controls retail never had explain themselves in VERA text.
        assert_eq!(at(layout.zoom), Some(LauncherStatusHelp::Text(ZOOM_HELP)));
        assert_eq!(
            at(layout.sidebar),
            Some(LauncherStatusHelp::Text(SIDEBAR_HELP))
        );
        assert_eq!(
            at(layout.checkboxes[3].1),
            Some(LauncherStatusHelp::Text(FULL_SCREEN_HELP))
        );
        assert_eq!(
            at(layout.checkboxes[4].1),
            Some(LauncherStatusHelp::Text(ORIGINAL_SHAPE_HELP))
        );
        assert_eq!(
            at(layout.checkboxes[5].1),
            Some(LauncherStatusHelp::Text(YR_ORIGINAL_STYLE_HELP))
        );
        assert_eq!(state.shell_status_help_key(5, 5, &layout), None);
    }

    #[test]
    fn physical_slider_capture_crosses_other_controls_without_activating_them() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let rect = layout.trackbar_rect(LauncherTrackbarId::Score);
        let thumb = state.shell_trackbar_thumb_left(LauncherTrackbarId::Score, rect, layout.scale);
        state.shell_mouse_down(rect.x + thumb + 5, rect.y + 10, &layout);
        assert_eq!(
            state.capture,
            Some(TrackbarHold {
                id: LauncherTrackbarId::Score,
                dragging: true,
            })
        );
        state.shell_mouse_move(799, 550, &layout);
        state.shell_mouse_up(799, 550, &layout);
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Score), 10);
        assert_eq!(state.capture, None);
        let output = state.drain_output();
        assert_eq!(output.events, [LauncherOptionsEvent::ScorePreview(1.0)]);
        assert_eq!(output.result, None);
    }

    #[test]
    fn lost_focus_cancels_drag_before_pointer_reentry() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        // Fixture starts at4: original128px/50px plaque projection puts the
        // thumb at client27 (screen150), so155 is inside its12px capture box.
        state.shell_mouse_down(155, 475, &layout);
        assert_eq!(
            state.capture,
            Some(TrackbarHold {
                id: LauncherTrackbarId::Score,
                dragging: true,
            })
        );
        state.shell_cancel_pointer_gesture();
        let before = state.pack();
        state.shell_mouse_move(799, 550, &layout);
        state.shell_mouse_up(799, 550, &layout);
        assert_eq!(state.pack(), before);
        assert!(state.drain_output().events.is_empty());
    }

    #[test]
    fn every_slider_press_holds_the_mouse_until_release() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let held = Some(TrackbarHold {
            id: LauncherTrackbarId::Difficulty,
            dragging: false,
        });
        // Above the admitted strip the press only takes the capture.
        state.shell_mouse_down(140, 210, &layout);
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Difficulty),
            1,
            "native lower strip rejects y=2"
        );
        assert_eq!(state.capture, held);
        state.shell_mouse_up(140, 210, &layout);
        // Beside the thumb it jumps once and holds without dragging.
        state.shell_mouse_down(140, 218, &layout);
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Difficulty), 0);
        assert_eq!(state.capture, held);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::GenericClick)]
        );
        state.shell_mouse_move(317, 218, &layout);
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Difficulty), 0);
        let back = layout.buttons[2].1;
        state.shell_mouse_move(back.x + 5, back.y + 5, &layout);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptSliderDifficulty"))
        );
        state.shell_mouse_up(back.x + 5, back.y + 5, &layout);
        assert_eq!(state.capture, None);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptButtonBack"))
        );
        assert_eq!(state.drain_output().result, None);
        // The checkbox text takes no press.
        state.shell_mouse_down(170, 310, &layout);
        assert!(state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        state.shell_mouse_down(150, 310, &layout);
        assert!(!state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::Checkbox)]
        );
    }

    #[test]
    fn button_release_requires_the_pressed_button_and_popup_dismissal_consumes_click() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        state.shell_mouse_down(700, 210, &layout);
        assert!(state.shell_button_pressed(LauncherParentResult::Keyboard));
        state.shell_mouse_up(700, 250, &layout);
        assert_eq!(state.drain_output().result, None);
        state.shell_mouse_down(520, 95, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::Zoom));
        state.shell_mouse_down(700, 550, &layout);
        state.shell_mouse_up(700, 550, &layout);
        assert_eq!(state.open_combo, None);
        let dismissed = state.drain_output();
        assert_eq!(dismissed.result, None);
        assert_eq!(
            dismissed.events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboOpen),
                LauncherOptionsEvent::Cue(LauncherCue::ComboClose)
            ]
        );
        state.shell_mouse_down(700, 550, &layout);
        let down = state.drain_output();
        assert_eq!(
            down.events,
            [LauncherOptionsEvent::Cue(LauncherCue::MainButton)]
        );
        assert_eq!(down.result, None);
        state.shell_mouse_up(700, 550, &layout);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Back)
        );
    }

    #[test]
    fn zoom_selection_uses_the_painted_row_and_emits_at_once() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        state.shell_mouse_down(520, 95, &layout);
        state.drain_output();
        let popup = state.shell_popup(&layout).unwrap();
        // 800x600 has room for Auto and 100% only.
        assert_eq!((popup.rows, popup.selected), (2, 0));
        assert_eq!(popup.rect.h, 2 * popup.row_height);
        let row = popup.row_rect(1);
        state.shell_mouse_move(row.x + 10, row.y + 5, &layout);
        assert_eq!(state.shell_popup(&layout).unwrap().hovered, Some(1));
        state.shell_mouse_down(row.x + 10, row.y + 5, &layout);
        assert_eq!(state.shell_selected_zoom(&layout), Zoom::Percent(100));
        assert!(state.shell_popup(&layout).is_none());
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboClose),
                LauncherOptionsEvent::ZoomSelected(Zoom::Percent(100))
            ]
        );

        // A bigger window lists more steps, and a choice it can't reach
        // shows and selects as the step it gives way to.
        let mut state = OptionsDialogState::new(
            LauncherOptionsLabels::resolve(&|_| None),
            LauncherOptionsValues::default(),
            Zoom::Percent(300),
            true,
        );
        state.open_combo = Some(LauncherComboId::Zoom);
        let layout = LauncherOptionsLayout::responsive(1920, 1080);
        assert_eq!(state.shell_selected_zoom(&layout), Zoom::Percent(200));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!((popup.rows, popup.selected), (4, 3));
        let below = popup.row_rect(4);
        state.shell_mouse_down(below.x + 10, below.y + 5, &layout);
        assert_eq!(
            state.zoom,
            Zoom::Percent(300),
            "below the last row picks nothing"
        );
    }

    #[test]
    fn sidebar_list_offers_the_sizes_the_battle_frame_has_room_for() {
        let mut state = state();
        // A 1920x1080 window at 100%: every size fits.
        let layout = LauncherOptionsLayout::new(800, 600).with_battle_frame((1920, 1080));
        let face = layout.sidebar;
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::Sidebar));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::Sidebar, 3, 0)
        );
        let xl = popup.row_rect(2);
        state.drain_output();
        state.shell_mouse_down(xl.x + 10, xl.y + 5, &layout);
        assert_eq!(state.open_combo, None);
        assert_eq!(
            state.shell_selected_sidebar(&layout),
            SidebarSize::ExtraLarge
        );
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboClose),
                LauncherOptionsEvent::SidebarSelected(SidebarSize::ExtraLarge)
            ]
        );

        // Zoomed in on a 3440x1440 screen, XL has no room: it shows as Large,
        // and the list stops there.
        let zoomed = LauncherOptionsLayout::new(800, 600).with_battle_frame((1720, 720));
        assert_eq!(state.shell_selected_sidebar(&zoomed), SidebarSize::Large);
        state.shell_mouse_down(face.x + 10, face.y + 5, &zoomed);
        let popup = state.shell_popup(&zoomed).unwrap();
        assert_eq!((popup.rows, popup.selected), (2, 1));
    }

    #[test]
    fn full_screen_box_sits_under_the_zoom_combo_and_applies_at_once() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let (id, rect) = layout.checkboxes[3];
        assert_eq!(id, LauncherCheckboxId::FullScreen);
        assert_eq!(rect, RectPx::new(351, 122, 180, 16));
        assert!(rect.y >= layout.zoom.y + layout.zoom.h);
        assert_eq!(state.shell_checkbox_label(id), FULL_SCREEN_LABEL);
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert!(state.shell_checkbox_checked(id));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::FullScreenChanged(true)
            ]
        );
    }

    #[test]
    fn original_shape_box_shares_the_full_screen_row_and_applies_at_once() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let (id, rect) = layout.checkboxes[4];
        let full_screen = layout.checkboxes[3].1;
        assert_eq!(id, LauncherCheckboxId::OriginalShape);
        // Level with Full Screen, in the empty spot under Visual Details:
        // clear of the slider, Full Screen and the Game Options heading.
        assert_eq!((rect.y, rect.h), (full_screen.y, full_screen.h));
        assert!(rect.x + rect.w <= full_screen.x);
        let detail = layout.trackbar_rect(LauncherTrackbarId::Detail);
        assert!(rect.y >= detail.y + detail.h);
        let heading = state
            .shell_labels(&layout)
            .into_iter()
            .find(|label| label.text == "Game Options")
            .expect("Game Options heading")
            .rect;
        assert!(rect.y + rect.h <= heading.y);
        assert_eq!(state.shell_checkbox_label(id), ORIGINAL_SHAPE_LABEL);
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert!(state.shell_checkbox_checked(id));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::OriginalShapeChanged(true)
            ]
        );
    }

    #[test]
    fn original_yr_style_box_shows_and_previews_the_selected_art() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let (id, rect) = layout.checkboxes[5];
        assert_eq!(id, LauncherCheckboxId::YrOriginalStyle);
        assert_eq!(state.shell_checkbox_label(id), YR_ORIGINAL_STYLE_LABEL);
        assert_eq!(
            state.shell_status_help_key(rect.x + 5, rect.y + 5, &layout),
            Some(LauncherStatusHelp::Text(YR_ORIGINAL_STYLE_HELP))
        );
        assert!(!state.shell_checkbox_checked(id));

        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert!(state.shell_checkbox_checked(id));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::YrOriginalStyleChanged(true),
            ]
        );
    }

    #[test]
    fn shift_queue_box_sits_under_scroll_rate_and_applies_at_once() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let (id, rect) = layout.checkboxes[6];
        assert_eq!(id, LauncherCheckboxId::ShiftQueue);
        // Beside the Original YR Style option, under the Scroll Rate slider,
        // clear of both and of the Audio Options heading.
        let yr_original_style = layout.checkboxes[5].1;
        let scroll = layout
            .trackbars
            .iter()
            .find(|(id, _)| *id == LauncherTrackbarId::Scroll)
            .unwrap()
            .1;
        assert_eq!((rect.y, rect.h), (yr_original_style.y, yr_original_style.h));
        assert!(rect.x >= yr_original_style.x + yr_original_style.w);
        assert_eq!(rect.x, scroll.x);
        assert!(rect.y >= scroll.y + scroll.h);
        let heading = state
            .shell_labels(&layout)
            .into_iter()
            .find(|label| label.text == "Audio Options")
            .expect("Audio Options heading")
            .rect;
        assert!(rect.y + rect.h <= heading.y);
        assert_eq!(state.shell_checkbox_label(id), SHIFT_QUEUE_LABEL);
        // Off until the player ticks it.
        assert!(!state.shell_checkbox_checked(id));
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert!(state.shell_checkbox_checked(id));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::ShiftQueueChanged(true)
            ]
        );
    }

    #[test]
    fn wol_test_service_box_sits_in_its_developer_band_and_applies_at_once() {
        let mut state = state();
        let layout = LauncherOptionsLayout::new(800, 600);
        let (id, rect) = layout.checkboxes[7];
        assert_eq!(id, LauncherCheckboxId::WolTestService);
        let labels = state.shell_labels(&layout);
        let heading = labels
            .iter()
            .find(|label| label.text == DEVELOPER_OPTIONS_LABEL)
            .expect("Developer Options heading")
            .rect;
        // Under the volume sliders and its heading, clear of the status line.
        let lowest_slider = layout
            .trackbars
            .iter()
            .map(|(_, rect)| rect.y + rect.h)
            .max()
            .unwrap();
        assert!(heading.y >= lowest_slider);
        assert!(rect.y >= heading.y + heading.h);
        assert!(rect.y + rect.h <= layout.status_help.y);
        assert_eq!(state.shell_checkbox_label(id), WOL_TEST_SERVICE_LABEL);
        assert!(!state.shell_checkbox_checked(id));
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert!(state.shell_checkbox_checked(id));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::WolTestServiceChanged(true)
            ]
        );
    }
}
