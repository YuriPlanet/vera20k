//! Retained model for the launcher Options dialog `0xD5` and VERA's settings.
//!
//! Every setting is one row of [`ROWS`]: its control, the page it sits on,
//! its caption and its status help. The `shell` child builds a page's nodes
//! from those rows, keeps them in step with [`LauncherOptionsValues`] and
//! turns what the player does into ordered effects; the app paints the nodes
//! with the shared widget painter. The model owns no profile, filesystem,
//! audio device, window or child-dialog callback.

pub(crate) mod shell;

use crate::ui::shell::page::PageCue;
use std::ops::{Index, IndexMut};

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

const BROWSER: bool = cfg!(all(target_arch = "wasm32", target_os = "unknown"));
/// The Display page's VSync (`GpuContext::set_vsync`). The browser presents
/// with the page's own refresh, so its build has no box.
pub(crate) const HAS_VSYNC: bool = !BROWSER;
/// The General page's HD Map Previews (`app::map_picture`). The browser
/// build cannot start the hidden run that draws a map's picture.
pub(crate) const HAS_HD_MAP_PREVIEWS: bool = !BROWSER;
/// The General page's Check for Updates: the desktop updater
/// (`app::updater`) checks once the dialog has slid out. The browser build
/// has no updater, and the in-game menu shows it greyed: no slide-out
/// happens there.
pub(crate) const HAS_CHECK_FOR_UPDATES: bool = !BROWSER;
/// The column's Mods button, which opens VERA's Mods page
/// (`ui::mods_shell`). The browser build has no Mods folder.
pub(crate) const HAS_MODS: bool = !BROWSER;
/// The Esc menu's Set Window Size button. A browser tab's canvas takes its size
/// from the page and an Android window fills the screen, so those builds have
/// none.
pub(crate) const HAS_SET_WINDOW_SIZE: bool = !BROWSER && !cfg!(target_os = "android");

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

/// The `[VERA] Kratos=` list's rows, top to bottom; the first is the default.
pub(crate) fn kratos_choices() -> Vec<crate::util::kratos_launch::KratosSetting> {
    use crate::util::kratos_launch::KratosSetting;
    vec![KratosSetting::Auto, KratosSetting::On, KratosSetting::Off]
}

/// The `[VERA] Kratos=` list's row text.
pub(crate) fn kratos_label(setting: crate::util::kratos_launch::KratosSetting) -> String {
    use crate::util::kratos_launch::KratosSetting;
    crate::i18n::tr(match setting {
        KratosSetting::Auto => "VERA:KratosSettingAuto",
        KratosSetting::On => "VERA:KratosSettingOn",
        KratosSetting::Off => "VERA:KratosSettingOff",
    })
}

/// A Zoom row: the zoom and the battle's size in game pixels in this window,
/// what `App::sync_zoom_pass` will give the match.
pub(crate) fn zoom_label(zoom: Zoom, (width, height): (u32, u32), original_shape: bool) -> String {
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

/// `[VERA] GraphicsAPI=` selects the renderer at startup. With `auto`, the
/// Windows startup record can switch between Vulkan and DirectX 12 after a
/// failed start (`app::persistence::startup_backend`). `opengl` requests the
/// desktop OpenGL compatibility renderer. wgpu's `WGPU_BACKEND` environment
/// variable takes precedence. The Options window's Display page lists it
/// where it chooses anything ([`HAS_GRAPHICS_API`]); a pick applies at the
/// next start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GraphicsApi {
    #[default]
    Auto,
    Vulkan,
    DirectX12,
    OpenGl,
}

impl GraphicsApi {
    /// The list's rows, top to bottom; the first is the default. DirectX 12
    /// stands before Vulkan as the one a Windows player reaches for.
    const ALL: [Self; 4] = [Self::Auto, Self::DirectX12, Self::Vulkan, Self::OpenGl];

    pub(crate) fn from_ini_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "vulkan" | "vk" => Self::Vulkan,
            "dx12" | "directx12" | "directx 12" | "d3d12" => Self::DirectX12,
            "opengl" => Self::OpenGl,
            _ => Self::Auto,
        }
    }

    pub(crate) fn ini_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vulkan => "vulkan",
            Self::DirectX12 => "dx12",
            Self::OpenGl => "opengl",
        }
    }

    /// The API asked for alone, when the key names one.
    pub(crate) fn backend(self) -> Option<wgpu::Backend> {
        match self {
            Self::Auto => None,
            Self::Vulkan => Some(wgpu::Backend::Vulkan),
            Self::DirectX12 => Some(wgpu::Backend::Dx12),
            Self::OpenGl => Some(wgpu::Backend::Gl),
        }
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The row's text.
    pub(crate) fn label(self) -> String {
        crate::i18n::tr(match self {
            Self::Auto => "VERA:GraphicsApiAuto",
            Self::DirectX12 => "VERA:GraphicsApiDirectX12",
            Self::Vulkan => "VERA:GraphicsApiVulkan",
            Self::OpenGl => "VERA:GraphicsApiOpenGl",
        })
    }
}

/// The Graphics API list is on the Display page where the key chooses
/// anything: Windows, with Vulkan and DirectX 12 to choose between and an
/// OpenGL compatibility mode. Metal and the browser have one renderer each.
pub(crate) const HAS_GRAPHICS_API: bool = cfg!(target_os = "windows");

/// A button of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherButton {
    /// Keyboard, Mods or Main Menu: one of gamemd's parent exits.
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

/// The match frame's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameShape {
    /// The window's own: a wider window shows more battlefield.
    #[default]
    Window,
    /// gamemd's 4:3 screen, with black bars on a window of any other shape.
    Original,
}

/// Where a zoomed frame sits in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomGeometry {
    /// Window pixels per 100 source pixels, on each axis; at least 100.
    pub percent: u32,
    pub src_width: u32,
    pub src_height: u32,
    /// The window pixel where source pixel (0, 0) lands.
    pub origin: (u32, u32),
}

