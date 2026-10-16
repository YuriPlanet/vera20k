//! Object health uses short entity borrows around synchronous Tag, Kill and
//! Destroy callbacks. Techno then reads their live consequences before its
//! anger, delay-kill and survivor/death continuations.

use super::*;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

pub(super) struct ReceiverHealthCommit {
    pub(super) building_entry_frame: Option<i32>,
    pub(super) state: damage::DamageState,
    pub(super) entered_techno_death: bool,
    #[cfg(test)]
    pub(super) reached_exact_zero: bool,
    pub(super) postmortem_candidate: Option<i32>,
    pub(super) fatal_category: EntityCategory,
    pub(super) positive_postlude: Option<(i32, bool, bool)>,
    pub(super) synchronous_retaliation: bool,
    pub(super) smoke_maintenance: Option<(EntityCategory, damage::DamageState)>,
    pub(super) healing_only: bool,
    pub(super) latch_hostile_hit: bool,
    pub(super) uncloak_after_damage: bool,
    pub(super) house_threat_delta: Option<i32>,
    /// The Psychedelic arm returned result 1 (`0x00701DBA`) before
    /// TechnoClass's own tail: only the Foot, Infantry and Unit receivers'
    /// tails follow, and they read the result, not `*damage`.
    pub(super) psychedelic: bool,
}

/// World access for the one Object5F5390 port. Observation-only callers use
/// the same body without executing the world's callback owners.
struct ReceiverHealthHost<'world, 'input, 'effects> {
    world: &'world mut Simulation,
    event: &'input EntityDamageEvent,
    rules: &'input RuleSet,
    registry: Option<&'input OverlayTypeRegistry>,
    effects: FrameEffects<'effects>,
    execute_world_callbacks: bool,
    #[cfg(test)]
    reached_exact_zero: bool,
}

impl super::object_health::ObjectHealthHost for ReceiverHealthHost<'_, '_, '_> {
    fn object(&self) -> Option<&GameEntity> {
        self.world.substrate.entities.get(self.event.target_id)
    }

    fn object_mut(&mut self) -> Option<&mut GameEntity> {
        self.world.substrate.entities.get_mut(self.event.target_id)
    }

    fn current_strength(&self) -> Option<i32> {
        let target = self.world.substrate.entities.get(self.event.target_id)?;
        Some(
            self.world
                .interner
                .object_type(target.type_ref(), self.rules)?
                .strength,
        )
    }

    fn invoke(&mut self, callback: super::object_health::HealthCallback) {
        use super::object_health::HealthCallback;
        let id = self.event.target_id;
        let context =
            UninitContext::new(Some(self.rules), self.registry).with_effects(self.effects);
        match callback {
            //456E00 is redraw/parent notification. Its visual body is outside
            //this represented receiver boundary, not a retained damaged-art latch.
            HealthCallback::Changed => {}
            HealthCallback::Tag { kind, with_source } => {
                if self.execute_world_callbacks
                    && super::world_receiver::callbacks_enabled(self.world)
                {
                    self.world.dispatch_object_tag_event(
                        id,
                        kind,
                        Some(id),
                        with_source.then_some(self.event.attacker_id),
                        context,
                    );
                }
            }
            HealthCallback::Kill => {
                #[cfg(test)]
                {
                    self.reached_exact_zero = true;
                }
                if !self.execute_world_callbacks {
                    return;
                }
                self.world.begin_receiver_kill_record(id);
                //5F5772..579A: the supplied source-House ABI pointer stays
                //fixed, but comparison against the source Techno's Owner is live.
                let attacker = self.event.attacker_id;
                let live_owner = (attacker != RAD_NO_ATTACKER)
                    .then(|| {
                        self.world
                            .substrate
                            .entities
                            .get(attacker)
                            .map(GameEntity::owner)
                    })
                    .flatten();
                let source = if let Some(house) = self.event.source_house
                    && (attacker == RAD_NO_ATTACKER || live_owner != Some(house))
                {
                    super::KillSource::House(Some(house))
                } else {
                    super::KillSource::Techno((attacker != RAD_NO_ATTACKER).then_some(attacker))
                };
                self.world
                    .record_the_kill(id, source, super::KillCallback::Terminal, context);
            }
            HealthCallback::Destroy => {
                if self.execute_world_callbacks {
                    //The existing fallback never books a loss a real Kill
                    //already booked. Both ordinary and delayed death use the
                    //same Destroy here, before Techno's PostMortem branch.
                    self.world.record_destruction_once(id);
                    if super::world_receiver::callbacks_enabled(self.world) {
                        self.world.object_destroy_callback(id, context);
                    }
                }
            }
        }
    }
}

