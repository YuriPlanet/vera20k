//! World host of `AircraftClass::Mission_Enter @ 0x00419C80`
//! (`aircraft::enter_mission`): the radio exchanges with the dock, the
//! dock coordinate and the landing step.
use super::{FrameEffects, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::enter_mission::{self, EnterHost, EnterNav};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::{air_movement, ground_pose};
use crate::sim::radio::{self, RadioMessage, RadioResponse};
use crate::sim::world::display_layers::DisplayLayer;

/// How far a descending aircraft steps toward its dock per visit, in leptons
/// along X and along Y (`0x00419F99..0x00419FC4`).
const DOCK_STEP_LEPTONS: i32 = 5;

impl Simulation {
    /// One Mission_Enter visit of aircraft `id`; the frames it returns.
    pub(crate) fn aircraft_mission_enter(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> i32 {
        enter_mission::enter_visit(&mut WorldEnter {
            sim: self,
            id,
            rules,
            registry,
            effects,
        })
    }
}

/// A Mission_Enter visit's world.
struct WorldEnter<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    registry: Option<&'a OverlayTypeRegistry>,
    effects: FrameEffects<'a>,
}

impl WorldEnter<'_> {
    fn entity(&self) -> &crate::sim::game_entity::GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .expect("aircraft dispatch")
    }

    fn transmit(&mut self, message: RadioMessage) -> RadioResponse {
        radio::transmit_to_contact(self.sim, self.id, message, Some(self.rules), self.effects)
    }
}

