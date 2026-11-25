//!487A10 damage/repair adapter to the shared numeric cell-entry authority.
use super::*;

pub(super) fn impassable(
    live: &mut LivePublication<'_>,
    object: CellObjectMember,
    cell: Cell,
) -> Result<bool, String> {
    crate::sim::world::object_entry::classify_object(
        live.sim,
        live.rules,
        object,
        cell,
        crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
    )
    .map(|code| code == 7)
}
