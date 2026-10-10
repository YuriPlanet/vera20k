//! Read-only INI inspection through production source selection and readers.
//!
//! Accessor reports describe the retained compatibility INI projection. Type
//! reports read the final scenario RuleSet returned by that same processing
//! owner; this tool never recreates constructors, readers or post-passes.

use std::path::Path;

use serde_json::{Value, json};

use crate::asset_tools::report::ErrorReport;
use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::retail_sources::{RetailRulesSources, select_ini};
use crate::rules::ruleset::RuleSet;
use crate::util::fixed_math::SimFixed;

#[derive(Clone, Copy, Debug)]
pub enum IniQuery<'a> {
    Accessor { section: &'a str, key: &'a str },
    Type { type_id: &'a str },
}

impl IniQuery<'_> {
    fn verb(self) -> &'static str {
        match self {
            Self::Accessor { .. } => "ini-get",
            Self::Type { .. } => "ini-type",
        }
    }
}

#[derive(Debug, Default)]
pub struct IniOptions {
    pub domain: Option<String>,
    pub reader: Option<String>,
    pub default: Option<String>,
    pub capacity: Option<usize>,
    pub map: Option<String>,
    pub mode_id: Option<i32>,
}

impl IniOptions {
    pub fn validate(&self, query: IniQuery<'_>) -> Result<(), String> {
        let verb = query.verb();
        match self.domain.as_deref() {
            Some("rules") => {
                if self.map.as_deref().is_none_or(str::is_empty) || self.mode_id.is_none() {
                    return Err(format!(
                        "{verb} --domain rules requires explicit --map and --mode-id"
                    ));
                }
            }
            Some("art") => {
                if self.map.is_some() || self.mode_id.is_some() {
                    return Err(
                        "ARTMD is fixed, not scenario-layered; omit --map and --mode-id".into(),
                    );
                }
            }
            _ => return Err(format!("{verb} requires --domain rules|art")),
        }
        if let IniQuery::Type { type_id } = query {
            if self.domain.as_deref() != Some("rules") {
                return Err("ini-type requires --domain rules".into());
            }
            if type_id.is_empty() {
                return Err("ini-type requires a nonempty type identity".into());
            }
            if self.reader.is_some() || self.default.is_some() || self.capacity.is_some() {
                return Err("ini-type rejects --reader, --default and --capacity; it reads final stored type fields".into());
            }
            return Ok(());
        }
        let reader = self.reader.as_deref().ok_or("ini-get requires --reader")?;
        if !matches!(
            reader,
            "raw" | "int" | "bool" | "double" | "string" | "range" | "speed" | "coord"
        ) {
            return Err("--reader must be raw|int|bool|double|string|range|speed|coord".into());
        }
        if reader == "raw" && self.default.is_some() {
            return Err("--default is not valid for the raw reader".into());
        }
        if reader != "raw" && self.default.is_none() {
            return Err(
                "typed readers require an explicit --default from the caller contract".into(),
            );
        }
        if (reader == "string") != self.capacity.is_some() {
            return Err("--capacity is required only for the string reader".into());
        }
        // Validate CLI values before mounting retail assets. These are supplied
        // typed defaults, not INI tokens, so strict CLI parsing is intentional.
        accessor(&IniFile::empty(), "", "", self)?;
        Ok(())
    }
}

fn cli_i32(text: &str) -> Result<i32, String> {
    text.parse()
        .map_err(|_| "--default requires a signed decimal i32 for this reader".into())
}

fn float_result(value: f64) -> Value {
    json!({
        "value": if value.is_finite() { json!(value) } else { Value::Null },
        "representation": value.to_string(),
        "binary64_bits": format!("0x{:016x}", value.to_bits()),
    })
}

