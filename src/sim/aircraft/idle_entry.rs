//! `AircraftClass::Enter_Idle_Mode @ 0x004176F0` (vt+0x484), the one owner of
//! an aircraft's idle mode. Mission_Move (state 0 without a NavCom, state 3
//! once stopped), Mission_Attack's state 10, Mission_Guard, Mission_AreaGuard
//! and Mission_Enter call it with `(0, 1)`, as do Mission_Unload once the hold
//! is empty, Fly's refused BeginLanding, the Scatter receiver
//! (`0x0041A590`), an owner change and a member leaving its team; Unlimbo
//! calls it with `(1, 1)` (`TechnoClass::Unlimbo @ 0x006F6E2A`). Both
//! arguments go only to the Foot base (`0x00417776..0x00417782`).
//!
//! - The head (`0x004176F8`): a suspended selector (vt+0x1FC, `0x005B3A10`)
//!   is restored (vt+0x1F8) and nothing else happens; a restored Patrol
//!   restarts at Mission+0xBC 0 with `+0x6D2` clear. Then Retreat, the two
//!   Paradrop and the two Spy Plane missions keep running unless an
//!   Airstrike (`+0x294`) owns the aircraft (jump table `0x00417BAC`), and a
//!   queued ParadropApproach or SpyplaneApproach commences at once
//!   (`0x00417764`). All three return 0.
//! - `FootClass::Enter_Idle_Mode @ 0x004D82B0`, whose answer is the call's.
//! - The pick (`0x00417787`): Area Guard for a computer house's unteamed armed
//!   aircraft, else Guard.
//! - The landed arm (`0x00417A38`), for an aircraft on the ground layer
//!   (vt+0x78), at or below its landing altitude (vt+0x1C8 against
//!   IFlyControl+0xC) or `MissileSpawn=` (Type `+0xD68`), whatever its layer
//!   and height. With `+0x3D4` set (`GameEntity::is_mission_only`, which
//!   Unlimbo sets on every non-Selectable missile), passengers (`+0x118`)
//!   unload, or guard in a team; a team member keeps the pick; any other
//!   aircraft drops its Target and its destination, in that order, and
//!   retreats. Without `+0x3D4` it drops the destination, then the Target,
//!   and keeps the pick.
//! - The airborne arm (`0x00417802`), for any other aircraft. With
//!   passengers, one without `+0x3D4` flies to
//!   Find_Nearest_Friendly_Airfield (`0x0041A160`) on Move; with it a team
//!   member guards and any other flies there to Unload. Without passengers:
//!   - a human house's `+0x3D4` aircraft, or a computer house's of a type
//!     without `Ammo=` (`+0x684`), leaves a team that has entered the map
//!     (`0x006EC370`) through `TeamClass::Remove_Member(this, -1, 0)`, which
//!     itself re-enters idle mode before this call goes on;
//!   - without a weapon in slot 0 (vt+0x3F8) a team member returns 0 and
//!     any other aircraft flies to its airfield on Move;
//!   - a `+0x3D4` aircraft without Ammo leaves its team, drops its Target
//!     and destination and retreats; with Ammo outside a team it hunts;
//!   - otherwise it attacks again while it has Ammo, a Target and Attack is
//!     its mission, or with Attack queued; keeps the pick off high flight
//!     (vt+0x54), while it Enters toward a NavCom, for a type without
//!     `Dock=` or as a team member outside the playfield (`+0x3D5`); and
//!     otherwise asks for a dock (vt+0x528), drops its destination and,
//!     the dock answering HELLO, flies there to Enter. Without one an
//!     AirportBound aircraft crashes (vt+0x3DC) and returns 0; any other
//!     flies to its airfield on Move.
//! - The tail (`0x00417AD4`): an armed aircraft without Ammo and out of radio
//!   contact asks for a dock, flies there and drops its Target to Enter; with
//!   none an AirportBound aircraft crashes and returns 0. Radio contact
//!   (`0x0065AE30`) makes it Enter. The mission is queued (vt+0x1E8) and
//!   commenced when ready (vt+0x200, vt+0x1EC).
//!
//! Evidence: tools/spatial_oracle/aircraft_idle_retreat.py runs the original
//! 0x004176F0 through both arms and its tail: every combination of what the
//! landed arm reads for a `MissileSpawn=` aircraft and for one on the ground
//! or at its landing altitude, armed and unarmed, the head's cases, and the
//! airborne arm's passengers, team, weapon, Ammo, Target, flight, NavCom,
//! Dock and playfield gates with each dock answer. `idle_entry_tests`
//! replays every row through [`enter_idle_mode`].
//!
//! Mission_Attack's state 10 clears `+0x6D5` around its call
//! ([`enter_idle_mode_after_attack`]). Its one reader, Aircraft vt+0x430
//! (`0x0041B9E0`), gates TechnoClass::Enter_Idle_Mode's planning-path
//! arrival (`0x00709A6B` -> `0x006385C0`).

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::sim::world::display_layers::DisplayLayer;

