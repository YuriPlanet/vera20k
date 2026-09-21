//! Camera positioning — keyboard scroll, mouse edge scroll, zoom, and clamping.
//!
//! Split from `match_runtime::sim_tick` to separate camera control from sim advancement.
//!
//! ## Coordinate frames
//! Three frames meet in this file and mixing them is the classic bug here:
//! * **World pixels** — what `terrain::iso_to_screen` / `terrain::lepton_to_screen`
//!   produce. `camera_x`/`camera_y` live in this frame and name the world point
//!   drawn at window pixel `(0, 0)`. Within world pixels there are two anchors
//!   that differ by half a tile: `iso_to_screen` gives a cell's *tile* corner,
//!   while an entity standing on that cell is drawn on the cell's diamond
//!   centre. `cell_centre_world_point` converts the first into the second and
//!   is what every camera move targets.
//! * **Window pixels** — `render_width()` × `render_height()`, cursor position,
//!   sidebar width. The batch shader maps `screen = (world - camera) * zoom`, so
//!   converting a window-pixel extent into world pixels is a divide by `zoom`.
//! * **Tactical-viewport pixels** — window pixels minus the sidebar column on the
//!   right. gamemd anchors the camera on the centre of *this* rect, not the
//!   window's, and its scroll clamp carries the same `viewport/2` terms.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::types::ScrollDir;
use crate::map::terrain;

/// Stock YR reserves a fixed 168-pixel right sidebar and a fixed 32-pixel
/// bottom strip when it builds the tactical view rectangle.
const TACTICAL_SIDEBAR_WIDTH_PX: u32 = 168;
const TACTICAL_BOTTOM_STRIP_HEIGHT_PX: u32 = 32;

/// Arrow-key scroll distance in world pixels per frame.
///
/// gamemd's main tick reads one flat constant for every arrow key. This path has
/// no coast ramp and no `ScrollRate` term — that machinery belongs to mouse edge
/// scrolling alone — and holding two arrows moves the full distance on both axes
/// because each key issues its own scroll.
const KEY_SCROLL_DISTANCE: f32 = 21.0;

/// Shift scales the arrow-scroll distance by 2.5 and the product goes through
/// the same truncating float-to-long as the rest of the scroll math, so the
/// boosted step is 52 px per frame rather than 52.5.
const KEY_SCROLL_SHIFT_MULTIPLIER: f32 = 2.5;

/// Ctrl replaces the distance with the map's longer side in cells, shifted left
/// by the lepton-per-cell shift. That overshoots every stock map, so the clamp
/// catches it and the view lands on the map border in a single frame.
const KEY_SCROLL_CTRL_CELL_SHIFT: u32 = 8;

/// Minimum zoom level — zoomed out enough to see a large portion of the map.
const MIN_ZOOM: f32 = 0.25;
/// Maximum zoom level — zoomed in close to pixel-level detail.
const MAX_ZOOM: f32 = 4.0;
/// Multiplicative zoom step per mouse wheel notch (smooth exponential zoom).
const ZOOM_STEP: f32 = 1.15;

// ---------------------------------------------------------------------------
// Edge auto-scroll — gamemd's CoastLevel ramp.
// ---------------------------------------------------------------------------
//
// Verified by live decompilation of the edge-scroll entry point this session.
// The mechanism is: a one-pixel at-edge band around the whole window; a
// nine-zone reference point that turns "which edge am I pushing" into a
// direction; a coast counter that ramps once per 16 ms while the cursor stays
// at the edge and decays at the same cadence once it leaves; and a speed table
// indexed by `8 - coast`, capped by the player's `[Options] ScrollRate`.

/// Raw per-frame scroll distances before the rules multiplier, indexed by
/// `8 - CoastLevel`. Read verbatim out of the running image this session.
///
/// Index 0 is unreachable in a stock game: the cap below forces the index to at
/// least `ScrollRate + 1`, and the options slider never reports `ScrollRate` below 0.
const EDGE_SCROLL_SPEED_TABLE: [i32; 9] = [448, 384, 320, 256, 192, 128, 64, 32, 16];

/// Constructor fallback for `[AudioVisual] ScrollMultiplier`.
const DEFAULT_SCROLL_MULTIPLIER: f64 = 0.07;

/// Fraction of the window width that splits the nine-zone direction bands on X.
const EDGE_SCROLL_ZONE_FRACTION_X: f64 = 0.16;
/// Fraction of the window height that splits the nine-zone direction bands on Y.
const EDGE_SCROLL_ZONE_FRACTION_Y: f64 = 0.21;

/// gamemd's radar timer counts `timeGetTime() >> 4`, so one unit is 16 ms.
const RADAR_TIMER_MS_SHIFT: u32 = 4;
/// CoastLevel moves by one step per radar-timer tick, up or down.
const COAST_STEP_TICKS: u32 = 1;

/// Radians to 16-bit direction units. Negative because screen Y grows downward
/// while the direction word increases clockwise from north.
const DIR16_PER_RADIAN: f64 = -(32768.0 / std::f64::consts::PI);

/// Camera step per octant in window pixels, ordered N, NE, E, SE, S, SW, W, NW —
/// the same eight `(dx, dy)` pairs gamemd's scroll routine indexes.
///
/// Diagonals move the full distance on **both** axes; they are not normalised.
const OCTANT_DELTA: [(f32, f32); 8] = [
    (0.0, -1.0),
    (1.0, -1.0),
    (1.0, 0.0),
    (1.0, 1.0),
    (0.0, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.0),
    (-1.0, -1.0),
];

/// Live edge-scroll ramp state. One instance per `AppState`.
#[derive(Debug, Clone)]
pub(crate) struct EdgeScrollState {
    /// Wall-clock origin for the radar timer. Edge scroll is driven by the real
    /// cursor, so it is presentation state and never enters the sim.
    epoch: std::time::Instant,
    /// gamemd's CoastLevel: 0 is the slowest creep, higher is faster.
    coast_level: i32,
    /// Last direction octant. Retained so the map keeps coasting in the same
    /// direction while the counter decays after the cursor leaves the edge.
    octant: usize,
    /// Radar-timer stamp of the last coast change. `None` matches gamemd's
    /// zero-delay start, where the very first step is allowed immediately.
    last_coast_change: Option<u32>,
}

impl Default for EdgeScrollState {
    fn default() -> Self {
        Self {
            epoch: std::time::Instant::now(),
            coast_level: 0,
            octant: 0,
            last_coast_change: None,
        }
    }
}

impl EdgeScrollState {
    /// Current radar-timer reading (16 ms units).
    pub(crate) fn radar_timer(&self) -> u32 {
        (self.epoch.elapsed().as_millis() as u32) >> RADAR_TIMER_MS_SHIFT
    }

    fn timer_expired(&self, now: u32) -> bool {
        match self.last_coast_change {
            None => true,
            Some(stamp) => now.wrapping_sub(stamp) >= COAST_STEP_TICKS,
        }
    }

    fn ramp_up(&mut self, now: u32) {
        if self.timer_expired(now) {
            self.last_coast_change = Some(now);
            self.coast_level += 1;
        }
    }

    fn decay(&mut self, now: u32) {
        if self.timer_expired(now) {
            self.last_coast_change = Some(now);
            self.coast_level = (self.coast_level - 1).max(0);
        }
    }

    fn coasting_direction(&self) -> Option<ScrollDir> {
        (self.coast_level > 0).then(|| scroll_dir_from_octant(self.octant))
    }
}

/// Nine-zone reference coordinate for one axis.
///
/// Below `fraction` of the axis the reference is the near edge, above
/// `1 - fraction` it is the far edge, and in between it is the axis midpoint.
/// The midpoint divide is gamemd's `size / 2` truncating toward zero.
fn zone_reference(value: i32, size: i32, fraction: f64) -> i32 {
    let value = value as f64;
    let size_f = size as f64;
    if value < size_f * fraction {
        0
    } else if value <= (1.0 - fraction) * size_f {
        size / 2
    } else {
        size - 1
    }
}

/// Turn an at-edge cursor position into one of eight scroll directions.
///
/// gamemd takes `atan2` from the window centre toward the nine-zone reference
/// point, converts the angle to a 16-bit direction word, rounds that to the
/// 8-bit direction byte, then rounds again to an octant. Only nine reference
/// points exist, so the result is a small fixed table in practice; the angle
/// arithmetic is reproduced rather than tabulated because the zone split
/// depends on the window's aspect ratio.
fn edge_scroll_octant(x: i32, y: i32, view_w: i32, view_h: i32) -> usize {
    let ref_x = zone_reference(x, view_w, EDGE_SCROLL_ZONE_FRACTION_X);
    let ref_y = zone_reference(y, view_h, EDGE_SCROLL_ZONE_FRACTION_Y);
    let centre_x = view_w / 2;
    let centre_y = view_h / 2;

    let angle = f64::from(centre_y - ref_y).atan2(f64::from(ref_x - centre_x));
    // gamemd's float-to-long helper runs with the x87 rounding mode set to
    // chop, so every conversion here truncates toward zero.
    let dir16 = ((angle - std::f64::consts::FRAC_PI_2) * DIR16_PER_RADIAN).trunc() as i32 as u16;
    // 16-bit direction word to direction byte, then byte to octant. Both are the
    // native "add half a step, then shift" rounding, and both wrap to north.
    let dir8 = (((dir16 >> 7) + 1) >> 1) as u8;
    usize::from(crate::util::direction_tables::dir_from_facing8(dir8))
}

fn scroll_dir_from_octant(octant: usize) -> ScrollDir {
    match octant & 7 {
        0 => ScrollDir::N,
        1 => ScrollDir::NE,
        2 => ScrollDir::E,
        3 => ScrollDir::SE,
        4 => ScrollDir::S,
        5 => ScrollDir::SW,
        6 => ScrollDir::W,
        _ => ScrollDir::NW,
    }
}

fn scroll_dir_octant(dir: ScrollDir) -> usize {
    match dir {
        ScrollDir::N => 0,
        ScrollDir::NE => 1,
        ScrollDir::E => 2,
        ScrollDir::SE => 3,
        ScrollDir::S => 4,
        ScrollDir::SW => 5,
        ScrollDir::W => 6,
        ScrollDir::NW => 7,
    }
}

/// Active edge-scroll intent for a cursor position. The trigger is exactly the
/// outermost integer pixel of the whole window, sidebar included.
pub(crate) fn edge_scroll_intent(
    cursor: (f32, f32),
    view_w: i32,
    view_h: i32,
) -> Option<ScrollDir> {
    if view_w <= 0 || view_h <= 0 {
        return None;
    }
    let x = cursor.0.floor() as i32;
    let y = cursor.1.floor() as i32;
    (x <= 0 || y <= 0 || x >= view_w - 1 || y >= view_h - 1)
        .then(|| scroll_dir_from_octant(edge_scroll_octant(x, y, view_w, view_h)))
}

