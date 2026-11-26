//! The army's teams: what each unit is *for*, and what it is held to.
//!
//! Above this the brain says "build this, push there"; below it a unit is told
//! to walk or shoot. What was missing is the middle: nothing owned a unit
//! between the two, so every unit was decided for individually, every frame —
//! which is why the opening squad stands in the base (nothing has given it a
//! job), why the whole army walks at one target (there is one fist and no
//! teams to split it into) and why a raid on the base is met by whoever
//! happens to be nearest.
//!
//! A squad here is a **role and a membership**, not a formation or a
//! state machine: it holds who belongs to it and what it is aimed at, and the
//! membership is rebuilt from the ledger every [`REGROUP_FRAMES`]. Rebuilding
//! rather than recruiting is the whole trick — a unit that dies simply is not
//! in the next count, and there is no "recruit / release / reform" bookkeeping
//! to get wrong. What *is* held across rebuilds is the target, for
//! [`TARGET_LOCK_FRAMES`]: a team that re-picks its target every few seconds
//! fights nothing, because it is always walking somewhere new.
//!
//! Roles are the ones a player runs: hold the base, look at what the enemy is
//! doing, and hit him. [`Role::Attack`] is the fist the advance step already
//! swings; this module is what takes units *out* of it when the base needs
//! them or when nobody is looking at the map.
//!
//! Determinism: `BTreeSet` membership in id order, sorted by squared integer
//! distance with the id as tie-break, so the same ledger gives the same squads.

use std::collections::BTreeSet;

use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use crate::rules::object_type::ObjectCategory;

use super::census::{Census, Unit};
use super::squared_distance;

/// How often the membership is rebuilt, in frames: twenty seconds. Often
/// enough that a dead unit is not carried for long, rare enough that a team is
/// not dissolved under a unit that is walking towards it.
const REGROUP_FRAMES: u64 = 300;

/// How long a team keeps its target, in frames: two minutes.
///
/// A target is worth walking to, and the walk is the expensive part. A team
/// that re-picks one every few seconds spends the whole fight walking between
/// them and arrives at none.
const TARGET_LOCK_FRAMES: u64 = 1_800;

/// How many units a raid on the base may call home.
///
/// Not the whole army: pulling everything back is what loses the base anyway —
/// the attacker only has to wait, and the house has paid for an army that is
/// standing in its own yard. Enough to meet the raid, taken from the nearest.
///
/// Eight was that "enough" and it was not. On Death.mmx seed 19 the ring held
/// fourteen raiders for most of the match, so eight defenders were outnumbered
/// and the raid farmed the base while the fist was away. Twelve is a line the
/// raid has to break instead of walk around.
///
/// Measured on the three Death.mmx seeds (america, normal, 36000 ticks),
/// against eight: the reference win arrived 5792 ticks early (35276 -> 29484)
/// with fewer losses (lost 52:112 -> 33:96), and the worst seed went from
/// own=147 enemy=10 army=82 (timeout) to own=157 enemy=1 army=93 (won).
const DEFEND_MAX: usize = 12;

/// The standing garrison: what holds the yard between raids.
///
/// [`DEFEND_MAX`] is a *ceiling* — what a raid may call home — not a standing
/// strength. A house that keeps half its army home forever is the house that
/// never attacks, and the old code did keep it: the defenders were formed only
/// when a raid was already on the base, so between raids the whole army was the
/// fist and an opening of two soldiers and a harvester marched at a construction
/// yard. Four is a squad: enough that a lone straggler does not walk into an
/// empty yard, few enough that a ten-unit fist still forms at twelve.
const PEACETIME_GARRISON: usize = 7;

/// Even a small opening needs a live defender. A strict `fighters / 3`
/// rounded down to zero for the first two soldiers, leaving the yard empty.
fn standing_garrison(fighters: usize) -> usize {
    if fighters == 0 { return 0; }
    // A five-man opening still needs its four-unit raiding detachment; keep
    // one sentinel, then scale the standing reserve as the economy grows.
    if fighters <= 5 { return 1; }
    PEACETIME_GARRISON.min((fighters + 2) / 3).min(fighters)
}

/// How near the base a hostile has to be for it to be a raid on the base, in
/// cells: the push step's own threat ring, because "the base is threatened"
/// must mean one thing. With a smaller number here an enemy inside the push's
/// ring but outside this one stopped the army from attacking and pulled nobody
/// home, so the raid met an empty yard.
const HOME_THREAT_CELLS: i32 = super::advance::THREAT_RADIUS;

/// How large the army has to be before the fist can take a base.
///
/// Below this an attack on a defended base is a house that has traded its army
/// for nothing, so the advance step keeps the fist home. The raid does not
/// wait for it: what the house does while the fist grows is [`Role::Harass`].
///
/// This is the number the advance step gates on too, so it lives here: one
/// owner for "how big is big enough".
pub(super) const ARMY_TO_ATTACK: usize = 10;

