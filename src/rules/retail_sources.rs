//! Selection of retail INI bytes at the production archive boundary.
//!
//! Selection transfers parsed snapshots to NativeRulesProcessOwner. It does not
//! own registry transitions or implement another layer/parser. Source identities
//! describe exactly the consumed buffer, including loose-file winners.
use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::ruleset::RuleSet;
use crate::util::sha256::sha256_hex;

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct IniSourceIdentity {
    pub(crate) logical_name: String,
    pub(crate) source_archive: String,
    pub(crate) entry_id: i32,
    pub(crate) payload_len: usize,
    pub(crate) source_sha256: String,
}

pub(crate) struct SelectedIni {
    pub(crate) ini: IniFile,
    pub(crate) source: IniSourceIdentity,
}

pub(crate) fn select_ini(assets: &AssetManager, name: &str) -> Result<SelectedIni, String> {
    let selected = assets
        .resolve_ref(name)
        .ok_or_else(|| format!("retail INI {name} is unavailable"))?;
    log::info!(
        "Loading {name} ({} bytes) from {}",
        selected.bytes.len(),
        selected.source_archive
    );
    let ini = IniFile::from_bytes(selected.bytes)
        .map_err(|error| format!("parse {name} from {}: {error}", selected.source_archive))?;
    Ok(SelectedIni {
        ini,
        source: IniSourceIdentity {
            logical_name: name.to_owned(),
            source_archive: selected.source_archive.to_owned(),
            entry_id: selected.entry_id,
            payload_len: selected.bytes.len(),
            source_sha256: sha256_hex(selected.bytes),
        },
    })
}

/// Transient selected inputs, consumed when the process authority is constructed.
/// Optional LANGRULE/SOUND retain the existing production missing/parse-failure
/// policy. ART is fixed and never enters the scenario layer stack.
pub(crate) struct RetailRulesSources {
    pub(crate) rulesmd: SelectedIni,
    pub(crate) langrule: Option<SelectedIni>,
    pub(crate) artmd: SelectedIni,
    pub(crate) soundmd: Option<SelectedIni>,
}

impl RetailRulesSources {
    pub(crate) fn select(assets: &AssetManager) -> Result<Self, String> {
        let rulesmd = select_ini(assets, "rulesmd.ini")?;
        let langrule = select_ini(assets, "langrule.ini").ok();
        let artmd = select_ini(assets, "artmd.ini")?;
        let soundmd = select_ini(assets, "soundmd.ini").ok();
        Ok(Self {
            rulesmd,
            langrule,
            artmd,
            soundmd,
        })
    }

    pub(crate) fn into_startup(self) -> Result<StartupRulesLoad, String> {
        let mut native_owner = NativeRulesProcessOwner::from_cold_start_sources(
            self.rulesmd.ini,
            self.langrule.map(|source| source.ini),
            self.artmd.ini,
        )
        .map_err(|error| format!("Native rules cold startup failed: {error}"))?;
        // Init_Game52C763..52C796 selects this once, separately from Rules passes.
        if let Some(sound) = self.soundmd {
            native_owner
                .select_fixed_sounds(crate::rules::sound_ini::SoundRegistry::from_ini(&sound.ini));
        }
        let (compatibility_rules, compatibility_projection) =
            match native_owner.startup_compatibility_projection() {
                Ok(processed) => {
                    let mut compatibility_rules = RuleSet::from_processed_rules(&processed)
                        .map_err(|error| {
                            log::warn!("Failed to parse startup rules projection: {error}")
                        })
                        .ok();
                    if let Some(rules) = compatibility_rules.as_mut() {
                        native_owner.bind_sinking_sounds(rules, &processed);
                    }
                    let compatibility_projection =
                        processed.into_projection_discarding_native_receipt();
                    (compatibility_rules, Some(compatibility_projection))
                }
                Err(error) => {
                    log::warn!("Failed to build startup rules projection: {error}");
                    (None, None)
                }
            };

        Ok(StartupRulesLoad {
            compatibility_rules,
            compatibility_projection,
            native_owner,
        })
    }
}

/// Cold-start native Rules authority plus the one shell compatibility
/// projection derived from the same selected INI snapshots.
pub(crate) struct StartupRulesLoad {
    compatibility_rules: Option<RuleSet>,
    compatibility_projection: Option<IniFile>,
    native_owner: NativeRulesProcessOwner,
}

impl StartupRulesLoad {
    pub(crate) fn into_parts(self) -> (Option<RuleSet>, Option<IniFile>, NativeRulesProcessOwner) {
        (
            self.compatibility_rules,
            self.compatibility_projection,
            self.native_owner,
        )
    }
}

