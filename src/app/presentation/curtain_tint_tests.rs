//! Native comparisons for the curtain's tint on buildings, SHP vehicles and
//! voxel aircraft, from `tools/superweapon_oracle.py`'s sections
//! `drawshp_curtain_arm`, `draw_curtain_arm`, `building_anim_light`,
//! `building_colour_word`, `anim_colour_word`, `blit_pickers` and `blitters`.

use super::*;
use crate::map::entities::EntityCategory;
use crate::render::palette_light::PaletteLight;
use crate::render::tactical_draw_plan::{BlitPolicy, SpriteEncoding};
use crate::rules::ini_parser::IniFile;
use crate::sim::game_entity::GameEntity;
use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
use crate::sim::timer::CdTimer;
use serde_json::Value;

/// Retail RULESMD.INI's `[ColorAdd]`, as the oracle's Rules hold it.
const RETAIL_COLOR_ADD: &str = "[ColorAdd]\nNone=0,0,0\nStrongRed=31,0,0\nStrongGreen=0,63,0\n\
    StrongBlue=0,0,31\nHighRed=24,0,0\nHighGreen=0,56,0\nHighBlue=0,0,24\n\
    BrightWhite=31,63,31\nLowWhite=7,7,7\nHighWhite=24,56,24\nMidWhite=14,28,14\n\
    Purple=15,0,15\nHighYellow=24,56,0\nTopYellow=16,32,0\n";
const TINT: [f32; 3] = [0.6, 0.9, 1.2];

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/superweapon_oracle.json")
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn timer(value: &Value) -> CdTimer {
    CdTimer::from_raw(int(&value[0]), int(&value[1]))
}

/// A Techno of the row's class under the row's curtain, at its tint stage.
fn curtained(row: &Value) -> GameEntity {
    let category = match row["kind"].as_str() {
        Some("unit") => EntityCategory::Unit,
        Some("aircraft") => EntityCategory::Aircraft,
        _ => EntityCategory::Structure,
    };
    let mut entity =
        GameEntity::test_default_of_category(1, "GAPOWR", "Americans", 10, 10, category);
    entity.flashing.start(int(&row["flash"]));
    entity.invulnerability = Some(InvulnerabilityState::with_tint(
        timer(&row["curtain"]),
        InvulnKind::ForceShield,
        u8::try_from(int(&row["stage"])).unwrap(),
        timer(&row["tint"]),
    ));
    if !row["airstrike"].is_null() {
        entity.airstrike.set_test_link(Some(99));
    }
    entity
}

/// A building under the row's curtain: a Force Shield when IronCurtain wrote
/// its byte 1, else an Iron Curtain (`+0x1C4` holds 0 or 1; a 2 reads as not
/// shielded, as the Iron Curtain's 0 does).
fn shielded(row: &Value) -> GameEntity {
    let mut entity = GameEntity::test_default(1, "GAPOWR", "Americans", 10, 10);
    entity.set_category_for_test(EntityCategory::Structure);
    let kind = if int(&row["shielded"]) == 1 {
        InvulnKind::ForceShield
    } else {
        InvulnKind::IronCurtain
    };
    entity.invulnerability = Some(InvulnerabilityState::new(timer(&row["curtain"]), kind));
    if !row["airstrike"].is_null() {
        entity.airstrike.set_test_link(Some(99));
    }
    entity
}

fn rules(force_color: i32) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "{RETAIL_COLOR_ADD}[AudioVisual]\nForceShieldColor={force_color}\nLaserTargetColor=4\n"
    ));
    RuleSet::from_ini(&ini).unwrap()
}

/// Each `drawshp_curtain_arm` row (TechnoClass::DrawSHP `0x0070631F..
/// 0x00706389` run natively) for a building and a unit including
/// airstrike markers: the intensity DrawSHP hands the blit is the
/// brightness [`techno_effect_light`] gives the draw at the row's frame as the
/// committed `binary_frame`, and the compatibility tint takes its ratio. All rows are compared.
#[test]
fn the_shp_draw_curtain_arm_matches_native() {
    assert_curtain_arm_matches("drawshp_curtain_arm");
}

/// The same for each `draw_curtain_arm` row (TechnoClass::Draw `0x00706776..
/// 0x007067E4`, the voxel draw's arm, run natively) for an aircraft and a
/// building: the intensity the voxel draw takes.
#[test]
fn the_voxel_draw_curtain_arm_matches_native() {
    assert_curtain_arm_matches("draw_curtain_arm");
}

