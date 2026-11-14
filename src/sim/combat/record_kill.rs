//! `TechnoClass::Record_The_Kill @ 0x00702D40`: the kill's experience award
//! and its score and House statistics. The world owner runs its synchronous
//! concrete-class and base Tag prefixes before the arithmetic/accounting continuation.

use super::*;

/// Pay one kill's veterancy award ([`record_the_kill`]'s `points`); the
/// recipient's own cost divides it inside `VeterancyClass::Add @ 0x0074FF50`.
///
/// Who receives it is the native redirection chain at
/// `0x00702E9D..0x00702FF0`, in this order (`EDI` is the killer):
/// 1. `killer+0x82` (`InOpenTransport`, set by
///    `TechnoClass::SetInOpenTransport @ 0x00710470`) AND `killer+0x11C`
///    (`Transporter`, written beside it in `InfantryClass::PerCellProcess` at
///    `0x0051A463`) non-null AND the transporter's type `Trainable=` → the
///    TRANSPORTER, with the transporter's cost (`0x00702EA7..0x00702EF0`).
/// 2. else the killer's own type `Trainable=` → the killer
///    (`0x00702EF5..0x00702F2C`).
/// 3. else the killer's type `MissileSpawn=` (`+0xD68`) AND `killer+0x2D4`
///    (spawn owner) non-null AND its type `Trainable=` → the spawn OWNER
///    (`0x00702F31..0x00702F96`); stock `V3ROCKET`/`DMISL`/`CMISL` are
///    `Trainable=no, MissileSpawn=yes`, which is how a V3 promotes.
/// 4. else an occupied Building (`vtable+0x400`, RTTI 6) → the occupant at
///    the building's fire index (`+0x688[+0x69C]`, `0x00702F98..0x00702FEA`),
///    with no `Trainable=` test on it. A BuildingType is untrainable by
///    default (constructor `0x0045E42E`) and no retail occupiable type sets
///    the key, so every garrison kill lands here: its occupants promote, the
///    building never does. FireAt has already advanced the index past the
///    shooter (`0x006FF031..0x006FF085`) when its bullet kills, so with
///    several occupants the NEXT one in line is paid.
///    RESIDUAL: native reads that slot unchecked; VERA pays nobody when the
///    index is past the occupant count ([`combat_weapon::firing_occupant_id`]).
///    Trigger: occupants leaving, but not all of them, between the shot and
///    the kill. Effect: native reads a stale slot. Frequency: rare.
/// 5. else nobody.
///
/// Every cost on both sides is the type's Cost_Of for the VICTIM's house
/// (`victim_house`; `0x00702ED1`, `0x00702F13`, `0x00702F77`, `0x00702FD4`).
fn award_kill_experience(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    killer_id: u64,
    victim_id: u64,
    points: i32,
    victim_house: Option<&HouseCostFactors>,
) {
    if killer_id == RAD_NO_ATTACKER || killer_id == victim_id {
        return;
    }
    let cost_of = |object: &ObjectType| rules.cost_of(object, victim_house);
    let Some(killer) = entities.get(killer_id) else {
        return;
    };
    let Some(killer_type) = interner.object_type(killer.type_ref(), rules) else {
        return;
    };
    let trainable_cost = |id: u64| -> Option<(u64, i32)> {
        let object = interner.object_type(entities.get(id)?.type_ref(), rules)?;
        object.trainable.then_some((id, cost_of(object)))
    };
    // Branch 1: a passenger firing from an OpenTopped transport pays its
    // transporter (the `+0x82`/`+0x11C` pair).
    let open_transporter = killer.passenger_role.open_transport_id();
    let recipient = if let Some(transporter) = open_transporter.and_then(trainable_cost) {
        Some(transporter)
    } else if killer_type.trainable {
        // Branch 2: the killer itself.
        Some((killer_id, cost_of(killer_type)))
    } else if killer_type.missile_spawn {
        // Branch 3: a spawned missile pays its launcher.
        killer.spawn_owner_id.owner().and_then(trainable_cost)
    } else {
        // Branch 4: an occupied building pays the occupant at its fire index.
        combat_weapon::firing_occupant_id(killer, killer_type).and_then(|occupant| {
            let occupant_type = interner.object_type(entities.get(occupant)?.type_ref(), rules)?;
            Some((occupant, cost_of(occupant_type)))
        })
    };
    let Some((recipient_id, recipient_cost)) = recipient else {
        return;
    };
    if let Some(recipient) = entities.get_mut(recipient_id) {
        recipient.veterancy.award_kill(
            recipient_cost,
            points,
            true,
            rules.general.veteran_ratio,
            rules.general.veteran_cap,
        );
    }
}