impl ZoomGeometry {
    pub fn new(window_width: u32, window_height: u32, percent: u32, shape: FrameShape) -> Self {
        let percent = percent.max(100);
        let mut src_width = (window_width * 100 / percent).max(1);
        let mut src_height = (window_height * 100 / percent).max(1);
        if shape == FrameShape::Original {
            if src_width * 3 > src_height * 4 {
                src_width = (src_height * 4 / 3).max(1);
            } else {
                src_height = (src_width * 3 / 4).max(1);
            }
        }
        let zoom = Self {
            percent,
            src_width,
            src_height,
            origin: (0, 0),
        };
        let (frame_width, frame_height) = zoom.frame_size();
        Self {
            origin: (
                window_width.saturating_sub(frame_width) / 2,
                window_height.saturating_sub(frame_height) / 2,
            ),
            ..zoom
        }
    }

    /// Window pixels per source pixel.
    pub fn scale(self) -> f32 {
        self.percent as f32 / 100.0
    }

    /// Whether every source pixel is a whole block of window pixels.
    pub fn is_sharp(self) -> bool {
        self.percent.is_multiple_of(100)
    }

    /// The scaled frame, in whole window pixels.
    pub fn frame_size(self) -> (u32, u32) {
        (
            self.src_width * self.percent / 100,
            self.src_height * self.percent / 100,
        )
    }

    /// Whether the frame is the whole window, unscaled: no pass is needed.
    pub fn is_whole_window(self, window_width: u32, window_height: u32) -> bool {
        self.percent == 100 && (self.src_width, self.src_height) == (window_width, window_height)
    }

    /// A window point in source pixels. The border clamps to the frame's edge.
    pub fn window_to_source(self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.scale();
        (
            ((x - self.origin.0 as f32) / scale).clamp(0.0, (self.src_width - 1) as f32),
            ((y - self.origin.1 as f32) / scale).clamp(0.0, (self.src_height - 1) as f32),
        )
    }

    /// A source point in window pixels.
    pub fn source_to_window(self, x: f32, y: f32) -> (f32, f32) {
        let scale = self.scale();
        (
            x * scale + self.origin.0 as f32,
            y * scale + self.origin.1 as f32,
        )
    }

    /// The window pixels wholly inside source rect `(x, y, width, height)`
    /// once scaled: one it shares with a neighbour keeps the zoom's blend.
    pub(crate) fn window_rect(self, (x, y, width, height): (u32, u32, u32, u32)) -> [u32; 4] {
        let (left, top) = self.source_to_window(x as f32, y as f32);
        let (right, bottom) = self.source_to_window((x + width) as f32, (y + height) as f32);
        let (frame_width, frame_height) = self.frame_size();
        let left = left.ceil() as u32;
        let top = top.ceil() as u32;
        let right = (right.floor() as u32).min(self.origin.0 + frame_width);
        let bottom = (bottom.floor() as u32).min(self.origin.1 + frame_height);
        [
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ]
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
        crate::i18n::tr(
            [
                "VERA:SidebarNormal",
                "VERA:SidebarLarge",
                "VERA:SidebarExtraLarge",
            ][self as usize],
        )
    }