#[cfg(test)]
#[path = "idle_entry_tests.rs"]
mod tests;

/// What the call reads that it does not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IdleFacts {
    /// vt+0x1FC (`0x005B3A10`): a suspended selector (`+0xB0 != -1`).
    suspended: bool,
    /// `+0x294`, an AirstrikeClass.
    airstrike: bool,
    /// `+0x3D4`.
    mission_only: bool,
    /// `+0x118`, the first passenger.
    passengers: bool,
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` on the owner.
    human: bool,
    /// `TechnoClass::Is_Armed @ 0x00701120` (vt+0x2AC).
    armed: bool,
    /// `GetWeapon(0)` (vt+0x3F8, `0x0070E140`) names a WeaponType.
    weapon: bool,
    /// Ammo `+0x2FC`.
    ammo: i32,
    /// The type's `Ammo=` (`+0x684`).
    type_ammo: i32,
    /// The type's `Dock=` list is not empty.
    docks: bool,
    /// The type's `AirportBound=` (`+0xE0D`).
    airport_bound: bool,
    /// `+0x3D5`, inside the playfield.
    in_playfield: bool,
    /// The landed arm's test (`0x004177BA..0x004177FC`): the ground layer,
    /// the height at or below the landing altitude, or `MissileSpawn=`.
    landed: bool,
}

/// What the call does and reads live, each where the original does it.
/// The missions are read live: leaving a team re-enters idle mode first.
/// It supplies the reads of `AircraftClass::Enter_Idle_Mode` (`0x004176F0`).
trait IdleHost {
    /// The current mission `+0xAC`.
    fn current(&mut self) -> MissionId;
    /// The queued mission `+0xB4`.
    fn queued(&mut self) -> MissionId;
    /// vt+0x1F8 Restore (`0x004D8F80`); answers the current mission after it.
    fn restore(&mut self) -> MissionId;
    /// Mission+0xBC = 0 and `+0x6D2` = 0 (`0x00417719`).
    fn restart_patrol(&mut self);
    /// vt+0x1EC Commence (`0x0041B870`).
    fn commence(&mut self);
    /// `FootClass::Enter_Idle_Mode @ 0x004D82B0` with the caller's `(0, 1)`.
    fn foot_enter_idle(&mut self) -> bool;
    /// The team `+0x5D4` is set.
    fn team(&mut self) -> bool;
    /// `TeamClass::Has_Entered_Map @ 0x006EC370` on the team.
    fn team_entered_map(&mut self) -> bool;
    /// `TeamClass::Remove_Member(this, -1, 0)` (`0x006EA870`).
    fn leave_team(&mut self);
    /// The Target `+0x2B4` is set.
    fn target(&mut self) -> bool;
    /// The NavCom `+0x5A4` is set.
    fn nav_com(&mut self) -> bool;
    /// vt+0x54 (`0x0041B920`), high in the air.
    fn in_air(&mut self) -> bool;
    /// vt+0x3C8 `Assign_Target(NULL)` (`0x006FCDB0`).
    fn clear_target(&mut self);
    /// vt+0x480 `Assign_Destination(NULL, 1)` (`0x0041AA80`).
    fn clear_destination(&mut self);
    /// vt+0x528 `Find_Docking_Bay(&Type->Dock, 0, 0)` (`0x0041BBD0`).
    fn find_dock(&mut self) -> Option<u64>;
    /// vt+0x278 `Transmit_Message(HELLO, dock)` answers ROGER.
    fn hello(&mut self, dock: u64) -> bool;
    /// vt+0x480 `Assign_Destination(dock, 1)`.
    fn assign_dock(&mut self, dock: u64);
    /// vt+0x480 `Assign_Destination(Find_Nearest_Friendly_Airfield(), 1)`.
    fn assign_airfield(&mut self);
    /// vt+0x3DC `Crash(NULL)` (`0x004DEBB0`).
    fn crash(&mut self);
    /// `RadioClass::In_Radio_Contact @ 0x0065AE30`.
    fn in_radio_contact(&mut self) -> bool;
    /// vt+0x1E8 `Queue_Mission(mission, 0)` (`0x0041BA90`).
    fn queue(&mut self, mission: MissionType);
    /// vt+0x200 `Ready_To_Commence` (`0x0041B5E0`).
    fn ready(&mut self) -> bool;
}

/// The call and its answer: the Foot base's, or 0 where the head returns, a
/// team member without a weapon returns or an AirportBound aircraft crashes.
fn enter_idle_mode(facts: &IdleFacts, host: &mut impl IdleHost) -> bool {
    use MissionType::{
        AreaGuard, Enter, Guard, ParadropApproach, ParadropOverfly, Patrol, Retreat,
        SpyplaneApproach, SpyplaneOverfly, Unload,
    };
    if facts.suspended {
        if host.restore().known() == Some(Patrol) {
            host.restart_patrol();
        }
        return false;
    }
    if matches!(
        host.current().known(),
        Some(Retreat | ParadropApproach | ParadropOverfly | SpyplaneApproach | SpyplaneOverfly)
    ) && !facts.airstrike
    {
        return false;
    }
    if matches!(
        host.queued().known(),
        Some(ParadropApproach | SpyplaneApproach)
    ) {
        host.commence();
        return false;
    }
    let answer = host.foot_enter_idle();
    // `0x00417787`, read again by the landed arm without `+0x3D4`
    // (`0x00417AA3`): no call in between changes the team.
    let pick = if !facts.human && !host.team() && facts.armed {
        AreaGuard
    } else {
        Guard
    };
    let mission = if facts.landed {
        if !facts.mission_only {
            host.clear_destination();
            host.clear_target();
            pick
        } else if facts.passengers {
            if host.team() { Guard } else { Unload }
        } else if host.team() {
            pick
        } else {
            host.clear_target();
            host.clear_destination();
            Retreat
        }
    } else {
        match airborne_arm(facts, pick, host) {
            Some(mission) => mission,
            None => return false,
        }
    };
    let mut mission = mission;
    if facts.ammo == 0 && facts.armed && !host.in_radio_contact() {
        match host.find_dock() {
            Some(dock) => {
                host.assign_dock(dock);
                host.clear_target();
                mission = Enter;
            }
            None if facts.airport_bound => {
                host.crash();
                return false;
            }
            None => {}
        }
    }
    if host.in_radio_contact() {
        mission = Enter;
    }
    host.queue(mission);
    if host.ready() {
        host.commence();
    }
    answer
}

/// The airborne arm (`0x00417802..0x00417A33`): the mission it leaves for
/// the tail, or `None` where the call returns 0.
fn airborne_arm(
    facts: &IdleFacts,
    pick: MissionType,
    host: &mut impl IdleHost,
) -> Option<MissionType> {
    use MissionType::{Attack, Enter, Guard, Hunt, Move, Retreat, Unload};
    if facts.passengers {
        if facts.mission_only {
            if host.team() {
                return Some(Guard);
            }
            host.assign_airfield();
            return Some(Unload);
        }
        host.assign_airfield();
        return Some(Move);
    }
    if (facts.mission_only && facts.human || !facts.human && facts.type_ammo == 0)
        && host.team()
        && host.team_entered_map()
    {
        host.leave_team();
    }
    if !facts.weapon {
        if host.team() {
            return None;
        }
        host.assign_airfield();
        return Some(Move);
    }
    if facts.mission_only {
        if facts.ammo == 0 {
            if host.team() {
                host.leave_team();
            }
            host.clear_target();
            host.clear_destination();
            return Some(Retreat);
        }
        return Some(if host.team() { pick } else { Hunt });
    }
    // `Get_Mission` (vt+0x184): the current mission, else the queued one.
    let (current, queued) = (host.current(), host.queued());
    let mission = if current == MissionId::NONE {
        queued
    } else {
        current
    };
    let attack = MissionId::from_known(Attack);
    if facts.ammo != 0 && host.target() && mission == attack || queued == attack {
        return Some(Attack);
    }
    if !host.in_air()
        || host.nav_com() && host.current() == MissionId::from_known(Enter)
        || !facts.docks
        || !facts.in_playfield && host.team()
    {
        return Some(pick);
    }
    let dock = host.find_dock();
    host.clear_destination();
    if let Some(dock) = dock
        && host.hello(dock)
    {
        host.assign_dock(dock);
        return Some(Enter);
    }
    if facts.airport_bound {
        host.crash();
        return None;
    }
    host.assign_airfield();
    Some(Move)
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// While set, [`enter_idle_mode_for`] only counts its calls, as oracles
    /// that stub Enter_Idle_Mode record them ([`record_idle_calls_for_test`]).
    static RECORDED_CALLS: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Runs `run` with [`enter_idle_mode_for`] recording its calls instead of
/// running; answers `run`'s result and the number of calls.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn record_idle_calls_for_test<R>(run: impl FnOnce() -> R) -> (R, u32) {
    RECORDED_CALLS.with(|calls| calls.set(Some(0)));
    let result = run();
    let calls = RECORDED_CALLS.with(|calls| calls.take()).unwrap_or(0);
    (result, calls)
}

/// [`enter_idle_mode`] for aircraft `id`: `AircraftClass::Enter_Idle_Mode`
/// (`0x004176F0`), with the [`IdleFacts`] it reads up front.
pub(crate) fn enter_idle_mode_for(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    enter_idle_mode_with(sim, id, rules, registry, frame_effects, true);
}

/// Mission_Attack state 10's Enter_Idle_Mode (`0x00418CFA`): the state clears
/// Aircraft `+0x6D5` before it (`0x00418C9B`) and sets it again after
/// (`0x00418D00`). The constructor writes 1 (`0x00413D81`) and no other code
/// clears it, so this call alone sees it clear: its planning-path arrival
/// (Aircraft vt+0x430 `0x0041B9E0`, [`Simulation::planning_enter_idle`])
/// starts no node.
pub(crate) fn enter_idle_mode_after_attack(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    enter_idle_mode_with(sim, id, rules, registry, frame_effects, false);
}

/// `planning_arrival` is Aircraft `+0x6D5` for this call.
fn enter_idle_mode_with(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
    planning_arrival: bool,
) {
    #[cfg(any(test, feature = "test-support"))]
    if RECORDED_CALLS
        .with(|calls| calls.get().map(|n| calls.set(Some(n + 1))))
        .is_some()
    {
        return;
    }
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let Some(object) = sim.entity_object_type(entity, rules) else {
        return;
    };
    let landing = super::landing_base::landing_base(
        entity,
        &sim.substrate.entities,
        Some((rules, &sim.interner)),
    );
    let landed = sim.entity_display_layer(id, rules) == Some(DisplayLayer::GROUND)
        || crate::sim::movement::air_movement::current_fly_height(
            entity,
            sim.resolved_terrain.as_ref(),
        ) <= landing
        || object.missile_spawn;
    let facts = IdleFacts {
        suspended: entity.mission.suspended() != MissionId::NONE,
        airstrike: entity.airstrike_link().is_some(),
        mission_only: entity.is_mission_only(),
        passengers: entity
            .passenger_role
            .cargo()
            .is_some_and(|cargo| cargo.count() != 0),
        human: sim.owner_is_human(entity.owner()),
        armed: crate::sim::combat::combat_weapon::is_armed(entity, object),
        weapon: crate::sim::combat::combat_weapon::primary_for_tier(
            object,
            entity.veterancy.rank(),
        )
        .is_some(),
        ammo: entity.ammo().map_or(-1, |ammo| ammo.current()),
        type_ammo: object.ammo,
        docks: !object.dock.is_empty(),
        airport_bound: object.airport_bound,
        in_playfield: entity.in_playfield(),
        landed,
    };
    enter_idle_mode(
        &facts,
        &mut WorldIdle {
            sim,
            id,
            rules,
            registry,
            frame_effects,
            planning_arrival,
        },
    );
}

struct WorldIdle<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    registry: Option<&'a OverlayTypeRegistry>,
    frame_effects: FrameEffects<'a>,
    /// Aircraft `+0x6D5` ([`enter_idle_mode_after_attack`]).
    planning_arrival: bool,
}

impl WorldIdle<'_> {
    fn entity(&self) -> &crate::sim::game_entity::GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .expect("idle-mode aircraft")
    }

    fn assign_destination(&mut self, destination: Option<NavTargetRef>) {
        self.sim
            .assign_aircraft_destination(self.id, destination, self.rules, self.frame_effects);
    }
}

impl IdleHost for WorldIdle<'_> {
    fn current(&mut self) -> MissionId {
        self.entity().mission.current()
    }

    fn queued(&mut self) -> MissionId {
        self.entity().mission.queued()
    }

    fn restore(&mut self) -> MissionId {
        self.sim
            .mission_restore_represented(self.id, self.rules, self.registry, self.frame_effects)
            .expect("Restore setters accept a live aircraft's archived Target and NavCom");
        self.entity().mission.current()
    }

    fn restart_patrol(&mut self) {
        super::restart_patrol(self.sim.substrate.entities.get_mut(self.id).unwrap());
    }

    fn commence(&mut self) {
        let now = self.sim.session.binary_frame();
        self.sim
            .substrate
            .mission_commence_exact(self.id, now)
            .expect("idle-mode aircraft");
    }

    fn foot_enter_idle(&mut self) -> bool {
        self.sim.foot_enter_idle_base_with(
            self.id,
            self.rules,
            self.registry,
            self.frame_effects,
            self.planning_arrival,
        )
    }

    fn team(&mut self) -> bool {
        self.sim.team_script_vm.team_for_member(self.id).is_some()
    }

    fn team_entered_map(&mut self) -> bool {
        let (team, _) = self
            .sim
            .team_script_vm
            .team_for_member(self.id)
            .expect("asked of a team member");
        self.sim.team_has_entered_map(team)
    }

    fn leave_team(&mut self) {
        self.sim
            .leave_team(self.id, false, self.rules, self.frame_effects);
    }

    fn target(&mut self) -> bool {
        crate::sim::combat::target_present(self.entity().attack_target.target(), self.sim)
    }

    fn nav_com(&mut self) -> bool {
        self.entity().navigation.nav_com().is_some()
    }

    fn in_air(&mut self) -> bool {
        crate::sim::movement::air_movement::is_high_flying(
            self.entity(),
            self.sim.resolved_terrain.as_ref(),
            Some((self.rules, &self.sim.interner)),
        )
    }

    fn clear_target(&mut self) {
        self.sim
            .assign_target_represented(self.id, None, self.rules, None, FrameEffects::default())
            .expect("idle-mode aircraft");
    }

    fn clear_destination(&mut self) {
        self.assign_destination(None);
    }

    fn find_dock(&mut self) -> Option<u64> {
        self.sim
            .aircraft_find_docking_bay(self.id, self.rules, self.frame_effects)
    }

    fn hello(&mut self, dock: u64) -> bool {
        radio::transmit(
            self.sim,
            self.id,
            dock,
            RadioMessage::Hello,
            RadioPayload::default(),
            self.rules,
            self.frame_effects,
        ) == RadioResponse::Roger
    }

    fn assign_dock(&mut self, dock: u64) {
        self.assign_destination(Some(NavTargetRef::object(dock)));
    }

    fn assign_airfield(&mut self) {
        let airfield = self
            .sim
            .aircraft_nearest_friendly_airfield_cell(self.id, self.rules);
        self.assign_destination(Some(airfield));
    }

    fn crash(&mut self) {
        self.sim
            .foot_crash(self.id, None, self.rules, self.registry, self.frame_effects);
    }

    fn in_radio_contact(&mut self) -> bool {
        !self.entity().radio_contacts.is_empty()
    }

    fn queue(&mut self, mission: MissionType) {
        super::queue_mission(&mut self.sim.substrate, self.id, mission);
    }

    fn ready(&mut self) -> bool {
        self.sim.mission_ready_to_commence(self.id, self.rules)
    }
}
