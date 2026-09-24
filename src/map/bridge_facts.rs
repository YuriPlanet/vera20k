//! Authoritative high-bridge cell facts stamped from map overlay data.

pub const BRIDGE_FLAG_ANCHOR_SELF: u32 = 0x80;
pub const BRIDGE_FLAG_STRUCTURAL: u32 = 0x100;
pub const BRIDGE_FLAG_TRANSITION: u32 = 0x200;
pub const BRIDGE_FLAG_DESTROYED_OR_RAMP: u32 = 0x400;
pub const BRIDGE_FLAG_DIRECTION_ZERO: u32 = 0x800;
pub const BRIDGE_FLAG_FORWARD_SIDE: u32 = 0x1000;
pub const BRIDGE_FLAG_EXTRA_SIDE: u32 = 0x10000;
/// The three bridge bits represented on live real and fallback CellClass flag
/// words for this mechanism. Other `+0x140` writers remain owned by their
/// established cell models until their native preservation contracts are mapped.
pub const MODELED_CELLCLASS_BRIDGE_FLAG_MASK: u32 =
    BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_FORWARD_SIDE;

/// Retained real/dummy flag authority includes the post-load gap writer586BF0.
/// The older1180 projection remains a narrow query view, not the storage mask.
pub(crate) const RETAINED_CELLCLASS_BRIDGE_FLAG_MASK: u32 =
    MODELED_CELLCLASS_BRIDGE_FLAG_MASK | BRIDGE_FLAG_DESTROYED_OR_RAMP | BRIDGE_FLAG_DIRECTION_ZERO;

/// The additional400/800 stores of SetBridgeDirection47E040/47E470.
/// Anchor/F1/F2/opposite share them; F3/extra preserve both. This matches the
/// existing full BridgeCellFacts Mark/Destroy writer without inventing relations.
/// `TechnoClass::IsOnBridge_ForFiring @ 0x00703B10` (`mask` `0x100`) and its
/// `0x00703CC0` twin (`0x400`), over the `CellClass+0x140` words of an object's
/// own cell and of its S, N, E and W neighbours (`g_DirectionOffsets`
/// `0x0089F688` indices 4, 0, 2, 6): the own cell carries `mask`, or a S/N
/// neighbour carries it with the span axis bit `0x800`, or an E/W one without.
/// A missing neighbour answers nothing. The OnBridge (`+0x8C`) exemption
/// before the test belongs to the caller.
pub(crate) fn near_bridge(
    own: u32,
    [south, north, east, west]: [Option<u32>; 4],
    mask: u32,
) -> bool {
    let span = |flags: Option<u32>, axis: bool| {
        flags.is_some_and(|flags| {
            flags & mask != 0 && (flags & BRIDGE_FLAG_DIRECTION_ZERO != 0) == axis
        })
    };
    own & mask != 0
        || span(south, true)
        || span(north, true)
        || span(east, false)
        || span(west, false)
}

pub(crate) fn apply_retained_cellclass_bridge_slot(
    flags: &mut u32,
    slot: BridgeStampSlot,
    set: bool,
    direction: u8,
) {
    apply_modeled_cellclass_bridge_slot(flags, slot, set);
    if matches!(
        slot,
        BridgeStampSlot::Anchor
            | BridgeStampSlot::Forward1
            | BridgeStampSlot::Forward2
            | BridgeStampSlot::Opposite
    ) {
        *flags &= !(BRIDGE_FLAG_DESTROYED_OR_RAMP | BRIDGE_FLAG_DIRECTION_ZERO);
        if !set {
            *flags |= BRIDGE_FLAG_DESTROYED_OR_RAMP;
        }
        if direction == 0 {
            *flags |= BRIDGE_FLAG_DIRECTION_ZERO;
        }
    }
}

/// Typed view of the CellClass flag word, single-sourced from the consts above.
///
/// Bit values are NOT redefined here — every predicate references the
/// `BRIDGE_FLAG_*` consts so map-load (this file), the topology service, and the
/// render draw-offset trait all read one source of truth. A thin newtype keeps
/// the existing const style (no `bitflags!` dep) while giving the topology
/// service a borrowable typed handle instead of a raw `u32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BridgeFlags(pub u32);