fn assert_curtain_arm_matches(section: &str) {
    let oracle = oracle();
    let rows = oracle[section].as_array().unwrap();
    assert_eq!(rows.len(), 92);
    let mut sim = Simulation::new();
    let mut compared = 0;
    for row in rows {
        let intensity = int(&row["intensity"]);
        sim.session
            .set_binary_frame_for_test(int(&row["frame"]) as u32);
        let (tint, light) = techno_effect_light_at(
            &curtained(row),
            TINT,
            PaletteLight::color_scheme([1000; 3], intensity),
            sim.session.binary_frame(),
            row["airstrike"] == "self",
        );
        let out = int(&row["out_intensity"]);
        assert_eq!(light.brightness(), out, "{row}");
        assert_eq!(
            tint,
            TINT.map(|c| c * (out as f32 / intensity as f32)),
            "{row}"
        );
        compared += 1;
    }
    assert_eq!(compared, 92);
}

/// Each `building_anim_light` row (BuildingClass::UpdateAnimation
/// `0x00450A47..0x00450A77` with 0x456FB0 and the slot loop run natively)
/// including airstrike markers: the ShouldUseCellDrawer anim's
/// intensity is the brightness [`building_anim_light`] gives it when the
/// committed frame is the one after the update's, cut to 16 bits; the anim
/// whose type lacks ShouldUseCellDrawer keeps its own.
#[test]
fn the_building_anim_light_matches_native() {
    let oracle = oracle();
    let rows = oracle["building_anim_light"].as_array().unwrap();
    assert_eq!(rows.len(), 48);
    let mut sim = Simulation::new();
    let mut compared = 0;
    for row in rows {
        assert_eq!(row["drawer_set"], serde_json::json!([true, false]), "{row}");
        assert_eq!(int(&row["anim_light"][1]), -12345, "{row}");
        sim.session
            .set_binary_frame_for_test(int(&row["frame"]) as u32 + 1);
        let mut building = curtained(row);
        building.building_art.capture_tint_sample(
            int(&row["frame"]) as u32,
            building.flashing.flashed(),
            building.invulnerability.as_ref(),
            &building.airstrike_tint,
            row["airstrike"] == "self",
        );
        let (_, light) = building_anim_light(
            &building,
            TINT,
            PaletteLight::color_scheme([1000; 3], int(&row["intensity"])),
            &sim,
        );
        assert_eq!(light.brightness(), int(&row["anim_light"][0]), "{row}");
        compared += 1;
    }
    assert_eq!(compared, 48);
}

/// Each `building_colour_word` row (BuildingClass_DrawBody
/// `0x0043D386..0x0043D544` and BuildingClass::Draw `0x0043DC1C..0x0043DDF1`
/// run natively) in the active RGB565 format including airstrike markers: both blocks
/// give the same low 16 bits, which are [`shp_colour_word`]'s building arm.
#[test]
fn the_building_colour_word_matches_native() {
    let oracle = oracle();
    let rows = oracle["building_colour_word"].as_array().unwrap();
    assert_eq!(rows.len(), 35);
    let mut sim = Simulation::new();
    let mut compared = 0;
    for row in rows {
        let native = row["draw_body_word"].as_u64().unwrap() & 0xFFFF;
        assert_eq!(native, row["draw_word"].as_u64().unwrap() & 0xFFFF, "{row}");
        if int(&row["pixel_format"]) != 2 {
            continue;
        }
        sim.session
            .set_binary_frame_for_test(int(&row["frame"]) as u32);
        let word = shp_colour_word(
            &shielded(row),
            &sim,
            &rules(int(&row["force_color"])),
            || row["shrouded"].as_bool().unwrap(),
        );
        assert_eq!(u64::from(word), native, "{row}");
        compared += 1;
    }
    assert_eq!(compared, 29);
}

