//! `TechnoClass::Record_The_Kill @ 0x00702D40`: the kill's experience award
//! and its score and House statistics. The world owner runs its synchronous
//! Tag prefix before the existing arithmetic/accounting continuation.

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
    let Some(killer_type) = rules.object(interner.resolve(killer.type_ref())) else {
        return;
    };
    let trainable_cost = |id: u64| -> Option<(u64, i32)> {
        let object = rules.object(interner.resolve(entities.get(id)?.type_ref()))?;
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
        killer.spawn_owner_id.and_then(trainable_cost)
    } else {
        // Branch 4: an occupied building pays the occupant at its fire index.
        combat_weapon::firing_occupant_id(killer, killer_type).and_then(|occupant| {
            let occupant_type =
                rules.object(interner.resolve(entities.get(occupant)?.type_ref()))?;
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

/// The caller decides whether this callback leaves the victim alive. Native
/// RecordKill always books immediately; this controls only VERA's fallback for
/// teardown sites whose native callback is still unrepresented.
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
/// 702D40. The UnInit fallback guard prevents an additional deferred record,
/// rather than suppressing this callback. Spawner missiles
/// credit their launcher's house; if the launcher dies during the missile's
/// flight, the kill goes uncredited.
///
/// NOT routed yet, because each site would need a `&RuleSet` threaded into a
/// function that does not take one: the Iron Curtain and Genetic Mutator
/// infantry kills, aircraft self-destruct, passengers dying with their
/// transport (`world::lifecycle`) or with a collapsing bridge, and passengers
/// ejected by a sell. Those victims book a Loss with no matching Kill.
fn record_the_kill(
    entities: &mut EntityStore,
    houses: &mut BTreeMap<InternedId, HouseState>,
    interner: &StringInterner,
    alliances: &HouseAllianceMap,
    victim_id: u64,
    source: KillSource,
    victim_cost: i32,
    callback: KillCallback,
    rules: &RuleSet,
) {
    let Some(victim) = entities.get(victim_id) else {
        return;
    };
    // E0's `DontScore=` (+C9F) returns AFTER the Tag prefix and before
    // bookkeeping. E4 has no such entry return: it awards score and victim
    // losses, but excludes DontScore from the killer's kill counters.
    // Stock sets
    // it on `SLAV`, `V3ROCKET`, `DMISL` and `CMISL`: without it every missile
    // shot down would promote its interceptor.
    if matches!(source, KillSource::Techno(_)) && victim.dont_score {
        return;
    }
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
    let victim_house = houses
        .get(&victim_owner)
        .map(crate::sim::house_state::HouseState::cost_factors);
    let victim_type = rules.object(interner.resolve(victim.type_ref()));
    let insignificant_building = victim.category() == EntityCategory::Structure
        && victim_type.is_some_and(|object| object.insignificant);
    // `0x00702E64` asks the killer's house `HouseClass::IsAlly @ 0x004F9A90`,
    // which reads only the asker's own ally bits: a one-way test.
    let asker = killer_id
        .and_then(|id| entities.get(id))
        .map(|killer| killer.owner())
        .or(killer_owner);
    let allied = asker.is_some_and(|asker| {
        crate::map::houses::is_allied_with(
            alliances,
            interner.resolve(asker),
            interner.resolve(victim_owner),
        )
    });
    let points =
        super::veterancy::kill_award_points(victim_cost, victim.veterancy.rank_kind(), allied);
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
    // 702FF0 awards experience before 703003..7031DC book the House fields.
    if let Some(victim) = entities.get_mut(victim_id) {
        let count_killer = !victim.dont_score;
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
}

impl crate::sim::world::Simulation {
    /// Original702D40/703230 world entry. CostOf base precedes Tag callbacks;
    /// every later owner/rank/Tag read belongs to its native continuation.
    /// Source/control flow: active Steam3e81a617 instruction bodies; existing
    /// object_health/house_statistics/veterancy corpora own the arithmetic.
    /// No own RNG, timer or detach; nested Tag actions use their existing owners.
    pub(crate) fn record_the_kill(
        &mut self,
        victim_id: u64,
        source: KillSource,
        callback: KillCallback,
        context: crate::sim::world::UninitContext<'_>,
    ) {
        let Some(rules) = context.rules() else {
            return;
        };
        let Some(victim) = self.substrate.entities.get(victim_id) else {
            return;
        };
        //702D50..61 /70323E..51 retain this base in EBP. Neither the House
        //nor rank is captured here: callback-driven changes are reread below.
        let victim_house = self
            .houses
            .get(&victim.owner())
            .map(HouseState::cost_factors);
        let victim_cost = rules
            .object(self.interner.resolve(victim.type_ref()))
            .map_or(0, |object| rules.cost_of(object, victim_house.as_ref()));
        self.record_kill_tag_prefix(victim_id, source, context);
        record_the_kill(
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
}

/// The immediate House-accounting half of RecordKill70300F..7031DC.
/// MatchStatistics owns all mutations; retained fatal attribution is an
/// observation, never a second deferred score authority. A Temporal erase
/// calls this at full health (71AAC4), so its later UnInit must also skip the
/// fallback. FootCrash4DEC51 calls at full health before4DEC72 zeroes it, so
/// health/attacker alone cannot identify a terminal callback. An attacker-free
/// live ChangeOwner callback leaves the existing death guard untouched, since
/// the object remains alive afterwards.
///
/// RESIDUAL: sale completion44A1EF writes Building+53C=-1 before its NULL kill
/// callback44A1F9, suppressing only BuildingsLost703054. VERA's existing sale
/// route does not retain that field or call RecordKill; Selling alone is not
/// its substitute. Live Tag callbacks belong to the world prefix above; the
/// radar redraw remains a separate presentation residual.
fn record_kill_credit(
    victim: &mut crate::sim::game_entity::GameEntity,
    houses: &mut BTreeMap<InternedId, HouseState>,
    killer_owner: Option<InternedId>,
    points: i32,
    insignificant_building: bool,
    count_killer: bool,
    callback: KillCallback,
) {
    victim.killed_by = killer_owner;
    if callback == KillCallback::Terminal
        || victim.health.current() == 0
        || killer_owner.is_some()
        || victim.crash.is_crashing()
    {
        victim.destruction_recorded = true;
    }
    // Score is added before the Building Insignificant branch703045.
    if let Some(killer) = killer_owner
        && let Some(house) = houses.get_mut(&killer)
    {
        house.economy.add_score(points);
    }
    if !insignificant_building {
        if let Some(house) = houses.get_mut(&victim.owner()) {
            house.stats.record_loss(victim.category());
        }
        if count_killer
            && let Some(killer) = killer_owner
            && let Some(house) = houses.get_mut(&killer)
        {
            house.stats.record_kill(victim.category());
        }
    }
}
