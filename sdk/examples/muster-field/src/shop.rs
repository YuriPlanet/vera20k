//! The camp between battles. (Owner: shop)
//!
//! Every order for a camp unit is refused and queued ([`judge`]), and
//! [`apply`] reads one frame's queue as one gesture of the player:
//! - A group move arrives spread over the cells around the click
//!   (`movement::group_destination`): the member nearest the group's middle
//!   gets the clicked cell, so that one's cell says whether the group went
//!   into the row or out of it.
//! - A key sends one order per selected unit, so each key acts once.
//! - What happened is one line, and the last line is the choice on the
//!   field: the row left to right, or the supplies.

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get, set};

use crate::content::{self, MERCS, Merc};
use crate::state::{self, CAMP_RANK, COMPANY, MAX_COMPANY, MAX_OFFERS, Member, Num, PICKS};
use crate::text::{Line, Say};
use crate::{perks, world};
use muster_field_data::camp::{self, IMPROVE, ODDS, PLACES, REDRAW, ROW};
use muster_field_data::proto;

/// The quartermaster; armed, or the Guard key would send no order.
pub const OFFICER: &str = "SSRV";
/// What stands for a supply to pick.
pub const CRATES: [&str; 3] = ["TRUCKB", "PICK", "BUS"];

/// Cells between two offers.
const SPACING: i32 = 3;
/// How far from the camp a member may stand, on either axis.
const REACH: i32 = 9;
/// The row's dismiss zone on either side of its line.
const ROW_DEPTH: i32 = 1;
const OFFICER_DY: i32 = -4;
const SUPPLY_DY: i32 = -7;

/// Orders in one batch at most.
const BATCH: usize = 32;
/// Searches for a free cell that may fail in one frame: each is 121 host
/// calls, and the frame's operations are limited. The officer, created
/// first, may use them all.
const MAX_FAILS: i64 = 4;
/// A unit needs this long to set off after it was ordered.
const SET_OFF: i64 = 15;

/// What a player's order means in a muster.
#[derive(Clone, Copy)]
pub enum Verdict {
    /// Let the engine run it.
    Allow,
    /// Refuse it and do nothing.
    Refuse,
    /// Refuse it and hand it to [`apply`] in the next module phase.
    Queue,
}

/// One queued order.
#[derive(Clone, Copy)]
pub struct Order {
    pub unit: Techno,
    pub command: i32,
    pub cell: Cell,
}

// ------------------------------------------------------------------- State

/// A place in the row. A kept one may have a kind and no unit: it comes
/// back when there is room for it.
struct Offer {
    kind: Num,
    kept: Num,
    unit: Num,
    speed: Num,
    fire: Num,
}

impl Offer {
    const EMPTY: Offer = Offer {
        kind: Num::new(-1),
        kept: Num::new(0),
        unit: Num::new(NO_HANDLE),
        speed: Num::new(NO_HANDLE),
        fire: Num::new(NO_HANDLE),
    };

    fn unit(&self) -> Option<Techno> {
        some_unit(&self.unit)
    }

    fn is_kept(&self) -> bool {
        self.kept.get() == 1
    }

    fn merc(&self) -> &'static Merc {
        merc(self.kind.get().max(0) as usize)
    }

    fn clear(&self) {
        self.kind.set(-1);
        self.kept.set(0);
        self.unit.set(NO_HANDLE);
    }

    fn take_away(&self) {
        if let Some(unit) = self.unit() {
            let _ = Action::remove(unit);
        }
        self.unit.set(NO_HANDLE);
    }
}

static OFFERS: [Offer; MAX_OFFERS] = [Offer::EMPTY; MAX_OFFERS];
static OFFICER_UNIT: Num = Num::new(NO_HANDLE);
static TRUCKS: [Num; 3] = [const { Num::new(NO_HANDLE) }; 3];
/// The supply each truck stands for, or -1. A choice waits until it is
/// made, so waiting a muster does not draw another.
static CHOICES: [Num; 3] = [const { Num::new(-1) }; 3];
/// The frame a member was last placed in this muster, or -1.
static PLACED: [Num; MAX_COMPANY] = [const { Num::new(-1) }; MAX_COMPANY];
static MUSTERS: Num = Num::new(0);
static FAILS: Num = Num::new(0);

#[derive(Clone, Copy)]
enum Role {
    Offer(usize),
    Member(usize),
    Officer,
    Truck(usize),
}

fn some_unit(slot: &Num) -> Option<Techno> {
    Some(Techno(slot.get())).filter(|unit| unit.0 != NO_HANDLE)
}

fn role(unit: Techno) -> Option<Role> {
    let is = |slot: &Num| slot.get() == unit.0;
    if unit.0 == NO_HANDLE {
        None
    } else if is(&OFFICER_UNIT) {
        Some(Role::Officer)
    } else if let Some(at) = OFFERS.iter().position(|offer| is(&offer.unit)) {
        Some(Role::Offer(at))
    } else if let Some(at) = COMPANY
        .iter()
        .position(|member| !member.is_empty() && is(&member.unit))
    {
        Some(Role::Member(at))
    } else {
        TRUCKS.iter().position(is).map(Role::Truck)
    }
}

fn merc(kind: usize) -> &'static Merc {
    MERCS.get(kind).unwrap_or(&MERCS[0])
}

fn camp_rank() -> usize {
    camp::rank(CAMP_RANK.get())
}

fn places() -> usize {
    PLACES[camp_rank()]
}

