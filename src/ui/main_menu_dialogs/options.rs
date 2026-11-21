//! Retained semantic model for the retail launcher Options dialog `0xD5`.
//!
//! The parent owns bounded control state and emits ordered effects. It owns no
//! profile, filesystem, audio device, window, or child-dialog callback.
//! The `shell` child supplies retail resource geometry and physical-pixel input;
//! the app shell renderer paints that model through the shared retail compositor.
//! Retail's controls and VERA's own share one dialog, sorted into the
//! [`Category`] pages its right-hand column picks between. Each control's
//! caption, status help and value come from one table per kind
//! ([`LauncherCheckboxId::keys`], [`LauncherTrackbarId::text`],
//! [`LauncherComboId::text`]).

pub(crate) mod shell;

use crate::ui::shell::page::PageCue;

/// gamemd-derived: launcher owner `OptionsClass__ShowLauncherDialog @
/// 0x0055FC80` and its primary-proc slice `0x0055FDB0..0x0056047A` bind the
/// RT_DIALOG `0xD5` controls to these launcher-local CSF keys/fallbacks. The
/// template's slider captions (`GUI:HigherDetail`, `GUI:Harder`,
/// `GUI:Faster`) never show: the init renames them by position. The combo's
/// own caption, `GUI:SetResolution`, left with the resolution list: the combo
/// is VERA's [`Zoom`] list now. The last four are captions `0xD5` itself never
/// shows: the in-game menu's Back and Game Controls page, and the Yes and No
/// beside the boxes of VERA's Esc menu.
pub(crate) const LAUNCHER_LABEL_SPECS: &[(&str, &str)] = &[
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
    ("GUI:Back", "Back"),
    ("GUI:GameControls", "Game Controls"),
    ("GUI:Yes", "Yes"),
    ("GUI:No", "No"),
];

/// VERA's own captions and status help on this page, as labels of VERA's
/// language files ([`crate::i18n`]): the retail string table has nothing for
/// full screen, zoom, the battle shape or the Red Alert 2 loading screen and
/// menu movie. The other boxes, lists and buttons name theirs in their tables.
pub(crate) const ZOOM_HELP: &str = "VERA:ZoomHelp";
pub(crate) const FULL_SCREEN_LABEL: &str = "VERA:FullScreen";
pub(crate) const FULL_SCREEN_HELP: &str = "VERA:FullScreenHelp";
pub(crate) const ORIGINAL_SHAPE_LABEL: &str = "VERA:OriginalShape";
pub(crate) const ORIGINAL_SHAPE_HELP: &str = "VERA:OriginalShapeHelp";
pub(crate) const YR_ORIGINAL_STYLE_LABEL: &str = "VERA:YrOriginalStyle";
pub(crate) const PERFORMANCE_STATS_LABEL: &str = "VERA:PerformanceStats";
pub(crate) const PERFORMANCE_STATS_HELP: &str = "VERA:PerformanceStatsHelp";
pub(crate) const CUSTOM_END_GAME_BANNER_LABEL: &str = "VERA:CustomEndGameBanner";
pub(crate) const CUSTOM_END_GAME_SCORE_LABEL: &str = "VERA:CustomEndGameScore";
pub(crate) const CUSTOM_END_GAME_MOVIE_LABEL: &str = "VERA:CustomEndGameMovie";
pub(crate) const COMMANDER_FEEDBACK_LABEL: &str = "VERA:CommanderFeedback";
pub(crate) const LANGUAGE_LABEL: &str = "VERA:Language";
/// The in-game menu's Options button, and the Back that ends its column.
pub(crate) const IN_GAME_OPTIONS_HELP: &str = "VERA:InGameOptionsHelp";
pub(crate) const IN_GAME_OPTIONS_BACK_HELP: &str = "VERA:InGameOptionsBackHelp";
/// What a greyed control says on the in-game menu: a setting the next game
/// starts with ([`LauncherTrackbarId::fixed_in_match`]), or Check for Updates.
pub(crate) const NOT_IN_MATCH_HELP: &str = "VERA:NotInMatchHelp";
/// The Display list's first row: the window system's own choice.
pub(crate) const DISPLAY_AUTO_LABEL: &str = "VERA:DisplayAuto";
/// The Display page's VSync (`GpuContext::set_vsync`). The browser presents
/// with the page's own refresh, so its build has no box.
pub(crate) const HAS_VSYNC: bool = !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
/// The General page's HD Map Previews (`app::map_picture`). The browser
/// build cannot start the hidden run that draws a map's picture.
pub(crate) const HAS_HD_MAP_PREVIEWS: bool =
    !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
/// The General page's Check for Updates: the desktop updater
/// (`app::updater`) checks once the dialog has slid out. The browser build
/// has no updater, and the in-game menu shows it greyed: no slide-out
/// happens there.
pub(crate) const HAS_CHECK_FOR_UPDATES: bool =
    !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
pub(crate) const CHECK_FOR_UPDATES_LABEL: &str = "VERA:CheckForUpdates";
pub(crate) const CHECK_FOR_UPDATES_HELP: &str = "VERA:CheckForUpdatesHelp";
/// The Interface page's Edit Layout: in a match the menu closes and the
/// layout edit mode (`app::input::layout_edit`) takes the mouse, as the Edit
/// Layout key does. The main menu has no match screen, so it shows it greyed.
pub(crate) const EDIT_LAYOUT_LABEL: &str = "VERA:LayoutEdit";
pub(crate) const EDIT_LAYOUT_HELP: &str = "VERA:EditLayoutHelp";
/// The column's Mods button, which opens VERA's Mods page
/// (`ui::mods_shell`). The browser build has no Mods folder.
pub(crate) const HAS_MODS: bool = !cfg!(all(target_arch = "wasm32", target_os = "unknown"));
/// The Esc menu's Set Window Size button. A browser tab's canvas takes its size
/// from the page and an Android window fills the screen, so those builds have
/// none.
pub(crate) const HAS_SET_WINDOW_SIZE: bool = !cfg!(any(
    all(target_arch = "wasm32", target_os = "unknown"),
    target_os = "android"
));
pub(crate) const SET_WINDOW_SIZE_LABEL: &str = "VERA:SetWindowSize";
pub(crate) const SET_WINDOW_SIZE_HELP: &str = "VERA:SetWindowSizeHelp";

/// The `[VERA] Ares=` list's rows, top to bottom; the first is the default.
pub(crate) fn ares_choices() -> Vec<crate::util::ares_launch::AresSetting> {
    use crate::util::ares_launch::AresSetting;
    vec![AresSetting::Auto, AresSetting::On, AresSetting::Off]
}

/// The `[VERA] Ares=` list's row text.
pub(crate) fn ares_label(setting: crate::util::ares_launch::AresSetting) -> String {
    use crate::util::ares_launch::AresSetting;
    crate::i18n::tr(match setting {
        AresSetting::Auto => "VERA:AresSettingAuto",
        AresSetting::On => "VERA:AresSettingOn",
        AresSetting::Off => "VERA:AresSettingOff",
    })
}

