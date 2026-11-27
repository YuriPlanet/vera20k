// The supplies' names and how often each can be taken. What they do is
// the simulation's (`perks` in `rules.wasm`). (Owner: content)

export const PERKS: i32 = 10;

export const BARRAGE: i32 = 0;
export const MEDICS: i32 = 1;
export const LAST_STAND: i32 = 2;
export const LEDGER: i32 = 3;
export const DRILL: i32 = 4;
export const PLATES: i32 = 5;
export const BARRELS: i32 = 6;
export const HORN: i32 = 7;
export const GRUDGE: i32 = 8;
export const GUNS: i32 = 9;

export class Perk {
  constructor(
    public name: string,
    /** For the first taking. */
    public first: string,
    /** For the next ones. */
    public again: string,
    public most: i64,
  ) {}
}

export const TABLE: StaticArray<Perk> = [
  new Perk("开场炮轰", "敌开战损血15%", "敌开战损血15%", 2),
  new Perk("野战医疗", "每3秒回血4%", "每3秒回血4%", 2),
  new Perk("背水一战", "独存回满且翻倍", "独存回满且翻倍", 1),
  new Perk("战利账房", "每场+100杀+15", "每场+100杀+15", 2),
  new Perk("老兵教头", "低价兵成老兵", "中价兵也成老兵", 2),
  new Perk("焊接钢板", "载具护甲+40%", "载具护甲+40%", 2),
  new Perk("加长枪管", "射程+1格", "射程+1格", 2),
  new Perk("冲锋号角", "短程前20秒护甲+60%", "短程前20秒护甲+60%", 2),
  new Perk("同袍血债", "每亡一人火力+20%", "每亡一人火力+20%", 2),
  new Perk("后方炮援", "每6秒轰最强敌", "每6秒轰最强敌", 2),
];

export function name(perk: i32): string {
  return perk >= 0 && perk < TABLE.length ? TABLE[perk].name : "";
}

/** What taking `perk` adds once it was taken `taken` times. */
export function effect(perk: i32, taken: i64): string {
  if (perk < 0 || perk >= TABLE.length) return "";
  const entry = TABLE[perk];
  return taken > 0 ? entry.again : entry.first;
}
