//! The Carryall: an AircraftType with `Carryall=` (`+0xDFC`, read at
//! `0x0041CC9B..0x0041CCC7`) picks up an allied vehicle, carries it and sets
//! it down. Each step, where gamemd runs it:
//! - The order: a human player's Carryall with an empty hold over an allied
//!   Unit answers Tote (0x11, `AircraftClass::What_Action_OnObject
//!   @ 0x00417CC0`, [`Simulation::carryall_tote_action`]), which its
//!   ClickedAction (`0x00417BD0`) turns into Move with the Unit as its
//!   destination (`Player_Assign_Mission(Move, NULL, unit)`).
//! - Mission_Move (vt+0x22C = `0x004166C0`) runs `Mission_Move_Carryall @
//!   0x00416D50` for a Carryall ([`move_visit`]): state 0 radios the Unit
//!   (HELLO, WANT_RIDE 0x24, HOLD_STILL 7), whose answers stop it, put it to
//!   Sleep and tether it (Unit `0x0073745C`, `0x0073750A`); states 1 and 2
//!   fly over it; state 3 lands, takes the Unit below aboard
//!   ([`Simulation::carryall_pick_up`]) or, loaded, sets its cargo down.
//! - While carried, the cargo follows the Carryall's turret facing and its
//!   Location every AI frame ([`Simulation::carryall_carry_cargo`]).
//! - The drop, `Carryall_Unload @ 0x00416AF0` ([`Simulation::carryall_unload`]),
//!   from Mission_Move state 3, Mission_Unload state 3 (`0x004154E7`) and
//!   Mission_Enter's dock arm (`0x0041A059..0x0041A124`).
//! - Release before the pickup: the waiting Unit's setter given another
//!   destination (`0x00741B28..0x00741BC5`) clears the Carryall's
//!   destination and breaks the tether; message 8 from the cargo or the
//!   NavCom is refused while the Carryall moves to it (`0x004194C5`).
//!   After it, the cargo is a passenger: the carrier's death kills it
//!   (KillPassengers), and its own expiry leaves the hold.
//!
//! The Fly landing reads the Carryall too: the 100-lepton landing base over
//! cargo or a contact (`landing_base`), CARYLAND (`world::fly_landing`) and
//! the Unit NavCom left out of the landing claims (`aircraft_fire_location`).
//!
//! No AI or team script orders a tote: gamemd reads `+0xDFC` only in the
//! AircraftClass functions above and FlyLocomotion's landing (`0x004CEC14`).
//!
//! The client draws the cargo under its Carryall
//! ([`Simulation::carryall_drawn_cargo`], `app::presentation::instances::units`).
//!
//! Ares 3.0 gates both Tote rungs with `Carryall.Allowed`/`Carryall.SizeLimit`
//! ([`Simulation::ares_carryall_can_lift`]).

use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::passenger::{DepartureFailure, DepartureRoute, depart_cargo_head};
use crate::sim::radio::{RadioMessage, RadioResponse};
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation, UninitContext};

/// `FootClass+0x424` IsOnCarryall: the cargo a Carryall took aboard
/// (`0x00417297`) until its drop (`0x00416C66`) or an ordinary aircraft
/// unload (`0x00415548`) clears it; the constructor clears it (`0x006F2FF3`).
/// The aircraft unload's team gate reads it (`0x0041556A`), and the Unit and
/// Techno drawing (`0x0073C36C`, `0x00706481`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct OnCarryall(bool);

impl OnCarryall {
    /// Whether a Carryall carries the object.
    pub const fn get(self) -> bool {
        self.0
    }

    pub(crate) fn set(&mut self, carried: bool) {
        self.0 = carried;
    }
}

/// What a Carryall's What_Action makes of an allied Unit
/// ([`Simulation::carryall_tote_action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryallTote {
    /// Tote (0x11), which its ClickedAction (`0x00417BD0`) sends as Move with
    /// the Unit as destination.
    Tote,
    /// 0: the arrow, and a click that orders nothing.
    Nothing,
    /// Ares: the base action, without its cell rung.
    Kept,
}

