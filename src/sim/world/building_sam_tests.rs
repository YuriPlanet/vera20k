//! Legal map SAM override through real constructors, movement and Logic.
//! These are Rust integration regressions, not original whole-frame goldens.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::fire_error_world::FireSubject;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::runtime::{SimResources, SimRuntime};

struct Scene(SimRuntime);

impl Scene {
    fn new(secondary: &str) -> Option<Self> {
        Self::with_sam(secondary, true)
    }

    fn with_sam(secondary: &str, sam: bool) -> Option<Self> {
        // CanPassiveAquire=no is a declared map control: otherwise the later
        // passive block can reacquire a target that SAM just rejected.
        let patch = IniFile::from_str(&format!(
            "[NASAM]\nSAM={}\nCanPassiveAquire=no\n{secondary}",
            if sam { "yes" } else { "no" }
        ));
        let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "Hills.mmx",
            &patch,
        )?;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
        retail
            .rules
            .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
        retail
            .rules
            .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
        let registry =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let patriot = rules.object("NASAM").unwrap();
        assert_eq!((patriot.ammo, patriot.initial_ammo), (-1, -1));
        assert!(!patriot.can_passive_acquire);
        assert_eq!(patriot.primary(), Some("RedEye2"));
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x44ad07);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.set_input_delay_ticks(0);
        let owner = sim.interner().get("Americans").unwrap();
        let rival = sim.interner().get("Russians").unwrap();
        sim.houses_mut_for_test()
            .get_mut(&rival)
            .unwrap()
            .set_is_human_for_test(true);
        sim.houses_mut_for_test()
            .get_mut(&rival)
            .unwrap()
            .set_current_iq_for_test(0);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        sim.fog_mut_for_test().reveal_all_for_owner(rival);
        // Clear arena, human roster and visibility are supplied match inputs.
        for (kind, house, x, y) in [
            ("GAPOWR", "Americans", 4, 24),
            ("NAPOWR", "Russians", 28, 28),
        ] {
            sim.spawn_object_with_overlay_registry(kind, house, x, y, 0, &rules, &registry)
                .unwrap();
        }
        let terrain_template = sim.resolved_terrain().cloned();
        Some(Self(SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template,
            },
        }))
    }

    fn spawn(&mut self, kind: &str, owner: &str, at: (u16, u16), facing: u8) -> u64 {
        self.0
            .simulation
            .spawn_object_with_overlay_registry(
                kind,
                owner,
                at.0,
                at.1,
                facing,
                &self.0.resources.rules,
                &self.0.resources.overlay_registry,
            )
            .expect("retail constructor and ordinary Unlimbo")
    }

    fn order(&mut self, command: Command) {
        let sim = &mut self.0.simulation;
        // An admitted human event sender, including for the enemy-owned
        // flight fixture; the ordinary receiver has no actor-owner gate.
        let owner = sim.interner().get("Americans").unwrap();
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session().tick() + 1,
            command,
        ));
    }

    fn frame(&mut self) -> SimFrameOutput {
        let commands = self.0.simulation.take_due_commands();
        let output = self
            .0
            .advance_frame(&commands, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        output
    }

    fn high(&self, id: u64) -> bool {
        crate::sim::movement::air_movement::is_high_flying(
            self.0.simulation.entities().get(id).unwrap(),
            self.0.simulation.resolved_terrain(),
            Some((&self.0.resources.rules, self.0.simulation.interner())),
        )
    }

    fn error(&self, source: u64, target: u64, slot: i32) -> FireError {
        let sim = &self.0.simulation;
        FireSubject {
            world: sim,
            rules: &self.0.resources.rules,
            overlay_registry: Some(&self.0.resources.overlay_registry),
            fog: Some(sim.fog.fire_view(sim.house_alliances.as_map())),
            firer: sim.entities().get(source).unwrap(),
            obj: self.0.resources.rules.object("NASAM").unwrap(),
            target: Some(crate::sim::combat::TargetKind::Entity(target)),
            weapon_index: slot,
        }
        .fire_error(true)
    }

    fn attack(&mut self, source: u64, target: u64) {
        // Supply the command-delivery boundary immediately before Logic.
        // The real target setter and mission queue/Commence remain in use.
        assert!(self.0.apply_command_for_test(
            "Americans",
            &Command::Attack {
                attacker_id: source,
                target_id: target,
            }
        ));
        assert_eq!(
            self.0
                .simulation
                .entities()
                .get(source)
                .unwrap()
                .attack_target
                .target(),
            Some(crate::sim::combat::TargetKind::Entity(target))
        );
    }

    fn high_rocketeer(&mut self) -> u64 {
        let flyer = self.spawn("JUMPJET", "Russians", (24, 16), 0);
        assert_eq!(
            self.0
                .simulation
                .entities()
                .get(flyer)
                .unwrap()
                .position
                .z(),
            0
        );
        self.order(Command::Move {
            entity_id: flyer,
            target_rx: 16,
            target_ry: 16,
        });
        let mut held = false;
        for _ in 0..600 {
            self.frame();
            let actor = self.0.simulation.entities().get(flyer).unwrap();
            let flight = actor.locomotor.as_ref().unwrap().jumpjet_runtime().unwrap();
            let destination = flight.destination();
            // Phase2 can occur briefly before arrival. The real Infantry
            // destination is its chosen subcell, not necessarily cell center.
            held = (actor.position.rx(), actor.position.ry()) == (16, 16)
                && crate::sim::movement::ground_pose::position_world_xy(&actor.position)
                    == [destination.x, destination.y]
                && flight.phase() == 2
                && self.high(flyer);
            if held {
                break;
            }
        }
        assert!(held, "real Move must finish at its chosen high hold point");
        let actor = self.0.simulation.entities().get(flyer).unwrap();
        assert!(actor.lifecycle.cell_marked() && actor.is_object_alive());
        assert_eq!(actor.category(), EntityCategory::Infantry);
        assert!(
            self.0
                .resources
                .rules
                .object("JUMPJET")
                .unwrap()
                .considered_aircraft
        );
        flyer
    }
}

