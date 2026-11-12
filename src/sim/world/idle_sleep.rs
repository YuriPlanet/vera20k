//! Idle objects sleep: the live object pass counts an idle object's visit as
//! owed instead of running its turn (VERA-only, sim-scale; results unchanged).
//!
//! gamemd visits every Logic member every frame (`LogicClass::AI @
//! 0x0055AFB0`). Between its timers, an idle object's visit changes only its
//! per-visit counters: `TechnoClass::AI_Update`'s `+0xC4` increment
//! (`0x006FA646`) and, for a building, UpdateAnimation's relight sample
//! (`0x00450A47`). It draws nothing and nothing else reads those counters
//! until its own next turn. So the pass may skip such a visit when
//! - the object was marked asleep at the end of a full turn
//!   ([`Simulation::sleep_after_turn`]), which names the first frame a timer
//!   of its own comes due;
//! - nothing has been handed it for writing since (`EntityStore` wakes it on
//!   every mutable hand-out, settling the owed counters first);
//! - nothing it reads outside itself has changed: its owner's power (the
//!   note compared at each visit), its cell's hidden state (the Mark owners
//!   wake every object) and the client's pending voice set.
//!
//! The sleeper keeps its Logic slot, so the pass's cursor and its
//! removal-closes-the-gap rule are unchanged, and the world work each turn
//! ends with (the sound prefix, pending overlay Recalcs) still runs at its
//! slot. The state hash and the save form read a sleeper as settled.
//!
//! This slice marks only plain buildings: no weapon, factory, super weapon,
//! occupants or sound of their own, at full health, on Guard. Everything else
//! stays awake. Builds with the Ares or Phobos capability mark nothing: their
//! per-visit steps are not sorted yet. A campaign marks nothing either
//! (`TechnoClass` vt+0x4A0's shroud prelude runs every visit there).
//!
//! [`Simulation::check_idle_sleep`] runs every turn the pass would skip and
//! panics if it changed anything but the owed counters.

use super::{FrameEffects, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};

/// The longest a sleeper goes without a full turn, whatever its timers say.
const SLEEP_CAP_FRAMES: u32 = 64;

/// The setting and its counters. Never saved or hashed.
#[derive(Debug, Default, Clone)]
pub(crate) struct IdleSleep {
    on: bool,
    check: bool,
    /// Visits counted instead of run (or, when checking, run and compared).
    counted: u64,
}

/// What the pass does with a Logic slot before its turn.
pub(super) enum SleepVisit {
    /// Run the turn.
    Awake,
    /// The visit was counted; no turn.
    Counted,
    /// Run the turn and compare ([`Simulation::finish_sleep_check`]).
    Check(Box<SleepCheck>),
}

/// A sleeper's state before a checked turn.
pub(super) struct SleepCheck {
    expected: serde_json::Value,
    scenario_rng: u64,
    logic_len: usize,
    entities: usize,
}

impl IdleSleep {
    /// Whether idle objects sleep, and units go quiet (`world::quiet_units`).
    pub(super) fn is_on(&self) -> bool {
        self.on
    }
}

impl Simulation {
    /// Let idle objects sleep from now on, or wake them all and stop: units
    /// go quiet (`world::quiet_units`) under the same switch. Real
    /// matches, replays and sim-bench turn it on; no result depends on it.
    pub fn sleep_idle_objects(&mut self, on: bool) {
        self.idle_sleep.on = on;
        if !on {
            self.substrate.entities.wake_all();
        }
    }

    /// [`Self::sleep_idle_objects`] as a match starts, unless the
    /// environment variable `VERA20K_IDLE_SLEEP` is `0`: the switch for
    /// ruling sleeping out of a fault report.
    pub fn sleep_idle_objects_in_match(&mut self) {
        let off = std::env::var_os("VERA20K_IDLE_SLEEP").is_some_and(|value| value == "0");
        self.sleep_idle_objects(!off);
    }

    /// Run every turn sleeping would skip and panic if it changed anything
    /// but the owed counters, drew from the Scenario stream or changed the
    /// Logic list.
    pub fn check_idle_sleep(&mut self, on: bool) {
        self.idle_sleep.check = on;
    }

    /// How many objects are asleep now, and how many visits were counted
    /// instead of run so far.
    pub fn idle_sleep_counts(&self) -> (usize, u64) {
        (
            self.substrate.entities.sleeping_count(),
            self.idle_sleep.counted,
        )
    }

