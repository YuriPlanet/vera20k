//! Physical-pixel presentation and input for launcher resource 0xD5.
//!
//! Retail RT_DIALOG/0xD5/1033 provides the DLU rectangles; common setup
//! 0x00608CD0/0x00609730 anchors the rail, and 0x0060B950 adjusts its title.
//! This module projects the retained OptionsDialogState instead of owning a
//! second set of option values or bypassing its ordered preview events.
//! The right-hand column picks a category; its page lays the controls out in
//! one column at the places retail's first band uses.

use super::{
    CHECK_FOR_UPDATES_HELP, CHECK_FOR_UPDATES_LABEL, CONTROLS_LABEL, CUSTOM_END_GAME_BANNER_HELP,
    CUSTOM_END_GAME_BANNER_LABEL, CUSTOM_END_GAME_MOVIE_HELP, CUSTOM_END_GAME_MOVIE_LABEL,
    CUSTOM_END_GAME_SCORE_HELP, CUSTOM_END_GAME_SCORE_LABEL, Category, DEVELOPER_OPTIONS_LABEL,
    DISPLAY_AUTO_LABEL, DISPLAY_HELP, DISPLAY_LABEL, DisplayTarget, FPS_COUNTER_HELP,
    FPS_COUNTER_LABEL, FULL_SCREEN_HELP, FULL_SCREEN_LABEL, HAS_CHECK_FOR_UPDATES,
    HD_MAP_PREVIEWS_HELP, HD_MAP_PREVIEWS_LABEL, HD_RADAR_HELP, HD_RADAR_LABEL, HEALTH_BARS_HELP,
    HEALTH_BARS_LABEL, HOUSE_COLOR_STRENGTH_HELP, HOUSE_COLOR_STRENGTH_LABEL, HealthBarMode,
    LOCK_CURSOR_HELP, LOCK_CURSOR_LABEL, LauncherButton, LauncherCheckboxId, LauncherComboId,
    LauncherCue, LauncherOptionsEvent, LauncherParentResult, LauncherTrackbarId,
    MOUSE_WHEEL_ZOOM_HELP, MOUSE_WHEEL_ZOOM_LABEL, ORIGINAL_SHAPE_HELP, ORIGINAL_SHAPE_LABEL,
    OptionsDialogState, PhysicalControlFrame, RENDER_INTERPOLATION_HELP,
    RENDER_INTERPOLATION_LABEL, RIGHT_CLICK_ORDERS_HELP, RIGHT_CLICK_ORDERS_LABEL,
    SHIFT_QUEUE_HELP, SHIFT_QUEUE_LABEL, SIDEBAR_HELP, SIDEBAR_LABEL, SKIP_INTRO_HELP,
    SKIP_INTRO_LABEL, SMOOTH_SIDEBAR_HELP, SMOOTH_SIDEBAR_LABEL, Setting, SidebarSize,
    VERA_ICON_HELP, VERA_ICON_LABEL, WOL_TEST_SERVICE_HELP, WOL_TEST_SERVICE_LABEL,
    YR_ORIGINAL_STYLE_HELP, YR_ORIGINAL_STYLE_LABEL, ZOOM_HELP, ZOOM_LABEL, Zoom,
};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, ShellScale, center_offset, child_window, dlu_rect, right_panel_rects,
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

impl ShellInteraction {
    /// After a category switch: the column doesn't move, so the pointer still
    /// rests on the button it released, and that button's hover and status
    /// help stay.
    pub(super) fn page_switched(self) -> Self {
        Self {
            hovered_button: self.hovered_button,
            status_help: self.status_help,
            ..Self::default()
        }
    }
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
pub(crate) const MAIN_MENU_BUTTON: u16 = 0x0686;
/// VERA's category buttons under Keyboard, in [`Category::ALL`]'s order. Not
/// retail controls: the ids only name their rects in the page layout.
const CATEGORY_BUTTONS: [u16; 6] = [0x7FF0, 0x7FF1, 0x7FF2, 0x7FF3, 0x7FF4, 0x7FF5];
const _: () = assert!(CATEGORY_BUTTONS.len() == Category::ALL.len());

/// A category's button: the stacked tops go on 27 DLUs at a time from
/// Keyboard's, as the family pages' do, so each lands on the next tile.
/// Its caption is the category's name and its help [`Category::help`].
const fn category_button(index: usize) -> MenuPageButtonSpec {
    MenuPageButtonSpec {
        id: CATEGORY_BUTTONS[index],
        dlu_top: 149 + 27 * index as i32,
        csf_key: "",
        tooltip_key: "",
        result: None,
    }
}

/// `0xD5` as a family page (Session `+0x30D8` clear at the main menu): the
/// right-panel column is Keyboard and Network (`0x006092CB`), then Main Menu
/// (`0x0060982C`); the heading and status line sit at the family places.
/// VERA leaves Network (`0x5CD`) out: it opens the IPX settings page `0xD7`,
/// and VERA's LAN runs on IPv4 UDP. VERA's category buttons take the six
/// tiles under Keyboard. Button captions come from the dialog's own labels.
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
        category_button(0),
        category_button(1),
        category_button(2),
        category_button(3),
        category_button(4),
        category_button(5),
    ],
    back: MenuPageButtonSpec {
        id: MAIN_MENU_BUTTON,
        dlu_top: 346,
        csf_key: "GUI:MainMenu",
        tooltip_key: "STT:MainOptButtonBack",
        result: Some(0x05CB),
    },
};

