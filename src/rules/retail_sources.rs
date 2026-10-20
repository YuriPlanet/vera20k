//! Selection of retail INI bytes at the production archive boundary.
//!
//! Selection transfers parsed snapshots to NativeRulesProcessOwner. It does not
//! own registry transitions or implement another layer/parser. Source identities
//! describe exactly the consumed buffer, including loose-file winners.
use std::sync::Arc;

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::directives::{
    DirectiveDiagnostic, DirectiveOptions, include_identity, process_directives,
};
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
use crate::util::sha256::sha256_hex;

#[derive(Clone, Debug, serde::Serialize)]
pub struct IniSourceIdentity {
    pub(crate) logical_name: String,
    pub source_archive: String,
    pub(crate) entry_id: i32,
    pub(crate) payload_len: usize,
    pub(crate) source_sha256: String,
}

#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct SelectedIni {
    pub ini: IniFile,
    pub source: IniSourceIdentity,
    /// Physical child snapshots in traversal order (including repeated DAG
    /// references). Root identity still describes the original root bytes.
    pub included_sources: Vec<IniSourceIdentity>,
    pub directive_diagnostics: Vec<DirectiveDiagnostic>,
}

pub fn select_ini(assets: &AssetManager, name: &str) -> Result<SelectedIni, String> {
    select_ini_with_options(assets, name, DirectiveOptions::default())
}

pub fn select_ini_with_options(
    assets: &AssetManager,
    name: &str,
    options: DirectiveOptions,
) -> Result<SelectedIni, String> {
    let mut selected = select_physical_ini(assets, name)?;
    let (expanded, included_sources) = expand_ini_with_assets(assets, selected.ini, name, options)?;
    selected.ini = expanded.ini;
    selected.included_sources = included_sources;
    selected.directive_diagnostics = expanded.diagnostics;
    Ok(selected)
}

/// Maps already own their decoded physical INI (including MMX wrappers).
/// They enter the same traversal/provider instead of selecting a second root.
pub(crate) fn expand_ini_with_assets(
    assets: &AssetManager,
    ini: IniFile,
    name: &str,
    options: DirectiveOptions,
) -> Result<
    (
        crate::rules::ini_parser::directives::ExpandedIni,
        Vec<IniSourceIdentity>,
    ),
    String,
> {
    let mut included_sources = Vec::new();
    let expanded = process_directives(
        ini,
        name,
        |reference| {
            // All references share the production VFS root; do not rebase on the
            // including document's host directory or open arbitrary host paths.
            let logical = include_identity(reference);
            let Some(resolved) = assets.resolve_ref(&logical) else {
                return Ok(None);
            };
            if resolved.bytes.is_empty() {
                return Ok(None);
            }
            let child = select_physical_ini(assets, &logical)?;
            included_sources.push(child.source);
            Ok(Some(child.ini))
        },
        options,
    )
    .map_err(|error| error.to_string())?;
    for diagnostic in &expanded.diagnostics {
        log::warn!("INI directive: {diagnostic}");
    }
    Ok((expanded, included_sources))
}

fn select_physical_ini(assets: &AssetManager, name: &str) -> Result<SelectedIni, String> {
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
        included_sources: Vec::new(),
        directive_diagnostics: Vec::new(),
        source: IniSourceIdentity {
            logical_name: name.to_owned(),
            source_archive: selected.source_archive.to_owned(),
            entry_id: selected.entry_id,
            payload_len: selected.bytes.len(),
            source_sha256: sha256_hex(selected.bytes),
        },
    })
}

/// Select the native mode object's INI, if there is a mode object.
/// Reset668B8E..668BAA gates on that object, then always processes the INI
/// returned by 5D67B0. A missing file or empty filename therefore still adds
/// an empty Process pass, including its fixed-ART reads and weapon postpass.
/// `None` means no selected object, not an unavailable file. Missing sources
/// have no fabricated archive identity. Present parse failures propagate;
/// native behavior for malformed data is not established here.
pub fn select_mode_ini(
    assets: &AssetManager,
    selected_name: Option<&str>,
) -> Result<Option<IniFile>, String> {
    let Some(name) = selected_name else {
        return Ok(None);
    };
    let name = name.trim();
    if name.is_empty() {
        return Ok(Some(IniFile::empty()));
    }
    if assets.resolve_ref(name).is_none() {
        log::warn!("Selected mode INI {name} is missing; processing an empty layer");
        return Ok(Some(IniFile::empty()));
    }
    select_ini(assets, name).map(|selected| Some(selected.ini))
}

