//! Explicit `[Tubes]` map-section parsing.
//!
//! Map-authored tubes are full TubeClass records with entry/exit cells and a
//! path-step buffer. Automatic low-bridge shell tubes are constructed later in
//! resolved terrain from final land type and theater tile identity.

use crate::map::tube_facts::TubeFact;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::{crt_atoi, strtok};

const MIN_TUBE_FIELDS: usize = 5;
/// `MapClass::ReadTubesINI` 0x007283C0 ReadStrings each row into 0x200 bytes.
const TUBE_ROW_CAPACITY: usize = 0x200;
/// `MapClass::ReadTubesINI` 0x007283C0 runs its path loop at most 100 times
/// (`while (iVar6 < 100)`), so at most 99 steps are ever counted into the
/// TubeClass record's +0x1C0 length field.
const NATIVE_PATH_LOOP_ITERATIONS: usize = 100;
const TUBE_PATH_SENTINEL: i32 = -1;

#[derive(Debug)]
pub(crate) struct RawTubeRecord {
    source_entry_ordinal: usize,
    value: String,
}

/// Move-only raw section receipt. Gameplay obtains this directly from the INI
/// instead of using `MapFile::explicit_tubes`, whose safe convenience parser
/// deliberately filters malformed rows.
#[derive(Debug, Default)]
pub(crate) struct RawTubeSection {
    records: Vec<RawTubeRecord>,
}

impl RawTubeSection {
    pub(crate) fn from_ini(ini: &IniFile) -> Self {
        let Some(section) = ini.section("Tubes") else {
            return Self::default();
        };
        let records = section
            .keys()
            .enumerate()
            .map(|(source_entry_ordinal, key)| RawTubeRecord {
                source_entry_ordinal,
                value: section.read_string(key, "", TUBE_ROW_CAPACITY),
            })
            .collect();
        Self { records }
    }
}

/// Native constructor identity retained beside the successful Tube fact.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TubeNativeInit {
    pub(crate) source_entry_ordinal: usize,
    pub(crate) native_unique_id: i32,
}

/// One successfully allocated, assigned, and parsed authored Tube record.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ConstructedMapTube {
    pub(crate) fact: TubeFact,
    pub(crate) native_init: TubeNativeInit,
}

/// Consumed-once successful/partial receipt for the raw Tube boundary. `Some`
/// with zero entries distinguishes an empty section whose reservation ran from
/// a load that never reached the boundary.
#[derive(Debug, Default)]
pub(crate) struct NativeMapTubeReceipt {
    pub(crate) entries: Vec<ConstructedMapTube>,
}

/// Consumed-once ownership state for the raw `[Tubes]` boundary.
#[derive(Debug, Default)]
pub(crate) enum NativeMapTubesState {
    #[default]
    Unconstructed,
    Pending(NativeMapTubeReceipt),
    Bound,
}

