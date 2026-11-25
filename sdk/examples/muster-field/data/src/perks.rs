//! The supplies' names and how often each can be taken. What they do is
//! the simulation's (`perks` in `rules.wasm`). (Owner: content)

pub const PERKS: usize = 10;

pub const BARRAGE: usize = 0;
pub const MEDICS: usize = 1;
pub const LAST_STAND: usize = 2;
pub const LEDGER: usize = 3;
pub const DRILL: usize = 4;
pub const PLATES: usize = 5;
pub const BARRELS: usize = 6;
pub const HORN: usize = 7;
pub const GRUDGE: usize = 8;
pub const GUNS: usize = 9;

pub struct Perk {
    pub name: &'static str,
    /// For the first taking and for the next ones.
    pub effect: [&'static str; 2],
    pub most: i64,
}

#[rustfmt::skip]
pub const TABLE: [Perk; PERKS] = [
    Perk { name: "开场炮轰", effect: ["敌开战损血15%"; 2], most: 2 },
    Perk { name: "野战医疗", effect: ["每3秒回血4%"; 2], most: 2 },
    Perk { name: "背水一战", effect: ["独存回满且翻倍"; 2], most: 1 },
    Perk { name: "战利账房", effect: ["每场+100杀+15"; 2], most: 2 },
    Perk { name: "老兵教头", effect: ["低价兵成老兵", "中价兵也成老兵"], most: 2 },
    Perk { name: "焊接钢板", effect: ["载具护甲+40%"; 2], most: 2 },
    Perk { name: "加长枪管", effect: ["射程+1格"; 2], most: 2 },
    Perk { name: "冲锋号角", effect: ["短程前20秒护甲+60%"; 2], most: 2 },
    Perk { name: "同袍血债", effect: ["每亡一人火力+20%"; 2], most: 2 },
    Perk { name: "后方炮援", effect: ["每6秒轰最强敌"; 2], most: 2 },
];

pub fn name(perk: usize) -> &'static str {
    TABLE.get(perk).map_or("", |entry| entry.name)
}

/// What taking `perk` adds once it was taken `taken` times.
pub fn effect(perk: usize, taken: i64) -> &'static str {
    TABLE
        .get(perk)
        .map_or("", |entry| entry.effect[usize::from(taken > 0)])
}
