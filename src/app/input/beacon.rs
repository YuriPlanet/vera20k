//! The beacon placement mode and the beacon clicks.
//!
//! The PlaceBeacon key (`BeaconPlacementCommandClass::Execute 0x005370A0`)
//! and the command bar's Beacon button (`TabClass::Action 0x006D0742`) both
//! reach `Enter_Beacon_Placement_Mode 0x00731A30`, which toggles Display's
//! beacon mode (`DisplayClass::SetPlaceBeaconMode 0x004AC960`, `+0x11B4`).
//! With nothing selected DetermineAction answers 0x3C in that mode and 0x3D
//! over a beacon (`0x006927B4..0x006927C5`, `0x006929AF..0x006929B8`); the
//! release (`BandBox_LeftUp 0x004AB9B0`) places a beacon at 0x3C
//! (`0x004AC033..0x004AC0B9`) and selects one at 0x3D
//! (`0x004ABE88..0x004ABED7`). Right-click (`DisplayClass::RightUp
//! 0x004AAE47`) and the team keys (`0x00731205`, `0x007313DE`, `0x007314FD`)
//! turn the mode off. The beacons themselves belong to
//! [`crate::app::match_runtime::beacons`].
//!
//! RESIDUAL: DetermineAction answers 0x3D only when the hovered object
//! passes its owner/cloak and type `+0xC9A` gates (`0x00692677..0x006926D9`);
//! VERA answers it over any beacon. Trigger: an unselected player hovers a
//! beacon cell holding a cloaked enemy or such a type. Effect: the click
//! selects the beacon instead of nothing. Frequency: rare.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::match_runtime::beacons::{self, BeaconTarget};

/// SetPlaceBeaconMode's argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeaconModeRequest {
    /// -1. Native negates RepairMode (`+0x11B0`, read at `0x004AC96B`), not
    /// the beacon byte: the button never turns the mode off.
    Toggle,
    /// 0.
    Off,
}

/// The Display mode bytes SetPlaceBeaconMode reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisplayModes {
    pub(crate) repair: bool,
    pub(crate) sell: bool,
    pub(crate) beacon: bool,
}

/// `DisplayClass::SetPlaceBeaconMode 0x004AC960`. Nothing changes when the
/// requested state equals the beacon byte or a building placement is
/// pending (`+0x11A8`). Entry needs a free slot (`CanPlaceBeacon`,
/// `0x004AC9AD`); otherwise the mode stays off, but Sell and Repair are
/// still cleared (`0x004AC9C0..0x004AC9D5`). The caller unselects on entry.
pub(crate) fn place_beacon_mode_transition(
    request: BeaconModeRequest,
    modes: DisplayModes,
    placement_armed: bool,
    can_place: bool,
) -> Option<DisplayModes> {
    let target = match request {
        BeaconModeRequest::Toggle => !modes.repair,
        BeaconModeRequest::Off => false,
    };
    if target == modes.beacon || placement_armed {
        return None;
    }
    Some(DisplayModes {
        repair: false,
        sell: false,
        beacon: target && can_place,
    })
}

/// The live owner of [`place_beacon_mode_transition`]: Display's bytes are
/// the sidebar gadget state's, and entry runs Unselect_All (`0x004AC9EA`).
/// The cursor rows reset by vt+0x48/+0x50 follow from the bytes.
pub(crate) fn set_place_beacon_mode(state: &mut AppState, request: BeaconModeRequest) {
    let gadgets = &state.match_state.match_presentation.sidebar_gadget_state;
    let modes = DisplayModes {
        repair: gadgets.repair_mode_on,
        sell: gadgets.sell_mode_on,
        beacon: gadgets.place_beacon_mode_on,
    };
    let can_place = beacons::local_house_index(state).is_some_and(|house| {
        state
            .match_state
            .match_presentation
            .beacons
            .can_place(house)
    });
    let Some(next) = place_beacon_mode_transition(
        request,
        modes,
        state.armed_building_type().is_some(),
        can_place,
    ) else {
        return;
    };
    let gadgets = &mut state.match_state.match_presentation.sidebar_gadget_state;
    gadgets.repair_mode_on = next.repair;
    gadgets.sell_mode_on = next.sell;
    gadgets.place_beacon_mode_on = next.beacon;
    if next.beacon {
        crate::app::input::dispatch::unselect_all(state);
    }
}

/// `BeaconPlacementCommandClass::Execute 0x005370A0`: the key refuses
/// GameMode 0 and 5.
pub(crate) fn execute_command(state: &mut AppState) {
    if beacons::feedback_mode(state.match_state.session_mode()) {
        enter_placement_mode(state);
    }
}

