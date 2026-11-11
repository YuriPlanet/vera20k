//! `AircraftClass::Mission_Enter @ 0x00419C80` (vt+0x240): an aircraft
//! flying to its dock, landing on the pad its radio contact names and
//! handing itself over. Mission+0xBC is MissionCom's handler state.
//!
//! Every visit first:
//! - an AirportBound aircraft landed (vt+0x1C8 = 0) away from its NavCom
//!   (a NavCom other than the cell's first Building, `0x0047C520`) takes
//!   that NavCom as its dock (`+0x6CC`, NULL unless a Building), breaks its
//!   radio contact (vt+0x274 OVER_OUT) and queues Move to commence at once
//!   (`Queue_Mission(Move, 1)`), returning 1;
//! - an aircraft off the ground whose NavCom is a Building other than its
//!   dock takes it as its dock (`0x00419D57`);
//! - an aircraft without a NavCom off the ground layer (vt+0x78) whose
//!   contact does not answer DOCKING (vt+0x274(0xE)) ROGER and that has no
//!   entry to fly toward (`TechnoClass @ 0x0070D8F0`) enters idle mode and
//!   returns 1.
//!
//! Then by state (jump table `0x0041A140`): 0 goes to 6; 0..5 set `+0x6D4`
//! and return 1. State 6 (`0x00419DE0`): an AirportBound aircraft off the
//! ground asks its contact DOCKING again; in radio contact it goes to 7 with
//! `+0x6D4` clear once its Fly descends (Get_Status 1, `0x004CFE50`) or, if
//! AirportBound, lands, and returns 3. Out of contact, without an entry to
//! fly toward, a descending aircraft goes to 7, and with no queued mission
//! it drops its destination and enters idle mode. State 7 (`0x00419F09`):
//! while descending it steps its Location at most 5 leptons in X and Y
//! toward its NavCom's dock coordinate (a Building's GetDockCoord, vt+0xA8,
//! for this aircraft; else vt+0x48) through SetLocation (vt+0x1B4). Landed,
//! it sends DOCK_NOW (0x15): ROGER queues Guard, 5 (an aircraft contact's
//! answer, `0x0041938E`) boards the contact (`0x0041A02C..0x0041A058`) and
//! any other answer enters idle mode. Those set `+0x6D4` and return 1; a
//! state past 7 returns 1.
//!
//! A ROGER for a `Carryall=` type with cargo delivers it to the dock
//! instead of queueing Guard (`0x0041A059..0x0041A124`,
//! [`EnterHost::deliver_cargo`]).
//!
//! Evidence: tools/spatial_oracle/aircraft_enter.py runs the original
//! 0x00419C80 over every state with each combination of what it reads,
//! the radio answers and the entry search stubbed; `enter_mission_tests`
//! replays every row through [`enter_visit`].

use crate::sim::mission::MissionType;

#[cfg(test)]
#[path = "enter_mission_tests.rs"]
mod tests;

/// The NavCom (`+0x5A4`) as the visit compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnterNav {
    None,
    /// A BuildingClass (What_Am_I 6).
    Building(u64),
    /// A cell or another object.
    Other,
}

impl EnterNav {
    fn building(self) -> Option<u64> {
        match self {
            Self::Building(id) => Some(id),
            Self::None | Self::Other => None,
        }
    }
}

/// `FlyLocomotionClass::Get_Status @ 0x004CFE50`'s descending answer.
const STATUS_DESCENDING: i32 = 1;

