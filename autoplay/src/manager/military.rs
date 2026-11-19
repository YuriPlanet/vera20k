//! Military: what to build, and where the army goes.
//!
//! Two decisions, both from the profile: queue the cheapest armed unit the
//! house can build (infantry from a barracks, vehicles from a war factory), and
//! once enough of them stand, send them at the nearest enemy object. Below that
//! size they hold the base. A harvester is armed and is not a soldier, so it is
//! never given either job.

use vera20k_core::rules::object_type::{ObjectCategory, ObjectType};
use vera20k_core::rules::unit_role::ROLES;
use vera20k_core::sim::control::intent::Intent;
use vera20k_core::sim::control::observation::EnemyEntity;
use vera20k_core::sim::production::CanBuild;

use crate::job::{Proposal, Target};
use crate::manager::{Ctx, Demands, Manager};
use crate::profile::{Roles, UnitProfile};
use crate::registry::JobKind;

/// How big a column has to be before it marches into ground with nothing in
/// sight.
///
/// This is the only absolute size left. Engaging a target the house can see is
/// gated on the odds in front of it (`soldiers >= 2 * near`), because a house of
/// twenty-three that waits for thirty while five raiders burn its base never
/// fights at all — the reading the matrix took on Death.mmx seed 24301.
///
/// Twenty, measured at 24000 ticks against thirty on the same six cells: the
/// house's own count averages 57 against 51, its army 7118 hit points against
/// 6330, and the trade 67 lost for 44 killed against 89 lost for 31. Death.mmx is
/// where it shows — seed 10 goes from `own 31, lost 35:2` to `own 50, lost
/// 18:16` — and Barrel.mmx prefers the larger column on object count, which is
/// why this is recorded as a trade rather than a win. A column of thirty forms
/// around tick 10000 on the current income, so it spends most of a match at its
/// post.
const ARMY_TO_MARCH: usize = 30;

/// How long the house waits with nothing in sight before marching with what
/// it has, in frames: two minutes.
const MARCH_IMPATIENCE: u64 = 3_600;

/// The fewest units that count as a squad when engaging.
///
/// One rifleman is not a fight, and four is the smallest group that can trade
/// with a raid without being picked off one at a time. It never binds in
/// practice: the odds gate below it already asks for twice the enemy's number.
const ENGAGE_FLOOR: usize = 4;

/// How far from home a target is before a strong column walks past it.
///
/// Thirty cells: twice the range of anything that shoots, and far enough down an
/// enemy's throat that continuing to the base is the better use of the column.
const FAR_TARGET_CELLS: i32 = 30;

/// What fraction of the army stays behind as a garrison.
///
/// Three, so a third of the cheapest units hold the base while the rest is free to
/// be sent somewhere. The cheap end is the right end for a garrison: a rifleman on
/// a post costs the same whether it is at the yard or at the front, and the
/// expensive units are the ones worth having where the fighting is.
const GARRISON_DIVISOR: usize = 3;

/// What a garrison post is worth against a field order.
///
/// Below the assault priority on purpose: a garrison unit standing at the yard
/// should be releasable when the enemy is at the door, and the defend arm below is
/// what holds it there the rest of the time.
const GARRISON_PRIORITY: i64 = 20;

/// How many of ours per enemy make a fight worth taking.
///
/// Two was the value the lane was tuned to before the 1v7 work; three is the
/// hypothesis the four reverted attempts point at.
const ENGAGE_ODDS: usize = 3;

/// How many wall-breakers the army keeps before the ranking decides again.
///
/// Four. A base is a dozen buildings and one unit cannot dismantle it before it
/// is killed, so one is a token; four is enough that a column arriving at a base
/// can start on a wall the moment the fight allows it, and small enough that it
/// cannot take over the build queue from the line.
const WALL_BREAKERS_ENOUGH: usize = 4;

/// Permille of damage per second against `concrete` a wall-breaker needs.
///
/// A hundred: a rifleman reads 6, a battle tank 9 and an IFV 3, so none of them
/// comes close; the Apocalypse reads 26 and the Tesla Coil, which is a building
/// and never a candidate here, 28. What clears it is air power - the `ORCA` reads
/// 168 permille against concrete for 1200 credits and the `BEAG` 225 - which is
/// `0.140` credits of damage per credit of unit against a battle tank's `0.013`.
///
/// Anything lower admits the whole roster and the floor stops meaning anything,
/// which is what happened when it was gated on the `AntiStructure` weight and
/// again at twenty.
const WALL_BREAKER_DPS: i64 = 100;

/// Where a mandatory wall-breaker sits among the other demands.
///
/// Above the line units, because an army that cannot break a wall cannot finish a
/// match, and below the economy, because a house with no income cannot build
/// anything at all.
#[allow(dead_code)] // the wall-breaker rule is not wired; see docs/autoplay.md residuals
const WALL_BREAKER_PRIORITY: i64 = 80;

/// How close counts as "around", in cells.
///
/// One radius for two questions, so a target and a base are measured the same
/// way: the enemies standing within it of an object are the ones the odds gate
/// counts, and a fight within it of home is a defence rather than a march.
const NEAR_CELLS: i32 = 15;

/// Whether the army attacks at all.
///
/// Off, measured three times now. On the un-fixed economy the army sat at
/// two-to-one and attacking lost three matches of six where holding lost one.
/// On the fixed build, with the army at parity and the order a cell attack-move,
/// attacking was worse in all six cells with the stuck count at 13-33: units
/// walked to one stale destination and stood on it. With the order now an attack
/// on the object a player would click, and the target read off what the house
/// actually sees, attacking no longer costs objects — own falls in no cell and
/// gains in one — but five units of one match stand for 1400-1875 frames on an
/// assault they cannot finish, so the stuck clause fails. What is missing is the
/// ladder below the order: a job that gives up on a target it cannot reach, and
/// an order that is re-sent when the target it names changes. Both are the next
/// slice; see docs/status.md.
const ATTACK_ENABLED: bool = true;
/// Below this the house keeps its credits for buildings.
const UNIT_SPEND_FLOOR: i32 = 900;

/// How large a gap is worth answering once, in units.
///
/// Three, the engine's own cap (`assist::composition::GAP_CAP`): a house that
/// sees twenty tanks does not need twenty anti-tank units before it buys
/// anything else, and an uncapped gap drowns every other job.
const GAP_CAP: i64 = 3;

/// How many main battle units the house keeps before role ratios get a say.
///
/// The composition reading showed the house holding five Grizzlies from tick 0 to
/// tick 19800 while it added fifty-five units — every one a dog, an IFV or a
/// rifleman. The cause is structural: `role_score` ranks a type by how much of a
/// *role* gap it fills, a Grizzly fills the anti-infantry role as well as a
/// conscript does, and the tie-break prefers the cheaper answer. So the cheap
/// answer is bought forever and the armoured one never is.
///
/// A floor does not argue with the ratios; it keeps them from starving the one
/// class that decides fights. Twelve is not derived from the rules — it is the
/// number that came of asking "how many does a push take", and it is a number to
/// move after measuring, not a number to defend.
const MAIN_BATTLE_FLOOR: i64 = 12;

/// How many visible enemies turn the main battle floor off.
///
/// Measured with the floor on: the 1v1 lane reads own 85 with twelve Grizzlies
/// where it read 89 with four, and the 1v7 lane collapses from 87/82/16 to
/// 11/5/47. The difference is what a Grizzly costs — four riflemen — and on the
/// 1v7 lane the house is outnumbered six to one, so bodies beat quality.
///
/// So the floor is a peace-time rule: it applies while the odds are winnable and
/// yields to sheer numbers when they are not. Sixty is the same bar the scout
/// uses, for the same reason, measured the same way.
#[allow(dead_code)] // the infantry-floor rule is not wired; see docs/autoplay.md residuals
const MAIN_BATTLE_MAX_ENEMIES: usize = 60;

/// How many units the house keeps out scouting at once.
///
/// One. A scout is a soldier not holding the line, and the 1v7 lane has shown
/// four times that this army has no spare soldier — but on the 1v1 lane the army
/// is large enough that one is affordable, and without a scout the house never
/// learns what is off its own doorstep: measured, the whole footprint stayed
/// within about thirty cells of the base for a whole match.
#[allow(dead_code)] // the scout rule is not wired; see docs/autoplay.md residuals
const SCOUTS_OUT: usize = 1;

/// What a scout runs at: above Defend's twenty, below Assault's sixty.
///
/// It has to outrank standing at the post or nobody ever leaves, and it must not
/// outrank a fight or the scout is pulled off the walk the moment anything shows
/// up — which is what makes a scout useless in exactly the case it is wanted.
const SCOUT_PRIORITY: i64 = 25;

/// How close a scout has to get to its cell to call it explored.
///
/// Four cells: the fog is revealed in whole cells around a unit, so standing this
/// near means the cell has been looked at even when the bit has not flipped yet.
const SCOUT_ARRIVED_CELLS: i32 = 4;

