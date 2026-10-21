//! Ordinary stock producer and complete observed canopy lifecycle.
//! Physical XMP03T4 and initial production-placed GAPOWR/AMRADR; actual launch House,
//! grant, recharge, queued SPECIAL_PLACE, flight, drop, fall and Anim lifecycle.
//! No mouse-gesture, player base-construction or original whole-match claim.

use crate::headless_scenario::{HeadlessScenario, battle_session, load_session};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::foundation::foundation_cell_offsets;
use crate::sim::anim_class::{AnimRemap, AnimWorldCoord};
use crate::sim::cell_rect::{
    CellRect, CellRectPassabilityContext, cell_is_in_playfield_height_aware, check_passability_rect,
};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::movement::{air_movement, ground_pose};
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use std::collections::BTreeMap;
use std::path::Path;

const SUPER: &str = "AmericanParaDropSpecial";
const PAYLOAD: usize = 8;

#[derive(Debug)]
struct CanopyObservation {
    boundary: &'static str,
    binary_frame: u32,
    passenger: u64,
    canopy: u64,
    body: DriveCoord,
    canopy_coord: Option<AnimWorldCoord>,
    anim_frame: Option<i32>,
    loops: Option<u8>,
}

#[derive(Default, Debug)]
struct PassengerHistory {
    canopy: Option<u64>,
    departure_z: i32,
    last_z: i32,
    lower_sample: bool,
    landed: bool,
    collapsed: bool,
}

struct RetailParachuteScene {
    scene: HeadlessScenario,
    owner: InternedId,
    power_plant: u64,
    provider: u64,
    target: (u16, u16),
    sw_type: InternedId,
}

impl RetailParachuteScene {
    fn load(root: &Path) -> Self {
        let mut launch = battle_session("XMP03T4.MAP", &[]);
        launch.options.unit_count = 0;
        launch.options.bases = true;
        launch.options.short_game = false;
        launch.options.crates = false;
        launch.options.shroud = false;
        launch.options.fog_of_war = false;
        let mut scene = load_session(root, "XMP03T4.MAP", 0x5F59_4001, launch)
            .expect("physical scenario and animation assets bind");
        let owner = scene
            .sim()
            .session()
            .current_house()
            .expect("launch Player");
        let owner_name = scene.sim().interner().resolve(owner).to_owned();
        let house = scene.sim().houses().get(&owner).unwrap();
        assert!(house.is_human());
        assert_eq!(
            scene.sim().interner().resolve(house.country().unwrap()),
            "Americans"
        );
        let rules = &scene.runtime.resources.rules;
        assert_eq!(
            rules.object("AMRADR").unwrap().super_weapon.as_deref(),
            Some(SUPER)
        );
        assert_eq!(
            rules.super_weapon(SUPER).unwrap().recharge_time_frames,
            3600
        );
        assert!(!rules.super_weapon(SUPER).unwrap().is_powered);
        assert_eq!(rules.object("GAPOWR").unwrap().power, 200);
        assert_eq!(rules.object("AMRADR").unwrap().power, -50);
        assert_eq!(rules.general.parachute_shp.as_deref(), Some("PARACH"));
        let infantry = rules.object("E1").unwrap();
        assert_eq!(
            infantry.speed_type,
            crate::rules::locomotor_type::SpeedType::Foot
        );
        assert_eq!(
            infantry.movement_zone,
            crate::rules::locomotor_type::MovementZone::Infantry
        );
        let art = rules.art().anim_runtime_config("PARACH").unwrap();
        assert!(
            art.raw_shp_frame_count.is_some(),
            "real SHP binder, no supplied count"
        );
        assert_eq!((art.loop_start, art.loop_end, art.loop_count), (20, 39, 30));
        assert_eq!(art.z_adjust, -10);

        // Declared initial completed power plant and provider placement. Grant
        // uses House power even for this IsPowered=false Super; the ordinary
        // House/Building updates must make the provider online and start charge.
        // Never edits a Super, House, frame, map cell, entity mission, falling
        // state or Anim runtime.
        let candidates: Vec<_> = scene
            .sim()
            .resolved_terrain()
            .unwrap()
            .cells()
            .iter()
            .map(|cell| (cell.rx, cell.ry))
            .collect();
        let [power_plant, provider] = ["GAPOWR", "AMRADR"].map(|type_id| {
            candidates
                .iter()
                .copied()
                .find_map(|(x, y)| {
                    let sim = &scene.runtime.simulation;
                    let rules = &scene.runtime.resources.rules;
                    let terrain = sim.resolved_terrain().unwrap();
                    let ty = rules.object(type_id).unwrap();
                    let level = terrain.cell(x, y)?.level;
                    let level_foundation =
                        foundation_cell_offsets(&ty.foundation)
                            .iter()
                            .all(|&(dx, dy)| {
                                let xy =
                                    (i32::from(x) + i32::from(dx), i32::from(y) + i32::from(dy));
                                cell_is_in_playfield_height_aware(
                                    xy,
                                    sim.playfield_bounds(),
                                    Some(terrain),
                                ) && terrain
                                    .cell(xy.0 as u16, xy.1 as u16)
                                    .is_some_and(|cell| cell.level == level && cell.slope_type == 0)
                            });
                    let clear_foundation =
                        foundation_cell_offsets(&ty.foundation)
                            .iter()
                            .all(|&(dx, dy)| {
                                clear_ground_rect(
                                    sim,
                                    CellRect::new(
                                        i32::from(x) + i32::from(dx),
                                        i32::from(y) + i32::from(dy),
                                        1,
                                        1,
                                    ),
                                )
                            });
                    if !level_foundation || !clear_foundation {
                        return None;
                    }
                    // This read-only candidate query is not a Build command. The
                    // initial-object constructor/Unlimbo below still admits the site.
                    scene.runtime.simulation.spawn_object_with_overlay_registry(
                        type_id,
                        &owner_name,
                        x,
                        y,
                        0,
                        &scene.runtime.resources.rules,
                        &scene.runtime.resources.overlay_registry,
                    )
                })
                .unwrap_or_else(|| panic!("a real foundation must admit initial {type_id}"))
        });
        // Select an interior ground patch via current passability, not a map
        // rewrite. Slopes/height variation are retained and later observed.
        let target = candidates
            .into_iter()
            .find(|&cell| clear_drop_patch(scene.sim(), cell))
            .expect("physical map has a clear ground paradrop target");
        let sw_type = scene
            .sim()
            .interner()
            .get(SUPER)
            .expect("House constructor Super registry");
        Self {
            scene,
            owner,
            power_plant,
            provider,
            target,
            sw_type,
        }
    }

