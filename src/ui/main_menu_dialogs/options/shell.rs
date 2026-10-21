//! Physical-pixel presentation and input for launcher resource 0xD5.
//!
//! Retail RT_DIALOG/0xD5/1033 provides the DLU rectangles; common setup
//! 0x00608CD0/0x00609730 anchors the rail, and 0x0060B950 adjusts its title.
//! This module projects the retained OptionsDialogState instead of owning a
//! second set of option values or bypassing its ordered preview events.
//! VERA's page lays its controls out on the main page's bands and rows.

use super::{
    DEVELOPER_OPTIONS_LABEL, DISPLAY_AUTO_LABEL, DISPLAY_HELP, DISPLAY_LABEL, DisplayChoice,
    DisplayTarget, FPS_COUNTER_HELP, FPS_COUNTER_LABEL, FULL_SCREEN_HELP, FULL_SCREEN_LABEL,
    LauncherButton, LauncherCheckboxId, LauncherComboId, LauncherCue, LauncherOptionsEvent,
    LauncherParentResult, LauncherTrackbarId, ORIGINAL_SHAPE_HELP, ORIGINAL_SHAPE_LABEL,
    OptionsDialogState, OptionsPage, PhysicalControlFrame, RENDER_INTERPOLATION_HELP,
    RENDER_INTERPOLATION_LABEL, SHIFT_QUEUE_HELP, SHIFT_QUEUE_LABEL, SIDEBAR_HELP, SIDEBAR_LABEL,
    SidebarSize, VERA_OPTIONS_HELP, VERA_OPTIONS_LABEL, WOL_TEST_SERVICE_HELP,
    WOL_TEST_SERVICE_LABEL, YR_ORIGINAL_STYLE_HELP, YR_ORIGINAL_STYLE_LABEL, ZOOM_HELP, ZOOM_LABEL,
    Zoom,
};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale, center_offset, child_window, dlu_rect};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageSpec};
use crate::ui::shell::trackbar::{authored_track, thumb_left_at};
use crate::ui::skirmish_shell::{COMBO_FACE_H, DropdownMetrics, combo_face_rect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ShellInteraction {
    hovered_button: Option<LauncherButton>,
    pressed_button: Option<LauncherButton>,
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
/// VERA's own button under Network. Not a retail control: the id only names
/// its rect in the page layout.
pub(crate) const VERA_OPTIONS_BUTTON: u16 = 0x7FFF;

/// `0xD5` as a family page (Session `+0x30D8` clear at the main menu): the
/// right-panel column is Keyboard and Network (`0x006092CB`), then Main Menu
/// (`0x0060982C`); the heading and status line sit at the family places.
/// VERA adds its VERA20K Options button in the next slot, the third stacked top
/// the family pages use. Button captions come from the dialog's own labels.
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
        // The string table has nothing for it: its caption and help are
        // VERA_OPTIONS_LABEL and VERA_OPTIONS_HELP.
        MenuPageButtonSpec {
            id: VERA_OPTIONS_BUTTON,
            dlu_top: 176,
            csf_key: "",
            tooltip_key: "",
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

/// VERA's page of `0xD5`: only Back in the column, in Main Menu's place, as
/// on the Keyboard page `0xA3`, with the Keyboard page's Back help.
pub(crate) const VERA_OPTIONS_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0x00D5),
    // The heading is VERA's own text, VERA_OPTIONS_LABEL.
    title_key: "",
    stacked: &[],
    back: MenuPageButtonSpec {
        id: MAIN_MENU_BUTTON,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:KeyboardButtonBack",
        result: None,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLayout {
    /// The page these controls are for.
    pub(crate) page: OptionsPage,
    pub(crate) title: RectPx,
    pub(crate) warning: RectPx,
    /// Status line `0x695`.
    pub(crate) status_help: RectPx,
    /// The page's list, in the resolution combo's retail place: Zoom on the
    /// main page, Sidebar on VERA's.
    pub(crate) combos: Vec<(LauncherComboId, RectPx)>,
    pub(crate) trackbars: Vec<(LauncherTrackbarId, RectPx)>,
    pub(crate) checkboxes: Vec<(LauncherCheckboxId, RectPx)>,
    pub(crate) buttons: Vec<(LauncherButton, RectPx)>,
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
    pub(crate) fn new(width: i32, height: i32, page: OptionsPage) -> Self {
        Self::build(width, height, ShellScale::RETAIL, page)
    }

    /// The same dialog scaled to fit the window.
    pub(crate) fn responsive(width: i32, height: i32, page: OptionsPage) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height), page)
    }

    fn build(width: i32, height: i32, scale: ShellScale, page: OptionsPage) -> Self {
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
        let spec = match page {
            OptionsPage::Main => &OPTIONS_PAGE,
            OptionsPage::Vera => &VERA_OPTIONS_PAGE,
        };
        let panel = if stretch {
            menu_page::compute_responsive_layout(spec, width as u32, height as u32)
        } else {
            menu_page::compute_layout(spec, width as u32, height as u32)
        };
        let button = |id| panel.button_rect(id).expect("0xD5 page button");
        // Either page's list takes the resolution combo's place.
        let combo = px(RectPx::new(351, 86, 180, COMBO_FACE_H));
        // VERA's page holds two lists in the Display Options band's right
        // column, each under its own caption. The band spans the Display
        // Options heading's end to the UI Options heading (45..151 px), and a
        // caption is 16 px tall, so the four rows stack with no spare:
        // caption 50..66, face 66..90, caption 94..110, face 110..134.
        let sidebar_combo = px(RectPx::new(351, 66, 180, COMBO_FACE_H));
        let display_combo = px(RectPx::new(351, 110, 180, COMBO_FACE_H));
        let (combos, trackbars, checkboxes, buttons) = match page {
            OptionsPage::Main => (
                vec![(LauncherComboId::Zoom, combo)],
                vec![
                    (LauncherTrackbarId::Detail, slider(89, 53, 120, 13)),
                    (LauncherTrackbarId::Difficulty, slider(92, 128, 120, 13)),
                    (LauncherTrackbarId::Scroll, slider(238, 203, 120, 13)),
                    (LauncherTrackbarId::Score, slider(82, 289, 85, 13)),
                    (LauncherTrackbarId::Sound, slider(179, 289, 85, 13)),
                    (LauncherTrackbarId::Voice, slider(277, 289, 85, 13)),
                ],
                vec![
                    (LauncherCheckboxId::Tooltips, dlu(93, 188, 130, 10)),
                    (LauncherCheckboxId::TargetLines, dlu(93, 203, 130, 10)),
                    (LauncherCheckboxId::ShowHidden, dlu(93, 218, 130, 10)),
                ],
                vec![
                    (
                        LauncherButton::Exit(LauncherParentResult::Keyboard),
                        button(KEYBOARD_BUTTON),
                    ),
                    (
                        LauncherButton::Exit(LauncherParentResult::Network),
                        button(NETWORK_BUTTON),
                    ),
                    (
                        LauncherButton::Page(OptionsPage::Vera),
                        button(VERA_OPTIONS_BUTTON),
                    ),
                    (
                        LauncherButton::Exit(LauncherParentResult::Back),
                        button(MAIN_MENU_BUTTON),
                    ),
                ],
            ),
            // VERA's page keeps the main page's bands and rows: one column
            // of boxes at the UI Options boxes' place, 15 DLUs apart.
            OptionsPage::Vera => (
                vec![
                    (LauncherComboId::Sidebar, sidebar_combo),
                    (LauncherComboId::Display, display_combo),
                ],
                Vec::new(),
                vec![
                    // Display Options: the window and how battles draw.
                    (LauncherCheckboxId::FullScreen, dlu(93, 38, 130, 10)),
                    (LauncherCheckboxId::OriginalShape, dlu(93, 53, 130, 10)),
                    (
                        LauncherCheckboxId::RenderInterpolation,
                        dlu(93, 68, 130, 10),
                    ),
                    // UI Options, in the main page's Game Options band.
                    (LauncherCheckboxId::YrOriginalStyle, dlu(93, 113, 130, 10)),
                    (LauncherCheckboxId::ShiftQueue, dlu(93, 128, 130, 10)),
                    // Developer Options, in the main page's UI Options band.
                    (LauncherCheckboxId::WolTestService, dlu(93, 188, 140, 10)),
                    (LauncherCheckboxId::FpsCounter, dlu(93, 203, 130, 10)),
                ],
                vec![(
                    LauncherButton::Page(OptionsPage::Main),
                    button(MAIN_MENU_BUTTON),
                )],
            ),
        };
        Self {
            page,
            title: panel.title,
            warning: panel.warning_monitor,
            status_help: panel.status_help,
            combos,
            trackbars,
            checkboxes,
            buttons,
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

    /// The slider's rect when this page has it.
    pub(crate) fn trackbar_rect(&self, id: LauncherTrackbarId) -> Option<RectPx> {
        self.trackbars
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
    }

    /// The list's rect when this page has it.
    pub(crate) fn combo_rect(&self, id: LauncherComboId) -> Option<RectPx> {
        self.combos
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
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
            text: self.shell_title_text(),
            align: Center,
            title: true,
        }];
        let statics: &[(&str, i32, i32, i32, i32, LauncherLabelAlign)] = match layout.page {
            OptionsPage::Main => &[
                (labels.display_options.as_str(), 20, 18, 140, 10, Left),
                (&labels.game_options, 20, 93, 298, 10, Left),
                (&labels.ui_options, 20, 168, 298, 10, Left),
                (&labels.audio_options, 20, 254, 298, 10, Left),
                (&labels.visual_details, 89, 38, 60, 10, Left),
                (&self.detail_caption, 149, 38, 60, 10, Right),
                (ZOOM_LABEL, 234, 38, 120, 10, Left),
                (&labels.difficulty, 92, 113, 60, 10, Left),
                (&self.difficulty_caption, 152, 113, 60, 10, Right),
                (&labels.scroll_rate, 238, 188, 60, 10, Left),
                (&self.scroll_caption, 298, 188, 60, 10, Right),
                (&labels.music_volume, 82, 274, 85, 10, Center),
                (&labels.sound_volume, 179, 274, 85, 10, Center),
                (&labels.voice_volume, 277, 274, 85, 10, Center),
                (&labels.blank, 2, 355, 303, 12, Left),
            ],
            // The main page's band headings at the same places.
            OptionsPage::Vera => &[
                (labels.display_options.as_str(), 20, 18, 140, 10, Left),
                (SIDEBAR_LABEL, 234, 31, 120, 10, Left),
                (DISPLAY_LABEL, 234, 58, 120, 10, Left),
                (&labels.ui_options, 20, 93, 298, 10, Left),
                (DEVELOPER_OPTIONS_LABEL, 20, 168, 298, 10, Left),
                (&labels.blank, 2, 355, 303, 12, Left),
            ],
        };
        for &(text, x, y, w, h, align) in statics {
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
            LauncherCheckboxId::RenderInterpolation => RENDER_INTERPOLATION_LABEL,
            LauncherCheckboxId::ShiftQueue => SHIFT_QUEUE_LABEL,
            LauncherCheckboxId::WolTestService => WOL_TEST_SERVICE_LABEL,
            LauncherCheckboxId::FpsCounter => FPS_COUNTER_LABEL,
        }
    }

    pub(crate) fn shell_button_label(&self, id: LauncherButton) -> &str {
        match id {
            LauncherButton::Exit(LauncherParentResult::Keyboard) => &self.labels.keyboard,
            LauncherButton::Exit(LauncherParentResult::Network) => &self.labels.network,
            LauncherButton::Exit(LauncherParentResult::Back) => &self.labels.main_menu,
            LauncherButton::Exit(LauncherParentResult::Terminal) => "",
            LauncherButton::Page(OptionsPage::Vera) => VERA_OPTIONS_LABEL,
            LauncherButton::Page(OptionsPage::Main) => &self.labels.back,
        }
    }

    /// The showing page's heading.
    pub(crate) fn shell_title_text(&self) -> &str {
        match self.page {
            OptionsPage::Main => &self.labels.options,
            OptionsPage::Vera => VERA_OPTIONS_LABEL,
        }
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
            LauncherCheckboxId::RenderInterpolation => self.values.render_interpolation,
            LauncherCheckboxId::ShiftQueue => self.values.shift_queue,
            LauncherCheckboxId::WolTestService => self.values.wol_test_service,
            LauncherCheckboxId::FpsCounter => self.values.fps_counter,
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
    pub(crate) fn shell_button_hovered(&self, id: LauncherButton) -> bool {
        self.shell_interaction.hovered_button == Some(id)
    }
    pub(crate) fn shell_button_pressed(&self, id: LauncherButton) -> bool {
        self.shell_interaction.pressed_button == Some(id) && self.shell_button_hovered(id)
    }

    /// The sidebar size the face shows: a size this battle frame has no room
    /// for shows as the size it gives way to.
    pub(crate) fn shell_selected_sidebar(&self, layout: &LauncherOptionsLayout) -> SidebarSize {
        let (width, height) = layout.battle_frame;
        self.values.sidebar.in_effect(width, height)
    }

    /// The Display list's rows, top to bottom: Auto, then every screen the
    /// window layer reported. Auto is first so a profile whose saved screen is
    /// gone still shows a picked row.
    pub(crate) fn display_choices(&self) -> Vec<DisplayTarget> {
        std::iter::once(DisplayTarget::Auto)
            .chain(
                self.displays
                    .iter()
                    .map(|choice| DisplayTarget::Fixed(choice.id.clone())),
            )
            .collect()
    }

    /// The Display face's text: the picked screen's summary, or just the id
    /// when the window layer no longer lists it.
    pub(crate) fn shell_display_label(&self) -> String {
        match &self.display {
            DisplayTarget::Auto => DISPLAY_AUTO_LABEL.to_owned(),
            DisplayTarget::Fixed(id) => self
                .displays
                .iter()
                .find(|choice| &choice.id == id)
                .map_or_else(|| id.clone(), DisplayChoice::label),
        }
    }

    /// The Display popup's rows, top to bottom: [`DISPLAY_AUTO_LABEL`], then
    /// each screen's [`DisplayChoice::label`].
    pub(crate) fn shell_display_rows(&self) -> Vec<String> {
        std::iter::once(DISPLAY_AUTO_LABEL.to_owned())
            .chain(self.displays.iter().map(DisplayChoice::label))
            .collect()
    }

    pub(crate) fn shell_popup(&self, layout: &LauncherOptionsLayout) -> Option<LauncherComboPopup> {
        let combo = self.open_combo?;
        // The list box hangs under the painted face; its rows use this
        // window's dropdown device metrics, the same the combo pages use.
        // Unlike the template's 74-DLU drop allocation (ComboDropWin
        // `0x006180CF..0x00618205`, five whole rows), it grows with the
        // choices, so it never scrolls: a 4K window's nine zoom rows still end
        // well above the page's bottom edge.
        let face_rect = layout.combo_rect(combo)?;
        let (rows, selected) = match combo {
            LauncherComboId::Zoom => {
                let choices = layout.zoom_choices();
                let shown = self.shell_selected_zoom(layout);
                let selected = choices.iter().position(|zoom| *zoom == shown);
                (choices.len(), selected)
            }
            LauncherComboId::Sidebar => {
                let choices = layout.sidebar_choices();
                let shown = self.shell_selected_sidebar(layout);
                let selected = choices.iter().position(|size| *size == shown);
                (choices.len(), selected)
            }
            LauncherComboId::Display => {
                let rows = self.display_choices();
                let selected = rows.iter().position(|target| target == &self.display);
                (rows.len(), selected)
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
            let (page, control) = match id {
                LauncherButton::Page(OptionsPage::Vera) => {
                    return Some(LauncherStatusHelp::Text(VERA_OPTIONS_HELP));
                }
                LauncherButton::Page(OptionsPage::Main) => (&VERA_OPTIONS_PAGE, MAIN_MENU_BUTTON),
                LauncherButton::Exit(LauncherParentResult::Keyboard) => {
                    (&OPTIONS_PAGE, KEYBOARD_BUTTON)
                }
                LauncherButton::Exit(LauncherParentResult::Network) => {
                    (&OPTIONS_PAGE, NETWORK_BUTTON)
                }
                LauncherButton::Exit(
                    LauncherParentResult::Back | LauncherParentResult::Terminal,
                ) => (&OPTIONS_PAGE, MAIN_MENU_BUTTON),
            };
            return page
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
                LauncherCheckboxId::RenderInterpolation => {
                    LauncherStatusHelp::Text(RENDER_INTERPOLATION_HELP)
                }
                LauncherCheckboxId::ShiftQueue => LauncherStatusHelp::Text(SHIFT_QUEUE_HELP),
                LauncherCheckboxId::WolTestService => {
                    LauncherStatusHelp::Text(WOL_TEST_SERVICE_HELP)
                }
                LauncherCheckboxId::FpsCounter => LauncherStatusHelp::Text(FPS_COUNTER_HELP),
            });
        }
        layout
            .combos
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(combo, _)| {
                LauncherStatusHelp::Text(match combo {
                    LauncherComboId::Zoom => ZOOM_HELP,
                    LauncherComboId::Sidebar => SIDEBAR_HELP,
                    LauncherComboId::Display => DISPLAY_HELP,
                })
            })
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
                LauncherComboId::Display => {
                    match row.and_then(|row| self.display_choices().get(row).cloned()) {
                        Some(target) => self.select_display(target),
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
        } else if let Some((combo, rect)) =
            layout.combos.iter().find(|(_, rect)| rect.contains(x, y))
        {
            self.combo_mouse_down(*combo, frame(*rect, x, y, scale));
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
            if let Some(rect) = layout.trackbar_rect(hold.id) {
                self.trackbar_mouse_move(hold.id, frame(rect, x, y, scale));
            }
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
            match id {
                LauncherButton::Exit(result) => self.request_result(result),
                LauncherButton::Page(page) => self.request_page(page),
            }
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
            vec![
                DisplayChoice {
                    id: "\\\\.\\DISPLAY1".to_owned(),
                    width: 1920,
                    height: 1080,
                    refresh_millihertz: Some(60_000),
                },
                DisplayChoice {
                    id: "\\\\.\\DISPLAY2".to_owned(),
                    width: 2560,
                    height: 1440,
                    refresh_millihertz: Some(144_000),
                },
            ],
            DisplayTarget::Auto,
        )
    }

    /// The dialog showing VERA's page, as after its slide-in.
    fn vera_state() -> OptionsDialogState {
        let mut state = state();
        state.show_page(OptionsPage::Vera);
        state
    }

    fn main_layout() -> LauncherOptionsLayout {
        LauncherOptionsLayout::new(800, 600, OptionsPage::Main)
    }

    fn vera_layout() -> LauncherOptionsLayout {
        LauncherOptionsLayout::new(800, 600, OptionsPage::Vera)
    }

    fn checkbox(layout: &LauncherOptionsLayout, id: LauncherCheckboxId) -> RectPx {
        layout
            .checkboxes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.page))
    }

    fn button(layout: &LauncherOptionsLayout, id: LauncherButton) -> RectPx {
        layout
            .buttons
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.page))
    }

    fn heading(state: &OptionsDialogState, layout: &LauncherOptionsLayout, text: &str) -> RectPx {
        state
            .shell_labels(layout)
            .into_iter()
            .find(|label| label.text == text)
            .unwrap_or_else(|| panic!("{text} heading"))
            .rect
    }

    fn disjoint(a: RectPx, b: RectPx) -> bool {
        a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
    }

    #[test]
    fn a_stretched_slider_maps_the_pointer_back_to_its_authored_track() {
        let mut dialog = state();
        let base = main_layout();
        let scale = ShellScale::from_screen(1600, 1200);
        let wide = LauncherOptionsLayout::responsive(1600, 1200, OptionsPage::Main);
        let authored = base.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        let stretched = wide.trackbar_rect(LauncherTrackbarId::Score).unwrap();
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
    fn main_page_keeps_retail_controls_with_zoom_in_the_resolution_place() {
        let state = state();
        let layout = main_layout();
        use LauncherCheckboxId::{ShowHidden, TargetLines, Tooltips};
        let boxes: Vec<_> = layout.checkboxes.iter().map(|(id, _)| *id).collect();
        assert_eq!(boxes, [Tooltips, TargetLines, ShowHidden]);
        assert_eq!(layout.trackbars.len(), 6);
        assert_eq!(
            layout.combos,
            [(
                LauncherComboId::Zoom,
                RectPx::new(351, 86, 180, COMBO_FACE_H)
            )]
        );
        let texts: Vec<_> = state
            .shell_labels(&layout)
            .into_iter()
            .map(|label| label.text.to_owned())
            .collect();
        for vera in [SIDEBAR_LABEL, DEVELOPER_OPTIONS_LABEL, VERA_OPTIONS_LABEL] {
            assert!(!texts.iter().any(|text| text == vera), "{vera}");
        }
        assert_eq!(texts[0], "Options");
        // VERA20K Options takes the slot under Network, the next stacked top.
        let network = button(&layout, LauncherButton::Exit(LauncherParentResult::Network));
        let keyboard = button(
            &layout,
            LauncherButton::Exit(LauncherParentResult::Keyboard),
        );
        let vera = button(&layout, LauncherButton::Page(OptionsPage::Vera));
        let main_menu = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
        assert_eq!((vera.x, vera.w, vera.h), (network.x, network.w, network.h));
        assert_eq!(vera.y - network.y, network.y - keyboard.y);
        assert!(vera.y + vera.h <= main_menu.y);
        assert_eq!(
            state.shell_button_label(LauncherButton::Page(OptionsPage::Vera)),
            VERA_OPTIONS_LABEL
        );
    }

    #[test]
    fn vera_page_holds_every_vera_control_clear_of_the_others() {
        let state = vera_state();
        let layout = vera_layout();
        assert!(layout.trackbars.is_empty());
        // The Sidebar and Display lists sit in the Display Options band's right
        // column, Sidebar above Display, each with its own caption.
        assert_eq!(
            layout.combos,
            [
                (
                    LauncherComboId::Sidebar,
                    RectPx::new(351, 66, 180, COMBO_FACE_H)
                ),
                (
                    LauncherComboId::Display,
                    RectPx::new(351, 110, 180, COMBO_FACE_H)
                ),
            ]
        );
        // Each of the ten boxes is on exactly one page.
        let main = main_layout();
        let all: Vec<_> = main
            .checkboxes
            .iter()
            .chain(&layout.checkboxes)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(all.len(), 10);
        for id in &all {
            assert_eq!(all.iter().filter(|other| *other == id).count(), 1, "{id:?}");
        }
        // No control or caption overlaps another, and all sit above the
        // status line.
        let labels = state.shell_labels(&layout);
        let mut rects: Vec<RectPx> = layout
            .checkboxes
            .iter()
            .map(|(_, rect)| *rect)
            .chain(layout.combos.iter().map(|(_, rect)| *rect))
            .collect();
        rects.extend(
            labels
                .iter()
                .filter(|label| !label.title && !label.text.is_empty())
                .map(|label| label.rect),
        );
        for (index, rect) in rects.iter().enumerate() {
            assert!(rect.y + rect.h <= layout.status_help.y, "{rect:?}");
            for other in &rects[index + 1..] {
                assert!(disjoint(*rect, *other), "{rect:?} overlaps {other:?}");
            }
        }
        // Each band's boxes sit under its heading and above the next.
        let display = heading(&state, &layout, "Display Options");
        let ui = heading(&state, &layout, "UI Options");
        let developer = heading(&state, &layout, DEVELOPER_OPTIONS_LABEL);
        use LauncherCheckboxId::*;
        for (id, top, bottom) in [
            (FullScreen, display, ui),
            (OriginalShape, display, ui),
            (RenderInterpolation, display, ui),
            (YrOriginalStyle, ui, developer),
            (ShiftQueue, ui, developer),
            (WolTestService, developer, layout.status_help),
            (FpsCounter, developer, layout.status_help),
        ] {
            let rect = checkbox(&layout, id);
            assert!(
                rect.y >= top.y + top.h && rect.y + rect.h <= bottom.y,
                "{id:?}"
            );
        }
        assert_eq!(state.shell_title_text(), VERA_OPTIONS_LABEL);
        assert_eq!(labels[0].text, VERA_OPTIONS_LABEL);
        assert_eq!(
            state.shell_button_label(LauncherButton::Page(OptionsPage::Main)),
            "Back"
        );
    }

    #[test]
    fn page_buttons_switch_pages_without_applying_anything() {
        let mut state = state();
        let main = main_layout();
        let before = state.pack();
        let vera = button(&main, LauncherButton::Page(OptionsPage::Vera));
        state.shell_mouse_down(vera.x + 5, vera.y + 5, &main);
        assert!(state.shell_button_pressed(LauncherButton::Page(OptionsPage::Vera)));
        state.shell_mouse_up(vera.x + 5, vera.y + 5, &main);
        let output = state.drain_output();
        assert_eq!(
            output.events,
            [LauncherOptionsEvent::Cue(LauncherCue::MainButton)]
        );
        assert_eq!(
            (output.result, output.page),
            (None, Some(OptionsPage::Vera))
        );
        // The request alone switches nothing: the page slides out first.
        assert_eq!(state.page(), OptionsPage::Main);
        state.show_page(OptionsPage::Vera);
        assert!(!state.shell_button_hovered(LauncherButton::Page(OptionsPage::Vera)));
        let layout = vera_layout();
        let back = button(&layout, LauncherButton::Page(OptionsPage::Main));
        assert_eq!(
            back,
            button(&main, LauncherButton::Exit(LauncherParentResult::Back))
        );
        state.shell_mouse_down(back.x + 5, back.y + 5, &layout);
        state.shell_mouse_up(back.x + 5, back.y + 5, &layout);
        let output = state.drain_output();
        assert_eq!(
            (output.result, output.page),
            (None, Some(OptionsPage::Main))
        );
        state.show_page(OptionsPage::Main);
        assert_eq!(state.shell_title_text(), "Options");
        assert_eq!(state.pack(), before);
    }

    #[test]
    fn showing_a_page_drops_an_open_list() {
        let mut state = state();
        let main = main_layout();
        let zoom = main.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &main);
        assert_eq!(state.open_combo, Some(LauncherComboId::Zoom));
        state.show_page(OptionsPage::Vera);
        assert_eq!(state.open_combo, None);
        assert!(state.shell_popup(&vera_layout()).is_none());
    }

    #[test]
    fn status_help_names_the_control_under_the_pointer() {
        let csf = |key| Some(LauncherStatusHelp::Csf(key));
        let text = |text| Some(LauncherStatusHelp::Text(text));
        let state = state();
        let layout = main_layout();
        let at = |rect: RectPx| state.shell_status_help_key(rect.x + 2, rect.y + 2, &layout);
        assert_eq!(
            at(button(
                &layout,
                LauncherButton::Exit(LauncherParentResult::Keyboard)
            )),
            csf("STT:MainOptButtonKeyboard")
        );
        assert_eq!(
            at(button(
                &layout,
                LauncherButton::Exit(LauncherParentResult::Back)
            )),
            csf("STT:MainOptButtonBack")
        );
        assert_eq!(
            at(button(&layout, LauncherButton::Page(OptionsPage::Vera))),
            text(VERA_OPTIONS_HELP)
        );
        assert_eq!(
            at(layout.trackbar_rect(LauncherTrackbarId::Score).unwrap()),
            csf("STT:MainOptSliderMusic")
        );
        // The display controls retail never had explain themselves in VERA text.
        assert_eq!(at(layout.combos[0].1), text(ZOOM_HELP));
        assert_eq!(state.shell_status_help_key(5, 5, &layout), None);

        let state = vera_state();
        let layout = vera_layout();
        let at = |rect: RectPx| state.shell_status_help_key(rect.x + 2, rect.y + 2, &layout);
        // Back reads like the Keyboard page's: "Return to the Options Menu."
        assert_eq!(
            at(button(&layout, LauncherButton::Page(OptionsPage::Main))),
            csf("STT:KeyboardButtonBack")
        );
        assert_eq!(at(layout.combos[0].1), text(SIDEBAR_HELP));
        use LauncherCheckboxId::*;
        for (id, help) in [
            (FullScreen, FULL_SCREEN_HELP),
            (OriginalShape, ORIGINAL_SHAPE_HELP),
            (RenderInterpolation, RENDER_INTERPOLATION_HELP),
            (YrOriginalStyle, YR_ORIGINAL_STYLE_HELP),
            (ShiftQueue, SHIFT_QUEUE_HELP),
            (WolTestService, WOL_TEST_SERVICE_HELP),
            (FpsCounter, FPS_COUNTER_HELP),
        ] {
            assert_eq!(at(checkbox(&layout, id)), text(help), "{id:?}");
        }
    }

    #[test]
    fn physical_slider_capture_crosses_other_controls_without_activating_them() {
        let mut state = state();
        let layout = main_layout();
        let rect = layout.trackbar_rect(LauncherTrackbarId::Score).unwrap();
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
        let layout = main_layout();
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
        let layout = main_layout();
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
        let back = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
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
        let layout = main_layout();
        state.shell_mouse_down(700, 210, &layout);
        assert!(state.shell_button_pressed(LauncherButton::Exit(LauncherParentResult::Keyboard)));
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
        let layout = main_layout();
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
            Vec::new(),
            DisplayTarget::Auto,
        );
        state.open_combo = Some(LauncherComboId::Zoom);
        let layout = LauncherOptionsLayout::responsive(1920, 1080, OptionsPage::Main);
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
        let mut state = vera_state();
        // A 1920x1080 window at 100%: every size fits.
        let layout = vera_layout().with_battle_frame((1920, 1080));
        let face = layout.combo_rect(LauncherComboId::Sidebar).unwrap();
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
        let zoomed = vera_layout().with_battle_frame((1720, 720));
        assert_eq!(state.shell_selected_sidebar(&zoomed), SidebarSize::Large);
        state.shell_mouse_down(face.x + 10, face.y + 5, &zoomed);
        let popup = state.shell_popup(&zoomed).unwrap();
        assert_eq!((popup.rows, popup.selected), (2, 1));
    }

    #[test]
    fn display_list_picks_the_screen_a_row_names() {
        let mut state = vera_state();
        let layout = vera_layout();
        let face = layout.combo_rect(LauncherComboId::Display).unwrap();
        // Auto is the face's text and the list's first row until a screen is
        // picked, so a fresh profile reads the way it always behaved.
        assert_eq!(state.shell_display_label(), DISPLAY_AUTO_LABEL);
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::Display));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::Display, 3, 0),
            "Auto, then the two screens"
        );
        assert_eq!(
            state.shell_display_rows(),
            [
                DISPLAY_AUTO_LABEL.to_owned(),
                "DISPLAY1: 1920x1080: 60Hz".to_owned(),
                "DISPLAY2: 2560x1440: 144Hz".to_owned(),
            ]
        );
        // The second screen's row: the pick publishes at once, like Sidebar.
        state.drain_output();
        let second = popup.row_rect(2);
        state.shell_mouse_down(second.x + 10, second.y + 5, &layout);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.shell_display_label(), "DISPLAY2: 2560x1440: 144Hz");
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboClose),
                LauncherOptionsEvent::DisplaySelected(DisplayTarget::Fixed(
                    "\\\\.\\DISPLAY2".to_owned()
                )),
            ]
        );
        // Back to Auto: the window returns to the system's own choice.
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(popup.selected, 2, "the picked screen shows selected");
        state.drain_output();
        let auto = popup.row_rect(0);
        state.shell_mouse_down(auto.x + 10, auto.y + 5, &layout);
        assert_eq!(state.shell_display_label(), DISPLAY_AUTO_LABEL);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboClose),
                LauncherOptionsEvent::DisplaySelected(DisplayTarget::Auto),
            ]
        );
    }

    #[test]
    fn display_face_names_a_saved_screen_the_desktop_no_longer_has() {
        // The profile keeps a name this desktop does not: the face still names
        // it, so the player can see what is saved, and its row is not selected.
        let mut state = OptionsDialogState::new(
            LauncherOptionsLabels::resolve(&|_| None),
            LauncherOptionsValues::default(),
            Zoom::Auto,
            true,
            state().displays().to_vec(),
            DisplayTarget::Fixed("\\\\.\\DISPLAY9".to_owned()),
        );
        state.show_page(OptionsPage::Vera);
        assert_eq!(state.shell_display_label(), "\\\\.\\DISPLAY9");
        let layout = vera_layout();
        let face = layout.combo_rect(LauncherComboId::Display).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.rows, popup.selected),
            (3, 0),
            "an absent screen still falls back to Auto's row"
        );
    }

    /// Each VERA box on its page: its caption, and a press that toggles it and
    /// reports the change after the cue.
    #[test]
    fn vera_boxes_toggle_at_once_after_their_cue() {
        use LauncherCheckboxId::*;
        let layout = vera_layout();
        for (id, label, event) in [
            (
                FullScreen,
                FULL_SCREEN_LABEL,
                LauncherOptionsEvent::FullScreenChanged as fn(bool) -> LauncherOptionsEvent,
            ),
            (
                OriginalShape,
                ORIGINAL_SHAPE_LABEL,
                LauncherOptionsEvent::OriginalShapeChanged,
            ),
            (
                RenderInterpolation,
                RENDER_INTERPOLATION_LABEL,
                LauncherOptionsEvent::RenderInterpolationChanged,
            ),
            (
                YrOriginalStyle,
                YR_ORIGINAL_STYLE_LABEL,
                LauncherOptionsEvent::YrOriginalStyleChanged,
            ),
            (
                ShiftQueue,
                SHIFT_QUEUE_LABEL,
                LauncherOptionsEvent::ShiftQueueChanged,
            ),
            (
                WolTestService,
                WOL_TEST_SERVICE_LABEL,
                LauncherOptionsEvent::WolTestServiceChanged,
            ),
            (
                FpsCounter,
                FPS_COUNTER_LABEL,
                LauncherOptionsEvent::FpsCounterChanged,
            ),
        ] {
            let mut state = vera_state();
            let rect = checkbox(&layout, id);
            assert_eq!(state.shell_checkbox_label(id), label);
            // Off until the player ticks it.
            assert!(!state.shell_checkbox_checked(id), "{id:?}");
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert!(state.shell_checkbox_checked(id), "{id:?}");
            assert_eq!(
                state.drain_output().events,
                [
                    LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                    event(true)
                ],
                "{id:?}"
            );
        }
    }

    #[test]
    fn fps_counter_box_follows_live_state() {
        let mut state = vera_state();
        let id = LauncherCheckboxId::FpsCounter;
        state.checkbox_mouse_down(
            id,
            PhysicalControlFrame {
                local_x: 5,
                local_y: 5,
                width: 195,
                height: 16,
            },
        );
        assert!(state.shell_checkbox_checked(id));
        // Ctrl+Alt+F while the page is open moves the box with the readout.
        state.sync_fps_counter(false);
        assert!(!state.shell_checkbox_checked(id));
    }
}
