//! Slave Miner deploy and undeploy: the SMIN vehicle becomes its refinery
//! (YAREFN) and back, and the slave manager moves with it.
//!
//! The slaves and their harvest cycle belong to the manager
//! (`sim::slave_manager`). Each conversion constructs the new form, whose
//! constructor builds a manager of its own, then hands the old form's
//! manager over (SetOwner `0x006AF580`), which frees the new form's fresh
//! slaves: `UnitClass::Deploy` (`0x00739956`, after the hand-off
//! `0x006B0D10`) and `BuildingClass::Sell` (`0x0044A047`).
//!
//! RESIDUAL: the conversions themselves are VERA's command paths, not
//! `UnitClass::Deploy`'s Unload mission or the Selling mission's undeploy
//! (chain 6, with the manager's relocation states).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/slave_manager, sim/world, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// Deploy a Slave Miner (SMIN) vehicle into its refinery form (YAREFN).
///
/// Returns the new YAREFN stable_id, or None if deploy failed.
#[cfg(test)]
pub fn deploy_slave_miner(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) -> Option<u64> {
    deploy_slave_miner_with_overlay_context(sim, stable_id, rules, None)
}

pub(crate) fn deploy_slave_miner_with_overlay_context(
    sim: &mut Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<u64> {
    let (owner, rx, ry, z, was_selected, target_type, converted_health) = {
        let entity = sim.substrate.entities.get(stable_id)?;
        let type_str = sim.interner.resolve(entity.type_ref());
        let obj = rules.object_case_insensitive(type_str)?;
        let target_type: &str = obj.deploys_into.as_deref()?;
        obj.enslaves.as_ref()?;
        (
            sim.interner.resolve(entity.owner()).to_string(),
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            entity.selected,
            target_type.to_string(),
            crate::sim::conversion_health::ConversionHealth::capture(
                entity,
                obj,
                rules.object(target_type)?,
                crate::sim::conversion_health::ConversionKind::Unit,
            ),
        )
    };

    // The vehicle leaves; its manager stays on it until the hand-off below.
    sim.uninit_with_rules(stable_id, rules);

    let new_sid: u64 = sim.spawn_object_at_height_with_overlay_context(
        &target_type,
        &owner,
        rx,
        ry,
        0,
        z,
        rules,
        overlay_registry,
    )?;

    if let Some(ge) = sim.substrate.entities.get_mut(new_sid) {
        converted_health.apply(ge);
        ge.selected = was_selected;
    }
    sim.transfer_slave_manager(stable_id, new_sid, true, rules, overlay_registry);
    Some(new_sid)
}

/// Undeploy a Slave Miner refinery (YAREFN) back into vehicle form (SMIN).
///
/// Returns the new SMIN stable_id, or None if undeploy failed.
#[cfg(test)]
pub fn undeploy_slave_miner(sim: &mut Simulation, stable_id: u64, rules: &RuleSet) -> Option<u64> {
    undeploy_slave_miner_with_overlay_context(sim, stable_id, rules, None)
}

pub(crate) fn undeploy_slave_miner_with_overlay_context(
    sim: &mut Simulation,
    stable_id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> Option<u64> {
    let (owner, rx, ry, z, was_selected, target_type, converted_health) = {
        let entity = sim.substrate.entities.get(stable_id)?;
        let type_str = sim.interner.resolve(entity.type_ref());
        let obj = rules.object_case_insensitive(type_str)?;
        let target_type: &str = obj.undeploys_into.as_deref()?;
        rules.object(target_type)?;
        (
            sim.interner.resolve(entity.owner()).to_string(),
            entity.position.rx,
            entity.position.ry,
            entity.position.z,
            entity.selected,
            target_type.to_string(),
            crate::sim::conversion_health::ConversionHealth::capture(
                entity,
                obj,
                rules.object(target_type)?,
                crate::sim::conversion_health::ConversionKind::Building,
            ),
        )
    };

    sim.uninit_with_rules(stable_id, rules);

    let new_sid: u64 = sim.spawn_object_at_height_with_overlay_context(
        &target_type,
        &owner,
        rx,
        ry,
        0,
        z,
        rules,
        overlay_registry,
    )?;

    if let Some(ge) = sim.substrate.entities.get_mut(new_sid) {
        converted_health.apply(ge);
        ge.selected = was_selected;
    }
    sim.transfer_slave_manager(stable_id, new_sid, false, rules, overlay_registry);
    Some(new_sid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::SimRng;
    use crate::sim::slave_manager::ManagerState;

    #[test]
    fn deploy_and_undeploy_hand_the_slave_manager_over() {
        let rules = make_test_rules();
        let seed = 0x51A7_E001;
        let mut sim = Simulation::with_seed(seed);
        let mut expected = SimRng::new(seed);
        let smin = sim
            .spawn_object_at_height("SMIN", "YuriCountry", 10, 10, 0, 0, &rules)
            .expect("spawn SMIN");
        // The SMIN's constructor word, then one per slave it builds.
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        let slaves: Vec<u64> = sim
            .substrate
            .entities
            .get(smin)
            .unwrap()
            .slave_manager
            .as_ref()
            .unwrap()
            .slaves()
            .collect();
        assert_eq!(slaves, vec![2, 3, 4, 5, 6]);

        let yarefn = deploy_slave_miner(&mut sim, smin, &rules).expect("deploy to YAREFN");
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        let manager = sim
            .substrate
            .entities
            .get(yarefn)
            .unwrap()
            .slave_manager
            .as_ref()
            .unwrap();
        assert_eq!(manager.slaves().collect::<Vec<_>>(), slaves);
        // 0x006B0D10: the idle manager moves to 4 on the way.
        assert_eq!(manager.state(), ManagerState::Deployed);
        assert!(
            sim.substrate
                .entities
                .get(smin)
                .unwrap()
                .slave_manager
                .is_none()
        );
        // The refinery's own fresh slaves were freed: UnInit in limbo.
        for fresh in 8..=12 {
            assert!(
                !sim.substrate
                    .entities
                    .get(fresh)
                    .unwrap()
                    .lifecycle
                    .object_alive
            );
        }
        for &slave in &slaves {
            assert_eq!(
                sim.substrate.entities.get(slave).unwrap().slave.owner(),
                Some(yarefn)
            );
        }

        let back = undeploy_slave_miner(&mut sim, yarefn, &rules).expect("undeploy to SMIN");
        for _ in 0..6 {
            let _ = expected.next_u32();
        }
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        assert_eq!(back, 13);
        let manager = sim
            .substrate
            .entities
            .get(back)
            .unwrap()
            .slave_manager
            .as_ref()
            .unwrap();
        assert_eq!(manager.slaves().collect::<Vec<_>>(), slaves);
        for fresh in 14..=18 {
            assert!(
                !sim.substrate
                    .entities
                    .get(fresh)
                    .unwrap()
                    .lifecycle
                    .object_alive
            );
        }
        for slave in slaves {
            assert_eq!(
                sim.substrate.entities.get(slave).unwrap().slave.owner(),
                Some(back)
            );
        }
    }

    /// Minimal rules for slave miner tests.
    fn make_test_rules() -> RuleSet {
        use crate::rules::ini_parser::IniFile;
        let ini_str: &str = "\
[InfantryTypes]\n1=SLAV\n\
[VehicleTypes]\n1=SMIN\n\
[BuildingTypes]\n1=YAREFN\n\
[SLAV]\nStrength=125\nSpeed=3\nSlaved=yes\nStorage=4\nHarvestRate=150\n\
[SMIN]\nStrength=2000\nSpeed=3\nEnslaves=SLAV\nSlavesNumber=5\nDeploysInto=YAREFN\nResourceGatherer=yes\nResourceDestination=yes\n\
[YAREFN]\nStrength=2000\nEnslaves=SLAV\nSlavesNumber=5\nUndeploysInto=SMIN\nFoundation=3x3\n\
";
        let ini: IniFile = IniFile::from_str(ini_str);
        RuleSet::from_ini(&ini).expect("test rules should parse")
    }
}
