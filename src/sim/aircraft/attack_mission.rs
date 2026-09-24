//! `AircraftClass::Mission_Attack @ 0x00417FE0` states 4..10: the release,
//! the follow-up shot, the strafe run and the exit. States 0, 1 and 3 (the
//! fire-location search and the approach) live in `world::aircraft_attack`.
//!
//! Each state is a pure function over plain facts and a host that performs
//! every effect where the original performs it, so the order of queries,
//! shots, destinations and Scenario draws is native:
//! - [`strike_visit`], states 4..9, runs in the combat phase, where VERA's
//!   FireAt lives (`combat::aircraft_release`);
//! - [`exit_visit`], state 10, runs in the aircraft's own dispatch.
//!
//! A visit returns Mission+0xBC, the mission delay and the `+0x6D2` release
//! latch write. The entry prefixes (`0x00418006`/`0x00418031`/`0x004180A1`/
//! `0x00418BEC`: the latch clear and the pending-ammo consume) run first, in
//! [`enter_attack_state`].
//!
//! Evidence: `tools/spatial_oracle/aircraft_states.py` runs the original
//! 0x00417FE0 for every state, code, class, Ammo, Target, CurleyShuffle and
//! IsClose combination, plus the state-9 delay and state 10 in full;
//! `tests::original_attack_state_rows` replays every combination it covers.
//!
//! RESIDUAL: the Cell `Scatter_Objects` (`0x00481670`, source = the
//! aircraft's coordinates, `1, 0, 0`), once after state 4's whole burst
//! (`0x004184BD`) and after each shot of states 5..9, is asked at its native
//! point and does nothing yet; its recipients' Scatter (`vt+0x174`) belongs
//! to the source-scatter owner. Trigger: every aircraft shot. Effect: units
//! in the bombed cell stay put instead of scattering. Frequency: every
//! bombing pass and missile release.

use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::entity_store::EntityStore;

#[cfg(test)]
#[path = "approach_range_tests.rs"]
mod approach_range_tests;

#[cfg(test)]
#[path = "attack_mission_tests.rs"]
mod tests;

/// The native Target pointer (`+0x2B4`) is non-NULL: a cell, or an object
/// not yet dead. Every state of Mission_Attack and the idle decision read it
/// through this. Natively the killing hit's Destroy = `Detach_All(1)`
/// (`ObjectClass::ReceiveDamage 0x005F5765..0x005F57AF`) announces the expiry,
/// so no Target names a dying object; VERA can keep a dying infantryman's id
/// until its death sequence ends.
pub(crate) fn aircraft_target_present(at: Option<&AttackTarget>, entities: &EntityStore) -> bool {
    match at.map(|attack| attack.target) {
        None => false,
        Some(TargetKind::Cell(..)) => true,
        Some(TargetKind::Entity(id)) => entities
            .get(id)
            .is_some_and(|target| !target.dying && target.health.current > 0),
    }
}

/// Native Mission_Attack entry prefixes418006/418031/4180A1/418BEC.
/// The readiness latch+6D2 already belongs to MissionLeafState; Ammo+2FC and
/// pending+6C8 have one owner independent of the current mission variant.
pub(crate) fn enter_attack_state(entity: &mut crate::sim::game_entity::GameEntity, state: u8) {
    if matches!(state, 0 | 1 | 3 | 10) && entity.mission_leaf.as_aircraft().is_some() {
        entity.mission_leaf.set_aircraft_action_latch(false);
    }
    if matches!(state, 1 | 3 | 10) {
        if let Some(ammo) = entity.aircraft_ammo.as_mut() {
            ammo.consume_release(state == 10);
        }
    }
}

/// One visit's writes: Mission+0xBC, the returned mission delay and, where
/// the visit writes it, the `+0x6D2` release latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Visit {
    pub state: u8,
    pub delay: i32,
    pub latch: Option<bool>,
}

impl Visit {
    fn to(state: u8, delay: i32) -> Self {
        Self {
            state,
            delay,
            latch: None,
        }
    }
}

/// The facts a strike visit (states 4..9) reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StrikeFacts {
    /// Mission+0xBC.
    pub state: u8,
    /// Target `+0x2B4` is non-NULL.
    pub target: bool,
    /// Ammo `+0x2FC`; -1 is unlimited.
    pub ammo: i32,
    /// IFlyControl `+0x18` (`0x0041B7F0`): weapon 0 fires a projectile with
    /// `ROT<=1` and no `Inviso=` (`combat_weapon::aircraft_strafes`).
    pub strafe: bool,
    /// IFlyControl `+0x1C` (`0x0041B840`): `Fighter=` (AircraftType `+0xE0E`).
    pub fighter: bool,
    /// `[General] CurleyShuffle=` (Rules `+0x17E1`).
    pub curley_shuffle: bool,
    /// Weapon 0's `ROF=` (`+0xB0`) and `Range=` (`+0xB4`, leptons), read
    /// through GetWeapon(0) whatever slot fired.
    pub weapon0_rof: i32,
    pub weapon0_range: i32,
    /// TechnoType `+0x678`, the lepton-per-frame speed from `Speed=`.
    pub speed: i32,
}

