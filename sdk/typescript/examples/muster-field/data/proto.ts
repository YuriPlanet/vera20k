// Between `rules.wasm` and `ui.wasm` (RFC 0001 section 10).
//
// The simulation publishes the camp as private house variables of the
// player's house, written when they change; the interface reads them each
// render frame. Variables are saved with the match, so an interface that
// starts again after a load sees the camp at once. Both `vera.json` files
// declare these names.
//
// The interface sends two-byte messages, `[what, index]`. The simulation
// turns each into the order a player would give a camp unit, so a button
// and the order do the same thing (`shop.uiOrder`).

/** `PHASE`: 0 waiting, 1 muster, 2 battle, 3 over. */
export const PHASE = "MfPhase";
export const WAVE = "MfWave";
export const BANNERS = "MfBanners";
export const RANK = "MfRank";
export const PICKS = "MfPicks";
/** `FLAG_OFFICER`, `FLAG_TRUCKS`. */
export const FLAGS = "MfFlags";
/** A byte per offer (`packOffer`), offer 0 in the low byte. */
export const OFFERS = "MfOffers";
/** A byte per supply truck: the supply plus one, 0 for none. */
export const CHOICES = "MfChoices";
/** A byte per member (`packMember`): members 0..5, then 5..10. */
export const COMPANY_LOW = "MfCompanyLow";
export const COMPANY_HIGH = "MfCompanyHigh";
/** Four bits per supply: how often it was taken. */
export const TAKEN = "MfTaken";
/** The battle: company standing, raiders standing, raiders arrived, 16 bits each. */
export const BATTLE = "MfBattle";
/** The front's index in `FRONT_NAMES`. */
export const FRONT = "MfFront";
/** The last battle (`packLast`); 0 before the first. */
export const LAST = "MfLast";

/** The officer stands in camp: improve and redraw work. */
export const FLAG_OFFICER: i64 = 1;
/** The supply trucks stand: a pick works. */
export const FLAG_TRUCKS: i64 = 2;

export const MSG_HIRE: u8 = 1;
export const MSG_KEEP: u8 = 2;
export const MSG_REDRAW: u8 = 3;
export const MSG_IMPROVE: u8 = 4;
export const MSG_START: u8 = 5;
export const MSG_PICK: u8 = 6;
export const MSG_DISMISS: u8 = 7;

/** Byte `at` of `packed`. */
export function byte(packed: i64, at: i32): u8 {
  if (at < 0 || at >= 8) return 0;
  return <u8>(packed >> (8 * at));
}

/** `packed` with byte `at` set to `value`. */
export function withByte(packed: i64, at: i32, value: u8): i64 {
  if (at < 0 || at >= 8) return packed;
  const shift = <i64>(8 * at);
  return (packed & ~(<i64>0xff << shift)) | (<i64>value << shift);
}

const KIND: u8 = 0x1f;
const BIT_A: u8 = 1 << 5;
const BIT_B: u8 = 1 << 6;

/**
 * A place in the row: `kind` its index in `MERCS` (−1 for none), `kept`,
 * and `standing` (its unit stands in the row; a kept offer may wait for
 * room).
 */
export function packOffer(kind: i32, kept: bool, standing: bool): u8 {
  if (kind < 0 || kind >= <i32>KIND) return 0;
  return (<u8>(kind + 1)) | (kept ? BIT_A : 0) | (standing ? BIT_B : 0);
}

/** The kind of a packed offer or member, −1 for none. */
export function kindOf(packed: u8): i32 {
  const kind = packed & KIND;
  return kind == 0 ? -1 : <i32>kind - 1;
}

/** A packed offer's `kept`, a packed member's `champion`. */
export function bitA(packed: u8): bool {
  return (packed & BIT_A) != 0;
}

/** Whether a packed offer's or member's unit stands. */
export function standing(packed: u8): bool {
  return (packed & BIT_B) != 0;
}

/** A place in the company: `champion` takes the offer's `kept` bit. */
export function packMember(kind: i32, champion: bool, standing: bool): u8 {
  return packOffer(kind, champion, standing);
}

/** How the last battle went: `result` 0 none yet, 1 won, 2 lost. */
export function packLast(result: i64, supply: bool, kills: i64, paid: i64): i64 {
  return (result & 3) | (supply ? 4 : 0) | ((kills & 0xffff) << 8) | ((paid & 0xffffff) << 24);
}

export function lastResult(packed: i64): i64 {
  return packed & 3;
}
export function lastSupply(packed: i64): bool {
  return (packed & 4) != 0;
}
export function lastKills(packed: i64): i64 {
  return (packed >> 8) & 0xffff;
}
export function lastPaid(packed: i64): i64 {
  return (packed >> 24) & 0xffffff;
}

/** Three counts of 16 bits. */
export function battle(ours: i64, theirs: i64, arrived: i64): i64 {
  return (ours & 0xffff) | ((theirs & 0xffff) << 16) | ((arrived & 0xffff) << 32);
}

/** Count `at` (0 ours, 1 theirs, 2 arrived) of a packed `battle`. */
export function unbattle(packed: i64, at: i32): i64 {
  return (packed >> (16 * <i64>at)) & 0xffff;
}

/** Four bits per supply. */
export function taken(packed: i64, perk: i32): i64 {
  if (perk < 0 || perk >= 16) return 0;
  return (packed >> (4 * <i64>perk)) & 0xf;
}

export function withTaken(packed: i64, perk: i32, count: i64): i64 {
  if (perk < 0 || perk >= 16) return packed;
  const shift = 4 * <i64>perk;
  return (packed & ~(<i64>0xf << shift)) | ((count & 0xf) << shift);
}
