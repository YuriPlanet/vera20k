//! Company-wide supplies and what a champion is worth. (Owner: content)
//!
//! Stacks last the campaign. What they do to units lives one battle: units
//! are fresh each muster, and each effect has a modifier group of its own,
//! so applying one again replaces it.

use vera20k_sdk::prelude::*;
use vera20k_sdk::{get, set};

use crate::content::{self, MERCS};
use crate::state::{self, COMPANY, Member, Num};
use crate::world;

pub use muster_field_data::perks::PERKS;
use muster_field_data::perks::{
    BARRAGE, BARRELS, DRILL, GRUDGE, GUNS, HORN, LAST_STAND, LEDGER, MEDICS, PLATES, TABLE,
};

const SECOND: i64 = 15;
/// How long the horn holds: the approach and the first exchange.
const HORN_FRAMES: i32 = 20 * SECOND as i32;
const HEAL_EVERY: i64 = 3 * SECOND;
const SHELL_EVERY: i64 = 6 * SECOND;

/// One modifier group per effect and property: a group holds one modifier
/// on an object, whatever its property.
#[derive(Clone, Copy)]
enum Group {
    ChampionFire = 4101,
    ChampionArmor,
    Plates,
    Barrels,
    Horn,
    GrudgeFire,
    LastFire,
    LastArmor,
}

static TAKEN: [Num; PERKS] = [const { Num::new(0) }; PERKS];
/// This battle: its first frame, who was fielded, how many of them fell and
/// whether the last of them made the stand.
static STARTED: Num = Num::new(0);
static FIELDED: Num = Num::new(0);
static FALLEN: Num = Num::new(0);
static STOOD: Num = Num::new(0);

fn taken(perk: usize) -> i64 {
    TAKEN
        .get(perk)
        .zip(TABLE.get(perk))
        .map_or(0, |(taken, entry)| taken.get().clamp(0, entry.most))
}

pub fn name(perk: usize) -> &'static str {
    muster_field_data::perks::name(perk)
}

/// A few words for the choice line: what taking it now adds.
pub fn effect(perk: usize) -> &'static str {
    muster_field_data::perks::effect(perk, taken(perk))
}

/// How often each supply was taken, for the interface.
pub fn taken_all() -> [i64; PERKS] {
    let mut all = [0; PERKS];
    for (perk, count) in all.iter_mut().enumerate() {
        *count = taken(perk);
    }
    all
}

/// Up to three different supplies that can still be taken.
pub fn choices() -> [Option<usize>; 3] {
    let mut open = [0usize; PERKS];
    let mut count = 0;
    for (perk, entry) in TABLE.iter().enumerate() {
        if taken(perk) < entry.most {
            open[count] = perk;
            count += 1;
        }
    }
    let mut drawn = [None; 3];
    for (at, slot) in drawn.iter_mut().enumerate().take(count) {
        let pick = (random(at as i32, count as i32 - 1).max(0) as usize)
            .min(count - 1)
            .max(at);
        open.swap(at, pick);
        *slot = Some(open[at]);
    }
    drawn
}

pub fn grant(perk: usize) {
    let (Some(entry), Some(slot)) = (TABLE.get(perk), TAKEN.get(perk)) else {
        return;
    };
    if slot.get() >= entry.most {
        return;
    }
    slot.add(1);
}

/// The rank a member's fresh unit gets: 0 rookie, 1 veteran, 2 elite.
pub fn rank(member: &Member) -> i64 {
    let Some(merc) = merc(member) else {
        return 0;
    };
    if member.is_champion() {
        merc.champion.rank
    } else {
        i64::from((merc.tier as i64) < taken(DRILL))
    }
}

fn merc(member: &Member) -> Option<&'static content::Merc> {
    MERCS.get(member.kind()).filter(|_| !member.is_empty())
}

fn fielded() -> impl Iterator<Item = (&'static Member, Techno)> {
    COMPANY
        .iter()
        .filter(|member| !member.is_empty())
        .filter_map(|member| member.unit().map(|unit| (member, unit)))
        .filter(|&(_, unit)| world::alive(unit))
}

fn promote(member: &Member, unit: Techno) {
    let due = rank(member);
    if get(unit, Prop::Rank).is_ok_and(|rank| rank < due) {
        let _ = set(unit, Prop::Rank, due);
    }
}

/// Multiplies `prop` by `percent` until the unit is gone.
fn scale(unit: Techno, prop: Prop, percent: i64, group: Group) {
    scale_for(unit, prop, percent, -1, group);
}

fn scale_for(unit: Techno, prop: Prop, percent: i64, frames: i32, group: Group) {
    if percent != 100 {
        let _ = Modifier::new(
            unit,
            prop,
            ModifierOp::Multiply,
            percent * FIXED_ONE / 100,
            frames,
            0,
            group as i32,
        );
    }
}

fn spark(unit: Techno, anim: &str) {
    if let Ok(anim) = anim_id(anim) {
        let _ = Action::attach_anim(unit, anim);
    }
}

