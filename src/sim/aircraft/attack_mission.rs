//! `AircraftClass::Mission_Attack @ 0x00417FE0` states 4..10: the release,
//! the follow-up shot, the strafe run and the exit. States 0, 1 and 3 (the
//! fire-location search and the approach) live in `world::aircraft_attack`.
//!
//! Each state is a pure function over plain facts and a host that performs
//! every effect where the original performs it, so the order of queries,
//! shots, destinations and Scenario draws is native:
//! - [`strike_visit`], states 4..9, runs through the receiver, where VERA's
//!   FireAt lives (`combat::aircraft_release`);
//! - [`exit_visit`], state 10, runs in the world's dispatch host.
//!
//! Both run inside the aircraft's own dispatch.
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
//! The Cell `Scatter_Objects` (`0x00481670`, source = the aircraft's
//! Location, `1, 0, 0`) runs once after state 4's whole burst (`0x004184BD`)
//! and after each shot of states 5..9.

use crate::sim::combat::fire_error::FireError;

#[cfg(all(test, tests_sim_a_b))]
#[path = "approach_range_tests.rs"]
mod approach_range_tests;

#[cfg(all(test, tests_sim_a_b))]
#[path = "attack_mission_tests.rs"]
mod tests;

/// Native Mission_Attack entry prefixes418006/418031/4180A1/418BEC.
/// The readiness latch+6D2 already belongs to MissionLeafState; Ammo+2FC and
/// pending+6C8 have one owner independent of the current mission variant.
pub(crate) fn enter_attack_state(entity: &mut crate::sim::game_entity::GameEntity, state: u8) {
    if matches!(state, 0 | 1 | 3 | 10) && entity.mission_leaf.as_aircraft().is_some() {
        entity.mission_leaf.set_aircraft_action_latch(false);
    }
    if matches!(state, 1 | 3 | 10)
        && let Some(ammo) = entity.ammo_mut()
    {
        ammo.consume_release(state == 10);
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

/// The Phobos `WeaponTypeExt` strafing keys the strike states read, with the
/// `[General]` fallbacks already applied (`Ext/WeaponType/Body.cpp:108-112`,
/// `Ext/Aircraft/Hooks.cpp:37`, `:211-228`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StrafeKeys {
    /// `Strafing.Shots.Get(5)`.
    pub shots: i32,
    /// `Strafing.SimulateBurst.Get([General] Strafing.SimulateBurst)`.
    pub simulate_burst: bool,
    /// `Strafing.UseAmmoPerShot.Get([General] Strafing.UseAmmoPerShot)`.
    pub use_ammo_per_shot: bool,
    /// `Strafing.TargetCell.Get([General] Strafing.TargetCell)`.
    pub target_cell: bool,
    /// `Strafing.EndDelay`; unset derives the range over speed.
    pub end_delay: Option<i32>,
}

impl StrafeKeys {
    /// A weapon that authors none of the five keys: all globals default off
    /// and `Get(5)` reads five shots.
    pub(crate) const STOCK: Self = Self {
        shots: 5,
        simulate_burst: false,
        use_ammo_per_shot: false,
        target_cell: false,
        end_delay: None,
    };

    /// The weapon's five keys with their `[General]` fallbacks applied
    /// (`Ext/WeaponType/Body.cpp:108-112`).
    pub(crate) fn from_weapon(
        weapon: &crate::rules::weapon_type::WeaponType,
        globals: &crate::rules::phobos_globals::PhobosGlobals,
    ) -> Self {
        let rules = weapon.phobos_strafing();
        Self {
            shots: rules.shots(),
            simulate_burst: rules.simulate_burst(globals),
            use_ammo_per_shot: rules.use_ammo_per_shot(globals),
            target_cell: rules.target_cell(globals),
            end_delay: rules.end_delay,
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
    /// Ammo `+0x2FC` as the visit opens; -1 is unlimited. The reads after a
    /// shot ask [`StrikeHost::ammo`].
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
    /// `GetWeapon(CurrentAircraftWeaponIndex)`'s Phobos strafing keys; weapon
    /// 0 stands for the slot the same way [`Self::weapon0_rof`] does.
    pub strafing: StrafeKeys,
    /// Phobos `AircraftTypeExt::FiringForceScatter.Get([General]
    /// AircraftFiringForceScatter)` (`Ext/Aircraft/Hooks.cpp:257`, `:348`,
    /// `:354`): false skips the attack mission's target-cell Scatter.
    pub firing_force_scatter: bool,
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
    /// `CellClass::Scatter_Objects(&Location, 1, 0, 0)` on the target's cell.
    fn scatter(&mut self);
    /// The same Scatter on the strafing target cell
    /// (`Ext/Aircraft/Hooks.cpp:257`: `pTargetCell->ScatterContent(Location,
    /// true, false, false)`), which the stored cell keeps.
    fn scatter_at_cell(&mut self, cell: (u16, u16));
    /// `vt+0x480 Assign_Destination(Target, 1)`.
    fn assign_target_destination(&mut self);
    /// `vt+0x45C Uncloak(0)`.
    fn uncloak(&mut self);
    /// `0x00418D1D`: `ftol(MissionControl[Mission].Rate * 900)` plus Scenario
    /// `RandomRanged(0, 2)`.
    fn epilogue(&mut self) -> i32;
    /// Ammo `+0x2FC` as it stands after a shot, which states 4 and 5 read
    /// again (`0x00418506`, `0x00418720`): Fire_At's passenger arm
    /// (Drop_Payload) spends one.
    fn ammo(&mut self) -> i32;
    /// Phobos `AircraftExt::Strafe_BombsDroppedThisRound` (`Ext/Aircraft/Body.h:14`).
    fn strafe_counter(&mut self) -> i32;
    /// The `AircraftExt::FireWeapon` increment (`Ext/Aircraft/Body.cpp:34`).
    fn set_strafe_counter(&mut self, value: i32);
    /// Phobos `AircraftExt::Strafe_TargetCell` (`Ext/Aircraft/Body.h:15`).
    fn strafe_cell(&mut self) -> Option<(u16, u16)>;
    /// The round-end clear (`Ext/Aircraft/Hooks.cpp:220`).
    fn set_strafe_cell(&mut self, cell: Option<(u16, u16)>);
    /// `MapClass::GetCellAt(Target->GetCoords())` into `Strafe_TargetCell`
    /// (`Ext/Aircraft/Hooks.cpp:234-235`, hook `0x4184CC`).
    fn store_target_cell(&mut self);
    /// `GetFireError(pTargetCell, weaponIndex, true)` (`Ext/Aircraft/Hooks.cpp:252`).
    fn cell_fire_error(&mut self, cell: (u16, u16)) -> FireError;
    /// `AircraftExt::FireWeapon(pThis, pTargetCell)` (`Ext/Aircraft/Hooks.cpp:255`,
    /// `Ext/Aircraft/Body.cpp:12-47`): the burst loop, then the round counter
    /// and the per-shot ammo rule.
    fn fire_at_cell(&mut self, cell: (u16, u16));
    /// `SetDestination(pTargetCell, 1)` (`Ext/Aircraft/Hooks.cpp:259`).
    fn assign_cell_destination(&mut self, cell: (u16, u16));
}

/// Phobos `AircraftClass_Mission_Attack_StrafeShots` (`Ext/Aircraft/Hooks.cpp:25-50`,
/// hook `0x417FF1`), at the Mission_Attack entry of every visit: a state
/// outside `FireAtTarget2_Strafe..FireAtTarget5_Strafe` (VERA 6..=9) resets
/// the round counter; with shots fired and `Strafing.Shots > 5`, state
/// `FireAtTarget3_Strafe` (7) re-enters `FireAtTarget2_Strafe` (6) while
/// `shots - 3 - counter > 0`.
pub(crate) fn strafe_entry(state: u8, counter: i32, shots: i32) -> (u8, i32) {
    if !(6..=9).contains(&state) {
        return (state, 0);
    }
    if counter != 0 && shots > 5 && state == 7 {
        let remaining = shots.wrapping_sub(3).wrapping_sub(counter);
        if remaining > 0 {
            return (6, counter);
        }
    }
    (state, counter)
}

/// Phobos `GetDelay` (`Ext/Aircraft/Hooks.cpp:211-228`), after a shot: the
/// weapon's `ROF=`, unless the round ends here — the last shot, the counter
/// reaching `Strafing.Shots` (equality, `:217`), or `Strafing.UseAmmoPerShot`
/// with Ammo 0 (`:218`) — in which case `Strafe_TargetCell` clears, the state
/// becomes `AirAttackStatus::FlyToPosition` (VERA 3) and the delay is
/// `Strafing.EndDelay` or `(Range + 4 * LeptonsPerCell) / Speed` (`:221`).
fn shot_delay(
    facts: &StrikeFacts,
    host: &mut impl StrikeHost,
    base: u8,
    is_last: bool,
) -> (u8, i32) {
    if is_last
        || host.strafe_counter() == facts.strafing.shots
        || (facts.strafing.use_ammo_per_shot && host.ammo() == 0)
    {
        host.set_strafe_cell(None);
        let delay = facts
            .strafing
            .end_delay
            .unwrap_or_else(|| state9_delay(facts.weapon0_range, facts.speed));
        (3, delay)
    } else {
        (base, facts.weapon0_rof)
    }
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

/// The Target test each strike state (4..9) opens with: no Target leaves for
/// state 10, and state 4 (`0x004182A3`) also leaves on Ammo 0.
pub(crate) fn strike_leaves(state: u8, target: bool, ammo: i32) -> bool {
    !target || (state == 4 && ammo == 0)
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
    if strike_leaves(facts.state, facts.target, facts.ammo) {
        return Visit::to(10, 1);
    }
    match facts.state {
        4 => {
            // `0x004182D3..0x0041830C`: a strafer does not turn to aim.
            if !facts.strafe {
                host.face_target();
            }
            match host.fire_error() {
                // `0x004184C2`: the suffix after the burst, whose delay and
                // round-end decision are Phobos's `GetDelay`
                // (`Ext/Aircraft/Hooks.cpp:233-241`).
                Ok => {
                    host.release();
                    if facts.strafe {
                        if facts.strafing.target_cell {
                            host.store_target_cell();
                        }
                        let (state, delay) = shot_delay(facts, host, 6, false);
                        Visit {
                            state,
                            delay,
                            latch: Some(true),
                        }
                    } else if facts.fighter {
                        let base = if host.ammo() > 0 { 1 } else { 10 };
                        let (state, delay) = shot_delay(facts, host, base, false);
                        Visit {
                            state,
                            delay,
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
                    if facts.firing_force_scatter {
                        host.scatter();
                    }
                    let state = if host.ammo() == 0 { 10 } else { shuffle };
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
            // Phobos `AircraftClass_Mission_Attack_StrafeCell`
            // (`Ext/Aircraft/Hooks.cpp:245-266`, hooks `0x41879D`/`0x4188AC`/
            // `0x4189BB`): with a stored target cell the shot goes at that
            // cell. GetFireError(cell, index, true) first; OK, FACING,
            // CLOAKED and RANGE fire, anything else polls (`:262`).
            if let Some(cell) = host.strafe_cell() {
                match host.cell_fire_error(cell) {
                    Ok | Facing | Cloaked | Range => {}
                    _ => return strafe_poll(facts),
                }
                host.fire_at_cell(cell);
                // `Ext/Aircraft/Hooks.cpp:257`: the fired-at cell scatters,
                // superseding the stock Scatter the `FiringForceScatter`
                // hooks now gate.
                if facts.firing_force_scatter {
                    host.scatter_at_cell(cell);
                }
                host.assign_cell_destination(cell);
                let next = facts.state + 1;
                let (state, delay) = shot_delay(facts, host, next, false);
                return Visit::to(state, delay);
            }
            match host.fire_error() {
                Ok | Facing | Cloaked => {}
                Range => host.assign_target_destination(),
                _ => return strafe_poll(facts),
            }
            host.fire_at();
            if facts.firing_force_scatter {
                host.scatter();
            }
            host.assign_target_destination();
            // `Ext/Aircraft/Hooks.cpp:255-266` (Delay2..4, `0x418883`/
            // `0x418992`/`0x418AA1`): the next strafe state, then `GetDelay`.
            let (state, delay) = shot_delay(facts, host, facts.state + 1, false);
            Visit::to(state, delay)
        }
        // `0x00418B1F`: the last bomb; state 3 consumes the pass's ammo.
        9 => {
            if let Some(cell) = host.strafe_cell() {
                match host.cell_fire_error(cell) {
                    Ok | Facing | Cloaked | Range => {}
                    _ => return strafe_poll(facts),
                }
                host.fire_at_cell(cell);
                if facts.firing_force_scatter {
                    host.scatter_at_cell(cell);
                }
                host.assign_cell_destination(cell);
                // `MissionStatus++` reaches ReturnToBase (10), then
                // `GetDelay(true)` (`Ext/Aircraft/Hooks.cpp:261-263`) moves to
                // FlyToPosition (3) with the end delay.
                let (_, delay) = shot_delay(facts, host, 3, true);
                return Visit::to(3, delay);
            }
            match host.fire_error() {
                Ok | Facing | Range | Cloaked => {
                    host.fire_at();
                    if facts.firing_force_scatter {
                        host.scatter();
                    }
                    // `GetDelay(true)` (`Ext/Aircraft/Hooks.cpp:225-228`):
                    // FlyToPosition (3) with `Strafing.EndDelay` or the
                    // derived range-over-speed delay.
                    let (_, delay) = shot_delay(facts, host, 3, true);
                    Visit::to(3, delay)
                }
                _ => strafe_poll(facts),
            }
        }
        _ => unreachable!("strike_visit runs states 4..9"),
    }
}

/// `0x00418B8A`: `(Range + 0x400) / Type+0x678`, CDQ and signed IDIV, the
/// sum wrapping. Phobos's derived `Strafing.EndDelay` uses the same
/// `(Range + 4 * LeptonsPerCell) / Speed` (`Ext/Aircraft/Hooks.cpp:221`).
/// RESIDUAL: a zero speed, or `INT_MIN / -1`, faults the original
/// (`0x00418BB4`); no retail aircraft reaches it, and VERA returns the
/// one-frame delay instead.
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
    /// `+0x294`, a borrowed AirstrikeClass link (Boris' MiGs).
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
