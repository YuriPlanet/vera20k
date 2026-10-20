use std::collections::HashMap;

/// Identifies a visual cursor from mouse.sha. Used as HashMap key in SoftwareCursor.
///
/// The ranges below are the compiled cursor descriptor table at `0x0082D028`:
/// 86 records of 28 bytes, indexed by `Set_Mouse_Cursor_Shape` (`0x005BDC80`) as
/// `base + 28 * row`. Each record is `(frame, count, rate, miniFrame, miniCount,
/// hotspotX, hotspotY)` with `miniFrame == -1` when the row has no second set
/// and the two hotspot fields taking `0` (near edge), `12345` (centre) or
/// `54321` (far edge) — `0x005BDD27`/`0x005BDD44` turn those sentinels into the
/// offset. `tools/mouse_oracle/cursor_table.py` dumps the table and the
/// `DisplayClass::SetCursorFromAction` (`0x004AAE90`, jump table `0x004AB884`)
/// action-to-row switch into `tools/mouse_oracle/cursor_table.json`, which
/// `cursor_rows_match_the_compiled_descriptor_table` checks this against.
///
/// Two switches read that table. The tactical one is `SetCursorFromAction`'s
/// main jump table (`0x004AB884`, indexed by action - 1). The radar one is its
/// mini switch (byte table `0x004AB838` indexed by action, jump table
/// `0x004AB7D0`), which the RadarClass mouse handler (`0x006539D0`) reaches
/// with `mini = 1`; a row whose `miniFrame` is `-1` falls back to its main set
/// even under the mini flag (`0x005BDCA3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CursorId {
    Default,
    Select,
    Move,
    NoMove,
    Attack,
    /// Cursor table row 21. Shared by out-of-range attack and by the harvest
    /// action — the action switch routes both to the same row, so they must
    /// stay one id or switching between them would restart the animation.
    AttackOutOfRange,
    AttackMove,
    /// Cursor table row 22 — the guard-area reticle.
    GuardArea,
    Deploy,
    NoDeploy,
    // Directional scroll cursors (move-allowed).
    ScrollN,
    ScrollNE,
    ScrollE,
    ScrollSE,
    ScrollS,
    ScrollSW,
    ScrollW,
    ScrollNW,
    // Directional scroll cursors (can't-scroll-further).
    NoMoveN,
    NoMoveNE,
    NoMoveE,
    NoMoveSE,
    NoMoveS,
    NoMoveSW,
    NoMoveW,
    NoMoveNW,
    MinimapMove,
    /// Radar (minimap) cursor sets — the `miniFrame`/`miniCount` pair of a
    /// descriptor row, shown when the original's RadarClass mouse handler
    /// (`0x006539D0`) passes `mini = 1` into `SetCursorFromAction`
    /// (`0x004AAE90`). The mini switch (byte table `0x004AB838` indexed by
    /// action, jump table `0x004AB7D0`) is its own action-to-row map and is
    /// NOT the tactical one. Row 0's mini set is the plain arrow the radar
    /// defaults to once the keep-table filter drops the action.
    DefaultMini,
    /// Row 19's mini set — the NoMove mini (mini switch action 2 -> row 19).
    MinimapNoMove,
    /// Rows 20/21/71 share mini frame 63 with count 5; row 26's mini set
    /// starts at the same frame 63 with count 1.
    MinimapAttack,
    MinimapNoEnter,
    /// Row 22's mini set.
    MinimapGuardArea,
    /// Row 25's mini set.
    MinimapEnter,
    /// Superweapon targeting mini sets: Nuke 513/1 (row 53), LightningStorm
    /// 514/1 (row 50), SpyPlane 512/1 (row 85), PsychicReveal 515/1 (row 84).
    /// Count-1 sets are static whatever the row's rate.
    NukeMini,
    LightningStormMini,
    SpyPlaneMini,
    PsychicRevealMini,
    /// Rows 47 (Paradrop) and 83 (PsychicDominator) share one static mini
    /// frame (516, `miniCount` -1 — the signed modulo in `Advance` pins the
    /// counter at 0, so the set never advances whatever the row's rate).
    ParadropMini,
    PsychicDominatorMini,
    Enter,
    NoEnter,
    EngineerRepair,
    TogglePower,
    NoTogglePower,
    /// 4-way pan cursor — cursor-table row 61, mouse.sha frame 385.
    Pan,
    // Right-drag pan, directional variants. Cursor-table rows 62..69, mouse.sha
    // frames 386..393, all count 1, rate 0, hotspot centre/centre. These are
    // NOT the barred edge-scroll rows: those are rows 9..16 / frames 10..17 with
    // edge-anchored hotspots.
    PanN,
    PanNE,
    PanE,
    PanSE,
    PanS,
    PanSW,
    PanW,
    PanNW,
    // Sell / repair mode cursors.
    Sell,
    SellUnit,
    NoSell,
    Repair,
    NoRepair,
    // Special unit cursors.
    DesolatorDeploy,
    GIDeploy,
    Crush,
    Tote,
    IvanBomb,
    Detonate,
    Demolish,
    Disarm,
    // Spy / infiltration cursors.
    Disguise,
    SpyTech,
    SpyPower,
    // Mind control cursors.
    MindControl,
    NoMindControl,
    RemoveSquid,
    InfantryAbsorb,
    // Superweapon cursors.
    Nuke,
    Chronosphere,
    IronCurtain,
    LightningStorm,
    Paradrop,
    ForceShield,
    NoForceShield,
    GeneticMutator,
    AirStrike,
    PsychicDominator,
    PsychicReveal,
    SpyPlane,
    Beacon,
}