/// How many go harassing, whatever size the army is.
///
/// A few, not the rest: a raid on the ore is worth a handful of units, and the
/// house still needs something at home for the raid that answers it.
const HARASS_MAX: usize = 4;
const HARASS_MIN: usize = 2;

fn harass_allocation(fighters: usize, garrison_and_scouts: usize) -> usize {
    // V49: raid from the surplus AFTER reserving a real main fist. In V48
    // an opening of five fighters became four raiders and one defender,
    // leaving an empty Attack squad and no way to form the first wave.
    let surplus = fighters.saturating_sub(garrison_and_scouts.saturating_add(ARMY_TO_ATTACK));
    if surplus < HARASS_MIN { 0 } else { surplus.min(HARASS_MAX) }
}

/// How much stronger than what stands at the target the army has to be before
/// it walks there, in permille: a fifth again.
///
/// Equal strength is not a fight a player takes. The attacker pays for the walk
/// — he arrives under the defender's guns, spread over the approach, while the
/// defender shoots from where he already stands — so a house that pushes on
/// parity trades its army for a base it then cannot hold. A fifth is a margin
/// small enough to be reached by one more production cycle and large enough to
/// survive the walk.
///
/// It is compared in hit points ([`Census::army_points`] against
/// [`Census::enemy_points`]), not in heads: ten conscripts are not ten tanks,
/// and an army counted in heads was how the house kept walking into a garrison
/// it had no answer to.
const PUSH_SUPERIORITY_PERMILLE: i32 = 1_200;

/// How near the target a hostile has to stand to count as meeting the push
/// there, in cells.
const PUSH_MEET_CELLS: i32 = 18;

/// Whether the army is strong enough to walk into what stands at `cell`.
///
/// What the house can see is what it can weigh: an enemy inside the fog is not
/// counted, so this is a floor on the defence and not a promise about it. The
/// answer for an empty field is `true`, which is the whole point — a base with
/// nothing standing in front of it is one the army should walk into.
pub(super) fn outguns(
    census: &Census,
    rules: &RuleSet,
    obs: &Observation<'_>,
    cell: (u16, u16),
    committed_points: i32,
) -> bool {
    let theirs = census.enemy_points(rules, obs, cell, PUSH_MEET_CELLS);
    if theirs == 0 {
        return true;
    }
    committed_points.saturating_mul(1_000) >= theirs.saturating_mul(PUSH_SUPERIORITY_PERMILLE)
}

/// Whether the fight around `cell` is one the house is winning: there is
/// something to fight there, and the house's army outweighs it by the margin.
///
/// The empty case is the difference from [`outguns`]: nothing standing around
/// a unit is not a fight it is winning, and a hurt unit with an empty field
/// around it is exactly the unit that walks home to be repaired. The margin
/// is the argument because there are two questions and one comparison: the
/// push asks whether it can win (1.2), and the retreat asks whether the
/// exchange this unit is in is one worth staying in (1.5).
pub(super) fn winning_fight(
    census: &Census,
    rules: &RuleSet,
    obs: &Observation<'_>,
    cell: (u16, u16),
    permille: i32,
) -> bool {
    let theirs = census.enemy_points(rules, obs, cell, PUSH_MEET_CELLS);
    if theirs == 0 {
        return false;
    }
    let reach = PUSH_MEET_CELLS.saturating_mul(PUSH_MEET_CELLS);
    let ours = census.units.iter()
        .filter(|unit| census.available(unit))
        .filter(|unit| squared_distance(unit.cell, cell) <= reach)
        .fold(0i32, |total, unit| total.saturating_add(unit.points));
    ours.saturating_mul(1_000) >= theirs.saturating_mul(permille)
}

/// What a team is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// Meet a hostile that has come into the base.
    Defend,
    /// Look at the map: what the enemy is building, where his ore is going.
    Scout,
    /// Go at what pays for the enemy's army: the ore, and the bays it is
    /// carried to. What an army too small to take a base should be doing.
    Harass,
    /// The fist: what is left goes to the advance step.
    Attack,
}

/// What a team is doing about its own strength.
///
/// The membership is rebuilt from the ledger, so a dead unit is simply absent
/// from the next count and there is no recruit/release bookkeeping to get wrong.
/// What a bare membership cannot say is whether the team is *whole*: a fist of
/// six that is meant to hold ten should not march, and a garrison of three that
/// is meant to hold four should be asking production for one more. `Muster` is
/// that difference, and it is what the production step reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SquadState {
    /// Below [`Squad::want`], and the house is meant to be filling it.
    Muster,
    /// At strength, or doing the job regardless.
    Ready,
}

