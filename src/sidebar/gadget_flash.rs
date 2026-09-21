//! Sidebar gadget flash primitive.
//!
//! Mirrors the SBGadgetClass flash sub-struct (+0x34 state / +0x38 period /
//! +0x3c countdown / +0x1e disabled) and the three driver functions
//! Start_Flash / Stop_Flash / Flash_AI. One instance per pressable gadget
//! that may flash. See ra2-rust-game-docs/SIDEBAR_TAB_FLASH_SCHEDULER_GHIDRA_REPORT.md
//! for the source semantics.
//!
//! ## Dependency rules
//! - Part of `sidebar/` — pure data + logic, no rendering or sim dependencies.

/// Persistent flash state for one pressable gadget.
///
/// Mirrors three contiguous fields on an SBGadgetClass instance plus the
/// separate +0x1e disabled gate. All four are byte-for-byte semantic
/// counterparts of the binary; the field names use plain-English meanings
/// rather than the binary offsets.
#[derive(Debug, Clone, Copy, Default)]
pub struct GadgetFlash {
    /// "Draw as pressed" bit. 0 = idle visual, 1 = pressed-look visual.
    /// Toggled by `tick` when a flash period elapses.
    pub state: u8,

    /// Toggle interval in ticks AND the "is-flashing" sentinel (non-zero ⇒ active).
    /// Stays constant for the lifetime of an active flash; reset by `stop`.
    pub period: u32,

    /// Ticks remaining until the next toggle. On the first cycle this is
    /// `period + extra_delay`; on every subsequent cycle it resets to `period`.
    pub countdown: u32,

    /// Auto-stop gate. When set, the next `tick` zeros all three flash fields
    /// and reports a state change.
    pub disabled: bool,
}

impl GadgetFlash {
    /// Schedule a flash. No-op (returns `false`) if a flash is already active —
    /// matches the gamemd Start_Flash guard at +0x38 != 0.
    ///
    /// `extra_delay` is added to the FIRST countdown only; the steady-state
    /// toggle interval is `period`.
    pub fn start(&mut self, period: u32, extra_delay: u32, initial_state: u8) -> bool {
        if self.period != 0 {
            return false;
        }
        self.period = period;
        self.countdown = period + extra_delay;
        self.state = initial_state;
        true
    }

    /// Cancel any active flash. No-op (returns `false`) if not currently flashing.
    /// Field-write order matches the binary: state → countdown → period.
    pub fn stop(&mut self) -> bool {
        if self.period == 0 {
            return false;
        }
        self.state = 0;
        self.countdown = 0;
        self.period = 0;
        true
    }

    /// Advance one game-logic tick. Returns `true` when the visible state
    /// changed (caller marks redraw / picks a new frame index).
    pub fn tick(&mut self) -> bool {
        if self.disabled {
            if self.period != 0 {
                self.state = 0;
                self.countdown = 0;
                self.period = 0;
                return true;
            }
            return false;
        }
        if self.countdown == 0 {
            return false;
        }
        self.countdown -= 1;
        if self.countdown == 0 {
            self.state ^= 1;
            self.countdown = self.period;
            return true;
        }
        false
    }

    /// True while a flash is scheduled (period != 0).
    pub fn is_active(&self) -> bool {
        self.period != 0
    }
}

/// Pick a SHP frame index for a 5-frame gadget given its three state bits.
///
/// Mirrors the SBGadgetClass::Draw conditional at gamemd's `0x0069DEB0`.
/// Output indices map to the 5-frame SHP convention used by `tab0N.shp`,
/// `repair.shp`, and `sell.shp`:
///   0 = idle, 1 = mode-active, 2 = disabled, 3 = pressed-idle, 4 = pressed-active.
///
/// Inputs:
/// - `disabled`: the gadget's disabled gate.
/// - `mode_active`: the persistent "this mode is on" / "this tab is selected" bit.
///   For tabs this is the active-tab bit; for Repair/Sell this is the
///   mode-on toggle.
/// - `state`: the transient "drawn as pressed" bit (set by mouse-down OR by
///   the flash AI's tick toggle).
///
/// The function assumes the gadget is pressable (the not-pressable / hover-static
/// branch from the binary is unused for any of our 5-frame gadgets).
pub fn frame_select(disabled: bool, mode_active: bool, state: u8) -> u8 {
    if disabled {
        return 2;
    }
    if state != 0 {
        if mode_active { 4 } else { 3 }
    } else if mode_active {
        1
    } else {
        0
    }
}

