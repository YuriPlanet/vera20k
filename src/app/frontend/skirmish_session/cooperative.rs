//! Cooperative Skirmish campaign registry and front-end progress state.
//!
//! Retail data comes from `CoopCampMD.ini`. The app supplies the process-lifetime
//! Scenario RNG so campaign-map variant construction and country eligibility
//! retries advance the same stream as the surrounding Skirmish shell.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::sim::rng::SimRng;

pub const COOPERATIVE_CAMPAIGN_INI: &str = "CoopCampMD.ini";
const MAP_VARIANT_COUNT: usize = 3;
/// Every CoopCampMD.ini value is a `ReadString(..., 0x80)` in 0x0049DB00.
const CAMPAIGN_VALUE_CAPACITY: usize = 0x80;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CooperativeError {
    #[error("launch Country index {index} is outside the allocated registry (count {count})")]
    InvalidCountryIndex { index: u16, count: usize },
    #[error("CoopCampMD.ini was not found in the configured retail assets")]
    MissingCampaignIni,
    #[error("failed to parse CoopCampMD.ini from {source_name}: {message}")]
    InvalidCampaignIni {
        source_name: String,
        message: String,
    },
    #[error("campaign index {index} is outside the Cooperative registry (count {count})")]
    InvalidCampaignIndex { index: usize, count: usize },
    #[error(
        "Cooperative campaign '{campaign}' stage {stage} has {found} map variants; native progress construction requires three"
    )]
    InvalidMapVariantCount {
        campaign: String,
        stage: usize,
        found: usize,
    },
    #[error("the global multiplayer country roster is empty")]
    EmptyCountryRoster,
    #[error("Cooperative human start count {0} is outside native slot storage")]
    InvalidHumanStartCount(i32),
    #[error("Cooperative source human count {0} exceeds native eight-slot storage")]
    TooManyHumans(usize),
    #[error("the Cooperative {role} country list has no country present in the global roster")]
    NoEligibleCountries { role: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooperativeAiDifficulty {
    Easy,
    Normal,
    Hard,
}

impl CooperativeAiDifficulty {
    fn from_ini(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "hard" => Self::Hard,
            "normal" => Self::Normal,
            // Native enum lookup retains the caller-provided Easy default when
            // the string is not a recognized difficulty name.
            _ => Self::Easy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeStage {
    /// Exactly the first three source-order `MapN` tokens selected by
    /// `RandomRanged(0,2)`.
    pub map_variants: [String; MAP_VARIANT_COUNT],
    /// Raw `CampaignPlayerN` tokens in source order.
    pub player_countries: Vec<String>,
    /// Raw `CampaignEnemyN` tokens in source order.
    pub enemy_countries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeCampaign {
    /// Source section named by the corresponding `[Campaigns]` value.
    pub section: String,
    pub campaign_name: String,
    pub load_screen: String,
    /// Retail intentionally spells this key `Pallet`.
    pub load_screen_pallet: String,
    pub ai_difficulty: CooperativeAiDifficulty,
    pub stages: Vec<CooperativeStage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CooperativeRegistry {
    /// Campaigns in `[Campaigns]` source order. The index is CampaignType.
    pub campaigns: Vec<CooperativeCampaign>,
}

impl CooperativeRegistry {
    /// Parse the already loaded retail payload like the coopcamp.cpp registry
    /// read 0x0049DB00. A successful empty or missing `[Campaigns]` section
    /// yields a valid zero-campaign registry, matching the native lazy loader's
    /// successful-empty state.
    pub fn from_ini(ini: &IniFile) -> Result<Self, CooperativeError> {
        let Some(roster) = ini.section("Campaigns") else {
            return Ok(Self::default());
        };

        let mut campaigns = Vec::with_capacity(roster.entry_count());
        for roster_key in roster.keys() {
            let Some(section_name) = roster.read_name(roster_key, CAMPAIGN_VALUE_CAPACITY) else {
                continue;
            };
            let section = ini.section_or_empty(section_name);
            let stage_count = section.read_int("NumberOfCampaignMaps", 0).max(0) as usize;

            let mut stages = Vec::with_capacity(stage_count);
            for stage_index in 0..stage_count {
                let source_number = stage_index + 1;
                let variants = campaign_tokens(section, &format!("Map{source_number}"));
                if variants.len() < MAP_VARIANT_COUNT {
                    return Err(CooperativeError::InvalidMapVariantCount {
                        campaign: section_name.to_string(),
                        stage: source_number,
                        found: variants.len(),
                    });
                }
                stages.push(CooperativeStage {
                    map_variants: [
                        variants[0].clone(),
                        variants[1].clone(),
                        variants[2].clone(),
                    ],
                    player_countries: campaign_tokens(
                        section,
                        &format!("CampaignPlayer{source_number}"),
                    ),
                    enemy_countries: campaign_tokens(
                        section,
                        &format!("CampaignEnemy{source_number}"),
                    ),
                });
            }

            campaigns.push(CooperativeCampaign {
                section: section_name.to_string(),
                campaign_name: section.read_string("CampaignName", "", CAMPAIGN_VALUE_CAPACITY),
                load_screen: section.read_string("CampaignLoadScreen", "", CAMPAIGN_VALUE_CAPACITY),
                load_screen_pallet: section.read_string(
                    "CampaignLoadScreenPallet",
                    "",
                    CAMPAIGN_VALUE_CAPACITY,
                ),
                ai_difficulty: CooperativeAiDifficulty::from_ini(&section.read_string(
                    "CampaignAI",
                    "Easy",
                    CAMPAIGN_VALUE_CAPACITY,
                )),
                stages,
            });
        }

        Ok(Self { campaigns })
    }

    pub fn from_assets(assets: &AssetManager) -> Result<Self, CooperativeError> {
        let Some((bytes, source)) = assets.get_with_source_ref(COOPERATIVE_CAMPAIGN_INI) else {
            return Err(CooperativeError::MissingCampaignIni);
        };
        let ini =
            IniFile::from_bytes(bytes).map_err(|error| CooperativeError::InvalidCampaignIni {
                source_name: source.to_string(),
                message: error.to_string(),
            })?;
        Self::from_ini(&ini)
    }

    pub fn campaign(&self, campaign_type: usize) -> Option<&CooperativeCampaign> {
        self.campaigns.get(campaign_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeProgressRecord {
    /// Derived snapshot of the two admitted source-order participant names.
    /// Native49B760 copies the low-byte names to record+0/+1C. MatchSession
    /// remains the identity authority; this record retains names for later
    /// Cooperative save lookup/score orientation, even after setup closes.
    partner_names: [Vec<u8>; 2],
    /// Native valid byte (`record+0x6C`).
    pub valid: bool,
    /// Native `CurrentMap`; fresh records start at -1, initialized at 0.
    pub current_map: i32,
    /// Native `CampaignType`; fresh records start at -1.
    pub campaign_type: i32,
    /// One RNG-selected filename per campaign stage.
    pub chosen_maps: Vec<String>,
    /// The two partners' running counters: `+0x48/+0x50/+0x58/+0x60` for
    /// the first name and `+0x4C/+0x54/+0x5C/+0x64` for the second, in the
    /// order C0 writes them.
    tallies: [[i32; 4]; 2],
    /// `+0x68`, the campaign's elapsed time.
    time: i32,
}

impl Default for CooperativeProgressRecord {
    /// `0x0049B610`.
    fn default() -> Self {
        Self {
            partner_names: [Vec::new(), Vec::new()],
            valid: false,
            current_map: -1,
            campaign_type: -1,
            chosen_maps: Vec::new(),
            tallies: [[0; 4]; 2],
            time: 0,
        }
    }
}

/// A decoded C0 text (`0x005E43CE`), the campaign record one machine sends
/// another. Names and counters are in the sender's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeCampaignText {
    pub campaign_type: i32,
    pub current_map: i32,
    pub time: i32,
    pub tallies: [[i32; 4]; 2],
    /// The variant (0..=2) each stage chose, up to the `-1` sentinel.
    pub variants: Vec<i32>,
    pub names: [Vec<u8>; 2],
}

impl CooperativeCampaignText {
    /// The `'C'` arm of `MPGameOptions__ParsePacket` (`0x005E43CE`): `C0`,
    /// then `strtok(",")` and `atol` over eleven integers, up to ten variant
    /// tokens ending at the first negative one, and two encoded names. Both
    /// names are required. Counters a short text omits are uninitialised
    /// natively; they read as 0 here.
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix("C0")?;
        let mut tokens = rest.split(',').filter(|token| !token.is_empty());
        let mut ints = [0i32; 11];
        ints[0] = -1;
        for value in &mut ints {
            match tokens.next() {
                Some(token) => *value = crate::rules::ini_value::crt_atoi(token),
                None => return None,
            }
        }
        let mut variants = Vec::new();
        for token in tokens.by_ref() {
            let value = crate::rules::ini_value::crt_atoi(token);
            if value < 0 {
                break;
            }
            variants.push(value);
            if variants.len() == 10 {
                break;
            }
        }
        let first = tokens.next()?;
        let second = tokens.next()?;
        Some(Self {
            campaign_type: ints[0],
            current_map: ints[1],
            time: ints[2],
            tallies: [
                [ints[3], ints[4], ints[5], ints[6]],
                [ints[7], ints[8], ints[9], ints[10]],
            ],
            variants,
            names: [
                decode_name(first.as_bytes()),
                decode_name(second.as_bytes()),
            ],
        })
    }
}

/// The `=HH` name encoding of `0x0049D680`: alphanumerics as they are,
/// anything else as `=` and two upper-case hex digits, at most `0x3FD`
/// output bytes. A byte from `0x80` has a negative `char` high nibble,
/// which writes `\0` and ends the text there.
pub(crate) fn encode_name(name: &[u8]) -> String {
    let mut out = Vec::new();
    for &byte in name {
        if byte == 0 || out.len() >= 0x3FD {
            break;
        }
        // `isalnum((char)byte)`: a byte from 0x80 indexes before the CRT
        // table natively; it is taken as not alphanumeric.
        if byte < 0x80 && byte.is_ascii_alphanumeric() {
            out.push(byte);
            continue;
        }
        out.push(b'=');
        if byte >= 0x80 {
            break;
        }
        let hex = |nibble: u8| {
            if nibble < 10 {
                b'0' + nibble
            } else {
                b'7' + nibble
            }
        };
        out.push(hex(byte >> 4));
        out.push(hex(byte & 0xF));
    }
    String::from_utf8(out).expect("ASCII output")
}

/// `0x0049CCF0`'s decoder: `=` takes one or two hex digits (either case;
/// another byte counts 0), a lone trailing `=` ends the name.
fn decode_name(text: &[u8]) -> Vec<u8> {
    let digit = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut out = Vec::new();
    let mut index = 0;
    while index < text.len() {
        if text[index] != b'=' {
            out.push(text[index]);
            index += 1;
            continue;
        }
        let Some(&high) = text.get(index + 1) else {
            break;
        };
        let mut value = digit(high).unwrap_or(0);
        index += 1;
        if let Some(&low) = text.get(index + 1) {
            value = (value << 4) | digit(low).unwrap_or(0);
            index += 1;
        }
        out.push(value);
        index += 1;
    }
    out
}

impl CooperativeProgressRecord {
    /// Snapshot producer49B760. Rebinding names leaves valid/map/campaign and
    /// the RNG-selected filenames untouched; campaign initialization likewise
    /// does not clear these names. A new49B610 record begins with empty names.
    fn bind_partners(&mut self, first: &str, second: &str) {
        self.partner_names = [
            crate::util::native_string::narrow_utf16_c_string(first.encode_utf16()),
            crate::util::native_string::narrow_utf16_c_string(second.encode_utf16()),
        ];
    }

    #[cfg(test)]
    pub fn partner_names(&self) -> [&[u8]; 2] {
        [
            self.partner_names[0].as_slice(),
            self.partner_names[1].as_slice(),
        ]
    }

    /// `0x0049CAF0`: initialize a record for a source-order campaign index.
    /// It marks the record valid, sets `CurrentMap` to -1 and zeroes the
    /// counters and time. A changed (including fresh `-1`) CampaignType then
    /// makes one inclusive `(0,2)` draw per stage and sets `CurrentMap` to 0;
    /// the same CampaignType keeps its maps and leaves `CurrentMap` at -1.
    /// Returns whether the map vector was rebuilt.
    pub fn initialize_for_campaign(
        &mut self,
        registry: &CooperativeRegistry,
        campaign_index: usize,
        rng: &mut SimRng,
    ) -> Result<bool, CooperativeError> {
        let campaign =
            registry
                .campaign(campaign_index)
                .ok_or(CooperativeError::InvalidCampaignIndex {
                    index: campaign_index,
                    count: registry.campaigns.len(),
                })?;

        self.valid = true;
        self.current_map = -1;
        self.tallies = [[0; 4]; 2];
        self.time = 0;
        if self.campaign_type == campaign_index as i32 {
            return Ok(false);
        }

        let mut chosen_maps = Vec::with_capacity(campaign.stages.len());
        for stage in &campaign.stages {
            let variant = rng.next_range_u32_inclusive(0, 2) as usize;
            chosen_maps.push(stage.map_variants[variant].clone());
        }
        self.chosen_maps = chosen_maps;
        self.current_map = 0;
        self.campaign_type = campaign_index as i32;
        Ok(true)
    }

    /// `0x0049D680`'s text, `None` for an invalid record or a CampaignType
    /// outside the registry: `C0` and eleven integers, each stage's variant
    /// index (`strcmpi`; no match counts 0), `-1`, and the two names.
    pub fn campaign_text(&self, registry: &CooperativeRegistry) -> Option<String> {
        if !self.valid {
            return None;
        }
        let campaign = registry.campaign(usize::try_from(self.campaign_type).ok()?)?;
        let [first, second] = self.tallies;
        let mut text = format!(
            "C0,{},{},{},{},{},{},{},{},{},{},{}",
            self.campaign_type,
            self.current_map,
            self.time,
            first[0],
            first[1],
            first[2],
            first[3],
            second[0],
            second[1],
            second[2],
            second[3],
        );
        for (stage, chosen) in self.chosen_maps.iter().enumerate() {
            let variant = campaign.stages.get(stage).and_then(|stage| {
                stage
                    .map_variants
                    .iter()
                    .position(|variant| variant.eq_ignore_ascii_case(chosen))
            });
            text.push_str(&format!(",{}", variant.unwrap_or(0)));
        }
        text.push_str(",-1,");
        text.push_str(&encode_name(&self.partner_names[0]));
        text.push(',');
        text.push_str(&encode_name(&self.partner_names[1]));
        Some(text)
    }

    /// `0x0049CCF0` for a received C0. The sender's first name becomes this
    /// record's second, and the reverse, so the first stays the local
    /// player's on both machines. A CampaignType
    /// outside the registry leaves the record; a stage count that differs
    /// from the variant count leaves it reset (`CurrentMap` -1) and returns
    /// false. Variant indices are not range checked natively; one past the
    /// stage's three leaves that stage's map empty here.
    fn accept_campaign_text(
        &mut self,
        registry: &CooperativeRegistry,
        text: &CooperativeCampaignText,
    ) -> bool {
        let Some(campaign) = usize::try_from(text.campaign_type)
            .ok()
            .and_then(|index| registry.campaign(index))
        else {
            return self.valid;
        };
        self.current_map = -1;
        self.tallies = [[0; 4]; 2];
        self.time = 0;
        if campaign.stages.len() != text.variants.len() {
            return false;
        }
        self.campaign_type = text.campaign_type;
        self.valid = true;
        self.chosen_maps = campaign
            .stages
            .iter()
            .zip(&text.variants)
            .map(|(stage, &variant)| {
                usize::try_from(variant)
                    .ok()
                    .and_then(|variant| stage.map_variants.get(variant))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        self.current_map = 0;
        self.partner_names = [text.names[1].clone(), text.names[0].clone()];
        true
    }

    /// `0x0049CC50` through `+5C`'s swapped rows: the stage, the sender's
    /// counters added onto the other partner's, and the time. A stage
    /// outside the chosen maps changes nothing.
    fn accept_stage(&mut self, text: &CooperativeCampaignText) -> bool {
        if !(0..self.chosen_maps.len() as i32).contains(&text.current_map) {
            return false;
        }
        self.current_map = text.current_map;
        for (own, sent) in self
            .tallies
            .iter_mut()
            .zip([text.tallies[1], text.tallies[0]])
        {
            for (value, add) in own.iter_mut().zip(sent) {
                *value = value.wrapping_add(add);
            }
        }
        self.time = text.time;
        true
    }

    /// `0x0049BCC0`: the stage, while the record is valid.
    fn stage(&self) -> Option<i32> {
        self.valid.then_some(self.current_map)
    }

    pub fn chosen_map(&self, map_index: usize) -> Option<&str> {
        self.chosen_maps.get(map_index).map(String::as_str)
    }

    pub fn current_chosen_map(&self) -> Option<&str> {
        usize::try_from(self.current_map)
            .ok()
            .and_then(|index| self.chosen_map(index))
    }

    /// `0x0049B930`, `+0xA0`'s accounting after a won mission. Row 0 (the
    /// first human House) counts for the first name when its narrowed UI
    /// name equals it (case-sensitive `strcmp`), else the rows swap. The
    /// mission's elapsed ticks are added to the time and `CurrentMap`
    /// advances. An invalid record changes nothing.
    pub fn record_win(&mut self, rows: [&CooperativeMissionRow; 2], elapsed: i32) -> bool {
        if !self.valid {
            return false;
        }
        let rows = if rows[0].name == self.partner_names[0] {
            rows
        } else {
            [rows[1], rows[0]]
        };
        for (tally, row) in self.tallies.iter_mut().zip(rows) {
            for (value, add) in tally.iter_mut().zip(row.tally()) {
                *value = value.wrapping_add(add);
            }
        }
        self.time = self.time.wrapping_add(elapsed);
        self.current_map = self.current_map.wrapping_add(1);
        true
    }

    /// `0x0049D070` with -1: the chosen map at `CurrentMap`, `None` past the
    /// last stage or on an invalid record.
    pub fn next_scenario_token(&self) -> Option<&str> {
        self.valid.then(|| self.current_chosen_map()).flatten()
    }

    /// `+0xA8`'s `can_continue`: `CurrentMap` (`0x0049BCC0`) below the stage
    /// count (`0x0049BCA0`).
    pub fn can_continue(&self) -> bool {
        self.current_map < self.chosen_maps.len() as i32
    }

    /// `0x0049C5A0`, the coopsave.ini writer, as the section and keys it
    /// would write. `house` holds the local node's and then the partner's
    /// country and colour (`0x00696F10`/`0x00696F20`).
    pub fn coop_save(
        &self,
        registry: &CooperativeRegistry,
        roster: &[CooperativeCountryRosterEntry],
        house: [(i32, i32); 2],
    ) -> CoopSave {
        let campaign = usize::try_from(self.campaign_type)
            .ok()
            .and_then(|index| registry.campaign(index));
        let (true, false, false, Some(campaign)) = (
            self.valid,
            self.partner_names[0].is_empty(),
            self.partner_names[1].is_empty(),
            campaign,
        ) else {
            return CoopSave::Failed;
        };
        // 0x0049C611..0x0049C624: the last stage won writes nothing.
        if self.current_map == campaign.stages.len() as i32 {
            return CoopSave::Unchanged;
        }
        let section = format!(
            "{}{}",
            encode_name(&self.partner_names[0]),
            encode_name(&self.partner_names[1])
        );
        let mut values = vec![
            ("CurrentMap".to_owned(), self.current_map.to_string()),
            ("CampaignType".to_owned(), self.campaign_type.to_string()),
        ];
        for stage in 0..campaign.stages.len() {
            values.push((
                format!("Map{}", stage + 1),
                self.chosen_maps.get(stage).cloned().unwrap_or_default(),
            ));
        }
        for (index, (country, color)) in house.into_iter().enumerate() {
            values.push((
                format!("House{}", index + 1),
                house_type_text(country, roster),
            ));
            values.push((format!("Color{}", index + 1), color.to_string()));
        }
        values.push(("Time".to_owned(), self.time.to_string()));
        let [first, second] = self.tallies;
        for (key, column) in [("Kills", 0), ("Built", 1), ("Lost", 2), ("Score", 3)] {
            values.push((format!("{key}1"), first[column].to_string()));
            values.push((format!("{key}2"), second[column].to_string()));
        }
        CoopSave::Write { section, values }
    }
}

/// One human House's row for `0x0049B930`: its narrowed UI name and the
/// figures the record adds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CooperativeMissionRow {
    pub name: Vec<u8>,
    pub kills: i32,
    pub built: i32,
    pub lost: i32,
    pub score: i32,
}

impl CooperativeMissionRow {
    /// The record's column order: kills, built, lost, score.
    fn tally(&self) -> [i32; 4] {
        [self.kills, self.built, self.lost, self.score]
    }
}

/// What `0x0049C5A0` does with the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoopSave {
    /// It returns false and writes nothing (`TXT_COOP_SAVE_FAILED`).
    Failed,
    /// The campaign's last stage: true without writing.
    Unchanged,
    /// Clear the section, put these keys in order, save the file. The file
    /// write's own result is ignored natively.
    Write {
        section: String,
        values: Vec<(String, String)>,
    },
}

/// The coopsave.ini file name (`0x0081F0F8`), opened beside the profile.
pub const COOP_SAVE_INI: &str = "coopsave.ini";

/// `INIClass::Put_HouseType` (`0x00475610`): a HouseType's ID, the
/// `<Player @ A>`..`<Player @ H>` pseudo-Houses `0x117B..0x1182`,
/// `<random>` for -2 and -3, else `<none>`.
pub(crate) fn house_type_text(index: i32, roster: &[CooperativeCountryRosterEntry]) -> String {
    match index {
        0x117B..=0x1182 => format!("<Player @ {}>", (b'A' + (index - 0x117B) as u8) as char),
        -3 | -2 => "<random>".to_owned(),
        _ => usize::try_from(index)
            .ok()
            .and_then(|index| roster.get(index))
            .map_or_else(|| "<none>".to_owned(), |entry| entry.id.clone()),
    }
}

/// The two independently ordered writes of Cooperative mode+68/5C30C0.
/// The session owner applies declared_count even when a stale campaign stage
/// leaves rows unchanged; this is not a second participant authority.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CooperativeAiPreparation {
    pub declared_count: Option<i32>,
    pub opponents: Option<Vec<crate::skirmish_launch::SkirmishAiSlot>>,
}

/// 5C30C0, called before69B8C0. No RNG. Country/color placeholders are masked
/// by Random exactly as other shell assignments; the canonical resolver owns
/// their eventual values.
pub(crate) fn prepare_ai(
    registry: &CooperativeRegistry,
    progress: Option<&CooperativeProgressRecord>,
    campaign_index: usize,
    header: crate::map::preview::ScenarioStartHeader,
    capacity: i32,
    difficulty: i32,
    existing: &[crate::skirmish_launch::SkirmishAiSlot],
) -> Result<CooperativeAiPreparation, CooperativeError> {
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
    };
    let first = header.coop_human_start_spots();
    let count = capacity.wrapping_sub(first);
    if count <= 0 {
        return Ok(CooperativeAiPreparation::default());
    }
    let mut effect = CooperativeAiPreparation {
        declared_count: Some(count),
        opponents: None,
    };
    let stage = progress.filter(|p| p.valid).map_or(0, |p| p.current_map);
    let Some(campaign) = registry.campaign(campaign_index) else {
        return Ok(effect);
    };
    if campaign.stages.is_empty() || stage >= campaign.stages.len() as i32 {
        return Ok(effect);
    }
    // Negative values would write before Session's native array; reject that
    // corrupt-data case instead of manufacturing an in-range native result.
    if first < 0 {
        return Err(CooperativeError::InvalidHumanStartCount(first));
    }
    let mut opponents: Vec<_> = existing
        .iter()
        .filter(|slot| i32::from(slot.raw_slot_index) < first)
        .cloned()
        .collect();
    let difficulty = match difficulty {
        0 => AiDifficulty::Easy,
        1 => AiDifficulty::Normal,
        _ => AiDifficulty::Hard,
    };
    for row in first..8 {
        if row - first >= count {
            break;
        }
        opponents.push(SkirmishAiSlot {
            raw_slot_index: row as u8,
            country: LaunchCountry::America,
            country_random: true,
            color_index: 0,
            color_random: true,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty,
        });
    }
    effect.opponents = Some(opponents);
    Ok(effect)
}

/// Mode+64/5C2710's retained C1 payload. Round-major order is also the native
/// draw order; a pair is [country index, color index]. This cache is generated
/// once after69B8C0 and replaced by a decoded C1 on guests. It does not replace
/// MatchSession's current resolved participant choices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CooperativeRoundAssignments {
    rounds: Vec<[[i32; 2]; 8]>,
}
impl CooperativeRoundAssignments {
    pub(crate) fn rounds(&self) -> &[[[i32; 2]; 8]] {
        &self.rounds
    }
    pub(crate) fn accept(&mut self, rounds: Vec<[[i32; 2]; 8]>) {
        self.rounds = rounds;
    }
}

/// What a Cooperative mode slot reads besides its own state: the session's
/// scenario file names (`[0xA8B8CC]`, catalog order), the nodes' names
/// (`0xA8DA78`, the local player first) and whether Choose Map `0x6B` is
/// the top dialog.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CooperativeSlotContext<'a> {
    pub scenarios: &'a [&'a str],
    pub nodes: &'a [&'a str],
    pub chooser_on_top: bool,
}

