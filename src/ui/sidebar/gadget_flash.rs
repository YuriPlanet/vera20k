//! Sidebar gadget flash primitive.
//!
//! Mirrors the SBGadgetClass flash sub-struct (+0x34 state / +0x38 period /
//! +0x3c countdown / +0x1e disabled) and the three driver functions
//! Start_Flash / Stop_Flash / Flash_AI. One instance per pressable gadget
//! that may flash. Native entries: Start69DFC0, Stop69DFF0, FlashAI69E010;
//! tools/sidebar_oracle/tab_flash.json executes their caller and timer states.
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
    /// StripAI 0x006A8E52..0x006A8E9B (also Sidebar 0x006A76F0): align
    /// tab pulses to the next ten-frame boundary. At an exact boundary the
    /// initial delay is ten, not zero. LEA wraps before the signed division.
    /// Original executed controls: tools/sidebar_oracle/tab_flash.json.
    pub(crate) fn start_tab_flash(&mut self, frame: i32) -> bool {
        let delay = 10 - frame % 10;
        let next_boundary = frame.wrapping_add(delay) / 10;
        self.start(10, delay as u32, u8::from(next_boundary % 2 == 0))
    }

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

/// `ShapeButtonClass::Draw @ 0x0069DEB0` frame choice (vtable slot +0x6C).
///
/// Disabled (+0x1E) draws frame 2. A button that reflects its toggle state
/// (+0x40) draws its on-state (+0x2D) as frame 0/1, or 3/4 while its flash
/// bit (+0x34) is lit; holding the mouse on it changes nothing.
/// `SidebarClass::Init @ 0x006A5310` sets +0x40 for the four tabs, Repair and
/// Sell (`0x006A539B`, `0x006A5405`, `0x006A546A`). Every other button draws
/// its pressed state (+0x2C) as frame 0/1: the scroll pair (Init leaves +0x40
/// clear) and Diplomacy/Options (`RadarClass @ 0x00653010`).
/// Original-executed controls: `tools/sidebar_oracle/command_bar/mapping-frame-native.json`.
pub fn shape_button_frame(
    disabled: bool,
    reflects_state: bool,
    pressed: bool,
    on: bool,
    flashing: bool,
) -> u8 {
    if disabled {
        2
    } else if !reflects_state {
        u8::from(pressed)
    } else if flashing {
        3 + u8::from(on)
    } else {
        u8::from(on)
    }
}

/// Persistent flash + mode state for the in-game sidebar gadgets.
///
/// Lives on `AppState` (one instance per app session). Ticked once per sim
/// tick by `sidebar_gadgets::update_sidebar_gadget_state`. Read by the
/// per-frame `SidebarView` builder to populate gadget `frame_index` fields.
#[derive(Debug, Clone, Default)]
pub struct SidebarGadgetState {
    /// One flash per tab gadget, indexed by `SidebarTab::tab_index()`.
    /// Building/Defense flash for linked completed buildings; Defense also
    /// flashes for ready Supers. Infantry has no native ready-flash poll.
    pub tab_flashes: [GadgetFlash; 4],

    /// Strip availability projected at sidebar refresh (native gadget+1E).
    /// Native only flips these on the AddCameo/Recalculate transitions, so the
    /// bit is event-driven state rather than a per-refresh projection.
    pub tab_disabled: [bool; 4],

    /// Mirrors Display +0x11B0 (set by 4AC8C0). Toggled by clicking the Repair button.
    /// Mutually exclusive with `sell_mode_on`; pending placement refuses changes.
    pub repair_mode_on: bool,

    /// Mirrors Display +0x11B1 (set by 4AC660). Toggled by clicking the Sell button.
    /// Mutually exclusive with `repair_mode_on`; pending placement refuses changes.
    pub sell_mode_on: bool,

    /// Per-button disabled bits. v1: always false.
    pub repair_disabled: bool,
    pub sell_disabled: bool,

    /// Last sim tick the orchestrator processed; used to advance per
    /// sim-tick delta (catch-up safe).
    pub last_sim_tick: u64,