/// One edge-scroll step. Returns the camera delta in **window pixels**.
///
/// `view_w`/`view_h` are the full window, sidebar included: gamemd's at-edge
/// test adds the sidebar surface width to the composition surface width, so the
/// east band sits at the far right of the screen, past the sidebar — hovering
/// just left of the sidebar does *not* scroll.
///
/// `scroll_rate` is the internal `[Options] ScrollRate` (0 fastest .. 6 slowest).
///
/// `movement_allowed` is the one-pixel preflight through the tactical clamp.
/// A blocked active edge skips the timer block; a blocked coast still decays.
/// `right_button_held` selects the slower `clamp(index + 1, 4, 8)` lane.
fn edge_scroll_step_with_context(
    state: &mut EdgeScrollState,
    cursor: (f32, f32),
    view_w: i32,
    view_h: i32,
    scroll_rate: u32,
    scroll_multiplier: f64,
    right_button_held: bool,
    movement_allowed: bool,
    now: u32,
) -> (f32, f32) {
    let active_direction = edge_scroll_intent(cursor, view_w, view_h);

    // ScrollRate caps the peak speed by flooring the table index, and the coast
    // counter is written back clamped so it cannot run away above the cap.
    // Native applies this cap before a blocked active-edge request returns.
    // The array clamp also makes a user-edited value outside the stock slider
    // range safe without changing ordinary 0..=6 behavior.
    let rate_floor = i32::try_from(scroll_rate)
        .unwrap_or(i32::MAX)
        .saturating_add(1);
    let base_index = ((8 - state.coast_level).max(rate_floor))
        .clamp(0, EDGE_SCROLL_SPEED_TABLE.len() as i32 - 1);
    state.coast_level = 8 - base_index;

    if active_direction.is_some() && !movement_allowed {
        // A blocked active edge returns after the cap but before direction,
        // timer, or ramp state changes.
        return (0.0, 0.0);
    }

    if let Some(direction) = active_direction {
        state.octant = scroll_dir_octant(direction);
    } else if state.coast_level == 0 {
        // Idle: nothing moves, but the decay timer is still serviced.
        state.decay(now);
        return (0.0, 0.0);
    }

    let index = if right_button_held {
        base_index.saturating_add(1).clamp(4, 8)
    } else {
        base_index
    };

    // Truncating float-to-long, matching gamemd's chop rounding mode: the stock
    // multiplier turns the table into 1, 2, 4, 8, 13, 17, 22, 26, 31 px/frame.
    let distance =
        (f64::from(EDGE_SCROLL_SPEED_TABLE[index as usize]) * scroll_multiplier).trunc() as f32;

    if active_direction.is_some() {
        state.ramp_up(now);
    } else {
        state.decay(now);
    }

    if !movement_allowed {
        // A blocked coast still reaches the normal decay above.
        return (0.0, 0.0);
    }

    let (dx, dy) = OCTANT_DELTA[state.octant];
    (dx * distance, dy * distance)
}

#[cfg(test)]
fn edge_scroll_step(
    state: &mut EdgeScrollState,
    cursor: (f32, f32),
    view_w: i32,
    view_h: i32,
    scroll_rate: u32,
    now: u32,
) -> (f32, f32) {
    edge_scroll_step_with_context(
        state,
        cursor,
        view_w,
        view_h,
        scroll_rate,
        DEFAULT_SCROLL_MULTIPLIER,
        false,
        true,
        now,
    )
}

// ---------------------------------------------------------------------------
// Camera bookmarks — the View1..4 / SetView1..4 commands.
// ---------------------------------------------------------------------------

/// Number of camera bookmark slots. gamemd ships exactly four commands per
/// direction (`View1..View4`, `SetView1..SetView4`), bound to F1..F4 and
/// Ctrl+F1..Ctrl+F4 in the stock keyboard INI.
pub(crate) const VIEW_BOOKMARK_SLOTS: usize = 4;

/// The four camera bookmark slots.
///
/// Each slot is a **cell**, not a pixel or lepton position: the recall command
/// projects the cell centre and lands it at the middle of the tactical viewport,
/// then runs the same clamp every other scroll source uses. Recall is an instant
/// jump, not a glide.
///
/// All four slots are seeded with the scenario's opening view at map load, so a
/// recall before any set is a valid "go home" rather than a jump to the map
/// corner. That is why the slots are plain cells rather than `Option`s — the
/// native structure has no empty state.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ViewBookmarks {
    slots: [(u16, u16); VIEW_BOOKMARK_SLOTS],
}

impl ViewBookmarks {
    /// Seed every slot with one cell (the map-load chained assignment).
    pub(crate) fn seed_all(&mut self, rx: u16, ry: u16) {
        self.slots = [(rx, ry); VIEW_BOOKMARK_SLOTS];
    }

    /// Store `cell` in one slot. Out-of-range slots are ignored.
    pub(crate) fn set(&mut self, slot: usize, rx: u16, ry: u16) {
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = (rx, ry);
        }
    }

    /// Read one slot.
    pub(crate) fn get(&self, slot: usize) -> Option<(u16, u16)> {
        self.slots.get(slot).copied()
    }
}

/// The cell under the centre of the tactical viewport — the point `SetView`
/// captures. gamemd reads the *view's* coordinate, not the cursor's, so a
/// bookmark records where the player is looking rather than where the mouse
/// happens to sit.
pub(crate) fn tactical_centre_cell(state: &AppState) -> (u16, u16) {
    let (tactical_w, tactical_h) =
        tactical_viewport_size_px(state.render_width(), state.render_height());
    let world_x = state.match_state.input.camera_x + tactical_w as f32 / (2.0 * state.match_state.input.zoom_level);
    let world_y = state.match_state.input.camera_y + tactical_h as f32 / (2.0 * state.match_state.input.zoom_level);
    crate::app::match_runtime::sim_tick::world_point_to_cell(
        world_x,
        world_y,
        &state.height_map(),
        Some(&state.match_state.match_presentation.tactical_bridge_inverse_map),
    )
}

/// `SetView<slot+1>` — capture the current view into a bookmark.
pub(crate) fn set_view_bookmark(state: &mut AppState, slot: usize) {
    let (rx, ry) = tactical_centre_cell(state);
    state.match_state.input.view_bookmarks.set(slot, rx, ry);
    log::info!("SetView{}: bookmark set to cell ({rx}, {ry})", slot + 1);
}

/// `View<slot+1>` — jump the camera to a bookmark.
pub(crate) fn recall_view_bookmark(state: &mut AppState, slot: usize) {
    let Some((rx, ry)) = state.match_state.input.view_bookmarks.get(slot) else {
        return;
    };
    center_camera_on_cell(state, rx, ry);
}

/// Seed all four bookmarks with the cell the view is currently centred on.
/// Called from the map-load and spawn-pick paths, which are where gamemd's
/// scenario reader fills the four slots with the opening view.
pub(crate) fn seed_view_bookmarks_from_current_view(state: &mut AppState) {
    let (rx, ry) = tactical_centre_cell(state);
    state.match_state.input.view_bookmarks.seed_all(rx, ry);
}

// ---------------------------------------------------------------------------
// Right-drag map pan.
// ---------------------------------------------------------------------------

/// Default `SM_CXDRAG` / `SM_CYDRAG` value used by the non-Windows development
/// fallback. The retail Windows path reads the live per-machine metrics.
#[cfg(not(windows))]
const DEFAULT_SYSTEM_DRAG_METRIC_PX: i32 = 4;

#[cfg(windows)]
const SM_CXDRAG: i32 = 68;
#[cfg(windows)]
const SM_CYDRAG: i32 = 69;

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetSystemMetrics(index: i32) -> i32;
}

/// Current Windows drag metrics, or the ordinary Windows default on development
/// targets that do not expose `GetSystemMetrics`.
fn system_drag_metrics_px() -> (i32, i32) {
    #[cfg(windows)]
    {
        // SAFETY: GetSystemMetrics is a process-wide read with no pointer
        // arguments. SM_CXDRAG and SM_CYDRAG return pixel counts.
        unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) }
    }
    #[cfg(not(windows))]
    {
        (DEFAULT_SYSTEM_DRAG_METRIC_PX, DEFAULT_SYSTEM_DRAG_METRIC_PX)
    }
}

/// gamemd crosses the right-drag latch when either axis is strictly greater
/// than twice its corresponding Windows drag metric. This is per-axis, not a
/// Euclidean-distance test.
fn right_drag_threshold_crossed(
    delta_x: f32,
    delta_y: f32,
    drag_metric_x: i32,
    drag_metric_y: i32,
) -> bool {
    delta_x.abs() > drag_metric_x.saturating_mul(2) as f32
        || delta_y.abs() > drag_metric_y.saturating_mul(2) as f32
}

/// gamemd divides the anchor displacement by `ScrollRate + 1` and truncates, so
/// a right drag held 100 px from its anchor with the stock rate moves the camera
/// 25 px along that axis every frame.
const RIGHT_DRAG_RATE_BIAS: u32 = 1;

/// Distance from a screen border, in pixels, inside which an anchor makes a
/// drag pushing further toward that border boost.
const RIGHT_DRAG_EDGE_BAND_PX: f32 = 10.0;
/// Minimum displacement the edge boost substitutes before the multiply.
const RIGHT_DRAG_EDGE_MIN_PX: i32 = 5;
/// The edge-boost multiplier (a left shift by two in the original).
const RIGHT_DRAG_EDGE_BOOST: i32 = 4;

/// Tactical mouse capture plus the right-drag pan's anchor and latches.
///
/// gamemd sets one "a button is captured" byte on the left *and* the right press
/// inside the play area and clears it on the matching release. While it is set,
/// the per-frame mouse block routes to the band-box or right-drag branch and
/// edge auto-scroll early-returns — which is why pushing a band box into a
/// screen border does not scroll the map in the original.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TacticalMouseState {
    /// A tactical mouse button holds the capture.
    pub(crate) captured: bool,
    /// Left button physically down.
    pub(crate) left_held: bool,
    /// Right button physically down.
    pub(crate) right_held: bool,
    /// Right-press anchor in render-target pixels.
    pub(crate) right_anchor: (f32, f32),
    /// The right drag has passed the system drag threshold. Once set, the
    /// release no longer runs the cancel/deselect ladder.
    pub(crate) right_threshold_crossed: bool,
    /// The right drag owns the camera (the band box did not win the race).
    right_pan_engaged: bool,
}

impl TacticalMouseState {
    /// Whether a press edge may arm anything and take the capture.
    ///
    /// `Tactical_Mouse_Message_Handler` 0x006930A0 keeps ONE capture byte at
    /// `+0x555A` for both buttons, and cases 0x201 (test at 0x00693194) and
    /// 0x204 (test at 0x006932D5) each require it to be clear before they arm.
    /// So whichever button presses first owns the gesture: a left press during a
    /// right-drag pan arms no band box, and a right press during a band drag
    /// records no pan anchor. Both are dropped rather than layered.
    pub(crate) fn press_may_arm(&self) -> bool {
        !self.captured
    }

    /// Left press edge. Returns whether the caller should arm the band drag.
    ///
    /// The physical-button byte is recorded either way, because
    /// `ScrollClass__UpdateMouseScrolling` 0x00692F30 gates on the capture byte
    /// first (0x00692F85) and then picks its branch from the LIVE button state
    /// via `Input__IsLogicalMouseButtonActive` — button 1 before button 2. That
    /// is what lets a left press interrupt a right-drag pan mid-gesture even
    /// when the press itself armed nothing.
    pub(crate) fn begin_left_press(&mut self) -> bool {
        self.left_held = true;
        if !self.press_may_arm() {
            return false;
        }
        self.captured = true;
        true
    }