/// Pick a SHP frame index for the 3-frame strip-scroll art (`r-up.shp` /
/// `r-dn.shp`). The scroll buttons do NOT use the 5-frame tab convention —
/// their art has exactly 3 frames: 0 = idle, 1 = pressed, 2 = disabled.
/// The view applies the native `6A6610` capacity-disabled state first.
pub fn scroll_frame_select(pressed: bool) -> u8 {
    u8::from(pressed)
}

/// Persistent flash + mode state for the in-game sidebar gadgets.
///
/// Lives on `AppState` (one instance per app session). Ticked once per sim
/// tick by `sidebar_gadgets::update_sidebar_gadget_state`. Read by the
/// per-frame `SidebarView` builder to populate gadget `frame_index` fields.
#[derive(Debug, Clone, Default)]
pub struct SidebarGadgetState {
    /// One flash per tab gadget, indexed by `SidebarTab::tab_index()`.
    /// Building (0) and Infantry (2) are kept in the array for uniformity
    /// but the orchestrator unconditionally `stop()`s them — they never
    /// flash in retail.
    pub tab_flashes: [GadgetFlash; 4],

    /// Strip availability projected at sidebar refresh (native gadget+1E).
    pub tab_disabled: [bool; 4],

    /// Mirrors SidebarClass +0x46c. Toggled by clicking the Repair button.
    /// Mutually exclusive with `sell_mode_on` and `AppState.targeting_mode`.
    pub repair_mode_on: bool,

    /// Mirrors SidebarClass +0x11B1. Toggled by clicking the Sell button.
    /// Mutually exclusive with `repair_mode_on` and `AppState.targeting_mode`.
    pub sell_mode_on: bool,

    /// Per-button disabled bits. v1: always false.
    pub repair_disabled: bool,
    pub sell_disabled: bool,

    /// Last sim tick the orchestrator processed; used to advance per
    /// sim-tick delta (catch-up safe).
    pub last_sim_tick: u64,

    /// Transient pressed-look bits (study G22): true while the gadget driver
    /// holds the button pressed-inside; popped on drag-off, restored on
    /// drag-back. Published by `app::input::gadget_input` after every gadget tick.
    pub tab_pressed: [bool; 4],
    pub repair_pressed: bool,
    pub sell_pressed: bool,
    pub scroll_down_pressed: bool,
    pub scroll_up_pressed: bool,
    pub top_pressed: [bool; 2],
    /// Native constructor6CFE20 starts the bottom bar open.
    pub command_bar_closed: bool,
    pub command_pressed: [bool; 11],
    pub command_thumb_pressed: bool,
}

