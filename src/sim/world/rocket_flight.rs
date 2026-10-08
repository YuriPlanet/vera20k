//! Production host for `RocketLocomotionClass` (`movement::rocket_movement`):
//! Move_To from the Foot destination setter, Process from the object turn,
//! and Detonate's shared effects at their native call sites.
//!
//! The rocket's private fields stay in its locomotor; Process runs on a copy
//! that is written back afterwards, so owner callbacks that reach the
//! locomotor (Mark, UnInit) see the frame's starting fields. Location, Mark,
//! display, AircraftTracker, facing, anims, sound, light and area damage use
//! their existing owners.

use super::Simulation;
use super::lifecycle::UninitContext;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::map::retail_trig::{TrigTable, required_math_tables};
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::rocket_movement::{self, RocketHost};
use crate::sim::projectile::ProjectileCoord;

struct RocketProcessHost<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
    registry: Option<&'a OverlayTypeRegistry>,
    trig: &'a TrigTable,
    id: u64,
    frame: u32,
    bridge_state_changed: bool,
}

impl<'a> RocketProcessHost<'a> {
    fn owner(&self) -> &GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .expect("Rocket Process owner exists until its UnInit returns")
    }

    fn owner_mut(&mut self) -> &mut GameEntity {
        self.sim
            .substrate
            .entities
            .get_mut(self.id)
            .expect("Rocket Process owner exists until its UnInit returns")
    }

    fn warhead(&self, name: &str) -> Option<&'a crate::rules::warhead_type::WarheadType> {
        let rules: &'a RuleSet = self.rules;
        let warhead = rules.warhead(name);
        if warhead.is_none() {
            log::warn!("rocket {} detonates with unknown warhead [{name}]", self.id);
        }
        warhead
    }
}

impl RocketHost for RocketProcessHost<'_> {
    fn binary_frame(&self) -> u32 {
        self.frame
    }
    fn trig(&self) -> &TrigTable {
        self.trig
    }
    fn spawn_owner_is_elite(&self) -> bool {
        self.owner()
            .spawn_owner_id
            .and_then(|spawner| self.sim.substrate.entities.get(spawner))
            .is_some_and(crate::sim::combat::gattling::is_elite)
    }
    fn location(&self) -> [i32; 3] {
        let location = position_world_coord(&self.owner().position);
        [location.x, location.y, location.z]
    }
    fn set_location(&mut self, coord: [i32; 3]) {
        // A marked rocket takes 0x004DB810's Mark(UP)/Mark(DOWN) pair.
        let [x, y, z] = coord;
        self.sim.foot_set_location_marked(
            self.id,
            DriveCoord { x, y, z },
            Some(self.rules),
            self.registry,
        );
    }
    fn height(&self) -> i32 {
        crate::sim::movement::air_movement::current_fly_height(
            self.owner(),
            self.sim.resolved_terrain.as_ref(),
        )
    }
    fn type_speed(&self) -> i32 {
        let owner = self.owner();
        self.rules
            .object(self.sim.interner.resolve(owner.type_ref()))
            .map_or(0, |object| {
                crate::util::fixed_math::ra2_speed_to_leptons_per_frame(object.speed)
            })
    }
    fn in_bounds(&self, cell: (i16, i16)) -> bool {
        self.sim.map_cell_in_bounds(cell)
    }
    fn mark(&mut self, put: bool) {
        if put {
            self.sim
                .foot_mark_put(self.id, Some(self.rules), self.registry);
        } else {
            self.sim
                .foot_mark_remove(self.id, Some(self.rules), self.registry);
        }
    }
    fn submit_display(&mut self) {
        self.sim
            .submit_entity_display(self.id, Some(self.rules), None);
    }
    fn tracker_cell(&self) -> (i16, i16) {
        self.owner().air_tracker_cell()
    }
    fn tracker_add(&mut self) {
        self.sim.aircraft_tracker_add(self.id);
    }
    fn tracker_update(&mut self, cell: (i16, i16)) {
        self.sim.aircraft_tracker_update_cell(self.id, cell);
    }
    fn tracker_remove(&mut self) {
        self.sim.aircraft_tracker_remove(self.id);
    }
    fn is_alive(&self) -> bool {
        self.owner().is_object_alive()
    }
    fn health(&self) -> i32 {
        self.owner().health.current
    }
    fn facing(&self) -> u16 {
        self.owner().body_facing_current(self.frame)
    }
    fn set_facing(&mut self, facing: u16) {
        let frame = self.frame;
        self.owner_mut().body_facing.set(facing, frame);
    }
    fn anim(&mut self, name: &'static str, coord: [i32; 3], delay: i32, z_adjust: i32) {
        let [x, y, z] = coord;
        self.sim.spawn_named_anim(
            self.rules,
            name,
            AnimWorldCoord { x, y, z },
            delay as u16,
            rocket_movement::PUFF_DRAW_FLAGS,
            z_adjust,
        );
    }
    fn aux_sound(&mut self, coord: [i32; 3]) {
        // Both callers pass the owner's Location, which the event records.
        debug_assert_eq!(coord, self.location());
        let owner = self.owner();
        let Some(sound) = self
            .rules
            .object(self.sim.interner.resolve(owner.type_ref()))
            .and_then(|object| object.aux_sound1.clone())
        else {
            return;
        };
        let event = super::SimSoundEvent::voc_at(sound, &owner.position);
        self.sim.sound_events.push(event);
    }
    fn structural_bridge_floor(&self) -> Option<i32> {
        let terrain = self.sim.resolved_terrain.as_ref()?;
        let cells = NativeCellQuery::canonical(terrain);
        let [x, y, _] = self.location();
        let cell = cells.lookup_world(x, y);
        if cells.flags(cell) & 0x100 == 0 {
            return None;
        }
        crate::sim::cell_kernel::native_cell_own_coords(cell, &cells).map(|(_, _, z)| z as i32)
    }
    fn explosion(&mut self, damage: i32, warhead: &str, coord: [i32; 3]) {
        let Some(warhead) = self.warhead(warhead) else {
            return;
        };
        let coordinate = ProjectileCoord::new(coord[0], coord[1], coord[2]);
        let land = crate::sim::combat::detonation_anim::land_at(self.sim, coordinate);
        if let Some(effect) = crate::sim::combat::detonation_anim::effect(
            self.sim, self.rules, warhead, damage, land, coordinate, coordinate,
        ) {
            super::damage_consequences::admit_explosion_effect(self.sim, self.rules, effect);
        }
    }
    fn combat_light(&mut self, damage: i32, warhead: &str, coord: [i32; 3]) {
        if self.rules.warhead(warhead).is_none() {
            return;
        }
        let warhead_ref = self.sim.interner.intern(warhead);
        self.sim
            .combat_light_requests
            .push(crate::sim::combat::CombatLightRequest {
                target_id: None,
                damage,
                warhead_ref,
                coord: ProjectileCoord::new(coord[0], coord[1], coord[2]),
                force_create: false,
                flags: 0,
            });
    }
    fn area_damage(&mut self, coord: [i32; 3], damage: i32, warhead: &str) {
        let Some(warhead_type) = self.warhead(warhead) else {
            return;
        };
        let warhead_ref = self.sim.interner.intern(warhead);
        self.bridge_state_changed |= crate::sim::combat::world_receiver::apply_area_damage(
            self.sim,
            self.rules,
            self.registry,
            ProjectileCoord::new(coord[0], coord[1], coord[2]),
            damage,
            warhead_type,
            (self.id, None, warhead_ref),
        );
    }
    fn uninit(&mut self) {
        self.sim
            .uninit_with_context(self.id, UninitContext::new(Some(self.rules), self.registry));
    }
}