    /// Transient pressed bits (+0x2C) of the buttons whose frame shows them:
    /// true while the gadget driver holds the button pressed-inside; popped on
    /// drag-off, restored on drag-back. Published by `app::input::gadget_input`
    /// after every gadget tick. Tabs, Repair and Sell draw their on-state
    /// instead (see [`shape_button_frame`]), so they publish none.
    pub scroll_down_pressed: bool,
    pub scroll_up_pressed: bool,
    pub top_pressed: [bool; 2],
    /// Native constructor6CFE20 starts the bottom bar open.
    pub command_bar_closed: bool,
    pub command_pressed: [bool; 11],
    pub command_thumb_pressed: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SelectionMode {
    Repair,
    Sell,
}

impl SidebarGadgetState {
    /// Display SetRepairMode4AC8C0 / SetSellMode4AC660 (-1). A pending
    /// placement TYPE refuses the entire transition. Both toggles clear the
    /// competing mode, and entry needs House+2F0 (owned-building tracking).
    /// Return whether UnselectAll48DC90 must run. The independent Super
    /// pointer and PlanMgrAC4CF4 are not these Display mode bytes.
    /// Source: original gamemd.exe instructions.
    pub(crate) fn toggle_selection_mode(
        &mut self,
        mode: SelectionMode,
        placement_armed: bool,
        owned_buildings: i32,
    ) -> bool {
        if placement_armed {
            return false;
        }
        let enabled = owned_buildings > 0
            && !match mode {
                SelectionMode::Repair => self.repair_mode_on,
                SelectionMode::Sell => self.sell_mode_on,
            };
        self.repair_mode_on = enabled && matches!(mode, SelectionMode::Repair);
        self.sell_mode_on = enabled && matches!(mode, SelectionMode::Sell);
        enabled
    }

    /// Advance existing pulses through missed ticks, then perform the current
    /// frame's poll followed by FlashAI, as Sidebar Action6A77CD/6A7A8C does.
    /// A newly ready item starts at this frame, rather than consuming ticks
    /// that passed before it was observed. Repeated renders do not tick again.
    pub(crate) fn update_tab_flashes(&mut self, tick: u64, frame: i32, ready: [bool; 4]) {
        let delta = tick.saturating_sub(self.last_sim_tick);
        for _ in 1..delta {
            for flash in &mut self.tab_flashes {
                flash.tick();
            }
        }
        for (flash, ready) in self.tab_flashes.iter_mut().zip(ready) {
            if ready {
                flash.start_tab_flash(frame);
            } else {
                flash.stop();
            }
        }
        if delta != 0 {
            for flash in &mut self.tab_flashes {
                flash.tick();
            }
        }
        self.last_sim_tick = tick;
    }