    /// Left release edge. Returns whether the caller should run the release body.
    ///
    /// Case 0x202 (test at 0x00693232) gates on the same shared byte and does
    /// not test which button set it: with the byte clear it exits having done
    /// nothing, and with it set it runs `BandBox_LeftUp` 0x004AB9B0 and drops
    /// the capture even when the right button was the one holding it.
    pub(crate) fn end_left_press(&mut self) -> bool {
        self.left_held = false;
        if !self.captured {
            return false;
        }
        self.captured = false;
        // The threshold latch deliberately SURVIVES. Native's case 0x202 writes
        // only the capture byte, at 0x00693290; the latch `this+0x5558`
        // (0x00884D40) is SET at 0x006934B6 inside `Tactical_RightDrag_Pan` and
        // cleared at 0x0069333F (case 0x204) and 0x006933B4 (case 0x205). Note
        // `get_xrefs_to` alone sees only those two absolute stores — every other
        // access is `this`-relative and produces no data xref, the standing
        // `param_1` pointer-arithmetic pitfall.
        //
        // Clearing it here would flip the right-release cancel ladder, which
        // case 0x205 gates at 0x00693397 and calls at 0x006933C6 only when the
        // latch is CLEAR: a pan that crossed the threshold, then took a left
        // click and a second still-held left press, would drop the player's
        // whole selection on the right release where gamemd is silent.
        //
        // The engaged latch is cleared purely to keep the two pan latches from
        // disagreeing about a gesture that has ended. It is a write native lacks
        // — `this+0x554C` is only ever set, never cleared after a gesture — but
        // it is unobservable: `right_drag_owns_frame` requires the capture, and
        // the capture can only come back through `begin_right_drag`, which
        // resets both latches anyway.
        self.right_pan_engaged = false;
        true
    }

    /// Right press inside the play area: record the anchor and take the capture.
    /// The press itself has no game effect in gamemd.
    pub(crate) fn begin_right_drag(&mut self, cursor: (f32, f32)) {
        self.right_anchor = cursor;
        self.right_threshold_crossed = false;
        self.right_pan_engaged = false;
        self.captured = true;
    }

    /// Drop the capture and both right-drag latches.
    pub(crate) fn release(&mut self) {
        self.captured = false;
        self.right_threshold_crossed = false;
        self.right_pan_engaged = false;
    }

    /// True while the right button owns the per-frame mouse block. The left
    /// button is tested first in the original, so a band box in progress keeps
    /// the pan from running.
    fn right_drag_owns_frame(&self) -> bool {
        self.captured && self.right_held && !self.left_held
    }
}

/// One right-drag pan step, in world pixels, for `ScrollMethod = 0` (the stock
/// value — the two cursor-warping methods have no in-game UI and ship disabled).
///
/// The displacement is measured from the **anchor**, not from the previous
/// frame, so the gesture behaves like a joystick: the further the cursor sits
/// from the press point, the faster the map slides, every frame the button is
/// held. Both axes truncate toward zero, matching the original's float-to-long.
fn right_drag_pan_step(
    anchor: (f32, f32),
    cursor: (f32, f32),
    view_w: f32,
    view_h: f32,
    scroll_rate: u32,
) -> (f32, f32) {
    // The native handler works on integer mouse coordinates.
    let mut dx = (cursor.0 - anchor.0).trunc() as i32;
    let mut dy = (cursor.1 - anchor.1).trunc() as i32;

    // An anchor pinned against a border still scrolls outward when the cursor
    // cannot travel any further. The native test is asymmetric — the X arm
    // compares against `width - 1` and the Y arm against the raw height.
    if dx == 0 {
        if anchor.0 <= 0.0 {
            dx = -1;
        } else if anchor.0 >= view_w - 1.0 {
            dx = 1;
        }
    }
    if dy == 0 {
        if anchor.1 <= 0.0 {
            dy = -1;
        } else if anchor.1 >= view_h {
            dy = 1;
        }
    }

    // Anchor within ten pixels of a border, dragging further that way: force at
    // least five pixels of displacement, then multiply by four.
    if anchor.0 < RIGHT_DRAG_EDGE_BAND_PX && dx < 0 {
        dx = dx.min(-RIGHT_DRAG_EDGE_MIN_PX) * RIGHT_DRAG_EDGE_BOOST;
    } else if anchor.0 > view_w - RIGHT_DRAG_EDGE_BAND_PX && dx > 0 {
        dx = dx.max(RIGHT_DRAG_EDGE_MIN_PX) * RIGHT_DRAG_EDGE_BOOST;
    }
    if anchor.1 < RIGHT_DRAG_EDGE_BAND_PX && dy < 0 {
        dy = dy.min(-RIGHT_DRAG_EDGE_MIN_PX) * RIGHT_DRAG_EDGE_BOOST;
    } else if anchor.1 > view_h - RIGHT_DRAG_EDGE_BAND_PX && dy > 0 {
        dy = dy.max(RIGHT_DRAG_EDGE_MIN_PX) * RIGHT_DRAG_EDGE_BOOST;
    }

    let divisor = (scroll_rate + RIGHT_DRAG_RATE_BIAS) as f32;
    ((dx as f32 / divisor).trunc(), (dy as f32 / divisor).trunc())
}

/// Drive the right-drag map pan for this frame.
fn update_right_drag_pan(state: &mut AppState) {
    if !state.match_state.input.tactical_mouse.right_drag_owns_frame() {
        return;
    }
    let anchor = state.match_state.input.tactical_mouse.right_anchor;
    let cursor = (state.match_state.input.cursor_x, state.match_state.input.cursor_y);
    let (drag_metric_x, drag_metric_y) = system_drag_metrics_px();
    if !state.match_state.input.tactical_mouse.right_threshold_crossed
        && right_drag_threshold_crossed(
            cursor.0 - anchor.0,
            cursor.1 - anchor.1,
            drag_metric_x,
            drag_metric_y,
        )
    {
        state.match_state.input.tactical_mouse.right_threshold_crossed = true;
    }
    if !state.match_state.input.tactical_mouse.right_threshold_crossed {
        return;
    }
    if !state.match_state.input.tactical_mouse.right_pan_engaged {
        // A live band box wins the race: the original cancels the drag instead
        // of engaging the pan, and only engages on a later frame.
        if state.match_state.input.selection_state.is_band_box_active() {
            state.match_state.input.selection_state.cancel_drag();
            return;
        }
        state.match_state.input.tactical_mouse.right_pan_engaged = true;
    }

    let (dx, dy) = right_drag_pan_step(
        anchor,
        cursor,
        state.render_width() as f32,
        state.render_height() as f32,
        state.match_state.match_presentation.in_game_options.scroll_rate,
    );
    // The pan distance is in window pixels. Stock YR has no world zoom, so the
    // divide is VERA-internal and exact at zoom 1.0.
    state.match_state.input.camera_x += dx / state.match_state.input.zoom_level;
    state.match_state.input.camera_y += dy / state.match_state.input.zoom_level;
}

/// Arrow-key scroll distance for this frame, in world pixels.
///
/// Shift is tested before Ctrl in the original, so Shift wins when both are
/// held. The Ctrl distance is derived from the map's longer side; with no map
/// loaded there is nothing to jump across and the distance is zero.
fn keyboard_scroll_distance(state: &AppState) -> f32 {
    if crate::app::input::dispatch::is_shift_held(state) {
        (KEY_SCROLL_DISTANCE * KEY_SCROLL_SHIFT_MULTIPLIER).trunc()
    } else if crate::app::input::dispatch::is_ctrl_held(state) {
        let cells = state
            .match_state.sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .map_or(0u32, |sim| u32::from(sim.fog.width.max(sim.fog.height)));
        (cells << KEY_SCROLL_CTRL_CELL_SHIFT) as f32
    } else {
        KEY_SCROLL_DISTANCE
    }
}

/// Update camera position based on keyboard and mouse edge scrolling.
pub(crate) fn update_camera(state: &mut AppState) {
    let sw: f32 = state.render_width() as f32;
    let sh: f32 = state.render_height() as f32;

    let key_distance = keyboard_scroll_distance(state);
    if state
        .match_state.input.keys_held
        .contains(&winit::keyboard::KeyCode::ArrowLeft)
    {
        state.match_state.input.camera_x -= key_distance / state.match_state.input.zoom_level;
    }
    if state
        .match_state.input.keys_held
        .contains(&winit::keyboard::KeyCode::ArrowRight)
    {
        state.match_state.input.camera_x += key_distance / state.match_state.input.zoom_level;
    }
    if state.match_state.input.keys_held.contains(&winit::keyboard::KeyCode::ArrowUp) {
        state.match_state.input.camera_y -= key_distance / state.match_state.input.zoom_level;
    }
    if state
        .match_state.input.keys_held
        .contains(&winit::keyboard::KeyCode::ArrowDown)
    {
        state.match_state.input.camera_y += key_distance / state.match_state.input.zoom_level;
    }

    update_right_drag_pan(state);
    // Each native scroll request reaches the clamp before the next source
    // probes. Keep the current point valid before edge/coast preflight.
    clamp_camera_to_playable_area(state, sw, sh);

    // gamemd's edge scroll early-returns while any tactical mouse button holds
    // the capture, so the map is frozen for the whole of a band-box or
    // right-drag gesture. The minimap-drag inhibit is VERA-internal: gamemd's
    // minimap re-centres only on press, while this flag owns the gesture here.
    if !state.match_state.input.tactical_mouse.captured && !state.match_state.input.minimap_dragging {
        let now = state.match_state.input.edge_scroll.radar_timer();
        let scroll_rate = state.match_state.match_presentation.in_game_options.scroll_rate;
        let active_direction =
            edge_scroll_intent((state.match_state.input.cursor_x, state.match_state.input.cursor_y), sw as i32, sh as i32);
        let requested_direction =
            active_direction.or_else(|| state.match_state.input.edge_scroll.coasting_direction());
        let movement_allowed = requested_direction
            .is_none_or(|direction| camera_scroll_direction_allowed(state, direction, sw, sh));
        let scroll_multiplier = state
            .rules()
            .map_or(DEFAULT_SCROLL_MULTIPLIER, |rules| {
                rules.general.scroll_multiplier
            });
        let (dx, dy) = edge_scroll_step_with_context(
            &mut state.match_state.input.edge_scroll,
            (state.match_state.input.cursor_x, state.match_state.input.cursor_y),
            sw as i32,
            sh as i32,
            scroll_rate,
            scroll_multiplier,
            state.match_state.match_presentation.in_game_gadgets.right_held,
            movement_allowed,
            now,
        );
        // The speed table is in window pixels. Stock YR has no world zoom, so
        // the divide is VERA-internal: it keeps the on-screen scroll rate
        // constant across VERA's zoom range and is exact at zoom 1.0.
        state.match_state.input.camera_x += dx / state.match_state.input.zoom_level;
        state.match_state.input.camera_y += dy / state.match_state.input.zoom_level;
    }

    clamp_camera_to_playable_area(state, sw, sh);

    // Smoothly animate zoom_level toward zoom_target each frame.
    animate_zoom(state);
}

