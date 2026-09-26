//! Infantry terminal lifetime, separate from sprite animation progress.
//!
//! VERA-internal compatibility policy, gamemd equivalent UNCHECKED. Preserve
//! the existing effect recipe and delivery order while correcting indefinite
//! retention of effect-only corpses. Native InfantryClass::ReceiveDamage
//! (0x00517FA0) has additional selectors/particle operations not implemented here.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::{Animation, LoopMode, SequenceKind, advance_animation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum InfantryDeathSequence {
    Die1,
    Die2,
}

impl InfantryDeathSequence {
    fn for_inf_death(inf_death: u8) -> Option<Self> {
        match inf_death {
            1 => Some(Self::Die1),
            2 => Some(Self::Die2),
            _ => None,
        }
    }
    fn animation(self) -> SequenceKind {
        match self {
            Self::Die1 => SequenceKind::Die1,
            Self::Die2 => SequenceKind::Die2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum InfantryTerminal {
    RetireNextVisit,
    Sequence(InfantryDeathSequence),
    /// The receiver's consuming DamageConsequences owns the cleanup boundary.
    AwaitingConsequences,
}

enum ReceiverDeathRecipe {
    Sequence,
    ExternalAnim(u8),
    Cleanup,
    /// A `JumpJet=` type's InfantryExplode (the InfDeath=3 construction,
    /// `0x0051831D..0x0051835D`) over a crash its `FootClass::Crash`
    /// accepted: the infantryman stays, alive at Health 0.
    CrashExplode,
}

/// Captured concrete-receiver inputs after recursive DeathWeapon damage;
/// consuming this postlude cannot forget the
/// matching lifetime decision or split effect/smudge selection between callers.
#[must_use]
pub(crate) struct InfantryDeathPostlude {
    id: u64,
    position: crate::sim::components::Position,
    world_z_leptons: i32,
    recipe: ReceiverDeathRecipe,
}

impl InfantryDeathPostlude {
    pub(crate) fn commit(
        self,
        world: &mut Simulation,
        rules: &RuleSet,
        effects: &mut crate::sim::combat::DeathEffects,
    ) {
        let anim = match self.recipe {
            ReceiverDeathRecipe::ExternalAnim(inf_death) => Some(inf_death),
            ReceiverDeathRecipe::CrashExplode => Some(INFANTRY_EXPLODE_INF_DEATH),
            ReceiverDeathRecipe::Sequence | ReceiverDeathRecipe::Cleanup => None,
        };
        if let Some(inf_death) = anim {
            crate::sim::combat::emit_infantry_death_anim(
                &rules.general,
                inf_death,
                self.position.rx,
                self.position.ry,
                self.position.sub_x,
                self.position.sub_y,
                self.position.z,
                self.world_z_leptons,
                &mut world.interner,
                &mut effects.explosion_effects,
            );
        }
        if matches!(self.recipe, ReceiverDeathRecipe::ExternalAnim(_)) {
            effects.immediate_uninit_ids.push(self.id);
        }
    }
}

/// The InfDeath arm whose anim is `[General] InfantryExplode=`: a `JumpJet=`
/// infantryman's death builds it whatever the warhead (`0x00518313`).
const INFANTRY_EXPLODE_INF_DEATH: u8 = 3;

impl Simulation {
    /// Select the represented concrete recipe after recursive DeathWeapon
    /// damage. Effects remain in the consuming postlude. Animation presence
    /// gates legacy effects, never an indefinite lifetime wait.
    ///
    /// A `JumpJet=` type (`+0xD94`, `0x00518313`) builds InfantryExplode
    /// whatever the warhead, ahead of the InfDeath table. A `Crashable=` one
    /// (`+0xD95`, `0x005185F1`) then crashes (`Crash(NULL)`, `0x0051860B`):
    /// accepted, it stays alive at Health 0 and falls (`world::jumpjet_cruise`,
    /// `movement::infantry_action`); refused on the ground, it is UnInit. An
    /// infantryman flown by the Jumpjet locomotor first runs the arm's
    /// Stop_Driver (`0x005180FE`) and Stun (`0x00518108`), which stop its
    /// locomotor with Scenario draws.
    ///
    /// RESIDUAL: a walker skips that Stop_Driver and Stun. Trigger: every
    /// infantry death. Effect: none observable: its Walk stop and Stun repeat
    /// the death arm's, and its death sequence or removal overwrites the Doing
    /// and cell-entry byte they write.
    pub(crate) fn begin_infantry_receiver_death(
        &mut self,
        id: u64,
        inf_death: u8,
        rules: &RuleSet,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        immediate_uninit_ids: &mut Vec<u64>,
    ) -> InfantryDeathPostlude {
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(crate::sim::movement::infantry_action::doing_owns_sequence)
        {
            if let Err(cause) = self.infantry_stop_driver(id, rules, overlay_registry) {
                log::debug!("infantry {id} death Stop_Driver: {cause}");
            }
            self.techno_death_stun(id, super::UninitContext::with_rules(rules));
        }
        // `InfantryClass::ReceiveDamage 0x0051810E..0x0051812E`, before the
        // death ladder: Queue_Mission(-1) (refused), Queue_Mission(Guard),
        // Commence. The corpse sits on Guard until it is removed.
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
            );
        }
        let now = self.session.binary_frame;
        let _ = self.mission_commence_exact(id, now);
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("fatal Infantry retained for postlude");
        debug_assert_eq!(entity.category, EntityCategory::Infantry);
        let position = entity.position.clone();
        let world_z_leptons =
            crate::sim::combat::object_world_z_leptons(entity, self.resolved_terrain.as_ref());
        let (jumpjet, crashable) = self
            .object_type(entity.type_ref(), rules)
            .map_or((false, false), |object| (object.jumpjet, object.crashable));
        let recipe = if jumpjet {
            if crashable && self.foot_crash(id, None, rules) {
                ReceiverDeathRecipe::CrashExplode
            } else {
                ReceiverDeathRecipe::ExternalAnim(INFANTRY_EXPLODE_INF_DEATH)
            }
        } else if entity.animation.is_none() {
            // The no-art cleanup belongs to this concrete receiver's postlude,
            // after any nested DeathWeapon receivers have finished.
            immediate_uninit_ids.push(id);
            ReceiverDeathRecipe::Cleanup
        } else if let Some(sequence) = InfantryDeathSequence::for_inf_death(inf_death) {
            self.begin_infantry_death_sequence(id, sequence);
            ReceiverDeathRecipe::Sequence
        } else if crate::sim::animation::inf_death_spawns_anim(inf_death) {
            ReceiverDeathRecipe::ExternalAnim(inf_death)
        } else {
            immediate_uninit_ids.push(id);
            ReceiverDeathRecipe::Cleanup
        };
        if !matches!(
            recipe,
            ReceiverDeathRecipe::Sequence | ReceiverDeathRecipe::CrashExplode
        ) {
            let entity = self.substrate.entities.get_mut(id).unwrap();
            entity.dying = true;
            entity.infantry_terminal = Some(InfantryTerminal::AwaitingConsequences);
        }
        InfantryDeathPostlude {
            id,
            position,
            world_z_leptons,
            recipe,
        }
    }

    /// VERA-internal compatibility, gamemd equivalent UNCHECKED: raw Genetic
    /// mutation keeps its whole-batch immediate replacement policy and bypasses
    /// ReceiveDamage effects. Retire on the victim's next Logic visit, as the
    /// former non-animated path did; a looping Stand must not retain a corpse.
    pub(crate) fn mark_raw_mutation_victim(&mut self, id: u64) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category != EntityCategory::Infantry || entity.dying || entity.health.current == 0
        {
            return false;
        }
        self.begin_raw_infantry_death(id, None)
    }

