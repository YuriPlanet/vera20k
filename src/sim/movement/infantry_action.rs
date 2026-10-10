//! `InfantryClass` actions, the DoType at Infantry `+0x6C4` (VERA's Doing,
//! `MissionLeafState::as_infantry`): `Do_Action @ 0x0051D6F0`, the stage tick
//! of `TechnoClass::AI_Update` (`0x006FABC4`), `DoType_Sequencer @
//! 0x00520AE0`, the locomotion action tail of `0x00520F40`, the idle actions of
//! `Assign_Target @ 0x0051B1F0` and of the firing AI's fire-frame refusal, and
//! a shot-down Jumpjet infantryman's fall: `InfantryClass::AI`'s Health reset
//! (`0x0051BC57`), the crash latch's AirDeathStart (`0x0054B02C`) and the
//! impact notice's AirDeathFinish (`0x00522AF7`).
//!
//! Every Infantry draws from its Doing and the single private Techno Stage.
//! Accepted Do_Action restarts that clock; refused requests retain it. Stage
//! advances in the object's Techno visit, followed by Process, readiness,
//! Fear, firing, sequencing and movement actions in its own Logic turn.
//! Presentation never advances this state or decides class completion.
//!
//! Ground-clock and firing evidence: anytown_damage/foot_missions.json,
//! stage_clock_receipt, ground_firing_receipt and ground_emission_receipt.
//! The latter's supplied deck pose retains empty upper occupancy; it proves
//! that controlled miss, not proper bridge-deck Unlimbo or damage admission.
//! Water remap/ordering and native constructor/readers are executed by
//! `tools/spatial_oracle/infantry_water_action.{py,json,meta.json}`: the original
//! 151 controls plus fire/idle/death/raw-action controls and physical GHOST/TANY
//! records. Audio requests are observed at the original disabled gate; wet
//! placement and audible playback remain outside those receiver comparisons.
//!
//! Native execution: `tools/spatial_oracle/jumpjet_infantry_actions.py` runs
//! the four action bodies on a Rocketeer flown by the real Jumpjet locomotor
//! ([`tests`]); `tools/spatial_oracle/jumpjet_infantry_crash.py` runs its kill
//! and fall (`world::jumpjet_infantry_tests`).

use crate::map::entities::EntityCategory;
use crate::rules::infantry_sequence::{action_kind, action_record};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
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

/// Infantry's Doing/Stage owns its pose and completion for every locomotor.
pub(crate) fn doing_owns_sequence(entity: &GameEntity) -> bool {
    entity.category == EntityCategory::Infantry
}