impl NativeMapTubesState {
    #[cfg(test)]
    pub(crate) fn as_ref(&self) -> Option<&NativeMapTubeReceipt> {
        match self {
            Self::Pending(receipt) => Some(receipt),
            Self::Unconstructed | Self::Bound => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AllocatedTubeParseError {
    #[error("row has {actual} fields; at least {minimum} are required before the path")]
    MissingFixedFields { actual: usize, minimum: usize },
    #[error("path runs out of fields before -1 or the native 100-iteration bound")]
    PathRunsOutBeforeNativeStop,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TubeConstructionError {
    #[error("[Tubes] source row {ordinal} allocation returned null before native ID assignment")]
    AllocationNull { ordinal: usize },
    #[error("[Tubes] source row {ordinal} failed after native ID {native_unique_id}: {error}")]
    AllocatedRowMalformed {
        ordinal: usize,
        native_unique_id: i32,
        error: AllocatedTubeParseError,
    },
}

/// Execute the proved constructor boundary over raw source records.
///
/// Allocation is checked first, `AssignUniqueID` is invoked second, and only
/// then is the row tokenized. Returning an error stops the section immediately;
/// there is no reject-and-continue arm in active-retail `gamemd.exe`.
pub(crate) fn construct_raw_tube_section(
    section: RawTubeSection,
    receipt: &mut NativeMapTubeReceipt,
    allocate: &mut dyn FnMut(usize) -> bool,
    assign_native_id: &mut dyn FnMut() -> i32,
) -> Result<(), TubeConstructionError> {
    for record in section.records {
        let RawTubeRecord {
            source_entry_ordinal,
            value,
        } = record;
        if !allocate(source_entry_ordinal) {
            return Err(TubeConstructionError::AllocationNull {
                ordinal: source_entry_ordinal,
            });
        }
        let native_unique_id = assign_native_id();
        let fact = parse_allocated_tube_entry(&value).map_err(|error| {
            TubeConstructionError::AllocatedRowMalformed {
                ordinal: source_entry_ordinal,
                native_unique_id,
                error,
            }
        })?;
        receipt.entries.push(ConstructedMapTube {
            fact,
            native_init: TubeNativeInit {
                source_entry_ordinal,
                native_unique_id,
            },
        });
    }
    Ok(())
}

pub fn parse_tubes(ini: &IniFile) -> Vec<TubeFact> {
    let Some(section) = ini.section("Tubes") else {
        return Vec::new();
    };
    parse_tubes_section(section)
}

fn parse_tubes_section(section: &IniSection) -> Vec<TubeFact> {
    let mut tubes = Vec::new();
    for key in section.keys() {
        match parse_allocated_tube_entry(&section.read_string(key, "", TUBE_ROW_CAPACITY)) {
            Ok(tube) => tubes.push(tube),
            Err(error) => log::warn!("dropping malformed [Tubes] convenience fact: {error}"),
        }
    }
    if !tubes.is_empty() {
        log::info!("Parsed {} explicit map tubes from [Tubes]", tubes.len());
    }
    tubes
}

fn parse_allocated_tube_entry(value: &str) -> Result<TubeFact, AllocatedTubeParseError> {
    let fields: Vec<&str> = strtok(value, &[',']).collect();
    if fields.len() < MIN_TUBE_FIELDS {
        return Err(AllocatedTubeParseError::MissingFixedFields {
            actual: fields.len(),
            minimum: MIN_TUBE_FIELDS,
        });
    }

    let entry = (crt_atoi(fields[0]) as u16, crt_atoi(fields[1]) as u16);
    let direction = crt_atoi(fields[2]);
    let exit = (crt_atoi(fields[3]) as u16, crt_atoi(fields[4]) as u16);

    // `MapClass::ReadTubesINI` 0x007283C0 path loop. The native loop presets
    // the count at +0x1C0 to -1 and increments it for every value it writes,
    // including the sentinel, so the count always lands one below the number of
    // slots touched. It runs at most 100 times (`iVar6 < 100`), which means an
    // over-long path is silently truncated to 99 counted steps rather than
    // rejected: the 100th value is written into slot 99 and never counted.
    let mut path_steps = Vec::new();
    let mut terminated = false;
    for (slot, raw) in fields.iter().skip(MIN_TUBE_FIELDS).enumerate() {
        let step = crt_atoi(raw);
        if step == TUBE_PATH_SENTINEL {
            terminated = true;
            break;
        }
        if slot + 1 == NATIVE_PATH_LOOP_ITERATIONS {
            // Native `while (iVar6 < 100)` exit. The row survives with the
            // steps it accumulated; the remaining fields are never read.
            terminated = true;
            break;
        }
        path_steps.push(step);
    }
    if !terminated {
        // Active-retail faults after construction/ID assignment when strtok
        // returns NULL before either stop condition. Rust surfaces that same
        // boundary as a hard load error in the gameplay constructor. The
        // legacy convenience parser logs and drops it instead.
        return Err(AllocatedTubeParseError::PathRunsOutBeforeNativeStop);
    }

    Ok(TubeFact::explicit(entry, exit, direction, path_steps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tube_facts::TubeSource;

    #[test]
    fn parse_tubes_preserves_entry_exit_direction_and_steps_until_sentinel() {
        let ini = IniFile::from_str("[Tubes]\n0=1,2,2,4,2,2,2,-1,6\n");

        let tubes = parse_tubes(&ini);

        assert_eq!(tubes.len(), 1);
        assert_eq!(tubes[0].entry, (1, 2));
        assert_eq!(tubes[0].exit, (4, 2));
        assert_eq!(tubes[0].direction, 2);
        assert_eq!(tubes[0].path_steps, vec![2, 2]);
        assert_eq!(tubes[0].source, TubeSource::ExplicitMap);
    }

    #[test]
    fn parse_tubes_preserves_source_order() {
        let ini = IniFile::from_str(
            "[Tubes]\n\
             7=7,0,6,4,0,6,6,-1\n\
             2=2,0,2,5,0,2,2,-1\n",
        );

        let tubes = parse_tubes(&ini);

        assert_eq!(tubes.len(), 2);
        assert_eq!(tubes[0].entry, (7, 0));
        assert_eq!(tubes[1].entry, (2, 0));
    }

    /// `MapClass::ReadTubesINI` 0x007283C0 does not reject an over-long path.
    /// Its `while (iVar6 < 100)` loop stops after 100 iterations, and the
    /// +0x1C0 counter it maintains lands on 99, so the row survives with 99
    /// steps and every later field is ignored.
    #[test]
    fn gsi_04_15_over_long_path_truncates_to_the_native_loop_bound() {
        let mut value = String::from("0,0,2,100,0");
        for _ in 0..105 {
            value.push_str(",2");
        }
        let ini = IniFile::from_str(&format!("[Tubes]\n0={value}\n"));

        let tubes = parse_tubes(&ini);

        assert_eq!(tubes.len(), 1, "gamemd keeps the row; it does not drop it");
        assert_eq!(tubes[0].path_len(), 99);
        assert_eq!(tubes[0].entry, (0, 0));
        assert_eq!(tubes[0].exit, (100, 0));
    }

    /// A sentinel sitting exactly on the last slot the native loop reaches is
    /// still a sentinel: iteration 100 writes -1 and breaks with the counter on
    /// 99, the same length the truncation path produces.
    #[test]
    fn gsi_04_15_sentinel_on_the_final_native_slot_yields_99_steps() {
        let mut value = String::from("0,0,2,100,0");
        for _ in 0..99 {
            value.push_str(",2");
        }
        value.push_str(",-1,7,7");
        let ini = IniFile::from_str(&format!("[Tubes]\n0={value}\n"));

        let tubes = parse_tubes(&ini);

        assert_eq!(tubes.len(), 1);
        assert_eq!(tubes[0].path_len(), 99);
    }

    #[test]
    fn missing_tubes_section_returns_empty_vec() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");

        assert!(parse_tubes(&ini).is_empty());
    }

    #[test]
    fn gsi_04_15_raw_atoi_values_and_low16_coordinates_are_preserved() {
        let ini = IniFile::from_str("[Tubes]\nname=-1,65537,9tail,4,2,10,-2,15,-1,6\n");

        let tubes = parse_tubes(&ini);

        assert_eq!(tubes.len(), 1);
        assert_eq!(tubes[0].entry, (u16::MAX, 1));
        assert_eq!(tubes[0].direction, 9);
        assert_eq!(tubes[0].path_steps, vec![10, -2, 15]);
    }

    /// Safe convenience parsing drops the row. The gameplay constructor is
    /// stricter: it assigns first, then returns a hard load error at this
    /// boundary so later rows and Overlay construction never run.
    #[test]
    fn gsi_04_15_missing_path_sentinel_is_rejected_safely() {
        let ini = IniFile::from_str("[Tubes]\n0=1,2,2,4,2,2,2\n");
        assert!(parse_tubes(&ini).is_empty());
    }

    // RESIDUAL — gamemd address 0x007283C0, `MapClass::ReadTubesINI`.
    //
    // Trigger, corrected: a `[Tubes]` row that runs out of fields before it
    // reaches either a `-1` or the native loop's 100th slot. A row that
    // simply has no sentinel inside 100 fields is NOT this case - since the
    // truncation fix in this module it is kept at 99 steps, matching the
    // native counter.
    //
    // Effect: gamemd calls `CRT__strtok` 0x007C9CC2 past the end of the row,
    // gets NULL, and passes it straight to `CRT__atoi` 0x007C9B72, which
    // dereferences it with no null test — an access violation during map
    // load. VERA spends the same constructor ID and aborts the load with a
    // typed error rather than terminating the process.
    //
    // Frequency: never in a retail map — the YR map editor always emits the
    // sentinel. Reachable only through a hand-edited or third-party map.
    //
    // Left divergent deliberately only in host failure form: matching gamemd
    // means crashing. Constructor cost and no-continuation behavior match.
    // Residual (formerly an ignored placeholder test): gamemd faults the process; VERA returns a hard load error after the same ID spend.
    // Intentional divergence: gamemd access-faults where VERA hard-errors the load.
}