impl BridgeFlags {
    #[inline]
    pub fn has(self, bit: u32) -> bool {
        self.0 & bit != 0
    }
    /// `0x80` — anchor cell of the stamp (adds the deck height in effective-Z).
    #[inline]
    pub fn anchor(self) -> bool {
        self.has(BRIDGE_FLAG_ANCHOR_SELF)
    }
    /// `0x100` — authoritative structural high-bridge cell. Distinct from the
    /// concrete/wood tileset windows (those are tile-id ranges, not this flag).
    #[inline]
    pub fn structural(self) -> bool {
        self.has(BRIDGE_FLAG_STRUCTURAL)
    }
    /// `0x200` — native bridge-entry flag, also required along traversable
    /// deck lanes by 0x004D9E08. The stamp sets Anchor/Forward1/Opposite and
    /// clears Forward2; this is not merely an along-span endpoint marker.
    #[inline]
    pub fn bridgehead(self) -> bool {
        self.has(BRIDGE_FLAG_TRANSITION)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum BridgeStampFamily {
    #[default]
    None,
    Nesw,
    Nwse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BridgeStampSlot {
    Anchor,
    Forward1,
    Forward2,
    Forward3,
    Opposite,
    ExtraDir6,
}

impl BridgeStampSlot {
    pub(crate) const fn writes_native_anchor(self) -> bool {
        matches!(self, Self::Forward1 | Self::Forward2 | Self::Opposite | Self::ExtraDir6)
    }
}

/// One native `CellClass::SetBridgeDirection_*` flag-word transaction.
///
/// The anchor crosses the CellStruct seam as two signed 16-bit words. Each
/// walked coordinate wraps at that same width before MapClass applies its
/// fixed `y * 512 + x` lookup, so off-axis requests can alias a real slot or
/// visit the one shared fallback CellClass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeFlagStamp {
    pub anchor: (i16, i16),
    pub direction: u8,
    pub set: bool,
}

impl BridgeFlagStamp {
    pub const fn new(anchor: (u16, u16), direction: u8, set: bool) -> Self {
        Self {
            anchor: (anchor.0 as i16, anchor.1 as i16),
            direction,
            set,
        }
    }

    /// Exact setter visitation order: anchor, three forward cells, opposite,
    /// then the direction-6 extra cell.
    pub(crate) fn slots(self) -> Option<[(BridgeStampSlot, Option<(i32, i32)>); 6]> {
        let _ = crate::util::direction::direction_delta(self.direction)?;
        let anchor = self.anchor;
        let f1 = packed_step(anchor, self.direction)?;
        let f2 = packed_step(f1, self.direction)?;
        let f3 = packed_step(f2, self.direction)?;
        let opposite_direction = crate::util::direction::opposite_direction(self.direction)?;
        let opposite = packed_step(anchor, opposite_direction)?;
        let extra = (self.direction == 6)
            .then(|| packed_step(opposite, 2))
            .flatten()
            .map(packed_i32);
        Some([
            (BridgeStampSlot::Anchor, Some(packed_i32(anchor))),
            (BridgeStampSlot::Forward1, Some(packed_i32(f1))),
            (BridgeStampSlot::Forward2, Some(packed_i32(f2))),
            (BridgeStampSlot::Forward3, Some(packed_i32(f3))),
            (BridgeStampSlot::Opposite, Some(packed_i32(opposite))),
            (BridgeStampSlot::ExtraDir6, extra),
        ])
    }
}

/// Apply the verified `0x80/0x100/0x1000` subset of one setter slot.
///
/// gamemd-derived: `CellClass::SetBridgeDirection_NESW @ 0x0047E040` and
/// `_NWSE @ 0x0047E470`. Intact anchor/F1/F2 set both bits, F3 sets only
/// `0x1000`, opposite sets `0x100` and clears `0x1000`, and Extra preserves
/// all three. Only Anchor sets/clears `0x80`; non-anchor slots preserve it.
pub(crate) fn apply_modeled_cellclass_bridge_slot(
    flags: &mut u32,
    slot: BridgeStampSlot,
    set: bool,
) {
    if set {
        match slot {
            BridgeStampSlot::Anchor | BridgeStampSlot::Forward1 | BridgeStampSlot::Forward2 => {
                *flags |= BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_FORWARD_SIDE;
                if slot == BridgeStampSlot::Anchor {
                    *flags |= BRIDGE_FLAG_ANCHOR_SELF;
                }
            }
            BridgeStampSlot::Forward3 => *flags |= BRIDGE_FLAG_FORWARD_SIDE,
            BridgeStampSlot::Opposite => {
                *flags &= !BRIDGE_FLAG_FORWARD_SIDE;
                *flags |= BRIDGE_FLAG_STRUCTURAL;
            }
            BridgeStampSlot::ExtraDir6 => {}
        }
    } else {
        match slot {
            BridgeStampSlot::Anchor
            | BridgeStampSlot::Forward1
            | BridgeStampSlot::Forward2
            | BridgeStampSlot::Opposite => {
                *flags &= !(BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_FORWARD_SIDE);
                if slot == BridgeStampSlot::Anchor {
                    *flags &= !BRIDGE_FLAG_ANCHOR_SELF;
                }
            }
            BridgeStampSlot::Forward3 => *flags &= !BRIDGE_FLAG_FORWARD_SIDE,
            BridgeStampSlot::ExtraDir6 => {}
        }
    }
}

fn packed_step(cell: (i16, i16), direction: u8) -> Option<(i16, i16)> {
    let (dx, dy) = crate::util::direction::direction_delta(direction)?;
    Some((
        cell.0.wrapping_add(dx as i16),
        cell.1.wrapping_add(dy as i16),
    ))
}

const fn packed_i32(cell: (i16, i16)) -> (i32, i32) {
    (cell.0 as i32, cell.1 as i32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeAnchorRelation {
    pub anchor: (u16, u16),
    pub slot: BridgeStampSlot,
    pub family: BridgeStampFamily,
    pub direction: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BridgeRampKind {
    TopRight,
    TopLeft,
    Middle1,
    Middle2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeRampTile {
    pub kind: BridgeRampKind,
    pub relative_tile_index: u16,
    pub height_byte: u8,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub struct BridgeCellFacts {
    pub raw_flags: u32,
    pub state_byte: u8,
    pub overlay_id: Option<u8>,
    pub family: BridgeStampFamily,
    pub direction: Option<u8>,
    pub anchor: Option<BridgeAnchorRelation>,
    /// Literal CellClass+0x2C. Native47E040 preserves this on Anchor/Forward3;
    /// the derived self relation above must not replace a retained pointer.
    pub native_anchor: Option<crate::map::cell_index::NativeCellIdentity>,
    pub ramp_tile: Option<BridgeRampTile>,
}

impl BridgeCellFacts {
    pub fn has_flag(self, flag: u32) -> bool {
        self.raw_flags & flag != 0
    }

    pub fn has_structural_bridge(self) -> bool {
        self.has_flag(BRIDGE_FLAG_STRUCTURAL)
    }

    pub fn has_transition_flag(self) -> bool {
        self.has_flag(BRIDGE_FLAG_TRANSITION)
    }

    pub fn is_anchor_self(self) -> bool {
        self.has_flag(BRIDGE_FLAG_ANCHOR_SELF)
    }
}

pub fn high_bridge_stamp_for_overlay(id: u8) -> Option<(BridgeStampFamily, u8)> {
    match id {
        0x18 => Some((BridgeStampFamily::Nesw, 0)),
        0x19 => Some((BridgeStampFamily::Nesw, 6)),
        0xED => Some((BridgeStampFamily::Nwse, 0)),
        0xEE => Some((BridgeStampFamily::Nwse, 6)),
        _ => None,
    }
}

#[cfg(test)]
pub fn stamp_set_bridge_direction(
    cells: &mut [BridgeCellFacts],
    width: u16,
    height: u16,
    anchor: (u16, u16),
    family: BridgeStampFamily,
    direction: u8,
    set: bool,
) {
    if crate::util::direction::direction_delta(direction).is_none() {
        return;
    }
    let slots = stamp_slots(anchor, direction);
    for (slot, pos) in slots {
        let Some((rx, ry)) = pos else {
            continue;
        };
        let Some(idx) = index(width, height, rx, ry) else {
            continue;
        };
        if idx >= cells.len() {
            continue;
        }
        let relation = BridgeAnchorRelation {
            anchor,
            slot,
            family,
            direction,
        };
        apply_bridge_fact_slot(&mut cells[idx], slot, relation, set);
        if slot.writes_native_anchor() {
            cells[idx].native_anchor = set.then(|| {
                crate::map::cell_index::NativeCellIdentity::Real(
                    usize::from(anchor.1) * usize::from(width) + usize::from(anchor.0),
                )
            });
        }
    }
}

pub(crate) fn apply_bridge_fact_slot(
    cell: &mut BridgeCellFacts,
    slot: BridgeStampSlot,
    relation: BridgeAnchorRelation,
    set: bool,
) {
    if set {
        stamp_intact(cell, slot, relation);
    } else {
        stamp_destroy(cell, slot, relation.direction);
    }
}

fn stamp_intact(cell: &mut BridgeCellFacts, slot: BridgeStampSlot, relation: BridgeAnchorRelation) {
    let direction_zero = relation.direction == 0;
    apply_modeled_cellclass_bridge_slot(&mut cell.raw_flags, slot, true);
    match slot {
        BridgeStampSlot::Anchor => {
            cell.raw_flags &= !BRIDGE_FLAG_DESTROYED_OR_RAMP;
            cell.raw_flags |=
                BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_TRANSITION | BRIDGE_FLAG_EXTRA_SIDE;
            set_direction_zero_flag(cell, direction_zero);
            write_default_state(cell, relation.direction);
            attach(cell, relation);
        }
        BridgeStampSlot::Forward1 => {
            cell.raw_flags &= !BRIDGE_FLAG_DESTROYED_OR_RAMP;
            cell.raw_flags |= BRIDGE_FLAG_TRANSITION | BRIDGE_FLAG_EXTRA_SIDE;
            set_direction_zero_flag(cell, direction_zero);
            write_default_state(cell, relation.direction);
            attach(cell, relation);
        }
        BridgeStampSlot::Forward2 => {
            // This transverse stamp slot is structural but lacks the 0x200
            // entry flag; do not infer that flag from structural presence.
            cell.raw_flags &= !(BRIDGE_FLAG_TRANSITION | BRIDGE_FLAG_DESTROYED_OR_RAMP);
            cell.raw_flags |= BRIDGE_FLAG_EXTRA_SIDE;
            set_direction_zero_flag(cell, direction_zero);
            write_default_state(cell, relation.direction);
            attach(cell, relation);
        }
        BridgeStampSlot::Forward3 => {
            // The modeled flag subset was applied above.
        }
        BridgeStampSlot::Opposite => {
            cell.raw_flags &= !BRIDGE_FLAG_DESTROYED_OR_RAMP;
            cell.raw_flags |= BRIDGE_FLAG_TRANSITION | BRIDGE_FLAG_EXTRA_SIDE;
            set_direction_zero_flag(cell, direction_zero);
            write_default_state(cell, relation.direction);
            attach(cell, relation);
        }
        BridgeStampSlot::ExtraDir6 => {
            cell.raw_flags &= !BRIDGE_FLAG_EXTRA_SIDE;
            cell.raw_flags |= BRIDGE_FLAG_EXTRA_SIDE;
            attach(cell, relation);
        }
    }
}

fn stamp_destroy(cell: &mut BridgeCellFacts, slot: BridgeStampSlot, direction: u8) {
    apply_modeled_cellclass_bridge_slot(&mut cell.raw_flags, slot, false);
    match slot {
        BridgeStampSlot::Anchor => {
            cell.raw_flags &= !(BRIDGE_FLAG_ANCHOR_SELF
                | BRIDGE_FLAG_TRANSITION
                | BRIDGE_FLAG_DIRECTION_ZERO
                | BRIDGE_FLAG_EXTRA_SIDE);
            cell.raw_flags |= BRIDGE_FLAG_DESTROYED_OR_RAMP;
            set_direction_zero_flag(cell, direction == 0);
            cell.state_byte = 0;
            detach(cell);
        }
        BridgeStampSlot::Forward1 | BridgeStampSlot::Forward2 | BridgeStampSlot::Opposite => {
            cell.raw_flags &=
                !(BRIDGE_FLAG_TRANSITION | BRIDGE_FLAG_DIRECTION_ZERO | BRIDGE_FLAG_EXTRA_SIDE);
            cell.raw_flags |= BRIDGE_FLAG_DESTROYED_OR_RAMP;
            set_direction_zero_flag(cell, direction == 0);
            cell.state_byte = 0;
            detach(cell);
        }
        BridgeStampSlot::Forward3 => {
            // The modeled flag subset was applied above.
        }
        BridgeStampSlot::ExtraDir6 => {
            cell.raw_flags &= !BRIDGE_FLAG_EXTRA_SIDE;
            detach(cell);
        }
    }
}

fn attach(cell: &mut BridgeCellFacts, relation: BridgeAnchorRelation) {
    cell.family = relation.family;
    cell.direction = Some(relation.direction);
    cell.anchor = Some(relation);
}

fn detach(cell: &mut BridgeCellFacts) {
    cell.family = BridgeStampFamily::None;
    cell.direction = None;
    cell.anchor = None;
}

fn write_default_state(cell: &mut BridgeCellFacts, direction: u8) {
    cell.state_byte = if direction == 0 { 0 } else { 9 };
}

fn set_direction_zero_flag(cell: &mut BridgeCellFacts, set: bool) {
    if set {
        cell.raw_flags |= BRIDGE_FLAG_DIRECTION_ZERO;
    } else {
        cell.raw_flags &= !BRIDGE_FLAG_DIRECTION_ZERO;
    }
}

#[cfg(test)]
fn stamp_slots(anchor: (u16, u16), direction: u8) -> [(BridgeStampSlot, Option<(u16, u16)>); 6] {
    let f1 = step(anchor, direction);
    let f2 = f1.and_then(|cell| step(cell, direction));
    let f3 = f2.and_then(|cell| step(cell, direction));
    let opposite = crate::util::direction::opposite_direction(direction)
        .and_then(|opposite| step(anchor, opposite));
    let extra = if direction == 6 {
        opposite.and_then(|cell| step(cell, 2))
    } else {
        None
    };
    [
        (BridgeStampSlot::Anchor, Some(anchor)),
        (BridgeStampSlot::Forward1, f1),
        (BridgeStampSlot::Forward2, f2),
        (BridgeStampSlot::Forward3, f3),
        (BridgeStampSlot::Opposite, opposite),
        (BridgeStampSlot::ExtraDir6, extra),
    ]
}

#[cfg(test)]
fn index(width: u16, height: u16, rx: u16, ry: u16) -> Option<usize> {
    if rx >= width || ry >= height {
        return None;
    }
    Some(ry as usize * width as usize + rx as usize)
}

#[cfg(test)]
fn step(cell: (u16, u16), direction: u8) -> Option<(u16, u16)> {
    let (dx, dy) = crate::util::direction::direction_delta(direction)?;
    let rx = cell.0 as i32 + dx;
    let ry = cell.1 as i32 + dy;
    if rx < 0 || ry < 0 || rx > u16::MAX as i32 || ry > u16::MAX as i32 {
        return None;
    }
    Some((rx as u16, ry as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts_at(cells: &[BridgeCellFacts], width: u16, rx: u16, ry: u16) -> BridgeCellFacts {
        cells[ry as usize * width as usize + rx as usize]
    }

    #[test]
    fn stamp_dir0_intact_sets_anchor_north_slots_and_south_opposite() {
        let width = 12;
        let mut cells = vec![BridgeCellFacts::default(); 12 * 12];
        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            0,
            true,
        );

        assert!(facts_at(&cells, width as u16, 5, 5).has_flag(BRIDGE_FLAG_ANCHOR_SELF));
        assert!(facts_at(&cells, width as u16, 5, 4).has_structural_bridge());
        assert!(facts_at(&cells, width as u16, 5, 3).has_structural_bridge());
        assert!(facts_at(&cells, width as u16, 5, 2).has_flag(BRIDGE_FLAG_FORWARD_SIDE));
        assert!(!facts_at(&cells, width as u16, 5, 2).has_structural_bridge());
        assert!(facts_at(&cells, width as u16, 5, 6).has_structural_bridge());
        assert!(facts_at(&cells, width as u16, 5, 6).has_transition_flag());
    }

    #[test]
    fn stamp_dir6_intact_sets_west_slots_and_two_east_slots() {
        let width = 12;
        let mut cells = vec![BridgeCellFacts::default(); 12 * 12];
        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            6,
            true,
        );

        assert!(facts_at(&cells, width as u16, 4, 5).has_structural_bridge());
        assert!(facts_at(&cells, width as u16, 4, 5).has_transition_flag());
        assert!(facts_at(&cells, width as u16, 3, 5).has_structural_bridge());
        assert!(!facts_at(&cells, width as u16, 3, 5).has_transition_flag());
        assert!(facts_at(&cells, width as u16, 5, 5).has_transition_flag());
        assert!(facts_at(&cells, width as u16, 2, 5).has_flag(BRIDGE_FLAG_FORWARD_SIDE));
        assert!(facts_at(&cells, width as u16, 6, 5).has_structural_bridge());
        assert!(facts_at(&cells, width as u16, 6, 5).has_transition_flag());
        assert!(facts_at(&cells, width as u16, 7, 5).has_flag(BRIDGE_FLAG_EXTRA_SIDE));
        assert!(!facts_at(&cells, width as u16, 7, 5).has_structural_bridge());
    }

    #[test]
    fn invalid_direction_does_not_stamp_any_bridge_cells() {
        let width = 12;
        let mut cells = vec![BridgeCellFacts::default(); 12 * 12];

        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            9,
            true,
        );

        assert!(!facts_at(&cells, width as u16, 5, 5).has_flag(BRIDGE_FLAG_ANCHOR_SELF));
        assert!(!cells.iter().any(|cell| cell.raw_flags != 0));
    }

    #[test]
    fn stamp_intact_writes_default_state_bytes_before_overlay_data_overwrite() {
        let width = 12;
        let mut cells = vec![BridgeCellFacts::default(); 12 * 12];
        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            6,
            true,
        );

        for (rx, ry) in [(5, 5), (4, 5), (3, 5), (6, 5)] {
            assert_eq!(facts_at(&cells, width as u16, rx, ry).state_byte, 9);
        }
        assert_eq!(facts_at(&cells, width as u16, 2, 5).state_byte, 0);
        assert_eq!(facts_at(&cells, width as u16, 7, 5).state_byte, 0);
    }

    #[test]
    fn stamp_intact_sets_0x80_only_on_anchor() {
        let width = 12;
        let mut cells = vec![BridgeCellFacts::default(); 12 * 12];
        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            0,
            true,
        );

        assert!(facts_at(&cells, width as u16, 5, 5).has_flag(BRIDGE_FLAG_ANCHOR_SELF));
        for (rx, ry) in [(5, 4), (5, 3), (5, 2), (5, 6)] {
            assert!(!facts_at(&cells, width as u16, rx, ry).has_flag(BRIDGE_FLAG_ANCHOR_SELF));
        }
    }

    #[test]
    fn stamp_destroy_emits_destroy_flags_only_on_anchor_forward1_forward2_opposite() {
        let width = 12;
        let mut cells = vec![BridgeCellFacts::default(); 12 * 12];
        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            6,
            true,
        );
        stamp_set_bridge_direction(
            &mut cells,
            width as u16,
            12,
            (5, 5),
            BridgeStampFamily::Nesw,
            6,
            false,
        );

        for (rx, ry) in [(5, 5), (4, 5), (3, 5), (6, 5)] {
            assert!(facts_at(&cells, width as u16, rx, ry).has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP));
        }
        assert!(!facts_at(&cells, width as u16, 2, 5).has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP));
        assert!(!facts_at(&cells, width as u16, 7, 5).has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP));
    }

