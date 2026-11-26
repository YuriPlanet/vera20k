//! Original CellClass callback487A10, called after synchronous bridge damage
//! and repair Recalc calls. Evidence:
//! tools/spatial_oracle/bridge_occupants.{py,json,meta.json}.
//!
//! This controller owns traversal order. The world host owns native-shaped
//! admission, active locomotor coordinates and complete direct damage effects.
use super::publication::CellCoord;

pub(crate) trait BridgeOccupantHost {
    type Cell: Copy + Eq;
    type Object: Copy;
    type Error;

    fn coord(&self, cell: Self::Cell) -> CellCoord;
    fn lookup(&mut self, coord: CellCoord) -> Self::Cell;
    fn ground_head(&self, cell: Self::Cell) -> Option<Self::Object>;
    fn next_object(&self, object: Self::Object) -> Option<Self::Object>;
    fn is_foot(&self, object: Self::Object) -> bool;
    fn is_techno(&self, object: Self::Object) -> bool;
    fn type_is_jumpjet(&mut self, object: Self::Object) -> Result<bool, Self::Error>;
    /// Original +1AC(selectedCell, -1, -1, null, true). Sentinel direction
    /// changes the bridge traversal gate; ordinary movement queries differ.
    fn admission(&mut self, object: Self::Object, cell: Self::Cell) -> Result<i32, Self::Error>;
    fn abstract_kind(&mut self, object: Self::Object) -> Result<i32, Self::Error>;
    fn current_health(&self, object: Self::Object) -> i32;
    /// Original +16C(&localHealth,0,C4Warhead,null,true,true,null).
    /// This is a copied packet, unlike BlowUpBridge's aliased Health pointer.
    fn receive_damage(&mut self, object: Self::Object, damage: i32) -> Result<(), Self::Error>;
    /// Current selected Cell center in leptons, with original47B3A0(128,128)
    /// signed-level/slope ground Z. Captured once, after the first traversal.
    fn ground_probe(&mut self, cell: Self::Cell) -> Result<[i32; 3], Self::Error>;
    /// Active ILocomotion+A0, with full stored Head_To/current raw coordinates.
    /// A Foot without its required locomotor is an error, not a false result.
    fn is_at_coord(&mut self, object: Self::Object, probe: [i32; 3]) -> Result<bool, Self::Error>;
}

/// Damage mode is the byte loaded at487A27, not the entire stack dword. A
/// nonzero mode adds the TechnoType.JumpJet resident gate; the neighbor pass
/// always requires Is_At_Coord and numeric admission7.
pub(crate) fn recheck_occupants<H: BridgeOccupantHost>(
    host: &mut H,
    selected: H::Cell,
    damage_mode: u8,
) -> Result<(), H::Error> {
    let mut current = host.ground_head(selected);
    while let Some(object) = current {
        // 487A2D captures NextObject before admission, kind and damage callbacks.
        let next = host.next_object(object);
        if host.admission(object, selected)? == 7
            || (damage_mode != 0 && host.is_techno(object) && host.type_is_jumpjet(object)?)
            || host.abstract_kind(object)? == 2
        {
            host.receive_damage(object, host.current_health(object))?;
        }
        current = next;
    }

    let probe = host.ground_probe(selected)?;
    for x in -2..=2i16 {
        for y in -2..=2i16 {
            // 487AFC rereads the selected identity on EVERY iteration. In
            // particular a shared dummy lookup can move this same receiver.
            let center = host.coord(selected);
            let cell = host.lookup((center.0.wrapping_add(x), center.1.wrapping_add(y)));
            if cell == selected {
                continue;
            }
            let mut current = host.ground_head(cell);
            while let Some(object) = current {
                if host.is_foot(object)
                    && host.is_at_coord(object, probe)?
                    && host.admission(object, selected)? == 7
                {
                    host.receive_damage(object, host.current_health(object))?;
                }
                // 487BD6 reads the live link AFTER callbacks, not a snapshot.
                current = host.next_object(object);
            }
        }
    }
    Ok(())
}

#[cfg(all(test, tests_sim_a_b))]
#[path = "occupants_tests.rs"]
mod tests;
