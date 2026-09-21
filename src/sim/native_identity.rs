//! Active-YR numeric `AbstractClass` identity and Scenario-owned continuation.
//!
//! This wrapping signed-32-bit namespace is independent of Rust's monotonic
//! collision-free stable handles and of every RNG stream. Constructors
//! preincrement it; they neither search for nor prevent duplicate values.

use crate::map::tubes::{
    NativeMapTubeReceipt, NativeMapTubesState, RawTubeSection, TubeConstructionError,
    construct_raw_tube_section,
};
use crate::rules::ini_parser::IniFile;
use crate::sim::world::Simulation;

pub(crate) const FRESH_SCENARIO_NATIVE_ID_SEED: u32 = 1_000_000;
pub(crate) const MAP_READ_NATIVE_ID_RESERVATION: u32 = 0x2710;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum NativeFreshIdPhase {
    PrefixSaved,
    MapReadReserved,
}

/// One Scenario's wrapping numeric-ID cursor. Original689310/689470 save/load
/// +214 as part of the raw Scenario block;683560 preserves it while resetting
/// the adjacent RNG. Evidence: tools/spatial_oracle/native_id_snapshot.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeUniqueIdCursor {
    value: u32,
    saved_after_fresh_prefix: u32,
    phase: NativeFreshIdPhase,
}

impl NativeUniqueIdCursor {
    #[cfg(test)]
    pub(crate) fn test_at_current_value(value: u32) -> Self {
        Self {
            value,
            saved_after_fresh_prefix: value.wrapping_sub(MAP_READ_NATIVE_ID_RESERVATION),
            phase: NativeFreshIdPhase::MapReadReserved,
        }
    }

    fn from_saved_prefix(saved_after_fresh_prefix: u32) -> Self {
        Self {
            value: saved_after_fresh_prefix,
            saved_after_fresh_prefix,
            phase: NativeFreshIdPhase::PrefixSaved,
        }
    }

    /// Assign the next native ID. Addition deliberately wraps and the returned
    /// i32 preserves the resulting bit pattern.
    pub(crate) fn next_id(&mut self) -> i32 {
        self.value = self.value.wrapping_add(1);
        self.value as i32
    }

    /// Enter the map reader by setting from the saved Full_Init snapshot.
    ///
    /// Shadowed theater constructors may have advanced the current cursor in
    /// between; native overwrites that current value with `C_saved + 0x2710`.
    pub(crate) fn reserve_map_read_from_saved(&mut self) -> Result<u32, NativeIdentityError> {
        if self.phase != NativeFreshIdPhase::PrefixSaved {
            return Err(NativeIdentityError::MapReadReservationAlreadyApplied);
        }
        self.value = self
            .saved_after_fresh_prefix
            .wrapping_add(MAP_READ_NATIVE_ID_RESERVATION);
        self.phase = NativeFreshIdPhase::MapReadReserved;
        Ok(self.value)
    }

    #[cfg(test)]
    pub(crate) fn current_raw(&self) -> u32 {
        self.value
    }