/// Ares `TechnoClass_RegisterDestruction_Veterancy` (`1004EEF0`, hook
/// `0x00702E9D`, resuming at `0x00702FF5`) in place of the stock
/// `award_kill_experience`. `EDI`, the killer, first becomes its
/// Airstrike's (`+0x294`) owner when that designator's type has
/// `Experience.FromAirstrike`, which also seeds the factor with its
/// `Experience.AirstrikeModifier`. Then the recipient:
/// 1. a passenger of an open-topped transport (the same stand-in for
///    `+0x82`/`+0x11C` as the stock chain): the passenger, times the
///    transporter's `Experience.PassengerModifier`, when the transporter is
///    untrainable or has `Experience.PromotePassengers`, is elite or not
///    allied with the killer, and the killer is trainable; else the
///    transporter when it is trainable, allied and has
///    `Experience.FromPassengers`;
/// 2. a `Gunner=` killer: itself while trainable and not elite, unless it
///    carries a gunner and lacks `Experience.FromPassengers`; else its first
///    passenger, times the modifier, when the IFV is elite or lacks
///    `Experience.FromPassengers`, the gunner is trainable and the IFV has
///    `Experience.PromotePassengers` — promoted at once;
/// 3. a trainable killer; 4. a `MissileSpawn=` killer's trainable spawn
///    owner; 5. the firing occupant of a garrison, now only when trainable,
///    promoted at once.
///
/// The recipient's house must count the killer's an ally. A non-missile
/// spawn of a trainable owner shares with that owner (`Experience.SpawnModifier`
/// for the spawn, `Experience.SpawnOwnerModifier` for the owner); a mind
/// controller not allied with the victim gains `Experience.MindControlSelfModifier`
/// itself and scales the rest by `Experience.MindControlVictimModifier`.
/// Each share is AddExperience (`1004EE30`) at the recipient's own Cost_Of.
/// Every factor product is a binary64 spill, as the DLL keeps them.
/// Returns the gunner or occupant to promote immediately
/// (`UpdateVeterancy 10046AF0`, called at `1004F2C7` with no flash).
fn award_ares_kill_experience(
    entities: &mut EntityStore,
    houses: &crate::sim::houses::Houses,
    interner: &StringInterner,
    alliances: &HouseAlliances,
    rules: &RuleSet,
    killer_id: u64,
    victim_id: u64,
    points: i32,
) -> Option<u64> {
    use crate::sim::combat::veterancy::VeterancyRank;
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

    fn techno_type<'r>(
        entities: &EntityStore,
        interner: &StringInterner,
        rules: &'r RuleSet,
        id: u64,
    ) -> Option<&'r ObjectType> {
        interner.object_type(entities.get(id)?.type_ref(), rules)
    }
    fn product(lhs: NativeF64Bits, rhs: NativeF64Bits) -> NativeF64Bits {
        X87::store_f64_masked_chop(X87::mul(X87::load_f64(lhs), X87::load_f64(rhs)))
    }
    let trainable = |entities: &EntityStore, id: u64| {
        techno_type(entities, interner, rules, id).is_some_and(|object| object.trainable)
    };
    let elite = |entity: &crate::sim::game_entity::GameEntity| {
        entity.veterancy.rank_kind() == VeterancyRank::Elite
    };
    if killer_id == RAD_NO_ATTACKER || killer_id == victim_id {
        return None;
    }
    let mut killer_id = killer_id;
    let mut factor = NativeF64Bits::ONE;
    // 1004EF19..1004EF4A: the designator takes the kill.
    if let Some(designator) = entities
        .get(killer_id)
        .and_then(crate::sim::game_entity::GameEntity::airstrike_link)
        && let Some(designator_type) = techno_type(entities, interner, rules, designator)
        && designator_type.ares_veterancy().experience_from_airstrike()
    {
        factor = designator_type.ares_veterancy().airstrike_modifier();
        killer_id = designator;
    }
    let killer = entities.get(killer_id)?;
    let killer_type = interner.object_type(killer.type_ref(), rules)?;
    let killer_ext = killer_type.ares_veterancy();
    let killer_house = killer.owner();
    let mut promote_now = false;
    let recipient = if let Some(transporter_id) = killer.passenger_role.open_transport_id() {
        let transporter = entities.get(transporter_id)?;
        let transporter_type = interner.object_type(transporter.type_ref(), rules)?;
        let transporter_ext = transporter_type.ares_veterancy();
        // The transporter's House asks about the killer's (vt+3C).
        let allied = alliances.is_allied(interner, transporter.owner(), killer_house);
        if (!transporter_type.trainable || transporter_ext.passengers_gain_experience())
            && (elite(transporter) || !allied)
            && killer_type.trainable
        {
            factor = product(transporter_ext.passenger_modifier(), factor);
            Some(killer_id)
        } else if transporter_type.trainable
            && transporter_ext.experience_from_passengers()
            && allied
        {
            Some(transporter_id)
        } else {
            None
        }
    } else if killer_type.gunner {
        let gunner = killer
            .passenger_role
            .cargo()
            .and_then(|cargo| cargo.passengers().first().copied());
        if killer_type.trainable
            && !elite(killer)
            && (gunner.is_none() || killer_ext.experience_from_passengers())
        {
            Some(killer_id)
        } else if let Some(gunner) = gunner
            && (elite(killer) || !killer_ext.experience_from_passengers())
            && trainable(entities, gunner)
            && killer_ext.passengers_gain_experience()
        {
            factor = product(killer_ext.passenger_modifier(), factor);
            promote_now = true;
            Some(gunner)
        } else {
            None
        }
    } else if killer_type.trainable {
        Some(killer_id)
    } else if killer_type.missile_spawn {
        killer
            .spawn_owner_id
            .owner()
            .filter(|&owner| trainable(entities, owner))
    } else {
        // 1004F12A..1004F164: Occupants[FiringOccupantIndex] must be
        // trainable now.
        let occupant = combat_weapon::firing_occupant_id(killer, killer_type)
            .filter(|&occupant| trainable(entities, occupant));
        promote_now = occupant.is_some();
        occupant
    };
    let mut recipient = recipient?;
    // 1004F16B..1004F1A3: the recipient's House asks about the killer's.
    if !alliances.is_allied(interner, entities.get(recipient)?.owner(), killer_house) {
        return None;
    }
    // 1004F1A5..1004F1DF: a non-missile spawn of a trainable owner.
    let mut spawn = None;
    let mut spawn_factor = NativeF64Bits::ONE;
    let mut owner_factor = NativeF64Bits::ONE;
    if let Some(spawner) = entities.get(recipient)?.spawn_owner_id.owner()
        && let Some(spawner_type) = techno_type(entities, interner, rules, spawner)
        && !spawner_type.missile_spawn
        && spawner_type.trainable
    {
        spawn_factor = spawner_type.ares_veterancy().spawn_modifier();
        owner_factor = spawner_type.ares_veterancy().spawn_owner_modifier();
        spawn = Some(recipient);
        recipient = spawner;
    }
    let add = |entities: &mut EntityStore, id: u64, factor: NativeF64Bits| {
        let Some(entity) = entities.get(id) else {
            return;
        };
        let Some(object) = interner.object_type(entity.type_ref(), rules) else {
            return;
        };
        // GetActualCost(pTechno->Owner): the recipient's own House.
        let factors = houses
            .get(&entity.owner())
            .map(crate::sim::house_state::HouseState::cost_factors);
        let cost = rules.cost_of(object, factors.as_ref());
        let weighted =
            X87::ftol_i32_low_masked(X87::mul(X87::load_i32(points), X87::load_f64(factor)));
        if let Some(entity) = entities.get_mut(id) {
            entity.veterancy.add_ares_kill_experience(
                cost,
                weighted,
                rules.general.veteran_ratio,
                rules.general.veteran_cap,
            );
        }
    };
    // 1004F1E1..1004F25F: a mind controller the victim's House is not
    // allied with (asked by the controller's House) shares first.
    let victim_owner = entities
        .get(victim_id)
        .map(crate::sim::game_entity::GameEntity::owner);
    if let Some(controller) = entities
        .get(recipient)
        .and_then(|entity| entity.mind_control.controller())
        && let Some(controller_owner) = entities
            .get(controller)
            .map(crate::sim::game_entity::GameEntity::owner)
        && victim_owner
            .is_none_or(|victim| !alliances.is_allied(interner, controller_owner, victim))
        && let Some(controller_type) = techno_type(entities, interner, rules, controller)
    {
        let controller_ext = controller_type.ares_veterancy();
        if controller_type.trainable {
            add(
                entities,
                controller,
                product(controller_ext.mind_control_self_modifier(), factor),
            );
        }
        factor = product(controller_ext.mind_control_victim_modifier(), factor);
    }
    add(entities, recipient, product(factor, owner_factor));
    if let Some(spawn) = spawn {
        add(entities, spawn, product(factor, spawn_factor));
    }
    promote_now.then_some(recipient)
}