    /// Frame pixels per 100 sidebar pixels.
    const fn requested_percent(self) -> u32 {
        100 + 50 * self as u32
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

/// The row of `all` a saved `index` names; one this build does not know reads
/// as the first, the original's.
fn saved_row<T: Copy>(all: &[T], index: i32) -> T {
    *usize::try_from(index)
        .ok()
        .and_then(|index| all.get(index))
        .unwrap_or(&all[0])
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
        crate::i18n::tr(
            [
                "VERA:SidebarColumnsTwo",
                "VERA:SidebarColumnsThree",
                "VERA:SidebarColumnsFour",
                "VERA:SidebarColumnsFive",
            ][self as usize],
        )
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The columns the grid lays.
    pub(crate) const fn count(self) -> usize {
        self as usize + 2
    }

    /// The `[VERA] SidebarColumns` value.
    pub(crate) const fn ini_value(self) -> i32 {
        self.count() as i32
    }

    /// A value this build does not know reads as the original's two.
    pub(crate) fn from_ini_value(value: i32) -> Self {
        saved_row(&Self::ALL, value - 2)
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
        crate::i18n::tr(
            [
                "VERA:HealthBarsOriginal",
                "VERA:HealthBarsAll",
                "VERA:HealthBarsNotFull",
            ][self as usize],
        )
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The `[VERA] HealthBars` value: the row.
    pub(crate) const fn ini_value(self) -> i32 {
        self as i32
    }

    /// A value this build does not know reads as the native rule, so a
    /// hand-edited INI never hides the original strip.
    pub(crate) fn from_ini_value(value: i32) -> Self {
        saved_row(&Self::ALL, value)
    }
}

/// The HD art list (`render::hd_switch`, `[VERA] HdArt=`): Off, the original
/// picture, or On, the HD art over the world and the HD sidebar. The HD art
/// key (Page Up) flips the same setting in a match. Presentation only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HdLook {
    #[default]
    Off,
    On,
}

impl HdLook {
    const ALL: [Self; 2] = [Self::Off, Self::On];

    /// The list's rows, top to bottom; the first is the default.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// Whether the row turns HD art on.
    pub(crate) const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }

    /// The row for the switch's state.
    pub(crate) const fn from_on(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }

    pub(crate) fn label(self) -> String {
        crate::i18n::tr(match self {
            Self::Off => "VERA:HdLookOff",
            Self::On => "VERA:HdLookOn",
        })
    }

    /// The rows while no HD art pack runs: Off alone.
    pub(crate) fn choices_without_pack() -> Vec<Self> {
        vec![Self::Off]
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
        crate::i18n::tr(
            [
                "VERA:ColorVisionOff",
                "VERA:ColorVisionGeneric",
                "VERA:ColorVisionDeuteranopia",
                "VERA:ColorVisionProtanopia",
                "VERA:ColorVisionTritanopia",
            ][self as usize],
        )
    }

    /// The list's rows, top to bottom.
    pub(crate) fn choices() -> Vec<Self> {
        Self::ALL.to_vec()
    }

    /// The `[VERA] ColorVisionMode` value: the row.
    pub(crate) const fn ini_value(self) -> i32 {
        self as i32
    }

    /// A value this build does not know reads as `Off`, so a hand-edited INI
    /// never enables a filter the build has no matrix for.
    pub(crate) fn from_ini_value(value: i32) -> Self {
        saved_row(&Self::ALL, value)
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

/// The page's lists. Each list's rows, face and picks come from
/// `OptionsDialogState::shell_list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherComboId {
    Zoom,
    Sidebar,
    SidebarColumns,
    Display,
    HealthBars,
    /// The whole-viewport colour filter.
    ColorVision,
    /// HD art, Off or On (`[VERA] HdArt=`).
    HdLook,
    /// VERA's interface language. Its rows are each language's own name for
    /// itself, so they need no translation.
    Language,
    /// The Ares extension setting, from the next start
    /// (`util::ares_launch::AresSetting`).
    Ares,
    /// The Kratos extension setting, from the next start
    /// (`util::kratos_launch::KratosSetting`).
    Kratos,
    /// The `[VERA] GraphicsAPI=` list: the renderer, from the next start;
    /// on the page where it chooses anything ([`HAS_GRAPHICS_API`]).
    GraphicsApi,
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

/// The sliders, in [`LauncherOptionsValues::positions`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherTrackbarId {
    Detail,
    Difficulty,
    Scroll,
    Score,
    Sound,
    Voice,
    /// How strongly the `[Colors]` team-colour bands are boosted, on the
    /// [`crate::rules::house_colors`] scale: position 0 is the retail strength.
    HouseColorStrength,
    /// Number of build cameos shown per sidebar row.
    SidebarColumns,
    /// The in-game menu's Game Speed, the slider of gamemd's Game Controls
    /// (`0xBBB` control `0x529`). The profile pack leaves it out: the menu
    /// sets it up with [`OptionsDialogState::set_in_game_controls`] and
    /// applies it on close.
    GameSpeed,
}

impl LauncherTrackbarId {
    pub(crate) const COUNT: usize = Self::GameSpeed as usize + 1;

    /// The names a plaque-less slider prints for its positions. The 0x497
    /// init sets each trackbar's range and position; every change sends
    /// WM_HSCROLL (0x0061E609..0x0061E6AF), whose handler (0x0055FF68) names
    /// the position (tables 0x0082A248, of which Detail shows entries 0 and
    /// 2, 0x0082A254 and 0x0082A260), so the resource captions never show.
    /// Game Speed's names come from the in-game menu.
    const fn steps(self) -> &'static [&'static str] {
        match self {
            Self::Detail => &["TXT_LOW", "TXT_HIGH"],
            Self::Difficulty => &["TXT_EASY", "TXT_NORMAL", "TXT_HARD"],
            Self::Scroll => &[
                "TXT_SLOWEST",
                "TXT_SLOWER",
                "TXT_SLOW",
                "TXT_MEDIUM",
                "TXT_FAST",
                "TXT_FASTER",
                "TXT_FASTEST",
            ],
            _ => &[],
        }
    }

    const fn maximum(self) -> u8 {
        match self {
            Self::Score | Self::Sound | Self::Voice => 10,
            Self::GameSpeed => crate::ui::shell::game_speed::MAX_POSITION as u8,
            Self::SidebarColumns => (SidebarColumns::ALL.len() - 1) as u8,
            Self::HouseColorStrength => {
                crate::rules::house_colors::STRENGTH_MAX - crate::rules::house_colors::STRENGTH_MIN
            }
            _ => self.steps().len() as u8 - 1,
        }
    }

    /// The slider's value at position 0. VERA's strength slider runs on the
    /// levels it stands for, so its plaque reads the level.
    const fn value_base(self) -> i32 {
        match self {
            Self::HouseColorStrength => crate::rules::house_colors::STRENGTH_MIN as i32,
            Self::SidebarColumns => 2,
            _ => 0,
        }
    }

    /// The value plaque's reserve; Detail, Difficulty and Scroll turn the
    /// plaque off (`0x4AC` with 0). Sidebar Columns uses a value plaque.
    pub(crate) const fn plaque_reserve(self) -> i32 {
        match self {
            Self::Score | Self::Sound | Self::Voice | Self::SidebarColumns => 50,
            _ => 0,
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
}

/// The boxes, in [`LauncherOptionsValues::checks`] order. Retail's first
/// three apply with the accepted pack; VERA's take effect as they change
/// (`app::persistence::options::launcher`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherCheckboxId {
    Tooltips,
    TargetLines,
    ShowHidden,
    /// The window fills the screen: the window's own state, never the pack.
    FullScreen,
    /// Battles keep gamemd's 4:3 screen shape instead of the window's.
    OriginalShape,
    /// The startup splash, the menu movie, the offline Skirmish background
    /// and the LAN/WOL lobby backgrounds use Yuri's Revenge's art instead of
    /// Red Alert 2's.
    ///
    /// User-set scope: this box switches only those surfaces. Every other
    /// shell screen follows gamemd's ordinary asset lookup (see
    /// `render::skirmish_shell_chrome`), whatever the box says. Widening it
    /// needs the user's explicit request.
    YrOriginalStyle,
    /// Shift + left click on a mobile build cameo queues several copies.
    ShiftQueue,
    /// The player's damaged buildings are sent the Repair order by themselves.
    AutoRepair,
    /// Android applies the saved edition when the player next starts the app.
    #[cfg(target_os = "android")]
    Ra2Mode,
    /// The wheel changes battle view distance.
    MouseWheelZoom,
    /// Alternative controls: left selects, right commands.
    RightClickOrders,
    /// In Place Beacon mode a left click on the radar places the beacon there.
    MinimapBeacons,
    /// The pointer stays inside the game window.
    LockCursor,
    /// The window, taskbar and Dock show VERA's artwork instead of the red star.
    VeraIcon,
    /// A sidebar drawn at a whole multiple draws its radar at that multiple.
    HdRadar,
    /// A sidebar drawn at a whole multiple draws from smoothed xBR copies.
    SmoothSidebar,
    /// The online client's enlarged art draws lightly smoothed.
    SmoothOnlineMenus,
    VSync,
    /// Battles draw at the monitor's refresh rate where draw time allows
    /// (`[VERA] HighRefreshRateInBattle=`), with either VSync setting.
    HighRefreshRateInBattle,
    /// The setup screens' map preview shows VERA's drawing of the whole map.
    HdMapPreviews,
    /// The window's FPS readout: live state no profile stores.
    PerformanceStats,
    /// The next launch skips `Init_Game`'s logo movie (`0x0052C5E2`).
    SkipIntro,
    /// Arms Ctrl+Alt+K, which switches every label between its text and its key.
    ShowStringKeys,
    /// VERA's `[EndGameScreens]` stages, each read when a match ends: the
    /// score page's picture, its music, and waiting for a beaten commander's
    /// losing clip.
    CustomEndGameBanner,
    CustomEndGameScore,
    CustomEndGameMovie,
    /// A mod's or map's computer commanders play their clips in the radar in
    /// skirmish (`docs/ini-commander-personalities.md`).
    CommanderFeedback,
    DetailedEvaText,
    /// Quick status rows (repair/reload countdowns today, more later).
    ConvenienceHelp,
    /// A structure PRODUCE queues behind a running or completed building: a
    /// rule switch the next offline scenario reads (`rules::personal_rules`).
    BuildingProductionQueue,
    /// The super weapon timer list shows six lines and pages the rest.
    CompactSuperTimers,
    /// VERA's box: the super weapon timer list shows one line per team and
    /// super weapon type instead of one per Super. Default off; the next
    /// timer draw reads it.
    GroupedSuperTimers,
}

impl LauncherCheckboxId {
    pub(crate) const COUNT: usize = Self::GroupedSuperTimers as usize + 1;

    /// Whether a toggle publishes: retail's three apply with the accepted
    /// pack (`0x0055FAA0`); VERA's publish each change.
    const fn publishes(self) -> bool {
        !matches!(self, Self::Tooltips | Self::TargetLines | Self::ShowHidden)
    }
}

/// One control of a page, named by the id its value and events go through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Setting {
    List(LauncherComboId),
    Slider(LauncherTrackbarId),
    Checkbox(LauncherCheckboxId),
}

impl Setting {
    /// The setting's caption and status help (`0x00604729..0x00604838` for
    /// retail's).
    pub(crate) fn text(self) -> (&'static str, &'static str) {
        if self == Self::Slider(LauncherTrackbarId::SidebarColumns) {
            return ("VERA:SidebarColumns", "VERA:SidebarColumnsHelp");
        }
        ROWS.iter()
            .find(|row| row.setting == self)
            .map_or(("", ""), |row| (row.caption, row.help))
    }

