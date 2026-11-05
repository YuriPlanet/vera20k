//! Read-only World and House namespace tables for Lua mods.
//!
//! One design, on top of `userdata.rs`: every query reads the per-frame
//! [`ScriptContext`] the host publishes for the duration of the `Update`
//! dispatch (`set_current` / `clear_current`), and `World` queries return the
//! userdata handles mods call methods on, as WolfCTOS does. The `House`
//! namespace keeps the WolfCTOS entry points as documented stubs — resolving
//! real houses into Lua objects is not wired yet, and a stub must not fake it.

use std::cell::RefCell;

use mlua::{Lua, Table, Value};

use super::snapshot::EntitySnapshot;
use super::userdata::{ScriptContext, techno_handle};

thread_local! {
    /// The frame context for the dispatch in progress; `None` outside the
    /// dispatch, where every query returns an empty table.
    static CURRENT: RefCell<Option<ScriptContext>> = RefCell::new(None);
}

/// Publish the frame context for the duration of the `Update` dispatch.
pub(crate) fn set_current(context: ScriptContext) {
    CURRENT.with(|cell| *cell.borrow_mut() = Some(context));
}

/// Drop the frame context after the dispatch is done.
pub(crate) fn clear_current() {
    CURRENT.with(|cell| *cell.borrow_mut() = None);
}

/// The context of the frame being dispatched, if any.
fn current() -> Option<ScriptContext> {
    CURRENT.with(|cell| cell.borrow().clone())
}

/// Install the `World` and `House` namespaces on the Lua state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    lua.globals().set("World", world_table(lua)?)?;
    lua.globals().set("House", house_table(lua)?)?;
    Ok(())
}

/// Live mobile entity ids this frame (vehicles, infantry, aircraft), in logic
/// order.
fn mobile_ids(ctx: &ScriptContext) -> Vec<u64> {
    filter_ids(ctx, &|entity| entity.is_mobile())
}

/// Live structure ids this frame, in logic order.
fn building_ids(ctx: &ScriptContext) -> Vec<u64> {
    filter_ids(ctx, &|entity| !entity.is_mobile())
}

/// Live entity ids passing `filter`, in logic order.
fn filter_ids(ctx: &ScriptContext, filter: &dyn Fn(&EntitySnapshot) -> bool) -> Vec<u64> {
    let snapshot = ctx.snapshot.borrow();
    snapshot
        .entity_order
        .iter()
        .copied()
        .filter(|id| snapshot.entity(*id).is_some_and(|entity| filter(&entity)))
        .collect()
}

/// Selected mobile entity ids, in selection order.
fn selected_ids(ctx: &ScriptContext) -> Vec<u64> {
    let snapshot = ctx.snapshot.borrow();
    snapshot
        .selected
        .iter()
        .copied()
        .filter(|id| {
            snapshot
                .entity(*id)
                .is_some_and(|entity| entity.is_mobile())
        })
        .collect()
}

/// Turn ids into the userdata handles mods call methods on.
fn handle_table(lua: &Lua, ctx: &ScriptContext, ids: Vec<u64>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (index, id) in ids.into_iter().enumerate() {
        table.set(index + 1, techno_handle(lua, ctx, id)?)?;
    }
    Ok(table)
}

fn world_table(lua: &Lua) -> mlua::Result<Table> {
    let world = lua.create_table()?;

    // World.GetUnits() -- mobile technos (vehicles, infantry, aircraft)
    world.set(
        "GetUnits",
        lua.create_function(|lua, (): ()| {
            let Some(ctx) = current() else {
                return lua.create_table();
            };
            handle_table(lua, &ctx, mobile_ids(&ctx))
        })?,
    )?;

    // World.GetBuildings() -- structure objects
    world.set(
        "GetBuildings",
        lua.create_function(|lua, (): ()| {
            let Some(ctx) = current() else {
                return lua.create_table();
            };
            handle_table(lua, &ctx, building_ids(&ctx))
        })?,
    )?;

    // World.GetAllUnits() -- every live techno, buildings included
    world.set(
        "GetAllUnits",
        lua.create_function(|lua, (): ()| {
            let Some(ctx) = current() else {
                return lua.create_table();
            };
            handle_table(lua, &ctx, filter_ids(&ctx, &|_| true))
        })?,
    )?;

    // World.GetUnitsInRadius(x, y, radius) -- mobile technos within the
    // radius, in the same map-cell coordinates GetPosition reports.
    world.set(
        "GetUnitsInRadius",
        lua.create_function(|lua, (x, y, radius): (f64, f64, f64)| {
            let Some(ctx) = current() else {
                return lua.create_table();
            };
            let ids = {
                let snapshot = ctx.snapshot.borrow();
                snapshot
                    .entity_order
                    .iter()
                    .copied()
                    .filter(|id| {
                        snapshot.entity(*id).is_some_and(|entity| {
                            entity.is_mobile()
                                && (entity.x - x).powi(2) + (entity.y - y).powi(2)
                                    <= radius * radius
                        })
                    })
                    .collect()
            };
            handle_table(lua, &ctx, ids)
        })?,
    )?;

    // World.GetSelectedUnits() -- the player's current selection
    world.set(
        "GetSelectedUnits",
        lua.create_function(|lua, (): ()| {
            let Some(ctx) = current() else {
                return lua.create_table();
            };
            handle_table(lua, &ctx, selected_ids(&ctx))
        })?,
    )?;

    Ok(world)
}

fn house_table(lua: &Lua) -> mlua::Result<Table> {
    let house = lua.create_table()?;

    // House.GetPlayer() -- WolfCTOS returns the local human house. The
    // snapshot carries `local_house`, but resolving it into a house object is
    // not wired yet: documented stub, returns nil (do not fake it).
    house.set(
        "GetPlayer",
        lua.create_function(|_, (): ()| -> mlua::Result<Value> {
            Ok(Value::Nil)
        })?,
    )?;

    // House.GetCount() -- registered-house count; not wired yet (stub, 0).
    house.set(
        "GetCount",
        lua.create_function(|_, (): ()| -> mlua::Result<i32> { Ok(0) })?,
    )?;

    // House.GetByIndex(index) -- not wired yet (stub, nil).
    house.set(
        "GetByIndex",
        lua.create_function(|_, _: i32| -> mlua::Result<Value> { Ok(Value::Nil) })?,
    )?;

    Ok(house)
}
