// A `rules.wasm` with rules access and house orders, the same as the Rust
// SDK's `sdk/examples/orders-rules`.
//
// In `onRules` it doubles the GI's (`E1`) `Strength`. On its first frame
// with a computer house that has a vehicle, it orders that vehicle two
// cells east, once.
import {
  Category, Cell, Find, House, Ini, NO_HANDLE, NO_ID, Op, Order, Rules, Status, Techno,
  get, lastStatus, log, onFrame, onRules, propNamed, Every,
} from "@vera20k/sdk";

onRules((): void => {
  const strength = Ini.int(Rules.get("E1", "Strength"), 125);
  if (Rules.set("E1", "Strength", (strength * 2).toString()) == Status.Ok) {
    log("orders-rules: doubled E1 Strength");
  }
});

/** Whether the order went out. */
let ordered = false;
const houses = new StaticArray<i64>(16);
const unit = new StaticArray<Techno>(1);

function tick(): void {
  if (ordered) return;
  // By name, as a module written for another engine version would.
  const isHuman = propNamed("IsHuman");
  const category = propNamed("Category");
  const cellProp = propNamed("Cell");
  if (isHuman == NO_ID || category == NO_ID || cellProp == NO_ID) return;
  const page = Find.houses().where(isHuman, Op.Eq, 0).page(houses, 0);
  if (lastStatus() != Status.Ok) return;
  for (let i = 0; i < page.count; i++) {
    const house: House = houses[i];
    const vehicle = Find.objects()
      .ownedBy(house)
      .where(category, Op.Eq, <i64>Category.Vehicle)
      .first();
    if (vehicle == NO_HANDLE) continue;
    const at: Cell = get(vehicle, cellProp);
    if (lastStatus() != Status.Ok) continue;
    unit[0] = vehicle;
    if (House.order(house, Order.moveTo(unit, Cell.at(Cell.x(at) + 2, Cell.y(at)))) == Status.Ok) {
      log("orders-rules: ordered");
      ordered = true;
      return;
    }
  }
}

onFrame(new Every(1), tick);