fn company() -> usize {
    COMPANY.iter().filter(|member| !member.is_empty()).count()
}

fn row() -> &'static [Offer] {
    &OFFERS[..ROW[camp_rank()].min(MAX_OFFERS)]
}

fn row_is_out() -> bool {
    row().iter().all(|offer| offer.unit().is_none())
}

/// Two of `kind` that a third would merge with.
fn pair(kind: usize) -> Option<[usize; 2]> {
    let mut same = COMPANY
        .iter()
        .enumerate()
        .filter(|(_, member)| !member.is_empty() && !member.is_champion() && member.kind() == kind);
    Some([same.next()?.0, same.next()?.0])
}

fn unit_rank(member: &Member) -> i64 {
    // A rank outside 0..=2 would stop the module.
    perks::rank(member).clamp(0, 2)
}

// ------------------------------------------------------------------ Orders

/// Whether `unit` is one of the camp's: offer, member, officer or supply.
pub fn owns(unit: Techno) -> bool {
    role(unit).is_some()
}

/// Runs inside `CommandAllowed`: it must not write to the game. Even a move
/// within the line is queued, as only the whole group tells a click into
/// the row from a destination spread into it.
pub fn judge(order: Order) -> Verdict {
    match role(order.unit) {
        Some(_) => Verdict::Queue,
        None => Verdict::Allow,
    }
}

/// Carries out one frame's queued orders; true when the battle shall start.
pub fn apply(orders: &[Order]) -> bool {
    let orders = &orders[..orders.len().min(BATCH)];
    FAILS.set(0);
    // A key is read against the camp as the player saw it.
    let first = orders.iter().find(|order| role(order.unit).is_some());
    let Some(&first) = first else {
        return false;
    };
    let first_role = role(first.unit);
    let key = |order: &Order, wanted: Command| order.command == wanted as i32;
    let mut guarded = Guarded::default();
    let (mut stop, mut scatter, mut can_start, mut listed_supplies) = (false, false, false, false);
    for order in orders {
        let Some(role) = role(order.unit) else {
            continue;
        };
        listed_supplies |= matches!(role, Role::Truck(_));
        if key(order, Command::Guard) && guards_itself(order) {
            guarded.add(role);
        } else if key(order, Command::Stop) {
            stop |= matches!(role, Role::Offer(_) | Role::Officer);
        } else if key(order, Command::Scatter) {
            scatter = true;
            can_start |= matches!(role, Role::Officer | Role::Member(_));
        }
    }
    forget_the_lost();
    if scatter {
        can_start |= some_unit(&OFFICER_UNIT).is_none();
    }

    let mut said = guarded.keep();
    let picks = PICKS.get();
    said |= moves(orders);
    let fresh = refill();
    said |= guarded.improve();
    if stop && !fresh {
        said |= redraw();
    }
    if can_start {
        start_line();
        return true;
    }
    if scatter && !said {
        status_line();
        said = true;
    }
    if !said {
        refusal(first, first_role);
    }
    show_supplies();
    ensure_officer();
    let supplies = !fresh && (listed_supplies || PICKS.get() != picks);
    if !(supplies && choice_line()) {
        row_line(fresh);
    }
    false
}

/// G guards where the unit is; guarding a place elsewhere is another order.
fn guards_itself(order: &Order) -> bool {
    if order.cell.0 == NO_HANDLE {
        return true;
    }
    world::cell_of(order.unit).is_some_and(|here| {
        let (dx, dy) = world::delta(here, order.cell);
        dx.abs().max(dy.abs()) <= 1
    })
}

fn refusal(order: Order, role: Option<Role>) {
    let is = |wanted: Command| order.command == wanted as i32;
    let moving = is(Command::Move) || is(Command::MoveToObject) || is(Command::AttackMove);
    log(match role {
        Some(Role::Officer) if moving => "军需官不用移动：按 S 换货，G 升级，X 开战。",
        Some(Role::Truck(_)) if is(Command::Move) => "选补给时只选一辆车，再把它移走。",
        _ if moving => "请用普通移动，落点选营地里的空地。",
        _ if is(Command::Stop) => "S 换货：要选中货位佣兵或军需官。",
        _ if is(Command::Guard) => "G 只对货位佣兵（保留）和军需官（升级）有用。",
        _ => "营地里不能攻击、部署或进入：只用移动和 S、G、X。",
    });
}

/// The units of one press of G.
#[derive(Default)]
struct Guarded {
    offers: [bool; MAX_OFFERS],
    offer: bool,
    officer: bool,
    member: bool,
}

impl Guarded {
    fn add(&mut self, role: Role) {
        match role {
            Role::Offer(at) => {
                if let Some(marked) = self.offers.get_mut(at) {
                    *marked = true;
                    self.offer = true;
                }
            }
            Role::Officer => self.officer = true,
            Role::Member(_) => self.member = true,
            Role::Truck(_) => {}
        }
    }

    /// A group is all kept, or all let go once all of it is kept.
    fn keep(&self) -> bool {
        if !self.offer {
            return false;
        }
        let marked = || {
            OFFERS
                .iter()
                .zip(self.offers)
                .filter(|(offer, marked)| *marked && offer.unit().is_some())
                .map(|(offer, _)| offer)
        };
        if marked().next().is_none() {
            return false;
        }
        let keep = marked().any(|offer| !offer.is_kept());
        let mut line = Line::new().text(if keep { "保留" } else { "取消保留" });
        for (at, offer) in marked().enumerate() {
            offer.kept.set(i64::from(keep));
            if let Some(unit) = offer.unit() {
                // The chevron is the cue on the field.
                let _ = set(unit, Prop::Rank, i64::from(keep));
            }
            line = line
                .text(if at == 0 { " " } else { "、" })
                .text(offer.merc().name);
        }
        line = line.text("，共留 ").num(kept() as i64).text(" 个");
        line = line.text(if self.officer {
            "；升级请只选军需官。"
        } else if keep {
            "（带军衔标记）。"
        } else {
            "。"
        });
        line.log();
        true
    }