    #[test]
    fn bridge_flags_newtype_matches_const_predicates() {
        // Single-source check: the typed newtype must agree bit-for-bit with the
        // `BridgeCellFacts` predicate path for the same raw flag word, across
        // each individual bit and a combined word.
        for raw in [
            0u32,
            BRIDGE_FLAG_ANCHOR_SELF,
            BRIDGE_FLAG_STRUCTURAL,
            BRIDGE_FLAG_TRANSITION,
            BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_TRANSITION,
            BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_FORWARD_SIDE,
        ] {
            let flags = BridgeFlags(raw);
            let facts = BridgeCellFacts {
                raw_flags: raw,
                ..BridgeCellFacts::default()
            };
            assert_eq!(
                flags.anchor(),
                facts.is_anchor_self(),
                "anchor raw={raw:#x}"
            );
            assert_eq!(
                flags.structural(),
                facts.has_structural_bridge(),
                "structural raw={raw:#x}"
            );
            assert_eq!(
                flags.bridgehead(),
                facts.has_transition_flag(),
                "bridgehead raw={raw:#x}"
            );
        }
    }

    #[test]
    fn high_bridge_stamp_classifier_ignores_low_bridge_ids() {
        for id in [0x18, 0x19, 0xED, 0xEE] {
            assert!(high_bridge_stamp_for_overlay(id).is_some());
        }
        for id in [0x4A, 0x7A, 0xCD, 0xE9] {
            assert_eq!(high_bridge_stamp_for_overlay(id), None);
        }
    }