    /// The pass reached `id`'s slot: count its visit if it sleeps on.
    pub(super) fn visit_sleeper(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> SleepVisit {
        if !self.idle_sleep.on {
            return SleepVisit::Awake;
        }
        self.substrate.entities.focus(id);
        let Some((wake_at, note)) = self.substrate.entities.sleeps_until(id) else {
            return SleepVisit::Awake;
        };
        let frame = self.session.binary_frame();
        let sleeps_on = frame < wake_at
            && !self.unit_voice_visit_interest.contains(&id)
            && self
                .substrate
                .entities
                .owner_of(id)
                .is_some_and(|owner| self.power_note(owner) == note);
        if !sleeps_on {
            self.substrate.entities.wake(id);
            return SleepVisit::Awake;
        }
        self.idle_sleep.counted += 1;
        if self.idle_sleep.check {
            self.substrate.entities.wake(id);
            // The checked turn stands for a counted one: still resting.
            self.substrate
                .entities
                .fit_to_sleep(id, frame.wrapping_sub(1));
            let expected = self
                .substrate
                .entities
                .get(id)
                .expect("a sleeper is stored")
                .clone();
            expected.settle_sleep(1);
            return SleepVisit::Check(Box::new(SleepCheck {
                expected: serde_json::to_value(&expected).expect("an entity serializes"),
                scenario_rng: self.scenario_rng.state(),
                logic_len: self.substrate.logic.len(),
                entities: self.substrate.entities.len(),
            }));
        }
        self.substrate.entities.owe_visit(id);
        // What every turn ends with for the world, at this slot as before:
        // the sound prefix (`FootClass::AI`'s edge sounds) and the commit
        // that publishes pending overlay Recalcs and smudge dirt.
        effects.flush_sound(self, rules);
        if self
            .overlay_grid
            .as_ref()
            .is_some_and(|grid| grid.has_synchronous_navigation_cells())
            || self
                .smudge_grid
                .as_ref()
                .is_some_and(|grid| grid.has_dirty())
        {
            let _ = self.commit_no_fire(rules, registry, effects);
        }
        SleepVisit::Counted
    }

    /// `id`'s checked turn is over: it must have been the visit sleeping
    /// would have counted.
    pub(super) fn finish_sleep_check(&self, id: u64, check: SleepCheck) {
        let after = self.substrate.entities.get(id).map(|entity| {
            let entity = entity.clone();
            entity.settle_sleep(self.substrate.entities.owed_visits(id));
            serde_json::to_value(&entity).expect("an entity serializes")
        });
        let name = self
            .substrate
            .entities
            .get(id)
            .map(|entity| self.interner.resolve(entity.type_ref()).to_string());
        let frame = self.session.binary_frame();
        assert_eq!(
            self.scenario_rng.state(),
            check.scenario_rng,
            "idle sleep: {name:?} {id} drew from the Scenario stream on frame {frame}"
        );
        assert_eq!(
            (self.substrate.logic.len(), self.substrate.entities.len()),
            (check.logic_len, check.entities),
            "idle sleep: {name:?} {id} changed the object lists on frame {frame}"
        );
        if after.as_ref() != Some(&check.expected) {
            let differing: Vec<String> = match (&after, &check.expected) {
                (Some(serde_json::Value::Object(after)), serde_json::Value::Object(before)) => {
                    before
                        .iter()
                        .filter(|(key, value)| after.get(*key) != Some(value))
                        .map(|(key, value)| format!("{key}: {value} -> {:?}", after.get(key)))
                        .collect()
                }
                _ => vec!["the object is gone".to_string()],
            };
            panic!("idle sleep: {name:?} {id} changed on frame {frame}: {differing:#?}");
        }
    }

    /// `id`'s full turn is over: mark it asleep if its next turns can change
    /// nothing until a timer of its own.
    pub(super) fn sleep_after_turn(&mut self, id: u64, rules: &RuleSet) {
        if !self.idle_sleep.on
            || self.substrate.entities.category_of(id) != Some(EntityCategory::Structure)
        {
            return;
        }
        let Some((wake_at, note)) = self.building_sleep(id, rules) else {
            self.substrate.entities.unfit_to_sleep(id);
            return;
        };
        // The rule reads the state this turn ended in, but the turn's
        // samplers ran before its later steps (a repair's last step follows
        // the power health sample). A second resting turn in a row has
        // sampled the resting state.
        let frame = self.session.binary_frame();
        if self.substrate.entities.fit_to_sleep(id, frame) {
            self.substrate.entities.sleep_until(id, wake_at, note);
        }
    }

    /// What a sleeping building reads of its owner every visit
    /// (`BuildingClass::IsPowerOnline @ 0x004555D0`, through
    /// [`Simulation::building_operational_state`]): full power, a blackout
    /// still running at this frame, a drained power source.
    fn power_note(&self, owner: InternedId) -> u8 {
        self.power_states.get(&owner).map_or(0x80, |state| {
            u8::from(state.has_full_power())
                | u8::from(state.blackout_remaining(self.session.binary_frame()) > 0) << 1
                | u8::from(state.has_drained_power_source()) << 2
        })
    }

    /// The frame plain idle building `id` may sleep until and its owner
    /// note, or `None` to stay awake.
    fn building_sleep(&self, id: u64, rules: &RuleSet) -> Option<(u32, u8)> {
        if cfg!(any(feature = "ares", feature = "phobos")) || !self.session.game_mode_nonzero() {
            return None;
        }
        let entity = self.substrate.entities.get(id)?;
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        // The type: every step its visit runs returns on the building's own
        // state or on the owner's power.
        let plain = object.has_stupid_guard_mode
            && !object.has_factory()
            && object.super_weapon.is_none()
            && object.super_weapon2.is_none()
            && !object.is_gattling
            && !object.has_turret
            && !object.can_be_occupied
            && object.ambient_sound.is_none()
            && object.working_sound.is_none()
            && object.not_working_sound.is_none()
            && !object.unit_repair
            && !object.unit_reload
            && !object.refinery
            && !object.infantry_absorb
            && !object.overpowerable
            && !object.gap_generator
            && !object.gate
            && !object.cloakable
            && object.powers_unit.is_none()
            && object.produce_cash_amount == 0;
        if !plain {
            return None;
        }
        // The state: nothing in progress.
        let health = entity.health.current();
        let idle = entity.lifecycle.object_alive()
            && !entity.lifecycle.in_limbo()
            && !entity.death.is_dying()
            && health == object.strength
            && entity.estimated_health
                == crate::sim::estimated_health::EstimatedHealth::from_raw(health)
            && entity.mission.current().known() == Some(MissionType::Guard)
            && entity.mission.queued() == MissionId::NONE
            && entity.attack_target.target().is_none()
            // UpdateAnimation sets `+0x6DD` every visit until a Commence
            // clears it (`0x0043FE4D`): set is its resting value.
            && entity.building_ready_latch() == 1
            && entity.flashing.frames() == 0
            // Each per-visit sampler holds what a fresh sample would give:
            // the power health sample (`0x00440042`), the damage fire
            // cache (`0x0043FC92`) and the relight sample (`0x00450A47`).
            && entity.building_power_health_sample() == Some(health)
            && !entity.damage_fires.is_active()
            && entity.damage_fires.anim_ids().iter().all(Option::is_none)
            && entity.airstrike_tint == Default::default()
            // Its building's airstrike flag reads `airstrike_link`, empty
            // below.
            && entity.building_art.tint_sample_is_current(
                entity.flashing.flashed(),
                entity.invulnerability.as_ref(),
                &entity.airstrike_tint,
                false,
            )
            && entity.native_stage().rate() == 0
            && !entity.native_stage().changed()
            && entity.invulnerability.is_none()
            && entity.airstrike_link().is_none()
            && entity.emp_remaining() == 0
            && !entity.ai_frozen()
            && !entity.temporal.is_warping_someone()
            && entity.bomb.is_none()
            && !entity.constructing_or_selling()
            && entity.pending_c4_detonation.pending().is_none()
            && entity.pending_building_fire.pending().is_none()
            && entity.capture_manager.is_none()
            && entity.spawn_manager.is_none()
            && entity.slave_manager.is_none()
            && entity.cloak.is_none()
            && entity.rocking.is_none()
            && entity.passenger_role.occupants().is_empty()
            && !entity.berserk.is_active()
            && !entity.deactivated.is_offline()
            && entity.tesla_charge.chargers().is_empty()
            && entity.drain.draining_me().is_none()
            && entity.drain.target().is_none()
            && !entity.repairing.is_on()
            && entity.parasite.eater().is_none()
            && entity.pending_entry().is_none()
            && !self.object_custom_sounds.contains_key(&id)
            && !crate::sim::combat::combat_weapon::is_armed(
                entity,
                object,
                &self.substrate.entities,
                rules,
                &self.interner,
            )
            && self.building_operational_state(id, rules)
                == Some(entity.building_last_operational.is_operational());
        if !idle {
            return None;
        }
        // `MissionClass::AI @ 0x005B3060`'s dispatch timer: the Guard handler
        // (`BuildingClass::Mission_Guard @ 0x004496B0`) runs when it is out.
        let now = self.session.binary_frame();
        let remaining = entity.mission.dispatch_timer().remaining_if_pending(now)?;
        let frames = u32::try_from(remaining).ok()?.min(SLEEP_CAP_FRAMES);
        let wake_at = now.checked_add(frames)?;
        // The turn at `now` has run; sleeping starts with the next frame.
        (wake_at > now.saturating_add(1)).then_some((wake_at, self.power_note(entity.owner())))
    }
}