/// The caller decides whether this callback leaves the victim alive. Native
/// RecordKill always books immediately; this controls only whether the
/// callback closes VERA's [`KillRecord`] guard.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KillCallback {
    Terminal,
    OwnerChange,
}

/// Original Object virtual+E0 (Techno702D40) versus+E4 (House703230).
/// This is the actual incoming pointer kind, not a guessed source-owner
/// projection. Both native Tag prefixes forward a NULL source argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KillSource {
    Techno(Option<u64>),
    House(Option<InternedId>),
}

impl KillSource {
    fn is_present(self) -> bool {
        match self {
            Self::Techno(source) => source.is_some(),
            Self::House(source) => source.is_some(),
        }
    }
}

/// `TechnoClass::Record_The_Kill @ 0x00702D40` for `victim_id`, destroyed by
/// `killer_id` (none for a death without an attacker) and credited to
/// `killer_owner`: the kill/score record and the experience award
/// ([`award_kill_experience`]). Every cost it reads is Cost_Of for the victim's
/// house. The victim's base cost is captured before Tag callbacks at702D61;
/// recipient costs reread the victim's current House at702ED1/702F13/702FD4.
/// One award feeds both the score
/// (`0x0070300F`) and the experience: the victim's cost, zero when the
/// killer's house is allied with the victim, else doubled for a veteran or
/// tripled for an elite victim, from the rank it died at. It is not
/// `Points=`, which gamemd parses but never reads back (TS legacy).
///
/// Every routed lethal path calls it at the instant of the kill, while the
/// victim's veterancy is still the value it died at. ChangeOwner7015A8 also
/// calls it with no attacker before the owner store: that call immediately
/// books the old House's loss while the object remains alive. Each actual
/// native callback books independently; there is no first-credit guard in
/// 702D40, and UnInit books nothing (`Simulation::close_kill_record`).
/// Spawner missiles
/// credit their launcher's house; if the launcher dies during the missile's
/// flight, the kill goes uncredited.
///
/// The Iron Curtain and Genetic Mutator infantry kills, aircraft
/// self-destruct and a collapsing bridge's members reach it through
/// ReceiveDamage; passengers dying with their transport through
/// `Simulation::kill_passengers`.
///
/// An absorbed passenger that cannot leave its building reaches it in
/// `Simulation::eject_absorbed_passengers`.
fn record_the_kill(
    entities: &mut EntityStore,
    houses: &mut crate::sim::houses::Houses,
    interner: &StringInterner,
    alliances: &HouseAlliances,
    victim_id: u64,
    source: KillSource,
    victim_cost: i32,
    callback: KillCallback,
    rules: &RuleSet,
) -> Option<u64> {
    let Some(victim) = entities.get(victim_id) else {
        return None;
    };
    // E0's `DontScore=` (+C9F) returns AFTER the Tag prefix and before
    // bookkeeping. E4 has no such entry return: it awards score and victim
    // losses, but excludes DontScore from the killer's kill counters.
    // Stock sets
    // it on `SLAV`, `V3ROCKET`, `DMISL` and `CMISL`: without it every missile
    // shot down would promote its interceptor.
    if matches!(source, KillSource::Techno(_)) && victim.tracking_facts.dont_score() {
        return None;
    }
    if rules.ares()
        && let KillSource::Techno(Some(attacker)) = source
    {
        // Ares RegisterDestruction_Bounty702E64 ->1004B100 precedes the
        // original alliance/XP work, after Tags and DontScore. No E4 payout.
        crate::sim::bounty::award(
            entities, houses, interner, alliances, rules, victim_id, attacker,
        );
    }
    // Bounty changes the existing wallet and may queue a presentation sum on
    // an actor. Reborrow the victim after that visit rather than retaining a
    // mutable-store alias across the native hook. Without Ares nothing above
    // touched the store, so the reborrow finds the same victim.
    let Some(victim) = entities.get(victim_id) else {
        return None;
    };
    let killer_id = match source {
        KillSource::Techno(source) => source,
        KillSource::House(_) => None,
    };
    //702E64 and703009 read the incoming Techno's current Owner after the
    //prefix. Source removal has no fabricated fallback to a captured House.
    let killer_owner = match source {
        KillSource::Techno(source) => source
            .and_then(|source| entities.get(source))
            .map(|source| source.owner()),
        KillSource::House(house) => house,
    };
    let victim_owner = victim.owner();
    let victim_type = interner.object_type(victim.type_ref(), rules);
    let insignificant_building = victim.category() == EntityCategory::Structure
        && victim_type.is_some_and(|object| object.insignificant);
    // `0x00702E64` asks the killer's house `HouseClass::IsAlly @ 0x004F9A90`,
    // which reads only the asker's own ally bits: a one-way test.
    let asker = killer_id
        .and_then(|id| entities.get(id))
        .map(|killer| killer.owner())
        .or(killer_owner);
    let allied = asker.is_some_and(|asker| alliances.is_allied(interner, asker, victim_owner));
    let points =
        super::veterancy::kill_award_points(victim_cost, victim.veterancy.rank_kind(), allied);
    let promote_now = if rules.ares() {
        killer_id.and_then(|killer_id| {
            award_ares_kill_experience(
                entities, houses, interner, alliances, rules, killer_id, victim_id, points,
            )
        })
    } else {
        let victim_house = houses
            .get(&victim_owner)
            .map(crate::sim::house_state::HouseState::cost_factors);
        if let Some(killer_id) = killer_id {
            award_kill_experience(
                entities,
                rules,
                interner,
                killer_id,
                victim_id,
                points,
                victim_house.as_ref(),
            );
        }
        None
    };
    // 702FF0 awards experience before 703003..7031DC book the House fields.
    if let Some(victim) = entities.get_mut(victim_id) {
        let count_killer = !victim.tracking_facts.dont_score();
        record_kill_credit(
            victim,
            houses,
            killer_owner,
            points,
            insignificant_building,
            count_killer,
            callback,
        );
    }
    promote_now
}

