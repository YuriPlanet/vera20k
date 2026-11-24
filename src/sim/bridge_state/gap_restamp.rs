//! Original586BF0: fresh-load inactive high-record gap flags, not a zone rebuild.
//! See PHASE3_BRIDGE_RECORD_GAP_RESTAMP_NATIVE_REPORT.md for caller/preservation.

use super::BridgeEndpointRecord;
use crate::map::resolved_terrain::ResolvedTerrainGrid;

pub(crate) fn restamp_inactive_high_records(
    terrain: &mut ResolvedTerrainGrid,
    records: &[BridgeEndpointRecord],
    mut publish: impl FnMut((i16, i16), Option<usize>, u32),
) {
    for record in records
        .iter()
        .rev()
        .filter(|record| record.is_high() && !record.active)
    {
        let a = (record.endpoint_a.0 as i16, record.endpoint_a.1 as i16);
        let b = (record.endpoint_b.0 as i16, record.endpoint_b.1 as i16);
        let horizontal = a.0 != b.0;
        // Production ComputeBridgeZones emits axis-aligned records. Corrupted
        // external records outside that invariant would not terminate natively.
        assert!(
            !horizontal || a.1 == b.1,
            "native bridge producer must emit an axis-aligned span"
        );
        let step = if horizontal {
            if b.0 > a.0 { (1i16, 0) } else { (-1, 0) }
        } else if b.1 > a.1 {
            (0, 1i16)
        } else {
            (0, -1)
        };
        let mut cursor = (a.0.wrapping_add(step.0), a.1.wrapping_add(step.1));
        while cursor != b {
            // Original center probe can itself stamp the shared dummy; do not
            // replace it with a rectangular allocation/presence predicate.
            if terrain.cellclass_bridge_flags_0x1180(i32::from(cursor.0), i32::from(cursor.1))
                & 0x100
                == 0
            {
                for offset in -2i16..=1 {
                    let requested = if horizontal {
                        (cursor.0, cursor.1.wrapping_add(offset))
                    } else {
                        (cursor.0.wrapping_add(offset), cursor.1)
                    };
                    let (index, flags) = terrain.apply_bridge_gap_flag_write(requested, horizontal);
                    publish(requested, index, flags);
                }
            }
            cursor = (cursor.0.wrapping_add(step.0), cursor.1.wrapping_add(step.1));
        }
    }
}
