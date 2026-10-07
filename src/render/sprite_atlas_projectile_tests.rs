use super::*;
use crate::assets::asset_manager::MediaArchiveMode;
use crate::rules::{ini_parser::IniFile, retail_ini_fixture::retail_ini};
use serde_json::Value;

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/bridge_render_inputs.json",
    ))
    .unwrap()
}

fn rules(rules: &str, art: &str) -> RuleSet {
    let art = IniFile::from_str(art);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(rules), &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules
}

fn simple_rules(art: &str) -> RuleSet {
    rules(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nPrimary=105mm\n[105mm]\nProjectile=Cannon\n[Cannon]\nImage=120MM\n",
        art,
    )
}

#[test]
fn retail_projectile_reader_matches_original_selected_cannon_inputs() {
    let Some(ini) = retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art_ini) = retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    let corpus = native();
    let expected = &corpus["layers"][0]["state"];
    let projectile = rules.projectile("Cannon").unwrap();
    for (key, value) in [
        ("shadow", projectile.shadow),
        ("inviso", projectile.inviso),
        ("arcing", projectile.arcing),
        ("inverse_rotates", projectile.rotates),
        ("anim_palette", projectile.anim_palette),
        ("firers_palette", projectile.firers_palette),
        ("flat", projectile.flat),
        ("voxel", projectile.voxel),
        ("aa", projectile.aa),
        ("ag", projectile.ag),
    ] {
        assert_eq!(Value::Bool(value), expected[key], "Cannon {key}");
    }
    assert_eq!(projectile.image.as_deref(), expected["image"].as_str());
    for (key, value) in [
        ("anim_low", projectile.anim_low),
        ("anim_high", projectile.anim_high),
        ("anim_rate", projectile.anim_rate),
    ] {
        assert_eq!(i64::from(value), expected[key].as_i64().unwrap(), "{key}");
    }
    let selected = &corpus["ordinary_selection"]["rows"][0];
    assert_eq!(
        rules.object("MTNK").unwrap().primary.as_deref(),
        selected["primary"].as_str()
    );
    let weapon = rules.weapon("105mm").unwrap();
    assert_eq!(
        weapon.projectile.as_deref(),
        selected["projectile"].as_str()
    );
    // Body ReadSpeed474810 stores 102 before the later7729F0 postpass;
    // WeaponType currently retains authored
    // Speed=40. GetSpeed773070 ignores that field for the selected ROT=0 Cannon.
    // Do not conceal the missing guided/no-projectile GetSpeed and MaxSpeed
    // conversion with a test-only adapter. See bridge-projectile-render.md's
    // required Speed reader/consumer residual and the joined launch comparison.
    assert_eq!(
        i64::from(rules.general.gravity),
        selected["gravity"].as_i64().unwrap()
    );
    assert!(
        !collect_effect_names(&rules)
            .iter()
            .any(|name| name.eq_ignore_ascii_case("120MM")),
        "Bullet images are not animation registrations"
    );
}

#[test]
fn bullet_palette_context_ignores_animation_aliases_and_alt_palette() {
    let rules = simple_rules("[120MM]\nImage=REDIRECT\nAltPalette=yes\n");
    let mut projectile = rules.projectile("Cannon").unwrap().clone();
    let art = rules.art();
    let effects = HashSet::from(["CANNON".to_string()]);
    let cells = effects.clone();
    for (anim, firer, expected) in [
        (false, false, SpritePaletteChoice::Bullet),
        (true, false, SpritePaletteChoice::Anim),
        (false, true, SpritePaletteChoice::Unit),
        (true, true, SpritePaletteChoice::Anim),
    ] {
        projectile.anim_palette = anim;
        projectile.firers_palette = firer;
        let key = projectile_key("Cannon", &projectile, 0, HouseColorIndex(4));
        assert_eq!(
            sprite_palette_for_key(&key, Some(art), &effects, &cells),
            expected
        );
        assert_eq!(
            key.house_color,
            if firer && !anim {
                HouseColorIndex(4)
            } else {
                HouseColorIndex(0)
            }
        );
    }
    let candidates = projectile_shp_candidates(&projectile, "tem", "TEMPERATE");
    let corpus = native();
    let loads = &corpus["controls"]["rows"][2]["first"]["asset_loads"];
    assert_eq!(candidates[0], loads[0]["name"].as_str().unwrap());
}

#[test]
fn projectile_theater_candidates_match_original_image_loader() {
    let corpus = native();
    let theaters = [
        ("tem", "TEMPERATE"),
        ("sno", "SNOW"),
        ("urb", "URBAN"),
        ("des", "DESERT"),
        ("ubn", "NEWURBAN"),
        ("lun", "LUNAR"),
    ];
    for row in corpus["controls"]["filenames"].as_array().unwrap() {
        let (ext, theater) = theaters[row["theater"].as_u64().unwrap() as usize];
        let rules = rules(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nPrimary=105mm\n[105mm]\nProjectile=Cannon\n[Cannon]\nImage=GTTEST\n",
            &format!("[GTTEST]\n{}=yes\n", row["key"].as_str().unwrap()),
        );
        let candidates =
            projectile_shp_candidates(rules.projectile("Cannon").unwrap(), ext, theater);
        let expected: Vec<_> = row["loads"].as_array().unwrap()[..2]
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_ascii_uppercase())
            .collect();
        assert_eq!(
            candidates
                .iter()
                .map(|s| s.to_ascii_uppercase())
                .collect::<Vec<_>>(),
            expected,
            "{theater} {}",
            row["key"]
        );
    }
}

