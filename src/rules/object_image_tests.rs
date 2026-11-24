use crate::rules::ini_parser::IniFile;

#[test]
fn retail_snow_seal_uses_the_retained_arctic_image() {
    let Some(snow) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str("[Map]\nTheater=SNOW\n"),
    ) else {
        return;
    };
    // ObjectTypeClass::LoadSHP5F9070..5F9119, reached by the stock GHOST
    // Rules reader after Full_Init687649 publishes the incoming theater.
    assert_eq!(snow.rules.object("GHOST").unwrap().image, "SEALA");
    assert_eq!(
        snow.rules
            .object("GHOST")
            .unwrap()
            .shp_image_loads()
            .first()
            .unwrap()
            .image,
        "SEALA"
    );
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let arctic = assets.get_ref("SEALA.SHP").unwrap();
    let ordinary = assets.get_ref("SEAL.SHP").unwrap();
    assert_ne!(
        arctic, ordinary,
        "the selected stock art is a different physical image"
    );
    let temperate = crate::rules::retail_ini_fixture::retail_battle_rules().unwrap();
    assert_eq!(temperate.rules.object("GHOST").unwrap().image, "SEAL");
}

#[test]
fn ares_alternate_theater_art_needs_the_theaters_art_section() {
    let virus_on = |theater: &str| {
        let patch = IniFile::from_str(&format!(
            "[Map]\nTheater={theater}\n[VIRUS]\nAlternateTheaterArt=yes\n"
        ));
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
            .map(|loaded| loaded.rules.object("VIRUS").unwrap().image.clone())
    };
    let Some(stock_desert) = virus_on("DESERT") else {
        return;
    };
    assert_eq!(stock_desert, "VIRUS", "the stock way never reads the key");
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    // Ares Load2DArt (hook 0x005F9070, 10041040): retail art holds [VIRUSD]
    // but no [VIRUST].
    assert_eq!(virus_on("DESERT").unwrap(), "VIRUSD");
    assert_eq!(virus_on("TEMPERATE").unwrap(), "VIRUS");
}
