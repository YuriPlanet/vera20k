//! A `rules.wasm` that runs actions through the Rust SDK (RFC 0001 preview).
//!
//! At scenario start it kills the first infantryman on the map, damages the
//! first vehicle with the `Super` warhead, heals it and orders it to guard,
//! and sets Red Alert 2 global 3 through stock action `SetGlobal`, logging
//! each result. The engine test
//! `sim::ext_modules::tests::a_rust_built_actions_module_runs` runs it.
#![no_std]

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, Result};

fn first(category: Category) -> Techno {
    let Ok(prop) = Prop::named("Category") else {
        return Techno(NO_HANDLE);
    };
    Techno(
        Find::objects()
            .where_prop(prop, Op::Eq, category as i64)
            .first(),
    )
}

fn report(what: &str, result: Result<()>) {
    log(what);
    if result.is_err() {
        log("actions-demo: failed");
    }
}

fn start() {
    report(
        "actions-demo: kill",
        Action::kill(first(Category::Infantry), Techno(NO_HANDLE)),
    );
    if let Ok(id) = lookup(NameKind::Warhead, "Super") {
        let warhead = TypeRef::new(NameKind::Warhead, id);
        report(
            "actions-demo: damage",
            Action::damage(first(Category::Vehicle), 10, warhead),
        );
    }
    let vehicle = first(Category::Vehicle);
    report("actions-demo: heal", Action::heal(vehicle, 5));
    report("actions-demo: guard", vehicle.set_mission(Mission::Guard));
    report("actions-demo: set global", Action::set_global(3));
}

vera20k_sdk::on_scenario_start!(start);
