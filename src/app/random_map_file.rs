//! VERA extension: a random map's file made without the game's menus, for a
//! launcher's random-map room (docs/spawn.md, "Random maps"). The room
//! carries only the share code and the file's SHA-256, and every player makes
//! the file from the code, so it must come out byte for byte the same on
//! every machine: desktop `vera20k --generate-map` and the browser's
//! `generate_map()` both end in [`generate`].
//!
//! What keeps it the same: the options are the code's alone, read without
//! Ares, so Ares' Urban Areas (whose foot picks draw the process Main stream)
//! never runs; the generator draws only streams seeded by the map's seed;
//! and the preview's terrain variants, which the setup dialog draws from the
//! process frontend Main stream, draw from a Main stream seeded by that seed.
//! The game's own settings (VERA.INI's Ares setting and Options switches)
//! are not read: the files of the game folder, and of a mod when one is
//! chosen, decide.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::map::rmg::RmgOptions;

/// The `[RandomMap]` key a `.SED` must name: without it the seed would be
/// the constructor's, which no player chose.
const SEED_KEY: &str = "Seed";
/// Widest `[Preview]` written; a wider one is halved until it fits. The
/// launcher and Choose Map show it scaled down, and the large sizes would
/// otherwise carry megabytes of preview.
const MAX_PREVIEW_WIDTH: u32 = 512;

/// The JSON a generation reports (docs/spawn.md, "Random maps").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RandomMapReport {
    /// The options' share code as this build writes it (with its version).
    pub code: String,
    /// SHA-256 of the file's bytes, lowercase hex.
    pub sha256: String,
    pub byte_length: usize,
    /// The options' seed; Fair Starts may have built the map from a later one.
    pub seed: u16,
    /// Start positions in the file, which is the options' player count.
    pub players: u32,
}

/// A generated map file and its report.
pub(crate) struct RandomMapFile {
    pub bytes: Vec<u8>,
    pub report: RandomMapReport,
}

/// Read a share code (`VR1-…`, any case) or `.SED` text with a
/// `[RandomMap]` section that names its `Seed`. The options a random-map
/// room cannot carry yet are refused: a theater other than temperate (0) or
/// snow (1), VERA's Urban Presence (`VeraUrban`) and the Extras.
pub(crate) fn read_options(input: &str) -> Result<RmgOptions> {
    let text = input.trim();
    let options = if text
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("VR1-"))
    {
        RmgOptions::default()
            .from_share_code(text, false)
            .map(|(options, _)| options)
            .context("the share code is damaged or cut short")?
    } else {
        // The INI reader drops a last line without a line feed, as the
        // game's does; text handed over by a launcher keeps it.
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!("{text}\n"));
        let section = ini
            .section("RandomMap")
            .context("the input is neither a share code (VR1-…) nor .SED text with [RandomMap]")?;
        ensure!(
            section.is_present(SEED_KEY),
            "the .SED text names no [RandomMap] {SEED_KEY}"
        );
        let mut options = RmgOptions::default();
        options.apply_sed(&ini, false);
        options.normalize();
        options
    };
    ensure!(
        matches!(options.theater, 0 | 1),
        "theater {} is not a random-map theater (0 temperate, 1 snow)",
        options.theater
    );
    if options.urban_enabled {
        bail!("random-map rooms do not take Urban Presence (VeraUrban) yet");
    }
    if options.extras != Default::default() {
        bail!("random-map rooms do not take the Extras yet");
    }
    Ok(options)
}

/// Resolve the mode and all generation inputs through the same owners used
/// by the process probe and ordinary Skirmish setup.
fn prepare_generation(
    assets: &mut crate::assets::asset_manager::AssetManager,
    rules: &crate::rules::process_owner::NativeRulesProcessOwner,
    options: &RmgOptions,
) -> Result<super::shell_random_map::PreparedRandomMapGeneration> {
    use super::shell_random_map::{RandomMapRulesPreparation, prepare_random_map_generation};
    let modes = crate::skirmish_modes::skirmish_modes_from_assets(assets)?;
    let mode = crate::skirmish_modes::default_skirmish_mode(&modes)
        .context("the selected mode catalog has no Skirmish game mode")?;
    ensure!(
        mode.random_maps_allowed,
        "GameMode={} in {} does not allow random maps",
        mode.id,
        assets.ini_roots().modes()
    );
    let mode_rules =
        crate::rules::retail_sources::select_mode_ini(assets, Some(&mode.override_file))
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!(
                    "could not read GameMode={} rules ({})",
                    mode.id, mode.override_file
                )
            })?;
    prepare_random_map_generation(
        assets,
        options,
        RandomMapRulesPreparation::NetworkAgreement(Some(rules)),
        mode_rules.as_ref(),
    )
    .context("the random map's theater or rules are unavailable")
}

