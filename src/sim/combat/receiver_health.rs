//! The local health commit ends its entity borrow before world callbacks run.
//!
//! This stage retains the existing Object/Techno/Building receiver ordering.
//! Its result carries decisions across the later synchronous lifecycle stages;
//! it does not execute or defer those stages itself.

use super::*;

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
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    alliances: &HouseAllianceMap,
    attacker_owner: Option<InternedId>,
    live_source_owner: Option<InternedId>,
    receiver_outcome: Option<ResolvedReceiveDamage>,
    binary_frame: u32,
) -> Option<ReceiverHealthCommit> {
    let target_id = event.target_id;
    let mut building_entry_frame = None;
    let attacker_id = event.attacker_id;
    let mut state = damage::DamageState::Unaffected;
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
    // A source not allied with the target's owner.
    let hostile_source = |target: &GameEntity| {
        attacker_id != RAD_NO_ATTACKER
            && attacker_owner.is_some_and(|source_owner| {
                !crate::map::houses::is_allied_with(
                    alliances,
                    interner.resolve(target.owner()),
                    interner.resolve(source_owner),
                )
            })
    };
    if let Some(target) = entities.get_mut(target_id) {
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
                positive_postlude: Some((value, true, hostile_source(target))),
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
        let target_type = rules.object(interner.resolve(target.type_ref()))?;
        if target.category == EntityCategory::Structure {
            building_entry_frame = Some(crate::sim::building_art::receiver_body_frame(
                target,
                target_type,
                rules,
            ));
        }
        let strength = target_type.strength;
        let mut packet = receive_outcome.hp_delta;
        let building_no_c4 = target.category == EntityCategory::Structure && !target_type.can_c4;
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
            target
                .disguise
                .get_or_insert_with(|| {
                    crate::sim::cloak_disguise::DisguiseRuntime::new(binary_frame)
                })
                .receive_damage_reveal(binary_frame, final_packet, target.category);
        }
        // Techno70202E tests exact0 after Object returns. Negative healed HP
        // remains on its ordinary tail.
        entered_techno_death = became_fatal
            || (reached_survivor_postlude && !already_dead && target.health.current == 0);
        if entered_techno_death {
            fatal_category = target.category;
        }
        let survivor_tail = reached_survivor_postlude && !already_dead && !entered_techno_death;
        uncloak_after_damage = survivor_tail;
        let hostile_source = hostile_source(target);
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
        smoke_maintenance = survivor_tail.then_some((target.category, state));
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