pub(super) fn commit_receiver_health(
    event: &EntityDamageEvent,
    world: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    receiver_outcome: Option<ResolvedReceiveDamage>,
    effects: FrameEffects<'_>,
    execute_world_callbacks: bool,
) -> Option<ReceiverHealthCommit> {
    let target_id = event.target_id;
    let attacker_id = event.attacker_id;
    let receive_outcome = receiver_outcome?.outcome;
    let binary_frame = world.session.binary_frame;
    let mut building_entry_frame = None;
    let mut fatal_category = EntityCategory::Unit;
    let live_source_owner = (attacker_id != RAD_NO_ATTACKER)
        .then(|| {
            world
                .substrate
                .entities
                .get(attacker_id)
                .map(GameEntity::owner)
        })
        .flatten();
    let attacker_owner = event.source_house.or(live_source_owner);
    let hostile_source = |target: &GameEntity,
                          source_owner: Option<InternedId>,
                          alliances: &HouseAllianceMap,
                          interner: &StringInterner| {
        attacker_id != RAD_NO_ATTACKER
            && source_owner.is_some_and(|source_owner| {
                !crate::map::houses::is_allied_with(
                    alliances,
                    interner.resolve(target.owner()),
                    interner.resolve(source_owner),
                )
            })
    };
    if let Some(value) = receive_outcome.psychedelic_value {
        //701D71..701DBA returns before Object and Techno's own tail.
        //The caller owns team removal before, then target-clear/Hunt after.
        let target = world.substrate.entities.get_mut(target_id)?;
        target.berserk.start(value);
        return Some(ReceiverHealthCommit {
            building_entry_frame,
            state: damage::DamageState::Damaged,
            entered_techno_death: false,
            #[cfg(test)]
            reached_exact_zero: false,
            postmortem_candidate: None,
            fatal_category,
            positive_postlude: Some((
                value,
                true,
                hostile_source(
                    target,
                    attacker_owner,
                    &world.house_alliances,
                    &world.interner,
                ),
            )),
            synchronous_retaliation: false,
            smoke_maintenance: None,
            healing_only: false,
            latch_hostile_hit: false,
            uncloak_after_damage: false,
            house_threat_delta: None,
            psychedelic: true,
        });
    }
    let (strength, building_no_c4) = {
        let target = world.substrate.entities.get(target_id)?;
        let ty = world.interner.object_type(target.type_ref(), rules)?;
        if target.category() == EntityCategory::Structure {
            building_entry_frame = Some(crate::sim::building_art::receiver_body_frame(
                target, ty, rules,
            ));
        }
        (
            ty.strength,
            target.category() == EntityCategory::Structure && !ty.can_c4,
        )
    };
    let mut packet = receive_outcome.hp_delta;
    let mut host = ReceiverHealthHost {
        world,
        event,
        rules,
        registry,
        effects,
        execute_world_callbacks,
        #[cfg(test)]
        reached_exact_zero: false,
    };
    let state = super::object_health::commit(
        &mut host,
        &mut packet,
        strength,
        receive_outcome.apply_object_damage,
        building_no_c4,
        rules.general.condition_red,
        attacker_id != RAD_NO_ATTACKER,
    );
    #[cfg(test)]
    let reached_exact_zero = host.reached_exact_zero;
    //End the adapter's exclusive borrow before the Techno continuation.
    let world = host.world;
    let final_packet = if receive_outcome.apply_object_damage {
        packet
    } else {
        receive_outcome.post_object_damage.unwrap_or(packet)
    };
    let reached_survivor_postlude = receive_outcome.reached_survivor_postlude;
    let live_source_owner = (attacker_id != RAD_NO_ATTACKER)
        .then(|| {
            world
                .substrate
                .entities
                .get(attacker_id)
                .map(GameEntity::owner)
        })
        .flatten();
    let attacker_owner = event.source_house.or(live_source_owner);
    //Steam3e81a617: Object701DF8 returns before701E12/701E32's live Type
    //reads,701E23/701E61's live Owners and701E68's immediate anger update.
    //This precedes result5/PostMortem exits, feedback sounds and Uncloak.
    //Control-flow evidence: original701DF8..701E71; arithmetic retains the
    //existing receiver_anger_delta/House owner and its native comparisons.
    let house_threat_delta = if reached_survivor_postlude
        && let Some(source_owner) = live_source_owner
        && let Some(target) = world.substrate.entities.get(target_id)
        && let Some(ty) = world.interner.object_type(target.type_ref(), rules)
    {
        let victim_owner = target.owner();
        let delta = receiver_anger_delta(final_packet, ty.strength, rules.type_cost(ty));
        if execute_world_callbacks {
            update_anger_nodes(
                &mut world.houses,
                &world.session.house_order,
                &world.house_alliances,
                &world.interner,
                victim_owner,
                source_owner,
                delta,
            );
        }
        Some(delta)
    } else {
        None
    };
    let mut entered_techno_death = state == damage::DamageState::Dead;
    let mut postmortem_candidate = None;
    let mut positive_postlude = None;
    let mut synchronous_retaliation = false;
    let mut smoke_maintenance = None;
    let mut healing_only = false;
    let mut latch_hostile_hit = false;
    let mut uncloak_after_damage = false;
    if let Some(target) = world.substrate.entities.get_mut(target_id) {
        let ty = world.interner.object_type(target.type_ref(), rules)?;
        let already_dead = state == damage::DamageState::AlreadyDead;
        //701FCB..70202E resets disguise reveal before the live exact-zero
        //override. Dead/AlreadyDead never take this survivor-only arm.
        if reached_survivor_postlude
            && matches!(
                state,
                damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            )
            && ty.can_disguise
            && !ty.perma_disguise
        {
            let category = target.category();
            target
                .disguise
                .get_or_insert_with(|| {
                    crate::sim::cloak_disguise::DisguiseRuntime::new(binary_frame)
                })
                .receive_damage_reveal(binary_frame, final_packet, category);
        }
        //Object5's early return skips Techno70202E's independent HP override.
        entered_techno_death |=
            reached_survivor_postlude && !already_dead && target.health.current() == 0;
        if entered_techno_death {
            fatal_category = target.category();
        }
        let survivor = reached_survivor_postlude && !already_dead && !entered_techno_death;
        uncloak_after_damage = survivor;
        let hostile = hostile_source(
            target,
            attacker_owner,
            &world.house_alliances,
            &world.interner,
        );
        latch_hostile_hit = survivor && hostile;
        smoke_maintenance = survivor.then_some((target.category(), state));
        synchronous_retaliation = event.damage >= 0 && survivor;
        healing_only = packet < 0 && !entered_techno_death;
        if packet > 0 {
            positive_postlude = Some((packet, survivor, hostile));
        }
        if state == damage::DamageState::Dead {
            postmortem_candidate =
                postmortem_duration_for_event(event, target, rules, &world.interner, state);
            if postmortem_candidate.is_some() {
                positive_postlude = None;
                synchronous_retaliation = false;
                smoke_maintenance = None;
                latch_hostile_hit = false;
            }
        }
    }
    Some(ReceiverHealthCommit {
        building_entry_frame,
        state,
        entered_techno_death,
        #[cfg(test)]
        reached_exact_zero,
        postmortem_candidate,
        fatal_category,
        positive_postlude,
        synchronous_retaliation,
        smoke_maintenance,
        healing_only,
        latch_hostile_hit,
        uncloak_after_damage,
        house_threat_delta,
        psychedelic: false,
    })
}