fn accessor(
    ini: &IniFile,
    section: &str,
    key: &str,
    options: &IniOptions,
) -> Result<Value, String> {
    let empty = IniSection::new(section.to_owned());
    let section = ini.section(section).unwrap_or(&empty);
    let default = options.default.as_deref().unwrap_or("");
    Ok(match options.reader.as_deref() {
        Some("raw") => json!(raw_value(section, key)),
        Some("int") => json!(section.read_int(key, cli_i32(default)?)),
        Some("bool") => {
            let default = default
                .parse::<bool>()
                .map_err(|_| "--default for bool must be true or false")?;
            json!(section.read_bool(key, default))
        }
        Some("double") => {
            let default = default
                .parse::<f64>()
                .map_err(|_| "--default for double must be a binary64 number")?;
            float_result(section.read_double(key, default))
        }
        Some("string") => json!(
            section.read_string(
                key,
                default,
                options
                    .capacity
                    .ok_or("string reader requires --capacity")?
            )
        ),
        Some("range") => json!(section.read_range(key, cli_i32(default)?)),
        Some("speed") => json!(section.read_speed(key, cli_i32(default)?)),
        Some("coord") => {
            let values = default
                .split(',')
                .map(cli_i32)
                .collect::<Result<Vec<_>, _>>()?;
            let values: [i32; 3] = values
                .try_into()
                .map_err(|_| "--default for coord must be three signed decimal integers: x,y,z")?;
            json!(section.read_coord3(key, values))
        }
        _ => return Err("ini-get requires a supported --reader".into()),
    })
}

/// The stored text, shown as it is: the tool's raw view, not a reader.
fn raw_value<'a>(section: &'a IniSection, key: &str) -> Option<&'a str> {
    section
        .raw_entries()
        .find_map(|(entry, value)| (entry == key).then_some(value))
}

fn presence(ini: &IniFile, section: &str, key: &str) -> Value {
    let selected = ini.section(section);
    json!({
        "section_present": selected.is_some(),
        "key_present": selected.is_some_and(|s| s.is_present(key)),
        "raw_value": selected.and_then(|s| raw_value(s, key)),
    })
}

fn layer(kind: &str, source: Value, ini: Option<&IniFile>, query: IniQuery<'_>) -> Value {
    let mut report = json!({
        "layer": kind,
        "source": source,
        "source_present": ini.is_some(),
    });
    if let IniQuery::Accessor { section, key } = query {
        report["authored"] = json!(ini.map(|ini| presence(ini, section, key)));
    }
    report
}

fn float32_result(value: f32) -> Value {
    json!({
        "value": if value.is_finite() { json!(value) } else { Value::Null },
        "representation": value.to_string(),
        "binary32_bits": format!("0x{:08x}", value.to_bits()),
    })
}

fn fixed_result(value: SimFixed) -> Value {
    json!({
        "value": value.to_num::<f64>(),
        "representation": value.to_string(),
        "storage": "SimFixed/I16F16",
        "fractional_bits": SimFixed::FRAC_NBITS,
        "raw_bits": value.to_bits(),
        "raw_bits_hex": format!("0x{:08x}", value.to_bits() as u32),
    })
}

fn assemble_type(rules: &RuleSet, type_id: &str, layers: Vec<Value>) -> Result<Value, String> {
    let object = rules
        .object(type_id)
        .ok_or_else(|| format!("type {type_id:?} is absent from the final scenario RuleSet"))?;
    let params = &object.jumpjet_params;
    Ok(json!({
        "schema_version": 1,
        "domain": "rules",
        "requested_type": type_id,
        "lookup": "case_insensitive_type_identity",
        "result_kind": "final_object_type",
        "source_layers": layers,
        "final_type": {
            "id": object.id,
            "name": object.name,
            "ui_name": object.ui_name,
            "image": object.image,
            "category": object.category,
            "locomotor": object.locomotor,
            "locomotor_clsid": object.locomotor.clsid(),
            "speed_type": object.speed_type,
            "movement_zone": object.movement_zone,
            "balloon_hover": object.balloon_hover,
            "is_simple_deployer": object.is_simple_deployer,
            "deploy_to_land": object.deploy_to_land,
            "hover_attack": object.hover_attack,
            "jumpjet": object.jumpjet,
            "crashable": object.crashable,
            "tilt_crash_jumpjet": object.tilt_crash_jumpjet,
            "jumpjet_params": {
                "turn_rate": params.turn_rate,
                "speed": fixed_result(params.speed),
                "climb": float32_result(params.climb),
                "crash": float32_result(params.crash),
                "height": params.height,
                "accel": float32_result(params.accel),
                "wobbles": float32_result(params.wobbles),
                "no_wobbles": params.no_wobbles,
                "deviation": params.deviation,
            },
        },
        "units": {"jumpjet_params.height": "leptons", "jumpjet_params.deviation": "leptons"},
        "limits": [
            "Fields come from the final Rust scenario RuleSet, including its constructors/readers/post-passes; this report alone establishes no new native equivalence.",
            "Jumpjet parameters are stored type values on every ObjectType, independent of JumpJet. Locomotor Link conversions/clamps and live flight state are not applied.",
            "Rules inspection executes the supported noncampaign skirmish path with the special TMCJ4F flag absent; campaign passes, LANGRULE Digest handling and that flagged extra pass are not modeled.",
            "Existing production parser/reader residuals remain; no replacement parser or type loader is used."
        ],
    }))
}