#[test]
fn bullet_frame_lookup_preserves_native_stencil_and_rejects_out_of_range() {
    let corpus = native();
    let hex = corpus["physical_image"]["hex"].as_str().unwrap();
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect();
    let source = ShpSource {
        shp: ShpFile::from_bytes(&bytes).unwrap(),
        found_name: "120MM.SHP".to_string(),
        draw_offsets: (0, 0),
    };
    let rules = simple_rules("");
    let projectile = rules.projectile("Cannon").unwrap();
    let key = projectile_key("Cannon", projectile, 0, HouseColorIndex(0));
    let mut raw = [0; 768];
    for c in corpus["palettes"][0]["colors"].as_array().unwrap() {
        let i = c["index"].as_u64().unwrap() as usize;
        for k in 0..3 {
            raw[i * 3 + k] = c["raw6"][k].as_u64().unwrap() as u8;
        }
    }
    let palette = Palette::from_bytes(&raw).unwrap();
    let rendered = render_shp_frame(&source, &palette, false, &key, Some(&rules)).unwrap();
    assert_eq!(rendered.indices, source.shp.frames[0].pixels);
    assert_eq!([rendered.width, rendered.height], [4, 4]);
    assert_eq!([rendered.offset_x, rendered.offset_y], [-2.0, -2.0]);
    assert_eq!(rendered.canvas_rect, [-12.0, -12.0, 24.0, 24.0]);
    assert!(
        render_shp_frame(
            &source,
            &palette,
            false,
            &projectile_key("Cannon", projectile, 1, HouseColorIndex(0)),
            Some(&rules)
        )
        .is_none()
    );
}

