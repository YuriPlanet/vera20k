//! The state the parts share. Module memory is saved and hashed with the
//! match, so plain statics do.

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::NO_HANDLE;
use vera20k_sdk::prelude::*;

/// A number in module memory.
pub struct Num(AtomicI64);

impl Num {
    pub const fn new(value: i64) -> Self {
        Self(AtomicI64::new(value))
    }

    pub fn get(&self) -> i64 {
        self.0.load(Relaxed)
    }

    pub fn set(&self, value: i64) {
        self.0.store(value, Relaxed);
    }

    pub fn add(&self, value: i64) {
        self.set(self.get() + value);
    }
}

pub const WAITING: i64 = 0;
pub const MUSTER: i64 = 1;
pub const BATTLE: i64 = 2;
pub const OVER: i64 = 3;

pub static PHASE: Num = Num::new(WAITING);
static PLAYER: Num = Num::new(NO_HANDLE);
static ENEMY: Num = Num::new(NO_HANDLE);
static CAMP: Num = Num::new(NO_HANDLE);

pub fn player() -> House {
    House(PLAYER.get())
}

pub fn enemy() -> House {
    House(ENEMY.get())
}

/// The middle cell of the offer row; everything is placed from it.
pub fn camp() -> Cell {
    Cell(CAMP.get())
}

pub fn set_sides(player: House, enemy: House, camp: Cell) {
    PLAYER.set(player.0);
    ENEMY.set(enemy.0);
    CAMP.set(camp.0);
}

/// The wave the player faces next, from 1. `campaign` writes it.
pub static WAVE: Num = Num::new(1);
/// Banners left; none is the end. `campaign` writes it.
pub static BANNERS: Num = Num::new(0);
/// The camp's rank, 0 to 2. `shop` writes it.
pub static CAMP_RANK: Num = Num::new(0);
/// Supply picks the player has yet to make. `campaign` adds for waves,
/// `shop` for merges and takes them away.
pub static PICKS: Num = Num::new(0);

pub use muster_field_data::{MAX_COMPANY, MAX_OFFERS, MAX_RAIDERS};

/// One hired mercenary. It outlives its unit: the dead return next muster.
pub struct Member {
    /// Index in `content::MERCS`; -1 for an empty place.
    pub kind: Num,
    /// 1 for a champion.
    pub champion: Num,
    /// Its place in the line, a cell reference.
    pub home: Num,
    /// Its unit on the field, or `NO_HANDLE`.
    pub unit: Num,
}

impl Member {
    const EMPTY: Member = Member {
        kind: Num::new(-1),
        champion: Num::new(0),
        home: Num::new(NO_HANDLE),
        unit: Num::new(NO_HANDLE),
    };

    pub fn is_empty(&self) -> bool {
        self.kind.get() < 0
    }

    pub fn is_champion(&self) -> bool {
        self.champion.get() == 1
    }

    pub fn kind(&self) -> usize {
        self.kind.get().max(0) as usize
    }

    pub fn unit(&self) -> Option<Techno> {
        Some(Techno(self.unit.get())).filter(|unit| unit.0 != NO_HANDLE)
    }

    pub fn clear(&self) {
        self.kind.set(-1);
        self.champion.set(0);
        self.home.set(NO_HANDLE);
        self.unit.set(NO_HANDLE);
    }
}

pub static COMPANY: [Member; MAX_COMPANY] = [Member::EMPTY; MAX_COMPANY];
/// The raiders on the field, or `NO_HANDLE`. `battle` writes it.
pub static RAIDERS: [Num; MAX_RAIDERS] = [const { Num::new(NO_HANDLE) }; MAX_RAIDERS];
