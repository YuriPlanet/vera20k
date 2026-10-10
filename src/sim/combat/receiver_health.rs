//! The local health commit ends its entity borrow before world callbacks run.
//!
//! This stage retains the existing Object/Techno/Building receiver ordering.
//! Its result carries decisions across the later synchronous lifecycle stages;
//! it does not execute or defer those stages itself.

use super::*;

pub(super) struct ReceiverHealthCommit {
    pub(super) building_entry_frame: Option<i32>,
    pub(super) became_fatal: bool,
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
    pub(super) building_damage_cue: Option<(u16, u16)>,
    pub(super) threat_feedback: Option<(InternedId, InternedId, i32, i32, i32)>,
}

#[allow(clippy::too_many_arguments)]
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
    // `BuildingClass::ReceiveDamage`'s damage-state dispatch result: the
    // building coordinate to sound the global struck cue at, or `None`.
    let mut building_damage_cue: Option<(u16, u16)> = None;
    let mut threat_feedback: Option<(InternedId, InternedId, i32, i32, i32)> = None;
    if let Some(target) = entities.get_mut(target_id) {
        let receive_outcome = receiver_outcome?.outcome;
        if let Some(value) = receive_outcome.psychedelic_value {
            // TechnoClass writes the signed kernel result first. The
            // first inactive->active transition then runs its callbacks
            // in order: team-member removal (done by the caller before
            // this commit), target clear, deferred Hunt queue. Passenger
            // cargo is unrelated and remains intact.
            target.berserk.timer = value;
            if !target.berserk.active {
                target.berserk.active = true;
                represented_assign_target(target, None);
                queue_entity_mission_deferred(target, MissionId::from_known(MissionType::Hunt));
            }
            return None;
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
        let receive_state = Some(state);
        // Object owns CanC4 rewriting and the overkill cap. Every subsequent
        // Techno reader consumes this same final packet, never the authored hit.
        let final_packet = if receive_outcome.apply_object_damage {
            packet
        } else {
            receive_outcome.post_object_damage.unwrap_or(packet)
        };
        //701FCB..70202E precedes the exact-zero override below. Native0/4
        //skip; admitted result1/2/3/5 resets the reveal block even if the
        //actor is already revealed. No RNG is drawn by this consequence.
        if reached_survivor_postlude
            && !matches!(
                state,
                damage::DamageState::Unaffected | damage::DamageState::Dead
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
        // remains on its ordinary tail; ObjectAlive/result5 is a different gate.
        entered_techno_death =
            became_fatal || (reached_survivor_postlude && target.health.current == 0);
        if entered_techno_death {
            fatal_category = target.category;
        }
        let survivor_tail = reached_survivor_postlude && !entered_techno_death;
        uncloak_after_damage = survivor_tail;
        let hostile_source = attacker_id != RAD_NO_ATTACKER
            && attacker_owner.is_some_and(|source_owner| {
                !crate::map::houses::is_allied_with(
                    alliances,
                    interner.resolve(target.owner()),
                    interner.resolve(source_owner),
                )
            });
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

        // gamemd-derived: `BuildingClass::ReceiveDamage @ 0x00442230`'s
        // damage-state dispatch, latched here and emitted below so it
        // lands after the shared Techno receiver's own consequences —
        // native only reaches it once `TechnoClass::ReceiveDamage`
        // (`0x00442425`) has returned.
        //
        // `0x0044242C MOV AL,[ESI+0x90]` is `ObjectClass::IsAlive`: a dead
        // building skips the dispatch and the function returns. Otherwise
        // `0x00442476 JMP [EAX*4 + 0x00442C18]` with `EAX = result - 2`
        // enters `{0x004426AC, 0x004426C8, 0x004424A2, 0x0044247D}`.
        // Entry 0 (result 2) multiplies the `float` at `+0xE8` of the
        // object pointed to by `BuildingClass+0x30C` by `1.5f`
        // (`0x007E4460`) when that pointer is non-null, then falls
        // through into entry 1 (result 3). That object's identity is
        // UNCHECKED — it is written once by `BuildingClass::Unlimbo @
        // 0x00440F5B` from `CALL 0x0062DC50` on `[BuildingTypeClass
        // +0x764]`, and read and rewritten by
        // `BuildingClass::UpdateGapGenerator_Tick` (`0x00454E7F`,
        // `0x0045500C`). Both entries reach
        // `0x004426D2 CMP [type+0x538],-1`, so only a type with **no**
        // `DamageSound=` of its own continues to
        // `0x00442700 MOV ECX,[Rules+0x714]` and `0x00442706 CALL
        // VocClass::PlayAtCoord @ 0x00750E20` at the building's own
        // coordinate (`0x004426DB LEA ECX,[ESI+0x9C]`). Not owner-gated,
        // draws no RNG.
        //
        // Results 2/3 are `DamageState::Yellow`/`Red` — the threshold
        // crossings `ObjectClass::ReceiveDamage @ 0x005F5390` computes
        // (2: HP went from `>= Strength >> 1` to below it; 3: from above
        // `Strength * Rules+0x1708` to below it). A hit that crosses
        // nothing returns 1 and is silent, so this is a per-crossing cue,
        // not a per-hit one.
        if matches!(
            receive_state,
            Some(damage::DamageState::Yellow | damage::DamageState::Red)
        ) && target.category == EntityCategory::Structure
            && target.lifecycle.object_alive
            && rules
                .object(interner.resolve(target.type_ref()))
                .is_some_and(|object| object.damage_sound.is_none())
        {
            building_damage_cue = Some((target.position.rx, target.position.ry));
        }
    }

    Some(ReceiverHealthCommit {
        building_entry_frame,
        state,
        became_fatal,
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
        building_damage_cue,
        threat_feedback,
    })
}