impl crate::sim::world::Simulation {
    /// Original virtual E0/E4 world entry, including Unit744720's E0 prefix.
    /// Base702D40/703230 captures CostOf after the Unit callbacks and before
    /// its own Tag callbacks; later owner/rank/Tag reads remain live.
    /// Source/control flow: active Steam gamemd instruction bodies; existing
    /// object_health/house_statistics/veterancy corpora own the arithmetic.
    /// No own RNG, timer or detach; nested Tag actions use their existing owners.
    pub(crate) fn record_the_kill(
        &mut self,
        victim_id: u64,
        source: KillSource,
        callback: KillCallback,
        context: crate::sim::world::UninitContext<'_>,
    ) {
        let rules = context.rules();
        if matches!(source, KillSource::Techno(_))
            && self
                .substrate
                .entities
                .get(victim_id)
                .is_some_and(|victim| victim.category() == EntityCategory::Unit)
        {
            // Unit vt7F5C70+E0 ->744720..744797 (Steam gamemd).
            // 744741 tests the incoming killer only for7;48 and29 follow
            // even for NULL. Each call re-reads Object+34, without Alive/HP
            // gates. This precedes the base CostOf and its6/4 callbacks.
            // Source: original bodies/vtable via tools.native_inspect.
            //74472F..74473F: a retained captor and transferable Tag skip
            //this prefix;738374 later copies that Tag to the escaped captor.
            // No own RNG/timer/detach; synchronous Tag actions own those.
            let captor_has_tag = self
                .substrate
                .entities
                .get(victim_id)
                .is_some_and(|victim| victim.capture_infantry_type_index.get() != -1)
                && self.trigger_runtime.object_tag_is_transferable(victim_id);
            if !captor_has_tag {
                for kind in [7, 48, 29] {
                    // Ares 3.0 UnitClass_RegisterDestruction_Trigger
                    // (100506D0, hook 0x744745): with a destroyer, 85
                    // (destroyed by House) precedes 7, its source the
                    // destroyer's House; no Alive test.
                    if rules.ares()
                        && kind == 7
                        && let KillSource::Techno(Some(destroyer)) = source
                    {
                        self.raise_ares_destroyed_by_house(victim_id, destroyer, context);
                    }
                    if kind != 7 || source.is_present() {
                        self.dispatch_object_tag_event(
                            victim_id,
                            kind,
                            Some(victim_id),
                            None,
                            context,
                        );
                    }
                }
            }
        }
        let Some(victim) = self.substrate.entities.get(victim_id) else {
            return;
        };
        //702D50..61 /70323E..51 retain this base in EBP. Neither the House
        //nor rank is captured here: callback-driven changes are reread below.
        let victim_house = self
            .houses
            .get(&victim.owner())
            .map(HouseState::cost_factors);
        let victim_cost = self
            .interner
            .object_type(victim.type_ref(), rules)
            .map_or(0, |object| rules.cost_of(object, victim_house.as_ref()));
        self.record_kill_tag_prefix(victim_id, source, context);
        let promote_now = record_the_kill(
            &mut self.substrate.entities,
            &mut self.houses,
            &self.interner,
            &self.house_alliances,
            victim_id,
            source,
            victim_cost,
            callback,
            rules,
        );
        // Ares1004F2C7: a gunner or an occupant cannot reach its own AI
        // promotion sample, so the hook updates its rank now, without flash.
        // Without Ares nobody is handed back.
        if let Some(recipient) = promote_now {
            self.update_ares_veterancy(recipient, false, false, rules, context.effects());
        }
        #[cfg(any(test, feature = "test-support"))]
        self.trace_lifecycle_for_test(
            crate::sim::world::LifecycleTestEvent::RecordKillBookkeeping {
                stable_id: victim_id,
            },
        );
    }

