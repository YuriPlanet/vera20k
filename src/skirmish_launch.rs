//! App-level skirmish launch contract.
//!
//! This module is intentionally data-only: it packages the lobby state the app
//! needs to create stock-offline houses and initial spawns without making `sim/`
//! depend on UI, rendering, audio, or networking modules.

use serde::{Deserialize, Serialize};

use crate::sim::game_options::GameOptions;
use crate::sim::rng::SimRng;
use crate::skirmish_modes::SkirmishGameMode;

pub const SKIRMISH_PLAYER_SLOT_COUNT: usize = 8;
pub const SKIRMISH_AI_SLOT_COUNT: usize = SKIRMISH_PLAYER_SLOT_COUNT - 1;
pub const HOUSE_COLOR_COUNT: usize = 8;

/// One human node in the native pre-Fill House-construction roster.
///
/// Nodes are stably sorted by the signed color DWORD (CreateHouses687F66..84). Observers still
/// construct a House (and therefore consume the constructor timer draw), but
/// do not increase Gather's required-start count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreFillHumanHouse {
    pub priority: i32,
    pub source_order: u8,
    pub observer: bool,
}

/// One native AI session slot before active opponents are compacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreFillAiHouseSlot {
    pub slot_index: u8,
    pub valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum PreFillFixedHouse {
    Neutral,
    Special,
}

/// Immutable House roster consumed identically by both noncampaign pre-Fill
/// construction passes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreFillHouseRoster {
    human_nodes: Vec<PreFillHumanHouse>,
    ai_slots: Vec<PreFillAiHouseSlot>,
    fixed_tail: [PreFillFixedHouse; 2],
}

impl PreFillHouseRoster {
    pub fn new(
        mut human_nodes: Vec<PreFillHumanHouse>,
        mut ai_slots: Vec<PreFillAiHouseSlot>,
    ) -> Self {
        // Native sorts by the signed color DWORD and preserves source-list
        // order for ties. `source_order` makes that ordering independent of
        // whichever app adapter assembled this normalized value.
        human_nodes.sort_by_key(|node| (node.priority, node.source_order));
        ai_slots.sort_by_key(|slot| slot.slot_index);
        Self {
            human_nodes,
            ai_slots,
            fixed_tail: [PreFillFixedHouse::Neutral, PreFillFixedHouse::Special],
        }
    }

    /// Compatibility constructor for callers that already hold the compact
    /// ordinary-skirmish session. Production shell packing retains all seven
    /// raw AI slots and does not use this shortcut.
    pub fn from_compact_skirmish(active_ai_count: usize) -> Self {
        let ai_slots = (0..SKIRMISH_PLAYER_SLOT_COUNT)
            .map(|slot_index| PreFillAiHouseSlot {
                slot_index: slot_index as u8,
                valid: slot_index > 0 && slot_index <= active_ai_count,
            })
            .collect();
        Self::new(
            vec![PreFillHumanHouse {
                priority: 0,
                source_order: 0,
                observer: false,
            }],
            ai_slots,
        )
    }

    pub fn human_nodes(&self) -> &[PreFillHumanHouse] {
        &self.human_nodes
    }

    pub fn ai_slots(&self) -> &[PreFillAiHouseSlot] {
        &self.ai_slots
    }

    pub fn fixed_tail(&self) -> &[PreFillFixedHouse; 2] {
        &self.fixed_tail
    }

    pub fn created_house_count(&self) -> usize {
        self.human_nodes.len()
            + self.ai_slots.iter().filter(|slot| slot.valid).count()
            + self.fixed_tail.len()
    }

    pub fn nonobserver_house_count(&self) -> usize {
        self.nonobserver_human_count() + self.ai_slots.iter().filter(|slot| slot.valid).count()
    }

    pub fn nonobserver_human_count(&self) -> usize {
        self.human_nodes
            .iter()
            .filter(|node| !node.observer)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "SkirmishLaunchModeInput")]
