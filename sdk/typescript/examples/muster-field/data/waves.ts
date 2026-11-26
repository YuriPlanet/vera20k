// The raiders of each wave. (Owner: battle)

export const WAVES: i32 = 8;

/** So many raiders of one TechnoType. */
export class Squad {
  constructor(public id: string, public count: i32) {}
}

export class Wave {
  constructor(
    public name: string,
    /** What to prepare for, in a few words. */
    public hint: string,
    /** The front rank first. */
    public squads: StaticArray<Squad>,
    /** What each raider killed pays. */
    public bounty: i32,
  ) {}

  size(): i32 {
    let size = 0;
    for (let at = 0; at < this.squads.length; at++) size += this.squads[at].count;
    return size;
  }
}

function squad(id: string, count: i32): Squad {
  return new Squad(id, count);
}

// No mind control, parasites, spawners, aircraft, chained blasts or disguises:
// a battle must be readable and must end. Each wave holds 1 to 24 raiders
// (`MAX_RAIDERS`).
const LIST: StaticArray<Wave> = [
  new Wave("探路散兵", "少量步兵", [squad("E2", 7), squad("INIT", 1)], 26),
  new Wave("人潮", "步兵成群，一辆轻坦压阵", [squad("LTNK", 1), squad("E2", 7), squad("E1", 1)], 33),
  new Wave("铁皮先锋", "轻坦开路，备反装甲", [squad("LTNK", 5), squad("E1", 3), squad("E2", 5)], 32),
  new Wave("混成纵队", "步车混编，火力密集", [squad("MTNK", 3), squad("HTK", 3), squad("INIT", 5), squad("FLAKT", 3)], 34),
  new Wave("远火阵地", "光棱与狙击，尽快贴近", [squad("E2", 7), squad("SHK", 5), squad("SREF", 3), squad("SNIPE", 4)], 30),
  new Wave("电锋突击", "磁暴与狂兽人贴身", [squad("TTNK", 4), squad("BRUTE", 8), squad("SHK", 7)], 35),
  new Wave("钢铁洪流", "重坦成列，反装甲为先", [
    squad("HTNK", 12), squad("TNKD", 2), squad("SHK", 4), squad("INIT", 4), squad("FLAKT", 2),
  ], 33),
  new Wave("最后总攻", "天启领军，步车齐上", [
    squad("APOC", 2), squad("HTNK", 6), squad("TTNK", 2), squad("SHK", 5), squad("SREF", 2), squad("SNIPE", 2), squad("E2", 2),
  ], 52),
];

/** Wave `number`, counted from 1. */
export function get(number: i64): Wave {
  const at = number - 1 < 0 ? 0 : number - 1 > <i64>(WAVES - 1) ? <i64>(WAVES - 1) : number - 1;
  return LIST[<i32>at];
}
