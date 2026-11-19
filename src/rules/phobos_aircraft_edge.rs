//! Phobos `SpawnFromEdge=` (`Ext/AircraftType/Body.cpp:24`, declared
//! `Ext/AircraftType/Body.h:15` as `Nullable<EdgeType>`) and the `[General]
//! AircraftSpawnFromEdge=` fallback (`Ext/Rules/Body.cpp:193`,
//! `Ext/Rules/Body.h:99`, constructed `Owner` at `Body.h:636`), plus their
//! `Mission_Retreat` siblings `RetreatToEdge=` (`Body.cpp:25`) and `[General]
//! AircraftRetreatToEdge=` (`Ext/Rules/Body.cpp:194`, `Body.h:100`, `Owner` at
//! `Body.h:637`).
//!
//! `AircraftExt::PickEdgeCellForPlane` (`Ext/Aircraft/Body.cpp:85-126`) reads
//! the pair for every aircraft edge pick: `Owner` keeps the edge the caller
//! passed, `Closest` scatters the destination cell and picks on `Edge::None`,
//! and `Random` spends one `Scenario RandomRanged(North, West)` draw to choose
//! the edge. Callers: `HouseClass::SendSpyPlanes` (`0x65EB2D`),
//! `SendAirstrike` (`0x65E881`), `SendParadrop` (`0x65E6DB`) and
//! `AircraftClass::Mission_Retreat` (`0x415A7A`), hooked at
//! `Ext/House/Hooks.cpp:352-450`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

pub use crate::rules::ini_value::PhobosEdgeType;

/// An AircraftType's own `SpawnFromEdge=`, unset when the key is absent or
/// rejected, so the `[General]` value applies.
pub fn read_spawn_from_edge(section: &IniSection) -> Option<PhobosEdgeType> {
    section.read_phobos_nullable_with("SpawnFromEdge", None, PhobosEdgeType::parse)
}

/// An AircraftType's own `RetreatToEdge=` (`Ext/AircraftType/Body.cpp:25`,
/// `Nullable<EdgeType>` `Body.h:16`, constructed unset `Body.h:40`): the edge
/// `Mission_Retreat` picks on (`0x415A7A`, `Ext/Aircraft/Body.cpp:89`), unset
/// falling back to `[General] AircraftRetreatToEdge`.
pub fn read_retreat_to_edge(section: &IniSection) -> Option<PhobosEdgeType> {
    section.read_phobos_nullable_with("RetreatToEdge", None, PhobosEdgeType::parse)
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn the_spawn_from_edge_key_parses_the_three_names() {
        assert_eq!(PhobosEdgeType::parse("owner"), Some(PhobosEdgeType::Owner));
        assert_eq!(
            PhobosEdgeType::parse("Closest"),
            Some(PhobosEdgeType::Closest)
        );
        assert_eq!(
            PhobosEdgeType::parse("random"),
            Some(PhobosEdgeType::Random)
        );
        assert_eq!(PhobosEdgeType::parse("nearest"), None);
    }

    #[test]
    fn an_aircraft_reads_its_own_spawn_from_edge_and_an_unset_one_stays_none() {
        let ini = IniFile::from_str("[PLANE]\nSpawnFromEdge=random\n[OTHER]\nStart=0\n");
        assert_eq!(
            read_spawn_from_edge(ini.section("PLANE").unwrap()),
            Some(PhobosEdgeType::Random)
        );
        assert_eq!(read_spawn_from_edge(ini.section("OTHER").unwrap()), None);
        let bad = IniFile::from_str("[PLANE]\nSpawnFromEdge=nearest\n");
        assert_eq!(read_spawn_from_edge(bad.section("PLANE").unwrap()), None);
    }
}
