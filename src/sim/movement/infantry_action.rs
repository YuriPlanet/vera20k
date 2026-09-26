//! `InfantryClass` actions, the DoType at Infantry `+0x6C4` (VERA's Doing,
//! `MissionLeafState::as_infantry`): `Do_Action @ 0x0051D6F0`, the
//! locomotion action tail of `0x00520F40`, `DoType_Sequencer @ 0x00520AE0` at
//! a sequence's end, and the idle actions `Assign_Target @ 0x0051B1F0` and the
//! firing AI's fire-frame refusal request.
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
//! RESIDUAL (stage clock): VERA advances every living animation, and reports
//! the sequencer's ends, at frame end (`sim::animation`, `world::mod`), after
//! every object's turn; natively an infantryman's stage steps in its own
//! `TechnoClass::AI_Update` and its sequencer and locomotion actions follow its
//! `Process`. VERA's locomotion actions run in the object's turn, before the
//! frame's combat pass. Trigger: an action ending, or a shot discharged while
//! the locomotor `Is_Moving_Now`. Effect: a sequence ends at the frame end
//! before the frame native ends it in, and a FireFly discharged in flight
//! yields to Fly or Hover one frame later than native; an object acting
//! earlier in the next frame sees the other Doing. Frequency: every action
//! end. Risk: the Doing a kill in that frame reads (the crash's Stop_Moving
//! count).
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

pub(crate) const DO_READY: i32 = 0;
pub(crate) const DO_PRONE: i32 = 2;
pub(crate) const DO_WALK: i32 = 3;
pub(crate) const DO_DOWN: i32 = 5;
pub(crate) const DO_CRAWL: i32 = 6;
pub(crate) const DO_UP: i32 = 7;
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

fn is_prone(entity: &GameEntity) -> bool {
    entity
        .infantry
        .as_ref()
        .is_some_and(|infantry| infantry.is_prone)
}

