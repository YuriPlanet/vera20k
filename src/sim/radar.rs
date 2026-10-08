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
//! `render::radar_events` owns admission, dedup, animation and the Spacebar
//! review ring.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ (via entity components).
//! - NEVER depends on render/, ui/, sidebar/, audio/, net/.

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
    use crate::map::entities::EntityCategory;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::game_entity::GameEntity;
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

    fn spawn_building(sim: &mut Simulation, id: u64, owner: &str, type_id: &str) {
        let owner_id = sim.interner.intern(owner);
        let type_ref = sim.interner.intern(type_id);
        let mut e = GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            owner_id,
            crate::sim::components::Health { current: 100 },
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        e.lifecycle.in_limbo = false;
        e.lifecycle.cell_marked = true;
        sim.substrate.entities.insert(e);
    }

    #[test]
    fn no_radar_without_building() {
        let sim = Simulation::new();
        let rules = make_rules_with_radar();
        assert!(!has_radar_for_owner(&sim, &rules, "Americans"));
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
            state.total_output = output;
            state.total_drain = drain;
            state.is_low_power = output < drain;
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
                sim.session.free_radar,
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

    #[test]
    fn free_radar_bypasses_low_power_and_power_blackout_with_a_provider() {
        let mut sim = Simulation::new();
        let rules = make_rules_with_radar();
        spawn_building(&mut sim, 1, "Americans", "GARADR");
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
            0,
        );
        let owner = sim.interner.get("Americans").unwrap();
        assert!(sim.power_states[&owner].is_low_power);
        assert!(!has_radar_for_owner(&sim, &rules, "Americans"));
        // House508DF0 reads Scenario FreeRadar when the House next rechecks.
        let recheck = |sim: &mut Simulation| {
            let state = sim.power_states.get_mut(&owner).unwrap();
            state.recheck_radar();
            crate::sim::power_system::assess_house_radar_projection(
                state,
                &sim.substrate.entities,
                &[1],
                &rules,
                owner,
                &sim.interner,
                true,
                0,
            );
        };
        recheck(&mut sim);
        assert!(has_radar_for_owner(&sim, &rules, "Americans"));
        sim.power_states
            .get_mut(&owner)
            .unwrap()
            .start_blackout(0, 100);
        recheck(&mut sim);
        assert!(
            has_radar_for_owner(&sim, &rules, "Americans"),
            "power outage is not native radar outage"
        );
    }

    #[test]
    fn radar_with_powered_building() {
        let mut sim = Simulation::new();
        let rules = make_rules_with_radar();
        // Spawn power plant (Power=200)
        spawn_building(&mut sim, 1, "Americans", "GAPOWR");
        // Spawn radar building (Power=-40)
        spawn_building(&mut sim, 2, "Americans", "GARADR");
        // Tick power states so cached state reflects the buildings.
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
            0,
        );
        assert!(has_radar_for_owner(&sim, &rules, "Americans"));
    }

    #[test]
    fn no_radar_when_low_power() {
        let mut sim = Simulation::new();
        let rules = make_rules_with_radar();
        // Only radar building, no power plant — drained > produced
        spawn_building(&mut sim, 1, "Americans", "GARADR");
        // Tick power states so low-power is detected.
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
            0,
        );
        assert!(!has_radar_for_owner(&sim, &rules, "Americans"));
    }

    #[test]
    fn spy_sat_does_not_replace_a_radar_provider() {
        let mut sim = Simulation::new();
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GASPYSAT\n\
             [GASPYSAT]\nName=Spy Satellite\nSpySat=yes\nPower=0\nFoundation=2x2\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("spy satellite test rules");
        spawn_building(&mut sim, 1, "Americans", "GASPYSAT");
        crate::sim::power_system::tick_power_states(
            &mut sim.power_states,
            &mut sim.substrate.entities,
            &rules,
            &sim.interner,
            0,
        );

        assert!(
            !has_radar_for_owner(&sim, &rules, "Americans"),
            "SpySat full-map reveal is separate from the native Radar=yes gate"
        );
    }

    #[test]
    fn native_event_discriminants_index_the_compiled_rows() {
        let variants = [
            RadarEventType::Combat,
            RadarEventType::Noncombat,
            RadarEventType::Dropzone,
            RadarEventType::BaseUnderAttack,
            RadarEventType::HarvesterUnderAttack,
            RadarEventType::EnemyObjectSensed,
            RadarEventType::UnitReady,
            RadarEventType::UnitLost,
            RadarEventType::UnitRepaired,
            RadarEventType::SpyInfiltration,
            RadarEventType::BuildingCaptured,
            RadarEventType::BeaconPlaced,
            RadarEventType::ConstructionComplete,
            RadarEventType::ImpactSilent,
            RadarEventType::BridgeRepaired,
            RadarEventType::StructureAbandoned,
            RadarEventType::AllyUnderAttack,
        ];
        for (index, event_type) in variants.into_iter().enumerate() {
            assert_eq!(event_type as usize, index);
        }
    }
}
