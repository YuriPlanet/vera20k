//! Retail Building448260 capture -> accepted Event4C6ED2 -> paid450630.
//! Rules/ART/assets are physical; the flat map and House/clock/health are priors.
use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::command::{CommandEnvelope, CommandRecord};
use crate::sim::estimated_health::EstimatedHealth;
use crate::sim::house_state::HouseState;
use crate::sim::runtime::{SimResources, SimRuntime};

#[test]
fn accepted_repair_after_capture_charges_the_current_owner_on_its_next_visit() {
    let Some(mut retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let mut roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    roots.extend(
        retail
            .rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.clone()),
    );
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let (mut sim, _, _) = entry_test_fixture::fixture();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    terrain.set_land_speed_rules_for_test(&rules.terrain_rules);
    let clear = rules.terrain_rules.semantics_for_land_type(0).unwrap();
    for cell in terrain.cells_mut_for_test() {
        cell.speed_costs = clear.speed_costs;
        cell.base_speed_costs = clear.speed_costs;
    }
    let old = sim.intern("Americans");
    let new = sim.intern("Russians");
    for owner in [old, new] {
        let mut house = HouseState::new(owner, 0, None, true, 5000, 10);
        house.set_current_iq_for_test(0);
        sim.houses.register(house);
    }
    sim.session.set_current_house_for_test(Some(new));
    let building = sim
        .spawn_object_with_overlay_registry("GAPOWR", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    // Keep both Houses alive during the ordinary post-Logic command visits.
    sim.spawn_object_with_overlay_registry("MTNK", "Americans", 17, 14, 0, &rules, &registry)
        .unwrap();
    sim.spawn_object_with_overlay_registry("HTNK", "Russians", 17, 16, 0, &rules, &registry)
        .unwrap();
    let plant = rules.object("GAPOWR").unwrap();
    assert_eq!((plant.strength, plant.cost), (750, 800));
    let entity = sim.substrate.entities.get_mut(building).unwrap();
    entity.health.set_for_test(374);
    entity.estimated_health = EstimatedHealth::from_raw(611);
    entity.repairing.set_for_test(true);
    sim.session.set_binary_frame_for_test(195);
    // Independent native event bytes; this remains compilable without RepairRecord.
    let mut payload = [0; 5];
    payload[..4].copy_from_slice(&i32::try_from(building).unwrap().to_le_bytes());
    payload[4] = 0x34;
    let record = CommandRecord::encode(0x15, 0, 195, &payload).unwrap();
    assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, u64::MAX)));
    sim.change_owner_with_rules(
        building,
        new,
        &rules,
        Some(&registry),
        FrameEffects::default(),
    );
    let captured = sim.substrate.entities.get(building).unwrap();
    assert_eq!(captured.owner(), new);
    assert!(!captured.repairing.is_on());
    assert_eq!(
        (captured.health.current(), captured.estimated_health.get()),
        (374, 611)
    );
    let terrain_template = sim.resolved_terrain.clone();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            overlay_registry: registry,
            terrain_template,
        },
    };
    runtime
        .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(
        runtime
            .simulation
            .substrate
            .entities
            .get(building)
            .unwrap()
            .repairing
            .is_on(),
        "the old sender's accepted event survives capture"
    );
    runtime
        .advance_frame(&[], 67, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    let sim = &runtime.simulation;
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current(),
        382
    );
    assert_eq!(sim.houses[&old].economy().credits(), 5000);
    assert_eq!(sim.houses[&new].economy().credits(), 4999);
}
