//! Retained semantic model for the retail launcher Options dialog `0xD5`.
//!
//! The parent owns bounded control state and emits ordered effects. It owns no
//! profile, filesystem, audio device, window, or child-dialog callback.
//! The `shell` child supplies retail resource geometry and physical-pixel input;
//! the app shell renderer paints that model through the shared retail compositor.
//! Retail's controls and VERA's own share one dialog, sorted into the
//! [`Category`] pages its right-hand column picks between.

use crate::ui::shell::trackbar::{
    TrackbarHold, TrackbarPress, trackbar_position_from_x, trackbar_press,
};

pub(crate) mod shell;

/// gamemd-derived: launcher owner `OptionsClass__ShowLauncherDialog @
/// 0x0055FC80` and its primary-proc slice `0x0055FDB0..0x0056047A` bind the
/// RT_DIALOG `0xD5` controls to these launcher-local CSF keys/fallbacks. The
/// template's slider captions (`GUI:HigherDetail`, `GUI:Harder`,
/// `GUI:Faster`) never show: the init renames them by position. The combo's
/// own caption, `GUI:SetResolution`, left with the resolution list: the combo
/// is VERA's [`Zoom`] list now.
pub(crate) const LAUNCHER_LABEL_SPECS: [(&str, &str); 29] = [
    ("GUI:OptionsMenu", "Options"),
    ("GUI:MainMenu", "Main Menu"),
    ("GUI:Keyboard", "Keyboard"),
    ("GUI:DisplayOptions", "Display Options"),
    ("GUI:GameOptions", "Game Options"),
    ("GUI:UIOptions", "UI Options"),
    ("GUI:AudioOptions", "Audio Options"),
    ("GUI:VisualDetails", "Visual Details"),
    ("GUI:Difficulty", "Difficulty"),
    ("GUI:Tooltips", "Tooltips"),
    ("GUI:ScrollRate", "Scroll Rate"),
    ("GUI:TargetLines", "Target Lines"),
    ("GUI:ShowHidden", "See Hidden Objects"),
    ("GUI:MusicVolume", "Music Volume"),
    ("GUI:SoundVolume", "Sound Volume"),
    ("GUI:VoiceVolume", "Voice Volume"),
    ("GUI:Blank", ""),
    ("TXT_LOW", "Low"),
    ("TXT_HIGH", "High"),
    ("TXT_EASY", "Easy"),
    ("TXT_NORMAL", "Normal"),
    ("TXT_HARD", "Hard"),
    ("TXT_SLOWEST", "Slowest"),
    ("TXT_SLOWER", "Slower"),
    ("TXT_SLOW", "Slow"),
    ("TXT_MEDIUM", "Medium"),
    ("TXT_FAST", "Fast"),
    ("TXT_FASTER", "Faster"),
    ("TXT_FASTEST", "Fastest"),
];

/// VERA's own captions and status help on this page, as labels of VERA's
/// language files ([`crate::i18n`]): the retail string table has nothing for
/// full screen, zoom, the battle shape or the Red Alert 2 loading screen and
/// menu movie.
pub(crate) const ZOOM_LABEL: &str = "VERA:Zoom";
pub(crate) const ZOOM_HELP: &str = "VERA:ZoomHelp";
pub(crate) const SIDEBAR_LABEL: &str = "VERA:Sidebar";
pub(crate) const SIDEBAR_HELP: &str = "VERA:SidebarHelp";
pub(crate) const FULL_SCREEN_LABEL: &str = "VERA:FullScreen";
pub(crate) const FULL_SCREEN_HELP: &str = "VERA:FullScreenHelp";
pub(crate) const ORIGINAL_SHAPE_LABEL: &str = "VERA:OriginalShape";
pub(crate) const ORIGINAL_SHAPE_HELP: &str = "VERA:OriginalShapeHelp";
pub(crate) const YR_ORIGINAL_STYLE_LABEL: &str = "VERA:YrOriginalStyle";
pub(crate) const YR_ORIGINAL_STYLE_HELP: &str = "VERA:YrOriginalStyleHelp";
pub(crate) const MOUSE_WHEEL_ZOOM_LABEL: &str = "VERA:MouseWheelZoom";
pub(crate) const MOUSE_WHEEL_ZOOM_HELP: &str = "VERA:MouseWheelZoomHelp";
pub(crate) const RIGHT_CLICK_ORDERS_LABEL: &str = "VERA:RightClickOrders";
pub(crate) const RIGHT_CLICK_ORDERS_HELP: &str = "VERA:RightClickOrdersHelp";
pub(crate) const LOCK_CURSOR_LABEL: &str = "VERA:LockCursor";
pub(crate) const LOCK_CURSOR_HELP: &str = "VERA:LockCursorHelp";
pub(crate) const VERA_ICON_LABEL: &str = "VERA:VeraIcon";
pub(crate) const VERA_ICON_HELP: &str = "VERA:VeraIconHelp";
pub(crate) const HD_RADAR_LABEL: &str = "VERA:HdRadar";
pub(crate) const HD_RADAR_HELP: &str = "VERA:HdRadarHelp";
pub(crate) const SMOOTH_SIDEBAR_LABEL: &str = "VERA:SmoothSidebar";
pub(crate) const SMOOTH_SIDEBAR_HELP: &str = "VERA:SmoothSidebarHelp";
pub(crate) const SMOOTH_ONLINE_MENUS_LABEL: &str = "VERA:SmoothOnlineMenus";
pub(crate) const SMOOTH_ONLINE_MENUS_HELP: &str = "VERA:SmoothOnlineMenusHelp";
/// The Interface page's HD Map Previews (`app::map_picture`). The browser
/// build cannot start the hidden run that draws a map's picture.
pub(crate) const HAS_HD_MAP_PREVIEWS: bool =
    !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
pub(crate) const HD_MAP_PREVIEWS_LABEL: &str = "VERA:HdMapPreviews";
pub(crate) const HD_MAP_PREVIEWS_HELP: &str = "VERA:HdMapPreviewsHelp";
pub(crate) const SHIFT_QUEUE_LABEL: &str = "VERA:ShiftQueue";
pub(crate) const AUTO_REPAIR_LABEL: &str = "VERA:AutoRepair";
/// The two category pages retail's string table has no band name for.
pub(crate) const CONTROLS_LABEL: &str = "VERA:Controls";
pub(crate) const DEVELOPER_OPTIONS_LABEL: &str = "VERA:DeveloperOptions";
pub(crate) const SKIP_INTRO_LABEL: &str = "VERA:SkipIntro";
pub(crate) const SKIP_INTRO_HELP: &str = "VERA:SkipIntroHelp";
pub(crate) const PERFORMANCE_STATS_LABEL: &str = "VERA:PerformanceStats";
pub(crate) const PERFORMANCE_STATS_HELP: &str = "VERA:PerformanceStatsHelp";
/// VERA's developer box: arms Ctrl+Alt+K, which switches every label between
/// its text and its own string-table key, VERA's own `VERA:` labels included.
/// Saved in `[VERA] ShowStringKeys=`; a start shows the texts.
pub(crate) const SHOW_STRING_KEYS_LABEL: &str = "VERA:ShowStringKeys";
pub(crate) const SHOW_STRING_KEYS_HELP: &str = "VERA:ShowStringKeysHelp";
/// VERA's end-game switches, the pair of `[EndGameScreens]` presentation
/// stages. The verdict is what the match shows where it is decided; the score
/// page is the dialog after it. Each stage owns its picture *and* its music, so
/// a player can keep one stage and drop the other.
pub(crate) const CUSTOM_END_GAME_BANNER_LABEL: &str = "VERA:CustomEndGameBanner";
pub(crate) const CUSTOM_END_GAME_BANNER_HELP: &str = "VERA:CustomEndGameBannerHelp";
pub(crate) const CUSTOM_END_GAME_SCORE_LABEL: &str = "VERA:CustomEndGameScore";
pub(crate) const CUSTOM_END_GAME_SCORE_HELP: &str = "VERA:CustomEndGameScoreHelp";
pub(crate) const CUSTOM_END_GAME_MOVIE_LABEL: &str = "VERA:CustomEndGameMovie";
pub(crate) const CUSTOM_END_GAME_MOVIE_HELP: &str = "VERA:CustomEndGameMovieHelp";
/// VERA's accessibility box: retail's string table has nothing for it.
pub(crate) const DETAILED_EVA_TEXT_LABEL: &str = "VERA:DetailedEvaText";
pub(crate) const DETAILED_EVA_TEXT_HELP: &str = "VERA:DetailedEvaTextHelp";
pub(crate) const SHIFT_QUEUE_HELP: &str = "VERA:ShiftQueueHelp";
pub(crate) const AUTO_REPAIR_HELP: &str = "VERA:AutoRepairHelp";
/// VERA's terrain deformation box (`docs/terrain-deformation.md`).
pub(crate) const TERRAIN_DEFORMATION_LABEL: &str = "VERA:TerrainDeformation";
pub(crate) const TERRAIN_DEFORMATION_HELP: &str = "VERA:TerrainDeformationHelp";
pub(crate) const DISPLAY_LABEL: &str = "VERA:Display";
pub(crate) const DISPLAY_HELP: &str = "VERA:DisplayHelp";
/// VERA's health-bar list. The retail string table has nothing for it.
pub(crate) const HEALTH_BARS_LABEL: &str = "VERA:HealthBars";
pub(crate) const HEALTH_BARS_HELP: &str = "VERA:HealthBarsHelp";
/// VERA's strength slider for the `[Colors]` team-colour bands. Retail's
/// string table has nothing for it.
pub(crate) const HOUSE_COLOR_STRENGTH_LABEL: &str = "VERA:HouseColorStrength";
pub(crate) const HOUSE_COLOR_STRENGTH_HELP: &str = "VERA:HouseColorStrengthHelp";
/// VERA's whole-viewport colour-vision filter. Retail's string table has
/// nothing for it.
pub(crate) const COLOR_VISION_LABEL: &str = "VERA:ColorVision";
pub(crate) const COLOR_VISION_HELP: &str = "VERA:ColorVisionHelp";
/// The Display list's first row: the window system's own choice.
pub(crate) const DISPLAY_AUTO_LABEL: &str = "VERA:DisplayAuto";
/// VERA's Language list on the Interface page. Its rows are each language's
/// own name for itself, so they need no translation.
pub(crate) const LANGUAGE_LABEL: &str = "VERA:Language";
pub(crate) const LANGUAGE_HELP: &str = "VERA:LanguageHelp";
/// The Developer page's Check for Updates: the desktop updater
/// (`app::updater`) checks once the dialog has slid out. The browser build
/// has no updater.
pub(crate) const HAS_CHECK_FOR_UPDATES: bool =
    !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
pub(crate) const CHECK_FOR_UPDATES_LABEL: &str = "VERA:CheckForUpdates";
pub(crate) const CHECK_FOR_UPDATES_HELP: &str = "VERA:CheckForUpdatesHelp";
/// The Esc menu's Set Window Size button. A browser tab's canvas takes its size
/// from the page and an Android window fills the screen, so those builds have
/// none.
pub(crate) const HAS_SET_WINDOW_SIZE: bool = !cfg!(any(
    all(target_arch = "wasm32", target_os = "unknown"),
    target_os = "android"
));
pub(crate) const SET_WINDOW_SIZE_LABEL: &str = "VERA:SetWindowSize";
pub(crate) const SET_WINDOW_SIZE_HELP: &str = "VERA:SetWindowSizeHelp";

/// A button of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherButton {
    /// Keyboard or Main Menu: one of gamemd's parent exits.
    Exit(LauncherParentResult),
    /// A category in the right-hand column: shows that category's page.
    Category(Category),
    /// Check for Updates on the Developer page.
    CheckForUpdates,
    /// The Esc menu's Set Window Size: the window goes back to how a launch opens it.
    SetWindowSize,
    /// The in-game menu's Keyboard, which opens gamemd's in-game `0xA3`.
    InGameKeyboard,
    /// The in-game menu's Sound, which opens gamemd's Sound Controls (`0xB8`)
    /// and its song list.
    InGameSound,
}

