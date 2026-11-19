//! Original56DB70 and56DAE0: record owner and synchronous activation callbacks.
//! Evidence: spatial_oracle/bridge_repair_zones and bridge_records.
use super::*;
use crate::sim::pathfinding::zone_build::find_high_bridge_record_index;

impl BridgeRuntimeState {
    /// Shared56DB70/56DAE0 prologue: an initial FindBridgeRecord (56DA10,
    /// radius3) miss recomputes the native record vector (56D6E0) once.
    fn first_query_record(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        query: (u16, u16),
    ) -> Option<usize> {
        let first = find_high_bridge_record_index(&self.endpoint_records, 0, query, 3);
        if first.is_some() {
            return first;
        }
        self.endpoint_records =
            record_scan::compute_bridge_endpoints(terrain, self.native_zone_source_size);
        find_high_bridge_record_index(&self.endpoint_records, 0, query, 3)
    }

    /// Every matching inactive record is activated before its edge/reachability
    /// callback; an unsupported callback retains all preceding native-visible
    /// writes.
    pub(crate) fn validate_repaired_zones(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        query: (i16, i16),
        mut activated: impl FnMut(&BridgeEndpointRecord) -> Result<bool, String>,
    ) -> Result<bool, String> {
        let query = (query.0 as u16, query.1 as u16);
        let mut next = self.first_query_record(terrain, query);
        let mut connectivity = false;
        while let Some(index) = next {
            let record = &mut self.endpoint_records[index];
            if !record.active {
                record.active = true;
                connectivity |= activated(record)?;
            }
            next = find_high_bridge_record_index(&self.endpoint_records, index + 1, query, 3);
        }
        Ok(connectivity)
    }

    /// MapClass::InvalidateBridgeZones (56DAE0), called by both structural
    /// damage machines after a collapse. Every matching active record is
    /// deactivated; the return requests56C510.
    ///
    /// Residual: the per-record RemoveBridgeZoneEdges (584E50) hierarchy edge
    /// removal is not ported. Whenever a bridge span falls, the caller's full
    /// navigation rebuild derives the graph from the active records instead,
    /// discarding 56DB70's append order and the zone IDs and holes of earlier
    /// 584550 repairs. Base zones and Can_Reach_Zone answers are the same, so
    /// only ties in Zone_precheck (42C290) can differ: a unit may take another
    /// equal-cost corridor after a bridge falls, which no player can tell.
    pub(crate) fn invalidate_bridge_zones(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        query: (i16, i16),
    ) -> bool {
        let query = (query.0 as u16, query.1 as u16);
        let mut next = self.first_query_record(terrain, query);
        let mut changed = false;
        while let Some(index) = next {
            let record = &mut self.endpoint_records[index];
            if record.active {
                record.active = false;
                changed = true;
            }
            next = find_high_bridge_record_index(&self.endpoint_records, index + 1, query, 3);
        }
        changed
    }
}