    /// Display4AC8C0(0)/4AC660(0): a pending building placement refuses mode
    /// changes. Display's Power/Waypoint/Beacon bytes have no active owner;
    /// the independent PlanMgr and bare Super pointer are not canceled.
    pub(crate) fn disable_selection_modes(&mut self, placement_armed: bool) {
        if !placement_armed {
            self.repair_mode_on = false;
            self.sell_mode_on = false;
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Native6A6472 enables after Add and6A6820/Recalculate disables after the
    /// last entry is removed; the retained bit is what the frame table reads.
    pub(crate) fn set_tab_disabled(&mut self, tab_index: usize, disabled: bool) {
        self.tab_disabled[tab_index] = disabled;
        self.tab_flashes[tab_index].disabled = disabled;
    }

    /// Frame index for a tab gadget. Caller passes whether this tab is the
    /// currently-active tab, the on-state Sidebar Action `0x006A7780` latches
    /// with ToggleClass TurnOn/TurnOff when it switches tabs.
    pub(crate) fn tab_frame(&self, tab_index: usize, is_active_tab: bool) -> u8 {
        shape_button_frame(
            self.tab_disabled[tab_index],
            true,
            false,
            is_active_tab,
            self.tab_flashes[tab_index].state != 0,
        )
    }

    /// Frame index for the Repair button. Only the tabs ever flash
    /// (StartFlashing `0x0069DFC0` callers `0x006A76F0` and StripAI).
    pub fn repair_frame(&self) -> u8 {
        shape_button_frame(
            self.repair_disabled,
            true,
            false,
            self.repair_mode_on,
            false,
        )
    }

    /// Frame index for the Sell button. Same logic as `repair_frame`.
    pub fn sell_frame(&self) -> u8 {
        shape_button_frame(self.sell_disabled, true, false, self.sell_mode_on, false)
    }

    /// Frame index for the strip scroll-down (+page) button; `disabled` is the
    /// native `6A6610` capacity state.
    pub fn scroll_down_frame(&self, disabled: bool) -> u8 {
        shape_button_frame(disabled, false, self.scroll_down_pressed, false, false)
    }

    /// Frame index for the strip scroll-up (−page) button.
    pub fn scroll_up_frame(&self, disabled: bool) -> u8 {
        shape_button_frame(disabled, false, self.scroll_up_pressed, false, false)
    }

    /// Frame index for Diplomacy (0) or Options (1).
    pub fn top_button_frame(&self, index: usize) -> u8 {
        shape_button_frame(false, false, self.top_pressed[index], false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_modes_refuse_placement_and_require_owned_buildings_before_clearing_selection() {
        let mut state = SidebarGadgetState::new();
        state.repair_mode_on = true;
        assert!(!state.toggle_selection_mode(SelectionMode::Sell, true, 1));
        assert!(state.repair_mode_on && !state.sell_mode_on);
        assert!(!state.toggle_selection_mode(SelectionMode::Repair, true, 1));
        assert!(
            state.repair_mode_on,
            "pending type also refuses switching a mode off"
        );
        assert!(!state.toggle_selection_mode(SelectionMode::Sell, false, 0));
        assert!(!state.repair_mode_on && !state.sell_mode_on);
        assert!(state.toggle_selection_mode(SelectionMode::Repair, false, 1));
        assert!(state.repair_mode_on && !state.sell_mode_on);
        assert!(state.toggle_selection_mode(SelectionMode::Sell, false, 1));
        assert!(!state.repair_mode_on && state.sell_mode_on);
        assert!(!state.toggle_selection_mode(SelectionMode::Sell, false, 1));
        assert!(!state.repair_mode_on && !state.sell_mode_on);
    }

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
    fn aggregator_frame_accessors() {
        let mut s = SidebarGadgetState::new();
        // Idle tab → 0; active tab → 1.
        assert_eq!(s.tab_frame(0, false), 0);
        assert_eq!(s.tab_frame(0, true), 1);
        // Flash mid-pulse on tab 3 (Vehicle).
        s.tab_flashes[3].state = 1;
        s.tab_flashes[3].period = 10;
        assert_eq!(s.tab_frame(3, false), 3, "inactive + flash");
        assert_eq!(s.tab_frame(3, true), 4, "active + flash");
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
    fn shape_button_frame_matches_original_draw() {
        let native =
            crate::test_fixture::json("tools/sidebar_oracle/command_bar/mapping-frame-native.json");
        let cases = native["frame_selector_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 32);
        for case in cases {
            let bit = |i: usize| case["flags_1e_2c_2d_34_40"][i] != 0;
            assert_eq!(
                u64::from(shape_button_frame(bit(0), bit(4), bit(1), bit(2), bit(3))),
                case["frame"].as_u64().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn scroll_buttons_show_their_press() {
        let mut s = SidebarGadgetState::new();
        s.scroll_down_pressed = true;
        assert_eq!(s.scroll_down_frame(false), 1);
        assert_eq!(s.scroll_up_frame(false), 0, "the pair is independent");
        assert_eq!(s.scroll_down_frame(true), 2, "disabled wins");
    }
}
#[test]
fn native_tab_flash_phase_and_timer_controls() {
    let rows = crate::test_fixture::json("tools/sidebar_oracle/tab_flash.json");
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let prior = input["prior"].as_array();
        let mut flash = GadgetFlash {
            state: prior.map_or(0, |p| p[0].as_u64().unwrap() as u8),
            period: prior.map_or(0, |p| p[1].as_u64().unwrap() as u32),
            countdown: prior.map_or(0, |p| p[2].as_u64().unwrap() as u32),
            disabled: false,
        };
        // The original poll supplies the admission; this comparison tests
        // the shared phase/timer port, not a second Rust ready predicate.
        if row["stop"] == "0x6a8ea0" {
            flash.start_tab_flash(input["frame"].as_i64().unwrap() as i32);
        } else {
            flash.stop();
        }
        let state =
            |flash: &GadgetFlash| serde_json::json!([flash.state, flash.period, flash.countdown]);
        assert_eq!(state(&flash), row["output"], "{input}");
        let mut tick = 0;
        for checkpoint in row["history"].as_array().unwrap() {
            let until = checkpoint["ticks"].as_u64().unwrap();
            while tick < until {
                flash.tick();
                tick += 1;
            }
            assert_eq!(state(&flash), checkpoint["state"], "{input}, tick {tick}");
        }
    }
}

#[test]
fn tab_flash_catchup_starts_new_ready_state_at_current_frame() {
    let mut caught_up = SidebarGadgetState::default();
    let mut stepped = SidebarGadgetState::default();
    caught_up.update_tab_flashes(0, 0, [false; 4]);
    for tick in 0..=100 {
        stepped.update_tab_flashes(tick, tick as i32, [tick == 100, false, false, false]);
    }
    caught_up.update_tab_flashes(100, 100, [true, false, false, false]);
    assert_eq!(caught_up.tab_flashes[0].countdown, 19);
    for tick in 101..=200 {
        stepped.update_tab_flashes(tick, tick as i32, [true, false, false, false]);
    }
    caught_up.update_tab_flashes(200, 200, [true, false, false, false]);
    let flash = caught_up.tab_flashes[0];
    assert_eq!(flash.state, stepped.tab_flashes[0].state);
    assert_eq!(flash.countdown, stepped.tab_flashes[0].countdown);
    caught_up.update_tab_flashes(200, 200, [true, false, false, false]);
    assert_eq!(caught_up.tab_flashes[0].countdown, flash.countdown);
    caught_up.update_tab_flashes(201, 201, [false; 4]);
    assert!(!caught_up.tab_flashes[0].is_active());
}
