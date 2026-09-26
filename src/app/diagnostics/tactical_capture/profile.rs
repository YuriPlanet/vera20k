//! Strict profile, contract, filesystem, and hashing boundary for tactical capture.
//!
//! Profiles own every fixed Battle input and diagnostic budget. This module
//! translates that sealed data into ordinary app launch types; it never seeds
//! a map or mutates simulation state.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
    SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession, SkirmishLocalSlot,
};

pub(crate) use super::integrity::{
    SealedJsonFile, parse_strict_json, read_stable_regular_bytes, sha256_file, sha256_hex,
    validate_new_output_directory,
};

pub(crate) const PROFILE_SCHEMA: &str = "vera20k.tactical-profile.v2";
pub(crate) const CONTRACT_SCHEMA: &str = "vera20k.tactical-capture-contract.v2";
pub(crate) const CHECKPOINT_RADAR_ONLINE_V2: &str = "radar-online-v2";
pub(crate) const EMBEDDED_CONTRACT: &str = include_str!("contract.v2.json");
pub(crate) const ABSOLUTE_TIMEOUT_MAX_SECONDS: u32 = 900;
pub(crate) const FRAME_FILE_NAME: &str = "frame.bgra";
pub(crate) const MANIFEST_FILE_NAME: &str = "capture.json";

const EXPECTED_ARCHIVE_SHA256: &str =
    "ff4138ba95f7efd8bded14342fc9082b99c47e43c25ab18236e4eea141b488e9";
const EXPECTED_ENTRY_SHA256: &str =
    "d751dce7cd3611077e9228c33235f39c71681fff6ac08ca1f716d963ad6ce070";
const EXPECTED_FONT_SHA256: &str =
    "6a8481fe107ee547893c018b13dba291c2020bec3de5da6525d9ac09f6bc2105";

const ENVIRONMENT_DENYLIST: [&str; 17] = [
    "RA2_QUICKPLAY",
    "RA2_DEV_SKIRMISH_SHELL",
    "RA2_DEBUG_SPAWN_UNITS",
    "RA2_DISABLE_LAT",
    "RA2_ENABLE_LAT",
    "RA2_DEBUG_CAMEO_PALETTES",
    "RA2_DEBUG_BRIDGE_RENDER_BUCKETS",
    "RA2_FORCE_TIB3_TO_TIB01",
    "RA2_TIB_ID_OFFSET",
    "RA2_FORCE_TIB_IMAGE",
    "RA2_DEBUG_MOUSE_CURSOR_SHEET",
    "RA2_NORMAL_COUNT",
    "RA2_NORMALS",
    "RA2_QUEUE_FRAME_MS",
    "RA2_DIR",
    "RA2_DUMP_SHROUD",
    "RA2_DEBUG_DEPTH_VIEW",
];

