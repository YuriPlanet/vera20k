//! Retained semantic model for the retail launcher Options dialog `0xD5`.
//!
//! The parent owns bounded control state and emits ordered effects. It owns no
//! profile, filesystem, audio device, window, or child-dialog callback.
//! The `shell` child supplies retail resource geometry and physical-pixel input;
//! the app shell renderer paints that model through the shared retail compositor.

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
pub(crate) const LAUNCHER_LABEL_SPECS: [(&str, &str); 30] = [
    ("GUI:OptionsMenu", "Options"),
    ("GUI:MainMenu", "Main Menu"),
    ("GUI:Keyboard", "Keyboard"),
    ("GUI:Network", "Network"),
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

/// VERA's own captions and status help on this page. The retail string table
/// has nothing for full screen, zoom, the battle shape or the Red Alert 2
/// loading screen and menu movie, so these are English everywhere.
pub(crate) const ZOOM_LABEL: &str = "Zoom";
pub(crate) const ZOOM_HELP: &str =
    "How big battles look. Auto picks the best size for your screen.";
pub(crate) const SIDEBAR_LABEL: &str = "Sidebar";
pub(crate) const SIDEBAR_HELP: &str = "How big the sidebar and bottom bar are in battles.";
pub(crate) const FULL_SCREEN_LABEL: &str = "Full Screen";
pub(crate) const FULL_SCREEN_HELP: &str = "Fill the whole screen. Untick it to play in a window.";
pub(crate) const ORIGINAL_SHAPE_LABEL: &str = "Original 4:3 Shape";
pub(crate) const ORIGINAL_SHAPE_HELP: &str =
    "Battles keep the original 4:3 shape, with bars on wide screens.";
pub(crate) const RA2_LOOK_LABEL: &str = "RA2 Loading & Menu Movie";
pub(crate) const RA2_LOOK_HELP: &str = "Show the Red Alert 2 loading screen and menu movie.";
pub(crate) const SHIFT_QUEUE_LABEL: &str = "Shift-Click Batch Queue";
pub(crate) const DEVELOPER_OPTIONS_LABEL: &str = "Developer Options";
pub(crate) const WOL_TEST_SERVICE_LABEL: &str = "Offline Internet Test Service";
pub(crate) const WOL_TEST_SERVICE_HELP: &str =
    "Internet opens with stand-in servers, lobbies and players, for testing its screens.";
pub(crate) const SHIFT_QUEUE_HELP: &str =
    "Shift-click an infantry, vehicle or aircraft cameo to queue several at once.";

/// VERA's battle zoom: the Options combo that took the place of gamemd's
/// resolution list (`0x005601A0..0x00560270`). The window keeps the size the
/// player gives it, and a zoomed match draws its whole frame (battlefield,
/// sidebar, cursor) at the window size divided by the zoom, then scales it up.
/// A whole multiple of 100% scales by whole pixels and stays sharp; the steps
/// between blend neighbouring pixels. The menus fit the window on their own.
/// Saved as `[VERA] Zoom` in RA2MD.INI, a section gamemd never reads.
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

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Large => "Large",
            Self::ExtraLarge => "XL",
        }
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

