//! Overlay-direct bridge destruction and repair walkers.
//!
//! Drives full-bridge collapse from a single hit on a cell whose overlay byte
//! is still in the raw body range (per the verification doc, raw-overlay
//! cells route here, not through the state machine). Distinct from the
//! state-machine drivers in `bridge_state/mod.rs`, which handle the
//! late-stage progression after overlays have been transitioned.
//!
//! ## Dependency rules
//! Same as sim/: depends on rules/ + map/; never render / ui / audio / net.

#[cfg(test)]
use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
#[cfg(test)]
use crate::sim::bridge_state::RepairOutcome;
use crate::sim::bridge_state::{Axis, BridgeRuntimeState, StateOutcome};
#[cfg(test)]
use crate::sim::rng::SimRng;

#[cfg(test)]
const REPAIR_VARIANT_LIMIT_INCLUSIVE: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairFamily {
    LowNs,
    LowEw,
    HighNs,
    HighEw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepairTransition {
    NoChange,
    Fixed(u8),
    RandomHealthy { base: u8 },
}

impl BridgeRuntimeState {
    pub(crate) fn is_low_destroy_overlay(overlay: u8) -> bool {
        (0x4A..=0x65).contains(&overlay)
    }

    pub(crate) fn is_high_destroy_overlay(overlay: u8) -> bool {
        (0xCD..=0xE8).contains(&overlay)
    }

    pub(crate) fn low_destroy_overlay_axis(overlay: u8) -> Option<Axis> {
        if Self::is_ns_walker_overlay_low(overlay) {
            Some(Axis::NS)
        } else if Self::is_ew_walker_overlay_low(overlay) {
            Some(Axis::EW)
        } else {
            None
        }
    }

    pub(crate) fn high_destroy_overlay_axis(overlay: u8) -> Option<Axis> {
        if Self::is_ns_walker_overlay_high(overlay) {
            Some(Axis::NS)
        } else if Self::is_ew_walker_overlay_high(overlay) {
            Some(Axis::EW)
        } else {
            None
        }
    }

    /// Engineer repair entry for a gamemd-style 5x5 scan. The outer dispatch
    /// chooses LOW if any scanned cell is a low bridge tile/ramp or low bridge
    /// overlay; otherwise it scans the HIGH overlay family.
    #[cfg(test)]
    pub fn repair_bridge_from_engineer_scan(
        &mut self,
        scan_cells: &[(u16, u16)],
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let low_candidate = scan_cells.iter().any(|&(rx, ry)| {
            let overlay = self.cell(rx, ry).map(|c| c.overlay_byte);
            Self::is_low_repair_outer_candidate(overlay, terrain.cell(rx, ry))
        });

        if low_candidate {
            self.repair_bridge_low_from_scan(scan_cells, rng, terrain)
        } else {
            self.repair_bridge_high_from_scan(scan_cells, rng, terrain)
        }
    }

    #[cfg(test)]
    fn repair_bridge_low_from_scan(
        &mut self,
        scan_cells: &[(u16, u16)],
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        for &(rx, ry) in scan_cells {
            let Some(overlay) = self.cell(rx, ry).map(|c| c.overlay_byte) else {
                continue;
            };
            if Self::is_low_repair_overlay(overlay) {
                return self.repair_bridge_low(rx, ry, rng, terrain);
            }
        }
        RepairOutcome::default()
    }

    #[cfg(test)]
    fn repair_bridge_high_from_scan(
        &mut self,
        scan_cells: &[(u16, u16)],
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        for &(rx, ry) in scan_cells {
            let Some(overlay) = self.cell(rx, ry).map(|c| c.overlay_byte) else {
                continue;
            };
            if Self::is_high_repair_overlay(overlay) {
                return self.repair_bridge_high(rx, ry, rng, terrain);
            }
        }
        RepairOutcome::default()
    }

    /// `MapClass::RepairBridge_Low` 0x0057F200 — the repair-side twin of
    /// `DestroyBridge_Low` 0x0057BAA0. Same axis classes, same three-case
    /// start-cell shift, dispatching into the repair walkers instead of the
    /// destroy ones.
    #[cfg(test)]
    fn repair_bridge_low(
        &mut self,
        rx: u16,
        ry: u16,
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let Some(overlay) = self.cell(rx, ry).map(|c| c.overlay_byte) else {
            return RepairOutcome::default();
        };
        if Self::is_ns_walker_overlay_low(overlay) {
            let (sx, sy) = self.find_walker_start_low_ns(rx, ry);
            return self.repair_bridge_walker_ns_low(sx, sy, rng, terrain);
        }
        if Self::is_ew_walker_overlay_low(overlay) {
            let (sx, sy) = self.find_walker_start_low_ew(rx, ry);
            return self.repair_bridge_walker_ew_low(sx, sy, rng, terrain);
        }
        RepairOutcome::default()
    }

    /// `MapClass::RepairBridge_High` 0x0057F440. Verified by decompile: it
    /// splits on NS = 0xCD..=0xD5 u 0xDF..=0xE2 u {0xE7} versus
    /// EW = 0xD6..=0xDE u 0xE3..=0xE6 u {0xE8} — the same classes
    /// `DestroyBridge_High` 0x0057CCF0 uses — then shifts the start cell by
    /// probing the two neighbours against the union band 0xCD..=0xE8 before
    /// entering the walker.
    #[cfg(test)]
    fn repair_bridge_high(
        &mut self,
        rx: u16,
        ry: u16,
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let Some(overlay) = self.cell(rx, ry).map(|c| c.overlay_byte) else {
            return RepairOutcome::default();
        };
        if Self::is_ns_walker_overlay_high(overlay) {
            let (sx, sy) = self.find_walker_start_high_ns(rx, ry);
            return self.repair_bridge_walker_ns_high(sx, sy, rng, terrain);
        }
        if Self::is_ew_walker_overlay_high(overlay) {
            let (sx, sy) = self.find_walker_start_high_ew(rx, ry);
            return self.repair_bridge_walker_ew_high(sx, sy, rng, terrain);
        }
        RepairOutcome::default()
    }

    /// `MapClass::RepairBridgeWalker_NS_Low` 0x0057F6A0.
    #[cfg(test)]
    fn repair_bridge_walker_ns_low(
        &mut self,
        sx: u16,
        sy: u16,
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let mut outcome = RepairOutcome::default();
        let mut x = sx;
        while x > 0
            && self
                .cell(x - 1, sy)
                .map(|c| Self::is_low_repair_overlay(c.overlay_byte))
                .unwrap_or(false)
        {
            x -= 1;
        }
        loop {
            let Some(overlay) = self.cell(x, sy).map(|c| c.overlay_byte) else {
                break;
            };
            if !Self::is_low_repair_overlay(overlay) {
                break;
            }
            self.apply_repair_to_strip_cell(
                Self::ns_triple(x, sy),
                RepairFamily::LowNs,
                rng,
                terrain,
                &mut outcome,
            );
            let Some(next_x) = x.checked_add(1) else {
                break;
            };
            x = next_x;
        }
        outcome
    }

    /// `MapClass::RepairBridgeWalker_EW_Low` 0x0057FBC0.
    #[cfg(test)]
    fn repair_bridge_walker_ew_low(
        &mut self,
        sx: u16,
        sy: u16,
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let mut outcome = RepairOutcome::default();
        let mut y = sy;
        while y > 0
            && self
                .cell(sx, y - 1)
                .map(|c| Self::is_low_repair_overlay(c.overlay_byte))
                .unwrap_or(false)
        {
            y -= 1;
        }
        loop {
            let Some(overlay) = self.cell(sx, y).map(|c| c.overlay_byte) else {
                break;
            };
            if !Self::is_low_repair_overlay(overlay) {
                break;
            }
            self.apply_repair_to_strip_cell(
                Self::ew_triple(sx, y),
                RepairFamily::LowEw,
                rng,
                terrain,
                &mut outcome,
            );
            let Some(next_y) = y.checked_add(1) else {
                break;
            };
            y = next_y;
        }
        outcome
    }

    /// `MapClass::RepairBridgeWalker_NS_High` 0x005800D0.
    #[cfg(test)]
    fn repair_bridge_walker_ns_high(
        &mut self,
        sx: u16,
        sy: u16,
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let mut outcome = RepairOutcome::default();
        let mut x = sx;
        while x > 0
            && self
                .cell(x - 1, sy)
                .map(|c| Self::is_high_repair_overlay(c.overlay_byte))
                .unwrap_or(false)
        {
            x -= 1;
        }
        loop {
            let Some(overlay) = self.cell(x, sy).map(|c| c.overlay_byte) else {
                break;
            };
            if !Self::is_high_repair_overlay(overlay) {
                break;
            }
            self.apply_repair_to_strip_cell(
                Self::ns_triple(x, sy),
                RepairFamily::HighNs,
                rng,
                terrain,
                &mut outcome,
            );
            let Some(next_x) = x.checked_add(1) else {
                break;
            };
            x = next_x;
        }
        outcome
    }

    /// `MapClass::RepairBridgeWalker_EW_High` 0x00580600.
    #[cfg(test)]
    fn repair_bridge_walker_ew_high(
        &mut self,
        sx: u16,
        sy: u16,
        rng: &mut SimRng,
        terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let mut outcome = RepairOutcome::default();
        let mut y = sy;
        while y > 0
            && self
                .cell(sx, y - 1)
                .map(|c| Self::is_high_repair_overlay(c.overlay_byte))
                .unwrap_or(false)
        {
            y -= 1;
        }
        loop {
            let Some(overlay) = self.cell(sx, y).map(|c| c.overlay_byte) else {
                break;
            };
            if !Self::is_high_repair_overlay(overlay) {
                break;
            }
            self.apply_repair_to_strip_cell(
                Self::ew_triple(sx, y),
                RepairFamily::HighEw,
                rng,
                terrain,
                &mut outcome,
            );
            let Some(next_y) = y.checked_add(1) else {
                break;
            };
            y = next_y;
        }
        outcome
    }

    #[cfg(test)]
    fn apply_repair_to_strip_cell(
        &mut self,
        triple: [Option<(u16, u16)>; 3],
        family: RepairFamily,
        rng: &mut SimRng,
        _terrain: &ResolvedTerrainGrid,
        outcome: &mut RepairOutcome,
    ) {
        let Some((crx, cry)) = triple[0] else {
            return;
        };
        let Some(prior_overlay) = self.cell(crx, cry).map(|c| c.overlay_byte) else {
            return;
        };

        let transition = Self::repair_transition(prior_overlay, family);
        let new_overlay = match transition {
            RepairTransition::NoChange => return,
            RepairTransition::Fixed(new_overlay) => {
                if new_overlay == prior_overlay {
                    return;
                }
                new_overlay
            }
            RepairTransition::RandomHealthy { base } => {
                let variant = Self::repair_variant_offset(rng);
                outcome.zones_dirty = true;
                base + variant
            }
        };

        let mut touched = Vec::with_capacity(3);
        let mut spans = std::collections::BTreeSet::new();
        for pos in triple.into_iter().flatten() {
            let overlay_changed =
                self.write_overlay_byte_deferred_recalc(pos.0, pos.1, new_overlay);
            if let Some(cell) = self.cell_mut(pos.0, pos.1) {
                if overlay_changed {
                    outcome.repaired_cells += 1;
                }
                if let Some(span_id) = cell.anchor_span_id {
                    spans.insert(span_id);
                }
                touched.push(pos);
            }
        }
        for &(rx, ry) in &touched {
            self.queue_overlay_recalc(rx, ry);
        }

        // Original ordinary repair walkers rewrite overlay/Recalc only. They
        // do not call56E990; pavement clearing belongs to repair-or-ramp's
        // fallback and the pavement span walker, at their own call sites.
        if matches!(prior_overlay, 0x64 | 0x65 | 0xE7 | 0xE8) {
            super::extend_unique_cells(&mut outcome.radar_cells, touched.iter().copied());
        }
        for span_id in spans {
            self.sync_anchor_span_damage_state(span_id);
        }
    }

    #[cfg(test)]
    fn sync_anchor_span_damage_state(&mut self, span_id: u16) {
        let anchor_pos = self.anchor_span(span_id).map(|span| span.anchor);
        if let Some((arx, ary)) = anchor_pos {
            let anchor_state = self.cell(arx, ary).map(|cell| cell.damage_state);
            if let (Some(state), Some(span)) = (anchor_state, self.anchor_span_mut(span_id)) {
                span.damage_state = state;
            }
        }
    }

    #[cfg(test)]
    fn repair_transition(overlay: u8, family: RepairFamily) -> RepairTransition {
        match family {
            RepairFamily::LowNs => match overlay {
                0x4E..=0x52 | 0x64 => RepairTransition::RandomHealthy { base: 0x4A },
                0x5C | 0x5D => RepairTransition::Fixed(0x5C),
                0x5E | 0x5F => RepairTransition::Fixed(0x5E),
                _ => RepairTransition::NoChange,
            },
            RepairFamily::LowEw => match overlay {
                0x57..=0x5B | 0x65 => RepairTransition::RandomHealthy { base: 0x53 },
                0x60 | 0x61 => RepairTransition::Fixed(0x60),
                0x62 | 0x63 => RepairTransition::Fixed(0x62),
                _ => RepairTransition::NoChange,
            },
            RepairFamily::HighNs => match overlay {
                0xD1..=0xD5 | 0xE7 => RepairTransition::RandomHealthy { base: 0xCD },
                0xDF | 0xE0 => RepairTransition::Fixed(0xDF),
                0xE1 | 0xE2 => RepairTransition::Fixed(0xE1),
                _ => RepairTransition::NoChange,
            },
            RepairFamily::HighEw => match overlay {
                0xDA..=0xDE | 0xE8 => RepairTransition::RandomHealthy { base: 0xD6 },
                0xE3 | 0xE4 => RepairTransition::Fixed(0xE3),
                0xE5 | 0xE6 => RepairTransition::Fixed(0xE5),
                _ => RepairTransition::NoChange,
            },
        }
    }

    /// Pick the healthy-tile variant for a repaired bridge strip. gamemd draws
    /// `RandomRanged(0, 3)` from the map-gen RNG using the multiply-high (scaled)
    /// shape — the HIGH two bits of one draw, not the low bits — so the variant
    /// comes from `next_range_u32_inclusive_scaled`. VERA fixed-map construction
    /// currently retains `Seed(0)`; the native fresh-process state is verified,
    /// while cross-match process retention remains UNCHECKED. Accepted generated
    /// maps retain their post-RMG cursor for the current match.
    #[cfg(test)]
    fn repair_variant_offset(rng: &mut SimRng) -> u8 {
        rng.next_range_u32_inclusive_scaled(0, u32::from(REPAIR_VARIANT_LIMIT_INCLUSIVE)) as u8
    }

    #[cfg(test)]
    fn is_low_repair_outer_candidate(
        overlay: Option<u8>,
        terrain_cell: Option<&ResolvedTerrainCell>,
    ) -> bool {
        overlay.map(Self::is_low_repair_overlay).unwrap_or(false)
            || terrain_cell
                .map(|cell| cell.is_wood_bridge_repair_tile)
                .unwrap_or(false)
    }

    #[cfg(test)]
    fn is_low_repair_overlay(overlay: u8) -> bool {
        (0x4A..=0x65).contains(&overlay)
    }

    #[cfg(test)]
    fn is_high_repair_overlay(overlay: u8) -> bool {
        (0xCD..=0xE8).contains(&overlay)
    }

    /// `DestroyBridge_High` 0x0057CCF0. Its axis classes are
    /// NS = 0xCD..=0xD5 u 0xDF..=0xE2 u {0xE7} and
    /// EW = 0xD6..=0xDE u 0xE3..=0xE6 u {0xE8}; their union is the band
    /// [`Self::is_high_destroy_overlay`] tests, and the neighbour probes inside
    /// the native function use that same union.
    ///
    /// Overlay-direct HIGH walker entry. Three responsibilities:
    /// 1. Classify the input cell's overlay byte to pick NS or EW walker.
    /// 2. Pre-walk start-cell shift: read the body-axis neighbors to find
    ///    a stable mid before walking. Multiple hits on different cells of
    ///    the same bridge converge to the same walker start.
    /// 3. Forward the shifted coord to the appropriate walker.
    ///
    /// Returns the walker's outcome (`Collapsed` once Task 7 lands;
    /// `NoChange` for now). When the input overlay is not in the HIGH body
    /// range, the entry returns `NoChange` without touching state.
    pub fn destroy_bridge_high(
        &mut self,
        rx: u16,
        ry: u16,
        terrain: &ResolvedTerrainGrid,
    ) -> StateOutcome {
        let Some(cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };
        let overlay = cell.overlay_byte;
        if !Self::is_high_destroy_overlay(overlay) {
            return StateOutcome::NoChange;
        }
        if Self::is_ns_walker_overlay_high(overlay) {
            let (sx, sy) = self.find_walker_start_high_ns(rx, ry);
            return self.destroy_bridge_walker_ns_high(sx, sy, terrain);
        }
        if Self::is_ew_walker_overlay_high(overlay) {
            let (sx, sy) = self.find_walker_start_high_ew(rx, ry);
            return self.destroy_bridge_walker_ew_high(sx, sy, terrain);
        }
        StateOutcome::NoChange
    }

    /// `DestroyBridge_Low` 0x0057BAA0. Its axis classes are
    /// NS = 0x4A..=0x52 u 0x5C..=0x5F u {0x64} and
    /// EW = 0x53..=0x5B u 0x60..=0x63 u {0x65}; their union is the band
    /// [`Self::is_low_destroy_overlay`] tests.
    ///
    /// Overlay-direct LOW walker entry. Same shape as `destroy_bridge_high`
    /// with overlay ranges shifted to the LOW body range
    /// (`[0x4A..=0x65]`).
    pub fn destroy_bridge_low(
        &mut self,
        rx: u16,
        ry: u16,
        terrain: &ResolvedTerrainGrid,
    ) -> StateOutcome {
        let Some(cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };
        let overlay = cell.overlay_byte;
        if !Self::is_low_destroy_overlay(overlay) {
            return StateOutcome::NoChange;
        }
        if Self::is_ns_walker_overlay_low(overlay) {
            let (sx, sy) = self.find_walker_start_low_ns(rx, ry);
            return self.destroy_bridge_walker_ns_low(sx, sy, terrain);
        }
        if Self::is_ew_walker_overlay_low(overlay) {
            let (sx, sy) = self.find_walker_start_low_ew(rx, ry);
            return self.destroy_bridge_walker_ew_low(sx, sy, terrain);
        }
        StateOutcome::NoChange
    }

    // ----- Pre-walk start-cell shift (mirror of binary's 3-case neighbor
    // check at the walker entries). -----

    fn find_walker_start_high_ns(&self, rx: u16, ry: u16) -> (u16, u16) {
        let in_range = |o: u8| (0xCD..=0xE8).contains(&o);
        // Body-axis neighbor 1 north of input.
        let north_off = ry == 0
            || self
                .cell(rx, ry - 1)
                .map(|c| !in_range(c.overlay_byte))
                .unwrap_or(true);
        if north_off {
            return (rx, ry.saturating_add(1));
        }
        // Body-axis neighbor 2 north of input.
        let north2_on = ry >= 2
            && self
                .cell(rx, ry - 2)
                .map(|c| in_range(c.overlay_byte))
                .unwrap_or(false);
        if north2_on {
            return (rx, ry - 1);
        }
        (rx, ry)
    }

    fn find_walker_start_high_ew(&self, rx: u16, ry: u16) -> (u16, u16) {
        let in_range = |o: u8| (0xCD..=0xE8).contains(&o);
        let west_off = rx == 0
            || self
                .cell(rx - 1, ry)
                .map(|c| !in_range(c.overlay_byte))
                .unwrap_or(true);
        if west_off {
            return (rx.saturating_add(1), ry);
        }
        let west2_on = rx >= 2
            && self
                .cell(rx - 2, ry)
                .map(|c| in_range(c.overlay_byte))
                .unwrap_or(false);
        if west2_on {
            return (rx - 1, ry);
        }
        (rx, ry)
    }

    fn find_walker_start_low_ns(&self, rx: u16, ry: u16) -> (u16, u16) {
        let in_range = |o: u8| (0x4A..=0x65).contains(&o);
        let north_off = ry == 0
            || self
                .cell(rx, ry - 1)
                .map(|c| !in_range(c.overlay_byte))
                .unwrap_or(true);
        if north_off {
            return (rx, ry.saturating_add(1));
        }
        let north2_on = ry >= 2
            && self
                .cell(rx, ry - 2)
                .map(|c| in_range(c.overlay_byte))
                .unwrap_or(false);
        if north2_on {
            return (rx, ry - 1);
        }
        (rx, ry)
    }

    fn find_walker_start_low_ew(&self, rx: u16, ry: u16) -> (u16, u16) {
        let in_range = |o: u8| (0x4A..=0x65).contains(&o);
        let west_off = rx == 0
            || self
                .cell(rx - 1, ry)
                .map(|c| !in_range(c.overlay_byte))
                .unwrap_or(true);
        if west_off {
            return (rx.saturating_add(1), ry);
        }
        let west2_on = rx >= 2
            && self
                .cell(rx - 2, ry)
                .map(|c| in_range(c.overlay_byte))
                .unwrap_or(false);
        if west2_on {
            return (rx - 1, ry);
        }
        (rx, ry)
    }

    // ----- Axis classification by overlay byte. -----

    pub(super) fn is_ns_walker_overlay_high(overlay: u8) -> bool {
        // HIGH NS axis sub-range:
        //   [0xCD..=0xD5] ∪ [0xDF..=0xE2] ∪ {0xE7}
        (0xCD..=0xD5).contains(&overlay) || (0xDF..=0xE2).contains(&overlay) || overlay == 0xE7
    }

    pub(super) fn is_ew_walker_overlay_high(overlay: u8) -> bool {
        // HIGH EW axis sub-range:
        //   [0xD6..=0xDE] ∪ [0xE3..=0xE6] ∪ {0xE8}
        (0xD6..=0xDE).contains(&overlay) || (0xE3..=0xE6).contains(&overlay) || overlay == 0xE8
    }

    pub(super) fn is_ns_walker_overlay_low(overlay: u8) -> bool {
        // LOW NS axis sub-range:
        //   [0x4A..=0x52] ∪ [0x5C..=0x5F] ∪ {0x64}
        (0x4A..=0x52).contains(&overlay) || (0x5C..=0x5F).contains(&overlay) || overlay == 0x64
    }

    pub(super) fn is_ew_walker_overlay_low(overlay: u8) -> bool {
        // LOW EW axis sub-range:
        //   [0x53..=0x5B] ∪ [0x60..=0x63] ∪ {0x65}
        (0x53..=0x5B).contains(&overlay) || (0x60..=0x63).contains(&overlay) || overlay == 0x65
    }

    // ----- Perpendicular neighbor classifiers used by sibling cascade. -----
    //
    // Indexed table is `pick_destruction_overlay(idx, axis, is_high)`, which
    // already ships in `bridge_specs.rs`. NS body cells consult the EW
    // classifier (perpendicular = west/east); EW body cells consult the NS
    // classifier (perpendicular = north/south).

    /// Classify the EW-axis perpendicular pattern at `(rx, ry)`. Reads the
    /// west and east neighbors' overlay bytes and returns a 0..=15 index.
    /// Bit assignment (matches the binary's switch order at the EW
    /// classifier — east-first, then west):
    /// - bit 0 (val 1): east in `{0xD1, 0xD3, 0xD5, 0xE0}`
    /// - bit 1 (val 2): east in `{0xD4, 0xE7}`
    /// - bit 2 (val 4): west in `{0xD2, 0xD3, 0xD4, 0xE2}`
    /// - bit 3 (val 8): west in `{0xD5, 0xE7}`
    /// `MapClass::CheckBridgeNeighbors_EW_High` 0x0057CAB0 — the west/east
    /// twin of 0x0057CBE0.
    pub(super) fn check_bridge_neighbors_ew_high(&self, rx: u16, ry: u16) -> u8 {
        let east = self
            .cell(rx.saturating_add(1), ry)
            .map(|c| c.overlay_byte)
            .unwrap_or(0);
        let west = if rx > 0 {
            self.cell(rx - 1, ry).map(|c| c.overlay_byte).unwrap_or(0)
        } else {
            0
        };
        let mut idx = 0u8;
        match east {
            0xD1 | 0xD3 | 0xD5 | 0xE0 => idx |= 1,
            0xD4 | 0xE7 => idx |= 2,
            _ => {}
        }
        match west {
            0xD2 | 0xD3 | 0xD4 | 0xE2 => idx |= 4,
            0xD5 | 0xE7 => idx |= 8,
            _ => {}
        }
        idx
    }

    /// Classify the NS-axis perpendicular pattern at `(rx, ry)`. Reads the
    /// north and south neighbors' overlay bytes and returns a 0..=15 index.
    /// Bit assignment (matches the binary — north-first, then south):
    /// - bit 0 (val 1): north in `{0xDA, 0xDC, 0xDE, 0xE4}`
    /// - bit 1 (val 2): north in `{0xDD, 0xE8}`
    /// - bit 2 (val 4): south in `{0xDB, 0xDC, 0xDD, 0xE6}`
    /// - bit 3 (val 8): south in `{0xDE, 0xE8}`
    /// `MapClass::CheckBridgeNeighbors_NS_High` 0x0057CBE0. Reads the north
    /// and south neighbours' overlay bytes into a 4-bit index: north in
    /// {0xDA, 0xDC, 0xDE, 0xE4} sets bit 0 and {0xDD, 0xE8} sets bit 1; south
    /// in {0xDB, 0xDC, 0xDD, 0xE6} sets bit 2 and {0xDE, 0xE8} sets bit 3.
    /// The native body returns early on the bit-2 case, which is equivalent
    /// here because the two south sets are disjoint.
    pub(super) fn check_bridge_neighbors_ns_high(&self, rx: u16, ry: u16) -> u8 {
        let north = if ry > 0 {
            self.cell(rx, ry - 1).map(|c| c.overlay_byte).unwrap_or(0)
        } else {
            0
        };
        let south = self
            .cell(rx, ry.saturating_add(1))
            .map(|c| c.overlay_byte)
            .unwrap_or(0);
        let mut idx = 0u8;
        match north {
            0xDA | 0xDC | 0xDE | 0xE4 => idx |= 1,
            0xDD | 0xE8 => idx |= 2,
            _ => {}
        }
        match south {
            0xDB | 0xDC | 0xDD | 0xE6 => idx |= 4,
            0xDE | 0xE8 => idx |= 8,
            _ => {}
        }
        idx
    }

    // ----- Cell-triple iteration helpers. -----

    fn ns_triple(rx: u16, ry: u16) -> [Option<(u16, u16)>; 3] {
        let north = if ry > 0 { Some((rx, ry - 1)) } else { None };
        let south = Some((rx, ry.saturating_add(1)));
        [Some((rx, ry)), north, south]
    }

    fn ew_triple(rx: u16, ry: u16) -> [Option<(u16, u16)>; 3] {
        let west = if rx > 0 { Some((rx - 1, ry)) } else { None };
        let east = Some((rx.saturating_add(1), ry));
        [Some((rx, ry)), west, east]
    }

    /// Main low-damage walkers write both perpendicular neighbors before the
    /// center cell, then defer center-first RecalcAttributes until after their
    /// sibling-cascade calls return.
    fn ns_low_root_write_order(rx: u16, ry: u16) -> [Option<(u16, u16)>; 3] {
        let [center, north, south] = Self::ns_triple(rx, ry);
        [north, south, center]
    }

    fn ew_low_root_write_order(rx: u16, ry: u16) -> [Option<(u16, u16)>; 3] {
        let [center, west, east] = Self::ew_triple(rx, ry);
        [west, east, center]
    }

    // ----- Sibling-cascade leaves (`apply_bridge_destruction_*_high`). -----

    /// Sibling-cascade leaf for the NS body axis. Validates `(rx, ry)` is
    /// in the HIGH overlay range, computes the perpendicular neighbor
    /// pattern via `check_bridge_neighbors_ew_high`, looks up next-overlay
    /// via the shipped `pick_destruction_overlay` table (HIGH NS), and
    /// writes the (this, north, south) length-axis triple. Returns the
    /// list of cells that hit final-collapse (overlay 0xE7) so the caller
    /// can emit BlowUpBridge actions.
    fn apply_bridge_destruction_ns_high(
        &mut self,
        rx: u16,
        ry: u16,
        radar: &mut Vec<(u16, u16)>,
    ) -> Vec<(u16, u16)> {
        use crate::sim::bridge_specs::pick_destruction_overlay;
        use crate::sim::bridge_state::{Axis, DamageState};

        let mut final_cells = Vec::new();
        let Some(cell) = self.cell(rx, ry).copied() else {
            return final_cells;
        };
        let cur = cell.overlay_byte;
        // Outer overlay gate: HIGH range.
        if !(0xCD..=0xE8).contains(&cur) {
            return final_cells;
        }

        let idx = self.check_bridge_neighbors_ew_high(rx, ry);
        // idx == 0 means no perpendicular pattern; cascade leaf is a no-op.
        if idx == 0 {
            return final_cells;
        }

        // Two-stage progression: table lookup for cur < 0xDF, then
        // intermediate fixed transitions for 0xDF/0xE1.
        let next = if cur < 0xDF {
            match pick_destruction_overlay(idx, Axis::NS, true) {
                Some(n) if n != cur => n,
                _ => return final_cells,
            }
        } else if cur == 0xDF {
            0xE0
        } else if cur == 0xE1 {
            0xE2
        } else {
            // 0xE0, 0xE2, 0xE3..0xE8: no further transition at this cell.
            return final_cells;
        };

        for slot in Self::ns_triple(rx, ry) {
            if let Some(pos) = slot {
                let _ = self.write_overlay_byte(pos.0, pos.1, next);
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    // Every touched cell (final OR intermediate) is minimap-dirty.
                    radar.push(pos);
                    if next == 0xE7 {
                        c.damage_state = DamageState::Destroyed;
                        final_cells.push(pos);
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }
        final_cells
    }

    /// Sibling-cascade leaf for the EW body axis. Mirror of
    /// `apply_bridge_destruction_ns_high` with EW axis classifier, EW
    /// table, EW intermediates (0xE3 → 0xE4, 0xE5 → 0xE6), and final 0xE8.
    fn apply_bridge_destruction_ew_high(
        &mut self,
        rx: u16,
        ry: u16,
        radar: &mut Vec<(u16, u16)>,
    ) -> Vec<(u16, u16)> {
        use crate::sim::bridge_specs::pick_destruction_overlay;
        use crate::sim::bridge_state::{Axis, DamageState};

        let mut final_cells = Vec::new();
        let Some(cell) = self.cell(rx, ry).copied() else {
            return final_cells;
        };
        let cur = cell.overlay_byte;
        if !(0xCD..=0xE8).contains(&cur) {
            return final_cells;
        }

        let idx = self.check_bridge_neighbors_ns_high(rx, ry);
        if idx == 0 {
            return final_cells;
        }

        let next = if cur < 0xE3 {
            match pick_destruction_overlay(idx, Axis::EW, true) {
                Some(n) if n != cur => n,
                _ => return final_cells,
            }
        } else if cur == 0xE3 {
            0xE4
        } else if cur == 0xE5 {
            0xE6
        } else {
            return final_cells;
        };

        for slot in Self::ew_triple(rx, ry) {
            if let Some(pos) = slot {
                let _ = self.write_overlay_byte(pos.0, pos.1, next);
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    // Every touched cell (final OR intermediate) is minimap-dirty.
                    radar.push(pos);
                    if next == 0xE8 {
                        c.damage_state = DamageState::Destroyed;
                        final_cells.push(pos);
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }
        final_cells
    }

    // ----- Walker bodies (HIGH). LOW remains stubbed for Task 8. -----

    /// `MapClass::DestroyBridgeWalker_NS_High` 0x0057CF60. Cascades through
    /// `MapClass::ApplyBridgeDestruction_NS_High` 0x0057E7A0 and, on final
    /// collapse, `MapClass::FindBridgeEndpoints_NS_High` 0x0057DC20.
    ///
    /// HIGH NS-axis walker. Reads the input cell's overlay, picks one of
    /// 5 cases:
    /// - `0xDF` → write 0xE0 to (this, north, south); cascade west sibling
    /// - `0xE1` → write 0xE2 to (this, north, south); cascade east sibling
    /// - `< 0xD3` → write 0xD3 to triple; cascade BOTH (rx±1, ry)
    /// - `[0xD3..=0xD5]` → write 0xE7 to triple (FINAL collapse); cascade
    ///   BOTH; mark zones_dirty
    /// - else → no-op
    ///
    /// Returns `Collapsed` when any cell hit final-collapse (0xE7), or
    /// `Absorbed` for an intermediate transition that touched no final
    /// cell. `NoChange` only when the initial overlay byte is outside the
    /// 5-case set.
    pub(super) fn destroy_bridge_walker_ns_high(
        &mut self,
        rx: u16,
        ry: u16,
        _terrain: &ResolvedTerrainGrid,
    ) -> StateOutcome {
        use crate::sim::bridge_specs::{CellAction, SetBridgeDirectionResult};
        use crate::sim::bridge_state::{Axis, DamageState, compute_adjacent_bridges_dirty};

        let Some(cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };
        let cur = cell.overlay_byte;

        // Pick case + sibling-cascade plan.
        let (next, siblings, is_final): (u8, Vec<(u16, u16)>, bool) = if cur == 0xDF {
            (0xE0, vec![(rx.wrapping_sub(1), ry)], false)
        } else if cur == 0xE1 {
            (0xE2, vec![(rx.saturating_add(1), ry)], false)
        } else if cur < 0xD3 {
            (
                0xD3,
                vec![(rx.wrapping_sub(1), ry), (rx.saturating_add(1), ry)],
                false,
            )
        } else if (0xD3..=0xD5).contains(&cur) {
            (
                0xE7,
                vec![(rx.wrapping_sub(1), ry), (rx.saturating_add(1), ry)],
                true,
            )
        } else {
            return StateOutcome::NoChange;
        };

        let mut destroyed: Vec<(u16, u16)> = Vec::new();
        let mut actions: Vec<((u16, u16), usize, CellAction)> = Vec::new();
        // BR-16: cells whose overlay this collapse touches (triple + cascade),
        // fed to the minimap radar-dirty channel by the orchestrator.
        let mut radar_cells: Vec<(u16, u16)> = Vec::new();

        // Write the (this, north, south) length-axis triple. The write is
        // UNCONDITIONAL: bridge destruction keys purely on the overlay band,
        // with no per-cell role exception, so a bridgehead/ramp cell caught in
        // the triple receives the destroy overlay (and a BlowUpBridge on a
        // final collapse) like any other cell — it is NOT left standing.
        for (slot, opt_pos) in Self::ns_triple(rx, ry).into_iter().enumerate() {
            if let Some(pos) = opt_pos {
                let _ = self.write_overlay_byte(pos.0, pos.1, next);
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    radar_cells.push(pos);
                    if is_final {
                        c.damage_state = DamageState::Destroyed;
                        destroyed.push(pos);
                        actions.push((pos, slot, CellAction::BlowUpBridge));
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }

        // Cascade to perpendicular siblings via the EW classifier-driven leaf.
        for (sx, sy) in siblings {
            if sx == u16::MAX {
                // wrapping_sub overflow at rx == 0 → west neighbor off-map.
                continue;
            }
            let sibling_finals = self.apply_bridge_destruction_ns_high(sx, sy, &mut radar_cells);
            for pos in sibling_finals {
                if !destroyed.contains(&pos) {
                    destroyed.push(pos);
                    actions.push((pos, 0, CellAction::BlowUpBridge));
                }
            }
        }

        if !is_final && destroyed.is_empty() {
            // Intermediate transition only — overlay/damage_state changed
            // but no cell hit final. Caller treats this as "absorbed".
            return StateOutcome::Absorbed {
                damaged_variant_cells: Vec::new(),
            };
        }

        let adj = compute_adjacent_bridges_dirty(rx, ry, Axis::NS);
        StateOutcome::Collapsed {
            binary_success: true,
            destroyed_cells: destroyed,
            set_bridge_direction: SetBridgeDirectionResult {
                actions,
                flag_stamp: None,
            },
            setter_transcript: Vec::new(),
            adjacent_bridges_dirty: adj,
            zones_dirty: is_final,
            radar_cells,
            damaged_variant_cells: Vec::new(),
        }
    }

    /// `MapClass::DestroyBridgeWalker_EW_High` 0x0057D530 — the compiled twin
    /// of 0x0057CF60 with the EW constants. Cascades through
    /// `MapClass::ApplyBridgeDestruction_EW_High` 0x0057ED00 and
    /// `MapClass::FindBridgeEndpoints_EW_High` 0x0057DAF0.
    ///
    /// HIGH EW-axis walker. Mirror of `destroy_bridge_walker_ns_high` with:
    /// - `0xE3` → write 0xE4 to (this, west, east); cascade south sibling
    /// - `0xE5` → write 0xE6 to triple; cascade north sibling
    /// - `< 0xDC` → write 0xDC to triple; cascade BOTH (rx, ry±1)
    /// - `[0xDC..=0xDE]` → write 0xE8 to triple (FINAL); cascade BOTH;
    ///   mark zones_dirty
    /// - else → no-op
    pub(super) fn destroy_bridge_walker_ew_high(
        &mut self,
        rx: u16,
        ry: u16,
        _terrain: &ResolvedTerrainGrid,
    ) -> StateOutcome {
        use crate::sim::bridge_specs::{CellAction, SetBridgeDirectionResult};
        use crate::sim::bridge_state::{Axis, DamageState, compute_adjacent_bridges_dirty};

        let Some(cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };
        let cur = cell.overlay_byte;

        let (next, siblings, is_final): (u8, Vec<(u16, u16)>, bool) = if cur == 0xE3 {
            (0xE4, vec![(rx, ry.saturating_add(1))], false)
        } else if cur == 0xE5 {
            (0xE6, vec![(rx, ry.wrapping_sub(1))], false)
        } else if cur < 0xDC {
            (
                0xDC,
                vec![(rx, ry.wrapping_sub(1)), (rx, ry.saturating_add(1))],
                false,
            )
        } else if (0xDC..=0xDE).contains(&cur) {
            (
                0xE8,
                vec![(rx, ry.wrapping_sub(1)), (rx, ry.saturating_add(1))],
                true,
            )
        } else {
            return StateOutcome::NoChange;
        };

        let mut destroyed: Vec<(u16, u16)> = Vec::new();
        let mut actions: Vec<((u16, u16), usize, CellAction)> = Vec::new();
        // BR-16: cells whose overlay this collapse touches (triple + cascade),
        // fed to the minimap radar-dirty channel by the orchestrator.
        let mut radar_cells: Vec<(u16, u16)> = Vec::new();

        for (slot, opt_pos) in Self::ew_triple(rx, ry).into_iter().enumerate() {
            if let Some(pos) = opt_pos {
                let _ = self.write_overlay_byte(pos.0, pos.1, next);
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    radar_cells.push(pos);
                    if is_final {
                        c.damage_state = DamageState::Destroyed;
                        destroyed.push(pos);
                        actions.push((pos, slot, CellAction::BlowUpBridge));
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }

        for (sx, sy) in siblings {
            if sy == u16::MAX {
                continue;
            }
            let sibling_finals = self.apply_bridge_destruction_ew_high(sx, sy, &mut radar_cells);
            for pos in sibling_finals {
                if !destroyed.contains(&pos) {
                    destroyed.push(pos);
                    actions.push((pos, 0, CellAction::BlowUpBridge));
                }
            }
        }

        if !is_final && destroyed.is_empty() {
            return StateOutcome::Absorbed {
                damaged_variant_cells: Vec::new(),
            };
        }

        let adj = compute_adjacent_bridges_dirty(rx, ry, Axis::EW);
        StateOutcome::Collapsed {
            binary_success: true,
            destroyed_cells: destroyed,
            set_bridge_direction: SetBridgeDirectionResult {
                actions,
                flag_stamp: None,
            },
            setter_transcript: Vec::new(),
            adjacent_bridges_dirty: adj,
            zones_dirty: is_final,
            radar_cells,
            damaged_variant_cells: Vec::new(),
        }
    }

    // ----- LOW perpendicular neighbor classifiers. -----

    /// Classify the EW-axis perpendicular pattern at `(rx, ry)` for LOW
    /// bridges. Bit assignment (matches the binary's switch order at the
    /// LOW EW classifier — east-first, then west):
    /// - bit 0 (val 1): east in `{0x4E, 0x50, 0x52, 0x5D}`
    /// - bit 1 (val 2): east in `{0x51, 0x64}`
    /// - bit 2 (val 4): west in `{0x4F, 0x50, 0x51, 0x5F}`
    /// - bit 3 (val 8): west in `{0x52, 0x64}`
    /// `MapClass::CheckBridgeNeighbors_EW_Low` 0x0057B870 — the LOW twin of
    /// 0x0057CAB0.
    pub(super) fn check_bridge_neighbors_ew_low(&self, rx: u16, ry: u16) -> u8 {
        let east = self
            .cell(rx.saturating_add(1), ry)
            .map(|c| c.overlay_byte)
            .unwrap_or(0);
        let west = if rx > 0 {
            self.cell(rx - 1, ry).map(|c| c.overlay_byte).unwrap_or(0)
        } else {
            0
        };
        let mut idx = 0u8;
        match east {
            0x4E | 0x50 | 0x52 | 0x5D => idx |= 1,
            0x51 | 0x64 => idx |= 2,
            _ => {}
        }
        match west {
            0x4F | 0x50 | 0x51 | 0x5F => idx |= 4,
            0x52 | 0x64 => idx |= 8,
            _ => {}
        }
        idx
    }

    /// Classify the NS-axis perpendicular pattern at `(rx, ry)` for LOW
    /// bridges. Bit assignment (matches the binary — north-first, then
    /// south):
    /// - bit 0 (val 1): north in `{0x57, 0x59, 0x5B, 0x61}`
    /// - bit 1 (val 2): north in `{0x5A, 0x65}`
    /// - bit 2 (val 4): south in `{0x58, 0x59, 0x5A, 0x63}`
    /// - bit 3 (val 8): south in `{0x5B, 0x65}`
    /// `MapClass::CheckBridgeNeighbors_NS_Low` 0x0057B990 — the LOW twin of
    /// 0x0057CBE0.
    pub(super) fn check_bridge_neighbors_ns_low(&self, rx: u16, ry: u16) -> u8 {
        let north = if ry > 0 {
            self.cell(rx, ry - 1).map(|c| c.overlay_byte).unwrap_or(0)
        } else {
            0
        };
        let south = self
            .cell(rx, ry.saturating_add(1))
            .map(|c| c.overlay_byte)
            .unwrap_or(0);
        let mut idx = 0u8;
        match north {
            0x57 | 0x59 | 0x5B | 0x61 => idx |= 1,
            0x5A | 0x65 => idx |= 2,
            _ => {}
        }
        match south {
            0x58 | 0x59 | 0x5A | 0x63 => idx |= 4,
            0x5B | 0x65 => idx |= 8,
            _ => {}
        }
        idx
    }

    // ----- LOW sibling-cascade leaves. -----

    /// Sibling-cascade leaf for the LOW NS body axis. Mirror of
    /// `apply_bridge_destruction_ns_high` with LOW outer gate
    /// (`[0x4A..=0x65]`), LOW NS table, LOW intermediates 0x5C/0x5E, and
    /// final 0x64.
    fn apply_bridge_destruction_ns_low(
        &mut self,
        rx: u16,
        ry: u16,
        radar: &mut Vec<(u16, u16)>,
    ) -> Vec<(u16, u16)> {
        use crate::sim::bridge_specs::pick_destruction_overlay;
        use crate::sim::bridge_state::{Axis, DamageState};

        let mut final_cells = Vec::new();
        let Some(cell) = self.cell(rx, ry).copied() else {
            return final_cells;
        };
        let cur = cell.overlay_byte;
        if !(0x4A..=0x65).contains(&cur) {
            return final_cells;
        }

        let idx = self.check_bridge_neighbors_ew_low(rx, ry);
        if idx == 0 {
            return final_cells;
        }

        let next = if cur < 0x5C {
            match pick_destruction_overlay(idx, Axis::NS, false) {
                Some(n) if n != cur => n,
                _ => return final_cells,
            }
        } else if cur == 0x5C {
            0x5D
        } else if cur == 0x5E {
            0x5F
        } else {
            return final_cells;
        };

        let triple = Self::ns_triple(rx, ry);
        for pos in triple.into_iter().flatten() {
            let _ = self.write_overlay_byte_deferred_recalc(pos.0, pos.1, next);
        }
        for slot in triple {
            if let Some(pos) = slot {
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    // Every touched cell (final OR intermediate) is minimap-dirty.
                    radar.push(pos);
                    if next == 0x64 {
                        c.damage_state = DamageState::Destroyed;
                        final_cells.push(pos);
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }
        for pos in triple.into_iter().flatten() {
            self.queue_overlay_recalc(pos.0, pos.1);
        }
        final_cells
    }

    /// Sibling-cascade leaf for the LOW EW body axis. Intermediates
    /// 0x60/0x62 → 0x61/0x63; final 0x65.
    fn apply_bridge_destruction_ew_low(
        &mut self,
        rx: u16,
        ry: u16,
        radar: &mut Vec<(u16, u16)>,
    ) -> Vec<(u16, u16)> {
        use crate::sim::bridge_specs::pick_destruction_overlay;
        use crate::sim::bridge_state::{Axis, DamageState};

        let mut final_cells = Vec::new();
        let Some(cell) = self.cell(rx, ry).copied() else {
            return final_cells;
        };
        let cur = cell.overlay_byte;
        if !(0x4A..=0x65).contains(&cur) {
            return final_cells;
        }

        let idx = self.check_bridge_neighbors_ns_low(rx, ry);
        if idx == 0 {
            return final_cells;
        }

        let next = if cur < 0x60 {
            match pick_destruction_overlay(idx, Axis::EW, false) {
                Some(n) if n != cur => n,
                _ => return final_cells,
            }
        } else if cur == 0x60 {
            0x61
        } else if cur == 0x62 {
            0x63
        } else {
            return final_cells;
        };

        let triple = Self::ew_triple(rx, ry);
        for pos in triple.into_iter().flatten() {
            let _ = self.write_overlay_byte_deferred_recalc(pos.0, pos.1, next);
        }
        for slot in triple {
            if let Some(pos) = slot {
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    // Every touched cell (final OR intermediate) is minimap-dirty.
                    radar.push(pos);
                    if next == 0x65 {
                        c.damage_state = DamageState::Destroyed;
                        final_cells.push(pos);
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }
        for pos in triple.into_iter().flatten() {
            self.queue_overlay_recalc(pos.0, pos.1);
        }
        final_cells
    }

    // ----- LOW walker bodies. -----

    /// `MapClass::DestroyBridgeWalker_NS_Low` 0x0057BCF0 — verified against
    /// the decompile: same body shape as 0x0057CF60 with the LOW constants,
    /// down to the three `RadarClass::MarkTerrainDirty` calls and the
    /// `RebuildZoneConnectivity` on final collapse. Cascades through
    /// `MapClass::ApplyBridgeDestruction_NS_Low` 0x0057DD50 and
    /// `MapClass::FindBridgeEndpoints_NS_Low` 0x0057C990.
    ///
    /// LOW NS-axis walker. Mirror of `destroy_bridge_walker_ns_high` with
    /// LOW case values:
    /// - `0x5C` → write 0x5D to (this, north, south); cascade west sibling
    /// - `0x5E` → write 0x5F to triple; cascade east sibling
    /// - `< 0x50` → write 0x50 to triple; cascade BOTH (rx±1, ry)
    /// - `[0x50..=0x52]` → write 0x64 to triple (FINAL); cascade BOTH;
    ///   mark zones_dirty
    /// - else → no-op
    pub(super) fn destroy_bridge_walker_ns_low(
        &mut self,
        rx: u16,
        ry: u16,
        _terrain: &ResolvedTerrainGrid,
    ) -> StateOutcome {
        use crate::sim::bridge_specs::{CellAction, SetBridgeDirectionResult};
        use crate::sim::bridge_state::{Axis, DamageState, compute_adjacent_bridges_dirty};

        let Some(cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };
        let cur = cell.overlay_byte;

        let (next, siblings, is_final): (u8, Vec<(u16, u16)>, bool) = if cur == 0x5C {
            (0x5D, vec![(rx.wrapping_sub(1), ry)], false)
        } else if cur == 0x5E {
            (0x5F, vec![(rx.saturating_add(1), ry)], false)
        } else if cur < 0x50 {
            (
                0x50,
                vec![(rx.wrapping_sub(1), ry), (rx.saturating_add(1), ry)],
                false,
            )
        } else if (0x50..=0x52).contains(&cur) {
            (
                0x64,
                vec![(rx.wrapping_sub(1), ry), (rx.saturating_add(1), ry)],
                true,
            )
        } else {
            return StateOutcome::NoChange;
        };

        let mut destroyed: Vec<(u16, u16)> = Vec::new();
        let mut actions: Vec<((u16, u16), usize, CellAction)> = Vec::new();
        // BR-16: cells whose overlay this collapse touches (triple + cascade),
        // fed to the minimap radar-dirty channel by the orchestrator.
        let mut radar_cells: Vec<(u16, u16)> = Vec::new();

        let triple = Self::ns_triple(rx, ry);
        for pos in Self::ns_low_root_write_order(rx, ry).into_iter().flatten() {
            let _ = self.write_overlay_byte_deferred_recalc(pos.0, pos.1, next);
        }
        for (slot, opt_pos) in triple.into_iter().enumerate() {
            if let Some(pos) = opt_pos {
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    radar_cells.push(pos);
                    if is_final {
                        c.damage_state = DamageState::Destroyed;
                        destroyed.push(pos);
                        actions.push((pos, slot, CellAction::BlowUpBridge));
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }

        for (sx, sy) in siblings {
            if sx == u16::MAX {
                continue;
            }
            let sibling_finals = self.apply_bridge_destruction_ns_low(sx, sy, &mut radar_cells);
            for pos in sibling_finals {
                if !destroyed.contains(&pos) {
                    destroyed.push(pos);
                    actions.push((pos, 0, CellAction::BlowUpBridge));
                }
            }
        }

        for pos in triple.into_iter().flatten() {
            self.queue_overlay_recalc(pos.0, pos.1);
        }

        if !is_final && destroyed.is_empty() {
            return StateOutcome::Absorbed {
                damaged_variant_cells: Vec::new(),
            };
        }

        let adj = compute_adjacent_bridges_dirty(rx, ry, Axis::NS);
        StateOutcome::Collapsed {
            binary_success: true,
            destroyed_cells: destroyed,
            set_bridge_direction: SetBridgeDirectionResult {
                actions,
                flag_stamp: None,
            },
            setter_transcript: Vec::new(),
            adjacent_bridges_dirty: adj,
            zones_dirty: is_final,
            radar_cells,
            damaged_variant_cells: Vec::new(),
        }
    }

    /// `MapClass::DestroyBridgeWalker_EW_Low` 0x0057C2B0 — the compiled twin
    /// of 0x0057BCF0 with the EW constants. Cascades through
    /// `MapClass::ApplyBridgeDestruction_EW_Low` 0x0057E2A0 and
    /// `MapClass::FindBridgeEndpoints_EW_Low` 0x0057C870.
    ///
    /// LOW EW-axis walker. Mirror of NS LOW with EW case values:
    /// - `0x60` → write 0x61 to (this, west, east); cascade south sibling
    /// - `0x62` → write 0x63 to triple; cascade north sibling
    /// - `< 0x59` → write 0x59 to triple; cascade BOTH (rx, ry±1)
    /// - `[0x59..=0x5B]` → write 0x65 to triple (FINAL); cascade BOTH;
    ///   mark zones_dirty
    /// - else → no-op
    pub(super) fn destroy_bridge_walker_ew_low(
        &mut self,
        rx: u16,
        ry: u16,
        _terrain: &ResolvedTerrainGrid,
    ) -> StateOutcome {
        use crate::sim::bridge_specs::{CellAction, SetBridgeDirectionResult};
        use crate::sim::bridge_state::{Axis, DamageState, compute_adjacent_bridges_dirty};

        let Some(cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };
        let cur = cell.overlay_byte;

        let (next, siblings, is_final): (u8, Vec<(u16, u16)>, bool) = if cur == 0x60 {
            (0x61, vec![(rx, ry.saturating_add(1))], false)
        } else if cur == 0x62 {
            (0x63, vec![(rx, ry.wrapping_sub(1))], false)
        } else if cur < 0x59 {
            (
                0x59,
                vec![(rx, ry.wrapping_sub(1)), (rx, ry.saturating_add(1))],
                false,
            )
        } else if (0x59..=0x5B).contains(&cur) {
            (
                0x65,
                vec![(rx, ry.wrapping_sub(1)), (rx, ry.saturating_add(1))],
                true,
            )
        } else {
            return StateOutcome::NoChange;
        };

        let mut destroyed: Vec<(u16, u16)> = Vec::new();
        let mut actions: Vec<((u16, u16), usize, CellAction)> = Vec::new();
        // BR-16: cells whose overlay this collapse touches (triple + cascade),
        // fed to the minimap radar-dirty channel by the orchestrator.
        let mut radar_cells: Vec<(u16, u16)> = Vec::new();

        let triple = Self::ew_triple(rx, ry);
        for pos in Self::ew_low_root_write_order(rx, ry).into_iter().flatten() {
            let _ = self.write_overlay_byte_deferred_recalc(pos.0, pos.1, next);
        }
        for (slot, opt_pos) in triple.into_iter().enumerate() {
            if let Some(pos) = opt_pos {
                if let Some(c) = self.cell_mut(pos.0, pos.1) {
                    radar_cells.push(pos);
                    if is_final {
                        c.damage_state = DamageState::Destroyed;
                        destroyed.push(pos);
                        actions.push((pos, slot, CellAction::BlowUpBridge));
                    } else {
                        c.damage_state = DamageState::Damaged;
                    }
                }
            }
        }

        for (sx, sy) in siblings {
            if sy == u16::MAX {
                continue;
            }
            let sibling_finals = self.apply_bridge_destruction_ew_low(sx, sy, &mut radar_cells);
            for pos in sibling_finals {
                if !destroyed.contains(&pos) {
                    destroyed.push(pos);
                    actions.push((pos, 0, CellAction::BlowUpBridge));
                }
            }
        }

        for pos in triple.into_iter().flatten() {
            self.queue_overlay_recalc(pos.0, pos.1);
        }

        if !is_final && destroyed.is_empty() {
            return StateOutcome::Absorbed {
                damaged_variant_cells: Vec::new(),
            };
        }

        let adj = compute_adjacent_bridges_dirty(rx, ry, Axis::EW);
        StateOutcome::Collapsed {
            binary_success: true,
            destroyed_cells: destroyed,
            set_bridge_direction: SetBridgeDirectionResult {
                actions,
                flag_stamp: None,
            },
            setter_transcript: Vec::new(),
            adjacent_bridges_dirty: adj,
            zones_dirty: is_final,
            radar_cells,
            damaged_variant_cells: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::bridge_state::{
        Axis, BridgeCellRole, BridgeEndpointRecord, BridgeRecordKind, BridgeRuntimeCell,
        BridgeheadAnchorClass, DamageState,
    };
    use crate::sim::rng::SimRng;

    fn empty_terrain() -> ResolvedTerrainGrid {
        ResolvedTerrainGrid::from_cells(0, 0, Vec::new())
    }

    #[test]
    fn destroy_overlay_predicates_match_gamemd_ranges() {
        assert!(!BridgeRuntimeState::is_low_destroy_overlay(0x49));
        assert!(BridgeRuntimeState::is_low_destroy_overlay(0x4A));
        assert!(BridgeRuntimeState::is_low_destroy_overlay(0x65));
        assert!(!BridgeRuntimeState::is_low_destroy_overlay(0x66));

        assert!(!BridgeRuntimeState::is_high_destroy_overlay(0xCC));
        assert!(BridgeRuntimeState::is_high_destroy_overlay(0xCD));
        assert!(BridgeRuntimeState::is_high_destroy_overlay(0xE8));
        assert!(!BridgeRuntimeState::is_high_destroy_overlay(0xE9));
    }

    #[test]
    fn destroy_overlay_axis_helpers_match_representative_subranges() {
        assert_eq!(
            BridgeRuntimeState::low_destroy_overlay_axis(0x4A),
            Some(Axis::NS)
        );
        assert_eq!(
            BridgeRuntimeState::low_destroy_overlay_axis(0x53),
            Some(Axis::EW)
        );
        assert_eq!(BridgeRuntimeState::low_destroy_overlay_axis(0x49), None);

        assert_eq!(
            BridgeRuntimeState::high_destroy_overlay_axis(0xCD),
            Some(Axis::NS)
        );
        assert_eq!(
            BridgeRuntimeState::high_destroy_overlay_axis(0xD6),
            Some(Axis::EW)
        );
        assert_eq!(BridgeRuntimeState::high_destroy_overlay_axis(0xCC), None);
    }

    fn terrain_with_wood_repair_tile_at(pos: Option<(u16, u16)>) -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(25);
        for cy in 0..5u16 {
            for cx in 0..5u16 {
                cells.push(ResolvedTerrainCell {
                    rx: cx,
                    ry: cy,
                    source_tile_index: 0,
                    source_sub_tile: 0,
                    final_tile_index: 0,
                    final_sub_tile: 0,
                    is_wood_bridge_repair_tile: pos == Some((cx, cy)),
                    level: 0,
                    filled_clear: false,
                    tileset_index: Some(0),
                    land_type: 0,
                    yr_cell_land_type: 0,
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
                    speed_costs: crate::rules::terrain_rules::SpeedCostProfile::default(),
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: false,
                    allows_tiberium: false,
                    height_in_pixels: 0,
                    variant: 0,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: 0,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: 0,
                    base_yr_cell_land_type: 0,
                    base_terrain_class: Default::default(),
                    base_speed_costs: Default::default(),
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        ResolvedTerrainGrid::from_cells(5, 5, cells)
    }

    fn seed_high_body_cell(state: &mut BridgeRuntimeState, rx: u16, ry: u16, overlay: u8) {
        state.test_seed_cell(
            rx,
            ry,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 5,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::NS),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(1),
                overlay_byte: overlay,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            },
        );
    }

    #[test]
    fn repair_scan_wood_bridge_tile_without_low_overlay_dispatches_low() {
        let mut state = BridgeRuntimeState::default();
        for y in 1..=3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xE7);
            state.cell_mut(2, y).unwrap().damage_state = DamageState::Destroyed;
        }

        let terrain = terrain_with_wood_repair_tile_at(Some((0, 0)));
        let scan = vec![(0, 0), (2, 2)];
        let mut rng = SimRng::new(789);
        let prior_rng = rng.state();

        let outcome = state.repair_bridge_from_engineer_scan(&scan, &mut rng, &terrain);

        assert!(!outcome.zones_dirty);
        assert!(outcome.radar_cells.is_empty());
        assert_eq!(outcome.repaired_cells, 0);
        assert_eq!(rng.state(), prior_rng);
        for y in 1..=3u16 {
            let cell = state.cell(2, y).unwrap();
            assert_eq!(cell.overlay_byte, 0xE7);
            assert_eq!(cell.damage_state, DamageState::Destroyed);
        }
    }

    #[test]
    fn repair_scan_without_low_overlay_or_wood_tile_dispatches_high() {
        let mut state = BridgeRuntimeState::default();
        for y in 1..=3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xE7);
            state.cell_mut(2, y).unwrap().damage_state = DamageState::Destroyed;
        }

        let terrain = terrain_with_wood_repair_tile_at(None);
        let scan = vec![(0, 0), (2, 2)];
        let mut rng = SimRng::new(790);

        let outcome = state.repair_bridge_from_engineer_scan(&scan, &mut rng, &terrain);

        assert!(outcome.zones_dirty);
        assert_eq!(outcome.repaired_cells, 3);
        for y in 1..=3u16 {
            assert!(
                matches!(
                    state.cell(2, y).unwrap().damage_state,
                    DamageState::Destroyed
                ),
                "gamemd leaves the body damage byte stale after bridge repair"
            );
        }
    }

    #[test]
    fn repair_scan_low_overlay_dispatches_low_when_tile_predicate_false() {
        let mut state = BridgeRuntimeState::default();
        for y in 1..=3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xE7);
            state.cell_mut(2, y).unwrap().damage_state = DamageState::Destroyed;
            seed_low_body_cell(&mut state, 1, y, Axis::NS, 0x64);
            state.cell_mut(1, y).unwrap().damage_state = DamageState::Destroyed;
        }

        let terrain = terrain_with_wood_repair_tile_at(None);
        let scan = vec![(2, 2), (1, 2)];
        let mut rng = SimRng::new(791);

        let outcome = state.repair_bridge_from_engineer_scan(&scan, &mut rng, &terrain);

        assert!(outcome.zones_dirty);
        assert_eq!(outcome.repaired_cells, 3);
        for y in 1..=3u16 {
            assert_eq!(
                state.cell(2, y).unwrap().damage_state,
                DamageState::Destroyed
            );
            assert!(
                matches!(
                    state.cell(1, y).unwrap().damage_state,
                    DamageState::Destroyed
                ),
                "gamemd leaves the body damage byte stale after bridge repair"
            );
        }
    }

    #[test]
    fn destroy_bridge_high_returns_nochange_for_low_overlay() {
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0x5A);
        let terrain = empty_terrain();
        assert_eq!(
            state.destroy_bridge_high(0, 0, &terrain),
            StateOutcome::NoChange
        );
    }

    #[test]
    fn destroy_bridge_low_returns_nochange_for_high_overlay() {
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xD0);
        let terrain = empty_terrain();
        assert_eq!(
            state.destroy_bridge_low(0, 0, &terrain),
            StateOutcome::NoChange
        );
    }

    #[test]
    fn repair_transition_table_matches_verified_binary_ranges() {
        assert_eq!(
            BridgeRuntimeState::repair_transition(0xE7, RepairFamily::HighNs),
            RepairTransition::RandomHealthy { base: 0xCD }
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0xE8, RepairFamily::HighEw),
            RepairTransition::RandomHealthy { base: 0xD6 }
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0x64, RepairFamily::LowNs),
            RepairTransition::RandomHealthy { base: 0x4A }
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0x65, RepairFamily::LowEw),
            RepairTransition::RandomHealthy { base: 0x53 }
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0xE0, RepairFamily::HighNs),
            RepairTransition::Fixed(0xDF)
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0xE3, RepairFamily::HighEw),
            RepairTransition::Fixed(0xE3)
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0x5C, RepairFamily::LowNs),
            RepairTransition::Fixed(0x5C)
        );
        assert_eq!(
            BridgeRuntimeState::repair_transition(0x60, RepairFamily::LowEw),
            RepairTransition::Fixed(0x60)
        );
    }

    #[test]
    fn repair_fixed_same_overlay_skips_whole_strip_and_rng() {
        let mut state = BridgeRuntimeState::default();
        for y in 1..=3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x5D);
            state.cell_mut(2, y).unwrap().damage_state = DamageState::Destroyed;
        }
        state.cell_mut(2, 2).unwrap().overlay_byte = 0x5C;

        let mut rng = SimRng::new(123);
        let prior_rng = rng.state();
        let terrain = empty_terrain();
        let mut outcome = RepairOutcome::default();
        state.apply_repair_to_strip_cell(
            BridgeRuntimeState::ns_triple(2, 2),
            RepairFamily::LowNs,
            &mut rng,
            &terrain,
            &mut outcome,
        );

        assert_eq!(outcome.repaired_cells, 0);
        assert!(!outcome.zones_dirty);
        assert_eq!(rng.state(), prior_rng);
        for y in 1..=3u16 {
            assert!(matches!(
                state.cell(2, y).unwrap().damage_state,
                DamageState::Destroyed
            ));
        }
    }

    #[test]
    fn repair_destroyed_high_ns_strip_rewrites_overlay_retains_stale_damage_and_radar_cells() {
        let mut state = BridgeRuntimeState::default();
        for y in 1..=3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xE7);
            state.cell_mut(2, y).unwrap().damage_state = DamageState::Destroyed;
        }

        let mut rng = SimRng::new(456);
        let terrain = empty_terrain();
        let mut outcome = RepairOutcome::default();
        state.apply_repair_to_strip_cell(
            BridgeRuntimeState::ns_triple(2, 2),
            RepairFamily::HighNs,
            &mut rng,
            &terrain,
            &mut outcome,
        );

        assert!(outcome.zones_dirty);
        assert_eq!(outcome.repaired_cells, 3);
        assert_eq!(outcome.radar_cells, vec![(2, 2), (2, 1), (2, 3)]);
        let center_overlay = state.cell(2, 2).unwrap().overlay_byte;
        assert!((0xCD..=0xD0).contains(&center_overlay));
        for y in 1..=3u16 {
            let cell = state.cell(2, y).unwrap();
            assert_eq!(cell.overlay_byte, center_overlay);
            assert!(
                matches!(cell.damage_state, DamageState::Destroyed),
                "gamemd leaves the body damage byte stale after bridge repair"
            );
        }
    }

    #[test]
    fn axis_classifiers_partition_high_range() {
        // Sample a few overlay values from each sub-range.
        for v in [0xCDu8, 0xD0, 0xD5, 0xDF, 0xE0, 0xE2, 0xE7] {
            assert!(BridgeRuntimeState::is_ns_walker_overlay_high(v));
            assert!(!BridgeRuntimeState::is_ew_walker_overlay_high(v));
        }
        for v in [0xD6u8, 0xDA, 0xDE, 0xE3, 0xE5, 0xE6, 0xE8] {
            assert!(BridgeRuntimeState::is_ew_walker_overlay_high(v));
            assert!(!BridgeRuntimeState::is_ns_walker_overlay_high(v));
        }
        // Out-of-range values match neither.
        for v in [0u8, 0x4A, 0x65, 0xCC, 0xE9, 0xFF] {
            assert!(!BridgeRuntimeState::is_ns_walker_overlay_high(v));
            assert!(!BridgeRuntimeState::is_ew_walker_overlay_high(v));
        }
    }

    #[test]
    fn axis_classifiers_partition_low_range() {
        for v in [0x4Au8, 0x4F, 0x52, 0x5C, 0x5F, 0x64] {
            assert!(BridgeRuntimeState::is_ns_walker_overlay_low(v));
            assert!(!BridgeRuntimeState::is_ew_walker_overlay_low(v));
        }
        for v in [0x53u8, 0x57, 0x5B, 0x60, 0x63, 0x65] {
            assert!(BridgeRuntimeState::is_ew_walker_overlay_low(v));
            assert!(!BridgeRuntimeState::is_ns_walker_overlay_low(v));
        }
        for v in [0u8, 0x49, 0x66, 0xCC, 0xD0, 0xFF] {
            assert!(!BridgeRuntimeState::is_ns_walker_overlay_low(v));
            assert!(!BridgeRuntimeState::is_ew_walker_overlay_low(v));
        }
    }

    #[test]
    fn start_shift_high_ns_north_off_steps_south() {
        // Only (2, 0) is a bridge cell. Hitting (2, 0) → north neighbor is
        // off-bridge, so walker starts south at (2, 1).
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 2, 0, 0xD0);
        assert_eq!(state.find_walker_start_high_ns(2, 0), (2, 1));
    }

    #[test]
    fn start_shift_high_ns_north2_on_steps_north() {
        // (2, 0), (2, 1), (2, 2) all on bridge. Hitting (2, 2): north-1
        // (2, 1) on; north-2 (2, 0) on → walker starts at (2, 1).
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xD0);
        }
        assert_eq!(state.find_walker_start_high_ns(2, 2), (2, 1));
    }

    #[test]
    fn start_shift_high_ns_stable_mid_no_shift() {
        // 3 bridge cells (2, 0..3). Hitting middle (2, 1): north-1 (2, 0) on,
        // north-2 (2, -1) off-map → no shift.
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xD0);
        }
        assert_eq!(state.find_walker_start_high_ns(2, 1), (2, 1));
    }

    #[test]
    fn start_shift_high_ew_west_off_steps_east() {
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 2, 0xD8);
        assert_eq!(state.find_walker_start_high_ew(0, 2), (1, 2));
    }

    #[test]
    fn ns_walker_intermediate_writes_0xd3_to_triple() {
        // 3 NS body cells at (2, 0..3) all overlay 0xD0 (< 0xD3). Hit (2, 1).
        // Expect (2, 0..3) all → 0xD3, damage_state Damaged (not Destroyed).
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xD0);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ns_high(2, 1, &terrain);
        assert!(
            matches!(outcome, StateOutcome::Absorbed { .. }),
            "no perpendicular pattern → no sibling collapse → Absorbed; got {:?}",
            outcome
        );
        for y in 0..3 {
            let c = state.cell(2, y).unwrap();
            assert_eq!(c.overlay_byte, 0xD3, "y={} should transition to 0xD3", y);
            assert_eq!(c.damage_state, DamageState::Damaged);
        }
    }

    #[test]
    fn ns_walker_final_writes_0xe7_marks_destroyed_zones_dirty() {
        // 3 NS body cells at (2, 0..3) all overlay 0xD4 (final-eligible
        // [0xD3..=0xD5]). Hit (2, 1). Expect 0xE7 + Destroyed + zones_dirty.
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xD4);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ns_high(2, 1, &terrain);
        match outcome {
            StateOutcome::Collapsed {
                destroyed_cells,
                zones_dirty,
                ..
            } => {
                assert!(zones_dirty, "final-stage collapse must mark zones_dirty");
                for y in 0..3 {
                    let c = state.cell(2, y).unwrap();
                    assert_eq!(c.overlay_byte, 0xE7);
                    assert_eq!(c.damage_state, DamageState::Destroyed);
                    assert!(
                        destroyed_cells.contains(&(2, y)),
                        "(2, {}) missing from destroyed_cells",
                        y
                    );
                }
            }
            other => panic!("expected Collapsed, got {:?}", other),
        }
    }

    #[test]
    fn ns_walker_0xdf_writes_0xe0_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xDF);
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ns_high(2, 1, &terrain);
        for y in 0..3 {
            assert_eq!(state.cell(2, y).unwrap().overlay_byte, 0xE0);
        }
    }

    #[test]
    fn ns_walker_0xe1_writes_0xe2_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xE1);
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ns_high(2, 1, &terrain);
        for y in 0..3 {
            assert_eq!(state.cell(2, y).unwrap().overlay_byte, 0xE2);
        }
    }

    #[test]
    fn ns_walker_returns_nochange_for_out_of_case_overlay() {
        // 0xD7 is in EW sub-range (would be routed to EW walker). NS walker
        // hit on it is a no-op.
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 2, 1, 0xD7);
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ns_high(2, 1, &terrain);
        assert_eq!(outcome, StateOutcome::NoChange);
        assert_eq!(state.cell(2, 1).unwrap().overlay_byte, 0xD7);
    }

    #[test]
    fn ew_walker_final_writes_0xe8_marks_destroyed_zones_dirty() {
        let mut state = BridgeRuntimeState::default();
        for x in 0..3u16 {
            state.test_seed_cell(
                x,
                2,
                BridgeRuntimeCell {
                    deck_present: true,
                    destroyable: true,
                    deck_level: 5,
                    bridge_group_id: Some(1),
                    damage_state: DamageState::Damaged,
                    axis: Some(Axis::EW),
                    role: BridgeCellRole::Body,
                    anchor_span_id: Some(1),
                    overlay_byte: 0xDD,
                    bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
                },
            );
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ew_high(1, 2, &terrain);
        match outcome {
            StateOutcome::Collapsed {
                destroyed_cells,
                zones_dirty,
                ..
            } => {
                assert!(zones_dirty);
                for x in 0..3 {
                    let c = state.cell(x, 2).unwrap();
                    assert_eq!(c.overlay_byte, 0xE8);
                    assert_eq!(c.damage_state, DamageState::Destroyed);
                    assert!(destroyed_cells.contains(&(x, 2)));
                }
            }
            other => panic!("expected Collapsed, got {:?}", other),
        }
    }

    #[test]
    fn ew_walker_0xe3_writes_0xe4_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for x in 0..3u16 {
            state.test_seed_cell(
                x,
                2,
                BridgeRuntimeCell {
                    deck_present: true,
                    destroyable: true,
                    deck_level: 5,
                    bridge_group_id: Some(1),
                    damage_state: DamageState::Damaged,
                    axis: Some(Axis::EW),
                    role: BridgeCellRole::Body,
                    anchor_span_id: Some(1),
                    overlay_byte: 0xE3,
                    bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
                },
            );
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ew_high(1, 2, &terrain);
        for x in 0..3 {
            assert_eq!(state.cell(x, 2).unwrap().overlay_byte, 0xE4);
        }
    }

    #[test]
    fn check_bridge_neighbors_ew_high_bit_layout() {
        // Verify all 4 bit-positions resolve correctly. Place a center cell
        // at (1, 0) so we can independently set west=(0,0) and east=(2,0).
        // bit 0 (east in {0xD1,0xD3,0xD5,0xE0}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 1, 0, 0xD0);
        seed_high_body_cell(&mut state, 2, 0, 0xD1);
        assert_eq!(state.check_bridge_neighbors_ew_high(1, 0), 1);
        // bit 1 (east in {0xD4, 0xE7}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 1, 0, 0xD0);
        seed_high_body_cell(&mut state, 2, 0, 0xD4);
        assert_eq!(state.check_bridge_neighbors_ew_high(1, 0), 2);
        // bit 2 (west in {0xD2, 0xD3, 0xD4, 0xE2}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xD2);
        seed_high_body_cell(&mut state, 1, 0, 0xD0);
        assert_eq!(state.check_bridge_neighbors_ew_high(1, 0), 4);
        // bit 3 (west in {0xD5, 0xE7}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xE7);
        seed_high_body_cell(&mut state, 1, 0, 0xD0);
        assert_eq!(state.check_bridge_neighbors_ew_high(1, 0), 8);
    }

    #[test]
    fn check_bridge_neighbors_ns_high_bit_layout() {
        // bit 0 (north in {0xDA, 0xDC, 0xDE, 0xE4}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xDA);
        seed_high_body_cell(&mut state, 0, 1, 0xD0);
        assert_eq!(state.check_bridge_neighbors_ns_high(0, 1), 1);
        // bit 1 (north in {0xDD, 0xE8}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xE8);
        seed_high_body_cell(&mut state, 0, 1, 0xD0);
        assert_eq!(state.check_bridge_neighbors_ns_high(0, 1), 2);
        // bit 2 (south in {0xDB, 0xDC, 0xDD, 0xE6}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xD0);
        seed_high_body_cell(&mut state, 0, 1, 0xE6);
        assert_eq!(state.check_bridge_neighbors_ns_high(0, 0), 4);
        // bit 3 (south in {0xDE, 0xE8}):
        let mut state = BridgeRuntimeState::default();
        seed_high_body_cell(&mut state, 0, 0, 0xD0);
        seed_high_body_cell(&mut state, 0, 1, 0xDE);
        assert_eq!(state.check_bridge_neighbors_ns_high(0, 0), 8);
    }

    fn seed_low_body_cell(
        state: &mut BridgeRuntimeState,
        rx: u16,
        ry: u16,
        axis: Axis,
        overlay: u8,
    ) {
        state.test_seed_cell(
            rx,
            ry,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 2,
                bridge_group_id: Some(2),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(axis),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(2),
                overlay_byte: overlay,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            },
        );
    }

    #[test]
    fn ns_low_walker_intermediate_writes_0x50_to_triple() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x4A);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ns_low(2, 1, &terrain);
        assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
        for y in 0..3 {
            let c = state.cell(2, y).unwrap();
            assert_eq!(c.overlay_byte, 0x50);
            assert_eq!(c.damage_state, DamageState::Damaged);
        }
    }

    #[test]
    fn gsi_04_13_low_overlay_projection_ops_preserve_native_phases_and_repeated_writes() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x4A);
        }

        assert!(!state.write_overlay_byte(2, 1, 0x4A));
        assert_eq!(
            state.take_overlay_projection_ops(),
            vec![
                crate::sim::bridge_state::BridgeOverlayProjectionOp::Write {
                    rx: 2,
                    ry: 1,
                    overlay_byte: 0x4A,
                },
                crate::sim::bridge_state::BridgeOverlayProjectionOp::Recalc { rx: 2, ry: 1 },
            ],
            "an overlapping native writer still performs RecalcAttributes"
        );

        let outcome = state.destroy_bridge_walker_ns_low(2, 1, &empty_terrain());
        assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
        assert_eq!(
            state.take_overlay_projection_ops(),
            projection_ops(
                &[((2, 0), 0x50), ((2, 2), 0x50), ((2, 1), 0x50)],
                &[(2, 1), (2, 0), (2, 2)],
            ),
            "NS root writes north/south/center before center/north/south recalc"
        );

        let mut ew = BridgeRuntimeState::default();
        for x in 0..3u16 {
            seed_low_body_cell(&mut ew, x, 2, Axis::EW, 0x53);
        }
        let outcome = ew.destroy_bridge_walker_ew_low(1, 2, &empty_terrain());
        assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
        assert_eq!(
            ew.take_overlay_projection_ops(),
            projection_ops(
                &[((0, 2), 0x59), ((2, 2), 0x59), ((1, 2), 0x59)],
                &[(1, 2), (0, 2), (2, 2)],
            ),
            "EW root writes west/east/center before center/west/east recalc"
        );

        let mut leaf = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut leaf, 2, y, Axis::NS, 0x4A);
        }
        seed_low_body_cell(&mut leaf, 3, 1, Axis::NS, 0x4E);
        let mut radar = Vec::new();
        let _ = leaf.apply_bridge_destruction_ns_low(2, 1, &mut radar);
        let leaf_byte = leaf.cell(2, 1).expect("leaf center").overlay_byte;
        assert_eq!(
            leaf.take_overlay_projection_ops(),
            projection_ops(
                &[
                    ((2, 1), leaf_byte),
                    ((2, 0), leaf_byte),
                    ((2, 2), leaf_byte),
                ],
                &[(2, 1), (2, 0), (2, 2)],
            ),
            "cascade leaf completes center/north/south writes before recalc"
        );

        let mut repair = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut repair, 2, y, Axis::NS, 0x64);
        }
        let mut repair_outcome = RepairOutcome::default();
        repair.apply_repair_to_strip_cell(
            BridgeRuntimeState::ns_triple(2, 1),
            RepairFamily::LowNs,
            &mut SimRng::new(0),
            &empty_terrain(),
            &mut repair_outcome,
        );
        let repair_byte = repair.cell(2, 1).expect("repair center").overlay_byte;
        assert_eq!(
            repair.take_overlay_projection_ops(),
            projection_ops(
                &[
                    ((2, 1), repair_byte),
                    ((2, 0), repair_byte),
                    ((2, 2), repair_byte),
                ],
                &[(2, 1), (2, 0), (2, 2)],
            ),
            "repair completes center/north/south writes before recalc"
        );

        let mut ew_leaf = BridgeRuntimeState::default();
        for x in 0..3u16 {
            seed_low_body_cell(&mut ew_leaf, x, 2, Axis::EW, 0x53);
        }
        seed_low_body_cell(&mut ew_leaf, 1, 1, Axis::EW, 0x57);
        let mut radar = Vec::new();
        let _ = ew_leaf.apply_bridge_destruction_ew_low(1, 2, &mut radar);
        let leaf_byte = ew_leaf.cell(1, 2).expect("EW leaf center").overlay_byte;
        assert_eq!(
            ew_leaf.take_overlay_projection_ops(),
            projection_ops(
                &[
                    ((1, 2), leaf_byte),
                    ((0, 2), leaf_byte),
                    ((2, 2), leaf_byte),
                ],
                &[(1, 2), (0, 2), (2, 2)],
            ),
            "EW cascade leaf completes center/west/east writes before recalc"
        );

        let mut ew_repair = BridgeRuntimeState::default();
        for x in 0..3u16 {
            seed_low_body_cell(&mut ew_repair, x, 2, Axis::EW, 0x65);
        }
        let mut repair_outcome = RepairOutcome::default();
        ew_repair.apply_repair_to_strip_cell(
            BridgeRuntimeState::ew_triple(1, 2),
            RepairFamily::LowEw,
            &mut SimRng::new(0),
            &empty_terrain(),
            &mut repair_outcome,
        );
        let repair_byte = ew_repair.cell(1, 2).expect("EW repair center").overlay_byte;
        assert_eq!(
            ew_repair.take_overlay_projection_ops(),
            projection_ops(
                &[
                    ((1, 2), repair_byte),
                    ((0, 2), repair_byte),
                    ((2, 2), repair_byte),
                ],
                &[(1, 2), (0, 2), (2, 2)],
            ),
            "EW repair completes center/west/east writes before recalc"
        );
    }

    fn projection_ops(
        writes: &[((u16, u16), u8)],
        recalcs: &[(u16, u16)],
    ) -> Vec<crate::sim::bridge_state::BridgeOverlayProjectionOp> {
        writes
            .iter()
            .map(|&((rx, ry), overlay_byte)| {
                crate::sim::bridge_state::BridgeOverlayProjectionOp::Write {
                    rx,
                    ry,
                    overlay_byte,
                }
            })
            .chain(recalcs.iter().map(|&(rx, ry)| {
                crate::sim::bridge_state::BridgeOverlayProjectionOp::Recalc { rx, ry }
            }))
            .collect()
    }

    #[test]
    fn low_direct_first_hit_damages_without_deactivating_zone_record_then_second_hit_collapses() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x4A);
        }
        state.test_set_endpoint_records(vec![BridgeEndpointRecord {
            endpoint_a: (2, 0),
            endpoint_b: (2, 2),
            group_id: 2,
            active: true,
            bridge_kind: BridgeRecordKind::Low,
        }]);
        let terrain = empty_terrain();

        let first = state.destroy_bridge_low(2, 1, &terrain);
        assert!(
            matches!(first, StateOutcome::Absorbed { .. }),
            "healthy low bridge hit must be an intermediate damage transition"
        );
        for y in 0..3 {
            let cell = state.cell(2, y).unwrap();
            assert_eq!(cell.overlay_byte, 0x50);
            assert_eq!(cell.damage_state, DamageState::Damaged);
            assert!(
                state.is_bridge_walkable(2, y),
                "damaged low bridge cell (2, {y}) must remain bridge-walkable"
            );
        }
        state.refresh_endpoint_active_flags();
        assert!(
            state.endpoint_records()[0].active,
            "intermediate damage must not remove low-bridge zone connectivity"
        );

        let second = state.destroy_bridge_low(2, 1, &terrain);
        match second {
            StateOutcome::Collapsed {
                destroyed_cells,
                zones_dirty,
                ..
            } => {
                assert!(
                    zones_dirty,
                    "destroyed-anchor transition rebuilds bridge zones"
                );
                for y in 0..3 {
                    assert!(destroyed_cells.contains(&(2, y)));
                    let cell = state.cell(2, y).unwrap();
                    assert_eq!(cell.overlay_byte, 0x64);
                    assert_eq!(cell.damage_state, DamageState::Destroyed);
                    assert!(
                        !state.is_bridge_walkable(2, y),
                        "destroyed low bridge cell (2, {y}) must stop being bridge-walkable"
                    );
                }
            }
            other => panic!("expected final collapse on second hit, got {:?}", other),
        }
        state.refresh_endpoint_active_flags();
        assert!(
            !state.endpoint_records()[0].active,
            "destroyed low bridge must remove bridge-zone connectivity"
        );
    }

    #[test]
    fn ns_low_walker_final_writes_0x64_marks_destroyed_zones_dirty() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x51);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ns_low(2, 1, &terrain);
        match outcome {
            StateOutcome::Collapsed {
                destroyed_cells,
                zones_dirty,
                ..
            } => {
                assert!(zones_dirty);
                for y in 0..3 {
                    let c = state.cell(2, y).unwrap();
                    assert_eq!(c.overlay_byte, 0x64);
                    assert_eq!(c.damage_state, DamageState::Destroyed);
                    assert!(destroyed_cells.contains(&(2, y)));
                }
            }
            other => panic!("expected Collapsed, got {:?}", other),
        }
    }

    #[test]
    fn ns_low_walker_0x5c_writes_0x5d_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x5C);
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ns_low(2, 1, &terrain);
        for y in 0..3 {
            assert_eq!(state.cell(2, y).unwrap().overlay_byte, 0x5D);
        }
    }

    #[test]
    fn ns_low_walker_0x5e_writes_0x5f_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x5E);
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ns_low(2, 1, &terrain);
        for y in 0..3 {
            assert_eq!(state.cell(2, y).unwrap().overlay_byte, 0x5F);
        }
    }

    #[test]
    fn ns_low_walker_returns_nochange_above_0x52_below_0x5c() {
        // 0x55 is in the LOW EW sub-range (would route to EW walker). NS
        // walker hit on it must be a no-op.
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 2, 1, Axis::NS, 0x55);
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ns_low(2, 1, &terrain);
        assert_eq!(outcome, StateOutcome::NoChange);
        assert_eq!(state.cell(2, 1).unwrap().overlay_byte, 0x55);
    }

    #[test]
    fn ew_low_walker_final_writes_0x65_marks_destroyed_zones_dirty() {
        let mut state = BridgeRuntimeState::default();
        for x in 0..3u16 {
            seed_low_body_cell(&mut state, x, 2, Axis::EW, 0x5A);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_walker_ew_low(1, 2, &terrain);
        match outcome {
            StateOutcome::Collapsed {
                destroyed_cells,
                zones_dirty,
                ..
            } => {
                assert!(zones_dirty);
                for x in 0..3 {
                    let c = state.cell(x, 2).unwrap();
                    assert_eq!(c.overlay_byte, 0x65);
                    assert_eq!(c.damage_state, DamageState::Destroyed);
                    assert!(destroyed_cells.contains(&(x, 2)));
                }
            }
            other => panic!("expected Collapsed, got {:?}", other),
        }
    }

    #[test]
    fn ew_low_walker_0x60_writes_0x61_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for x in 0..3u16 {
            seed_low_body_cell(&mut state, x, 2, Axis::EW, 0x60);
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ew_low(1, 2, &terrain);
        for x in 0..3 {
            assert_eq!(state.cell(x, 2).unwrap().overlay_byte, 0x61);
        }
    }

    #[test]
    fn ew_low_walker_0x62_writes_0x63_intermediate() {
        let mut state = BridgeRuntimeState::default();
        for x in 0..3u16 {
            seed_low_body_cell(&mut state, x, 2, Axis::EW, 0x62);
        }
        let terrain = empty_terrain();
        let _ = state.destroy_bridge_walker_ew_low(1, 2, &terrain);
        for x in 0..3 {
            assert_eq!(state.cell(x, 2).unwrap().overlay_byte, 0x63);
        }
    }

    #[test]
    fn check_bridge_neighbors_ew_low_bit_layout() {
        // bit 0 (east in {0x4E, 0x50, 0x52, 0x5D}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 1, 0, Axis::NS, 0x4A);
        seed_low_body_cell(&mut state, 2, 0, Axis::NS, 0x4E);
        assert_eq!(state.check_bridge_neighbors_ew_low(1, 0), 1);
        // bit 1 (east in {0x51, 0x64}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 1, 0, Axis::NS, 0x4A);
        seed_low_body_cell(&mut state, 2, 0, Axis::NS, 0x64);
        assert_eq!(state.check_bridge_neighbors_ew_low(1, 0), 2);
        // bit 2 (west in {0x4F, 0x50, 0x51, 0x5F}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 0, 0, Axis::NS, 0x4F);
        seed_low_body_cell(&mut state, 1, 0, Axis::NS, 0x4A);
        assert_eq!(state.check_bridge_neighbors_ew_low(1, 0), 4);
        // bit 3 (west in {0x52, 0x64}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 0, 0, Axis::NS, 0x52);
        seed_low_body_cell(&mut state, 1, 0, Axis::NS, 0x4A);
        assert_eq!(state.check_bridge_neighbors_ew_low(1, 0), 8);
    }

    #[test]
    fn check_bridge_neighbors_ns_low_bit_layout() {
        // bit 0 (north in {0x57, 0x59, 0x5B, 0x61}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 0, 0, Axis::EW, 0x57);
        seed_low_body_cell(&mut state, 0, 1, Axis::EW, 0x4A);
        assert_eq!(state.check_bridge_neighbors_ns_low(0, 1), 1);
        // bit 1 (north in {0x5A, 0x65}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 0, 0, Axis::EW, 0x65);
        seed_low_body_cell(&mut state, 0, 1, Axis::EW, 0x4A);
        assert_eq!(state.check_bridge_neighbors_ns_low(0, 1), 2);
        // bit 2 (south in {0x58, 0x59, 0x5A, 0x63}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 0, 0, Axis::EW, 0x4A);
        seed_low_body_cell(&mut state, 0, 1, Axis::EW, 0x63);
        assert_eq!(state.check_bridge_neighbors_ns_low(0, 0), 4);
        // bit 3 (south in {0x5B, 0x65}):
        let mut state = BridgeRuntimeState::default();
        seed_low_body_cell(&mut state, 0, 0, Axis::EW, 0x4A);
        seed_low_body_cell(&mut state, 0, 1, Axis::EW, 0x5B);
        assert_eq!(state.check_bridge_neighbors_ns_low(0, 0), 8);
    }

    #[test]
    fn destroy_bridge_low_classifies_ns_axis_into_walker() {
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_low_body_cell(&mut state, 2, y, Axis::NS, 0x4A);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_low(2, 1, &terrain);
        assert!(matches!(outcome, StateOutcome::Absorbed { .. }));
        for y in 0..3 {
            assert_eq!(state.cell(2, y).unwrap().overlay_byte, 0x50);
        }
    }

    #[test]
    fn destroy_bridge_high_classifies_ns_axis_into_walker() {
        // 0xD0 routes to NS walker. 0xD0 < 0xD3 → intermediate write 0xD3
        // to triple. Without perpendicular pattern → Absorbed.
        let mut state = BridgeRuntimeState::default();
        for y in 0..3u16 {
            seed_high_body_cell(&mut state, 2, y, 0xD0);
        }
        let terrain = empty_terrain();
        let outcome = state.destroy_bridge_high(2, 1, &terrain);
        assert!(
            matches!(outcome, StateOutcome::Absorbed { .. }),
            "Task 7 wires real NS walker; expected Absorbed got {:?}",
            outcome,
        );
        for y in 0..3 {
            assert_eq!(state.cell(2, y).unwrap().overlay_byte, 0xD3);
        }
    }
}

#[cfg(test)]
mod mapgen_variant_tests {
    use super::*;
    use crate::sim::rng::SimRng;

    #[test]
    fn repair_variants_follow_native_seed_zero_mapgen_stream() {
        // Fresh native MapGen is the generated Seed(0) object, so these exact
        // scaled draws pin the healthy-overlay variants consumed by repair.
        let mut rng = SimRng::new(0);
        let actual: Vec<u8> = (0..8)
            .map(|_| BridgeRuntimeState::repair_variant_offset(&mut rng))
            .collect();
        assert_eq!(actual, [1, 1, 2, 2, 0, 1, 3, 0]);
    }

    #[test]
    fn repair_variant_consumes_whatever_stream_it_is_handed() {
        // The walker is stream-agnostic: a seeded stream advances and yields a variant
        // within the inclusive limit. (Proves the pick is not hard-coded to 0.)
        let mut rng = SimRng::new(0x1234_5678);
        let before = rng.state();
        let variant = BridgeRuntimeState::repair_variant_offset(&mut rng);
        assert!(variant <= REPAIR_VARIANT_LIMIT_INCLUSIVE);
        assert_ne!(rng.state(), before, "seeded stream must advance");
    }
}
