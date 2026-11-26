//! Between `rules.wasm` and `ui.wasm` (RFC 0001 section 10).
//!
//! The simulation publishes the camp as private house variables of the
//! player's house, written when they change; the interface reads them each
//! render frame. Variables are saved with the match, so an interface that
//! starts again after a load sees the camp at once.
//!
//! The interface sends two-byte messages, `[what, index]`. The simulation
//! turns each into the order a player would give a camp unit, so a button
//! and the order do the same thing (`shop::ui_order`).

/// `state::PHASE`: 0 waiting, 1 muster, 2 battle, 3 over.
pub const PHASE: &str = "MfPhase";
pub const WAVE: &str = "MfWave";
pub const BANNERS: &str = "MfBanners";
pub const RANK: &str = "MfRank";
pub const PICKS: &str = "MfPicks";
/// [`FLAG_OFFICER`], [`FLAG_TRUCKS`].
pub const FLAGS: &str = "MfFlags";
/// A byte per offer ([`Offer`]), offer 0 in the low byte.
pub const OFFERS: &str = "MfOffers";
/// A byte per supply truck: the supply plus one, 0 for none.
pub const CHOICES: &str = "MfChoices";
/// A byte per member ([`Member`]): members 0..5, then 5..10.
pub const COMPANY_LOW: &str = "MfCompanyLow";
pub const COMPANY_HIGH: &str = "MfCompanyHigh";
/// Four bits per supply: how often it was taken.
pub const TAKEN: &str = "MfTaken";
/// The battle: company standing, raiders standing, raiders arrived, 16 bits
/// each.
pub const BATTLE: &str = "MfBattle";
/// The front's index in [`crate::FRONT_NAMES`].
pub const FRONT: &str = "MfFront";
/// The last battle ([`Last`]); 0 before the first.
pub const LAST: &str = "MfLast";

/// The officer stands in camp: improve and redraw work.
pub const FLAG_OFFICER: i64 = 1;
/// The supply trucks stand: a pick works.
pub const FLAG_TRUCKS: i64 = 2;

pub const MSG_HIRE: u8 = 1;
pub const MSG_KEEP: u8 = 2;
pub const MSG_REDRAW: u8 = 3;
pub const MSG_IMPROVE: u8 = 4;
pub const MSG_START: u8 = 5;
pub const MSG_PICK: u8 = 6;
pub const MSG_DISMISS: u8 = 7;

/// Byte `at` of `packed`.
pub const fn byte(packed: i64, at: usize) -> u8 {
    if at >= 8 {
        return 0;
    }
    (packed >> (8 * at)) as u8
}

/// `packed` with byte `at` set to `value`.
pub const fn with_byte(packed: i64, at: usize, value: u8) -> i64 {
    if at >= 8 {
        return packed;
    }
    let shift = 8 * at;
    (packed & !(0xff << shift)) | ((value as i64) << shift)
}

const KIND: u8 = 0x1f;
const BIT_A: u8 = 1 << 5;
const BIT_B: u8 = 1 << 6;

/// A place in the row.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Offer {
    /// Index in `content::MERCS`.
    pub kind: Option<usize>,
    pub kept: bool,
    /// Its unit stands in the row (a kept offer may wait for room).
    pub standing: bool,
}

impl Offer {
    pub const fn pack(self) -> u8 {
        let kind = match self.kind {
            Some(kind) if kind < KIND as usize => kind as u8 + 1,
            _ => return 0,
        };
        kind | if self.kept { BIT_A } else { 0 } | if self.standing { BIT_B } else { 0 }
    }

    pub const fn unpack(byte: u8) -> Self {
        let kind = byte & KIND;
        Self {
            kind: if kind == 0 { None } else { Some(kind as usize - 1) },
            kept: byte & BIT_A != 0,
            standing: byte & BIT_B != 0,
        }
    }
}

/// A place in the company.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Member {
    pub kind: Option<usize>,
    pub champion: bool,
    /// Its unit is on the field.
    pub standing: bool,
}

impl Member {
    pub const fn pack(self) -> u8 {
        Offer {
            kind: self.kind,
            kept: self.champion,
            standing: self.standing,
        }
        .pack()
    }

    pub const fn unpack(byte: u8) -> Self {
        let offer = Offer::unpack(byte);
        Self {
            kind: offer.kind,
            champion: offer.kept,
            standing: offer.standing,
        }
    }
}

/// How the last battle went.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Last {
    /// 0 none yet, 1 won, 2 lost.
    pub result: u8,
    pub supply: bool,
    pub kills: i64,
    pub paid: i64,
}

impl Last {
    pub const fn pack(self) -> i64 {
        (self.result as i64 & 3)
            | if self.supply { 4 } else { 0 }
            | (self.kills & 0xffff) << 8
            | (self.paid & 0xff_ffff) << 24
    }

    pub const fn unpack(packed: i64) -> Self {
        Self {
            result: (packed & 3) as u8,
            supply: packed & 4 != 0,
            kills: (packed >> 8) & 0xffff,
            paid: (packed >> 24) & 0xff_ffff,
        }
    }
}

/// Three counts of 16 bits.
pub const fn battle(ours: i64, theirs: i64, arrived: i64) -> i64 {
    (ours & 0xffff) | (theirs & 0xffff) << 16 | (arrived & 0xffff) << 32
}

pub const fn unbattle(packed: i64) -> (i64, i64, i64) {
    (packed & 0xffff, (packed >> 16) & 0xffff, (packed >> 32) & 0xffff)
}

/// Four bits per supply.
pub const fn taken(packed: i64, perk: usize) -> i64 {
    if perk >= 16 {
        return 0;
    }
    (packed >> (4 * perk)) & 0xf
}

pub const fn with_taken(packed: i64, perk: usize, count: i64) -> i64 {
    if perk >= 16 {
        return packed;
    }
    let shift = 4 * perk;
    (packed & !(0xf << shift)) | ((count & 0xf) << shift)
}