const STAGE_NAMES: [&str; 9] = [
    "yard_active",
    "power_ready",
    "power_active",
    "refinery_ready",
    "refinery_active",
    "radar_ready",
    "radar_active",
    "radar_online",
    "readiness_and_warm_frames",
];
const STAGE_TICK_CAPS: [u32; 9] = [48, 640, 64, 2048, 64, 1024, 64, 4096, 18];
const STAGE_WALL_CAPS: [u32; 9] = [15, 90, 15, 270, 15, 140, 15, 20, 10];

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalCaptureProfile {
    pub(crate) schema_version: String,
    pub(crate) profile_id: String,
    pub(crate) checkpoint: String,
    pub(crate) fixture: TacticalFixture,
    pub(crate) launch: TacticalLaunch,
    pub(crate) capture: TacticalCaptureSettings,
    pub(crate) budgets: TacticalBudgets,
    pub(crate) pixel_inputs: TacticalPixelInputs,
    pub(crate) evidence_limitations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalFixture {
    pub(crate) logical_map_name: String,
    pub(crate) theater: String,
    pub(crate) map_size: Dimensions,
    pub(crate) local_size: Dimensions,
    pub(crate) start_waypoint_count: u32,
    pub(crate) archive_name: String,
    pub(crate) archive_byte_length: u64,
    pub(crate) archive_sha256: String,
    pub(crate) mix_entry_id: u32,
    pub(crate) entry_payload_byte_length: u64,
    pub(crate) entry_payload_sha256: String,
    pub(crate) entry_digest_authority: String,
    pub(crate) battle_descriptor_id: i32,
    pub(crate) catalog_scen_index: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Dimensions {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalLaunch {
    pub(crate) mode: TacticalMode,
    pub(crate) seed: u32,
    pub(crate) input_delay_ticks: u32,
    pub(crate) player_name: String,
    pub(crate) local: TacticalSlot,
    pub(crate) opponents: Vec<TacticalAiSlot>,
    pub(crate) options: TacticalOptions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalMode {
    pub(crate) id: i32,
    pub(crate) ui_name_key: String,
    pub(crate) tooltip_key: String,
    pub(crate) override_file: String,
    pub(crate) map_filter: String,
    pub(crate) random_maps_allowed: bool,
    pub(crate) allies_allowed: bool,
    pub(crate) must_ally: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum TacticalCountry {
    Russia,
    Yuri,
}

impl TacticalCountry {
    pub(crate) const fn launch_country(self) -> LaunchCountry {
        match self {
            Self::Russia => LaunchCountry::Russia,
            Self::Yuri => LaunchCountry::Yuri,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalSlot {
    pub(crate) country: TacticalCountry,
    pub(crate) country_random: bool,
    pub(crate) color_index: u8,
    pub(crate) color_random: bool,
    pub(crate) start_position: u8,
    pub(crate) team: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalAiSlot {
    pub(crate) country: TacticalCountry,
    pub(crate) country_random: bool,
    pub(crate) color_index: u8,
    pub(crate) color_random: bool,
    pub(crate) start_position: u8,
    pub(crate) team: Option<u8>,
    pub(crate) difficulty: TacticalAiDifficulty,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum TacticalAiDifficulty {
    Easy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalOptions {
    pub(crate) starting_credits: i32,
    pub(crate) unit_count: i32,
    pub(crate) tech_level: i32,
    pub(crate) game_speed: i32,
    pub(crate) default_ai_difficulty: i32,
    pub(crate) short_game: bool,
    pub(crate) bases: bool,
    pub(crate) bridges_destroyable: bool,
    pub(crate) super_weapons: bool,
    pub(crate) build_off_ally: bool,
    pub(crate) crates: bool,
    pub(crate) mcv_redeploy: bool,
    pub(crate) fog_of_war: bool,
    pub(crate) shroud: bool,
    pub(crate) tiberium_grows: bool,
    pub(crate) multi_engineer: bool,
    pub(crate) harvester_truce: bool,
    pub(crate) ally_change_allowed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalCaptureSettings {
    pub(crate) internal_width: u32,
    pub(crate) internal_height: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) surface_formats: Vec<String>,
    pub(crate) vsync: bool,
    pub(crate) upscale: bool,
    pub(crate) extra_animations: bool,
    pub(crate) exact_step_hz: u32,
    pub(crate) sim_tick_ms: u32,
    pub(crate) app_ui_scale: f64,
    pub(crate) post_load_cursor: CursorPoint,
    pub(crate) cursor_id: String,
    pub(crate) software_cursor_required: bool,
    pub(crate) placement_radius: u32,
    pub(crate) warm_frames: u32,
    pub(crate) build_targets: BuildTargets,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CursorPoint {
    pub(crate) x: u32,
    pub(crate) y: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BuildTargets {
    pub(crate) power: String,
    pub(crate) refinery: String,
    pub(crate) radar: String,
    pub(crate) refinery_spawned_harvester: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalBudgets {
    pub(crate) stages: Vec<StageBudget>,
    pub(crate) overall_tick_cap: u32,
    pub(crate) post_l0_timeout_seconds: u32,
    pub(crate) child_timeout_seconds: u32,
    pub(crate) absolute_timeout_max_seconds: u32,
    pub(crate) expected_ledger: ExpectedLedger,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageBudget {
    pub(crate) name: String,
    pub(crate) tick_cap: u32,
    pub(crate) wall_seconds: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectedLedger {
    pub(crate) yard_active: u64,
    pub(crate) power_ready: u64,
    pub(crate) power_active: u64,
    pub(crate) refinery_ready: u64,
    pub(crate) refinery_active: u64,
    pub(crate) radar_ready: u64,
    pub(crate) radar_active: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalPixelInputs {
    pub(crate) font: AbsolutePixelFile,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AbsolutePixelFile {
    pub(crate) path: PathBuf,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TacticalCaptureContract {
    pub(crate) schema_version: String,
    pub(crate) absolute_max_child_timeout_seconds: u32,
    pub(crate) environment_denylist: Vec<String>,
}

fn require_lower_sha256(value: &str, label: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{label} SHA-256 must be 64 lowercase hexadecimal characters"
    );
    Ok(())
}

impl TacticalCaptureProfile {
    pub(crate) fn load_strict(path: &Path) -> Result<SealedJsonFile<Self>> {
        let (bytes, digest) = read_stable_regular_bytes(path, "tactical profile")?;
        let document: serde_json::Value = parse_strict_json(&bytes, "tactical profile")?;
        ensure!(
            document["schema_version"] == PROFILE_SCHEMA,
            "unsupported profile schema; current capture requires tactical-profile.v2 (native 1x sidebar); retain v1 only as historical evidence"
        );
        let value: Self = serde_json::from_value(document)?;
        value.validate()?;
        Ok(SealedJsonFile {
            path: path.to_path_buf(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
            bytes,
            value,
        })
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == PROFILE_SCHEMA,
            "unsupported profile schema; current capture requires tactical-profile.v2 (native 1x sidebar); retain v1 only as historical evidence"
        );
        ensure!(
            self.checkpoint == CHECKPOINT_RADAR_ONLINE_V2,
            "unsupported tactical checkpoint {:?}",
            self.checkpoint
        );
        ensure!(
            matches!(
                self.profile_id.as_str(),
                "soviet-radar-online-v2" | "yuri-radar-online-v2"
            ),
            "unsupported tactical profile {:?}",
            self.profile_id
        );
        self.validate_fixture()?;
        self.validate_launch()?;
        self.validate_capture()?;
        self.validate_budgets()?;
        self.validate_pixel_inputs()?;
        ensure!(
            !self.evidence_limitations.is_empty()
                && self
                    .evidence_limitations
                    .iter()
                    .all(|value| !value.is_empty()),
            "evidence_limitations must contain nonempty strings"
        );
        Ok(())
    }

    fn is_soviet(&self) -> bool {
        self.profile_id == "soviet-radar-online-v2"
    }

    fn validate_fixture(&self) -> Result<()> {
        let fixture = &self.fixture;
        ensure!(fixture.logical_map_name == "Fight.MAP", "wrong fixture map");
        ensure!(fixture.theater == "NEWURBAN", "wrong fixture theater");
        ensure!(
            fixture.map_size.width == 81 && fixture.map_size.height == 52,
            "wrong fixture map size"
        );
        ensure!(
            fixture.local_size.width == 75 && fixture.local_size.height == 42,
            "wrong fixture local size"
        );
        ensure!(fixture.start_waypoint_count == 2, "wrong waypoint count");
        ensure!(fixture.archive_name == "multimd.mix", "wrong archive name");
        ensure!(
            fixture.archive_byte_length == 31_264_268
                && fixture.archive_sha256 == EXPECTED_ARCHIVE_SHA256,
            "wrong archive identity"
        );
        ensure!(fixture.mix_entry_id == 0x9306_F050, "wrong MIX entry ID");
        ensure!(
            fixture.entry_payload_byte_length == 91_254
                && fixture.entry_payload_sha256 == EXPECTED_ENTRY_SHA256,
            "wrong entry payload identity"
        );
        ensure!(
            fixture.entry_digest_authority == "DECLARED_FIXTURE_PROVENANCE",
            "entry digest authority must stay declared provenance"
        );
        ensure!(
            fixture.battle_descriptor_id == 1 && fixture.catalog_scen_index == 12,
            "wrong Battle/catalog provenance"
        );
        require_lower_sha256(&fixture.archive_sha256, "fixture archive")?;
        require_lower_sha256(&fixture.entry_payload_sha256, "fixture entry")?;
        Ok(())
    }

    fn validate_launch(&self) -> Result<()> {
        let launch = &self.launch;
        let mode = &launch.mode;
        ensure!(
            mode.id == 1
                && mode.ui_name_key == "GUI:Battle"
                && mode.tooltip_key == "STT:ModeBattle"
                && mode.override_file == "MPBattleMD.ini"
                && mode.map_filter == "standard"
                && mode.random_maps_allowed
                && mode.allies_allowed
                && !mode.must_ally,
            "launch mode is not the fixed stock Battle descriptor"
        );
        ensure!(launch.seed == 0x1234_5678, "wrong controlled seed");
        ensure!(launch.input_delay_ticks == 2, "wrong input delay");
        let (name, local, ai) = if self.is_soviet() {
            (
                "VERA-SOVIET",
                TacticalCountry::Russia,
                TacticalCountry::Yuri,
            )
        } else {
            ("VERA-YURI", TacticalCountry::Yuri, TacticalCountry::Russia)
        };
        ensure!(
            launch.player_name == name,
            "wrong profile-owned player name"
        );
        validate_slot(&launch.local, local, 0, 0)?;
        ensure!(
            launch.opponents.len() == 1,
            "profile must contain exactly one active opponent"
        );
        validate_ai_slot(&launch.opponents[0], ai, 1, 1)?;
        validate_options(&launch.options)?;
        Ok(())
    }

    fn validate_capture(&self) -> Result<()> {
        let capture = &self.capture;
        ensure!(
            capture.internal_width == 800
                && capture.internal_height == 600
                && capture.output_width == 800
                && capture.output_height == 600,
            "v2 supports only an 800x600 internal/final surface"
        );
        ensure!(
            capture.surface_formats.len() == 2
                && capture.surface_formats[0] == "Bgra8Unorm"
                && capture.surface_formats[1] == "Bgra8UnormSrgb",
            "unsupported BGRA8 surface format set"
        );
        ensure!(
            capture.vsync && !capture.upscale && capture.extra_animations,
            "capture graphics options differ"
        );
        ensure!(
            capture.exact_step_hz == 45 && capture.sim_tick_ms == 22,
            "capture exact-step convention differs"
        );
        ensure!(
            capture.app_ui_scale == 1.0,
            "current native sidebar requires app UI scale 1.0; half-scale profiles are historical"
        );
        ensure!(
            capture.post_load_cursor.x == 316 && capture.post_load_cursor.y == 284,
            "post-load cursor must be tactical-interior center"
        );
        ensure!(
            capture.cursor_id == "Default" && capture.software_cursor_required,
            "neutral software cursor contract differs"
        );
        ensure!(
            capture.placement_radius == 16 && capture.warm_frames == 16,
            "placement/warm-frame contract differs"
        );
        let expected = if self.is_soviet() {
            ("NAPOWR", "NAREFN", "NARADR")
        } else {
            ("YAPOWR", "YAREFN", "NAPSIS")
        };
        ensure!(
            capture.build_targets.power == expected.0
                && capture.build_targets.refinery == expected.1
                && capture.build_targets.radar == expected.2,
            "profile build targets differ"
        );
        ensure!(
            capture.build_targets.refinery_spawned_harvester.as_deref()
                == self.is_soviet().then_some("HARV"),
            "profile refinery-spawned harvester expectation differs"
        );
        Ok(())
    }

    fn validate_budgets(&self) -> Result<()> {
        ensure!(
            self.budgets.stages.len() == STAGE_NAMES.len(),
            "profile must contain nine stage budgets"
        );
        for (index, stage) in self.budgets.stages.iter().enumerate() {
            ensure!(
                stage.name == STAGE_NAMES[index]
                    && stage.tick_cap == STAGE_TICK_CAPS[index]
                    && stage.wall_seconds == STAGE_WALL_CAPS[index],
                "stage budget {index} differs from tactical v2"
            );
        }
        ensure!(
            self.budgets.overall_tick_cap == 8192
                && self.budgets.post_l0_timeout_seconds == 600
                && self.budgets.child_timeout_seconds == 720
                && self.budgets.absolute_timeout_max_seconds == ABSOLUTE_TIMEOUT_MAX_SECONDS,
            "overall tactical budgets differ"
        );
        // Current Rust production timing: factory enqueue is observed at N+1,
        // first progress at N+2, then 53 intervals at the resolved rate. A
        // placed building completes one tick after its place command plus its
        // type's build-up on the human placement route
        // (`sim::building_construction`), so the sides differ.
        // Radar availability can begin during buildup, but its rendered Online
        // transition uses wall time. Only deterministic construction milestones
        // belong in this fixed ledger; the script records bounded readiness.
        let ledger = &self.budgets.expected_ledger;
        let expected = if self.is_soviet() {
            [34, 619, 672, 2635, 2688, 3644, 3676]
        } else {
            [30, 615, 662, 2625, 2676, 3632, 3681]
        };
        ensure!(
            [
                ledger.yard_active,
                ledger.power_ready,
                ledger.power_active,
                ledger.refinery_ready,
                ledger.refinery_active,
                ledger.radar_ready,
                ledger.radar_active,
            ] == expected,
            "expected current-production ledger differs"
        );
        Ok(())
    }

    fn validate_pixel_inputs(&self) -> Result<()> {
        let font = &self.pixel_inputs.font;
        ensure!(
            font.path == Path::new(r"C:\Windows\Fonts\verdana.ttf"),
            "font path differs from the sealed tactical v2 pixel input"
        );
        ensure!(
            font.byte_length == 243_304 && font.sha256 == EXPECTED_FONT_SHA256,
            "font identity differs"
        );
        require_lower_sha256(&font.sha256, "font")?;
        Ok(())
    }

    pub(crate) fn launch_session(&self) -> SkirmishLaunchSession {
        let mode = &self.launch.mode;
        SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                id: mode.id,
                ui_name_key: mode.ui_name_key.clone(),
                tooltip_key: mode.tooltip_key.clone(),
                override_file: mode.override_file.clone(),
                map_filter: mode.map_filter.clone(),
                random_maps_allowed: mode.random_maps_allowed,
                allies_allowed: mode.allies_allowed,
                must_ally: mode.must_ally,
            },
            selected_map_file: Some(self.fixture.logical_map_name.clone()),
            player_name: self.launch.player_name.clone(),
            local: SkirmishLocalSlot {
                country: self.launch.local.country.launch_country(),
                country_random: self.launch.local.country_random,
                color_index: self.launch.local.color_index,
                color_random: self.launch.local.color_random,
                start_position: LaunchStartPosition::Position(self.launch.local.start_position),
                team: launch_team(self.launch.local.team),
            },
            opponents: self
                .launch
                .opponents
                .iter()
                .map(|slot| SkirmishAiSlot {
                    country: slot.country.launch_country(),
                    country_random: slot.country_random,
                    color_index: slot.color_index,
                    color_random: slot.color_random,
                    start_position: LaunchStartPosition::Position(slot.start_position),
                    team: launch_team(slot.team),
                    difficulty: AiDifficulty::Easy,
                })
                .collect(),
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(
                    self.launch.opponents.len(),
                ),
            options: self.launch.options.to_launch_options(),
        }
    }
}

impl TacticalOptions {
    pub(crate) fn to_launch_options(&self) -> SkirmishLaunchOptions {
        SkirmishLaunchOptions {
            starting_credits: self.starting_credits,
            unit_count: self.unit_count,
            tech_level: self.tech_level,
            game_speed: self.game_speed,
            default_ai_difficulty: self.default_ai_difficulty,
            short_game: self.short_game,
            bases: self.bases,
            bridges_destroyable: self.bridges_destroyable,
            super_weapons: self.super_weapons,
            build_off_ally: self.build_off_ally,
            crates: self.crates,
            mcv_redeploy: self.mcv_redeploy,
            fog_of_war: self.fog_of_war,
            shroud: self.shroud,
            tiberium_grows: self.tiberium_grows,
            multi_engineer: self.multi_engineer,
            harvester_truce: self.harvester_truce,
            ally_change_allowed: self.ally_change_allowed,
        }
    }
}

impl TacticalCaptureContract {
    pub(crate) fn load_external(path: &Path) -> Result<SealedJsonFile<Self>> {
        let (bytes, digest) = read_stable_regular_bytes(path, "tactical contract")?;
        ensure!(
            bytes == EMBEDDED_CONTRACT.as_bytes(),
            "external tactical contract bytes differ from embedded executable contract"
        );
        let value: Self = parse_strict_json(&bytes, "tactical contract")?;
        value.validate()?;
        Ok(SealedJsonFile {
            path: path.to_path_buf(),
            byte_length: digest.byte_length,
            sha256: digest.sha256,
            bytes,
            value,
        })
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == CONTRACT_SCHEMA,
            "wrong contract schema"
        );
        ensure!(
            self.absolute_max_child_timeout_seconds == ABSOLUTE_TIMEOUT_MAX_SECONDS,
            "wrong absolute tactical timeout maximum"
        );
        ensure!(
            self.environment_denylist.as_slice()
                == ENVIRONMENT_DENYLIST.map(str::to_owned).as_slice(),
            "environment denylist differs from tactical v2"
        );
        let unique: BTreeSet<&str> = self
            .environment_denylist
            .iter()
            .map(String::as_str)
            .collect();
        ensure!(
            unique.len() == self.environment_denylist.len(),
            "environment denylist contains duplicates"
        );
        Ok(())
    }

    pub(crate) fn validate_environment(&self) -> Result<()> {
        let present: Vec<&str> = self
            .environment_denylist
            .iter()
            .map(String::as_str)
            .filter(|name| std::env::var_os(name).is_some())
            .collect();
        ensure!(
            present.is_empty(),
            "tactical capture environment contains denied overrides: {present:?}"
        );
        Ok(())
    }
}

fn launch_team(team: Option<u8>) -> LaunchTeam {
    match team {
        Some(team) => LaunchTeam::Team(team),
        None => LaunchTeam::None,
    }
}

fn validate_slot(
    slot: &TacticalSlot,
    country: TacticalCountry,
    color: u8,
    start: u8,
) -> Result<()> {
    ensure!(
        slot.country == country
            && !slot.country_random
            && slot.color_index == color
            && !slot.color_random
            && slot.start_position == start
            && slot.team.is_none(),
        "local slot differs from the fixed profile"
    );
    Ok(())
}

fn validate_ai_slot(
    slot: &TacticalAiSlot,
    country: TacticalCountry,
    color: u8,
    start: u8,
) -> Result<()> {
    ensure!(
        slot.country == country
            && !slot.country_random
            && slot.color_index == color
            && !slot.color_random
            && slot.start_position == start
            && slot.team.is_none()
            && slot.difficulty == TacticalAiDifficulty::Easy,
        "AI slot differs from the fixed profile"
    );
    Ok(())
}

fn validate_options(options: &TacticalOptions) -> Result<()> {
    ensure!(
        options.starting_credits == 10_000
            && options.unit_count == 0
            && options.tech_level == 10
            && options.game_speed == 1
            && options.default_ai_difficulty == 0
            && options.short_game
            && options.bases
            && options.bridges_destroyable
            && options.super_weapons
            && options.build_off_ally
            && options.crates
            && options.mcv_redeploy
            && !options.fog_of_war
            && options.shroud
            && options.tiberium_grows
            && !options.multi_engineer
            && !options.harvester_truce
            && options.ally_change_allowed,
        "launch options differ from tactical v2"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn strict_json_rejects_duplicates_and_boolean_integer_substitution() {
        let duplicate = br#"{"schema_version":"a","schema_version":"b"}"#;
        assert!(parse_strict_json::<TacticalCaptureContract>(duplicate, "test").is_err());

        let boolean_timeout = br#"{
            "schema_version":"vera20k.tactical-capture-contract.v2",
            "absolute_max_child_timeout_seconds":true,
            "environment_denylist":[]
        }"#;
        assert!(parse_strict_json::<TacticalCaptureContract>(boolean_timeout, "test").is_err());
    }

    #[test]
    fn tracked_profiles_build_classifier_valid_fixed_battle_sessions() {
        use crate::match_bootstrap::{StartupSessionClassification, classify_startup_session};

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for (file_name, expected_country, expected_harvester) in [
            (
                "soviet-radar-online-v2.json",
                LaunchCountry::Russia,
                Some("HARV"),
            ),
            ("yuri-radar-online-v2.json", LaunchCountry::Yuri, None),
        ] {
            let path = root
                .join("tools/tactical_certification/profiles")
                .join(file_name);
            let sealed = TacticalCaptureProfile::load_strict(&path).expect("profile");
            let session = sealed.value.launch_session();
            assert_eq!(session.local.country, expected_country);
            assert_eq!(session.options.unit_count, 0);
            assert_eq!(session.options.starting_credits, 10_000);
            assert_eq!(session.opponents.len(), 1);
            assert_eq!(
                sealed
                    .value
                    .capture
                    .build_targets
                    .refinery_spawned_harvester
                    .as_deref(),
                expected_harvester
            );
            assert!(matches!(
                classify_startup_session(&session),
                StartupSessionClassification::AcceptedExplicitFixedBattle(_)
            ));
        }
    }

    #[test]
    fn current_profiles_match_compiled_native_geometry_and_reject_old_scale() {
        use crate::sidebar::{
            SidebarChromeLayoutSpec, SidebarTheme, compute_layout_with_spec,
            radar_minimap_rect_with_spec,
        };
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for (side, theme) in [
            ("soviet", SidebarTheme::Soviet),
            ("yuri", SidebarTheme::Yuri),
        ] {
            let directory = root.join("tools/tactical_certification/profiles");
            let old = TacticalCaptureProfile::load_strict(
                &directory.join(format!("{side}-radar-online-v1.json")),
            )
            .unwrap_err();
            assert!(old.to_string().contains("v1 only as historical"));
            let mut profile = TacticalCaptureProfile::load_strict(
                &directory.join(format!("{side}-radar-online-v2.json")),
            )
            .unwrap()
            .value;
            assert_eq!(profile.capture.app_ui_scale, 1.0);
            let spec = SidebarChromeLayoutSpec::for_theme(theme);
            let layout = compute_layout_with_spec(spec, 800.0, 600.0, 0);
            assert_eq!(
                (
                    layout.sidebar_x,
                    layout.side1_y,
                    layout.cameo_grid_top,
                    layout.side3_y,
                    layout.side2_tile_count
                ),
                (632.0, 158.0, 227.0, 527.0, 6)
            );
            let aperture = radar_minimap_rect_with_spec(800.0, spec);
            assert_eq!(
                (aperture.x, aperture.y, aperture.w, aperture.h),
                (648.0, 49.0, 140.0, 108.0)
            );
            profile.capture.app_ui_scale = 0.5;
            assert!(
                profile
                    .validate()
                    .unwrap_err()
                    .to_string()
                    .contains("half-scale profiles are historical")
            );
        }
        let (width, height) = crate::app::input::camera::tactical_viewport_size_px(800, 600);
        // UI artwork no longer scales; native tactical exclusions remain 168x32.
        assert_eq!((width, height), (632, 568));
        assert_eq!((width / 2, height / 2), (316, 284));
    }
}
