// A `rules.wasm` with a runtime trigger (RFC 0001 sections 6a and 7), the
// same as the Rust SDK's `sdk/examples/panic-triggers`.
//
// At scenario start it makes one repeating `TakeDamage` trigger whose
// module action is `OnHit`, and binds it to every object on the map. Each
// hit then reaches `onHit` in the module phase, which sets Red Alert 2
// local variable 7 and logs the damage. The engine test
// `sim::ext_modules::trigger_tests::a_rust_built_trigger_module_receives_its_records`
// runs it.
import {
  Event, Find, NO_HANDLE, Status, TakeDamageEvent, Trigger, TriggerRef, Var,
  lastStatus, log, namedAction, onScenarioStart,
} from "@vera20k/sdk";

const LOCAL_7 = Var.local(7);
const page = new StaticArray<i64>(64);

function onHit(e: TakeDamageEvent): void {
  LOCAL_7.set(1);
  log("panic-triggers: hit for " + e.damage.toString());
}

namedAction("OnHit", onHit);

onScenarioStart((): void => {
  const trigger = Trigger.on(Event.TakeDamage).enqueue(onHit).done();
  if (trigger == NO_HANDLE) {
    log("panic-triggers: trigger_create failed");
    return;
  }
  const find = Find.objects();
  let after: i64 = 0;
  while (true) {
    const found = find.page(page, after);
    if (lastStatus() != Status.Ok) break;
    const count = found.count;
    const next = found.next;
    for (let i = 0; i < count; i++) TriggerRef.bind(trigger, page[i]);
    if (next == 0) break;
    after = next;
  }
});