/// One team: who is in it, what it is aimed at, and until when.
#[derive(Debug, Clone)]
pub(super) struct Squad {
    pub(super) role: Role,
    pub(super) members: BTreeSet<u64>,
    /// What the team is aimed at, when it is aimed at one object.
    pub(super) target: Option<u64>,
    /// The frame the team may re-pick its target on.
    pub(super) lock_until: u64,
    /// What the team is meant to hold. The designed strength, not the count: a
    /// team rebuilt to whatever is alive would call six a fist and never ask for
    /// the other four.
    pub(super) want: usize,
    /// Whether it is at `want`, and what it is doing about the difference.
    pub(super) state: SquadState,
}

impl Squad {
    /// Whether this team is still holding its target at `tick`.
    pub(super) fn locked(&self, tick: u64) -> bool {
        self.target.is_some() && tick < self.lock_until
    }
}

/// Every team, and when the membership was last rebuilt.
#[derive(Debug, Default)]
pub(super) struct Squads {
    squads: Vec<Squad>,
    /// When the membership was last rebuilt. `None` before the first one, so
    /// the army has teams on the first frame rather than twenty seconds into
    /// the match — which is when the opening squad is standing in the base.
    formed_at: Option<u64>,
    /// The raider the teams were last rebuilt for. A new one is answered on the
    /// frame it appears; the same one is answered on the interval, so a raid
    /// that no unit can reach does not rebuild the whole army every frame.
    answered_raid: Option<u64>,
}

impl Squads {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Every object standing in a team, by stable id.
    ///
    /// The scope key asks for this once a frame so the board can drop the
    /// orders aimed at everything the teams do not hold.
    pub(super) fn members(&self) -> std::collections::BTreeSet<u64> {
        self.squads
            .iter()
            .flat_map(|squad| squad.members.iter().copied())
            .collect()
    }

    /// Rebuild the membership from the ledger, if the term has run or a new
    /// raid needs an answer.
    pub(super) fn regroup(&mut self, tick: u64, census: &Census) {
        self.regroup_focused(tick, census, false);
    }

