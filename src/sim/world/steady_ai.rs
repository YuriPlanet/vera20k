//! Steady objects: between its timers, a Unit's or Infantryman's AI visit
//! steps only its counter and stage clock, so its turn counts the visit
//! instead of running it (VERA-only, sim-scale; results unchanged).
//!
//! `TechnoClass::AI_Update @ 0x006F9E50`, which `UnitClass::AI` and
//! `InfantryClass::AI` reach every frame, and the leaf steps before it act on
//! what the object holds: a queued mission, a curtain, a drain, a manager, a
//! clock of its own, a target, damage. An object holding none of the first
//! five, moving or standing, finds every step off but
//! - the `+0xC4` counter (`0x006FA646`), which every visit steps;
//! - the common stage clock (Techno6FABC4), on the frames it advances;
//! - `MissionClass::AI @ 0x005B3060`'s handler, when its dispatch timer runs
//!   out;
//! - the passive block (`0x006FA65A`), when its targeting timer runs out and
//!   the block would act;
//! - the estimated health's recover (Techno6F9F6E), on the frames it moves;
//! - the voice probe (Techno6F9EBB), while the client waits on its voice;
//! - with a target, the illegal drop (`0x006FA472`) on every sixteenth
//!   frame, the reach block (`0x006FAB48`) outside a team, and the allied
//!   drop (`0x006FA30C`) when its house is allied with the target's;
//! - hurt, the house heal arms (`0x006FA79A`) on their frames.
//!
//! What reads the object alone stays off by its mark: a target's
//! off-mission clear (`0x006FA5E8`) and a healer's drops (`0x006FA9D8`), a
//! hurt self-healer's pulse (`0x006FA743`) and a hurt type's damage sparks.
//!
//! At the end of a turn the pass asks [`ai_rests`], and whether a trigger
//! has given the object a sound for `ObjectClass::AI`'s sound block
//! (`0x005F3E78`); when it rests without one, the store marks it steady.
//! Every mutable hand-out clears the mark, and so does a new sound
//! (`ObjectClass::Set_Custom_Sound` wakes the object), so while it is set
//! the object's state is what it was then and [`ai_rests`] still holds: it
//! reads the object's own state and type, the frame only where a later frame
//! cannot turn a step on again (a spent curtain stays spent), and the
//! hidden-object count of its Cell, which only a building's arrival or
//! departure changes, and those wake every object. The next turn's head
//! ([`Simulation::steady_ai_visit`]) asks the rest of the list, which read
//! timers, the frame, the client's voice set, its team, its house's heal
//! count and its target's owner; when none acts, it steps the counter and
//! the stage clock in place of the visit. The rest of the turn (the
//! locomotor, the fire visit) runs as before.
//!
//! Only Units and Infantry are marked, under idle sleeping's switch and
//! gates: no extension's unsorted per-visit steps, no campaign (its shroud
//! prelude), a rule policy that allows sleeping.
//! [`Simulation::check_idle_sleep`] runs every visit a steady object would
//! count and panics if it changed anything else: the object, the Scenario or
//! Main stream or the object lists, and once a frame the state hash.

use super::Simulation;
use super::techno_ai::{ObjectAiCtx, ObjectAiOutcome};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionId;

/// One object's published resting decisions: id, sleep, quiet, steady.
type RestMark = (u64, super::idle_sleep::SleepDecision, bool, bool);

impl Simulation {
    /// Whether `id` is steady now: its AI visit may be counted while nothing
    /// [`Self::steady_ai_visit`] asks acts.
    #[cfg(test)]
    pub(super) fn steady(&self, id: u64) -> bool {
        self.substrate.entities.is_steady(id)
    }

    /// `id`'s AI visit at the head of its turn: counted when it is steady and
    /// no timer or outside wait of the visit acts this frame, else run.
    pub(super) fn steady_ai_visit(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: &mut ObjectAiCtx<'_>,
    ) -> ObjectAiOutcome {
        if !self.substrate.entities.is_steady(id) || self.steady_visit_acts(id, rules) {
            return self.object_ai_visit_one_with_effects(id, rules, ctx);
        }
        self.idle_sleep.steady += 1;
        if self.idle_sleep.check {
            let check = self.steady_check(id);
            let outcome = self.object_ai_visit_one_with_effects(id, rules, ctx);
            self.finish_sleep_check(id, check);
            return outcome;
        }
        // The `+0xC4` step (`0x006FA646`) and the stage clock, the visit's
        // two writes (`world::techno_ai::common_body`).
        self.substrate.entities.count_mission_ai_visit(id);
        super::techno_ai::stage_step(self, id);
        ObjectAiOutcome::visited()
    }

