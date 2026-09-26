//! `InfantryClass` actions, the DoType at Infantry `+0x6C4` (VERA's Doing,
//! `MissionLeafState::as_infantry`): `Do_Action @ 0x0051D6F0`, the stage tick
//! of `TechnoClass::AI_Update` (`0x006FABC4`), `DoType_Sequencer @
//! 0x00520AE0`, the locomotion action tail of `0x00520F40`, the idle actions of
//! `Assign_Target @ 0x0051B1F0` and of the firing AI's fire-frame refusal, and
//! a shot-down Jumpjet infantryman's fall: `InfantryClass::AI`'s Health reset
//! (`0x0051BC57`), the crash latch's AirDeathStart (`0x0054B02C`) and the
//! impact notice's AirDeathFinish (`0x00522AF7`).
//!
//! Every native Infantry draws the sequence of its Doing. VERA keeps the Doing
//! whole only for an infantryman flown by the Jumpjet locomotor (the Rocketeer
//! and the Cosmonaut, [`doing_owns_sequence`]). Its Doing comes from these
//! bodies and the firing arm, and every accepted action restarts its displayed
//! sequence, as Do_Action restarts the stage (`+0xF8`) and its timer. As
//! natively, the stage steps in its own Techno AI (`world::techno_ai`), and the
//! sequencer and locomotion actions follow its `Process` (`world::object_turn`).
//! Every other infantryman keeps the animation cascade (`sim::animation`),
//! clocked at frame end, with its Doing written only by the receivers that
//! request one (the failed path, the slave's dig, the Cheer's end).
//!
//! RESIDUAL (in-flight FireFly): VERA fires in the combat pass that follows
//! every object's turn, while native fires in the object's own AI before its
//! sequencer and locomotion actions (`0x0051BF59`). Trigger: a shot discharged
//! while the locomotor `Is_Moving_Now`. Effect: the FireFly yields to Fly or
//! Hover one frame later than native. Frequency: a Rocketeer shooting on the
//! move. Risk: a kill in that frame reads FireFly, whose kill stops the
//! locomotor 11 times where Hover's stops it 10 (one more Scenario draw).
//!
//! Native execution: `tools/spatial_oracle/jumpjet_infantry_actions.py` runs
//! the four action bodies on a Rocketeer flown by the real Jumpjet locomotor
//! ([`tests`]); `tools/spatial_oracle/jumpjet_infantry_crash.py` runs its kill
//! and fall (`world::jumpjet_infantry_tests`).

use crate::map::entities::EntityCategory;
use crate::rules::infantry_sequence::{action_kind, action_record};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::Animation;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

pub(crate) const DO_READY: i32 = 0;
pub(crate) const DO_GUARD: i32 = 1;
pub(crate) const DO_PRONE: i32 = 2;
pub(crate) const DO_WALK: i32 = 3;
pub(crate) const DO_DOWN: i32 = 5;
pub(crate) const DO_CRAWL: i32 = 6;
pub(crate) const DO_UP: i32 = 7;
pub(crate) const DO_IDLE1: i32 = 9;
pub(crate) const DO_IDLE2: i32 = 0x0A;
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
pub(crate) const DO_AIR_DEATH_START: i32 = 0x22;
pub(crate) const DO_AIR_DEATH_FALLING: i32 = 0x23;
pub(crate) const DO_AIR_DEATH_FINISH: i32 = 0x24;
pub(crate) const DO_PANIC: i32 = 0x25;