    /// Whether this build has it: the browser's has no VSync, High Refresh
    /// Rate or HD Map Previews, and only Windows has a Graphics API to pick.
    const fn available(self) -> bool {
        match self {
            Self::Checkbox(
                LauncherCheckboxId::VSync | LauncherCheckboxId::HighRefreshRateInBattle,
            ) => HAS_VSYNC,
            Self::List(LauncherComboId::GraphicsApi) => HAS_GRAPHICS_API,
            Self::Checkbox(LauncherCheckboxId::HdMapPreviews) => HAS_HD_MAP_PREVIEWS,
            _ => true,
        }
    }
}

/// A row of the settings table: a setting, the page it sits on, its caption
/// and its status help, retail's string-table keys or VERA's own labels.
pub(crate) struct Row {
    pub(crate) setting: Setting,
    pub(crate) page: Category,
    pub(crate) caption: &'static str,
    pub(crate) help: &'static str,
}

/// A row; one named by `"Name"` alone takes VERA's `VERA:Name` caption and
/// `VERA:NameHelp` help. `$kind` names both the [`Setting`] and its id enum.
macro_rules! row {
    ($page:ident, $kind:ident($id:ident), $name:literal) => {
        row!(
            $page,
            $kind($id),
            concat!("VERA:", $name),
            concat!("VERA:", $name, "Help")
        )
    };
    ($page:ident, $kind:ident($id:ident), $caption:expr, $help:expr) => {
        Row {
            setting: Setting::$kind($kind::$id),
            page: Category::$page,
            caption: $caption,
            help: $help,
        }
    };
}

/// Every setting, page by page, top to bottom. Retail `0xD5` lays its
/// controls out in four bands whose headings name the first four pages; VERA
/// adds Controls and General. Display groups its kinds: the lists (Windows
/// adds Graphics API after Display, with the other lists about where and how
/// the game draws), the two sliders, then the boxes. The sliders stand above
/// VSync, which the browser build lacks, so both builds put them in the same
/// place: at 640x480's 0.8 fit a slider's place decides whether it keeps its
/// frame art.
pub(crate) const ROWS: &[Row] = {
    use LauncherCheckboxId as Checkbox;
    use LauncherComboId as List;
    use LauncherTrackbarId as Slider;
    &[
        row!(Display, List(Zoom), "Zoom"),
        row!(Display, List(Display), "Display"),
        row!(Display, List(GraphicsApi), "GraphicsApi"),
        row!(Display, List(ColorVision), "ColorVision"),
        row!(Display, Slider(HouseColorStrength), "HouseColorStrength"),
        row!(
            Display,
            Slider(Detail),
            "GUI:VisualDetails",
            "STT:MainOptSliderVisual"
        ),
        row!(Display, Checkbox(FullScreen), "FullScreen"),
        row!(Display, Checkbox(VSync), "VSync"),
        row!(
            Display,
            Checkbox(HighRefreshRateInBattle),
            "HighRefreshRateInBattle"
        ),
        row!(Display, Checkbox(OriginalShape), "OriginalShape"),
        row!(Display, Checkbox(PerformanceStats), "PerformanceStats"),
        row!(
            Gameplay,
            Slider(Difficulty),
            "GUI:Difficulty",
            "STT:MainOptSliderDifficulty"
        ),
        row!(Gameplay, Checkbox(ShiftQueue), "ShiftQueue"),
        row!(
            Gameplay,
            Checkbox(BuildingProductionQueue),
            "BuildingProductionQueue"
        ),
        row!(Gameplay, Checkbox(AutoRepair), "AutoRepair"),
        #[cfg(target_os = "android")]
        row!(Gameplay, Checkbox(Ra2Mode), "Ra2Mode"),
        row!(Gameplay, List(Ares), "AresSetting"),
        row!(Gameplay, List(Kratos), "KratosSetting"),
        row!(
            Gameplay,
            Checkbox(CustomEndGameBanner),
            "CustomEndGameBanner"
        ),
        row!(Gameplay, Checkbox(CustomEndGameScore), "CustomEndGameScore"),
        row!(Gameplay, Checkbox(CustomEndGameMovie), "CustomEndGameMovie"),
        row!(Gameplay, Checkbox(CommanderFeedback), "CommanderFeedback"),
        row!(Interface, List(Sidebar), "Sidebar"),
        row!(Interface, List(SidebarColumns), "SidebarColumns"),
        row!(Interface, Checkbox(HdRadar), "HdRadar"),
        row!(Interface, Checkbox(SmoothSidebar), "SmoothSidebar"),
        row!(Interface, List(HealthBars), "HealthBars"),
        row!(
            Interface,
            Checkbox(Tooltips),
            "GUI:Tooltips",
            "STT:MainOptCBoxTooltips"
        ),
        row!(
            Interface,
            Checkbox(TargetLines),
            "GUI:TargetLines",
            "STT:MainOptCBoxTargetLines"
        ),
        row!(
            Interface,
            Checkbox(ShowHidden),
            "GUI:ShowHidden",
            "STT:MainOptCBoxHidden"
        ),
        row!(
            Interface,
            Checkbox(CompactSuperTimers),
            "CompactSuperTimers"
        ),
        row!(
            Interface,
            Checkbox(GroupedSuperTimers),
            "GroupedSuperTimers"
        ),
        row!(Interface, Checkbox(DetailedEvaText), "DetailedEvaText"),
        row!(Interface, Checkbox(ConvenienceHelp), "ConvenienceHelp"),
        row!(
            Audio,
            Slider(Score),
            "GUI:MusicVolume",
            "STT:MainOptSliderMusic"
        ),
        row!(
            Audio,
            Slider(Sound),
            "GUI:SoundVolume",
            "STT:MainOptSliderSound"
        ),
        row!(
            Audio,
            Slider(Voice),
            "GUI:VoiceVolume",
            "STT:MainOptSliderVoice"
        ),
        row!(
            Controls,
            Slider(Scroll),
            "GUI:ScrollRate",
            "STT:MainOptSliderScroll"
        ),
        row!(Controls, Checkbox(RightClickOrders), "RightClickOrders"),
        row!(Controls, Checkbox(MinimapBeacons), "MinimapBeacons"),
        row!(Controls, Checkbox(MouseWheelZoom), "MouseWheelZoom"),
        row!(Controls, Checkbox(LockCursor), "LockCursor"),
        row!(General, List(Language), "Language"),
        // VERA's HD art switch; Display is full at 640x480.
        row!(General, List(HdLook), "HdLook"),
        row!(General, Checkbox(YrOriginalStyle), "YrOriginalStyle"),
        row!(General, Checkbox(SmoothOnlineMenus), "SmoothOnlineMenus"),
        row!(General, Checkbox(HdMapPreviews), "HdMapPreviews"),
        row!(General, Checkbox(VeraIcon), "VeraIcon"),
        row!(General, Checkbox(SkipIntro), "SkipIntro"),
        row!(General, Checkbox(ShowStringKeys), "ShowStringKeys"),
        // The caption is the in-game menu's own; the help retail's for the Game
        // Options speed slider.
        row!(
            GameControls,
            Slider(GameSpeed),
            "",
            "STT:GameOptSliderSpeed"
        ),
    ]
};

/// The in-game menu's Game Controls page: gamemd's `0xBBB` settings and the
/// three volumes of its Sound child, each also on its own page.
const GAME_CONTROLS: [Setting; 8] = {
    use LauncherCheckboxId as Check;
    use LauncherTrackbarId as Slider;
    [
        Setting::Slider(Slider::GameSpeed),
        Setting::Slider(Slider::Scroll),
        Setting::Slider(Slider::Score),
        Setting::Slider(Slider::Sound),
        Setting::Slider(Slider::Voice),
        Setting::Checkbox(Check::TargetLines),
        Setting::Checkbox(Check::ShowHidden),
        Setting::Checkbox(Check::Tooltips),
    ]
};

/// The dialog's pages, top to bottom in the right-hand column under retail's
/// Keyboard. Picking a page swaps the controls in place; one set of values
/// covers them all, and the in-game menu shows the same pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Display,
    Gameplay,
    Interface,
    Audio,
    Controls,
    General,
    LauncherMain,
    Enhancements,
    Extensions,
    /// The in-game menu's first page, which its own Game Controls button
    /// shows ([`GAME_CONTROLS`]). No column button.
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