/// The native Infantry locomotor-class test, independent of which families
/// have migrated their presentation to Doing/Stage.
pub(crate) fn uses_jumpjet_locomotor(entity: &GameEntity) -> bool {
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
            type_id: &object.id,
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
    ///   a bridge (`0x0051D793..0x0051D8B8`) remaps the raw action. It reads
    ///   the physical cell even on a bridge, requests a transition sound
    ///   against the old `+0x6E8`, then stores the new water state before
    ///   unchanged/noninterruptible admission. The constructor's sentinel2
    ///   requests no sound on the first transition.
    /// - Airborne remap: Ready becomes Hover (`0x0051D8BF..0x0051D8EE`) while
    ///   high flying (vtable `+0x54`), off a bridge, when the type's Hover
    ///   record starts past frame 0.
    /// - Walk becomes Panic at fear 200 (`0x0051D8F5..0x0051D906`).
    /// - Then admission ([`do_action_admits`]), the Doing write
    ///   (`0x0051D9D2`), at Health exactly 0 the re-entry into Stop_Driver
    ///   (`0x0051DA96`), and
    ///   the prone byte: Down lies down, Up and Deploy stand up
    ///   (`0x0051DAA7..0x0051DAC8`).
    ///
    /// Not represented:
    /// - the SlaveOwner/storage/full-load Walk-to-Carry remap
    ///   (`0x0051D739..0x0051D773`), affecting loaded SLAV trips;
    /// - the random first stage a caller's third argument asks for
    ///   (`0x0051DA4A..0x0051DA84`, a Scenario draw): the crash latch, the
    ///   impact notice, the AirDeath arm and the idle fidgets pass 0, and the
    ///   native corpora's restarted stages show it for the other callers VERA
    ///   ports.
    ///
    /// Every accepted action restarts the shared native stage and timer.
    pub(super) fn apply_infantry_do_action(
        &mut self,
        id: u64,
        requested: i32,
        force: bool,
        facts: &DoActionType<'_>,
        rules: &RuleSet,
    ) -> Result<bool, String> {
        //51D701: -1 returns before the type record or water-state reads.
        if requested == -1 {
            return Ok(false);
        }
        if action_record(requested).is_none() {
            return Err(format!(
                "Do_Action request {requested} has no native action record"
            ));
        }
        // Raw WetDie20/21 and Guard1 have native records even though the
        // presentation vocabulary has no corresponding SequenceKind.
        let kind = action_kind(requested);
        let sequences = rules.animation_sequence(facts.type_id);
        // The type's native records (Type `+0xE3C`), signed as the reader
        // stores them; a type without them answers from its draw layout.
        let record = |action: i32| sequences.and_then(|set| set.infantry_action(action));
        //0x51D70F: a zero requested-sequence count refuses before any state.
        let has_sequence = match record(requested) {
            Some(record) => record.frames_per_facing != 0,
            None => kind
                .and_then(|kind| sequences.and_then(|set| set.get(&kind)))
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
        if current == DO_PARADROP && actor.is_falling_down() {
            return Ok(false);
        }
        if requested == DO_DOWN && !facts.crawls {
            return Ok(false);
        }
        let mut requested = requested;
        if facts.movement_zone == MovementZone::AmphibiousDestroyer {
            //51D7B0: virtual+1B8 is Abstract41BEA0, signed physical XYZ/256
            // packed to two WORDs. Map5657A0 retains one canonical Cell,
            // including Dummy land/type and stamping on an off-map lookup.
            let physical = super::ground_pose::position_world_coord(&actor.position);
            let packed = ((physical.x / 256) as i16, (physical.y / 256) as i16);
            let land = if let Some(terrain) = self.resolved_terrain.as_ref() {
                let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                cells.land_type(cells.lookup(packed))
            } else {
                let dummy = self.effective_shared_cell_dummy();
                dummy.stamp_coord(i32::from(packed.0), i32::from(packed.1));
                dummy.land_type()
            };
            let on_land = !matches!(land, 2 | 6) || on_bridge;
            if !on_land {
                //51D7DE..51D83D: only these requested records remap. Native
                // does not recheck the mapped record's count before admission.
                requested = match requested {
                    0 | 2 => DO_TREAD,
                    3 | 6 => DO_SWIM,
                    9 => 18,
                    10 => 19,
                    11 => 20,
                    12 => 21,
                    4 | 8 => 22,
                    other => other,
                };
            }
            let old_water_state = actor.mission_leaf.as_infantry().unwrap().water_state();
            let sound_id =
                rules
                    .object(facts.type_id)
                    .and_then(|object| match (old_water_state, on_land) {
                        (0, true) => object.leave_water_sound.as_ref(),
                        (1, false) => object.enter_water_sound.as_ref(),
                        _ => None,
                    });
            if let Some(sound_id) = sound_id {
                //51D842..51D8B3: PlayAt7509E0 uses Location and context0.
                // Queue the original request before the6E8 store/admission;
                // the app's audio gate and playback own audible output.
                let location =
                    super::ground_pose::object_location(actor, self.resolved_terrain.as_ref());
                self.sound_events
                    .push(crate::sim::world::SimSoundEvent::VocAt {
                        sound_id: sound_id.clone(),
                        audible_to: None,
                        rx: actor.position.rx,
                        ry: actor.position.ry,
                        sub_x: actor.position.sub_x,
                        sub_y: actor.position.sub_y,
                        world_z_leptons: location.z,
                    });
            }
            //51D8B8 precedes both unchanged and interruptibility refusals.
            // Every other MovementZone retains its constructor/load state.
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .mission_leaf
                .set_infantry_water_state(on_land);
        }
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Do_Action receiver")?;
        //0x51D8E0: the Hover record's start frame, signed.
        let hover_frames = match record(DO_HOVER) {
            Some(record) => record.start_frame > 0,
            None => sequences
                .and_then(|set| set.get(&crate::sim::animation::SequenceKind::Hover))
                .is_some_and(|sequence| sequence.start_frame > 0),
        };
        if requested == DO_READY
            && crate::sim::movement::air_movement::is_high_flying(
                actor,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            )
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
        //51D9CF..51DA44: rate is the original action-table byte, with
        // SpeedNormalize only for these six actions. Capture it at restart;
        // a later speed change does not retime the retained countdown.
        let mut rate = i32::from(
            action_record(requested)
                .ok_or("Do_Action has no native action record")?
                .frame_delay,
        );
        if matches!(requested, 9 | 10 | 18 | 19 | 23 | 32) {
            rate = self.session.game_options.speed_normalize(rate);
        }
        //51D939/51D981: the admitted Deploy/Undeploy sound observes the
        // previous Doing and Stage. Refused actions do not emit this cue.
        self.emit_deploy_action_sound(id, requested, rules)?;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Do_Action receiver")?;
        actor
            .mission_leaf
            .set_infantry_doing_verified(requested)
            .map_err(|error| format!("Do_Action wrote an invalid Doing: {error:?}"))?;
        //51D9D2..51DA44: every accepted class action writes Doing and
        // restarts its stage. The sequencer and draw share this exact state.
        actor.restart_native_stage(0, self.session.binary_frame as i32, rate);
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

    /// What `InfantryClass::AI` does with an infantryman's action after its
    /// `Process`: the first Doing=-1 dispatch also reaches this owner for an
    /// ordinary walker, so construction/Unlimbo keeps the native -1 and the
    /// first stationary AI selects Ready through the actual default arm.
    /// Firing (`0x0051BF59`) precedes the sequencer (`0x0051BF6A`) and
    /// locomotion actions (`0x0051BF7B`). Returns bridge-state consequences.
    pub(crate) fn infantry_action_turn(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        // Original51BCA4/51BCAC and51BF5E/51BF66 test Object+90, not
        // Health or the presentation death flag. Retained death sequences
        // still reach Fear -> Fire -> Sequencer until actual UnInit.
        if actor.category != EntityCategory::Infantry || !actor.is_object_alive() {
            return Ok(false);
        }
        //51BDE7..51BE3E precedes the second Ready/Commence and Fear. A
        // rate-zero clock cancels+68D, then unforced Deployed/Ready. Prone
        // is deliberately absent from this caller's action ladder.
        if actor.mission_leaf.foot_firing_sequence_latch() != 0 && actor.native_stage().rate() == 0
        {
            let requested = if actor.infantry_deploy_doing() {
                DO_DEPLOYED
            } else {
                DO_READY
            };
            self.substrate
                .entities
                .get_mut(id)
                .expect("retained action receiver")
                .mission_leaf
                .set_foot_firing_sequence(0);
            self.infantry_do_action(id, requested, false, rules)?;
        }
        self.object_ai_post_movement_promote_one(id, Some(rules));
        //51BF0B is after Process and the second Ready/Commence, including a
        // retained death sequence. Shared DoAction decides stance refusal.
        let mut bridge_changed = self.infantry_fear_turn(id, rules, registry)?;
        //51BF59 owns all Infantry firing; its new Bullet/Anim objects join
        // the current dynamic Logic suffix before the next actor visit.
        bridge_changed |= self
            .commit_fire_visit(
                crate::sim::combat::world_receiver::FireVisit::InfantryTarget(id),
                rules,
                registry,
            )
            .bridge_state_changed;
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|actor| !actor.is_object_alive())
        {
            return Ok(bridge_changed);
        }
        if self.infantry_sequencer(id, rules) {
            return Ok(bridge_changed);
        }
        self.infantry_movement_actions(id, rules, registry);
        Ok(bridge_changed)
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
    /// Die1..5, Deploy, Undeploy, Paradrop and Shovel use their own class
    /// completion receivers. Answers true when it UnInit the infantryman.
    pub(crate) fn infantry_sequencer(&mut self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return false;
        };
        let actor_type = actor.type_ref();
        let complete = if doing != -1 {
            let stage = actor.native_stage().value();
            let sequences = rules.animation_sequence(self.interner.resolve(actor.type_ref()));
            let count = match sequences.and_then(|set| set.infantry_action(doing)) {
                Some(record) => record.frames_per_facing,
                None => action_kind(doing)
                    .and_then(|kind| sequences.and_then(|set| set.get(&kind)))
                    .map_or(0, |def| i32::from(def.frame_count)),
            };
            stage >= count
        } else {
            true
        };
        let uninitialized = if complete {
            match doing {
                DO_AIR_DEATH_START => {
                    if let Err(cause) =
                        self.infantry_do_action(id, DO_AIR_DEATH_FALLING, true, rules)
                    {
                        log::debug!("infantry {id} AirDeathFalling: {cause}");
                    }
                    false
                }
                0x14 | 0x15 | DO_AIR_DEATH_FINISH => {
                    // UnInit owns Limbo and eventual Foot sound release.
                    self.uninit_with_rules(id, rules);
                    true
                }
                action if takes_default_arm(action) => {
                    // `0x00520CE6..0x00520D16`: a completed action (not -1) turns
                    // the body to its record's facing hint first.
                    if let Some(facing) = rules
                        .animation_sequence(self.interner.resolve(actor_type))
                        .and_then(|set| set.infantry_action(action))
                        .and_then(|record| {
                            crate::rules::infantry_sequence::completion_facing(record.facing_hint)
                        })
                        && let Some(actor) = self.substrate.entities.get_mut(id)
                    {
                        crate::sim::animation::snap_completion_facing(
                            &mut actor.body_facing,
                            facing,
                            self.session.binary_frame,
                        );
                    }
                    self.infantry_default_action(id, action, rules);
                    false
                }
                DO_DEPLOY | 31 => {
                    if let Err(cause) = self.infantry_deploy_completion(id, doing, rules) {
                        log::debug!("infantry {id} deployment completion: {cause}");
                    }
                    false
                }
                11..=15 => {
                    self.leave_dead_body(id, rules);
                    self.uninit_with_rules(id, rules);
                    true
                }
                38 => {
                    let requested = if self
                        .substrate
                        .entities
                        .get(id)
                        .is_some_and(|actor| actor.mission.current().raw() == 10)
                    {
                        38
                    } else {
                        DO_READY
                    };
                    if let Err(cause) = self.infantry_do_action(id, requested, true, rules) {
                        log::debug!("infantry {id} Shovel completion: {cause}");
                    }
                    false
                }
                _ => false,
            }
        } else {
            false
        };
        //520E52: the landing tail runs even below the sequence's count.
        if self.substrate.entities.get(id).is_some_and(|actor| {
            actor
                .mission_leaf
                .as_infantry()
                .is_some_and(|leaf| leaf.doing() == DO_PARADROP)
                && !actor.is_falling_down()
        }) {
            if let Err(cause) = self.infantry_do_action(id, DO_READY, true, rules) {
                log::debug!("infantry {id} Paradrop completion: {cause}");
            }
        }
        // Health zero is a retained death sequence, not UnInit. Keep its
        // native count boundary distinct from ordinary AI eligibility.
        uninitialized
            || self
                .substrate
                .entities
                .get(id)
                .is_none_or(|actor| !actor.lifecycle.object_alive)
    }

    /// The Infantry `INoticeSink` answer to the Jumpjet crash impact
    /// (`0x00522A60` -> `0x00522AF7..0x00522BA1`, notice `0x117C`), for a
    /// `Crashable=` type: SetHeight (vtable `+0x1CC`) to the bridge deck on a
    /// high-bridge cell (`[0x00A8F234]`, 416) and to 0 elsewhere, then a
    /// forced AirDeathFinish (`0x00522B9B`). The infantryman stays: its
    /// sequencer UnInits it when AirDeathFinish has played.
    pub(crate) fn infantry_crash_impact(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let crashable = self
            .object_type(actor.type_ref(), rules)
            .is_some_and(|object| object.crashable);
        if !crashable {
            return;
        }
        // GetCoords (`0x00522B24`), its truncated cell (`0x00522B29..0x00522B4F`)
        // and that cell's flags through Map[cell] (`0x00522B62`).
        let [x, y] = crate::sim::movement::ground_pose::object_center_xy(actor);
        let high_bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cell = terrain.native_cell_identity((
                crate::util::lepton::lepton_to_cell_packed(x),
                crate::util::lepton::lepton_to_cell_packed(y),
            ));
            terrain.native_cell_flags(cell) & 0x100 != 0
        });
        self.set_object_height(
            id,
            if high_bridge {
                crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
            } else {
                0
            },
            Some(rules),
            registry,
        );
        if let Err(cause) = self.infantry_do_action(id, DO_AIR_DEATH_FINISH, true, rules) {
            log::debug!("infantry {id} AirDeathFinish: {cause}");
        }
    }

    /// The locomotion action tail of `0x00520F40` (`0x00521144..0x0052130F`),
    /// which `InfantryClass::AI` runs after the sequencer (`0x0051BF7B`), for
    /// an infantryman whose Doing owns its sequence.
    ///
    /// While the locomotor's `Is_Really_Moving_Now` (vtable `+0xA8`, called at
    /// `0x00521161`, [`super::motion_query::is_really_moving_now`]) holds:
    /// Walk's class byte +0x36, else the base body (`0x004B4C50`) asking
    /// `Is_Moving_Now`, so a Jumpjet outside States 0 and 2 (`0x0054D0D0`):
    /// - A `JumpJet=` type flown by the Jumpjet locomotor (its class id against
    ///   `0x007E9AC0`) flies (Fly, 0x18) above a speed fraction of 0.8 and
    ///   hovers (Hover, 0x17) at or below it. The firing latch (`+0x68D`) holds
    ///   the fire action instead.
    /// - Otherwise it crawls when prone, else walks.
    ///
    /// Otherwise Walk, Fly and Hover return to Ready (Hover again while high
    /// flying), Crawl to Prone and Swim to Tread.
    ///
    /// The raw shared `+0x68D` is raised by firing, then cleared by discharge,
    /// refusal, class target change or the rate-zero clock arm. Its readers
    /// observe the same byte; no pending-shot cache mirrors its lifetime.
    ///
    /// The stopped Move prefix520F40..520FAF reuses the class destination
    /// and idle owners. Its NavNULL arm consumes the archived factory rally
    /// after Walk has released the barracks contact. The remaining recovery
    /// branches520FB0..521144 are separate residuals, not approximated here.
    ///
    /// Original comparison: the 20 consumer rows of
    /// `tools/spatial_oracle/infantry_movement_action.json`.
    pub(crate) fn infantry_movement_actions(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let stopped_move = self.substrate.entities.get(id).is_some_and(|actor| {
            actor.category == EntityCategory::Infantry
                && actor.is_ai_alive()
                && actor.mission.effective().known() == Some(crate::sim::mission::MissionType::Move)
                && super::motion_query::is_moving(actor) == Some(false)
        });
        if stopped_move {
            let destination = self
                .substrate
                .entities
                .get(id)
                .and_then(|actor| actor.navigation.nav_com);
            if let Some(destination) = destination {
                if let Err(cause) = self.set_infantry_destination(id, destination, rules, registry)
                {
                    log::debug!("Infantry {id} stopped Move destination: {cause}");
                }
                if let Some(actor) = self.substrate.entities.get_mut(id) {
                    actor
                        .foot_speed
                        .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
                }
            } else {
                self.infantry_enter_idle_mode(id, rules, registry);
            }
        }
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if !doing_owns_sequence(actor) || !actor.is_ai_alive() {
            return;
        }
        let Some(doing) = actor.mission_leaf.as_infantry().map(|leaf| leaf.doing()) else {
            return;
        };
        let moving_now = super::motion_query::is_really_moving_now(
            actor,
            Some(super::SpeedRules::new(
                rules,
                &self.interner,
                &self.type_handles,
                &self.houses,
            )),
            self.session.binary_frame,
        );
        let requested = if moving_now {
            let jumpjet_type = self
                .object_type(actor.type_ref(), rules)
                .is_some_and(|object| object.jumpjet);
            if jumpjet_type && uses_jumpjet_locomotor(actor) {
                let firing = actor.mission_leaf.foot_firing_sequence_latch() != 0;
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
    /// 0x0051B24F`). The class setter calls this before its DeployFire gate
    /// and before the Techno base target write. All Infantry families take
    /// the same action admission; a refused action retains Doing and stage.
    pub(crate) fn infantry_target_change_action(&mut self, id: u64, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        if actor.category != EntityCategory::Infantry || actor.health.current <= 0 {
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
        self.substrate
            .entities
            .get_mut(id)
            .expect("target-change receiver was borrowed above")
            .mission_leaf
            .set_foot_firing_sequence(0);
        if let Err(cause) = self.infantry_do_action(id, requested, false, rules) {
            log::debug!("infantry {id} target change action: {cause}");
        }
    }

    /// The firing AI's refusal at the fire frame (`0x005209FD..0x00520A51`):
    /// GetFireError is not OK, so the latch drops and the infantryman returns
    /// to an idle action, unforced: Prone when prone, else Deployed after a
    /// deploy action, else Ready. Run for an infantryman whose Doing owns its
    /// sequence, through the same Do_Action admission as its other callers.
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
        self.substrate
            .entities
            .get_mut(id)
            .expect("retained refused-fire receiver")
            .mission_leaf
            .set_foot_firing_sequence(0);
        if let Err(cause) = self.infantry_do_action(id, requested, false, rules) {
            log::debug!("infantry {id} refused fire action: {cause}");
        }
    }

    /// The sequencer's default arm (`0x00520D1B..0x00520E4A`), for an action
    /// with no arm of its own, as a forced Do_Action:
    /// - moving (locomotor `Is_Moving`, vtable `+0x10` at `0x00520D38`: the
    ///   active locomotor's query) faster than a tenth: Crawl when prone,
    ///   else Walk;
    /// - a deploy action: Deployed;
    /// - prone: Prone;
    /// - otherwise Ready.
    ///
    /// The sequencer applies the completion facing hint first. RESIDUAL:
    /// the actual AirstrikeClass+294 secondary-fire repeat at520D7E..520E00
    /// needs that manager's lifecycle; ordinary GI has no such manager.
    fn infantry_default_action(&mut self, id: u64, action: i32, rules: &RuleSet) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        //520D38 dispatches ILocomotion+10 for every family, including
        //ordinary Teleport requests. Keep Rocket's existing false fallback
        //until its destination producer/lifecycle supplies the common query.
        //Evidence: jumpjet_infantry_actions --default-motion (54 original
        //Teleport/Infantry sequencer controls) and existing Walk/Jumpjet rows.
        let is_moving = super::motion_query::is_moving(actor) == Some(true);
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
        let result = self.infantry_do_action(id, requested, true, rules);
        if let Err(cause) = result {
            log::debug!("infantry {id} action {action} completion: {cause}");
        }
    }
}

/// The type facts `Do_Action` reads besides its sequence records.
pub(super) struct DoActionType<'a> {
    pub(super) type_id: &'a str,
    /// Type `+0x5B4`.
    pub(super) movement_zone: MovementZone,
    /// `Crawls=`, Type `+0xEBD`.
    pub(super) crawls: bool,
}

#[cfg(test)]
#[path = "infantry_action_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "infantry_water_action_tests.rs"]
mod water_tests;