/// What a strike visit asks and does, each where the original does it.
pub(crate) trait StrikeHost {
    /// `vt+0x3C0(Target, vt+0x2E4 SelectWeapon(Target), 1)`.
    fn fire_error(&mut self) -> FireError;
    /// `vt+0x3AC` (`0x006F7780`): InRange with SelectWeapon's slot.
    fn is_close(&mut self) -> bool;
    /// PrimaryFacing `+0x388` and SecondaryFacing `+0x3A0` Set(DirTo(Target)).
    fn face_target(&mut self);
    /// State 4's release (`0x00418403..0x004184BD`): `+0x6C8` pending, the
    /// burst loop re-reading SelectWeapon's Burst, then the Scatter.
    fn release(&mut self);
    /// `vt+0x3CC FireAt(Target, SelectWeapon(Target))`, once.
    fn fire_at(&mut self);
    /// `CellClass::Scatter_Objects(&Coords, 1, 0, 0)` on the target's cell.
    fn scatter(&mut self);
    /// `vt+0x480 Assign_Destination(Target, 1)`.
    fn assign_target_destination(&mut self);
    /// `vt+0x45C Uncloak(0)`.
    fn uncloak(&mut self);
    /// `0x00418D1D`: `ftol(MissionControl[Mission].Rate * 900)` plus Scenario
    /// `RandomRanged(0, 2)`.
    fn epilogue(&mut self) -> i32;
}

/// `0x00418BC5`: a refused strafe shot polls next frame; at Ammo 0 the run
/// ends and the latch clears.
fn strafe_poll(facts: &StrikeFacts) -> Visit {
    if facts.ammo == 0 {
        Visit {
            state: 10,
            delay: 1,
            latch: Some(false),
        }
    } else {
        Visit::to(facts.state, 1)
    }
}

/// States 4..9 (`0x004182A3`, `0x0041858C`, `0x0041879D`, `0x004188AC`,
/// `0x004189BB`, `0x00418ACA`). The GetFireError switches are
/// `0x00418D98`/`0x00418DB8`/`0x00418DD0`/`0x00418DE8`/`0x00418E00`/
/// `0x00418E14`: states 4 and 5 act on codes 0, 2, 3 and 9 and take their
/// default arm on 1, 4, 5, 6, 7, 8, 10 and 11; states 6..8 fire on 0, 2 and 9
/// (8 first assigns the Target), state 9 on 0, 2, 8 and 9, and both poll on
/// the rest.
pub(crate) fn strike_visit(facts: &StrikeFacts, host: &mut impl StrikeHost) -> Visit {
    use FireError::{Cloaked, Facing, Ok, Range, Rearm};
    // `0x004182A3`: state 4 also leaves on Ammo 0; 5..9 test Target only.
    if !facts.target || (facts.state == 4 && facts.ammo == 0) {
        return Visit::to(10, 1);
    }
    match facts.state {
        4 => {
            // `0x004182D3..0x0041830C`: a strafer does not turn to aim.
            if !facts.strafe {
                host.face_target();
            }
            match host.fire_error() {
                // `0x004184C2`: the suffix after the burst.
                Ok => {
                    host.release();
                    if facts.strafe {
                        Visit {
                            state: 6,
                            delay: facts.weapon0_rof,
                            latch: Some(true),
                        }
                    } else if facts.fighter {
                        Visit {
                            state: if facts.ammo > 0 { 1 } else { 10 },
                            delay: facts.weapon0_rof,
                            latch: Some(true),
                        }
                    } else {
                        Visit::to(5, 1)
                    }
                }
                // `0x00418368`.
                Facing => {
                    if facts.ammo == 0 {
                        return Visit::to(10, 1);
                    }
                    let state = if !host.is_close() || facts.strafe {
                        1
                    } else if facts.fighter || !facts.curley_shuffle {
                        4
                    } else {
                        1
                    };
                    Visit::to(state, if facts.strafe { 45 } else { 1 })
                }
                Rearm => Visit::to(4, 1),
                // `0x0041834C`.
                Cloaked => {
                    host.uncloak();
                    Visit::to(4, 1)
                }
                // `0x00418544`.
                _ => {
                    if facts.ammo == 0 {
                        Visit::to(10, 1)
                    } else if facts.strafe {
                        Visit::to(4, 1)
                    } else {
                        Visit::to(5, 1)
                    }
                }
            }
        }
        // The follow-up shot of an aircraft that does not strafe.
        5 => {
            host.face_target();
            let shuffle = if facts.curley_shuffle { 1 } else { 4 };
            match host.fire_error() {
                // `0x004186B6`: no pending ammo, so the shot is free.
                Ok => {
                    host.fire_at();
                    host.scatter();
                    let state = if facts.ammo == 0 { 10 } else { shuffle };
                    Visit::to(state, host.epilogue())
                }
                // `0x00418634`.
                Facing => {
                    if facts.ammo == 0 {
                        return Visit::to(10, host.epilogue());
                    }
                    let state = if host.is_close() && !facts.strafe {
                        shuffle
                    } else {
                        1
                    };
                    if facts.strafe {
                        Visit::to(state, 45)
                    } else {
                        Visit::to(state, host.epilogue())
                    }
                }
                Rearm => Visit::to(5, host.epilogue()),
                // `0x00418623`.
                Cloaked => {
                    host.uncloak();
                    Visit::to(5, host.epilogue())
                }
                // `0x0041874E`.
                _ => {
                    if facts.ammo == 0 {
                        return Visit::to(10, host.epilogue());
                    }
                    let state = if host.is_close() { shuffle } else { 1 };
                    Visit::to(state, host.epilogue())
                }
            }
        }
        // The strafe run: one bomb each, flying on through the target.
        6..=8 => {
            match host.fire_error() {
                Ok | Facing | Cloaked => {}
                Range => host.assign_target_destination(),
                _ => return strafe_poll(facts),
            }
            host.fire_at();
            host.scatter();
            host.assign_target_destination();
            Visit::to(facts.state + 1, facts.weapon0_rof)
        }
        // `0x00418B1F`: the last bomb; state 3 consumes the pass's ammo.
        9 => match host.fire_error() {
            Ok | Facing | Range | Cloaked => {
                host.fire_at();
                host.scatter();
                Visit::to(3, state9_delay(facts.weapon0_range, facts.speed))
            }
            _ => strafe_poll(facts),
        },
        _ => unreachable!("strike_visit runs states 4..9"),
    }
}