    /// Keep one fist in a multi-house war. Optional harassment is valuable in
    /// a 1v1, but sending four units toward a second opponent while the main
    /// force attacks the first is the losing two-front fight reported in V23.
    pub(super) fn regroup_focused(
        &mut self, tick: u64, census: &Census, multiple_houses: bool,
    ) {
        let raid = home_threat(census);
        let new_raid = raid
            .map(|(target, _)| target)
            .filter(|target| Some(*target) != self.answered_raid);
        // A raid that nothing is handling cannot wait for the interval: the
        // units it needs are the ones standing idle at home, and by the time
        // the interval ran they would be standing in a wrecked yard.
        let interval = if new_raid.is_some() {
            0
        } else {
            REGROUP_FRAMES
        };
        if self
            .formed_at
            .is_some_and(|at| tick.saturating_sub(at) < interval)
        {
            return;
        }
        self.answered_raid = raid.map(|(target, _)| target);
        // Read before the clear: a team that is still locked keeps its raider,
        // otherwise every rebuild sends the defenders at a different hostile
        // and they meet none of them.
        let held = self
            .squads
            .iter()
            .find(|squad| squad.role == Role::Defend)
            .filter(|squad| squad.locked(tick))
            .and_then(|squad| squad.target);
        // Retain living members in their old jobs on a peaceful regroup.
        // Re-sorting from scratch by a moving unit's current distance made a
        // defender turn into a harasser and a harasser join the push every
        // 300 frames. An urgent raid still recruits the nearest troops first.
        let old_defend = self.squad(Role::Defend)
            .map(|squad| squad.members.clone()).unwrap_or_default();
        let old_harass = self.squad(Role::Harass)
            .map(|squad| squad.members.clone()).unwrap_or_default();
        self.formed_at = Some(tick);
        self.squads.clear();

        let mut taken: BTreeSet<u64> = BTreeSet::new();
        // Detached actors retain Attack membership for the operation but
        // cannot be recruited again for scouting, defence or raiding.
        let available = census.units.iter().filter(|unit| census.available(unit)
            && !census.detached.contains(&unit.entity_id)).count();

        // The scouts first: a dog that has been sent to look is not one of the
        // fist, and the fist is short of it either way.
        let scouts: BTreeSet<u64> = census
            .units
            .iter()
            .filter(|unit| unit.traits.scout && census.available(unit)
                && !census.elites.contains(&unit.entity_id)
                && !census.detached.contains(&unit.entity_id))
            .map(|unit| unit.entity_id)
            .collect();
        // A scout is not a strength the house designs to: every dog and every
        // scout the rules carry goes, so the team is whole the moment it exists.
        let scout_want = scouts.len();
        taken.extend(scouts.iter().copied());
        self.squads.push(Squad {
            role: Role::Scout,
            members: scouts,
            target: None,
            lock_until: tick.saturating_add(TARGET_LOCK_FRAMES),
            want: scout_want,
            state: SquadState::Ready,
        });

        // The defenders are always a team.
        //
        // They used to exist only while a raid was already on the base, and
        // between raids every unit that was not a scout or a raider went into the
        // fist. That is why an opening army of two soldiers and a harvester
        // marched at a construction yard instead of holding the yard it came
        // from, and why the yard was empty when the raid arrived. A house that
        // expands is a house that can afford to, and that is a house holding
        // something.
        //
        // `held` is the raider the old team was still locked on, read before the
        // clear, so a rebuild does not send the garrison at a different hostile
        // every twenty seconds and meet none of them.
        let raid = raid.map(|(id, _)| id).or(held);
        // Where the garrison forms: the raid when there is one, the yard when
        // there is not. Without this the defenders with no raid had nowhere to
        // stand and the pass below gave them the nearest hostile anywhere on the
        // map.
        let anchor = raid
            .and_then(|id| {
                census
                    .enemies
                    .iter()
                    .find(|enemy| enemy.entity_id == id)
                    .map(|enemy| enemy.cell)
            })
            .or(census.base_cell);
        let raiders = census
            .base_cell
            .map_or(0, |home| census.enemies_near(home, HOME_THREAT_CELLS));
        // Enough to meet the raid: the raid's own size, with DEFEND_MAX as the
        // floor and half the army as the ceiling. Twelve was measured against a
        // ring of fourteen and was short; a fixed number is the wrong shape for
        // this, because the raid is not fixed. Between raids the standing
        // garrison is a third of the army at most, so a fist still forms.
        let want = if raiders > 0 {
            raiders
                .max(DEFEND_MAX)
                .min((available / 2).max(1))
                .min(available)
        } else {
            standing_garrison(available.saturating_sub(scout_want))
        };
        let borrowed: BTreeSet<_> = old_defend.intersection(&census.borrowed_defenders)
            .copied().collect();
        if want > 0 || !borrowed.is_empty() {
            let mut nearest: Vec<&Unit> = census
                .units
                .iter()
                .filter(|unit| {
                    census.available(unit)
                        && !census.detached.contains(&unit.entity_id)
                        && !taken.contains(&unit.entity_id)
                })
                .collect();
            // Infantry first, then distance, then id. Not by name: the ledger
            // knows which job each type answers, and the body a house rebuilds
            // fastest is the one the rules price lowest for the job. The
            // expensive answer is what the fist keeps.
            nearest.sort_by_key(|unit| {
                let infantry = census
                    .facts(unit.type_id)
                    .is_some_and(|facts| facts.category == ObjectCategory::Infantry);
                let distance = anchor.map_or(0, |cell| squared_distance(unit.cell, cell));
                (raid.is_none() && distance > 14 * 14,
                 raid.is_none() && !old_defend.contains(&unit.entity_id),
                 !infantry, distance, unit.entity_id)
            });
            // Reserve one local AA body (two in a large garrison) inside the
            // existing defender quota. Do not recall the whole field army to
            // satisfy a paper reserve or steal another task's unit.
            let aa_want = if want >= 4 { 2 } else { usize::from(want >= 2) };
            let mut members: BTreeSet<u64> = nearest.iter().copied()
                .filter(|u| census.mobile_aa.contains(&u.entity_id)
                    && census.base_cell.is_some_and(|home|squared_distance(u.cell,home)<=24*24))
                .take(aa_want).map(|u|u.entity_id).collect();
            for unit in nearest {
                if members.len()>=want { break; }
                members.insert(unit.entity_id);
            }
            let ready = members.len();
            members.extend(borrowed.iter().copied());
            taken.extend(members.iter().copied());
            let state = if ready >= want {
                SquadState::Ready
            } else {
                SquadState::Muster
            };
            self.squads.push(Squad {
                role: Role::Defend,
                members,
                target: raid,
                lock_until: tick.saturating_add(TARGET_LOCK_FRAMES),
                want,
                state,
            });
        }

        // The raid is drawn from what the fist does not need, and the fist
        // needs the whole army while it is still below the push threshold:
        // nothing else is going to attack anyway. Once it is big enough to
        // push, the surplus goes at the enemy's ore — an army that only ever
        // stands at home while the enemy's economy pays for the defence it
        // is waiting to meet is the match this lost.
        //
        // Not the whole army either way — a raid is worth a handful, and the
        // house still needs something at home for the raid that answers it.
        // The denominator must match the actual actors available to advance.
        // Ships, heroes and task-owned units cannot make up a land fist.
        let main_total = census.units.iter().filter(|u| census.main_available(u)).count();
        let main_taken = census.units.iter().filter(|u| census.main_available(u)
            && taken.contains(&u.entity_id)).count();
        let harass_count = if multiple_houses || raiders > 0 { 0 }
            else { harass_allocation(main_total, main_taken) };
        if harass_count >= HARASS_MIN {
            let mut nearest: Vec<&Unit> = census
                .units
                .iter()
                .filter(|unit| {
                    census.main_available(unit)
                        && matches!(unit.slot, super::census::Slot::Vehicle | super::census::Slot::Infantry)
                        && !taken.contains(&unit.entity_id)
                })
                .collect();
            nearest.sort_by_key(|unit| {
                (
                    census.elites.contains(&unit.entity_id),
                    !old_harass.contains(&unit.entity_id),
                    // Mobile vehicles with good rule-defined speed suit a
                    // mining raid better than slow, costly siege assets.
                    unit.slot != super::census::Slot::Vehicle,
                    std::cmp::Reverse(census.facts(unit.type_id).map_or(0, |ty| ty.speed)),
                    squared_distance(unit.cell, enemy_anchor(census)),
                    unit.entity_id,
                )
            });
            let members: BTreeSet<u64> = nearest
                .into_iter()
                .take(harass_count)
                .map(|unit| unit.entity_id)
                .collect();
            taken.extend(members.iter().copied());
            let state = if members.len() >= harass_count {
                SquadState::Ready
            } else {
                SquadState::Muster
            };
            self.squads.push(Squad {
                role: Role::Harass,
                members,
                target: None,
                lock_until: tick.saturating_add(TARGET_LOCK_FRAMES),
                want: harass_count,
                state,
            });
        }

        // What is left is the fist. Its designed strength is
        // [`ARMY_TO_ATTACK`]: the same number the advance step gates on, so a
        // fist of six is `Muster` here and does not march there. That is the
        // difference between the two states being a label and being the thing
        // production reads.
        let attack: BTreeSet<u64> = census
            .units
            .iter()
            .filter(|unit| census.available(unit))
            .map(|unit| unit.entity_id)
            .filter(|id| !taken.contains(id))
            .collect();
        let state = if census.units.iter().filter(|u| attack.contains(&u.entity_id)
            && census.main_available(u)).count() >= ARMY_TO_ATTACK {
            SquadState::Ready
        } else {
            SquadState::Muster
        };
        self.squads.push(Squad {
            role: Role::Attack,
            members: attack,
            target: None,
            lock_until: tick,
            want: ARMY_TO_ATTACK,
            state,
        });
    }