/// Transient selected inputs, consumed when the process authority is constructed.
/// Optional LANGRULE retains the existing production missing/parse-failure
/// policy. ART is fixed and never enters the scenario layer stack. Audio
/// definitions are separately selected and injected before Rules construction.
#[cfg_attr(any(test, feature = "test-support"), derive(Clone))]
pub struct RetailRulesSources {
    pub rulesmd: SelectedIni,
    pub langrule: Option<SelectedIni>,
    pub artmd: SelectedIni,
}

impl RetailRulesSources {
    pub fn select(assets: &AssetManager) -> Result<Self, String> {
        let rulesmd = select_ini(assets, "rulesmd.ini")?;
        let langrule = select_ini(assets, "langrule.ini").ok();
        let artmd = select_ini(assets, "artmd.ini")?;
        Ok(Self {
            rulesmd,
            langrule,
            artmd,
        })
    }

    /// The cold-started native Rules authority alone, without the shell
    /// compatibility projection [`Self::into_startup`] also derives.
    pub fn into_owner(
        self,
        sounds: Arc<SoundRegistry>,
        eva: Arc<EvaRegistry>,
    ) -> Result<NativeRulesProcessOwner, String> {
        NativeRulesProcessOwner::from_cold_start_sources(
            self.rulesmd.ini,
            self.langrule.map(|source| source.ini),
            self.artmd.ini,
            sounds,
            eva,
        )
        .map_err(|error| format!("Native rules cold startup failed: {error}"))
    }

