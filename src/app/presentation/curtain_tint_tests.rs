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

fn oracle() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap()
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
    entity
}

fn rules(force_color: i32) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "{RETAIL_COLOR_ADD}[AudioVisual]\nForceShieldColor={force_color}\n"
    ));
    RuleSet::from_ini(&ini).unwrap()
}

/// Each `drawshp_curtain_arm` row (TechnoClass::DrawSHP `0x0070631F..
/// 0x00706389` run natively) for a building and a unit with no
/// airstrike: the intensity DrawSHP hands the blit is the
/// brightness [`techno_effect_light`] gives the draw at the row's frame as the
/// committed `binary_frame`, and the compatibility tint takes its ratio. The
/// other rows are its residuals.
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
        if !row["airstrike"].is_null() {
            continue;
        }
        let intensity = int(&row["intensity"]);
        sim.session.binary_frame = int(&row["frame"]) as u32;
        let (tint, light) = techno_effect_light(
            &curtained(row),
            TINT,
            PaletteLight::color_scheme([1000; 3], intensity),
            &sim,
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
    assert_eq!(compared, 88);
}

/// Each `building_anim_light` row (BuildingClass::UpdateAnimation
/// `0x00450A47..0x00450A77` with 0x456FB0 and the slot loop run natively)
/// with no airstrike: the ShouldUseCellDrawer anim's
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
        if !row["airstrike"].is_null() {
            continue;
        }
        sim.session.binary_frame = int(&row["frame"]) as u32 + 1;
        let (_, light) = building_anim_light(
            &curtained(row),
            TINT,
            PaletteLight::color_scheme([1000; 3], int(&row["intensity"])),
            &sim,
        );
        assert_eq!(light.brightness(), int(&row["anim_light"][0]), "{row}");
        compared += 1;
    }
    assert_eq!(compared, 46);
}

/// Each `building_colour_word` row (BuildingClass_DrawBody
/// `0x0043D386..0x0043D544` and BuildingClass::Draw `0x0043DC1C..0x0043DDF1`
/// run natively) in the active RGB565 format with no airstrike: both blocks
/// give the same low 16 bits, which are [`building_colour_word`]'s.
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
        if !row["airstrike"].is_null() || int(&row["pixel_format"]) != 2 {
            continue;
        }
        sim.session.binary_frame = int(&row["frame"]) as u32;
        let word = building_colour_word(
            &shielded(row),
            &sim,
            &rules(int(&row["force_color"])),
            || row["shrouded"].as_bool().unwrap(),
        );
        assert_eq!(u64::from(word), native, "{row}");
        compared += 1;
    }
    assert_eq!(compared, 25);
}

/// Each `anim_colour_word` row (AnimClass::DrawIt `0x004233EE..0x00423630`
/// run natively, the cell's objects walked by `0x0047C520`) with no
/// airstrike: a slot anim over a building, first among the cell's objects or
/// behind another, takes that building's [`building_colour_word`]; an anim
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
        if !row["airstrike"].is_null() {
            continue;
        }
        assert_eq!(row["word"], row["stored"], "{row}");
        let native = row["word"].as_u64().unwrap() & 0xFFFF;
        if !row["slot_anim"].as_bool().unwrap() || row["occupant"] == "none" {
            assert_eq!(native, 0, "{row}");
        } else {
            sim.session.binary_frame = int(&row["frame"]) as u32;
            let word = building_colour_word(
                &shielded(row),
                &sim,
                &rules(int(&row["force_color"])),
                || row["shrouded"].as_bool().unwrap(),
            );
            assert_eq!(u64::from(word), native, "{row}");
        }
        compared += 1;
    }
    assert_eq!(compared, 11);
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
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(rules.general.force_shield_color, 6);
    assert_eq!(rules.color_add.slots[6].name.as_deref(), Some("HighBlue"));
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