/// Accepted Psychedelic-warhead state owned by TechnoClass: the berserk byte
/// (`Techno+0x298`) and its time, the signed dword at `+0x29C`. The receiver's
/// Psychedelic arm starts it ([`Self::start`]) and `TechnoClass::AI_Update`
/// counts it down ([`Self::count_down`]).
///
/// The time is the exact distance-zero damage kernel result, so it
/// deliberately remains signed rather than becoming a Rust duration type.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct BerserkState {
    active: bool,
    timer: i32,
}

impl BerserkState {
    /// The object is berserk.
    pub(crate) const fn is_active(self) -> bool {
        self.active
    }

    /// The time left. The Psychedelic arm and the countdown use the field;
    /// tests read it through this.
    #[cfg(test)]
    pub(crate) const fn timer(self) -> i32 {
        self.timer
    }

    /// A fixture's state.
    #[cfg(test)]
    pub(crate) fn for_test(active: bool, timer: i32) -> Self {
        Self { active, timer }
    }

    /// Install a fixture's byte.
    #[cfg(test)]
    pub(crate) fn set_active_for_test(&mut self, active: bool) {
        self.active = active;
    }

    /// Install a fixture's time.
    #[cfg(test)]
    pub(crate) fn set_timer_for_test(&mut self, timer: i32) {
        self.timer = timer;
    }

    /// The Psychedelic arm's writes (`0x00701D6B..0x00701D7D`): the time,
    /// then the flag. Answers whether this hit turned the object berserk; a
    /// berserk object only has its time refreshed.
    pub fn start(&mut self, time: i32) -> bool {
        self.timer = time;
        !std::mem::replace(&mut self.active, true)
    }

    /// `TechnoClass::AI_Update`'s countdown (`0x006F9F0D..0x006F9F31`): a
    /// berserk object's time drops by one each frame. At zero or below the
    /// flag and the time clear, and the answer is true.
    pub fn count_down(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.timer = self.timer.wrapping_sub(1);
        if self.timer > 0 {
            return false;
        }
        *self = Self::default();
        true
    }
}
