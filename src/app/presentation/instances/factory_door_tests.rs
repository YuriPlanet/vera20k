//! A war factory's unload art (`BuildingClass::DrawIt @ 0x0043D290`): the
//! retail art keys, the art an unloading factory selects, and the under-door
//! piece. The atlas half is synthetic; real pixels come from captures.
use super::*;
use crate::render::draw_state::DrawDecision;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;
use std::collections::HashMap;

/// `LoadVisualAssets 0x0045F230` reads the four names from each retail war
/// factory's art section; no other retail type has any.
#[test]
fn retail_war_factories_read_their_unload_art() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let door = |building: &str| {
        let object = rules.object(building).unwrap();
        rules
            .art()
            .resolve_metadata_entry(building, &object.image)
            .unwrap()
            .factory_door
            .clone()
    };
    for (building, names) in [
        ("GAWEAP", ["GAWEAP_2", "GAWEAP_3", "GAWEAP_1", "GAWEAP_4"]),
        ("NAWEAP", ["NAWEAP_2", "NAWEAP_4", "NAWEAP_1", "NAWEAP_3"]),
        ("YAWEAP", ["YAWEAP_2", "YAWEAP_3", "YAWEAP_1", "YAWEAP_4"]),
    ] {
        let door = door(building);
        assert_eq!(
            [
                door.deploying.as_deref(),
                door.roof_deploying.as_deref(),
                door.under_door.as_deref(),
                door.under_roof_door.as_deref(),
            ],
            names.map(Some),
            "{building}"
        );
    }
    let mut with_door: Vec<&str> = rules
        .all_objects()
        .filter(|object| {
            rules
                .art()
                .resolve_metadata_entry(&object.id, &object.image)
                .is_some_and(|art| art.factory_door != Default::default())
        })
        .map(|object| object.id.as_str())
        .collect();
    with_door.sort_unstable();
    assert_eq!(with_door, ["GAWEAP", "NAWEAP", "YAWEAP"]);
}

/// A retail Allied war factory with `product` (placed clear of its
/// footprint) in its radio slot 0, unloading once `unload` is queued and a
/// frame has run.
fn unloading_factory(rules: &RuleSet, product: &str, unload: bool) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 60);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 10, 10, 0, rules)
        .expect("factory");
    let product = sim
        .spawn_object(product, "Americans", 20, 20, 64, rules)
        .expect("product");
    crate::sim::production::mark_war_factory_spawn_contact(
        &mut sim,
        rules,
        factory,
        product,
        crate::sim::world::FrameEffects::default(),
    );
    if unload {
        sim.queue_order_mission(factory, MissionType::Unload, rules);
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    (sim, factory)
}

fn selected_art(sim: &Simulation, rules: &RuleSet, factory: u64) -> Option<[Option<String>; 2]> {
    let entity = sim.entities().get(factory).unwrap();
    let object = rules.object("GAWEAP").unwrap();
    let art = rules
        .art()
        .resolve_metadata_entry("GAWEAP", &object.image)
        .unwrap();
    factory_unload_art(sim, rules, entity, art).map(|art| {
        assert_eq!(art.palette, ShpPaletteContext::SelectedScheme);
        [
            art.body.map(|(name, z)| format!("{name} {z}")),
            art.under_door.map(str::to_owned),
        ]
    })
}

/// In Mission Unload a ground product takes `DeployingAnim=` at z -20 and
/// `UnderDoorAnim=`; a `JumpJet=` Night Hawk takes the roof pair at z -40
/// (`0x0043D33F..0x0043D386`, `0x0043D692..0x0043D6DB`). Another mission
/// draws the ordinary body.
#[test]
fn an_unloading_factory_selects_its_door_or_roof_art() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert!(rules.object("SHAD").unwrap().jumpjet);
    let (sim, factory) = unloading_factory(rules, "MTNK", false);
    assert_eq!(selected_art(&sim, rules, factory), None);
    let (sim, factory) = unloading_factory(rules, "MTNK", true);
    assert_eq!(
        selected_art(&sim, rules, factory),
        Some([Some("GAWEAP_2 -20".into()), Some("GAWEAP_1".into())])
    );
    let (sim, factory) = unloading_factory(rules, "SHAD", true);
    assert_eq!(
        selected_art(&sim, rules, factory),
        Some([Some("GAWEAP_3 -40".into()), Some("GAWEAP_4".into())])
    );
}