/// What a `Mission_Move_Carryall @ 0x00416D50` visit reads and does, each
/// where the original does it.
pub(crate) trait CarryallMoveHost {
    /// NavCom (`+0x5A4`).
    fn nav_com(&mut self) -> Option<NavTargetRef>;
    /// The NavCom is a Unit (What_Am_I 1).
    fn nav_is_unit(&mut self, id: u64) -> bool;
    /// vt+0x484 `Enter_Idle_Mode(0, 1)`.
    fn enter_idle_mode(&mut self);
    /// State 0's tote rung (`0x00416DD3..0x00416E31`): the NavCom as a Techno
    /// (`As_Techno @ 0x0040DD70`) while the hold is empty (`+0x118`), the
    /// owner allied with it and its house allied with this Carryall
    /// (`0x004F9A90` both ways), and a Unit. Ares adds CarryallCanLift.
    fn tote_target(&mut self) -> Option<u64>;
    /// `Contact_With_Whom(0)` (`0x0065AD30`).
    fn contact(&mut self) -> Option<u64>;
    /// vt+0x274 `Transmit_Message(message)` to contact 0.
    fn transmit_to_contact(&mut self, message: RadioMessage) -> RadioResponse;
    /// vt+0x278 `Transmit_Message(message, to)`.
    fn transmit(&mut self, message: RadioMessage, to: u64) -> RadioResponse;
    /// vt+0x480 `Assign_Destination(NULL, 1)`.
    fn clear_destination(&mut self);
    /// vt+0x480 `Assign_Destination(Find_Attack_Cell(NavCom), 1)`, then the
    /// team's `Assign_Mission_Target(NavCom)` (`0x006E9050`).
    fn assign_attack_cell(&mut self);
    /// The locomotor's `Move_To` the NavCom's center (vt+0x48).
    fn move_to_nav_center(&mut self);
    /// `FootClass::IsLandZoneClear` (vt+0x550) of the NavCom.
    fn land_zone_clear(&mut self) -> bool;
    /// The locomotor's Get_Status (`+0x90`) is 1, descending.
    fn descending(&mut self) -> bool;
    /// The NavCom's Location (`+0x9C`) is in this aircraft's Location cell.
    fn nav_in_own_cell(&mut self) -> bool;
    /// `+0x6D4`.
    fn set_transition_ready(&mut self, ready: bool);
    /// The locomotor's Is_Moving (`+0x10`).
    fn is_moving(&mut self) -> bool;
    /// The hold has a first passenger (`+0x118`).
    fn loaded(&mut self) -> bool;
    /// A pending entry (`+0x500`).
    fn pending_entry(&mut self) -> bool;
    /// vt+0x124 Mark: DOWN when `down`, else UP.
    fn mark(&mut self, down: bool);
    /// `Carryall_Unload @ 0x00416AF0`.
    fn unload(&mut self);
    /// `CellClass::FindFirstUnit` of the cell at the Location, on its bridge
    /// list when the cell has a bridge (`+0x140 & 0x100`).
    fn unit_below(&mut self) -> Option<u64>;
    /// The pickup (`0x00417275..0x004172A6`).
    fn pick_up(&mut self, unit: u64);
    /// The Rate epilogue on the mission current now (`0x00416D9D`).
    fn epilogue(&mut self) -> i32;
}