fn deploy_action(doing: i32) -> bool {
    (DO_DEPLOY..=DO_DEPLOYED_IDLE).contains(&doing)
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
        let facts = DoActionType {
            type_id: object.id.clone(),
            movement_zone: object.movement_zone,
            crawls: object.crawls,
        };
        self.apply_infantry_do_action(id, requested, force, &facts, rules)
    }

    /// `InfantryClass::Do_Action` 0x0051D6F0 with a random-frame argument 0.
    ///
    /// In order:
    /// - The requested action's record must have frames (`0x0051D70F`, Type
    ///   `+0xE3C`).
    /// - A falling paradropper keeps its Paradrop (`0x0051D722..0x0051D733`).
    /// - Down needs `Crawls=` (`0x0051D77A..0x0051D78D`).
    /// - Water remap: an AmphibiousDestroyer type on a Water or Beach cell off
    ///   a bridge (`0x0051D793..0x0051D8B8`) remaps and plays a wet sound,
    ///   which Rust does not own. Its Doing is left untouched.
    /// - Airborne remap: Ready becomes Hover (`0x0051D8BF..0x0051D8EE`) while
    ///   high flying (vtable `+0x54`), off a bridge, when the type's Hover
    ///   record starts past frame 0.
    /// - Walk becomes Panic at fear 200 (`0x0051D8F5..0x0051D906`).
    /// - Then admission ([`do_action_admits`]), the Doing write
    ///   (`0x0051D9D2`) and the prone byte: Down lies down, Up and Deploy stand
    ///   up (`0x0051DAA7..0x0051DAC8`).
    ///
    /// Not represented:
    /// - the carried Walk remap (`+0x2DC`, `0x0051D739`): only a Jumpjet-flown
    ///   infantryman requests Walk, and none is ever carried;
    /// - the Deploy and Undeploy sounds;
    /// - a zero-Health infantryman's re-entry into Stop_Driver
    ///   (`0x0051DA96`), which only a crashing Jumpjet infantryman reaches.
    ///
    /// Stage and timer: for an infantryman whose Doing owns its sequence
    /// ([`doing_owns_sequence`]), the stage and timer the action arms are its
    /// animation's restart. Every other keeps its animation cascade.
    pub(super) fn apply_infantry_do_action(
        &mut self,
        id: u64,
        requested: i32,
        force: bool,
        facts: &DoActionType,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        let kind = action_kind(requested)
            .ok_or_else(|| format!("Do_Action request {requested} has no sequence"))?;
        let sequences = rules.animation_sequence(&facts.type_id);
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
        let current = actor
            .mission_leaf
            .as_infantry()
            .ok_or("Do_Action requires Infantry Doing")?
            .doing();
        let on_bridge = actor.on_bridge;
        if current == DO_PARADROP && actor.object_is_falling_down != 0 {
            return Ok(false);
        }
        if requested == DO_DOWN && !facts.crawls {
            return Ok(false);
        }
        if facts.movement_zone == MovementZone::AmphibiousDestroyer && !on_bridge {
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
        if let Some(infantry) = actor.infantry.as_mut() {
            match requested {
                DO_DOWN => infantry.is_prone = true,
                DO_UP | DO_DEPLOY => infantry.is_prone = false,
                _ => {}
            }
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
    /// RESIDUAL (firing latch): native raises `+0x68D` as the fire action
    /// starts (`0x00520912`) and drops it at the discharge (`0x0051DF70`), a
    /// fire-frame refusal (`0x00520A03`), a target change (`0x0051B20E`) or
    /// a rate-0 stage (`0x0051BE01`). VERA's leaf copy of the latch has no
    /// producer, so the tail reads the pending shot
    /// (`AttackTarget::pending_infantry_fire`), which spans the same frames
    /// but also ends when the shooter gains a movement goal or its sequence
    /// changes (`world_receiver`). Trigger: a Jumpjet infantryman firing with
    /// a goal still set. Effect: its FireFly can yield to Fly or Hover before
    /// the fire frame, where native holds it; the leaf latch's other readers
    /// (Ready, `MissionClass`; Area Guard `0x004D6E97`; the Foot checksum
    /// `0x004DBD28`) never see it raised. Frequency: rare for the first,
    /// every shot for the second. Risk: readiness while firing.
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
                if actor.foot_speed.above_eight_tenths() {
                    DO_FLY
                } else {
                    DO_HOVER
                }
            } else if is_prone(actor) {
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

    /// `InfantryClass::Assign_Target @ 0x0051B1F0` handed a target other than
    /// the one it holds (`0x0051B1FB`), on a live receiver (`0x0051B203`):
    /// the firing latch drops (`0x0051B20E`, VERA's leaf copy) and the
    /// infantryman returns to an idle action, unforced: Deployed after a
    /// deploy action, else Prone when prone, else Ready (`0x0051B214..
    /// 0x0051B24F`). Run for an infantryman whose Doing owns its sequence;
    /// every other keeps the entity-local clear of
    /// `concrete_effects::represented_assign_target_admitted`.
    pub(crate) fn infantry_target_change_action(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !doing_owns_sequence(actor) || actor.health.current <= 0 {
            return;
        }
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return;
        };
        let requested = if deploy_action(doing) {
            DO_DEPLOYED
        } else if is_prone(actor) {
            DO_PRONE
        } else {
            DO_READY
        };
        if let Err(cause) = self.infantry_do_action(id, requested, false, rules) {
            log::debug!("infantry {id} target change action: {cause}");
        }
    }

    /// The firing AI's refusal at the fire frame (`0x005209FD..0x00520A51`):
    /// GetFireError is not OK, so the latch drops and the infantryman returns
    /// to an idle action, unforced: Prone when prone, else Deployed after a
    /// deploy action, else Ready. Run for an infantryman whose Doing owns its
    /// sequence; every other takes the animation cascade's idle sequence.
    pub(crate) fn infantry_fire_refused_action(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !doing_owns_sequence(actor) {
            return;
        }
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return;
        };
        let requested = if is_prone(actor) {
            DO_PRONE
        } else if deploy_action(doing) {
            DO_DEPLOYED
        } else {
            DO_READY
        };
        if let Err(cause) = self.infantry_do_action(id, requested, false, rules) {
            log::debug!("infantry {id} refused fire action: {cause}");
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
    /// Every other infantryman keeps the default arm for the Cheer (32) alone.
    /// The other actions it installs either hold (Ready, Prone, Deployed), end
    /// in their own owners (the death sequences), or stay a residual (Shovel,
    /// `sim::slave_manager`).
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
        if action == DO_CHEER {
            self.infantry_default_action(id, action, rules);
        }
    }

    /// The sequencer's default arm (`0x00520D1B..0x00520E4A`), for an action
    /// with no arm of its own, as a forced Do_Action:
    /// - moving (locomotor `Is_Moving`, vtable `+0x10` at `0x00520D38`: the
    ///   Walk or Jumpjet moving byte) faster than a tenth: Crawl when prone,
    ///   else Walk;
    /// - a deploy action: Deployed;
    /// - prone: Prone;
    /// - otherwise Ready.
    ///
    /// A walker's Doing has no locomotion actions in VERA to take it off Walk
    /// or Crawl again, and the walk's end clears both (`0x00521B20`), so its
    /// moving request clears the Doing instead.
    ///
    /// The completion facing (`0x00520CEB..0x00520D16`) is the animation
    /// clock's. The secondary-fire repeat (`0x00520D7E..0x00520E00`) is not
    /// reached: no Jumpjet infantryman has a secondary fire action, and the
    /// walker arm runs for the Cheer alone.
    fn infantry_default_action(&mut self, id: u64, action: i32, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let is_moving = actor.locomotor.as_ref().is_some_and(|locomotor| {
            locomotor
                .jumpjet_runtime()
                .map(|runtime| runtime.moving)
                .or_else(|| locomotor.walk_is_moving())
                .unwrap_or(false)
        });
        let moving = is_moving && actor.foot_speed.above_tenth();
        let requested = if moving {
            if is_prone(actor) { DO_CRAWL } else { DO_WALK }
        } else if deploy_action(action) {
            DO_DEPLOYED
        } else if is_prone(actor) {
            DO_PRONE
        } else {
            DO_READY
        };
        let result = if moving && !doing_owns_sequence(actor) {
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
            self.infantry_do_action(id, requested, true, rules)
        };
        if let Err(cause) = result {
            log::debug!("infantry {id} action {action} completion: {cause}");
        }
    }
}

/// The type facts `Do_Action` reads besides its sequence records.
pub(super) struct DoActionType {
    pub(super) type_id: String,
    /// Type `+0x5B4`.
    pub(super) movement_zone: MovementZone,
    /// `Crawls=`, Type `+0xEBD`.
    pub(super) crawls: bool,
}

#[cfg(test)]
#[path = "infantry_action_tests.rs"]
mod tests;