impl LauncherButton {
    /// Whether it sits among the page's own controls, rather than being one
    /// of the right-hand column's.
    pub(crate) const fn on_page(self) -> bool {
        matches!(
            self,
            Self::CheckForUpdates | Self::SetWindowSize | Self::InGameKeyboard | Self::InGameSound
        )
    }
}

/// VERA's battle zoom: the Options combo that took the place of gamemd's
/// resolution list (`0x005601A0..0x00560270`). The window keeps the size the
/// player gives it, and a zoomed match draws its whole frame (battlefield,
/// sidebar, cursor) at the window size divided by the zoom, then scales it up.
/// A whole multiple of 100% scales by whole pixels and stays sharp; the steps
/// between blend neighbouring pixels. The menus fit the window on their own.
/// Saved as `[VERA] Zoom` in VERA.INI, a section gamemd never reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Zoom {
    /// The largest sharp zoom that still shows the original 800x600 view.
    #[default]
    Auto,
    /// A fixed zoom in percent: a multiple of [`Zoom::STEP`], at least 100.
    Percent(u32),
}

impl Zoom {
    /// The gap between the fixed choices.
    pub(crate) const STEP: u32 = 50;

    /// The combo's rows for a window of `width` x `height`, top to bottom:
    /// Auto, then every step from 100% to the largest this window allows.
    pub(crate) fn choices(width: u32, height: u32) -> Vec<Self> {
        std::iter::once(Self::Auto)
            .chain(
                (100..=Self::largest_percent(width, height))
                    .step_by(Self::STEP as usize)
                    .map(Self::Percent),
            )
            .collect()
    }

    /// The largest step that still shows gamemd's smallest mode, 640x480.
    fn largest_percent(width: u32, height: u32) -> u32 {
        let fits = (width * 100 / 640).min(height * 100 / 480);
        (fits / Self::STEP * Self::STEP).max(100)
    }

    /// The zoom in effect for a window of `width` x `height`, in percent. A
    /// fixed choice gives way only where it would cut the view below 640x480.
    pub(crate) fn percent(self, width: u32, height: u32) -> u32 {
        match self {
            Self::Auto => (width / 800).min(height / 600).max(1) * 100,
            Self::Percent(percent) => percent.clamp(100, Self::largest_percent(width, height)),
        }
    }

    /// The `[VERA] Zoom` value: 0 for Auto, otherwise the percent.
    pub(crate) const fn ini_value(self) -> i32 {
        match self {
            Self::Auto => 0,
            Self::Percent(percent) => percent as i32,
        }
    }

    /// 1 to 3 are the whole factors the first release saved. Any other value
    /// this build doesn't know reads as Auto.
    pub(crate) fn from_ini_value(value: i32) -> Self {
        match u32::try_from(value) {
            Ok(factor @ 1..=3) => Self::Percent(factor * 100),
            Ok(percent @ 100..=10_000) if percent % Self::STEP == 0 => Self::Percent(percent),
            _ => Self::Auto,
        }
    }
}

/// VERA's sidebar size: how big a battle draws its sidebar, on top of the
/// zoom; the battlefield takes the rest of the frame. Like the zoom, a size
/// never leaves less than gamemd's smallest mode: it needs a battle frame of
/// at least 640x480 at that size. Saved as `[VERA] Sidebar`, the percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) enum SidebarSize {
    #[default]
    Normal,
    Large,
    ExtraLarge,
}

impl SidebarSize {
    /// The combo's rows, top to bottom, where the frame has room for them.
    const ALL: [Self; 3] = [Self::Normal, Self::Large, Self::ExtraLarge];

    /// The row's text.
    pub(crate) fn label(self) -> String {
        crate::i18n::tr(match self {
            Self::Normal => "VERA:SidebarNormal",
            Self::Large => "VERA:SidebarLarge",
            Self::ExtraLarge => "VERA:SidebarExtraLarge",
        })
    }

    /// Frame pixels per 100 sidebar pixels.
    const fn requested_percent(self) -> u32 {
        match self {
            Self::Normal => 100,
            Self::Large => 150,
            Self::ExtraLarge => 200,
        }
    }

    fn fits(self, frame_width: u32, frame_height: u32) -> bool {
        let percent = self.requested_percent();
        percent == 100
            || (frame_width * 100 >= 640 * percent && frame_height * 100 >= 480 * percent)
    }

    /// The sizes a battle frame of `frame_width` x `frame_height` has room for.
    pub(crate) fn choices(frame_width: u32, frame_height: u32) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|size| size.fits(frame_width, frame_height))
            .collect()
    }

    /// The size in effect: one the frame has no room for gives way to the
    /// largest that fits.
    pub(crate) fn in_effect(self, frame_width: u32, frame_height: u32) -> Self {
        Self::ALL
            .into_iter()
            .rev()
            .find(|size| *size <= self && size.fits(frame_width, frame_height))
            .unwrap_or(Self::Normal)
    }

    /// The sidebar's scale in effect, in percent.
    pub(crate) fn percent(self, frame_width: u32, frame_height: u32) -> u32 {
        self.in_effect(frame_width, frame_height)
            .requested_percent()
    }

    /// The `[VERA] Sidebar` value: the percent.
    pub(crate) const fn ini_value(self) -> i32 {
        self.requested_percent() as i32
    }

    /// Any value this build doesn't know reads as Normal.
    pub(crate) const fn from_ini_value(value: i32) -> Self {
        match value {
            150 => Self::Large,
            200 => Self::ExtraLarge,
            _ => Self::Normal,
        }
    }
}

/// VERA's `[VERA] HealthBars=` health-strip policy: which objects draw a
/// health strip.
///
/// gamemd draws a strip for the selected object and a bracket only when it is
/// selected, so [`Self::Vanilla`] is the native rule. The other arms are VERA
/// presentation extensions: they widen the same strip to the objects the local
/// player can see. Nothing here reaches simulation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HealthBarMode {
    /// gamemd: selected objects draw the strip and bracket, the hovered object
    /// draws a bare strip, everything else draws nothing.
    #[default]
    Vanilla,
    /// [`Self::Vanilla`], plus a bare strip on every other object the local
    /// player can see on a revealed cell. Neutral/Special passive houses are
    /// scenery and get no automatic strip.
    Always,
    /// [`Self::Vanilla`], plus a bare strip on every other object the local
    /// player can see on a revealed cell that is below full health. Same
    /// passive-house exclusion as [`Self::Always`].
    AlwaysExceptFull,
}

impl HealthBarMode {
    /// The list's rows, top to bottom; the first is the default.
    const ALL: [Self; 3] = [Self::Vanilla, Self::Always, Self::AlwaysExceptFull];

    /// The row's text.
    pub(crate) fn label(self) -> String {
        crate::i18n::tr(match self {
            Self::Vanilla => "VERA:HealthBarsOriginal",
            Self::Always => "VERA:HealthBarsAll",
            Self::AlwaysExceptFull => "VERA:HealthBarsNotFull",
        })
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The `[VERA] HealthBars` value.
    pub(crate) const fn ini_value(self) -> i32 {
        match self {
            Self::Vanilla => 0,
            Self::Always => 1,
            Self::AlwaysExceptFull => 2,
        }
    }

    /// A value this build does not know reads as the native rule, so a
    /// hand-edited INI never hides the original strip.
    pub(crate) const fn from_ini_value(value: i32) -> Self {
        match value {
            1 => Self::Always,
            2 => Self::AlwaysExceptFull,
            _ => Self::Vanilla,
        }
    }
}

/// VERA's `[VERA] ColorVisionMode=` whole-viewport colour-vision filter.
///
/// gamemd has no equivalent: this is a presentation-only final-frame pass
/// (`render::color_vision`), never simulation state. `Generic` and
/// `Deuteranopia` are the same correction matrix; the former is just a label
/// for players who cannot name their deficiency. The matrices themselves live
/// in `render/`, so `ui/` owns only the enum and its strings and the app frame
/// maps a mode to a matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ColorVisionMode {
    /// The retail image; the pass is skipped entirely.
    #[default]
    Off,
    /// "Not sure? Use this": the common red-green correction.
    Generic,
    /// Red-green, green-cone.
    Deuteranopia,
    /// Red-green, red-cone.
    Protanopia,
    /// Blue-yellow; the rarest and least reliable model.
    Tritanopia,
}

impl ColorVisionMode {
    /// The list's rows, top to bottom; the first is the default.
    const ALL: [Self; 5] = [
        Self::Off,
        Self::Generic,
        Self::Deuteranopia,
        Self::Protanopia,
        Self::Tritanopia,
    ];

    /// The row's text.
    pub(crate) fn label(self) -> String {
        crate::i18n::tr(match self {
            Self::Off => "VERA:ColorVisionOff",
            Self::Generic => "VERA:ColorVisionGeneric",
            Self::Deuteranopia => "VERA:ColorVisionDeuteranopia",
            Self::Protanopia => "VERA:ColorVisionProtanopia",
            Self::Tritanopia => "VERA:ColorVisionTritanopia",
        })
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The `[VERA] ColorVisionMode` value.
    pub(crate) const fn ini_value(self) -> i32 {
        match self {
            Self::Off => 0,
            Self::Generic => 1,
            Self::Deuteranopia => 2,
            Self::Protanopia => 3,
            Self::Tritanopia => 4,
        }
    }

    /// A value this build does not know reads as `Off`, so a hand-edited INI
    /// never enables a filter the build has no matrix for.
    pub(crate) const fn from_ini_value(value: i32) -> Self {
        match value {
            1 => Self::Generic,
            2 => Self::Deuteranopia,
            3 => Self::Protanopia,
            4 => Self::Tritanopia,
            _ => Self::Off,
        }
    }
}

/// One screen the Display list offers, as the window layer reported it when the
/// page opened. VERA's own data: no profile or native code names a monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayChoice {
    /// The window system's own name, `[VERA] Display=`'s value. Windows answers
    /// `\\.\DISPLAY1`; the list shows the part after the last backslash.
    pub(crate) id: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The mode's refresh rate, when the window system reports one.
    pub(crate) refresh_millihertz: Option<u32>,
}

impl DisplayChoice {
    /// The row's text for the `number`th screen of the list (1-based),
    /// `Screen 1: 1920x1080 60Hz`. The window system's own name is not shown:
    /// macOS answers `Monitor #<model number>`, which means nothing to a
    /// player and is the same for two identical monitors. A screen that
    /// reports no refresh rate leaves it out instead of guessing one.
    pub(crate) fn label(&self, number: usize) -> String {
        let (width, height) = (&self.width, &self.height);
        match self.refresh_millihertz {
            // A rate the backend does not know, and the zero some virtual
            // adapters report for a mode they do not drive, are both left out
            // rather than shown as a refresh rate of zero.
            Some(millihertz) if millihertz > 0 => crate::i18n::tr_fill(
                "VERA:DisplayScreenRate",
                &[
                    ("number", &number),
                    ("width", width),
                    ("height", height),
                    ("rate", &(millihertz / 1000)),
                ],
            ),
            _ => crate::i18n::tr_fill(
                "VERA:DisplayScreen",
                &[("number", &number), ("width", width), ("height", height)],
            ),
        }
    }
}

/// Which screen the game opens on: the player's pick, or the window system's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum DisplayTarget {
    /// `[VERA] Display=` absent or empty: no screen is named and the window
    /// is never moved, so it opens wherever the window system puts it, as in
    /// every build before this option.
    #[default]
    Auto,
    /// A [`DisplayChoice::id`]. A screen that is gone at the next launch leaves
    /// the id here and opens on the system's choice, like the deleted `Zoom`
    /// steps below.
    Fixed(String),
}

impl DisplayTarget {
    /// The `[VERA] Display=` value: empty for Auto, otherwise the id.
    pub(crate) fn ini_value(&self) -> &str {
        match self {
            Self::Auto => "",
            Self::Fixed(id) => id,
        }
    }
    /// An empty or absent key reads as Auto; any other value names a screen,
    /// whether or not it is attached now.
    pub(crate) fn from_ini_value(value: &str) -> Self {
        if value.is_empty() {
            Self::Auto
        } else {
            Self::Fixed(value.to_owned())
        }
    }
}

