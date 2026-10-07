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
mod retail_tests {
    //! Production composition witness: a stock V3 Launcher on retail Hills
    //! force-attacks a tank through an ordinary command and bound runtime
    //! frames. The flight's native comparison is
    //! `movement::rocket_movement::tests`; this run claims no native
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
    fn sites(
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
                let tank = sim.spawn_object("MTNK", &owner_name, target.0, target.1, 0, rules)?;
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
        for frame in 0..1500 {
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
             states {transitions:?}, highest Z {highest}, detonation frame {detonated} \
             near {impact:?}, target health {strength} -> {health}, anims {anim_names:?}"
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
