use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::world::Simulation;

/// `0x0043B150` on retail rules: an enemy tank's intent shows to the local
/// player whose powered Psychic Sensor (`NAPSIS`, `PsychicDetectionRadius=15`)
/// stands within 15 cells of its Target or, with none in range, of its
/// NavCom; never to an ally, and never without a local player.
#[test]
fn a_psychic_sensor_senses_an_enemy_target_or_destination_in_range() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    assert_eq!(rules.object("NAPSIS").unwrap().psychic_detection_radius, 15);
    let (mut sim, tank, americans) = super::psychic_sensor_watch_fixture(&rules);
    let detected = |sim: &Simulation| {
        let Some(viewer) = sim.session.current_house() else {
            return false;
        };
        let sites: Vec<_> = sim.psychic_detection_sites(viewer, &rules).collect();
        let cells = sim
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        sim.techno_intent_psychically_detected(tank, viewer, &sites, cells.as_ref())
    };
    let set = |sim: &mut Simulation, target: Option<TargetKind>, nav: Option<NavTargetRef>| {
        let entity = sim.substrate.entities.get_mut(tank).unwrap();
        entity.attack_target.set_for_test(target);
        entity.navigation.set_nav_com_for_test(nav);
    };
    let near = Some(TargetKind::Cell(18, 18));
    let far = Some(TargetKind::Cell(30, 30));
    assert!(detected(&sim));
    sim.session.set_current_house_for_test(None);
    assert!(!detected(&sim), "no local player");
    sim.session.set_current_house_for_test(Some(americans));
    set(&mut sim, far, None);
    assert!(!detected(&sim));
    set(&mut sim, far, Some(NavTargetRef::cell(19, 16)));
    assert!(detected(&sim), "a Foot's NavCom in range");
    set(&mut sim, None, Some(NavTargetRef::cell(30, 30)));
    assert!(!detected(&sim));

    set(&mut sim, near, None);
    sim.house_alliances_mut_for_test()
        .entry("AMERICANS".into())
        .or_default()
        .insert("RUSSIANS".into());
    assert!(!detected(&sim), "an ally's intent");
}
