//! A persistent strategic directive for the player's autonomous house.
//!
//! Tactical passes decide *how* to fight; this layer decides *what matters
//! most* while they do it. It reads only the shared census, the measured
//! economy and last-seen ore, never the simulation's hidden enemy state.
//!
//! The directive is deliberately small. It does not replace the battle-tested
//! target chooser or issue native commands: it coordinates the expensive
//! decisions (saving for an MCV, replacing a lost income chain, buying
//! specialist units, spending on nonessential structures). Urgent failures
//! pre-empt a long-term plan; ordinary changes are held across frames.

use std::collections::BTreeMap;

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::census::Census;
use super::plan::Item;
use super::squads;

/// The nearest known resource field needs enough tiles to justify paying for
/// another construction yard; one isolated ore tile is not a second economy.
const FAR_FIELD_STOCK: u32 = 16;
const FAR_FROM_YARD_CELLS: i32 = 24;
const FAR_FROM_REFINERY_CELLS: i32 = 16;
const BASE_RAID_CELLS: i32 = 18;

/// A strategic choice normally lasts twenty seconds. This is not a cooldown
/// on native commands; it stops fluctuating income or one squad casualty from
/// flipping the house's expensive priorities every frame.
const MIN_DIRECTIVE_FRAMES: u64 = 300;
const ECONOMY_STABLE_FRAMES: u64 = 120;
const BASE_QUIET_FRAMES: u64 = 150;
const FORCE_SAMPLE_FRAMES: u64 = 300;
const AFTER_LOSS_MUSTER_FRAMES: u64 = 450;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Objective {
    /// No working construction yard: recover by deploying or buying an MCV.
    RecoverYard,
    /// An income bottleneck: protect the first refinery and harvester before
    /// another lab, tower, luxury building or second base.
    RecoverIncome,
    /// A *currently visible* enemy is close to the base: keep the garrison
    /// and the production lines focused on immediate survival.
    HoldBase,
    /// The army took a large loss or is not yet ready for a serious push.
    #[default]
    Muster,
    /// There is a confirmed, sufficiently rich field beyond our bays and
    /// enough infrastructure to support a new yard.
    ExpandOre,
    /// The attack squad is big enough to seek a decisive fight. Tactical
    /// `advance` still performs its original reachability and outguns checks.
    PressAttack,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Signals {
    pub(super) yards: usize,
    pub(super) yard_makers: usize,
    pub(super) refineries: usize,
    pub(super) harvesters: usize,
    pub(super) raiders_visible: usize,
    pub(super) attack_members: usize,
    pub(super) fighters: usize,
    pub(super) fighting_points: i32,
    pub(super) credits: i32,
    pub(super) income_per_minute: Option<i32>,
    pub(super) far_ore_stock: u32,
}

impl Signals {
    fn economy_ok(self) -> bool {
        self.refineries > 0 && self.harvesters > 0
    }

    fn can_expand(self) -> bool {
        // A second base is not an opening purchase. There must be a real,
        // last-seen field, an unloading bay and enough working miners and
        // fighters that paying for an MCV will not starve the first base.
        self.yards > 0
            && self.refineries > 0
            && self.harvesters >= 2
            && self.fighters >= 6
            && self.far_ore_stock >= FAR_FIELD_STOCK
            // A depleted home field can have zero measured income even when
            // a known distant field would save the economy. If the bank can
            // fund an entire second-yard operation, do not trap the house
            // waiting for income that cannot arrive at its old base.
            && (self.income_per_minute.is_some_and(|rate| rate > 0)
                || self.credits >= 7_000)
    }
}

/// A rich, already-explored central mineral field is an independent strategic
/// opportunity. Even a ready attack force can invest in it if the treasury can
/// fund the new yard *without* sacrificing the next tank wave. Smaller fields
/// retain v9's proven "attack first" behaviour.
fn should_expand_while_fighting(signals: Signals) -> bool {
    signals.can_expand()
        && (signals.attack_members < squads::ARMY_TO_ATTACK
            || (signals.far_ore_stock >= FAR_FIELD_STOCK * 2 && signals.credits >= 7_000))
}

/// The known, unserved ore, not the omniscient native overlay grid. This is
/// deliberately evaluated against each refinery and yard, not just the first
/// base anchor: ore beside a captured/new refinery does not justify another.
fn distant_ore_stock(
    known: &BTreeMap<(u16, u16), u8>,
    yard_cells: &[(u16, u16)],
    refinery_cells: &[(u16, u16)],
) -> u32 {
    known
        .iter()
        .filter(|(cell, _)| {
            yard_cells.iter().all(|anchor| {
                super::squared_distance(**cell, *anchor)
                    > FAR_FROM_YARD_CELLS * FAR_FROM_YARD_CELLS
            }) && refinery_cells.iter().all(|anchor| {
                super::squared_distance(**cell, *anchor)
                    > FAR_FROM_REFINERY_CELLS * FAR_FROM_REFINERY_CELLS
            })
        })
        .fold(0_u32, |total, (_, stage)| total.saturating_add(u32::from(*stage) + 1))
}

impl Signals {
    pub(super) fn read(
        rules: &RuleSet,
        obs: &Observation<'_>,
        census: &Census,
        attack_members: usize,
        known_ore: &BTreeMap<(u16, u16), u8>,
        income_per_minute: Option<i32>,
    ) -> Self {
        let raiders_visible = census.base_cell.map_or(0, |home| {
            census.enemies.iter()
                // History may suggest where to search, but cannot trigger
                // the same emergency as a unit currently attacking the base.
                .filter(|enemy| obs.is_cell_visible(enemy.cell.0, enemy.cell.1))
                .filter(|enemy| super::squared_distance(enemy.cell, home)
                    <= BASE_RAID_CELLS * BASE_RAID_CELLS)
                .filter(|enemy| obs.object_type(rules, enemy.type_id)
                    .is_some_and(|ty| ty.category != ObjectCategory::Building
                        || ty.weapon_list.iter().any(Option::is_some)))
                .count()
        });
        Self {
            yards: census.yards,
            yard_makers: census.yard_makers,
            refineries: census.refineries,
            harvesters: census.harvesters,
            raiders_visible,
            attack_members,
            fighters: census.fighters,
            fighting_points: census.army_points(),
            credits: obs.credits(),
            income_per_minute,
            far_ore_stock: distant_ore_stock(known_ore,
                &census.yard_cells, &census.refinery_cells),
        }
    }
}

/// Cross-frame commitment and its evidence. All changes are driven by
/// integer observations; this adds no RNG draws or new command types.
#[derive(Debug, Default)]
pub(super) struct Director {
    objective: Objective,
    started_at: u64,
    initialized: bool,
    last_raid_at: Option<u64>,
    raided_now: bool,
    income_stable_since: Option<u64>,
    force_sample: Option<(u64, i32)>,
    replenish_until: u64,
}

impl Director {
    pub(super) fn objective(&self) -> Objective {
        self.objective
    }

    pub(super) fn initialized(&self) -> bool {
        self.initialized
    }

    /// With no `observe` call, preserve the semantics of focused tests that
    /// invoke one production function directly. In a real frame `think`
    /// initializes the director before any module makes a purchase.
    pub(super) fn may_expand(&self) -> bool {
        !self.initialized || self.objective == Objective::ExpandOre
    }

    pub(super) fn recovering_income(&self) -> bool {
        self.initialized && self.objective == Objective::RecoverIncome
    }

    pub(super) fn protecting_base(&self) -> bool {
        self.initialized && self.objective == Objective::HoldBase
    }

    pub(super) fn under_attack_now(&self) -> bool {
        self.raided_now
    }

    pub(super) fn permits_build(&self, item: Item) -> bool {
        if !self.initialized {
            return true;
        }
        match self.objective {
            Objective::RecoverYard | Objective::RecoverIncome => matches!(item,
                Item::Power | Item::Refinery | Item::Barracks | Item::Weapons | Item::Support),
            Objective::HoldBase => !matches!(item,
                Item::Superweapon | Item::SupportSuperweapon | Item::SpySatellite | Item::CloningVats | Item::RobotControl
                | Item::IndustrialPlant | Item::Purifier),
            // The build plan's StrategicStrike gate, not the name of the
            // stage, determines when an affordable strike can be mounted.
            Objective::ExpandOre => !matches!(item,
                Item::SpySatellite | Item::CloningVats | Item::RobotControl),
            Objective::Muster => true,
            Objective::PressAttack => true,
        }
    }

    /// Returns a change only when the commitment actually changes. In
    /// particular, one cleared threat cell does not immediately resume buying
    /// expansion, and a newly lost last harvester interrupts any luxury plan.
    pub(super) fn observe(&mut self, now: u64, signals: Signals) -> Option<(Objective, Objective)> {
        self.raided_now = signals.raiders_visible > 0;
        if signals.raiders_visible > 0 {
            self.last_raid_at = Some(now);
        }
        if signals.economy_ok() {
            self.income_stable_since.get_or_insert(now);
        } else {
            self.income_stable_since = None;
        }
        if let Some((at, previous)) = self.force_sample {
            if now.saturating_sub(at) >= FORCE_SAMPLE_FRAMES {
                if previous >= 1_500
                    && signals.fighting_points.saturating_mul(100)
                        < previous.saturating_mul(65)
                    && previous.saturating_sub(signals.fighting_points) >= 700
                {
                    self.replenish_until = now.saturating_add(AFTER_LOSS_MUSTER_FRAMES);
                }
                self.force_sample = Some((now, signals.fighting_points));
            }
        } else {
            self.force_sample = Some((now, signals.fighting_points));
        }

        let threatened = signals.raiders_visible > 0 || self.last_raid_at
            .is_some_and(|at| now.saturating_sub(at) < BASE_QUIET_FRAMES);
        let economy_recovered = self.income_stable_since
            .is_some_and(|at| now.saturating_sub(at) >= ECONOMY_STABLE_FRAMES);
        let desired = if signals.yards == 0 {
            Objective::RecoverYard
        } else if !signals.economy_ok() || !economy_recovered {
            Objective::RecoverIncome
        } else if threatened {
            Objective::HoldBase
        } else if now < self.replenish_until {
            Objective::Muster
        } else if should_expand_while_fighting(signals) {
            Objective::ExpandOre
        } else if signals.attack_members >= squads::ARMY_TO_ATTACK {
            Objective::PressAttack
        } else {
            Objective::Muster
        };

        let urgent = matches!(desired,
            Objective::RecoverYard | Objective::RecoverIncome | Objective::HoldBase)
            && (desired == Objective::RecoverYard
                || (desired == Objective::RecoverIncome && !signals.economy_ok())
                || signals.raiders_visible > 0);
        if !self.initialized || desired != self.objective && (urgent
            || now.saturating_sub(self.started_at) >= MIN_DIRECTIVE_FRAMES)
        {
            let before = self.objective;
            self.objective = desired;
            self.started_at = now;
            self.initialized = true;
            if before != desired {
                return Some((before, desired));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Signals {
        Signals {
            yards: 1, refineries: 1, harvesters: 2, fighters: 7,
            attack_members: 5, income_per_minute: Some(2_000),
            far_ore_stock: 0, fighting_points: 2_000, credits: 4_000,
            ..Signals::default()
        }
    }

    fn ready(d: &mut Director, mut s: Signals) {
        d.observe(0, s);
        s.fighting_points = 2_000;
        d.observe(400, s);
    }

    #[test]
    fn lost_income_interrupts_an_expansion_and_does_not_return_immediately() {
        let mut director = Director::default();
        let mut s = base();
        s.far_ore_stock = 40;
        ready(&mut director, s);
        assert_eq!(director.objective(), Objective::ExpandOre);
        assert!(director.may_expand());

        s.harvesters = 0;
        director.observe(401, s);
        assert_eq!(director.objective(), Objective::RecoverIncome);
        assert!(!director.may_expand());
        assert!(!director.permits_build(Item::Defense));
        s.harvesters = 1;
        director.observe(402, s);
        assert_eq!(director.objective(), Objective::RecoverIncome);
        director.observe(600, s);
        assert_eq!(director.objective(), Objective::RecoverIncome);
        director.observe(702, s);
        assert_eq!(director.objective(), Objective::Muster);
    }

    #[test]
    fn raid_preempts_expansion_and_holds_until_quiet() {
        let mut director = Director::default();
        let mut s = base();
        s.far_ore_stock = 40;
        ready(&mut director, s);
        s.raiders_visible = 2;
        director.observe(401, s);
        assert_eq!(director.objective(), Objective::HoldBase);
        assert!(!director.may_expand());
        s.raiders_visible = 0;
        director.observe(402, s);
        assert_eq!(director.objective(), Objective::HoldBase);
        director.observe(800, s);
        assert_eq!(director.objective(), Objective::ExpandOre);
    }

    #[test]
    fn income_recovery_under_fire_does_not_disallow_emergency_troops() {
        let mut director = Director::default();
        let mut s = base();
        s.refineries = 0;
        s.raiders_visible = 2;
        director.observe(0, s);
        assert_eq!(director.objective(), Objective::RecoverIncome);
        assert!(director.under_attack_now());
        assert!(director.permits_build(Item::Power));
        assert!(!director.permits_build(Item::Defense));
        s.raiders_visible = 0;
        director.observe(1, s);
        assert!(!director.under_attack_now());
    }

    #[test]
    fn an_attack_ready_army_avoids_buying_a_second_base() {
        let mut director = Director::default();
        let mut s = base();
        s.attack_members = squads::ARMY_TO_ATTACK;
        s.far_ore_stock = 80;
        ready(&mut director, s);
        assert_eq!(director.objective(), Objective::PressAttack);
        assert!(!director.may_expand());
    }

    #[test]
    fn a_single_far_ore_cell_does_not_purchase_a_second_base() {
        let mut director = Director::default();
        let mut s = base();
        s.far_ore_stock = 1;
        ready(&mut director, s);
        assert_eq!(director.objective(), Objective::Muster);
        assert!(!director.may_expand());
    }

    #[test]
    fn a_rich_but_depleted_base_can_fund_an_ore_expansion() {
        let mut director = Director::default();
        let mut s = base();
        s.income_per_minute = Some(0);
        s.credits = 9_000;
        s.far_ore_stock = 25;
        ready(&mut director, s);
        assert_eq!(director.objective(), Objective::ExpandOre);
        assert!(director.may_expand());
        s.credits = 1_000;
        // A fresh new director must not enter the same expansion with no
        // income and no savings; the current one keeps its minimum term.
        let mut fresh = Director::default();
        ready(&mut fresh, s);
        assert_eq!(fresh.objective(), Objective::Muster);
    }

    #[test]
    fn current_base_loss_preempts_every_other_objective() {
        let mut director = Director::default();
        let mut s = base();
        ready(&mut director, s);
        s.yards = 0;
        director.observe(401, s);
        assert_eq!(director.objective(), Objective::RecoverYard);
        assert!(!director.permits_build(Item::Superweapon));
    }

    #[test]
    fn a_large_force_loss_temporarily_stops_expansion() {
        let mut director = Director::default();
        let mut s = base();
        s.far_ore_stock = 40;
        director.observe(0, s);
        s.fighting_points = 900;
        director.observe(400, s);
        assert_eq!(director.objective(), Objective::Muster);
        assert!(!director.may_expand());
        director.observe(800, s);
        assert_eq!(director.objective(), Objective::Muster);
        director.observe(1_400, s);
        assert_eq!(director.objective(), Objective::ExpandOre);
    }

    #[test]
    fn distant_resources_are_unknown_until_revealed() {
        let mut ore = BTreeMap::new();
        ore.insert((80, 80), 3);
        ore.insert((81, 80), 3);
        ore.insert((82, 80), 3);
        ore.insert((83, 80), 3);
        ore.insert((84, 80), 3);
        assert_eq!(distant_ore_stock(&ore, &[(10, 10)], &[(12, 10)]), 20);
        assert_eq!(distant_ore_stock(&ore, &[(80, 80)], &[(12, 10)]), 0);
        assert_eq!(distant_ore_stock(&ore, &[(10, 10)], &[(79, 80)]), 0);
        assert_eq!(distant_ore_stock(&BTreeMap::new(), &[(10, 10)], &[]), 0);
    }
}

#[cfg(test)]
mod rich_frontier_tests {
    use super::*;
    #[test]
    fn a_rich_middle_field_can_be_funded_with_a_ready_attack_army() {
        let signals = Signals {
            yards: 1, refineries: 2, harvesters: 4,
            fighters: 20, attack_members: squads::ARMY_TO_ATTACK + 2,
            credits: 9000, income_per_minute: Some(1000),
            far_ore_stock: FAR_FIELD_STOCK * 3,
            ..Signals::default()
        };
        assert!(should_expand_while_fighting(signals));
        assert!(!should_expand_while_fighting(Signals { credits: 4000, ..signals }),
            "expansion must not steal the current tank wave's money");
    }
}
