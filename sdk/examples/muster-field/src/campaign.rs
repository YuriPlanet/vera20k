//! The waves as a campaign: pay, banners and the end. (Owner: battle)

use crate::battle::{self, Report};
use crate::state::{BANNERS, Num, PICKS, WAVE};
use crate::text::{Line, Say};
use crate::waves::{self, WAVES};
use crate::{perks, world};
use muster_field_data::proto::Last;

const PURSE: i64 = 1600;
const START_BANNERS: i64 = 3;
const WAGE: i64 = 420;
/// Added to the wage per wave.
const WAGE_STEP: i64 = 115;

const NOTHING: i64 = 0;
const WON: i64 = 1;
const LOST: i64 = 2;
/// What [`settle`] leaves for the next muster's one line.
static RESULT: Num = Num::new(NOTHING);
static KILLS: Num = Num::new(0);
static PAID: Num = Num::new(0);
static SUPPLY: Num = Num::new(0);
/// The last battle as `proto::Last` packs it; unlike [`RESULT`] it stays.
static LAST: Num = Num::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Muster,
    Victory,
    Defeat,
}

/// The purse and the banners of a new campaign.
pub fn begin() {
    WAVE.set(1);
    BANNERS.set(START_BANNERS);
    PICKS.set(0);
    RESULT.set(NOTHING);
    LAST.set(0);
    let change = (PURSE - world::credits()).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    let _ = world::pay(change as i32);
}

/// Pays for the battle, win or lose. It prints only when the campaign is
/// over: otherwise the muster follows in the same frame, whose one campaign
/// line is [`announce`]'s.
pub fn settle(report: &Report) -> Next {
    let number = WAVE.get().clamp(1, WAVES as i64);
    let wave = waves::get(number);
    let kills = report.kills.clamp(0, wave.size() as i64);
    let wage = WAGE + WAGE_STEP * number;
    let bounty = kills * i64::from(wave.bounty);
    let bonus = i64::from(perks::bonus_pay(kills, report.won));
    let due = (wage + bounty + bonus).clamp(0, i64::from(i32::MAX));
    let paid = if world::pay(due as i32) { due } else { 0 };
    RESULT.set(if report.won { WON } else { LOST });
    KILLS.set(kills);
    PAID.set(paid);
    SUPPLY.set(0);

    let next = if !report.won {
        let left = (BANNERS.get() - 1).max(0);
        BANNERS.set(left);
        if left == 0 {
            Next::Defeat
        } else {
            Next::Muster
        }
    } else if number >= WAVES as i64 {
        Next::Victory
    } else {
        WAVE.set(number + 1);
        if matches!(number, 2 | 4 | 6) {
            PICKS.add(1);
            SUPPLY.set(1);
        }
        Next::Muster
    };
    LAST.set(
        Last {
            result: if report.won { 1 } else { 2 },
            supply: SUPPLY.get() == 1,
            kills,
            paid,
        }
        .pack(),
    );
    if next != Next::Muster {
        outcome(Line::new()).log();
        RESULT.set(NOTHING);
    }
    next
}

/// The last battle in a few words, if there was one.
fn outcome(line: Line) -> Line {
    let word = match RESULT.get() {
        WON => "胜，歼",
        LOST => "败，歼",
        _ => return line,
    };
    let line = line
        .text(word)
        .num(KILLS.get())
        .text("，得")
        .num(PAID.get());
    if SUPPLY.get() == 1 {
        line.text("＋补给。")
    } else {
        line.text("。")
    }
}

/// The one campaign line of a muster: how the last battle went, then the
/// wave, where it comes from, what it is and the banners.
pub fn announce() {
    let number = WAVE.get();
    let wave = waves::get(number);
    outcome(Line::new())
        .text("第")
        .num(number)
        .text("/")
        .num(WAVES as i64)
        .text("波【")
        .text(wave.name)
        .text("】自")
        .text(battle::front_name())
        .text("来：")
        .text(wave.hint)
        .text("。军旗")
        .num(BANNERS.get())
        .log();
    RESULT.set(NOTHING);
}

/// The last battle, packed, for the interface.
pub fn last() -> i64 {
    LAST.get()
}