impl EnterHost for WorldEnter<'_> {
    fn height(&mut self) -> i32 {
        air_movement::current_fly_height(self.entity(), self.sim.resolved_terrain.as_ref())
    }

    fn airport_bound(&mut self) -> bool {
        self.sim
            .object_type(self.entity().type_ref(), self.rules)
            .is_some_and(|object| object.airport_bound)
    }

    /// `Mission_Enter @ 0x00419C80`'s NavCom (`+0x5A4`) reads (`0x00419CA4`,
    /// `0x00419D19`, `0x00419D37`, `0x00419D5D`); What_Am_I 6 is a Building.
    fn nav_com(&mut self) -> EnterNav {
        match self.entity().navigation.nav_com() {
            None => EnterNav::None,
            Some(NavTargetRef::Cell { .. }) => EnterNav::Other,
            Some(NavTargetRef::Object { id }) => {
                if self
                    .sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|object| object.category() == EntityCategory::Structure)
                {
                    EnterNav::Building(id)
                } else {
                    EnterNav::Other
                }
            }
        }
    }

    /// The cell of the Location (`MapClass` lookup, the shared dummy cell
    /// off the map, which holds nothing).
    fn building_here(&mut self) -> Option<u64> {
        let coord = ground_pose::position_world_coord(&self.entity().position);
        let (x, y) = (coord.x / 256, coord.y / 256);
        let (x, y) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
        self.sim
            .substrate
            .occupancy
            .first_building_on_layer(x, y, MovementLayer::Ground)
    }

    fn dock(&mut self) -> Option<u64> {
        self.entity().ammo().and_then(|ammo| ammo.dock())
    }

    fn set_dock(&mut self, dock: Option<u64>) {
        self.sim.set_aircraft_dock(self.id, dock);
    }

    fn over_out(&mut self) {
        self.transmit(RadioMessage::Break);
    }

    fn queue(&mut self, mission: MissionType, start: bool) {
        let now = self.sim.session.binary_frame();
        let _ = self.sim.mission_queue_exact(
            self.id,
            MissionId::from_known(mission),
            i32::from(start),
            now,
            &LiveReadyInputProvider { rules: self.rules },
        );
    }

    fn on_ground_layer(&mut self) -> bool {
        self.sim.entity_display_layer(self.id, Some(self.rules)) == Some(DisplayLayer::GROUND)
    }

    fn docking(&mut self) -> bool {
        self.transmit(RadioMessage::CanDock) == RadioResponse::Roger
    }

    fn approach_pending_entry(&mut self) -> bool {
        crate::sim::docking::building_dock::park_pending_entry(
            self.sim,
            self.rules,
            self.id,
            self.effects,
        )
    }

    fn enter_idle_mode(&mut self) {
        crate::sim::aircraft::enter_idle_mode_for(
            self.sim,
            self.id,
            self.rules,
            self.registry,
            self.effects,
        );
    }

    /// Mission+0xBC as `Mission_Enter @ 0x00419C80` reads it (`0x00419DA9`).
    fn state(&mut self) -> u32 {
        self.entity().mission.handler_state()
    }

    /// Mission+0xBC, Mission_Enter's own stores (`0x00419DC3`, `0x00419E4C`,
    /// `0x00419EF0`).
    fn set_state(&mut self, state: u32) {
        let entity = self.sim.substrate.entities.get_mut(self.id).unwrap();
        entity.mission.set_handler_state(state);
    }

    /// `+0x6D4`, `Mission_Enter @ 0x00419C80`'s stores (`0x00419DCD..0x00419FFD`).
    fn set_transition_ready(&mut self, ready: bool) {
        let entity = self.sim.substrate.entities.get_mut(self.id).unwrap();
        entity
            .mission_leaf
            .set_aircraft_transition_ready(u8::from(ready));
    }

    fn in_radio_contact(&mut self) -> bool {
        !self.entity().radio_contacts.is_empty()
    }

    /// A missile's Rocket never Enters: no idle arm picks Enter for an
    /// aircraft out of radio contact, and nothing links a missile.
    fn status(&mut self) -> i32 {
        air_movement::fly_status(self.entity()).unwrap_or(3)
    }

    /// `Mission_Enter @ 0x00419C80`'s `+0xB4 == -1` test (`0x00419E58`).
    fn nothing_queued(&mut self) -> bool {
        self.entity().mission.queued() == MissionId::NONE
    }

    fn clear_destination(&mut self) {
        self.sim
            .assign_aircraft_destination(self.id, None, self.rules, self.effects);
    }

    /// `Mission_Enter @ 0x00419C80`'s state-7 step: NavCom `+0x5A4` (`0x00419F35`,
    /// `0x00419F61`), clamped by 5 leptons (`0x00419F99..0x00419FC4`).
    fn step_toward_nav_com(&mut self) {
        let Some(nav) = self.entity().navigation.nav_com() else {
            return;
        };
        let building = match self.nav_com() {
            EnterNav::Building(id) => Some(id),
            EnterNav::None | EnterNav::Other => None,
        };
        let terrain = self.sim.resolved_terrain.as_ref();
        let target = match building {
            Some(building) => crate::sim::movement::building_dock_coordinate(
                &self.sim.substrate.entities,
                building,
                Some(self.id),
                terrain,
                self.rules,
                &self.sim.interner,
            ),
            None => ground_pose::target_get_coords(
                crate::sim::combat::TargetKind::from(nav),
                self.sim,
                terrain
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            ),
        }
        .expect("a live NavCom has a coordinate");
        let here = ground_pose::object_location(self.entity(), terrain);
        let step = |to: i32, from: i32| {
            to.wrapping_sub(from)
                .clamp(-DOCK_STEP_LEPTONS, DOCK_STEP_LEPTONS)
        };
        let next = DriveCoord {
            x: here.x.wrapping_add(step(target.x, here.x)),
            y: here.y.wrapping_add(step(target.y, here.y)),
            z: here.z,
        };
        self.sim.foot_set_location_marked(
            self.id,
            next,
            Some(self.rules),
            self.registry,
            self.effects,
        );
    }

    fn dock_now(&mut self) -> i32 {
        i32::from(self.transmit(RadioMessage::DockNow).code())
    }

    /// RESIDUAL, dormant: an aircraft receiver answers DOCK_NOW with 5
    /// (`0x00419300`), so natively an aircraft reaches this only with
    /// another aircraft as its radio contact, which only a transport
    /// aircraft's boarding forms; no retail aircraft type has `Passengers=`.
    /// VERA's aircraft receiver answers CAN_LOAD and DOCK_NOW 0
    /// (`radio::receive`), so neither the Infantry nor the Unit
    /// PerCellProcess arm puts a passenger aboard an aircraft.
    fn board_contact(&mut self) {}
}
