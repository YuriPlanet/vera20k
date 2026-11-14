//! `AircraftClass::Mission_Guard @ 0x0041A5C0` (vt+0x21C, the Guard and
//! Sticky slot) and `AircraftClass::Mission_AreaGuard @ 0x0041A940`
//! (vt+0x220): what an aircraft does on its idle missions. Neither keeps a
//! Mission+0xBC state.
//!
//! Mission_Guard:
//! - At its type's flight level (vt+0x1C8 equal to Type vt+0xBC): a team
//!   member with a NavCom sets `+0x6D4` and queues Move, and a team member
//!   returns `ftol(Rate x 900)` of the current mission (`0x0041A612`); an
//!   aircraft without a weapon in slot 0 flies to the cell under it
//!   (`MapClass::Get_CellClass_At_Coord @ 0x00565730`) on Move; any other
//!   sets `+0x6D4` and re-enters idle mode. Those return 1.
//! - Below it: an armed aircraft short of its type's `Ammo=` hunts a dock out
//!   of radio contact (`0x0041A7BD`): one found, it queues Enter, flies there
//!   and drops its Target. The two other rearm thresholds (Ammo 0, below half)
//!   belong to the globals `0x00889ECC`/`0x00889ECD`, which no code writes,
//!   so they are dormant. Then an aircraft below half its `Ammo=` in radio
//!   contact with a `UnitReload=` building waits there (`0x0041A6E0`); one
//!   with a Target queues Attack; those return 1. An unarmed aircraft, and
//!   one landed out of radio contact, returns 45 frames. A computer house's
//!   aircraft takes the nearest enemy vehicle on a bridge deck around it
//!   (`HouseClass @ 0x00500300`) as its Target and queues Attack. Then a
//!   human house's aircraft off high flight (vt+0x54) returns the Rate
//!   epilogue (`ftol(Rate x 900) + RandomRanged(0, 2)`), and any other runs
//!   `FootClass::Mission_Guard @ 0x004D5070`.
//!
//! Mission_AreaGuard: at its flight level an aircraft outside a team
//! re-enters idle mode, and it returns 1; below it an armed aircraft without
//! Ammo out of radio contact re-enters idle mode, then a Target queues Attack
//! (1) and anything else runs `FootClass::Mission_AreaGuard @ 0x004D6AA0`.
//!
//! Evidence: tools/spatial_oracle/aircraft_guard.py runs the original
//! 0x0041A5C0 and 0x0041A940 over every combination of the reads above, with
//! the dock search, the house search and the Foot bodies stubbed;
//! `guard_mission_tests` replays every row through [`guard_visit`] and
//! [`area_guard_visit`].

use crate::sim::mission::MissionType;

#[cfg(all(test, tests_sim_a_b))]
#[path = "guard_mission_tests.rs"]
mod tests;

/// Frames an unarmed or landed, unlinked aircraft waits (`0x0041A854`,
/// `0x0041A878`).
const GUARD_IDLE_FRAMES: i32 = 0x2D;

