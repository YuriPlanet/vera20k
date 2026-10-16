//! The local health commit ends its entity borrow before world callbacks run.
//!
//! This stage retains the existing Object/Techno/Building receiver ordering.
//! Surviving Object damage raises its synchronous Tag events here before the
//! Techno continuation. Its result then carries decisions to later lifecycle
//! stages after re-reading any state the Tag callbacks changed.

use super::*;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

pub(super) struct ReceiverHealthCommit {
    pub(super) building_entry_frame: Option<i32>,
    pub(super) state: damage::DamageState,
    pub(super) entered_techno_death: bool,
    pub(super) reached_exact_zero: bool,
    pub(super) postmortem_candidate: Option<i32>,
    pub(super) fatal_category: EntityCategory,
    pub(super) positive_postlude: Option<(i32, bool, bool)>,
    pub(super) synchronous_retaliation: bool,
    pub(super) smoke_maintenance: Option<(EntityCategory, damage::DamageState)>,
    pub(super) healing_only: bool,
    pub(super) latch_hostile_hit: bool,
    pub(super) uncloak_after_damage: bool,
    pub(super) threat_feedback: Option<(InternedId, InternedId, i32, i32, i32)>,
    /// The Psychedelic arm returned result 1 (`0x00701DBA`) before
    /// TechnoClass's own tail: only the Foot, Infantry and Unit receivers'
    /// tails follow, and they read the result, not `*damage`.
    pub(super) psychedelic: bool,
}

