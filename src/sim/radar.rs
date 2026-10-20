//! Radar availability detection and radar-event requests.
//!
//! Map `[Basic] FreeRadar` enables radar independently of buildings and power.
//! Otherwise buildings with `Radar=yes` provide the tactical
//! radar; `SpySat=yes` is handled by the separate shroud-reveal path. The
//! ordinary provider branch goes offline at negative house power balance.
//!
//! Radar events themselves are client-local: native `RadarClass` keeps one
//! event array per process beside its radar surfaces. The simulation therefore
//! only names the event it reached ([`RadarEventRequest`]);
//! `render::radar::events` owns admission, dedup, animation and the Spacebar
//! review ring.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ (via entity components).
//! - NEVER depends on render/, ui/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// The owner's radar as its House last assessed it.
///
/// Native House `0x00508DF0` denies radar while the House's radar outage runs,
/// then grants it for Scenario+34A4 (map FreeRadar) before power and
/// providers: a building provides radar if its ObjectType has `Radar=yes` and
/// the house is not in low power. This is a house-level gate; stock Allied
/// `GAAIRC` and `AMRADR` omit `Powered=yes`. The app supplies its preferred
/// local owner.
pub fn has_radar_for_owner(sim: &Simulation, _rules: &RuleSet, owner: &str) -> bool {
    let Some(owner_id) = sim.interner.get(owner) else {
        return false;
    };
    crate::sim::power_system::has_active_radar(&sim.power_states, owner_id)
}

/// Native runtime radar event type (`CreateRadarEvent @ 0x0065FA70`'s ECX).
/// The discriminant indexes the client's compiled type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RadarEventType {
    Combat = 0,
    Noncombat = 1,
    Dropzone = 2,
    BaseUnderAttack = 3,
    HarvesterUnderAttack = 4,
    EnemyObjectSensed = 5,
    UnitReady = 6,
    UnitLost = 7,
    UnitRepaired = 8,
    SpyInfiltration = 9,
    BuildingCaptured = 10,
    BeaconPlaced = 11,
    ConstructionComplete = 12,
    ImpactSilent = 13,
    BridgeRepaired = 14,
    StructureAbandoned = 15,
    AllyUnderAttack = 16,
}

impl TryFrom<i32> for RadarEventType {
    type Error = ();

    /// The compiled65FA70 type-table ordinal. Invalid native array indexes
    /// are rejected rather than addressing memory outside the table.
    fn try_from(index: i32) -> Result<Self, Self::Error> {
        Ok(match index {
            0 => Self::Combat,
            1 => Self::Noncombat,
            2 => Self::Dropzone,
            3 => Self::BaseUnderAttack,
            4 => Self::HarvesterUnderAttack,
            5 => Self::EnemyObjectSensed,
            6 => Self::UnitReady,
            7 => Self::UnitLost,
            8 => Self::UnitRepaired,
            9 => Self::SpyInfiltration,
            10 => Self::BuildingCaptured,
            11 => Self::BeaconPlaced,
            12 => Self::ConstructionComplete,
            13 => Self::ImpactSilent,
            14 => Self::BridgeRepaired,
            15 => Self::StructureAbandoned,
            16 => Self::AllyUnderAttack,
            _ => return Err(()),
        })
    }
}

/// A `CreateRadarEvent(type, cell)` call the simulation reached.
///
/// It carries no acceptance result: the live event array that dedupes it is
/// the local client's, so the consumer that filters the event to the local
/// player also decides admission and any EVA line gated on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadarEventRequest {
    pub event_type: RadarEventType,
    /// Isometric cell of the event.
    pub rx: u16,
    pub ry: u16,
}

impl RadarEventRequest {
    pub const fn new(event_type: RadarEventType, rx: u16, ry: u16) -> Self {
        Self { event_type, rx, ry }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::ini_parser::IniFile;

    use crate::sim::world::Simulation;

    fn make_rules_with_radar() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GARADR\n1=GAPOWR\n\
             [GARADR]\nName=Radar\nRadar=yes\nPower=-40\nFoundation=2x2\n\
             [GAPOWR]\nName=Power Plant\nStrength=100\nPower=200\nFoundation=2x2\n",
        );
        RuleSet::from_ini(&ini).expect("radar test rules")
    }

    #[test]
    fn free_radar_and_radar_outage_match_original_empty_provider_decisions() {
        let original: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/free_radar_oracle/fixtures/native-free-radar.json",
        ))
        .unwrap();
        let rules = make_rules_with_radar();
        let mut checked = 0;
        for case in original["availability_cases"].as_array().unwrap() {
            let timer = case["timer_start_duration_frame"].as_array().unwrap();
            let timer: Vec<_> = timer.iter().map(|v| v.as_i64().unwrap() as i32).collect();
            let mut sim =
                Simulation::from_descriptor(&crate::sim::scenario_session::ScenarioDescriptor {
                    free_radar: case["free_radar"].as_u64().unwrap() != 0,
                    ..Default::default()
                });
            let owner = sim.interner.intern("Americans");
            let power = case["power_output_drain"].as_array().unwrap();
            let output = power[0].as_i64().unwrap() as i32;
            let drain = power[1].as_i64().unwrap() as i32;
            let mut state = crate::sim::power_system::PowerState::default();
            state.set_total_output_for_test(output);
            state.set_total_drain_for_test(drain);
            state.set_is_low_power_for_test(output < drain);
            state.set_radar_outage_for_test(crate::sim::timer::CdTimer::from_raw(
                timer[0], timer[1],
            ));
            crate::sim::power_system::assess_house_radar_projection(
                &mut state,
                &sim.substrate.entities,
                &[],
                &rules,
                owner,
                &sim.interner,
                sim.session.free_radar(),
                timer[2] as u32,
            );
            sim.power_states.insert(owner, state);
            assert_eq!(
                has_radar_for_owner(&sim, &rules, "Americans"),
                case["available"].as_u64().unwrap() != 0,
                "{case}"
            );
            assert!(!has_radar_for_owner(&sim, &rules, "UnknownOwner"));
            checked += 1;
        }
        assert_eq!(checked, 70);
    }
}
