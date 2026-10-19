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
    #[error("campaign index {index} has no preconstructed reserve progress record")]
    MissingReserveProgress { index: usize },
    #[error(
        "Cooperative progress selected map '{scenario}', but it is absent from the loadable Skirmish map registry"
    )]
    MissingScenarioMap { scenario: String },
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

    /// Match any of the three source variants, preserving the registry's first
    /// source-order match. Scenario filenames are ASCII case-insensitive.
    pub fn campaign_for_map(&self, scenario: &str) -> Option<usize> {
        self.campaign_stage_for_map(scenario)
            .map(|(campaign, _)| campaign)
    }

    /// Resolve both the source-order campaign and stage for a concrete variant.
    /// The active progress record needs the stage index as its `CurrentMap`.
    pub fn campaign_stage_for_map(&self, scenario: &str) -> Option<(usize, usize)> {
        self.campaigns
            .iter()
            .enumerate()
            .find_map(|(campaign_index, campaign)| {
                campaign
                    .stages
                    .iter()
                    .position(|stage| {
                        stage
                            .map_variants
                            .iter()
                            .any(|variant| variant.eq_ignore_ascii_case(scenario))
                    })
                    .map(|stage_index| (campaign_index, stage_index))
            })
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
}

impl Default for CooperativeProgressRecord {
    fn default() -> Self {
        Self {
            partner_names: [Vec::new(), Vec::new()],
            valid: false,
            current_map: -1,
            campaign_type: -1,
            chosen_maps: Vec::new(),
        }
    }
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

    /// Initialize/reset a record for a source-order campaign index.
    ///
    /// A changed (including fresh `-1`) CampaignType makes exactly one logical
    /// inclusive `(0,2)` call per stage. Reinitializing the same CampaignType
    /// resets progress to map zero without rerolling or advancing the RNG.
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
        self.current_map = 0;
        if self.campaign_type == campaign_index as i32 {
            return Ok(false);
        }