/// The page's two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherComboId {
    Zoom,
    Sidebar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLabels {
    pub(crate) options: String,
    pub(crate) main_menu: String,
    pub(crate) keyboard: String,
    pub(crate) network: String,
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
            network: label(3),
            display_options: label(4),
            game_options: label(5),
            ui_options: label(6),
            audio_options: label(7),
            visual_details: label(8),
            difficulty: label(9),
            tooltips: label(10),
            scroll_rate: label(11),
            target_lines: label(12),
            show_hidden: label(13),
            music_volume: label(14),
            sound_volume: label(15),
            voice_volume: label(16),
            blank: label(17),
            low: label(18),
            high: label(19),
            easy: label(20),
            normal: label(21),
            hard: label(22),
            scroll_tokens: [
                label(23),
                label(24),
                label(25),
                label(26),
                label(27),
                label(28),
                label(29),
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
}

impl LauncherTrackbarId {
    const fn maximum(self) -> u8 {
        match self {
            Self::Detail => 1,
            Self::Difficulty => 2,
            Self::Scroll => 6,
            Self::Score | Self::Sound | Self::Voice => 10,
        }
    }

    /// The value plaque's reserve; Detail, Difficulty and Scroll turn the
    /// plaque off (`0x4AC` with 0).
    pub(crate) const fn plaque_reserve(self) -> i32 {
        match self {
            Self::Detail | Self::Difficulty | Self::Scroll => 0,
            Self::Score | Self::Sound | Self::Voice => 50,
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
    /// VERA's own box: the startup splash and the menu movie use Red Alert 2's
    /// art instead of Yuri's Revenge's. The movie changes at once; the splash,
    /// drawn before any menu, at the next launch.
    Ra2Look,
    /// VERA's own box: Shift + left click on a mobile build cameo queues
    /// several copies instead of gamemd's one. It applies at once.
    ShiftQueue,
    /// VERA's developer box: Internet answers from the stand-in WOL test
    /// service instead of showing that WOLAPI is missing. The next Internet
    /// visit reads it.
    WolTestService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherCue {
    MainButton,
    GenericClick,
    Checkbox,
    ComboOpen,
    ComboClose,
}

#[derive(Debug, Clone, PartialEq)]
/// gamemd-derived: notification order in launcher primary-proc slice
/// `0x0055FDB0..0x0056047A`; owner `OptionsClass__ShowLauncherDialog @
/// 0x0055FC80` consumes these before any parent-result teardown.
pub(crate) enum LauncherOptionsEvent {
    Cue(LauncherCue),
    ZoomSelected(Zoom),
    SidebarSelected(SidebarSize),
    FullScreenChanged(bool),
    OriginalShapeChanged(bool),
    Ra2LookChanged(bool),
    ShiftQueueChanged(bool),
    WolTestServiceChanged(bool),
    ScorePreview(f32),
    SoundPreview(f32),
    VoicePreview(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// gamemd-derived: the distinct exits owned by
/// `OptionsClass__ShowLauncherDialog @ 0x0055FC80`; every accepted result first
/// projects through `OptionsClass__ApplyFromLauncherDialog @ 0x0055FAA0`.
pub(crate) enum LauncherParentResult {
    Back,
    Network,
    Keyboard,
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LauncherOptionsFrameOutput {
    pub(crate) events: Vec<LauncherOptionsEvent>,
    pub(crate) result: Option<LauncherParentResult>,
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
    pub(crate) ra2_look: bool,
    pub(crate) shift_queue: bool,
    pub(crate) wol_test_service: bool,
    /// The Sidebar list's choice; like Zoom, it applies at once.
    pub(crate) sidebar: SidebarSize,
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
            ra2_look: false,
            shift_queue: false,
            wol_test_service: false,
            sidebar: SidebarSize::Normal,
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
    shell_interaction: shell::ShellInteraction,
}

impl Default for OptionsDialogState {
    fn default() -> Self {
        let labels = LauncherOptionsLabels::resolve(&|_| None);
        Self::new(labels, LauncherOptionsValues::default(), Zoom::Auto, false)
    }
}

impl OptionsDialogState {
    /// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80`
    /// creates each fresh `0xD5` parent, while primary-proc slice
    /// `0x0055FDB0..0x0056047A` admits initial positions and preserves the
    /// resource Detail/Difficulty/Scroll captions until a changed notification.
    pub(crate) fn new(
        labels: LauncherOptionsLabels,
        mut values: LauncherOptionsValues,
        zoom: Zoom,
        launcher_audio_available: bool,
    ) -> Self {
        values.detail_position = admitted_initial_position(i64::from(values.detail_position), 1);
        values.difficulty_position =
            admitted_initial_position(i64::from(values.difficulty_position), 2);
        values.scroll_position = admitted_initial_position(i64::from(values.scroll_position), 6);
        values.score_position = admitted_initial_position(i64::from(values.score_position), 10);
        values.sound_position = admitted_initial_position(i64::from(values.sound_position), 10);
        values.voice_position = admitted_initial_position(i64::from(values.voice_position), 10);
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
            open_combo: None,
            launcher_audio_available,
            capture: None,
            pending_events: Vec::new(),
            pending_result: None,
            shell_interaction: shell::ShellInteraction::default(),
        }
    }

    pub(crate) const fn launcher_audio_available(&self) -> bool {
        self.launcher_audio_available
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

    pub(crate) fn trackbar_position(&self, id: LauncherTrackbarId) -> u8 {
        match id {
            LauncherTrackbarId::Detail => self.values.detail_position,
            LauncherTrackbarId::Difficulty => self.values.difficulty_position,
            LauncherTrackbarId::Scroll => self.values.scroll_position,
            LauncherTrackbarId::Score => self.values.score_position,
            LauncherTrackbarId::Sound => self.values.sound_position,
            LauncherTrackbarId::Voice => self.values.voice_position,
        }
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` changes
    /// captions and queues preview/cue work only after the integer position
    /// actually changes; the accepted projection remains owned by `0x0055FAA0`.
    fn set_trackbar_position(&mut self, id: LauncherTrackbarId, position: u8) {
        if position > id.maximum() || (id.is_audio() && !self.launcher_audio_available) {
            return;
        }
        let slot = match id {
            LauncherTrackbarId::Detail => &mut self.values.detail_position,
            LauncherTrackbarId::Difficulty => &mut self.values.difficulty_position,
            LauncherTrackbarId::Scroll => &mut self.values.scroll_position,
            LauncherTrackbarId::Score => &mut self.values.score_position,
            LauncherTrackbarId::Sound => &mut self.values.sound_position,
            LauncherTrackbarId::Voice => &mut self.values.voice_position,
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
            LauncherTrackbarId::Score => self.pending_events.push(
                LauncherOptionsEvent::ScorePreview(f32::from(position) * 0.1),
            ),
            LauncherTrackbarId::Sound => self.pending_events.push(
                LauncherOptionsEvent::SoundPreview(f32::from(position) * 0.1),
            ),
            LauncherTrackbarId::Voice => self.pending_events.push(
                LauncherOptionsEvent::VoicePreview(f32::from(position) * 0.1),
            ),
        }
    }

    /// A press on one trackbar ([`trackbar_press`]).
    pub(crate) fn trackbar_mouse_down(
        &mut self,
        id: LauncherTrackbarId,
        frame: PhysicalControlFrame,
    ) {
        if id.is_audio() && !self.launcher_audio_available {
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
            LauncherCheckboxId::Ra2Look => &mut self.values.ra2_look,
            LauncherCheckboxId::ShiftQueue => &mut self.values.shift_queue,
            LauncherCheckboxId::WolTestService => &mut self.values.wol_test_service,
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
            LauncherCheckboxId::Ra2Look => self
                .pending_events
                .push(LauncherOptionsEvent::Ra2LookChanged(checked)),
            LauncherCheckboxId::ShiftQueue => self
                .pending_events
                .push(LauncherOptionsEvent::ShiftQueueChanged(checked)),
            LauncherCheckboxId::WolTestService => self
                .pending_events
                .push(LauncherOptionsEvent::WolTestServiceChanged(checked)),
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
    /// Back/Network/Keyboard result only on the later activation release.
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

    /// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80` gives
    /// Back, Network, Keyboard, and terminal completion distinct parent
    /// results; the first admitted result owns the frame's teardown path.
    pub(crate) fn request_result(&mut self, result: LauncherParentResult) {
        if self.pending_result.is_none() {
            self.pending_result = Some(result);
        }
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` observes
    /// control notifications in order before owner `0x0055FC80` dispatches a
    /// parent result through accepted projection `0x0055FAA0`.
    pub(crate) fn drain_output(&mut self) -> LauncherOptionsFrameOutput {
        LauncherOptionsFrameOutput {
            events: std::mem::take(&mut self.pending_events),
            result: self.pending_result.take(),
        }
    }
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
    fn escape_is_no_event_and_first_parent_result_wins_one_frame() {
        let mut state = state(true);
        assert!(state.drain_output().events.is_empty());
        state.request_result(LauncherParentResult::Network);
        state.request_result(LauncherParentResult::Back);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Network)
        );
        assert_eq!(state.drain_output().result, None);
    }
}