    #[cfg(test)]
    pub(crate) fn saved_after_fresh_prefix(&self) -> u32 {
        self.saved_after_fresh_prefix
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NativeIdentityError {
    #[error("fresh native-ID map-read reservation was already applied")]
    MapReadReservationAlreadyApplied,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NativeMapTubeConstructionError {
    #[error("fresh Simulation has no native-ID cursor installed")]
    MissingFreshCursor,
    #[error("fresh Simulation already owns a raw [Tubes] receipt")]
    ReceiptAlreadyInstalled,
    #[error("fresh Simulation has not constructed its raw [Tubes] receipt")]
    ReceiptNotConstructed,
    #[error("fresh Simulation raw [Tubes] receipt was already bound")]
    ReceiptAlreadyBound,
    #[error(transparent)]
    Identity(#[from] NativeIdentityError),
    #[error(transparent)]
    Tube(#[from] TubeConstructionError),
}

impl Simulation {
    /// Assign one native numeric identity for an actual fresh-map constructor.
    /// Stable Rust handles remain independent; callers must invoke this only
    /// after the native-equivalent allocation/type gate has succeeded.
    pub(crate) fn next_native_load_id(&mut self) -> Result<i32, NativeMapTubeConstructionError> {
        self.native_unique_ids
            .as_mut()
            .map(NativeUniqueIdCursor::next_id)
            .ok_or(NativeMapTubeConstructionError::MissingFreshCursor)
    }

    /// Apply the one gameplay map-read reservation and construct every raw
    /// `[Tubes]` row in source order. This runs only after fallible asset-root
    /// discovery, so an earlier asset error leaves the saved prefix untouched.
    pub(crate) fn construct_native_map_tubes(
        &mut self,
        map_ini: &IniFile,
    ) -> Result<(), NativeMapTubeConstructionError> {
        self.construct_native_map_tubes_with_allocator(map_ini, |_| true)
    }

    fn construct_native_map_tubes_with_allocator(
        &mut self,
        map_ini: &IniFile,
        mut allocate: impl FnMut(usize) -> bool,
    ) -> Result<(), NativeMapTubeConstructionError> {
        if !matches!(self.native_map_tubes, NativeMapTubesState::Unconstructed) {
            return Err(NativeMapTubeConstructionError::ReceiptAlreadyInstalled);
        }
        let cursor = self
            .native_unique_ids
            .as_mut()
            .ok_or(NativeMapTubeConstructionError::MissingFreshCursor)?;
        cursor.reserve_map_read_from_saved()?;

        let raw_section = RawTubeSection::from_ini(map_ini);
        self.native_map_tubes = NativeMapTubesState::Pending(NativeMapTubeReceipt::default());
        let NativeMapTubesState::Pending(receipt) = &mut self.native_map_tubes else {
            unreachable!("fresh Tube receipt was installed immediately above")
        };
        let mut assign_native_id = || cursor.next_id();
        construct_raw_tube_section(raw_section, receipt, &mut allocate, &mut assign_native_id)?;
        Ok(())
    }

    pub(crate) fn take_native_map_tubes_receipt(
        &mut self,
    ) -> Result<NativeMapTubeReceipt, NativeMapTubeConstructionError> {
        match std::mem::replace(&mut self.native_map_tubes, NativeMapTubesState::Bound) {
            NativeMapTubesState::Pending(receipt) => Ok(receipt),
            NativeMapTubesState::Unconstructed => {
                self.native_map_tubes = NativeMapTubesState::Unconstructed;
                Err(NativeMapTubeConstructionError::ReceiptNotConstructed)
            }
            NativeMapTubesState::Bound => {
                self.native_map_tubes = NativeMapTubesState::Bound;
                Err(NativeMapTubeConstructionError::ReceiptAlreadyBound)
            }
        }
    }
}

/// Consumed-once native-ID half of the stock-offline pre-Fill plan.
#[derive(Debug)]
pub(crate) struct NativeFreshIdPrefixReceipt {
    cursor: NativeUniqueIdCursor,
    #[cfg(test)]
    checkpoints: NativeFreshIdPrefixCheckpoints,
}

impl NativeFreshIdPrefixReceipt {
    pub(crate) fn into_cursor(self) -> NativeUniqueIdCursor {
        self.cursor
    }

    #[cfg(test)]
    pub(crate) fn checkpoints(&self) -> NativeFreshIdPrefixCheckpoints {
        self.checkpoints
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeFreshIdPrefixCheckpoints {
    pub(crate) after_early_types: u32,
    pub(crate) after_first_house_generation: u32,
    pub(crate) after_first_resize: u32,
    pub(crate) after_rebuilt_types: u32,
    pub(crate) after_final_house_generation: u32,
    pub(crate) after_final_resize: u32,
}

/// Fold the exact noncampaign Full_Init constructor generations without
/// constructing a second registry or touching Scenario RNG.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_noncampaign_fresh_id_prefix(
    early_type_count: usize,
    first_super_weapon_type_count: usize,
    first_house_count: usize,
    rebuilt_type_count: usize,
    final_super_weapon_type_count: usize,
    final_house_count: usize,
    map_width: u32,
    map_height: u32,
) -> NativeFreshIdPrefixReceipt {
    let mut value = FRESH_SCENARIO_NATIVE_ID_SEED;
    value = advance(value, early_type_count);
    #[cfg(test)]
    let after_early_types = value;
    value = advance(
        value,
        house_block_count(first_house_count, first_super_weapon_type_count),
    );
    #[cfg(test)]
    let after_first_house_generation = value;
    value = value.wrapping_add(resize_constructor_count(map_width, map_height));
    #[cfg(test)]
    let after_first_resize = value;
    value = advance(value, rebuilt_type_count);
    #[cfg(test)]
    let after_rebuilt_types = value;
    value = advance(
        value,
        house_block_count(final_house_count, final_super_weapon_type_count),
    );
    #[cfg(test)]
    let after_final_house_generation = value;
    value = value.wrapping_add(resize_constructor_count(map_width, map_height));
    #[cfg(test)]
    let after_final_resize = value;

    NativeFreshIdPrefixReceipt {
        cursor: NativeUniqueIdCursor::from_saved_prefix(value),
        #[cfg(test)]
        checkpoints: NativeFreshIdPrefixCheckpoints {
            after_early_types,
            after_first_house_generation,
            after_first_resize,
            after_rebuilt_types,
            after_final_house_generation,
            after_final_resize,
        },
    }
}

fn advance(value: u32, count: usize) -> u32 {
    value.wrapping_add(count as u32)
}

fn house_block_count(house_count: usize, super_weapon_type_count: usize) -> usize {
    house_count.wrapping_mul(super_weapon_type_count.wrapping_add(1))
}

fn resize_constructor_count(map_width: u32, map_height: u32) -> u32 {
    map_height
        .wrapping_mul(map_width.wrapping_mul(2).wrapping_sub(1))
        .wrapping_add(1)
}

#[cfg(test)]
mod tests {
    use super::{
        MAP_READ_NATIVE_ID_RESERVATION, NativeFreshIdPrefixCheckpoints,
        NativeMapTubeConstructionError, NativeUniqueIdCursor, build_noncampaign_fresh_id_prefix,
    };
    use crate::map::tubes::{AllocatedTubeParseError, TubeConstructionError};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::world::Simulation;

    fn simulation_with_saved_prefix(saved: u32) -> Simulation {
        let mut simulation = Simulation::with_seed(0);
        simulation.native_unique_ids = Some(NativeUniqueIdCursor::from_saved_prefix(saved));
        simulation
    }

    #[test]
    fn native_id_continuation_survives_production_snapshot_envelope() {
        use crate::sim::snapshot::GameSnapshot;
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/native_id_snapshot.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let input = case["input"].as_u64().unwrap() as u32;
            let mut sim =
                simulation_with_saved_prefix(input.wrapping_sub(MAP_READ_NATIVE_ID_RESERVATION));
            sim.session.map_name = "native-id-continuation".into();
            let cursor = sim.native_unique_ids.as_mut().unwrap();
            cursor.reserve_map_read_from_saved().unwrap();
            assert_eq!(cursor.current_raw(), case["saved"].as_u64().unwrap() as u32);
            let bytes = GameSnapshot::save_validated(&sim, 17, 23, "native ID continuation", 0);
            let mut restored =
                GameSnapshot::load_validated(&bytes, 17, 23, "native-id-continuation").unwrap();
            let restored_cursor = restored.sim.native_unique_ids.as_mut().unwrap();
            assert_eq!(
                restored_cursor.current_raw(),
                case["loaded"].as_u64().unwrap() as u32
            );
            assert_eq!(
                restored_cursor.next_id() as u32,
                case["next"].as_u64().unwrap() as u32
            );
            assert!(restored_cursor.reserve_map_read_from_saved().is_err());
        }
    }

    #[test]
    fn fixture_b_folds_both_house_and_resize_generations_in_order() {
        let receipt = build_noncampaign_fresh_id_prefix(0, 2, 2, 5, 2, 2, 2, 3);
        assert_eq!(
            receipt.checkpoints(),
            NativeFreshIdPrefixCheckpoints {
                after_early_types: 1_000_000,
                after_first_house_generation: 1_000_006,
                after_first_resize: 1_000_016,
                after_rebuilt_types: 1_000_021,
                after_final_house_generation: 1_000_027,
                after_final_resize: 1_000_037,
            }
        );
        assert_eq!(receipt.cursor.saved_after_fresh_prefix(), 1_000_037);
    }

    #[test]
    fn map_read_reservation_sets_from_wrapping_saved_snapshot() {
        let mut cursor = super::NativeUniqueIdCursor::from_saved_prefix(0xFFFF_FFF0);
        assert_eq!(cursor.next_id() as u32, 0xFFFF_FFF1);
        assert_eq!(
            cursor.reserve_map_read_from_saved().unwrap(),
            0xFFFF_FFF0u32.wrapping_add(MAP_READ_NATIVE_ID_RESERVATION)
        );
        assert_eq!(cursor.current_raw(), 0x0000_2700);
        assert!(cursor.reserve_map_read_from_saved().is_err());
    }

    #[test]
    fn raw_tubes_reserve_then_assign_every_source_row_in_order() {
        let ini = IniFile::from_str(
            "[Tubes]\n\
             7=7,0,6,4,0,6,6,-1\n\
             2=2,0,2,5,0,2,2,-1\n",
        );
        let mut simulation = simulation_with_saved_prefix(1_000_037);

        simulation.construct_native_map_tubes(&ini).unwrap();

        assert_eq!(
            simulation.native_unique_ids.as_ref().unwrap().current_raw(),
            1_010_039
        );
        let receipt = simulation.native_map_tubes.as_ref().unwrap();
        assert_eq!(receipt.entries.len(), 2);
        assert_eq!(receipt.entries[0].native_init.source_entry_ordinal, 0);
        assert_eq!(receipt.entries[0].native_init.native_unique_id, 1_010_038);
        assert_eq!(receipt.entries[0].fact.entry, (7, 0));
        assert_eq!(receipt.entries[1].native_init.source_entry_ordinal, 1);
        assert_eq!(receipt.entries[1].native_init.native_unique_id, 1_010_039);
        assert_eq!(receipt.entries[1].fact.entry, (2, 0));
    }

    #[test]
    fn allocated_malformed_tube_spends_one_id_then_stops_the_section() {
        let ini = IniFile::from_str(
            "[Tubes]\n\
             first=7,0,6,4,0,6,6,-1\n\
             bad=1,2,2,4,2,2,2\n\
             later=7,0,6,4,0,6,6,-1\n",
        );
        let mut simulation = simulation_with_saved_prefix(1_000_037);
        let mut allocation_visits = Vec::new();

        let error = simulation
            .construct_native_map_tubes_with_allocator(&ini, |source| {
                allocation_visits.push(source);
                true
            })
            .unwrap_err();

        assert_eq!(allocation_visits, vec![0, 1]);
        assert_eq!(
            error,
            NativeMapTubeConstructionError::Tube(TubeConstructionError::AllocatedRowMalformed {
                ordinal: 1,
                native_unique_id: 1_010_039,
                error: AllocatedTubeParseError::PathRunsOutBeforeNativeStop,
            })
        );
        assert_eq!(
            simulation.native_unique_ids.as_ref().unwrap().current_raw(),
            1_010_039
        );
        let receipt = simulation.native_map_tubes.as_ref().unwrap();
        assert_eq!(receipt.entries.len(), 1);
        assert_eq!(receipt.entries[0].native_init.source_entry_ordinal, 0);
        assert_eq!(receipt.entries[0].native_init.native_unique_id, 1_010_038);
    }

    #[test]
    fn tube_allocation_null_spends_no_id_then_stops_the_section() {
        let ini = IniFile::from_str(
            "[Tubes]\n\
             null=1,2,2,4,2,2,2,-1\n\
             later=7,0,6,4,0,6,6,-1\n",
        );
        let mut simulation = simulation_with_saved_prefix(1_000_037);
        let mut allocation_visits = Vec::new();

        let error = simulation
            .construct_native_map_tubes_with_allocator(&ini, |source| {
                allocation_visits.push(source);
                false
            })
            .unwrap_err();

        assert_eq!(allocation_visits, vec![0]);
        assert_eq!(
            error,
            NativeMapTubeConstructionError::Tube(TubeConstructionError::AllocationNull {
                ordinal: 0,
            })
        );
        assert_eq!(
            simulation.native_unique_ids.as_ref().unwrap().current_raw(),
            1_010_037
        );
        assert!(
            simulation
                .native_map_tubes
                .as_ref()
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn filtered_convenience_tubes_are_not_native_identity_authority() {
        let ini = IniFile::from_str(
            "[Tubes]\n\
             malformed=1,2,2,4,2,2,2\n\
             valid=7,0,6,4,0,6,6,-1\n",
        );
        assert_eq!(crate::map::tubes::parse_tubes(&ini).len(), 1);
        let mut simulation = simulation_with_saved_prefix(1_000_018);

        let error = simulation.construct_native_map_tubes(&ini).unwrap_err();

        assert_eq!(
            error,
            NativeMapTubeConstructionError::Tube(TubeConstructionError::AllocatedRowMalformed {
                ordinal: 0,
                native_unique_id: 1_010_019,
                error: AllocatedTubeParseError::PathRunsOutBeforeNativeStop,
            })
        );
        assert_eq!(
            simulation.native_unique_ids.as_ref().unwrap().current_raw(),
            1_010_019
        );
        assert!(
            simulation
                .native_map_tubes
                .as_ref()
                .unwrap()
                .entries
                .is_empty(),
            "the later convenience fact must never become a native binding"
        );
    }

    #[test]
    fn empty_tube_section_still_consumes_the_one_map_read_reservation() {
        let ini = IniFile::from_str("[Map]\nSize=0,0,2,3\n");
        let mut simulation = simulation_with_saved_prefix(0xFFFF_FFF0);

        simulation.construct_native_map_tubes(&ini).unwrap();

        assert_eq!(
            simulation.native_unique_ids.as_ref().unwrap().current_raw(),
            0x0000_2700
        );
        assert!(
            simulation
                .native_map_tubes
                .as_ref()
                .unwrap()
                .entries
                .is_empty()
        );
        assert_eq!(
            simulation.construct_native_map_tubes(&ini).unwrap_err(),
            NativeMapTubeConstructionError::ReceiptAlreadyInstalled
        );
    }
}