/// One campaign row of `+0x4C`'s chooser list: `"%d. %s"` with the
/// campaign's number and name, and the scenario record of its reserve's
/// current map as item data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CooperativeChooserRow {
    pub campaign: usize,
    pub record: usize,
}

/// `strnicmp(file, token, strlen(token)) == 0`: the scenario file starts
/// with the campaign token, case-insensitively (`0x005C1980`, `+0x20`,
/// `+0x44`).
fn scenario_starts_with(file: &str, token: &str) -> bool {
    file.len() >= token.len()
        && file.as_bytes()[..token.len()].eq_ignore_ascii_case(token.as_bytes())
}

/// `strcmpi(file, sprintf("%s.MAP", token)) == 0` (`+0x4C`).
fn scenario_is_map(file: &str, token: &str) -> bool {
    file.len() == token.len() + 4
        && scenario_starts_with(file, token)
        && file[token.len()..].eq_ignore_ascii_case(".MAP")
}

/// `0x0049B610` then `0x0049CAF0(campaign)`. An index outside the registry
/// leaves the record invalid, as the native initializer returns without
/// writing.
fn fresh_record(
    registry: &CooperativeRegistry,
    campaign: usize,
    rng: &mut SimRng,
) -> CooperativeProgressRecord {
    let mut record = CooperativeProgressRecord::default();
    let _ = record.initialize_for_campaign(registry, campaign, rng);
    record
}