/// Generate the map `options` describe from the loaded game files and Rules
/// owner, as a multiplayer map file named `Random Map <seed>` for 2 to its
/// start count, with its preview and start header.
pub(crate) fn generate(
    assets: &mut crate::assets::asset_manager::AssetManager,
    rules: &crate::rules::process_owner::NativeRulesProcessOwner,
    options: &RmgOptions,
) -> Result<RandomMapFile> {
    let prepared = prepare_generation(assets, rules, options)?;
    let run = crate::map::rmg::build::generate_map(
        options,
        &prepared.settings,
        &prepared.resolved_inputs,
        &prepared.blocks,
        &crate::map::rmg::build::GenerationObjects::tech(&prepared.tech_types, prepared.ares),
    );
    let generated = run.map();
    ensure!(
        generated.unfilled_start_slots == 0,
        "the map has no room for {} of its {} players; try another seed or a larger size",
        generated.unfilled_start_slots,
        options.num_players
    );
    let seed = options.seed_u16();
    // The selector's table is the only draw the preview makes; a fresh
    // cache draws it from this seeded stream instead of the process's.
    let mut main = crate::sim::rng::MainRng::new(u64::from(seed)).draws();
    let preview = super::shell_random_map::rasterise_random_map(
        &generated.map_file,
        // No baked markers: the start header numbers the starts.
        &[],
        prepared.preview_inputs(),
        Some(assets),
        &mut main,
        &mut Default::default(),
    )
    .map(|mut image| {
        while image.width > MAX_PREVIEW_WIDTH {
            image = image.halved();
        }
        image
    });
    let starts = crate::app::loading::composition::scenario_start_staging(
        &generated.map_file,
        &generated.start_waypoints,
    )
    .preview_source_bounds();
    let players = generated.start_waypoints.len() as u32;
    let bytes = crate::map::map_export::multiplayer_map_text(
        &generated.map_file,
        &format!("Random Map {seed}"),
        2,
        players,
        preview
            .as_ref()
            .map(|image| crate::map::map_export::ExportPreview {
                width: image.width,
                height: image.height,
                rgba: &image.rgba,
            }),
        starts.as_ref(),
    )
    .into_bytes();
    Ok(RandomMapFile {
        report: RandomMapReport {
            code: options.share_code(false),
            sha256: crate::util::sha256::sha256_hex(&bytes),
            byte_length: bytes.len(),
            seed,
            players,
        },
        bytes,
    })
}

/// Read `input` ([`read_options`]), load the game folder `config` names with
/// the selected `active_mod` (`mods::launch_mod`), and generate its map.
pub(crate) fn generate_from_game(
    config: &crate::util::config::GameConfig,
    active_mod: Option<crate::app::mods::ActiveMod>,
    input: &str,
) -> Result<RandomMapFile> {
    let options = read_options(input)?;
    let (mut process, _, _) = crate::app::spawn_probe::load_process_sources(
        config,
        Default::default(),
        crate::app::frontend::startup_options::RetailStartupOptions::default().media_archive_mode,
        active_mod,
        // The game's files decide, whatever the player's own setting.
        crate::util::ares_launch::AresSetting::Auto,
        crate::util::kratos_launch::KratosSetting::Auto,
        false,
    )?;
    let (assets, rules, _) = process.manager_and_rules_with_tile_cache();
    let assets = assets.context("the game files are not loaded")?;
    assets.open_for_play();
    generate(
        assets,
        rules.context("the game's rules are not loaded")?,
        &options,
    )
}

