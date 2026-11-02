//! Physical-pixel presentation and input for launcher resource 0xD5.
//!
//! Retail RT_DIALOG/0xD5/1033 provides the DLU rectangles; common setup
//! 0x00608CD0/0x00609730 anchors the rail, and 0x0060B950 adjusts its title.
//! This module projects the retained OptionsDialogState instead of owning a
//! second set of option values or bypassing its ordered preview events.
//! VERA's page lays its controls out on the main page's bands and rows.

use super::{
    CHECK_FOR_UPDATES_HELP, CHECK_FOR_UPDATES_LABEL,
    CUSTOM_END_GAME_BANNER_HELP, CUSTOM_END_GAME_BANNER_LABEL, CUSTOM_END_GAME_MOVIE_HELP,
    CUSTOM_END_GAME_MOVIE_LABEL, CUSTOM_END_GAME_SCORE_HELP, CUSTOM_END_GAME_SCORE_LABEL,
    DISPLAY_AUTO_LABEL, DISPLAY_HELP,
    DisplayTarget, FPS_COUNTER_HELP, FPS_COUNTER_LABEL, FULL_SCREEN_HELP, FULL_SCREEN_LABEL,
    HAS_CHECK_FOR_UPDATES, HOUSE_COLOR_STRENGTH_HELP, LOCK_CURSOR_HELP,
    LOCK_CURSOR_LABEL, LauncherButton, LauncherCheckboxId, LauncherComboId, LauncherCue,
    LauncherOptionsEvent, LauncherParentResult, LauncherTrackbarId, MOUSE_WHEEL_ZOOM_HELP,
    MOUSE_WHEEL_ZOOM_LABEL, NETWORK_PAGE_NOTE, NETWORK_SETUP_HELP, NETWORK_SETUP_LABEL,
    ORIGINAL_SHAPE_HELP, ORIGINAL_SHAPE_LABEL, OptionsDialogState,
    OptionsPage, PhysicalControlFrame, RENDER_INTERPOLATION_HELP, RENDER_INTERPOLATION_LABEL,
    RIGHT_CLICK_ORDERS_HELP, RIGHT_CLICK_ORDERS_LABEL, SHIFT_QUEUE_HELP, SHIFT_QUEUE_LABEL,
    SIDEBAR_HELP, SKIP_STARTUP_LOGO_HELP, SKIP_STARTUP_LOGO_LABEL, SidebarSize,
    VERA_ICON_HELP, VERA_ICON_LABEL, WOL_TEST_SERVICE_HELP,
    WOL_TEST_SERVICE_LABEL, YR_ORIGINAL_STYLE_HELP, YR_ORIGINAL_STYLE_LABEL, ZOOM_HELP,
    Zoom, OPTIONS_TABS, Category, Setting,
};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, ShellScale, center_offset, child_window, dlu_rect,
};
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
/// VERA adds its Vera Options button in the next slot, the third stacked top
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
        // Inert: the family page keeps the slot so the retail column's cell
        // positions stay put, but the tab model never builds this button.
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

/// VERA's Check for Updates on its page, in the column's first slot. Not a
/// retail control: the id only names its rect in the page layout.
pub(crate) const CHECK_FOR_UPDATES_BUTTON: u16 = 0x7FFE;