/// One `Mission_Move_Carryall @ 0x00416D50` visit from `state`
/// (Mission+0xBC): the state it leaves and the frames it returns.
/// - State 0: without a NavCom, idle mode. A tote target is radioed: a
///   contact other than it is BROKEN off; a HELLO it does not answer ROGER
///   drops the destination and enters idle mode; a WANT_RIDE it does not
///   answer ROGER BREAKs it and enters idle mode; otherwise it is told
///   HOLD_STILL. Then the destination becomes Find_Attack_Cell of the NavCom,
///   the team takes it as its mission target, and the state is 1. Each arm
///   returns the Rate epilogue.
/// - State 1 (`0x00416F29`): without a NavCom idle mode; otherwise Move_To
///   the NavCom's center and state 2. Returns 1.
/// - State 2 (`0x00416F82`): an object NavCom no longer contact 0, or a Cell
///   NavCom whose landing zone is not clear, restarts at 0. Descending, a
///   Unit NavCom that left this cell restarts at 0; otherwise `+0x6D4`
///   clears. A stopped locomotor is state 3. Returns 1.
/// - State 3 (`0x004170DD`): loaded and awaiting an entry, `+0x6D4` is set;
///   loaded otherwise, Mark(UP), the drop, Mark(DOWN) and state 0. Empty, with
///   Mark(UP) held: the Unit below, contact 0 answering NEED_TO_MOVE ROGER,
///   is taken aboard (the state stays 3); otherwise state 0. Then BREAK,
///   Mark(DOWN), `+0x6D4` and idle mode. Returns 1.
/// - A state past 3 returns the epilogue.
pub(crate) fn move_visit(state: u8, host: &mut impl CarryallMoveHost) -> (u8, i32) {
    match state {
        0 => {
            if host.nav_com().is_none() {
                host.enter_idle_mode();
                return (0, host.epilogue());
            }
            if let Some(unit) = host.tote_target() {
                if host.contact() != Some(unit) {
                    host.transmit_to_contact(RadioMessage::Break);
                }
                if host.transmit(RadioMessage::Hello, unit) != RadioResponse::Roger {
                    host.clear_destination();
                    host.enter_idle_mode();
                    return (0, host.epilogue());
                }
                if host.transmit_to_contact(RadioMessage::WantRide) != RadioResponse::Roger {
                    host.transmit_to_contact(RadioMessage::Break);
                    host.enter_idle_mode();
                    return (0, host.epilogue());
                }
                host.transmit_to_contact(RadioMessage::HoldStill);
            }
            host.assign_attack_cell();
            (1, host.epilogue())
        }
        1 => {
            if host.nav_com().is_none() {
                host.enter_idle_mode();
                return (1, 1);
            }
            host.move_to_nav_center();
            (2, 1)
        }
        2 => {
            let nav = host.nav_com();
            match nav {
                Some(NavTargetRef::Object { id }) if host.contact() != Some(id) => return (0, 1),
                Some(NavTargetRef::Cell { .. }) if !host.land_zone_clear() => return (0, 1),
                _ => {}
            }
            if host.descending() {
                if let Some(NavTargetRef::Object { id }) = nav
                    && host.nav_is_unit(id)
                    && !host.nav_in_own_cell()
                {
                    return (0, 1);
                }
                host.set_transition_ready(false);
            }
            if !host.is_moving() {
                return (3, 1);
            }
            (2, 1)
        }
        3 => {
            if host.loaded() {
                if host.pending_entry() {
                    host.set_transition_ready(true);
                    return (3, 1);
                }
                host.mark(false);
                host.unload();
                host.mark(true);
                return (0, 1);
            }
            host.mark(false);
            let next = match host.unit_below() {
                Some(unit)
                    if host.contact() == Some(unit)
                        && host.transmit_to_contact(RadioMessage::NeedToMove)
                            == RadioResponse::Roger =>
                {
                    host.pick_up(unit);
                    3
                }
                _ => 0,
            };
            host.transmit_to_contact(RadioMessage::Break);
            host.mark(true);
            host.set_transition_ready(true);
            host.enter_idle_mode();
            (next, 1)
        }
        state => (state, host.epilogue()),
    }
}

impl Simulation {
    /// Whether `aircraft` is a `Carryall=` AircraftType (`+0xDFC`).
    pub(crate) fn is_carryall(&self, aircraft: u64, rules: &RuleSet) -> bool {
        self.substrate.entities.get(aircraft).is_some_and(|entity| {
            entity.category() == crate::map::entities::EntityCategory::Aircraft
                && self
                    .entity_object_type(entity, rules)
                    .is_some_and(|object| object.carryall)
        })
    }