#[test]
fn firer_registration_covers_each_house_scheme_and_lookup_uses_the_supplied_one() {
    let rules = rules(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nPrimary=105mm\n[105mm]\nProjectile=Cannon\n[Cannon]\nImage=120MM\nFirersPalette=yes\n",
        "",
    );
    let projectile = rules.projectile("Cannon").unwrap();
    let assets = AssetManager::from_loose_root_for_test(&std::env::temp_dir());
    // Already bound two-frame asset. The draw selects the retained Bullet+114
    // House's scheme or the local player's (`0x004683A1..0x004683D1`), so
    // every House colour of the match is registered, and no ANIM.PAL key.
    let mut files = HashMap::from([("CANNON".to_string(), ("120MM.SHP".to_string(), 2))]);
    let house_colors = HouseColorMap::from([
        ("Russians".to_string(), HouseColorIndex(4)),
        ("Americans".to_string(), HouseColorIndex(2)),
        ("Neutral".to_string(), crate::rules::house_colors::NO_REMAP),
    ]);
    let mut needed = HashSet::new();
    register_projectile_frames(
        &mut needed,
        &mut files,
        &assets,
        Some(&rules),
        &house_colors,
        "tem",
        "TEMPERATE",
    );
    let expected: HashSet<_> = (0..2)
        .flat_map(|frame| {
            house_colors
                .values()
                .map(move |&color| projectile_key("Cannon", projectile, frame, color))
        })
        .collect();
    assert_eq!(needed, expected);
    assert!(
        needed
            .iter()
            .all(|key| key.palette_context == ShpPaletteContext::BulletFirer)
    );
    let entry = |page| ShpSpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [4.0; 2],
        offset_x: -2.0,
        offset_y: -2.0,
        canvas_rect: [-12.0, -12.0, 24.0, 24.0],
        extended: false,
        page,
    };
    for frame in 0..2 {
        let russian = projectile_key("Cannon", projectile, frame, HouseColorIndex(4));
        let american = projectile_key("Cannon", projectile, frame, HouseColorIndex(2));
        let atlas = SpriteAtlas::new(
            Vec::new(),
            HashMap::from([(russian, entry(0)), (american, entry(1))]),
        );
        for (scheme, page) in [(HouseColorIndex(4), 0), (HouseColorIndex(2), 1)] {
            assert_eq!(
                atlas
                    .projectile_sprite("Cannon", projectile, frame, scheme)
                    .unwrap()
                    .page,
                page
            );
        }
        assert!(
            atlas
                .projectile_sprite("Cannon", projectile, 2, HouseColorIndex(4))
                .is_none()
        );
    }
}
#[test]
#[ignore = "requires physical retail archives; production selected Hills reader and SHP/palette binding"]
fn retail_hills_projectile_assets_match_original_reader_and_physical_bytes() {
    let root = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let scenario = crate::headless_scenario::load(&root, "Hills.mmx", 0x0B21_D6E5).unwrap();
    let rules = &scenario.runtime.resources.rules;
    let art = rules.art();
    let assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let corpus = native();
    let projectile = rules.projectile("Cannon").unwrap();
    assert_eq!(
        rules.object("MTNK").unwrap().primary.as_deref(),
        Some("105mm")
    );
    assert_eq!(
        rules.weapon("105mm").unwrap().projectile.as_deref(),
        Some("Cannon")
    );
    let mut needed = HashSet::new();
    let mut files = HashMap::new();
    register_projectile_frames(
        &mut needed,
        &mut files,
        &assets,
        Some(rules),
        &HouseColorMap::new(),
        "tem",
        "TEMPERATE",
    );
    let (file, count) = &files["CANNON"];
    assert_eq!(
        u64::from(*count),
        corpus["layers"][0]["state"]["frame_count"]
            .as_u64()
            .unwrap()
    );
    assert!(needed.contains(&projectile_key("Cannon", projectile, 0, HouseColorIndex(0))));
    let data = assets.get_ref(file).unwrap();
    assert_eq!(
        data.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        corpus["physical_image"]["hex"].as_str().unwrap()
    );
    let mut source = load_shp_source(
        &assets,
        "CANNON",
        Some(file),
        "tem",
        "TEMPERATE",
        Some(rules),
        Some(art),
    )
    .unwrap();
    source.draw_offsets = (0, 0);
    let palette = Palette::from_bytes(assets.get_ref("palette.pal").unwrap()).unwrap();
    let sprite = render_shp_frame(
        &source,
        &palette,
        false,
        &projectile_key("Cannon", projectile, 0, HouseColorIndex(0)),
        Some(rules),
    )
    .unwrap();
    assert_eq!(sprite.indices, source.shp.frames[0].pixels);
    for (index, rgba) in sprite.indices.iter().zip(sprite.rgba.chunks_exact(4)) {
        let c = corpus["palettes"][0]["colors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["index"].as_u64() == Some(u64::from(*index)))
            .unwrap();
        assert_eq!(
            &rgba[..3],
            &std::array::from_fn::<_, 3, _>(|k| c["raw6"][k].as_u64().unwrap() as u8 * 4)
        );
        assert_eq!(rgba[3], if *index == 0 { 0 } else { 255 });
    }
}

/// Only pinned physical bytes are installed; no implementation-generated SHP.
struct ProjectileAssetDirectory(std::path::PathBuf);

impl ProjectileAssetDirectory {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vera20k-projectile-image-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for ProjectileAssetDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn projectile_asset_binding_uses_native_last_load_not_current_image_text() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/bridge_render_art_state.json",
    ))
    .unwrap();
    let bytes = native()["physical_image"]["hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    let directory = ProjectileAssetDirectory::new();
    std::fs::write(directory.0.join("120mm.shp"), bytes).unwrap();
    let assets = AssetManager::from_loose_root_for_test(&directory.0);
    for case in corpus["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["name"].as_str().unwrap().starts_with("physical_"))
    {
        let id = case["identity"].as_str().unwrap();
        let prefix = format!(
            "[General]\nMetallicDebris=D\n[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile={id}\n"
        );
        let art = IniFile::from_str(case["art"].as_str().unwrap());
        let mut layers = RulesLayerStack::new(IniFile::from_str(&prefix));
        let initial =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
        let unread = initial.projectile(id).unwrap();
        assert_eq!(
            unread.image.as_deref(),
            case["constructor"]["image"].as_str()
        );
        assert!(
            projectile_shp_candidates(unread, "tem", "TEMPERATE").is_empty(),
            "constructor text is not an admitted image load"
        );
        for row in case["rows"].as_array().unwrap() {
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(row["rules"].as_str().unwrap()),
            );
            let rules =
                RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap())
                    .unwrap();
            let projectile = rules.projectile(id).unwrap();
            let candidates = projectile_shp_candidates(projectile, "tem", "TEMPERATE");
            let found = find_shp(&assets, &candidates);
            assert_eq!(
                found.map(|(name, _)| name),
                row["state"]["loaded_file"].as_str(),
                "{} {}",
                case["name"],
                row["rules"]
            );
            let count = found.map_or(0, |(_, bytes)| {
                ShpFile::frame_count_from_bytes(bytes).unwrap()
            });
            assert_eq!(
                u64::from(count),
                row["state"]["frame_count"].as_u64().unwrap()
            );
            if let Some((_, bytes)) = found {
                let shp = ShpFile::from_bytes(bytes).unwrap();
                assert_eq!(
                    serde_json::json!([shp.width, shp.height]),
                    row["state"]["canvas"]
                );
            }
            let mut needed = HashSet::new();
            let mut files = HashMap::new();
            register_projectile_frames(
                &mut needed,
                &mut files,
                &assets,
                Some(&rules),
                &HouseColorMap::new(),
                "tem",
                "TEMPERATE",
            );
            assert_eq!(
                needed.len(),
                if projectile.inviso {
                    0
                } else {
                    usize::from(count)
                }
            );
        }
    }
}