    /// Complete the represented raw-kill handoff. Bridge fallout supplies its
    /// C4 sequence selector; mutation and aircraft retirement supply no action.
    /// No ReceiveDamage effects are introduced on these compatibility paths.
    /// Returns false for other categories, whose existing lifetime stays local.
    pub(crate) fn begin_raw_infantry_death(&mut self, id: u64, inf_death: Option<u8>) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if entity.category != EntityCategory::Infantry {
            return false;
        }
        entity.health.current = 0;
        entity.dying = true;
        // Legacy raw coordinate scans can revisit an UnInit'd object before
        // pending deletion drains it. Preserve their HP write, but never give
        // a completed lifetime another scheduled terminal action.
        if !entity.lifecycle.object_alive {
            return true;
        }
        let sequence = entity
            .animation
            .as_ref()
            .and_then(|_| inf_death.and_then(InfantryDeathSequence::for_inf_death));
        if let Some(sequence) = sequence {
            self.begin_infantry_death_sequence(id, sequence);
        } else {
            entity.infantry_terminal = Some(InfantryTerminal::RetireNextVisit);
        }
        true
    }

    /// Native Do_Action51D6F0 retains progress for an unchanged action. A
    /// captured receiver can write a different action after UnInit, without
    /// restoring Logic membership or canceling its pending deletion.
    pub(crate) fn begin_infantry_death_sequence(
        &mut self,
        id: u64,
        sequence: InfantryDeathSequence,
    ) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        debug_assert_eq!(entity.category, EntityCategory::Infantry);
        entity.dying = true;
        if entity.lifecycle.object_alive {
            entity.infantry_terminal = Some(InfantryTerminal::Sequence(sequence));
        }
        // Do_Action's `+0x6C4` write (`0x0051D6F0`): the death Doing that
        // Infantry GetFireError answers CANT for (`0x0051C8B8`).
        if entity.mission_leaf.as_infantry().is_some() {
            entity
                .mission_leaf
                .set_infantry_doing_verified(i32::from(crate::rules::infantry_sequence::action_id(
                    sequence.animation(),
                )))
                .expect("a death Doing is in the verified table");
        }
        if entity
            .animation
            .as_ref()
            .is_none_or(|animation| animation.sequence != sequence.animation())
        {
            entity.animation = Some(Animation::new(sequence.animation()));
        }
    }

    /// Consume this object's terminal Logic visit, including eventual UnInit.
    /// Returns false only when the object is outside this lifetime mechanism.
    ///
    /// A Die1/Die2 corpse still runs its Techno AI subset each visit (see
    /// `techno_ai::dying_infantry_techno_ai`); when its sequence completes,
    /// `InfantryClass 0x00520BC6` leaves a corpse anim, then UnInit.
    pub(super) fn visit_infantry_terminal(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        ctx: super::techno_ai::ObjectAiCtx<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(terminal) = entity.infantry_terminal else {
            return false;
        };
        debug_assert!(entity.dying && entity.category == EntityCategory::Infantry);
        let finished = match terminal {
            InfantryTerminal::RetireNextVisit => true,
            InfantryTerminal::AwaitingConsequences => return true,
            InfantryTerminal::Sequence(sequence) => {
                let Some(rules) = rules else {
                    return true;
                };
                super::techno_ai::dying_infantry_techno_ai(self, id, rules, ctx);
                let Some(entity) = self.substrate.entities.get(id) else {
                    return true;
                };
                let def = rules
                    .animation_sequence(self.interner.resolve(entity.type_ref()))
                    .and_then(|set| set.get(&sequence.animation()));
                let animation = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .animation
                    .as_mut();
                match (animation, def) {
                    (Some(animation), Some(def))
                        if animation.sequence == sequence.animation()
                            && def.frame_count > 0
                            && def.frame_delay > 0
                            && (!def.normalized
                                || self
                                    .session
                                    .game_options
                                    .normalized_anim_delay(def.frame_delay)
                                    > 0)
                            && matches!(def.loop_mode, LoopMode::HoldLast) =>
                    {
                        advance_animation(animation, def, &self.session.game_options);
                        animation.finished
                    }
                    // VERA-internal malformed/missing art fallback: never wait
                    // indefinitely on a looping or non-progressing definition.
                    _ => true,
                }
            }
        };
        if finished {
            if let (InfantryTerminal::Sequence(_), Some(rules)) = (terminal, rules) {
                self.leave_dead_body(id, rules);
            }
            self.release_move_sound(id);
            if let Some(rules) = rules {
                self.uninit_with_rules(id, rules);
            } else {
                self.uninit(id);
            }
        }
        true
    }

    /// `InfantryClass 0x00520BC6..0x00520CA4`, a Die1..Die5 sequence's
    /// completion: the type's `DeadBodies=`, else `[General] DeadBodies=`
    /// unless the type is `NotHuman=`, one Scenario `Random() % count`
    /// (`0x00520C11`/`0x00520C6B`), and `AnimClass(body, GetCoords, 0, 1,
    /// 0x600, 0, 0)` (`0x00520CA4`), before the UnInit. Returns the corpse
    /// anim it built.
    ///
    /// Native execution: `tools/spatial_oracle/infantry_death_completion.py`
    /// ([`dead_body_tests`]).
    fn leave_dead_body(
        &mut self,
        id: u64,
        rules: &RuleSet,
    ) -> Option<crate::sim::intern::InternedId> {
        let entity = self.substrate.entities.get(id)?;
        let object = self.object_type(entity.type_ref(), rules)?;
        let bodies = if !object.dead_bodies.is_empty() {
            &object.dead_bodies
        } else if !object.not_human {
            &rules.general.dead_bodies
        } else {
            return None;
        };
        if bodies.is_empty() {
            return None;
        }
        let location = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let body = self.pick_death_anim(bodies).to_string();
        let body = self.interner.intern(&body);
        self.admit_death_anim(
            rules,
            body,
            crate::sim::combat::destruction_effects::DeathAnimSpawn {
                coord: crate::sim::anim_class::AnimWorldCoord {
                    x: location.x,
                    y: location.y,
                    z: location.z,
                },
                delay: 0,
                draws: None,
            },
        );
        Some(body)
    }
}