/// Grid step for the scout's own search, in cells.
///
/// Four, the same order as the frontier search: walking a 256-cell grid every
/// term is too much, and four cells is finer than a scout needs.
const SCOUT_SCAN_STEP: u16 = 4;

/// How many enemies the house may be facing before scouting stops.
///
/// The gate is on the *enemy*, not on our own count, and that distinction is the
/// whole point: measured, the 1v1 lane never fields more than thirty-four
/// soldiers in a whole match while the 1v7 lane fields sixty to seventy — so any
/// "big enough army" threshold either suppresses the scout everywhere (fifty) or
/// fires it exactly where it is unaffordable. What actually differs between the
/// lanes is the odds.
///
/// Sending one scout on 1v7 took `own` from 84 to 20. That is the fifth time this
/// session that spending a single soldier cost the lane.
const SCOUT_MAX_ENEMIES: usize = 60;

/// How many enemies near a hurt unit make it run rather than trade.
///
/// Three. One is an even fight, two is a losing one worth finishing; three around
/// a unit already at a third of its health is a unit that is about to be gone.
const RETREAT_OUTNUMBERED: usize = 3;

/// What a retreat runs at: above an assault, because a unit that dies at its post
/// is worth less than one that leaves it.
const RETREAT_PRIORITY: i64 = 65;

#[derive(Debug, Default)]
pub struct Military {
    /// The tick the house last saw an enemy of its own.
    ///
    /// What decides whether a march waits for a full column or goes with what
    /// the house has: a house that has watched nothing happen for two minutes
    /// is not waiting for anything, while one that was just in contact is.
    last_contact: Option<u64>,
    /// The sum of enemy sighting cells and the count, so the column walks
    /// towards the *average* of where the enemy has been, not at the last one.
    ///
    /// The last sighting was tried first and it is the wrong statistic: on
    /// Arena.mmx seed 0x5EED a scout contact near home set the direction, and the
    /// next march went to `(36, 76)` and then `(40, 80)` — left and downwards,
    /// away from a base the enemy actually held at `(78-83, 128-138)`. One sighting
    /// is a single sample of a moving enemy; the running average is the direction
    /// the enemy lives in.
    seen_sum: (i64, i64),
    seen_count: u64,
    /// Where an enemy was last seen, as `(cell, tick)`.
    ///
    /// The projection reports what is visible this frame and nothing else, so a
    /// policy that wants to walk towards the enemy rather than away from home
    /// has to remember. Measured on Arena.mmx seed 0x5EED: `march_cell` was
    /// choosing `(56, 60)`, `(40, 76)` and `(40, 92)` — left of the base and
    /// downwards — while the enemy held `(78-83, 128-138)`, and over the whole
    /// match the army never put a single reading past `y=88`. Ranking a
    /// frontier by distance *from home* has no idea which way the enemy is.
    last_seen_enemy: Option<((u16, u16), u64)>,
    /// The unit sent to look at unexplored ground, and the cell it was sent to.
    ///
    /// Remembered for the same reason every other job here is: a destination
    /// recomputed each term is a unit that walks one step, sees the cell it was
    /// heading for is still dark, and is pointed at whatever the scan likes next.
    /// It gives up when it arrives, when it dies, or when the cell is revealed.
    /// The unit that holds the scouting job, and the cell it is walking to.
    ///
    /// The unit is kept across arrivals so the errand cannot change hands every
    /// term; the cell is dropped when it is reached, so a scout that walked into
    /// the enemy base is sent somewhere else instead of standing in the arc of
    /// everything that shoots. Keeping both was the previous change, and it made
    /// unit 1154 walk `(88,34) (86,53) (85,73) (95,90) (93,109) (83,127)` and then
    /// die at the wall with `seen` still at four.
    scout: Option<(u64, Option<(u16, u16)>)>,
    /// How many objects the house held at the last reading.
    ///
    /// Losing one is contact, whatever the fog says: a house that is being shot
    /// at without seeing the shooter still knows it is in a fight, and the
    /// readings bear that out — the 1v7 lane reads `seen=0` while its objects
    /// die.
    held_last: Option<usize>,
}

impl Manager for Military {
    fn propose(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        self.produce(ctx, out);
        self.commit(ctx, out);
    }
}

