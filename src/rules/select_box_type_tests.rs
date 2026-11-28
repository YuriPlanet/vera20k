//! `[SelectBoxTypes]` catalog and `[AudioVisual]` default reads
//! (`New/Type/SelectBoxTypeClass.cpp:9-34`, `Ext/Rules/Body.cpp:333-334`).
use super::{SelectBoxGlobals, SelectBoxTypeCatalog};
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_shields::AffectedHouse;

fn catalog(text: &str) -> SelectBoxTypeCatalog {
    SelectBoxTypeCatalog::from_ini(&IniFile::from_str(text))
}

#[test]
fn an_entry_without_keys_keeps_the_constructor_defaults() {
    let catalog = catalog("[SelectBoxTypes]\n0=BOX\n[BOX]\n");
    let entry = catalog.entry("BOX").expect("listed entry");
    assert_eq!(entry.name, "BOX");
    assert_eq!(entry.shape, None);
    assert_eq!(entry.palette, None);
    assert_eq!(entry.frames, None);
    assert_eq!(entry.offset, [0, 0]);
    assert_eq!(entry.translucency, 0);
    assert_eq!(entry.visible_to_houses, AffectedHouse::ALL);
    assert!(entry.visible_to_houses_observer);
    assert!(entry.draw_above_techno);
    assert_eq!(entry.ground_shape, None);
    assert_eq!(entry.ground_frames, None);
    assert_eq!(entry.ground_offset, [0, 0]);
    assert!(entry.ground_always_draw);
    assert!(!entry.ground_line);
    assert_eq!(entry.ground_line_color.base, [0, 255, 0]);
    assert_eq!(entry.ground_line_color.condition_yellow, None);
    assert_eq!(entry.ground_line_color.condition_red, None);
    assert!(!entry.ground_line_dashed);
    assert_eq!(entry.palette_name(), "palette.pal");
    assert_eq!(entry.ground_palette_name(), "palette.pal");
}

#[test]
fn the_sixteen_keys_read_in_native_order() {
    let catalog = catalog(
        "[SelectBoxTypes]\n0=BOX\n[BOX]\n\
         Shape=BOXSHPE\nPalette=custom.pal\nFrames=3,4,5\nOffset=6,-7\n\
         Translucency=50\nVisibleToHouses=owner,allies\n\
         VisibleToHouses.Observer=no\nDrawAboveTechno=no\n\
         GroundShape=GROUND\nGroundPalette=ground.pal\nGroundFrames=8,9,10\n\
         GroundOffset=-11,12\nGround.AlwaysDraw=no\nGroundLine=yes\n\
         GroundLineColor=1,2,3\nGroundLineColor.ConditionYellow=4,5,6\n\
         GroundLineColor.ConditionRed=7,8,9\nGroundLine.Dashed=yes\n",
    );
    let entry = catalog.entry("BOX").expect("listed entry");
    assert_eq!(entry.shape.as_deref(), Some("BOXSHPE.shp"));
    assert_eq!(entry.palette.as_deref(), Some("custom.pal"));
    assert_eq!(entry.frames, Some([3, 4, 5]));
    assert_eq!(entry.offset, [6, -7]);
    assert_eq!(entry.translucency, 50);
    assert_eq!(entry.visible_to_houses, AffectedHouse::TEAM);
    assert!(!entry.visible_to_houses_observer);
    assert!(!entry.draw_above_techno);
    assert_eq!(entry.ground_shape.as_deref(), Some("GROUND.shp"));
    assert_eq!(entry.ground_palette.as_deref(), Some("ground.pal"));
    assert_eq!(entry.ground_frames, Some([8, 9, 10]));
    assert_eq!(entry.ground_offset, [-11, 12]);
    assert!(!entry.ground_always_draw);
    assert!(entry.ground_line);
    assert_eq!(entry.ground_line_color.base, [1, 2, 3]);
    assert_eq!(entry.ground_line_color.condition_yellow, Some([4, 5, 6]));
    assert_eq!(entry.ground_line_color.condition_red, Some([7, 8, 9]));
    assert!(entry.ground_line_dashed);
    assert_eq!(entry.palette_name(), "custom.pal");
}

#[test]
fn the_shp_reader_appends_only_a_missing_suffix() {
    let catalog = catalog("[SelectBoxTypes]\n0=A\n1=B\n[A]\nShape=already.SHP\n[B]\nShape=plain\n");
    assert_eq!(
        catalog.entry("A").unwrap().shape.as_deref(),
        Some("already.SHP")
    );
    assert_eq!(
        catalog.entry("B").unwrap().shape.as_deref(),
        Some("plain.shp")
    );
}

#[test]
fn the_list_appends_each_pass_and_lookups_ignore_case() {
    let mut catalog = catalog("[SelectBoxTypes]\n0=BOX\n[BOX]\nShape=A\n");
    assert_eq!(catalog.index_of("box"), Some(0));
    catalog.apply_pass(&IniFile::from_str(
        "[SelectBoxTypes]\n0=BOX\n1=SECOND\n[SECOND]\nShape=B\n",
    ));
    assert_eq!(catalog.types().len(), 2);
    assert_eq!(
        catalog.entry("SECOND").unwrap().shape.as_deref(),
        Some("B.shp")
    );
    // The retained first entry re-reads its own section each pass.
    assert_eq!(
        catalog.entry("BOX").unwrap().shape.as_deref(),
        Some("A.shp")
    );
}

#[test]
fn an_empty_shape_or_palette_retains_the_previous_pointer() {
    let mut catalog = catalog("[SelectBoxTypes]\n0=BOX\n[BOX]\nShape=A\nPalette=one.pal\n");
    catalog.apply_pass(&IniFile::from_str(
        "[BOX]\nShape=\nPalette=\nGroundShape=\n",
    ));
    let entry = catalog.entry("BOX").expect("retained entry");
    assert_eq!(entry.shape.as_deref(), Some("A.shp"));
    assert_eq!(entry.palette.as_deref(), Some("one.pal"));
}

#[test]
fn the_audiovisual_defaults_resolve_against_the_catalog() {
    let types = catalog("[SelectBoxTypes]\n0=BOX\n1=UNIT\n[BOX]\n[UNIT]\n");
    let globals = SelectBoxGlobals::from_ini(
        &IniFile::from_str(
            "[AudioVisual]\nDefaultInfantrySelectBox=BOX\nDefaultUnitSelectBox=NOPE\n",
        ),
        &types,
    );
    assert_eq!(globals.default_infantry.as_deref(), Some("BOX"));
    assert_eq!(
        globals.default_unit, None,
        "an unresolved name keeps the null default"
    );
    let absent = SelectBoxGlobals::from_ini(&IniFile::from_str("[General]\n"), &types);
    assert_eq!(absent, SelectBoxGlobals::default());
}
