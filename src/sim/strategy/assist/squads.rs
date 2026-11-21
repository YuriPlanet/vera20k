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
const DEFEND_MAX: usize = 8;

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
) -> bool {
    let theirs = census.enemy_points(rules, obs, cell, PUSH_MEET_CELLS);
    if theirs == 0 {
        return true;
    }
    let ours = census.army_points();
    ours.saturating_mul(1_000) >= theirs.saturating_mul(PUSH_SUPERIORITY_PERMILLE)
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
    let ours = census.army_points();
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

/// One team: who is in it, what it is aimed at, and until when.
#[derive(Debug, Clone)]
pub(super) struct Squad {
    pub(super) role: Role,
    pub(super) members: BTreeSet<u64>,
    /// What the team is aimed at, when it is aimed at one object.
    pub(super) target: Option<u64>,
    /// The frame the team may re-pick its target on.
    pub(super) lock_until: u64,
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

    /// Rebuild the membership from the ledger, if the term has run or a new
    /// raid needs an answer.
    pub(super) fn regroup(&mut self, tick: u64, census: &Census) {
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
        self.formed_at = Some(tick);
        self.squads.clear();

        let mut taken: BTreeSet<u64> = BTreeSet::new();

        // The scouts first: a dog that has been sent to look is not one of the
        // fist, and the fist is short of it either way.
        let scouts: BTreeSet<u64> = census
            .units
            .iter()
            .filter(|unit| unit.traits.scout && unit.orderable && !unit.hand)
            .map(|unit| unit.entity_id)
            .collect();
        taken.extend(scouts.iter().copied());
        self.squads.push(Squad {
            role: Role::Scout,
            members: scouts,
            target: None,
            lock_until: tick.saturating_add(TARGET_LOCK_FRAMES),
        });

        // Then the defenders, if the base is being raided: the nearest few,
        // because the nearest are the ones that can arrive. `held` is the
        // raider the old team was still locked on, read before the clear.
        let raid = raid.map(|(id, _)| id).or(held);
        let raid_cell = raid.and_then(|id| {
            census
                .enemies
                .iter()
                .find(|enemy| enemy.entity_id == id)
                .map(|enemy| enemy.cell)
        });
        if let (Some(threat), Some(cell)) = (raid, raid_cell) {
            let mut nearest: Vec<&Unit> = census
                .units
                .iter()
                .filter(|unit| {
                    unit.traits.combat
                        && unit.orderable
                        && !unit.hand
                        && !taken.contains(&unit.entity_id)
                })
                .collect();
            nearest.sort_by_key(|unit| (squared_distance(unit.cell, cell), unit.entity_id));
            let members: BTreeSet<u64> = nearest
                .into_iter()
                .take(DEFEND_MAX)
                .map(|unit| unit.entity_id)
                .collect();
            taken.extend(members.iter().copied());
            self.squads.push(Squad {
                role: Role::Defend,
                members,
                target: Some(threat),
                lock_until: tick.saturating_add(TARGET_LOCK_FRAMES),
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
        let fist_needs = usize::from(census.fighters >= ARMY_TO_ATTACK) * ARMY_TO_ATTACK;
        if census.fighters >= taken.len() + fist_needs + HARASS_MAX {
            let mut nearest: Vec<&Unit> = census
                .units
                .iter()
                .filter(|unit| {
                    unit.traits.combat
                        && unit.orderable
                        && !unit.hand
                        && !taken.contains(&unit.entity_id)
                })
                .collect();
            nearest.sort_by_key(|unit| {
                (
                    squared_distance(unit.cell, enemy_anchor(census)),
                    unit.entity_id,
                )
            });
            let members: BTreeSet<u64> = nearest
                .into_iter()
                .take(HARASS_MAX)
                .map(|unit| unit.entity_id)
                .collect();
            taken.extend(members.iter().copied());
            self.squads.push(Squad {
                role: Role::Harass,
                members,
                target: None,
                lock_until: tick.saturating_add(TARGET_LOCK_FRAMES),
            });
        }

        // What is left is the fist.
        let attack: BTreeSet<u64> = census
            .units
            .iter()
            .filter(|unit| unit.traits.combat && unit.orderable && !unit.hand)
            .map(|unit| unit.entity_id)
            .filter(|id| !taken.contains(id))
            .collect();
        self.squads.push(Squad {
            role: Role::Attack,
            members: attack,
            target: None,
            lock_until: tick,
        });
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

    /// A hostile in the base calls some of the army home — and only some,
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
        for id in 0..12u64 {
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
        census.fighters = 12;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let defend = squads.squad(Role::Defend).expect("a raid is met");
        assert_eq!(
            defend.members.len(),
            DEFEND_MAX,
            "some of the army, not all"
        );
        assert!(
            defend.members.contains(&0),
            "the nearest is called: {:?}",
            defend.members
        );
        assert!(
            !defend.members.contains(&11),
            "the farthest is not: {:?}",
            defend.members
        );
        let attack = squads.squad(Role::Attack).expect("the fist");
        assert_eq!(
            attack.members.len(),
            12 - DEFEND_MAX,
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
        assert!(
            squads.squad(Role::Defend).is_none(),
            "nothing to answer yet"
        );

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
    fn an_army_too_small_to_take_a_base_goes_harassing() {
        let mut census = Census::default();
        census.base_cell = Some((20, 20));
        for id in 0..5u64 {
            census.units.push(unit(id, (24, 24)));
        }
        census.fighters = 5;
        let mut squads = Squads::new();
        squads.regroup(0, &census);
        let harass = squads
            .squad(Role::Harass)
            .expect("a small army does not stand in its yard");
        assert_eq!(harass.members.len(), HARASS_MAX, "a handful, not the rest");
        assert_eq!(
            squads
                .squad(Role::Attack)
                .map_or(0, |squad| squad.members.len()),
            5 - HARASS_MAX,
            "and the house keeps something at home"
        );
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
            30 - HARASS_MAX,
            "and the fist keeps everything the raid does not take"
        );
    }

    /// A retail arena: two houses on open ground, needed to score real types.
    /// The rules come from the repository's own `ini/` snapshot, so these run
    /// without a retail install.
    fn arena(seed: u64) -> Option<(crate::sim::world::Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
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
            !outguns(&census, &rules, &obs, (26, 26)),
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
            outguns(&census, &rules, &obs, (26, 26)),
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
