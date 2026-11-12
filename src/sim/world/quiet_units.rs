//! Quiet units: a resting Unit's turn skips the steps that would only find
//! their state off (VERA-only, sim-scale; results unchanged).
//!
//! Most of an idle tank's turn reads its record only to learn that something
//! is off: no curtain tint, no drain, no rocking, no bomb or manager, no door
//! moving. Those fields sit all over the record, so each read loads another
//! cache line. At the end of a turn the pass asks every one of those
//! questions at once ([`rests`]); when all answer off, the store marks the
//! unit quiet. Every mutable hand-out clears the mark
//! (`EntityStore::wake_slot`), so while it is set the unit's state is what it
//! was when it was set, and only the frame moves on. Each answer in [`rests`]
//! therefore still holds: it reads the unit's own state and type, and the
//! frame only where a later frame cannot turn it on again (a spent curtain
//! stays spent). A turn that finds the mark still set at its end leaves it
//! without asking again.
//!
//! The skipping steps ask the live mark, so a step that writes the unit
//! earlier in the same turn makes the later ones run as before. Each names
//! this module where it asks.
//!
//! Only Units are marked, and only in builds without the Ares or Phobos
//! capability, whose per-visit steps are not sorted. Idle sleeping's switch
//! turns it on and off ([`Simulation::sleep_idle_objects`]). A debug build
//! checks [`rests`] at the start of every quiet turn.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::SpeedRules;

impl Simulation {
    /// Whether `id` is quiet now: its steps listed in [`rests`] may be
    /// skipped.
    #[inline]
    pub(crate) fn quiet(&self, id: u64) -> bool {
        self.substrate.entities.is_quiet(id)
    }

    /// `id`'s turn is over: mark it quiet if [`rests`] holds.
    pub(super) fn quiet_after_turn(&mut self, id: u64, rules: &RuleSet) {
        if cfg!(any(feature = "ares", feature = "phobos"))
            || !self.idle_sleep.is_on()
            || self.substrate.entities.category_of(id) != Some(EntityCategory::Unit)
            || self.quiet(id)
        {
            return;
        }
        let frame = self.session.binary_frame();
        let speed = SpeedRules::new(rules, &self.interner, &self.houses);
        let quiet = self.substrate.entities.get(id).is_some_and(|entity| {
            self.interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| rests(entity, object, speed, frame, rules.ares()))
        });
        if quiet {
            self.substrate.entities.set_quiet(id);
        }
    }

    /// Debug builds: a quiet unit still rests as [`rests`] asks.
    #[cfg(debug_assertions)]
    pub(super) fn check_quiet(&self, id: u64, rules: &RuleSet) {
        if !self.quiet(id) {
            return;
        }
        let frame = self.session.binary_frame();
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("a quiet unit is stored");
        let object = self
            .interner
            .object_type(entity.type_ref(), rules)
            .expect("a quiet unit has its type");
        let speed = SpeedRules::new(rules, &self.interner, &self.houses);
        assert!(
            rests(entity, object, speed, frame, rules.ares()),
            "quiet unit {id} ({}) no longer rests on frame {frame}",
            self.interner.resolve(entity.type_ref())
        );
    }
}

/// Whether every step a quiet turn skips finds `entity` (a Unit of type
/// `object`) off at `frame` and at every later frame while its state stays
/// as it is.
///
/// The type's facts come first, then what a fighting, moving or hurt unit
/// fails, so a unit that cannot go quiet answers after a few reads (every
/// term only reads, so the order changes no answer).
pub(crate) fn rests(
    entity: &GameEntity,
    object: &ObjectType,
    speed: SpeedRules<'_>,
    frame: u32,
    ares: bool,
) -> bool {
    entity.category() == EntityCategory::Unit
        // The fire visit (`UnitClass::AI @ 0x007365E1`, `0x007365E8`): no
        // Gattling stage to wind down and no spinning turret.
        && !object.is_gattling
        && !object.turret_spins
        // FootClass::AI's body counter (`0x004DA886..0x004DAA01`): no idle
        // frames to count.
        && object.idle_rate == 0
        && !object.deploy_to_land
        // The fire visit again: no target and no NavCom.
        && entity.attack_target.target().is_none()
        && entity.navigation.nav_com().is_none()
        // Both self-heal steps leave an object at its type's Strength.
        && entity.health.current() == object.strength
        // The body counter again: the unit does not walk, so the move sound
        // sees no change, and it holds no move sound to count down.
        && !crate::sim::movement::motion_query::is_moving_now(entity, speed, frame)
        && !entity.move_sound.is_active()
        && entity.lifecycle.object_alive()
        && !entity.lifecycle.in_limbo()
        && !entity.death.is_dying()
        // The warp prologue (`Simulation::temporal_ai_prologue`) and the
        // frozen tests after it: neither warped nor warping in.
        && !entity.is_warped_out()
        && !entity.is_warping_in()
        // The tube tests (`UnitClass::AI @ 0x007360C0`).
        && entity.low_bridge_tube_state.is_none()
        // Both drain steps.
        && entity.drain.is_idle()
        // The tint step: no curtain, or a spent one at stage 0.
        && entity
            .invulnerability
            .as_ref()
            .is_none_or(|curtain| !curtain.updates(frame as i32))
        // The promotion sample, rocking, recoil and the door clock.
        && !entity.veterancy.rank_changed()
        && !crate::sim::rocking::updates(entity)
        && !entity.voxel_recoil_moves()
        && entity.door_rests()
        // The Behind indicator off a hiding cell.
        && entity.behind_anim().is_none()
        // ObjectClass::AI's fall block, the locomotor's admission and
        // FootClass::AI's fall edge.
        && !entity.is_falling_down()
        && !entity.falling_edge_pending()
        // The locomotor's owed destination setter and the NavCom stop.
        && !entity.navigation.destination_setter_owed()
        // The bomb, the managers and the cloak steps.
        && entity.bomb.is_none()
        && entity.slave_manager.is_none()
        && entity.capture_manager.is_none()
        && entity.spawn_manager.is_none()
        && entity.cloak.is_none()
        && !super::techno_ai_cloak::cloak_producer_ticks(entity, object, ares)
        // The common stage clock.
        && entity.native_stage().rests()
        // FootClass::AI's idle-entry latch clear.
        && entity.mission_leaf.foot_idle_entry_latch() == 0
        // FootClass::AI's pending entry and the eater's ParasiteClass AI.
        && entity.pending_entry().is_none()
        && entity.parasite.eater().is_none()
        // UnitClass::AI's harvesting clear.
        && entity.miner.as_ref().is_none_or(|miner| !miner.harvesting())
        // The fire visit's Facing slot has nothing to do.
        && super::unit_post::facing_rests(entity, object.turret_rot, frame)
}