/// `0x0049B760` with the two nodes' names, only when there are exactly two.
fn bind_two_nodes(record: &mut CooperativeProgressRecord, nodes: &[&str]) {
    if let [first, second] = nodes {
        record.bind_partners(first, second);
    }
}

/// MPCooperative's progress state (vtable `0x007EE27C`, constructor
/// `0x005C1470`): the active record `+0x40`, the reserve per campaign
/// `+0x324`, the campaign `+0x33C`, the refresh flag `+0x340` and the
/// reserve taken `+0x4C`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeProgressState {
    active: Option<CooperativeProgressRecord>,
    reserves: Vec<Option<CooperativeProgressRecord>>,
    round_assignments: CooperativeRoundAssignments,
    /// `+0x33C`: the campaign the setup screen's countries come from. It
    /// follows the active record only where a slot copies it.
    campaign: i32,
    /// `+0x340`: the setup screen's countries need `0x005C2500`.
    refresh_pending: bool,
    /// `+0x4C`: the campaign whose reserve Choose Map last took.
    reserve_taken: i32,
}

impl Default for CooperativeProgressState {
    fn default() -> Self {
        Self {
            active: None,
            reserves: Vec::new(),
            round_assignments: CooperativeRoundAssignments::default(),
            campaign: 0,
            refresh_pending: true,
            reserve_taken: 0,
        }
    }
}

impl CooperativeProgressState {
    /// Construct the per-campaign reserve vector after boot seeding. Stock data
    /// consumes ten logical `(0,2)` calls here even when Battle is later chosen.
    pub fn construct(
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
    ) -> Result<Self, CooperativeError> {
        let mut reserves = Vec::with_capacity(registry.campaigns.len());
        for campaign_index in 0..registry.campaigns.len() {
            let mut record = CooperativeProgressRecord::default();
            record.initialize_for_campaign(registry, campaign_index, rng)?;
            reserves.push(Some(record));
        }
        Ok(Self {
            reserves,
            ..Self::default()
        })
    }

    /// `+0x10` (`0x005C18A0`): delete every reserve and build one per
    /// campaign, one draw per stage each.
    pub(crate) fn rebuild_reserves(&mut self, registry: &CooperativeRegistry, rng: &mut SimRng) {
        self.reserves = (0..registry.campaigns.len())
            .map(|campaign| Some(fresh_record(registry, campaign, rng)))
            .collect();
    }

    /// `0x005C1980`: the last campaign one of whose three first-stage
    /// variants starts the current scenario's file name; scenario 0 never
    /// matches. -1 when none does.
    pub(crate) fn campaign_of_scenario(
        registry: &CooperativeRegistry,
        scenarios: &[&str],
        current: usize,
    ) -> i32 {
        let mut found = -1;
        for (index, campaign) in registry.campaigns.iter().enumerate() {
            let Some(stage) = campaign.stages.first() else {
                continue;
            };
            for variant in &stage.map_variants {
                if current != 0
                    && scenarios
                        .get(current)
                        .is_some_and(|file| scenario_starts_with(file, variant))
                {
                    found = index as i32;
                }
            }
        }
        found
    }