/// Production cold-start selection. Failed compatibility projection does not
/// discard the native registry owner; only selection/cold construction can fail.
pub(crate) fn load_startup_rules(assets: &AssetManager) -> Option<StartupRulesLoad> {
    RetailRulesSources::select(assets)
        .and_then(RetailRulesSources::into_startup)
        .map_err(|error| log::warn!("{error}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn selected_sources_hash_consumed_bytes_and_survive_disk_changes() {
        let directory = TestDirectory::new("rules-selection");
        let root = b"[General]\r\nBuildSpeed=.7\r\nFlightLevel=1500\r\n";
        directory.write("rulesmd.ini", root);
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let selected = RetailRulesSources::select(&assets).expect("selected sources");
        assert_eq!(selected.rulesmd.source.source_sha256, sha256_hex(root));
        assert_eq!(selected.rulesmd.source.payload_len, root.len());
        assert!(selected.langrule.is_none());
        assert!(selected.soundmd.is_none());
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=9\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=8x8\n");
        let (_, _, mut owner) = selected.into_startup().expect("cold startup").into_parts();
        let (rules, projection, art, _) = owner
            .load_noncampaign_scenario(None, &IniFile::empty())
            .expect("scenario from retained startup")
            .into_parts();
        assert_eq!(
            projection.section("General").unwrap().get("BuildSpeed"),
            Some(".7")
        );
        assert_eq!(art.section("TECH").unwrap().get("Foundation"), Some("2x3"));
        assert_eq!(
            rules.production.build_speed.bits(),
            f64::from(0.7_f32).to_bits()
        );
        assert_eq!(
            owner
                .selected_rules_root()
                .section("General")
                .unwrap()
                .get("FlightLevel"),
            Some("1500")
        );
    }

    #[test]
    fn langrule_overlays_standalone_rulesmd_through_retained_owner() {
        // Migrated from the deleted app-local compose_rules_layers regression.
        let owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n"),
            Some(IniFile::from_str("[General]\nBuildSpeed=.58\n")),
            IniFile::empty(),
        )
        .expect("cold startup");
        let processed = owner
            .startup_compatibility_projection()
            .expect("startup projection");
        let ini = processed.ini();
        assert_eq!(
            ini.section("General").unwrap().get("BuildSpeed"),
            Some(".58")
        );
        assert_eq!(
            ini.section("General").unwrap().get("FlightLevel"),
            Some("1500")
        );
        let rules = RuleSet::from_processed_rules(&processed).expect("processed rules");
        assert_eq!(
            rules.production.build_speed.bits(),
            f64::from(0.58_f32).to_bits()
        );
    }

    #[test]
    fn random_map_catalog_uses_retained_root_langrule_and_fixed_art() {
        let directory = TestDirectory::new("rmg-retained-rules");
        directory.write("rulesmd.ini", b"[AI]\nNeutralTechBuildings=TECH1\n");
        directory.write("langrule.ini", b"[AI]\nNeutralTechBuildings=TECH2\n");
        directory.write("artmd.ini", b"[TECH2]\nFoundation=2x3\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let (_, _, owner) = load_startup_rules(&assets).expect("startup").into_parts();
        directory.write("langrule.ini", b"[AI]\nNeutralTechBuildings=WRONG\n");
        directory.write("artmd.ini", b"[TECH2]\nFoundation=1x1\n");
        let projected = owner
            .startup_compatibility_projection()
            .expect("read-only startup projection");
        let types = crate::map::rmg::tech_catalog::resolve(projected.ini(), owner.fixed_art());
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "TECH2");
        assert_eq!(
            types[0].footprint,
            vec![(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]
        );
        let again = owner
            .startup_compatibility_projection()
            .expect("repeated projection");
        assert_eq!(again.content_hash(), projected.content_hash());
    }
    /// Stock field regressions through retained startup, scenario passes and
    /// fixed ART. These are retail-data checks, not fresh native goldens.
    #[test]
    fn stock_hills_battle_fields_match_retail_contracts() {
        use crate::util::fixed_math::SimFixed;
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = retail.rules;
        let gacnst = rules.object("GACNST").expect("retail GACNST");
        assert_eq!(gacnst.strength, 1000);
        assert_eq!(gacnst.armor, "concrete");
        assert_eq!(gacnst.adjacent, 2);
        assert_eq!(gacnst.power, 0);
        let gapowr = rules.object("GAPOWR").expect("retail GAPOWR");
        assert_eq!(gapowr.strength, 750);
        assert_eq!(gapowr.armor, "wood");
        assert_eq!(gapowr.adjacent, 2);
        assert_eq!(gapowr.power, 200);
        let amradr = rules.object("AMRADR").expect("retail AMRADR");
        assert_eq!(amradr.strength, 600);
        assert_eq!(amradr.armor, "steel");
        assert_eq!(amradr.adjacent, 2);
        assert_eq!(amradr.power, -50);
        assert!(amradr.radar);

        for id in ["GAWEAP", "NAWEAP", "GAYARD", "NAYARD", "YAWEAP", "YAYARD"] {
            assert!(
                rules
                    .object(id)
                    .is_some_and(|object| object.weapons_factory)
            );
        }
        for id in ["GAPILE", "GAPOWR"] {
            assert!(
                !rules
                    .object(id)
                    .is_some_and(|object| object.weapons_factory)
            );
        }

        assert_eq!(
            rules.object("APOC").expect("retail APOC").weight,
            SimFixed::lit("3.5")
        );
        assert_eq!(
            rules.object("MTNK").expect("retail MTNK").weight,
            SimFixed::lit("2")
        );
        assert_eq!(
            rules.general.direct_rocking_coefficient,
            SimFixed::lit("1.5")
        );
        assert_eq!(rules.general.fallback_coefficient, SimFixed::lit("0.1"));
        let v3wh = rules.warhead("V3WH").expect("processed retail V3WH");
        assert!(v3wh.rocker);
        assert!(!v3wh.direct_rocker);

        for id in ["GHOST", "TANY", "PTROOP"] {
            assert!(rules.object(id).is_some_and(|object| object.c4));
        }
        for id in ["CAMISC01", "CAMISC02", "CAMISC06", "AMMOCRAT"] {
            assert!(!rules.object(id).is_some_and(|object| object.can_c4));
        }
        let napowr = rules.object("NAPOWR").expect("retail NAPOWR");
        assert_eq!(napowr.strength, 750);
        assert!(napowr.can_c4);
        assert_eq!(rules.c4_delay_ticks, 27);
    }
}