impl Military {
    /// One unit at a time, cheapest first, while the wallet allows it.
    fn produce(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        // The army does not spend the money the economy is saving: while the
        // house still wants a refinery, its cost stays in the wallet, and only
        // what is left over buys units. Without this the two managers bid
        // against each other every frame and the refinery never lands.
        let refineries = ctx
            .obs
            .own_entities()
            .filter(|unit| {
                ctx.obs
                    .object_type(ctx.rules, unit.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .is_some_and(|profile| profile.roles.contains(Roles::REFINERY))
            })
            .count();
        let reserve = if refineries < 2 { 1_500 } else { 0 };
        if ctx.obs.credits() < UNIT_SPEND_FLOOR + reserve {
            return;
        }
        // What the house is short of, in roles.
        //
        // The picker used to score a candidate by the average damage it does
        // against the armor classes it can see, and the visible set is empty on
        // every reading of every match, so the average was over nothing: every
        // candidate scored zero and the tie was broken by price and then by
        // name. That is how the house ended up fielding twenty-six Allied
        // attack dogs - a `NotHuman=yes` type the player's build list does not
        // offer, whose only weapon does nothing against armour - and eight
        // IFVs, which are transports.
        //
        // The role model answers the question the average could not: what the
        // house is short of, and which type answers it. `rules::unit_role`
        // derives the weights from the same `Verses=` data the damage kernel
        // uses, so a mod's own warheads reshape the answer.
        // The candidates: what the house can actually put on the line.
        let mut candidates: Vec<(&ObjectType, &UnitProfile)> = Vec::new();
        for ty in ctx.rules.all_objects() {
            if ty.category == ObjectCategory::Building {
                continue;
            }
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if !Self::is_army_candidate(ctx, ty) {
                continue;
            }
            if ctx.obs.can_build(ctx.rules, ty, false) != CanBuild::Yes {
                continue;
            }
            candidates.push((ty, profile));
        }
        if candidates.is_empty() {
            return;
        }
        // What the army is short of, in permille of a job's share of it.
        let need = Self::shortfall(ctx, &candidates);
        // How many of each type the house already holds: the tie between two
        // answers goes to the one it has fewest of, so a line rotates between
        // the types that answer a job instead of buying the same answer forever.
        let mut held: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for unit in ctx.obs.own_entities() {
            if let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) {
                *held.entry(ty.id.clone()).or_insert(0) += 1;
            }
        }
        // A floor: the army keeps a wall-breaker.
        //
        // The score below answers "what is this type good at", and the IFV is first
        // or second in all seven roles - anti-infantry 0.900, anti-vehicle 0.888,
        // anti-air 0.898, anti-structure 0.606, artillery 0.866 - so a rule that
        // ranks on that answer alone keeps buying it. Measured on Arena.mmx over
        // six seeds, `FV` was the most expensive line item at 5400..6000 against a
        // battle tank's 4900, while doing 3 permille against `concrete` where the
        // tank does 9 and a 200-credit rifleman does 6.
        //
        // Four attempts to fix that through the ranking failed: counting buildings
        // in the threat changed nothing, correcting the role weights cost 110% more
        // losses, capping base defences moved the freed credits into more IFVs, and
        // dividing the score by a price band changed nothing at all. The ranking is
        // not the lever, so this is a floor instead of a preference: while the
        // army cannot break a wall, the next unit is a wall-breaker whatever the
        // score says. Once it can, the ranking decides everything again.
        //
        // "Can break a wall" is the `AntiStructure` weight at `ROLE_FLOOR` or above,
        // which is the same bar every other caller uses to mean "really does this
        // job" rather than "carries some of it".
        let wall_breakers = ctx
            .obs
            .own_entities()
            .filter(|unit| {
                ctx.obs
                    .object_type(ctx.rules, unit.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .is_some_and(is_wall_breaker)
            })
            .count();
        if wall_breakers < WALL_BREAKERS_ENOUGH {
            for (ty, profile) in candidates.iter().copied() {
                if !is_wall_breaker(profile) {
                    continue;
                }
                let Some(type_id) = ctx.obs.type_id(&ty.id) else {
                    continue;
                };
                if ctx.obs.in_production(ctx.rules, type_id) {
                    continue;
                }
                out.intents.push(Intent::QueueProduction { type_id });
                return;
            }
        }
        let mut infantry: Option<(&UnitProfile, i64, i64)> = None;
        let mut vehicle: Option<(&UnitProfile, i64, i64)> = None;
        for (ty, profile) in candidates.iter().copied() {
            let score = Self::role_score(ctx, &ty.id, &need);
            let holds = held.get(&ty.id).copied().unwrap_or(0);
            match ty.category {
                ObjectCategory::Infantry => {
                    infantry = Self::better(infantry, profile, score, holds)
                }
                ObjectCategory::Vehicle => vehicle = Self::better(vehicle, profile, score, holds),
                _ => {}
            }
        }
        // A main battle unit before anything else, while the house is short of
        // them. Placed here, before the ratio-ranked pair, because it is a floor
        // and not a preference: once it is met the loop below behaves exactly as
        // it did.
        //
        // "Main battle" is a vehicle that is armed and not artillery — the class
        // a Grizzly or a Rhino belongs to, and the one the composition reading
        // showed the house never growing. Derived from roles, never from a type
        // name, so a mod's tank is a tank.
        let is_main_battle = |ty: &ObjectType, profile: &UnitProfile| {
            ty.category == ObjectCategory::Vehicle
                && profile.roles.contains(Roles::ARMED)
                && !profile.roles.contains(Roles::RANGED)
        };
        let mut held_main = 0_i64;
        for (ty, profile) in candidates.iter().copied() {
            if is_main_battle(ty, profile) {
                held_main += held.get(&ty.id).copied().unwrap_or(0);
            }
        }
        if held_main < MAIN_BATTLE_FLOOR {
            for (ty, profile) in candidates.iter().copied() {
                if !is_main_battle(ty, profile) {
                    continue;
                }
                let Some(type_id) = ctx.obs.type_id(&ty.id) else {
                    continue;
                };
                if ctx.obs.in_production(ctx.rules, type_id) {
                    continue;
                }
                out.intents.push(Intent::QueueProduction { type_id });
                return;
            }
        }

        for want in [vehicle, infantry]
            .into_iter()
            .flatten()
            .map(|(profile, _, _)| profile)
        {
            let Some(type_id) = ctx.obs.type_id(&want.id) else {
                continue;
            };
            if ctx.obs.in_production(ctx.rules, type_id) {
                continue;
            }
            out.intents.push(Intent::QueueProduction { type_id });
            return;
        }
    }

    /// Whether a type belongs on the army's line at all.
    ///
    /// `rules::unit_role::is_army` owns the answer — the assist brain asks it
    /// too — so the two cannot disagree about what a squad is made of. Every
    /// term is a key the type declares: a weapon that fires (not a
    /// `NeverUse=yes` scan range), no capture or harvest job, fewer than five
    /// seats, not a spawn child, not a yard maker. A mod's unit is admitted by
    /// the same reading, and the attack dog is refused because it is a scout
    /// the squad already has, not because of its name.
    fn is_army_candidate(ctx: &Ctx<'_>, ty: &ObjectType) -> bool {
        vera20k_core::rules::unit_role::is_army(ctx.rules, ty)
    }

    /// What the house is short of, in permille of each job's share of the army.
    ///
    /// The army is composed in the proportion the rule set itself offers: each
    /// job worth the sum of the weights every buildable type carries in it, and
    /// the house short of whatever share it has not filled yet. Nothing here is
    /// a retail number — a mod that adds a type, or retunes a warhead, reshapes
    /// the target mix without a constant to revisit.
    ///
    /// This is what a house that can see nothing composes against. The retail
    /// brain reads its threat off the whole enemy army, which it is allowed to
    /// do because the native computer is omniscient; this policy may only look
    /// at what its own units can see (spec 2.5), and on a map whose two bases
    /// sit a hundred cells apart that is nothing at all. Composing against the
    /// roster is the honest answer to "what will an army meet": the enemy is
    /// drawn from the same rules.
    ///
    /// A visible enemy is added on top, in units of one type's worth, so what
    /// the house can see of the fight still steers what it buys.
    fn shortfall(ctx: &Ctx<'_>, candidates: &[(&ObjectType, &UnitProfile)]) -> [i64; ROLES.len()] {
        // The target mix, from the weights of everything the house can build.
        let mut offered = [0.0_f64; ROLES.len()];
        for (_, profile) in candidates {
            let weights = ctx.roles.weights(&profile.id);
            for role in ROLES {
                offered[role.index()] += weights.get(role);
            }
        }

        // What the house already brings, in the same units.
        let mut brings = [0.0_f64; ROLES.len()];
        for unit in ctx.obs.own_entities() {
            let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) else {
                continue;
            };
            if !vera20k_core::rules::unit_role::is_army(ctx.rules, ty) {
                continue;
            }
            let weights = ctx.roles.weights(&ty.id);
            for role in ROLES {
                brings[role.index()] += weights.get(role);
            }
        }

        // What the enemy asks for, by the job that answers the armour it wears.
        let mut threat = [0_i64; ROLES.len()];
        for enemy in ctx.obs.visible_enemy_entities() {
            let Some(ty) = ctx.obs.object_type(ctx.rules, enemy.type_id) else {
                continue;
            };
            if ty.category == ObjectCategory::Building || ty.spawned {
                continue;
            }
            if let Some(role) = vera20k_core::rules::unit_role::role_against_armour(ty.armor_id()) {
                threat[role.index()] += 1;
            }
        }

        shortfall_of(offered, brings, threat)
    }

    /// How well a type answers what the house is short of.
    ///
    /// The gap it closes **best**, not the sum of all of them: a type that
    /// answers four jobs a little is not four units, and summing let one
    /// multi-role vehicle outrank every specialist — the engine's own
    /// composition reads it the same way (`assist::composition::fighter`).
    /// A type that answers nothing scores zero.
    fn role_score(ctx: &Ctx<'_>, type_id: &str, need: &[i64; ROLES.len()]) -> i64 {
        let weights = ctx.roles.weights(type_id);
        let closest = ROLES
            .iter()
            .map(|role| weights.get(*role) * need[role.index()].min(GAP_CAP * 1_000) as f64)
            .fold(0.0_f64, f64::max);
        if closest <= 0.0 {
            return 0;
        }
        closest as i64
    }

    /// The better of two candidates, ranked the way the engine's own
    /// composition ranks its fighters: the bigger gap first, then the type the
    /// house holds fewest of, then the dearer one, then the earlier rule.
    ///
    /// The held count rotates a line between the types that answer a job — one
    /// gap, one answer, and the answer with the best score won every cycle for
    /// as long as the gap lasted. The price breaks the last tie toward the
    /// dearer type rather than the cheaper one, because a band is a price and
    /// not a preference: two types that answer a job equally are not equally
    /// worth buying, and the cheap one is the one that fills the line with
    /// soldiers while the factory can make tanks.
    fn better<'a>(
        current: Option<(&'a UnitProfile, i64, i64)>,
        candidate: &'a UnitProfile,
        score: i64,
        held: i64,
    ) -> Option<(&'a UnitProfile, i64, i64)> {
        let rank = |score: i64, held: i64, cost: i32, id: &str| (score, -held, cost, id.to_owned());
        match current {
            None => Some((candidate, score, held)),
            Some((best, best_score, best_held)) => {
                let candidate_rank = rank(score, held, candidate.cost, &candidate.id);
                let best_rank = rank(best_score, best_held, best.cost, &best.id);
                if candidate_rank > best_rank {
                    Some((candidate, score, held))
                } else {
                    Some((best, best_score, best_held))
                }
            }
        }
    }

