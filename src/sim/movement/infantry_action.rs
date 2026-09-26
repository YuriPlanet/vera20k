//! `InfantryClass` actions, the DoType at Infantry `+0x6C4` (VERA's Doing,
//! `MissionLeafState::as_infantry`): `Do_Action @ 0x0051D6F0`, the
//! locomotion action tail of `0x00520F40` and `DoType_Sequencer @ 0x00520AE0`
//! at a sequence's end.
//!
//! Every native Infantry draws the sequence of its Doing. VERA keeps the Doing
//! whole only for an infantryman flown by the Jumpjet locomotor (the Rocketeer
//! and the Cosmonaut, [`doing_owns_sequence`]): its Doing comes from these
//! bodies and the firing arm, and every accepted action restarts its displayed
//! sequence, as Do_Action restarts the stage (`+0xF8`) and its timer. Every
//! other infantryman keeps the animation cascade (`sim::animation`), with its
//! Doing written only by the receivers that request one (the failed path, the
//! slave's dig, the Cheer's end).
//!
//! Native execution: `tools/spatial_oracle/jumpjet_infantry_actions.py` runs
//! the four bodies on a Rocketeer flown by the real Jumpjet locomotor
//! ([`tests`]).

use crate::map::entities::EntityCategory;
use crate::rules::infantry_sequence::{action_kind, action_record};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::Animation;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

pub(crate) const DO_READY: i32 = 0;
pub(crate) const DO_PRONE: i32 = 2;
pub(crate) const DO_WALK: i32 = 3;
pub(crate) const DO_CRAWL: i32 = 6;
pub(crate) const DO_TREAD: i32 = 0x10;
pub(crate) const DO_SWIM: i32 = 0x11;
pub(crate) const DO_HOVER: i32 = 0x17;
pub(crate) const DO_FLY: i32 = 0x18;
pub(crate) const DO_FIRE_FLY: i32 = 0x1A;
pub(crate) const DO_DEPLOY: i32 = 0x1B;
pub(crate) const DO_DEPLOYED: i32 = 0x1C;
pub(crate) const DO_DEPLOYED_IDLE: i32 = 0x1E;
pub(crate) const DO_CHEER: i32 = 0x20;
pub(crate) const DO_PARADROP: i32 = 0x21;
pub(crate) const DO_PANIC: i32 = 0x25;

/// The fraction thresholds the actions read from Foot `+0x578`, truncated to
/// `SimFixed` as `FootSpeedState::set_speed_fraction_native_bits` truncates
/// the stored double: 0.1 (`0x007E3860`, the sequencer's default arm) and 0.8
/// (`0x007EB5C8`, the locomotion action tail).
fn speed_fraction_above_tenth(entity: &GameEntity) -> bool {
    entity.foot_speed.applied_fraction > SimFixed::ONE / SimFixed::from_num(10)
}

fn speed_fraction_above_eight_tenths(entity: &GameEntity) -> bool {
    // floor(0.8 * 2^16).
    entity.foot_speed.applied_fraction > SimFixed::from_bits(52_428)
}

/// Whether this infantryman's displayed sequence is its Doing's, which VERA
/// keeps for an Infantry flown by the Jumpjet locomotor: every accepted action
/// restarts it, and the animation cascade leaves it alone.
pub(crate) fn doing_owns_sequence(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Infantry
        && entity
            .locomotor
            .as_ref()
            .is_some_and(|locomotor| locomotor.jumpjet_runtime().is_some())
}

/// `Do_Action` 0x0051D6F0 admission after its request remaps: an unchanged
/// action refuses (`0x0051D90B`), and an established action that is not
/// interruptible refuses unless forced (`0x0051D919..0x0051D92E`, the record's
/// byte 0 at `0x007EAF7C`). `-1` always admits.
pub(crate) fn do_action_admits(current: i32, requested: i32, force: bool) -> bool {
    if requested == current {
        return false;
    }
    force || current == -1 || action_record(current).is_some_and(|record| record.interruptible)
}