/// `InfantryClass::IsInDeathSequence @ 0x00522CB0`: Die1..5, WetDie1/2 and the
/// three AirDeath actions. Assign_Target refuses such an infantryman as a
/// target (`0x006FCF2D`), and its AI keeps Health 0 in them (`0x0051BC5E`).
pub(crate) fn in_death_sequence(doing: i32) -> bool {
    matches!(doing, 0x0B..=0x0F | 0x14 | 0x15 | 0x22..=0x24)
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
    ///   (`0x0051D9D2`), at Health exactly 0 the re-entry into Stop_Driver
    ///   (`0x0051DA96`, which only a crashing Jumpjet infantryman reaches), and
    ///   the prone byte: Down lies down, Up and Deploy stand up
    ///   (`0x0051DAA7..0x0051DAC8`).
    ///
    /// Not represented:
    /// - the carried Walk remap (`+0x2DC`, `0x0051D739`): only a Jumpjet-flown
    ///   infantryman requests Walk, and none is ever carried;
    /// - the Deploy and Undeploy sounds;
    /// - the random first stage a caller's third argument asks for
    ///   (`0x0051DA4A..0x0051DA84`, a Scenario draw): the crash latch, the
    ///   impact notice, the AirDeath arm and the idle fidgets pass 0, and the
    ///   native corpora's restarted stages show it for the other callers VERA
    ///   ports.
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
        // `0x0051DA96..0x0051DAA1`: an accepted action at Health exactly 0
        // re-enters Stop_Driver, whose own Do_Action finds the action it
        // just wrote. The Can_Enter_Cell it runs reads no overlay here.
        if actor.health.current == 0
            && let Err(cause) = self.infantry_stop_driver(id, rules, None)
        {
            log::debug!("infantry {id} Do_Action Stop_Driver: {cause}");
        }
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return Ok(true);
        };
        if let Some(infantry) = actor.infantry.as_mut() {
            match requested {
                DO_DOWN => infantry.is_prone = true,
                DO_UP | DO_DEPLOY => infantry.is_prone = false,
                _ => {}
            }
        }
        Ok(true)
    }

    /// `InfantryClass::AI`'s Health reset (`0x0051BC57..0x0051BC96`), before
    /// `FootClass::AI`: an infantryman at Health 0 or below whose action is
    /// not a death sequence ([`in_death_sequence`]) is set to Health 1. Only
    /// a crashing Jumpjet infantryman reaches its own AI at Health 0: every
    /// other death removes it or turns it to a dying sequence first.
    pub(crate) fn infantry_health_reset(&mut self, id: u64) {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return;
        };
        if actor.health.current <= 0 && !in_death_sequence(doing) {
            actor.health.current = 1;
        }
    }

    /// The stage tick of `TechnoClass::AI_Update` (`0x006FABC4..0x006FAC2A`),
    /// before `Process`, for an infantryman whose Doing owns its sequence: the
    /// stage (`+0xF8`, the animation's frame) steps by one each time its timer
    /// has run the action's rate since the last step or the action's start,
    /// and never wraps; the draw shows it modulo the frame count
    /// (`0x00518E18`, `resolve_shp_frame`) and the sequencer reads its end.
    /// A rate of 0 never steps.
    ///
    /// RESIDUAL (timer): native's stage timer is a frame timer that Do_Action
    /// starts at the current frame with the action's rate
    /// (`0x0051DA13..0x0051DA44`); VERA counts the object's own ticks since
    /// the action started, at the rate of the current game speed. Trigger: an
    /// action started earlier in the frame than the object's own AI (a
    /// player's order reaching a Rocketeer mid-shot, whose target-change idle
    /// action runs before its turn), or a game-speed change mid-action.
    /// Effect: that action's stage steps one frame early. Frequency: orders to
    /// a Rocketeer that is firing. Risk: the actions those reach (Hover,
    /// Ready) loop under a refused default arm, so only the pose moves.
    ///
    /// RESIDUAL: the stage is 16 bits where native's is 32, so a Hover held
    /// for 65,536 steps (about 2.4 hours at rate 2) wraps to 0: one skipped
    /// pose in its loop, nothing else (its default arm is refused anyway).
    pub(crate) fn infantry_stage_tick(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !doing_owns_sequence(actor) {
            return;
        }
        let Some(animation) = actor.animation.as_ref() else {
            return;
        };
        let Some(def) = rules
            .animation_sequence(self.interner.resolve(actor.type_ref()))
            .and_then(|set| set.get(&animation.sequence))
        else {
            return;
        };
        let rate = if def.normalized {
            self.session
                .game_options
                .normalized_anim_delay(def.frame_delay)
        } else {
            def.frame_delay
        };
        if rate == 0 {
            return;
        }
        let Some(animation) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.animation.as_mut())
        else {
            return;
        };
        animation.elapsed_frames = animation.elapsed_frames.saturating_add(1);
        if animation.elapsed_frames >= rate {
            animation.elapsed_frames = 0;
            animation.frame_index = animation.frame_index.wrapping_add(1);
        }
    }

    /// What `InfantryClass::AI` does with an infantryman's action after its
    /// `Process`, for one whose Doing owns its sequence: the sequencer
    /// (`0x0051BF6A`), then the locomotion actions (`0x0051BF7B`). Answers
    /// true when the sequencer UnInit the infantryman, whose turn then ends.
    pub(crate) fn infantry_action_turn(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        if !doing_owns_sequence(actor) || actor.dying || !actor.is_ai_alive() {
            return false;
        }
        if self.infantry_sequencer(id, rules) {
            return true;
        }
        self.infantry_movement_actions(id, rules);
        false
    }

    /// `InfantryClass::DoType_Sequencer @ 0x00520AE0` for an infantryman
    /// whose Doing owns its sequence. With no action (-1) it takes the
    /// default arm every frame (`0x00520AEF`); otherwise once the stage
    /// reaches the action's frame count (`0x00520B09`), it dispatches through
    /// the byte table at `0x00520F1C`:
    /// - the default arm ([`Self::infantry_default_action`]), after the
    ///   completed action's facing hint (`0x00520CEB..0x00520D16`), refused
    ///   while the action it asks for is the one playing, so a held Hover's
    ///   stage keeps growing and its draw wraps;
    /// - AirDeathStart forces AirDeathFalling (`0x00520BB9`);
    /// - WetDie and AirDeathFinish UnInit the infantryman (`0x00520CB8`);
    ///   AirDeathFinish leaves no body (`DeadBodies=` is Die1..5's, `0x00520BC6`).
    ///
    /// Die1..5, Deploy, Undeploy, Paradrop and Shovel have arms of their own
    /// that no Jumpjet-flown infantryman reaches: the dying ones leave through
    /// `world::infantry_terminal`. Answers true when it UnInit the
    /// infantryman.
    fn infantry_sequencer(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return false;
        };
        let actor_type = actor.type_ref();
        if doing != -1 {
            let stage = actor.animation.as_ref().map_or(0, |a| a.frame_index);
            let sequences = rules.animation_sequence(self.interner.resolve(actor.type_ref()));
            let count = match sequences.and_then(|set| set.infantry_action(doing)) {
                Some(record) => record.frames_per_facing,
                None => action_kind(doing)
                    .and_then(|kind| sequences.and_then(|set| set.get(&kind)))
                    .map_or(0, |def| i32::from(def.frame_count)),
            };
            if i32::from(stage) < count {
                return false;
            }
        }
        match doing {
            DO_AIR_DEATH_START => {
                if let Err(cause) = self.infantry_do_action(id, DO_AIR_DEATH_FALLING, true, rules) {
                    log::debug!("infantry {id} AirDeathFalling: {cause}");
                }
                false
            }
            0x14 | 0x15 | DO_AIR_DEATH_FINISH => {
                // UnInit (vtable `+0xF8`); the Foot destructor lets its sounds
                // play out.
                self.sound_events
                    .push(crate::sim::world::SimSoundEvent::ObjectSoundReleased { owner: id });
                self.release_move_sound(id);
                self.uninit_with_rules(id, rules);
                true
            }
            action if takes_default_arm(action) => {
                // `0x00520CE6..0x00520D16`: a completed action (not -1) turns
                // the body to its record's facing hint first.
                if let Some(facing) = action_kind(action).and_then(|kind| {
                    rules
                        .animation_sequence(self.interner.resolve(actor_type))
                        .and_then(|set| set.get(&kind))
                        .and_then(|def| def.completion_facing)
                }) && let Some(actor) = self.substrate.entities.get_mut(id)
                {
                    crate::sim::animation::snap_completion_facing(
                        &mut actor.facing,
                        &mut actor.body_facing,
                        facing,
                        self.session.binary_frame,
                    );
                }
                self.infantry_default_action(id, action, rules);
                false
            }
            _ => false,
        }
    }

    /// The Infantry `INoticeSink` answer to the Jumpjet crash impact
    /// (`0x00522A60` -> `0x00522AF7..0x00522BA1`, notice `0x117C`), for a
    /// `Crashable=` type: SetHeight (vtable `+0x1CC`) to the bridge deck on a
    /// high-bridge cell (`[0x00A8F234]`, 416) and to 0 elsewhere, then a
    /// forced AirDeathFinish (`0x00522B9B`). The infantryman stays: its
    /// sequencer UnInits it when AirDeathFinish has played.
    pub(crate) fn infantry_crash_impact(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let crashable = self
            .object_type(actor.type_ref(), rules)
            .is_some_and(|object| object.crashable);
        if !crashable {
            return;
        }
        let coord = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
        let high_bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cell = terrain.native_cell_identity((
                (coord.x.div_euclid(256)) as i16,
                (coord.y.div_euclid(256)) as i16,
            ));
            terrain.native_cell_flags(cell) & 0x100 != 0
        });
        self.set_object_height(
            id,
            if high_bridge {
                crate::sim::movement::jumpjet_flight::BRIDGE_DECK_LEPTONS
            } else {
                0
            },
        );
        if let Err(cause) = self.infantry_do_action(id, DO_AIR_DEATH_FINISH, true, rules) {
            log::debug!("infantry {id} AirDeathFinish: {cause}");
        }
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

    /// `InfantryClass::DoType_Sequencer @ 0x00520AE0` for a walker whose
    /// Doing `action` has played its sequence to the end: VERA's frame-end
    /// animation clock (`sim::animation`) stands for the stage (`+0xF8`,
    /// `+0x100..`) that Do_Action arms, and reports the end. Only the Cheer
    /// (32) takes its default arm here. The other actions a walker installs
    /// either hold (Ready, Prone, Deployed), end in their own owners (the death
    /// sequences), or stay a residual (Shovel, `sim::slave_manager`).
    ///
    /// An infantryman whose Doing owns its sequence reaches the sequencer in
    /// its own turn instead ([`Self::infantry_action_turn`]).
    pub(crate) fn infantry_action_completed(&mut self, id: u64, action: i32, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) != Some(action) {
            return;
        }
        if !doing_owns_sequence(actor) && action == DO_CHEER {
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
    /// Its callers apply the completed action's facing hint
    /// (`0x00520CEB..0x00520D16`) first: the sequencer for an infantryman
    /// whose Doing owns its sequence, the animation cascade for a walker. The
    /// secondary-fire repeat (`0x00520D7E..0x00520E00`) is not reached: no
    /// Jumpjet infantryman has a secondary fire action, and the walker arm runs
    /// for the Cheer alone.
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