/// Original Infantry518F90 -> Foot41C090 -> Techno705E00 -> Shape4AED70,
/// with physical E1/GI, a selected DarkRed Convert and admitted flat cells.
/// These retained draw inputs do not replay the gas or Infantry AI lifecycle.
#[test]
fn infantry_shp_material_matches_original_gi_draws() {
    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/infantry_berserk_body.json");
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &battle.rules;
    let shared = &native["shared"];
    assert_eq!(
        rules.general.berserk_color,
        int(&shared["native_rules"]["berserk_color"])
    );
    assert_eq!(
        rules.general.extra_infantry_light,
        int(&shared["native_rules"]["extra_infantry_light"])
    );
    let object = rules.object("E1").unwrap();
    assert_eq!(
        object.image,
        shared["native_type"]["image"].as_str().unwrap()
    );
    assert_eq!(object.strength, int(&shared["native_type"]["strength"]));
    let sim = Simulation::new();
    let grid = CellLightGrid::new();
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let mut gi = GameEntity::test_default_of_category(
            1,
            "E1",
            "Computer1",
            10,
            20,
            EntityCategory::Infantry,
        );
        gi.berserk
            .set_active_for_test(row["input"]["berserk"].as_bool().unwrap());
        let light = body_palette_light(&grid, (10, 20), body_extra_light(&gi, rules, &sim, None));
        let (_, light) = techno_effect_light(&gi, [1.; 3], light, &sim);
        assert_eq!(
            light.brightness(),
            int(&row["body"]["intensity"]),
            "{}",
            row["name"]
        );
        let word = shp_colour_word(&gi, &sim, rules, || {
            row["input"]["shrouded"].as_bool().unwrap()
        });
        assert_eq!(
            u64::from(word),
            row["body"]["colour_word"].as_u64().unwrap(),
            "{}",
            row["name"]
        );
    }
}

/// A production ChaosAttack installs and refreshes the existing Infantry
/// Berserk owner. The draw word follows that owner through ordinary frames
/// after a Move ends exposure. Native evidence for the word is the GI corpus;
/// this arena is a Rust integration regression, not a native world replay.
#[test]
fn ordinary_stock_gas_refresh_and_expiry_reach_the_infantry_shp_material() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{FrameEffects, TickLane};

    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = battle.rules;
    let roots = crate::rules::effect_asset_catalog::anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
    let native =
        crate::test_fixture::json("tools/procedural_drawing_oracle/infantry_berserk_body.json");
    let active_word = native["rows"][1]["body"]["colour_word"].as_u64().unwrap() as u16;
    let mut histories = Vec::new();
    for move_step in [40, 120] {
        let mut sim = Simulation::with_seed(31);
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        for (name, human) in [("YuriCountry", true), ("Russians", false)] {
            let id = sim.interner.intern(name);
            let mut house = HouseState::new(id, 0, None, human, 0, 10);
            house.current_iq = if human {
                0
            } else {
                rules.general.max_iq_levels
            };
            sim.houses.register(house);
        }
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.resolve_rule_animation_lists(&rules);
        let drone = sim
            .spawn_object("CAOS", "YuriCountry", 10, 10, 0, &rules)
            .unwrap();
        let gi = sim
            .spawn_object("E1", "Russians", 12, 10, 0, &rules)
            .unwrap();
        let owner = sim.entities().get(drone).unwrap().owner();
        let enemy = sim.entities().get(gi).unwrap().owner();
        sim.fog.mark_visible_for_owner(owner, 12, 10);
        sim.fog.mark_visible_for_owner(enemy, 10, 10);
        sim.session.set_current_house_for_test(Some(owner));
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::Attack {
                attacker_id: drone,
                target_id: gi,
            },
        ));
        let mut gas_shots = Vec::new();
        let mut transitions = Vec::new();
        let mut active = false;
        assert_eq!(
            shp_colour_word(sim.entities().get(gi).unwrap(), &sim, &rules, || false),
            0
        );
        for step in 1..=800 {
            if step == move_step {
                sim.queue_command(CommandEnvelope::new(
                    owner,
                    step,
                    Command::Move {
                        entity_id: drone,
                        target_rx: 2,
                        target_ry: 2,
                    },
                ));
            }
            let commands = sim.take_due_commands();
            let output = sim
                .advance_app_frame(
                    &commands,
                    Some(&rules),
                    None,
                    33,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .expect("ordinary gas integration frame");
            assert!(output.tick.frame_committed);
            for shot in output
                .fire_events
                .iter()
                .filter(|shot| shot.attacker_id == drone)
            {
                assert_eq!(sim.interner.resolve(shot.weapon_id), "ChaosAttack");
                gas_shots.push(step);
            }
            let entity = sim.entities().get(gi).expect("gas does not kill the GI");
            assert_eq!(
                entity.health.current(),
                rules.object("E1").unwrap().strength
            );
            let now = entity.berserk.is_active();
            if now != active {
                transitions.push((step, now));
                active = now;
            }
            assert_eq!(
                shp_colour_word(entity, &sim, &rules, || false),
                if now { active_word } else { 0 },
                "Move{move_step}, step{step}: retained state reaches the draw material",
            );
        }
        assert_eq!(
            transitions.len(),
            2,
            "Move{move_step}: {transitions:?}, shots{gas_shots:?}"
        );
        assert!(transitions[0].1 && !transitions[1].1);
        histories.push((gas_shots, transitions[1].0));
    }
    assert_eq!(histories[0].0.len(), 1, "single exposure: {histories:?}");
    assert!(histories[1].0.len() > 1, "refresh exposure: {histories:?}");
    assert!(
        histories[1].1 > histories[0].1,
        "refresh postpones clearing: {histories:?}"
    );
}