pub struct SkirmishLaunchMode {
    pub class: crate::skirmish_modes::MpModeClass,
    pub id: i32,
    pub ui_name_key: String,
    pub tooltip_key: String,
    pub override_file: String,
    pub map_filter: String,
    pub random_maps_allowed: bool,
    pub allies_allowed: bool,
    pub must_ally: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkirmishLaunchModeInput {
    #[serde(default)]
    class: Option<crate::skirmish_modes::MpModeClass>,
    id: i32,
    ui_name_key: String,
    tooltip_key: String,
    override_file: String,
    map_filter: String,
    random_maps_allowed: bool,
    allies_allowed: bool,
    must_ally: bool,
}

impl TryFrom<SkirmishLaunchModeInput> for SkirmishLaunchMode {
    type Error = String;
    fn try_from(input: SkirmishLaunchModeInput) -> Result<Self, Self::Error> {
        use crate::skirmish_modes::MpModeClass;
        // Compatibility only: original MPModesMD catalog identities read by5D7CE0.
        // Production catalog conversion always supplies class explicitly.
        let class = input
            .class
            .or_else(|| {
                let (file, class) = match input.id {
                    1 => ("MPBattleMD.ini", MpModeClass::Battle),
                    2 => ("MPFreeForAllMD.ini", MpModeClass::FreeForAll),
                    3 => ("MPCoopMD.ini", MpModeClass::Cooperative),
                    4 => ("MPUnholyMD.ini", MpModeClass::Unholy),
                    5 => ("MPMWMD.ini", MpModeClass::ManBattle),
                    6 => ("MPDuelMD.ini", MpModeClass::ManBattle),
                    7 => ("MPMeatMD.ini", MpModeClass::ManBattle),
                    8 => ("MPNavalMD.ini", MpModeClass::ManBattle),
                    9 => ("MPTeamMD.ini", MpModeClass::Battle),
                    _ => return None,
                };
                input
                    .override_file
                    .eq_ignore_ascii_case(file)
                    .then_some(class)
            })
            .ok_or_else(|| "legacy custom launch mode requires an explicit class".to_owned())?;
        Ok(Self {
            class,
            id: input.id,
            ui_name_key: input.ui_name_key,
            tooltip_key: input.tooltip_key,
            override_file: input.override_file,
            map_filter: input.map_filter,
            random_maps_allowed: input.random_maps_allowed,
            allies_allowed: input.allies_allowed,
            must_ally: input.must_ally,
        })
    }
}

impl SkirmishLaunchMode {
    pub fn from_game_mode(mode: &SkirmishGameMode) -> Self {
        Self {
            class: mode.class,
            id: mode.id,
            ui_name_key: mode.ui_name_key.clone(),
            tooltip_key: mode.tooltip_key.clone(),
            override_file: mode.override_file.clone(),
            map_filter: mode.map_filter.clone(),
            random_maps_allowed: mode.random_maps_allowed,
            allies_allowed: mode.allies_allowed,
            must_ally: mode.must_ally,
        }
    }
}

/// Stable native HouseType array identity. Stock menu names are compatibility
/// constants; accepted settings may reference any allocated Country entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "LaunchCountryInput", into = "u16")]
pub struct LaunchCountry(crate::rules::ruleset::CountryIdx);

#[derive(Deserialize)]
#[serde(untagged)]
enum LaunchCountryInput {
    Index(u16),
    Legacy(String),
}

impl TryFrom<LaunchCountryInput> for LaunchCountry {
    type Error = String;
    fn try_from(input: LaunchCountryInput) -> Result<Self, Self::Error> {
        match input {
            LaunchCountryInput::Index(index) => Ok(Self(crate::rules::ruleset::CountryIdx(index))),
            LaunchCountryInput::Legacy(name) => {
                let names = [
                    "America",
                    "Korea",
                    "France",
                    "Germany",
                    "GreatBritain",
                    "Libya",
                    "Iraq",
                    "Cuba",
                    "Russia",
                    "Yuri",
                ];
                names
                    .iter()
                    .position(|candidate| *candidate == name)
                    .map(|index| Self::from_country_index(index as u32))
                    .ok_or_else(|| format!("unknown legacy launch country {name}"))
            }
        }
    }
}

impl Default for LaunchCountry {
    fn default() -> Self {
        Self::America
    }
}

impl From<LaunchCountry> for u16 {
    fn from(country: LaunchCountry) -> Self {
        country.raw_index()
    }
}

#[expect(
    non_upper_case_globals,
    reason = "preserve existing named launch constants and legacy profiles"
)]
impl LaunchCountry {
    pub const America: Self = Self(crate::rules::ruleset::CountryIdx(0));
    pub const Korea: Self = Self(crate::rules::ruleset::CountryIdx(1));
    pub const France: Self = Self(crate::rules::ruleset::CountryIdx(2));
    pub const Germany: Self = Self(crate::rules::ruleset::CountryIdx(3));
    pub const GreatBritain: Self = Self(crate::rules::ruleset::CountryIdx(4));
    pub const Libya: Self = Self(crate::rules::ruleset::CountryIdx(5));
    pub const Iraq: Self = Self(crate::rules::ruleset::CountryIdx(6));
    pub const Cuba: Self = Self(crate::rules::ruleset::CountryIdx(7));
    pub const Russia: Self = Self(crate::rules::ruleset::CountryIdx(8));
    pub const Yuri: Self = Self(crate::rules::ruleset::CountryIdx(9));
    /// The stock ten-player menu domain, not the allocated registry's bounds.
    pub const ALL: [Self; 10] = [
        Self::America,
        Self::Korea,
        Self::France,
        Self::Germany,
        Self::GreatBritain,
        Self::Libya,
        Self::Iraq,
        Self::Cuba,
        Self::Russia,
        Self::Yuri,
    ];

