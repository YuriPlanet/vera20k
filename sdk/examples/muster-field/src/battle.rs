//! One battle: where the raiders arrive, who attacks whom, and how it ends.
//! (Owner: battle)
//!
//! Both sides stand held for a short face-off and then fight by themselves:
//! every second each unit's target is looked over, with a preference for
//! what its weapon can hurt. A champion fielded alone is surrounded:
//! early waves are too small to stop some champions, and no champion is to
//! hold a wave by itself.

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

use crate::state::{self, COMPANY, MAX_COMPANY, MAX_RAIDERS, Num, RAIDERS, WAVE};
use crate::text::{Line, Say};
use crate::{content, perks, waves, world};
use muster_field_data::FRONT_NAMES;

pub struct Report {
    pub won: bool,
    pub kills: i64,
}

/// Frames both sides look at each other.
const FACE_OFF: i32 = 45;
/// Timer seconds of fighting (15 frames each).
const FIGHT_SECONDS: i32 = 120;
/// What the raiders' fire and armor are multiplied by against a champion
/// fielded alone, so that no champion clears a wave by itself.
const SURROUND: i64 = 6 * FIXED_ONE;
/// Frames the field is left as it is once a side is gone.
const AFTERMATH: i32 = 30;
/// Frames between two looks at one side's targets.
const LOOK: i32 = 15;
/// Frames without a hit on either side after which everyone closes in.
const STANDOFF: i32 = 120;
/// Frames that closing in is left to work before targets are looked at.
const CLOSING: i32 = 45;
/// Frames between two counts of the living.
const COUNT: i32 = 3;

const FACING_OFF: i64 = 0;
const FIGHTING: i64 = 1;
const WON: i64 = 2;
const LOST: i64 = 3;
const IDLE: i64 = 4;

static STAGE: Num = Num::new(IDLE);
/// The frame the stage ends.
static UNTIL: Num = Num::new(0);
static ARRIVED: Num = Num::new(0);
/// The health on the field at the last count, and when that last changed.
static HEALTH: Num = Num::new(0);
static HIT_AT: Num = Num::new(0);
/// No look at targets before this frame.
static LOOK_AT: Num = Num::new(0);

const FIGHTERS: usize = MAX_COMPANY + MAX_RAIDERS;
/// Per fighter, the company first: its index in `NATURES`, whether it is a
/// vehicle, and looks in a row it spent unable to get at its target.
static NATURE: [Num; FIGHTERS] = [const { Num::new(0) }; FIGHTERS];
static IS_VEHICLE: [Num; FIGHTERS] = [const { Num::new(0) }; FIGHTERS];
static STUCK: [Num; FIGHTERS] = [const { Num::new(0) }; FIGHTERS];

/// Creates the wave's raiders, starts the countdown and sends both sides in.
pub fn start() {
    let now = frame();
    let wave = waves::get(WAVE.get());
    for raider in &RAIDERS {
        raider.set(NO_HANDLE);
    }
    for stuck in &STUCK {
        stuck.set(0);
    }
    let side = spawn(wave);
    let (mut fielded, mut champions) = (0, 0);
    for (at, member) in COMPANY.iter().enumerate() {
        match member.unit() {
            Some(unit) if !member.is_empty() && world::alive(unit) => {
                let id = content::MERCS.get(member.kind()).map_or("", |merc| merc.id);
                enlist(at, unit, id);
                fielded += 1;
                champions += i32::from(member.is_champion());
            }
            _ => member.unit.set(NO_HANDLE),
        }
    }
    perks::battle_started();

    let arrived = ARRIVED.get();
    if arrived == 0 {
        // Nowhere to stand on this map: the wave counts as beaten, or the
        // same wave would come back until the banners were gone.
        log("袭击者找不到进攻营地的路，此波不战而胜。");
        STAGE.set(WON);
        UNTIL.set(i64::from(now + AFTERMATH));
        return;
    }
    for slot in 0..FIGHTERS {
        if let Some(unit) = fighter(slot) {
            // They lapse by themselves, so nothing is left to release.
            let _ = Modifier::multiply(unit, Prop::SpeedMultiplier, 0, FACE_OFF);
            let _ = Modifier::override_(unit, Prop::DisableWeapons, 1, FACE_OFF);
        }
    }
    let alone = fielded == 1 && champions == 1;
    if alone {
        for raider in &RAIDERS {
            let unit = Techno(raider.get());
            if unit.0 != NO_HANDLE {
                for prop in [Prop::FirepowerMultiplier, Prop::ArmorMultiplier] {
                    let _ = Modifier::multiply(unit, prop, SURROUND, -1);
                }
            }
        }
    }
    STAGE.set(FACING_OFF);
    UNTIL.set(i64::from(now + FACE_OFF));
    count_down(FACE_OFF / 15);
    Line::new()
        .text("【")
        .text(wave.name)
        .text("】")
        .num(arrived)
        .text("名袭击者自")
        .text(side.name)
        .text("来，")
        .num(i64::from(FACE_OFF / 15))
        .text("秒后交战。")
        .text(if alone {
            "精锐孤军被围，敌攻防×6。"
        } else {
            ""
        })
        .log();
}

