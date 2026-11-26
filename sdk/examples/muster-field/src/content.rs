//! The mercenaries. (Owner: content)
//!
//! Each was measured in this engine against the others at equal budgets,
//! under both owners; the prices follow those battles. None beats all the
//! others: foot loses to the flak track and the Gattling tank, those to
//! guns, guns to shock troopers and brutes, and those two to commandos.

/// What a supply can apply to; a mercenary has one or more.
pub const FOOT: u32 = 1 << 0;
pub const VEHICLE: u32 = 1 << 1;
/// Fights from a distance.
pub const RANGED: u32 = 1 << 2;
/// Closes in: a weapon range of four cells or less.
pub const ASSAULT: u32 = 1 << 3;

/// What stands in for three members. `fire` and `armor` are percent on top
/// of the rank, tuned per type: elite rank changes most types' weapons.
#[derive(Clone, Copy)]
pub struct Champion {
    pub rank: i64,
    pub fire: i64,
    pub armor: i64,
}

const fn elite(fire: i64, armor: i64) -> Champion {
    Champion {
        rank: 2,
        fire,
        armor,
    }
}

/// For the types whose elite weapons alone would clear a wave.
const fn veteran(fire: i64, armor: i64) -> Champion {
    Champion {
        rank: 1,
        fire,
        armor,
    }
}

pub struct Merc {
    /// The TechnoType.
    pub id: &'static str,
    /// At most five Han characters: the row's line holds five of them.
    pub name: &'static str,
    pub price: i32,
    /// 0 to 2; `shop` draws by camp rank.
    pub tier: usize,
    pub tags: u32,
    pub champion: Champion,
}

#[rustfmt::skip]
pub const MERCS: [Merc; 15] = [
    Merc { id: "E2", name: "动员兵", price: 130, tier: 0, tags: FOOT | ASSAULT, champion: elite(110, 125) },
    Merc { id: "E1", name: "美国大兵", price: 135, tier: 0, tags: FOOT | ASSAULT, champion: elite(105, 120) },
    Merc { id: "FLAKT", name: "防空步兵", price: 160, tier: 0, tags: FOOT, champion: elite(115, 170) },
    Merc { id: "INIT", name: "尤里新兵", price: 190, tier: 0, tags: FOOT, champion: elite(110, 135) },
    Merc { id: "SHK", name: "磁暴步兵", price: 280, tier: 0, tags: FOOT | ASSAULT, champion: elite(72, 85) },
    Merc { id: "SNIPE", name: "狙击手", price: 340, tier: 1, tags: FOOT | RANGED, champion: elite(100, 150) },
    Merc { id: "BRUTE", name: "狂兽人", price: 420, tier: 1, tags: FOOT | ASSAULT, champion: elite(115, 175) },
    Merc { id: "HTK", name: "防空履带车", price: 480, tier: 1, tags: VEHICLE, champion: elite(115, 145) },
    Merc { id: "YTNK", name: "盖特坦克", price: 590, tier: 1, tags: VEHICLE, champion: elite(130, 152) },
    Merc { id: "MTNK", name: "灰熊坦克", price: 620, tier: 1, tags: VEHICLE, champion: elite(70, 85) },
    Merc { id: "GHOST", name: "海豹突击队", price: 640, tier: 2, tags: FOOT, champion: elite(100, 260) },
    Merc { id: "TNKD", name: "坦克杀手", price: 780, tier: 2, tags: VEHICLE, champion: elite(120, 175) },
    Merc { id: "SREF", name: "光棱坦克", price: 900, tier: 2, tags: VEHICLE | RANGED, champion: veteran(170, 215) },
    Merc { id: "TTNK", name: "磁能坦克", price: 1120, tier: 2, tags: VEHICLE | ASSAULT, champion: veteran(112, 116) },
    Merc { id: "APOC", name: "天启坦克", price: 1300, tier: 2, tags: VEHICLE, champion: veteran(92, 119) },
];