    /// A previously committed attack member may have disappeared from the
    /// membership during recovery. Restore an unassigned actor to the same
    /// attack squad, but never override a different role's new assignment.
    /// The caller owns readiness, cohort provenance and forecast checks.
    pub(super) fn restore_attack_member(&mut self, actor: u64) -> bool {
        if self.squads.iter().any(|s| s.role != Role::Attack && s.members.contains(&actor)) {
            return false;
        }
        let Some(team) = self.squads.iter_mut().find(|s| s.role == Role::Attack) else {
            return false;
        };
        team.members.insert(actor);
        true
    }

    /// Every team, in role order: the micro layer walks all of them.
    pub(super) fn all(&self) -> &[Squad] {
        &self.squads
    }

    /// The team of `role`.
    pub(super) fn squad(&self, role: Role) -> Option<&Squad> {
        self.squads.iter().find(|squad| squad.role == role)
    }
}

/// Where the enemy's economy is, as far as the house knows: the cell the
/// harassers are measured from. The ledger's enemies are what the house can
/// see, and what it can see is what it can raid.
fn enemy_anchor(census: &Census) -> (u16, u16) {
    census
        .enemies
        .first()
        .map_or_else(|| census.base_cell.unwrap_or((0, 0)), |enemy| enemy.cell)
}

