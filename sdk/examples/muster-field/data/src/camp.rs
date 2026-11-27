//! The camp's sizes and prices, by camp rank where they grow. (Owner: shop)

/// Offers in the row.
pub const ROW: [usize; 3] = [3, 4, 5];
/// Places in the company.
pub const PLACES: [usize; 3] = [5, 7, 9];
/// Percent of the draws that are of tier 0, 1 and 2.
pub const ODDS: [[i32; 3]; 3] = [[88, 12, 0], [52, 38, 10], [28, 44, 28]];
/// What raising the camp from rank 0 and from rank 1 costs.
pub const IMPROVE: [i32; 2] = [600, 1400];

pub const REDRAW: i32 = 50;
/// A dismissal pays back this part of the price. With a champion paid back
/// as three, more would make merge and dismiss a cheap way to supplies.
pub const REFUND_PERCENT: i32 = 50;

/// `rank` held to the ranks there are.
pub fn rank(rank: i64) -> usize {
    (rank.max(0) as usize).min(ROW.len() - 1)
}

/// What dismissing a member pays back.
pub fn refund(price: i32, champion: bool) -> i32 {
    let three = if champion { 3 } else { 1 };
    three * (price * REFUND_PERCENT / 100)
}