/// What a Mission_Enter visit does and reads, each where the original does
/// it.
pub(crate) trait EnterHost {
    /// GetHeight (vt+0x1C8).
    fn height(&mut self) -> i32;
    /// The type's `AirportBound=` (`+0xE0D`).
    fn airport_bound(&mut self) -> bool;
    /// The NavCom `+0x5A4`.
    fn nav_com(&mut self) -> EnterNav;
    /// The first Building of the cell under the aircraft (vt+0x1BC,
    /// `0x0047C520`).
    fn building_here(&mut self) -> Option<u64>;
    /// The dock `+0x6CC`.
    fn dock(&mut self) -> Option<u64>;
    /// `+0x6CC = dock`.
    fn set_dock(&mut self, dock: Option<u64>);
    /// vt+0x274 `Transmit_Message(OVER_OUT)` to contact 0.
    fn over_out(&mut self);
    /// vt+0x1E8 `Queue_Mission(mission, start)` (`0x0041BA90`).
    fn queue(&mut self, mission: MissionType, start: bool);
    /// vt+0x78 answers the ground layer (2).
    fn on_ground_layer(&mut self) -> bool;
    /// vt+0x274 `Transmit_Message(DOCKING)` to contact 0 answers ROGER.
    fn docking(&mut self) -> bool;
    /// `TechnoClass @ 0x0070D8F0`: a pending entry (`+0x500`) to fly toward.
    fn approach_pending_entry(&mut self) -> bool;
    /// vt+0x484 `Enter_Idle_Mode(0, 1)` (`0x004176F0`).
    fn enter_idle_mode(&mut self);
    /// Mission+0xBC.
    fn state(&mut self) -> u32;
    /// Mission+0xBC = `state`.
    fn set_state(&mut self, state: u32);
    /// `+0x6D4 = ready`.
    fn set_transition_ready(&mut self, ready: bool);
    /// `RadioClass::In_Radio_Contact @ 0x0065AE30`.
    fn in_radio_contact(&mut self) -> bool;
    /// The locomotor's Get_Status (ILocomotion `+0x90`).
    fn status(&mut self) -> i32;
    /// The queued mission `+0xB4` is NONE.
    fn nothing_queued(&mut self) -> bool;
    /// vt+0x480 `Assign_Destination(NULL, 1)`.
    fn clear_destination(&mut self);
    /// Steps the Location at most 5 leptons in X and Y toward the NavCom's
    /// dock coordinate, keeping Z, through SetLocation (vt+0x1B4); nothing
    /// without a NavCom.
    fn step_toward_nav_com(&mut self);
    /// vt+0x274 `Transmit_Message(DOCK_NOW)` to contact 0.
    fn dock_now(&mut self) -> i32;
    /// Limbo (vt+0xD4, `0x004DB260`), then `CargoClass::AddPassenger @
    /// 0x004733A0` on contact 0's cargo (`+0x114`).
    fn board_contact(&mut self);
    /// DOCK_NOW's ROGER for a `Carryall=` type (`+0xDFC`) with a first
    /// passenger (`+0x118`): its delivery (`0x0041A06D..0x0041A124`). False,
    /// doing nothing, for any other aircraft.
    fn deliver_cargo(&mut self) -> bool;
}

/// One Mission_Enter visit and the frames it returns.
pub(crate) fn enter_visit(host: &mut impl EnterHost) -> i32 {
    if host.height() == 0 && host.airport_bound() {
        let nav = host.nav_com();
        if nav != EnterNav::None {
            let here = host.building_here();
            if nav.building().is_none() || nav.building() != here {
                host.set_dock(nav.building());
                host.over_out();
                host.queue(MissionType::Move, true);
                return 1;
            }
        }
    }
    if host.height() > 0
        && let EnterNav::Building(building) = host.nav_com()
        && host.dock() != Some(building)
    {
        host.set_dock(Some(building));
    }
    if host.nav_com() == EnterNav::None
        && !host.on_ground_layer()
        && !host.docking()
        && !host.approach_pending_entry()
    {
        host.enter_idle_mode();
        return 1;
    }
    match host.state() {
        0..=5 => {
            if host.state() == 0 {
                host.set_state(6);
            }
            host.set_transition_ready(true);
            1
        }
        6 => {
            if host.airport_bound() && host.height() != 0 {
                host.docking();
            }
            if host.in_radio_contact() {
                if host.status() == STATUS_DESCENDING || host.airport_bound() && host.height() == 0
                {
                    host.set_state(7);
                    host.set_transition_ready(false);
                }
                return 3;
            }
            if !host.approach_pending_entry() {
                if host.status() == STATUS_DESCENDING {
                    host.set_state(7);
                    host.set_transition_ready(true);
                }
                if host.nothing_queued() {
                    host.clear_destination();
                    host.enter_idle_mode();
                }
            }
            host.set_transition_ready(true);
            1
        }
        7 => {
            if host.status() == STATUS_DESCENDING {
                host.step_toward_nav_com();
                host.set_transition_ready(true);
                return 1;
            }
            host.set_transition_ready(true);
            match host.dock_now() {
                5 => host.board_contact(),
                1 => {
                    if !host.deliver_cargo() {
                        host.queue(MissionType::Guard, false);
                    }
                }
                _ => host.enter_idle_mode(),
            }
            1
        }
        _ => 1,
    }
}