    #[test]
    fn gsi_04_01_shared_dummy_subset_preserves_unrelated_cell_attributes() {
        let unrelated = BRIDGE_FLAG_TRANSITION
            | BRIDGE_FLAG_DESTROYED_OR_RAMP
            | BRIDGE_FLAG_DIRECTION_ZERO
            | BRIDGE_FLAG_EXTRA_SIDE;
        let mut flags = unrelated;

        apply_modeled_cellclass_bridge_slot(&mut flags, BridgeStampSlot::Anchor, true);
        assert_eq!(flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK, 0x1180);
        assert_eq!(flags & !MODELED_CELLCLASS_BRIDGE_FLAG_MASK, unrelated);

        apply_modeled_cellclass_bridge_slot(&mut flags, BridgeStampSlot::Anchor, false);
        assert_eq!(flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK, 0);
        assert_eq!(flags & !MODELED_CELLCLASS_BRIDGE_FLAG_MASK, unrelated);
    }
}

/// Bridge body axis. Body cells are stacked along this axis; ramps face
/// perpendicular.
///
/// Mapping: `Axis::EW` ↔ `BridgeDirection::EastWest` ↔ state byte 9–17;
/// `Axis::NS` ↔ `BridgeDirection::NorthSouth` ↔ state byte 0–8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Axis {
    /// Body cells stacked north–south (along Y); ramps face east/west.
    /// State byte range 0–8.
    NS,
    /// Body cells stacked east–west (along X); ramps face north/south.
    /// State byte range 9–17.
    EW,
}