/// A Zoom row: the zoom and the battle's size in game pixels in this window,
/// what `App::sync_zoom_pass` will give the match.
pub(crate) fn zoom_label(zoom: Zoom, (width, height): (u32, u32), original_shape: bool) -> String {
    use crate::render::upscale_pass::{FrameShape, ZoomGeometry};
    let shape = if original_shape {
        FrameShape::Original
    } else {
        FrameShape::Window
    };
    let geometry = ZoomGeometry::new(width, height, zoom.percent(width, height), shape);
    let (width, height) = (&geometry.src_width, &geometry.src_height);
    match zoom {
        Zoom::Auto => {
            crate::i18n::tr_fill("VERA:ZoomAuto", &[("width", width), ("height", height)])
        }
        Zoom::Percent(percent) => crate::i18n::tr_fill(
            "VERA:ZoomPercent",
            &[("percent", &percent), ("width", width), ("height", height)],
        ),
    }
}

/// A button of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherButton {
    /// Keyboard or Main Menu: one of gamemd's parent exits.
    Exit(LauncherParentResult),
    /// A category in the right-hand column: shows that category's page.
    Category(Category),
    /// The in-game menu's Options, under its Game Controls: the main menu's
    /// column takes the menu's place, on its first page.
    InGameOptions,
    /// The foot of that column, where Resume stands on the menu: the menu
    /// and its Game Controls page again.
    InGameBack,
    /// Check for Updates on the General page.
    CheckForUpdates,
    /// Edit Layout on the Interface page.
    EditLayout,
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
            Self::CheckForUpdates
                | Self::EditLayout
                | Self::SetWindowSize
                | Self::InGameKeyboard
                | Self::InGameSound
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

/// VERA's `[VERA] SidebarColumns=`: how many build pictures a sidebar row
/// holds.
///
/// 2 is gamemd's own (GetVisibleSlotCount `0x006AC430` counts `2 * rows`).
/// 3 to 5 share the span the original's two columns take between smaller
/// cells, so the sidebar keeps its width and only its inside changes.
/// Presentation only: it never reaches simulation state or the rules online
/// players compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SidebarColumns {
    #[default]
    Two,
    Three,
    Four,
    Five,
}

impl SidebarColumns {
    /// The list's rows, top to bottom; the first is the default.
    const ALL: [Self; 4] = [Self::Two, Self::Three, Self::Four, Self::Five];