/// Whether `DoType_Sequencer @ 0x00520AE0` ends `action` in its default arm
/// (`0x00520CE6`) rather than one of its own (the byte table at
/// `0x00520F1C`): Die1..5 leave a body, WetDie and AirDeathFinish UnInit,
/// Deploy, Undeploy, AirDeathStart and Shovel request their next action, and
/// Paradrop does nothing. No action (-1) takes the default arm every frame
/// (`0x00520AEF`).
pub(crate) fn takes_default_arm(action: i32) -> bool {
    !matches!(
        action,
        0x0B..=0x0F | 0x14 | 0x15 | 0x1B | 0x1F | 0x21 | 0x22 | 0x24 | 0x26
    )
}

impl Simulation {
    /// `InfantryClass::Do_Action` 0x0051D6F0 for `requested` with the
    /// caller's `force` and a random-frame argument 0, as the class's own
    /// receivers request it. Answers whether the action was accepted.
    pub(crate) fn infantry_do_action(
        &mut self,
        id: u64,
        requested: i32,
        force: bool,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Do_Action receiver")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Do_Action requires the Infantry type")?;
        let type_id = object.id.clone();
        let movement_zone = object.movement_zone;
        let doing = actor
            .mission_leaf
            .as_infantry()
            .ok_or("Do_Action requires Infantry Doing")?
            .doing();
        let on_bridge = actor.on_bridge;
        self.apply_infantry_do_action(
            id,
            doing,
            requested,
            force,
            &type_id,
            movement_zone,
            on_bridge,
            rules,
        )
    }

    /// `InfantryClass::Do_Action` 0x0051D6F0 with a random-frame argument 0.
    ///
    /// In order:
    /// - The requested action's record must have frames (`0x0051D70F`, Type
    ///   `+0xE3C`).
    /// - A falling paradropper keeps its Paradrop (`0x0051D722..0x0051D733`).
    /// - Water remap: an AmphibiousDestroyer type on a Water or Beach cell off
    ///   a bridge (`0x0051D793..0x0051D8B8`) remaps and plays a wet sound,
    ///   which Rust does not own. Its Doing is left untouched.
    /// - Airborne remap: Ready becomes Hover (`0x0051D8BF..0x0051D8EE`) while
    ///   high flying (vtable `+0x54`), off a bridge, when the type's Hover
    ///   record starts past frame 0.
    /// - Walk becomes Panic at fear 200 (`0x0051D8F5..0x0051D906`).
    /// - Then admission ([`do_action_admits`]) and the Doing write
    ///   (`0x0051D9D2`).
    ///
    /// Not represented:
    /// - the carried Walk remap (`+0x2DC`, `0x0051D739`);
    /// - Down without `Crawls=` (`0x0051D77A`);
    /// - the Deploy and Undeploy sounds;
    /// - a zero-Health infantryman's re-entry into Stop_Driver
    ///   (`0x0051DA96`), which only a crashing Jumpjet infantryman reaches.
    ///
    /// Stage and timer: for an infantryman whose Doing owns its sequence
    /// ([`doing_owns_sequence`]), the stage and timer the action arms are its
    /// animation's restart. Every other keeps its animation cascade.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_infantry_do_action(
        &mut self,
        id: u64,
        current: i32,
        requested: i32,
        force: bool,
        type_id: &str,
        movement_zone: MovementZone,
        on_bridge: bool,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let kind = action_kind(requested)
            .ok_or_else(|| format!("Do_Action request {requested} has no sequence"))?;
        let sequences = rules.animation_sequence(type_id);
        // The type's native records (Type `+0xE3C`), signed as the reader
        // stores them; a type without them answers from its draw layout.
        let record = |action: i32| sequences.and_then(|set| set.infantry_action(action));
        //0x51D70F: a zero requested-sequence count refuses before any state.
        let has_sequence = match record(requested) {
            Some(record) => record.frames_per_facing != 0,
            None => sequences
                .and_then(|set| set.get(&kind))
                .is_some_and(|sequence| sequence.frame_count != 0),
        };
        if !has_sequence {
            return Ok(false);
        }
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Do_Action receiver")?;
        if current == DO_PARADROP && actor.object_is_falling_down != 0 {
            return Ok(false);
        }
        if movement_zone == MovementZone::AmphibiousDestroyer && !on_bridge {
            let land = self
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(actor.position.rx, actor.position.ry))
                .map(|cell| cell.yr_cell_land_type);
            //0x51D7C6: LandType Water(2)/Beach(6) remaps 0/2 -> 16 and writes +6E8.
            if matches!(land, Some(2) | Some(6)) {
                return Ok(false);
            }
        }
        let mut requested = requested;
        //0x51D8E0: the Hover record's start frame, signed.
        let hover_frames = match record(DO_HOVER) {
            Some(record) => record.start_frame > 0,
            None => sequences
                .and_then(|set| set.get(&crate::sim::animation::SequenceKind::Hover))
                .is_some_and(|sequence| sequence.start_frame > 0),
        };
        if requested == DO_READY
            && crate::sim::combat::in_range::is_high_flying(actor)
            && !on_bridge
            && hover_frames
        {
            requested = DO_HOVER;
        }
        let fear = actor
            .infantry
            .as_ref()
            .map_or(0, |infantry| infantry.fear_level);
        if requested == DO_WALK && fear >= 200 {
            requested = DO_PANIC;
        }
        if !do_action_admits(current, requested, force) {
            return Ok(false);
        }
        let owns_sequence = doing_owns_sequence(actor);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Do_Action receiver")?;
        actor
            .mission_leaf
            .set_infantry_doing_verified(requested)
            .map_err(|error| format!("Do_Action wrote an invalid Doing: {error:?}"))?;
        if owns_sequence && let Some(kind) = action_kind(requested) {
            actor.animation = Some(Animation::new(kind));
        }
        Ok(true)
    }

    /// What `InfantryClass::AI` does with an infantryman's action after its
    /// `Process`, for one whose Doing owns its sequence: the sequencer, which
    /// with no action (-1) takes its default arm every frame (`0x00520AEF`;
    /// an action's end reaches it through the animation clock instead,
    /// [`Self::infantry_action_completed`]), then the locomotion actions.
    pub(crate) fn infantry_action_turn(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !doing_owns_sequence(actor) || actor.dying || !actor.is_ai_alive() {
            return;
        }
        if actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) == Some(-1) {
            self.infantry_default_action(id, -1, rules);
        }
        self.infantry_movement_actions(id, rules);
    }

    /// The locomotion action tail of `0x00520F40` (`0x00521144..0x0052130F`),
    /// which `InfantryClass::AI` runs after the sequencer (`0x0051BF7B`), for
    /// an infantryman whose Doing owns its sequence.
    ///
    /// While the locomotor `Is_Moving_Now` (vtable `+0xA8`; a Jumpjet outside
    /// States 0 and 2, `0x0054D0D0`):
    /// - A `JumpJet=` type flown by the Jumpjet locomotor (its class id against
    ///   `0x007E9AC0`) flies (Fly, 0x18) above a speed fraction of 0.8 and
    ///   hovers (Hover, 0x17) at or below it. The firing latch (`+0x68D`) holds
    ///   the fire action instead.
    /// - Otherwise it crawls when prone, else walks.
    ///
    /// Otherwise Walk, Fly and Hover return to Ready (Hover again while high
    /// flying), Crawl to Prone and Swim to Tread.
    ///
    /// The latch is VERA's pending shot: native raises `+0x68D` as the fire
    /// action starts (`0x00520912`) and drops it at the discharge
    /// (`0x0051DF70`), which is the span `AttackTarget::pending_infantry_fire`
    /// covers. VERA's leaf copy of the latch has no producer.
    ///
    /// Not run: the earlier movement recovery of `0x00520F40`
    /// (`0x00520F40..0x00521144`), whose work VERA's movement adapter does.
    pub(crate) fn infantry_movement_actions(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !doing_owns_sequence(actor) || actor.dying || !actor.is_ai_alive() {
            return;
        }
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return;
        };
        let Some(runtime) = actor
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.jumpjet_runtime())
        else {
            return;
        };
        let moving_now = !matches!(runtime.phase, 0 | 2);
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        let requested = if moving_now {
            let jumpjet_type = self
                .object_type(actor.type_ref(), rules)
                .is_some_and(|object| object.jumpjet);
            if jumpjet_type {
                let firing = actor
                    .attack_target
                    .as_ref()
                    .is_some_and(|attack| attack.pending_infantry_fire.is_some());
                if firing {
                    return;
                }
                if speed_fraction_above_eight_tenths(actor) {
                    DO_FLY
                } else {
                    DO_HOVER
                }
            } else if prone {
                DO_CRAWL
            } else {
                DO_WALK
            }
        } else {
            match doing {
                DO_WALK | DO_FLY | DO_HOVER => DO_READY,
                DO_CRAWL => DO_PRONE,
                DO_SWIM => DO_TREAD,
                _ => return,
            }
        };
        if let Err(cause) = self.infantry_do_action(id, requested, false, rules) {
            log::debug!("infantry {id} locomotion action: {cause}");
        }
    }

    /// `InfantryClass::DoType_Sequencer @ 0x00520AE0` for an Infantry whose
    /// Doing `action` has played its sequence to the end: VERA's animation
    /// clock (`sim::animation`) stands for the stage (`+0xF8`, `+0x100..`)
    /// that Do_Action arms, and reports the end.
    ///
    /// An infantryman whose Doing owns its sequence takes the native arm for
    /// its action; the default arm is followed by the locomotion action tail,
    /// as `InfantryClass::AI` runs them back to back (`0x0051BF6A`,
    /// `0x0051BF7B`). A looping sequence (Hover, Fly, Walk) wraps instead of
    /// ending: its default arm either repeats the action, which Do_Action
    /// refuses, or restarts it through Walk and the tail, which the wrap
    /// matches.
    ///
    /// Every other infantryman keeps the arm for the Cheer (32) alone. The
    /// other actions it installs either hold (Ready, Prone, Deployed), end in
    /// their own owners (the death sequences), or stay a residual (Shovel,
    /// `sim::slave_manager`). Its Cheer takes the default arm with the Walk
    /// and Crawl requests unrepresented: the walk's end clears both
    /// (`0x00521B20`), so a moving cheerer's Doing is cleared at once.
    pub(crate) fn infantry_action_completed(&mut self, id: u64, action: i32, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) != Some(action) {
            return;
        }
        if doing_owns_sequence(actor) {
            if takes_default_arm(action) {
                self.infantry_default_action(id, action, rules);
                self.infantry_movement_actions(id, rules);
            }
            return;
        }
        if action != DO_CHEER {
            return;
        }
        let frame = self.session.binary_frame;
        let moving = super::ready_producer::is_moving_now_for(actor, frame)
            && speed_fraction_above_tenth(actor);
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        let result = if moving {
            self.substrate
                .entities
                .get_mut(id)
                .ok_or_else(|| "retired Do_Action receiver".to_string())
                .and_then(|actor| {
                    actor
                        .mission_leaf
                        .set_infantry_doing_verified(-1)
                        .map(|()| true)
                        .map_err(|error| format!("{error:?}"))
                })
        } else {
            self.infantry_do_action(id, if prone { DO_PRONE } else { DO_READY }, true, rules)
        };
        if let Err(cause) = result {
            log::debug!("infantry {id} Cheer completion: {cause}");
        }
    }

    /// The sequencer's default arm (`0x00520D1B..0x00520E4A`), for an action
    /// with no arm of its own, as a forced Do_Action:
    /// - moving (locomotor `Is_Moving`, vtable `+0x10`: a Jumpjet's moving
    ///   byte) faster than a tenth: Crawl when prone, else Walk;
    /// - a deploy action: Deployed;
    /// - prone: Prone;
    /// - otherwise Ready.
    ///
    /// The completion facing (`0x00520CEB..0x00520D16`) is the animation
    /// clock's. The secondary-fire repeat (`0x00520D7E..0x00520E00`) is not
    /// reached: no Jumpjet infantryman has a secondary fire action.
    fn infantry_default_action(&mut self, id: u64, action: i32, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let moving = actor
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.jumpjet_runtime())
            .is_some_and(|runtime| runtime.moving)
            && speed_fraction_above_tenth(actor);
        let prone = actor
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone);
        let requested = if moving {
            if prone { DO_CRAWL } else { DO_WALK }
        } else if (DO_DEPLOY..=DO_DEPLOYED_IDLE).contains(&action) {
            DO_DEPLOYED
        } else if prone {
            DO_PRONE
        } else {
            DO_READY
        };
        if let Err(cause) = self.infantry_do_action(id, requested, true, rules) {
            log::debug!("infantry {id} action {action} completion: {cause}");
        }
    }
}

#[cfg(test)]
#[path = "infantry_action_tests.rs"]
mod tests;
