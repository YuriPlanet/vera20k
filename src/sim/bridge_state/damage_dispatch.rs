//! Apply_area_damage's four bridge blocks, 00489E87..0048A2C4.
//!
//! Keep the selected CellClass and first anchor lookup across callbacks. Each
//! later block reads their current fields; a driver can change the next block's
//! admission. Native comparisons: tools/spatial_oracle/bridge_damage_admission.

use super::DispatchPath;

#[cfg(test)]
#[path = "damage_dispatch_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(crate) struct CellFields {
    pub flags: u32,
    pub tile: i32,
    pub overlay: i32,
    pub level: u8,
}

pub(crate) trait DamageHost {
    type Cell: Copy;
    fn fields(&self, cell: Self::Cell) -> CellFields;
    /// Read self/+2C's current coordinate, then perform the native GetCell.
    fn resolve_anchor(&mut self, cell: Self::Cell) -> Option<Self::Cell>;
    fn tile_bases(&self) -> [i32; 2];
    fn middle_tiles(&self) -> Option<[i32; 2]>;
    fn roll_strength(&mut self) -> i32;
    fn apply(&mut self, path: DispatchPath) -> bool;
    fn detach(&mut self, cell: Self::Cell);
    fn dirty(&mut self, path: DispatchPath);
}

fn middle_matches(tile: i32, base: i32, middle: Option<[i32; 2]>) -> bool {
    let Some(middle) = middle else { return false };
    let relative = tile.wrapping_sub(base).wrapping_add(1);
    middle
        .into_iter()
        .any(|first| (0..4).any(|step| relative == first.wrapping_add(step)))
}

fn in_height_window(fields: CellFields, impact_z_leptons: i32) -> bool {
    if fields.flags & 0x100 == 0 {
        return true;
    }
    let ground_level = i32::from(fields.level as i8);
    let level_height = crate::util::lepton::LEPTONS_PER_LEVEL as i32;
    let deck_height = crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32;
    impact_z_leptons > (ground_level - 2) * level_height + deck_height
        && impact_z_leptons <= (ground_level + 1) * level_height + deck_height
}

/// The caller has already admitted SpecialFlags::DestroyableBridges and Wall.
/// Damage remains a signed dword. IonCannon identity bypasses every ranged draw.
pub(crate) fn dispatch<H: DamageHost>(
    host: &mut H,
    cell: H::Cell,
    damage: i32,
    impact_z_leptons: i32,
    ion_cannon: bool,
) {
    let initial = host.fields(cell);
    let high_tile = middle_matches(initial.tile, host.tile_bases()[0], host.middle_tiles());
    let anchor = if initial.flags & 0x100 != 0 {
        host.resolve_anchor(cell)
    } else {
        None
    };
    for path in [
        DispatchPath::HighStateMachine,
        DispatchPath::LowStateMachine,
        DispatchPath::LowDirect,
        DispatchPath::HighDirect,
    ] {
        let current = host.fields(cell);
        let matches = match path {
            DispatchPath::HighStateMachine => {
                (high_tile
                    || anchor
                        .is_some_and(|anchor| matches!(host.fields(anchor).overlay, 0x18 | 0x19)))
                    && in_height_window(current, impact_z_leptons)
            }
            DispatchPath::LowStateMachine => {
                (middle_matches(current.tile, host.tile_bases()[1], host.middle_tiles())
                    || anchor
                        .is_some_and(|anchor| matches!(host.fields(anchor).overlay, 0xed | 0xee)))
                    && in_height_window(current, impact_z_leptons)
            }
            DispatchPath::LowDirect => (0x4a..=0x63).contains(&current.overlay),
            DispatchPath::HighDirect => (0xcd..=0xe6).contains(&current.overlay),
        };
        if !matches || (!ion_cannon && host.roll_strength() >= damage) {
            continue;
        }
        let attempts = if ion_cannon && path.is_state_machine() {
            4
        } else {
            1
        };
        for _ in 0..attempts {
            if host.apply(path) {
                host.detach(cell);
                break;
            }
        }
        if path.is_state_machine() {
            host.dirty(path);
        }
    }
}

/// ApplyDamageToCell 00587180 chooses its driver again on every invocation;
/// the caller's A/B block does not force the concrete/wooden family.
pub(crate) fn select_driver<H: DamageHost>(host: &mut H, cell: H::Cell) -> Option<DispatchPath> {
    let fields = host.fields(cell);
    if (0x4a..=0x63).contains(&fields.overlay) {
        return Some(DispatchPath::LowDirect);
    }
    if (0xcd..=0xe6).contains(&fields.overlay) {
        return Some(DispatchPath::HighDirect);
    }
    let anchor = if fields.flags & 0x100 != 0 {
        host.resolve_anchor(cell)
    } else {
        None
    };
    if anchor.is_some_and(|anchor| matches!(host.fields(anchor).overlay, 0x18 | 0x19))
        || middle_matches(fields.tile, host.tile_bases()[0], host.middle_tiles())
    {
        Some(DispatchPath::HighStateMachine)
    } else if anchor.is_some_and(|anchor| matches!(host.fields(anchor).overlay, 0xed | 0xee))
        || middle_matches(fields.tile, host.tile_bases()[1], host.middle_tiles())
    {
        Some(DispatchPath::LowStateMachine)
    } else {
        None
    }
}