    /// Costly and for good, so a group with more than the officer does not.
    fn improve(&self) -> bool {
        if !self.officer || self.offer {
            return false;
        }
        if self.member {
            log("升级营地请只选军需官，再按 G。");
            return true;
        }
        improve();
        true
    }
}

// ------------------------------------------------------------------- Moves

#[derive(Clone, Copy)]
struct Step {
    unit: Techno,
    role: Role,
    to: Cell,
}

fn moves(orders: &[Order]) -> bool {
    let nowhere = Step {
        unit: Techno(NO_HANDLE),
        role: Role::Officer,
        to: Cell(NO_HANDLE),
    };
    let mut steps = [nowhere; BATCH];
    let mut len = 0;
    for order in orders {
        if order.command != Command::Move as i32 || order.cell.0 == NO_HANDLE {
            continue;
        }
        let Some(role) = role(order.unit) else {
            continue;
        };
        // Of two orders to one unit the later is meant.
        match steps[..len].iter().position(|step| step.unit == order.unit) {
            Some(at) => steps[at].to = order.cell,
            None if len < BATCH => {
                steps[len] = Step {
                    unit: order.unit,
                    role,
                    to: order.cell,
                };
                len += 1;
            }
            None => {}
        }
    }
    let steps = &steps[..len];
    match steps {
        [] => false,
        [
            Step {
                role: Role::Truck(at),
                ..
            },
        ] => pick(*at),
        _ => {
            if in_row(clicked(steps)) {
                let dismissed = dismiss(steps);
                let offered = steps.iter().any(|step| matches!(step.role, Role::Offer(_)));
                if offered && !dismissed {
                    log("把佣兵移到货位外面才算雇佣。");
                }
                dismissed || offered
            } else {
                let hired = hire(steps);
                place(steps, !hired) || hired
            }
        }
    }
}

/// Where the player clicked: the destination of the unit nearest the middle
/// of the group, of equals the last, as the engine's group spread picks it.
fn clicked(steps: &[Step]) -> Cell {
    let Some(last) = steps.last() else {
        return Cell(NO_HANDLE);
    };
    if steps.len() == 1 {
        return last.to;
    }
    let mut at = [(0i64, 0i64, Cell(NO_HANDLE)); BATCH];
    let (mut len, mut sum_x, mut sum_y) = (0, 0, 0);
    for step in steps.iter().take(BATCH) {
        if let (Ok(x), Ok(y)) = (get(step.unit, Prop::X), get(step.unit, Prop::Y)) {
            at[len] = (x, y, step.to);
            len += 1;
            sum_x += x;
            sum_y += y;
        }
    }
    if len == 0 {
        return last.to;
    }
    let (middle_x, middle_y) = (sum_x / len as i64, sum_y / len as i64);
    let far = |x: i64, y: i64| (x - middle_x).pow(2) + (y - middle_y).pow(2);
    let mut best = (i64::MAX, last.to);
    for &(x, y, to) in at[..len].iter().rev() {
        if far(x, y) < best.0 {
            best = (far(x, y), to);
        }
    }
    best.1
}

fn dismiss(steps: &[Step]) -> bool {
    let (mut count, mut refund, mut one) = (0, 0, (0, false));
    let mut failed = false;
    for step in steps {
        let Some(Role::Member(at)) = role(step.unit) else {
            continue;
        };
        let member = &COMPANY[at];
        if Action::remove(step.unit).is_err() && world::alive(step.unit) {
            failed = true;
            continue;
        }
        one = (member.kind(), member.is_champion());
        refund += camp::refund(merc(member.kind()).price, member.is_champion());
        count += 1;
        member.clear();
        PLACED[at].set(-1);
    }
    if count == 0 {
        if failed {
            log("这名队员现在不能遣散。");
        }
        return failed;
    }
    let mut line = Line::new().text("遣散 ");
    line = if count == 1 {
        let champion = if one.1 { "精锐" } else { "" };
        line.text(champion).text(merc(one.0).name)
    } else {
        line.num(count).text(" 人")
    };
    line = if world::pay(refund) {
        line.text("，退 +").num(i64::from(refund))
    } else {
        line.text("，退款失败")
    };
    purse(line).text("。").log();
    true
}

/// Moves members in the line. `tell` is off when a hire says it already.
fn place(steps: &[Step], tell: bool) -> bool {
    let (mut count, mut stuck, mut clamped) = (0, 0, false);
    for step in steps {
        // A unit hired from these same steps has had its order.
        let Role::Member(at) = step.role else {
            continue;
        };
        let member = &COMPANY[at];
        if member.unit.get() != step.unit.0 {
            continue; // Merged away by the same gesture.
        }
        let to = stand(member.kind(), step.to);
        if send(at, to) {
            count += 1;
            clamped |= is_outside(step.to);
        } else {
            stuck += 1;
        }
    }
    if !tell || count + stuck == 0 {
        return false;
    }
    let mut line = Line::new();
    line = if count == 0 {
        line.text("走不到那里，队员留在原地")
    } else {
        line.text("列阵 ").num(count).text(" 人，下一轮仍站那里")
    };
    if clamped {
        line = line.text("（只能在营地 9 格内）");
    }
    line.text("。").log();
    true
}