fn assemble(
    ini: &IniFile,
    section: &str,
    key: &str,
    options: &IniOptions,
    layers: Vec<Value>,
) -> Result<Value, String> {
    Ok(json!({
        "schema_version": 1,
        "domain": options.domain,
        "section": section,
        "key": key,
        "lookup": "exact_case",
        "reader": options.reader,
        "supplied_default": options.default,
        "string_capacity_bytes": options.capacity,
        "authored_layers": layers,
        "processed": presence(ini, section, key),
        "accessor_result": accessor(ini, section, key, options)?,
        "result_kind": "production_ini_accessor",
        "limits": [
            "An accessor result is not a final gameplay field: constructors, type allocation timing, field-specific defaults/clamps, art indirection and post-read passes may change it.",
            "Rules inspection executes the supported noncampaign skirmish path with the special TMCJ4F flag absent; campaign passes, LANGRULE Digest handling and that flagged extra pass are not modeled.",
            "Authored values reflect the production byte parser: empty values are omitted and duplicate-key resolution retains its documented stock-inert residual.",
            "Reader residuals remain: malformed ReadDouble/coord values use deterministic Rust recovery; ReadRange overflow differs from native. No new native equivalence is claimed."
        ],
    }))
}

pub fn run(
    assets: &mut AssetManager,
    retail_dir: &Path,
    query: IniQuery<'_>,
    options: &IniOptions,
) -> Result<Value, ErrorReport> {
    run_inner(assets, retail_dir, query, options).map_err(|error| ErrorReport {
        error,
        hint: Some(match query {
            IniQuery::Accessor { .. } => "Use asset ini-get --help; typed results require the native caller's default and reader.",
            IniQuery::Type { .. } => "Use asset ini-type --help; select a rules type, map and skirmish mode explicitly.",
        }.into()),
    })
}