/// Whether the retail panel at this window size has a tile for every column
/// button above Main Menu's. 640x480's six tiles hold five, too few for the
/// seven, so `0xD5` takes the fitted 800x600 page there instead: not native,
/// and the only size whose Options art is scaled while the other pages keep
/// the 640-wide set.
pub(crate) fn retail_column_fits(width: i32, height: i32) -> bool {
    right_panel_rects(width, height).tile_count > OPTIONS_PAGE.stacked.len() as i32
}

// A page in DLUs: the category's name in the first retail band heading's
// place (`GUI:DisplayOptions` at 20,18), then one column of rows from the
// Visual Details caption's place (89,38). Boxes keep the UI Options boxes' x
// and 15-DLU pitch. A slider's track sits 15 DLUs under its caption, as
// Visual Details' does, and the next row starts 9 DLUs under the track: at
// 640x480's 0.8 fit, 8 puts a stacked slider's edge where it maps back a
// pixel off, and that slider loses its frame art. A list's face hangs 16 px
// under its caption's top, as VERA's lists did on main's page, and the next
// row starts 4 px under the face.
const HEADING: (i32, i32) = (20, 18);
const ROW_X: i32 = 89;
const BOX_X: i32 = 93;
const FIRST_ROW: i32 = 38;
const BOX_PITCH: i32 = 15;
const SLIDER_PITCH: i32 = 37;
const LIST_PITCH: i32 = 27;
/// A box's width. Retail's are 130 DLUs, where VERA's longer captions, such as
/// "Custom End Game Banner", wrap, and a caption that wraps loses its second
/// line to the 10-DLU height.
const BOX_W: i32 = 160;

/// A caption the page paints, by what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageLabel {
    /// The category's name, in a retail band heading's place.
    Heading,
    /// A list's caption, over its face.
    List(LauncherComboId),
    /// A slider's caption, over its track.
    Slider(LauncherTrackbarId),
    /// The step a plaque-less slider stands at, right-aligned over the
    /// track's end as retail prints Visual Details'.
    Step(LauncherTrackbarId),
    /// Static `GUI:Blank` at the status line's template place.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLayout {
    /// The category whose page this is.
    pub(crate) category: Category,
    pub(crate) title: RectPx,
    pub(crate) warning: RectPx,
    /// Status line `0x695`.
    pub(crate) status_help: RectPx,
    pub(crate) labels: Vec<(RectPx, PageLabel)>,
    pub(crate) combos: Vec<(LauncherComboId, RectPx)>,
    pub(crate) trackbars: Vec<(LauncherTrackbarId, RectPx)>,
    pub(crate) checkboxes: Vec<(LauncherCheckboxId, RectPx)>,
    /// The column top to bottom, then the page's own Check for Updates.
    pub(crate) buttons: Vec<(LauncherButton, RectPx)>,
    /// The mapping the rects are in: [`ShellScale::RETAIL`] centres the
    /// authored dialog in the window, otherwise the authored dialog is
    /// scaled to fit it, centred. Control sub-rects and art fits key on this.
    pub(crate) scale: ShellScale,
    /// The window, which a match shares: the zoom list depends on it.
    screen: (u32, u32),
    /// A battle's frame in this window at the chosen zoom and shape: the
    /// sidebar list depends on it.
    battle_frame: (u32, u32),
}

impl LauncherOptionsLayout {
    /// Gamemd's centred `0xD5`: the authored dialog placed in the window at
    /// [`ShellScale::RETAIL`], or fitted where the panel is too short for the
    /// column ([`retail_column_fits`]).
    pub(crate) fn new(width: i32, height: i32, category: Category) -> Self {
        let scale = if retail_column_fits(width, height) {
            ShellScale::RETAIL
        } else {
            ShellScale::from_screen(width, height)
        };
        Self::build(width, height, scale, category)
    }

    /// The same dialog scaled to fit the window.
    pub(crate) fn responsive(width: i32, height: i32, category: Category) -> Self {
        Self::build(
            width,
            height,
            ShellScale::from_screen(width, height),
            category,
        )
    }