/// Orders member `at` to `to` and makes that its place in the line.
fn send(at: usize, to: Cell) -> bool {
    let member = &COMPANY[at];
    let Some(unit) = member.unit() else {
        return false;
    };
    if Action::set_mission(unit, Mission::Move, to).is_err() {
        return false;
    }
    member.home.set(to.0);
    PLACED[at].set(i64::from(frame()));
    true
}

fn hire(steps: &[Step]) -> bool {
    let mut wanted = [(0usize, Cell(NO_HANDLE)); MAX_OFFERS];
    let mut len = 0;
    for step in steps {
        if let Role::Offer(at) = step.role {
            if len < MAX_OFFERS && OFFERS[at].unit() == Some(step.unit) {
                wanted[len] = (at, step.to);
                len += 1;
            }
        }
    }
    if len == 0 {
        return false;
    }
    let mut hired = [0usize; MAX_OFFERS];
    let (mut count, mut cost, mut merged, mut merges_made, mut clamped) = (0, 0, None, 0, false);
    let mut pay_failed = false;
    // Merges first: they free places, and one hire can complete a pair.
    while len > 0 {
        let money = world::credits();
        let room = company() < places();
        let affordable = |&(at, _): &(usize, Cell)| i64::from(OFFERS[at].merc().price) <= money;
        let merges = |&(at, _): &(usize, Cell)| pair(OFFERS[at].kind.get() as usize).is_some();
        let pending = &wanted[..len];
        let next = pending
            .iter()
            .position(|want| affordable(want) && merges(want))
            .or_else(|| pending.iter().position(|want| room && affordable(want)));
        let Some(next) = next else {
            break;
        };
        let (at, to) = wanted[next];
        let offer = &OFFERS[at];
        let (kind, price) = (offer.kind.get() as usize, offer.merc().price);
        match enlist(offer, kind, to) {
            Some(champion) => {
                if count < MAX_OFFERS {
                    hired[count] = kind;
                    count += 1;
                }
                cost += price;
                clamped |= is_outside(to);
                if champion {
                    merged = Some(kind);
                    merges_made += 1;
                }
            }
            None => pay_failed = true,
        }
        wanted.copy_within(next + 1..len, next);
        len -= 1;
    }

    let left = wanted[..len].first().map(|&(at, _)| OFFERS[at].merc());
    if count == 0 {
        match left {
            Some(offer) if i64::from(offer.price) > world::credits() => {
                short(Line::new().text("钱不够，").text(offer.name), offer.price)
            }
            Some(_) if !pay_failed => full_line(),
            _ => log("这名佣兵现在雇不了。"),
        }
        return true;
    }
    let mut line = Line::new();
    line = match (merged, count) {
        (Some(_), 1) => line.text("三合一！精锐").text(merc(hired[0]).name),
        (_, 1..=2) => {
            line = line.text("雇佣 ").text(merc(hired[0]).name);
            if count == 2 {
                line = line.text("、").text(merc(hired[1]).name);
            }
            line
        }
        _ => line.text("雇佣 ").num(count as i64).text(" 人"),
    };
    line = line.text(" -").num(i64::from(cost));
    if let Some(kind) = merged {
        if count > 1 {
            line = line.text("，合成精锐").text(merc(kind).name);
        }
        if CHOICES.iter().any(|choice| choice.get() >= 0) || any_choice() {
            line = line.text("，补给+").num(merges_made);
            if row_is_out() {
                // The free row takes the last line from the supplies.
                line = line.text("（选车按 X 看）");
            }
        }
    } else if count == 1 && pair(hired[0]).is_some() {
        line = line.text("（同名 2/3）");
    }
    line = purse(line);
    // After a merge the line is long enough without it.
    if let Some(offer) = left.filter(|_| merged.is_none()) {
        line = line.text("；").text(offer.name);
        line = if i64::from(offer.price) > world::credits() {
            line.text("要 ")
                .num(i64::from(offer.price))
                .text("，钱不够")
        } else {
            line.text("没位置了")
        };
    } else if clamped {
        line = line.text("（落点收回 9 格内）");
    }
    line.text("。").log();
    true
}

fn any_choice() -> bool {
    perks::choices().iter().any(Option::is_some)
}

/// One hire and the merge it makes: whether a champion came of it, or
/// `None` when it could not be done.
fn enlist(offer: &Offer, kind: usize, to: Cell) -> Option<bool> {
    let unit = offer.unit()?;
    let pair = pair(kind);
    let slot = match pair {
        Some([first, _]) => first,
        None => COMPANY.iter().position(Member::is_empty)?,
    };
    if !world::pay(-offer.merc().price) {
        return None;
    }
    world::release(world::Hold(offer.speed.get(), offer.fire.get()));
    offer.clear();
    for at in pair.iter().flatten().copied() {
        if let Some(old) = COMPANY[at].unit() {
            let _ = Action::remove(old);
        }
        COMPANY[at].clear();
        PLACED[at].set(-1);
    }
    let member = &COMPANY[slot];
    member.kind.set(kind as i64);
    member.champion.set(i64::from(pair.is_some()));
    member.unit.set(unit.0);
    let to = stand(kind, to);
    member.home.set(to.0);
    PLACED[slot].set(-1);
    let _ = set(unit, Prop::Rank, unit_rank(member));
    send(slot, to);
    if pair.is_some() {
        PICKS.add(1);
    }
    Some(pair.is_some())
}