/// Every frame of a battle; the report once it is decided. By then the field
/// is clear of both sides and the timer stands.
pub fn tick(now: i32) -> Option<Report> {
    let over = i64::from(now) >= UNTIL.get();
    match STAGE.get() {
        FACING_OFF if over => {
            STAGE.set(FIGHTING);
            UNTIL.set(i64::from(now + FIGHT_SECONDS * 15));
            count_down(FIGHT_SECONDS);
            HIT_AT.set(i64::from(now));
            LOOK_AT.set(i64::from(now));
            look(true);
            look(false);
        }
        FIGHTING => {
            if now % COUNT == 0 || over {
                let (ours, theirs, health) = count_living();
                if health != HEALTH.get() {
                    HEALTH.set(health);
                    HIT_AT.set(i64::from(now));
                } else if i64::from(now) - HIT_AT.get() >= i64::from(STANDOFF) {
                    HIT_AT.set(i64::from(now));
                    LOOK_AT.set(i64::from(now + CLOSING));
                    close_in();
                }
                // Both gone is a loss, as is a raider left when time is up.
                if ours == 0 || theirs == 0 {
                    STAGE.set(if ours > 0 { WON } else { LOST });
                    UNTIL.set(i64::from(now + AFTERMATH));
                    let _ = Action::timer_stop();
                    return None;
                }
                if over {
                    log("时间到，袭击者仍在场上。");
                    return Some(close(false));
                }
            }
            if i64::from(now) < LOOK_AT.get() {
                return None;
            }
            match now % LOOK {
                0 => look(true),
                7 => look(false),
                _ => {}
            }
        }
        stage @ (WON | LOST) if over => return Some(close(stage == WON)),
        _ => {}
    }
    None
}

fn close(won: bool) -> Report {
    let (_, living, _) = count_living();
    let kills = (ARRIVED.get() - living).max(0);
    let _ = Action::timer_stop();
    world::clear_forces(state::player());
    world::clear_forces(state::enemy());
    for member in &COMPANY {
        member.unit.set(NO_HANDLE);
    }
    for raider in &RAIDERS {
        raider.set(NO_HANDLE);
    }
    ARRIVED.set(0);
    STAGE.set(IDLE);
    Report { won, kills }
}

fn count_down(seconds: i32) {
    let _ = Action::timer_set(seconds);
    let _ = Action::timer_start();
}

/// Forgets the dead; counts the living of both sides and their health.
fn count_living() -> (i64, i64, i64) {
    let (mut ours, mut theirs, mut health) = (0, 0, 0);
    for slot in 0..FIGHTERS {
        let Some(unit) = fighter(slot) else { continue };
        // A unit that no longer answers is gone.
        let left = unit.health().unwrap_or(0);
        let living = left > 0;
        health += left.max(0);
        if let Some(member) = COMPANY.get(slot) {
            if living {
                ours += 1;
            } else {
                member.unit.set(NO_HANDLE);
            }
        } else if living {
            theirs += 1;
        } else {
            RAIDERS[slot - MAX_COMPANY].set(NO_HANDLE);
        }
    }
    (ours, theirs, health)
}

