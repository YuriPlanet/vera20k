//! Builtin Ares WarheadExt+88 effects, with one TechnoExt+20 instance owner.
//! Installed10058520/10058E00/100588E0/10042C30; see docs/ares-attach-effect.md.
use super::GameEntity;
use crate::sim::intern::InternedId;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct AttachedEffect {
    warhead: InternedId,
    animation: Option<u64>,
    remaining: i32,
    source_house: Option<InternedId>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct AttachEffects {
    entries: Vec<AttachedEffect>,
    animations_hidden: bool,
}

impl GameEntity {
    pub(crate) fn attached_effect_anims(&self) -> impl Iterator<Item = u64> + '_ {
        self.attach_effects
            .entries
            .iter()
            .filter_map(|entry| entry.animation)
    }
    pub(crate) fn attached_effect_anim_expired(&mut self, expired: u64) {
        //10058770 clears the held pointer, without restarting or erasing the effect.
        for entry in &mut self.attach_effects.entries {
            if entry.animation == Some(expired) {
                entry.animation = None;
            }
        }
    }
    pub(crate) fn hash_attach_effects<H: Hasher>(&self, hasher: &mut H) {
        if !self.attach_effects.entries.is_empty() || self.attach_effects.animations_hidden {
            b"ares-attach-effects-v1".hash(hasher);
            self.attach_effects.hash(hasher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{
        projectile::ProjectileCoord,
        world::{FrameEffects, ObjectAiCtx, Simulation},
    };

    fn scene(discard: bool) -> (Simulation, RuleSet, u64, u64) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[VehicleTypes]\n0=EMITTER\n[BuildingTypes]\n0=TOWER\n[Warheads]\n0=AE_TEST\n[Animations]\n0=VISUAL\n\
             [EMITTER]\nStrength=400\nArmor=heavy\nSpeed=6\nPrimary=PULSE\nLocomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\nMovementZone=Normal\n\
             [TOWER]\nStrength=1000\nArmor=steel\nCanPassiveAquire=no\n\
             [PULSE]\nDamage=0\nROF=1\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=AE_TEST\n\
             [INVIS]\nAirburst=yes\nInviso=yes\nAG=yes\nAA=no\n\
             [AE_TEST]\nCellSpread=.7\nVerses=3%,3%,3%,3%,3%,3%,3%,3%,3%,3%,3%\nAffectsAllies=yes\nAttachEffect.Duration=5\nAttachEffect.Animation=VISUAL\nAttachEffect.DiscardOnEntry={}\n",
            if discard { "yes" } else { "no" }
        ));
        let art = IniFile::from_str("[TOWER]\nFoundation=1x1\n[VISUAL]\nRate=1\nEnd=4\n");
        let layers = crate::rules::native_processing::RulesLayerStack::new(ini);
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
        registry.bind_anim_frame_count_for_test("VISUAL", 4);
        rules.install_art_data(registry);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        let target = sim
            .spawn_object("TOWER", "Russians", 10, 10, 0, &rules)
            .unwrap();
        let source = sim
            .spawn_object("EMITTER", "Americans", 12, 10, 0, &rules)
            .unwrap();
        sim.resolve_type_handles(&rules);
        (sim, rules, target, source)
    }

    fn hit(sim: &mut Simulation, rules: &RuleSet, target: u64, source: Option<InternedId>) {
        let coord = crate::sim::movement::ground_pose::position_world_coord(
            &sim.entities().get(target).unwrap().position,
        );
        sim.apply_ares_attach_effect_detonation(
            ProjectileCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            },
            source,
            rules.warhead("AE_TEST").unwrap(),
            rules,
        );
    }

    #[test]
    fn attach_effect_native_histories_cover_reapply_pointer_limbo_and_retirement() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/ares_attach_effect.json",
        ))
        .unwrap();
        for row in corpus["controls"].as_array().unwrap() {
            let (mut sim, rules, target, source) =
                scene(row["discard_on_entry"].as_bool().unwrap());
            let source_house = sim.entities().get(source).unwrap().owner();
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .deactivated
                .set_for_test(true);
            for step in row["steps"].as_array().unwrap() {
                match step["action"].as_str().unwrap() {
                    "prepared" => {}
                    "hit" => hit(&mut sim, &rules, target, Some(source_house)),
                    "reapply_other_source" => {
                        let other = sim.entities().get(target).unwrap().owner();
                        hit(&mut sim, &rules, target, Some(other));
                    }
                    "tick" => sim.update_ares_attach_effects(target, &rules),
                    "limbo_on" | "limbo_off" => sim
                        .substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .lifecycle
                        .set_in_limbo_for_test(step["action"] == "limbo_on"),
                    "pointer_expiry" => {
                        let held = sim.entities().get(target).unwrap().attach_effects.entries[0]
                            .animation
                            .unwrap();
                        sim.notify_anim_pointer_expired(held);
                    }
                    // These are the exact native vector boundaries exercised by
                    // the corpus; production Limbo/DTOR call the same owners.
                    "remove" => sim.limbo_ares_attach_effects(target, &rules),
                    "dtor" => sim.destroy_ares_attach_effects(target, &rules),
                    action => panic!("unknown native action {action}"),
                }
                let state = &sim.entities().get(target).unwrap().attach_effects;
                let label = format!("{} {}", row["name"], step["action"]);
                assert_eq!(
                    serde_json::json!(
                        state
                            .entries
                            .iter()
                            .map(|entry| entry.remaining)
                            .collect::<Vec<_>>()
                    ),
                    step["remaining"],
                    "{label}"
                );
                assert_eq!(
                    serde_json::json!(
                        state
                            .entries
                            .iter()
                            .map(|entry| entry.animation.is_some())
                            .collect::<Vec<_>>()
                    ),
                    step["animations"],
                    "{label}"
                );
                assert_eq!(
                    serde_json::json!(
                        state
                            .entries
                            .iter()
                            .map(|entry| entry.source_house == Some(source_house))
                            .collect::<Vec<_>>()
                    ),
                    step["owner_matches_source"],
                    "{label}"
                );
                assert_eq!(
                    serde_json::json!(
                        state
                            .entries
                            .iter()
                            .map(|entry| entry.animation.map(|id| sim
                                .anim(id)
                                .unwrap()
                                .runtime()
                                .loop_remaining))
                            .collect::<Vec<_>>()
                    ),
                    step["iterations"],
                    "{label}"
                );
                assert_eq!(
                    state.animations_hidden,
                    step["hidden"].as_bool().unwrap(),
                    "{label}"
                );
                let created: Vec<_> = sim.anims().map(|(&id, _)| id).collect();
                assert_eq!(
                    created.len() as u64,
                    step["ctor_count"].as_u64().unwrap(),
                    "{label}"
                );
                assert_eq!(
                    created
                        .iter()
                        .filter(|id| sim.pending_delete().contains_object(id))
                        .count() as u64,
                    step["uninit_count"].as_u64().unwrap(),
                    "{label}"
                );
            }
        }
    }

    #[test]
    fn attach_effect_parent_bullet_actual_ai_snapshot_and_limbo_use_one_owner() {
        use crate::sim::combat::{TargetKind, world_fire::FireVisit};
        let (mut sim, rules, target, source) = scene(false);
        sim.commit_fire_visit(
            FireVisit::Direct {
                id: source,
                target: TargetKind::Entity(target),
                weapon_index: 0,
            },
            &rules,
            None,
            FrameEffects::empty(),
        );
        let bullets: Vec<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
        assert_eq!(bullets.len(), 1);
        for id in bullets {
            sim.object_ai_visit_one(id, Some(&rules), ObjectAiCtx::default());
        }
        let victim = sim.entities().get(target).unwrap();
        assert_eq!(victim.attach_effects.entries[0].remaining, 5);
        let anim = victim.attach_effects.entries[0].animation.unwrap();
        assert_eq!(
            sim.anim(anim).unwrap().owner_house(),
            Some(sim.entities().get(source).unwrap().owner())
        );
        let attachments_before = victim.attach_effects.clone();
        // Scenario full-load deliberately reseeds this stream to zero, as
        // deserialize_scenario_rng_reset owns. Isolate that existing native
        // load transition before comparing the complete saved state hash.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "AE", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
            .unwrap()
            .sim;
        assert_eq!(
            restored.entities().get(target).unwrap().attach_effects,
            attachments_before
        );
        assert_eq!(restored.state_hash(), sim.state_hash());
        restored.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
        assert_eq!(
            restored
                .entities()
                .get(target)
                .unwrap()
                .attach_effects
                .entries[0]
                .remaining,
            4
        );
        restored.techno_limbo_with_rules(target, &rules, None);
        assert!(
            restored
                .entities()
                .get(target)
                .unwrap()
                .attach_effects
                .entries[0]
                .animation
                .is_none()
        );
        assert!(restored.pending_delete().contains_object(&anim));
        restored.update_ares_attach_effects(target, &rules);
        assert_eq!(
            restored
                .entities()
                .get(target)
                .unwrap()
                .attach_effects
                .entries[0]
                .remaining,
            4
        );
        // Ordinary UnInit and its deferred drain reach the shared Techno DTOR.
        restored.uninit_with_rules(target, &rules);
        restored.process_pending_delete_with(&rules, None, FrameEffects::empty(), 1, false);
        assert!(restored.entities().get(target).is_none());
    }
}