    fn step(&mut self) -> usize {
        let due = self.scene.runtime.simulation.take_due_commands();
        let output = self
            .scene
            .runtime
            .advance_frame(
                &due,
                crate::app::types::SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary bound frame");
        assert!(
            output.tick.frame_committed,
            "match must continue: {:?}",
            output.tick
        );
        let house = self.scene.sim().houses().get(&self.owner).unwrap();
        assert!(
            !house.is_defeated() && house.outcome().is_none(),
            "owner must remain active"
        );
        output.tick.executed_commands
    }

    fn ready(&self) -> bool {
        let sw = &self.scene.sim().super_weapons()[&self.owner][&self.sw_type];
        sw.is_active() && sw.is_ready() && !sw.is_suspended()
    }

    fn log_recharge_state(&self, boundary: &str) {
        let sim = self.scene.sim();
        eprintln!(
            "paradrop {boundary}: frame={} power_plant={} provider={} operational={:?} power={:?} super={:?}",
            sim.session().binary_frame(),
            self.power_plant,
            self.provider,
            sim.building_operational_state(self.provider, &self.scene.runtime.resources.rules),
            sim.power_states().get(&self.owner),
            sim.super_weapons()[&self.owner][&self.sw_type],
        );
    }

    /// Returns the real carrier and its cargo identities before any drop.
    fn charge_and_launch(&mut self) -> (u64, Vec<u64>) {
        assert!(!self.ready(), "no supplied readiness");
        let mut grant_frame = None;
        for _ in 0..4112 {
            if grant_frame.is_none()
                && self.scene.sim().super_weapons()[&self.owner][&self.sw_type].is_active()
            {
                self.log_recharge_state("granted");
                grant_frame = Some(self.scene.sim().session().binary_frame());
            }
            if self.ready() {
                break;
            }
            self.step();
        }
        if !self.ready() {
            self.log_recharge_state("charge_timeout");
        }
        assert!(
            self.ready(),
            "provider grant and normal recharge: {:?}",
            self.scene.sim().super_weapons()[&self.owner][&self.sw_type]
        );
        self.log_recharge_state("ready");
        let recharge = u32::try_from(
            self.scene
                .runtime
                .resources
                .rules
                .super_weapon(SUPER)
                .unwrap()
                .recharge_time_frames,
        )
        .expect("nonnegative physical recharge duration");
        assert_eq!(
            self.scene
                .sim()
                .session()
                .binary_frame()
                .wrapping_sub(grant_frame.expect("observed the real provider grant")),
            recharge,
            "grant to ready consumes the complete physical recharge duration",
        );
        let sim = &mut self.scene.runtime.simulation;
        let execute_tick = sim.session().tick() + 1;
        sim.queue_command(CommandEnvelope::new(
            self.owner,
            execute_tick,
            Command::LaunchSuperWeapon {
                sw_type_id: self.sw_type,
                target_rx: self.target.0,
                target_ry: self.target.1,
            },
        ));
        assert_eq!(self.step(), 1, "one queued SPECIAL_PLACE executes");
        assert!(!self.ready(), "the real launch consumes readiness");
        let sim = self.scene.sim();
        let planes: Vec<_> = sim
            .entities()
            .values()
            .filter(|entity| {
                entity.owner() == self.owner
                    && sim.interner().resolve(entity.type_ref()) == "PDPLANE"
            })
            .collect();
        assert_eq!(planes.len(), 1);
        let plane = planes[0];
        let passengers = plane.passenger_role.cargo().unwrap().passengers().to_vec();
        assert_eq!(passengers.len(), PAYLOAD, "physical American payload");
        for &id in &passengers {
            let passenger = sim.entities().get(id).unwrap();
            assert_eq!(sim.interner().resolve(passenger.type_ref()), "E1");
            assert_eq!(passenger.owner(), self.owner);
            assert!(passenger.lifecycle.in_limbo() && !falling_observation(passenger));
            assert!(!sim.anims().any(|(_, anim)| anim.owner_entity() == Some(id)
                && sim.interner().resolve(anim.type_id()) == "PARACH"));
        }
        (plane.stable_id(), passengers)
    }

    /// Observe all real passengers through departure, descent, landing and
    /// canopy collapse. No rendering implementation or pixel oracle is included.
    fn observe_trip(&mut self, plane: u64, passengers: &[u64]) -> Vec<CanopyObservation> {
        let mut history: BTreeMap<_, _> = passengers
            .iter()
            .copied()
            .map(|id| (id, PassengerHistory::default()))
            .collect();
        let mut observations = Vec::new();
        for _ in 0..4000 {
            self.step();
            let sim = self.scene.sim();
            let cells = NativeCellQuery::isolated(sim.resolved_terrain().unwrap());
            for (&id, prior) in &mut history {
                let body = sim.entities().get(id).expect("clear-scene E1 survives");
                if body.lifecycle.in_limbo() {
                    continue;
                }
                assert_eq!(body.owner(), self.owner);
                let location = ground_pose::object_get_coords(body, sim.resolved_terrain());
                let boundary = if prior.canopy.is_none() {
                    assert!(
                        falling_observation(body),
                        "observe real departure before landing"
                    );
                    let attached: Vec<_> = sim
                        .anims()
                        .filter(|(_, anim)| {
                            anim.owner_entity() == Some(id)
                                && sim.interner().resolve(anim.type_id()) == "PARACH"
                        })
                        .map(|(&anim_id, _)| anim_id)
                        .collect();
                    assert_eq!(attached.len(), 1, "one actually constructed canopy");
                    prior.canopy = Some(attached[0]);
                    prior.departure_z = location.z;
                    prior.last_z = location.z;
                    Some("departure")
                } else if !prior.landed {
                    assert!(location.z <= prior.last_z, "natural descent does not rise");
                    prior.last_z = location.z;
                    if air_movement::queried_flight_height(body, Some(&cells)) == 0 {
                        assert!(
                            !falling_observation(body),
                            "the landing owner clears falling"
                        );
                        assert!(
                            prior.lower_sample,
                            "a natural lower-height sample precedes landing"
                        );
                        prior.landed = true;
                        Some("landing")
                    } else if !prior.lower_sample && prior.departure_z - location.z >= 128 {
                        assert!(falling_observation(body));
                        prior.lower_sample = true;
                        Some("lower_descent")
                    } else {
                        None
                    }
                } else if !prior.collapsed && sim.anim(prior.canopy.unwrap()).is_none() {
                    prior.collapsed = true;
                    Some("collapse_complete")
                } else {
                    None
                };
                if let Some(boundary) = boundary {
                    let canopy = prior.canopy.unwrap();
                    let anim = sim.anim(canopy);
                    if boundary != "collapse_complete" {
                        let anim = anim.expect("canopy survives through the landing edge");
                        assert_eq!(anim.owner_entity(), Some(id));
                        assert_eq!(anim.remap(), Some(AnimRemap::House(self.owner)));
                        let xyz = sim.anim_absolute_coord(canopy).unwrap();
                        assert_eq!(
                            (xyz.x, xyz.y, xyz.z),
                            (location.x, location.y, location.z + 75)
                        );
                        if boundary == "landing" {
                            assert_eq!(anim.runtime().loop_remaining, 0);
                        }
                    }
                    let sample = CanopyObservation {
                        boundary,
                        binary_frame: sim.session().binary_frame(),
                        passenger: id,
                        canopy,
                        body: location,
                        canopy_coord: sim.anim_absolute_coord(canopy),
                        anim_frame: anim.map(|a| a.runtime().current_frame),
                        loops: anim.map(|a| a.runtime().loop_remaining),
                    };
                    eprintln!(
                        "{} frame={} passenger={} canopy={} body={:?} canopy_coord={:?} anim_frame={:?} loops={:?}",
                        sample.boundary,
                        sample.binary_frame,
                        sample.passenger,
                        sample.canopy,
                        sample.body,
                        sample.canopy_coord,
                        sample.anim_frame,
                        sample.loops
                    );
                    observations.push(sample);
                }
            }
            if history.values().all(|state| state.collapsed) && !sim.entities().contains(plane) {
                assert_eq!(observations.len(), PAYLOAD * 4);
                return observations;
            }
        }
        panic!("flight/descent/collapse exceeded 4000 frames: {history:?}");
    }
}

// Read the retained fall words through serialization only at lifecycle
// boundaries. This observes core-private state without exposing or setting it.
fn falling_observation(entity: &crate::sim::game_entity::GameEntity) -> bool {
    serde_json::to_value(entity).expect("serialize retained entity")["fall"]["falling"]
        .as_bool()
        .expect("retained Object fall state")
}

fn clear_ground_rect(sim: &Simulation, rect: CellRect) -> bool {
    let terrain = sim.resolved_terrain().unwrap();
    let cells = NativeCellQuery::isolated(terrain);
    check_passability_rect(CellRectPassabilityContext {
        native_cells: Some(&cells),
        rect,
        speed_type: crate::rules::locomotor_type::SpeedType::Foot,
        required_zone_id: None,
        movement_zone: crate::rules::locomotor_type::MovementZone::Infantry,
        required_height_or_level: None,
        bridge_aware_zone: true,
        reject_any_overlay: true,
        resolved_terrain: Some(terrain),
        overlay_grid: sim.overlay_grid(),
        occupancy: Some(sim.occupancy()),
        zone_grid: sim.zone_grid(),
    })
}

fn clear_drop_patch(sim: &Simulation, (x, y): (u16, u16)) -> bool {
    let terrain = sim.resolved_terrain().unwrap();
    let inside = (-5..=5).all(|dx| {
        (-5..=5).all(|dy| {
            let xy = (i32::from(x) + dx, i32::from(y) + dy);
            if !cell_is_in_playfield_height_aware(xy, sim.playfield_bounds(), Some(terrain)) {
                return false;
            }
            let Some(tile) = terrain.cell(xy.0 as u16, xy.1 as u16) else {
                return false;
            };
            if tile.has_bridge_deck || tile.bridge_transition {
                return false;
            }
            true
        })
    });
    inside
        && clear_ground_rect(
            sim,
            CellRect::new(i32::from(x) - 5, i32::from(y) - 5, 11, 11),
        )
}

#[test]
#[ignore = "requires physical retail assets and a full natural recharge/flight"]
fn retail_american_paradrop_produces_and_retires_its_own_canopies() {
    let (root, _) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1 required");
    let mut witness = RetailParachuteScene::load(&root);
    assert!(witness.scene.sim().entities().contains(witness.power_plant));
    assert!(witness.scene.sim().entities().contains(witness.provider));
    let (plane, passengers) = witness.charge_and_launch();
    let observations = witness.observe_trip(plane, &passengers);
    assert_eq!(
        observations
            .iter()
            .filter(|row| row.boundary == "departure")
            .count(),
        PAYLOAD
    );
}