    /// The common Tag responsibility in E0 and E4. Incoming source pointer
    /// presence gates6/4/7; Tag source itself is NULL at every original call.
    /// Only the attached object's live90 byte gates these events, not HP0,
    /// Terminal or DontScore. Each dispatch may change its owner/lifetime/Tag.
    fn record_kill_tag_prefix(
        &mut self,
        victim: u64,
        source: KillSource,
        context: crate::sim::world::UninitContext<'_>,
    ) {
        if source.is_present() {
            for kind in [6, 4] {
                if self
                    .substrate
                    .entities
                    .get(victim)
                    .is_some_and(|victim| victim.lifecycle.object_alive())
                {
                    self.dispatch_object_tag_event(victim, kind, Some(victim), None, context);
                }
            }
        }
        // Ares 3.0 TechnoClass_RegisterLoss_Trigger (10050690, hook 0x7032B0):
        // E4 raises 85 with its incoming House after 4 and before the RTTI
        // test below, so for every class, while the object is alive.
        if context.rules().ares()
            && let KillSource::House(Some(house)) = source
            && self
                .substrate
                .entities
                .get(victim)
                .is_some_and(|victim| victim.lifecycle.object_alive())
        {
            self.dispatch_object_ares_event(
                victim,
                85,
                Some(crate::sim::trigger_runtime::AresEventSource::House(house)),
                context,
            );
        }
        //702DCE/7032BE query WhatAmI once: Unit1 skips7/48/29,
        //after its earlier6/4 callbacks have already returned. Original
        //Unit vt7F5C70+2C ->746E20 returns1; Aircraft vt7E22A4+2C
        //->41C180 returns2 (the shared multiplayer_checksum RTTI mapping).
        let Some(entity) = self.substrate.entities.get(victim) else {
            return;
        };
        if !entity.lifecycle.object_alive() || entity.category() == EntityCategory::Unit {
            return;
        }
        // Ares 3.0 TechnoClass_RegisterDestruction_Trigger (10050650, hook
        // 0x702DD6): past the RTTI test, E0 raises 85 with the destroyer's
        // House before 7.
        if context.rules().ares()
            && let KillSource::Techno(Some(destroyer)) = source
        {
            self.raise_ares_destroyed_by_house(victim, destroyer, context);
        }
        for kind in [7, 48, 29] {
            if kind == 7 && !source.is_present() {
                continue;
            }
            if self
                .substrate
                .entities
                .get(victim)
                .is_some_and(|victim| victim.lifecycle.object_alive())
            {
                self.dispatch_object_tag_event(victim, kind, Some(victim), None, context);
            }
        }
    }