fn full_line() {
    let mut line = Line::new().text("队伍已满");
    line = fraction(line).text("：先遣散");
    if let Some(&cost) = IMPROVE.get(camp_rank()) {
        line = line
            .text("，或选军需官按 G 升级（")
            .num(i64::from(cost))
            .text("）");
    }
    line.text("。").log();
}

// ---------------------------------------------------------------- Supplies

fn pick(at: usize) -> bool {
    let perk = CHOICES[at].get();
    if PICKS.get() <= 0 || perk < 0 {
        return false;
    }
    let perk = perk as usize;
    // What this taking adds: after the grant it would be the next one's.
    let effect = perks::effect(perk);
    perks::grant(perk);
    PICKS.add(-1);
    // Trucks anew: a second order to the old ones picks nothing.
    for (truck, choice) in TRUCKS.iter().zip(&CHOICES) {
        if let Some(unit) = some_unit(truck) {
            let _ = Action::remove(unit);
        }
        truck.set(NO_HANDLE);
        choice.set(-1);
    }
    // A supply can change a rank.
    for member in COMPANY.iter().filter(|member| !member.is_empty()) {
        if let Some(unit) = member.unit() {
            let _ = set(unit, Prop::Rank, unit_rank(member));
        }
    }
    let mut line = Line::new()
        .text("获得补给：")
        .text(perks::name(perk))
        .text("，")
        .text(effect);
    if PICKS.get() > 0 && any_choice() {
        line = line.text("；还可选 ").num(PICKS.get()).text(" 次");
    }
    line.text("。").log();
    true
}

/// Three trucks while there is a pick and something left to pick.
fn show_supplies() {
    if PICKS.get() <= 0 {
        return;
    }
    if CHOICES.iter().all(|choice| choice.get() < 0) {
        let drawn = perks::choices();
        for (choice, perk) in CHOICES.iter().zip(drawn) {
            choice.set(perk.map_or(-1, |perk| perk as i64));
        }
    }
    for (at, (truck, id)) in TRUCKS.iter().zip(CRATES).enumerate() {
        if CHOICES[at].get() < 0 || truck.get() != NO_HANDLE {
            continue;
        }
        let spot = world::offset(state::camp(), (at as i32 - 1) * SPACING, SUPPLY_DY);
        if let Some(unit) = create(spot, id) {
            world::hold(unit);
            truck.set(unit.0);
        }
    }
}

/// The choice, if one is on the field.
fn choice_line() -> bool {
    if PICKS.get() <= 0 || TRUCKS.iter().all(|truck| truck.get() == NO_HANDLE) {
        return false;
    }
    let mut units = [None; MAX_OFFERS];
    for (slot, (truck, choice)) in units.iter_mut().zip(TRUCKS.iter().zip(&CHOICES)) {
        *slot = some_unit(truck).filter(|_| choice.get() >= 0);
    }
    let (shown, len) = left_to_right(units);
    if len == 0 {
        return false;
    }
    // Names and effects on two lines: on one, three of them are too wide.
    let mut names = Line::new().text("移走一辆车选补给，左起：");
    let mut effects = Line::new().text("效果：");
    for (place, &(_, at)) in shown[..len].iter().enumerate() {
        let perk = CHOICES[at].get() as usize;
        let gap = if place == 0 { "" } else { "｜" };
        names = names.text(gap).text(perks::name(perk));
        effects = effects.text(gap).text(perks::effect(perk));
    }
    names.log();
    effects.log();
    true
}

// -------------------------------------------------------------------- Camp

fn kept() -> usize {
    row().iter().filter(|offer| offer.is_kept()).count()
}

fn draw() -> usize {
    let odds = &ODDS[camp_rank()];
    let roll = random(1, 100);
    let (mut tier, mut sum) = (0, odds[0]);
    while roll > sum && tier + 1 < odds.len() {
        tier += 1;
        sum += odds[tier];
    }
    let of_tier = || {
        MERCS
            .iter()
            .enumerate()
            .filter(move |(_, merc)| merc.tier == tier)
    };
    let count = of_tier().count();
    if count == 0 {
        return random(0, MERCS.len() as i32 - 1) as usize;
    }
    let nth = random(0, count as i32 - 1) as usize;
    of_tier().nth(nth).map_or(0, |(kind, _)| kind)
}

/// Stands offer `at` on its spot: the one it keeps, or a draw.
fn stock(at: usize, draw_new: bool) {
    let offer = &OFFERS[at];
    if draw_new && !offer.is_kept() {
        offer.kind.set(draw() as i64);
    }
    if offer.kind.get() < 0 {
        return;
    }
    let count = row().len() as i32;
    let dx = at as i32 * SPACING - (count - 1) * SPACING / 2;
    let spot = world::offset(state::camp(), dx, 0);
    let Some(unit) = create(spot, offer.merc().id) else {
        if !offer.is_kept() {
            offer.clear();
        }
        return;
    };
    offer.unit.set(unit.0);
    let held = world::hold(unit);
    offer.speed.set(held.0);
    offer.fire.set(held.1);
    if offer.is_kept() {
        let _ = set(unit, Prop::Rank, 1);
    }
}

/// A row that was hired out is drawn new for nothing.
fn refill() -> bool {
    if !row_is_out() {
        return false;
    }
    for at in 0..row().len() {
        stock(at, true);
    }
    !row_is_out()
}