#[cfg(feature = "ares")]
mod receiver {
    use super::*;
    use crate::rules::{ruleset::RuleSet, warhead_type::WarheadType};
    use crate::sim::{
        anim_class::AnimWorldCoord, components::AnimClassSpawnDescriptor, world::Simulation,
    };

    impl Simulation {
        /// Bullet46920B ->100536E0 calls AE10052700 after EMP. Source is the
        /// House captured from the live firer before either effect, not a Techno.
        pub(crate) fn apply_ares_attach_effect_detonation(
            &mut self,
            impact: crate::sim::projectile::ProjectileCoord,
            source_house: Option<InternedId>,
            warhead: &WarheadType,
            rules: &RuleSet,
        ) {
            let Some(extension) = warhead.ares_rules() else {
                return;
            };
            let settings = extension.attach_effect();
            if settings.duration() == 0 {
                return;
            }
            // The first complete chain is a neutral-stat ground WH attachment.
            // Stat aggregation, decloak, Temporal and self-effects remain explicit
            // later mechanisms; never apply a misleading visual-only substitute.
            if !settings.neutral_visual_path() {
                log::debug!(
                    "Ares AttachEffect {} requires a later dependency chain",
                    warhead.id
                );
                return;
            }
            let targets = self.ares_effect_ground_targets(impact, warhead.cell_spread_f64, rules);
            for target in targets {
                let Some(victim) = self.substrate.entities.get(target) else {
                    continue;
                };
                let Some(kind) = self.interner.object_type(victim.type_ref(), rules) else {
                    continue;
                };
                let allied = source_house.is_some_and(|source| {
                    crate::sim::combat::combat_weapon::is_ally_by_object(
                        Some(self.house_alliances.as_map()),
                        &self.interner,
                        source,
                        victim.owner(),
                    )
                });
                if victim.lifecycle.in_limbo()
                    || !victim.lifecycle.object_alive()
                    || victim.health.current() <= 0
                    || !extension.can_affect_house(
                        source_house.is_some(),
                        source_house == Some(victim.owner()),
                        allied,
                        warhead.affects_allies,
                    )
                    || f64::from_bits(warhead.interaction(kind.armor_id()).multiplier().bits())
                        .abs()
                        < 0.001
                    || (!settings.penetrates_iron_curtain()
                        && crate::sim::superweapon::invulnerability::is_invulnerable(
                            victim.invulnerability.as_ref(),
                            self.session.binary_frame(),
                        ))
                {
                    continue;
                }
                let wh = self.interner.intern(&warhead.id);
                let repeated = (!settings.cumulative())
                    .then(|| {
                        self.substrate
                            .entities
                            .get(target)
                            .unwrap()
                            .attach_effects
                            .entries
                            .iter()
                            .position(|entry| entry.warhead == wh)
                    })
                    .flatten();
                if let Some(index) = repeated {
                    let victim = self.substrate.entities.get_mut(target).unwrap();
                    victim.attach_effects.entries[index].remaining = settings.duration();
                    //100585BB preserves the first source House on reapplication.
                    if settings.animation().is_some() && settings.anim_reset_on_reapply() {
                        self.retire_attached_effect_anim(target, index, rules);
                        self.create_attached_effect_anim(target, index, rules);
                    }
                } else {
                    let victim = self.substrate.entities.get_mut(target).unwrap();
                    let index = victim.attach_effects.entries.len();
                    victim.attach_effects.entries.push(AttachedEffect {
                        warhead: wh,
                        animation: None,
                        remaining: settings.duration(),
                        source_house,
                    });
                    self.create_attached_effect_anim(target, index, rules);
                }
            }
        }