/// Per-cell anchor tile-class for bridgehead-adjacent cells.
///
/// Mirrors the four `IsoTileTypeIndex` slots used by the bridgehead state
/// machine. Each value corresponds to a BridgeSet-relative tile_id offset
/// (slot 0..3); the actual tile_ids are theater-portable via
/// `BridgeMiddle1` / `BridgeMiddle2`.
///
/// - `Variant0` — pristine bridgehead (map-load default for cells with no
///   author-damaged anchor placement).
/// - `Variant1` — first DamageB intermediate. Reached only via neighbor
///   `UpdateRamp_*_DamageB` progression on a Variant0 target.
/// - `Damaged` — second DamageB intermediate. Reached only via neighbor
///   `UpdateRamp_*_DamageB` progression on a Variant1 target. Also written
///   by Collapse* paths advancing any non-AboutToFall variant.
/// - `AboutToFall` — most-damaged variant. Two reach paths:
///   1. **Direct hit on a bridgehead cell** — the bridgehead state machine
///      writes the anchor straight to this slot (skipping Variant1/Damaged).
///   2. **Map-load author-damaged anchor** — maps may place this tile_id
///      directly; the renderer reflects it from frame 1.
///
/// Meaningful only when `BridgeRuntimeCell.role` is `Anchor` or
/// `Bridgehead`; the renderer ignores it on other roles.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum BridgeheadAnchorClass {
    #[default]
    Variant0,
    Variant1,
    Damaged,
    AboutToFall,
}
