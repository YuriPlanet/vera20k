//! Retained semantic model for launcher Options dialog resource `0xD5`.
//!
//! The parent owns bounded control state and emits ordered effects. It owns no
//! profile, filesystem, audio device, window, or child-dialog callback.

use crate::ui::client_theme;

pub(crate) mod shell;

/// gamemd-derived: launcher owner `OptionsClass__ShowLauncherDialog @
/// 0x0055FC80` and its primary-proc slice `0x0055FDB0..0x0056047A` bind the
/// RT_DIALOG `0xD5` controls to these launcher-local CSF keys/fallbacks.
pub(crate) const LAUNCHER_LABEL_SPECS: [(&str, &str); 34] = [
    ("GUI:OptionsMenu", "Options"),
    ("GUI:MainMenu", "Main Menu"),
    ("GUI:Keyboard", "Keyboard"),
    ("GUI:Network", "Network"),
    ("GUI:DisplayOptions", "Display Options"),
    ("GUI:GameOptions", "Game Options"),
    ("GUI:UIOptions", "UI Options"),
    ("GUI:AudioOptions", "Audio Options"),
    ("GUI:SetResolution", "Set Game Resolution"),
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
    ("GUI:HigherDetail", "Higher"),
    ("GUI:Harder", "Harder"),
    ("GUI:Faster", "Faster"),
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
    pub(crate) set_resolution: String,
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
    higher_detail: String,
    harder: String,
    faster: String,
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
            set_resolution: label(8),
            visual_details: label(9),
            difficulty: label(10),
            tooltips: label(11),
            scroll_rate: label(12),
            target_lines: label(13),
            show_hidden: label(14),
            music_volume: label(15),
            sound_volume: label(16),
            voice_volume: label(17),
            blank: label(18),
            higher_detail: label(19),
            harder: label(20),
            faster: label(21),
            low: label(22),
            high: label(23),
            easy: label(24),
            normal: label(25),
            hard: label(26),
            scroll_tokens: [
                label(27),
                label(28),
                label(29),
                label(30),
                label(31),
                label(32),
                label(33),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherRegion {
    Display,
    Game,
    Ui,
    Audio,
    RightRail,
    Footer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherControl {
    Detail,
    Resolution,
    Difficulty,
    Tooltips,
    TargetLines,
    ShowHidden,
    Scroll,
    Score,
    Sound,
    Voice,
    Keyboard,
    Network,
    MainMenu,
    BlankFooter,
    WarningDecoration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherSlot {
    Column { column: u8, order: u8 },
    Row { order: u8 },
    Lane { lane: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LauncherDescriptorEntry {
    id: u16,
    control: LauncherControl,
    region: LauncherRegion,
    slot: LauncherSlot,
}

/// gamemd-derived: the `0xD5` resource hierarchy consumed by primary-proc
/// slice `0x0055FDB0..0x0056047A`; dormant control `0x603` is not admitted by
/// the active launcher owner `OptionsClass__ShowLauncherDialog @ 0x0055FC80`.
const LAUNCHER_DESCRIPTOR: [LauncherDescriptorEntry; 15] = [
    LauncherDescriptorEntry {
        id: 0x52B,
        control: LauncherControl::Detail,
        region: LauncherRegion::Display,
        slot: LauncherSlot::Column {
            column: 0,
            order: 0,
        },
    },
    LauncherDescriptorEntry {
        id: 0x6ED,
        control: LauncherControl::Resolution,
        region: LauncherRegion::Display,
        slot: LauncherSlot::Column {
            column: 1,
            order: 0,
        },
    },
    LauncherDescriptorEntry {
        id: 0x50F,
        control: LauncherControl::Difficulty,
        region: LauncherRegion::Game,
        slot: LauncherSlot::Row { order: 0 },
    },
    LauncherDescriptorEntry {
        id: 0x602,
        control: LauncherControl::Tooltips,
        region: LauncherRegion::Ui,
        slot: LauncherSlot::Column {
            column: 0,
            order: 0,
        },
    },
    LauncherDescriptorEntry {
        id: 0x601,
        control: LauncherControl::TargetLines,
        region: LauncherRegion::Ui,
        slot: LauncherSlot::Column {
            column: 0,
            order: 1,
        },
    },
    LauncherDescriptorEntry {
        id: 0x604,
        control: LauncherControl::ShowHidden,
        region: LauncherRegion::Ui,
        slot: LauncherSlot::Column {
            column: 0,
            order: 2,
        },
    },
    LauncherDescriptorEntry {
        id: 0x52A,
        control: LauncherControl::Scroll,
        region: LauncherRegion::Ui,
        slot: LauncherSlot::Column {
            column: 1,
            order: 0,
        },
    },
    LauncherDescriptorEntry {
        id: 0x52F,
        control: LauncherControl::Score,
        region: LauncherRegion::Audio,
        slot: LauncherSlot::Lane { lane: 0 },
    },
    LauncherDescriptorEntry {
        id: 0x532,
        control: LauncherControl::Sound,
        region: LauncherRegion::Audio,
        slot: LauncherSlot::Lane { lane: 1 },
    },
    LauncherDescriptorEntry {
        id: 0x536,
        control: LauncherControl::Voice,
        region: LauncherRegion::Audio,
        slot: LauncherSlot::Lane { lane: 2 },
    },
    LauncherDescriptorEntry {
        id: 0x5CE,
        control: LauncherControl::Keyboard,
        region: LauncherRegion::RightRail,
        slot: LauncherSlot::Row { order: 0 },
    },
    LauncherDescriptorEntry {
        id: 0x5CD,
        control: LauncherControl::Network,
        region: LauncherRegion::RightRail,
        slot: LauncherSlot::Row { order: 1 },
    },
    LauncherDescriptorEntry {
        id: 0x686,
        control: LauncherControl::MainMenu,
        region: LauncherRegion::RightRail,
        slot: LauncherSlot::Row { order: 2 },
    },
    LauncherDescriptorEntry {
        id: 0x695,
        control: LauncherControl::BlankFooter,
        region: LauncherRegion::Footer,
        slot: LauncherSlot::Row { order: 0 },
    },
    LauncherDescriptorEntry {
        id: 0x71C,
        control: LauncherControl::WarningDecoration,
        region: LauncherRegion::Footer,
        slot: LauncherSlot::Row { order: 1 },
    },
];

fn launcher_entries(region: LauncherRegion) -> Vec<&'static LauncherDescriptorEntry> {
    let mut entries = LAUNCHER_DESCRIPTOR
        .iter()
        .filter(|entry| entry.region == region)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| match entry.slot {
        LauncherSlot::Column { order, .. } | LauncherSlot::Row { order } => order,
        LauncherSlot::Lane { lane } => lane,
    });
    entries
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

    const fn plaque_reserve(self) -> i32 {
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
    ResolutionSelected { width: i32, height: i32 },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherResolutionRow {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) label: String,
}

impl LauncherResolutionRow {
    /// gamemd-derived: primary-proc slice `0x005601A0..0x00560270` appends
    /// admitted dimension pairs to the `0xD5` combo using literal
    /// `%d x %d x 16` display text.
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            label: format!("{width} x {height} x 16"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherOptionsValues {
    pub(crate) detail_position: u8,
    pub(crate) difficulty_position: u8,
    pub(crate) scroll_position: u8,
    pub(crate) tooltips: bool,
    pub(crate) target_lines: bool,
    pub(crate) show_hidden: bool,
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

const TRACKBAR_HEIGHT_PX: i32 = 24;
const CHECKBOX_HEIGHT_PX: i32 = 20;
const COMBO_HEIGHT_PX: i32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalLocalRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl PhysicalLocalRect {
    const fn from_min_size(left: i32, top: i32, width: i32, height: i32) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }
}

/// One rounded physical-pixel control rectangle shared by native-style paint
/// geometry and input admission. Each global edge is rounded before width and
/// height subtraction, matching the Winit/egui boundary contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalControlRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl PhysicalControlRect {
    fn from_logical_edges(
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
        pixels_per_point: f64,
    ) -> Option<Self> {
        if !pixels_per_point.is_finite()
            || pixels_per_point <= 0.0
            || ![left, top, right, bottom].into_iter().all(f64::is_finite)
        {
            return None;
        }
        let physical = |value: f64| (value * pixels_per_point).round() as i32;
        let rect = Self {
            left: physical(left),
            top: physical(top),
            right: physical(right),
            bottom: physical(bottom),
        };
        (rect.width() > 0 && rect.height() > 0).then_some(rect)
    }

    fn from_egui(rect: egui::Rect, pixels_per_point: f32) -> Option<Self> {
        Self::from_logical_edges(
            f64::from(rect.left()),
            f64::from(rect.top()),
            f64::from(rect.right()),
            f64::from(rect.bottom()),
            f64::from(pixels_per_point),
        )
    }

    const fn width(self) -> i32 {
        self.right - self.left
    }

    const fn height(self) -> i32 {
        self.bottom - self.top
    }

    fn frame_from_logical_pointer(
        self,
        pointer_x: f64,
        pointer_y: f64,
        pixels_per_point: f64,
    ) -> Option<PhysicalControlFrame> {
        if !pixels_per_point.is_finite()
            || pixels_per_point <= 0.0
            || !pointer_x.is_finite()
            || !pointer_y.is_finite()
        {
            return None;
        }
        let physical = |value: f64| (value * pixels_per_point).round() as i32;
        Some(PhysicalControlFrame {
            local_x: physical(pointer_x) - self.left,
            local_y: physical(pointer_y) - self.top,
            width: self.width(),
            height: self.height(),
        })
    }

    fn local_rect_to_egui(self, rect: PhysicalLocalRect, pixels_per_point: f32) -> egui::Rect {
        let point = |x: i32, y: i32| {
            egui::pos2(
                (self.left + x) as f32 / pixels_per_point,
                (self.top + y) as f32 / pixels_per_point,
            )
        };
        egui::Rect::from_min_max(point(rect.left, rect.top), point(rect.right, rect.bottom))
    }

    fn local_pos_to_egui(self, x: i32, y: i32, pixels_per_point: f32) -> egui::Pos2 {
        egui::pos2(
            (self.left + x) as f32 / pixels_per_point,
            (self.top + y) as f32 / pixels_per_point,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackbarPaintGeometry {
    rail: PhysicalLocalRect,
    thumb: PhysicalLocalRect,
}

fn trackbar_paint_geometry(
    control: PhysicalControlRect,
    id: LauncherTrackbarId,
    position: u8,
) -> TrackbarPaintGeometry {
    // gamemd-derived: `TrackBar_ProcessMouse @ 0x0061D950` and its owner-draw
    // path share the literal 12x22 grip, 6px rail inset, and plaque reserve;
    // paint therefore consumes the same integer `thumb_left` as admission.
    let width = control.width();
    let height = control.height();
    let rail_right = (width - id.plaque_reserve() - 6).max(6);
    let rail_top = height / 2 - 2;
    let thumb_top = (height - 22) / 2;
    TrackbarPaintGeometry {
        rail: PhysicalLocalRect {
            left: 6,
            top: rail_top,
            right: rail_right,
            bottom: rail_top + 4,
        },
        thumb: PhysicalLocalRect::from_min_size(
            thumb_left(position, width, id.plaque_reserve(), id.maximum()),
            thumb_top,
            12,
            22,
        ),
    }
}

fn points_for_physical_pixels(pixels: i32, pixels_per_point: f32) -> f32 {
    if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels as f32 / pixels_per_point
    } else {
        pixels as f32
    }
}

/// One integer-pixel input frame derived from the same rounded control
/// rectangle that owns launcher custom-control paint geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicalControlFrame {
    pub(crate) local_x: i32,
    pub(crate) local_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl PhysicalControlFrame {
    #[cfg(test)]
    pub(crate) fn from_logical(
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
        pointer_x: f64,
        pointer_y: f64,
        pixels_per_point: f64,
    ) -> Option<Self> {
        PhysicalControlRect::from_logical_edges(left, top, right, bottom, pixels_per_point)?
            .frame_from_logical_pointer(pointer_x, pointer_y, pixels_per_point)
    }
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

/// gamemd-derived: `TrackBar_ProcessMouse @ 0x0061D950` uses the literal
/// reserve/13/12/6 geometry and `(range + 1)` integer partition below.
pub(crate) fn trackbar_position_from_x(
    raw_mouse_x: i32,
    client_width: i32,
    plaque_reserve: i32,
    maximum: u8,
) -> u8 {
    let usable_span = (client_width - plaque_reserve - 13).max(1);
    let maximum_track_x = (client_width - plaque_reserve - 12).max(1);
    let track_x = (raw_mouse_x - 6).clamp(1, maximum_track_x);
    let relative = ((track_x - 1) * (i32::from(maximum) + 1)) / usable_span;
    relative.min(i32::from(maximum)) as u8
}

pub(crate) fn thumb_left(position: u8, client_width: i32, reserve: i32, maximum: u8) -> i32 {
    let usable_span = (client_width - reserve - 13).max(1);
    // Original 0x0061E486..0x0061E4A8 (TBM_SETPOS) and
    // 0x0061DC44..0x0061DC58 (drag) divide the painted offset by range.
    // Mouse quantization above deliberately uses range + 1 instead.
    1 + i32::from(position) * usable_span / i32::from(maximum).max(1)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OptionsDialogState {
    labels: LauncherOptionsLabels,
    values: LauncherOptionsValues,
    resolution_rows: Vec<LauncherResolutionRow>,
    selected_resolution: Option<usize>,
    resolution_popup_open: bool,
    launcher_audio_available: bool,
    detail_caption: String,
    difficulty_caption: String,
    scroll_caption: String,
    capture: Option<LauncherTrackbarId>,
    pending_events: Vec<LauncherOptionsEvent>,
    pending_result: Option<LauncherParentResult>,
    shell_interaction: shell::ShellInteraction,
}

impl Default for OptionsDialogState {
    fn default() -> Self {
        let labels = LauncherOptionsLabels::resolve(&|_| None);
        Self::new(
            labels,
            LauncherOptionsValues::default(),
            Vec::new(),
            None,
            false,
        )
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
        resolution_rows: Vec<LauncherResolutionRow>,
        selected_resolution: Option<usize>,
        launcher_audio_available: bool,
    ) -> Self {
        values.detail_position = admitted_initial_position(i64::from(values.detail_position), 1);
        values.difficulty_position =
            admitted_initial_position(i64::from(values.difficulty_position), 2);
        values.scroll_position = admitted_initial_position(i64::from(values.scroll_position), 6);
        values.score_position = admitted_initial_position(i64::from(values.score_position), 10);
        values.sound_position = admitted_initial_position(i64::from(values.sound_position), 10);
        values.voice_position = admitted_initial_position(i64::from(values.voice_position), 10);
        let selected_resolution =
            selected_resolution.filter(|index| *index < resolution_rows.len());
        Self {
            detail_caption: labels.higher_detail.clone(),
            difficulty_caption: labels.harder.clone(),
            scroll_caption: labels.faster.clone(),
            labels,
            values,
            resolution_rows,
            selected_resolution,
            resolution_popup_open: false,
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
                self.detail_caption = if position == 0 {
                    self.labels.low.clone()
                } else {
                    self.labels.high.clone()
                };
                self.pending_events
                    .push(LauncherOptionsEvent::Cue(LauncherCue::GenericClick));
            }
            LauncherTrackbarId::Difficulty => {
                self.difficulty_caption = match position {
                    0 => self.labels.easy.clone(),
                    1 => self.labels.normal.clone(),
                    _ => self.labels.hard.clone(),
                };
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

    /// gamemd-derived: the initial down row of `TrackBar_ProcessMouse @
    /// 0x0061D950` requires `y > bottom - 18`; thumb-down captures without a
    /// jump, while a rail down jumps once and does not capture.
    pub(crate) fn trackbar_mouse_down(
        &mut self,
        id: LauncherTrackbarId,
        frame: PhysicalControlFrame,
    ) {
        if (id.is_audio() && !self.launcher_audio_available)
            || frame.local_x < 0
            || frame.local_x >= frame.width
            || frame.local_y <= frame.height - 18
            || frame.local_y >= frame.height
        {
            return;
        }
        let left = thumb_left(
            self.trackbar_position(id),
            frame.width,
            id.plaque_reserve(),
            id.maximum(),
        );
        if (left..left + 12).contains(&frame.local_x) {
            self.capture = Some(id);
            return;
        }
        let position = trackbar_position_from_x(
            frame.local_x,
            frame.width,
            id.plaque_reserve(),
            id.maximum(),
        );
        self.set_trackbar_position(id, position);
    }

    pub(crate) fn trackbar_mouse_move(
        &mut self,
        id: LauncherTrackbarId,
        frame: PhysicalControlFrame,
    ) {
        if self.capture != Some(id) {
            return;
        }
        let position = trackbar_position_from_x(
            frame.local_x,
            frame.width,
            id.plaque_reserve(),
            id.maximum(),
        );
        self.set_trackbar_position(id, position);
    }

    pub(crate) fn trackbar_mouse_up(&mut self, id: LauncherTrackbarId) {
        if self.capture == Some(id) {
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
        };
        *slot = !*slot;
        self.pending_events
            .push(LauncherOptionsEvent::Cue(LauncherCue::Checkbox));
    }

    /// gamemd-derived: combo owner `0x00617250` emits GUIComboOpen before its
    /// strict `x > client_width - 20` arrow admission test.
    pub(crate) fn combo_mouse_down(&mut self, frame: PhysicalControlFrame) {
        if frame.local_x < 0
            || frame.local_x >= frame.width
            || frame.local_y < 0
            || frame.local_y >= frame.height
        {
            return;
        }
        self.pending_events
            .push(LauncherOptionsEvent::Cue(LauncherCue::ComboOpen));
        if frame.local_x > frame.width - 20 {
            self.resolution_popup_open = !self.resolution_popup_open;
        }
    }

    /// gamemd-derived: ordinary `0xD5` owner-draw buttons emit
    /// `[AudioVisual] GUIMainButtonSound` on admitted primary mouse-down;
    /// `OptionsClass__ShowLauncherDialog @ 0x0055FC80` observes the distinct
    /// Back/Network/Keyboard result only on the later activation release.
    pub(crate) fn main_button_mouse_down(&mut self) {
        self.pending_events
            .push(LauncherOptionsEvent::Cue(LauncherCue::MainButton));
    }

    /// gamemd-derived: primary-proc slice `0x0055FDB0..0x0056047A` accepts only
    /// a valid combo row and publishes its width/height immediately; final
    /// accepted projection `0x0055FAA0` does not own the resolution pair.
    pub(crate) fn select_resolution(&mut self, index: usize) {
        let Some(row) = self.resolution_rows.get(index) else {
            return;
        };
        self.selected_resolution = Some(index);
        self.resolution_popup_open = false;
        self.pending_events
            .push(LauncherOptionsEvent::ResolutionSelected {
                width: row.width,
                height: row.height,
            });
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

    fn selected_resolution_label(&self) -> &str {
        self.selected_resolution
            .and_then(|index| self.resolution_rows.get(index))
            .map_or("", |row| row.label.as_str())
    }
}

pub(crate) fn draw_launcher_options_dialog(
    ctx: &egui::Context,
    state: &mut OptionsDialogState,
) -> LauncherOptionsFrameOutput {
    let palette = client_theme::apply_client_theme(ctx);
    super::draw_backdrop(ctx, "options_backdrop");

    egui::Window::new("")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(client_theme::card_frame(palette.panel, palette.line))
        .min_width(720.0)
        .show(ctx, |ui| {
            ui.set_max_width(720.0);
            ui.horizontal_top(|ui| {
                ui.set_width(540.0);
                ui.vertical(|ui| {
                    draw_display_group(ui, state, palette);
                    ui.add_space(10.0);
                    draw_game_group(ui, state, palette);
                    ui.add_space(10.0);
                    draw_ui_group(ui, state, palette);
                    ui.add_space(10.0);
                    draw_audio_group(ui, state, palette);
                });
                ui.separator();
                draw_right_rail(ui, state);
            });
            draw_footer(ui, state);
        });

    state.drain_output()
}

fn draw_display_group(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    palette: client_theme::ClientPalette,
) {
    client_theme::section_label(ui, &state.labels.display_options, palette);
    ui.columns(2, |columns| {
        for entry in launcher_entries(LauncherRegion::Display) {
            let LauncherSlot::Column { column, .. } = entry.slot else {
                continue;
            };
            let column = &mut columns[usize::from(column)];
            match entry.control {
                LauncherControl::Detail => {
                    column.label(format!(
                        "{} — {}",
                        state.labels.visual_details, state.detail_caption
                    ));
                    draw_trackbar(column, state, LauncherTrackbarId::Detail, 180.0, palette);
                }
                LauncherControl::Resolution => {
                    column.label(&state.labels.set_resolution);
                    draw_resolution_combo(column, state, palette);
                }
                _ => {}
            }
        }
    });
}

fn draw_game_group(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    palette: client_theme::ClientPalette,
) {
    client_theme::section_label(ui, &state.labels.game_options, palette);
    for entry in launcher_entries(LauncherRegion::Game) {
        if entry.control == LauncherControl::Difficulty {
            ui.label(format!(
                "{} — {}",
                state.labels.difficulty, state.difficulty_caption
            ));
            draw_trackbar(ui, state, LauncherTrackbarId::Difficulty, 180.0, palette);
        }
    }
}

fn draw_ui_group(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    palette: client_theme::ClientPalette,
) {
    client_theme::section_label(ui, &state.labels.ui_options, palette);
    ui.columns(2, |columns| {
        for entry in launcher_entries(LauncherRegion::Ui) {
            let LauncherSlot::Column { column, .. } = entry.slot else {
                continue;
            };
            let column = &mut columns[usize::from(column)];
            let checkbox = match entry.control {
                LauncherControl::Tooltips => {
                    Some((LauncherCheckboxId::Tooltips, state.labels.tooltips.clone()))
                }
                LauncherControl::TargetLines => Some((
                    LauncherCheckboxId::TargetLines,
                    state.labels.target_lines.clone(),
                )),
                LauncherControl::ShowHidden => Some((
                    LauncherCheckboxId::ShowHidden,
                    state.labels.show_hidden.clone(),
                )),
                _ => None,
            };
            if let Some((id, label)) = checkbox {
                draw_checkbox(column, state, id, &label, palette);
            } else if entry.control == LauncherControl::Scroll {
                column.label(format!(
                    "{} — {}",
                    state.labels.scroll_rate, state.scroll_caption
                ));
                draw_trackbar(column, state, LauncherTrackbarId::Scroll, 180.0, palette);
            }
        }
    });
}

fn draw_audio_group(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    palette: client_theme::ClientPalette,
) {
    client_theme::section_label(ui, &state.labels.audio_options, palette);
    ui.columns(3, |columns| {
        for entry in launcher_entries(LauncherRegion::Audio) {
            let LauncherSlot::Lane { lane } = entry.slot else {
                continue;
            };
            let (id, label) = match entry.control {
                LauncherControl::Score => {
                    (LauncherTrackbarId::Score, state.labels.music_volume.clone())
                }
                LauncherControl::Sound => {
                    (LauncherTrackbarId::Sound, state.labels.sound_volume.clone())
                }
                LauncherControl::Voice => {
                    (LauncherTrackbarId::Voice, state.labels.voice_volume.clone())
                }
                _ => continue,
            };
            let column = &mut columns[usize::from(lane)];
            column.label(label);
            draw_trackbar(column, state, id, 128.0, palette);
        }
    });
}

fn draw_right_rail(ui: &mut egui::Ui, state: &mut OptionsDialogState) {
    ui.vertical_centered_justified(|ui| {
        ui.heading(&state.labels.options);
        ui.add_space(18.0);
        for entry in launcher_entries(LauncherRegion::RightRail) {
            let (label, result) = match entry.control {
                LauncherControl::Keyboard => (
                    state.labels.keyboard.clone(),
                    LauncherParentResult::Keyboard,
                ),
                LauncherControl::Network => {
                    (state.labels.network.clone(), LauncherParentResult::Network)
                }
                LauncherControl::MainMenu => {
                    ui.add_space(36.0);
                    (state.labels.main_menu.clone(), LauncherParentResult::Back)
                }
                _ => continue,
            };
            draw_parent_result_button(ui, state, &label, result);
        }
    });
}

fn draw_parent_result_button(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    label: &str,
    result: LauncherParentResult,
) {
    let response = ui.button(label);
    let pressed = ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
    if pressed && response.is_pointer_button_down_on() {
        state.main_button_mouse_down();
    }
    if response.clicked_by(egui::PointerButton::Primary) {
        state.request_result(result);
    }
}

fn draw_footer(ui: &mut egui::Ui, state: &OptionsDialogState) {
    for entry in launcher_entries(LauncherRegion::Footer) {
        match entry.control {
            LauncherControl::BlankFooter if !state.labels.blank.is_empty() => {
                ui.label(&state.labels.blank);
            }
            // Exact `0x71C` warning-decoration SHP/timing is a frozen visual
            // exactification residual; the semantic parent keeps it inert.
            LauncherControl::WarningDecoration | LauncherControl::BlankFooter => {}
            _ => {}
        }
    }
}

fn draw_trackbar(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    id: LauncherTrackbarId,
    width: f32,
    palette: client_theme::ClientPalette,
) {
    let enabled = !id.is_audio() || state.launcher_audio_available;
    let pixels_per_point = ui.ctx().pixels_per_point();
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(
            width,
            points_for_physical_pixels(TRACKBAR_HEIGHT_PX, pixels_per_point),
        ),
        egui::Sense::click_and_drag(),
    );
    let control = PhysicalControlRect::from_egui(rect, pixels_per_point);
    let rail_color = if enabled {
        palette.line
    } else {
        palette.text_muted.gamma_multiply(0.45)
    };
    if let Some(control) = control {
        let geometry = trackbar_paint_geometry(control, id, state.trackbar_position(id));
        ui.painter().rect_filled(
            control.local_rect_to_egui(geometry.rail, pixels_per_point),
            points_for_physical_pixels(1, pixels_per_point),
            rail_color,
        );
        ui.painter().rect_filled(
            control.local_rect_to_egui(geometry.thumb, pixels_per_point),
            points_for_physical_pixels(2, pixels_per_point),
            if enabled {
                palette.accent
            } else {
                palette.text_muted
            },
        );
    }

    let (pointer, pressed, released, down) = ui.input(|input| {
        (
            input.pointer.interact_pos(),
            input.pointer.button_pressed(egui::PointerButton::Primary),
            input.pointer.button_released(egui::PointerButton::Primary),
            input.pointer.button_down(egui::PointerButton::Primary),
        )
    });
    if let Some(pointer) = pointer
        && let Some(control) = control
        && let Some(frame) = control.frame_from_logical_pointer(
            f64::from(pointer.x),
            f64::from(pointer.y),
            f64::from(pixels_per_point),
        )
    {
        if pressed {
            state.trackbar_mouse_down(id, frame);
        } else if down {
            state.trackbar_mouse_move(id, frame);
        }
    }
    if released {
        state.trackbar_mouse_up(id);
    }
}

fn draw_checkbox(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    id: LauncherCheckboxId,
    label: &str,
    palette: client_theme::ClientPalette,
) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(
            220.0,
            points_for_physical_pixels(CHECKBOX_HEIGHT_PX, pixels_per_point),
        ),
        egui::Sense::click(),
    );
    let control = PhysicalControlRect::from_egui(rect, pixels_per_point);
    let checked = match id {
        LauncherCheckboxId::Tooltips => state.values.tooltips,
        LauncherCheckboxId::TargetLines => state.values.target_lines,
        LauncherCheckboxId::ShowHidden => state.values.show_hidden,
    };
    if let Some(control) = control {
        let icon = PhysicalLocalRect::from_min_size(0, 0, 18, 18);
        ui.painter().rect_stroke(
            control.local_rect_to_egui(icon, pixels_per_point),
            points_for_physical_pixels(1, pixels_per_point),
            egui::Stroke::new(
                points_for_physical_pixels(1, pixels_per_point),
                palette.line,
            ),
            egui::StrokeKind::Middle,
        );
        if checked {
            ui.painter().line_segment(
                [
                    control.local_pos_to_egui(3, 9, pixels_per_point),
                    control.local_pos_to_egui(9, 15, pixels_per_point),
                ],
                egui::Stroke::new(
                    points_for_physical_pixels(2, pixels_per_point),
                    palette.accent,
                ),
            );
            ui.painter().line_segment(
                [
                    control.local_pos_to_egui(9, 15, pixels_per_point),
                    control.local_pos_to_egui(16, 3, pixels_per_point),
                ],
                egui::Stroke::new(
                    points_for_physical_pixels(2, pixels_per_point),
                    palette.accent,
                ),
            );
        }
        ui.painter().text(
            control.local_pos_to_egui(26, 1, pixels_per_point),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(13.0),
            palette.text,
        );
    }
    let pressed = ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
    if pressed
        && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
        && let Some(control) = control
        && let Some(frame) = control.frame_from_logical_pointer(
            f64::from(pointer.x),
            f64::from(pointer.y),
            f64::from(pixels_per_point),
        )
    {
        state.checkbox_mouse_down(id, frame);
    }
}

fn draw_resolution_combo(
    ui: &mut egui::Ui,
    state: &mut OptionsDialogState,
    palette: client_theme::ClientPalette,
) {
    let pixels_per_point = ui.ctx().pixels_per_point();
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(
            180.0,
            points_for_physical_pixels(COMBO_HEIGHT_PX, pixels_per_point),
        ),
        egui::Sense::click(),
    );
    let control = PhysicalControlRect::from_egui(rect, pixels_per_point);
    if let Some(control) = control {
        let face = PhysicalLocalRect::from_min_size(0, 0, control.width(), control.height());
        let face = control.local_rect_to_egui(face, pixels_per_point);
        ui.painter().rect_filled(
            face,
            points_for_physical_pixels(1, pixels_per_point),
            palette.panel_alt,
        );
        ui.painter().rect_stroke(
            face,
            points_for_physical_pixels(1, pixels_per_point),
            egui::Stroke::new(
                points_for_physical_pixels(1, pixels_per_point),
                palette.line,
            ),
            egui::StrokeKind::Middle,
        );
        ui.painter().text(
            control.local_pos_to_egui(3, 4, pixels_per_point),
            egui::Align2::LEFT_TOP,
            state.selected_resolution_label(),
            egui::FontId::proportional(13.0),
            palette.text,
        );
        ui.painter().text(
            control.local_pos_to_egui(control.width() - 10, control.height() / 2, pixels_per_point),
            egui::Align2::CENTER_CENTER,
            "▼",
            egui::FontId::proportional(12.0),
            palette.text,
        );
    }
    let pressed = ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
    if pressed
        && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
        && let Some(control) = control
        && let Some(frame) = control.frame_from_logical_pointer(
            f64::from(pointer.x),
            f64::from(pointer.y),
            f64::from(pixels_per_point),
        )
    {
        state.combo_mouse_down(frame);
    }
    if state.resolution_popup_open {
        let rows: Vec<(usize, String, bool)> = state
            .resolution_rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                (
                    index,
                    row.label.clone(),
                    state.selected_resolution == Some(index),
                )
            })
            .collect();
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            for (index, label, selected) in rows {
                if ui.selectable_label(selected, label).clicked() {
                    state.select_resolution(index);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::csf_file::CsfFile;

    fn labels() -> LauncherOptionsLabels {
        LauncherOptionsLabels::resolve(&|_| None)
    }

    fn state(audio: bool) -> OptionsDialogState {
        OptionsDialogState::new(
            labels(),
            LauncherOptionsValues::default(),
            vec![
                LauncherResolutionRow::new(800, 600),
                LauncherResolutionRow::new(1024, 768),
            ],
            Some(1),
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

    fn physical_control(
        scale: f64,
        logical_width: f64,
        physical_height: i32,
    ) -> PhysicalControlRect {
        PhysicalControlRect::from_logical_edges(
            10.25,
            20.25,
            10.25 + logical_width,
            20.25 + f64::from(physical_height) / scale,
            scale,
        )
        .unwrap()
    }

    fn production_frame(
        scale: f64,
        logical_width: f64,
        physical_height: i32,
        local_x: i32,
        local_y: i32,
    ) -> PhysicalControlFrame {
        let control = physical_control(scale, logical_width, physical_height);
        PhysicalControlFrame::from_logical(
            10.25,
            20.25,
            10.25 + logical_width,
            20.25 + f64::from(physical_height) / scale,
            f64::from(control.left + local_x) / scale,
            f64::from(control.top + local_y) / scale,
            scale,
        )
        .unwrap()
    }

    fn set_position_without_notification(
        state: &mut OptionsDialogState,
        id: LauncherTrackbarId,
        position: u8,
    ) {
        match id {
            LauncherTrackbarId::Detail => state.values.detail_position = position,
            LauncherTrackbarId::Difficulty => state.values.difficulty_position = position,
            LauncherTrackbarId::Scroll => state.values.scroll_position = position,
            LauncherTrackbarId::Score => state.values.score_position = position,
            LauncherTrackbarId::Sound => state.values.sound_position = position,
            LauncherTrackbarId::Voice => state.values.voice_position = position,
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
    fn exact_launcher_label_fallbacks_are_local_and_complete() {
        const EXPECTED: [(&str, &str); 34] = [
            ("GUI:OptionsMenu", "Options"),
            ("GUI:MainMenu", "Main Menu"),
            ("GUI:Keyboard", "Keyboard"),
            ("GUI:Network", "Network"),
            ("GUI:DisplayOptions", "Display Options"),
            ("GUI:GameOptions", "Game Options"),
            ("GUI:UIOptions", "UI Options"),
            ("GUI:AudioOptions", "Audio Options"),
            ("GUI:SetResolution", "Set Game Resolution"),
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
            ("GUI:HigherDetail", "Higher"),
            ("GUI:Harder", "Harder"),
            ("GUI:Faster", "Faster"),
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
        assert_eq!(LAUNCHER_LABEL_SPECS, EXPECTED);
        let seen = std::cell::RefCell::new(Vec::new());
        let labels = LauncherOptionsLabels::resolve(&|key| {
            seen.borrow_mut().push(key.to_string());
            None
        });
        assert_eq!(seen.into_inner(), EXPECTED.map(|(key, _)| key.to_string()));
        let actual_values = [
            labels.options.as_str(),
            labels.main_menu.as_str(),
            labels.keyboard.as_str(),
            labels.network.as_str(),
            labels.display_options.as_str(),
            labels.game_options.as_str(),
            labels.ui_options.as_str(),
            labels.audio_options.as_str(),
            labels.set_resolution.as_str(),
            labels.visual_details.as_str(),
            labels.difficulty.as_str(),
            labels.tooltips.as_str(),
            labels.scroll_rate.as_str(),
            labels.target_lines.as_str(),
            labels.show_hidden.as_str(),
            labels.music_volume.as_str(),
            labels.sound_volume.as_str(),
            labels.voice_volume.as_str(),
            labels.blank.as_str(),
            labels.higher_detail.as_str(),
            labels.harder.as_str(),
            labels.faster.as_str(),
            labels.low.as_str(),
            labels.high.as_str(),
            labels.easy.as_str(),
            labels.normal.as_str(),
            labels.hard.as_str(),
            labels.scroll_tokens[0].as_str(),
            labels.scroll_tokens[1].as_str(),
            labels.scroll_tokens[2].as_str(),
            labels.scroll_tokens[3].as_str(),
            labels.scroll_tokens[4].as_str(),
            labels.scroll_tokens[5].as_str(),
            labels.scroll_tokens[6].as_str(),
        ];
        assert_eq!(actual_values, EXPECTED.map(|(_, fallback)| fallback));
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
    fn descriptor_is_complete_authoritative_layout_and_omits_dormant_603() {
        let actual =
            LAUNCHER_DESCRIPTOR.map(|entry| (entry.id, entry.control, entry.region, entry.slot));
        let expected = [
            (
                0x52B,
                LauncherControl::Detail,
                LauncherRegion::Display,
                LauncherSlot::Column {
                    column: 0,
                    order: 0,
                },
            ),
            (
                0x6ED,
                LauncherControl::Resolution,
                LauncherRegion::Display,
                LauncherSlot::Column {
                    column: 1,
                    order: 0,
                },
            ),
            (
                0x50F,
                LauncherControl::Difficulty,
                LauncherRegion::Game,
                LauncherSlot::Row { order: 0 },
            ),
            (
                0x602,
                LauncherControl::Tooltips,
                LauncherRegion::Ui,
                LauncherSlot::Column {
                    column: 0,
                    order: 0,
                },
            ),
            (
                0x601,
                LauncherControl::TargetLines,
                LauncherRegion::Ui,
                LauncherSlot::Column {
                    column: 0,
                    order: 1,
                },
            ),
            (
                0x604,
                LauncherControl::ShowHidden,
                LauncherRegion::Ui,
                LauncherSlot::Column {
                    column: 0,
                    order: 2,
                },
            ),
            (
                0x52A,
                LauncherControl::Scroll,
                LauncherRegion::Ui,
                LauncherSlot::Column {
                    column: 1,
                    order: 0,
                },
            ),
            (
                0x52F,
                LauncherControl::Score,
                LauncherRegion::Audio,
                LauncherSlot::Lane { lane: 0 },
            ),
            (
                0x532,
                LauncherControl::Sound,
                LauncherRegion::Audio,
                LauncherSlot::Lane { lane: 1 },
            ),
            (
                0x536,
                LauncherControl::Voice,
                LauncherRegion::Audio,
                LauncherSlot::Lane { lane: 2 },
            ),
            (
                0x5CE,
                LauncherControl::Keyboard,
                LauncherRegion::RightRail,
                LauncherSlot::Row { order: 0 },
            ),
            (
                0x5CD,
                LauncherControl::Network,
                LauncherRegion::RightRail,
                LauncherSlot::Row { order: 1 },
            ),
            (
                0x686,
                LauncherControl::MainMenu,
                LauncherRegion::RightRail,
                LauncherSlot::Row { order: 2 },
            ),
            (
                0x695,
                LauncherControl::BlankFooter,
                LauncherRegion::Footer,
                LauncherSlot::Row { order: 0 },
            ),
            (
                0x71C,
                LauncherControl::WarningDecoration,
                LauncherRegion::Footer,
                LauncherSlot::Row { order: 1 },
            ),
        ];
        assert_eq!(actual, expected);
        assert!(!LAUNCHER_DESCRIPTOR.iter().any(|entry| entry.id == 0x603));
        assert_eq!(
            launcher_entries(LauncherRegion::Audio)
                .iter()
                .map(|entry| entry.control)
                .collect::<Vec<_>>(),
            [
                LauncherControl::Score,
                LauncherControl::Sound,
                LauncherControl::Voice,
            ]
        );
        assert_eq!(
            launcher_entries(LauncherRegion::RightRail)
                .iter()
                .map(|entry| entry.control)
                .collect::<Vec<_>>(),
            [
                LauncherControl::Keyboard,
                LauncherControl::Network,
                LauncherControl::MainMenu,
            ]
        );
    }

    #[test]
    fn initial_captions_ignore_positions_and_only_changed_values_swap_tokens() {
        let mut state = OptionsDialogState::new(
            labels(),
            LauncherOptionsValues {
                detail_position: 0,
                difficulty_position: 2,
                scroll_position: 0,
                ..Default::default()
            },
            Vec::new(),
            None,
            true,
        );
        assert_eq!(state.detail_caption, "Higher");
        assert_eq!(state.difficulty_caption, "Harder");
        assert_eq!(state.scroll_caption, "Faster");

        state.set_trackbar_position(LauncherTrackbarId::Detail, 1);
        state.set_trackbar_position(LauncherTrackbarId::Difficulty, 2);
        state.set_trackbar_position(LauncherTrackbarId::Scroll, 6);
        assert_eq!(state.detail_caption, "High");
        assert_eq!(
            state.difficulty_caption, "Harder",
            "unchanged move retains resource caption"
        );
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
    fn physical_frame_rounds_global_edges_before_subtraction_at_common_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let frame =
                PhysicalControlFrame::from_logical(10.25, 20.25, 28.25, 38.25, 27.25, 37.25, scale)
                    .unwrap();
            assert_eq!(
                frame.width,
                (28.25_f64 * scale).round() as i32 - (10.25_f64 * scale).round() as i32
            );
            assert_eq!(
                frame.height,
                (38.25_f64 * scale).round() as i32 - (20.25_f64 * scale).round() as i32
            );
            assert_eq!(
                frame.local_x,
                (27.25_f64 * scale).round() as i32 - (10.25_f64 * scale).round() as i32
            );
            assert_eq!(
                frame.local_y,
                (37.25_f64 * scale).round() as i32 - (20.25_f64 * scale).round() as i32
            );
        }
        assert!(PhysicalControlFrame::from_logical(0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0).is_none());
        assert!(
            PhysicalControlFrame::from_logical(0.0, 0.0, 1.0, 1.0, 0.0, 0.0, f64::NAN).is_none()
        );
    }

    #[test]
    fn painted_thumb_centers_capture_and_drag_through_physical_frames_at_common_scales() {
        let ids = [
            LauncherTrackbarId::Detail,
            LauncherTrackbarId::Difficulty,
            LauncherTrackbarId::Scroll,
            LauncherTrackbarId::Score,
            LauncherTrackbarId::Sound,
            LauncherTrackbarId::Voice,
        ];
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for id in ids {
                let logical_width = if id.is_audio() { 128.0 } else { 180.0 };
                let control = physical_control(scale, logical_width, TRACKBAR_HEIGHT_PX);
                assert_eq!(control.height(), TRACKBAR_HEIGHT_PX);
                for position in 0..=id.maximum() {
                    let mut state = state(true);
                    set_position_without_notification(&mut state, id, position);
                    let geometry = trackbar_paint_geometry(control, id, position);
                    assert_eq!(geometry.thumb.right - geometry.thumb.left, 12);
                    assert_eq!(geometry.thumb.bottom - geometry.thumb.top, 22);
                    let down = production_frame(
                        scale,
                        logical_width,
                        TRACKBAR_HEIGHT_PX,
                        (geometry.thumb.left + geometry.thumb.right) / 2,
                        (geometry.thumb.top + geometry.thumb.bottom) / 2,
                    );
                    state.trackbar_mouse_down(id, down);
                    assert_eq!(
                        state.trackbar_position(id),
                        position,
                        "scale={scale} id={id:?}"
                    );
                    assert_eq!(state.capture, Some(id), "scale={scale} id={id:?}");

                    let drag = production_frame(
                        scale,
                        logical_width,
                        TRACKBAR_HEIGHT_PX,
                        control.width() + 100,
                        -400,
                    );
                    state.trackbar_mouse_move(id, drag);
                    assert_eq!(state.trackbar_position(id), id.maximum());
                    state.trackbar_mouse_up(id);
                    assert_eq!(state.capture, None);
                }
            }
        }
    }

    #[test]
    fn checkbox_and_combo_paint_boundaries_drive_admission_at_common_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let checkbox = physical_control(scale, 220.0, CHECKBOX_HEIGHT_PX);
            assert_eq!(checkbox.height(), CHECKBOX_HEIGHT_PX);
            let mut checkbox_state = state(true);
            checkbox_state.checkbox_mouse_down(
                LauncherCheckboxId::Tooltips,
                production_frame(scale, 220.0, CHECKBOX_HEIGHT_PX, 17, 17),
            );
            assert!(!checkbox_state.values.tooltips, "scale={scale}");
            checkbox_state.drain_output();
            checkbox_state.checkbox_mouse_down(
                LauncherCheckboxId::Tooltips,
                production_frame(scale, 220.0, CHECKBOX_HEIGHT_PX, 18, 5),
            );
            assert!(!checkbox_state.values.tooltips, "scale={scale}");
            assert!(checkbox_state.drain_output().events.is_empty());

            let combo = physical_control(scale, 180.0, COMBO_HEIGHT_PX);
            assert_eq!(combo.height(), COMBO_HEIGHT_PX);
            let arrow_left = combo.width() - 20;
            let mut combo_state = state(true);
            combo_state.combo_mouse_down(production_frame(
                scale,
                180.0,
                COMBO_HEIGHT_PX,
                arrow_left,
                COMBO_HEIGHT_PX / 2,
            ));
            assert!(!combo_state.resolution_popup_open, "scale={scale}");
            assert_eq!(
                combo_state.drain_output().events,
                [LauncherOptionsEvent::Cue(LauncherCue::ComboOpen)]
            );
            combo_state.combo_mouse_down(production_frame(
                scale,
                180.0,
                COMBO_HEIGHT_PX,
                arrow_left + 1,
                COMBO_HEIGHT_PX / 2,
            ));
            assert!(combo_state.resolution_popup_open, "scale={scale}");
        }
    }

    #[test]
    fn trackbar_formula_matches_all_canonical_d5_thresholds() {
        let cases = [
            (180, 0, 1, vec![(90, 0), (91, 1)]),
            (180, 0, 2, vec![(62, 0), (63, 1), (118, 1), (119, 2)]),
            (
                180,
                0,
                6,
                vec![
                    (30, 0),
                    (31, 1),
                    (55, 2),
                    (79, 3),
                    (103, 4),
                    (127, 5),
                    (151, 6),
                ],
            ),
            (
                128,
                50,
                10,
                vec![
                    (12, 0),
                    (13, 1),
                    (19, 2),
                    (25, 3),
                    (31, 4),
                    (37, 5),
                    (43, 6),
                    (49, 7),
                    (55, 8),
                    (61, 9),
                    (67, 10),
                ],
            ),
        ];
        for (width, reserve, maximum, samples) in cases {
            for (x, expected) in samples {
                assert_eq!(
                    trackbar_position_from_x(x, width, reserve, maximum),
                    expected,
                    "width={width} reserve={reserve} x={x}"
                );
            }
        }
    }

    #[test]
    fn launcher_trackbar_position_and_painted_thumb_match_original_instruction_goldens() {
        // Reproduced by tools/storage_oracle/launcher_trackbar.py from the
        // hash-checked retail executable. This compares the separate native
        // range+1 pointer partition and range-only retained paint calculation.
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/launcher_trackbar.json"
        ))
        .unwrap();
        // BBB/B8 add geometries to the shared oracle; retain original D5 coverage.
        let geometries: Vec<_> = golden["geometries"].as_array().unwrap().iter()
            .filter(|geometry| matches!(geometry["width"].as_i64(), Some(128 | 180))).collect();
        assert_eq!(geometries.len(), 16);
        let integer = |value: &serde_json::Value, key: &str| value[key].as_i64().unwrap() as i32;
        let mut position_count = 0;
        let mut pointer_count = 0;
        for geometry in geometries {
            let width = integer(geometry, "width");
            let reserve = integer(geometry, "reserve");
            let maximum = integer(geometry, "maximum") as u8;
            for sample in geometry["positions"].as_array().unwrap() {
                let position = integer(sample, "position") as u8;
                let left = thumb_left(position, width, reserve, maximum);
                assert_eq!(
                    left,
                    integer(sample, "thumb_left"),
                    "{geometry:?} {sample:?}"
                );
                assert_eq!(left + 12, integer(sample, "thumb_right"));
                position_count += 1;
            }
            for sample in geometry["pointers"].as_array().unwrap() {
                let x = integer(sample, "x");
                let position = trackbar_position_from_x(x, width, reserve, maximum);
                assert_eq!(
                    i32::from(position),
                    integer(sample, "position"),
                    "width={width} reserve={reserve} maximum={maximum} x={x}"
                );
                let left = thumb_left(position, width, reserve, maximum);
                assert_eq!(left, integer(sample, "thumb_left"));
                assert_eq!(left + 12, integer(sample, "thumb_right"));
                pointer_count += 1;
            }
        }
        assert_eq!(position_count, 92);
        assert_eq!(pointer_count, 2736);
    }

    #[test]
    fn thumb_down_captures_without_jump_and_captured_motion_is_x_only() {
        let mut state = state(true);
        let before = state.trackbar_position(LauncherTrackbarId::Difficulty);
        let left = thumb_left(before, 180, 0, 2);
        state.trackbar_mouse_down(LauncherTrackbarId::Difficulty, frame(left + 5, 23, 180, 24));
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Difficulty),
            before
        );
        assert_eq!(state.capture, Some(LauncherTrackbarId::Difficulty));
        state.trackbar_mouse_move(LauncherTrackbarId::Difficulty, frame(179, -400, 180, 24));
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Difficulty), 2);
        state.trackbar_mouse_up(LauncherTrackbarId::Difficulty);
        assert_eq!(state.capture, None);
    }

    #[test]
    fn rail_jump_has_lower_strip_gate_and_never_captures() {
        let mut state = state(true);
        state.trackbar_mouse_down(LauncherTrackbarId::Scroll, frame(151, 6, 180, 24));
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Scroll),
            3,
            "strict lower edge rejects"
        );
        state.trackbar_mouse_down(LauncherTrackbarId::Scroll, frame(151, 7, 180, 24));
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Scroll), 6);
        assert_eq!(state.capture, None);
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
    fn combo_cues_before_strict_arrow_toggle_and_valid_selection() {
        let mut state = state(true);
        state.combo_mouse_down(frame(160, 5, 180, 24));
        assert!(!state.resolution_popup_open, "strict edge does not open");
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(LauncherCue::ComboOpen)]
        );
        state.combo_mouse_down(frame(161, 5, 180, 24));
        assert!(state.resolution_popup_open);
        state.select_resolution(99);
        assert!(
            state
                .drain_output()
                .events
                .iter()
                .all(|event| !matches!(event, LauncherOptionsEvent::ResolutionSelected { .. }))
        );
        state.select_resolution(0);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::ResolutionSelected {
                width: 800,
                height: 600
            }]
        );
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
    fn main_button_press_cues_before_release_result() {
        let mut state = state(true);
        state.main_button_mouse_down();
        assert_eq!(
            state.drain_output(),
            LauncherOptionsFrameOutput {
                events: vec![LauncherOptionsEvent::Cue(LauncherCue::MainButton)],
                result: None,
            }
        );

        state.request_result(LauncherParentResult::Network);
        assert_eq!(
            state.drain_output(),
            LauncherOptionsFrameOutput {
                events: Vec::new(),
                result: Some(LauncherParentResult::Network),
            }
        );
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