fn entry(page: u8) -> ShpSpriteEntry {
    ShpSpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [8.0; 2],
        offset_x: 2.0,
        offset_y: -4.0,
        canvas_rect: [0.0, 0.0, 8.0, 8.0],
        extended: false,
        page,
        source_palette: 1,
    }
}

/// The under-door piece draws frame 1 at or below `ConditionYellow=`, else
/// 0, at `-AdjustForZ(Z)` with gradient entry 0 after the bib, and DrawSHP
/// follows it with the frame's shadow half (`0x0043D927..0x0043D9C9`).
#[test]
fn the_under_door_draws_its_damage_frame_and_shadow_after_the_bib() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let key = |frame| ShpSpriteKey {
        palette_context: ShpPaletteContext::SelectedScheme,
        type_id: "GAWEAP_1".into(),
        facing: 0,
        frame,
        house_color: HouseColorIndex(1),
    };
    // Frames 0 and 1, then their shadows 2 and 3; the page marks the frame.
    let mut atlas = SpriteAtlas::from_test_entries(HashMap::from(
        [0, 1, 2, 3].map(|frame| (key(frame), entry(frame as u8))),
    ));
    atlas.set_object_shp_frame_count_for_test("GAWEAP_1", 4);
    let object = rules.object("GAWEAP").unwrap();
    for (health, frame) in [(object.strength, 0), (object.strength / 2, 1)] {
        let (mut sim, factory) = unloading_factory(rules, "MTNK", true);
        sim.entities_mut_for_test()
            .get_mut(factory)
            .unwrap()
            .health
            .set_for_test(health);
        let mut pieces = Vec::new();
        emit_factory_under_door(
            &mut pieces,
            &atlas,
            rules,
            Some(object),
            sim.entities().get(factory).unwrap(),
            key(0),
            [100.0, 50.0],
            0,
            0.5,
            [1.0; 3],
            crate::render::palette_light::PaletteLight::default(),
            0,
            DrawState::default(),
            DrawDecision {
                visible: true,
                state: DrawState::default(),
                native_visual_state: 0,
            },
        );
        assert_eq!(pieces.len(), 2, "health {health}");
        assert!(
            pieces
                .iter()
                .all(|piece| piece.kind == BuildingPieceKind::UnderDoor)
        );
        assert_eq!(
            pieces.iter().map(|piece| piece.target).collect::<Vec<_>>(),
            [
                ObjectTexture::ShpPage(frame),
                ObjectTexture::SpriteAtlasEdit(
                    frame + 2,
                    crate::render::terrain_draw::TerrainPiece::Shadow
                ),
            ],
            "health {health}"
        );
        let body = &pieces[0].instance;
        assert_eq!(body.position, [102.0, 46.0]);
        assert_eq!(body.z_adjust, lifted_z_adjust(0, SHP_DRAW_Z_ADJUST_PX));
        assert_eq!(body.z_gradient, pack_z_gradient(ZGradient::Flat, false));
    }
}

/// An unloading factory redraws the product its tether holds, and the
/// product refuses its own Display slot (`0x0043DAA0`, `0x0073B0B0`); a
/// factory in another mission does neither.
#[test]
fn an_unloading_factory_takes_over_its_products_draw() {
    use crate::app::presentation::render::draw_plan_lowering::PlannedObjectInstance;
    use crate::render::tactical_draw_plan::{
        BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer,
    };
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for unload in [false, true] {
        let (sim, factory) = unloading_factory(rules, "MTNK", unload);
        let product = sim
            .entities()
            .get(factory)
            .unwrap()
            .radio_contacts
            .slot(0)
            .expect("the product stays in contact");
        let parent = |id| ObjectDraw {
            id,
            layer: TacticalLayer(2),
            display_order: 0,
            policy: BlitPolicy::z_read(SpriteEncoding::Voxel),
        };
        let mut planned = vec![
            PlannedObjectInstance::building(parent(factory), Vec::new()),
            PlannedObjectInstance::object(parent(product), Vec::new()),
        ];
        crate::app::presentation::instances::assign_tether_redraws(&sim, &mut planned);
        let expected: Vec<u64> = if unload { vec![product] } else { Vec::new() };
        assert_eq!(planned[0].tether_redraws, expected, "unload {unload}");
        assert_eq!(planned[1].drawn_by_tether, unload, "unload {unload}");
    }
}