/// What one Rocket Process did that its object turn reports on.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RocketProcessOutcome {
    /// The impact's area damage changed a bridge.
    pub(crate) bridge_state_changed: bool,
}

impl Simulation {
    /// Rocket Move_To (`0x006632E0`) for the Foot destination setter's
    /// locomotor dispatch.
    pub(crate) fn rocket_move_to(&mut self, id: u64, to: DriveCoord, rules: &RuleSet) {
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let (block, _) = rules
            .missile_spawn
            .rocket_block(self.interner.resolve(entity.type_ref()));
        if let Some(rocket) = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.rocket_runtime_mut())
        {
            rocket_movement::move_to(rocket, block, to, frame);
        }
    }

    /// Rocket Process (`0x006622C0`) for one owner whose active locomotor is
    /// Rocket.
    pub(crate) fn process_rocket(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> RocketProcessOutcome {
        let Some(entity) = self.substrate.entities.get(id) else {
            return RocketProcessOutcome::default();
        };
        let Some(mut rocket) = entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.rocket_runtime())
            .cloned()
        else {
            return RocketProcessOutcome::default();
        };
        let (block, cmisl) = rules
            .missile_spawn
            .rocket_block(self.interner.resolve(entity.type_ref()));
        let (trig, _) = required_math_tables();
        let frame = self.session.binary_frame;
        let mut host = RocketProcessHost {
            sim: self,
            rules,
            registry,
            trig,
            id,
            frame,
            bridge_state_changed: false,
        };
        rocket_movement::process(&mut rocket, block, cmisl, &mut host);
        let outcome = RocketProcessOutcome {
            bridge_state_changed: host.bridge_state_changed,
        };
        if let Some(installed) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.locomotor.as_mut())
            .and_then(|locomotor| locomotor.rocket_runtime_mut())
        {
            *installed = rocket;
        }
        outcome
    }
}

#[cfg(test)]
pub(super) mod retail_tests {
    //! Production composition witnesses: a stock V3 Launcher on retail Hills
    //! and a Dreadnought on retail Lost Lake force-attack a target through an
    //! ordinary command and bound runtime frames. The flight's native
    //! comparison is
    //! `movement::rocket_movement::tests`, the missile's Mission_Move and
    //! Find_Attack_Cell `world::aircraft_move::tests`, its idle mode and
    //! Mission_Retreat `aircraft::idle_entry::tests` and
    //! `aircraft::retreat_mission::tests`; this run claims no native
    //! whole-flight or whole-match comparison.

