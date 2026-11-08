use super::*;
use crate::app::presentation::interpolation::RenderInterpolation;
use crate::rules::ini_parser::IniFile;
use crate::sim::world::Simulation;

fn fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=LAMP\n1=VISIBLE\n\
         [LAMP]\nStrength=6000\nInvisibleInGame=yes\nSelectable=no\n\
         [VISIBLE]\nStrength=100\nSelectable=no\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    let player = sim.intern("Player");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(player));
    (sim, rules)
}

fn view<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    interpolation: &'a RenderInterpolation,
) -> PresentedWorld<'a> {
    PresentedWorld {
        sim,
        rules: Some(rules),
        interpolation,
        translucent_viewer: None,
    }
}

/// `AppendVisibleBuildingSelectables @ 0x006D9CE0` never registers an
/// InvisibleInGame building (0x006D9D5E..0x006D9D6C): a click on a lamp's
/// anchor finds the object sharing it, and a band holding only the lamp has
/// caught nothing.
#[test]
fn an_invisible_lamp_is_never_registered_for_the_click_or_the_band() {
    let (mut sim, rules) = fixture();
    let lamp = sim
        .spawn_object("LAMP", "Neutral", 10, 10, 0, &rules)
        .unwrap();
    let beside = sim
        .spawn_object("VISIBLE", "Neutral", 10, 10, 0, &rules)
        .unwrap();
    let interpolation = RenderInterpolation::new(false);
    let source = view(&sim, &rules, &interpolation);
    let (x, y) = source.screen_position(sim.entities().get(lamp).unwrap());
    assert!(rules.object("LAMP").unwrap().invisible);
    assert_eq!(
        pick_entity_at_point(source, &[lamp, beside], None, None, x, y, None, None),
        Some(beside)
    );
    let band = |order: &[u64]| {
        band_rect_contains_drawn_object(
            source,
            order,
            None,
            None,
            x - 1.,
            y - 1.,
            x + 1.,
            y + 1.,
            Some(sim.interner()),
        )
    };
    assert!(!band(&[lamp]));
    assert!(band(&[lamp, beside]));
}

#[test]
fn own_invisible_object_and_nonselectable_visible_object_still_answer_hover() {
    let (mut sim, rules) = fixture();
    let own = sim
        .spawn_object("LAMP", "Player", 10, 10, 0, &rules)
        .unwrap();
    let visible = sim
        .spawn_object("VISIBLE", "Neutral", 20, 20, 0, &rules)
        .unwrap();
    let interpolation = RenderInterpolation::new(false);
    let source = view(&sim, &rules, &interpolation);
    assert!(
        sim.entities()
            .get(own)
            .unwrap()
            .discovery
            .owned_by_current_house()
    );
    for id in [own, visible] {
        let (x, y) = source.screen_position(sim.entities().get(id).unwrap());
        assert_eq!(
            hover_target_at_point(source, x, y, "Player", true)
                .unwrap()
                .stable_id,
            id
        );
    }
}

#[test]
fn invisible_cell_fallback_is_rejected_after_the_ground_list_head_is_chosen() {
    let (mut sim, rules) = fixture();
    let lamp = sim
        .spawn_object("LAMP", "Neutral", 10, 10, 0, &rules)
        .unwrap();
    let interpolation = RenderInterpolation::new(false);
    let source = view(&sim, &rules, &interpolation);
    let (x, y) = source.screen_position(sim.entities().get(lamp).unwrap());
    assert_eq!(
        sim.first_ground_cell_object((10, 10)),
        Some(CellObjectMember::Entity(lamp))
    );
    assert_eq!(
        pick_entity_at_point(&sim, &[], None, None, x, y, None, None),
        Some(lamp)
    );
    assert_eq!(
        pick_entity_at_point(source, &[], None, None, x, y, None, None),
        None
    );
}