    /// The page's controls, top to bottom: the build's own.
    pub(crate) fn settings(self) -> Vec<Setting> {
        let rows = ROWS
            .iter()
            .filter(|row| row.page == self && self != Self::GameControls)
            .map(|row| row.setting);
        let game_controls = GAME_CONTROLS
            .into_iter()
            .filter(|_| self == Self::GameControls);
        rows.chain(game_controls)
            .filter(|setting| setting.available())
            .collect()
    }

    /// The page's name: retail's band heading for the four bands `0xD5` has,
    /// VERA's own for Controls and General, and the in-game menu's own button
    /// for Game Controls.
    pub(crate) const fn heading(self) -> &'static str {
        match self {
            Self::Display => "GUI:DisplayOptions",
            Self::Gameplay => "GUI:GameOptions",
            Self::Interface => "GUI:UIOptions",
            Self::Audio => "GUI:AudioOptions",
            Self::Controls => "VERA:Controls",
            Self::General => "VERA:General",
            Self::LauncherMain => "GUI:OptionsMenu",
            Self::Enhancements => "VERA:EnhancementsDebug",
            Self::Extensions => "VERA:ExtensionsOptions",
            Self::GameControls => "GUI:GameControls",
        }
    }

    /// The status help for the page's column button: what the page is for.
    /// Game Controls has no column button.
    pub(crate) const fn help(self) -> Option<&'static str> {
        match self {
            Self::Display => Some("VERA:DisplayPageHelp"),
            Self::Gameplay => Some("VERA:GameplayPageHelp"),
            Self::Interface => Some("VERA:InterfacePageHelp"),
            Self::Audio => Some("VERA:AudioPageHelp"),
            Self::Controls => Some("VERA:ControlsPageHelp"),
            Self::General => Some("VERA:GeneralPageHelp"),
            Self::Enhancements => Some("VERA:EnhancementsDebugHelp"),
            Self::Extensions => Some("VERA:ExtensionsOptionsHelp"),
            Self::LauncherMain | Self::GameControls => None,
        }
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
    /// Played from the next start.
    KratosSelected(crate::util::kratos_launch::KratosSetting),
    /// The `[VERA] GraphicsAPI=` list's pick, read at the next start.
    GraphicsApiSelected(GraphicsApi),
    ColorVisionModeSelected(ColorVisionMode),
    /// The HD art list's pick, applied to the next frame.
    HdLookSelected(HdLook),
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