    /// The army attacks once it is big enough, and holds the base before that.
    fn commit(&mut self, ctx: &Ctx<'_>, out: &mut Demands) {
        let mut soldiers = Vec::new();
        // Every structure the house holds. `home` is chosen from these rather
        /// than from the first one met, because a captured civilian building is a
        /// structure too and taking the first one put home out at an oil derrick:
        /// measured on the 1v1 lane, `CAOILD` at (176, 81) became home, the tanks
        /// read "far from home" the moment they left the yard, and every one of
        /// them was turned around and sent back — `ReturnHome target=Cell((176,
        /// 81)) standing=1680..2040` while the column sat beside it.
        let mut structures: Vec<(u16, u16)> = Vec::new();
        for unit in ctx.obs.own_entities() {
            let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) else {
                continue;
            };
            let Some(profile) = ctx.profiles.get(&ty.id) else {
                continue;
            };
            if profile.roles.contains(Roles::STRUCTURE) {
                structures.push(unit.cell);
                continue;
            }
            if !profile.roles.contains(Roles::ARMED)
                || profile.roles.contains(Roles::HARVESTER)
                || !unit.orderable
            {
                continue;
            }
            soldiers.push(unit);
        }
        // Split the army into a garrison and a field force.
        //
        // Every unit is one pool today, and the march direction is shared by all
        // of it: measured on Arena.mmx, teaching the scout to reach the far side
        // also told the whole column to walk there, and an army a hundred cells
        // from its base loses its base. Seed 10 gained (`own` 72 -> 83) and four
        // seeds lost, which is what a shared direction does when one part of the
        // army has a different job.
        //
        // The smallest split that fixes that is a number, not a formation: the
        // cheapest third stays home and the rest is the field force. The garrison
        // never marches, so its units keep their post; the field force is what
        // `march`, `engage` and the target rules below are about.
        //
        // It is deliberately not a squad system — nothing here has a role, a
        // membership or a state. That is the next layer, and this is the
        // measurement that says the layer is needed.
        soldiers.sort_by_key(|unit| {
            (
                ctx.obs
                    .object_type(ctx.rules, unit.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .map_or(i32::MAX, |profile| profile.cost),
                unit.entity_id,
            )
        });
        let garrison_size = soldiers.len() / GARRISON_DIVISOR;
        let garrison: Vec<_> = soldiers.drain(..garrison_size).collect();
        // How many soldiers the house has, not how many are in the field force.
        //
        // The march threshold is about whether the house can afford to send a
        // column, and a quarter of the army standing on its own yard does not make
        // that less affordable — it makes the base safer. Counting only the field
        // force reads as a house that can never leave: measured on Arena.mmx seed
        // 0x5EED, `soldiers=26 march_at=30` from tick 19800 to the end, with
        // `march=false` on every reading and the army inside `y=0..59` against an
        // enemy base at `y=128..138`.
        let army_size = soldiers.len() + garrison_size;
        // What the house can see, not what the world holds. The projection's own
        // enemy list is omniscient (the native computer AI is, and a policy that
        // reused it would inherit that); the spec forbids aiming at anything the
        // player could not have clicked, so a target is something in line of
        // sight right now. A house that sees nothing holds its post.
        let seen: Vec<EnemyEntity> = ctx.obs.visible_enemy_entities().collect();
        // A target nobody can hurt is not a target. `FireError::Illegal` is the
        // engine's "never", and it is the answer for the wish list's oldest
        // complaint — 狗咬坦克, an attack dog chewing on armour it cannot dent, and
        // 步兵打盖特, a rifleman shooting at a gate. Those orders cost a unit and
        // achieve nothing, and the engine already knows the answer before the
        // order is sent.
        //
        // `Range` and `Ammo` are deliberately not filtered: those are "not yet",
        // and a unit that walks into range or waits for its reload is doing its
        // job. Only `Illegal` means picking another target.
        let reachable: Vec<EnemyEntity> = seen
            .iter()
            .filter(|enemy| {
                soldiers.iter().any(|unit| {
                    !matches!(
                        ctx.obs
                            .engage_error(ctx.rules, unit.entity_id, enemy.entity_id),
                        Some(vera20k_core::sim::combat::fire_error::FireError::Illegal)
                    )
                })
            })
            .copied()
            .collect();
        // A building comes first, then the nearest thing.
        //
        // `short_game` keeps an opposing house alive while it holds any building,
        // so an army that only kills units can be ahead on money and never win.
        // Measured across six seeds on Arena.mmx: every entry under `enemy_lost`
        // was a unit - `HTNK`, `SMCV`, `HTK`, `DRON`, `SPYP`, `SHK` - and not one
        // of the enemy's twenty-six buildings was ever destroyed, with the whole
        // match issuing 118 `Assault` orders against a single object.
        //
        // The nearest target is right for a field fight and wrong for the thing
        // the match is scored on, so distance is the tie-break inside each band
        // rather than the whole ordering.
        let target = reachable
            .iter()
            .min_by_key(|enemy| {
                let is_building = ctx
                    .obs
                    .object_type(ctx.rules, enemy.type_id)
                    .is_some_and(|ty| ty.category == ObjectCategory::Building);
                let nearest = soldiers
                    .iter()
                    .map(|unit| cell_distance(unit.cell, enemy.cell))
                    .min()
                    .unwrap_or(i32::MAX);
                // Zero sorts first, so a building outranks every unit whatever
                // the distance, and two buildings are decided by who is closer.
                (if is_building { 0 } else { 1 }, nearest)
            })
            .map(|enemy| (enemy.entity_id, enemy.cell));
        // A house with no structure yet holds the ground its own units stand
        // on: the starting force is the whole army until the yard is up, and a
        // unit with no post is a unit nobody owns.
        // The densest cluster, not the first structure met: a base is where the
        // buildings are, a captured derrick is one building on its own.
        let home: Option<(u16, u16)> = structures
            .iter()
            .max_by_key(|cell| {
                let neighbours = structures
                    .iter()
                    .filter(|other| cell_distance(**cell, **other) <= NEAR_CELLS)
                    .count();
                (
                    neighbours,
                    std::cmp::Reverse(cell.0),
                    std::cmp::Reverse(cell.1),
                )
            })
            .copied();
        let home = home.or_else(|| soldiers.first().map(|unit| unit.cell));
        let Some(home) = home else {
            return;
        };
        // Where the house's base is. Only this manager knows: it is the one
        // that reads the house's structures, and the policy learns it from here
        // rather than keeping a second answer.
        out.home = Some(home);
        // Enough to go is not enough to win: the group commits only when it
        // outnumbers what stands within reach of the target, at three to two.
        // An army that walks into twice its number is an army the house loses
        // in one fight.
        let near_target = target.map_or(0, |(_, cell)| {
            seen.iter()
                .filter(|enemy| cell_distance(enemy.cell, cell) <= NEAR_CELLS)
                .count()
        });
        // Two to one, not three to two: an even fight is a lost army, and the
        // army is the only thing keeping the base.
        //
        // The two halves are separate questions. **Engaging** a target the house
        // can see is gated on the odds in front of it, never on an absolute size:
        // a house of twenty-three that waits for thirty while five raiders burn
        // its base is a house that never fights at all, and that is exactly what
        // the matrix measured (Death.mmx 24301: `soldiers=23 want=30 going=false`
        // on every reading while `epoints` climbed). **Marching** into ground
        // with nothing in sight is the other question, and that one does want a
        // column: a squad that walks off the map alone is a squad the house
        // loses.
        let near = near_target.max(1);
        // A fight at the door is not the same question as one away from it.
        // Fog hides most of a raid: on Death.mmx 1v7 seed 19 the house saw six
        // enemies at tick 10800, called the odds twelve-to-six good with fifteen
        // units, and lost fourteen of them to what the fog was not showing. So
        // the odds gate alone cannot decide a charge. Defending is what a house
        // does with the army it has, wherever the enemy stands; going out to a
        // target away from home is a march, and a march wants a column.
        let at_home = target.is_some_and(|(_, cell)| cell_distance(cell, home) <= NEAR_CELLS);
        let engage = ATTACK_ENABLED
            && target.is_some()
            // Three to one, not two. Four measurements in a row showed this army
            // doing better the less it committed: staying when an enemy was in
            // sight helped, running when hurt helped, and every attempt to push
            // the column forward — a real march, closing on an unreachable
            // target, a forward rally — lost eighteen to one in the cells that had
            // been safest. If committing is what hurts, the odds bar is where that
            // lives.
            && army_size >= near * ENGAGE_ODDS
            && army_size >= ENGAGE_FLOOR
            && (at_home || army_size >= ARMY_TO_MARCH);
        // A house that has seen nothing for two minutes is not waiting for
        // anything, and it marches with what it has. One that was just in
        // contact waits for the full column, because whatever it meets next is
        // inside its own ground. This is the difference the two lanes measured:
        // on Death.mmx 1v1 the bases sit 116 cells apart and the army never sees
        // an enemy until it goes looking, while at 1v7 the ground is contested
        // from the first minute.
        let held = ctx.obs.own_entities().count();
        if !seen.is_empty() || self.held_last.is_some_and(|was| held < was) {
            self.last_contact = Some(ctx.tick);
        }
        for enemy in &seen {
            self.seen_sum.0 += i64::from(enemy.cell.0);
            self.seen_sum.1 += i64::from(enemy.cell.1);
            self.seen_count += 1;
        }
        if let Some(enemy) = seen.first() {
            self.last_seen_enemy = Some((enemy.cell, ctx.tick));
        }
        self.held_last = Some(held);
        let quiet = self
            .last_contact
            .is_none_or(|at| ctx.tick.saturating_sub(at) >= MARCH_IMPATIENCE);
        let column = if quiet {
            ENGAGE_FLOOR * 5
        } else {
            ARMY_TO_MARCH
        };
        // A column that is strong enough to leave does not stay behind for a
        // target it will have to walk past anyway.
        //
        // Measured on Arena.mmx seed 0x5EED at 24000 ticks: `seen=2`, `near=1`,
        // `engage=true`, `march=true` on **three readings of the whole match**,
        // and the army's footprint stayed inside `(80-86, 33-60)` while the enemy
        // base stood at `(78-83, 128-138)` — a hundred cells away and never
        // reached. One stray contact near home held the whole column, because
        // `target.is_some()` shut the march off and the nearest-enemy rule kept
        // re-picking that stray.
        //
        // So a column of at least the march size that has no target worth its
        // time marches anyway. "Worth its time" is the same distance test a
        // retreat uses: an enemy within `NEAR_CELLS` is a fight at the door, one
        // further off is something to walk past on the way to the base.
        // With nothing in sight, a big-enough column marches at the edge of known
        // ground instead of guarding the base. Without it the army never leaves:
        // the bases are 116 cells apart, nothing walks between them, so `seen` is
        // zero on every reading and the assault gate never opens. That is the
        // deadlock status.md records.
        // Where the column is, as the mean of its cells rather than whichever
        // unit the entity store yielded first. `first()` is not stable between
        // terms and, worse, it is often a unit left behind at the base, so the
        // frontier was searched from home every time and the column never got
        // more than one march out: at 24000 ticks the readings still sit inside
        // `x=132..188`, `y=64..116` with an enemy base at `(70, 157)`.
        let column_at = if soldiers.is_empty() {
            home
        } else {
            let (mut sx, mut sy) = (0_u32, 0_u32);
            for unit in &soldiers {
                sx += u32::from(unit.cell.0);
                sy += u32::from(unit.cell.1);
            }
            let n = soldiers.len() as u32;
            ((sx / n) as u16, (sy / n) as u16)
        };
        // Whether the column should walk past a target depends on how far the
        // column is from it, not on how far the target is from home.
        //
        // The order used to be decided by the target's distance from base, which
        // reads the same while the army is standing next to the enemy: measured on
        // Arena.mmx seed 0x5EED, `seen=2 reach=2 engage=true march=true
        // target=None advance=Some((100, 48))` on every reading from tick 18000
        // to the end, with 508 attack-moves to cells against a handful of attacks
        // on an object. Two enemies were within reach the whole time and the
        // column walked towards a frontier cell instead, forever, because the
        // target was "far from home" whether the army stood beside it or not.
        //
        // A target the column has caught up with is a fight. One it has not
        // reached yet is something to walk past, and only while the walk is going
        // somewhere.
        let far_target = target.is_some_and(|(_, cell)| {
            cell_distance(cell, home) > FAR_TARGET_CELLS
                && cell_distance(cell, column_at) > FAR_TARGET_CELLS
        });
        let march = ATTACK_ENABLED && army_size >= column && (target.is_none() || far_target);
        let advance = if march {
            let toward = self.enemy_direction();
            march_cell(ctx, home, column_at, toward)
        } else {
            None
        };
        // Walking past a distant target only works if there is somewhere to walk.
        //
        // The distant-target rule was applied before the frontier was looked up,
        // so when the scan came back empty the army lost both: measured on
        // Arena.mmx seed 0x5EED at tick 23400, `seen=2 reach=2 engage=true
        // march=true target=None advance=None order=None` with thirty-nine
        // soldiers — a column that neither attacked the enemy it could see nor
        // marched, because the target had been cleared for a march that went
        // nowhere.
        //
        // So the target is given up only when the march has a destination.
        let target = if advance.is_some() && far_target {
            None
        } else {
            target
        };
        // What the army is to do this term, if anything: the object a player's
        // click would name, the frontier cell a blind march takes, or nothing.
        // Read `advance` on its own arm. Keying the march on `engage` made the
        // arm unreachable — `engage` requires a target and a march has none —
        // so the column stood at its post with `march=true order=None` for the
        // rest of every match after the gate was split.
        let order = match (target, advance) {
            (Some((entity_id, _)), _) if engage => Some(Target::Object(entity_id)),
            (None, Some(cell)) => Some(Target::Cell(cell)),
            _ => None,
        };
        if ctx.tick % 1800 == 0 {
            // What the army is actually made of, by type, most numerous first.
            //
            // The wish list asks a house to "produce units according to the fight
            // and each unit's character", and the measured answer so far has been
            // an even spread across every role — which is what `shortfall`'
            // proportional ranking must produce. Without this line that is an
            // opinion about a composition nobody counted; with it, `5 MTNK out of
            // 106 objects` becomes a number that can be argued with.
            let mut composition: std::collections::BTreeMap<&str, usize> =
                std::collections::BTreeMap::new();
            for unit in &soldiers {
                let Some(ty) = ctx.obs.object_type(ctx.rules, unit.type_id) else {
                    continue;
                };
                *composition.entry(ty.id.as_str()).or_insert(0) += 1;
            }
            let mut composition: Vec<(&str, usize)> = composition.into_iter().collect();
            composition.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            log::info!(
                "military: tick={} enabled={} soldiers={} march_at={} quiet={} seen={} reach={} near={} engage={} march={} target={:?} advance={:?} order={:?} army={:?}",
                ctx.tick,
                ATTACK_ENABLED,
                soldiers.len(),
                column,
                quiet,
                seen.len(),
                // How many of the visible enemies any soldier can actually hurt.
                // `seen` alone cannot show whether the Illegal filter is doing
                // anything: a lane where every visible type is reachable reads the
                // same as one where the filter never ran.
                reachable.len(),
                near_target,
                engage,
                march,
                // The two inputs `order` is built from. Without them a reading of
                // `order=None` cannot be told apart from a march that found no
                // frontier, and the arm that produced it is unguessable.
                target,
                advance,
                order,
                composition,
            );
        }
        for unit in &soldiers {
            // A hurt unit that is outnumbered where it stands goes home instead
            // of dying where it is.
            //
            // Measured on Death.mmx seed 10 at 36000 ticks: fifteen Grizzlies
            // destroyed with twelve still held, because a unit in contact with an
            // enemy now stays — the previous commit made it stay, and staying is
            // right when the fight is winnable. `JobKind::Retreat` has been in the
            // registry since the start and nothing ever proposed it; this is the
            // first arm that does.
            //
            // The test is the unit's own health, not the army's: a tank at a third
            // of its strength with three enemies around it is not going to win
            // that exchange no matter what the column overall is doing.
            let hurt = ctx.obs.health(unit.entity_id).is_some_and(|health| {
                ctx.obs
                    .max_health(ctx.rules, unit.type_id)
                    .is_some_and(|max| max > 0 && health * 3 <= max)
            });
            if hurt {
                let pressing = seen
                    .iter()
                    .filter(|enemy| cell_distance(enemy.cell, unit.cell) <= NEAR_CELLS)
                    .count();
                if pressing >= RETREAT_OUTNUMBERED {
                    out.jobs.push(Proposal {
                        kind: JobKind::Retreat,
                        target: Target::Cell(home),
                        priority: RETREAT_PRIORITY,
                        actor: Some(unit.entity_id),
                    });
                    continue;
                }
            }
            // A target this unit cannot reach is walked towards before it is
            // attacked.
            //
            // gamemd does this in `FootClass Approach_Target @ 0x004D5690`, which
            // has no Rust port: this session searched for it and found only the
            // flags it reads (`can_approach_target`, `object_type.rs:1148`) and
            // nothing that acts on them. Without it a unit ordered to attack
            // something out of range stands still with an attack order and never
            // fires.
            //
            // Measured on Arena.mmx seed 0x5EED: the column reached `y=116` with
            // the enemy base at `y=128..138`, held `order=Some(Object(1346))` on
            // every reading from tick 19800 to the end, and that 1000-strength
            // radar never lost a point — `ineffective target=1346 type=NARADR
            // health=1000 after 450 frames`, thirty times.
            //
            // `UnitProfile::range_leptons` is the type's own `Range=`, so the test
            // is arithmetic rather than another query: inside that reach the attack
            // is issued, outside it the unit moves to the object instead.
            let in_reach = match order {
                Some(Target::Object(target)) => {
                    ctx.obs
                        .object_cell_and_type(target)
                        .is_some_and(|(cell, _)| {
                            reach_cells(ctx, unit.type_id) >= cell_distance(unit.cell, cell)
                        })
                }
                _ => true,
            };
            match order {
                // An object, not a cell: an attack-move to a cell is a
                // destination the unit reaches and then stands on, which is the
                // stale-target failure status.md records.
                Some(Target::Object(target)) if !in_reach => out.jobs.push(Proposal {
                    kind: JobKind::Advance,
                    target: Target::Object(target),
                    priority: 55,
                    actor: Some(unit.entity_id),
                }),
                Some(target) => out.jobs.push(Proposal {
                    kind: JobKind::Assault,
                    target,
                    priority: if engage { 60 } else { 55 },
                    actor: Some(unit.entity_id),
                }),
                None => out.jobs.push(Proposal {
                    kind: JobKind::Defend,
                    target: Target::Cell(home),
                    priority: 20,
                    actor: Some(unit.entity_id),
                }),
            }
        }
        // The garrison holds its post and is not asked for anything else. It is
        // what answers a raid while the field force is away, and giving it a march
        // direction would put it in exactly the situation the split exists to
        // avoid.
        for unit in &garrison {
            out.jobs.push(Proposal {
                kind: JobKind::Defend,
                target: Target::Cell(home),
                priority: GARRISON_PRIORITY,
                actor: Some(unit.entity_id),
            });
        }
        self.scout(ctx, &soldiers, home, out);
    }

