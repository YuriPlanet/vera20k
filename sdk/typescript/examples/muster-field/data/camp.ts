// The camp's sizes and prices, by camp rank where they grow. (Owner: shop)

/** Offers in the row. */
export const ROW: StaticArray<i32> = [3, 4, 5];
/** Places in the company. */
export const PLACES: StaticArray<i32> = [5, 7, 9];
/** Percent of the draws that are of tier 0, 1 and 2, three per rank. */
export const ODDS: StaticArray<i32> = [88, 12, 0, 52, 38, 10, 28, 44, 28];
/** What raising the camp from rank 0 and from rank 1 costs. */
export const IMPROVE: StaticArray<i32> = [600, 1400];

export const REDRAW: i32 = 50;
/**
 * A dismissal pays back this part of the price. With a champion paid back
 * as three, more would make merge and dismiss a cheap way to supplies.
 */
export const REFUND_PERCENT: i32 = 50;

/** `rank` held to the ranks there are. */
export function rank(rank: i64): i32 {
  const held = rank < 0 ? 0 : rank;
  return <i32>min<i64>(held, <i64>(ROW.length - 1));
}

/** What dismissing a member pays back. */
export function refund(price: i32, champion: bool): i32 {
  const three = champion ? 3 : 1;
  return three * ((price * REFUND_PERCENT) / 100);
}
