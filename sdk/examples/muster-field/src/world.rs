//! What several parts ask of the map and the engine.

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

use crate::state;

pub fn offset(cell: Cell, dx: i32, dy: i32) -> Cell {
    let x = (i32::from(cell.x()) + dx).clamp(1, 510) as u16;
    let y = (i32::from(cell.y()) + dy).clamp(1, 510) as u16;
    Cell::at(x, y)
}

pub fn delta(from: Cell, to: Cell) -> (i32, i32) {
    (
        i32::from(to.x()) - i32::from(from.x()),
        i32::from(to.y()) - i32::from(from.y()),
    )
}

pub fn cell_of(unit: Techno) -> Option<Cell> {
    get(unit, Prop::Cell).ok().map(Cell)
}

pub fn alive(unit: Techno) -> bool {
    unit.0 != NO_HANDLE && unit.health().is_ok_and(|health| health > 0)
}

/// Creates a unit on `spot` or the closest ring of cells that takes it.
pub fn create_near(spot: Cell, id: &str, house: House) -> Option<Techno> {
    let ty = TypeRef::techno_type(id).ok()?;
    for ring in 0..=5i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                if let Ok(unit) = Action::create_object(offset(spot, dx, dy), ty, house) {
                    return Some(unit);
                }
            }
        }
    }
    None
}

/// Removes everything `house` has on the map, strays such as a tank's crew
/// included.
pub fn clear_forces(house: House) {
    let mut found = [0i64; 64];
    let mut after = 0;
    loop {
        let Ok((count, next)) = Find::objects().owned_by(house).page(&mut found, after) else {
            return;
        };
        for &object in &found[..count] {
            let _ = Action::remove(Techno(object));
        }
        if next == 0 {
            return;
        }
        after = next;
    }
}

pub fn credits() -> i64 {
    get(state::player(), Prop::Money).unwrap_or(0)
}

/// Pays the player; a negative amount charges them.
pub fn pay(amount: i32) -> bool {
    Action::add_money(state::player(), amount).is_ok()
}

/// What keeps a unit still and harmless: its speed and weapon holds.
#[derive(Clone, Copy)]
pub struct Hold(pub i64, pub i64);

pub fn hold(unit: Techno) -> Hold {
    let speed = Modifier::multiply(unit, Prop::SpeedMultiplier, 0, -1);
    let fire = Modifier::override_(unit, Prop::DisableWeapons, 1, -1);
    Hold(
        speed.map_or(NO_HANDLE, |held| held.0),
        fire.map_or(NO_HANDLE, |held| held.0),
    )
}

pub fn release(hold: Hold) {
    for handle in [hold.0, hold.1] {
        if handle != NO_HANDLE {
            let _ = Modifier(handle).remove();
        }
    }
}