fn run_inner(
    assets: &mut AssetManager,
    retail_dir: &Path,
    query: IniQuery<'_>,
    options: &IniOptions,
) -> Result<Value, String> {
    options.validate(query)?;
    if options.domain.as_deref() == Some("art") {
        let IniQuery::Accessor { section, key } = query else {
            unreachable!("validated art accessor")
        };
        let art = select_ini(assets, "artmd.ini")?;
        let layers = vec![layer("fixed_art", json!(art.source), Some(&art.ini), query)];
        return assemble(&art.ini, section, key, options, layers);
    }
    let audio_definitions = crate::rules::audio_sources::AudioDefinitions::select(assets);
    let sources = RetailRulesSources::select(assets)?;
    // The app retains these registrations between cold Rules selection and
    // roster/map lookup. A shell-discovered YRO may supply either later input.
    // Preserve the app's tolerant failure policy and partially registered VFS.
    if let Err(error) = assets.register_neutral_archives() {
        log::warn!("Could not register retail neutral shell archives: {error:#}");
    }
    if let Err(error) = crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
        retail_dir, assets, None,
    ) {
        log::warn!("Could not enumerate retail skirmish scenarios: {error:#}");
    }
    let roster = select_ini(assets, "MPModesMD.ini")?;
    let modes = crate::skirmish_modes::skirmish_modes_from_selected_ini(
        assets,
        &roster.ini,
        &roster.source.source_archive,
    )
    .map_err(|e| e.to_string())?;
    let id = options.mode_id.expect("validated mode id");
    let mode = crate::skirmish_modes::mode_by_id(&modes, id)
        .ok_or_else(|| format!("mode id {id} is absent from MPModesMD.ini"))?;
    if !mode.class.lists_in_skirmish() {
        return Err(format!(
            "mode id {id} is not in the supported noncampaign skirmish list"
        ));
    }
    let map = crate::map::source::load_map_by_name_or_path_with_assets(
        retail_dir,
        options.map.as_deref().expect("validated map"),
        assets,
    )
    .map_err(|e| e.to_string())?;
    // App match loading activates theater archives after selecting startup
    // sources and before resolving the mode override. Follow that route;
    // headless's older pre-theater mode lookup is not the claimed path.
    crate::map::theater::load_theater(assets, &map.map.header.theater).ok_or_else(|| {
        format!(
            "load theater {} for app rule-source selection",
            map.map.header.theater
        )
    })?;
    let selected_mode = if mode.override_file.trim().is_empty() {
        None
    } else {
        Some(select_ini(assets, mode.override_file.trim())?)
    };
    let layers = vec![
        layer(
            "rulesmd",
            json!(sources.rulesmd.source),
            Some(&sources.rulesmd.ini),
            query,
        ),
        layer(
            "langrule",
            sources
                .langrule
                .as_ref()
                .map_or(Value::Null, |ini| json!(ini.source)),
            sources.langrule.as_ref().map(|ini| &ini.ini),
            query,
        ),
        layer(
            "game_mode",
            selected_mode
                .as_ref()
                .map_or(Value::Null, |ini| json!(ini.source)),
            selected_mode.as_ref().map(|ini| &ini.ini),
            query,
        ),
        layer("scenario", json!(map.source), Some(&map.map.ini), query),
    ];
    // Retain provenance before moving the selected snapshots into their owner.
    let fixed_art_source = json!(sources.artmd.source);
    let fixed_sound_source = audio_definitions.sound_source().map(|source| json!(source));
    let (_, _, mut owner) = sources
        .into_startup(std::sync::Arc::clone(audio_definitions.sounds()))?
        .into_parts();
    let (rules, processed, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
                selected_mode.as_ref().map(|ini| &ini.ini),
            ),
            &map.map.ini,
        )
        .map_err(|e| e.to_string())?
        .into_parts();
    let mut report = match query {
        IniQuery::Accessor { section, key } => assemble(&processed, section, key, options, layers)?,
        IniQuery::Type { type_id } => assemble_type(&rules, type_id, layers)?,
    };
    report["mode_roster_source"] = json!(roster.source);
    report["mode"] = json!({"id": mode.id, "class": format!("{:?}", mode.class), "override_file": mode.override_file});
    report["source_selection_route"] =
        json!("app_noncampaign_startup_then_shell_registration_then_map_then_theater_then_mode");
    report["fixed_art_source"] = fixed_art_source;
    report["fixed_sound_source"] = json!(fixed_sound_source);
    if matches!(query, IniQuery::Type { .. }) {
        report["scenario"] = json!({
            "requested_map": options.map,
            "name": map.map.basic.name,
            "theater": map.map.header.theater,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::process_owner::NativeRulesProcessOwner;

    fn options(reader: &str, default: &str) -> IniOptions {
        IniOptions {
            domain: Some("art".into()),
            reader: Some(reader.into()),
            default: (reader != "raw").then(|| default.into()),
            ..Default::default()
        }
    }

    #[test]
    fn absent_sections_still_use_native_string_default_capacity_and_trim() {
        let mut query = options("string", "  default  ");
        query.capacity = Some(6);
        assert_eq!(
            accessor(&IniFile::empty(), "missing", "key", &query).unwrap(),
            "def"
        );
    }

    #[test]
    fn exact_case_and_omitted_empty_values_remain_visible() {
        let ini = IniFile::from_str("[Case]\nKey=42\nEmpty=\n");
        assert_eq!(presence(&ini, "Case", "key")["key_present"], false);
        assert_eq!(presence(&ini, "Case", "Key")["raw_value"], "42");
        assert_eq!(presence(&ini, "Case", "Empty")["key_present"], false);
        assert_eq!(
            accessor(&ini, "Case", "Empty", &options("int", "73")).unwrap(),
            73
        );
    }

    #[test]
    fn nonfinite_double_keeps_bits_in_valid_json() {
        let result = accessor(
            &IniFile::from_str("[S]\nK=1e39%\n"),
            "S",
            "K",
            &options("double", "0"),
        )
        .unwrap();
        assert_eq!(result["value"], Value::Null);
        assert_eq!(result["binary64_bits"], "0x7ff0000000000000");
        serde_json::from_str::<Value>(&serde_json::to_string(&result).unwrap()).unwrap();
    }

    #[test]
    fn accessor_keeps_layered_default_retention() {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str("[General]\nTreeStrength=42\n"),
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
        )
        .unwrap();
        let (_, processed, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::from_str("[General]\nTreeStrength=$xyz\n"),
            )
            .unwrap()
            .into_parts();
        assert_eq!(
            accessor(&processed, "General", "TreeStrength", &options("int", "7")).unwrap(),
            42
        );
        assert_eq!(
            presence(&processed, "General", "TreeStrength")["raw_value"],
            "$xyz"
        );
    }

    #[test]
    fn raw_weapon_speed_is_not_claimed_as_final_postpass_field() {
        // Same production reader/postpass as rules::weapon_speed_tests; no
        // replacement implementation of the projectile-dependent calculation.
        let root = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nSpeed=40\nRange=5\nProjectile=SHOT\n[SHOT]\nROT=0\n",
        );
        let authored = presence(&root, "GUN", "Speed");
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
        )
        .unwrap();
        let (rules, processed, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap()
            .into_parts();
        let report = assemble(&processed, "GUN", "Speed", &options("raw", ""), vec![]).unwrap();
        assert_eq!(authored["raw_value"], "40");
        assert_eq!(report["accessor_result"], "40");
        assert_ne!(rules.weapon("GUN").unwrap().speed, 40);
        assert_eq!(report["result_kind"], "production_ini_accessor");
        assert!(
            report["limits"][0]
                .as_str()
                .unwrap()
                .contains("not a final gameplay field")
        );
    }

    #[test]
    fn final_type_report_uses_layered_object_fields_and_full_jumpjet_block() {
        let root = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nName=Fixture unit\n\
             Locomotor={92612C46-F71F-11D1-AC9F-006008055BB5}\n\
             BalloonHover=yes\nIsSimpleDeployer=yes\nDeployToLand=no\nHoverAttack=yes\n\
             JumpjetTurnRate=7\nJumpjetSpeed=27\nJumpjetClimb=3.125\nJumpjetCrash=9.5\n\
             JumpjetHeight=600\nJumpjetAccel=1.25\nJumpjetWobbles=0.25\n\
             JumpjetNoWobbles=yes\nJumpjetDeviation=42\n",
        );
        let mode =
            IniFile::from_str("[UNIT]\nBalloonHover=no\nDeployToLand=yes\nJumpjetHeight=650\n");
        let map = IniFile::from_str(
            "[UNIT]\nIsSimpleDeployer=no\nJumpjetClimb=6.5\nJumpjetSpeed=29.75\n\
             JumpjetWobbles=-0.0\nJumpJetTurnRate=99\nJumpJetAccel=99\n",
        );
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
        )
        .unwrap();
        let (rules, processed, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
                &map,
            )
            .unwrap()
            .into_parts();
        let source = json!({"logical_name": "fixture.map", "source_sha256": "fixture identity"});
        let report = assemble_type(
            &rules,
            "unit",
            vec![layer(
                "scenario",
                source.clone(),
                Some(&map),
                IniQuery::Type { type_id: "unit" },
            )],
        )
        .unwrap();
        assert_eq!(report["result_kind"], "final_object_type");
        assert_eq!(report["requested_type"], "unit");
        assert_eq!(report["source_layers"][0]["source"], source);
        assert!(report["source_layers"][0].get("authored").is_none());
        let object = &report["final_type"];
        assert_eq!(object["id"], "UNIT");
        assert_eq!(object["category"], "Vehicle");
        assert_eq!(object["locomotor"], "Jumpjet");
        assert_eq!(object["balloon_hover"], false);
        assert_eq!(object["is_simple_deployer"], false);
        assert_eq!(object["deploy_to_land"], true);
        assert_eq!(object["hover_attack"], true);
        assert_eq!(object["jumpjet"], false);
        let params = &object["jumpjet_params"];
        assert_eq!(params.as_object().unwrap().len(), 9);
        assert_eq!(params["turn_rate"], 7);
        assert_eq!(params["speed"]["value"], 29.0);
        assert_eq!(params["speed"]["raw_bits"], 29 * 65536);
        assert_eq!(params["speed"]["raw_bits_hex"], "0x001d0000");
        assert_eq!(params["speed"]["fractional_bits"], 16);
        assert_eq!(params["climb"]["value"], 6.5);
        assert_eq!(params["climb"]["binary32_bits"], "0x40d00000");
        assert_eq!(params["crash"]["value"], 9.5);
        assert_eq!(params["height"], 650);
        assert_eq!(params["accel"]["value"], 1.25);
        assert_eq!(params["wobbles"]["binary32_bits"], "0x80000000");
        assert_eq!(params["no_wobbles"], true);
        assert_eq!(params["deviation"], 42);
        // The wrong-case authored tokens remain visible to ini-get, but are
        // not substituted for the values returned by the type reader.
        assert_eq!(
            presence(&processed, "UNIT", "JumpJetAccel")["raw_value"],
            "99"
        );
        serde_json::from_str::<Value>(&serde_json::to_string(&report).unwrap()).unwrap();
    }

    #[test]
    fn final_type_report_rejects_an_ini_section_without_a_registered_type() {
        let root = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nStrength=100\n[UNREGISTERED]\nBalloonHover=yes\n",
        );
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root,
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
        )
        .unwrap();
        let (rules, processed, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap()
            .into_parts();
        assert_eq!(
            presence(&processed, "UNREGISTERED", "BalloonHover")["raw_value"],
            "yes"
        );
        assert!(
            assemble_type(&rules, "UNREGISTERED", vec![])
                .unwrap_err()
                .contains("absent from the final scenario RuleSet")
        );
        let report = assemble_type(&rules, "UNIT", vec![]).unwrap();
        assert_eq!(report["final_type"]["balloon_hover"], false);
        assert_eq!(report["final_type"]["jumpjet_params"]["turn_rate"], 4);
        assert_eq!(
            report["final_type"]["jumpjet_params"]["speed"]["value"],
            14.0
        );
        assert_eq!(
            report["final_type"]["jumpjet_params"]["accel"]["value"],
            2.0
        );
    }

    #[test]
    fn final_type_float_report_keeps_nonfinite_payload_bits_in_valid_json() {
        let result = float32_result(f32::from_bits(0x7fc01234));
        assert_eq!(result["value"], Value::Null);
        assert_eq!(result["binary32_bits"], "0x7fc01234");
        serde_json::from_str::<Value>(&serde_json::to_string(&result).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "requires retail YR installation containing RiverRam.yro; run explicitly"]
    fn retail_yro_scenario_inspection_uses_retained_discovery() {
        let retail = std::path::PathBuf::from(std::env::var("RA2_DIR").expect("set RA2_DIR"));
        let mut assets =
            crate::asset_tools::root::open_manager(&retail, false).expect("retail manager");
        let query = IniOptions {
            domain: Some("rules".into()),
            reader: Some("speed".into()),
            default: Some("0".into()),
            map: Some("RiverRam.MAP".into()),
            mode_id: Some(1),
            ..Default::default()
        };
        let report = run_inner(
            &mut assets,
            &retail,
            IniQuery::Accessor {
                section: "HoverMissile",
                key: "Speed",
            },
            &query,
        )
        .expect("inspect map registered by production scenario discovery");
        assert_eq!(report["accessor_result"], 102);
        let source = &report["authored_layers"][3]["source"];
        assert_eq!(source["kind"], "mix");
        assert!(
            source["source_archive"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case("RiverRam.yro")
        );
        let selected = assets
            .resolve_ref("RiverRam.MAP")
            .expect("retained scenario archive");
        assert_eq!(
            source["source_sha256"],
            crate::util::sha256::sha256_hex(selected.bytes)
        );
    }
}