    /// `+0x20` (`0x005C1A10`), the mode becoming current. With a setup
    /// dialog open, the active record goes unless Choose Map is on top with
    /// the scenario's campaign; a kept record rebinds the two names (or
    /// none). Without an active record a new one takes campaign 0 (one draw
    /// per stage). When the scenario is no first-stage map, the campaign is
    /// 0 and the scenario whose name the record's first map starts is
    /// selected (else scenario 0); returns it. Otherwise the record rerolls
    /// (another campaign, then this one) until its first map starts the
    /// scenario's name. The `"O8"` observer text this slot sends is not
    /// ported (observers are refused in Cooperative).
    pub(crate) fn enter_mode(
        &mut self,
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
        context: CooperativeSlotContext,
        current: usize,
        setup_open: bool,
    ) -> Option<usize> {
        if setup_open {
            let campaign = Self::campaign_of_scenario(registry, context.scenarios, current);
            let keep = self.active.as_ref().is_some_and(|active| {
                context.chooser_on_top && active.valid && active.campaign_type == campaign
            });
            if !keep {
                self.active = None;
            }
            if let Some(active) = self.active.as_mut() {
                match context.nodes {
                    [first, second] => active.bind_partners(first, second),
                    _ => active.bind_partners("", ""),
                }
            }
        }
        let mut selected = None;
        if self.active.is_none() {
            let mut record = fresh_record(registry, 0, rng);
            let campaign = Self::campaign_of_scenario(registry, context.scenarios, current);
            if campaign < 0 {
                self.campaign = 0;
                let token = record.chosen_map(0).unwrap_or_default();
                selected = Some(
                    context
                        .scenarios
                        .iter()
                        .position(|file| scenario_starts_with(file, token))
                        .unwrap_or(0),
                );
            } else {
                self.campaign = campaign;
                let file = context.scenarios.get(current).copied().unwrap_or_default();
                // The campaign was found from one of these variants, so a
                // reroll reaches it; a campaign without stages has no map.
                while let Some(token) = record.chosen_map(0)
                    && !scenario_starts_with(file, token)
                {
                    let other = usize::from(campaign == 0);
                    let _ = record.initialize_for_campaign(registry, other, rng);
                    let _ = record.initialize_for_campaign(registry, campaign as usize, rng);
                }
            }
            self.active = Some(record);
        }
        self.refresh_pending = true;
        selected
    }

    /// `+0x44` (`0x005C1CC0`), the mode's default scenario: the active
    /// record (made if missing) is initialized for campaign 0 and the first
    /// scenario its first map starts is returned. None rebuilds the
    /// reserves (`+0x10`); an empty registry returns None at once.
    pub(crate) fn default_scenario(
        &mut self,
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
        scenarios: &[&str],
    ) -> Option<usize> {
        if registry.campaigns.is_empty() {
            return None;
        }
        let active = self.active.get_or_insert_with(Default::default);
        let _ = active.initialize_for_campaign(registry, 0, rng);
        let token = active.chosen_map(0).filter(|_| active.valid);
        if let Some(index) = token.and_then(|token| {
            scenarios
                .iter()
                .position(|file| scenario_starts_with(file, token))
        }) {
            return Some(index);
        }
        self.rebuild_reserves(registry, rng);
        None
    }

    /// `+0x4C` (`0x005C3B10`), Choose Map's list in Cooperative. A missing
    /// active record is made with the two nodes' names and no campaign
    /// (`0x0049B660`); a present one loses its names unless there are two
    /// nodes. Missing reserves are built. Each campaign whose reserve's
    /// current map names one of `records` (`"%s.MAP"`) gives a row; the
    /// others are skipped. The saved-campaign row is not ported.
    pub(crate) fn chooser_rows(
        &mut self,
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
        nodes: &[&str],
        records: &[(usize, &str)],
    ) -> Vec<CooperativeChooserRow> {
        match self.active.as_mut() {
            None if nodes.len() == 2 => {
                let mut record = CooperativeProgressRecord::default();
                bind_two_nodes(&mut record, nodes);
                self.active = Some(record);
            }
            Some(active) if nodes.len() != 2 => active.bind_partners("", ""),
            _ => {}
        }
        if self.reserves.is_empty() {
            self.rebuild_reserves(registry, rng);
        }
        (0..registry.campaigns.len())
            .filter_map(|campaign| {
                let token = self
                    .reserves
                    .get(campaign)?
                    .as_ref()?
                    .current_chosen_map()?;
                let record = records
                    .iter()
                    .find(|(_, file)| scenario_is_map(file, token))?
                    .0;
                Some(CooperativeChooserRow { campaign, record })
            })
            .collect()
    }

    /// `+0x18` (`0x005C1DC0`), Use Map's campaign: with Choose Map on top,
    /// the chosen campaign's reserve becomes the active record and a fresh
    /// reserve replaces it (one draw per stage). Returns whether the setup
    /// screen's countries refresh (`0x005C2500` and the `+0x1C` re-check):
    /// false without an active record, for the saved-campaign row (not
    /// ported) or when `+0x33C`'s campaign lacks the record's stage.
    pub(crate) fn accept_chooser_row(
        &mut self,
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
        context: CooperativeSlotContext,
        row: usize,
    ) -> bool {
        if self.active.is_none() {
            return false;
        }
        if row >= registry.campaigns.len() {
            if let Some(active) = self.active.as_mut() {
                bind_two_nodes(active, context.nodes);
            }
            return false;
        }
        if context.chooser_on_top {
            let taken = self
                .reserves
                .get_mut(row)
                .and_then(Option::take)
                .unwrap_or_else(|| fresh_record(registry, row, rng));
            self.active = Some(taken);
            let replacement = fresh_record(registry, row, rng);
            if let Some(slot) = self.reserves.get_mut(row) {
                *slot = Some(replacement);
            }
            self.reserve_taken = row as i32;
            if let Some(active) = self.active.as_mut() {
                bind_two_nodes(active, context.nodes);
            }
        }
        self.stage_countries(registry).is_some()
    }

    /// `+0x24` (`0x005C1CB0`): the setup screen's countries need a refresh.
    pub(crate) fn request_refresh(&mut self) {
        self.refresh_pending = true;
    }

    /// `+0x60` (`0x005C2680`), publish the campaign: a valid active record
    /// sets `+0x33C`; with a setup dialog open a pending refresh is taken.
    /// Returns (refresh, C0 text); None without an active record.
    pub(crate) fn publish(
        &mut self,
        registry: &CooperativeRegistry,
        setup_open: bool,
    ) -> Option<(bool, Option<String>)> {
        let active = self.active.as_ref()?;
        if active.valid {
            self.campaign = active.campaign_type;
        }
        let refresh = setup_open && self.refresh_pending;
        if refresh {
            self.refresh_pending = false;
        }
        Some((refresh, active.campaign_text(registry)))
    }

    /// `+0x94` (`0x005C4110`), a guest joined: the active record is made
    /// for campaign 0 if missing, takes the two names, then `+0x60`. The
    /// saved-campaign tail (`TXT_SAVE_CAMP_AVAILABLE`) is not ported.
    pub(crate) fn join(
        &mut self,
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
        nodes: &[&str],
        setup_open: bool,
    ) -> Option<(bool, Option<String>)> {
        let active = self
            .active
            .get_or_insert_with(|| fresh_record(registry, 0, rng));
        bind_two_nodes(active, nodes);
        self.publish(registry, setup_open)
    }

    /// `+0x58` (`0x005C2040`), a received C0's campaign: a missing active
    /// record is recreated for campaign 0 (`0x005C3EC0`) and flags a
    /// refresh, as does a campaign change; then `0x0049CCF0` and `+0x33C`.
    pub(crate) fn receive_campaign(
        &mut self,
        registry: &CooperativeRegistry,
        rng: &mut SimRng,
        nodes: &[&str],
        text: &CooperativeCampaignText,
    ) {
        if self.active.is_none() {
            let mut record = fresh_record(registry, 0, rng);
            bind_two_nodes(&mut record, nodes);
            self.active = Some(record);
            self.refresh_pending = true;
        }
        let active = self.active.as_mut().expect("made above");
        if !active.valid || active.campaign_type != text.campaign_type {
            self.refresh_pending = true;
        }
        active.accept_campaign_text(registry, text);
        self.campaign = text.campaign_type;
    }