        fn create_attached_effect_anim(&mut self, target: u64, index: usize, rules: &RuleSet) {
            let Some(victim) = self.substrate.entities.get(target) else {
                return;
            };
            if matches!(
                victim.cloak.as_ref().map(|cloak| cloak.state()),
                Some(1 | 2)
            ) {
                return;
            }
            let Some(entry) = victim.attach_effects.entries.get(index) else {
                return;
            };
            if entry.animation.is_some() {
                return;
            }
            let source = entry.source_house;
            let Some(name) = rules
                .warhead(self.interner.resolve(entry.warhead))
                .and_then(|wh| wh.ares_rules())
                .and_then(|extension| extension.attach_effect().animation())
            else {
                return;
            };
            let coord = crate::sim::movement::ground_pose::position_world_coord(&victim.position);
            let world = AnimWorldCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            };
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let name = self.interner.intern(name);
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(name, rx, ry, sub_x, sub_y, z)
            };
            if let Ok(anim) = self.spawn_anim_at_world(rules, descriptor, world) {
                self.set_anim_owner_object(anim, Some(target), rules);
                self.substrate.anims.get_mut(anim).unwrap().hold_loops();
                if let Some(source) = source {
                    self.substrate.set_anim_owner_house(anim, source);
                }
                if let Some(entry) = self
                    .substrate
                    .entities
                    .get_mut(target)
                    .and_then(|victim| victim.attach_effects.entries.get_mut(index))
                {
                    entry.animation = Some(anim);
                }
            }
        }

        fn retire_attached_effect_anim(&mut self, target: u64, index: usize, rules: &RuleSet) {
            let anim = self
                .substrate
                .entities
                .get_mut(target)
                .and_then(|victim| victim.attach_effects.entries.get_mut(index))
                .and_then(|entry| entry.animation.take());
            if let Some(anim) = anim {
                self.destroy_anim_with_context(anim, rules);
            }
        }

        /// Common head1004B434, before delayed EMP/power recovery1004B528.
        /// Actual actor visits own the clock; passenger/chrono/limbo skip it.
        pub(crate) fn update_ares_attach_effects(&mut self, target: u64, rules: &RuleSet) {
            let Some(victim) = self.substrate.entities.get(target) else {
                return;
            };
            if victim.lifecycle.in_limbo()
                || victim.chrono_warp_latch()
                || victim.passenger_role.inside_transport_id().is_some()
            {
                return;
            }
            if victim.attach_effects.entries.is_empty() {
                return;
            }
            let hidden = matches!(
                victim.cloak.as_ref().map(|cloak| cloak.state()),
                Some(1 | 2)
            );
            if hidden != victim.attach_effects.animations_hidden {
                let len = victim.attach_effects.entries.len();
                for index in 0..len {
                    if hidden {
                        self.retire_attached_effect_anim(target, index, rules);
                    } else {
                        self.create_attached_effect_anim(target, index, rules);
                    }
                }
                self.substrate
                    .entities
                    .get_mut(target)
                    .unwrap()
                    .attach_effects
                    .animations_hidden = hidden;
            }
            //10058EC0 decrements positive durations after the visibility work.
            let mut index = 0;
            loop {
                let Some(entry) = self
                    .substrate
                    .entities
                    .get_mut(target)
                    .and_then(|victim| victim.attach_effects.entries.get_mut(index))
                else {
                    break;
                };
                if entry.remaining > 0 {
                    entry.remaining -= 1;
                }
                if entry.remaining == 0 {
                    self.retire_attached_effect_anim(target, index, rules);
                    self.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .attach_effects
                        .entries
                        .remove(index);
                } else {
                    index += 1;
                }
            }
        }

        /// Remove6F6AC9 ->1004B140 ->100588E0. Ordinary attachments survive
        /// boarding with a paused duration; DiscardOnEntry removes the entry.
        pub(crate) fn limbo_ares_attach_effects(&mut self, target: u64, rules: &RuleSet) {
            let Some(victim) = self.substrate.entities.get(target) else {
                return;
            };
            if victim.attach_effects.entries.is_empty() {
                return;
            }
            let len = victim.attach_effects.entries.len();
            self.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .attach_effects
                .animations_hidden = true;
            for index in 0..len {
                self.retire_attached_effect_anim(target, index, rules);
            }
            let interner = &self.interner;
            self.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .attach_effects
                .entries
                .retain(|entry| {
                    !rules
                        .warhead(interner.resolve(entry.warhead))
                        .and_then(|wh| wh.ares_rules())
                        .is_some_and(|extension| extension.attach_effect().discard_on_entry())
                });
        }

        /// TechnoDTOR6F4500 ->10046FA0 ->10042C30, before the stock voice tail.
        pub(crate) fn destroy_ares_attach_effects(&mut self, target: u64, rules: &RuleSet) {
            let len = self
                .substrate
                .entities
                .get(target)
                .map_or(0, |victim| victim.attach_effects.entries.len());
            for index in 0..len {
                self.retire_attached_effect_anim(target, index, rules);
            }
            if let Some(victim) = self.substrate.entities.get_mut(target) {
                victim.attach_effects.entries.clear();
            }
        }
    }
}
