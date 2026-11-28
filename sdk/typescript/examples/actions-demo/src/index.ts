// A `rules.wasm` that runs actions, the same as the Rust SDK's
// `sdk/examples/actions-demo`.
//
// At scenario start it kills the first infantryman on the map, damages the
// first vehicle with the `Super` warhead, heals it and orders it to guard,
// and sets Red Alert 2 global 3 through stock action `SetGlobal`, logging
// each result. The engine test
// `sim::ext_modules::tests::a_rust_built_actions_module_runs` runs it.
import {
  Action, Category, Find, Mission, NO_HANDLE, NO_ID, NameKind, Op, Status, Techno, TypeRef,
  log, lookup, onScenarioStart, propNamed,
} from "@vera20k/sdk";

function first(category: Category): Techno {
  const prop = propNamed("Category");
  if (prop == NO_ID) return NO_HANDLE;
  return Find.objects().where(prop, Op.Eq, <i64>category).first();
}

function report(what: string, result: Status): void {
  log(what);
  if (result != Status.Ok) log("actions-demo: failed");
}

onScenarioStart((): void => {
  report("actions-demo: kill", Action.kill(first(Category.Infantry), NO_HANDLE));
  const id = lookup(NameKind.Warhead, "Super");
  if (id != NO_ID) {
    const warhead = TypeRef.make(NameKind.Warhead, id);
    report("actions-demo: damage", Action.damage(first(Category.Vehicle), 10, warhead));
  }
  const vehicle = first(Category.Vehicle);
  report("actions-demo: heal", Action.heal(vehicle, 5));
  report("actions-demo: guard", Techno.setMission(vehicle, Mission.Guard));
  report("actions-demo: set global", Action.setGlobal(3));
});
