//! Framework-A gadget substrate core (study §6.1): retained gadget lists with
//! gamemd-native dispatch semantics — retained order = hit priority = draw
//! order, sticky capture, fire-on-release button machine.
//!
//! Pure and deterministic: NO wall-clock reads anywhere in this module tree;
//! every tick input arrives in a `tick::GadgetInput` snapshot built by the app
//! driver. Clause IDs (G1..G25) cite the behavior contract in
//! GADGET_DIALOG_CONTROL_ENGINE_SUBSTRATE_SERVICE_STUDY.md §5.
//!
//! ## Dependency rules
//! - ui/ module: std only — no render/, assets/, audio/, net/.

pub mod button;
pub mod focus;
pub mod list;
pub mod tick;

/// Queued left-mouse-down event (G8 source code).
pub const KEY_LMB_DOWN: u16 = 0x001;
/// Queued right-mouse-down event.
pub const KEY_RMB_DOWN: u16 = 0x002;
/// Queued left-mouse-up event (low byte 1 ⇒ event-coordinate source, G6).
pub const KEY_LMB_UP: u16 = 0x801;
/// Queued right-mouse-up event (low byte 2 ⇒ event-coordinate source, G6).
pub const KEY_RMB_UP: u16 = 0x802;

/// Event-flag bits assembled per tick (G8).
pub const FLAG_LEFT_PRESS: u16 = 0x0001;
pub const FLAG_LEFT_HELD: u16 = 0x0002;
pub const FLAG_LEFT_RELEASE: u16 = 0x0004;
pub const FLAG_LEFT_UP: u16 = 0x0008;
pub const FLAG_RIGHT_PRESS: u16 = 0x0010;
pub const FLAG_RIGHT_HELD: u16 = 0x0020;
pub const FLAG_RIGHT_RELEASE: u16 = 0x0040;
pub const FLAG_RIGHT_UP: u16 = 0x0080;
/// Queued non-mouse event yields exactly this flag (G8); doubles as the
/// keyboard-focus mask bit a focused gadget carries (G18).
pub const FLAG_KEYBOARD: u16 = 0x0100;

/// Press bits — the sticky-capture acquire test (G17).
pub const PRESS_BITS: u16 = FLAG_LEFT_PRESS | FLAG_RIGHT_PRESS; // 0x11
/// Release bits — capture release + the G22 strip test.
pub const RELEASE_BITS: u16 = FLAG_LEFT_RELEASE | FLAG_RIGHT_RELEASE; // 0x44

/// Result protocol (G13): a fired control posts `id | RESULT_BUTTON`.
pub const RESULT_BUTTON: u16 = 0x8000;
/// Extra OR'd marker iff a right-release fired AND the mask includes
/// `FLAG_RIGHT_PRESS` (G13).
pub const RESULT_RIGHT: u16 = 0x4000;

/// Hit-test best-area seed: the fixed 1024×768 constants, NOT live resolution
/// (G14). A gadget with area > 786,432 px² can never win a hit-test.
pub const HIT_SEED_AREA: i32 = 1024 * 768;

/// Ctor rule (G4): a sticky gadget always ORs press+left bits into its mask.
pub const STICKY_CTOR_MASK: u16 = 0x0005;

/// SelectClass cameo construction mask (A2, study cameo lane §3): LEFTPRESS |
/// LEFTUP | RIGHTPRESS. The cameo Action acts on the press bits and discards
/// LEFTUP; there is no release bit, so cameos fire on mouse-DOWN.
pub const CAMEO_FLAGS: u16 = FLAG_LEFT_PRESS | FLAG_LEFT_UP | FLAG_RIGHT_PRESS; // 0x19

/// Full-tactical catcher mask (A3, study tactical lane §3): all mouse bits
/// except RIGHTUP and KEYBOARD = 0x7F.
pub const TACTICAL_REGION_FLAGS: u16 = FLAG_LEFT_PRESS
    | FLAG_LEFT_HELD
    | FLAG_LEFT_RELEASE
    | FLAG_LEFT_UP
    | FLAG_RIGHT_PRESS
    | FLAG_RIGHT_HELD
    | FLAG_RIGHT_RELEASE; // 0x7F

/// Minimap/radar region mask. gamemd's is 0x9F (study minimap lane §1b: left
/// press/held/release/up + right-press + right-up; NOT right-held/right-release).
/// We ADD RIGHT_RELEASE (→ 0xDF) as a Rust-native divergence: with 0x9F a
/// right-press acquires sticky capture that RIGHT_RELEASE can't clear (it's
/// masked out), leaving the minimap holding capture until the next left-release
/// — an input-eating quirk. Including RIGHT_RELEASE lets a minimap right-click
/// acquire-then-release capture cleanly, producing gamemd's observable output
/// (right-press centers the tactical view) without the quirk.
pub const MINIMAP_REGION_FLAGS: u16 = FLAG_LEFT_PRESS
    | FLAG_LEFT_HELD
    | FLAG_LEFT_RELEASE
    | FLAG_LEFT_UP
    | FLAG_RIGHT_PRESS
    | FLAG_RIGHT_RELEASE
    | FLAG_RIGHT_UP; // 0xDF

/// Stable per-list gadget identity. Never reused within a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GadgetHandle(pub u32);

/// Caller-assigned list identity; `FocusState.current_list` compares these for
/// the G5 fresh-list reset. The app owns uniqueness (one in-game list today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListId(pub u32);

/// Integer pixel rect with HALF-OPEN containment (G14): left/top in,
/// right/bottom out — the same convention as `ui::shell::geom::RectPx` and the
/// native unsigned-compare filter; deliberately NOT `sidebar::Rect` (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GadgetRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl GadgetRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Half-open containment via the unsigned-compare trick: a negative delta
    /// wraps to a huge u32 and rejects, so no explicit lower-bound test is
    /// needed (G14/G15). Zero width or height never contains anything.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        (px.wrapping_sub(self.x) as u32) < self.w as u32
            && (py.wrapping_sub(self.y) as u32) < self.h as u32
    }

    /// Signed pixel area (G14 does signed i32 math).
    pub fn area(&self) -> i32 {
        self.w * self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_half_open_edges() {
        let r = GadgetRect::new(10, 20, 5, 4);
        assert!(r.contains(10, 20), "left/top edge IN");
        assert!(r.contains(14, 23), "last interior pixel IN");
        assert!(!r.contains(15, 20), "right edge OUT (half-open)");
        assert!(!r.contains(10, 24), "bottom edge OUT (half-open)");
        assert!(
            !r.contains(9, 20),
            "negative delta rejects via unsigned wrap"
        );
        assert!(!r.contains(10, 19));
    }
}
