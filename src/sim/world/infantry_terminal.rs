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
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        effects: &mut crate::sim::combat::DeathEffects,
    ) {
        if let ReceiverDeathRecipe::ExternalAnim(inf_death) = self.recipe {
            let mut smudges = Vec::new();
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
                &mut smudges,
            );
            for request in smudges {
                #[cfg(test)]
                if world.receiver_fixture.is_some() {
                    effects.smudge_spawn_requests.push(request);
                    continue;
                }
                world.commit_smudge_request_inline(rules, overlay_registry, request);
            }
        }
        if matches!(self.recipe, ReceiverDeathRecipe::ExternalAnim(_)) {
            effects.immediate_uninit_ids.push(self.id);
        }
    }
}

impl Simulation {
    /// Select the represented concrete recipe after recursive DeathWeapon
    /// damage. Effects remain in the consuming postlude. Animation presence
    /// gates legacy effects, never an indefinite lifetime wait.
    pub(crate) fn begin_infantry_receiver_death(
        &mut self,
        id: u64,
        inf_death: u8,
        immediate_uninit_ids: &mut Vec<u64>,
    ) -> InfantryDeathPostlude {
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("fatal Infantry retained for postlude");
        debug_assert_eq!(entity.category, EntityCategory::Infantry);
        let position = entity.position.clone();
        let world_z_leptons =
            crate::sim::combat::object_world_z_leptons(entity, self.resolved_terrain.as_ref());
        let recipe = if entity.animation.is_none() {
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
        if !matches!(recipe, ReceiverDeathRecipe::Sequence) {
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
    pub(super) fn visit_infantry_terminal(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
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
            self.release_move_sound(id);
            if let Some(rules) = rules {
                self.uninit_with_rules(id, rules);
            } else {
                self.uninit(id);
            }
        }
        true
    }
}
