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

use std::collections::{BTreeMap, VecDeque};

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
const FORCE_SAMPLE_FRAMES: u64 = 120;
const FORCE_HISTORY_FRAMES: u64 = 3_600;
const MIN_RECOVERY_FRAMES: u64 = 450;
const MAX_RECOVERY_FRAMES: u64 = 2_700;

/// A large army must replace a meaningful fraction of its lost force.
/// Use a bounded time, not a fixed tiny force that every large army already has.
fn recovery_target(peak: i32) -> i32 {
    (peak.max(0) / 4 * 3).max(1_200)
}

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
    /// A recently depleted fighting force is pulling back and replacing losses.
    RebuildArmy,
    /// The army is not yet ready for a serious push.
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
    pub(super) campaign: bool,
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
    pub(super) fn economy_ok_for_preparation(self)->bool {
        self.yards>0 && self.refineries>0 && self.harvesters>=2
            && self.raiders_visible==0 && (self.credits>=6500
                || self.income_per_minute.is_some_and(|rate|rate>=1200))
    }

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
            || (signals.far_ore_stock >= FAR_FIELD_STOCK * 2 && signals.fighters>=8
                && (signals.credits>=5_500 || signals.income_per_minute.is_some_and(|rate|rate>=1_800))))
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
        let raiders_visible = obs.visible_enemy_entities().filter(|enemy| {
            let near_home = census.base_cell.is_some_and(|home|
                super::squared_distance(enemy.cell,home) <= BASE_RAID_CELLS*BASE_RAID_CELLS);
            if super::convergence::mobile_threat(rules,obs,enemy) {
                near_home || super::convergence::near_income(census,enemy.cell)
            } else {
                near_home && obs.object_type(rules,enemy.type_id).is_some_and(|ty|
                    ty.category == ObjectCategory::Building && ty.weapon_list.iter().any(Option::is_some)
                    && obs.visible_enemy_building_operational(rules,enemy.entity_id))
            }
        }).count();
        Self {
            campaign: obs.is_campaign(),
            yards: census.yards,
            yard_makers: census.yard_makers,
            refineries: census.refineries,
            harvesters: census.harvesters,
            raiders_visible,
            attack_members,
            fighters: census.available_fighters(),
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
    force_samples: VecDeque<(u64, i32)>,
    loss_recovery_since: Option<u64>,
    had_income_chain: bool,
    had_yard: bool,
    field_operation: bool,
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

    /// A campaign can deliberately begin with a barracks or field army
    /// instead of a construction yard. Do not invent a lost-base emergency.
    pub(super) fn field_operation(&self) -> bool { self.field_operation }

    pub(super) fn rebuilding_force(&self) -> bool {
        self.loss_recovery_since.is_some()
    }

    /// A loss elsewhere is not authority to recall a healthy local force.
    /// Keep the loss detector for diagnosis; only a current home raid makes
    /// its old global recovery restrictions relevant.
    pub(super) fn recovering_under_fire(&self) -> bool {
        self.rebuilding_force() && self.under_attack_now()
    }

    pub(super) fn emergency_spending(&self, obs: &Observation<'_>) -> bool {
        (self.initialized && (self.recovering_under_fire()
            || self.objective == Objective::RecoverYard
            || (self.had_income_chain && self.recovering_income())))
            || (obs.power_balance() < 0 && !obs.power_blackout_active())
    }

    pub(super) fn under_attack_now(&self) -> bool {
        self.raided_now
    }

    pub(super) fn permits_build(&self, item: Item) -> bool {
        if !self.initialized {
            return true;
        }
        if self.recovering_under_fire() {
            return matches!(item, Item::Power | Item::Refinery | Item::Barracks
                | Item::Weapons | Item::Support | Item::Repair);
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
            Objective::RebuildArmy | Objective::Muster | Objective::PressAttack => true,
        }
    }

    fn observe_force(&mut self, now: u64, points: i32) {
        if let Some(since) = self.loss_recovery_since {
            let elapsed = now.saturating_sub(since);
            let peak = self.force_samples.iter().map(|(_, value)| *value).max().unwrap_or(0);
            let target = recovery_target(peak);
            if elapsed >= MAX_RECOVERY_FRAMES
                || (elapsed >= MIN_RECOVERY_FRAMES && points >= target)
            {
                let reason = if points >= target && elapsed >= MIN_RECOVERY_FRAMES { "threshold" } else { "timeout" };
                log::info!("auto play: force_recovery_exit tick={now} elapsed={elapsed} recent_peak={peak} fighting_points={points} target={target} reason={reason}");
                self.loss_recovery_since = None;
                self.force_samples.clear();
            } else {
                return;
            }
        }
        while self.force_samples.front()
            .is_some_and(|(at, _)| now.saturating_sub(*at) > FORCE_HISTORY_FRAMES)
        {
            self.force_samples.pop_front();
        }
        if self.force_samples.back()
            .is_some_and(|(at, _)| now.saturating_sub(*at) < FORCE_SAMPLE_FRAMES)
        {
            return;
        }
        let peak = self.force_samples.iter().map(|(_, points)| *points).max().unwrap_or(0);
        if peak >= 1_500 && points.saturating_mul(100) < peak.saturating_mul(65)
            && peak.saturating_sub(points) >= 700
        {
            self.loss_recovery_since = Some(now);
            log::info!("auto play: force_recovery tick={now} recent_peak={peak} fighting_points={points}");
        }
        self.force_samples.push_back((now, points));
    }

    /// Returns a change only when the commitment actually changes. In
    /// particular, one cleared threat cell does not immediately resume buying
    /// expansion, and a newly lost last harvester interrupts any luxury plan.
    pub(super) fn observe(&mut self, now: u64, signals: Signals) -> Option<(Objective, Objective)> {
        self.had_yard |= signals.yards>0;
        self.field_operation=signals.campaign && !self.had_yard && signals.yards==0;
        self.raided_now = signals.raiders_visible > 0;
        if signals.raiders_visible > 0 {
            self.last_raid_at = Some(now);
        }
        if signals.economy_ok() {
            self.had_income_chain = true;
            self.income_stable_since.get_or_insert(now);
        } else {
            self.income_stable_since = None;
        }
        self.observe_force(now, signals.fighting_points);

        let threatened = signals.raiders_visible > 0 || self.last_raid_at
            .is_some_and(|at| now.saturating_sub(at) < BASE_QUIET_FRAMES);
        let economy_recovered = self.income_stable_since
            .is_some_and(|at| now.saturating_sub(at) >= ECONOMY_STABLE_FRAMES);
        let desired = if signals.yards == 0 && !self.field_operation {
            Objective::RecoverYard
        } else if (!self.field_operation || self.had_income_chain || signals.refineries>0)
            && (!signals.economy_ok() || !economy_recovered) {
            Objective::RecoverIncome
        } else if threatened {
            Objective::HoldBase
        } else if self.rebuilding_force() {
            Objective::RebuildArmy
        } else if should_expand_while_fighting(signals) {
            Objective::ExpandOre
        } else if signals.attack_members >= squads::ARMY_TO_ATTACK {
            Objective::PressAttack
        } else {
            Objective::Muster
        };

        let urgent = desired == Objective::RebuildArmy || matches!(desired,
            Objective::RecoverYard | Objective::RecoverIncome | Objective::HoldBase)
            && (desired == Objective::RecoverYard
                || (desired == Objective::RecoverIncome && !signals.economy_ok())
                || signals.raiders_visible > 0);
        if !self.initialized || desired != self.objective && (urgent
            || self.objective == Objective::RebuildArmy
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

#[cfg(all(test, tests_sim_p_s))]
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
    fn a_yardless_campaign_is_not_a_lost_yard_or_income_emergency() {
        let mut director=Director::default();
        let s=Signals {campaign:true, fighters:3, credits:1500, ..Default::default()};
        director.observe(0,s);
        assert!(director.field_operation());
        assert_eq!(director.objective(),Objective::Muster);
        assert!(!director.recovering_income());
        director.observe(400,Signals{yards:1,..s});
        assert!(!director.field_operation());
        director.observe(401,s);
        assert_eq!(director.objective(),Objective::RecoverYard);
    }

    #[test]
    fn a_yardless_skirmish_keeps_native_recovery_policy() {
        let mut director=Director::default();
        director.observe(0,Signals{fighters:3,credits:1500,..Default::default()});
        assert!(!director.field_operation());
        assert_eq!(director.objective(),Objective::RecoverYard);
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
    fn losses_across_multiple_samples_interrupt_expansion_and_recover() {
        let mut director = Director::default();
        let mut s = base();
        s.far_ore_stock = 40;
        ready(&mut director, s);
        s.fighting_points = 1_700;
        director.observe(520, s);
        s.fighting_points = 1_400;
        director.observe(640, s);
        assert_eq!(director.objective(), Objective::ExpandOre);
        s.fighting_points = 1_190;
        director.observe(760, s);
        assert_eq!(director.objective(), Objective::RebuildArmy);
        assert!(director.rebuilding_force());
        assert!(!director.may_expand());
        assert!(director.permits_build(Item::Weapons));
        assert!(!director.recovering_under_fire());
        assert!(director.permits_build(Item::Tech));
        s.fighting_points = 1_300;
        director.observe(1_209, s);
        assert!(director.rebuilding_force());
        director.observe(1_210, s);
        assert!(director.rebuilding_force(), "65% of the prior peak is not a recovered force");
        s.fighting_points = 1_500;
        director.observe(1_211, s);
        assert_eq!(director.objective(), Objective::ExpandOre);
    }

    #[test]
    fn loss_recovery_only_holds_offense_during_a_current_raid() {
        let mut director = Director::default();
        let mut s = base();
        ready(&mut director, s);
        s.fighting_points = 100;
        director.observe(520, s);
        assert!(director.rebuilding_force());
        assert!(!director.recovering_under_fire());
        assert!(director.permits_build(Item::Tech));
        s.raiders_visible = 2;
        director.observe(521, s);
        assert!(director.recovering_under_fire());
        assert!(!director.permits_build(Item::Tech));
        s.raiders_visible = 0;
        director.observe(522, s);
        assert!(director.rebuilding_force());
        assert!(!director.recovering_under_fire());
    }

    #[test]
    fn recovery_threshold_scales_with_the_previous_force() {
        assert_eq!(recovery_target(2_000),1_500);
        assert_eq!(recovery_target(12_000),9_000);
        assert_eq!(recovery_target(1_500),1_200);
    }

    #[test]
    fn force_recovery_times_out_and_clears_the_old_peak() {
        let mut director = Director::default();
        let mut s = base();
        ready(&mut director, s);
        s.fighting_points = 100;
        director.observe(520, s);
        assert!(director.rebuilding_force());
        director.observe(520 + MAX_RECOVERY_FRAMES, s);
        assert!(!director.rebuilding_force());
        director.observe(520 + MAX_RECOVERY_FRAMES + FORCE_SAMPLE_FRAMES, s);
        assert!(!director.rebuilding_force());
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

#[cfg(all(test, tests_sim_p_s))]
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