fn redraw() -> bool {
    let free = row().len() - kept();
    if free == 0 {
        log("货位全都保留着：先选中按 G 取消保留。");
        return true;
    }
    if world::credits() < i64::from(REDRAW) {
        short(Line::new().text("钱不够，换货"), REDRAW);
        return true;
    }
    if !world::pay(-REDRAW) {
        log("现在不能换货。");
        return true;
    }
    for offer in row().iter().filter(|offer| !offer.is_kept()) {
        offer.take_away();
        offer.clear();
    }
    let mut drawn = false;
    for (at, offer) in row().iter().enumerate() {
        if !offer.is_kept() {
            stock(at, true);
            drawn |= offer.unit().is_some();
        }
    }
    if !drawn {
        world::pay(REDRAW);
        log("货位放不下新佣兵，换货的钱已退回。");
        return true;
    }
    let mut line = Line::new().text("换货 -").num(i64::from(REDRAW));
    if kept() > 0 {
        line = line.text("（保留的 ").num(kept() as i64).text(" 个不变）");
    }
    purse(line).text("。").log();
    true
}

fn improve() {
    let rank = camp_rank();
    let Some(&cost) = IMPROVE.get(rank) else {
        fraction(Line::new().text("营地已是最高 3 级：5 个货位，队伍"))
            .text("。")
            .log();
        return;
    };
    if world::credits() < i64::from(cost) {
        short(Line::new().text("钱不够，升级营地"), cost);
        return;
    }
    if !world::pay(-cost) {
        log("现在不能升级营地。");
        return;
    }
    // The row is centred, so every offer moves.
    for offer in &OFFERS {
        offer.take_away();
    }
    let old = row().len();
    CAMP_RANK.set(rank as i64 + 1);
    for at in 0..row().len() {
        stock(at, at >= old);
    }
    // The row's zone grew over some of the line.
    for (at, member) in COMPANY.iter().enumerate() {
        if member.is_empty() {
            continue;
        }
        let home = Cell(member.home.get());
        if in_row(home) {
            let to = stand(member.kind(), home);
            if !send(at, to) {
                member.home.set(to.0);
            }
        }
    }
    let mut line = Line::new()
        .text("营地 ")
        .num(rank as i64 + 2)
        .text(" 级 -")
        .num(i64::from(cost));
    line = purse(line);
    if let Some(&next) = IMPROVE.get(rank + 1) {
        line = line.text("，下次升级 ").num(i64::from(next));
    }
    line.text("。").log();
}

fn ensure_officer() {
    if some_unit(&OFFICER_UNIT).is_some() {
        return;
    }
    // The camp's own cell held the player's first unit; and any armed unit of
    // the bound types will do rather than none.
    let tries = [
        (OFFICER, OFFICER_DY),
        (OFFICER, -OFFICER_DY),
        (OFFICER, 0),
        (MERCS[0].id, 0),
    ];
    for (id, dy) in tries {
        if let Some(unit) = create(world::offset(state::camp(), 0, dy), id) {
            world::hold(unit);
            OFFICER_UNIT.set(unit.0);
            return;
        }
    }
}

fn create(spot: Cell, id: &str) -> Option<Techno> {
    if FAILS.get() >= MAX_FAILS {
        return None;
    }
    let made = world::create_near(spot, id, state::player());
    if made.is_none() {
        FAILS.add(1);
    }
    made
}

fn forget_the_lost() {
    let lost = |slot: &Num| some_unit(slot).is_some_and(|unit| !world::alive(unit));
    for offer in &OFFERS {
        if lost(&offer.unit) {
            if offer.is_kept() {
                offer.unit.set(NO_HANDLE);
            } else {
                offer.clear();
            }
        }
    }
    for slot in TRUCKS.iter().chain([&OFFICER_UNIT]) {
        if lost(slot) {
            slot.set(NO_HANDLE);
        }
    }
    for member in &COMPANY {
        if lost(&member.unit) {
            member.unit.set(NO_HANDLE);
        }
    }
}

// ------------------------------------------------------------------ Ground

fn in_row(cell: Cell) -> bool {
    if cell.0 == NO_HANDLE {
        return false;
    }
    let (dx, dy) = world::delta(state::camp(), cell);
    let half = ((row().len() as i32 - 1) * SPACING + 1) / 2;
    dy.abs() <= ROW_DEPTH && dx.abs() <= half + 1
}

fn is_outside(cell: Cell) -> bool {
    let (dx, dy) = world::delta(state::camp(), cell);
    dx.abs().max(dy.abs()) > REACH
}

/// The closest place in the line to `wanted` that `kind` can stand on.
fn stand(kind: usize, wanted: Cell) -> Cell {
    let camp = state::camp();
    let (dx, mut dy) = world::delta(camp, wanted);
    if in_row(wanted) {
        dy = if dy < 0 {
            -ROW_DEPTH - 1
        } else {
            ROW_DEPTH + 1
        };
    }
    let place = world::offset(camp, dx.clamp(-REACH, REACH), dy.clamp(-REACH, REACH));
    let speed = if merc(kind).tags & content::FOOT != 0 {
        SpeedType::Foot
    } else {
        SpeedType::Track
    };
    for ring in 0..=3i32 {
        for y in -ring..=ring {
            for x in -ring..=ring {
                let cell = world::offset(place, x, y);
                if x.abs().max(y.abs()) == ring
                    && !is_outside(cell)
                    && !in_row(cell)
                    && cell.passable(speed) == Ok(true)
                {
                    return cell;
                }
            }
        }
    }
    place
}