    /// Whether steady `id`'s visit acts on more than its counter and stage
    /// clock this frame: its mission handler or passive block is due, its
    /// estimated health moves, the client waits on its voice, a target step
    /// that reads more than the object asks, or a house heal arm heals it.
    fn steady_visit_acts(&self, id: u64, rules: &RuleSet) -> bool {
        use super::techno_ai::{
            allied_target_drops, house_heal_frame, house_heals, illegal_target_drop_frame,
            target_reach_asks,
        };
        let frame = self.session.binary_frame();
        let Some(entity) = self.substrate.entities.get(id) else {
            return true;
        };
        entity.mission_handler_due(frame)
            || entity
                .estimated_health
                .recovers(entity.health.current(), frame)
            || (entity.passive_scan.timer().due(frame)
                && super::techno_ai::passive_block_acts(self, id, rules))
            || self.unit_voice_visit_interest.contains(&id)
            || (entity.attack_target.target().is_some()
                && (illegal_target_drop_frame(frame)
                    || target_reach_asks(self, id, entity)
                    || allied_target_drops(self, id, rules)))
            || (house_heal_frame(frame, rules)
                && self
                    .interner
                    .object_type(entity.type_ref(), rules)
                    .is_none_or(|object| house_heals(&self.houses, entity, object, frame, rules)))
    }

    /// Check mode: what steady `id`'s visit must leave. Once a frame, on a
    /// visit that leaves the stage clock as it is, the state hash too.
    fn steady_check(&mut self, id: u64) -> super::idle_sleep::SleepCheck {
        let frame = self.session.binary_frame();
        let quiet = self.quiet(id);
        let mut expected = self
            .substrate
            .entities
            .get(id)
            .expect("a steady object is stored")
            .clone();
        expected.settle_sleep(1);
        let now = frame as i32;
        let stage = !quiet && expected.native_stage().advances(now);
        if stage {
            expected.tick_native_stage(now);
        }
        let hash = !stage && self.idle_sleep.steady_hashed != frame.wrapping_add(1);
        if hash {
            self.idle_sleep.steady_hashed = frame.wrapping_add(1);
        }
        self.sleep_check("steady AI", id, expected, hash)
    }

    /// Whether `id`'s end-of-turn state satisfies the steady gates and
    /// [`ai_rests`]. Read-only, so the rest pass may ask every object side by
    /// side before it publishes the marks.
    pub(super) fn steady_candidate(&self, id: u64, rules: &RuleSet) -> bool {
        if !self.idle_sleep.is_on()
            || !matches!(
                self.substrate.entities.category_of(id),
                Some(EntityCategory::Unit | EntityCategory::Infantry)
            )
            || self.substrate.entities.is_steady(id)
            || self.object_custom_sounds.contains_key(&id)
            || self.extension_steps_unsorted(rules)
            || self.session.uses_local_player_shroud()
            || !self.session.rule_policy().allows_idle_object_sleep()
        {
            return false;
        }
        let frame = self.session.binary_frame();
        self.substrate.entities.get(id).is_some_and(|entity| {
            self.interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| {
                    ai_rests(entity, object, frame, rules)
                        // The healer drops (`0x006FA9D8`): only a healer's
                        // target goes.
                        && (entity.attack_target.target().is_none()
                            || !super::techno_ai::combat_damage_heals(self, entity, object, rules))
                        // The Behind indicator: no Cell of its that could
                        // hide it, whatever stands there.
                        && super::techno_ai::behind_indicator_coord(self, id, entity, object)
                            .and_then(super::techno_ai::behind_indicator_cell)
                            .is_none_or(|(rx, ry)| {
                                self.substrate.hidden_occupation.count(rx, ry) == 0
                            })
                })
        })
    }

    /// The resting decisions of the object at `index` in the live Logic
    /// order, or `None` when it has no mark to publish. Read-only, so a
    /// parallel pass may ask it beside its other work.
    fn resting_candidate_at(
        &self,
        index: usize,
        rules: &RuleSet,
        speed: super::super::movement::SpeedRules,
    ) -> Option<RestMark> {
        if !self.idle_sleep.is_on() {
            return None;
        }
        let id = *self.substrate.logic.as_slice().get(index)?;
        let sleep = self.sleep_candidate(id, rules);
        let quiet = self.quiet_candidate(id, rules, speed);
        let steady = self.steady_candidate(id, rules);
        (quiet || steady || !matches!(sleep, super::idle_sleep::SleepDecision::NotApplicable))
            .then_some((id, sleep, quiet, steady))
    }

    /// Publish one [`Self::resting_candidate_at`] answer, sleep first as a
    /// turn's own end did (`idle_sleep::sleep_after_turn`).
    fn apply_rest_mark(&mut self, mark: RestMark) {
        let (id, sleep, quiet, steady) = mark;
        self.apply_sleep_decision(id, sleep);
        if quiet {
            self.substrate.entities.set_quiet(id);
        }
        if steady {
            self.substrate.entities.set_steady(id);
        }
    }

    /// Publish every object's end-of-turn resting marks after the live pass
    /// has run. Candidates are read side by side against the pass's final
    /// world; the marks only let the object's own next visit skip steps whose
    /// state still satisfies them, and any later write clears the mark.
    ///
    /// Sim-scale: native asks at the end of each object's own turn, VERA
    /// asks once for all of them after the pass. A later object can write one
    /// this pass already visited, which clears its mark; the final world is
    /// what the next visit reads, so the set it sees is the same.
    pub(super) fn mark_resting_after_pass(&mut self, rules: &RuleSet) {
        if !self.idle_sleep.is_on() {
            return;
        }
        let candidates = {
            let ids = self.substrate.logic.as_slice();
            let speed =
                super::super::movement::SpeedRules::new(rules, &self.interner, &self.houses);
            crate::sim::parallel::filter_map_indices(ids.len(), |index| {
                self.resting_candidate_at(index, rules, speed)
            })
        };
        for mark in candidates {
            self.apply_rest_mark(mark);
        }
    }
}