/// The dialog's values: a state per box and a position per slider, indexed by
/// their ids, and the lists' choices. Zoom, Display and Language live on the
/// dialog, which knows their choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherOptionsValues {
    pub(crate) checks: [bool; LauncherCheckboxId::COUNT],
    /// VERA's team-colour strength stands at its level less
    /// [`crate::rules::house_colors::STRENGTH_MIN`]: position 0 is gamemd's
    /// own bands.
    pub(crate) positions: [u8; LauncherTrackbarId::COUNT],
    pub(crate) sidebar: SidebarSize,
    pub(crate) sidebar_columns: SidebarColumns,
    pub(crate) health_bars: HealthBarMode,
    pub(crate) ares: crate::util::ares_launch::AresSetting,
    pub(crate) kratos: crate::util::kratos_launch::KratosSetting,
    /// The Graphics API list's choice; it applies from the next start.
    pub(crate) graphics_api: GraphicsApi,
    pub(crate) color_vision: ColorVisionMode,
    /// Whether HD art is on.
    pub(crate) hd_look: HdLook,
    /// Whether the running mod brings an HD art pack: without one the list
    /// offers only Off and its help names what it needs.
    pub(crate) hd_look_available: bool,
}

impl Default for LauncherOptionsValues {
    fn default() -> Self {
        use LauncherCheckboxId as C;
        let mut checks = [false; C::COUNT];
        for on in [
            C::Tooltips,
            C::TargetLines,
            C::VSync,
            C::HdMapPreviews,
            C::CommanderFeedback,
        ] {
            checks[on as usize] = true;
        }
        Self {
            checks,
            positions: [1, 1, 3, 4, 7, 7, 0, 0, 0],
            sidebar: SidebarSize::default(),
            sidebar_columns: SidebarColumns::default(),
            health_bars: HealthBarMode::default(),
            ares: Default::default(),
            kratos: Default::default(),
            graphics_api: GraphicsApi::default(),
            color_vision: ColorVisionMode::default(),
            hd_look: HdLook::default(),
            hd_look_available: false,
        }
    }
}

impl Index<LauncherCheckboxId> for LauncherOptionsValues {
    type Output = bool;
    fn index(&self, id: LauncherCheckboxId) -> &bool {
        &self.checks[id as usize]
    }
}

impl IndexMut<LauncherCheckboxId> for LauncherOptionsValues {
    fn index_mut(&mut self, id: LauncherCheckboxId) -> &mut bool {
        &mut self.checks[id as usize]
    }
}

impl Index<LauncherTrackbarId> for LauncherOptionsValues {
    type Output = u8;
    fn index(&self, id: LauncherTrackbarId) -> &u8 {
        &self.positions[id as usize]
    }
}

impl IndexMut<LauncherTrackbarId> for LauncherOptionsValues {
    fn index_mut(&mut self, id: LauncherTrackbarId) -> &mut u8 {
        &mut self.positions[id as usize]
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
    /// when this dialog was built, and the list's pick.
    displays: Vec<DisplayChoice>,
    display: DisplayTarget,
    /// The languages the Language list offers, and the tag of the one in
    /// use. Set by [`Self::set_languages`]; empty until then.
    languages: Vec<crate::i18n::LanguageChoice>,
    language: String,
    /// The size this launch opened the window at, which the Esc menu's Set
    /// Window Size button names.
    launch_window_size: (u32, u32),
    /// The in-game menu's Game Speed caption and one value name per
    /// position, and its Sound button caption.
    game_speed_caption: String,
    game_speed_names: Vec<String>,
    in_game_sound_label: String,
    /// Whether the dialog is the in-game menu's: a running match greys what
    /// it keeps as it began.
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
    /// their positions from the start. `displays` and `display` are VERA's own
    /// Display list, which gamemd's `0xD5` has no control for.
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
            values[id] = admitted_initial_position(i64::from(values[id]), id.maximum());
        }
        // VERA's own slider takes the top level for a position past its range:
        // there is no boost above it to ask for.
        let strength = Slider::HouseColorStrength;
        values[strength] = values[strength].min(strength.maximum());
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
    /// for, and Edit Layout arranges a match screen, which only a match has.
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
        use LauncherCheckboxId as C;
        use LauncherTrackbarId as S;
        let v = &self.values;
        LauncherOptionsPacked {
            detail_level: if v[S::Detail] == 0 { 0 } else { 2 },
            difficulty: i32::from(v[S::Difficulty]),
            unit_action_lines: v[C::TargetLines],
            show_hidden: v[C::ShowHidden],
            tooltips: v[C::Tooltips],
            scroll_rate: 6 - i32::from(v[S::Scroll]),
            score_volume: f32::from(v[S::Score]) * 0.1,
            sound_volume: f32::from(v[S::Sound]) * 0.1,
            voice_volume: f32::from(v[S::Voice]) * 0.1,
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
        let speed = LauncherTrackbarId::GameSpeed;
        self.game_speed_caption = speed_caption;
        self.game_speed_names = speed_names;
        self.values[speed] = admitted_initial_position(speed_position.into(), speed.maximum());
        self.in_game_sound_label = sound_label;
        self.in_match = true;
    }