pub(super) fn commit_receiver_health(
    event: &EntityDamageEvent,
    world: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    receiver_outcome: Option<ResolvedReceiveDamage>,
    effects: FrameEffects<'_>,
    callbacks: bool,
) -> Option<ReceiverHealthCommit> {
    let target_id = event.target_id;
    let mut building_entry_frame = None;
    let attacker_id = event.attacker_id;
    let state;
    let mut became_fatal = false;
    let mut entered_techno_death = false;
    let mut reached_exact_zero = false;
    let mut postmortem_candidate = None;
    let mut fatal_category = EntityCategory::Unit;
    let mut positive_postlude: Option<(i32, bool, bool)> = None;
    let mut synchronous_retaliation = false;
    let mut smoke_maintenance: Option<(EntityCategory, damage::DamageState)> = None;
    let mut healing_only = false;
    let mut latch_hostile_hit = false;
    let mut uncloak_after_damage = false;
    let mut threat_feedback: Option<(InternedId, InternedId, i32, i32, i32)> = None;
    let binary_frame = world.session.binary_frame;
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
    // A source not allied with the target's owner.
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
    let (packet, final_packet, reached_survivor_postlude, strength) = {
        let interner = &world.interner;
        let alliances = &world.house_alliances;
        let target = world.substrate.entities.get_mut(target_id)?;
        let receive_outcome = receiver_outcome?.outcome;
        if let Some(value) = receive_outcome.psychedelic_value {
            // TechnoClass writes the signed kernel result first, then the
            // flag. The first activation's callbacks (team removal, target
            // clear, Hunt) run in the caller, around this commit. Passenger
            // cargo is unrelated and remains intact. Result 1 follows, new
            // or already berserk (`0x00701DBA`).
            target.berserk.start(value);
            return Some(ReceiverHealthCommit {
                building_entry_frame,
                state: damage::DamageState::Damaged,
                entered_techno_death,
                reached_exact_zero,
                postmortem_candidate,
                fatal_category,
                positive_postlude: Some((
                    value,
                    true,
                    hostile_source(target, attacker_owner, alliances, interner),
                )),
                synchronous_retaliation,
                smoke_maintenance,
                healing_only,
                latch_hostile_hit,
                uncloak_after_damage,
                threat_feedback,
                psychedelic: true,
            });
        }
        let reached_survivor_postlude = receive_outcome.reached_survivor_postlude;
        let target_type = interner.object_type(target.type_ref(), rules)?;
        if target.category() == EntityCategory::Structure {
            building_entry_frame = Some(crate::sim::building_art::receiver_body_frame(
                target,
                target_type,
                rules,
            ));
        }
        let strength = target_type.strength;
        let mut packet = receive_outcome.hp_delta;
        let building_no_c4 = target.category() == EntityCategory::Structure && !target_type.can_c4;
        state = super::object_health::commit(
            target,
            &mut packet,
            strength,
            receive_outcome.apply_object_damage,
            building_no_c4,
            rules.general.condition_red,
            |_target, callback| match callback {
                // Native456E00's Changed callback is redraw/parent notification,
                // not a retained damaged-art health latch. Its visual body is
                // outside this represented receiver boundary.
                super::object_health::HealthCallback::Changed => {}
                super::object_health::HealthCallback::Kill => {
                    reached_exact_zero = true;
                }
                super::object_health::HealthCallback::Destroy => {
                    became_fatal = true;
                }
            },
        );
        // Object owns CanC4 rewriting and the overkill cap. Every subsequent
        // Techno reader consumes this same final packet, never the authored hit.
        let final_packet = if receive_outcome.apply_object_damage {
            packet
        } else {
            receive_outcome.post_object_damage.unwrap_or(packet)
        };
        (packet, final_packet, reached_survivor_postlude, strength)
    };
    // Object5F57B5..5807 runs both surviving-damage Tag visits before
    // returning to Techno701E5E. Both require a source Techno and re-read
    // ObjectAlive and the attachment after the preceding synchronous callback.
    // The shared Tag owner receives actor=victim and source=attacker; it reads
    // the source's current House during Event44, never a captured House pointer.
    // Steam3e81a617..08600; tools/spatial_oracle/object_health --damage-tags.
    //
    // RESIDUAL: earlier in the same body the hit raises the damage-state Tag
    // events through 0x006E53A0 (0x005F55F0..0x005F573F): 39/42 when it turns
    // the object yellow, 40/43 when it turns it red, 38/41 on its first
    // damage. VERA dispatches none of them. Trigger: a map trigger on events
    // 38-43 (FirstDamaged/HalfHealth/QuarterHealth). Effect: it never fires.
    // Frequency: unmeasured across retail maps. Risk: the scripted reaction to
    // a damaged object is missing.
    if callbacks
        && attacker_id != RAD_NO_ATTACKER
        && matches!(
            state,
            damage::DamageState::Damaged | damage::DamageState::Yellow | damage::DamageState::Red
        )
    {
        for kind in [6, 44] {
            if world
                .substrate
                .entities
                .get(target_id)
                .is_none_or(|target| !target.lifecycle.object_alive())
            {
                break;
            }
            world.dispatch_object_tag_event(
                target_id,
                kind,
                Some(target_id),
                Some(attacker_id),
                UninitContext::new(Some(rules), registry).with_effects(effects),
            );
        }
    }
    // World callbacks may change owner, health and disguise, or clear alive.
    // Every later Techno decision consumes that resulting authoritative object.
    //701DF8 returns from Object;701E23/701E61 re-read source/victim Owner
    //for anger, rather than preserving their pre-Tag House identities.
    let interner = &world.interner;
    let alliances = &world.house_alliances;
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
    if let Some(target) = world.substrate.entities.get_mut(target_id) {
        let target_type = interner.object_type(target.type_ref(), rules)?;
        // Past the anger write, Object's result 5 (ObjectAlive already clear)
        // returns at once (`0x00701E71`, `0x00701F80..0x00701F91`): no reveal,
        // death arm or survivor tail.
        let already_dead = state == damage::DamageState::AlreadyDead;
        //701FCB..70202E precedes the exact-zero override below. Native0/4
        //skip; admitted result1/2/3 resets the reveal block even if the
        //actor is already revealed. No RNG is drawn by this consequence.
        if reached_survivor_postlude
            && matches!(
                state,
                damage::DamageState::Damaged
                    | damage::DamageState::Yellow
                    | damage::DamageState::Red
            )
            && target_type.can_disguise
            && !target_type.perma_disguise
        {
            let category = target.category();
            target
                .disguise
                .get_or_insert_with(|| {
                    crate::sim::cloak_disguise::DisguiseRuntime::new(binary_frame)
                })
                .receive_damage_reveal(binary_frame, final_packet, category);
        }
        // Techno70202E tests exact0 after Object returns. Negative healed HP
        // remains on its ordinary tail.
        entered_techno_death = became_fatal
            || (reached_survivor_postlude && !already_dead && target.health.current() == 0);
        if entered_techno_death {
            fatal_category = target.category();
        }
        let survivor_tail = reached_survivor_postlude && !already_dead && !entered_techno_death;
        uncloak_after_damage = survivor_tail;
        let hostile_source = hostile_source(target, attacker_owner, alliances, interner);
        if reached_survivor_postlude && let Some(source_owner) = live_source_owner {
            threat_feedback = Some((
                target.owner(),
                source_owner,
                final_packet,
                strength,
                // Receiver anger reads the type's GetCost (vtable `+0xAC`).
                rules.type_cost(target_type),
            ));
        }
        latch_hostile_hit = survivor_tail && hostile_source;
        smoke_maintenance = survivor_tail.then_some((target.category(), state));
        synchronous_retaliation = event.damage >= 0 && survivor_tail;
        healing_only = packet < 0 && !entered_techno_death;
        if packet > 0 {
            positive_postlude = Some((packet, survivor_tail, hostile_source));
        }
        if became_fatal
            && let Some(duration) =
                postmortem_duration_for_event(event, target, rules, interner, state)
        {
            postmortem_candidate = Some(duration);
            positive_postlude = None;
            synchronous_retaliation = false;
            smoke_maintenance = None;
            latch_hostile_hit = false;
        }
    }

    Some(ReceiverHealthCommit {
        building_entry_frame,
        state,
        entered_techno_death,
        reached_exact_zero,
        postmortem_candidate,
        fatal_category,
        positive_postlude,
        synchronous_retaliation,
        smoke_maintenance,
        healing_only,
        latch_hostile_hit,
        uncloak_after_damage,
        threat_feedback,
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