        let mut chosen_maps = Vec::with_capacity(campaign.stages.len());
        for stage in &campaign.stages {
            let variant = rng.next_range_u32_inclusive(0, 2) as usize;
            chosen_maps.push(stage.map_variants[variant].clone());
        }
        self.chosen_maps = chosen_maps;
        self.campaign_type = campaign_index as i32;
        Ok(true)
    }

    pub fn chosen_map(&self, map_index: usize) -> Option<&str> {
        self.chosen_maps.get(map_index).map(String::as_str)
    }

    pub fn current_chosen_map(&self) -> Option<&str> {
        usize::try_from(self.current_map)
            .ok()
            .and_then(|index| self.chosen_map(index))
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

/// Cooperative mode object progress ownership relevant to shell RNG parity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CooperativeProgressState {
    active: Option<CooperativeProgressRecord>,
    reserves: Vec<Option<CooperativeProgressRecord>>,
    round_assignments: CooperativeRoundAssignments,
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
            active: None,
            reserves,
            round_assignments: CooperativeRoundAssignments::default(),
        })
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

    #[cfg(test)]
    pub fn reserve(&self, campaign_index: usize) -> Option<&CooperativeProgressRecord> {
        self.reserves.get(campaign_index)?.as_ref()
    }

    /// Cooperative open creates a separately initialized active record only
    /// when the active pointer is null.
    pub fn ensure_active(
        &mut self,
        registry: &CooperativeRegistry,
        campaign_index: usize,
        rng: &mut SimRng,
    ) -> Result<&CooperativeProgressRecord, CooperativeError> {
        if self.active.is_none() {
            let mut record = CooperativeProgressRecord::default();
            record.initialize_for_campaign(registry, campaign_index, rng)?;
            self.active = Some(record);
        }
        Ok(self.active.as_ref().expect("active record was initialized"))
    }

    /// Accepted Choose Map campaign switching moves the selected preconstructed
    /// reserve into the active slot, then constructs a fresh replacement reserve
    /// for that campaign. The replacement consumes one `(0,2)` call per stage.
    pub fn accept_campaign_swap(
        &mut self,
        registry: &CooperativeRegistry,
        campaign_index: usize,
        rng: &mut SimRng,
    ) -> Result<&CooperativeProgressRecord, CooperativeError> {
        if campaign_index >= registry.campaigns.len() {
            return Err(CooperativeError::InvalidCampaignIndex {
                index: campaign_index,
                count: registry.campaigns.len(),
            });
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.campaign_type == campaign_index as i32)
        {
            return Ok(self.active.as_ref().expect("selected campaign is active"));
        }
        let reserve_slot = self.reserves.get_mut(campaign_index).ok_or(
            CooperativeError::MissingReserveProgress {
                index: campaign_index,
            },
        )?;
        let selected = reserve_slot
            .take()
            .ok_or(CooperativeError::MissingReserveProgress {
                index: campaign_index,
            })?;
        self.active = Some(selected);

        let mut replacement = CooperativeProgressRecord::default();
        replacement.initialize_for_campaign(registry, campaign_index, rng)?;
        *reserve_slot = Some(replacement);
        Ok(self
            .active
            .as_ref()
            .expect("selected reserve became active"))
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
        assert_eq!(registry.campaign_for_map("a2b"), Some(0));
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
        state.ensure_active(&registry, 0, &mut rng).unwrap();
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
        state.clear_network_partners();
        assert_eq!(state.active().unwrap(), &before);
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
        assert_eq!(progress.current_map, 0);
        assert_eq!(progress.chosen_maps, chosen_before);
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn construction_initializes_every_reserve_and_swap_resamples_replacement() {
        let registry = registry();
        let mut rng = SimRng::new(77);
        let mut reference = rng.clone();
        for campaign in &registry.campaigns {
            for _ in &campaign.stages {
                reference.next_range_u32_inclusive(0, 2);
            }
        }
        let mut state = CooperativeProgressState::construct(&registry, &mut rng).unwrap();
        assert_eq!(rng.state(), reference.state());

        let selected_reserve = state.reserve(1).unwrap().clone();
        for _ in &registry.campaigns[1].stages {
            reference.next_range_u32_inclusive(0, 2);
        }
        let active = state.accept_campaign_swap(&registry, 1, &mut rng).unwrap();
        assert_eq!(*active, selected_reserve);
        assert!(state.reserve(1).is_some());
        assert_eq!(rng.state(), reference.state());
    }

    #[test]
    fn ensure_active_only_draws_when_pointer_is_null() {
        let registry = registry();
        let mut rng = SimRng::new(9);
        let mut state = CooperativeProgressState::construct(&registry, &mut rng).unwrap();
        state.ensure_active(&registry, 0, &mut rng).unwrap();
        let after_first = rng.state();
        state.ensure_active(&registry, 1, &mut rng).unwrap();
        assert_eq!(rng.state(), after_first);
        assert_eq!(state.active().unwrap().campaign_type, 0);
    }

    #[test]
    fn accepting_current_campaign_does_not_replace_or_resample() {
        let registry = registry();
        let mut rng = SimRng::new(19);
        let mut state = CooperativeProgressState::construct(&registry, &mut rng).unwrap();
        state.ensure_active(&registry, 0, &mut rng).unwrap();
        let before_rng = rng.state();
        let before_reserve = state.reserve(0).unwrap().clone();

        state.accept_campaign_swap(&registry, 0, &mut rng).unwrap();

        assert_eq!(rng.state(), before_rng);
        assert_eq!(*state.reserve(0).unwrap(), before_reserve);
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
    fn country_name_alias_resolves_to_global_roster_index() {
        let registry = registry();
        let progress = CooperativeProgressRecord {
            valid: true,
            campaign_type: 1,
            current_map: 0,
            ..Default::default()
        };
        let mut rng = SimRng::new(1);
        assert_eq!(
            draw_country_for_progress(
                &registry,
                &progress,
                CooperativeCountryRole::Player,
                &roster(),
                &mut rng,
            )
            .unwrap(),
            0
        );
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
}