/// Smoothing factor for zoom animation. Each frame, zoom_level moves this
/// fraction of the remaining distance toward zoom_target. 0.35 ≈ snappy ease-out.
const ZOOM_EASE: f32 = 0.35;
/// Snap threshold — when zoom_level is this close to zoom_target, jump to it.
const ZOOM_SNAP: f32 = 0.002;

/// Set zoom target, anchored on the cursor position.
///
/// Records the world point under the cursor so `animate_zoom` can keep it
/// pinned at that screen position during the smooth ease.
///
/// **Currently unbound.** Stock YR has no world zoom at all, and the wheel — the
/// only input that used to reach this — is the sidebar strip scroll in gamemd.
/// The zoom machinery is kept intact (`animate_zoom` still runs, and every
/// camera path still divides by `zoom_level`) so a future non-wheel binding or a
/// spectator/debug view can drive it, but nothing calls this today.
#[allow(dead_code)]
pub(crate) fn apply_zoom(state: &mut AppState, delta_lines: f32) {
    let old_target = state.match_state.input.zoom_target;
    let factor = ZOOM_STEP.powf(delta_lines);
    let new_target = (old_target * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    if (new_target - old_target).abs() < 1e-6 {
        return;
    }

    // Record the world point under the cursor — animate_zoom keeps it stable.
    let z = state.match_state.input.zoom_level;
    state.match_state.input.zoom_anchor_world = [
        state.match_state.input.cursor_x / z + state.match_state.input.camera_x,
        state.match_state.input.cursor_y / z + state.match_state.input.camera_y,
    ];
    state.match_state.input.zoom_anchor_screen = [state.match_state.input.cursor_x, state.match_state.input.cursor_y];
    state.match_state.input.zoom_target = new_target;
}

/// Animate zoom_level toward zoom_target each frame, adjusting the camera so
/// the anchor world point stays at the anchor screen position.
pub(crate) fn animate_zoom(state: &mut AppState) {
    let diff = state.match_state.input.zoom_target - state.match_state.input.zoom_level;
    if diff.abs() < ZOOM_SNAP {
        if (state.match_state.input.zoom_level - state.match_state.input.zoom_target).abs() > 1e-7 {
            state.match_state.input.zoom_level = state.match_state.input.zoom_target;
            let sw = state.render_width() as f32;
            let sh = state.render_height() as f32;
            clamp_camera_to_playable_area(state, sw, sh);
        }
        return;
    }

    state.match_state.input.zoom_level += diff * ZOOM_EASE;

    // Adjust camera so the anchor world point stays at the anchor screen position:
    //   anchor_world_x = anchor_screen_x / zoom + camera_x
    //   camera_x = anchor_world_x - anchor_screen_x / zoom
    state.match_state.input.camera_x = state.match_state.input.zoom_anchor_world[0] - state.match_state.input.zoom_anchor_screen[0] / state.match_state.input.zoom_level;
    state.match_state.input.camera_y = state.match_state.input.zoom_anchor_world[1] - state.match_state.input.zoom_anchor_screen[1] / state.match_state.input.zoom_level;

    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    clamp_camera_to_playable_area(state, sw, sh);
}

/// Camera top-left, in world pixels, that puts `world` at the centre of the
/// **tactical** viewport rather than the centre of the window.
///
/// The tactical extents are window pixels, so their half-extents are divided by
/// `zoom` to come back to world pixels.
pub(crate) fn tactical_camera_top_left(
    world: (f32, f32),
    tactical_w: f32,
    tactical_h: f32,
    zoom: f32,
) -> (f32, f32) {
    (
        world.0 - tactical_w / (2.0 * zoom),
        world.1 - tactical_h / (2.0 * zoom),
    )
}

/// The tactical viewport, in render-target pixels, as `(x, y, w, h)`.
///
/// The native engine keeps exactly one tactical rect and clips **every**
/// battlefield draw to it: an object intersects its own screen rect with the
/// tactical rect and hands the intersection to its blitter as a clip rect, and
/// the depth and shroud buffers are allocated at the rect's dimensions, not the
/// screen's. The sidebar lives on its own surface that is blitted alongside, so
/// no battlefield pixel can reach the sidebar column even when a sprite,
/// bracket or health bar overhangs the boundary.
///
/// VERA composites the whole frame into one render target, so that guarantee has
/// to come from a scissor rect instead — otherwise the battlefield is drawn
/// across the full window and only *covered* by whatever sidebar art happens to
/// be opaque that frame.
///
pub(crate) fn tactical_viewport_px(state: &AppState) -> (u32, u32, u32, u32) {
    let (width, height) = tactical_viewport_size_px(state.render_width(), state.render_height());
    (0, 0, width, height)
}

/// Active YR tactical dimensions. The right sidebar and bottom strip are fixed
/// native-pixel reservations at every supported resolution. Degenerate test
/// targets retain a one-pixel scissor rather than producing an invalid extent.
pub(crate) fn tactical_viewport_size_px(render_w: u32, render_h: u32) -> (u32, u32) {
    (
        render_w.saturating_sub(TACTICAL_SIDEBAR_WIDTH_PX).max(1),
        render_h
            .saturating_sub(TACTICAL_BOTTOM_STRIP_HEIGHT_PX)
            .max(1),
    )
}

/// World-pixel position of a cell's **projected cell coordinate** — the point a
/// "go here" camera move should land on, and where an entity standing on that
/// cell is drawn.
///
/// gamemd's camera-set builds the cell-centre lepton coordinate `(cell << 8) +
/// 0x80` on both axes and projects it. VERA's reproduction of that same point is
/// `util::lepton::lepton_to_screen` at the cell centre, which is the centre of
/// the cell's diamond: `iso_to_screen + (TILE_WIDTH/2, TILE_HEIGHT/2)`. Both
/// half-tiles are needed because `iso_to_screen` anchors the north-west corner
/// of the tile's diamond bounding box, and the projected cell coordinate sits
/// half a tile east and half a tile south of it.
///
/// Keeping this identical to the entity projection is the whole contract:
/// centring on a cell has to put a unit standing there at the tactical centre,
/// and `centring_lands_a_unit_on_that_cell_at_the_tactical_centre` fails the
/// moment the two drift apart.
pub(crate) fn cell_centre_world_point(rx: u16, ry: u16, z: u8) -> (f32, f32) {
    let (nw_x, nw_y) = terrain::iso_to_screen(rx, ry, z);
    (
        nw_x + terrain::TILE_WIDTH / 2.0,
        nw_y + terrain::TILE_HEIGHT / 2.0,
    )
}

/// The view centre `CenterViewCommandClass` picks for a selection, in absolute
/// leptons. `None` when nothing is selected.
///
/// `CenterViewCommandClass::Execute` 0x00536E00 bails on an empty selection and
/// otherwise hands off to 0x004AE290, which is this reduction:
///
/// 1. Sum every selected object's coordinate (`+0x9C`, `+0xA0`, `+0xA4`) and
///    divide by the count — the plain centroid.
/// 2. **For three or more objects only**, find the object farthest from that
///    centroid and average again with it removed: `(sum - outlier) / (n - 1)`.
///    Dropping the straggler is what stops one scout on the far side of the map
///    from dragging the view off the group the player actually looked at.
///
/// Two details of the original are load-bearing and reproduced exactly.
/// `CoordStruct__Distance3D` 0x0041C380 truncates `sqrt(dx²+dy²+dz²)` to an
/// integer before the comparison, and the comparison is a strict `<` against the
/// running best, so objects whose distances truncate to the same lepton tie and
/// the earliest in selection order wins. And the running best starts at zero
/// with the outlier initialised to the origin, so a 3+ selection whose objects
/// all sit exactly on the centroid never replaces it and divides the full sum by
/// `n - 1` — a native quirk that lands the view off the group. It needs every
/// object on the identical lepton coordinate, which stacking rules make
/// unreachable in ordinary play; it is reproduced rather than silently repaired.
pub(crate) fn selection_view_centre_leptons(coords: &[(i32, i32, i32)]) -> Option<(i32, i32, i32)> {
    let count = i32::try_from(coords.len()).ok().filter(|n| *n > 0)?;
    // Native accumulates in a 32-bit register and wraps; a debug build would
    // otherwise panic where gamemd keeps going. Unreachable below five figures
    // of selected units, but the project's scale target is 20 000.
    let sum = coords.iter().fold((0i32, 0i32, 0i32), |acc, c| {
        (
            acc.0.wrapping_add(c.0),
            acc.1.wrapping_add(c.1),
            acc.2.wrapping_add(c.2),
        )
    });
    // C integer division truncates toward zero, and so does Rust's.
    let mean = (sum.0 / count, sum.1 / count, sum.2 / count);
    if count <= 2 {
        return Some(mean);
    }

    let mut best_distance = 0i32;
    let mut outlier = (0i32, 0i32, 0i32);
    for &coord in coords {
        let distance = crate::util::native_x87::distance_3d_leptons(
            [mean.0, mean.1, mean.2],
            [coord.0, coord.1, coord.2],
        );
        if best_distance < distance {
            best_distance = distance;
            outlier = coord;
        }
    }

    let divisor = count - 1;
    Some((
        (sum.0 - outlier.0) / divisor,
        (sum.1 - outlier.1) / divisor,
        (sum.2 - outlier.2) / divisor,
    ))
}

/// Put an absolute lepton coordinate at the centre of the tactical viewport.
///
/// `TacticalClass__SetViewToCoordInstant` takes the whole coordinate, so the
/// centre keeps both its sub-cell offset and its **Z**: the Z runs through
/// `Tactical__AdjustForZ`, and dropping it would miss by 15 px per elevation
/// level and by 216 px on an aircraft at the stock `FlightLevel` of 1500.
pub(crate) fn center_camera_on_lepton_point(
    state: &mut AppState,
    lepton_x: i32,
    lepton_y: i32,
    lepton_z: i32,
) {
    let world = crate::util::lepton::absolute_leptons_to_screen(lepton_x, lepton_y, lepton_z);
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let (tactical_w, tactical_h) =
        tactical_viewport_size_px(state.render_width(), state.render_height());
    let (cx, cy) = tactical_camera_top_left(
        world,
        tactical_w as f32,
        tactical_h as f32,
        state.match_state.input.zoom_level,
    );
    state.match_state.input.camera_x = cx;
    state.match_state.input.camera_y = cy;
    clamp_camera_to_playable_area(state, sw, sh);
}

/// `CenterView` (Numpad 5 in the stock archive): snap the tactical view onto the
/// current selection. Nothing selected means nothing happens.
pub(crate) fn center_view_on_selection(state: &mut AppState) {
    let ordered = crate::app::input::dispatch::selected_stable_ids_in_order(state);
    let coords: Vec<(i32, i32, i32)> = {
        let Some(sim) = state.match_state.sim_runtime.as_ref().map(|rt| &rt.simulation) else {
            return;
        };
        ordered
            .iter()
            .filter_map(|id| sim.entities().get(*id))
            .map(|entity| entity_lepton_coord(entity))
            .collect()
    };
    let Some((cx, cy, cz)) = selection_view_centre_leptons(&coords) else {
        return;
    };
    center_camera_on_lepton_point(state, cx, cy, cz);
}

/// `Follow` (F): latch the camera onto the selection, or let it go.
///
/// `FollowCommandClass::Execute` 0x00537A10 is a toggle with a bias toward
/// releasing: with nothing selected, or while already following anything at all,
/// it clears the `DisplayClass` pair; only an idle camera plus a live selection
/// latches, and it latches the FIRST object in the selection array rather than
/// anything nearer the cursor.
pub(crate) fn toggle_follow_target(state: &mut AppState) {
    let already_following = state.match_state.input.follow_target.is_some();
    let first_selected = crate::app::input::dispatch::selected_stable_ids_in_order(state)
        .first()
        .copied();
    state.match_state.input.follow_target = match first_selected {
        Some(id) if !already_following => Some(id),
        _ => None,
    };
}

/// Drive the follow camera for this tick.
///
/// Native hangs this off the very end of `LogicClass__PerTickUpdate`
/// 0x0055B6B8, after every object, factory and house has updated: read the
/// follow object and snap the view straight onto its coordinate. There is no
/// easing and no dead zone, so the followed unit stays pinned to the centre and
/// the player cannot scroll away while the latch is held.
///
/// The pair is cleared from two lifecycle points — `ObjectClass__Destroy`
/// 0x005F5306 and `ObjectClass__Deselect` 0x005F4513 — so the camera is handed
/// back when the followed object dies or leaves the selection.
pub(crate) fn update_follow_camera(state: &mut AppState) {
    let Some(id) = state.match_state.input.follow_target else {
        return;
    };
    let coord = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(|sim| sim.entities().get(id))
        .filter(|entity| entity.lifecycle.object_alive && entity.selected)
        .map(entity_lepton_coord);
    let Some((x, y, z)) = coord else {
        state.match_state.input.follow_target = None;
        return;
    };
    center_camera_on_lepton_point(state, x, y, z);
}

/// An entity's absolute lepton coordinate, the VERA equivalent of the native
/// object's `+0x9C`/`+0xA0`/`+0xA4` triple.
/// The Z is the composed world height, not the terrain level: a Kirov at the
/// stock `FlightLevel` sits about six cells up in coordinate space, and the
/// straggler search is a 3D distance, so feeding it ground height would drop a
/// different object and move the centre on X and Y as well as Y-by-Z.
fn entity_lepton_coord(entity: &crate::sim::game_entity::GameEntity) -> (i32, i32, i32) {
    let cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
    let x = i32::from(entity.position.rx) * cell + entity.position.sub_x.to_num::<i32>();
    let y = i32::from(entity.position.ry) * cell + entity.position.sub_y.to_num::<i32>();
    (x, y, crate::render::locomotor_visual::world_z_leptons(entity))
}

pub(crate) fn center_camera_on_cell(state: &mut AppState, rx: u16, ry: u16) {
    let z = state.height_map().get(&(rx, ry)).copied().unwrap_or(0);
    let world = cell_centre_world_point(rx, ry, z);
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let (tactical_w, tactical_h) =
        tactical_viewport_size_px(state.render_width(), state.render_height());
    let (cx, cy) = tactical_camera_top_left(
        world,
        tactical_w as f32,
        tactical_h as f32,
        state.match_state.input.zoom_level,
    );
    state.match_state.input.camera_x = cx;
    state.match_state.input.camera_y = cy;
    clamp_camera_to_playable_area(state, sw, sh);
}

pub(crate) fn clamp_camera_to_playable_area(state: &mut AppState, sw: f32, sh: f32) {
    sync_playfield_presentation_bounds(state);
    let (camera_x, camera_y) = clamp_camera_point_for_state(
        state,
        (
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ),
        sw,
        sh,
    );
    state.match_state.input.camera_y = camera_y;
    state.match_state.input.camera_x = camera_x;
}

/// Reconcile the camera's exact mode-zero scroll authority with the current
/// normalized MapClass LocalSize. This is called from every clamp entry and
/// the minimap frame feed, so trigger action 0x28 is visible before any next
/// presentation operation; the `(bounds, revision)` comparison also forces a
/// restored timeline and repeated writer through the same path.
pub(crate) fn sync_playfield_presentation_bounds(
    state: &mut AppState,
) -> (Option<crate::map::playfield::PlayfieldBounds>, u64) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        // Non-match renderer fixtures have no MapClass authority to install.
        // Keep their explicit test bounds; live parity paths always have a sim.
        return (None, 0);
    };
    let authority = runtime.view().playfield_authority();
    if state
        .match_state
        .match_presentation
        .installed_playfield_authority
        != Some(authority)
    {
        if let Some(grid) = state.match_state.match_presentation.terrain_grid.as_mut() {
            grid.install_playfield_local_bounds(authority.0);
        }
        state
            .match_state
            .match_presentation
            .installed_playfield_authority = Some(authority);
    }
    authority
}