    /// Event 85 from E0: the source slot holds `destroyer->Owner`, read now.
    fn raise_ares_destroyed_by_house(
        &mut self,
        victim: u64,
        destroyer: u64,
        context: crate::sim::world::UninitContext<'_>,
    ) {
        let house = self.substrate.entities.get(destroyer).map(|destroyer| {
            crate::sim::trigger_runtime::AresEventSource::House(destroyer.owner())
        });
        self.dispatch_object_ares_event(victim, 85, house, context);
    }
}

/// The object's kill bookkeeping:
/// - the house credited with destroying it, which the credit step
///   ([`record_kill_credit`]) captures. gamemd's kill-record step receives the
///   actual killer at the moment of destruction; infantry linger in the logic
///   vector through a death animation, so this keeps the callback's
///   attribution. It does not defer or suppress the native score callback,
///   and saves do not keep it.
/// - a VERA guard that says the object's destruction is closed: a terminal
///   RecordTheKill callback credited it, or UnInit removed it. No booking
///   reads it (native callbacks may record again, and UnInit books nothing);
///   the dead-object check in `Simulation`'s frame validation, the hash and
///   test counts do. It does not stand in for native-alive or dying.
///
/// The credit step sets both. UnInit and ReceiveDamage's Destroy set the
/// guard (`Simulation::close_kill_record`, [`Self::close`]); the receiver's
/// next exact-zero callback and a PostMortem restore clear both
/// ([`Self::clear`]); the construction boundary clears the guard
/// ([`Self::reset_constructed`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct KillRecord {
    #[serde(skip)]
    killed_by: Option<InternedId>,
    #[serde(default)]
    destruction_recorded: bool,
}