/// `0x00418B8A`: `(Range + 0x400) / Type+0x678`, CDQ and signed IDIV, the
/// sum wrapping. RESIDUAL: a zero speed, or `INT_MIN / -1`, faults the
/// original (`0x00418BB4`); no retail aircraft reaches it, and VERA returns
/// the one-frame delay instead.
pub(crate) fn state9_delay(range: i32, speed: i32) -> i32 {
    range.wrapping_add(0x400).checked_div(speed).unwrap_or(1)
}

/// The facts state 10 reads, after the entry prefix consumed pending ammo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExitFacts {
    pub ammo: i32,
    pub target: bool,
    /// Techno `+0x3D4` (`GameEntity::is_mission_only`): Aircraft Unlimbo
    /// sets it (`0x004143EB`) for a type that is not Selectable, not Landable
    /// or whose weapon 0 is a Camera, and so do the reinforcement and paradrop
    /// spawners.
    pub leaves_map: bool,
    /// `HouseClass::IsControlledByHuman @ 0x0050B730`.
    pub human: bool,
    /// `+0x294`, an AirstrikeClass (Boris' MiGs): the aircraft leaf's
    /// `airstrike_manager_present`, which no VERA producer sets.
    pub airstrike: bool,
}

/// What state 10 does beyond its own writes.
pub(crate) trait ExitHost {
    /// `vt+0x3C8 Assign_Target(NULL)` (`0x006FCDB0`).
    fn clear_target(&mut self);
    /// `MapClass::PickCellOnEdge` (`0x004AA440`) on the house's own edge
    /// (`0x0050DA80`), then `Assign_Destination(cell, 1)`.
    fn assign_edge_destination(&mut self);
    /// `vt+0x1E8 Queue_Mission(Retreat, 0)`.
    fn retreat(&mut self);
    /// `vt+0x484 Enter_Idle_Mode(0, 1)` (`0x004176F0`).
    fn enter_idle_mode(&mut self);
}

/// State 10 (`0x00418BEC`), after the prefix.
pub(crate) fn exit_visit(facts: &ExitFacts, host: &mut impl ExitHost) -> Visit {
    // `0x00418C15`: ammo left and a target: search again.
    if facts.ammo != 0 && facts.target {
        return Visit::to(1, 1);
    }
    // `0x00418C21`: only an empty aircraft of a human house (or one that
    // leaves the map) lets go of its target.
    if facts.ammo == 0 && (facts.leaves_map || facts.human) {
        host.clear_target();
    }
    // `0x00418C43`.
    host.assign_edge_destination();
    if facts.airstrike && facts.ammo > 0 {
        host.retreat();
    } else {
        host.enter_idle_mode();
    }
    Visit {
        state: 10,
        delay: 1,
        latch: Some(false),
    }
}

/// `0x00418D1D`, the delay of states 1, 2 and 5:
/// `ftol(MissionControl[mission].Rate * 900)` plus one Scenario
/// `RandomRanged(0, 2)`. Mission_Attack runs as the Attack mission's handler,
/// so its callers pass Attack; the table is indexed by the current mission.
pub(crate) fn mission_epilogue(
    rules: &crate::rules::ruleset::RuleSet,
    mission: crate::sim::mission::MissionType,
    rng: &mut crate::sim::rng::SimRng,
) -> i32 {
    (rules.mission_control.rate_frames(mission) as i32)
        .wrapping_add(rng.next_range_i32_inclusive(0, 2))
}