// --------------------------------------------------------------- Interface

/// The order a player would give a camp unit for the interface's message
/// `what` (`proto::MSG_*`) about offer, truck or member `index`: a button
/// does what the order does, through [`apply`]. `None` when the unit it
/// needs is not in camp.
pub fn ui_order(what: u8, index: usize) -> Option<Order> {
    let order = |unit: Techno, command: Command, cell: Cell| Order {
        unit,
        command: command as i32,
        cell,
    };
    let offer = || row().get(index).and_then(Offer::unit);
    // The officer, or any camp unit the same key works on.
    let officer = || some_unit(&OFFICER_UNIT);
    let camp = state::camp();
    match what {
        proto::MSG_HIRE => Some(order(offer()?, Command::Move, free_place())),
        proto::MSG_KEEP => Some(order(offer()?, Command::Guard, Cell(NO_HANDLE))),
        proto::MSG_REDRAW => {
            let unit = officer().or_else(|| row().iter().find_map(Offer::unit))?;
            Some(order(unit, Command::Stop, Cell(NO_HANDLE)))
        }
        proto::MSG_IMPROVE => Some(order(officer()?, Command::Guard, Cell(NO_HANDLE))),
        proto::MSG_START => {
            let member = COMPANY.iter().find_map(Member::unit);
            let unit = officer()
                .or(member)
                .or_else(|| row().iter().find_map(Offer::unit))?;
            Some(order(unit, Command::Scatter, Cell(NO_HANDLE)))
        }
        proto::MSG_PICK => {
            let truck = TRUCKS.get(index).and_then(some_unit)?;
            let away = world::offset(camp, 0, SUPPLY_DY - 2);
            Some(order(truck, Command::Move, away))
        }
        // Into the row: a dismissal.
        proto::MSG_DISMISS => Some(order(COMPANY.get(index)?.unit()?, Command::Move, camp)),
        _ => None,
    }
}

/// The first place of the starting line that no member calls home.
fn free_place() -> Cell {
    let camp = state::camp();
    for at in 0..MAX_COMPANY as i32 {
        let spot = world::offset(camp, (at % 5 - 2) * 2, 4 + at / 5 * 2);
        if COMPANY.iter().all(|member| member.home.get() != spot.0) {
            return spot;
        }
    }
    world::offset(camp, 0, 4)
}

/// The camp as `proto` publishes it.
pub struct View {
    pub offers: i64,
    pub choices: i64,
    pub company: [i64; 2],
    pub flags: i64,
}

pub fn ui_view() -> View {
    let mut offers = 0;
    for (at, offer) in row().iter().enumerate() {
        let kind = offer.kind.get();
        let packed = proto::Offer {
            kind: (kind >= 0).then_some(kind as usize),
            kept: offer.is_kept(),
            standing: offer.unit().is_some(),
        };
        offers = proto::with_byte(offers, at, packed.pack());
    }
    let trucks = TRUCKS.iter().any(|truck| some_unit(truck).is_some());
    let mut choices = 0;
    for (at, (truck, choice)) in TRUCKS.iter().zip(&CHOICES).enumerate() {
        let perk = choice.get();
        if some_unit(truck).is_some() && perk >= 0 && PICKS.get() > 0 {
            choices = proto::with_byte(choices, at, perk as u8 + 1);
        }
    }
    let mut company = [0; 2];
    for (at, member) in COMPANY.iter().enumerate() {
        let packed = proto::Member {
            kind: (!member.is_empty()).then(|| member.kind()),
            champion: member.is_champion(),
            standing: member.unit().is_some(),
        };
        let half = &mut company[at / 5];
        *half = proto::with_byte(*half, at % 5, packed.pack());
    }
    let mut flags = 0;
    if some_unit(&OFFICER_UNIT).is_some() {
        flags |= proto::FLAG_OFFICER;
    }
    if trucks && PICKS.get() > 0 {
        flags |= proto::FLAG_TRUCKS;
    }
    View {
        offers,
        choices,
        company,
        flags,
    }
}

// ------------------------------------------------------------------ Phases

/// Puts the camp and the company on the cleared field.
pub fn open() {
    FAILS.set(0);
    MUSTERS.add(1);
    OFFICER_UNIT.set(NO_HANDLE);
    for truck in &TRUCKS {
        truck.set(NO_HANDLE);
    }
    for offer in &OFFERS {
        offer.unit.set(NO_HANDLE);
    }
    ensure_officer();
    for at in 0..row().len() {
        stock(at, true);
    }
    show_supplies();
    let mut missing = 0;
    let camp = state::camp();
    for (at, member) in COMPANY.iter().enumerate() {
        PLACED[at].set(-1);
        member.unit.set(NO_HANDLE);
        if member.is_empty() {
            continue;
        }
        if member.home.get() == NO_HANDLE {
            let at = at as i32;
            let spot = world::offset(camp, (at % 5 - 2) * 2, 4 + at / 5 * 2);
            member.home.set(spot.0);
        }
        let id = merc(member.kind()).id;
        let made =
            create(Cell(member.home.get()), id).or_else(|| create(world::offset(camp, 0, 4), id));
        match made {
            Some(unit) => {
                member.unit.set(unit.0);
                let _ = set(unit, Prop::Rank, unit_rank(member));
            }
            None => missing += 1,
        }
    }
    opening(missing);
}