/// Each `anim_colour_word` row (AnimClass::DrawIt `0x004233EE..0x00423630`
/// run natively, the cell's objects walked by `0x0047C520`) including
/// airstrike markers: a slot anim over a building, first among the cell's objects or
/// behind another, takes that building's [`shp_colour_word`]; an anim
/// that is not a slot anim, or whose cell holds no building, takes none. The
/// rows stand in for VERA's cell lookup (`instances/shp.rs`
/// `anim_colour_word`), which is read, not replayed.
#[test]
fn the_anim_colour_word_matches_native() {
    let oracle = oracle();
    let rows = oracle["anim_colour_word"].as_array().unwrap();
    assert_eq!(rows.len(), 13);
    let mut sim = Simulation::new();
    let mut compared = 0;
    for row in rows {
        assert_eq!(row["word"], row["stored"], "{row}");
        let native = row["word"].as_u64().unwrap() & 0xFFFF;
        if !row["slot_anim"].as_bool().unwrap() || row["occupant"] == "none" {
            assert_eq!(native, 0, "{row}");
        } else {
            sim.session
                .set_binary_frame_for_test(int(&row["frame"]) as u32);
            let word = shp_colour_word(
                &shielded(row),
                &sim,
                &rules(int(&row["force_color"])),
                || row["shrouded"].as_bool().unwrap(),
            );
            assert_eq!(u64::from(word), native, "{row}");
        }
        compared += 1;
    }
    assert_eq!(compared, 13);
}

/// The blitter each draw's flags pick (`blit_pickers`: ConvertClass
/// `0x00490B90`, compressed frames `0x00490E50`) and whether its tinted copy
/// ORs the colour word into the pixels its plain copy draws (`blitters`,
/// each run natively) are what [`BlitPolicy::ors_colour_word`] answers for
/// VERA's policy of that draw: DrawBody's pieces (flags 0x6E00, `opaque`),
/// AnimClass::DrawIt and other DrawSHP callers (0x2800, 0x2E00, `z_read`),
/// and the voxel cache blit, which always picks a compressed-frame blitter.
/// A hole and a pixel behind the Z line stay untouched, and only the low 16
/// bits of the word reach a pixel; [`PaletteLight::rgb565`] ORs a light's
/// word the same way.
#[test]
fn the_blitters_or_the_colour_word_as_the_policy_answers() {
    let oracle = oracle();
    let mut ors = std::collections::BTreeMap::new();
    for row in oracle["blitters"].as_array().unwrap() {
        let word = (row["word"].as_u64().unwrap() & 0xFFFF) as u16;
        let pixels = |name: &str| -> Vec<u16> {
            row[name]
                .as_array()
                .unwrap()
                .iter()
                .map(|pixel| pixel.as_u64().unwrap() as u16)
                .collect()
        };
        let (plain, tinted) = (pixels("plain"), pixels("tinted"));
        assert_eq!(
            plain.iter().filter(|&&pixel| pixel == 0xAAAA).count(),
            2,
            "{row}"
        );
        let ored = plain
            .iter()
            .zip(&tinted)
            .all(|(&p, &t)| t == if p == 0xAAAA { p } else { p | word });
        if word == 0 {
            assert_eq!(plain, tinted, "{row}");
            continue;
        }
        assert!(ored || plain == tinted, "{row}");
        let field = row["field"].as_u64().unwrap();
        assert_eq!(*ors.entry(field).or_insert(ored), ored, "{row}");
        let light = PaletteLight::color_scheme([1000; 3], 1000);
        for index in [0u8, 5, 200] {
            let pixel = light.rgb565([90, 170, 250], index, 64);
            let with_word = light
                .with_colour_word(word)
                .rgb565([90, 170, 250], index, 64);
            assert_eq!(with_word, if index == 0 { pixel } else { pixel | word });
        }
    }
    let policy = |flags: u64| match flags {
        0x6E00 => BlitPolicy::opaque(SpriteEncoding::Plain),
        0x2E00 | 0x2800 => BlitPolicy::z_read(SpriteEncoding::Plain),
        other => panic!("flags {other:#x}"),
    };
    for row in oracle["blit_pickers"].as_array().unwrap() {
        let flags = row[0].as_u64().unwrap();
        let (uncompressed, compressed) = (row[1].as_u64().unwrap(), row[2].as_u64().unwrap());
        assert_eq!(
            policy(flags).ors_colour_word(false),
            ors[&uncompressed],
            "{row}"
        );
        assert_eq!(
            policy(flags).ors_colour_word(true),
            ors[&compressed],
            "{row}"
        );
        if flags == 0x2800 {
            let voxel = BlitPolicy::z_read(SpriteEncoding::Voxel);
            assert_eq!(voxel.ors_colour_word(false), ors[&compressed], "{row}");
        }
    }
    assert_eq!(ors.len(), 4);
}