/// All loaded cursor animation sequences from mouse.sha, keyed by CursorId.
pub(crate) struct SoftwareCursor {
    pub(crate) sequences: HashMap<CursorId, SoftwareCursorSequence>,
    /// PlanNode63D630 reuses the loaded mouse shape, rows76/77, through the
    /// same mouse palette. CPU logical pixels preserve its interleaving with
    /// line/rectangle stores without adding a second asset loader or atlas.
    planning_badges: [Option<PlanningBadge>; 2],
}

pub(crate) struct PlanningBadge {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) rgba: Vec<u8>,
}

impl SoftwareCursor {
    pub(crate) fn planning_badge(&self, frame: usize) -> Option<&PlanningBadge> {
        self.planning_badges.get(frame.checked_sub(433)?)?.as_ref()
    }
    /// Look up a cursor sequence by id, falling back to Default if not found.
    pub(crate) fn get(&self, id: CursorId) -> Option<&SoftwareCursorSequence> {
        self.sequences
            .get(&id)
            .or_else(|| self.sequences.get(&CursorId::Default))
    }
}

pub(crate) struct SoftwareCursorFrame {
    pub(crate) texture: BatchTexture,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

pub(crate) struct SoftwareCursorSequence {
    pub(crate) frames: Vec<SoftwareCursorFrame>,
    pub(crate) interval_ms: u64,
    pub(crate) hotspot: [f32; 2],
}
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use image::RgbaImage;
use std::path::Path;

/// Hotspot position within the FullWidth × FullHeight canvas.
/// Matches the near/centre/far sentinels the two hotspot fields of a descriptor
/// row carry (`0`, `12345`, `54321`; see the `CursorId` header).
/// The hotspot pixel is the one that registers as the actual click point.
#[derive(Clone, Copy)]
enum CursorHotspot {
    /// Left, Top — tip of an arrow pointing upper-left (Default cursor).
    TopLeft,
    /// Center, Top — top-center edge (scroll north).
    CenterTop,
    /// Right, Top — top-right corner (scroll north-east).
    RightTop,
    /// Right, Middle — right edge (scroll east).
    RightMiddle,
    /// Right, Bottom — bottom-right corner (scroll south-east).
    RightBottom,
    /// Center, Bottom — bottom-center edge (scroll south).
    CenterBottom,
    /// Left, Bottom — bottom-left corner (scroll south-west).
    LeftBottom,
    /// Left, Middle — left edge (scroll west).
    LeftMiddle,
    /// Center, Middle — center of frame (attack, move, select, deploy).
    CenterMiddle,
}

/// The cursor animation clock is wall-clock milliseconds shifted right by four,
/// so one clock unit is 16 ms. Rates in the compiled cursor table are counted in
/// these units, not in display frames.
pub(crate) const CURSOR_TIMER_UNIT_MS: u64 = 16;

/// Every animated row of the compiled cursor table carries a rate of 4 clock
/// units, so an animated cursor advances one frame every 64 ms. The previous
/// 133 ms here came from a display-FPS guess and ran every animated cursor at
/// roughly half speed.
pub(crate) const ANIM_RATE_UNITS: u64 = 4;
const ANIM_INTERVAL_MS: u64 = ANIM_RATE_UNITS * CURSOR_TIMER_UNIT_MS;

/// Complete cursor definition table. Frame ranges and rates are compiled into
/// the game executable (not INI-driven); rows carrying `0` are static in the
/// original and must not animate.
/// Format: (CursorId, start_frame, frame_count, interval_ms, hotspot).
const CURSOR_DEFS: &[(CursorId, usize, usize, u64, CursorHotspot)] = &[
    // --- Core gameplay cursors ---
    // Native82D028 row0: normal frame0 and static small variant frame1.
    (CursorId::Default, 0, 2, 0, CursorHotspot::TopLeft),
    (
        CursorId::Select,
        18,
        13,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::Move,
        31,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::NoMove, 41, 1, 0, CursorHotspot::CenterMiddle),
    (
        CursorId::Attack,
        53,
        5,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::AttackOutOfRange,
        58,
        5,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::AttackMove,
        404,
        9,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // Guard-area reticle. Cursor table row 22: start 68, count 5, rate 4.
    (
        CursorId::GuardArea,
        68,
        5,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::Deploy,
        110,
        9,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::NoDeploy, 119, 1, 0, CursorHotspot::CenterMiddle),
    // --- Radar (minimap) cursor sets: the miniFrame/miniCount pairs ---
    // Row 18 (Move)'s mini set. The RadarClass mouse handler (0x006539D0)
    // reaches it through the mini switch: actions 1 (Move) and 5 land on
    // row 18, and action 2 (NoMove) takes row 18 while a drag owns the
    // radar and row 19 otherwise.
    (
        CursorId::MinimapMove,
        42,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // Row 19's mini set — the NoMove mini.
    (
        CursorId::MinimapNoMove,
        52,
        1,
        0,
        CursorHotspot::CenterMiddle,
    ),
    // Rows 20/21/71 share mini frame 63 with count 5; row 26's mini set
    // starts at the same frame 63 with count 1. Both live in the table.
    (
        CursorId::MinimapAttack,
        63,
        5,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::MinimapNoEnter,
        63,
        1,
        0,
        CursorHotspot::CenterMiddle,
    ),
    // Row 22's mini set.
    (
        CursorId::MinimapGuardArea,
        73,
        5,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // Row 25's mini set.
    (
        CursorId::MinimapEnter,
        100,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // Superweapon targeting mini sets, one per targeting row that carries a
    // mini pair: Nuke 513/1 (row 53), LightningStorm 514/1 (row 50),
    // SpyPlane 512/1 (row 85), PsychicReveal 515/1 (row 84).
    (
        CursorId::NukeMini,
        513,
        1,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::LightningStormMini,
        514,
        1,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::SpyPlaneMini,
        512,
        1,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::PsychicRevealMini,
        515,
        1,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // Rows 47 (Paradrop) and 83 (PsychicDominator) share mini frame 516 with
    // miniCount -1: the signed modulo in Advance pins the counter at 0, so
    // the set is static. Row 47's rate is 0 and row 83's is 4 — both pin.
    (
        CursorId::ParadropMini,
        516,
        1,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::PsychicDominatorMini,
        516,
        1,
        0,
        CursorHotspot::CenterMiddle,
    ),
    // Row 0's mini set — the plain arrow the radar defaults to when the
    // action does not survive its keep-table filter. Its fallthrough exit is
    // `SetCursorRow(0, mini = 1)` at `0x00653D62`.
    (CursorId::DefaultMini, 1, 1, 0, CursorHotspot::TopLeft),
    (
        CursorId::Enter,
        89,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::NoEnter, 99, 1, 0, CursorHotspot::CenterMiddle),
    (
        CursorId::EngineerRepair,
        170,
        20,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::TogglePower,
        339,
        6,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::NoTogglePower,
        384,
        1,
        0,
        CursorHotspot::CenterMiddle,
    ),
    // --- Directional scroll cursors (can scroll) ---
    (CursorId::ScrollN, 2, 1, 0, CursorHotspot::CenterTop),
    (CursorId::ScrollNE, 3, 1, 0, CursorHotspot::RightTop),
    (CursorId::ScrollE, 4, 1, 0, CursorHotspot::RightMiddle),
    (CursorId::ScrollSE, 5, 1, 0, CursorHotspot::RightBottom),
    (CursorId::ScrollS, 6, 1, 0, CursorHotspot::CenterBottom),
    (CursorId::ScrollSW, 7, 1, 0, CursorHotspot::LeftBottom),
    (CursorId::ScrollW, 8, 1, 0, CursorHotspot::LeftMiddle),
    (CursorId::ScrollNW, 9, 1, 0, CursorHotspot::TopLeft),
    // --- Directional scroll cursors (can't scroll further) ---
    (CursorId::NoMoveN, 10, 1, 0, CursorHotspot::CenterTop),
    (CursorId::NoMoveNE, 11, 1, 0, CursorHotspot::RightTop),
    (CursorId::NoMoveE, 12, 1, 0, CursorHotspot::RightMiddle),
    (CursorId::NoMoveSE, 13, 1, 0, CursorHotspot::RightBottom),
    (CursorId::NoMoveS, 14, 1, 0, CursorHotspot::CenterBottom),
    (CursorId::NoMoveSW, 15, 1, 0, CursorHotspot::LeftBottom),
    (CursorId::NoMoveW, 16, 1, 0, CursorHotspot::LeftMiddle),
    (CursorId::NoMoveNW, 17, 1, 0, CursorHotspot::TopLeft),
    // --- Right-drag pan, cursor-table rows 61..69 ---
    // Read from the 28-byte descriptor table at 0x0082D028 (Set_Mouse_Cursor_Shape
    // 0x005BDC80 indexes it by `shape * 0x1C`): rows 61..69 are StartFrame
    // 385..393, each count 1, rate 0, hotspot sentinels 0x3039/0x3039 =
    // centre/centre. The same decode reproduces rows 1..8 and 9..16 as the
    // already-landed ScrollN..NW (frames 2..9) and NoMoveN..NW (frames 10..17)
    // with all sixteen of their hotspots, so the stride and base are confirmed.
    (CursorId::Pan, 385, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanN, 386, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanNE, 387, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanE, 388, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanSE, 389, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanS, 390, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanSW, 391, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanW, 392, 1, 0, CursorHotspot::CenterMiddle),
    (CursorId::PanNW, 393, 1, 0, CursorHotspot::CenterMiddle),
    // --- Sell / repair mode ---
    (
        CursorId::Sell,
        129,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::SellUnit,
        139,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::NoSell, 149, 1, 0, CursorHotspot::CenterMiddle),
    (
        CursorId::Repair,
        150,
        20,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::NoRepair, 190, 1, 0, CursorHotspot::CenterMiddle),
    // --- Special unit cursors ---
    (
        CursorId::DesolatorDeploy,
        78,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::GIDeploy, 239, 10, 0, CursorHotspot::CenterMiddle),
    (CursorId::Crush, 219, 5, 0, CursorHotspot::CenterMiddle),
    // Rows 54 and 51 carry ten frames at rate 0: the original keeps them on
    // one frame rather than cycling the strip.
    (CursorId::Tote, 329, 10, 0, CursorHotspot::CenterMiddle),
    // Row 38 (`0x0082D450`): start 204, count 5, rate 0 — a still frame.
    (CursorId::IvanBomb, 204, 5, 0, CursorHotspot::CenterMiddle),
    (CursorId::Detonate, 299, 10, 0, CursorHotspot::CenterMiddle),
    (
        CursorId::Demolish,
        309,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::Disarm, 369, 15, 0, CursorHotspot::CenterMiddle),
    // --- Spy / infiltration ---
    (CursorId::Disguise, 199, 5, 0, CursorHotspot::CenterMiddle),
    (CursorId::SpyTech, 224, 5, 0, CursorHotspot::CenterMiddle),
    (CursorId::SpyPower, 229, 5, 0, CursorHotspot::CenterMiddle),
    // --- Mind control ---
    (
        CursorId::MindControl,
        209,
        5,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::NoMindControl,
        431,
        1,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::RemoveSquid,
        214,
        5,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::InfantryAbsorb,
        422,
        9,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // --- Superweapons ---
    (
        CursorId::Nuke,
        319,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    // Rows 58, 57 and 47 all carry rate 0 — these three reticles are single
    // static frames in the original, not animations.
    (
        CursorId::Chronosphere,
        357,
        12,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::IronCurtain,
        346,
        5,
        0,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::LightningStorm,
        279,
        20,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (CursorId::Paradrop, 259, 10, 0, CursorHotspot::CenterMiddle),
    (
        CursorId::ForceShield,
        450,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::NoForceShield,
        460,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::GeneticMutator,
        470,
        10,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::AirStrike,
        480,
        8,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::PsychicDominator,
        488,
        8,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::PsychicReveal,
        496,
        8,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::SpyPlane,
        504,
        8,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
    (
        CursorId::Beacon,
        435,
        15,
        ANIM_INTERVAL_MS,
        CursorHotspot::CenterMiddle,
    ),
];

pub(crate) fn build_software_cursor(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
) -> Option<SoftwareCursor> {
    // Init_Game 0x0052BA60 converts MOUSEPAL.PAL like every palette load.
    let palette = asset_manager
        .get_ref("mousepal.pal")
        .and_then(|data| Palette::from_bytes(data).ok())?;
    let shp = if let Some(load) = asset_manager
        .load_file_from_mix("mouse.sha")
        .or_else(|| asset_manager.load_file_from_mix("mouse.shp"))
    {
        ShpFile::from_bytes(&load.bytes).ok()?
    } else {
        let shp_data = asset_manager
            .get_ref("mouse.sha")
            .or_else(|| asset_manager.get_ref("mouse.shp"))?;
        ShpFile::from_bytes(shp_data).ok()?
    };
    if shp.frames.is_empty() {
        return None;
    }

    maybe_export_mouse_sheet(&shp, &palette);

    let mut sequences: HashMap<CursorId, SoftwareCursorSequence> = HashMap::new();
    for &(id, start, count, interval_ms, hotspot) in CURSOR_DEFS {
        if let Some(seq) = build_cursor_sequence(
            gpu,
            batch,
            &shp,
            &palette,
            start,
            count,
            interval_ms,
            hotspot,
        ) {
            sequences.insert(id, seq);
        } else if id == CursorId::Default {
            // Default cursor must load successfully — bail if it fails.
            return None;
        }
        // Non-default cursors that fail silently skip; SoftwareCursor::get() falls back to Default.
    }
    let planning_badges = [433, 434].map(|frame| {
        Some(PlanningBadge {
            width: i32::from(shp.width),
            height: i32::from(shp.height),
            rgba: frame_to_canvas_rgba(&shp, frame, &palette).ok()?,
        })
    });
    Some(SoftwareCursor {
        sequences,
        planning_badges,
    })
}

fn build_cursor_sequence(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    shp: &ShpFile,
    palette: &Palette,
    start: usize,
    count: usize,
    interval_ms: u64,
    hotspot: CursorHotspot,
) -> Option<SoftwareCursorSequence> {
    let mut frames = Vec::new();
    for frame_idx in start..start + count {
        if let Some(frame) = build_cursor_frame(gpu, batch, shp, palette, frame_idx) {
            frames.push(frame);
        }
    }
    if frames.is_empty() {
        return None;
    }
    Some(SoftwareCursorSequence {
        frames,
        interval_ms,
        hotspot: resolve_hotspot(hotspot, shp),
    })
}

fn resolve_hotspot(hotspot: CursorHotspot, shp: &ShpFile) -> [f32; 2] {
    hotspot_offset(hotspot, shp.width, shp.height)
}

/// `Set_Mouse_Cursor_Shape` (`0x005BDC80`) decodes the record's two hotspot
/// sentinel fields against the mouse.sha canvas — `word[+2]`/`word[+4]` of
/// the file header, the same words `ShpFile::from_bytes` reads as
/// `width`/`height`: `12345` (centre) is a truncating half
/// (`movsx; cdq; sub; sar 1` at `0x005BDD33`), `54321` (far edge) is the
/// full width/height (`movsx edi, word [ebx+2]` at `0x005BDD52`), and `0`
/// (near edge) is 0. Integer division matters: an odd canvas halves toward
/// zero, and the far edge is one pixel past the last one.
fn hotspot_offset(hotspot: CursorHotspot, width: u16, height: u16) -> [f32; 2] {
    let w = width as f32;
    let h = height as f32;
    let half_w = (width / 2) as f32;
    let half_h = (height / 2) as f32;
    match hotspot {
        CursorHotspot::TopLeft => [0.0, 0.0],
        CursorHotspot::CenterTop => [half_w, 0.0],
        CursorHotspot::RightTop => [w, 0.0],
        CursorHotspot::RightMiddle => [w, half_h],
        CursorHotspot::RightBottom => [w, h],
        CursorHotspot::CenterBottom => [half_w, h],
        CursorHotspot::LeftBottom => [0.0, h],
        CursorHotspot::LeftMiddle => [0.0, half_h],
        CursorHotspot::CenterMiddle => [half_w, half_h],
    }
}

fn build_cursor_frame(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    shp: &ShpFile,
    palette: &Palette,
    frame_idx: usize,
) -> Option<SoftwareCursorFrame> {
    if frame_idx >= shp.frames.len() {
        return None;
    }
    let rgba = frame_to_canvas_rgba(shp, frame_idx, palette).ok()?;
    let texture = batch.create_texture(gpu, &rgba, shp.width as u32, shp.height as u32);
    Some(SoftwareCursorFrame {
        texture,
        width: shp.width as f32,
        height: shp.height as f32,
    })
}

fn maybe_export_mouse_sheet(shp: &ShpFile, palette: &Palette) {
    let enabled = std::env::var("RA2_DEBUG_MOUSE_CURSOR_SHEET")
        .ok()
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false);
    if !enabled {
        return;
    }

    let max_frames = shp.frames.len();
    let cols = 10u32;
    let rows = (max_frames as u32).div_ceil(cols);
    let cell_w = shp.width as u32;
    let cell_h = shp.height as u32;
    let sheet_w = cols * cell_w;
    let sheet_h = rows * cell_h;
    let mut rgba = vec![0u8; (sheet_w * sheet_h * 4) as usize];
    fill_checkerboard(&mut rgba, sheet_w, sheet_h);

    for frame_idx in 0..max_frames {
        let Ok(frame_rgba) = frame_to_canvas_rgba(shp, frame_idx, palette) else {
            continue;
        };
        let col = (frame_idx as u32) % cols;
        let row = (frame_idx as u32) / cols;
        let dst_x = col * cell_w;
        let dst_y = row * cell_h;
        blit_rgba(
            &mut rgba,
            sheet_w,
            &frame_rgba,
            cell_w,
            cell_h,
            dst_x,
            dst_y,
        );
        draw_cell_border(&mut rgba, sheet_w, sheet_h, dst_x, dst_y, cell_w, cell_h);
    }

    if let Some(img) = RgbaImage::from_raw(sheet_w, sheet_h, rgba) {
        let _ = img.save(Path::new("debug_mouse_cursor_sheet.png"));
        log::info!(
            "Saved debug_mouse_cursor_sheet.png ({} frames, {}x{})",
            max_frames,
            sheet_w,
            sheet_h
        );
    }
}

fn fill_checkerboard(dst: &mut [u8], width: u32, height: u32) {
    for y in 0..height {
        for x in 0..width {
            let light = ((x / 4) + (y / 4)) % 2 == 0;
            let value = if light { 54 } else { 32 };
            let idx = ((y * width + x) * 4) as usize;
            dst[idx] = value;
            dst[idx + 1] = value;
            dst[idx + 2] = value;
            dst[idx + 3] = 255;
        }
    }
}

fn draw_cell_border(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    dst_x: u32,
    dst_y: u32,
    cell_w: u32,
    cell_h: u32,
) {
    let max_x = (dst_x + cell_w).min(dst_width);
    let max_y = (dst_y + cell_h).min(dst_height);
    for x in dst_x..max_x {
        set_rgba(dst, dst_width, x, dst_y, [255, 0, 255, 255]);
        if max_y > 0 {
            set_rgba(dst, dst_width, x, max_y - 1, [255, 0, 255, 255]);
        }
    }
    for y in dst_y..max_y {
        set_rgba(dst, dst_width, dst_x, y, [255, 0, 255, 255]);
        if max_x > 0 {
            set_rgba(dst, dst_width, max_x - 1, y, [255, 0, 255, 255]);
        }
    }
}

fn set_rgba(dst: &mut [u8], dst_width: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let idx = ((y * dst_width + x) * 4) as usize;
    if idx + 3 >= dst.len() {
        return;
    }
    dst[idx..idx + 4].copy_from_slice(&rgba);
}

fn blit_rgba(
    dst: &mut [u8],
    dst_width: u32,
    src: &[u8],
    src_width: u32,
    src_height: u32,
    dst_x: u32,
    dst_y: u32,
) {
    for y in 0..src_height {
        for x in 0..src_width {
            let src_i = ((y * src_width + x) * 4) as usize;
            let dst_i = (((dst_y + y) * dst_width + (dst_x + x)) * 4) as usize;
            if src_i + 3 >= src.len() || dst_i + 3 >= dst.len() {
                continue;
            }
            dst[dst_i..dst_i + 4].copy_from_slice(&src[src_i..src_i + 4]);
        }
    }
}

fn frame_to_canvas_rgba(shp: &ShpFile, frame_idx: usize, palette: &Palette) -> Result<Vec<u8>, ()> {
    let frame = shp.frames.get(frame_idx).ok_or(())?;
    let frame_rgba = shp.frame_to_rgba(frame_idx, palette).map_err(|_| ())?;
    let canvas_w = shp.width as u32;
    let canvas_h = shp.height as u32;
    let mut canvas = vec![0u8; (canvas_w * canvas_h * 4) as usize];
    blit_rgba(
        &mut canvas,
        canvas_w,
        &frame_rgba,
        frame.frame_width as u32,
        frame.frame_height as u32,
        frame.frame_x as u32,
        frame.frame_y as u32,
    );
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: CursorId) -> (usize, usize, u64) {
        CURSOR_DEFS
            .iter()
            .find(|(row_id, ..)| *row_id == id)
            .map(|&(_, start, count, interval, _)| (start, count, interval))
            .expect("cursor row")
    }

    /// Spot-check the rows a player sees most: select, move, the two attack
    /// reticles and attack-move all animate at the same 64 ms.
    #[test]
    fn common_gameplay_cursors_carry_the_retail_range_and_rate() {
        assert_eq!(row(CursorId::Select), (18, 13, 64));
        assert_eq!(row(CursorId::Move), (31, 10, 64));
        assert_eq!(row(CursorId::Attack), (53, 5, 64));
        assert_eq!(row(CursorId::AttackOutOfRange), (58, 5, 64));
        assert_eq!(row(CursorId::AttackMove), (404, 9, 64));
    }

    /// Every loaded frame set must come from the compiled descriptor table,
    /// with that record's frame count, rate and centre hotspot. Default packs
    /// both static row-0 sets so Planning Mode can select its small arrow.
    /// The table is
    /// dumped from the executable by `tools/mouse_oracle/cursor_table.py`, so a
    /// row guessed from a document or drifted by one frame fails here. Each
    /// record contributes both its main set and its `miniFrame` set.
    #[test]
    fn cursor_rows_match_the_compiled_descriptor_table() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/mouse_oracle/cursor_table.json",
        ))
        .unwrap();
        // Every frame set the table carries: each record's main set plus its
        // mini set. A Vec, not a frame-keyed map — frame 63 begins two
        // different mini sets (rows 20/21/71 count 5, row 26 count 1) and
        // frame 516 two rows' static mini set, so a start frame does not
        // identify a set on its own.
        let mut sets: Vec<(usize, usize, u64, u64, u64)> = Vec::new();
        for record in native["rows"].as_array().expect("rows") {
            let rate = record["rate"].as_u64().expect("rate");
            let hotspots = (
                record["hotspot_x"].as_u64().expect("hotspot x"),
                record["hotspot_y"].as_u64().expect("hotspot y"),
            );
            sets.push((
                record["frame"].as_u64().expect("frame") as usize,
                record["count"].as_u64().expect("count") as usize,
                rate,
                hotspots.0,
                hotspots.1,
            ));
            if let Some(frame) = record["mini_frame"].as_u64() {
                // miniCount -1 (null here): the signed modulo in Advance
                // divides by -1, the remainder is always 0, so the set is
                // static whatever the row's rate.
                let (count, mini_rate) = match record["mini_count"].as_u64() {
                    Some(count) => (count as usize, rate),
                    None => (1, 0),
                };
                sets.push((frame as usize, count, mini_rate, hotspots.0, hotspots.1));
            }
        }
        for &(id, start, count, interval, hotspot) in CURSOR_DEFS {
            let loaded_sets = if id == CursorId::Default {
                // build_software_cursor_instances selects frames[1] for the
                // PlanMgr63AC41 small-arrow route. This is two native static
                // sets in one atlas allocation, not a two-frame animation.
                assert_eq!(count, 2, "Default packs normal and small arrows");
                vec![(start, 1), (start + 1, 1)]
            } else {
                vec![(start, count)]
            };
            for (set_start, set_count) in loaded_sets {
                let Some(&(_, _, rate, hx, hy)) = sets.iter().find(|&&(f, c, r, ..)| {
                    f == set_start && c == set_count && r * CURSOR_TIMER_UNIT_MS == interval
                }) else {
                    panic!(
                        "{id:?} starts at frame {set_start} with count {set_count} and {interval} ms, \
                         which begins no frame set of the compiled descriptor table"
                    );
                };
                assert_eq!(
                    rate * CURSOR_TIMER_UNIT_MS,
                    interval,
                    "{id:?} carries rate {rate} units"
                );
                if hx == 12345 && hy == 12345 {
                    assert!(
                        matches!(hotspot, CursorHotspot::CenterMiddle),
                        "{id:?} carries centre/centre hotspot sentinels"
                    );
                }
            }
        }
    }

    /// `0x005BDD33` halves the canvas toward zero and `0x005BDD52` takes the
    /// full canvas: on the 88x68 mouse.sha the centre hotspot is 44,34 and
    /// the far corner one pixel past the last one at 88,68 — not 87,67, and
    /// not a float half.
    #[test]
    fn hotspot_sentinels_decode_to_truncating_centre_and_full_far_edge() {
        assert_eq!(
            hotspot_offset(CursorHotspot::CenterMiddle, 88, 68),
            [44.0, 34.0]
        );
        assert_eq!(
            hotspot_offset(CursorHotspot::RightBottom, 88, 68),
            [88.0, 68.0]
        );
        assert_eq!(hotspot_offset(CursorHotspot::RightTop, 88, 68), [88.0, 0.0]);
        assert_eq!(
            hotspot_offset(CursorHotspot::LeftMiddle, 88, 68),
            [0.0, 34.0]
        );
        assert_eq!(hotspot_offset(CursorHotspot::TopLeft, 88, 68), [0.0, 0.0]);
        // Odd canvas: 7 / 2 truncates to 3, the far edge is 7.
        assert_eq!(
            hotspot_offset(CursorHotspot::CenterMiddle, 7, 7),
            [3.0, 3.0]
        );
        assert_eq!(hotspot_offset(CursorHotspot::RightBottom, 7, 7), [7.0, 7.0]);
    }

    /// The radar's default cursor is row 0's mini set — the plain arrow at
    /// frame 1 — not the tactical main arrow (frame 0) and not the move
    /// cluster: the RadarClass handler's fallthrough exit is
    /// `SetCursorRow(0, mini = 1)` at `0x00653D62`, and its keep-table filter
    /// (`0x00653F04`, action-1 indexed) zeroes every action outside
    /// {1,2,3,5,6,9,16,20,26,38,41,65,66,67,72} to row 0.
    #[test]
    fn radar_default_cursor_is_row_zero_mini_set() {
        assert_eq!(row(CursorId::DefaultMini), (1, 1, 0));
        // Default's atlas also keeps row0's small frame for Planning Mode.
        assert_eq!(row(CursorId::Default), (0, 2, 0));
    }

    /// The superweapon targeting rows that carry a mini pair keep their own
    /// mini set on the radar (mini switch action -> row -> mini): Nuke 513/1,
    /// LightningStorm 514/1, SpyPlane 512/1, PsychicReveal 515/1, and the
    /// static 516 shared by Paradrop and PsychicDominator.
    #[test]
    fn superweapon_mini_sets_are_the_targeting_rows_second_frame_sets() {
        assert_eq!(row(CursorId::NukeMini), (513, 1, ANIM_INTERVAL_MS));
        assert_eq!(
            row(CursorId::LightningStormMini),
            (514, 1, ANIM_INTERVAL_MS)
        );
        assert_eq!(row(CursorId::SpyPlaneMini), (512, 1, ANIM_INTERVAL_MS));
        assert_eq!(row(CursorId::PsychicRevealMini), (515, 1, ANIM_INTERVAL_MS));
        assert_eq!(row(CursorId::ParadropMini), (516, 1, 0));
        assert_eq!(row(CursorId::PsychicDominatorMini), (516, 1, 0));
    }

    /// The minimap's Move cursor is not a row of its own: frames 42-51 are row
    /// 18's second set, the `miniFrame`/`miniCount` pair of its record.
    #[test]
    fn minimap_move_is_the_move_rows_second_frame_set() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/mouse_oracle/cursor_table.json",
        ))
        .unwrap();
        let move_row = native["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["frame"].as_u64() == Some(31))
            .expect("row 18 starts at frame 31");
        let (start, count, _) = row(CursorId::MinimapMove);
        assert_eq!(
            (
                move_row["mini_frame"].as_u64().expect("mini frame") as usize,
                move_row["mini_count"].as_u64().expect("mini count") as usize
            ),
            (start, count),
            "MinimapMove must stay row 18's mini set"
        );
    }

    /// The actions `input::cursor` cites by number must reach the row whose
    /// frames this table gives to the named `CursorId`. It ties the three links
    /// the original separates — action, descriptor row, frame set — so a citation
    /// in a comment cannot drift away from either table.
    #[test]
    fn cited_actions_reach_the_row_the_named_cursor_uses() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/mouse_oracle/cursor_table.json",
        ))
        .unwrap();
        let by_row: HashMap<usize, &serde_json::Value> = native["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|record| (record["row"].as_u64().expect("row") as usize, record))
            .collect();
        let entries = native["action_switch"]["entries"]
            .as_array()
            .expect("entries");
        assert_eq!(
            entries.len(),
            native["action_switch"]["entry_count"]
                .as_u64()
                .expect("entry count") as usize,
            "the fixture must carry as many entries as the dispatch's bound"
        );

        for (action, id) in [
            (1u32, CursorId::Move),
            (2, CursorId::NoMove),
            (9, CursorId::Enter),
            (4, CursorId::Deploy),
            (16, CursorId::Demolish),
            (30, CursorId::NoDeploy),
            (64, CursorId::Demolish),
            (71, CursorId::AirStrike),
            (53, CursorId::IvanBomb),
            (57, CursorId::Disarm),
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry["action"].as_u64() == Some(u64::from(action)))
                .unwrap_or_else(|| panic!("the switch records no entry for action {action}"));
            let row_number = entry["row"].as_u64().expect("entry row") as usize;
            let record = by_row
                .get(&row_number)
                .unwrap_or_else(|| panic!("action {action} reaches missing row {row_number}"));
            let (start, count, _) = CURSOR_DEFS
                .iter()
                .find(|(cursor, ..)| *cursor == id)
                .map(|&(_, start, count, interval, _)| (start, count, interval))
                .expect("cursor row");
            assert_eq!(
                (
                    record["frame"].as_u64().expect("frame") as usize,
                    record["count"].as_u64().expect("count") as usize
                ),
                (start, count),
                "action {action} reaches row {row_number}, which is not {id:?}'s frame set"
            );
        }
    }
}