    use crate::headless_scenario::SIM_TICK_MS;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::movement::ground_pose::position_world_coord;
    use std::collections::BTreeSet;

    /// Flat walkable In_Bounds ground for a launcher, and the same ten cells
    /// east of it, with the playfield around the whole path: between the
    /// kamikaze tracker's targets (every 30 frames, dropped on every 16th by
    /// `TechnoClass::AI`'s illegal-target test, the missile having no weapon)
    /// a missile past the playfield's edge leaves the map (`aircraft::leave_map`).
    pub(in crate::sim::world) fn sites(
        scenario: &crate::headless_scenario::HeadlessScenario,
    ) -> Vec<((u16, u16), (u16, u16))> {
        let sim = scenario.sim();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let navigation = sim.path_grid().unwrap();
        let in_playfield = |x: u16, y: u16| {
            (i32::from(x) - 3..=i32::from(x) + 13).all(|x| {
                (i32::from(y) - 3..=i32::from(y) + 3).all(|y| {
                    crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                        (x, y),
                        sim.playfield_bounds,
                        Some(terrain),
                    )
                })
            })
        };
        // The flight's steps need In_Bounds cells around both ends.
        let open = |x: u16, y: u16, level: u8| {
            [(-3, -3), (3, -3), (-3, 3), (3, 3)]
                .iter()
                .all(|&(dx, dy)| sim.map_cell_in_bounds((x as i16 + dx, y as i16 + dy)))
                && terrain.cell(x, y).is_some_and(|cell| {
                    cell.level == level
                        && cell.slope_type == 0
                        && !cell.is_water
                        && !cell.bridge_facts.has_structural_bridge()
                })
                && navigation
                    .cell(x, y)
                    .is_some_and(|cell| cell.ground_walkable)
                && sim.substrate.occupancy.get(x, y).is_none()
        };
        terrain
            .cells()
            .iter()
            .filter_map(|cell| {
                let (x, y) = (cell.rx, cell.ry);
                let target = (x.checked_add(10)?, y);
                (open(x, y, cell.level)
                    && open(target.0, target.1, cell.level)
                    && in_playfield(x, y))
                .then_some(((x, y), target))
            })
            .collect()
    }

    #[test]
    #[ignore = "requires the configured retail install and stock Hills.mmx"]
    fn retail_v3_strike_flies_its_rocket_into_the_target() {
        // Ordered at frame 0, the tracker's target is dropped before the
        // missile's first Mission_Attack visit; ordered at frame 9, it is held
        // through a state 1 visit.
        for (order_frame, held) in [(0, false), (9, true)] {
            let retail = std::env::var("RA2_DIR")
                .map(std::path::PathBuf::from)
                .expect("RA2_DIR names the retail install");
            let mut scenario = crate::headless_scenario::load(&retail, "Hills.mmx", 0x0B21_D6E5)
                .expect("load retail Hills through the production loader");
            let candidates = sites(&scenario);
            let owner = scenario.sim().session.current_house.expect("launch house");
            let owner_name = scenario.sim().interner.resolve(owner).to_owned();
            let runtime = &mut scenario.runtime;
            let rules = &runtime.resources.rules;
            let (tilt_frames, altitude) = (
                rules.missile_spawn.v3.tilt_frames,
                rules.missile_spawn.v3.altitude,
            );
            let sim = &mut runtime.simulation;
            // The first candidate whose cells admit both Unlimbos.
            let ((launcher_cell, target_cell), v3, target) = candidates
                .into_iter()
                .find_map(|(launcher, target)| {
                    let tank =
                        sim.spawn_object("MTNK", &owner_name, target.0, target.1, 0, rules)?;
                    match sim.spawn_object("V3", &owner_name, launcher.0, launcher.1, 0, rules) {
                        Some(v3) => Some(((launcher, target), v3, tank)),
                        None => {
                            sim.uninit(tank);
                            None
                        }
                    }
                })
                .expect("Hills admits a V3 and a tank on open level ground");
            sim.resolve_type_handles(rules);
            let strength = sim.substrate.entities.get(target).unwrap().health.current;
            let missile = sim
                .substrate
                .entities
                .get(v3)
                .and_then(|e| e.spawn_manager.as_ref())
                .and_then(|m| m.slots[0].spawn)
                .expect("the V3 builds its missile");
            let mut seen_anims: BTreeSet<_> = sim.anims().map(|(&id, _)| id).collect();
            let mut anim_names = BTreeSet::new();
            let mut launched = None;
            let mut transitions = Vec::new();
            let mut highest = i32::MIN;
            let mut impact = None;
            let mut detonated = None;
            let mut last_location = None;
            let mut moves = Vec::new();
            for frame in 0..1500 {
                let commands = if frame == order_frame {
                    vec![CommandEnvelope::new(
                        owner,
                        runtime.simulation.session.tick + 1,
                        Command::ForceAttack {
                            attacker_id: v3,
                            target_id: target,
                        },
                    )]
                } else {
                    Vec::new()
                };
                runtime
                    .advance_frame_for_tooling(&commands, SIM_TICK_MS)
                    .expect("advance production frame");
                let sim = &runtime.simulation;
                for (&id, anim) in sim.anims() {
                    if seen_anims.insert(id) {
                        anim_names.insert(sim.interner.resolve(anim.type_id).to_owned());
                    }
                }
                // Detonate's UnInit leaves the frame's pending-delete flush.
                let Some(entity) = sim
                    .substrate
                    .entities
                    .get(missile)
                    .filter(|entity| entity.is_object_alive())
                else {
                    detonated = Some(frame);
                    impact = last_location;
                    break;
                };
                if entity.lifecycle.in_limbo {
                    continue;
                }
                launched.get_or_insert(frame);
                // The current mission and Mission_Move's or Mission_Attack's
                // state.
                let state = match entity.aircraft_mission {
                    Some(
                        crate::sim::aircraft::AircraftMission::Move { sub_state }
                        | crate::sim::aircraft::AircraftMission::Attack { sub_state },
                    ) => Some(sub_state),
                    _ => None,
                };
                let mission = (entity.mission.current().known(), state);
                let visit = (mission, entity.navigation.nav_com);
                if moves.last().is_none_or(|&(_, last)| last != visit) {
                    moves.push((frame, visit));
                }
                let location = position_world_coord(&entity.position);
                highest = highest.max(location.z);
                last_location = Some(location);
                let state = entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.rocket_runtime())
                    .expect("the missile flies on its Rocket locomotor")
                    .mission_state_for_test();
                if transitions.last().is_none_or(|&(_, last)| last != state) {
                    transitions.push((frame, state));
                }
            }
            let launched = launched.expect("the V3 launches its missile");
            let detonated = detonated.expect("the missile detonates");
            let impact = impact.expect("the missile flew");
            let health = runtime
                .simulation
                .substrate
                .entities
                .get(target)
                .map_or(0, |e| e.health.current);
            println!(
                "V3 at {launcher_cell:?}, MTNK at {target_cell:?}: launch frame {launched}, \
                 states {transitions:?}, missions {moves:?}, highest Z {highest}, \
                 detonation frame {detonated} near {impact:?}, target health {strength} -> \
                 {health}, anims {anim_names:?}"
            );
            // The V3's own AI launches the missile at the tank with Move
            // queued; its Unlimbo appends it to the logic vector
            // (`0x0055BAA0`), so it commences Move later in the same pass. The
            // next frame's first Mission_Move visit's Find_Attack_Cell moves
            // the NavCom to a free cell beside the tank (Spawned, the missile
            // may share only a spawner's cell), which the flying Rocket's
            // Move_To ignores. The tracker's Attack, pushed in the launching
            // pass, commences two frames after it, restarting the mission
            // state at Mission_Attack's 0.
            use crate::sim::components::NavTargetRef;
            use crate::sim::mission::MissionType::{Attack, Move, Retreat};
            let tank = Some(NavTargetRef::Entity { id: target });
            assert_eq!(
                moves[0],
                (launched, ((Some(Move), Some(0)), tank)),
                "{moves:?}"
            );
            let (_, (mission, nav)) = moves[1];
            assert_eq!(mission, (Some(Move), Some(1)), "{moves:?}");
            let Some(NavTargetRef::Cell { rx, ry }) = nav else {
                panic!("Find_Attack_Cell picks a cell: {moves:?}");
            };
            assert!(
                (rx, ry) != target_cell
                    && rx.abs_diff(target_cell.0) <= 2
                    && ry.abs_diff(target_cell.1) <= 2,
                "a free cell beside the tank: {moves:?}"
            );
            assert_eq!(
                moves[2],
                (launched + 2, ((Some(Attack), Some(0)), nav)),
                "{moves:?}"
            );
            // Once TechnoClass::AI drops the target the weaponless missile cannot
            // fire at, state 10 enters idle mode, which drops the destination
            // and commences Retreat for good; Mission_Retreat's first visit
            // heads for a cell past the playfield's edge.
            let retreat = moves
                .iter()
                .position(|&(_, ((mission, _), _))| mission == Some(Retreat))
                .expect("the missile retreats");
            assert_eq!(
                moves[retreat - 1].1.0,
                (Some(Attack), Some(10)),
                "{moves:?}"
            );
            // While it holds the target, state 1's FindFireLocation finds no
            // cell for a weaponless missile (`0x004197C0`), and the NULL
            // destination clears the NavCom (`0x004D9510`): state 10 follows
            // with none, and its Rate epilogue outlasts the target.
            let attack = &moves[3..retreat];
            assert!(
                attack
                    .iter()
                    .all(|&(_, ((mission, state), _))| mission == Some(Attack)
                        && matches!(state, Some(1 | 10))),
                "{moves:?}"
            );
            assert_eq!(
                attack.iter().any(|&(_, ((_, state), _))| state == Some(1)),
                held,
                "{moves:?}"
            );
            for pair in attack.windows(2) {
                if pair[0].1.0.1 == Some(1) {
                    assert_eq!(pair[1].1, ((Some(Attack), Some(10)), None), "{moves:?}");
                }
            }
            assert_eq!(moves[retreat].1, ((Some(Retreat), None), None), "{moves:?}");
            assert!(
                moves[retreat..]
                    .iter()
                    .all(|&(_, (mission, _))| mission == (Some(Retreat), None)),
                "Retreat holds: {moves:?}"
            );
            let Some((_, (_, Some(NavTargetRef::Cell { rx, ry })))) = moves.get(retreat + 1) else {
                panic!("Mission_Retreat picks an edge cell: {moves:?}");
            };
            let sim = &runtime.simulation;
            assert!(
                !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (i32::from(*rx), i32::from(*ry)),
                    sim.playfield_bounds,
                    sim.resolved_terrain.as_ref(),
                ),
                "the edge cell lies past the playfield: {moves:?}"
            );
            // Move_To starts the tilt; the climb follows TiltFrames later.
            assert_eq!(transitions[0].1, 2);
            assert_eq!(transitions[1], (transitions[0].0 + tilt_frames as u64, 3));
            assert_eq!(transitions[2].1, 4);
            assert!(highest >= altitude, "the climb reaches the cruise altitude");
            let center = [
                i32::from(target_cell.0) * 256 + 128,
                i32::from(target_cell.1) * 256 + 128,
            ];
            assert!(
                (impact.x - center[0]).abs() <= 384 && (impact.y - center[1]).abs() <= 384,
                "the missile comes down on the tank"
            );
            assert!(health < strength, "the impact damages the target");
            for name in ["V3TAKOFF", "V3TRAIL"] {
                assert!(anim_names.contains(name), "{name} puffs are constructed");
            }
        }
    }

    /// Open water for a Dreadnought (its 5x5 neighbourhood all water, with no
    /// bridge) and flat walkable In_Bounds ground `distance` cells east or
    /// west of it for a target, with the playfield around both and the path
    /// between them.
    fn naval_sites(
        scenario: &crate::headless_scenario::HeadlessScenario,
        distance: u16,
    ) -> Vec<((u16, u16), (u16, u16))> {
        let sim = scenario.sim();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let navigation = sim.path_grid().unwrap();
        let in_playfield = |x: i32, y: i32| {
            sim.map_cell_in_bounds((x as i16, y as i16))
                && crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (x, y),
                    sim.playfield_bounds,
                    Some(terrain),
                )
        };
        let open_water = |x: u16, y: u16| {
            (-2..=2).all(|dx| {
                (-2..=2).all(|dy| {
                    let (cx, cy) = (i32::from(x) + dx, i32::from(y) + dy);
                    in_playfield(cx, cy)
                        && terrain.cell(cx as u16, cy as u16).is_some_and(|cell| {
                            cell.is_water && !cell.bridge_facts.has_structural_bridge()
                        })
                })
            }) && sim.substrate.occupancy.get(x, y).is_none()
        };
        let open_ground = |x: u16, y: u16| {
            (-3..=3).all(|d| in_playfield(i32::from(x) + d, i32::from(y) + d))
                && terrain.cell(x, y).is_some_and(|cell| {
                    cell.slope_type == 0
                        && !cell.is_water
                        && !cell.bridge_facts.has_structural_bridge()
                })
                && navigation
                    .cell(x, y)
                    .is_some_and(|cell| cell.ground_walkable)
                && sim.substrate.occupancy.get(x, y).is_none()
        };
        terrain
            .cells()
            .iter()
            .filter(|cell| open_water(cell.rx, cell.ry))
            .flat_map(|cell| {
                let (x, y) = (cell.rx, cell.ry);
                [x.checked_add(distance), x.checked_sub(distance)]
                    .into_iter()
                    .flatten()
                    .filter(move |&tx| {
                        let (lo, hi) = (x.min(tx), x.max(tx));
                        (lo..=hi).all(|cx| in_playfield(i32::from(cx), i32::from(y)))
                    })
                    .map(move |tx| ((x, y), (tx, y)))
            })
            .filter(|&(_, (tx, ty))| open_ground(tx, ty))
            .collect()
    }

    /// A Dreadnought on retail Lost Lake force-attacks a Soviet MCV ashore
    /// through an ordinary command: each volley's two missiles leave 20
    /// frames apart (the SpawnTimer), every missile comes down on the MCV,
    /// and the pool rebuilds itself for a second volley (`SpawnRegenRate=`).
    /// The pieces' native comparisons are the manager's
    /// (`spawn_manager::oracle_tests`), the launch coordinate's
    /// (`tools/projectile_oracle/ifv_fire_coord.json`), the DMisl flight's
    /// (`movement::rocket_movement::tests`) and the tracker's
    /// (`kamikaze::tests`); this run claims no whole-volley native comparison.
    #[test]
    #[ignore = "requires the configured retail install and stock Lostlake.mmx"]
    fn retail_dreadnought_volleys_land_on_the_target_and_regenerate() {
        let retail = std::env::var("RA2_DIR")
            .map(std::path::PathBuf::from)
            .expect("RA2_DIR names the retail install");
        let mut scenario = crate::headless_scenario::load(&retail, "Lostlake.mmx", 0x0B21_D6E5)
            .expect("load retail Lost Lake through the production loader");
        let candidates = naval_sites(&scenario, 15);
        let owner = scenario.sim().session.current_house.expect("launch house");
        let owner_name = scenario.sim().interner.resolve(owner).to_owned();
        let runtime = &mut scenario.runtime;
        let rules = &runtime.resources.rules;
        let regen_rate = rules.object("DRED").expect("[DRED]").spawn_regen_rate as u64;
        let sim = &mut runtime.simulation;
        let ((dred_cell, target_cell), dred, target) = candidates
            .into_iter()
            .find_map(|(launcher, target)| {
                let mcv = sim.spawn_object("SMCV", &owner_name, target.0, target.1, 0, rules)?;
                match sim.spawn_object("DRED", &owner_name, launcher.0, launcher.1, 0, rules) {
                    Some(dred) => Some(((launcher, target), dred, mcv)),
                    None => {
                        sim.uninit(mcv);
                        None
                    }
                }
            })
            .expect("Lost Lake admits a Dreadnought on open water and an MCV ashore");
        sim.resolve_type_handles(rules);
        let strength = sim.substrate.entities.get(target).unwrap().health.current;
        let dred_start = position_world_coord(&sim.substrate.entities.get(dred).unwrap().position);
        // (launch frame, slot, missile), and each missile's detonation frame
        // and last location.
        let mut launches = Vec::new();
        let mut landed = std::collections::BTreeMap::new();
        let mut flying = std::collections::BTreeMap::new();
        let mut health_log = vec![(0, strength)];
        for frame in 0..2400u64 {
            let commands = if frame == 0 {
                vec![CommandEnvelope::new(
                    owner,
                    runtime.simulation.session.tick + 1,
                    Command::ForceAttack {
                        attacker_id: dred,
                        target_id: target,
                    },
                )]
            } else {
                Vec::new()
            };
            runtime
                .advance_frame_for_tooling(&commands, SIM_TICK_MS)
                .expect("advance production frame");
            let sim = &runtime.simulation;
            let slots = sim
                .substrate
                .entities
                .get(dred)
                .and_then(|e| e.spawn_manager.as_ref())
                .map(|m| m.slots.clone())
                .unwrap_or_default();
            for (slot, missile) in slots
                .iter()
                .enumerate()
                .filter_map(|(slot, s)| Some((slot, s.spawn?)))
            {
                let out = sim
                    .substrate
                    .entities
                    .get(missile)
                    .is_some_and(|e| !e.lifecycle.in_limbo);
                if out && !flying.contains_key(&missile) && !landed.contains_key(&missile) {
                    launches.push((frame, slot, missile));
                    flying.insert(missile, None);
                }
            }
            for (&missile, last) in flying.iter_mut() {
                match sim
                    .substrate
                    .entities
                    .get(missile)
                    .filter(|e| e.is_object_alive())
                {
                    Some(e) => *last = Some(position_world_coord(&e.position)),
                    None => {
                        landed.insert(missile, (frame, *last));
                    }
                }
            }
            flying.retain(|missile, _| !landed.contains_key(missile));
            let health = sim
                .substrate
                .entities
                .get(target)
                .filter(|e| e.is_object_alive())
                .map_or(0, |e| e.health.current);
            if health_log.last().is_some_and(|&(_, last)| last != health) {
                health_log.push((frame, health));
            }
            if health == 0 || landed.len() >= 4 {
                break;
            }
        }
        let sim = &runtime.simulation;
        let dred_end = position_world_coord(&sim.substrate.entities.get(dred).unwrap().position);
        println!(
            "DRED at {dred_cell:?}, SMCV at {target_cell:?}: launches {launches:?}, \
             landings {landed:?}, target health {health_log:?}, DRED moved {}",
            (dred_start.x, dred_start.y) != (dred_end.x, dred_end.y)
        );
        assert!(launches.len() >= 4, "two volleys launch: {launches:?}");
        for volley in launches.chunks(2).take(2) {
            assert_eq!(
                (volley[0].1, volley[1].1, volley[1].0 - volley[0].0),
                (0, 1, 20),
                "slot 0 then slot 1, 20 frames apart: {launches:?}"
            );
        }
        assert!(
            launches[2].0 - launches[1].0 >= regen_rate,
            "the second volley waits out the regeneration: {launches:?}"
        );
        let center = [
            i32::from(target_cell.0) * 256 + 128,
            i32::from(target_cell.1) * 256 + 128,
        ];
        for &(frame, location) in landed.values() {
            let location = location.expect("each missile flew");
            assert!(
                (location.x - center[0]).abs() <= 512 && (location.y - center[1]).abs() <= 512,
                "each missile comes down on the MCV at frame {frame}: {location:?}"
            );
        }
        let mut landing_frames: Vec<u64> = landed.values().map(|&(frame, _)| frame).collect();
        landing_frames.sort_unstable();
        let damage_frames: Vec<u64> = health_log[1..].iter().map(|&(frame, _)| frame).collect();
        assert_eq!(
            damage_frames, landing_frames,
            "each detonation damages the target in its own frame: {health_log:?}"
        );
        assert_eq!(
            (dred_start.x, dred_start.y),
            (dred_end.x, dred_end.y),
            "the Dreadnought fires from where it stands"
        );
    }

    /// Two enemy Flak Tracks beside a V3 Launcher on retail Hills shoot its
    /// rocket down in flight, through an ordinary command and bound runtime
    /// frames. The rocket is a target only once the climb has put it in the
    /// AircraftTracker. The killing hit plays its `Explosion=` anim, and
    /// Crash keeps the rocket alive with no Health and drops it from every
    /// Flak Track (`AircraftClass::ReceiveDamage @ 0x004165C0`). The next
    /// frame's Process tail (`0x00662D74..0x00662FE1`) detonates it in the
    /// air, short of the target. The kill does not touch the launcher: its
    /// slot was freed when the rocket's KamikazeWait ran out, and it rebuilds
    /// the rocket `SpawnRegenRate=` after that. The pieces' evidence stays
    /// with their owners: the scan (`combat::greatest_threat`), the crash
    /// (`tools/spatial_oracle/aircraft_crash.py`), the flight
    /// (`movement::rocket_movement::tests`) and the manager
    /// (`spawn_manager::oracle_tests`). This run claims no
    /// whole-interception native comparison.
    #[test]
    #[ignore = "requires the configured retail install and stock Hills.mmx"]
    fn retail_flak_tracks_shoot_down_a_v3_rocket_in_flight() {
        use crate::sim::spawn_manager::SpawnSlotState;
        use crate::skirmish_launch::{
            AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, PreFillHouseRoster,
            SkirmishAiSlot,
        };
        let retail = std::env::var("RA2_DIR")
            .map(std::path::PathBuf::from)
            .expect("RA2_DIR names the retail install");
        let mut session = crate::headless_scenario::battle_session("Hills.mmx");
        session.opponents.push(SkirmishAiSlot {
            country: LaunchCountry::Russia,
            country_random: false,
            color_index: 1,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty: AiDifficulty::Easy,
        });
        session.pre_fill_house_roster = PreFillHouseRoster::from_compact_skirmish(1);
        let launch =
            crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(session).unwrap();
        let mut scenario =
            crate::headless_scenario::load_with_launch(&retail, "Hills.mmx", 0x0B21_D6E5, launch)
                .expect("load retail Hills with a computer opponent");
        let candidates = sites(&scenario);
        let owner = scenario.sim().session.current_house.expect("launch house");
        let owner_name = scenario.sim().interner.resolve(owner).to_owned();
        let enemy_name = scenario
            .sim()
            .houses
            .iter()
            .find(|(name, house)| **name != owner && !house.multiplay_passive)
            .map(|(name, _)| scenario.sim().interner.resolve(*name).to_owned())
            .expect("the computer house");
        let runtime = &mut scenario.runtime;
        let rules = &runtime.resources.rules;
        let regen_rate = rules.object("V3").expect("[V3]").spawn_regen_rate as u64;
        let explosions: BTreeSet<String> = rules
            .object("V3ROCKET")
            .expect("[V3ROCKET]")
            .explosion_anims
            .iter()
            .map(|name| name.to_ascii_uppercase())
            .collect();
        let sim = &mut runtime.simulation;
        // The first site that admits the V3, its target and both Flak Tracks,
        // which stand halfway along the path, two cells either side of it.
        let ((launcher_cell, target_cell), v3, target, flak) = candidates
            .into_iter()
            .find_map(|(launcher, target)| {
                let mut placed = Vec::new();
                let tank = sim.spawn_object("MTNK", &enemy_name, target.0, target.1, 0, rules);
                placed.extend(tank);
                let v3 = sim.spawn_object("V3", &owner_name, launcher.0, launcher.1, 0, rules);
                placed.extend(v3);
                let flak: Vec<_> = [launcher.1 - 2, launcher.1 + 2]
                    .into_iter()
                    .filter_map(|y| {
                        sim.spawn_object("HTK", &enemy_name, launcher.0 + 5, y, 0, rules)
                    })
                    .collect();
                placed.extend(flak.iter().copied());
                match (tank, v3, flak.len()) {
                    (Some(tank), Some(v3), 2) => Some(((launcher, target), v3, tank, flak)),
                    _ => {
                        for id in placed {
                            sim.uninit(id);
                        }
                        None
                    }
                }
            })
            .expect("Hills admits a V3, its target and two Flak Tracks");
        sim.resolve_type_handles(rules);
        let strength = sim.substrate.entities.get(target).unwrap().health.current;
        let target_z =
            position_world_coord(&sim.substrate.entities.get(target).unwrap().position).z;
        let missile = sim
            .substrate
            .entities
            .get(v3)
            .and_then(|e| e.spawn_manager.as_ref())
            .and_then(|m| m.slots[0].spawn)
            .expect("the V3 builds its missile");
        let mut seen_anims: BTreeSet<_> = sim.anims().map(|(&id, _)| id).collect();
        let mut tracked = None;
        let mut freed = None;
        let mut acquired = None;
        let mut killed = None;
        let mut detonated = None;
        let mut refilled = None;
        let mut death_anims = Vec::new();
        let mut detonation_anims = Vec::new();
        let mut last_location = None;
        for frame in 0..1500u64 {
            let commands = if frame == 0 {
                vec![CommandEnvelope::new(
                    owner,
                    runtime.simulation.session.tick + 1,
                    Command::ForceAttack {
                        attacker_id: v3,
                        target_id: target,
                    },
                )]
            } else {
                Vec::new()
            };
            runtime
                .advance_frame_for_tooling(&commands, SIM_TICK_MS)
                .expect("advance production frame");
            let sim = &runtime.simulation;
            let new_anims: Vec<(String, crate::sim::anim_class::AnimWorldCoord)> = sim
                .anims()
                .filter(|(id, _)| !seen_anims.contains(*id))
                .map(|(_, anim)| {
                    (
                        sim.interner.resolve(anim.type_id).to_ascii_uppercase(),
                        anim.world_coord,
                    )
                })
                .collect();
            seen_anims.extend(sim.anims().map(|(&id, _)| id));
            let slot = sim
                .substrate
                .entities
                .get(v3)
                .and_then(|e| e.spawn_manager.as_ref())
                .map(|m| m.slots[0].clone())
                .expect("the V3's slot");
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().health.current,
                strength,
                "the rocket never reaches the target"
            );
            if slot.state == SpawnSlotState::Regenerating {
                freed.get_or_insert(frame);
            }
            if detonated.is_some() {
                if slot.state == SpawnSlotState::ReadyDocked && slot.spawn.is_some() {
                    refilled = Some(frame);
                    break;
                }
                continue;
            }
            let Some(entity) = sim
                .substrate
                .entities
                .get(missile)
                .filter(|entity| entity.is_object_alive())
            else {
                detonated = Some(frame);
                detonation_anims = new_anims;
                continue;
            };
            last_location = Some(position_world_coord(&entity.position));
            let targeted = flak.iter().any(|&id| {
                sim.substrate.entities.get(id).is_some_and(|flak| {
                    flak.attack_target.as_ref().map(|attack| attack.target)
                        == Some(crate::sim::combat::TargetKind::Entity(missile))
                })
            });
            if entity.air_spatial_bucket().is_some() {
                tracked.get_or_insert(frame);
            }
            if targeted && acquired.is_none() {
                assert!(
                    tracked.is_some(),
                    "a Flak Track targets the rocket only once it is in the AircraftTracker"
                );
                acquired = Some(frame);
            }
            if entity.health.current == 0 && killed.is_none() {
                killed = Some(frame);
                death_anims = new_anims;
                assert!(entity.crashing, "Crash keeps the dead rocket alive");
                assert!(
                    flak.iter().all(|&id| sim
                        .substrate
                        .entities
                        .get(id)
                        .is_none_or(|flak| flak.attack_target.is_none())),
                    "Crash's Detach_All drops the rocket from every Flak Track"
                );
            }
        }
        println!(
            "V3 at {launcher_cell:?}, MTNK at {target_cell:?}, flak {flak:?}: tracked {tracked:?}, \
             slot freed {freed:?}, acquired {acquired:?}, killed {killed:?}, detonated {detonated:?} at \
             {last_location:?}, refilled {refilled:?}; death anims {death_anims:?}, \
             detonation anims {detonation_anims:?}"
        );
        let (tracked, acquired) = (
            tracked.unwrap(),
            acquired.expect("a Flak Track acquires it"),
        );
        let killed = killed.expect("the Flak Tracks shoot the rocket down");
        assert!(tracked <= acquired && acquired < killed);
        assert_eq!(
            detonated,
            Some(killed + 1),
            "the next frame's Process detonates the dead rocket"
        );
        assert!(
            death_anims
                .iter()
                .any(|(name, _)| explosions.contains(name)),
            "the killing hit plays an `Explosion=` anim: {death_anims:?}"
        );
        let location = last_location.unwrap();
        assert!(
            detonation_anims
                .iter()
                .any(|(_, coord)| (coord.x - location.x).abs() <= 256
                    && (coord.y - location.y).abs() <= 256
                    && coord.z > target_z + 128),
            "the rocket explodes in the air where it fell: {detonation_anims:?}"
        );
        let freed = freed.expect("the slot frees");
        assert!(freed < killed, "the slot frees before the kill");
        let refilled = refilled.expect("the slot rebuilds its rocket");
        assert!(
            (freed + regen_rate..freed + regen_rate + 20).contains(&refilled),
            "the rocket is rebuilt at the first manager pass after SpawnRegenRate"
        );
    }
}