/// Retail `[AudioVisual] ForceShieldColor=6` reads through the production
/// rules reader, and its `[ColorAdd]` entry, HighBlue, gives the word the
/// oracle's DrawBody computes from retail's table (`building_colour_word`'s
/// first row: 0x0018 in the low 16 bits).
#[test]
fn retail_force_shield_colour_is_high_blue() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(rules.general.force_shield_color, 6);
    assert_eq!(
        rules.color_add.entries()[6].name.as_deref(),
        Some("HighBlue")
    );
    let oracle = oracle();
    let native = oracle["building_colour_word"][0]["draw_body_word"]
        .as_u64()
        .unwrap();
    assert_eq!(
        u64::from(
            rules
                .color_add
                .rgb565_word(rules.general.force_shield_color)
        ),
        native & 0xFFFF
    );
    assert_eq!(native & 0xFFFF, 0x0018);
}

/// UpdateAnimation reads before Techno updates either tint. Crossing the
/// random stage-three boundary must retain that pre-update sample, including
/// through serialization, instead of resampling the new stage at an old frame.
#[test]
fn building_animation_retains_the_pre_techno_airstrike_sample() {
    use crate::sim::rng::SimRng;
    let mut building = GameEntity::test_default_of_category(
        1,
        "GAPOWR",
        "Americans",
        10,
        10,
        EntityCategory::Structure,
    );
    let mut rng = SimRng::new(7);
    building.airstrike_tint.start(1000, 100000);
    for frame in 1000..1010 {
        building.airstrike_tint.update_tint(frame, true, &mut rng);
    }
    building
        .building_art
        .capture_tint_sample(1010, false, None, &building.airstrike_tint, true);
    building.airstrike_tint.update_tint(1010, true, &mut rng);
    let mut sim = Simulation::new();
    sim.session.set_binary_frame_for_test(1011);
    let input = PaletteLight::color_scheme([1000; 3], 500);
    let (_, retained) = building_anim_light(&building, TINT, input, &sim);
    let (_, body) = techno_effect_light_at(&building, TINT, input, 1010, true);
    assert_eq!(retained.brightness(), 1000);
    let native: Value =
        serde_json::from_str(crate::test_fixture::text("tools/airstrike_tint.json")).unwrap();
    let row = native["intensities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[0] == 0 && r[2] == 3 && r[3] == 24)
        .unwrap();
    assert_eq!(body.brightness(), int(&row[4][5]));
    assert_ne!(retained.brightness(), body.brightness());
    let serialized = serde_json::to_value(&building.building_art).unwrap();
    building.building_art = serde_json::from_value(serialized).unwrap();
    assert_eq!(
        building_anim_light(&building, TINT, input, &sim)
            .1
            .brightness(),
        retained.brightness()
    );
}