    pub fn into_startup(
        self,
        sounds: Arc<SoundRegistry>,
        eva: Arc<EvaRegistry>,
    ) -> Result<StartupRulesLoad, String> {
        let native_owner = self.into_owner(sounds, eva)?;
        let (compatibility_rules, compatibility_projection) =
            match native_owner.startup_compatibility_projection() {
                Ok(processed) => {
                    let mut compatibility_rules = RuleSet::from_processed_rules(&processed)
                        .map_err(|error| {
                            log::warn!("Failed to parse startup rules projection: {error}")
                        })
                        .ok();
                    if let Some(rules) = compatibility_rules.as_mut() {
                        native_owner.bind_type_audio_references(rules, &processed);
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
pub struct StartupRulesLoad {
    compatibility_rules: Option<RuleSet>,
    compatibility_projection: Option<IniFile>,
    native_owner: NativeRulesProcessOwner,
}

impl StartupRulesLoad {
    pub fn into_parts(self) -> (Option<RuleSet>, Option<IniFile>, NativeRulesProcessOwner) {
        (
            self.compatibility_rules,
            self.compatibility_projection,
            self.native_owner,
        )
    }
}

/// Production cold-start selection. Failed compatibility projection does not
/// discard the native registry owner; only selection/cold construction can fail.
pub fn load_startup_rules(
    assets: &AssetManager,
    sounds: Arc<SoundRegistry>,
    eva: Arc<EvaRegistry>,
) -> Option<StartupRulesLoad> {
    RetailRulesSources::select(assets)
        .and_then(|sources| sources.into_startup(sounds, eva))
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
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=9\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=8x8\n");
        let (_, _, mut owner) = selected
            .into_startup(Arc::default(), Default::default())
            .expect("cold startup")
            .into_parts();
        let (rules, projection, art, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .expect("scenario from retained startup")
            .into_parts();
        assert_eq!(
            projection
                .section("General")
                .unwrap()
                .get_for_test("BuildSpeed"),
            Some(".7")
        );
        assert_eq!(
            art.section("TECH").unwrap().get_for_test("Foundation"),
            Some("2x3")
        );
        assert_eq!(
            rules.production.build_speed.bits(),
            f64::from(0.7_f32).to_bits()
        );
        assert_eq!(
            owner
                .selected_rules_root()
                .section("General")
                .unwrap()
                .get_for_test("FlightLevel"),
            Some("1500")
        );
    }

    #[test]
    fn optional_mode_ini_missing_file_continues_with_an_empty_layer() {
        // Reset668BA0..668BAA processes the empty INI returned by 5D67B0.
        // ReadTypeData679A5D..679A82 must sweep fixed ART before the map
        // allocates MAP, including an animation first named by AudioVisual.
        let directory = TestDirectory::new("optional-mode-ini-missing");
        directory.write(
            "rulesmd.ini",
            b"[General]\nBuildSpeed=.7\n[AudioVisual]\nSmoke=LATE\n",
        );
        directory.write("artmd.ini", b"[LATE]\nNext=CHILD\n[CHILD]\nRate=1\n");
        directory.write("MPEmptyMD.ini", b"");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        assert!(
            select_ini(&assets, "MPMissingMD.ini").is_err(),
            "the required-INI selector keeps a missing file fatal (rulesmd policy)"
        );
        let load = |name| {
            let selected = select_mode_ini(&assets, Some(name)).expect("select mode");
            let (_, _, mut owner) = load_startup_rules(&assets, Arc::default(), Default::default())
                .expect("startup")
                .into_parts();
            let (rules, ini, _, receipt) = owner
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
                        selected.as_ref(),
                    ),
                    &IniFile::from_str("[Animations]\n0=MAP\n[General]\nFlightLevel=1500\n"),
                )
                .expect("scenario rebuild with selected mode")
                .into_parts();
            assert_eq!(
                rules.production.build_speed.bits(),
                f64::from(0.7_f32).to_bits()
            );
            assert_eq!(
                ini.section("General").unwrap().get_for_test("FlightLevel"),
                Some("1500")
            );
            let (pre, post) = receipt.into_parts();
            (rules.source_ini_hash(), pre.into_parts(), post.into_parts())
        };
        let missing = load("MPMissingMD.ini");
        let empty = load("MPEmptyMD.ini");
        assert_eq!(
            missing.2, empty.2,
            "a missing file must retain the empty Process pass"
        );
        assert_eq!(
            missing.1, empty.1,
            "the pre-reset constructor prefix stays identical"
        );
        assert_eq!(
            missing.0, empty.0,
            "selected empty sources have the same rules identity"
        );
        assert_eq!(
            load(""),
            empty,
            "an object with an empty filename still processes its INI"
        );
    }

    #[test]
    fn no_selected_mode_object_selects_no_layer() {
        let directory = TestDirectory::new("no-mode-ini");
        directory.write("rulesmd.ini", b"[General]\nBuildSpeed=.7\n");
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let selected = select_mode_ini(&assets, None).expect("no selected mode");
        assert!(selected.is_none());
    }

    #[test]
    fn optional_mode_ini_present_file_overlays_rulesmd_under_the_map() {
        let directory = TestDirectory::new("optional-mode-ini-present");
        directory.write(
            "rulesmd.ini",
            b"[General]\nBuildSpeed=.7\nFlightLevel=1500\n",
        );
        directory.write("artmd.ini", b"[TECH]\nFoundation=2x3\n");
        directory.write("MPBattleMD.ini", b"[General]\nFlightLevel=1200\n");
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let selected = select_mode_ini(&assets, Some("MPBattleMD.ini"))
            .expect("present optional INI selects")
            .expect("a present file yields a layer");
        assert_eq!(
            selected
                .section("General")
                .unwrap()
                .get_for_test("FlightLevel"),
            Some("1200")
        );
        let (_, _, mut owner) = load_startup_rules(&assets, Arc::default(), Default::default())
            .expect("startup")
            .into_parts();
        let (_, ini, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(
                    &selected,
                )),
                &IniFile::from_str("[General]\nBuildSpeed=1\n"),
            )
            .expect("scenario rebuild with the mode layer")
            .into_parts();
        let general = ini.section("General").unwrap();
        assert_eq!(
            general.get_for_test("FlightLevel"),
            Some("1200"),
            "the mode layer beats rulesmd"
        );
        assert_eq!(
            general.get_for_test("BuildSpeed"),
            Some("1"),
            "the map layer beats the mode layer"
        );
    }

    #[test]
    fn langrule_overlays_standalone_rulesmd_through_retained_owner() {
        // Migrated from the deleted app-local compose_rules_layers regression.
        let owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n"),
            Some(IniFile::from_str("[General]\nBuildSpeed=.58\n")),
            IniFile::empty(),
            Arc::default(),
            Default::default(),
        )
        .expect("cold startup");
        let processed = owner
            .startup_compatibility_projection()
            .expect("startup projection");
        let ini = processed.ini();
        assert_eq!(
            ini.section("General").unwrap().get_for_test("BuildSpeed"),
            Some(".58")
        );
        assert_eq!(
            ini.section("General").unwrap().get_for_test("FlightLevel"),
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
        let (_, _, owner) = load_startup_rules(&assets, Arc::default(), Default::default())
            .expect("startup")
            .into_parts();
        directory.write("langrule.ini", b"[AI]\nNeutralTechBuildings=WRONG\n");
        directory.write("artmd.ini", b"[TECH2]\nFoundation=1x1\n");
        let projected = owner
            .startup_compatibility_projection()
            .expect("read-only startup projection");
        let types = crate::map::rmg::tech_catalog::resolve(
            projected.neutral_tech_buildings(),
            owner.fixed_art(),
        );
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
    #[ignore = "slow; run when changing what it covers"]
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
            SimFixed::ONE
        );
        assert_eq!(
            rules.object("DEST").expect("retail DEST").weight,
            SimFixed::lit("3")
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
        assert_eq!(rules.c4_delay_frames(), 26);
    }
}