impl KillRecord {
    /// The house credited with the kill.
    pub(crate) const fn killer(self) -> Option<InternedId> {
        self.killed_by
    }

    /// The object's destruction is closed: credited or removed.
    pub(crate) const fn is_recorded(self) -> bool {
        self.destruction_recorded
    }

    /// UnInit or Destroy closed the record, booking nothing.
    pub(crate) fn close(&mut self) {
        self.destruction_recorded = true;
    }

    /// Drop the credit and the guard: a retained hull's next exact-zero
    /// callback must capture its own credit
    /// (`ObjectSubstrate::begin_receiver_kill_record`), and a PostMortem restore
    /// keeps no killer.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The construction boundary's fact: no loss booked.
    pub(crate) fn reset_constructed(&mut self) {
        self.destruction_recorded = false;
    }

    /// Install a fixture's killer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_killed_by_for_test(&mut self, killer: Option<InternedId>) {
        self.killed_by = killer;
    }

    /// Install a fixture's guard.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_destruction_recorded_for_test(&mut self, recorded: bool) {
        self.destruction_recorded = recorded;
    }
}

/// The immediate House-accounting half of RecordKill70300F..7031DC.
/// MatchStatistics owns all mutations; retained fatal attribution is an
/// observation, never a second deferred score authority. A Temporal erase
/// calls this at full health (71AAC4) and FootCrash4DEC51 before4DEC72
/// zeroes it, so health/attacker alone cannot identify a terminal callback.
/// An attacker-free live ChangeOwner callback leaves the existing death guard
/// untouched, since the object remains alive afterwards.
///
/// Building+53C=-1 suppresses only BuildingsLost703054, including when a
/// sale's NULL callback synchronously changes owner or removes the receiver.
/// Live Tag callbacks belong to the world prefix above; the radar redraw
/// remains a separate presentation residual.
fn record_kill_credit(
    victim: &mut crate::sim::game_entity::GameEntity,
    houses: &mut crate::sim::houses::Houses,
    killer_owner: Option<InternedId>,
    points: i32,
    insignificant_building: bool,
    count_killer: bool,
    callback: KillCallback,
) {
    victim.kill_record.killed_by = killer_owner;
    if callback == KillCallback::Terminal
        || victim.health.current() == 0
        || killer_owner.is_some()
        || victim.crash.is_crashing()
    {
        victim.kill_record.destruction_recorded = true;
    }
    // Score is added before the Building Insignificant branch703045.
    if let Some(killer) = killer_owner
        && let Some(house) = houses.get_mut(&killer)
    {
        house.economy_mut().add_score(points);
    }
    if !insignificant_building {
        if (victim.category() != EntityCategory::Structure || victim.building_loss_index() != -1)
            && let Some(house) = houses.get_mut(&victim.owner())
        {
            house.stats_mut().record_loss(victim.category());
        }
        let victim_house = houses.index_of(victim.owner());
        if count_killer
            && let Some(killer) = killer_owner
            && let Some(house) = houses.get_mut(&killer)
        {
            house
                .stats_mut()
                .record_kill(victim.category(), victim_house);
        }
    }
}