/// Moves every unit toward the middle of the other side: in reach is not
/// always able to hit, over a cliff or behind a building. The next looks
/// send them at their targets again.
fn close_in() {
    let (mut sums, mut counts) = ([(0, 0); 2], [0; 2]);
    for slot in 0..FIGHTERS {
        if let Some(cell) = fighter(slot).and_then(world::cell_of) {
            let side = usize::from(slot >= MAX_COMPANY);
            sums[side].0 += i32::from(cell.x());
            sums[side].1 += i32::from(cell.y());
            counts[side] += 1;
        }
    }
    for slot in 0..FIGHTERS {
        let Some(unit) = fighter(slot) else { continue };
        let side = usize::from(slot < MAX_COMPANY);
        if counts[side] > 0 {
            let (x, y) = (sums[side].0 / counts[side], sums[side].1 / counts[side]);
            let _ = Action::set_mission(unit, Mission::Move, Cell::at(x as u16, y as u16));
        }
    }
}

fn fighter(slot: usize) -> Option<Techno> {
    match COMPANY.get(slot) {
        Some(member) => member.unit(),
        None => RAIDERS
            .get(slot - MAX_COMPANY)
            .map(|raider| Techno(raider.get()))
            .filter(|unit| unit.0 != NO_HANDLE),
    }
}

// ----------------------------------------------------------------- Targets

/// What a weapon is for.
#[derive(Clone, Copy)]
enum Suits {
    Both,
    Foot,
    /// All but harmless to armor.
    FootOnly,
    Armor,
    /// All but harmless to foot.
    ArmorOnly,
}

struct Nature {
    id: &'static str,
    suits: Suits,
    /// Its weapon's reach in cells, rounded up.
    reach: i32,
}

const fn nature(id: &'static str, suits: Suits, reach: i32) -> Nature {
    Nature { id, suits, reach }
}

/// From the weapons' Range and Verses in `rulesmd.ini`. The first stands in
/// for a type that is not listed.
const NATURES: [Nature; 18] = [
    nature("", Suits::Both, 6),
    nature("E1", Suits::Foot, 4),
    nature("E2", Suits::Foot, 4),
    nature("INIT", Suits::Foot, 5),
    nature("FLAKT", Suits::Foot, 5),
    nature("HTK", Suits::Foot, 5),
    nature("YTNK", Suits::Foot, 6),
    nature("SNIPE", Suits::FootOnly, 14),
    nature("GHOST", Suits::FootOnly, 6),
    nature("SHK", Suits::Both, 3),
    nature("BRUTE", Suits::Both, 2),
    nature("TTNK", Suits::Both, 4),
    nature("SREF", Suits::Both, 10),
    nature("MTNK", Suits::Armor, 5),
    nature("LTNK", Suits::Armor, 5),
    nature("HTNK", Suits::Armor, 6),
    nature("APOC", Suits::Armor, 6),
    nature("TNKD", Suits::ArmorOnly, 5),
];

/// All in half cells.
const CROWDED: i32 = 3;
const CHANGE_OF_MIND: i32 = 6;
const BLOCKED: i32 = 40;

impl Suits {
    /// How much farther a target of the wrong sort looks.
    fn misfit(self, vehicle: bool) -> i32 {
        match (self, vehicle) {
            (Suits::Foot, true) => 20,
            (Suits::FootOnly, true) => 120,
            (Suits::Armor, false) => 16,
            (Suits::ArmorOnly, false) => 60,
            _ => 0,
        }
    }
}

fn enlist(slot: usize, unit: Techno, id: &str) {
    let kind = NATURES.iter().position(|known| known.id == id).unwrap_or(0);
    NATURE[slot].set(kind as i64);
    IS_VEHICLE[slot].set(i64::from(get(unit, Prop::Category) == Ok(1)));
}