#[test]
fn sam_rejects_a_real_high_rocketeer_that_the_ordinary_aa_query_accepts() {
    let Some(mut ordinary) = Scene::with_sam("", false) else {
        return;
    };
    let control_target = ordinary.high_rocketeer();
    let control_pose = crate::sim::movement::ground_pose::object_location(
        ordinary
            .0
            .simulation
            .entities()
            .get(control_target)
            .unwrap(),
        ordinary.0.simulation.resolved_terrain(),
    );
    let control_gun = ordinary.spawn("NASAM", "Americans", (16, 24), 0);
    ordinary.attack(control_gun, control_target);
    let mut ordinary_shot = false;
    for _ in 0..120 {
        let output = ordinary.frame();
        if let Some(shot) = output
            .fire_events
            .iter()
            .find(|shot| shot.attacker_id == control_gun)
        {
            assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
            assert_eq!(
                shot.target,
                crate::sim::combat::TargetKind::Entity(control_target)
            );
            assert_eq!(
                ordinary.0.simulation.interner().resolve(shot.weapon_id),
                "RedEye2"
            );
            ordinary_shot = true;
            break;
        }
    }
    // Ordinary Attack's real facing/voxel retry and GetFireError must admit
    // the shot. Constructor byte0 need not match the chosen subcell exactly.
    assert!(
        ordinary_shot,
        "SAM=no must reach real AA admission and FireAt"
    );
    let Some(mut scene) = Scene::new("") else {
        return;
    };
    let flyer = scene.high_rocketeer();
    assert_eq!(
        crate::sim::movement::ground_pose::object_location(
            scene.0.simulation.entities().get(flyer).unwrap(),
            scene.0.simulation.resolved_terrain(),
        ),
        control_pose
    );
    let gun = scene.spawn("NASAM", "Americans", (16, 24), 0);
    scene.attack(gun, flyer);
    let output = scene.frame();
    assert!(
        !output
            .fire_events
            .iter()
            .any(|shot| shot.attacker_id == gun)
    );
    let gun = scene.0.simulation.entities().get(gun).unwrap();
    assert_eq!(
        gun.attack_target.target(),
        None,
        "SAM state0 rejects RTTI15 despite high flight"
    );
    assert_eq!(gun.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(gun.mission.handler_state(), 0);
    assert_eq!(gun.rearm_timer.duration(), 0);
}

#[test]
fn sam_turns_for_a_real_aircraft_then_fires_the_two_authored_slots_in_order() {
    let Some(mut scene) = Scene::new("Secondary=RedEye2\n") else {
        return;
    };
    assert_eq!(
        scene.0.resources.rules.object("NASAM").unwrap().secondary(),
        Some("RedEye2")
    );
    let plane = scene.spawn("ORCA", "Russians", (16, 16), 0);
    assert_eq!(
        scene
            .0
            .simulation
            .entities()
            .get(plane)
            .unwrap()
            .position
            .z(),
        0
    );
    // Radial flight keeps its bearing stable while the native SAM rotates.
    // Stop at a real high-flight observation, before the distant arrival.
    scene.order(Command::Move {
        entity_id: plane,
        target_rx: 16,
        target_ry: 4,
    });
    let mut high = false;
    for _ in 0..240 {
        scene.frame();
        high = scene.high(plane);
        if high {
            break;
        }
    }
    assert!(high, "ordinary Aircraft Move must take off");
    assert_eq!(
        scene.0.simulation.entities().get(plane).unwrap().category(),
        EntityCategory::Aircraft
    );
    let gun = scene.spawn("NASAM", "Americans", (16, 24), 64);
    assert_eq!(scene.error(gun, plane, 0), FireError::Facing);
    scene.attack(gun, plane);
    let mut turned = false;
    let mut state1 = false;
    let mut fired = false;
    for _ in 0..120 {
        let actor = scene.0.simulation.entities().get(gun).unwrap();
        let before_state = actor.mission.handler_state();
        let before_error = scene.error(gun, plane, 0);
        turned |= actor
            .body_facing
            .is_rotating(scene.0.simulation.session().binary_frame());
        state1 |= before_state == 1;
        let output = scene.frame();
        let shots = output
            .fire_events
            .iter()
            .filter(|shot| shot.attacker_id == gun)
            .collect::<Vec<_>>();
        if shots.is_empty() {
            continue;
        }
        assert!(
            turned && state1,
            "actual state0 turning must precede state1 fire"
        );
        assert_eq!((before_state, before_error), (1, FireError::Ok));
        assert_eq!(
            shots
                .iter()
                .map(|shot| shot.weapon_slot)
                .collect::<Vec<_>>(),
            [WeaponSlot::Primary, WeaponSlot::Secondary]
        );
        assert!(shots.iter().all(|shot| shot.target
            == crate::sim::combat::TargetKind::Entity(plane)
            && scene.0.simulation.interner().resolve(shot.weapon_id) == "RedEye2"));
        let actor = scene.0.simulation.entities().get(gun).unwrap();
        assert_eq!(actor.mission.handler_state(), 0);
        assert!((55..=57).contains(&actor.rearm_timer.duration()));
        fired = true;
        break;
    }
    assert!(
        fired,
        "state1 must reach real slot0 FireError and both synchronous FireAt calls"
    );
}