    pub(crate) fn trackbar_position(&self, id: LauncherTrackbarId) -> u8 {
        if id == LauncherTrackbarId::SidebarColumns {
            return (self.values.sidebar_columns.count() - 2) as u8;
        }
        self.values[id]
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` changes
    /// captions and queues preview/cue work only after the integer position
    /// actually changes; the accepted projection remains owned by `0x0055FAA0`.
    fn set_trackbar_position(&mut self, id: LauncherTrackbarId, position: u8) {
        if position > id.maximum()
            || !self.trackbar_enabled(id)
            || self.trackbar_position(id) == position
        {
            return;
        }
        if id != LauncherTrackbarId::SidebarColumns {
            self.values[id] = position;
        }
        let volume = f32::from(position) * 0.1;
        // The audio sliders preview instead of the notch click.
        let event = match id {
            LauncherTrackbarId::Score => LauncherOptionsEvent::ScorePreview(volume),
            LauncherTrackbarId::Sound => LauncherOptionsEvent::SoundPreview(volume),
            LauncherTrackbarId::Voice => LauncherOptionsEvent::VoicePreview(volume),
            LauncherTrackbarId::SidebarColumns => {
                self.pending_cues.push(PageCue::ListClick);
                let columns = SidebarColumns::ALL[usize::from(position)];
                self.values.sidebar_columns = columns;
                LauncherOptionsEvent::SidebarColumnsSelected(columns)
            }
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
        self.values[id]
    }

    fn set_checkbox(&mut self, id: LauncherCheckboxId, checked: bool) {
        self.values[id] = checked;
        if id.publishes() {
            self.pending_events
                .push(LauncherOptionsEvent::Toggled(id, checked));
        }
    }

    /// Follow a full-screen change the page didn't make (the window's own
    /// title-bar button), so the box shows what the window is.
    pub(crate) fn sync_full_screen(&mut self, full_screen: bool) {
        self.values[LauncherCheckboxId::FullScreen] = full_screen;
    }

    /// Follow Ctrl+Alt+F while the page is open, so the box shows the readout.
    pub(crate) fn sync_performance_stats(&mut self, shown: bool) {
        self.values[LauncherCheckboxId::PerformanceStats] = shown;
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
            E::KratosSelected(setting) => v.kratos = *setting,
            E::GraphicsApiSelected(api) => v.graphics_api = *api,
            E::ColorVisionModeSelected(mode) => v.color_vision = *mode,
            E::HdLookSelected(look) => v.hd_look = *look,
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

    /// VERA's team-colour strength as the level the profile stores: the
    /// slider's position lifted onto the [`crate::rules::house_colors`] scale.
    pub(crate) fn house_color_strength(&self) -> u8 {
        self.values[LauncherTrackbarId::HouseColorStrength]
            + crate::rules::house_colors::STRENGTH_MIN
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
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::assets::csf_file::CsfFile;
    use crate::ui::main_menu_dialogs::options::shell::LauncherOptionsLayout;
    use crate::ui::shell::geom::RectPx;

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
        layout.checkbox_rect(id).unwrap()
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
        let mut values = LauncherOptionsValues::default();
        // A hand-edited profile or a stale caller can ask for more.
        values[LauncherTrackbarId::HouseColorStrength] = 200;
        let mut state = OptionsDialogState::new(
            labels(),
            values,
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
        let mut values = LauncherOptionsValues::default();
        values[LauncherTrackbarId::Detail] = 0;
        values[LauncherTrackbarId::Difficulty] = 2;
        values[LauncherTrackbarId::Scroll] = 0;
        let mut state = OptionsDialogState::new(
            labels(),
            values,
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
            Some((id, true))
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
        let held = Some((id, false));
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
        assert_ne!(
            in_match.values[LauncherCheckboxId::ShiftQueue],
            before[LauncherCheckboxId::ShiftQueue]
        );
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
        assert!(state.values[LauncherCheckboxId::Tooltips]);
        click(&mut state, 17, rect.h - 1);
        assert!(!state.values[LauncherCheckboxId::Tooltips]);
        assert_eq!(drained(&mut state), (vec![PageCue::Checkbox], vec![]));
        click(&mut state, 18, 5);
        click(&mut state, 80, 5);
        assert!(!state.values[LauncherCheckboxId::Tooltips]);
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

    /// The Graphics API list publishes its pick for the profile, as the Ares
    /// list does: the game reads it at its next start. Its rows are the
    /// key's four values, Auto first, each naming its own value.
    #[test]
    fn graphics_api_list_publishes_its_pick_for_the_next_start() {
        let values: Vec<&str> = GraphicsApi::choices()
            .iter()
            .map(|api| api.ini_value())
            .collect();
        assert_eq!(values, ["auto", "dx12", "vulkan", "opengl"]);
        for api in GraphicsApi::choices() {
            assert_eq!(GraphicsApi::from_ini_value(api.ini_value()), api);
        }
        if !HAS_GRAPHICS_API {
            return;
        }
        use LauncherComboId::GraphicsApi as GraphicsApiList;
        let mut state = state(true);
        let display = page(Category::Display);
        let press = |state: &mut OptionsDialogState, layout: &LauncherOptionsLayout, x, y| {
            state.shell_mouse_down(x, y, layout);
            state.shell_mouse_up(x, y, layout);
        };
        let face = display.combo_rect(GraphicsApiList).unwrap();
        press(&mut state, &display, face.x + 4, face.y + 5);
        assert_eq!(state.shell_open_list(), Some(GraphicsApiList));
        let dx12 = state.shell_popup(&display).unwrap().row_rect(1);
        press(&mut state, &display, dx12.x + 10, dx12.y + 5);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(state.values.graphics_api, GraphicsApi::DirectX12);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboOpen, PageCue::ComboClose],
                vec![LauncherOptionsEvent::GraphicsApiSelected(
                    GraphicsApi::DirectX12
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
            assert_eq!(state.values[LauncherCheckboxId::OriginalShape], checked);
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
    /// twice. The in-game Game Controls page repeats rows of the column,

    /// HD art is Off or On, and Off alone while no HD art pack runs, so it
    /// cannot be turned on without one; the list starts Off.
    #[test]
    fn hd_look_offers_on_only_with_a_pack() {
        assert_eq!(HdLook::choices(), [HdLook::Off, HdLook::On]);
        assert_eq!(HdLook::choices_without_pack(), [HdLook::Off]);
        assert_eq!(HdLook::from_on(true), HdLook::On);
        let values = LauncherOptionsValues::default();
        assert_eq!(values.hd_look, HdLook::Off);
        assert!(!values.hd_look_available);
    }
    /// Game Speed aside.
    #[test]
    fn every_control_is_on_exactly_one_page() {
        let placed: Vec<Setting> = Category::ALL
            .iter()
            .flat_map(|page| page.settings())
            .collect();
        let count = |kind: fn(&Setting) -> bool| placed.iter().filter(|s| kind(s)).count();
        let boxes = LauncherCheckboxId::COUNT
            - 2 * usize::from(!HAS_VSYNC)
            - usize::from(!HAS_HD_MAP_PREVIEWS);
        assert_eq!(count(|s| matches!(s, Setting::Checkbox(_))), boxes);
        // Every slider but Game Speed and Sidebar Columns, whose slider only
        // the launcher's Enhancements page shows (`shell::launcher_pages`);
        // these pages keep its list. All eight lists, and Graphics API where
        // the build has it.
        assert_eq!(
            count(|s| matches!(s, Setting::Slider(_))),
            LauncherTrackbarId::COUNT - 2
        );
        assert_eq!(
            count(|s| matches!(s, Setting::List(_))),
            // HD art's list included.
            10 + usize::from(HAS_GRAPHICS_API)
        );
        for (index, setting) in placed.iter().enumerate() {
            assert!(!placed[index + 1..].contains(setting), "{setting:?}");
        }
        for setting in Category::GameControls.settings() {
            assert!(
                setting == Setting::Slider(LauncherTrackbarId::GameSpeed)
                    || placed.contains(&setting),
                "{setting:?}"
            );
        }
    }

    #[test]
    fn zoom_centres_whole_blocks_and_maps_points_both_ways() {
        // An even window: the frame covers it exactly.
        let wide = ZoomGeometry::new(3440, 1440, 200, FrameShape::Window);
        assert_eq!(
            (wide.src_width, wide.src_height, wide.origin),
            (1720, 720, (0, 0))
        );
        // An odd window keeps a border under one block, split evenly.
        let odd = ZoomGeometry::new(1925, 1083, 300, FrameShape::Window);
        assert_eq!(
            (odd.src_width, odd.src_height, odd.origin),
            (641, 361, (1, 0))
        );
        assert_eq!(odd.source_to_window(10.0, 20.0), (31.0, 60.0));
        assert_eq!(odd.window_to_source(31.0, 60.0), (10.0, 20.0));
        // The border clamps to the frame's edge pixels.
        assert_eq!(odd.window_to_source(0.0, 1082.0), (0.0, 360.0));
        // 100% is the identity.
        let plain = ZoomGeometry::new(800, 600, 100, FrameShape::Window);
        assert_eq!(
            (plain.src_width, plain.src_height, plain.origin),
            (800, 600, (0, 0))
        );
        assert_eq!(plain.window_to_source(399.5, 299.5), (399.5, 299.5));
        assert!(plain.is_whole_window(800, 600));
        assert!(!wide.is_whole_window(3440, 1440));
        assert!(wide.is_sharp() && odd.is_sharp());
    }

    #[test]
    fn a_step_between_whole_zooms_scales_smoothly_from_the_centre() {
        let half = ZoomGeometry::new(1920, 1080, 150, FrameShape::Window);
        assert_eq!(
            (half.src_width, half.src_height, half.origin),
            (1280, 720, (0, 0))
        );
        assert!(!half.is_sharp());
        assert_eq!(half.frame_size(), (1920, 1080));
        assert_eq!(half.source_to_window(10.0, 20.0), (15.0, 30.0));
        assert_eq!(half.window_to_source(15.0, 30.0), (10.0, 20.0));
        // A window the step doesn't divide keeps a border under one source
        // pixel.
        let odd = ZoomGeometry::new(1366, 768, 150, FrameShape::Window);
        assert_eq!((odd.src_width, odd.src_height), (910, 512));
        assert_eq!((odd.frame_size(), odd.origin), ((1365, 768), (0, 0)));
        assert!(!odd.is_whole_window(1366, 768));
    }

    #[test]
    fn original_shape_keeps_a_4_3_frame_between_centred_bars() {
        let frame = |w, h, percent| {
            let zoom = ZoomGeometry::new(w, h, percent, FrameShape::Original);
            (zoom.src_width, zoom.src_height, zoom.origin)
        };
        // A widescreen gets bars at the sides, at any zoom.
        assert_eq!(frame(1920, 1080, 100), (1440, 1080, (240, 0)));
        assert_eq!(frame(1920, 1080, 200), (720, 540, (240, 0)));
        assert_eq!(frame(1920, 1080, 150), (960, 720, (240, 0)));
        // A taller screen gets them above and below.
        assert_eq!(frame(1280, 1024, 100), (1280, 960, (0, 32)));
        // A 4:3 window is unchanged, and at 100% needs no pass.
        assert_eq!(frame(1600, 1200, 200), (800, 600, (0, 0)));
        let whole = ZoomGeometry::new(1024, 768, 100, FrameShape::Original);
        assert!(whole.is_whole_window(1024, 768));
        let barred = ZoomGeometry::new(1920, 1080, 100, FrameShape::Original);
        assert!(!barred.is_whole_window(1920, 1080));
        // The bars clamp to the frame's edge, like the zoom's own border.
        assert_eq!(barred.window_to_source(100.0, 500.0), (0.0, 500.0));
        assert_eq!(barred.source_to_window(0.0, 0.0), (240.0, 0.0));
    }
}