#[derive(Clone, Copy)]
struct Foe {
    unit: Techno,
    x: i32,
    y: i32,
    vehicle: bool,
    /// How many of the looking side are on it.
    taken: i32,
}

/// Sees that each unit of one side fights what it best can: the nearest
/// foe, farther for a misfit or a crowded one.
fn look(company: bool) {
    let (own, other) = if company {
        (0..MAX_COMPANY, MAX_COMPANY..FIGHTERS)
    } else {
        (MAX_COMPANY..FIGHTERS, 0..MAX_COMPANY)
    };
    let nobody = Foe {
        unit: Techno(NO_HANDLE),
        x: 0,
        y: 0,
        vehicle: false,
        taken: 0,
    };
    let mut foes = [nobody; MAX_RAIDERS];
    let mut count = 0;
    for slot in other {
        let Some(unit) = fighter(slot) else { continue };
        let Some(cell) = world::cell_of(unit) else {
            continue;
        };
        if let Some(foe) = foes.get_mut(count) {
            *foe = Foe {
                unit,
                x: i32::from(cell.x()),
                y: i32::from(cell.y()),
                vehicle: IS_VEHICLE[slot].get() == 1,
                taken: 0,
            };
            count += 1;
        }
    }
    let foes = &mut foes[..count];
    if foes.is_empty() {
        return;
    }
    // Everyone's present target first, so that crowds are known.
    let mut present = [None; FIGHTERS];
    for slot in own.clone() {
        let Some(unit) = fighter(slot) else { continue };
        let target = unit.target().unwrap_or(NO_HANDLE);
        let at = foes.iter().position(|foe| foe.unit.0 == target);
        if let Some(foe) = at.and_then(|at| foes.get_mut(at)) {
            foe.taken += 1;
        }
        present[slot] = at;
    }
    for slot in own {
        let Some(unit) = fighter(slot) else { continue };
        let Some(cell) = world::cell_of(unit) else {
            continue;
        };
        let (x, y) = (i32::from(cell.x()), i32::from(cell.y()));
        let kind = &NATURES[(NATURE[slot].get() as usize).min(NATURES.len() - 1)];
        let present = present[slot];
        let mut stuck = 0;
        if let Some(foe) = present.and_then(|at| foes.get(at)) {
            // Out of reach and not under way: a lost order or no way there.
            if half_cells(x - foe.x, y - foe.y) > 2 * kind.reach + 3
                && get(unit, Prop::Moving) == Ok(0)
            {
                stuck = STUCK[slot].get() + 1;
            }
        }
        STUCK[slot].set(stuck);
        let mut best: Option<(i32, usize)> = None;
        let mut kept = i32::MAX;
        for (at, foe) in foes.iter().enumerate() {
            let mine = present == Some(at);
            let mut score = half_cells(x - foe.x, y - foe.y)
                + kind.suits.misfit(foe.vehicle)
                + CROWDED * (foe.taken - i32::from(mine));
            if mine {
                if stuck >= 2 {
                    score += BLOCKED;
                }
                kept = score;
            }
            if best.is_none_or(|(least, _)| score < least) {
                best = Some((score, at));
            }
        }
        let Some((score, mut chosen)) = best else {
            continue;
        };
        if let Some(at) = present {
            if kept <= score + CHANGE_OF_MIND {
                if stuck == 0 {
                    continue;
                }
                chosen = at; // The same order once more.
            }
            foes[at].taken -= 1;
        }
        foes[chosen].taken += 1;
        // A module's own order does not ask its own `CommandAllowed`.
        let _ = Action::set_mission_on(unit, Mission::Attack, foes[chosen].unit);
    }
}

/// A distance in half cells, close to the straight line.
fn half_cells(dx: i32, dy: i32) -> i32 {
    let (dx, dy) = (dx.abs(), dy.abs());
    2 * dx.max(dy) + dx.min(dy)
}

// ------------------------------------------------------------------ Ground

/// One of the eight ways out of camp, named as the screen shows it.
#[derive(Clone, Copy)]
struct Side {
    dx: i32,
    dy: i32,
    /// Steps to where the front rank appears, about 16 cells.
    steps: i32,
    name: &'static str,
}

