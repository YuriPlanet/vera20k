// The mercenaries. (Owner: content)
//
// Each was measured in this engine against the others at equal budgets,
// under both owners; the prices follow those battles. None beats all the
// others: foot loses to the flak track and the Gattling tank, those to
// guns, guns to shock troopers and brutes, and those two to commandos.

/** What a supply can apply to; a mercenary has one or more. */
export const FOOT: u32 = 1 << 0;
export const VEHICLE: u32 = 1 << 1;
/** Fights from a distance. */
export const RANGED: u32 = 1 << 2;
/** Closes in: a weapon range of four cells or less. */
export const ASSAULT: u32 = 1 << 3;

/**
 * What stands in for three members. `fire` and `armor` are percent on top
 * of the rank, tuned per type: elite rank changes most types' weapons.
 */
export class Champion {
  constructor(public rank: i64, public fire: i64, public armor: i64) {}
}

function elite(fire: i64, armor: i64): Champion {
  return new Champion(2, fire, armor);
}

/** For the types whose elite weapons alone would clear a wave. */
function veteran(fire: i64, armor: i64): Champion {
  return new Champion(1, fire, armor);
}

export class Merc {
  constructor(
    /** The TechnoType. */
    public id: string,
    /** At most five Han characters: the row's line holds five of them. */
    public name: string,
    public price: i32,
    /** 0 to 2; `shop` draws by camp rank. */
    public tier: i32,
    public tags: u32,
    public champion: Champion,
  ) {}
}

export const MERCS: StaticArray<Merc> = [
  new Merc("E2", "动员兵", 130, 0, FOOT | ASSAULT, elite(110, 125)),
  new Merc("E1", "美国大兵", 135, 0, FOOT | ASSAULT, elite(105, 120)),
  new Merc("FLAKT", "防空步兵", 160, 0, FOOT, elite(115, 170)),
  new Merc("INIT", "尤里新兵", 190, 0, FOOT, elite(110, 135)),
  new Merc("SHK", "磁暴步兵", 280, 0, FOOT | ASSAULT, elite(72, 85)),
  new Merc("SNIPE", "狙击手", 340, 1, FOOT | RANGED, elite(100, 150)),
  new Merc("BRUTE", "狂兽人", 420, 1, FOOT | ASSAULT, elite(115, 175)),
  new Merc("HTK", "防空履带车", 480, 1, VEHICLE, elite(115, 145)),
  new Merc("YTNK", "盖特坦克", 590, 1, VEHICLE, elite(130, 152)),
  new Merc("MTNK", "灰熊坦克", 620, 1, VEHICLE, elite(70, 85)),
  new Merc("GHOST", "海豹突击队", 640, 2, FOOT, elite(100, 260)),
  new Merc("TNKD", "坦克杀手", 780, 2, VEHICLE, elite(120, 175)),
  new Merc("SREF", "光棱坦克", 900, 2, VEHICLE | RANGED, veteran(170, 215)),
  new Merc("TTNK", "磁能坦克", 1120, 2, VEHICLE | ASSAULT, veteran(112, 116)),
  new Merc("APOC", "天启坦克", 1300, 2, VEHICLE, veteran(92, 119)),
];

/** Mercenary `kind`, or the first for an index out of the table. */
export function merc(kind: i32): Merc {
  return kind >= 0 && kind < MERCS.length ? MERCS[kind] : MERCS[0];
}