fn clamp_camera_point_for_state(
    state: &AppState,
    point: (f32, f32),
    sw: f32,
    sh: f32,
) -> (f32, f32) {
    let Some(grid) = &state.match_state.match_presentation.terrain_grid else {
        return point;
    };
    let (area_x, area_y, area_w, area_h) = match grid.local_bounds {
        Some(b) => (b.pixel_x, b.pixel_y, b.pixel_w, b.pixel_h),
        None if state.match_state.sim_runtime.is_some() => {
            // A live match without normalized MapClass bounds must not silently
            // substitute the allocated terrain rectangle.
            return point;
        }
        None => (
            grid.origin_x,
            grid.origin_y,
            grid.world_width,
            grid.world_height,
        ),
    };
    let (viewport_w, viewport_h) =
        tactical_viewport_size_px(sw.max(0.0) as u32, sh.max(0.0) as u32);
    clamp_camera_point_to_local_bounds(
        point,
        (area_x, area_y, area_w, area_h),
        (viewport_w as f32, viewport_h as f32),
        state.match_state.input.zoom_level,
    )
}

fn clamp_camera_point_to_local_bounds(
    point: (f32, f32),
    area: (f32, f32, f32, f32),
    viewport: (f32, f32),
    zoom: f32,
) -> (f32, f32) {
    let (area_x, area_y, area_w, area_h) = area;
    let visible_w = viewport.0 / zoom;
    let visible_h = viewport.1 / zoom;
    let x_min = area_x - 30.0;
    let x_max = x_min + area_w - visible_w;
    let y_min = area_y;
    let y_max = y_min + area_h - visible_h - 15.0;

    // Native comparison order is Y low/high, then X low/high. Each side is a
    // strict comparison, so an exact boundary remains untouched. Do not invent
    // a centred fallback when a custom map is smaller than the viewport.
    let mut x = point.0;
    let mut y = point.1;
    if y < y_min {
        y = y_min;
    } else if y > y_max {
        y = y_max;
    }
    if x < x_min {
        x = x_min;
    } else if x > x_max {
        x = x_max;
    }
    (x, y)
}

fn camera_scroll_direction_allowed(
    state: &AppState,
    direction: ScrollDir,
    sw: f32,
    sh: f32,
) -> bool {
    if state.match_state.match_presentation.terrain_grid.is_none() {
        return true;
    }
    let (dx, dy) = OCTANT_DELTA[scroll_dir_octant(direction)];
    let candidate = (
        state.match_state.input.camera_x + dx / state.match_state.input.zoom_level,
        state.match_state.input.camera_y + dy / state.match_state.input.zoom_level,
    );
    let clamped = clamp_camera_point_for_state(state, candidate, sw, sh);
    requested_scroll_survives_clamp((state.match_state.input.camera_x, state.match_state.input.camera_y), clamped, direction)
}

fn requested_scroll_survives_clamp(
    current: (f32, f32),
    clamped: (f32, f32),
    direction: ScrollDir,
) -> bool {
    let (dx, dy) = OCTANT_DELTA[scroll_dir_octant(direction)];
    (dx != 0.0 && clamped.0 != current.0) || (dy != 0.0 && clamped.1 != current.1)
}

/// The cursor a right-drag pan shows, or `None` when no pan owns the frame.
///
/// `Some(None)` is the plain pan cursor; `Some(Some(dir))` is the directional
/// variant that says the map cannot scroll any further that way.
///
/// `Tactical_RightDrag_Pan` 0x00693440 ends every engaged frame by probing all
/// four cardinals with a dry-run `Scroll_Map(dir, 1, 0)` and setting one bit per
/// direction that came back blocked — bit index `dir / 2`, so bit 0 = north,
/// 1 = east, 2 = south, 3 = west. It then indexes the 16-entry cursor table at
/// `DAT_0083E790` with that mask and hands the result to the shape setter
/// (`vtable +0x48`). Read out of the binary, that table is:
///
/// ```text
///  mask 0 -> 0x3D    1 (N) -> 0x3E    3 (N|E) -> 0x3F    2 (E) -> 0x40
///  6 (E|S) -> 0x41   4 (S) -> 0x42   12 (S|W) -> 0x43    8 (W) -> 0x44
///  9 (W|N) -> 0x45   every other mask -> 0x3D
/// ```
///
/// So 0x3D is the unconstrained pan shape and 0x3E..0x45 are the eight compass
/// directions in N, NE, E, SE, S, SW, W, NW order. Contradictory masks — north
/// AND south blocked, for instance — fall back to the plain shape rather than
/// picking a diagonal.
pub(crate) fn right_drag_pan_cursor_state(state: &AppState) -> Option<Option<ScrollDir>> {
    // The gate is the capture, not the latch. Native writes this cursor from
    // INSIDE `Tactical_RightDrag_Pan`, and `ScrollClass__UpdateMouseScrolling`
    // 0x00692F30 only calls that with the capture byte +0x555A set, button 1
    // inactive and button 2 active — which is `right_drag_owns_frame`. Gating on
    // the engaged latch alone would let a left click taken mid-pan strand it set
    // (the left release clears the capture, and the right release then skips its
    // own cleanup), leaving the pan cursor suppressing every other cursor for
    // the rest of the match.
    if !state.match_state.input.tactical_mouse.right_drag_owns_frame()
        || !state.match_state.input.tactical_mouse.right_pan_engaged
    {
        return None;
    }
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let mut mask: u8 = 0;
    for (bit, dir) in [ScrollDir::N, ScrollDir::E, ScrollDir::S, ScrollDir::W]
        .into_iter()
        .enumerate()
    {
        if !camera_scroll_direction_allowed(state, dir, sw, sh) {
            mask |= 1 << bit;
        }
    }
    Some(blocked_mask_to_scroll_dir(mask))
}

/// The compass direction native's 16-entry pan-cursor table assigns to a
/// blocked-direction mask, or `None` for the unconstrained pan shape.
fn blocked_mask_to_scroll_dir(mask: u8) -> Option<ScrollDir> {
    Some(match mask {
        1 => ScrollDir::N,
        3 => ScrollDir::NE,
        2 => ScrollDir::E,
        6 => ScrollDir::SE,
        4 => ScrollDir::S,
        12 => ScrollDir::SW,
        8 => ScrollDir::W,
        9 => ScrollDir::NW,
        _ => return None,
    })
}

