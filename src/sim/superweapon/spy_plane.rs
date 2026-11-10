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
    // This case plays no positional cue: the target cell's centre, with no
    // Map lookup (a lookup off the map would stamp the shared Dummy).
    let world = crate::sim::components::DriveCoord::cell(rx, ry, 0).into();
    sim.sound_events.emit(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        world,
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
                super::paradrop::Payload::None,
                frame_effects,
            );
        }
    }
    sent
}

/// SW_SpyPlane::Activate (Ares 3.0 `10075780`, hook 6CC390): a charged
/// Super sends its owner's `SpyPlane.Count` planes of `SpyPlane.Type`
/// (Initialize: `SPYP`) in `SpyPlane.Mission` (Initialize:
/// SpyplaneApproach) at the cell `GetCellAt` finds, with no destination,
/// through `HouseClass::SendSpyPlanes @ 0x0065EAB0`, whose loop stops at the
/// first plane it cannot build or put on the map. The activation is
/// reported even uncharged; its Flags are none, so the common Launch runs
/// whole.
///
/// RESIDUALS: an unknown `SpyPlane.Type` (index -1) indexes the type array
/// out of range natively; VERA sends nothing. An off-map cell targets the
/// shared dummy cell natively; VERA targets the requested cell.
#[cfg(feature = "ares")]
pub(super) fn ares_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &crate::rules::superweapon_type::SuperWeaponType,
    cell: (u16, u16),
    frame_effects: FrameEffects<'_>,
) {
    if !super::is_charged(sim, owner, sw_type) {
        return;
    }
    let Some(spy_plane) = rules
        .super_weapon_metadata(&sw.id)
        .map(|metadata| metadata.launch().spy_plane().clone())
    else {
        return;
    };
    let Some(plane) = spy_plane.plane.as_deref() else {
        return;
    };
    for _ in 0..spy_plane.count {
        if !super::paradrop::send_planes(
            sim,
            rules,
            owner,
            plane,
            spy_plane.mission,
            cell,
            super::paradrop::Payload::None,
            frame_effects,
        ) {
            break;
        }
    }
}
