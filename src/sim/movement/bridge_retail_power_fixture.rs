//! Establish retail Robot Tank support before measuring bridge movement.

use crate::headless_scenario::HeadlessScenario;
use crate::sim::runtime::SimRuntime;

pub(super) fn prepare_powered_mover(
    scenario: &mut HeadlessScenario,
    owner_name: &str,
    unit_type: &str,
    approach: (u16, u16),
) {
    if !scenario
        .runtime
        .resources
        .rules
        .object(unit_type)
        .expect("retail mover type")
        .powered_unit
    {
        return;
    }

    // Unit739F21..739F59 correctly deactivates a PoweredUnit without its
    // House50E1B0 center. Stock ROBO needs an operational GAROBO, whose
    // Powered/Power=-100 in turn needs GAPOWR. Use the existing placement,
    // spawn, house-power and operational-edge owners rather than bypassing
    // their latches. These are fixture buildings, away from the crossing.
    let (owner, center) = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        let rules = &resources.rules;
        assert_eq!(
            rules.object("GAROBO").unwrap().powers_unit.as_deref(),
            Some(unit_type),
            "this fixture establishes the retail Robot Tank prerequisite"
        );
        let owner = simulation.interner.get(owner_name).unwrap();
        let sites: Vec<_> = simulation
            .resolved_terrain
            .as_ref()
            .expect("retail terrain")
            .iter()
            .map(|cell| (cell.rx, cell.ry))
            .filter(|&(rx, ry)| rx.abs_diff(approach.0).max(ry.abs_diff(approach.1)) >= 32)
            .collect();
        let mut center = None;
        for name in ["GAPOWR", "GAROBO"] {
            let object = rules.object(name).expect("retail support building");
            let (rx, ry) = sites
                .iter()
                .copied()
                .find(|&(rx, ry)| {
                    crate::sim::build_site::can_place_building_at(
                        simulation,
                        rules,
                        Some(&resources.overlay_registry),
                        object,
                        (rx as i16, ry as i16),
                        Some(owner),
                        None,
                    )
                })
                .unwrap_or_else(|| panic!("no legal site away from the bridge for {name}"));
            let id = simulation
                .spawn_object(name, owner_name, rx, ry, 0, rules)
                .unwrap_or_else(|| panic!("{name} must spawn at its admitted site"));
            if name == "GAROBO" {
                center = Some(id);
            }
        }
        simulation.resolve_type_handles(rules);
        (owner, center.unwrap())
    };

    // The center's first on edge Reactivates its units and resets their
    // orders. Settle that edge before spawning/commanding the measured mover.
    for _ in 0..300 {
        scenario.tick();
        let sim = scenario.sim();
        if sim.has_powered_center(owner)
            && sim
                .entities()
                .get(center)
                .is_some_and(|building| building.building_powered_center.is_counted())
        {
            return;
        }
    }
    panic!("retail Robot Control Center must become operational before movement");
}
