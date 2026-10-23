//! House508DF0 and local defeat4FC21B must reach the existing Radar656DF0
//! owner in order. Synthetic housing pixels exercise state, not retail art.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::world::{FrameEffects, Simulation, TacticalRadarBoundary, TickLane};
use std::cell::RefCell;

struct RadarBoundary {
    radar: RefCell<RadarPresentation>,
    cues: RefCell<Vec<RadarAvailabilityCue>>,
}

impl TacticalRadarBoundary for RadarBoundary {
    fn set_available(&self, _: &Simulation, _: &RuleSet, available: bool) {
        if let Some(cue) = self.radar.borrow_mut().set_has_radar(available) {
            self.cues.borrow_mut().push(cue);
        }
    }
}

/// The old app poll remains the fallback for view changes. Once the reached
/// calls update this same owner, polling its final value cannot replay a cue.
fn frame(sim: &mut Simulation, rules: &RuleSet, boundary: &RadarBoundary) {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        rules,
        None,
        67,
        TickLane::Ordinary,
        FrameEffects::empty().with_tactical_radar(boundary),
    )
    .expect("ordinary stock House frame");
    let available = crate::sim::radar::has_radar_for_owner(sim, rules, "Americans");
    boundary.set_available(sim, rules, available);
}

#[test]
fn retail_local_defeat_delivers_radar_off_then_on_in_one_house_frame() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    assert!(rules.object("GAAIRC").unwrap().radar);
    assert!(rules.object("GAPOWR").unwrap().power > 0);
    for losing_house in [0, 1] {
        let mut sim = Simulation::with_seed(0x508D_F0);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = true;
        let mut owners = Vec::new();
        for (country, human) in [("Americans", true), ("British", false)] {
            let owner = sim.intern(country);
            let side = rules.country_side_index(country).unwrap().0;
            let mut house = HouseState::new(owner, side, Some(owner), human, 50_000, 10);
            house.project_country_mults(&rules, sim.interner());
            sim.houses_mut_for_test().register(house);
            owners.push(owner);
        }
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owners[0]));
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let buildings: Vec<Vec<u64>> = [("Americans", 4), ("British", 20)]
            .into_iter()
            .map(|(country, y)| {
                [("GACNST", 4), ("GAPOWR", 11), ("GAAIRC", 18)]
                    .into_iter()
                    .map(|(kind, x)| sim.spawn_object(kind, country, x, y, 0, &rules).unwrap())
                    .collect()
            })
            .collect();
        let boundary = RadarBoundary {
            radar: RefCell::new(RadarPresentation::new(vec![vec![0; 4]; 33], 1, 1).unwrap()),
            cues: RefCell::new(Vec::new()),
        };
        frame(&mut sim, &rules, &boundary);
        frame(&mut sim, &rules, &boundary);
        assert!(crate::sim::radar::has_radar_for_owner(
            &sim,
            &rules,
            "Americans"
        ));
        // Settle the real housing through its normal draw/timer owner.
        for draw in 0..=32 {
            boundary.radar.borrow_mut().advance_draw(draw * 64);
        }
        assert_eq!(boundary.radar.borrow().phase(), RadarAnimPhase::Online);
        boundary.cues.borrow_mut().clear();
        // House Tags run after this House's defeat gate. Condition 9 (no
        // units) registers only with the House, so its action destroys the
        // base at frame 8, then ordinary tail cleanup retires its counts.
        // The previous radar assessment remains on until frame 9.
        let country = ["Americans", "British"][losing_house];
        let selector = rules.country_index(country).unwrap().0;
        let map = crate::map::map_file::MapFile::from_bytes(
            format!(
                "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
                 [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
                 [Basic]\nNewINIFormat=4\n\
                 [Triggers]\nREMOVE={country},<none>,Remove base,0,1,1,1,0\n\
                 [Events]\nREMOVE=1,9,0,{selector}\n\
                 [Actions]\nREMOVE=1,119,0,{selector},0,0,0,0,A\n\
                 [Tags]\nREMOVETAG=0,Remove base,REMOVE\n"
            )
            .as_bytes(),
        )
        .unwrap();
        sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
        sim.classify_map_triggers(&rules);
        while sim.session().binary_frame() <= 8 {
            frame(&mut sim, &rules, &boundary);
        }
        assert!(!sim.houses()[&owners[losing_house]].is_defeated());
        assert!(
            buildings[losing_house]
                .iter()
                .all(|&id| sim.entities().get(id).is_none())
        );
        assert!(crate::sim::radar::has_radar_for_owner(
            &sim,
            &rules,
            "Americans"
        ));
        boundary.cues.borrow_mut().clear();
        frame(&mut sim, &rules, &boundary);
        assert!(sim.houses()[&owners[losing_house]].is_defeated());
        let expected = if losing_house == 0 {
            vec![RadarAvailabilityCue::Off, RadarAvailabilityCue::On]
        } else {
            Vec::new()
        };
        assert_eq!(
            *boundary.cues.borrow(),
            expected,
            "both local transitions must precede final-state reconciliation"
        );
    }
}