/// VERA's page of `0xD5`: Check for Updates at the top of the column (not in
/// the browser build), and Back in Main Menu's place, as on the Keyboard page
/// `0xA3`, with the Keyboard page's Back help.
pub(crate) const VERA_OPTIONS_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0x00D5),
    // Only the slide's button draws read this page spec.
    title_key: "",
    // Its caption and help are CHECK_FOR_UPDATES_LABEL and _HELP.
    stacked: if HAS_CHECK_FOR_UPDATES {
        &[MenuPageButtonSpec {
            id: CHECK_FOR_UPDATES_BUTTON,
            dlu_top: 122,
            csf_key: "",
            tooltip_key: "",
            result: None,
        }]
    } else {
        &[]
    },
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
    /// Setting-name captions for combo and slider rows on the selected tab.
    pub(crate) setting_labels: Vec<(RectPx, &'static str)>,
    /// The step name beside a slider that carries no value plaque (Detail,
    /// Difficulty, Scroll), as retail prints it to the right of the track.
    pub(crate) value_labels: Vec<(RectPx, LauncherTrackbarId)>,
    /// The selected tab's heading and the rule under it, inside the form card.
    pub(crate) section_header: Option<(RectPx, &'static str)>,
    pub(crate) section_rule: Option<RectPx>,
    /// The framed board on the dialog's left side: the tab column down its
    /// left edge and the form card beside it. It keeps the full height between
    /// the dialog's top edge and the status line, so the sheet fills the left
    /// side whatever the selected tab holds.
    pub(crate) content_panel: RectPx,
    /// The selected tab's form card, inset in the board beside the tabs.
    pub(crate) content_body: RectPx,
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
    /// [`ShellScale::RETAIL`], showing `tab`.
    pub(crate) fn new(width: i32, height: i32, tab: Category) -> Self {
        Self::build(width, height, ShellScale::RETAIL, tab)
    }

    /// The same dialog scaled to fit the window.
    pub(crate) fn responsive(width: i32, height: i32, tab: Category) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height), tab)
    }

    fn build(width: i32, height: i32, scale: ShellScale, tab: Category) -> Self {
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
        // The right-panel frame (title, status line, Back button) follows the
        // panel, not the dialog centring. Every level reuses the family panel;
        // the old Main page's right column held Keyboard/Network/Vera, now the
        // root shows the category list there instead.
        let spec = &OPTIONS_PAGE;
        let panel = if stretch {
            menu_page::compute_responsive_layout(spec, width as u32, height as u32)
        } else {
            menu_page::compute_layout(spec, width as u32, height as u32)
        };
        let back_rect = panel.button_rect(MAIN_MENU_BUTTON).expect("0xD5 back button");
        // The dialog's whole left half is one framed board, at the place the
        // retail dialog laid its four control bands: a column of page tabs
        // down its left edge and the selected tab's form beside it. The board
        // keeps the full height between the dialog's top edge and the status
        // line, so the sheet fills the left side instead of shrinking to the
        // rows it happens to hold.
        let panel_x = 6;
        let panel_y = 8;
        let panel_w = 410;
        let panel_h = 340;
        // Seven tabs at 42 DLU with a 4-DLU gap stack to 336, inside the
        // board's 340.
        let tab_x = panel_x + 8;
        let tab_w = 96;
        let tab_top = panel_y + 10;
        let tab_h = 42;
        let tab_pitch = 46;
        // The form card: the rest of the board, inset by the same margin.
        let body_x = tab_x + tab_w + 8;
        let body_y = panel_y + 10;
        let body_w = panel_x + panel_w - 10 - body_x;
        let body_h = panel_h - 20;
        // One form grid for every tab: captions start at the card's left
        // margin, controls one caption-width further right, so a checkbox
        // icon, a caption and a list label share a left edge and the control
        // column stays put as tabs change.
        let content_x = body_x + 10;
        let control_x = content_x + 104;
        // The card's heading and the rule under it, then the row grid.
        let header_rect = dlu(content_x, body_y + 8, body_w - 20, 12);
        let rule_rect = dlu(content_x, body_y + 23, body_w - 20, 1);
        let first_top = body_y + 30;
        let row = |top: i32, h: i32| dlu(content_x, top, body_w - 20, h);
        let caption = |top: i32| dlu(content_x, top + 2, 104, 10);
        // A control placed on the row grid but sized in authored pixels: the
        // chrome atlas keys combo faces on their pixel width and height, not
        // on DLUs, so a combo must be authored at the atlas's own size.
        let px_at = |x_dlu: i32, y_dlu: i32, w: i32, h: i32| {
            let origin = dlu_rect(x_dlu, y_dlu, 0, 0);
            px(RectPx::new(origin.x, origin.y, w, h))
        };

        let mut buttons: Vec<(LauncherButton, RectPx)> = OPTIONS_TABS
            .iter()
            .enumerate()
            .map(|(index, category)| {
                (
                    LauncherButton::Tab(*category),
                    dlu(tab_x, tab_top + index as i32 * tab_pitch, tab_w, tab_h),
                )
            })
            .collect();
        buttons.push((LauncherButton::Exit(LauncherParentResult::Back), back_rect));

        let mut combos = Vec::new();
        let mut trackbars = Vec::new();
        let mut checkboxes = Vec::new();
        let mut setting_labels = Vec::new();
        let mut value_labels = Vec::new();
        let mut top = first_top;
        for setting in tab.spec().settings {
            match setting {
                Setting::Checkbox(s) => checkboxes.push((s.underlying, row(top, 16))),
                Setting::Select(s) => {
                    setting_labels.push((caption(top), s.label));
                    // 180 pixels is the retail resolution list's face, one of
                    // the widths the chrome atlas carries; any other width
                    // would paint the arrow over no face.
                    combos.push((s.underlying, px_at(control_x, top, 180, COMBO_FACE_H)));
                }
                Setting::Slider(s) => {
                    // The value plaque widens the window by its reserve; the
                    // plaque-less sliders (Detail, Difficulty, Scroll) keep the
                    // 120-DLU window retail authored for them, so every row
                    // still resolves to one of the atlas's nine frames.
                    let width = if s.underlying.plaque_reserve() > 0 {
                        150
                    } else {
                        120
                    };
                    setting_labels.push((caption(top), s.label));
                    trackbars.push((s.underlying, slider(control_x, top, width, 13)));
                    if s.underlying.plaque_reserve() == 0 {
                        // Retail prints the step beside these tracks; the
                        // captions are per-position tokens, so the paint side
                        // resolves them from the live value.
                        value_labels.push((
                            dlu(control_x + 130, top + 2, 38, 10),
                            s.underlying,
                        ));
                    }
                }
                Setting::Radio(_) | Setting::Text(_) | Setting::Color(_) => {}
            }
            top += 24;
        }
        let mut bottom = (top - 24 + 16).max(first_top + 16);
        // The Network category's controls live in the retail D7 box; a one-line
        // note says what that box holds, so the tab is never a lone button.
        if tab == Category::Network {
            setting_labels.push((dlu(content_x, first_top + 2, body_w - 20, 10), NETWORK_PAGE_NOTE));
            bottom = first_top + 16;
        }
        // The categories whose real controls are retail dialogs of their own
        // keep the way in, under the rows; the desktop build's Developer tab
        // keeps Check for Updates.
        let mut actions: Vec<LauncherButton> = Vec::new();
        match tab {
            Category::Controls => actions.push(LauncherButton::Exit(LauncherParentResult::Keyboard)),
            Category::Network => actions.push(LauncherButton::Exit(LauncherParentResult::Network)),
            Category::Developer if HAS_CHECK_FOR_UPDATES => {
                actions.push(LauncherButton::CheckForUpdates);
            }
            _ => {}
        }
        let mut action_top = bottom + 12;
        for action in actions {
            buttons.push((action, dlu(content_x, action_top, 150, 20)));
            action_top += 26;
        }

        Self {
            page: OptionsPage::Main,
            title: panel.title,
            warning: panel.warning_monitor,
            status_help: panel.status_help,
            combos,
            trackbars,
            checkboxes,
            buttons,
            setting_labels,
            value_labels,
            section_header: Some((header_rect, tab.label())),
            section_rule: Some(rule_rect),
            content_panel: dlu(panel_x, panel_y, panel_w, panel_h),
            content_body: dlu(body_x, body_y, body_w, body_h),
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
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LauncherShellLabel<'a> {
    pub(crate) rect: RectPx,
    pub(crate) text: &'a str,
    pub(crate) align: LauncherLabelAlign,
    pub(crate) title: bool,
    /// A section heading inside the form card: drawn brighter than a row
    /// caption so the tab's name reads as the sheet's title.
    pub(crate) heading: bool,
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
        use LauncherLabelAlign::{Center, Left};
        let mut result = vec![LauncherShellLabel {
            rect: layout.title,
            text: self.shell_title_text(),
            align: Center,
            title: true,
            heading: false,
        }];
        // The selected tab's heading, then one caption per labelled row.
        if let Some((rect, text)) = layout.section_header {
            result.push(LauncherShellLabel {
                rect,
                text,
                align: Left,
                title: false,
                heading: true,
            });
        }
        for &(rect, text) in &layout.setting_labels {
            result.push(LauncherShellLabel {
                rect,
                text: self.setting_caption(text),
                align: Left,
                title: false,
                heading: false,
            });
        }
        result
    }

    /// A settings row's caption: the string this dialog already resolved for
    /// a retail `GUI:` key, or the declared text itself when VERA authored it.
    fn setting_caption(&self, label: &'static str) -> &str {
        match label {
            "GUI:VisualDetails" => self.labels.visual_details.as_str(),
            "GUI:Difficulty" => self.labels.difficulty.as_str(),
            "GUI:ScrollRate" => self.labels.scroll_rate.as_str(),
            "GUI:MusicVolume" => self.labels.music_volume.as_str(),
            "GUI:SoundVolume" => self.labels.sound_volume.as_str(),
            "GUI:VoiceVolume" => self.labels.voice_volume.as_str(),
            "GUI:Tooltips" => self.labels.tooltips.as_str(),
            "GUI:TargetLines" => self.labels.target_lines.as_str(),
            "GUI:ShowHidden" => self.labels.show_hidden.as_str(),
            other => other,
        }
    }

    pub(crate) fn shell_checkbox_label(&self, id: LauncherCheckboxId) -> &str {        match id {
            LauncherCheckboxId::Tooltips => &self.labels.tooltips,
            LauncherCheckboxId::TargetLines => &self.labels.target_lines,
            LauncherCheckboxId::ShowHidden => &self.labels.show_hidden,
            LauncherCheckboxId::FullScreen => FULL_SCREEN_LABEL,
            LauncherCheckboxId::OriginalShape => ORIGINAL_SHAPE_LABEL,
            LauncherCheckboxId::YrOriginalStyle => YR_ORIGINAL_STYLE_LABEL,
            LauncherCheckboxId::RenderInterpolation => RENDER_INTERPOLATION_LABEL,
            LauncherCheckboxId::MouseWheelZoom => MOUSE_WHEEL_ZOOM_LABEL,
            LauncherCheckboxId::RightClickOrders => RIGHT_CLICK_ORDERS_LABEL,
            LauncherCheckboxId::LockCursor => LOCK_CURSOR_LABEL,
            LauncherCheckboxId::VeraIcon => VERA_ICON_LABEL,
            LauncherCheckboxId::ShiftQueue => SHIFT_QUEUE_LABEL,
            LauncherCheckboxId::WolTestService => WOL_TEST_SERVICE_LABEL,
            LauncherCheckboxId::FpsCounter => FPS_COUNTER_LABEL,
            LauncherCheckboxId::SkipStartupLogo => SKIP_STARTUP_LOGO_LABEL,
            LauncherCheckboxId::CustomEndGameBanner => CUSTOM_END_GAME_BANNER_LABEL,
            LauncherCheckboxId::CustomEndGameScore => CUSTOM_END_GAME_SCORE_LABEL,
            LauncherCheckboxId::CustomEndGameMovie => CUSTOM_END_GAME_MOVIE_LABEL,
        }
    }

    /// The step name a plaque-less trackbar prints beside its track, in the
    /// order the dialog already resolved for it.
    pub(crate) fn shell_trackbar_caption(&self, id: LauncherTrackbarId) -> Option<&str> {
        match id {
            LauncherTrackbarId::Detail => Some(self.detail_caption.as_str()),
            LauncherTrackbarId::Difficulty => Some(self.difficulty_caption.as_str()),
            LauncherTrackbarId::Scroll => Some(self.scroll_caption.as_str()),
            _ => None,
        }
    }

    pub(crate) fn shell_button_label(&self, id: LauncherButton) -> &str {
        match id {
            LauncherButton::Exit(LauncherParentResult::Keyboard) => &self.labels.keyboard,
            LauncherButton::Exit(LauncherParentResult::Network) => NETWORK_SETUP_LABEL,
            LauncherButton::Exit(LauncherParentResult::Back) => &self.labels.main_menu,
            LauncherButton::Exit(LauncherParentResult::Terminal) => "",
            LauncherButton::Page(_) => &self.labels.back,
            LauncherButton::CheckForUpdates => CHECK_FOR_UPDATES_LABEL,
            LauncherButton::Tab(category) => category.label(),
        }
    }

    /// The dialog's heading: the family page's own title, as the tab column
    /// already names the section.
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
            LauncherCheckboxId::RenderInterpolation => self.values.render_interpolation,
            LauncherCheckboxId::MouseWheelZoom => self.values.mouse_wheel_zoom,
            LauncherCheckboxId::RightClickOrders => self.values.right_click_orders,
            LauncherCheckboxId::LockCursor => self.values.lock_cursor,
            LauncherCheckboxId::VeraIcon => self.values.vera_icon,
            LauncherCheckboxId::ShiftQueue => self.values.shift_queue,
            LauncherCheckboxId::WolTestService => self.values.wol_test_service,
            LauncherCheckboxId::FpsCounter => self.values.fps_counter,
            LauncherCheckboxId::SkipStartupLogo => self.values.skip_startup_logo,
            LauncherCheckboxId::CustomEndGameBanner => self.values.custom_end_game_banner,
            LauncherCheckboxId::CustomEndGameScore => self.values.custom_end_game_score,
            LauncherCheckboxId::CustomEndGameMovie => self.values.custom_end_game_movie,
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

    /// The Display face's text: the picked screen's row text, or just the id
    /// when the window layer no longer lists it.
    pub(crate) fn shell_display_label(&self) -> String {
        match &self.display {
            DisplayTarget::Auto => DISPLAY_AUTO_LABEL.to_owned(),
            DisplayTarget::Fixed(id) => self
                .displays
                .iter()
                .position(|choice| &choice.id == id)
                .map_or_else(|| id.clone(), |index| self.displays[index].label(index + 1)),
        }
    }

    /// The Display popup's rows, top to bottom: [`DISPLAY_AUTO_LABEL`], then
    /// each screen's [`DisplayChoice::label`], numbered from 1.
    pub(crate) fn shell_display_rows(&self) -> Vec<String> {
        std::iter::once(DISPLAY_AUTO_LABEL.to_owned())
            .chain(
                self.displays
                    .iter()
                    .enumerate()
                    .map(|(index, choice)| choice.label(index + 1)),
            )
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
                LauncherButton::CheckForUpdates => {
                    return Some(LauncherStatusHelp::Text(CHECK_FOR_UPDATES_HELP));
                }
                LauncherButton::Tab(category) => {
                    return Some(LauncherStatusHelp::Text(category.help()));
                }
                LauncherButton::Page(_) => (&VERA_OPTIONS_PAGE, MAIN_MENU_BUTTON),
                LauncherButton::Exit(LauncherParentResult::Keyboard) => {
                    (&OPTIONS_PAGE, KEYBOARD_BUTTON)
                }
                LauncherButton::Exit(LauncherParentResult::Network) => {
                    return Some(LauncherStatusHelp::Text(NETWORK_SETUP_HELP));
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
            return Some(match id {
                LauncherTrackbarId::Detail => LauncherStatusHelp::Csf("STT:MainOptSliderVisual"),
                LauncherTrackbarId::Difficulty => {
                    LauncherStatusHelp::Csf("STT:MainOptSliderDifficulty")
                }
                LauncherTrackbarId::Scroll => LauncherStatusHelp::Csf("STT:MainOptSliderScroll"),
                LauncherTrackbarId::Score => LauncherStatusHelp::Csf("STT:MainOptSliderMusic"),
                LauncherTrackbarId::Sound => LauncherStatusHelp::Csf("STT:MainOptSliderSound"),
                LauncherTrackbarId::Voice => LauncherStatusHelp::Csf("STT:MainOptSliderVoice"),
                // VERA's own slider: retail has no string for it.
                LauncherTrackbarId::HouseColorStrength => {
                    LauncherStatusHelp::Text(HOUSE_COLOR_STRENGTH_HELP)
                }
            });
        }
        if let Some((id, _)) = layout
            .combos
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
        {
            return Some(match id {
                LauncherComboId::Zoom => LauncherStatusHelp::Text(ZOOM_HELP),
                LauncherComboId::Sidebar => LauncherStatusHelp::Text(SIDEBAR_HELP),
                LauncherComboId::Display => LauncherStatusHelp::Text(DISPLAY_HELP),
            });
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
                LauncherCheckboxId::MouseWheelZoom => {
                    LauncherStatusHelp::Text(MOUSE_WHEEL_ZOOM_HELP)
                }
                LauncherCheckboxId::RightClickOrders => {
                    LauncherStatusHelp::Text(RIGHT_CLICK_ORDERS_HELP)
                }
                LauncherCheckboxId::LockCursor => LauncherStatusHelp::Text(LOCK_CURSOR_HELP),
                LauncherCheckboxId::VeraIcon => LauncherStatusHelp::Text(VERA_ICON_HELP),
                LauncherCheckboxId::ShiftQueue => LauncherStatusHelp::Text(SHIFT_QUEUE_HELP),
                LauncherCheckboxId::WolTestService => {
                    LauncherStatusHelp::Text(WOL_TEST_SERVICE_HELP)
                }
                LauncherCheckboxId::FpsCounter => LauncherStatusHelp::Text(FPS_COUNTER_HELP),
                LauncherCheckboxId::SkipStartupLogo => {
                    LauncherStatusHelp::Text(SKIP_STARTUP_LOGO_HELP)
                }
                LauncherCheckboxId::CustomEndGameBanner => {
                    LauncherStatusHelp::Text(CUSTOM_END_GAME_BANNER_HELP)
                }
                LauncherCheckboxId::CustomEndGameScore => {
                    LauncherStatusHelp::Text(CUSTOM_END_GAME_SCORE_HELP)
                }
                LauncherCheckboxId::CustomEndGameMovie => {
                    LauncherStatusHelp::Text(CUSTOM_END_GAME_MOVIE_HELP)
                }
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
                LauncherButton::Exit(LauncherParentResult::Back) => self.back_pressed(),
                LauncherButton::Exit(result) => self.request_result(result),
                LauncherButton::Page(page) => self.request_page(page),
                LauncherButton::CheckForUpdates => self.request_update_check(),
                LauncherButton::Tab(category) => self.select_tab(category),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::house_colors::{STRENGTH_MAX, STRENGTH_RETAIL};
    use crate::ui::main_menu_dialogs::options::{
        DisplayChoice, HOUSE_COLOR_STRENGTH_LABEL, LauncherCue, LauncherOptionsEvent,
        LauncherOptionsLabels, LauncherOptionsValues, ZOOM_LABEL,
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

    /// The category list (root): one row per category, plus Back.
    fn categories_layout() -> LauncherOptionsLayout {
        LauncherOptionsLayout::new(800, 600, Category::Display)
    }

    /// A category's settings subpage in retail 800x600.
    fn settings_layout(category: Category) -> LauncherOptionsLayout {
        LauncherOptionsLayout::new(800, 600, category)
    }

    /// A category's settings subpage stretched to a larger window.
    fn settings_layout_wide(category: Category) -> LauncherOptionsLayout {
        LauncherOptionsLayout::responsive(1600, 1200, category)
    }

    /// Every slider either page lays out must land on one of the chrome
    /// atlas's nine authored trackbar windows. The frame lookup is an exact
    /// size match, so a row sized like a plain track but painted with a value
    /// plaque resolves to no frame at all: the page would draw the thumb and
    /// the plaque over empty background.
    #[test]
    fn every_page_slider_lands_on_an_authored_trackbar_frame() {
        use crate::render::skirmish_shell_chrome::trackbar_frame_slot;
        // Every settings subpage lays out its trackbars from the same authored
        // table, so each must resolve to one of the chrome atlas's authored
        // trackbar windows. Categories without trackbars simply contribute no
        // rows.
        for category in [
            Category::Display,
            Category::Audio,
            Category::Gameplay,
            Category::Interface,
            Category::Controls,
            Category::Network,
            Category::Developer,
        ] {
            let layout = settings_layout(category);
            for &(id, rect) in &layout.trackbars {
                let base = layout.scale.base_rect(rect);
                assert!(
                    trackbar_frame_slot(id.plaque_reserve() > 0, base.w, base.h).is_some(),
                    "{id:?} on the {:?} subpage is {}x{}, which no authored trackbar frame covers",
                    category,
                    base.w,
                    base.h
                );
            }
        }
    }

    /// Every control a level lays out that belongs in the content panel: the
    /// settings rows and the page's action buttons. Back is the right rail's.
    fn content_rects(layout: &LauncherOptionsLayout) -> Vec<RectPx> {
        let mut rects: Vec<RectPx> = Vec::new();
        rects.extend(layout.checkboxes.iter().map(|(_, rect)| *rect));
        rects.extend(layout.combos.iter().map(|(_, rect)| *rect));
        rects.extend(layout.trackbars.iter().map(|(_, rect)| *rect));
        rects.extend(
            layout
                .buttons
                .iter()
                .filter(|(id, _)| *id != LauncherButton::Exit(LauncherParentResult::Back))
                .map(|(_, rect)| *rect),
        );
        rects
    }

    const EVERY_CATEGORY: [Category; 7] = [
        Category::Display,
        Category::Audio,
        Category::Gameplay,
        Category::Interface,
        Category::Controls,
        Category::Network,
        Category::Developer,
    ];

    /// Every page of the dialog: one per tab, since the tab column is always
    /// on screen and the form beside it is what changes.
    fn every_level(
        build: impl Fn(Category) -> LauncherOptionsLayout,
    ) -> Vec<(&'static str, LauncherOptionsLayout)> {
        EVERY_CATEGORY
            .into_iter()
            .map(|category| (category.label(), build(category)))
            .collect()
    }

    #[test]
    fn every_control_sits_inside_the_content_panel() {
        for (name, layout) in every_level(|navigation| LauncherOptionsLayout::new(800, 600, navigation)) {
            let panel = layout.content_panel;
            for rect in content_rects(&layout) {
                assert!(
                    rect.x >= panel.x
                        && rect.y >= panel.y
                        && rect.x + rect.w <= panel.x + panel.w
                        && rect.y + rect.h <= panel.y + panel.h,
                    "{name}: a control at {rect:?} leaves the content panel {panel:?}"
                );
            }
        }
    }

    #[test]
    fn a_levels_controls_never_overlap() {
        for (name, layout) in every_level(|navigation| LauncherOptionsLayout::new(800, 600, navigation)) {
            let rects = content_rects(&layout);
            for (index, a) in rects.iter().enumerate() {
                for b in &rects[index + 1..] {
                    assert!(
                        disjoint(*a, *b),
                        "{name}: two controls overlap, {a:?} and {b:?}"
                    );
                }
            }
        }
    }

    /// A scaled page keeps the same structure: the panel grows with the shell
    /// and every control stays inside it, clear of its neighbours.
    #[test]
    fn a_scaled_level_keeps_its_controls_in_the_panel() {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for (name, layout) in every_level(|navigation| {
                LauncherOptionsLayout::responsive(width, height, navigation)
            }) {
                let panel = layout.content_panel;
                assert!(
                    panel.x >= 0
                        && panel.y >= 0
                        && panel.x + panel.w <= width
                        && panel.y + panel.h <= height,
                    "{name} at {width}x{height}: the content panel {panel:?} leaves the window"
                );
                let rects = content_rects(&layout);
                for rect in &rects {
                    assert!(
                        rect.x >= panel.x
                            && rect.y >= panel.y
                            && rect.x + rect.w <= panel.x + panel.w
                            && rect.y + rect.h <= panel.y + panel.h,
                        "{name} at {width}x{height}: a control at {rect:?} leaves the panel {panel:?}"
                    );
                }
                for (index, a) in rects.iter().enumerate() {
                    for b in &rects[index + 1..] {
                        assert!(
                            disjoint(*a, *b),
                            "{name} at {width}x{height}: two controls overlap, {a:?} and {b:?}"
                        );
                    }
                }
            }
        }
    }

    /// The chrome atlas keys combo faces on exact authored widths, so a combo
    /// laid out at any other width paints its arrow over no face at all.
    #[test]
    fn every_combo_face_uses_an_authored_width() {
        for category in EVERY_CATEGORY {
            let layout = settings_layout(category);
            for &(id, rect) in &layout.combos {
                assert_eq!(
                    layout.scale.base_rect(rect).w,
                    180,
                    "{id:?} on the {:?} subpage must use the retail 180-pixel face",
                    category
                );
            }
        }
    }

    fn checkbox(layout: &LauncherOptionsLayout, id: LauncherCheckboxId) -> RectPx {        layout
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

    fn disjoint(a: RectPx, b: RectPx) -> bool {
        a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
    }

    #[test]
    fn a_stretched_slider_maps_the_pointer_back_to_its_authored_track() {
        let mut dialog = state();
        let base = settings_layout(Category::Audio);
        let scale = ShellScale::from_screen(1600, 1200);
        let wide = settings_layout_wide(Category::Audio);
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

    /// The root category list shows one row per category plus Back, with no
    /// overlap, and names each category in the status line on hover.
    #[test]
    fn tab_column_shows_every_page_and_back_without_overlap() {
        let state = state();
        let layout = categories_layout();
        // One tab per category; Back stays on the family rail.
        let mut categories: Vec<_> = layout
            .buttons
            .iter()
            .filter_map(|(id, _)| match id {
                LauncherButton::Tab(c) => Some(*c),
                _ => None,
            })
            .collect();
        categories.sort_by_key(|c| *c as i32);
        assert_eq!(categories, EVERY_CATEGORY);
        // No two buttons overlap.
        for (i, (_, a)) in layout.buttons.iter().enumerate() {
            for (_, b) in layout.buttons.iter().skip(i + 1) {
                assert!(disjoint(*a, *b), "{a:?} overlaps {b:?}");
            }
        }
        // The dialog keeps the family heading; the tab column names the sheet
        // and the selected tab's heading sits over its own form.
        assert_eq!(state.shell_title_text(), "Options");
        let labels = state.shell_labels(&layout);
        assert_eq!(labels[0].text, "Options");
        assert_eq!(labels[1].text, Category::Display.label());
        let body = layout.content_body;
        for category in categories {
            let rect = button(&layout, LauncherButton::Tab(category));
            // The tab column is its own column: no tab runs into the form.
            assert!(rect.x + rect.w <= body.x, "{category:?}");
            assert!(rect.y >= layout.content_panel.y, "{category:?}");
            assert!(
                rect.y + rect.h <= layout.content_panel.y + layout.content_panel.h,
                "{category:?}"
            );
            // Hovering a tab explains it in the status line.
            assert_eq!(
                state.shell_status_help_key(rect.x + 2, rect.y + 2, &layout),
                Some(LauncherStatusHelp::Text(category.help()))
            );
        }
    }

    /// The selected tab's controls do not overlap one another and all sit
    /// above the status line; the form's heading names the tab.
    #[test]
    fn selected_tab_rows_stay_clear_of_one_another() {
        let state = state();
        let layout = settings_layout(Category::Display);
        // The control hit-rects (boxes, lists, sliders) never overlap and all
        // sit above the status line.
        let rects: Vec<RectPx> = layout
            .checkboxes
            .iter()
            .map(|(_, rect)| *rect)
            .chain(layout.trackbars.iter().map(|(_, rect)| *rect))
            .chain(layout.combos.iter().map(|(_, rect)| *rect))
            .collect();
        for (index, rect) in rects.iter().enumerate() {
            assert!(rect.y + rect.h <= layout.status_help.y, "{rect:?}");
            for other in &rects[index + 1..] {
                assert!(disjoint(*rect, *other), "{rect:?} overlaps {other:?}");
            }
        }
        // The dialog keeps its heading; the section heading names the tab.
        assert_eq!(state.shell_title_text(), "Options");
        let labels = state.shell_labels(&layout);
        assert_eq!(labels[0].text, "Options");
        assert_eq!(labels[1].text, "Display");
        let captions: Vec<&str> = labels
            .iter()
            .filter(|label| !label.title)
            .map(|label| label.text)
            .collect();
        assert!(captions.contains(&ZOOM_LABEL));
        assert!(captions.contains(&HOUSE_COLOR_STRENGTH_LABEL));
        // Hovering a control names it in the status line.
        let slider = layout
            .trackbar_rect(LauncherTrackbarId::HouseColorStrength)
            .unwrap();
        assert_eq!(
            state.shell_status_help_key(slider.x + 2, slider.y + 2, &layout),
            Some(LauncherStatusHelp::Text(HOUSE_COLOR_STRENGTH_HELP))
        );
    }

    /// Pressing a tab swaps the form beside it in place, applying nothing;
    /// Back closes the dialog from any tab.
    #[test]
    fn pressing_a_tab_swaps_the_form_and_back_closes_without_applying() {
        let before = state().pack();
        let mut state = state();
        // The dialog opens on the first tab, its form already beside it.
        let display_page = settings_layout(Category::Display);
        assert_eq!(state.tab(), Category::Display);
        assert_eq!(state.shell_labels(&display_page)[1].text, "Display");
        // Press the Audio tab: the tab lights, the form swaps.
        let audio = button(&display_page, LauncherButton::Tab(Category::Audio));
        state.shell_mouse_down(audio.x + 5, audio.y + 5, &display_page);
        assert!(state.shell_button_pressed(LauncherButton::Tab(Category::Audio)));
        state.shell_mouse_up(audio.x + 5, audio.y + 5, &display_page);
        assert_eq!(state.tab(), Category::Audio);
        assert_eq!(state.drain_output().result, None);
        let audio_page = settings_layout(Category::Audio);
        assert_eq!(state.shell_labels(&audio_page)[1].text, "Audio");
        // The dialog keeps its own heading: the tab column names the section.
        assert_eq!(state.shell_title_text(), "Options");
        // Back closes the dialog and applies nothing.
        let back = button(&audio_page, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_down(back.x + 5, back.y + 5, &audio_page);
        state.shell_mouse_up(back.x + 5, back.y + 5, &audio_page);
        assert_eq!(state.drain_output().result, Some(LauncherParentResult::Back));
        assert_eq!(state.pack(), before);
    }

    /// Switching tabs closes any open dropdown: the next form starts clean.
    #[test]
    fn switching_tabs_drops_an_open_list() {
        let mut state = state();
        let layout = settings_layout(Category::Display);
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::Zoom));
        state.select_tab(Category::Audio);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.tab(), Category::Audio);
        let audio_page = settings_layout(Category::Audio);
        assert!(state.shell_popup(&audio_page).is_none());
    }

    #[test]
    fn status_help_names_the_control_under_the_pointer() {
        let csf = |key| Some(LauncherStatusHelp::Csf(key));
        let text = |text| Some(LauncherStatusHelp::Text(text));
        // The root category list: each row explains its category, Back reads as
        // the Main Menu button.
        let dialog = state();
        let layout = categories_layout();
        let at = |rect: RectPx| dialog.shell_status_help_key(rect.x + 2, rect.y + 2, &layout);
        assert_eq!(
            at(button(&layout, LauncherButton::Exit(LauncherParentResult::Back))),
            csf("STT:MainOptButtonBack")
        );
        assert_eq!(
            at(button(&layout, LauncherButton::Tab(Category::Display))),
            text(Category::Display.help())
        );
        assert_eq!(dialog.shell_status_help_key(5, 5, &layout), None);

        // A settings subpage names its own controls.
        let dialog = state();
        let layout = settings_layout(Category::Display);
        let at = |rect: RectPx| dialog.shell_status_help_key(rect.x + 2, rect.y + 2, &layout);
        assert_eq!(at(layout.combos[0].1), text(ZOOM_HELP));
        use LauncherCheckboxId::*;
        for (id, help) in [
            (FullScreen, FULL_SCREEN_HELP),
            (OriginalShape, ORIGINAL_SHAPE_HELP),
            (RenderInterpolation, RENDER_INTERPOLATION_HELP),
            (YrOriginalStyle, YR_ORIGINAL_STYLE_HELP),
        ] {
            assert_eq!(at(checkbox(&layout, id)), text(help), "{id:?}");
        }
    }

    #[test]
    fn physical_slider_capture_crosses_other_controls_without_activating_them() {
        let mut state = state();
        let layout = settings_layout(Category::Audio);
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
        let layout = settings_layout(Category::Audio);
        // Press inside the Score slider's track to begin a drag.
        let rect = layout.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        let thumb = state.shell_trackbar_thumb_left(LauncherTrackbarId::Score, rect, layout.scale);
        state.shell_mouse_down(rect.x + thumb + 5, rect.y + rect.h / 2, &layout);
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
        // The Difficulty slider lives on the Gameplay subpage.
        let gameplay = settings_layout(Category::Gameplay);
        let srect = gameplay.trackbar_rect(LauncherTrackbarId::Difficulty).unwrap();
        let held = Some(TrackbarHold {
            id: LauncherTrackbarId::Difficulty,
            dragging: false,
        });
        // Above the admitted strip (the window's bottom 18 rows) the press
        // only takes the capture.
        state.shell_mouse_down(srect.x + 2, srect.y + 2, &gameplay);
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Difficulty),
            1,
            "native lower strip rejects y=2"
        );
        assert_eq!(state.capture, held);
        state.shell_mouse_up(srect.x + 2, srect.y + 2, &gameplay);
        // Beside the thumb it jumps once and holds without dragging.
        state.shell_mouse_down(srect.x + 2, srect.y + 10, &gameplay);
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Difficulty), 0);
        assert_eq!(state.capture, held);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::GenericClick)]
        );
        state.shell_mouse_move(srect.x + 2, srect.y + 10, &gameplay);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptSliderDifficulty"))
        );
        state.shell_mouse_up(srect.x + srect.w - 2, srect.y + srect.h - 3, &gameplay);
        assert_eq!(state.capture, None);
        // Back's status help reads after hovering it.
        let back = button(&gameplay, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_move(back.x + 5, back.y + 5, &gameplay);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptButtonBack"))
        );
        assert_eq!(state.drain_output().result, None);
        // The Tooltips checkbox lives on the Interface subpage. Its caption
        // takes no press; its 18x18 icon toggles, and the box starts ticked.
        let interface = settings_layout(Category::Interface);
        let crect = checkbox(&interface, LauncherCheckboxId::Tooltips);
        state.shell_mouse_down(crect.x + 30, crect.y + 5, &interface);
        assert!(state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        state.shell_mouse_down(crect.x + 5, crect.y + 5, &interface);
        assert!(!state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::Checkbox)]
        );
    }

    #[test]
    fn button_release_requires_the_pressed_button_and_popup_dismissal_consumes_click() {
        // At the root category list, a press lights the button but a release
        // off it does nothing; Back at the root requests the parent result.
        let categories = categories_layout();
        let mut state = state();
        let display = button(&categories, LauncherButton::Tab(Category::Display));
        state.shell_mouse_down(display.x + 5, display.y + 5, &categories);
        assert!(state.shell_button_pressed(LauncherButton::Tab(Category::Display)));
        // Release away from the button: no drill, nothing requested.
        state.shell_mouse_up(display.x + 5, display.y - 30, &categories);
        assert_eq!(state.drain_output().result, None);
        assert_eq!(state.tab(), Category::Display);
        // Back at the root requests the parent result.
        let back = button(&categories, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_down(back.x + 5, back.y + 5, &categories);
        state.shell_mouse_up(back.x + 5, back.y + 5, &categories);
        assert_eq!(state.drain_output().result, Some(LauncherParentResult::Back));

        // Drill into Display: its Zoom combo opens and a click elsewhere only
        // dismisses the popup without toggling a control.
        state.select_tab(Category::Display);
        let display_page = settings_layout(Category::Display);
        let zoom = display_page.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &display_page);
        assert_eq!(state.open_combo, Some(LauncherComboId::Zoom));
        state.shell_mouse_down(back.x + 5, back.y + 5, &display_page);
        state.shell_mouse_up(back.x + 5, back.y + 5, &display_page);
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
        // The dialog has one level: Back always closes it, whatever tab shows.
        state.shell_mouse_down(back.x + 5, back.y + 5, &display_page);
        state.shell_mouse_up(back.x + 5, back.y + 5, &display_page);
        assert_eq!(state.tab(), Category::Display);
        assert_eq!(state.drain_output().result, Some(LauncherParentResult::Back));
    }

    #[test]
    fn zoom_selection_uses_the_painted_row_and_emits_at_once() {
        let mut state = state();
        state.select_tab(Category::Display);
        let layout = settings_layout(Category::Display);
        // Open Zoom by clicking its own painted face, wherever the category
        // layout placed it.
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
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
        // 1920x1080 lists four steps; 300% is beyond them.
        let layout = LauncherOptionsLayout::responsive(
            1920,
            1080,
            Category::Display,
        );
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
        let layout = settings_layout(Category::Interface).with_battle_frame((1920, 1080));
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
        let zoomed = settings_layout(Category::Interface).with_battle_frame((1720, 720));
        assert_eq!(state.shell_selected_sidebar(&zoomed), SidebarSize::Large);
        state.shell_mouse_down(face.x + 10, face.y + 5, &zoomed);
        let popup = state.shell_popup(&zoomed).unwrap();
        assert_eq!((popup.rows, popup.selected), (2, 1));
    }

    #[test]
    fn display_list_picks_the_screen_a_row_names() {
        let mut state = state();
        let layout = settings_layout(Category::Display);
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
                "Screen 1: 1920x1080 60Hz".to_owned(),
                "Screen 2: 2560x1440 144Hz".to_owned(),
            ]
        );
        // The second screen's row: the pick publishes at once, like Sidebar.
        state.drain_output();
        let second = popup.row_rect(2);
        state.shell_mouse_down(second.x + 10, second.y + 5, &layout);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.shell_display_label(), "Screen 2: 2560x1440 144Hz");
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
        assert_eq!(state.shell_display_label(), "\\\\.\\DISPLAY9");
        let layout = settings_layout(Category::Display);
        let face = layout.combo_rect(LauncherComboId::Display).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.rows, popup.selected),
            (3, 0),
            "an absent screen still falls back to Auto's row"
        );
    }

    /// Each box on a settings subpage toggles with its cue and reports the
    /// change; a fresh subpage per box keeps them independent.
    #[test]
    fn settings_checkboxes_toggle_with_their_cue() {
        use LauncherCheckboxId::*;
        let layout = settings_layout(Category::Display);
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
        ] {
            let mut state = state();
            let rect = checkbox(&layout, id);
            assert_eq!(state.shell_checkbox_label(id), label);
            let before = state.shell_checkbox_checked(id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert_eq!(state.shell_checkbox_checked(id), !before, "{id:?}");
            assert_eq!(
                state.drain_output().events,
                [
                    LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                    event(!before)
                ],
                "{id:?}"
            );
        }
    }

    /// The three end-game boxes are the VERA boxes that start ticked: a stock
    /// profile plays and draws whatever a rules set names, so the player's
    /// first click turns that override off rather than on. Each publishes its
    /// own event and leaves the other two alone.
    #[test]
    fn the_end_game_boxes_start_ticked_and_publish_their_own_event() {
        use LauncherCheckboxId::{CustomEndGameBanner, CustomEndGameMovie, CustomEndGameScore};
        // The score box lives with the audio rows; the banner and movie boxes
        // live with the gameplay rows.
        for (category, id, label, off) in [
            (
                Category::Gameplay,
                CustomEndGameBanner,
                CUSTOM_END_GAME_BANNER_LABEL,
                LauncherOptionsEvent::CustomEndGameBannerChanged(false),
            ),
            (
                Category::Audio,
                CustomEndGameScore,
                CUSTOM_END_GAME_SCORE_LABEL,
                LauncherOptionsEvent::CustomEndGameScoreChanged(false),
            ),
            (
                Category::Gameplay,
                CustomEndGameMovie,
                CUSTOM_END_GAME_MOVIE_LABEL,
                LauncherOptionsEvent::CustomEndGameMovieChanged(false),
            ),
        ] {
            // A fresh page per box: each click is the player's first, and the
            // boxes must not remember each other's state.
            let layout = settings_layout(category);
            let mut state = state();
            assert_eq!(state.shell_checkbox_label(id), label);
            assert!(state.shell_checkbox_checked(id), "{id:?} starts ticked");
            let rect = checkbox(&layout, id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert!(!state.shell_checkbox_checked(id), "{id:?} unticked");
            assert_eq!(
                state.drain_output().events,
                [LauncherOptionsEvent::Cue(LauncherCue::Checkbox), off],
                "{id:?}"
            );
            for other in [CustomEndGameBanner, CustomEndGameScore, CustomEndGameMovie] {
                if other != id {
                    assert!(
                        state.shell_checkbox_checked(other),
                        "{id:?}'s click left {other:?} alone"
                    );
                }
            }
        }
    }

    /// The Accessibility slider publishes the level it stops on, not the
    /// position, so the profile never sees gamemd's bands as 0.
    #[test]
    fn house_color_strength_slider_publishes_the_level_it_stops_on() {
        let mut state = state();
        let layout = settings_layout(Category::Display);
        let slider = layout
            .trackbar_rect(LauncherTrackbarId::HouseColorStrength)
            .unwrap();
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL);
        assert_eq!(
            state.trackbar_plaque_text(LauncherTrackbarId::HouseColorStrength),
            "1"
        );

        // The strip admits from `bottom - 18`; the far right is the top level.
        let retail_pack = state.pack();
        state.shell_mouse_down(slider.x + slider.w - 2, slider.y + slider.h - 3, &layout);
        state.shell_mouse_up(slider.x + slider.w - 2, slider.y + slider.h - 3, &layout);
        assert_eq!(state.house_color_strength(), STRENGTH_MAX);
        assert_eq!(
            state.trackbar_plaque_text(LauncherTrackbarId::HouseColorStrength),
            "10"
        );
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::GenericClick),
                LauncherOptionsEvent::HouseColorStrengthChanged(STRENGTH_MAX),
            ],
            "the notch click leads the level it stands for"
        );
        assert_eq!(state.pack(), retail_pack, "the retail pack is untouched");

        // Back to the left end: the original bands, and a level of 1.
        state.shell_mouse_down(slider.x + 1, slider.y + slider.h - 3, &layout);
        state.shell_mouse_up(slider.x + 1, slider.y + slider.h - 3, &layout);
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::GenericClick),
                LauncherOptionsEvent::HouseColorStrengthChanged(STRENGTH_RETAIL),
            ]
        );
    }

    /// The Developer subpage surfaces every diagnostic toggle, including the
    /// Offline Internet (WOL) Test Service that the old VERA page kept hidden
    /// from the normal flow. Under the unified drill-down model the diagnostic
    /// toggles live on the Developer subpage and each toggles with its own
    /// typed event.
    #[test]
    fn developer_subpage_surfaces_the_diagnostic_wol_test_service() {
        let mut state = state();
        state.values.wol_test_service = true;
        let layout = settings_layout(Category::Developer);
        // The Developer subpage carries exactly its three diagnostic toggles.
        let ids: Vec<_> = layout.checkboxes.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&LauncherCheckboxId::WolTestService));
        assert!(ids.contains(&LauncherCheckboxId::FpsCounter));
        assert!(ids.contains(&LauncherCheckboxId::SkipStartupLogo));
        // Toggling the WOL test service emits its typed event and flips state.
        let rect = checkbox(&layout, LauncherCheckboxId::WolTestService);
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::WolTestServiceChanged(false),
            ]
        );
        assert!(!state.shell_checkbox_checked(LauncherCheckboxId::WolTestService));
    }

    #[test]
    fn fps_counter_box_follows_live_state() {
        let mut state = state();
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