    /// `+0x5C` (`0x005C2100`), a received C0's stage and counters. Returns
    /// whether the setup screen refreshes: when the stage changed or a
    /// refresh is pending, and a setup dialog is open (which takes it).
    pub(crate) fn receive_stage(
        &mut self,
        text: &CooperativeCampaignText,
        setup_open: bool,
    ) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let before = active.stage();
        active.accept_stage(text);
        let refresh = (before != Some(text.current_map) || self.refresh_pending) && setup_open;
        if refresh {
            self.refresh_pending = false;
        }
        refresh
    }

    /// The `+0x33C` campaign's stage at the active record's (or 0), as
    /// `0x005C23B0`, `+0x1C` and `+0x28` read it; None without that
    /// campaign or stage.
    fn stage_countries<'r>(&self, registry: &'r CooperativeRegistry) -> Option<&'r [String]> {
        let campaign = registry.campaign(usize::try_from(self.campaign).ok()?)?;
        let stage = self
            .active
            .as_ref()
            .and_then(CooperativeProgressRecord::stage)
            .unwrap_or(0);
        let stage = campaign.stages.get(usize::try_from(stage).ok()?)?;
        Some(&stage.player_countries)
    }

    /// The setup screen's country list (`0x005C23B0`): the stage's player
    /// countries in authored order, Random first when there are several.
    pub(crate) fn setup_countries<'r>(
        &self,
        registry: &'r CooperativeRegistry,
    ) -> Option<&'r [String]> {
        self.stage_countries(registry)
    }

    /// `+0x33C`, the campaign `+0x64`, `+0x68` and the setup screen read.
    pub(crate) fn campaign(&self) -> usize {
        usize::try_from(self.campaign).unwrap_or(0)
    }

    /// `0x005C21D0`'s description: `(CampaignType, CurrentMap)` while the
    /// active record is valid.
    pub(crate) fn description(&self) -> Option<(i32, i32)> {
        self.active
            .as_ref()
            .filter(|active| active.valid)
            .map(|active| (active.campaign_type, active.current_map))
    }

    /// `+0x1C` (`0x005C17B0`), country admission: true without the stage;
    /// Random (-2) only when the stage has several countries; observer
    /// (-3) always; else the country must be one of the stage's.
    pub(crate) fn admits_country(
        &self,
        registry: &CooperativeRegistry,
        roster: &[CooperativeCountryRosterEntry],
        country: i32,
    ) -> bool {
        let Some(countries) = self.stage_countries(registry) else {
            return true;
        };
        if (country == -2 && countries.len() > 1) || country == -3 {
            return true;
        }
        countries
            .iter()
            .any(|token| find_country(roster, token) == country)
    }

    /// `+0x28` (`0x005C1FA0`), the default country: the stage's single
    /// country, else Random (-2).
    pub(crate) fn default_country(
        &self,
        registry: &CooperativeRegistry,
        roster: &[CooperativeCountryRosterEntry],
    ) -> i32 {
        match self.stage_countries(registry) {
            Some([only]) => find_country(roster, only),
            _ => -2,
        }
    }

    /// Original5C2710: generate every campaign round using the CURRENT
    /// stage's country callbacks, including all8 slots even when fewer are
    /// active. Human colors come from already-resolved source-order nodes.
    pub(crate) fn prepare_round_assignments(
        &mut self,
        registry: &CooperativeRegistry,
        campaign_index: usize,
        roster: &[CooperativeCountryRosterEntry],
        human_colors: &[i32],
        rng: &mut SimRng,
    ) -> Result<(), CooperativeError> {
        if human_colors.len() > 8 {
            return Err(CooperativeError::TooManyHumans(human_colors.len()));
        }
        let campaign =
            registry
                .campaign(campaign_index)
                .ok_or(CooperativeError::InvalidCampaignIndex {
                    index: campaign_index,
                    count: registry.campaigns.len(),
                })?;
        self.round_assignments.rounds.clear();
        let progress = self.active.clone().unwrap_or_default();
        let rounds = (campaign.stages.len() as i8).max(0) as usize;
        for _ in 0..rounds {
            let mut colors = [-2; 8];
            colors[..human_colors.len()].copy_from_slice(human_colors);
            let mut round = [[-1; 2]; 8];
            for row in 0..8 {
                let role = if row < human_colors.len() {
                    CooperativeCountryRole::Player
                } else {
                    CooperativeCountryRole::Enemy
                };
                let country =
                    draw_country_for_progress(registry, &progress, role, roster, rng)? as i32;
                let color = if row < human_colors.len() {
                    colors[row]
                } else {
                    let color = loop {
                        let candidate = rng.next_range_u32_inclusive(0, 7) as i32;
                        if !colors.contains(&candidate) {
                            break candidate;
                        }
                    };
                    colors[row] = color;
                    color
                };
                round[row] = [country, color];
            }
            self.round_assignments.rounds.push(round);
        }
        Ok(())
    }

    pub(crate) fn round_assignments(&self) -> &CooperativeRoundAssignments {
        &self.round_assignments
    }

    /// Native+74/5C2A60 applies decoded service values without random draws.
    pub(crate) fn accept_round_assignments(&mut self, rounds: Vec<[[i32; 2]; 8]>) {
        self.round_assignments.accept(rounds);
    }

    /// MPCooperative5C1D80 Start validation binds exactly two humans only
    /// while an active progress record already exists. It never allocates or
    /// rerolls a record, and a different participant count leaves it intact.
    pub fn bind_network_partners(&mut self, names: &[&str]) -> bool {
        let [first, second] = names else {
            return false;
        };
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        active.bind_partners(first, second);
        true
    }

    /// Coop peer departure +90/5C3F80 ends at5C40E9..5C40FC by binding
    /// two empty names through49B760. Keep the retained progress/maps intact;
    /// the saved-campaign replacement branch is a separate caller effect.
    pub fn clear_network_partners(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.bind_partners("", "");
        }
    }

    pub fn active(&self) -> Option<&CooperativeProgressRecord> {
        self.active.as_ref()
    }

    pub(crate) fn active_mut(&mut self) -> Option<&mut CooperativeProgressRecord> {
        self.active.as_mut()
    }

    #[cfg(test)]
    pub fn reserve(&self, campaign_index: usize) -> Option<&CooperativeProgressRecord> {
        self.reserves.get(campaign_index)?.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeCountryRosterEntry {
    /// HouseType registry ID, such as `Americans`.
    pub id: String,
    /// Optional HouseType `Name=` alias, such as `America`.
    pub name: Option<String>,
}

impl CooperativeCountryRosterEntry {
    pub fn new(id: impl Into<String>, name: Option<&str>) -> Self {
        Self {
            id: id.into(),
            name: name.map(str::to_string),
        }
    }

    fn matches(&self, token: &str) -> bool {
        self.id.eq_ignore_ascii_case(token)
            || self
                .name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(token))
    }
}

/// `HouseTypeClass__FindIndexByNameOrID`: the roster index, -1 if absent.
fn find_country(roster: &[CooperativeCountryRosterEntry], token: &str) -> i32 {
    roster
        .iter()
        .position(|entry| entry.matches(token))
        .map_or(-1, |index| index as i32)
}