const fn side(dx: i32, dy: i32, steps: i32, name: &'static str) -> Side {
    Side {
        dx,
        dy,
        steps,
        name,
    }
}

const SIDES: [Side; 8] = [
    side(0, 1, 16, FRONT_NAMES[0]),
    side(1, 0, 16, FRONT_NAMES[1]),
    side(0, -1, 16, FRONT_NAMES[2]),
    side(-1, 0, 16, FRONT_NAMES[3]),
    side(1, 1, 12, FRONT_NAMES[4]),
    side(1, -1, 12, FRONT_NAMES[5]),
    side(-1, 1, 12, FRONT_NAMES[6]),
    side(-1, -1, 12, FRONT_NAMES[7]),
];
/// Ranks behind the front, then ahead of it where the map ends.
const DEPTHS: [i32; 11] = [0, 1, 2, 3, 4, -1, -2, -3, -4, 5, 6];
const LATERALS: [i32; 11] = [0, -1, 1, -2, 2, -3, 3, -4, 4, -5, 5];
/// Cells tried per battle for each of the two passes of `spawn`.
const ATTEMPTS: usize = 400;

/// Cell diagonals kept clear of the map's edges (LocalSize leaves about so
/// many out of sight).
const EDGE_TOP: i32 = 10;
const EDGE_BOTTOM: i32 = 14;
const EDGE_SIDE: i32 = 5;
/// The ground known around the camp, this far either way.
const HALF: i32 = 20;
const SPAN: usize = 2 * HALF as usize + 1;
/// Bit `x` of row `y`: a vehicle gets from the camp to that cell over
/// terrain. Trees, walls and units are met by `CreateObject`'s refusals.
static REACHED: [Num; SPAN] = [const { Num::new(0) }; SPAN];
static SURVEYED: Num = Num::new(NO_HANDLE);
static FRONT: Num = Num::new(0);

/// The side the raiders will come from, for the muster's line.
pub fn front_name() -> &'static str {
    front().name
}

/// [`front`]'s index in `SIDES` and `FRONT_NAMES`, for the interface.
pub fn front_index() -> i64 {
    front();
    FRONT.get()
}

/// The company standing, the raiders standing and the raiders that came,
/// for the interface. Reads only: [`count_living`] forgets the dead.
pub fn standing() -> (i64, i64, i64) {
    if STAGE.get() == IDLE {
        return (0, 0, 0);
    }
    let (mut ours, mut theirs) = (0, 0);
    for slot in 0..FIGHTERS {
        let Some(unit) = fighter(slot) else { continue };
        if unit.health().unwrap_or(0) > 0 {
            if slot < MAX_COMPANY {
                ours += 1;
            } else {
                theirs += 1;
            }
        }
    }
    (ours, theirs, ARRIVED.get())
}

fn front() -> Side {
    let camp = state::camp();
    if SURVEYED.get() != camp.0 {
        SURVEYED.set(camp.0);
        survey(camp);
        let mut best = (0, -1);
        for (at, side) in SIDES.iter().enumerate() {
            let open = openness(*side);
            if open > best.1 {
                best = (at, open);
            }
        }
        FRONT.set(best.0 as i64);
    }
    SIDES[(FRONT.get() as usize).min(SIDES.len() - 1)]
}