/// Whether every step of the AI visit of `entity` (a Unit or Infantryman of
/// type `object`) but those [`Simulation::steady_ai_visit`] asks about finds
/// it off at `frame` and at every later frame while its state stays as it
/// is. The healer drops and the Behind indicator's Cell are asked apart
/// ([`Simulation::steady_after_turn`]).
///
/// What a busy object fails comes first, so most answer after a few reads
/// (every term only reads, so the order changes no answer).
fn ai_rests(entity: &GameEntity, object: &ObjectType, frame: u32, rules: &RuleSet) -> bool {
    let health = entity.health.current();
    // InfantryClass::AI's Health reset (`0x0051BC57`) at Health 0, and a
    // hurt self-healer's pulse (`0x006FA743`); the house heal arms
    // (`0x006FA79A`) the head asks, the damage sparks below.
    health > 0
        && health <= object.strength
        && (health == object.strength
            || !crate::sim::combat::veterancy::holds_self_heal(entity, object))
        // Ready/Commence (Unit736465, Infantry51BC51).
        && entity.mission.queued() == MissionId::NONE
        // A target's off-mission clear (`0x006FA5E8`); its drops and reach
        // block the head asks, the healer drops the mark.
        && !super::techno_ai::off_mission_clear_drops(entity)
        // The visit's own gates: alive, not dying, no tube.
        && entity.lifecycle.object_alive()
        && !entity.lifecycle.in_limbo()
        && !entity.death.is_dying()
        && entity.active_tube().is_none()
        // A Unit's offline sparks (`0x007360C9`) and both drain steps.
        && !entity.deactivated.is_offline()
        && entity.drain.is_idle()
        // The Temporal prologue: no chain warping it, neither warped out nor
        // warping in.
        && !entity.temporal.is_warped()
        && !entity.is_warped_out()
        && !entity.is_warping_in()
        // A Unit's death count (`0x0073635B`) and firing counter
        // (Unit7363C9); the Hunt queue (`0x007363DE`) asks only a type that
        // deploys into a building.
        && !entity.unit_death_counter.running()
        && !entity.unit_firing_counter.counting()
        && object.deploys_into.is_none()
        // The Gattling sound (Techno6F9E70) and the ambient sound.
        && !object.is_gattling
        && object.ambient_sound.is_none()
        // The tint step: no curtain, or a spent one at stage 0.
        && entity
            .invulnerability
            .as_ref()
            .is_none_or(|curtain| !curtain.updates(frame as i32))
        // Berserk, promotion, rocking, recoil, ChargeTurret, the Door clock
        // and a retained Behind indicator.
        && !entity.berserk.is_active()
        && !entity.veterancy.rank_changed()
        && !crate::sim::rocking::updates(entity)
        && !entity.voxel_recoil_moves()
        && !GameEntity::charges_turret(object)
        && entity.door_rests()
        && entity.behind_anim().is_none()
        // ObjectClass::AI's fall block.
        && !entity.is_falling_down()
        // The bomb, the managers and the cloak steps.
        && entity.bomb.is_none()
        && entity.slave_manager.is_none()
        && entity.capture_manager.is_none()
        && entity.spawn_manager.is_none()
        && entity.cloak.is_none()
        && !super::techno_ai_cloak::cloak_producer_ticks(entity, object, rules.ares())
        // The flash countdown (Techno6FAC31) and the damage sparks.
        && !entity.flashing.counting()
        && !crate::sim::particles::spawn::damage_sparks_admit(entity, object, rules)
}