/// At most five lines, the row last: the newest line stays longest. The
/// first muster opens with the credit (README.md).
fn opening(missing: i64) {
    let muster = MUSTERS.get();
    if muster == 1 {
        log("玩法参考 B 站 UP 主 MK 的红警酒馆，致谢。");
        log("把货位佣兵移到空地＝雇佣；队员移动＝列阵，移回货位＝遣散退半价。");
        Line::new()
            .text("军需官：S 换货 ")
            .num(i64::from(REDRAW))
            .text("、G 升级 ")
            .num(i64::from(IMPROVE[0]))
            .text("、X 开战（队员也行）；货位：G 保留、X 查看。")
            .log();
    }
    let no_officer = some_unit(&OFFICER_UNIT).is_none();
    if no_officer {
        log("军需官进不了场：选中任意单位按 X 开战。");
    } else if missing > 0 {
        fraction(
            Line::new()
                .text("有 ")
                .num(missing)
                .text(" 人进不了场，本轮缺阵；队伍"),
        )
        .text("。")
        .log();
    } else if PICKS.get() > 0 {
        // The supplies take two lines.
    } else if muster == 2 {
        log("三名同种普通队员合成一名精锐并得一次补给，满员也能雇第三名。");
    } else if muster == 3 {
        log("G 保留的佣兵换货和下轮都不走；队员站位会一直沿用。");
    }
    if muster > 1 || (!no_officer && missing == 0) {
        status_line();
    }
    choice_line();
    row_line(false);
}

/// Takes down all but the company, and settles where each member stands.
pub fn close() {
    for slot in TRUCKS.iter().chain([&OFFICER_UNIT]) {
        if let Some(unit) = some_unit(slot) {
            let _ = Action::remove(unit);
        }
        slot.set(NO_HANDLE);
    }
    for offer in &OFFERS {
        offer.take_away();
        if !offer.is_kept() {
            offer.clear();
        }
    }
    let now = i64::from(frame());
    for (at, member) in COMPANY.iter().enumerate() {
        let placed = PLACED[at].get();
        PLACED[at].set(-1);
        // One who was not ordered keeps the place, not where it was squeezed.
        let Some(unit) = member.unit().filter(|_| placed >= 0) else {
            continue;
        };
        // One still on the way will stand where it was sent.
        if now - placed < SET_OFF || get(unit, Prop::Moving) == Ok(1) {
            continue;
        }
        if let Some(cell) = world::cell_of(unit) {
            if cell.0 != member.home.get() {
                member.home.set(stand(member.kind(), cell).0);
            }
        }
    }
}

// -------------------------------------------------------------------- Text

fn fraction(line: Line) -> Line {
    line.text(" ")
        .num(company() as i64)
        .text("/")
        .num(places() as i64)
}

/// The money left and the places used, after every action.
fn purse(line: Line) -> Line {
    fraction(line.text("，余 ").num(world::credits()).text("，队伍"))
}

fn short(line: Line, cost: i32) {
    let money = world::credits();
    line.text("要 ")
        .num(i64::from(cost))
        .text("，余 ")
        .num(money)
        .text("（还差 ")
        .num(i64::from(cost) - money)
        .text("）。")
        .log();
}

fn start_line() {
    let ready = COMPANY.iter().filter(|member| member.unit().is_some());
    purse(
        Line::new()
            .text("点兵结束，")
            .num(ready.count() as i64)
            .text(" 人出战"),
    )
    .text("。")
    .log();
}

fn status_line() {
    let rank = camp_rank();
    let mut line = Line::new().text("营地 ").num(rank as i64 + 1).text(" 级");
    line = purse(line).text("；换货 ").num(i64::from(REDRAW));
    line = match IMPROVE.get(rank) {
        Some(&cost) => line.text("，升级 ").num(i64::from(cost)),
        None => line.text("，已满级"),
    };
    line.text("。").log();
}

/// The places of `units` as they stand on the screen, where x - y grows to
/// the right.
fn left_to_right(units: [Option<Techno>; MAX_OFFERS]) -> ([(i32, usize); MAX_OFFERS], usize) {
    let mut shown = [(0i32, 0usize); MAX_OFFERS];
    let mut len = 0;
    for (at, unit) in units.into_iter().enumerate() {
        let Some(cell) = unit.and_then(world::cell_of) else {
            continue;
        };
        let (x, y) = world::delta(state::camp(), cell);
        if len < MAX_OFFERS {
            shown[len] = (x - y, at);
            len += 1;
        }
    }
    // By hand: the library's sort brings panicking paths.
    for end in 1..len {
        let mut at = end;
        while at > 0 && shown[at].0 < shown[at - 1].0 {
            shown.swap(at, at - 1);
            at -= 1;
        }
    }
    (shown, len)
}

fn row_line(fresh: bool) {
    let mut units = [None; MAX_OFFERS];
    for (slot, offer) in units.iter_mut().zip(row()) {
        *slot = offer.unit();
    }
    let (shown, len) = left_to_right(units);
    if len == 0 {
        return log("货位空着：选中军需官按 S 换货。");
    }
    let mut line = Line::new().text(if fresh {
        "免费新货左起："
    } else {
        "货位左起："
    });
    for (place, &(_, at)) in shown[..len].iter().enumerate() {
        let offer = &OFFERS[at];
        line = line
            .text(if place == 0 { "" } else { " " })
            .text(offer.merc().name)
            .num(i64::from(offer.merc().price));
        if offer.is_kept() {
            line = line.text("[留]");
        }
    }
    line.log();
}