    /// The Tote rung of `AircraftClass::What_Action_OnObject @ 0x00417CC0`
    /// for `aircraft` over `target`, asked with the base action Select (7)
    /// or 0 (`0x00417D06..0x00417D75`): a human player's (`0x0050B6F0`)
    /// `Carryall=` type whose owner is allied with the target
    /// (`0x004F9A90`) and the target's house with it, its hold empty
    /// (`+0x118`), over a Unit (What_Am_I 1). `None` leaves the base action
    /// to the rows after it.
    ///
    /// Tote then becomes 0 ([`CarryallTote::Nothing`],
    /// `0x00417E7D..0x00417EF3`) for a Unit whose cell (vt+0x1B8, a real
    /// one) has a `WeaponsFactory=` first Building (`+0x16BD`): one still
    /// leaving its factory.
    ///
    /// Ares 3.0 AircraftClass_GetActionOnObject_CanTote (10048A60, hook
    /// 0x417D75) keeps the base action when CarryallCanLift refuses
    /// ([`Self::ares_carryall_can_lift`]), past the base action's cell rung
    /// (0x417DF6, [`CarryallTote::Kept`]).
    pub fn carryall_tote_action(
        &self,
        aircraft: u64,
        target: u64,
        rules: &RuleSet,
    ) -> Option<CarryallTote> {
        let carrier = self.substrate.entities.get(aircraft)?;
        let unit = self.substrate.entities.get(target)?;
        if !self.is_carryall(aircraft, rules)
            || !self.house_is_human_player(carrier.owner())
            || !self.mutually_allied(carrier.owner(), unit.owner())
            || carrier.passenger_role.passenger_count() != 0
            || unit.category() != crate::map::entities::EntityCategory::Unit
        {
            return None;
        }
        if rules.ares_form() && !self.ares_carryall_can_lift(aircraft, target, rules) {
            return Some(CarryallTote::Kept);
        }
        let factory = self
            .substrate
            .occupancy
            .first_building_on_layer(
                unit.position.rx(),
                unit.position.ry(),
                crate::sim::movement::locomotor::MovementLayer::Ground,
            )
            .is_some_and(|building| self.is_weapons_factory(building, rules));
        Some(if factory {
            CarryallTote::Nothing
        } else {
            CarryallTote::Tote
        })
    }

    /// Ares 3.0 `TechnoTypeExt::ExtData::CarryallCanLift @ 1003D9E0`, of
    /// `aircraft`'s type for the Unit `target`: no parasite eating it
    /// (`+0x694`); its type's `Carryall.Allowed` (+0x2F8) when set, else
    /// neither `Organic=` (`+0xD97`) nor `NonVehicle=` (`+0xE1B`); and with
    /// the Carryall's `Carryall.SizeLimit` (+0x2FC) set and not negative, the
    /// target's `Size=` (`+0x380`, a double) not above it.
    pub(crate) fn ares_carryall_can_lift(
        &self,
        aircraft: u64,
        target: u64,
        rules: &RuleSet,
    ) -> bool {
        let (Some(carrier), Some(unit)) = (
            self.substrate.entities.get(aircraft),
            self.substrate.entities.get(target),
        ) else {
            return false;
        };
        let (Some(carrier_type), Some(unit_type)) = (
            self.entity_object_type(carrier, rules),
            self.entity_object_type(unit, rules),
        ) else {
            return false;
        };
        if unit.parasite.eater().is_some() {
            return false;
        }
        let liftable = unit_type
            .techno_combat()
            .carryall_allowed()
            .unwrap_or(!unit_type.organic && !unit_type.non_vehicle);
        if !liftable {
            return false;
        }
        match carrier_type.techno_combat().carryall_size_limit() {
            Some(limit) if limit >= 0 => f64::from(limit) >= unit_type.size,
            _ => true,
        }
    }