/// Reads the ground around the camp and floods it from there.
fn survey(camp: Cell) {
    let west = i32::from(camp.x()) - HALF;
    let north = i32::from(camp.y()) - HALF;
    let corner = Cell::at(west.max(0) as u16, north.max(0) as u16);
    let mut open = inside(camp, west, north);
    // The closed cells are few, and each one found costs operations.
    let blocked = Find::cells()
        .within(corner, SPAN as i32, SPAN as i32)
        .where_prop(SpeedType::Track.passable(), Op::Eq, 0);
    rule_out(&mut open, blocked, west, north);
    let mut flood = [0u64; SPAN];
    // From the open cell nearest the camp.
    'seed: for ring in 0..=4 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                let (x, y) = (HALF + dx, (HALF + dy) as usize);
                if open[y] >> x & 1 == 1 {
                    flood[y] = 1 << x;
                    break 'seed;
                }
            }
        }
    }
    loop {
        let mut grown = false;
        // Down, then up: few rounds even round a bend.
        for step in 0..2 * SPAN {
            let y = if step < SPAN {
                step
            } else {
                2 * SPAN - 1 - step
            };
            let before = flood[y];
            let mut row = before;
            if y > 0 {
                row |= flood[y - 1];
            }
            if y + 1 < SPAN {
                row |= flood[y + 1];
            }
            row &= open[y];
            loop {
                let wider = (row | row << 1 | row >> 1) & open[y];
                if wider == row {
                    break;
                }
                row = wider;
            }
            if row != before {
                flood[y] = row;
                grown = true;
            }
        }
        if !grown {
            break;
        }
    }
    for (kept, row) in REACHED.iter().zip(flood) {
        kept.set(row as i64);
    }
}

fn reached(dx: i32, dy: i32) -> bool {
    let (x, y) = (dx + HALF, dy + HALF);
    (0..SPAN as i32).contains(&x)
        && REACHED
            .get(y as usize)
            .is_some_and(|row| row.get() as u64 >> x & 1 == 1)
}

/// The cells within the map, less the rows along its edge that the player
/// does not see. The map is a diamond: each edge is found straight across it
/// from the camp, in a few reads.
fn inside(camp: Cell, west: i32, north: i32) -> [u64; SPAN] {
    let (x, y) = (i32::from(camp.x()), i32::from(camp.y()));
    let steps_to_edge = |dx: i32, dy: i32| {
        let (mut on_map, mut beyond) = (0, 32);
        while beyond - on_map > 1 {
            let step = (on_map + beyond) / 2;
            let (x, y) = (x + dx * step, y + dy * step);
            if x >= 0 && y >= 0 && get(Cell::at(x as u16, y as u16), Prop::LandType).is_ok() {
                on_map = step;
            } else {
                beyond = step;
            }
        }
        on_map
    };
    // The screen's top, bottom, left and right, in cells each a diagonal.
    let top = x + y - (2 * steps_to_edge(-1, -1) - EDGE_TOP).max(0);
    let bottom = x + y + (2 * steps_to_edge(1, 1) - EDGE_BOTTOM).max(0);
    let left = x - y - (2 * steps_to_edge(-1, 1) - EDGE_SIDE).max(0);
    let right = x - y + (2 * steps_to_edge(1, -1) - EDGE_SIDE).max(0);
    let mut rows = [0u64; SPAN];
    for (at, row) in rows.iter_mut().enumerate() {
        let y = north + at as i32;
        let first = ((top - y).max(left + y) - west).max(-west).max(0);
        let last = ((bottom - y).min(right + y) - west).min(SPAN as i32 - 1);
        if y >= 0 && first <= last {
            *row = ((1u64 << (last + 1)) - 1) & !((1u64 << first) - 1);
        }
    }
    rows
}

/// Clears the cells `query` finds.
fn rule_out(open: &mut [u64; SPAN], mut query: Find, west: i32, north: i32) {
    let mut found = [0i64; 256];
    let mut after = 0;
    while let Ok((count, next)) = query.page(&mut found, after) {
        for &cell in found.iter().take(count) {
            let cell = Cell(cell);
            if let Some(row) = open.get_mut((i32::from(cell.y()) - north) as usize) {
                *row &= !(1 << ((i32::from(cell.x()) - west) as u32 & 63));
            }
        }
        if next == 0 {
            break;
        }
        after = next;
    }
}

/// From the camp, `out` steps toward `side` and `across` to the right of it.
fn along(side: Side, out: i32, across: i32) -> (i32, i32) {
    (
        side.dx * out - side.dy * across,
        side.dy * out + side.dx * across,
    )
}

