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
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
/// Lives on `AppState` (one instance per app session). Ticked on every pass
/// by `sidebar_gadgets::update_sidebar_gadget_state`. Read by the
/// per-frame `SidebarView` builder to populate gadget `frame_index` fields.
#[derive(Debug, Clone, Default, PartialEq)]
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

    /// Mirrors Display +0x11B4, beacon placement mode, which only
    /// SetPlaceBeaconMode 4AC960 sets (`app::input::beacon`). Repair, Sell
    /// and Power changes clear it (4AC916, 4AC6BB, 4AC87B).
    pub place_beacon_mode_on: bool,

    /// Mirrors Display +0x11B2, power mode, which only SetTogglePowerMode
    /// 4AC820 sets: YR never arms it, Ares' TogglePower command does
    /// (`app::input::power_mode`). Repair, Sell and Beacon changes clear it
    /// (4AC908, 4AC6AD, 4AC9C7).
    pub power_mode_on: bool,

    /// Last sim tick the orchestrator processed: offline, the frames since
    /// it are this pass's Sidebar Action visits
    /// (`app::presentation::sidebar_gadgets::sidebar_action_visits`).
    pub last_sim_tick: u64,

    /// Transient pressed bits (+0x2C) of the buttons whose frame shows them:
    /// true while the match's chrome holds the button pressed-inside; popped
    /// on drag-off, restored on drag-back. Published by
    /// `app::input::chrome_input` after every pointer message it takes. Tabs, Repair and Sell draw their on-state
    /// instead (see [`shape_button_frame`]), so they publish none.
    pub scroll_down_pressed: bool,
    pub scroll_up_pressed: bool,
    pub top_pressed: [bool; 2],
    /// Native constructor6CFE20 starts the bottom bar open.
    pub command_bar_closed: bool,
    /// The bar was open when the local player lost (global 824410, written
    /// only at 4FC1DE/4FC1E7): a match that is over reopens it (4FC591). Like
    /// the global, nothing clears it at a new match.
    pub command_bar_open_at_defeat: bool,
    pub command_pressed: [bool; 11],
    pub command_thumb_pressed: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SelectionMode {
    Repair,
    Sell,
}

impl SidebarGadgetState {
    /// MPlayer_Defeated's local branch closes an open command bar without a
    /// sound and remembers it (`0x004FC1CB..0x004FC1E7`, CloseCommandBar
    /// 0x006D1660).
    pub(crate) fn close_command_bar_at_local_defeat(&mut self) {
        self.command_bar_open_at_defeat = !self.command_bar_closed;
        self.command_bar_closed = true;
    }

    /// Any House's MPlayer_Defeated that finds the match over reopens the bar
    /// the local loss closed (`0x004FC591..0x004FC59F`, OpenCommandBar
    /// 0x006D1610). While the match goes on (`0x004FC57C..0x004FC58B`) a
    /// defeated player watches without it, and SidebarInput refuses to reopen
    /// it (0x006D08F5).
    pub(crate) fn reopen_command_bar_at_match_end(&mut self, match_decided: bool) {
        if match_decided && self.command_bar_open_at_defeat {
            self.command_bar_closed = false;
        }
    }