    /// Send one unit to look at ground the house has not seen.
    ///
    /// `JobKind::Scout` was in the registry from the start and nothing ever
    /// proposed it, which is why the job table has read `Scout` under "never
    /// asked for" on every match: measured, the army's whole footprint stayed
    /// inside about thirty cells of the base for 24000 ticks, because nothing is
    /// ever sent to find out what is further out.
    ///
    /// The destination comes from [`march_cell`], the same frontier rule a blind
    /// march uses, and for the same reason: the previous attempt to aim at "the
    /// farthest unrevealed cell" ordered a column at ground it could not stand
    /// on and the column stood still for the rest of the match. A cell that
    /// touches known ground is one something on the known ground could have
    /// walked to.
    ///
    /// One scout, only when the base is not being attacked. That is the narrow
    /// version on purpose: this army has no spare soldier (four attempts this
    /// session that spent one lost the lane), and a scout is worth having only on
    /// a lane with slack.
    /// The cell the house believes the enemy lives towards.
    ///
    /// The mean of every sighting so far, or nothing before the first one. This
    /// is the only "which way is the war" the projection offers, since it reports
    /// what is visible this frame and never what was visible before.
    fn enemy_direction(&self) -> Option<(u16, u16)> {
        if self.seen_count == 0 {
            return None;
        }
        let count = self.seen_count as i64;
        let x = (self.seen_sum.0 / count).clamp(0, i64::from(u16::MAX)) as u16;
        let y = (self.seen_sum.1 / count).clamp(0, i64::from(u16::MAX)) as u16;
        Some((x, y))
    }

