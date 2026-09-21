//! Immutable TubeClass-shaped map facts.
//!
//! The original engine stores TubeClass objects in a global array and stores a
//! per-cell tube index on CellClass. Rust keeps the static map-load facts here;
//! sim systems decide whether damage/state currently makes a tube usable.

/// gamemd TubeClass. `MapClass::ReadTubesINI` 0x007283C0 allocates each record
/// through `TubeClass::Constructor` 0x00727FD0 and fills +0x24/+0x26 entry,
/// +0x2C direction, +0x28/+0x2A exit, the 100-slot path array at +0x30 and the
/// length at +0x1C0 — the field set this struct mirrors.
///
/// The record's other five natives are persistence and have no counterpart
/// here by design, because VERA snapshots instead of implementing
/// IPersistStream: `TubeClass::Load` 0x007281A0, `Save` 0x007281E0,
/// `GetClassID` 0x007286D0, `Compute_CRC` 0x00728630 (which feeds 106 CRC
/// words per tube: the four i16 endpoints, the i32 direction, all 100 path
/// dwords and the length. The Ghidra plate on 0x00728630 says 105 and is
/// wrong by one; that is a read-only candidate correction) and
/// `MapClass::WriteTubesINI` 0x00728280, the map-editor save-side
/// compaction that rewrites the section and resets each entry cell's
/// +0x116 tube index to -1; its sole caller is `Save_Scenario_Map_File`,
/// an editor path with no in-match trigger. Tube records are immutable after load, so
/// none of them can move gameplay state. gamemd equivalent UNCHECKED.
///
/// Compact TubeClass array index.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TubeId(pub u16);

impl TubeId {
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Raw `CellClass+0x116` Tube-array index.
///
/// Active YR stores this field as a signed 16-bit value.  A negative value or
/// a nonnegative value outside the current Tube registry admits automatic
/// construction; only an in-range nonnegative value is a gameplay `TubeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeTubeCellIndex(i16);

impl Default for NativeTubeCellIndex {
    fn default() -> Self {
        Self::NONE
    }
}

impl NativeTubeCellIndex {
    pub const NONE: Self = Self(-1);

    pub const fn from_raw(raw: i16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> i16 {
        self.0
    }

    /// Native publication truncates the registry ordinal to the low word.
    pub const fn from_registry_ordinal(ordinal: usize) -> Self {
        Self(ordinal as u16 as i16)
    }

    pub fn validated_id(self, registry_len: usize) -> Option<TubeId> {
        let ordinal = usize::try_from(self.0).ok()?;
        (ordinal < registry_len).then_some(TubeId(ordinal as u16))
    }

    pub fn admits_automatic_construction(self, registry_len: usize) -> bool {
        self.0 < 0 || usize::from(self.0 as u16) >= registry_len
    }
}

/// Why this tube exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TubeSource {
    /// Auto-created inline by `CellClass::RecalcAttributes` for Tunnel land.
    AutomaticRecalcShell,
    /// Explicit map tube data with a real TubeClass path buffer.
    ExplicitMap,
}

impl TubeSource {
    /// Compatibility spelling for generated/synthetic callers. Active authored
    /// ownership is the automatic Recalc shell named above.
    #[allow(non_upper_case_globals)]
    pub const AutoLowBridge: Self = Self::AutomaticRecalcShell;
}

/// TubeClass fields that affect pathing and movement.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TubeFact {
    pub entry: (u16, u16),
    pub exit: (u16, u16),
    /// Raw CRT-`atoi` direction stored by TubeClass. Movement consumers mask
    /// path entries with `& 7`; the final-facing consumer uses this value raw.
    pub direction: i32,
    /// Explicit `[Tubes]` can later populate this. Auto low-bridge tubes have
    /// path_len=0 and binary-fills the unused 100-slot buffer with -1.
    pub path_steps: Vec<i32>,
    pub source: TubeSource,
}

impl TubeFact {
    pub fn automatic_recalc_shell(cell: (u16, u16), direction: u8) -> Self {
        Self {
            entry: cell,
            exit: cell,
            direction: i32::from(direction),
            path_steps: Vec::new(),
            source: TubeSource::AutomaticRecalcShell,
        }
    }

    #[cfg(test)]
    pub fn auto_low_bridge(cell: (u16, u16), direction: u8) -> Self {
        Self::automatic_recalc_shell(cell, direction)
    }

    pub fn explicit(
        entry: (u16, u16),
        exit: (u16, u16),
        direction: i32,
        path_steps: Vec<i32>,
    ) -> Self {
        Self {
            entry,
            exit,
            direction,
            path_steps,
            source: TubeSource::ExplicitMap,
        }
    }

    pub fn path_len(&self) -> usize {
        self.path_steps.len()
    }

    pub fn path_steps(&self) -> &[i32] {
        &self.path_steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_recalc_tube_is_same_cell_zero_step_shell() {
        let tube = TubeFact::automatic_recalc_shell((12, 34), 2);

        assert_eq!(tube.entry, (12, 34));
        assert_eq!(tube.exit, (12, 34));
        assert_eq!(tube.direction, 2);
        assert_eq!(tube.path_len(), 0);
        assert_eq!(tube.source, TubeSource::AutomaticRecalcShell);
    }

    #[test]
    fn native_cell_index_preserves_signed_and_out_of_range_admission() {
        assert!(NativeTubeCellIndex::NONE.admits_automatic_construction(2));
        assert_eq!(NativeTubeCellIndex::NONE.validated_id(2), None);

        let valid = NativeTubeCellIndex::from_raw(1);
        assert!(!valid.admits_automatic_construction(2));
        assert_eq!(valid.validated_id(2), Some(TubeId(1)));

        let stale = NativeTubeCellIndex::from_raw(2);
        assert!(stale.admits_automatic_construction(2));
        assert_eq!(stale.validated_id(2), None);

        let truncated_negative = NativeTubeCellIndex::from_registry_ordinal(0x8000);
        assert_eq!(truncated_negative.raw(), i16::MIN);
        assert!(truncated_negative.admits_automatic_construction(0x8001));
    }

    #[test]
    fn explicit_tube_preserves_path_steps() {
        let tube = TubeFact::explicit((1, 1), (4, 1), 2, vec![2, 2, 2]);

        assert_eq!(tube.entry, (1, 1));
        assert_eq!(tube.exit, (4, 1));
        assert_eq!(tube.path_steps, vec![2, 2, 2]);
        assert_eq!(tube.path_len(), 3);
        assert_eq!(tube.source, TubeSource::ExplicitMap);
    }
}