/// What a Mission_Guard visit reads that it does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuardFacts {
    /// GetHeight (vt+0x1C8).
    pub(crate) height: i32,
    /// The type's FlightLevel (Type vt+0xBC).
    pub(crate) flight_level: i32,
    /// The team `+0x5D4` is set.
    pub(crate) team: bool,
    /// The NavCom `+0x5A4` is set.
    pub(crate) nav_com: bool,
    /// `GetWeapon(0)` (vt+0x3F8) names a WeaponType.
    pub(crate) weapon: bool,
    /// `TechnoClass::Is_Armed @ 0x00701120` (vt+0x2AC).
    pub(crate) armed: bool,
    /// Ammo `+0x2FC`.
    pub(crate) ammo: i32,
    /// The type's `Ammo=` (`+0x684`).
    pub(crate) type_ammo: i32,
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` on the owner.
    pub(crate) human: bool,
}

/// What a Mission_Guard or Mission_AreaGuard visit does and reads live, each
/// where the original does it.
pub(crate) trait GuardHost {
    /// `+0x6D4 = 1`.
    fn set_transition_ready(&mut self);
    /// vt+0x1E8 `Queue_Mission(mission, 0)` (`0x0041BA90`).
    fn queue(&mut self, mission: MissionType);
    /// `ftol(Rate x 900)` of the current mission (`0x005B3A00`).
    fn rate(&mut self) -> i32;
    /// vt+0x480 `Assign_Destination(cell under the aircraft, 1)`.
    fn assign_own_cell(&mut self);
    /// vt+0x484 `Enter_Idle_Mode(0, 1)` (`0x004176F0`).
    fn enter_idle_mode(&mut self);
    /// `RadioClass::In_Radio_Contact @ 0x0065AE30`.
    fn in_radio_contact(&mut self) -> bool;
    /// vt+0x528 `Find_Docking_Bay(&Type->Dock, 0, 0)` (`0x0041BBD0`).
    fn find_dock(&mut self) -> Option<u64>;
    /// vt+0x480 `Assign_Destination(dock, 1)`.
    fn assign_dock(&mut self, dock: u64);
    /// vt+0x3C8 `Assign_Target(NULL)`.
    fn clear_target(&mut self);
    /// Contact 0 is a Building whose type is `UnitReload=` (`+0x16AA`).
    fn contact_reloads(&mut self) -> bool;
    /// The Target `+0x2B4` is set.
    fn target(&mut self) -> bool;
    /// `HouseClass @ 0x00500300` on the owner and the Location: the nearest
    /// enemy vehicle on a bridge deck; vt+0x3C8 `Assign_Target` on it and
    /// vt+0x1E8 `Queue_Mission(Attack, 0)` when there is one.
    fn attack_bridge_unit(&mut self);
    /// vt+0x54 (`0x0041B920`), high in the air.
    fn in_air(&mut self) -> bool;
    /// Scenario `RandomRanged(0, 2)` (`0x0041A91C`).
    fn jitter(&mut self) -> i32;
    /// `FootClass::Mission_Guard @ 0x004D5070`.
    fn foot_guard(&mut self) -> i32;
    /// `FootClass::Mission_AreaGuard @ 0x004D6AA0`.
    fn foot_area_guard(&mut self) -> i32;
}

/// One Mission_Guard visit and the frames it returns.
pub(crate) fn guard_visit(facts: &GuardFacts, host: &mut impl GuardHost) -> i32 {
    if facts.height == facts.flight_level {
        if facts.team {
            if facts.nav_com {
                host.set_transition_ready();
                host.queue(MissionType::Move);
            }
            return host.rate();
        }
        if !facts.weapon {
            host.assign_own_cell();
            host.queue(MissionType::Move);
        } else {
            host.set_transition_ready();
            host.enter_idle_mode();
        }
        return 1;
    }
    // `0x0041A793`: the stock rearm threshold.
    if facts.armed
        && facts.ammo < facts.type_ammo
        && !host.in_radio_contact()
        && let Some(dock) = host.find_dock()
    {
        host.queue(MissionType::Enter);
        host.assign_dock(dock);
        host.clear_target();
        return 1;
    }
    // `0x0041A6E0`: half the type's Ammo, rounded toward zero.
    if facts.ammo != -1
        && facts.ammo < facts.type_ammo / 2
        && host.in_radio_contact()
        && host.contact_reloads()
    {
        return 1;
    }
    if host.target() {
        host.queue(MissionType::Attack);
        return 1;
    }
    if !facts.armed || facts.height == 0 && !host.in_radio_contact() {
        return GUARD_IDLE_FRAMES;
    }
    if !facts.human {
        host.attack_bridge_unit();
    }
    if facts.human && !host.in_air() {
        let rate = host.rate();
        return rate.wrapping_add(host.jitter());
    }
    host.foot_guard()
}

/// One Mission_AreaGuard visit and the frames it returns.
pub(crate) fn area_guard_visit(facts: &GuardFacts, host: &mut impl GuardHost) -> i32 {
    if facts.height == facts.flight_level {
        if !facts.team {
            host.enter_idle_mode();
        }
        return 1;
    }
    if facts.ammo == 0 && facts.armed && !host.in_radio_contact() {
        host.enter_idle_mode();
    }
    if host.target() {
        host.queue(MissionType::Attack);
        return 1;
    }
    host.foot_area_guard()
}