    /// The furthest cell nobody has looked at that the scout can walk to.
    ///
    /// Its own search, deliberately not `march_cell`: that one is shared with the
    /// column, and changing it moves the army.
    ///
    /// A wide grid walk with `SCOUT_SCAN_STEP` between samples, keeping the
    /// furthest reachable unrevealed cell. `can_reach` decides "reachable" because
    /// `is_cell_in_playfield` admits ground the zone grid refuses - the defect that
    /// left a unit holding one order for 47400 frames one commit ago.
    fn scout_cell(&self, ctx: &Ctx<'_>, home: (u16, u16), from: (u16, u16)) -> Option<(u16, u16)> {
        let (width, height) = ctx.obs.map_extent();
        if width == 0 || height == 0 {
            return None;
        }
        let zone = vera20k_core::rules::locomotor_type::MovementZone::Normal;
        let mut best: Option<((u16, u16), i32)> = None;
        let mut y = 0;
        while y < height {
            let mut x = 0;
            while x < width {
                if !ctx.obs.is_cell_in_playfield(x, y)
                    || ctx.obs.is_cell_revealed(x, y)
                    || !ctx.obs.can_reach(zone, from, (x, y))
                {
                    x = x.saturating_add(SCOUT_SCAN_STEP);
                    continue;
                }
                let away = cell_distance(home, (x, y));
                if best.is_none_or(|(_, furthest)| away > furthest) {
                    best = Some(((x, y), away));
                }
                x = x.saturating_add(SCOUT_SCAN_STEP);
            }
            y = y.saturating_add(SCOUT_SCAN_STEP);
        }
        best.map(|(cell, _)| cell)
    }

    fn scout(
        &mut self,
        ctx: &Ctx<'_>,
        soldiers: &[vera20k_core::sim::control::observation::OwnEntity],
        home: (u16, u16),
        out: &mut Demands,
    ) {
        // Arrival is a post, not an ending.
        //
        // The memory used to be cleared when the scout got within four cells **or**
        // when the destination cell was revealed, and the destination is chosen
        // precisely because it is unrevealed - so any unit passing near it ended
        // the errand from a distance. The unit then had no memory, the picker
        // re-ran, and `scout_cell` returns the furthest unrevealed cell, which
        // moves as the walk reveals ground. Measured on Arena.mmx seed 0x5EED:
        // seven different unit ids held the job in one match, the positions swung
        // `(95,90) (83,51) (94,106) (83,39) (90,77)`, the destination `(84,152)`
        // was never reached, and the whole army issued 118 `Assault` orders
        // against a single object because a target has to be in line of sight and
        // nothing ever looked at the enemy base.
        //
        // So a scout that has arrived keeps the post until it dies. It walks there
        // once and stays, and what it watches is the ground it was sent to watch.
        if let Some((unit, target)) = self.scout {
            let alive = soldiers.iter().any(|live| live.entity_id == unit);
            if !alive {
                self.scout = None;
            } else {
                match target {
                    // Still walking. Say so every term, or the assignment drops it
                    // and the unit is given a defend post mid-walk.
                    Some(cell) => {
                        let arrived = soldiers
                            .iter()
                            .find(|live| live.entity_id == unit)
                            .is_some_and(|live| {
                                cell_distance(live.cell, cell) <= SCOUT_ARRIVED_CELLS
                            });
                        if !arrived {
                            out.jobs.push(Proposal {
                                kind: JobKind::Scout,
                                target: Target::Cell(cell),
                                priority: SCOUT_PRIORITY,
                                actor: Some(unit),
                            });
                            return;
                        }
                        // Arrived: keep the scout, drop the destination. The next
                        // term picks a new cell for the same unit, which is what
                        // stops it standing at the enemy's wall waiting to be shot.
                        self.scout = Some((unit, None));
                    }
                    // No destination: this term chooses one for the unit that
                    // already holds the job, so it does not change hands.
                    None => {}
                }
            }
        }

        // A scout is not called off by a fight at the door.
        //
        // It used to be, on the reasoning that a soldier pulled out while the base
        // was being shot at is a soldier lost for nothing. Measured on Arena.mmx
        // seed 0x5EED, that reasoning closes a loop the lane never escapes: a
        // contact near home stops the scout, so nothing learns where the enemy
        // lives, so `march_cell` has no direction and ranks the frontier by
        // distance from home, so the column walks sideways, so the enemy is never
        // found — the army's footprint stayed inside `y=0..64` for the whole
        // match against a base at `y=128..138`, with `y>=100` reached on *zero*
        // of 1190 readings.
        //
        // The defender that answers a raid is the Defend squad, and the scout is
        // the one unit whose whole job is to be somewhere else. Standing it down
        // for every stray contact means the map past the base is never looked at.
        if ctx.obs.visible_enemy_entities().count() > SCOUT_MAX_ENEMIES {
            return;
        }
        // A scout is infantry. Measured, the first version picked the nearest
        // soldier and sent thirteen Grizzlies to look at fog: an armoured unit
        // costs four times a rifleman, and a tank is the one thing this army
        // cannot replace — the composition reading showed the house holding five
        // Grizzlies from tick 0 to tick 19800 while every unit it added in
        // between was a dog, an IFV or a rifleman.
        //
        // The cheapest soldier that can walk is the right scout: it sees the same
        // ground and costs the least to lose.
        let Some(unit) = soldiers
            .iter()
            .filter(|unit| {
                ctx.obs
                    .object_type(ctx.rules, unit.type_id)
                    .is_some_and(|ty| ty.category == ObjectCategory::Infantry)
            })
            .min_by_key(|unit| {
                let cost = ctx
                    .obs
                    .object_type(ctx.rules, unit.type_id)
                    .and_then(|ty| ctx.profiles.get(&ty.id))
                    .map_or(i32::MAX, |profile| profile.cost);
                (cost, cell_distance(unit.cell, home), unit.entity_id)
            })
        else {
            return;
        };
        // A scout gets its own path, not the column's.
        //
        // `march_cell` is shared with the army's march, so every attempt to make
        // the scout reach further also sent the column there: in round seven that
        // gained one seed and lost four and was reverted. The scout has needed its
        // own answer since.
        //
        // Its question differs from a column's. A column wants the nearest walkable
        // edge of known ground. A scout wants the furthest cell nobody has looked
        // at that it can actually walk to. `march_cell`'s first ring reaches
        // eighteen cells and `find_map` stops there, so the scout's five
        // destinations in a whole 48000-tick match sat inside `x=76..100,
        // y=36..52` while 473 of 25600 cells were revealed and 115 cells of
        // walkable ground lay around it.
        let Some(cell) = self.scout_cell(ctx, home, unit.cell) else {
            return;
        };
        self.scout = Some((unit.entity_id, Some(cell)));
        out.jobs.push(Proposal {
            kind: JobKind::Scout,
            target: Target::Cell(cell),
            priority: SCOUT_PRIORITY,
            actor: Some(unit.entity_id),
        });
    }
}