    fn build(width: i32, height: i32, scale: ShellScale, category: Category) -> Self {
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
        let panel = if stretch {
            menu_page::compute_responsive_layout(&OPTIONS_PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_layout(&OPTIONS_PAGE, width as u32, height as u32)
        };
        let column = |id| panel.button_rect(id).expect("0xD5 column button");
        let mut buttons = vec![(
            LauncherButton::Exit(LauncherParentResult::Keyboard),
            column(KEYBOARD_BUTTON),
        )];
        buttons.extend(
            Category::ALL
                .into_iter()
                .zip(CATEGORY_BUTTONS)
                .map(|(category, id)| (LauncherButton::Category(category), column(id))),
        );
        buttons.push((
            LauncherButton::Exit(LauncherParentResult::Back),
            column(MAIN_MENU_BUTTON),
        ));

        let mut labels = vec![
            (dlu(HEADING.0, HEADING.1, 298, 10), PageLabel::Heading),
            (dlu(2, 355, 303, 12), PageLabel::Blank),
        ];
        let mut combos = Vec::new();
        let mut trackbars = Vec::new();
        let mut checkboxes = Vec::new();
        let mut top = FIRST_ROW;
        for &setting in category.settings() {
            match setting {
                Setting::List(id) => {
                    labels.push((dlu(ROW_X, top, 120, 10), PageLabel::List(id)));
                    // The chrome atlas keys combo faces on their pixel size:
                    // 180 is the retail resolution list's face.
                    let caption = dlu_rect(ROW_X, top, 0, 0);
                    combos.push((
                        id,
                        px(RectPx::new(caption.x, caption.y + 16, 180, COMBO_FACE_H)),
                    ));
                    top += LIST_PITCH;
                }
                // Trackbar art exists for retail's windows only: 150 DLUs with
                // a value plaque (Generate Map's), 120 without (Visual Details').
                Setting::Slider(id) if id.plaque_reserve() > 0 => {
                    labels.push((dlu(ROW_X, top, 150, 10), PageLabel::Slider(id)));
                    trackbars.push((id, slider(ROW_X, top + 15, 150, 13)));
                    top += SLIDER_PITCH;
                }
                Setting::Slider(id) => {
                    labels.push((dlu(ROW_X, top, 60, 10), PageLabel::Slider(id)));
                    labels.push((dlu(ROW_X + 60, top, 60, 10), PageLabel::Step(id)));
                    trackbars.push((id, slider(ROW_X, top + 15, 120, 13)));
                    top += SLIDER_PITCH;
                }
                Setting::Checkbox(id) => {
                    checkboxes.push((id, dlu(BOX_X, top, BOX_W, 10)));
                    top += BOX_PITCH;
                }
            }
        }
        // Not retail: the column has no tile left for it, so it sits under the
        // Developer page's boxes, the size of Sound Options' Play and Stop
        // (`0xD6`, MNBTTN type 3).
        if category == Category::Developer && HAS_CHECK_FOR_UPDATES {
            buttons.push((LauncherButton::CheckForUpdates, dlu(ROW_X, top + 6, 83, 15)));
        }
        Self {
            category,
            title: panel.title,
            warning: panel.warning_monitor,
            status_help: panel.status_help,
            labels,
            combos,
            trackbars,
            checkboxes,
            buttons,
            scale,
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
        let mut result = vec![LauncherShellLabel {
            rect: layout.title,
            text: self.shell_title_text(),
            align: Center,
            title: true,
        }];
        result.extend(layout.labels.iter().map(|&(rect, label)| {
            let (text, align) = match label {
                PageLabel::Heading => (self.shell_category_label(layout.category), Left),
                PageLabel::List(id) => (list_caption(id), Left),
                PageLabel::Slider(id) => (self.slider_caption(id), Left),
                PageLabel::Step(id) => (self.slider_step(id), Right),
                PageLabel::Blank => (self.labels.blank.as_str(), Left),
            };
            LauncherShellLabel {
                rect,
                text,
                align,
                title: false,
            }
        }));
        result
    }

    /// A category's name: retail's band heading for the four bands `0xD5`
    /// has, VERA's own for the other two.
    pub(crate) fn shell_category_label(&self, category: Category) -> &str {
        match category {
            Category::Display => &self.labels.display_options,
            Category::Gameplay => &self.labels.game_options,
            Category::Interface => &self.labels.ui_options,
            Category::Audio => &self.labels.audio_options,
            Category::Controls => CONTROLS_LABEL,
            Category::Developer => DEVELOPER_OPTIONS_LABEL,
        }
    }

    fn slider_caption(&self, id: LauncherTrackbarId) -> &str {
        match id {
            LauncherTrackbarId::Detail => &self.labels.visual_details,
            LauncherTrackbarId::Difficulty => &self.labels.difficulty,
            LauncherTrackbarId::Scroll => &self.labels.scroll_rate,
            LauncherTrackbarId::Score => &self.labels.music_volume,
            LauncherTrackbarId::Sound => &self.labels.sound_volume,
            LauncherTrackbarId::Voice => &self.labels.voice_volume,
            LauncherTrackbarId::HouseColorStrength => HOUSE_COLOR_STRENGTH_LABEL,
        }
    }

    /// The step name a plaque-less slider prints; a plaque slider shows its
    /// value on the plaque instead.
    fn slider_step(&self, id: LauncherTrackbarId) -> &str {
        match id {
            LauncherTrackbarId::Detail => &self.detail_caption,
            LauncherTrackbarId::Difficulty => &self.difficulty_caption,
            LauncherTrackbarId::Scroll => &self.scroll_caption,
            _ => "",
        }
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
            LauncherCheckboxId::MouseWheelZoom => MOUSE_WHEEL_ZOOM_LABEL,
            LauncherCheckboxId::RightClickOrders => RIGHT_CLICK_ORDERS_LABEL,
            LauncherCheckboxId::LockCursor => LOCK_CURSOR_LABEL,
            LauncherCheckboxId::VeraIcon => VERA_ICON_LABEL,
            LauncherCheckboxId::HdRadar => HD_RADAR_LABEL,
            LauncherCheckboxId::SmoothSidebar => SMOOTH_SIDEBAR_LABEL,
            LauncherCheckboxId::HdMapPreviews => HD_MAP_PREVIEWS_LABEL,
            LauncherCheckboxId::ShiftQueue => SHIFT_QUEUE_LABEL,
            LauncherCheckboxId::WolTestService => WOL_TEST_SERVICE_LABEL,
            LauncherCheckboxId::FpsCounter => FPS_COUNTER_LABEL,
            LauncherCheckboxId::SkipIntro => SKIP_INTRO_LABEL,
            LauncherCheckboxId::CustomEndGameBanner => CUSTOM_END_GAME_BANNER_LABEL,
            LauncherCheckboxId::CustomEndGameScore => CUSTOM_END_GAME_SCORE_LABEL,
            LauncherCheckboxId::CustomEndGameMovie => CUSTOM_END_GAME_MOVIE_LABEL,
        }
    }

    pub(crate) fn shell_button_label(&self, id: LauncherButton) -> &str {
        match id {
            LauncherButton::Exit(LauncherParentResult::Keyboard) => &self.labels.keyboard,
            LauncherButton::Exit(LauncherParentResult::Back) => &self.labels.main_menu,
            LauncherButton::Exit(LauncherParentResult::Terminal) => "",
            LauncherButton::Category(category) => self.shell_category_label(category),
            LauncherButton::CheckForUpdates => CHECK_FOR_UPDATES_LABEL,
        }
    }

    /// Whether `id` is the column button of the page that shows.
    pub(crate) fn shell_button_selected(&self, id: LauncherButton) -> bool {
        id == LauncherButton::Category(self.category)
    }

    /// The dialog's heading, the same on every page.
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
            LauncherCheckboxId::HdRadar => self.values.hd_radar,
            LauncherCheckboxId::SmoothSidebar => self.values.smooth_sidebar,
            LauncherCheckboxId::HdMapPreviews => self.values.hd_map_previews,
            LauncherCheckboxId::ShiftQueue => self.values.shift_queue,
            LauncherCheckboxId::WolTestService => self.values.wol_test_service,
            LauncherCheckboxId::FpsCounter => self.values.fps_counter,
            LauncherCheckboxId::SkipIntro => self.values.skip_intro,
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

    /// The Health Bars face's text: the retained choice.
    pub(crate) fn shell_selected_health_bars(&self) -> HealthBarMode {
        self.values.health_bars
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
    /// each screen's `DisplayChoice::label`, numbered from 1.
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
            LauncherComboId::HealthBars => {
                let choices = HealthBarMode::choices();
                let selected = choices
                    .iter()
                    .position(|mode| *mode == self.values.health_bars);
                (choices.len(), selected)
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
                LauncherButton::Category(category) => {
                    return Some(LauncherStatusHelp::Text(category.help()));
                }
                LauncherButton::CheckForUpdates => {
                    return Some(LauncherStatusHelp::Text(CHECK_FOR_UPDATES_HELP));
                }
                LauncherButton::Exit(LauncherParentResult::Keyboard) => KEYBOARD_BUTTON,
                LauncherButton::Exit(
                    LauncherParentResult::Back | LauncherParentResult::Terminal,
                ) => MAIN_MENU_BUTTON,
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
                LauncherCheckboxId::HdRadar => LauncherStatusHelp::Text(HD_RADAR_HELP),
                LauncherCheckboxId::SmoothSidebar => LauncherStatusHelp::Text(SMOOTH_SIDEBAR_HELP),
                LauncherCheckboxId::HdMapPreviews => LauncherStatusHelp::Text(HD_MAP_PREVIEWS_HELP),
                LauncherCheckboxId::ShiftQueue => LauncherStatusHelp::Text(SHIFT_QUEUE_HELP),
                LauncherCheckboxId::WolTestService => {
                    LauncherStatusHelp::Text(WOL_TEST_SERVICE_HELP)
                }
                LauncherCheckboxId::FpsCounter => LauncherStatusHelp::Text(FPS_COUNTER_HELP),
                LauncherCheckboxId::SkipIntro => LauncherStatusHelp::Text(SKIP_INTRO_HELP),
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
                    LauncherComboId::HealthBars => HEALTH_BARS_HELP,
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
                LauncherComboId::HealthBars => {
                    match row.and_then(|row| HealthBarMode::choices().get(row).copied()) {
                        Some(mode) => self.select_health_bars(mode),
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
                LauncherButton::Category(category) => self.select_category(category),
                LauncherButton::CheckForUpdates => self.request_update_check(),
            }
        }
    }
}

/// A list's caption: VERA's own, as retail has no such lists.
fn list_caption(id: LauncherComboId) -> &'static str {
    match id {
        LauncherComboId::Zoom => ZOOM_LABEL,
        LauncherComboId::Sidebar => SIDEBAR_LABEL,
        LauncherComboId::Display => DISPLAY_LABEL,
        LauncherComboId::HealthBars => HEALTH_BARS_LABEL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::house_colors::{STRENGTH_MAX, STRENGTH_RETAIL};
    use crate::ui::main_menu_dialogs::options::{
        DisplayChoice, LauncherCue, LauncherOptionsEvent, LauncherOptionsLabels,
        LauncherOptionsValues,
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

    /// A category's page in the retail 800x600 window.
    fn page(category: Category) -> LauncherOptionsLayout {
        LauncherOptionsLayout::new(800, 600, category)
    }

    fn checkbox(layout: &LauncherOptionsLayout, id: LauncherCheckboxId) -> RectPx {
        layout
            .checkboxes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.category))
    }

    fn button(layout: &LauncherOptionsLayout, id: LauncherButton) -> RectPx {
        layout
            .buttons
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.category))
    }

    fn disjoint(a: RectPx, b: RectPx) -> bool {
        a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
    }

    /// Every slider a page lays out must land on one of the chrome atlas's
    /// authored trackbar windows. The frame lookup is an exact size match, so
    /// a row sized like a plain track but painted with a value plaque
    /// resolves to no frame at all: the page would draw the thumb and the
    /// plaque over empty background.
    #[test]
    fn every_page_slider_lands_on_an_authored_trackbar_frame() {
        use crate::render::skirmish_shell_chrome::trackbar_frame_slot;
        for category in Category::ALL {
            let layout = page(category);
            for &(id, rect) in &layout.trackbars {
                let base = layout.scale.base_rect(rect);
                assert!(
                    trackbar_frame_slot(id.plaque_reserve() > 0, base.w, base.h).is_some(),
                    "{id:?} on the {category:?} page is {}x{}, which no authored trackbar frame covers",
                    base.w,
                    base.h
                );
            }
        }
    }

    /// The chrome atlas keys combo faces on exact authored widths, so a combo
    /// laid out at any other width paints its arrow over no face at all.
    #[test]
    fn every_combo_face_uses_an_authored_width() {
        for category in Category::ALL {
            let layout = page(category);
            for &(id, rect) in &layout.combos {
                assert_eq!(
                    layout.scale.base_rect(rect).w,
                    180,
                    "{id:?} on the {category:?} page"
                );
            }
        }
    }

    /// The right-hand column: retail's Keyboard, a button per category on the
    /// tiles under it, and Main Menu on the last of the 800x600 panel's nine
    /// tiles, each a 156x42 SDBTNANM cell flush with the panel's right edge.
    #[test]
    fn the_column_holds_keyboard_every_category_and_main_menu() {
        let layout = page(Category::Display);
        let mut expected = vec![LauncherButton::Exit(LauncherParentResult::Keyboard)];
        expected.extend(Category::ALL.map(LauncherButton::Category));
        expected.push(LauncherButton::Exit(LauncherParentResult::Back));
        let ids: Vec<_> = layout.buttons.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, expected);
        let tile = |row: i32| RectPx::new(644, 199 + 42 * row, 156, 42);
        let (main_menu, stacked) = layout.buttons.split_last().unwrap();
        for (row, (id, rect)) in stacked.iter().enumerate() {
            assert_eq!(*rect, tile(row as i32), "{id:?}");
        }
        assert_eq!(main_menu.1, tile(8));
    }

    /// 640x480's retail panel has six tiles, five above Main Menu: too few for
    /// the column, so the page takes the fitted 800x600 layout there, and
    /// every button keeps a tile of its own inside the window.
    #[test]
    fn a_window_too_short_for_the_column_takes_the_fitted_page() {
        assert!(retail_column_fits(800, 600));
        assert!(!retail_column_fits(640, 480));
        let layout = LauncherOptionsLayout::new(640, 480, Category::Display);
        assert_eq!(
            layout,
            LauncherOptionsLayout::responsive(640, 480, Category::Display)
        );
        assert_eq!(layout.buttons.len(), 8);
        for (index, (id, a)) in layout.buttons.iter().enumerate() {
            assert!(a.x + a.w <= 640 && a.y + a.h <= 480, "{id:?} at {a:?}");
            for (other, b) in &layout.buttons[index + 1..] {
                assert!(disjoint(*a, *b), "{id:?} overlaps {other:?}");
            }
        }
    }

    /// Every page's controls stay left of the column, above the status line
    /// and clear of one another, at the retail size and fitted ones.
    #[test]
    fn every_page_keeps_its_controls_clear_of_one_another_and_the_column() {
        for (width, height) in [(800, 600), (640, 480), (1920, 1080)] {
            for category in Category::ALL {
                let layout = if (width, height) == (1920, 1080) {
                    LauncherOptionsLayout::responsive(width, height, category)
                } else {
                    LauncherOptionsLayout::new(width, height, category)
                };
                let column = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
                let rects: Vec<RectPx> = layout
                    .checkboxes
                    .iter()
                    .map(|(_, rect)| *rect)
                    .chain(layout.combos.iter().map(|(_, rect)| *rect))
                    .chain(layout.trackbars.iter().map(|(_, rect)| *rect))
                    .chain(
                        layout
                            .buttons
                            .iter()
                            .filter(|(id, _)| *id == LauncherButton::CheckForUpdates)
                            .map(|(_, rect)| *rect),
                    )
                    .collect();
                for (index, a) in rects.iter().enumerate() {
                    assert!(
                        a.x >= 0 && a.x + a.w <= column.x && a.y + a.h <= layout.status_help.y,
                        "{category:?} at {width}x{height}: {a:?}"
                    );
                    for b in &rects[index + 1..] {
                        assert!(
                            disjoint(*a, *b),
                            "{category:?} at {width}x{height}: {a:?} overlaps {b:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_stretched_slider_maps_the_pointer_back_to_its_authored_track() {
        let mut dialog = state();
        let base = page(Category::Audio);
        let scale = ShellScale::from_screen(1600, 1200);
        let wide = LauncherOptionsLayout::responsive(1600, 1200, Category::Audio);
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

    /// A category button shows its page in place, applying nothing; the
    /// page's heading is retail's band name. Main Menu closes from any page.
    #[test]
    fn pressing_a_category_shows_its_page_in_place_and_main_menu_closes() {
        let before = state().pack();
        let mut state = state();
        assert_eq!(state.category(), Category::Display);
        assert!(state.shell_button_selected(LauncherButton::Category(Category::Display)));
        let display = page(Category::Display);
        let audio = button(&display, LauncherButton::Category(Category::Audio));
        state.shell_mouse_down(audio.x + 5, audio.y + 5, &display);
        assert!(state.shell_button_pressed(LauncherButton::Category(Category::Audio)));
        state.shell_mouse_up(audio.x + 5, audio.y + 5, &display);
        assert_eq!(state.category(), Category::Audio);
        assert!(state.shell_button_selected(LauncherButton::Category(Category::Audio)));
        assert!(!state.shell_button_selected(LauncherButton::Category(Category::Display)));
        let output = state.drain_output();
        assert_eq!(
            output.events,
            [LauncherOptionsEvent::Cue(LauncherCue::MainButton)]
        );
        assert_eq!((output.result, output.check_for_updates), (None, false));
        // The pointer still rests on the button: its help stays up.
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Text(Category::Audio.help()))
        );
        let audio_page = page(Category::Audio);
        let labels = state.shell_labels(&audio_page);
        assert_eq!(
            (labels[0].text, labels[1].text),
            ("Options", "Audio Options")
        );
        let back = button(
            &audio_page,
            LauncherButton::Exit(LauncherParentResult::Back),
        );
        state.shell_mouse_down(back.x + 5, back.y + 5, &audio_page);
        state.shell_mouse_up(back.x + 5, back.y + 5, &audio_page);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Back)
        );
        assert_eq!(state.pack(), before);
    }

    /// Switching pages closes an open list: the next page starts clean.
    #[test]
    fn switching_category_drops_an_open_list() {
        let mut state = state();
        let layout = page(Category::Display);
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::Zoom));
        state.select_category(Category::Audio);
        assert_eq!(state.open_combo, None);
        assert!(state.shell_popup(&page(Category::Audio)).is_none());
    }

    #[test]
    fn status_help_names_the_control_under_the_pointer() {
        let csf = |key| Some(LauncherStatusHelp::Csf(key));
        let text = |text| Some(LauncherStatusHelp::Text(text));
        let dialog = state();
        let layout = page(Category::Display);
        let at = |rect: RectPx| dialog.shell_status_help_key(rect.x + 2, rect.y + 2, &layout);
        for (result, key) in [
            (LauncherParentResult::Keyboard, "STT:MainOptButtonKeyboard"),
            (LauncherParentResult::Back, "STT:MainOptButtonBack"),
        ] {
            assert_eq!(
                at(button(&layout, LauncherButton::Exit(result))),
                csf(key),
                "{result:?}"
            );
        }
        for category in Category::ALL {
            assert_eq!(
                at(button(&layout, LauncherButton::Category(category))),
                text(category.help())
            );
        }
        assert_eq!(dialog.shell_status_help_key(5, 5, &layout), None);
        assert_eq!(
            at(layout.combo_rect(LauncherComboId::Zoom).unwrap()),
            text(ZOOM_HELP)
        );
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
    fn check_for_updates_sits_on_the_developer_page_and_only_asks() {
        for category in Category::ALL {
            assert_eq!(
                page(category)
                    .buttons
                    .iter()
                    .any(|(id, _)| *id == LauncherButton::CheckForUpdates),
                HAS_CHECK_FOR_UPDATES && category == Category::Developer,
                "{category:?}"
            );
        }
        if !HAS_CHECK_FOR_UPDATES {
            return;
        }
        let mut state = state();
        state.select_category(Category::Developer);
        let layout = page(Category::Developer);
        let check = button(&layout, LauncherButton::CheckForUpdates);
        let last_box = layout.checkboxes.last().unwrap().1;
        assert!(
            check.y >= last_box.y + last_box.h,
            "{check:?} under {last_box:?}"
        );
        assert_eq!(
            state.shell_button_label(LauncherButton::CheckForUpdates),
            "Check for Updates"
        );
        let before = state.pack();
        state.shell_mouse_down(check.x + 5, check.y + 5, &layout);
        state.shell_mouse_up(check.x + 5, check.y + 5, &layout);
        let output = state.drain_output();
        assert_eq!((output.result, output.check_for_updates), (None, true));
        assert_eq!(state.pack(), before);
    }

    #[test]
    fn physical_slider_capture_crosses_other_controls_without_activating_them() {
        let mut state = state();
        let layout = page(Category::Audio);
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
        let layout = page(Category::Audio);
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
        let gameplay = page(Category::Gameplay);
        let srect = gameplay
            .trackbar_rect(LauncherTrackbarId::Difficulty)
            .unwrap();
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
        // Main Menu's status help reads after hovering it.
        let back = button(&gameplay, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_move(back.x + 5, back.y + 5, &gameplay);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptButtonBack"))
        );
        assert_eq!(state.drain_output().result, None);
        // The Tooltips box: its caption takes no press; its 18x18 icon
        // toggles, and the box starts ticked.
        let interface = page(Category::Interface);
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
        let layout = page(Category::Display);
        let mut state = state();
        // A press lights the button; a release off it does nothing.
        let audio = button(&layout, LauncherButton::Category(Category::Audio));
        state.shell_mouse_down(audio.x + 5, audio.y + 5, &layout);
        assert!(state.shell_button_pressed(LauncherButton::Category(Category::Audio)));
        state.shell_mouse_up(audio.x - 200, audio.y + 5, &layout);
        assert_eq!(state.category(), Category::Display);
        assert_eq!(state.drain_output().result, None);
        // With a list open, a click elsewhere only closes it.
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::Zoom));
        let back = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_down(back.x + 5, back.y + 5, &layout);
        state.shell_mouse_up(back.x + 5, back.y + 5, &layout);
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
        state.shell_mouse_down(back.x + 5, back.y + 5, &layout);
        state.shell_mouse_up(back.x + 5, back.y + 5, &layout);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Back)
        );
    }

    #[test]
    fn zoom_selection_uses_the_painted_row_and_emits_at_once() {
        let mut state = state();
        let layout = page(Category::Display);
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
        let layout = LauncherOptionsLayout::responsive(1920, 1080, Category::Display);
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
        let layout = page(Category::Interface).with_battle_frame((1920, 1080));
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
        let zoomed = page(Category::Interface).with_battle_frame((1720, 720));
        assert_eq!(state.shell_selected_sidebar(&zoomed), SidebarSize::Large);
        state.shell_mouse_down(face.x + 10, face.y + 5, &zoomed);
        let popup = state.shell_popup(&zoomed).unwrap();
        assert_eq!((popup.rows, popup.selected), (2, 1));
    }

    #[test]
    fn display_list_picks_the_screen_a_row_names() {
        let mut state = state();
        let layout = page(Category::Display);
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
        let layout = page(Category::Display);
        let face = layout.combo_rect(LauncherComboId::Display).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.rows, popup.selected),
            (3, 0),
            "an absent screen still falls back to Auto's row"
        );
    }

    /// Each box on the Display page toggles with its cue and reports the
    /// change; a fresh dialog per box keeps them independent.
    #[test]
    fn settings_checkboxes_toggle_with_their_cue() {
        use LauncherCheckboxId::*;
        let layout = page(Category::Display);
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

    /// The three end-game boxes start unticked like every VERA box. Each
    /// publishes its own event and leaves the other two alone.
    #[test]
    fn the_end_game_boxes_start_unticked_and_publish_their_own_event() {
        use LauncherCheckboxId::{CustomEndGameBanner, CustomEndGameMovie, CustomEndGameScore};
        let layout = page(Category::Gameplay);
        for (id, label, on) in [
            (
                CustomEndGameBanner,
                CUSTOM_END_GAME_BANNER_LABEL,
                LauncherOptionsEvent::CustomEndGameBannerChanged(true),
            ),
            (
                CustomEndGameScore,
                CUSTOM_END_GAME_SCORE_LABEL,
                LauncherOptionsEvent::CustomEndGameScoreChanged(true),
            ),
            (
                CustomEndGameMovie,
                CUSTOM_END_GAME_MOVIE_LABEL,
                LauncherOptionsEvent::CustomEndGameMovieChanged(true),
            ),
        ] {
            // A fresh dialog per box: each click is the player's first, and
            // the boxes must not remember each other's state.
            let mut state = state();
            assert_eq!(state.shell_checkbox_label(id), label);
            assert!(!state.shell_checkbox_checked(id), "{id:?} starts unticked");
            let rect = checkbox(&layout, id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert!(state.shell_checkbox_checked(id), "{id:?} ticked");
            assert_eq!(
                state.drain_output().events,
                [LauncherOptionsEvent::Cue(LauncherCue::Checkbox), on],
                "{id:?}"
            );
            for other in [CustomEndGameBanner, CustomEndGameScore, CustomEndGameMovie] {
                if other != id {
                    assert!(
                        !state.shell_checkbox_checked(other),
                        "{id:?}'s click left {other:?} alone"
                    );
                }
            }
        }
    }

    /// The Health Bars list on the Interface page: Original, All Units, and
    /// All Units, Not Full. Its pick publishes at once like the Sidebar list.
    #[test]
    fn health_bars_list_publishes_its_pick_at_once() {
        let mut state = state();
        let layout = page(Category::Interface);
        let face = layout.combo_rect(LauncherComboId::HealthBars).unwrap();
        assert_eq!(state.shell_selected_health_bars(), HealthBarMode::Vanilla);
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.open_combo, Some(LauncherComboId::HealthBars));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::HealthBars, 3, 0)
        );
        state.drain_output();
        let last = popup.row_rect(2);
        state.shell_mouse_down(last.x + 10, last.y + 5, &layout);
        assert_eq!(state.open_combo, None);
        assert_eq!(
            state.shell_selected_health_bars(),
            HealthBarMode::AlwaysExceptFull
        );
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboClose),
                LauncherOptionsEvent::HealthBarModeSelected(HealthBarMode::AlwaysExceptFull)
            ]
        );
    }

    /// The Team Color Strength slider publishes the level it stops on, not
    /// the position, so the profile never sees gamemd's bands as 0.
    #[test]
    fn house_color_strength_slider_publishes_the_level_it_stops_on() {
        let mut state = state();
        let layout = page(Category::Display);
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

    #[test]
    fn the_developer_page_cannot_toggle_the_diagnostic_wol_test_service() {
        let mut state = state();
        state.values.wol_test_service = true;
        for category in Category::ALL {
            assert!(
                page(category)
                    .checkboxes
                    .iter()
                    .all(|(id, _)| *id != LauncherCheckboxId::WolTestService),
                "{category:?}"
            );
        }
        // The Developer page holds FPS and Skip Startup Logo only.
        let layout = page(Category::Developer);
        for (id, event) in [
            (
                LauncherCheckboxId::FpsCounter,
                LauncherOptionsEvent::FpsCounterChanged(true),
            ),
            (
                LauncherCheckboxId::SkipIntro,
                LauncherOptionsEvent::SkipIntroChanged(true),
            ),
        ] {
            let rect = checkbox(&layout, id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert_eq!(
                state.drain_output().events,
                [LauncherOptionsEvent::Cue(LauncherCue::Checkbox), event]
            );
        }
        assert!(state.shell_checkbox_checked(LauncherCheckboxId::WolTestService));
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
