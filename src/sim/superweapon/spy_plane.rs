//! The Spy Plane: `SuperClass::Launch @ 0x006CC390` case 8
//! (`0x006CD66F..0x006CD70B`) and `HouseClass::SendSpyPlanes @ 0x0065EAB0`,
//! which builds each plane and starts it in Mission_SpyplaneApproach. The
//! flight, the camera and the exit are `aircraft::spyplane_mission`; the
//! removal past the map's edge is `aircraft::leave_map`.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `spy_plane_launch` and
//! `send_spy_planes` execute the original code; `spy_plane_tests.rs` replays
//! them.
//!

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};

/// The plane. gamemd holds the literal (`0x00842560`, looked up at
/// `0x006CD67F` through `AircraftTypeClass::FindIndex @ 0x0041CAA0`); no INI
/// key names it.
const SPY_PLANE: &str = "SPYP";

/// Launch case 8 for `owner`'s Spy Plane at `cell`: one plane per
/// `AllyParaDropInf=` entry, each targeting the cell. Returns whether a plane
/// took off.
///
/// The case runs only for a charged Super (`+0x6F`, `0x006CD66F`), which
/// ClickFire already admitted. It finds the plane's type (`0x0041CAA0`, -1
/// when no `[AircraftTypes]` entry is named `SPYP`) and the cell
/// (`MapClass::operator[] @ 0x005657A0`); the shared dummy cell, which an
/// off-map cell answers, sends nothing (`0x006CD69A..0x006CD6A4`). Unless
/// `AllyParaDropInf=` and `AllyParaDropNum=` have as many entries (`Rules+
/// 0xC4C`, `+0xC68`, `0x006CD6A6..0x006CD6BA`) nothing is sent either; each
/// infantry entry then sends one plane while the type is known
/// (`0x006CD6C2`) through SendSpyPlanes ([`super::paradrop::send_planes`]).
/// The local player's launch then drops a queued `EVA_SpyPlaneReady`
/// (`0x006CD6E9..0x006CD707`, the app's `launch_drops_ready_line`), whatever
/// was sent.
///
/// The plane runs no VERA-only aircraft mission: its native mission
/// (`aircraft::spyplane_mission`) is the whole of its behavior.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    (rx, ry): (u16, u16),
    frame_effects: FrameEffects<'_>,
) -> bool {
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx,
        ry,
    });
    let known_type = rules
        .aircraft_ids
        .iter()
        .any(|id| id.eq_ignore_ascii_case(SPY_PLANE));
    let real_cell = sim.resolved_terrain.as_ref().is_some_and(|terrain| {
        matches!(
            NativeCellQuery::canonical(terrain).lookup((rx as i16, ry as i16)),
            NativeCellIdentity::Real(_)
        )
    });
    let lists = &rules.general.ally_paradrop;
    if !real_cell || lists.infantry.len() != lists.counts.len() {
        return false;
    }
    let mut sent = false;
    for _ in 0..lists.infantry.len() {
        if known_type {
            sent |= super::paradrop::send_planes(
                sim,
                rules,
                owner,
                SPY_PLANE,
                MissionType::SpyplaneApproach,
                (rx, ry),
                None,
                frame_effects,
            );
        }
    }
    sent
}