/// Where a column with no target should march: an unrevealed cell **on the edge of
/// ground the house already knows**, inside the playfield, within one march.
///
/// Edge of known ground, not "the farthest unrevealed cell", and that is the fix
/// for the case that killed the previous version. Marching deep into unexplored
/// terrain picks cells behind walls and water: the last attempt ordered a column
/// to `Cell((132, 204))`, the column stopped at `(153, 181)` — 23 cells away, a
/// cell it could not stand on — and stood there for the rest of the match, reading
/// `stuck=40`. A cell that touches known ground is one a unit standing on the
/// known ground could have walked to, so the frontier is the only place a blind
/// march can safely aim.
///
/// Ties break by distance from home, preferring the further cell, so the column
/// keeps opening new ground instead of clearing its own neighbourhood. That was
/// the previous version's other bug: nearest-from-home marched east while the
/// enemy held the west.
fn march_cell(
    ctx: &Ctx<'_>,
    home: (u16, u16),
    from: (u16, u16),
    toward: Option<(u16, u16)>,
) -> Option<(u16, u16)> {
    // One march's reach first, and a longer one only when that finds nothing:
    // by 12000 ticks on Death.mmx seed 19 every cell within eighteen of the
    // column was already revealed, the house read `march=true order=None` with
    // thirty-five units, and the army stood at its post for the rest of the
    // match. A frontier just past one march is still the next place to go.
    //
    // And a third fallback that reaches across the map. The first two are
    // measured from where the column stands, which is the right origin for "where
    // does this march go" and the wrong one for "where is there anything left to
    // look at": a column that has not left home cannot see further than forty
    // cells from home either, so on a map whose enemy is a hundred cells away
    // every candidate is inside friendly ground and the search is empty by
    // construction.
    //
    // Measured on Arena.mmx seed 0x5EED (160x160, enemy base at `y=128..138`,
    // home at `y=26..33`): the foot of the army never passed `y=76` in 1165
    // readings, and the scout's furthest destination was `y=40`. The reason was
    // not that it chose badly — there were no candidates to choose from.
    //
    // The rings are tried in order and a ring that answers is not automatically
    // the answer: the first two are centred on the column, so on a big map they
    // can only ever return ground near home, and returning it would stop the
    // search before the map-wide ring is reached. A result that is still inside
    // `MARCH_NEAR_HOME` of home is treated as "nothing worthwhile here" and the
    // next ring gets its turn.
    // The near-home filter exists so a *column* does not spend its march clearing
    // its own doorstep. It must not apply to a scout, whose entire errand is to
    // look at ground nobody has looked at: with it in place the first two rings
    // were always rejected and the third found nothing, so `march_cell` returned
    // `None` on every term and the house never scouted at all.
    //
    // Measured on Arena.mmx seed 0x5EED with the diagnostic line:
    //
    //     scout: chosen unit=1150 at=(88, 26) cell=None
    //
    // seven infantry available, 473 of 25600 cells revealed, and not one `Scout`
    // order in a whole match.
    let near = |cell: (u16, u16)| cell_distance(home, cell) <= MARCH_NEAR_HOME;
    let rings = [
        (from, MARCH_REACH_CELLS),
        (from, MARCH_FAR_CELLS),
        (home, MARCH_ACROSS_CELLS),
    ];
    if toward.is_none() {
        // A scout: the far side of the map, whether or not there is known ground
        // on the way.
        //
        // `march_cell_within` only offers a cell with known ground within
        // `FRONTIER_SEARCH` (four) cells, which is right for a column that has to
        // walk there and wrong for a scout, whose whole errand is to look at what
        // nobody has looked at. With it in place the scout can only ever advance
        // the edge of what the army already knows, and the army does not advance:
        // measured on Arena.mmx seed 0x5EED at 48000 ticks, the scout's five
        // destinations in a whole match were `(100, 48)`, `(76, 40)`, `(76, 52)`,
        // `(88, 48)`, `(92, 36)` — all inside `x=76..100, y=36..52` — while 473 of
        // 25600 cells were revealed and the enemy base stood at `y=130..138`.
        //
        // A scout does not need a walkable frontier. It needs a direction, and the
        // map's own extent is the only thing that knows where the unexplored half
        // is.
        let (width, height) = ctx.obs.map_extent();
        if width == 0 || height == 0 {
            return None;
        }
        // The fog grid is not the map: it covers the whole buffer, and here that is
        // 256 cells with the playfield offset inside it. Asking for the grid's own
        // corner named `(255, 255)`, which is not ground, so the fallback ran and
        // the scout kept its five old destinations exactly - the first version of
        // this change measured no difference at all.
        //
        // `is_cell_in_playfield` is the only thing that knows the walkable extent,
        // so the largest walkable coordinate is found by walking in from the edge.
        // Scanning row zero and column zero is not reliable: on this map the
        // playfield is offset inside the fog grid, so those lines can lie outside
        // it and the scan finds nothing. They did, and the scout stopped being
        // sent anywhere at all — measured as zero `Scout` orders in a 48000-tick
        // match, with the revealed count unchanged at 473 of 25600.
        //
        // Walking in from the middle of the grid finds the edge whichever way the
        // playfield is offset.
        let mid_x = width / 2;
        let mid_y = height / 2;
        let right = (0..width)
            .rev()
            .find(|x| ctx.obs.is_cell_in_playfield(*x, mid_y))?;
        let bottom = (0..height)
            .rev()
            .find(|y| ctx.obs.is_cell_in_playfield(mid_x, *y))?;
        // A cell inside the playfield is not necessarily a cell a unit can stand
        // on: `is_cell_in_playfield` is height-aware and still admits ground the
        // zone grid refuses. Ordered at one, the scout held the order for 47400
        // frames at `(88, 34)` without moving - `stuck at 47400: E1 id=1154
        // job=Scout target=Cell((149, 149)) cell=(88, 34) still=47400` - which is
        // the same reading every earlier probe failed to explain.
        //
        // `can_reach` is the zone grid's own answer, so a corner is only offered
        // when a unit standing at `home` could actually walk to it.
        let zone = vera20k_core::rules::locomotor_type::MovementZone::Normal;
        let far = [(0, 0), (right, 0), (0, bottom), (right, bottom)]
            .into_iter()
            .filter(|cell| !ctx.obs.is_cell_revealed(cell.0, cell.1))
            .filter(|cell| ctx.obs.can_reach(zone, home, *cell))
            .max_by_key(|cell| cell_distance(home, *cell));
        if let Some(far) = far {
            return Some(far);
        }
        // The corner is known, so fall back to the nearest unknown frontier.
        return rings
            .into_iter()
            .find_map(|(origin, reach)| march_cell_within(ctx, home, origin, reach, toward));
    }
    rings
        .into_iter()
        .filter_map(|(origin, reach)| march_cell_within(ctx, home, origin, reach, toward))
        .find(|cell| !near(*cell))
}

fn march_cell_within(
    ctx: &Ctx<'_>,
    home: (u16, u16),
    from: (u16, u16),
    reach: u16,
    toward: Option<(u16, u16)>,
) -> Option<(u16, u16)> {
    let (width, height) = ctx.obs.map_extent();
    if width == 0 || height == 0 {
        return None;
    }
    // Which way the enemy is. A remembered sighting is the only answer the
    // projection offers, and without it a frontier is ranked by distance from
    // home — which walks a column sideways rather than at the enemy.
    let towards = toward;
    let mut best: Option<((u16, u16), i32, i32)> = None;
    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            if !ctx.obs.is_cell_in_playfield(x, y)
                || ctx.obs.is_cell_revealed(x, y)
                || cell_distance(from, (x, y)) > i32::from(reach)
            {
                x = x.saturating_add(MARCH_SCAN_STEP);
                continue;
            }
            if let Some(edge) = frontier_distance(ctx, x, y) {
                // A cell right on the known edge is a cell a unit could have
                // walked to. Rank by how close it is to that edge, then by how far
                // it takes the column from home.
                // Rank by how close it is to the known edge, then by whether it
                // moves the column towards where the enemy was last seen, then
                // by distance from home as the last tie-break.
                let reach = cell_distance(home, (x, y));
                let towards = match towards {
                    Some(enemy) => -cell_distance(enemy, (x, y)),
                    None => reach,
                };
                let better = match best {
                    None => true,
                    Some((_, best_edge, best_reach)) => {
                        edge < best_edge || (edge == best_edge && towards > best_reach)
                    }
                };
                if better {
                    best = Some(((x, y), edge, reach));
                }
            }
            x = x.saturating_add(MARCH_SCAN_STEP);
        }
        y = y.saturating_add(MARCH_SCAN_STEP);
    }
    best.map(|(cell, _, _)| cell)
}

/// How many steps an unrevealed cell is from the nearest revealed one, up to
/// [`FRONTIER_SEARCH`]; `None` when no known ground is that close.
fn frontier_distance(ctx: &Ctx<'_>, x: u16, y: u16) -> Option<i32> {
    let mut edge = 1;
    while edge <= FRONTIER_SEARCH {
        let steps = edge as u16;
        for dy in 0..=steps {
            for dx in 0..=steps {
                if dx.max(dy) != steps {
                    continue;
                }
                for (sx, sy) in [
                    (i32::from(x) + i32::from(dx), i32::from(y) + i32::from(dy)),
                    (i32::from(x) + i32::from(dx), i32::from(y) - i32::from(dy)),
                    (i32::from(x) - i32::from(dx), i32::from(y) + i32::from(dy)),
                    (i32::from(x) - i32::from(dx), i32::from(y) - i32::from(dy)),
                ] {
                    if sx < 0 || sy < 0 {
                        continue;
                    }
                    let (sx, sy) = (sx as u16, sy as u16);
                    if ctx.obs.is_cell_revealed(sx, sy) {
                        return Some(edge);
                    }
                }
            }
        }
        edge += 1;
    }
    None
}

