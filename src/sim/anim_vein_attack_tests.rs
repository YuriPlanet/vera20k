//! Phobos `AnimExt::VeinAttackAI` (`Ext/Anim/Body.cpp:164-195`, hook
//! `0x4243BC`): the `IsVeins=` anim's every-other-frame damage, its
//! `ImmuneToVeins` skip and the precondition retirement.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::overlay_grid::OverlayGrid;

const VEIN_DAMAGE: i32 = 25;

fn fixture() -> (Simulation, RuleSet) {
    let ini = IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [InfantryTypes]\n0=CIV\n1=IMMUNE\n\
         [CIV]\nImage=GI\nStrength=100\nArmor=none\nSpeed=4\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [IMMUNE]\nImage=GI\nStrength=100\nArmor=none\nSpeed=4\nImmuneToVeins=yes\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [Animations]\n0=VEINS\n\
         [Warheads]\n0=VEINWH\n\
         [VEINWH]\nCellSpread=0\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nVeinholeWarhead=VEINWH\n\
         [General]\nVeinDamage={VEIN_DAMAGE}\n"
    ));
    let art = IniFile::from_str("[VEINS]\nRate=900\nEnd=4\nIsVeins=yes\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("VEINS", 4);
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 41);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn spawn_veins(sim: &mut Simulation, rules: &RuleSet, rx: u16, ry: u16) -> u64 {
    let type_id = sim.interner.intern("VEINS");
    sim.spawn_anim_at_world(
        rules,
        AnimClassSpawnDescriptor {
            delay: 1,
            ..AnimClassSpawnDescriptor::new(
                type_id,
                rx,
                ry,
                crate::util::fixed_math::SimFixed::ZERO,
                crate::util::fixed_math::SimFixed::ZERO,
                0,
            )
        },
        AnimWorldCoord {
            x: i32::from(rx) * 256 + 128,
            y: i32::from(ry) * 256 + 128,
            z: 0,
        },
    )
    .unwrap_or_else(|e| panic!("veins anim spawns: {e:?}"))
}

fn insert_occupant(sim: &mut Simulation, rules: &RuleSet, type_id: &str, rx: u16, ry: u16) -> u64 {
    let _ = rules;
    let mut entity = crate::sim::game_entity::GameEntity::test_default_of_category(
        900,
        type_id,
        "Americans",
        rx,
        ry,
        EntityCategory::Infantry,
    );
    entity.set_type_ref_for_test(sim.interner.intern(type_id));
    let id = entity.stable_id();
    sim.entities_mut_for_test().insert(entity);
    sim.substrate.occupancy.add(
        rx,
        ry,
        id,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
    );
    id
}

/// The delay countdown's `AnimClass::Start` and the first-AI guard consume
/// the first two visits before the ordinary AI body runs.
fn prime(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    visit(sim, rules, id, 3);
    visit(sim, rules, id, 4);
}

fn visit(sim: &mut Simulation, rules: &RuleSet, id: u64, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    sim.visit_anim(
        id,
        rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    )
    .unwrap();
}

/// An even frame damages the low occupant through the configured warhead;
/// an odd frame does not.
#[test]
fn veins_anim_damages_the_occupant_every_other_frame() {
    let (mut sim, rules) = fixture();
    assert_eq!(
        rules.art().anim_runtime_config("VEINS").unwrap().is_veins,
        true
    );
    assert!(rules.object("CIV").is_some(), "CIV type registered");
    assert!(rules.object("IMMUNE").is_some(), "IMMUNE type registered");
    assert_eq!(rules.general.vein_damage, VEIN_DAMAGE);
    assert!(!rules.object("CIV").unwrap().immune_to_veins);
    assert!(rules.object("IMMUNE").unwrap().immune_to_veins);
    let mut grid = OverlayGrid::new(32, 32);
    grid.place_overlay(12, 12, 126, 0x30);
    sim.install_overlay_grid_for_load(grid);
    let civ = insert_occupant(&mut sim, &rules, "CIV", 12, 12);
    let anim = spawn_veins(&mut sim, &rules, 12, 12);

    prime(&mut sim, &rules, anim);
    visit(&mut sim, &rules, anim, 5);
    assert_eq!(sim.entities().get(civ).unwrap().health.current(), 100);
    visit(&mut sim, &rules, anim, 6);
    assert_eq!(
        sim.entities().get(civ).unwrap().health.current(),
        100 - VEIN_DAMAGE
    );
    assert!(
        !sim.anim(anim).unwrap().runtime.inactive,
        "a matching weed cell keeps the anim going"
    );
}

/// `ImmuneToVeins=yes` skips the occupant, and a cell without the fully
/// grown weed overlay retires the anim.
#[test]
fn veins_anim_skips_immune_types_and_retires_off_the_weed_cell() {
    let (mut sim, rules) = fixture();
    let mut grid = OverlayGrid::new(32, 32);
    grid.place_overlay(12, 12, 126, 0x30);
    sim.install_overlay_grid_for_load(grid);
    let immune = insert_occupant(&mut sim, &rules, "IMMUNE", 12, 12);
    let anim = spawn_veins(&mut sim, &rules, 12, 12);
    prime(&mut sim, &rules, anim);
    visit(&mut sim, &rules, anim, 6);
    assert_eq!(sim.entities().get(immune).unwrap().health.current(), 100);

    // No overlay at (20,20): the precondition retires the anim.
    let bare = spawn_veins(&mut sim, &rules, 20, 20);
    prime(&mut sim, &rules, bare);
    visit(&mut sim, &rules, bare, 5);
    assert!(sim.anim(bare).unwrap().runtime.inactive);
}
