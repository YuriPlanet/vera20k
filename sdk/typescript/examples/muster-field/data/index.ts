// What "Muster Field"'s `rules.wasm` and `ui.wasm` both know: the tables
// of what can be hired, won and met, the camp's prices, and the protocol
// between the two (the variables the simulation publishes and the messages
// the interface sends). No engine calls: both modules import it. The port
// of the Rust example's `data/` crate.

export const MAX_OFFERS: i32 = 5;
export const MAX_COMPANY: i32 = 10;
export const MAX_RAIDERS: i32 = 24;

/** The sides the raiders come from, as `battle` names them (its `SIDES` order). */
export const FRONT_NAMES: StaticArray<string> = ["西南", "东南", "东北", "西北", "南面", "东面", "西面", "北面"];