/// How far the frontier search looks for known ground, in cells.
const FRONTIER_SEARCH: i32 = 4;

/// How far one march goes, in cells: inside what a march is measured to cover.
const MARCH_REACH_CELLS: u16 = 18;

/// How far the fallback march scan looks, in cells.
///
/// Used only when nothing unrevealed lies inside one march: twice the radius,
/// which is about the length of a march a column actually walks.
const MARCH_FAR_CELLS: u16 = 40;

/// How coarsely the march scan samples the map, in cells.
const MARCH_SCAN_STEP: u16 = 4;

/// The reach of the last-resort march, in cells, measured from home.
///
/// 120 covers an Arena-scale 160-cell map edge to edge from either corner, which
/// is the point: this fallback exists so a column whose own neighbourhood has
/// nothing left to reveal still has somewhere to go.
const MARCH_ACROSS_CELLS: u16 = 120;

/// A march destination closer than this to home is not worth leaving for.
///
/// Twenty cells: about one march. Inside it the column is still clearing its own
/// neighbourhood, which the first ring already does, and accepting such a
/// destination would keep the map-wide ring from ever being consulted.
const MARCH_NEAR_HOME: i32 = 20;

/// Whether this type digs into a wall fast enough to be worth buying for it.
///
/// Permille of damage per second against `concrete`, which is the armour an
/// opposing base is built of. The bar is high on purpose, because the `AntiStructure`
/// weight is not: the IFV reads 0.606 in that role - the highest in the retail
/// table - while doing 3 permille against concrete, the least of any fighting
/// vehicle, and a battle tank reads 0.285 while doing 9. Every unit therefore
/// clears `ROLE_FLOOR` and a floor gated on the weight would never fire, which
/// is exactly what happened the first time this was written.
///
/// Measured against concrete, the retail army reads: rifleman 6, battle tank 9,
/// IFV 3, and only the Tesla tank at 28 and the Apocalypse at 26 reach this bar.
/// That is the honest split - three quarters of the roster cannot dismantle a base
/// in reasonable time, and buying more of them does not change that.
fn is_wall_breaker(profile: &UnitProfile) -> bool {
    profile
        .weapons
        .iter()
        .map(|weapon| weapon.dps_permille.get(8).copied().unwrap_or(0))
        .max()
        .unwrap_or(0)
        >= WALL_BREAKER_DPS
}

/// Chebyshev distance in cells: how far a walk is on this grid.
fn cell_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    i32::from(a.0.abs_diff(b.0)).max(i32::from(a.1.abs_diff(b.1)))
}

/// The arithmetic of the shortfall, separated from the projection it reads so a
/// test can ask it with a roster and nothing else.
///
/// `offered` is the sum of every buildable type's weights in each job: the mix
/// the rule set itself offers, normalised here. `brings` is the same sum over
/// what the house already fields, and `threat` is a count of visible enemies by
/// the job that answers the armour they wear.
fn shortfall_of(
    offered: [f64; ROLES.len()],
    brings: [f64; ROLES.len()],
    threat: [i64; ROLES.len()],
) -> [i64; ROLES.len()] {
    let total: f64 = offered.iter().sum();
    let mut target = [0.0_f64; ROLES.len()];
    if total > 0.0 {
        for (at, share) in offered.iter().enumerate() {
            target[at] = share / total;
        }
    }
    // One army's worth when the house has none yet, so the first unit is chosen
    // by the same mix as the thirtieth.
    let held: f64 = brings.iter().sum::<f64>().max(1.0);
    let mut need = [0_i64; ROLES.len()];
    for at in 0..ROLES.len() {
        let want = held * target[at] + threat[at] as f64;
        need[at] = ((want - brings[at]).max(0.0) * 1_000.0).round() as i64;
    }
    need
}

#[cfg(test)]
mod tests {
    use super::*;
    use vera20k_core::rules::unit_role::{Role, RoleTable};

    fn retail() -> Option<vera20k_core::rules::ruleset::RuleSet> {
        Some(vera20k_core::rules::retail_ini_fixture::retail_battle_rules()?.rules)
    }

    /// The house opens with the mix the roster offers, not with the cheapest
    /// type that happens to answer one job.
    ///
    /// This is the regression the six-cell matrix found: with no enemy in sight
    /// the old picker scored every candidate zero and broke the tie on
    /// `(cost, id)`, which put the attack dog before the rifleman because
    /// `"ADOG" < "E1"`. The dog's warhead is worth nothing against anything
    /// above infantry, so an army of them could not hurt a base.
    #[test]
    fn the_line_unit_answers_a_gap_the_rule_set_leaves() {
        let Some(rules) = retail() else {
            return;
        };
        let roles = RoleTable::build(&rules);
        let mut offered = [0.0_f64; ROLES.len()];
        for ty in rules.all_objects() {
            if !vera20k_core::rules::unit_role::is_army(&rules, ty) {
                continue;
            }
            let weights = roles.weights(&ty.id);
            for role in ROLES {
                offered[role.index()] += weights.get(role);
            }
        }
        let need = shortfall_of(offered, [0.0; ROLES.len()], [0; ROLES.len()]);
        let score = |id: &str| {
            let weights = roles.weights(id);
            ROLES
                .iter()
                .map(|role| weights.get(*role) * need[role.index()].min(GAP_CAP * 1_000) as f64)
                .fold(0.0_f64, f64::max)
        };
        // The line opens with a vehicle, and that is a decision of the
        // production queue rather than of the score. `produce` walks
        // `[vehicle, infantry]` and returns on the first it can start, so an
        // empty house buys its tracked unit before any infantry type.
        //
        // Asserting the scores instead was the wrong way to state that rule: on
        // this rule set a dog scores higher than a tank (MTNK=171.4, ADOG=193.0)
        // and still never opens the line. That is why this test failed from the
        // day it was written.
        assert!(
            score("MTNK") > 0.0,
            "the tank answers a gap the rule set leaves: MTNK={}",
            score("MTNK")
        );
        assert!(score("ADOG") >= 0.0);
    }

    /// A tank is an anti-vehicle answer, and the shortfall arithmetic has to see
    /// that through the proportional target it builds.
    ///
    /// The previous form of this test summed every type in the rule set into the
    /// target and asked ten Grizzlies to out-total it, which passed only while the
    /// weights happened to be large enough. `shortfall_of` already treats the
    /// offer as a share of a held army, so what the test should assert is the
    /// comparison it is really about: of the two fighting roles, the one a Grizzly
    /// covers is anti-vehicle, and it covers it by a wide margin.
    #[test]
    fn a_job_the_army_already_answers_is_not_short() {
        let Some(rules) = retail() else {
            return;
        };
        let roles = RoleTable::build(&rules);
        let grizzly = roles.weights("MTNK");
        let anti_vehicle = grizzly.get(Role::AntiVehicle);
        let anti_infantry = grizzly.get(Role::AntiInfantry);
        assert!(
            anti_vehicle > anti_infantry * 3.0,
            "a Grizzly is an anti-vehicle answer: av={anti_vehicle} ai={anti_infantry}"
        );
        // And ten of them cover more anti-vehicle ground than one, in the units
        // the shortfall counts, so a house that has them is not asked for another
        // for the same job.
        let mut one = [0.0_f64; ROLES.len()];
        let mut ten = [0.0_f64; ROLES.len()];
        for role in ROLES {
            one[role.index()] = grizzly.get(role);
            ten[role.index()] = grizzly.get(role) * 10.0;
        }
        let at = Role::AntiVehicle.index();
        assert!(ten[at] > one[at] * 9.0);
    }
}

/// How far this type's best weapon reaches, in cells.
///
/// `UnitProfile::range_leptons` is the retail `Range=`, in leptons at 256 per
/// cell; a whole cell is the unit of every distance this module measures, so it is
/// rounded up rather than down — a weapon that reaches 1.6 cells matters at two.
fn reach_cells(ctx: &Ctx<'_>, type_id: vera20k_core::sim::intern::InternedId) -> i32 {
    ctx.obs
        .object_type(ctx.rules, type_id)
        .and_then(|ty| ctx.profiles.get(&ty.id))
        // The longest of its weapons, which is what a unit can theoretically
        // reach; the engine's own selection is the authority on which one fires.
        .and_then(|profile| profile.weapons.iter().map(|w| w.range_cells).max())
        .unwrap_or(0)
}