/// How open the ground is where a wave would stand and on its way in.
fn openness(side: Side) -> i32 {
    let mut open = 0;
    // A sample of the cells: this runs for every side in one frame.
    for across in [-4, -2, 0, 2, 4] {
        for back in [0, 2, 4] {
            let (dx, dy) = along(side, side.steps + back, across);
            open += 6 * i32::from(reached(dx, dy));
        }
    }
    for across in [-2, 0, 2] {
        // As many samples for a diagonal as for a straight way in.
        for part in 1..=6 {
            let (dx, dy) = along(side, side.steps * part / 6, across);
            open += 4 * i32::from(reached(dx, dy));
        }
    }
    open
}

/// The wave's types, unit by unit.
fn line_up(wave: &'static waves::Wave) -> impl Iterator<Item = &'static str> {
    wave.squads
        .iter()
        .flat_map(|squad| core::iter::repeat_n(squad.id, squad.count))
        .take(MAX_RAIDERS)
}

/// Places the wave in ranks on the most open side. What does not fit goes
/// to the next most open one; failing all, to any cell away from the camp.
/// Returns the side most of it came from.
fn spawn(wave: &'static waves::Wave) -> Side {
    let first = front();
    let camp = state::camp();
    let enemy = state::enemy();
    let mut tried = [false; SIDES.len()];
    let mut raiders = line_up(wave).peekable();
    let mut side = first;
    let mut attempts = 0;
    let mut arrived = 0;
    let mut main = (0, first);
    'sides: for _ in 0..SIDES.len() {
        for (at, known) in SIDES.iter().enumerate() {
            if known.dx == side.dx && known.dy == side.dy {
                tried[at] = true;
            }
        }
        let before = arrived;
        'ranks: for back in DEPTHS {
            for across in LATERALS {
                let Some(&id) = raiders.peek() else {
                    break 'ranks;
                };
                let (dx, dy) = along(side, side.steps + back, across);
                if !reached(dx, dy) {
                    continue;
                }
                if attempts >= ATTEMPTS {
                    break 'sides;
                }
                attempts += 1;
                let Ok(ty) = TypeRef::techno_type(id) else {
                    raiders.next();
                    continue;
                };
                if let Ok(unit) = Action::create_object(world::offset(camp, dx, dy), ty, enemy) {
                    RAIDERS[arrived].set(unit.0);
                    enlist(MAX_COMPANY + arrived, unit, id);
                    arrived += 1;
                    raiders.next();
                }
            }
        }
        if arrived - before > main.0 {
            main = (arrived - before, side);
        }
        if raiders.peek().is_none() {
            break;
        }
        let mut next: Option<(i32, Side)> = None;
        for (at, known) in SIDES.iter().enumerate() {
            if !tried[at] {
                let open = openness(*known);
                if next.is_none_or(|(most, _)| open > most) {
                    next = Some((open, *known));
                }
            }
        }
        let Some((_, known)) = next else { break };
        side = known;
    }

    if arrived == 0 {
        // No reachable ground: rings around the camp, the farthest first.
        let mut raiders = line_up(wave).peekable();
        let mut attempts = 0;
        'rings: for ring in (4..=16).rev() {
            for step in 0..8 * ring {
                let Some(&id) = raiders.peek() else {
                    break 'rings;
                };
                let (along_edge, edge) = (step % (2 * ring) - ring, step / (2 * ring));
                let (dx, dy) = match edge {
                    0 => (along_edge, -ring),
                    1 => (ring, along_edge),
                    2 => (-along_edge, ring),
                    _ => (-ring, -along_edge),
                };
                if attempts >= ATTEMPTS {
                    break 'rings;
                }
                attempts += 1;
                let Ok(ty) = TypeRef::techno_type(id) else {
                    raiders.next();
                    continue;
                };
                if let Ok(unit) = Action::create_object(world::offset(camp, dx, dy), ty, enemy) {
                    RAIDERS[arrived].set(unit.0);
                    enlist(MAX_COMPANY + arrived, unit, id);
                    arrived += 1;
                    raiders.next();
                }
            }
        }
    }
    ARRIVED.set(arrived as i64);
    main.1
}