#[cfg(test)]
mod dead_body_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use serde_json::Value;

    /// Every `infantry_death_completion.json` Die1..Die5 row that completes
    /// (stage at or past a nonzero count) replays through
    /// `Simulation::leave_dead_body` from the same Scenario seed: the corpse it
    /// picks (the type's list, else Rules', none for NotHuman) and the RNG
    /// state after.
    #[test]
    fn leave_dead_body_matches_the_original() {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/infantry_death_completion.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 68);
        let mut compared = 0;
        for row in &rows {
            let input = &row["input"];
            let int = |key: &str| input[key].as_i64().unwrap();
            if !(0xB..=0xF).contains(&int("doing"))
                || int("count") <= 0
                || int("stage") < int("count")
                || int("alloc") == 0
            {
                continue;
            }
            let type_bodies: Vec<String> = (0..int("type_bodies"))
                .map(|n| format!("TBODY{n}"))
                .collect();
            let ini = format!(
                "[General]\nDeadBodies=RBODY0,RBODY1,RBODY2,RBODY3,RBODY4,RBODY5\n\
                 [InfantryTypes]\n0=DOOMED\n[DOOMED]\nStrength=100\nNotHuman={}\n{}",
                if int("not_human") != 0 { "yes" } else { "no" },
                if type_bodies.is_empty() {
                    String::new()
                } else {
                    format!("DeadBodies={}\n", type_bodies.join(","))
                },
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            let mut sim = Simulation::with_seed(int("seed") as u64);
            let mut entity =
                crate::sim::game_entity::GameEntity::test_default(1, "DOOMED", "Americans", 5, 5);
            entity.type_ref = sim.interner.intern("DOOMED");
            entity.category = EntityCategory::Infantry;
            sim.substrate.entities.insert(entity);
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_before"].as_str().unwrap()
            );

            let picked = sim
                .leave_dead_body(1, &rules)
                .map(|body| sim.interner.resolve(body).to_string());
            let native = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["call"] == "anim")
                .map(|event| {
                    let anim = event["anim"].as_i64().unwrap();
                    assert_eq!(event["rest"], serde_json::json!([0, 1, 0x600, 0, 0]));
                    if anim >= 0x2F00 {
                        format!("TBODY{}", anim - 0x2F00)
                    } else {
                        format!("RBODY{}", anim - 0x2E00)
                    }
                });
            assert_eq!(picked, native, "{input}");
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}"
            );
            compared += 1;
        }
        assert_eq!(compared, 61);
    }
}