/// The hostile inside the base, and where it stands: a raid, and the thing the
/// defenders are for. Nearest to the base first, ties broken by id.
fn home_threat(census: &Census) -> Option<(u64, (u16, u16))> {
    // No base, no raid: a house whose yard is gone has nothing to defend, and
    // the ledger says so by having no base cell.
    let base = census.base_cell?;
    census
        .enemies
        .iter()
        .filter(|enemy| squared_distance(enemy.cell, base) <= HOME_THREAT_CELLS * HOME_THREAT_CELLS)
        .min_by_key(|enemy| (squared_distance(enemy.cell, base), enemy.entity_id))
        .map(|enemy| (enemy.entity_id, enemy.cell))
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn recovered_attack_member_may_return_but_never_steal_another_role() {
        let mut squads=Squads::default();
        for (role,members) in [(Role::Attack,BTreeSet::from([1u64])),
            (Role::Defend,BTreeSet::from([2u64]))] {
            squads.squads.push(Squad {role,members,target:None,lock_until:0,
                want:2,state:SquadState::Ready});
        }
        assert!(squads.restore_attack_member(3));
        assert!(squads.restore_attack_member(3));
        assert!(!squads.restore_attack_member(2));
        assert_eq!(squads.squad(Role::Attack).unwrap().members,BTreeSet::from([1,3]));
        assert_eq!(squads.squad(Role::Defend).unwrap().members,BTreeSet::from([2]));
    }

    #[test]
    fn standing_garrison_scales_without_consuming_small_opening() {
        assert_eq!(standing_garrison(0), 0);
        assert_eq!(standing_garrison(1), 1);
        assert_eq!(standing_garrison(5), 1);
        assert_eq!(standing_garrison(6), 2);
        assert_eq!(standing_garrison(12), 4);
        assert_eq!(standing_garrison(21), 7);
        assert_eq!(standing_garrison(50), 7);
    }

    /// One fighting unit of the ledger's, for the tests that only need to know
    /// how many there are and where they stand.
    fn unit(entity_id: u64, cell: (u16, u16)) -> Unit {
        Unit {
            entity_id,
            type_id: crate::sim::intern::InternedId::default(),
            cell,
            health_permille: 1_000,
            points: 100,
            mission: crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Guard,
            ),
            orderable: true,
            hand: false,
            slot: crate::sim::strategy::assist::census::Slot::Infantry,
            tier: 0,
            traits: crate::sim::strategy::assist::census::Traits {
                combat: true,
                armed: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn unavailable_units_do_not_fill_a_fist_or_disappear_from_owned_health() {
        let mut census = Census::default();
        census.base_cell = Some((20,20));
        for id in 0..18 { census.units.push(unit(id, (20,20))); }
        census.fighters = 18;
        census.unavailable.extend([0,1,2,3,4]);
        census.detached.insert(17);
        assert_eq!(census.available_fighters(), 13);
        assert_eq!(census.available_points(), 1_300);
        assert_eq!(census.army_points(), 1_800);
        assert!(!census.main_available(&census.units[17]));
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        assert!(squads.members().is_disjoint(&census.unavailable));
        assert!(squads.squad(Role::Attack).unwrap().members.contains(&17));
    }

    #[test]
    fn borrowed_defender_keeps_membership_without_counting_as_available() {
        let mut census = Census::default();
        census.base_cell = Some((20,20));
        for id in 0..12 { census.units.push(unit(id, (20,20))); }
        census.fighters = 12;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let guard = *squads.squad(Role::Defend).unwrap().members.iter().next().unwrap();
        census.borrowed_defenders.insert(guard);
        census.unavailable.insert(guard);
        squads.regroup(REGROUP_FRAMES, &census);
        assert!(squads.squad(Role::Defend).unwrap().members.contains(&guard));
        assert!(!squads.squad(Role::Attack).unwrap().members.contains(&guard));
        assert_eq!(census.available_fighters(), 11);
    }

    #[test]
    fn peaceful_regroup_preserves_existing_garrison_when_new_troops_arrive() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        for id in 0..9u64 {
            census.units.push(unit(id, (30 + id as u16, 30)));
        }
        census.fighters = 9;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let before = squads.squad(Role::Defend).expect("garrison").members.clone();
        assert_eq!(before.len(), 3);
        // A new recruit is nearer than the original defenders, but it should
        // reinforce the attack roster rather than retask three walking guards.
        census.units.push(unit(10, (21, 21)));
        census.fighters += 1;
        squads.regroup(REGROUP_FRAMES, &census);
        let after = &squads.squad(Role::Defend).expect("garrison").members;
        assert_eq!(&before, after);
    }

    /// A hostile in the base calls some of the army home — and only some,
    /// A raid bigger than the floor gets the defenders it needs, not the floor.
    /// The floor stays a floor: at or below it the squad is DEFEND_MAX, so the
    /// twelve that were measured are never taken away again.
    #[test]
    fn a_raid_bigger_than_the_floor_calls_more_than_the_floor() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        census
            .enemies
            .push(crate::sim::control::observation::EnemyEntity {
                entity_id: 999,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (21, 20),
            });
        // Nineteen more inside the ring, so the raid is twenty strong and the
        // floor of twelve is not what the house is facing.
        for id in 1_000..1_019_u64 {
            census
                .enemies
                .push(crate::sim::control::observation::EnemyEntity {
                    entity_id: id,
                    type_id: crate::sim::intern::InternedId::default(),
                    cell: (22, 20),
                });
        }
        let fighters = 30_u64;
        for id in 0..fighters {
            census.units.push(Unit {
                entity_id: id,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (20 + u16::try_from(id).unwrap_or(0), 30),
                health_permille: 1_000,
                points: 100,
                mission: crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Guard,
                ),
                orderable: true,
                hand: false,
                slot: crate::sim::strategy::assist::census::Slot::Infantry,
                tier: 0,
                traits: crate::sim::strategy::assist::census::Traits {
                    combat: true,
                    armed: true,
                    ..Default::default()
                },
            });
        }
        census.fighters = usize::try_from(fighters).unwrap_or(30);
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let defend = squads.squad(Role::Defend).expect("a raid is met");
        assert_eq!(
            defend.members.len(),
            15,
            "half the army meets a raid of twenty, and twelve would not"
        );
    }
    /// because pulling the whole army back is how the base is lost anyway.
    #[test]
    fn a_raid_on_the_base_calls_the_nearest_few_home() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        census
            .enemies
            .push(crate::sim::control::observation::EnemyEntity {
                entity_id: 999,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (21, 20),
            });
        // Four more than the defenders, so the test still says something about
        // a split: a fixture the size of DEFEND_MAX would make "the farthest is
        // not called" false by construction rather than by rule.
        let fighters = DEFEND_MAX + 4;
        for id in 0..u64::try_from(fighters).unwrap_or(12) {
            census.units.push(Unit {
                entity_id: id,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (20 + u16::try_from(id).unwrap_or(0), 30),
                health_permille: 1_000,
                points: 100,
                mission: crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Guard,
                ),
                orderable: true,
                hand: false,
                slot: crate::sim::strategy::assist::census::Slot::Infantry,
                tier: 0,
                traits: crate::sim::strategy::assist::census::Traits {
                    combat: true,
                    armed: true,
                    ..Default::default()
                },
            });
        }
        // The ledger's own count of the fighting army, which the teams read to
        // know whether the army is big enough to be a fist at all.
        census.fighters = usize::try_from(fighters).unwrap_or(12);
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let defend = squads.squad(Role::Defend).expect("a raid is met");
        assert_eq!(
            defend.members.len(),
            (fighters / 2).max(1),
            "half the army holds the base; the rest is free to march"
        );
        assert!(
            defend.members.contains(&0),
            "the nearest is called: {:?}",
            defend.members
        );
        let farthest = u64::try_from(fighters).unwrap_or(12) - 1;
        assert!(
            !defend.members.contains(&farthest),
            "the farthest is not: {:?}",
            defend.members
        );
        let attack = squads.squad(Role::Attack).expect("the fist");
        assert_eq!(
            attack.members.len(),
            fighters - (fighters / 2).max(1),
            "the rest stay in the fist"
        );
    }

    /// A hostile that stops the push calls the army home too. The push's own
    /// threat ring decides "the base is threatened"; with a smaller ring here
    /// the enemies between the two stopped the army from attacking and pulled
    /// nobody back, so the raid met an empty yard.
    #[test]
    fn a_raider_that_stops_the_push_is_answered_at_home() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        // Ten cells out: outside the old eight-cell ring, inside the push's.
        census
            .enemies
            .push(crate::sim::control::observation::EnemyEntity {
                entity_id: 999,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (20, 30),
            });
        census.units.push(unit(0, (20, 25)));
        census.fighters = 1;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let defend = squads
            .squad(Role::Defend)
            .expect("the yard answers the raid");
        assert!(defend.members.contains(&0));
        assert_eq!(defend.target, Some(999));
    }

    /// A raid that appears between two rebuilds is answered at once: the units
    /// it needs are the idle ones, and twenty seconds of them standing there is
    /// the yard burning.
    #[test]
    fn a_new_raider_is_answered_within_the_regroup_term() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        census.units.push(unit(0, (20, 25)));
        census.fighters = 1;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        assert!(squads.squad(Role::Defend).is_some_and(|squad|
            squad.members.contains(&0) && squad.target.is_none()),
            "one opening sentinel stays at the yard before a raid appears");

        census
            .enemies
            .push(crate::sim::control::observation::EnemyEntity {
                entity_id: 42,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (21, 21),
            });
        squads.regroup(1, &census);
        let defend = squads
            .squad(Role::Defend)
            .expect("answered on the next frame");
        assert_eq!(defend.target, Some(42));
    }

    /// And the same raider does not rebuild the army every frame after that.
    #[test]
    fn the_same_raider_waits_for_the_regroup_term() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        census
            .enemies
            .push(crate::sim::control::observation::EnemyEntity {
                entity_id: 42,
                type_id: crate::sim::intern::InternedId::default(),
                cell: (21, 21),
            });
        census.units.push(unit(0, (20, 25)));
        census.fighters = 1;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        assert!(squads.squad(Role::Defend).is_some());
        // A unit that joins late does not get a team until the term runs: the
        // teams are not rebuilt per frame.
        census.units.push(unit(1, (22, 22)));
        squads.regroup(1, &census);
        assert!(
            !squads
                .squad(Role::Defend)
                .is_some_and(|squad| squad.members.contains(&1)),
            "the interval still owns the outline"
        );
    }

    /// An army too small to take a base is not an army that should be standing
    /// in its own yard: the nearest few go at what pays for the enemy's army.
    #[test]
    fn a_small_army_keeps_a_real_fist_before_adding_raiders() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        for id in 0..7u64 { census.units.push(unit(id, (24, 24))); }
        census.fighters = 7;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        assert!(squads.squad(Role::Harass).is_none());
        assert_eq!(squads.squad(Role::Attack).unwrap().members.len(), 4);
        assert_eq!(squads.squad(Role::Defend).unwrap().members.len(), 3);
    }

    #[test]
    fn a_multi_house_game_keeps_one_main_fist_instead_of_raiding_second_front() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        for id in 0..25u64 {
            census.units.push(unit(id, (24, 24)));
        }
        census.fighters = 25;
        let mut single = Squads::new();
        single.regroup_focused(0, &census, false);
        assert!(single.squad(Role::Harass).is_some_and(|team| !team.members.is_empty()));

        let mut multi = Squads::new();
        multi.regroup_focused(0, &census, true);
        assert!(multi.squad(Role::Harass).is_none_or(|team| team.members.is_empty()));
        assert!(multi.squad(Role::Attack).is_some_and(|team| team.members.len() >= 10));
    }

    /// An army that is still growing is not an army that should leave the
    /// enemy's ore alone. The raid is a detachment, so it goes at every size:
    /// what the fist does while the gate holds it home is what decides whether
    /// the gate ever opens.
    #[test]
    fn a_massing_army_raids_as_well_as_masses() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        for id in 0..30u64 {
            census.units.push(unit(id, (24, 24)));
        }
        census.fighters = 30;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let harass = squads
            .squad(Role::Harass)
            .expect("an army that is still growing does not leave the ore alone");
        assert_eq!(harass.members.len(), HARASS_MAX, "a handful, not the rest");
        assert_eq!(
            squads
                .squad(Role::Attack)
                .map_or(0, |squad| squad.members.len()),
            30 - HARASS_MAX - PEACETIME_GARRISON.min(30 / 3),
            "and the fist keeps everything the garrison and the raid do not take"
        );
    }

    /// A retail arena: two houses on open ground, needed to score real types.
    /// The rules come from the repository's own `ini/` snapshot, so these run
    /// without a retail install.
    fn arena(seed: u64) -> Option<(crate::sim::world::Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena_known(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    fn ledger(
        rules: &RuleSet,
        sim: &crate::sim::world::Simulation,
        owner: crate::sim::intern::InternedId,
    ) -> Census {
        let mut census = Census::default();
        census.refresh(
            &crate::rules::unit_role::RoleTable::build(rules),
            rules,
            &Observation::new(sim, owner),
        );
        census
    }

    /// What stands at the target is weighed, not counted: two tanks are not an
    /// army that can walk into ten, and an army counted in heads was how the
    /// house kept trading itself for a base.
    #[test]
    fn an_army_that_cannot_outfight_the_garrison_is_not_sent() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for cell in [(10u16, 10u16), (11, 10)] {
            sim.spawn_object("MTNK", "Americans", cell.0, cell.1, 255, &rules)
                .expect("a Grizzly of our own");
        }
        for cell in [(26u16, 26u16), (27, 26), (28, 26), (26, 27), (27, 27)] {
            sim.spawn_object("MTNK", "Russians", cell.0, cell.1, 255, &rules)
                .expect("a Soviet Grizzly");
        }
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        assert!(
            census.army_points() > 0,
            "the ledger scores the army it has: {}",
            census.army_points()
        );
        assert!(
            !outguns(&census, &rules, &obs, (26, 26), census.army_points()),
            "two tanks do not outfight five: ours={} theirs={}",
            census.army_points(),
            census.enemy_points(&rules, &obs, (26, 26), PUSH_MEET_CELLS)
        );
    }

    /// And a target with nothing standing in front of it is one the army walks
    /// into, however small the army is: an empty base is not a garrison.
    #[test]
    fn an_undefended_target_is_one_the_army_walks_into() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("MTNK", "Americans", 10, 10, 255, &rules)
            .expect("a Grizzly of our own");
        let census = ledger(&rules, &sim, owner);
        let obs = Observation::new(&sim, owner);
        assert!(
            outguns(&census, &rules, &obs, (26, 26), census.army_points()),
            "nothing stands there: theirs={}",
            census.enemy_points(&rules, &obs, (26, 26), PUSH_MEET_CELLS)
        );
    }

    /// No hostile in the base, no defenders: the whole army is the fist.
    #[test]
    fn an_untouched_base_keeps_its_army_in_the_fist() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        assert!(squads.squad(Role::Defend).is_none());
        // And a rebuild inside the term does nothing, so a team is not
        // dissolved under a unit that is walking towards it.
        squads.regroup(REGROUP_FRAMES - 1, &census);
        assert_eq!(squads.squad(Role::Attack).map_or(0, |s| s.members.len()), 0);
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod v29_raid_allocation_tests {
    use super::harass_allocation;

    #[test]
    fn raids_require_surplus_after_a_full_main_fist() {
        assert_eq!(harass_allocation(11, 2), 0);
        assert_eq!(harass_allocation(13, 2), 0);
        assert_eq!(harass_allocation(14, 2), 2);
        assert_eq!(harass_allocation(30, 5), 4);
    }

    #[test]
    fn a_small_home_force_is_not_exhausted_by_a_mining_raid() {
        assert_eq!(harass_allocation(10, 6), 0);
        assert_eq!(harass_allocation(11, 5), 0);
        assert_eq!(harass_allocation(12, 5), 0);
    }
}