    /// Both `HouseClass::Is_Ally_ByObject @ 0x004F9A90` tests a tote makes:
    /// `a` allied with `b` and `b` with `a`.
    fn mutually_allied(
        &self,
        a: crate::sim::intern::InternedId,
        b: crate::sim::intern::InternedId,
    ) -> bool {
        let allied = |x, y| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                x,
                y,
            )
        };
        allied(a, b) && allied(b, a)
    }

    /// The cargo `AircraftClass::Draw_It @ 0x004144B0` draws before its own
    /// shadow and body (`0x0041461D..0x00414653`): a `Carryall=` aircraft's
    /// first passenger (`+0x118`).
    pub fn carryall_drawn_cargo(&self, aircraft: u64, rules: &RuleSet) -> Option<u64> {
        if !self.is_carryall(aircraft, rules) {
            return None;
        }
        let hold = self
            .substrate
            .entities
            .get(aircraft)?
            .passenger_role
            .cargo()?;
        hold.passengers().first().copied()
    }

    /// Mission_Move_Carryall's tote rung ([`CarryallMoveHost::tote_target`]).
    pub(crate) fn carryall_tote_target(&self, aircraft: u64, rules: &RuleSet) -> Option<u64> {
        let carrier = self.substrate.entities.get(aircraft)?;
        let Some(NavTargetRef::Object { id }) = carrier.navigation.nav_com() else {
            return None;
        };
        let target = crate::sim::radio::as_techno(self, id)?;
        let unit = self.substrate.entities.get(target)?;
        if carrier.passenger_role.passenger_count() != 0
            || !self.mutually_allied(carrier.owner(), unit.owner())
            || unit.category() != crate::map::entities::EntityCategory::Unit
        {
            return None;
        }
        // Ares 3.0 AircraftClass_Mi_MoveCarryall_CanTote (10048B00, hook
        // 0x416E37): a Unit it cannot lift is flown to without the radio
        // exchange (0x416EC9).
        if rules.ares_form() && !self.ares_carryall_can_lift(aircraft, target, rules) {
            return None;
        }
        Some(target)
    }

    /// State 3's pickup (`0x00417275..0x004172A6`): the Unit's Limbo
    /// (vt+0xD4, which BREAKs its contacts), OnBridge (`+0x8C`) cleared,
    /// IsOnCarryall (`+0x424`) set and `CargoClass::AddPassenger @
    /// 0x004733A0` into the Carryall's hold, which writes neither Transporter
    /// (`+0x11C`) nor `+0x82`.
    pub(crate) fn carryall_pick_up(
        &mut self,
        aircraft: u64,
        unit: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let _ = self.techno_limbo_with_context(unit, context);
        let Some(type_ref) = self.substrate.entities.get_mut(unit).map(|cargo| {
            cargo.on_bridge.store(false);
            cargo.on_carryall.set(true);
            cargo.passenger_role.set_inside_transport(aircraft, false);
            cargo.type_ref()
        }) else {
            return;
        };
        let size = self
            .object_type(type_ref, rules)
            .map_or(1.0, |object| object.size);
        let _ = self.add_cargo_passenger_chain(aircraft, &[(unit, size)], context);
    }

    /// `AircraftClass::AI`'s Carryall block (`0x00415136..0x00415184`), after
    /// the damage smoke: a `Carryall=` aircraft's first passenger takes the
    /// aircraft's turret facing (SecondaryFacing `+0x3A0`, the whole
    /// FacingClass, its rate included) as its body facing (`+0x388`) and its
    /// turret facing, then the aircraft's Location (SetLocation vt+0x1B4).
    ///
    /// A cargo without a turret facing in VERA ([`GameEntity::barrel_facing`])
    /// takes only the body copy; nothing reads its `+0x3A0`.
    ///
    /// [`GameEntity::barrel_facing`]: crate::sim::game_entity::GameEntity::barrel_facing
    pub(crate) fn carryall_carry_cargo(
        &mut self,
        aircraft: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // `0x00414DAA`: an aircraft its own FootClass::AI removed never gets
        // here.
        if !self.is_carryall(aircraft, rules) {
            return;
        }
        let Some(carrier) = self
            .substrate
            .entities
            .get(aircraft)
            .filter(|carrier| carrier.lifecycle.object_alive())
        else {
            return;
        };
        let Some(&cargo) = carrier
            .passenger_role
            .cargo()
            .and_then(|hold| hold.passengers().first())
        else {
            return;
        };
        let facing = carrier.barrel_facing;
        let location = crate::sim::movement::ground_pose::object_location(
            carrier,
            self.resolved_terrain.as_ref(),
        );
        if let Some(facing) = facing
            && let Some(cargo) = self.substrate.entities.get_mut(cargo)
        {
            cargo.body_facing = facing;
            if cargo.barrel_facing.is_some() {
                cargo.barrel_facing = Some(facing);
            }
        }
        self.foot_set_location_marked(cargo, location, rules, registry, effects);
    }

    /// `AircraftClass::Carryall_Unload @ 0x00416AF0` (the database calls it
    /// Carryall_Pickup): the hold's head comes off (`CargoClass::
    /// RemoveFirstPassenger @ 0x00473430`) and is put down under the
    /// aircraft:
    /// - at the ground height under the aircraft's Location (`0x00578080`),
    ///   plus the bridge deck and OnBridge (`+0x8C`) set over a cell with a
    ///   bridge (`+0x140 & 0x100`), OnBridge cleared otherwise;
    /// - with a new locomotor of its type's `Locomotor=` (`+0x34C`,
    ///   `0x0041C250`), linked to it, in place of the old one;
    /// - through Unlimbo (vt+0xD8) facing north (0). A refused one goes back
    ///   to the hold's head (`0x00416C4E`) and nothing else happens.
    ///
    /// Once down: IsOnCarryall cleared, ReReveal and UpdateReveal (vt+0x48C,
    /// vt+0x488) and the sight border (`0x00567DA0`). Then the aircraft UNTETHERs
    /// and BREAKs a contact 0 that is the cargo, and takes NearbyLocation
    /// (`0x00703590`) of itself as its destination, or NULL when none is found.
    ///
    /// Unlike the vehicle and aircraft unloads it neither resets the gunner
    /// nor queues a mission for the cargo, and plays no sound.
    pub(crate) fn carryall_unload(
        &mut self,
        aircraft: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let terrain = self.resolved_terrain.as_ref();
        let Some(carrier) = self.substrate.entities.get(aircraft) else {
            return;
        };
        let location = crate::sim::movement::ground_pose::object_location(carrier, terrain);
        let mut coord = location;
        coord.z = crate::sim::movement::ground_pose::ground_surface_z_at(
            [location.x, location.y],
            false,
            terrain,
            None,
        )
        .unwrap_or(0);
        let bridge = terrain.is_some_and(|terrain| {
            let cell = terrain
                .native_cell_identity(((location.x / 256) as i16, (location.y / 256) as i16));
            terrain.native_cell_flags(cell) & 0x100 != 0
        });
        if bridge {
            coord.z = coord
                .z
                .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        let requested = carrier.position.at_location_with_level(coord);
        let frame = self.session.binary_frame();
        let ares = rules.ares_form();
        // Ares 3.0 AircraftClass_Carryall_Unload_Facing (10048920, hook
        // 0x416C3A) Unlimbos the cargo facing the Carryall's turret (its
        // vt+0x2A8, SecondaryFacing's current value, `0x0041C1E0`) to the
        // nearest 1/256 turn, where stock gamemd passes 0.
        let facing = if ares {
            carrier
                .barrel_facing
                .map_or(0, |turret| (((turret.current(frame) >> 7) + 1) >> 1) as u8)
        } else {
            0
        };
        let mut dropped = None;
        let _ = depart_cargo_head(
            self,
            rules,
            registry,
            aircraft,
            DepartureRoute::Carryall,
            |sim, cargo| {
                let object = sim
                    .substrate
                    .entities
                    .get(cargo)
                    .and_then(|entity| sim.entity_object_type(entity, rules).cloned())
                    .ok_or(DepartureFailure::MissingPassenger)?;
                if let Some(entity) = sim.substrate.entities.get_mut(cargo) {
                    entity.on_bridge.store(bridge);
                    entity.locomotor = Some(
                        crate::sim::movement::locomotor::LocomotorState::from_object_type(
                            &object, frame,
                        ),
                    );
                }
                let Some((position, placement)) = sim.unlimbo_class_entry(
                    cargo,
                    requested,
                    PlacementEvidence::EvaluateMark,
                    rules,
                    registry,
                ) else {
                    return Err(DepartureFailure::Placement);
                };
                crate::sim::passenger::reveal_unloaded_passenger(
                    sim,
                    rules,
                    aircraft,
                    cargo,
                    position,
                    placement,
                    Some(facing),
                    effects,
                )?;
                // 10048920's success arm: the rates the AI copy brought from
                // the Carryall go back to the cargo type's `ROT=` and
                // `TurretROT=` (else `ROT=`).
                if ares && let Some(entity) = sim.substrate.entities.get_mut(cargo) {
                    entity.body_facing.set_rot(object.turret_rot);
                    if let Some(turret) = entity.barrel_facing.as_mut() {
                        turret.set_rot(object.secondary_turn_rate());
                    }
                }
                dropped = Some(cargo);
                Ok(())
            },
            effects,
        );
        let Some(cargo) = dropped else {
            // Ares 3.0 AircraftClass_Carryall_Unload_DestroyCargo (100488C0,
            // hook 0x416C4D) destroys a cargo the ground refused, and the
            // Carryall, each with `C4Warhead=` for its Health.
            if ares {
                self.ares_destroy_refused_carryall(aircraft, rules, registry, effects);
            }
            return;
        };
        if let Some(entity) = self.substrate.entities.get_mut(cargo) {
            entity.on_carryall.set(false);
        }
        self.refresh_object_sight(cargo, rules, 0, effects);
        // Ares 3.0 AircraftClass_Carryall_Unload_UpdateCargo (10048A10, hook
        // 0x416C94): the cargo's UpdatePosition(2) (vt+0x18C, its arrival
        // Per_Cell_Process), then a Deactivated one powers its locomotor off.
        if ares {
            if let Err(cause) = self.per_cell_process(
                cargo,
                crate::sim::movement::PerCellReason::Arrival,
                rules,
                registry,
                effects,
            ) {
                log::warn!("Carryall cargo {cargo} arrival: {cause:?}");
            }
            if let Some(entity) = self.substrate.entities.get_mut(cargo)
                && entity.deactivated.is_offline()
                && let Some(locomotor) = entity.locomotor.as_mut()
                && locomotor.is_powered()
            {
                locomotor.power_off();
            }
        }
        self.refresh_object_sight_border(
            cargo,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        let nearby = self.techno_nearby_location(aircraft, None, rules);
        if self
            .substrate
            .entities
            .get(aircraft)
            .and_then(|entity| entity.radio_contacts.slot(0))
            == Some(cargo)
        {
            crate::sim::radio::transmit_to_contact(
                self,
                aircraft,
                RadioMessage::Untether,
                rules,
                effects,
            );
            crate::sim::radio::transmit_to_contact(
                self,
                aircraft,
                RadioMessage::Break,
                rules,
                effects,
            );
        }
        // Ares 3.0 AircraftClass_Carryall_Unload_Guard (100489D0, hook
        // 0x416CF4): the cargo's Transporter (`+0x11C`) cleared, Guard queued
        // to start (vt+0x1E8(5, 1)) and its team takes it again.
        if ares {
            if let Some(entity) = self.substrate.entities.get_mut(cargo) {
                entity.passenger_role.clear_transporter();
            }
            let _ = self.mission_queue_exact(
                cargo,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
                1,
                self.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
            if let Some((team, _)) = self.team_script_vm.team_for_member(cargo) {
                self.team_add_member(team, cargo, false, rules, registry, effects);
            }
        }
        let destination = nearby
            .filter(|&cell| cell != (0, 0))
            .map(|(rx, ry)| NavTargetRef::cell(rx as u16, ry as u16));
        self.assign_aircraft_destination(aircraft, destination, rules, effects);
    }

    /// The refusal arm of Ares 3.0 `AircraftClass_Carryall_Unload_DestroyCargo
    /// @ 100488C0` (hook 0x416C4D): `ReceiveDamage(Health, 0, C4Warhead=,
    /// NULL, 1, 1, NULL)` on the cargo and then on the Carryall.
    ///
    /// RESIDUAL: VERA damages the Carryall alone and lets its death kill the
    /// cargo, which the departure put back in the hold (KillPassengers), so
    /// the cargo dies without its own receiver: no kill credit to nobody, no
    /// death animation or survivors. The damage receiver is not established
    /// for a Limbo victim. Trigger: an Ares Carryall dropping its cargo where
    /// the cargo cannot stand (a deep-water cell, a building).
    fn ares_destroy_refused_carryall(
        &mut self,
        aircraft: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(health) = self
            .substrate
            .entities
            .get(aircraft)
            .map(|entity| entity.health.current())
        else {
            return;
        };
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        self.commit_direct_damage_receiver(
            rules,
            registry,
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                aircraft,
                health,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            effects,
        );
    }
}

#[cfg(test)]
#[path = "carryall_tests.rs"]
mod tests;