    /// Display SetRepairMode4AC8C0 / SetSellMode4AC660 (-1). A pending
    /// placement TYPE refuses the entire transition. Both toggles clear the
    /// competing mode, the power byte and the beacon byte (4AC901..4AC916 /
    /// 4AC6A6..4AC6BB), and entry needs House+2F0 (owned-building tracking).
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
        self.place_beacon_mode_on = false;
        self.power_mode_on = false;
        let enabled = owned_buildings > 0
            && !match mode {
                SelectionMode::Repair => self.repair_mode_on,
                SelectionMode::Sell => self.sell_mode_on,
            };
        self.repair_mode_on = enabled && matches!(mode, SelectionMode::Repair);
        self.sell_mode_on = enabled && matches!(mode, SelectionMode::Sell);
        enabled
    }

    /// Residual: a load keeps the running pulses, where SidebarClass Init
    /// 6A5310 stops them and the next StripAI restarts them on a ten-frame
    /// boundary; the blink phase can differ by up to ten frames once, which
    /// a player cannot see.
    ///
    /// This pass's Sidebar Action `visits` (6A77CD/6A7A8C): each polls the
    /// tabs in StripAI, then runs FlashAI 69E010, whose countdown moves once
    /// per call. The earlier visits only tick existing pulses; the last one
    /// polls, so a newly ready item starts at this frame rather than consuming
    /// visits that passed before it was observed. A pass without a visit
    /// neither polls nor ticks.
    pub(crate) fn update_tab_flashes(&mut self, visits: u64, frame: i32, ready: [bool; 4]) {
        if visits == 0 {
            return;
        }
        for _ in 1..visits {
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
        for flash in &mut self.tab_flashes {
            flash.tick();
        }
    }

    /// Display4AC8C0(0)/4AC660(0): a pending building placement refuses mode
    /// changes. Only the Repair/Sell bytes clear here: switching off a mode
    /// that is on clears the power byte too, but power is never on beside
    /// either. The Beacon byte has its own owner (app::input::beacon,
    /// 4AC960) and Power its own setter ([`Self::set_power_mode`]). The
    /// Waypoint byte (+0x11B3, 4AC700) is never armed in YR, and the PlanMgr
    /// planning mode and the bare Super pointer are separate owners these
    /// Display setters do not cancel.
    pub(crate) fn disable_selection_modes(&mut self, placement_armed: bool) {
        if !placement_armed {
            self.repair_mode_on = false;
            self.sell_mode_on = false;
        }
    }

    /// `DisplayClass::SetTogglePowerMode @ 0x004AC820`: `toggle` is the -1
    /// argument (the opposite of the current byte), otherwise 0. Nothing
    /// changes when the requested state equals the byte or a placement type
    /// is set (`+0x11A8`). A change clears the Repair, Sell, Waypoint and
    /// Beacon bytes (`0x004AC866..0x004AC87B`); entry needs House+2F0
    /// (owned-building tracking) above zero, else the mode stays off. Return
    /// whether UnselectAll48DC90 must run (`0x004AC89B`, entry only). The
    /// cursor rows reset by vt+0x48/+0x50 follow from the bytes.
    pub(crate) fn set_power_mode(
        &mut self,
        toggle: bool,
        placement_armed: bool,
        owned_buildings: i32,
    ) -> bool {
        let target = toggle && !self.power_mode_on;
        if target == self.power_mode_on || placement_armed {
            return false;
        }
        self.repair_mode_on = false;
        self.sell_mode_on = false;
        self.place_beacon_mode_on = false;
        self.power_mode_on = target && owned_buildings > 0;
        self.power_mode_on
    }

    /// SetRepairMode(0) and SetSellMode(0), then the direct Power, Planning
    /// and Beacon byte clears (+0x11B2..+0x11B4) that Begin_Building_Placement
    /// (4FB883..4FB8A2), SelectClass::Action (6AAF6D..6AAF8B) and TAction46's
    /// lock (6DEDB2..6DEDD0, 6E268A..6E26A6) share. Only the first two are
    /// refused while a placement type is set; TAction46 nulls it first.
    /// SetWaypointMode (4AC756) also clears the beacon byte; VERA has no
    /// owner for the Planning-waypoint mode, so that path does not exist here.
    pub(crate) fn disable_cursor_modes(&mut self) {
        self.disable_selection_modes(false);
        self.power_mode_on = false;
        self.place_beacon_mode_on = false;
    }

    /// `DisplayClass::Init_Clear @ 0x004A88C0` writes the Repair, Sell and
    /// Power bytes to zero (`0x004A88EB`, `0x004A88FD`, `0x004A8903`) for a
    /// new scenario; the Beacon byte (+0x11B4) is not among them.
    pub(crate) fn clear_for_new_scenario(&mut self) {
        self.repair_mode_on = false;
        self.sell_mode_on = false;
        self.power_mode_on = false;
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
    /// Repair (`0x00B0B3A0`) and Sell (`0x00B07DF8`) are never disabled: the
    /// GadgetClass constructor clears +0x1E (`0x004E1322`), no reference to
    /// either button writes it, and GadgetClass::Disable `0x004E1460` (vtable
    /// slot +0x3C) is only called on the tabs (Init `0x006A546E`, Recalculate
    /// `0x006AAAE7`, `0x006A6858`) and the scroll pair (`0x006A6610`).
    pub fn repair_frame(&self) -> u8 {
        shape_button_frame(false, true, false, self.repair_mode_on, false)
    }

    /// Frame index for the Sell button. Same logic as `repair_frame`.
    pub fn sell_frame(&self) -> u8 {
        shape_button_frame(false, true, false, self.sell_mode_on, false)
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

#[cfg(all(test, tests_ui))]
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

    /// The team-key prologue (Recall731205, CenterCamera7313DE, AddTeam7314FD)
    /// calls this owner for SetRepairMode(0) 0x4AC8C0 / SetSellMode(0)
    /// 0x4AC660: a pending building placement refuses both, and the beacon
    /// byte (+0x11B4) stays with its own owner (app::input::beacon, 4AC960).
    #[test]
    fn disable_selection_modes_refuses_placement_and_leaves_the_beacon_byte() {
        let mut state = SidebarGadgetState::new();
        state.repair_mode_on = true;
        state.sell_mode_on = true;
        state.place_beacon_mode_on = true;
        state.disable_selection_modes(true);
        assert!(
            state.repair_mode_on && state.sell_mode_on,
            "a pending placement refuses 4AC8C0(0)/4AC660(0)"
        );
        assert!(
            state.place_beacon_mode_on,
            "the beacon byte belongs to 4AC960's owner, not this one"
        );
        state.disable_selection_modes(false);
        assert!(!state.repair_mode_on && !state.sell_mode_on);
        assert!(state.place_beacon_mode_on);
    }

    /// SetTogglePowerMode 0x004AC820 against the Repair setter 0x004AC8C0.
    #[test]
    fn power_mode_needs_a_building_and_excludes_the_other_display_modes() {
        let mut state = SidebarGadgetState::new();
        state.sell_mode_on = true;
        state.place_beacon_mode_on = true;
        assert!(!state.set_power_mode(true, true, 1), "placement refuses it");
        assert!(state.sell_mode_on && !state.power_mode_on);
        assert!(!state.set_power_mode(true, false, 0));
        assert!(
            !state.power_mode_on && !state.sell_mode_on && !state.place_beacon_mode_on,
            "a refused entry still clears the other modes"
        );
        assert!(state.set_power_mode(true, false, 1), "entry unselects");
        assert!(state.power_mode_on);
        assert!(state.toggle_selection_mode(SelectionMode::Repair, false, 1));
        assert!(state.repair_mode_on && !state.power_mode_on);
        assert!(state.set_power_mode(true, false, 1));
        assert!(
            !state.set_power_mode(true, false, 1),
            "the key toggles it off"
        );
        assert!(!state.power_mode_on && !state.repair_mode_on);
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
        // Repair/Sell are never disabled, so they never draw frame 2.
        assert_eq!(s.repair_frame(), 0);
        s.repair_mode_on = true;
        assert_eq!(s.repair_frame(), 1);
        assert_eq!(s.sell_frame(), 0);
        s.sell_mode_on = true;
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
    for tick in 1..=100 {
        stepped.update_tab_flashes(1, tick, [tick == 100, false, false, false]);
    }
    caught_up.update_tab_flashes(100, 100, [true, false, false, false]);
    assert_eq!(caught_up.tab_flashes[0].countdown, 19);
    for tick in 101..=200 {
        stepped.update_tab_flashes(1, tick, [true, false, false, false]);
    }
    caught_up.update_tab_flashes(100, 200, [true, false, false, false]);
    let flash = caught_up.tab_flashes[0];
    assert_eq!(flash.state, stepped.tab_flashes[0].state);
    assert_eq!(flash.countdown, stepped.tab_flashes[0].countdown);
    // A pass without a Sidebar Action visit (no frame offline, or the menu
    // or a deactivated window online) neither ticks nor stops the pulse.
    caught_up.update_tab_flashes(0, 200, [false; 4]);
    assert_eq!(caught_up.tab_flashes[0], flash);
    caught_up.update_tab_flashes(1, 201, [false; 4]);
    assert!(!caught_up.tab_flashes[0].is_active());
}

#[test]
fn a_local_loss_closes_the_command_bar_until_the_match_is_over() {
    let mut open = SidebarGadgetState::default();
    open.close_command_bar_at_local_defeat();
    assert!(open.command_bar_closed);
    open.reopen_command_bar_at_match_end(false);
    assert!(open.command_bar_closed, "the match goes on");
    open.reopen_command_bar_at_match_end(true);
    assert!(!open.command_bar_closed);

    let mut closed = SidebarGadgetState {
        command_bar_closed: true,
        ..SidebarGadgetState::default()
    };
    closed.close_command_bar_at_local_defeat();
    closed.reopen_command_bar_at_match_end(true);
    assert!(
        closed.command_bar_closed,
        "a bar the player closed stays closed"
    );
}
