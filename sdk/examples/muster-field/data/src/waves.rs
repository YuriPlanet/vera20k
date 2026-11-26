//! The raiders of each wave. (Owner: battle)

pub const WAVES: usize = 8;

/// So many raiders of one TechnoType.
pub struct Squad {
    pub id: &'static str,
    pub count: usize,
}

pub struct Wave {
    pub name: &'static str,
    /// What to prepare for, in a few words.
    pub hint: &'static str,
    /// The front rank first.
    pub squads: &'static [Squad],
    /// What each raider killed pays.
    pub bounty: i32,
}

impl Wave {
    pub fn size(&self) -> usize {
        self.squads.iter().map(|squad| squad.count).sum()
    }
}

const fn squad(id: &'static str, count: usize) -> Squad {
    Squad { id, count }
}

// No mind control, parasites, spawners, aircraft, chained blasts or disguises:
// a battle must be readable and must end.
const LIST: [Wave; WAVES] = [
    Wave {
        name: "探路散兵",
        hint: "少量步兵",
        squads: &[squad("E2", 7), squad("INIT", 1)],
        bounty: 26,
    },
    Wave {
        name: "人潮",
        hint: "步兵成群，一辆轻坦压阵",
        squads: &[squad("LTNK", 1), squad("E2", 7), squad("E1", 1)],
        bounty: 33,
    },
    Wave {
        name: "铁皮先锋",
        hint: "轻坦开路，备反装甲",
        squads: &[squad("LTNK", 5), squad("E1", 3), squad("E2", 5)],
        bounty: 32,
    },
    Wave {
        name: "混成纵队",
        hint: "步车混编，火力密集",
        squads: &[
            squad("MTNK", 3),
            squad("HTK", 3),
            squad("INIT", 5),
            squad("FLAKT", 3),
        ],
        bounty: 34,
    },
    Wave {
        name: "远火阵地",
        hint: "光棱与狙击，尽快贴近",
        squads: &[
            squad("E2", 7),
            squad("SHK", 5),
            squad("SREF", 3),
            squad("SNIPE", 4),
        ],
        bounty: 30,
    },
    Wave {
        name: "电锋突击",
        hint: "磁暴与狂兽人贴身",
        squads: &[squad("TTNK", 4), squad("BRUTE", 8), squad("SHK", 7)],
        bounty: 35,
    },
    Wave {
        name: "钢铁洪流",
        hint: "重坦成列，反装甲为先",
        squads: &[
            squad("HTNK", 12),
            squad("TNKD", 2),
            squad("SHK", 4),
            squad("INIT", 4),
            squad("FLAKT", 2),
        ],
        bounty: 33,
    },
    Wave {
        name: "最后总攻",
        hint: "天启领军，步车齐上",
        squads: &[
            squad("APOC", 2),
            squad("HTNK", 6),
            squad("TTNK", 2),
            squad("SHK", 5),
            squad("SREF", 2),
            squad("SNIPE", 2),
            squad("E2", 2),
        ],
        bounty: 52,
    },
];

const _: () = {
    let mut wave = 0;
    while wave < WAVES {
        let squads = LIST[wave].squads;
        let mut size = 0;
        let mut at = 0;
        while at < squads.len() {
            size += squads[at].count;
            at += 1;
        }
        assert!(size > 0 && size <= crate::MAX_RAIDERS);
        wave += 1;
    }
};

/// Wave `number`, counted from 1.
pub fn get(number: i64) -> &'static Wave {
    &LIST[(number - 1).clamp(0, WAVES as i64 - 1) as usize]
}