/// The page's lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherComboId {
    Zoom,
    Sidebar,
    Display,
    /// The `[VERA] HealthBars=` list.
    HealthBars,
    /// The `[VERA] ColorVisionMode=` list: the whole-viewport filter.
    ColorVision,
    /// The `[VERA] Language=` list: VERA's interface language.
    Language,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLabels {
    pub(crate) options: String,
    pub(crate) main_menu: String,
    pub(crate) keyboard: String,
    pub(crate) display_options: String,
    pub(crate) game_options: String,
    pub(crate) ui_options: String,
    pub(crate) audio_options: String,
    pub(crate) visual_details: String,
    pub(crate) difficulty: String,
    pub(crate) tooltips: String,
    pub(crate) scroll_rate: String,
    pub(crate) target_lines: String,
    pub(crate) show_hidden: String,
    pub(crate) music_volume: String,
    pub(crate) sound_volume: String,
    pub(crate) voice_volume: String,
    pub(crate) blank: String,
    low: String,
    high: String,
    easy: String,
    normal: String,
    hard: String,
    scroll_tokens: [String; 7],
}

impl LauncherOptionsLabels {
    pub(crate) fn resolve(csf: &dyn Fn(&str) -> Option<String>) -> Self {
        let label = |index: usize| {
            let (key, fallback) = LAUNCHER_LABEL_SPECS[index];
            csf(key).unwrap_or_else(|| fallback.to_string())
        };
        Self {
            options: label(0),
            main_menu: label(1),
            keyboard: label(2),
            display_options: label(3),
            game_options: label(4),
            ui_options: label(5),
            audio_options: label(6),
            visual_details: label(7),
            difficulty: label(8),
            tooltips: label(9),
            scroll_rate: label(10),
            target_lines: label(11),
            show_hidden: label(12),
            music_volume: label(13),
            sound_volume: label(14),
            voice_volume: label(15),
            blank: label(16),
            low: label(17),
            high: label(18),
            easy: label(19),
            normal: label(20),
            hard: label(21),
            scroll_tokens: [
                label(22),
                label(23),
                label(24),
                label(25),
                label(26),
                label(27),
                label(28),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherTrackbarId {
    Detail,
    Difficulty,
    Scroll,
    Score,
    Sound,
    Voice,
    /// VERA's own slider: how strongly the `[Colors]` team-colour bands are
    /// boosted, on the [`crate::rules::house_colors`] scale. Its position is
    /// the level minus [`crate::rules::house_colors::STRENGTH_MIN`], so the
    /// retail strength is position 0 and the strongest is position 9.
    HouseColorStrength,
    /// The in-game menu's Game Speed, the slider of gamemd's Game Controls
    /// (`0xBBB` control `0x529`). No main-menu page has it and the profile
    /// pack leaves it out: the menu sets it up with
    /// [`OptionsDialogState::set_in_game_controls`] and applies it on close.
    GameSpeed,
}

impl LauncherTrackbarId {
    const fn maximum(self) -> u8 {
        match self {
            Self::Detail => 1,
            Self::Difficulty => 2,
            Self::Scroll => 6,
            Self::GameSpeed => crate::ui::shell::game_speed::MAX_POSITION as u8,
            Self::Score | Self::Sound | Self::Voice => 10,
            Self::HouseColorStrength => {
                crate::rules::house_colors::STRENGTH_MAX - crate::rules::house_colors::STRENGTH_MIN
            }
        }
    }

    /// The value plaque's reserve; Detail, Difficulty and Scroll turn the
    /// plaque off (`0x4AC` with 0).
    pub(crate) const fn plaque_reserve(self) -> i32 {
        match self {
            Self::Detail | Self::Difficulty | Self::Scroll | Self::GameSpeed => 0,
            Self::Score | Self::Sound | Self::Voice | Self::HouseColorStrength => 50,
        }
    }

    const fn is_audio(self) -> bool {
        matches!(self, Self::Score | Self::Sound | Self::Voice)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherCheckboxId {
    Tooltips,
    TargetLines,
    ShowHidden,
    /// VERA's own box: the window fills the screen. It changes the window at
    /// once instead of waiting for the parent's accept.
    FullScreen,
    /// VERA's own box: battles keep gamemd's 4:3 screen shape instead of the
    /// window's. Like Full Screen, it applies at once.
    OriginalShape,
    /// VERA's own box: the startup splash, the menu movie, the offline
    /// Skirmish background and the LAN/WOL lobby backgrounds use Yuri's
    /// Revenge's art instead of Red Alert 2's. The movie and the shell atlas
    /// change at once; the splash, drawn before any menu, at the next launch.
    ///
    /// User-set scope: this box switches only those surfaces. Every other
    /// shell screen follows gamemd's ordinary asset lookup (see
    /// `render::skirmish_shell_chrome`), whatever the box says. Widening it
    /// needs the user's explicit request.
    YrOriginalStyle,
    /// VERA's own box: Shift + left click on a mobile build cameo queues
    /// several copies instead of gamemd's one. It applies at once.
    ShiftQueue,
    /// VERA's own box: the player's damaged buildings are sent the Repair
    /// order by themselves (`app::input::auto_repair`). It applies at once.
    AutoRepair,
    /// VERA's own box: explosions dent open ground as in Tiberian Sun and a
    /// nuke leaves a crater. Saved; the next game's rules read it, so a match
    /// already running keeps its own.
    TerrainDeformation,
    /// VERA's local camera option: the wheel changes battle view distance.
    MouseWheelZoom,
    /// VERA-only alternative controls: left selects, right commands.
    RightClickOrders,
    /// VERA's cursor lock: the pointer stays inside the game window. It
    /// applies at once and is saved.
    LockCursor,
    /// VERA's own box: the window, taskbar and Dock show VERA's artwork
    /// instead of Red Alert 2's red star (`app::window_icon`). It applies at
    /// once and is saved; the installed program's own icon stays the red star.
    VeraIcon,
    /// VERA's own box: when the sidebar draws at a whole multiple of its
    /// pixels, the radar draws at that multiple too (`render::radar::hd`).
    /// Saved; a battle follows it at once.
    HdRadar,
    /// VERA's own box: when the sidebar draws at a whole multiple of its
    /// pixels, its cameos and frame pieces draw from lightly smoothed xBR copies
    /// at that multiple (`render::sidebar_smoothing`). Saved; a battle
    /// follows it at once.
    SmoothSidebar,
    /// VERA's own box: the online client's enlarged art draws lightly
    /// smoothed (`BatchRenderer::draw_chrome`). Saved; the next frame follows
    /// it.
    SmoothOnlineMenus,
    /// VERA's own box: the setup screens' map preview shows VERA's own
    /// drawing of the whole map (`app::map_picture`) instead of the
    /// map's `[PreviewPack]` picture. On by default; saved, and the next
    /// preview follows it.
    HdMapPreviews,
    /// VERA's developer box: the window's FPS readout. Like Full Screen it
    /// shows and changes live state at once, and is never saved: the readout
    /// starts hidden at every launch.
    PerformanceStats,
    /// VERA's developer box: the next launch skips `Init_Game`'s logo movie
    /// (`0x0052C5E2`); the splash and the first-run intro still play.
    SkipIntro,
    /// VERA's developer box: arms Ctrl+Alt+K, which switches every label
    /// between its text and its own string-table key. Saved; a start shows
    /// the texts until the hotkey asks for keys.
    ShowStringKeys,
    /// VERA's box: the score page draws the country's own victory or defeat
    /// picture from `[EndGameScreens]` instead of the side's `MP*SCRN` art. On
    /// by default — a rules set that names pictures shows them — and it covers
    /// the picture only. The next finished match reads it, so it needs no live
    /// apply.
    CustomEndGameBanner,
    /// VERA's box: the score page starts the country's own
    /// `VictoryMusic=`/`DefeatMusic=` theme instead of the side's. Also read
    /// once per finished match.
    CustomEndGameScore,
    /// VERA's box: a finished match waits for a beaten computer commander's
    /// losing clip before the score page opens and the battle is torn down.
    /// Read when the match ends, so it needs no live apply either.
    CustomEndGameMovie,
    /// VERA-only accessibility box: detailed EVA text rows, default off.
    DetailedEvaText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherCue {
    MainButton,
    GenericClick,
    Checkbox,
    ComboOpen,
    ComboClose,
}

impl From<crate::ui::shell::combobox::ComboCue> for LauncherCue {
    fn from(cue: crate::ui::shell::combobox::ComboCue) -> Self {
        match cue {
            crate::ui::shell::combobox::ComboCue::Open => Self::ComboOpen,
            crate::ui::shell::combobox::ComboCue::Close => Self::ComboClose,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// gamemd-derived: notification order in launcher primary-proc slice
/// `0x0055FDB0..0x0056047A`; owner `OptionsClass__ShowLauncherDialog @
/// 0x0055FC80` consumes these before any parent-result teardown.
pub(crate) enum LauncherOptionsEvent {
    Cue(LauncherCue),
    ZoomSelected(Zoom),
    SidebarSelected(SidebarSize),
    /// The Health Bars list's pick, applied at once like Sidebar.
    HealthBarModeSelected(HealthBarMode),
    /// The Colour Vision list's pick, applied to the very next frame.
    ColorVisionModeSelected(ColorVisionMode),
    /// The Display list's pick, applied to the window at once.
    DisplaySelected(DisplayTarget),
    /// The Language list's pick, a language tag, applied at once.
    LanguageSelected(String),
    FullScreenChanged(bool),
    /// The Esc menu's Set Window Size button, applied to the window at once.
    SetWindowSizeRequested,
    OriginalShapeChanged(bool),
    YrOriginalStyleChanged(bool),
    ShiftQueueChanged(bool),
    AutoRepairChanged(bool),
    TerrainDeformationChanged(bool),
    MouseWheelZoomChanged(bool),
    RightClickOrdersChanged(bool),
    LockCursorChanged(bool),
    VeraIconChanged(bool),
    HdRadarChanged(bool),
    SmoothSidebarChanged(bool),
    SmoothOnlineMenusChanged(bool),
    HdMapPreviewsChanged(bool),
    PerformanceStatsChanged(bool),
    ShowStringKeysChanged(bool),
    SkipIntroChanged(bool),
    CustomEndGameBannerChanged(bool),
    CustomEndGameScoreChanged(bool),
    CustomEndGameMovieChanged(bool),
    DetailedEvaTextChanged(bool),
    /// VERA's team-colour strength, as the level the profile stores.
    HouseColorStrengthChanged(u8),
    ScorePreview(f32),
    SoundPreview(f32),
    VoicePreview(f32),
    /// The in-game menu's Keyboard and Sound buttons, which that menu routes
    /// (`app::input::pause_menu`).
    InGameKeyboardRequested,
    InGameSoundRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// gamemd-derived: the distinct exits owned by
/// `OptionsClass__ShowLauncherDialog @ 0x0055FC80`; every accepted result first
/// projects through `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0`.
pub(crate) enum LauncherParentResult {
    Back,
    Keyboard,
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LauncherOptionsFrameOutput {
    pub(crate) events: Vec<LauncherOptionsEvent>,
    pub(crate) result: Option<LauncherParentResult>,
    /// Check for Updates: the dialog slides out, then the updater checks.
    pub(crate) check_for_updates: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherOptionsValues {
    pub(crate) detail_position: u8,
    pub(crate) difficulty_position: u8,
    pub(crate) scroll_position: u8,
    pub(crate) tooltips: bool,
    pub(crate) target_lines: bool,
    pub(crate) show_hidden: bool,
    /// The window's state when the page opened, then each toggle.
    pub(crate) full_screen: bool,
    pub(crate) original_shape: bool,
    pub(crate) yr_original_style: bool,
    pub(crate) mouse_wheel_zoom: bool,
    pub(crate) right_click_orders: bool,
    pub(crate) lock_cursor: bool,
    pub(crate) vera_icon: bool,
    pub(crate) hd_radar: bool,
    pub(crate) smooth_sidebar: bool,
    pub(crate) smooth_online_menus: bool,
    pub(crate) hd_map_previews: bool,
    pub(crate) shift_queue: bool,
    pub(crate) auto_repair: bool,
    pub(crate) terrain_deformation: bool,
    /// The FPS readout's live state when the page opened, then each toggle.
    pub(crate) performance_stats: bool,
    /// `[VERA] ShowStringKeys=`, the String Keys box, then each toggle.
    pub(crate) show_string_keys: bool,
    pub(crate) skip_intro: bool,
    /// The country's own end-game picture is allowed; see
    /// [`LauncherCheckboxId::CustomEndGameBanner`].
    pub(crate) custom_end_game_banner: bool,
    /// The country's own end-game music is allowed; see
    /// [`LauncherCheckboxId::CustomEndGameScore`].
    pub(crate) custom_end_game_score: bool,
    /// The exit waits for the closing commander clip; see
    /// [`LauncherCheckboxId::CustomEndGameMovie`].
    pub(crate) custom_end_game_movie: bool,
    /// The accessibility box; see [`LauncherCheckboxId::DetailedEvaText`].
    pub(crate) detailed_eva_text: bool,
    /// The Sidebar list's choice; like Zoom, it applies at once.
    pub(crate) sidebar: SidebarSize,
    /// The Health Bars list's choice; like Sidebar, it applies at once.
    pub(crate) health_bars: HealthBarMode,
    /// The Colour Vision list's choice; reads every frame, so it applies at
    /// once without rebuilding anything.
    pub(crate) color_vision: ColorVisionMode,
    /// VERA's team-colour strength as the slider's position,
    /// [`crate::rules::house_colors::STRENGTH_MIN`] less than the level the
    /// profile stores. Position 0 is gamemd's own bands.
    pub(crate) house_color_position: u8,
    pub(crate) score_position: u8,
    pub(crate) sound_position: u8,
    pub(crate) voice_position: u8,
}

impl Default for LauncherOptionsValues {
    fn default() -> Self {
        Self {
            detail_position: 1,
            difficulty_position: 1,
            scroll_position: 3,
            tooltips: true,
            target_lines: true,
            show_hidden: false,
            full_screen: false,
            original_shape: false,
            yr_original_style: false,
            mouse_wheel_zoom: false,
            right_click_orders: false,
            lock_cursor: false,
            vera_icon: false,
            hd_radar: false,
            smooth_sidebar: false,
            smooth_online_menus: false,
            hd_map_previews: true,
            shift_queue: false,
            auto_repair: false,
            terrain_deformation: false,
            performance_stats: false,
            show_string_keys: false,
            skip_intro: false,
            custom_end_game_banner: false,
            custom_end_game_score: false,
            custom_end_game_movie: false,
            detailed_eva_text: false,
            sidebar: SidebarSize::Normal,
            health_bars: HealthBarMode::Vanilla,
            color_vision: ColorVisionMode::Off,
            house_color_position: 0,
            score_position: 4,
            sound_position: 7,
            voice_position: 7,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LauncherOptionsPacked {
    pub(crate) detail_level: i32,
    pub(crate) difficulty: i32,
    pub(crate) unit_action_lines: bool,
    pub(crate) show_hidden: bool,
    pub(crate) tooltips: bool,
    pub(crate) scroll_rate: i32,
    pub(crate) score_volume: f32,
    pub(crate) sound_volume: f32,
    pub(crate) voice_volume: f32,
}

/// One integer-pixel input frame from the retail control rectangle used by
/// the shell renderer and input admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicalControlFrame {
    pub(crate) local_x: i32,
    pub(crate) local_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// gamemd-derived: the fresh launcher trackbars created by primary-proc slice
/// `0x0055FDB0..0x0056047A` reject an out-of-range set-position request and
/// retain constructor position zero rather than clamping it.
pub(crate) fn admitted_initial_position(requested: i64, maximum: u8) -> u8 {
    u8::try_from(requested)
        .ok()
        .filter(|value| *value <= maximum)
        .unwrap_or(0)
}

/// gamemd-derived: launcher volume setup in primary-proc slice
/// `0x0055FDB0..0x0056047A`, fed by setters `0x005FA4A0/0x005FA510/0x005FA590`,
/// forms the x87 request `trunc(volume * 10 + 0.5)` before range admission.
pub(crate) fn admitted_volume_position(volume: f32) -> u8 {
    let request = f64::from(volume) * 10.0 + 0.5;
    if !request.is_finite() {
        return 0;
    }
    admitted_initial_position(request.trunc() as i64, 10)
}

/// Detail caption by position: `TXT_LOW`, else `TXT_HIGH` (table
/// `0x0082A248` entries 0 and 2).
fn detail_caption(labels: &LauncherOptionsLabels, position: u8) -> String {
    if position == 0 {
        labels.low.clone()
    } else {
        labels.high.clone()
    }
}

/// Difficulty caption by position (table `0x0082A254`).
fn difficulty_caption(labels: &LauncherOptionsLabels, position: u8) -> String {
    match position {
        0 => labels.easy.clone(),
        1 => labels.normal.clone(),
        _ => labels.hard.clone(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OptionsDialogState {
    labels: LauncherOptionsLabels,
    values: LauncherOptionsValues,
    zoom: Zoom,
    /// The screens the Display list offers, as the window layer reported them
    /// when this dialog was built. VERA's own list; no profile stores it.
    displays: Vec<DisplayChoice>,
    /// The Display list's current pick.
    display: DisplayTarget,
    /// The languages the Language list offers, and the tag of the one in
    /// use. Set by [`Self::set_languages`]; empty until then.
    languages: Vec<crate::i18n::LanguageChoice>,
    language: String,
    /// The size this launch opened the window at, which the Esc menu's Set
    /// Window Size button names. Set by [`Self::set_launch_window_size`]; zero
    /// until then.
    launch_window_size: (u32, u32),
    /// The in-game menu's Game Speed slider: its position, and the caption
    /// and one value name per position. Set by [`Self::set_in_game_controls`].
    game_speed_position: u8,
    game_speed_caption: String,
    game_speed_names: Vec<String>,
    /// The in-game menu's Sound button caption. Set with the speed.
    in_game_sound_label: String,
    /// The list that is open, if any.
    open_combo: Option<LauncherComboId>,
    launcher_audio_available: bool,
    detail_caption: String,
    difficulty_caption: String,
    scroll_caption: String,
    /// The slider holding the mouse since a press on it.
    capture: Option<TrackbarHold<LauncherTrackbarId>>,
    pending_events: Vec<LauncherOptionsEvent>,
    pending_result: Option<LauncherParentResult>,
    /// The category whose page shows.
    category: Category,
    pending_update_check: bool,
    shell_interaction: shell::ShellInteraction,
}

impl Default for OptionsDialogState {
    fn default() -> Self {
        let labels = LauncherOptionsLabels::resolve(&|_| None);
        Self::new(
            labels,
            LauncherOptionsValues::default(),
            Zoom::Auto,
            false,
            Vec::new(),
            DisplayTarget::Auto,
        )
    }
}

impl OptionsDialogState {
    /// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80`
    /// creates each fresh `0xD5` parent, while primary-proc slice
    /// `0x0055FDB0..0x0056047A` admits initial positions and preserves the
    /// resource Detail/Difficulty/Scroll captions until a changed notification.
    /// `displays` and `display` are VERA's own Display list, which gamemd's
    /// `0xD5` has no control for.
    pub(crate) fn new(
        labels: LauncherOptionsLabels,
        mut values: LauncherOptionsValues,
        zoom: Zoom,
        launcher_audio_available: bool,
        displays: Vec<DisplayChoice>,
        display: DisplayTarget,
    ) -> Self {
        values.detail_position = admitted_initial_position(i64::from(values.detail_position), 1);
        values.difficulty_position =
            admitted_initial_position(i64::from(values.difficulty_position), 2);
        values.scroll_position = admitted_initial_position(i64::from(values.scroll_position), 6);
        values.score_position = admitted_initial_position(i64::from(values.score_position), 10);
        values.sound_position = admitted_initial_position(i64::from(values.sound_position), 10);
        values.voice_position = admitted_initial_position(i64::from(values.voice_position), 10);
        // VERA's own slider takes the top level for a position past its range:
        // there is no boost above it to ask for, and the profile never holds
        // one either.
        values.house_color_position = values
            .house_color_position
            .min(LauncherTrackbarId::HouseColorStrength.maximum());
        // The 0x497 init sets each trackbar's range and position; every change
        // sends WM_HSCROLL (0x0061E609..0x0061E6AF), whose handler
        // (0x0055FF68) names the position (tables 0x0082A248 / 0x0082A254 /
        // 0x0082A260), so the resource captions never show.
        let detail_caption = detail_caption(&labels, values.detail_position);
        let difficulty_caption = difficulty_caption(&labels, values.difficulty_position);
        let scroll_caption = labels.scroll_tokens[usize::from(values.scroll_position)].clone();
        Self {
            detail_caption,
            difficulty_caption,
            scroll_caption,
            labels,
            values,
            zoom,
            displays,
            display,
            languages: Vec::new(),
            language: String::new(),
            launch_window_size: (0, 0),
            game_speed_position: 0,
            game_speed_caption: String::new(),
            game_speed_names: Vec::new(),
            in_game_sound_label: String::new(),
            open_combo: None,
            launcher_audio_available,
            capture: None,
            pending_events: Vec::new(),
            pending_result: None,
            category: Category::ALL[0],
            pending_update_check: false,
            shell_interaction: shell::ShellInteraction::default(),
        }
    }

    /// What the Language list offers, and the tag of the language in use.
    pub(crate) fn set_languages(
        &mut self,
        languages: Vec<crate::i18n::LanguageChoice>,
        language: String,
    ) {
        self.languages = languages;
        self.language = language;
    }

    /// The size this launch opened the window at, width then height.
    pub(crate) fn set_launch_window_size(&mut self, size: (u32, u32)) {
        self.launch_window_size = size;
    }

    /// After a language change: the retail captions read from the string
    /// table again, and the slider steps they name with them.
    pub(crate) fn set_labels(&mut self, labels: LauncherOptionsLabels) {
        self.detail_caption = detail_caption(&labels, self.values.detail_position);
        self.difficulty_caption = difficulty_caption(&labels, self.values.difficulty_position);
        self.scroll_caption =
            labels.scroll_tokens[usize::from(self.values.scroll_position)].clone();
        self.labels = labels;
    }

    /// The screens the Display list offers, in the window layer's order. The
    /// shell's own drawing reads the field; only the tests need them back.
    #[cfg(test)]
    pub(crate) fn displays(&self) -> &[DisplayChoice] {
        &self.displays
    }

    /// Whether a trackbar window is enabled: `0x00560361..0x00560467` (as
    /// `0x6B6300` does for its own three) enables the audio ones only with an
    /// output device (`Audio__HasOutputDevice @ 0x00407000`).
    pub(crate) const fn trackbar_enabled(&self, id: LauncherTrackbarId) -> bool {
        !id.is_audio() || self.launcher_audio_available
    }

    /// Whether a button window is enabled: BBB's init disables its Sound
    /// button without an output device (`0x004E2201..0x004E2223`), and the
    /// in-game page's Sound button stands for it.
    pub(crate) const fn button_enabled(&self, id: LauncherButton) -> bool {
        !matches!(id, LauncherButton::InGameSound) || self.launcher_audio_available
    }

    /// gamemd-derived: the parent snapshot consumed by
    /// `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0` maps the six
    /// positions and three normalized checkboxes exactly as below.
    pub(crate) fn pack(&self) -> LauncherOptionsPacked {
        LauncherOptionsPacked {
            detail_level: if self.values.detail_position == 0 {
                0
            } else {
                2
            },
            difficulty: i32::from(self.values.difficulty_position),
            unit_action_lines: self.values.target_lines,
            show_hidden: self.values.show_hidden,
            tooltips: self.values.tooltips,
            scroll_rate: 6 - i32::from(self.values.scroll_position),
            score_volume: f32::from(self.values.score_position) * 0.1,
            sound_volume: f32::from(self.values.sound_position) * 0.1,
            voice_volume: f32::from(self.values.voice_position) * 0.1,
        }
    }

    /// Give the in-game menu's Game Speed slider its caption, the name of
    /// each position's speed and where it stands, and the Sound button its
    /// caption. A position past the slider's end shows as its first.
    pub(crate) fn set_in_game_controls(
        &mut self,
        speed_caption: String,
        speed_names: Vec<String>,
        speed_position: u8,
        sound_label: String,
    ) {
        self.game_speed_caption = speed_caption;
        self.game_speed_names = speed_names;
        self.game_speed_position = if speed_position > LauncherTrackbarId::GameSpeed.maximum() {
            0
        } else {
            speed_position
        };
        self.in_game_sound_label = sound_label;
    }

    pub(crate) fn trackbar_position(&self, id: LauncherTrackbarId) -> u8 {
        match id {
            LauncherTrackbarId::GameSpeed => self.game_speed_position,
            LauncherTrackbarId::Detail => self.values.detail_position,
            LauncherTrackbarId::Difficulty => self.values.difficulty_position,
            LauncherTrackbarId::Scroll => self.values.scroll_position,
            LauncherTrackbarId::Score => self.values.score_position,
            LauncherTrackbarId::Sound => self.values.sound_position,
            LauncherTrackbarId::Voice => self.values.voice_position,
            LauncherTrackbarId::HouseColorStrength => self.values.house_color_position,
        }
    }

    /// VERA's team-colour strength as the level the profile stores: the
    /// slider's position lifted onto the
    /// [`crate::rules::house_colors`] scale.
    pub(crate) const fn house_color_strength(&self) -> u8 {
        self.values.house_color_position + crate::rules::house_colors::STRENGTH_MIN
    }

    /// What a plaque trackbar's plaque reads. Retail's sliders show their
    /// position; VERA's strength shows the level that position stands for, so
    /// the player never sees a 0 for gamemd's own colours.
    pub(crate) fn trackbar_plaque_text(&self, id: LauncherTrackbarId) -> String {
        match id {
            LauncherTrackbarId::HouseColorStrength => self.house_color_strength().to_string(),
            _ => self.trackbar_position(id).to_string(),
        }
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` changes
    /// captions and queues preview/cue work only after the integer position
    /// actually changes; the accepted projection remains owned by `0x0055FAA0`.
    fn set_trackbar_position(&mut self, id: LauncherTrackbarId, position: u8) {
        if position > id.maximum() || !self.trackbar_enabled(id) {
            return;
        }
        let slot = match id {
            LauncherTrackbarId::GameSpeed => &mut self.game_speed_position,
            LauncherTrackbarId::Detail => &mut self.values.detail_position,
            LauncherTrackbarId::Difficulty => &mut self.values.difficulty_position,
            LauncherTrackbarId::Scroll => &mut self.values.scroll_position,
            LauncherTrackbarId::Score => &mut self.values.score_position,
            LauncherTrackbarId::Sound => &mut self.values.sound_position,
            LauncherTrackbarId::Voice => &mut self.values.voice_position,
            LauncherTrackbarId::HouseColorStrength => &mut self.values.house_color_position,
        };
        if *slot == position {
            return;
        }
        *slot = position;
        match id {
            LauncherTrackbarId::Detail => {
                self.detail_caption = detail_caption(&self.labels, position);
                self.pending_events
                    .push(LauncherOptionsEvent::Cue(LauncherCue::GenericClick));
            }
            LauncherTrackbarId::Difficulty => {
                self.difficulty_caption = difficulty_caption(&self.labels, position);
                self.pending_events
                    .push(LauncherOptionsEvent::Cue(LauncherCue::GenericClick));
            }
            LauncherTrackbarId::Scroll => {
                self.scroll_caption = self.labels.scroll_tokens[usize::from(position)].clone();
                self.pending_events
                    .push(LauncherOptionsEvent::Cue(LauncherCue::GenericClick));
            }
            // 61E609..61E6DD: a changed position plays the cue. The speed
            // itself applies when the menu closes.
            LauncherTrackbarId::GameSpeed => {
                self.pending_events
                    .push(LauncherOptionsEvent::Cue(LauncherCue::GenericClick));
            }
            LauncherTrackbarId::Score => self.pending_events.push(
                LauncherOptionsEvent::ScorePreview(f32::from(position) * 0.1),
            ),
            LauncherTrackbarId::Sound => self.pending_events.push(
                LauncherOptionsEvent::SoundPreview(f32::from(position) * 0.1),
            ),
            LauncherTrackbarId::Voice => self.pending_events.push(
                LauncherOptionsEvent::VoicePreview(f32::from(position) * 0.1),
            ),
            // A once-per-drag setting rather than a live preview: the battle
            // this applies to is loaded later, and the accept writes it out.
            // With no audible preview to audition, the retail notch click is
            // the drag's only feedback, as on the detail, difficulty and
            // scroll sliders.
            LauncherTrackbarId::HouseColorStrength => {
                self.pending_events
                    .push(LauncherOptionsEvent::Cue(LauncherCue::GenericClick));
                self.pending_events
                    .push(LauncherOptionsEvent::HouseColorStrengthChanged(
                        position + crate::rules::house_colors::STRENGTH_MIN,
                    ))
            }
        }
    }

    /// A press on one trackbar ([`trackbar_press`]).
    pub(crate) fn trackbar_mouse_down(
        &mut self,
        id: LauncherTrackbarId,
        frame: PhysicalControlFrame,
    ) {
        if !self.trackbar_enabled(id) {
            return;
        }
        let press = trackbar_press(
            i32::from(self.trackbar_position(id)),
            frame.local_x,
            frame.local_y,
            frame.width,
            frame.height,
            id.plaque_reserve(),
            i32::from(id.maximum()),
        );
        self.capture = press.hold(id);
        if let TrackbarPress::Jump(position) = press {
            // The position never exceeds the byte-sized maximum.
            self.set_trackbar_position(id, position as u8);
        }
    }

    pub(crate) fn trackbar_mouse_move(
        &mut self,
        id: LauncherTrackbarId,
        frame: PhysicalControlFrame,
    ) {
        if self.capture != Some(TrackbarHold { id, dragging: true }) {
            return;
        }
        let position = trackbar_position_from_x(
            frame.local_x,
            frame.width,
            id.plaque_reserve(),
            i32::from(id.maximum()),
        );
        self.set_trackbar_position(id, position as u8);
    }

    pub(crate) fn trackbar_mouse_up(&mut self, id: LauncherTrackbarId) {
        if self.capture.is_some_and(|hold| hold.id == id) {
            self.capture = None;
        }
    }

    /// gamemd-derived: checkbox owner `0x006163A0` admits only the unsigned
    /// 18x18 icon square, normalizes the toggle, then emits the checkbox cue.
    pub(crate) fn checkbox_mouse_down(
        &mut self,
        id: LauncherCheckboxId,
        frame: PhysicalControlFrame,
    ) {
        if !(0..18).contains(&frame.local_x) || !(0..18).contains(&frame.local_y) {
            return;
        }
        let slot = match id {
            LauncherCheckboxId::Tooltips => &mut self.values.tooltips,
            LauncherCheckboxId::TargetLines => &mut self.values.target_lines,
            LauncherCheckboxId::ShowHidden => &mut self.values.show_hidden,
            LauncherCheckboxId::FullScreen => &mut self.values.full_screen,
            LauncherCheckboxId::OriginalShape => &mut self.values.original_shape,
            LauncherCheckboxId::YrOriginalStyle => &mut self.values.yr_original_style,
            LauncherCheckboxId::ShiftQueue => &mut self.values.shift_queue,
            LauncherCheckboxId::AutoRepair => &mut self.values.auto_repair,
            LauncherCheckboxId::TerrainDeformation => &mut self.values.terrain_deformation,
            LauncherCheckboxId::MouseWheelZoom => &mut self.values.mouse_wheel_zoom,
            LauncherCheckboxId::RightClickOrders => &mut self.values.right_click_orders,
            LauncherCheckboxId::LockCursor => &mut self.values.lock_cursor,
            LauncherCheckboxId::VeraIcon => &mut self.values.vera_icon,
            LauncherCheckboxId::HdRadar => &mut self.values.hd_radar,
            LauncherCheckboxId::SmoothSidebar => &mut self.values.smooth_sidebar,
            LauncherCheckboxId::SmoothOnlineMenus => &mut self.values.smooth_online_menus,
            LauncherCheckboxId::HdMapPreviews => &mut self.values.hd_map_previews,
            LauncherCheckboxId::PerformanceStats => &mut self.values.performance_stats,
            LauncherCheckboxId::ShowStringKeys => &mut self.values.show_string_keys,
            LauncherCheckboxId::SkipIntro => &mut self.values.skip_intro,
            LauncherCheckboxId::CustomEndGameBanner => &mut self.values.custom_end_game_banner,
            LauncherCheckboxId::CustomEndGameScore => &mut self.values.custom_end_game_score,
            LauncherCheckboxId::CustomEndGameMovie => &mut self.values.custom_end_game_movie,
            LauncherCheckboxId::DetailedEvaText => &mut self.values.detailed_eva_text,
        };
        *slot = !*slot;
        let checked = *slot;
        self.pending_events
            .push(LauncherOptionsEvent::Cue(LauncherCue::Checkbox));
        match id {
            LauncherCheckboxId::FullScreen => self
                .pending_events
                .push(LauncherOptionsEvent::FullScreenChanged(checked)),
            LauncherCheckboxId::OriginalShape => self
                .pending_events
                .push(LauncherOptionsEvent::OriginalShapeChanged(checked)),
            LauncherCheckboxId::YrOriginalStyle => self
                .pending_events
                .push(LauncherOptionsEvent::YrOriginalStyleChanged(checked)),
            LauncherCheckboxId::ShiftQueue => self
                .pending_events
                .push(LauncherOptionsEvent::ShiftQueueChanged(checked)),
            LauncherCheckboxId::AutoRepair => self
                .pending_events
                .push(LauncherOptionsEvent::AutoRepairChanged(checked)),
            LauncherCheckboxId::TerrainDeformation => self
                .pending_events
                .push(LauncherOptionsEvent::TerrainDeformationChanged(checked)),
            LauncherCheckboxId::MouseWheelZoom => self
                .pending_events
                .push(LauncherOptionsEvent::MouseWheelZoomChanged(checked)),
            LauncherCheckboxId::RightClickOrders => self
                .pending_events
                .push(LauncherOptionsEvent::RightClickOrdersChanged(checked)),
            LauncherCheckboxId::LockCursor => self
                .pending_events
                .push(LauncherOptionsEvent::LockCursorChanged(checked)),
            LauncherCheckboxId::VeraIcon => self
                .pending_events
                .push(LauncherOptionsEvent::VeraIconChanged(checked)),
            LauncherCheckboxId::HdRadar => self
                .pending_events
                .push(LauncherOptionsEvent::HdRadarChanged(checked)),
            LauncherCheckboxId::SmoothSidebar => self
                .pending_events
                .push(LauncherOptionsEvent::SmoothSidebarChanged(checked)),
            LauncherCheckboxId::SmoothOnlineMenus => self
                .pending_events
                .push(LauncherOptionsEvent::SmoothOnlineMenusChanged(checked)),
            LauncherCheckboxId::HdMapPreviews => self
                .pending_events
                .push(LauncherOptionsEvent::HdMapPreviewsChanged(checked)),
            LauncherCheckboxId::PerformanceStats => self
                .pending_events
                .push(LauncherOptionsEvent::PerformanceStatsChanged(checked)),
            LauncherCheckboxId::ShowStringKeys => self
                .pending_events
                .push(LauncherOptionsEvent::ShowStringKeysChanged(checked)),
            LauncherCheckboxId::SkipIntro => self
                .pending_events
                .push(LauncherOptionsEvent::SkipIntroChanged(checked)),
            LauncherCheckboxId::CustomEndGameBanner => self
                .pending_events
                .push(LauncherOptionsEvent::CustomEndGameBannerChanged(checked)),
            LauncherCheckboxId::CustomEndGameScore => self
                .pending_events
                .push(LauncherOptionsEvent::CustomEndGameScoreChanged(checked)),
            LauncherCheckboxId::CustomEndGameMovie => self
                .pending_events
                .push(LauncherOptionsEvent::CustomEndGameMovieChanged(checked)),
            LauncherCheckboxId::DetailedEvaText => self
                .pending_events
                .push(LauncherOptionsEvent::DetailedEvaTextChanged(checked)),
            LauncherCheckboxId::Tooltips
            | LauncherCheckboxId::TargetLines
            | LauncherCheckboxId::ShowHidden => {}
        }
    }

    /// Follow a full-screen change the page didn't make (the window's own
    /// title-bar button), so the box shows what the window is.
    pub(crate) fn sync_full_screen(&mut self, full_screen: bool) {
        self.values.full_screen = full_screen;
    }

    /// Follow Ctrl+Alt+F while the page is open, so the box shows the readout.
    pub(crate) fn sync_performance_stats(&mut self, shown: bool) {
        self.values.performance_stats = shown;
    }

    /// Like gamemd's combo owner `0x00617250`, a press emits GUIComboOpen
    /// first. gamemd then opens the list only from the 20-pixel arrow
    /// (`x > client_width - 20`); VERA's own lists open from anywhere on them.
    pub(crate) fn combo_mouse_down(&mut self, combo: LauncherComboId, frame: PhysicalControlFrame) {
        if frame.local_x < 0
            || frame.local_x >= frame.width
            || frame.local_y < 0
            || frame.local_y >= frame.height
        {
            return;
        }
        self.pending_events
            .push(LauncherOptionsEvent::Cue(LauncherCue::ComboOpen));
        self.open_combo = (self.open_combo != Some(combo)).then_some(combo);
    }

    /// gamemd-derived: ordinary `0xD5` owner-draw buttons emit
    /// `[AudioVisual] GUIMainButtonSound` on admitted primary mouse-down;
    /// `OptionsClass__ShowLauncherDialog @ 0x0055FC80` observes the distinct
    /// Back/Keyboard result only on the later activation release.
    pub(crate) fn main_button_mouse_down(&mut self) {
        self.pending_events
            .push(LauncherOptionsEvent::Cue(LauncherCue::MainButton));
    }

    /// Like gamemd's resolution row (primary-proc slice
    /// `0x0055FDB0..0x0056047A`), a picked zoom row is published at once; the
    /// accepted projection `0x0055FAA0` never owns it.
    pub(crate) fn select_zoom(&mut self, zoom: Zoom) {
        self.zoom = zoom;
        self.open_combo = None;
        self.pending_events
            .push(LauncherOptionsEvent::ZoomSelected(zoom));
    }

    /// The Sidebar list publishes its pick at once, like Zoom.
    pub(crate) fn select_sidebar(&mut self, size: SidebarSize) {
        self.values.sidebar = size;
        self.open_combo = None;
        self.pending_events
            .push(LauncherOptionsEvent::SidebarSelected(size));
    }

    /// The Health Bars list publishes its pick at once too.
    pub(crate) fn select_health_bars(&mut self, mode: HealthBarMode) {
        self.values.health_bars = mode;
        self.open_combo = None;
        self.pending_events
            .push(LauncherOptionsEvent::HealthBarModeSelected(mode));
    }

    /// The Colour Vision list publishes its pick at once too: the filter reads
    /// the profile every frame, so no atlas or ramp has to be rebuilt.
    pub(crate) fn select_color_vision(&mut self, mode: ColorVisionMode) {
        self.values.color_vision = mode;
        self.open_combo = None;
        self.pending_events
            .push(LauncherOptionsEvent::ColorVisionModeSelected(mode));
    }

    /// The Display list publishes its pick at once too: the window moves to
    /// that screen straight away. Picking the screen already chosen, or one the
    /// window layer did not list, changes nothing.
    pub(crate) fn select_display(&mut self, display: DisplayTarget) {
        self.open_combo = None;
        let listed = match &display {
            DisplayTarget::Auto => true,
            DisplayTarget::Fixed(id) => self.displays.iter().any(|choice| &choice.id == id),
        };
        if !listed || self.display == display {
            return;
        }
        self.display = display.clone();
        self.pending_events
            .push(LauncherOptionsEvent::DisplaySelected(display));
    }

    /// The Language list publishes its pick at once: the next frame draws in
    /// that language. The language in use, or one the list does not offer,
    /// changes nothing.
    pub(crate) fn select_language(&mut self, tag: &str) {
        self.open_combo = None;
        if self.language == tag || !self.languages.iter().any(|choice| choice.tag == tag) {
            return;
        }
        self.language = tag.to_owned();
        self.pending_events
            .push(LauncherOptionsEvent::LanguageSelected(tag.to_owned()));
    }

    /// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80` gives
    /// Back, Keyboard, and terminal completion distinct parent results; the
    /// first admitted result owns the frame's teardown path.
    pub(crate) fn request_result(&mut self, result: LauncherParentResult) {
        if !self.requested() {
            self.pending_result = Some(result);
        }
    }

    pub(crate) const fn category(&self) -> Category {
        self.category
    }

    /// Show `category`'s page in place: no slide, and nothing is applied or
    /// saved. A held slider, an open list and a pressed button don't carry
    /// over to the new page. A dialog rebuilt after Keyboard closes reopens
    /// on the page the player left the same way.
    pub(crate) fn select_category(&mut self, category: Category) {
        if self.category == category {
            return;
        }
        self.category = category;
        self.open_combo = None;
        self.capture = None;
        self.shell_interaction = self.shell_interaction.page_switched();
    }

    /// Ask for Check for Updates; like a parent result, the first request of
    /// a frame wins.
    pub(crate) fn request_update_check(&mut self) {
        if !self.requested() {
            self.pending_update_check = true;
        }
    }

    fn requested(&self) -> bool {
        self.pending_result.is_some() || self.pending_update_check
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` observes
    /// control notifications in order before owner `0x0055FC80` dispatches a
    /// parent result through accepted projection `0x0055FAA0`.
    pub(crate) fn drain_output(&mut self) -> LauncherOptionsFrameOutput {
        LauncherOptionsFrameOutput {
            events: std::mem::take(&mut self.pending_events),
            result: self.pending_result.take(),
            check_for_updates: std::mem::take(&mut self.pending_update_check),
        }
    }
}

/// The dialog's pages, top to bottom in the right-hand column under retail's
/// Keyboard. Retail `0xD5` lays its controls out in four bands whose headings
/// (`GUI:DisplayOptions`, `GUI:GameOptions`, `GUI:UIOptions`,
/// `GUI:AudioOptions`) name the first four pages; VERA adds the last two.
/// Picking a page swaps the controls in place; one set of values covers them
/// all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Display,
    Gameplay,
    Interface,
    Audio,
    Controls,
    Developer,
}

impl Category {
    /// Every page, in the column's order.
    pub(crate) const ALL: [Self; 6] = [
        Self::Display,
        Self::Gameplay,
        Self::Interface,
        Self::Audio,
        Self::Controls,
        Self::Developer,
    ];

    /// The page's controls, top to bottom.
    pub(crate) const fn settings(self) -> &'static [Setting] {
        use LauncherCheckboxId as Check;
        use LauncherComboId as List;
        use LauncherTrackbarId as Slider;
        match self {
            Self::Display => &[
                Setting::List(List::Zoom),
                Setting::List(List::Display),
                Setting::Slider(Slider::HouseColorStrength),
                Setting::List(List::ColorVision),
                Setting::Checkbox(Check::FullScreen),
                Setting::Checkbox(Check::OriginalShape),
                Setting::Checkbox(Check::YrOriginalStyle),
            ],
            Self::Gameplay => &[
                Setting::Slider(Slider::Detail),
                Setting::Slider(Slider::Difficulty),
                Setting::Checkbox(Check::TargetLines),
                Setting::Checkbox(Check::ShowHidden),
                Setting::Checkbox(Check::ShiftQueue),
                Setting::Checkbox(Check::AutoRepair),
                Setting::Checkbox(Check::TerrainDeformation),
                Setting::Checkbox(Check::CustomEndGameBanner),
                Setting::Checkbox(Check::CustomEndGameScore),
                Setting::Checkbox(Check::CustomEndGameMovie),
            ],
            Self::Interface if !HAS_HD_MAP_PREVIEWS => &[
                Setting::List(List::Language),
                Setting::List(List::Sidebar),
                Setting::List(List::HealthBars),
                Setting::Checkbox(Check::Tooltips),
                Setting::Checkbox(Check::VeraIcon),
                Setting::Checkbox(Check::HdRadar),
                Setting::Checkbox(Check::SmoothSidebar),
                Setting::Checkbox(Check::SmoothOnlineMenus),
                Setting::Checkbox(Check::DetailedEvaText),
            ],
            Self::Interface => &[
                Setting::List(List::Language),
                Setting::List(List::Sidebar),
                Setting::List(List::HealthBars),
                Setting::Checkbox(Check::Tooltips),
                Setting::Checkbox(Check::VeraIcon),
                Setting::Checkbox(Check::HdRadar),
                Setting::Checkbox(Check::SmoothSidebar),
                Setting::Checkbox(Check::SmoothOnlineMenus),
                Setting::Checkbox(Check::HdMapPreviews),
                Setting::Checkbox(Check::DetailedEvaText),
            ],
            Self::Audio => &[
                Setting::Slider(Slider::Score),
                Setting::Slider(Slider::Sound),
                Setting::Slider(Slider::Voice),
            ],
            Self::Controls => &[
                Setting::Slider(Slider::Scroll),
                Setting::Checkbox(Check::RightClickOrders),
                Setting::Checkbox(Check::MouseWheelZoom),
                Setting::Checkbox(Check::LockCursor),
            ],
            // The Offline Internet Test Service stays off every page: it swaps
            // Internet for stand-in servers, so only `[VERA] WolTestService=`
            // turns it on.
            Self::Developer => &[
                Setting::Checkbox(Check::PerformanceStats),
                Setting::Checkbox(Check::SkipIntro),
                Setting::Checkbox(Check::ShowStringKeys),
            ],
        }
    }

    /// The status help for the page's button, a VERA language-file label:
    /// retail's string table has nothing for VERA's pages. The browser build
    /// leaves out what it lacks.
    pub(crate) const fn help(self) -> &'static str {
        match self {
            Self::Display => "VERA:DisplayPageHelp",
            Self::Gameplay => "VERA:GameplayPageHelp",
            Self::Interface if HAS_HD_MAP_PREVIEWS => "VERA:InterfacePageHelp",
            Self::Interface => "VERA:InterfacePageHelpBrowser",
            Self::Audio => "VERA:AudioPageHelp",
            Self::Controls => "VERA:ControlsPageHelp",
            Self::Developer if HAS_CHECK_FOR_UPDATES => "VERA:DeveloperPageHelp",
            Self::Developer => "VERA:DeveloperPageHelpBrowser",
        }
    }

    /// The category's page on the in-game menu: what a running match can use
    /// of all the main menu's categories. Controls is the page the menu opens
    /// on, under the template's own Game Controls button: the settings of
    /// gamemd's Game Controls (`0xBBB`) with the three volumes of its Sound
    /// child, so the ones a player reaches for mid-match are one Escape away.
    /// Game Options and Display Options have a tab each.
    pub(crate) const fn in_game_settings(self) -> &'static [Setting] {
        use LauncherCheckboxId as Check;
        use LauncherComboId as List;
        use LauncherTrackbarId as Slider;
        match self {
            Self::Gameplay => &[
                Setting::Slider(Slider::Detail),
                Setting::List(List::HealthBars),
                Setting::Checkbox(Check::RightClickOrders),
                Setting::Checkbox(Check::ShiftQueue),
                Setting::Checkbox(Check::AutoRepair),
                Setting::Checkbox(Check::MouseWheelZoom),
                Setting::Checkbox(Check::LockCursor),
            ],
            Self::Display => &[
                Setting::List(List::Zoom),
                Setting::List(List::Display),
                Setting::List(List::Sidebar),
                Setting::List(List::ColorVision),
                Setting::List(List::Language),
                Setting::Checkbox(Check::FullScreen),
                Setting::Checkbox(Check::OriginalShape),
                Setting::Checkbox(Check::YrOriginalStyle),
                Setting::Checkbox(Check::HdRadar),
                Setting::Checkbox(Check::SmoothSidebar),
                Setting::Checkbox(Check::PerformanceStats),
            ],
            Self::Controls => &[
                Setting::Slider(Slider::GameSpeed),
                Setting::Slider(Slider::Scroll),
                Setting::Slider(Slider::Score),
                Setting::Slider(Slider::Sound),
                Setting::Slider(Slider::Voice),
                Setting::Checkbox(Check::TargetLines),
                Setting::Checkbox(Check::ShowHidden),
                Setting::Checkbox(Check::Tooltips),
            ],
            _ => &[],
        }
    }

    /// [`Self::help`] for the page's tab on the in-game menu.
    pub(crate) const fn in_game_help(self) -> &'static str {
        match self {
            Self::Gameplay => "VERA:InGameGameplayHelp",
            Self::Display => "VERA:InGameDisplayHelp",
            _ => self.help(),
        }
    }
}

/// One control of a page, named by the id its value and events go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Setting {
    List(LauncherComboId),
    Slider(LauncherTrackbarId),
    Checkbox(LauncherCheckboxId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::csf_file::CsfFile;
    use crate::ui::shell::trackbar::thumb_left;

    fn labels() -> LauncherOptionsLabels {
        LauncherOptionsLabels::resolve(&|_| None)
    }

    fn state(audio: bool) -> OptionsDialogState {
        OptionsDialogState::new(
            labels(),
            LauncherOptionsValues::default(),
            Zoom::Percent(200),
            audio,
            Vec::new(),
            DisplayTarget::Auto,
        )
    }

    fn frame(x: i32, y: i32, width: i32, height: i32) -> PhysicalControlFrame {
        PhysicalControlFrame {
            local_x: x,
            local_y: y,
            width,
            height,
        }
    }

    fn build_single_label_csf(label: &str, value: &str) -> Vec<u8> {
        let encoded_value: Vec<u8> = value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .map(|byte| !byte)
            .collect();
        let mut data = Vec::new();
        data.extend_from_slice(&0x4353_4620_u32.to_le_bytes());
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&0_u32.to_le_bytes());
        data.extend_from_slice(&0x4C42_4C20_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&(label.len() as u32).to_le_bytes());
        data.extend_from_slice(label.as_bytes());
        data.extend_from_slice(&0x5354_5220_u32.to_le_bytes());
        data.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        data.extend_from_slice(&encoded_value);
        data
    }

    #[test]
    fn loaded_nonempty_csf_missing_launcher_key_uses_local_fallback() {
        let csf = CsfFile::from_bytes(&build_single_label_csf(
            "GUI:OptionsMenu",
            "Localized Options",
        ))
        .unwrap();
        assert!(!csf.is_empty());
        let labels = LauncherOptionsLabels::resolve(&|key| csf.get(key).map(str::to_owned));
        assert_eq!(labels.options, "Localized Options");
        assert_eq!(labels.main_menu, "Main Menu");
        assert_eq!(csf.text("GUI:MainMenu"), "MISSING:'GUI:MainMenu'");
    }

    /// VERA's strength slider holds positions 0..=9 and nothing else, and the
    /// level it publishes is the profile's own 1..=10 scale.
    #[test]
    fn house_color_strength_stays_on_the_supported_range() {
        use crate::rules::house_colors::{STRENGTH_MAX, STRENGTH_RETAIL};
        let mut state = OptionsDialogState::new(
            labels(),
            LauncherOptionsValues {
                // A hand-edited profile or a stale caller can ask for more.
                house_color_position: 200,
                ..Default::default()
            },
            Zoom::Auto,
            true,
            Vec::new(),
            DisplayTarget::Auto,
        );
        assert_eq!(state.house_color_strength(), STRENGTH_MAX);
        state.set_trackbar_position(LauncherTrackbarId::HouseColorStrength, 4);
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL + 4);
        // Past the control's range the native guard rejects the set outright.
        state.set_trackbar_position(LauncherTrackbarId::HouseColorStrength, 10);
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL + 4);
        // A real change publishes one notch click and one level: the click is
        // the drag's only feedback, since a strength has nothing to audition.
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::GenericClick),
                LauncherOptionsEvent::HouseColorStrengthChanged(STRENGTH_RETAIL + 4),
            ]
        );
    }

    #[test]
    fn captions_name_the_position_from_the_start_and_only_changes_click() {
        let mut state = OptionsDialogState::new(
            labels(),
            LauncherOptionsValues {
                detail_position: 0,
                difficulty_position: 2,
                scroll_position: 0,
                ..Default::default()
            },
            Zoom::Auto,
            true,
            Vec::new(),
            DisplayTarget::Auto,
        );
        // Retail opens with the positions named (TXT_LOW/HARD/SLOWEST here).
        assert_eq!(state.detail_caption, "Low");
        assert_eq!(state.difficulty_caption, "Hard");
        assert_eq!(state.scroll_caption, "Slowest");

        state.set_trackbar_position(LauncherTrackbarId::Detail, 1);
        state.set_trackbar_position(LauncherTrackbarId::Difficulty, 2);
        state.set_trackbar_position(LauncherTrackbarId::Scroll, 6);
        assert_eq!(state.detail_caption, "High");
        assert_eq!(state.difficulty_caption, "Hard", "an unchanged move");
        assert_eq!(state.scroll_caption, "Fastest");
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::GenericClick),
                LauncherOptionsEvent::Cue(LauncherCue::GenericClick),
            ]
        );
    }

    #[test]
    fn initial_control_requests_reject_out_of_range_and_nonfinite_volume() {
        assert_eq!(admitted_initial_position(-1, 6), 0);
        assert_eq!(admitted_initial_position(7, 6), 0);
        assert_eq!(admitted_initial_position(6, 6), 6);
        assert_eq!(admitted_volume_position(0.74), 7);
        assert_eq!(admitted_volume_position(0.75), 8);
        assert_eq!(admitted_volume_position(-0.2), 0);
        assert_eq!(admitted_volume_position(1.1), 0);
        assert_eq!(admitted_volume_position(f32::NAN), 0);
        assert_eq!(admitted_volume_position(f32::INFINITY), 0);
    }

    #[test]
    fn thumb_down_captures_without_jump_and_captured_motion_is_x_only() {
        let mut state = state(true);
        let before = state.trackbar_position(LauncherTrackbarId::Difficulty);
        let left = thumb_left(i32::from(before), 180, 0, 2);
        state.trackbar_mouse_down(LauncherTrackbarId::Difficulty, frame(left + 5, 23, 180, 24));
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Difficulty),
            before
        );
        assert_eq!(
            state.capture,
            Some(TrackbarHold {
                id: LauncherTrackbarId::Difficulty,
                dragging: true,
            })
        );
        state.trackbar_mouse_move(LauncherTrackbarId::Difficulty, frame(179, -400, 180, 24));
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Difficulty), 2);
        state.trackbar_mouse_up(LauncherTrackbarId::Difficulty);
        assert_eq!(state.capture, None);
    }

    #[test]
    fn rail_jump_has_lower_strip_gate_and_holds_without_drag() {
        let mut state = state(true);
        let held = Some(TrackbarHold {
            id: LauncherTrackbarId::Scroll,
            dragging: false,
        });
        state.trackbar_mouse_down(LauncherTrackbarId::Scroll, frame(151, 6, 180, 24));
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Scroll),
            3,
            "strict lower edge rejects"
        );
        assert_eq!(state.capture, held);
        state.trackbar_mouse_up(LauncherTrackbarId::Scroll);
        state.trackbar_mouse_down(LauncherTrackbarId::Scroll, frame(151, 7, 180, 24));
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Scroll), 6);
        assert_eq!(state.capture, held);
        state.trackbar_mouse_move(LauncherTrackbarId::Scroll, frame(0, 7, 180, 24));
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Scroll), 6);
    }

    #[test]
    fn common_audio_gate_rejects_every_audio_input_without_mutation() {
        let mut state = state(false);
        let before = state.values;
        for id in [
            LauncherTrackbarId::Score,
            LauncherTrackbarId::Sound,
            LauncherTrackbarId::Voice,
        ] {
            state.trackbar_mouse_down(id, frame(67, 23, 128, 24));
            state.trackbar_mouse_move(id, frame(67, 23, 128, 24));
        }
        assert_eq!(state.values, before);
        assert!(state.drain_output().events.is_empty());
    }

    #[test]
    fn checkbox_hit_is_strict_icon_square_and_cue_follows_toggle() {
        let mut state = state(true);
        assert!(state.values.tooltips);
        state.checkbox_mouse_down(LauncherCheckboxId::Tooltips, frame(17, 17, 220, 20));
        assert!(!state.values.tooltips);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::Checkbox)]
        );
        state.checkbox_mouse_down(LauncherCheckboxId::Tooltips, frame(18, 5, 220, 20));
        state.checkbox_mouse_down(LauncherCheckboxId::Tooltips, frame(80, 5, 220, 20));
        assert!(!state.values.tooltips);
        assert!(state.drain_output().events.is_empty());
    }

    #[test]
    fn combo_cues_then_toggles_from_anywhere_on_its_face() {
        use LauncherComboId::{Sidebar, Zoom as ZoomList};
        let mut state = state(true);
        state.combo_mouse_down(ZoomList, frame(4, 5, 180, 24));
        assert_eq!(
            state.open_combo,
            Some(ZoomList),
            "the face opens the list, not only the arrow"
        );
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::ComboOpen)]
        );
        state.combo_mouse_down(ZoomList, frame(161, 5, 180, 24));
        assert_eq!(state.open_combo, None, "a second press closes it");
        state.combo_mouse_down(ZoomList, frame(180, 5, 180, 24));
        assert_eq!(state.open_combo, None, "outside the face does nothing");
        state.combo_mouse_down(ZoomList, frame(161, 5, 180, 24));
        assert_eq!(state.open_combo, Some(ZoomList));
        let _ = state.drain_output();
        state.select_zoom(Zoom::Auto);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.zoom, Zoom::Auto);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::ZoomSelected(Zoom::Auto)]
        );

        // The Sidebar list works the same way, on its own value.
        state.combo_mouse_down(Sidebar, frame(4, 5, 180, 24));
        assert_eq!(state.open_combo, Some(Sidebar));
        state.select_sidebar(SidebarSize::Large);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.values.sidebar, SidebarSize::Large);
        assert_eq!(state.zoom, Zoom::Auto);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboOpen),
                LauncherOptionsEvent::SidebarSelected(SidebarSize::Large)
            ]
        );

        // The Health Bars list works the same way, on its own value.
        use LauncherComboId::HealthBars as HealthBarsList;
        state.combo_mouse_down(HealthBarsList, frame(4, 5, 180, 24));
        assert_eq!(state.open_combo, Some(HealthBarsList));
        state.select_health_bars(HealthBarMode::Always);
        assert_eq!(state.open_combo, None);
        assert_eq!(state.values.health_bars, HealthBarMode::Always);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::ComboOpen),
                LauncherOptionsEvent::HealthBarModeSelected(HealthBarMode::Always)
            ]
        );
    }

    #[test]
    fn sidebar_sizes_need_a_640x480_frame_at_that_size() {
        use SidebarSize::{ExtraLarge, Large, Normal};
        assert_eq!(SidebarSize::choices(800, 600), [Normal]);
        assert_eq!(SidebarSize::choices(960, 720), [Normal, Large]);
        assert_eq!(
            SidebarSize::choices(1920, 1080),
            [Normal, Large, ExtraLarge]
        );
        // A 3440x1440 window at 200% zoom has room for Large only.
        assert_eq!(SidebarSize::choices(1720, 720), [Normal, Large]);
        // A size the frame has no room for gives way to the largest that fits.
        assert_eq!(ExtraLarge.in_effect(1720, 720), Large);
        assert_eq!(ExtraLarge.percent(1720, 720), 150);
        assert_eq!(Large.percent(800, 600), 100);
        assert_eq!(ExtraLarge.percent(1920, 1080), 200);
        // Normal fits even a tiny window.
        assert_eq!(SidebarSize::choices(0, 0), [Normal]);
        for size in [Normal, Large, ExtraLarge] {
            assert_eq!(SidebarSize::from_ini_value(size.ini_value()), size);
        }
        assert_eq!(SidebarSize::from_ini_value(175), Normal);
    }

    #[test]
    fn full_screen_box_reports_each_toggle_after_its_cue() {
        let mut state = state(true);
        state.checkbox_mouse_down(LauncherCheckboxId::FullScreen, frame(5, 5, 180, 16));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::FullScreenChanged(true),
            ]
        );
        // The window's own button changes it with no event; the next click
        // turns it off again.
        state.sync_full_screen(false);
        state.checkbox_mouse_down(LauncherCheckboxId::FullScreen, frame(5, 5, 180, 16));
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                LauncherOptionsEvent::FullScreenChanged(true),
            ]
        );
        // The full-screen state is the window's, not part of the accepted pack.
        assert_eq!(state.pack(), self::state(true).pack());
    }

    #[test]
    fn original_shape_box_reports_each_toggle_after_its_cue() {
        let mut state = state(true);
        for checked in [true, false] {
            state.checkbox_mouse_down(LauncherCheckboxId::OriginalShape, frame(5, 5, 195, 16));
            assert_eq!(state.values.original_shape, checked);
            assert_eq!(
                state.drain_output().events,
                [
                    LauncherOptionsEvent::Cue(LauncherCue::Checkbox),
                    LauncherOptionsEvent::OriginalShapeChanged(checked),
                ]
            );
        }
        // Applied at once, so the accepted pack never carries it.
        state.checkbox_mouse_down(LauncherCheckboxId::OriginalShape, frame(5, 5, 195, 16));
        assert_eq!(state.pack(), self::state(true).pack());
    }

    #[test]
    fn zoom_steps_by_half_and_never_shows_less_than_640x480() {
        let percent = |zoom: Zoom, w, h| zoom.percent(w, h);
        // Auto stays sharp and keeps at least the original 800x600 view.
        assert_eq!(percent(Zoom::Auto, 800, 600), 100);
        assert_eq!(percent(Zoom::Auto, 1920, 1080), 100);
        assert_eq!(percent(Zoom::Auto, 1600, 1200), 200);
        assert_eq!(percent(Zoom::Auto, 3440, 1440), 200);
        assert_eq!(percent(Zoom::Auto, 3840, 2160), 300);
        // A fixed choice gives way only where it would cut below 640x480.
        assert_eq!(percent(Zoom::Percent(100), 3840, 2160), 100);
        assert_eq!(percent(Zoom::Percent(300), 3440, 1440), 300);
        assert_eq!(percent(Zoom::Percent(300), 1920, 1080), 200);
        assert_eq!(percent(Zoom::Percent(200), 1024, 768), 150);
        // A minimised or tiny window still gets a usable zoom.
        assert_eq!(percent(Zoom::Percent(300), 0, 0), 100);
        // The list: Auto, then every step the window allows.
        assert_eq!(Zoom::choices(800, 600), [Zoom::Auto, Zoom::Percent(100)]);
        assert_eq!(
            Zoom::choices(1920, 1080),
            [
                Zoom::Auto,
                Zoom::Percent(100),
                Zoom::Percent(150),
                Zoom::Percent(200)
            ]
        );
        let ultra = Zoom::choices(3840, 2160);
        assert_eq!(ultra.last(), Some(&Zoom::Percent(450)));
        for zoom in ultra {
            assert_eq!(Zoom::from_ini_value(zoom.ini_value()), zoom);
        }
        // The first release saved whole factors.
        assert_eq!(Zoom::from_ini_value(2), Zoom::Percent(200));
        for unknown in [-1, 4, 99, 125, 10_050] {
            assert_eq!(Zoom::from_ini_value(unknown), Zoom::Auto);
        }
    }

    #[test]
    fn audio_changes_emit_preview_without_generic_click_and_pack_exact_values() {
        let mut state = state(true);
        state.set_trackbar_position(LauncherTrackbarId::Score, 0);
        state.set_trackbar_position(LauncherTrackbarId::Sound, 5);
        state.set_trackbar_position(LauncherTrackbarId::Voice, 6);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::ScorePreview(0.0),
                LauncherOptionsEvent::SoundPreview(0.5),
                LauncherOptionsEvent::VoicePreview(0.6),
            ]
        );
        let packed = state.pack();
        assert_eq!(packed.detail_level, 2);
        assert_eq!(packed.difficulty, 1);
        assert_eq!(packed.scroll_rate, 3);
        assert_eq!(packed.score_volume.to_bits(), 0.0_f32.to_bits());
        assert_eq!(packed.sound_volume.to_bits(), 0.5_f32.to_bits());
        assert_eq!(packed.voice_volume.to_bits(), 0.6_f32.to_bits());
    }

    #[test]
    fn the_first_request_of_a_frame_wins_across_results_and_the_update_check() {
        let mut state = state(true);
        state.request_update_check();
        state.request_result(LauncherParentResult::Back);
        let output = state.drain_output();
        assert_eq!((output.result, output.check_for_updates), (None, true));
        state.request_result(LauncherParentResult::Keyboard);
        state.request_update_check();
        let output = state.drain_output();
        assert_eq!(
            (output.result, output.check_for_updates),
            (Some(LauncherParentResult::Keyboard), false)
        );
    }

    #[test]
    fn escape_is_no_event_and_first_parent_result_wins_one_frame() {
        let mut state = state(true);
        assert!(state.drain_output().events.is_empty());
        state.request_result(LauncherParentResult::Keyboard);
        state.request_result(LauncherParentResult::Back);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Keyboard)
        );
        assert_eq!(state.drain_output().result, None);
    }

    fn display_settings() -> Vec<DisplayChoice> {
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
                refresh_millihertz: Some(143_999),
            },
            DisplayChoice {
                id: "\\\\.\\DISPLAY3".to_owned(),
                width: 800,
                height: 600,
                refresh_millihertz: None,
            },
        ]
    }

    #[test]
    fn display_rows_name_the_list_position_size_and_rate() {
        let choices = display_settings();
        // The list position names the screen; the device name only persists.
        assert_eq!(choices[0].label(1), "Screen 1: 1920x1080 60Hz");
        // A rate that is not a whole number of hertz still reads as its
        // truncated figure, so a 143.999 Hz panel says 143Hz, not 144Hz.
        assert_eq!(choices[1].label(2), "Screen 2: 2560x1440 143Hz");
        // A screen that reports no rate leaves it out instead of inventing one.
        assert_eq!(choices[2].label(3), "Screen 3: 800x600");
    }

    #[test]
    fn display_target_round_trips_through_its_ini_value() {
        assert_eq!(DisplayTarget::Auto.ini_value(), "");
        assert_eq!(DisplayTarget::from_ini_value(""), DisplayTarget::Auto);
        // A name the machine no longer has is kept: unplugging a screen must
        // not silently rewrite the player's choice.
        let named = DisplayTarget::Fixed("\\\\.\\DISPLAY2".to_owned());
        assert_eq!(named.ini_value(), "\\\\.\\DISPLAY2");
        assert_eq!(
            DisplayTarget::from_ini_value(named.ini_value()),
            named,
            "a retained name survives a restart"
        );
    }

    #[test]
    fn display_list_offers_auto_then_every_attached_screen() {
        let mut state = OptionsDialogState::new(
            labels(),
            LauncherOptionsValues::default(),
            Zoom::Auto,
            true,
            display_settings(),
            DisplayTarget::Auto,
        );
        assert_eq!(state.displays().len(), 3);
        assert_eq!(
            state.display_choices(),
            [
                DisplayTarget::Auto,
                DisplayTarget::Fixed("\\\\.\\DISPLAY1".to_owned()),
                DisplayTarget::Fixed("\\\\.\\DISPLAY2".to_owned()),
                DisplayTarget::Fixed("\\\\.\\DISPLAY3".to_owned()),
            ]
        );

        // Picking a screen applies at once, like Zoom and Sidebar.
        state.select_display(DisplayTarget::Fixed("\\\\.\\DISPLAY2".to_owned()));
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::DisplaySelected(DisplayTarget::Fixed(
                "\\\\.\\DISPLAY2".to_owned()
            ))]
        );
        // Picking it again asks for nothing: the window is already there.
        state.select_display(DisplayTarget::Fixed("\\\\.\\DISPLAY2".to_owned()));
        assert!(state.drain_output().events.is_empty());
        // A screen the window layer did not list cannot be picked.
        state.select_display(DisplayTarget::Fixed("\\\\.\\DISPLAY9".to_owned()));
        assert!(state.drain_output().events.is_empty());
        // Auto is always available, so a stale saved name is recoverable.
        state.select_display(DisplayTarget::Auto);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::DisplaySelected(DisplayTarget::Auto)]
        );
    }

    #[test]
    fn a_saved_screen_that_is_gone_still_opens_on_auto() {
        // The profile keeps a name this desktop no longer has. The face shows
        // it, so the player can see what is saved, and Auto is one row away.
        let state = OptionsDialogState::new(
            labels(),
            LauncherOptionsValues::default(),
            Zoom::Auto,
            true,
            display_settings(),
            DisplayTarget::Fixed("\\\\.\\DISPLAY7".to_owned()),
        );
        assert_eq!(state.display_choices().len(), 4);
    }

    /// Every control is on exactly one page: one the table leaves out could
    /// not be reached, and one listed twice would show twice. The diagnostic
    /// test service is on none.
    #[test]
    fn every_control_is_on_exactly_one_page() {
        use LauncherCheckboxId as Check;
        use LauncherComboId as List;
        use LauncherTrackbarId as Slider;
        let placed: Vec<Setting> = Category::ALL
            .iter()
            .flat_map(|page| page.settings())
            .copied()
            .collect();
        let every: Vec<Setting> = [
            List::Zoom,
            List::Sidebar,
            List::Display,
            List::HealthBars,
            List::ColorVision,
            List::Language,
        ]
        .map(Setting::List)
        .into_iter()
        .chain(
            [
                Slider::Detail,
                Slider::Difficulty,
                Slider::Scroll,
                Slider::Score,
                Slider::Sound,
                Slider::Voice,
                Slider::HouseColorStrength,
            ]
            .map(Setting::Slider),
        )
        .chain(
            [
                Check::Tooltips,
                Check::TargetLines,
                Check::ShowHidden,
                Check::FullScreen,
                Check::OriginalShape,
                Check::YrOriginalStyle,
                Check::MouseWheelZoom,
                Check::RightClickOrders,
                Check::LockCursor,
                Check::VeraIcon,
                Check::HdRadar,
                Check::SmoothSidebar,
                Check::SmoothOnlineMenus,
                Check::HdMapPreviews,
                Check::ShiftQueue,
                Check::AutoRepair,
                Check::TerrainDeformation,
                Check::PerformanceStats,
                Check::ShowStringKeys,
                Check::SkipIntro,
                Check::CustomEndGameBanner,
                Check::CustomEndGameScore,
                Check::CustomEndGameMovie,
                Check::DetailedEvaText,
            ]
            .map(Setting::Checkbox),
        )
        .collect();
        for setting in &every {
            let pages = placed.iter().filter(|placed| *placed == setting).count();
            assert_eq!(pages, 1, "{setting:?}");
        }
        assert_eq!(placed.len(), every.len());
    }
}