/// `Enter_Beacon_Placement_Mode 0x00731A30`, the button's whole action:
/// refused once the local player is out (`[0x00A8B538]`).
pub(crate) fn enter_placement_mode(state: &mut AppState) {
    let defeated = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_none_or(|runtime| runtime.simulation.local_player_defeated());
    if !defeated {
        set_place_beacon_mode(state, BeaconModeRequest::Toggle);
    }
}

/// Whether DetermineAction's no-selection arm answers a beacon action.
pub(crate) fn mode_active(state: &AppState) -> bool {
    state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .place_beacon_mode_on
}

/// The coordinate both clicks and DetermineAction build from the Cell: its
/// centre and level (`Level * 104`, `0x004AC05A..0x004AC09A`).
fn cell_coord(state: &AppState, cell: (u16, u16)) -> [i32; 3] {
    let level = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| runtime.simulation.resolved_terrain())
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .map_or(0, |cell| i32::from(cell.level as i8));
    [
        i32::from(cell.0 as i16) * 256 + 128,
        i32::from(cell.1 as i16) * 256 + 128,
        level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
    ]
}

/// DetermineAction's 0x3D test (`0x00431090` at `0x006927BC`).
pub(crate) fn beacon_at_cell(state: &AppState, cell: (u16, u16)) -> bool {
    state
        .match_state
        .match_presentation
        .beacons
        .find_at(cell_coord(state, cell))
        .is_some()
}

/// BandBox_LeftUp's action 0x3C: PlaceBeacon for PlayerPtr unless the
/// player is out, then the mode is left whatever happened.
pub(crate) fn place_at_cell(state: &mut AppState, cell: (u16, u16)) {
    let defeated = state
        .match_state
        .sim_runtime
        .as_ref()
        .is_none_or(|runtime| runtime.simulation.local_player_defeated());
    if !defeated && let Some(house) = beacons::local_house_index(state) {
        let coord = cell_coord(state, cell);
        beacons::place_beacon(state, house, coord, None);
    }
    set_place_beacon_mode(state, BeaconModeRequest::Off);
}

/// BandBox_LeftUp's action 0x3D: `SelectBeacon 0x00430F70` at the Cell.
pub(crate) fn select_at_cell(state: &mut AppState, cell: (u16, u16)) {
    let coord = cell_coord(state, cell);
    if let Some((house, slot)) = state.match_state.match_presentation.beacons.find_at(coord) {
        crate::app::input::dispatch::unselect_all(state);
        state
            .match_state
            .match_presentation
            .beacons
            .set_selected(house, slot);
    }
}

/// `DeleteCommand 0x00731A10`'s beacon arm: with no planning node to delete,
/// `DeleteBeacon(-1, -1)` (`0x00731A24`).
pub(crate) fn delete_selected(state: &mut AppState) {
    beacons::delete_beacon(state, BeaconTarget::Selected);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::match_runtime::beacons::feedback_mode;
    use crate::app::match_runtime::session::SessionMode;

    const OFF: DisplayModes = DisplayModes {
        repair: false,
        sell: false,
        beacon: false,
    };

    #[test]
    fn the_key_admits_network_modes_and_the_toggle_reads_repair_mode() {
        assert!(feedback_mode(SessionMode::Lan) && feedback_mode(SessionMode::Wol));
        assert!(!feedback_mode(SessionMode::Skirmish) && !feedback_mode(SessionMode::Campaign));
        let toggle = BeaconModeRequest::Toggle;
        let on = DisplayModes {
            beacon: true,
            ..OFF
        };
        assert_eq!(
            place_beacon_mode_transition(toggle, DisplayModes { sell: true, ..OFF }, false, true),
            Some(on),
            "entry clears Sell"
        );
        assert_eq!(
            place_beacon_mode_transition(toggle, on, false, true),
            None,
            "a second press leaves the mode on"
        );
        assert_eq!(
            place_beacon_mode_transition(
                toggle,
                DisplayModes {
                    repair: true,
                    ..OFF
                },
                false,
                true
            ),
            None,
            "with Repair on the toggle asks for off, which it already is"
        );
        assert_eq!(
            place_beacon_mode_transition(BeaconModeRequest::Off, on, false, true),
            Some(OFF)
        );
    }

    #[test]
    fn full_slots_or_a_pending_placement_keep_the_mode_off() {
        let toggle = BeaconModeRequest::Toggle;
        let sell = DisplayModes { sell: true, ..OFF };
        assert_eq!(
            place_beacon_mode_transition(toggle, sell, false, false),
            Some(OFF),
            "no free slot: off, Sell cleared anyway"
        );
        assert_eq!(place_beacon_mode_transition(toggle, sell, true, true), None);
    }
}