fn raiders() -> impl Iterator<Item = Techno> {
    state::RAIDERS
        .iter()
        .map(|slot| Techno(slot.get()))
        .filter(|unit| !unit.is_none())
}

/// After the raiders are placed and before anyone is ordered.
pub fn battle_started() {
    STARTED.set(i64::from(frame()));
    FALLEN.set(0);
    STOOD.set(0);
    let mut count = 0;
    for (member, unit) in fielded() {
        count += 1;
        outfit(member, unit);
    }
    FIELDED.set(count);
    barrage();
}

fn outfit(member: &Member, unit: Techno) {
    let Some(merc) = merc(member) else {
        return;
    };
    promote(member, unit);
    if member.is_champion() {
        let content::Champion { fire, armor, .. } = merc.champion;
        scale(unit, Prop::FirepowerMultiplier, fire, Group::ChampionFire);
        scale(unit, Prop::ArmorMultiplier, armor, Group::ChampionArmor);
    }
    if merc.tags & content::VEHICLE != 0 {
        let percent = 100 + 40 * taken(PLATES);
        scale(unit, Prop::ArmorMultiplier, percent, Group::Plates);
    }
    if merc.tags & content::ASSAULT != 0 {
        let percent = 100 + 60 * taken(HORN);
        scale_for(
            unit,
            Prop::ArmorMultiplier,
            percent,
            HORN_FRAMES,
            Group::Horn,
        );
    }
    let cells = taken(BARRELS);
    if cells > 0 {
        let _ = Modifier::new(
            unit,
            Prop::WeaponRangeExtra,
            ModifierOp::Add,
            256 * cells,
            -1,
            0,
            Group::Barrels as i32,
        );
    }
}

fn barrage() {
    let left = 100 - 15 * taken(BARRAGE);
    if left == 100 {
        return;
    }
    for unit in raiders() {
        let (Ok(full), Ok(now)) = (get(unit, Prop::Strength), unit.health()) else {
            continue;
        };
        let to = (full * left / 100).max(1);
        if to < now && unit.set_health(to).is_ok() {
            spark(unit, "EXPLOSML");
        }
    }
}

/// Every frame of a battle: work once a second.
pub fn battle_tick(now: i32) {
    let since = i64::from(now) - STARTED.get();
    if since <= 0 || since % SECOND != 0 {
        return;
    }
    let (medics, guns) = (taken(MEDICS), taken(GUNS));
    let watched = taken(GRUDGE) > 0 || (taken(LAST_STAND) > 0 && STOOD.get() == 0);
    let heals = medics > 0 && since % HEAL_EVERY == 0;
    if watched || heals {
        let mut living = 0;
        let mut last = None;
        for (_, unit) in fielded() {
            living += 1;
            last = Some(unit);
            if heals {
                heal(unit, 4 * medics);
            }
        }
        if watched {
            fell(living, last);
        }
    }
    if guns > 0 && since % SHELL_EVERY == 0 {
        shell(guns);
    }
}

fn heal(unit: Techno, percent: i64) {
    let (Ok(full), Ok(now)) = (get(unit, Prop::Strength), unit.health()) else {
        return;
    };
    if now < full {
        let amount = (full * percent / 100).clamp(1, i64::from(i32::MAX));
        let _ = Action::heal(unit, amount as i32);
    }
}

fn fell(living: i64, last: Option<Techno>) {
    let fallen = (FIELDED.get() - living).max(0);
    if fallen > FALLEN.get() {
        FALLEN.set(fallen);
        let percent = 100 + 20 * taken(GRUDGE) * fallen;
        for (_, unit) in fielded() {
            scale(unit, Prop::FirepowerMultiplier, percent, Group::GrudgeFire);
        }
    }
    // Someone has to have fallen: a company of one makes no stand.
    let Some(unit) = last.filter(|_| living == 1 && fallen > 0) else {
        return;
    };
    if taken(LAST_STAND) > 0 && STOOD.get() == 0 {
        STOOD.set(1);
        if let Ok(full) = get(unit, Prop::Strength) {
            let _ = unit.set_health(full);
        }
        scale(unit, Prop::FirepowerMultiplier, 200, Group::LastFire);
        scale(unit, Prop::ArmorMultiplier, 200, Group::LastArmor);
        spark(unit, "RING1");
    }
}

/// A shell on the raider with the most health, through armor of every kind.
fn shell(guns: i64) {
    let target = raiders()
        .filter_map(|unit| unit.health().ok().filter(|&hp| hp > 0).map(|hp| (hp, unit)))
        .max_by_key(|&(hp, _)| hp);
    let (Some((_, unit)), Ok(warhead)) = (target, TypeRef::warhead("Super")) else {
        return;
    };
    spark(unit, "EXPLOMED");
    let _ = Action::damage(unit, (100 * guns) as i32, warhead);
}

/// Extra pay for a finished wave, won or lost.
pub fn bonus_pay(kills: i64, _won: bool) -> i32 {
    let kills = kills.clamp(0, state::MAX_RAIDERS as i64);
    (taken(LEDGER) * (100 + 15 * kills)) as i32
}
