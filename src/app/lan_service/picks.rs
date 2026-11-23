//! The LAN pick domain: the side, colour, start and team one seat carries
//! on the wire, as the shell writes them.
//!
//! One owner: the host's `R` packet, the room it publishes, the
//! cooperative round and the join request decode their values here, so a
//! pick the host accepts is a pick the room re-applies.

use anyhow::{Result, anyhow};

use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishLocalSlot};

/// The wire's pick values, as the shell writes them: the side (-3 while the
/// seat watches, -2 for Random, else the country index 0..=65535), the
/// colour (-2 for Random, else the index 0..=255), the start (-2 or -1 for
/// Auto, else a waypoint 0..=119) and the team (-2 or -1 for no team, else
/// the team index 0..=255).
///
/// A value outside its domain is refused, never clamped to another value.
pub(super) fn decoded_pick(
    old: &SkirmishLocalSlot,
    country: i32,
    color: i32,
    start: i32,
    team: i32,
) -> Result<SkirmishLocalSlot> {
    let mut slot = old.clone();
    // The side: -3 while the seat watches, -2 for Random, else the country
    // index.
    match country {
        -3 => slot.country_random = false,
        -2 => {
            //696F99 clears the concrete country when SetCountry receives Random.
            //The AI-array parser5E5BAB..5E5C3C likewise replaces its word with -2; there
            //is no previous display-row country to retain in that representation.
            slot.country_random = true;
            slot.country = LaunchCountry::America;
        }
        _ => {
            let index = u16::try_from(country)
                .map_err(|_| anyhow!("country outside native registry domain"))?;
            slot.country = LaunchCountry::from_country_index(u32::from(index));
            slot.country_random = false;
        }
    }
    // The colour: -2 for Random.
    match color {
        -2 => {
            //696FC9 sets the concrete color to -1 for Random. The Rust slot
            //represents that sentinel with color_random; its unused u8 payload
            //has canonical value0, independent of whichever row was displayed.
            slot.color_random = true;
            slot.color_index = 0;
        }
        _ => {
            let index =
                u8::try_from(color).map_err(|_| anyhow!("color outside native slot domain"))?;
            slot.color_random = false;
            slot.color_index = index;
        }
    }
    // The start: -2 or -1 for Auto, else a waypoint below the shell's own
    // start list (SKIRMISH_MAX_SLOT_COUNT).
    slot.start_position = LaunchStartPosition::from_shell_value(start)
        .ok_or_else(|| anyhow!("start outside the shell's start domain"))?;
    // The team: -2 or -1 for no team, else the team index.
    slot.team = LaunchTeam::from_shell_value(team)
        .ok_or_else(|| anyhow!("team outside the shell's team domain"))?;
    Ok(slot)
}

/// A participant's own side and colour, as the join request and the
/// participant details carry them: the same side and colour rules the room
/// publication uses. Type3 initializes start/team to Random.
/// Subsequent authoritative host options own their selected values
/// (696FF0/697020 at5E060D..5E061A).
pub(super) fn decoded_participant(
    country: i32,
    color: i32,
    initial: &SkirmishLocalSlot,
) -> Result<SkirmishLocalSlot> {
    decoded_pick(initial, country, color, -2, -2)
}

/// The host's read of a guest's `R` packet: the four words the shell
/// wrote, decoded as one pick, and whether the seat watches.
pub(super) fn decoded_player_choices(
    text: &[u8],
    old: &SkirmishLocalSlot,
) -> Result<(SkirmishLocalSlot, bool)> {
    let text = crate::util::native_string::widen_bytes(text);
    let values: Vec<_> = crate::rules::ini_value::strtok(&text, &[','])
        .map(crate::rules::ini_value::crt_atoi)
        .collect();
    anyhow::ensure!(values.len() >= 4, "incomplete player choices");
    let (country, color, start, team) = (values[0], values[1], values[2], values[3]);
    let slot = decoded_pick(old, country, color, start, team)?;
    Ok((slot, country == -3))
}