/// Resolve authored tokens through the same ID/Name aliases used by the
/// native country eligibility owner. Ordering and repeated tokens are retained
/// for consumers that build a menu from the authored list.
pub fn country_indices_in_authored_order(
    tokens: &[String],
    roster: &[CooperativeCountryRosterEntry],
) -> Vec<usize> {
    tokens
        .iter()
        .filter_map(|token| roster.iter().position(|entry| entry.matches(token)))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooperativeCountryRole {
    Player,
    Enemy,
}

impl CooperativeCountryRole {
    fn label(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Enemy => "enemy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CooperativeCountryEligibility {
    accepted: Vec<bool>,
}

impl CooperativeCountryEligibility {
    /// Compile the token loop into the same predicate over every global roster
    /// index. `<random>` and unknown tokens never equal a concrete candidate.
    pub fn compile(
        tokens: &[String],
        roster: &[CooperativeCountryRosterEntry],
        role: CooperativeCountryRole,
    ) -> Result<Self, CooperativeError> {
        if roster.is_empty() {
            return Err(CooperativeError::EmptyCountryRoster);
        }
        let mut accepted = vec![false; roster.len()];
        for index in country_indices_in_authored_order(tokens, roster) {
            accepted[index] = true;
        }
        if !accepted.iter().any(|accepted| *accepted) {
            return Err(CooperativeError::NoEligibleCountries { role: role.label() });
        }
        Ok(Self { accepted })
    }

    /// Draw against the full global roster bound until membership succeeds.
    /// Validation at construction makes the native unbounded retry safe without
    /// adding an attempt cap that would change the RNG transcript.
    pub fn draw(&self, rng: &mut SimRng) -> usize {
        loop {
            let candidate =
                rng.next_range_u32_inclusive(0, (self.accepted.len() - 1) as u32) as usize;
            if self.accepted[candidate] {
                return candidate;
            }
        }
    }
}

/// Cooperative human/AI country callback over the active progress record.
///
/// Invalid/missing campaign or map data returns country zero without consuming
/// RNG. A present but empty/all-invalid eligibility list returns a validation
/// error instead of entering the native infinite retry loop.
pub fn draw_country_for_progress(
    registry: &CooperativeRegistry,
    progress: &CooperativeProgressRecord,
    role: CooperativeCountryRole,
    roster: &[CooperativeCountryRosterEntry],
    rng: &mut SimRng,
) -> Result<usize, CooperativeError> {
    if !progress.valid {
        return Ok(0);
    }
    let Ok(campaign_index) = usize::try_from(progress.campaign_type) else {
        return Ok(0);
    };
    let Some(campaign) = registry.campaign(campaign_index) else {
        return Ok(0);
    };
    let Ok(map_index) = usize::try_from(progress.current_map) else {
        return Ok(0);
    };
    let Some(stage) = campaign.stages.get(map_index) else {
        return Ok(0);
    };
    let tokens = match role {
        CooperativeCountryRole::Player => &stage.player_countries,
        CooperativeCountryRole::Enemy => &stage.enemy_countries,
    };
    let eligibility = CooperativeCountryEligibility::compile(tokens, roster, role)?;
    Ok(eligibility.draw(rng))
}

/// `MapN`, `CampaignPlayerN` and `CampaignEnemyN` are `ReadString(0x80)`
/// values split with `strtok(",")`; an absent key yields no tokens.
fn campaign_tokens(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_list(key, CAMPAIGN_VALUE_CAPACITY)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cooperative_rounds_preserve_full_rng_and_current_stage_draw_order() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        let corpus = &corpus["cooperative"];
        let registry = CooperativeRegistry::from_ini(&IniFile::from_str(
            corpus["campaign_ini"].as_str().unwrap(),
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let campaign_index = registry
                .campaigns
                .iter()
                .position(|campaign| campaign.section == case["campaign"].as_str().unwrap())
                .unwrap();
            let roster: Vec<_> = case["roster"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    CooperativeCountryRosterEntry::new(
                        row["id"].as_str().unwrap(),
                        row["name"].as_str(),
                    )
                })
                .collect();
            let colors: Vec<_> = case["human_colors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|color| color.as_i64().unwrap() as i32)
                .collect();
            let mut owner = CooperativeProgressState::default();
            owner.active = Some(CooperativeProgressRecord {
                valid: true,
                campaign_type: campaign_index as i32,
                current_map: case["current_stage"].as_i64().unwrap() as i32,
                ..Default::default()
            });
            let mut rng =
                SimRng::from_native_state_hex_for_test(case["rng_before"].as_str().unwrap());
            let (result, draws) = crate::sim::rng::trace_draws(|| {
                owner.prepare_round_assignments(
                    &registry,
                    campaign_index,
                    &roster,
                    &colors,
                    &mut rng,
                )
            });
            result.unwrap();
            assert_eq!(
                serde_json::to_value(owner.round_assignments().rounds()).unwrap(),
                case["rounds"],
                "{} rounds",
                case["name"]
            );
            assert_eq!(
                rng.native_state_hex(),
                case["rng_after"].as_str().unwrap(),
                "{} full RNG",
                case["name"]
            );
            let expected_raw: Vec<_> = case["draws"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|draw| draw["raw"].as_array().unwrap().iter().cloned())
                .collect();
            assert_eq!(
                draws
                    .iter()
                    .map(|draw| draw["value"].clone())
                    .collect::<Vec<_>>(),
                expected_raw,
                "{} raw draw order",
                case["name"]
            );
            let before = rng.native_state_hex();
            let rounds = owner.round_assignments().rounds().to_vec();
            owner.accept_round_assignments(rounds.clone());
            assert_eq!(owner.round_assignments().rounds(), rounds);
            assert_eq!(rng.native_state_hex(), before, "decoded C1 does not draw");
        }
    }

    #[test]
    fn native_cooperative_ai_preparation_keeps_partial_count_effects() {
        use crate::skirmish_launch::{
            AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
        };
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        let corpus = &corpus["cooperative"];
        let registry = CooperativeRegistry::from_ini(&IniFile::from_str(
            corpus["campaign_ini"].as_str().unwrap(),
        ))
        .unwrap();
        for case in corpus["preparation"].as_array().unwrap() {
            let existing = vec![SkirmishAiSlot {
                raw_slot_index: 5,
                country: LaunchCountry::Yuri,
                country_random: false,
                color_index: 3,
                color_random: false,
                start_position: LaunchStartPosition::Position(1),
                team: LaunchTeam::Team(2),
                difficulty: AiDifficulty::Hard,
            }];
            let progress = CooperativeProgressRecord {
                valid: true,
                campaign_type: 0,
                current_map: case["current_stage"].as_i64().unwrap() as i32,
                ..Default::default()
            };
            let mut header = crate::map::preview::ScenarioStartHeader::default();
            header.read_ini(&IniFile::from_str(&format!(
                "[Header]\nNumCoopHumanStartSpots={}",
                case["first"]
            )));
            let effect = prepare_ai(
                &registry,
                Some(&progress),
                0,
                header,
                case["capacity"].as_i64().unwrap() as i32,
                case["difficulty"].as_i64().unwrap() as i32,
                &existing,
            )
            .unwrap();
            assert_eq!(
                effect.declared_count.unwrap_or(17),
                case["count"].as_i64().unwrap() as i32,
                "{} count",
                case["name"]
            );
            let rows = effect.opponents.unwrap_or(existing);
            for row in 0..8 {
                let expected = &case["rows"][row];
                let actual = rows.iter().find(|ai| usize::from(ai.raw_slot_index) == row);
                if expected[0].as_i64() == Some(-1) {
                    assert!(actual.is_none());
                    continue;
                }
                let actual = actual.unwrap();
                assert_eq!(
                    actual.difficulty.as_i32(),
                    expected[0].as_i64().unwrap() as i32
                );
                assert_eq!(actual.country_random, expected[1].as_i64() == Some(-2));
                assert_eq!(actual.color_random, expected[2].as_i64() == Some(-2));
                if actual.color_random {
                    assert_eq!(actual.start_position, LaunchStartPosition::Auto);
                    assert_eq!(actual.team, LaunchTeam::None);
                }
            }
            assert!(case["draws"].as_array().unwrap().is_empty());
        }
    }

    fn registry() -> CooperativeRegistry {
        CooperativeRegistry::from_ini(&IniFile::from_str(
            "[Campaigns]\nCampaign1=Allied\nCampaign2=World\n\
             [Allied]\nNumberOfCampaignMaps=2\nCampaignName=Name:All_Coop\n\
             CampaignPlayer1=Americans,British\nCampaignEnemy1=Russians,YuriCountry\n\
             CampaignPlayer2=Americans\nCampaignEnemy2=YuriCountry\nCampaignAI=Normal\n\
             Map1=A1,A1B,A1C\nMap2=A2,A2B,A2C\nCampaignLoadScreen=LSA.shp\n\
             CampaignLoadScreenPallet=LSA.pal\n\
             [World]\nNumberOfCampaignMaps=1\nCampaignPlayer1=America\n\
             CampaignEnemy1=YuriCountry\nMap1=W1,W1B,W1C\n",
        ))
        .unwrap()
    }

    fn roster() -> Vec<CooperativeCountryRosterEntry> {
        vec![
            CooperativeCountryRosterEntry::new("Americans", Some("America")),
            CooperativeCountryRosterEntry::new("British", Some("Great Britain")),
            CooperativeCountryRosterEntry::new("Russians", Some("Russia")),
            CooperativeCountryRosterEntry::new("YuriCountry", Some("YuriCountry")),
        ]
    }

    #[test]
    fn parses_campaigns_in_source_order_and_native_key_spelling() {
        let registry = registry();
        assert_eq!(registry.campaigns.len(), 2);
        assert_eq!(registry.campaigns[0].section, "Allied");
        assert_eq!(registry.campaigns[1].section, "World");
        assert_eq!(registry.campaigns[0].campaign_name, "Name:All_Coop");
        assert_eq!(registry.campaigns[0].load_screen, "LSA.shp");
        assert_eq!(registry.campaigns[0].load_screen_pallet, "LSA.pal");
        assert_eq!(
            registry.campaigns[0].ai_difficulty,
            CooperativeAiDifficulty::Normal
        );
        assert_eq!(
            registry.campaigns[1].ai_difficulty,
            CooperativeAiDifficulty::Easy
        );
        assert_eq!(registry.campaigns[0].stages[1].map_variants[2], "A2C");
    }

    #[test]
    fn rejects_stage_with_fewer_than_three_native_variants() {
        let ini = IniFile::from_str(
            "[Campaigns]\n1=Broken\n[Broken]\nNumberOfCampaignMaps=1\nMap1=One,Two\n",
        );
        assert_eq!(
            CooperativeRegistry::from_ini(&ini),
            Err(CooperativeError::InvalidMapVariantCount {
                campaign: "Broken".to_string(),
                stage: 1,
                found: 2,
            })
        );
    }

    #[test]
    fn start_partner_binding_is_a_derived_snapshot_without_progress_or_rng_mutation() {
        let registry = registry();
        let mut rng = SimRng::new(19);
        let mut state = CooperativeProgressState::construct(&registry, &mut rng).unwrap();
        assert!(!state.bind_network_partners(&["Host", "Guest"]));
        state.clear_network_partners();
        assert!(state.active().is_none());
        state.join(&registry, &mut rng, &[], false);
        let before = state.active().unwrap().clone();
        let rng_before = rng.state();
        assert!(state.bind_network_partners(&["H\u{0131}", "G\u{0100}ignored"]));
        let active = state.active().unwrap();
        assert_eq!(active.partner_names(), [b"H1".as_slice(), b"G".as_slice()]);
        assert_eq!(active.campaign_type, before.campaign_type);
        assert_eq!(active.current_map, before.current_map);
        assert_eq!(active.chosen_maps, before.chosen_maps);
        assert_eq!(active.valid, before.valid);
        assert_eq!(rng.state(), rng_before);
        assert!(!state.bind_network_partners(&["OnlyOne"]));
        assert_eq!(
            state.active().unwrap().partner_names(),
            [b"H1".as_slice(), b"G".as_slice()]
        );
        state
            .active
            .as_mut()
            .unwrap()
            .initialize_for_campaign(&registry, 0, &mut rng)
            .unwrap();
        assert_eq!(
            state.active().unwrap().partner_names(),
            [b"H1".as_slice(), b"G".as_slice()]
        );
        // 0x0049CAF0 on the same campaign keeps the maps but resets the stage.
        assert_eq!(state.active().unwrap().current_map, -1);
        assert_eq!(state.active().unwrap().chosen_maps, before.chosen_maps);
        state.clear_network_partners();
        assert_eq!(
            state.active().unwrap().partner_names(),
            [b"".as_slice(), b"".as_slice()]
        );
        assert_eq!(rng.state(), rng_before);
    }

    #[test]
    fn fresh_progress_draws_once_per_stage_and_same_type_does_not_reroll() {
        let registry = registry();
        let mut rng = SimRng::new(0x1234);
        let mut reference = rng.clone();
        let expected: Vec<String> = registry.campaigns[0]
            .stages
            .iter()
            .map(|stage| {
                let variant = reference.next_range_u32_inclusive(0, 2) as usize;
                stage.map_variants[variant].clone()
            })
            .collect();

        let mut progress = CooperativeProgressRecord::default();
        assert!(
            progress
                .initialize_for_campaign(&registry, 0, &mut rng)
                .unwrap()
        );
        assert_eq!(progress.chosen_maps, expected);
        assert_eq!(rng.state(), reference.state());

        progress.current_map = 1;
        let before = rng.state();
        let chosen_before = progress.chosen_maps.clone();
        assert!(
            !progress
                .initialize_for_campaign(&registry, 0, &mut rng)
                .unwrap()
        );
        assert_eq!(progress.current_map, -1);
        assert_eq!(progress.chosen_maps, chosen_before);
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn country_draw_uses_global_bound_and_unbounded_retries() {
        let registry = registry();
        let progress = CooperativeProgressRecord {
            valid: true,
            campaign_type: 0,
            current_map: 0,
            ..Default::default()
        };
        let roster = roster();

        let mut rng = SimRng::new(31);
        let mut reference = rng.clone();
        let expected = loop {
            let candidate = reference.next_range_u32_inclusive(0, 3) as usize;
            if candidate == 0 || candidate == 1 {
                break candidate;
            }
        };
        let actual = draw_country_for_progress(
            &registry,
            &progress,
            CooperativeCountryRole::Player,
            &roster,
            &mut rng,
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(rng.state(), reference.state());
    }

    #[test]
    fn invalid_progress_returns_zero_without_rng_and_invalid_list_returns_error() {
        let registry = registry();
        let roster = roster();
        let mut rng = SimRng::new(5);
        let before = rng.state();
        assert_eq!(
            draw_country_for_progress(
                &registry,
                &CooperativeProgressRecord::default(),
                CooperativeCountryRole::Player,
                &roster,
                &mut rng,
            )
            .unwrap(),
            0
        );
        assert_eq!(rng.state(), before);

        let progress = CooperativeProgressRecord {
            valid: true,
            campaign_type: 0,
            current_map: 1,
            ..Default::default()
        };
        let no_match_roster = vec![CooperativeCountryRosterEntry::new(
            "Russians",
            Some("Russia"),
        )];
        let before = rng.state();
        assert_eq!(
            draw_country_for_progress(
                &registry,
                &progress,
                CooperativeCountryRole::Player,
                &no_match_roster,
                &mut rng,
            ),
            Err(CooperativeError::NoEligibleCountries { role: "player" })
        );
        assert_eq!(rng.state(), before);
    }

    fn context<'a>(
        scenarios: &'a [&'a str],
        nodes: &'a [&'a str],
        chooser_on_top: bool,
    ) -> CooperativeSlotContext<'a> {
        CooperativeSlotContext {
            scenarios,
            nodes,
            chooser_on_top,
        }
    }

    /// The inclusive `(0,2)` draws `run` makes, replayed on a clone: one
    /// range draw may take several raw words.
    fn draws_of(rng: &mut SimRng, run: impl FnOnce(&mut SimRng)) -> Vec<u32> {
        let mut replay = rng.clone();
        run(rng);
        let mut draws = Vec::new();
        while replay.state() != rng.state() {
            draws.push(replay.next_range_u32_inclusive(0, 2));
            assert!(draws.len() < 1000, "not (0,2) draws");
        }
        draws
    }

    #[test]
    fn c0_text_writes_variants_sentinel_and_encoded_names() {
        let registry = registry();
        let record = CooperativeProgressRecord {
            partner_names: [b"Host 1".to_vec(), b"G,x".to_vec()],
            valid: true,
            current_map: 1,
            campaign_type: 0,
            chosen_maps: vec!["A1B".to_string(), "a2c".to_string()],
            tallies: [[1, 2, 3, 4], [5, 6, 7, 8]],
            time: 77,
        };
        let text = record.campaign_text(&registry).unwrap();
        assert_eq!(text, "C0,0,1,77,1,2,3,4,5,6,7,8,1,2,-1,Host=201,G=2Cx");
        assert_eq!(
            CooperativeCampaignText::parse(&text).unwrap(),
            CooperativeCampaignText {
                campaign_type: 0,
                current_map: 1,
                time: 77,
                tallies: [[1, 2, 3, 4], [5, 6, 7, 8]],
                variants: vec![1, 2],
                names: [b"Host 1".to_vec(), b"G,x".to_vec()],
            }
        );
        // 0x0049D680 writes nothing for an invalid record or unknown type.
        let invalid = CooperativeProgressRecord {
            valid: false,
            ..record.clone()
        };
        assert_eq!(invalid.campaign_text(&registry), None);
        let unknown = CooperativeProgressRecord {
            campaign_type: 2,
            ..record
        };
        assert_eq!(unknown.campaign_text(&registry), None);
        // Both names are required.
        assert_eq!(
            CooperativeCampaignText::parse("C0,0,1,77,1,2,3,4,5,6,7,8,1,2,-1,Host"),
            None
        );
    }

    #[test]
    fn c0_names_encode_hex_and_stop_at_high_bytes() {
        assert_eq!(encode_name(b"Ab 9"), "Ab=209");
        assert_eq!(encode_name(&[b'x', 0xE9, b'y']), "x=");
        assert_eq!(encode_name(b"a\0b"), "a");
        assert_eq!(decode_name(b"a=2c=4"), vec![b'a', 0x2C, 0x04]);
        assert_eq!(decode_name(b"ab="), b"ab".to_vec());
        assert_eq!(decode_name(b"=G1"), vec![0x01]);
    }

    #[test]
    fn received_c0_swaps_names_and_counter_rows() {
        let registry = registry();
        let text =
            CooperativeCampaignText::parse("C0,0,1,77,1,2,3,4,5,6,7,8,1,2,-1,Host=201,G=2Cx")
                .unwrap();
        let mut rng = SimRng::new(5);
        let mut state = CooperativeProgressState::default();
        // 0x005C3EC0 recreates the missing record for campaign 0 first.
        let draws = draws_of(&mut rng, |rng| {
            state.receive_campaign(&registry, rng, &["G,x", "Host 1"], &text)
        });
        assert_eq!(draws.len(), 2);
        let active = state.active().unwrap();
        assert!(active.valid);
        assert_eq!(active.campaign_type, 0);
        assert_eq!(active.current_map, 0);
        assert_eq!(active.chosen_maps, ["A1B", "A2C"]);
        assert_eq!(
            active.partner_names(),
            [b"G,x".as_slice(), b"Host 1".as_slice()]
        );
        assert_eq!(state.campaign(), 0);

        assert!(state.receive_stage(&text, true));
        let active = state.active().unwrap();
        assert_eq!(active.current_map, 1);
        assert_eq!(active.tallies, [[5, 6, 7, 8], [1, 2, 3, 4]]);
        assert_eq!(active.time, 77);
        // Same stage and nothing pending: no refresh; counters add again.
        assert!(!state.receive_stage(&text, true));
        assert_eq!(
            state.active().unwrap().tallies,
            [[10, 12, 14, 16], [2, 4, 6, 8]]
        );
        // A stage past the chosen maps changes nothing.
        let mut far = text.clone();
        far.current_map = 2;
        assert!(!state.receive_stage(&far, false));
        assert_eq!(state.active().unwrap().current_map, 1);

        // A variant count unlike the stage count resets and keeps the type.
        let mut short = text.clone();
        short.variants = vec![1];
        state.receive_campaign(&registry, &mut rng, &[], &short);
        let active = state.active().unwrap();
        assert_eq!(active.current_map, -1);
        assert_eq!(active.tallies, [[0; 4]; 2]);
        assert_eq!(active.chosen_maps, ["A1B", "A2C"]);
    }

    #[test]
    fn campaign_of_scenario_takes_the_last_first_stage_prefix_and_never_scenario_zero() {
        let registry = registry();
        let scenarios = ["A1.MAP", "x.map", "w1c.map", "A1BX.MAP"];
        let campaign = |current| {
            CooperativeProgressState::campaign_of_scenario(&registry, &scenarios, current)
        };
        assert_eq!(campaign(0), -1);
        assert_eq!(campaign(1), -1);
        assert_eq!(campaign(2), 1);
        assert_eq!(campaign(3), 0);
        assert_eq!(campaign(9), -1);
    }

    #[test]
    fn entering_cooperative_off_campaign_selects_the_first_map_scenario() {
        let registry = registry();
        let scenarios = ["A1.MAP", "x.map", "a1b.map", "A1C.MAP"];
        let mut rng = SimRng::new(0x77);
        let mut reference = rng.clone();
        let first = registry.campaigns[0].stages[0].map_variants
            [reference.next_range_u32_inclusive(0, 2) as usize]
            .clone();
        reference.next_range_u32_inclusive(0, 2);
        let expected = scenarios
            .iter()
            .position(|file| scenario_starts_with(file, &first))
            .unwrap();
        let mut state = CooperativeProgressState::default();
        state.campaign = 1;
        state.refresh_pending = false;
        let selected = state.enter_mode(
            &registry,
            &mut rng,
            context(&scenarios, &[], false),
            1,
            true,
        );
        assert_eq!(selected, Some(expected));
        assert_eq!(rng.state(), reference.state(), "one draw per stage");
        assert_eq!(state.campaign(), 0);
        assert!(state.refresh_pending);
        assert_eq!(state.active().unwrap().chosen_map(0), Some(first.as_str()));
    }

    #[test]
    fn entering_cooperative_on_a_campaign_map_rerolls_until_its_first_map_matches() {
        let registry = registry();
        // Only variant "W1" starts "W1.MAP" ("W1B" and "W1C" do not).
        let scenarios = ["A1.MAP", "x.map", "W1.MAP"];
        for seed in 0..8 {
            let mut rng = SimRng::new(seed);
            let mut state = CooperativeProgressState::default();
            let mut selected = Some(0);
            let draws = draws_of(&mut rng, |rng| {
                selected =
                    state.enter_mode(&registry, rng, context(&scenarios, &[], false), 2, false);
            });
            assert_eq!(selected, None);
            assert_eq!(state.campaign(), 1);
            let active = state.active().unwrap();
            assert_eq!(active.campaign_type, 1);
            assert_eq!(active.chosen_map(0), Some("W1"));
            // Campaign 0's two draws, then per pass: campaign 0 again (none on
            // the first pass, it is already type 0) and World's one draw.
            let world = &draws[2..];
            assert_eq!(world.len() % 3, 1, "seed {seed}: {draws:?}");
            assert_eq!(*world.last().unwrap(), 0);
            assert!(
                world
                    .iter()
                    .step_by(3)
                    .rev()
                    .skip(1)
                    .all(|&value| value != 0)
            );
        }
    }

    #[test]
    fn entering_cooperative_keeps_the_record_only_under_choose_map() {
        let registry = registry();
        let scenarios = ["A1.MAP", "x.map", "W1C.MAP"];
        let mut rng = SimRng::new(3);
        let mut state = CooperativeProgressState::default();
        state.active = Some(CooperativeProgressRecord {
            valid: true,
            campaign_type: 1,
            current_map: 0,
            chosen_maps: vec!["W1C".to_string()],
            ..Default::default()
        });
        let draws = draws_of(&mut rng, |rng| {
            state.enter_mode(
                &registry,
                rng,
                context(&scenarios, &["Host", "Guest"], true),
                2,
                true,
            );
        });
        assert!(draws.is_empty());
        assert_eq!(
            state.active().unwrap().partner_names(),
            [b"Host".as_slice(), b"Guest".as_slice()]
        );
        // Without Choose Map on top the record goes and a new one is made.
        let draws = draws_of(&mut rng, |rng| {
            state.enter_mode(
                &registry,
                rng,
                context(&scenarios, &["Host"], false),
                2,
                true,
            );
        });
        assert!(draws.len() >= 3);
        assert_eq!(
            state.active().unwrap().partner_names(),
            [b"".as_slice(), b"".as_slice()]
        );
    }

    #[test]
    fn default_scenario_initializes_campaign_zero_or_rebuilds_reserves() {
        let registry = registry();
        let mut rng = SimRng::new(11);
        let mut state = CooperativeProgressState::default();
        let mut found = None;
        let draws = draws_of(&mut rng, |rng| {
            found =
                state.default_scenario(&registry, rng, &["x.map", "A1.MAP", "A1B.MAP", "A1C.MAP"]);
        });
        assert_eq!(draws.len(), 2);
        let token = state.active().unwrap().chosen_map(0).unwrap().to_string();
        // The first scenario the token starts: "A1.MAP" also starts "A1".
        let expected = match token.as_str() {
            "A1" => 1,
            "A1B" => 2,
            _ => 3,
        };
        assert_eq!(found, Some(expected));
        let mut state = CooperativeProgressState::default();
        let draws = draws_of(&mut rng, |rng| {
            found = state.default_scenario(&registry, rng, &["x.map"]);
        });
        assert_eq!(found, None);
        assert_eq!(draws.len(), 2 + 3, "campaign 0, then +0x10's reserves");
        assert!(state.reserve(1).is_some());
        let empty = CooperativeRegistry::default();
        assert_eq!(state.default_scenario(&empty, &mut rng, &["A1.MAP"]), None);
    }

    #[test]
    fn chooser_rows_list_campaigns_whose_reserve_map_is_a_scenario() {
        let registry = CooperativeRegistry::from_ini(&IniFile::from_str(
            "[Campaigns]\nCampaign1=Allied\nCampaign2=World\n\
             [Allied]\nNumberOfCampaignMaps=1\nCampaignPlayer1=Americans\n\
             Map1=C1A01MD,C1A01MD,C1A01MD\n\
             [World]\nNumberOfCampaignMaps=1\nCampaignPlayer1=Americans\n\
             Map1=C4W01MD,C4W01MD,C4W01MD\n",
        ))
        .unwrap();
        let mut rng = SimRng::new(1);
        let mut state = CooperativeProgressState::default();
        let records = [(4, "c1a01md.map"), (7, "C1A01MDX.MAP"), (9, "C4W01MD.MPR")];
        let rows = state.chooser_rows(&registry, &mut rng, &["Host", "Guest"], &records);
        assert_eq!(
            rows,
            [CooperativeChooserRow {
                campaign: 0,
                record: 4
            }]
        );
        let active = state.active().unwrap();
        assert!(!active.valid, "0x0049B660 makes a record with no campaign");
        assert_eq!(
            active.partner_names(),
            [b"Host".as_slice(), b"Guest".as_slice()]
        );
        assert!(state.reserve(0).is_some() && state.reserve(1).is_some());
        // A present record loses its names unless there are two nodes.
        state.chooser_rows(&registry, &mut rng, &["Host"], &records);
        assert_eq!(
            state.active().unwrap().partner_names(),
            [b"".as_slice(), b"".as_slice()]
        );
    }

    #[test]
    fn using_a_campaign_row_takes_its_reserve_and_draws_a_replacement() {
        let registry = registry();
        let mut rng = SimRng::new(21);
        let mut state = CooperativeProgressState::construct(&registry, &mut rng).unwrap();
        assert!(!state.accept_chooser_row(&registry, &mut rng, context(&[], &[], true), 1));
        state.active = Some(CooperativeProgressRecord::default());
        let reserve = state.reserve(1).unwrap().clone();
        let draws = draws_of(&mut rng, |rng| {
            assert!(state.accept_chooser_row(
                &registry,
                rng,
                context(&[], &["Host", "Guest"], true),
                1
            ));
        });
        assert_eq!(draws.len(), 1, "World's one stage");
        let active = state.active().unwrap();
        assert_eq!(active.chosen_maps, reserve.chosen_maps);
        assert_eq!(
            active.partner_names(),
            [b"Host".as_slice(), b"Guest".as_slice()]
        );
        assert_eq!(state.reserve_taken, 1);
        assert!(state.reserve(1).unwrap().valid);
        // Not under Choose Map: nothing is taken.
        let before = state.clone();
        let draws = draws_of(&mut rng, |rng| {
            state.accept_chooser_row(&registry, rng, context(&[], &[], false), 0);
        });
        assert!(draws.is_empty());
        assert_eq!(state, before);
    }

    #[test]
    fn stage_countries_admit_and_default_from_the_setup_campaign() {
        let registry = registry();
        let roster = roster();
        let mut state = CooperativeProgressState::default();
        state.active = Some(CooperativeProgressRecord {
            valid: true,
            campaign_type: 0,
            current_map: 0,
            chosen_maps: vec!["A1".to_string(), "A2".to_string()],
            ..Default::default()
        });
        assert!(state.admits_country(&registry, &roster, -2));
        assert!(state.admits_country(&registry, &roster, -3));
        assert!(state.admits_country(&registry, &roster, 1));
        assert!(!state.admits_country(&registry, &roster, 2));
        assert_eq!(state.default_country(&registry, &roster), -2);
        assert_eq!(state.description(), Some((0, 0)));

        state.active.as_mut().unwrap().current_map = 1;
        assert!(!state.admits_country(&registry, &roster, -2));
        assert!(state.admits_country(&registry, &roster, 0));
        assert_eq!(state.default_country(&registry, &roster), 0);

        // +0x33C outside the registry: everything passes, Random default.
        state.campaign = 5;
        assert!(state.admits_country(&registry, &roster, 2));
        assert_eq!(state.default_country(&registry, &roster), -2);
        assert_eq!(state.setup_countries(&registry), None);
    }

    #[test]
    fn publish_copies_the_valid_type_and_takes_a_pending_refresh_only_with_setup() {
        let registry = registry();
        let mut state = CooperativeProgressState::default();
        assert_eq!(state.publish(&registry, true), None);
        state.active = Some(CooperativeProgressRecord {
            valid: true,
            campaign_type: 1,
            current_map: 0,
            chosen_maps: vec!["W1B".to_string()],
            ..Default::default()
        });
        let (refresh, text) = state.publish(&registry, false).unwrap();
        assert!(!refresh);
        assert_eq!(text.as_deref(), Some("C0,1,0,0,0,0,0,0,0,0,0,0,1,-1,,"));
        assert_eq!(state.campaign(), 1);
        assert!(state.refresh_pending);
        assert!(state.publish(&registry, true).unwrap().0);
        assert!(!state.publish(&registry, true).unwrap().0);
    }

    fn won_record() -> CooperativeProgressRecord {
        let mut record = CooperativeProgressRecord::default();
        record
            .initialize_for_campaign(&registry(), 0, &mut SimRng::new(4))
            .unwrap();
        record.bind_partners("Host", "Guest");
        record
    }

    fn mission_row(
        name: &str,
        kills: i32,
        built: i32,
        lost: i32,
        score: i32,
    ) -> CooperativeMissionRow {
        CooperativeMissionRow {
            name: name.as_bytes().to_vec(),
            kills,
            built,
            lost,
            score,
        }
    }

    #[test]
    fn a_won_mission_adds_rows_by_the_first_name_and_advances_the_stage() {
        let mut record = won_record();
        let guest = mission_row("Guest", 1, 2, 3, 4);
        let host = mission_row("Host", 10, 20, 30, 40);
        // Row 0 is not the first name: the rows swap (0x0049B930).
        assert!(record.record_win([&guest, &host], 600));
        assert_eq!(record.tallies, [[10, 20, 30, 40], [1, 2, 3, 4]]);
        assert_eq!((record.current_map, record.time), (1, 600));
        assert!(record.can_continue());
        assert_eq!(record.next_scenario_token(), record.chosen_map(1));
        // A case difference is not the same name.
        let other = mission_row("host", 1, 1, 1, 1);
        assert!(record.record_win([&other, &guest], 60));
        assert_eq!(record.tallies, [[11, 22, 33, 44], [2, 3, 4, 5]]);
        assert_eq!(record.current_map, 2);
        assert!(!record.can_continue());
        assert_eq!(record.next_scenario_token(), None);

        let mut invalid = CooperativeProgressRecord::default();
        assert!(!invalid.record_win([&host, &guest], 60));
        assert_eq!(invalid.current_map, -1);
    }

    #[test]
    fn the_coop_save_writes_keys_in_native_order_and_skips_the_last_stage() {
        let registry = registry();
        let mut record = won_record();
        let host = mission_row("Host", 10, 20, 30, 40);
        let guest = mission_row("Guest", 1, 2, 3, 4);
        record.record_win([&host, &guest], 600);
        let CoopSave::Write { section, values } =
            record.coop_save(&registry, &roster(), [(0, 3), (-2, 5)])
        else {
            panic!("a mid-campaign win writes");
        };
        assert_eq!(section, "HostGuest");
        let maps = (record.chosen_map(0).unwrap(), record.chosen_map(1).unwrap());
        let keys: Vec<(&str, &str)> = values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("CurrentMap", "1"),
                ("CampaignType", "0"),
                ("Map1", maps.0),
                ("Map2", maps.1),
                ("House1", "Americans"),
                ("Color1", "3"),
                ("House2", "<random>"),
                ("Color2", "5"),
                ("Time", "600"),
                ("Kills1", "10"),
                ("Kills2", "1"),
                ("Built1", "20"),
                ("Built2", "2"),
                ("Lost1", "30"),
                ("Lost2", "3"),
                ("Score1", "40"),
                ("Score2", "4"),
            ]
        );
        record.record_win([&host, &guest], 60);
        assert_eq!(
            record.coop_save(&registry, &roster(), [(0, 3), (0, 5)]),
            CoopSave::Unchanged
        );
        record.bind_partners("Host", "");
        assert_eq!(
            record.coop_save(&registry, &roster(), [(0, 3), (0, 5)]),
            CoopSave::Failed,
            "a departed partner's empty name fails the save"
        );
    }

    #[test]
    fn house_type_text_follows_put_house_type() {
        let roster = roster();
        assert_eq!(house_type_text(1, &roster), "British");
        assert_eq!(house_type_text(-2, &roster), "<random>");
        assert_eq!(house_type_text(-3, &roster), "<random>");
        assert_eq!(house_type_text(-1, &roster), "<none>");
        assert_eq!(house_type_text(9, &roster), "<none>");
        assert_eq!(house_type_text(0x117B, &roster), "<Player @ A>");
        assert_eq!(house_type_text(0x1182, &roster), "<Player @ H>");
    }
}