impl SidebarGadgetState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Frame index for a tab gadget. Caller passes whether this tab is the
    /// currently-active tab (the externally driven latch-ON mirror,
    /// study §2.5 / G22 Kind 2).
    #[cfg(test)]
    pub fn tab_frame(&self, tab_index: usize, is_active_tab: bool) -> u8 {
        if self.tab_disabled[tab_index] {
            return 2;
        }
        self.tab_frame_enabled(tab_index, is_active_tab)
    }

    pub(crate) fn tab_frame_enabled(&self, tab_index: usize, is_active_tab: bool) -> u8 {
        let flash = &self.tab_flashes[tab_index];
        // Pressed-look = flash pulse OR live press-hold (study G22).
        let state = if self.tab_pressed[tab_index] {
            1
        } else {
            flash.state
        };
        frame_select(false, is_active_tab, state)
    }

    /// Frame index for the Repair button. Repair has no flash AI — the state
    /// bit is the live press-hold; "stays pressed" comes from `mode_active`.
    pub fn repair_frame(&self) -> u8 {
        frame_select(
            self.repair_disabled,
            self.repair_mode_on,
            u8::from(self.repair_pressed),
        )
    }

    /// Frame index for the Sell button. Same logic as `repair_frame`.
    pub fn sell_frame(&self) -> u8 {
        frame_select(
            self.sell_disabled,
            self.sell_mode_on,
            u8::from(self.sell_pressed),
        )
    }

    /// Frame index for the strip scroll-down (+page) button. No mode bit, no
    /// flash — pressed-look only, using the 3-frame R-DN convention.
    pub fn scroll_down_frame(&self) -> u8 {
        scroll_frame_select(self.scroll_down_pressed)
    }

    /// Frame index for the strip scroll-up (−page) button (3-frame R-UP
    /// convention).
    pub fn scroll_up_frame(&self) -> u8 {
        scroll_frame_select(self.scroll_up_pressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_from_idle_initialises_all_fields() {
        let mut g = GadgetFlash::default();
        assert!(g.start(10, 7, 1));
        assert_eq!(g.period, 10);
        assert_eq!(g.countdown, 17, "first countdown is period + extra_delay");
        assert_eq!(g.state, 1);
        assert!(g.is_active());
    }

    #[test]
    fn start_while_active_is_noop() {
        let mut g = GadgetFlash::default();
        g.start(10, 7, 1);
        let snapshot = g;
        assert!(!g.start(20, 0, 0), "guard returns false");
        assert_eq!(g.period, snapshot.period);
        assert_eq!(g.countdown, snapshot.countdown);
        assert_eq!(g.state, snapshot.state);
    }

    #[test]
    fn stop_from_active_zeros_in_order() {
        let mut g = GadgetFlash::default();
        g.start(10, 0, 1);
        assert!(g.stop());
        assert_eq!(g.state, 0);
        assert_eq!(g.countdown, 0);
        assert_eq!(g.period, 0);
    }

    #[test]
    fn stop_from_idle_is_noop() {
        let mut g = GadgetFlash::default();
        assert!(!g.stop());
    }

    #[test]
    fn tick_decrements_countdown_until_toggle() {
        let mut g = GadgetFlash::default();
        // First cycle: period=10, extra_delay=5, initial=0 → countdown=15.
        g.start(10, 5, 0);
        for _ in 0..14 {
            assert!(!g.tick(), "no toggle yet");
        }
        assert!(g.tick(), "15th tick toggles");
        assert_eq!(g.state, 1, "state XOR-toggled to 1");
        assert_eq!(
            g.countdown, 10,
            "countdown resets to period, not period+extra"
        );
    }

    #[test]
    fn tick_steady_state_toggles_every_period_ticks() {
        let mut g = GadgetFlash::default();
        g.start(10, 0, 0);
        // First cycle: countdown=10. 10 ticks → toggle.
        for _ in 0..9 {
            assert!(!g.tick());
        }
        assert!(g.tick());
        assert_eq!(g.state, 1);
        // Second cycle: countdown=10. 10 more ticks → toggle back to 0.
        for _ in 0..9 {
            assert!(!g.tick());
        }
        assert!(g.tick());
        assert_eq!(g.state, 0);
    }

    #[test]
    fn tick_when_idle_is_noop() {
        let mut g = GadgetFlash::default();
        assert!(!g.tick());
        assert_eq!(g.countdown, 0);
        assert_eq!(g.state, 0);
    }

    #[test]
    fn tick_when_disabled_auto_stops_active_flash() {
        let mut g = GadgetFlash::default();
        g.start(10, 0, 1);
        g.disabled = true;
        assert!(g.tick(), "auto-stop reports a change");
        assert_eq!(g.state, 0);
        assert_eq!(g.countdown, 0);
        assert_eq!(g.period, 0);
    }

    #[test]
    fn tick_when_disabled_and_idle_is_noop() {
        let mut g = GadgetFlash::default();
        g.disabled = true;
        assert!(!g.tick());
    }

    #[test]
    fn aggregator_frame_accessors() {
        let mut s = SidebarGadgetState::new();
        // Idle tab → 0; active tab → 1.
        assert_eq!(s.tab_frame(0, false), 0);
        assert_eq!(s.tab_frame(0, true), 1);
        // Flash mid-pulse on tab 3 (Vehicle).
        s.tab_flashes[3].state = 1;
        s.tab_flashes[3].period = 10;
        assert_eq!(s.tab_frame(3, false), 3, "inactive + pressed-look");
        assert_eq!(s.tab_frame(3, true), 4, "active + pressed-look");
        // Disabled overrides everything.
        s.tab_disabled[3] = true;
        assert_eq!(s.tab_frame(3, true), 2);
        // Repair/Sell.
        assert_eq!(s.repair_frame(), 0);
        s.repair_mode_on = true;
        assert_eq!(s.repair_frame(), 1);
        s.repair_disabled = true;
        assert_eq!(s.repair_frame(), 2);
        s.sell_mode_on = true;
        s.sell_disabled = false;
        assert_eq!(s.sell_frame(), 1);
    }

    #[test]
    fn frame_select_table() {
        // (disabled, mode_active, state) → expected frame
        let cases: &[(bool, bool, u8, u8)] = &[
            (false, false, 0, 0), // idle
            (false, true, 0, 1),  // mode-active
            (true, false, 0, 2),  // disabled (mode and state ignored)
            (true, true, 0, 2),
            (true, false, 1, 2),
            (true, true, 1, 2),
            (false, false, 1, 3), // pressed-idle
            (false, true, 1, 4),  // pressed-active
        ];
        for &(disabled, mode_active, state, expected) in cases {
            let got = frame_select(disabled, mode_active, state);
            assert_eq!(
                got, expected,
                "frame_select(disabled={disabled}, mode_active={mode_active}, state={state}) expected {expected}, got {got}"
            );
        }
    }

    #[test]
    fn pressed_bits_drive_frames_3_and_4() {
        let mut s = SidebarGadgetState::new();
        s.repair_pressed = true;
        assert_eq!(s.repair_frame(), 3, "pressed-idle");
        s.repair_mode_on = true;
        assert_eq!(s.repair_frame(), 4, "pressed-active");
        s.tab_pressed[1] = true;
        assert_eq!(s.tab_frame(1, false), 3);
        assert_eq!(s.tab_frame(1, true), 4);
    }

    #[test]
    fn scroll_buttons_use_three_frame_convention() {
        // R-UP.SHP / R-DN.SHP have exactly 3 frames: 0 = idle, 1 = pressed,
        // 2 = disabled — NOT the 5-frame tab convention (pressed at 3).
        assert_eq!(scroll_frame_select(false), 0, "idle selects frame 0");
        assert_eq!(scroll_frame_select(true), 1, "pressed selects frame 1");
        let mut s = SidebarGadgetState::new();
        assert_eq!(s.scroll_down_frame(), 0);
        assert_eq!(s.scroll_up_frame(), 0);
        s.scroll_down_pressed = true;
        assert_eq!(s.scroll_down_frame(), 1, "pressed-look is frame 1");
        assert_eq!(s.scroll_up_frame(), 0, "the pair is independent");
        s.scroll_down_pressed = false;
        s.scroll_up_pressed = true;
        assert_eq!(s.scroll_down_frame(), 0);
        assert_eq!(s.scroll_up_frame(), 1);
    }
}