    /// The row's text.
    pub(crate) fn label(self) -> String {
        crate::i18n::tr(match self {
            Self::Two => "VERA:SidebarColumnsTwo",
            Self::Three => "VERA:SidebarColumnsThree",
            Self::Four => "VERA:SidebarColumnsFour",
            Self::Five => "VERA:SidebarColumnsFive",
        })
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The columns the grid lays.
    pub(crate) const fn count(self) -> usize {
        match self {
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::Five => 5,
        }
    }

    /// The `[VERA] SidebarColumns` value.
    pub(crate) const fn ini_value(self) -> i32 {
        self.count() as i32
    }

    /// A value this build does not know reads as the original's two.
    pub(crate) const fn from_ini_value(value: i32) -> Self {
        match value {
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            _ => Self::Two,
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
    /// The `[VERA] SidebarColumns=` list: 2 to 5 cameos a row.
    SidebarColumns,
    Display,
    /// The `[VERA] HealthBars=` list.
    HealthBars,
    /// The `[VERA] ColorVisionMode=` list: the whole-viewport filter.
    ColorVision,
    /// The `[VERA] Language=` list: VERA's interface language. Its rows are
    /// each language's own name for itself, so they need no translation.
    Language,
    /// The `[VERA] Ares=` list: the Ares extension setting, from the next
    /// start (`util::ares_launch::AresSetting`).
    Ares,
}

impl LauncherComboId {
    /// The list's caption and status help: VERA's own, as retail has no such
    /// lists.
    pub(crate) const fn text(self) -> (&'static str, &'static str) {
        match self {
            Self::Zoom => ("VERA:Zoom", ZOOM_HELP),
            Self::Sidebar => ("VERA:Sidebar", "VERA:SidebarHelp"),
            Self::SidebarColumns => ("VERA:SidebarColumns", "VERA:SidebarColumnsHelp"),
            Self::Display => ("VERA:Display", "VERA:DisplayHelp"),
            Self::HealthBars => ("VERA:HealthBars", "VERA:HealthBarsHelp"),
            Self::ColorVision => ("VERA:ColorVision", "VERA:ColorVisionHelp"),
            Self::Language => (LANGUAGE_LABEL, "VERA:LanguageHelp"),
            Self::Ares => ("VERA:AresSetting", "VERA:AresSettingHelp"),
        }
    }
}

/// The retail captions of [`LAUNCHER_LABEL_SPECS`], read from the string table
/// when the dialog opens and again after a language change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLabels(Vec<(&'static str, String)>);

impl LauncherOptionsLabels {
    pub(crate) fn resolve(csf: &dyn Fn(&str) -> Option<String>) -> Self {
        Self(
            LAUNCHER_LABEL_SPECS
                .iter()
                .map(|&(key, fallback)| (key, csf(key).unwrap_or_else(|| fallback.to_owned())))
                .collect(),
        )
    }

    /// `key`'s text; empty for a key the table doesn't hold.
    pub(crate) fn get(&self, key: &str) -> &str {
        self.0
            .iter()
            .find(|(k, _)| *k == key)
            .map_or("", |(_, text)| text)
    }

    /// Show `text` for `key`: VERA's Esc menu opens Options over a page, and
    /// its Main Menu button returns there as Back.
    pub(crate) fn set(&mut self, key: &str, text: String) {
        if let Some(entry) = self.0.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = text;
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

    /// The slider's value at position 0. VERA's strength slider runs on the
    /// levels it stands for, so its plaque reads the level.
    const fn value_base(self) -> i32 {
        match self {
            Self::HouseColorStrength => crate::rules::house_colors::STRENGTH_MIN as i32,
            _ => 0,
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

    /// Whether a running match keeps it as the match began: the campaign
    /// difficulty is taken when a game starts, and the team colour strength is
    /// baked into a battle's colour tables when it loads, so the in-game menu
    /// shows them greyed. gamemd's in-game menu has no Difficulty either, so a
    /// mission's restart keeps its own.
    pub(crate) const fn fixed_in_match(self) -> bool {
        matches!(self, Self::Difficulty | Self::HouseColorStrength)
    }

    /// The caption (Game Speed's is the in-game menu's own), the status help
    /// (`0x00604729..0x00604838`), and the names a plaque-less slider prints
    /// for its positions. The 0x497 init sets each trackbar's range and
    /// position; every change sends WM_HSCROLL (0x0061E609..0x0061E6AF), whose
    /// handler (0x0055FF68) names the position (tables 0x0082A248, of which
    /// Detail shows entries 0 and 2, 0x0082A254 and 0x0082A260), so the
    /// resource captions never show.
    const fn text(self) -> (&'static str, &'static str, &'static [&'static str]) {
        match self {
            Self::Detail => (
                "GUI:VisualDetails",
                "STT:MainOptSliderVisual",
                &["TXT_LOW", "TXT_HIGH"],
            ),
            Self::Difficulty => (
                "GUI:Difficulty",
                "STT:MainOptSliderDifficulty",
                &["TXT_EASY", "TXT_NORMAL", "TXT_HARD"],
            ),
            Self::Scroll => (
                "GUI:ScrollRate",
                "STT:MainOptSliderScroll",
                &[
                    "TXT_SLOWEST",
                    "TXT_SLOWER",
                    "TXT_SLOW",
                    "TXT_MEDIUM",
                    "TXT_FAST",
                    "TXT_FASTER",
                    "TXT_FASTEST",
                ],
            ),
            Self::Score => ("GUI:MusicVolume", "STT:MainOptSliderMusic", &[]),
            Self::Sound => ("GUI:SoundVolume", "STT:MainOptSliderSound", &[]),
            Self::Voice => ("GUI:VoiceVolume", "STT:MainOptSliderVoice", &[]),
            // VERA's own slider: retail has no string for it.
            Self::HouseColorStrength => (
                "VERA:HouseColorStrength",
                "VERA:HouseColorStrengthHelp",
                &[],
            ),
            // Retail's string for the Game Options speed slider.
            Self::GameSpeed => ("", "STT:GameOptSliderSpeed", &[]),
        }
    }

    /// Where its position lives.
    fn slot(self, values: &mut LauncherOptionsValues) -> &mut u8 {
        match self {
            Self::Detail => &mut values.detail_position,
            Self::Difficulty => &mut values.difficulty_position,
            Self::Scroll => &mut values.scroll_position,
            Self::Score => &mut values.score_position,
            Self::Sound => &mut values.sound_position,
            Self::Voice => &mut values.voice_position,
            Self::HouseColorStrength => &mut values.house_color_position,
            Self::GameSpeed => &mut values.game_speed_position,
        }
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
    /// VERA's local camera option: the wheel changes battle view distance.
    MouseWheelZoom,
    /// VERA-only alternative controls: left selects, right commands.
    RightClickOrders,
    /// VERA's own box: in Place Beacon mode a left click on the radar
    /// places the beacon there (`app::input::radar`, `docs/minimap-beacons.md`).
    /// It applies at once.
    MinimapBeacons,
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
    /// VERA's own box: the window presents in step with the display
    /// (`GpuContext::set_vsync`). On by default; saved, and the next frame
    /// follows it.
    VSync,
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
    /// between its text and its own string-table key, VERA's own `VERA:`
    /// labels included. Saved in `[VERA] ShowStringKeys=`; a start shows the
    /// texts until the hotkey asks for keys.
    ShowStringKeys,
    /// VERA's end-game switches, the pair of `[EndGameScreens]` presentation
    /// stages: the verdict is what the match shows where it is decided; the
    /// score page is the dialog after it. Each stage owns its picture *and*
    /// its music, so a player can keep one stage and drop the other.
    ///
    /// This box: the score page draws the country's own victory or defeat
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
    /// VERA's box (`docs/ini-commander-personalities.md`): the computer
    /// commanders a mod or map adds play their clips in the radar in
    /// skirmish. On by default; the clip layer reads it every pass, so
    /// unticking it in a match silences them at once.
    CommanderFeedback,
    /// VERA-only accessibility box: detailed EVA text rows, default off.
    DetailedEvaText,
    /// VERA's umbrella box: quick status rows (repair/reload countdowns today,
    /// more later), default off. Independent of [`Self::DetailedEvaText`].
    ConvenienceHelp,
    /// VERA's gameplay box: a structure PRODUCE queues behind a running or
    /// completed building instead of being dropped. A rule switch the next
    /// offline scenario reads (`rules::personal_rules`); off by default.
    BuildingProductionQueue,
    /// VERA's box: the super weapon timer list shows six lines and pages the
    /// rest. Default off; the next timer draw reads it.
    CompactSuperTimers,
}

impl LauncherCheckboxId {
    /// The box's caption and status help.
    pub(crate) const fn keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Tooltips => ("GUI:Tooltips", "STT:MainOptCBoxTooltips"),
            Self::TargetLines => ("GUI:TargetLines", "STT:MainOptCBoxTargetLines"),
            Self::ShowHidden => ("GUI:ShowHidden", "STT:MainOptCBoxHidden"),
            Self::FullScreen => (FULL_SCREEN_LABEL, FULL_SCREEN_HELP),
            Self::OriginalShape => (ORIGINAL_SHAPE_LABEL, ORIGINAL_SHAPE_HELP),
            Self::YrOriginalStyle => (YR_ORIGINAL_STYLE_LABEL, "VERA:YrOriginalStyleHelp"),
            Self::ShiftQueue => ("VERA:ShiftQueue", "VERA:ShiftQueueHelp"),
            Self::AutoRepair => ("VERA:AutoRepair", "VERA:AutoRepairHelp"),
            Self::MouseWheelZoom => ("VERA:MouseWheelZoom", "VERA:MouseWheelZoomHelp"),
            Self::RightClickOrders => ("VERA:RightClickOrders", "VERA:RightClickOrdersHelp"),
            Self::MinimapBeacons => ("VERA:MinimapBeacons", "VERA:MinimapBeaconsHelp"),
            Self::LockCursor => ("VERA:LockCursor", "VERA:LockCursorHelp"),
            Self::VeraIcon => ("VERA:VeraIcon", "VERA:VeraIconHelp"),
            Self::HdRadar => ("VERA:HdRadar", "VERA:HdRadarHelp"),
            Self::SmoothSidebar => ("VERA:SmoothSidebar", "VERA:SmoothSidebarHelp"),
            Self::SmoothOnlineMenus => ("VERA:SmoothOnlineMenus", "VERA:SmoothOnlineMenusHelp"),
            Self::VSync => ("VERA:VSync", "VERA:VSyncHelp"),
            Self::HdMapPreviews => ("VERA:HdMapPreviews", "VERA:HdMapPreviewsHelp"),
            Self::PerformanceStats => (PERFORMANCE_STATS_LABEL, PERFORMANCE_STATS_HELP),
            Self::SkipIntro => ("VERA:SkipIntro", "VERA:SkipIntroHelp"),
            Self::ShowStringKeys => ("VERA:ShowStringKeys", "VERA:ShowStringKeysHelp"),
            Self::CustomEndGameBanner => {
                (CUSTOM_END_GAME_BANNER_LABEL, "VERA:CustomEndGameBannerHelp")
            }
            Self::CustomEndGameScore => {
                (CUSTOM_END_GAME_SCORE_LABEL, "VERA:CustomEndGameScoreHelp")
            }
            Self::CustomEndGameMovie => {
                (CUSTOM_END_GAME_MOVIE_LABEL, "VERA:CustomEndGameMovieHelp")
            }
            Self::CommanderFeedback => (COMMANDER_FEEDBACK_LABEL, "VERA:CommanderFeedbackHelp"),
            Self::DetailedEvaText => ("VERA:DetailedEvaText", "VERA:DetailedEvaTextHelp"),
            Self::ConvenienceHelp => ("VERA:ConvenienceHelp", "VERA:ConvenienceHelpHelp"),
            Self::BuildingProductionQueue => (
                "VERA:BuildingProductionQueue",
                "VERA:BuildingProductionQueueHelp",
            ),
            Self::CompactSuperTimers => ("VERA:CompactSuperTimers", "VERA:CompactSuperTimersHelp"),
        }
    }

    /// Where its value lives.
    fn slot(self, v: &mut LauncherOptionsValues) -> &mut bool {
        match self {
            Self::Tooltips => &mut v.tooltips,
            Self::TargetLines => &mut v.target_lines,
            Self::ShowHidden => &mut v.show_hidden,
            Self::FullScreen => &mut v.full_screen,
            Self::OriginalShape => &mut v.original_shape,
            Self::YrOriginalStyle => &mut v.yr_original_style,
            Self::ShiftQueue => &mut v.shift_queue,
            Self::AutoRepair => &mut v.auto_repair,
            Self::MouseWheelZoom => &mut v.mouse_wheel_zoom,
            Self::RightClickOrders => &mut v.right_click_orders,
            Self::MinimapBeacons => &mut v.minimap_beacons,
            Self::LockCursor => &mut v.lock_cursor,
            Self::VeraIcon => &mut v.vera_icon,
            Self::HdRadar => &mut v.hd_radar,
            Self::SmoothSidebar => &mut v.smooth_sidebar,
            Self::SmoothOnlineMenus => &mut v.smooth_online_menus,
            Self::VSync => &mut v.vsync,
            Self::HdMapPreviews => &mut v.hd_map_previews,
            Self::PerformanceStats => &mut v.performance_stats,
            Self::SkipIntro => &mut v.skip_intro,
            Self::ShowStringKeys => &mut v.show_string_keys,
            Self::CustomEndGameBanner => &mut v.custom_end_game_banner,
            Self::CustomEndGameScore => &mut v.custom_end_game_score,
            Self::CustomEndGameMovie => &mut v.custom_end_game_movie,
            Self::CommanderFeedback => &mut v.commander_feedback,
            Self::DetailedEvaText => &mut v.detailed_eva_text,
            Self::ConvenienceHelp => &mut v.convenience_help,
            Self::BuildingProductionQueue => &mut v.building_production_queue,
            Self::CompactSuperTimers => &mut v.compact_super_timers,
        }
    }

    /// Whether a toggle publishes: retail's three apply with the accepted
    /// pack (`0x0055FAA0`); VERA's publish each change.
    const fn publishes(self) -> bool {
        !matches!(self, Self::Tooltips | Self::TargetLines | Self::ShowHidden)
    }
}

#[derive(Debug, Clone, PartialEq)]
/// gamemd-derived: notification order in launcher primary-proc slice
/// `0x0055FDB0..0x0056047A`; owner `OptionsClass__ShowLauncherDialog @
/// 0x0055FC80` consumes these before any parent-result teardown. A list's pick
/// and a VERA box's change publish at once; see their ids for what each does.
pub(crate) enum LauncherOptionsEvent {
    ZoomSelected(Zoom),
    SidebarSelected(SidebarSize),
    SidebarColumnsSelected(SidebarColumns),
    HealthBarModeSelected(HealthBarMode),
    /// Played from the next start.
    AresSelected(crate::util::ares_launch::AresSetting),
    ColorVisionModeSelected(ColorVisionMode),
    DisplaySelected(DisplayTarget),
    /// A language tag.
    LanguageSelected(String),
    /// A VERA box's new state ([`LauncherCheckboxId::publishes`]).
    Toggled(LauncherCheckboxId, bool),
    /// The Esc menu's Set Window Size button, applied to the window at once.
    SetWindowSizeRequested,
    /// VERA's team-colour strength, as the level the profile stores.
    HouseColorStrengthChanged(u8),
    ScorePreview(f32),
    SoundPreview(f32),
    VoicePreview(f32),
    /// The in-game menu's Keyboard and Sound buttons, which that menu routes
    /// (`app::input::pause_menu`).
    InGameKeyboardRequested,
    InGameSoundRequested,
    /// Edit Layout, which the in-game menu routes the same way: it closes and
    /// the layout edit mode starts.
    EditLayoutRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// gamemd-derived: the distinct exits owned by
/// `OptionsClass__ShowLauncherDialog @ 0x0055FC80`; every accepted result first
/// projects through `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0`.
pub(crate) enum LauncherParentResult {
    Back,
    Keyboard,
    /// VERA's own: the Mods page, a child of the dialog as Keyboard is. The
    /// profile is written before it opens, since Use This Mod leaves the game
    /// from there.
    Mods,
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LauncherOptionsFrameOutput {
    /// The controls' sounds, which play before the events run.
    pub(crate) cues: Vec<PageCue>,
    pub(crate) events: Vec<LauncherOptionsEvent>,
    pub(crate) result: Option<LauncherParentResult>,
    /// Check for Updates: the dialog slides out, then the updater checks.
    pub(crate) check_for_updates: bool,
}

/// The dialog's values: a field per box ([`LauncherCheckboxId`]) and list, and
/// a position per slider.
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
    pub(crate) vsync: bool,
    pub(crate) hd_map_previews: bool,
    pub(crate) shift_queue: bool,
    pub(crate) auto_repair: bool,
    pub(crate) minimap_beacons: bool,
    /// The FPS readout's live state when the page opened, then each toggle.
    pub(crate) performance_stats: bool,
    pub(crate) show_string_keys: bool,
    pub(crate) skip_intro: bool,
    pub(crate) custom_end_game_banner: bool,
    pub(crate) custom_end_game_score: bool,
    pub(crate) custom_end_game_movie: bool,
    pub(crate) commander_feedback: bool,
    pub(crate) detailed_eva_text: bool,
    pub(crate) convenience_help: bool,
    pub(crate) building_production_queue: bool,
    pub(crate) compact_super_timers: bool,
    /// The Sidebar list's choice; like Zoom, it applies at once.
    pub(crate) sidebar: SidebarSize,
    /// The Sidebar Columns list's choice; it applies at once too.
    pub(crate) sidebar_columns: SidebarColumns,
    /// The Health Bars list's choice; like Sidebar, it applies at once.
    pub(crate) health_bars: HealthBarMode,
    /// The Ares list's choice; it applies from the next start, as a mod does.
    pub(crate) ares: crate::util::ares_launch::AresSetting,
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
    /// The in-game menu's Game Speed slider, set by
    /// [`OptionsDialogState::set_in_game_controls`]; no profile pack holds it.
    pub(crate) game_speed_position: u8,
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
            vsync: true,
            hd_map_previews: true,
            shift_queue: false,
            auto_repair: false,
            minimap_beacons: false,
            performance_stats: false,
            show_string_keys: false,
            skip_intro: false,
            custom_end_game_banner: false,
            custom_end_game_score: false,
            custom_end_game_movie: false,
            commander_feedback: true,
            detailed_eva_text: false,
            convenience_help: false,
            building_production_queue: false,
            compact_super_timers: false,
            sidebar: SidebarSize::Normal,
            sidebar_columns: SidebarColumns::Two,
            health_bars: HealthBarMode::Vanilla,
            ares: crate::util::ares_launch::AresSetting::Auto,
            color_vision: ColorVisionMode::Off,
            house_color_position: 0,
            score_position: 4,
            sound_position: 7,
            voice_position: 7,
            game_speed_position: 0,
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

#[derive(Debug)]
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
    /// The in-game menu's Game Speed caption and one value name per
    /// position, and its Sound button caption. Set by
    /// [`Self::set_in_game_controls`].
    game_speed_caption: String,
    game_speed_names: Vec<String>,
    in_game_sound_label: String,
    /// Whether the dialog is the in-game menu's, set with the speed: a
    /// running match greys what it keeps as it began.
    in_match: bool,
    launcher_audio_available: bool,
    pending_cues: Vec<PageCue>,
    pending_events: Vec<LauncherOptionsEvent>,
    pending_result: Option<LauncherParentResult>,
    /// The category whose page shows.
    category: Category,
    pending_update_check: bool,
    /// The controls on the GUI tree: hit tests, captures, hover, the open
    /// list and the status help.
    shell: shell::ShellGui,
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
    /// `0x0055FDB0..0x0056047A` admits initial positions; the sliders name
    /// their positions from the start ([`LauncherTrackbarId::text`]).
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
        use LauncherTrackbarId as Slider;
        for id in [
            Slider::Detail,
            Slider::Difficulty,
            Slider::Scroll,
            Slider::Score,
            Slider::Sound,
            Slider::Voice,
        ] {
            let slot = id.slot(&mut values);
            *slot = admitted_initial_position(i64::from(*slot), id.maximum());
        }
        // VERA's own slider takes the top level for a position past its range:
        // there is no boost above it to ask for, and the profile never holds
        // one either.
        values.house_color_position = values
            .house_color_position
            .min(Slider::HouseColorStrength.maximum());
        Self {
            labels,
            values,
            zoom,
            displays,
            display,
            languages: Vec::new(),
            language: String::new(),
            launch_window_size: (0, 0),
            game_speed_caption: String::new(),
            game_speed_names: Vec::new(),
            in_game_sound_label: String::new(),
            in_match: false,
            launcher_audio_available,
            pending_cues: Vec::new(),
            pending_events: Vec::new(),
            pending_result: None,
            category: Category::ALL[0],
            pending_update_check: false,
            shell: shell::ShellGui::default(),
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
        self.labels = labels;
    }

    /// A caption: VERA's (`VERA:`) from its language files, retail's from
    /// the labels the dialog holds.
    fn text(&self, key: &str) -> std::borrow::Cow<'_, str> {
        if key.starts_with("VERA:") {
            crate::i18n::tr(key).into()
        } else {
            self.labels.get(key).into()
        }
    }

    /// Whether a trackbar window is enabled: `0x00560361..0x00560467` (as
    /// `0x6B6300` does for its own three) enables the audio ones only with an
    /// output device (`Audio__HasOutputDevice @ 0x00407000`). VERA's in-game
    /// menu also greys the ones a match keeps as it began.
    pub(crate) const fn trackbar_enabled(&self, id: LauncherTrackbarId) -> bool {
        (!id.is_audio() || self.launcher_audio_available) && !(self.in_match && id.fixed_in_match())
    }

    /// Whether a button window is enabled: BBB's init disables its Sound
    /// button without an output device (`0x004E2201..0x004E2223`), and the
    /// in-game page's Sound button stands for it. VERA's Check for Updates
    /// leaves the main menu for the update check, which a match has no place
    /// for, and
    /// Edit Layout arranges a match screen, which only a match has.
    pub(crate) const fn button_enabled(&self, id: LauncherButton) -> bool {
        match id {
            LauncherButton::InGameSound => self.launcher_audio_available,
            LauncherButton::CheckForUpdates => !self.in_match,
            LauncherButton::EditLayout => self.in_match,
            _ => true,
        }
    }

    /// gamemd-derived: the parent snapshot consumed by
    /// `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0` maps the six
    /// positions and three normalized checkboxes exactly as below.
    pub(crate) fn pack(&self) -> LauncherOptionsPacked {
        let v = &self.values;
        LauncherOptionsPacked {
            detail_level: if v.detail_position == 0 { 0 } else { 2 },
            difficulty: i32::from(v.difficulty_position),
            unit_action_lines: v.target_lines,
            show_hidden: v.show_hidden,
            tooltips: v.tooltips,
            scroll_rate: 6 - i32::from(v.scroll_position),
            score_volume: f32::from(v.score_position) * 0.1,
            sound_volume: f32::from(v.sound_position) * 0.1,
            voice_volume: f32::from(v.voice_position) * 0.1,
        }
    }

    /// Make this the in-game menu's dialog: give its Game Speed slider its
    /// caption, the name of each position's speed and where it stands, and
    /// the Sound button its caption. A position past the slider's end shows
    /// as its first.
    pub(crate) fn set_in_game_controls(
        &mut self,
        speed_caption: String,
        speed_names: Vec<String>,
        speed_position: u8,
        sound_label: String,
    ) {
        self.game_speed_caption = speed_caption;
        self.game_speed_names = speed_names;
        self.values.game_speed_position = admitted_initial_position(
            speed_position.into(),
            LauncherTrackbarId::GameSpeed.maximum(),
        );
        self.in_game_sound_label = sound_label;
        self.in_match = true;
    }

    pub(crate) fn trackbar_position(&self, id: LauncherTrackbarId) -> u8 {
        let mut values = self.values;
        *id.slot(&mut values)
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` changes
    /// captions and queues preview/cue work only after the integer position
    /// actually changes; the accepted projection remains owned by `0x0055FAA0`.
    fn set_trackbar_position(&mut self, id: LauncherTrackbarId, position: u8) {
        if position > id.maximum() || !self.trackbar_enabled(id) {
            return;
        }
        let slot = id.slot(&mut self.values);
        if *slot == position {
            return;
        }
        *slot = position;
        let volume = f32::from(position) * 0.1;
        // The audio sliders preview instead of the notch click.
        let event = match id {
            LauncherTrackbarId::Score => LauncherOptionsEvent::ScorePreview(volume),
            LauncherTrackbarId::Sound => LauncherOptionsEvent::SoundPreview(volume),
            LauncherTrackbarId::Voice => LauncherOptionsEvent::VoicePreview(volume),
            // 61E609..61E6DD: a changed position plays the cue. Game Speed
            // applies when the menu closes. The strength is a once-per-drag
            // setting rather than a live preview: the battle it applies to
            // loads later, and the accept writes it out.
            id => {
                self.pending_cues.push(PageCue::ListClick);
                if id != LauncherTrackbarId::HouseColorStrength {
                    return;
                }
                LauncherOptionsEvent::HouseColorStrengthChanged(
                    position + crate::rules::house_colors::STRENGTH_MIN,
                )
            }
        };
        self.pending_events.push(event);
    }

    pub(crate) fn shell_checkbox_checked(&self, id: LauncherCheckboxId) -> bool {
        let mut values = self.values;
        *id.slot(&mut values)
    }

    fn set_checkbox(&mut self, id: LauncherCheckboxId, checked: bool) {
        *id.slot(&mut self.values) = checked;
        if id.publishes() {
            self.pending_events
                .push(LauncherOptionsEvent::Toggled(id, checked));
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

    /// A list's pick, published at once: like gamemd's resolution row
    /// (primary-proc slice `0x0055FDB0..0x0056047A`), the accepted projection
    /// `0x0055FAA0` never owns it. The window moves to a picked screen and a
    /// picked language reads from the next frame, so picking the screen or
    /// language already in use changes nothing.
    fn publish(&mut self, event: LauncherOptionsEvent) {
        use LauncherOptionsEvent as E;
        let v = &mut self.values;
        match &event {
            E::ZoomSelected(zoom) => self.zoom = *zoom,
            E::SidebarSelected(size) => v.sidebar = *size,
            E::SidebarColumnsSelected(columns) => v.sidebar_columns = *columns,
            E::HealthBarModeSelected(mode) => v.health_bars = *mode,
            E::AresSelected(setting) => v.ares = *setting,
            E::ColorVisionModeSelected(mode) => v.color_vision = *mode,
            E::DisplaySelected(display) if *display == self.display => return,
            E::DisplaySelected(display) => self.display = display.clone(),
            E::LanguageSelected(tag) if *tag == self.language => return,
            E::LanguageSelected(tag) => self.language = tag.clone(),
            _ => {}
        }
        self.pending_events.push(event);
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
        self.shell.clear();
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
            cues: std::mem::take(&mut self.pending_cues),
            events: std::mem::take(&mut self.pending_events),
            result: self.pending_result.take(),
            check_for_updates: std::mem::take(&mut self.pending_update_check),
        }
    }
}

#[cfg(test)]
impl OptionsDialogState {
    /// The Display list's pick; one the window layer did not list changes
    /// nothing.
    pub(crate) fn select_display(&mut self, display: DisplayTarget) {
        if self.display_choices().contains(&display) {
            self.publish(LauncherOptionsEvent::DisplaySelected(display));
        }
    }

    /// The screens the Display list offers, in the window layer's order.
    pub(crate) fn displays(&self) -> &[DisplayChoice] {
        &self.displays
    }

    /// VERA's team-colour strength as the level the profile stores: the
    /// slider's position lifted onto the [`crate::rules::house_colors`] scale.
    pub(crate) const fn house_color_strength(&self) -> u8 {
        self.values.house_color_position + crate::rules::house_colors::STRENGTH_MIN
    }
}

/// The dialog's pages, top to bottom in the right-hand column under retail's
/// Keyboard. Retail `0xD5` lays its controls out in four bands whose headings
/// (`GUI:DisplayOptions`, `GUI:GameOptions`, `GUI:UIOptions`,
/// `GUI:AudioOptions`) name the first four pages; VERA adds Controls and
/// General. Picking a page swaps the controls in place; one set of values
/// covers them all, and the in-game menu shows the same pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Display,
    Gameplay,
    Interface,
    Audio,
    Controls,
    General,
    /// The in-game menu's first page, which its own Game Controls button
    /// shows: gamemd's Game Controls (`0xBBB`) settings and the three volumes
    /// of its Sound child, each also on its own page. No column button.
    GameControls,
}

impl Category {
    /// Every page of the column, in its order.
    pub(crate) const ALL: [Self; 6] = [
        Self::Display,
        Self::Gameplay,
        Self::Interface,
        Self::Audio,
        Self::Controls,
        Self::General,
    ];

    /// The page's controls, top to bottom: the build's own, as VSync and HD
    /// Map Previews are missing from the browser's.
    pub(crate) fn settings(self) -> Vec<Setting> {
        use LauncherCheckboxId as Check;
        let missing = [
            (HAS_VSYNC, Check::VSync),
            (HAS_HD_MAP_PREVIEWS, Check::HdMapPreviews),
        ];
        let missing = |setting| {
            missing
                .iter()
                .any(|&(has, id)| !has && setting == Setting::Checkbox(id))
        };
        self.all_settings()
            .iter()
            .copied()
            .filter(|&setting| !missing(setting))
            .collect()
    }

    const fn all_settings(self) -> &'static [Setting] {
        use LauncherCheckboxId as Check;
        use LauncherComboId as List;
        use LauncherTrackbarId as Slider;
        match self {
            // The sliders stand above VSync, which the browser build lacks, so
            // both builds put them in the same place: at 640x480's 0.8 fit a
            // slider's place decides whether it keeps its frame art (the note
            // on the row pitches in `shell`).
            Self::Display => &[
                Setting::List(List::Zoom),
                Setting::List(List::Display),
                Setting::Slider(Slider::HouseColorStrength),
                Setting::List(List::ColorVision),
                Setting::Slider(Slider::Detail),
                Setting::Checkbox(Check::FullScreen),
                Setting::Checkbox(Check::VSync),
                Setting::Checkbox(Check::OriginalShape),
                Setting::Checkbox(Check::PerformanceStats),
            ],
            Self::Gameplay => &[
                Setting::Slider(Slider::Difficulty),
                Setting::Checkbox(Check::ShiftQueue),
                Setting::Checkbox(Check::BuildingProductionQueue),
                Setting::Checkbox(Check::AutoRepair),
                Setting::List(List::Ares),
                Setting::Checkbox(Check::CustomEndGameBanner),
                Setting::Checkbox(Check::CustomEndGameScore),
                Setting::Checkbox(Check::CustomEndGameMovie),
                Setting::Checkbox(Check::CommanderFeedback),
            ],
            Self::Interface => &[
                Setting::List(List::Sidebar),
                Setting::List(List::SidebarColumns),
                Setting::Checkbox(Check::HdRadar),
                Setting::Checkbox(Check::SmoothSidebar),
                Setting::List(List::HealthBars),
                Setting::Checkbox(Check::Tooltips),
                Setting::Checkbox(Check::TargetLines),
                Setting::Checkbox(Check::ShowHidden),
                Setting::Checkbox(Check::CompactSuperTimers),
                Setting::Checkbox(Check::DetailedEvaText),
                Setting::Checkbox(Check::ConvenienceHelp),
            ],
            Self::Audio => &[
                Setting::Slider(Slider::Score),
                Setting::Slider(Slider::Sound),
                Setting::Slider(Slider::Voice),
            ],
            Self::Controls => &[
                Setting::Slider(Slider::Scroll),
                Setting::Checkbox(Check::RightClickOrders),
                Setting::Checkbox(Check::MinimapBeacons),
                Setting::Checkbox(Check::MouseWheelZoom),
                Setting::Checkbox(Check::LockCursor),
            ],
            Self::General => &[
                Setting::List(List::Language),
                Setting::Checkbox(Check::YrOriginalStyle),
                Setting::Checkbox(Check::SmoothOnlineMenus),
                Setting::Checkbox(Check::HdMapPreviews),
                Setting::Checkbox(Check::VeraIcon),
                Setting::Checkbox(Check::SkipIntro),
                Setting::Checkbox(Check::ShowStringKeys),
            ],
            Self::GameControls => &[
                Setting::Slider(Slider::GameSpeed),
                Setting::Slider(Slider::Scroll),
                Setting::Slider(Slider::Score),
                Setting::Slider(Slider::Sound),
                Setting::Slider(Slider::Voice),
                Setting::Checkbox(Check::TargetLines),
                Setting::Checkbox(Check::ShowHidden),
                Setting::Checkbox(Check::Tooltips),
            ],
        }
    }

    /// The status help for the page's column button, a VERA language-file
    /// label: retail's string table has nothing for VERA's pages. It says
    /// what the page is for rather than listing its rows. Game Controls has
    /// no column button.
    pub(crate) const fn help(self) -> Option<&'static str> {
        match self {
            Self::Display => Some("VERA:DisplayPageHelp"),
            Self::Gameplay => Some("VERA:GameplayPageHelp"),
            Self::Interface => Some("VERA:InterfacePageHelp"),
            Self::Audio => Some("VERA:AudioPageHelp"),
            Self::Controls => Some("VERA:ControlsPageHelp"),
            Self::General => Some("VERA:GeneralPageHelp"),
            Self::GameControls => None,
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

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::assets::csf_file::CsfFile;
    use crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout;
    use crate::ui::shell::geom::RectPx;
    use crate::ui::shell::trackbar::TrackbarHold;

    /// The frame's cues and events.
    fn drained(state: &mut OptionsDialogState) -> (Vec<PageCue>, Vec<LauncherOptionsEvent>) {
        let output = state.drain_output();
        (output.cues, output.events)
    }

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
            .unwrap()
    }

    /// A click on the icon of the Display page's box `id`.
    fn click_display_box(state: &mut OptionsDialogState, id: LauncherCheckboxId) {
        let layout = page(Category::Display);
        let rect = checkbox(&layout, id);
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        state.shell_mouse_up(rect.x + 5, rect.y + 5, &layout);
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
        assert_eq!(labels.get("GUI:OptionsMenu"), "Localized Options");
        assert_eq!(labels.get("GUI:MainMenu"), "Main Menu");
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
            drained(&mut state),
            (
                vec![PageCue::ListClick],
                vec![LauncherOptionsEvent::HouseColorStrengthChanged(
                    STRENGTH_RETAIL + 4
                )]
            )
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
        assert_eq!(state.slider_step(LauncherTrackbarId::Detail), "Low");
        assert_eq!(state.slider_step(LauncherTrackbarId::Difficulty), "Hard");
        assert_eq!(state.slider_step(LauncherTrackbarId::Scroll), "Slowest");

        state.set_trackbar_position(LauncherTrackbarId::Detail, 1);
        state.set_trackbar_position(LauncherTrackbarId::Difficulty, 2);
        state.set_trackbar_position(LauncherTrackbarId::Scroll, 6);
        assert_eq!(state.slider_step(LauncherTrackbarId::Detail), "High");
        assert_eq!(
            state.slider_step(LauncherTrackbarId::Difficulty),
            "Hard",
            "an unchanged move"
        );
        assert_eq!(state.slider_step(LauncherTrackbarId::Scroll), "Fastest");
        assert_eq!(
            drained(&mut state),
            (vec![PageCue::ListClick, PageCue::ListClick], vec![])
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
        let layout = page(Category::Gameplay);
        let id = LauncherTrackbarId::Difficulty;
        let rect = layout.trackbar_rect(id).unwrap();
        let before = state.trackbar_position(id);
        let left = state.shell_trackbar_thumb_left(id, rect, layout.scale);
        let y = rect.y + rect.h - 1;
        state.shell_mouse_down(rect.x + left + 5, y, &layout);
        assert_eq!(state.trackbar_position(id), before);
        assert_eq!(
            state.shell_trackbar_hold(),
            Some(TrackbarHold { id, dragging: true })
        );
        state.shell_mouse_move(rect.x + rect.w - 1, y - 400, &layout);
        assert_eq!(state.trackbar_position(id), 2);
        state.shell_mouse_up(rect.x + rect.w - 1, y - 400, &layout);
        assert_eq!(state.shell_trackbar_hold(), None);
    }

    #[test]
    fn rail_jump_has_lower_strip_gate_and_holds_without_drag() {
        let mut state = state(true);
        let layout = page(Category::Controls);
        let id = LauncherTrackbarId::Scroll;
        let rect = layout.trackbar_rect(id).unwrap();
        let held = Some(TrackbarHold {
            id,
            dragging: false,
        });
        let x = rect.x + rect.w - 2;
        state.shell_mouse_down(x, rect.y + rect.h - 18, &layout);
        assert_eq!(state.trackbar_position(id), 3, "strict lower edge rejects");
        assert_eq!(state.shell_trackbar_hold(), held);
        state.shell_mouse_up(x, rect.y + rect.h - 18, &layout);
        state.shell_mouse_down(x, rect.y + rect.h - 17, &layout);
        assert_eq!(state.trackbar_position(id), 6);
        assert_eq!(state.shell_trackbar_hold(), held);
        state.shell_mouse_move(rect.x, rect.y + rect.h - 17, &layout);
        assert_eq!(state.trackbar_position(id), 6);
    }

    #[test]
    fn common_audio_gate_rejects_every_audio_input_without_mutation() {
        let mut state = state(false);
        let before = state.values;
        let layout = page(Category::Audio);
        for id in [
            LauncherTrackbarId::Score,
            LauncherTrackbarId::Sound,
            LauncherTrackbarId::Voice,
        ] {
            let rect = layout.trackbar_rect(id).unwrap();
            let (x, y) = (rect.x + 67, rect.y + rect.h - 1);
            state.shell_mouse_down(x, y, &layout);
            state.shell_mouse_move(x, y, &layout);
            state.shell_mouse_up(x, y, &layout);
        }
        assert_eq!(state.values, before);
        assert!(state.drain_output().events.is_empty());
    }

    /// The in-game menu greys what a match keeps as it began, and a press
    /// there changes nothing. The main menu's dialog still takes them.
    #[test]
    fn a_match_greys_what_it_keeps_and_ignores_presses_on_it() {
        let mut main_menu = state(true);
        let mut in_match = state(true);
        in_match.set_in_game_controls(String::new(), Vec::new(), 0, String::new());
        let before = in_match.values;
        for (category, id) in [
            (Category::Gameplay, LauncherTrackbarId::Difficulty),
            (Category::Display, LauncherTrackbarId::HouseColorStrength),
        ] {
            // The track's far end, which the main menu's dialog jumps to.
            let layout = page(category);
            let rect = layout.trackbar_rect(id).unwrap();
            let (x, y) = (
                rect.x + rect.w - 1 - id.plaque_reserve(),
                rect.y + rect.h - 1,
            );
            for dialog in [&mut in_match, &mut main_menu] {
                dialog.shell_mouse_down(x, y, &layout);
                dialog.shell_mouse_up(x, y, &layout);
            }
            assert!(!in_match.trackbar_enabled(id) && main_menu.trackbar_enabled(id));
            assert_ne!(
                main_menu.trackbar_position(id),
                in_match.trackbar_position(id),
                "{id:?}"
            );
        }
        assert_eq!(in_match.values, before);
        assert!(in_match.drain_output().events.is_empty());
        assert!(!in_match.button_enabled(LauncherButton::CheckForUpdates));
        assert!(main_menu.button_enabled(LauncherButton::CheckForUpdates));
        // What the match does take still changes.
        let layout = page(Category::Gameplay);
        let rect = checkbox(&layout, LauncherCheckboxId::ShiftQueue);
        in_match.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert_ne!(in_match.values.shift_queue, before.shift_queue);
    }

    #[test]
    fn checkbox_hit_is_strict_icon_square_and_cue_follows_toggle() {
        let mut state = state(true);
        let layout = page(Category::Interface);
        let rect = checkbox(&layout, LauncherCheckboxId::Tooltips);
        let click = |state: &mut OptionsDialogState, x, y| {
            state.shell_mouse_down(rect.x + x, rect.y + y, &layout);
            state.shell_mouse_up(rect.x + x, rect.y + y, &layout);
        };
        assert!(state.values.tooltips);
        click(&mut state, 17, rect.h - 1);
        assert!(!state.values.tooltips);
        assert_eq!(drained(&mut state), (vec![PageCue::Checkbox], vec![]));
        click(&mut state, 18, 5);
        click(&mut state, 80, 5);
        assert!(!state.values.tooltips);
        assert_eq!(drained(&mut state), (vec![], vec![]));
    }

    #[test]
    fn combo_cues_then_toggles_from_anywhere_on_its_face() {
        use LauncherComboId::{Sidebar, Zoom as ZoomList};
        let mut state = state(true);
        let layout = page(Category::Display);
        let face = layout.combo_rect(ZoomList).unwrap();
        let press = |state: &mut OptionsDialogState, layout: &LauncherOptionsLayout, x, y| {
            state.shell_mouse_down(x, y, layout);
            state.shell_mouse_up(x, y, layout);
        };
        press(&mut state, &layout, face.x + 4, face.y + 5);
        assert_eq!(
            state.shell_open_list(),
            Some(ZoomList),
            "the face opens the list, not only the arrow"
        );
        assert_eq!(drained(&mut state), (vec![PageCue::ComboOpen], vec![]));
        press(&mut state, &layout, face.x + 161, face.y + 5);
        assert_eq!(state.shell_open_list(), None, "a second press closes it");
        press(&mut state, &layout, face.x + 180, face.y + 5);
        assert_eq!(
            state.shell_open_list(),
            None,
            "outside the face does nothing"
        );
        press(&mut state, &layout, face.x + 161, face.y + 5);
        assert_eq!(state.shell_open_list(), Some(ZoomList));
        let _ = state.drain_output();
        let auto = state.shell_popup(&layout).unwrap().row_rect(0);
        press(&mut state, &layout, auto.x + 10, auto.y + 5);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(state.zoom, Zoom::Auto);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboClose],
                vec![LauncherOptionsEvent::ZoomSelected(Zoom::Auto)]
            )
        );

        // The Sidebar list works the same way, on its own value.
        let interface = page(Category::Interface).with_battle_frame((1920, 1080));
        let face = interface.combo_rect(Sidebar).unwrap();
        press(&mut state, &interface, face.x + 4, face.y + 5);
        assert_eq!(state.shell_open_list(), Some(Sidebar));
        let large = state.shell_popup(&interface).unwrap().row_rect(1);
        press(&mut state, &interface, large.x + 10, large.y + 5);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(state.values.sidebar, SidebarSize::Large);
        assert_eq!(state.zoom, Zoom::Auto);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboOpen, PageCue::ComboClose],
                vec![LauncherOptionsEvent::SidebarSelected(SidebarSize::Large)]
            )
        );

        // The Health Bars list works the same way, on its own value.
        use LauncherComboId::HealthBars as HealthBarsList;
        let face = interface.combo_rect(HealthBarsList).unwrap();
        press(&mut state, &interface, face.x + 4, face.y + 5);
        assert_eq!(state.shell_open_list(), Some(HealthBarsList));
        let always = state.shell_popup(&interface).unwrap().row_rect(1);
        press(&mut state, &interface, always.x + 10, always.y + 5);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(state.values.health_bars, HealthBarMode::Always);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboOpen, PageCue::ComboClose],
                vec![LauncherOptionsEvent::HealthBarModeSelected(
                    HealthBarMode::Always
                )]
            )
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
        click_display_box(&mut state, LauncherCheckboxId::FullScreen);
        let on = LauncherOptionsEvent::Toggled(LauncherCheckboxId::FullScreen, true);
        assert_eq!(
            drained(&mut state),
            (vec![PageCue::Checkbox], vec![on.clone()])
        );
        // The window's own button changes it with no event; the next click
        // turns it off again.
        state.sync_full_screen(false);
        click_display_box(&mut state, LauncherCheckboxId::FullScreen);
        assert_eq!(drained(&mut state), (vec![PageCue::Checkbox], vec![on]));
        // The full-screen state is the window's, not part of the accepted pack.
        assert_eq!(state.pack(), self::state(true).pack());
    }

    #[test]
    fn original_shape_box_reports_each_toggle_after_its_cue() {
        let mut state = state(true);
        for checked in [true, false] {
            click_display_box(&mut state, LauncherCheckboxId::OriginalShape);
            assert_eq!(state.values.original_shape, checked);
            let event = LauncherOptionsEvent::Toggled(LauncherCheckboxId::OriginalShape, checked);
            assert_eq!(drained(&mut state), (vec![PageCue::Checkbox], vec![event]));
        }
        // Applied at once, so the accepted pack never carries it.
        click_display_box(&mut state, LauncherCheckboxId::OriginalShape);
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

    /// Every control is on exactly one page of the column: one the table
    /// leaves out could not be reached, and one listed twice would show
    /// twice. The diagnostic test service is on none. The in-game Game
    /// Controls page repeats rows of the column, Game Speed aside.
    #[test]
    fn every_control_is_on_exactly_one_page() {
        use LauncherCheckboxId as Check;
        use LauncherComboId as List;
        use LauncherTrackbarId as Slider;
        let placed: Vec<Setting> = Category::ALL
            .iter()
            .flat_map(|page| page.settings())
            .collect();
        let every: Vec<Setting> = [
            List::Zoom,
            List::Sidebar,
            List::SidebarColumns,
            List::Display,
            List::HealthBars,
            List::ColorVision,
            List::Language,
            List::Ares,
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
                Check::MinimapBeacons,
                Check::LockCursor,
                Check::VeraIcon,
                Check::HdRadar,
                Check::SmoothSidebar,
                Check::SmoothOnlineMenus,
                Check::VSync,
                Check::HdMapPreviews,
                Check::ShiftQueue,
                Check::AutoRepair,
                Check::PerformanceStats,
                Check::ShowStringKeys,
                Check::SkipIntro,
                Check::CustomEndGameBanner,
                Check::CustomEndGameScore,
                Check::CustomEndGameMovie,
                Check::CommanderFeedback,
                Check::DetailedEvaText,
                Check::ConvenienceHelp,
                Check::BuildingProductionQueue,
                Check::CompactSuperTimers,
            ]
            .map(Setting::Checkbox),
        )
        .collect();
        for setting in &every {
            let pages = placed.iter().filter(|placed| *placed == setting).count();
            assert_eq!(pages, 1, "{setting:?}");
        }
        assert_eq!(placed.len(), every.len());
        for setting in Category::GameControls.settings() {
            assert!(
                setting == Setting::Slider(Slider::GameSpeed) || placed.contains(&setting),
                "{setting:?}"
            );
        }
    }
}
