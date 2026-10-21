//! Ordinary stock MIND/PsychicControl Bullet46920B -> CaptureManager471D40
//! -> overload471A50/471C78 -> next real Techno RockingUpdate70B570.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use crate::util::fixed_math::SimFixed;
use std::collections::BTreeSet;

pub struct MindScene {
    rules: std::sync::Arc<RuleSet>,
    sim: Simulation,
    mind: u64,
    targets: [u64; 4],
}

#[cfg(any(test, feature = "test-support"))]
impl MindScene {
    pub fn retail() -> Option<Self> {
        let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let object = rules.object("MIND").unwrap();
        assert_eq!(object.primary(), Some("MultipleMindControlTank"));
        assert!(rules.object_has_loaded_voxel(rules.type_handle("MIND").unwrap()));
        let weapon = rules.weapon("MultipleMindControlTank").unwrap();
        assert!(weapon.infinite_mind_control);
        assert_eq!(weapon.damage, 3);
        assert_eq!(weapon.projectile.as_deref(), Some("PsychicControl"));
        assert_eq!(weapon.warhead.as_deref(), Some("Controller"));
        assert!(rules.projectile("PsychicControl").unwrap().inviso);
        assert!(rules.warhead("Controller").unwrap().mind_control);
        assert_eq!(weapon.range_leptons, 6 * 256);
        assert!(rules.weapon("105mm").unwrap().range_leptons < weapon.range_leptons);
        assert_eq!(rules.mind_control.overload_count, [3, 6, 10, 50]);
        assert_eq!(rules.mind_control.overload_damage, [0, 50, 100, 500]);
        assert_eq!(rules.bridge_warheads.c4_name, "Super");
        let super_wh = rules.warhead(&rules.bridge_warheads.c4_name).unwrap();
        assert_eq!(
            f64::from_bits(super_wh.interaction(object.armor_id()).multiplier().bits()),
            1.0
        );
        let spark = rules.combat_damage.default_spark_system.as_deref().unwrap();
        assert!(rules.ps_type_id_by_name(spark).is_some());

        let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 0x471a50, 64);
        // The arena's open LocalSize retains native Size width20 independently
        // of its rectangular terrain. Supply Size height64 for this larger
        // scene: Bullet In_Bounds568300 must admit the cardinal encounter and
        // its ordinary tank pursuit (the arena's default20 ends at x+y=60).
        // Allocated terrain/fog64x64 and native MapSize20x64 are independent.
        sim.set_playfield_size_height_for_test(Some(64));
        let native_width = sim.playfield_bounds().unwrap().base;
        for cell in [(32, 32), (32, 26), (38, 32), (32, 38), (26, 32)] {
            assert!(crate::map::playfield::size_diamond_contains(
                native_width,
                sim.playfield_size_height().unwrap(),
                cell,
            ));
        }
        // Production scenario construction allocates the whole map's fog
        // domain before Reveal/ChangeOwner. The arena supplies terrain only;
        // per-owner mark helpers must not grow mismatched partial domains.
        sim.fog = crate::sim::vision::FogState::new(64, 64);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        let mind = sim
            .spawn_object("MIND", "Americans", 32, 32, 0, &rules)
            .expect("ordinary physical Mastermind Unlimbo");
        let mut targets = Vec::new();
        // Cardinal retail Range6 puts every target in the controller's range,
        // beyond stock105mm Range5, and away from the other three captives.
        for (x, y) in [(32, 26), (38, 32), (32, 38), (26, 32)] {
            targets.push(
                sim.spawn_object("MTNK", "Russians", x, y, 0, &rules)
                    .expect("ordinary hostile tank Unlimbo"),
            );
        }
        let truck = rules.object("TRUCKA").unwrap();
        assert!(truck.primary().is_none() && truck.secondary().is_none());
        for (house, x, y) in [("Americans", 4, 4), ("Russians", 58, 58)] {
            sim.spawn_object("TRUCKA", house, x, y, 0, &rules)
                .expect("ordinary distant house survivor");
        }
        let owner = sim.entities().get(mind).unwrap().owner();
        let enemy = sim.entities().get(targets[0]).unwrap().owner();
        assert_ne!(owner, enemy);
        assert!(sim.houses.get(&owner).unwrap().is_human());
        sim.session.set_current_house_for_test(Some(owner));
        // Supplied map knowledge admits the real commands/scanner for both
        // houses; counterfire is not avoided by hiding the controller.
        sim.fog.mark_visible_for_owner(enemy, 32, 32);
        for &id in &targets {
            let actor = sim.entities().get(id).unwrap();
            sim.fog
                .mark_visible_for_owner(owner, actor.position.rx(), actor.position.ry());
            sim.queue_command(CommandEnvelope::new(
                enemy,
                1,
                Command::Guard {
                    entity_id: id,
                    target: None,
                },
            ));
        }
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::Guard {
                entity_id: mind,
                target: None,
            },
        ));
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::Attack {
                attacker_id: mind,
                target_id: targets[0],
            },
        ));
        eprintln!(
            "MIND ordinary initial Attack issue_tick={} due_tick=1 target={}",
            sim.session.tick(),
            targets[0]
        );
        Some(Self {
            rules: rules.into(),
            sim,
            mind,
            targets: targets.try_into().unwrap(),
        })
    }

    pub fn step(&mut self) -> Vec<crate::sim::world::SimFireEvent> {
        let before = self.sim.entities().get(self.mind).map(|actor| {
            (
                actor.health.current(),
                self.words(),
                actor
                    .capture_manager
                    .as_ref()
                    .map(|manager| manager.victims().count()),
            )
        });
        let commands = self.sim.take_due_commands();
        let output = self
            .sim
            .advance_app_frame(
                &commands,
                &self.rules,
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary stock Mastermind frame");
        assert!(output.tick.frame_committed);
        if before.is_some_and(|(hp, _, _)| hp > 0)
            && self
                .sim
                .entities()
                .get(self.mind)
                .is_none_or(|actor| !actor.lifecycle.object_alive())
        {
            eprintln!(
                "MIND actual terminal visited_frame={} before={before:?} houses={:?} shots={:?} lifecycle={:?}",
                self.sim.session.binary_frame() - 1,
                self.sim
                    .houses
                    .iter()
                    .map(|(id, house)| (self.sim.interner.resolve(*id), house.is_defeated()))
                    .collect::<Vec<_>>(),
                output
                    .fire_events
                    .iter()
                    .map(|shot| (
                        shot.attacker_id,
                        self.sim.interner.resolve(shot.weapon_id),
                        shot.target
                    ))
                    .collect::<Vec<_>>(),
                output.lifecycle_outputs
            );
        }
        output.fire_events
    }

    pub fn words(&self) -> [u32; 4] {
        self.sim
            .entities()
            .get(self.mind)
            .unwrap()
            .rocking
            .as_ref()
            .map_or([0; 4], |state| state.native_bits())
    }

    pub fn victims(&self) -> Vec<u64> {
        self.sim
            .entities()
            .get(self.mind)
            .unwrap()
            .capture_manager
            .as_ref()
            .unwrap()
            .victims()
            .collect()
    }

    pub fn capture_four_through_real_attacks(&mut self) {
        self.capture_four_observing(|_| {});
    }

    pub fn capture_four_observing(&mut self, mut observe: impl FnMut(&Self)) {
        let mut fired_at = BTreeSet::new();
        let mut requested = self.targets[0];
        for _ in 0..600 {
            let shots = self.step();
            for shot in shots.iter().filter(|shot| shot.attacker_id == self.mind) {
                assert_eq!(
                    self.sim.interner.resolve(shot.weapon_id),
                    "MultipleMindControlTank"
                );
                assert_eq!(shot.weapon_slot, WeaponSlot::Primary);
                let TargetKind::Entity(target) = shot.target else {
                    panic!("stock mind control must have a real tank target");
                };
                assert!(self.targets.contains(&target));
                fired_at.insert(target);
                eprintln!(
                    "MIND ordinary Fire visited_frame={} tick={} target={} fire={:?}",
                    self.sim.session.binary_frame() - 1,
                    self.sim.session.tick(),
                    target,
                    shot.fire_coord
                );
            }
            let controller = self.sim.entities().get(self.mind).unwrap();
            assert!(controller.lifecycle.object_alive());
            assert_eq!(
                controller.health.current(),
                self.rules.object("MIND").unwrap().strength,
                "ordinary capture prerequisites must avoid counterfire before overload"
            );
            let owner = controller.owner();
            let victims = self.victims();
            for &id in &victims {
                let captive = self.sim.entities().get(id).unwrap();
                assert!(captive.lifecycle.object_alive() && captive.health.current() > 0);
                assert_eq!(captive.owner(), owner);
                assert_eq!(captive.mind_control.controller(), Some(self.mind));
                assert!(
                    fired_at.contains(&id),
                    "real selected FireAt must precede CaptureUnit"
                );
            }
            observe(self);
            if victims.contains(&requested) {
                eprintln!(
                    "MIND ordinary capture visited_frame={} tick={} requested={} nodes={victims:?}",
                    self.sim.session.binary_frame() - 1,
                    self.sim.session.tick(),
                    requested
                );
            }
            if victims.len() == 4 {
                assert_eq!(fired_at.len(), 4);
                assert!(self.targets.iter().all(|id| victims.contains(id)));
                self.sim.queue_command(CommandEnvelope::new(
                    owner,
                    self.sim.session.tick() + 1,
                    Command::Stop {
                        entity_id: self.mind,
                    },
                ));
                return;
            }
            if victims.contains(&requested) {
                requested = *self
                    .targets
                    .iter()
                    .find(|id| !victims.contains(id))
                    .unwrap();
                eprintln!(
                    "MIND ordinary next Attack issue_tick={} due_tick={} target={requested}",
                    self.sim.session.tick(),
                    self.sim.session.tick() + 1
                );
                self.sim.queue_command(CommandEnvelope::new(
                    owner,
                    self.sim.session.tick() + 1,
                    Command::Attack {
                        attacker_id: self.mind,
                        target_id: requested,
                    },
                ));
            }
        }
        panic!(
            "ordinary stock Mastermind did not reach four real captures: fired={fired_at:?}, victims={:?}",
            self.victims()
        );
    }

    pub fn select_controller(&mut self) {
        let owner = self.sim.entities().get(self.mind).unwrap().owner();
        self.sim.queue_command(CommandEnvelope::new(
            owner,
            self.sim.session.tick() + 1,
            Command::Select {
                entity_ids: vec![self.mind],
                additive: false,
            },
        ));
    }

    pub fn simulation(&self) -> &Simulation {
        &self.sim
    }

    pub(crate) fn simulation_mut(&mut self) -> &mut Simulation {
        &mut self.sim
    }

    pub fn rules(&self) -> &std::sync::Arc<RuleSet> {
        &self.rules
    }

    pub fn mind(&self) -> u64 {
        self.mind
    }

    pub fn targets(&self) -> [u64; 4] {
        self.targets
    }

    pub fn queue_command(&mut self, command: CommandEnvelope) {
        self.sim.queue_command(command);
    }

    /// Internal Rust continuation only: explicitly rebind process streams and
    /// external resources through their existing owners, without claiming
    /// native/app SaveGame persistence.
    pub fn internal_snapshot_continuation(&self) -> Self {
        let saved =
            crate::sim::snapshot::GameSnapshot::save(&self.sim, 0, 0, "mastermind-overload", 0);
        let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
            .unwrap()
            .sim;
        loaded.main_rng = self.sim.main_rng.snapshot_for_test().into();
        loaded.scenario_rng = self.sim.scenario_rng.clone();
        loaded.mapgen_rng = self.sim.mapgen_rng.clone();
        loaded.restore_after_snapshot_load().unwrap();
        loaded.rebuild_caches_after_load(
            self.sim.resolved_terrain.clone().unwrap(),
            self.sim.terrain_speed_config().clone(),
            &self.rules,
        );
        loaded.resolve_type_handles(&self.rules);
        assert!(loaded.rebuild_dynamic_navigation(&self.rules));
        Self {
            rules: self.rules.clone(),
            sim: loaded,
            mind: self.mind,
            targets: self.targets,
        }
    }

    pub fn first_pulse(&mut self) -> u32 {
        let initial_health = self.sim.entities().get(self.mind).unwrap().health.current();
        let spark = self
            .rules
            .ps_type_id_by_name(
                self.rules
                    .combat_damage
                    .default_spark_system
                    .as_deref()
                    .unwrap(),
            )
            .unwrap();
        for _ in 0..180 {
            let before_words = self.words();
            let before: BTreeSet<_> = self
                .sim
                .particle_systems()
                .iter()
                .map(|(&id, _)| id)
                .collect();
            self.step();
            let controller = self.sim.entities().get(self.mind).unwrap();
            assert!(controller.lifecycle.object_alive() && controller.health.current() > 0);
            assert_eq!(self.victims().len(), 4);
            if controller.health.current() < initial_health {
                assert_eq!(
                    initial_health - controller.health.current(),
                    self.rules.mind_control.overload_damage[1],
                    "actual nonlethal C4 receiver loss must precede the missing rocking assertion"
                );
                let born: Vec<_> = self
                    .sim
                    .particle_systems()
                    .iter()
                    .filter(|(id, system)| !before.contains(id) && system.type_id() == spark)
                    .collect();
                assert_eq!(
                    born.len(),
                    5,
                    "the actual overload pulse must construct five live stock SparkSys objects"
                );
                assert!(born.iter().all(|(_, system)| system.alive()
                    && !system.in_limbo()
                    && system.in_logic_vector()));
                let pulse_frame = self.sim.session.binary_frame() - 1;
                eprintln!(
                    "MIND ordinary overload visited_frame={pulse_frame} tick={} health={} new_sparks={}",
                    self.sim.session.tick(),
                    controller.health.current(),
                    born.len()
                );
                let words = self.words();
                assert_eq!(
                    words[2] & 0x7fff_ffff,
                    0x3c75_c28f,
                    "native471C78 must assign row1's signed sideways RATE after the real pulse; visited_frame={}",
                    self.sim.session.binary_frame() - 1
                );
                assert_eq!(
                    [words[0], words[1], words[3]],
                    [before_words[0], before_words[1], before_words[3]],
                    "direct overload store preserves the other three native words"
                );
                self.step();
                let controller = self.sim.entities().get(self.mind).unwrap();
                assert!(controller.lifecycle.object_alive() && controller.health.current() > 0);
                assert!(
                    controller.rocking.as_ref().unwrap().angle_sideways().abs()
                        > SimFixed::lit("0.005"),
                    "the next ordinary Techno visit must expose the overload's visible roll"
                );
                return pulse_frame;
            }
        }
        panic!("four real captive nodes never produced the stock overload pulse");
    }
}

pub fn spark_ids(scene: &MindScene) -> BTreeSet<u64> {
    let ty = scene
        .rules
        .ps_type_id_by_name(
            scene
                .rules
                .combat_damage
                .default_spark_system
                .as_deref()
                .unwrap(),
        )
        .unwrap();
    scene
        .sim
        .particle_systems()
        .iter()
        .filter(|(_, system)| system.type_id() == ty)
        .map(|(&id, _)| id)
        .collect()
}

pub fn captive_ring_ids(scene: &MindScene) -> BTreeSet<crate::sim::anim_class::AnimId> {
    let name = scene.rules.mind_control.controlled_anim.as_deref().unwrap();
    scene
        .sim
        .anims()
        .filter(|(_, anim)| {
            scene.sim.interner.resolve(anim.type_id()) == name
                && anim
                    .owner_entity()
                    .is_some_and(|id| scene.targets.contains(&id))
        })
        .map(|(&id, _)| id)
        .collect()
}