    pub const fn index(self) -> crate::rules::ruleset::CountryIdx {
        self.0
    }
    pub const fn raw_index(self) -> u16 {
        self.0.0
    }
    pub fn from_country_index(index: u32) -> Self {
        Self(crate::rules::ruleset::CountryIdx(
            u16::try_from(index).expect("Country index exceeds canonical registry identity space"),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum LaunchStartPosition {
    /// No start reserved; the launch assigns one (the shell's Random entry).
    Auto,
    /// Specific waypoint index.
    Position(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum LaunchTeam {
    None,
    Team(u8),
}

impl LaunchTeam {
    pub const fn from_shell_value(value: i32) -> Self {
        if value < 0 {
            Self::None
        } else {
            Self::Team(value as u8)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub enum AiDifficulty {
    #[default]
    Easy,
    Normal,
    Hard,
}

impl AiDifficulty {
    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Hard => 0,
            Self::Normal => 1,
            Self::Easy => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkirmishLaunchOptions {
    pub starting_credits: i32,
    pub unit_count: i32,
    pub tech_level: i32,
    pub game_speed: i32,
    /// Rules-owned default/global difficulty retained from
    /// `[MultiplayerDialogSettings]`. Offline AI houses carry their selected
    /// row difficulty separately; this value is not their shared authority.
    pub default_ai_difficulty: i32,
    pub short_game: bool,
    pub bases: bool,
    pub bridges_destroyable: bool,
    pub super_weapons: bool,
    pub build_off_ally: bool,
    pub crates: bool,
    pub mcv_redeploy: bool,
    pub fog_of_war: bool,
    pub shroud: bool,
    pub tiberium_grows: bool,
    pub multi_engineer: bool,
    pub harvester_truce: bool,
    pub ally_change_allowed: bool,
}

impl Default for SkirmishLaunchOptions {
    fn default() -> Self {
        let defaults = GameOptions::default();
        Self {
            starting_credits: defaults.starting_credits,
            unit_count: defaults.unit_count,
            tech_level: defaults.tech_level,
            game_speed: defaults.game_speed,
            default_ai_difficulty: defaults.ai_difficulty,
            short_game: defaults.short_game,
            bases: defaults.bases,
            bridges_destroyable: defaults.bridges_destroyable,
            super_weapons: defaults.super_weapons,
            build_off_ally: defaults.build_off_ally,
            crates: defaults.crates,
            mcv_redeploy: defaults.mcv_redeploy,
            fog_of_war: defaults.fog_of_war,
            shroud: defaults.shroud,
            tiberium_grows: defaults.tiberium_grows,
            multi_engineer: defaults.multi_engineer,
            harvester_truce: defaults.harvester_truce,
            ally_change_allowed: defaults.ally_change_allowed,
        }
    }
}

impl SkirmishLaunchOptions {
    /// Build the launch base from per-match options parsed once from
    /// `[MultiplayerDialogSettings]`. The setup dialog later overrides the
    /// values it exposes as widgets; the remaining fields — tech level and the
    /// non-widget toggles (bases, shroud, tiberium growth, …) — flow straight
    /// through to the match from this base. The rules-owned default AI
    /// difficulty is retained for the global GameOptions mirror; configured
    /// opponent difficulty is copied separately into each HouseState.
    pub fn from_game_options(options: &GameOptions) -> Self {
        Self {
            starting_credits: options.starting_credits,
            unit_count: options.unit_count,
            tech_level: options.tech_level,
            game_speed: options.game_speed,
            default_ai_difficulty: options.ai_difficulty,
            short_game: options.short_game,
            bases: options.bases,
            bridges_destroyable: options.bridges_destroyable,
            super_weapons: options.super_weapons,
            build_off_ally: options.build_off_ally,
            crates: options.crates,
            mcv_redeploy: options.mcv_redeploy,
            fog_of_war: options.fog_of_war,
            shroud: options.shroud,
            tiberium_grows: options.tiberium_grows,
            multi_engineer: options.multi_engineer,
            harvester_truce: options.harvester_truce,
            ally_change_allowed: options.ally_change_allowed,
        }
    }

    pub fn to_game_options(&self, ai_players: i32) -> GameOptions {
        GameOptions {
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
            starting_credits: self.starting_credits,
            unit_count: self.unit_count,
            tech_level: self.tech_level,
            game_speed: self.game_speed,
            ai_difficulty: self.default_ai_difficulty,
            ai_players,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkirmishLocalSlot {
    pub country: LaunchCountry,
    /// When true, `country` is a placeholder to be replaced by a random draw
    /// on the app-owned shell Scenario cursor before map loading.
    pub country_random: bool,
    pub color_index: u8,
    /// When true, `color_index` is a placeholder to be replaced by a random
    /// collision-free color draw during shell session resolution. The raw UI
    /// selection and persisted snapshot retain the Random sentinel.
    pub color_random: bool,
    pub start_position: LaunchStartPosition,
    pub team: LaunchTeam,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkirmishAiSlot {
    /// Index0..7 in native Session country/color/start/team arrays, before
    /// inactive entries are omitted. This is not a constructed House index.
    #[serde(default = "legacy_raw_ai_slot")]
    pub raw_slot_index: u8,
    pub country: LaunchCountry,
    /// When true, `country` is a placeholder to be replaced by a random draw
    /// on the app-owned shell Scenario cursor before map loading.
    pub country_random: bool,
    pub color_index: u8,
    /// See [`SkirmishLocalSlot::color_random`].
    pub color_random: bool,
    pub start_position: LaunchStartPosition,
    pub team: LaunchTeam,
    pub difficulty: AiDifficulty,
}

fn legacy_raw_ai_slot() -> u8 {
    u8::MAX
}

/// Native node+6B. Observer=-1; other values are mode-specific player roles
/// (FFA source indices, Siege defender/attacker), independent of team+63/+67.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum LaunchHumanRole {
    Observer,
    Player { group: i32 },
}

impl Default for LaunchHumanRole {
    fn default() -> Self {
        Self::Player { group: 0 }
    }
}
impl LaunchHumanRole {
    pub const fn from_native(value: i32) -> Self {
        if value == -1 {
            Self::Observer
        } else {
            Self::Player { group: value }
        }
    }
    pub const fn native_value(self) -> i32 {
        match self {
            Self::Observer => -1,
            Self::Player { group } => group,
        }
    }
    pub const fn from_observer(observer: bool) -> Self {
        if observer {
            Self::Observer
        } else {
            Self::Player { group: 0 }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "SkirmishHumanSlotInput")]
pub struct SkirmishHumanSlot {
    /// Visible A8DA90 binding, independent of source-list membership. A host
    /// kick clears this binding immediately; type10 later removes the source
    /// node. Until then it still participates in native Start/assignment logic.
    pub raw_slot_index: Option<u8>,
    pub name: String,
    pub slot: SkirmishLocalSlot,
    pub role: LaunchHumanRole,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkirmishHumanSlotInput {
    #[serde(default = "legacy_human_row")]
    raw_slot_index: Option<u8>,
    name: String,
    slot: SkirmishLocalSlot,
    #[serde(default)]
    role: Option<LaunchHumanRole>,
    #[serde(default)]
    observer: Option<bool>,
}
fn legacy_human_row() -> Option<u8> {
    Some(u8::MAX)
}

impl TryFrom<SkirmishHumanSlotInput> for SkirmishHumanSlot {
    type Error = String;
    fn try_from(input: SkirmishHumanSlotInput) -> Result<Self, Self::Error> {
        let role = match (input.role, input.observer) {
            (Some(role), None) => role,
            (None, Some(observer)) => LaunchHumanRole::from_observer(observer),
            (None, None) => LaunchHumanRole::default(),
            (Some(_), Some(_)) => {
                return Err("human role and legacy observer cannot both be supplied".into());
            }
        };
        if matches!(role, LaunchHumanRole::Player { group: -1 }) {
            return Err("native role-1 must use Observer".into());
        }
        Ok(Self {
            raw_slot_index: input.raw_slot_index,
            name: input.name,
            slot: input.slot,
            role,
        })
    }
}
impl SkirmishHumanSlot {
    pub const fn is_observer(&self) -> bool {
        matches!(self.role, LaunchHumanRole::Observer)
    }
    pub fn set_observer(&mut self, observer: bool) {
        if observer {
            self.role = LaunchHumanRole::Observer;
        } else if self.is_observer() {
            self.role = LaunchHumanRole::default();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "SkirmishLaunchSessionInput")]
pub struct SkirmishLaunchSession {
    pub mode: SkirmishLaunchMode,
    pub selected_map_file: Option<String>,
    /// Native NodeNameType source order, before CreateHouses sorts by color.
    /// A received setup retains the host's resolved order; this client's
    /// identity is an index, never inferred from a display name or House order.
    pub humans: Vec<SkirmishHumanSlot>,
    pub local_player_index: usize,
    /// Session+A8B274 declared computer count. Usually equals active rows;
    /// Cooperative5C30C0 writes it before a failing stage lookup, leaving rows.
    pub ai_player_count: i32,
    pub opponents: Vec<SkirmishAiSlot>,
    /// Native pre-compaction House constructor roster used by both pre-Fill
    /// passes. It deliberately retains inactive AI slots and observer state.
    pub pre_fill_house_roster: PreFillHouseRoster,
    pub options: SkirmishLaunchOptions,
}

/// Decode old capture/profile inputs once at the boundary. The runtime and
/// newly serialized form have only the canonical ordered human vector.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkirmishLaunchSessionInput {
    mode: SkirmishLaunchMode,
    selected_map_file: Option<String>,
    #[serde(default)]
    humans: Option<Vec<SkirmishHumanSlot>>,
    #[serde(default)]
    local_player_index: usize,
    #[serde(default)]
    player_name: Option<String>,
    #[serde(default)]
    local: Option<SkirmishLocalSlot>,
    #[serde(default)]
    ai_player_count: Option<i32>,
    opponents: Vec<SkirmishAiSlot>,
    pre_fill_house_roster: PreFillHouseRoster,
    options: SkirmishLaunchOptions,
}

impl TryFrom<SkirmishLaunchSessionInput> for SkirmishLaunchSession {
    type Error = String;

    fn try_from(input: SkirmishLaunchSessionInput) -> Result<Self, Self::Error> {
        let mut humans = match (input.humans, input.player_name, input.local) {
            (Some(humans), None, None) => humans,
            (None, Some(name), Some(slot)) if input.local_player_index == 0 => {
                vec![SkirmishHumanSlot {
                    raw_slot_index: Some(0),
                    name,
                    slot,
                    role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
                }]
            }
            _ => {
                return Err(
                    "launch requires humans or the legacy player_name/local pair, never both"
                        .into(),
                );
            }
        };
        if input.local_player_index >= humans.len() {
            return Err("launch local_player_index is outside the human roster".into());
        }
        for (index, human) in humans.iter_mut().enumerate() {
            if human.raw_slot_index == Some(u8::MAX) {
                human.raw_slot_index = Some(index as u8);
            }
        }
        let mut opponents = input.opponents;
        for (index, opponent) in opponents.iter_mut().enumerate() {
            if opponent.raw_slot_index == u8::MAX {
                opponent.raw_slot_index = (index + 1) as u8;
            }
        }
        opponents.sort_by_key(|opponent| opponent.raw_slot_index);
        if opponents
            .iter()
            .any(|opponent| opponent.raw_slot_index >= SKIRMISH_PLAYER_SLOT_COUNT as u8)
            || opponents
                .windows(2)
                .any(|pair| pair[0].raw_slot_index == pair[1].raw_slot_index)
        {
            return Err("launch AI raw slot indices must be distinct and below8".into());
        }
        Ok(Self {
            mode: input.mode,
            selected_map_file: input.selected_map_file,
            humans,
            local_player_index: input.local_player_index,
            ai_player_count: input.ai_player_count.unwrap_or(opponents.len() as i32),
            opponents,
            pre_fill_house_roster: input.pre_fill_house_roster,
            options: input.options,
        })
    }
}

/// Staged shell assignment copy used by the app's native-order close
/// transaction. Keeping the phases explicit lets snapshot Slot packing occur
/// after local country but before local colour without mutating raw controls.
pub struct ShellRandomAssignmentState {
    resolved: SkirmishLaunchSession,
    used_colors: Vec<u8>,
}

impl ShellRandomAssignmentState {
    pub fn new(session: &SkirmishLaunchSession) -> Self {
        let mut used_colors = Vec::new();
        used_colors.extend(
            session
                .humans
                .iter()
                .filter(|human| !human.is_observer() && !human.slot.color_random)
                .map(|human| human.slot.color_index),
        );
        for opponent in &session.opponents {
            if !opponent.color_random {
                used_colors.push(opponent.color_index);
            }
        }
        let mut resolved = session.clone();
        resolved
            .opponents
            .sort_by_key(|opponent| opponent.raw_slot_index);
        Self {
            resolved,
            used_colors,
        }
    }

    pub fn resolve_local_country<E>(
        &mut self,
        rng: &mut SimRng,
        draw_country: &mut impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<(), E> {
        self.resolve_human_country(self.resolved.local_player_index, rng, draw_country)
    }

    fn resolve_human_country<E>(
        &mut self,
        index: usize,
        rng: &mut SimRng,
        draw_country: &mut impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<(), E> {
        let slot = &mut self.resolved.humans[index].slot;
        if slot.country_random {
            slot.country = draw_country(RandomCountryRole::Human, rng)?;
            slot.country_random = false;
        }
        Ok(())
    }

    /// Materialize the AI assignment arrays after the local-country phase.
    /// The app close transaction starts with an empty opponent list so this
    /// write occurs at the same native phase as the raw persisted Slot pack.
    pub fn pack_ai_assignments(&mut self, opponents: &[SkirmishAiSlot]) {
        debug_assert!(self.resolved.opponents.is_empty());
        self.resolved.ai_player_count = opponents.len() as i32;
        self.resolved.opponents = opponents.to_vec();
        self.resolved
            .opponents
            .sort_by_key(|opponent| opponent.raw_slot_index);
        self.used_colors.extend(
            opponents
                .iter()
                .filter(|opponent| !opponent.color_random)
                .map(|opponent| opponent.color_index),
        );
    }

    pub fn resolve_local_color(&mut self, rng: &mut SimRng) {
        self.resolve_human_color(self.resolved.local_player_index, rng);
    }

    fn resolve_human_color(&mut self, index: usize, rng: &mut SimRng) {
        let slot = &mut self.resolved.humans[index].slot;
        if slot.color_random {
            let color = draw_collision_free_color(rng, &self.used_colors);
            slot.color_index = color;
            slot.color_random = false;
            self.used_colors.push(color);
        }
    }

    /// ProcessRandomAssignments69B8E8..69B9C5 walks human source order,
    /// before CreateHouses sorts by resolved color. Observer normalization at
    /// 69B974 consumes neither country nor color draws.
    pub fn resolve_humans<E>(
        &mut self,
        rng: &mut SimRng,
        draw_country: &mut impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<(), E> {
        for index in 0..self.resolved.humans.len() {
            let human = &mut self.resolved.humans[index];
            if human.is_observer() {
                human.slot.country = LaunchCountry::America;
                human.slot.country_random = false;
                human.slot.color_index = 8;
                human.slot.color_random = false;
                continue;
            }
            self.resolve_human_country(index, rng, draw_country)?;
            self.resolve_human_color(index, rng);
        }
        Ok(())
    }

    pub fn resolve_ai<E>(
        &mut self,
        rng: &mut SimRng,
        draw_country: &mut impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<(), E> {
        for (index, opponent) in self.resolved.opponents.iter_mut().enumerate() {
            if opponent.country_random {
                opponent.country = draw_country(RandomCountryRole::Ai { index }, rng)?;
                opponent.country_random = false;
            }
            if opponent.color_random {
                let color = draw_collision_free_color(rng, &self.used_colors);
                opponent.color_index = color;
                opponent.color_random = false;
                self.used_colors.push(color);
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> SkirmishLaunchSession {
        self.resolved.rebuild_house_roster();
        self.resolved
    }
}

impl SkirmishLaunchSession {
    /// Gather688415..42F combines source-human nonobservers with Session's
    /// declared AI count, independently of how many AI rows construct Houses.
    pub fn required_start_count(&self) -> usize {
        (self.pre_fill_house_roster.nonobserver_human_count() as i32)
            .wrapping_add(self.ai_player_count)
            .max(0) as usize
    }

    /// Rebuild the constructor cache after accepted roster/choice changes.
    /// Human identity and observer state remain authoritative in `humans`.
    pub fn rebuild_house_roster(&mut self) {
        let humans = self
            .humans
            .iter()
            .enumerate()
            .map(|(index, human)| PreFillHumanHouse {
                priority: i32::from(human.slot.color_index),
                source_order: index as u8,
                observer: human.is_observer(),
            })
            .collect();
        let ai = (0..SKIRMISH_PLAYER_SLOT_COUNT)
            .map(|index| PreFillAiHouseSlot {
                slot_index: index as u8,
                valid: self
                    .opponents
                    .iter()
                    .any(|opponent| usize::from(opponent.raw_slot_index) == index),
            })
            .collect();
        self.pre_fill_house_roster = PreFillHouseRoster::new(humans, ai);
    }

    pub fn house_roster_is_consistent(&self) -> bool {
        let mut derived = self.clone();
        derived.rebuild_house_roster();
        derived.pre_fill_house_roster == self.pre_fill_house_roster
    }

    pub fn local_player(&self) -> &SkirmishHumanSlot {
        &self.humans[self.local_player_index]
    }

    pub fn local_player_mut(&mut self) -> &mut SkirmishHumanSlot {
        &mut self.humans[self.local_player_index]
    }

    pub fn local_slot(&self) -> &SkirmishLocalSlot {
        &self.local_player().slot
    }

    pub fn local_slot_mut(&mut self) -> &mut SkirmishLocalSlot {
        &mut self.local_player_mut().slot
    }

    /// Resolve every slot left on "random country"/"random color" using the
    /// verified ordinary selected-mode Scenario-RNG order:
    /// **all humans first, then all AI slots**, and **within each slot country
    /// before color**. Each random country is one inclusive `(0, 9)` draw; each
    /// random color is one inclusive `(0, 7)` draw repeated until it does not
    /// collide with an already-assigned color (every retry is one more draw).
    /// Slots that already hold a concrete value are left untouched and consume
    /// no draw, so the stream only advances for the slots that requested random.
    ///
    /// The supplied RNG must be the app-owned frontend/process Scenario cursor,
    /// never the freshly seeded gameplay cursor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_shell_random_assignments(&self, rng: &mut SimRng) -> Self {
        self.resolve_shell_random_assignments_with(rng, |_, rng| {
            LaunchCountry::from_country_index(rng.next_range_u32_inclusive(0, 9))
        })
    }

    /// Resolve shell assignments while delegating country selection to the
    /// selected MPModes authority. Cooperative uses this hook for eligibility
    /// retries; colours remain here so every rejected country candidate shifts
    /// the following colour draw.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resolve_shell_random_assignments_with(
        &self,
        rng: &mut SimRng,
        mut draw_country: impl FnMut(RandomCountryRole, &mut SimRng) -> LaunchCountry,
    ) -> Self {
        match self.try_resolve_shell_random_assignments_with(rng, |role, rng| {
            Ok::<_, std::convert::Infallible>(draw_country(role, rng))
        }) {
            Ok(resolved) => resolved,
            Err(never) => match never {},
        }
    }

    /// Fallible form of shell assignment resolution for selected-mode country
    /// authorities whose data can be malformed. An error stops the transaction
    /// immediately, retaining draws already consumed but making no later
    /// country or colour calls.
    #[cfg(any(test, feature = "test-support"))]
    pub fn try_resolve_shell_random_assignments_with<E>(
        &self,
        rng: &mut SimRng,
        mut draw_country: impl FnMut(RandomCountryRole, &mut SimRng) -> Result<LaunchCountry, E>,
    ) -> Result<Self, E> {
        let mut state = ShellRandomAssignmentState::new(self);

        // Phase A: source-order humans, COUNTRY then COLOR per node.
        state.resolve_humans(rng, &mut draw_country)?;

        // Phase B: AI slots in order — each COUNTRY then COLOR.
        state.resolve_ai(rng, &mut draw_country)?;

        Ok(state.finish())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomCountryRole {
    Human,
    Ai { index: usize },
}

/// Draw an unused shell colour in `0..=7`.
///
/// Each collision retry advances the supplied Scenario stream once. More
/// Random colour slots than free colours intentionally has no exit, matching
/// the native malformed-state behavior.
fn draw_collision_free_color(rng: &mut SimRng, used: &[u8]) -> u8 {
    loop {
        let color = rng.next_range_u32_inclusive(0, 7) as u8;
        if !used.contains(&color) {
            return color;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchValidationError {
    NoSelectedMap,
    NoSelectedMode {
        mode_id: i32,
    },
    NoEnabledOpponent,
    MapCapacityExceeded {
        capacity: i32,
        requested_players: usize,
    },
    SameExplicitTeam {
        team: u8,
    },
    InvalidColorIndex {
        slot: usize,
        color_index: usize,
    },
    InvalidStartPosition {
        slot: usize,
        position: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_humans_observers_and_ai_match_executed_native_assignments() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/lan_lobby.json",
        ))
        .unwrap();
        for case in corpus["assignments"]
            .as_array()
            .expect("native assignment corpus")
        {
            let mut session = random_test_session();
            session.humans = case["initial"]["humans"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let country = row[0].as_i64().unwrap();
                    let side_ex = row[1].as_i64().unwrap();
                    SkirmishHumanSlot {
                        raw_slot_index: Some(index as u8),
                        name: format!("Human{index}"),
                        role: crate::skirmish_launch::LaunchHumanRole::from_observer(
                            country == -3 || side_ex == -3,
                        ),
                        slot: SkirmishLocalSlot {
                            country: LaunchCountry::from_country_index(country.max(0) as u32),
                            country_random: side_ex == -2,
                            color_index: row[2].as_i64().unwrap().max(0) as u8,
                            color_random: row[3].as_i64().unwrap() == -2,
                            start_position: LaunchStartPosition::Auto,
                            team: LaunchTeam::None,
                        },
                    }
                })
                .collect();
            session.local_player_index = session.humans.len() - 1;
            session.opponents = case["active_ai_slots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|raw| {
                    let index = raw.as_u64().unwrap() as usize;
                    let row = &case["initial"]["ai"][index];
                    SkirmishAiSlot {
                        raw_slot_index: index as u8,
                        country: LaunchCountry::from_country_index(
                            row[0].as_i64().unwrap().max(0) as u32
                        ),
                        country_random: row[0].as_i64().unwrap() == -2,
                        color_index: row[1].as_i64().unwrap().max(0) as u8,
                        color_random: row[1].as_i64().unwrap() == -2,
                        start_position: LaunchStartPosition::Auto,
                        team: LaunchTeam::None,
                        difficulty: AiDifficulty::Easy,
                    }
                })
                .collect();
            let mut rng = SimRng::from_native_state_hex_for_test(
                case["initial"]["rng_hex"].as_str().unwrap(),
            );
            let resolved = session.resolve_shell_random_assignments(&mut rng);
            for (index, human) in resolved.humans.iter().enumerate() {
                let row = &case["result"]["humans"][index];
                // Native keeps country-3 until CreateHouses687F2D normalizes
                // observer country0. The typed launch keeps this valid country
                // and the independent canonical observer bit together.
                assert_eq!(
                    human.slot.country,
                    LaunchCountry::from_country_index(row[0].as_i64().unwrap().max(0) as u32),
                    "{} human{index}",
                    case["name"]
                );
                assert_eq!(human.slot.color_index, row[2].as_u64().unwrap() as u8);
                assert!(!human.slot.country_random && !human.slot.color_random);
            }
            for ai in &resolved.opponents {
                let row = &case["result"]["ai"][usize::from(ai.raw_slot_index)];
                assert_eq!(
                    ai.country,
                    LaunchCountry::from_country_index(row[0].as_u64().unwrap() as u32)
                );
                assert_eq!(ai.color_index, row[1].as_u64().unwrap() as u8);
            }
            assert_eq!(
                rng.native_state_hex(),
                case["result"]["rng_hex"].as_str().unwrap(),
                "{} full stream state",
                case["name"]
            );
        }
    }

    #[test]
    fn declared_ai_count_survives_partial_mode_effect_independently_of_constructed_rows() {
        let mut session = random_test_session();
        session.rebuild_house_roster();
        session.ai_player_count = 5;
        assert_eq!(session.required_start_count(), 6);
        assert_eq!(session.pre_fill_house_roster.created_house_count(), 5);
        assert!(session.house_roster_is_consistent());
        let restored: SkirmishLaunchSession =
            serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
        assert_eq!(restored.ai_player_count, 5);
        assert_eq!(restored.opponents.len(), 2);
    }

    #[test]
    fn catalog_mode_class_survives_custom_filename_and_legacy_unknown_mode_is_rejected() {
        let mut mode = random_test_session().mode;
        mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
        mode.id = 77;
        mode.override_file = "custom.ini".into();
        let mut value = serde_json::to_value(&mode).unwrap();
        assert_eq!(
            serde_json::from_value::<SkirmishLaunchMode>(value.clone()).unwrap(),
            mode
        );
        value.as_object_mut().unwrap().remove("class");
        assert!(serde_json::from_value::<SkirmishLaunchMode>(value).is_err());
    }

    #[test]
    fn legacy_launch_deserializes_once_and_serializes_only_canonical_humans() {
        let profile: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/map_observation.command-bar.example.json",
        ))
        .unwrap();
        let session: SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        assert_eq!(session.humans.len(), 1);
        assert_eq!(session.local_player_index, 0);
        assert_eq!(session.local_player().name, "Player");
        let canonical = serde_json::to_value(&session).unwrap();
        assert!(canonical.get("local").is_none());
        assert!(canonical.get("player_name").is_none());
        assert_eq!(
            serde_json::from_value::<SkirmishLaunchSession>(canonical.clone()).unwrap(),
            session
        );
        let mut competing = canonical;
        competing["player_name"] = "Other".into();
        assert!(serde_json::from_value::<SkirmishLaunchSession>(competing).is_err());
    }

    #[test]
    fn launch_options_preserve_build_off_ally_default() {
        let options = SkirmishLaunchOptions::default();
        let game_options = options.to_game_options(3);

        assert!(game_options.build_off_ally);
        assert_eq!(game_options.ai_players, 3);
        assert_eq!(game_options.ai_difficulty, 0);
    }

    #[test]
    fn country_identity_retains_allocated_indices_and_reads_legacy_names() {
        let custom = LaunchCountry::from_country_index(14);
        assert_eq!(custom.raw_index(), 14);
        assert_eq!(serde_json::to_value(custom).unwrap(), serde_json::json!(14));
        assert_eq!(serde_json::from_str::<LaunchCountry>("14").unwrap(), custom);
        assert_eq!(
            serde_json::from_str::<LaunchCountry>("\"Yuri\"").unwrap(),
            LaunchCountry::Yuri
        );
        assert!(serde_json::from_str::<LaunchCountry>("65536").is_err());
    }

    #[test]
    fn from_game_options_carries_non_widget_fields_to_the_match() {
        // The launch base is the path by which parsed-INI fields the setup
        // dialog does not expose (tech level, bases, shroud, …) reach the
        // launched match. Build a base from modded options and confirm those
        // fields survive the round-trip through to_game_options.
        let parsed = GameOptions {
            tech_level: 3,
            bases: false,
            shroud: false,
            tiberium_grows: false,
            harvester_truce: true,
            multi_engineer: true,
            bridges_destroyable: false,
            ally_change_allowed: false,
            fog_of_war: true,
            ai_difficulty: 2,
            ..Default::default()
        };

        let base = SkirmishLaunchOptions::from_game_options(&parsed);
        let launched = base.to_game_options(2);

        assert_eq!(launched.tech_level, 3);
        assert!(!launched.bases);
        assert!(!launched.shroud);
        assert!(!launched.tiberium_grows);
        assert!(launched.harvester_truce);
        assert!(launched.multi_engineer);
        assert!(!launched.bridges_destroyable);
        assert!(!launched.ally_change_allowed);
        assert!(launched.fog_of_war);
        // The global/default difficulty remains rules-owned; individual AI
        // rows are copied to HouseState during launch population.
        assert_eq!(launched.ai_players, 2);
        assert_eq!(launched.ai_difficulty, 2);
    }

    #[test]
    fn launch_mode_carries_selected_mpmode_data() {
        let mode = SkirmishGameMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 9,
            ui_name_key: "GUI:TeamGame".to_string(),
            tooltip_key: "STT:ModeTeamGame".to_string(),
            override_file: "MPTeamMD.ini".to_string(),
            map_filter: "teamgame".to_string(),
            random_maps_allowed: false,
            allies_allowed: true,
            must_ally: true,
        };

        let launch_mode = SkirmishLaunchMode::from_game_mode(&mode);

        assert_eq!(launch_mode.id, 9);
        assert_eq!(launch_mode.ui_name_key, "GUI:TeamGame");
        assert_eq!(launch_mode.override_file, "MPTeamMD.ini");
        assert!(launch_mode.allies_allowed);
        assert!(launch_mode.must_ally);
    }

    #[test]
    fn shell_team_values_use_negative_none_and_zero_based_teams() {
        assert_eq!(LaunchTeam::from_shell_value(-2), LaunchTeam::None);
        assert_eq!(LaunchTeam::from_shell_value(-1), LaunchTeam::None);
        assert_eq!(LaunchTeam::from_shell_value(0), LaunchTeam::Team(0));
        assert_eq!(LaunchTeam::from_shell_value(3), LaunchTeam::Team(3));
    }

    #[test]
    fn pre_fill_roster_orders_signed_humans_and_raw_ai_slots() {
        let roster = PreFillHouseRoster::new(
            vec![
                PreFillHumanHouse {
                    priority: 4,
                    source_order: 2,
                    observer: false,
                },
                PreFillHumanHouse {
                    priority: -1,
                    source_order: 1,
                    observer: true,
                },
                PreFillHumanHouse {
                    priority: -1,
                    source_order: 0,
                    observer: false,
                },
            ],
            vec![
                PreFillAiHouseSlot {
                    slot_index: 6,
                    valid: true,
                },
                PreFillAiHouseSlot {
                    slot_index: 1,
                    valid: false,
                },
                PreFillAiHouseSlot {
                    slot_index: 3,
                    valid: true,
                },
            ],
        );

        assert_eq!(
            roster
                .human_nodes()
                .iter()
                .map(|node| (node.priority, node.source_order))
                .collect::<Vec<_>>(),
            vec![(-1, 0), (-1, 1), (4, 2)]
        );
        assert_eq!(
            roster
                .ai_slots()
                .iter()
                .map(|slot| (slot.slot_index, slot.valid))
                .collect::<Vec<_>>(),
            vec![(1, false), (3, true), (6, true)]
        );
        assert_eq!(roster.nonobserver_human_count(), 2);
        assert_eq!(roster.nonobserver_house_count(), 4);
        assert_eq!(roster.created_house_count(), 7);
    }

    fn random_test_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                class: crate::skirmish_modes::MpModeClass::Battle,
                id: 1,
                ui_name_key: "GUI:Battle".to_string(),
                tooltip_key: "STT:ModeBattle".to_string(),
                override_file: "MPBattleMD.ini".to_string(),
                map_filter: "standard".to_string(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            selected_map_file: Some("test.mmx".to_string()),
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".to_string(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: true,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 2,
            opponents: vec![
                SkirmishAiSlot {
                    raw_slot_index: 1,
                    country: LaunchCountry::Russia,
                    country_random: true,
                    color_index: 1,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                    difficulty: AiDifficulty::Easy,
                },
                SkirmishAiSlot {
                    raw_slot_index: 2,
                    country: LaunchCountry::Cuba,
                    country_random: false,
                    color_index: 2,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                    difficulty: AiDifficulty::Easy,
                },
            ],
            pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(2),
            options: SkirmishLaunchOptions::default(),
        }
    }

    #[test]
    fn legacy_observer_input_converts_to_role_without_a_second_observer_authority() {
        let mut value = serde_json::to_value(&random_test_session().humans[0]).unwrap();
        value.as_object_mut().unwrap().remove("role");
        value["observer"] = serde_json::json!(true);
        let observer: SkirmishHumanSlot = serde_json::from_value(value.clone()).unwrap();
        assert!(observer.is_observer());
        assert_eq!(observer.role.native_value(), -1);
        let serialized = serde_json::to_value(&observer).unwrap();
        assert!(serialized.get("observer").is_none());
        value["role"] = serde_json::json!({"Player":{"group":2}});
        assert!(serde_json::from_value::<SkirmishHumanSlot>(value.clone()).is_err());
        value.as_object_mut().unwrap().remove("observer");
        let mut player: SkirmishHumanSlot = serde_json::from_value(value).unwrap();
        player.set_observer(false);
        assert_eq!(player.role.native_value(), 2);
        player.set_observer(true);
        player.set_observer(false);
        assert_eq!(player.role.native_value(), 0);
    }

    #[test]
    fn shell_random_assignments_are_deterministic_for_a_seed() {
        let session = random_test_session();
        let mut rng_a = SimRng::new(0xC0FFEE);
        let mut rng_b = SimRng::new(0xC0FFEE);

        let first = session.resolve_shell_random_assignments(&mut rng_a);
        let second = session.resolve_shell_random_assignments(&mut rng_b);

        assert_eq!(first, second, "same seed must yield the same assignment");
        assert!(!first.local_slot().country_random);
        assert!(!first.opponents[0].country_random);
    }

    #[test]
    fn shell_random_assignments_only_draw_for_random_slots_in_order() {
        let session = random_test_session();

        // Two random slots (local + opponent 0); opponent 1 is concrete and must
        // not consume a draw or change. The draw order is local first, then AI,
        // so resolving by hand in that order must reproduce the same result.
        let mut rng = SimRng::new(7);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        let mut expected_rng = SimRng::new(7);
        let expected_local =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));
        let expected_ai0 =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));

        assert_eq!(resolved.local_slot().country, expected_local);
        assert_eq!(resolved.opponents[0].country, expected_ai0);
        // The concrete slot is untouched and the stream is now exhausted of the
        // two expected draws — both RNGs must be in the same state.
        assert_eq!(resolved.opponents[1].country, LaunchCountry::Cuba);
        assert!(!resolved.opponents[1].country_random);
        assert_eq!(rng.state(), expected_rng.state());
    }

    #[test]
    fn shell_random_assignments_leave_concrete_session_untouched() {
        let mut session = random_test_session();
        session.local_slot_mut().country_random = false;
        session.opponents[0].country_random = false;

        let before = SimRng::new(42).state();
        let mut rng = SimRng::new(42);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        assert_eq!(resolved, session, "no random slots means no change");
        assert_eq!(rng.state(), before, "no random slots means no draws");
    }

    #[test]
    fn shell_random_assignments_draw_country_then_color_humans_then_ai() {
        // local + ai0 random country AND random color; ai1 concrete (color 2).
        let mut session = random_test_session();
        session.local_slot_mut().color_random = true;
        session.opponents[0].color_random = true;
        assert!(!session.opponents[1].color_random);

        let mut rng = SimRng::new(7);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        // Replay the Rust compatibility order by hand: ALL humans (country then
        // color), THEN all AI (country then color). The used-color set seeds
        // with concrete colors first (here ai1 = 2), then accumulates as colors
        // are assigned.
        let mut expected_rng = SimRng::new(7);
        let mut used: Vec<u8> = vec![session.opponents[1].color_index];
        let exp_local_country =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));
        let exp_local_color = draw_collision_free_color(&mut expected_rng, &used);
        used.push(exp_local_color);
        let exp_ai0_country =
            LaunchCountry::from_country_index(expected_rng.next_range_u32_inclusive(0, 9));
        let exp_ai0_color = draw_collision_free_color(&mut expected_rng, &used);

        assert_eq!(resolved.local_slot().country, exp_local_country);
        assert_eq!(resolved.local_slot().color_index, exp_local_color);
        assert_eq!(resolved.opponents[0].country, exp_ai0_country);
        assert_eq!(resolved.opponents[0].color_index, exp_ai0_color);
        assert_eq!(
            resolved.opponents[1].color_index, 2,
            "concrete AI color untouched"
        );
        assert!(!resolved.local_slot().color_random && !resolved.opponents[0].color_random);
        // Identical stream state proves this Rust compatibility helper used this
        // order and count (country before color, humans before AI, collision
        // retries included).
        assert_eq!(rng.state(), expected_rng.state());
    }

    #[test]
    fn shell_random_assignments_color_collision_forces_redraw() {
        // Pre-occupy the color the seed would draw first, so the resolver must
        // redraw — proving each collision costs an extra scenario-cursor draw.
        let seed = 12345;
        let first_color = {
            let mut r = SimRng::new(seed);
            r.next_range_u32_inclusive(0, 7) as u8
        };

        let mut session = random_test_session();
        session.local_slot_mut().country_random = false;
        session.local_slot_mut().color_random = false;
        session.local_slot_mut().color_index = first_color; // occupy the colliding color
        session.opponents.truncate(1);
        session.opponents[0].country_random = false;
        session.opponents[0].color_random = true;

        let mut rng = SimRng::new(seed);
        let resolved = session.resolve_shell_random_assignments(&mut rng);

        assert_ne!(
            resolved.opponents[0].color_index, first_color,
            "assigned color must avoid the occupied one"
        );
        // The colliding first draw plus at least one retry => more than one draw.
        let mut one_draw = SimRng::new(seed);
        one_draw.next_range_u32_inclusive(0, 7);
        assert_ne!(
            rng.state(),
            one_draw.state(),
            "a color collision must cost an extra draw"
        );
    }

    #[test]
    fn fallible_country_authority_stops_before_later_assignment_draws() {
        let mut session = random_test_session();
        session.local_slot_mut().color_random = true;
        session.opponents[0].color_random = true;
        let seed = 0x4567;
        let mut rng = SimRng::new(seed);
        let mut roles = Vec::new();

        let result = session.try_resolve_shell_random_assignments_with(
            &mut rng,
            |role, _| -> Result<LaunchCountry, &'static str> {
                roles.push(role);
                match role {
                    RandomCountryRole::Human => Ok(LaunchCountry::America),
                    RandomCountryRole::Ai { .. } => Err("invalid Cooperative country list"),
                }
            },
        );

        assert_eq!(result, Err("invalid Cooperative country list"));
        assert_eq!(
            roles,
            vec![RandomCountryRole::Human, RandomCountryRole::Ai { index: 0 }]
        );
        let mut expected_rng = SimRng::new(seed);
        let _ = draw_collision_free_color(&mut expected_rng, &[1, 2]);
        assert_eq!(rng.state(), expected_rng.state());
    }
}
