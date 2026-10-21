//! `AircraftClass::Mission_Move @ 0x004166E0`, the handler of every aircraft
//! on the Move mission: a Fly aircraft its player or its spawn manager sent,
//! and a missile its launcher just fired. Its state is Mission+0xBC, which
//! Commence zeroes.
//!
//! - State 0 (`0x004166FB`): with no NavCom (`+0x5A4`) the aircraft enters
//!   idle mode (`vt+0x484(0, 1)`); otherwise its destination becomes
//!   `Find_Attack_Cell(NavCom)` (`vt+0x480(.., 1)`, `0x00418E20`) and the
//!   state 1. Both, and a state past 4, return the Rate epilogue
//!   (`0x00416713`): `ftol(Rate x 900)` of the mission current after those
//!   calls, plus one Scenario `RandomRanged(0, 2)` (`0x0041673E`).
//! - State 1 (`0x00416769`): no NavCom enters idle mode; otherwise the
//!   locomotor's Move_To (`+0x44`) heads for the NavCom's coordinate
//!   (`vt+0x4C`, the aircraft asking) and the state is 2.
//! - State 2 (`0x004167CE`): a stopped locomotor (`Is_Moving`, `+0x10`), or a
//!   NavCom whose cell is the aircraft's own (`vt+0x1B8`), is state 3; a
//!   NavCom cell not free for landing (`0x00419B00`, counting the aircraft
//!   itself) restarts at 0; otherwise state 4.
//! - State 3 (`0x0041698E`): idle mode once the locomotor stops.
//! - State 4 (`0x004169CD`): as state 2, staying 4 where state 2 enters it.
//!
//! States 1..4 return one frame. `0x00419B00` counts the aircraft's own Cell
//! NavCom against its cell, so a moving aircraft with a Cell NavCom cycles 0,
//! 1, 2, a new Find_Attack_Cell and Move_To every Rate epilogue, unless that
//! function answers free before looking at objects: for a cell outside the
//! playfield, an AirportBound aircraft (`0x00419B8A`), or a spawned aircraft
//! over a spawner.
//!
//! The Move slot (`vt+0x22C`) is `0x004166C0`, which sends a `Carryall=`
//! type (Type+0xDFC) to `0x00416D50` instead. RESIDUAL: that handler is not
//! ported. No retail aircraft type sets the key
//! (`retail_aircraft_types_are_not_carryalls`), so it is dormant.
//!
//! Evidence: tools/spatial_oracle/aircraft_move.py runs the original
//! 0x004166E0 from states 0..5 with a NULL, Cell and object NavCom, moving or
//! not, and its Find_Attack_Cell and landing predicates on supplied map
//! states; `world::aircraft_move`'s tests replay every row through
//! [`move_visit`] and the world queries it gives the production host.
//!
//! Not ported, unreachable in YR: state 2's waypoint-path arm
//! (`0x004167CE..0x00416885`: with `+0x520` set, within 256 leptons of the
//! NavCom it steps to the path's next waypoint through `0x004DC8C0`). `+0x520`
//! is written only by `FootClass::SetPathIndex @ 0x004DC810` (vt+0x1A4), with
//! an index that `HouseClass @ 0x005023B0` finds among the waypoints of the
//! house paths `+0x210`. The only routine that adds such a waypoint,
//! `WaypointPathClass @ 0x007639A0`, is called only from `0x00502160`, which
//! no call, jump or pointer reaches, so those paths stay empty. YR planning
//! mode uses its own nodes (`world::planning`).

/// What a Mission_Move visit asks and does, each where the original does it.
pub(crate) trait MoveHost {
    /// NavCom (`+0x5A4`) is non-NULL.
    fn nav_com(&self) -> bool;
    /// `vt+0x484 Enter_Idle_Mode(0, 1)`.
    fn enter_idle_mode(&mut self);
    /// `vt+0x480 Assign_Destination(Find_Attack_Cell(NavCom), 1)`.
    fn assign_attack_cell(&mut self);
    /// The locomotor's `Move_To(NavCom vt+0x4C(this))`.
    fn move_to_nav_com(&mut self);
    /// The locomotor's `Is_Moving` (`+0x10`).
    fn is_moving(&mut self) -> bool;
    /// The NavCom's cell (its `vt+0x4C` coordinate over 256 toward zero) is
    /// the aircraft's own (`vt+0x1B8`, `0x0041BEA0`).
    fn nav_cell_is_own(&mut self) -> bool;
    /// `Is_Cell_Free_For_Landing(NavCom cell, 1)`.
    fn nav_cell_free(&mut self) -> bool;
    /// The Rate epilogue on the mission current now.
    fn epilogue(&mut self) -> i32;
}

/// One Mission_Move visit from `state`: its writes, Mission+0xBC and the
/// returned mission delay.
pub(crate) fn move_visit(state: u8, host: &mut impl MoveHost) -> (u8, i32) {
    let state = match state {
        0 => {
            if host.nav_com() {
                host.assign_attack_cell();
                1
            } else {
                host.enter_idle_mode();
                0
            }
        }
        1 => {
            if !host.nav_com() {
                host.enter_idle_mode();
                return (state, 1);
            }
            host.move_to_nav_com();
            return (2, 1);
        }
        2 | 4 => {
            let state = if !host.is_moving() || host.nav_com() && host.nav_cell_is_own() {
                3
            } else if host.nav_com() && !host.nav_cell_free() {
                0
            } else {
                4
            };
            return (state, 1);
        }
        3 => {
            if !host.is_moving() {
                host.enter_idle_mode();
            }
            return (state, 1);
        }
        state => state,
    };
    (state, host.epilogue())
}