/// Desktop `--generate-map`: the game folder comes from config.toml as for
/// play, the mod from config.toml unless `--mod` or `--no-mod` chooses. The
/// file is written beside `output` first and then renamed over it, so no
/// reader sees part of one.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn run_desktop(
    input: &str,
    output: &std::path::Path,
    mod_switch: Option<&crate::app::mods::ModSwitch>,
) -> Result<RandomMapReport> {
    let mut config = crate::util::config::GameConfig::load()?;
    // RA2 Mode's theaters, as the game the launcher starts plays them.
    config.use_ra2_mode(
        crate::app::spawn_probe::existing_profile(config.paths.settings_dir()).ra2_mode,
    );
    let map = generate_from_game(
        &config,
        crate::app::mods::launch_mod(&config.paths, mod_switch).map_err(anyhow::Error::msg)?,
        input,
    )?;
    let mut partial = output.as_os_str().to_owned();
    partial.push(".part");
    let partial = std::path::PathBuf::from(partial);
    std::fs::write(&partial, &map.bytes)
        .and_then(|()| std::fs::rename(&partial, output))
        .with_context(|| format!("could not write {}", output.display()))?;
    Ok(map.report)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    const SMALL_SED: &str = "[RandomMap]\nWidth=0\nHeight=0\nNumPlayers=2\nSeed=4242\nMapType=1\nTheater=0\nVeraFair=yes";

    #[test]
    fn a_sed_reads_as_its_share_code_and_its_last_line_counts() {
        // SMALL_SED ends without a line feed: VeraFair must still be read.
        let from_sed = read_options(SMALL_SED).unwrap();
        assert!(from_sed.fair_starts);
        let code = from_sed.share_code(false);
        assert_eq!(read_options(&code.to_lowercase()).unwrap(), from_sed);
    }

    #[test]
    fn options_a_room_cannot_carry_are_refused() {
        let refused = |sed: &str| read_options(sed).unwrap_err().to_string();
        assert!(refused(&SMALL_SED.replace("Theater=0", "Theater=2")).contains("theater 2"));
        assert!(refused(&format!("{SMALL_SED}\nVeraUrban=yes")).contains("VeraUrban"));
        assert!(refused(&format!("{SMALL_SED}\nVeraLandmark=1")).contains("Extras"));
        let unseeded = SMALL_SED.replace("Seed=4242\n", "");
        assert!(
            read_options(&unseeded)
                .unwrap_err()
                .to_string()
                .contains("Seed")
        );
        assert!(read_options("VR1-ABCDE").is_err());
    }

    /// The process sources of the retail install `RA2_DIR` names, loaded as
    /// the CLI loads them.
    fn retail_process() -> Option<crate::app::process_assets::ProcessAssets> {
        let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
        let root = std::env::temp_dir().join(format!("vera-random-map-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("config.toml"),
            format!(
                "[paths]\nra2_dir = '{}'\nsettings_dir = '{}'\n",
                retail.display(),
                root.display()
            ),
        )
        .unwrap();
        let config =
            crate::util::config::GameConfig::load_from(&root, &root.join("vera20k")).unwrap();
        let (process, _, _) = crate::app::spawn_probe::load_process_sources(
            &config,
            Default::default(),
            crate::app::frontend::startup_options::RetailStartupOptions::default()
                .media_archive_mode,
            None,
            crate::util::ares_launch::AresSetting::Auto,
            crate::util::kratos_launch::KratosSetting::Auto,
            false,
        )
        .unwrap();
        Some(process)
    }

    fn generate_in(
        process: &mut crate::app::process_assets::ProcessAssets,
        input: &str,
    ) -> RandomMapFile {
        let (assets, rules, _) = process.manager_and_rules_with_tile_cache();
        generate(
            assets.unwrap(),
            rules.unwrap(),
            &read_options(input).unwrap(),
        )
        .unwrap()
    }

    /// The files every machine must write. A change to the generator, the
    /// preview or the writer changes these; so would a platform that builds
    /// another map, which is what rooms cannot survive.
    #[test]
    #[ignore = "slow; requires the retail install (RA2_DIR)"]
    fn generated_files_are_the_same_every_time_and_match_their_golden_digests() {
        let Some(mut process) = retail_process() else {
            return;
        };
        for (sed, sha256) in [
            (
                SMALL_SED,
                "66e4edc6ebaa5b37da13e661d2b917164625fac79863ddefaa5eb0f836732e0f",
            ),
            (
                "[RandomMap]\nWidth=1\nHeight=1\nNumPlayers=4\nSeed=777\nMapType=3\nTheater=1\nVeraFair=yes\n",
                "c23bc8dd3380390ff9d63da7128011bcbba4ba926c41f36b9a4c83d91932e5c4",
            ),
            (
                "[RandomMap]\nWidth=0\nHeight=0\nNumPlayers=3\nSeed=31\nMapType=0\nTheater=0\nWaterAmount=60\n",
                "92c4bb01c582da5b0debee84e4bb351cbfd9b7d4f620d98ee14ea70a15f74b7c",
            ),
        ] {
            let first = generate_in(&mut process, sed);
            let again = generate_in(&mut process, sed);
            assert!(first.bytes == again.bytes, "{sed}: two runs differ");
            // The code the report carries makes the same file.
            let from_code = generate_in(&mut process, &first.report.code);
            assert!(first.bytes == from_code.bytes, "{sed}: its code differs");
            assert_eq!(first.report.sha256, sha256, "{sed}");
        }
    }

    #[test]
    fn selected_mode_rules_reach_generation_and_its_random_map_gate() {
        let Some((retail, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let startup = owner.startup_compatibility_projection().unwrap();
        let stock = startup.neutral_tech_buildings();
        assert!(
            stock.len() > 1,
            "retail must provide more than one neutral tech building"
        );
        let selected_type = &stock[0];
        let root = crate::map::source::test_support::TestDirectory::new("rmg-selected-mode");
        let mod_dir = root.path().join("Mods").join("Catalog");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(mod_dir.join("mod.ini"), "[Files]\nModes=custom-modes.ini\n").unwrap();
        // FreeForAll-only catalogs are eligible too; no Battle row is invented.
        let roster = "[FreeForAll]\n12=GUI:Custom,STT:Custom,selected-mode.ini,standard,true\n";
        std::fs::write(mod_dir.join("custom-modes.ini"), roster).unwrap();
        std::fs::write(
            mod_dir.join("selected-mode.ini"),
            format!("[AI]\nNeutralTechBuildings={selected_type}\n"),
        )
        .unwrap();
        root.write(
            "config.toml",
            format!(
                "[paths]\nra2_dir='{}'\nsettings_dir='{}'\nmod='Catalog'\n",
                retail.display(),
                root.path().display()
            )
            .as_bytes(),
        );
        let config =
            crate::util::config::GameConfig::load_from(root.path(), &root.path().join("vera20k"))
                .unwrap();
        let load = || {
            crate::app::spawn_probe::load_process_sources(
                &config,
                Default::default(),
                crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
                crate::app::mods::launch_mod(&config.paths, None).unwrap(),
                crate::util::ares_launch::AresSetting::Auto,
                crate::util::kratos_launch::KratosSetting::Auto,
                false,
            )
            .unwrap()
            .0
        };
        let options = read_options(SMALL_SED).unwrap();
        let mut process = load();
        let (assets, rules, _) = process.manager_and_rules_with_tile_cache();
        let prepared = prepare_generation(assets.unwrap(), rules.unwrap(), &options).unwrap();
        assert_eq!(
            prepared
                .tech_types
                .iter()
                .map(|ty| ty.name.as_str())
                .collect::<Vec<_>>(),
            [selected_type.as_str()]
        );
        std::fs::write(
            mod_dir.join("custom-modes.ini"),
            roster.replace(",true", ",false"),
        )
        .unwrap();
        let mut refused = load();
        let (assets, rules, _) = refused.manager_and_rules_with_tile_cache();
        let error = prepare_generation(assets.unwrap(), rules.unwrap(), &options)
            .err()
            .expect("selected mode refuses random maps");
        assert!(error.to_string().contains("GameMode=12"));
        assert!(error.to_string().contains("does not allow random maps"));
    }
}