/// Active edge intent plus whether the tactical clamp removes all requested
/// components. Cursor feedback and motion both use this exact probe.
pub(crate) fn edge_scroll_cursor_state(state: &AppState) -> Option<(ScrollDir, bool)> {
    let sw = state.render_width() as f32;
    let sh = state.render_height() as f32;
    let direction = edge_scroll_intent((state.match_state.input.cursor_x, state.match_state.input.cursor_y), sw as i32, sh as i32)?;
    Some((
        direction,
        !camera_scroll_direction_allowed(state, direction, sw, sh),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW_W: f32 = 1024.0;
    const WINDOW_H: f32 = 768.0;
    const SIDEBAR_W: f32 = 168.0;
    const TACTICAL_W: f32 = WINDOW_W - SIDEBAR_W;
    const TACTICAL_H: f32 = WINDOW_H - TACTICAL_BOTTOM_STRIP_HEIGHT_PX as f32;

    /// Screen position the batch shader gives a world point for a camera.
    fn screen_of(world: (f32, f32), camera: (f32, f32), zoom: f32) -> (f32, f32) {
        ((world.0 - camera.0) * zoom, (world.1 - camera.1) * zoom)
    }

    /// Where the production entity path draws a ground unit standing on a cell.
    /// `render::locomotor_visual::ground_screen_position` forwards straight to
    /// this, so it is the same projection a real unit gets.
    fn entity_world_point(rx: u16, ry: u16, z: u8) -> (f32, f32) {
        use crate::util::lepton::{CELL_CENTER_LEPTON, lepton_to_screen};
        lepton_to_screen(rx, ry, CELL_CENTER_LEPTON, CELL_CENTER_LEPTON, z)
    }

    #[test]
    fn centring_lands_a_unit_on_that_cell_at_the_tactical_centre() {
        // The invariant that matters: put a unit on a cell, centre on that cell,
        // and the unit must sit at the tactical viewport's centre. Anchoring the
        // camera anywhere else -- window centre, or the tile diamond's centre --
        // fails this.
        for (rx, ry, z) in [(10_u16, 10_u16, 0_u8), (0, 0, 0), (37, 12, 3), (63, 1, 2)] {
            let unit = entity_world_point(rx, ry, z);
            let camera = tactical_camera_top_left(
                cell_centre_world_point(rx, ry, z),
                TACTICAL_W,
                TACTICAL_H,
                1.0,
            );
            let (sx, sy) = screen_of(unit, camera, 1.0);
            assert!(
                (sx - TACTICAL_W / 2.0).abs() < 1e-3,
                "cell ({rx},{ry},z={z}): sx {sx}"
            );
            assert!(
                (sy - TACTICAL_H / 2.0).abs() < 1e-3,
                "cell ({rx},{ry},z={z}): sy {sy}"
            );
        }
    }

    /// The centring target IS the tile diamond's centre, because that is where
    /// gamemd projects a cell's coordinate and therefore where it draws a unit
    /// standing on that cell.
    ///
    /// This test previously asserted the opposite — that the target was the tile
    /// row, 15 px above the diamond centre — and it was right about the *code*
    /// and wrong about the *engine*: the entity projection it was checked
    /// against was itself half a tile high, so both sides agreed on the wrong
    /// row. With the entity anchor now on the diamond centre, the target follows
    /// it there. The invariant the pair really encodes is the assertion below
    /// that the two are equal; the literals are the hand-walked fixture.
    #[test]
    fn centring_target_is_the_tile_diamond_centre_where_a_unit_stands() {
        // Hand-walked fixture: cell (10, 10) at ground level.
        //   iso_to_screen        = (30*(10-10) - 30, 15*(10+10) + 15) = (-30, 315)
        //   diamond centre       = iso_to_screen + (30, 15)           = (  0, 330)
        assert_eq!(cell_centre_world_point(10, 10, 0), (0.0, 330.0));
        assert_eq!(
            cell_centre_world_point(10, 10, 0),
            entity_world_point(10, 10, 0),
            "centring on a cell must target exactly where a unit on it is drawn"
        );

        let camera = tactical_camera_top_left(
            cell_centre_world_point(10, 10, 0),
            TACTICAL_W,
            TACTICAL_H,
            1.0,
        );
        // Tactical rect is 1024 - 168 = 856 wide, so its centre is x = 428.
        assert_eq!(camera, (-428.0, -38.0));
        // Guard against the pre-fix behaviour, which anchored on the window
        // centre and put the target 84 px east of the tactical centre.
        assert_ne!(camera.0, 0.0 - WINDOW_W / 2.0);
    }

    #[test]
    fn centring_stays_on_the_tactical_centre_across_zoom() {
        for zoom in [0.25_f32, 0.5, 1.0, 2.0, 4.0] {
            let unit = entity_world_point(37, 12, 3);
            let camera = tactical_camera_top_left(
                cell_centre_world_point(37, 12, 3),
                TACTICAL_W,
                TACTICAL_H,
                zoom,
            );
            let (sx, sy) = screen_of(unit, camera, zoom);
            assert!((sx - TACTICAL_W / 2.0).abs() < 1e-3, "zoom {zoom}: sx {sx}");
            assert!((sy - TACTICAL_H / 2.0).abs() < 1e-3);
        }
    }

    /// Half a tile on **both** axes from the tile corner — the Y half is the one
    /// that used to be missing, which put every camera move 15 px north of the
    /// unit it was supposed to centre on.
    #[test]
    fn cell_centre_shifts_half_a_tile_on_both_axes_from_the_tile_corner() {
        for (rx, ry, z) in [(0_u16, 0_u16, 0_u8), (5, 9, 2), (63, 1, 0)] {
            let (nw_x, nw_y) = terrain::iso_to_screen(rx, ry, z);
            let (cx, cy) = cell_centre_world_point(rx, ry, z);
            assert_eq!(cx - nw_x, terrain::TILE_WIDTH / 2.0);
            assert_eq!(cy - nw_y, terrain::TILE_HEIGHT / 2.0);
        }
    }

    // -- edge scroll ---------------------------------------------------------

    const VIEW_W: i32 = 1024;
    const VIEW_H: i32 = 768;
    /// Stock `[Options] ScrollRate` default (0 fastest .. 6 slowest).
    const DEFAULT_SCROLL_RATE: u32 = 3;

    fn state() -> EdgeScrollState {
        EdgeScrollState::default()
    }

    #[test]
    fn nine_zone_direction_covers_all_eight_octants() {
        // 0.16 * 1024 = 163.84, 0.21 * 768 = 161.28.
        let left = 0;
        let right = VIEW_W - 1;
        let top = 0;
        let bottom = VIEW_H - 1;
        let mid_x = VIEW_W / 2;
        let mid_y = VIEW_H / 2;

        assert_eq!(edge_scroll_octant(mid_x, top, VIEW_W, VIEW_H), 0, "N");
        assert_eq!(edge_scroll_octant(right, top, VIEW_W, VIEW_H), 1, "NE");
        assert_eq!(edge_scroll_octant(right, mid_y, VIEW_W, VIEW_H), 2, "E");
        assert_eq!(edge_scroll_octant(right, bottom, VIEW_W, VIEW_H), 3, "SE");
        assert_eq!(edge_scroll_octant(mid_x, bottom, VIEW_W, VIEW_H), 4, "S");
        assert_eq!(edge_scroll_octant(left, bottom, VIEW_W, VIEW_H), 5, "SW");
        assert_eq!(edge_scroll_octant(left, mid_y, VIEW_W, VIEW_H), 6, "W");
        assert_eq!(edge_scroll_octant(left, top, VIEW_W, VIEW_H), 7, "NW");
    }

    #[test]
    fn touching_the_left_edge_high_up_scrolls_diagonally_not_straight_west() {
        // y = 100 is inside the top 21 % band, so the reference point is the
        // top-left corner and the scroll runs NW. Four independent axis tests
        // would report plain west here.
        assert_eq!(edge_scroll_octant(0, 100, VIEW_W, VIEW_H), 7);
        // y = 300 is in the middle band, so the same x gives plain west.
        assert_eq!(edge_scroll_octant(0, 300, VIEW_W, VIEW_H), 6);
    }

    #[test]
    fn item82_edge_band_is_one_pixel_and_east_is_the_outer_window_edge() {
        let mut st = state();
        // Ten pixels in from the left does nothing — the pre-fix 10 px band did.
        assert_eq!(
            edge_scroll_step(
                &mut st,
                (10.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                0
            ),
            (0.0, 0.0)
        );
        // Just left of the sidebar does nothing either.
        let sidebar_x = (VIEW_W as f32) - 168.0 - 1.0;
        assert_eq!(
            edge_scroll_step(
                &mut st,
                (sidebar_x, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                0
            ),
            (0.0, 0.0)
        );
        // The far right column of the window does scroll east.
        let (dx, dy) = edge_scroll_step(
            &mut st,
            ((VIEW_W - 1) as f32, 300.0),
            VIEW_W,
            VIEW_H,
            DEFAULT_SCROLL_RATE,
            0,
        );
        assert!(dx > 0.0, "dx {dx}");
        assert_eq!(dy, 0.0);
    }

    #[test]
    fn coast_ramps_geometrically_and_scroll_rate_caps_the_peak() {
        let mut st = state();
        // One step per radar tick; the cursor sits on the left edge throughout.
        let mut seen = Vec::new();
        for tick in 0..12_u32 {
            let (dx, _) = edge_scroll_step(
                &mut st,
                (0.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                tick,
            );
            seen.push(-dx);
        }
        // Retail curve for ScrollRate 3: 1, 2, 4, 8, 13 then held at the cap.
        assert_eq!(&seen[..5], &[1.0, 2.0, 4.0, 8.0, 13.0]);
        assert!(seen[5..].iter().all(|&v| v == 13.0), "{seen:?}");
    }

    #[test]
    fn scroll_rate_changes_the_peak_speed() {
        // ScrollRate is no longer inert: 0 is fastest, 6 slowest.
        let peaks: Vec<f32> = (0..=6_u32)
            .map(|rate| {
                let mut st = state();
                let mut last = 0.0;
                for tick in 0..20_u32 {
                    let (dx, _) =
                        edge_scroll_step(&mut st, (0.0, 300.0), VIEW_W, VIEW_H, rate, tick);
                    last = -dx;
                }
                last
            })
            .collect();
        assert_eq!(peaks, vec![26.0, 22.0, 17.0, 13.0, 8.0, 4.0, 2.0]);
    }

    #[test]
    fn leaving_the_edge_coasts_down_instead_of_stopping_dead() {
        let mut st = state();
        for tick in 0..8_u32 {
            edge_scroll_step(
                &mut st,
                (0.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                tick,
            );
        }
        // Cursor moves back into the middle of the screen.
        let mut coast = Vec::new();
        for tick in 8..16_u32 {
            let (dx, _) = edge_scroll_step(
                &mut st,
                (500.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                tick,
            );
            coast.push(-dx);
        }
        // Still moving west, decaying one table step per tick. gamemd stops
        // dead once the counter reaches zero — there is no trailing 1 px creep.
        assert_eq!(coast, vec![13.0, 8.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn coast_only_steps_once_per_radar_tick() {
        let mut st = state();
        // Five calls inside the same 16 ms tick must all use the same speed
        // after the first (zero-delay) step.
        let first = edge_scroll_step(
            &mut st,
            (0.0, 300.0),
            VIEW_W,
            VIEW_H,
            DEFAULT_SCROLL_RATE,
            7,
        );
        assert_eq!(first, (-1.0, 0.0));
        for _ in 0..4 {
            assert_eq!(
                edge_scroll_step(
                    &mut st,
                    (0.0, 300.0),
                    VIEW_W,
                    VIEW_H,
                    DEFAULT_SCROLL_RATE,
                    7
                ),
                (-2.0, 0.0)
            );
        }
        assert_eq!(
            edge_scroll_step(
                &mut st,
                (0.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                8
            ),
            (-2.0, 0.0)
        );
        assert_eq!(
            edge_scroll_step(
                &mut st,
                (0.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                9
            ),
            (-4.0, 0.0)
        );
    }

    #[test]
    fn corner_scroll_moves_the_full_distance_on_both_axes() {
        let mut st = state();
        let (dx, dy) =
            edge_scroll_step(&mut st, (0.0, 0.0), VIEW_W, VIEW_H, DEFAULT_SCROLL_RATE, 0);
        assert_eq!((dx, dy), (-1.0, -1.0));
    }

    #[test]
    fn item82_blocked_active_edge_caps_coast_without_changing_direction_or_timer() {
        let mut st = state();
        st.coast_level = 5;
        st.octant = 3;
        st.last_coast_change = Some(2);
        let before = (st.octant, st.last_coast_change);
        assert_eq!(
            edge_scroll_step_with_context(
                &mut st,
                (0.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                DEFAULT_SCROLL_MULTIPLIER,
                false,
                false,
                7,
            ),
            (0.0, 0.0),
        );
        assert_eq!(st.coast_level, 4, "ScrollRate cap still applies");
        assert_eq!((st.octant, st.last_coast_change), before);
    }

    #[test]
    fn item82_blocked_coast_still_decays() {
        let mut st = state();
        for tick in 0..5 {
            edge_scroll_step_with_context(
                &mut st,
                (0.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                DEFAULT_SCROLL_MULTIPLIER,
                false,
                true,
                tick,
            );
        }
        let before = st.coast_level;
        assert!(before > 0);
        assert_eq!(
            edge_scroll_step_with_context(
                &mut st,
                (500.0, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                DEFAULT_SCROLL_MULTIPLIER,
                false,
                false,
                5,
            ),
            (0.0, 0.0),
        );
        // The ScrollRate cap first pulls the overshot 5 back to 4, then the
        // ordinary off-edge timer decay takes it to 3.
        assert_eq!((before, st.coast_level), (5, 3));
    }

    #[test]
    fn item82_diagonal_probe_slides_when_one_component_survives() {
        assert!(requested_scroll_survives_clamp(
            (70.0, 300.0),
            (70.0, 299.0),
            ScrollDir::NW,
        ));
        assert!(!requested_scroll_survives_clamp(
            (70.0, 200.0),
            (70.0, 200.0),
            ScrollDir::NW,
        ));
    }

    #[test]
    fn item82_uncaptured_rmb_caps_edge_speed_at_thirteen() {
        let mut st = state();
        st.coast_level = 7;
        let (dx, dy) = edge_scroll_step_with_context(
            &mut st,
            ((VIEW_W - 1) as f32, 300.0),
            VIEW_W,
            VIEW_H,
            0,
            DEFAULT_SCROLL_MULTIPLIER,
            true,
            true,
            0,
        );
        assert_eq!((dx, dy), (13.0, 0.0));
    }

    #[test]
    fn item82_edge_distance_uses_loaded_scroll_multiplier() {
        let mut st = state();
        assert_eq!(
            edge_scroll_step_with_context(
                &mut st,
                ((VIEW_W - 1) as f32, 300.0),
                VIEW_W,
                VIEW_H,
                DEFAULT_SCROLL_RATE,
                0.125,
                false,
                true,
                0,
            ),
            (2.0, 0.0),
        );
    }

    /// Fixed view dimensions returned by the active right-sidebar path.
    #[test]
    fn item82_tactical_view_is_fixed_168_by_32_inset() {
        assert_eq!(tactical_viewport_size_px(800, 600), (632, 568));
        assert_eq!(tactical_viewport_size_px(640, 480), (472, 448));
        assert_eq!(tactical_viewport_size_px(1920, 1080), (1752, 1048));
    }

    #[test]
    fn item82_local_clamp_uses_native_minus_30_and_minus_15_bounds() {
        let area = (100.0, 200.0, 1_000.0, 800.0);
        let viewport = (632.0, 568.0);
        assert_eq!(
            clamp_camera_point_to_local_bounds((-999.0, -999.0), area, viewport, 1.0),
            (70.0, 200.0),
        );
        assert_eq!(
            clamp_camera_point_to_local_bounds((9_999.0, 9_999.0), area, viewport, 1.0),
            (438.0, 417.0),
        );
        assert_eq!(
            clamp_camera_point_to_local_bounds((70.0, 417.0), area, viewport, 1.0),
            (70.0, 417.0),
            "equality is not clamped",
        );
    }

    /// A degenerate layout must not scissor the battlefield out of existence —
    /// a zero-width scissor drops every tactical draw call silently.
    #[test]
    fn tactical_viewport_width_never_collapses_or_overruns_the_target() {
        assert_eq!(tactical_viewport_size_px(168, 32), (1, 1));
        assert_eq!(tactical_viewport_size_px(100, 20), (1, 1));
    }

    // -- camera bookmarks ----------------------------------------------------

    /// All four slots start on the scenario's opening view, so a recall before
    /// any set is a "go home" rather than a jump to the map corner, and setting
    /// one slot leaves the other three alone.
    #[test]
    fn bookmarks_seed_all_four_and_set_touches_one() {
        let mut marks = ViewBookmarks::default();
        marks.seed_all(37, 12);
        for slot in 0..VIEW_BOOKMARK_SLOTS {
            assert_eq!(marks.get(slot), Some((37, 12)), "slot {slot}");
        }
        marks.set(0, 10, 10);
        assert_eq!(marks.get(0), Some((10, 10)));
        for slot in 1..VIEW_BOOKMARK_SLOTS {
            assert_eq!(marks.get(slot), Some((37, 12)), "slot {slot}");
        }
        // Four slots, no more.
        assert_eq!(marks.get(VIEW_BOOKMARK_SLOTS), None);
    }

    /// Recalling a bookmark lands the stored cell at the centre of the tactical
    /// viewport — the same anchoring `center_camera_on_cell` performs, which is
    /// what the native recall does after projecting the cell centre.
    #[test]
    fn recalling_a_bookmark_centres_that_cell_in_the_tactical_viewport() {
        let mut marks = ViewBookmarks::default();
        marks.seed_all(37, 12);
        marks.set(1, 10, 10);
        let (rx, ry) = marks.get(1).expect("slot 1");
        let camera = tactical_camera_top_left(
            cell_centre_world_point(rx, ry, 0),
            TACTICAL_W,
            TACTICAL_H,
            1.0,
        );
        let (sx, sy) = screen_of(entity_world_point(rx, ry, 0), camera, 1.0);
        assert!((sx - TACTICAL_W / 2.0).abs() < 1e-3, "sx {sx}");
        assert!((sy - TACTICAL_H / 2.0).abs() < 1e-3, "sy {sy}");
    }

    // -- keyboard scroll -----------------------------------------------------

    /// The Shift boost truncates: 21 * 2.5 is 52.5 and the native float-to-long
    /// chops it to 52, not 53.
    #[test]
    fn shift_boosted_keyboard_scroll_truncates_to_52() {
        assert_eq!(KEY_SCROLL_DISTANCE, 21.0);
        assert_eq!(
            (KEY_SCROLL_DISTANCE * KEY_SCROLL_SHIFT_MULTIPLIER).trunc(),
            52.0
        );
    }

    /// The Ctrl distance is the map's longer side in cells shifted by 8, which
    /// overshoots the widest stock map by orders of magnitude — the clamp is
    /// what actually stops the view, so this is a map-edge jump.
    #[test]
    fn ctrl_keyboard_scroll_overshoots_the_whole_map() {
        for cells in [64u32, 128, 256] {
            let jump = (cells << KEY_SCROLL_CTRL_CELL_SHIFT) as f32;
            // World width of a square map that many cells on a side.
            let world_span = cells as f32 * terrain::TILE_WIDTH;
            assert!(jump > world_span, "{cells} cells: {jump} vs {world_span}");
        }
    }

    // -- right-drag pan ------------------------------------------------------

    const PAN_VIEW_W: f32 = 1024.0;
    const PAN_VIEW_H: f32 = 768.0;
    /// Stock `[Options] ScrollRate` default (0 fastest .. 6 slowest).
    const PAN_SCROLL_RATE: u32 = 3;

    /// The native threshold is strict and independent for each axis. A custom
    /// 4x6 system metric therefore requires more than 8px X or more than 12px Y.
    #[test]
    fn right_drag_threshold_is_strict_and_axis_independent() {
        assert!(!right_drag_threshold_crossed(8.0, 12.0, 4, 6));
        assert!(right_drag_threshold_crossed(8.01, 0.0, 4, 6));
        assert!(right_drag_threshold_crossed(0.0, -12.01, 4, 6));
        assert!(!right_drag_threshold_crossed(7.0, 11.0, 4, 6));
    }

    /// The pan is measured from the anchor, not from the previous frame, so
    /// holding the cursor still keeps the map sliding at a constant rate.
    #[test]
    fn pan_speed_is_the_anchor_displacement_over_scroll_rate_plus_one() {
        let anchor = (400.0, 300.0);
        assert_eq!(
            right_drag_pan_step(
                anchor,
                (500.0, 300.0),
                PAN_VIEW_W,
                PAN_VIEW_H,
                PAN_SCROLL_RATE
            ),
            (25.0, 0.0)
        );
        assert_eq!(
            right_drag_pan_step(
                anchor,
                (300.0, 380.0),
                PAN_VIEW_W,
                PAN_VIEW_H,
                PAN_SCROLL_RATE
            ),
            (-25.0, 20.0)
        );
        // Slower ScrollRate, same gesture, smaller step.
        assert_eq!(
            right_drag_pan_step(anchor, (500.0, 300.0), PAN_VIEW_W, PAN_VIEW_H, 6),
            (14.0, 0.0)
        );
    }

    /// Both axes truncate toward zero, so a drag shorter than the divisor moves
    /// the camera not at all rather than by a rounded pixel.
    #[test]
    fn pan_truncates_toward_zero_on_both_signs() {
        let anchor = (400.0, 300.0);
        assert_eq!(
            right_drag_pan_step(
                anchor,
                (403.0, 297.0),
                PAN_VIEW_W,
                PAN_VIEW_H,
                PAN_SCROLL_RATE
            ),
            (0.0, 0.0)
        );
        assert_eq!(
            right_drag_pan_step(
                anchor,
                (407.0, 293.0),
                PAN_VIEW_W,
                PAN_VIEW_H,
                PAN_SCROLL_RATE
            ),
            (1.0, -1.0)
        );
    }

    /// An anchor pressed against a border boosts: the displacement is forced to
    /// at least five pixels outward and then multiplied by four, which is how a
    /// right drag started at the screen edge still scrolls.
    #[test]
    fn pan_boosts_when_the_anchor_sits_on_a_border() {
        // Anchor 4 px from the left edge, cursor 1 px further left: the raw
        // displacement is -1, the boost substitutes -5 and quadruples it.
        let (dx, _) = right_drag_pan_step(
            (4.0, 300.0),
            (3.0, 300.0),
            PAN_VIEW_W,
            PAN_VIEW_H,
            PAN_SCROLL_RATE,
        );
        assert_eq!(dx, -5.0);
        // Away from the border the same gesture does nothing.
        let (plain, _) = right_drag_pan_step(
            (400.0, 300.0),
            (399.0, 300.0),
            PAN_VIEW_W,
            PAN_VIEW_H,
            PAN_SCROLL_RATE,
        );
        assert_eq!(plain, 0.0);
    }

    /// Capture routing: the left button is tested first, so a band box in
    /// progress keeps the right drag from stealing the camera.
    #[test]
    fn left_button_wins_the_per_frame_mouse_block() {
        let mut mouse = TacticalMouseState::default();
        mouse.right_held = true;
        mouse.begin_right_drag((100.0, 100.0));
        assert!(mouse.right_drag_owns_frame());
        mouse.left_held = true;
        assert!(!mouse.right_drag_owns_frame());
        mouse.left_held = false;
        mouse.release();
        assert!(!mouse.captured);
        assert!(!mouse.right_drag_owns_frame());
    }

    /// One and two objects take the plain centroid — the outlier pass is gated
    /// on `2 < count`, so a pair never drops half of itself.
    #[test]
    fn one_or_two_objects_centre_on_the_plain_mean() {
        assert_eq!(
            selection_view_centre_leptons(&[(1000, 2000, 0)]),
            Some((1000, 2000, 0))
        );
        assert_eq!(
            selection_view_centre_leptons(&[(1000, 2000, 0), (2000, 4000, 0)]),
            Some((1500, 3000, 0))
        );
        assert_eq!(selection_view_centre_leptons(&[]), None);
    }

    /// Three or more: the farthest object is dropped, so a straggler cannot drag
    /// the view off the group.
    #[test]
    fn three_objects_drop_the_straggler_before_averaging() {
        // Two together at x=1000 and 2000, one far away at x=30000.
        let coords = [(1000, 0, 0), (2000, 0, 0), (30_000, 0, 0)];
        // Plain mean would be 11000 — out in empty map between the group and the
        // straggler. Dropping the straggler gives (1000+2000)/2 = 1500.
        assert_eq!(
            selection_view_centre_leptons(&coords),
            Some((1500, 0, 0)),
            "the view must land on the pair, not between the pair and the scout"
        );
    }

    /// `CoordStruct__Distance3D` truncates to whole leptons and the comparison
    /// is a strict `<`, so equal truncated distances keep the FIRST object in
    /// selection order as the recorded straggler — later equals never displace
    /// it.
    #[test]
    fn equal_truncated_distances_keep_the_earliest_object_as_the_straggler() {
        // Symmetric about x = 200: both outer objects sit 100 from the mean.
        let coords = [(100, 0, 0), (200, 0, 0), (300, 0, 0)];
        // First-wins on the tie records (100,0,0), so the survivors average to
        // (600 - 100) / 2.
        assert_eq!(selection_view_centre_leptons(&coords), Some((250, 0, 0)));
    }

    /// The straggler search runs gamemd's approximate square root, not an exact
    /// one. On a pure axis offset of `d` the exact root is `d` itself, so any
    /// exact kernel round-trips every input; the native table does not.
    #[test]
    fn the_straggler_search_runs_the_native_approximate_root() {
        use crate::util::native_x87::distance_3d_leptons;
        // Pinned golden: the first axis offset where the table's answer is not
        // the exact root. `isqrt` or `f64::sqrt` both return 129 here.
        assert_eq!(distance_3d_leptons([0, 0, 0], [129, 0, 0]), 128);

        // And the straggler search really runs that kernel. This selection
        // centres on 129: the FIRST object is 128 out and the LAST is 129 out.
        // The table truncates both to 128, so they tie and first-wins keeps the
        // first as the straggler; an exact root separates them and would drop
        // the last instead, moving the centre from 65 to 193.
        let coords = [(257, 0, 0), (130, 0, 0), (0, 0, 0)];
        assert_eq!(
            distance_3d_leptons([129, 0, 0], [257, 0, 0]),
            distance_3d_leptons([129, 0, 0], [0, 0, 0]),
            "the two candidates must tie under the native table"
        );
        assert_ne!(
            i64::from(128).pow(2).isqrt(),
            i64::from(129).pow(2).isqrt(),
            "an exact root would have separated them"
        );
        assert_eq!(selection_view_centre_leptons(&coords), Some((65, 0, 0)));
    }

    /// Distance is 3D: elevation decides the straggler when the ground plane
    /// alone would tie.
    #[test]
    fn elevation_participates_in_the_straggler_search() {
        let flat = [(0, 0, 0), (100, 0, 0), (200, 0, 0)];
        let raised = [(0, 0, 0), (100, 0, 5000), (200, 0, 0)];
        assert_ne!(
            selection_view_centre_leptons(&flat),
            selection_view_centre_leptons(&raised)
        );
    }

    /// The native quirk, reproduced rather than repaired: with 3+ objects all on
    /// the same coordinate no distance ever beats the initial zero, so the
    /// origin stays the recorded outlier and the full sum is divided by n-1.
    #[test]
    fn a_fully_stacked_selection_reproduces_the_native_off_centre_divide() {
        let coords = [(600, 900, 0), (600, 900, 0), (600, 900, 0)];
        assert_eq!(selection_view_centre_leptons(&coords), Some((900, 1350, 0)));
    }

    /// A left click taken mid-pan ends the pan cursor but must NOT rewrite the
    /// threshold latch, because the right release reads that latch to decide
    /// whether to run its cancel ladder.
    #[test]
    fn a_left_release_mid_pan_ends_the_pan_but_keeps_the_threshold_latch() {
        let mut mouse = TacticalMouseState::default();
        mouse.right_held = true;
        mouse.begin_right_drag((100.0, 100.0));
        mouse.right_threshold_crossed = true;
        mouse.right_pan_engaged = true;
        assert!(mouse.right_drag_owns_frame());

        // Left press is swallowed by the shared capture byte but still recorded.
        assert!(!mouse.begin_left_press());
        assert!(!mouse.right_drag_owns_frame(), "left freezes the pan");

        // Left release takes the capture, so the pan can no longer own a frame.
        assert!(mouse.end_left_press());
        assert!(!mouse.captured);
        assert!(!mouse.right_drag_owns_frame());
        assert!(
            mouse.right_threshold_crossed,
            "the threshold latch survives; the right-release cancel ladder reads it"
        );

        // A second left press re-takes the capture. The right release that
        // follows must still see a crossed threshold and stay silent — clearing
        // the latch above would have made it drop the whole selection.
        assert!(mouse.begin_left_press());
        assert!(mouse.captured);
        assert!(mouse.right_threshold_crossed);
    }

    /// The pan cursor's 16-entry table, read out of `DAT_0083E790`. Only eight
    /// masks name a direction; every other mask — including every contradictory
    /// one, and the all-clear mask 0 — falls back to the plain pan shape.
    #[test]
    fn the_pan_cursor_table_names_eight_directions_and_falls_back_otherwise() {
        // The eight table entries 0x3E..0x45, in the order the table lists them.
        for (mask, expected) in [
            (1u8, ScrollDir::N),
            (3, ScrollDir::NE),
            (2, ScrollDir::E),
            (6, ScrollDir::SE),
            (4, ScrollDir::S),
            (12, ScrollDir::SW),
            (8, ScrollDir::W),
            (9, ScrollDir::NW),
        ] {
            assert_eq!(
                blocked_mask_to_scroll_dir(mask),
                Some(expected),
                "mask {mask} must select {expected:?}"
            );
        }
        // Nothing blocked, and the contradictory pairs north|south and east|west.
        for mask in [0u8, 5, 10] {
            assert_eq!(blocked_mask_to_scroll_dir(mask), None, "mask {mask}");
        }
        // Every mask the table maps back to the plain shape.
        for mask in [7u8, 11, 13, 14, 15] {
            assert_eq!(blocked_mask_to_scroll_dir(mask), None, "mask {mask}");
        }
    }

    /// The four press/release orderings of a two-button chord, against the one
    /// shared capture byte. Both press cases refuse to arm while it is set, and
    /// the left release runs its body whenever it is set, whichever button set
    /// it — so the release still fires after a swallowed press.
    #[test]
    fn one_capture_byte_arbitrates_both_buttons_across_all_four_orderings() {
        // Baseline: an uncontested left click arms and then releases.
        let mut mouse = TacticalMouseState::default();
        assert!(mouse.begin_left_press());
        assert!(mouse.captured);
        assert!(mouse.end_left_press());
        assert!(!mouse.captured);

        // R-down, L-down, L-up, R-up. The left press is swallowed, but its
        // release still runs the body and takes the capture away from the pan.
        let mut mouse = TacticalMouseState::default();
        mouse.right_held = true;
        mouse.begin_right_drag((100.0, 100.0));
        assert!(!mouse.begin_left_press());
        assert!(mouse.left_held, "the live button byte is recorded anyway");
        assert!(!mouse.right_drag_owns_frame(), "left freezes the pan");
        assert_eq!(mouse.right_anchor, (100.0, 100.0), "anchor is untouched");
        assert!(mouse.end_left_press());
        assert!(!mouse.captured);

        // R-down, L-down, R-up, L-up. The right release already cleared the
        // byte, so the trailing left release does nothing at all.
        let mut mouse = TacticalMouseState::default();
        mouse.right_held = true;
        mouse.begin_right_drag((100.0, 100.0));
        assert!(!mouse.begin_left_press());
        mouse.right_held = false;
        mouse.release();
        assert!(!mouse.end_left_press());

        // L-down, R-down, L-up. The right press finds the byte set, so it is
        // swallowed and records no anchor; the left release still owns the
        // ending and hands the byte back.
        let mut mouse = TacticalMouseState::default();
        assert!(mouse.begin_left_press());
        mouse.right_held = true;
        if mouse.press_may_arm() {
            mouse.begin_right_drag((250.0, 250.0));
        }
        assert_eq!(
            mouse.right_anchor,
            (0.0, 0.0),
            "the swallowed right press left the anchor alone"
        );
        assert!(mouse.end_left_press());
        assert!(mouse.press_may_arm(), "the byte is free again");
    }
}
